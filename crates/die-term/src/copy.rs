//! Copy state's side of a terminal (scope §5.5, D19): the scrollback and screen as die-core's
//! [`SurfaceText`], and the snapshot's lines as the rows the viewport shows.
//!
//! Normal state starts with the whole text, at the viewport the wheel left in insert state if it
//! isn't the live screen. As output comes, an update reads only from the row that was the
//! screen's top at the last read, since rows above it are history and don't change, and says
//! how many of the oldest lines left the scrollback. A resize, a switch of
//! screens or a flood past what the marks can count reads the whole text again, and a switch of
//! screens says the lines are numbered afresh.
//!
//! The snapshot ends at the cursor's line, or at the last line with something on it below that:
//! the screen's blank rows past both aren't lines a motion goes to, so `G` and `j` stop at the
//! prompt, as evil-ghostel's do. Each row the program's line ran on from is said to be wrapped,
//! at the terminal's width, so a window keeps its place in that line when a resize reflows it.

use crate::Term;
use crate::text::{Mark, Text};
use die_core::SurfaceText;

/// What a terminal's copy state last read.
#[derive(Default)]
pub struct CopyText {
    /// The absolute row of the last text's first line.
    first: u64,
    /// The absolute row of the screen's top at the last read.
    screen_top: u64,
    mark: Option<Mark>,
    /// The last read was the alternate screen's.
    alt: bool,
}

/// `text`'s rows as lines, each ending in a newline, up to the cursor's line or the last one
/// drawn on, whichever is lower, with the rows wrapped at `cols` columns.
fn lines(text: &Text, cols: u16) -> SurfaceText {
    let drawn = text.lines.iter().rposition(|l| !l.text.is_empty());
    let keep = drawn.max(text.cursor.map(|c| c.0)).map_or(0, |l| l + 1);
    let rows = &text.lines[..keep];
    let mut s = String::with_capacity(rows.iter().map(|l| l.text.len() + 1).sum());
    for l in rows {
        s.push_str(&l.text);
        s.push('\n');
    }
    let mut st = SurfaceText::new(s);
    let wrapped = rows.iter().enumerate().filter(|(_, l)| l.wrapped);
    st.wrapped = wrapped.map(|(i, _)| (i, usize::from(cols))).collect();
    st
}

impl CopyText {
    /// The text for core: all of it, or with `update`, what changed since the last read.
    pub fn text(&mut self, term: &mut Term, update: bool) -> SurfaceText {
        let (first, cols) = (term.history.pruned(), term.size().0);
        let screen_top = first + term.viewport().history;
        let since = self
            .mark
            .filter(|_| update)
            .and_then(|m| term.added_since(m));
        let from = self
            .screen_top
            .checked_sub(first)
            .filter(|_| since.is_some());
        let mut st = match from {
            Some(from) => {
                let text = term.text_from(self.screen_top);
                let mut st = lines(&text, cols);
                st.cursor = text.cursor.map(|(l, b)| (l + from as usize, b));
                (st.dropped, st.from) = (first.saturating_sub(self.first) as usize, from as usize);
                self.mark = Some(text.mark);
                st
            }
            None => {
                let text = term.text();
                let mut st = lines(&text, cols);
                st.cursor = text.cursor;
                self.mark = Some(text.mark);
                st
            }
        };
        st.live_top = (screen_top - first) as usize;
        let shown = term.viewport();
        st.shown_top = (!shown.at_bottom).then(|| shown.top.saturating_sub(first) as usize);
        // Another screen's rows have nothing to do with the last one's.
        let alt = term.alt_screen();
        st.renumbered = std::mem::replace(&mut self.alt, alt) != alt;
        (self.first, self.screen_top) = (first, screen_top);
        st
    }

    /// Show copy state's line `top` on the viewport's top row, or the live screen with None.
    pub fn scroll(&self, term: &mut Term, top: Option<usize>) {
        let row = top.map(|t| self.first + t as u64);
        match row {
            Some(row) if row < term.history.pruned() + term.viewport().history => {
                term.scroll_to_row(row)
            }
            _ => term.scroll_to_bottom(),
        }
    }
}
