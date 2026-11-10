//! Every tiny-skia call, behind one module, so a later plots extension can lift it into a shared
//! crate (scope §9, the rule of two). The PNG exporter rasterizes the vector model here
//! ([`rasterize`]), and the pixel renderer draws with the same calls at the view's scale into a
//! [`Canvas`] it keeps and into the pixel buffer it shows ([`Target`]).

use crate::dial::Dial;
use crate::fonts::Fonts;
use crate::model::{
    Board, Element, HIGHLIGHTER_ALPHA, Point, Rect, Shape, head_length, label_lines,
};
use crate::svg::{Look, picture};
use die_core::{PixelRect, Rgb};
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PixmapMut, PremultipliedColorU8,
    Stroke, StrokeDash, Transform,
};

/// The longest side a raster may have: Metal's texture cap, and sanity for an export.
pub const MAX_SIDE: u32 = 16_384;

/// The tool dial's disc's opacity.
const DIAL_ALPHA: f32 = 0.12;
/// The hovered sector's.
const SECTOR_ALPHA: f32 = 0.35;
/// The spokes' and the dead zone's circle's.
const SPOKE_ALPHA: f32 = 0.5;
/// The straight steps a sector's arc is drawn in.
const ARC_STEPS: usize = 6;

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
        draw_element(&mut pm.as_mut(), el, look, t, fonts);
    }
    Ok(Raster {
        width: w,
        height: h,
        rgba: pm.take_demultiplied(),
    })
}

/// Draw one element into `pm` through `t`, a scale and a translation.
fn draw_element(
    pm: &mut PixmapMut<'_>,
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
            let (mut end, head) = (*b, head_length(width));
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
fn blit(pm: &mut PixmapMut<'_>, x0: i32, y0: i32, w: usize, h: usize, cov: &[u8], ink: Rgb) {
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

// ------------------------------------------------------------------ the pixel renderer's canvas

/// Board units to a picture's pixels: `px = p × scale + (tx, ty)`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Map {
    /// Pixels per board unit.
    pub scale: f32,
    /// Where the board's origin is, in pixels.
    pub tx: f32,
    /// See `tx`.
    pub ty: f32,
}

impl Map {
    /// The same picture drawn into a canvas whose top-left pixel is `r`'s.
    pub fn into_rect(self, r: PixelRect) -> Map {
        Map {
            scale: self.scale,
            tx: self.tx - r.x as f32,
            ty: self.ty - r.y as f32,
        }
    }
    fn transform(self) -> Transform {
        Transform::from_row(self.scale, 0.0, 0.0, self.scale, self.tx, self.ty)
    }
    /// The pixels a board rectangle covers, `pad` pixels around, inside a `w` × `h` picture.
    pub fn pixels(self, r: &Rect, pad: f32, w: u32, h: u32) -> PixelRect {
        let (x0, y0) = (
            r.min.x * self.scale + self.tx,
            r.min.y * self.scale + self.ty,
        );
        let (x1, y1) = (
            r.max.x * self.scale + self.tx,
            r.max.y * self.scale + self.ty,
        );
        let clamp = |v: f32, max: u32| v.max(0.0).min(max as f32) as u32;
        let (x0, y0) = (clamp((x0 - pad).floor(), w), clamp((y0 - pad).floor(), h));
        let (x1, y1) = (clamp((x1 + pad).ceil(), w), clamp((y1 + pad).ceil(), h));
        PixelRect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }
}

/// The `width × 4` bytes of row `y` from `x` for `w` pixels.
fn span(width: u32, x: u32, y: u32, w: u32) -> std::ops::Range<usize> {
    let start = (y as usize * width as usize + x as usize) * 4;
    start..start + w as usize * 4
}

/// Where the renderer draws: its own canvas, or the pixel buffer's bytes.
pub enum Target<'a> {
    /// A canvas the renderer keeps.
    Canvas(&'a mut Canvas),
    /// Premultiplied RGBA8, `width × 4` bytes a row, as a pixel buffer holds it.
    Bytes {
        /// The bytes.
        bytes: &'a mut [u8],
        /// Their width.
        width: u32,
        /// Their height.
        height: u32,
    },
}

impl Target<'_> {
    fn with(&mut self, f: impl FnOnce(&mut PixmapMut<'_>)) {
        match self {
            Target::Canvas(c) => f(&mut c.pm.as_mut()),
            Target::Bytes {
                bytes,
                width,
                height,
            } => {
                if let Some(mut pm) = PixmapMut::from_bytes(bytes, *width, *height) {
                    f(&mut pm);
                }
            }
        }
    }
    /// Draw one element, without its label if it's one (labels are the frame's cells).
    pub fn draw(&mut self, el: &Element, look: &Look, map: Map) {
        self.with(|pm| draw_element(pm, el, look, map.transform(), None));
    }
    /// Outline a board rectangle one pixel wide in `color` at `alpha`, dashed if asked: the
    /// selection's and the marquee's.
    pub fn outline(&mut self, r: &Rect, map: Map, color: Rgb, alpha: f32, dashed: bool) {
        let Some(sk) = tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y) else {
            return;
        };
        let path = PathBuilder::from_rect(sk);
        let mut st = stroke(1.0 / map.scale.max(0.01));
        if dashed {
            let d = 4.0 / map.scale.max(0.01);
            st.dash = StrokeDash::new(vec![d, d], 0.0);
        }
        self.with(|pm| pm.stroke_path(&path, &paint(color, alpha), &st, map.transform(), None));
    }
    /// The tool dial in `color`: a translucent disc, the hovered sector filled, eight spokes
    /// from the dead zone to the rim, and the dead zone's circle. Its names are the frame's cells.
    pub fn dial(&mut self, d: &Dial, map: Map, color: Rgb) {
        let t = map.transform();
        let (c, r) = (d.centre, d.radius);
        let hair = stroke(1.0 / map.scale.max(0.01));
        let mut spokes = PathBuilder::new();
        for k in 0..8 {
            let dir = Dial::direction(k as f32 - 0.5);
            spokes.move_to(c.x + dir.x * d.dead, c.y + dir.y * d.dead);
            spokes.line_to(c.x + dir.x * r, c.y + dir.y * r);
        }
        spokes.push_circle(c.x, c.y, d.dead);
        let spokes = spokes.finish();
        let wedge = d.hovered.and_then(|i| sector(c, d.dead, r, i));
        self.with(|pm| {
            if let Some(disc) = PathBuilder::from_circle(c.x, c.y, r) {
                pm.fill_path(&disc, &paint(color, DIAL_ALPHA), FillRule::Winding, t, None);
            }
            if let Some(wedge) = wedge {
                pm.fill_path(
                    &wedge,
                    &paint(color, SECTOR_ALPHA),
                    FillRule::Winding,
                    t,
                    None,
                );
            }
            if let Some(spokes) = spokes {
                pm.stroke_path(&spokes, &paint(color, SPOKE_ALPHA), &hair, t, None);
            }
        });
    }
}

