//! Following the source (contract-p2 §4): an edit is charted once the buffer is quiet; a parse
//! error keeps the last figure with a banner, `⏎` or a click on it goes to the error, and a fix
//! brings the figure back; a data file is polled while shown and its buffer followed when open;
//! no timer runs when no figure shows; a view following live data slides to the newest.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{BufferId, Origin, ShowIn};
use die_fig::Fig;
use die_fig::interact::{Cursor, Setting};
use die_fig::scene::Hit;
use std::time::Duration;

/// A point on the banner, found as the pointer finds it: it sits a few px in from the top.
fn on_banner(s: &mut Setup) -> (f32, f32) {
    let st = state(s);
    let st = st.borrow();
    let p = &st.panes[&s.win];
    (0..80)
        .map(|y| (40.0, y as f32))
        .find(|&at| p.hit(at) == Hit::Banner)
        .expect("a banner")
}

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

#[test]
fn a_parse_error_keeps_the_last_figure_with_a_banner_and_a_fix_brings_it_back() {
    let rows_json = r#"[{"x": 1, "y": 2},
 {"x": 2, "y": 4},
 {"x": 3, "y": 3}]
"#;
    let mut s = setup_with("data.json", rows_json);
    let st = state(&mut s);
    let built = st.borrow().model_gen;
    assert!(st.borrow().model.is_some(), "{:?}", st.borrow().error);
    // Break it on its second line.
    replace(
        &mut s,
        None,
        "[{\"x\": 1, \"y\": 2},\n {\"x\": 2 \"y\": 4}]\n",
    );
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    let banner = st.borrow().banner().expect("a banner");
    assert!(banner.contains("line 2"), "{banner}");
    assert_eq!(st.borrow().model_gen, built, "the last figure kept");
    assert!(st.borrow().model.is_some());
    assert!(status(&mut s).contains("line 2"), "{}", status(&mut s));
    // ⏎ goes to the error's line and column in the source.
    keys(&mut s.h, "<CR>");
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    let t = s.h.text(s.src).unwrap();
    let at = t.cursor(s.win).unwrap();
    assert_eq!(t.line_of(at), 1, "the second line");
    // Fixed: the banner goes and the figure follows.
    s.h.show(s.buf, ShowIn::Current).unwrap();
    s.h.run_until_idle();
    replace(
        &mut s,
        None,
        "[{\"x\": 1, \"y\": 2},\n {\"x\": 2, \"y\": 9}]\n",
    );
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    assert_eq!(st.borrow().banner(), None);
    assert!(st.borrow().model_gen > built);
}

#[test]
fn a_click_on_the_banner_goes_to_the_error() {
    let mut s = setup_with(
        "data.json",
        "[{\"x\": 1, \"y\": 2}, {\"x\": 2, \"y\": 3}]\n",
    );
    replace(&mut s, None, "[{\"x\": 1, \"y\": 2}, {\"x\" 2}]\n");
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    assert!(state(&mut s).borrow().banner().is_some());
    let at = on_banner(&mut s);
    click(&mut s, at);
    assert_eq!(s.h.window_buffer(s.win), Some(s.src), "to the source");
    let t = s.h.text(s.src).unwrap();
    let at = t.cursor(s.win).unwrap();
    assert!(at > 0, "at the error's column: {at}");
}

#[test]
fn a_click_on_the_banner_goes_to_the_error_with_no_chart_yet() {
    let mut s = setup_with("data.json", "[{\"x\": 1, \"y\": 2},\n {\"x\" 2}]\n");
    let st = state(&mut s);
    assert!(st.borrow().model.is_none());
    assert!(st.borrow().banner().is_some());
    drop(st);
    let at = on_banner(&mut s);
    click(&mut s, at);
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    let t = s.h.text(s.src).unwrap();
    assert_eq!(t.line_of(t.cursor(s.win).unwrap()), 1, "the error's line");
}

#[test]
fn an_empty_source_shows_why_in_place_of_a_chart() {
    let mut s = setup_with("data.csv", "x,a\n");
    let st = state(&mut s);
    assert!(st.borrow().model.is_none());
    let banner = st.borrow().banner().expect("why");
    assert!(banner.contains("nothing to draw"), "{banner}");
    // A row arrives: a chart.
    append(&mut s, None, "1,2\n2,3\n");
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    assert!(st.borrow().model.is_some(), "{:?}", st.borrow().banner());
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
    h.run_until_idle();
    h.run("fig-toggle", &die_core::Args::default()).unwrap();
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup { h, src, buf, win };
    let st = state(&mut s);
    assert!(st.borrow().model.is_some(), "{:?}", st.borrow().banner());
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
    h.run_until_idle();
    h.run("fig-toggle", &die_core::Args::default()).unwrap();
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
