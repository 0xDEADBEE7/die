//! The pointer, as a dashboard's (contract-p2 §0, briefs/mouse-ux.md §2–§4, §6–§10): hover with
//! no delay and its card, the nearest series brought forward, the pointer leaving, the focusing
//! press, clicks, ⇧-click, the brush and its Esc, pans by ⌥ and the middle button and on an
//! axis, the wheel's bursts, ⌘-wheel zoom about the pointer, double-clicks, the legend, and the
//! right button, Space and Ctrl-click opening nothing. Presents are held to the overlay's union.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Button, Input, KeyEvent, Mods, PointerKind};
use die_fig::interact::Cursor;
use die_fig::interact::consts::{CLICK_SLOP, NEAREST_SERIES};
use die_fig::scene::Rect;
use std::time::Duration;

fn point(series: &str, row: u32) -> Cursor {
    Cursor::Point {
        series: die_fig::data::SeriesKey::new(series, 0),
        row,
    }
}

/// Let every ease finish and every burst end.
fn settle(s: &mut Setup) {
    s.h.advance(Duration::from_millis(400));
}

/// The overlay's last drawn bounds.
fn bounds(s: &mut Setup) -> Option<Rect> {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].overlay_bounds()
}

/// A zoomed-in view, settled, so there's room to pan.
fn zoomed() -> Setup {
    let mut s = setup();
    keys(&mut s.h, "++++");
    settle(&mut s);
    assert!(view(&mut s).x.is_some());
    s
}

#[test]
fn hover_snaps_to_the_nearest_sample_at_once_and_redraws_only_the_overlay() {
    let mut s = setup();
    let (base, over) = buffers(&mut s);
    presents(&mut s.h);
    let (x, y) = sample_at(&mut s, "a", 5);
    mv(&mut s, (x + 1.5, y));
    assert_eq!(
        hover(&mut s),
        Some(point("a", 5)),
        "no delay, the nearest x"
    );
    let p = presents(&mut s.h);
    assert!(!p.is_empty(), "the card and crosshair drawn");
    assert!(
        p.iter().all(|(id, _)| *id == over),
        "the base untouched: {p:?}"
    );
    assert!(!p.iter().any(|(id, _)| *id == base));
    // Every sample, within a cell too: the crosshair follows the hand to the next sample.
    let (x6, _) = sample_at(&mut s, "a", 6);
    mv(&mut s, (x6 - 0.5, y));
    assert_eq!(hover(&mut s), Some(point("a", 6)));
}

/// Whether `inner` lies within `outer`.
fn within(inner: die_core::PixelRect, outer: die_core::PixelRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

#[test]
fn a_hover_presents_within_the_union_of_what_it_drew_and_draws() {
    let mut s = setup();
    let (_, over) = buffers(&mut s);
    let (x, y) = sample_at(&mut s, "a", 4);
    mv(&mut s, (x, y));
    let before = bounds(&mut s).expect("the overlay drew");
    presents(&mut s.h);
    let (x, y) = sample_at(&mut s, "a", 5);
    mv(&mut s, (x, y));
    let after = bounds(&mut s).expect("drawn again");
    let p = presents(&mut s.h);
    assert_eq!(p.len(), 1, "one present for one move: {p:?}");
    assert_eq!(p[0].0, over);
    let (w, h, _) = s.h.pixels(over).unwrap();
    let union = die_fig::raster::to_physical(before.union(&after), 2.0, w, h);
    assert!(
        within(p[0].1, union.into()),
        "{:?} within {union:?}",
        p[0].1
    );
    // Small: the old and new crosshair, dots and card, not the plot the emphasis washes.
    let plot = die_fig::raster::to_physical(plot(&mut s), 2.0, w, h);
    assert!(
        p[0].1.width * p[0].1.height < plot.w * plot.h / 2,
        "{:?} against the plot's {plot:?}",
        p[0].1
    );
    // A move that changes nothing draws nothing.
    mv(&mut s, (x, y));
    assert!(presents(&mut s.h).is_empty());
    // One far off presents both places, still inside the union.
    let (x, y) = sample_at(&mut s, "a", 16);
    mv(&mut s, (x, y));
    let later = bounds(&mut s).unwrap();
    let union = die_fig::raster::to_physical(after.union(&later), 2.0, w, h);
    let p = presents(&mut s.h);
    assert!(
        p.iter().all(|(_, r)| within(*r, union.into())),
        "{p:?} within {union:?}"
    );
}

#[test]
fn the_series_nearest_the_pointer_comes_forward_and_none_far_from_both() {
    let mut s = setup();
    let (x, ya) = sample_at(&mut s, "a", 3);
    mv(&mut s, (x, ya + 2.0));
    let st = state(&mut s);
    let near = st.borrow().panes[&s.win].near.clone();
    assert_eq!(near, Some(die_fig::data::SeriesKey::new("a", 0)));
    let em = st.borrow().panes[&s.win].overlay().emphasis;
    assert_eq!(em, near, "the overlay brings it forward");
    let (_, yb) = sample_at(&mut s, "b", 3);
    assert!(
        (yb - ya).abs() > 3.0 * NEAREST_SERIES,
        "the two lines apart at x = 3"
    );
    mv(&mut s, (x, (ya + yb) / 2.0));
    assert_eq!(
        st.borrow().panes[&s.win].near,
        None,
        "past NEAREST_SERIES of both"
    );
    assert!(hover(&mut s).is_some(), "the crosshair still snaps there");
}

#[test]
fn the_pointer_leaving_clears_the_hover_and_its_card() {
    let mut s = setup();
    let (_, over) = buffers(&mut s);
    let (x, y) = sample_at(&mut s, "b", 8);
    mv(&mut s, (x, y));
    assert!(hover(&mut s).is_some());
    presents(&mut s.h);
    let errors = s.h.send(Input::PointerLeft);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(hover(&mut s), None);
    assert_eq!(bounds(&mut s), None, "nothing left drawn");
    let p = presents(&mut s.h);
    assert!(
        p.iter().all(|(id, _)| *id == over) && !p.is_empty(),
        "the card cleared: {p:?}"
    );
    let (w, h, px) = s.h.pixels(over).unwrap();
    assert!(px.chunks_exact(4).all(|p| p[3] == 0), "{w}×{h} all clear");
}

#[test]
fn a_click_puts_the_cursor_on_the_nearest_sample_and_is_no_undo_step() {
    let mut s = setup();
    let (x, y) = sample_at(&mut s, "b", 7);
    click(&mut s, (x + 1.0, y + 1.0));
    assert_eq!(cursor(&mut s), Some(point("b", 7)));
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), 0);
    // The cursor stays when the pointer goes; its card shows then.
    s.h.send(Input::PointerLeft);
    assert_eq!(cursor(&mut s), Some(point("b", 7)));
    assert!(bounds(&mut s).is_some(), "the cursor's card");
}

