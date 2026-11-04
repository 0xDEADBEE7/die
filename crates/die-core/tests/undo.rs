//! K2: undo in production. Undo and redo report the range they changed, undo restores the
//! selection with its shape, `undo_boundary` ends a step, and history is capped per buffer
//! (2,000 steps or 16 MiB of text by default, whichever comes first).

use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;

fn setup(text: &str) -> (Ctx, BufferId, WindowId) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 40, 10).unwrap();
    (ctx, b, w)
}

/// Run `f` as one top-level unit: by default, its edits are one undo step (A11).
fn step<R: 'static>(ctx: &mut Ctx, b: BufferId, f: impl FnOnce(&mut TextBuf) -> R + 'static) -> R {
    let out = Rc::new(RefCell::new(None));
    let o = out.clone();
    ctx.defer(move |ctx| {
        *o.borrow_mut() = Some(f(ctx.text_mut(b)?));
        Ok(())
    });
    ctx.run_until_idle();
    out.borrow_mut().take().expect("the step ran")
}

fn insert(ctx: &mut Ctx, b: BufferId, at: usize, s: &'static str) {
    step(ctx, b, move |t| {
        t.tx(Origin::User).insert(at, s).commit().unwrap()
    });
}

fn text(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).unwrap().rope().to_string()
}

fn undo(ctx: &mut Ctx, b: BufferId) -> Result<std::ops::Range<usize>> {
    step(ctx, b, |t| t.undo())
}

fn redo(ctx: &mut Ctx, b: BufferId) -> Result<std::ops::Range<usize>> {
    step(ctx, b, |t| t.redo())
}

#[test]
fn undo_and_redo_report_the_range_they_changed() {
    let (mut ctx, b, _) = setup("hello world");
    insert(&mut ctx, b, 6, "big ");
    assert_eq!(undo(&mut ctx, b), Ok(6..6));
    assert_eq!(text(&ctx, b), "hello world");
    assert_eq!(redo(&mut ctx, b), Ok(6..10));
    assert_eq!(text(&ctx, b), "hello big world");
    step(&mut ctx, b, |t| {
        t.tx(Origin::User).delete(0..6).commit().unwrap()
    });
    assert_eq!(undo(&mut ctx, b), Ok(0..6));
    assert_eq!(redo(&mut ctx, b), Ok(0..0));
}

#[test]
fn a_step_with_several_changes_reports_their_hull() {
    let (mut ctx, b, w) = setup("abcdefghij");
    step(&mut ctx, b, |t| {
        t.tx(Origin::User)
            .replace(0..1, "XX")
            .replace(8..9, "")
            .commit()
            .unwrap();
        t.tx(Origin::User).insert(0, ">").commit().unwrap();
    });
    assert_eq!(text(&ctx, b), ">XXbcdefghj");
    assert_eq!(undo(&mut ctx, b), Ok(0..9));
    assert_eq!(text(&ctx, b), "abcdefghij");
    // The deleted `i` was at 10 in the redone text, so the hull ends there.
    assert_eq!(redo(&mut ctx, b), Ok(0..10));
    assert_eq!(text(&ctx, b), ">XXbcdefghj");
    assert_eq!(
        ctx.text(b).unwrap().selection(w),
        Some(Selection::cursor(0)),
        "redo's cursor is at the hull's start"
    );
}

#[test]
fn undo_restores_the_selection_with_its_shape() {
    let (mut ctx, b, w) = setup("one\ntwo\nthree\n");
    let sel = Selection {
        anchor: 0,
        head: 5,
        shape: Shape::Line,
        goal: Some(1),
    };
    ctx.text_mut(b).unwrap().set_selection(w, sel);
    step(&mut ctx, b, |t| {
        t.tx(Origin::User).delete(0..8).commit().unwrap()
    });
    ctx.text_mut(b).unwrap().set_cursor(w, 2);
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "one\ntwo\nthree\n");
    assert_eq!(ctx.text(b).unwrap().selection(w), Some(sel));
}

