//! die-vim's side of the core review's findings: where `u` leaves the cursor (DR-01),
//! what an undefined key does to a count (DR-03), and insert state for a window that
//! isn't focused (DR-04). The oracle checks the cursor cases too, in `undo-cursor.json`.

use die_test_cap as _;

use die_core::{Args, Ctx, Deco, Key, KeyEvent, Mods, Origin, State};
use die_vim::Vim;

fn editor() -> Ctx {
    Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
}

fn run(text: &str, cursor: usize, keys: &str) -> (usize, Vec<String>) {
    let mut ctx = editor();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    assert_eq!(ctx.text_mut(b).unwrap().set_cursor(w, cursor), Some(cursor));
    ctx.type_keys(keys);
    let t = ctx.text(b).unwrap();
    (t.cursor(w).unwrap(), ctx.messages.clone())
}

fn cursor_after(text: &str, cursor: usize, keys: &str) -> usize {
    run(text, cursor, keys).0
}

/// Your Evil's `u` restores the point from before the command that made the step's first
/// change (oracle, `undo-cursor`). The old code went to the start of the undone range.
#[test]
fn u_returns_to_where_the_steps_first_change_was_made() {
    for (text, at, keys, want) in [
        ("abc\n", 0, "ox<Esc>u", 0),
        ("abc\ndef\n", 1, "ox<Esc>u", 1),
        ("abc\ndef\n", 5, "Ox<Esc>u", 5),
        ("abc\n", 1, "3ox<Esc>u", 1),
        ("abcdef\n", 2, "ox<Esc>u", 2),
        ("abc\n", 1, "ox<CR>y<Esc>u", 1),
        ("ab\ncd\n", 0, "jAx<Up>y<Esc>u", 4),
        ("abcd\nef\n", 0, "jAx<Up>y<Esc>u", 6),
        ("abcdef\n", 0, "A<BS><BS>x<Esc>u", 5),
    ] {
        assert_eq!(cursor_after(text, at, keys), want, "{keys} in {text:?}");
    }
}

/// These already agreed. Using core's selection from `begin_undo_group` would break the
/// first: the cursor moved after the session began, and before its first change.
#[test]
fn u_keeps_the_cursor_moved_after_a_session_began() {
    for (text, at, keys, want) in [
        ("abc\n", 0, "i<Right>x<Esc>u", 1),
        ("abcdef\n", 0, "i<Right><Right>x<Left><Left>y<Esc>u", 2),
        ("abc\n", 1, "Ax<Esc>u", 2),
        ("  abc\n", 3, "Ix<Esc>u", 2),
    ] {
        assert_eq!(cursor_after(text, at, keys), want, "{keys} in {text:?}");
    }
}

#[test]
fn u_on_the_empty_line_after_a_final_newline_stays_there() {
    assert_eq!(cursor_after("", 0, "ifoo<CR><Esc>ibar<Esc>u"), 4);
}

#[test]
fn c_r_puts_the_cursor_at_the_start_of_the_last_step_redone() {
    assert_eq!(cursor_after("abc\n", 1, "ix<Esc>$u<C-r>"), 1);
}

/// Emacs's `undefined` resets the prefix argument. The oracle's runner can't pin it until
/// K7's per-key log (call 23).
#[test]
fn an_undefined_key_drops_the_count() {
    let (cursor, messages) = run("abcdef\n", 0, "3gzl");
    assert_eq!(
        cursor, 1,
        "l after an undefined key moves once: {messages:?}"
    );
    assert_eq!(messages, ["gz is undefined"]);
}

#[test]
fn insert_entered_for_another_window_puts_that_buffer_in_insert_state() {
    let mut ctx = editor();
    let a = ctx.create_text("a", "aaa\n");
    let wa = ctx.open_window(a, 80, 24).unwrap();
    let b = ctx.create_text("b", "bbb\n");
    let wb = ctx.open_window(b, 80, 24).unwrap();
    ctx.focus(wa);
    let args = Args {
        window: Some(wb),
        buffer: Some(b),
        ..Default::default()
    };
    ctx.run("vim-insert", &args).unwrap();
    assert_eq!(ctx.state_of(a), State::Normal, "A wasn't asked to insert");
    assert_eq!(ctx.state(), State::Normal, "A is the focused buffer");
    assert_eq!(ctx.state_of(b), State::Insert, "B is in insert state");
    ctx.run("vim-normal-state", &args).unwrap();
    assert_eq!(ctx.state_of(b), State::Normal, "and leaves it the same way");
    assert_eq!(ctx.state_of(a), State::Normal);
}

/// Escape is the one way out of insert state so far. Once undo stops resetting a group
/// (DR-14), a session left open would swallow every later edit into one step.
#[test]
fn escape_ends_the_undo_group_for_a_window_that_isnt_focused() {
    let mut ctx = editor();
    let a = ctx.create_text("a", "aaa\n");
    let wa = ctx.open_window(a, 80, 24).unwrap();
    let b = ctx.create_text("b", "bbb\n");
    let wb = ctx.open_window(b, 80, 24).unwrap();
    ctx.focus(wa);
    let args = Args {
        window: Some(wb),
        buffer: Some(b),
        ..Default::default()
    };
    ctx.run("vim-insert", &args).unwrap();
    ctx.focus(wb);
    ctx.type_keys("x<Esc>iy<Esc>u");
    let text = ctx.text(b).unwrap().rope().to_string();
    assert_eq!(text, "xbbb\n", "u undoes only the last session");
}