#[test]
fn ctrl_click_is_a_click() {
    let mut s = setup();
    let (x, y) = sample_at(&mut s, "a", 2);
    click_with(&mut s, (x, y), CTRL);
    assert_eq!(cursor(&mut s), Some(point("a", 2)));
}

#[test]
fn shift_click_sets_the_reference_moves_it_and_clears_it_on_itself() {
    let mut s = setup();
    let (x, y) = sample_at(&mut s, "a", 4);
    click_with(&mut s, (x, y), SHIFT);
    assert_eq!(view(&mut s).reference, Some(point("a", 4)));
    assert_eq!(cursor(&mut s), None, "the cursor isn't moved");
    let (x2, y2) = sample_at(&mut s, "a", 9);
    click_with(&mut s, (x2, y2), SHIFT);
    assert_eq!(
        view(&mut s).reference,
        Some(point("a", 9)),
        "two points, never three"
    );
    click_with(&mut s, (x2, y2), SHIFT);
    assert_eq!(view(&mut s).reference, None);
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), 0, "no undo step");
}

#[test]
fn shift_clicking_the_references_moment_near_another_line_clears_it() {
    let mut s = setup();
    let (x, y) = sample_at(&mut s, "a", 4);
    click_with(&mut s, (x, y), SHIFT);
    assert_eq!(view(&mut s).reference, Some(point("a", 4)));
    // The same x, by b: one rule across both, so this is A again.
    let (xb, yb) = sample_at(&mut s, "b", 4);
    click_with(&mut s, (xb, yb), SHIFT);
    assert_eq!(view(&mut s).reference, None);
}

#[test]
fn a_quick_second_click_into_a_window_just_focused_is_a_click() {
    let mut s = setup();
    let other =
        s.h.show(s.src, die_core::ShowIn::Split(die_core::Side::Right))
            .unwrap()
            .expect("a window");
    s.h.focus(other);
    s.h.run_until_idle();
    let (x, y) = sample_at(&mut s, "a", 6);
    click(&mut s, (x, y));
    assert_eq!(s.h.focused(), Some(s.win), "focused");
    assert_eq!(cursor(&mut s), None, "the focusing press only focuses");
    s.h.advance(Duration::from_millis(60));
    click(&mut s, (x, y));
    assert_eq!(
        cursor(&mut s),
        Some(point("a", 6)),
        "the next, 60 ms on, is a click"
    );
    // A double click into a window not focused: its second press resets.
    keys(&mut s.h, "++++");
    settle(&mut s);
    assert!(view(&mut s).x.is_some());
    s.h.focus(other);
    s.h.run_until_idle();
    let p = mid(plot(&mut s));
    click(&mut s, p);
    s.h.advance(Duration::from_millis(100));
    pointer(
        &mut s,
        PointerKind::Press(Button::Left),
        p,
        Mods::default(),
        2,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Left),
        p,
        Mods::default(),
        2,
    );
    settle(&mut s);
    assert_eq!(view(&mut s).x, None, "reset");
}

#[test]
fn the_press_that_focuses_the_window_only_focuses_it() {
    let mut s = setup();
    // The app activated from another: Focus comes with no press having brought it.
    s.h.send(Input::Focus(false));
    s.h.send(Input::Focus(true));
    let (x, y) = sample_at(&mut s, "a", 6);
    click(&mut s, (x, y));
    assert_eq!(cursor(&mut s), None, "the activating click only focuses");
    click(&mut s, (x, y));
    assert_eq!(cursor(&mut s), Some(point("a", 6)), "the next is a click");
    // Past the grace, a press after the focus came is a click.
    s.h.send(Input::Focus(false));
    s.h.send(Input::Focus(true));
    s.h.advance(Duration::from_millis(200));
    let (x, y) = sample_at(&mut s, "a", 8);
    click(&mut s, (x, y));
    assert_eq!(cursor(&mut s), Some(point("a", 8)));
    // Hover needs no focus.
    s.h.send(Input::Focus(false));
    let (x, y) = sample_at(&mut s, "b", 3);
    mv(&mut s, (x, y));
    assert!(hover(&mut s).is_some());
}

