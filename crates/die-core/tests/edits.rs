//! Criteria 5, 6, 7: edit origins (C14), undo across silent edits and undo groups, `edits_since` (C15).

use die_test_cap as _;

mod support;
use die_core::*;
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
    assert!(has_msg(&ctx, "read-only"), "{:?}", ctx.messages);
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

#[test]
fn c14_an_empty_read_only_range_blocks_nothing_so_undo_and_redo_round_trip() {
    // The gate's property test found it (prop.proptest-regressions, fd99bf…): an open group of
    // extension edits, a read-only range made empty, then undo and redo. The undo deleted
    // across the range's point, the redo's insertions carried the point along, and its
    // deletion then spanned the point and was refused.
    let (mut ctx, b, _) = vim_ctx("a", 40, 10);
    let t = ctx.text_mut(b).unwrap();
    t.begin_undo_group();
    t.tx(Origin::Extension).insert(0, "aa").commit().unwrap();
    t.tx(Origin::Extension).insert(2, "aaa").commit().unwrap();
    t.tx(Origin::Extension).replace(3..6, "").commit().unwrap();
    t.decorate(0..0, Deco::ReadOnly).unwrap();
    assert_eq!(t.rope().to_string(), "aaa");
    t.undo().unwrap();
    assert_eq!(t.rope().to_string(), "a");
    t.redo().expect("redo right after a successful undo");
    assert_eq!(t.rope().to_string(), "aaa");
    // A range an extension's edit emptied protects nothing either: the user deletes across it.
    let ro = t.decorate(1..2, Deco::ReadOnly).unwrap();
    t.tx(Origin::Extension).replace(1..2, "").commit().unwrap();
    assert!(t.decoration(ro).unwrap().range.is_empty());
    t.tx(Origin::User).replace(0..2, "").commit().unwrap();
    assert_eq!(t.rope().to_string(), "");
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
    assert!(
        has_msg(&ctx, "undo history was cut short"),
        "{:?}",
        ctx.messages
    );
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
    assert!(has_msg(&ctx, "nothing to undo"));
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

// ---------------------------------------------------------------- what the text review found

fn in_unit<R: 'static>(
    ctx: &mut Ctx,
    b: BufferId,
    f: impl FnOnce(&mut TextBuf) -> R + 'static,
) -> R {
    let out = std::rc::Rc::new(std::cell::RefCell::new(None));
    let o = out.clone();
    ctx.defer(move |ctx| {
        *o.borrow_mut() = Some(f(ctx.text_mut(b)?));
        Ok(())
    });
    ctx.run_until_idle();
    out.borrow_mut().take().expect("the unit ran")
}

/// `d0` at column 0, an empty `c` motion, `p` with an empty register.
#[test]
fn an_empty_change_makes_no_revision_no_log_entry_and_no_undo_step() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "abc");
    ctx.open_window(b, 40, 5).unwrap();
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::User).insert(1, "x").commit().unwrap()
    });
    let rev = ctx.text(b).unwrap().rev();
    for f in [
        (|t| t.tx(Origin::User).insert(2, "").commit()) as fn(&mut TextBuf) -> Result<u64>,
        |t| t.tx(Origin::User).delete(2..2).commit(),
        |t| t.tx(Origin::Extension).silent().replace(0..0, "").commit(),
    ] {
        assert_eq!(in_unit(&mut ctx, b, f), Ok(rev));
    }
    let t = ctx.text(b).unwrap();
    assert_eq!(t.rev(), rev);
    assert_eq!(t.edits_since(rev).unwrap().count(), 0);
    in_unit(&mut ctx, b, |t| t.undo()).unwrap();
    assert_eq!(
        text(&ctx, b),
        "abc",
        "u undoes the x, not the empty changes"
    );

    // Redo survives one, and a transaction with a real change drops only the empty one.
    in_unit(&mut ctx, b, |t| t.tx(Origin::User).delete(0..0).commit()).unwrap();
    in_unit(&mut ctx, b, |t| t.redo()).unwrap();
    assert_eq!(text(&ctx, b), "axbc");
    let rev = ctx.text(b).unwrap().rev();
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::User).insert(0, "").insert(4, "!").commit()
    })
    .unwrap();
    assert_eq!(ctx.text(b).unwrap().rev(), rev + 1);
    assert_eq!(text(&ctx, b), "axbc!");
}

