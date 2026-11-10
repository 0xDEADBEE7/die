//! The cell renderer: the board in DIE's own cells, buildable today and the Linux preview's
//! fallback once the pixel renderer exists. Rectangles, lines and arrows snap to the grid and
//! draw in the light box glyphs (U+2500-257F), which die-render joins across cells; arrowheads
//! are `▶ ◀ ▲ ▼`; ellipses and strokes are braille (U+2800-28FF, 2×4 dots a cell, the ink's
//! colour per cell); labels are text; the selection is reverse video, the highlighter faint.
//! The newest element wins a cell. Honest as annotation, not as handwriting.
//!
//! The committed picture is drawn into a grid once per change and kept; each frame copies it,
//! draws the gesture's live element and the marquee over the copy, and writes the frame.

use crate::model::{CellSize, Element, Id, LIVE, Point, Rect, Shape, label_lines};
use crate::render::{Damage, Renderer, Scene};
use crate::view::View;
use die_core::{CellPixels, Color, Frame, FrameCell, Glyph, RenderInfo};
use std::f32::consts::TAU;

const UP: u8 = 1;
const RIGHT: u8 = 2;
const DOWN: u8 = 4;
const LEFT: u8 = 8;

/// The light box glyph for each set of arms: bit 0 up, 1 right, 2 down, 3 left.
const BOX: [char; 16] = [
    ' ', '╵', '╶', '└', '╷', '│', '┌', '├', '╴', '┘', '─', '┴', '┐', '┤', '┬', '┼',
];

/// A cell's width and height when the host hasn't said, as in a harness without
/// `set_cell_pixels`: a text cell at about 16 pt on a 1x display.
const FALLBACK_CELL: (f32, f32) = (10.0, 20.0);

#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum Kind {
    #[default]
    Blank,
    Box(u8),
    Braille(u8),
    Glyph(char),
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct CellBuf {
    owner: Option<Id>,
    kind: Kind,
    fg: Color,
    faint: bool,
    reverse: bool,
}

