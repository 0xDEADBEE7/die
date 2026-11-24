//! A gantt in the editor (the gantt's contract §3): a `.fig` gantt opening as its figure and
//! `\v` back to its text, the keys (j k, h l, ] [, w b, gg G, zoom and fit), hover with its card
//! and the pointer leaving, a click, a brush that zooms the time with undo, the wheel's two
//! meanings and ⌘-wheel, search and the detail panel with ⏎ on a row that names a task, `t` and
//! `s`, `x` `X` and the legend, following the source's edits, and the session across a restart.
//! Every plan pins its `today` (or has none), and times are in UTC, so nothing reads the clock.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Config, Harness, Input, Mods, ShowIn};
use die_fig::Fig;
use die_fig::data::SeriesKey;
use die_fig::detail::Item;
use die_fig::gantt::{GanttLayout, GanttModel};
use die_fig::interact::Cursor;
use die_fig::scene::{FixedMeasure, Hit, Item as Drawn, Rect};
use die_vim::Vim;
use jiff::tz::TimeZone;
use std::time::Duration;

/// A fixture's text, read at run time.
fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/data/gantt")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// `name` holding `text`, shown as its figure in a window `cols` × `rows` cells (5 × 10
/// logical px each): a gantt.
fn gantt_with(name: &str, text: &str, cols: usize, rows: usize) -> Setup {
    let mut h = harness();
    h.set_window_size(cols, rows);
    h.run_until_idle();
    let (src, win) = h.open_text(name, text).unwrap();
    h.run_until_idle();
    let errors = run(&mut h, "fig-toggle");
    assert!(errors.is_empty(), "{errors:?}");
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup { h, src, buf, win };
    let st = state(&mut s);
    assert!(st.borrow().is_gantt(), "a gantt: {:?}", st.borrow().error);
    s
}

/// release.fig: eleven tasks under three groups, three of them milestones, today pinned at
/// 25 Nov 2026; in a window 880 × 700, where every row fits.
fn release_plan() -> Setup {
    gantt_with("release.fig", &fixture("release.fig"), 176, 70)
}

/// Four tasks coloured by their owner, so the legend lists Maya, Ravi and Lena.
const OWNERS: &str = r#"{
  "type": "gantt", "title": "Owners", "today": "2026-11-10", "color": "owner",
  "tasks": [
    {"id": "a", "label": "Alpha", "start": "2026-11-02", "duration": "3d", "owner": "Maya"},
    {"id": "b", "label": "Beta", "after": "a", "duration": "4d", "owner": "Ravi"},
    {"id": "c", "label": "Gamma", "after": "b", "duration": "2d", "owner": "Maya"},
    {"id": "d", "label": "Delta", "start": "2026-11-05", "duration": "5d", "owner": "Lena"}
  ]
}"#;

fn owners() -> Setup {
    gantt_with("owners.fig", OWNERS, 176, 50)
}

