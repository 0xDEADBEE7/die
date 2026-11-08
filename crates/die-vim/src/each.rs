//! `:norm` and `:g`: keys or an ex line on each line of a range, as your Evil's `evil-ex-normal`
//! and `evil-ex-global` run them. The lines are found first, each kept as a marker, so what an
//! earlier line adds or takes away doesn't move the rest. One `u` undoes the whole run.
//!
//! `:norm` feeds a line's keys kept apart from what's fed behind them (`repeat.rs`), as your Evil
//! runs each line's keyboard macro inside `ignore-errors`, and goes on in [`advance`] when die-core
//! says they've run out or failed. A line whose keys fail ends there and the next one starts, and
//! what fed the `:norm`, a macro or another `:norm`'s line, goes on after it. A typed `C-g` stops
//! the whole run. `:g` runs its ex line on each line in turn, which waits too when the line is a
//! `:norm`, and the first error ends it.

use crate::grammar::buffer_window;
use crate::substitute::{self, Lines};
use crate::text::rest_pos;
use crate::{Vim, ex, operators, repeat, search};
use die_core::{Args, BufferId, Ctx, Error, Gravity, MarkerId, Pattern, Prefix, Result, State};
use die_core::{TextBuf, WindowId};
use std::collections::VecDeque;
use std::ops::Range;

/// How deep `:norm` and `:g` can start inside each other, as in `:norm @a` with `a` holding
/// `:norm @a`. Your Evil's run out of Lisp nesting, and nothing else would end these.
const MAX_DEPTH: usize = 100;

const NORMAL: (&str, &str) = ("norm", "normal");
const GLOBAL: (&str, &str) = ("g", "global");
const VGLOBAL: (&str, &str) = ("v", "vglobal");
/// Their shortest and whole names, for `:`'s list.
pub const NAMES: [(&str, &str); 3] = [NORMAL, GLOBAL, VGLOBAL];

/// One `:norm` or `:g`, with the lines it has still to do.
pub struct Run {
    buffer: BufferId,
    window: WindowId,
    /// Where each line's turn starts, in order.
    marks: VecDeque<MarkerId>,
    what: Each,
    /// While a line's keys run: how many fed keys are behind them, to run once they have.
    running: Option<usize>,
    /// Whether a change the keys make is what `.` repeats after, and how many macros had run.
    top: bool,
    runs: usize,
    /// Keys fed after the line's own ran out, to end what they left waiting.
    tries: u8,
    /// From `@:` with a count: its ex line, and how many more times to run it once this is over.
    again: Option<(String, usize)>,
}

#[derive(Clone)]
enum Each {
    /// `:norm`'s keys, in notation.
    Keys(String),
    /// `:g`'s ex line.
    Ex(String),
}

/// `:norm`, `:g` and `:v` with what follows their range, or None for another command.
pub fn ex(ctx: &mut Ctx, lines: &Lines, rest: &str) -> Option<Result> {
    let letters = rest.find(|c: char| !c.is_ascii_alphabetic());
    let (name, after) = rest.split_at(letters.unwrap_or(rest.len()));
    // `-` and `_` carry on a command's name, as in `g-foo`.
    if after.starts_with(['-', '_']) {
        return None;
    }
    let (bang, after) = after
        .strip_prefix('!')
        .map_or((false, after), |after| (true, after));
    let is = |n| ex::abbreviates(name, n);
    let invert = match () {
        // Vim's `!` leaves out mappings. Your Evil has no bang here and types it as a key.
        _ if is(NORMAL) => None,
        _ if is(GLOBAL) => Some(bang),
        _ if is(VGLOBAL) => Some(!bang),
        _ => return None,
    };
    if ctx.with_ext(|v: &mut Vim, _| v.each.len()).ok()? >= MAX_DEPTH {
        return Some(Err(Error::Msg(
            "Too many :norm and :g inside each other".into(),
        )));
    }
    // In normal state, as your Evil's start.
    Some(end_line(ctx).and_then(|()| match invert {
        None => normal(ctx, lines, after.trim_start()),
        Some(invert) => global(ctx, lines, after, invert),
    }))
}