#[test]
fn an_empty_change_is_still_checked_for_range_but_not_read_only() {
    let mut t = TextBuf::new("中x");
    assert_eq!(
        t.tx(Origin::User).insert(1, "").commit(),
        Err(Error::BadRange { start: 1, end: 1 })
    );
    t.settings_mut().read_only = true;
    assert_eq!(t.tx(Origin::User).insert(0, "").commit(), Ok(0));
    assert_eq!(
        t.tx(Origin::User).insert(0, "y").commit(),
        Err(Error::ReadOnly { at: 0 })
    );
}

/// `ra` on an `a` makes a step in Evil and Vim, so `u` after it doesn't undo the change before.
#[test]
fn text_replaced_by_itself_is_still_a_change() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "abc");
    ctx.open_window(b, 40, 5).unwrap();
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::User).delete(2..3).commit().unwrap()
    });
    in_unit(&mut ctx, b, |t| t.undo()).unwrap();
    assert_eq!(text(&ctx, b), "abc");
    let rev = ctx.text(b).unwrap().rev();
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::User).replace(0..1, "a").commit().unwrap()
    });
    assert_eq!(ctx.text(b).unwrap().rev(), rev + 1);
    assert_eq!(
        in_unit(&mut ctx, b, |t| t.redo()),
        Err(Error::NothingToUndo),
        "it cleared redo"
    );
    assert_eq!(in_unit(&mut ctx, b, |t| t.undo()), Ok(0..1));
    assert_eq!(text(&ctx, b), "abc", "u undid only that");
}

#[test]
fn a_marker_can_only_sit_on_a_character_boundary() {
    let mut t = TextBuf::new("中x");
    assert_eq!(
        t.add_marker(1, Gravity::Left),
        Err(Error::BadRange { start: 1, end: 1 })
    );
    let m = t.add_marker(99, Gravity::Left).unwrap();
    assert_eq!(t.marker(m), Some(4), "still clamps to the end");
    assert_eq!(t.remove_marker(m), Some(4));
    assert_eq!(t.remove_marker(m), None, "a dead id says so");
    assert_eq!(t.marker(m), None);
}

#[test]
fn setting_stickiness_on_a_dead_decoration_says_so() {
    let mut t = TextBuf::new("abc");
    let d = t.decorate(0..1, Deco::Face("bold")).unwrap();
    assert_eq!(t.set_sticky(d, Gravity::Left, Gravity::Right), Some(()));
    assert_eq!(
        t.undecorate(d).map(|d| d.sticky),
        Some((Gravity::Left, Gravity::Right))
    );
    assert_eq!(t.set_sticky(d, Gravity::Left, Gravity::Right), None);
}

#[test]
fn point_clamps_like_line_of() {
    let t = TextBuf::new("ab\ncd");
    assert_eq!(t.point(4), Point { line: 1, col: 1 });
    assert_eq!(t.point(99), Point { line: 1, col: 2 });
    assert_eq!(t.line_of(99), 1);
}

#[test]
fn text_inserted_at_a_cursor_pushes_only_the_acting_windows_cursor() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "abc");
    let other = ctx.open_window(b, 40, 5).unwrap();
    let acting = ctx.open_window(b, 40, 5).unwrap();
    ctx.focus(acting);
    let t = ctx.text_mut(b).unwrap();
    t.set_cursor(other, 1);
    t.set_cursor(acting, 1);
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::Extension)
            .silent()
            .insert(1, "XYZ")
            .commit()
            .unwrap()
    });
    let t = ctx.text(b).unwrap();
    assert_eq!(
        t.selection(acting),
        Some(Selection::cursor(4)),
        "it stays collapsed"
    );
    assert_eq!(
        t.selection(other),
        Some(Selection::cursor(1)),
        "as Emacs's window points do"
    );

    // Text inserted before a cursor moves it, in every window.
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::User).insert(0, ">").commit().unwrap()
    });
    let t = ctx.text(b).unwrap();
    assert_eq!((t.cursor(acting), t.cursor(other)), (Some(5), Some(2)));
}

