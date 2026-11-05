//! Full key events (press, repeat, release) and Vim-style key notation (contract §7).
//!
//! One parser serves keymaps, `feed_keys`, the harness and the Evil oracle's fixtures. As
//! in Vim, `<...>` is one key only when its name is known, so `<<` is two `<`s.
#![warn(missing_docs)]

use unicode_segmentation::UnicodeSegmentation;

/// The key the keyboard layout produced: a character, or a named key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// A character, with Shift already applied (`A`, `$`).
    Char(char),
    /// Escape.
    Esc,
    /// Return.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert.
    Insert,
    /// A function key, `F(1)` to `F(24)`.
    F(u8),
    /// A modifier key pressed or released on its own. These reach only surfaces in
    /// passthrough, for the terminal's key reporting; keymaps never see them.
    Modifier(ModifierKey),
    /// A key with no name here; its physical key is in `KeyEvent::code`. Keymaps never match
    /// it; passthrough forwards it.
    Unidentified,
}

/// Modifier keys, as keys in their own right. Which side was pressed is in `KeyEvent::code`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModifierKey {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Alt (Option on macOS).
    Alt,
    /// Super: Cmd on macOS, the logo key on Linux.
    Super,
    /// Caps Lock.
    CapsLock,
}

/// Modifiers held for a key event.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods {
    /// Control.
    pub ctrl: bool,
    /// Alt (Option on macOS). DIE treats both Option keys as Alt.
    pub alt: bool,
    /// Shift. A character already has it applied, so it counts only on named keys and with
    /// Ctrl and a letter (`Chord`).
    pub shift: bool,
    /// Super: Cmd on macOS, the logo key on Linux. Notation writes it `<D-…>`, which names
    /// the DIE layer (scope D18); the layer is Super until config sets it, by M2.
    pub super_: bool,
}

/// What a key did. Keymaps match `Press` and `Repeat`; `Release` reaches only surfaces in
/// passthrough (contract §7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    /// The key went down.
    Press,
    /// The key is held, and the OS repeated it.
    Repeat,
    /// The key came up.
    Release,
}

/// One key event, from the OS or from notation (scope §5.6: full key events). It carries
/// what the terminal's key encoder needs (spike S1): the action, the logical key, the
/// physical key, the unshifted character, the modifiers with their sides, and any text.
#[derive(Clone, PartialEq, Debug)]
pub struct KeyEvent {
    /// The logical key: what the layout produced, with Shift applied.
    pub key: Key,
    /// The modifiers held.
    pub mods: Mods,
    /// Press, repeat or release.
    pub kind: KeyKind,
    /// Text the OS committed beyond what `key` names: a dead-key composition on layouts that
    /// have dead keys (´ then e gives é; Option's dead keys don't occur, since Option is Alt),
    /// an input method's commit, the Character Viewer or the emoji picker. None for ordinary
    /// keys, and None whenever Ctrl, Alt or Super is held: R4 doesn't copy the OS's per-key
    /// text, which is a C0 control character for Ctrl chords (\u{1} for C-a) and the plain
    /// letter for Alt (x for M-x).
    pub text: Option<String>,
    /// The physical key, as a W3C `code` name ("KeyA", "Digit4", "ShiftRight"), when the
    /// OS gave one. None for keys from notation.
    pub code: Option<String>,
    /// What the key types with no modifiers held (`4` for `$`), when the OS gave it. With
    /// `key`, this gives the terminal's consumed modifiers: Shift is consumed when `key`'s
    /// character differs from this one; Alt never is, since Option is Alt.
    pub unshifted: Option<char>,
    /// Which of `mods` are held on the right-hand side, for the terminal's key encoding (M2).
    pub right_mods: Mods,
    /// Caps Lock is on. Keymaps ignore it.
    pub caps_lock: bool,
    /// Num Lock is on, where the platform has it (not macOS). Keymaps ignore it.
    pub num_lock: bool,
}

/// What a keymap matches. `KeyEvent::chord` and notation fold keys the same way: Shift is
/// already in a character (`A`, `$`), so it counts only on named keys (`<S-Tab>`) and with
/// Ctrl and a letter. That letter is folded to lowercase, so Ctrl-a is `<C-a>` and
/// Ctrl-Shift-a is `<C-S-a>`, with Caps Lock on or off. And Ctrl-[, Ctrl-i and Ctrl-m alone
/// are `<Esc>`, `<Tab>` and `<CR>`, as in GUI Emacs and Vim, so they can't be bound apart.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    /// The key, folded as above.
    pub key: Key,
    /// The modifiers that count.
    pub mods: Mods,
}

