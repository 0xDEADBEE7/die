//! K6 and K9's test plan. Workspace and windows here, and the host's calls in `host.rs`. The two
//! properties `mutate` reads are in `prop.rs`: `windows_tile_the_frame` (with a split, a close, an
//! only, a resize and a frame size in any order, every cell above the command line belongs to
//! exactly one window) and `unmodified_means_the_saved_text` (with edits, silent edits, undo, redo
//! and saves in any order, the flag is clear only when the text is what was saved).
//!
//! The rest, by example:
//! - Splits: which side the new window goes, half the cells, same cursor and scroll, focus stays.
//!   A window too small to divide. Close gives cells to the neighbour before it, and focus to the
//!   window used last. The only window can't close. `only` removes the others, and their views.
//! - Resize moves one border, stops at the minimum, and a frame made small and big again gives the
//!   sizes back. Balance follows how many windows each side holds.
//! - Focus by direction: picks by the cursor's row or column, and None at the frame's edge.
//! - Killing a buffer shown in two windows: both go on showing another one, a `*scratch*` if
//!   there's none, and the hook runs. A window's view of a buffer it left waits, moved through edits.
//! - `show` for each `ShowIn`. Names stay unique. A directory comes from the
//!   file, or from the buffer focused when it was made.
//! - Modified: undo back clears it, a save moves what counts, text replaced by itself sets it.
//! - Registers and the clipboard: a kind survives, an outside copy is news once, DIE's own write
//!   and the text it knows aren't news, and what was there at startup is.
//! - A surface hears size and focus. S4's windows outside the tree keep their sizes, and in a frame
//!   the host set they don't take the focus from the tree.

use die_test_cap as _;

use die_core::*;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

fn ctx() -> Ctx {
    Ctx::new(vec![]).unwrap()
}

fn chars(text: &str) -> Register {
    Register {
        kind: RegKind::Char,
        text: text.into(),
    }
}

/// A workspace with a frame and one window on a text buffer.
fn frame(cols: usize, rows: usize, text: &str) -> (Ctx, BufferId, WindowId) {
    let mut c = ctx();
    c.set_frame_size(cols, rows);
    let b = c.create_text("t", text);
    let w = c.open_window(b, 1, 1).unwrap();
    (c, b, w)
}

fn place(c: &Ctx, w: WindowId) -> (usize, usize) {
    let p = c.placements().into_iter().find(|p| p.window == w).unwrap();
    (p.col, p.row)
}

// ------------------------------------------------------------------ splits

#[test]
fn a_first_window_fills_the_frame() {
    let (c, _, w) = frame(80, 24, "x");
    assert_eq!(c.window_size(w), Some((80, 22)), "status and command line");
    assert_eq!(place(&c, w), (0, 0));
    assert_eq!(c.window_list(), [w]);
}

#[test]
fn a_split_takes_half_on_its_side_and_the_focus_stays() {
    let (mut c, _, w) = frame(81, 24, "x");
    let right = c.split_window(w, Side::Right).unwrap();
    assert_eq!(c.focused(), Some(w));
    assert_eq!(
        c.window_size(w),
        Some((40, 22)),
        "41 cells with the divider"
    );
    assert_eq!(
        c.window_size(right),
        Some((40, 22)),
        "the frame's right edge"
    );
    assert_eq!(place(&c, right), (41, 0));

    let (mut c, _, w) = frame(80, 24, "x");
    let left = c.split_window(w, Side::Left).unwrap();
    assert_eq!(place(&c, left), (0, 0));
    assert_eq!(place(&c, w), (40, 0));
    assert_eq!(c.window_list(), [left, w]);

    let (mut c, _, w) = frame(80, 24, "x");
    let above = c.split_window(w, Side::Above).unwrap();
    assert_eq!(place(&c, above), (0, 0));
    assert_eq!(place(&c, w), (0, 11));
    assert_eq!(
        c.window_size(above),
        Some((80, 10)),
        "half of 23, less the status"
    );
    assert_eq!(c.window_size(w), Some((80, 11)), "the rest");

    let (mut c, _, w) = frame(80, 24, "x");
    let below = c.split_window(w, Side::Below).unwrap();
    assert_eq!(place(&c, w), (0, 0));
    assert_eq!(place(&c, below), (0, 12));
}

#[test]
fn a_split_shows_the_same_buffer_at_the_same_place() {
    let text = "line\n".repeat(100);
    let (mut c, b, w) = frame(40, 12, &text);
    c.text_mut(b).unwrap().set_cursor(w, 300).unwrap();
    let _ = c.scene();
    let top = c.text(b).unwrap().scroll(w);
    assert!(top.is_some_and(|t| t.0 > 0), "it scrolled: {top:?}");
    let new = c.split_window(w, Side::Right).unwrap();
    assert_eq!(c.window_buffer(new), Some(b));
    assert_eq!(c.text(b).unwrap().cursor(new), Some(300));
    assert_eq!(c.text(b).unwrap().scroll(new), top);
}

