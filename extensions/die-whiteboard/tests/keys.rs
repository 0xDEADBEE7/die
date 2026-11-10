//! Every key, as `describe-key` sees it: each binding in the kind keymap reaches its command,
//! `:` is die-vim's ex, the `C-w` keys are its window commands, `\z` toggles the width, the
//! keys compose with die-vim's, and a label's typed text reaches the surface.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Config, Harness, Key, KeyAction, KeyEvent, Settings, State, parse_keys};
use die_vim::Vim;
use die_whiteboard::Whiteboard;

/// What `keys` do in the focused window: the command and the keymap it was found in.
fn describe(h: &Harness, keys: &str) -> (String, String) {
    let events = parse_keys(keys).unwrap();
    match h.describe_key(&events).unwrap().action {
        KeyAction::Command { name, keymap, .. } => (name, keymap),
        other => panic!("{keys}: {other:?}"),
    }
}

const NORMAL: [(&str, &str); 40] = [
    ("ZZ", "whiteboard-save-and-close"),
    ("ZQ", "vim-quit"),
    ("<C-^>", "vim-alternate-buffer"),
    ("<C-o>", "vim-jump-backward"),
    ("p", "whiteboard-tool-pen"),
    ("r", "whiteboard-tool-rect"),
    ("c", "whiteboard-tool-ellipse"),
    ("a", "whiteboard-tool-arrow"),
    ("l", "whiteboard-right"),
    ("t", "whiteboard-tool-label"),
    ("v", "whiteboard-tool-select"),
    ("e", "whiteboard-eraser"),
    ("h", "whiteboard-left"),
    ("j", "whiteboard-down"),
    ("k", "whiteboard-up"),
    ("x", "whiteboard-delete"),
    ("<Del>", "whiteboard-delete"),
    ("y", "whiteboard-duplicate"),
    ("<Esc>", "whiteboard-escape"),
    ("1", "whiteboard-ink"),
    ("6", "whiteboard-ink"),
    ("[", "whiteboard-thinner"),
    ("]", "whiteboard-thicker"),
    ("u", "whiteboard-undo"),
    ("<C-r>", "whiteboard-redo"),
    ("<D-z>", "whiteboard-undo"),
    ("<D-S-z>", "whiteboard-redo"),
    ("+", "whiteboard-zoom-in"),
    ("-", "whiteboard-zoom-out"),
    ("0", "whiteboard-zoom-reset"),
    ("E", "whiteboard-export"),
    ("C", "whiteboard-copy-image"),
    ("S", "whiteboard-save"),
    (":", "vim-ex"),
    ("<C-w>s", "vim-window-split"),
    ("<C-w>v", "vim-window-vsplit"),
    ("<C-w>w", "vim-window-next"),
    ("<C-w>c", "vim-window-delete"),
    ("<C-w>o", "vim-window-only"),
    ("\\z", "toggle-max-width"),
];

#[test]
fn every_binding_is_in_the_kind_keymap_where_describe_key_finds_it() {
    let s = setup("keys");
    for (key, cmd) in NORMAL {
        let (name, keymap) = describe(&s.h, key);
        assert_eq!((key, name.as_str()), (key, cmd));
        assert_eq!(keymap, "kind whiteboard", "{key}");
    }
    let names: Vec<&str> = s.h.command_names();
    for (_, cmd) in NORMAL {
        assert!(names.contains(&cmd), "{cmd} exists");
    }
    for cmd in [
        "whiteboard",
        "whiteboard-new",
        "whiteboard-open",
        "whiteboard-tool",
        "whiteboard-tool-line",
        "whiteboard-highlighter",
        "whiteboard-label-end",
        "whiteboard-label-newline",
        "whiteboard-label-backspace",
    ] {
        assert_eq!(
            s.h.command_doc(cmd).map(|d| d.1),
            Some("whiteboard"),
            "{cmd}"
        );
    }
    // `l` is the line tool only through `whiteboard-tool-line`: `l` nudges, as `hjkl` do.
    let d = s.h.describe_command("whiteboard-tool-line").unwrap();
    assert!(d.keys.is_empty(), "{:?}", d.keys);
    let d = s.h.describe_command("whiteboard-left").unwrap();
    assert_eq!(
        d.keys,
        [(
            "kind whiteboard".to_string(),
            State::Normal,
            "h".to_string()
        )]
    );
    // A key nothing binds goes to the surface, which doesn't take it outside a label.
    let d =
        s.h.describe_key(&[KeyEvent::press(Key::Char('z'))])
            .unwrap();
    assert_eq!(
        d.action,
        KeyAction::Surface {
            kind: "whiteboard",
            declared: false
        }
    );
}

