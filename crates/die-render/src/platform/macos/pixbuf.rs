//! The host's pixel buffers (contract §11, scope §5.2): S1's staging ring, ported. For each
//! buffer a producer made there's one RGBA8Unorm texture the frames sample, a ring of `RING`
//! shared MTLBuffers sized to the buffer's largest size so far, and at most one frame waiting
//! in a slot. `present` copies the damaged rows into a free slot on the producer's thread,
//! superseding a frame the main thread hasn't taken yet and merging its damage, and waits when
//! the ring is full. The main thread only tries the locks (`Shadow::take`), records a blit of
//! each waiting slot into its texture, and the slot goes back to its ring from the command
//! buffer's completed handler, on Metal's thread, so the main thread never waits on a lock a
//! producer can hold (S1's deadlock, docs/records/s1-renderer.md §4.6). Two exceptions, neither
//! a cycle: a frame that fails to encode drops its blits on the main thread, which waits for
//! `st`, held by a producer only across a few lines or a Metal allocation (`resize`); and a
//! producer on the main thread itself (the whiteboard rasterizes in `event()`) waits like any
//! producer when the ring's three slots are all in flight, which `present`'s own wait for the
//! GPU makes rare. Two threads presenting one buffer at once are serialized by `presenting`, so
//! the second supersedes the first and no slot is lost between the copy and the hand-off.

use super::metal::texture;
use crate::frame::PixelStats;
use crate::window::Pending;
use block2::RcBlock;
use die_core::{PixelBufferId, PixelRect, PixelSink};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLBuffer, MTLCommandBuffer, MTLCreateSystemDefaultDevice, MTLDevice,
    MTLOrigin, MTLPixelFormat, MTLResourceOptions, MTLSize, MTLStorageMode, MTLTexture,
    MTLTextureUsage,
};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// Slots in a buffer's ring: one the GPU may be reading, one waiting, one free for the
/// producer to fill, which S1 measured 0 waits at 60 Hz with.
pub const RING: usize = 3;

/// A staging buffer, filled on a producer's thread and read by the GPU.
struct Slot(Retained<ProtocolObject<dyn MTLBuffer>>);

// SAFETY: Metal's resources are thread-safe (Metal Programming Guide, "Thread Safety"): any
// thread may make, fill and release a buffer. What isn't, the command buffers and encoders,
// stays on the main thread.
unsafe impl Send for Slot {}

/// A buffer's texture, made on its producer's thread and sampled by the frames.
struct Tex(Retained<ProtocolObject<dyn MTLTexture>>);

// SAFETY: as `Slot`: a texture is a Metal resource, which any thread may make and release.
unsafe impl Send for Tex {}

/// A frame a producer presented, waiting in a slot for the main thread.
struct Waiting {
    slot: usize,
    damage: PixelRect,
}

/// One buffer's ring and texture, under its lock, which the main thread only tries.
struct State {
    /// What the frames sample, the buffer's size: None once destroyed, or if Metal had none.
    texture: Option<Tex>,
    /// A blit has gone into the texture since it was made; before that it shows nothing.
    shown: bool,
    /// The ring, each slot `bytes` long: the buffer's largest size so far, so a resize within
    /// it makes nothing. Empty once destroyed, or if Metal had no memory for it.
    slots: Vec<Slot>,
    bytes: usize,
    free: Vec<usize>,
    waiting: Option<Waiting>,
}

/// A buffer: its state, the signal that a slot came free, for a producer waiting on one, and
/// the lock a present holds from its first look at the ring to its hand-off, so that two
/// presenters of one buffer take turns. Producers' only; the main thread never touches it.
struct Buffer {
    st: Mutex<State>,
    freed: Condvar,
    presenting: Mutex<()>,
}

/// A lock, poisoned or not: a panic on the other side leaves the ring usable.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct Counts {
    presents: AtomicU64,
    superseded: AtomicU64,
    ring_waits: AtomicU64,
    producer_bytes: AtomicU64,
    main_bytes: AtomicU64,
    blits: AtomicU64,
    blit_bytes: AtomicU64,
}

/// The host's pixel sink (`PixelSink`): every buffer's ring and texture, the flag a present
/// raises, and what it has counted.
pub struct Pixels {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    pending: Pending,
    buffers: Mutex<HashMap<PixelBufferId, Arc<Buffer>>>,
    counts: Counts,
}

