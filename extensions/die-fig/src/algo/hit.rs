//! The hit index: what the pointer is nearest. Nearest x by binary search over each visible
//! series' raw samples; points and nodes in a grid hash with cells of `HIT_DENSE`; edges by
//! segment distance.
//!
//! The plot builds one with its base layer and the surface asks it after the scene's regions
//! (ruling 5). Everything is deterministic: the grid is a `BTreeMap`, and a tie between two
//! points or two segments at the same distance goes to the one added last, which draws on top.

use crate::data::SeriesKey;
use crate::interact::consts::HIT_DENSE;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A series' samples: x and y.
type Samples = (Arc<[f64]>, Arc<[f64]>);

/// What's where, for the pointer: series by x, points and segments by screen position.
#[derive(Default)]
pub struct HitIndex {
    series: BTreeMap<SeriesKey, Samples>,
    /// The keys points name, each once.
    keys: Vec<SeriesKey>,
    key_of: BTreeMap<SeriesKey, u32>,
    points: Vec<Point>,
    /// Each cell's points, in the order added.
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    segments: Vec<Segment>,
}

struct Point {
    key: u32,
    row: u32,
    x: f32,
    y: f32,
}

struct Segment {
    id: u32,
    a: (f32, f32),
    b: (f32, f32),
}

impl HitIndex {
    /// An empty index.
    pub fn new() -> HitIndex {
        HitIndex::default()
    }

    /// A series' samples, sorted by x (ascending; a NaN x, if any, only at the end). A NaN y is
    /// a gap, which [`HitIndex::nearest_x`] steps over. Adding a key again replaces its samples.
    pub fn add_series(&mut self, key: SeriesKey, xs: Arc<[f64]>, ys: Arc<[f64]>) {
        self.series.insert(key, (xs, ys));
    }

    /// A point at a screen position (logical px). One that isn't finite can't be hit.
    pub fn add_point(&mut self, key: SeriesKey, row: u32, x: f32, y: f32) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        let key = match self.key_of.get(&key) {
            Some(&k) => k,
            None => {
                let k = self.keys.len() as u32;
                self.key_of.insert(key.clone(), k);
                self.keys.push(key);
                k
            }
        };
        let i = self.points.len() as u32;
        self.points.push(Point { key, row, x, y });
        self.grid.entry(cell(x, y)).or_default().push(i);
    }

    /// A segment between screen positions: an edge. One that isn't finite can't be hit.
    pub fn add_segment(&mut self, id: u32, a: (f32, f32), b: (f32, f32)) {
        if [a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
            self.segments.push(Segment { id, a, b });
        }
    }

    /// The row of a series' sample nearest x, in data: its index in the samples given to
    /// [`HitIndex::add_series`]. Missing samples (NaN y) are passed over; on a tie the earlier x
    /// wins, and of samples at the same x the first. None for a key it doesn't have, a series
    /// with no sample present, or a NaN x.
    pub fn nearest_x(&self, key: &SeriesKey, x: f64) -> Option<u32> {
        let (xs, ys) = self.series.get(key)?;
        if x.is_nan() {
            return None;
        }
        let len = xs.len().min(ys.len());
        let xs = &xs[..xs[..len].partition_point(|v| !v.is_nan())];
        let present = |i: &usize| !ys[*i].is_nan();
        let p = xs.partition_point(|&v| v < x);
        let right = (p..xs.len()).find(present);
        let left = (0..p).rev().find(present);
        let pick = match (left, right) {
            (Some(l), Some(r)) => {
                if x - xs[l] <= xs[r] - x {
                    l
                } else {
                    r
                }
            }
            (l, r) => l.or(r)?,
        };
        let first = xs.partition_point(|&v| v < xs[pick]);
        (first..=pick).find(present).map(|i| i as u32)
    }

    /// The point nearest a screen position, within `reach` (inclusive).
    pub fn nearest_point(&self, x: f32, y: f32, reach: f32) -> Option<(SeriesKey, u32)> {
        if !(x.is_finite() && y.is_finite() && reach >= 0.0) {
            return None;
        }
        let mut best: Option<(f64, u32)> = None;
        let mut consider = |i: u32| {
            let p = &self.points[i as usize];
            let d = dist2((x, y), (p.x, p.y));
            if d <= f64::from(reach) * f64::from(reach)
                && best.is_none_or(|(bd, bi)| d < bd || (d == bd && i > bi))
            {
                best = Some((d, i));
            }
        };
        let (lo, hi) = (cell(x - reach, y - reach), cell(x + reach, y + reach));
        let columns = i64::from(hi.0) - i64::from(lo.0) + 1;
        if !reach.is_finite() || columns > self.grid.len() as i64 {
            (0..self.points.len() as u32).for_each(consider);
        } else {
            for cx in lo.0..=hi.0 {
                for (_, ids) in self.grid.range((cx, lo.1)..=(cx, hi.1)) {
                    ids.iter().copied().for_each(&mut consider);
                }
            }
        }
        let (_, i) = best?;
        let p = &self.points[i as usize];
        Some((self.keys[p.key as usize].clone(), p.row))
    }

    /// The segment nearest a screen position, within `reach` (inclusive).
    pub fn nearest_segment(&self, x: f32, y: f32, reach: f32) -> Option<u32> {
        if !(x.is_finite() && y.is_finite() && reach >= 0.0) {
            return None;
        }
        let mut best: Option<(f32, u32)> = None;
        for s in &self.segments {
            let (x0, x1) = (s.a.0.min(s.b.0), s.a.0.max(s.b.0));
            let (y0, y1) = (s.a.1.min(s.b.1), s.a.1.max(s.b.1));
            if x < x0 - reach || x > x1 + reach || y < y0 - reach || y > y1 + reach {
                continue;
            }
            let d = segment_distance((x, y), s.a, s.b);
            if d <= reach && best.is_none_or(|(bd, _)| d <= bd) {
                best = Some((d, s.id));
            }
        }
        best.map(|(_, id)| id)
    }
}

/// The distance from `p` to the segment from `a` to `b` (to `a` when they're the same point).
pub fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = (f64::from(p.0), f64::from(p.1));
    let (ax, ay) = (f64::from(a.0), f64::from(a.1));
    let (dx, dy) = (f64::from(b.0) - ax, f64::from(b.1) - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((px - (ax + t * dx)).hypot(py - (ay + t * dy))) as f32
}

/// The grid cell a screen position is in.
fn cell(x: f32, y: f32) -> (i32, i32) {
    (
        (x / HIT_DENSE).floor() as i32,
        (y / HIT_DENSE).floor() as i32,
    )
}

fn dist2(a: (f32, f32), b: (f32, f32)) -> f64 {
    let (dx, dy) = (
        f64::from(a.0) - f64::from(b.0),
        f64::from(a.1) - f64::from(b.1),
    );
    dx * dx + dy * dy
}
