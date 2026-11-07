//! die-vim: DIE's Vim grammar (scope §10). It depends on die-core only (D15), and
//! its behaviour is your Evil's, checked case by case by the Evil oracle
//! (`tests/oracle.rs`), except the deviations named in `tests/oracle/deviations.json`.
//!
//! `grammar.rs` says how counts, motions, operators and insert sessions fit
//! together. So far (V1 to V4): motions, counts, insert and replace sessions with their
//! undo, operators, registers, text objects, visual state (`visual.rs`), `.` and macros
//! (`repeat.rs`), scrolling (`scroll.rs`), search (`search.rs`), windows (`window.rs`), and
//! `:e`, `:b` and `:N` (`ex.rs`), and marks and the jump list (`marks.rs`). `registers.rs` is the one
//! place that touches registers, for K6.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

mod complete;
mod ex;
mod grammar;
mod insert;
mod insert_keys;
mod marks;
mod motions;
mod objects;
mod operators;
mod registers;
mod repeat;
mod scripts;
mod scroll;
mod search;
mod substitute;
mod text;
mod visual;
mod window;
pub use window::quit_app;

use die_core::key_notation;
use die_core::{
    Args, BufferId, Ctx, Dir, Error, Extension, Prefix, Registry, Result, Shape, State, WindowId,
};
use grammar::{
    CountWas, Kind, Operator, buffer_window, char_key, count_digit, forget_goal, run_motion,
};
use insert::{Edit, Entry, Session};
use motions::{Find, MOTIONS};
use objects::OBJECTS;
use operators::Pending;
use repeat::{Change, Recorder};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
    /// The last search, which `n` and `N` repeat.
    search: Option<search::Last>,
    /// Each buffer's last selection, for `gv`.
    last_visual: HashMap<BufferId, visual::Last>,
    /// The register named before a selection began: its operator uses it, as your Evil's does.
    visual_register: Option<char>,
    /// What `.` and macros keep, which a key observer shares.
    rec: Rc<RefCell<Recorder>>,
    /// Each buffer's `C-o` waiting for its command.
    once: HashMap<BufferId, insert_keys::Once>,
    /// Commands run so far, for a command that acts on being run twice in a row.
    commands: u64,
    /// What `C-p` and `C-n` remember between presses.
    dabbrev: complete::Dabbrev,
    /// The last `C-y` or `C-e`, whose count the next press keeps.
    last_copy: Option<insert_keys::LastCopy>,
    /// The code `C-v` has read so far, in octal, and the count for what it puts in.
    quoted: Option<(u32, Option<usize>)>,
    /// What each window showed, and where, as the last command left it.
    seen: HashMap<WindowId, (BufferId, usize)>,
    /// Each buffer's `g;` and `g,`.
    changes: HashMap<BufferId, marks::Changes>,
    /// How the count waiting in core's prefix came about.
    count_was: grammar::CountWas,
    /// The last `:s`'s replacement and whether it had `g`, which `&` and `~` use.
    substitute: (String, bool),
}

#[rustfmt::skip]
const INSERTS: &[(&str, &str, Entry, &str)] = &[
    ("i", "vim-insert", Entry::Insert, "Insert before the cursor."),
    ("a", "vim-append", Entry::Append, "Insert after the cursor."),
    ("I", "vim-insert-line", Entry::InsertLine, "Insert at the line's first non-blank."),
    ("A", "vim-append-line", Entry::AppendLine, "Insert at the end of the line."),
    ("o", "vim-open-below", Entry::OpenBelow, "Open a line below, indented like this one."),
    ("O", "vim-open-above", Entry::OpenAbove, "Open a line above, indented like the one above it."),
    ("R", "vim-replace-state", Entry::Replace, "Type over the text, until Escape."),
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
    ("=", "=", "vim-indent", Operator::Indent, "Indent lines like the line above each."),
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
    ("<Del>", "vim-insert-delete", Edit::Delete, "Delete the character at the cursor."),
    ("<C-t>", "vim-shift-right-line", Edit::Shift { right: true }, "Shift the line right."),
];

