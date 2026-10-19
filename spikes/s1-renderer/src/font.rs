//! Core Text: load the font from files (no install), cell metrics, per-character
//! fallback, and rasterization of one glyph into a gray or BGRA bitmap.

use objc2_core_foundation::{CFArray, CFRange, CFRetained, CFString, CFURL, CFURLPathStyle, CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_core_text::{CTFont, CTFontDescriptor, CTFontManagerCreateFontDescriptorsFromURL, CTFontOrientation, CTFontSymbolicTraits};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

pub struct Face { pub font: CFRetained<CTFont>, pub color: bool, pub name: String }

#[derive(Clone, Copy, Debug)]
pub struct Metrics { pub cell_w: u32, pub cell_h: u32, pub baseline: f64, pub ascent: f64, pub descent: f64, pub leading: f64, pub advance: f64, pub ul_pos: f64, pub ul_thick: f64 }

pub struct FontSet {
    pub px: f64,
    pub faces: Vec<Face>, // 0..4: regular, bold, italic, bold-italic; then fallbacks
    pub files: [PathBuf; 4],
    pub metrics: Metrics,
    cache: HashMap<(char, u8), (usize, u16)>,
    pub fallback_ms: f64,
}

pub struct Raster { pub w: u32, pub h: u32, pub left: i32, pub top: i32, pub color: bool, pub data: Vec<u8> }

/// Where the four faces come from. `term` is Ioskeley Mono Term (DIE's default).
pub fn font_files(dir: &Path, prefix: &str) -> [PathBuf; 4] {
    ["Regular", "Bold", "Italic", "BoldItalic"].map(|s| dir.join(format!("{prefix}-{s}.ttf")))
}

pub fn load_face_file(path: &Path, px: f64) -> Option<Face> {
    let url = CFURL::with_file_system_path(None, Some(&CFString::from_str(path.to_str()?)), CFURLPathStyle::CFURLPOSIXPathStyle, false)?;
    let arr: CFRetained<CFArray> = unsafe { CTFontManagerCreateFontDescriptorsFromURL(&url) }?;
    if arr.count() == 0 { return None; }
    let desc = unsafe { &*(arr.value_at_index(0) as *const CTFontDescriptor) };
    let font = unsafe { CTFont::with_font_descriptor(desc, px, std::ptr::null()) };
    Some(face_of(font))
}

fn face_of(font: CFRetained<CTFont>) -> Face {
    let color = unsafe { font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitColorGlyphs);
    let name = unsafe { font.post_script_name() }.to_string();
    Face { font, color, name }
}

pub fn glyph_of(font: &CTFont, ch: char) -> u16 {
    let mut u = [0u16; 2];
    let s = ch.encode_utf16(&mut u);
    let mut g = [0u16; 2];
    let ok = unsafe { font.glyphs_for_characters(NonNull::new(s.as_mut_ptr()).unwrap(), NonNull::new(g.as_mut_ptr()).unwrap(), s.len() as isize) };
    if ok { g[0] } else { 0 }
}

pub fn advance_of(font: &CTFont, g: u16) -> f64 {
    let mut adv = CGSize { width: 0.0, height: 0.0 };
    let gg = [g];
    unsafe { font.advances_for_glyphs(CTFontOrientation::Horizontal, NonNull::new(gg.as_ptr() as *mut u16).unwrap(), &mut adv, 1) };
    adv.width
}

impl FontSet {
    pub fn new(files: &[PathBuf; 4], pt: f64, scale: f64) -> Result<FontSet, String> {
        let px = pt * scale;
        let mut faces = vec![];
        for f in files {
            faces.push(load_face_file(f, px).ok_or_else(|| format!("cannot load font file {}", f.display()))?);
        }
        let r = &faces[0].font;
        let (ascent, descent, leading) = unsafe { (r.ascent(), r.descent(), r.leading()) };
        let advance = advance_of(r, glyph_of(r, 'M'));
        let (ul_pos, ul_thick) = unsafe { (r.underline_position(), r.underline_thickness()) };
        // Cell height includes the leading so the font's own box/block glyphs
        // (which span ascent..-(descent+leading)) tile; width is floor(advance).
        let metrics = Metrics {
            cell_w: (advance + 1e-3).floor() as u32,
            cell_h: (ascent + descent + leading).round() as u32,
            baseline: ascent, ascent, descent, leading, advance, ul_pos, ul_thick,
        };
        Ok(FontSet { px, faces, files: files.clone(), metrics, cache: HashMap::new(), fallback_ms: 0.0 })
    }

    /// (face index, glyph id) for a character in a style (bold=1, italic=2).
    /// Order: the styled primary face, then the regular face, then Core Text's
    /// system fallback (CTFontCreateForString), cached per character.
    pub fn lookup(&mut self, ch: char, style: u8) -> (usize, u16) {
        if let Some(&v) = self.cache.get(&(ch, style)) { return v; }
        let primary = (style & 3) as usize;
        let mut res = (primary, glyph_of(&self.faces[primary].font, ch));
        if res.1 == 0 && primary != 0 { res = (0, glyph_of(&self.faces[0].font, ch)); }
        if res.1 == 0 {
            let t = crate::stats::now_s();
            let mut u = [0u16; 2];
            let n = ch.encode_utf16(&mut u).len();
            let s = CFString::from_str(&ch.to_string());
            let f = unsafe { self.faces[primary].font.for_string(&s, CFRange { location: 0, length: n as isize }) };
            let name = unsafe { f.post_script_name() }.to_string();
            let idx = match self.faces.iter().position(|x| x.name == name) {
                Some(i) => i,
                None => { self.faces.push(face_of(f)); self.faces.len() - 1 }
            };
            let g = glyph_of(&self.faces[idx].font, ch);
            res = if g != 0 { (idx, g) } else { (0, 0) };
            self.fallback_ms += (crate::stats::now_s() - t) * 1e3;
        }
        self.cache.insert((ch, style), res);
        res
    }

    /// Rasterizes a glyph positioned in a cell span (1 or 2 cells). Returns
    /// None for blank glyphs. Glyphs wider than their span are scaled down to
    /// fit (fallback fonts, emoji); the baseline is the primary font's.
    pub fn rasterize(&self, face: usize, g: u16, span: u32) -> Option<Raster> {
        let f = &self.faces[face];
        let m = &self.metrics;
        let gg = [g];
        let b: CGRect = unsafe { f.font.bounding_rects_for_glyphs(CTFontOrientation::Horizontal, NonNull::new(gg.as_ptr() as *mut u16).unwrap(), std::ptr::null_mut(), 1) };
        if b.size.width <= 0.0 || b.size.height <= 0.0 { return None; }
        let span_w = (span * m.cell_w) as f64;
        let adv = advance_of(&f.font, g);
        let ink_w = b.origin.x + b.size.width;
        let s = if adv > span_w + 0.5 || (face >= 4 && ink_w > span_w + 0.5) { span_w / adv.max(ink_w) } else { 1.0 };
        let x_off = if s < 1.0 || face >= 4 { ((span_w - adv * s) / 2.0).max(0.0) } else { 0.0 };
        // vertical: keep the baseline; if a scaled/fallback glyph is taller than the
        // cell, centre it vertically instead.
        let (mut base_y, cell_h) = (m.baseline, m.cell_h as f64);
        let ink_top = base_y - (b.origin.y + b.size.height) * s;
        let ink_bot = base_y - b.origin.y * s;
        if face >= 4 && (ink_top < 0.0 || ink_bot > cell_h) { base_y += (cell_h - (ink_bot + ink_top)) / 2.0; }
        let left = x_off + b.origin.x * s;
        let top = base_y - (b.origin.y + b.size.height) * s;
        let (l, t) = (left.floor() as i32 - 1, top.floor() as i32 - 1);
        let (r, btm) = ((left + b.size.width * s).ceil() as i32 + 1, (top + b.size.height * s).ceil() as i32 + 1);
        let (w, h) = ((r - l) as usize, (btm - t) as usize);
        let bpp = if f.color { 4 } else { 1 };
        let mut data = vec![0u8; w * h * bpp];
        let ctx = unsafe {
            if f.color {
                let cs = CGColorSpace::new_device_rgb().unwrap();
                CGBitmapContextCreate(data.as_mut_ptr() as *mut _, w, h, 8, w * 4, Some(&cs), CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0)
            } else {
                let cs = CGColorSpace::new_device_gray().unwrap();
                CGBitmapContextCreate(data.as_mut_ptr() as *mut _, w, h, 8, w, Some(&cs), CGImageAlphaInfo::None.0)
            }
        }?;
        let c = Some(&*ctx);
        CGContext::set_allows_antialiasing(c, true);
        CGContext::set_should_antialias(c, true);
        CGContext::set_allows_font_smoothing(c, true);
        CGContext::set_should_smooth_fonts(c, false);
        CGContext::set_allows_font_subpixel_positioning(c, true);
        CGContext::set_should_subpixel_position_fonts(c, true);
        CGContext::set_should_subpixel_quantize_fonts(c, false);
        if !f.color { CGContext::set_gray_fill_color(c, 1.0, 1.0); }
        // pen position in bitmap space (origin bottom-left)
        let pen = CGPoint { x: x_off - l as f64, y: btm as f64 - base_y };
        CGContext::translate_ctm(c, pen.x, pen.y);
        CGContext::scale_ctm(c, s as CGFloat, s as CGFloat);
        let _ = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };
        let pos = [CGPoint { x: 0.0, y: 0.0 }];
        unsafe { f.font.draw_glyphs(NonNull::new(gg.as_ptr() as *mut u16).unwrap(), NonNull::new(pos.as_ptr() as *mut CGPoint).unwrap(), 1, &ctx) };
        drop(ctx);
        Some(Raster { w: w as u32, h: h as u32, left: l, top: t, color: f.color, data })
    }
}
