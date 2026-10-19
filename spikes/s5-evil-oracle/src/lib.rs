//! Types for the Evil oracle's fixtures (spike S5).
//!
//! The oracle (`oracle/evil-oracle.el`, run by `run-oracle.sh`) runs each
//! case's keys through the user's Evil and writes one JSON file per topic
//! to `fixtures/`. These types are the schema die-vim's tests will read;
//! `validate` checks that a fixture is internally consistent.
//!
//! Positions are 0-based: `line` counts `\n`s before the position, `col`
//! counts characters (Unicode scalar values) from the start of its line,
//! and `byte` is the UTF-8 offset from the start of the text.

use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

/// The schema version this crate reads.
pub const SCHEMA: u32 = 1;

/// One fixture file: every case of one topic.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub schema: u32,
    pub topic: String,
    pub generator: String,
    /// `batch` for `emacs --batch`; `tty` for cases that need a real
    /// redisplay (window geometry, wrapped lines).
    pub run: Run,
    pub evil_version: String,
    /// SHA-256 over Evil's `.el` files, naming the exact Evil used.
    pub evil_source_sha256: String,
    pub emacs_version: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Run {
    Batch,
    Tty,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub description: Option<String>,
    /// `oracle` when Evil produced `expected`; `hand-written` otherwise.
    pub source: Source,
    /// Why a hand-written case couldn't come from the oracle.
    pub reason: Option<String>,
    /// The keys in Vim notation, as written in the case file.
    pub keys: String,
    /// `keys` split into single keys: a character, or `<Name>`.
    pub key_tokens: Vec<String>,
    /// The Emacs major mode the case ran in.
    pub mode: String,
    pub input: Input,
    pub expected: Expected,
    /// How textbook Vim differs, when it does (die-vim follows Evil).
    pub vim: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Oracle,
    HandWritten,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub text: String,
    pub cursor: Pos,
    pub visual: Option<Selection>,
    pub registers: Option<BTreeMap<String, Register>>,
    /// Text another app put on the macOS clipboard before the keys.
    pub clipboard: Option<String>,
    pub window: Option<Window>,
    /// Byte offsets where each screen line starts (wrapped lines).
    pub layout: Option<Vec<usize>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub text: String,
    pub cursor: Pos,
    pub state: State,
    pub visual: Option<Selection>,
    /// Always has the unnamed register `"`, plus every register the keys
    /// changed (`0`-`9`, `a`-`z`, `-`).  `None` means empty.
    pub registers: BTreeMap<String, Option<Register>>,
    pub clipboard: Clipboard,
    pub window: Option<Window>,
    pub errors: Option<Vec<ErrorInfo>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
    pub byte: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(rename = "type")]
    pub kind: SelectionKind,
    pub start: Pos,
    /// Exclusive end.
    pub end: Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionKind {
    Char,
    Line,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Normal,
    Insert,
    Visual,
    Replace,
    Operator,
    Motion,
    Emacs,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Register {
    pub text: String,
    #[serde(rename = "type")]
    pub kind: RegisterKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegisterKind {
    Char,
    Line,
    Block,
    /// A recorded macro; `text` is its keys in Vim notation.
    Macro,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clipboard {
    /// What the macOS clipboard holds after the keys.
    pub text: Option<String>,
    /// Every write to the clipboard, in order (Evil's `kill-new`).
    pub writes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub width: usize,
    pub height: usize,
    pub top_line: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorInfo {
    pub symbol: String,
    pub message: String,
    /// How many key events had been read when the error happened.
    pub after_keys: usize,
}

/// Vim key names the oracle understands (matched case-insensitively).
const KEY_NAMES: &[&str] = &[
    "Esc", "CR", "Enter", "Return", "NL", "BS", "Tab", "Space", "lt", "Bslash", "Bar", "Del",
    "Up", "Down", "Left", "Right", "Home", "End",
];

/// Split Vim-notation keys into tokens, the way die-vim will: `<Name>` is
/// one key when Name is known (or `C-x`), otherwise `<` is a literal.
pub fn parse_keys(keys: &str) -> Vec<String> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(len) = chars[i + 1..].iter().position(|&c| c == '>') {
                let name: String = chars[i + 1..i + 1 + len].iter().collect();
                if let Some(tok) = key_name(&name) {
                    out.push(tok);
                    i += len + 2;
                    continue;
                }
            }
        }
        out.push(chars[i].to_string());
        i += 1;
    }
    out
}

fn key_name(name: &str) -> Option<String> {
    let n: Vec<char> = name.chars().collect();
    if n.len() == 3 && (n[0] == 'C' || n[0] == 'c') && n[1] == '-' {
        return Some(format!("<C-{}>", n[2].to_lowercase()));
    }
    KEY_NAMES
        .iter()
        .find(|k| k.eq_ignore_ascii_case(name))
        .map(|k| format!("<{k}>"))
}

/// The position of `byte` in `text`, or an error if it isn't on a char
/// boundary inside the text.
pub fn pos_of_byte(text: &str, byte: usize) -> Result<Pos, String> {
    if byte > text.len() || !text.is_char_boundary(byte) {
        return Err(format!("byte {byte} is not a char boundary of the text"));
    }
    let before = &text[..byte];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Ok(Pos { line, col: text[line_start..byte].chars().count(), byte })
}

fn check_pos(text: &str, p: Pos, what: &str) -> Result<(), String> {
    let want = pos_of_byte(text, p.byte).map_err(|e| format!("{what}: {e}"))?;
    if want != p {
        return Err(format!("{what}: {p:?} but byte {} is {want:?}", p.byte));
    }
    Ok(())
}

fn check_selection(text: &str, s: &Selection, what: &str) -> Result<(), String> {
    check_pos(text, s.start, &format!("{what} start"))?;
    check_pos(text, s.end, &format!("{what} end"))?;
    if s.kind != SelectionKind::Block && s.start.byte > s.end.byte {
        return Err(format!("{what}: start after end"));
    }
    Ok(())
}

fn check_register(name: &str, r: &Register, what: &str) -> Result<(), String> {
    let ok_name = name == "\""
        || (name.len() == 1 && name.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase() || c == '-'));
    if !ok_name {
        return Err(format!("{what}: bad register name {name:?}"));
    }
    if r.kind == RegisterKind::Line && !r.text.ends_with('\n') {
        return Err(format!("{what}: linewise register {name} doesn't end in a newline"));
    }
    Ok(())
}

