//! The pen's ink: a streamline low-pass over the pointer's samples as they come, and
//! Douglas-Peucker simplification once the stroke ends. Width is constant per stroke: there's no
//! pressure to read (winit has no tablet events on macOS).

use crate::model::{Point, dist_to_segment};

/// How much of each sample's movement is held back: perfect-freehand's `streamline`.
pub const STREAMLINE: f32 = 0.5;
/// How far a point may be from the simplified stroke, in logical pixels.
pub const SIMPLIFY: f32 = 0.5;

/// The low-pass: each point moves from the last by half the way to the sample.
#[derive(Clone, Debug, Default)]
pub struct Streamline {
    last: Option<Point>,
}

impl Streamline {
    /// The smoothed point for this sample. The first is the sample itself.
    pub fn push(&mut self, raw: Point) -> Point {
        let p = match self.last {
            None => raw,
            Some(last) => last + (raw - last) * (1.0 - STREAMLINE),
        };
        self.last = Some(p);
        p
    }
}

/// The points of a polyline within `eps` of the original, the ends kept (Douglas-Peucker).
pub fn simplify(points: &[Point], eps: f32) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((lo, hi)) = stack.pop() {
        if hi <= lo + 1 {
            continue;
        }
        let (mut worst, mut at) = (0.0f32, lo);
        for (i, p) in points.iter().enumerate().take(hi).skip(lo + 1) {
            let d = dist_to_segment(*p, points[lo], points[hi]);
            if d > worst {
                (worst, at) = (d, i);
            }
        }
        if worst > eps {
            keep[at] = true;
            stack.push((lo, at));
            stack.push((at, hi));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_low_pass_holds_half_of_each_move_back() {
        let mut s = Streamline::default();
        assert_eq!(s.push(Point::new(0.0, 0.0)), Point::new(0.0, 0.0));
        assert_eq!(s.push(Point::new(10.0, 0.0)), Point::new(5.0, 0.0));
        assert_eq!(s.push(Point::new(10.0, 0.0)), Point::new(7.5, 0.0));
    }

    #[test]
    fn a_straight_run_simplifies_to_its_ends_and_a_corner_stays() {
        let line: Vec<Point> = (0..20).map(|i| Point::new(i as f32, 0.0)).collect();
        assert_eq!(simplify(&line, 0.5), [line[0], line[19]]);
        let mut corner = line.clone();
        corner.extend((1..10).map(|i| Point::new(19.0, i as f32)));
        let got = simplify(&corner, 0.5);
        assert_eq!(got, [line[0], line[19], Point::new(19.0, 9.0)]);
        let wobble = [
            Point::new(0.0, 0.0),
            Point::new(5.0, 0.4),
            Point::new(10.0, 0.0),
        ];
        assert_eq!(simplify(&wobble, 0.5).len(), 2, "under the tolerance");
    }
}