#[test]
fn a_drag_brushes_with_its_preview_in_the_overlay_and_the_release_zooms_one_undo_step() {
    let mut s = setup();
    let (base, over) = buffers(&mut s);
    let (x0, y) = sample_at(&mut s, "a", 5);
    let (x1, _) = sample_at(&mut s, "a", 15);
    press(&mut s, (x0, y));
    presents(&mut s.h);
    drag(&mut s, (x0 + CLICK_SLOP + 1.0, y));
    drag(&mut s, ((x0 + x1) / 2.0, y));
    let st = state(&mut s);
    assert!(
        st.borrow().panes[&s.win].overlay().brush.is_some(),
        "the preview"
    );
    assert_eq!(hover(&mut s), None, "hover hides during a brush");
    let p = presents(&mut s.h);
    assert!(p.iter().all(|(id, _)| *id == over), "the base waits: {p:?}");
    drag(&mut s, (x1, y));
    release(&mut s, (x1, y));
    let v = view(&mut s);
    let (a, b) = v.x.expect("zoomed to the brush");
    assert!((a - 5.0).abs() < 0.2 && (b - 15.0).abs() < 0.2, "{a}..{b}");
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), 1);
    // It eases there, then stops: no timer left.
    assert!(st.borrow().panes[&s.win].easing());
    assert!(s.h.next_timer().is_some());
    settle(&mut s);
    assert!(!st.borrow().panes[&s.win].easing());
    assert_eq!(s.h.next_timer(), None);
    assert!(presents(&mut s.h).iter().any(|(id, _)| *id == base));
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None, "u takes the zoom back");
}

#[test]
fn a_press_released_inside_the_slop_is_a_click_not_a_brush() {
    let mut s = setup();
    let (x, y) = sample_at(&mut s, "a", 10);
    press(&mut s, (x, y));
    drag(&mut s, (x + CLICK_SLOP - 1.0, y));
    release(&mut s, (x + CLICK_SLOP - 1.0, y));
    assert_eq!(view(&mut s).x, None);
    assert_eq!(cursor(&mut s), Some(point("a", 10)));
}

#[test]
fn esc_mid_brush_drops_it_and_its_release_does_nothing() {
    let mut s = setup();
    let (x0, y) = sample_at(&mut s, "a", 3);
    let (x1, _) = sample_at(&mut s, "a", 13);
    press(&mut s, (x0, y));
    drag(&mut s, (x1, y));
    keys(&mut s.h, "<Esc>");
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].overlay().brush.is_none());
    release(&mut s, (x1, y));
    assert_eq!(view(&mut s).x, None);
    assert_eq!(cursor(&mut s), None);
}

#[test]
fn alt_drag_pans_live_and_is_one_undo_step_and_esc_takes_a_pan_back() {
    let mut s = zoomed();
    let before = view(&mut s).x.unwrap();
    let p = mid(plot(&mut s));
    gesture_with(
        &mut s,
        &[p, (p.0 + 10.0, p.1), (p.0 + 40.0, p.1)],
        ALT,
        Button::Left,
    );
    let after = view(&mut s).x.unwrap();
    assert!(
        after.0 < before.0,
        "the content follows the hand: {before:?} {after:?}"
    );
    let st = state(&mut s);
    assert!(!st.borrow().panes[&s.win].easing(), "a pan never eases");
    let steps = st.borrow().panes[&s.win].undo.undoable();
    // The middle button pans too; Esc mid-pan puts it back.
    pointer(
        &mut s,
        PointerKind::Press(Button::Middle),
        p,
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Middle),
        (p.0 + 30.0, p.1),
        Mods::default(),
        0,
    );
    assert_ne!(view(&mut s).x.unwrap(), after, "live");
    keys(&mut s.h, "<Esc>");
    assert_eq!(view(&mut s).x.unwrap(), after, "taken back");
    pointer(
        &mut s,
        PointerKind::Release(Button::Middle),
        (p.0 + 30.0, p.1),
        Mods::default(),
        1,
    );
    assert_eq!(view(&mut s).x.unwrap(), after);
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), steps);
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!(view(&mut s).x.unwrap(), before, "the pan was one step");
}

#[test]
fn a_key_mid_pan_ends_it_where_it_stands() {
    let mut s = zoomed();
    let p = mid(plot(&mut s));
    let steps = {
        let st = state(&mut s);
        st.borrow().panes[&s.win].undo.undoable()
    };
    pointer(
        &mut s,
        PointerKind::Press(Button::Middle),
        p,
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Middle),
        (p.0 + 30.0, p.1),
        Mods::default(),
        0,
    );
    let moved = view(&mut s).x;
    keys(&mut s.h, "X");
    assert_eq!(view(&mut s).x, moved, "kept");
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), steps + 1);
    pointer(
        &mut s,
        PointerKind::Release(Button::Middle),
        (p.0 + 60.0, p.1),
        Mods::default(),
        1,
    );
    assert_eq!(view(&mut s).x, moved, "its release does nothing more");
}

#[test]
fn a_drag_on_the_x_axis_pans_x() {
    let mut s = zoomed();
    let before = view(&mut s).x.unwrap();
    let axis = {
        let st = state(&mut s);
        st.borrow().panes[&s.win].layout.as_ref().unwrap().x_axis
    };
    let a = mid(axis);
    gesture(&mut s, &[a, (a.0 - 20.0, a.1), (a.0 - 40.0, a.1)]);
    let after = view(&mut s).x.unwrap();
    assert!(
        after.0 > before.0,
        "dragged left, later data: {before:?} {after:?}"
    );
    assert_eq!(cursor(&mut s), None, "no brush, no click");
}

