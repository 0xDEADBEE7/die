//! die-vim: DIE's Vim grammar (scope §10). It depends on die-core only (D15), and
//! its behaviour is your Evil's, checked case by case by the Evil oracle
//! (`tests/oracle.rs`), except the deviations named in `tests/oracle/deviations.json`.
//!
//! `grammar.rs` says how counts, motions, operators and insert sessions fit
//! together. So far (V1, V3): motions, counts, insert sessions with their undo,
//! operators, registers and text objects. `registers.rs` is the one place that touches
//! registers, for K6.

mod grammar;
mod insert;
mod motions;
mod objects;
mod operators;
mod registers;
mod scripts;
mod text;

use die_core::{Args, BufferId, Ctx, Dir, Error, Extension, Registry, Result, State};
use grammar::{Kind, Operator, buffer_window, char_key, count_digit, forget_goal, run_motion};
use insert::{Edit, Entry, Session};
use motions::{Find, MOTIONS};
use objects::OBJECTS;
use operators::Pending;
use std::collections::HashMap;

/// The die-vim extension.
#[derive(Default)]
pub struct Vim {
    /// Each buffer's insert session. Core keeps insert state per buffer, and
    /// Evil keeps a session's count and keys per buffer too.
    inserts: HashMap<BufferId, Session>,
    /// Each buffer's operator waiting for its motion. Core keeps that state per buffer too.
    pending: HashMap<BufferId, Pending>,
    /// The last `f`, `F`, `t` or `T`, which `;` and `,` repeat.
    find: Option<Find>,
}

#[rustfmt::skip]
const INSERTS: &[(&str, &str, Entry, &str)] = &[
    ("i", "vim-insert", Entry::Insert, "Insert before the cursor."),
    ("a", "vim-append", Entry::Append, "Insert after the cursor."),
    ("I", "vim-insert-line", Entry::InsertLine, "Insert at the line's first non-blank."),
    ("A", "vim-append-line", Entry::AppendLine, "Insert at the end of the line."),
    ("o", "vim-open-below", Entry::OpenBelow, "Open a line below, indented like this one."),
    ("O", "vim-open-above", Entry::OpenAbove, "Open a line above, indented like the one above it."),
];

/// The operators: their keys in normal state, the last key again in operator-pending
/// state meaning whole lines (`dd`, `g~~`, and `g~g~` too), and their commands.
#[rustfmt::skip]
const OPERATORS: &[(&str, &str, &str, Operator, &str)] = &[
    ("d", "d", "vim-delete", Operator::Delete, "Delete what a motion covers."),
    ("c", "c", "vim-change", Operator::Change, "Delete what a motion covers, and insert."),
    ("y", "y", "vim-yank", Operator::Yank, "Yank what a motion covers."),
    ("<lt>", "<lt>", "vim-shift-left", Operator::ShiftLeft, "Shift lines left."),
    (">", ">", "vim-shift-right", Operator::ShiftRight, "Shift lines right."),
    ("g~", "~", "vim-invert-case", Operator::InvertCase, "Invert the case of what a motion covers."),
    ("gU", "U", "vim-upcase", Operator::Upcase, "Upper-case what a motion covers."),
    ("gu", "u", "vim-downcase", Operator::Downcase, "Lower-case what a motion covers."),
];

/// Commands that are an operator over a motion of their own.
#[rustfmt::skip]
const OWN_MOTION: &[(&str, &str, Operator, Kind, operators::Own, &str)] = &[
    ("x", "vim-delete-char", Operator::Delete, Kind::Exclusive, motions::forward_char, "Delete count characters."),
    ("X", "vim-delete-char-before", Operator::Delete, Kind::Exclusive, motions::backward_char, "Delete count characters before the cursor."),
    ("s", "vim-substitute", Operator::Change, Kind::Exclusive, motions::forward_char, "Change count characters."),
    ("D", "vim-delete-line", Operator::Delete, Kind::Inclusive, motions::end_of_line, "Delete to the end of the line."),
    ("C", "vim-change-line", Operator::Change, Kind::Inclusive, motions::end_of_line, "Change to the end of the line."),
    ("~", "vim-invert-char", Operator::InvertCase, Kind::Exclusive, motions::forward_char, "Invert the case of count characters."),
];

