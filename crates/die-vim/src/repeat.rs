//! `.` and macros, as your Evil has them, except where deviations.json says otherwise.
//!
//! **`.`** replays keys. Every command that changes text says what its keys were
//! ([`commit`]), with its count and register kept apart so a count on `.` can replace them.
//! An insert session's keys end with its Escape. A visual operator also keeps how much it
//! acted on ([`Extent`]), and `.` selects that much from the cursor before it replays.
//!
//! **`q`** records the typed keys, not the fed ones, so `qb@aq` holds `@a`. A recording
//! is notation, in a register marked [`RegKind::Macro`]. **`@`** feeds a register's keys, as
//! `Ctx::feed_keys` says: they run ahead of what's typed, and a failure drops them.
//!
//! Inside a macro, a change becomes what `.` repeats, and a `.` before any change repeats
//! the one from before the macro (deviations.json). After it, `.` runs the whole macro again,
//! and `u` undoes all of it. Fed keys run ahead of typed ones, so a macro has ended when the
//! next typed key arrives. An observer that has no `Ctx` notes that key, and [`before`], at
//! the start of every command, does what's due.
//!
//! **`:norm`** feeds each line's keys the same way ([`feed_line`]), and `each.rs` waits for
//! them. Your Evil runs them with its command hooks, so after it `.` is the last change they
//! made, a macro they ran counting as `@x`, as if typed.

use crate::Vim;
use crate::grammar::buffer_window;
use crate::insert;
use crate::motions::Find;
use crate::registers;
use crate::visual;
use die_core::{Args, BufferId, Ctx, Error, Key, KeyEvent, KeySource, RegKind, Register, Result};
use die_core::{State, key_notation, parse_keys};
use std::cell::RefCell;
use std::rc::Rc;

/// How much of the text a macro may feed at once.
const MAX_FEED: usize = 1 << 20;
/// How many macros may start between two typed keys. Emacs ends a macro that calls itself
/// forever when its Lisp nesting runs out, and nothing else would end one here.
const MAX_RUNS: usize = 100_000;

/// What a visual selection covered, to select as much again from the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extent {
    /// Characters: on one line this many past the first, or else this many lines down and
    /// this many past that line's start.
    Char { lines: usize, chars: usize },
    /// Whole lines, this many below the first.
    Line { lines: usize },
    /// A rectangle, this many lines down and columns across.
    Block { lines: usize, cols: usize },
}

/// A change `.` can replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub count: Option<usize>,
    pub register: Option<char>,
    /// In notation. An operator's keys come with its motion's.
    pub keys: String,
    pub visual: Option<Extent>,
}

impl Change {
    /// The command that's running, on its own.
    pub fn of(a: &Args) -> Change {
        Change {
            count: a.count,
            register: a.register,
            keys: key_notation(&a.keys),
            visual: None,
        }
    }

    /// This change, if keys typed it. One run from `:` has none, and `.` can't type it again.
    pub fn typed(self) -> Option<Change> {
        (!self.keys.is_empty()).then_some(self)
    }

    fn notation(&self) -> String {
        let mut s = String::new();
        if let Some(r) = self.register {
            s.push('"');
            s.push_str(&key_notation(&[KeyEvent::press(Key::Char(r))]));
        }
        if let Some(n) = self.count {
            s.push_str(&n.to_string());
        }
        s + &self.keys
    }
}

