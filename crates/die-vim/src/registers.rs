//! Registers, as your Evil keeps them. Everything die-vim reads or writes goes through
//! this module.
//!
//! The clipboard works as Emacs's does. Every new kill goes to it, whether or not a register was
//! named. Reading the newest kill first takes in what another app copied since, as a new kill
//! that isn't written back. `+` is the clipboard itself, and reading it leaves the ring alone.
//! `*` is your Evil's primary selection, which only DIE reaches, so it's a register like any other.
//!
//! Evil's unnamed register and `1`-`9` are one thing, Emacs's kill ring. Every yank and
//! delete pushes onto it unless the register is `_`, whether or not a register was named.
//! So `"` holds the newest kill, `1` the same, `2` the one before, and so on (S5 row 8).
//! `0` holds the last yank that named no register, `-` the last delete without a newline
//! that named none, and `a`-`z` what was yanked into them. Naming `A`-`Z` appends to
//! `a`-`z`, and then `"` holds only the new text (S5 row 9).

use crate::repeat::keys_of;
use crate::text::{Chars, Class, class, same_word};
use die_core::{Ctx, Error, Key, RegKind, Register, Result, TextBuf, key_notation, parse_keys};

/// How many kills can be read back: `"1` to `"9`. Emacs keeps more, out of reach.
const RING: usize = 9;

/// A place in the kill ring. The newest is `"` and `1`, which always agree.
fn slot(i: usize) -> char {
    match i {
        0 => '"',
        _ => char::from_digit((i + 1) as u32, 10).expect("a slot under 9"),
    }
}

fn set_slot(ctx: &mut Ctx, i: usize, r: Register) {
    if i == 0 {
        ctx.set_register('1', r.clone());
    }
    ctx.set_register(slot(i), r);
}

/// A new kill, Emacs's `kill-new`: the ring moves down one, the oldest goes, and the clipboard
/// gets it.
fn push(ctx: &mut Ctx, r: Register) {
    ctx.write_clipboard(&r);
    ring_push(ctx, r);
}

fn ring_push(ctx: &mut Ctx, r: Register) {
    for i in (1..RING).rev() {
        if let Some(older) = ctx.register(slot(i - 1)).cloned() {
            set_slot(ctx, i, older);
        }
    }
    set_slot(ctx, 0, r);
}

/// Naming `1`-`9` overwrites that place in the ring, counting round if the ring is shorter,
/// as Emacs's `current-kill` does. With an empty ring it's a kill.
fn overwrite(ctx: &mut Ctx, digit: u32, r: Register) {
    let len = (0..RING)
        .take_while(|&i| ctx.register(slot(i)).is_some())
        .count();
    if len == 0 {
        push(ctx, r);
    } else {
        set_slot(ctx, (digit as usize - 1) % len, r);
    }
}

/// Append to a register. Text of two kinds, or linewise text, ends up as whole lines. A
/// recording appended to anything is keys, and anything else appended to one is text.
fn append(ctx: &mut Ctx, c: char, r: Register) {
    let old = ctx.register(c).cloned();
    let old = match (&r.kind, old) {
        (RegKind::Macro, Some(old)) => {
            let text = keys_of(&old) + &r.text;
            ctx.set_register(
                c,
                Register {
                    kind: RegKind::Macro,
                    text,
                },
            );
            return;
        }
        (_, old) => old.map(plain),
    };
    let joined = match &old {
        None => r,
        Some(old) if old.kind == RegKind::Char && r.kind == RegKind::Char => Register {
            kind: RegKind::Char,
            text: format!("{}{}", old.text, r.text),
        },
        Some(old) => {
            let line = |s: &str| match s.is_empty() || s.ends_with('\n') {
                true => s.to_owned(),
                false => format!("{s}\n"),
            };
            Register {
                kind: RegKind::Line,
                text: line(&old.text) + &line(&r.text),
            }
        }
    };
    ctx.set_register(c, joined);
}

