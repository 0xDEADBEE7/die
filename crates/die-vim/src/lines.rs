//! The ex commands on whole lines, as your Evil's `evil-ex-delete`, `evil-ex-yank`, `evil-ex-put`,
//! `evil-move`, `evil-copy`, `evil-ex-join`, `evil-shift-right` and `evil-shift-left` have them:
//! `:d`, `:y`, `:pu`, `:m`, `:t` and `:co`, `:j`, `:>` and `:<`, with the range `ex.rs` reads,
//! the cursor's line if none is given. `:sort` is in `sort.rs`.
//!
//! Each is one undo step. Before its first change the cursor goes where your Evil's `u` comes back
//! to, and after it where your Evil leaves it. None is what `.` repeats, as in your Evil, and under
//! `:g` each runs on each line in turn. `:g/pat/d` alone is quicker, as in your Evil: [`flush`].

use crate::grammar::{buffer_window, forget_goal};
use crate::operators::{self, Span};
use crate::substitute::{Address, Lines};
use crate::text::{indent_end, last_line, line_end, normal_pos, rest_pos};
use crate::{Vim, marks, registers, visual};
use die_core::{Args, BufferId, Ctx, Dir, Error, Origin, Pattern, Result, TextBuf, WindowId};
use std::ops::Range;

/// The lines a command takes, from 0, and whether the ex line named them, which moves your Evil's
/// cursor to the first one's start before the command runs.
pub struct Taken {
    pub b: BufferId,
    pub w: WindowId,
    pub first: usize,
    pub last: usize,
    pub named: bool,
}

fn msg(m: &str) -> Error {
    Error::Msg(m.into())
}

/// The lines `lines` names. Given backward, your Evil asks whether to swap them, and here it's
/// refused.
pub fn taken(ctx: &mut Ctx, lines: &Lines) -> Result<Taken> {
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let here = || -> Result<usize> {
        let t = ctx.text(b)?;
        Ok(t.line_of(t.cursor(w).unwrap_or(0)))
    };
    let (first, last) = match lines {
        Lines::Cursor => (here()?, here()?),
        Lines::All => (0, last_line(ctx.text(b)?)),
        Lines::Selection => {
            visual::last_lines(ctx, b)?.ok_or_else(|| msg("There's no selection yet"))?
        }
        Lines::Bytes(r) => {
            let t = ctx.text(b)?;
            (
                t.line_of(r.start),
                t.line_of(r.end.saturating_sub(1).max(r.start)),
            )
        }
        Lines::Between(from, to) => (from.line(ctx, b, w)?, to.line(ctx, b, w)?),
    };
    if first > last {
        return Err(msg("Backward range given"));
    }
    let named = !matches!(lines, Lines::Cursor);
    Ok(Taken {
        b,
        w,
        first,
        last,
        named,
    })
}

/// Whole lines `first..=last`, as `dd` takes them.
fn span(t: &TextBuf, first: usize, last: usize) -> Span {
    operators::lines(t, t.line_start(first), t.line_start(last))
}

/// `[x] [count]` after `:d` and `:y`, as your Evil's `evil-ex-get-optional-register-and-count`
/// reads them: one argument that's a number is the count, and anything else the register.
fn register_and_count(rest: &str) -> Result<(Option<char>, Option<usize>)> {
    let number = |s: &str| {
        let digits = s.strip_prefix('-').unwrap_or(s);
        digits.starts_with(|c: char| ('1'..='9').contains(&c))
            && digits.bytes().all(|c| c.is_ascii_digit())
    };
    let args: Vec<&str> = rest.split_whitespace().collect();
    let (register, count) = match args[..] {
        [] => (None, None),
        [n] if number(n) => (None, Some(n)),
        [r] => (Some(r), None),
        [r, n] => (Some(r), Some(n)),
        _ => return Err(msg("Invalid use")),
    };
    let register = match register.map(|r| (r.chars().next(), r.chars().count())) {
        None => None,
        Some((c, 1)) => c,
        Some(_) => return Err(msg("Invalid register")),
    };
    let count = match count {
        None => None,
        Some(n) if number(n) && !n.starts_with('-') => Some(n.parse().unwrap_or(usize::MAX)),
        Some(_) => return Err(msg("Invalid count")),
    };
    Ok((register, count))
}

/// The lines a count takes: that many from the range's last line, as Vim's.
fn counted(t: &TextBuf, r: &Taken, count: Option<usize>) -> (usize, usize) {
    match count {
        Some(n) => (r.last, r.last.saturating_add(n - 1).min(last_line(t))),
        None => (r.first, r.last),
    }
}

/// `:d [x] [count]`: the lines go into register x, or the kill ring, as `dd` takes them. The
/// cursor stays at the start of the range's first line, where the next line comes up.
pub fn delete(ctx: &mut Ctx, lines: &Lines, rest: &str) -> Result {
    let (register, count) = register_and_count(rest)?;
    let r = taken(ctx, lines)?;
    let t = ctx.text_mut(r.b)?;
    let (first, last) = counted(t, &r, count);
    let (start, span) = (t.line_start(r.first), span(t, first, last));
    t.set_cursor(r.w, span.beg);
    operators::cut(ctx, r.b, register, span)?;
    let t = ctx.text_mut(r.b)?;
    t.set_cursor(r.w, normal_pos(t, start.min(t.len())));
    Ok(())
}