pub(crate) type Body = fn(&mut Ctx, &Args) -> Result;

/// The other commands.
#[rustfmt::skip]
const COMMANDS: &[(&str, &str, Body)] = &[
    ("vim-ex", ":: read a line and run it.", ex::open),
    ("vim-search-forward", "/: read a pattern to search forward for.", |ctx, a| search::open(ctx, a, true)),
    ("vim-search-backward", "?: read a pattern to search backward for.", |ctx, a| search::open(ctx, a, false)),
    ("vim-search-next", "n: the last search again.", |ctx, a| search::again(ctx, a, false)),
    ("vim-search-previous", "N: the last search again, the other way.", |ctx, a| search::again(ctx, a, true)),
    ("vim-search-word-forward", "*: search forward for the word under the cursor.", |ctx, a| search::word(ctx, a, true)),
    ("vim-search-word-backward", "#: search backward for the word under the cursor.", |ctx, a| search::word(ctx, a, false)),
    ("vim-goto-definition", "gd: to the first place in the text with the symbol under the cursor, as your Evil's falls back to.", search::definition),
    ("vim-cancel", "Escape after a key that waits for another: only the operator, the count and the goal column go.", operators::cancel),
    ("vim-replace", "r: replace count characters with the next key.", operators::replace),
    ("vim-join", "J: join count lines.", |ctx, a| operators::join(ctx, a, false)),
    ("vim-join-whitespace", "gJ: join count lines, keeping their whitespace.", |ctx, a| operators::join(ctx, a, true)),
    ("vim-find-file-at-point", "gf, gF: open the file named under the cursor, at its line and column if it gives them.", ex::find_file_at_point),
    ("vim-what-cursor-position", "ga: say what the character under the cursor is, and where.", operators::what_cursor_position),
    ("vim-repeat-substitute", "&: the last :s again on this line, or count lines, or the selection, without its flags.", substitute::again),
    ("vim-repeat-substitute-all", "g&: the last :s again on every line, with its flags.", substitute::again_everywhere),
    ("vim-paste-after", "p: paste after the cursor, or below this line.", |ctx, a| operators::paste(ctx, a, false)),
    ("vim-paste-before", "P: paste before the cursor, or above this line.", |ctx, a| operators::paste(ctx, a, true)),
    ("vim-use-register", "\"x: use a register for the next command.", operators::name_register),
    ("vim-digit-argument", "A count digit.", |ctx, a| count_digit(ctx, a, a.char().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize)),
    ("vim-beginning-of-line", "0: to the start of the line, or a count digit.", zero),
    ("vim-line-beginning", "C-a: to the start of the screen row, as Emacs's global C-a does under your Evil.", |ctx, a| run_motion(ctx, a, Kind::Exclusive, None, motions::row_beginning)),
    ("vim-universal-argument", "C-u: the prefix argument, as in your Evil: 4, four times an earlier C-u, or the digits already typed.", universal_argument),
    ("vim-scroll-down", "C-d: scroll half a window down, or count rows, and the cursor with it.", scroll::half_down),
    ("vim-scroll-page-down", "C-f: scroll count windows down, less two rows.", |ctx, a| scroll::page(ctx, a, true)),
    ("vim-scroll-page-up", "C-b: scroll count windows up, less two rows.", |ctx, a| scroll::page(ctx, a, false)),
    ("vim-scroll-line-down", "C-e: scroll count rows down.", |ctx, a| scroll::rows(ctx, a, true)),
    ("vim-scroll-line-up", "C-y: scroll count rows up.", |ctx, a| scroll::rows(ctx, a, false)),
    ("vim-scroll-line-to-center", "zz: put the cursor's line, or line count, in the window's middle.", |ctx, a| scroll::put_line(ctx, a, |rows| rows / 2)),
    ("vim-scroll-line-to-top", "zt: put the cursor's line, or line count, at the window's top.", |ctx, a| scroll::put_line(ctx, a, |_| 0)),
    ("vim-scroll-line-to-bottom", "zb: put the cursor's line, or line count, at the window's bottom.", |ctx, a| scroll::put_line(ctx, a, |rows| rows.saturating_sub(1))),
    ("vim-repeat-find-char", ";: repeat the last f, F, t or T.", |ctx, a| repeat_find(ctx, a, false)),
    ("vim-repeat-find-char-reverse", ",: repeat it the other way.", |ctx, a| repeat_find(ctx, a, true)),
    ("vim-force-normal-state", "Escape in normal state: forget the goal column, as Evil does.", force_normal_state),
    ("vim-undo", "u: undo count changes.", |ctx, a| history(ctx, a, false)),
    ("vim-redo", "C-r: redo count changes.", |ctx, a| history(ctx, a, true)),
    ("vim-replace-char", "Type over a character.", |ctx, a| typed_char(ctx, a, Edit::Over(a.typed().unwrap_or_default()))),
    ("vim-replace-backspace", "Back a character, and put back what typing over it took.", |ctx, a| insert_edit(ctx, a, Edit::Restore)),
    ("vim-replace-backward-word", "Back to the start of a word, putting back what typing over it took.", |ctx, a| insert_edit(ctx, a, Edit::RestoreWord)),
    ("vim-repeat", ".: repeat the last change.", repeat::dot),
    ("vim-set-mark", "m: set the named mark at the cursor.", marks::set),
    ("vim-goto-mark", "`: to the named mark.", |ctx, a| marks::goto(ctx, a, false)),
    ("vim-goto-mark-line", "': to the first non-blank of the named mark's line.", |ctx, a| marks::goto(ctx, a, true)),
    ("vim-jump-backward", "C-o: to an older place in the jump list.", |ctx, a| marks::jump(ctx, a, true)),
    ("vim-jump-forward", "C-i, which is Tab: to a newer place in the jump list.", |ctx, a| marks::jump(ctx, a, false)),
    ("vim-insert-resume", "gi: insert where insert state last ended.", marks::insert_resume),
    ("vim-goto-last-change", "g;: to an older place in the change list.", |ctx, a| marks::change_step(ctx, a, true)),
    ("vim-goto-last-change-reverse", "g,: to a newer place in the change list.", |ctx, a| marks::change_step(ctx, a, false)),
    ("vim-record-macro", "q: stop recording, or ask which register to record into.", repeat::q),
    ("vim-record-macro-into", "The key after q: start recording into that register.", repeat::start_recording),
    ("vim-execute-macro", "@x: type register x, count times. @@ repeats the last.", repeat::at),
    ("vim-visual-char", "v: select characters.", |ctx, a| visual::enter(ctx, a, Shape::Char)),
    ("vim-visual-line", "V: select whole lines.", |ctx, a| visual::enter(ctx, a, Shape::Line)),
    ("vim-visual-block", "C-v: select a rectangle.", |ctx, a| visual::enter(ctx, a, Shape::Block)),
    ("vim-visual-restore", "gv: select the last selection again.", visual::restore),
    ("vim-exit-visual", "Escape: stop selecting.", visual::escape),
    ("vim-visual-swap", "o: to the selection's other end.", |ctx, a| visual::swap(ctx, a, false)),
    ("vim-visual-swap-corner", "O: to the block's other corner on this line.", |ctx, a| visual::swap(ctx, a, true)),
    ("vim-visual-insert", "I: insert at the selection's start, on each line of a block or of whole lines.", |ctx, a| visual::insert(ctx, a, false)),
    ("vim-visual-append", "A: insert after the selection, on each line of a block or of whole lines.", |ctx, a| visual::insert(ctx, a, true)),
    ("vim-insert-char", "Insert the typed text.", |ctx, a| typed_char(ctx, a, Edit::Text(a.typed().unwrap_or_default()))),
    ("vim-execute-normal", "C-o: one command in normal state, then back.", insert_keys::execute_normal),
    ("vim-complete-previous", "C-p: complete from words before the cursor, then after it, then in other buffers.", complete::previous),
    ("vim-complete-next", "C-n: complete from words after the cursor, then other buffers, then before it.", complete::next),
    ("vim-shift-left-line", "C-d: shift the line left. After a typed 0, take away its indentation.", insert_keys::shift_left),
    ("vim-paste-from-register", "C-r x: put in a register's text.", insert_keys::paste_register),
    ("vim-paste-last-insertion", "C-a: put in the last insertion.", |ctx, a| insert_keys::paste(ctx, a, '.')),
    ("vim-paste-last-insertion-stop", "C-@: put in the last insertion, and go to normal state.", insert_keys::paste_and_stop),
    ("vim-copy-from-above", "C-y: copy from the line above.", |ctx, a| insert_keys::copy(ctx, a, true)),
    ("vim-copy-from-below", "C-e: copy from the line below.", |ctx, a| insert_keys::copy(ctx, a, false)),
    ("vim-quoted-insert", "C-v x: put in the next key as it is, or a code in octal.", insert_keys::quoted),
    ("vim-normal-state", "Escape: back to normal state.", end_session),
];

