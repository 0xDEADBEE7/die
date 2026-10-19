//! The scene: one grid of cells per frame (scope §5.1). `Ctx::compose` lays
//! every placed window, its gutter and status line, the dividers between
//! windows and the command-line row into one `Scene`, with each cell's faces
//! resolved to colors. die-render draws it as it is.
//!
//! Search, selection and bracket highlights are computed here, at render time,
//! from the window's selection, the search pattern and the cursor, so they need
//! no stored decorations.

use crate::ctx::*;
use crate::faces::*;
use crate::ids::*;
use crate::keymap::State;
use crate::layout::{CellKind, Viewport};
use crate::text::*;
use std::fmt::{self, Write};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use unicode_width::UnicodeWidthChar;

/// What a cell draws.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Glyph {
    Char(char),
    /// A grapheme of several characters, such as a letter with combining marks.
    Cluster(Box<str>),
    /// The right half of the wide character in the cell before it.
    Continuation,
}

impl fmt::Display for Glyph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Glyph::Char(c) => f.write_char(*c),
            Glyph::Cluster(s) => f.write_str(s),
            Glyph::Continuation => Ok(()),
        }
    }
}

impl Glyph {
    fn of(s: &str) -> Glyph {
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (None, _) => Glyph::Continuation,
            (Some(c), None) => Glyph::Char(c),
            _ => Glyph::Cluster(s.into()),
        }
    }
}

/// One cell of the grid, with its faces resolved to colors and attributes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Cell {
    pub glyph: Glyph,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: Underline,
    pub underline_color: Rgb,
}

impl Cell {
    /// A cell drawing `glyph` in `style`, which must set `fg` and `bg`.
    fn new(glyph: Glyph, s: Style) -> Cell {
        let fg = s.fg.unwrap_or(FALLBACK_FG);
        Cell {
            glyph,
            fg,
            bg: s.bg.unwrap_or(FALLBACK_BG),
            bold: s.bold.unwrap_or(false),
            italic: s.italic.unwrap_or(false),
            strike: s.strike.unwrap_or(false),
            underline: s.underline.unwrap_or_default(),
            underline_color: s.underline_color.unwrap_or(fg),
        }
    }
}

/// Used only when a theme leaves `default`'s colors unset.
const FALLBACK_FG: Rgb = Rgb::hex("#d0d0d0");
const FALLBACK_BG: Rgb = Rgb::hex("#101010");

/// How a cursor is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CursorShape {
    /// Normal and visual state.
    Block,
    /// Insert state, and the command line.
    Bar,
    /// A line under the character.
    Underscore,
    /// Operator-pending state: the lower half of a block, as Evil draws it.
    HalfBlock,
    /// The cursor of a window that doesn't have focus.
    Hollow,
}

/// A cursor in the frame.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    /// The `cursor` face's background.
    pub color: Rgb,
}

/// Where a window sits in the frame: its text area's top-left cell. The text
/// area is the window's own size (`Ctx::window_size`), its status line is the
/// row below that, and a divider column follows it on the right unless it
/// reaches the frame's right edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    pub window: WindowId,
    pub col: usize,
    pub row: usize,
}

/// One frame, as one grid of cells.
#[derive(Clone, PartialEq, Debug)]
pub struct Scene {
    pub cols: usize,
    pub rows: usize,
    /// Row-major: row `r` is `cells[r * cols..(r + 1) * cols]`.
    pub cells: Vec<Cell>,
    /// For each cell, the window and buffer position it shows, if any. Pointer
    /// input maps through this.
    pub positions: Vec<Option<(WindowId, usize)>>,
    /// The cursor that takes input first, then a hollow cursor for each other window.
    pub cursors: Vec<Cursor>,
    /// `default`'s background, for the pixels around the grid.
    pub background: Rgb,
}

impl Scene {
    pub fn row(&self, r: usize) -> &[Cell] {
        &self.cells[r * self.cols..(r + 1) * self.cols]
    }