#[derive(Clone, Default)]
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<CellBuf>,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Grid {
        Grid {
            cols,
            rows,
            cells: vec![CellBuf::default(); cols * rows],
        }
    }

    /// The cell at (col, row) for `owner` to draw in: as it is if `owner` drew it last, else
    /// blank, since the newest element wins a cell. None outside the grid.
    fn claim(
        &mut self,
        col: i64,
        row: i64,
        owner: Id,
        fg: Color,
        faint: bool,
    ) -> Option<&mut CellBuf> {
        if col < 0 || row < 0 || col >= self.cols as i64 || row >= self.rows as i64 {
            return None;
        }
        let c = &mut self.cells[row as usize * self.cols + col as usize];
        if c.owner != Some(owner) {
            *c = CellBuf {
                owner: Some(owner),
                fg,
                faint,
                ..CellBuf::default()
            };
        }
        Some(c)
    }

    fn arms(&mut self, col: i64, row: i64, arms: u8, pen: &Pen) {
        if let Some(c) = self.claim(col, row, pen.owner, pen.fg, pen.faint) {
            c.kind = match c.kind {
                Kind::Box(a) => Kind::Box(a | arms),
                Kind::Glyph(g) => Kind::Glyph(g),
                _ => Kind::Box(arms),
            };
        }
    }

    fn glyph(&mut self, col: i64, row: i64, ch: char, pen: &Pen) {
        if let Some(c) = self.claim(col, row, pen.owner, pen.fg, pen.faint) {
            c.kind = Kind::Glyph(ch);
        }
    }

    /// Set a braille dot, by its dot coordinates (two a cell across, four down).
    fn dot(&mut self, dx: i64, dy: i64, pen: &Pen) {
        let (col, row) = (dx.div_euclid(2), dy.div_euclid(4));
        let (x, y) = (dx.rem_euclid(2) as u8, dy.rem_euclid(4) as u8);
        // Unicode's dot order: 1-3 down the left, 4-6 down the right, then 7 left, 8 right.
        let bit = if y < 3 { y + 3 * x } else { 6 + x };
        if let Some(c) = self.claim(col, row, pen.owner, pen.fg, pen.faint) {
            c.kind = match c.kind {
                Kind::Braille(b) => Kind::Braille(b | 1 << bit),
                Kind::Glyph(g) => Kind::Glyph(g),
                _ => Kind::Braille(1 << bit),
            };
        }
    }

    /// A horizontal run of arms along `row`, joining at its ends.
    fn hline(&mut self, row: i64, c0: i64, c1: i64, pen: &Pen) {
        let (a, b) = (c0.min(c1), c0.max(c1));
        if a == b {
            return self.arms(a, row, LEFT | RIGHT, pen);
        }
        for c in a..=b {
            let arms = if c == a {
                RIGHT
            } else if c == b {
                LEFT
            } else {
                LEFT | RIGHT
            };
            self.arms(c, row, arms, pen);
        }
    }

    /// A vertical run of arms down `col`, joining at its ends.
    fn vline(&mut self, col: i64, r0: i64, r1: i64, pen: &Pen) {
        let (a, b) = (r0.min(r1), r0.max(r1));
        if a == b {
            return self.arms(col, a, UP | DOWN, pen);
        }
        for r in a..=b {
            let arms = if r == a {
                DOWN
            } else if r == b {
                UP
            } else {
                UP | DOWN
            };
            self.arms(col, r, arms, pen);
        }
    }

    /// A line of dots from one dot to another (Bresenham).
    fn dot_line(&mut self, (x0, y0): (i64, i64), (x1, y1): (i64, i64), pen: &Pen) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        // A long way outside the grid is still bounded by the grid's own size.
        let most = (self.cols as i64 * 2 + self.rows as i64 * 4 + dx + (-dy)).min(1 << 20);
        for _ in 0..=most {
            self.dot(x, y, pen);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

/// Who's drawing, and in what.
struct Pen {
    owner: Id,
    fg: Color,
    faint: bool,
}

/// Board units to cells and dots.
struct Map {
    f: f32,
    pan: (f32, f32),
    cw: f32,
    ch: f32,
}

impl Map {
    fn new(view: &View, cell: CellPixels) -> Map {
        let (cw, ch) = if cell.width == 0 || cell.height == 0 {
            FALLBACK_CELL
        } else {
            (cell.width as f32, cell.height as f32)
        };
        Map {
            f: view.factor(cell.scale),
            pan: view.pan,
            cw,
            ch,
        }
    }
    fn px(&self, p: Point) -> (f32, f32) {
        (p.x * self.f + self.pan.0, p.y * self.f + self.pan.1)
    }
    fn cell(&self, p: Point) -> (i64, i64) {
        let (x, y) = self.px(p);
        ((x / self.cw).floor() as i64, (y / self.ch).floor() as i64)
    }
    fn dot(&self, p: Point) -> (i64, i64) {
        let (x, y) = self.px(p);
        (
            (x / (self.cw / 2.0)).floor() as i64,
            (y / (self.ch / 4.0)).floor() as i64,
        )
    }
}

/// The arrowhead for a line going in direction `d`.
fn head(d: Point) -> char {
    if d.x.abs() >= d.y.abs() {
        if d.x >= 0.0 { '▶' } else { '◀' }
    } else if d.y >= 0.0 {
        '▼'
    } else {
        '▲'
    }
}

fn draw(grid: &mut Grid, map: &Map, el: &Element, fg: Color) {
    let faint = matches!(
        el.shape,
        Shape::Stroke {
            highlighter: true,
            ..
        }
    );
    let pen = Pen {
        owner: el.id,
        fg,
        faint,
    };
    match &el.shape {
        Shape::Stroke { points, .. } => {
            if let [only] = points.as_slice() {
                let (x, y) = map.dot(*only);
                grid.dot(x, y, &pen);
            }
            for w in points.windows(2) {
                grid.dot_line(map.dot(w[0]), map.dot(w[1]), &pen);
            }
        }
        Shape::Rect { a, b } => {
            let r = Rect::from_points(*a, *b);
            let ((c0, r0), (c1, r1)) = (map.cell(r.min), map.cell(r.max));
            grid.hline(r0, c0, c1, &pen);
            grid.hline(r1, c0, c1, &pen);
            grid.vline(c0, r0, r1, &pen);
            grid.vline(c1, r0, r1, &pen);
        }
        Shape::Ellipse { a, b } => {
            let r = Rect::from_points(*a, *b);
            let c = r.center();
            let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
            let around = TAU * rx.max(ry) * map.f / (map.cw / 2.0).min(map.ch / 4.0);
            let n = (around.ceil() as usize).clamp(8, 4096);
            let at = |i: usize| {
                let t = TAU * i as f32 / n as f32;
                map.dot(Point::new(c.x + rx * t.cos(), c.y + ry * t.sin()))
            };
            for i in 0..n {
                grid.dot_line(at(i), at(i + 1), &pen);
            }
        }
        Shape::Line { a, b, arrow, .. } => {
            let ((c0, r0), (c1, r1)) = (map.cell(*a), map.cell(*b));
            if r0 == r1 {
                grid.hline(r0, c0, c1, &pen);
            } else if c0 == c1 {
                grid.vline(c0, r0, r1, &pen);
            } else {
                grid.dot_line(map.dot(*a), map.dot(*b), &pen);
            }
            if *arrow {
                grid.glyph(c1, r1, head(*b - *a), &pen);
            }
        }
        Shape::Label { at, text } => {
            let (c0, r0) = map.cell(*at);
            for (i, line) in label_lines(text).iter().enumerate() {
                for (j, ch) in line.chars().enumerate() {
                    grid.glyph(c0 + j as i64, r0 + i as i64, ch, &pen);
                }
            }
        }
    }
}

/// The renderer: the committed grid, and whether it's stale.
pub struct Cells {
    committed: Grid,
    stale: bool,
}

impl Default for Cells {
    fn default() -> Cells {
        Cells::new()
    }
}

impl Cells {
    /// A renderer with nothing drawn yet.
    pub fn new() -> Cells {
        Cells {
            committed: Grid::default(),
            stale: true,
        }
    }

    fn rebuild(&mut self, cols: usize, rows: usize, scene: &Scene<'_>, map: &Map) {
        let mut grid = Grid::new(cols, rows);
        for el in &scene.board.elements {
            draw(&mut grid, map, el, ink(scene, el));
        }
        if !scene.selection.is_empty() {
            for c in &mut grid.cells {
                if c.owner.is_some_and(|o| scene.selection.contains(&o)) {
                    c.reverse = true;
                }
            }
        }
        self.committed = grid;
        self.stale = false;
    }
}

fn ink(scene: &Scene<'_>, el: &Element) -> Color {
    let i = usize::from(el.ink.clamp(1, 6)) - 1;
    scene.inks[i]
}

impl Renderer for Cells {
    fn name(&self) -> &'static str {
        "cells"
    }

    fn resize(&mut self, _cols: usize, _rows: usize, _cell: CellPixels) {
        self.stale = true;
    }

    fn board_changed(&mut self, _damage: Damage) {
        self.stale = true;
    }

    fn render(&mut self, frame: &mut Frame, scene: &Scene<'_>, info: &RenderInfo) {
        let (cols, rows) = (frame.cols(), frame.rows());
        let map = Map::new(scene.view, info.cell);
        if self.stale || (self.committed.cols, self.committed.rows) != (cols, rows) {
            self.rebuild(cols, rows, scene, &map);
        }
        let mut grid = self.committed.clone();
        if let Some(live) = scene.live {
            draw(&mut grid, &map, live, ink(scene, live));
        }
        if let Some(m) = scene.marquee {
            let pen = Pen {
                owner: LIVE,
                fg: Color::Default,
                faint: true,
            };
            let ((c0, r0), (c1, r1)) = (map.cell(m.min), map.cell(m.max));
            grid.hline(r0, c0, c1, &pen);
            grid.hline(r1, c0, c1, &pen);
            grid.vline(c0, r0, r1, &pen);
            grid.vline(c1, r0, r1, &pen);
        }
        frame.clear();
        for (i, c) in grid.cells.iter().enumerate() {
            let ch = match c.kind {
                Kind::Blank => continue,
                Kind::Box(arms) => BOX[usize::from(arms & 15)],
                Kind::Braille(bits) => char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '),
                Kind::Glyph(g) => g,
            };
            let mut cell = FrameCell::new(Glyph::Char(ch), c.fg, Color::Default);
            cell.faint = c.faint;
            cell.reverse = c.reverse;
            frame.put(i / cols, i % cols, cell);
        }
    }
}

