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
/// characters, except with Ctrl and a letter, which is lowercase. No two of its characters
/// form one grapheme cluster: literal ones that did would come back as one key.
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
        // Ctrl-i and Ctrl-m alone are written <Tab> and <CR>, so they come back as those keys.
        let aliased = matches!(key, Key::Char('i' | 'm')) && c && !a && !shift && !d;
        let key = if aliased { Key::Char('j') } else { key };
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

    /// Committed text comes back as one key per grapheme cluster, typing the same text.
    #[test]
    fn committed_text_round_trips(texts in prop::collection::vec("[^\\p{Cc}]{1,4}", 1..4)) {
        let evs: Vec<_> = texts.iter().map(|t| KeyEvent::committed(t)).collect();
        let s = key_notation(&evs);
        let back = parse_keys(&s).map_err(|e| TestCaseError::fail(format!("{s:?}: {e}")))?;
        prop_assert_eq!(typed_text(&back), typed_text(&evs), "via {:?}", s);
    }
}

// ---------------------------------------------------------------- what the keys review found

/// S1's real run: winit's text for Ctrl chords is a control character, and for Alt the plain
/// letter. `<char>` must not match either, and insert state must not type them.
#[test]
fn a_key_with_ctrl_alt_or_super_held_prints_nothing() {
    let os = |key, text: &str, f: fn(&mut Mods)| {
        let mut m = Mods::default();
        f(&mut m);
        KeyEvent {
            text: Some(text.into()),
            ..KeyEvent::press(key).mods(m)
        }
    };
    let held_ctrl: fn(&mut Mods) = |m| m.ctrl = true;
    let held_alt: fn(&mut Mods) = |m| m.alt = true;
    let held_super: fn(&mut Mods) = |m| m.super_ = true;
    assert_eq!(os(Key::Char('a'), "\u{1}", held_ctrl).printable(), None);
    assert_eq!(os(Key::Char('/'), "/", held_ctrl).printable(), None);
    assert_eq!(os(Key::Char('x'), "x", held_alt).printable(), None);
    assert_eq!(os(Key::Char('v'), "v", held_super).printable(), None);
    assert_eq!(ctrl('a').printable(), None);
    // Shift alone still types.
    assert_eq!(ch('A').printable().as_deref(), Some("A"));
}

#[test]
fn committed_text_prints_unless_its_empty_or_holds_a_control_character() {
    assert_eq!(KeyEvent::committed("é").printable().as_deref(), Some("é"));
    assert_eq!(KeyEvent::committed("").printable(), None);
    assert_eq!(KeyEvent::committed("a\u{7}").printable(), None);
    // A key whose OS text is a control character, with no modifier: Return's "\r".
    let cr = KeyEvent {
        text: Some("\r".into()),
        ..KeyEvent::press(Key::Enter)
    };
    assert_eq!(cr.printable(), None);
    assert_eq!(KeyEvent::press(Key::Char('\u{7f}')).printable(), None);
}

/// Each is one key: as typed, recorded and replayed.
const CLUSTERS: &[&str] = &[
    "👍🏽",
    "🇦🇺",
    "❤\u{fe0f}",
    "e\u{301}",
    "👨\u{200d}👩\u{200d}👧",
];

#[test]
fn literal_characters_of_one_cluster_are_one_key() {
    for c in CLUSTERS {
        assert_eq!(one(c), KeyEvent::committed(c), "{c:?}");
        assert_eq!(one(c).printable().as_deref(), Some(*c), "{c:?}");
    }
    assert_eq!(
        keys("r❤\u{fe0f}x"),
        [ch('r'), KeyEvent::committed("❤\u{fe0f}"), ch('x')]
    );
    assert_eq!(keys("<Esc>🇦🇺").len(), 2);
}

/// Only literal characters merge. A named key is a key of its own, and committed text can't
/// hold a control character, so CR LF, one cluster, stays two keys.
#[test]
fn named_keys_and_control_characters_dont_merge() {
    assert_eq!(keys("<Space>\u{301}"), [ch(' '), ch('\u{301}')]);
    assert_eq!(keys("<lt>\u{301}"), [ch('<'), ch('\u{301}')]);
    assert_eq!(keys("\r\n"), [ch('\r'), ch('\n')]);
    assert_eq!(keys("<C-e>\u{301}"), [ctrl('e'), ch('\u{301}')]);
}

