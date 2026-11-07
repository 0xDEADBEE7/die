//! Marks, the jump list and the change list, as your Evil has them (`evil-jumps.el`), except
//! where deviations.json says otherwise.
//!
//! **Marks** are core's named positions ([`TextBuf::mark`]), so they move with edits and the
//! session keeps them. Any character names one. `A` to `Z` are one each across buffers, and live
//! in the buffer they point into. `[` and `]` are what the last operator or paste covered, `^`
//! where insert state ended, `<` and `>` the last selection, and `.` the last change.
//!
//! **The jump list** is core's [`PlaceList`] for the window, newest first, as Evil's ring is. A
//! command Evil gives `:jump` sets a jump where the cursor is before it runs, whether or not it
//! then gets anywhere, except as the motion of an operator ([`before`]). A window that came to show
//! another buffer by any other way sets one where it was. Setting a jump after going back through
//! the list drops as many of the *oldest* jumps as it went back, which is what Evil's
//! `(ring-remove ring)` does. It looks like a slip, and it's what your Evil does.
//!
//! **The change list** is `g;` and `g,`. Your Evil has no `goto-chg`, so there they and `` `. ``
//! only signal. These are Vim's, per buffer, read from core's change log before and after each
//! command. A change on the line of the last one and fewer than 79 columns from it replaces it.

use crate::Vim;
use crate::grammar::{Kind, Start, Target, buffer_window, run_motion};
use crate::insert::{self, Entry};
use crate::motions;
use crate::operators;
use crate::repeat::Change;
use crate::text::indent_end;
use crate::visual;
use die_core::{
    Args, BufferId, Ctx, Error, Gravity, MarkerId, Place, PlaceList, Result, Selection, Shape,
    ShowIn, State, TextBuf, WindowId,
};

/// What your Evil's rings hold.
const MAX_JUMPS: usize = 100;
/// What Vim's change list holds.
const MAX_CHANGES: usize = 100;
/// Vim's reach for a change that replaces the last one, with no `textwidth`.
const SAME_CHANGE: usize = 79;

/// The commands Evil gives `:jump`, as this crate names them.
const JUMPS: &[&str] = &[
    "vim-goto-first-line",
    "vim-goto-line",
    "vim-jump-item",
    "vim-forward-paragraph",
    "vim-backward-paragraph",
    "vim-window-top",
    "vim-window-middle",
    "vim-window-bottom",
    "vim-undo",
    "vim-search-forward",
    "vim-search-backward",
    "vim-search-next",
    "vim-search-previous",
    "vim-search-word-forward",
    "vim-search-word-backward",
    "vim-forward-section",
    "vim-backward-section",
    "vim-goto-definition",
    "vim-goto-mark",
    "vim-goto-mark-line",
    "vim-insert-resume",
    "vim-find-file-at-point",
];

/// Whether a command sets a jump before it runs.
pub fn sets_jump(name: &str) -> bool {
    JUMPS.contains(&name)
}

/// Whether a command that lands in another buffer has said all there is to say about it.
pub fn says_its_jump(name: &str) -> bool {
    sets_jump(name) || matches!(name, "vim-jump-backward" | "vim-jump-forward")
}

/// What die-vim keeps per buffer for `g;` and `g,`.
#[derive(Default)]
pub struct Changes {
    rev: u64,
    /// Oldest first.
    list: Vec<MarkerId>,
    /// Where `g;` and `g,` have got to; the list's length when they haven't begun.
    at: usize,
}

/// A place, as a buffer and a position.
type Spot = (BufferId, usize);

fn cursor_of(ctx: &Ctx, b: BufferId, w: WindowId) -> Option<usize> {
    ctx.text(b).ok()?.cursor(w)
}

/// Runs ahead of each command.
pub fn before(ctx: &mut Ctx, a: &Args, jump: bool) {
    let Ok((b, w)) = buffer_window(ctx, a) else {
        return;
    };
    let Some(pos) = cursor_of(ctx, b, w) else {
        return;
    };
    crossed(ctx, w, b);
    note_changes(ctx, b);
    if jump && ctx.state_of(b) != State::OperatorPending {
        set_jump(ctx, w, (b, pos));
    }
}

