//! K3: key notation. One parser for keymaps, `feed_keys`, the harness and the Evil oracle's
//! fixtures. It follows Vim: `<...>` is one key only when its name is known.
use die_core::*;
use proptest::prelude::*;

fn keys(s: &str) -> Vec<KeyEvent> {
    parse_keys(s).unwrap_or_else(|e| panic!("{s:?}: {e}"))
}

fn one(s: &str) -> KeyEvent {
    let mut v = keys(s);
    assert_eq!(v.len(), 1, "{s:?} is {} keys", v.len());
    v.remove(0)
}

fn ch(c: char) -> KeyEvent {
    KeyEvent::press(Key::Char(c))
}

fn with(ev: KeyEvent, f: impl FnOnce(&mut Mods)) -> KeyEvent {
    let mut m = ev.mods;
    f(&mut m);
    ev.mods(m)
}

fn ctrl(c: char) -> KeyEvent {
    with(ch(c), |m| m.ctrl = true)
}

#[test]
fn vim_names_are_case_insensitive() {
    for (s, want) in [
        ("<Esc>", Key::Esc),
        ("<esc>", Key::Esc),
        ("<ESC>", Key::Esc),
        ("<Escape>", Key::Esc),
        ("<CR>", Key::Enter),
        ("<cr>", Key::Enter),
        ("<Enter>", Key::Enter),
        ("<Return>", Key::Enter),
        ("<BS>", Key::Backspace),
        ("<Del>", Key::Delete),
        ("<Tab>", Key::Tab),
        ("<Up>", Key::Up),
        ("<Home>", Key::Home),
        ("<End>", Key::End),
        ("<PageUp>", Key::PageUp),
        ("<PageDown>", Key::PageDown),
        ("<F5>", Key::F(5)),
        ("<f12>", Key::F(12)),
        ("<Space>", Key::Char(' ')),
        ("<lt>", Key::Char('<')),
        ("<Bslash>", Key::Char('\\')),
        ("<Bar>", Key::Char('|')),
        ("<Super>", Key::Modifier(ModifierKey::Super)),
        ("<Command>", Key::Modifier(ModifierKey::Super)),
    ] {
        assert_eq!(one(s), KeyEvent::press(want), "{s}");
    }
    assert_eq!(one("<NL>"), ctrl('j'), "<NL> is Ctrl-J, as in Vim");
}

#[test]
fn unknown_brackets_are_literal_characters() {
    let lit = |s: &str| s.chars().map(ch).collect::<Vec<_>>();
    assert_eq!(keys("<<j>>"), lit("<<j>>"));
    assert_eq!(keys("<j>"), lit("<j>"));
    assert_eq!(keys("a<"), lit("a<"));
    assert_eq!(keys("<lt>x"), lit("<x"));
    assert_eq!(keys("<C-V>j<c-r>"), [ctrl('v'), ch('j'), ctrl('r')]);
    assert_eq!(keys("<Esc >"), lit("<Esc >"));
    // A bracketed word that isn't a key is a typo, not text: write <lt> for a literal <.
    // So is anything after modifiers that isn't a key.
    for typo in ["<Esx>", "<C-bad>", "<C-X-a>", "<C-Page-Up>"] {
        assert!(parse_keys(typo).is_err(), "{typo}");
    }
}

#[test]
fn modifiers_fold_as_vim_does() {
    let alt = |c| with(ch(c), |m| m.alt = true);
    assert_eq!(one("<C-x>"), ctrl('x'));
    assert_eq!(one("<c-x>"), ctrl('x'));
    assert_eq!(one("<C-X>"), ctrl('x'), "Ctrl folds case, as in Vim");
    let ctrl_shift_a = with(ctrl('a'), |m| m.shift = true);
    assert_eq!(one("<C-S-a>"), ctrl_shift_a, "Shift is kept with Ctrl");
    assert_eq!(one("<C-S-A>"), ctrl_shift_a);
    assert_eq!(one("<S-a>"), ch('A'), "Shift with a letter is the capital");
    assert_eq!(one("<A-x>"), alt('x'));
    assert_eq!(one("<M-x>"), alt('x'));
    assert_eq!(one("<A-X>"), alt('X'), "only Ctrl folds case");
    assert_eq!(
        one("<D-v>"),
        with(ch('v'), |m| m.super_ = true),
        "D names the DIE layer, Super until config sets it"
    );
    assert_eq!(one("<C->>"), ctrl('>'));
    assert_eq!(one("<C-->"), ctrl('-'));
    assert_eq!(one("<C-lt>"), ctrl('<'));
    assert_eq!(
        one("<S-Tab>"),
        with(KeyEvent::press(Key::Tab), |m| m.shift = true),
        "Shift stays on named keys"
    );
    assert_eq!(one("<C-Space>"), ctrl(' '));
}

