//! The scene: one grid of cells per frame (scope §5.1). `Ctx::compose` lays
//! every placed window, its gutter and status line, the dividers between
//! windows and the command-line row into one `Scene`, with each cell's faces
//! resolved to colors. die-render draws it as it is.
//!
//! Search, selection and bracket highlights are computed here, at render time,
//! from the window's selection, the search pattern and the cursor, so they need
//! no stored decorations.

use crate::ext::*;
use crate::faces::*;
use crate::ids::*;
use crate::keymap::State;
use crate::layout::{CellKind, Dir, Viewport, control_cells};
use crate::text::*;
use std::cmp::Reverse;
use std::fmt::{self, Write};
use std::ops::{Range, RangeInclusive};
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
    pub(crate) fn of(s: &str) -> Glyph {
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (None, _) => Glyph::Continuation,
            (Some(c), None) => Glyph::Char(c),
            _ => Glyph::Cluster(s.into()),
        }
    }
}

/// One cell of the grid, with its faces resolved to colors and attributes.
///
/// A cell producer resolves what a cell doesn't hold: reverse video swaps `fg`
/// and `bg`, faint keeps 65 % of `fg` and takes 35 % of `bg` (as ghostel does), invisible
/// text draws a space, and blinking text draws steadily. Cells are built with
/// `Cell::new` and then their fields set, so a field added later (overline and
/// the link id, with die-term in M2) breaks no producer. Outside die-core, a
/// struct expression doesn't compile, even one that names only some fields:
///
/// ```compile_fail,E0639
/// use die_core::{Cell, Glyph, Rgb};
/// let c = Rgb::hex("#000000");
/// let _ = Cell { bold: true, ..Cell::new(Glyph::Char('x'), c, c) };
/// ```
#[non_exhaustive]
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
    /// `glyph` in `fg` on `bg`, with no attributes and the underline in `fg`.
    pub fn new(glyph: Glyph, fg: Rgb, bg: Rgb) -> Cell {
        Cell {
            glyph,
            fg,
            bg,
            bold: false,
            italic: false,
            strike: false,
            underline: Underline::None,
            underline_color: fg,
        }
    }

    /// A cell drawing `glyph` in `style`, which has `default` under it, so only a
    /// theme that leaves `default`'s colors unset gets the fallbacks.
    pub(crate) fn styled(glyph: Glyph, s: Style) -> Cell {
        let fg = s.fg.unwrap_or(FALLBACK_FG);
        let mut c = Cell::new(glyph, fg, s.bg.unwrap_or(FALLBACK_BG));
        c.bold = s.bold.unwrap_or(false);
        c.italic = s.italic.unwrap_or(false);
        c.strike = s.strike.unwrap_or(false);
        c.underline = s.underline.unwrap_or_default();
        c.underline_color = s.underline_color.unwrap_or(fg);
        c
    }
}

/// Used only when a theme leaves `default`'s colors unset.
const FALLBACK_FG: Rgb = Rgb::hex("#d0d0d0");
const FALLBACK_BG: Rgb = Rgb::hex("#101010");

/// How a cursor is drawn, in its `color`. It doesn't blink (scope §8). Its pixels
/// are logical, as Emacs's `(bar . 2)` is: 4 physical pixels at 2x.
///
/// A cursor the color of its cell's background wouldn't show, and a `Block` would
/// erase the glyph. So on such a cell, as mono-dark's current search match is, the
/// cell's foreground takes the cursor's place, and a `Block`'s glyph stays in the
/// cell's background, as Emacs's `x_set_cursor_gc` does.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CursorShape {
    /// Normal and visual state: the cell filled with the color, and its glyph
    /// drawn in the cell's background color and only inside the cell, as Emacs's
    /// box cursor does.
    Block,
    /// Insert state and the command line: a bar 2 logical px wide at the cell's
    /// left edge, over the glyph (Evil's `(bar . 2)`).
    Bar,
    /// Replace state (Evil's `hbar`), and with die-term (M2) a terminal's
    /// underline cursor (DECSCUSR 3 and 4): a bar 2 logical px high at the cell's
    /// bottom, over the glyph.
    Underline,
    /// Operator-pending state: the cell's lower half filled with the color,
    /// hiding the glyph there, as Evil's half cursor does.
    HalfBlock,
    /// A cursor that doesn't take input: another window's, or the focused
    /// window's while the command line has a prompt, and every cursor while
    /// DIE's window doesn't have the keyboard focus. A 1 logical px outline
    /// of the cell, with the glyph drawn as it is, as Emacs draws the cursor of a
    /// window that isn't selected.
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
    /// The position is just past the row's last cell, with no cell there in the window (contract
    /// §9, "Line ends"). `col` is that last cell. A bar goes at its right edge, not its left, and
    /// other shapes draw on the cell as usual.
    pub past_end: bool,
}

