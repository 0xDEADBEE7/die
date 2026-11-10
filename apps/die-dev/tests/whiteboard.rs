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

/// The cells your DIE has at 16 pt on a 2x display, so a board makes its pixel buffer.
const CELL: die::core::CellPixels = die::core::CellPixels {
    width: 19,
    height: 40,
    scale: 2.0,
};

/// A pointer event at a board point of the focused window, the view at its default.
fn point(h: &mut Harness, kind: die::core::PointerKind, (bx, by): (f32, f32), clicks: u8) {
    let w = h.focused().unwrap();
    let p = h.placements().into_iter().find(|p| p.window == w).unwrap();
    let g = h.surface_area(w).unwrap();
    let (r0, c0) = (p.row, p.col + g.text_x);
    let (x, y) = (bx * CELL.scale as f32, by * CELL.scale as f32);
    let px = (
        x + c0 as f32 * CELL.width as f32,
        y + r0 as f32 * CELL.height as f32,
    );
    let errors = h.send(die::core::Input::Pointer(die::core::PointerEvent {
        kind,
        row: r0 + (y / CELL.height as f32).floor().max(0.0) as usize,
        col: c0 + (x / CELL.width as f32).floor().max(0.0) as usize,
        clicks,
        mods: die::core::Mods::default(),
        px: Some(px),
    }));
    assert!(errors.is_empty(), "{kind:?} at ({bx}, {by}): {errors:?}");
}

fn stroke(h: &mut Harness, points: &[(f32, f32)]) {
    use die::core::{Button, PointerKind};
    point(h, PointerKind::Press(Button::Left), points[0], 1);
    for p in &points[1..] {
        point(h, PointerKind::Drag(Button::Left), *p, 0);
    }
    point(
        h,
        PointerKind::Release(Button::Left),
        points[points.len() - 1],
        1,
    );
}

/// The focused board's pixel buffer: its placements in the scene and the alpha at a board point.
fn alpha_at(h: &mut Harness, (bx, by): (f32, f32)) -> u8 {
    let scene = h.scene();
    let p = scene
        .placements
        .first()
        .unwrap_or_else(|| panic!("a placement: {}", scene.snapshot()));
    let (w, _, px) = h.pixels(p.buffer).expect("the buffer");
    let (x, y) = ((bx * 2.0) as u32, (by * 2.0) as u32);
    px[((y * w + x) * 4 + 3) as usize]
}