impl Pixels {
    /// The sink, on the system's GPU, or None on a Mac without Metal, which `ready` says first.
    pub fn new(pending: Pending) -> Option<Arc<Pixels>> {
        Some(Arc::new(Pixels {
            device: MTLCreateSystemDefaultDevice()?,
            pending,
            buffers: Mutex::default(),
            counts: Counts::default(),
        }))
    }

    /// What it has done so far.
    pub fn stats(&self) -> PixelStats {
        let c = &self.counts;
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
        PixelStats {
            presents: n(&c.presents),
            superseded: n(&c.superseded),
            ring_waits: n(&c.ring_waits),
            producer_bytes: n(&c.producer_bytes),
            main_bytes: n(&c.main_bytes),
            blits: n(&c.blits),
            blit_bytes: n(&c.blit_bytes),
        }
    }

    fn buffer(&self, id: PixelBufferId) -> Option<Arc<Buffer>> {
        lock(&self.buffers).get(&id).cloned()
    }

    /// `RING` slots of `bytes` each, or none if Metal has no memory for them: then a present
    /// stages nothing, rather than wait for a slot that never comes.
    fn ring(&self, bytes: usize) -> Vec<Slot> {
        let shared = MTLResourceOptions::StorageModeShared;
        let slot = |_| {
            Some(Slot(
                self.device.newBufferWithLength_options(bytes, shared)?,
            ))
        };
        (0..RING)
            .map(slot)
            .collect::<Option<_>>()
            .unwrap_or_default()
    }

    /// The texture a buffer this size is sampled from: GPU memory, which only the blits write.
    fn texture(&self, size: (u32, u32)) -> Option<Tex> {
        let (format, usage) = (MTLPixelFormat::RGBA8Unorm, MTLTextureUsage::ShaderRead);
        let made = texture(&self.device, size, format, usage, MTLStorageMode::Private);
        made.ok().map(Tex)
    }
}

/// Copy `damage`'s rows of `pixels` (`width * 4` bytes a row) into `slot`, tightly, at
/// `damage.width * 4` bytes a row. The bytes copied: 0, and nothing copied, if the damage reaches
/// past `pixels` or the slot's `room`, which the trait's caller never lets it.
fn copy_rows(pixels: &[u8], width: u32, damage: PixelRect, slot: &Slot, room: usize) -> usize {
    let (row, stride) = (damage.width as usize * 4, width as usize * 4);
    let (x, y, rows) = (
        damage.x as usize * 4,
        damage.y as usize,
        damage.height as usize,
    );
    if row * rows > room || x + row > stride || (y + rows) * stride > pixels.len() {
        return 0;
    }
    let to = slot.0.contents().cast::<u8>();
    for r in 0..rows {
        let src = &pixels[(y + r) * stride + x..][..row];
        let dst = to.as_ptr().wrapping_add(r * row);
        // SAFETY: the slot holds `room` bytes and `(r + 1) * row <= rows * row <= room`, checked
        // above; `src` is `row` bytes Rust owns, so they can't overlap; and nothing else writes
        // this slot, taken from the free list for this copy alone.
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), dst, row) };
    }
    row * rows
}

impl PixelSink for Pixels {
    fn create(&self, id: PixelBufferId, width: u32, height: u32) {
        let bytes = width as usize * height as usize * 4;
        let slots = self.ring(bytes);
        let st = State {
            texture: self.texture((width, height)),
            shown: false,
            free: (0..slots.len()).collect(),
            slots,
            bytes,
            waiting: None,
        };
        let buffer = Buffer {
            st: Mutex::new(st),
            freed: Condvar::new(),
            presenting: Mutex::new(()),
        };
        lock(&self.buffers).insert(id, Arc::new(buffer));
    }

