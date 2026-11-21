//! A premultiplied RGBA8 target with clipping: coverage in an ink, pictures over what's there,
//! and filled rectangles, in die-markdown's paint's arithmetic: source-over on the stored bytes
//! with coverage as it is, as DIE draws its own text, rounded to the nearest. On an opaque
//! target what's drawn keeps it opaque; on a transparent one it's correct premultiplied alpha.

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
    /// The pixels `x0..x1` by `y0..y1`.
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Clip {
        Clip { x0, y0, x1, y1 }
    }

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

    /// `width × height` pixels, `stride` bytes a row. None if a row doesn't fit its stride,
    /// `bytes` is too short, or a side is past `i32::MAX`.
    pub fn with_stride(
        bytes: &'a mut [u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> Option<Pixels<'a>> {
        let row = width as usize * 4;
        let needed = match height {
            0 => Some(0),
            h => stride
                .checked_mul(h as usize - 1)
                .and_then(|n| n.checked_add(row)),
        };
        let fits = needed.is_some_and(|n| bytes.len() >= n);
        let small = i32::try_from(width).is_ok() && i32::try_from(height).is_ok();
        (stride >= row && fits && small).then_some(Pixels {
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

    /// The bytes of a stretch of a row, columns `x0..x1`, both inside.
    fn span(&mut self, row: i32, x0: i32, x1: i32) -> &mut [u8] {
        let at = row as usize * self.stride;
        &mut self.bytes[at + x0 as usize * 4..at + x1 as usize * 4]
    }

    /// Coverage in an opaque ink at `alpha` of 255, its origin at (x, y): the pen's whole pixel
    /// and the baseline's row. Each pixel takes `cov × alpha / 255` of the ink, rounded, so at
    /// 255 it's the coverage itself, as die-markdown draws its text.
    pub fn coverage(&mut self, x: i32, y: i32, mask: &Mask, ink: Rgb, alpha: u8, clip: Clip) {
        let (x0, y0) = (x.saturating_add(mask.left), y.saturating_add(mask.top));
        let (w, h) = (mask.width as i32, mask.height as i32);
        let c = clip.meet(self.whole()).meet(Clip {
            x0,
            y0,
            x1: x0.saturating_add(w),
            y1: y0.saturating_add(h),
        });
        if c.is_empty() || alpha == 0 || mask.cov.len() < w as usize * h as usize {
            return;
        }
        let alpha = u32::from(alpha);
        for row in c.y0..c.y1 {
            let line = (row - y0) as usize * w as usize;
            let cov = &mask.cov[line + (c.x0 - x0) as usize..line + (c.x1 - x0) as usize];
            let out = self.span(row, c.x0, c.x1);
            for (px, cov) in out.chunks_exact_mut(4).zip(cov) {
                let a = match alpha {
                    255 => u32::from(*cov),
                    alpha => (u32::from(*cov) * alpha + 127) / 255,
                };
                match a {
                    0 => {}
                    255 => px.copy_from_slice(&[ink.0, ink.1, ink.2, 255]),
                    a => blend(px, ink, a),
                }
            }
        }
    }

    /// Premultiplied pixels, `w × 4` bytes a row, over what's there, at (x, y): each channel
    /// `source + target × (255 − source alpha) / 255`, alpha too, so an opaque target stays
    /// opaque and a transparent one takes the picture's own alpha.
    pub fn over(&mut self, x: i32, y: i32, w: u32, h: u32, pixels: &[u8], clip: Clip) {
        let (Ok(wi), Ok(hi)) = (i32::try_from(w), i32::try_from(h)) else {
            return;
        };
        let c = clip.meet(self.whole()).meet(Clip {
            x0: x,
            y0: y,
            x1: x.saturating_add(wi),
            y1: y.saturating_add(hi),
        });
        if c.is_empty() || pixels.len() < w as usize * h as usize * 4 {
            return;
        }
        for row in c.y0..c.y1 {
            let line = (row - y) as usize * w as usize * 4;
            let from = &pixels[line + (c.x0 - x) as usize * 4..line + (c.x1 - x) as usize * 4];
            let out = self.span(row, c.x0, c.x1);
            for (px, from) in out.chunks_exact_mut(4).zip(from.chunks_exact(4)) {
                let rest = 255 - u32::from(from[3]);
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
            for px in self.span(row, c.x0, c.x1).chunks_exact_mut(4) {
                match alpha {
                    255 => px.copy_from_slice(&[ink.0, ink.1, ink.2, 255]),
                    a => blend(px, ink, u32::from(a)),
                }
            }
        }
    }
}

/// One premultiplied pixel: `ink` over what's there, `a` of 255 of it; more than 255 is all of
/// it.
pub fn blend(px: &mut [u8], ink: Rgb, a: u32) {
    let a = a.min(255);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One premultiplied pixel as die-markdown's paint blended it before die-draw.
    fn markdowns(px: [u8; 4], ink: Rgb, cov: u8) -> [u8; 4] {
        let a = u32::from(cov);
        let mut out = px;
        if a == 255 {
            return [ink.0, ink.1, ink.2, 255];
        }
        for (to, from) in out.iter_mut().zip([ink.0, ink.1, ink.2, 255]) {
            *to = ((u32::from(from) * a + u32::from(*to) * (255 - a) + 127) / 255) as u8;
        }
        out
    }

    fn solid(width: u32, height: u32, cov: u8) -> Mask {
        Mask {
            left: 0,
            top: 0,
            width,
            height,
            cov: vec![cov; (width * height) as usize],
        }
    }

    #[test]
    fn coverage_at_full_alpha_is_markdowns_and_an_alpha_takes_its_share() {
        let ink = Rgb(200, 100, 50);
        let every = Mask {
            left: 0,
            top: 0,
            width: 256,
            height: 1,
            cov: (0..=255).collect(),
        };
        // Every coverage, over an opaque, a transparent and a half-covered ground.
        for ground in [[10, 20, 30, 255], [0, 0, 0, 0], [40, 40, 40, 128]] {
            let mut bytes = ground.repeat(256);
            let mut to = Pixels::new(&mut bytes, 256, 1).unwrap();
            let whole = to.whole();
            to.coverage(0, 0, &every, ink, 255, whole);
            for (cov, px) in bytes.chunks_exact(4).enumerate() {
                assert_eq!(px, markdowns(ground, ink, cov as u8), "{ground:?} at {cov}");
            }
            // At an alpha, each pixel takes `cov × alpha / 255` of the ink, rounded.
            let mut faded = ground.repeat(256);
            let mut to = Pixels::new(&mut faded, 256, 1).unwrap();
            to.coverage(0, 0, &every, ink, 100, whole);
            for (cov, px) in faded.chunks_exact(4).enumerate() {
                let share = ((cov as u32 * 100 + 127) / 255) as u8;
                assert_eq!(px, markdowns(ground, ink, share), "{ground:?} at {cov}");
            }
        }
        // Half of white on nothing is white at half alpha, premultiplied; none is nothing.
        let mut bytes = vec![0; 8];
        let mut to = Pixels::new(&mut bytes, 2, 1).unwrap();
        to.coverage(0, 0, &solid(1, 1, 255), Rgb(255, 255, 255), 128, to.whole());
        to.coverage(1, 0, &solid(1, 1, 255), Rgb(255, 255, 255), 0, to.whole());
        assert_eq!(bytes, [128, 128, 128, 128, 0, 0, 0, 0]);
        // A picture over an opaque ground leaves it opaque, over nothing keeps its own alpha.
        let picture = [100, 50, 0, 128];
        let mut bytes = [10, 20, 30, 255, 0, 0, 0, 0];
        let mut to = Pixels::new(&mut bytes, 2, 1).unwrap();
        to.over(0, 0, 1, 1, &picture, to.whole());
        to.over(1, 0, 1, 1, &picture, to.whole());
        assert_eq!(bytes, [105, 60, 15, 255, 100, 50, 0, 128]);
        // A filled rectangle: a copy at 255, a blend below.
        let mut bytes = [0, 0, 0, 255, 0, 0, 0, 255];
        let mut to = Pixels::new(&mut bytes, 2, 1).unwrap();
        let (left, right) = (Clip::new(0, 0, 1, 1), Clip::new(1, 0, 2, 1));
        to.fill(left, ink, 255);
        to.fill(right, ink, 51);
        assert_eq!(bytes, [200, 100, 50, 255, 40, 20, 10, 255]);
        // A blend of more than all of the ink is all of it.
        for a in [256, 1000, u32::MAX] {
            let mut px = [10, 20, 30, 40];
            blend(&mut px, ink, a);
            assert_eq!(px, [200, 100, 50, 255], "at {a}");
        }
    }

    #[test]
    fn everything_drawn_stays_in_the_clip_and_the_buffer() {
        // 4 × 4 pixels, 5 a row in the bytes: the fifth is padding, never touched.
        let mut bytes = vec![9u8; 5 * 4 * 4];
        let mut to = Pixels::with_stride(&mut bytes, 4, 4, 20).unwrap();
        let ink = Rgb(1, 2, 3);
        let clip = Clip::new(0, 1, 2, 4);
        // A 3 × 3 mask from (-1, -1): its rows 1 and its columns 1 to 2 are in the clip.
        to.coverage(-1, -1, &solid(3, 3, 255), ink, 255, clip);
        // A 2 × 2 picture at (3, 3), half off the buffer's edge: only (3, 3) is drawn.
        to.over(3, 3, 2, 2, &[255; 16], to.whole());
        // A rectangle past every edge, clipped to the buffer: row 0 from column 2.
        to.fill(Clip::new(2, -5, 99, 1), ink, 255);
        let drawn = |x: usize, y: usize| match (x, y) {
            (0 | 1, 1) => Some([1, 2, 3, 255]),
            (3, 3) => Some([255; 4]),
            (2 | 3, 0) => Some([1, 2, 3, 255]),
            _ => None,
        };
        for y in 0..4 {
            for x in 0..5 {
                let at = y * 20 + x * 4;
                let want = drawn(x, y).filter(|_| x < 4).unwrap_or([9; 4]);
                assert_eq!(bytes[at..at + 4], want, "({x}, {y})");
            }
        }
        // Clips meet; an empty one draws nothing; a buffer too short or a stride too narrow is
        // no buffer.
        assert_eq!(clip.meet(Clip::new(1, 0, 9, 2)), Clip::new(1, 1, 2, 2));
        assert!(Clip::new(3, 0, 3, 9).is_empty() && !clip.is_empty());
        assert!(clip.meet(Clip::new(5, 5, 9, 9)).is_empty());
        let mut short = vec![0u8; 4 * 4 * 4 - 1];
        assert!(Pixels::new(&mut short, 4, 4).is_none());
        assert!(Pixels::with_stride(&mut bytes, 4, 4, 15).is_none());
        let mut none: [u8; 0] = [];
        assert!(Pixels::new(&mut none, 0, 0).is_some());
    }
}
