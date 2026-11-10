//! Every tiny-skia call, behind one module, so a later plots extension can lift it into a shared
//! crate (scope §9, the rule of two). Today the PNG exporter rasterizes the vector model here;
//! the pixel renderer will draw with the same calls at the view's scale.

use crate::fonts::Fonts;
use crate::model::{Board, Element, HIGHLIGHTER_ALPHA, Point, Rect, Shape, label_lines};
use crate::svg::{Look, picture};
use die_core::Rgb;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Stroke,
    Transform,
};

/// The longest side a raster may have: Metal's texture cap, and sanity for an export.
pub const MAX_SIDE: u32 = 16_384;

/// Pixels, straight (not premultiplied) RGBA, rows top first.
#[derive(Clone, PartialEq, Debug)]
pub struct Raster {
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// `width × height × 4` bytes.
    pub rgba: Vec<u8>,
}

impl Raster {
    /// The straight RGBA of the pixel at (x, y), or None outside.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.width + x) as usize * 4;
        self.rgba.get(i..i + 4).map(|p| [p[0], p[1], p[2], p[3]])
    }
}

fn paint(c: Rgb, alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0, c.1, c.2, (alpha * 255.0).round() as u8);
    p.anti_alias = true;
    p
}

fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

/// The board rasterized at `scale` pixels per board unit over its picture box, on `look.bg` or
/// transparent, labels through `fonts` if there are any.
pub fn rasterize(
    board: &Board,
    look: &Look,
    scale: f32,
    fonts: Option<&Fonts>,
) -> Result<Raster, String> {
    let pic = picture(board, look.cell);
    let side = |n: f32| (n * scale).ceil().max(1.0) as u32;
    let (w, h) = (side(pic.width()), side(pic.height()));
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}×{h} is more than {MAX_SIDE} pixels a side"));
    }
    let mut pm = Pixmap::new(w, h).ok_or_else(|| format!("no pixmap of {w}×{h}"))?;
    if let Some(bg) = look.bg {
        pm.fill(tiny_skia::Color::from_rgba8(bg.0, bg.1, bg.2, 255));
    }
    let t = Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        -pic.min.x * scale,
        -pic.min.y * scale,
    );
    for el in &board.elements {
        draw_element(&mut pm, el, look, t, fonts);
    }
    Ok(Raster {
        width: w,
        height: h,
        rgba: pm.take_demultiplied(),
    })
}