    /// The row's characters, with trailing spaces trimmed.
    pub fn row_text(&self, r: usize) -> String {
        let s: String = self.row(r).iter().map(|c| c.glyph.to_string()).collect();
        s.trim_end().to_string()
    }

    /// The window and buffer position the cell at (row, col) shows.
    pub fn position(&self, row: usize, col: usize) -> Option<(WindowId, usize)> {
        (row < self.rows && col < self.cols)
            .then(|| self.positions[row * self.cols + col])
            .flatten()
    }

    fn put(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = cell;
            self.positions[row * self.cols + col] = None;
        }
    }

    /// The scene as plain text, for tests and the harness: the characters, then
    /// a letter per cell naming its style, a legend for the letters, and the cursors.
    pub fn snapshot(&self) -> String {
        type Key = (Rgb, Rgb, bool, bool, bool, Underline, Rgb);
        let key = |c: &Cell| {
            (
                c.fg,
                c.bg,
                c.bold,
                c.italic,
                c.strike,
                c.underline,
                c.underline_color,
            )
        };
        let letters: Vec<char> = ('a'..='z').chain('A'..='Z').chain('0'..='9').collect();
        let mut styles: Vec<Key> = vec![];
        let mut out = format!("scene {}x{}\ntext\n", self.cols, self.rows);
        for r in 0..self.rows {
            let line: String = self.row(r).iter().map(|c| c.glyph.to_string()).collect();
            let _ = writeln!(out, "|{line}|");
        }
        out.push_str("style\n");
        for r in 0..self.rows {
            let mut line = String::new();
            for c in self.row(r) {
                let k = key(c);
                let i = styles.iter().position(|s| *s == k).unwrap_or_else(|| {
                    styles.push(k);
                    styles.len() - 1
                });
                line.push(letters.get(i).copied().unwrap_or('?'));
            }
            let _ = writeln!(out, "|{line}|");
        }
        out.push_str("legend\n");
        for (i, (fg, bg, bold, italic, strike, ul, ulc)) in styles.iter().enumerate() {
            let _ = write!(
                out,
                "{} {fg} on {bg}",
                letters.get(i).copied().unwrap_or('?')
            );
            for (on, name) in [(bold, "bold"), (italic, "italic"), (strike, "strike")] {
                if *on {
                    let _ = write!(out, " {name}");
                }
            }
            if *ul != Underline::None {
                let _ = write!(out, " underline {ul:?} {ulc}");
            }
            out.push('\n');
        }
        out.push_str("cursors\n");
        for c in &self.cursors {
            let _ = writeln!(out, "{},{} {:?} {}", c.row, c.col, c.shape, c.color);
        }
        out
    }
}

/// How a window's text is painted in this frame.
struct Look<'a> {
    faces: &'a Faces,
    /// `default`, with every attribute set.
    base: Style,
    search: Option<&'a str>,
    visual: bool,
}

/// What a window's text shows in this frame, beyond its decorations.
struct Highlights {
    search: Vec<Range<usize>>,
    cursor: Option<usize>,
    bracket: Option<[usize; 2]>,
    selection: Option<Selection>,
}

impl Ctx {
    /// Highlight every match of `pattern` in every window, as `/` does in
    /// die-vim. `None` turns it off. A match that starts at a window's cursor
    /// uses `search-current`.
    pub fn set_search_highlight(&mut self, pattern: Option<String>) {
        self.search_highlight = pattern.filter(|p| !p.is_empty());
    }

