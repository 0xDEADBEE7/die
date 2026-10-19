//! The Evil oracle's fixtures: their schema, their guards, and which die-vim
//! work package each case lands with (built in spike S5, moved here in V0).
//!
//! The oracle (`oracle/evil-oracle.el`, run by `run-oracle.sh`) runs each
//! case's keys through the user's Evil and writes one JSON file per topic
//! to `fixtures/`. These types are the schema die-vim's oracle test reads;
//! `validate` checks that a fixture is internally consistent. Running the
//! oracle needs Emacs; reading the committed fixtures doesn't.
//!
//! Positions are 0-based: `line` counts `\n`s before the position, `col`
//! counts characters (Unicode scalar values) from the start of its line,
//! and `byte` is the UTF-8 offset from the start of the text.

use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

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
    /// `keys` split into single keys as Evil read them (`evil-oracle--parse-keys`): a
    /// character, or `<Name>`. `validate` checks that die-core reads them alike.
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

/// Check that die-core reads `keys` as Evil did: one key per token, through the notation
/// parser die-vim, the harness and keymaps use. A name Evil's side doesn't know, which
/// it typed as literal characters, fails here until `evil-oracle.el` learns it.
fn check_keys(keys: &str, tokens: &[String]) -> Result<(), String> {
    let ours = die_core::parse_keys(keys)?;
    let mut evil = Vec::new();
    for t in tokens {
        match die_core::parse_keys(t)?.as_slice() {
            [ev] => evil.push(ev.clone()),
            evs => return Err(format!("key token {t:?} is {} keys", evs.len())),
        }
    }
    if ours != evil {
        return Err(format!(
            "die-core reads {keys:?} as {} keys, not as key_tokens {tokens:?}",
            ours.len()
        ));
    }
    Ok(())
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
    Ok(Pos {
        line,
        col: text[line_start..byte].chars().count(),
        byte,
    })
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
        || (name.len() == 1
            && name
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase() || c == '-'));
    if !ok_name {
        return Err(format!("{what}: bad register name {name:?}"));
    }
    if r.kind == RegisterKind::Line && !r.text.ends_with('\n') {
        return Err(format!(
            "{what}: linewise register {name} doesn't end in a newline"
        ));
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
        check_keys(&self.keys, &self.key_tokens)?;
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
        if let Some(last) = e.clipboard.writes.last()
            && e.clipboard.text.as_deref() != Some(last.as_str())
        {
            return Err("clipboard text isn't the last write".into());
        }
        for err in e.errors.iter().flatten() {
            if err.after_keys > self.key_tokens.len() {
                return Err(format!(
                    "error after {} keys, but there are {}",
                    err.after_keys,
                    self.key_tokens.len()
                ));
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
            c.validate(self.run)
                .map_err(|e| format!("{}/{}: {e}", self.topic, c.id))?;
        }
        Ok(())
    }
}

/// The committed fixtures, one file per topic.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Every fixture in `dir`, sorted by topic, each validated.
pub fn load_fixtures(dir: &Path) -> Result<Vec<Fixture>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let fx: Fixture =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        let stem = p.file_stem().unwrap_or_default().to_string_lossy();
        fx.validate(&stem)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        out.push(fx);
    }
    Ok(out)
}

// ------------------------------------------------------------------ tranches

/// The die-vim work package a case lands with (docs/m0-plan.md §5): the
/// highest-numbered package among the features its keys and registers use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tranche {
    /// Motions, counts, insert sessions and their undo.
    V1,
    /// Search, and moving between files (`:e`, `:b`, `:N`).
    V2,
    /// Operators, registers and the clipboard, text objects.
    V3,
    /// Visual mode, `.`, macros, `R`.
    V4,
    /// The rest of ex: `:s`, `:w`, `:q` and friends.
    V5,
    /// Geometry and windows: every case that needs a tty Emacs.
    V6,
}

impl Tranche {
    pub const ALL: [Tranche; 6] = [
        Tranche::V1,
        Tranche::V2,
        Tranche::V3,
        Tranche::V4,
        Tranche::V5,
        Tranche::V6,
    ];

    pub fn name(self) -> &'static str {
        ["V1", "V2", "V3", "V4", "V5", "V6"][self as usize]
    }

    pub fn parse(s: &str) -> Option<Tranche> {
        Tranche::ALL.into_iter().find(|t| t.name() == s)
    }
}