#[test]
fn undo_moves_the_selection_through_silent_edits() {
    let (mut ctx, b, w) = setup("abc\n");
    let sel = Selection {
        anchor: 1,
        head: 3,
        shape: Shape::Block,
        goal: None,
    };
    ctx.text_mut(b).unwrap().set_selection(w, sel);
    insert(&mut ctx, b, 3, "XYZ");
    step(&mut ctx, b, |t| {
        t.tx(Origin::Extension)
            .silent()
            .insert(0, ">> ")
            .commit()
            .unwrap()
    });
    assert_eq!(undo(&mut ctx, b), Ok(6..6));
    assert_eq!(text(&ctx, b), ">> abc\n");
    let want = Selection {
        anchor: 4,
        head: 6,
        ..sel
    };
    assert_eq!(ctx.text(b).unwrap().selection(w), Some(want));
}

#[test]
fn redo_puts_a_cursor_at_the_start_of_the_change() {
    let (mut ctx, b, w) = setup("abc");
    insert(&mut ctx, b, 1, "xy");
    undo(&mut ctx, b).unwrap();
    ctx.text_mut(b).unwrap().set_selection(
        w,
        Selection {
            anchor: 0,
            head: 3,
            shape: Shape::Line,
            goal: None,
        },
    );
    redo(&mut ctx, b).unwrap();
    assert_eq!(
        ctx.text(b).unwrap().selection(w),
        Some(Selection::cursor(1))
    );
}

#[test]
fn undo_boundary_splits_one_command() {
    let (mut ctx, b, _) = setup("-");
    step(&mut ctx, b, |t| {
        t.tx(Origin::User).insert(0, "a").commit().unwrap();
        t.undo_boundary();
        t.tx(Origin::User).insert(1, "b").commit().unwrap();
    });
    assert_eq!(text(&ctx, b), "ab-");
    assert_eq!(undo(&mut ctx, b), Ok(1..1));
    assert_eq!(text(&ctx, b), "a-");
    assert_eq!(undo(&mut ctx, b), Ok(0..0));
    assert_eq!(text(&ctx, b), "-");
}

#[test]
fn undo_boundary_splits_an_open_group() {
    let (mut ctx, b, _) = setup("");
    step(&mut ctx, b, |t| t.begin_undo_group());
    insert(&mut ctx, b, 0, "x");
    insert(&mut ctx, b, 1, "y");
    step(&mut ctx, b, |t| t.undo_boundary());
    insert(&mut ctx, b, 2, "z");
    insert(&mut ctx, b, 3, "w");
    step(&mut ctx, b, |t| t.end_undo_group());
    insert(&mut ctx, b, 4, "!");
    assert_eq!(text(&ctx, b), "xyzw!");
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "xyzw");
    undo(&mut ctx, b).unwrap();
    assert_eq!(
        text(&ctx, b),
        "xy",
        "the part after the boundary is one step"
    );
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "", "the part before it is one step");
    // Undoing the first part didn't reopen it: a new edit is its own step.
    insert(&mut ctx, b, 0, "q");
    insert(&mut ctx, b, 1, "r");
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "q");
}

#[test]
fn an_edit_before_the_group_in_the_same_command_joins_it() {
    // die-vim's `o`: open a line, then start the insert session, in one command (A11).
    let (mut ctx, b, _) = setup("abc");
    step(&mut ctx, b, |t| {
        t.tx(Origin::User).insert(3, "\n").commit().unwrap();
        t.begin_undo_group();
    });
    insert(&mut ctx, b, 4, "x");
    insert(&mut ctx, b, 5, "y");
    step(&mut ctx, b, |t| t.end_undo_group());
    assert_eq!(undo(&mut ctx, b), Ok(3..3));
    assert_eq!(text(&ctx, b), "abc");
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
}

#[test]
fn undo_keeps_the_newest_2000_steps() {
    let (mut ctx, b, _) = setup("");
    for _ in 0..2005 {
        insert(&mut ctx, b, 0, "x");
    }
    for i in 0..2000 {
        assert!(undo(&mut ctx, b).is_ok(), "undo {i}");
    }
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
    assert_eq!(text(&ctx, b), "xxxxx");
}

