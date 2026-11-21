//! Following the source (contract-p2 §4): an edit is charted once the buffer is quiet; a parse
//! error keeps the last figure with a banner, `⏎` or a click on it goes to the error, and a fix
//! brings the figure back; a data file is polled while shown and its buffer followed when open;
//! no timer runs when no figure shows; a view following live data slides to the newest.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{BufferId, Origin, ShowIn};
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
    click(&mut s, (40.0, 6.0));
    assert_eq!(s.h.window_buffer(s.win), Some(s.src), "to the source");
    let t = s.h.text(s.src).unwrap();
    let at = t.cursor(s.win).unwrap();
    assert!(at > 0, "at the error's column: {at}");
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
