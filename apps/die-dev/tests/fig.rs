//! fig through your config: it comes in through `die::Config.extensions`, `\v` on a CSV shows
//! its chart in the window on mono-dark's ground and goes back, the pointer's hover draws its
//! card in the overlay alone, `\v` in visual state charts the selection, `\hk` describes a
//! figure's key, `\z` works on a figure, mono-dark styles fig's chrome and not its data's
//! colours, and nothing of fig's runs before `\v` or after it's gone. And the whole round trip
//! as you'd make it: a CSV, `\v`, a hover bringing a series forward with its card, a brush, the
//! wheel, ⌘-wheel and `+`, an edit of the CSV in a split that the chart follows, and `\v` back.
//! And a `.fig` file through your config: it opens as its figure, `\v` flips it to its text and
//! back, and one that can't draw shows the symbolic error figure, its log a `!` away. And a
//! graph: `\v` on a graph's JSON, a node's hover and its dwell, a click, the detail panel and a
//! yank through die-vim's kill ring, back to the text, and a `.fig` of a graph opening as it.
//! And a graph as you'd use it, start to end: services.json opened, `\v` and its layout's frame,
//! a hover's dwell with its card, a click, h j k l, a node dragged and pinned, `u` and `<C-r>`,
//! a pan, a ⌘-wheel zoom, the panel and Tab into it, `/` as it's typed and Esc, `t`, `0`, an edit
//! of the JSON the graph follows with the pin kept, and `\v` back. And a dashboard through your
//! config: health.fig opens as its dashboard, a hover on a linked chart, `Tab` to a panel, `f`
//! full size and back, `T` to the last hour, and `\v` to its text and back. And dashboards as
//! you'd use them, start to end, in a process with a HOME of its own: health.fig's panels
//! presented once their jobs land, a hover at 10:05 on the left chart drawing the moment in the
//! right one and each tile's mini crosshair, a click, Tab, `f` and back, `T` to the last hour, a
//! brush on both charts and `u`; broken-panel.fig's two bad panels beside four good ones, `!`
//! and back; live.fig asking first, ticker.sh's rows streaming in with the time following, a
//! pan stopping it and `G` again, and `\v` to the text with every process the figure ran gone.

use die_test_cap as _;

use die::core::{
    Button, CellPixels, Harness, Input, KeyAction, Mods, PixelBufferId, PixelRect, PixelSinkCall,
    PointerEvent, PointerKind, WindowId, parse_keys,
};
use die_fig::Fig;
use die_fig::dashboard::draw::focus_ring;
use die_fig::dashboard::{DashModel, PanelState};
use die_fig::data::SeriesKey;
use die_fig::figure::{TileHover, hover_at};
use die_fig::interact::Cursor;
use die_fig::plot::ErrorView;
use die_fig::scene::{Item, Paint, Rect, Stroke, Verb};
use die_fig::surface::Shared;
use die_fig::surface::dash::DashPane;
use die_fig::theme::Color;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Your cells on your panel: 19 by 40 pixels at 2x.
const CELL: CellPixels = CellPixels {
    width: 19,
    height: 40,
    scale: 2.0,
};

fn data() -> String {
    let mut s = String::from("time,api,worker\n");
    for i in 0..30 {
        s.push_str(&format!(
            "2026-11-21T06:{i:02}:00Z,{},{}\n",
            600 + i * 7 % 50,
            300 + i * 11 % 40
        ));
    }
    s
}

fn harness() -> Harness {
    let config = die_dev::config(vec![]);
    let names: Vec<&str> = config.extensions.iter().map(|e| e.name()).collect();
    assert!(
        names.contains(&"fig"),
        "your config's extensions: {names:?}"
    );
    let mut exts = die::built_in();
    exts.extend(config.extensions);
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.open_text("requests.csv", &data()).unwrap();
    h.set_window_size(120, 40);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

/// Type keys that must all work.
fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

/// The figure's two buffers, as the scene places them: base, then overlay.
fn placed(h: &mut Harness) -> Vec<(die::core::PixelBufferId, i8)> {
    let scene = h.scene();
    scene.placements.iter().map(|p| (p.buffer, p.z)).collect()
}

#[test]
fn leader_v_charts_a_csv_on_mono_darks_ground_and_goes_back() {
    let mut h = harness();
    assert_eq!(h.next_timer(), None, "nothing of fig's runs before \\v");
    assert_eq!(h.command_doc("fig-toggle").map(|d| d.1), Some("fig"));
    let src = h.focused_buffer().unwrap();
    keys(&mut h, "\\v");
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("requests.csv *fig*"));
    let placed = placed(&mut h);
    assert_eq!(placed.iter().map(|p| p.1).collect::<Vec<_>>(), vec![-2, -1]);
    // The whole window's width, and mono-dark's ground under the chart.
    let (w, ht, px) = h.pixels(placed[0].0).unwrap();
    let g = h.surface_area(h.focused().unwrap()).unwrap();
    assert_eq!(w, g.cols as u32 * CELL.width);
    assert!(g.cols >= 110, "the whole width: {}", g.cols);
    let at = ((ht - 4) * w + 4) as usize * 4;
    assert_eq!(&px[at..at + 4], &[0x1a, 0x1a, 0x1a, 255]);
    // Something is drawn on it: the chart's lines, in colours of their own.
    let inked = px
        .chunks_exact(4)
        .filter(|p| p[..3] != [0x1a, 0x1a, 0x1a])
        .count();
    assert!(inked > 1000, "{inked} px drawn");
    assert_eq!(h.next_timer(), None, "at rest, nothing runs");
    // A hover draws the overlay alone.
    h.fake_pixels().take_calls();
    let (r0, c0) = {
        let w0 = h.focused().unwrap();
        let p = h.placements().into_iter().find(|p| p.window == w0).unwrap();
        (p.row, p.col + g.text_x)
    };
    let (x, y) = (
        c0 as f32 * 19.0 + w as f32 / 2.0,
        r0 as f32 * 40.0 + ht as f32 / 2.0,
    );
    let errors = h.send(Input::Pointer(PointerEvent {
        kind: PointerKind::Move,
        row: r0 + (ht as usize / 2) / 40,
        col: c0 + (w as usize / 2) / 19,
        clicks: 0,
        mods: Mods::default(),
        px: Some((x, y)),
    }));
    assert!(errors.is_empty(), "{errors:?}");
    let presents: Vec<_> = h
        .fake_pixels()
        .take_calls()
        .into_iter()
        .filter_map(|c| match c {
            PixelSinkCall::Present(id, r) => Some((id, r)),
            _ => None,
        })
        .collect();
    assert!(!presents.is_empty(), "the card and crosshair");
    assert!(
        presents.iter().all(|(id, _)| *id == placed[1].0),
        "the overlay alone: {presents:?}"
    );
    // Back to the text, and the figure goes with everything it ran.
    keys(&mut h, "\\v");
    assert_eq!(h.focused_buffer(), Some(src));
    assert!(!h.is_live(fig));
    assert_eq!(h.next_timer(), None);
}

#[test]
fn leader_v_in_visual_state_charts_the_selection() {
    let mut h = harness();
    keys(&mut h, "Vjjjj\\v");
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("*fig: selection*"));
    keys(&mut h, "\\v");
    assert_eq!(
        h.buffer_name(h.focused_buffer().unwrap()),
        Some("requests.csv")
    );
}

#[test]
fn describe_key_on_a_figure_names_figs_commands_and_leader_z_works_there() {
    let mut h = harness();
    keys(&mut h, "\\v");
    for (k, cmd) in [
        ("l", "fig-right"),
        ("+", "fig-zoom-in"),
        ("\\v", "fig-toggle"),
    ] {
        let events = parse_keys(k).unwrap();
        assert!(
            matches!(
                h.describe_key(&events).unwrap().action,
                KeyAction::Command { ref name, .. } if name == cmd
            ),
            "{k}"
        );
    }
    // `\hk l` describes the key in `*help*`.
    keys(&mut h, "\\hkl");
    let help = h.find_buffer("*help*").expect("*help*");
    let text = h.text(help).unwrap().rope().to_string();
    assert!(text.contains("fig-right"), "{text}");
    // \z on a figure: the column your config centres, and back.
    let fig = h.find_buffer("requests.csv *fig*").unwrap();
    h.show(fig, die::core::ShowIn::Current).unwrap();
    h.run_until_idle();
    keys(&mut h, "\\z");
    keys(&mut h, "\\z");
}

#[test]
fn mono_dark_styles_figs_chrome_and_leaves_its_data_to_fig() {
    let t = die_dev::theme::mono_dark();
    let status = ["fig-good", "fig-warning", "fig-serious", "fig-critical"];
    for name in die_fig::faces() {
        let data = name.starts_with("fig-series-") || status.contains(name);
        assert_eq!(t.faces.contains_key(*name), !data, "{name}");
    }
    let fg = |name: &str| t.faces[name].fg;
    assert_eq!(fg("fig-grid"), Some(die::core::Rgb::hex("#2a2a2a")));
    assert_eq!(fg("fig-tick"), Some(die::core::Rgb::hex("#888888")));
    assert_eq!(fg("fig-tooltip"), Some(die::core::Rgb::hex("#252525")));
    assert_eq!(fg("fig-cursor"), Some(die::core::Rgb::hex("#eeeeee")));
    // The theme styles nothing fig didn't declare.
    let h = harness();
    assert!(
        !h.messages.iter().any(|m| m.contains("no one declared")),
        "{:?}",
        h.messages
    );
}

