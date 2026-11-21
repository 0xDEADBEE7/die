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
fn hovering_a_legend_key_brings_its_series_forward() {
    let mut s = setup();
    let key_b = mid(legend(&mut s, "b"));
    mv(&mut s, key_b);
    let st = state(&mut s);
    let b = die_fig::data::SeriesKey::new("b", 0);
    assert_eq!(st.borrow().panes[&s.win].legend, Some(b.clone()));
    assert_eq!(st.borrow().panes[&s.win].overlay().emphasis, Some(b));
    assert_eq!(hover(&mut s), None, "no crosshair off the plot");
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
