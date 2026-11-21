//! Following the source (contract-p2 §4): an edit is charted once the buffer is quiet; a data
//! file is polled while shown and its buffer followed when open; no timer runs when no figure
//! shows; a view following live data slides to the newest. What a failed parse shows (the error
//! figure, the stale badge and the log) is `tests/errors.rs`'s.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{BufferId, Origin, ShowIn};
use die_fig::Fig;
use die_fig::interact::{Cursor, Setting};
use std::time::Duration;

/// Append `text` to a buffer (the source when None), as a user's edit.
fn append(s: &mut Setup, buf: Option<BufferId>, text: &str) {
    let buf = buf.unwrap_or(s.src);
    let t = s.h.text_mut(buf).unwrap();
    let end = t.len();
    t.tx(Origin::User).insert(end, text).commit().unwrap();
    s.h.run_until_idle();
}

/// Replace a buffer's text (the source's when None).
fn replace(s: &mut Setup, buf: Option<BufferId>, text: &str) {
    let buf = buf.unwrap_or(s.src);
    let t = s.h.text_mut(buf).unwrap();
    let end = t.len();
    t.tx(Origin::User).replace(0..end, text).commit().unwrap();
    s.h.run_until_idle();
}

fn rows(s: &mut Setup) -> usize {
    let st = state(s);
    let st = st.borrow();
    st.model.as_ref().expect("a model").series[0].xs.len()
}

#[test]
fn an_edit_is_charted_once_the_source_is_quiet() {
    let mut s = setup();
    assert_eq!(rows(&mut s), 21);
    let builds = state(&mut s).borrow().builds;
    append(&mut s, None, "21,21,-1\n");
    assert_eq!(
        rows(&mut s),
        21,
        "not yet: the buffer must be quiet a while"
    );
    assert!(s.h.next_timer().is_some(), "the quiet time's timer");
    append(&mut s, None, "22,22,-2\n");
    s.h.advance(Duration::from_millis(100));
    assert_eq!(rows(&mut s), 21, "the second edit started the quiet over");
    s.h.advance(Duration::from_millis(100));
    s.h.run_until_idle();
    assert_eq!(rows(&mut s), 23);
    assert_eq!(
        state(&mut s).borrow().builds,
        builds + 1,
        "two edits, one build"
    );
    assert_eq!(s.h.next_timer(), None, "nothing left running");
}

/// A spec in a directory of its own reading `rates.csv` beside it.
fn spec_reading_a_file(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let data = dir.path().join("rates.csv");
    std::fs::write(&data, crossing()).unwrap();
    let spec = dir.path().join("rates.fig.json");
    std::fs::write(
        &spec,
        r#"{"type": "line", "data": "rates.csv", "x": "x", "y": "a"}"#,
    )
    .unwrap();
    (spec, data)
}

#[test]
fn a_data_file_is_polled_while_the_figure_shows() {
    let dir = TempDir::new("poll");
    let (spec, data) = spec_reading_a_file(&dir);
    let mut h = harness();
    let src = h.open_file(&spec).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    // A .fig.json opens as its figure.
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src, "the figure in the window");
    let mut s = Setup { h, src, buf, win };
    let st = state(&mut s);
    assert!(st.borrow().model.is_some(), "{:?}", st.borrow().badge());
    assert_eq!(rows(&mut s), 21);
    assert!(st.borrow().polling(), "the file's poll");
    // Changed on disk: read again at the next poll.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&data, "x,a,b\n0,1,2\n1,2,3\n").unwrap();
    s.h.advance(Duration::from_secs(1));
    s.h.run_until_idle();
    assert_eq!(rows(&mut s), 2);
    // Away from the figure: no poll.
    keys(&mut s.h, "<C-^>");
    assert_ne!(s.h.window_buffer(s.win), Some(s.buf));
    assert!(!st.borrow().polling(), "no poll while no window shows it");
    // Back: polled again.
    s.h.show(s.buf, ShowIn::Current).unwrap();
    s.h.run_until_idle();
    keys(&mut s.h, "l");
    assert!(st.borrow().polling());
}

