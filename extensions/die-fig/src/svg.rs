//! A scene to SVG at 1×: paths as `<path>`, text as `<text>` in a system font stack with
//! `tabular-nums` where asked, gradients as `<linearGradient>`, clips as `<clipPath>`.
//!
//! The document is the scene's logical size, and draws only what the scene does: a scene that
//! wants a ground draws one. Each gradient, clip and shadow's filter is defined just before the
//! element that uses it, with an id `fig-` and a letter and a number. Hairlines are 1 px with
//! `shape-rendering="crispEdges"` and a stroke that doesn't scale with the picture, so they stay
//! one px when it's shown larger. A shadow is a black rounded rectangle under a Gaussian blur of
//! σ = `blur / 2`, as the raster draws it, the blur at most [`MAX_BLUR`]. A text keeps its
//! spaces (`xml:space="preserve"`), as the raster does, and one with a `max_width` is clipped to
//! it, since a browser's font isn't the raster's and the cut was made by the layout. Pushes and pops nest
//! as groups; a pop that doesn't match the innermost push closes the groups inside it and opens
//! them again.

use crate::raster::{MAX_BLUR, plain};
use crate::scene::{Anchor, Cap, Item, Join, Paint, PathData, Rect, Scene, Stroke, Verb, Weight};
use crate::theme::Color;
use std::fmt::Write as _;

/// The families a text asks a browser for: SF where there is one, the system's face elsewhere.
pub const FONT_FAMILY: &str =
    "-apple-system, BlinkMacSystemFont, 'SF Pro Text', system-ui, 'Helvetica Neue', sans-serif";
/// The families a monospace text asks for.
pub const MONO_FAMILY: &str = "ui-monospace, 'SF Mono', Menlo, monospace";

/// The scene as an SVG document at 1× (text as `<text>` with a system font stack and
/// tabular-nums where asked; gradients as `<linearGradient>`; clips as `<clipPath>`).
pub fn write(scene: &Scene) -> String {
    let mut w = Writer {
        out: String::new(),
        ids: 0,
        open: Vec::new(),
    };
    let (sw, sh) = (num(scene.width), num(scene.height));
    let _ = writeln!(
        w.out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{sw}\" height=\"{sh}\" \
         viewBox=\"0 0 {sw} {sh}\">"
    );
    for item in &scene.items {
        w.item(item);
    }
    for _ in 0..w.open.len() {
        w.out.push_str("</g>\n");
    }
    w.out.push_str("</svg>\n");
    w.out
}

/// A group that's open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Clip,
    Opacity,
}

struct Writer {
    out: String,
    ids: u32,
    /// The groups open, innermost last, each with its opening tag.
    open: Vec<(Group, String)>,
}

impl Writer {
    /// A new id: `fig-` and `kind` and a number.
    fn id(&mut self, kind: char) -> String {
        self.ids += 1;
        format!("fig-{kind}{}", self.ids)
    }

    fn push(&mut self, kind: Group, tag: String) {
        self.out.push_str(&tag);
        self.open.push((kind, tag));
    }

    /// Closes the innermost group of `kind`, and opens again those inside it.
    fn pop(&mut self, kind: Group) {
        let Some(i) = self.open.iter().rposition(|(k, _)| *k == kind) else {
            return;
        };
        let inside: Vec<_> = self.open.drain(i + 1..).collect();
        self.open.pop();
        for _ in 0..=inside.len() {
            self.out.push_str("</g>\n");
        }
        for (k, tag) in inside {
            self.push(k, tag);
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::PushClip(r) => {
                let id = self.id('c');
                let _ = writeln!(
                    self.out,
                    "<defs><clipPath id=\"{id}\"><rect{}/></clipPath></defs>",
                    rect_attrs(*r)
                );
                self.push(Group::Clip, format!("<g clip-path=\"url(#{id})\">\n"));
            }
            Item::PopClip => self.pop(Group::Clip),
            Item::PushOpacity(a) => {
                self.push(
                    Group::Opacity,
                    format!("<g opacity=\"{}\">\n", num(unit(*a))),
                );
            }
            Item::PopOpacity => self.pop(Group::Opacity),
            Item::Path { path, fill, stroke } => {
                let fill = match fill {
                    Some(f) => self.paint(f, "fill"),
                    None => " fill=\"none\"".into(),
                };
                let stroke = match stroke {
                    Some(s) => self.stroke(s),
                    None => String::new(),
                };
                let _ = writeln!(self.out, "<path d=\"{}\"{fill}{stroke}/>", d(path));
            }
            Item::Text {
                text,
                x,
                y,
                spec,
                anchor,
                color,
                alpha,
                max_width,
            } => {
                let clip = match max_width {
                    Some(m) => {
                        let m = m.max(0.0);
                        let left = match anchor {
                            Anchor::Start => *x,
                            Anchor::Middle => x - m / 2.0,
                            Anchor::End => x - m,
                        };
                        let r = Rect::new(left, y - 1.2 * spec.size, m, 1.7 * spec.size);
                        let id = self.id('t');
                        let _ = writeln!(
                            self.out,
                            "<defs><clipPath id=\"{id}\"><rect{}/></clipPath></defs>",
                            rect_attrs(r)
                        );
                        format!(" clip-path=\"url(#{id})\"")
                    }
                    None => String::new(),
                };
                let anchor = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let weight = match spec.weight {
                    Weight::Regular => 400,
                    Weight::Medium => 500,
                    Weight::Semibold => 600,
                };
                let family = if spec.mono { MONO_FAMILY } else { FONT_FAMILY };
                let _ = write!(
                    self.out,
                    "<text x=\"{}\" y=\"{}\" font-family=\"{family}\" font-size=\"{}\" \
                     font-weight=\"{weight}\" text-anchor=\"{anchor}\" xml:space=\"preserve\"{}",
                    num(*x),
                    num(*y),
                    num(spec.size),
                    solid("fill", *color, *alpha),
                );
                if spec.tabular {
                    self.out
                        .push_str(" style=\"font-variant-numeric:tabular-nums\"");
                }
                let _ = writeln!(self.out, "{clip}>{}</text>", escape(text));
            }
            Item::Shadow {
                rect,
                radius,
                blur,
                dy,
                alpha,
            } => {
                let r = Rect::new(rect.x, rect.y + dy, rect.w, rect.h);
                let blur = if blur.is_nan() {
                    0.0
                } else {
                    blur.clamp(0.0, MAX_BLUR)
                };
                let filter = if blur > 0.0 {
                    let id = self.id('s');
                    let _ = writeln!(
                        self.out,
                        "<defs><filter id=\"{id}\" filterUnits=\"userSpaceOnUse\"{}>\
                         <feGaussianBlur stdDeviation=\"{}\"/></filter></defs>",
                        rect_attrs(r.inflate(2.0 * blur)),
                        num(blur / 2.0)
                    );
                    format!(" filter=\"url(#{id})\"")
                } else {
                    String::new()
                };
                let most = (r.w.min(r.h) / 2.0).max(0.0);
                let rx = num(radius.clamp(0.0, most));
                let _ = writeln!(
                    self.out,
                    "<rect{} rx=\"{rx}\" ry=\"{rx}\"{}{filter}/>",
                    rect_attrs(r),
                    solid("fill", Color(0, 0, 0), *alpha)
                );
            }
        }
    }

