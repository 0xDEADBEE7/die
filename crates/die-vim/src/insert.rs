//! Insert and replace state. A session runs from entering either to Escape: it's one
//! undo step, it records its edits so a count can replay them, and it indents new
//! lines the way your Evil does in text buffers (like the line above). From visual state
//! on lines or a block, what's typed goes in on each of them when it ends ([`Rows`]).

use crate::grammar::{buffer_window, forget_goal};
use crate::motions::word_back;
use crate::repeat::Change;
use crate::text::{
    Chars, blank_line, indent_end, indent_stops, indentation, line_end, next_stop, pos_at_col,
    prev_stop, rest_pos,
};
use die_core::{Args, Ctx, Error, Origin, Result, Selection, State, TextBuf, WindowId};

/// The most typed text a count may replay at once, K4's budget for one line. K4 took
/// away the layout memory this guarded, so it can rise, and it stands until it does.
pub const MAX_REPEAT: usize = 1 << 20;
/// The most edits a count may replay one by one, so a mistyped count can't freeze the
/// editor. K4 made an edit cost the same on any line, so this can rise too.
const MAX_REPLAY: usize = 10_000;

/// How a session started. A count repeats the session, opening a line first
/// when it opened one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// `i`
    Insert,
    /// `a`
    Append,
    /// `I`
    InsertLine,
    /// `A`
    AppendLine,
    /// `o`
    OpenBelow,
    /// `O`
    OpenAbove,
    /// `R`
    Replace,
}

/// One edit typed in insert state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Text(String),
    /// `<CR>`
    Newline,
    /// `<BS>`
    Backspace,
    /// `<C-w>`
    DeleteWord,
    /// `<Tab>`
    Tab,
    Left,
    Right,
    Up,
    Down,
    /// A key in replace state: over a character, or at a line's end, in front of its newline.
    Over(String),
    /// `<BS>` in replace state. What it puts back isn't in the edit, so a count's replay only
    /// steps back, as your Evil's does when nothing was replaced there.
    Restore,
    /// `<C-w>` in replace state: `<BS>` back to the start of a word.
    RestoreWord,
}

/// Where on each line a session from visual state goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAt {
    /// A display column.
    Col(usize),
    FirstNonBlank,
    LineEnd,
}

/// The lines a session goes in on, the first being where it was typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    pub line: usize,
    pub at: RowAt,
    pub n: usize,
    /// `I` on a block leaves a line too short for its column alone. Otherwise it's padded.
    pub skip_short: bool,
}

pub struct Session {
    pub entry: Entry,
    pub count: usize,
    pub edits: Vec<Edit>,
    /// Escape empties the line if it holds only indentation. Entering by `a`, `A`,
    /// `o` or `O` sets it, a newline keeps it, and any other edit clears it, as
    /// Evil's `evil-maybe-remove-spaces` does.
    pub drop_indent: bool,
    /// The keys so far, for `.`. A session die-vim didn't start has none.
    pub repeat: Option<Change>,
    /// The other lines it goes in on.
    pub rows: Option<Rows>,
    /// In replace state, what each key took: `<BS>` from just past it puts it back.
    restore: Vec<Taken>,
}

/// What a key in replace state wrote over: the bytes it now holds, and what they were.
struct Taken {
    at: usize,
    end: usize,
    old: Option<String>,
}

impl Session {
    /// A session for insert state entered some other way.
    pub fn bare() -> Session {
        Session {
            entry: Entry::Insert,
            count: 1,
            edits: vec![],
            drop_indent: false,
            repeat: None,
            rows: None,
            restore: vec![],
        }
    }
}