/// An insertion at either end of a selection follows the same rule: one gravity per window.
#[test]
fn a_selection_in_another_window_stays_before_inserted_text() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "abcdef");
    let other = ctx.open_window(b, 40, 5).unwrap();
    let acting = ctx.open_window(b, 40, 5).unwrap();
    ctx.focus(acting);
    let sel = Selection {
        anchor: 2,
        head: 4,
        shape: Shape::Char,
        goal: Some(7),
    };
    let t = ctx.text_mut(b).unwrap();
    t.set_selection(other, sel);
    t.set_selection(acting, sel);
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::Extension).insert(4, "XY").commit().unwrap()
    });
    let t = ctx.text(b).unwrap();
    assert_eq!(t.selection(other), Some(sel));
    assert_eq!(t.selection(acting), Some(Selection { head: 6, ..sel }));
    // At the anchor, the acting window's anchor follows too.
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::Extension).insert(2, "--").commit().unwrap()
    });
    let t = ctx.text(b).unwrap();
    assert_eq!(
        t.selection(acting),
        Some(Selection {
            anchor: 4,
            head: 8,
            ..sel
        })
    );
    assert_eq!(
        t.selection(other),
        Some(Selection {
            anchor: 2,
            head: 6,
            ..sel
        })
    );
}

/// `TextBuf::map_all` runs for undo's changes too; the acting window's selection is then set
/// by undo, and the other window's stays put before reinserted text.
#[test]
fn undo_moves_another_windows_cursor_like_any_other_edit() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "abcd");
    let other = ctx.open_window(b, 40, 5).unwrap();
    let acting = ctx.open_window(b, 40, 5).unwrap();
    ctx.focus(acting);
    in_unit(&mut ctx, b, |t| {
        t.tx(Origin::User).delete(1..3).commit().unwrap()
    });
    ctx.text_mut(b).unwrap().set_cursor(other, 1);
    in_unit(&mut ctx, b, |t| t.undo()).unwrap();
    assert_eq!(text(&ctx, b), "abcd");
    assert_eq!(ctx.text(b).unwrap().cursor(other), Some(1));
}

// ---------------------------------------------------------------- the acting window

thread_local! {
    /// Where the `go` command below moves the focus, and the window `open-insert` opened.
    static TARGET: std::cell::Cell<Option<WindowId>> = const { std::cell::Cell::new(None) };
}

fn insert_x(ctx: &mut Ctx, b: BufferId) -> Result {
    let t = ctx.text_mut(b)?;
    t.tx(Origin::Extension).silent().insert(1, "x").commit()?;
    Ok(())
}

struct Acting;
impl Extension for Acting {
    fn name(&self) -> &'static str {
        "acting"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("insert", "", |ctx, a| insert_x(ctx, a.buffer.unwrap()));
        reg.command("nested-insert", "", |ctx, a| {
            let inner = Args {
                buffer: a.buffer,
                ..Default::default()
            };
            ctx.run("insert", &inner)
        });
        reg.command("go", "", |ctx, _| {
            ctx.focus(TARGET.get().unwrap());
            Ok(())
        });
        reg.command("go-and-insert", "", |ctx, a| {
            ctx.focus(TARGET.get().unwrap());
            insert_x(ctx, a.buffer.unwrap())
        });
        reg.command("open-and-insert", "", |ctx, a| {
            let b = a.buffer.unwrap();
            let opened = ctx.open_window(b, 40, 5)?;
            TARGET.set(Some(opened));
            ctx.text_mut(b)?.set_cursor(opened, 1);
            insert_x(ctx, b)
        });
    }
}