/// `:y [x] [count]`: as `yy` takes the lines. Your Evil's cursor goes back where it was after a
/// range, to the selection's start if `:` was typed in visual state, and to the line's start after
/// no range.
pub fn yank(ctx: &mut Ctx, lines: &Lines, rest: &str) -> Result {
    let (register, count) = register_and_count(rest)?;
    let r = taken(ctx, lines)?;
    let t = ctx.text(r.b)?;
    let (first, last) = counted(t, &r, count);
    let saved = operators::taken(t, span(t, first, last));
    registers::save(ctx, register, saved, true)?;
    let from_visual = ctx.with_ext(|v: &mut Vim, _| v.ex_visual)?;
    let to = match lines {
        Lines::Cursor => Some(ctx.text(r.b)?.line_start(r.first)),
        _ if from_visual => ctx.text(r.b)?.mark('<'),
        _ => None,
    };
    let t = ctx.text_mut(r.b)?;
    forget_goal(t, r.w);
    if let Some(to) = to {
        t.set_cursor(r.w, normal_pos(t, to));
    }
    Ok(())
}

/// `:pu[!] [x]`: register x, or the newest kill, as whole lines below the range's last line, or
/// above it with `!`, whatever kind of text it holds. The cursor goes to the first non-blank of
/// the last line put. Unlike your Evil's, it leaves the register as it was (deviations.json).
pub fn put(ctx: &mut Ctx, lines: &Lines, above: bool, rest: &str) -> Result {
    let named: Vec<char> = rest.chars().filter(|&c| c != ' ').collect();
    let name = named.first().copied().unwrap_or('"');
    if named.len() > 1 && name != '=' {
        return Err(msg("Trailing characters"));
    }
    let r = taken(ctx, lines)?;
    let text = registers::read(ctx, Some(name))?.text;
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let t = ctx.text_mut(r.b)?;
    let (at, new, first) = if above {
        let at = t.line_start(r.last);
        (at, format!("{body}\n"), at)
    } else {
        let at = line_end(t, r.last);
        (at, format!("\n{body}"), at + 1)
    };
    // Where your Evil's `u` goes back to.
    t.set_cursor(r.w, at);
    t.tx(Origin::User).insert(at, new).commit()?;
    let end = first + body.len();
    marks::covered(t, first, end)?;
    let to = indent_end(t, t.line_of(end));
    t.set_cursor(r.w, rest_pos(t, to));
    Ok(())
}

/// The line `:m` and `:t` put lines after, from 1 with 0 before the first, read from what follows
/// the name as your Evil's `<addr>` reads it: an address, all of it.
fn address(ctx: &mut Ctx, r: &Taken, rest: &str) -> Result<usize> {
    match Address::parse(rest) {
        Some((a, "")) => a.number(ctx, r.b, r.w),
        _ => Err(msg("Invalid address")),
    }
}

/// Where the lines after line `n` (from 1) start: the end of the text past its last line.
fn after_line(t: &TextBuf, n: usize) -> usize {
    if n < t.line_count() {
        t.line_start(n)
    } else {
        t.len()
    }
}

/// Lines put at `at`, ending in a newline, which a last line without one gets first. Returns where
/// the last of them starts, which is where your Evil leaves the cursor.
fn put_lines(t: &mut TextBuf, at: usize, mut lines: String) -> Result<usize> {
    if !lines.ends_with('\n') {
        lines.push('\n');
    }
    let unended = at == t.len() && !t.is_empty() && t.rope().byte(at - 1) != b'\n';
    if unended {
        lines.insert(0, '\n');
    }
    let end = at + lines.len();
    t.tx(Origin::User).insert(at, lines).commit()?;
    Ok(t.line_start(t.line_of(end - 1)))
}

/// `:m address`: the lines go after that line, `:m0` to the top. A text without a final newline
/// keeps it that way, unless the lines go last, as in your Evil. Moved into itself, nothing moves.
/// To where they are, nothing moves either, and a range given leaves the cursor at its start.
pub fn move_to(ctx: &mut Ctx, lines: &Lines, rest: &str) -> Result {
    let r = taken(ctx, lines)?;
    let to = address(ctx, &r, rest)?;
    let t = ctx.text_mut(r.b)?;
    if to == r.first {
        if r.named {
            t.set_cursor(r.w, t.line_start(r.first));
        }
        return Ok(());
    }
    let span = span(t, r.first, r.last);
    let (beg, end) = (span.beg, span.end);
    let lines = t.rope().byte_slice(beg..end).to_string();
    let ended = t.is_empty() || t.rope().byte(t.len() - 1) == b'\n';
    let at = after_line(t, to);
    let mut at = match at {
        _ if at <= beg => at,
        _ if at >= end => at - (end - beg),
        _ => beg,
    };
    t.set_cursor(r.w, beg);
    t.tx(Origin::User).delete(beg..end).commit()?;
    if !ended && !t.is_empty() && t.rope().byte(t.len() - 1) == b'\n' {
        let len = t.len();
        t.tx(Origin::User).delete(len - 1..len).commit()?;
        at = at.min(t.len());
    }
    let to = put_lines(t, at, lines)?;
    t.set_cursor(r.w, to);
    Ok(())
}