/// Enter insert state: move to where `entry` inserts, opening a line if it does.
pub fn begin(ctx: &mut Ctx, a: &Args, entry: Entry) -> Result<Session> {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    let line = t.line_of(p);
    match entry {
        Entry::Insert => {}
        Entry::Append if p < line_end(t, line) => {
            let q = next_stop(t, p).unwrap_or(p);
            t.set_cursor(w, q);
        }
        Entry::Append => {}
        // On a line of nothing but spaces that's the last one, where `^` lands, as in Evil.
        Entry::InsertLine => {
            t.set_cursor(w, rest_pos(t, indent_end(t, line)));
        }
        Entry::AppendLine => {
            t.set_cursor(w, line_end(t, line));
        }
        Entry::Replace => {}
        Entry::OpenBelow => open_below(t, w, line)?,
        Entry::OpenAbove => {
            let s = t.line_start(line);
            t.tx(Origin::User).insert(s, "\n").commit()?;
            indent_from_above(t, w, line)?;
        }
    }
    // A goal column from normal state doesn't carry into insert state's <Up>.
    forget_goal(t, w);
    // Only now, so that a failed `o` or `O` leaves no group open. The line it
    // opened joins the group: core adds an earlier edit by the same command.
    t.begin_undo_group();
    let replacing = entry == Entry::Replace;
    ctx.set_state_of(
        b,
        if replacing {
            State::Replace
        } else {
            State::Insert
        },
    );
    Ok(Session {
        entry,
        count: a.count.unwrap_or(1).max(1),
        drop_indent: !replacing && entry != Entry::Insert && entry != Entry::InsertLine,
        ..Session::bare()
    })
}

/// Apply one insert-state edit and record it.
pub fn edit(ctx: &mut Ctx, a: &Args, s: &mut Session, e: Edit) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    match &e {
        Edit::Restore => restore(t, w, s)?,
        Edit::RestoreWord => {
            let to = word_start(t, p).ok_or(Error::Msg("Beginning of buffer".into()))?;
            while t.cursor(w).is_some_and(|c| c > to) {
                restore(t, w, s)?;
            }
        }
        _ if s.entry == Entry::Replace => {
            let (old, len) = (over_text(t, p, &e), t.len());
            apply(t, w, &e)?;
            // Your Evil notes what's at the cursor before every command, so going over a place
            // again, or just past it, forgets what the key before took there.
            s.restore.retain(|r| r.at != p);
            moved_after(s, p, t.len() as isize - len as isize);
            if matches!(e, Edit::Over(_) | Edit::Newline) {
                let end = t.cursor(w).unwrap_or(p);
                s.restore.push(Taken { at: p, end, old });
            }
        }
        _ => apply(t, w, &e)?,
    }
    if e != Edit::Newline {
        s.drop_indent = false;
    }
    s.edits.push(e);
    Ok(())
}

/// Escape: replay the session count-1 more times, drop a newly opened line's
/// indentation if nothing was typed on it, close the undo step, and step back off
/// the end of the insertion, as Evil does. An edit that fails here still ends the
/// session; the first error is returned.
pub fn end(ctx: &mut Ctx, a: &Args, s: Option<Session>) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let cursor = |t: &TextBuf| t.cursor(w).unwrap_or(0);
    let mut r = Ok(());
    if let Some(s) = s {
        r = replay(t, w, &s);
        r = r.and(rows(t, w, &s));
        let line = t.line_of(cursor(t));
        let (ls, le) = (t.line_start(line), line_end(t, line));
        if s.drop_indent && le > ls && blank_line(t, line) {
            let dropped = t.tx(Origin::User).delete(ls..le).commit();
            r = r.and(dropped.map(drop));
        }
        t.end_undo_group();
    }
    let p = cursor(t);
    let back = if p > t.line_start(t.line_of(p)) {
        prev_stop(t, p).unwrap_or(p)
    } else {
        p
    };
    // A session that ends on the empty line after a final newline leaves the
    // cursor there, as Evil and Vim do, so the next `i` types on that line. Only
    // motions keep off it (scope §10).
    t.set_cursor(w, rest_pos(t, back));
    ctx.set_state_of(b, State::Normal);
    r
}

/// Replay the session count-1 more times. Typed text alone goes in at once; anything
/// else goes key by key. A count too large to do either fails, with the session's
/// first copy kept: more is an error, not a hang.
fn replay(t: &mut TextBuf, w: WindowId, s: &Session) -> Result {
    let more = s.count - 1;
    let opens = matches!(s.entry, Entry::OpenBelow | Entry::OpenAbove);
    let too_large = || Error::Msg("Count too large".into());
    let text: Option<String> = s
        .edits
        .iter()
        .map(|e| match e {
            Edit::Text(x) => Some(x.as_str()),
            _ => None,
        })
        .collect();
    if let (Some(one), false) = (text, opens) {
        if more == 0 || one.is_empty() {
            return Ok(());
        }
        let n = one
            .len()
            .checked_mul(more)
            .filter(|&n| n <= MAX_REPEAT)
            .ok_or_else(too_large)?;
        let p = t.cursor(w).unwrap_or(0);
        t.tx(Origin::User).insert(p, one.repeat(more)).commit()?;
        t.set_cursor(w, p + n);
        return Ok(());
    }
    (s.edits.len() + usize::from(opens))
        .checked_mul(more)
        .filter(|&n| n <= MAX_REPLAY)
        .ok_or_else(too_large)?;
    (0..more).try_for_each(|_| {
        if opens {
            let line = t.line_of(t.cursor(w).unwrap_or(0));
            open_below(t, w, line)?;
        }
        s.edits.iter().try_for_each(|e| apply(t, w, e))
    })
}