#[test]
fn the_wheel_pans_by_whole_pixels_carrying_the_rest_and_a_burst_is_one_undo_step() {
    let mut s = zoomed();
    let before = view(&mut s).x.unwrap();
    let p = mid(plot(&mut s));
    let st = state(&mut s);
    let steps = st.borrow().panes[&s.win].undo.undoable();
    // A quarter cell is 2.5 physical px: 2 now, the half carried.
    scroll(&mut s, p, (0.25, 0.0), Mods::default());
    let one = view(&mut s).x.unwrap();
    assert!(one.0 > before.0, "scrolled right, later");
    scroll(&mut s, p, (0.25, 0.0), Mods::default());
    scroll(&mut s, p, (0.25, 0.0), Mods::default());
    scroll(&mut s, p, (0.25, 0.0), Mods::default());
    let four = view(&mut s).x.unwrap();
    // Ten physical px in all, so five logical: the carry lost nothing.
    let l = plot(&mut s);
    let span = before.1 - before.0;
    let moved = (four.0 - before.0) / span * f64::from(l.w);
    assert!((moved - 5.0).abs() < 0.01, "moved {moved} px");
    assert!(!st.borrow().panes[&s.win].easing(), "the wheel never eases");
    assert_eq!(
        st.borrow().panes[&s.win].undo.undoable(),
        steps,
        "not until the burst ends"
    );
    assert!(s.h.next_timer().is_some(), "the burst's end");
    s.h.advance(Duration::from_millis(250));
    assert_eq!(
        st.borrow().panes[&s.win].undo.undoable(),
        steps + 1,
        "one step"
    );
    assert_eq!(s.h.next_timer(), None, "nothing left running");
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!(view(&mut s).x.unwrap(), before);
}

#[test]
fn a_mouse_wheel_pans_time_with_up_and_down_too() {
    let mut s = zoomed();
    let before = view(&mut s).x.unwrap();
    let p = mid(plot(&mut s));
    scroll(&mut s, p, (0.0, 1.0), Mods::default());
    assert!(
        view(&mut s).x.unwrap().0 > before.0,
        "down moves later on an x-chart"
    );
}

#[test]
fn a_mouse_wheel_over_the_x_axis_pans_x() {
    let mut s = zoomed();
    let before = view(&mut s).x.unwrap();
    let axis = {
        let st = state(&mut s);
        st.borrow().panes[&s.win].layout.as_ref().unwrap().x_axis
    };
    scroll(&mut s, mid(axis), (0.0, 1.0), Mods::default());
    let after = view(&mut s).x.unwrap();
    assert!(
        after.0 > before.0,
        "down over the x axis: later, {before:?} {after:?}"
    );
    assert_eq!(view(&mut s).y, None, "x alone");
}

#[test]
fn once_y_is_set_by_hand_the_wheel_and_a_drag_pan_both_ways() {
    let mut s = setup();
    let l = plot(&mut s);
    // ⌘-drag: a box brush, which takes y off the fit.
    let (a, b) = (
        (l.x + l.w * 0.3, l.y + l.h * 0.3),
        (l.x + l.w * 0.7, l.y + l.h * 0.7),
    );
    gesture_with(&mut s, &[a, mid(l), b], CMD, Button::Left);
    settle(&mut s);
    let v = view(&mut s);
    let (x0, y0) = (v.x.expect("x zoomed"), v.y.expect("y by hand"));
    let p = mid(plot(&mut s));
    scroll(&mut s, p, (1.0, 1.0), Mods::default());
    let v = view(&mut s);
    assert!(v.x.unwrap().0 > x0.0, "right: later");
    assert!(v.y.unwrap().0 < y0.0, "down: lower");
    s.h.advance(Duration::from_millis(250));
    let (x1, y1) = (v.x.unwrap(), v.y.unwrap());
    gesture_with(
        &mut s,
        &[p, (p.0, p.1 + 10.0), (p.0, p.1 + 30.0)],
        ALT,
        Button::Left,
    );
    let v = view(&mut s);
    assert_eq!(v.x, Some(x1), "a drag straight down leaves x");
    assert!(
        v.y.unwrap().0 > y1.0,
        "and shows what's above: {y1:?} to {:?}",
        v.y
    );
}

#[test]
fn a_burst_keeps_the_meaning_of_its_first_event() {
    let mut s = zoomed();
    let before = view(&mut s).x.unwrap();
    let p = mid(plot(&mut s));
    scroll(&mut s, p, (1.0, 0.0), Mods::default());
    // Momentum with ⌘ gone down: still a pan, not a zoom.
    scroll(&mut s, p, (1.0, 0.0), CMD);
    let after = view(&mut s).x.unwrap();
    let span = |d: (f64, f64)| d.1 - d.0;
    assert!(
        (span(after) - span(before)).abs() < 1e-9,
        "the same span: panned"
    );
    s.h.advance(Duration::from_millis(250));
    // A new burst with ⌘: a zoom.
    scroll(&mut s, p, (0.0, -4.0), CMD);
    assert!(
        span(view(&mut s).x.unwrap()) < span(after) - 1e-9,
        "zoomed in"
    );
}

#[test]
fn cmd_wheel_zooms_about_the_pointer_keeping_its_value_under_it() {
    let mut s = setup();
    let l = plot(&mut s);
    let at = (l.x + l.w * 0.25, l.y + l.h / 2.0);
    let value = |s: &mut Setup| {
        let st = state(s);
        let w = s.win;
        let st = st.borrow();
        st.panes[&w].layout.as_ref().unwrap().x.invert(at.0)
    };
    let v0 = value(&mut s);
    scroll(&mut s, at, (0.0, -4.0), CMD);
    let (a, b) = view(&mut s).x.expect("zoomed");
    assert!(b - a < 20.0 / 1.2, "one ZOOM_STEP in: {a}..{b}");
    let st = state(&mut s);
    assert!(
        !st.borrow().panes[&s.win].easing(),
        "the wheel's zoom never eases"
    );
    let v1 = value(&mut s);
    assert!((v1 - v0).abs() < 1e-6, "{v0} stays under the pointer: {v1}");
    // Ctrl is an alias.
    s.h.advance(Duration::from_millis(250));
    scroll(&mut s, at, (0.0, -4.0), CTRL);
    let (c, d) = view(&mut s).x.unwrap();
    assert!(d - c < b - a);
}