/// What `.` and macros keep. Shared with the key observer, which has no `Ctx`.
#[derive(Default)]
pub struct Recorder {
    /// The register `q` is recording into, and the typed keys so far.
    recording: Option<(char, Vec<KeyEvent>)>,
    /// A key was typed since [`before`] last ran.
    typed: bool,
    /// What `.` repeats.
    last: Option<Change>,
    /// What `.` repeats once keys are typed again.
    then_last: Option<Change>,
    /// The buffer whose undo group a macro has open.
    undo_group: Option<BufferId>,
    /// The `f` that `;` repeats once keys are typed again: `.` doesn't change it.
    then_find: Option<Option<Find>>,
    /// The last `@x`, which `@@` repeats.
    last_macro: Option<char>,
    /// Macros started since a key was typed.
    runs: usize,
    /// Keys fed and not yet run. When it runs out, a macro has reached its end.
    fed: usize,
    /// A macro or `.` is running: no key has been typed since it fed its keys.
    replaying: bool,
    /// The state `q` was pressed in, while it waits for a register's name.
    resume: Option<State>,
    /// While `:norm` runs a line's keys, and a change is what `.` repeats as when typed: where
    /// the last macro or `.` those keys ran ends, as a count of keys still to run.
    norm: Option<Option<usize>>,
}

impl Recorder {
    /// Whether a key that ran now came from a macro or a `.` that `:norm`'s keys ran, and not
    /// from those keys themselves.
    fn in_macro(&self) -> bool {
        matches!(self.norm, Some(Some(end)) if self.fed >= end)
    }

    /// What `.` repeats, what `@@` does and the `;` `.` left alone, as they'll be once keys are
    /// typed again, for the session.
    pub fn kept(&self) -> (Option<&Change>, Option<char>, Option<Option<Find>>) {
        let last = self.then_last.as_ref().or(self.last.as_ref());
        (last, self.last_macro, self.then_find)
    }

    /// From the session.
    pub fn restore(&mut self, last: Option<Change>, last_macro: Option<char>) {
        self.last = last;
        self.last_macro = last_macro;
    }
}

fn recorder(ctx: &mut Ctx) -> Result<Rc<RefCell<Recorder>>> {
    Ok(ctx.with_ext(|v: &mut Vim, _| v.rec.clone())?)
}

/// Note typed keys, and record them if `q` is on.
pub fn observe(ctx: &mut Ctx, rec: Rc<RefCell<Recorder>>) {
    ctx.observe_keys(move |ev, src| {
        let mut r = rec.borrow_mut();
        if src == KeySource::Fed {
            r.fed = r.fed.saturating_sub(1);
            return;
        }
        r.typed = true;
        r.replaying = false;
        if let Some((_, keys)) = &mut r.recording {
            keys.push(ev.clone());
        }
    })
    .detach();
    ctx.on_fed_dropped(end_failed_replay).detach();
}

/// What was fed has gone: a macro or `.` failed, or a typed `C-g` stopped it. As your Evil's
/// `evil-execute-macro` does, the buffer goes back to normal state, before the next key is
/// looked up. Then a `:norm` goes on to its next line, unless it was a quit.
fn end_failed_replay(ctx: &mut Ctx) -> Result {
    let quit = ctx.take_quit();
    let rec = recorder(ctx)?;
    let replaying = {
        let mut r = rec.borrow_mut();
        r.fed = 0;
        std::mem::take(&mut r.replaying)
    };
    let left = if replaying { to_normal(ctx) } else { Ok(()) };
    crate::each::dropped(ctx, quit).and(left)
}

/// Leave a session or a selection a replay left behind. The session's keys aren't a change `.`
/// can repeat, since they never reached their Escape.
pub fn to_normal(ctx: &mut Ctx) -> Result {
    let Some(b) = ctx.focused_buffer() else {
        return Ok(());
    };
    let a = Args {
        buffer: Some(b),
        window: ctx.focused(),
        ..Default::default()
    };
    match ctx.state_of(b) {
        State::Visual => visual::escape(ctx, &a),
        State::Insert | State::Replace => {
            let s = ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
            insert::end(ctx, &a, s)
        }
        _ => Ok(()),
    }
}

