//! Core Text: a font face loaded from a file (nothing is installed), its metrics,
//! glyph lookup, the system's fallback, shaping and drawing a glyph into a bitmap.
//! What goes where is `crate::font`'s decision: this only reads and draws.

use crate::font::{FaceMetrics, Fit, Ink};
use crate::frame::{GlyphBitmap, Plane};
use objc2_core_foundation::{
    CFArray, CFAttributedString, CFDictionary, CFRange, CFRetained, CFString, CFURL, CGPoint,
    CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontManagerCreateFontDescriptorsFromURL, CTFontOrientation,
    CTFontSymbolicTraits, CTLine, CTRun, kCTFontAttributeName,
};
use std::path::Path;
use std::ptr::{NonNull, null_mut};

/// One font face at one pixel size.
pub struct Face {
    font: CFRetained<CTFont>,
}

impl Face {
    /// The first face in the font file at `path`, at `px` pixels per em. Core Text
    /// would take a size of 0 to mean 12, so a size that isn't positive is an error.
    pub fn from_file(path: &Path, px: f64) -> Result<Face, String> {
        if !(px > 0.0 && px.is_finite()) {
            return Err(format!("font size {px} isn't a positive size"));
        }
        let bad = || format!("can't load a font from {}", path.display());
        let url = CFURL::from_file_path(path).ok_or_else(bad)?;
        // SAFETY: `url` is a valid CFURL; the call returns a new array or null.
        let descs = unsafe { CTFontManagerCreateFontDescriptorsFromURL(&url) }.ok_or_else(bad)?;
        // SAFETY: every element of the array that call returns is a CTFontDescriptor.
        let descs: CFRetained<CFArray<CTFontDescriptor>> =
            unsafe { CFRetained::cast_unchecked(descs) };
        // Bounds-checked, and retained, so its lifetime isn't tied to `descs`.
        let desc = descs.get(0).ok_or_else(bad)?;
        // SAFETY: a null matrix means the identity transform.
        let font = unsafe { CTFont::with_font_descriptor(&desc, px, std::ptr::null()) };
        Ok(Face { font })
    }

    /// The face's PostScript name.
    pub fn postscript_name(&self) -> String {
        // SAFETY: a plain getter on a live font.
        unsafe { self.font.post_script_name() }.to_string()
    }

    /// Whether the face draws in color (emoji).
    pub fn is_color(&self) -> bool {
        is_color(&self.font)
    }

    /// The face's vertical metrics and underline, in pixels.
    pub fn metrics(&self) -> FaceMetrics {
        let f = &self.font;
        FaceMetrics {
            // SAFETY: a plain getter on a live font.
            ascent: unsafe { f.ascent() },
            // SAFETY: a plain getter on a live font.
            descent: unsafe { f.descent() },
            // SAFETY: a plain getter on a live font.
            leading: unsafe { f.leading() },
            // SAFETY: a plain getter on a live font.
            underline_position: unsafe { f.underline_position() },
            // SAFETY: a plain getter on a live font.
            underline_thickness: unsafe { f.underline_thickness() },
            // SAFETY: a plain getter on a live font.
            x_height: unsafe { f.x_height() },
        }
    }

    /// The glyph for `ch`, or None if the face doesn't have one.
    pub fn glyph(&self, ch: char) -> Option<u16> {
        let mut chars = [0u16; 2];
        let chars = ch.encode_utf16(&mut chars);
        let mut glyphs = [0u16; 2];
        // SAFETY: both pointers cover `chars.len()` (1 or 2) elements, as the call requires.
        let ok = unsafe {
            self.font.glyphs_for_characters(
                NonNull::from(&mut *chars).cast(),
                NonNull::from(&mut glyphs).cast(),
                chars.len() as isize,
            )
        };
        (ok && glyphs[0] != 0).then_some(glyphs[0])
    }

