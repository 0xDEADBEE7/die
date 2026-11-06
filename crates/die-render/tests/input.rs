//! Key and pointer mapping, with no window, so these run on every platform. `tests/keys.rs`
//! holds the same mapping to real events on macOS.

use die_test_cap as _;

use die_core::{Button, Key, KeyEvent, KeyKind, ModifierKey, Mods, PointerEvent, PointerKind};
use die_render::font::{CellMetrics, FaceMetrics};
use die_render::input::{Held, KeyFacts, Pointer, caps_event, key_event};
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;
use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WinitKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};

fn character(s: &str) -> WinitKey {
    WinitKey::Character(s.into())
}

fn press(code: KeyCode, logical: WinitKey, unshifted: WinitKey) -> KeyFacts {
    KeyFacts {
        down: true,
        repeat: false,
        physical: PhysicalKey::Code(code),
        logical,
        unshifted,
        text: None,
    }
}

fn named(code: KeyCode, key: NamedKey) -> KeyFacts {
    press(code, WinitKey::Named(key), WinitKey::Named(key))
}

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

/// The 19×40 cell at 2x.
fn cell() -> CellMetrics {
    let m = FaceMetrics {
        ascent: 30.8799,
        descent: 7.04,
        leading: 2.0801,
        underline_position: -3.2,
        underline_thickness: 1.6,
        x_height: 17.0,
    };
    CellMetrics::new(m, 19.2, 2.0)
}

#[test]
fn named_keys_map_and_those_without_a_name_are_unidentified() {
    let held = Held::default();
    let cases = [
        (NamedKey::Escape, Key::Esc),
        (NamedKey::Enter, Key::Enter),
        (NamedKey::Delete, Key::Delete),
        (NamedKey::ArrowLeft, Key::Left),
        (NamedKey::Insert, Key::Insert),
        (NamedKey::Space, Key::Char(' ')),
        (NamedKey::F1, Key::F(1)),
        (NamedKey::F24, Key::F(24)),
        (NamedKey::F25, Key::Unidentified),
        (NamedKey::Find, Key::Unidentified),
        (NamedKey::Meta, Key::Modifier(ModifierKey::Super)),
    ];
    for (from, to) in cases {
        let got = key_event(&named(KeyCode::KeyQ, from), &held).key;
        assert_eq!(got, to, "{from:?}");
    }
}

#[test]
fn a_character_key_has_every_field() {
    let mut held = Held::default();
    held.key(KeyCode::ShiftRight, true);
    let facts = KeyFacts {
        text: Some("$".into()),
        ..press(KeyCode::Digit4, character("$"), character("4"))
    };
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    let want = KeyEvent {
        code: Some("Digit4".into()),
        unshifted: Some('4'),
        right_mods: shift,
        ..KeyEvent::press(Key::Char('$')).mods(shift)
    };
    assert_eq!(key_event(&facts, &held), want);
    let repeat = KeyFacts {
        repeat: true,
        ..facts.clone()
    };
    assert_eq!(key_event(&repeat, &held).kind, KeyKind::Repeat);
    let release = KeyFacts {
        down: false,
        ..facts
    };
    assert_eq!(key_event(&release, &held).kind, KeyKind::Release);
}

#[test]
fn text_is_only_what_the_key_does_not_name() {
    let held = Held::default();
    let text = |logical: WinitKey, typed: &str| {
        let facts = KeyFacts {
            text: Some(typed.into()),
            ..press(KeyCode::KeyA, logical, character("a"))
        };
        (key_event(&facts, &held).key, key_event(&facts, &held).text)
    };
    assert_eq!(text(character("a"), "a"), (Key::Char('a'), None));
    assert_eq!(
        text(character("é"), "é"),
        (Key::Char('é'), None),
        "a layout's own letter"
    );
    let cluster = "e\u{301}";
    assert_eq!(
        text(character(cluster), cluster),
        (Key::Char('e'), Some(cluster.to_owned())),
        "more than the key can name"
    );
    let dead = WinitKey::Dead(Some('´'));
    assert_eq!(
        text(dead, "é"),
        (Key::Unidentified, Some("é".to_owned())),
        "a dead key's composition"
    );
    let enter = WinitKey::Named(NamedKey::Enter);
    assert_eq!(text(enter, "\r"), (Key::Enter, None));
    // With Ctrl, the OS's text is a control character, which is never wanted.
    let mut held = Held::default();
    held.key(KeyCode::ControlLeft, true);
    let facts = KeyFacts {
        text: Some("\u{1}".into()),
        ..press(KeyCode::KeyA, character("a"), character("a"))
    };
    assert_eq!(key_event(&facts, &held).text, None);
    assert_eq!(key_event(&facts, &held).mods, ctrl());
}

