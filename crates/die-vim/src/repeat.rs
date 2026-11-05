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

/// A macro or `.` failed. As your Evil's `evil-execute-macro` does, the buffer goes back to
/// normal state, before the next key is looked up. The session's keys aren't a change `.` can
/// repeat, since they never reached their Escape.
fn end_failed_replay(ctx: &mut Ctx) -> Result {
    let rec = recorder(ctx)?;
    if !std::mem::take(&mut rec.borrow_mut().replaying) {
        return Ok(());
    }
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
    r.fed += n;
    r.replaying = true;
    Ok(())
}

/// This change is the one `.` repeats now.
pub fn commit(ctx: &mut Ctx, c: Change) -> Result {
    recorder(ctx)?.borrow_mut().last = Some(c);
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
            r.recording = Some((c, vec![]));
        }
        None => return Err(Error::Msg("Invalid register".into())),
    }
    Ok(())
}

/// `@x` and `@@`: type a register count times. An empty register is an error, except while
/// recording, so `qa...@aq` can make a macro that calls itself.
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
        // A macro inside a macro belongs to the outer one. `.` doesn't keep the count.
        r.then_last.get_or_insert_with(|| Change {
            count: None,
            ..Change::of(a)
        });
        if r.undo_group.is_none() {
            r.undo_group = Some(b);
            ctx.text_mut(b)?.begin_undo_group();
        }
    }
    feed(ctx, &rec, &keys.repeat(n))
}

/// What a register holds, as the keys `@` should type: a recording as it is, and anything
/// else one key per character, as Vim and Evil do.
pub fn keys_of(r: &Register) -> String {
    if r.kind == RegKind::Macro {
        return r.text.clone();
    }
    let mut s = String::new();
    for c in r.text.chars() {
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