/// Every present since the last ask, by buffer.
fn presents(h: &mut Harness) -> Vec<(PixelBufferId, PixelRect)> {
    h.fake_pixels()
        .take_calls()
        .into_iter()
        .filter_map(|c| match c {
            PixelSinkCall::Present(id, r) => Some((id, r)),
            _ => None,
        })
        .collect()
}

/// A pointer event at a logical point of a window's area, on your cells.
fn pointer(
    h: &mut Harness,
    win: WindowId,
    kind: PointerKind,
    (lx, ly): (f32, f32),
    mods: Mods,
    clicks: u8,
) {
    let p = h
        .placements()
        .into_iter()
        .find(|p| p.window == win)
        .unwrap();
    let g = h.surface_area(win).unwrap();
    let (r0, c0) = (p.row, p.col + g.text_x);
    let scale = CELL.scale as f32;
    let (x, y) = (lx * scale, ly * scale);
    let errors = h.send(Input::Pointer(PointerEvent {
        kind,
        row: r0 + (y / CELL.height as f32) as usize,
        col: c0 + (x / CELL.width as f32) as usize,
        clicks,
        mods,
        px: Some((
            x + c0 as f32 * CELL.width as f32,
            y + r0 as f32 * CELL.height as f32,
        )),
    }));
    assert!(errors.is_empty(), "{kind:?} at ({lx}, {ly}): {errors:?}");
}

const CMD: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: false,
    super_: true,
};

/// A series' key, by its name.
fn key(st: &Shared, name: &str) -> SeriesKey {
    let st = st.borrow();
    let model = st.model.as_ref().expect("a model");
    model
        .series
        .iter()
        .find(|i| &*i.key.name == name)
        .map(|i| i.key.clone())
        .unwrap_or_else(|| panic!("no series {name}"))
}

/// Where a series' sample is in a window's area, in logical px, as last drawn.
fn sample(st: &Shared, win: WindowId, name: &str, row: usize) -> (f32, f32) {
    let st = st.borrow();
    let model = st.model.as_ref().expect("a model");
    let info = model.series.iter().find(|i| &*i.key.name == name).unwrap();
    let l = st.panes[&win].layout.as_ref().expect("drawn");
    (l.x.map(info.xs[row]), l.y.map(info.ys[row]))
}

/// A window's x domain, which every gesture below sets.
fn shown_x(st: &Shared, win: WindowId) -> (f64, f64) {
    st.borrow().panes[&win].view.x.expect("a domain set")
}

/// The plot's rectangle in a window, as last drawn.
fn plot_rect(st: &Shared, win: WindowId) -> die_fig::scene::Rect {
    st.borrow().panes[&win].layout.as_ref().expect("drawn").plot
}

#[test]
fn the_round_trip_a_csv_its_chart_hover_brush_wheel_zoom_an_edit_followed_and_back() {
    let mut h = harness();
    let src = h.focused_buffer().unwrap();
    let win = h.focused().unwrap();
    presents(&mut h);

    // `\v`: the chart, its pixels presented on the base.
    keys(&mut h, "\\v");
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.window_buffer(win), Some(fig));
    let st = h
        .with_ext(|f: &mut Fig, _| f.figure(fig))
        .unwrap()
        .expect("a figure");
    let (base, over) = st.borrow().panes[&win].buffers().expect("its two buffers");
    let p = presents(&mut h);
    assert!(
        p.iter().any(|(id, _)| *id == base),
        "the base presented: {p:?}"
    );
    let (_, _, px) = h.pixels(base).unwrap();
    let ground = [0x1a, 0x1a, 0x1a];
    let inked = px.chunks_exact(4).filter(|p| p[..3] != ground).count();
    assert!(inked > 1000, "{inked} px of chart");
    let (api, worker) = (key(&st, "api"), key(&st, "worker"));
    let tooltip = st.borrow().theme.tooltip;
    let scale = CELL.scale as f32;

    // A hover on api's line: the crosshair snaps to its sample, api comes forward with its row
    // marked in the card, worker fades under the wash, and only the overlay is presented.
    let (x, y) = sample(&st, win, "api", 10);
    pointer(
        &mut h,
        win,
        PointerKind::Move,
        (x + 1.0, y),
        Mods::default(),
        0,
    );
    {
        let s = st.borrow();
        let pane = &s.panes[&win];
        assert_eq!(
            pane.hover,
            Some(Cursor::Point {
                series: api.clone(),
                row: 10
            })
        );
        assert_eq!(pane.overlay().emphasis, Some(api.clone()), "api forward");
    }
    let p = presents(&mut h);
    assert!(!p.is_empty(), "the hover drawn");
    assert!(
        p.iter().all(|(id, _)| *id == over),
        "the overlay alone: {p:?}"
    );
    let (ow, _, opx) = h.pixels(over).unwrap();
    let card = opx
        .chunks_exact(4)
        .filter(|p| p[3] == 255 && p[..3] == [tooltip.0, tooltip.1, tooltip.2])
        .count();
    assert!(card > 200, "the card's ground: {card} px");
    let (wx, wy) = sample(&st, win, "worker", 25);
    let i = ((wy * scale) as u32 * ow + (wx * scale) as u32) as usize * 4;
    assert!(
        (120..=210).contains(&opx[i + 3]),
        "worker under the wash: {:?}",
        &opx[i..i + 4]
    );

    // A brush from api's 5th sample to its 20th zooms there, eased, and then nothing runs.
    let (full0, full1) = {
        let s = st.borrow();
        let m = s.model.as_ref().unwrap();
        let i = m.series.iter().find(|i| i.key == api).unwrap();
        (i.xs[0], i.xs[29])
    };
    let (x0, y0) = sample(&st, win, "api", 5);
    let (x1, _) = sample(&st, win, "api", 20);
    let left = Button::Left;
    pointer(
        &mut h,
        win,
        PointerKind::Press(left),
        (x0, y0),
        Mods::default(),
        1,
    );
    pointer(
        &mut h,
        win,
        PointerKind::Drag(left),
        (x0 + 12.0, y0),
        Mods::default(),
        0,
    );
    pointer(
        &mut h,
        win,
        PointerKind::Drag(left),
        (x1, y0),
        Mods::default(),
        0,
    );
    pointer(
        &mut h,
        win,
        PointerKind::Release(left),
        (x1, y0),
        Mods::default(),
        1,
    );
    let (a, b) = shown_x(&st, win);
    let slack = (full1 - full0) * 0.01;
    let (s5, s20) = (full0 + 5.0 * 60.0, full0 + 20.0 * 60.0);
    assert!(
        (a - s5).abs() < slack && (b - s20).abs() < slack,
        "brushed to {a}..{b}, not {s5}..{s20}"
    );
    assert!(st.borrow().panes[&win].easing(), "eased there");
    h.advance(Duration::from_millis(400));
    assert!(!st.borrow().panes[&win].easing());
    assert_eq!(h.next_timer(), None, "at rest after the ease");
    assert!(presents(&mut h).iter().any(|(id, _)| *id == base));

    // The wheel pans: down shows later, the span kept, never eased; the burst ends.
    let before = shown_x(&st, win);
    let r = plot_rect(&st, win);
    let mid = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let wheel = |dx, dy| PointerKind::Scroll { dx, dy };
    pointer(&mut h, win, wheel(0.0, 1.0), mid, Mods::default(), 0);
    let after = shown_x(&st, win);
    assert!(after.0 > before.0, "later: {before:?} to {after:?}");
    let (w0, w1) = (before.1 - before.0, after.1 - after.0);
    assert!((w1 - w0).abs() < w0 * 1e-9, "the span kept: {w0} {w1}");
    assert!(!st.borrow().panes[&win].easing(), "the wheel never eases");
    h.advance(Duration::from_millis(250));
    assert_eq!(h.next_timer(), None, "the burst over");

    // ⌘-wheel zooms in about the pointer, the value under it staying there.
    let r = plot_rect(&st, win);
    let at = (r.x + r.w * 0.25, r.y + r.h / 2.0);
    let under = |st: &Shared| {
        let s = st.borrow();
        s.panes[&win].layout.as_ref().unwrap().x.invert(at.0)
    };
    let v0 = under(&st);
    let before = shown_x(&st, win);
    pointer(&mut h, win, wheel(0.0, -4.0), at, CMD, 0);
    let after = shown_x(&st, win);
    assert!(
        after.1 - after.0 < before.1 - before.0,
        "zoomed in: {before:?} to {after:?}"
    );
    let px_secs = (after.1 - after.0) / f64::from(plot_rect(&st, win).w);
    let v1 = under(&st);
    assert!(
        (v1 - v0).abs() < px_secs,
        "{v0} kept under the pointer: {v1}"
    );
    h.advance(Duration::from_millis(250));
    assert_eq!(h.next_timer(), None);

    // `+` zooms in by a step, eased, and stops.
    let before = shown_x(&st, win);
    keys(&mut h, "+");
    assert!(st.borrow().panes[&win].easing(), "a key's zoom eases");
    h.advance(Duration::from_millis(400));
    assert!(!st.borrow().panes[&win].easing());
    let after = shown_x(&st, win);
    let ratio = (before.1 - before.0) / (after.1 - after.0);
    assert!((ratio - 1.25).abs() < 0.01, "one ZOOM_STEP: {ratio}");
    assert_eq!(h.next_timer(), None);

    // The CSV in a split beside the chart: an edit of the row in the middle of the view is
    // charted once the buffer is quiet, and the chart's y reaches the new value.
    keys(&mut h, "<C-w>v");
    let win2 = h.focused().unwrap();
    assert_ne!(win2, win);
    assert_eq!(h.window_buffer(win2), Some(fig));
    keys(&mut h, "\\v");
    assert_eq!(h.window_buffer(win2), Some(src), "the text in the split");
    assert_eq!(h.window_buffer(win), Some(fig), "the chart beside it");
    assert!(h.is_live(fig));
    let (shown0, shown1) = shown_x(&st, win);
    let row = (((shown0 + shown1) / 2.0 - full0) / 60.0).round() as usize;
    let builds = st.borrow().builds;
    let drawn = st.borrow().panes[&win].base_gen;
    presents(&mut h);
    keys(&mut h, &format!("{}G$ciw900<Esc>", row + 2));
    let line = h.text(src).unwrap().rope().to_string();
    let line = line.lines().nth(row + 1).unwrap().to_string();
    assert!(line.ends_with(",900"), "{line}");
    h.advance(Duration::from_millis(200));
    h.run_until_idle();
    h.scene();
    assert_eq!(st.borrow().builds, builds + 1, "charted once");
    {
        let s = st.borrow();
        let m = s.model.as_ref().unwrap();
        let w = m.series.iter().find(|i| i.key == worker).unwrap();
        assert_eq!(w.ys[row], 900.0, "the model has the edit");
        let pane = &s.panes[&win];
        assert!(pane.base_gen > drawn, "the base drawn again");
        assert_eq!(pane.view.x, Some((shown0, shown1)), "the view kept");
        let y = pane.layout.as_ref().unwrap().y.domain();
        assert!(y.1 >= 900.0, "y fits the edit: {y:?}");
    }
    assert!(
        presents(&mut h).iter().any(|(id, _)| *id == base),
        "the new chart presented"
    );
    assert_eq!(h.next_timer(), None, "at rest after the build");

    // `\v` back in the chart's window: the text in both, the figure gone, nothing running.
    keys(&mut h, "<C-w>w");
    assert_eq!(h.focused(), Some(win));
    keys(&mut h, "\\v");
    h.run_until_idle();
    assert_eq!(h.window_buffer(win), Some(src));
    assert_eq!(h.window_buffer(win2), Some(src));
    assert!(!h.is_live(fig));
    assert_eq!(h.next_timer(), None);
}

