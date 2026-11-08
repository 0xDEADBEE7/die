//! Copy state (scope §5.5, D19): a surface's text as a read-only snapshot, which die-vim's
//! normal and visual grammar runs over through the text kind's keymap, while the window goes on
//! showing the surface's own cells at the snapshot's scroll position, with the cursor, the
//! selection and the search's matches drawn over them. A window whose cursor is on the
//! surface's cursor line or below it, in normal state, follows new output, as Evil's point
//! follows a terminal's cursor; one that went up stays where it is until it comes back down.

use crate::ext::*;
use crate::ids::*;
use crate::keymap::State;
use crate::layout::CellKind;
use crate::scene::{CursorShape, Scene, WindowPlacement, selected};
use crate::text::*;
use crate::windows::View;

/// What copy state reads of a surface ([`Surface::text`]): lines, each ending in a newline,
/// oldest first. An update replaces the last text's lines from `from` to its end, after
/// `dropped` lines left its start. The whole text is an update from line 0 with none dropped.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceText {
    /// The lines, from `from` on.
    pub text: String,
    /// Lines that left the last text's start, as a terminal's oldest scrollback does.
    pub dropped: usize,
    /// The first line `text` replaces, counted after `dropped` went.
    pub from: usize,
    /// The surface's own cursor, as a line and a byte in it: where copy state starts, and what
    /// it follows. With none, the last line is.
    pub cursor: Option<(usize, usize)>,
    /// The line its live view shows on the top row. Copy state scrolls no lower.
    pub live_top: usize,
}

impl SurfaceText {
    /// The whole text: these lines, with no cursor, the live view at the first.
    pub fn new(text: String) -> SurfaceText {
        SurfaceText {
            text,
            ..SurfaceText::default()
        }
    }
}

/// A surface buffer's copy state.
pub(crate) struct Copy {
    pub(crate) text: TextBuf,
    /// The surface's cursor as a position, as of the last read.
    cursor: usize,
    /// The surface said its text changed since the last read.
    stale: bool,
}

/// Where a window was in a snapshot, to put it back after an update.
struct Place {
    w: WindowId,
    /// The cursor and the selection's other end, each as a line and a display column.
    head: (usize, usize),
    anchor: (usize, usize),
    sel: Selection,
    top: usize,
    /// On the surface's cursor line or below it, in normal state: the lines below that line.
    below: Option<usize>,
    /// On the surface's cursor.
    on_cursor: bool,
    /// Lines from its top to the live view's.
    above_live: usize,
}

/// The line a cursor can go to last: the one before the empty line after a final newline.
fn last_line(t: &TextBuf) -> usize {
    let ends = !t.is_empty() && t.rope().byte(t.len() - 1) == b'\n';
    t.line_count().saturating_sub(1 + usize::from(ends))
}

impl Copy {
    /// Take in what the surface said: its text from `from`, its cursor and its live view.
    fn take(&mut self, st: &SurfaceText) {
        let t = &mut self.text;
        let cut = t.line_start(st.dropped);
        let from = t.line_start(st.dropped + st.from);
        t.splice(from..t.len(), &st.text);
        t.splice(0..cut, "");
        let (line, byte) = st.cursor.unwrap_or((last_line(t), 0));
        let at = (t.line_start(line) + byte).min(t.len());
        self.cursor = t.snap(at, None);
        let live = t.line_start(st.live_top.min(last_line(t)));
        t.top_limit = Some(live);
    }
    fn live_top(&self) -> usize {
        self.text.line_of(self.text.top_limit.unwrap_or(0))
    }
}

impl Ctx {
    /// A surface's text changed, with new output say. Copy state reads it again while a window
    /// shows the bottom (scope §5.5): its cursor on the surface's cursor line or below, in normal
    /// state. A buffer in passthrough has no copy state, and this does nothing there.
    pub fn surface_text_changed(&mut self, buf: BufferId) {
        if let Some(c) = self.copy_mut(buf) {
            c.stale = true;
        }
    }

    fn copy_mut(&mut self, buf: BufferId) -> Option<&mut Copy> {
        match &mut self.buffers.get_mut(buf)?.content {
            Content::Custom { copy, .. } => copy.as_deref_mut(),
            Content::Text(_) => None,
        }
    }

    /// Bring a surface buffer's copy state up to its modal state: a snapshot outside
    /// passthrough and none in it. With `follow`, read what changed if a window follows it.
    pub(crate) fn copy_sync(&mut self, buf: BufferId, follow: bool) {
        let Some(b) = self.buffers.get_mut(buf) else {
            return;
        };
        let passthrough = b.state == State::Passthrough;
        let Content::Custom { obj, copy, .. } = &mut b.content else {
            return;
        };
        match copy {
            Some(_) if passthrough => *copy = None,
            Some(c) if c.stale && follow => self.copy_follow(buf, false),
            None if !passthrough && obj.is_some() => self.copy_enter(buf),
            _ => {}
        }
    }