/// At the start of every command: if keys are being typed again, a macro has ended.
pub fn before(ctx: &mut Ctx) -> Result {
    let rec = recorder(ctx)?;
    let mut r = rec.borrow_mut();
    if !r.typed {
        return Ok(());
    }
    r.typed = false;
    r.runs = 0;
    r.fed = 0;
    // A `:norm` waiting on a prompt doesn't hush what's typed.
    ctx.hush_key_errors(false);
    if let Some(c) = r.then_last.take() {
        r.last = Some(c);
    }
    if let Some(b) = r.undo_group.take()
        && let Ok(t) = ctx.text_mut(b)
    {
        // What's typed next is a step of its own, even in an insert session the macro began.
        t.end_undo_group();
        t.undo_boundary();
    }
    if let Some(f) = r.then_find.take() {
        ctx.with_ext(|v: &mut Vim, _| v.find = f)?;
    }
    Ok(())
}

/// Whether this key is a macro's last. An operator can't wait here: your Evil's fails.
pub fn macro_ending(ctx: &mut Ctx) -> Result<bool> {
    let rec = recorder(ctx)?;
    let r = rec.borrow();
    Ok(r.runs > 0 && r.fed == 0)
}

fn feed(ctx: &mut Ctx, rec: &RefCell<Recorder>, keys: &str) -> Result {
    let n = parse_keys(keys).map_or(0, |k| k.len());
    ctx.feed_keys(keys)?;
    let mut r = rec.borrow_mut();
    if r.norm.is_some() && !r.in_macro() {
        r.norm = Some(Some(r.fed));
    }
    r.fed += n;
    r.replaying = true;
    Ok(())
}

/// `:norm` and `:g` start: what typed keys ended is over, and one `u` undoes all that follows,
/// as a macro's does. Whether a change their keys make is what `.` repeats, as when typed: not
/// inside a macro, whose own `@x` is that. And how many macros had run, for each line to start
/// from, so a line's macros can call themselves as long as a typed one can.
pub fn begin_lines(ctx: &mut Ctx, b: BufferId) -> Result<(bool, usize)> {
    before(ctx)?;
    let rec = recorder(ctx)?;
    let mut r = rec.borrow_mut();
    open_group(ctx, &mut r, b)?;
    Ok((r.then_last.is_none(), r.runs))
}

/// One `u` undoes what follows, until keys are typed again.
fn open_group(ctx: &mut Ctx, r: &mut Recorder, b: BufferId) -> Result {
    if r.undo_group.is_none() {
        r.undo_group = Some(b);
        ctx.text_mut(b)?.begin_undo_group();
    }
    Ok(())
}

/// Feed a line's keys for `:norm`, and return how many keys are left to run once they have. If
/// keys were typed while a line waited, as for a prompt it left open, they've ended the undo
/// group, and the rest starts another.
pub fn feed_line(ctx: &mut Ctx, b: BufferId, keys: &str, top: bool, runs: usize) -> Result<usize> {
    before(ctx)?;
    let rec = recorder(ctx)?;
    let n = parse_keys(keys).map_or(0, |k| k.len());
    ctx.feed_keys(keys)?;
    let mut r = rec.borrow_mut();
    open_group(ctx, &mut r, b)?;
    let left = r.fed;
    r.fed += n;
    r.replaying = true;
    r.runs = runs;
    if top {
        r.norm = Some(None);
    }
    // Your Evil's `ignore-errors`: a line that fails says nothing.
    ctx.hush_key_errors(true);
    Ok(left)
}

/// A `q` still waiting for a register's name when `:norm`'s keys ran out reads none.
pub fn unwait(ctx: &mut Ctx, b: BufferId) -> Result {
    let back = recorder(ctx)?.borrow_mut().resume.take();
    ctx.set_state_of(b, back.unwrap_or(State::Normal));
    Ok(())
}

/// `:norm` and `:g` are over.
pub fn end_lines(ctx: &mut Ctx) -> Result {
    ctx.hush_key_errors(false);
    recorder(ctx)?.borrow_mut().norm = None;
    Ok(())
}

