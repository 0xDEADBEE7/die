//! What drawing leaves behind: a hit for each mark (a bar, a point, a cell, a tile, a node),
//! and the one card any of them shows, the same for every kind of panel.

use crate::canvas::{Align, Canvas, Ink, Pen, Rect, Theme};
use die_draw::Rgb;

/// How far from a point the pointer still finds it, in logical px.
const REACH: f32 = 24.0;

/// Where a hit is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Rect(Rect),
    Point(f32, f32),
}

/// A mark the pointer or the keys can be on, and what its card says.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub shape: Shape,
    /// Its place in key order: its panel, its series (a horizontal bar's row, a tree's level),
    /// and its place along that.
    pub key: (usize, usize, usize),
    /// The card's heading (may be empty), its key's colour, and its rows of a name and a value.
    pub title: String,
    pub color: Rgb,
    pub rows: Vec<(String, String)>,
}

impl Hit {
    /// Where its card points from.
    pub fn anchor(&self) -> (f32, f32) {
        match self.shape {
            Shape::Rect(r) => (r.x + r.w / 2.0, r.y.min(r.bottom())),
            Shape::Point(x, y) => (x, y),
        }
    }
}

/// The hit under a place: a rectangle holding it, else the nearest point in reach.
pub fn at(hits: &[Hit], x: f32, y: f32) -> Option<usize> {
    let held = hits.iter().position(|h| match h.shape {
        Shape::Rect(r) => r.contains(x, y),
        Shape::Point(..) => false,
    });
    let dist = |h: &Hit| match h.shape {
        Shape::Point(px, py) => (px - x).hypot(py - y),
        Shape::Rect(_) => f32::INFINITY,
    };
    let near = (0..hits.len()).min_by(|&a, &b| dist(&hits[a]).total_cmp(&dist(&hits[b])));
    held.or(near.filter(|&i| dist(&hits[i]) <= REACH))
}

/// Which way a key moves the cursor: `h`, `l`, `k`, `j`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Where a key takes the cursor from hit `from` among `panel`'s hits (in key order): left and
/// right along its series, up and down to the series before or after it (a horizontal bar's
/// row, a tree's level) at the hit nearest across. With no cursor in the panel, its first hit,
/// or its last going left or up. At an end it stays.
pub fn step(hits: &[Hit], panel: usize, from: Option<usize>, dir: Dir) -> Option<usize> {
    let mine: Vec<usize> = (0..hits.len())
        .filter(|&i| hits[i].key.0 == panel)
        .collect();
    let back = matches!(dir, Dir::Left | Dir::Up);
    let Some(from) = from.filter(|i| mine.contains(i)) else {
        return if back { mine.last() } else { mine.first() }.copied();
    };
    let series = hits[from].key.1;
    if matches!(dir, Dir::Left | Dir::Right) {
        let along: Vec<usize> = mine
            .into_iter()
            .filter(|&i| hits[i].key.1 == series)
            .collect();
        let at = along.iter().position(|&i| i == from)?;
        let to = if back {
            at.checked_sub(1)
        } else {
            Some(at + 1)
        };
        return Some(to.and_then(|t| along.get(t).copied()).unwrap_or(from));
    }
    let others = mine.iter().map(|&i| hits[i].key.1);
    let next = match back {
        true => others.filter(|&s| s < series).max(),
        false => others.filter(|&s| s > series).min(),
    };
    let Some(next) = next else {
        return Some(from);
    };
    let x = hits[from].anchor().0;
    let far = |i: &usize| (hits[*i].anchor().0 - x).abs();
    let there = mine.into_iter().filter(|&i| hits[i].key.1 == next);
    there.min_by(|a, b| far(a).total_cmp(&far(b)))
}

/// What a hit's card says, in a line: its heading, then each row's name and value.
pub fn words(hit: &Hit) -> String {
    let rows = hit.rows.iter().map(|(n, v)| format!("{n} {v}"));
    let head = Some(hit.title.clone()).filter(|t| !t.is_empty());
    head.into_iter().chain(rows).collect::<Vec<_>>().join(", ")
}

