//! The display list a figure is drawn from: paths, text, shadows, clips and opacity, in the
//! order they're drawn, and the [`Region`]s that say what each place on screen is, for the
//! pointer.
//!
//! Units are logical px, laid out for a given `scale`; the raster multiplies by it and the SVG
//! writer takes the 1× layout. Text is measured through [`Measure`], so layout needs no fonts in
//! a test: [`FixedMeasure`] stands in for them.

use crate::data::SeriesKey;
use crate::theme::Color;

/// A rectangle: its top-left corner and its size.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub w: f32,
    /// The height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from its corner and size.
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Whether a point is in it: its left and top edges are, its right and bottom aren't.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The smallest rectangle that holds both. A rectangle with no area holds nothing, so the
    /// union with one is the other.
    pub fn union(&self, o: &Rect) -> Rect {
        if o.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *o;
        }
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        let (r, b) = (self.right().max(o.right()), self.bottom().max(o.bottom()));
        Rect::new(x, y, r - x, b - y)
    }

    /// What both cover, if anything.
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let (r, b) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        let r = Rect::new(x, y, r - x, b - y);
        (!r.is_empty()).then_some(r)
    }

    /// The rectangle grown `d` on every side (shrunk for a negative `d`).
    pub fn inflate(&self, d: f32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }

    /// The right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Whether it has no area.
    pub fn is_empty(&self) -> bool {
        !(self.w > 0.0 && self.h > 0.0)
    }
}

/// How heavy a text's face is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Weight {
    /// Text.
    #[default]
    Regular,
    /// 500.
    Medium,
    /// 600: titles.
    Semibold,
}

/// How a text is set: its size in logical px, its weight, tabular figures, the monospace.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextSpec {
    /// The size in logical px.
    pub size: f32,
    /// The weight.
    pub weight: Weight,
    /// Digits as tabular figures, so numbers in a column line up.
    pub tabular: bool,
    /// The monospace.
    pub mono: bool,
}

impl TextSpec {
    /// Regular text at a size.
    pub fn new(size: f32) -> TextSpec {
        TextSpec {
            size,
            ..TextSpec::default()
        }
    }

    /// The same at a weight.
    pub fn weight(self, weight: Weight) -> TextSpec {
        TextSpec { weight, ..self }
    }

    /// The same with tabular figures.
    pub fn tabular(self) -> TextSpec {
        TextSpec {
            tabular: true,
            ..self
        }
    }

    /// The same in the monospace.
    pub fn mono(self) -> TextSpec {
        TextSpec { mono: true, ..self }
    }
}

/// Where a text's `x` is on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Its start.
    Start,
    /// Its middle.
    Middle,
    /// Its end.
    End,
}

/// How a stroke's ends are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// Square, at the end.
    Butt,
    /// Round, half the width past the end.
    Round,
    /// Square, half the width past the end.
    Square,
}

/// How a stroke's corners are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// Pointed.
    Miter,
    /// Round.
    Round,
    /// Cut off.
    Bevel,
}

/// One step of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verb {
    /// A new subpath at a point.
    Move(f32, f32),
    /// A line to a point.
    Line(f32, f32),
    /// A quadratic curve through a control point to a point.
    Quad(f32, f32, f32, f32),
    /// A cubic curve through two control points to a point.
    Cubic(f32, f32, f32, f32, f32, f32),
    /// Back to the subpath's start.
    Close,
}

/// A path: its steps in order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PathData {
    /// The steps.
    pub verbs: Vec<Verb>,
}

/// How far a cubic's control points sit along the tangent for a quarter circle of radius 1.
const KAPPA: f32 = 0.552_284_8;

impl PathData {
    /// Lines through the points in order, open. No points, no path.
    pub fn polyline(pts: &[(f32, f32)]) -> PathData {
        let mut verbs = Vec::with_capacity(pts.len());
        for (i, &(x, y)) in pts.iter().enumerate() {
            verbs.push(if i == 0 {
                Verb::Move(x, y)
            } else {
                Verb::Line(x, y)
            });
        }
        PathData { verbs }
    }