/// Where typing `s` at `p` stops taking text in replace state, and how many spaces it leaves
/// to keep what's after it in its column, as Emacs's overwrite mode does. At a line's end it
/// takes nothing. A tab it wouldn't fill stays.
fn over(t: &TextBuf, p: usize, s: &str) -> (usize, usize) {
    let eol = line_end(t, t.line_of(p));
    let width = TextBuf::new(s).display_col(s.len());
    if width == 0 {
        return (p, 0);
    }
    let target = t.display_col(p) + width;
    let mut end = p;
    while end < eol && t.display_col(end) < target {
        end = next_stop(t, end).unwrap_or(eol).min(eol);
    }
    let reached = t.display_col(end);
    if reached <= target {
        return (end, 0);
    }
    let last = prev_stop(t, end).unwrap_or(p);
    if t.rope().get_byte(last) == Some(b'\t') {
        (last, 0)
    } else {
        (end, reached - target)
    }
}

/// What this edit takes in replace state, for `<BS>` to put back. Nothing but a key does.
fn over_text(t: &TextBuf, p: usize, e: &Edit) -> Option<String> {
    let Edit::Over(s) = e else { return None };
    let (end, _) = over(t, p, s);
    (end > p).then(|| t.rope().byte_slice(p..end).to_string())
}

/// `<BS>` in replace state: put back what the key before the cursor took, or just step back.
fn restore(t: &mut TextBuf, w: WindowId, s: &mut Session) -> Result {
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    let Some(i) = s.restore.iter().rposition(|r| r.end == p && r.at <= p) else {
        return apply(t, w, &Edit::Restore);
    };
    let Taken { at, old, .. } = s.restore.remove(i);
    let len = t.len();
    t.tx(Origin::User)
        .replace(at..p, old.unwrap_or_default())
        .commit()?;
    t.set_cursor(w, at);
    moved_after(s, at, t.len() as isize - len as isize);
    Ok(())
}

/// The text after `p` moved by `by` bytes.
fn moved_after(s: &mut Session, p: usize, by: isize) {
    for r in s.restore.iter_mut().filter(|r| r.at > p) {
        r.at = r.at.saturating_add_signed(by);
        r.end = r.end.saturating_add_signed(by);
    }
}

/// Where `<C-w>` goes back to: the start of a word, never past the line's start, unless it's
/// at the line's start, when it goes over the newline.
fn word_start(t: &TextBuf, p: usize) -> Option<usize> {
    let ls = t.line_start(t.line_of(p));
    if p == ls {
        prev_char(t, p)
    } else {
        word_back(t, p, false).map(|q| q.max(ls))
    }
}

/// The place for display column `col` on `line`, as `move-to-column` with its force has it: a
/// tab the column falls inside becomes spaces, so the place is exact. A wide character it falls
/// inside stays, and the place is after it.
pub fn at_col_forced(t: &mut TextBuf, line: usize, col: usize) -> Result<usize> {
    let p = pos_at_col(t, line, col).min(line_end(t, line));
    let have = t.display_col(p);
    let tab = prev_stop(t, p).filter(|&q| {
        q >= t.line_start(line) && t.rope().get_byte(q) == Some(b'\t') && t.display_col(q) <= col
    });
    let (Some(q), true) = (tab, have > col) else {
        return Ok(p);
    };
    let before = t.display_col(q);
    t.tx(Origin::User)
        .replace(q..p, " ".repeat(have - before))
        .commit()?;
    Ok(q + col - before)
}

