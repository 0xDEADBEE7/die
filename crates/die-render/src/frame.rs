//! The frame data that crosses the boundary between neutral code and the backend.
//! These are plain values: neutral code makes them and reads them, the driver
//! hands them over, and the backend never decides anything from them.
#![warn(missing_docs)]

use die_core::Rgb;

/// What a quad's fragment shader does. Written as `Quad::kind`.
pub mod kind {
    /// A rectangle of `Quad::color`.
    pub const SOLID: u32 = 0;
    /// `Quad::color` through coverage from the gray atlas.
    pub const GRAY: u32 = 1;
    /// A premultiplied pixel from the color atlas, as it is. `Quad::color` is unused.
    pub const COLOR: u32 = 2;
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
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Presented {
    /// The drawable's width in pixels.
    pub w: u32,
    /// The drawable's height in pixels.
    pub h: u32,
    /// Its pixels, if the surface was asked to read this frame back.
    pub image: Option<Image>,
}

impl Image {
    /// The pixel at (x, y), as red, green and blue.
    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.w as usize + x as usize) * 4;
        [self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]]
    }
}