/// The chord for `key` with `mods` held.
fn fold(key: Key, mut mods: Mods) -> Chord {
    let key = match key {
        Key::Char(c) if mods.ctrl && c.is_ascii_alphabetic() => Key::Char(c.to_ascii_lowercase()),
        Key::Char(_) => {
            mods.shift = false;
            key
        }
        k => k,
    };
    let bare_ctrl = mods.ctrl && !mods.alt && !mods.super_ && !mods.shift;
    let alias = match key {
        Key::Char('[') if bare_ctrl => Some(Key::Esc),
        Key::Char('i') if bare_ctrl => Some(Key::Tab),
        Key::Char('m') if bare_ctrl => Some(Key::Enter),
        _ => None,
    };
    if let Some(key) = alias {
        mods.ctrl = false;
        return Chord { key, mods };
    }
    Chord { key, mods }
}

impl KeyEvent {
    /// A press of `key`, with no modifiers and none of the OS's fields.
    pub fn press(key: Key) -> Self {
        KeyEvent {
            key,
            mods: Mods::default(),
            kind: KeyKind::Press,
            text: None,
            code: None,
            unshifted: None,
            right_mods: Mods::default(),
            caps_lock: false,
            num_lock: false,
        }
    }
    /// Committed text, as delivered for dead keys. `key` is its first character. Empty text
    /// is no key at all: `Unidentified`, with no notation.
    pub fn committed(s: &str) -> Self {
        KeyEvent {
            text: Some(s.into()),
            ..KeyEvent::press(s.chars().next().map_or(Key::Unidentified, Key::Char))
        }
    }
    /// This event with `m` held.
    pub fn mods(mut self, m: Mods) -> Self {
        self.mods = m;
        self
    }
    /// This event as a press, repeat or release.
    pub fn kind(mut self, k: KeyKind) -> Self {
        self.kind = k;
        self
    }
    /// The chord a keymap matches (see `Chord`).
    pub fn chord(&self) -> Chord {
        fold(self.key, self.mods)
    }
    /// The text this key types, or None: with Ctrl, Alt or Super held, for empty text, and for
    /// text containing a control character. `<char>` matches exactly the keys where this is Some.
    pub fn printable(&self) -> Option<String> {
        if self.mods.ctrl || self.mods.alt || self.mods.super_ {
            return None;
        }
        match (&self.text, self.key) {
            (Some(t), _) => (!t.is_empty() && !t.chars().any(char::is_control)).then(|| t.clone()),
            (None, Key::Char(c)) if !c.is_control() => Some(c.to_string()),
            _ => None,
        }
    }
}

/// One step of a key pattern in a keymap.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Pat {
    /// One key, folded as `KeyEvent::chord` folds an event.
    Chord(Chord),
    /// `<char>`: any printable character, including committed text.
    AnyChar,
    /// `<any>`: any key at all.
    Any,
    /// `<leader>`: the leader key from config (`Registry::set_leader`), `\` by default.
    Leader,
}

/// Names for keys in notation, matched without regard to case. The first name for a key
/// is the one `key_notation` writes.
const NAMES: &[(&str, Key)] = &[
    ("Esc", Key::Esc),
    ("Escape", Key::Esc),
    ("CR", Key::Enter),
    ("Enter", Key::Enter),
    ("Return", Key::Enter),
    ("Tab", Key::Tab),
    ("BS", Key::Backspace),
    ("Backspace", Key::Backspace),
    ("Del", Key::Delete),
    ("Delete", Key::Delete),
    ("Up", Key::Up),
    ("Down", Key::Down),
    ("Left", Key::Left),
    ("Right", Key::Right),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Insert", Key::Insert),
    ("Space", Key::Char(' ')),
    ("lt", Key::Char('<')),
    ("Bslash", Key::Char('\\')),
    ("Bar", Key::Char('|')),
    ("Shift", Key::Modifier(ModifierKey::Shift)),
    ("Control", Key::Modifier(ModifierKey::Control)),
    ("Alt", Key::Modifier(ModifierKey::Alt)),
    ("Super", Key::Modifier(ModifierKey::Super)),
    ("Command", Key::Modifier(ModifierKey::Super)),
    ("CapsLock", Key::Modifier(ModifierKey::CapsLock)),
];

