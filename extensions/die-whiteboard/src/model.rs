//! The board: its elements in board units, which are logical pixels and the SVG's user units,
//! what each covers, what the pointer hits, and how a line or an arrow binds to a shape. Nothing
//! here knows about cells, the view or the file.

use std::ops::{Add, Mul, Sub};

/// An element's id, unique on its board for the board's life, and kept in the file.
pub type Id = u32;

/// The id a gesture's live element has before it's added to the board.
pub const LIVE: Id = Id::MAX;

/// A point in board units.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Point {
    /// Rightward.
    pub x: f32,
    /// Downward.
    pub y: f32,
}

impl Point {
    /// The point at (x, y).
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    /// The distance to `o`.
    pub fn dist(self, o: Point) -> f32 {
        (self.x - o.x).hypot(self.y - o.y)
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Point {
    type Output = Point;
    fn mul(self, k: f32) -> Point {
        Point::new(self.x * k, self.y * k)
    }
}

/// An axis-aligned rectangle in board units, `min` at its top left.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    /// The top left.
    pub min: Point,
    /// The bottom right.
    pub max: Point,
}

impl Rect {
    /// The rectangle with `a` and `b` as opposite corners, whichever way round.
    pub fn from_points(a: Point, b: Point) -> Rect {
        Rect {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }
    /// Its width.
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }
    /// Its height.
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
    /// Its centre.
    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }
    /// This rectangle grown by `by` on every side.
    pub fn inflate(&self, by: f32) -> Rect {
        Rect {
            min: Point::new(self.min.x - by, self.min.y - by),
            max: Point::new(self.max.x + by, self.max.y + by),
        }
    }
    /// Whether `p` is inside it, edges included.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    /// Whether `r` lies wholly inside it.
    pub fn contains_rect(&self, r: &Rect) -> bool {
        self.contains(r.min) && self.contains(r.max)
    }
    /// The smallest rectangle holding both.
    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            min: Point::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: Point::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }
}

/// A cell's size in board units: what one of a label's characters takes on screen, so a label's
/// extent is known without a font. The default stands in until the host says what a cell is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CellSize {
    /// A cell's width.
    pub w: f32,
    /// A cell's height.
    pub h: f32,
}

impl Default for CellSize {
    fn default() -> CellSize {
        CellSize { w: 10.0, h: 20.0 }
    }
}

/// The three widths `[` and `]` step through, in logical pixels.
pub const WIDTHS: [f32; 3] = [2.0, 4.0, 8.0];
/// Their names, for the status line.
pub const WIDTH_NAMES: [&str; 3] = ["thin", "medium", "thick"];
/// The highlighter's width.
pub const HIGHLIGHTER_WIDTH: f32 = 16.0;
/// The highlighter's opacity.
pub const HIGHLIGHTER_ALPHA: f32 = 0.35;
/// How many inks there are: `1`..`6`.
pub const INKS: u8 = 6;
/// How near a line's end must be to a shape to bind to it.
pub const BIND_WITHIN: f32 = 12.0;
/// How far from an element's edge a hit still counts.
pub const HIT_SLACK: f32 = 4.0;

/// What an element is.
#[derive(Clone, PartialEq, Debug)]
pub enum Shape {
    /// Freehand ink: its points after smoothing and simplification, drawn with round caps and
    /// joins at the element's width, or at the highlighter's width and opacity.
    Stroke {
        /// The points, at least one.
        points: Vec<Point>,
        /// Drawn as the highlighter draws.
        highlighter: bool,
    },
    /// A rectangle between two corners.
    Rect {
        /// One corner.
        a: Point,
        /// The opposite corner.
        b: Point,
    },
    /// An ellipse filling the box between two corners.
    Ellipse {
        /// One corner.
        a: Point,
        /// The opposite corner.
        b: Point,
    },
    /// A straight line, with an arrowhead at `b` if it's an arrow, each end bound to a shape it
    /// was drawn against, so it follows when the shape moves.
    Line {
        /// Its start.
        a: Point,
        /// Its end, where the arrowhead is.
        b: Point,
        /// An arrow rather than a plain line.
        arrow: bool,
        /// The shape `a` is bound to.
        from: Option<Id>,
        /// The shape `b` is bound to.
        to: Option<Id>,
    },
    /// Text at a point: its top left, one line per `\n`.
    Label {
        /// Its top left.
        at: Point,
        /// Its text.
        text: String,
    },
}