#[test]
fn a_double_click_resets_the_view_eased_and_one_undo_step() {
    let mut s = zoomed();
    let st = state(&mut s);
    let steps = st.borrow().panes[&s.win].undo.undoable();
    let p = mid(plot(&mut s));
    double_click(&mut s, p);
    assert!(st.borrow().panes[&s.win].easing(), "eased");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None);
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), steps + 1);
    // Its first click put the cursor there: harmless, and no step.
    assert!(cursor(&mut s).is_some());
}

#[test]
fn a_double_click_on_the_x_axis_resets_x() {
    let mut s = zoomed();
    let axis = {
        let st = state(&mut s);
        st.borrow().panes[&s.win].layout.as_ref().unwrap().x_axis
    };
    double_click(&mut s, mid(axis));
    settle(&mut s);
    assert_eq!(view(&mut s).x, None);
}

/// The legend key's rectangle of a series.
fn legend(s: &mut Setup, name: &str) -> Rect {
    let st = state(s);
    let w = s.win;
    let st = st.borrow();
    let l = st.panes[&w].layout.as_ref().unwrap();
    l.legend.iter().find(|(k, _)| &*k.name == name).unwrap().1
}

#[test]
fn a_legend_click_toggles_alt_click_solos_and_a_double_click_solos_as_one_step() {
    let mut s = setup();
    let a = die_fig::data::SeriesKey::new("a", 0);
    let b = die_fig::data::SeriesKey::new("b", 0);
    let key_a = mid(legend(&mut s, "a"));
    click(&mut s, key_a);
    assert!(view(&mut s).hidden.contains(&a));
    click(&mut s, key_a);
    assert!(!view(&mut s).hidden.contains(&a), "and back");
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].undo.undoable(), 2, "a step each");
    click_with(&mut s, key_a, ALT);
    assert_eq!(view(&mut s).only, Some(a.clone()));
    click_with(&mut s, key_a, ALT);
    assert_eq!(
        view(&mut s).only,
        None,
        "⌥-click on the one alone shows all"
    );
    let steps = st.borrow().panes[&s.win].undo.undoable();
    let key_b = mid(legend(&mut s, "b"));
    double_click(&mut s, key_b);
    let v = view(&mut s);
    assert_eq!(v.only, Some(b.clone()));
    assert!(
        !v.hidden.contains(&b),
        "the first click's toggle taken back"
    );
    assert_eq!(
        st.borrow().panes[&s.win].undo.undoable(),
        steps + 1,
        "one step"
    );
    keys(&mut s.h, "u");
    let v = view(&mut s);
    assert_eq!((v.only, v.hidden.len()), (None, 0));
}

#[test]
fn hovering_a_legend_key_brings_its_series_forward_and_underlines_it() {
    let mut s = setup();
    let rect = legend(&mut s, "b");
    let key_b = mid(rect);
    mv(&mut s, key_b);
    let st = state(&mut s);
    let b = die_fig::data::SeriesKey::new("b", 0);
    assert_eq!(st.borrow().panes[&s.win].legend, Some(b.clone()));
    let over = st.borrow().panes[&s.win].overlay();
    assert_eq!(over.emphasis, Some(b));
    assert_eq!(over.pointer, Some(key_b), "the pointer, for the underline");
    assert_eq!(hover(&mut s), None, "no crosshair off the plot");
    // The overlay draws a line under the key's name.
    let scene = {
        let st = st.borrow();
        let p = &st.panes[&s.win];
        let model = st.model.as_ref().unwrap();
        let view = p.view.clone();
        die_fig::plot::overlay(
            model,
            p.layout.as_ref().unwrap(),
            &view,
            &over,
            &st.theme,
            &mut die_fig::scene::FixedMeasure,
        )
    };
    let underline = scene.items.iter().any(|i| match i {
        die_fig::scene::Item::Path {
            path,
            fill: None,
            stroke: Some(_),
        } => {
            path.verbs.len() == 2
                && path.verbs.iter().all(|v| match v {
                    die_fig::scene::Verb::Move(x, y) | die_fig::scene::Verb::Line(x, y) => {
                        rect.inflate(4.0).contains(*x, *y)
                    }
                    _ => false,
                })
        }
        _ => false,
    });
    assert!(underline, "{:?}", scene.items.len());
    s.h.send(Input::PointerLeft);
    assert_eq!(st.borrow().panes[&s.win].overlay().emphasis, None);
}

#[test]
fn the_right_button_space_and_ctrl_open_nothing() {
    let mut s = setup();
    let p = mid(plot(&mut s));
    presents(&mut s.h);
    pointer(
        &mut s,
        PointerKind::Press(Button::Right),
        p,
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Right),
        (p.0 + 30.0, p.1),
        Mods::default(),
        0,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Right),
        (p.0 + 30.0, p.1),
        Mods::default(),
        1,
    );
    assert_eq!(cursor(&mut s), None);
    assert_eq!(view(&mut s).x, None);
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].overlay().brush.is_none());
    // Space: nothing of the figure's, and no dial.
    let before = view(&mut s);
    let errors =
        s.h.send(Input::Key(KeyEvent::press(die_core::Key::Char(' '))));
    let _ = errors;
    assert_eq!(view(&mut s), before);
    assert_eq!(s.h.next_timer(), None);
}

#[test]
fn a_second_button_mid_brush_ends_nothing_and_starts_nothing() {
    let mut s = setup();
    let (x0, y) = sample_at(&mut s, "a", 4);
    let (x1, _) = sample_at(&mut s, "a", 14);
    press(&mut s, (x0, y));
    drag(&mut s, ((x0 + x1) / 2.0, y));
    pointer(
        &mut s,
        PointerKind::Press(Button::Right),
        (x1, y),
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Right),
        (x1, y),
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Press(Button::Middle),
        (x1, y),
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Middle),
        (x1, y),
        Mods::default(),
        1,
    );
    let st = state(&mut s);
    assert!(
        st.borrow().panes[&s.win].overlay().brush.is_some(),
        "the brush goes on"
    );
    drag(&mut s, (x1, y));
    release(&mut s, (x1, y));
    let (a, b) = view(&mut s).x.expect("the brush's zoom");
    assert!((a - 4.0).abs() < 0.2 && (b - 14.0).abs() < 0.2, "{a}..{b}");
}

