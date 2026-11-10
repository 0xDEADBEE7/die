//! The frame data that crosses the boundary between neutral code and the backend.
//! These are plain values: neutral code makes them and reads them, the driver
//! hands them over, and the backend never decides anything from them.

use die_core::Rgb;

/// What a quad's fragment shader does. Written as `Quad::kind`.
pub mod kind {
    /// A rectangle of `Quad::color`.
    pub const SOLID: u32 = 0;
    /// `Quad::color` through coverage from the gray atlas.
    pub const GRAY: u32 = 1;
    /// A premultiplied pixel from the color atlas, as it is. `Quad::color` is unused.
    pub const COLOR: u32 = 2;
    /// A premultiplied pixel from a placement's buffer (`rows::Frame::placed` says which), 1:1
    /// and nearest, and nothing past the buffer's edge. `Quad::color` is unused.
    pub const PIXELS: u32 = 3;
}

/// What the host's pixel sink has done, for the `pixels` check: the producers' presents, the
/// bytes they copied into the staging ring on their threads and on the main thread (which must
/// stay 0 for a producer of its own), and the blits the main thread recorded.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct PixelStats {
    /// Presents that reached the ring.
    pub presents: u64,
    /// Presents whose frame the main thread never took, since a later one superseded it.
    pub superseded: u64,
    /// Presents that waited for a free slot.
    pub ring_waits: u64,
    /// Bytes copied into the ring on a producer's own thread.
    pub producer_bytes: u64,
    /// Bytes copied into the ring on the main thread.
    pub main_bytes: u64,
    /// Blits the main thread recorded, and their bytes.
    pub blits: u64,
    /// The bytes those blits moved.
    pub blit_bytes: u64,
}

/// One instance of the one pipeline: a rectangle of the target, in pixels from the
/// top left. The shader's struct must match this field for field, and
/// `tests/platform.rs` asks the compiled shader. MSL aligns its `float4`s to 16
/// bytes, so the struct needs its padding.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quad {
    /// x, y, width, height in the target, in pixels.
    pub rect: [f32; 4],
    /// x, y, width, height in the atlas, in pixels.
    pub uv: [f32; 4],
    /// 0xRRGGBBAA, not premultiplied.
    pub color: u32,
    /// One of `kind`'s values.
    pub kind: u32,
    /// Keeps the struct's size a multiple of 16.
    pub pad: [u32; 2],
}

/// `c` as `Quad::color` has it.
pub fn rgba(c: Rgb) -> u32 {
    u32::from_be_bytes([c.0, c.1, c.2, 255])
}

/// Which atlas a glyph's pixels live in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plane {
    /// One byte of coverage per pixel, tinted by the cell's color.
    Gray,
    /// Four bytes per pixel, BGRA and premultiplied: emoji.
    Color,
}

/// Pixels for the backend to copy into an atlas.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Upload {
    /// The atlas.
    pub plane: Plane,
    /// The left edge, in atlas pixels.
    pub x: u32,
    /// The top edge, in atlas pixels.
    pub y: u32,
    /// The width in pixels.
    pub w: u32,
    /// The height in pixels.
    pub h: u32,
    /// `h` rows of `w` pixels, in the plane's format, top row first.
    pub data: Vec<u8>,
}

/// One glyph drawn into its own bitmap, by a face or by the renderer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GlyphBitmap {
    /// The width in pixels.
    pub w: u32,
    /// The height in pixels.
    pub h: u32,
    /// Where the bitmap's left edge goes, in pixels from the cell's left edge.
    pub left: i32,
    /// Where the bitmap's top edge goes, in pixels from the cell's top edge.
    pub top: i32,
    /// Which atlas it goes in, and so its format.
    pub plane: Plane,
    /// `h` rows of `w` pixels, top row first.
    pub data: Vec<u8>,
}

/// A rendered frame, read back: `h` rows of `w` BGRA pixels, top row first.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Image {
    /// The width in pixels.
    pub w: u32,
    /// The height in pixels.
    pub h: u32,
    /// The pixels.
    pub bgra: Vec<u8>,
}

/// A frame on the window's drawable.
#[derive(Clone, PartialEq, Debug)]
pub struct Presented {
    /// The drawable's width in pixels.
    pub w: u32,
    /// The drawable's height in pixels.
    pub h: u32,
    /// Its pixels, if the surface was asked to read this frame back.
    pub image: Option<Image>,
    /// Which of the surface's presents it is, as `Surface::take_presented` names it.
    pub id: u64,
    /// The main thread's time on it in ms, up to the commit.
    pub cpu_ms: f64,
    /// The GPU's time on it in ms.
    pub gpu_ms: f64,
}

impl Image {
    /// The pixel at (x, y), as red, green and blue.
    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.w as usize + x as usize) * 4;
        [self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]]
    }
}
