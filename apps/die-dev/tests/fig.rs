//! fig through your config: it comes in through `die::Config.extensions`, `\v` runs its toggle
//! in normal and visual state, `\hk` describes the key, mono-dark styles its chrome and not its
//! data's colours, and nothing of it runs before `\v`. The figure itself comes later: for now
//! the toggle says so.

use die_test_cap as _;

use die::core::{Harness, KeyAction, parse_keys};

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
    h.open_text("data.csv", "time,a,b\n1,2,3\n2,4,6\n").unwrap();
    h.set_window_size(120, 40);
    h.run_until_idle();
    h
}

/// Type keys that must all work.
fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

fn said(h: &Harness) -> usize {
    h.messages
        .iter()
        .filter(|m| *m == "fig: not built yet")
        .count()
}

#[test]
fn leader_v_runs_figs_toggle_in_normal_and_visual_state() {
    let mut h = harness();
    assert_eq!(h.next_timer(), None, "nothing of fig's runs before \\v");
    assert_eq!(h.command_doc("fig-toggle").map(|d| d.1), Some("fig"));
    let events = parse_keys("\\v").unwrap();
    assert!(matches!(
        h.describe_key(&events).unwrap().action,
        KeyAction::Command { ref name, .. } if name == "fig-toggle"
    ));
    keys(&mut h, "\\v");
    assert_eq!(said(&h), 1, "{:?}", h.messages);
    keys(&mut h, "vj\\v");
    assert_eq!(said(&h), 2, "{:?}", h.messages);
    assert_eq!(h.next_timer(), None);
    // `\hk \v` describes the key in `*help*`.
    keys(&mut h, "<Esc>\\hk\\v");
    let help = h.find_buffer("*help*").expect("*help*");
    let text = h.text(help).unwrap().rope().to_string();
    assert!(text.contains("fig-toggle"), "{text}");
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
