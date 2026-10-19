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
    // A bracketed word that isn't a key is a typo, not text: write <lt> for a literal <.
    assert!(parse_keys("<Esx>").is_err());
    assert!(parse_keys("<C-bad>").is_err());
}

#[test]
fn modifiers_fold_as_vim_does() {
    let alt = |c| with(ch(c), |m| m.alt = true);
    assert_eq!(one("<C-x>"), ctrl('x'));
    assert_eq!(one("<c-x>"), ctrl('x'));
    assert_eq!(one("<C-X>"), ctrl('x'), "Ctrl folds case, as in Vim");
    assert_eq!(
        one("<C-S-a>"),
        ctrl('A'),
        "Shift with a letter is the capital"
    );
    assert_eq!(one("<A-x>"), alt('x'));
    assert_eq!(one("<M-x>"), alt('x'));
    assert_eq!(one("<A-X>"), alt('X'), "only Ctrl folds case");
    assert_eq!(one("<D-v>"), with(ch('v'), |m| m.cmd = true));
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
        ctrl('A'),
        with(ch('X'), |m| m.alt = true),
        with(KeyEvent::press(Key::Tab), |m| m.shift = true),
        KeyEvent::press(Key::F(3)),
        ctrl(' '),
        KeyEvent::press(Key::Delete),
    ];
    assert_eq!(
        key_notation(&ev),
        "<lt><C-S-a><A-X><S-Tab><F3><C-Space><Del>"
    );
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
/// characters (a Ctrl chord with a capital letter is how Ctrl-Shift is written).
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
        let shift = s && !matches!(key, Key::Char(_));
        KeyEvent::press(key).mods(Mods {
            ctrl: c,
            alt: a,
            shift,
            cmd: d,
            ..Mods::default()
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn notation_round_trips(evs in prop::collection::vec(key_press(), 0..8)) {
        let s = key_notation(&evs);
        let back = parse_keys(&s).map_err(|e| TestCaseError::fail(format!("{s:?}: {e}")))?;
        prop_assert_eq!(back, evs, "via {:?}", s);
    }
}
