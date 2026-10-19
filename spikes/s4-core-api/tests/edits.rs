//! Criteria 5, 6, 7: edit origins (C14), undo across silent edits and undo groups, `edits_since` (C15).
mod support;
use s4core::*;
use support::*;

fn has_msg(ctx: &Ctx, needle: &str) -> bool {
    ctx.messages.iter().any(|m| m.contains(needle))
}

// ---------------------------------------------------------------- criterion 5: C14

#[test]
fn c14_user_dd_fails_in_read_only_but_silent_extension_append_succeeds() {
    let (mut ctx, b, w) = vim_ctx("you: hi\nbot: hello\n> ", 40, 10);
    let ro = ctx
        .text_mut(b)
        .unwrap()
        .decorate(0..19, Deco::ReadOnly)
        .unwrap();
    ctx.type_keys("jdd");
    assert!(has_msg(&ctx, "ReadOnly"), "{:?}", ctx.messages);
    assert_eq!(
        text(&ctx, b),
        "you: hi\nbot: hello\n> ",
        "die-vim's dd (Origin::User) changed nothing"
    );
    assert_eq!(lc(&ctx, b, w), (1, 0));

    // orchid streaming: silent Extension edits succeed at the end of and inside the transcript.
    let t = ctx.text_mut(b).unwrap();
    t.tx(Origin::Extension)
        .silent()
        .insert(19, "bot: more\n")
        .commit()
        .unwrap();
    t.tx(Origin::Extension)
        .silent()
        .replace(13..18, "HELLO")
        .commit()
        .unwrap();
    assert_eq!(text(&ctx, b), "you: hi\nbot: HELLO\nbot: more\n> ");
    // A User edit outside the read-only range works.
    let t = ctx.text_mut(b).unwrap();
    let end = t.len();
    t.tx(Origin::User).insert(end, "typed").commit().unwrap();
    assert!(t.decoration(ro).is_some());
    // Whole-buffer read-only also blocks only User edits.
    t.settings_mut().read_only = true;
    assert_eq!(
        t.tx(Origin::User).insert(0, "x").commit(),
        Err(Error::ReadOnly { at: 0 })
    );
    assert!(t.tx(Origin::Extension).insert(0, "x").commit().is_ok());
}

#[test]
fn c14_transaction_is_atomic() {
    let (mut ctx, b, _) = vim_ctx("aaa bbb ccc", 40, 10);
    let t = ctx.text_mut(b).unwrap();
    t.decorate(4..7, Deco::ReadOnly).unwrap();
    let rev = t.rev();
    let r = t
        .tx(Origin::User)
        .replace(0..1, "A")
        .replace(5..6, "B")
        .commit();
    assert_eq!(r, Err(Error::ReadOnly { at: 5 }));
    assert_eq!(
        (t.rope().to_string().as_str(), t.rev()),
        ("aaa bbb ccc", rev),
        "nothing applied"
    );
}

// ---------------------------------------------------------------- criterion 6: undo

fn chat() -> (Ctx, BufferId, WindowId) {
    let (mut ctx, b, w) = vim_ctx("you: hi\n> ", 40, 10);
    let t = ctx.text_mut(b).unwrap();
    t.decorate(0..8, Deco::ReadOnly).unwrap();
    t.set_cursor(w, 10);
    (ctx, b, w)
}

fn silent_reply(ctx: &mut Ctx, b: BufferId) {
    // The extension appends a reply above the input area: at the end of the transcript.
    ctx.text_mut(b)
        .unwrap()
        .tx(Origin::Extension)
        .silent()
        .insert(8, "bot: hey\n")
        .commit()
        .unwrap();
}

#[test]
fn undo_after_silent_edit_above_restores_user_text_exactly() {
    let (mut ctx, b, w) = chat();
    ctx.type_keys("ihello<Esc>");
    assert_eq!(text(&ctx, b), "you: hi\n> hello");
    silent_reply(&mut ctx, b);
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\n> hello");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\n> ", "{:?}", ctx.messages);
    assert_eq!(
        cursor(&ctx, b, w),
        19,
        "cursor back where `i` was pressed, shifted by the reply"
    );
    ctx.type_keys("<C-r>");
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\n> hello");
}

