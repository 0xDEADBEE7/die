//! The tool dial: the eight tools around the pointer, one picked by direction. Space held, or
//! the right button held, opens it where the pointer is; moving past the dead zone hovers the
//! sector in that direction; letting go picks it, or in the dead zone picks nothing. Eight
//! sectors of 45°, north first then clockwise: pen, rect, ellipse, arrow, line, label, eraser,
//! select (the highlighter stays on `h`: it's a pen mode). Its geometry is in board units, as
//! the marquee's is, sized from the cell at the view's zoom so on screen it's always the same:
//! a dead zone of a cell's larger side around the centre, the names about two cell heights out
//! on whole cells (two rows up and down, three columns out, so they read in both renderers), the
//! disc three cell heights in radius. The surface owns one while it's open
//! ([`crate::surface`]); both renderers draw it from the [`Scene`](crate::render::Scene) over
//! the committed picture, as the marquee is drawn, so a hover costs its own rectangle; and it
//! never touches the model, the file or the undo log.

use crate::model::{CellSize, Point, Rect};
use crate::surface::Tool;
use std::f32::consts::TAU;

/// The sectors, north first then clockwise.
pub const SECTORS: [Tool; 8] = [
    Tool::Pen,
    Tool::Rect,
    Tool::Ellipse,
    Tool::Arrow,
    Tool::Line,
    Tool::Label,
    Tool::Eraser,
    Tool::Select,
];

/// The disc's radius, in cell heights.
pub const RADIUS: f32 = 3.0;

/// Where each sector's name hangs off the centre's cell, in columns and rows, and by which end:
/// its start east of the centre, its end west of it, its middle north and south. Whole cells,
/// so a name keeps its row wherever in a cell the centre falls.
const NAME_CELLS: [(f32, f32, Align); 8] = [
    (0.0, -2.0, Align::Middle),
    (3.0, -1.0, Align::Start),
    (3.0, 0.0, Align::Start),
    (3.0, 1.0, Align::Start),
    (0.0, 2.0, Align::Middle),
    (-3.0, 1.0, Align::End),
    (-3.0, 0.0, Align::End),
    (-3.0, -1.0, Align::End),
];

/// What opened the dial, which is what closes it: the other's key or button does nothing to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opener {
    /// Space, held.
    Space,
    /// The right button, held.
    RightButton,
}

/// How a name hangs off its point: by its start, its middle or its end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    /// It starts there: east of the centre.
    Start,
    /// It's centred there: north and south.
    Middle,
    /// It ends there: west of the centre.
    End,
}

impl Align {
    /// The column a name `len` cells long starts in, hung off column `at`.
    pub fn start(self, at: i64, len: i64) -> i64 {
        match self {
            Align::Start => at,
            Align::Middle => at - len / 2,
            Align::End => at - len + 1,
        }
    }
}

/// A dial while it's open.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dial {
    /// Its centre, in board units.
    pub centre: Point,
    /// The disc's radius, in board units: [`RADIUS`] cell heights on screen.
    pub radius: f32,
    /// The dead zone's radius, in board units: a cell's larger side on screen.
    pub dead: f32,
    /// A screen cell in board units at the view's zoom: what the names are laid out in.
    pub cell: CellSize,
    /// The sector under the pointer, an index into [`SECTORS`], or None in the dead zone.
    pub hovered: Option<usize>,
    /// What opened it.
    pub by: Opener,
}

impl Dial {
    /// A dial about `centre` with nothing hovered yet, sized from a cell of `cell` board units
    /// at `zoom`.
    pub fn open(centre: Point, by: Opener, cell: CellSize, zoom: f32) -> Dial {
        let zoom = if zoom > 0.0 { zoom } else { 1.0 };
        let cell = CellSize {
            w: cell.w / zoom,
            h: cell.h / zoom,
        };
        Dial {
            centre,
            radius: RADIUS * cell.h,
            dead: cell.w.max(cell.h),
            cell,
            hovered: None,
            by,
        }
    }

    /// The unit vector `turn` eighths of a turn clockwise from north (up: negative y): sector
    /// `i`'s middle at `i`, the spoke before it at `i - 0.5`.
    pub fn direction(turn: f32) -> Point {
        let a = TAU * turn / 8.0;
        Point::new(a.sin(), -a.cos())
    }