    /// Ask the surface for its text, guarded. None if it's checked out or has no text.
    fn copy_read(&mut self, buf: BufferId, update: bool) -> Option<SurfaceText> {
        let owner = match self.buffers.get(buf).map(|b| &b.content) {
            Some(Content::Custom {
                owner,
                obj: Some(_),
                ..
            }) => *owner,
            _ => return None,
        };
        let read = |ctx: &mut Ctx| Ok(ctx.checkout_surface(buf, |s, _| s.text(update))?);
        self.guarded(owner, read).ok().flatten()
    }

    /// Take the snapshot, with every window showing the buffer at the surface's cursor.
    fn copy_enter(&mut self, buf: BufferId) {
        let Some(st) = self.copy_read(buf, false) else {
            return;
        };
        let mut text = TextBuf::new("");
        *text.settings_mut() = Settings {
            wrap: Wrap::Off,
            read_only: true,
            undo_steps: 0,
            ..Settings::default()
        };
        text.snapshot = true;
        for w in self.windows_showing(buf) {
            let win = &self.windows[w];
            text.add_view(w, win.cols, win.rows);
        }
        let mut c = Copy {
            text,
            cursor: 0,
            stale: false,
        };
        c.take(&st);
        if let Some(Content::Custom { copy, .. }) =
            self.buffers.get_mut(buf).map(|b| &mut b.content)
        {
            *copy = Some(Box::new(c));
        }
        self.copy_home(buf);
    }

    /// Read the whole text again, as after a resize reflows it.
    pub(crate) fn copy_retake(&mut self, buf: BufferId) {
        if self.copy_mut(buf).is_some() {
            self.copy_follow(buf, true);
        }
    }

    /// Every window on the surface's cursor, showing the live view.
    fn copy_home(&mut self, buf: BufferId) {
        let Some(c) = self.copy_mut(buf) else { return };
        let (at, top) = (c.cursor, c.text.top_limit.unwrap_or(0));
        let wins: Vec<WindowId> = c.text.views.keys().copied().collect();
        let at = self.rested(buf, at);
        let Some(c) = self.copy_mut(buf) else { return };
        for w in wins {
            c.text.set_cursor(w, at);
            c.text.set_scroll(w, top, 0);
        }
        // A window that shows it later starts here too.
        let sel = Selection::cursor(at);
        let (cols, rows, top_row) = (0, 0, 0);
        c.text.rest = Some(View {
            sel,
            top,
            top_row,
            cols,
            rows,
        });
    }

    /// Read what changed, if a window follows, or with `whole` all of it. Each follower keeps
    /// its place below the surface's cursor line, on the cursor if it was there, and its
    /// distance above the live view. The others keep their lines, less those that left the start.
    fn copy_follow(&mut self, buf: BufferId, whole: bool) {
        let follows = self.state_of(buf) == State::Normal && self.prompt.is_none();
        let Some(c) = self.copy_mut(buf) else { return };
        let (t, cursor) = (&c.text, c.cursor);
        let (cursor_line, live) = (t.line_of(cursor), c.live_top());
        let at = |p: usize| (t.line_of(p), t.display_col(p));
        let places: Vec<Place> = (t.views.iter())
            .map(|(&w, v)| {
                let head = at(v.sel.head);
                let top = t.line_of(t.scroll(w).map_or(0, |s| s.0));
                Place {
                    w,
                    head,
                    anchor: at(v.sel.anchor),
                    sel: v.sel,
                    top,
                    below: head.0.checked_sub(cursor_line).filter(|_| follows),
                    on_cursor: v.sel.head == cursor,
                    above_live: live.saturating_sub(top),
                }
            })
            .collect();
        if !whole && places.iter().all(|p| p.below.is_none()) {
            return; // it stays stale until one comes back down
        }
        c.stale = false;
        let Some(mut st) = self.copy_read(buf, !whole) else {
            return;
        };
        if whole {
            (st.dropped, st.from) = (0, 0);
        }
        let Some(c) = self.copy_mut(buf) else { return };
        c.take(&st);
        let (t, cursor) = (&mut c.text, c.cursor);
        let (cursor_line, last) = (t.line_of(cursor), last_line(t));
        let live = t.line_of(t.top_limit.unwrap_or(0));
        let kept = |t: &TextBuf, (line, vcol): (usize, usize)| {
            t.pos_at_vcol(line.saturating_sub(st.dropped).min(last), vcol)
        };
        let mut moved = vec![];
        for p in places {
            let (anchor, head, top) = match p.below {
                Some(below) => {
                    let line = (cursor_line + below).min(last);
                    let head = match p.on_cursor {
                        true => cursor,
                        false => t.pos_at_vcol(line, p.head.1),
                    };
                    (head, head, live - p.above_live.min(live))
                }
                None => {
                    let top = p.top.saturating_sub(st.dropped);
                    (kept(t, p.anchor), kept(t, p.head), top)
                }
            };
            t.set_scroll(p.w, t.line_start(top), 0);
            moved.push((p.w, anchor, head, p.sel));
        }
        for (w, anchor, head, sel) in moved {
            let rested = self.rested(buf, head);
            let anchor = if anchor == head { rested } else { anchor };
            let sel = Selection {
                anchor,
                head: rested,
                ..sel
            };
            if let Some(c) = self.copy_mut(buf) {
                c.text.set_selection(w, sel);
            }
        }
    }