    /// A rectangle, clockwise from its top-left corner, closed.
    pub fn rect(r: Rect) -> PathData {
        PathData {
            verbs: vec![
                Verb::Move(r.x, r.y),
                Verb::Line(r.right(), r.y),
                Verb::Line(r.right(), r.bottom()),
                Verb::Line(r.x, r.bottom()),
                Verb::Close,
            ],
        }
    }

    /// A rectangle with rounded corners, `radii` top-left, top-right, bottom-right and
    /// bottom-left, each clamped to half the shorter side. Corners of radius 0 are square.
    pub fn rounded(r: Rect, radii: [f32; 4]) -> PathData {
        let most = (r.w.min(r.h) / 2.0).max(0.0);
        let [tl, tr, br, bl] = radii.map(|v| v.clamp(0.0, most));
        let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
        let k = 1.0 - KAPPA;
        let mut verbs = vec![Verb::Move(x0 + tl, y0), Verb::Line(x1 - tr, y0)];
        if tr > 0.0 {
            verbs.push(Verb::Cubic(x1 - tr * k, y0, x1, y0 + tr * k, x1, y0 + tr));
        }
        verbs.push(Verb::Line(x1, y1 - br));
        if br > 0.0 {
            verbs.push(Verb::Cubic(x1, y1 - br * k, x1 - br * k, y1, x1 - br, y1));
        }
        verbs.push(Verb::Line(x0 + bl, y1));
        if bl > 0.0 {
            verbs.push(Verb::Cubic(x0 + bl * k, y1, x0, y1 - bl * k, x0, y1 - bl));
        }
        verbs.push(Verb::Line(x0, y0 + tl));
        if tl > 0.0 {
            verbs.push(Verb::Cubic(x0, y0 + tl * k, x0 + tl * k, y0, x0 + tl, y0));
        }
        verbs.push(Verb::Close);
        PathData { verbs }
    }

    /// A circle, as four cubics clockwise from its top, closed.
    pub fn circle(cx: f32, cy: f32, r: f32) -> PathData {
        let mut path = PathData::arc(cx, cy, r, 0.0, 360.0);
        path.verbs.push(Verb::Close);
        path
    }

    /// An open arc of radius `r` about (cx, cy), clockwise from `from` to `to` degrees, both
    /// measured clockwise from north (up, negative y), as a compass does. A `to` at or
    /// before `from` goes round past north (350 to 10 is 20°); equal angles are a whole turn.
    /// Each quarter turn or less is one cubic.
    pub fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> PathData {
        let mut sweep = (to - from).rem_euclid(360.0);
        if sweep == 0.0 {
            sweep = 360.0;
        }
        let at = |deg: f32| {
            let a = deg.to_radians();
            (cx + r * a.sin(), cy - r * a.cos())
        };
        let parts = (sweep / 90.0).ceil().max(1.0) as usize;
        let step = sweep / parts as f32;
        // A cubic's handles for an arc of `step` degrees.
        let handle = 4.0 / 3.0 * (step.to_radians() / 4.0).tan() * r;
        let start = at(from);
        let mut verbs = vec![Verb::Move(start.0, start.1)];
        for i in 0..parts {
            let (a0, a1) = (from + step * i as f32, from + step * (i + 1) as f32);
            let (p0, p1) = (at(a0), at(a1));
            // The tangent, clockwise, at an angle from north: (cos, sin) on screen.
            let t = |deg: f32| {
                let a = deg.to_radians();
                (a.cos(), a.sin())
            };
            let (t0, t1) = (t(a0), t(a1));
            verbs.push(Verb::Cubic(
                p0.0 + handle * t0.0,
                p0.1 + handle * t0.1,
                p1.0 - handle * t1.0,
                p1.1 - handle * t1.1,
                p1.0,
                p1.1,
            ));
        }
        PathData { verbs }
    }

