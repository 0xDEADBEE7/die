//! Core Text: a font face loaded from a file (nothing is installed), its metrics,
//! and glyph lookup. R2 adds fallback and rasterization.

use objc2_core_foundation::{CFArray, CFRetained, CFURL, CGSize};
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontManagerCreateFontDescriptorsFromURL, CTFontOrientation,
    CTFontSymbolicTraits,
};
use std::path::Path;
use std::ptr::NonNull;

/// One font face at one pixel size.
pub struct Face {
    font: CFRetained<CTFont>,
}

/// A face's vertical metrics and underline, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMetrics {
    pub ascent: f64,
    pub descent: f64,
    pub leading: f64,
    pub underline_position: f64,
    pub underline_thickness: f64,
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

    pub fn postscript_name(&self) -> String {
        // SAFETY: a plain getter on a live font.
        unsafe { self.font.post_script_name() }.to_string()
    }

    /// Whether the face draws in color (emoji).
    pub fn is_color(&self) -> bool {
        // SAFETY: a plain getter on a live font.
        unsafe { self.font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitColorGlyphs)
    }

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
}
