//! The canvas every panel draws on: tiny-skia for shapes and die-draw's [`Book`] for text, in
//! logical px at the display's scale; and the theme, die-fig's dark look (its `theme.rs`
//! defaults, which DIE's mono-dark gives).
//!
//! Hairlines (grid, axes) are filled as whole physical pixels, one a logical px at 1×, two at
//! 2×, so they are never blurred. Lines are cut to a rectangle and broken where a point isn't
//! finite, as a missing value maps.

use die_draw::{Book, Pixels, Rgb, TextLook, Weight};
use tiny_skia as sk;

/// A rectangle in logical px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Smaller by `l`, `t`, `r` and `b` at its sides, never below nothing.
    pub fn inset(&self, l: f32, t: f32, r: f32, b: f32) -> Rect {
        let (w, h) = ((self.w - l - r).max(0.0), (self.h - t - b).max(0.0));
        Rect::new(self.x + l, self.y + t, w, h)
    }

    /// The least rectangle holding both.
    pub fn union(&self, o: Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }
}

/// Every colour a figure is drawn in.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The ground; text; titles and values; ticks and secondary text.
    pub surface: Rgb,
    pub text: Rgb,
    pub strong: Rgb,
    pub muted: Rgb,
    /// Gridlines and panel frames; the zero line; the card's ground; its border.
    pub grid: Rgb,
    pub baseline: Rgb,
    pub card: Rgb,
    pub ring: Rgb,
    /// The neutral `dim` names.
    pub dim: Rgb,
    /// The eight series slots: blue, orange, aqua, yellow, magenta, green, violet, red.
    pub series: Vec<Rgb>,
    /// Good, warning, serious and critical.
    pub status: Vec<Rgb>,
}

/// The slots' names, in order.
const SLOTS: &str = "blue orange aqua yellow magenta green violet red";

impl Default for Theme {
    /// die-fig's defaults on a dark surface.
    fn default() -> Theme {
        let list = |s: &str| s.split(' ').map(hex).collect();
        Theme {
            surface: hex("1a1a1a"),
            text: hex("cccccc"),
            strong: hex("eeeeee"),
            muted: hex("888888"),
            grid: hex("2a2a2a"),
            baseline: hex("3a3a3a"),
            card: hex("252525"),
            ring: hex("3a3a3a"),
            dim: hex("5a5a5a"),
            series: list("3987e5 d95926 199e70 c98500 d55181 0ca30c 9085e9 e66767"),
            status: list("0ca30c fab219 ec835a d03b3b"),
        }
    }
}

impl Theme {
    /// Series slot `i`, round again past the eighth.
    pub fn slot(&self, i: usize) -> Rgb {
        self.series[i % self.series.len()]
    }

    /// A spec's colour: a slot's name, a status's, `dim`, `bright`, `accent` or `#rrggbb`.
    pub fn named(&self, name: &str) -> Option<Rgb> {
        let n = name.trim().to_ascii_lowercase();
        if let Some(h) = n.strip_prefix('#') {
            return (h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex(h));
        }
        if let Some(i) = SLOTS.split(' ').position(|s| s == n) {
            return Some(self.series[i]);
        }
        let statuses = "good warning serious critical".split(' ');
        if let Some(i) = statuses.into_iter().position(|s| s == n) {
            return Some(self.status[i]);
        }
        match n.as_str() {
            "dim" => Some(self.dim),
            "bright" => Some(self.strong),
            "accent" => Some(hex("cc4444")),
            _ => None,
        }
    }
}

/// `rrggbb` as a colour; what isn't hex is black.
pub fn hex(s: &str) -> Rgb {
    let v = u32::from_str_radix(s, 16).unwrap_or(0);
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// `t` of the way from `a` to `b`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// How a text is set: its size in logical px, its weight and its colour. Digits are tabular.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub size: f32,
    pub weight: Weight,
    pub color: Rgb,
}

impl Ink {
    pub fn new(size: f32, color: Rgb) -> Ink {
        let weight = Weight::Regular;
        Ink {
            size,
            weight,
            color,
        }
    }

    /// The same, semibold.
    pub fn bold(self) -> Ink {
        Ink {
            weight: Weight::Semibold,
            ..self
        }
    }

    fn look(self) -> TextLook {
        TextLook::new(self.weight, self.size).tabular()
    }
}

/// Which of a text's points `x` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Middle,
    End,
}

/// How a line is drawn: its width in logical px, its colour, whether it's dashed.
#[derive(Clone, Copy, Debug)]
pub struct Pen {
    pub width: f32,
    pub color: Rgb,
    pub dash: bool,
}

