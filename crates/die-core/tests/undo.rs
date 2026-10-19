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