/// Store into a named register, Evil's `evil-set-register`.
fn set(ctx: &mut Ctx, c: char, r: Register) -> Result {
    match c {
        ':' | '.' | '%' => return Err(Error::Msg("Can't modify read-only register".into())),
        '"' => push(ctx, r),
        '_' => {}
        '+' => ctx.write_clipboard(&r),
        'A'..='Z' => append(ctx, c.to_ascii_lowercase(), r),
        _ => match c.to_digit(10) {
            Some(d @ 1..=9) => overwrite(ctx, d, r),
            _ => ctx.set_register(c, r),
        },
    }
    Ok(())
}

/// Save what a yank or a delete took. A yank that named no register also goes to `0`.
/// It fails, before anything else happens, on a register that can't be written.
pub fn save(ctx: &mut Ctx, name: Option<char>, r: Register, yank: bool) -> Result {
    if let Some(c) = name {
        set(ctx, c, r.clone())?;
    }
    if yank && name.is_none() {
        ctx.set_register('0', r.clone());
    }
    if name != Some('_') {
        push(ctx, r);
    }
    Ok(())
}

/// Note a delete that named no register and has no newline in it.
pub fn small_delete(ctx: &mut Ctx, text: &str) {
    ctx.set_register(
        '-',
        Register {
            kind: RegKind::Char,
            text: text.to_owned(),
        },
    );
}

/// What `p` and `P` would paste from `name`, or from the newest kill with none. An empty
/// register is an error, and so are the ones die-vim has no text for.
pub fn read(ctx: &mut Ctx, name: Option<char>) -> Result<Register> {
    // Only the newest kill looks at the clipboard, as Emacs's `current-kill` with 0.
    if matches!(name, None | Some('"' | '1'))
        && let Some(r) = ctx.clipboard_news()
    {
        ring_push(ctx, r);
    }
    let Some(c) = name else {
        return ctx
            .register('"')
            .cloned()
            .ok_or_else(|| Error::Msg("Kill ring is empty".into()));
    };
    let from_clipboard;
    let held = match c {
        '_' => {
            return Ok(Register {
                kind: RegKind::Char,
                text: String::new(),
            });
        }
        // `1` is read from `"`, which is the ring's head even if only `"` was ever set.
        '1' => ctx.register('"'),
        ':' | '.' | '%' | '#' | '/' | '=' => None,
        '+' => {
            from_clipboard = ctx.clipboard();
            from_clipboard.as_ref()
        }
        _ => ctx.register(c.to_lowercase().next().unwrap_or(c)),
    };
    held.cloned()
        .map(plain)
        .ok_or_else(|| Error::Msg(format!("Register {c} is empty")))
}

/// What `<C-r>` pastes on a command line, as your Evil's `evil-get-register` reads it: a register,
/// or what's under the cursor for `<C-w>` (the word), `<C-a>` (the WORD), `<C-o>` (the symbol),
/// `<C-f>` (the file name) and `<C-l>` (the line). `:` and `/` are the last of each line, and `%`
/// and `#` this buffer's and the last buffer's file. `.` and `=` aren't built.
pub fn for_prompt(ctx: &mut Ctx, c: char) -> Result<String> {
    let empty = || Error::Msg(format!("Register {} is empty", shown(c)));
    let last = |ctx: &Ctx, history: &str, none: &str| match ctx.prompt_history(history).last() {
        Some(line) => Ok(line.clone()),
        None => Err(Error::Msg(none.into())),
    };
    let here = ctx.focused_buffer();
    let path = |ctx: &Ctx, b| Some(ctx.file(b)?.path.display().to_string());
    let no_file = || Error::Msg("No file name".into());
    match c {
        '\x17' | '\x01' | '\x0f' | '\x06' | '\x0c' => under_cursor(ctx, c).ok_or_else(empty),
        ':' => last(ctx, "ex", "No previous command line"),
        '/' => last(ctx, "search", "No previous regular expression"),
        '%' => here.and_then(|b| path(ctx, b)).ok_or_else(no_file),
        '#' => {
            let other = ctx.buffer_list().into_iter().find(|b| Some(*b) != here);
            other.and_then(|b| path(ctx, b)).ok_or_else(no_file)
        }
        _ if c.is_control() => Err(empty()),
        _ => read(ctx, Some(c)).map(|r| r.text),
    }
}