#[test]
fn a_data_file_open_in_a_buffer_follows_its_edits_instead() {
    let dir = TempDir::new("follow");
    let (spec, data) = spec_reading_a_file(&dir);
    let mut h = harness();
    let file = h.open_file(&data).unwrap();
    let src = h.open_file(&spec).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    // A .fig.json opens as its figure.
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup { h, src, buf, win };
    assert_eq!(rows(&mut s), 21);
    keys(&mut s.h, "l");
    let st = state(&mut s);
    assert!(
        !st.borrow().polling(),
        "read from its buffer: {:?}",
        st.borrow().files()
    );
    assert_eq!(st.borrow().followed.len(), 1);
    append(&mut s, Some(file), "21,21,-1\n");
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    assert_eq!(rows(&mut s), 22, "the buffer's edit, unsaved");
}

#[test]
fn a_figure_no_window_shows_charts_its_source_when_shown_again() {
    let mut s = setup();
    keys(&mut s.h, "<C-^>");
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    keys(&mut s.h, "Go21,21,-1<Esc>");
    s.h.advance(Duration::from_millis(500));
    assert_eq!(s.h.next_timer(), None, "nothing while it isn't shown");
    assert_eq!(rows(&mut s), 21);
    s.h.show(s.buf, ShowIn::Current).unwrap();
    s.h.run_until_idle();
    assert_eq!(rows(&mut s), 22, "built when shown");
}

#[test]
fn a_view_following_live_data_slides_to_the_newest() {
    let mut s = setup();
    keys(&mut s.h, "++++");
    s.h.advance(Duration::from_millis(400));
    keys(&mut s.h, "G");
    s.h.advance(Duration::from_millis(400));
    let v = view(&mut s);
    assert!(v.follow, "G follows: {v:?}");
    let (_, b) = v.x.unwrap();
    assert!((b - 20.0).abs() < 1e-9, "the right edge on the newest: {b}");
    assert!(status(&mut s).contains("following"));
    append(&mut s, None, "21,21,-1\n22,22,-2\n");
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    s.h.advance(Duration::from_millis(400));
    let (_, b) = view(&mut s).x.unwrap();
    assert!((b - 22.0).abs() < 1e-9, "slid to the newest: {b}");
    // A pan back into the past stops following.
    let p = mid(plot(&mut s));
    gesture_with(
        &mut s,
        &[p, (p.0 + 30.0, p.1), (p.0 + 60.0, p.1)],
        ALT,
        die_core::Button::Left,
    );
    assert!(!view(&mut s).follow);
    append(&mut s, None, "23,23,-3\n");
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    let (_, b) = view(&mut s).x.unwrap();
    assert!(b < 22.0, "it stays where it was put: {b}");
}

#[test]
fn a_setting_turned_while_a_build_runs_loses_neither() {
    let mut s = setup();
    append(&mut s, None, "21,21,-1\n22,22,-2\n");
    let (buf, win) = (s.buf, s.win);
    // The quiet time's build and s's answer in one turn: the setting waits for the build.
    s.h.with_ext(|f: &mut Fig, ctx| {
        let st = f.figure(buf).unwrap();
        let mut st = st.borrow_mut();
        st.build(ctx);
        st.flip(ctx, win, Setting::Grid);
    })
    .unwrap();
    s.h.run_until_idle();
    s.h.advance(Duration::from_secs(2));
    let st = state(&mut s);
    assert_eq!(rows(&mut s), 23, "the edit charted");
    assert_eq!(st.borrow().toggles.grid, Some(false));
    let grid = |s: &mut Setup| state(s).borrow().model.as_ref().unwrap().chart.view.grid;
    assert!(!grid(&mut s), "and the setting with it");
    assert_eq!(s.h.next_timer(), None, "nothing behind");
    // An undo across the setting while a build runs: the same.
    append(&mut s, None, "23,23,-3\n");
    s.h.with_ext(|f: &mut Fig, ctx| {
        let st = f.figure(buf).unwrap();
        let mut st = st.borrow_mut();
        st.build(ctx);
        assert!(st.undo(ctx, win, false));
    })
    .unwrap();
    s.h.run_until_idle();
    s.h.advance(Duration::from_secs(2));
    assert_eq!(rows(&mut s), 24);
    assert_eq!(st.borrow().toggles.grid, None);
    assert!(grid(&mut s), "the grid back");
}