/// Runs after each command.
pub fn after(ctx: &mut Ctx, a: &Args, quiet: bool) {
    let Ok((_, w)) = buffer_window(ctx, a) else {
        return;
    };
    // Not the buffer in `a`: the command may have left it.
    let Some(b) = ctx.window_buffer(w) else {
        return;
    };
    let Some(pos) = cursor_of(ctx, b, w) else {
        return;
    };
    if !quiet {
        crossed(ctx, w, b);
    }
    note_changes(ctx, b);
    let _ = ctx.with_ext(|v: &mut Vim, _| v.seen.insert(w, (b, pos)));
}

/// [`Ctx::set_jump_rule`]: this window is about to go far with no key. A jump, as ahead of a
/// command Evil gives `:jump`, and the place counts as seen, so the next key's [`crossed`] doesn't
/// set an older one on top of it.
pub fn before_far_move(ctx: &mut Ctx, w: WindowId) {
    let Some(b) = ctx.window_buffer(w) else {
        return;
    };
    let Some(pos) = cursor_of(ctx, b, w) else {
        return;
    };
    crossed(ctx, w, b);
    set_jump(ctx, w, (b, pos));
    let _ = ctx.with_ext(|v: &mut Vim, _| v.seen.insert(w, (b, pos)));
}

/// A window that shows another buffer than it did last time sets a jump where it was.
fn crossed(ctx: &mut Ctx, w: WindowId, now: BufferId) {
    let seen = ctx.with_ext(|v: &mut Vim, _| v.seen.get(&w).copied());
    if let Ok(Some((was, pos))) = seen
        && was != now
        && ctx.text(was).is_ok()
    {
        set_jump(ctx, w, (was, pos));
    }
}

fn alive(ctx: &Ctx, p: Place) -> Option<usize> {
    ctx.text(p.buffer).ok()?.marker(p.marker)
}

/// A window's list, without what a killed buffer took with it.
fn list_of(ctx: &Ctx, w: WindowId) -> PlaceList {
    let mut list = ctx.places(w).cloned().unwrap_or_default();
    let before = list.places.len();
    list.places.retain(|p| alive(ctx, *p).is_some());
    if list.places.len() != before {
        let last = list.places.len().checked_sub(1);
        list.index = list.index.and_then(|i| last.map(|l| i.min(l)));
    }
    list
}

fn drop_place(ctx: &mut Ctx, p: Place) {
    if let Ok(t) = ctx.text_mut(p.buffer) {
        t.remove_marker(p.marker);
    }
}

/// Evil's `evil--jumps-push`: not if it's the newest already. Only `*messages*` is left out: your
/// Evil keeps jumps only in files and in `*new*` and `*scratch*`, as it can go back to no others,
/// and here a buffer can be gone back to by its id.
fn push(ctx: &mut Ctx, list: &mut PlaceList, (b, pos): Spot) {
    if ctx.buffer_name(b) == Some("*messages*") {
        return;
    }
    if let Some(top) = list.places.first()
        && top.buffer == b
        && alive(ctx, *top) == Some(pos)
    {
        return;
    }
    let Ok(t) = ctx.text_mut(b) else { return };
    let Ok(marker) = t.add_marker(pos, Gravity::Left) else {
        return;
    };
    list.places.insert(0, Place { buffer: b, marker });
    if list.places.len() > MAX_JUMPS
        && let Some(oldest) = list.places.pop()
    {
        drop_place(ctx, oldest);
    }
}

/// Evil's `evil-set-jump`.
fn set_jump(ctx: &mut Ctx, w: WindowId, spot: Spot) {
    let mut list = list_of(ctx, w);
    set_jump_in(ctx, &mut list, spot);
    ctx.set_places(w, list);
}

fn set_jump_in(ctx: &mut Ctx, list: &mut PlaceList, spot: Spot) {
    for _ in 0..list.index.take().unwrap_or(0) {
        if let Some(oldest) = list.places.pop() {
            drop_place(ctx, oldest);
        }
    }
    push(ctx, list, spot);
}