#[test]
fn ctrl_bracket_i_and_m_are_escape_tab_and_return() {
    for (alias, named) in [
        ("<C-[>", "<Esc>"),
        ("<c-[>", "<Esc>"),
        ("<C-i>", "<Tab>"),
        ("<C-I>", "<Tab>"),
        ("<C-m>", "<CR>"),
        ("<C-M>", "<CR>"),
    ] {
        assert_eq!(parse_pats(alias), parse_pats(named), "{alias}");
        assert_eq!(one(alias), one(named), "{alias}");
    }
    // Another modifier makes it a different key, in notation and in events.
    for different in ["<C-S-i>", "<A-C-i>", "<C-S-m>", "<C-D-[>"] {
        assert_ne!(parse_pats(different), parse_pats("<Tab>"), "{different}");
        assert_ne!(parse_pats(different), parse_pats("<CR>"), "{different}");
        assert_ne!(parse_pats(different), parse_pats("<Esc>"), "{different}");
    }
    let tab = KeyEvent::press(Key::Tab).chord();
    assert_eq!(ctrl('i').chord(), tab);
    assert_eq!(ctrl('I').chord(), tab, "Caps Lock on");
    assert_eq!(ctrl('[').chord(), KeyEvent::press(Key::Esc).chord());
    assert_eq!(ctrl('m').chord(), KeyEvent::press(Key::Enter).chord());
    // The raw event keeps the difference, for a terminal's key reporting.
    assert_eq!(ctrl('[').key, Key::Char('['));
    assert!(ctrl('[').mods.ctrl);
    assert_eq!(key_notation(&[ctrl('[')]), "<Esc>");
}

#[test]
fn notation_refuses_what_no_key_can_send() {
    for typo in [
        "<C-S-2>",
        "<S-1>",
        "<S-Space>",
        "<C-S-Space>",
        "<S-é>",
        "<S-lt>",
        "<C-a",
        "<C-x",
        "<D-",
        "<A-b c",
    ] {
        if typo == "<D-" {
            // Nothing after the dash: a literal `<`, as `<C-` always was.
            assert_eq!(keys(typo), [ch('<'), ch('D'), ch('-')]);
            continue;
        }
        assert!(parse_keys(typo).is_err(), "{typo}");
        assert!(parse_pats(typo).is_err(), "{typo}");
    }
    let unclosed = parse_keys("x<C-a").unwrap_err();
    assert!(unclosed.contains("unclosed <C-a"), "{unclosed}");
    let shifted = parse_keys("<C-S-2>").unwrap_err();
    assert!(shifted.contains("<C-@>"), "{shifted}");
    let space = parse_keys("<S-Space>").unwrap_err();
    assert!(space.contains("write <Space>"), "{space}");
    // Shift still means what it says on letters and named keys.
    assert_eq!(one("<S-a>"), ch('A'));
    assert!(parse_keys("<C-S-a>").is_ok());
    assert!(parse_keys("<S-Tab>").is_ok());
    assert!(parse_keys("<S-F1>").is_ok());
    // An event with Shift held on Space still matches <Space>: the layout sends plain Space.
    let shift_space = with(ch(' '), |m| m.shift = true);
    assert_eq!(
        parse_pats("<Space>").unwrap(),
        [Pat::Chord(shift_space.chord())]
    );
}

#[test]
fn num_lock_and_caps_lock_dont_change_what_a_key_matches() {
    let plain = ch('a');
    let locked = KeyEvent {
        num_lock: true,
        caps_lock: true,
        ..plain.clone()
    };
    assert_eq!(locked.chord(), plain.chord());
    assert_eq!(locked.printable(), plain.printable());
    assert!(!KeyEvent::press(Key::Char('a')).num_lock);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn single_cluster_texts_round_trip_as_one_key_each(
        picks in prop::collection::vec(0..CLUSTERS.len(), 1..6),
    ) {
        let texts: Vec<&str> = picks.iter().map(|&i| CLUSTERS[i]).collect();
        let evs: Vec<_> = texts.iter().map(|t| KeyEvent::committed(t)).collect();
        let s = key_notation(&evs);
        let back = parse_keys(&s).map_err(|e| TestCaseError::fail(format!("{s:?}: {e}")))?;
        prop_assert_eq!(back.len(), texts.len(), "via {:?}", s);
        for (k, t) in back.iter().zip(&texts) {
            prop_assert_eq!(k.printable(), Some(t.to_string()));
        }
    }
}