    /// On the producer's thread: a free slot, waiting for one if the ring is full, the rows
    /// copied with the lock dropped, then the frame left waiting and the loop's flag raised.
    fn present(&self, id: PixelBufferId, pixels: &[u8], width: u32, mut damage: PixelRect) {
        let Some(b) = self.buffer(id) else { return };
        let _turn = lock(&b.presenting);
        let mut st = lock(&b.st);
        if let Some(w) = st.waiting.take() {
            damage = damage.union(w.damage);
            st.free.push(w.slot);
            self.counts.superseded.fetch_add(1, Ordering::Relaxed);
        }
        while st.free.is_empty() && !st.slots.is_empty() {
            self.counts.ring_waits.fetch_add(1, Ordering::Relaxed);
            st = b.freed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        let Some(slot) = st.free.pop() else { return };
        let (to, room) = (Slot(st.slots[slot].0.clone()), st.bytes);
        drop(st);
        let bytes = copy_rows(pixels, width, damage, &to, room) as u64;
        let main = MainThreadMarker::new().is_some();
        let counted = if main {
            &self.counts.main_bytes
        } else {
            &self.counts.producer_bytes
        };
        counted.fetch_add(bytes, Ordering::Relaxed);
        self.counts.presents.fetch_add(1, Ordering::Relaxed);
        let mut st = lock(&b.st);
        let gone = slot >= st.slots.len();
        if bytes == 0 && !gone {
            st.free.push(slot);
        }
        if bytes == 0 || gone {
            return;
        }
        st.waiting = Some(Waiting { slot, damage });
        self.pending.set();
    }

    /// A new texture, shown by nothing until a blit; a frame waiting at the old size is dropped;
    /// and a bigger ring if the size outgrew it, with a slot the GPU is reading kept out of it
    /// until it's given back.
    fn resize(&self, id: PixelBufferId, width: u32, height: u32) {
        let Some(b) = self.buffer(id) else { return };
        let mut st = lock(&b.st);
        if let Some(w) = st.waiting.take() {
            st.free.push(w.slot);
        }
        st.texture = self.texture((width, height));
        st.shown = false;
        let bytes = width as usize * height as usize * 4;
        if bytes > st.bytes {
            let out: Vec<usize> = (0..st.slots.len())
                .filter(|i| !st.free.contains(i))
                .collect();
            st.slots = self.ring(bytes);
            st.bytes = bytes;
            st.free = (0..st.slots.len()).filter(|i| !out.contains(i)).collect();
        }
    }

    /// The texture and the ring go; a producer waiting on the ring is let go; and the loop owes
    /// a frame as after a present, since the next scene has no placement of it and a wake
    /// alone takes no turn of core.
    fn destroy(&self, id: PixelBufferId) {
        let Some(b) = lock(&self.buffers).remove(&id) else {
            return;
        };
        let mut st = lock(&b.st);
        (st.texture, st.waiting) = (None, None);
        st.slots.clear();
        st.free.clear();
        b.freed.notify_all();
        self.pending.set();
    }
}

/// A waiting frame, for the main thread to blit into its buffer's texture. Dropping it gives
/// the slot back: the command buffer's completed handler does, on Metal's thread
/// (`release_after`), so the main thread takes no lock for it.
pub struct Blit {
    buffer: Arc<Buffer>,
    slot: usize,
    source: Slot,
    damage: PixelRect,
    texture: Tex,
}

impl Blit {
    /// Record the copy of the slot's rows into the texture's damaged rectangle.
    pub(super) fn encode(&self, encoder: &ProtocolObject<dyn MTLBlitCommandEncoder>) {
        let d = self.damage;
        let row = d.width as usize * 4;
        let size = MTLSize {
            width: d.width as usize,
            height: d.height as usize,
            depth: 1,
        };
        let origin = MTLOrigin {
            x: d.x as usize,
            y: d.y as usize,
            z: 0,
        };
        // SAFETY: the slot holds the damage's `row * height` bytes from its start (`copy_rows`
        // put them there, in a ring sized for the buffer's largest size); the damage is inside the
        // texture, since core clips it to the buffer the texture is sized to and a resize drops a
        // frame waiting at the old size; and this blit retains both until the command buffer is
        // done with them.
        unsafe {
            encoder.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &self.source.0, 0, row, row * d.height as usize, size, &self.texture.0, 0, 0, origin,
            )
        };
    }
}

impl Drop for Blit {
    fn drop(&mut self) {
        let mut st = lock(&self.buffer.st);
        if self.slot < st.slots.len() {
            st.free.push(self.slot);
        }
        self.buffer.freed.notify_one();
    }
}

/// Give the blits' slots back once `commands` has completed, from its handler on Metal's
/// thread. Call it before the commit. The handler runs Rust that Metal called, so a panic in
/// it is caught (platform/mod.rs's rule) and the slots stay out, rather than unwind into Metal.
pub(super) fn release_after(blits: Vec<Blit>, commands: &ProtocolObject<dyn MTLCommandBuffer>) {
    let held = Mutex::new(Some(blits));
    let block = RcBlock::new(move |_: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(lock(&held).take())));
    });
    // SAFETY: the block takes what the method says, Metal copies it, and all it holds is `Send`.
    unsafe { commands.addCompletedHandler(RcBlock::as_ptr(&block)) };
}