#[test]
fn undo_keeps_16_mib_of_text() {
    let (mut ctx, b, _) = setup("");
    // 1 MiB in 64-byte lines: S4's layout redoes a whole line on each edit (K4 fixes that).
    let mib = format!("{}\n", "a".repeat(63)).repeat(1 << 14);
    assert_eq!(mib.len(), 1 << 20);
    for _ in 0..18 {
        let s = mib.clone();
        step(&mut ctx, b, move |t| {
            t.tx(Origin::User).insert(0, s).commit().unwrap()
        });
    }
    for i in 0..16 {
        assert!(undo(&mut ctx, b).is_ok(), "undo {i}");
    }
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
    assert_eq!(ctx.text(b).unwrap().len(), 2 << 20);
}

#[test]
fn the_newest_step_is_kept_even_over_the_byte_cap() {
    let (mut ctx, b, _) = setup("");
    ctx.text_mut(b).unwrap().settings_mut().undo_bytes = 10;
    insert(&mut ctx, b, 0, "a much longer text than ten bytes");
    insert(&mut ctx, b, 0, "and another");
    assert!(undo(&mut ctx, b).is_ok());
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
}

#[test]
fn limits_are_buffer_settings() {
    let (mut ctx, b, _) = setup("");
    let s = ctx.text(b).unwrap().settings().clone();
    assert_eq!((s.undo_steps, s.undo_bytes), (2000, 16 << 20));
    ctx.text_mut(b).unwrap().settings_mut().undo_steps = 2;
    for c in ["a", "b", "c"] {
        insert(&mut ctx, b, 0, c);
    }
    assert!(undo(&mut ctx, b).is_ok());
    assert!(undo(&mut ctx, b).is_ok());
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
    assert_eq!(text(&ctx, b), "a");
    // No steps: nothing is undoable.
    ctx.text_mut(b).unwrap().settings_mut().undo_steps = 0;
    insert(&mut ctx, b, 0, "d");
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
}

#[test]
fn redo_puts_a_cursor_at_the_start_of_a_multi_change_step() {
    // `>j`: one transaction indents two lines. Its changes are recorded last one first.
    let (mut ctx, b, w) = setup("a\nb\n");
    step(&mut ctx, b, |t| {
        t.tx(Origin::User)
            .insert(0, "\t")
            .insert(2, "\t")
            .commit()
            .unwrap()
    });
    assert_eq!(undo(&mut ctx, b), Ok(0..2));
    assert_eq!(redo(&mut ctx, b), Ok(0..4));
    assert_eq!(text(&ctx, b), "\ta\n\tb\n");
    assert_eq!(
        ctx.text(b).unwrap().selection(w),
        Some(Selection::cursor(0))
    );
}

#[test]
fn lowering_the_caps_applies_at_the_next_undo() {
    let (mut ctx, b, _) = setup("");
    for c in ["a", "b", "c"] {
        insert(&mut ctx, b, 0, c);
    }
    ctx.text_mut(b).unwrap().settings_mut().undo_steps = 1;
    assert_eq!(undo(&mut ctx, b), Ok(0..0));
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
    assert_eq!(text(&ctx, b), "ba");
    // 0 turns undo off at once.
    insert(&mut ctx, b, 0, "d");
    insert(&mut ctx, b, 0, "e");
    ctx.text_mut(b).unwrap().settings_mut().undo_steps = 0;
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo));
}

