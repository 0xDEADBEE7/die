//! Check 9 without a window or screen recording: render a test page into an
//! offscreen Metal texture, read the pixels back, and measure tiling, wide
//! glyph fit and advances. Also writes PNGs for a human look.

use crate::font::{advance_of, glyph_of, FontSet};
use crate::gpu::{new_texture, Gpu, RenderOpts, Target};
use crate::scene::{Grid, Scene, Style, Underline};
use crate::stats::{results_dir, J};
use objc2_metal::{MTLOrigin, MTLPixelFormat, MTLRegion, MTLSize, MTLStorageMode, MTLTexture, MTLTextureUsage};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

const COLS: usize = 40;
const ROWS: usize = 15;

fn page() -> Scene {
    let mut g = Grid::new(COLS, ROWS);
    let w = Style { fg: 0xffffffff, bg: 0x000000ff, ..Default::default() };
    for r in 0..ROWS { g.clear_row(r, 0, COLS, 0x000000ff); }
    g.put(0, 0, &"─".repeat(12), w, COLS);
    for r in 1..4 {
        g.put(r, 0, "│", w, COLS);
        g.put(r, 2, "┼┼┼┼", w, COLS);
        g.put(r, 8, "████", w, COLS);
        g.put(r, 14, "⣿⣿⣿⣿", w, COLS);
        g.put(r, 20, "╬╬╬╬", w, COLS);
        g.put(r, 26, "▒▒▒▒", w, COLS);
    }
    // each test glyph has a blank cell on both sides
    for (col, s) in [(1, "中"), (5, "😀"), (9, "—"), (12, "…"), (15, "→"), (18, "①"), (21, "한")] { g.put(5, col, s, w, COLS); }
    g.put(7, 0, "fn main() { let x = 42; } // — … → ← ≠", w, COLS);
    g.put(8, 0, "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ▁▂▃▄▅▆▇█ ░▒▓ ▖▗▘▙▚▛▜▝▞▟", w, COLS);
    g.put(9, 0, "┌──┬──┐ ╭──╮ ╔══╦══╗ ┏━━┓ ┄┄┈┈", w, COLS);
    g.put(10, 0, "│  │  │ │  │ ║  ║  ║ ┃  ┃ ╱╲╳", w, COLS);
    g.put(11, 0, "└──┴──┘ ╰──╯ ╚══╩══╝ ┗━━┛ ╌╌", w, COLS);
    let mut c = 0;
    for ul in [Underline::Single, Underline::Double, Underline::Curly, Underline::Dotted, Underline::Dashed] {
        c = g.put(12, c, "ulined", Style { ul, ul_color: 0xff5555ff, ..w }, COLS) + 1;
    }
    g.put(13, 0, "\u{E0B0}\u{E0B1}\u{E0B2}\u{E0B3}\u{E0B4}\u{E0B6} powerline", w, COLS);
    g.put(14, 0, "bold ", Style { attrs: crate::scene::attr::BOLD, ..w }, COLS);
    g.put(14, 5, "italic ", Style { attrs: crate::scene::attr::ITALIC, ..w }, COLS);
    Scene { grid: g, cursor: None, placement: None }
}