#[test]
fn an_unknown_physical_key_has_no_code() {
    let facts = KeyFacts {
        physical: PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
        ..press(KeyCode::KeyA, character("a"), WinitKey::Dead(None))
    };
    let ev = key_event(&facts, &Held::default());
    assert_eq!((ev.code, ev.unshifted), (None, None));
}

#[test]
fn sides_come_from_the_modifier_keys_and_both_sides_are_one_modifier() {
    let mut held = Held::default();
    held.key(KeyCode::ControlLeft, true);
    assert_eq!((held.mods(), held.right_mods()), (ctrl(), Mods::default()));
    held.key(KeyCode::ControlRight, true);
    assert_eq!((held.mods(), held.right_mods()), (ctrl(), ctrl()));
    held.key(KeyCode::ControlLeft, false);
    assert_eq!(
        (held.mods(), held.right_mods()),
        (ctrl(), ctrl()),
        "the other is still down"
    );
    held.key(KeyCode::ControlRight, false);
    assert_eq!(held.mods(), Mods::default());
    held.key(KeyCode::AltRight, true);
    held.key(KeyCode::SuperLeft, true);
    held.key(KeyCode::ShiftLeft, true);
    let all = Mods {
        alt: true,
        super_: true,
        shift: true,
        ctrl: false,
    };
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    assert_eq!((held.mods(), held.right_mods()), (all, alt));
}

#[test]
fn the_os_corrects_what_the_keys_missed_but_keeps_the_side_they_said() {
    let mut held = Held::default();
    // Released while another app had the keyboard.
    held.key(KeyCode::ShiftRight, true);
    held.sync(ModifiersState::empty());
    assert_eq!((held.mods(), held.right_mods()), Default::default());
    // Held before the window had the keyboard: no key's event says which side.
    held.sync(ModifiersState::CONTROL);
    assert_eq!((held.mods(), held.right_mods()), (ctrl(), Mods::default()));
    // The modifier's own event then says the side, and the state agrees.
    let mut held = Held::default();
    held.key(KeyCode::ControlRight, true);
    held.sync(ModifiersState::CONTROL);
    assert_eq!((held.mods(), held.right_mods()), (ctrl(), ctrl()));
}

#[test]
fn caps_lock_shows_on_every_key_and_its_own_key() {
    let mut held = Held::default();
    // winit's own event, where it has one: each press toggles, and a release doesn't.
    held.key(KeyCode::CapsLock, true);
    assert!(held.caps());
    held.key(KeyCode::CapsLock, false);
    assert!(held.caps());
    let facts = press(KeyCode::KeyA, character("A"), character("a"));
    let a = key_event(&facts, &held);
    assert!(a.caps_lock);
    assert_eq!(a.chord().key, Key::Char('A'));
    held.key(KeyCode::CapsLock, true);
    assert!(!held.caps());
    // From the key monitor, on macOS.
    held.set_caps(true);
    let on = caps_event(&held);
    assert_eq!(on.key, Key::Modifier(ModifierKey::CapsLock));
    assert_eq!(on.code.as_deref(), Some("CapsLock"));
    assert_eq!((on.kind, on.caps_lock), (KeyKind::Press, true));
    held.set_caps(false);
    let off = caps_event(&held);
    assert_eq!((off.kind, off.caps_lock), (KeyKind::Release, false));
}

/// `press` and `release` of the left button, `at` that instant.
fn click(p: &mut Pointer, at: Instant) -> (PointerEvent, PointerEvent) {
    let mods = Mods::default();
    let down = p.button(MouseButton::Left, true, at, mods).unwrap();
    let up = p.button(MouseButton::Left, false, at, mods).unwrap();
    (down, up)
}

#[test]
fn a_second_button_coming_and_going_leaves_the_drag_and_its_click_count() {
    let (mods, now) = (Mods::default(), Instant::now());
    let mut p = Pointer::new(Duration::from_millis(400));
    click(&mut p, now);
    p.button(MouseButton::Left, true, now, mods).unwrap();
    p.button(MouseButton::Right, true, now, mods).unwrap();
    p.button(MouseButton::Right, false, now, mods).unwrap();
    let moved = p.moved((100.0, 100.0), &cell(), (80, 24), mods).unwrap();
    assert_eq!(moved.kind, PointerKind::Drag(Button::Left));
    let up = p.button(MouseButton::Left, false, now, mods).unwrap();
    assert_eq!(
        (up.kind, up.clicks),
        (PointerKind::Release(Button::Left), 2)
    );
}