#[test]
fn undo_is_exact_around_streamed_silent_text() {
    // An extension streams output in chunks, each right after the last, before, after
    // and touching the user's text.
    for at in [0, 3, 6, 7] {
        let (mut ctx, b, _) = setup("abc\nd");
        insert(&mut ctx, b, 3, "XYZ");
        let mut p = at;
        for chunk in ["1", "22", "中"] {
            step(&mut ctx, b, move |t| {
                t.tx(Origin::Extension)
                    .silent()
                    .insert(p, chunk)
                    .commit()
                    .unwrap()
            });
            p += chunk.len();
        }
        let streamed = "122中";
        let mut want = "abc\nd".to_string();
        // Streaming at the end of the user's text lands after it, and stays after undo.
        want.insert_str(if at <= 3 { at } else { at - 3 }, streamed);
        let undone = if at <= 3 { 3 + streamed.len() } else { 3 };
        assert_eq!(undo(&mut ctx, b), Ok(undone..undone), "streamed at {at}");
        assert_eq!(text(&ctx, b), want, "streamed at {at}");
        redo(&mut ctx, b).unwrap();
        undo(&mut ctx, b).unwrap();
        assert_eq!(text(&ctx, b), want, "streamed at {at}, after a redo");
    }
}

/// The selection comes from just before the step's first edit, not from
/// `begin_undo_group`: `i<Right>x<Esc>u` in your Evil returns to where x was typed.
#[test]
fn undo_restores_the_selection_from_before_the_groups_first_edit() {
    let (mut ctx, b, w) = setup("abc\n");
    step(&mut ctx, b, |t| t.begin_undo_group()); // `i`, cursor at 0
    step(&mut ctx, b, move |t| t.set_cursor(w, 1)); // <Right>
    insert(&mut ctx, b, 1, "x"); // x
    step(&mut ctx, b, |t| t.end_undo_group()); // <Esc>
    undo(&mut ctx, b).unwrap();
    assert_eq!(ctx.text(b).unwrap().cursor(w), Some(1));
}

/// `o`: an edit before the group in the same command joins it, and the selection is the one
/// from before that edit.
#[test]
fn an_edit_that_joins_a_group_gives_the_step_its_selection() {
    let (mut ctx, b, w) = setup("abc");
    ctx.text_mut(b).unwrap().set_cursor(w, 1);
    step(&mut ctx, b, move |t| {
        t.tx(Origin::User).insert(3, "\n").commit().unwrap();
        t.set_cursor(w, 4);
        t.begin_undo_group();
    });
    insert(&mut ctx, b, 4, "x");
    step(&mut ctx, b, |t| t.end_undo_group());
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "abc");
    assert_eq!(ctx.text(b).unwrap().cursor(w), Some(1));
}

/// A silent edit that lands before the group's first change moves the selection with it.
#[test]
fn a_silent_edit_before_the_groups_first_change_moves_its_selection() {
    let (mut ctx, b, w) = setup("abc\n");
    step(&mut ctx, b, |t| t.begin_undo_group());
    step(&mut ctx, b, move |t| t.set_cursor(w, 1));
    step(&mut ctx, b, |t| {
        t.tx(Origin::Extension)
            .silent()
            .insert(0, ">> ")
            .commit()
            .unwrap()
    });
    assert_eq!(ctx.text(b).unwrap().cursor(w), Some(4));
    insert(&mut ctx, b, 4, "x");
    step(&mut ctx, b, |t| t.end_undo_group());
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), ">> abc\n");
    assert_eq!(ctx.text(b).unwrap().cursor(w), Some(4));
}

/// Code between units has no command of its own to be attributed to: it used to get the last
/// command's number, and one `u` took back both.
#[test]
fn an_edit_made_outside_any_command_is_its_own_undo_step() {
    let (mut ctx, b, _) = setup("");
    insert(&mut ctx, b, 0, "a"); // a command
    let host = |ctx: &mut Ctx, at, s: &str| {
        let t = ctx.text_mut(b).unwrap();
        t.tx(Origin::User).insert(at, s).commit().unwrap();
    };
    host(&mut ctx, 1, "b");
    assert_eq!(text(&ctx, b), "ab");
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "a", "u took back the command's edit too");
    // Host edits between the same two units are one step.
    host(&mut ctx, 1, "b");
    host(&mut ctx, 2, "c");
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "a");
}