#[test]
fn silent_edit_mid_session_keeps_one_undo_step() {
    let (mut ctx, b, _) = chat();
    ctx.type_keys("ihel");
    silent_reply(&mut ctx, b);
    ctx.type_keys("lo<Esc>");
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\n> hello");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\n> ", "{:?}", ctx.messages);
}

#[test]
fn redo_after_silent_edit() {
    let (mut ctx, b, _) = chat();
    ctx.type_keys("ihello<Esc>u");
    silent_reply(&mut ctx, b);
    ctx.type_keys("<C-r>");
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\n> hello");
    silent_reply(&mut ctx, b);
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "you: hi\nbot: hey\nbot: hey\n> ");
}

#[test]
fn undo_refuses_when_an_extension_rewrote_the_users_text() {
    let (mut ctx, b, _) = chat();
    ctx.type_keys("ihello<Esc>");
    ctx.text_mut(b)
        .unwrap()
        .tx(Origin::Extension)
        .silent()
        .replace(12..14, "LL")
        .commit()
        .unwrap();
    ctx.type_keys("u");
    assert!(has_msg(&ctx, "UndoConflict"), "{:?}", ctx.messages);
    assert_eq!(text(&ctx, b), "you: hi\n> heLLo", "nothing half-undone");
}

#[test]
fn insert_session_is_one_step_and_restores_cursor() {
    let (mut ctx, b, w) = vim_ctx("abc\n", 40, 10);
    ctx.type_keys("liXY<CR>Z<Esc>");
    assert_eq!(text(&ctx, b), "aXY\nZbc\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "abc\n");
    assert_eq!(cursor(&ctx, b, w), 1);
    ctx.type_keys("u");
    assert!(has_msg(&ctx, "NothingToUndo"));
}

#[test]
fn count_dd_is_one_step_and_restores_cursor() {
    let (mut ctx, b, w) = vim_ctx("l0\nl1\nl2\nl3\nl4\n", 40, 10);
    ctx.type_keys("jl3dd");
    assert_eq!(text(&ctx, b), "l0\nl4\n");
    assert_eq!(ctx.register('"').unwrap().text, "l1\nl2\nl3\n");
    ctx.type_keys("gg"); // unbound: harmless
    ctx.type_keys("u");
    assert_eq!(
        text(&ctx, b),
        "l0\nl1\nl2\nl3\nl4\n",
        "three transactions, one step"
    );
    assert_eq!(
        lc(&ctx, b, w),
        (1, 1),
        "cursor restored to where dd started"
    );
}

// ---------------------------------------------------------------- criterion 7: C15

#[derive(Default)]
struct Chat {
    seen: Vec<(String, Point)>,
}
impl Mode for Chat {}
struct ChatExt;
impl Extension for ChatExt {
    fn name(&self) -> &'static str {
        "chat"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Chat>("chat", Chat::default);
    }
}

#[test]
fn c15_checked_out_mode_sees_own_edits_and_old_rev_is_none() {
    let mut ctx = Ctx::new(vec![Box::new(ChatExt)]).unwrap();
    let b = ctx.create_text("chat", "line\n");
    ctx.enable_mode::<Chat>(b).unwrap();
    let rev0 = ctx.text(b).unwrap().rev();
    ctx.with_mode(b, |m: &mut Chat, ctx| {
        let t = ctx.text_mut(b).unwrap();
        t.tx(Origin::Extension)
            .silent()
            .insert(5, "streamed")
            .commit()
            .unwrap();
        m.seen = t
            .edits_since(rev0)
            .unwrap()
            .map(|c| (c.text.clone(), c.new_end_point))
            .collect();
    })
    .unwrap();
    let seen = ctx.with_mode(b, |m: &mut Chat, _| m.seen.clone()).unwrap();
    assert_eq!(seen, [("streamed".to_string(), Point { line: 1, col: 8 })]);

    let t = ctx.text_mut(b).unwrap();
    for _ in 0..LOG_CAP + 5 {
        let end = t.len();
        t.tx(Origin::Extension)
            .silent()
            .insert(end, ".")
            .commit()
            .unwrap();
    }
    assert!(
        t.edits_since(rev0).is_none(),
        "too old for the bounded log: resync fully"
    );
    let now = t.rev();
    assert_eq!(t.edits_since(now).unwrap().count(), 0);
    assert_eq!(t.edits_since(now - 3).unwrap().count(), 3);
}