#[test]
fn clicks_count_up_while_they_are_quick_on_one_cell_with_one_button() {
    let double = Duration::from_millis(400);
    let mut p = Pointer::new(double);
    let t = Instant::now();
    let (down, up) = click(&mut p, t);
    assert_eq!(down.kind, PointerKind::Press(Button::Left));
    assert_eq!(up.kind, PointerKind::Release(Button::Left));
    assert_eq!((down.clicks, up.clicks), (1, 1));
    let second = t + double;
    assert_eq!(click(&mut p, second).0.clicks, 2);
    let third = second + Duration::from_millis(100);
    let (down, up) = click(&mut p, third);
    assert_eq!((down.clicks, up.clicks), (3, 3));
    // Too late.
    let late = third + double + Duration::from_millis(1);
    assert_eq!(click(&mut p, late).0.clicks, 1);
    // Another button.
    let (mods, soon) = (Mods::default(), late + Duration::from_millis(1));
    let right = p.button(MouseButton::Right, true, soon, mods).unwrap();
    assert_eq!(
        (right.kind, right.clicks),
        (PointerKind::Press(Button::Right), 1)
    );
    p.button(MouseButton::Right, false, soon, mods).unwrap();
    // Another cell.
    click(&mut p, soon);
    p.moved((100.0, 100.0), &cell(), (80, 24), mods).unwrap();
    assert_eq!(click(&mut p, soon).0.clicks, 1);
    // No name in die-core.
    assert_eq!(p.button(MouseButton::Back, true, soon, mods), None);
}

#[test]
fn moving_gives_an_event_per_cell_clamped_to_the_frame_and_drags_with_a_button_down() {
    let (c, grid, mods) = (cell(), (80, 24), ctrl());
    let (w, h) = (f64::from(c.w), f64::from(c.h));
    let mut p = Pointer::new(Duration::ZERO);
    assert_eq!(p.moved((1.0, 1.0), &c, grid, mods), None, "the same cell");
    let moved = p.moved((w * 3.5, h * 2.5), &c, grid, mods).unwrap();
    let want = PointerEvent {
        kind: PointerKind::Move,
        row: 2,
        col: 3,
        clicks: 0,
        mods,
    };
    assert_eq!(moved, want);
    assert_eq!(p.moved((w * 3.9, h * 2.9), &c, grid, mods), None);
    let at = Instant::now();
    let down = p.button(MouseButton::Left, true, at, mods).unwrap();
    assert_eq!((down.row, down.col), (2, 3));
    let drag = p.moved((w * 4.0, h * 2.0), &c, grid, mods).unwrap();
    assert_eq!(drag.kind, PointerKind::Drag(Button::Left));
    assert_eq!(drag.clicks, 0);
    p.button(MouseButton::Left, false, at, mods).unwrap();
    let past = p.moved((1e6, -5.0), &c, grid, mods).unwrap();
    assert_eq!((past.kind, past.row, past.col), (PointerKind::Move, 0, 79));
}

#[test]
fn a_scroll_is_how_far_the_view_goes_in_cells() {
    let (c, mods) = (cell(), Mods::default());
    let p = Pointer::new(Duration::ZERO);
    let scroll = |delta| match p.wheel(delta, &c, mods).map(|e| e.kind) {
        Some(PointerKind::Scroll { dx, dy }) => Some((dx, dy)),
        other => {
            assert_eq!(other, None);
            None
        }
    };
    let (w, h) = (f64::from(c.w), f64::from(c.h));
    // A trackpad pushing the content down and right moves the view up and left, in fractions.
    let pixels = PhysicalPosition::new(w * 1.5, h * 0.25);
    assert_eq!(
        scroll(MouseScrollDelta::PixelDelta(pixels)),
        Some((-1.5, -0.25))
    );
    let wheel = MouseScrollDelta::LineDelta(0.0, -3.0);
    assert_eq!(scroll(wheel), Some((-0.0, 3.0)));
    let end = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0));
    assert_eq!(scroll(end), None, "a gesture's end");
    let event = p.wheel(wheel, &c, mods).unwrap();
    assert_eq!(event.clicks, 0);
}
