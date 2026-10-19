//! Full key events (press, repeat, release) and Vim-style key notation.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Tab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods {
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

#[derive(Clone, PartialEq, Debug)]
pub struct KeyEvent {
    pub key: Key,
    pub mods: Mods,
    pub kind: KeyKind,
    /// Committed text, for dead keys and the Character Viewer ("é" from Option-e e).
    pub text: Option<String>,
}

/// What a keymap matches: a key plus modifiers, with Shift folded into characters.
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
        }
    }
    /// Committed text, as delivered for dead keys.
    pub fn committed(s: &str) -> Self {
        let c = s.chars().next().unwrap_or(' ');
        KeyEvent {
            key: Key::Char(c),
            mods: Mods::default(),
            kind: KeyKind::Press,
            text: Some(s.into()),
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
    pub fn chord(&self) -> Chord {
        let mut mods = self.mods;
        if let Key::Char(_) = self.key {
            mods.shift = false;
        }
        Chord {
            key: self.key,
            mods,
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
}

pub const LEADER: char = '\\';

/// Parse notation such as `gg`, `<C-x>`, `<leader>o`, `<S-Tab>`, `<D-Esc>` (Cmd), `<char>`, `<any>`.
pub fn parse_pats(s: &str) -> Result<Vec<Pat>, String> {
    let mut out = vec![];
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '<' {
            out.push(Pat::Chord(Chord {
                key: Key::Char(c),
                mods: Mods::default(),
            }));
            continue;
        }
        let mut tok = String::new();
        loop {
            match it.next() {
                Some('>') => break,
                Some(ch) => tok.push(ch),
                None => return Err(format!("unclosed < in {s:?}")),
            }
        }
        out.push(parse_token(&tok).ok_or_else(|| format!("bad key <{tok}> in {s:?}"))?);
    }
    Ok(out)
}

fn parse_token(tok: &str) -> Option<Pat> {
    match tok {
        "char" => return Some(Pat::AnyChar),
        "any" => return Some(Pat::Any),
        _ => {}
    }
    let mut mods = Mods::default();
    let mut parts: Vec<&str> = tok.split('-').collect();
    let name = if tok.ends_with("--") {
        "-"
    } else {
        parts.pop()?
    };
    for m in parts {
        match m {
            "C" => mods.ctrl = true,
            "A" | "M" => mods.alt = true,
            "S" => mods.shift = true,
            "D" => mods.cmd = true,
            "" => {}
            _ => return None,
        }
    }
    let key = match name {
        "Esc" => Key::Esc,
        "CR" | "Enter" | "Return" => Key::Enter,
        "Tab" => Key::Tab,
        "BS" => Key::Backspace,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "Space" => Key::Char(' '),
        "lt" => Key::Char('<'),
        "leader" => Key::Char(LEADER),
        n if n.chars().count() == 1 => Key::Char(n.chars().next()?),
        _ => return None,
    };
    if let Key::Char(_) = key {
        mods.shift = false;
    }
    Some(Pat::Chord(Chord { key, mods }))
}

/// Parse notation into concrete key presses, for `feed_keys` and the harness.
pub fn parse_keys(s: &str) -> Result<Vec<KeyEvent>, String> {
    parse_pats(s)?
        .into_iter()
        .map(|p| match p {
            Pat::Chord(c) => Ok(KeyEvent::press(c.key).mods(c.mods)),
            _ => Err(format!("wildcards can't be typed: {s:?}")),
        })
        .collect()
}

/// The inverse of `parse_keys`, so die-vim can store a recorded macro as register text.
pub fn key_notation(evs: &[KeyEvent]) -> String {
    let mut s = String::new();
    for e in evs {
        let m = e.mods;
        let name = match e.key {
            Key::Char('<') => "lt".to_string(),
            Key::Char(c) if !(m.ctrl || m.alt || m.cmd) => {
                s.push(c);
                continue;
            }
            Key::Char(c) => c.to_string(),
            k => format!("{k:?}")
                .replace("Enter", "CR")
                .replace("Backspace", "BS"),
        };
        let pre = [
            (m.ctrl, "C-"),
            (m.alt, "A-"),
            (m.shift, "S-"),
            (m.cmd, "D-"),
        ]
        .iter()
        .filter(|p| p.0)
        .map(|p| p.1)
        .collect::<String>();
        s.push_str(&format!("<{pre}{name}>"));
    }
    s
}