#[test]
fn a_brush_holds_the_view_still_while_live_data_arrives() {
    let mut s = setup();
    let (x0, y) = sample_at(&mut s, "a", 5);
    let (x1, _) = sample_at(&mut s, "a", 15);
    press(&mut s, (x0, y));
    drag(&mut s, ((x0 + x1) / 2.0, y));
    // Rows 21 to 40 arrive mid-drag, charted once the source is quiet.
    let more: String = (21..=40).map(|i| format!("{i},{i},{}\n", 20 - i)).collect();
    append(&mut s, None, &more);
    s.h.advance(Duration::from_millis(200));
    assert_eq!(rows(&mut s), 41, "the data arrived");
    let (still, _) = sample_at(&mut s, "a", 15);
    assert!(
        (still - x1).abs() < 0.5,
        "nothing slid under the hand: {x1} to {still}"
    );
    drag(&mut s, (x1, y));
    release(&mut s, (x1, y));
    let (a, b) = view(&mut s).x.expect("zoomed to the brush");
    assert!(
        (a - 5.0).abs() < 0.2 && (b - 15.0).abs() < 0.2,
        "what was brushed: {a}..{b}"
    );
    // A brush let go with nothing to zoom to lets the view fit the data again.
    s.h.advance(Duration::from_millis(400));
    keys(&mut s.h, "0");
    s.h.advance(Duration::from_millis(400));
    let p = mid(plot(&mut s));
    press(&mut s, p);
    drag(&mut s, (p.0 + 6.0, p.1));
    keys(&mut s.h, "<Esc>");
    release(&mut s, (p.0 + 6.0, p.1));
    s.h.run_until_idle();
    assert_eq!(view(&mut s).x, None);
    let (x40, _) = sample_at(&mut s, "a", 40);
    assert!(
        x40 <= plot(&mut s).right() + 0.5,
        "the base fits all 41 rows again"
    );
}

#[test]
fn a_bands_cursor_and_reference_go_when_the_data_shrinks_past_them() {
    let mut s = setup_with(
        "services.csv",
        "service,rpm\napi,640\nworker,300\nbatch,120\nauth,410\nmail,90\nsearch,220\n",
    );
    keys(&mut s.h, "lG");
    assert!(
        matches!(cursor(&mut s), Some(Cursor::Band { band: 5, .. })),
        "{:?}",
        cursor(&mut s)
    );
    keys(&mut s.h, "m");
    assert!(view(&mut s).reference.is_some());
    replace(&mut s, None, "service,rpm\napi,640\nworker,300\n");
    s.h.advance(Duration::from_millis(200));
    assert_eq!(cursor(&mut s), None, "band 5 is gone");
    assert_eq!(view(&mut s).reference, None);
}

#[test]
fn a_zoomed_view_the_data_has_left_fits_it_again() {
    let mut s = setup();
    keys(&mut s.h, "lG++++");
    s.h.advance(Duration::from_millis(400));
    let (a, _) = view(&mut s).x.expect("zoomed");
    assert!(a > 4.0, "{a}");
    replace(&mut s, None, "x,a,b\n0,0,4\n1,1,3\n2,2,2\n3,3,1\n4,4,0\n");
    s.h.advance(Duration::from_millis(200));
    assert_eq!(rows(&mut s), 5);
    assert_eq!(view(&mut s).x, None, "nothing was left to show");
}