/// A step put back by redo takes no later edits, even from host code that has run no unit.
#[test]
fn a_step_put_back_by_redo_takes_no_later_edits() {
    let (mut ctx, b, _) = setup("");
    let t = ctx.text_mut(b).unwrap();
    t.tx(Origin::User).insert(0, "a").commit().unwrap();
    t.undo().unwrap();
    t.redo().unwrap();
    t.tx(Origin::User).insert(1, "b").commit().unwrap();
    t.undo().unwrap();
    assert_eq!(t.rope().to_string(), "a");
}

/// An explicit group spans commands until it ends. Undoing inside it (Vim's `<C-o>u`, or any
/// extension calling `undo`) leaves it open: the next edit starts a new step inside it.
#[test]
fn an_undo_inside_an_open_group_leaves_the_group_open() {
    let (mut ctx, b, _) = setup("");
    step(&mut ctx, b, |t| t.begin_undo_group());
    insert(&mut ctx, b, 0, "x");
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "");
    insert(&mut ctx, b, 0, "y");
    insert(&mut ctx, b, 1, "z");
    step(&mut ctx, b, |t| t.end_undo_group());
    undo(&mut ctx, b).unwrap();
    assert_eq!(
        text(&ctx, b),
        "",
        "the rest of the group (yz) should be one step"
    );
}

#[test]
fn a_redo_inside_an_open_group_leaves_the_group_open() {
    let (mut ctx, b, _) = setup("");
    insert(&mut ctx, b, 0, "x");
    undo(&mut ctx, b).unwrap();
    step(&mut ctx, b, |t| t.begin_undo_group());
    redo(&mut ctx, b).unwrap();
    insert(&mut ctx, b, 1, "y");
    insert(&mut ctx, b, 2, "z");
    step(&mut ctx, b, |t| t.end_undo_group());
    insert(&mut ctx, b, 3, "!");
    undo(&mut ctx, b).unwrap();
    assert_eq!(
        text(&ctx, b),
        "xyz",
        "the group ended, so ! is its own step"
    );
    undo(&mut ctx, b).unwrap();
    assert_eq!(
        text(&ctx, b),
        "x",
        "yz is one step, apart from the redone x"
    );
}

#[test]
fn a_group_left_open_takes_every_later_edit() {
    let (mut ctx, b, _) = setup("");
    step(&mut ctx, b, |t| t.begin_undo_group());
    for (i, c) in ["a", "b", "c"].into_iter().enumerate() {
        insert(&mut ctx, b, i, c);
    }
    assert_eq!(undo(&mut ctx, b), Ok(0..0));
    assert_eq!(text(&ctx, b), "", "one step, though no one ended the group");
}

/// Contract §9: if a silent edit changed text an undo step needs, undo refuses with an error
/// and drops the older history. When the edit lands inside a group still collecting, the
/// history goes at the next keystroke, and the undo that reaches the gap says why, once.
#[test]
fn history_dropped_by_a_conflict_while_typing_is_reported_as_a_conflict() {
    let (mut ctx, b, _) = setup("");
    insert(&mut ctx, b, 0, "older ");
    step(&mut ctx, b, |t| t.begin_undo_group());
    insert(&mut ctx, b, 6, "ab");
    step(&mut ctx, b, |t| {
        t.tx(Origin::Extension)
            .silent()
            .replace(7..8, "B")
            .commit()
            .unwrap()
    });
    insert(&mut ctx, b, 8, "c");
    step(&mut ctx, b, |t| t.end_undo_group());
    assert_eq!(text(&ctx, b), "older aBc");
    undo(&mut ctx, b).unwrap();
    assert_eq!(text(&ctx, b), "older aB");
    assert_eq!(
        undo(&mut ctx, b),
        Err(Error::UndoConflict),
        "the older steps were dropped by a conflict"
    );
    assert_eq!(undo(&mut ctx, b), Err(Error::NothingToUndo), "said once");
    assert_eq!(redo(&mut ctx, b), Ok(8..9), "redo was never affected");
}
