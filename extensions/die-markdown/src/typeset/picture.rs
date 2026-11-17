//! Pictures: a PNG file decoded to premultiplied RGBA, and a picture drawn at another size.

use super::Picture;
use std::path::Path;

/// Reads and decodes a PNG of any colour type and depth.
pub(super) fn load(path: &Path) -> Result<Picture, String> {
    let wrong = |what: &dyn std::fmt::Display| format!("{}: {what}", path.display());
    let bytes = std::fs::read(path).map_err(|e| wrong(&e))?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // Palettes, grey levels and sixteen bits a channel all come out as eight-bit grey or RGB,
    // with alpha where the file has any.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| wrong(&e))?;
    let size = reader
        .output_buffer_size()
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
    Ok(Picture {
        width: info.width,
        height: info.height,
        pixels,
    })
}

/// For each of `to` pixels along one axis, the first of `from` source pixels it's made of and
/// how much of each: the mean of what it covers when the picture shrinks, between the two
/// nearest when it grows.
fn taps(from: usize, to: usize) -> Vec<(usize, Vec<f32>)> {
    let ratio = from as f32 / to as f32;
    let tap = |i: usize| {
        if ratio > 1.0 {
            let (a, b) = (i as f32 * ratio, (i + 1) as f32 * ratio);
            let first = (a.floor() as usize).min(from - 1);
            let last = (b.ceil() as usize).clamp(first + 1, from);
            let share = |s: usize| (((s + 1) as f32).min(b) - (s as f32).max(a)).max(0.0);
            let weights: Vec<f32> = (first..last).map(share).collect();
            let sum: f32 = weights.iter().sum();
            (first, weights.iter().map(|w| w / sum.max(1e-6)).collect())
        } else {
            let centre = ((i as f32 + 0.5) * ratio - 0.5).max(0.0);
            let first = centre.floor() as usize;
            if first + 1 >= from {
                return (from - 1, vec![1.0]);
            }
            let t = centre - first as f32;
            (first, vec![1.0 - t, t])
        }
    };
    (0..to).map(tap).collect()
}

/// A picture's pixels at `w` by `h`: across, then down.
pub(super) fn scaled(picture: &Picture, w: u32, h: u32) -> Vec<u8> {
    let (sw, sh) = (picture.width as usize, picture.height as usize);
    let (w, h) = (w as usize, h as usize);
    if sw == 0 || sh == 0 || w == 0 || h == 0 || picture.pixels.len() < sw * sh * 4 {
        return vec![0; w * h * 4];
    }
    let mut wide = vec![0u8; w * sh * 4];
    let across = taps(sw, w);
    for (row, out) in picture
        .pixels
        .chunks_exact(sw * 4)
        .zip(wide.chunks_exact_mut(w * 4))
    {
        for ((first, weights), px) in across.iter().zip(out.chunks_exact_mut(4)) {
            let mut sum = [0.0f32; 4];
            for (source, weight) in row[first * 4..].chunks_exact(4).zip(weights) {
                for (total, value) in sum.iter_mut().zip(source) {
                    *total += f32::from(*value) * weight;
                }
            }
            for (to, total) in px.iter_mut().zip(sum) {
                *to = (total + 0.5) as u8;
            }
        }
    }
    let mut out = vec![0u8; w * h * 4];
    let down = taps(sh, h);
    let mut sums = vec![0.0f32; w * 4];
    for ((first, weights), row) in down.iter().zip(out.chunks_exact_mut(w * 4)) {
        sums.fill(0.0);
        for (source, weight) in wide[first * w * 4..].chunks_exact(w * 4).zip(weights) {
            for (total, value) in sums.iter_mut().zip(source) {
                *total += f32::from(*value) * weight;
            }
        }
        for (to, total) in row.iter_mut().zip(&sums) {
            *to = (total + 0.5) as u8;
        }
    }
    out
}