/// Where a window sits in the frame: its text area's top-left cell. The text
/// area is the window's own size (`Ctx::window_size`), its status line is the
/// row below that, and a divider column follows it on the right unless it
/// reaches the frame's right edge. (`Placement` is left for M3's images and
/// pixel buffers on rectangles of cells.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowPlacement {
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
    /// the character it shows: every cell of a tab, a wide character or a Hide's
    /// replacement maps to it, and a line's newline has a cell of its own on a
    /// row with room for one. Cells past the end of a row map to its last
    /// position, and past the end of a line's last row, to its newline (on the
    /// last line, to the end of the buffer), so a click there lands at the
    /// line's end, as in Emacs. Other cells, such as virtual text, gutters, status
    /// lines and the rows below the buffer's last line, map to None.
    pub positions: Vec<Option<(WindowId, usize)>>,
    /// Every cursor drawn. The one that takes input (the command line's while
    /// a prompt is open, otherwise the focused window's) comes first, and it's
    /// the only one that can be other than `Hollow`. A cursor that's hidden, or past the
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

    pub(crate) fn put(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = cell;
            self.positions[row * self.cols + col] = None;
        }
    }

    pub(crate) fn map(&mut self, row: usize, col: usize, to: (WindowId, usize)) {
        if row < self.rows && col < self.cols {
            self.positions[row * self.cols + col] = Some(to);
        }
    }

    /// The scene as plain text, for tests and the harness: the characters, then
    /// a letter per cell naming its style, a legend for the letters, and the
    /// cursors. It panics past 62 styles, the letters it has.
    pub fn snapshot(&self) -> String {
        type Key = (Rgb, Rgb, bool, bool, bool, Underline, Rgb);
        // Every field, so a new one doesn't compile here until the key shows it.
        let key = |c: &Cell| {
            let Cell {
                glyph: _,
                fg,
                bg,
                bold,
                italic,
                strike,
                underline,
                underline_color,
            } = *c;
            (fg, bg, bold, italic, strike, underline, underline_color)
        };
        let letters: Vec<char> = ('a'..='z').chain('A'..='Z').chain('0'..='9').collect();
        let letter = |i: usize| match letters.get(i) {
            Some(l) => *l,
            None => panic!("a snapshot shows at most {} styles", letters.len()),
        };
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
                line.push(letter(i));
            }
            let _ = writeln!(out, "|{line}|");
        }
        out.push_str("legend\n");
        for (i, (fg, bg, bold, italic, strike, ul, ulc)) in styles.iter().enumerate() {
            let _ = write!(out, "{} {fg} on {bg}", letter(i));
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
            let past = if c.past_end { " past-end" } else { "" };
            let _ = writeln!(out, "{},{} {:?}{past} {}", c.row, c.col, c.shape, c.color);
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
    pub fn compose(&mut self, cols: usize, rows: usize, placements: &[WindowPlacement]) -> Scene {
        self.sync_messages();
        self.sweep_frames();
        let base = self
            .faces
            .get("default")
            .over(Style::new().fg(FALLBACK_FG).bg(FALLBACK_BG));
        let blank = Cell::styled(Glyph::Char(' '), base);
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
        let input_in_prompt = self.prompt.is_some();
        let mut hollow = vec![];
        let mut placed = Vec::with_capacity(placements.len());
        for p in placements {
            let Some(win) = self.windows.get(p.window) else {
                continue;
            };
            let (wc, wr, bid) = (win.cols, win.rows, win.buffer);
            placed.push((*p, wc, wr));
            let focused = self.focused() == Some(p.window);
            let cursor = self.draw_window(&mut scene, p, base, focused);
            self.draw_status(&mut scene, p, p.row + wr, base);
            if p.col + wc < cols {
                let style = self.faces.get("divider").over(base);
                for y in 0..=wr {
                    scene.put(p.row + y, p.col + wc, Cell::styled(Glyph::Char('│'), style));
                }
            }
            // A cursor past an unwrapped window's edge isn't drawn (M0 has no
            // horizontal scrolling), nor one outside the frame.
            let cursor = cursor
                .filter(|&(r, c, ..)| r < wr && c < wc)
                .map(|(r, c, past, own)| (p.row + r, p.col + c, past, own))
                .filter(|&(r, c, ..)| r < rows && c < cols);
            if let Some((row, col, past_end, own)) = cursor {
                let state = self.buffers.get(bid).map_or(State::Normal, |b| b.state);
                let takes_input = focused && !input_in_prompt;
                let shape = if takes_input && self.app_focused {
                    own.unwrap_or(CursorShape::for_state(state))
                } else {
                    CursorShape::Hollow
                };
                let cur = Cursor {
                    row,
                    col,
                    shape,
                    color: cursor_color,
                    past_end,
                };
                if takes_input {
                    scene.cursors.push(cur)
                } else {
                    hollow.push(cur)
                }
            }
        }
        let mut covered = rows;
        let (x, end) = self.prompt_band(placements, cols);
        if let (Some(label), Some(last)) = (self.prompt_lead(), rows.checked_sub(1)) {
            let room = self.prompt_list_height().min(last);
            let row = last - room;
            covered = row;
            self.draw_prompt_list(&mut scene, row, room, base, (x, end));
            // As Emacs's minibuffer shrinks the windows above it, their status lines sit on
            // the list from edge to edge, with the list's content inside the margins below.
            if let Some(sep) = row.checked_sub(1).filter(|_| room > 0) {
                for &(p, ..) in placed
                    .iter()
                    .filter(|(p, _, wr)| (p.row..p.row + wr).contains(&sep))
                {
                    self.draw_status(&mut scene, &p, sep, base);
                }
                covered = sep;
            }
            hollow.retain(|c: &Cursor| c.row < covered);
            let label = label.as_str();
            let text = self.prompt_text().unwrap_or_default();
            let prompt = self.faces.get("prompt").over(base);
            let note = self.prompt_note();
            let parts: Vec<_> = glyphs(label, prompt)
                .chain(glyphs(text, base))
                .chain(glyphs(note, prompt))
                .collect();
            let typed = self.prompt_before_cursor().unwrap_or_default();
            let at = glyphs(label, prompt).chain(glyphs(typed, base)).count();
            // When it doesn't fit, leave off the start, up to where the cursor shows.
            let (mut col, mut skip) = (parts[..at].iter().map(|g| g.1).sum::<usize>(), 0);
            while col >= end - x && skip < at {
                col -= parts[skip].1;
                skip += 1;
            }
            put_text(&mut scene, row, x, end, parts.into_iter().skip(skip));
            if col < end - x {
                let bar = Cursor {
                    row,
                    col: x + col,
                    shape: if self.app_focused {
                        CursorShape::Bar
                    } else {
                        CursorShape::Hollow
                    },
                    color: cursor_color,
                    past_end: false,
                };
                scene.cursors.insert(0, bar);
            }
        } else if let Some(last) = rows.checked_sub(1) {
            // A message stops a column short of the note, which keeps its place.
            let note = self.log.note.as_deref().unwrap_or_default();
            let wide: usize = glyphs(note, base).map(|g| g.1).sum();
            let room = end.saturating_sub(wide + usize::from(wide > 0)).max(x);
            let echo = self.log.echo.as_deref().unwrap_or_default();
            put_text(&mut scene, last, x, room, glyphs(echo, base));
            let at = end.saturating_sub(wide).max(x);
            put_text(&mut scene, last, at, end, glyphs(note, base));
        }
        scene.cursors.extend(hollow);
        self.drawn.note(&scene, placed);
        self.drawn.covered = covered;
        scene
    }

    /// The columns the prompt and the echo area use, start and end: with a maximum width on
    /// the focused window's buffer (`\z`), that window's margins, where its line numbers start
    /// and where its text ends, as your Emacs's minibuffer has them. Otherwise the frame's.
    fn prompt_band(&self, placements: &[WindowPlacement], cols: usize) -> (usize, usize) {
        let band = || {
            let w = self.focused()?;
            let p = placements.iter().find(|p| p.window == w)?;
            let t = self.text(self.windows.get(w)?.buffer).ok()?;
            t.settings().max_width?;
            let g = t.geometry(w)?;
            let x = (p.col + g.pad).min(cols.saturating_sub(2));
            Some((x, (x + g.gutter + g.text_w).min(cols)))
        };
        band().unwrap_or((0, cols))
    }

    /// A window's status line, on `row`: its own row below its text, or the row above the
    /// prompt's list.
    fn draw_status(&mut self, scene: &mut Scene, p: &WindowPlacement, row: usize, base: Style) {
        let Some((cols, buffer)) = self.windows.get(p.window).map(|w| (w.cols, w.buffer)) else {
            return;
        };
        let status = if self.focused() == Some(p.window) {
            "status-line"
        } else {
            "status-line-inactive"
        };
        let style = self.faces.get(status).over(base);
        for x in 0..cols {
            scene.put(row, p.col + x, Cell::styled(Glyph::Char(' '), style));
        }
        if self.buffers.contains_key(buffer) {
            let (text, tag) = self.status_line(buffer);
            let at = (p.col + cols).saturating_sub(tag.len() + 1).max(p.col + 1);
            let room = if tag.is_empty() { p.col + cols } else { at - 1 };
            put_text(
                scene,
                row,
                p.col + 1,
                room.max(p.col + 1),
                glyphs(&text, style),
            );
            put_text(scene, row, at, p.col + cols, glyphs(tag, style));
        }
    }

    /// Draw one window's content; returns its cursor's (row, col) in the window, whether it's
    /// past the end of its row, and the shape a surface asks for.
    fn draw_window(
        &mut self,
        scene: &mut Scene,
        p: &WindowPlacement,
        base: Style,
        focused: bool,
    ) -> Option<(usize, usize, bool, Option<CursorShape>)> {
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
                draw_text(scene, p, (cols, rows), t, modes, &look, &mut self.messages)
                    .map(|(r, c, past)| (r, c, past, None))
            }
            Content::Custom { .. } => self.draw_surface(scene, p, base, focused),
        }
    }
}

