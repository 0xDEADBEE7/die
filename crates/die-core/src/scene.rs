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
use std::cmp::Reverse;
use std::fmt::{self, Write};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// What a cell draws.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Glyph {
    /// A single character.
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
    /// What the cell draws.
    pub glyph: Glyph,
    /// The foreground color.
    pub fg: Rgb,
    /// The background color.
    pub bg: Rgb,
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Struck-through text.
    pub strike: bool,
    /// The underline's style.
    pub underline: Underline,
    /// The underline's color.
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
    /// Operator-pending state: the lower half of a block, as Evil draws it.
    HalfBlock,
    /// A cursor that doesn't take input: another window's, or the focused
    /// window's while the command line has a prompt.
    Hollow,
}

/// A cursor on the grid.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cursor {
    /// The cell's row.
    pub row: usize,
    /// The cell's column.
    pub col: usize,
    /// How it's drawn.
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
    /// The window placed.
    pub window: WindowId,
    /// The frame column of the text area's left edge.
    pub col: usize,
    /// The frame row of the text area's top edge.
    pub row: usize,
}

/// One frame, as one grid of cells.
#[derive(Clone, PartialEq, Debug)]
pub struct Scene {
    /// The grid's width in cells.
    pub cols: usize,
    /// The grid's height in cells.
    pub rows: usize,
    /// Row-major: row `r` is `cells[r * cols..(r + 1) * cols]`.
    pub cells: Vec<Cell>,
    /// For each cell of a window's text, the window and the buffer position of
    /// the character it shows: every cell of a tab or a wide character maps to
    /// it. Other cells, such as gutters, status lines and the cells past the
    /// end of a line, map to None.
    pub positions: Vec<Option<(WindowId, usize)>>,
    /// Every cursor drawn. The one that takes input (the command line's while
    /// a prompt is open, otherwise the focused window's) comes first, and it's
    /// the only one that isn't `Hollow`. A cursor that's hidden, or past the
    /// edge of an unwrapped window (M0 has no horizontal scrolling), isn't drawn.
    pub cursors: Vec<Cursor>,
    /// `default`'s background, for the pixels around the grid.
    pub background: Rgb,
}

impl Scene {
    /// Row `r`'s cells.
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
    search: Option<&'a Pattern>,
    /// Draw the selection: visual state, in the focused window.
    visual: bool,
}

/// What a window's text shows in this frame, beyond its decorations.
struct Highlights {
    search: Vec<Range<usize>>,
    /// The match that starts at the cursor.
    current: Option<Range<usize>>,
    bracket: Option<[usize; 2]>,
    selection: Option<Selection>,
}

impl Ctx {
    /// Highlight every match of `pattern` in every window, as `/` does in
    /// die-vim. `None` turns it off. A match that starts at a window's cursor
    /// uses `search-current`. Matches are found as `TextBuf::matches` finds
    /// them, and the pattern's case rule is the caller's (smartcase is V2's).
    pub fn set_search_highlight(&mut self, pattern: Option<Pattern>) {
        self.search_highlight = pattern.filter(|p| !p.as_str().is_empty());
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
            let cursor = self.draw_window(&mut scene, p, base, focused);
            let status = if focused {
                "status-line"
            } else {
                "status-line-inactive"
            };
            let style = self.faces.get(status).over(base);
            let row = p.row + wr;
            for x in 0..wc {
                scene.put(row, p.col + x, Cell::new(Glyph::Char(' '), style));
            }
            if let Some(b) = self.buffers.get(bid) {
                put_text(
                    &mut scene,
                    row,
                    p.col + 1,
                    p.col + wc,
                    glyphs(&b.name, style),
                );
            }
            if p.col + wc < cols {
                let style = self.faces.get("divider").over(base);
                for y in 0..=wr {
                    scene.put(p.row + y, p.col + wc, Cell::new(Glyph::Char('│'), style));
                }
            }
            // A cursor past an unwrapped window's edge isn't drawn (M0 has no
            // horizontal scrolling), nor one outside the frame.
            let cursor = cursor
                .filter(|&(r, c)| r < wr && c < wc)
                .map(|(r, c)| (p.row + r, p.col + c))
                .filter(|&(r, c)| r < rows && c < cols);
            if let Some((row, col)) = cursor {
                let state = self.buffers.get(bid).map_or(State::Normal, |b| b.state);
                let shape = if focused && !input_in_prompt {
                    shape_for(state)
                } else {
                    CursorShape::Hollow
                };
                let cur = Cursor {
                    row,
                    col,
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
            let parts: Vec<_> = glyphs(label, prompt).chain(glyphs(text, base)).collect();
            // When it doesn't fit, show the end, where the cursor is.
            let (mut width, mut skip) = (parts.iter().map(|g| g.1).sum::<usize>(), 0);
            while width >= cols && skip < parts.len() {
                width -= parts[skip].1;
                skip += 1;
            }
            let col = put_text(&mut scene, last, 0, cols, parts.into_iter().skip(skip));
            if col < cols {
                let bar = Cursor {
                    row: last,
                    col,
                    shape: CursorShape::Bar,
                    color: cursor_color,
                };
                scene.cursors.insert(0, bar);
            }
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
        focused: bool,
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
                    search: self.search_highlight.as_ref(),
                    visual: state == State::Visual && focused,
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
                    put_text(scene, p.row + r, p.col, p.col + cols, glyphs(line, base));
                }
                None
            }
        }
    }
}

