//! Dashboards in the editor (contract-p4 §5): opening from a `.fig`, `\v` and `:fig`, the panels
//! built in jobs and shown as they come; the dashboard's keys (Tab, S-Tab, f, T, R, G, ?) and
//! every panel figure's keys in the focused panel; the pointer (mouse-ux §4.6, §4.7 and every
//! rule of §3: the focusing press, a press held by its panel and button, wheel bursts, the hover
//! worked out again after a view change, the pointer leaving); the linked crosshair, zoom and
//! brush as one undo step; heatmaps; live sources with the trust prompt, R and follow with G;
//! per-panel errors with `!`; the session; and what a hover costs.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, Button, Mods, PointerKind};
use die_fig::dashboard::layout::PanelFigure;
use die_fig::interact::Cursor;
use die_fig::scene::Rect;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A fixture's path.
fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/data")
        .join(rel)
}

/// `:fig PATH` in a window `cols` × `rows` cells (5 × 10 logical px each), the dashboard drawn.
fn open_at(path: &Path, cols: usize, rows: usize) -> Setup {
    let mut h = harness();
    h.set_window_size(cols, rows);
    h.run_until_idle();
    let (src, win) = h.open_text("notes.txt", "notes\n").unwrap();
    h.run_until_idle();
    let args = Args {
        text: Some(path.display().to_string()),
        ..Args::default()
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

/// health.fig (m4) at m4's size, 880 × 610.
fn health() -> Setup {
    open_at(&fixture("dash/health.fig"), 176, 61)
}

/// Let every ease finish and every burst end.
fn settle(s: &mut Setup) {
    s.h.advance(Duration::from_millis(400));
}

/// Panel `i`'s frame, in the area's logical px, as last laid out.
fn frame(s: &mut Setup, i: usize) -> Rect {
    let st = state(s);
    let st = st.borrow();
    let dp = st.panes[&s.win].dash.as_ref().expect("a dashboard");
    dp.layout
        .as_ref()
        .unwrap()
        .panel(i)
        .expect("laid out")
        .frame
}

/// Panel `i`'s plot, in the area's px.
fn plot_of(s: &mut Setup, i: usize) -> Rect {
    let st = state(s);
    let st = st.borrow();
    let dp = st.panes[&s.win].dash.as_ref().unwrap();
    let p = dp.layout.as_ref().unwrap().panel(i).unwrap();
    let (l, _) = p.chart().expect("a chart");
    let (ox, oy) = p.origin();
    Rect::new(l.plot.x + ox, l.plot.y + oy, l.plot.w, l.plot.h)
}

/// The area's x of time `t` on panel `i`'s chart.
fn x_of(s: &mut Setup, i: usize, t: f64) -> f32 {
    let st = state(s);
    let st = st.borrow();
    let dp = st.panes[&s.win].dash.as_ref().unwrap();
    let p = dp.layout.as_ref().unwrap().panel(i).unwrap();
    let (l, _) = p.chart().unwrap();
    l.x.map(t) + p.origin().0
}

/// The dashboard's shared time, as its view has it.
fn time(s: &mut Setup) -> Option<(f64, f64)> {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().view.time
}

/// The focused panel.
fn focused(s: &mut Setup) -> Option<usize> {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().view.focused
}

/// Panel `i`'s cursor.
fn panel_cursor(s: &mut Setup, i: usize) -> Option<Cursor> {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().subs[i]
        .cursor
        .clone()
}

/// The dashboard's undo steps.
fn undoable(s: &mut Setup) -> usize {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().undo.undoable()
}

/// The panel the pointer is over.
fn hovered(s: &mut Setup) -> Option<usize> {
    let st = state(s);
    let st = st.borrow();
    st.panes[&s.win].dash.as_ref().unwrap().hovered
}

#[test]
fn a_dashboard_opens_from_fig_with_every_panel_drawn_and_its_status_says_so() {
    let mut s = health();
    let st = state(&mut s);
    {
        let st = st.borrow();
        let d = st.dash.as_ref().expect("a dashboard");
        let m = d.model.as_ref().expect("built");
        assert_eq!(m.panels.len(), 6);
        assert!(m.panels.iter().all(|p| p.ready()), "every panel draws");
        assert_eq!(m.linked().len(), 6, "four stats and two charts on time");
        let dp = st.panes[&s.win].dash.as_ref().unwrap();
        let l = dp.layout.as_ref().unwrap();
        assert!(matches!(
            l.panel(4).unwrap().figure,
            PanelFigure::Chart { .. }
        ));
    }
    // m4's places: tiles across the top, the charts under them.
    assert_eq!(frame(&mut s, 0).y, 92.0);
    assert_eq!(frame(&mut s, 4).y, 208.0);
    assert!(status(&mut s).contains("6 panels"), "{}", status(&mut s));
    // The time opens on the last six hours.
    let (a, b) = time(&mut s).expect("a range");
    assert!((b - a - 21_600.0).abs() < 1e-6);
    // The data files are polled each second while it shows, and nothing else runs.
    assert_eq!(s.h.next_timer(), Some(Duration::from_secs(1)));
}

/// The union of what the overlay presented since the last ask, in logical px.
fn overlay_presented(s: &mut Setup) -> Option<Rect> {
    let (_, over) = buffers(s);
    let mut out: Option<Rect> = None;
    for (id, r) in presents(&mut s.h) {
        if id != over {
            continue;
        }
        let k = CELL.scale as f32;
        let r = Rect::new(
            r.x as f32 / k,
            r.y as f32 / k,
            r.width as f32 / k,
            r.height as f32 / k,
        );
        out = Some(out.map_or(r, |o| o.union(&r)));
    }
    out
}

/// The middle of panel `i`'s plot.
fn plot_mid(s: &mut Setup, i: usize) -> (f32, f32) {
    mid(plot_of(s, i))
}

#[test]
fn hovering_a_linked_chart_shows_its_card_and_the_moment_in_the_other_chart_and_the_tiles() {
    let mut s = health();
    presents(&mut s.h);
    let at = plot_mid(&mut s, 4);
    mv(&mut s, at);
    assert_eq!(hovered(&mut s), Some(4));
    let hover = {
        let st = state(&mut s);
        let st = st.borrow();
        st.panes[&s.win].dash.as_ref().unwrap().subs[4]
            .hover
            .clone()
    };
    assert!(matches!(hover, Some(Cursor::Point { .. })), "{hover:?}");
    // The overlay drew the card in the hovered chart and the moment's strip in the other: one
    // present reaching across both (ruling 10's union).
    let r = overlay_presented(&mut s).expect("the overlay drew");
    let right = frame(&mut s, 5);
    assert!(r.x < at.0 && r.right() > right.x + 20.0, "{r:?}");
    // The pointer leaving takes it all away.
    presents(&mut s.h);
    s.h.send(die_core::Input::PointerLeft);
    assert_eq!(hovered(&mut s), None);
    assert!(overlay_presented(&mut s).is_some(), "cleared");
}

#[test]
fn a_click_on_a_panel_focuses_it_and_puts_its_cursor_and_on_a_title_focuses_only() {
    let mut s = health();
    let at = plot_mid(&mut s, 5);
    click(&mut s, at);
    assert_eq!(focused(&mut s), Some(5));
    assert!(panel_cursor(&mut s, 5).is_some(), "the cursor is there");
    assert!(status(&mut s).contains("panel 6/6"), "{}", status(&mut s));
    // On the left chart's title: focus only.
    let f = frame(&mut s, 4);
    click(&mut s, (f.x + 30.0, f.y + 22.0));
    assert_eq!(focused(&mut s), Some(4));
    assert_eq!(panel_cursor(&mut s, 4), None);
    // A tile: focus, and its cursor on the trend's newest point.
    let t = frame(&mut s, 1);
    click(&mut s, mid(t));
    assert_eq!(focused(&mut s), Some(1));
    let st = state(&mut s);
    let points = st.borrow().panes[&s.win]
        .dash
        .as_ref()
        .unwrap()
        .tile_points
        .clone();
    assert!(points[1].is_some(), "{points:?}");
}

#[test]
fn tab_and_shift_tab_go_round_the_panels_in_reading_order() {
    let mut s = health();
    keys(&mut s.h, "<Tab>");
    assert_eq!(focused(&mut s), Some(0));
    keys(&mut s.h, "<Tab><Tab><Tab><Tab>");
    assert_eq!(focused(&mut s), Some(4));
    keys(&mut s.h, "<Tab><Tab>");
    assert_eq!(focused(&mut s), Some(0), "round again");
    keys(&mut s.h, "<S-Tab>");
    assert_eq!(focused(&mut s), Some(5));
}

#[test]
fn a_brush_on_a_linked_chart_zooms_every_linked_panel_as_one_undo_step() {
    let mut s = health();
    let p = plot_of(&mut s, 4);
    let y = p.y + p.h * 0.5;
    let (a, b) = (p.x + p.w * 0.25, p.x + p.w * 0.5);
    let before = time(&mut s).unwrap();
    gesture(&mut s, &[(a, y), (a + 20.0, y), (b, y)]);
    settle(&mut s);
    let (t0, t1) = time(&mut s).unwrap();
    assert!((t1 - t0) < (before.1 - before.0) * 0.4, "{t0} {t1}");
    assert_eq!(undoable(&mut s), 1);
    // The other chart shows the same time.
    let x0 = x_of(&mut s, 5, t0);
    let p5 = plot_of(&mut s, 5);
    assert!((x0 - p5.x).abs() < 1.0, "{x0} vs {}", p5.x);
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!(time(&mut s), Some(before));
    keys(&mut s.h, "<C-r>");
    settle(&mut s);
    assert_eq!(time(&mut s), Some((t0, t1)));
}

#[test]
fn the_wheel_pans_the_shared_time_when_the_rows_fit_and_cmd_wheel_zooms_it_one_step_a_burst() {
    let mut s = health();
    let before = time(&mut s).unwrap();
    let at = plot_mid(&mut s, 5);
    scroll(&mut s, at, (0.0, -3.0), Mods::default());
    scroll(&mut s, at, (0.0, -3.0), Mods::default());
    let (a, b) = time(&mut s).unwrap();
    assert!(b < before.1, "earlier: {b} < {}", before.1);
    assert!(
        ((b - a) - (before.1 - before.0)).abs() < 1.0,
        "the width kept"
    );
    settle(&mut s);
    assert_eq!(undoable(&mut s), 1, "one burst, one step");
    scroll(&mut s, at, (0.0, -4.0), CMD);
    let (c, d) = time(&mut s).unwrap();
    assert!(d - c < b - a, "zoomed in");
    settle(&mut s);
    assert_eq!(undoable(&mut s), 2);
}

#[test]
fn a_double_click_on_a_linked_plot_puts_the_time_back_to_its_range_and_on_a_title_full_size() {
    let mut s = health();
    let at = plot_mid(&mut s, 4);
    scroll(&mut s, at, (0.0, -4.0), CMD);
    settle(&mut s);
    let zoomed = time(&mut s).unwrap();
    assert!(zoomed.1 - zoomed.0 < 21_000.0);
    double_click(&mut s, at);
    settle(&mut s);
    let (a, b) = time(&mut s).unwrap();
    assert!((b - a - 21_600.0).abs() < 1e-6, "the range again");
    let f = frame(&mut s, 5);
    double_click(&mut s, (f.x + 30.0, f.y + 22.0));
    let full = {
        let st = state(&mut s);
        let st = st.borrow();
        st.panes[&s.win].dash.as_ref().unwrap().view.full
    };
    assert_eq!(full, Some(5));
    keys(&mut s.h, "<Esc>");
    let full = {
        let st = state(&mut s);
        let st = st.borrow();
        st.panes[&s.win].dash.as_ref().unwrap().view.full
    };
    assert_eq!(full, None, "Esc leaves full size");
}

#[test]
fn f_shows_the_focused_panel_full_size_and_back() {
    let mut s = health();
    let errors = keys_errors(&mut s.h, "f");
    assert!(errors.iter().any(|e| e.contains("focus")), "{errors:?}");
    keys(&mut s.h, "<Tab><Tab><Tab><Tab><Tab>f");
    let f = frame(&mut s, 4);
    assert!(f.w > 800.0 && f.h > 400.0, "{f:?}");
    keys(&mut s.h, "f");
    let f = frame(&mut s, 4);
    assert!(f.w < 500.0, "{f:?}");
}

#[test]
fn keys_act_on_the_focused_panel_and_a_linked_zoom_moves_every_linked_panel() {
    let mut s = health();
    keys(&mut s.h, "<Tab><Tab><Tab><Tab><Tab>");
    keys(&mut s.h, "l");
    assert!(panel_cursor(&mut s, 4).is_some());
    let before = time(&mut s).unwrap();
    keys(&mut s.h, "+");
    settle(&mut s);
    let (a, b) = time(&mut s).unwrap();
    assert!(b - a < before.1 - before.0, "zoomed");
    assert_eq!(undoable(&mut s), 1);
    keys(&mut s.h, "0");
    settle(&mut s);
    let (a, b) = time(&mut s).unwrap();
    assert!(
        (b - a - 21_600.0).abs() < 1e-6,
        "0 is the time's range on a linked panel"
    );
    // A tile's h and l move its cursor along its trend.
    keys(&mut s.h, "<Tab><Tab><Tab>");
    assert_eq!(focused(&mut s), Some(1));
    keys(&mut s.h, "hh");
    let st = state(&mut s);
    let points = st.borrow().panes[&s.win]
        .dash
        .as_ref()
        .unwrap()
        .tile_points
        .clone();
    assert!(points[1].is_some(), "{points:?}");
}

#[test]
fn the_focusing_press_only_focuses_and_a_press_is_held_by_its_panel() {
    let mut s = health();
    s.h.send(die_core::Input::Focus(false));
    s.h.send(die_core::Input::Focus(true));
    let at = plot_mid(&mut s, 4);
    click(&mut s, at);
    assert_eq!(focused(&mut s), None, "the press that focused only focused");
    click(&mut s, at);
    assert_eq!(focused(&mut s), Some(4));
    // A drag that wanders onto the other chart stays the first's brush.
    let p = plot_of(&mut s, 4);
    let y = p.y + p.h / 2.0;
    let f5 = frame(&mut s, 5);
    pointer(
        &mut s,
        PointerKind::Press(Button::Left),
        (p.x + 20.0, y),
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Left),
        (p.x + 60.0, y),
        Mods::default(),
        0,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Left),
        (f5.x + 40.0, y),
        Mods::default(),
        0,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Left),
        (f5.x + 40.0, y),
        Mods::default(),
        1,
    );
    settle(&mut s);
    assert_eq!(focused(&mut s), Some(4));
    assert_eq!(undoable(&mut s), 1, "the brush to the plot's edge");
}