/// Parse notation such as `gg`, `<C-x>`, `<leader>o`, `<S-Tab>`, `<D-Esc>` (the DIE layer's
/// Esc), `<char>`, `<any>`. Names are matched without regard to case. Ctrl with a letter folds
/// its case, as in Vim (`<C-V>` is `<C-v>`); `<C-S-a>` is Ctrl-Shift-A. A `<` that doesn't
/// start a key is a literal `<`, but a bracketed word that isn't a key name, modifiers
/// followed by something that isn't a key (`<C-X-a>`) or with no closing `>` (`<C-a`), and
/// Shift written with a character it can't change (`<C-S-2>`, `<S-Space>`) are errors, since
/// they're almost always typos: write `<lt>` for a literal `<`, and the shifted character
/// itself, as in `<C-@>`. `<C-[>`, `<C-i>` and `<C-m>` are `<Esc>`, `<Tab>` and `<CR>`. Literal
/// characters that form one grapheme cluster are one key, whose chord holds only the first
/// of them, so a pattern can't match it and they're an error too: bind `<char>`.
pub fn parse_pats(s: &str) -> Result<Vec<Pat>, String> {
    let steps = parse(s)?;
    for run in steps.chunk_by(|a, b| a.1 && b.1) {
        let text: String = run.iter().filter_map(literal).collect();
        if let Some(g) = text.graphemes(true).find(|g| one_key(g)) {
            return Err(format!(
                "one key can't be bound to {g}: bind <char> and read Args::typed"
            ));
        }
    }
    Ok(steps.into_iter().map(|(p, _)| p).collect())
}

/// The character a step of `parse` types, if it's a literal one.
fn literal(step: &(Pat, bool)) -> Option<char> {
    match step {
        (
            Pat::Chord(Chord {
                key: Key::Char(c), ..
            }),
            true,
        ) => Some(*c),
        _ => None,
    }
}

/// Whether a grapheme cluster is one key rather than a key per character. A cluster with a
/// control character in it (CR LF) isn't: committed text can't hold one.
fn one_key(g: &str) -> bool {
    g.chars().nth(1).is_some() && !g.chars().any(char::is_control)
}

/// `parse_pats`, saying which steps are literal characters rather than `<...>`.
fn parse(s: &str) -> Result<Vec<(Pat, bool)>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < c.len() {
        if c[i] == '<'
            && let Some((pat, len)) = bracket(&c[i..]).map_err(|e| format!("{e} in {s:?}"))?
        {
            out.push((pat, false));
            i += len;
            continue;
        }
        let literal = Chord {
            key: Key::Char(c[i]),
            mods: Mods::default(),
        };
        out.push((Pat::Chord(literal), true));
        i += 1;
    }
    Ok(out)
}

/// The key named by a `<...>` at the start of `c`, and how many characters it takes.
/// Ok(None) means the `<` is a literal character.
fn bracket(c: &[char]) -> Result<Option<(Pat, usize)>, String> {
    let close = |from: usize| (from..c.len()).find(|&j| c[j] == '>');
    let mut mods = Mods::default();
    let mut i = 1;
    // Modifiers: a letter and a dash, with something after them.
    while i + 2 < c.len() && c[i + 1] == '-' {
        match c[i].to_ascii_uppercase() {
            'C' => mods.ctrl = true,
            'A' | 'M' => mods.alt = true,
            'S' => mods.shift = true,
            'D' => mods.super_ = true,
            _ => break,
        }
        i += 2;
    }
    let has_mods = i > 1;
    // One character after modifiers: `<C-x>`, `<C->>`, `<C-->`.
    if has_mods && c.get(i + 1) == Some(&'>') {
        return Ok(Some((chord(Key::Char(c[i]), mods)?, i + 2)));
    }
    let Some(end) = close(i) else {
        return if has_mods {
            Err(format!("unclosed <{}", String::from_iter(&c[1..])))
        } else {
            Ok(None)
        };
    };
    let name: String = c[i..end].iter().collect();
    let word = name.len() >= 2 && name.chars().all(|ch| ch.is_ascii_alphanumeric());
    let unknown = || Err(format!("unknown key <{}>", String::from_iter(&c[1..end])));
    if !word {
        return if has_mods { unknown() } else { Ok(None) };
    }
    let lower = name.to_ascii_lowercase();
    let pat = match lower.as_str() {
        "char" if !has_mods => Pat::AnyChar,
        "any" if !has_mods => Pat::Any,
        "leader" if !has_mods => Pat::Leader,
        "nl" => {
            mods.ctrl = true;
            chord(Key::Char('j'), mods)?
        }
        _ => match key_named(&lower) {
            Some(key) => chord(key, mods)?,
            None => return unknown(),
        },
    };
    Ok(Some((pat, end + 1)))
}