#[test]
fn leader_and_wildcards_are_patterns_not_keys() {
    let pats = parse_pats("<leader>f<char><any>").unwrap();
    assert_eq!(pats[0], Pat::Leader);
    assert_eq!(pats[2], Pat::AnyChar);
    assert_eq!(pats[3], Pat::Any);
    assert_eq!(parse_pats("<Leader>").unwrap(), [Pat::Leader]);
    for s in ["<char>", "<any>", "<leader>x"] {
        assert!(parse_keys(s).is_err(), "{s} can't be typed");
    }
}

#[test]
fn notation_of_special_keys() {
    let ev = [
        ch('<'),
        ctrl('A'), // Ctrl-a with Caps Lock on
        with(ch('A'), |m| {
            m.ctrl = true;
            m.shift = true;
        }),
        with(ch('X'), |m| m.alt = true),
        with(KeyEvent::press(Key::Tab), |m| m.shift = true),
        KeyEvent::press(Key::F(3)),
        ctrl(' '),
        KeyEvent::press(Key::Delete),
    ];
    assert_eq!(
        key_notation(&ev),
        "<lt><C-a><C-S-a><A-X><S-Tab><F3><C-Space><Del>"
    );
    let committed = [KeyEvent::committed("❤️"), KeyEvent::committed("´x<")];
    assert_eq!(key_notation(&committed), "❤️´x<lt>", "all of the text");
}

const NAMED: &[Key] = &[
    Key::Esc,
    Key::Enter,
    Key::Tab,
    Key::Backspace,
    Key::Delete,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Insert,
];

/// A key press as notation can write it: no text, no OS fields, and Shift folded into
/// characters, except with Ctrl and a letter, which is lowercase.
fn key_press() -> impl Strategy<Value = KeyEvent> {
    let chars = prop_oneof![
        prop::char::range('a', 'z'),
        prop::char::range('A', 'Z'),
        prop::sample::select(vec![
            '<', '>', '-', ' ', '\\', '|', '"', '@', '0', '9', 'é', '→'
        ]),
    ];
    let key = prop_oneof![
        chars.prop_map(Key::Char),
        prop::sample::select(NAMED.to_vec()),
        (1u8..=24).prop_map(Key::F),
    ];
    (key, any::<[bool; 4]>()).prop_map(|(key, [c, a, s, d])| {
        let (key, shift) = match key {
            Key::Char(ch) if c && ch.is_ascii_alphabetic() => {
                (Key::Char(ch.to_ascii_lowercase()), s)
            }
            Key::Char(_) => (key, false),
            _ => (key, s),
        };
        KeyEvent::press(key).mods(Mods {
            ctrl: c,
            alt: a,
            shift,
            super_: d,
        })
    })
}

fn typed_text(evs: &[KeyEvent]) -> Option<String> {
    evs.iter().map(KeyEvent::printable).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn notation_round_trips(evs in prop::collection::vec(key_press(), 0..8)) {
        let s = key_notation(&evs);
        let back = parse_keys(&s).map_err(|e| TestCaseError::fail(format!("{s:?}: {e}")))?;
        prop_assert_eq!(back, evs, "via {:?}", s);
    }

    /// Committed text comes back as one key per character, typing the same text.
    #[test]
    fn committed_text_round_trips(texts in prop::collection::vec("[^\\p{Cc}]{1,4}", 1..4)) {
        let evs: Vec<_> = texts.iter().map(|t| KeyEvent::committed(t)).collect();
        let s = key_notation(&evs);
        let back = parse_keys(&s).map_err(|e| TestCaseError::fail(format!("{s:?}: {e}")))?;
        prop_assert_eq!(typed_text(&back), typed_text(&evs), "via {:?}", s);
    }
}