/// Commands that are an operator over count whole lines.
#[rustfmt::skip]
const OWN_LINES: &[(&str, &str, Operator, &str)] = &[
    ("S", "vim-change-whole-line", Operator::Change, "Change count lines."),
    ("Y", "vim-yank-line", Operator::Yank, "Yank count lines."),
];

#[rustfmt::skip]
const EDITS: &[(&str, &str, Edit, &str)] = &[
    ("<CR>", "vim-insert-newline", Edit::Newline, "A new line, indented like the line above."),
    ("<BS>", "vim-delete-backward-char", Edit::Backspace, "Delete the character before the cursor."),
    ("<C-w>", "vim-delete-backward-word", Edit::DeleteWord, "Delete back to the start of a word."),
    ("<Tab>", "vim-indent-relative", Edit::Tab, "Indent to the next indent point of the line above."),
    ("<Left>", "vim-insert-left", Edit::Left, "Left a character."),
    ("<Right>", "vim-insert-right", Edit::Right, "Right a character."),
    ("<Up>", "vim-insert-up", Edit::Up, "Up a line."),
    ("<Down>", "vim-insert-down", Edit::Down, "Down a line."),
];

impl Extension for Vim {
    fn name(&self) -> &'static str {
        "die-vim"
    }

    fn init(&mut self, ctx: &mut Ctx) -> Result {
        // A buffer killed in insert state never gets its Escape, nor one with an
        // operator waiting its motion.
        ctx.on_buffer_killed(|ctx, b| {
            ctx.with_ext(|v: &mut Vim, _| {
                v.inserts.remove(&b);
                v.pending.remove(&b);
            })?;
            Ok(())
        })
        .detach();
        Ok(())
    }

    fn register(&mut self, reg: &mut Registry) {
        use State::{Insert, Normal, OperatorPending};
        let k = reg.kind_keymap("text");
        // Motions work the same in normal and operator-pending state: `run_motion` looks
        // for a waiting operator.
        for state in [Normal, OperatorPending] {
            for m in MOTIONS {
                k.bind(state, m.keys, m.name);
            }
            // Escape while a key waits for another, and Tab is a character to search for.
            for m in MOTIONS {
                if let Some(key) = m.keys.strip_suffix("<char>") {
                    k.bind(state, &format!("{key}<Esc>"), "vim-cancel");
                    k.bind(state, &format!("{key}<Tab>"), m.name);
                }
            }
            for (keys, cmd) in [
                ("<Left>", "h"),
                ("<Right>", "l"),
                ("<Up>", "k"),
                ("<Down>", "j"),
            ] {
                let m = MOTIONS
                    .iter()
                    .find(|m| m.keys == cmd)
                    .expect("bound motion");
                k.bind(state, keys, m.name);
            }
            for d in '1'..='9' {
                k.bind(state, &d.to_string(), "vim-digit-argument");
            }
            k.bind(state, "0", "vim-beginning-of-line")
                .bind(state, ";", "vim-repeat-find-char")
                .bind(state, ",", "vim-repeat-find-char-reverse")
                .bind(state, "<C-a>", "vim-line-beginning");
        }
        k.bind(Normal, "u", "vim-undo")
            .bind(Normal, "<C-r>", "vim-redo")
            .bind(Normal, "<Esc>", "vim-force-normal-state");
        for (keys, name, ..) in INSERTS {
            k.bind(Normal, keys, name);
        }
        for (keys, again, name, ..) in OPERATORS {
            k.bind(Normal, keys, name)
                .bind(OperatorPending, again, name);
            if keys != again {
                k.bind(OperatorPending, keys, name);
            }
        }
        for (keys, name, ..) in OBJECTS {
            k.bind(OperatorPending, keys, name);
        }
        // Anything else cancels an operator, and so does a key that fails under a prefix,
        // which core wouldn't try against <any> (contract §7).
        for keys in [
            "<Esc>", "<any>", "i<any>", "a<any>", "g<any>", "f<any>", "F<any>", "t<any>", "T<any>",
        ] {
            k.bind(OperatorPending, keys, "vim-cancel");
        }
        for (keys, name, ..) in OWN_MOTION {
            k.bind(Normal, keys, name);
        }
        for (keys, name, ..) in OWN_LINES {
            k.bind(Normal, keys, name);
        }
        k.bind(Normal, "r<any>", "vim-replace")
            .bind(Normal, "J", "vim-join")
            .bind(Normal, "p", "vim-paste-after")
            .bind(Normal, "P", "vim-paste-before")
            .bind(Normal, "\"<any>", "vim-use-register");
        k.bind(Insert, "<char>", "vim-insert-char")
            .bind(Insert, "<Esc>", "vim-normal-state");
        for (keys, name, ..) in EDITS {
            k.bind(Insert, keys, name);
        }

        for m in MOTIONS {
            match m.keys.strip_suffix("<char>") {
                // f, F, t, T: remember the search for ; and ,
                Some(key) => {
                    let key = key.chars().next().expect("a key");
                    reg.command(m.name, m.doc, move |ctx, a| {
                        let find = Find::new(key, char_key(a));
                        ctx.with_ext(|v: &mut Vim, _| v.find = Some(find))?;
                        run_motion(ctx, a, m.kind, char_key(a), m.run)
                    });
                }
                None => reg.command(m.name, m.doc, move |ctx, a| {
                    run_motion(ctx, a, m.kind, None, m.run)
                }),
            }
        }
        reg.command(
            "vim-cancel",
            "Escape after a key that waits for another: only the operator, the count and the goal column go.",
            operators::cancel,
        );
        for &(_, _, name, op, doc) in OPERATORS {
            reg.command(name, doc, move |ctx, a| operators::on_key(ctx, a, op));
        }
        let mut objects: Vec<&str> = vec![];
        for &(_, name, object, doc) in OBJECTS {
            // Several keys can name one object, as `i(`, `i)` and `ib` do.
            if !objects.contains(&name) {
                objects.push(name);
                reg.command(name, doc, move |ctx, a| {
                    operators::on_object(ctx, a, object)
                });
            }
        }
        for &(_, name, op, kind, motion, doc) in OWN_MOTION {
            reg.command(name, doc, move |ctx, a| {
                operators::with_motion(ctx, a, op, kind, motion)
            });
        }
        for &(_, name, op, doc) in OWN_LINES {
            reg.command(name, doc, move |ctx, a| operators::with_lines(ctx, a, op));
        }
        reg.command(
            "vim-replace",
            "r: replace count characters with the next key.",
            operators::replace,
        );
        reg.command("vim-join", "J: join count lines.", operators::join);
        reg.command(
            "vim-paste-after",
            "p: paste after the cursor, or below this line.",
            |ctx, a| operators::paste(ctx, a, false),
        );
        reg.command(
            "vim-paste-before",
            "P: paste before the cursor, or above this line.",
            |ctx, a| operators::paste(ctx, a, true),
        );
        reg.command(
            "vim-use-register",
            "\"x: use a register for the next command.",
            operators::name_register,
        );
        reg.command("vim-digit-argument", "A count digit.", |ctx, a| {
            let d = a.char().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
            count_digit(ctx, a, d)
        });
        reg.command(
            "vim-beginning-of-line",
            "0: to the start of the line, or a count digit.",
            |ctx, a| match a.count {
                Some(_) => count_digit(ctx, a, 0),
                None => run_motion(ctx, a, Kind::Exclusive, None, motions::beginning_of_line),
            },
        );
        reg.command(
            "vim-line-beginning",
            "C-a: to the start of the screen row, as Emacs's global C-a does under your Evil.",
            |ctx, a| {
                let (_, w) = buffer_window(ctx, a)?;
                run_motion(ctx, a, Kind::Exclusive, None, move |s| {
                    motions::row_beginning(s, w)
                })
            },
        );
        reg.command(
            "vim-repeat-find-char",
            ";: repeat the last f, F, t or T.",
            |ctx, a| repeat_find(ctx, a, false),
        );
        reg.command(
            "vim-repeat-find-char-reverse",
            ",: repeat it the other way.",
            |ctx, a| repeat_find(ctx, a, true),
        );
        reg.command(
            "vim-force-normal-state",
            "Escape in normal state: forget the goal column, as Evil does.",
            |ctx, a| {
                let (b, w) = buffer_window(ctx, a)?;
                forget_goal(ctx.text_mut(b)?, w);
                Ok(())
            },
        );
        reg.command("vim-undo", "u: undo count changes.", |ctx, a| {
            history(ctx, a, false)
        });
        reg.command("vim-redo", "C-r: redo count changes.", |ctx, a| {
            history(ctx, a, true)
        });

        for &(_, name, entry, doc) in INSERTS {
            reg.command(name, doc, move |ctx, a| {
                let (b, _) = buffer_window(ctx, a)?;
                let s = insert::begin(ctx, a, entry)?;
                ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
                Ok(())
            });
        }
        reg.command("vim-insert-char", "Insert the typed text.", |ctx, a| {
            insert_edit(ctx, a, Edit::Text(a.typed().unwrap_or_default()))
        });
        for (_, name, edit, doc) in EDITS {
            reg.command(name, doc, move |ctx, a| insert_edit(ctx, a, edit.clone()));
        }
        reg.command(
            "vim-normal-state",
            "Escape: back to normal state.",
            |ctx, a| {
                let (b, _) = buffer_window(ctx, a)?;
                let s = ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
                insert::end(ctx, a, s)
            },
        );
    }
}