/// Go in on the session's other lines, keeping the cursor on the first.
fn rows(t: &mut TextBuf, w: WindowId, s: &Session) -> Result {
    let Some(rows) = s.rows else { return Ok(()) };
    if rows.n < 2 {
        return Ok(());
    }
    (s.edits.len() * s.count)
        .checked_mul(rows.n)
        .filter(|&n| n <= MAX_REPEAT)
        .ok_or_else(|| Error::Msg("Count too large".into()))?;
    let here = t.cursor(w).unwrap_or(0);
    let mut done = Ok(());
    for line in rows.line + 1..rows.line + rows.n {
        if line >= t.line_count() {
            break;
        }
        let eol = line_end(t, line);
        let pos = match rows.at {
            RowAt::Col(col) => {
                let have = t.display_col(eol);
                if have < col && rows.skip_short {
                    continue;
                }
                if have < col {
                    t.tx(Origin::User)
                        .insert(eol, " ".repeat(col - have))
                        .commit()?;
                    eol + col - have
                } else {
                    at_col_forced(t, line, col)?
                }
            }
            RowAt::FirstNonBlank => indent_end(t, line),
            RowAt::LineEnd => eol,
        };
        t.set_cursor(w, pos);
        for _ in 0..s.count {
            done = done.and(s.edits.iter().try_for_each(|e| apply(t, w, e)));
        }
    }
    t.set_cursor(w, here);
    done
}

fn apply(t: &mut TextBuf, w: WindowId, e: &Edit) -> Result {
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    let fail = |m: &str| Err(Error::Msg(m.into()));
    // Only a run of Up and Down keeps a goal. Any other edit clears it, even one that fails.
    if !matches!(e, Edit::Up | Edit::Down) {
        forget_goal(t, w);
    }
    let pos = match e {
        Edit::Text(s) => {
            t.tx(Origin::User).insert(p, s.as_str()).commit()?;
            p + s.len()
        }
        Edit::Newline => return newline(t, w, p),
        Edit::Backspace => {
            let Some(q) = prev_char(t, p) else {
                return fail("Beginning of buffer");
            };
            t.tx(Origin::User).delete(q..p).commit()?;
            q
        }
        // At a line's start it joins the lines; otherwise it deletes back to the
        // start of a word, never past the line's start.
        Edit::DeleteWord => {
            let Some(q) = word_start(t, p) else {
                return fail("Beginning of buffer");
            };
            t.tx(Origin::User).delete(q..p).commit()?;
            q
        }
        Edit::RestoreWord => match word_start(t, p) {
            Some(q) => q,
            None => return fail("Beginning of buffer"),
        },
        Edit::Tab => return tab(t, w, p),
        Edit::Over(s) => {
            let (end, spaces) = over(t, p, s);
            t.tx(Origin::User)
                .replace(p..end, format!("{s}{}", " ".repeat(spaces)))
                .commit()?;
            p + s.len()
        }
        Edit::Restore => match prev_char(t, p) {
            Some(q) => q,
            None => return fail("Beginning of buffer"),
        },
        Edit::Left => match prev_stop(t, p) {
            Some(q) => q,
            None => return fail("Beginning of buffer"),
        },
        Edit::Right => match next_stop(t, p) {
            Some(q) => q,
            None => return fail("End of buffer"),
        },
        // By screen row, as next-line does under your line-move-visual, and a run of
        // them keeps the column it started at. Down on the last row goes to the end of
        // the text before it fails, as next-line does, and the column stays for an Up.
        Edit::Up | Edit::Down => {
            let (dy, none) = if *e == Edit::Up {
                (-1, "Beginning of buffer")
            } else {
                (1, "End of buffer")
            };
            let goal = t.selection(w).and_then(|s| s.goal);
            return match t.move_visual(w, p, dy, goal) {
                Some((q, goal)) if q != p => {
                    t.set_selection(w, Selection::cursor(q).with_goal(Some(goal)));
                    Ok(())
                }
                Some((_, goal)) if dy > 0 => {
                    t.set_selection(w, Selection::cursor(t.len()).with_goal(Some(goal)));
                    fail(none)
                }
                _ => fail(none),
            };
        }
    };
    t.set_cursor(w, pos);
    Ok(())
}

fn prev_char(t: &TextBuf, p: usize) -> Option<usize> {
    let mut c = Chars::at(t, p);
    c.back().then(|| c.pos())
}