    /// The box every point and control point of the path is in, which holds the path. None
    /// for a path with no points.
    pub fn bounds(&self) -> Option<Rect> {
        let mut lo = (f32::INFINITY, f32::INFINITY);
        let mut hi = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut take = |x: f32, y: f32| {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        };
        for v in &self.verbs {
            match *v {
                Verb::Move(x, y) | Verb::Line(x, y) => take(x, y),
                Verb::Quad(a, b, x, y) => {
                    take(a, b);
                    take(x, y);
                }
                Verb::Cubic(a, b, c, d, x, y) => {
                    take(a, b);
                    take(c, d);
                    take(x, y);
                }
                Verb::Close => {}
            }
        }
        (lo.0 <= hi.0).then(|| Rect::new(lo.0, lo.1, hi.0 - lo.0, hi.1 - lo.1))
    }
}

/// What a fill or a stroke is painted with.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// One colour at an opacity, 0 to 1.
    Solid {
        /// The colour.
        color: Color,
        /// Its opacity.
        alpha: f32,
    },
    /// A gradient along a line, with stops at offsets 0 to 1 along it.
    Linear {
        /// Where offset 0 is.
        from: (f32, f32),
        /// Where offset 1 is.
        to: (f32, f32),
        /// Each stop's offset, colour and opacity.
        stops: Vec<(f32, Color, f32)>,
    },
}

impl Paint {
    /// One colour at an opacity.
    pub fn solid(color: Color, alpha: f32) -> Paint {
        Paint::Solid { color, alpha }
    }
}

/// How a path's outline is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// Its width in logical px.
    pub width: f32,
    /// What it's painted with.
    pub paint: Paint,
    /// Its ends.
    pub cap: Cap,
    /// Its corners.
    pub join: Join,
    /// Dashes: so long on, so long off.
    pub dash: Option<(f32, f32)>,
    /// A hairline: one logical px wide, snapped to whole physical pixels (+0.5 for odd
    /// widths) so it's never blurred.
    pub hairline: bool,
}

impl Stroke {
    /// A solid stroke of a width in a colour: butt ends, mitred corners, no dashes.
    pub fn solid(width: f32, color: Color, alpha: f32) -> Stroke {
        Stroke {
            width,
            paint: Paint::solid(color, alpha),
            cap: Cap::Butt,
            join: Join::Miter,
            dash: None,
            hairline: false,
        }
    }

    /// A hairline in a colour.
    pub fn hairline(color: Color, alpha: f32) -> Stroke {
        Stroke {
            hairline: true,
            ..Stroke::solid(1.0, color, alpha)
        }
    }

    /// How far past its path the stroke can reach: half its width, as far as a mitre goes at
    /// tiny-skia's limit of 4 for mitred corners and a square cap's corner.
    fn reach(&self) -> f32 {
        let half = if self.hairline { 0.5 } else { self.width / 2.0 };
        match (self.join, self.cap) {
            (Join::Miter, _) => half * 4.0,
            (_, Cap::Square) => half * std::f32::consts::SQRT_2,
            _ => half,
        }
    }
}

/// One thing drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A path, filled, stroked, or both (the fill first).
    Path {
        /// The path.
        path: PathData,
        /// Its fill, by the nonzero rule.
        fill: Option<Paint>,
        /// Its outline.
        stroke: Option<Stroke>,
    },
    /// A line of text.
    Text {
        /// What it says.
        text: Box<str>,
        /// Where its anchor is.
        x: f32,
        /// Its baseline.
        y: f32,
        /// How it's set.
        spec: TextSpec,
        /// Which point of it `x` is.
        anchor: Anchor,
        /// Its colour.
        color: Color,
        /// Its opacity, 0 to 1.
        alpha: f32,
        /// The widest it may be: cut with '…' past this.
        max_width: Option<f32>,
    },
    /// A soft shadow under a rounded rectangle: the card's. It reaches at most twice `blur`
    /// past its rectangle, moved down `dy`.
    Shadow {
        /// The rectangle casting it.
        rect: Rect,
        /// Its corners' radius.
        radius: f32,
        /// How far it blurs.
        blur: f32,
        /// How far down it falls.
        dy: f32,
        /// Its opacity at the darkest, 0 to 1.
        alpha: f32,
    },
    /// What follows is clipped to this, and to any clip already pushed.
    PushClip(Rect),
    /// The last clip pushed no longer applies.
    PopClip,
    /// What follows is drawn as a group at this opacity, 0 to 1.
    PushOpacity(f32),
    /// The last opacity pushed no longer applies.
    PopOpacity,
}

