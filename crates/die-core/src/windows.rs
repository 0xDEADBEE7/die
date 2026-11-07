//! Windows (K6, contract §5): each buffer's per-window views (C10), and each window's place in the
//! split tree, with focus, split, close, only and resize.
#![warn(missing_docs)]
use crate::error::*;
use crate::ext::*;
use crate::ids::*;
use crate::scene::WindowPlacement;
use crate::text::*;
use crate::workspace::Side;
use crate::workspace::tree::{Axis, Rect};
use std::any::TypeId;

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
        let fixed = |t: &TextBuf, mut v: View| {
            v.sel.anchor = t.snap(v.sel.anchor, None);
            v.sel.head = t.snap(v.sel.head, None);
            let top = t.dline_start(v.top);
            if top != v.top {
                v.top = top;
                v.top_row = 0;
            }
            v
        };
        let wins: Vec<WindowId> = self.views.keys().copied().collect();
        for w in wins {
            let v = fixed(self, self.views[&w]);
            self.views.insert(w, v);
        }
        let wins: Vec<WindowId> = self.parked.keys().copied().collect();
        for w in wins {
            let v = fixed(self, self.parked[&w]);
            self.parked.insert(w, v);
        }
        self.rest = self.rest.map(|v| fixed(self, v));
    }
    /// Move every view through an edit. Text inserted at a cursor goes after it in the acting
    /// window and before it in every other window, as Emacs's window points do. One gravity for
    /// both ends keeps a collapsed cursor collapsed.
    pub(crate) fn map_views(&mut self, m: impl Fn(usize, Gravity) -> usize) {
        let actor = self.actor;
        let out_of_sight = self.parked.values_mut().chain(self.rest.as_mut());
        for v in out_of_sight {
            v.sel.anchor = m(v.sel.anchor, Gravity::Left);
            v.sel.head = m(v.sel.head, Gravity::Left);
            v.top = m(v.top, Gravity::Left);
        }
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
    /// A new window `new` starts as `w` is, as a split does.
    pub(crate) fn copy_view(&mut self, w: WindowId, new: WindowId) {
        if let Some(v) = self.views.get(&w).copied() {
            self.views.insert(new, v);
        }
    }
    /// A window stops showing this buffer. Its view waits for it.
    pub(crate) fn park_view(&mut self, w: WindowId) {
        if let Some(v) = self.views.remove(&w) {
            self.parked.insert(w, v);
            self.rest = Some(v);
        }
    }
    /// A window shows this buffer: from where it left it, or else where `from` is, or else where
    /// the last window to leave did, or else at the start.
    pub(crate) fn unpark_view(
        &mut self,
        w: WindowId,
        from: Option<WindowId>,
        cols: usize,
        rows: usize,
    ) {
        let from = from.and_then(|f| self.views.get(&f).copied());
        match self.parked.remove(&w).or(from).or(self.rest) {
            Some(v) => {
                self.views.insert(w, View { cols, rows, ..v });
            }
            None => self.add_view(w, cols, rows),
        }
    }
    /// A window has closed. If it was showing this buffer, it's the last to leave. If it had left
    /// already, its view just goes.
    pub(crate) fn drop_view(&mut self, w: WindowId) {
        self.parked.remove(&w);
        if let Some(v) = self.views.remove(&w) {
            self.rest = Some(v);
        }
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

/// A window: what it shows, its size in cells, and where it sits. A window in the split tree
/// gets these from it. One that isn't (a second [`Ctx::open_window`]) keeps what it was given.
pub(crate) struct Window {
    pub buffer: BufferId,
    pub cols: usize,
    pub rows: usize,
    /// Its cells in the frame, from the last tiling. None if it's not in the tree.
    pub rect: Option<Rect>,
    pub used: u64,
}

impl Ctx {
    pub(crate) fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// The window a running command acts for, or else the focused one.
    pub(crate) fn current_window(&self) -> Option<WindowId> {
        let live = self.cur_window.filter(|w| self.windows.contains_key(*w));
        live.or(self.focused)
    }

    /// The window showing `buf` that was used last.
    pub(crate) fn window_used_last(&self, buf: BufferId) -> Option<WindowId> {
        let showing = self.windows.iter().filter(|(_, w)| w.buffer == buf);
        showing.max_by_key(|(_, w)| w.used).map(|(id, _)| id)
    }

    /// The windows showing `buf`.
    pub(crate) fn windows_showing(&self, buf: BufferId) -> Vec<WindowId> {
        let showing = self.windows.iter().filter(|(_, w)| w.buffer == buf);
        showing.map(|(id, _)| id).collect()
    }

    /// Open a window on a buffer and focus it, with no [`Event::Focus`]: a surface knows it's being
    /// shown. The first window in a workspace is the split tree's root, and fills the frame:
    /// without [`Ctx::set_frame_size`], this size says what the frame is. Any other stands outside
    /// the tree, at this size, and only [`Ctx::compose`] draws it, with the placement it's given.
    /// So it takes the focus only without a frame size, from a host that places windows itself:
    /// once the host has set one, it draws with [`Ctx::scene`], and the focus stays in the tree
    /// unless [`Ctx::focus`] moves it. Windows join the tree through [`Ctx::split_window`] and
    /// [`Ctx::show`].
    pub fn open_window(&mut self, buf: BufferId, cols: usize, rows: usize) -> Result<WindowId> {
        self.buffers.get(buf).ok_or(Checkout::Gone)?;
        let used = self.next_tick();
        let w = self.windows.insert(Window {
            buffer: buf,
            cols,
            rows,
            rect: None,
            used,
        });
        let b = &mut self.buffers[buf];
        b.used = used;
        if let Content::Text(t) = &mut b.content {
            t.add_view(w, cols, rows);
        }
        let first = self.tree.is_empty();
        if first {
            self.tree.set_root(w);
            if !self.tree.fixed {
                self.tree.frame = (cols, rows + 2);
            }
            self.retile();
        }
        if first || !self.tree.fixed {
            self.take_focus(w);
        }
        Ok(w)
    }

    /// Put a window on a buffer that's alive. Its view of the old one stays, out of sight, so
    /// going back finds the same cursor and scroll position. With none of its own in the new
    /// one, it starts where the window used last on that buffer is.
    pub(crate) fn set_window_buffer(&mut self, w: WindowId, buf: BufferId) {
        let old = self.windows[w].buffer;
        if old == buf {
            return;
        }
        let focused = self.focused == Some(w);
        if focused {
            self.focus_event(old, false);
        }
        if let Some(Content::Text(t)) = self.buffers.get_mut(old).map(|b| &mut b.content) {
            t.park_view(w);
        }
        let from = self.window_used_last(buf);
        let used = self.next_tick();
        let win = &mut self.windows[w];
        win.buffer = buf;
        let (cols, rows) = (win.cols, win.rows);
        let b = &mut self.buffers[buf];
        b.used = used;
        if let Content::Text(t) = &mut b.content {
            t.unpark_view(w, from, cols, rows);
        }
        self.open_messages_at_end(w, buf);
        if focused {
            self.focus_event(buf, true);
        }
        self.size_events(w, buf, cols, rows);
    }

    /// Focus a window. A running command goes on acting for it (contract §9). A surface in a
    /// window that gains or loses the focus gets [`Event::Focus`], queued (C3).
    pub fn focus(&mut self, w: WindowId) {
        if !self.windows.contains_key(w) {
            return;
        }
        let was = self.focused.filter(|f| *f != w);
        if self.focused != Some(w) {
            let old = was.and_then(|f| self.windows.get(f)).map(|f| f.buffer);
            if let Some(old) = old {
                self.focus_event(old, false);
            }
            self.focus_event(self.windows[w].buffer, true);
        }
        self.take_focus(w);
    }
    fn take_focus(&mut self, w: WindowId) {
        self.focused = Some(w);
        // A running command goes on acting for the window it moved the focus to.
        self.cur_window = self.cur_window.and(Some(w));
        let used = self.next_tick();
        let win = &mut self.windows[w];
        win.used = used;
        let buf = win.buffer;
        if let Some(b) = self.buffers.get_mut(buf) {
            b.used = used;
        }
    }
    /// Tell a surface it gained or lost the focus (C3). While DIE's OS window doesn't have it, no
    /// surface does, so there's nothing to tell: [`Input::Focus`] tells the one focused by then.
    pub(crate) fn focus_event(&mut self, buf: BufferId, gained: bool) {
        if !self.app_focused {
            return;
        }
        if let Some(Content::Custom { owner, .. }) = self.buffers.get(buf).map(|b| &b.content) {
            let owner = *owner;
            self.defer_as(owner, move |ctx| {
                ctx.checkout_surface(buf, |s, ctx| s.event(&Event::Focus(gained), ctx))?;
                Ok(())
            });
        }
    }
    /// The focused window.
    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }
    /// The buffer the focused window shows.
    pub fn focused_buffer(&self) -> Option<BufferId> {
        self.focused
            .and_then(|w| self.windows.get(w))
            .map(|w| w.buffer)
    }
    /// The buffer a window shows.
    pub fn window_buffer(&self, w: WindowId) -> Option<BufferId> {
        self.windows.get(w).map(|w| w.buffer)
    }
    /// A window's columns and rows.
    pub fn window_size(&self, w: WindowId) -> Option<(usize, usize)> {
        self.windows.get(w).map(|w| (w.cols, w.rows))
    }
    /// The windows in the split tree, left to right and top to bottom within each split.
    pub fn window_list(&self) -> Vec<WindowId> {
        self.tree.windows()
    }

    /// The windows in the split tree, the one used last first.
    pub fn windows_by_use(&self) -> Vec<WindowId> {
        let mut all = self.tree.windows();
        all.sort_by_key(|w| std::cmp::Reverse(self.windows.get(*w).map(|w| w.used)));
        all
    }

    /// Change a window's size. In the split tree, this moves its borders as far as they'll go,
    /// and the windows that gain or lose cells change with it. A sole window in a frame the
    /// host hasn't set takes the size, and the frame follows it. A window outside the tree just
    /// takes it. Each mode's `on_resize` and the surface's Resize event are queued (C3).
    pub fn resize(&mut self, w: WindowId, cols: usize, rows: usize) -> Result {
        let win = self.windows.get(w).ok_or(Checkout::Gone)?;
        let (had_cols, had_rows) = (win.cols, win.rows);
        if self.tree.contains(w) {
            if !self.tree.is_sole(w) {
                let by = |new: usize, had: usize| new as isize - had as isize;
                self.tree.resize(w, by(cols, had_cols), by(rows, had_rows));
                self.retile();
                return Ok(());
            }
            if self.tree.fixed {
                return Ok(());
            }
            self.tree.frame = (cols, rows + 2);
        }
        self.set_size(w, cols, rows);
        self.retile();
        Ok(())
    }
    fn set_size(&mut self, w: WindowId, cols: usize, rows: usize) {
        let win = &mut self.windows[w];
        win.cols = cols;
        win.rows = rows;
        let buf = win.buffer;
        self.size_events(w, buf, cols, rows);
    }
    /// A window showing `buf` has this size now: tell the view, the surface and the modes (C3).
    fn size_events(&mut self, w: WindowId, buf: BufferId, cols: usize, rows: usize) {
        let Some(b) = self.buffers.get_mut(buf) else {
            return;
        };
        match &mut b.content {
            Content::Text(t) => t.resize_view(w, cols, rows),
            Content::Custom { owner, .. } => {
                let owner = *owner;
                self.defer_as(owner, move |ctx| {
                    let ev = Event::Resize { cols, rows };
                    ctx.checkout_surface(buf, |s, ctx| s.event(&ev, ctx))?;
                    Ok(())
                });
            }
        }
        let modes: Vec<(TypeId, ExtIdx)> = self.buffers[buf]
            .modes
            .iter()
            .map(|m| (m.type_id, self.modes[m.spec].owner))
            .collect();
        for (tid, owner) in modes {
            self.defer_as(owner, move |ctx| {
                ctx.checkout_mode(buf, tid, |m, ctx| m.on_resize(w, cols, rows, ctx))?;
                Ok(())
            });
        }
    }

    // -------------------------------------------------------------- the frame and the split tree

    /// Set the frame's size in cells, with the command line's row. The windows in the split tree
    /// divide what's above that row. In a frame too small for them, some get no cells.
    pub fn set_frame_size(&mut self, cols: usize, rows: usize) {
        self.tree.frame = (cols, rows);
        self.tree.fixed = true;
        self.retile();
    }
    /// The frame's size in cells, with the command line's row.
    pub fn frame_size(&self) -> (usize, usize) {
        self.tree.frame
    }
    /// Where the split tree's windows go, for [`Ctx::compose`]. One with no cells isn't listed.
    pub fn placements(&self) -> Vec<WindowPlacement> {
        let placed = self.tree.windows().into_iter();
        placed
            .filter_map(|window| {
                let w = self.windows.get(window)?;
                let r = w.rect?;
                (w.cols > 0 && w.rows > 0).then_some(WindowPlacement {
                    window,
                    col: r.x,
                    row: r.y,
                })
            })
            .collect()
    }
    /// Give every window in the tree the cells it now has.
    pub(crate) fn retile(&mut self) {
        let frame_cols = self.tree.frame.0;
        for (w, r) in self.tree.tile() {
            let (cols, rows) = r.text_size(frame_cols);
            let win = &mut self.windows[w];
            win.rect = Some(r);
            if (win.cols, win.rows) != (cols, rows) {
                self.set_size(w, cols, rows);
            }
        }
    }
    fn in_tree(&self, w: WindowId) -> Result<Rect> {
        let win = self.windows.get(w).ok_or(Checkout::Gone)?;
        win.rect
            .ok_or_else(|| Error::Msg("that window isn't in the split tree".into()))
    }

    /// Divide a window's cells with a new window at `side` of it, showing the same buffer with
    /// the same cursor and scroll position, as Emacs's `split-window` does. The new window gets
    /// half, and the focus stays where it is. An error if it's too small to divide.
    pub fn split_window(&mut self, w: WindowId, side: Side) -> Result<WindowId> {
        let rect = self.in_tree(w)?;
        let buf = self.windows[w].buffer;
        let new = self.windows.insert(Window {
            buffer: buf,
            cols: 0,
            rows: 0,
            rect: Some(rect),
            used: 0, // not used yet, so closing a window doesn't focus it over one that was
        });
        if !self.tree.split(w, new, side, rect) {
            self.windows.remove(new);
            return Err(Error::Msg("That window is too small to split".into()));
        }
        if let Some(Content::Text(t)) = self.buffers.get_mut(buf).map(|b| &mut b.content) {
            t.copy_view(w, new);
        }
        self.retile();
        Ok(new)
    }

    /// Close a window, and give its cells to its neighbour. The focus goes to the window in the
    /// tree used last. An error for the only window in the tree, as in Emacs.
    pub fn close_window(&mut self, w: WindowId) -> Result {
        self.windows.get(w).ok_or(Checkout::Gone)?;
        if self.tree.is_sole(w) {
            return Err(Error::Msg("That's the only window".into()));
        }
        self.tree.remove(w);
        self.drop_window(w);
        self.retile();
        Ok(())
    }

    /// [`Ctx::close_window`], then even out the split it was in if that still stands, as Evil's
    /// `C-w c` does. A split of two goes with the window, so nothing evens out then.
    pub fn close_window_balancing(&mut self, w: WindowId) -> Result {
        self.windows.get(w).ok_or(Checkout::Gone)?;
        if !self.tree.remove_balancing(w) {
            return Err(Error::Msg("That's the only window".into()));
        }
        self.drop_window(w);
        self.retile();
        Ok(())
    }

    /// Give two windows in the tree each other's places and sizes. The focus stays on its window.
    pub fn swap_windows(&mut self, a: WindowId, b: WindowId) -> Result {
        self.in_tree(a)?;
        self.in_tree(b)?;
        self.tree.swap(a, b);
        self.retile();
        Ok(())
    }

    /// Give two windows in the tree each other's places, each keeping the size of its text where
    /// that fits, as Emacs's `window-swap-states` with a size does: on each axis, a window whose
    /// whole size doesn't fit takes its place's. Evil's `C-w x`. The focus stays on its window.
    pub fn exchange_windows(&mut self, a: WindowId, b: WindowId) -> Result {
        self.in_tree(a)?;
        self.in_tree(b)?;
        let (had_a, had_b) = (self.window_size(a), self.window_size(b));
        self.tree.swap(a, b);
        self.retile();
        for axis in [Axis::Cols, Axis::Rows] {
            let len = |s: Option<(usize, usize)>| {
                let (cols, rows) = s.unwrap_or_default();
                (if axis == Axis::Cols { cols } else { rows }) as isize
            };
            let by = len(had_b) - len(self.window_size(b));
            if !self.tree.trade(b, a, axis, by) {
                self.tree.resize_whole(b, axis, by);
                self.retile();
                let by = len(had_a) - len(self.window_size(a));
                self.tree.resize_whole(a, axis, by);
            }
            self.retile();
        }
        Ok(())
    }

    /// Move a window in the tree to run the whole length of the frame's `side`, the others keeping
    /// their shape, and even out the frame: Evil's `C-w H`, `J`, `K` and `L`.
    pub fn move_window(&mut self, w: WindowId, side: Side) -> Result {
        self.in_tree(w)?;
        self.tree.move_to_edge(w, side);
        self.balance_frame();
        Ok(())
    }

    /// Make a window the only one in the tree, and focus it. Windows outside the tree stay.
    pub fn only_window(&mut self, w: WindowId) -> Result {
        self.in_tree(w)?;
        let gone = self.tree.only(w);
        self.focus(w);
        for g in gone {
            self.drop_window(g);
        }
        self.retile();
        Ok(())
    }

    /// A window has left the tree: forget it, and move the focus off it.
    pub(crate) fn drop_window(&mut self, w: WindowId) {
        let Some(win) = self.windows.remove(w) else {
            return;
        };
        for b in self.buffers.values_mut() {
            if let Content::Text(t) = &mut b.content {
                t.drop_view(w);
            }
        }
        if self.focused == Some(w) {
            self.focus_event(win.buffer, false);
            self.focused = None;
            let used = |id: &WindowId| self.windows.get(*id).map(|w| w.used);
            let next = self.tree.windows().into_iter().max_by_key(used);
            match next {
                Some(next) => self.focus(next),
                None => self.cur_window = None,
            }
        }
        if self.cur_window == Some(w) {
            self.cur_window = self.focused;
        }
    }

    /// Even out the split holding a window, and the splits inside it: Emacs's `balance-windows`
    /// on the window's parent, as Evil does after each split and close.
    pub fn balance_windows(&mut self, w: WindowId) {
        self.tree.balance(w);
        self.retile();
    }

    /// Even out the whole frame: Emacs's `balance-windows` with no window, Evil's `C-w =`.
    pub fn balance_frame(&mut self) {
        self.tree.balance_all();
        self.retile();
    }

    /// The window at `side` of `w`, on the row or column of its cursor, as Emacs's
    /// `window-in-direction` picks. None at the frame's edge, or for a window outside the tree.
    pub fn window_in_direction(&mut self, w: WindowId, side: Side) -> Option<WindowId> {
        let r = self.windows.get(w)?.rect?;
        let (row, col) = self.cursor_cell(w, r);
        let at = match side {
            Side::Left | Side::Right => row,
            Side::Above | Side::Below => col,
        };
        let others = self.tree.windows().into_iter().filter(|o| *o != w);
        let hit = others
            .filter_map(|o| Some((o, self.windows.get(o)?.rect?)))
            .find(|(_, o)| r.touches(o, side, at));
        hit.map(|(o, _)| o)
    }
    /// Focus the window at `side` of the current one, and return it.
    pub fn focus_direction(&mut self, side: Side) -> Option<WindowId> {
        let w = self.current_window()?;
        let next = self.window_in_direction(w, side)?;
        self.focus(next);
        Some(next)
    }
    /// The frame cell a window's cursor is on, or its first cell if it hasn't one on screen.
    fn cursor_cell(&mut self, w: WindowId, r: Rect) -> (usize, usize) {
        let buf = self.windows[w].buffer;
        let Some(Content::Text(t)) = self.buffers.get_mut(buf).map(|b| &mut b.content) else {
            return (r.y, r.x);
        };
        t.scroll_to_cursor(w);
        let at = t.cursor(w).and_then(|c| t.screen_pos(w, c));
        at.map_or((r.y, r.x), |(row, col)| {
            let col = (r.x + col).min((r.x + r.w).saturating_sub(1));
            (r.y + row, col)
        })
    }
}