/// `<CR>`, as Emacs's `newline` with `electric-indent-mode` in fundamental-mode:
/// the line left behind loses its trailing whitespace, and the new line takes the
/// indentation of the line above it.
fn newline(t: &mut TextBuf, w: WindowId, p: usize) -> Result {
    let mut c = Chars::at(t, p);
    while matches!(c.prev_ch(), Some(' ' | '\t')) {
        c.back();
    }
    let start = c.pos();
    t.tx(Origin::User).replace(start..p, "\n").commit()?;
    indent_from_above(t, w, t.line_of(start) + 1)
}

/// `o`: a new line below `line`, indented like it.
fn open_below(t: &mut TextBuf, w: WindowId, line: usize) -> Result {
    let e = line_end(t, line);
    t.tx(Origin::User).insert(e, "\n").commit()?;
    indent_from_above(t, w, line + 1)
}

/// Give `line` the indentation of the line above it, or none if that line is blank or
/// there is none (Emacs's `indent-according-to-mode` in fundamental-mode). As Emacs's
/// `indent-line-to` does, it only changes what it must: an indentation of the right width
/// stays as it is, tabs and all. Too little gains spaces, and too much loses what's past the
/// column. The cursor goes to the end of the indentation.
pub fn indent_from_above(t: &mut TextBuf, w: WindowId, line: usize) -> Result {
    let col = match line.checked_sub(1) {
        Some(above) if !blank_line(t, above) => indentation(t, above),
        _ => 0,
    };
    let stops = indent_stops(t, line);
    let (end, now) = stops[stops.len() - 1];
    if now < col {
        let spaces = " ".repeat(col - now);
        t.tx(Origin::User).insert(end, spaces).commit()?;
    } else if now > col {
        // What fits stays. A tab past the column becomes the spaces that reach it. The first
        // stop is column 0, so one always fits.
        let &(keep, at) = stops.iter().rfind(|&&(_, c)| c <= col).unwrap_or(&stops[0]);
        let spaces = " ".repeat(col - at);
        t.tx(Origin::User).replace(keep..end, spaces).commit()?;
    }
    t.set_cursor(w, indent_end(t, line));
    Ok(())
}

/// `<Tab>`, as Emacs's `indent-relative` in fundamental-mode: to the next indent
/// point of the previous non-empty line, else to the next tab stop (every
/// `tab_width` columns).
fn tab(t: &mut TextBuf, w: WindowId, p: usize) -> Result {
    let line = t.line_of(p);
    let col = t.display_col(p);
    let above = (0..line).rev().find(|&l| line_end(t, l) > t.line_start(l));
    let (start, to) = match above.and_then(|l| indent_point(t, l, col)) {
        Some(to) => (p, to),
        // tab-to-tab-stop drops the whitespace before point first.
        None => {
            let mut c = Chars::at(t, p);
            while matches!(c.prev_ch(), Some(' ' | '\t')) && c.pos() > t.line_start(line) {
                c.back();
            }
            let width = t.settings().tab_width.max(1);
            (c.pos(), (col / width + 1) * width)
        }
    };
    let from = t.display_col(start);
    let spaces = " ".repeat(to.saturating_sub(from));
    t.tx(Origin::User)
        .replace(start..p, spaces.as_str())
        .commit()?;
    t.set_cursor(w, start + spaces.len());
    Ok(())
}

/// The column of the next indent point on `line` after column `col`: past the
/// rest of the word there, then past the whitespace after it. There's none if
/// the word runs to the line's end; trailing whitespace still makes one.
fn indent_point(t: &TextBuf, line: usize, col: usize) -> Option<usize> {
    let (s, e) = (t.line_start(line), line_end(t, line));
    let p = t
        .display_line(s)
        .stops()
        .iter()
        .rfind(|st| st.pos >= s && st.pos <= e && st.vcol <= col)
        .map_or(s, |st| st.pos);
    let mut c = Chars::at(t, p);
    let ws = |ch: Option<char>| matches!(ch, Some(' ' | '\t'));
    if !ws(c.ch()) {
        while c.pos() < e && !ws(c.ch()) {
            c.fwd();
        }
        if c.pos() == e {
            return None;
        }
    }
    while c.pos() < e && ws(c.ch()) {
        c.fwd();
    }
    Some(t.display_col(c.pos()))
}
