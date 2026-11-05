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

use die_core::{Ctx, Error, RegKind, Register, Result};

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

/// Append to a register. Text of two kinds, or linewise text, ends up as whole lines.
fn append(ctx: &mut Ctx, c: char, r: Register) {
    let joined = match ctx.register(c) {
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
        .ok_or_else(|| Error::Msg(format!("Register {c} is empty")))
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