impl Case {
    /// Check that the case is internally consistent.
    pub fn validate(&self, run: Run) -> Result<(), String> {
        if self.id.is_empty() || self.keys.is_empty() {
            return Err("empty id or keys".into());
        }
        if self.source == Source::HandWritten && self.reason.is_none() {
            return Err("hand-written case without a reason".into());
        }
        let tokens = parse_keys(&self.keys);
        if tokens != self.key_tokens {
            return Err(format!("key_tokens {:?} but parse_keys gives {tokens:?}", self.key_tokens));
        }
        let (i, e) = (&self.input, &self.expected);
        check_pos(&i.text, i.cursor, "input cursor")?;
        check_pos(&e.text, e.cursor, "expected cursor")?;
        if let Some(v) = &i.visual {
            check_selection(&i.text, v, "input selection")?;
        }
        match (e.state, &e.visual) {
            (State::Visual, Some(v)) => check_selection(&e.text, v, "expected selection")?,
            (State::Visual, None) => return Err("visual state without a selection".into()),
            (_, Some(_)) => return Err("selection outside visual state".into()),
            (_, None) => {}
        }
        for (name, r) in i.registers.iter().flatten() {
            check_register(name, r, "input")?;
        }
        if !e.registers.contains_key("\"") {
            return Err("expected registers lack the unnamed register".into());
        }
        for (name, r) in &e.registers {
            if let Some(r) = r {
                check_register(name, r, "expected")?;
            }
        }
        if let Some(last) = e.clipboard.writes.last() {
            if e.clipboard.text.as_deref() != Some(last.as_str()) {
                return Err("clipboard text isn't the last write".into());
            }
        }
        for err in e.errors.iter().flatten() {
            if err.after_keys > self.key_tokens.len() {
                return Err(format!("error after {} keys, but there are {}", err.after_keys, self.key_tokens.len()));
            }
        }
        if (run == Run::Tty) != i.window.is_some() || i.window.is_some() != e.window.is_some() {
            return Err("window must be given exactly for tty cases".into());
        }
        if let Some(layout) = &i.layout {
            if layout.first() != Some(&0) || layout.windows(2).any(|w| w[0] >= w[1]) {
                return Err(format!("layout {layout:?} must start at 0 and increase"));
            }
            for &b in layout {
                pos_of_byte(&i.text, b).map_err(|e| format!("layout: {e}"))?;
            }
        }
        Ok(())
    }
}

impl Fixture {
    /// Check the header and every case; ids must be unique in the file.
    pub fn validate(&self, file_stem: &str) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema {} (want {SCHEMA})", self.schema));
        }
        if self.topic != file_stem {
            return Err(format!("topic {:?} in file {file_stem}.json", self.topic));
        }
        if self.cases.is_empty() {
            return Err("no cases".into());
        }
        let mut ids = HashSet::new();
        for c in &self.cases {
            if !ids.insert(c.id.as_str()) {
                return Err(format!("duplicate id {}", c.id));
            }
            c.validate(self.run).map_err(|e| format!("{}/{}: {e}", self.topic, c.id))?;
        }
        Ok(())
    }
}