/// `text`'s graphemes in `style`, each with its width in cells, as layout
/// draws them: a control character shows as '?'.
fn glyphs(text: &str, style: Style) -> impl Iterator<Item = (Glyph, usize, Style)> + '_ {
    text.graphemes(true).map(move |g| {
        if g.chars().any(char::is_control) {
            (Glyph::Char('?'), 1, style)
        } else {
            (Glyph::of(g), g.width().max(1), style)
        }
    })
}

/// Put glyphs on `row` from `col` and return the column after them. They
/// stop before `end`: a wide glyph that doesn't fit isn't drawn, and the
/// cells it would have taken keep what they had.
fn put_text(
    scene: &mut Scene,
    row: usize,
    mut col: usize,
    end: usize,
    glyphs: impl Iterator<Item = (Glyph, usize, Style)>,
) -> usize {
    for (g, w, style) in glyphs {
        if col + w > end {
            break;
        }
        scene.put(row, col, Cell::new(g, style));
        for i in 1..w {
            scene.put(row, col + i, Cell::new(Glyph::Continuation, style));
        }
        col += w;
    }
    col
}

fn shape_for(state: State) -> CursorShape {
    match state {
        State::Insert => CursorShape::Bar,
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
    // The columns left for text: an unwrapped line shows only its first ones.
    let room = cols.saturating_sub(g.text_x);
    let positions = || {
        vrows
            .iter()
            .flat_map(|r| r.cells.iter().take(room).filter_map(|c| c.pos))
    };
    let visible = match (positions().min(), positions().max()) {
        // To the end of the last character drawn, so the range slices.
        (Some(lo), Some(hi)) => lo..t.rope().char_to_byte(t.rope().byte_to_char(hi) + 1),
        _ => 0..0,
    };

    // Faces, bottom to top (faces.rs): stored Face decorations, then mode
    // providers' (faces only, C16).
    let mut deco: Vec<(Range<usize>, &'static str)> = t
        .decorations()
        .filter_map(|(_, d)| match d.deco {
            Deco::Face(f) => Some((d.range.clone(), f)),
            _ => None,
        })
        .collect();
    deco.sort_by_key(|(r, _)| (r.start, Reverse(r.end)));
    for m in modes.iter_mut() {
        if let Some(o) = &mut m.obj {
            let tt: &TextBuf = t;
            let _ = catch_unwind(AssertUnwindSafe(|| {
                o.decorate(tt, visible.clone(), &mut deco)
            }));
        }
    }
    let (search, current) = look
        .search
        .map_or((vec![], None), |pat| matches(t, pat, &visible, cursor));
    let hl = Highlights {
        search,
        current,
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
        for (_, f) in deco.iter().filter(|(r, _)| r.contains(&q)) {
            s = faces.get(f).over(s);
        }
        let current = hl.current.as_ref().is_some_and(|m| m.contains(&q));
        if current || hl.search.iter().any(|m| m.contains(&q)) {
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
            // A window narrower than its gutter shows the gutter's start.
            for (k, ch) in s.chars().enumerate().take(cols.saturating_sub(g.pad)) {
                scene.put(y, p.col + g.pad + k, Cell::new(Glyph::Char(ch), style));
            }
        }
        // A tab's later cells and a wide character's right half have no position
        // in layout; they show the character of the cell they belong to.
        let mut last = None;
        for (j, c) in vr.cells.iter().enumerate().take(room) {
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
            if let (Some(q), true) = (q, y < scene.rows && x < scene.cols) {
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

/// The matches of `pat` that touch `visible`, left to right without overlaps
/// as Vim draws them, and the match that starts at `cursor`, which can overlap them.
fn matches(
    t: &TextBuf,
    pat: &Pattern,
    visible: &Range<usize>,
    cursor: Option<usize>,
) -> (Vec<Range<usize>>, Option<Range<usize>>) {
    let current = cursor.and_then(|c| t.match_at(pat, c));
    (t.matches(pat, visible.clone()), current)
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