#[test]
fn a_window_too_small_to_divide_refuses() {
    let (mut c, _, w) = frame(3, 24, "x");
    assert!(c.split_window(w, Side::Right).is_err());
    assert_eq!(c.window_list(), [w]);
    assert!(
        c.split_window(w, Side::Below).is_ok(),
        "there's room this way"
    );
    let (mut c, _, w) = frame(80, 4, "x");
    assert!(c.split_window(w, Side::Below).is_err());
    assert_eq!(c.window_list(), [w]);
}

#[test]
fn a_split_the_same_way_joins_its_parent() {
    let (mut c, _, a) = frame(120, 24, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Right).unwrap();
    assert_eq!(c.window_list(), [a, b, d]);
    assert_eq!(place(&c, a), (0, 0));
    assert_eq!(place(&c, b), (60, 0));
    assert_eq!(place(&c, d), (90, 0));
}

#[test]
fn the_only_window_cant_close_and_a_dead_one_is_gone() {
    let (mut c, _, a) = frame(80, 24, "x");
    assert!(c.close_window(a).is_err());
    let b = c.split_window(a, Side::Right).unwrap();
    c.close_window(b).unwrap();
    assert_eq!(c.close_window(b), Err(Checkout::Gone.into()));
    assert_eq!(c.window_size(a), Some((80, 22)), "it got the cells back");
    assert_eq!(c.window_buffer(b), None);
    assert_eq!(c.placements().len(), 1);
}

