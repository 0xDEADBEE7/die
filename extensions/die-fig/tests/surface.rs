//! The surface: `\v` opens a figure in the window and goes back, the two buffers per window are
//! placed and drawn, nothing runs at rest, a window gone frees its buffers, the theme is
//! followed, `:fig FILE` and `:fig !cmd` and `fig-spec`.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, PixelSinkCall, ShowIn};
use die_fig::interact::View;

#[test]
fn toggle_shows_the_figure_in_the_window_and_goes_back() {
    let mut s = setup();
    assert_eq!(s.h.buffer_name(s.buf), Some("data.csv *fig*"));
    let st = state(&mut s);
    assert!(
        st.borrow().model.is_some(),
        "built: {:?}",
        st.borrow().error
    );
    // The base at z −2 and the overlay at z −1, each the area's size.
    let scene = s.h.scene();
    let placed: Vec<i8> = scene.placements.iter().map(|p| p.z).collect();
    assert_eq!(placed, vec![-2, -1], "{}", scene.snapshot());
    let (base, over) = buffers(&mut s);
    let (w, h, _) = s.h.pixels(base).unwrap();
    let g = s.h.surface_area(s.win).unwrap();
    assert_eq!((w, h), (g.cols as u32 * 10, g.rows as u32 * 20));
    assert_eq!(s.h.pixels(over).unwrap().0, w);
    // The base is opaque on the theme's ground; the overlay clear.
    let bg = st.borrow().theme.surface;
    assert_eq!(pixel(&s.h, base, (1, h - 2)), [bg.0, bg.1, bg.2, 255]);
    assert_eq!(pixel(&s.h, over, (1, h - 2))[3], 0);
    drop(st);
    // Back: the source in the window, and the figure goes.
    assert!(run(&mut s.h, "fig-toggle").is_empty());
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    assert!(!s.h.is_live(s.buf));
    assert_eq!(s.h.next_timer(), None, "nothing left running");
}

#[test]
fn nothing_runs_at_rest_and_the_surface_wants_every_sample() {
    let mut s = setup();
    assert_eq!(s.h.next_timer(), None);
    let (r0, c0) = origin(&mut s.h, s.win);
    assert!(
        s.h.fine_motion_at(r0 + 2, c0 + 2),
        "fine motion over the figure"
    );
    // The figure gives no text: no copy state, so the pointer stays the figure's.
    assert_eq!(s.h.state_of(s.buf), die_core::State::Normal);
}

#[test]
fn toggling_back_keeps_the_view_for_the_next_figure_of_the_source() {
    let mut s = setup();
    keys(&mut s.h, "+");
    s.h.advance(std::time::Duration::from_millis(400));
    let zoomed = view(&mut s);
    assert!(zoomed.x.is_some());
    run(&mut s.h, "fig-toggle");
    run(&mut s.h, "fig-toggle");
    s.buf = s.h.window_buffer(s.win).unwrap();
    assert_eq!(view(&mut s).x, zoomed.x);
}

#[test]
fn a_window_gone_frees_its_buffers_and_a_second_window_has_its_own() {
    let mut s = setup();
    s.h.fake_pixels().take_calls();
    keys(&mut s.h, "<C-w>v");
    let windows = s.h.window_list();
    assert_eq!(windows.len(), 2);
    let st = state(&mut s);
    assert_eq!(st.borrow().panes.len(), 2, "a pane each");
    let other = *windows.iter().find(|w| **w != s.win).unwrap();
    let created =
        s.h.fake_pixels()
            .take_calls()
            .iter()
            .filter(|c| matches!(c, PixelSinkCall::Create(..)))
            .count();
    assert_eq!(created, 2, "the new window's base and overlay");
    // Close one: its buffers go in the settle after the command (ruling 11).
    s.h.focus(other);
    keys(&mut s.h, "<C-w>c");
    assert_eq!(st.borrow().panes.len(), 1);
    let destroyed =
        s.h.fake_pixels()
            .take_calls()
            .iter()
            .filter(|c| matches!(c, PixelSinkCall::Destroy(..)))
            .count();
    assert_eq!(destroyed, 2);
}

#[test]
fn the_buffers_grow_and_never_shrink() {
    let mut s = setup();
    let (base, _) = buffers(&mut s);
    let (w0, h0, _) = s.h.pixels(base).unwrap();
    s.h.set_window_size(120, 40);
    s.h.run_until_idle();
    let (w1, h1, _) = s.h.pixels(base).unwrap();
    assert!(w1 > w0 && h1 > h0);
    s.h.set_window_size(60, 20);
    s.h.run_until_idle();
    assert_eq!(s.h.pixels(base).unwrap().0, w1, "never shrunk");
    let g = s.h.surface_area(s.win).unwrap();
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].cols, g.cols);
}

