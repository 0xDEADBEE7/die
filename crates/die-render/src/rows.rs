//! The row cache and diff: a frame's cells in, its instances out.
//!
//! `Frames` keeps the last frame's cells and each row's instances, and rebuilds only
//! the rows whose cells changed (scope §5.11). A frame takes three steps, so that
//! the glyphs can come from wherever the driver gets them, and tests can supply
//! their own:
//!
//! 1. `diff` compares the scene with the last frame, and says which glyphs the
//!    changed rows need that the atlas doesn't have.
//! 2. `supply` hands over each one's bitmap. If that fills the atlas, it starts over
//!    and every row needs its glyphs again, so `wanted` says what's still missing.
//! 3. `build` rebuilds the changed rows and gives the frame's instances. Cursors are
//!    drawn fresh each frame, on top, so moving one rebuilds no row.
//!
//! Instances go in three layers, so that nothing covers what should be above it:
//! every background, then every glyph and line, then the cursors. A glyph may reach
//! past its cell, and a neighbour's background doesn't cut it off.
#![warn(missing_docs)]

use crate::atlas::{Atlas, Full, GlyphKey, Slot};
use crate::font::CellMetrics;
use crate::frame::{GlyphBitmap, Plane, Quad, Upload, kind, rgba};
use crate::sprite::Line;
use die_core::{Cell, Cursor, CursorShape, Glyph, Rgb, Scene, Underline};
use std::collections::HashSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The instances of one row, between frames.
#[derive(Clone, Default)]
struct Row {
    backgrounds: Vec<Quad>,
    glyphs: Vec<Quad>,
    dirty: bool,
}

/// The cells a row's glyph takes: its width, but no more than the continuation
/// cells after it. A padding cell at a row's end is a continuation too, with no
/// wide character before it, so the width has to agree.
fn span_at(row: &[Cell], col: usize) -> u16 {
    let width = match &row[col].glyph {
        Glyph::Char(c) => c.width().unwrap_or(1),
        Glyph::Cluster(s) => s.width(),
        Glyph::Continuation => return 1,
    };
    let room = row[col + 1..]
        .iter()
        .take_while(|c| c.glyph == Glyph::Continuation)
        .count();
    width.max(1).min(room + 1).min(usize::from(u16::MAX)) as u16
}

/// Call `f` with each glyph the cell at `col` draws: its text, then its lines.
fn each_key(row: &[Cell], col: usize, mut f: impl FnMut(GlyphKey)) {
    let cell = &row[col];
    if !matches!(cell.glyph, Glyph::Continuation | Glyph::Char(' ')) {
        f(GlyphKey::Text {
            glyph: cell.glyph.clone(),
            bold: cell.bold,
            italic: cell.italic,
            span: span_at(row, col),
        });
    }
    if cell.underline != Underline::None {
        f(GlyphKey::Line(Line::Under(cell.underline)));
    }
    if cell.strike {
        f(GlyphKey::Line(Line::Strike));
    }
}

/// An instance for a glyph in its cell at pixel (x, y).
fn glyph_quad(slot: &Slot, x: f32, y: f32, color: Rgb) -> Quad {
    let (w, h) = (slot.w as f32, slot.h as f32);
    Quad {
        rect: [x + slot.left as f32, y + slot.top as f32, w, h],
        uv: [slot.x as f32, slot.y as f32, w, h],
        color: rgba(color),
        kind: match slot.plane {
            Plane::Gray => kind::GRAY,
            Plane::Color => kind::COLOR,
        },
        pad: [0; 2],
    }
}

fn solid(rect: [f32; 4], color: Rgb) -> Quad {
    Quad {
        rect,
        uv: [0.0; 4],
        color: rgba(color),
        kind: kind::SOLID,
        pad: [0; 2],
    }
}

/// The instances for a cell's glyph and lines. `ink` replaces their colors, as a
/// block cursor does.
fn cell_glyphs(
    (row, col): (&[Cell], usize),
    y: f32,
    cell: &CellMetrics,
    atlas: &Atlas,
    ink: Option<Rgb>,
    out: &mut Vec<Quad>,
) {
    let c = &row[col];
    let x = col as f32 * cell.w as f32;
    each_key(row, col, |key| {
        let color = match (&key, ink) {
            (_, Some(ink)) => ink,
            (GlyphKey::Line(Line::Under(_)), None) => c.underline_color,
            _ => c.fg,
        };
        if let Some(Some(slot)) = atlas.get(&key) {
            out.push(glyph_quad(&slot, x, y, color));
        }
    });
}