/// `:t address` and `:co address`: a copy of the lines goes after that line.
pub fn copy_to(ctx: &mut Ctx, lines: &Lines, rest: &str) -> Result {
    let r = taken(ctx, lines)?;
    let to = address(ctx, &r, rest)?;
    let t = ctx.text_mut(r.b)?;
    let span = span(t, r.first, r.last);
    let lines = t.rope().byte_slice(span.beg..span.end).to_string();
    let at = after_line(t, to);
    t.set_cursor(r.w, at);
    let to = put_lines(t, at, lines)?;
    t.set_cursor(r.w, to);
    Ok(())
}

/// `:j[!] [count]`: the range's lines, or count lines from its last, joined as `J` joins them, or
/// with `!` as `gJ` does. With a count the cursor stays at the range's start, as in your Evil.
pub fn join(ctx: &mut Ctx, lines: &Lines, keep: bool, rest: &str) -> Result {
    let count = match rest {
        "" => None,
        n if n.starts_with(|c: char| ('1'..='9').contains(&c))
            && n.bytes().all(|c| c.is_ascii_digit()) =>
        {
            Some(n.parse().unwrap_or(usize::MAX))
        }
        _ => return Err(msg("Invalid count")),
    };
    let r = taken(ctx, lines)?;
    let (from, n) = match count {
        Some(n) => (r.last, n),
        None => (r.first, r.last - r.first + 1),
    };
    let t = ctx.text_mut(r.b)?;
    let start = t.line_start(from);
    // Where your Evil's `u` goes back to: just after the first newline the join takes.
    if from < last_line(t) {
        t.set_cursor(r.w, t.line_start(from + 1));
    }
    let a = Args {
        count: Some(n),
        buffer: Some(r.b),
        window: Some(r.w),
        ..Args::default()
    };
    operators::join_from(ctx, &a, start, keep)?;
    if count.is_some() {
        let t = ctx.text_mut(r.b)?;
        t.set_cursor(r.w, t.line_start(r.first));
    }
    Ok(())
}

/// `:>` and `:<`: one shift of the lines, as `>>` and `<<` make it. What follows the name is
/// ignored, as your Evil's count is outside visual state, and `:>>` is no command there. The
/// cursor keeps its column, or after a range goes to its first line's start.
pub fn shift(ctx: &mut Ctx, lines: &Lines, right: bool) -> Result {
    let r = taken(ctx, lines)?;
    let t = ctx.text_mut(r.b)?;
    forget_goal(t, r.w);
    let start = t.line_start(r.first);
    let from = match r.named {
        true => start,
        false => t.cursor(r.w).unwrap_or(start),
    };
    let span = span(t, r.first, r.last);
    t.set_cursor(r.w, start);
    operators::shift(t, r.w, span, right, Some((1, from)))
}

/// `:g/pat/d` and `:v/pat/d`, as your Evil's `evil--ex-performant-global-delete` runs them: the
/// lines go in one edit, into no register, and the last search stays as it was. The cursor goes to
/// the last match, or to where its line was if it went.
pub fn flush(ctx: &mut Ctx, lines: Range<usize>, pat: &Pattern, invert: bool) -> Result {
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let t = ctx.text_mut(b)?;
    let mut gone: Vec<Range<usize>> = vec![];
    let mut first_end = None;
    let mut start = lines.start;
    while start < lines.end {
        let next = t.line_start(t.line_of(start) + 1);
        let end = next - usize::from(t.rope().byte(next - 1) == b'\n');
        if t.first_match_in(pat, start..end).is_some() != invert {
            first_end.get_or_insert(next);
            match gone.last_mut() {
                Some(g) if g.end == start => g.end = next,
                _ => gone.push(start..next),
            }
        }
        start = next;
    }
    let found = t.search(pat, lines.end, Dir::Backward);
    let last = found
        .filter(|f| !f.wrapped && f.range.start >= lines.start)
        .map_or(lines.end, |f| f.range.start);
    let before: usize = gone.iter().filter(|g| g.end <= last).map(|g| g.len()).sum();
    let inside = gone.iter().find(|g| g.contains(&last)).map(|g| g.start);
    let to = inside.unwrap_or(last) - before;
    // Where your Evil's `u` goes back to: after the first line flushed, or the start of the first
    // run kept out.
    let undo_at = if invert {
        gone.first().map(|g| g.start)
    } else {
        first_end
    };
    if let Some(at) = undo_at {
        t.set_cursor(w, at);
        let tx = gone
            .into_iter()
            .fold(t.tx(Origin::User), |tx, g| tx.delete(g));
        tx.commit()?;
    }
    t.set_cursor(w, normal_pos(t, to.min(t.len())));
    Ok(())
}