/// Windows `first` and `second` on "abc", both cursors at 1, and the focus on `first`.
fn two_windows() -> (Ctx, BufferId, WindowId, WindowId) {
    let mut ctx = Ctx::new(vec![Box::new(Acting)]).unwrap();
    let b = ctx.create_text("t", "abc");
    let first = ctx.open_window(b, 40, 5).unwrap();
    let second = ctx.open_window(b, 40, 5).unwrap();
    ctx.focus(first);
    at_one(&mut ctx, b, first, second);
    (ctx, b, first, second)
}

fn at_one(ctx: &mut Ctx, b: BufferId, w1: WindowId, w2: WindowId) {
    let t = ctx.text_mut(b).unwrap();
    t.set_cursor(w1, 1);
    t.set_cursor(w2, 1);
}

fn cursors(ctx: &Ctx, b: BufferId, w1: WindowId, w2: WindowId) -> (usize, usize) {
    let t = ctx.text(b).unwrap();
    (t.cursor(w1).unwrap(), t.cursor(w2).unwrap())
}

/// Run a command from a callback, where the focused window would otherwise act.
fn run_for(ctx: &mut Ctx, name: &'static str, b: BufferId, window: Option<WindowId>) {
    let args = Args {
        window,
        buffer: Some(b),
        ..Default::default()
    };
    ctx.defer(move |c| c.run(name, &args));
    ctx.run_until_idle();
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

/// Code between commands acts for the focused window, not the last one opened.
#[test]
fn between_commands_the_focused_window_acts() {
    let (mut ctx, b, first, second) = two_windows();
    insert_x(&mut ctx, b).unwrap();
    assert_eq!(cursors(&ctx, b, first, second), (2, 1));
}

#[test]
fn a_command_acts_for_the_window_it_was_run_for() {
    let (mut ctx, b, first, second) = two_windows();
    run_for(&mut ctx, "insert", b, Some(second));
    assert_eq!(cursors(&ctx, b, first, second), (1, 2));
}

#[test]
fn a_command_run_with_no_window_acts_for_the_focused_one() {
    let (mut ctx, b, first, second) = two_windows();
    run_for(&mut ctx, "insert", b, None);
    assert_eq!(cursors(&ctx, b, first, second), (2, 1));
}

/// A command run with no window, inside one run for a window, acts for that window. When
/// they've returned, the focused window acts again.
#[test]
fn a_nested_command_with_no_window_acts_for_its_callers() {
    let (mut ctx, b, first, second) = two_windows();
    run_for(&mut ctx, "nested-insert", b, Some(second));
    assert_eq!(cursors(&ctx, b, first, second), (1, 2));
    at_one(&mut ctx, b, first, second);
    insert_x(&mut ctx, b).unwrap();
    assert_eq!(cursors(&ctx, b, first, second), (2, 1));
}

/// Moving the focus inside a command moves the acting window with it.
#[test]
fn a_command_goes_on_acting_for_the_window_it_focused() {
    let (mut ctx, b, first, second) = two_windows();
    TARGET.set(Some(second));
    run_for(&mut ctx, "go-and-insert", b, Some(first));
    assert_eq!(cursors(&ctx, b, first, second), (1, 2));
}

/// Once a command that moved the focus has returned, the new window acts.
#[test]
fn code_after_a_command_that_moved_the_focus_acts_for_the_new_window() {
    let (mut ctx, b, first, second) = two_windows();
    TARGET.set(Some(second));
    run_for(&mut ctx, "go", b, Some(first));
    insert_x(&mut ctx, b).unwrap();
    assert_eq!(cursors(&ctx, b, first, second), (1, 2));
}

/// A command that opens a window and writes to it acts for the window it opened.
#[test]
fn a_command_acts_for_a_window_it_opened() {
    let (mut ctx, b, first, _) = two_windows();
    run_for(&mut ctx, "open-and-insert", b, Some(first));
    let opened = TARGET.get().unwrap();
    assert_eq!(cursors(&ctx, b, first, opened), (1, 2));
}