#[test]
fn closing_gives_the_cells_to_the_neighbour_before_it() {
    let (mut c, _, a) = frame(120, 24, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Right).unwrap();
    let before = c.window_size(d);
    c.close_window(b).unwrap();
    assert_eq!(c.window_list(), [a, d]);
    assert_eq!(
        c.window_size(a).unwrap().0,
        89,
        "a's 60 and b's 30, less a divider"
    );
    assert_eq!(c.window_size(d), before);

    // The first has no neighbour before it.
    let (mut c, _, a) = frame(120, 24, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    c.close_window(a).unwrap();
    assert_eq!(c.window_size(b), Some((120, 22)));
}

#[test]
fn closing_the_focused_window_focuses_the_one_used_last() {
    let (mut c, _, a) = frame(120, 48, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(a, Side::Below).unwrap();
    c.focus(d);
    c.focus(b);
    c.focus(a);
    c.close_window(a).unwrap();
    assert_eq!(c.focused(), Some(b));
    c.close_window(b).unwrap();
    assert_eq!(c.focused(), Some(d));
    assert_eq!(
        c.window_size(d),
        Some((120, 46)),
        "a nested split closed up"
    );
}

#[test]
fn closing_doesnt_focus_a_window_never_used_over_one_that_was() {
    let (mut c, _, a) = frame(120, 48, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    c.focus(b);
    c.focus(a);
    let _new = c.split_window(b, Side::Below).unwrap();
    let d = c.split_window(a, Side::Below).unwrap();
    assert_ne!(d, b);
    c.close_window(a).unwrap();
    assert_eq!(c.focused(), Some(b));
}

#[test]
fn a_nested_split_left_with_one_window_joins_the_split_around_it() {
    let (mut c, _, a) = frame(120, 48, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Below).unwrap();
    let e = c.split_window(b, Side::Right).unwrap();
    c.close_window(d).unwrap();
    assert_eq!(c.window_list(), [a, b, e]);
    // Now they're one split, so resizing b takes from e and leaves a alone.
    let (ac, bc) = (c.window_size(a).unwrap().0, c.window_size(b).unwrap().0);
    c.resize(b, bc + 2, 46).unwrap();
    assert_eq!(c.window_size(a).unwrap().0, ac);
    assert_eq!(c.window_size(b).unwrap().0, bc + 2);
}

#[test]
fn only_leaves_one_and_forgets_the_others_views() {
    let (mut c, buf, a) = frame(80, 24, "text");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(a, Side::Below).unwrap();
    c.only_window(b).unwrap();
    assert_eq!(c.focused(), Some(b));
    assert_eq!(c.window_list(), [b]);
    assert_eq!(c.window_size(b), Some((80, 22)));
    for gone in [a, d] {
        assert_eq!(c.window_buffer(gone), None);
        assert_eq!(c.text(buf).unwrap().cursor(gone), None);
    }
}

// ------------------------------------------------------------------ resize and balance

#[test]
fn resize_moves_one_border_as_far_as_it_will_go() {
    let (mut c, _, a) = frame(100, 24, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    c.resize(a, 59, 22).unwrap();
    assert_eq!(c.window_size(a), Some((59, 22)));
    assert_eq!(c.window_size(b), Some((40, 22)));
    assert_eq!(place(&c, b), (60, 0));
    c.resize(a, 1000, 22).unwrap();
    assert_eq!(c.window_size(b), Some((2, 22)), "its minimum at the edge");
    assert_eq!(c.window_size(a), Some((97, 22)));
    c.resize(a, 0, 22).unwrap();
    assert_eq!(
        c.window_size(a),
        Some((1, 22)),
        "a cell of text and the divider"
    );
    c.resize(b, 50, 22).unwrap();
    assert_eq!(c.window_size(b), Some((50, 22)));
}

#[test]
fn resize_can_take_from_the_split_around_it() {
    let (mut c, _, a) = frame(100, 48, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Below).unwrap();
    let e = c.split_window(b, Side::Right).unwrap();
    let e_cols = c.window_size(e).unwrap().0;
    let (b_cols, a_cols) = (c.window_size(b).unwrap().0, c.window_size(a).unwrap().0);
    c.resize(b, b_cols + e_cols + 10, 22).unwrap();
    assert_eq!(c.window_size(e).unwrap().0, 2, "e gave what it had first");
    assert_eq!(c.window_size(a).unwrap().0, a_cols - 12, "then a");
    assert_eq!(
        c.window_size(d).unwrap().0,
        100 - (a_cols - 12) - 1,
        "d spans what's right of a"
    );
}

#[test]
fn a_frame_made_small_and_big_again_gives_the_sizes_back() {
    let (mut c, _, a) = frame(100, 48, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Below).unwrap();
    c.resize(a, 30, 46).unwrap();
    let sizes = |c: &Ctx| [a, b, d].map(|w| c.window_size(w).unwrap());
    let before = sizes(&c);
    c.set_frame_size(5, 3);
    let _ = c.scene();
    c.set_frame_size(100, 48);
    assert_eq!(sizes(&c), before);
}

#[test]
fn balance_follows_how_many_windows_each_side_holds() {
    let (mut c, _, a) = frame(121, 24, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Right).unwrap();
    c.balance_windows(a);
    assert_eq!(c.window_size(a).unwrap().0, 40, "41 with its divider");
    assert_eq!(c.window_size(b).unwrap().0, 39, "40 with its divider");
    assert_eq!(c.window_size(d).unwrap().0, 40, "40 at the frame's edge");

    // A side split in two the other way counts one wide.
    let (mut c, _, a) = frame(121, 48, "x");
    let b = c.split_window(a, Side::Right).unwrap();
    let d = c.split_window(b, Side::Below).unwrap();
    c.resize(a, 90, 46).unwrap();
    c.balance_windows(a);
    assert_eq!(c.window_size(a).unwrap().0, 60);
    assert_eq!(c.window_size(b).unwrap().1, c.window_size(d).unwrap().1 + 1);
}

#[test]
fn a_sole_window_in_a_frame_the_host_set_stays_its_size() {
    let (mut c, _, w) = frame(80, 24, "x");
    c.resize(w, 10, 10).unwrap();
    assert_eq!(c.window_size(w), Some((80, 22)));
}

// ------------------------------------------------------------------ S4's windows

#[test]
fn a_sole_window_with_no_frame_says_what_the_frame_is() {
    let mut c = ctx();
    let b = c.create_text("t", "x");
    let w = c.open_window(b, 30, 5).unwrap();
    assert_eq!(c.frame_size(), (30, 7));
    c.resize(w, 40, 10).unwrap();
    assert_eq!(c.window_size(w), Some((40, 10)));
    assert_eq!(c.frame_size(), (40, 12));
    let s = c.scene();
    assert_eq!((s.cols, s.rows), (40, 12));
    let right = c.split_window(w, Side::Right).unwrap();
    assert_eq!(c.window_size(right), Some((20, 10)), "half the new size");
}

#[test]
fn a_second_open_window_stays_out_of_the_tree_at_its_own_size() {
    let mut c = ctx();
    let b = c.create_text("t", "x");
    let first = c.open_window(b, 30, 5).unwrap();
    let w = c.open_window(b, 11, 3).unwrap();
    assert_eq!(c.focused(), Some(w));
    assert_eq!(c.window_size(w), Some((11, 3)));
    c.resize(w, 12, 4).unwrap();
    assert_eq!(c.window_size(w), Some((12, 4)));
    assert_eq!(c.window_size(first), Some((30, 5)));
    assert_eq!(c.window_list(), [first]);
    assert!(c.split_window(w, Side::Right).is_err());
    assert_eq!(c.window_in_direction(w, Side::Left), None);
    c.close_window(w).unwrap();
    assert_eq!(c.focused(), Some(first));
}

/// a | (b e / d), split in the order that leaves a nested split to dissolve.
fn a_beside_b_e_over_d(balance: bool) -> (Ctx, [WindowId; 4]) {
    let (mut c, _, a) = frame(80, 24, "");
    let even = |c: &mut Ctx, w| {
        if balance {
            c.balance_windows(w);
        }
    };
    let b = c.split_window(a, Side::Right).unwrap();
    even(&mut c, a);
    let d = c.split_window(b, Side::Below).unwrap();
    even(&mut c, a);
    let e = c.split_window(b, Side::Right).unwrap();
    even(&mut c, a);
    (c, [a, b, e, d])
}

#[test]
fn a_close_that_dissolves_a_split_leaves_every_width() {
    let widths = |c: &Ctx, ws: &[WindowId]| -> Vec<usize> {
        ws.iter().map(|w| c.window_size(*w).unwrap().0).collect()
    };

    // After a balance, whose design counts windows and not cells.
    let (mut c, [a, b, e, d]) = a_beside_b_e_over_d(true);
    let had = widths(&c, &[a, b, e]);
    c.close_window(d).unwrap();
    assert_eq!(widths(&c, &[a, b, e]), had, "after a balance");
    assert_eq!(c.window_size(b).unwrap().1, 22, "d's height went to b");
    assert_eq!(c.window_size(e).unwrap().1, 22, "and to the one beside it");

    // After a frame resize, either way, with the design at another length than the screen's.
    for cols in [40, 160] {
        let (mut c, [a, b, e, d]) = a_beside_b_e_over_d(false);
        c.set_frame_size(cols, 24);
        let had = widths(&c, &[a, b, e]);
        c.close_window(d).unwrap();
        assert_eq!(widths(&c, &[a, b, e]), had, "in a frame {cols} wide");
        c.set_frame_size(80, 24);
        assert_eq!(c.window_size(a).unwrap().0, 39, "a is still half of 80");
    }
}

#[test]
fn a_second_open_window_leaves_the_focus_in_a_frame_the_host_set() {
    let (mut c, x, a) = frame(80, 24, "x");
    let y = c.create_text("y", "y");
    let outside = c.open_window(y, 20, 5).unwrap();
    assert_eq!(c.focused(), Some(a), "scene() doesn't draw the new one");
    assert_eq!(c.window_list(), [a]);
    assert_eq!(c.window_size(outside), Some((20, 5)));
    let solid = |k: &Cursor| k.shape != CursorShape::Hollow;
    assert!(c.scene().cursors.iter().any(solid));
    let b = c.show(x, ShowIn::Split(Side::Right)).unwrap().unwrap();
    assert_eq!(c.window_list(), [a, b]);
    assert_eq!(c.focus_direction(Side::Left), Some(a));
}

#[test]
fn closing_focuses_a_window_in_the_tree_over_one_outside_it() {
    let (mut c, _, a) = frame(80, 24, "x");
    let y = c.create_text("y", "y");
    let outside = c.open_window(y, 20, 5).unwrap();
    let b = c.split_window(a, Side::Right).unwrap();
    c.focus(outside);
    c.focus(b);
    c.close_window(b).unwrap();
    assert_eq!(
        c.focused(),
        Some(a),
        "outside was used later, and isn't drawn"
    );
}

// ------------------------------------------------------------------ focus by direction

/// a over b on the left, c on the right.
fn two_left_one_right() -> (Ctx, BufferId, [WindowId; 3]) {
    let (mut c, buf, a) = frame(100, 48, &"line\n".repeat(50));
    let r = c.split_window(a, Side::Right).unwrap();
    let b = c.split_window(a, Side::Below).unwrap();
    (c, buf, [a, b, r])
}

#[test]
fn focus_moves_to_the_window_on_that_side() {
    let (mut c, _, [a, b, r]) = two_left_one_right();
    c.focus(a);
    assert_eq!(c.focus_direction(Side::Below), Some(b));
    assert_eq!(c.focused(), Some(b));
    assert_eq!(c.focus_direction(Side::Above), Some(a));
    assert_eq!(c.focus_direction(Side::Right), Some(r));
    assert_eq!(c.focused(), Some(r));
}

#[test]
fn at_the_frames_edge_there_is_no_window_and_the_focus_stays() {
    let (mut c, _, [a, b, r]) = two_left_one_right();
    c.focus(a);
    assert_eq!(c.focus_direction(Side::Left), None);
    assert_eq!(c.focus_direction(Side::Above), None);
    assert_eq!(c.focused(), Some(a));
    c.focus(b);
    assert_eq!(c.focus_direction(Side::Below), None);
    c.focus(r);
    assert_eq!(c.focus_direction(Side::Right), None);
}

#[test]
fn the_cursors_row_picks_between_two_windows_on_that_side() {
    let (mut c, buf, [a, b, r]) = two_left_one_right();
    c.focus(r);
    assert_eq!(c.window_in_direction(r, Side::Left), Some(a), "row 0");
    // Line 30 is below a's rows, so it's across from b.
    let pos = c.text(buf).unwrap().line_start(30);
    c.text_mut(buf).unwrap().set_cursor(r, pos).unwrap();
    assert_eq!(c.window_in_direction(r, Side::Left), Some(b));
}

// ------------------------------------------------------------------ killing and showing

#[test]
fn killing_a_buffer_shown_in_two_windows_moves_both_on() {
    let (mut c, gone, a) = frame(80, 24, "doomed");
    let b = c.split_window(a, Side::Right).unwrap();
    let kept = c.create_text("kept", "still here");
    let log = Rc::new(RefCell::new(vec![]));
    let l = log.clone();
    let _hook = c.on_buffer_killed(move |_, b| {
        l.borrow_mut().push(b);
        Ok(())
    });
    assert!(c.kill(gone));
    assert!(!c.is_live(gone));
    assert_eq!(c.window_buffer(a), Some(kept));
    assert_eq!(c.window_buffer(b), Some(kept));
    assert!(c.text(kept).unwrap().cursor(a).is_some());
    assert!(c.text(kept).unwrap().cursor(b).is_some());
    assert_eq!(c.focused_buffer(), Some(kept));
    c.run_until_idle();
    assert_eq!(*log.borrow(), [gone]);
    assert!(!c.kill(gone), "already dead");
    let s = c.scene();
    assert!(s.row_text(0).contains("still here"));
}

#[test]
fn killing_the_last_buffer_leaves_a_scratch() {
    let (mut c, gone, a) = frame(80, 24, "doomed");
    assert!(c.kill(gone));
    let b = c.window_buffer(a).unwrap();
    assert_eq!(c.buffer_name(b), Some("*scratch*"));
    assert_eq!(c.text(b).unwrap().len(), 0);
    assert_eq!(c.text(b).unwrap().cursor(a), Some(0));
    assert_eq!(c.state(), State::Normal);
}

#[test]
fn killing_a_buffer_no_window_shows_moves_nothing() {
    let (mut c, b, a) = frame(80, 24, "shown");
    let other = c.create_text("other", "");
    assert!(c.kill(other));
    assert_eq!(c.window_buffer(a), Some(b));
    assert_eq!(c.buffer_list(), [b], "and no scratch");
}

#[test]
fn a_killed_buffers_window_prefers_one_no_window_shows() {
    let (mut c, gone, a) = frame(80, 24, "doomed");
    let b = c.split_window(a, Side::Right).unwrap();
    let shown = c.create_text("shown", "");
    let unseen = c.create_text("unseen", "");
    c.show(shown, ShowIn::Current).unwrap();
    assert_eq!(c.window_buffer(a), Some(shown));
    assert_eq!(c.window_buffer(b), Some(gone));
    assert!(c.kill(gone));
    assert_eq!(c.window_buffer(b), Some(unseen));
}

#[test]
fn show_current_switches_the_acting_window() {
    let (mut c, _, a) = frame(80, 24, "first");
    let other = c.create_text("other", "second");
    assert_eq!(c.show(other, ShowIn::Current), Ok(Some(a)));
    assert_eq!(c.window_buffer(a), Some(other));
    assert_eq!(c.focused_buffer(), Some(other));
    assert_eq!(c.window_list(), [a]);
}

#[test]
fn show_in_a_split_makes_a_window_and_focuses_it() {
    let (mut c, b, a) = frame(80, 24, "first");
    let other = c.create_text("other", "second");
    let w = c.show(other, ShowIn::Split(Side::Below)).unwrap().unwrap();
    assert_ne!(w, a);
    assert_eq!(c.focused(), Some(w));
    assert_eq!(c.window_buffer(w), Some(other));
    assert_eq!(c.window_buffer(a), Some(b));
    assert_eq!(c.text(other).unwrap().cursor(w), Some(0));
    assert_eq!(c.text(b).unwrap().cursor(a), Some(0));
}

#[test]
fn show_existing_finds_the_window_or_uses_the_current_one() {
    let (mut c, b, a) = frame(80, 24, "first");
    let other = c.create_text("other", "second");
    let w = c.split_window(a, Side::Right).unwrap();
    c.show(other, ShowIn::Current).unwrap();
    assert_eq!(c.window_buffer(a), Some(other));
    // b is in w only.
    assert_eq!(c.show(b, ShowIn::Existing), Ok(Some(w)));
    assert_eq!(c.focused(), Some(w));
    assert_eq!(c.window_buffer(a), Some(other), "nothing else changed");
    // No window shows this one.
    let third = c.create_text("third", "");
    c.focus(a);
    assert_eq!(c.show(third, ShowIn::Existing), Ok(Some(a)));
    assert_eq!(c.window_buffer(a), Some(third));
}

#[test]
fn show_existing_prefers_the_current_window() {
    let (mut c, b, a) = frame(80, 24, "first");
    let w = c.split_window(a, Side::Right).unwrap();
    c.focus(w);
    c.focus(a);
    assert_eq!(c.show(b, ShowIn::Existing), Ok(Some(a)));
    assert_eq!(c.focused(), Some(a));
}

#[test]
fn show_background_shows_nothing() {
    let (mut c, b, a) = frame(80, 24, "first");
    let other = c.create_text("other", "");
    assert_eq!(c.show(other, ShowIn::Background), Ok(None));
    assert_eq!(c.window_buffer(a), Some(b));
    assert!(c.is_live(other));
    assert!(c.kill(other));
    assert_eq!(
        c.show(other, ShowIn::Background),
        Err(Checkout::Gone.into())
    );
    assert_eq!(c.show(other, ShowIn::Current), Err(Checkout::Gone.into()));
}

#[test]
fn show_with_no_window_makes_the_first_one() {
    let mut c = ctx();
    c.set_frame_size(50, 20);
    let b = c.create_text("t", "x");
    let w = c.show(b, ShowIn::Current).unwrap().unwrap();
    assert_eq!(c.focused(), Some(w));
    assert_eq!(c.window_size(w), Some((50, 18)));
    let mut c = ctx();
    let b = c.create_text("t", "x");
    assert!(c.show(b, ShowIn::Split(Side::Below)).is_err());
}

#[test]
fn a_command_shows_in_the_window_it_acts_for() {
    let cfg = Config {
        register: Some(Box::new(|reg: &mut Registry| {
            reg.command("show-other", "Show the buffer named other.", |ctx, _| {
                let b = ctx.find_buffer("other").unwrap();
                ctx.show(b, ShowIn::Current).map(|_| ())
            });
        })),
        ..Default::default()
    };
    let mut c = Ctx::with_config(vec![], cfg).unwrap();
    c.set_frame_size(80, 24);
    let first = c.create_text("first", "");
    let a = c.open_window(first, 1, 1).unwrap();
    let w = c.split_window(a, Side::Right).unwrap();
    let other = c.create_text("other", "");
    let args = Args {
        window: Some(w),
        ..Default::default()
    };
    c.run("show-other", &args).unwrap();
    assert_eq!(c.window_buffer(w), Some(other));
    assert_eq!(c.window_buffer(a), Some(first));
    assert_eq!(c.focused(), Some(w));
}

#[test]
fn a_view_waits_for_the_window_that_left_and_moves_through_edits() {
    let (mut c, b, w) = frame(80, 24, "0123456789");
    c.text_mut(b).unwrap().set_cursor(w, 5).unwrap();
    let other = c.create_text("other", "");
    c.show(other, ShowIn::Current).unwrap();
    assert_eq!(c.text(b).unwrap().cursor(w), None, "it doesn't show it now");
    // An edit before the cursor, from anywhere.
    c.text_mut(b)
        .unwrap()
        .tx(Origin::Extension)
        .insert(0, "abc")
        .commit()
        .unwrap();
    c.show(b, ShowIn::Current).unwrap();
    assert_eq!(c.text(b).unwrap().cursor(w), Some(8));
}

#[test]
fn a_new_window_on_a_buffer_starts_where_the_last_one_left_it() {
    let (mut c, b, w) = frame(80, 24, "0123456789");
    let right = c.split_window(w, Side::Right).unwrap();
    c.text_mut(b).unwrap().set_cursor(right, 7).unwrap();
    let other = c.create_text("other", "");
    c.show(other, ShowIn::Current).unwrap();
    // w was the current one, and shows `other`. Closing `right` leaves b on no window.
    assert_eq!(c.window_buffer(w), Some(other));
    c.close_window(right).unwrap();
    let new = c.show(b, ShowIn::Split(Side::Below)).unwrap().unwrap();
    assert_eq!(c.text(b).unwrap().cursor(new), Some(7));

    // Shown in a window already, it starts where that one is.
    let (mut c, other, w) = frame(80, 24, "");
    let right = c.split_window(w, Side::Right).unwrap();
    let b = c.create_text("b", "0123456789");
    c.show(b, ShowIn::Current).unwrap();
    c.text_mut(b).unwrap().set_cursor(w, 3).unwrap();
    c.focus(right);
    assert_eq!(c.window_buffer(right), Some(other));
    c.show(b, ShowIn::Current).unwrap();
    assert_eq!(c.text(b).unwrap().cursor(right), Some(3));
}

#[test]
fn closing_a_window_that_left_a_buffer_long_ago_doesnt_move_where_it_rests() {
    let (mut c, x, a) = frame(80, 24, "0123456789012345678901234");
    let b = c.split_window(a, Side::Right).unwrap();
    let y = c.create_text("y", "");
    c.text_mut(x).unwrap().set_cursor(a, 5).unwrap();
    c.show(y, ShowIn::Current).unwrap();
    c.focus(b);
    c.text_mut(x).unwrap().set_cursor(b, 20).unwrap();
    c.show(y, ShowIn::Current).unwrap();
    c.close_window(a).unwrap();
    let new = c.split_window(b, Side::Right).unwrap();
    c.focus(new);
    c.show(x, ShowIn::Current).unwrap();
    assert_eq!(c.text(x).unwrap().cursor(new), Some(20), "b left last");
}

// ------------------------------------------------------------------ names and directories

#[test]
fn names_stay_unique() {
    let mut c = ctx();
    c.create_text("a", "");
    let b = c.create_text("a", "");
    assert_eq!(c.buffer_name(b), Some("a<2>"));
    let s = c.create_surface("a", Plain::default());
    assert_eq!(c.buffer_name(s), Some("a<3>"));
    assert_eq!(c.find_buffer("a<2>"), Some(b));
    assert!(c.kill(b));
    assert_eq!(c.buffer_name(b), None);
    assert_eq!(c.find_buffer("a<2>"), None);
}

#[test]
fn buffer_list_has_the_one_used_last_first() {
    let mut c = ctx();
    let a = c.create_text("a", "");
    let b = c.create_text("b", "");
    assert_eq!(c.buffer_list(), [b, a]);
    let w = c.open_window(a, 10, 10).unwrap();
    assert_eq!(c.buffer_list(), [a, b]);
    c.show(b, ShowIn::Current).unwrap();
    assert_eq!(c.buffer_list(), [b, a]);
    assert_eq!(c.window_buffer(w), Some(b));
}

#[test]
fn a_directory_comes_from_the_file_or_the_focused_buffer() {
    let mut c = ctx();
    let start = std::env::current_dir().unwrap();
    let a = c.create_text("a", "");
    assert_eq!(c.directory(a), Some(start.as_path()));

    let file = c
        .open_file(Path::new("/nonexistent-dir-for-die/sub/f.txt"))
        .unwrap();
    assert_eq!(
        c.directory(file),
        Some(Path::new("/nonexistent-dir-for-die/sub"))
    );

    c.open_window(file, 10, 10).unwrap();
    let b = c.create_text("b", "");
    assert_eq!(
        c.directory(b),
        Some(Path::new("/nonexistent-dir-for-die/sub")),
        "as in Emacs"
    );
    c.set_directory(b, "/somewhere");
    assert_eq!(c.directory(b), Some(Path::new("/somewhere")));
    assert!(c.kill(b));
    assert_eq!(c.directory(b), None);
    c.set_directory(b, "/ignored");
}

// ------------------------------------------------------------------ the modified flag

fn edit(c: &mut Ctx, b: BufferId, at: usize, s: &str) {
    let t = c.text_mut(b).unwrap();
    t.tx(Origin::User).insert(at, s).commit().unwrap();
    t.undo_boundary();
}

#[test]
fn modified_follows_undo_and_redo_back_to_the_saved_text() {
    let (mut c, b, _) = frame(80, 24, "abc");
    let flag = |c: &Ctx| c.text(b).unwrap().modified();
    assert!(!flag(&c), "a new buffer");
    edit(&mut c, b, 0, "x");
    assert!(flag(&c));
    edit(&mut c, b, 0, "y");
    c.text_mut(b).unwrap().undo().unwrap();
    assert!(flag(&c), "still one edit off");
    c.text_mut(b).unwrap().undo().unwrap();
    assert!(!flag(&c), "back at the saved text");
    c.text_mut(b).unwrap().redo().unwrap();
    assert!(flag(&c));
}

#[test]
fn a_save_moves_what_counts_as_unmodified() {
    let (mut c, b, _) = frame(80, 24, "abc");
    let flag = |c: &Ctx| c.text(b).unwrap().modified();
    edit(&mut c, b, 0, "x");
    c.text_mut(b).unwrap().set_modified(false);
    assert!(!flag(&c));
    c.text_mut(b).unwrap().undo().unwrap();
    assert!(flag(&c), "before the save");
    c.text_mut(b).unwrap().redo().unwrap();
    assert!(!flag(&c), "the saved text again");
    c.text_mut(b).unwrap().set_modified(true);
    assert!(flag(&c), "said so");
}

#[test]
fn modified_after_a_save_in_the_middle_of_a_step() {
    let (mut c, b, _) = frame(80, 24, "abc");
    let t = c.text_mut(b).unwrap();
    t.begin_undo_group();
    t.tx(Origin::User).insert(0, "x").commit().unwrap();
    t.set_modified(false);
    t.tx(Origin::User).insert(0, "y").commit().unwrap();
    assert!(t.modified(), "a later edit in the same step");
    t.end_undo_group();
    t.undo().unwrap();
    assert!(t.modified(), "the step's start isn't what was saved");
}

#[test]
fn what_doesnt_change_the_text_is_as_emacs_counts_it() {
    let (mut c, b, _) = frame(80, 24, "abc");
    let t = c.text_mut(b).unwrap();
    t.tx(Origin::User).insert(1, "").commit().unwrap();
    assert!(!t.modified(), "an empty change is nothing");
    t.tx(Origin::User).replace(0..1, "a").commit().unwrap();
    assert!(t.modified(), "text replaced by itself is an undo step");
}

#[test]
fn a_silent_edit_stays_modified_until_a_save() {
    let (mut c, b, _) = frame(80, 24, "abc");
    let t = c.text_mut(b).unwrap();
    t.tx(Origin::User).insert(0, "x").commit().unwrap();
    t.undo_boundary();
    t.tx(Origin::Extension)
        .silent()
        .insert(0, "s")
        .commit()
        .unwrap();
    t.undo().unwrap();
    assert!(t.modified(), "undo can't take a silent edit back");
    t.set_modified(false);
    assert!(!t.modified());
}

// ------------------------------------------------------------------ registers and the clipboard

#[test]
fn registers_keep_their_kind() {
    let mut c = ctx();
    for (name, kind) in [
        ('a', RegKind::Char),
        ('b', RegKind::Line),
        ('c', RegKind::Block),
    ] {
        let r = Register {
            kind,
            text: "t".into(),
        };
        c.set_register(name, r.clone());
        assert_eq!(c.register(name), Some(&r));
    }
    assert_eq!(c.register('d'), None);
}

#[test]
fn another_apps_copy_is_news_once() {
    let mut c = ctx();
    let board = FakeClipboard::default();
    c.set_clipboard(board.clone());
    assert_eq!(c.clipboard_news(), None, "nothing there");
    board.copy_from_outside("copied");
    assert_eq!(c.clipboard_news(), Some(chars("copied")));
    assert_eq!(c.clipboard_news(), None, "only once");
    assert_eq!(c.clipboard(), Some(chars("copied")), "still there");
    board.copy_from_outside("copied");
    assert_eq!(
        c.clipboard_news(),
        None,
        "the text it knows, as Emacs compares"
    );
    board.copy_from_outside("again");
    assert_eq!(c.clipboard_news(), Some(chars("again")));
}

#[test]
fn what_was_there_at_startup_counts_as_news() {
    let mut c = ctx();
    let board = FakeClipboard::default();
    board.copy_from_outside("from before");
    c.set_clipboard(board.clone());
    assert_eq!(c.clipboard_news(), Some(chars("from before")));
}

#[test]
fn dies_own_write_isnt_news_and_its_kind_survives() {
    let mut c = ctx();
    let board = FakeClipboard::default();
    c.set_clipboard(board.clone());
    let r = Register {
        kind: RegKind::Line,
        text: "line\n".into(),
    };
    c.write_clipboard(&r);
    assert_eq!(c.clipboard_news(), None);
    assert_eq!(c.clipboard(), Some(r.clone()));
    assert_eq!(board.take_writes(), [r]);
    assert_eq!(board.take_writes(), []);
}

#[test]
fn a_config_can_bring_the_clipboard_and_each_workspace_has_its_own_otherwise() {
    let board = FakeClipboard::default();
    board.copy_from_outside("mine");
    let cfg = Config {
        clipboard: Some(Box::new(board)),
        ..Default::default()
    };
    let mut c = Ctx::with_config(vec![], cfg).unwrap();
    assert_eq!(c.clipboard(), Some(chars("mine")));
    let mut other = ctx();
    assert_eq!(other.clipboard(), None);
}

// ------------------------------------------------------------------ surfaces

#[derive(Default)]
struct Plain {
    log: Rc<RefCell<Vec<Event>>>,
}

impl Surface for Plain {
    fn kind(&self) -> &'static str {
        "plain"
    }
    fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        self.log.borrow_mut().push(ev.clone());
        false
    }
}

#[test]
fn a_surface_hears_its_size_and_the_focus() {
    let (mut c, _, a) = frame(80, 24, "text");
    let log: Rc<RefCell<Vec<Event>>> = Rc::default();
    let s = c.create_surface("s", Plain { log: log.clone() });
    let w = c.show(s, ShowIn::Split(Side::Right)).unwrap().unwrap();
    c.run_until_idle();
    assert_eq!(
        *log.borrow(),
        [Event::Resize { cols: 40, rows: 22 }, Event::Focus(true),],
        "shown in a new window, then focused"
    );
    log.borrow_mut().clear();

    c.focus(a);
    c.run_until_idle();
    assert_eq!(*log.borrow(), [Event::Focus(false)]);
    log.borrow_mut().clear();

    c.focus(w);
    c.resize(a, 30, 22).unwrap();
    c.run_until_idle();
    assert_eq!(
        *log.borrow(),
        [Event::Focus(true), Event::Resize { cols: 49, rows: 22 }]
    );
    log.borrow_mut().clear();

    c.close_window(w).unwrap();
    c.run_until_idle();
    assert_eq!(*log.borrow(), [Event::Focus(false)]);
}

#[test]
fn a_window_on_a_surface_has_a_direction_from_its_first_cell() {
    let (mut c, _, a) = frame(80, 24, "text");
    let s = c.create_surface("s", Plain::default());
    let w = c.show(s, ShowIn::Split(Side::Right)).unwrap().unwrap();
    assert_eq!(c.window_in_direction(w, Side::Left), Some(a));
    assert_eq!(c.window_in_direction(a, Side::Right), Some(w));
}
