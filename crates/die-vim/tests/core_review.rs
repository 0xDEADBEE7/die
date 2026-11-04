//! die-vim's side of the core review's findings: where `u` leaves the cursor (DR-01),
//! what an undefined key does to a count (DR-03), and insert state for a window that
//! isn't focused (DR-04). The oracle checks the cursor cases too, in `undo-cursor.json`.

use die_core::{Args, Ctx, Key, KeyEvent, Mods, State};
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
