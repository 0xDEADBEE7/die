//! Restarts (ruling 16), as `:rebuild` makes them, through the Config path: the session handed
//! over to a file, DIE gone, and a new DIE started on it, twice. Each figure comes back by its
//! source with its view (domains, hidden set, cursor), the one the focused window showed shown in
//! it; one no window showed waits to be built until it's shown. The reference and the undo
//! history don't come back.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, Config, Harness, ShowIn};
use die_fig::Fig;
use die_fig::data::SeriesKey;
use die_fig::interact::Cursor;
use die_vim::Vim;
use jiff::tz::TimeZone;
use std::time::Duration;

fn up(state: &TempDir) -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Fig::with_zone(TimeZone::UTC)),
    ];
    let config = Config {
        session: Some(state.path().to_path_buf()),
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    let lost: Vec<&String> = h
        .messages
        .iter()
        .filter(|m| m.contains("didn't come back") || m.contains("dropped"))
        .collect();
    assert!(lost.is_empty(), "{lost:?}");
    h
}

fn restart(h: Harness, state: &TempDir) -> Harness {
    let mut h = h;
    h.hand_over_session().unwrap();
    drop(h);
    up(state)
}

/// The figure the focused window shows, as a setup.
fn shown(h: Harness) -> Setup {
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    Setup {
        h,
        src: buf,
        buf,
        win,
    }
}

fn figures(h: &mut Harness) -> Vec<String> {
    let mut names: Vec<String> = h
        .with_ext(|f: &mut Fig, _| f.figures())
        .unwrap()
        .into_iter()
        .filter_map(|b| h.buffer_name(b).map(String::from))
        .collect();
    names.sort();
    names
}

#[test]
fn a_figure_comes_back_with_its_view_after_two_restarts() {
    let state_dir = TempDir::new("session-state");
    let dir = TempDir::new("session-data");
    let path = dir.path().join("rates.csv");
    std::fs::write(&path, crossing()).unwrap();
    let mut h = up(&state_dir);
    let src = h.open_file(&path).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    h.run_until_idle();
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    let mut s = shown(h);
    keys(&mut s.h, "++++");
    s.h.advance(Duration::from_millis(400));
    keys(&mut s.h, "gg15l");
    if cursor(&mut s)
        .is_some_and(|c| !matches!(c, Cursor::Point { ref series, .. } if &*series.name == "a"))
    {
        keys(&mut s.h, "k");
    }
    keys(&mut s.h, "jxk");
    keys(&mut s.h, "m");
    let want = view(&mut s);
    let at = cursor(&mut s);
    assert!(want.x.is_some() && want.hidden.contains(&SeriesKey::new("b", 0)));
    assert!(want.reference.is_some());
    let mut h = restart(s.h, &state_dir);
    assert_eq!(figures(&mut h), vec!["rates.csv *fig*"]);
    let mut s = shown(h);
    assert_eq!(
        s.h.buffer_name(s.buf),
        Some("rates.csv *fig*"),
        "shown again"
    );
    let st = state(&mut s);
    assert!(
        st.borrow().model.is_some(),
        "built once shown: {:?}",
        st.borrow().banner()
    );
    let got = view(&mut s);
    assert_eq!(got.x, want.x);
    assert_eq!(got.hidden, want.hidden);
    assert_eq!(cursor(&mut s), at);
    assert_eq!(got.reference, None, "the reference isn't kept");
    assert_eq!(
        st.borrow().panes[&s.win].undo.undoable(),
        0,
        "nor the history"
    );
    // And again.
    drop(st);
    let mut h = restart(s.h, &state_dir);
    assert_eq!(figures(&mut h), vec!["rates.csv *fig*"]);
    let mut s = shown(h);
    assert_eq!(view(&mut s).x, want.x);
    assert_eq!(cursor(&mut s), at);
}

#[test]
fn a_figure_of_a_file_comes_back_and_one_not_shown_waits_to_be_built() {
    let state_dir = TempDir::new("session-files");
    let dir = TempDir::new("session-files-data");
    let one = dir.path().join("one.csv");
    let two = dir.path().join("two.csv");
    std::fs::write(&one, crossing()).unwrap();
    std::fs::write(&two, crossing()).unwrap();
    let mut h = up(&state_dir);
    h.open_text("notes", "notes\n").unwrap();
    for p in [&one, &two] {
        let args = Args {
            text: Some(p.display().to_string()),
            ..Args::default()
        };
        h.run("fig", &args).unwrap();
        h.run_until_idle();
    }
    let mut h = restart(h, &state_dir);
    assert_eq!(figures(&mut h), vec!["one.csv *fig*", "two.csv *fig*"]);
    let mut s = shown(h);
    assert_eq!(
        s.h.buffer_name(s.buf),
        Some("two.csv *fig*"),
        "the one shown last"
    );
    assert!(state(&mut s).borrow().model.is_some());
    let other = s.h.find_buffer("one.csv *fig*").unwrap();
    let st =
        s.h.with_ext(|f: &mut Fig, _| f.figure(other))
            .unwrap()
            .unwrap();
    assert!(st.borrow().behind && st.borrow().builds == 0, "unbuilt");
    s.h.show(other, ShowIn::Current).unwrap();
    s.h.run_until_idle();
    assert!(st.borrow().model.is_some(), "built when shown");
}
