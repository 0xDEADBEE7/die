//! Pictures as PNG: read into premultiplied RGBA8, and written from straight RGBA8. The reader
//! is die-markdown's, lifted.

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

/// The most bytes a picture decodes to as RGBA8: 256 MiB, a square 8192 pixels a side. A
/// file's header can claim any size in a few bytes, and the png crate's own limits don't cover
/// the buffer its pixels are read into.
pub const LARGEST: u64 = 256 << 20;

/// A PNG's picture, premultiplied, from a PNG of any colour type and depth. One that would be
/// more than [`LARGEST`] bytes is "too large to decode", before any of its pixels are read.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let wrong = |what: &dyn std::fmt::Display| what.to_string();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // Palettes, grey levels and sixteen bits a channel all come out as eight-bit grey or RGB,
    // with alpha where the file has any.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| wrong(&e))?;
    let (width, height) = reader.info().size();
    let size = reader
        .output_buffer_size()
        .filter(|_| {
            let count = u64::from(width) * u64::from(height);
            count.checked_mul(4).is_some_and(|n| n <= LARGEST)
        })
        .ok_or_else(|| wrong(&"too large to decode"))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(|e| wrong(&e))?;
    let data = buffer
        .get(..info.buffer_size())
        .ok_or_else(|| wrong(&"cut short"))?;
    let count = info.width as usize * info.height as usize;
    let mut pixels = Vec::with_capacity(count * 4);
    let over = |c: u8, a: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
    match info.color_type {
        png::ColorType::Rgba => {
            for p in data.chunks_exact(4) {
                pixels.extend([over(p[0], p[3]), over(p[1], p[3]), over(p[2], p[3]), p[3]]);
            }
        }
        png::ColorType::Rgb => {
            for p in data.chunks_exact(3) {
                pixels.extend([p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in data.chunks_exact(2) {
                let grey = over(p[0], p[1]);
                pixels.extend([grey, grey, grey, p[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for grey in data {
                pixels.extend([*grey, *grey, *grey, 255]);
            }
        }
        png::ColorType::Indexed => return Err(wrong(&"its palette wasn't expanded")),
    }
    if count == 0 || pixels.len() != count * 4 {
        return Err(wrong(&"no pixels"));
    }
    Ok(Image {
        width: info.width,
        height: info.height,
        pixels,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_written_reads_back_premultiplied_and_demultiplies_to_what_was_written() {
        // Opaque red, half blue, nothing, and a fifth of an orange.
        let straight = [
            255, 0, 0, 255, 0, 0, 255, 128, 10, 20, 30, 0, 200, 100, 50, 51,
        ];
        let png = encode(2, 2, &straight).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let image = decode(&png).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        let premultiplied = [255, 0, 0, 255, 0, 0, 128, 128, 0, 0, 0, 0, 40, 20, 10, 51];
        assert_eq!(image.pixels, premultiplied);
        let mut back = image.pixels.clone();
        demultiply(&mut back);
        assert_eq!(
            back,
            [255, 0, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0, 200, 100, 50, 51]
        );
        // What isn't a PNG is an error; a file's errors name it.
        assert!(decode(b"not a png").is_err());
        let missing = Path::new("/nonexistent/die-draw.png");
        let why = load(missing).unwrap_err();
        assert!(why.starts_with("/nonexistent/die-draw.png: "), "{why}");
    }

    #[test]
    fn a_png_claiming_a_picture_too_large_is_refused_before_its_pixels_are_made_room_for() {
        // A header of 100,000 pixels a side, 40 GB as RGBA, in a file of a few bytes.
        let mut claim = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut claim, 100_000, 100_000);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_chunk(png::chunk::IDAT, &[0x78, 0x9c]).unwrap();
        }
        assert!(claim.len() < 100);
        assert_eq!(decode(&claim).unwrap_err(), "too large to decode");
    }
}