/// Forty tasks, each after the last: more rows than any window here shows. No today line.
fn tall_spec() -> String {
    let tasks: Vec<String> = (0..40)
        .map(|i| {
            let place = match i {
                0 => r#""start": "2026-11-02""#.to_string(),
                _ => format!(r#""after": "t{}""#, i - 1),
            };
            format!(r#"{{"id": "t{i}", "label": "Task {i}", {place}, "duration": "2d"}}"#)
        })
        .collect();
    format!(
        r#"{{"type": "gantt", "title": "Tall", "today": false, "tasks": [{}]}}"#,
        tasks.join(",\n")
    )
}

/// Let every ease finish and every burst end.
fn settle(s: &mut Setup) {
    s.h.advance(Duration::from_millis(400));
}

fn task(key: &str) -> Cursor {
    Cursor::Task(key.into())
}

/// Ask the gantt's model and its layout as last drawn.
fn drawn<R>(s: &mut Setup, f: impl FnOnce(&GanttModel, &GanttLayout) -> R) -> R {
    let st = state(s);
    let st = st.borrow();
    let g = st.gantt.as_ref().expect("a gantt");
    let l = st.panes[&s.win].gantt.as_ref().expect("drawn");
    f(g, l)
}

/// A task's bar, as last drawn.
fn bar(s: &mut Setup, key: &str) -> Rect {
    drawn(s, |g, l| {
        let i = g.index_of(key).expect("the task");
        l.task(i).expect("it has a row").bar
    })
}

/// The time shown, as last drawn.
fn time(s: &mut Setup) -> (f64, f64) {
    drawn(s, |_, l| l.domain)
}

/// How far the rows are scrolled, as last drawn.
fn scrolled(s: &mut Setup) -> f32 {
    drawn(s, |_, l| l.scroll)
}

/// A place on the plot's rows that no task's bar reaches.
fn off_any_bar(s: &mut Setup) -> (f32, f32) {
    drawn(s, |g, l| {
        let p = l.plot;
        for j in 1..40 {
            for i in 1..60 {
                let at = (p.x + p.w * i as f32 / 60.0, p.y + p.h * j as f32 / 40.0);
                if die_fig::gantt::hover(g, l, at.0, at.1).is_none() {
                    return at;
                }
            }
        }
        panic!("no place off every bar");
    })
}

/// The text the overlay draws now, as the pane's state has it.
fn overlay_text(s: &mut Setup) -> Vec<String> {
    let st = state(s);
    let st = st.borrow();
    let g = st.gantt.as_ref().expect("a gantt");
    let p = &st.panes[&s.win];
    let l = p.gantt.as_ref().expect("drawn");
    let theme = die_fig::theme::Theme::mono_dark();
    let ov = p.gantt_overlay(g);
    let scene = die_fig::gantt::overlay(g, l, &p.view, &ov, &theme, &mut FixedMeasure);
    (scene.items.iter())
        .filter_map(|i| match i {
            Drawn::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// A click on the middle of a task's bar.
fn click_task(s: &mut Setup, key: &str) {
    let at = mid(bar(s, key));
    click(s, at);
}

fn undoable(s: &mut Setup) -> usize {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].undo.undoable()
}

/// Where the overlay last drew.
fn over_bounds(s: &mut Setup) -> Option<Rect> {
    let st = state(s);
    st.borrow().panes[&s.win].overlay_bounds()
}

/// The source's text replaced, as an edit, and the build after it let land.
fn replace_src(s: &mut Setup, text: &str) {
    let len = s.h.text(s.src).unwrap().len();
    s.h.text_mut(s.src)
        .unwrap()
        .tx(die_core::Origin::User)
        .replace(0..len, text)
        .commit()
        .unwrap();
    s.h.advance(Duration::from_millis(400));
    s.h.run_until_idle();
}

#[test]
fn a_fig_gantt_opens_as_its_figure_and_draws_and_backslash_v_flips_to_its_text() {
    let dir = TempDir::new("gantt-open");
    let path = dir.path().join("release.fig");
    std::fs::write(&path, fixture("release.fig")).unwrap();
    let mut h = harness();
    h.set_window_size(176, 70);
    h.run_until_idle();
    let src = h.open_file(&path).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src, "a .fig opens as its figure");
    let mut s = Setup { h, src, buf, win };
    let st = state(&mut s);
    assert!(st.borrow().is_gantt(), "{:?}", st.borrow().error);
    assert!(st.borrow().model.is_none() && st.borrow().graph.is_none());
    let (base, over) = buffers(&mut s);
    assert_ne!(base, over);
    let (w, h, px) = s.h.pixels(base).expect("drawn");
    let inked = (px.chunks_exact(4))
        .filter(|p| p[..3] != [0x1a, 0x1a, 0x1a])
        .count();
    assert!(inked > 2000, "{inked} px of gantt on {w}×{h}");
    assert!(status(&mut s).contains("11 tasks"), "{}", status(&mut s));
    // The layout the pointer and the keys ask is the gantt's: its rows and its time.
    let (rows, tasks) = drawn(&mut s, |g, l| (l.rows.len(), g.len()));
    assert_eq!((rows, tasks), (14, 11), "eleven tasks under three headings");
    // \v: its text, and its figure again.
    keys(&mut s.h, "\\v");
    assert_eq!(s.h.window_buffer(s.win), Some(src), "its text");
    let errors = run(&mut s.h, "fig-toggle");
    assert!(errors.is_empty(), "{errors:?}");
    s.buf = s.h.window_buffer(s.win).unwrap();
    assert_ne!(s.buf, src, "its figure again");
    assert!(state(&mut s).borrow().is_gantt());
    // A gantt in a buffer with no file is at rest once drawn: no timer, no poll.
    let mut t = release_plan();
    assert_eq!(t.h.next_timer(), None, "at rest, nothing runs");
    assert!(state(&mut t).borrow().is_gantt());
}

#[test]
fn j_k_put_and_move_the_cursor_square_brackets_follow_the_arrows_and_gg_cap_g_go_to_the_ends() {
    let mut s = release_plan();
    assert_eq!(cursor(&mut s), None, "no cursor while it opens");
    keys(&mut s.h, "j");
    assert_eq!(
        cursor(&mut s),
        Some(task("research")),
        "the first j puts it on the first task in view"
    );
    assert_eq!(undoable(&mut s), 0, "the cursor isn't an undo step");
    keys(&mut s.h, "j");
    assert_eq!(cursor(&mut s), Some(task("spec")));
    keys(&mut s.h, "3j");
    assert_eq!(cursor(&mut s), Some(task("ui")), "a count repeats");
    keys(&mut s.h, "k");
    assert_eq!(cursor(&mut s), Some(task("api")));
    // ] follows an arrow to a task that comes after; again goes round the same task's.
    keys(&mut s.h, "gg");
    assert_eq!(cursor(&mut s), Some(task("research")), "gg: the first task");
    keys(&mut s.h, "]");
    assert_eq!(cursor(&mut s), Some(task("spec")), "research → spec");
    keys(&mut s.h, "]");
    assert_eq!(
        cursor(&mut s),
        Some(task("review")),
        "spec's own: the chain goes on"
    );
    keys(&mut s.h, "]");
    assert_eq!(
        cursor(&mut s),
        Some(task("api")),
        "again: the next that comes after spec"
    );
    keys(&mut s.h, "]");
    assert_eq!(cursor(&mut s), Some(task("ui")));
    keys(&mut s.h, "]");
    assert_eq!(cursor(&mut s), Some(task("review")), "round to the first");
    // [ goes to one the cursor's task comes after.
    keys(&mut s.h, "[");
    assert_eq!(
        cursor(&mut s),
        Some(task("spec")),
        "review comes after spec"
    );
    keys(&mut s.h, "[");
    assert_eq!(cursor(&mut s), Some(task("research")));
    let errors = keys_errors(&mut s.h, "[");
    assert!(
        errors.is_empty(),
        "nothing before it, and no error: {errors:?}"
    );
    assert_eq!(cursor(&mut s), Some(task("research")));
    // G, the last; w and b, the milestones; l and h, by when they start.
    keys(&mut s.h, "G");
    assert_eq!(cursor(&mut s), Some(task("launch")), "G: the last task");
    keys(&mut s.h, "b");
    assert_eq!(
        cursor(&mut s),
        Some(task("beta")),
        "b: the milestone before"
    );
    keys(&mut s.h, "ggw");
    assert_eq!(
        cursor(&mut s),
        Some(task("review")),
        "w: the next milestone"
    );
    keys(&mut s.h, "ggl");
    assert_eq!(cursor(&mut s), Some(task("spec")), "l: the next to start");
    keys(&mut s.h, "h");
    assert_eq!(cursor(&mut s), Some(task("research")));
    // H and L: the top and bottom task in view.
    keys(&mut s.h, "L");
    assert_eq!(cursor(&mut s), Some(task("launch")));
    keys(&mut s.h, "H");
    assert_eq!(cursor(&mut s), Some(task("research")));
    // m and `: a gantt has no reference, and says so.
    for key in ["m", "`"] {
        let errors = keys_errors(&mut s.h, key);
        assert!(
            errors.iter().any(|e| e == "fig: a gantt has no reference"),
            "{key}: {errors:?}"
        );
    }
    assert_eq!(view(&mut s).reference, None);
}

#[test]
fn the_pointer_over_a_bar_shows_its_card_leaving_clears_it_and_a_click_puts_the_cursor_there() {
    let mut s = release_plan();
    assert_eq!(over_bounds(&mut s), None, "nothing over the gantt at rest");
    let (_, over) = buffers(&mut s);
    presents(&mut s.h);
    let api = mid(bar(&mut s, "api"));
    mv(&mut s, api);
    assert_eq!(hover(&mut s), Some(task("api")), "no click, no delay");
    let text = overlay_text(&mut s);
    for want in ["API", "Start", "Duration", "Write the spec"] {
        assert!(
            text.iter().any(|t| t == want),
            "the card says {want}: {text:?}"
        );
    }
    assert!(
        presents(&mut s.h).iter().any(|(id, _)| *id == over),
        "the overlay drew"
    );
    let card = over_bounds(&mut s).expect("drawn");
    assert!(card.w > 100.0 && card.h > 60.0, "{card:?}");
    assert_eq!(cursor(&mut s), None, "a hover is no cursor");
    // Its label in the gutter finds it too.
    let label = drawn(&mut s, |_, l| (l.gutter.x + 8.0, l.plot.y));
    mv(&mut s, (label.0, api.1));
    assert_eq!(hover(&mut s), Some(task("api")), "by its label");
    // Off any bar, the date under the pointer is said, and no task is lit.
    let empty = off_any_bar(&mut s);
    mv(&mut s, empty);
    assert_eq!(hover(&mut s), None);
    let text = overlay_text(&mut s);
    assert!(
        !text.iter().any(|t| t == "Start") && text.len() == 1,
        "only the date's chip: {text:?}"
    );
    // Leaving clears it.
    mv(&mut s, api);
    s.h.send(Input::PointerLeft);
    assert_eq!(hover(&mut s), None);
    assert_eq!(over_bounds(&mut s), None, "the overlay is clear");
    // A click puts the cursor there; on the background, nothing.
    click(&mut s, api);
    assert_eq!(cursor(&mut s), Some(task("api")));
    assert_eq!(undoable(&mut s), 0, "no undo step");
    click(&mut s, empty);
    assert_eq!(
        cursor(&mut s),
        Some(task("api")),
        "the background does nothing"
    );
    // The cursor's task wears its ring and its card once the pointer has gone.
    s.h.send(Input::PointerLeft);
    let text = overlay_text(&mut s);
    assert!(
        text.iter().any(|t| t == "Start"),
        "the cursor's card: {text:?}"
    );
}

#[test]
fn a_drag_across_the_plot_zooms_the_time_u_undoes_it_and_0_fits() {
    let mut s = release_plan();
    let fit = time(&mut s);
    let plot = drawn(&mut s, |_, l| l.plot);
    let y = plot.y + plot.h - 6.0;
    let (x0, x1) = (plot.x + plot.w * 0.25, plot.x + plot.w * 0.5);
    let (t0, t1) = drawn(&mut s, |_, l| (l.time(x0), l.time(x1)));
    press(&mut s, (x0, y));
    drag(&mut s, (x0 + 30.0, y));
    // While it's drawn, the brush shows and the time holds still.
    let brushing = {
        let st = state(&mut s);
        let st = st.borrow();
        st.panes[&s.win]
            .gantt_overlay(st.gantt.as_ref().unwrap())
            .brush
    };
    assert_eq!(brushing, Some((x0, x0 + 30.0)));
    assert_eq!(view(&mut s).x, None);
    drag(&mut s, (x1, y));
    release(&mut s, (x1, y));
    let v = view(&mut s).x.expect("zoomed");
    assert!(
        (v.0 - t0).abs() < 1.0 && (v.1 - t1).abs() < 1.0,
        "{v:?} is {t0}..{t1}"
    );
    assert_eq!(view(&mut s).y, None, "the rows stay");
    assert_eq!(undoable(&mut s), 1, "one undo step");
    // It eases there: part way at once, there once it has.
    let part = time(&mut s);
    assert!(part.0 < t0 && part.1 > t1, "easing: {part:?}");
    settle(&mut s);
    let there = time(&mut s);
    assert!((there.0 - t0).abs() < 1.0 && (there.1 - t1).abs() < 1.0);
    keys(&mut s.h, "u");
    assert_eq!(view(&mut s).x, None, "u undoes it");
    settle(&mut s);
    assert_eq!(time(&mut s), fit);
    keys(&mut s.h, "<C-r>");
    assert!(view(&mut s).x.is_some(), "C-r does it again");
    keys(&mut s.h, "0");
    assert_eq!(view(&mut s).x, None, "0 fits the plan");
    // + zooms about the cursor's task, which stays where it is.
    settle(&mut s);
    click_task(&mut s, "api");
    let before = mid(bar(&mut s, "api")).0;
    keys(&mut s.h, "+");
    settle(&mut s);
    let zoomed = time(&mut s);
    assert!(zoomed.1 - zoomed.0 < fit.1 - fit.0, "zoomed in");
    assert!((mid(bar(&mut s, "api")).0 - before).abs() < 1.5);
    // A drag on the axis pans the time and never brushes; ⌥-drag pans too.
    let axis = drawn(&mut s, |_, l| mid(l.axis));
    let at = time(&mut s);
    gesture(
        &mut s,
        &[axis, (axis.0 - 20.0, axis.1), (axis.0 - 60.0, axis.1)],
    );
    let panned = time(&mut s);
    assert!(panned.0 > at.0, "later: {at:?} → {panned:?}");
    assert!(
        ((panned.1 - panned.0) - (at.1 - at.0)).abs() < 1.0,
        "the same span"
    );
    let mid_plot = mid(plot);
    gesture_with(
        &mut s,
        &[
            mid_plot,
            (mid_plot.0 + 20.0, mid_plot.1),
            (mid_plot.0 + 60.0, mid_plot.1),
        ],
        ALT,
        die_core::Button::Left,
    );
    let back = time(&mut s);
    assert!(
        (back.0 - at.0).abs() < (at.1 - at.0) * 0.01,
        "{back:?} is {at:?} again"
    );
}

#[test]
fn the_wheel_scrolls_rows_on_a_tall_plan_and_pans_the_time_on_a_short_one_and_cmd_wheel_zooms() {
    // Forty rows in a window that shows a dozen: up and down are the rows.
    let mut s = gantt_with("tall.fig", &tall_spec(), 176, 40);
    let (plot, most) = drawn(&mut s, |_, l| (l.plot, l.max_scroll()));
    assert!(most > 100.0, "the rows overflow: {most}");
    let at = mid(plot);
    let fit = time(&mut s);
    scroll(&mut s, at, (0.0, 3.0), Mods::default());
    assert_eq!(scrolled(&mut s), 30.0, "three lines, a cell high each");
    assert_eq!(view(&mut s).x, None, "the time stays");
    // The burst keeps its first meaning: a sideways event in it moves nothing across.
    scroll(&mut s, at, (4.0, 0.0), Mods::default());
    assert_eq!(view(&mut s).x, None);
    assert_eq!(undoable(&mut s), 0, "no step until the burst ends");
    settle(&mut s);
    assert_eq!(undoable(&mut s), 1, "one undo step a burst");
    // Sideways pans the time, here too.
    scroll(&mut s, at, (4.0, 0.0), Mods::default());
    let panned = view(&mut s).x.expect("panned");
    assert!(panned.0 > fit.0, "later: {panned:?}");
    assert_eq!(scrolled(&mut s), 30.0, "the rows stay");
    settle(&mut s);
    keys(&mut s.h, "u");
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!((scrolled(&mut s), view(&mut s).x), (0.0, None));
    // j past the last row in view scrolls its row into view.
    keys(&mut s.h, "G");
    settle(&mut s);
    assert_eq!(cursor(&mut s), Some(task("t39")));
    assert_eq!(scrolled(&mut s), most, "the last row in view");

    // A short plan, every row in view: up and down pan the time.
    let mut s = release_plan();
    assert_eq!(drawn(&mut s, |_, l| l.max_scroll()), 0.0, "the rows fit");
    let plot = drawn(&mut s, |_, l| l.plot);
    let at = mid(plot);
    let fit = time(&mut s);
    scroll(&mut s, at, (0.0, 3.0), Mods::default());
    let v = view(&mut s).x.expect("the time panned");
    assert!(v.0 > fit.0, "down is later: {v:?}");
    assert_eq!(view(&mut s).y, None);
    settle(&mut s);
    // ⌘-wheel zooms the time about the pointer.
    let under = drawn(&mut s, |_, l| l.time(at.0));
    let span = time(&mut s);
    scroll(&mut s, at, (0.0, -4.0), CMD);
    let zoomed = time(&mut s);
    assert!(zoomed.1 - zoomed.0 < span.1 - span.0, "up zooms in");
    let now = drawn(&mut s, |_, l| l.time(at.0));
    assert!(
        (now - under).abs() < (zoomed.1 - zoomed.0) * 0.005,
        "about the pointer"
    );
    assert_eq!(view(&mut s).y, None, "the rows aren't zoomed");
}

/// The detail panel's title, its cursor's row and whether it has the keys.
fn panel_at(s: &mut Setup) -> (String, usize, bool) {
    let st = state(s);
    let st = st.borrow();
    let p = st.panes[&s.win].panel.as_ref().expect("open");
    (p.tree.title.clone(), p.state.row, p.state.focused)
}

#[test]
fn slash_finds_a_task_by_its_label_and_return_on_a_comes_after_row_moves_the_cursor() {
    let mut s = release_plan();
    for log in s.h.type_keys("/sync").unwrap() {
        assert!(log.errors.is_empty(), "{:?}", log.errors);
    }
    assert_eq!(
        cursor(&mut s),
        Some(task("sync")),
        "the best match as typed"
    );
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].matches.len(), 1, "lit once");
    keys(&mut s.h, "<Esc>");
    assert_eq!(cursor(&mut s), None, "Esc puts the cursor back");
    keys(&mut s.h, "/sync<CR>");
    assert_eq!(cursor(&mut s), Some(task("sync")), "⏎ keeps the find");
    assert_eq!(st.borrow().panes[&s.win].found_node, Some(task("sync")));
    // By its id too, and n goes on through the matches.
    keys(&mut s.h, "/qa<CR>");
    assert_eq!(cursor(&mut s), Some(task("qa")), "found by its id");
    keys(&mut s.h, "/a<CR>");
    let first = cursor(&mut s).expect("a match");
    keys(&mut s.h, "n");
    let second = cursor(&mut s).expect("the next");
    assert_ne!(first, second, "n: the next match");
    keys(&mut s.h, "N");
    assert_eq!(cursor(&mut s), Some(first));
    keys(&mut s.h, "<Esc>");
    assert_eq!(st.borrow().panes[&s.win].found_node, None);

    // ⏎ opens the detail panel for the cursor's task.
    keys(&mut s.h, "/sync<CR><Esc>");
    keys(&mut s.h, "<CR>");
    let (title, _, focused) = panel_at(&mut s);
    assert_eq!((title.as_str(), focused), ("Offline sync", false));
    // Tab gives it the keys; down to the row under `Comes after` that names the API.
    keys(&mut s.h, "<Tab>");
    let api = {
        let st = st.borrow();
        let p = st.panes[&s.win].panel.as_ref().unwrap();
        let after = (p.tree.rows.iter())
            .position(|r| r.key == "Comes after")
            .expect("a Comes after section");
        let api = (p.tree.rows.iter())
            .position(|r| matches!(&r.item, Some(Item::Task { id, .. }) if &**id == "api"))
            .expect("the API among them");
        assert!(api > after, "under Comes after");
        api
    };
    for _ in 0..60 {
        if panel_at(&mut s).1 == api {
            break;
        }
        keys(&mut s.h, "j");
    }
    assert_eq!(panel_at(&mut s).1, api);
    assert_eq!(
        cursor(&mut s),
        Some(task("sync")),
        "j moved the panel's cursor"
    );
    keys(&mut s.h, "<CR>");
    assert_eq!(
        cursor(&mut s),
        Some(task("api")),
        "⏎ takes the cursor there"
    );
    assert_eq!(panel_at(&mut s).0, "API", "and the panel follows");
    // A double-click on a task opens its detail; on the background it fits.
    keys(&mut s.h, "<S-Tab><CR>");
    assert!(st.borrow().panes[&s.win].panel.is_none(), "⏎ closes it");
    settle(&mut s);
    let ui = mid(bar(&mut s, "ui"));
    double_click(&mut s, ui);
    assert_eq!(panel_at(&mut s).0, "Web UI");
    assert_eq!(cursor(&mut s), Some(task("ui")));
    keys(&mut s.h, "<Esc>");
    keys(&mut s.h, "+");
    settle(&mut s);
    assert!(view(&mut s).x.is_some());
    let empty = off_any_bar(&mut s);
    double_click(&mut s, empty);
    assert_eq!(
        view(&mut s).x,
        None,
        "a double-click on the background fits"
    );
}

#[test]
fn t_shows_the_tasks_with_the_dates_worked_out_and_s_turns_the_links_off_and_u_brings_them_back() {
    let mut s = release_plan();
    keys(&mut s.h, "/sync<CR><Esc>");
    keys(&mut s.h, "t");
    let b =
        s.h.find_buffer("*fig table: release.fig*")
            .expect("the table");
    let text = s.h.text(b).unwrap().rope().to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("id\tlabel\tgroup\tstart\tend\tduration\tprogress\tafter"),
        "{}",
        lines[0]
    );
    // The API comes after the spec for twelve working days: its dates were worked out.
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("api\tAPI\tBuild\t2026-11-13\t2026-11-30\t12d\t0.7\tspec")),
        "{text}"
    );
    let w = (s.h.window_list().into_iter())
        .find(|w| s.h.window_buffer(*w) == Some(b))
        .unwrap();
    let t = s.h.text(b).unwrap();
    let line = t.line_of(t.cursor(w).unwrap());
    assert!(
        lines[line].starts_with("sync\t"),
        "on the cursor's row: {}",
        lines[line]
    );

    // s: the gantt's own menu; links off draws no arrow, and is one undo step.
    let mut s = release_plan();
    keys(&mut s.h, "j+");
    settle(&mut s);
    let (kept, at) = (view(&mut s), cursor(&mut s));
    let links = drawn(&mut s, |_, l| l.links.len());
    assert!(links > 5, "{links} arrows");
    let st = state(&mut s);
    let builds = st.borrow().builds;
    keys(&mut s.h, "s");
    let c = s.h.prompt_candidates();
    assert_eq!(
        c,
        ["grid: on", "axes: on", "links: on", "today: on"],
        "{c:?}"
    );
    keys(&mut s.h, "links<CR>");
    assert_eq!(st.borrow().toggles.links, Some(false));
    assert_eq!(drawn(&mut s, |_, l| l.links.len()), 0, "no arrows");
    assert_eq!(
        st.borrow().builds,
        builds,
        "built again from the table in hand"
    );
    assert_eq!(
        (view(&mut s), cursor(&mut s)),
        (kept.clone(), at.clone()),
        "view and cursor stay"
    );
    assert!(s.h.messages.iter().any(|m| m == "settings: links off"));
    // today off takes the line away; the menu says what's turned.
    keys(&mut s.h, "s");
    assert!(s.h.prompt_candidates().contains(&"links: off"));
    keys(&mut s.h, "today<CR>");
    assert!(drawn(&mut s, |g, _| g.today().is_none()), "no today line");
    // u brings each back, a step each, the view as it was.
    keys(&mut s.h, "u");
    assert!(drawn(&mut s, |g, _| g.today().is_some()), "today again");
    assert_eq!(
        st.borrow().toggles.links,
        Some(false),
        "the links still off"
    );
    keys(&mut s.h, "u");
    assert_eq!(st.borrow().toggles.links, None, "undone");
    assert_eq!(
        drawn(&mut s, |_, l| l.links.len()),
        links,
        "the arrows again"
    );
    assert_eq!(view(&mut s), kept);
    assert_eq!(st.borrow().builds, builds, "and never read again");
}

