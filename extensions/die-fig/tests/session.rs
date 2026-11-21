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
        st.borrow().badge()
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

#[test]
fn a_figure_of_a_command_comes_back_charted_from_what_it_printed_not_run_again() {
    let state_dir = TempDir::new("session-cmd");
    let dir = TempDir::new("session-cmd-data");
    let log = dir.path().join("ran.log");
    let ran = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let mut h = up(&state_dir);
    h.open_text("notes", "notes\n").unwrap();
    let cmd = format!(
        "!echo ran >> '{}'; printf 'x,y\\n1,2\\n2,3\\n3,1\\n'",
        log.display()
    );
    let args = Args {
        text: Some(cmd),
        ..Args::default()
    };
    h.run("fig", &args).unwrap();
    h.run_until_idle();
    assert_eq!(ran(), 1);
    let h = restart(h, &state_dir);
    let mut s = shown(h);
    let name = s.h.buffer_name(s.buf).unwrap_or_default().to_string();
    assert!(name.starts_with("*fig: !echo ran"), "shown again: {name}");
    let st = state(&mut s);
    let rows = st.borrow().model.as_ref().map(|m| m.series[0].xs.len());
    assert_eq!(
        rows,
        Some(3),
        "charted from what it printed: {:?}",
        st.borrow().badge()
    );
    assert_eq!(ran(), 1, "and not run again");
}

#[test]
fn the_session_keeps_the_view_of_the_window_the_focus_moved_to() {
    let mut s = setup();
    keys(&mut s.h, "<C-w>s");
    let other = s.h.focused().unwrap();
    assert_ne!(other, s.win);
    keys(&mut s.h, "++++");
    s.h.advance(Duration::from_millis(400));
    s.h.scene();
    // The figure's record, its x domain: a pair once zoomed, null at the fit.
    let x_of = |h: &mut Harness| {
        let json: String = h.session_json().split_whitespace().collect();
        let at = json.find("\"fig\":{").expect("the figure's record");
        let x = json[at..].find("\"x\":").expect("its x") + at + 4;
        json[x..x + 4].to_string()
    };
    assert_ne!(x_of(&mut s.h), "null", "the zoomed window's");
    // The focus back on the window that never zoomed: the session keeps its view.
    s.h.focus(s.win);
    s.h.scene();
    assert_eq!(x_of(&mut s.h), "null");
}