/// A cell in board units at these cell pixels, for labels and nudges: the fallback cell when
/// the host hasn't said.
pub fn cell_size(cell: CellPixels) -> CellSize {
    let scale = if cell.scale > 0.0 {
        cell.scale as f32
    } else {
        1.0
    };
    if cell.width == 0 || cell.height == 0 {
        return CellSize {
            w: FALLBACK_CELL.0 / scale,
            h: FALLBACK_CELL.1 / scale,
        };
    }
    CellSize {
        w: cell.width as f32 / scale,
        h: cell.height as f32 / scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_bits_follow_unicodes_dot_order() {
        let mut g = Grid::new(1, 1);
        let pen = Pen {
            owner: 1,
            fg: Color::Default,
            faint: false,
        };
        // Dot 1 (top left) is bit 0, dot 4 (top right) bit 3, dot 7 bit 6, dot 8 bit 7.
        g.dot(0, 0, &pen);
        assert_eq!(g.cells[0].kind, Kind::Braille(0b0000_0001));
        g.dot(1, 0, &pen);
        assert_eq!(g.cells[0].kind, Kind::Braille(0b0000_1001));
        g.dot(0, 3, &pen);
        g.dot(1, 3, &pen);
        assert_eq!(g.cells[0].kind, Kind::Braille(0b1100_1001));
        assert_eq!(char::from_u32(0x2800 + 0b1100_1001), Some('⣉'));
    }

    #[test]
    fn the_newest_element_wins_a_cell_and_arms_join() {
        let mut g = Grid::new(3, 3);
        let one = Pen {
            owner: 1,
            fg: Color::Default,
            faint: false,
        };
        let two = Pen {
            owner: 2,
            fg: Color::Default,
            faint: false,
        };
        g.hline(1, 0, 2, &one);
        g.vline(1, 0, 2, &one);
        let kinds: Vec<Kind> = g.cells.iter().map(|c| c.kind).collect();
        assert_eq!(kinds[4], Kind::Box(15), "a cross where they meet");
        assert_eq!((kinds[3], kinds[5]), (Kind::Box(RIGHT), Kind::Box(LEFT)));
        g.dot(2, 4, &two);
        assert_eq!(g.cells[4].kind, Kind::Braille(1), "the newer took the cell");
        assert_eq!(BOX[UP as usize | RIGHT as usize], '└');
    }
}
