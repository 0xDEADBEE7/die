//! What die-render's neutral tests share: scenes, a stand-in for the faces, and a
//! stand-in for the GPU that draws a frame's data into an `Image` by the same rules
//! the shader follows.
#![allow(dead_code)]

use die_core::{Cell, Ctx, Cursor, CursorShape, Glyph, Rgb, Scene};
use die_render::atlas::GlyphKey;
use die_render::font::{CellMetrics, FaceMetrics};
use die_render::frame::{GlyphBitmap, Image, Plane, Quad, Upload, kind};
use die_render::rows::Frames;
use die_render::sprite::draw;
use unicode_width::UnicodeWidthStr;

pub const FG: Rgb = Rgb(200, 210, 220);
pub const BG: Rgb = Rgb(16, 16, 32);
pub const RED: Rgb = Rgb(220, 40, 40);
pub const GREEN: Rgb = Rgb(40, 200, 80);
pub const BLUE: Rgb = Rgb(50, 80, 230);

/// Ioskeley Mono Term's metrics at 16 pt on a 2x display, as S1 measured them.
pub fn term_metrics() -> FaceMetrics {
    FaceMetrics {
        ascent: 30.8799,
        descent: 7.04,
        leading: 2.0801,
        underline_position: -3.2,
        underline_thickness: 1.6,
        x_height: 17.0,
    }
}

/// The 19×40 cell at 2x.
pub fn cell2x() -> CellMetrics {
    CellMetrics::new(term_metrics(), 19.2, 2.0)
}

/// A blank grid in `FG` on `BG`, through the one call that builds a `Scene` without
/// naming its fields, so a field going away (K9) doesn't reach these tests.
pub fn blank(cols: usize, rows: usize) -> Scene {
    let mut s = Ctx::new(vec![]).unwrap().compose(cols, rows, &[]);
    s.background = BG;
    s.cursors.clear();
    for c in &mut s.cells {
        *c = Cell::new(Glyph::Char(' '), FG, BG);
    }
    s
}

/// Write `text` on a row from a column, one cell per grapheme by its width, with
/// wide characters' right halves as continuation cells. `f` sets each cell's look.
pub fn put_with(s: &mut Scene, row: usize, col: usize, text: &[&str], f: impl Fn(&mut Cell)) {
    let cols = s.cols;
    let mut at = col;
    for g in text {
        let w = g.width().max(1);
        let mut chars = g.chars();
        let glyph = match (chars.next(), chars.next()) {
            (Some(c), None) => Glyph::Char(c),
            _ => Glyph::Cluster((*g).into()),
        };
        let mut lead = Cell::new(glyph, FG, BG);
        f(&mut lead);
        s.cells[row * cols + at] = lead;
        for i in 1..w {
            let mut cont = Cell::new(Glyph::Continuation, FG, BG);
            f(&mut cont);
            s.cells[row * cols + at + i] = cont;
        }
        at += w;
    }
}

/// Write `text` character by character.
pub fn put(s: &mut Scene, row: usize, col: usize, text: &str) {
    let chars: Vec<String> = text.chars().map(String::from).collect();
    let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
    put_with(s, row, col, &refs, |_| {});
}

pub fn cursor(row: usize, col: usize, shape: CursorShape) -> Cursor {
    Cursor {
        row,
        col,
        shape,
        color: RED,
        past_end: false,
    }
}

/// What the stand-in face gives for a key: a pattern that depends on the glyph, its
/// style and its span, so two different keys paint different pixels. A line comes
/// from the real sprite code, and a character the renderer draws as well.
pub fn fake_bitmap(key: &GlyphKey, cell: &CellMetrics) -> Option<GlyphBitmap> {
    match key {
        GlyphKey::Line(l) => Some(cell.line_bitmap(*l)),
        GlyphKey::Text {
            glyph,
            bold,
            italic,
            span,
        } => {
            let text = glyph.to_string();
            let first = text.chars().next()?;
            if let Some(d) = draw(first, cell.w, cell.h, cell.thin) {
                return Some(cell.cell_bitmap(d));
            }
            let seed = text
                .chars()
                .fold(u32::from(*bold) + 2 * u32::from(*italic), |a, c| {
                    a.wrapping_mul(31).wrapping_add(c as u32)
                });
            let (w, h) = (u32::from(*span) * cell.w - 2, cell.h - 4);
            let data = (0..w * h)
                .map(|i| ((i.wrapping_mul(7).wrapping_add(seed.wrapping_mul(13))) % 200 + 55) as u8)
                .collect();
            Some(GlyphBitmap {
                w,
                h,
                left: 1,
                top: 2,
                plane: Plane::Gray,
                data,
            })
        }
    }
}