    /// A paint as `what`'s attributes; a gradient defined first.
    fn paint(&mut self, paint: &Paint, what: &str) -> String {
        match paint {
            Paint::Solid { color, alpha } => solid(what, *color, *alpha),
            Paint::Linear { from, to, stops } => {
                let mut stops: Vec<_> = stops.iter().filter(|s| s.0.is_finite()).collect();
                if stops.is_empty() {
                    return format!(" {what}=\"none\"");
                }
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                let id = self.id('g');
                let _ = write!(
                    self.out,
                    "<defs><linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" \
                     x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
                    num(from.0),
                    num(from.1),
                    num(to.0),
                    num(to.1)
                );
                for (o, c, a) in stops {
                    let _ = write!(
                        self.out,
                        "<stop offset=\"{}\" stop-color=\"{}\" stop-opacity=\"{}\"/>",
                        num(o.clamp(0.0, 1.0)),
                        hex(*c),
                        num(unit(*a))
                    );
                }
                self.out.push_str("</linearGradient></defs>\n");
                format!(" {what}=\"url(#{id})\"")
            }
        }
    }

    /// A stroke's attributes.
    fn stroke(&mut self, s: &Stroke) -> String {
        let mut out = self.paint(&s.paint, "stroke");
        let width = if s.hairline { 1.0 } else { s.width.max(0.0) };
        let _ = write!(out, " stroke-width=\"{}\"", num(width));
        let cap = match s.cap {
            Cap::Butt => "butt",
            Cap::Round => "round",
            Cap::Square => "square",
        };
        let join = match s.join {
            Join::Miter => "miter",
            Join::Round => "round",
            Join::Bevel => "bevel",
        };
        let _ = write!(out, " stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"");
        if let Some((on, off)) = s.dash {
            let _ = write!(out, " stroke-dasharray=\"{} {}\"", num(on), num(off));
        }
        if s.hairline {
            out.push_str(" shape-rendering=\"crispEdges\" vector-effect=\"non-scaling-stroke\"");
        }
        out
    }
}

/// A colour at an opacity as `what`'s attributes.
fn solid(what: &str, c: Color, alpha: f32) -> String {
    let a = unit(alpha);
    if a >= 1.0 {
        format!(" {what}=\"{}\"", hex(c))
    } else {
        format!(" {what}=\"{}\" {what}-opacity=\"{}\"", hex(c), num(a))
    }
}

/// An opacity clamped to 0 to 1, a NaN as 0.
fn unit(a: f32) -> f32 {
    if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) }
}

fn rect_attrs(r: Rect) -> String {
    format!(
        " x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
        num(r.x),
        num(r.y),
        num(r.w.max(0.0)),
        num(r.h.max(0.0))
    )
}

/// `#rrggbb`.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// A number with at most three decimals and no trailing zeros: `12`, `0.5`, `-3.125`.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "-0" | "" => "0".into(),
        s => s.into(),
    }
}

/// A path's steps as `d`.
fn d(path: &PathData) -> String {
    let mut out = String::new();
    for v in &path.verbs {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = match *v {
            Verb::Move(x, y) => write!(out, "M{} {}", num(x), num(y)),
            Verb::Line(x, y) => write!(out, "L{} {}", num(x), num(y)),
            Verb::Quad(a, b, x, y) => {
                write!(out, "Q{} {} {} {}", num(a), num(b), num(x), num(y))
            }
            Verb::Cubic(a, b, c, e, x, y) => write!(
                out,
                "C{} {} {} {} {} {}",
                num(a),
                num(b),
                num(c),
                num(e),
                num(x),
                num(y)
            ),
            Verb::Close => write!(out, "Z"),
        };
    }
    out
}

/// Text as XML content, as the raster draws it: the markup's characters as entities, a tab or
/// a line break as a space, and the other characters XML can't hold (the control characters,
/// U+FFFE and U+FFFF) left out.
fn escape(text: &str) -> String {
    let text = plain(text);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}