/// One element of a board.
#[derive(Clone, PartialEq, Debug)]
pub struct Element {
    /// Its id.
    pub id: Id,
    /// Its ink, `1`..=[`INKS`].
    pub ink: u8,
    /// Its width, an index into [`WIDTHS`].
    pub width: u8,
    /// What it is.
    pub shape: Shape,
}

/// The lines of a label's text: one at least, so an empty label has a place.
pub fn label_lines(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() { vec![""] } else { lines }
}

/// A label's extent in board units: its longest line by its lines, a cell at least.
pub fn label_size(text: &str, cell: CellSize) -> (f32, f32) {
    let lines = label_lines(text);
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let cols = longest.max(1) as f32;
    (cols * cell.w, lines.len() as f32 * cell.h)
}

impl Element {
    /// The width it's drawn at.
    pub fn stroke_width(&self) -> f32 {
        match &self.shape {
            Shape::Stroke {
                highlighter: true, ..
            } => HIGHLIGHTER_WIDTH,
            _ => WIDTHS[usize::from(self.width).min(WIDTHS.len() - 1)],
        }
    }

    /// What it covers, its width included.
    pub fn bounds(&self, cell: CellSize) -> Rect {
        let half = self.stroke_width() / 2.0;
        match &self.shape {
            Shape::Stroke { points, .. } => {
                let first = points.first().copied().unwrap_or_default();
                let r = points.iter().fold(Rect::from_points(first, first), |r, p| {
                    r.union(&Rect::from_points(*p, *p))
                });
                r.inflate(half)
            }
            Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
                Rect::from_points(*a, *b).inflate(half)
            }
            Shape::Line { a, b, .. } => Rect::from_points(*a, *b).inflate(half.max(1.0)),
            Shape::Label { at, text } => {
                let (w, h) = label_size(text, cell);
                Rect::from_points(*at, Point::new(at.x + w, at.y + h))
            }
        }
    }

    /// Whether `p` is on it, or within [`HIT_SLACK`] of it: on a stroke's or a line's path, or
    /// inside a shape's or a label's box, which is what a finger means by a box.
    pub fn hits(&self, p: Point, cell: CellSize) -> bool {
        let tol = HIT_SLACK + self.stroke_width() / 2.0;
        match &self.shape {
            Shape::Stroke { points, .. } => match points.as_slice() {
                [] => false,
                [only] => only.dist(p) <= tol,
                _ => points
                    .windows(2)
                    .any(|w| dist_to_segment(p, w[0], w[1]) <= tol),
            },
            Shape::Line { a, b, .. } => dist_to_segment(p, *a, *b) <= tol,
            Shape::Rect { .. } | Shape::Ellipse { .. } | Shape::Label { .. } => {
                self.bounds(cell).inflate(HIT_SLACK).contains(p)
            }
        }
    }

    /// Move it by `d`.
    pub fn translate(&mut self, d: Point) {
        match &mut self.shape {
            Shape::Stroke { points, .. } => points.iter_mut().for_each(|p| *p = *p + d),
            Shape::Rect { a, b } | Shape::Ellipse { a, b } | Shape::Line { a, b, .. } => {
                *a = *a + d;
                *b = *b + d;
            }
            Shape::Label { at, .. } => *at = *at + d,
        }
    }

    /// Move the ends bound to `shape` by `d`, as a line follows a shape it was drawn against.
    pub fn follow(&mut self, shape: Id, d: Point) {
        if let Shape::Line { a, b, from, to, .. } = &mut self.shape {
            if *from == Some(shape) {
                *a = *a + d;
            }
            if *to == Some(shape) {
                *b = *b + d;
            }
        }
    }

    /// Whether either end is bound to `shape`.
    pub fn bound_to(&self, shape: Id) -> bool {
        matches!(&self.shape, Shape::Line { from, to, .. } if *from == Some(shape) || *to == Some(shape))
    }

    /// Forget a binding to `shape`, which is gone.
    pub fn unbind(&mut self, shape: Id) {
        if let Shape::Line { from, to, .. } = &mut self.shape {
            if *from == Some(shape) {
                *from = None;
            }
            if *to == Some(shape) {
                *to = None;
            }
        }
    }

    /// Whether it's a shape a line can bind to.
    pub fn bindable(&self) -> bool {
        matches!(self.shape, Shape::Rect { .. } | Shape::Ellipse { .. })
    }
}

