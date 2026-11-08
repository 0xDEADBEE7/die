//! The engine's render state as rows of die-core cells, and the cursor (the record's G2).
//!
//! A cell holds what die-core's `Cell` can: the glyph, the colors, bold, italic, strike and the
//! underline. The rest is resolved here, as `Cell`'s producers do: reverse video swaps the
//! colors, faint blends the foreground toward the background (65 %, as ghostel does), invisible
//! text draws a space, and blinking text draws steadily. A wide character's second cell is a
//! `Continuation`.

use die_core::{Cell, CursorShape, Glyph, Rgb, Underline};
use libghostty_vt::Terminal;
use libghostty_vt::render::{
    CellIteration, CellIterator, Colors, CursorVisualStyle, Dirty, RenderState, RowIterator,
};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::{RgbColor, StyleColor, Underline as EngineUnderline};

/// The viewport's changed rows, ready for die-core.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The viewport's width in cells.
    pub cols: u16,
    /// Its height.
    pub rows: u16,
    /// Every row is in `changed`: the first frame, and after a resize, a scroll, a color change
    /// (the theme's, or the program's: OSC 4, 10 and 11, reverse video with DECSCNM) or a full
    /// redraw by the program.
    pub full: bool,
    /// The rows that changed, by row from the viewport's top, top first. Each has `cols` cells.
    pub changed: Vec<(u16, Vec<Cell>)>,
    /// The cursor.
    pub cursor: TermCursor,
    /// The default background, which the area around the grid takes.
    pub background: Rgb,
    /// The default foreground.
    pub foreground: Rgb,
}

/// The terminal's cursor, as the program set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermCursor {
    /// Its cell, (row, column) from the viewport's top-left, when it's in the viewport. On a
    /// wide character it's the character's first cell.
    pub at: Option<(u16, u16)>,
    /// The shape the program asked for (DECSCUSR): a block, a bar, an underline, or hollow.
    pub shape: CursorShape,
    /// The program asked for a blinking cursor.
    pub blink: bool,
    /// The program shows it (DECTCEM).
    pub visible: bool,
    /// The color the program set (OSC 12), if any.
    pub color: Option<Rgb>,
}

pub(crate) struct Screen {
    state: RenderState<'static>,
    rows: RowIterator<'static>,
    cells: CellIterator<'static>,
    /// The next frame has every row.
    redraw: bool,
    cursor: Option<TermCursor>,
    /// The default colors the last frame was drawn in, (foreground, background): when they
    /// change, so does every cell drawn in them.
    drawn: Option<(Rgb, Rgb)>,
    buf: String,
}

pub(crate) fn rgb(c: RgbColor) -> Rgb {
    Rgb(c.r, c.g, c.b)
}

impl Screen {
    pub(crate) fn new() -> Result<Screen, libghostty_vt::Error> {
        Ok(Screen {
            state: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            redraw: true,
            cursor: None,
            drawn: None,
            buf: String::new(),
        })
    }

    /// Send every row at the next frame.
    pub(crate) fn redraw(&mut self) {
        self.redraw = true;
    }

    /// Start again from a fresh render state, whose first update reads every row. Another
    /// render state's update (copy state's text) consumes the terminal's dirty flags, so this
    /// one would miss what changed.
    pub(crate) fn rebuild(&mut self) {
        if let Ok(state) = RenderState::new() {
            self.state = state;
        }
        self.redraw = true;
    }

    pub(crate) fn frame(&mut self, term: &Terminal<'static, 'static>, full: bool) -> Option<Frame> {
        let Screen {
            state,
            rows,
            cells,
            redraw,
            cursor: last,
            drawn,
            buf,
        } = self;
        let snap = state.update(term).ok()?;
        let dirty = snap.dirty().ok()?;
        let colors = snap.colors().ok()?;
        let defaults = (rgb(colors.foreground), rgb(colors.background));
        let full = full || *redraw || dirty == Dirty::Full || *drawn != Some(defaults);
        let cursor = cursor_of(&snap)?;
        if !full && dirty == Dirty::Clean && *last == Some(cursor) {
            return None;
        }
        let (cols, height) = (snap.cols().ok()?, snap.rows().ok()?);
        let mut changed = Vec::new();
        {
            let mut it = rows.update(&snap).ok()?;
            let mut y = 0u16;
            loop {
                let row = if full {
                    let Some(row) = it.next() else { break };
                    y += 1;
                    (y - 1, row)
                } else {
                    let Some(row) = it.next_dirty() else { break };
                    row
                };
                let mut line = Vec::with_capacity(usize::from(cols));
                let Ok(mut ci) = cells.update(row.1) else {
                    continue;
                };
                while let Some(c) = ci.next() {
                    line.push(cell(c, &colors, buf));
                }
                line.resize(usize::from(cols), blank(&colors));
                changed.push((row.0, line));
            }
        }
        let _ = snap.clean();
        *redraw = false;
        *last = Some(cursor);
        *drawn = Some(defaults);
        Some(Frame {
            cols,
            rows: height,
            full,
            changed,
            cursor,
            background: defaults.1,
            foreground: defaults.0,
        })
    }
}