    /// Compose a `cols` × `rows` frame: each placed window with its gutter,
    /// status line and divider, and the command line in the last row. Each
    /// window is scrolled to show its cursor first (C5).
    pub fn compose(&mut self, cols: usize, rows: usize, placements: &[Placement]) -> Scene {
        let base = self
            .faces
            .get("default")
            .over(Style::new().fg(FALLBACK_FG).bg(FALLBACK_BG));
        let blank = Cell::new(Glyph::Char(' '), base);
        let background = base.bg.unwrap_or(FALLBACK_BG);
        let mut scene = Scene {
            cols,
            rows,
            cells: vec![blank; cols * rows],
            positions: vec![None; cols * rows],
            cursors: vec![],
            background,
        };
        let cursor_color = self
            .faces
            .get("cursor")
            .bg
            .or(base.fg)
            .unwrap_or(FALLBACK_FG);
        let input_in_prompt = self.prompt_label().is_some();
        let mut hollow = vec![];
        for p in placements {
            let Some(win) = self.windows.get(p.window) else {
                continue;
            };
            let (wc, wr, bid) = (win.cols, win.rows, win.buffer);
            let focused = self.focused() == Some(p.window);
            let cursor = self.draw_window(&mut scene, p, base);
            let status = if focused {
                "status-line"
            } else {
                "status-line-inactive"
            };
            let style = self.faces.get(status).over(base);
            let name = self
                .buffers
                .get(bid)
                .map_or(String::new(), |b| format!(" {}", b.name));
            let mut x = 0;
            for ch in name.chars().chain(std::iter::repeat(' ')) {
                if x >= wc {
                    break;
                }
                x += put_char(&mut scene, p.row + wr, p.col + x, ch, style);
            }
            if p.col + wc < cols {
                let style = self.faces.get("divider").over(base);
                for y in 0..=wr {
                    scene.put(p.row + y, p.col + wc, Cell::new(Glyph::Char('│'), style));
                }
            }
            if let Some((r, c)) = cursor.filter(|(r, c)| p.row + r < rows && p.col + c < cols) {
                let state = self.buffers.get(bid).map_or(State::Normal, |b| b.state);
                let shape = if focused && !input_in_prompt {
                    shape_for(state)
                } else {
                    CursorShape::Hollow
                };
                let cur = Cursor {
                    row: p.row + r,
                    col: p.col + c,
                    shape,
                    color: cursor_color,
                };
                if shape == CursorShape::Hollow {
                    hollow.push(cur)
                } else {
                    scene.cursors.push(cur)
                }
            }
        }
        if let (Some(label), Some(text), Some(last)) =
            (self.prompt_label(), self.prompt_text(), rows.checked_sub(1))
        {
            let prompt = self.faces.get("prompt").over(base);
            let mut x = 0;
            for (s, ch) in label
                .chars()
                .map(|c| (prompt, c))
                .chain(text.chars().map(|c| (base, c)))
            {
                x += put_char(&mut scene, last, x, ch, s);
            }
            scene.cursors.insert(
                0,
                Cursor {
                    row: last,
                    col: x,
                    shape: CursorShape::Bar,
                    color: cursor_color,
                },
            );
        }
        scene.cursors.extend(hollow);
        scene
    }

    /// Draw one window's content; returns its cursor's (row, col) in the window.
    fn draw_window(
        &mut self,
        scene: &mut Scene,
        p: &Placement,
        base: Style,
    ) -> Option<(usize, usize)> {
        let win = self.windows.get(p.window)?;
        let (cols, rows) = (win.cols, win.rows);
        let b = self.buffers.get_mut(win.buffer)?;
        let state = b.state;
        let Buffer { content, modes, .. } = b;
        match content {
            Content::Text(t) => {
                let look = Look {
                    faces: &self.faces,
                    base,
                    search: self.search_highlight.as_deref(),
                    visual: state == State::Visual,
                };
                draw_text(scene, p, (cols, rows), t, modes, &look)
            }
            Content::Custom { obj, .. } => {
                let mut out = vec![];
                if let Some(s) = obj
                    && catch_unwind(AssertUnwindSafe(|| s.render(cols, rows, &mut out))).is_err()
                {
                    out = vec!["<render panicked>".into()]; // until the next good render (§15)
                }
                for (r, line) in out.iter().enumerate().take(rows) {
                    let mut x = 0;
                    for ch in line.chars() {
                        if x >= cols {
                            break;
                        }
                        x += put_char(scene, p.row + r, p.col + x, ch, base);
                    }
                }
                None
            }
        }
    }
}

