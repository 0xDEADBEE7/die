//! The control keys of insert and replace state that aren't plain edits, as your Evil's
//! `evil-insert-state-map` has them: `C-o`, `C-r`, `C-a`, `C-@`, `C-v`, `C-q`, `C-y`, `C-e`, and
//! the special case of `C-d`. `C-p` and `C-n` are in `complete.rs`.
//!
//! **`C-o`** ends the insert step: the session's count is dropped, its other lines get what
//! was typed, and `.` keeps its keys so far. [`after_command`] goes back when the next
//! command is over, one that didn't leave an operator, a count or fed keys waiting.

use crate::grammar::{Goal, buffer_window};
use crate::insert::{self, Edit, Session};
use crate::registers;
use crate::repeat::{self, Change};
use crate::text::{line_end, next_stop};
use crate::{Vim, end_session, insert_edit, visual};
use die_core::{Args, BufferId, Ctx, Error, Key, KeyEvent, Prefix, Result, State, key_notation};

/// A `C-o` waiting for its command.
pub struct Once {
    /// The state to go back to.
    back: State,
    /// The line whose end the cursor was past, if it was.
    eol_line: Option<usize>,
    /// The next command to finish isn't `C-o` itself.
    armed: bool,
}

/// The last `C-y` or `C-e`.
pub struct LastCopy {
    at: u64,
    above: bool,
    n: usize,
}

fn session(ctx: &mut Ctx, b: BufferId) -> Result<Session> {
    let s = ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
    Ok(s.unwrap_or_else(Session::bare))
}

fn put_session(ctx: &mut Ctx, b: BufferId, s: Session) -> Result {
    ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
    Ok(())
}

/// `C-o`: as Evil's `evil-execute-in-normal-state`.
pub fn execute_normal(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let back = ctx.state_of(b);
    let t = ctx.text(b)?;
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    let line = t.line_of(p);
    let eol_line = (p == line_end(t, line)).then_some(line);
    let mut s = session(ctx, b)?;
    if let Some(mut c) = s.repeat.take() {
        c.keys.push_str(&key_notation(&a.keys));
        repeat::commit(ctx, c)?;
    }
    let r = insert::pause(ctx, a, &mut s);
    put_session(ctx, b, s)?;
    // At the end of a `.` or a macro, Evil counts that as the command it waited for.
    let once = Once {
        back,
        eol_line,
        armed: repeat::replay_ending(ctx)?,
    };
    ctx.with_ext(|v: &mut Vim, _| v.once.insert(b, once))?;
    r
}

/// Run a motion. One run under `C-o` is what `.` repeats, as in your Evil, unless it's an
/// operator's.
pub fn motion(ctx: &mut Ctx, a: &Args, run: impl FnOnce(&mut Ctx) -> Result) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let alone = ctx.state_of(b) == State::Normal
        && ctx.with_ext(|v: &mut Vim, _| v.once.contains_key(&b))?;
    run(ctx)?;
    if alone {
        repeat::commit(ctx, Change::of(a))?;
    }
    Ok(())
}

/// After every command: if it's the one `C-o` was waiting for, go back.
pub fn after_command(ctx: &mut Ctx, a: &Args, r: &Result) {
    // A failure here is one the command's own result already covers.
    let _ = back_from_once(ctx, a, r);
}

fn back_from_once(ctx: &mut Ctx, a: &Args, r: &Result) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let state = ctx.state_of(b);
    let armed = ctx.with_ext(|v: &mut Vim, _| {
        v.once
            .get_mut(&b)
            .map(|o| std::mem::replace(&mut o.armed, true))
    })?;
    if armed != Some(true) {
        return Ok(());
    }
    // Evil's `h` and `l` signal as if they'd never run, so the wait goes on.
    let never_ran =
        matches!(r, Err(Error::Msg(m)) if m == "End of line" || m == "Beginning of line");
    let waiting = matches!(state, State::OperatorPending | State::CommandLine)
        || ctx.prefix() != Prefix::default()
        || never_ran
        || repeat::feeding(ctx)?;
    if waiting {
        return Ok(());
    }
    let Some(once) = ctx.with_ext(|v: &mut Vim, _| v.once.remove(&b))? else {
        return Ok(());
    };
    if matches!(state, State::Insert | State::Replace) {
        // It started a session of its own. Evil records nothing typed after a `C-o`.
        let mut s = session(ctx, b)?;
        if let Some(mut c) = s.repeat.take() {
            c.keys.push_str("<Esc>");
            repeat::commit(ctx, c)?;
        }
        s.count = 1;
        return put_session(ctx, b, s);
    }
    if state == State::Visual {
        visual::escape(ctx, a)?;
    }
    let t = ctx.text(b)?;
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let (p, line) = (sel.head, t.line_of(sel.head));
    let eol = line_end(t, line);
    // Past the end again if it was before, or if `$` took it there: Evil's normal state
    // hasn't stepped back yet when it goes back.
    let was_past = once.eol_line == Some(line)
        && next_stop(t, p) == Some(eol)
        && !matches!(a.char(), Some('0' | '^'));
    let pos = if Goal::decode(sel.goal) == Some(Goal::Eol) || was_past {
        eol
    } else {
        p
    };
    insert::resume(ctx, b, w, once.back, pos)
}

/// `C-d`: the shift, unless what was just typed is a `0`.
pub fn shift_left(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let after_zero = ctx.with_ext(|v: &mut Vim, _| {
        let last = v.inserts.get(&b).and_then(|s| s.edits.last());
        last.is_some_and(|e| *e == Edit::Text("0".into()))
    })?;
    let e = match after_zero {
        true => Edit::ZeroIndent,
        false => Edit::Shift { right: false },
    };
    insert_edit(ctx, a, e)
}