#[test]
fn insert_state_has_the_labels_keys_and_typed_text_reaches_the_surface() {
    let mut s = setup("insert");
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 20.0));
    assert_eq!(s.h.state(), State::Insert);
    for (key, cmd) in [
        ("<Esc>", "whiteboard-label-end"),
        ("<CR>", "whiteboard-label-end"),
        ("<S-CR>", "whiteboard-label-newline"),
        ("<BS>", "whiteboard-label-backspace"),
        ("<D-z>", "whiteboard-undo"),
        ("<D-S-z>", "whiteboard-redo"),
    ] {
        let (name, keymap) = describe(&s.h, key);
        assert_eq!(
            (key, name.as_str(), keymap.as_str()),
            (key, cmd, "kind whiteboard")
        );
    }
    let d =
        s.h.describe_key(&[KeyEvent::press(Key::Char('q'))])
            .unwrap();
    assert_eq!(
        d.action,
        KeyAction::Surface {
            kind: "whiteboard",
            declared: true
        },
        "typed text is the label's"
    );
    keys(&mut s.h, "<Esc>");
}

#[test]
fn the_width_toggles_with_your_text_defaults_and_the_keys_compose_with_die_vims() {
    let exts: Vec<Box<dyn die_core::Extension>> =
        vec![Box::new(Vim::default()), Box::new(Whiteboard::for_tests())];
    let config = Config {
        text_defaults: Settings {
            max_width: Some(80),
            center: true,
            ..Settings::default()
        },
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.open_text("notes", "").unwrap();
    h.set_window_size(120, 30);
    h.set_cell_pixels(CELL);
    let dir = TempDir::new("width");
    open(&mut h, &dir.path().join("w.svg"));
    let w = h.focused().unwrap();
    let g = h.surface_area(w).unwrap();
    assert_eq!((g.text_x, g.text_w), (20, 80), "a file's width, centred");
    keys(&mut h, "\\z");
    h.run_until_idle();
    let g = h.surface_area(w).unwrap();
    assert_eq!((g.text_x, g.text_w), (0, 120));
    keys(&mut h, "\\z");
    h.run_until_idle();
    assert_eq!(h.surface_area(w).unwrap().text_w, 80);
    // die-vim's ex and window keys work from the board.
    keys(&mut h, ":vsplit<CR>");
    assert_eq!(h.window_list().len(), 2);
    keys(&mut h, "<C-w>o");
    assert_eq!(h.window_list().len(), 1);
    keys(&mut h, "<C-w>s<C-w>j");
    assert_eq!(h.window_list().len(), 2);
    keys(&mut h, "<C-w>c");
    assert_eq!(h.window_list().len(), 1);
    // die-vim's own grammar isn't inherited: `dd` is two undefined keys, not a delete.
    let errors = keys_errors(&mut h, "d");
    assert_eq!(errors, ["d is undefined"]);
    assert_eq!(h.state(), State::Normal);
}

#[test]
fn full_width_as_an_option() {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Whiteboard::new(die_whiteboard::Options {
            full_width: true,
            ..die_whiteboard::Options::default()
        })),
    ];
    let config = Config {
        text_defaults: Settings {
            max_width: Some(80),
            center: true,
            ..Settings::default()
        },
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.open_text("notes", "").unwrap();
    h.set_window_size(120, 30);
    let dir = TempDir::new("full");
    open(&mut h, &dir.path().join("w.svg"));
    let w = h.focused().unwrap();
    assert_eq!(h.surface_area(w).unwrap().text_w, 120);
}

#[test]
fn zz_writes_the_board_and_closes_its_window_and_c_caret_flips_to_the_last_buffer() {
    let mut s = setup("reflexes");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let notes = s.h.find_buffer("notes").unwrap();
    keys(&mut s.h, "<C-^>");
    assert_eq!(s.h.focused_buffer(), Some(notes), "the buffer used before");
    keys(&mut s.h, "<C-^>");
    assert_eq!(s.h.focused_buffer(), Some(s.buf));
    keys(&mut s.h, "<C-w>s");
    assert_eq!(s.h.window_list().len(), 2);
    assert!(!s.path.exists());
    keys(&mut s.h, "ZZ");
    assert_eq!(s.h.window_list().len(), 1, "closed, as :x does");
    assert!(s.path.exists(), "written first");
    assert!(s.h.is_live(s.buf), "the buffer stays, as :x leaves it");
}