/// `C-o` and `C-i`.
pub fn jump(ctx: &mut Ctx, a: &Args, back: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let here = cursor_of(ctx, b, w).ok_or(Error::Msg("no cursor".into()))?;
    let mut list = list_of(ctx, w);
    let mut to = (b, here);
    let most = list.places.len() + 1;
    for _ in 0..a.count.unwrap_or(1).clamp(1, most) {
        if list.index.is_none() {
            list.index = Some(0);
            push(ctx, &mut list, (b, here));
        }
        let i = list.index.unwrap_or(0);
        let next = if back { Some(i + 1) } else { i.checked_sub(1) };
        let hit = next.and_then(|n| list.places.get(n).map(|p| (n, *p)));
        if let Some((n, p)) = hit
            && let Some(pos) = alive(ctx, p)
        {
            list.index = Some(n);
            to = (p.buffer, pos);
        }
    }
    ctx.set_places(w, list);
    go(ctx, a, to, Kind::Exclusive, false)
}

/// Go to a spot, as a motion if it's in this buffer. An operator can't reach into another.
fn go(ctx: &mut Ctx, a: &Args, (to, pos): Spot, kind: Kind, line: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    if to == b {
        return run_motion(ctx, a, kind, None, move |s: &Start| {
            let pos = if line {
                indent_end(s.text, s.text.line_of(pos))
            } else {
                pos
            };
            Ok(Target {
                to_last_line: true,
                ..Target::at(pos)
            })
        });
    }
    if ctx.state_of(b) == State::OperatorPending {
        operators::cancel(ctx, a)?;
        return Err(Error::Msg("That's in another buffer".into()));
    }
    ctx.show(to, ShowIn::Current)?;
    let t = ctx.text_mut(to)?;
    let pos = if line {
        indent_end(t, t.line_of(pos))
    } else {
        pos
    };
    t.set_cursor(w, crate::text::normal_pos(t, pos));
    Ok(())
}

/// `m`: set a mark at the cursor.
pub fn set(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let Some(name) = a.char() else { return Ok(()) };
    let pos = cursor_of(ctx, b, w).ok_or(Error::Msg("no cursor".into()))?;
    let at = point_in_selection(ctx, b, w).unwrap_or(pos);
    match name {
        '`' | '\'' => {
            set_jump(ctx, w, (b, pos));
            Ok(())
        }
        '(' | ')' | '{' | '}' | '.' => {
            Err(Error::Msg(format!("Cannot set special marker `{name}'")))
        }
        // These name the variables behind your Evil's selection. Setting them shows nowhere but
        // in `gv`, and setting them with no selection yet is an error.
        '<' | '>' => match ctx
            .text(b)?
            .mark(name)
            .or(visual::is_on(ctx, b).then_some(pos))
        {
            Some(_) => Ok(()),
            None => Err(Error::Msg("Wrong type argument: markerp, nil".into())),
        },
        _ => {
            if name.is_ascii_uppercase() {
                for other in ctx.buffer_list() {
                    if let Ok(t) = ctx.text_mut(other) {
                        t.remove_mark(name);
                    }
                }
            }
            ctx.text_mut(b)?.set_mark(name, at, Gravity::Left)
        }
    }
}

/// Where your Evil's point is for a command run from a selection. It widens the region to cover
/// what's selected first, so going forward that's past the head, or past its line.
fn point_in_selection(ctx: &Ctx, b: BufferId, w: WindowId) -> Option<usize> {
    if !visual::is_on(ctx, b) {
        return None;
    }
    let t = ctx.text(b).ok()?;
    let s = t.selection(w)?;
    let line = t.line_of(s.head);
    match s.shape {
        Shape::Char if s.head >= s.anchor => crate::text::next_stop(t, s.head),
        Shape::Line if s.head >= s.anchor && line + 1 < t.line_count() => {
            Some(t.line_start(line + 1))
        }
        Shape::Line => Some(t.line_start(line)),
        _ => Some(s.head),
    }
}