/// Feed keys a command needs next, without the macro they run in, if any, ending early.
pub fn feed_more(ctx: &mut Ctx, keys: &str) -> Result {
    let rec = recorder(ctx)?;
    let n = parse_keys(keys).map_or(0, |k| k.len());
    ctx.feed_keys(keys)?;
    rec.borrow_mut().fed += n;
    Ok(())
}

/// How many keys that were fed are still to run.
pub fn fed_left(ctx: &mut Ctx) -> Result<usize> {
    Ok(recorder(ctx)?.borrow().fed)
}

/// Whether this key was the last that a macro or `.` fed.
pub fn replay_ending(ctx: &mut Ctx) -> Result<bool> {
    let rec = recorder(ctx)?;
    let r = rec.borrow();
    Ok(r.replaying && r.fed == 0)
}

/// This change is the one `.` repeats now, unless it came from the command line: then the last
/// typed one stays.
pub fn commit(ctx: &mut Ctx, c: Change) -> Result {
    if let Some(c) = c.typed() {
        let rec = recorder(ctx)?;
        let mut r = rec.borrow_mut();
        // From `:norm`'s own keys, it comes after any macro they ran.
        if r.norm.is_some() && !r.in_macro() {
            r.then_last = None;
        }
        r.last = Some(c);
    }
    Ok(())
}

/// `.`: the last change, with this count instead of its own. Your Evil keeps the change as it
/// was, so the next `.` goes back to the old count, and the `f` that `;` repeats stays.
pub fn dot(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let find = ctx.with_ext(|v: &mut Vim, _| v.find)?;
    let rec = recorder(ctx)?;
    let mut c = {
        let mut r = rec.borrow_mut();
        let Some(c) = r.last.clone() else {
            return Err(Error::Msg("No previous change".into()));
        };
        r.then_find.get_or_insert(find);
        if a.count.is_some() && r.then_last.is_none() {
            r.then_last = Some(c.clone());
        }
        c
    };
    // A count says how much of a selection's operator to do when it's made, and `.` has its size
    // from the selection already, as your Evil's has.
    if a.count.is_some() && c.visual.is_none() {
        c.count = a.count;
    }
    if let Some(e) = c.visual {
        visual::select(ctx, b, w, e)?;
    }
    feed(ctx, &rec, &c.notation())
}

/// `q`: stop recording, or wait for the name of a register to record into. Your Evil reads
/// one key here, and a prompt's state is for that.
pub fn q(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let rec = recorder(ctx)?;
    let stopped = rec.borrow_mut().recording.take();
    match stopped {
        Some((name, mut keys)) => {
            keys.pop();
            ctx.set_note(None);
            registers::record(ctx, name, key_notation(&keys))
        }
        None => {
            rec.borrow_mut().resume = Some(ctx.state_of(b));
            ctx.set_state_of(b, State::CommandLine);
            Ok(())
        }
    }
}

/// The key after `q`: start recording into that register.
pub fn start_recording(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let rec = recorder(ctx)?;
    let mut r = rec.borrow_mut();
    ctx.set_state_of(b, r.resume.take().unwrap_or(State::Normal));
    match a.char().filter(char::is_ascii_alphanumeric) {
        Some(c) => {
            // As `evil-record-macro` does, so `@a` in `qa...@aq` finds nothing, until `q` stops.
            // `qA` adds to what's there. A digit's register is the kill ring's, and stays.
            if c.is_ascii_lowercase() {
                ctx.clear_register(c);
            }
            // As Vim shows it, the whole time, with the name as typed.
            ctx.set_note(Some(format!("recording @{c}")));
            r.recording = Some((c, vec![]));
        }
        None => return Err(Error::Msg("Invalid register".into())),
    }
    Ok(())
}

