//! fig through your config: it comes in through `die::Config.extensions`, `\v` on a CSV shows
//! its chart in the window on mono-dark's ground and goes back, the pointer's hover draws its
//! card in the overlay alone, `\v` in visual state charts the selection, `\hk` describes a
//! figure's key, `\z` works on a figure, mono-dark styles fig's chrome and not its data's
//! colours, and nothing of fig's runs before `\v` or after it's gone.

use die_test_cap as _;

use die::core::{
    CellPixels, Harness, Input, KeyAction, Mods, PixelSinkCall, PointerEvent, PointerKind,
    parse_keys,
};

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