impl Extension for Vim {
    fn name(&self) -> &'static str {
        "die-vim"
    }

    fn init(&mut self, ctx: &mut Ctx) -> Result {
        repeat::observe(ctx, self.rec.clone());
        ctx.set_cursor_rest(text::rest_pos);
        ctx.set_insert_end(insert::stepped_back);
        // A buffer killed in insert state never gets its Escape, nor one with an
        // operator waiting its motion.
        ctx.on_buffer_killed(|ctx, b| {
            ctx.with_ext(|v: &mut Vim, _| {
                v.inserts.remove(&b);
                v.once.remove(&b);
                v.pending.remove(&b);
                v.last_visual.remove(&b);
                v.changes.remove(&b);
            })?;
            Ok(())
        })
        .detach();
        Ok(())
    }

    fn register(&mut self, reg: &mut Registry) {
        use State::{CommandLine, Insert, Normal, OperatorPending, Replace, Visual};
        let k = reg.kind_keymap("text");
        // Motions work the same in normal and operator-pending state: `run_motion` looks
        // for a waiting operator.
        for state in [Normal, OperatorPending, Visual] {
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
                .bind(state, "<C-a>", "vim-line-beginning")
                .bind(state, "<C-u>", "vim-universal-argument")
                .bind(state, "`<any>", "vim-goto-mark")
                .bind(state, "'<any>", "vim-goto-mark-line")
                .bind(state, "<C-o>", "vim-jump-backward")
                .bind(state, "<Tab>", "vim-jump-forward");
        }
        k.bind(Normal, "gf", "vim-find-file-at-point")
            .bind(Normal, "gF", "vim-find-file-at-point")
            .bind(Normal, "u", "vim-undo")
            .bind(Normal, "<C-r>", "vim-redo")
            .bind(Normal, "<Esc>", "vim-force-normal-state");
        for (keys, name, ..) in INSERTS {
            k.bind(Normal, keys, name);
        }
        for (keys, again, name, ..) in OPERATORS {
            k.bind(Normal, keys, name)
                .bind(Visual, keys, name)
                .bind(OperatorPending, again, name);
            if keys != again {
                k.bind(OperatorPending, keys, name);
            }
        }
        for (keys, name, ..) in OBJECTS {
            k.bind(OperatorPending, keys, name).bind(Visual, keys, name);
        }
        // Anything else cancels an operator, and so does a key that fails under a prefix,
        // which core wouldn't try against <any> (contract §7).
        for keys in [
            "<Esc>", "<any>", "i<any>", "a<any>", "g<any>", "[<any>", "]<any>", "f<any>", "F<any>",
            "t<any>", "T<any>",
        ] {
            k.bind(OperatorPending, keys, "vim-cancel");
        }
        for state in [Normal, Visual] {
            for (keys, name, ..) in OWN_MOTION {
                k.bind(state, keys, name);
            }
            for (keys, name, ..) in OWN_LINES {
                k.bind(state, keys, name);
            }
            k.bind(state, "r<any>", "vim-replace")
                .bind(state, "J", "vim-join")
                .bind(state, "gJ", "vim-join-whitespace")
                .bind(state, "ga", "vim-what-cursor-position")
                .bind(state, "&", "vim-repeat-substitute")
                .bind(state, "g&", "vim-repeat-substitute-all")
                .bind(state, "p", "vim-paste-after")
                .bind(state, "P", "vim-paste-before")
                .bind(state, "\"<any>", "vim-use-register")
                .bind(state, "v", "vim-visual-char")
                .bind(state, "V", "vim-visual-line")
                .bind(state, "<C-v>", "vim-visual-block")
                .bind(state, "gv", "vim-visual-restore")
                .bind(state, "m<any>", "vim-set-mark")
                .bind(state, "gi", "vim-insert-resume")
                .bind(state, "g;", "vim-goto-last-change")
                .bind(state, "g,", "vim-goto-last-change-reverse")
                .bind(state, "<C-d>", "vim-scroll-down")
                .bind(state, "<C-f>", "vim-scroll-page-down")
                .bind(state, "<C-b>", "vim-scroll-page-up")
                .bind(state, "<C-e>", "vim-scroll-line-down")
                .bind(state, "<C-y>", "vim-scroll-line-up")
                .bind(state, "zz", "vim-scroll-line-to-center")
                .bind(state, "zt", "vim-scroll-line-to-top")
                .bind(state, "zb", "vim-scroll-line-to-bottom")
                .bind(state, "q", "vim-record-macro")
                .bind(state, "@<any>", "vim-execute-macro");
        }
        for state in [Normal, Visual] {
            k.bind(state, ":", "vim-ex");
        }
        for state in [Normal, OperatorPending, Visual] {
            k.bind(state, "/", "vim-search-forward")
                .bind(state, "?", "vim-search-backward")
                .bind(state, "n", "vim-search-next")
                .bind(state, "N", "vim-search-previous")
                .bind(state, "*", "vim-search-word-forward")
                .bind(state, "#", "vim-search-word-backward")
                .bind(state, "gd", "vim-goto-definition");
        }
        for &(keys, name, ..) in window::MAP {
            for key in keys.split_whitespace() {
                let keys = format!("<C-w>{key}");
                k.bind(Normal, &keys, name).bind(Visual, &keys, name);
            }
        }
        k.bind(Normal, ".", "vim-repeat")
            .bind(CommandLine, "<any>", "vim-record-macro-into");
        k.bind(Visual, "<Esc>", "vim-exit-visual")
            .bind(Visual, "o", "vim-visual-swap")
            .bind(Visual, "O", "vim-visual-swap-corner")
            .bind(Visual, "I", "vim-visual-insert")
            .bind(Visual, "A", "vim-visual-append")
            .bind(Visual, "u", "vim-downcase")
            .bind(Visual, "U", "vim-upcase")
            .bind(Visual, "R", "vim-change-whole-line")
            .bind(Visual, "X", "vim-delete-line");
        k.bind(Replace, "<char>", "vim-replace-char")
            .bind(Replace, "<BS>", "vim-replace-backspace")
            .bind(Replace, "<Del>", "vim-replace-backspace")
            .bind(Replace, "<C-w>", "vim-replace-backward-word")
            .bind(Replace, "<Esc>", "vim-normal-state");
        // The rest of insert state's edits do the same here, as your Evil's replace state has them.
        for (keys, name, edit, _) in EDITS {
            if !matches!(edit, Edit::Backspace | Edit::DeleteWord | Edit::Delete) {
                k.bind(Replace, keys, name);
            }
        }
        k.bind(Insert, "<char>", "vim-insert-char")
            .bind(Insert, "<Esc>", "vim-normal-state");
        for (keys, name, ..) in EDITS {
            k.bind(Insert, keys, name);
        }
        for state in [Insert, Replace] {
            k.bind(state, "<C-o>", "vim-execute-normal")
                .bind(state, "<C-p>", "vim-complete-previous")
                .bind(state, "<C-n>", "vim-complete-next")
                .bind(state, "<C-d>", "vim-shift-left-line")
                .bind(state, "<C-a>", "vim-paste-last-insertion")
                .bind(state, "<C-@>", "vim-paste-last-insertion-stop")
                .bind(state, "<C-y>", "vim-copy-from-above")
                .bind(state, "<C-e>", "vim-copy-from-below")
                .bind(state, "<C-r><any>", "vim-paste-from-register")
                .bind(state, "<C-v><any>", "vim-quoted-insert")
                .bind(state, "<C-q><any>", "vim-quoted-insert")
                .bind(state, "<C-u>", "vim-universal-argument");
        }

        for m in MOTIONS {
            match m.keys.strip_suffix("<char>") {
                // f, F, t, T: remember the search for ; and ,
                Some(key) => {
                    let key = key.chars().next().expect("a key");
                    command(reg, m.name, m.doc, move |ctx, a| {
                        let find = Find::new(key, char_key(a));
                        ctx.with_ext(|v: &mut Vim, _| v.find = Some(find))?;
                        insert_keys::motion(ctx, a, |ctx| {
                            run_motion(ctx, a, m.kind, char_key(a), m.run)
                        })
                    });
                }
                None => command(reg, m.name, m.doc, move |ctx, a| {
                    insert_keys::motion(ctx, a, |ctx| run_motion(ctx, a, m.kind, None, m.run))
                }),
            }
        }
        for &(_, _, name, op, doc) in OPERATORS {
            command(reg, name, doc, move |ctx, a| operators::on_key(ctx, a, op));
        }
        let mut objects: Vec<&str> = vec![];
        for &(_, name, object, doc) in OBJECTS {
            // Several keys can name one object, as `i(`, `i)` and `ib` do.
            if !objects.contains(&name) {
                objects.push(name);
                let shape = visual::ObjectShape::of(name);
                command(reg, name, doc, move |ctx, a| {
                    operators::on_object(ctx, a, object, shape)
                });
            }
        }
        for &(_, name, op, kind, motion, doc) in OWN_MOTION {
            command(reg, name, doc, move |ctx, a| {
                operators::with_motion(ctx, a, op, kind, motion)
            });
        }
        for &(_, name, op, doc) in OWN_LINES {
            command(reg, name, doc, move |ctx, a| {
                operators::with_lines(ctx, a, op)
            });
        }
        for &(_, name, entry, doc) in INSERTS {
            command(reg, name, doc, move |ctx, a| {
                let (b, _) = buffer_window(ctx, a)?;
                let mut s = insert::begin(ctx, a, entry)?;
                s.repeat = Change::of(a).typed();
                ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
                Ok(())
            });
        }
        for (_, name, edit, doc) in EDITS {
            command(reg, name, doc, move |ctx, a| {
                insert_edit(ctx, a, edit.clone())
            });
        }
        for &(name, doc, f) in COMMANDS {
            command(reg, name, doc, f);
        }
        for &(_, name, doc, f) in window::MAP {
            command(reg, name, doc, move |ctx, a| window::run(ctx, a, f));
        }
    }
}

