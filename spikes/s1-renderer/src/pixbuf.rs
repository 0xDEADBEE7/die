//! Pixel buffer (scope §5.2). The producer owns one persistent CPU buffer.
//! `present(damage)` copies the damaged rectangle into a ring of shared
//! MTLBuffers on the producer's thread; the main thread only encodes a blit.
//! If the main thread hasn't consumed the previous frame, it is superseded
//! and its damage merged; if the ring is full, the producer waits (never main).

use crate::gpu::{PixBlit, SendBuf};
use crate::stats::{is_main_thread, now_s};
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLBuffer, MTLDevice, MTLResourceOptions};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub const RING: usize = 3;

struct Pending { slot: usize, rect: [u32; 4], frame: u64 }
struct State { free: Vec<usize>, pending: Option<Pending>, size: (u32, u32), resize: Option<(u32, u32, f64)> }

pub struct Shared {
    slots: Vec<Arc<SendBuf>>,
    st: Mutex<State>,
    cv: Condvar,
    pub produced: AtomicU64,
    pub superseded: AtomicU64,
    pub ring_waits: AtomicU64,
    pub main_memcpy_bytes: AtomicU64,
    pub producer_memcpy_bytes: AtomicU64,
    pub copy_ms: Mutex<Vec<f64>>,
    pub stop: AtomicBool,
}

fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
    let (x1, y1) = ((a[0] + a[2]).max(b[0] + b[2]), (a[1] + a[3]).max(b[1] + b[3]));
    [x0, y0, x1 - x0, y1 - y0]
}

impl Shared {
    /// Slots are sized for `max_bytes` so a resize never reallocates the ring.
    pub fn new(device: &ProtocolObject<dyn MTLDevice>, w: u32, h: u32, max_bytes: usize) -> Arc<Shared> {
        let slots = (0..RING).map(|_| Arc::new(SendBuf(device.newBufferWithLength_options(max_bytes, MTLResourceOptions::StorageModeShared).unwrap()))).collect();
        Arc::new(Shared { slots, st: Mutex::new(State { free: (0..RING).collect(), pending: None, size: (w, h), resize: None }), cv: Condvar::new(),
            produced: AtomicU64::new(0), superseded: AtomicU64::new(0), ring_waits: AtomicU64::new(0), main_memcpy_bytes: AtomicU64::new(0),
            producer_memcpy_bytes: AtomicU64::new(0), copy_ms: Mutex::new(vec![]), stop: AtomicBool::new(false) })
    }

    /// Called on the producer thread. `buf` is the producer's full frame (stride w*4).
    pub fn present(&self, buf: &[u8], w: u32, damage: [u32; 4], frame: u64, wake: &dyn Fn()) {
        let mut st = self.st.lock().unwrap();
        let mut rect = damage;
        if let Some(p) = st.pending.take() {
            self.superseded.fetch_add(1, Ordering::Relaxed);
            rect = union(rect, p.rect);
            st.free.push(p.slot);
        }
        while st.free.is_empty() && !self.stop.load(Ordering::Relaxed) {
            self.ring_waits.fetch_add(1, Ordering::Relaxed);
            st = self.cv.wait_timeout(st, std::time::Duration::from_millis(100)).unwrap().0;
        }
        let Some(slot) = st.free.pop() else { return };
        drop(st);
        let t = now_s();
        let dst = self.slots[slot].0.contents().as_ptr() as *mut u8;
        let [x, y, rw, rh] = rect;
        for row in 0..rh as usize {
            let src = ((y as usize + row) * w as usize + x as usize) * 4;
            unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr().add(src), dst.add(row * rw as usize * 4), rw as usize * 4) };
        }
        let bytes = (rw * rh * 4) as u64;
        if is_main_thread() { self.main_memcpy_bytes.fetch_add(bytes, Ordering::Relaxed); } else { self.producer_memcpy_bytes.fetch_add(bytes, Ordering::Relaxed); }
        self.copy_ms.lock().unwrap().push((now_s() - t) * 1e3);
        self.produced.fetch_add(1, Ordering::Relaxed);
        self.st.lock().unwrap().pending = Some(Pending { slot, rect, frame });
        wake();
    }

    /// Main thread: the newest frame, as a blit for the next command buffer.
    /// Uses try_lock: the main thread never waits on the producer; if the lock
    /// is busy it simply gets the frame on the next wakeup.
    pub fn take(self: &Arc<Self>) -> Option<PixBlit> {
        let p = self.st.try_lock().ok()?.pending.take()?;
        let me = self.clone();
        let slot = p.slot;
        Some(PixBlit { buf: self.slots[slot].clone(), rect: p.rect, frame: p.frame, release: Box::new(move || { me.st.lock().unwrap().free.push(slot); me.cv.notify_one(); }) })
    }

    /// Main thread tells the producer its new physical size and scale.
    pub fn resize(&self, w: u32, h: u32, scale: f64) {
        let mut st = self.st.lock().unwrap();
        if let Some(p) = st.pending.take() { st.free.push(p.slot); }
        st.resize = Some((w, h, scale));
    }
    pub fn size(&self) -> (u32, u32) { self.st.lock().unwrap().size }
    pub fn has_pending(&self) -> bool { self.st.try_lock().map(|s| s.pending.is_some()).unwrap_or(true) }
}

/// A 60 Hz producer drawing an animated pattern with full-frame damage.
pub fn spawn(shared: Arc<Shared>, fps: f64, wake: Arc<dyn Fn() + Send + Sync>) -> std::thread::JoinHandle<Vec<f64>> {
    std::thread::Builder::new().name("pixbuf-producer".into()).spawn(move || {
        let (mut w, mut h) = shared.size();
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let mut frame = 0u64;
        let period = 1.0 / fps;
        let mut next = now_s();
        let mut draw_ms = vec![];
        while !shared.stop.load(Ordering::Relaxed) {
            let resize = shared.st.lock().unwrap().resize.take();
            if let Some((nw, nh, _scale)) = resize {
                shared.st.lock().unwrap().size = (nw, nh);
                (w, h) = (nw, nh);
                buf = vec![0u8; (w * h * 4) as usize];
            }
            frame += 1;
            let t = now_s();
            let f = frame as u32;
            for y in 0..h {
                let row = &mut buf[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
                let yy = y.wrapping_add(f * 3);
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let xx = (x as u32).wrapping_add(f * 5);
                    px[0] = ((xx ^ yy) & 0xff) as u8;
                    px[1] = ((xx >> 1).wrapping_add(yy >> 2) & 0xff) as u8;
                    px[2] = (f.wrapping_mul(2) & 0xff) as u8;
                    px[3] = 255;
                }
            }
            draw_ms.push((now_s() - t) * 1e3);
            shared.present(&buf, w, [0, 0, w, h], frame, &*wake);
            next += period;
            let now = now_s();
            if next > now { std::thread::sleep(std::time::Duration::from_secs_f64(next - now)); } else { next = now; }
        }
        draw_ms
    }).unwrap()
}