/// The hit marked, and its card beside it, kept inside `bounds`.
pub fn card(c: &mut Canvas, th: &Theme, hit: &Hit, bounds: Rect) {
    match hit.shape {
        Shape::Rect(r) => c.outline(r, 2.0, Pen::new(1.5, th.strong)),
        Shape::Point(x, y) => {
            c.dot((x, y), 6.5, th.surface);
            c.dot((x, y), 5.0, hit.color);
        }
    }
    let (name, value, head) = (Ink::new(12.0, th.muted), Ink::new(12.0, th.strong), 13.0);
    let head = Ink::new(head, th.strong).bold();
    let mut w: f32 = 0.0;
    if !hit.title.is_empty() {
        w = c.width(&hit.title, head) + 18.0;
    }
    for (n, v) in &hit.rows {
        w = w.max(c.width(n, name) + c.width(v, value) + 20.0);
    }
    let w = (w + 24.0).min(bounds.w * 0.9).min(420.0);
    let lines = hit.rows.len() + usize::from(!hit.title.is_empty());
    let h = lines as f32 * 20.0 + 16.0;
    let (ax, ay) = hit.anchor();
    let mut x = ax + 14.0;
    if x + w > bounds.right() - 4.0 {
        x = ax - 14.0 - w;
    }
    let mut y = ay - h - 10.0;
    if y < bounds.y + 4.0 {
        y = ay + 16.0;
    }
    let x = x.clamp(bounds.x + 4.0, (bounds.right() - w - 4.0).max(bounds.x));
    let y = y.clamp(bounds.y + 4.0, (bounds.bottom() - h - 4.0).max(bounds.y));
    let r = Rect::new(x, y, w, h);
    c.fill(r, 8.0, th.card, 1.0);
    c.outline(r, 8.0, Pen::new(1.0, th.ring));
    let (mut base, left, right) = (y + 22.0, x + 12.0, x + w - 12.0);
    if !hit.title.is_empty() {
        c.dot((left + 4.0, base - 4.5), 4.0, hit.color);
        c.text(
            &hit.title,
            head,
            (left + 14.0, base),
            Align::Start,
            w - 38.0,
        );
        base += 20.0;
    }
    for (n, v) in &hit.rows {
        let nw = c.width(n, name).min(w * 0.45);
        let vw = c.text(v, value, (right, base), Align::End, w - nw - 36.0);
        c.text(n, name, (left, base), Align::Start, w - vw - 36.0);
        base += 20.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(shape: Shape) -> Hit {
        let (title, rows) = (String::new(), Vec::new());
        let color = Rgb(0, 0, 0);
        Hit {
            shape,
            key: (0, 0, 0),
            title,
            color,
            rows,
        }
    }

    #[test]
    fn the_pointer_finds_a_rectangle_or_the_nearest_point_in_reach() {
        let hits = [
            hit(Shape::Point(10.0, 10.0)),
            hit(Shape::Point(40.0, 10.0)),
            hit(Shape::Rect(Rect::new(100.0, 0.0, 20.0, 20.0))),
        ];
        assert_eq!(at(&hits, 30.0, 12.0), Some(1));
        assert_eq!(at(&hits, 110.0, 5.0), Some(2));
        assert_eq!(at(&hits, 70.0, 80.0), None);
    }

    #[test]
    fn keys_move_along_a_series_and_across_to_the_nearest_of_the_next() {
        let at = |key, x| Hit {
            key,
            ..hit(Shape::Point(x, 0.0))
        };
        let hits = [
            at((0, 0, 0), 0.0),
            at((0, 0, 1), 10.0),
            at((1, 0, 0), 0.0),
            at((1, 0, 1), 10.0),
            at((1, 0, 2), 20.0),
            at((1, 2, 0), 18.0),
            at((1, 2, 1), 30.0),
        ];
        assert_eq!(step(&hits, 1, None, Dir::Right), Some(2));
        assert_eq!(
            step(&hits, 1, Some(0), Dir::Left),
            Some(6),
            "another panel's"
        );
        assert_eq!(step(&hits, 1, Some(2), Dir::Right), Some(3));
        assert_eq!(
            step(&hits, 1, Some(2), Dir::Left),
            Some(2),
            "it stays at an end"
        );
        assert_eq!(
            step(&hits, 1, Some(4), Dir::Down),
            Some(5),
            "the nearest across"
        );
        assert_eq!(step(&hits, 1, Some(5), Dir::Up), Some(4));
        assert_eq!(step(&hits, 1, Some(6), Dir::Down), Some(6));
        assert_eq!(step(&hits, 2, None, Dir::Right), None);
        let card = Hit {
            title: "a".into(),
            rows: vec![("x".into(), "1".into()), ("y".into(), "2".into())],
            ..hit(Shape::Point(0.0, 0.0))
        };
        assert_eq!(words(&card), "a, x 1, y 2");
    }
}
