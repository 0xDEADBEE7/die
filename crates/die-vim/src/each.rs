//! `:norm` and `:g`: keys or an ex line on each line of a range, as your Evil's `evil-ex-normal`
//! and `evil-ex-global` run them. The lines are found first, each kept as a marker, so what an
//! earlier line adds or takes away doesn't move the rest. One `u` undoes the whole run.
//!
//! `:norm` feeds a line's keys and waits for them to run (`repeat.rs`), so the loop goes on in
//! [`advance`], after each command. A line whose keys fail ends there and the next one starts, as
//! your Evil's ignores the error. A typed `C-g` stops the whole run. `:g` runs its ex line on each
//! line in turn, which waits too when the line is a `:norm`, and the first error ends it.

use crate::grammar::buffer_window;
use crate::substitute::{self, Lines};
use crate::text::rest_pos;
use crate::{Vim, ex, operators, repeat, search};
use die_core::{Args, BufferId, Ctx, Error, Gravity, MarkerId, Pattern, Prefix, Result, State};
use die_core::{TextBuf, WindowId};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::time::Duration;

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
    /// While a line's keys run: how many fed keys are left once they have.
    running: Option<usize>,
    /// Whether a change the keys make is what `.` repeats after, and how many macros had run.
    top: bool,
    runs: usize,
    /// A prompt the line's keys left open has been sent `C-g`.
    cancelled: bool,
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
/// A match must lie within its line. The command runs from where the line's match starts, and
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
    search::remember_pattern(ctx, pat.clone())?;
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let lines = match lines {
        Lines::Cursor => Lines::All,
        lines => lines.clone(),
    };
    let range = substitute::line_range(ctx, b, w, lines)?;
    let t = ctx.text_mut(b)?;
    // Each line's first match, as `:s` finds them.
    let hits: HashMap<usize, usize> = t
        .substitutions(&pat, range.clone(), "", false)
        .into_iter()
        .map(|(m, _)| (t.line_of(m.start), m))
        .filter(|(line, m)| t.line_of(m.end) == *line)
        .map(|(line, m)| (line, m.start))
        .collect();
    let marks = markers(t, range.clone(), |t, line| {
        let hit = hits.get(&t.line_of(line.start));
        match (hit, invert) {
            (Some(&at), false) => Some(at),
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
fn parts(arg: &str) -> (String, Option<&str>) {
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

/// Start a run. Inside another, the one that's running takes it up.
fn start(
    ctx: &mut Ctx,
    buffer: BufferId,
    window: WindowId,
    marks: VecDeque<MarkerId>,
    what: Each,
) -> Result {
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
        cancelled: false,
    };
    if outer.is_none() {
        let stalled = ctx.idle(Duration::ZERO, |ctx| {
            ctx.defer(stalled);
            Ok(())
        });
        ctx.with_ext(|v: &mut Vim, _| v.stalled = Some(stalled))?;
    }
    ctx.with_ext(|v: &mut Vim, _| v.each.push(run))?;
    match outer {
        Some(_) => Ok(()),
        None => advance(ctx),
    }
}

/// After every command: if a line's keys may have run, see once what's queued has.
pub fn command_ended(ctx: &mut Ctx) -> Result {
    if ctx.with_ext(|v: &mut Vim, _| !v.each.is_empty())? {
        // After the call that says the keys were dropped, if they were.
        ctx.defer(advance);
    }
    Ok(())
}

/// The keys stopped. If a line's last key waits for another, as `:norm f` leaves it, that's over,
/// as when your Evil's run out: an Escape ends it, or isn't bound after it, and the line is over.
fn stalled(ctx: &mut Ctx) -> Result {
    let running = ctx.with_ext(|v: &mut Vim, _| v.each.last().and_then(|r| r.running))?;
    match running {
        Some(left) if repeat::fed_left(ctx)? <= left && ctx.prompt_text().is_none() => {
            repeat::feed_more(ctx, "<Esc>")
        }
        _ => Ok(()),
    }
}

/// What was fed is gone. After an error the line is over, and after a quit everything is.
pub fn dropped(ctx: &mut Ctx, quit: bool) -> Result {
    if quit {
        while abort(ctx) {}
        return Ok(());
    }
    advance(ctx)
}

/// Go on with the newest run, and the ones it was started from, until keys are left to run.
pub fn advance(ctx: &mut Ctx) -> Result {
    loop {
        let newest = ctx.with_ext(|v: &mut Vim, _| {
            let r = v.each.last()?;
            Some((r.buffer, r.window, r.running, r.top, r.runs, r.cancelled))
        })?;
        let Some((b, w, running, top, runs, cancelled)) = newest else {
            return Ok(());
        };
        if let Some(left) = running {
            if repeat::fed_left(ctx)? > left {
                return Ok(());
            }
            // Your Evil's prompt fails when the keys run out, so one left open is cancelled.
            if ctx.prompt_text().is_some() {
                if cancelled {
                    while abort(ctx) {}
                    return Err(Error::Msg("A prompt :norm left open won't close".into()));
                }
                ctx.with_ext(|v: &mut Vim, _| v.each.last_mut().map(|r| r.cancelled = true))?;
                return repeat::feed_more(ctx, "<C-g>");
            }
            // What can't be ended stays, and the next line goes ahead.
            let _ = end_line(ctx);
        }
        let next = ctx.with_ext(|v: &mut Vim, _| {
            let r = v.each.last_mut()?;
            (r.running, r.cancelled) = (None, false);
            Some((r.marks.pop_front()?, r.what.clone()))
        })?;
        let Some((m, what)) = next else {
            abort(ctx);
            continue;
        };
        let at = ctx.text_mut(b).ok().and_then(|t| t.remove_marker(m));
        let Some(at) = at.filter(|_| ctx.window_buffer(w) == Some(b)) else {
            // The buffer is gone, or the window shows another.
            abort(ctx);
            continue;
        };
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
                let depth = ctx.with_ext(|v: &mut Vim, _| v.each.len())?;
                if let Err(e) = ex::run(ctx, &line) {
                    // Your Evil's `:g` stops at an error, and so does a `:norm` it started.
                    while ctx.with_ext(|v: &mut Vim, _| v.each.len())? >= depth && abort(ctx) {}
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
    let none_left = ctx.with_ext(|v: &mut Vim, _| {
        let none = v.each.is_empty();
        if none {
            v.stalled = None;
        }
        none
    });
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
