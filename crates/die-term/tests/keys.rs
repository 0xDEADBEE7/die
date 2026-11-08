//! Keys in, bytes out: legacy encoding with its modes, the kitty keyboard flags with release
//! and repeat, Alt as Meta, and the events the OS gives as well as notation's.

use die_test_cap as _;

use die_core::{Key, KeyEvent, KeyKind, ModifierKey, Mods};
use die_term::Term;

fn term() -> Term {
    Term::new(80, 24, (18, 36)).unwrap()
}

fn mods(spec: &str) -> Mods {
    Mods {
        ctrl: spec.contains('C'),
        alt: spec.contains('M'),
        shift: spec.contains('S'),
        super_: spec.contains('D'),
    }
}

fn key(k: Key, m: &str) -> KeyEvent {
    KeyEvent::press(k).mods(mods(m))
}

fn c(ch: char) -> Key {
    Key::Char(ch)
}

/// The key's bytes, escaped.
fn sent(t: &mut Term, ev: &KeyEvent) -> String {
    t.key(ev).escape_ascii().to_string()
}

#[test]
fn legacy_encoding() {
    let mut t = term();
    let cases = [
        (key(c('a'), ""), "a"),
        (key(c('A'), ""), "A"),
        (key(c('$'), ""), "$"),
        (key(c('c'), "C"), "\\x03"),
        (key(c('@'), "C"), "\\x00"),
        (key(c('x'), "M"), "\\x1bx"),
        (key(c('s'), "CM"), "\\x1b\\x13"),
        (key(Key::Enter, ""), "\\r"),
        (key(Key::Enter, "M"), "\\x1b\\r"),
        (key(Key::Tab, ""), "\\t"),
        (key(Key::Tab, "S"), "\\x1b[Z"),
        (key(Key::Esc, ""), "\\x1b"),
        (key(Key::Backspace, ""), "\\x7f"),
        (key(Key::Backspace, "C"), "\\x08"),
        (key(Key::Up, ""), "\\x1b[A"),
        (key(Key::Up, "C"), "\\x1b[1;5A"),
        (key(Key::Left, "S"), "\\x1b[1;2D"),
        (key(Key::Home, ""), "\\x1b[H"),
        (key(Key::PageDown, ""), "\\x1b[6~"),
        (key(Key::Delete, ""), "\\x1b[3~"),
        (key(Key::F(1), ""), "\\x1bOP"),
        (key(Key::F(5), ""), "\\x1b[15~"),
        (key(Key::F(12), "S"), "\\x1b[24;2~"),
        (KeyEvent::committed("é"), "\\xc3\\xa9"),
        (key(c('a'), "").kind(KeyKind::Repeat), "a"),
        (key(c('a'), "").kind(KeyKind::Release), ""),
        (key(Key::Modifier(ModifierKey::Shift), ""), ""),
    ];
    for (ev, want) in cases {
        assert_eq!(sent(&mut t, &ev), want, "{ev:?}");
    }
}

#[test]
fn ctrl_brackets_i_and_m_are_esc_tab_and_return_in_legacy() {
    // As GUI Emacs, Vim and die-core's keymaps fold them; Ghostty alone would send CSI u.
    let mut t = term();
    assert_eq!(sent(&mut t, &key(c('['), "C")), "\\x1b");
    assert_eq!(sent(&mut t, &key(c('i'), "C")), "\\t");
    assert_eq!(sent(&mut t, &key(c('m'), "C")), "\\r");
    assert_eq!(
        sent(&mut t, &key(c('['), "CM")),
        "\\x1b[91;7u",
        "not with other modifiers"
    );
    // Ghostty's own encoding of a modified Return, which ghostel sends too, and Claude Code
    // reads as a new line.
    assert_eq!(sent(&mut t, &key(Key::Enter, "S")), "\\x1b[27;2;13~");
    assert_eq!(sent(&mut t, &key(Key::Enter, "C")), "\\x1b[27;5;13~");
    // With the kitty flags, they stay apart.
    t.feed(b"\x1b[>1u");
    assert_eq!(sent(&mut t, &key(c('['), "C")), "\\x1b[91;5u");
    assert_eq!(sent(&mut t, &key(c('i'), "C")), "\\x1b[105;5u");
    assert_eq!(sent(&mut t, &key(Key::Esc, "")), "\\x1b[27u");
}