#[test]
fn x_hides_the_cursors_colour_name_cap_x_shows_all_and_a_legend_click_does_the_same() {
    let mut s = owners();
    let maya = SeriesKey::new("Maya", 0);
    let legend = drawn(&mut s, |_, l| l.legend.clone());
    assert_eq!(legend.len(), 3, "Maya, Ravi and Lena");
    assert_eq!(drawn(&mut s, |_, l| l.order().len()), 4);
    click_task(&mut s, "a");
    keys(&mut s.h, "x");
    assert!(
        view(&mut s).hidden.contains(&maya),
        "x hides the cursor's name"
    );
    assert_eq!(cursor(&mut s), None, "the cursor goes with its row");
    assert_eq!(
        drawn(&mut s, |_, l| l.order().len()),
        2,
        "Maya's two rows gone"
    );
    assert!(status(&mut s).contains("2 hidden"), "{}", status(&mut s));
    keys(&mut s.h, "X");
    assert!(view(&mut s).hidden.is_empty(), "X shows all");
    assert_eq!(drawn(&mut s, |_, l| l.order().len()), 4);
    keys(&mut s.h, "u");
    assert!(view(&mut s).hidden.contains(&maya), "each is an undo step");
    keys(&mut s.h, "X");
    // o: only the cursor's name.
    click_task(&mut s, "b");
    keys(&mut s.h, "o");
    assert_eq!(view(&mut s).only, Some(SeriesKey::new("Ravi", 0)));
    assert_eq!(drawn(&mut s, |_, l| l.order().len()), 1);
    keys(&mut s.h, "X");
    // A legend click hides its name, again shows it; a double-click shows only it.
    let key = |s: &mut Setup, name: &str| {
        let r = drawn(s, |_, l| {
            (l.legend.iter())
                .find(|(k, _)| &*k.name == name)
                .map(|(_, r)| *r)
                .expect("its key")
        });
        mid(r)
    };
    let at = key(&mut s, "Lena");
    click(&mut s, at);
    assert!(view(&mut s).hidden.contains(&SeriesKey::new("Lena", 0)));
    assert_eq!(drawn(&mut s, |_, l| l.order().len()), 3);
    let at = key(&mut s, "Lena");
    click(&mut s, at);
    assert!(view(&mut s).hidden.is_empty(), "again shows it");
    s.h.advance(Duration::from_millis(600));
    let at = key(&mut s, "Maya");
    double_click(&mut s, at);
    assert_eq!(view(&mut s).only, Some(maya), "a double-click: only it");
    // x with no cursor, and on a plan no name colours, says why.
    keys(&mut s.h, "X");
    let errors = keys_errors(&mut s.h, "x");
    assert!(
        errors.iter().any(|e| e.contains("no cursor yet")),
        "{errors:?}"
    );
    let mut t = gantt_with("tall.fig", &tall_spec(), 176, 40);
    keys(&mut t.h, "j");
    let errors = keys_errors(&mut t.h, "x");
    assert!(
        errors.iter().any(|e| e.contains("no group or colour")),
        "{errors:?}"
    );
    assert!(view(&mut t).hidden.is_empty());
}

