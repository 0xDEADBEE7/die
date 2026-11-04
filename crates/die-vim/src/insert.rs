//! Insert state. A session runs from entering insert state to Escape: it's one
//! undo step, it records its edits so a count can replay them, and it indents new
//! lines the way your Evil does in text buffers (like the line above).

use crate::grammar::buffer_window;
use crate::motions::word_back;
use crate::text::{
    Chars, blank_line, indent_end, indentation, line_end, next_stop, normal_pos, pos_at_col,
    prev_stop,
};
use die_core::{Args, Ctx, Error, Origin, Result, State, TextBuf, WindowId};

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
}

pub struct Session {
    pub entry: Entry,
    pub count: usize,
    pub edits: Vec<Edit>,
    /// Escape empties the line if it holds only indentation. Entering by `a`, `A`,
    /// `o` or `O` sets it, a newline keeps it, and any other edit clears it, as
    /// Evil's `evil-maybe-remove-spaces` does.
    pub drop_indent: bool,
}

impl Session {
    /// A session for insert state entered some other way.
    pub fn bare() -> Session {
        Session {
            entry: Entry::Insert,
            count: 1,
            edits: vec![],
            drop_indent: false,
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
        Entry::InsertLine => {
            t.set_cursor(w, indent_end(t, line));
        }
        Entry::AppendLine => {
            t.set_cursor(w, line_end(t, line));
        }
        Entry::OpenBelow => open_below(t, w, line)?,
        Entry::OpenAbove => {
            let s = t.line_start(line);
            t.tx(Origin::User).insert(s, "\n").commit()?;
            indent_from_above(t, w, line)?;
        }
    }
    // Only now, so that a failed `o` or `O` leaves no group open. The line it
    // opened joins the group: core adds an earlier edit by the same command.
    t.begin_undo_group();
    ctx.set_state_of(b, State::Insert);
    Ok(Session {
        entry,
        count: a.count.unwrap_or(1).max(1),
        edits: vec![],
        drop_indent: entry != Entry::Insert && entry != Entry::InsertLine,
    })
}

/// Apply one insert-state edit and record it.
pub fn edit(ctx: &mut Ctx, a: &Args, s: &mut Session, e: Edit) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    apply(ctx.text_mut(b)?, w, &e)?;
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
        r = (1..s.count).try_for_each(|_| {
            if matches!(s.entry, Entry::OpenBelow | Entry::OpenAbove) {
                let line = t.line_of(cursor(t));
                open_below(t, w, line)?;
            }
            s.edits.iter().try_for_each(|e| apply(t, w, e))
        });
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
    let after_final_newline = back == t.len() && t.line_start(t.line_of(back)) == back;
    t.set_cursor(
        w,
        if after_final_newline {
            back
        } else {
            normal_pos(t, back)
        },
    );
    ctx.set_state_of(b, State::Normal);
    r
}

fn apply(t: &mut TextBuf, w: WindowId, e: &Edit) -> Result {
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    let line = t.line_of(p);
    let ls = t.line_start(line);
    let fail = |m: &str| Err(Error::Msg(m.into()));
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
            let q = if p == ls {
                prev_char(t, p)
            } else {
                word_back(t, p, false).map(|q| q.max(ls))
            };
            let Some(q) = q else {
                return fail("Beginning of buffer");
            };
            t.tx(Origin::User).delete(q..p).commit()?;
            q
        }
        Edit::Tab => return tab(t, w, p),
        Edit::Left => match prev_stop(t, p) {
            Some(q) => q,
            None => return fail("Beginning of buffer"),
        },
        Edit::Right => match next_stop(t, p) {
            Some(q) => q,
            None => return fail("End of buffer"),
        },
        Edit::Up | Edit::Down => {
            let next = if *e == Edit::Up {
                line.checked_sub(1)
            } else {
                Some(line + 1)
            };
            let Some(l) = next.filter(|&l| l < t.line_count()) else {
                return fail("No line there");
            };
            pos_at_col(t, l, t.display_col(p))
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

/// Give `line` the indentation of the line above it, in spaces, or none if that
/// line is blank or there is none (Emacs's `indent-according-to-mode` in
/// fundamental-mode). The cursor goes to the end of the indentation.
fn indent_from_above(t: &mut TextBuf, w: WindowId, line: usize) -> Result {
    let col = match line.checked_sub(1) {
        Some(above) if !blank_line(t, above) => indentation(t, above),
        _ => 0,
    };
    let (s, end) = (t.line_start(line), indent_end(t, line));
    let indent = " ".repeat(col);
    if t.rope().byte_slice(s..end) != indent.as_str() {
        t.tx(Origin::User).replace(s..end, indent).commit()?;
    }
    t.set_cursor(w, s + col);
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
        .stops
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
