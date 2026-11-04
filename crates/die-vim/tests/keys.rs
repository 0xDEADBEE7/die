//! Keys the Evil oracle can't record: Escape while `f` waits for its character is a
//! quit, which ends batch Emacs's keyboard macro.

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

#[test]
fn a_cancelled_find_leaves_the_last_one_for_semicolon() {
    let (cursor, messages) = run("axbxcx\n", "fxf<Esc>;");
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(cursor, 3);
}