#[test]
fn an_edit_that_removes_the_cursors_task_lets_the_cursor_go_and_keeps_the_view_and_a_broken_spec_keeps_the_figure()
 {
    let mut s = owners();
    click_task(&mut s, "d");
    keys(&mut s.h, "+");
    settle(&mut s);
    click_task(&mut s, "b");
    keys(&mut s.h, "x");
    click_task(&mut s, "d");
    let kept = view(&mut s);
    assert!(kept.x.is_some() && kept.hidden.contains(&SeriesKey::new("Ravi", 0)));
    // An edit that keeps every task: the view and the cursor stay.
    replace_src(&mut s, &OWNERS.replace("Alpha", "Alpha one"));
    assert_eq!(view(&mut s), kept);
    assert_eq!(cursor(&mut s), Some(task("d")));
    assert!(
        drawn(&mut s, |g, _| g.label(0) == "Alpha one"),
        "the edit drawn"
    );
    // Delta goes: the cursor lets go, the view stays.
    let without: String = (OWNERS.lines())
        .filter(|l| !l.contains("\"id\": \"d\""))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\"owner\": \"Maya\"},\n  ]", "\"owner\": \"Maya\"}\n  ]");
    replace_src(&mut s, &without);
    let st = state(&mut s);
    assert!(st.borrow().error.is_none(), "{:?}", st.borrow().error);
    assert_eq!(drawn(&mut s, |g, _| g.len()), 3);
    assert_eq!(cursor(&mut s), None, "its task is gone");
    assert_eq!(view(&mut s).x, kept.x, "the time shown stays");
    assert!(view(&mut s).hidden.contains(&SeriesKey::new("Ravi", 0)));
    // Ravi's task goes too: a name that's gone leaves the hidden set.
    let one = without.replace("\"owner\": \"Ravi\"", "\"owner\": \"Maya\"");
    replace_src(&mut s, &one);
    assert!(view(&mut s).hidden.is_empty(), "{:?}", view(&mut s).hidden);
    // A spec that breaks keeps the figure, with the stale badge saying where.
    replace_src(
        &mut s,
        &one.replace("\"after\": \"b\"", "\"after\": \"nobody\""),
    );
    assert!(st.borrow().is_gantt(), "the figure stays");
    assert_eq!(drawn(&mut s, |g, _| g.len()), 3);
    let badge = st.borrow().badge().expect("the stale badge");
    assert!(badge.starts_with("Error"), "{badge}");
    assert!(status(&mut s).contains("Error"), "{}", status(&mut s));
    let has_badge = {
        let st = st.borrow();
        let p = &st.panes[&s.win];
        let (w, _) = p.size();
        (0..w as i32).any(|x| (0..80).any(|y| p.hit((x as f32, y as f32)) == Hit::Badge))
    };
    assert!(has_badge, "the badge is drawn, and takes the pointer");
    assert_eq!(view(&mut s).x, kept.x);
    // Mended, it's a figure again with no badge.
    replace_src(&mut s, &one);
    assert_eq!(st.borrow().badge(), None);
}