#[test]
fn a_theme_change_draws_the_base_again_in_its_colours() {
    let mut s = setup();
    let (base, _) = buffers(&mut s);
    let ansi = die_core::Theme::default().ansi;
    let paper = die_core::Theme::new("paper", ansi).face(
        "default",
        die_core::Style::new()
            .fg(die_core::Rgb::hex("#333333"))
            .bg(die_core::Rgb::hex("#fafafa")),
    );
    s.h.set_theme(paper);
    // Compared at the next event or command: a move.
    mv(&mut s, (2.0, 2.0));
    let (_, h, _) = s.h.pixels(base).unwrap();
    assert_eq!(pixel(&s.h, base, (1, h - 2)), [0xfa, 0xfa, 0xfa, 255]);
}

#[test]
fn fig_file_charts_a_file_no_buffer_visits_and_fig_spec_writes_it_as_a_spec() {
    let dir = TempDir::new("file");
    let path = dir.path().join("rates.csv");
    std::fs::write(&path, crossing()).unwrap();
    let mut h = harness();
    h.open_text("notes", "notes\n").unwrap();
    let args = Args {
        text: Some(path.display().to_string()),
        ..Args::default()
    };
    h.run("fig", &args).unwrap();
    h.run_until_idle();
    let w = h.focused().unwrap();
    let buf = h.window_buffer(w).unwrap();
    assert_eq!(h.buffer_name(buf), Some("rates.csv *fig*"));
    let mut s = Setup {
        h,
        src: buf,
        buf,
        win: w,
    };
    assert!(state(&mut s).borrow().model.is_some());
    // Polled while shown.
    assert!(s.h.next_timer().is_some(), "the file's poll");
    keys(&mut s.h, ":fig-spec<CR>");
    let spec = s.h.window_buffer(s.win).unwrap();
    assert_eq!(s.h.buffer_name(spec), Some("rates.fig.json"));
    let text = s.h.text(spec).unwrap().rope().to_string();
    assert!(text.contains("rates.csv"), "the data by its file: {text}");
    assert!(!text.contains("\"values\""), "{text}");
    // No window shows the figure now: no poll.
    assert_eq!(s.h.next_timer(), None);
    // It reads back as a chart.
    let back = die_fig::spec::parse(
        &text,
        "rates.fig.json",
        dir.path(),
        &die_fig::data::ReadOptions::new(jiff::tz::TimeZone::UTC),
    );
    assert!(back.is_ok(), "{back:?}");
}

#[test]
fn fig_bang_charts_what_a_command_prints() {
    let mut h = harness();
    h.open_text("notes", "notes\n").unwrap();
    let args = Args {
        text: Some("!printf 'x,y\\n1,2\\n2,4\\n3,3\\n'".into()),
        ..Args::default()
    };
    h.run("fig", &args).unwrap();
    h.run_until_idle();
    let w = h.focused().unwrap();
    let buf = h.window_buffer(w).unwrap();
    assert!(h.buffer_name(buf).unwrap().starts_with("*fig: !printf"));
    let mut s = Setup {
        h,
        src: buf,
        buf,
        win: w,
    };
    let st = state(&mut s);
    let st = st.borrow();
    assert!(st.model.is_some(), "{:?}", st.error);
    assert_eq!(st.model.as_ref().unwrap().keys().len(), 1);
}

#[test]
fn a_selection_charts_its_lines() {
    let mut h = harness();
    let text = format!("a log line\n{}trailing\n", crossing());
    let (src, w) = h.open_text("log.txt", &text).unwrap();
    // From the header down five lines, in visual line state.
    keys(&mut h, "jVjjjj");
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    let buf = h.window_buffer(w).unwrap();
    assert_eq!(h.buffer_name(buf), Some("*fig: selection*"));
    assert_eq!(h.state_of(src), die_core::State::Normal, "visual left");
    let mut s = Setup {
        h,
        src,
        buf,
        win: w,
    };
    let st = state(&mut s);
    let model = st.borrow().model.clone().expect("a model");
    assert_eq!(model.series[0].xs.len(), 4, "the four rows selected");
    // Back to the text it came from.
    drop(st);
    run(&mut s.h, "fig-toggle");
    assert_eq!(s.h.window_buffer(s.win), Some(src));
}

#[test]
fn the_status_line_names_the_figure() {
    let mut s = setup();
    let line = status(&mut s);
    assert!(line.contains("data.csv  2 series"), "{line}");
}

#[test]
fn showing_the_figure_again_in_a_window_starts_from_where_it_was() {
    let mut s = setup();
    keys(&mut s.h, "l");
    let c = cursor(&mut s);
    assert!(c.is_some());
    // Away by C-^, which leaves the figure alive, and back.
    keys(&mut s.h, "<C-^>");
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    assert!(s.h.is_live(s.buf));
    assert_eq!(s.h.next_timer(), None);
    s.h.show(s.buf, ShowIn::Current).unwrap();
    s.h.run_until_idle();
    assert_eq!(cursor(&mut s), c);
    assert_ne!(
        view(&mut s),
        View {
            follow: true,
            ..View::default()
        }
    );
}