fn cursor_of(snap: &libghostty_vt::render::Snapshot<'_, '_>) -> Option<TermCursor> {
    let c = snap.cursor().ok()?;
    let shape = match c.visual_style {
        CursorVisualStyle::Bar => CursorShape::Bar,
        CursorVisualStyle::Underline => CursorShape::Underline,
        CursorVisualStyle::BlockHollow => CursorShape::Hollow,
        _ => CursorShape::Block,
    };
    Some(TermCursor {
        at: c
            .viewport
            .map(|v| (v.y, v.x.saturating_sub(u16::from(v.at_wide_tail)))),
        shape,
        blink: c.blinking,
        visible: c.visible,
        color: snap.cursor_color().ok().flatten().map(rgb),
    })
}

fn blank(colors: &Colors) -> Cell {
    Cell::new(
        Glyph::Char(' '),
        rgb(colors.foreground),
        rgb(colors.background),
    )
}

/// ghostel's dim: 65 % of the foreground and 35 % of the background.
fn faint(fg: Rgb, bg: Rgb) -> Rgb {
    let mix = |f: u8, b: u8| ((u16::from(f) * 166 + u16::from(b) * 90) / 256) as u8;
    Rgb(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2))
}

/// One engine cell as a die-core cell.
fn cell(c: &CellIteration<'_, '_>, colors: &Colors, buf: &mut String) -> Cell {
    let wide = c
        .raw_cell()
        .and_then(|r| r.wide())
        .unwrap_or(CellWide::Narrow);
    let mut glyph = match wide {
        CellWide::SpacerTail => Glyph::Continuation,
        CellWide::SpacerHead => Glyph::Char(' '),
        _ => glyph(c, buf),
    };
    let mut fg = c
        .fg_color()
        .ok()
        .flatten()
        .map_or(rgb(colors.foreground), rgb);
    let mut bg = c
        .bg_color()
        .ok()
        .flatten()
        .map_or(rgb(colors.background), rgb);
    let style = if c.has_styling().unwrap_or(false) {
        c.style().ok()
    } else {
        None
    };
    let Some(s) = style else {
        return Cell::new(glyph, fg, bg);
    };
    if s.faint {
        fg = faint(fg, bg);
    }
    if s.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if s.invisible && glyph != Glyph::Continuation {
        glyph = Glyph::Char(' ');
    }
    let mut out = Cell::new(glyph, fg, bg);
    out.bold = s.bold;
    out.italic = s.italic;
    out.strike = s.strikethrough;
    out.underline = match s.underline {
        EngineUnderline::None => Underline::None,
        EngineUnderline::Double => Underline::Double,
        EngineUnderline::Curly => Underline::Curly,
        EngineUnderline::Dotted => Underline::Dotted,
        EngineUnderline::Dashed => Underline::Dashed,
        _ => Underline::Single,
    };
    out.underline_color = match s.underline_color {
        StyleColor::None => fg,
        StyleColor::Rgb(c) => rgb(c),
        StyleColor::Palette(i) => rgb(colors.palette[usize::from(i.0)]),
    };
    out
}

fn glyph(c: &CellIteration<'_, '_>, buf: &mut String) -> Glyph {
    match c.graphemes_len().unwrap_or(0) {
        0 => Glyph::Char(' '),
        1 => {
            let mut one = ['\0'];
            match c.graphemes_buf(&mut one) {
                Ok(()) if one[0] != '\0' => Glyph::Char(one[0]),
                _ => Glyph::Char(' '),
            }
        }
        _ => match c.graphemes_utf8(buf) {
            Ok(()) => Glyph::Cluster(buf.as_str().into()),
            Err(_) => Glyph::Char(' '),
        },
    }
}