/// `:norm keys`: the keys, each character one key, on each line from its start, the cursor's line
/// if no range is given.
fn normal(ctx: &mut Ctx, lines: &Lines, keys: &str) -> Result {
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let range = substitute::line_range(ctx, b, w, lines.clone())?;
    let marks = markers(ctx.text_mut(b)?, range, |_, line| Some(line.start))?;
    start(ctx, b, w, marks, Each::Keys(repeat::text_keys(keys)))
}

/// `:g/pattern/command`, or with `invert` the lines that don't match. Any delimiter will do, an
/// empty pattern is the last search's, and the pattern becomes the last search, as in your Evil.
/// A match must end by its line's end. The command runs from where the line's match starts, and
/// with no line to run on the cursor goes to the range's end, as your Evil leaves it.
fn global(ctx: &mut Ctx, lines: &Lines, arg: &str, invert: bool) -> Result {
    let (from, command) = parts(arg.trim_start());
    let pat = if from.is_empty() {
        let last = search::last_pattern(ctx)?;
        last.ok_or_else(|| Error::Msg("No previous pattern".into()))?
    } else {
        Pattern::new(&from, search::ignores_case(&from))?
    };
    // Your Evil has no default command, and one with no name, as `:g/a/3`, fails there too.
    let named = command.is_some_and(|c| !substitute::range(c.trim_start()).1.trim().is_empty());
    let command = command
        .filter(|_| named)
        .ok_or_else(|| Error::Msg("No command given".into()))?;
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let lines = match lines {
        Lines::Cursor => Lines::All,
        lines => lines.clone(),
    };
    let range = substitute::line_range(ctx, b, w, lines)?;
    // `:g/pat/d` alone, as your Evil runs it.
    let (on, after) = substitute::range(command.trim());
    if matches!(on, Lines::Cursor) && !after.is_empty() && ex::abbreviates(after, ("d", "delete")) {
        return crate::lines::flush(ctx, range, &pat, invert);
    }
    search::remember_pattern(ctx, pat.clone())?;
    let t = ctx.text_mut(b)?;
    // Each line's first match ending by the line's end, as your Evil's `re-search-forward` finds
    // it, bounded there.
    let marks = markers(t, range.clone(), |t, line| {
        let hit = t.first_match_in(&pat, line.clone());
        match (hit, invert) {
            (Some(m), false) => Some(m.start),
            (None, true) => Some(line.start),
            _ => None,
        }
    })?;
    if marks.is_empty() && !range.is_empty() {
        // Not on the empty line after a final newline, as anywhere in die-vim (deviations.json).
        let past = range.end == t.len() && !t.is_empty() && t.rope().byte(t.len() - 1) == b'\n';
        t.set_cursor(w, rest_pos(t, range.end - usize::from(past)));
    }
    start(ctx, b, w, marks, Each::Ex(command.to_string()))
}

/// What `:g` reads after its name: the pattern, up to the delimiter again, and what follows that,
/// if it's there. A backslash keeps the delimiter in the pattern.
pub(crate) fn parts(arg: &str) -> (String, Option<&str>) {
    let mut chars = arg.char_indices();
    let Some((_, delimiter)) = chars.next() else {
        return (String::new(), None);
    };
    let mut from = String::new();
    while let Some((i, c)) = chars.next() {
        if c == delimiter {
            return (from, Some(&arg[i + c.len_utf8()..]));
        }
        if c == '\\'
            && let Some((_, next)) = chars.next()
        {
            if next != delimiter {
                from.push('\\');
            }
            from.push(next);
        } else {
            from.push(c);
        }
    }
    (from, None)
}

