//! Pictures as PNG: read into premultiplied RGBA8, and written from straight RGBA8.
//!
//! The skeleton writes PNGs and reads none: the lift from die-markdown brings the reader.

use std::path::Path;

/// A picture: premultiplied RGBA8, `width × 4` bytes a row.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Image {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its pixels, rows top first.
    pub pixels: Vec<u8>,
}

/// A PNG's picture, premultiplied.
pub fn decode(_bytes: &[u8]) -> Result<Image, String> {
    Err("die-draw reads no PNG yet".into())
}

/// The picture in the PNG file at `path`. An error says `{path}: {why}`.
pub fn load(path: &Path) -> Result<Image, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A PNG of straight (not premultiplied) RGBA8 pixels, `width × 4` bytes a row.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Premultiplied RGBA8 back to straight, in place, rounding as tiny-skia does. A pixel at
/// alpha 0 or 255 is left as it is.
pub fn demultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 || a == 255 {
            continue;
        }
        let a = f64::from(a) / 255.0;
        for c in &mut px[..3] {
            *c = (f64::from(*c) / a + 0.5).min(255.0) as u8;
        }
    }
}