/// The main thread's view of the buffers: each one it knows, and its texture as last seen, so a
/// frame whose try at a buffer's lock failed still draws its placement from the texture it had.
#[derive(Default)]
pub struct Shadow {
    known: HashMap<PixelBufferId, (Arc<Buffer>, Option<Tex>)>,
}

impl Shadow {
    /// The frames waiting, as blits for the next command buffer, of every buffer, placed or
    /// not, so that no ring fills up unseen. It only tries each lock: a buffer whose producer
    /// holds its lock keeps its frame for the next wakeup, which the producer's wake brings.
    pub fn take(&mut self, pixels: &Pixels) -> Vec<Blit> {
        if let Ok(live) = pixels.buffers.try_lock() {
            self.known.retain(|id, _| live.contains_key(id));
            for (id, b) in live.iter() {
                self.known.entry(*id).or_insert_with(|| (b.clone(), None));
            }
        }
        let mut blits = vec![];
        for (buffer, texture) in self.known.values_mut() {
            let Ok(mut st) = buffer.st.try_lock() else {
                continue;
            };
            let waiting = st.waiting.take();
            if let Some(w) = waiting.filter(|w| w.slot < st.slots.len()) {
                let Some(t) = st.texture.as_ref().map(|t| Tex(t.0.clone())) else {
                    // Metal had no texture for the new size: nothing to blit into, and the old
                    // size's texture mustn't stand in for it.
                    st.free.push(w.slot);
                    *texture = None;
                    continue;
                };
                st.shown = true;
                let bytes = u64::from(w.damage.width) * u64::from(w.damage.height) * 4;
                pixels.counts.blits.fetch_add(1, Ordering::Relaxed);
                pixels.counts.blit_bytes.fetch_add(bytes, Ordering::Relaxed);
                blits.push(Blit {
                    buffer: buffer.clone(),
                    slot: w.slot,
                    source: Slot(st.slots[w.slot].0.clone()),
                    damage: w.damage,
                    texture: t,
                });
            }
            *texture = match (&st.texture, st.shown) {
                (Some(t), true) => Some(Tex(t.0.clone())),
                _ => None,
            };
        }
        blits
    }

