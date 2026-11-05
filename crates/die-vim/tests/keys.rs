//! Keys the Evil oracle can't record: Escape while `f` waits for its character is a
//! quit, which ends batch Emacs's keyboard macro. And `j` on the last line with a final
//! newline, which is rule 1's, with no oracle case of its own.

use die_test_cap as _;

use die_core::Ctx;
use die_vim::Vim;

fn run(text: &str, keys: &str) -> (usize, Vec<String>) {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    ctx.type_keys(keys);
    let cursor = ctx.text(b).unwrap().cursor(w).unwrap();
    (cursor, ctx.messages.clone())
}

#[test]
fn escape_cancels_a_pending_find_quietly_and_takes_the_count() {
    for key in ["f", "F", "t", "T"] {
        let (cursor, messages) = run("abcdef\n", &format!("3{key}<Esc>l"));
        assert!(messages.is_empty(), "{key}: {messages:?}");
        assert_eq!(cursor, 1, "{key}: l after the cancel moves once");
    }
}

/// Rule 1: on the last line, j does what your Evil's does without the final newline.
#[test]
fn j_on_the_last_line_signals_only_if_its_goal_column_wouldnt_move_the_cursor() {
    for (text, keys, cursor, fails) in [
        ("abc\n", "2lj", 2, true),
        ("abc\n", "$j", 2, false),
        ("abcdef\nab\n", "4ljj", 8, false),
    ] {
        let (at, messages) = run(text, keys);
        assert_eq!(at, cursor, "{keys}");
        assert_eq!(!messages.is_empty(), fails, "{keys}: {messages:?}");
    }
}

/// Evil's `f` signals quit when Escape cancels it, and a command that ended in a signal
/// loses the goal column like any other. The oracle stops at a quit, so this is here.
#[test]
fn a_cancelled_find_forgets_the_goal_column() {
    for key in ["f", "F", "t", "T"] {
        let (cursor, messages) = run("abcdef\nab\nabcdef\n", &format!("4lj{key}<Esc>j"));
        assert!(messages.is_empty(), "{key}: {messages:?}");
        assert_eq!(cursor, 11, "{key}: j goes to the column the cursor is in");
    }
}

#[test]
fn a_cancelled_find_leaves_the_last_one_for_semicolon() {
    let (cursor, messages) = run("axbxcx\n", "fxf<Esc>;");
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(cursor, 3);
}

/// K11a: `:` runs what's in the registry, with the rest of the line, and remembers it.
#[test]
fn colon_reads_a_line_and_runs_a_command() {
    let (cursor, messages) = run("abc\n", ":vim-forward-char<CR>");
    assert_eq!(cursor, 1);
    assert!(messages.is_empty(), "{messages:?}");

    let (cursor, messages) = run("abc\n", ":nonsense<CR>l");
    assert_eq!(messages, ["no command named nonsense"]);
    assert_eq!(cursor, 1, "typed keys stay after a failure");

    let (cursor, messages) = run("abc\n", ":<BS>l");
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(cursor, 1, "<BS> left the line, so l moved");
}

/// K11a: `/` and `?` read a line with a history of their own. V2 makes them search.
#[test]
fn slash_and_question_share_a_history_and_cancel_quietly() {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let b = ctx.create_text("t", "abc\n");
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    ctx.type_keys("/abc");
    assert_eq!(ctx.prompt_text(), Some("abc"));
    ctx.type_keys("<CR>?<Up>");
    assert_eq!(ctx.prompt_text(), Some("abc"));
    ctx.type_keys("<Esc>:<Up>");
    assert_eq!(ctx.prompt_text(), Some(""), ": has its own");
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state_of(b), die_core::State::Normal);
}