/// A register's name for a message: `^W` for a control character.
fn shown(c: char) -> String {
    match u8::try_from(c) {
        Ok(b) if b < 32 => format!("^{}", char::from(b + 64)),
        _ => c.to_string(),
    }
}

/// The thing `<C-r>` names under the focused window's cursor.
fn under_cursor(ctx: &Ctx, c: char) -> Option<String> {
    let w = ctx.focused()?;
    let t = ctx.text(ctx.window_buffer(w)?).ok()?;
    let at = t.cursor(w)?;
    let word = |big: bool| thing(t, at, |c| !blank(c), |a, b| same_word(a, b, big));
    match c {
        '\x17' => word(false),
        '\x01' => word(true),
        '\x0f' => thing(t, at, |c| !blank(c), |a, b| symbolic(a) == symbolic(b)),
        '\x06' => thing(t, at, file_char, |_, _| true),
        _ => {
            let line = t.rope().line(t.line_of(at)).to_string();
            Some(line.trim_end_matches('\n').to_owned())
        }
    }
}

fn blank(c: char) -> bool {
    matches!(class(c, false), Class::Blank | Class::Newline)
}

/// A word or symbol character in Emacs's standard syntax table, against other punctuation.
fn symbolic(c: char) -> bool {
    class(c, false) == Class::Word || "_-+*/&|<>=".contains(c)
}

/// Emacs's `thing-at-point-file-name-chars`.
fn file_char(c: char) -> bool {
    c.is_alphanumeric() || "-@~/_.${}#%,:".contains(c)
}

/// The run of `same` characters that `is` picks out at `at`, or the one that ends just before it,
/// as Emacs's `thing-at-point` takes it.
fn thing(
    t: &TextBuf,
    at: usize,
    is: impl Fn(char) -> bool,
    same: impl Fn(char, char) -> bool,
) -> Option<String> {
    let mut start = Chars::at(t, at);
    if !start.ch().is_some_and(&is) {
        start.prev_ch().filter(|c| is(*c))?;
        start.back();
    }
    let mut end = Chars::at(t, start.pos());
    let mut here = end.ch()?;
    while let Some(c) = end.fwd().then(|| end.ch()).flatten() {
        if !is(c) || !same(here, c) {
            break;
        }
        here = c;
    }
    let mut here = start.ch()?;
    while let Some(c) = start.prev_ch().filter(|c| is(*c) && same(*c, here)) {
        here = c;
        start.back();
    }
    Some(t.rope().byte_slice(start.pos()..end.pos()).to_string())
}

/// A recording as text: what its keys would type, as your Evil holds it. Keys with no
/// character stay in notation.
fn plain(r: Register) -> Register {
    if r.kind != RegKind::Macro {
        return r;
    }
    let mut text = String::new();
    for ev in parse_keys(&r.text).unwrap_or_default() {
        let c = ev.chord();
        match (ev.printable(), c.key) {
            (Some(s), _) => text.push_str(&s),
            (_, Key::Esc) => text.push('\x1b'),
            (_, Key::Enter) => text.push('\r'),
            (_, Key::Tab) => text.push('\t'),
            (_, Key::Backspace) => text.push('\x7f'),
            (_, Key::Char(k)) if c.mods.ctrl && k.is_ascii_alphabetic() => {
                text.push(char::from(k.to_ascii_lowercase() as u8 - b'`'));
            }
            _ => text.push_str(&key_notation(std::slice::from_ref(&ev))),
        }
    }
    Register {
        kind: RegKind::Char,
        text,
    }
}