    /// The horizontal advance of `glyph`, in pixels.
    pub fn advance(&self, glyph: u16) -> f64 {
        let mut adv = CGSize {
            width: 0.0,
            height: 0.0,
        };
        let mut g = glyph;
        // SAFETY: one glyph in, one advance out.
        unsafe {
            self.font.advances_for_glyphs(
                CTFontOrientation::Horizontal,
                NonNull::from(&mut g),
                &mut adv,
                1,
            )
        };
        adv.width
    }

    /// The face the system picks for `text`, keeping this face's style where it can.
    pub fn fallback_for(&self, text: &str) -> Face {
        let string = CFString::from_str(text);
        let range = CFRange {
            location: 0,
            length: text.encode_utf16().count() as isize,
        };
        // SAFETY: `range` is the whole of `string`, in UTF-16 units as Core Foundation
        // counts, and the call returns a new font.
        let font = unsafe { self.font.for_string(&string, range) };
        Face { font }
    }

    /// Lay `text` out with this face, as Core Text does: marks go where the font
    /// says, and a character this face lacks comes from a face Core Text picks.
    pub fn shape(&self, text: &str) -> Shaped {
        let string = CFString::from_str(text);
        // SAFETY: reading a constant the framework exports.
        let key = unsafe { kCTFontAttributeName };
        let attrs = CFDictionary::from_slices(&[key], &[&*self.font]);
        // SAFETY: the default allocator, and a live string and dictionary that map
        // the font attribute's name to a font, which is what the key asks for.
        let attributed =
            unsafe { CFAttributedString::new(None, Some(&string), Some(attrs.as_opaque())) };
        let Some(attributed) = attributed else {
            return Shaped::default();
        };
        // SAFETY: a live attributed string; the call returns a new line.
        let line = unsafe { CTLine::with_attributed_string(&attributed) };
        // SAFETY: a plain getter on a live line, with null for the outputs it skips.
        let advance = unsafe { line.typographic_bounds(null_mut(), null_mut(), null_mut()) };
        // SAFETY: a plain getter on a live line.
        let runs = unsafe { line.glyph_runs() };
        // SAFETY: Core Text documents this array as holding CTRuns.
        let runs: CFRetained<CFArray<CTRun>> = unsafe { CFRetained::cast_unchecked(runs) };
        let runs: Vec<Run> = runs.iter().filter_map(|r| read_run(&r, key)).collect();
        let ink = runs.iter().fold(Ink::default(), |a, r| a.union(r.ink()));
        let plane = if runs.iter().any(|r| is_color(&r.font)) {
            Plane::Color
        } else {
            Plane::Gray
        };
        Shaped {
            runs,
            advance,
            ink,
            plane,
        }
    }
}

fn is_color(font: &CTFont) -> bool {
    // SAFETY: a plain getter on a live font.
    unsafe { font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitColorGlyphs)
}

/// A run's font, glyphs and their positions, or None if it has no glyphs.
fn read_run(run: &CTRun, font_key: &CFString) -> Option<Run> {
    // SAFETY: a plain getter on a live run.
    let count = unsafe { run.glyph_count() };
    let n = usize::try_from(count).ok().filter(|&n| n > 0)?;
    let mut glyphs = vec![0u16; n];
    let mut positions = vec![CGPoint::ZERO; n];
    // A length of 0 asks for the run to its end, which is `n` elements.
    let all = CFRange {
        location: 0,
        length: 0,
    };
    // SAFETY: the buffer holds the `n` glyphs that range asks for.
    unsafe { run.glyphs(all, NonNull::from(&mut *glyphs).cast()) };
    // SAFETY: the buffer holds the `n` positions that range asks for.
    unsafe { run.positions(all, NonNull::from(&mut *positions).cast()) };
    // SAFETY: a plain getter on a live run.
    let attrs = unsafe { run.attributes() };
    // SAFETY: a run's attributes have CFString names, and the font attribute's value
    // is a CTFont, which is the one this looks up.
    let attrs: &CFDictionary<CFString, CTFont> = unsafe { attrs.cast_unchecked() };
    let font = attrs.get(font_key)?;
    Some(Run {
        font,
        glyphs,
        positions,
    })
}