/// One insert-state edit. The session is checked out of `Vim` while it's applied
/// (contract §9: read state, release it, act, write it back).
fn insert_edit(ctx: &mut Ctx, a: &Args, e: Edit) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let s = ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
    let mut s = s.unwrap_or_else(Session::bare);
    let r = insert::edit(ctx, a, &mut s, e);
    ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
    r
}

fn repeat_find(ctx: &mut Ctx, a: &Args, reverse: bool) -> Result {
    let last = ctx.with_ext(|v: &mut Vim, _| v.find)?;
    let Some(mut find) = last else {
        operators::cancel(ctx, a)?;
        return Err(Error::Msg("No previous f, F, t or T".into()));
    };
    if reverse {
        find = find.reversed();
    }
    find.repeat = true;
    // As in Evil: a repeat forward includes the target, one backward doesn't.
    let kind = match find.dir {
        Dir::Forward => Kind::Inclusive,
        Dir::Backward => Kind::Exclusive,
    };
    run_motion(ctx, a, kind, None, move |s| motions::find(s, find))
}

/// `u` and `C-r`. Like Evil, `u` leaves the cursor where it was before the last
/// step undone (core restores it); `C-r` puts it at the start of the last step
/// redone, as Vim does. The empty line after a final newline keeps the cursor, as
/// `insert::end` does.
fn history(ctx: &mut Ctx, a: &Args, redo: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let mut stopped = Ok(());
    for i in 0..a.count.unwrap_or(1).max(1) {
        match if redo { t.redo() } else { t.undo() } {
            Ok(_) => {}
            Err(e) if i == 0 => return Err(e),
            Err(Error::NothingToUndo) => break,
            // A conflict is reported only once, so a count can't swallow it.
            Err(e) => {
                stopped = Err(e);
                break;
            }
        }
    }
    let p = t.cursor(w).unwrap_or(0);
    t.set_cursor(w, text::rest_pos(t, p));
    stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn killing_a_buffer_with_an_operator_waiting_drops_it() {
        let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
        let b = ctx.create_text("t", "a\n");
        let w = ctx.open_window(b, 80, 24).expect("a window");
        ctx.focus(w);
        ctx.type_keys("d");
        let waiting = |ctx: &mut Ctx| ctx.with_ext(|v: &mut Vim, _| v.pending.len()).unwrap();
        assert_eq!(waiting(&mut ctx), 1);
        assert!(ctx.kill(b));
        ctx.run_until_idle();
        assert_eq!(waiting(&mut ctx), 0);
    }

    #[test]
    fn killing_a_buffer_in_insert_state_drops_its_session() {
        let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
        let b = ctx.create_text("t", "a\n");
        let w = ctx.open_window(b, 80, 24).expect("a window");
        ctx.focus(w);
        ctx.type_keys("ifoo");
        let sessions = |ctx: &mut Ctx| ctx.with_ext(|v: &mut Vim, _| v.inserts.len()).unwrap();
        assert_eq!(sessions(&mut ctx), 1);
        assert!(ctx.kill(b));
        ctx.run_until_idle();
        assert_eq!(sessions(&mut ctx), 0);
    }
}