/// A marker on each line of the whole lines `range` where `at` gives a place, with the line's
/// bytes, newline left out.
fn markers(
    t: &mut TextBuf,
    range: Range<usize>,
    at: impl Fn(&TextBuf, Range<usize>) -> Option<usize>,
) -> Result<VecDeque<MarkerId>> {
    let mut marks = VecDeque::new();
    let mut start = range.start;
    while start < range.end {
        let next = t.line_start(t.line_of(start) + 1);
        let end = if t.rope().byte(next - 1) == b'\n' {
            next - 1
        } else {
            next
        };
        if let Some(p) = at(t, start..end) {
            marks.push_back(t.add_marker(p, Gravity::Left)?);
        }
        start = next;
    }
    Ok(marks)
}

/// Start a run, and take it as far as its first line's keys. Inside another, that's before the
/// key that started it is over, so an error in a `:g` a `:norm` line started is that line's.
fn start(
    ctx: &mut Ctx,
    buffer: BufferId,
    window: WindowId,
    marks: VecDeque<MarkerId>,
    what: Each,
) -> Result {
    let depth = ctx.with_ext(|v: &mut Vim, _| v.each.len())?;
    let outer = ctx.with_ext(|v: &mut Vim, _| v.each.first().map(|r| (r.top, r.runs)))?;
    let (top, runs) = match outer {
        Some(outer) => outer,
        None => repeat::begin_lines(ctx, buffer)?,
    };
    let run = Run {
        buffer,
        window,
        marks,
        what,
        running: None,
        top,
        runs,
        tries: 0,
        again: None,
    };
    ctx.with_ext(|v: &mut Vim, _| v.each.push(run))?;
    advance(ctx, depth)
}

/// `@:` with a count: the ex line `n` times, each after the last is over, as your Evil's run one
/// after another. A `:norm` or `:g` that waits for its keys runs the rest when it's done.
pub fn repeat_ex(ctx: &mut Ctx, line: &str, n: usize) -> Result {
    for more in (0..n).rev() {
        let depth = ctx.with_ext(|v: &mut Vim, _| v.each.len())?;
        ex::run(ctx, line)?;
        let again = Some((line.to_string(), more));
        let waits =
            ctx.with_ext(|v: &mut Vim, _| v.each.get_mut(depth).map(|r| r.again = again))?;
        if waits.is_some() {
            break;
        }
    }
    Ok(())
}

/// The newest line's keys have all run (contract §6). What they left waiting ends as when your
/// Evil's run out: a key waiting for another gets an Escape, and a prompt takes its line as it is,
/// then a `C-g` if it's still open. Then the line is over.
pub fn ran_out(ctx: &mut Ctx) -> Result {
    let tries = ctx.with_ext(|v: &mut Vim, _| {
        let r = v.each.last()?;
        r.running.map(|_| r.tries)
    })?;
    let Some(tries) = tries else {
        return Ok(());
    };
    let key = match () {
        _ if ctx.prompt_text().is_some() && tries == 0 => "<CR>",
        _ if ctx.prompt_text().is_some() => "<C-g>",
        _ if ctx.keys_pending() => "<Esc>",
        _ => return line_over(ctx),
    };
    if tries >= 2 {
        while abort(ctx) {}
        return Err(Error::Msg(
            "What a :norm line left waiting won't end".into(),
        ));
    }
    ctx.with_ext(|v: &mut Vim, _| v.each.last_mut().map(|r| r.tries += 1))?;
    repeat::feed_apart(ctx, key)
}

/// While a line's keys run, how many fed keys are behind them.
pub fn behind(ctx: &mut Ctx) -> Result<Option<usize>> {
    Ok(ctx.with_ext(|v: &mut Vim, _| v.each.last().and_then(|r| r.running))?)
}

/// What was fed is gone. After an error the line is over, and after a quit everything is.
pub fn dropped(ctx: &mut Ctx, quit: bool) -> Result {
    if quit {
        while abort(ctx) {}
        return Ok(());
    }
    line_over(ctx)
}

/// The newest run's line is over: back to normal state, and on to the next.
fn line_over(ctx: &mut Ctx) -> Result {
    let was = ctx.with_ext(|v: &mut Vim, _| {
        let r = v.each.last_mut()?;
        r.tries = 0;
        r.running.take()
    })?;
    if was.is_none() {
        return Ok(());
    }
    // What can't be ended stays, and the next line goes ahead.
    let _ = end_line(ctx);
    advance(ctx, 0)
}