impl Pen {
    pub fn new(width: f32, color: Rgb) -> Pen {
        let dash = false;
        Pen { width, color, dash }
    }
}

/// A picture being drawn: its pixels (premultiplied RGBA8), its scale, and the fonts.
pub struct Canvas<'a> {
    pub pixmap: sk::Pixmap,
    pub scale: f32,
    book: &'a mut Book,
}

impl<'a> Canvas<'a> {
    /// `w × h` logical px at `scale`, all `ground`. None when that is no pixels.
    pub fn new(book: &'a mut Book, (w, h): (f32, f32), scale: f32, ground: Rgb) -> Option<Self> {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let (pw, ph) = ((w * scale).round(), (h * scale).round());
        let mut pixmap = sk::Pixmap::new(pw.max(0.0) as u32, ph.max(0.0) as u32)?;
        pixmap.fill(sk::Color::from_rgba8(ground.0, ground.1, ground.2, 255));
        Some(Canvas {
            pixmap,
            scale,
            book,
        })
    }

    /// A canvas over a picture already drawn at `scale`.
    pub fn over(book: &'a mut Book, pixmap: sk::Pixmap, scale: f32) -> Self {
        Canvas {
            pixmap,
            scale,
            book,
        }
    }

    /// Its size in logical px.
    pub fn size(&self) -> (f32, f32) {
        let (w, h) = (self.pixmap.width() as f32, self.pixmap.height() as f32);
        (w / self.scale, h / self.scale)
    }

    fn paint(color: Rgb, alpha: f32) -> sk::Paint<'static> {
        let mut p = sk::Paint::default();
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        p.set_color_rgba8(color.0, color.1, color.2, a);
        p
    }

    fn fill_path(&mut self, path: Option<sk::Path>, color: Rgb, alpha: f32) {
        let at = sk::Transform::from_scale(self.scale, self.scale);
        if let Some(p) = path {
            let paint = Canvas::paint(color, alpha);
            self.pixmap
                .fill_path(&p, &paint, sk::FillRule::Winding, at, None);
        }
    }

    fn stroke(&mut self, path: Option<sk::Path>, pen: Pen) {
        let at = sk::Transform::from_scale(self.scale, self.scale);
        let mut s = sk::Stroke {
            width: pen.width,
            line_cap: sk::LineCap::Round,
            line_join: sk::LineJoin::Round,
            ..sk::Stroke::default()
        };
        if pen.dash {
            s.dash = sk::StrokeDash::new(vec![4.0, 4.0], 0.0);
            s.line_cap = sk::LineCap::Butt;
        }
        if let Some(p) = path {
            let paint = Canvas::paint(pen.color, 1.0);
            self.pixmap.stroke_path(&p, &paint, &s, at, None);
        }
    }

    /// A rectangle filled at `alpha`, its corners rounded by `r`.
    pub fn fill(&mut self, b: Rect, r: f32, color: Rgb, alpha: f32) {
        self.fill_path(rounded(b, r), color, alpha);
    }

    /// A rectangle's outline, its corners rounded by `r`.
    pub fn outline(&mut self, b: Rect, r: f32, pen: Pen) {
        self.stroke(rounded(b, r), pen);
    }

    /// A disc.
    pub fn dot(&mut self, (x, y): (f32, f32), r: f32, color: Rgb) {
        if x.is_finite() && y.is_finite() {
            self.fill_path(sk::PathBuilder::from_circle(x, y, r), color, 1.0);
        }
    }

    /// A horizontal or vertical hairline on whole pixels.
    pub fn hair(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), color: Rgb) {
        let (s, thick) = (self.scale, self.scale.floor().max(1.0));
        let (x, y) = ((x0.min(x1) * s).round(), (y0.min(y1) * s).round());
        let w = ((x1 - x0).abs() * s).round().max(thick);
        let h = ((y1 - y0).abs() * s).round().max(thick);
        if let Some(r) = sk::Rect::from_xywh(x, y, w, h) {
            let paint = Canvas::paint(color, 1.0);
            let id = sk::Transform::identity();
            self.pixmap.fill_rect(r, &paint, id, None);
        }
    }

    /// A polyline cut to `clip`, broken where a point isn't finite.
    pub fn line(&mut self, pts: &[(f32, f32)], pen: Pen, clip: Rect) {
        let mut pb = sk::PathBuilder::new();
        let mut end = None;
        for w in pts.windows(2) {
            let Some((a, b)) = cut(w[0], w[1], clip) else {
                continue;
            };
            if end != Some(a) {
                pb.move_to(a.0, a.1);
            }
            pb.line_to(b.0, b.1);
            end = Some(b);
        }
        self.stroke(pb.finish(), pen);
    }

    /// A curve from `a` down (or up) to `b`, leaving and arriving upright.
    pub fn curve(&mut self, a: (f32, f32), b: (f32, f32), pen: Pen) {
        let mid = (a.1 + b.1) / 2.0;
        let mut pb = sk::PathBuilder::new();
        pb.move_to(a.0, a.1);
        pb.cubic_to(a.0, mid, b.0, mid, b.0, b.1);
        self.stroke(pb.finish(), pen);
    }

    /// A triangle, filled.
    pub fn triangle(&mut self, pts: [(f32, f32); 3], color: Rgb) {
        let mut pb = sk::PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        pb.line_to(pts[1].0, pts[1].1);
        pb.line_to(pts[2].0, pts[2].1);
        pb.close();
        self.fill_path(pb.finish(), color, 1.0);
    }

    /// How wide a text is, in logical px.
    pub fn width(&mut self, text: &str, ink: Ink) -> f32 {
        let e = self.book.pass().extent(text, ink.look(), self.scale);
        e.width / self.scale
    }

    /// A text with its baseline at `y`, its start, middle or end at `x`, cut with '…' to at
    /// most `max` px wide. Its width.
    pub fn text(
        &mut self,
        text: &str,
        ink: Ink,
        (x, y): (f32, f32),
        align: Align,
        max: f32,
    ) -> f32 {
        let s = self.scale;
        let mut pass = self.book.pass();
        let line = pass.set_within(text, ink.look(), s, max.max(0.0) * s);
        let w = line.extent.width;
        let x = match align {
            Align::Start => x * s,
            Align::Middle => x * s - w / 2.0,
            Align::End => x * s - w,
        };
        let (pw, ph) = (self.pixmap.width(), self.pixmap.height());
        if let Some(mut px) = Pixels::new(self.pixmap.data_mut(), pw, ph) {
            let clip = px.whole();
            pass.draw(&line, &mut px, x, y * s, ink.color, 255, clip);
        }
        w / s
    }
}