/// `@:`: the last ex line, count times. As in your Evil, `.` stays as it was.
fn last_ex_line(ctx: &mut Ctx, a: &Args) -> Result {
    let Some(line) = ctx.prompt_history("ex").last().cloned() else {
        return Err(Error::Msg("No previous command line".into()));
    };
    let n = a.count.unwrap_or(1).max(1);
    if n > MAX_FEED {
        return Err(Error::Msg("Count too large".into()));
    }
    (0..n).try_for_each(|_| crate::ex::run(ctx, &line))
}

/// `@x` and `@@`: type a register count times. An empty register is an error, except while
/// recording, so `qa...@aq` can make a macro that calls itself. `@:` is the last ex line.
pub fn at(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let rec = recorder(ctx)?;
    let name = match a.char() {
        Some('@') => rec.borrow().last_macro,
        c => c,
    };
    let Some(name) = name else {
        return Err(Error::Msg("No previously executed keyboard macro".into()));
    };
    rec.borrow_mut().last_macro = Some(name);
    if name == ':' {
        return last_ex_line(ctx, a);
    }
    let keys = registers::macro_keys(ctx, name);
    if keys.is_empty() {
        if rec.borrow().recording.is_some() {
            return Ok(());
        }
        return Err(Error::Msg("No previous macro".into()));
    }
    let n = a.count.unwrap_or(1).max(1);
    if keys.len().saturating_mul(n) > MAX_FEED {
        return Err(Error::Msg("Count too large".into()));
    }
    {
        let mut r = rec.borrow_mut();
        if r.runs >= MAX_RUNS {
            return Err(Error::Msg("Too many macros without a typed key".into()));
        }
        r.runs += 1;
        // A macro inside a macro belongs to the outer one, and one of `:norm`'s keys is a change
        // as if typed. `.` doesn't keep the count.
        let c = Change {
            count: None,
            ..Change::of(a)
        };
        if r.norm.is_some() && !r.in_macro() {
            r.then_last = Some(c);
        } else {
            r.then_last.get_or_insert(c);
        }
        open_group(ctx, &mut r, b)?;
    }
    feed(ctx, &rec, &keys.repeat(n))
}

/// What a register holds, as the keys `@` should type: a recording as it is, and anything
/// else one key per character, as Vim and Evil do.
pub fn keys_of(r: &Register) -> String {
    if r.kind == RegKind::Macro {
        return r.text.clone();
    }
    text_keys(&r.text)
}

/// Text as the keys that type it, one per character.
pub fn text_keys(text: &str) -> String {
    let mut s = String::new();
    for c in text.chars() {
        match c {
            '<' => s.push_str("<lt>"),
            '\n' => s.push_str("<NL>"),
            '\r' => s.push_str("<CR>"),
            '\t' => s.push_str("<Tab>"),
            '\x1b' => s.push_str("<Esc>"),
            c if u32::from(c) < 0x20 => {
                let letter = char::from(c as u8 + b'`');
                s.push_str(&format!("<C-{letter}>"));
            }
            c => s.push(c),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Register {
        Register {
            kind: RegKind::Char,
            text: text.into(),
        }
    }

    #[test]
    fn plain_text_is_typed_as_it_is() {
        for (text, keys) in [
            ("Vec<String>", "Vec<lt>String>"),
            ("x\ny", "x<NL>y"),
            ("<CR>", "<lt>CR>"),
            ("\t\r\x1b\x01", "<Tab><CR><Esc><C-a>"),
        ] {
            assert_eq!(keys_of(&plain(text)), keys);
            assert!(die_core::parse_keys(keys).is_ok(), "{keys}");
        }
    }

    #[test]
    fn a_recording_is_already_keys() {
        let r = Register {
            kind: RegKind::Macro,
            text: "A!<Esc>".into(),
        };
        assert_eq!(keys_of(&r), "A!<Esc>");
    }

    #[test]
    fn a_change_is_written_register_then_count_then_keys() {
        let c = Change {
            count: Some(3),
            register: Some('a'),
            keys: "dw".into(),
            visual: None,
        };
        assert_eq!(c.notation(), "\"a3dw");
    }
}