/// How many characters `text` takes after it in this state.
fn takes(ctx: &Ctx, b: BufferId, n: usize) -> usize {
    match ctx.state_of(b) {
        State::Replace => n,
        _ => 0,
    }
}

/// `C-r` and `C-a`: a register's text, whole lines or not, with no count.
pub fn paste(ctx: &mut Ctx, a: &Args, name: char) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let text = registers::read(ctx, Some(name))?.text;
    let take = takes(ctx, b, text.chars().count());
    let once = Args {
        count: None,
        ..a.clone()
    };
    insert_edit(ctx, &once, Edit::Literal(text, take))
}

/// `C-r x`. A key with no character names no register there is.
pub fn paste_register(ctx: &mut Ctx, a: &Args) -> Result {
    match a.char() {
        Some(c) => paste(ctx, a, c),
        None => Err(Error::Msg("Register is empty".into())),
    }
}

/// `C-@`: only if there was something to paste does it stop.
pub fn paste_and_stop(ctx: &mut Ctx, a: &Args) -> Result {
    paste(ctx, a, '.')?;
    let esc = Args {
        keys: vec![KeyEvent::press(Key::Esc)],
        ..a.clone()
    };
    end_session(ctx, &esc)
}

/// `C-y` and `C-e`. Pressed again, a count carries on. Typed text in replace state goes over
/// what's there, a character at a time. A session's count or `.` copies one again, afresh.
pub fn copy(ctx: &mut Ctx, a: &Args, above: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let n = ctx.with_ext(|v: &mut Vim, _| {
        let again = v
            .last_copy
            .as_ref()
            .filter(|l| l.above == above && l.at + 1 == v.commands && a.count.is_none());
        let n = again.map_or(a.count.unwrap_or(1), |l| l.n);
        v.last_copy = Some(LastCopy {
            at: v.commands,
            above,
            n,
        });
        n
    })?;
    let t = ctx.text(b)?;
    let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
    let text = insert::copied(t, p, above, n);
    let once = Args {
        count: None,
        ..a.clone()
    };
    if ctx.state_of(b) == State::Replace {
        let mut r = Ok(());
        for (i, c) in text.chars().enumerate() {
            let one = Args {
                keys: if i == 0 { a.keys.clone() } else { vec![] },
                ..once.clone()
            };
            r = r.and(insert_edit(ctx, &one, Edit::Over(c.to_string())));
        }
        return r;
    }
    insert_edit(ctx, &once, Edit::Text(text))?;
    let mut s = session(ctx, b)?;
    if let Some(last) = s.edits.last_mut() {
        *last = Edit::Copy { above };
    }
    put_session(ctx, b, s)
}

/// The character a key stands for under `C-v`. A key with none, like an arrow, has none.
fn literal_code(ev: &KeyEvent) -> Option<u32> {
    let c = ev.chord();
    match (ev.printable(), c.key) {
        (Some(s), _) => s.chars().next().map(u32::from),
        (_, Key::Esc) => Some(0x1b),
        (_, Key::Enter) => Some(0x0d),
        (_, Key::Tab) => Some(0x09),
        (_, Key::Backspace) => Some(0x7f),
        (_, Key::Char(k)) if c.mods.ctrl && (k as u32) < 0x80 => Some(k as u32 & 0x1f),
        _ => None,
    }
}

/// `C-v` and `C-q`, as Emacs's `read-quoted-char`: octal digits build a code until another key
/// ends it, which then runs as itself, unless it's `<CR>` or `C-q`. Any other key is put in as
/// it is, as the character it stands for. Each key after the first comes back here by feeding
/// `C-v` ahead of it.
pub fn quoted(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let Some(ev) = a.keys.last().cloned() else {
        return Ok(());
    };
    let held = ctx.with_ext(|v: &mut Vim, _| v.quoted.take())?;
    let code = literal_code(&ev);
    let digit = ev
        .printable()
        .and_then(|s| s.chars().next())
        .and_then(|c| c.to_digit(8));
    if let Some(d) = digit {
        let (so_far, count) = held.unwrap_or((0, a.count));
        let next = so_far.saturating_mul(8).saturating_add(d);
        ctx.with_ext(|v: &mut Vim, _| v.quoted = Some((next, count)))?;
        let keys = match held {
            Some(_) => key_notation(std::slice::from_ref(&ev)),
            None => key_notation(&a.keys),
        };
        add_keys(ctx, b, &keys)?;
        return repeat::feed_more(ctx, "<C-v>");
    }
    let c = ev.chord();
    let ends_it = c.key == Key::Enter || (c.key == Key::Char('q') && c.mods.ctrl);
    let (text, count, keys, again) = match held {
        Some((code, count)) => (code, count, "<C-q>".to_owned(), !ends_it),
        None => (
            code.unwrap_or(0),
            a.count,
            key_notation(&a.keys),
            code.is_none(),
        ),
    };
    let text = char::from_u32(text).ok_or(Error::Msg("Invalid character".into()))?;
    let take = takes(ctx, b, 1);
    let once = Args {
        count,
        keys: vec![],
        ..a.clone()
    };
    insert_edit(ctx, &once, Edit::Literal(text.to_string(), take))?;
    add_keys(ctx, b, &keys)?;
    if again {
        return repeat::feed_more(ctx, &key_notation(std::slice::from_ref(&ev)));
    }
    Ok(())
}

/// These keys are part of what `.` repeats.
fn add_keys(ctx: &mut Ctx, b: BufferId, keys: &str) -> Result {
    ctx.with_ext(|v: &mut Vim, _| {
        if let Some(c) = v.inserts.get_mut(&b).and_then(|s| s.repeat.as_mut()) {
            c.keys.push_str(keys);
        }
    })?;
    Ok(())
}