    /// The sector under `p`, or None in the dead zone.
    pub fn sector_at(&self, p: Point) -> Option<usize> {
        if p.dist(self.centre) < self.dead {
            return None;
        }
        let d = p - self.centre;
        let turn = (d.x.atan2(-d.y) / TAU).rem_euclid(1.0) * 8.0;
        Some((turn + 0.5).floor() as usize % 8)
    }

    /// The pointer is at `p`: whether the sector hovered changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let was = self.hovered;
        self.hovered = self.sector_at(p);
        was != self.hovered
    }

    /// The tool hovered, if one is.
    pub fn tool(&self) -> Option<Tool> {
        self.hovered.map(|i| SECTORS[i])
    }

    /// The disc: what the pixel renderer draws and presents. The names are cells hung off it.
    pub fn bounds(&self) -> Rect {
        Rect::from_points(self.centre, self.centre).inflate(self.radius)
    }

    /// Where sector `i`'s name hangs: its point, whole cells from the centre, and by which end.
    pub fn name_at(&self, i: usize) -> (Point, Align) {
        let (cols, rows, align) = NAME_CELLS[i];
        let at = self.centre + Point::new(cols * self.cell.w, rows * self.cell.h);
        (at, align)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dial() -> Dial {
        Dial::open(
            Point::new(100.0, 100.0),
            Opener::Space,
            CellSize { w: 5.0, h: 10.0 },
            1.0,
        )
    }

    #[test]
    fn the_sectors_go_clockwise_from_north_and_the_dead_zone_is_a_cells_larger_side() {
        let d = dial();
        assert_eq!((d.radius, d.dead), (30.0, 10.0));
        let at = |dx: f32, dy: f32| d.sector_at(Point::new(100.0 + dx, 100.0 + dy));
        assert_eq!(at(0.0, -20.0), Some(0), "north: the pen");
        assert_eq!(at(20.0, -20.0), Some(1), "north-east: rect");
        assert_eq!(at(20.0, 0.0), Some(2), "east: ellipse");
        assert_eq!(at(20.0, 20.0), Some(3));
        assert_eq!(at(0.0, 20.0), Some(4), "south: line");
        assert_eq!(at(-20.0, 20.0), Some(5));
        assert_eq!(at(-20.0, 0.0), Some(6), "west: the eraser");
        assert_eq!(at(-20.0, -20.0), Some(7), "north-west: select");
        assert_eq!(
            at(-3.0, -20.0),
            Some(0),
            "a little off north is still north"
        );
        assert_eq!(at(0.0, -9.0), None, "the dead zone");
        assert_eq!(at(0.0, 0.0), None);
        assert_eq!(SECTORS[6], Tool::Eraser);
        let mut d = d;
        assert!(d.hover(Point::new(120.0, 100.0)));
        assert!(!d.hover(Point::new(125.0, 101.0)), "the same sector");
        assert_eq!(d.tool(), Some(Tool::Ellipse));
        assert!(d.hover(Point::new(100.0, 100.0)));
        assert_eq!(d.tool(), None);
    }

    #[test]
    fn the_names_hang_whole_cells_out_by_the_end_that_faces_the_centre_and_zoom_keeps_the_size() {
        let d = dial();
        let (p, a) = d.name_at(0);
        assert_eq!((p, a), (Point::new(100.0, 80.0), Align::Middle), "north");
        let (p, a) = d.name_at(1);
        assert_eq!(
            (p, a),
            (Point::new(115.0, 90.0), Align::Start),
            "north-east"
        );
        let (p, a) = d.name_at(2);
        assert_eq!((p, a), (Point::new(115.0, 100.0), Align::Start));
        let (p, a) = d.name_at(6);
        assert_eq!((p, a), (Point::new(85.0, 100.0), Align::End), "west");
        assert_eq!(Align::Middle.start(10, 4), 8);
        assert_eq!(Align::End.start(10, 6), 5);
        assert_eq!(Align::Start.start(10, 6), 10);
        assert_eq!(
            d.bounds(),
            Rect::from_points(Point::new(70.0, 70.0), Point::new(130.0, 130.0))
        );
        let zoomed = Dial::open(
            d.centre,
            Opener::RightButton,
            CellSize { w: 5.0, h: 10.0 },
            2.0,
        );
        assert_eq!(
            (zoomed.radius, zoomed.dead),
            (15.0, 5.0),
            "half the board units at 2x"
        );
        assert_eq!(zoomed.name_at(4).0, Point::new(100.0, 110.0));
    }
}