fn up(state_dir: &TempDir) -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Fig::with_zone(TimeZone::UTC)),
    ];
    let config = Config {
        session: Some(state_dir.path().to_path_buf()),
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(176, 70);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

#[test]
fn a_restart_keeps_the_view_the_cursor_and_the_settings() {
    let state_dir = TempDir::new("gantt-session-state");
    let dir = TempDir::new("gantt-session-data");
    let path = dir.path().join("owners.fig");
    std::fs::write(&path, OWNERS).unwrap();
    let mut h = up(&state_dir);
    let src = h.open_file(&path).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src, "a .fig opens as its figure");
    // Past the grace of the focus the start gave, so the first press isn't only focusing.
    h.advance(Duration::from_millis(200));
    let mut s = Setup { h, src, buf, win };
    assert!(state(&mut s).borrow().is_gantt());
    click_task(&mut s, "b");
    keys(&mut s.h, "x");
    click_task(&mut s, "d");
    keys(&mut s.h, "++");
    settle(&mut s);
    keys(&mut s.h, "s");
    keys(&mut s.h, "links<CR>");
    keys(&mut s.h, "s");
    keys(&mut s.h, "grid<CR>");
    keys(&mut s.h, "<CR>");
    let want = view(&mut s);
    assert!(want.x.is_some() && want.hidden.contains(&SeriesKey::new("Ravi", 0)));
    let mut h = s.h;
    h.hand_over_session().unwrap();
    drop(h);
    let h = up(&state_dir);
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup {
        h,
        src: buf,
        buf,
        win,
    };
    assert_eq!(s.h.buffer_name(buf), Some("owners.fig *fig*"));
    let st = state(&mut s);
    assert!(st.borrow().is_gantt(), "built: {:?}", st.borrow().error);
    let got = view(&mut s);
    assert_eq!(got.x, want.x, "the time shown");
    assert_eq!(got.hidden, want.hidden, "what's hidden");
    assert_eq!(cursor(&mut s), Some(task("d")), "the cursor");
    let t = st.borrow().toggles;
    assert_eq!(
        (t.links, t.grid, t.axes, t.today),
        (Some(false), Some(false), None, None)
    );
    assert_eq!(
        drawn(&mut s, |_, l| l.links.len()),
        0,
        "drawn with the links off"
    );
    let panel = st.borrow().panes[&s.win]
        .panel
        .as_ref()
        .map(|p| p.tree.title.clone());
    assert_eq!(panel.as_deref(), Some("Delta"), "the panel open again");
    assert_eq!(
        st.borrow().panes[&s.win].undo.undoable(),
        0,
        "not the undo history"
    );
}