/// What a place on screen is, for the pointer. Precedence is the order the surface asks.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// The stale badge: the last parse failed, and the figure shown is the one before.
    Badge,
    /// The error figure in its symbolic state, or the log's header: a click turns it over.
    ErrorFigure,
    /// An entry of the error figure's log, from 0.
    LogEntry(u32),
    /// A row of the detail panel, by its index in the tree.
    Detail(u32),
    /// A fold triangle of the detail panel: its branch's row, by its index in the tree.
    DetailFold(u32),
    /// A legend key: its swatch and name.
    LegendKey(SeriesKey),
    /// A panel's x axis strip.
    AxisX(u16),
    /// A panel's y axis strip.
    AxisY(u16),
    /// A panel's title.
    PanelTitle(u16),
    /// A bar or a histogram bin: the band, and the segment under the pointer if stacked.
    Bar {
        /// The panel.
        panel: u16,
        /// The band, from 0.
        band: u32,
        /// The series of the segment, if the bar has several.
        series: Option<SeriesKey>,
    },
    /// A heatmap's cell.
    Cell {
        /// The panel.
        panel: u16,
        /// The row.
        row: u32,
        /// The column.
        col: u32,
    },
    /// A graph's node, by its id.
    Node(Box<str>),
    /// A gantt's task, by its key: its bar, or its label in the gutter.
    Task(Box<str>),
    /// A panel's plot, where no mark is.
    Plot(u16),
    /// Padding, titles, the gaps between panels: off the plot.
    Chrome,
}

/// A place on screen and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// Where.
    pub rect: Rect,
    /// What.
    pub hit: Hit,
}

/// A figure's display list and its regions, for a size and a scale.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    /// The width in logical px.
    pub width: f32,
    /// The height in logical px.
    pub height: f32,
    /// Physical px to a logical px, which the layout was made for.
    pub scale: f32,
    /// What's drawn, in order.
    pub items: Vec<Item>,
    /// What places are, in the order pushed.
    pub regions: Vec<Region>,
}

impl Scene {
    /// An empty scene of a size, laid out for a scale.
    pub fn new(width: f32, height: f32, scale: f32) -> Scene {
        Scene {
            width,
            height,
            scale,
            items: Vec::new(),
            regions: Vec::new(),
        }
    }

    /// Draws `item` over what's drawn so far.
    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    /// Says what a place is. A region pushed later is over those before it.
    pub fn region(&mut self, rect: Rect, hit: Hit) {
        self.regions.push(Region { rect, hit });
    }

    /// What's at a point: the last-pushed region that holds it.
    pub fn hit(&self, x: f32, y: f32) -> Option<&Hit> {
        let r = self.regions.iter().rev().find(|r| r.rect.contains(x, y));
        r.map(|r| &r.hit)
    }

