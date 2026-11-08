//! Copy state's text: the scrollback and the screen as lines, one per row (D19, scope §5.5).
//!
//! Each line knows its row and the column each of its characters is drawn in, so a position
//! in the text maps to a cell and back, with a wide character taking two columns. Rows are
//! absolute: they count every line that ever scrolled into history, so a row keeps its number
//! while newer output pushes it up and the oldest lines are dropped. A [`Mark`] says how many
//! lines were added since, so a snapshot can follow new output by reading only those.
//!
//! The rows are read through a render state of their own, with overscan over the whole
//! scrollback. Its update consumes the terminal's dirty flags, so the frame's render state is
//! rebuilt after it.

use crate::Term;
use libghostty_vt::render::{CellIterator, Overscan, RenderState, RowIterator};
use libghostty_vt::screen::{CellWide, Screen, TrackedGridRef};
use libghostty_vt::terminal::{Point, PointCoordinate, PointSpace};
use libghostty_vt::{Error, Terminal};

/// A point in the terminal's output, to count the lines added after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    epoch: u64,
    pushed: u64,
}

/// One row as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The row's absolute number: rows ever scrolled away above it, plus its place.
    pub row: u64,
    /// The row's text, without its trailing blank cells (the cursor's row keeps those up to
    /// the cursor).
    pub text: String,
    /// The row continues on the next one: the program's line was soft-wrapped here.
    pub wrapped: bool,
    /// Each drawn character's byte offset in `text` and its first column, in order.
    cells: Vec<(usize, u16)>,
    /// The column after the last character's.
    end: u16,
}

impl Line {
    /// The column the character at byte `at` starts in. Past the end, the column after the
    /// last character.
    pub fn col(&self, at: usize) -> u16 {
        if at >= self.text.len() {
            return self.end;
        }
        let i = self.cells.partition_point(|c| c.0 <= at);
        self.cells.get(i.wrapping_sub(1)).map_or(0, |c| c.1)
    }

    /// The byte offset of the character drawn in column `col`; a wide character's second
    /// column gives the character. Past the end, the text's length.
    pub fn at(&self, col: u16) -> usize {
        if col >= self.end {
            return self.text.len();
        }
        let i = self.cells.partition_point(|c| c.1 <= col);
        self.cells.get(i.wrapping_sub(1)).map_or(0, |c| c.0)
    }

    /// The columns the row's text takes.
    pub fn width(&self) -> u16 {
        self.end
    }
}

/// The scrollback and screen as text, for copy state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// The rows, oldest first.
    pub lines: Vec<Line>,
    /// The cursor's line, as an index into `lines`, and byte in it, when it's among them.
    pub cursor: Option<(usize, usize)>,
    /// The point this text was read at.
    pub mark: Mark,
    /// The text is the alternate screen's, which has no scrollback.
    pub alt_screen: bool,
}

/// How many rows went into history and were dropped from its top, followed through a tracked
/// reference to the active screen's top row, moved there after each feed.
pub(crate) struct History {
    anchor: Option<TrackedGridRef>,
    /// The anchor's screen row when it was set: the history's length then.
    anchor_y: u64,
    pushed: u64,
    pruned: u64,
    /// Counts the times the counts started again: a resize, a switch of screens, an anchor lost.
    epoch: u64,
    alt: bool,
}

fn history_rows(term: &Terminal<'_, '_>) -> u64 {
    term.scrollback_rows().unwrap_or(0) as u64
}

impl History {
    pub(crate) fn new(term: &Terminal<'_, '_>) -> Result<History, Error> {
        Ok(History {
            anchor: Some(term.track_grid_ref(Point::Active(PointCoordinate { x: 0, y: 0 }))?),
            anchor_y: history_rows(term),
            pushed: 0,
            pruned: 0,
            epoch: 0,
            alt: false,
        })
    }

    pub(crate) fn pruned(&self) -> u64 {
        self.pruned
    }

    fn mark(&self) -> Mark {
        Mark {
            epoch: self.epoch,
            pushed: self.pushed,
        }
    }

    /// After the terminal changed: count what moved, and move the anchor to the top again.
    pub(crate) fn update(&mut self, term: &mut Terminal<'static, 'static>) {
        let alt = matches!(term.active_screen(), Ok(Screen::Alternate));
        if alt != self.alt {
            self.alt = alt;
            return self.restart(term);
        }
        if alt {
            return;
        }
        let now = history_rows(term);
        let at = self
            .anchor
            .as_ref()
            .and_then(|a| a.point(PointSpace::Screen).ok().flatten())
            .map(|p| u64::from(p.y));
        match at {
            Some(y) if y <= self.anchor_y && y <= now => {
                self.pushed += now - y;
                self.pruned += self.anchor_y - y;
                self.reanchor(term);
            }
            _ => self.restart(term),
        }
    }

    /// The counts start again: marks from before no longer count.
    pub(crate) fn restart(&mut self, term: &mut Terminal<'static, 'static>) {
        self.epoch += 1;
        self.reanchor(term);
    }

    fn reanchor(&mut self, term: &mut Terminal<'static, 'static>) {
        let top = Point::Active(PointCoordinate { x: 0, y: 0 });
        let moved = match self.anchor.as_mut() {
            Some(a) => a.set(term, top).is_ok(),
            None => false,
        };
        if !moved {
            self.anchor = term.track_grid_ref(top).ok();
        }
        self.anchor_y = history_rows(term);
    }
}

