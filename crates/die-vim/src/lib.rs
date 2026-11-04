//! die-vim: DIE's Vim grammar (scope §10). It depends on die-core only (D15), and
//! its behaviour is your Evil's, checked case by case by the Evil oracle
//! (`tests/oracle.rs`), except the deviations named in `tests/oracle/deviations.json`.
//!
//! `grammar.rs` says how counts, motions, operators and insert sessions fit
//! together. So far (V1): motions, counts, and insert sessions with their undo.

mod grammar;
mod insert;
mod motions;
mod scripts;
mod text;

use die_core::{Args, BufferId, Ctx, Dir, Error, Extension, Registry, Result, State};
use grammar::{Kind, buffer_window, count_digit, run_motion};
use insert::{Edit, Entry, Session};
use motions::{Find, MOTIONS};
use std::collections::HashMap;

/// The die-vim extension.
#[derive(Default)]
pub struct Vim {
    /// Each buffer's insert session. Core keeps insert state per buffer, and
    /// Evil keeps a session's count and keys per buffer too.
    inserts: HashMap<BufferId, Session>,
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
        // A buffer killed in insert state never gets its Escape.
        ctx.on_buffer_killed(|ctx, b| {
            ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
            Ok(())
        })
        .detach();
        Ok(())
    }

    fn register(&mut self, reg: &mut Registry) {
        use State::{Insert, Normal};
        let k = reg.kind_keymap("text");
        for m in MOTIONS {
            k.bind(Normal, m.keys, m.name);
        }
        for key in ["f", "F", "t", "T"] {
            k.bind(Normal, &format!("{key}<Esc>"), "vim-cancel");
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
            k.bind(Normal, keys, m.name);
        }
        for d in '1'..='9' {
            k.bind(Normal, &d.to_string(), "vim-digit-argument");
        }
        k.bind(Normal, "0", "vim-beginning-of-line")
            .bind(Normal, ";", "vim-repeat-find-char")
            .bind(Normal, ",", "vim-repeat-find-char-reverse")
            .bind(Normal, "u", "vim-undo")
            .bind(Normal, "<C-r>", "vim-redo")
            .bind(Normal, "<C-a>", "vim-line-beginning")
            .bind(Normal, "<Esc>", "vim-force-normal-state");
        for (keys, name, ..) in INSERTS {
            k.bind(Normal, keys, name);
        }
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
                        let find = Find::new(key, a.char());
                        ctx.with_ext(|v: &mut Vim, _| v.find = Some(find))?;
                        run_motion(ctx, a, m.kind, a.char(), m.run)
                    });
                }
                None => reg.command(m.name, m.doc, move |ctx, a| {
                    run_motion(ctx, a, m.kind, None, m.run)
                }),
            }
        }
        reg.command(
            "vim-cancel",
            "Escape after a key that waits for another: nothing, and the count goes.",
            |_, _| Ok(()),
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
                let t = ctx.text_mut(b)?;
                if let Some(sel) = t.selection(w) {
                    t.set_selection(w, sel.with_goal(None));
                }
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
    let mut find = last.ok_or(Error::Msg("No previous f, F, t or T".into()))?;
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

/// `u` and `C-r`. Like Evil, the cursor goes to the start of the last step undone or redone.
fn history(ctx: &mut Ctx, a: &Args, redo: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let mut start = 0;
    for i in 0..a.count.unwrap_or(1).max(1) {
        match if redo { t.redo() } else { t.undo() } {
            Ok(r) => start = r.start,
            Err(e) if i == 0 => return Err(e),
            Err(_) => break,
        }
    }
    t.set_cursor(w, text::normal_pos(t, start));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
