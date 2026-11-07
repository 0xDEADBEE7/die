//! `:s`: its line range, its parts, and the case of what it writes. Patterns and replacements
//! are Rust's (D4); core's [`die_core::TextBuf::substitutions`] finds and expands them.
use crate::grammar::buffer_window;
use crate::search::{self, ignores_case};
use crate::text::{indent_end, next_stop, rest_pos};
use crate::{Vim, operators, visual};
use die_core::{Args, BufferId, Ctx, Error, Origin, Pattern, Result, Shape, WindowId};
use std::ops::Range;

/// What `:` puts on the line from a selection.
pub const VISUAL: &str = "'<,'>";

/// `&` and `:&`, as your Evil writes them: the last pattern, the last replacement, no flags.
const AGAIN: &str = "//~/";
/// `g&` and `:&&`: with the last flags.
const AGAIN_WITH_FLAGS: &str = "//~/&";

/// The lines `:s` looks at.
#[derive(Clone)]
pub enum Lines {
    Cursor,
    All,
    Selection,
    /// These bytes, from `&`.
    Bytes(Range<usize>),
}

/// A line that's `:s`: its range, and what follows the `s`, which starts with the delimiter.
pub fn parse(text: &str) -> Option<(Lines, &str)> {
    let (range, rest) = if let Some(rest) = text.strip_prefix('%') {
        (Lines::All, rest)
    } else if let Some(rest) = text.strip_prefix(VISUAL) {
        (Lines::Selection, rest)
    } else {
        (Lines::Cursor, text)
    };
    match rest {
        "&" => return Some((range, AGAIN)),
        "&&" => return Some((range, AGAIN_WITH_FLAGS)),
        _ => {}
    }
    let rest = rest.strip_prefix('s')?;
    // `-` and `_` carry on a command's name, such as `s-foo`.
    let delimiter = rest
        .chars()
        .next()
        .is_some_and(|c| !(c.is_alphanumeric() || c.is_whitespace() || "-_\\\"|".contains(c)));
    delimiter.then_some((range, rest))
}

/// Run `:s`, given its range and what follows the `s`. No match isn't an error, as in your Evil.
/// It's one undo step, and the cursor ends on the first non-blank of the last line it changed.
/// It shares its pattern with `n`, and an empty pattern is the last one. As in your Evil, it ends
/// the highlight of the last search.
pub fn run(ctx: &mut Ctx, lines: Lines, rest: &str) -> Result {
    substitute(ctx, lines, rest, true)
}

/// `:s`. `from_line` says it came from the `:` line: that's what makes your Evil's `u` go to the
/// first replacement's end, and from a key it goes back to where the key was pressed.
fn substitute(ctx: &mut Ctx, lines: Lines, rest: &str, from_line: bool) -> Result {
    let last = ctx.with_ext(|v: &mut Vim, _| v.substitute.clone())?;
    let (from, to, global) = parts(rest, &last)?;
    let pat = if from.is_empty() {
        let last = search::last_pattern(ctx)?;
        last.ok_or_else(|| Error::Msg(search::NONE_YET.into()))?
    } else {
        Pattern::new(&from, ignores_case(&from))?
    };
    ctx.with_ext(|v: &mut Vim, _| v.substitute = (to.clone(), global))?;
    let ignore_case = ignores_case(pat.as_str());
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let lines = line_range(ctx, b, w, lines)?;
    search::remember_pattern(ctx, pat.clone())?;
    ctx.set_search_highlight(None);
    let t = ctx.text_mut(b)?;
    let mut changes = t.substitutions(&pat, lines, &to, global);
    if changes.is_empty() {
        let from = pat.as_str();
        ctx.messages.push(format!("Pattern not found: {from}"));
        return Ok(());
    }
    if ignore_case {
        for (m, new) in &mut changes {
            let old = String::from(t.rope().byte_slice(m.clone()));
            *new = kept_case(&old, new);
        }
    }
    // Where the last replacement ends once every change has moved it.
    let moved: isize = changes
        .iter()
        .map(|(m, new)| new.len() as isize - m.len() as isize)
        .sum();
    let last = changes.last().map_or(0, |(m, _)| m.end);
    let last = last.saturating_add_signed(moved);
    // Where your Evil's `u` goes: the first replacement's end, as Emacs's undo chain leaves it.
    if from_line {
        t.set_cursor(w, rest_pos(t, changes.first().map_or(0, |(m, _)| m.end)));
    }
    let tx = t.tx(Origin::User);
    let tx = changes
        .into_iter()
        .fold(tx, |tx, (m, new)| tx.replace(m, new));
    tx.commit()?;
    let to = indent_end(t, t.line_of(last));
    t.set_cursor(w, rest_pos(t, to));
    Ok(())
}

