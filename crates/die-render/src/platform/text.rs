//! Core Text: a font face loaded from a file (nothing is installed), its metrics,
//! and glyph lookup. R2 adds fallback and rasterization.

use objc2_core_foundation::{CFArray, CFRetained, CFString, CFURL, CFURLPathStyle, CGSize};
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
    /// The first face in the font file at `path`, at `px` pixels per em.
    pub fn from_file(path: &Path, px: f64) -> Result<Face, String> {
        let bad = || format!("can't load a font from {}", path.display());
        let s = CFString::from_str(path.to_str().ok_or_else(bad)?);
        let url = CFURL::with_file_system_path(
            None,
            Some(&s),
            CFURLPathStyle::CFURLPOSIXPathStyle,
            false,
        )
        .ok_or_else(bad)?;
        // SAFETY: `url` is a valid CFURL; the call returns a new array or null.
        let descs: CFRetained<CFArray> =
            unsafe { CTFontManagerCreateFontDescriptorsFromURL(&url) }.ok_or_else(bad)?;
        if descs.count() == 0 {
            return Err(bad());
        }
        // SAFETY: every element of the array this call returns is a CTFontDescriptor,
        // and `descs` keeps it alive while it's borrowed.
        let desc = unsafe { &*(descs.value_at_index(0) as *const CTFontDescriptor) };
        // SAFETY: a null matrix means the identity transform.
        let font = unsafe { CTFont::with_font_descriptor(desc, px, std::ptr::null()) };
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
        // SAFETY: plain getters on a live font.
        unsafe {
            FaceMetrics {
                ascent: f.ascent(),
                descent: f.descent(),
                leading: f.leading(),
                underline_position: f.underline_position(),
                underline_thickness: f.underline_thickness(),
            }
        }
    }

    /// The glyph for `ch`, or None if the face doesn't have one.
    pub fn glyph(&self, ch: char) -> Option<u16> {
        let mut chars = [0u16; 2];
        let chars = ch.encode_utf16(&mut chars);
        let mut glyphs = [0u16; 2];
        // SAFETY: both buffers hold `chars.len()` (1 or 2) elements, as the call requires.
        let ok = unsafe {
            self.font.glyphs_for_characters(
                NonNull::from(&mut chars[0]),
                NonNull::from(&mut glyphs[0]),
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