/// `` ` `` and `'`: go to a mark, or for `'` to the first non-blank of its line.
pub fn goto(ctx: &mut Ctx, a: &Args, line: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let kind = if line {
        Kind::Linewise
    } else {
        Kind::Exclusive
    };
    let Some(name) = a.char() else {
        return operators::cancel(ctx, a);
    };
    if matches!(name, '{' | '}') {
        let run = if name == '{' {
            motions::backward_paragraph
        } else {
            motions::forward_paragraph
        };
        return run_motion(ctx, a, kind, None, run);
    }
    let spot = match name {
        '`' | '\'' => swap(ctx, b, w),
        '<' | '>' if visual::is_on(ctx, b) => {
            let t = ctx.text(b)?;
            t.selection(w)
                .map(|s| (b, selection_end(t, s, name == '<')))
        }
        'A'..='Z' => ctx
            .buffer_list()
            .into_iter()
            .find_map(|o| ctx.text(o).ok()?.mark(name).map(|p| (o, p))),
        _ => ctx.text(b)?.mark(name).map(|p| (b, p)),
    };
    let Some(spot) = spot else {
        operators::cancel(ctx, a)?;
        return Err(Error::Msg(format!("Marker `{name}' is not set")));
    };
    go(ctx, a, spot, kind, line)
}

/// Evil's `evil-jump-backward-swap`, which is what the marks `` ` `` and `'` are: where it lands
/// is the older jump, and this place stays the newest one.
fn swap(ctx: &mut Ctx, b: BufferId, w: WindowId) -> Option<Spot> {
    let here = cursor_of(ctx, b, w)?;
    let mut list = list_of(ctx, w);
    if list.index.is_none() {
        list.index = Some(0);
        push(ctx, &mut list, (b, here));
    }
    let next = list.index.unwrap_or(0) + 1;
    let hit = list.places.get(next).copied();
    let to = hit.and_then(|p| Some((p.buffer, alive(ctx, p)?)));
    if to.is_some() {
        list.index = Some(next);
    }
    set_jump_in(ctx, &mut list, (b, here));
    ctx.set_places(w, list);
    Some(to.unwrap_or((b, here)))
}

/// `<` or `>` of a selection: whole lines start at the line's start and end at its end.
fn selection_end(t: &TextBuf, s: Selection, first: bool) -> usize {
    let (lo, hi) = (s.anchor.min(s.head), s.anchor.max(s.head));
    match (s.shape, first) {
        (Shape::Line, true) => t.line_start(t.line_of(lo)),
        (Shape::Line, false) => crate::text::line_end(t, t.line_of(hi)),
        (_, true) => lo,
        (_, false) => hi,
    }
}

/// A selection has ended, so `<` and `>` are its ends.
pub fn selection_left(t: &mut TextBuf, s: Selection) -> Result {
    let (first, last) = (selection_end(t, s, true), selection_end(t, s, false));
    t.set_mark('<', first, Gravity::Left)?;
    t.set_mark('>', last, Gravity::Left)
}

/// The bytes `beg..` an operator is about to act on: this follows their end through its edit.
pub fn track(t: &mut TextBuf, end: usize) -> Option<MarkerId> {
    t.add_marker(end, Gravity::Left).ok()
}

/// An operator ran on what starts at `beg` and ended at `end`: `[` is that start, and `]` the
/// character before that end, or the start if there's nothing left.
pub fn operated(t: &mut TextBuf, beg: usize, end: Option<MarkerId>) -> Result {
    let end = end.and_then(|m| t.remove_marker(m)).unwrap_or(beg);
    covered(t, beg, end)
}

/// `[` and `]` for what's at `beg..end`.
pub fn covered(t: &mut TextBuf, beg: usize, end: usize) -> Result {
    // An edit that changed how long a case is can leave a byte count inside a character.
    let mut beg = beg.min(t.len());
    while !t.is_boundary(beg) {
        beg -= 1;
    }
    let last = crate::text::prev_stop(t, end.min(t.len())).unwrap_or(beg);
    t.set_mark('[', beg, Gravity::Left)?;
    t.set_mark(']', last.max(beg), Gravity::Left)
}

/// Insert state ended here, which `gi` goes back to.
pub fn insert_ended(t: &mut TextBuf, pos: usize) -> Result {
    t.set_mark('^', pos, Gravity::Right)
}

/// `gi`: where insert state last ended, and insert. From a selection, only go there.
pub fn insert_resume(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let to = ctx.text(b)?.mark('^');
    if visual::is_on(ctx, b) {
        return match to {
            Some(pos) => go(ctx, a, (b, pos), Kind::Exclusive, false),
            None => Ok(()),
        };
    }
    if let Some(pos) = to {
        ctx.text_mut(b)?.set_cursor(w, pos);
    }
    let mut s = insert::begin(ctx, a, Entry::Insert)?;
    s.repeat = Change::of(a).typed();
    ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
    Ok(())
}

