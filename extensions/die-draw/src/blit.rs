//! A premultiplied RGBA8 target with clipping: coverage in an ink, pictures over what's there,
//! and filled rectangles. The lift from die-markdown replaces the skeleton's bodies with its
//! own arithmetic.

use crate::Rgb;
use crate::fonts::Mask;

/// Premultiplied RGBA8 pixels, `stride` bytes a row: DIE's buffers and tiny-skia's are
/// `width × 4`.
pub struct Pixels<'a> {
    bytes: &'a mut [u8],
    width: u32,
    height: u32,
    stride: usize,
}

/// A half-open pixel rectangle, `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Clip {
    /// The left column.
    pub x0: i32,
    /// The top row.
    pub y0: i32,
    /// The column past the right.
    pub x1: i32,
    /// The row past the bottom.
    pub y1: i32,
}

impl Clip {
    /// What both cover.
    pub fn meet(self, other: Clip) -> Clip {
        Clip {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// Whether it covers no pixel.
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
}

impl<'a> Pixels<'a> {
    /// `width × height` pixels, `width × 4` bytes a row. None if `bytes` is too short.
    pub fn new(bytes: &'a mut [u8], width: u32, height: u32) -> Option<Pixels<'a>> {
        Pixels::with_stride(bytes, width, height, width as usize * 4)
    }

    /// `width × height` pixels, `stride` bytes a row. None if a row doesn't fit its stride or
    /// `bytes` is too short.
    pub fn with_stride(
        bytes: &'a mut [u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> Option<Pixels<'a>> {
        let row = width as usize * 4;
        let needed = match height {
            0 => 0,
            h => stride * (h as usize - 1) + row,
        };
        (stride >= row && bytes.len() >= needed).then_some(Pixels {
            bytes,
            width,
            height,
            stride,
        })
    }

    /// Its width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Its height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// All of it, as a clip.
    pub fn whole(&self) -> Clip {
        Clip {
            x0: 0,
            y0: 0,
            x1: self.width as i32,
            y1: self.height as i32,
        }
    }

    /// The pixel at a column and row, which must be inside.
    fn px(&mut self, x: i32, y: i32) -> &mut [u8] {
        let at = y as usize * self.stride + x as usize * 4;
        &mut self.bytes[at..at + 4]
    }

    /// Coverage in an opaque ink at `alpha` of 255, its origin at (x, y): the pen's whole pixel
    /// and the baseline's row. Each pixel takes `cov × alpha / 255` of the ink, so at 255 it's
    /// the coverage itself.
    pub fn coverage(&mut self, x: i32, y: i32, mask: &Mask, ink: Rgb, alpha: u8, clip: Clip) {
        let (x0, y0) = (x + mask.left, y + mask.top);
        let (w, h) = (mask.width as i32, mask.height as i32);
        let c = clip.meet(self.whole()).meet(Clip {
            x0,
            y0,
            x1: x0 + w,
            y1: y0 + h,
        });
        if c.is_empty() || mask.cov.len() < (w * h) as usize {
            return;
        }
        for row in c.y0..c.y1 {
            for col in c.x0..c.x1 {
                let cov = u32::from(mask.cov[((row - y0) * w + col - x0) as usize]);
                let a = cov * u32::from(alpha) / 255;
                match a {
                    0 => {}
                    255 => self
                        .px(col, row)
                        .copy_from_slice(&[ink.0, ink.1, ink.2, 255]),
                    a => blend(self.px(col, row), ink, a),
                }
            }
        }
    }

    /// Premultiplied pixels, `w × 4` bytes a row, over what's there, at (x, y).
    pub fn over(&mut self, x: i32, y: i32, w: u32, h: u32, pixels: &[u8], clip: Clip) {
        let (w, h) = (w as i32, h as i32);
        let c = clip.meet(self.whole()).meet(Clip {
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
        });
        if c.is_empty() || pixels.len() < (w * h) as usize * 4 {
            return;
        }
        for row in c.y0..c.y1 {
            for col in c.x0..c.x1 {
                let at = ((row - y) * w + col - x) as usize * 4;
                let from = &pixels[at..at + 4];
                let rest = 255 - u32::from(from[3]);
                let px = self.px(col, row);
                for (to, value) in px.iter_mut().zip(from) {
                    *to = (u32::from(*value) + (u32::from(*to) * rest + 127) / 255).min(255) as u8;
                }
            }
        }
    }

    /// A rectangle in an ink at `alpha` of 255: a copy at 255, else blended over.
    pub fn fill(&mut self, r: Clip, ink: Rgb, alpha: u8) {
        let c = r.meet(self.whole());
        if c.is_empty() || alpha == 0 {
            return;
        }
        for row in c.y0..c.y1 {
            for col in c.x0..c.x1 {
                match alpha {
                    255 => self
                        .px(col, row)
                        .copy_from_slice(&[ink.0, ink.1, ink.2, 255]),
                    a => blend(self.px(col, row), ink, u32::from(a)),
                }
            }
        }
    }
}

/// One premultiplied pixel: `ink` over what's there, `a` of 255 of it.
pub fn blend(px: &mut [u8], ink: Rgb, a: u32) {
    let rest = 255 - a;
    for (to, from) in px.iter_mut().zip([ink.0, ink.1, ink.2, 255]) {
        *to = ((u32::from(from) * a + u32::from(*to) * rest + 127) / 255) as u8;
    }
}

/// How far each glyph's outline grows a side, in physical pixels for each physical pixel of its
/// size: a Mac draws text with font smoothing on, and CoreText's smoothing grows a glyph in
/// proportion to its size, the more the lighter its colour, up to a limit. Dark text (a
/// luminance of [`GROW_DARK_UNDER`] or less) grows the least, light text (from
/// [`GROW_LIGHT_FROM`]) the most, and what's between in proportion. Measured (for die-markdown's
/// page) against CoreText drawing the same text in three palettes at scales 1, 1.5 and 2.
const GROW_DARK: f32 = 0.0068;
const GROW_LIGHT: f32 = 0.0145;
const GROW_DARK_UNDER: f32 = 0.1;
const GROW_LIGHT_FROM: f32 = 0.585;
/// The most a glyph grows a side, in physical pixels.
const GROW_MOST: f32 = 0.345;

/// How far a glyph `px` physical pixels an em drawn in `ink` grows a side, in physical pixels:
/// as far as a Mac's font smoothing grows it, by its size and how light its colour is.
pub fn smoothing(px: f32, ink: Rgb) -> f32 {
    let part = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let light = 0.2126 * part(ink.0) + 0.7152 * part(ink.1) + 0.0722 * part(ink.2);
    let span = GROW_LIGHT_FROM - GROW_DARK_UNDER;
    let lighter = ((light - GROW_DARK_UNDER) / span).clamp(0.0, 1.0);
    let per_px = GROW_DARK + (GROW_LIGHT - GROW_DARK) * lighter;
    (per_px * px.max(0.0)).min(GROW_MOST)
}
