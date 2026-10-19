//! Insert sessions where the Evil oracle can't reach: two buffers, read-only
//! text, a buffer's own tab width, and the changes an edit logs.

use die_core::{BufferId, Ctx, State, WindowId};
use die_vim::Vim;

fn editor() -> Ctx {
    Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
}

fn open(ctx: &mut Ctx, text: &str) -> (BufferId, WindowId) {
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 80, 24).expect("a window");
    (b, w)
}

fn text(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).expect("a text buffer").rope().to_string()
}

#[test]
fn each_buffer_has_its_own_insert_session() {
    let mut ctx = editor();
    let (a, wa) = open(&mut ctx, "aaa\n");
    let (b, wb) = open(&mut ctx, "bbb\n");
    ctx.focus(wb);
    ctx.type_keys("ibar");
    ctx.focus(wa);
    ctx.type_keys("3ifoo");
    ctx.focus(wb);
    ctx.type_keys("<Esc>");
    assert_eq!(
        text(&ctx, b),
        "barbbb\n",
        "B's Escape replays B's session only"
    );
    assert_eq!(ctx.state(), State::Normal);

    ctx.focus(wa);
    assert_eq!(ctx.state(), State::Insert);
    ctx.type_keys("<Esc>");
    assert_eq!(text(&ctx, a), "foofoofooaaa\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, a), "aaa\n", "A's session is one undo step");
    ctx.focus(wb);
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "bbb\n", "and so is B's");
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn a_failed_o_leaves_no_undo_group_open() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "a\nb\n");
    ctx.text_mut(b).unwrap().settings_mut().read_only = true;
    ctx.type_keys("o");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal);

    ctx.text_mut(b).unwrap().settings_mut().read_only = false;
    ctx.type_keys("ix<Esc>iy<Esc>u");
    assert_eq!(text(&ctx, b), "xa\nb\n", "u undoes only the last session");
}

#[test]
fn escape_ends_the_session_when_dropping_indentation_fails() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "  a\n");
    ctx.type_keys("o");
    assert_eq!(text(&ctx, b), "  a\n  \n");
    ctx.text_mut(b).unwrap().settings_mut().read_only = true;
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal);

    ctx.text_mut(b).unwrap().settings_mut().read_only = false;
    ctx.type_keys("ix<Esc>u");
    assert_eq!(text(&ctx, b), "  a\n  \n", "u undoes only the x");
}

#[test]
fn tab_stops_follow_the_buffers_tab_width() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "ab\n");
    ctx.text_mut(b).unwrap().settings_mut().tab_width = 4;
    ctx.type_keys("a<Tab><Esc>");
    assert_eq!(text(&ctx, b), "a   b\n");
}

#[test]
fn opening_an_unindented_line_makes_one_change() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "a\n");
    let rev = ctx.text(b).unwrap().rev();
    ctx.type_keys("o");
    let t = ctx.text(b).unwrap();
    let changes: Vec<_> = t.edits_since(rev).expect("in the log").collect();
    assert_eq!(changes.len(), 1, "{changes:?}");
}