    /// The texture a placement of `id` samples, once its buffer has been shown.
    pub fn texture(&self, id: PixelBufferId) -> Option<&ProtocolObject<dyn MTLTexture>> {
        self.known.get(&id)?.1.as_ref().map(|t| &*t.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// An id, which only core gives out.
    fn id() -> PixelBufferId {
        let mut ctx = die_core::Ctx::new(vec![]).unwrap();
        ctx.set_pixel_sink(die_core::FakePixelSink::default())
            .unwrap();
        ctx.pixel_buffer(1, 1).unwrap().0
    }

    fn sink() -> Option<(Arc<Pixels>, Pending)> {
        let pending = Pending::default();
        Some((Pixels::new(pending.clone())?, pending))
    }

    /// `w` × `h` of one byte value.
    fn pixels(w: u32, h: u32, v: u8) -> Vec<u8> {
        vec![v; w as usize * h as usize * 4]
    }

    /// The slot's first `n` bytes.
    fn staged(b: &Blit, n: usize) -> Vec<u8> {
        let p = b.source.0.contents().cast::<u8>();
        // SAFETY: the slot holds at least `n` bytes, all written by `copy_rows`, and nothing
        // writes it while this blit holds it.
        unsafe { std::slice::from_raw_parts(p.as_ptr(), n) }.to_vec()
    }

    #[test]
    fn a_present_waits_in_a_slot_with_its_rows_and_a_later_one_supersedes_it_merging_damage() {
        let Some((p, pending)) = sink() else { return };
        let a = id();
        p.create(a, 8, 8);
        let mut shadow = Shadow::default();
        assert!(shadow.take(&p).is_empty(), "nothing yet");
        assert!(shadow.texture(a).is_none(), "not shown yet");
        p.present(a, &pixels(8, 8, 7), 8, PixelRect::new(0, 0, 2, 2));
        assert!(pending.take());
        p.present(a, &pixels(8, 8, 9), 8, PixelRect::new(4, 4, 2, 2));
        let blits = shadow.take(&p);
        assert_eq!(blits.len(), 1);
        assert_eq!(blits[0].damage, PixelRect::new(0, 0, 6, 6), "the union");
        assert_eq!(
            staged(&blits[0], 6 * 6 * 4),
            pixels(6, 6, 9),
            "the later rows"
        );
        assert!(shadow.texture(a).is_some(), "shown once a blit is recorded");
        let s = p.stats();
        assert_eq!((s.presents, s.superseded, s.blits), (2, 1, 1));
        assert_eq!((s.producer_bytes, s.blit_bytes), (16 + 144, 144));
        assert_eq!(s.main_bytes, 0, "a test thread isn't the main one");
        assert!(shadow.take(&p).is_empty(), "taken once");
    }

    #[test]
    fn a_full_ring_waits_the_producer_until_a_slot_comes_back_and_never_the_main_thread() {
        let Some((p, _)) = sink() else { return };
        let a = id();
        p.create(a, 4, 4);
        let mut shadow = Shadow::default();
        let whole = PixelRect::whole(4, 4);
        let mut held = vec![];
        for _ in 0..RING {
            p.present(a, &pixels(4, 4, 1), 4, whole);
            held.extend(shadow.take(&p));
        }
        assert_eq!(held.len(), RING, "every slot is out with the GPU");
        let (tx, rx) = mpsc::channel();
        let producer = {
            let p = p.clone();
            std::thread::spawn(move || {
                p.present(a, &pixels(4, 4, 2), 4, whole);
                tx.send(()).unwrap();
            })
        };
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "the producer waits"
        );
        assert!(shadow.take(&p).is_empty(), "nothing waits, and main didn't");
        drop(held.pop());
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a slot came back");
        producer.join().unwrap();
        assert!(p.stats().ring_waits >= 1);
        assert_eq!(shadow.take(&p).len(), 1, "the frame that waited");
    }

    #[test]
    fn the_main_thread_only_tries_a_lock_a_producer_holds() {
        let Some((p, _)) = sink() else { return };
        let a = id();
        p.create(a, 4, 4);
        p.present(a, &pixels(4, 4, 1), 4, PixelRect::whole(4, 4));
        let b = p.buffer(a).unwrap();
        let guard = lock(&b.st);
        let (tx, rx) = mpsc::channel();
        let main = {
            let p = p.clone();
            std::thread::spawn(move || tx.send(Shadow::default().take(&p).len()))
        };
        let got = rx.recv_timeout(Duration::from_secs(5));
        assert_eq!(got.ok(), Some(0), "it came back at once, with nothing");
        drop(guard);
        main.join().unwrap().unwrap();
        assert_eq!(Shadow::default().take(&p).len(), 1, "the frame still waits");
    }

    #[test]
    fn a_resize_drops_the_frame_waiting_hides_the_texture_and_grows_the_ring_around_a_slot_out() {
        let Some((p, _)) = sink() else { return };
        let a = id();
        p.create(a, 4, 4);
        let mut shadow = Shadow::default();
        p.present(a, &pixels(4, 4, 1), 4, PixelRect::whole(4, 4));
        let shown = shadow.take(&p);
        assert_eq!(shown.len(), 1);
        drop(shown);
        assert!(shadow.texture(a).is_some());
        p.present(a, &pixels(4, 4, 1), 4, PixelRect::whole(4, 4));
        p.resize(a, 4, 2);
        assert!(shadow.take(&p).is_empty(), "the old size's frame went");
        assert!(shadow.texture(a).is_none(), "nothing shows until a present");
        let b = p.buffer(a).unwrap();
        assert_eq!(lock(&b.st).bytes, 64, "the ring kept its size");
        p.present(a, &pixels(4, 2, 3), 4, PixelRect::whole(4, 2));
        let out = shadow.take(&p);
        assert_eq!(out.len(), 1);
        p.resize(a, 8, 8);
        {
            let st = lock(&b.st);
            assert_eq!((st.bytes, st.slots.len()), (256, RING), "a bigger ring");
            assert_eq!(st.free.len(), RING - 1, "the slot out stays out");
        }
        drop(out);
        assert_eq!(lock(&b.st).free.len(), RING, "and comes back");
        p.present(a, &pixels(8, 8, 4), 8, PixelRect::whole(8, 8));
        assert_eq!(shadow.take(&p).len(), 1, "the bigger frame stages");
    }

    #[test]
    fn destroyed_while_a_frame_waits_nothing_is_blitted_and_a_waiting_producer_is_let_go() {
        let Some((p, pending)) = sink() else { return };
        let a = id();
        p.create(a, 4, 4);
        let mut shadow = Shadow::default();
        let whole = PixelRect::whole(4, 4);
        let mut held = vec![];
        for _ in 0..RING {
            p.present(a, &pixels(4, 4, 1), 4, whole);
            held.extend(shadow.take(&p));
        }
        let waiting = {
            let p = p.clone();
            std::thread::spawn(move || p.present(a, &pixels(4, 4, 2), 4, whole))
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(pending.take());
        p.destroy(a);
        assert!(pending.take(), "a frame is owed, with no placement of it");
        waiting.join().expect("let go");
        assert!(shadow.take(&p).is_empty());
        assert!(shadow.texture(a).is_none(), "forgotten");
        assert!(lock(&p.buffers).is_empty());
        let before = p.stats().presents;
        p.present(a, &pixels(4, 4, 2), 4, whole);
        assert_eq!(
            p.stats().presents,
            before,
            "a present of a gone buffer is nothing"
        );
        drop(held);
    }

    #[test]
    fn damage_past_the_pixels_copies_nothing_and_stages_nothing() {
        let Some((p, pending)) = sink() else { return };
        let a = id();
        p.create(a, 4, 4);
        p.present(a, &pixels(4, 2, 1), 4, PixelRect::whole(4, 4));
        assert!(Shadow::default().take(&p).is_empty());
        assert_eq!(p.stats().producer_bytes, 0);
        assert!(!pending.is_set());
        let b = p.buffer(a).unwrap();
        assert_eq!(lock(&b.st).free.len(), RING, "its slot came back");
    }

    #[test]
    fn two_threads_presenting_one_buffer_at_once_lose_no_slot_and_neither_hangs() {
        let Some((p, _)) = sink() else { return };
        let a = id();
        // Big, so that a copy takes long enough for the two to interleave between their locks.
        let (w, h) = (2048, 2048);
        p.create(a, w, h);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let (tx, rx) = mpsc::channel();
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let (p, barrier, tx) = (p.clone(), barrier.clone(), tx.clone());
                std::thread::spawn(move || {
                    let pixels = pixels(w, h, 1);
                    barrier.wait();
                    for _ in 0..12 {
                        p.present(a, &pixels, w, PixelRect::whole(w, h));
                    }
                    tx.send(()).unwrap();
                })
            })
            .collect();
        drop(tx);
        for _ in 0..2 {
            rx.recv_timeout(Duration::from_secs(20))
                .expect("a presenting thread hung on a slot it lost");
        }
        threads.into_iter().for_each(|t| t.join().unwrap());
        let b = p.buffer(a).unwrap();
        let st = lock(&b.st);
        let waiting = usize::from(st.waiting.is_some());
        assert_eq!(
            st.free.len() + waiting,
            RING,
            "every slot is free or waiting"
        );
        let s = p.stats();
        assert_eq!(s.presents, 24);
        assert_eq!(s.superseded, 23, "each took the last one's slot back");
    }

    #[test]
    fn a_resize_metal_cant_texture_shows_nothing_rather_than_the_old_size() {
        let Some((p, _)) = sink() else { return };
        let a = id();
        p.create(a, 4, 4);
        let mut shadow = Shadow::default();
        p.present(a, &pixels(4, 4, 1), 4, PixelRect::whole(4, 4));
        drop(shadow.take(&p));
        assert!(shadow.texture(a).is_some());
        // A side past Metal's cap gets no texture; core refuses such a size, but the sink
        // mustn't rely on it.
        let wide = die_core::PIXEL_MAX_SIDE + 1;
        p.resize(a, wide, 1);
        p.present(a, &pixels(wide, 1, 2), wide, PixelRect::whole(wide, 1));
        assert!(shadow.take(&p).is_empty(), "nothing to blit into");
        assert!(
            shadow.texture(a).is_none(),
            "the old size's texture doesn't stand in"
        );
        let b = p.buffer(a).unwrap();
        assert_eq!(lock(&b.st).free.len(), RING, "the slot came back");
    }
}
