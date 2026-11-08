//! `:!cmd`, `:{range}!cmd`, `:r !cmd`, `:r file` and the `!` operator, as your Evil's
//! `evil-shell-command` and `evil-read`, on die-core's shell commands
//! ([`Ctx::shell_output`]). Without a range, `:!` is Emacs's `shell-command`. With one, the lines
//! go to the command and what it prints, errors too, takes their place, as one change. `:r !`
//! puts what it prints below the line. A command that outlasts core's wait finishes later, and if
//! the text has changed by then, it's left alone, as it is if what it printed isn't UTF-8.
use crate::ex::{abbreviates, absolute};
use crate::grammar::buffer_window;
use crate::operators::Span;
use crate::substitute::{Lines, line_range};
use crate::text::rest_pos;
use crate::{Vim, ex};
use die_core::process::Ran;
use die_core::{Args, BufferId, Ctx, Error, Origin, Result, WindowId};
use std::ops::Range;

/// `:r[ead]`'s shortest and whole name.
pub const NAME: (&str, &str) = ("r", "read");

/// An ex line's `!cmd` or `:r` after its range `range` names `lines`, or None if it's neither.
pub fn ex(ctx: &mut Ctx, lines: &Lines, range: &str, rest: &str) -> Option<Result> {
    if let Some(typed) = rest.strip_prefix('!') {
        return Some(bang(ctx, lines, typed));
    }
    let letters = rest.find(|c: char| !c.is_ascii_alphabetic());
    let (name, arg) = rest.split_at(letters.unwrap_or(rest.len()));
    let named = !name.is_empty() && abbreviates(name, NAME) && !arg.starts_with(['-', '_']);
    // `:0r` reads in above the first line, as your Evil's count of 0 does.
    named.then(|| read(ctx, lines, range.trim() == "0", arg.trim()))
}

/// `:!`: Emacs's `shell-command`, or with a range, the lines through the command.
fn bang(ctx: &mut Ctx, lines: &Lines, typed: &str) -> Result {
    let line = command(ctx, typed)?;
    let (b, w) = buffer_window(ctx, &Args::default())?;
    if matches!(lines, Lines::Cursor) {
        // Your Evil's is an operator, which goes to its line's start first.
        let t = ctx.text_mut(b)?;
        let start = t.line_start(t.line_of(t.cursor(w).unwrap_or(0)));
        t.set_cursor(w, start);
        return ctx.shell_command(&line);
    }
    let range = line_range(ctx, b, w, lines.clone())?;
    let input = ctx.text(b)?.rope().byte_slice(range.clone()).to_string();
    run(ctx, b, w, &line, Some(input), range, false)
}

/// The command a `:!` line runs, as your Evil reads it: `!` alone is the last one again, and `%`
/// and `#` are this buffer's file and the other buffer's, unless `\` comes before.
fn command(ctx: &mut Ctx, typed: &str) -> Result<String> {
    let (again, typed) = match typed.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, typed.trim()),
    };
    if typed.is_empty() {
        let last = ctx
            .with_ext(|v: &mut Vim, _| v.shell.clone())?
            .filter(|_| again);
        let what = if again { " previous" } else { "" };
        return last.ok_or_else(|| Error::Msg(format!("No{what} shell command")));
    }
    let (b, _) = buffer_window(ctx, &Args::default())?;
    let name = |b: Option<BufferId>| Some(ctx.file(b?)?.path.display().to_string());
    let others = ctx.buffer_list().into_iter().filter(|o| *o != b);
    let (here, other) = (name(Some(b)), name(others.into_iter().next()));
    let mut line = String::new();
    let mut chars = typed.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, &here, &other) {
            ('\\', ..) if matches!(chars.peek(), Some('%' | '#')) => line.extend(chars.next()),
            ('%', Some(file), _) | ('#', _, Some(file)) => line += file,
            (c, ..) => line.push(c),
        }
    }
    let kept = line.clone();
    ctx.with_ext(|v: &mut Vim, _| v.shell = Some(kept))?;
    Ok(line)
}