#[test]
fn hover_snaps_again_when_the_view_moves_under_a_still_pointer() {
    let mut s = zoomed();
    let p = mid(plot(&mut s));
    mv(&mut s, p);
    let first = hover(&mut s).expect("hovering");
    // The wheel moves the data under a pointer that sends nothing more.
    for _ in 0..6 {
        scroll(&mut s, p, (2.0, 0.0), Mods::default());
    }
    let now = hover(&mut s).expect("still hovering");
    assert_ne!(now, first, "snapped again from where the pointer rests");
}

#[test]
fn a_key_that_moves_the_cursor_hides_the_hover_until_the_pointer_moves() {
    let mut s = setup();
    let (x, y) = sample_at(&mut s, "a", 10);
    mv(&mut s, (x, y));
    assert!(hover(&mut s).is_some());
    keys(&mut s.h, "l");
    assert_eq!(hover(&mut s), None, "the cursor's card now");
    assert!(cursor(&mut s).is_some());
    mv(&mut s, (x + 1.0, y));
    assert!(hover(&mut s).is_some());
}

#[test]
fn the_hover_card_is_drawn_on_the_cards_ground_and_goes_with_the_pointer() {
    let mut s = setup();
    let (_, over) = buffers(&mut s);
    let (x, y) = sample_at(&mut s, "a", 6);
    mv(&mut s, (x, y));
    let tooltip = state(&mut s).borrow().theme.tooltip;
    let (_, _, px) = s.h.pixels(over).unwrap();
    let ground = px
        .chunks_exact(4)
        .filter(|p| p[3] == 255 && p[..3] == [tooltip.0, tooltip.1, tooltip.2])
        .count();
    assert!(ground > 200, "the card's ground: {ground} px");
    s.h.send(Input::PointerLeft);
    let (_, _, px) = s.h.pixels(over).unwrap();
    assert!(px.chunks_exact(4).all(|p| p[3] == 0));
}

/// A bar chart of six services, one band each.
fn bars() -> Setup {
    setup_with(
        "services.csv",
        "service,rpm\napi,640\nworker,300\nbatch,120\nauth,410\nmail,90\nsearch,220\n",
    )
}

/// The middle of band `i`, halfway up the plot.
fn band_mid(s: &mut Setup, i: usize) -> (f32, f32) {
    let st = state(s);
    let w = s.win;
    let st = st.borrow();
    let l = st.panes[&w].layout.as_ref().unwrap();
    let (start, width) = l.x.band_of(i).expect("a band scale across");
    (start + width / 2.0, l.plot.y + l.plot.h / 2.0)
}

#[test]
fn hovering_a_bar_chart_targets_the_band_under_the_pointer() {
    let mut s = bars();
    let at = band_mid(&mut s, 2);
    mv(&mut s, at);
    assert!(
        matches!(hover(&mut s), Some(Cursor::Band { band: 2, .. })),
        "{:?}",
        hover(&mut s)
    );
    let four = band_mid(&mut s, 4);
    click(&mut s, four);
    assert!(matches!(cursor(&mut s), Some(Cursor::Band { band: 4, .. })));
}

/// A bar chart of forty bands, too narrow to show at full width: room to brush.
fn many_bars() -> Setup {
    let mut text = String::from("service,rpm\n");
    for i in 0..40 {
        text.push_str(&format!("s{i},{}\n", 100 + (i * 37) % 300));
    }
    setup_with("services.csv", &text)
}

#[test]
fn a_drag_across_bands_brushes_whole_bands_and_zooms_to_them() {
    let mut s = many_bars();
    let (a, b) = (band_mid(&mut s, 10), band_mid(&mut s, 13));
    gesture(&mut s, &[a, ((a.0 + b.0) / 2.0, a.1), b]);
    let (lo, hi) = view(&mut s).x.expect("zoomed to the bands");
    assert!(lo <= 10.0 && hi >= 14.0, "{lo}..{hi}");
    assert_eq!(
        (lo.fract(), hi.fract()),
        (0.0, 0.0),
        "whole bands: {lo}..{hi}"
    );
    // Six bands already at full width: a brush there does nothing (mouse-ux §4.2).
    let mut s = bars();
    let (a, b) = (band_mid(&mut s, 1), band_mid(&mut s, 3));
    gesture(&mut s, &[a, ((a.0 + b.0) / 2.0, a.1), b]);
    assert_eq!(view(&mut s).x, None);
}

#[test]
fn a_mouse_wheel_pans_bars_along_their_bands_with_up_and_down_too() {
    let mut s = many_bars();
    let (a, b) = (band_mid(&mut s, 2), band_mid(&mut s, 16));
    gesture(&mut s, &[a, ((a.0 + b.0) / 2.0, a.1), b]);
    settle(&mut s);
    let before = view(&mut s).x.expect("zoomed to the bands");
    let p = mid(plot(&mut s));
    scroll(&mut s, p, (0.0, 2.0), Mods::default());
    let down = view(&mut s).x.unwrap();
    assert!(down.0 > before.0, "down moves on: {before:?} to {down:?}");
    s.h.advance(Duration::from_millis(250));
    scroll(&mut s, p, (2.0, 0.0), Mods::default());
    assert!(view(&mut s).x.unwrap().0 > down.0, "and so does right");
}

