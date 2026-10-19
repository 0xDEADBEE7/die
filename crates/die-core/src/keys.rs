//! Full key events (press, repeat, release) and Vim-style key notation (contract §7).
//!
//! One parser serves keymaps, `feed_keys`, the harness and the Evil oracle's fixtures. As
//! in Vim, `<...>` is one key only when its name is known, so `<<` is two `<`s.

/// The key the keyboard layout produced: a character, or a named key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// A character, with Shift already applied (`A`, `$`).
    Char(char),
    Esc,
    Enter,
    Tab,
    Backspace,
    /// Forward delete.
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    /// A function key, `F(1)` to `F(24)`.
    F(u8),
    /// A modifier key pressed or released on its own. These reach only surfaces in
    /// passthrough, for the terminal's key reporting; keymaps never see them.
    Modifier(ModifierKey),
}

/// Modifier keys, as keys in their own right. Which side was pressed is in `KeyEvent::code`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModifierKey {
    Shift,
    Control,
    Alt,
    Command,
    CapsLock,
}

/// Modifiers in effect for a key event. Keymaps match `ctrl`, `alt`, `shift` and `cmd`
/// (Shift only on named keys); the rest is for the terminal's key encoding (M2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    /// Option. DIE treats both Option keys as Alt.
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Caps Lock is on.
    pub caps_lock: bool,
    /// Which held modifiers are the right-hand key.
    pub right: RightMods,
}

/// The modifiers held on the right-hand side of the keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct RightMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

/// One key event, from the OS or from notation (scope §5.6: full key events). It carries
/// what the terminal's key encoder needs (spike S1): the action, the logical key, the
/// physical key, the unshifted character, the modifiers with their sides, and any text.
#[derive(Clone, PartialEq, Debug)]
pub struct KeyEvent {
    pub key: Key,
    pub mods: Mods,
    pub kind: KeyKind,
    /// Committed text, for dead keys and the Character Viewer ("é" from Option-e e).
    pub text: Option<String>,
    /// The physical key, as a W3C `code` name ("KeyA", "Digit4", "ShiftRight"), when the
    /// OS gave one. None for keys from notation.
    pub code: Option<String>,
    /// What the key types with no modifiers held (`4` for `$`), when the OS gave it.
    pub unshifted: Option<char>,
}

/// What a keymap matches: a key plus Ctrl, Alt, Cmd, and Shift on named keys only.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub key: Key,
    pub mods: Mods,
}

impl KeyEvent {
    pub fn press(key: Key) -> Self {
        KeyEvent {
            key,
            mods: Mods::default(),
            kind: KeyKind::Press,
            text: None,
            code: None,
            unshifted: None,
        }
    }
    /// Committed text, as delivered for dead keys.
    pub fn committed(s: &str) -> Self {
        let c = s.chars().next().unwrap_or(' ');
        KeyEvent {
            text: Some(s.into()),
            ..KeyEvent::press(Key::Char(c))
        }
    }
    pub fn mods(mut self, m: Mods) -> Self {
        self.mods = m;
        self
    }
    pub fn kind(mut self, k: KeyKind) -> Self {
        self.kind = k;
        self
    }
    /// The chord a keymap matches: Caps Lock, sides and Shift on characters dropped.
    pub fn chord(&self) -> Chord {
        let m = self.mods;
        Chord {
            key: self.key,
            mods: Mods {
                ctrl: m.ctrl,
                alt: m.alt,
                shift: m.shift && !matches!(self.key, Key::Char(_)),
                cmd: m.cmd,
                ..Mods::default()
            },
        }
    }
    /// The text this key types, if it is printable. `<char>` matches exactly these keys.
    pub fn printable(&self) -> Option<String> {
        if let Some(t) = &self.text {
            return Some(t.clone());
        }
        match self.key {
            Key::Char(c)
                if !self.mods.ctrl && !self.mods.alt && !self.mods.cmd && !c.is_control() =>
            {
                Some(c.to_string())
            }
            _ => None,
        }
    }
}

/// One step of a key pattern in a keymap.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Pat {
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
    ("Command", Key::Modifier(ModifierKey::Command)),
    ("CapsLock", Key::Modifier(ModifierKey::CapsLock)),
];