/// `:r`: below line `lines` ends on, or above the first with `zero`, what a `!` command prints,
/// or a file's text.
fn read(ctx: &mut Ctx, lines: &Lines, zero: bool, arg: &str) -> Result {
    if arg.is_empty() {
        return Ok(());
    }
    let (b, w) = buffer_window(ctx, &Args::default())?;
    let at = match zero {
        true => 0,
        false => line_range(ctx, b, w, lines.clone())?.end,
    };
    let Some(typed) = arg.strip_prefix('!') else {
        let path = absolute(ctx.directory(b), arg);
        let text = std::fs::read_to_string(&path);
        let text = text.map_err(|e| Error::Msg(format!("{}: {e}", path.display())))?;
        return put(ctx, b, w, at..at, &text, false);
    };
    let line = command(ctx, typed)?;
    run(ctx, b, w, &line, None, at..at, true)
}

/// Run `line` with `input`, and put what it prints in place of `range` of `b`, if the text is as
/// it was, as [`put`] puts it.
fn run(
    ctx: &mut Ctx,
    b: BufferId,
    w: WindowId,
    line: &str,
    input: Option<String>,
    range: Range<usize>,
    last: bool,
) -> Result {
    let rev = ctx.text(b)?.rev();
    let dir = absolute(ctx.directory(b), "");
    let what = line.to_string();
    ctx.shell_output(&dir, line, input, move |ctx, ran: Ran| {
        ran.status.map_err(|e| Error::Msg(e.to_string()))?;
        // As DIE edits only UTF-8, bytes that aren't don't go in as replacements.
        let refused = [
            (ran.cut, "printed more than DIE keeps"),
            (ran.lossy, "printed bytes that aren't UTF-8"),
        ];
        if let Some((_, why)) = refused.iter().find(|r| r.0) {
            let why = format!("`{what}` {why}, so the text is as it was");
            return Err(Error::Msg(why));
        }
        if ctx.text(b)?.rev() != rev {
            let why =
                format!("The text changed while `{what}` ran, so what it printed isn't in it");
            return Err(Error::Msg(why));
        }
        put(ctx, b, w, range, &ran.text, last)
    })
}

/// Put `text` in place of `range`, as one change, the cursor on its first line, or with `last` its
/// last. An empty range is `:r`'s, which adds a newline before past a last line with none, and
/// after unless the text ends in one, as your Evil's does.
fn put(
    ctx: &mut Ctx,
    b: BufferId,
    w: WindowId,
    range: Range<usize>,
    text: &str,
    last: bool,
) -> Result {
    let t = ctx.text_mut(b)?;
    let reads = range.is_empty();
    let unended =
        range.start == t.len() && t.rope().get_byte(t.len().wrapping_sub(1)) != Some(b'\n');
    let lead = if reads && unended { "\n" } else { "" };
    let tail = if reads && !text.is_empty() && !text.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    let new = format!("{lead}{text}{tail}");
    let end = range.start + new.len();
    let shown = ctx.window_buffer(w) == Some(b);
    let t = ctx.text_mut(b)?;
    // Where `u` puts the cursor back, as your Evil's undo goes to where the text went.
    if shown {
        t.set_cursor(w, range.start);
    }
    t.tx(Origin::User).replace(range.clone(), new).commit()?;
    let at = match last {
        true => t.line_start(t.line_of(end).saturating_sub(1)),
        false => range.start + lead.len(),
    };
    if shown {
        t.set_cursor(w, rest_pos(t, at));
    }
    Ok(())
}

/// The `!` operator: `:` with its lines and a `!`, as your Evil's opens it, the cursor on the
/// first of them.
pub fn prompt(ctx: &mut Ctx, b: BufferId, w: WindowId, span: Span) -> Result {
    let t = ctx.text_mut(b)?;
    let lines = t.line_of(span.end.saturating_sub(1).max(span.beg)) - t.line_of(span.beg) + 1;
    t.set_cursor(w, span.beg);
    let a = Args {
        count: Some(lines),
        ..Args::default()
    };
    ex::open_with(ctx, &a, "!")
}