/// The distance from `p` to the segment `a`-`b`.
pub fn dist_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

/// The point of a rectangle's outline nearest `p`.
pub fn nearest_on_rect(p: Point, r: &Rect) -> Point {
    let c = Point::new(p.x.clamp(r.min.x, r.max.x), p.y.clamp(r.min.y, r.max.y));
    let edges = [
        (c.x - r.min.x, Point::new(r.min.x, c.y)),
        (r.max.x - c.x, Point::new(r.max.x, c.y)),
        (c.y - r.min.y, Point::new(c.x, r.min.y)),
        (r.max.y - c.y, Point::new(c.x, r.max.y)),
    ];
    edges
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(c, |e| e.1)
}

/// The point of an ellipse's outline in the direction of `p` from its centre.
pub fn nearest_on_ellipse(p: Point, r: &Rect) -> Point {
    let c = r.center();
    let (rx, ry) = ((r.width() / 2.0).max(0.01), (r.height() / 2.0).max(0.01));
    let (nx, ny) = ((p.x - c.x) / rx, (p.y - c.y) / ry);
    if nx.abs() < f32::EPSILON && ny.abs() < f32::EPSILON {
        return Point::new(c.x, c.y - ry);
    }
    let t = ny.atan2(nx);
    Point::new(c.x + rx * t.cos(), c.y + ry * t.sin())
}

/// A board: its elements, oldest first, which is the order they're drawn in.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Board {
    /// The elements, oldest first.
    pub elements: Vec<Element>,
    /// The id the next element gets.
    pub next_id: Id,
}