struct Img { w: usize, h: usize, px: Vec<u8>, cw: usize, ch: usize }
impl Img {
    fn cov(&self, x: usize, y: usize) -> f64 { self.px[(y * self.w + x) * 4 + 2] as f64 / 255.0 } // red of BGRA
    fn ink(&self, x: usize, y: usize) -> bool { let p = &self.px[(y * self.w + x) * 4..][..3]; p.iter().any(|&v| v > 40) }
    /// ink bounding box inside a pixel rect: (x0, y0, x1, y1) exclusive
    fn bbox(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Option<(usize, usize, usize, usize)> {
        let mut b: Option<(usize, usize, usize, usize)> = None;
        for y in y0..y1.min(self.h) { for x in x0..x1.min(self.w) { if self.ink(x, y) {
            b = Some(match b { None => (x, y, x + 1, y + 1), Some((a, bb, c, d)) => (a.min(x), bb.min(y), c.max(x + 1), d.max(y + 1)) });
        } } }
        b
    }
}

fn hline_check(img: &Img, row: usize, c0: usize, c1: usize) -> (f64, usize) {
    let (y0, x0, x1) = (row * img.ch, c0 * img.cw, c1 * img.cw);
    let mut best = (0.0, 0);
    for y in y0..y0 + img.ch {
        let avg = (x0..x1).map(|x| img.cov(x, y)).sum::<f64>() / (x1 - x0) as f64;
        if avg >= 0.9 { let mn = (x0..x1).map(|x| img.cov(x, y)).fold(1.0, f64::min); if best.1 == 0 || mn < best.0 { best.0 = mn; } best.1 += 1; }
    }
    best // (min coverage along the line's core rows, number of core rows)
}

fn vline_check(img: &Img, col0: usize, col1: usize, r0: usize, r1: usize) -> (f64, usize) {
    let (x0, x1, y0, y1) = (col0 * img.cw, col1 * img.cw, r0 * img.ch, r1 * img.ch);
    let mut best = (0.0, 0);
    for x in x0..x1 {
        let avg = (y0..y1).map(|y| img.cov(x, y)).sum::<f64>() / (y1 - y0) as f64;
        if avg >= 0.9 { let mn = (y0..y1).map(|y| img.cov(x, y)).fold(1.0, f64::min); if best.1 == 0 || mn < best.0 { best.0 = mn; } best.1 += 1; }
    }
    best
}

/// Dot centres along y in a braille block: within-cell vs across-boundary spacing.
fn braille_check(img: &Img, c0: usize, c1: usize, r0: usize, r1: usize) -> J {
    let (x0, x1) = (c0 * img.cw, c1 * img.cw);
    let prof: Vec<f64> = (r0 * img.ch..r1 * img.ch).map(|y| (x0..x1).map(|x| img.cov(x, y)).sum::<f64>()).collect();
    let thr = prof.iter().cloned().fold(0.0, f64::max) * 0.3;
    let mut centers = vec![];
    let mut i = 0;
    while i < prof.len() { if prof[i] > thr { let s = i; while i < prof.len() && prof[i] > thr { i += 1; } centers.push((s + i) as f64 / 2.0); } else { i += 1; } }
    let gaps: Vec<f64> = centers.windows(2).map(|w| w[1] - w[0]).collect();
    let (mn, mx) = (gaps.iter().cloned().fold(f64::MAX, f64::min), gaps.iter().cloned().fold(0.0, f64::max));
    let ch = img.ch as f64;
    let (mut within, mut across) = (vec![], vec![]);
    for w in centers.windows(2) { if (w[0] / ch).floor() == (w[1] / ch).floor() { within.push(w[1] - w[0]); } else { across.push(w[1] - w[0]); } }
    let avg = |v: &Vec<f64>| v.iter().sum::<f64>() / v.len().max(1) as f64;
    // a dot crossing a cell boundary would show up as a run spanning k*ch
    let crossing = centers.iter().any(|c| { let m = c % img.ch as f64; m < 1.0 || m > img.ch as f64 - 1.0 });
    // "tiles": 4 dot rows per cell, the same in every cell, none crossing a cell edge.
    // "even": the gap across a cell edge matches the pitch inside a cell (within 15%).
    J::obj(vec![("dot_rows", J::N(centers.len() as f64)), ("pitch_within_cell_px", J::N(avg(&within))), ("pitch_across_cells_px", J::N(avg(&across))),
        ("spacing_min_px", J::N(mn)), ("spacing_max_px", J::N(mx)), ("dot_on_boundary", J::B(crossing)),
        ("tiles", J::B(centers.len() == 4 * (r1 - r0) && !crossing)), ("even", J::B(mx / mn.max(1e-9) <= 1.15))])
}

fn render_page(gpu: &mut Gpu) -> Img {
    let (cw, ch) = gpu.cell();
    let (w, h) = (COLS as u32 * cw, ROWS as u32 * ch);
    let tex = new_texture(&gpu.device, w, h, MTLPixelFormat::BGRA8Unorm, MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead, MTLStorageMode::Shared);
    let mut sc = page();
    gpu.render(&mut sc, Target::Texture(&tex), None, vec![], "fidelity");
    let mut px = vec![0u8; (w * h * 4) as usize];
    let region = MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: w as usize, height: h as usize, depth: 1 } };
    unsafe { tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(NonNull::new(px.as_mut_ptr() as *mut _).unwrap(), (w * 4) as usize, region, 0) };
    Img { w: w as usize, h: h as usize, px, cw: cw as usize, ch: ch as usize }
}

fn write_png(img: &Img, name: &str) -> PathBuf {
    let p = results_dir().join(name);
    let f = std::fs::File::create(&p).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), img.w as u32, img.h as u32);
    e.set_color(png::ColorType::Rgba);
    let mut w = e.write_header().unwrap();
    let rgba: Vec<u8> = img.px.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    w.write_image_data(&rgba).unwrap();
    p
}

fn cell_fit(img: &Img, row: usize, col: usize, span: usize) -> J {
    let (x0, y0) = (col * img.cw, row * img.ch);
    // search one cell either side for ink belonging to this glyph
    let b = img.bbox(x0.saturating_sub(img.cw), y0, x0 + (span + 1) * img.cw, y0 + img.ch);
    let Some((bx0, by0, bx1, by1)) = b else { return J::obj(vec![("ink", J::B(false))]) };
    let inside = bx0 + 1 >= x0 && bx1 <= x0 + span * img.cw + 1 && by0 >= y0 && by1 <= y0 + img.ch;
    J::obj(vec![("ink", J::B(true)), ("span_cells", J::N(span as f64)), ("ink_x", J::s(format!("{}..{}", bx0 as i64 - x0 as i64, bx1 as i64 - x0 as i64))),
        ("ink_w_cells", J::N((bx1 - bx0) as f64 / img.cw as f64)), ("fits", J::B(inside))])
}

