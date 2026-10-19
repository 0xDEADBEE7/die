//! Engine-side host logic for one terminal, shared by the parser thread (in-process) and
//! the helper process: read the PTY, feed the engine, answer DIE's jobs, and produce
//! frames under credit-based coalescing (at most one frame in flight; DIE returns the
//! credit at vsync).
use crate::engine::Engine;
use crate::proto::{Pixels, Stats, ToDie, ToEngine};
use crate::util::now_ns;
use std::os::fd::RawFd;

/// How long a synchronized-output hold (mode 2026) may delay frames.
const HOLD_TIMEOUT_NS: u64 = 1_000_000_000;

pub struct Host {
    pub eng: Engine,
    pub pty: RawFd,
    pub child: i32,
    /// Frames DIE will accept right now (0 or 1).
    pub credits: u32,
    pub dirty: bool,
    pub need_full: bool,
    /// Earliest PTY read whose bytes are not in a frame yet.
    pub t_first_read: u64,
    pub held_since: Option<u64>,
    pub pty_out: Vec<u8>,
    pub eof: bool,
    pub stats: Stats,
    rbuf: Vec<u8>,
}

impl Host {
    pub fn new(eng: Engine, pty: RawFd, child: i32) -> Host {
        Host {
            eng,
            pty,
            child,
            credits: 1,
            dirty: true,
            need_full: true,
            t_first_read: 0,
            held_since: None,
            pty_out: Vec::new(),
            eof: false,
            stats: Stats { pid: crate::util::pid() as u32, ..Default::default() },
            rbuf: vec![0u8; 64 * 1024],
        }
    }

    /// Read and parse up to `budget` bytes of PTY output. Sets `eof` when the child side closed.
    pub fn read_pty(&mut self, budget: usize) {
        let mut total = 0;
        while total < budget {
            // SAFETY: reading into our buffer.
            let n = unsafe { libc::read(self.pty, self.rbuf.as_mut_ptr().cast(), self.rbuf.len()) };
            if n > 0 {
                if self.t_first_read == 0 {
                    self.t_first_read = now_ns();
                }
                let n = n as usize;
                self.eng.write(&self.rbuf[..n]);
                total += n;
                self.stats.bytes_parsed += n as u64;
                self.dirty = true;
                let r = std::mem::take(&mut self.eng.out.borrow_mut().pty);
                self.pty_out.extend_from_slice(&r);
            } else if n == 0 {
                self.eof = true;
                break;
            } else {
                let e = std::io::Error::last_os_error().raw_os_error();
                if e == Some(libc::EAGAIN) || e == Some(libc::EINTR) {
                    break;
                }
                self.eof = true; // EIO: the slave side is gone
                break;
            }
        }
        self.flush_pty();
    }

    /// Write queued bytes (replies, keys) to the PTY without blocking.
    pub fn flush_pty(&mut self) {
        while !self.pty_out.is_empty() {
            // SAFETY: writing from our buffer.
            let n = unsafe { libc::write(self.pty, self.pty_out.as_ptr().cast(), self.pty_out.len()) };
            if n <= 0 {
                break;
            }
            self.pty_out.drain(..n as usize);
        }
    }

    pub fn handle(&mut self, m: ToEngine, send: &mut dyn FnMut(ToDie)) {
        match m {
            ToEngine::Key { key, mods, action, text, unshifted, .. } => {
                let b = self.eng.encode_key(key, mods, action, &text, unshifted);
                self.pty_out.extend_from_slice(&b);
            }
            ToEngine::Text(b) => self.pty_out.extend_from_slice(&b),
            ToEngine::Mouse { action, button, mods, x, y, any_pressed } => {
                let b = self.eng.encode_mouse(action, button, mods, x, y, any_pressed);
                self.pty_out.extend_from_slice(&b);
            }
            ToEngine::Resize { cols, rows, cell_w, cell_h } => {
                self.eng.resize(cols, rows, cell_w, cell_h);
                crate::pty::set_size(self.pty, cols, rows, cell_w, cell_h);
                self.need_full = true;
            }
            ToEngine::Ack { .. } => self.credits = 1,
            ToEngine::Focus(f) => {
                if self.eng.term.mode(libghostty_vt::terminal::Mode::FOCUS_EVENT).unwrap_or(false) {
                    self.pty_out.extend_from_slice(if f { b"\x1b[I" } else { b"\x1b[O" });
                }
            }
            ToEngine::FetchScrollback { req } => {
                let (lines, text) = self.eng.scrollback_text();
                send(ToDie::Scrollback { req, lines, text, t_built: now_ns() });
            }
            ToEngine::GetStats => {
                let mut s = self.stats.clone();
                s.png_decodes = crate::hook::DECODES.load(std::sync::atomic::Ordering::Relaxed);
                s.png_decode_ns = crate::hook::DECODE_NS.load(std::sync::atomic::Ordering::Relaxed);
                send(ToDie::Stats(s));
            }
            ToEngine::Shutdown => {
                // SAFETY: signalling our own child.
                unsafe { libc::kill(self.child, libc::SIGKILL) };
            }
        }
        self.flush_pty();
    }

    /// Poll timeout for the engine-side loop: -1 (block) unless a sync hold may expire.
    pub fn timeout_ms(&self) -> i32 {
        match self.held_since {
            Some(t) if self.dirty => {
                let left = (t + HOLD_TIMEOUT_NS).saturating_sub(now_ns());
                (left / 1_000_000) as i32 + 1
            }
            _ => -1,
        }
    }

    /// Hand DIE what it should have now: effects, new images (any time), and at most one
    /// frame per credit.
    pub fn produce(&mut self, alloc: &mut dyn FnMut(usize) -> (Pixels, *mut u8), send: &mut dyn FnMut(ToDie)) {
        let effects = std::mem::take(&mut self.eng.out.borrow_mut().effects);
        if std::env::var_os("S2_DEBUG").is_some() {
            eprintln!(
                "[eng] produce credits={} dirty={} held={} effects={} parsed={}",
                self.credits, self.dirty, self.eng.held, effects.len(), self.stats.bytes_parsed
            );
        }
        for e in effects {
            send(ToDie::Effect(e));
        }
        let held = if self.eng.held {
            let t = *self.held_since.get_or_insert_with(now_ns);
            now_ns() < t + HOLD_TIMEOUT_NS
        } else {
            self.held_since = None;
            false
        };
        if held {
            return;
        }
        for img in self.eng.new_images(alloc) {
            self.stats.images_copied += 1;
            self.stats.image_bytes_copied += img.pixels.bytes().len() as u64;
            send(ToDie::Image(img));
        }
        if self.credits > 0 && (self.dirty || self.need_full) {
            let full = self.need_full;
            self.dirty = false;
            self.need_full = false;
            if let Some(f) = self.eng.frame(full, self.t_first_read) {
                self.t_first_read = 0;
                self.credits -= 1;
                self.stats.frames += 1;
                send(ToDie::Frame(f));
                for id in self.eng.gone_images() {
                    send(ToDie::ImageGone { image_id: id });
                }
            }
        }
    }

    /// Reap the child once the PTY hit EOF.
    pub fn exit_status(&self) -> i32 {
        for _ in 0..200 {
            if let Some(s) = crate::pty::try_wait(self.child) {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        -1
    }
}