/// Register a command. Every one goes through here so that [`repeat::before`] runs first, with
/// [`marks::before`], and [`scroll::reveal`] after it, with [`marks::after`].
fn command(
    reg: &mut Registry,
    name: &str,
    doc: &'static str,
    f: impl Fn(&mut Ctx, &Args) -> Result + 'static,
) {
    let (jump, quiet) = (marks::sets_jump(name), marks::says_its_jump(name));
    reg.command(name, doc, move |ctx, a| {
        repeat::before(ctx)?;
        ctx.with_ext(|v: &mut Vim, _| v.commands += 1)?;
        marks::before(ctx, a, jump);
        let r = f(ctx, a);
        marks::after(ctx, a, quiet);
        scroll::reveal(ctx, a);
        insert_keys::after_command(ctx, a, &r);
        r
    });
}

/// `0`: a count digit if a count has begun, else a motion.
fn zero(ctx: &mut Ctx, a: &Args) -> Result {
    let was = ctx.with_ext(|v: &mut Vim, _| v.count_was)?;
    match a.count {
        Some(_) if was != CountWas::Kept => count_digit(ctx, a, 0),
        _ => run_motion(ctx, a, Kind::Exclusive, None, motions::beginning_of_line),
    }
}

/// `C-u`: your Evil leaves it Emacs's prefix argument, so it scrolls nothing. It's no motion, so
/// it ends a waiting operator, and that operator's count goes too.
fn universal_argument(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let waiting = ctx.state_of(b) == State::OperatorPending;
    if waiting {
        operators::cancel(ctx, a)?;
    }
    let was = ctx.with_ext(|v: &mut Vim, _| v.count_was)?;
    let (count, now) = match a.count.filter(|_| !waiting) {
        Some(n) if was == CountWas::Universal => (n.saturating_mul(4), CountWas::Universal),
        Some(n) if was == CountWas::Digits => (n, CountWas::Kept),
        _ => (4, CountWas::Universal),
    };
    ctx.with_ext(|v: &mut Vim, _| v.count_was = now)?;
    ctx.set_prefix(Prefix {
        count: Some(count),
        register: a.register,
    });
    Ok(())
}