/// `text`'s graphemes in `style`, each with its width in cells, as layout
/// draws them: a control character shows as `^A`, a cell per character.
pub(crate) fn glyphs(text: &str, style: Style) -> impl Iterator<Item = (Glyph, usize, Style)> + '_ {
    text.graphemes(true)
        .flat_map(move |g| match g.chars().next().filter(|c| c.is_control()) {
            Some(c) => control_cells(c)
                .into_iter()
                .map(|c| (Glyph::Char(c), 1, style))
                .collect(),
            None => vec![(Glyph::of(g), g.width().max(1), style)],
        })
}

/// Put glyphs on `row` from `col` and return the column after them. They
/// stop before `end`: a wide glyph that doesn't fit isn't drawn, and the
/// cells it would have taken keep what they had.
pub(crate) fn put_text(
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
        scene.put(row, col, Cell::styled(g, style));
        for i in 1..w {
            scene.put(row, col + i, Cell::styled(Glyph::Continuation, style));
        }
        col += w;
    }
    col
}

fn draw_text(
    scene: &mut Scene,
    p: &WindowPlacement,
    (cols, rows): (usize, usize),
    t: &mut TextBuf,
    modes: &mut [ModeSlot],
    look: &Look,
    messages: &mut Vec<String>,
) -> Option<(usize, usize, bool)> {
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
    let wrapped = t.settings().wrap != Wrap::Off;
    // A line end has a cell only with room for one: past a full row's end, it takes none.
    let has_cell = |col: usize| col < room && !(wrapped && col >= g.text_w);
    let positions = || {
        let drawn = vrows.iter().flat_map(|r| r.cells.iter().take(room));
        let ends = vrows.iter().filter_map(|r| r.line_end);
        drawn
            .filter(|c| c.kind != CellKind::Filler)
            .map(|c| c.pos)
            .chain(ends.filter(|e| has_cell(e.col)).map(|e| e.pos))
    };
    let visible = match (positions().min(), positions().max()) {
        // To the end of the last character drawn, so the range slices.
        (Some(lo), Some(hi)) if hi < t.len() => {
            lo..t.rope().char_to_byte(t.rope().byte_to_char(hi) + 1)
        }
        (Some(lo), Some(hi)) => lo..hi,
        _ => 0..0,
    };

    // Faces, bottom to top (faces.rs): stored Face decorations, then mode
    // providers' (faces only, C16).
    let mut deco: Vec<(Range<usize>, &'static str)> = t
        .decorations()
        .filter_map(|(_, d)| match d.deco {
            Deco::Face(f) if d.range.start < visible.end && d.range.end > visible.start => {
                Some((d.range.clone(), f))
            }
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
    let bracket = cursor.and_then(|c| bracket_pair(t, c, &visible));
    let selection = t
        .selection(p.window)
        .filter(|_| look.visual)
        .map(|s| selected(t, s));
    let block = selection.as_ref().is_some_and(|s| s.1.is_some());

    // Each face once, and a sweep over the cells, so a frame costs one step per
    // cell and per face, not their product. `deco`'s order is the stacking order.
    let layers: Vec<(Range<usize>, Style)> = deco
        .iter()
        .map(|(r, f)| (r.clone(), faces.of_decoration(f, messages)))
        .collect();
    let mut by_start: Vec<usize> = (0..layers.len()).collect();
    by_start.sort_by_key(|&i| layers[i].0.start);
    let [search_face, current_face, bracket_face, selection_face] =
        ["search", "search-current", "bracket-match", "selection"].map(|f| faces.get(f));
    let (mut next, mut on, mut in_search, mut at) = (0, Vec::<usize>::new(), 0, 0);
    let mut style_at = |q: usize, vcol: usize, width: usize| {
        // Cells only move forward through a window; start over if one ever doesn't.
        if q < at {
            (next, in_search) = (0, 0);
            on.clear();
        }
        at = q;
        while let Some(&i) = by_start.get(next).filter(|&&i| layers[i].0.start <= q) {
            on.insert(on.partition_point(|&a| a < i), i);
            next += 1;
        }
        on.retain(|&i| layers[i].0.end > q);
        let mut s = on.iter().fold(base, |s, &i| layers[i].1.over(s));
        while search.get(in_search).is_some_and(|m| m.end <= q) {
            in_search += 1;
        }
        if current.as_ref().is_some_and(|m| m.contains(&q)) {
            s = current_face.over(s);
        } else if search.get(in_search).is_some_and(|m| m.contains(&q)) {
            s = search_face.over(s);
        }
        if bracket.is_some_and(|b| b.contains(&q)) {
            s = bracket_face.over(s);
        }
        if let Some((bytes, block_cols)) = &selection
            && bytes.contains(&q)
            && block_cols.as_ref().is_none_or(|v| {
                // A block stops at a line's end, as Evil's overlays do.
                vcol <= *v.end()
                    && vcol + width > *v.start()
                    && q < t.len()
                    && t.rope().byte(q) != b'\n'
            })
        {
            s = selection_face.over(s);
        }
        s
    };

    let text_x = p.col + g.text_x;
    for (i, vr) in vrows.iter().enumerate().take(rows) {
        let y = p.row + i;
        if g.gutter > 0 {
            let face = if vr.current {
                "line-number-current"
            } else {
                "line-number"
            };
            let style = faces.get(face).over(base);
            let label = vr.number.map_or(String::new(), |n| n.to_string());
            // A number wider than its cells shows its last digits, so the text doesn't move.
            let w = g.gutter - 1;
            let s = format!("{:>w$} ", &label[label.len().saturating_sub(w)..]);
            // A window narrower than its gutter shows the gutter's start.
            for (k, ch) in s.chars().enumerate().take(cols.saturating_sub(g.pad)) {
                scene.put(y, p.col + g.pad + k, Cell::styled(Glyph::Char(ch), style));
            }
        }
        // Every cell of a tab, a wide character or a replacement shows its one position, and
        // virtual text is styled as the position it stands before, but isn't that position. A
        // block takes a replacement whole, by that position's column, as it does a tab.
        let mut row_end = None;
        for (j, c) in vr.cells.iter().enumerate().take(room) {
            let mut style = base;
            if c.kind != CellKind::Filler {
                let whole = block && c.kind == CellKind::Replacement;
                // A block takes a tab or a wide character it covers any part of.
                let (vcol, width) = if whole {
                    (t.display_col(c.pos), 1)
                } else if block {
                    let next = t.display_col(t.snap(c.pos + 1, Some(Dir::Forward)));
                    (c.vcol, next.saturating_sub(c.vcol).max(1))
                } else {
                    (c.vcol, 1)
                };
                style = style_at(c.pos, vcol, width);
            }
            if let Some(f) = c.face {
                style = faces.of_decoration(f, messages).over(style);
            }
            scene.put(y, text_x + j, Cell::styled(c.glyph.clone(), style));
            if !matches!(c.kind, CellKind::Filler | CellKind::Virtual) {
                row_end = Some(c.pos);
                scene.map(y, text_x + j, (p.window, c.pos));
            }
        }
        // The newline's own cell, and where the cells past the row's end land.
        // Padding where a wide character didn't fit is past the end too.
        if let Some(e) = vr.line_end {
            if has_cell(e.col) {
                let style = style_at(e.pos, e.vcol, 1);
                scene.put(y, text_x + e.col, Cell::styled(Glyph::Char(' '), style));
            }
            row_end = Some(e.pos);
        }
        if let Some(to) = row_end {
            let tail = vr.cells.iter().rposition(|c| c.kind != CellKind::Filler);
            for j in tail.map_or(0, |n| n + 1)..room {
                scene.map(y, text_x + j, (p.window, to));
            }
        }
    }
    if !t.settings().cursor_visible {
        return None;
    }
    let (row, col) = cursor.and_then(|c| t.screen_pos(p.window, c))?;
    // Past a full row with no right padding, it draws on the row's last cell.
    let past_end = wrapped && col == g.text_x + g.text_w && col >= cols;
    Some((row, col - usize::from(past_end), past_end))
}

/// A selection's bytes, and for a block its display columns on those lines. A
/// charwise selection covers both of its ends, as Vim's does. A linewise one takes
/// in the buffer's end on the last line, which has a cell too.
pub(crate) fn selected(
    t: &TextBuf,
    sel: Selection,
) -> (Range<usize>, Option<RangeInclusive<usize>>) {
    let (lo, hi) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let past = t.line_of(hi) + 1;
    let end = t.line_start(past) + usize::from(past == t.line_count());
    let lines = t.line_start(t.line_of(lo))..end;
    match sel.shape {
        Shape::Char => (lo..hi + 1, None),
        Shape::Line => (lines, None),
        Shape::Block => {
            let (a, h) = (t.display_col(sel.anchor), t.display_col(sel.head));
            let right = if sel.goal == Some(usize::MAX) {
                usize::MAX
            } else {
                a.max(h)
            };
            (lines, Some(a.min(h)..=right))
        }
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