/// The driver's loop, with the stand-in face. Returns the frame's data.
pub fn frame(frames: &mut Frames, scene: &Scene) -> (Vec<Quad>, Rgb, Vec<Upload>) {
    let cell = *frames.cell();
    let mut wanted = frames.diff(scene);
    while !wanted.is_empty() {
        for key in &wanted {
            frames.supply(key, fake_bitmap(key, &cell));
        }
        wanted = frames.wanted();
    }
    let f = frames.build(scene);
    (f.quads.to_vec(), f.clear, f.uploads)
}

/// The GPU's job in plain Rust, by the shader's rules: nearest sampling, and blending
/// with premultiplied alpha. Comparing with the real one holds the shader to them.
pub struct SoftGpu {
    gray: Vec<u8>,
    color: Vec<u8>,
    size: u32,
}

impl SoftGpu {
    pub fn new(size: u32) -> SoftGpu {
        SoftGpu {
            gray: vec![0; (size * size) as usize],
            color: vec![0; (size * size * 4) as usize],
            size,
        }
    }

    pub fn upload(&mut self, u: &Upload) {
        let bytes = match u.plane {
            Plane::Gray => 1,
            Plane::Color => 4,
        };
        let plane = match u.plane {
            Plane::Gray => &mut self.gray,
            Plane::Color => &mut self.color,
        };
        for y in 0..u.h as usize {
            let at = ((u.y as usize + y) * self.size as usize + u.x as usize) * bytes;
            let row = u.w as usize * bytes;
            plane[at..at + row].copy_from_slice(&u.data[y * row..(y + 1) * row]);
        }
    }

    pub fn draw(&self, quads: &[Quad], clear: Rgb, w: u32, h: u32) -> Image {
        let mut px: Vec<[f32; 3]> = vec![
            [
                f32::from(clear.0) / 255.0,
                f32::from(clear.1) / 255.0,
                f32::from(clear.2) / 255.0
            ];
            (w * h) as usize
        ];
        for q in quads {
            let [x, y, qw, qh] = q.rect;
            let [u, v, _, _] = q.uv;
            let c = q.color.to_be_bytes().map(|b| f32::from(b) / 255.0);
            for py in (y as i64).max(0)..((y + qh) as i64).min(i64::from(h)) {
                for px_x in (x as i64).max(0)..((x + qw) as i64).min(i64::from(w)) {
                    let (sx, sy) = (
                        (u + (px_x as f32 - x)) as usize,
                        (v + (py as f32 - y)) as usize,
                    );
                    let at = sy * self.size as usize + sx;
                    let (src, a): ([f32; 3], f32) = match q.kind {
                        kind::SOLID => ([c[0] * c[3], c[1] * c[3], c[2] * c[3]], c[3]),
                        kind::GRAY => {
                            let a = f32::from(self.gray[at]) / 255.0 * c[3];
                            ([c[0] * a, c[1] * a, c[2] * a], a)
                        }
                        _ => {
                            let p = &self.color[at * 4..at * 4 + 4];
                            let f = |i: usize| f32::from(p[i]) / 255.0;
                            ([f(2), f(1), f(0)], f(3))
                        }
                    };
                    let d = &mut px[py as usize * w as usize + px_x as usize];
                    for i in 0..3 {
                        d[i] = src[i] + d[i] * (1.0 - a);
                    }
                }
            }
        }
        let mut bgra = Vec::with_capacity(px.len() * 4);
        for p in &px {
            let b = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            bgra.extend_from_slice(&[b(p[2]), b(p[1]), b(p[0]), 255]);
        }
        Image { w, h, bgra }
    }
}

/// A frame drawn by a renderer's data: diff, supply, build, upload, draw.
pub fn paint(frames: &mut Frames, gpu: &mut SoftGpu, scene: &Scene) -> Image {
    let (quads, clear, uploads) = frame(frames, scene);
    for u in &uploads {
        gpu.upload(u);
    }
    let (w, h) = frames.cell().grid(scene.cols, scene.rows);
    gpu.draw(&quads, clear, w, h)
}

/// A small generator with no dependency: the same numbers every run.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