/// A rectangle's path, its corners rounded by `r` (at most half a side).
fn rounded(b: Rect, r: f32) -> Option<sk::Path> {
    let r = r.min(b.w / 2.0).min(b.h / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (b.x, b.y, b.right(), b.bottom());
    if r == 0.0 {
        return sk::Rect::from_ltrb(x0, y0, x1, y1).map(sk::PathBuilder::from_rect);
    }
    let mut p = sk::PathBuilder::new();
    p.move_to(x0 + r, y0);
    p.line_to(x1 - r, y0);
    p.quad_to(x1, y0, x1, y0 + r);
    p.line_to(x1, y1 - r);
    p.quad_to(x1, y1, x1 - r, y1);
    p.line_to(x0 + r, y1);
    p.quad_to(x0, y1, x0, y1 - r);
    p.line_to(x0, y0 + r);
    p.quad_to(x0, y0, x0 + r, y0);
    p.close();
    p.finish()
}

/// The part of the segment from `a` to `b` inside `r` (Liang and Barsky's way); None when no
/// part is, or an end isn't finite.
fn cut(a: (f32, f32), b: (f32, f32), r: Rect) -> Option<((f32, f32), (f32, f32))> {
    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let sides = [
        (-dx, a.0 - r.x),
        (dx, r.right() - a.0),
        (-dy, a.1 - r.y),
        (dy, r.bottom() - a.1),
    ];
    for (p, q) in sides {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    let at = |t: f32| (a.0 + t * dx, a.1 + t * dy);
    (t0 <= t1).then(|| (at(t0), at(t1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_by_name_and_segments_cut_to_a_rectangle() {
        let th = Theme::default();
        assert_eq!(th.named("red"), Some(hex("e66767")));
        assert_eq!(th.named("Dim"), Some(th.dim));
        assert_eq!(th.named("#0A0B0C"), Some(Rgb(10, 11, 12)));
        assert_eq!(th.named("pink"), None);
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            cut((-5.0, 5.0), (15.0, 5.0), r),
            Some(((0.0, 5.0), (10.0, 5.0)))
        );
        assert_eq!(cut((-5.0, 20.0), (15.0, 20.0), r), None);
        assert_eq!(cut((f32::NAN, 1.0), (2.0, 2.0), r), None);
    }
}