    /// The union of everything drawn, each item within the clips pushed around it: what an
    /// overlay damages. Generous where it can't be exact: a stroke reaches as far as a mitre
    /// can, a text an em a character (or its `max_width`) and from 1.2 em above its baseline to
    /// half an em below, a shadow twice its blur, and everything a px more for antialiasing.
    /// None if nothing is drawn.
    pub fn bounds(&self) -> Option<Rect> {
        let mut clips: Vec<Option<Rect>> = Vec::new();
        let mut out: Option<Rect> = None;
        for item in &self.items {
            let drawn = match item {
                Item::PushClip(r) => {
                    let inner = match clips.last() {
                        Some(Some(c)) => c.intersect(r),
                        Some(None) => None,
                        None => (!r.is_empty()).then_some(*r),
                    };
                    clips.push(inner);
                    continue;
                }
                Item::PopClip => {
                    clips.pop();
                    continue;
                }
                Item::PushOpacity(_) | Item::PopOpacity => continue,
                Item::Path { path, fill, stroke } => {
                    let Some(b) = path.bounds() else { continue };
                    match (fill, stroke) {
                        (None, None) => continue,
                        (_, Some(s)) => b.inflate(s.reach()),
                        (Some(_), None) => b,
                    }
                }
                Item::Text {
                    text,
                    x,
                    y,
                    spec,
                    anchor,
                    max_width,
                    ..
                } => {
                    let em = spec.size;
                    let mut w = text.chars().count() as f32 * em;
                    if let Some(m) = max_width {
                        w = w.min(*m);
                    }
                    let left = match anchor {
                        Anchor::Start => *x,
                        Anchor::Middle => x - w / 2.0,
                        Anchor::End => x - w,
                    };
                    Rect::new(left, y - 1.2 * em, w, 1.7 * em)
                }
                Item::Shadow { rect, blur, dy, .. } => {
                    Rect::new(rect.x, rect.y + dy, rect.w, rect.h).inflate(2.0 * blur.max(0.0))
                }
            };
            let drawn = drawn.inflate(1.0);
            let drawn = match clips.last() {
                Some(Some(c)) => c.intersect(&drawn),
                Some(None) => None,
                None => Some(drawn),
            };
            if let Some(d) = drawn {
                out = Some(out.map_or(d, |o| o.union(&d)));
            }
        }
        out
    }
}

/// Text measurement in logical px, so layout needs no fonts in tests.
pub trait Measure {
    /// How far the pen moves over `text`.
    fn width(&mut self, text: &str, spec: TextSpec) -> f32;
    /// How far the face rises above the baseline.
    fn ascent(&mut self, spec: TextSpec) -> f32;
    /// How far it drops below, positive.
    fn descent(&mut self, spec: TextSpec) -> f32;
    /// How tall its capitals and digits are.
    fn cap(&mut self, spec: TextSpec) -> f32;
    /// The longest prefix + '…' that fits `max`, or the whole text.
    fn fit(&mut self, text: &str, spec: TextSpec, max: f32) -> String;
}

/// A fixed-advance measure for tests: 0.6 × size a char (0.55 for a digit set tabular), ascent
/// 0.8, descent 0.2 and capitals 0.7 × size.
pub struct FixedMeasure;

impl FixedMeasure {
    fn advance(c: char, spec: TextSpec) -> f32 {
        if spec.tabular && c.is_ascii_digit() {
            0.55 * spec.size
        } else {
            0.6 * spec.size
        }
    }
}

impl Measure for FixedMeasure {
    fn width(&mut self, text: &str, spec: TextSpec) -> f32 {
        text.chars().map(|c| FixedMeasure::advance(c, spec)).sum()
    }

    fn ascent(&mut self, spec: TextSpec) -> f32 {
        0.8 * spec.size
    }

    fn descent(&mut self, spec: TextSpec) -> f32 {
        0.2 * spec.size
    }

    fn cap(&mut self, spec: TextSpec) -> f32 {
        0.7 * spec.size
    }