/// Draw one element into `pm` through `t`, a scale and a translation.
pub fn draw_element(
    pm: &mut Pixmap,
    el: &Element,
    look: &Look,
    t: Transform,
    fonts: Option<&Fonts>,
) {
    let ink = look.inks[usize::from(el.ink.clamp(1, 6)) - 1];
    let width = el.stroke_width();
    match &el.shape {
        Shape::Stroke {
            points,
            highlighter,
        } => {
            let alpha = if *highlighter { HIGHLIGHTER_ALPHA } else { 1.0 };
            let p = paint(ink, alpha);
            match points.as_slice() {
                [] => {}
                [only] => {
                    if let Some(dot) = PathBuilder::from_circle(only.x, only.y, width / 2.0) {
                        pm.fill_path(&dot, &p, FillRule::Winding, t, None);
                    }
                }
                _ => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(points[0].x, points[0].y);
                    for q in &points[1..] {
                        pb.line_to(q.x, q.y);
                    }
                    if let Some(path) = pb.finish() {
                        pm.stroke_path(&path, &p, &stroke(width), t, None);
                    }
                }
            }
        }
        Shape::Rect { a, b } => {
            let r = Rect::from_points(*a, *b);
            let sk = tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y);
            if let Some(sk) = sk {
                let path = PathBuilder::from_rect(sk);
                pm.stroke_path(&path, &paint(ink, 1.0), &stroke(width), t, None);
            }
        }
        Shape::Ellipse { a, b } => {
            let r = Rect::from_points(*a, *b);
            let sk = tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y);
            if let Some(path) = sk.and_then(PathBuilder::from_oval) {
                pm.stroke_path(&path, &paint(ink, 1.0), &stroke(width), t, None);
            }
        }
        Shape::Line { a, b, arrow, .. } => {
            let p = paint(ink, 1.0);
            let len = a.dist(*b);
            let (mut end, head) = (*b, 4.0 * width + 6.0);
            if *arrow && len > 0.0 {
                let u = (*b - *a) * (1.0 / len);
                let perp = Point::new(-u.y, u.x);
                let base = *b - u * head;
                let (l, r) = (base + perp * (head * 0.4), base - perp * (head * 0.4));
                let mut pb = PathBuilder::new();
                pb.move_to(b.x, b.y);
                pb.line_to(l.x, l.y);
                pb.line_to(r.x, r.y);
                pb.close();
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &p, FillRule::Winding, t, None);
                }
                end = *b - u * (head * 0.6);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(end.x, end.y);
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &p, &stroke(width), t, None);
            }
        }
        Shape::Label { at, text } => {
            let Some(fonts) = fonts else { return };
            let px = look.cell.h * 0.8 * t.sy;
            let ascent = fonts.ascent(px);
            for (i, line) in label_lines(text).iter().enumerate() {
                let top = (at.y + i as f32 * look.cell.h) * t.sy + t.ty;
                let baseline = top + ascent;
                for (j, ch) in line.chars().enumerate() {
                    let left = (at.x + j as f32 * look.cell.w) * t.sx + t.tx;
                    let (m, cov) = fonts.glyph(ch, px);
                    let x0 = (left + m.xmin as f32).round() as i32;
                    let y0 = (baseline - (m.ymin + m.height as i32) as f32).round() as i32;
                    blit(pm, x0, y0, m.width, m.height, &cov, ink);
                }
            }
        }
    }
}

/// Blend a coverage bitmap of `ink` over the pixmap at (x0, y0).
fn blit(pm: &mut Pixmap, x0: i32, y0: i32, w: usize, h: usize, cov: &[u8], ink: Rgb) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let px = pm.pixels_mut();
    for j in 0..h {
        let y = y0 + j as i32;
        if y < 0 || y >= ph {
            continue;
        }
        for i in 0..w {
            let x = x0 + i as i32;
            let a = u32::from(cov[j * w + i]);
            if x < 0 || x >= pw || a == 0 {
                continue;
            }
            let at = (y * pw + x) as usize;
            let d = px[at];
            let inv = 255 - a;
            let mix = |s: u8, d: u8| ((u32::from(s) * a + u32::from(d) * inv + 127) / 255) as u8;
            let alpha = mix(255, d.alpha());
            let (r, g, b) = (
                mix(ink.0, d.red()).min(alpha),
                mix(ink.1, d.green()).min(alpha),
                mix(ink.2, d.blue()).min(alpha),
            );
            if let Some(c) = PremultipliedColorU8::from_rgba(r, g, b, alpha) {
                px[at] = c;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CellSize;

    #[test]
    fn a_stroke_puts_ink_where_it_goes_and_nowhere_else() {
        let mut board = Board::default();
        board.add(
            Shape::Stroke {
                points: vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
                highlighter: false,
            },
            2,
            1,
        );
        let look = Look {
            bg: Some(Rgb(0, 0, 0)),
            cell: CellSize::default(),
            ..Look::default()
        };
        let r = rasterize(&board, &look, 1.0, None).unwrap();
        // 100 wide plus half the width (2) and 16 of pad either side.
        assert_eq!((r.width, r.height), (136, 36));
        let on = r.pixel(66, 18).unwrap();
        assert!(on[0] > 150 && on[1] < 60, "red ink on the line: {on:?}");
        assert_eq!(
            r.pixel(66, 2),
            Some([0, 0, 0, 255]),
            "the background off it"
        );
        let clear = Look { bg: None, ..look };
        let r = rasterize(&board, &clear, 2.0, None).unwrap();
        assert_eq!((r.width, r.height), (272, 72));
        assert_eq!(r.pixel(10, 4).unwrap()[3], 0, "transparent off the ink");
    }
}