/// A directory of the test's own, made empty.
fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("die-dev-fig-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// A file opened in the focused window, as `:e` opens it.
fn open(h: &mut Harness, path: &std::path::Path) -> die::core::BufferId {
    let b = h.open_file(path).unwrap();
    h.show(b, die::core::ShowIn::Current).unwrap();
    h.run_until_idle();
    h.scene();
    b
}

#[test]
fn a_fig_file_opens_as_its_figure_and_leader_v_flips_it_to_its_text_and_back() {
    let d = dir("open");
    std::fs::write(d.join("requests.csv"), data()).unwrap();
    let spec = d.join("requests.fig");
    std::fs::write(
        &spec,
        "{\"type\": \"line\", \"title\": \"Requests\", \"data\": \"requests.csv\", \"x\": \"time\", \"y\": \"api\"}\n",
    )
    .unwrap();
    let mut h = harness();
    let src = open(&mut h, &spec);
    let fig = h.focused_buffer().unwrap();
    assert_eq!(
        h.buffer_name(fig),
        Some("requests.fig *fig*"),
        "opened as its figure"
    );
    let placed = placed(&mut h);
    assert_eq!(placed.iter().map(|p| p.1).collect::<Vec<_>>(), vec![-2, -1]);
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    assert!(
        st.borrow().model.is_some(),
        "drawn: {:?}",
        st.borrow().error
    );
    keys(&mut h, "\\v");
    assert_eq!(h.focused_buffer(), Some(src), "\\v: its text");
    keys(&mut h, "l");
    assert_eq!(h.focused_buffer(), Some(src), "and it stays text");
    keys(&mut h, "\\v");
    let again = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(again), Some("requests.fig *fig*"), "and back");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_fig_file_that_cannot_draw_shows_the_symbolic_figure_and_its_log() {
    let d = dir("broken");
    let spec = d.join("broken.fig");
    std::fs::write(
        &spec,
        "{\"type\": \"line\",\n \"data\": \"gone.csv\", \"x\": \"t\", \"y\": \"v\"}\n",
    )
    .unwrap();
    let mut h = harness();
    open(&mut h, &spec);
    let fig = h.focused_buffer().unwrap();
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    assert!(st.borrow().model.is_none());
    let win = h.focused().unwrap();
    let (w, ht) = st.borrow().panes[&win].size();
    assert_eq!(
        st.borrow().panes[&win].hit((w / 2.0, ht / 2.0)),
        die_fig::scene::Hit::ErrorFigure
    );
    // Drawn on mono-dark's ground, with something on it: the glyph and the words.
    let (base, _) = st.borrow().panes[&win].buffers().unwrap();
    let (_, _, px) = h.pixels(base).unwrap();
    let inked = px
        .chunks_exact(4)
        .filter(|p| p[..3] != [0x1a, 0x1a, 0x1a])
        .count();
    assert!(inked > 500, "{inked} px of the error figure");
    keys(&mut h, "!");
    assert_eq!(st.borrow().panes[&win].log, Some((0, 0)), "! shows the log");
    assert!(
        h.messages
            .iter()
            .any(|m| m.contains("broken.fig") && m.contains("no such file")),
        "said in *messages*: {:?}",
        h.messages
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// One of die-fig's graph fixtures, read at run time.
fn graph_fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/die-fig/examples/data/graphs")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// Where a node is in a window's area, as last drawn.
fn node_at(st: &Shared, win: WindowId, id: &str) -> (f32, f32) {
    let st = st.borrow();
    let g = st.graph.as_ref().expect("a graph");
    let i = g.index_of(id).expect("the node");
    st.panes[&win]
        .graph
        .as_ref()
        .expect("drawn")
        .node(i)
        .unwrap()
        .at
}

#[test]
fn a_graph_round_trip_its_hover_a_click_the_panel_a_yank_and_back() {
    let d = dir("graph");
    let path = d.join("services.json");
    std::fs::write(&path, graph_fixture("services.json")).unwrap();
    let mut h = harness();
    let src = open(&mut h, &path);
    keys(&mut h, "\\v");
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("services.json *fig*"));
    let placed = placed(&mut h);
    assert_eq!(placed.iter().map(|p| p.1).collect::<Vec<_>>(), vec![-2, -1]);
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    assert!(st.borrow().graph.is_some(), "{:?}", st.borrow().error);
    let win = h.focused().unwrap();
    // Hover on worker draws the overlay alone; after its dwell it's in focus.
    h.fake_pixels().take_calls();
    let at = node_at(&st, win, "worker");
    pointer(&mut h, win, PointerKind::Move, at, Mods::default(), 0);
    let p = presents(&mut h);
    assert!(
        !p.is_empty() && p.iter().all(|(id, _)| *id == placed[1].0),
        "{p:?}"
    );
    h.advance(Duration::from_millis(150));
    assert!(
        st.borrow().panes[&win].dwell.is_some(),
        "in focus after the dwell"
    );
    // A click puts the cursor there; ⏎ opens its panel, Tab gives it the keys, y copies.
    pointer(
        &mut h,
        win,
        PointerKind::Press(Button::Left),
        at,
        Mods::default(),
        1,
    );
    pointer(
        &mut h,
        win,
        PointerKind::Release(Button::Left),
        at,
        Mods::default(),
        1,
    );
    assert_eq!(
        st.borrow().panes[&win].cursor,
        Some(Cursor::Node("worker".into()))
    );
    keys(&mut h, "<CR><Tab>");
    let title = (st.borrow().panes[&win].panel.as_ref()).map(|p| p.tree.title.clone());
    assert_eq!(title.as_deref(), Some("worker"));
    keys(&mut h, "y");
    let r = h.register('"').map(|r| r.text.clone());
    assert_eq!(
        r.as_deref(),
        Some("worker"),
        "y copied the id, into the kill ring"
    );
    assert_eq!(
        h.register('1').map(|r| r.text.clone()).as_deref(),
        Some("worker")
    );
    keys(&mut h, "<Esc>");
    assert!(st.borrow().panes[&win].panel.is_none());
    // Back to the text, the figure gone with what it ran.
    keys(&mut h, "\\v");
    assert_eq!(h.focused_buffer(), Some(src));
    assert!(!h.is_live(fig));
    // A `.fig` holding a graph opens as its figure.
    let spec = d.join("services.fig");
    std::fs::write(&spec, graph_fixture("services.json")).unwrap();
    open(&mut h, &spec);
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("services.fig *fig*"));
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    assert!(st.borrow().graph.is_some());
    let _ = std::fs::remove_dir_all(&d);
}

// ------------------------------------------------------------- a graph as the owner uses it

/// A pixel of a buffer as last presented, at a logical point of the area.
fn pixel(h: &Harness, id: PixelBufferId, (x, y): (f32, f32)) -> [u8; 4] {
    let (w, ht, px) = h.pixels(id).expect("presented");
    let s = CELL.scale as f32;
    let (px_x, px_y) = (x * s, y * s);
    assert!(
        px_x >= 0.0 && px_y >= 0.0 && (px_x as u32) < w && (px_y as u32) < ht,
        "({x}, {y}) is off the buffer"
    );
    let i = (px_y as u32 * w + px_x as u32) as usize * 4;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// A press at a point and its release there.
fn click(h: &mut Harness, win: WindowId, at: (f32, f32)) {
    let none = Mods::default();
    pointer(h, win, PointerKind::Press(Button::Left), at, none, 1);
    pointer(h, win, PointerKind::Release(Button::Left), at, none, 1);
}

/// A press at the first point, drags through the rest and a release at the last.
fn drag(h: &mut Harness, win: WindowId, points: &[(f32, f32)]) {
    let none = Mods::default();
    let (first, last) = (points[0], points[points.len() - 1]);
    pointer(h, win, PointerKind::Press(Button::Left), first, none, 1);
    for p in &points[1..] {
        pointer(h, win, PointerKind::Drag(Button::Left), *p, none, 0);
    }
    pointer(h, win, PointerKind::Release(Button::Left), last, none, 1);
}

/// The place in a window's graph farthest from every node with no node or edge under it.
fn empty_spot(st: &Shared, win: WindowId) -> (f32, f32) {
    let st = st.borrow();
    let g = st.graph.as_ref().unwrap();
    let l = st.panes[&win].graph.as_ref().unwrap();
    let r = l.region;
    let mut best: Option<((f32, f32), f32)> = None;
    for i in 1..40 {
        for j in 1..20 {
            let p = (r.x + r.w * i as f32 / 40.0, r.y + r.h * j as f32 / 20.0);
            if die_fig::graph::hover(g, l, p.0, p.1).is_some()
                || l.hits.nearest_segment(p.0, p.1, 16.0).is_some()
            {
                continue;
            }
            let d = (l.nodes.iter().flatten())
                .map(|n| (n.at.0 - p.0).hypot(n.at.1 - p.1))
                .fold(f32::INFINITY, f32::min);
            if best.is_none_or(|(_, b)| d > b) {
                best = Some((p, d));
            }
        }
    }
    best.expect("an empty spot").0
}

/// The node a window's cursor is on.
fn cursor_node(st: &Shared, win: WindowId) -> String {
    match &st.borrow().panes[&win].cursor {
        Some(Cursor::Node(id)) => id.to_string(),
        other => panic!("the cursor is on no node: {other:?}"),
    }
}

/// Nothing of fig's under way in the window: no ease, no file poll, and two seconds on nothing
/// drawn. (The core's own tick looks at the open file every second, so the next timer isn't
/// fig's to say.)
fn at_rest(h: &mut Harness, st: &Shared, win: WindowId) {
    assert!(!st.borrow().panes[&win].easing(), "no ease");
    assert!(!st.borrow().polling(), "no file poll");
    let drawn = st.borrow().panes[&win].base_gen;
    presents(h);
    h.advance(Duration::from_secs(2));
    let p = presents(h);
    assert!(p.is_empty(), "nothing drawn at rest: {p:?}");
    assert_eq!(st.borrow().panes[&win].base_gen, drawn);
}

/// Whether two points are within `d` of each other.
fn near(a: (f32, f32), b: (f32, f32), d: f32) -> bool {
    (a.0 - b.0).abs() < d && (a.1 - b.1).abs() < d
}

#[test]
fn a_graph_as_you_use_it_hover_click_keys_drag_pan_zoom_panel_search_table_an_edit_and_back() {
    let d = dir("owner-graph");
    let path = d.join("services.json");
    std::fs::write(&path, graph_fixture("services.json")).unwrap();
    let mut h = harness();
    let src = open(&mut h, &path);
    let win = h.focused().unwrap();
    presents(&mut h);

    // `\v`: the layout runs in a job, and once it lands the base is presented with every node
    // where the layout put it, in its group's colour (platform, edge, data: slots 1, 2, 3).
    keys(&mut h, "\\v");
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("services.json *fig*"));
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    assert!(st.borrow().graph.is_some(), "{:?}", st.borrow().error);
    assert_eq!(st.borrow().builds, 1, "laid out once, in a job");
    let (base, over) = st.borrow().panes[&win].buffers().expect("its two buffers");
    let p = presents(&mut h);
    assert!(
        p.iter().any(|(id, _)| *id == base),
        "the base presented: {p:?}"
    );
    let theme = st.borrow().theme.clone();
    for (id, slot) in [("api", 0), ("web", 1), ("db", 2)] {
        let c = theme.series[slot];
        let at = node_at(&st, win, id);
        assert_eq!(
            pixel(&h, base, at)[..3],
            [c.0, c.1, c.2],
            "{id} drawn at {at:?} in its group's colour"
        );
    }
    at_rest(&mut h, &st, win);

    // A hover on worker: the overlay alone is presented; after its dwell worker is in focus,
    // the rest dims under the wash and its card is up.
    let worker = node_at(&st, win, "worker");
    let none = Mods::default();
    pointer(
        &mut h,
        win,
        PointerKind::Move,
        (worker.0 + 1.0, worker.1),
        none,
        0,
    );
    assert_eq!(
        st.borrow().panes[&win].hover,
        Some(Cursor::Node("worker".into()))
    );
    assert_eq!(st.borrow().panes[&win].dwell, None, "not yet in focus");
    let p = presents(&mut h);
    assert!(
        !p.is_empty() && p.iter().all(|(id, _)| *id == over),
        "the overlay alone: {p:?}"
    );
    h.advance(Duration::from_millis(150));
    {
        let s = st.borrow();
        let pane = &s.panes[&win];
        assert_eq!(pane.dwell, Some(Cursor::Node("worker".into())), "dwelt");
        let ov = pane.graph_overlay(s.graph.as_ref().unwrap());
        assert_eq!(ov.emphasis, Some(Cursor::Node("worker".into())));
    }
    let p = presents(&mut h);
    assert!(
        !p.is_empty() && p.iter().all(|(id, _)| *id == over),
        "the emphasis drawn in the overlay alone: {p:?}"
    );
    let (_, _, opx) = h.pixels(over).unwrap();
    let tooltip = theme.tooltip;
    let card = (opx.chunks_exact(4))
        .filter(|p| p[3] == 255 && p[..3] == [tooltip.0, tooltip.1, tooltip.2])
        .count();
    assert!(card > 400, "the card's ground: {card} px");
    let washed = (opx.chunks_exact(4))
        .filter(|p| (180..=200).contains(&p[3]))
        .count();
    assert!(washed > 300, "the rest under the wash: {washed} px");
    at_rest(&mut h, &st, win);

    // A click puts the cursor on worker.
    click(&mut h, win, worker);
    assert_eq!(cursor_node(&st, win), "worker");

    // h j k l go to the nearest node that way, and the view keeps it in sight.
    for (k, way) in [
        ("l", (1.0, 0.0)),
        ("j", (0.0, 1.0)),
        ("h", (-1.0, 0.0)),
        ("k", (0.0, -1.0)),
    ] {
        let from = cursor_node(&st, win);
        keys(&mut h, k);
        let to = cursor_node(&st, win);
        assert_ne!(to, from, "{k} moved from {from}");
        let (a, b) = (node_at(&st, win, &from), node_at(&st, win, &to));
        let along = (b.0 - a.0) * way.0 + (b.1 - a.1) * way.1;
        assert!(
            along > 0.0,
            "{k}: {to} at {b:?} lies that way of {from} at {a:?}"
        );
        h.advance(Duration::from_millis(400));
        let s = st.borrow();
        let region = s.panes[&win].graph.as_ref().unwrap().region;
        drop(s);
        let b = node_at(&st, win, &to);
        assert!(region.contains(b.0, b.1), "{k}: {to} in sight");
    }

    // mailer dragged to an empty place: it moves under the hand and pins, nothing else moves,
    // one undo step; u lets it go back, and <C-r> pins it there again.
    let from = node_at(&st, win, "mailer");
    let spot = empty_spot(&st, win);
    let db = node_at(&st, win, "db");
    let steps = st.borrow().panes[&win].undo.undoable();
    drag(
        &mut h,
        win,
        &[
            (from.0 + 1.0, from.1 + 1.0),
            (from.0 + 16.0, from.1 + 1.0),
            (spot.0 + 1.0, spot.1 + 1.0),
        ],
    );
    assert!(
        st.borrow().panes[&win].view.pinned.contains_key("mailer"),
        "pinned"
    );
    let pinned = node_at(&st, win, "mailer");
    assert!(
        near(pinned, spot, 1.5),
        "under the hand: {pinned:?} {spot:?}"
    );
    assert!(near(node_at(&st, win, "db"), db, 0.5), "nothing else moves");
    assert_eq!(st.borrow().panes[&win].undo.undoable(), steps + 1);
    keys(&mut h, "u");
    h.advance(Duration::from_millis(400));
    assert!(
        st.borrow().panes[&win].view.pinned.is_empty(),
        "u lets it go"
    );
    assert!(
        near(node_at(&st, win, "mailer"), from, 1.0),
        "back where it was"
    );
    keys(&mut h, "<C-r>");
    h.advance(Duration::from_millis(400));
    let pin = st.borrow().panes[&win].view.pinned.get("mailer").copied();
    assert!(pin.is_some(), "pinned again");
    assert!(near(node_at(&st, win, "mailer"), spot, 1.5));

    // A drag on the background, toward the middle so the hand stays in the window, pans 1:1.
    let spot = empty_spot(&st, win);
    let region = st.borrow().panes[&win].graph.as_ref().unwrap().region;
    let way = |at: f32, lo: f32, len: f32| if at > lo + len / 2.0 { -1.0 } else { 1.0 };
    let (dx, dy) = (
        30.0 * way(spot.0, region.x, region.w),
        12.0 * way(spot.1, region.y, region.h),
    );
    let api = node_at(&st, win, "api");
    drag(
        &mut h,
        win,
        &[
            spot,
            (spot.0 + dx / 3.0, spot.1 + dy / 3.0),
            (spot.0 + dx, spot.1 + dy),
        ],
    );
    let now = node_at(&st, win, "api");
    assert!(
        near((now.0 - dx, now.1 - dy), api, 1.0),
        "1:1 by ({dx}, {dy}): {api:?} → {now:?}"
    );
    assert!(st.borrow().panes[&win].view.pinned.contains_key("mailer"));

    // ⌘-wheel zooms in about the pointer: db stays under it; the wheel never eases.
    let at = node_at(&st, win, "db");
    let span = |st: &Shared| st.borrow().panes[&win].view.x.map(|x| x.1 - x.0).unwrap();
    let before = span(&st);
    for _ in 0..2 {
        pointer(
            &mut h,
            win,
            PointerKind::Scroll { dx: 0.0, dy: -2.0 },
            at,
            CMD,
            0,
        );
    }
    assert!(span(&st) < before, "zoomed in");
    assert!(
        near(node_at(&st, win, "db"), at, 1.0),
        "db kept under the pointer"
    );
    assert!(!st.borrow().panes[&win].easing(), "the wheel never eases");
    at_rest(&mut h, &st, win);

    // ⏎ on worker: its detail panel beside the graph, which lays out in what's left; Tab gives
    // the panel the keys, and j moves its cursor, presenting the panel's rectangle alone.
    let worker = node_at(&st, win, "worker");
    click(&mut h, win, worker);
    presents(&mut h);
    keys(&mut h, "<CR>");
    let rect = {
        let s = st.borrow();
        let pane = &s.panes[&win];
        let panel = pane.panel.as_ref().expect("the panel open");
        assert_eq!(panel.tree.title, "worker");
        assert!(!panel.state.focused);
        let (fw, _) = pane.figure_size();
        assert!(fw < pane.size().0, "{fw} of {}", pane.size().0);
        assert!(pane.graph.as_ref().unwrap().size.0 <= fw);
        pane.panel_rect().unwrap()
    };
    assert!(presents(&mut h).iter().any(|(id, _)| *id == base));
    let ground = theme.surface;
    let (bw, _, bpx) = h.pixels(base).unwrap();
    let s = CELL.scale as f32;
    let (x0, x1) = ((rect.x * s) as u32 + 2, (rect.right() * s) as u32 - 2);
    let (y0, y1) = ((rect.y * s) as u32 + 2, (rect.bottom() * s) as u32 - 2);
    let (mut all, mut off) = (0, 0);
    for y in (y0..y1).step_by(3) {
        for x in (x0..x1).step_by(3) {
            let i = (y * bw + x) as usize * 4;
            all += 1;
            off += usize::from(bpx[i..i + 3] != [ground.0, ground.1, ground.2]);
        }
    }
    assert!(off * 10 > all * 9, "the panel's own ground: {off} of {all}");
    keys(&mut h, "<Tab>");
    assert!(
        st.borrow().panes[&win].panel_focused(),
        "Tab: the keys to the panel"
    );
    let row = st.borrow().panes[&win].panel.as_ref().unwrap().state.row;
    presents(&mut h);
    keys(&mut h, "j");
    assert!(st.borrow().panes[&win].panel.as_ref().unwrap().state.row > row);
    assert_eq!(cursor_node(&st, win), "worker", "the figure's cursor stays");
    let p = presents(&mut h);
    let left = (rect.x * s) as u32;
    assert!(
        !p.is_empty() && p.iter().all(|(id, r)| *id == base && r.x + 1 >= left),
        "the panel's rectangle alone: {p:?} (the panel from x {left})"
    );
    keys(&mut h, "<S-Tab>");
    assert!(!st.borrow().panes[&win].panel_focused());

    // `/a`: the best match takes the cursor as it's typed, every match lit, the panel
    // following; Esc puts the cursor back where it was.
    for log in h.type_keys("/a").unwrap() {
        assert!(log.errors.is_empty(), "{}: {:?}", log.key, log.errors);
    }
    assert_eq!(cursor_node(&st, win), "api", "the best match as typed");
    {
        let s = st.borrow();
        let pane = &s.panes[&win];
        assert!(
            pane.matches.len() >= 5,
            "the matches lit: {}",
            pane.matches.len()
        );
        assert_eq!(
            pane.panel.as_ref().unwrap().tree.title,
            "api",
            "the panel follows"
        );
    }
    keys(&mut h, "<Esc>");
    assert_eq!(cursor_node(&st, win), "worker", "Esc puts it back");
    {
        let s = st.borrow();
        let pane = &s.panes[&win];
        assert!(pane.matches.is_empty());
        assert_eq!(pane.found_node, None);
        assert_eq!(pane.panel.as_ref().unwrap().tree.title, "worker");
    }
    keys(&mut h, "<Esc>");
    assert!(
        st.borrow().panes[&win].panel.is_none(),
        "Esc closes the panel"
    );

    // `t`: the node and edge tables in a split, on worker's row; <C-w>c closes it.
    keys(&mut h, "t");
    let table = h
        .find_buffer("*fig table: services.json*")
        .expect("the tables");
    let tw = (h.window_list().into_iter())
        .find(|w| h.window_buffer(*w) == Some(table))
        .expect("shown");
    let text = h.text(table).unwrap().rope().to_string();
    assert!(text.starts_with("id\t"), "{text}");
    assert!(
        text.contains("\n\nfrom\tto"),
        "the edges after a blank line"
    );
    let t = h.text(table).unwrap();
    let line = t.line_of(t.cursor(tw).unwrap());
    assert!(
        text.lines()
            .nth(line)
            .is_some_and(|l| l.starts_with("worker\t")),
        "on worker's row: line {line}"
    );
    if h.focused() != Some(tw) {
        h.focus(tw);
    }
    keys(&mut h, "<C-w>c");
    assert_eq!(h.focused(), Some(win));
    assert_eq!(h.window_buffer(win), Some(fig));

    // `0` fits the whole graph, the pin with it, so a narrower window still shows it all.
    keys(&mut h, "0");
    h.advance(Duration::from_millis(400));
    assert_eq!(st.borrow().panes[&win].view.x, None, "fit");
    assert_eq!(
        st.borrow().panes[&win].view.pinned.get("mailer").copied(),
        pin
    );

    // The JSON in a split beside the graph: mailer moved to the data group is drawn in data's
    // colour once the buffer is quiet, and its pin and every place are kept.
    keys(&mut h, "<C-w>v");
    let win2 = h.focused().unwrap();
    assert_ne!(win2, win);
    keys(&mut h, "\\v");
    assert_eq!(h.window_buffer(win2), Some(src), "the text in the split");
    assert_eq!(h.window_buffer(win), Some(fig), "the graph beside it");
    let mailer = node_at(&st, win, "mailer");
    let api = node_at(&st, win, "api");
    let region = st.borrow().panes[&win].graph.as_ref().unwrap().region;
    assert!(
        region.contains(mailer.0, mailer.1),
        "{mailer:?} in {region:?}"
    );
    let builds = st.borrow().builds;
    keys(&mut h, "gg/\"mailer<CR>/platform<CR>ciwdata<Esc>");
    let line = h.text(src).unwrap().rope().to_string();
    let i = line.lines().position(|l| l.contains("\"mailer\"")).unwrap();
    assert_eq!(
        line.lines().nth(i + 1).unwrap().trim(),
        "\"group\": \"data\","
    );
    h.advance(Duration::from_millis(200));
    h.run_until_idle();
    h.scene();
    assert_eq!(st.borrow().builds, builds + 1, "followed once");
    {
        let s = st.borrow();
        let g = s.graph.as_ref().unwrap();
        let m = g.index_of("mailer").unwrap();
        assert_eq!(g.spec.nodes[m].group.as_deref(), Some("data"));
        assert_eq!(
            s.panes[&win].view.pinned.get("mailer").copied(),
            pin,
            "the pin kept"
        );
    }
    assert!(
        near(node_at(&st, win, "mailer"), mailer, 0.5),
        "where it was pinned"
    );
    assert!(
        near(node_at(&st, win, "api"), api, 0.5),
        "the rest in place"
    );
    let c = theme.series[2];
    assert_eq!(
        pixel(&h, base, node_at(&st, win, "mailer"))[..3],
        [c.0, c.1, c.2],
        "mailer in data's colour"
    );
    at_rest(&mut h, &st, win);

    // `\v` back in the graph's window: the text in both, the figure gone, nothing running.
    keys(&mut h, "<C-w>w");
    assert_eq!(h.focused(), Some(win));
    keys(&mut h, "\\v");
    h.run_until_idle();
    assert_eq!(h.window_buffer(win), Some(src));
    assert_eq!(h.window_buffer(win2), Some(src));
    assert!(!h.is_live(fig), "the figure gone with what it ran");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_dashboard_round_trip_its_linked_hover_tab_f_t_and_back_to_its_text() {
    let d = dir("dash");
    let from = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/die-fig/examples/data/dash");
    for f in ["health.fig", "requests.csv", "service.csv"] {
        std::fs::copy(from.join(f), d.join(f)).unwrap();
    }
    let mut h = harness();
    h.set_window_size(100, 34);
    h.run_until_idle();
    let src = open(&mut h, &d.join("health.fig"));
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("health.fig *fig*"));
    let win = h.focused().unwrap();
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    let panels = |st: &Shared| {
        let st = st.borrow();
        let m = st.dash.as_ref().and_then(|d| d.model.as_ref());
        m.map_or(0, |m| m.panels.iter().filter(|p| p.ready()).count())
    };
    assert_eq!(panels(&st), 6, "every panel draws");
    // A hover on the left chart: its card and the moment in the right one, the overlay alone.
    let plot = {
        let st = st.borrow();
        let dp = st.panes[&win].dash.as_ref().unwrap();
        let p = dp.layout.as_ref().unwrap().panel(4).unwrap();
        let (l, _) = p.chart().unwrap();
        let (ox, oy) = p.origin();
        die_fig::scene::Rect::new(l.plot.x + ox, l.plot.y + oy, l.plot.w, l.plot.h)
    };
    presents(&mut h);
    let at = (plot.x + plot.w / 2.0, plot.y + plot.h / 2.0);
    pointer(&mut h, win, PointerKind::Move, at, Mods::default(), 0);
    let (base, _) = st.borrow().panes[&win].buffers().unwrap();
    let calls = presents(&mut h);
    assert!(!calls.is_empty() && calls.iter().all(|(id, _)| *id != base));
    let time = |st: &Shared| st.borrow().panes[&win].dash.as_ref().unwrap().view.time;
    keys(&mut h, "<Tab><Tab><Tab><Tab><Tab>f");
    assert_eq!(
        st.borrow().panes[&win].dash.as_ref().unwrap().view.full,
        Some(4)
    );
    keys(&mut h, "fT1h<CR>");
    h.advance(Duration::from_millis(400));
    let (a, b) = time(&st).unwrap();
    assert!((b - a - 3600.0).abs() < 1e-6);
    assert!(h.screen().contains("panel 5/6"), "{}", h.screen());
    keys(&mut h, "\\v");
    assert_eq!(h.focused_buffer(), Some(src), "its text");
    keys(&mut h, "\\v");
    let again = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(again), Some("health.fig *fig*"), "and back");
    // With the dashboard's view as it was left: the last hour, panel 5 focused.
    let st = h
        .with_ext(|f: &mut Fig, _| f.figure(again))
        .unwrap()
        .unwrap();
    let (a, b) = time(&st).unwrap();
    assert!((b - a - 3600.0).abs() < 1e-6);
    assert_eq!(
        st.borrow().panes[&win].dash.as_ref().unwrap().view.focused,
        Some(4)
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ------------------------------------------------------- dashboards as the owner uses them

/// Set in the process the dashboard test runs itself again in, which has a HOME of its own: a
/// live command's PATH comes from a login shell, and that reads its HOME's profile.
const OWN_HOME: &str = "DIE_DEV_FIG_DASHBOARD_CHILD";

/// One of die-fig's examples copied into `d`, its mode with it (the scripts run).
fn example(d: &Path, rel: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/die-fig/examples/data")
        .join(rel);
    let to = d.join(from.file_name().unwrap());
    std::fs::copy(&from, &to).unwrap();
    to
}

/// What `f` makes of a window's dashboard and the figure's model.
fn on_dash<R>(st: &Shared, win: WindowId, f: impl FnOnce(&DashPane, &DashModel) -> R) -> R {
    let s = st.borrow();
    let dp = s.panes[&win]
        .dash
        .as_ref()
        .expect("a dashboard in the window");
    let m = (s.dash.as_ref().and_then(|d| d.model.as_ref())).expect("its model");
    f(dp, m)
}

/// Panel `i`'s frame in the area's logical px, as last laid out.
fn frame(st: &Shared, win: WindowId, i: usize) -> Rect {
    on_dash(st, win, |dp, _| {
        dp.layout
            .as_ref()
            .expect("laid out")
            .panel(i)
            .expect("a panel")
            .frame
    })
}

/// Panel `i`'s plot in the area's logical px.
fn panel_plot(st: &Shared, win: WindowId, i: usize) -> Rect {
    on_dash(st, win, |dp, _| {
        let p = dp.layout.as_ref().unwrap().panel(i).unwrap();
        let (l, _) = p.chart().expect("a chart");
        let (ox, oy) = p.origin();
        Rect::new(l.plot.x + ox, l.plot.y + oy, l.plot.w, l.plot.h)
    })
}

/// The area's x of time `t` on panel `i`'s chart, and the chart's x domain.
fn panel_x(st: &Shared, win: WindowId, i: usize, t: f64) -> (f32, (f64, f64)) {
    on_dash(st, win, |dp, _| {
        let p = dp.layout.as_ref().unwrap().panel(i).unwrap();
        let (l, _) = p.chart().expect("a chart");
        (l.x.map(t) + p.origin().0, l.x.domain())
    })
}

/// The dashboard's shared time.
fn shared_time(st: &Shared, win: WindowId) -> (f64, f64) {
    on_dash(st, win, |dp, _| dp.view.time.expect("a time"))
}

/// The presented pixels of a buffer inside `r` (the area's logical px) that `want` picks, at
/// their logical places.
fn pixels_in(
    h: &Harness,
    id: PixelBufferId,
    r: Rect,
    want: impl Fn(&[u8]) -> bool,
) -> Vec<(f32, f32)> {
    let (w, ht, px) = h.pixels(id).expect("presented");
    let s = CELL.scale as f32;
    let (x0, x1) = ((r.x * s).max(0.0) as u32, ((r.right() * s) as u32).min(w));
    let (y0, y1) = ((r.y * s).max(0.0) as u32, ((r.bottom() * s) as u32).min(ht));
    let mut out = vec![];
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize * 4;
            if want(&px[i..i + 4]) {
                out.push((x as f32 / s, y as f32 / s));
            }
        }
    }
    out
}