#[test]
fn a_heatmap_in_the_editor_hovers_clicks_moves_by_cell_brushes_whole_cells_and_shows_its_detail() {
    let text = std::fs::read_to_string(fixture("heatmap.json")).unwrap();
    let mut s = setup_with("heatmap.json", &text);
    s.h.set_window_size(176, 61);
    s.h.run_until_idle();
    let p = plot(&mut s);
    // The cell under the pointer is the hover; a click puts the cursor on it.
    let at = (p.x + p.w * 0.3, p.y + p.h * 0.4);
    mv(&mut s, at);
    assert!(
        matches!(hover(&mut s), Some(Cursor::Cell { .. })),
        "{:?}",
        hover(&mut s)
    );
    click(&mut s, at);
    let Some(Cursor::Cell { row, col }) = cursor(&mut s) else {
        panic!("a cell: {:?}", cursor(&mut s));
    };
    keys(&mut s.h, "l");
    assert_eq!(cursor(&mut s), Some(Cursor::Cell { row, col: col + 1 }));
    keys(&mut s.h, "j");
    let Some(Cursor::Cell { row: r2, .. }) = cursor(&mut s) else {
        panic!()
    };
    assert_ne!(r2, row, "j moves by row");
    // ⏎: the cell's detail.
    keys(&mut s.h, "<CR>");
    let open = state(&mut s).borrow().panes[&s.win].panel.is_some();
    assert!(open, "the detail panel");
    keys(&mut s.h, "<CR>");
    // A box brush zooms to whole cells.
    gesture(
        &mut s,
        &[
            (p.x + p.w * 0.2, p.y + p.h * 0.2),
            (p.x + p.w * 0.25, p.y + p.h * 0.25),
            (p.x + p.w * 0.5, p.y + p.h * 0.6),
        ],
    );
    s.h.advance(Duration::from_millis(400));
    let v = view(&mut s);
    let (x0, x1) = v.x.expect("x zoomed");
    let (y0, y1) = v.y.expect("y zoomed");
    for e in [x0, x1, y0, y1] {
        assert!((e - e.round()).abs() < 1e-9, "a cell's edge: {e}");
    }
    // t: the grid as a table.
    let errors = run(&mut s.h, "fig-table");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn bang_turns_a_failed_panels_error_figure_over_to_its_log_and_back() {
    let mut s = open_at(&fixture("dash/broken-panel.fig"), 176, 61);
    let failed: Vec<usize> = {
        let st = state(&mut s);
        let st = st.borrow();
        let m = st.dash.as_ref().unwrap().model.as_ref().unwrap();
        (0..m.panels.len())
            .filter(|&i| matches!(m.panels[i].state, die_fig::dashboard::PanelState::Failed(_)))
            .collect()
    };
    assert!(failed.len() >= 2, "{failed:?}");
    let i = failed[0];
    let errors = keys_errors(&mut s.h, "!");
    assert!(
        !errors.is_empty(),
        "nothing focused or hovered with an error"
    );
    // Hovered: `!` on the panel under the pointer.
    let f = frame(&mut s, i);
    mv(&mut s, (f.x + f.w / 2.0, f.y + f.h * 0.7));
    keys(&mut s.h, "!");
    let error = |s: &mut Setup| {
        let st = state(s);
        let st = st.borrow();
        st.panes[&s.win].dash.as_ref().unwrap().view.panels[i].error
    };
    assert!(matches!(
        error(&mut s),
        die_fig::plot::ErrorView::Log { .. }
    ));
    keys(&mut s.h, "!");
    assert_eq!(error(&mut s), die_fig::plot::ErrorView::Symbolic);
}

#[test]
fn a_tall_dashboard_scrolls_its_rows_and_a_burst_that_scrolls_the_page_never_pans_the_time() {
    let mut s = open_at(&fixture("dash/tall.fig"), 176, 61);
    let max = {
        let st = state(&mut s);
        let st = st.borrow();
        st.panes[&s.win]
            .dash
            .as_ref()
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .max_scroll
    };
    assert!(max > 0.0, "it overflows");
    let before = time(&mut s);
    let at = (440.0, 400.0);
    for _ in 0..6 {
        scroll(&mut s, at, (0.0, 2.0), Mods::default());
    }
    let scrolled = {
        let st = state(&mut s);
        let st = st.borrow();
        st.panes[&s.win].dash.as_ref().unwrap().view.scroll
    };
    assert!(scrolled > 0.0, "the page scrolled");
    assert_eq!(time(&mut s), before, "the burst kept to the page");
    // Across, the wheel is the panel's.
    settle(&mut s);
    let st = state(&mut s);
    let linked: Vec<usize> = st
        .borrow()
        .dash
        .as_ref()
        .unwrap()
        .model
        .as_ref()
        .unwrap()
        .linked();
    drop(st);
    if let Some(&i) = linked.iter().find(|&&i| {
        let st = state(&mut s);
        let st = st.borrow();
        st.dash.as_ref().unwrap().model.as_ref().unwrap().panels[i]
            .chart()
            .is_some()
    }) {
        let at = plot_mid(&mut s, i);
        scroll(&mut s, at, (-3.0, 0.0), Mods::default());
        settle(&mut s);
        assert_ne!(time(&mut s), before, "dx pans the time");
    }
}

#[test]
fn t_offers_the_ranges_and_a_custom_one_each_eased_and_one_undo_step() {
    let mut s = health();
    keys(&mut s.h, "T");
    keys(&mut s.h, "1h<CR>");
    settle(&mut s);
    let (a, b) = time(&mut s).unwrap();
    assert!((b - a - 3600.0).abs() < 1e-6, "{a} {b}");
    assert_eq!(undoable(&mut s), 1);
    keys(&mut s.h, "T");
    keys(&mut s.h, "08:00-09:30<CR>");
    settle(&mut s);
    let (a, b) = time(&mut s).unwrap();
    assert!((b - a - 5400.0).abs() < 1e-6, "{a} {b}");
    let chip = {
        let st = state(&mut s);
        let st = st.borrow();
        let dp = st.panes[&s.win].dash.as_ref().unwrap();
        dp.layout
            .as_ref()
            .unwrap()
            .header
            .chip
            .as_ref()
            .unwrap()
            .text
            .text
            .clone()
    };
    assert_eq!(chip, "08:00 – 09:30");
    // A click on the chip opens the same ranges.
    let r = {
        let st = state(&mut s);
        let st = st.borrow();
        let dp = st.panes[&s.win].dash.as_ref().unwrap();
        dp.layout
            .as_ref()
            .unwrap()
            .header
            .chip
            .as_ref()
            .unwrap()
            .rect
    };
    click(&mut s, mid(r));
    assert!(s.h.prompt_text().is_some(), "the ranges");
    keys(&mut s.h, "all<CR>");
    settle(&mut s);
    assert_eq!(time(&mut s), None, "all of the data");
}

#[test]
#[ignore = "speed"]
fn a_hover_move_on_health_repaints_within_eight_ms() {
    let mut s = health();
    let p = plot_of(&mut s, 4);
    let moves = 40;
    presents(&mut s.h);
    let t0 = std::time::Instant::now();
    for i in 0..moves {
        let x = p.x + p.w * (0.1 + 0.02 * i as f32);
        mv(&mut s, (x, p.y + p.h * 0.5));
    }
    let each = t0.elapsed() / moves;
    // What each move presented: the overlay's union of the card and the linked strips alone.
    let (base, over) = buffers(&mut s);
    let calls = presents(&mut s.h);
    assert!(
        calls.iter().all(|(id, _)| *id == over),
        "the base wasn't drawn again"
    );
    assert!(!calls.iter().any(|(id, _)| *id == base));
    eprintln!(
        "health.fig: {each:?} a hover move, {} presents",
        calls.len()
    );
    assert!(each < Duration::from_millis(8), "{each:?} a move");
}

/// Wait, on the real clock, for the commands to print, moving the fake one on so the poll runs,
/// until `done` or about 8 s.
fn wait_for(s: &mut Setup, mut done: impl FnMut(&mut Setup) -> bool) -> bool {
    for _ in 0..160 {
        if done(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
        s.h.advance(Duration::from_millis(40));
    }
    done(s)
}

/// Whether panel `i` of the dashboard draws.
fn ready(s: &mut Setup, i: usize) -> bool {
    let st = state(s);
    let st = st.borrow();
    let m = st.dash.as_ref().and_then(|d| d.model.as_ref());
    m.is_some_and(|m| m.panels.get(i).is_some_and(|p| p.ready()))
}

/// The commands' process groups now running.
fn pids(s: &mut Setup) -> Vec<u32> {
    let st = state(s);
    let st = st.borrow();
    let Some(l) = st.live.as_ref() else {
        return vec![];
    };
    l.keys().iter().filter_map(|k| l.status(k)?.pid).collect()
}

/// Whether a process is still there.
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn a_live_dashboard_asks_before_it_runs_streams_its_rows_and_stops_them_on_quit() {
    let mut s = open_at(&fixture("live/live.fig"), 176, 61);
    let screen = s.h.screen();
    assert!(
        screen.contains("Stream \"./ticker.sh\" for live.fig? (y/n)"),
        "{screen}"
    );
    assert_eq!(
        pids(&mut s),
        Vec::<u32>::new(),
        "nothing runs before the yes"
    );
    keys(&mut s.h, "y<CR>");
    let screen = s.h.screen();
    assert!(
        screen.contains("Run \"./errors.sh\" every 5s for live.fig? (y/n)"),
        "{screen}"
    );
    keys(&mut s.h, "y<CR>");
    // The stream's chart and both stats draw once rows come; the run's bars too.
    assert!(
        wait_for(&mut s, |s| ready(s, 2) && ready(s, 0) && ready(s, 3)),
        "rows came"
    );
    let live = {
        let st = state(&mut s);
        let st = st.borrow();
        st.dash
            .as_ref()
            .unwrap()
            .model
            .as_ref()
            .unwrap()
            .live
            .clone()
    };
    assert!(live.is_some(), "the header says it's live");
    assert!(status(&mut s).contains("following"), "{}", status(&mut s));
    // New rows slide the shared time while it follows.
    let (_, end) = time(&mut s).unwrap();
    assert!(
        wait_for(&mut s, |s| time(s).is_some_and(|t| t.1 > end)),
        "it follows"
    );
    let running = pids(&mut s);
    assert!(!running.is_empty());
    // Your yes is kept by the session.
    s.h.shutdown();
    std::thread::sleep(Duration::from_millis(100));
    for pid in running {
        assert!(!alive(pid), "{pid} stopped at quit");
    }
}

#[test]
fn a_no_shows_not_run_on_its_panels_and_r_asks_again() {
    let mut s = open_at(&fixture("live/live.fig"), 176, 61);
    keys(&mut s.h, "n<CR>");
    keys(&mut s.h, "n<CR>");
    let said = |s: &mut Setup| {
        let st = state(s);
        let st = st.borrow();
        let m = st.dash.as_ref().unwrap().model.as_ref().unwrap();
        m.panels
            .iter()
            .filter(|p| match &p.state {
                die_fig::dashboard::PanelState::Failed(f) => f
                    .errors
                    .iter()
                    .any(|e| e.msg.contains("not run: you said no")),
                _ => false,
            })
            .count()
    };
    assert_eq!(said(&mut s), 4, "every panel of both commands");
    assert_eq!(pids(&mut s), Vec::<u32>::new());
    // The spec's own file is polled each second; no command is polled.
    assert!(
        s.h.next_timer()
            .is_some_and(|t| t >= Duration::from_secs(1)),
        "nothing runs, nothing polls: {:?}",
        s.h.next_timer()
    );
    keys(&mut s.h, "R");
    let screen = s.h.screen();
    assert!(screen.contains("Stream \"./ticker.sh\""), "{screen}");
    keys(&mut s.h, "<Esc>");
    keys(&mut s.h, "<Esc>");
    s.h.shutdown();
}

#[test]
fn panning_away_stops_following_and_g_follows_again() {
    let mut s = open_at(&fixture("live/live.fig"), 176, 61);
    keys(&mut s.h, "y<CR>y<CR>");
    assert!(wait_for(&mut s, |s| ready(s, 2)), "rows came");
    // Enough rows to pan back through.
    std::thread::sleep(Duration::from_millis(1500));
    s.h.advance(Duration::from_millis(40));
    keys(&mut s.h, "T");
    keys(&mut s.h, "-1s<CR>");
    settle(&mut s);
    let at = plot_mid(&mut s, 2);
    scroll(&mut s, at, (0.0, -2.0), Mods::default());
    settle(&mut s);
    let following = |s: &mut Setup| {
        let st = state(s);
        let st = st.borrow();
        st.panes[&s.win].dash.as_ref().unwrap().follow
    };
    assert!(!following(&mut s), "panned into the past");
    let parked = time(&mut s);
    std::thread::sleep(Duration::from_millis(300));
    s.h.advance(Duration::from_millis(40));
    assert_eq!(
        time(&mut s),
        parked,
        "new rows don't move a view that isn't following"
    );
    keys(&mut s.h, "<Tab><Tab><Tab>G");
    settle(&mut s);
    assert!(following(&mut s));
    s.h.shutdown();
}

#[test]
fn a_chart_of_a_stream_alone_asks_waits_quietly_and_draws_as_rows_come() {
    let dir = TempDir::new("live-chart");
    let ticker = std::fs::read_to_string(fixture("live/ticker.sh")).unwrap();
    let script = dir.path().join("ticker.sh");
    std::fs::write(&script, ticker).unwrap();
    std::process::Command::new("chmod")
        .args(["+x", &script.display().to_string()])
        .status()
        .unwrap();
    let spec = dir.path().join("rpm.fig");
    std::fs::write(
        &spec,
        r#"{"type": "line", "title": "rpm", "data": {"stream": "./ticker.sh"}, "x": "time", "y": "rpm"}"#,
    )
    .unwrap();
    let mut s = open_at(&spec, 176, 61);
    assert!(
        s.h.screen()
            .contains("Stream \"./ticker.sh\" for rpm.fig? (y/n)")
    );
    keys(&mut s.h, "y<CR>");
    let drawn = |s: &mut Setup| state(s).borrow().model.is_some();
    assert!(wait_for(&mut s, drawn), "rows came");
    assert!(status(&mut s).contains("1 series"), "{}", status(&mut s));
    s.h.shutdown();
}

/// The window as presented, base and overlay composited, as a PNG at `path`.
fn shoot(s: &mut Setup, path: &Path) {
    let (base, over) = buffers(s);
    let (aw, ah) = {
        let st = state(s);
        let st = st.borrow();
        st.panes[&s.win].area_px()
    };
    let (bw, _, b) = s.h.pixels(base).expect("the base");
    let (ow, _, o) = s.h.pixels(over).unwrap_or((bw, 0, vec![]));
    let mut out = vec![0u8; (aw * ah * 4) as usize];
    for y in 0..ah {
        for x in 0..aw {
            let bi = ((y * bw + x) * 4) as usize;
            let oi = ((y * ow + x) * 4) as usize;
            let i = ((y * aw + x) * 4) as usize;
            let a = o.get(oi + 3).copied().unwrap_or(0) as u32;
            for c in 0..3 {
                let top = o.get(oi + c).copied().unwrap_or(0) as u32;
                out[i + c] = (top + b[bi + c] as u32 * (255 - a) / 255).min(255) as u8;
            }
            out[i + 3] = 255;
        }
    }
    let png = die_draw::image::encode(aw, ah, &out).unwrap();
    std::fs::write(path, png).unwrap();
}

#[test]
#[ignore = "renders"]
fn renders_for_the_look() {
    let Some(dir) = std::env::var_os("DIE_FIG_RENDERS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    // The linked crosshair: the pointer over the left chart at 10:05.
    let mut s = health();
    let t = {
        let st = state(&mut s);
        let st = st.borrow();
        st.dash
            .as_ref()
            .unwrap()
            .model
            .as_ref()
            .unwrap()
            .newest()
            .unwrap()
            - 7200.0
            + 300.0
    };
    let x = x_of(&mut s, 4, t);
    let p = plot_of(&mut s, 4);
    mv(&mut s, (x, p.y + p.h * 0.35));
    shoot(&mut s, &dir.join("health-linked.png"));
    // A focused panel, its cursor at 10:05 and its card, the moment in the other chart.
    let mut s = health();
    let p5 = plot_of(&mut s, 5);
    let x = x_of(&mut s, 5, t);
    click(&mut s, (x, p5.y + p5.h * 0.5));
    s.h.send(die_core::Input::PointerLeft);
    shoot(&mut s, &dir.join("health-focused.png"));
    // Full size.
    let mut s = health();
    keys(&mut s.h, "<Tab><Tab><Tab><Tab><Tab>f");
    let p = plot_of(&mut s, 4);
    mv(&mut s, (p.x + p.w * 0.66, p.y + p.h * 0.4));
    shoot(&mut s, &dir.join("health-full.png"));
    // A tile hovered: its card.
    let mut s = health();
    let f = frame(&mut s, 0);
    mv(&mut s, (f.x + f.w * 0.3, f.y + f.h * 0.5));
    shoot(&mut s, &dir.join("health-tile-hover.png"));
    // live.fig after a few stream rows.
    let mut s = open_at(&fixture("live/live.fig"), 176, 61);
    keys(&mut s.h, "y<CR>y<CR>");
    assert!(
        wait_for(&mut s, |s| ready(s, 2) && ready(s, 3)),
        "rows came"
    );
    std::thread::sleep(Duration::from_millis(2500));
    s.h.advance(Duration::from_millis(40));
    let p = plot_of(&mut s, 2);
    mv(&mut s, (p.x + p.w * 0.8, p.y + p.h * 0.5));
    shoot(&mut s, &dir.join("live.png"));
    // The same, its time zoomed to the rows' own few seconds.
    std::thread::sleep(Duration::from_millis(3000));
    s.h.advance(Duration::from_millis(40));
    keys(&mut s.h, "T");
    keys(&mut s.h, "-6s<CR>");
    settle(&mut s);
    let p = plot_of(&mut s, 2);
    mv(&mut s, (p.x + p.w * 0.6, p.y + p.h * 0.5));
    shoot(&mut s, &dir.join("live-zoomed.png"));
    s.h.shutdown();
    // The dashboard's keys.
    let mut s = health();
    keys(&mut s.h, "?");
    shoot(&mut s, &dir.join("health-keys.png"));
    // A stat alone: a dashboard of one tile.
    let mut s = open_at(&fixture("tiles/requests.json"), 120, 30);
    let f = frame(&mut s, 0);
    mv(&mut s, (f.x + f.w * 0.8, f.y + f.h * 0.5));
    shoot(&mut s, &dir.join("stat-alone.png"));
    // A graph beside a heatmap, a column of two charts, a row of meters; the heatmap hovered.
    let mut s = open_at(&fixture("dash/ops.fig"), 176, 61);
    let f = frame(&mut s, 4);
    mv(&mut s, (f.x + f.w * 0.5, f.y + f.h * 0.55));
    shoot(&mut s, &dir.join("ops.png"));
    // A tall dashboard scrolled, and a broken one with a panel's log.
    let mut s = open_at(&fixture("dash/tall.fig"), 176, 61);
    for _ in 0..5 {
        scroll(&mut s, (440.0, 400.0), (0.0, 3.0), Mods::default());
    }
    settle(&mut s);
    shoot(&mut s, &dir.join("tall-scrolled.png"));
    let mut s = open_at(&fixture("dash/broken-panel.fig"), 176, 61);
    shoot(&mut s, &dir.join("broken-panel.png"));
}

#[test]
fn return_opens_the_focused_panels_detail_beside_the_dashboard_and_it_follows_the_cursor() {
    let mut s = health();
    let errors = keys_errors(&mut s.h, "<CR>");
    assert!(!errors.is_empty(), "nothing focused: {errors:?}");
    let at = plot_mid(&mut s, 4);
    click(&mut s, at);
    let wide = frame(&mut s, 5).right();
    keys(&mut s.h, "<CR>");
    let title = |s: &mut Setup| {
        let st = state(s);
        let st = st.borrow();
        st.panes[&s.win]
            .panel
            .as_ref()
            .map(|p| p.tree.title.clone())
    };
    let first = title(&mut s).expect("the detail panel");
    // The dashboard lays out in what the panel leaves.
    assert!(frame(&mut s, 5).right() < wide - 100.0);
    keys(&mut s.h, "lll");
    assert_ne!(title(&mut s), Some(first), "it follows the cursor");
    // Tab goes through the panels, then the detail panel's stop.
    keys(&mut s.h, "<Tab><Tab>");
    let focused_panel = state(&mut s).borrow().panes[&s.win].panel_focused();
    assert!(focused_panel, "after the last panel, the detail panel");
    keys(&mut s.h, "<Tab>");
    assert_eq!(focused(&mut s), Some(0));
    keys(&mut s.h, "<Esc>");
    assert!(state(&mut s).borrow().panes[&s.win].panel.is_none());
}

#[test]
fn search_finds_in_the_focused_panel_and_s_turns_its_settings() {
    let mut s = health();
    let at = plot_mid(&mut s, 4);
    click(&mut s, at);
    keys(&mut s.h, "/work<CR>");
    match panel_cursor(&mut s, 4) {
        Some(Cursor::Point { series, .. }) => assert_eq!(&*series.name, "worker"),
        c => panic!("{c:?}"),
    }
    keys(&mut s.h, "s");
    keys(&mut s.h, "markers<CR>");
    let toggled = {
        let st = state(&mut s);
        let st = st.borrow();
        st.dash.as_ref().unwrap().toggles[4]
    };
    assert_eq!(toggled.markers, Some(true), "{toggled:?}");
    let errors = run(&mut s.h, "fig-table");
    assert!(errors.is_empty(), "{errors:?}");
    let table = s.h.buffer_list().into_iter().find(|b| {
        s.h.buffer_name(*b)
            .is_some_and(|n| n.starts_with("*fig table"))
    });
    let text = s.h.text(table.unwrap()).unwrap().rope().to_string();
    assert!(
        text.starts_with("time\t"),
        "{}",
        &text[..40.min(text.len())]
    );
}

#[test]
fn backslash_v_on_a_dashboard_spec_shows_it_and_toggles_back_to_the_text() {
    let path = fixture("dash/health.fig");
    let mut h = harness();
    h.set_window_size(176, 61);
    h.run_until_idle();
    let src = h.open_file(&path).unwrap();
    h.show(src, die_core::ShowIn::Current).unwrap();
    h.run_until_idle();
    let win = h.focused().unwrap();
    // A .fig opens as its figure.
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src);
    let mut s = Setup { h, src, buf, win };
    assert!(state(&mut s).borrow().dash.is_some());
    let errors = run(&mut s.h, "fig-toggle");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(s.h.window_buffer(win), Some(src), "its text");
    let errors = run(&mut s.h, "fig-toggle");
    assert!(errors.is_empty(), "{errors:?}");
    assert_ne!(s.h.window_buffer(win), Some(src), "its figure again");
}

#[test]
fn a_live_figure_no_window_shows_stops_its_commands_and_its_timers() {
    let mut s = open_at(&fixture("live/live.fig"), 176, 61);
    keys(&mut s.h, "y<CR>y<CR>");
    assert!(
        wait_for(&mut s, |s| !pids(s).is_empty() && ready(s, 2)),
        "running"
    );
    let running = pids(&mut s);
    // The window goes back to the notes: no window shows the figure.
    keys(&mut s.h, "<C-^>");
    assert_ne!(s.h.window_buffer(s.win), Some(s.buf));
    std::thread::sleep(Duration::from_millis(1500));
    for pid in running {
        assert!(!alive(pid), "{pid} stopped");
    }
    assert_eq!(s.h.next_timer(), None, "no timers at rest");
    s.h.shutdown();
}

fn up(state_dir: &TempDir) -> die_core::Harness {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(die_vim::Vim::default()),
        Box::new(die_fig::Fig::with_zone(jiff::tz::TimeZone::UTC)),
    ];
    let config = die_core::Config {
        session: Some(state_dir.path().to_path_buf()),
        ..die_core::Config::default()
    };
    let mut h = die_core::Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(176, 61);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

#[test]
fn a_restart_keeps_the_time_the_focus_full_size_each_panels_view_and_your_yeses() {
    let state_dir = TempDir::new("dash-session");
    let mut h = up(&state_dir);
    let (src, win) = h.open_text("notes.txt", "notes\n").unwrap();
    h.run_until_idle();
    let args = Args {
        text: Some(fixture("dash/health.fig").display().to_string()),
        ..Args::default()
    };
    h.run("fig", &args).unwrap();
    h.run_until_idle();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup { h, src, buf, win };
    keys(&mut s.h, "T");
    keys(&mut s.h, "1h<CR>");
    let at = plot_mid(&mut s, 5);
    click(&mut s, at);
    keys(&mut s.h, "f");
    settle(&mut s);
    let (t, cursor) = (time(&mut s), panel_cursor(&mut s, 5));
    s.trust_yes();
    let mut h = s.h;
    h.hand_over_session().unwrap();
    drop(h);
    let mut h = up(&state_dir);
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup {
        h,
        src: buf,
        buf,
        win,
    };
    let st = state(&mut s);
    assert!(st.borrow().dash.is_some(), "the dashboard came back");
    drop(st);
    assert_eq!(time(&mut s), t);
    assert_eq!(focused(&mut s), Some(5));
    assert_eq!(panel_cursor(&mut s, 5), cursor);
    let full = state(&mut s).borrow().panes[&s.win]
        .dash
        .as_ref()
        .unwrap()
        .view
        .full;
    assert_eq!(full, Some(5));
    let kept = state(&mut s)
        .borrow()
        .trust
        .borrow()
        .command(Some(Path::new("/x/y.fig")), "./a.sh");
    assert_eq!(kept, die_fig::live::Answer::Yes, "your yes came back");
}

impl Setup {
    /// A yes to a made-up command, as the prompt would keep it.
    fn trust_yes(&mut self) {
        let st = state(self);
        st.borrow()
            .trust
            .borrow_mut()
            .allow_command(Some(Path::new("/x/y.fig")), "./a.sh");
        st.borrow_mut().trust_rev += 1;
    }
}

#[test]
fn a_stat_alone_is_a_dashboard_of_one_tile_with_its_card() {
    let mut s = open_at(&fixture("tiles/requests.json"), 120, 30);
    assert!(ready(&mut s, 0), "the tile draws");
    let f = frame(&mut s, 0);
    mv(&mut s, (f.x + f.w * 0.3, f.y + f.h * 0.5));
    assert_eq!(hovered(&mut s), Some(0));
    presents(&mut s.h);
    mv(&mut s, (f.x + f.w * 0.35, f.y + f.h * 0.5));
    click(&mut s, (f.x + f.w * 0.3, f.y + f.h * 0.5));
    assert_eq!(focused(&mut s), Some(0));
}