// ---------------------------------------------------------------- a dashboard's gantt panel

/// `:fig PATH` in a window `cols` × `rows` cells, the dashboard drawn and its data read.
fn open_dash(path: &std::path::Path, cols: usize, rows: usize) -> Setup {
    let mut h = harness();
    h.set_window_size(cols, rows);
    h.run_until_idle();
    let (src, win) = h.open_text("notes.txt", "notes\n").unwrap();
    h.run_until_idle();
    let args = die_core::Args {
        text: Some(path.display().to_string()),
        ..die_core::Args::default()
    };
    h.run("fig", &args).unwrap();
    h.run_until_idle();
    h.scene();
    let errors = h.take_errors();
    assert!(errors.is_empty(), "{errors:?}");
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src, "the figure shows in the window");
    Setup { h, src, buf, win }
}

/// Ask a dashboard's panel `i`: its gantt's model, its layout as last drawn, and the figure's
/// origin in the area.
fn panel<R>(
    s: &mut Setup,
    i: usize,
    f: impl FnOnce(&GanttModel, &GanttLayout, (f32, f32)) -> R,
) -> R {
    let st = state(s);
    let st = st.borrow();
    let model = st.dash.as_ref().unwrap().model.as_ref().expect("built");
    let g = model.panels[i].gantt().expect("a gantt panel");
    let dp = st.panes[&s.win].dash.as_ref().expect("a dashboard");
    let pl = dp.layout.as_ref().unwrap().panel(i).expect("laid out");
    f(g, pl.gantt().expect("a gantt's layout"), pl.origin())
}

