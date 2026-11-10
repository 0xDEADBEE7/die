//! The whiteboard through your config: it comes in through `die::Config.extensions`, `\w` opens
//! its prompt in normal state, `\hk` describes its keys, its inks take mono-dark's colours, and a
//! board drawn with your text defaults is 80 columns centred until `\z`. Nothing is written
//! outside a temporary directory.

use die_test_cap as _;

use die::core::{Args, Harness, KeyAction, parse_keys};
use die_whiteboard::Whiteboard;
use std::path::PathBuf;

fn harness() -> Harness {
    let config = die_dev::config(vec![]);
    let names: Vec<&str> = config.extensions.iter().map(|e| e.name()).collect();
    assert_eq!(names, ["orchid", "whiteboard"], "your config's extensions");
    let mut exts = die::built_in();
    exts.extend(config.extensions);
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.open_text("notes", "my notes\n").unwrap();
    h.set_window_size(120, 40);
    h
}

fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-dev-whiteboard-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn leader_w_opens_the_prompt_and_a_board_has_your_keys_and_colours() {
    let dir = tree("leader");
    let mut h = harness();
    let notes = h.find_buffer("notes").unwrap();
    h.set_directory(notes, &dir);
    assert_eq!(
        h.next_timer(),
        None,
        "nothing of the whiteboard's runs before \\w"
    );
    for name in ["whiteboard", "whiteboard-export", "whiteboard-copy-image"] {
        assert_eq!(
            h.command_doc(name).map(|d| d.1),
            Some("whiteboard"),
            "{name}"
        );
    }
    keys(&mut h, "\\w");
    assert!(
        h.prompt_text().unwrap().ends_with('/'),
        "{:?}",
        h.prompt_text()
    );
    keys(&mut h, "arch<CR>");
    let w = h.focused().unwrap();
    let buf = h.window_buffer(w).unwrap();
    assert_eq!(h.buffer_name(buf), Some("*whiteboard: arch.svg*"));
    let g = h.surface_area(w).unwrap();
    assert_eq!(
        (g.text_x, g.text_w),
        (20, 80),
        "your text defaults, centred"
    );
    keys(&mut h, "\\z");
    h.run_until_idle();
    assert_eq!(h.surface_area(w).unwrap().text_w, 120);
    // `\hk r` describes the key in `*help*`.
    keys(&mut h, "\\hkr");
    let help = h.find_buffer("*help*").expect("*help*");
    let text = h.text(help).unwrap().rope().to_string();
    assert!(text.contains("whiteboard-tool-rect"), "{text}");
    assert!(text.contains("kind whiteboard"), "{text}");
    let events = parse_keys("2").unwrap();
    assert!(matches!(
        h.describe_key(&events).unwrap().action,
        KeyAction::Command { ref name, .. } if name == "whiteboard-ink"
    ));
    // Your leader's keys still reach their commands from a board.
    keys(&mut h, "\\]");
    assert!(h.prompt_text().is_some());
    keys(&mut h, "<Esc>");
    // mono-dark's ANSI red is ink 2.
    let inks = die_whiteboard::ink_rgbs(&h);
    assert_eq!(inks[1], die::core::Rgb::hex("#cc4444"));
    assert_eq!(
        inks[0],
        die::core::Rgb::hex("#cccccc"),
        "ink 1 is the default foreground"
    );
    // Nothing was written: no stroke, no file.
    assert!(!dir.join("arch.svg").exists());
    let wb = h
        .with_ext(|wb: &mut Whiteboard, _| wb.boards().len())
        .unwrap();
    assert_eq!(wb, 1);
    h.run("whiteboard-save", &Args::default()).unwrap();
    assert!(dir.join("arch.svg").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