/// In your GUI Emacs and in Vim, Ctrl-[ is Escape. The OS sends `[` with Ctrl held.
#[test]
fn ctrl_bracket_leaves_insert_state() {
    let mut ctx = editor();
    let b = ctx.create_text("t", "abc\n");
    ctx.open_window(b, 80, 24).unwrap();
    ctx.type_keys("ix");
    assert_eq!(ctx.state(), State::Insert);
    let held = Mods {
        ctrl: true,
        ..Mods::default()
    };
    ctx.input(KeyEvent::press(Key::Char('[')).mods(held));
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal);
    assert_eq!(ctx.text(b).unwrap().rope().to_string(), "xabc\n");
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

/// `u` run for one of two windows on a buffer moves that window's cursor, wherever the focus is.
#[test]
fn u_run_for_a_window_that_isnt_focused_moves_that_windows_cursor() {
    let mut ctx = editor();
    let b = ctx.create_text("b", "abcdef\n");
    let wb = ctx.open_window(b, 80, 24).unwrap();
    let wa = ctx.open_window(b, 80, 24).unwrap();
    ctx.focus(wa);
    ctx.text_mut(b).unwrap().set_cursor(wa, 3);
    ctx.type_keys("ix<Esc>$");
    ctx.text_mut(b).unwrap().set_cursor(wb, 0);
    let args = Args {
        window: Some(wb),
        buffer: Some(b),
        ..Default::default()
    };
    ctx.run("vim-undo", &args).unwrap();
    let t = ctx.text(b).unwrap();
    assert_eq!(t.cursor(wb), Some(3), "the window u ran for goes back");
    assert_eq!(t.cursor(wa), Some(5), "the other one only follows its text");
}

/// The step keeps the selection of the window `o` ran for, though another window has the focus.
#[test]
fn u_returns_to_where_o_was_run_for_a_window_that_isnt_focused() {
    let mut ctx = editor();
    let a = ctx.create_text("a", "aaa\n");
    let wa = ctx.open_window(a, 80, 24).unwrap();
    let b = ctx.create_text("b", "abc\n");
    let wb = ctx.open_window(b, 80, 24).unwrap();
    ctx.text_mut(b).unwrap().set_cursor(wb, 1);
    ctx.focus(wa);
    let args = Args {
        window: Some(wb),
        buffer: Some(b),
        ..Default::default()
    };
    ctx.defer(move |c| c.run("vim-open-below", &args));
    ctx.run_until_idle();
    ctx.focus(wb);
    ctx.type_keys("x<Esc>u");
    let t = ctx.text(b).unwrap();
    assert_eq!(t.rope().to_string(), "abc\n");
    assert_eq!(t.cursor(wb), Some(1), "where o was typed");
}

/// A silent edit inside the last session drops the older history, and the undo that reaches
/// the gap says why, once. A count mustn't swallow that: it's the only report there is.
#[test]
fn u_with_a_count_reports_the_conflict_it_reaches() {
    let mut ctx = editor();
    let b = ctx.create_text("t", "");
    ctx.open_window(b, 80, 24).unwrap();
    ctx.type_keys("iolder <Esc>aab");
    ctx.defer(move |c| {
        let t = c.text_mut(b)?;
        t.tx(Origin::Extension)
            .silent()
            .replace(7..8, "B")
            .commit()?;
        Ok(())
    });
    ctx.run_until_idle();
    ctx.type_keys("c<Esc>3u");
    assert_eq!(ctx.messages, ["UndoConflict"]);
    assert_eq!(ctx.text(b).unwrap().rope().to_string(), "older aB");
}

/// The steps before the read-only one are undone, and `u` says what stopped it.
#[test]
fn u_with_a_count_reports_a_read_only_step() {
    let mut ctx = editor();
    let b = ctx.create_text("t", "xyz\n");
    ctx.open_window(b, 80, 24).unwrap();
    ctx.type_keys("ia<Esc>Ab<Esc>");
    let t = ctx.text_mut(b).unwrap();
    t.decorate(0..1, Deco::ReadOnly).unwrap();
    ctx.type_keys("3u");
    assert_eq!(ctx.text(b).unwrap().rope().to_string(), "axyz\n");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert!(
        ctx.messages[0].starts_with("ReadOnly"),
        "{:?}",
        ctx.messages
    );
}

/// Running out of history partway through a count isn't an error, in either direction.
#[test]
fn a_count_past_the_end_of_history_is_quiet() {
    let (_, messages) = run("abc\n", 0, "ix<Esc>3u3<C-r>");
    assert!(messages.is_empty(), "{messages:?}");
    let (_, messages) = run("abc\n", 0, "u");
    assert_eq!(
        messages,
        ["NothingToUndo"],
        "with nothing at all to do, it says so"
    );
}

/// DR-12: a control character is two columns wide, as Emacs draws it, so `j` keeps Evil's column.
#[test]
fn a_control_character_is_two_columns_wide() {
    assert_eq!(cursor_after("a\rbc\nxyzwv\n", 3, "j"), 9, "after \\r");
    assert_eq!(cursor_after("a\x01bc\nxyzwv\n", 3, "j"), 9, "after \\x01");
}