/// A row's backgrounds, one for each run of a color, skipping the scene's own
/// background, which the frame is cleared to.
fn row_backgrounds(row: &[Cell], y: f32, cell: &CellMetrics, background: Rgb, out: &mut Vec<Quad>) {
    let (cw, ch) = (cell.w as f32, cell.h as f32);
    let mut start = 0;
    for col in 1..=row.len() {
        if col == row.len() || row[col].bg != row[start].bg {
            if row[start].bg != background {
                let width = (col - start) as f32 * cw;
                out.push(solid([start as f32 * cw, y, width, ch], row[start].bg));
            }
            start = col;
        }
    }
}

/// The rectangles of a cursor on `span` cells at pixel (x, y), as `CursorShape`'s
/// rustdoc says, with its pixels logical. The bar and underline never leave the cell.
fn cursor_rects(
    shape: CursorShape,
    x: f32,
    y: f32,
    span: f32,
    cell: &CellMetrics,
) -> Vec<[f32; 4]> {
    let (w, h) = (span * cell.w as f32, cell.h as f32);
    let two = cell.logical(2.0) as f32;
    let one = (cell.logical(1.0) as f32).min(w / 2.0).min(h / 2.0);
    match shape {
        CursorShape::Block => vec![[x, y, w, h]],
        CursorShape::Bar => vec![[x, y, two.min(w), h]],
        CursorShape::Underline => vec![[x, y + h - two.min(h), w, two.min(h)]],
        CursorShape::HalfBlock => {
            let half = (h / 2.0).floor();
            vec![[x, y + h - half, w, half]]
        }
        CursorShape::Hollow => vec![
            [x, y, w, one],
            [x, y + h - one, w, one],
            [x, y + one, one, h - 2.0 * one],
            [x + w - one, y + one, one, h - 2.0 * one],
        ],
    }
}

/// What a cursor of this color is drawn in on `cell`: its color, unless that's the
/// cell's background, where it wouldn't show and a block would erase the glyph. There
/// the cell's foreground takes over, as Emacs's `x_set_cursor_gc` has it, then the
/// scene's background, then black or white, so there's always a color that shows.
fn cursor_color(color: Rgb, cell: &Cell, background: Rgb) -> Rgb {
    if color != cell.bg {
        return color;
    }
    let last = [Rgb(0, 0, 0), Rgb(255, 255, 255)];
    let others = [cell.fg, background].into_iter().chain(last);
    others.into_iter().find(|&c| c != color).unwrap_or(color)
}

/// A frame's data for the backend.
#[derive(Debug)]
pub struct Frame<'a> {
    /// The instances, in draw order.
    pub quads: &'a [Quad],
    /// What the target is cleared to first.
    pub clear: Rgb,
    /// Pixels for the backend to copy into the atlases first.
    pub uploads: Vec<Upload>,
}

/// The row cache and the glyph atlas: the renderer's state between frames.
pub struct Frames {
    cell: CellMetrics,
    atlas: Atlas,
    cols: usize,
    background: Option<Rgb>,
    /// The last frame's cells, row-major as `Scene::cells` is.
    cells: Vec<Cell>,
    rows: Vec<Row>,
    quads: Vec<Quad>,
    rebuilt: Vec<usize>,
    /// Whether this frame has already started the atlas over.
    started_over: bool,
    /// Whether a glyph was dropped for lack of room, to try again next frame.
    dropped: bool,
}

impl Frames {
    /// No frame yet, with square atlas planes of these sizes, in pixels.
    pub fn new(cell: CellMetrics, gray: u32, color: u32) -> Frames {
        Frames {
            cell,
            atlas: Atlas::new(gray, color),
            cols: 0,
            background: None,
            cells: vec![],
            rows: vec![],
            quads: vec![],
            rebuilt: vec![],
            started_over: false,
            dropped: false,
        }
    }

    /// The cell's geometry.
    pub fn cell(&self) -> &CellMetrics {
        &self.cell
    }

    /// Which rows the last `build` rebuilt.
    pub fn rebuilt(&self) -> &[usize] {
        &self.rebuilt
    }

    fn start_over(&mut self) {
        self.atlas.clear();
        self.rows.iter_mut().for_each(|r| r.dirty = true);
    }