/// Panel `i`'s cursor.
fn panel_cursor(s: &mut Setup, i: usize) -> Option<Cursor> {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().subs[i]
        .cursor
        .clone()
}

/// The focused panel.
fn focused(s: &mut Setup) -> Option<usize> {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().view.focused
}

#[test]
fn a_dashboards_gantt_panel_draws_takes_the_focus_and_the_keys_hovers_and_shows_its_tasks() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/dash/delivery.fig");
    let mut s = open_dash(&path, 176, 76);
    let st = state(&mut s);
    assert!(st.borrow().error.is_none(), "{:?}", st.borrow().error);
    // Two tiles, the burn-down and the plan: the fourth panel is the gantt, with its title
    // and subtitle the panel's own.
    let (kinds, title) = {
        let st = st.borrow();
        let m = st.dash.as_ref().unwrap().model.as_ref().expect("built");
        let kinds: Vec<_> = m.panels.iter().map(|p| p.kind).collect();
        (kinds, m.panels[3].title.clone())
    };
    use die_fig::dashboard::PanelKind::{Chart, Gantt, Tile};
    assert_eq!(kinds, [Tile, Tile, Chart, Gantt]);
    assert_eq!(title.as_deref(), Some("Q4 engineering plan"));
    // It draws: twelve tasks under its axis, its bars inked on the base inside its frame.
    let (tasks, rows, plot, origin) =
        panel(&mut s, 3, |g, l, o| (g.len(), l.rows.len(), l.plot, o));
    assert_eq!((tasks, rows), (12, 12), "twelve tasks, a row each");
    let (base, _) = buffers(&mut s);
    let (w, _, px) = s.h.pixels(base).expect("drawn");
    let bar = panel(&mut s, 3, |g, l, o| {
        let b = l.task(g.index_of("auth-2").unwrap()).unwrap().bar;
        Rect::new(b.x + o.0, b.y + o.1, b.w, b.h)
    });
    let at = mid(bar);
    let scale = CELL.scale as f32;
    let i = ((at.1 * scale) as u32 * w + (at.0 * scale) as u32) as usize * 4;
    assert_ne!(
        px[i..i + 3],
        [0x1a, 0x1a, 0x1a],
        "a bar's colour at its middle"
    );
    assert!(
        plot.h >= 4.0 * 28.0,
        "room for four rows at the least: {plot:?}"
    );
    // Tab focuses it, the fourth stop; its keys are a gantt's.
    assert_eq!(focused(&mut s), None);
    keys(&mut s.h, "<Tab><Tab><Tab><Tab>");
    assert_eq!(focused(&mut s), Some(3));
    keys(&mut s.h, "j");
    assert_eq!(
        panel_cursor(&mut s, 3),
        Some(task("auth")),
        "the first task in view"
    );
    keys(&mut s.h, "j");
    assert_eq!(panel_cursor(&mut s, 3), Some(task("auth-1")));
    keys(&mut s.h, "]");
    assert_eq!(
        panel_cursor(&mut s, 3),
        Some(task("auth-2")),
        "] follows its arrow"
    );
    let errors = keys_errors(&mut s.h, "m");
    assert!(
        errors.iter().any(|e| e == "fig: a gantt has no reference"),
        "{errors:?}"
    );
    // Its hover shows its card: the pane finds the task, and the dashboard's overlay draws it.
    assert_eq!(
        over_bounds(&mut s).map(|b| b.w > 100.0),
        Some(true),
        "the cursor's card"
    );
    let bill = panel(&mut s, 3, |g, l, o| {
        let b = l.task(g.index_of("bill-2").unwrap()).unwrap().bar;
        (b.x + b.w / 2.0 + o.0, b.y + b.h / 2.0 + o.1)
    });
    mv(&mut s, bill);
    let (hovered, text) = {
        let st = st.borrow();
        let model = st.dash.as_ref().unwrap().model.as_ref().unwrap();
        let dp = st.panes[&s.win].dash.as_ref().unwrap();
        let layout = dp.layout.as_ref().unwrap();
        let hover = die_fig::dashboard::hover(model, layout, bill.0, bill.1).expect("on a panel");
        let theme = die_fig::theme::Theme::mono_dark();
        let over = die_fig::dashboard::overlay(
            model,
            layout,
            &dp.view,
            Some(&hover),
            &theme,
            &mut FixedMeasure,
        );
        let text: Vec<String> = (over.items.iter())
            .filter_map(|i| match i {
                Drawn::Text { text, .. } => Some(text.to_string()),
                _ => None,
            })
            .collect();
        (dp.subs[3].hover.clone(), text)
    };
    assert_eq!(
        hovered,
        Some(task("bill-2")),
        "the panel's pane hovers the task"
    );
    for want in ["Usage metering", "Start", "Progress"] {
        assert!(
            text.iter().any(|t| t == want),
            "the card says {want}: {text:?}"
        );
    }
    // A click puts its cursor there; a drag across its plot zooms its own time, not the page.
    click(&mut s, bill);
    assert_eq!(panel_cursor(&mut s, 3), Some(task("bill-2")));
    let fit = panel(&mut s, 3, |_, l, _| l.domain);
    let y = origin.1 + plot.y + plot.h - 4.0;
    let (x0, x1) = (
        origin.0 + plot.x + plot.w * 0.3,
        origin.0 + plot.x + plot.w * 0.6,
    );
    gesture(&mut s, &[(x0, y), (x0 + 30.0, y), (x1, y)]);
    settle(&mut s);
    let zoomed = panel(&mut s, 3, |_, l, _| l.domain);
    assert!(
        zoomed.1 - zoomed.0 < (fit.1 - fit.0) * 0.5,
        "{fit:?} → {zoomed:?}"
    );
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!(
        panel(&mut s, 3, |_, l, _| l.domain),
        fit,
        "u: the dashboard's undo"
    );
    // t shows its tasks, on the cursor's row.
    keys(&mut s.h, "t");
    let b =
        s.h.find_buffer("*fig table: delivery.fig*")
            .expect("the table");
    let table = s.h.text(b).unwrap().rope().to_string();
    let lines: Vec<&str> = table.lines().collect();
    assert!(
        lines[0].starts_with("id\tlabel\tgroup\tstart\tend"),
        "{}",
        lines[0]
    );
    assert_eq!(lines.len(), 13, "the header and twelve tasks");
    let tw = (s.h.window_list().into_iter())
        .find(|w| s.h.window_buffer(*w) == Some(b))
        .unwrap();
    let t = s.h.text(b).unwrap();
    let line = t.line_of(t.cursor(tw).unwrap());
    assert!(lines[line].starts_with("bill-2\t"), "{}", lines[line]);
}
