//! The font policy, as data in and data out: the cell's size from the primary
//! face's metrics, which source draws each character, and where a glyph goes in its
//! cells, squeezed if it doesn't fit. The driver asks the backend's faces for the
//! inputs. Nothing here touches a font.
#![warn(missing_docs)]

use crate::frame::{GlyphBitmap, Plane};
use crate::sprite::{self, Line};
use std::ops::RangeInclusive;
use std::path::PathBuf;

/// A face's vertical metrics and underline, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMetrics {
    /// Above the baseline.
    pub ascent: f64,
    /// Below the baseline, as a positive number.
    pub descent: f64,
    /// Between lines.
    pub leading: f64,
    /// The underline's center, from the baseline, negative below it.
    pub underline_position: f64,
    /// The underline's thickness.
    pub underline_thickness: f64,
    /// The height of a lowercase x.
    pub x_height: f64,
}

/// A rectangle in a face's coordinates: pixels from the pen, with y going up.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ink {
    /// The left edge.
    pub x: f64,
    /// The bottom edge.
    pub y: f64,
    /// The width.
    pub w: f64,
    /// The height.
    pub h: f64,
}

impl Ink {
    /// Whether it covers nothing.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// The rectangle around both.
    pub fn union(self, o: Ink) -> Ink {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        let right = (self.x + self.w).max(o.x + o.w);
        let top = (self.y + self.h).max(o.y + o.h);
        Ink {
            x,
            y,
            w: right - x,
            h: top - y,
        }
    }
}

/// A cell's geometry in physical pixels, from the primary face (scope §5.11).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    /// The cell's width.
    pub w: u32,
    /// The cell's height.
    pub h: u32,
    /// The baseline, in pixels from the cell's top.
    pub baseline: f64,
    /// A light line's thickness, for box glyphs and underlines.
    pub thin: u32,
    /// The underline's top, in pixels from the cell's top.
    pub underline_y: i32,
    /// The strike-through's top, in pixels from the cell's top.
    pub strike_y: i32,
    /// Physical pixels per logical pixel.
    pub scale: f64,
}

impl CellMetrics {
    /// The cell for a primary face with these metrics and this advance.
    ///
    /// The width is the advance rounded down, and the height includes the leading,
    /// so a font's own box glyphs tile too. The small margin keeps an advance that
    /// comes out a hair under a whole pixel from losing that pixel.
    pub fn new(m: FaceMetrics, advance: f64, scale: f64) -> CellMetrics {
        let h = (m.ascent + m.descent + m.leading).round().max(1.0);
        let thin = m.underline_thickness.round().max(1.0);
        let underline = m.ascent - m.underline_position - m.underline_thickness / 2.0;
        CellMetrics {
            w: (advance + 1e-3).floor().max(1.0) as u32,
            h: h as u32,
            baseline: m.ascent,
            thin: thin as u32,
            underline_y: underline.round().min(h - 2.0 * thin) as i32,
            strike_y: (m.ascent - m.x_height / 2.0 - thin / 2.0).round() as i32,
            scale,
        }
    }

    /// `n` logical pixels in physical pixels: at least one, and whole, so a line
    /// stays crisp. This is the one place display scaling applies to a cursor.
    pub fn logical(&self, n: f64) -> u32 {
        (n * self.scale).round().max(1.0) as u32
    }

    /// A bitmap the size of the cell, from a sprite's coverage.
    pub fn cell_bitmap(&self, data: Vec<u8>) -> GlyphBitmap {
        GlyphBitmap {
            w: self.w,
            h: self.h,
            left: 0,
            top: 0,
            plane: Plane::Gray,
            data,
        }
    }

    /// The pixels a grid of `cols` × `rows` of these cells takes, and no more.
    pub fn grid(&self, cols: usize, rows: usize) -> (u32, u32) {
        (cols as u32 * self.w, rows as u32 * self.h)
    }

    /// The bitmap for a line over a cell.
    pub fn line_bitmap(&self, l: Line) -> GlyphBitmap {
        let y = match l {
            Line::Under(_) => self.underline_y,
            Line::Strike => self.strike_y,
        };
        self.cell_bitmap(sprite::line(l, self.w, self.h, y, self.thin))
    }
}

/// Where a glyph goes: the bitmap to draw it into, and the pen inside that bitmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The scale to draw at, 1 unless it's squeezed.
    pub scale: f64,
    /// The pen's x in the bitmap.
    pub pen_x: f64,
    /// The pen's y in the bitmap, from its bottom edge, as Core Graphics counts.
    pub pen_y: f64,
    /// The bitmap's left edge, in pixels from the cell's left edge.
    pub left: i32,
    /// The bitmap's top edge, in pixels from the cell's top edge.
    pub top: i32,
    /// The bitmap's width.
    pub w: u32,
    /// The bitmap's height.
    pub h: u32,
}