/// Parse notation such as `gg`, `<C-x>`, `<leader>o`, `<S-Tab>`, `<D-Esc>` (Cmd), `<char>`,
/// `<any>`. Names are matched without regard to case. Ctrl with a letter folds its case, as
/// in Vim (`<C-V>` is `<C-v>`); `<C-S-a>` is Ctrl-Shift-A. A `<` that doesn't start a known
/// name is a literal `<`, but a bracketed word that isn't a key name is an error, since it's
/// almost always a typo: write `<lt>` for a literal `<`.
pub fn parse_pats(s: &str) -> Result<Vec<Pat>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < c.len() {
        if c[i] == '<'
            && let Some((pat, len)) = bracket(&c[i..]).map_err(|e| format!("{e} in {s:?}"))?
        {
            out.push(pat);
            i += len;
            continue;
        }
        out.push(Pat::Chord(Chord {
            key: Key::Char(c[i]),
            mods: Mods::default(),
        }));
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
            'D' => mods.cmd = true,
            _ => break,
        }
        i += 2;
    }
    let has_mods = i > 1;
    // One character after modifiers: `<C-x>`, `<C->>`, `<C-->`.
    if has_mods && c.get(i + 1) == Some(&'>') {
        return Ok(Some((chord(Key::Char(c[i]), mods), i + 2)));
    }
    let Some(end) = close(i) else { return Ok(None) };
    let name: String = c[i..end].iter().collect();
    let word = name.len() >= 2 && name.chars().all(|ch| ch.is_ascii_alphanumeric());
    if !word {
        return Ok(None);
    }
    let lower = name.to_ascii_lowercase();
    let pat = match lower.as_str() {
        "char" if !has_mods => Pat::AnyChar,
        "any" if !has_mods => Pat::Any,
        "leader" if !has_mods => Pat::Leader,
        "nl" => {
            mods.ctrl = true;
            chord(Key::Char('j'), mods)
        }
        _ => match key_named(&lower) {
            Some(key) => chord(key, mods),
            None => {
                return Err(format!(
                    "unknown key <{}>",
                    c[1..end].iter().collect::<String>()
                ));
            }
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

/// A chord from notation: Shift is folded into a character, and Ctrl folds a letter's case
/// unless Shift was written.
fn chord(key: Key, mut mods: Mods) -> Pat {
    let key = match key {
        Key::Char(c) if c.is_ascii_alphabetic() && mods.shift => Key::Char(c.to_ascii_uppercase()),
        Key::Char(c) if mods.ctrl => Key::Char(c.to_ascii_lowercase()),
        k => k,
    };
    if let Key::Char(_) = key {
        mods.shift = false;
    }
    Pat::Chord(Chord { key, mods })
}

/// Parse notation into concrete key presses, for `feed_keys` and the harness. `<char>`,
/// `<any>` and `<leader>` can't be typed (`Ctx::feed_keys` resolves `<leader>`).
pub fn parse_keys(s: &str) -> Result<Vec<KeyEvent>, String> {
    typed(s, None)
}

/// `parse_keys`, with `<leader>` resolved when a leader is given.
pub(crate) fn typed(s: &str, leader: Option<Chord>) -> Result<Vec<KeyEvent>, String> {
    parse_pats(s)?
        .into_iter()
        .map(|p| match (p, leader) {
            (Pat::Chord(c), _) | (Pat::Leader, Some(c)) => Ok(KeyEvent::press(c.key).mods(c.mods)),
            _ => Err(format!("{s:?} has a key pattern, which can't be typed")),
        })
        .collect()
}

/// The inverse of `parse_keys`, so die-vim can store a recorded macro as register text.
pub fn key_notation(evs: &[KeyEvent]) -> String {
    let mut s = String::new();
    for e in evs {
        let m = e.chord().mods;
        let mut shift = m.shift;
        let name = match e.key {
            Key::Char('<') if !(m.ctrl || m.alt || m.cmd) => "lt".to_string(),
            Key::Char(c) if !(m.ctrl || m.alt || m.cmd) => {
                s.push(c);
                continue;
            }
            // Ctrl folds a letter's case, so a capital is written with Shift.
            Key::Char(c) if m.ctrl && c.is_ascii_uppercase() => {
                shift = true;
                c.to_ascii_lowercase().to_string()
            }
            Key::Char(' ') => "Space".into(),
            Key::Char('<') => "lt".into(),
            Key::Char(c) => c.to_string(),
            Key::F(n) => format!("F{n}"),
            k => NAMES
                .iter()
                .find(|(_, x)| *x == k)
                .map_or("?", |(n, _)| n)
                .to_string(),
        };
        let pre: String = [(m.ctrl, "C-"), (m.alt, "A-"), (shift, "S-"), (m.cmd, "D-")]
            .iter()
            .filter(|p| p.0)
            .map(|p| p.1)
            .collect();
        s.push_str(&format!("<{pre}{name}>"));
    }
    s
}
