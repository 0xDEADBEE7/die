//! The jellyfish in a DIE buffer: the screen is the sea's own picture of the clock, on the
//! theme's background; a timer runs only while it swims; and the keys a buffer can't do without.

use die_test_cap as _;

use die_core::{
    Args, Config, Extension, Glyph, Harness, Input, Key, KeyEvent, KeyLog, Mods, PointerKind, Rgb,
    Settings, Style, Theme,
};
use die_jellyfish::sea::{Extras, Sea, Tones};
use die_jellyfish::{Jellyfish, NAME, TICK};
use die_vim::Vim;
use std::time::Duration;

/// Your theme's background and text, and a light theme's background.
const DARK: Rgb = Rgb(0x1a, 0x1a, 0x1a);
const TEXT: Rgb = Rgb(0xcc, 0xcc, 0xcc);
const LIGHT: Rgb = Rgb(0xf5, 0xf5, 0xf0);

/// The window's area, in cells.
const AREA: (usize, usize) = (120, 50);

/// die-vim and the jellyfish as your config has it, under a theme with your background and your
/// 80 centred columns, a text buffer on screen in a 120 x 50 window.
fn harness() -> Harness {
    let exts: Vec<Box<dyn Extension>> =
        vec![Box::new(Vim::default()), Box::new(Jellyfish::default())];
    let config = Config {
        theme: Some(Theme::default().face("default", Style::new().fg(TEXT).bg(DARK))),
        text_defaults: Settings {
            max_width: Some(80),
            center: true,
            ..Settings::default()
        },
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.open_text("notes", "notes\n").unwrap();
    h.set_window_size(AREA.0, AREA.1);
    h.run_until_idle();
    h
}

/// `jellyfish`, run to the end.
fn show(h: &mut Harness) {
    h.run("jellyfish", &Args::default()).unwrap();
    h.run_until_idle();
}

/// The name of the buffer the focused window shows.
fn showing(h: &Harness) -> &str {
    h.buffer_name(h.focused_buffer().unwrap()).unwrap()
}

/// A cell's letter and its colour, or None for a cell with nothing in it.
type Picture = Vec<Option<(char, Rgb)>>;

/// What the window shows, every cell of it on `paper`.
fn on_screen(h: &mut Harness, paper: Rgb) -> Picture {
    let scene = h.scene();
    let mut cells = vec![];
    for r in 0..AREA.1 {
        for cell in scene.row(r) {
            assert_eq!(cell.bg, paper, "row {r}");
            cells.push(match cell.glyph {
                Glyph::Char(' ') => None,
                Glyph::Char(c) => Some((c, cell.fg)),
                _ => panic!("{:?}", cell.glyph),
            });
        }
    }
    cells
}

/// What a new sea draws for the moment `frames` ticks in, under a theme with this background
/// and text: the picture the clock alone gives.
fn at_frame(frames: u32, paper: Rgb, text: Rgb) -> Picture {
    let hex = |Rgb(r, g, b): Rgb| u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    let extras = Extras {
        water: true,
        shoal: false,
    };
    let mut sea = Sea::new();
    let tones = Tones::for_theme(hex(paper), hex(text));
    let t = (TICK * frames).as_secs_f64();
    sea.draw(AREA.0, AREA.1, 0.5, tones, extras, t);
    let cell = |(&code, &ink): (&u16, &u32)| {
        let ink = Rgb((ink >> 16) as u8, (ink >> 8) as u8, ink as u8);
        (code != 0).then(|| (char::from_u32(code.into()).unwrap(), ink))
    };
    sea.glyphs().iter().zip(sea.inks()).map(cell).collect()
}

#[test]
fn the_screen_is_the_pure_frame_at_the_clock_full_width_on_the_themes_background() {
    let mut h = harness();
    show(&mut h);
    assert_eq!(showing(&h), NAME);
    let area = h.surface_area(h.focused().unwrap()).unwrap();
    assert_eq!(
        (area.text_x, area.text_w, area.rows),
        (0, AREA.0, AREA.1),
        "the window's whole width, where a file has 80 columns centred"
    );

    assert_eq!(h.advance(TICK * 90), Vec::<String>::new());
    let screen = on_screen(&mut h, DARK);
    assert!(screen.iter().flatten().count() > 400, "an animal");
    assert!(screen == at_frame(90, DARK, TEXT), "frame 90 of the sea");
    assert!(h.scene().cursors.is_empty(), "no cursor");

    // A light theme: its background from the next frame, and its tones at the next tick.
    h.set_theme(Theme::default().face("default", Style::new().fg(DARK).bg(LIGHT)));
    h.advance(TICK);
    assert!(
        on_screen(&mut h, LIGHT) == at_frame(91, LIGHT, DARK),
        "frame 91, in a light paper's tones"
    );
}

#[test]
fn it_swims_while_shown_rests_after_five_idle_minutes_wakes_at_a_key_and_leaves_no_timer() {
    let mut h = harness();
    assert_eq!(h.next_timer(), None, "nothing at init");
    show(&mut h);
    assert_eq!(h.next_timer(), Some(h.now() + TICK), "shown: it swims");
    h.advance(TICK * 90);

    // A file takes its window: the next tick is the last, and nothing runs while it's hidden.
    h.type_keys(":b notes<CR>").unwrap();
    h.advance(TICK);
    assert_eq!(h.next_timer(), None, "hidden");
    assert_eq!(h.advance(Duration::from_secs(60)), Vec::<String>::new());

    // Back in the window, by C-^ this time and not its own command, it goes on from frame 90,
    // the minute away not counted: no second opening.
    h.type_keys("<C-^>").unwrap();
    assert_eq!(showing(&h), NAME);
    assert_eq!(h.next_timer(), Some(h.now() + TICK), "shown again");
    h.advance(TICK);
    assert!(on_screen(&mut h, DARK) == at_frame(91, DARK, TEXT));

    // Five minutes without a key, counted from when it woke: 9,000 frames, and then a still
    // picture with no timer.
    h.advance(Duration::from_secs(300));
    assert_eq!(h.next_timer(), None, "at rest");
    let resting = on_screen(&mut h, DARK);
    assert!(resting == at_frame(9090, DARK, TEXT), "where it stopped");
    assert_eq!(h.advance(Duration::from_secs(60)), Vec::<String>::new());
    assert_eq!(h.next_timer(), None, "still at rest");
    assert!(on_screen(&mut h, DARK) == resting, "a still picture");

    // The end of a key's command wakes it, where it rested.
    let log = h.type_keys("<C-w>=").unwrap();
    assert!(log.iter().all(|key| key.errors.is_empty()), "a command");
    assert_eq!(h.next_timer(), Some(h.now() + TICK), "awake");
    h.advance(TICK);
    assert!(on_screen(&mut h, DARK) == at_frame(9091, DARK, TEXT));

    // So do a key nobody takes, which ends no command, and the pointer on it; and losing the
    // keyboard doesn't stop it.
    h.advance(Duration::from_secs(300));
    assert_eq!(h.next_timer(), None, "at rest again");
    assert_eq!(h.type_keys("x").unwrap()[0].errors, ["x is undefined"]);
    assert_eq!(h.next_timer(), Some(h.now() + TICK), "a key on it");
    h.advance(Duration::from_secs(300));
    assert_eq!(h.next_timer(), None, "and again");
    h.pointer(PointerKind::Move, 10, 10);
    assert_eq!(h.next_timer(), Some(h.now() + TICK), "the pointer on it");
    h.send(Input::Focus(false));
    assert_eq!(h.next_timer(), Some(h.now() + TICK), "without the keyboard");

    let buf = h.find_buffer(NAME).unwrap();
    h.kill(buf);
    assert_eq!(h.next_timer(), None, "killed");
    assert_eq!(h.advance(Duration::from_secs(1)), Vec::<String>::new());
}

#[test]
fn q_uncovers_the_buffer_beneath_colon_c_w_and_c_o_work_esc_is_silent_and_i_is_undefined() {
    let mut h = harness();
    show(&mut h);
    let log = h.type_keys("i<Esc><C-w><Esc>").unwrap();
    assert_eq!(log[0].errors, ["i is undefined"], "as on any surface");
    let quiet = |log: &[KeyLog]| log.iter().all(|key| key.errors.is_empty());
    assert!(quiet(&log[1..]), "Esc in normal state, and after C-w");
    // Ctrl-[ as the OS sends it, `[` with Ctrl held: notation would make it Esc before core did.
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let bracket = KeyEvent::press(Key::Char('[')).mods(ctrl);
    assert_eq!(h.send(Input::Key(bracket)), Vec::<String>::new());

    h.type_keys("<C-w>v").unwrap();
    assert_eq!(h.window_list().len(), 2, "C-w v");
    h.type_keys("<C-w>c").unwrap();
    assert_eq!(h.window_list().len(), 1, "C-w c");
    h.type_keys(":b notes<CR>").unwrap();
    assert_eq!(showing(&h), "notes", "`:` is the command line");
    // A buffer made since notes was shown is the newer, and the jellyfish covers the one in use
    // all the same: C-o, which is C-^ here, goes back to the notes.
    h.create_text("made since", "");
    h.type_keys(":jellyfish<CR>").unwrap();
    assert_eq!(showing(&h), NAME);
    assert!(quiet(&h.type_keys("<C-o>").unwrap()));
    assert_eq!(showing(&h), "notes", "C-o");
    h.type_keys(":jellyfish<CR>").unwrap();

    // q: the buffer goes, and the one it was shown over is back, though a buffer made behind
    // the jellyfish meanwhile is the one used last.
    h.create_text("behind", "");
    assert!(quiet(&h.type_keys("q").unwrap()), "q");
    assert_eq!(showing(&h), "notes");
    assert_eq!(h.find_buffer(NAME), None);
    assert_eq!(h.next_timer(), None);

    // The next one starts at its first spark. Its window leaves it and comes back by C-^, so
    // what it covered is forgotten, and q shows the buffer used last, as for any buffer.
    show(&mut h);
    assert!(on_screen(&mut h, DARK) == at_frame(0, DARK, TEXT));
    assert!(quiet(&h.type_keys(":b behind<CR><C-^>q").unwrap()));
    assert_eq!(showing(&h), "behind");

    // With no buffer beneath it, q leaves core's own empty one.
    show(&mut h);
    for name in ["notes", "made since", "behind"] {
        let buf = h.find_buffer(name).unwrap();
        h.kill(buf);
    }
    assert!(quiet(&h.type_keys("q").unwrap()));
    assert_eq!(showing(&h), "*scratch*");
}