    /// The whole text if it fits; else the longest prefix that fits with '…' after it; else,
    /// when not even '…' fits, nothing.
    fn fit(&mut self, text: &str, spec: TextSpec, max: f32) -> String {
        if self.width(text, spec) <= max {
            return text.to_string();
        }
        let dots = FixedMeasure::advance('…', spec);
        if dots > max {
            return String::new();
        }
        let mut pen = dots;
        let mut end = 0;
        for (i, c) in text.char_indices() {
            pen += FixedMeasure::advance(c, spec);
            if pen > max {
                break;
            }
            end = i + c.len_utf8();
        }
        format!("{}…", &text[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: Color = Color(1, 2, 3);

    fn key(name: &str) -> SeriesKey {
        SeriesKey {
            name: name.into(),
            nth: 0,
        }
    }

    #[test]
    fn a_rect_holds_its_top_left_edges_but_not_its_bottom_right() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!((r.right(), r.bottom()), (40.0, 60.0));
        assert!(r.contains(10.0, 20.0));
        assert!(r.contains(39.9, 59.9));
        assert!(!r.contains(40.0, 30.0));
        assert!(!r.contains(20.0, 60.0));
        assert!(!r.contains(9.9, 30.0));
        assert_eq!(r.inflate(2.0), Rect::new(8.0, 18.0, 34.0, 44.0));
        assert_eq!(r.inflate(-5.0), Rect::new(15.0, 25.0, 20.0, 30.0));
        assert!(!Rect::default().contains(0.0, 0.0));
    }

    #[test]
    fn a_union_ignores_rects_with_no_area() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, -5.0, 20.0, 5.0);
        assert_eq!(a.union(&b), Rect::new(0.0, -5.0, 25.0, 15.0));
        assert_eq!(b.union(&a), a.union(&b));
        let none = Rect::new(100.0, 100.0, 0.0, 5.0);
        assert_eq!(a.union(&none), a);
        assert_eq!(none.union(&a), a);
        // They share an edge, which has no area.
        assert_eq!(a.intersect(&b), None);
        assert_eq!(
            a.intersect(&Rect::new(5.0, 5.0, 10.0, 10.0)),
            Some(Rect::new(5.0, 5.0, 5.0, 5.0))
        );
        assert_eq!(a.intersect(&Rect::new(20.0, 0.0, 5.0, 5.0)), None);
    }

    #[test]
    fn paths_from_points_rects_and_curves() {
        assert_eq!(PathData::polyline(&[]).verbs, []);
        let p = PathData::polyline(&[(0.0, 1.0), (2.0, 3.0), (4.0, 5.0)]);
        assert_eq!(
            p.verbs,
            [
                Verb::Move(0.0, 1.0),
                Verb::Line(2.0, 3.0),
                Verb::Line(4.0, 5.0)
            ]
        );
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(PathData::rect(r).bounds(), Some(r));
        assert_eq!(PathData::rect(r).verbs.last(), Some(&Verb::Close));
        // Square corners where the radius is 0, and radii clamped to half the shorter side.
        let sq = PathData::rounded(r, [0.0; 4]);
        assert!(!sq.verbs.iter().any(|v| matches!(v, Verb::Cubic(..))));
        assert_eq!(sq.bounds(), Some(r));
        let round = PathData::rounded(r, [9.0, 0.0, 1.0, 0.0]);
        let cubics = round
            .verbs
            .iter()
            .filter(|v| matches!(v, Verb::Cubic(..)))
            .count();
        assert_eq!(cubics, 2);
        assert_eq!(round.verbs[0], Verb::Move(2.5, 2.0));
        assert_eq!(round.bounds(), Some(r));
    }