/// `&`: `:s//~/` on this line, on count lines, or on what's selected. Unlike `:s` from the line,
/// it ends on the first non-blank whether or not anything matched. Before any pattern your Evil's
/// matches nothing but the empty string, which only moves the cursor. `.` doesn't repeat it.
pub fn again(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let lines = if visual::is_on(ctx, b) {
        let t = ctx.text(b)?;
        let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
        let (lo, hi) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        let lines = if sel.shape == Shape::Line {
            let span = operators::lines(t, lo, hi);
            span.beg..span.end
        } else {
            lo..next_stop(t, hi).unwrap_or(hi)
        };
        visual::escape(ctx, a)?;
        ctx.text_mut(b)?.set_cursor(w, lo);
        lines
    } else {
        let span = operators::this_line_and_more(ctx.text(b)?, w, a.count);
        span.beg..span.end
    };
    repeat_on(ctx, a, Lines::Bytes(lines), AGAIN)
}

/// `g&`: `:%s//~/&`. A count means nothing to it, and it ends any selection.
pub fn again_everywhere(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    if visual::is_on(ctx, b) {
        visual::escape(ctx, a)?;
    }
    repeat_on(ctx, a, Lines::All, AGAIN_WITH_FLAGS)
}

fn repeat_on(ctx: &mut Ctx, a: &Args, lines: Lines, rest: &str) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    if search::last_pattern(ctx)?.is_some() {
        substitute(ctx, lines, rest, false)?;
    } else {
        let range = line_range(ctx, b, w, lines)?;
        let t = ctx.text_mut(b)?;
        t.set_cursor(w, range.end.saturating_sub(1).max(range.start));
    }
    let t = ctx.text_mut(b)?;
    let line = t.line_of(t.cursor(w).unwrap_or(0));
    let to = indent_end(t, line);
    t.set_cursor(w, rest_pos(t, to));
    Ok(())
}

fn line_range(ctx: &mut Ctx, b: BufferId, w: WindowId, lines: Lines) -> Result<Range<usize>> {
    let (lo, hi) = match lines {
        Lines::All => return Ok(0..ctx.text(b)?.len()),
        Lines::Bytes(range) => return Ok(range),
        Lines::Selection => visual::last_lines(ctx, b)?
            .ok_or_else(|| Error::Msg("There's no selection yet".into()))?,
        Lines::Cursor => {
            let t = ctx.text(b)?;
            let line = t.line_of(t.cursor(w).unwrap_or(0));
            (line, line)
        }
    };
    let t = ctx.text(b)?;
    Ok(t.line_start(lo)..t.line_start(hi + 1))
}

/// `/from/to/flags`, whose delimiter is the first character. `\` before the delimiter makes it
/// part of the text, and a missing last delimiter is fine. `g` is the one flag, and `&` the last
/// one's. A replacement of `~` is the last replacement.
fn parts(rest: &str, last: &(String, bool)) -> Result<(String, String, bool)> {
    let mut chars = rest.chars();
    let delimiter = chars.next().unwrap_or('/');
    let mut pieces = vec![String::new()];
    while let Some(c) = chars.next() {
        if c == delimiter {
            pieces.push(String::new());
            continue;
        }
        let Some(piece) = pieces.last_mut() else {
            continue;
        };
        if c != '\\' {
            piece.push(c);
        } else if let Some(next) = chars.next() {
            if next != delimiter {
                piece.push('\\');
            }
            piece.push(next);
        } else {
            piece.push(c);
        }
    }
    let flags = pieces.get(2).map_or("", String::as_str);
    if pieces.len() > 3 || !matches!(flags, "" | "g" | "&") {
        return Err(Error::Msg(format!("Only the g flag is built: {rest}")));
    }
    let global = flags == "g" || (flags == "&" && last.1);
    let to = match pieces.get(1).map(String::as_str) {
        Some("~") => last.0.clone(),
        Some(to) => escapes(to),
        None => String::new(),
    };
    Ok((pieces.swap_remove(0), to, global))
}

/// `\n`, `\t` and `\\` in a replacement are those characters, as in your Evil. Any other backslash
/// is itself (D4).
fn escapes(to: &str) -> String {
    let mut out = String::with_capacity(to.len());
    let mut chars = to.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') | None => out.push('\\'),
            Some(other) => out.extend(['\\', other]),
        }
    }
    out
}

/// `new` in the case of the `old` it replaces, by the rules of Emacs's `replace-match`: all caps
/// stays all caps, and capitalized words stay capitalized.
fn kept_case(old: &str, new: &str) -> String {
    let (mut lower, mut upper, mut multiletter, mut plain_initial) = (false, false, false, false);
    let mut in_word = false;
    for c in old.chars() {
        if c.is_lowercase() {
            lower = true;
            plain_initial |= !in_word;
            multiletter |= in_word;
        } else if c.is_uppercase() {
            upper = true;
            multiletter |= in_word;
        } else {
            plain_initial |= !in_word;
        }
        in_word = c.is_alphanumeric();
    }
    if !lower && multiletter || !plain_initial && !multiletter && upper {
        return new.to_uppercase();
    }
    if plain_initial || !multiletter {
        return new.to_string();
    }
    let mut out = String::with_capacity(new.len());
    let mut in_word = false;
    for c in new.chars() {
        if in_word {
            out.push(c);
        } else {
            out.extend(c.to_uppercase());
        }
        in_word = c.is_alphanumeric();
    }
    out
}