/// Go on with the newest run, and the ones it was started from down to `depth` of them, until a
/// line's keys are left to run.
fn advance(ctx: &mut Ctx, depth: usize) -> Result {
    loop {
        let newest = ctx.with_ext(|v: &mut Vim, _| {
            let r = v.each.last().filter(|_| v.each.len() > depth)?;
            Some((r.buffer, r.window, r.running.is_some(), r.top, r.runs))
        })?;
        let Some((b, w, running, top, runs)) = newest else {
            return Ok(());
        };
        if running {
            return Ok(());
        }
        let next = ctx.with_ext(|v: &mut Vim, _| {
            let r = v.each.last_mut()?;
            Some(match r.marks.pop_front() {
                Some(m) => Ok((m, r.what.clone())),
                None => Err(r.again.take()),
            })
        })?;
        let (m, what) = match next {
            Some(Ok(next)) => next,
            Some(Err(again)) => {
                // It's over, and `@:` with a count may run its line again.
                abort(ctx);
                if let Some((line, n)) = again.filter(|a| a.1 > 0) {
                    repeat_ex(ctx, &line, n)?;
                }
                continue;
            }
            None => return Ok(()),
        };
        let at = ctx.text_mut(b).ok().and_then(|t| t.remove_marker(m));
        let Some(at) = at.filter(|_| ctx.window_buffer(w) == Some(b)) else {
            // The buffer is gone, or the window shows another.
            abort(ctx);
            continue;
        };
        let was = ctx.text(b)?.cursor(w);
        ctx.text_mut(b)?.set_cursor(w, at);
        match what {
            Each::Keys(keys) => {
                let left = repeat::feed_line(ctx, b, &keys, top, runs)?;
                ctx.with_ext(|v: &mut Vim, _| {
                    if let Some(r) = v.each.last_mut() {
                        r.running = Some(left);
                    }
                })?;
                return Ok(());
            }
            Each::Ex(line) => {
                let own = ctx.with_ext(|v: &mut Vim, _| v.each.len())?;
                if let Err(e) = ex::run(ctx, &line) {
                    // Your Evil's reads the command before the cursor moves, so one it doesn't
                    // know leaves the cursor be.
                    if let (Error::NoCommand(_), Some(was)) = (&e, was) {
                        ctx.text_mut(b)?.set_cursor(w, was);
                    }
                    // Your Evil's `:g` stops at an error, and so does a `:norm` it started.
                    while ctx.with_ext(|v: &mut Vim, _| v.each.len())? >= own && abort(ctx) {}
                    return Err(e);
                }
            }
        }
    }
}

/// End the newest run, and its markers with it. False if none was running.
fn abort(ctx: &mut Ctx) -> bool {
    let Ok(Some(run)) = ctx.with_ext(|v: &mut Vim, _| v.each.pop()) else {
        return false;
    };
    if let Ok(t) = ctx.text_mut(run.buffer) {
        for m in run.marks {
            t.remove_marker(m);
        }
    }
    let none_left = ctx.with_ext(|v: &mut Vim, _| v.each.is_empty());
    if none_left.unwrap_or(true) {
        let _ = repeat::end_lines(ctx);
    }
    true
}

/// Back to normal state, as your Evil's `evil-force-normal-state` after each line, whatever the
/// keys left waiting: a session ends without being what `.` repeats, and a count goes.
fn end_line(ctx: &mut Ctx) -> Result {
    ctx.set_prefix(Prefix::default());
    let Some(b) = ctx.focused_buffer() else {
        return Ok(());
    };
    let a = Args {
        buffer: Some(b),
        window: ctx.focused(),
        ..Default::default()
    };
    match ctx.state_of(b) {
        State::OperatorPending => operators::cancel(ctx, &a),
        State::CommandLine if ctx.prompt_text().is_none() => repeat::unwait(ctx, b),
        _ => repeat::to_normal(ctx),
    }
}