    /// The snapshot line a window shows on its top row, scrolled to its cursor first (C5), or
    /// None outside copy state.
    pub(crate) fn copy_top(&mut self, buf: BufferId, w: WindowId) -> Option<usize> {
        let t = &mut self.copy_mut(buf)?.text;
        t.scroll_to_cursor(w);
        Some(t.line_of(t.scroll(w)?.0))
    }

    /// Draw copy state's selection and search matches over the cells a surface drew, and
    /// return the cursor, as `draw_window` does.
    pub(crate) fn draw_copy(
        &self,
        scene: &mut Scene,
        p: &WindowPlacement,
        buf: BufferId,
        focused: bool,
    ) -> Option<(usize, usize, bool, Option<CursorShape>)> {
        let t = self.buffers.get(buf)?.content.text()?;
        let (w, cols) = (p.window, self.windows.get(p.window)?.cols);
        let (vp, sel) = (t.viewport(w)?, t.selection(w)?);
        let shown = || {
            let cells = vp.rows.iter().flat_map(|r| r.cells.iter().map(|c| c.pos));
            cells.chain(vp.rows.iter().filter_map(|r| r.line_end.map(|e| e.pos)))
        };
        let (lo, hi) = (shown().min().unwrap_or(0), shown().max().unwrap_or(0));
        let rope = t.rope();
        let hi = match hi < t.len() {
            true => rope.char_to_byte(rope.byte_to_char(hi) + 1),
            false => hi,
        };
        let pattern = self.search_highlight.as_ref();
        let found = pattern.map_or(vec![], |pat| t.matches(pat, lo..hi));
        let current = pattern.and_then(|pat| t.match_at(pat, sel.head));
        let visual = focused && self.state_of(buf) == State::Visual;
        let chosen = visual.then(|| selected(t, sel));
        // A block stops at a line's end, as in a text window.
        let in_selection = |pos: usize, vcol: usize| {
            let Some((bytes, block)) = &chosen else {
                return false;
            };
            let end = pos >= t.len() || rope.byte(pos) == b'\n';
            bytes.contains(&pos) && block.as_ref().is_none_or(|v| v.contains(&vcol) && !end)
        };
        let face_at = |pos: usize, vcol: usize| match () {
            _ if in_selection(pos, vcol) => Some("selection"),
            _ if current.as_ref().is_some_and(|m| m.contains(&pos)) => Some("search-current"),
            _ if found.iter().any(|m| m.contains(&pos)) => Some("search"),
            _ => None,
        };
        for (i, r) in vp.rows.iter().enumerate() {
            let cells = r.cells.iter().enumerate();
            let cells = cells.filter(|(_, c)| c.kind != CellKind::Filler);
            let ends = r.line_end.map(|e| (e.col, e.pos, e.vcol));
            let at = cells.map(|(j, c)| (j, c.pos, c.vcol)).chain(ends);
            let y = p.row + i;
            let mut end = None;
            for (j, pos, vcol) in at.filter(|a| a.0 < cols) {
                // A click finds the position, as on text, and past the row's end its end.
                scene.map(y, p.col + j, (w, pos));
                end = Some((j, pos));
                let Some(face) = face_at(pos, vcol) else {
                    continue;
                };
                let style = self.faces.get(face);
                let Some(mut cell) = scene.row(y).get(p.col + j).cloned() else {
                    continue;
                };
                cell.fg = style.fg.unwrap_or(cell.fg);
                cell.bg = style.bg.unwrap_or(cell.bg);
                scene.put(y, p.col + j, cell);
            }
            if let Some((j, pos)) = end {
                (j + 1..cols).for_each(|x| scene.map(y, p.col + x, (w, pos)));
            }
        }
        let (row, col) = t.screen_pos(w, sel.head)?;
        Some((row, col, false, None))
    }
}