impl Board {
    /// A fresh id, never given on this board before.
    pub fn fresh_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }
    /// Add an element on top, with a fresh id. Returns its index and id.
    pub fn add(&mut self, shape: Shape, ink: u8, width: u8) -> (usize, Id) {
        let id = self.fresh_id();
        self.elements.push(Element {
            id,
            ink,
            width,
            shape,
        });
        (self.elements.len() - 1, id)
    }
    /// Where the element with `id` is.
    pub fn index_of(&self, id: Id) -> Option<usize> {
        self.elements.iter().position(|e| e.id == id)
    }
    /// The element with `id`.
    pub fn get(&self, id: Id) -> Option<&Element> {
        self.elements.iter().find(|e| e.id == id)
    }
    /// The element with `id`, to change.
    pub fn get_mut(&mut self, id: Id) -> Option<&mut Element> {
        self.elements.iter_mut().find(|e| e.id == id)
    }
    /// Take the element with `id` out, with where it was. Lines bound to it forget it.
    pub fn remove(&mut self, id: Id) -> Option<(usize, Element)> {
        let i = self.index_of(id)?;
        let el = self.elements.remove(i);
        if el.bindable() {
            self.elements.iter_mut().for_each(|e| e.unbind(id));
        }
        Some((i, el))
    }
    /// Put an element back at `index`, or on top if that's past the end. Its id must be one
    /// this board gave.
    pub fn insert(&mut self, index: usize, el: Element) {
        self.next_id = self.next_id.max(el.id.saturating_add(1));
        let at = index.min(self.elements.len());
        self.elements.insert(at, el);
    }
    /// Whether there's nothing on it.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
    /// How many elements it has.
    pub fn len(&self) -> usize {
        self.elements.len()
    }
    /// What everything covers, or None for an empty board.
    pub fn bounds(&self, cell: CellSize) -> Option<Rect> {
        let mut all = self.elements.iter().map(|e| e.bounds(cell));
        let first = all.next()?;
        Some(all.fold(first, |r, b| r.union(&b)))
    }
    /// The topmost element under `p`.
    pub fn hit(&self, p: Point, cell: CellSize) -> Option<Id> {
        self.elements
            .iter()
            .rev()
            .find(|e| e.hits(p, cell))
            .map(|e| e.id)
    }
    /// The elements lying wholly inside `r`, oldest first.
    pub fn within(&self, r: &Rect, cell: CellSize) -> Vec<Id> {
        self.elements
            .iter()
            .filter(|e| r.contains_rect(&e.bounds(cell)))
            .map(|e| e.id)
            .collect()
    }
    /// Bind each end of a line to the topmost shape within [`BIND_WITHIN`] of it, if there's
    /// one, and put the end on that shape's outline.
    pub fn bind(&self, el: &mut Element) {
        let Shape::Line { a, b, from, to, .. } = &mut el.shape else {
            return;
        };
        for (end, bound) in [(a, from), (b, to)] {
            let near = self.elements.iter().rev().find(|e| {
                e.bindable()
                    && e.id != el.id
                    && e.bounds(CellSize::default())
                        .inflate(BIND_WITHIN)
                        .contains(*end)
            });
            let Some(shape) = near else {
                continue;
            };
            let box_ = match &shape.shape {
                Shape::Rect { a, b } | Shape::Ellipse { a, b } => Rect::from_points(*a, *b),
                _ => continue,
            };
            *end = match shape.shape {
                Shape::Ellipse { .. } => nearest_on_ellipse(*end, &box_),
                _ => nearest_on_rect(*end, &box_),
            };
            *bound = Some(shape.id);
        }
    }
    /// The lines bound to any of `ids` that aren't among them: what follows when they move.
    pub fn followers(&self, ids: &[Id]) -> Vec<Id> {
        self.elements
            .iter()
            .filter(|e| !ids.contains(&e.id) && ids.iter().any(|id| e.bound_to(*id)))
            .map(|e| e.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: &[(f32, f32)]) -> Element {
        Element {
            id: 1,
            ink: 1,
            width: 1,
            shape: Shape::Stroke {
                points: points.iter().map(|&(x, y)| Point::new(x, y)).collect(),
                highlighter: false,
            },
        }
    }

    #[test]
    fn hits_are_within_slack_of_the_path_or_inside_a_box() {
        let s = stroke(&[(0.0, 0.0), (100.0, 0.0)]);
        let cell = CellSize::default();
        assert!(s.hits(Point::new(50.0, 5.0), cell));
        assert!(!s.hits(Point::new(50.0, 7.0), cell), "4 + 2 of slack");
        let r = Element {
            id: 2,
            ink: 1,
            width: 0,
            shape: Shape::Rect {
                a: Point::new(10.0, 10.0),
                b: Point::new(50.0, 30.0),
            },
        };
        assert!(r.hits(Point::new(30.0, 20.0), cell), "inside counts");
        assert!(!r.hits(Point::new(60.0, 20.0), cell));
        let l = Element {
            id: 3,
            ink: 1,
            width: 0,
            shape: Shape::Label {
                at: Point::new(0.0, 0.0),
                text: "ab\ncde".into(),
            },
        };
        assert_eq!(
            l.bounds(cell),
            Rect::from_points(Point::new(0.0, 0.0), Point::new(30.0, 40.0))
        );
    }

    #[test]
    fn a_line_binds_to_a_nearby_shape_and_follows_it() {
        let mut board = Board::default();
        let (_, rect) = board.add(
            Shape::Rect {
                a: Point::new(0.0, 0.0),
                b: Point::new(40.0, 40.0),
            },
            1,
            1,
        );
        let mut line = Element {
            id: LIVE,
            ink: 1,
            width: 1,
            shape: Shape::Line {
                a: Point::new(48.0, 20.0),
                b: Point::new(200.0, 20.0),
                arrow: true,
                from: None,
                to: None,
            },
        };
        board.bind(&mut line);
        let Shape::Line { a, from, to, .. } = &line.shape else {
            panic!()
        };
        assert_eq!((*a, *from, *to), (Point::new(40.0, 20.0), Some(rect), None));
        let (_, id) = board.add(line.shape.clone(), 1, 1);
        assert_eq!(board.followers(&[rect]), [id]);
        board
            .get_mut(id)
            .unwrap()
            .follow(rect, Point::new(5.0, 5.0));
        let Shape::Line { a, b, .. } = board.get(id).unwrap().shape else {
            panic!()
        };
        assert_eq!((a, b), (Point::new(45.0, 25.0), Point::new(200.0, 20.0)));
        board.remove(rect);
        assert!(
            !board.get(id).unwrap().bound_to(rect),
            "a gone shape unbinds"
        );
    }

    #[test]
    fn removed_elements_go_back_where_they_were() {
        let mut board = Board::default();
        for i in 0..3 {
            board.add(
                Shape::Label {
                    at: Point::new(i as f32, 0.0),
                    text: i.to_string(),
                },
                1,
                1,
            );
        }
        let (i, el) = board.remove(2).unwrap();
        assert_eq!(i, 1);
        board.insert(i, el);
        let ids: Vec<Id> = board.elements.iter().map(|e| e.id).collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(board.fresh_id(), 4);
    }
}