/// Sector `i` of a dial about `c`, from `inner` to `outer`, its arcs in straight steps.
fn sector(c: Point, inner: f32, outer: f32, i: usize) -> Option<Path> {
    let at = |k: usize, r: f32| {
        let dir = Dial::direction(i as f32 - 0.5 + k as f32 / ARC_STEPS as f32);
        (c.x + dir.x * r, c.y + dir.y * r)
    };
    let mut pb = PathBuilder::new();
    let (x, y) = at(0, inner);
    pb.move_to(x, y);
    for k in 0..=ARC_STEPS {
        let (x, y) = at(k, outer);
        pb.line_to(x, y);
    }
    for k in (0..=ARC_STEPS).rev() {
        let (x, y) = at(k, inner);
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

/// A picture the renderer keeps: premultiplied RGBA8, `width × 4` bytes a row, transparent to
/// begin with.
pub struct Canvas {
    pm: Pixmap,
}

impl Canvas {
    /// A transparent canvas this size, or None for a side of 0 or past [`MAX_SIDE`].
    pub fn new(width: u32, height: u32) -> Option<Canvas> {
        if width > MAX_SIDE || height > MAX_SIDE {
            return None;
        }
        Pixmap::new(width, height).map(|pm| Canvas { pm })
    }
    /// Its width.
    pub fn width(&self) -> u32 {
        self.pm.width()
    }
    /// Its height.
    pub fn height(&self) -> u32 {
        self.pm.height()
    }
    /// The bytes.
    pub fn data(&self) -> &[u8] {
        self.pm.data()
    }
    /// A target on it.
    pub fn target(&mut self) -> Target<'_> {
        Target::Canvas(self)
    }
    /// Make it transparent.
    pub fn clear(&mut self) {
        self.pm.fill(tiny_skia::Color::TRANSPARENT);
    }
    /// Make `r` transparent.
    pub fn clear_rect(&mut self, r: PixelRect) {
        let r = r.clipped_to(self.width(), self.height());
        let width = self.width();
        for y in r.y..r.y + r.height {
            self.pm.data_mut()[span(width, r.x, y, r.width)].fill(0);
        }
    }
    /// Put a smaller canvas's pixels at (x, y), over whatever was there.
    pub fn paste(&mut self, other: &Canvas, x: u32, y: u32) {
        let (w, h) = (self.width(), self.height());
        let r = PixelRect::new(x, y, other.width(), other.height()).clipped_to(w, h);
        for row in 0..r.height {
            let from = span(other.width(), 0, row, r.width);
            let to = span(w, r.x, r.y + row, r.width);
            self.pm.data_mut()[to].copy_from_slice(&other.pm.data()[from]);
        }
    }
    /// Copy `r` of it into `bytes`, a picture of the same size.
    pub fn copy_to(&self, bytes: &mut [u8], r: PixelRect) {
        let r = r.clipped_to(self.width(), self.height());
        for y in r.y..r.y + r.height {
            let at = span(self.width(), r.x, y, r.width);
            bytes[at.clone()].copy_from_slice(&self.pm.data()[at]);
        }
    }
    /// Move the picture by `dx`, `dy` pixels, leaving what it uncovers transparent: the strips
    /// that need drawing, which `shift_exposes` lists.
    pub fn shift(&mut self, dx: i32, dy: i32) {
        let (w, h) = (self.width() as i64, self.height() as i64);
        let (dx, dy) = (i64::from(dx), i64::from(dy));
        if dx.abs() >= w || dy.abs() >= h {
            self.clear();
            return;
        }
        let width = self.width();
        let rows: Vec<u32> = if dy >= 0 {
            (0..h).rev().map(|y| y as u32).collect()
        } else {
            (0..h).map(|y| y as u32).collect()
        };
        for y in rows {
            let src = i64::from(y) - dy;
            if !(0..h).contains(&src) {
                self.pm.data_mut()[span(width, 0, y, width)].fill(0);
                continue;
            }
            // The row as a whole first, then the columns within it.
            if src != i64::from(y) {
                let from = span(width, 0, src as u32, width);
                let to = span(width, 0, y, width).start;
                self.pm.data_mut().copy_within(from, to);
            }
            let row = &mut self.pm.data_mut()[span(width, 0, y, width)];
            if dx > 0 {
                let d = dx as usize * 4;
                row.copy_within(0..row.len() - d, d);
                row[..d].fill(0);
            } else if dx < 0 {
                let d = (-dx) as usize * 4;
                row.copy_within(d.., 0);
                let n = row.len();
                row[n - d..].fill(0);
            }
        }
    }
    /// The strips a [`Canvas::shift`] by `dx`, `dy` uncovers, to draw again.
    pub fn shift_exposes(&self, dx: i32, dy: i32) -> Vec<PixelRect> {
        let (w, h) = (self.width(), self.height());
        if dx.unsigned_abs() >= w || dy.unsigned_abs() >= h {
            return vec![PixelRect::whole(w, h)];
        }
        let mut out = vec![];
        let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
        if ay > 0 {
            let y = if dy > 0 { 0 } else { h - ay };
            out.push(PixelRect::new(0, y, w, ay));
        }
        if ax > 0 {
            let x = if dx > 0 { 0 } else { w - ax };
            let (y, rows) = if dy > 0 { (ay, h - ay) } else { (0, h - ay) };
            out.push(PixelRect::new(x, y, ax, rows));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CellSize;

    fn px(c: &Canvas, x: u32, y: u32) -> [u8; 4] {
        let i = (y * c.width() + x) as usize * 4;
        [
            c.data()[i],
            c.data()[i + 1],
            c.data()[i + 2],
            c.data()[i + 3],
        ]
    }

    #[test]
    fn a_shift_moves_the_picture_and_says_what_it_uncovered() {
        let mut c = Canvas::new(6, 4).unwrap();
        let mut dot = Canvas::new(1, 1).unwrap();
        dot.target().draw(
            &Element {
                id: 1,
                ink: 1,
                width: 2,
                shape: Shape::Stroke {
                    points: vec![Point::new(0.0, 0.0)],
                    highlighter: false,
                },
            },
            &Look::default(),
            Map {
                scale: 1.0,
                tx: 0.5,
                ty: 0.5,
            },
        );
        assert_eq!(px(&dot, 0, 0)[3], 255, "a solid pixel to move about");
        c.paste(&dot, 2, 1);
        c.shift(1, 2);
        assert_eq!(px(&c, 3, 3)[3], 255, "moved right and down");
        assert_eq!(px(&c, 2, 1)[3], 0, "and gone from where it was");
        assert_eq!(
            c.shift_exposes(1, 2),
            [PixelRect::new(0, 0, 6, 2), PixelRect::new(0, 2, 1, 2)]
        );
        c.shift(-3, -3);
        assert_eq!(px(&c, 0, 0)[3], 255);
        assert_eq!(
            c.shift_exposes(-3, -3),
            [PixelRect::new(0, 1, 6, 3), PixelRect::new(3, 0, 3, 1)]
        );
        assert_eq!(c.shift_exposes(6, 0), [PixelRect::whole(6, 4)], "all of it");
        c.clear_rect(PixelRect::new(0, 0, 1, 1));
        assert_eq!(px(&c, 0, 0)[3], 0);
        let mut bytes = vec![1u8; 6 * 4 * 4];
        c.copy_to(&mut bytes, PixelRect::new(0, 0, 6, 1));
        assert_eq!(&bytes[..4], &[0, 0, 0, 0], "the first row copied");
        assert_eq!(bytes[24], 1, "the second left alone");
        assert_eq!(
            Map {
                scale: 2.0,
                tx: 10.0,
                ty: 10.0
            }
            .pixels(
                &Rect::from_points(Point::new(0.0, 0.0), Point::new(5.0, 5.0)),
                1.0,
                100,
                100
            ),
            PixelRect::new(9, 9, 12, 12)
        );
    }

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