/// Bring a buffer's change list up to date from core's log. Only the last change is sure to
/// be in the coordinates of the text as it is now, and no command needs more.
fn note_changes(ctx: &mut Ctx, b: BufferId) {
    let Ok(t) = ctx.text(b) else { return };
    let rev = t.rev();
    let seed = t.mark('.');
    let Ok(mut c) = ctx.with_ext(|v: &mut Vim, _| v.changes.remove(&b)) else {
        return;
    };
    let Ok(t) = ctx.text_mut(b) else { return };
    match &mut c {
        Some(c) if c.rev == rev => {}
        Some(c) => {
            let last = t.edits_since(c.rev).and_then(|changes| {
                let last = changes.filter(|e| !e.silent).last()?;
                Some((last.start, last.new_end))
            });
            c.rev = rev;
            if let Some((start, new_end)) = last {
                let at = if new_end > start {
                    crate::text::prev_stop(t, new_end).unwrap_or(start)
                } else {
                    start
                };
                changed(t, c, at);
            }
        }
        None => {
            let list = seed.and_then(|p| t.add_marker(p, Gravity::Left).ok());
            let list: Vec<MarkerId> = list.into_iter().collect();
            c = Some(Changes {
                rev,
                at: list.len(),
                list,
            });
        }
    }
    let _ = ctx.with_ext(|v: &mut Vim, _| v.changes.insert(b, c.unwrap_or_default()));
}

fn changed(t: &mut TextBuf, c: &mut Changes, at: usize) {
    let at = at.min(t.len());
    let near = c
        .list
        .last()
        .and_then(|m| t.marker(*m))
        .is_some_and(|p| t.line_of(p) == t.line_of(at) && p.abs_diff(at) < SAME_CHANGE);
    if near && let Some(m) = c.list.pop() {
        t.remove_marker(m);
    }
    if let Ok(m) = t.add_marker(at, Gravity::Left) {
        c.list.push(m);
    }
    if c.list.len() > MAX_CHANGES {
        t.remove_marker(c.list.remove(0));
    }
    c.at = c.list.len();
    let _ = t.set_mark('.', at, Gravity::Left);
}

/// `g;` and `g,`, as Vim's: a count that goes too far stops at the end of the list, and it's only
/// an error from there.
pub fn change_step(ctx: &mut Ctx, a: &Args, older: bool) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let n = a.count.unwrap_or(1).max(1);
    let got = ctx.with_ext(|v: &mut Vim, _| {
        let c = v.changes.get_mut(&b)?;
        let len = c.list.len();
        let to = match (older, len) {
            (_, 0) => Err("The change list is empty"),
            (true, _) if c.at == 0 => Err("At start of the change list"),
            (true, _) => Ok(c.at.saturating_sub(n)),
            (false, _) if c.at + 1 == len => Err("At end of the change list"),
            (false, _) => Ok(c.at.saturating_add(n).min(len - 1)),
        };
        Some(to.map(|i| {
            c.at = i;
            c.list[i]
        }))
    })?;
    let m = match got.unwrap_or(Err("The change list is empty")) {
        Ok(m) => m,
        Err(e) => {
            operators::cancel(ctx, a)?;
            return Err(Error::Msg(e.into()));
        }
    };
    let pos = ctx.text(b)?.marker(m).unwrap_or(0);
    go(ctx, a, (b, pos), Kind::Exclusive, false)
}

/// A new window starts with a copy of the window it came from's jumps, as your Evil's does.
pub fn copy_jumps(ctx: &mut Ctx, from: WindowId, to: WindowId) {
    let mut list = list_of(ctx, from);
    for p in &mut list.places {
        let pos = alive(ctx, *p).unwrap_or(0);
        if let Ok(t) = ctx.text_mut(p.buffer)
            && let Ok(m) = t.add_marker(pos, Gravity::Left)
        {
            p.marker = m;
        }
    }
    ctx.set_places(to, list);
}
