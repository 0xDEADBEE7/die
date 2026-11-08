//! What a cell of the frame shows (K9). Core keeps the last composed frame's map of cells, since
//! the host owns the frame itself. What a click does is K14's.
#![warn(missing_docs)]
use crate::ext::*;
use crate::ids::*;
use crate::scene::{Scene, WindowPlacement};

/// The part of a window a cell belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPart {
    /// The text area, with its gutter and padding.
    Text,
    /// The status line.
    Status,
    /// The divider on its right.
    Divider,
}

/// What a cell of the frame showed in the last composed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellHit {
    /// The window it belongs to.
    pub window: WindowId,
    /// The part of the window.
    pub part: WindowPart,
    /// The buffer position it maps to, as [`Scene::positions`] says.
    pub pos: Option<usize>,
}

/// What the last frame's cells showed.
#[derive(Default)]
pub(crate) struct Drawn {
    cols: usize,
    rows: usize,
    positions: Vec<Option<(WindowId, usize)>>,
    /// Each placed window's placement and text size.
    placed: Vec<(WindowPlacement, usize, usize)>,
    /// The first row the prompt covers: the windows drawn under it don't show from there down.
    pub(crate) covered: usize,
}

impl Drawn {
    /// Note the frame just composed. This keeps a copy of the map, because the host owns the
    /// frame: it costs a memory copy of 16 bytes a cell.
    pub(crate) fn note(&mut self, s: &Scene, placed: Vec<(WindowPlacement, usize, usize)>) {
        self.cols = s.cols;
        self.rows = s.rows;
        self.positions.clone_from(&s.positions);
        self.placed = placed;
    }

    /// Where the last frame drew a window.
    pub(crate) fn placement(&self, w: WindowId) -> Option<WindowPlacement> {
        self.placed
            .iter()
            .find(|(p, ..)| p.window == w)
            .map(|p| p.0)
    }
}

impl Ctx {
    /// The position a click means on a text cell that shows none: the first to its right on the
    /// row, as for the gutter, or else the nearest to its left, as for virtual text at the row's
    /// edge, or else the last above it, as for the rows below the text.
    pub(crate) fn nearest_pos(&self, win: WindowId, row: usize, col: usize) -> Option<usize> {
        let d = &self.drawn;
        let (p, cols, _) = d.placed.iter().find(|(p, ..)| p.window == win)?;
        // A frame composed smaller than the window shows only part of it.
        let cells = p.col..(p.col + cols).min(d.cols);
        let at = |r: usize, c: usize| d.positions[r * d.cols + c].filter(|(w, _)| *w == win);
        let last_of = |r| cells.clone().rev().find_map(|c| at(r, c));
        (col..cells.end)
            .find_map(|c| at(row, c))
            .or_else(|| (cells.start..col).rev().find_map(|c| at(row, c)))
            .or_else(|| (p.row..row).rev().find_map(last_of))
            .map(|(_, pos)| pos)
    }

    /// What the cell at (row, col) of the last composed frame showed: its window, the part of it,
    /// and the buffer position. None for a cell outside every window, such as the command line,
    /// or one the prompt's list covers.
    pub fn cell_at(&self, row: usize, col: usize) -> Option<CellHit> {
        let d = &self.drawn;
        if row >= d.rows.min(d.covered) || col >= d.cols {
            return None;
        }
        let pos = d.positions[row * d.cols + col];
        d.placed.iter().rev().find_map(|&(p, cols, rows)| {
            let right = usize::from(p.col + cols < d.cols);
            let inside = (p.col..p.col + cols + right).contains(&col)
                && (p.row..=p.row + rows).contains(&row);
            inside.then(|| CellHit {
                window: p.window,
                part: if col == p.col + cols {
                    WindowPart::Divider
                } else if row == p.row + rows {
                    WindowPart::Status
                } else {
                    WindowPart::Text
                },
                pos: pos.filter(|(w, _)| *w == p.window).map(|(_, at)| at),
            })
        })
    }
}