/// Points over two unordered numbers.
fn scatter() -> Setup {
    let mut text = String::from("u,v\n");
    for (u, v) in [
        (5, 1),
        (1, 7),
        (3, 3),
        (8, 6),
        (2, 9),
        (7, 2),
        (4, 5),
        (9, 8),
    ] {
        text.push_str(&format!("{u},{v}\n"));
    }
    setup_with("points.csv", &text)
}

#[test]
fn hovering_a_scatter_finds_the_nearest_point_within_reach() {
    let mut s = scatter();
    let name = {
        let st = state(&mut s);
        let st = st.borrow();
        st.model.as_ref().unwrap().series[0].key.name.to_string()
    };
    let (x, y) = sample_at(&mut s, &name, 3);
    mv(&mut s, (x + 3.0, y - 3.0));
    assert!(
        matches!(hover(&mut s), Some(Cursor::Point { row: 3, .. })),
        "{:?}",
        hover(&mut s)
    );
    // Far from every point: nothing.
    let l = plot(&mut s);
    mv(&mut s, (l.x + 2.0, l.y + 2.0));
    assert_eq!(hover(&mut s), None);
}

#[test]
fn a_drag_on_a_scatter_is_a_box_brush_zooming_both_axes() {
    let mut s = scatter();
    let l = plot(&mut s);
    let (a, b) = (
        (l.x + l.w * 0.2, l.y + l.h * 0.2),
        (l.x + l.w * 0.6, l.y + l.h * 0.7),
    );
    gesture(&mut s, &[a, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), b]);
    let v = view(&mut s);
    assert!(v.x.is_some() && v.y.is_some(), "{v:?}");
}

#[test]
fn shift_holds_a_box_brush_to_its_larger_side() {
    let mut s = scatter();
    let l = plot(&mut s);
    let a = (l.x + l.w * 0.2, l.y + l.h * 0.4);
    let b = (a.0 + l.w * 0.5, a.1 + 25.0);
    gesture_with(
        &mut s,
        &[a, ((a.0 + b.0) / 2.0, a.1 + 12.0), b],
        SHIFT,
        Button::Left,
    );
    let v = view(&mut s);
    assert!(v.x.is_some(), "across: {v:?}");
    assert_eq!(v.y, None, "and only across");
    // Up and down, the other way.
    settle(&mut s);
    keys(&mut s.h, "0");
    settle(&mut s);
    let a = (l.x + l.w * 0.5, l.y + l.h * 0.1);
    let b = (a.0 + 20.0, a.1 + l.h * 0.6);
    pointer(&mut s, PointerKind::Press(Button::Left), a, SHIFT, 1);
    pointer(&mut s, PointerKind::Drag(Button::Left), b, SHIFT, 0);
    let st = state(&mut s);
    let shown = st.borrow().panes[&s.win].overlay().brush.map(|b| b.0);
    assert_eq!(
        shown,
        Some(die_fig::interact::BrushKind::Y),
        "drawn as it applies"
    );
    pointer(&mut s, PointerKind::Release(Button::Left), b, SHIFT, 1);
    let v = view(&mut s);
    assert!(v.y.is_some() && v.x.is_none(), "{v:?}");
}

#[test]
fn an_axis_strip_lights_under_the_pointer_and_a_category_label_targets_its_band() {
    let mut s = setup();
    let axis = {
        let st = state(&mut s);
        st.borrow().panes[&s.win].layout.as_ref().unwrap().y_axis
    };
    mv(&mut s, mid(axis));
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].lit_strip(), Some(axis));
    assert!(bounds(&mut s).is_some(), "drawn");
    s.h.send(Input::PointerLeft);
    assert_eq!(st.borrow().panes[&s.win].lit_strip(), None);
    assert_eq!(bounds(&mut s), None);
    // On bars, a category's label is its band.
    let mut s = bars();
    let x_axis = {
        let st = state(&mut s);
        st.borrow().panes[&s.win].layout.as_ref().unwrap().x_axis
    };
    let at = (band_mid(&mut s, 2).0, x_axis.y + x_axis.h / 2.0);
    mv(&mut s, at);
    assert!(
        matches!(
            hover(&mut s),
            Some(Cursor::Band {
                band: 2,
                series: None
            })
        ),
        "{:?}",
        hover(&mut s)
    );
    click(&mut s, at);
    assert!(
        matches!(cursor(&mut s), Some(Cursor::Band { band: 2, .. })),
        "{:?}",
        cursor(&mut s)
    );
}

#[test]
fn the_cards_side_lasts_across_a_redraw_of_the_base() {
    let mut s = zoomed();
    let st = state(&mut s);
    let cell = st.borrow().panes[&s.win]
        .layout
        .as_ref()
        .unwrap()
        .cell_width;
    assert_eq!(
        cell,
        CELL.width as f32 / CELL.scale as f32,
        "a cell is the hysteresis"
    );
    // Near the right edge the card goes left of the crosshair.
    let l = plot(&mut s);
    mv(&mut s, (l.right() - 4.0, l.y + l.h / 2.0));
    assert_eq!(st.borrow().panes[&s.win].card_flipped(), Some(true));
    // The base drawn again, for live data past the view: the side it was put on lasts.
    s.h.send(Input::PointerLeft);
    let drawn = st.borrow().panes[&s.win].base_gen;
    let t = s.h.text_mut(s.src).unwrap();
    let end = t.len();
    t.tx(die_core::Origin::User)
        .insert(end, "21,21,-1\n")
        .commit()
        .unwrap();
    s.h.advance(Duration::from_millis(200));
    assert!(st.borrow().panes[&s.win].base_gen > drawn, "drawn again");
    assert_eq!(st.borrow().panes[&s.win].card_flipped(), Some(true));
}

