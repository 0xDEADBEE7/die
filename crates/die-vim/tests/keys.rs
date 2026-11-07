//! Keys the Evil oracle can't record: Escape while `f` waits for its character is a
//! quit, which ends batch Emacs's keyboard macro. And `j` on the last line with a final
//! newline, which is rule 1's, with no oracle case of its own.

use die_test_cap as _;

use die_core::{
    Args, Button, Ctx, Extension, Input, Mods, PointerEvent, PointerKind, Registry, Result, State,
};
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

/// The oracle has one buffer. `C-p` and `C-n` go on to the others, this buffer's words first,
/// and past a buffer that isn't to be searched.
#[test]
fn completion_goes_on_to_other_buffers() {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    ctx.create_text("*Messages*", "fomessage\n");
    ctx.create_text("other", "fother fob\n");
    let b = ctx.create_text("t", "fob\n\n");
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    let text = |ctx: &Ctx| ctx.text(b).unwrap().rope().to_string();

    ctx.type_keys("jifo<C-p>");
    assert_eq!(text(&ctx), "fob\nfob\n");
    ctx.type_keys("<C-p>");
    assert_eq!(
        text(&ctx),
        "fob\nfother\n",
        "after this buffer's, the others'"
    );
    ctx.type_keys("<C-p><Esc>");
    assert_eq!(text(&ctx), "fob\nfo\n", "not *Messages*, and not fob again");
}

/// An extension with a command of its own in normal state, and a key for it.
struct Other;

impl Extension for Other {
    fn name(&self) -> &'static str {
        "other"
    }
    fn register(&mut self, reg: &mut Registry) {
        fn nothing(_: &mut Ctx, _: &Args) -> Result {
            Ok(())
        }
        reg.command("other-nothing", "Does nothing.", nothing);
        reg.global_keymap()
            .bind(State::Normal, "<F5>", "other-nothing");
    }
}

fn click(ctx: &mut Ctx, row: usize, col: usize) {
    for kind in [
        PointerKind::Press(Button::Left),
        PointerKind::Release(Button::Left),
    ] {
        ctx.handle(Input::Pointer(PointerEvent {
            kind,
            row,
            col,
            clicks: 1,
            mods: Mods::default(),
        }));
        ctx.run_ready();
    }
}

/// `C-o` waits for one command, anyone's. A click is one too, as it is in Emacs.
#[test]
fn c_o_ends_with_whatever_comes_next() {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default()), Box::new(Other)]).expect("it registers");
    let b = ctx.create_text("t", "abc\n");
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    ctx.scene();
    ctx.type_keys("i<C-o><F5>");
    assert_eq!(ctx.state(), State::Insert, "after another extension's command");
    ctx.type_keys("Z");
    assert_eq!(ctx.text(b).unwrap().rope().to_string(), "Zabc\n");
    ctx.type_keys("<C-o>");
    assert_eq!(ctx.state(), State::Normal);
    click(&mut ctx, 0, 2);
    assert_eq!(ctx.state(), State::Insert, "after a click");
    ctx.type_keys("Y<Esc>");
    assert_eq!(ctx.text(b).unwrap().rope().to_string(), "ZaYbc\n");
    ctx.type_keys("j");
    assert_eq!(ctx.state(), State::Normal, "and nothing is left waiting");
}

/// A `C-o` whose command takes the window to another buffer still goes back in its own.
#[test]
fn c_o_goes_back_in_its_own_buffer() {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let other = ctx.create_text("other", "x\n");
    let b = ctx.create_text("t", "abc\n");
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    ctx.type_keys("i<C-o>:b other<CR>");
    assert_eq!(ctx.window_buffer(w), Some(other));
    assert_eq!(ctx.state_of(other), State::Normal);
    assert_eq!(ctx.state_of(b), State::Insert);
}

/// A click runs no command of die-vim's, but `C-p` after one isn't a press after a press.
#[test]
fn completion_after_a_click_starts_anew() {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let b = ctx.create_text("t", "foo fob\n");
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    ctx.scene();
    ctx.type_keys("ofo<C-p>");
    click(&mut ctx, 0, 1);
    ctx.type_keys("<C-p>");
    assert_eq!(ctx.messages, Vec::<String>::new());
    assert_eq!(
        ctx.text(b).unwrap().rope().to_string(),
        "foboo fob\nfob\n"
    );
}

/// `k` is one byte and a Kelvin sign three, and the one completes to the other.
#[test]
fn completion_folds_case_across_lengths() {
    let (_, messages) = run("\u{212a}x\n\nkb\n", "jik<C-p>");
    assert_eq!(messages, Vec::<String>::new());
}

/// The oracle has one buffer. `C-x C-p` goes on to the others too, once this one has no more.
#[test]
fn line_completion_goes_on_to_other_buffers() {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    ctx.create_text("other", "  far away\n");
    let b = ctx.create_text("t", "fob\n\n");
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    let text = |ctx: &Ctx| ctx.text(b).unwrap().rope().to_string();
    ctx.type_keys("jif<C-x><C-p>");
    assert_eq!(text(&ctx), "fob\nfob\n");
    ctx.type_keys("<C-x><C-p>");
    assert_eq!(text(&ctx), "fob\nfar away\n");
    ctx.type_keys("<C-x><C-p>");
    assert_eq!(text(&ctx), "fob\nf\n", "none left, so what was typed");
}
