//! Pixel buffers (contract §11, scope §5.2, D7 and D11): the producer an extension draws into,
//! the sink the host installs once to take what it presents, and what core keeps of them, which
//! is only which buffers exist. Core holds no pixels and draws none: a producer owns its buffer,
//! the host's sink owns what the GPU reads, and a surface shows a buffer with [`Frame::place`].
//!
//! [`Frame::place`]: crate::Frame::place
#![warn(missing_docs)]
use crate::error::*;
use crate::ext::Ctx;
use crate::rt::Waker;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The most a side can be: Metal's texture side cap, which the Linux backend's limits are held to
/// when it lands.
pub const PIXEL_MAX_SIDE: u32 = 16_384;

/// A pixel buffer, named across threads: its producer's, and the placements that show it. An id
/// is never given out twice in one workspace, so a placement of a destroyed buffer names nothing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PixelBufferId(u64);

/// A rectangle of pixels in a buffer, from its top-left corner.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct PixelRect {
    /// Its left edge.
    pub x: u32,
    /// Its top edge.
    pub y: u32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

impl PixelRect {
    /// This rectangle.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }
    /// The whole of a buffer this size.
    pub const fn whole(width: u32, height: u32) -> PixelRect {
        PixelRect::new(0, 0, width, height)
    }
    /// Nothing in it.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
    /// The part of it inside a `width` × `height` buffer.
    pub fn clipped_to(self, width: u32, height: u32) -> PixelRect {
        let (x, y) = (self.x.min(width), self.y.min(height));
        let right = self.x.saturating_add(self.width).min(width);
        let bottom = self.y.saturating_add(self.height).min(height);
        PixelRect::new(x, y, right - x, bottom - y)
    }
    /// The smallest rectangle holding both, as a sink merges the damage of two presents it
    /// takes together. An empty one adds nothing; an edge past `u32::MAX` stops there.
    pub fn union(self, other: PixelRect) -> PixelRect {
        match (self.is_empty(), other.is_empty()) {
            (true, _) => other,
            (_, true) => self,
            _ => {
                let (x, y) = (self.x.min(other.x), self.y.min(other.y));
                let far = |a: u32, n: u32| a.saturating_add(n);
                let right = far(self.x, self.width).max(far(other.x, other.width));
                let bottom = far(self.y, self.height).max(far(other.y, other.height));
                PixelRect::new(x, y, right - x, bottom - y)
            }
        }
    }
}

/// Where producers' pixels go: the host's, connected once with [`Ctx::set_pixel_sink`] as the
/// waker is, or the harness's fake. Every call runs on the producer's thread, which is the main
/// thread for a surface that rasterizes in `event()`, or one of the producer's own.
///
/// The rule the shapes keep (scope §5.2): `present` runs on the caller's thread and may block it,
/// as when the host's ring of staging buffers is full, but the main thread never waits on a lock
/// a producer can hold. A host's sink hands pixels over through something the main thread only
/// tries (`try_lock`), and a hand-off that's busy shows on the next frame. The fake takes one
/// lock briefly on both sides, which a host's sink mustn't.
///
/// A buffer shows nothing until its first `present` after `create` or `resize`: a placement of
/// it draws nothing yet, rather than whatever the GPU's memory held.
pub trait PixelSink: Send + Sync {
    /// A buffer this size exists now, transparent.
    fn create(&self, id: PixelBufferId, width: u32, height: u32);
    /// `damage` of the buffer changed. `pixels` is the whole buffer, row-major, `width * 4` bytes
    /// a row of premultiplied RGBA8; the sink copies the damage out of it, and reads nothing
    /// else. The damage is inside the buffer and never empty.
    fn present(&self, id: PixelBufferId, pixels: &[u8], width: u32, damage: PixelRect);
    /// The buffer is this size now, and transparent again.
    fn resize(&self, id: PixelBufferId, width: u32, height: u32);
    /// The buffer is gone.
    fn destroy(&self, id: PixelBufferId);
}

impl<S: PixelSink + ?Sized> PixelSink for Arc<S> {
    fn create(&self, id: PixelBufferId, width: u32, height: u32) {
        (**self).create(id, width, height)
    }
    fn present(&self, id: PixelBufferId, pixels: &[u8], width: u32, damage: PixelRect) {
        (**self).present(id, pixels, width, damage)
    }
    fn resize(&self, id: PixelBufferId, width: u32, height: u32) {
        (**self).resize(id, width, height)
    }
    fn destroy(&self, id: PixelBufferId) {
        (**self).destroy(id)
    }
}

/// The handle that draws into a pixel buffer ([`Ctx::pixel_buffer`]). It owns the buffer: one
/// persistent run of premultiplied RGBA8, `width * 4` bytes a row. It's `Send`, so a thread of
/// its own can own it, and nothing of `Ctx` goes with it (D7). Dropping it destroys the buffer:
/// the sink is told, and a placement that still names it draws nothing.
pub struct PixelProducer {
    id: PixelBufferId,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    sink: Arc<dyn PixelSink>,
    waker: Waker,
    alive: Arc<AtomicBool>,
}

// A producer is for another thread, so it must carry nothing of core's single-threaded state.
const _: () = {
    const fn ok<T: Send + Sync + 'static>() {}
    ok::<PixelProducer>();
};