    /// Step 1: compare `scene` with the last frame and note the rows that changed.
    /// A new size or background changes every row, since a cell in the background's
    /// color has no instance. Returns the glyphs those rows need.
    pub fn diff(&mut self, scene: &Scene) -> Vec<GlyphKey> {
        self.started_over = false;
        if std::mem::take(&mut self.dropped) {
            self.start_over();
            self.started_over = true;
        }
        let resized = self.cols != scene.cols || self.rows.len() != scene.rows;
        if resized {
            self.cols = scene.cols;
            self.cells = scene.cells.clone();
            self.rows = vec![Row::default(); scene.rows];
        }
        let everything = resized || self.background != Some(scene.background);
        self.background = Some(scene.background);
        for r in 0..scene.rows {
            let at = r * scene.cols..(r + 1) * scene.cols;
            if everything || self.cells[at.clone()] != scene.cells[at.clone()] {
                self.rows[r].dirty = true;
                self.cells[at.clone()].clone_from_slice(&scene.cells[at]);
            }
        }
        self.wanted()
    }

    /// The glyphs the changed rows need that the atlas doesn't have, each once.
    pub fn wanted(&self) -> Vec<GlyphKey> {
        let mut seen = HashSet::new();
        let mut out = vec![];
        for r in (0..self.rows.len()).filter(|&r| self.rows[r].dirty) {
            let row = &self.cells[r * self.cols..(r + 1) * self.cols];
            for col in 0..row.len() {
                each_key(row, col, |key| {
                    if self.atlas.get(&key).is_none() && seen.insert(key.clone()) {
                        out.push(key);
                    }
                });
            }
        }
        out
    }

    /// Step 2: `key`'s bitmap, or `None` if it draws nothing.
    ///
    /// If the atlas is full, it starts over once a frame, and the rows want their
    /// glyphs again. A frame that needs more than a whole atlas drops the rest, and
    /// the next frame tries again.
    pub fn supply(&mut self, key: &GlyphKey, bitmap: Option<GlyphBitmap>) {
        let Err(Full(bitmap)) = self.atlas.insert(key, bitmap) else {
            return;
        };
        if !self.started_over {
            self.started_over = true;
            self.start_over();
            if self.atlas.insert(key, Some(bitmap)).is_ok() {
                return;
            }
        }
        self.dropped = true;
        let _ = self.atlas.insert(key, None);
    }

    /// Step 3: rebuild the rows that changed, then give the frame's instances.
    pub fn build(&mut self, scene: &Scene) -> Frame<'_> {
        self.rebuilt.clear();
        let (cell, atlas) = (&self.cell, &self.atlas);
        for r in 0..self.rows.len() {
            if !std::mem::take(&mut self.rows[r].dirty) {
                continue;
            }
            let row = &self.cells[r * self.cols..(r + 1) * self.cols];
            let y = r as f32 * cell.h as f32;
            let out = &mut self.rows[r];
            out.backgrounds.clear();
            out.glyphs.clear();
            row_backgrounds(row, y, cell, scene.background, &mut out.backgrounds);
            for col in 0..row.len() {
                cell_glyphs((row, col), y, cell, atlas, None, &mut out.glyphs);
            }
            self.rebuilt.push(r);
        }
        self.quads.clear();
        for r in &self.rows {
            self.quads.extend_from_slice(&r.backgrounds);
        }
        for r in &self.rows {
            self.quads.extend_from_slice(&r.glyphs);
        }
        for c in &scene.cursors {
            self.cursor(c, scene.background);
        }
        Frame {
            quads: &self.quads,
            clear: scene.background,
            uploads: self.atlas.take_uploads(),
        }
    }

    /// A cursor's instances. On the right half of a wide character it takes the
    /// character's first cell, and it covers the character's cells. A cursor off the
    /// grid draws nothing.
    fn cursor(&mut self, cursor: &Cursor, background: Rgb) {
        if cursor.row >= self.rows.len() || cursor.col >= self.cols {
            return;
        }
        let row = &self.cells[cursor.row * self.cols..(cursor.row + 1) * self.cols];
        let mut col = cursor.col;
        while col > 0 && row[col].glyph == Glyph::Continuation {
            col -= 1;
        }
        let span = f32::from(span_at(row, col));
        let cell = &self.cell;
        let (x, y) = (
            col as f32 * cell.w as f32,
            cursor.row as f32 * cell.h as f32,
        );
        let color = cursor_color(cursor.color, &row[col], background);
        for rect in cursor_rects(cursor.shape, x, y, span, cell) {
            self.quads.push(solid(rect, color));
        }
        if cursor.shape == CursorShape::Block {
            let ink = Some(row[col].bg);
            cell_glyphs((row, col), y, cell, &self.atlas, ink, &mut self.quads);
        }
    }
}