    #[test]
    fn circles_and_arcs_run_clockwise_from_north() {
        let c = PathData::circle(10.0, 10.0, 5.0);
        assert_eq!(c.verbs.len(), 6);
        assert_eq!(c.verbs[0], Verb::Move(10.0, 5.0));
        let b = c.bounds().unwrap();
        assert!(
            (b.x - 5.0).abs() < 1e-4 && (b.right() - 15.0).abs() < 1e-4,
            "{b:?}"
        );
        assert!(
            (b.y - 5.0).abs() < 1e-4 && (b.bottom() - 15.0).abs() < 1e-4,
            "{b:?}"
        );
        // A quarter from north ends at east, one cubic.
        let q = PathData::arc(0.0, 0.0, 10.0, 0.0, 90.0);
        assert_eq!(q.verbs.len(), 2);
        let Verb::Cubic(c1x, c1y, _, _, x, y) = q.verbs[1] else {
            panic!("{q:?}")
        };
        assert!((x - 10.0).abs() < 1e-4 && y.abs() < 1e-4, "{q:?}");
        // Its first handle heads east from the top: clockwise on screen.
        assert!(c1x > 0.0 && (c1y + 10.0).abs() < 1e-4, "{q:?}");
        // Past north: 350 to 10 is a short arc of one cubic.
        assert_eq!(PathData::arc(0.0, 0.0, 1.0, 350.0, 10.0).verbs.len(), 2);
        // Equal angles are a whole turn.
        assert_eq!(PathData::arc(0.0, 0.0, 1.0, 45.0, 45.0).verbs.len(), 5);
    }

    #[test]
    fn the_last_region_pushed_is_hit_first() {
        let mut s = Scene::new(100.0, 50.0, 2.0);
        assert_eq!(s.hit(1.0, 1.0), None);
        s.region(Rect::new(0.0, 0.0, 100.0, 50.0), Hit::Chrome);
        s.region(Rect::new(10.0, 10.0, 80.0, 30.0), Hit::Plot(0));
        s.region(Rect::new(20.0, 0.0, 10.0, 10.0), Hit::LegendKey(key("api")));
        s.region(Rect::new(10.0, 10.0, 5.0, 5.0), Hit::Node("n1".into()));
        assert_eq!(s.hit(1.0, 1.0), Some(&Hit::Chrome));
        assert_eq!(s.hit(50.0, 20.0), Some(&Hit::Plot(0)));
        assert_eq!(s.hit(25.0, 5.0), Some(&Hit::LegendKey(key("api"))));
        assert_eq!(s.hit(12.0, 12.0), Some(&Hit::Node("n1".into())));
        assert_eq!(s.hit(15.0, 12.0), Some(&Hit::Plot(0)));
        assert_eq!(s.hit(100.0, 1.0), None);
    }

    #[test]
    fn bounds_hold_everything_drawn_within_its_clips() {
        let mut s = Scene::new(200.0, 100.0, 1.0);
        assert_eq!(s.bounds(), None);
        // Regions, clips and opacity draw nothing; nor does a path with no paint.
        s.region(Rect::new(0.0, 0.0, 200.0, 100.0), Hit::Chrome);
        s.push(Item::PushOpacity(0.5));
        s.push(Item::PopOpacity);
        s.push(Item::Path {
            path: PathData::rect(Rect::new(0.0, 0.0, 200.0, 100.0)),
            fill: None,
            stroke: None,
        });
        assert_eq!(s.bounds(), None);
        // A fill is its path's box and a px for antialiasing.
        s.push(Item::Path {
            path: PathData::rect(Rect::new(10.0, 10.0, 10.0, 10.0)),
            fill: Some(Paint::solid(INK, 1.0)),
            stroke: None,
        });
        assert_eq!(s.bounds(), Some(Rect::new(9.0, 9.0, 12.0, 12.0)));
        // A round-joined stroke reaches half its width further.
        s.items.clear();
        let stroke = Stroke {
            join: Join::Round,
            cap: Cap::Round,
            ..Stroke::solid(4.0, INK, 1.0)
        };
        s.push(Item::Path {
            path: PathData::polyline(&[(50.0, 50.0), (60.0, 50.0)]),
            fill: None,
            stroke: Some(stroke),
        });
        assert_eq!(s.bounds(), Some(Rect::new(47.0, 47.0, 16.0, 6.0)));
        // Inside a clip, only what the clip lets through; a clip inside a clip is their meet.
        s.items.clear();
        s.push(Item::PushClip(Rect::new(0.0, 0.0, 30.0, 30.0)));
        s.push(Item::PushClip(Rect::new(20.0, 0.0, 100.0, 100.0)));
        s.push(Item::Path {
            path: PathData::rect(Rect::new(0.0, 0.0, 200.0, 100.0)),
            fill: Some(Paint::solid(INK, 1.0)),
            stroke: None,
        });
        s.push(Item::PopClip);
        s.push(Item::PopClip);
        assert_eq!(s.bounds(), Some(Rect::new(20.0, 0.0, 10.0, 30.0)));
        // Clipped away entirely, nothing.
        s.items.clear();
        s.push(Item::PushClip(Rect::new(0.0, 0.0, 10.0, 10.0)));
        s.push(Item::PushClip(Rect::new(50.0, 50.0, 10.0, 10.0)));
        s.push(Item::Path {
            path: PathData::rect(Rect::new(0.0, 0.0, 200.0, 100.0)),
            fill: Some(Paint::solid(INK, 1.0)),
            stroke: None,
        });
        assert_eq!(s.bounds(), None);
    }