impl Term {
    /// A mark at the output as it is now.
    pub fn mark(&self) -> Mark {
        self.history.mark()
    }

    /// How many lines went into the scrollback since `mark`, or None when that can't be told:
    /// after a resize, a switch to or from the alternate screen, or a flood that dropped the row
    /// the count follows. Then read the whole text again.
    pub fn added_since(&self, mark: Mark) -> Option<u64> {
        (mark.epoch == self.history.epoch).then(|| self.history.pushed - mark.pushed)
    }

    /// The whole scrollback and screen as text.
    pub fn text(&mut self) -> Text {
        self.text_from(0)
    }

    /// The rows from absolute row `row` to the end, as text: what a snapshot reads to follow
    /// new output, from the first row that may have changed.
    pub fn text_from(&mut self, row: u64) -> Text {
        let pruned = self.history.pruned;
        let first = row.saturating_sub(pruned);
        let mut lines = capture(&self.term, first, pruned).unwrap_or_default();
        self.screen.rebuild();
        let cols = self.size().0;
        let cursor_row =
            pruned + history_rows(&self.term) + u64::from(self.term.cursor_y().unwrap_or(0));
        let cursor_col = self
            .term
            .cursor_x()
            .unwrap_or(0)
            .min(cols.saturating_sub(1));
        let cursor = lines.iter().position(|l| l.row == cursor_row).map(|i| {
            let line = &mut lines[i];
            // The cursor's row keeps its blanks through the cursor's, so the cursor has a place.
            for c in line.end..=cursor_col {
                line.cells.push((line.text.len(), c));
                line.text.push(' ');
                line.end = c + 1;
            }
            (i, line.at(cursor_col))
        });
        Text {
            lines,
            cursor,
            mark: self.history.mark(),
            alt_screen: self.alt_screen(),
        }
    }
}

/// Read screen rows `first..` as lines numbered from `pruned`.
fn capture(term: &Terminal<'static, 'static>, first: u64, pruned: u64) -> Result<Vec<Line>, Error> {
    let bar = term.scrollbar()?;
    let (top, len, total) = (bar.offset, bar.len, bar.total);
    let clamp = |n: u64| u16::try_from(n).unwrap_or(u16::MAX);
    let mut state = RenderState::new()?;
    state.set_overscan(Overscan {
        above: clamp(top.saturating_sub(first)),
        below: clamp(total.saturating_sub(top + len)),
    })?;
    let snap = state.update(term)?;
    let mut rows = RowIterator::new()?;
    let mut cells = CellIterator::new()?;
    let mut it = rows.update(&snap)?;
    let mut lines = Vec::new();
    let mut buf = String::new();
    while let Some(row) = it.next() {
        let y = top as i64 + i64::from(row.viewport_y()?);
        if y < first as i64 {
            continue;
        }
        let raw = row.raw_row()?;
        let mut line = Line {
            row: pruned + y as u64,
            text: String::new(),
            wrapped: raw.is_wrapped().unwrap_or(false),
            cells: Vec::new(),
            end: 0,
        };
        let mut end_byte = 0;
        let mut add = |line: &mut Line, col: u16, wide: CellWide, s: Option<&str>| {
            let width = match wide {
                CellWide::SpacerTail | CellWide::SpacerHead => return,
                CellWide::Wide => 2,
                _ => 1,
            };
            line.cells.push((line.text.len(), col));
            match s.filter(|s| !s.is_empty()) {
                Some(s) => line.text.push_str(s),
                None => line.text.push(' '),
            }
            // Trailing blanks go: the text ends at the last cell drawn with something.
            if !s.is_none_or(|s| s.trim_start().is_empty()) {
                end_byte = line.text.len();
                line.end = col + width;
            }
        };
        if raw.has_grapheme_cluster().unwrap_or(true) {
            let mut ci = cells.update(row)?;
            let mut col = 0;
            while let Some(c) = ci.next() {
                let wide = c
                    .raw_cell()
                    .and_then(|r| r.wide())
                    .unwrap_or(CellWide::Narrow);
                let ok = c.graphemes_utf8(&mut buf).is_ok();
                add(&mut line, col, wide, ok.then_some(buf.as_str()));
                col += 1;
            }
        } else {
            // One codepoint a cell at most: read the row's cells at once, and ask a cell's width
            // only when it can be wide (nothing below U+1100 is). A wide character's second
            // cell is its spacer.
            let mut one = [0u8; 4];
            let mut tail = false;
            for (col, c) in (0..).zip(row.cells_raw()?) {
                if std::mem::take(&mut tail) {
                    continue;
                }
                let cp = c.codepoint().unwrap_or(0);
                let wide = match cp {
                    0x1100.. => c.wide().unwrap_or(CellWide::Narrow),
                    _ => CellWide::Narrow,
                };
                tail = wide == CellWide::Wide;
                let ch = char::from_u32(cp).filter(|&c| c != '\0');
                add(
                    &mut line,
                    col,
                    wide,
                    ch.map(|ch| &*ch.encode_utf8(&mut one)),
                );
            }
        }
        line.text.truncate(end_byte);
        let keep = line.cells.partition_point(|c| c.0 < end_byte);
        line.cells.truncate(keep);
        lines.push(line);
    }
    Ok(lines)
}