/// A run of glyphs from one font.
struct Run {
    font: CFRetained<CTFont>,
    glyphs: Vec<u16>,
    positions: Vec<CGPoint>,
}

impl Run {
    fn ink(&self) -> Ink {
        let n = self.glyphs.len();
        let mut rects = vec![CGRect::ZERO; n];
        // SAFETY: the glyphs and the rects each hold `n` elements, and Core Text only
        // reads the glyphs, which is why a pointer from a shared reference is enough.
        unsafe {
            self.font.bounding_rects_for_glyphs(
                CTFontOrientation::Horizontal,
                NonNull::from(&*self.glyphs).cast(),
                rects.as_mut_ptr(),
                n as isize,
            )
        };
        let rects = rects.iter().zip(&self.positions);
        rects.fold(Ink::default(), |a, (r, p)| {
            a.union(Ink {
                x: r.origin.x + p.x,
                y: r.origin.y + p.y,
                w: r.size.width,
                h: r.size.height,
            })
        })
    }
}

/// Text laid out by Core Text, ready to draw.
pub struct Shaped {
    runs: Vec<Run>,
    /// How far the text moves the pen, in pixels.
    pub advance: f64,
    /// The rectangle its pixels fall in, from the pen.
    pub ink: Ink,
    /// Which atlas its bitmap goes in.
    pub plane: Plane,
}

impl Default for Shaped {
    fn default() -> Shaped {
        Shaped {
            runs: vec![],
            advance: 0.0,
            ink: Ink::default(),
            plane: Plane::Gray,
        }
    }
}

impl Shaped {
    /// Draw it as `fit` says, into a new bitmap in its plane's format. Gray is coverage
    /// with no smoothing, and color is premultiplied BGRA. `None` if Core Graphics
    /// can't make a bitmap of that size.
    pub fn draw(&self, fit: &Fit) -> Option<GlyphBitmap> {
        let (w, h) = (fit.w as usize, fit.h as usize);
        let (bytes, space, info) = match self.plane {
            Plane::Gray => (
                1,
                CGColorSpace::new_device_gray()?,
                CGImageAlphaInfo::None.0,
            ),
            Plane::Color => (
                4,
                CGColorSpace::new_device_rgb()?,
                CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
            ),
        };
        let mut data = vec![0u8; w * h * bytes];
        // SAFETY: `data` holds `h` rows of `w * bytes` bytes, as the context is told, and
        // it isn't moved or freed until the context is dropped, below.
        let ctx = unsafe {
            CGBitmapContextCreate(
                data.as_mut_ptr().cast(),
                w,
                h,
                8,
                w * bytes,
                Some(&space),
                info,
            )
        }?;
        let c = Some(&*ctx);
        CGContext::set_allows_antialiasing(c, true);
        CGContext::set_should_antialias(c, true);
        CGContext::set_should_smooth_fonts(c, false);
        CGContext::set_allows_font_subpixel_positioning(c, true);
        CGContext::set_should_subpixel_position_fonts(c, true);
        CGContext::set_should_subpixel_quantize_fonts(c, false);
        CGContext::set_gray_fill_color(c, 1.0, 1.0);
        CGContext::translate_ctm(c, fit.pen_x, fit.pen_y);
        CGContext::scale_ctm(c, fit.scale, fit.scale);
        for r in &self.runs {
            // SAFETY: the glyphs and positions each hold the count given, Core Text only
            // reads them, and `ctx` is live.
            unsafe {
                r.font.draw_glyphs(
                    NonNull::from(&*r.glyphs).cast(),
                    NonNull::from(&*r.positions).cast(),
                    r.glyphs.len(),
                    &ctx,
                )
            };
        }
        drop(ctx);
        Some(GlyphBitmap {
            w: fit.w,
            h: fit.h,
            left: fit.left,
            top: fit.top,
            plane: self.plane,
            data,
        })
    }
}