/// A DIE on your config, keeping its session in `state`, with the whiteboard's boards back.
fn up(state: &std::path::Path) -> Harness {
    let mut config = die_dev::config(vec![]);
    config.core.session = Some(state.to_path_buf());
    let mut exts = die::built_in();
    exts.extend(config.extensions);
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.resume_session();
    h.set_window_size(120, 40);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

#[test]
fn a_board_drawn_in_pixels_through_your_config_exports_splits_and_survives_two_restarts() {
    let dir = tree("pixels");
    let state = tree("pixels-state");
    let mut h = up(&state);
    h.open_text("notes", "my notes\n").unwrap();
    let notes = h.find_buffer("notes").unwrap();
    h.set_directory(notes, &dir);
    keys(&mut h, "\\w");
    keys(&mut h, "talk<CR>");
    let w = h.focused().unwrap();
    let buf = h.window_buffer(w).unwrap();
    assert_eq!(h.buffer_name(buf), Some("*whiteboard: talk.svg*"));
    // One buffer the area's size, under the glyphs, transparent until something is drawn.
    let scene = h.scene();
    assert_eq!(scene.placements.len(), 1, "{}", scene.snapshot());
    let g = h.surface_area(w).unwrap();
    let p = scene.placements[0];
    assert_eq!((p.cols, p.rows, p.z), (g.text_w, g.rows, -1));
    let (bw, bh, _) = h.pixels(p.buffer).unwrap();
    assert_eq!(
        (bw, bh),
        (80 * 19, g.rows as u32 * 40),
        "your 80 columns at 19 by 40"
    );
    // A stroke in mono-dark's red, a rectangle, an arrow, a label.
    keys(&mut h, "2");
    stroke(
        &mut h,
        &[(10.0, 10.0), (40.0, 12.0), (80.0, 10.0), (120.0, 14.0)],
    );
    assert_eq!(alpha_at(&mut h, (80.0, 10.0)), 255);
    assert_eq!(alpha_at(&mut h, (80.0, 30.0)), 0);
    keys(&mut h, "1r");
    stroke(&mut h, &[(20.0, 60.0), (120.0, 120.0)]);
    keys(&mut h, "a");
    stroke(&mut h, &[(130.0, 90.0), (200.0, 90.0)]);
    keys(&mut h, "t");
    stroke(&mut h, &[(30.0, 80.0)]);
    keys(&mut h, "box<Esc>");
    assert_eq!(
        alpha_at(&mut h, (20.0, 90.0)),
        255,
        "the rectangle's left edge"
    );
    let (r0, c0) = (p.row, p.col + g.text_x);
    let row = h.scene().row_text(r0 + 4);
    assert!(row.contains("box"), "{row:?} at row 4 from {r0}, col {c0}");
    let wb = h
        .with_ext(|wb: &mut Whiteboard, _| wb.boards().len())
        .unwrap();
    assert_eq!(wb, 1);
    // Export: the PNG beside the file, from the model, at your 2x.
    keys(&mut h, "Epng<CR>");
    h.run_until_idle();
    let png = dir.join("talk.png");
    assert!(png.exists(), "{:?}", h.messages);
    let raster = die_whiteboard::export::decode_png(&std::fs::read(&png).unwrap()).unwrap();
    assert!(
        raster.width > 400 && raster.height > 200,
        "{}x{}",
        raster.width,
        raster.height
    );
    // `\z` widens the board to the window, and the buffer follows.
    keys(&mut h, "\\z");
    h.run_until_idle();
    assert_eq!(h.surface_area(w).unwrap().text_w, 120);
    let p = h.scene().placements[0];
    assert_eq!(p.cols, 120);
    let (bw, _, _) = h.pixels(p.buffer).unwrap();
    assert_eq!(bw, 120 * 19);
    assert_eq!(
        alpha_at(&mut h, (80.0, 10.0)),
        255,
        "drawn again at the new size"
    );
    // A split, and a second window on the board: two placements of one buffer.
    keys(&mut h, "<C-w>s");
    h.run_until_idle();
    assert_eq!(h.window_list().len(), 2);
    let scene = h.scene();
    assert_eq!(scene.placements.len(), 2, "{}", scene.snapshot());
    assert_eq!(scene.placements[0].buffer, scene.placements[1].buffer);
    // Two restarts, as `:rebuild` makes them: the board comes back drawn, and works.
    for round in 1..=2 {
        h.hand_over_session().unwrap();
        drop(h);
        h = up(&state);
        let buf = h
            .find_buffer("*whiteboard: talk.svg*")
            .expect("the board is back");
        assert_eq!(h.focused_buffer(), Some(buf), "round {round}");
        let scene = h.scene();
        assert_eq!(
            scene.placements.len(),
            1,
            "round {round}: {}",
            scene.snapshot()
        );
        assert_eq!(
            alpha_at(&mut h, (80.0, 10.0)),
            255,
            "round {round}: the stroke is back"
        );
        assert!(
            h.scene().row_text(r0 + 4).contains("box"),
            "round {round}: the label"
        );
        // It works: a stroke, undone and redone (the log is this session's, not the file's).
        let y = 200.0 + round as f32 * 20.0;
        stroke(&mut h, &[(10.0, y), (60.0, y)]);
        assert_eq!(alpha_at(&mut h, (30.0, y)), 255);
        keys(&mut h, "u");
        assert_eq!(alpha_at(&mut h, (30.0, y)), 0, "round {round}: undone");
        keys(&mut h, "<C-r>");
        assert_eq!(alpha_at(&mut h, (30.0, y)), 255);
        h.run("whiteboard-save", &Args::default()).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&state);
}