/// Keys that take one more key as their argument.
const ARG_KEYS: &[&str] = &["f", "F", "t", "T", "r", "m", "'", "`"];
/// Two-key prefixes outside operators.
const PREFIXES: &[&str] = &["g", "z", "Z", "[", "]", "<C-w>"];

/// Which work package `case` lands with. `run` is its fixture's run.
pub fn tranche(case: &Case, run: Run) -> Tranche {
    let mut t = Tranche::V1;
    let mut need = |x: Tranche| t = t.max(x);
    if run == Run::Tty {
        need(Tranche::V6);
    }
    let i = &case.input;
    if i.visual.is_some() {
        need(Tranche::V4);
    }
    if i.clipboard.is_some() || case.expected.clipboard.text.is_some() {
        need(Tranche::V3);
    }
    for r in i.registers.iter().flat_map(|m| m.values()) {
        need(if r.kind == RegisterKind::Macro {
            Tranche::V4
        } else {
            Tranche::V3
        });
    }
    if case.expected.registers.values().any(|r| r.is_some()) {
        need(Tranche::V3);
    }
    for f in key_features(&case.key_tokens) {
        need(f);
    }
    t
}

/// The packages each key in `keys` needs, walking normal, insert and replace
/// state the way Vim reads them. Approximate where Vim's grammar is ambiguous
/// without running it, but never below what a key needs.
fn key_features(keys: &[String]) -> Vec<Tranche> {
    #[derive(PartialEq)]
    enum St {
        Normal,
        Insert,
        // After an operator: count, then a motion, a text object or the operator again.
        Op(String),
    }
    let mut out = Vec::new();
    let mut st = St::Normal;
    let mut recording = false;
    let mut i = 0;
    let at = |i: usize| keys.get(i).map(String::as_str).unwrap_or("");
    while i < keys.len() {
        let k = at(i);
        match &st {
            St::Insert => {
                match k {
                    "<Esc>" => st = St::Normal,
                    // Insert a register's text.
                    "<C-r>" => {
                        out.push(Tranche::V3);
                        i += 1;
                    }
                    _ => {}
                }
                i += 1;
                continue;
            }
            St::Op(op) => {
                let op = op.clone();
                let insert_after = op == "c";
                let mut done = true;
                match k {
                    _ if k.len() == 1 && k.chars().all(|c| c.is_ascii_digit()) && k != "0" => {
                        done = false
                    }
                    "i" | "a" => i += 1,
                    _ if ARG_KEYS.contains(&k) => i += 1,
                    "/" | "?" => {
                        out.push(Tranche::V2);
                        i = skip_line(keys, i);
                    }
                    "g" => i += 1,
                    _ => {}
                }
                i += 1;
                if done {
                    st = if insert_after { St::Insert } else { St::Normal };
                }
                continue;
            }
            St::Normal => {}
        }
        match k {
            "/" | "?" => {
                out.push(Tranche::V2);
                i = skip_line(keys, i);
            }
            ":" => {
                let end = skip_line(keys, i);
                let cmd: String = keys[i + 1..end.min(keys.len())]
                    .iter()
                    .filter(|t| t.chars().count() == 1)
                    .map(String::as_str)
                    .collect();
                let word: String = cmd
                    .chars()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .collect();
                let goto = !cmd.is_empty() && cmd.chars().all(|c| c.is_ascii_digit());
                out.push(
                    if goto || matches!(word.as_str(), "e" | "edit" | "b" | "buffer") {
                        Tranche::V2
                    } else {
                        Tranche::V5
                    },
                );
                i = end;
            }
            "n" | "N" | "*" | "#" => out.push(Tranche::V2),
            "\"" => {
                out.push(Tranche::V3);
                i += 1;
            }
            "q" => {
                out.push(Tranche::V4);
                // `q` + register starts recording; `q` while recording stops it.
                if !recording {
                    i += 1;
                }
                recording = !recording;
            }
            "@" => {
                out.push(Tranche::V4);
                i += 1;
            }
            "v" | "V" | "<C-v>" | "." => out.push(Tranche::V4),
            "R" => {
                out.push(Tranche::V4);
                st = St::Insert;
            }
            "u" | "<C-r>" => out.push(Tranche::V1),
            "i" | "a" | "I" | "A" | "o" | "O" => st = St::Insert,
            "d" | "c" | "y" | ">" | "<" | "=" => {
                out.push(Tranche::V3);
                // `dd`, `cc`, `yy`, `>>`, `<<` act on the line at once.
                if at(i + 1) == k {
                    i += 1;
                    if k == "c" {
                        st = St::Insert;
                    }
                } else {
                    st = St::Op(k.to_string());
                }
            }
            "C" | "s" | "S" => {
                out.push(Tranche::V3);
                st = St::Insert;
            }
            "x" | "X" | "D" | "J" | "~" | "p" | "P" | "Y" => out.push(Tranche::V3),
            "r" => {
                out.push(Tranche::V3);
                i += 1;
            }
            "g" => {
                match at(i + 1) {
                    "u" | "U" | "~" | "?" => {
                        out.push(Tranche::V3);
                        st = St::Op(format!("g{}", at(i + 1)));
                    }
                    "J" | "p" | "P" => out.push(Tranche::V3),
                    "v" => out.push(Tranche::V4),
                    "i" => st = St::Insert,
                    _ => {}
                }
                i += 1;
            }
            "z" | "<C-w>" | "<C-d>" | "<C-u>" | "<C-f>" | "<C-b>" | "<C-e>" | "<C-y>" | "H"
            | "M" | "L" => {
                out.push(Tranche::V6);
                if PREFIXES.contains(&k) {
                    i += 1;
                }
            }
            _ if ARG_KEYS.contains(&k) => i += 1,
            _ if PREFIXES.contains(&k) => i += 1,
            _ => {}
        }
        i += 1;
    }
    out
}

