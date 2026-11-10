//! The view: where the board is in the window's area. Board units are logical pixels; a physical
//! pixel in the area is a board point times the display's scale times the zoom, plus the pan,
//! which is in physical pixels. The pointer comes in physical pixels and goes through the inverse.
//! Two windows on one board share it (C10).

use crate::model::Point;

/// The least and the most the board is zoomed.
pub const ZOOM_MIN: f32 = 0.25;
/// See [`ZOOM_MIN`].
pub const ZOOM_MAX: f32 = 4.0;
/// What `+` and `-` multiply the zoom by.
pub const ZOOM_STEP: f32 = 1.25;

/// Pan and zoom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct View {
    /// Where the board's origin is in the area, in physical pixels.
    pub pan: (f32, f32),
    /// Physical pixels per logical pixel on top of the display's scale: 1 at first.
    pub zoom: f32,
}

impl Default for View {
    fn default() -> View {
        View {
            pan: (0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl View {
    /// Physical pixels per board unit at the display's `scale` (0, unknown, counts as 1).
    pub fn factor(&self, scale: f64) -> f32 {
        let scale = if scale > 0.0 { scale as f32 } else { 1.0 };
        scale * self.zoom
    }
    /// The board point under a physical pixel of the area.
    pub fn to_board(&self, x: f32, y: f32, scale: f64) -> Point {
        let f = self.factor(scale);
        Point::new((x - self.pan.0) / f, (y - self.pan.1) / f)
    }
    /// The physical pixel of the area a board point is at.
    pub fn to_physical(&self, p: Point, scale: f64) -> (f32, f32) {
        let f = self.factor(scale);
        (p.x * f + self.pan.0, p.y * f + self.pan.1)
    }
    /// Pan by physical pixels.
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan.0 += dx;
        self.pan.1 += dy;
    }
    /// Multiply the zoom by `by`, within its limits, keeping the board point under the physical
    /// pixel `about` where it is.
    pub fn zoom_by(&mut self, by: f32, about: (f32, f32), scale: f64) {
        let before = self.to_board(about.0, about.1, scale);
        self.zoom = (self.zoom * by).clamp(ZOOM_MIN, ZOOM_MAX);
        let (x, y) = self.to_physical(before, scale);
        self.pan.0 += about.0 - x;
        self.pan.1 += about.1 - y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_maps_through_pan_zoom_and_scale() {
        let mut v = View::default();
        v.pan_by(20.0, 10.0);
        assert_eq!(v.to_board(60.0, 50.0, 2.0), Point::new(20.0, 20.0));
        assert_eq!(v.to_physical(Point::new(20.0, 20.0), 2.0), (60.0, 50.0));
        v.zoom_by(2.0, (60.0, 50.0), 2.0);
        assert_eq!(v.zoom, 2.0);
        assert_eq!(v.to_board(60.0, 50.0, 2.0), Point::new(20.0, 20.0), "held");
        v.zoom_by(100.0, (0.0, 0.0), 2.0);
        assert_eq!(v.zoom, ZOOM_MAX);
        assert_eq!(View::default().factor(0.0), 1.0, "an unknown scale is 1");
    }
}