#[test]
fn a_hand_trembling_over_a_dense_series_keeps_it_forward() {
    // Ten thousand samples whose values jump about a band, over a flat line far below.
    let mut text = String::from("x,a,b\n");
    let mut seed: u64 = 7;
    for i in 0..10_000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let noise = (seed >> 33) as f64 / f64::from(u32::MAX >> 1);
        text.push_str(&format!("{i},{:.3},0\n", 100.0 + 40.0 * noise));
    }
    let mut s = setup_with("dense.csv", &text);
    let st = state(&mut s);
    let a = die_fig::data::SeriesKey::new("a", 0);
    let l = plot(&mut s);
    let y100 = st.borrow().panes[&s.win]
        .layout
        .as_ref()
        .unwrap()
        .y
        .map(100.0);
    // Just under the band, where a single sample's distance comes and goes past 24 px.
    let (mut x, y) = (l.x + l.w * 0.4, y100 + 16.0);
    for i in 0..200 {
        let at = l.x + l.w * 0.4 + i as f32 * 0.3;
        mv(&mut s, (at, y));
        if st.borrow().panes[&s.win].near.is_some() {
            x = at;
            break;
        }
    }
    assert_eq!(
        st.borrow().panes[&s.win].near,
        Some(a.clone()),
        "brought forward"
    );
    let mut changes = 0;
    let mut last = Some(a.clone());
    for i in 0..60 {
        let dx = ((i * 13) % 7) as f32 * 0.5 - 1.5;
        let dy = ((i * 5) % 5) as f32 * 0.5 - 1.0;
        mv(&mut s, (x + dx, y + dy));
        let near = st.borrow().panes[&s.win].near.clone();
        if near != last {
            changes += 1;
            last = near;
        }
    }
    assert_eq!(changes, 0, "steady under a trembling hand");
    // Well away from both, it lets go.
    mv(&mut s, (x, (y100 + l.bottom()) / 2.0));
    assert_eq!(st.borrow().panes[&s.win].near, None);
}

#[test]
fn a_resize_that_doesnt_grow_the_buffers_leaves_nothing_of_the_overlay_behind() {
    for how in ["narrower", "\\z", "<C-w>s"] {
        let mut s = setup();
        let (_, over) = buffers(&mut s);
        let (x, y) = sample_at(&mut s, "a", 12);
        mv(&mut s, (x, y));
        keys(&mut s.h, "gg15l");
        mv(&mut s, (x, y));
        assert!(bounds(&mut s).is_some());
        match how {
            "narrower" => {
                s.h.set_window_size(80, 30);
                s.h.run_until_idle();
            }
            k => keys(&mut s.h, k),
        }
        // Gone: the hover, and the cursor's crosshair, card and all.
        s.h.send(Input::PointerLeft);
        let st = state(&mut s);
        st.borrow_mut().panes.get_mut(&s.win).unwrap().cursor = None;
        keys(&mut s.h, "X");
        assert_eq!(bounds(&mut s), None, "{how}: nothing to draw");
        let (w, h, px) = s.h.pixels(over).unwrap();
        let left = px.chunks_exact(4).filter(|p| p[3] != 0).count();
        assert_eq!(left, 0, "{how}: {left} px of {w}×{h} left behind");
    }
}

#[test]
fn a_size_change_drops_the_hover_and_its_card_until_the_pointer_moves_and_keeps_the_cursor() {
    let mut s = setup();
    keys(&mut s.h, "l");
    let kept = cursor(&mut s);
    assert!(kept.is_some());
    let at = sample_at(&mut s, "a", 10);
    mv(&mut s, at);
    assert!(hover(&mut s).is_some());
    // The window narrows: what the pointer was over is somewhere else now.
    keys(&mut s.h, "<C-w>v");
    s.h.scene();
    assert_eq!(hover(&mut s), None, "the hover goes");
    let st = state(&mut s);
    let o = st.borrow().panes[&s.win].overlay();
    assert_eq!(o.hover, None);
    assert_eq!(o.pointer, None, "no card");
    drop(st);
    assert_eq!(cursor(&mut s), kept, "the keyboard's cursor stays");
    // The next move brings it back.
    let at = sample_at(&mut s, "a", 10);
    mv(&mut s, at);
    assert!(hover(&mut s).is_some());
}

/// `n` bars a minute apart: a day of minutes at 1440.
fn bars_over_time(n: usize) -> String {
    let rows: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "{{\"time\": {}, \"v\": {}}}",
                1_763_700_000 + i * 60,
                (i * 37 % 11) + 1
            )
        })
        .collect();
    format!(
        "{{\"type\": \"bar\", \"data\": {{\"values\": [{}]}}, \"x\": {{\"field\": \"time\", \"type\": \"time\", \"unit\": \"s\"}}, \"y\": \"v\"}}\n",
        rows.join(",")
    )
}

#[test]
#[ignore = "speed"]
fn hovering_two_thousand_bars_costs_a_few_ms_a_move() {
    let mut s = setup_with("bars.json", &bars_over_time(2000));
    let bars = state(&mut s).borrow().panes[&s.win]
        .layout
        .as_ref()
        .map_or(0, |l| l.bars.len());
    assert!(bars > 1000, "{bars} bars drawn");
    let p = plot(&mut s);
    // Low in the plot, where the card's first place covers bars and it looks for another.
    let moves = 20;
    let t0 = std::time::Instant::now();
    for i in 0..moves {
        let x = p.x + p.w * (0.2 + 0.03 * i as f32);
        mv(&mut s, (x, p.y + p.h * 0.9));
    }
    let each = t0.elapsed() / moves;
    eprintln!("{bars} bars: {each:?} a hover move");
    assert!(hover(&mut s).is_some());
    assert!(each < Duration::from_millis(8), "{each:?} a move");
}