#[test]
fn application_modes() {
    let mut t = term();
    t.feed(b"\x1b[?1h");
    assert!(t.modes().app_cursor_keys);
    assert_eq!(sent(&mut t, &key(Key::Up, "")), "\\x1bOA", "DECCKM");
    assert_eq!(sent(&mut t, &key(Key::Home, "")), "\\x1bOH");
    t.feed(b"\x1b[?1l");
    assert_eq!(sent(&mut t, &key(Key::Up, "")), "\\x1b[A");
    // DECKPAM: mode 1035 is on, so the keypad still types, as in Ghostty on macOS, where num
    // lock is always on.
    t.feed(b"\x1b=");
    let mut enter = key(Key::Enter, "");
    enter.code = Some("NumpadEnter".into());
    assert_eq!(sent(&mut t, &enter), "\\r");
}

#[test]
fn kitty_flags_with_release_and_repeat() {
    let mut t = term();
    t.feed(b"\x1b[>1u");
    assert_eq!(t.modes().kitty_flags, 1);
    assert_eq!(sent(&mut t, &key(c('c'), "C")), "\\x1b[99;5u");
    assert_eq!(sent(&mut t, &key(c('x'), "M")), "\\x1b[120;3u");
    assert_eq!(sent(&mut t, &key(Key::Enter, "S")), "\\x1b[13;2u");
    assert_eq!(sent(&mut t, &key(c('a'), "")), "a", "plain text stays text");
    assert_eq!(sent(&mut t, &key(c('a'), "").kind(KeyKind::Release)), "");
    // All flags: every key is CSI u, releases and repeats included, with alternates.
    t.feed(b"\x1b[<u\x1b[>31u");
    assert_eq!(t.modes().kitty_flags, 31);
    let a = key(c('a'), "");
    assert_eq!(sent(&mut t, &a), "\\x1b[97;;97u");
    assert_eq!(
        sent(&mut t, &a.clone().kind(KeyKind::Repeat)),
        "\\x1b[97;1:2;97u"
    );
    assert_eq!(sent(&mut t, &a.kind(KeyKind::Release)), "\\x1b[97;1:3u");
    assert_eq!(
        sent(&mut t, &key(c('A'), "")),
        "\\x1b[97:65;2;65u",
        "shifted key"
    );
    assert_eq!(
        sent(&mut t, &key(Key::Enter, "").kind(KeyKind::Release)),
        "\\x1b[13;1:3u"
    );
    assert_eq!(
        sent(&mut t, &key(Key::Up, "").kind(KeyKind::Release)),
        "\\x1b[1;1:3A"
    );
    let shift = KeyEvent::press(Key::Modifier(ModifierKey::Shift)).mods(mods("S"));
    assert_eq!(sent(&mut t, &shift), "\\x1b[57441;2u", "a modifier alone");
    // Popping the flags returns to legacy.
    t.feed(b"\x1b[<u");
    assert_eq!(t.modes().kitty_flags, 0);
    assert_eq!(sent(&mut t, &key(c('c'), "C")), "\\x03");
}

#[test]
fn the_os_events_physical_key_and_sides() {
    let mut t = term();
    t.feed(b"\x1b[>31u");
    // A French AZERTY 'a' sits where QWERTY has 'q': the code names the physical key, and
    // the text is what the layout typed.
    let mut azerty = key(c('a'), "");
    azerty.code = Some("KeyQ".into());
    azerty.unshifted = Some('a');
    assert_eq!(
        sent(&mut t, &azerty),
        "\\x1b[97::113;;97u",
        "with the US layout's key, q"
    );
    // Shift held with the unshifted key the OS gave: Shift is consumed.
    let mut dollar = key(c('$'), "S");
    dollar.code = Some("Digit4".into());
    dollar.unshifted = Some('4');
    assert_eq!(sent(&mut t, &dollar), "\\x1b[52:36;2;36u");
    // The right Shift on its own.
    let mut right = KeyEvent::press(Key::Modifier(ModifierKey::Shift)).mods(mods("S"));
    right.code = Some("ShiftRight".into());
    right.right_mods = mods("S");
    assert_eq!(sent(&mut t, &right), "\\x1b[57447;2u");
    // Caps Lock reported as a lock with all flags on.
    let mut caps = key(c('a'), "");
    caps.caps_lock = true;
    assert_eq!(sent(&mut t, &caps), "\\x1b[97;65;97u");
}