/// An opaque pixel of colour `c`.
fn is(c: Color) -> impl Fn(&[u8]) -> bool {
    move |p| p == [c.0, c.1, c.2, 255]
}

/// The middle of some pixels across.
fn mid_x(pts: &[(f32, f32)]) -> f32 {
    pts.iter().map(|p| p.0).sum::<f32>() / pts.len() as f32
}

/// A tile's trend line in its panel's scene: the points of the path stroked in `colour`.
fn trend_line(scene: &die_fig::scene::Scene, colour: Color) -> Vec<(f32, f32)> {
    (scene.items.iter())
        .find_map(|it| match it {
            Item::Path {
                path,
                fill: None,
                stroke:
                    Some(Stroke {
                        paint: Paint::Solid { color, .. },
                        ..
                    }),
            } if *color == colour => Some(
                (path.verbs.iter())
                    .filter_map(|v| match v {
                        Verb::Move(x, y) | Verb::Line(x, y) => Some((*x, *y)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("a trend line")
}

/// Whether panel `i`'s figure is in the presented base, not only its frame and title: a
/// tile's trend line and the dot on its newest point, a chart's line in each series' colour.
fn drawn(h: &Harness, st: &Shared, win: WindowId, base: PixelBufferId, i: usize) -> bool {
    let theme = st.borrow().theme.clone();
    let inside = frame(st, win, i).inflate(-3.0);
    let colours: Vec<Color> = on_dash(st, win, |_, m| match m.panels[i].chart() {
        Some(cm) => cm.series.iter().map(|s| cm.color(s, &theme)).collect(),
        None => vec![theme.series[0], theme.other],
    });
    colours
        .iter()
        .all(|c| pixels_in(h, base, inside, is(*c)).len() >= 8)
}

/// The union of what a buffer presented since the last ask, in logical px.
fn presented(h: &mut Harness, id: PixelBufferId) -> Option<Rect> {
    let s = CELL.scale as f32;
    (presents(h).into_iter())
        .filter(|(b, _)| *b == id)
        .map(|(_, r)| {
            let (x, y) = (r.x as f32 / s, r.y as f32 / s);
            Rect::new(x, y, r.width as f32 / s, r.height as f32 / s)
        })
        .reduce(|a, b| a.union(&b))
}

/// The process groups a live figure's commands have running.
fn running(st: &Shared) -> Vec<u32> {
    let s = st.borrow();
    let Some(l) = s.live.as_ref() else {
        return vec![];
    };
    l.keys().iter().filter_map(|k| l.status(k)?.pid).collect()
}

/// The rows a live figure's stream keeps.
fn streamed(st: &Shared) -> usize {
    let s = st.borrow();
    let Some(l) = s.live.as_ref() else {
        return 0;
    };
    let mut statuses = l.keys().into_iter().filter_map(|k| l.status(&k));
    statuses.find(|s| s.stream).map_or(0, |s| s.rows)
}

/// The dashboard's newest sample.
fn newest(st: &Shared) -> f64 {
    let s = st.borrow();
    let m = s.dash.as_ref().and_then(|d| d.model.as_ref()).unwrap();
    m.newest().expect("a newest sample")
}

/// Waits on the real clock for the commands to print, moving the fake one on so fig's poll
/// takes what they printed, until `done` or about 10 s.
fn wait_for(h: &mut Harness, mut done: impl FnMut(&mut Harness) -> bool) -> bool {
    for _ in 0..200 {
        if done(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
        let errors = h.advance(Duration::from_millis(40));
        assert!(errors.is_empty(), "{errors:?}");
    }
    done(h)
}

/// Every process now in one of these groups, as `ps` sees them, zombies too.
fn in_groups(groups: &BTreeSet<u32>) -> Vec<String> {
    let out = Command::new("/bin/ps")
        .args(["-ax", "-o", "pgid=,pid=,stat=,command="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines()
        .filter(|l| {
            let pgid = l.split_whitespace().next().and_then(|p| p.parse().ok());
            pgid.is_some_and(|p| groups.contains(&p))
        })
        .map(str::to_owned)
        .collect()
}

/// The processes this one started that are still there, zombies too (but the `ps` asking).
fn own_children() -> Vec<String> {
    let me = std::process::id().to_string();
    let out = Command::new("/bin/ps")
        .args(["-ax", "-o", "ppid=,pid=,stat=,command="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines()
        .filter(|l| l.split_whitespace().next() == Some(me.as_str()))
        .filter(|l| !l.contains("/bin/ps -ax"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn dashboards_as_you_use_them_health_a_broken_panel_and_live_data_start_to_end() {
    if std::env::var_os(OWN_HOME).is_some() {
        dashboards_start_to_end();
        return;
    }
    // Again in a process of its own, with a HOME, a ZDOTDIR and a SHELL of its own, so the
    // login shell that live commands get their PATH from reads no profile of yours.
    let home = dir("dash-home");
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "dashboards_as_you_use_them_health_a_broken_panel_and_live_data_start_to_end",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(OWN_HOME, "1")
        .env("HOME", &home)
        .env("ZDOTDIR", &home)
        .env("SHELL", "/bin/sh")
        .env_remove("ENV")
        .env_remove("BASH_ENV")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&home);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && said.contains("1 passed"),
        "the test in its own HOME: {said}"
    );
}

/// The test, in the process with its own HOME.
fn dashboards_start_to_end() {
    let d = dir("dash-owner");
    let health = example(&d, "dash/health.fig");
    for f in [
        "dash/requests.csv",
        "dash/service.csv",
        "dash/broken-panel.fig",
    ] {
        example(&d, f);
    }
    let mut h = harness();
    h.set_window_size(100, 34);
    h.run_until_idle();
    let win = h.focused().unwrap();
    let none = Mods::default();
    presents(&mut h);

    // health.fig opens as its dashboard. Its plan and each data file's read are jobs; once they
    // land, the base is presented over every panel with its figure in it: each tile's trend
    // and the dot on its newest point, each chart's lines in their series' colours.
    open(&mut h, &health);
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("health.fig *fig*"));
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    let theme = st.borrow().theme.clone();
    let (base, over) = st.borrow().panes[&win].buffers().expect("its two buffers");
    let n = on_dash(&st, win, |_, m| {
        assert!(m.panels.iter().all(|p| p.ready()), "every panel draws");
        m.panels.len()
    });
    assert_eq!(n, 6, "four tiles and two charts");
    let shown = presented(&mut h, base).expect("the base presented");
    for i in 0..n {
        let f = frame(&st, win, i);
        assert!(
            shown.contains(f.x, f.y) && shown.contains(f.right() - 0.5, f.bottom() - 0.5),
            "panel {i} at {f:?} presented in {shown:?}"
        );
        assert!(drawn(&h, &st, win, base, i), "panel {i}'s figure drawn");
    }
    // At rest nothing is drawn again; its files are looked at each second while it shows.
    h.advance(Duration::from_secs(2));
    let p = presents(&mut h);
    assert!(p.is_empty(), "nothing drawn at rest: {p:?}");

    // A hover on the left chart at 10:05: the overlay alone is presented, with the chart's
    // card, the linked moment in the right chart (a dashed hairline at 10:05 and the area's dot
    // on it) and each tile's mini crosshair on the part of its trend that holds 10:05.
    let (t, at) = on_dash(&st, win, |dp, m| {
        let (start, _) = dp.view.time.expect("the last six hours");
        let t = start + 4.0 * 3600.0 + 300.0;
        let cm = m.panels[4].chart().expect("a chart");
        assert!(cm.series[0].xs.contains(&t), "a sample at 10:05");
        let p = dp.layout.as_ref().unwrap().panel(4).unwrap();
        let (l, _) = p.chart().unwrap();
        let (ox, oy) = p.origin();
        (t, (l.x.map(t) + ox, l.plot.y + oy + l.plot.h * 0.5))
    });
    pointer(&mut h, win, PointerKind::Move, at, none, 0);
    let hover = on_dash(&st, win, |dp, m| {
        assert_eq!(dp.hovered, Some(4), "the left chart hovered");
        let hover = dp.subs[4].hover.clone();
        let Some(Cursor::Point { series, row }) = &hover else {
            panic!("a sample hovered: {hover:?}");
        };
        let cm = m.panels[4].chart().unwrap();
        let s = cm.series.iter().find(|s| &s.key == series).unwrap();
        assert_eq!(s.xs[*row as usize], t, "the hover on 10:05");
        hover
    });
    let p = presents(&mut h);
    assert!(
        !p.is_empty() && p.iter().all(|(id, _)| *id == over),
        "the overlay alone: {p:?}"
    );
    let card = pixels_in(&h, over, frame(&st, win, 4), is(theme.tooltip)).len();
    assert!(card > 400, "the card's ground in the left chart: {card} px");
    let (plot5, (x5, _)) = (panel_plot(&st, win, 5), panel_x(&st, win, 5, t));
    let line = pixels_in(&h, over, Rect::new(x5 - 1.0, plot5.y, 2.0, plot5.h), |p| {
        p[3] > 0
    });
    let k = CELL.scale as f32;
    let mut rows: Vec<u32> = line.iter().map(|p| (p.1 * k) as u32).collect();
    rows.dedup();
    let of = (plot5.h * k) as usize;
    assert!(
        rows.len() * 10 > of * 3 && rows.len() * 10 < of * 8,
        "a dashed line at 10:05 in the right chart: {} of {of} rows inked",
        rows.len()
    );
    let area = on_dash(&st, win, |_, m| {
        let cm = m.panels[5].chart().unwrap();
        cm.color(&cm.series[0], &theme)
    });
    let dot = pixels_in(
        &h,
        over,
        Rect::new(x5 - 6.0, plot5.y - 2.0, 12.0, plot5.h + 4.0),
        is(area),
    );
    assert!(dot.len() >= 10, "the area's dot on it: {} px", dot.len());
    for i in 0..4 {
        let f = frame(&st, win, i);
        let mini = pixels_in(&h, over, f, is(theme.series[0]));
        assert!(
            mini.len() >= 10,
            "tile {i}'s mini crosshair: {} px",
            mini.len()
        );
        // On the point of the trend's part that holds 10:05, where the base drew it.
        let want = on_dash(&st, win, |dp, _| {
            let p = dp.layout.as_ref().unwrap().panel(i).unwrap();
            let tm = p.tile().expect("a tile");
            let Some(TileHover::Trend { point, .. }) = hover_at(tm, t) else {
                panic!("tile {i} has 10:05 on its trend");
            };
            let line = trend_line(&p.scene, theme.other);
            assert_eq!(line.len(), tm.trend.len(), "tile {i}'s trend as drawn");
            (line[point].0 + p.frame.x, line[point].1 + p.frame.y)
        });
        let ys: Vec<(f32, f32)> = mini.iter().map(|p| (p.1, p.0)).collect();
        let got = (mid_x(&mini), mid_x(&ys));
        assert!(
            near(got, want, 1.0),
            "tile {i}'s crosshair at {got:?}, 10:05's point at {want:?}"
        );
    }

    // A click on the left chart focuses it and puts its cursor on 10:05; the status line says
    // which panel, and its frame takes the focus ring. Tab takes the focus to the right chart.
    let ring = |h: &Harness, i: usize| {
        let f = frame(&st, win, i);
        let edge = Rect::new(f.x - 1.5, f.y + f.h * 0.3, 3.0, f.h * 0.4);
        pixels_in(h, base, edge, is(focus_ring(&theme))).len()
    };
    assert_eq!((ring(&h, 4), ring(&h, 5)), (0, 0), "no ring before a focus");
    click(&mut h, win, at);
    on_dash(&st, win, |dp, _| {
        assert_eq!(dp.view.focused, Some(4));
        assert_eq!(dp.subs[4].cursor, hover, "the cursor on 10:05");
    });
    assert!(h.screen().contains("panel 5/6"), "{}", h.screen());
    assert!(
        ring(&h, 4) >= 10 && ring(&h, 5) == 0,
        "the left chart's ring"
    );
    keys(&mut h, "<Tab>");
    assert_eq!(on_dash(&st, win, |dp, _| dp.view.focused), Some(5));
    assert!(h.screen().contains("panel 6/6"), "{}", h.screen());
    assert!(
        ring(&h, 5) >= 10 && ring(&h, 4) == 0,
        "the right chart's ring"
    );

    // f shows the right chart over the whole dashboard, drawn there, and f again puts it back.
    let small = frame(&st, win, 5);
    let (aw, _) = st.borrow().panes[&win].size();
    presents(&mut h);
    keys(&mut h, "f");
    assert_eq!(on_dash(&st, win, |dp, _| dp.view.full), Some(5));
    let big = frame(&st, win, 5);
    assert!(big.w > aw * 0.9, "full size: {big:?} of {aw}");
    assert!(presented(&mut h, base).is_some(), "presented full size");
    assert!(drawn(&h, &st, win, base, 5), "drawn full size");
    keys(&mut h, "f");
    assert_eq!(on_dash(&st, win, |dp, _| dp.view.full), None);
    assert_eq!(frame(&st, win, 5), small, "back in its place");

    // T, then 1h: the last hour, eased, its chip saying so.
    keys(&mut h, "T");
    assert!(h.prompt_text().is_some(), "the ranges asked for");
    keys(&mut h, "1h<CR>");
    h.advance(Duration::from_millis(400));
    let hour = shared_time(&st, win);
    assert!((hour.1 - hour.0 - 3600.0).abs() < 1e-6, "{hour:?}");
    let chip = on_dash(&st, win, |dp, _| {
        let chip = dp.layout.as_ref().unwrap().header.chip.as_ref();
        chip.expect("the chip").text.text.clone()
    });
    assert_eq!(chip, "Last hour");

    // A brush across the left chart zooms both charts to what it covered, as one undo step,
    // and u puts the hour back.
    let steps = on_dash(&st, win, |dp, _| dp.undo.undoable());
    let p4 = panel_plot(&st, win, 4);
    let y = p4.y + p4.h * 0.5;
    let (a, b) = (p4.x + p4.w * 0.25, p4.x + p4.w * 0.6);
    drag(&mut h, win, &[(a, y), (a + 20.0, y), (b, y)]);
    h.advance(Duration::from_millis(400));
    let (t0, t1) = shared_time(&st, win);
    let want = 3600.0 * 0.35;
    assert!(
        ((t1 - t0) - want).abs() < want * 0.1,
        "zoomed to the brush: {} s",
        t1 - t0
    );
    for i in [4, 5] {
        let (_, (d0, d1)) = panel_x(&st, win, i, t0);
        assert!(
            (d0 - t0).abs() < 1.0 && (d1 - t1).abs() < 1.0,
            "panel {i} shows {d0}..{d1}, the time {t0}..{t1}"
        );
    }
    assert_eq!(on_dash(&st, win, |dp, _| dp.undo.undoable()), steps + 1);
    keys(&mut h, "u");
    h.advance(Duration::from_millis(400));
    assert_eq!(shared_time(&st, win), hour, "u: the hour again");
    assert_eq!(on_dash(&st, win, |dp, _| dp.undo.undoable()), steps);

    // broken-panel.fig in the same window: its two bad panels show their error figures and the
    // other four draw. ! on a bad one turns its figure over to its log and back, the others as
    // they were.
    open(&mut h, &d.join("broken-panel.fig"));
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("broken-panel.fig *fig*"));
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    let (base, _) = st.borrow().panes[&win].buffers().unwrap();
    let failed: Vec<usize> = on_dash(&st, win, |_, m| {
        let bad = |i: &usize| matches!(m.panels[*i].state, PanelState::Failed(_));
        (0..m.panels.len()).filter(bad).collect()
    });
    assert_eq!(failed, [1, 5], "the missing file and the mismatch");
    for i in [0, 2, 3, 4] {
        assert!(drawn(&h, &st, win, base, i), "panel {i} drawn");
    }
    let ground = theme.surface;
    for &i in &failed {
        let inside = frame(&st, win, i).inflate(-3.0);
        let ink = pixels_in(&h, base, inside, |p| {
            p[..3] != [ground.0, ground.1, ground.2]
        });
        assert!(
            ink.len() > 200,
            "panel {i}'s error figure: {} px",
            ink.len()
        );
    }
    keys(&mut h, "<Tab><Tab>");
    assert_eq!(on_dash(&st, win, |dp, _| dp.view.focused), Some(1));
    let looks = |h: &Harness| -> Vec<Vec<(f32, f32)>> {
        let inside = |i| frame(&st, win, i).inflate(-3.0);
        (0..6)
            .map(|i| {
                pixels_in(h, base, inside(i), |p| {
                    p[..3] != [ground.0, ground.1, ground.2]
                })
            })
            .collect()
    };
    let symbolic = looks(&h);
    keys(&mut h, "!");
    assert!(matches!(
        on_dash(&st, win, |dp, _| dp.view.panels[1].error),
        ErrorView::Log { .. }
    ));
    let log = looks(&h);
    assert_ne!(log[1], symbolic[1], "its log drawn in its frame");
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(log[i], symbolic[i], "panel {i} as it was");
    }
    keys(&mut h, "!");
    assert_eq!(
        on_dash(&st, win, |dp, _| dp.view.panels[1].error),
        ErrorView::Symbolic
    );
    assert_eq!(looks(&h), symbolic, "the symbolic figure again");

    // live.fig: nothing runs before your yes to each command. Then ticker.sh's rows stream
    // in, the chart and the stats draw them, and the shared time follows the newest.
    let l = dir("dash-live");
    let live = example(&l, "live/live.fig");
    for f in ["live/ticker.sh", "live/errors.sh"] {
        example(&l, f);
    }
    let src = open(&mut h, &live);
    let fig = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(fig), Some("live.fig *fig*"));
    let st = h.with_ext(|f: &mut Fig, _| f.figure(fig)).unwrap().unwrap();
    let (base, _) = st.borrow().panes[&win].buffers().unwrap();
    let screen = h.screen();
    assert!(
        screen.contains("Stream \"./ticker.sh\" for live.fig? (y/n)"),
        "{screen}"
    );
    assert!(running(&st).is_empty(), "nothing runs before the yes");
    keys(&mut h, "y<CR>");
    let screen = h.screen();
    assert!(
        screen.contains("Run \"./errors.sh\" every 5s for live.fig? (y/n)"),
        "{screen}"
    );
    keys(&mut h, "y<CR>");
    let mut groups = BTreeSet::new();
    let mut seen = |h: &mut Harness, st: &Shared| {
        groups.extend(running(st));
        h.scene();
    };
    let ready = |st: &Shared, i: usize| {
        let s = st.borrow();
        let m = s.dash.as_ref().and_then(|d| d.model.as_ref());
        m.is_some_and(|m| m.panels[i].ready())
    };
    assert!(
        wait_for(&mut h, |h| {
            seen(h, &st);
            ready(&st, 0) && ready(&st, 2) && ready(&st, 3) && streamed(&st) >= 5
        }),
        "rows came: {} streamed",
        streamed(&st)
    );
    assert!(drawn(&h, &st, win, base, 2), "the stream's chart drawn");
    let following = |st: &Shared| on_dash(st, win, |dp, _| dp.follow);
    let header = |st: &Shared| {
        on_dash(st, win, |_, m| {
            m.live.as_ref().map(|l| l.text.clone()).unwrap_or_default()
        })
    };
    assert!(following(&st), "it follows");
    assert!(header(&st).starts_with("live"), "{}", header(&st));
    assert!(h.screen().contains("following"), "{}", h.screen());
    let (rows, (_, end)) = (streamed(&st), shared_time(&st, win));
    assert!(
        wait_for(&mut h, |h| {
            seen(h, &st);
            streamed(&st) > rows && shared_time(&st, win).1 > end
        }),
        "new rows slide the time"
    );
    assert_eq!(shared_time(&st, win).1, newest(&st), "at the newest");

    // T to the last second, then the wheel on the chart pans back: following stops, the header
    // says paused, and new rows leave the time where it is. G follows again, at the newest.
    assert!(
        wait_for(&mut h, |h| {
            seen(h, &st);
            streamed(&st) >= 15
        }),
        "three seconds of rows"
    );
    keys(&mut h, "T");
    keys(&mut h, "-1s<CR>");
    h.advance(Duration::from_millis(400));
    let (a, b) = shared_time(&st, win);
    assert!((b - a - 1.0).abs() < 1e-6 && following(&st), "{a} {b}");
    let mid = {
        let p = panel_plot(&st, win, 2);
        (p.x + p.w / 2.0, p.y + p.h / 2.0)
    };
    let wheel = PointerKind::Scroll { dx: 0.0, dy: -2.0 };
    pointer(&mut h, win, wheel, mid, none, 0);
    h.advance(Duration::from_millis(400));
    assert!(!following(&st), "panned back");
    assert_eq!(header(&st), "paused");
    assert!(!h.screen().contains("following"), "{}", h.screen());
    let (parked, rows) = (shared_time(&st, win), streamed(&st));
    assert!(parked.1 < newest(&st));
    assert!(
        wait_for(&mut h, |h| {
            seen(h, &st);
            streamed(&st) > rows + 2
        }),
        "rows still come"
    );
    assert_eq!(shared_time(&st, win), parked, "and the time stays");
    keys(&mut h, "G");
    h.advance(Duration::from_millis(400));
    assert!(following(&st), "G follows again");
    let (a, b) = shared_time(&st, win);
    assert_eq!(b, newest(&st), "at the newest");
    assert!((b - a - 1.0).abs() < 1e-6, "its width kept");
    assert!(h.screen().contains("following"), "{}", h.screen());
    seen(&mut h, &st);
    assert!(!groups.is_empty(), "ticker.sh ran in a group of its own");

    // \v: live.fig's text in the window, the figure gone, and with it ticker.sh and every
    // process in its group, reaped (no zombie).
    keys(&mut h, "\\v");
    h.run_until_idle();
    assert_eq!(h.focused_buffer(), Some(src), "its text");
    assert!(!h.is_live(fig), "the figure gone");
    let start = std::time::Instant::now();
    while !in_groups(&groups).is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
        h.advance(Duration::from_millis(100));
    }
    let left = in_groups(&groups);
    assert!(left.is_empty(), "the commands' processes left: {left:?}");
    let children = own_children();
    assert!(
        children.is_empty(),
        "processes this test started: {children:?}"
    );
    h.shutdown();
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&l);
}