/// A printable key in insert or replace state: a digit of `C-u`'s count, while there's one.
fn typed_char(ctx: &mut Ctx, a: &Args, e: Edit) -> Result {
    match a.char().and_then(|c| c.to_digit(10)) {
        Some(d) if a.count.is_some() => count_digit(ctx, a, d as usize),
        _ => insert_edit(ctx, a, e),
    }
}

fn force_normal_state(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    forget_goal(ctx.text_mut(b)?, w);
    Ok(())
}

/// Escape from a session. Its keys, with this one, are what `.` repeats.
pub(crate) fn end_session(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let mut s = ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
    let again = s.as_mut().and_then(|s| s.repeat.take());
    let r = insert::end(ctx, a, s);
    if let Some(mut c) = again {
        c.keys.push_str(&key_notation(&a.keys));
        repeat::commit(ctx, c)?;
    }
    r
}

/// One insert-state edit. The session is checked out of `Vim` while it's applied
/// (contract §9: read state, release it, act, write it back).
pub(crate) fn insert_edit(ctx: &mut Ctx, a: &Args, e: Edit) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let s = ctx.with_ext(|v: &mut Vim, _| v.inserts.remove(&b))?;
    let mut s = s.unwrap_or_else(Session::bare);
    let kill = insert::killed(ctx, a, &e);
    let r = kill.clone().and_then(|_| insert::edit(ctx, a, &mut s, e));
    if let (Ok(()), Some(c)) = (&r, &mut s.repeat) {
        c.keys.push_str(&key_notation(&a.keys));
    }
    ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
    if let (Ok(()), Ok(Some(k))) = (&r, kill) {
        registers::save(ctx, None, k, false)?;
    }
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
    // Even if it fails, the goal column goes, as after any command but `j` and `k`.
    forget_goal(t, w);
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