/// `q` stopped: keep the keys in this register. Recording nothing into a register that's
/// empty leaves it empty.
pub fn record(ctx: &mut Ctx, name: char, keys: String) -> Result {
    if keys.is_empty() {
        return Ok(());
    }
    let r = Register {
        kind: RegKind::Macro,
        text: keys,
    };
    set(ctx, name, r)
}

/// The keys `@` would type from this register, or none if it's empty.
pub fn macro_keys(ctx: &Ctx, name: char) -> String {
    let c = name.to_lowercase().next().unwrap_or(name);
    let held = if c == '1' { '"' } else { c };
    ctx.register(held).map(keys_of).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vim;
    use die_core::FakeClipboard;

    fn ctx() -> Ctx {
        Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
    }

    fn reg(kind: RegKind, text: &str) -> Register {
        Register {
            kind,
            text: text.into(),
        }
    }

    fn chars(text: &str) -> Register {
        reg(RegKind::Char, text)
    }

    fn text(ctx: &Ctx, c: char) -> Option<&str> {
        ctx.register(c).map(|r| r.text.as_str())
    }

    #[test]
    fn every_kill_reaches_the_clipboard_but_not_what_only_names_a_register() {
        let mut ctx = ctx();
        let board = FakeClipboard::default();
        ctx.set_clipboard(board.clone());
        save(&mut ctx, None, chars("one"), false).unwrap();
        save(&mut ctx, Some('a'), chars("two"), true).unwrap();
        save(&mut ctx, Some('_'), chars("gone"), true).unwrap();
        small_delete(&mut ctx, "small");
        let written: Vec<String> = board.take_writes().into_iter().map(|r| r.text).collect();
        assert_eq!(written, ["one", "two"]);
    }

    #[test]
    fn only_the_newest_kill_takes_in_another_apps_copy() {
        let mut ctx = ctx();
        let board = FakeClipboard::default();
        ctx.set_clipboard(board.clone());
        save(&mut ctx, None, chars("kill"), false).unwrap();
        board.take_writes();

        board.copy_from_outside("copied");
        ctx.set_register('a', chars("named"));
        assert_eq!(
            read(&mut ctx, Some('a')).map(|r| r.text),
            Ok("named".into())
        );
        assert_eq!(text(&ctx, '"'), Some("kill"), "not for another register");
        assert_eq!(
            read(&mut ctx, Some('+')).map(|r| r.text),
            Ok("copied".into())
        );
        assert_eq!(text(&ctx, '"'), Some("kill"), "nor for reading it as `+`");

        assert_eq!(read(&mut ctx, None).map(|r| r.text), Ok("copied".into()));
        assert_eq!(text(&ctx, '1'), Some("copied"));
        assert_eq!(text(&ctx, '2'), Some("kill"));
        assert_eq!(board.take_writes(), [], "and it isn't written back");

        board.copy_from_outside("again");
        assert_eq!(
            read(&mut ctx, Some('1')).map(|r| r.text),
            Ok("again".into())
        );
    }

    #[test]
    fn plus_writes_the_clipboard_and_star_is_a_register() {
        let mut ctx = ctx();
        let board = FakeClipboard::default();
        ctx.set_clipboard(board.clone());
        ctx.set_register('*', chars("primary"));
        assert_eq!(
            read(&mut ctx, Some('*')).map(|r| r.text),
            Ok("primary".into())
        );
        assert_eq!(board.held(), None);
        set(&mut ctx, '+', chars("board")).unwrap();
        assert_eq!(board.held(), Some(chars("board")));
        assert_eq!(text(&ctx, '+'), None, "it holds no copy");
    }

    #[test]
    fn the_ring_keeps_nine_kills_newest_first() {
        let mut ctx = ctx();
        for n in 0..11 {
            save(&mut ctx, None, chars(&n.to_string()), false).unwrap();
        }
        assert_eq!(text(&ctx, '"'), Some("10"));
        assert_eq!(text(&ctx, '1'), Some("10"));
        assert_eq!(text(&ctx, '2'), Some("9"));
        assert_eq!(text(&ctx, '9'), Some("2"));
    }

    #[test]
    fn a_preset_unnamed_register_is_the_rings_head() {
        let mut ctx = ctx();
        ctx.set_register('"', chars("mine"));
        assert_eq!(read(&mut ctx, Some('1')).map(|r| r.text), Ok("mine".into()));
        save(&mut ctx, None, chars("new"), false).unwrap();
        assert_eq!(text(&ctx, '2'), Some("mine"));
    }

    #[test]
    fn only_a_yank_that_names_no_register_fills_zero() {
        let mut ctx = ctx();
        save(&mut ctx, None, chars("deleted"), false).unwrap();
        assert_eq!(text(&ctx, '0'), None);
        save(&mut ctx, Some('a'), chars("named"), true).unwrap();
        assert_eq!(text(&ctx, '0'), None);
        save(&mut ctx, None, chars("yanked"), true).unwrap();
        assert_eq!(text(&ctx, '0'), Some("yanked"));
    }

    #[test]
    fn the_black_hole_keeps_nothing() {
        let mut ctx = ctx();
        save(&mut ctx, Some('_'), chars("gone"), true).unwrap();
        assert_eq!(text(&ctx, '"'), None);
        assert_eq!(text(&ctx, '0'), None);
        assert_eq!(read(&mut ctx, Some('_')).map(|r| r.text), Ok(String::new()));
    }

    #[test]
    fn appending_mixed_kinds_makes_whole_lines() {
        for (old, new, want, kind) in [
            (chars("a"), chars("b"), "ab", RegKind::Char),
            (
                chars("a"),
                reg(RegKind::Line, "b\n"),
                "a\nb\n",
                RegKind::Line,
            ),
            (
                reg(RegKind::Line, "a\n"),
                chars("b"),
                "a\nb\n",
                RegKind::Line,
            ),
            (
                reg(RegKind::Line, "a\n"),
                reg(RegKind::Line, "b\n"),
                "a\nb\n",
                RegKind::Line,
            ),
        ] {
            let mut ctx = ctx();
            ctx.set_register('a', old);
            save(&mut ctx, Some('A'), new, true).unwrap();
            assert_eq!(ctx.register('a'), Some(&reg(kind, want)));
        }
    }

    #[test]
    fn read_only_registers_refuse_and_nothing_is_kept() {
        let mut ctx = ctx();
        assert!(save(&mut ctx, Some(':'), chars("x"), true).is_err());
        assert_eq!(text(&ctx, '"'), None);
    }

    #[test]
    fn an_empty_register_is_an_error() {
        let mut ctx = ctx();
        assert!(read(&mut ctx, None).is_err());
        assert!(read(&mut ctx, Some('q')).is_err());
        assert!(read(&mut ctx, Some('Q')).is_err());
        assert!(read(&mut ctx, Some('+')).is_err());
    }

    #[test]
    fn naming_a_digit_overwrites_that_place_counting_round() {
        // Your Evil: yw, w, yw, "3yw leaves b, b, a in 1, 2, 3 (oracle: registers).
        let mut ctx = ctx();
        save(&mut ctx, None, chars("a"), true).unwrap();
        save(&mut ctx, None, chars("b"), true).unwrap();
        save(&mut ctx, Some('3'), chars("b"), true).unwrap();
        assert_eq!(text(&ctx, '1'), Some("b"));
        assert_eq!(text(&ctx, '2'), Some("b"));
        assert_eq!(text(&ctx, '3'), Some("a"));
    }
}