/// The index of the `<CR>` or `<Esc>` that ends the command line opened at `i`.
fn skip_line(keys: &[String], i: usize) -> usize {
    let mut j = i + 1;
    while j < keys.len() && keys[j] != "<CR>" && keys[j] != "<Esc>" {
        j += 1;
    }
    j
}

// ------------------------------------------------------------------ deviations and pending

/// `deviations.json`: the cases die-vim deliberately doesn't match (scope §10,
/// spike S5 §4). Each is excluded from the Evil comparison and gets a
/// hand-written fixture instead.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deviations {
    pub schema: u32,
    pub rules: Vec<DeviationRule>,
}

/// One reason to deviate, and the cases it covers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviationRule {
    pub rule: String,
    /// The row of the S5 record's §4 table, when the rule comes from it.
    pub s5_row: Option<u32>,
    pub kind: DeviationKind,
    /// What your Evil does.
    pub evil: String,
    /// What die-vim does instead.
    pub die_vim: String,
    pub cases: Vec<DeviationCase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviationKind {
    /// A behaviour of how Emacs buffers end, not a choice anyone made.
    EmacsArtifact,
    EvilBug,
    /// Vim has it; your Evil doesn't.
    EvilGap,
    /// Undo is Vim's `u`/`C-r` (your S5 answer).
    UndoModel,
    /// DIE's regex syntax is Rust's (D4), so the keys change, not the result.
    RegexSyntax,
    /// Needs file-type modes (M1); until then die-vim indents like the line above.
    NeedsModes,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviationCase {
    /// `topic/id`.
    pub case: String,
    pub note: Option<String>,
    /// The hand-written fixture's file, once it's written (by V7 at the latest).
    pub fixture: Option<String>,
}

impl Deviations {
    pub fn parse(text: &str) -> Result<Deviations, String> {
        let d: Deviations = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if d.schema != 1 {
            return Err(format!("deviations schema {} (want 1)", d.schema));
        }
        let mut seen = HashSet::new();
        for r in &d.rules {
            if r.evil.is_empty() || r.die_vim.is_empty() || r.cases.is_empty() {
                return Err(format!("rule {} needs evil, die_vim and cases", r.rule));
            }
            for c in &r.cases {
                if !seen.insert(c.case.as_str()) {
                    return Err(format!("{} is listed twice", c.case));
                }
            }
        }
        Ok(d)
    }

    pub fn cases(&self) -> impl Iterator<Item = &DeviationCase> {
        self.rules.iter().flat_map(|r| &r.cases)
    }
}

/// Parse `pending.txt`: one `TRANCHE topic/id` per line; `#` starts a comment.
pub fn parse_pending(text: &str) -> Result<Vec<(Tranche, String)>, String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(t), Some(case), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("pending line {}: want `TRANCHE topic/id`", n + 1));
        };
        let t =
            Tranche::parse(t).ok_or_else(|| format!("pending line {}: no tranche {t}", n + 1))?;
        if !seen.insert(case.to_string()) {
            return Err(format!("pending line {}: {case} is listed twice", n + 1));
        }
        out.push((t, case.to_string()));
    }
    Ok(out)
}