    #[test]
    fn bounds_of_text_and_shadows_are_generous() {
        let mut s = Scene::new(200.0, 100.0, 1.0);
        let text = |anchor, max_width| Item::Text {
            text: "abcd".into(),
            x: 100.0,
            y: 50.0,
            spec: TextSpec::new(10.0),
            anchor,
            color: INK,
            alpha: 1.0,
            max_width,
        };
        s.push(text(Anchor::End, None));
        assert_eq!(s.bounds(), Some(Rect::new(59.0, 37.0, 42.0, 19.0)));
        s.items.clear();
        s.push(text(Anchor::Middle, Some(20.0)));
        assert_eq!(s.bounds(), Some(Rect::new(89.0, 37.0, 22.0, 19.0)));
        s.items.clear();
        s.push(text(Anchor::Start, None));
        s.push(Item::Shadow {
            rect: Rect::new(10.0, 10.0, 20.0, 20.0),
            radius: 4.0,
            blur: 3.0,
            dy: 2.0,
            alpha: 0.3,
        });
        assert_eq!(s.bounds(), Some(Rect::new(3.0, 5.0, 138.0, 51.0)));
    }

    #[test]
    fn the_fixed_measure_is_six_tenths_an_em_a_char() {
        let mut m = FixedMeasure;
        let spec = TextSpec::new(10.0);
        assert_eq!(m.width("abc", spec), 18.0);
        assert_eq!(m.width("", spec), 0.0);
        assert_eq!(m.width("é日", spec), 12.0);
        // Tabular digits are narrower; other characters aren't.
        assert_eq!(m.width("10.5", spec.tabular()), 5.5 * 3.0 + 6.0);
        assert_eq!(m.width("10.5", spec), 24.0);
        assert_eq!(m.ascent(spec), 8.0);
        assert_eq!(m.descent(spec), 2.0);
        assert_eq!(m.cap(spec), 7.0);
        let bold = spec.weight(Weight::Semibold).mono();
        assert_eq!(
            (bold.weight, bold.mono, bold.size),
            (Weight::Semibold, true, 10.0)
        );
    }

    #[test]
    fn fit_cuts_with_an_ellipsis_or_keeps_the_whole() {
        let mut m = FixedMeasure;
        let spec = TextSpec::new(10.0);
        assert_eq!(m.fit("abcdef", spec, 36.0), "abcdef");
        assert_eq!(m.fit("abcdef", spec, 100.0), "abcdef");
        // 35 px: '…' and four letters is 30, five is 36.
        assert_eq!(m.fit("abcdef", spec, 35.0), "abcd…");
        assert_eq!(m.fit("abcdef", spec, 6.0), "…");
        assert_eq!(m.fit("abcdef", spec, 5.0), "");
        assert_eq!(m.fit("日本語です", spec, 20.0), "日本…");
        let fitted = m.fit("a long title", spec, 40.0);
        assert!(m.width(&fitted, spec) <= 40.0, "{fitted}");
    }
}