impl PixelProducer {
    /// The buffer's id, for [`Frame::place`](crate::Frame::place).
    pub fn id(&self) -> PixelBufferId {
        self.id
    }
    /// Its width and height in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    /// A row's bytes: the width times 4.
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }
    /// The pixels, row-major, premultiplied RGBA8.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    /// The pixels, to draw into. Nothing shows until [`PixelProducer::present`].
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
    /// Make the buffer this size, transparent, and tell the sink: what was drawn is gone, and the
    /// placement shows nothing until the next present. Err for a size a buffer can't be, with the
    /// buffer as it was.
    pub fn resize(&mut self, width: u32, height: u32) -> Result {
        check_size(width, height)?;
        (self.width, self.height) = (width, height);
        self.pixels = vec![0; width as usize * height as usize * 4];
        self.sink.resize(self.id, width, height);
        Ok(())
    }
    /// `damage` is drawn: hand it to the sink, on this thread, and wake the loop so a frame
    /// follows. The damage is clipped to the buffer; none left means nothing happens.
    pub fn present(&self, damage: PixelRect) {
        let damage = damage.clipped_to(self.width, self.height);
        if damage.is_empty() {
            return;
        }
        self.sink.present(self.id, &self.pixels, self.width, damage);
        self.waker.wake();
    }
}

impl Drop for PixelProducer {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.sink.destroy(self.id);
        self.waker.wake();
    }
}

impl fmt::Debug for PixelProducer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PixelProducer({:?}, {}x{})",
            self.id, self.width, self.height
        )
    }
}

fn check_size(width: u32, height: u32) -> Result {
    if width == 0 || height == 0 || width > PIXEL_MAX_SIDE || height > PIXEL_MAX_SIDE {
        return Err(Error::Msg(format!(
            "a pixel buffer is 1 to {PIXEL_MAX_SIDE} pixels a side, not {width}x{height}"
        )));
    }
    Ok(())
}

/// What core keeps of pixel buffers: the host's sink, the last id given out, and which buffers
/// are alive, each by the flag its producer clears when it's dropped, on whichever thread.
#[derive(Default)]
pub(crate) struct Pixels {
    sink: Option<Arc<dyn PixelSink>>,
    last: u64,
    alive: HashMap<PixelBufferId, Arc<AtomicBool>>,
}

impl Pixels {
    /// Whether this buffer still exists, forgetting it once its producer has gone.
    pub(crate) fn is_alive(&mut self, id: PixelBufferId) -> bool {
        let alive = self
            .alive
            .get(&id)
            .is_some_and(|a| a.load(Ordering::Acquire));
        if !alive {
            self.alive.remove(&id);
        }
        alive
    }
}

impl Ctx {
    /// Connect where producers' pixels go, once, as [`Ctx::set_waker`] connects the loop: the
    /// host does at startup, before any extension runs, and the harness gives its fake. A second
    /// call is an error.
    pub fn set_pixel_sink(&mut self, sink: impl PixelSink + 'static) -> Result {
        if self.pixels.sink.is_some() {
            return Err(Error::Msg("the pixel sink is already connected".into()));
        }
        self.pixels.sink = Some(Arc::new(sink));
        Ok(())
    }
    /// Whether this host shows pixels: a sink is connected. A workspace with no window has none,
    /// and a host whose renderer has no pixel path yet, so a surface draws cells then.
    pub fn pixels_supported(&self) -> bool {
        self.pixels.sink.is_some()
    }
    /// A pixel buffer `width` × `height` physical pixels, transparent, and the [`PixelProducer`] that
    /// draws into it. Size it from a `Resize`'s cells and [`CellPixels`](crate::CellPixels), and
    /// show it with [`Frame::place`](crate::Frame::place). Err with no sink connected
    /// ([`Ctx::pixels_supported`]), or a side of 0 or over [`PIXEL_MAX_SIDE`].
    pub fn pixel_buffer(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(PixelBufferId, PixelProducer)> {
        let Some(sink) = self.pixels.sink.clone() else {
            return Err(Error::Msg("this host shows no pixels".into()));
        };
        check_size(width, height)?;
        self.pixels.last += 1;
        let id = PixelBufferId(self.pixels.last);
        let alive = Arc::new(AtomicBool::new(true));
        // Buffers dropped since, placed or not, go here: compose forgets only the placed ones.
        self.pixels.alive.retain(|_, a| a.load(Ordering::Acquire));
        self.pixels.alive.insert(id, alive.clone());
        sink.create(id, width, height);
        let p = PixelProducer {
            id,
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
            sink,
            waker: self.waker.clone(),
            alive,
        };
        Ok((id, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::FakePixelSink;

    #[test]
    fn buffers_dropped_without_a_placement_are_forgotten_at_the_next_one() {
        let mut ctx = Ctx::new(vec![]).unwrap();
        ctx.set_pixel_sink(FakePixelSink::default()).unwrap();
        for _ in 0..8 {
            let (_, producer) = ctx.pixel_buffer(1, 1).unwrap();
            drop(producer);
        }
        let (id, _kept) = ctx.pixel_buffer(1, 1).unwrap();
        assert_eq!(ctx.pixels.alive.len(), 1, "the eight dropped ones went");
        assert!(ctx.pixels.is_alive(id));
    }
}
