//! A buffer's per-window views (C10): each window's selection, scroll position and size.
#![warn(missing_docs)]
use crate::ids::*;
use crate::text::*;

/// Per-window state of a text buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    pub sel: Selection,
    pub top: usize,
    pub top_row: usize,
    pub cols: usize,
    pub rows: usize,
}

impl TextBuf {
    /// Keep every cursor on a visible position and every scroll top on a display-line start.
    pub(crate) fn normalize_views(&mut self) {
        let wins: Vec<WindowId> = self.views.keys().copied().collect();
        for w in wins {
            let v = self.views[&w];
            let (a, h, t) = (
                self.snap(v.sel.anchor, None),
                self.snap(v.sel.head, None),
                self.dline_start(v.top),
            );
            let v = self.views.get_mut(&w).unwrap();
            v.sel.anchor = a;
            v.sel.head = h;
            if t != v.top {
                v.top = t;
                v.top_row = 0;
            }
        }
    }
    /// Move every view through an edit. Text inserted at a cursor goes after it in the acting
    /// window and before it in every other window, as Emacs's window points do. One gravity for
    /// both ends keeps a collapsed cursor collapsed.
    pub(crate) fn map_views(&mut self, m: impl Fn(usize, Gravity) -> usize) {
        let actor = self.actor;
        for (w, v) in self.views.iter_mut() {
            let g = if Some(*w) == actor {
                Gravity::Right
            } else {
                Gravity::Left
            };
            v.sel.anchor = m(v.sel.anchor, g);
            v.sel.head = m(v.sel.head, g);
            v.top = m(v.top, Gravity::Left);
        }
    }

    pub(crate) fn add_view(&mut self, w: WindowId, cols: usize, rows: usize) {
        let p = self.snap(0, None);
        self.views.insert(
            w,
            View {
                sel: Selection::cursor(p),
                top: 0,
                top_row: 0,
                cols,
                rows,
            },
        );
    }
    pub(crate) fn resize_view(&mut self, w: WindowId, cols: usize, rows: usize) {
        if let Some(v) = self.views.get_mut(&w) {
            v.cols = cols;
            v.rows = rows;
        }
    }
    /// A window's selection, or None if it doesn't show this buffer.
    pub fn selection(&self, w: WindowId) -> Option<Selection> {
        self.views.get(&w).map(|v| v.sel)
    }
    /// A window's cursor, its selection's head, or None if it doesn't show this buffer.
    pub fn cursor(&self, w: WindowId) -> Option<usize> {
        self.views.get(&w).map(|v| v.sel.head)
    }
    /// Set a selection. Positions inside hidden text are moved to a visible one.
    pub fn set_selection(&mut self, w: WindowId, mut sel: Selection) -> Option<Selection> {
        sel.anchor = self.snap(sel.anchor, None);
        sel.head = self.snap(sel.head, None);
        let v = self.views.get_mut(&w)?;
        v.sel = sel;
        Some(sel)
    }
    /// Set a window's selection to a cursor at `pos`, moved to a visible position, and return
    /// where it went.
    pub fn set_cursor(&mut self, w: WindowId, pos: usize) -> Option<usize> {
        self.set_selection(w, Selection::cursor(pos))
            .map(|s| s.head)
    }
}