/// Put a character at (row, col), and a continuation after it if it's wide.
/// Returns the columns it took.
fn put_char(scene: &mut Scene, row: usize, col: usize, ch: char, style: Style) -> usize {
    scene.put(row, col, Cell::new(Glyph::Char(ch), style));
    let w = ch.width().unwrap_or(0).max(1);
    if w == 2 {
        scene.put(row, col + 1, Cell::new(Glyph::Continuation, style));
    }
    w
}

fn shape_for(state: State) -> CursorShape {
    match state {
        State::Insert | State::CommandLine => CursorShape::Bar,
        State::OperatorPending => CursorShape::HalfBlock,
        _ => CursorShape::Block,
    }
}

fn draw_text(
    scene: &mut Scene,
    p: &Placement,
    (cols, rows): (usize, usize),
    t: &mut TextBuf,
    modes: &mut [ModeSlot],
    look: &Look,
) -> Option<(usize, usize)> {
    let Look { faces, base, .. } = *look;
    t.scroll_to_cursor(p.window);
    let Viewport {
        geometry: g,
        rows: vrows,
        ..
    } = t.viewport(p.window)?;
    let cursor = t.cursor(p.window);
    let positions = || {
        vrows
            .iter()
            .flat_map(|r| r.cells.iter().filter_map(|c| c.pos))
    };
    let visible = positions().min().unwrap_or(0)..positions().max().map_or(t.len(), |q| q + 1);

    // Faces: stored Face decorations, then mode providers (faces only, C16).
    let mut deco: Vec<(Range<usize>, &'static str)> = t
        .decorations()
        .filter_map(|(_, d)| match d.deco {
            Deco::Face(f) => Some((d.range.clone(), f)),
            _ => None,
        })
        .collect();
    for m in modes.iter_mut() {
        if let Some(o) = &mut m.obj {
            let tt: &TextBuf = t;
            let _ = catch_unwind(AssertUnwindSafe(|| {
                o.decorate(tt, visible.clone(), &mut deco)
            }));
        }
    }
    let hl = Highlights {
        search: look.search.map_or(vec![], |pat| matches(t, pat, &visible)),
        cursor,
        bracket: cursor.and_then(|c| bracket_pair(t, c, &visible)),
        selection: if look.visual {
            t.selection(p.window)
        } else {
            None
        },
    };
    let block = hl.selection.filter(|s| s.shape == Shape::Block).map(|s| {
        let (a, h) = (t.display_col(s.anchor), t.display_col(s.head));
        let (la, lh) = (t.line_of(s.anchor), t.line_of(s.head));
        (la.min(lh)..=la.max(lh), a.min(h)..=a.max(h))
    });
    let style_at = |q: usize| {
        let mut s = base;
        if let Some((_, f)) = deco.iter().rev().find(|(r, _)| r.contains(&q)) {
            s = faces.get(f).over(s);
        }
        if let Some(m) = hl.search.iter().find(|m| m.contains(&q)) {
            let current = hl.cursor == Some(m.start);
            s = faces
                .get(if current { "search-current" } else { "search" })
                .over(s);
        }
        if hl.bracket.is_some_and(|b| b.contains(&q)) {
            s = faces.get("bracket-match").over(s);
        }
        if let Some(sel) = hl.selection {
            let on = match (&block, sel.shape) {
                (Some((lines, vcols)), _) => {
                    lines.contains(&t.line_of(q)) && vcols.contains(&t.display_col(q))
                }
                (None, Shape::Line) => {
                    let (a, h) = (t.line_of(sel.anchor), t.line_of(sel.head));
                    (a.min(h)..=a.max(h)).contains(&t.line_of(q))
                }
                // A charwise selection covers both of its ends, as Vim's does.
                _ => (sel.anchor.min(sel.head)..=sel.anchor.max(sel.head)).contains(&q),
            };
            if on {
                s = faces.get("selection").over(s);
            }
        }
        s
    };

    let cur_line = cursor.map(|c| t.line_of(c));
    let numbers = t.settings().line_numbers;
    for (i, vr) in vrows.iter().enumerate().take(rows) {
        let y = p.row + i;
        if g.gutter > 0 {
            let current = cur_line == Some(vr.first_line);
            let face = if current {
                "line-number-current"
            } else {
                "line-number"
            };
            let style = faces.get(face).over(base);
            let label = match (vr.row, numbers, cur_line) {
                (0, LineNumbers::Relative, Some(cl)) if cl != vr.first_line => {
                    cl.abs_diff(vr.first_line).to_string()
                }
                (0, _, _) => (vr.first_line + 1).to_string(),
                _ => String::new(),
            };
            let s = format!("{label:>w$} ", w = g.gutter - 1);
            for (k, ch) in s.chars().enumerate() {
                scene.put(y, p.col + g.pad + k, Cell::new(Glyph::Char(ch), style));
            }
        }
        // A tab's later cells and a wide character's right half have no position;
        // they take the style of the cell they belong to.
        let mut last = None;
        for (j, c) in vr
            .cells
            .iter()
            .enumerate()
            .take(cols.saturating_sub(g.text_x))
        {
            let q = match (c.pos, c.kind) {
                (Some(q), _) => Some(q),
                (None, CellKind::Tab) => last,
                (None, CellKind::Text) if c.text.is_empty() => last,
                _ => None,
            };
            last = q;
            let style = q.map_or(base, style_at);
            let x = p.col + g.text_x + j;
            scene.put(y, x, Cell::new(Glyph::of(&c.text), style));
            if let (Some(q), true) = (c.pos, y < scene.rows && x < scene.cols) {
                scene.positions[y * scene.cols + x] = Some((p.window, q));
            }
        }
    }
    if t.settings().cursor_visible {
        cursor.and_then(|c| t.screen_pos(p.window, c))
    } else {
        None
    }
}

/// Every match of `pat` that touches `visible`.
fn matches(t: &TextBuf, pat: &str, visible: &Range<usize>) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut from = visible.start.saturating_sub(pat.len() - 1);
    while let Some(at) = t.find(pat, from) {
        // `find` wraps around to the start: a match before `from` means we're done.
        if at < from || at >= visible.end {
            break;
        }
        out.push(at..at + pat.len());
        from = at + 1;
    }
    out
}

/// The bracket at `pos` and its partner, when both are in `visible`.
fn bracket_pair(t: &TextBuf, pos: usize, visible: &Range<usize>) -> Option<[usize; 2]> {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    let rope = t.rope();
    if pos >= rope.len_bytes() {
        return None;
    }
    let ch = rope.char(rope.byte_to_char(pos));
    let (open, close, forward) = PAIRS.iter().find_map(|&(o, c)| match ch {
        _ if ch == o => Some((o, c, true)),
        _ if ch == c => Some((o, c, false)),
        _ => None,
    })?;
    let (mut depth, mut at) = (0usize, rope.byte_to_char(pos));
    loop {
        let c = rope.char(at);
        if c == open || c == close {
            if (c == open) == forward {
                depth += 1;
            } else {
                depth -= 1;
            }
            if depth == 0 {
                return Some([pos, rope.char_to_byte(at)]);
            }
        }
        if forward {
            at += 1;
            if at >= rope.len_chars() || rope.char_to_byte(at) >= visible.end {
                return None;
            }
        } else {
            if at == 0 || rope.char_to_byte(at - 1) < visible.start {
                return None;
            }
            at -= 1;
        }
    }
}