pub fn advances(font: &mut FontSet) -> J {
    let adv_m = font.metrics.advance;
    let mut rows = vec![];
    for ch in ['M', '—', '…', '→', '←', '│', '─', '█', '⣿', '①', '中', '😀'] {
        let (face, g) = font.lookup(ch, 0);
        let a = advance_of(&font.faces[face].font, g);
        rows.push(J::obj(vec![("char", J::s(ch.to_string())), ("face", J::s(&font.faces[face].name)), ("advance_px", J::N(a)), ("cells", J::N(a / adv_m)), ("wcwidth", J::N(ch.width().unwrap_or(0) as f64))]));
    }
    // census: characters the primary font draws ~2 cells wide that Unicode calls single-width
    let f = &font.faces[0].font;
    let (mut n, mut ex) = (0, vec![]);
    for cp in 0x20u32..0x30000 {
        let Some(c) = char::from_u32(cp) else { continue };
        if c.width() != Some(1) { continue; }
        let g = glyph_of(f, c);
        if g == 0 { continue; }
        if advance_of(f, g) > adv_m * 1.5 { n += 1; if ex.len() < 12 { ex.push(J::s(format!("U+{cp:04X} {c}"))); } }
    }
    J::obj(vec![("face", J::s(&font.faces[0].name)), ("advance_px", J::N(adv_m)), ("cell", J::s(format!("{}x{}", font.metrics.cell_w, font.metrics.cell_h))),
        ("ascent", J::N(font.metrics.ascent)), ("descent", J::N(font.metrics.descent)), ("leading", J::N(font.metrics.leading)),
        ("single_width_chars_drawn_two_cells", J::N(n as f64)), ("examples", J::A(ex)), ("chars", J::A(rows))])
}

pub fn run(fonts: &[(String, [PathBuf; 4])]) -> J {
    let mut out = vec![];
    for (label, files) in fonts {
        for (mode, opts) in [("sprites", RenderOpts { sprites: true, braille_sprite: false }), ("font_only", RenderOpts { sprites: false, braille_sprite: false }), ("braille_sprite", RenderOpts { sprites: true, braille_sprite: true })] {
            let mut gpu = match Gpu::new(files, 16.0, 2.0, opts, Arc::new(|| {})) { Ok(g) => g, Err(e) => { out.push((format!("{label}_{mode}"), J::s(e))); continue; } };
            let img = render_page(&mut gpu);
            let png = write_png(&img, &format!("fidelity-{label}-{mode}.png"));
            let h = hline_check(&img, 0, 0, 12);
            let v = vline_check(&img, 0, 1, 1, 4);
            let gh: Vec<(f64, usize)> = (1..4).map(|r| hline_check(&img, r, 2, 6)).collect();
            let gv = vline_check(&img, 2, 6, 1, 4);
            let block_min = (img.ch..4 * img.ch).flat_map(|y| (8 * img.cw..12 * img.cw).map(move |x| (x, y))).map(|(x, y)| img.cov(x, y)).fold(1.0, f64::min);
            let box_ok = h.0 >= 0.7 && h.1 > 0 && v.0 >= 0.7 && v.1 > 0 && gh.iter().all(|g| g.0 >= 0.7 && g.1 > 0) && gv.0 >= 0.7;
            let j = J::obj(vec![("png", J::s(png.display().to_string())), ("cell", J::s(format!("{}x{}", img.cw, img.ch))),
                ("box_hline_min_cov", J::N(h.0)), ("box_vline_min_cov", J::N(v.0)), ("box_grid_h_min_cov", J::N(gh.iter().map(|g| g.0).fold(1.0, f64::min))), ("box_grid_v_min_cov", J::N(gv.0)),
                ("box_tiles", J::B(box_ok)), ("block_min_cov", J::N(block_min)), ("block_tiles", J::B(block_min >= 0.95)),
                ("braille", braille_check(&img, 14, 18, 1, 4)),
                ("cjk_zhong", cell_fit(&img, 5, 1, 2)), ("emoji", cell_fit(&img, 5, 5, 2)), ("hangul", cell_fit(&img, 5, 21, 2)),
                ("em_dash", cell_fit(&img, 5, 9, 1)), ("ellipsis", cell_fit(&img, 5, 12, 1)), ("arrow", cell_fit(&img, 5, 15, 1)), ("circled_1", cell_fit(&img, 5, 18, 1)),
                ("fallback_faces", J::A(gpu.font.faces.iter().skip(4).map(|f| J::s(&f.name)).collect()))]);
            eprintln!("[s1] fidelity {label}/{mode}: box_tiles={box_ok} block_min={block_min:.3} -> {}", png.display());
            out.push((format!("{label}_{mode}"), j));
            if mode == "sprites" { out.push((format!("{label}_advances"), advances(&mut gpu.font))); }
        }
    }
    J::O(out)
}