/// Fit a shaped glyph with this `ink` and `advance` into `span` cells.
///
/// A glyph from the primary font stays where its font puts it. The cell's width is
/// the font's advance rounded down, so an advance up to a pixel per cell over is what
/// it's meant to have. Past that it's too wide, as Term's long arrows are, so it's
/// squeezed and centered. A wide character that the font draws narrower than its
/// cells is centered across them.
///
/// A fallback face's glyph has no such promise. It's squeezed if its ink or advance
/// doesn't fit across or its ink doesn't fit down, and centered across the span by its
/// advance. If its ink would still leave the span by more than half a pixel, it moves
/// back in, and if its ink would leave the cell's height, it moves to the cell's
/// middle. `None` if it has no ink.
pub fn fit(ink: Ink, advance: f64, span: u32, cell: &CellMetrics, fallback: bool) -> Option<Fit> {
    if ink.is_empty() {
        return None;
    }
    let (span_w, cell_h) = (f64::from(span * cell.w), f64::from(cell.h));
    let (too_wide, across) = if fallback {
        (advance.max(ink.w) > span_w + 0.5, ink.w)
    } else {
        (advance > f64::from(span * (cell.w + 1)), ink.x + ink.w)
    };
    let mut scale = 1.0_f64;
    if too_wide {
        scale = span_w / advance.max(across);
    }
    if fallback && ink.h * scale > cell_h {
        scale = cell_h / ink.h;
    }
    let mut pen_x = if scale < 1.0 || fallback || span > 1 {
        ((span_w - advance * scale) / 2.0).max(0.0)
    } else {
        0.0
    };
    if fallback {
        let left = pen_x + ink.x * scale;
        let right = left + ink.w * scale;
        if left < -0.5 {
            pen_x -= left;
        } else if right > span_w + 0.5 {
            pen_x -= right - span_w;
        }
    }
    let mut baseline = cell.baseline;
    if fallback {
        let top = baseline - (ink.y + ink.h) * scale;
        let bottom = baseline - ink.y * scale;
        if top < 0.0 || bottom > cell_h {
            baseline += (cell_h - (bottom + top)) / 2.0;
        }
    }
    let (x0, y0) = (pen_x + ink.x * scale, baseline - (ink.y + ink.h) * scale);
    // A pixel of margin on each side, for the antialiased edge.
    let (l, t) = (x0.floor() as i32 - 1, y0.floor() as i32 - 1);
    let r = (x0 + ink.w * scale).ceil() as i32 + 1;
    let b = (y0 + ink.h * scale).ceil() as i32 + 1;
    Some(Fit {
        scale,
        pen_x: pen_x - f64::from(l),
        pen_y: f64::from(b) - baseline,
        left: l,
        top: t,
        w: (r - l) as u32,
        h: (b - t) as u32,
    })
}

/// The first face in `chain` that has every character of `text`, as `has` reports.
/// A cluster stays in one face where it can, so its marks fit its base.
pub fn covering(chain: &[usize], text: &str, has: impl Fn(usize, char) -> bool) -> Option<usize> {
    let all = |&i: &usize| text.chars().all(|c| has(i, c));
    chain.iter().copied().find(all)
}

/// Who draws a range of characters.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Draw {
    /// The renderer, so they join across cells.
    Renderer,
    /// The primary font: a range handed back, as braille is by default.
    Primary,
    /// These font files' faces first, in this order, then the primary font.
    Files(Vec<PathBuf>),
}

/// A per-range override (scope §5.11). The first rule whose range holds a cluster's
/// first character decides it, and with no rule the primary font draws it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RangeRule {
    /// The characters it covers.
    pub range: RangeInclusive<char>,
    /// Who draws them.
    pub draw: Draw,
}

impl RangeRule {
    /// The rule for a range.
    pub fn new(range: RangeInclusive<char>, draw: Draw) -> RangeRule {
        RangeRule { range, draw }
    }

    /// DIE's defaults: the renderer draws box, block and powerline glyphs, and
    /// braille stays in the font, as your `init.el` pins it.
    pub fn defaults() -> Vec<RangeRule> {
        vec![
            RangeRule::new('\u{2500}'..='\u{259F}', Draw::Renderer),
            RangeRule::new('\u{E0B0}'..='\u{E0B6}', Draw::Renderer),
            RangeRule::new('\u{2800}'..='\u{28FF}', Draw::Primary),
        ]
    }

    /// The font files it pins, in the order to try them.
    pub fn files(&self) -> &[PathBuf] {
        match &self.draw {
            Draw::Files(files) => files,
            Draw::Renderer | Draw::Primary => &[],
        }
    }
}

/// Where a cluster's pixels come from, as the rules say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Source {
    /// The character for the renderer to draw, if the rules give it this cluster. It
    /// draws single characters only, so a character with a mark has none. If it
    /// doesn't draw this one either, the faces do.
    pub sprite: Option<char>,
    /// The rule that decided, as an index into the rules, if any covers it.
    pub rule: Option<usize>,
}

/// What `rules` say about `text`: the first rule whose range holds its first character
/// decides.
pub fn source(rules: &[RangeRule], text: &str) -> Source {
    let mut chars = text.chars();
    let first = chars.next();
    let alone = chars.next().is_none();
    let rule = first.and_then(|f| rules.iter().position(|r| r.range.contains(&f)));
    let drawn = rule.is_some_and(|r| rules[r].draw == Draw::Renderer);
    Source {
        sprite: first.filter(|_| alone && drawn),
        rule,
    }
}

/// The faces each of `rules` pins, as indexes, with the rules' files loaded in order
/// after the first `first` faces.
pub fn pins(rules: &[RangeRule], first: usize) -> Vec<Vec<usize>> {
    let mut next = first;
    let pinned = |rule: &RangeRule| {
        let faces = (next..next + rule.files().len()).collect();
        next += rule.files().len();
        faces
    };
    rules.iter().map(pinned).collect()
}

/// Which of the primary family's four faces draws a style: regular, bold, italic,
/// bold italic.
pub fn styled_face(bold: bool, italic: bool) -> usize {
    usize::from(bold) + 2 * usize::from(italic)
}

/// The order to try faces in, as indexes: the faces a rule pins, then the styled face,
/// then the regular one. The system's fallback comes after these.
pub fn chain(pinned: &[usize], styled: usize) -> Vec<usize> {
    let mut out = pinned.to_vec();
    out.push(styled);
    if styled != 0 {
        out.push(0);
    }
    out
}
