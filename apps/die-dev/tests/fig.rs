//! fig through your config: it comes in through `die::Config.extensions`, `\v` on a CSV shows
//! its chart in the window on mono-dark's ground and goes back, the pointer's hover draws its
//! card in the overlay alone, `\v` in visual state charts the selection, `\hk` describes a
//! figure's key, `\z` works on a figure, mono-dark styles fig's chrome and not its data's
//! colours, and nothing of fig's runs before `\v` or after it's gone. And the whole round trip
//! as you'd make it: a CSV, `\v`, a hover bringing a series forward with its card, a brush, the
//! wheel, ⌘-wheel and `+`, an edit of the CSV in a split that the chart follows, and `\v` back.
//! And a `.fig` file through your config: it opens as its figure, `\v` flips it to its text and
//! back, and one that can't draw shows the symbolic error figure, its log a `!` away.

use die_test_cap as _;

use die::core::{
    Button, CellPixels, Harness, Input, KeyAction, Mods, PixelBufferId, PixelRect, PixelSinkCall,
    PointerEvent, PointerKind, WindowId, parse_keys,
};
use die_fig::Fig;
use die_fig::data::SeriesKey;
use die_fig::interact::Cursor;
use die_fig::surface::Shared;
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