fn key_named(lower: &str) -> Option<Key> {
    if let Some((_, k)) = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(lower)) {
        return Some(*k);
    }
    let n: u8 = lower.strip_prefix('f')?.parse().ok()?;
    (1..=24).contains(&n).then_some(Key::F(n))
}

/// A chord from notation: a written Shift capitalizes a letter, as the layout would, and then
/// the key folds as an event's does. Any other character can't take a written Shift: the
/// layout already applied it, so no key sends what it says.
fn chord(key: Key, mods: Mods) -> Result<Pat, String> {
    let key = match key {
        Key::Char(c) if mods.shift && c.is_ascii_alphabetic() => Key::Char(c.to_ascii_uppercase()),
        Key::Char(' ') if mods.shift => {
            return Err("Shift doesn't change Space: write <Space>".into());
        }
        Key::Char(_) if mods.shift => {
            return Err("write the shifted character, as in <C-@>, not <C-S-2>".into());
        }
        k => k,
    };
    Ok(Pat::Chord(fold(key, mods)))
}

/// Parse notation into concrete key presses, for `feed_keys` and the harness. `<char>`,
/// `<any>` and `<leader>` can't be typed (`Ctx::feed_keys` resolves `<leader>`).
pub fn parse_keys(s: &str) -> Result<Vec<KeyEvent>, String> {
    typed(s, None)
}

/// `parse_keys`, with `<leader>` resolved when a leader is given. Literal characters that
/// form one grapheme cluster are one key, as committed text, so a recorded `r❤️` replays as
/// one key and not as `r❤` and a stray variation selector.
pub(crate) fn typed(s: &str, leader: Option<Chord>) -> Result<Vec<KeyEvent>, String> {
    let (mut out, mut run) = (vec![], String::new());
    for step in parse(s)? {
        if let Some(c) = literal(&step) {
            run.push(c);
            continue;
        }
        literals(&mut run, &mut out);
        match (step.0, leader) {
            (Pat::Chord(c), _) | (Pat::Leader, Some(c)) => {
                out.push(KeyEvent::press(c.key).mods(c.mods));
            }
            _ => return Err(format!("{s:?} has a key pattern, which can't be typed")),
        }
    }
    literals(&mut run, &mut out);
    Ok(out)
}

/// Turn a run of literal characters into keys, one per grapheme cluster.
fn literals(run: &mut String, out: &mut Vec<KeyEvent>) {
    for g in run.graphemes(true) {
        if one_key(g) {
            out.push(KeyEvent::committed(g));
        } else {
            out.extend(g.chars().map(|c| KeyEvent::press(Key::Char(c))));
        }
    }
    run.clear();
}

/// The inverse of `parse_keys`, so die-vim can store a recorded macro as register text.
/// Committed text is written as its characters, which parse back as one key per grapheme
/// cluster.
pub fn key_notation(evs: &[KeyEvent]) -> String {
    let mut s = String::new();
    let literal = |s: &mut String, c| match c {
        '<' => s.push_str("<lt>"),
        c => s.push(c),
    };
    for e in evs {
        // The OS's text for a Ctrl chord isn't what the key types: the chord is.
        if let (Some(t), Some(_)) = (&e.text, e.printable()) {
            t.chars().for_each(|c| literal(&mut s, c));
            continue;
        }
        let Chord { key, mods } = e.chord();
        let Mods {
            ctrl,
            alt,
            shift,
            super_,
        } = mods;
        let name = match key {
            Key::Char(c) if !(ctrl || alt || super_) => {
                literal(&mut s, c);
                continue;
            }
            Key::Char(' ') => "Space".into(),
            Key::Char('<') => "lt".into(),
            Key::Char(c) => c.to_string(),
            Key::F(n) => format!("F{n}"),
            Key::Unidentified => continue,
            k => NAMES
                .iter()
                .find(|(_, x)| *x == k)
                .map_or("?", |(n, _)| n)
                .to_string(),
        };
        let pre: String = [(ctrl, "C-"), (alt, "A-"), (shift, "S-"), (super_, "D-")]
            .iter()
            .filter(|p| p.0)
            .map(|p| p.1)
            .collect();
        s.push_str(&format!("<{pre}{name}>"));
    }
    s
}
