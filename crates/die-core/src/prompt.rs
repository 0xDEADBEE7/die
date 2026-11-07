//! The command line (contract §12, K11a): one line of text with a cursor, Emacs's and Vim's line
//! editing keys, and a history per name. It's what `:`, `/` and `ctx.prompt` type into.
//!
//! The keys, which a prompt takes before any keymap sees them:
//! - Answer: `<CR>`. Cancel: `<Esc>`, `<C-g>` and `<C-c>`.
//! - Move: `<Left>` `<C-b>`, `<Right>` `<C-f>`, `<Home>` `<C-a>`, `<End>` `<C-e>`, and by word
//!   `<M-b>` `<M-Left>` `<C-Left>` `<S-Left>`, `<M-f>` `<M-Right>` `<C-Right>` `<S-Right>`.
//! - Delete: `<BS>` `<C-h>`, `<Del>` `<C-d>`, a word with `<C-w>` `<M-BS>` and `<M-d>`, and to
//!   either end of the line with `<C-u>` and `<C-k>`.
//! - History: `<Up>` `<C-p>` `<PageUp>` for older, `<Down>` `<C-n>` `<PageDown>` for newer, when
//!   there are no candidates. Going past the newest brings back what you were typing.
//! - Paste at the cursor: `<D-v>` the clipboard, without the newlines it ends in, `<C-y>` the
//!   newest kill, and `<C-r>` the register the next key names ([`Ctx::set_prompt_register`]).
//! - A key with super goes to the keymaps first, so the DIE layer works with a prompt open.
//!
//! [`PromptSpec::evil_keys`] changes `<Up>`, `<Down>`, `<C-u>` and `<C-b>` to Evil's, for `:` and
//! `/`, and `<C-v>` inserts the next key as it is. The other Emacs keys stay on those too
//! (scope §10).
//!
//! A prompt with candidates shows up to 20 of them under its line, flex matched against what's
//! typed, and some keys change:
//! - `<CR>` answers with the chosen candidate, or the typed text if none matches. `<M-CR>` answers
//!   with the typed text whatever matches. [`PromptSpec::require_match`] holds both to a candidate.
//! - `<Tab>` puts the chosen candidate on the line, or the first if none is chosen.
//! - `<Down>` `<C-n>` and `<Up>` `<C-p>` choose the next and last row, stopping at the ends,
//!   `<PageDown>` `<C-v>` and `<PageUp>` `<M-v>` go a screenful, and `<M-<>` and `<M->>` go to the
//!   first and the last.
//! - With [`PromptSpec::descends`], a candidate ending in `/` goes on the line instead of
//!   answering. While nothing's typed after the directory no row is chosen, shown as `*/N`, so
//!   `<CR>` answers with the line. `<Down>` chooses the first row, and `<Up>` from it the line
//!   again, as vertico's `directory` preselect has it.
//! - History is on `<M-p>` and `<M-n>`, which also work with no candidates.
//!
//! The cursor moves by what the scene draws as one glyph, a grapheme cluster. A word is a run of
//! alphanumerics and `_`, or a run of other non-blanks, as in Vim, so `<C-w>` on `a/b` takes `b`.
#![warn(missing_docs)]
mod draw;
mod flex;
pub use flex::flex_rank;

use crate::error::*;
use crate::ext::*;
use crate::keymap::*;
use crate::keys::*;
use crate::rt::*;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;

type PromptFn = Box<dyn FnOnce(&mut Ctx, Option<String>) -> Result>;
type ChangeFn = Rc<RefCell<dyn FnMut(&mut Ctx, &str) -> Result>>;
type LiveFn = Rc<RefCell<dyn FnMut(&str) -> (usize, Vec<String>)>>;

/// How many answers a history keeps.
pub(crate) const HISTORY: usize = 100;
/// How many candidates show at once, as the owner's `vertico-count`.
const ROWS: usize = 20;

/// What a prompt shows and remembers. A plain `&str` is a label and nothing else.
pub struct PromptSpec {
    label: String,
    initial: String,
    history: Option<String>,
    backspace_cancels: bool,
    evil_keys: bool,
    on_change: Option<ChangeFn>,
    source: Option<Source>,
    descends: bool,
    require_match: bool,
}

enum Source {
    Fixed(Vec<String>),
    Live(LiveFn),
}

impl PromptSpec {
    /// A prompt with this label.
    pub fn new(label: &str) -> Self {
        PromptSpec {
            label: label.into(),
            initial: String::new(),
            history: None,
            backspace_cancels: false,
            evil_keys: false,
            on_change: None,
            source: None,
            descends: false,
            require_match: false,
        }
    }
    /// Start with this text, the cursor at its end.
    pub fn initial(mut self, text: &str) -> Self {
        self.initial = text.into();
        self
    }
    /// Remember answers under this name, and offer them on `<Up>`. Prompts that share a name
    /// share a history. A cancel, an empty answer and a repeat of the newest add nothing.
    pub fn history(mut self, name: &str) -> Self {
        self.history = Some(name.into());
        self
    }
    /// `<BS>` on an empty line cancels, as on Vim's command line.
    pub fn backspace_cancels(mut self) -> Self {
        self.backspace_cancels = true;
        self
    }
    /// The keys of Evil's command line where they differ from Emacs's: `<Up>` and `<Down>` take
    /// only entries that start with the text before the cursor, and the cursor stays. `<C-u>`
    /// deletes the whole line and `<C-b>` goes to its start.
    pub fn evil_keys(mut self) -> Self {
        self.evil_keys = true;
        self
    }
    /// Run `f` with the text each time a key changes it, for incremental search. Each call is
    /// queued (C3), in order with the answer, and isn't part of the key stream: an error from it
    /// is reported and drops nothing.
    pub fn on_change(mut self, f: impl FnMut(&mut Ctx, &str) -> Result + 'static) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(f)));
        self
    }
    /// Offer these, flex matched against the text. With nothing typed they show in this order, so
    /// put the likeliest first.
    pub fn candidates<S: Into<String>>(mut self, items: impl IntoIterator<Item = S>) -> Self {
        let items = items.into_iter().map(Into::into).collect();
        self.source = Some(Source::Fixed(items));
        self
    }
    /// Work out the candidates from the text, each time a key changes it. `f` returns how many
    /// bytes at the start of the text aren't part of the match, and the candidates for the rest:
    /// for `src/ma`, `(4, entries_of("src/"))`, and choosing `main.rs` makes the line
    /// `src/main.rs`. It runs on the main thread with each change, so keep what's slow to get.
    pub fn candidates_from(
        mut self,
        f: impl FnMut(&str) -> (usize, Vec<String>) + 'static,
    ) -> Self {
        self.source = Some(Source::Live(Rc::new(RefCell::new(f))));
        self
    }
    /// `<CR>` and `<M-CR>` answer only with a candidate or an empty line. Anything else stays on
    /// the line, with a note until the next key, as a prompt that requires a match does in Vertico.
    pub fn require_match(mut self) -> Self {
        self.require_match = true;
        self
    }
    /// A candidate ending in `/` goes on the line instead of answering, as a directory does in
    /// `find-file`. With nothing typed after the directory no row is chosen, and `<CR>` answers
    /// with the directory: what that means is up to the prompt's owner.
    pub fn descends(mut self) -> Self {
        self.descends = true;
        self
    }
}

impl From<&str> for PromptSpec {
    fn from(label: &str) -> Self {
        PromptSpec::new(label)
    }
}

/// `items` in the order the owner's `vertico-sort-history-length-alpha` puts them: those answered
/// under this history (oldest first, as [`Ctx::prompt_history`] gives it), newest first, then the
/// shortest, then alphabetically. For [`PromptSpec::candidates`], which keeps the order it's given.
pub fn recent_first(history: &[String], mut items: Vec<String>) -> Vec<String> {
    items.sort_by_cached_key(|item| {
        let age = history.iter().rev().position(|h| h == item);
        (
            age.unwrap_or(usize::MAX),
            item.chars().count(),
            item.clone(),
        )
    });
    items
}

/// A prompt's candidates, and which of them match the text now.
struct List {
    live: Option<LiveFn>,
    items: Vec<String>,
    /// How many bytes of the text aren't matched against `items`.
    base: usize,
    /// Indices into `items`, best first.
    matches: Vec<usize>,
    /// An index into `matches`. None means the line itself, or that nothing matches.
    chosen: Option<usize>,
    /// The line can be chosen, above the first row.
    line_row: bool,
    /// The first of `matches` showing.
    top: usize,
    /// The rows taken so far. Like the owner's minibuffer, the list grows and doesn't shrink.
    height: usize,
}

impl List {
    fn new(source: Source, line_row: bool) -> List {
        let (live, items) = match source {
            Source::Fixed(items) => (None, items),
            Source::Live(f) => (Some(f), vec![]),
        };
        List {
            live,
            items,
            base: 0,
            matches: vec![],
            chosen: None,
            line_row,
            top: 0,
            height: 0,
        }
    }
    fn query<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.base..).unwrap_or(text)
    }
    fn filter(&mut self, text: &str) {
        let query = self.query(text);
        self.matches = flex::rank(query, self.items.iter().map(String::as_str));
        let on_line = self.line_row && query.is_empty();
        self.chosen = (!self.matches.is_empty() && !on_line).then_some(0);
        self.top = 0;
        self.height = self.height.max(self.matches.len().min(ROWS));
    }
    fn chosen_item(&self) -> Option<&str> {
        self.item_at(self.chosen?)
    }
    fn item_at(&self, at: usize) -> Option<&str> {
        Some(&self.items[*self.matches.get(at)?])
    }
    /// The chosen row's place, with the line at -1.
    fn at(&self) -> isize {
        self.chosen.map_or(-1, |c| c as isize)
    }
    /// Choose this row, or the nearest there is.
    fn goto(&mut self, at: isize) {
        let first = if self.line_row || self.matches.is_empty() {
            -1
        } else {
            0
        };
        let last = self.matches.len() as isize - 1;
        self.chosen = usize::try_from(at.min(last).max(first)).ok();
    }
    /// The first row, and from there the line, as `vertico-first` does.
    fn first(&mut self) {
        self.goto(if self.at() > 0 { 0 } else { -1 });
    }
}

/// What the next key means, after a key that reads one.
enum Reading {
    /// The name of a register to paste, after `<C-r>`.
    Register,
    /// A character to insert, after `<C-v>`.
    Literal,
}

/// The character a key stands for when it's read as one, as Emacs's `read-char` has it: `<C-w>`
/// is `^W`. None for a key with no character, such as `<Up>`.
fn key_char(ev: &KeyEvent) -> Option<char> {
    let c = ev.chord();
    match c.key {
        _ if ev.printable().is_some() => ev.printable()?.chars().next(),
        Key::Esc => Some('\x1b'),
        Key::Enter => Some('\r'),
        Key::Tab => Some('\t'),
        Key::Backspace => Some('\x7f'),
        Key::Char(k) if c.mods.ctrl && k.is_ascii_alphabetic() => {
            Some(char::from(k.to_ascii_lowercase() as u8 - b'`'))
        }
        _ => None,
    }
}

pub(crate) struct Prompt {
    spec: PromptSpec,
    text: String,
    /// A byte offset into `text`, on a grapheme boundary.
    cursor: usize,
    /// Which history entry `text` came from, counting back from the newest.
    recalled: Option<usize>,
    /// What was typed before going back in the history.
    draft: String,
    list: Option<List>,
    /// `<CR>` was refused, until the next key.
    refused: bool,
    reading: Option<Reading>,
    ext: ExtIdx,
    prev: State,
    f: PromptFn,
}

/// Which kind of word a grapheme belongs to: true for alphanumerics. None for a blank.
fn word_class(g: &str) -> Option<bool> {
    let c = g.chars().next()?;
    (!c.is_whitespace()).then_some(c.is_alphanumeric() || c == '_')
}

/// How many bytes of `gs` make up blanks and then one word.
fn word_len<'a>(gs: impl Iterator<Item = &'a str>) -> usize {
    let mut gs = gs.peekable();
    let mut len = 0;
    while let Some(g) = gs.next_if(|g| word_class(g).is_none()) {
        len += g.len();
    }
    let class = gs.peek().and_then(|g| word_class(g));
    while let Some(g) = gs.next_if(|g| word_class(g) == class) {
        len += g.len();
    }
    len
}

impl Prompt {
    fn before(&self) -> usize {
        let last = self.text[..self.cursor].graphemes(true).next_back();
        self.cursor - last.map_or(0, str::len)
    }
    fn after(&self) -> usize {
        let next = self.text[self.cursor..].graphemes(true).next();
        self.cursor + next.map_or(0, str::len)
    }
    fn word_before(&self) -> usize {
        self.cursor - word_len(self.text[..self.cursor].graphemes(true).rev())
    }
    fn word_after(&self) -> usize {
        self.cursor + word_len(self.text[self.cursor..].graphemes(true))
    }
    fn replace(&mut self, r: Range<usize>, s: &str) {
        self.cursor = r.start + s.len();
        self.text.replace_range(r, s);
    }
    fn set_line(&mut self, s: String) {
        self.cursor = s.len();
        self.text = s;
    }
    /// The line choosing the chosen candidate would make, and whether it ends in `/`.
    fn chosen_line(&self) -> Option<(String, bool)> {
        let l = self.list.as_ref()?;
        self.line_with(l.chosen_item()?)
    }
    /// What `<Tab>` puts on the line: the chosen candidate, or the first.
    fn tab_line(&self) -> Option<(String, bool)> {
        let l = self.list.as_ref()?;
        self.line_with(l.item_at(l.chosen.unwrap_or(0))?)
    }
    fn line_with(&self, item: &str) -> Option<(String, bool)> {
        let fixed = self
            .text
            .get(..self.list.as_ref()?.base)
            .unwrap_or_default();
        Some((format!("{fixed}{item}"), item.ends_with('/')))
    }
    /// Whether [`PromptSpec::require_match`] refuses the line as it is.
    fn refuses(&self) -> bool {
        let Some(l) = self.list.as_ref().filter(|_| self.spec.require_match) else {
            return false;
        };
        let query = l.query(&self.text);
        !self.text.is_empty() && !l.items.iter().any(|i| i == query)
    }
    fn step(&mut self, by: isize) {
        if let Some(l) = self.list.as_mut() {
            l.goto(l.at().saturating_add(by));
        }
    }
    /// A screenful of rows, for paging.
    fn page(&self) -> isize {
        self.list.as_ref().map_or(0, |l| l.height.max(1) as isize)
    }
    /// Go older or newer in `past`, which is oldest first. Past the oldest, nothing happens. With
    /// `matching`, only entries that start with the text before the cursor count, on a grapheme
    /// boundary, and the cursor stays.
    fn recall(&mut self, past: &[String], older: bool, matching: bool) {
        let at = self.cursor;
        let back = |i: usize| past[past.len() - 1 - i].as_str();
        let fits = |i: &usize| {
            let e = back(*i);
            !matching
                || e.starts_with(&self.text[..at])
                    && (e.len() == at || e.grapheme_indices(true).any(|(g, _)| g == at))
        };
        let found = match (self.recalled, older) {
            (None, false) => return,
            (from, true) => (from.map_or(0, |i| i + 1)..past.len()).find(fits),
            (Some(from), false) => (0..from).rev().find(fits),
        };
        let Some(found) = found else {
            if !older {
                self.recalled = None;
                let draft = std::mem::take(&mut self.draft);
                self.set_line(draft);
            }
            return;
        };
        if self.recalled.is_none() {
            self.draft = self.text.clone();
        }
        self.recalled = Some(found);
        self.text = back(found).to_string();
        if !matching {
            self.cursor = self.text.len();
        }
    }
}

impl Ctx {
    /// A command-line prompt. `f` runs from the top of the loop with the answer, or None on
    /// cancel. A label will do for `spec`. One prompt is open at a time, and a new one cancels
    /// the last.
    pub fn prompt(
        &mut self,
        spec: impl Into<PromptSpec>,
        f: impl FnOnce(&mut Ctx, Option<String>) -> Result + 'static,
    ) {
        if self.prompt.is_some() {
            self.finish_prompt(None, false);
        }
        let prev = self.state();
        self.set_state(State::CommandLine);
        let mut spec: PromptSpec = spec.into();
        let list = spec.source.take().map(|s| List::new(s, spec.descends));
        let text = spec.initial.clone();
        self.prompt = Some(Prompt {
            cursor: text.len(),
            text,
            spec,
            recalled: None,
            draft: String::new(),
            list,
            refused: false,
            reading: None,
            ext: self.cur_ext(),
            prev,
            f: Box::new(f),
        });
        self.prompt_refilter();
    }
    /// The text typed into the open prompt, if there is one.
    pub fn prompt_text(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.text.as_str())
    }
    /// The candidates that match the open prompt's text, best first.
    pub fn prompt_candidates(&self) -> Vec<&str> {
        let list = self.prompt.as_ref().and_then(|p| p.list.as_ref());
        list.map_or(vec![], |l| {
            l.matches.iter().map(|&i| l.items[i].as_str()).collect()
        })
    }
    /// The chosen candidate of the open prompt, if any matches.
    pub fn prompt_chosen(&self) -> Option<&str> {
        self.prompt.as_ref()?.list.as_ref()?.chosen_item()
    }
    /// The rows the open prompt's list has taken.
    pub(crate) fn prompt_list_height(&self) -> usize {
        let list = self.prompt.as_ref().and_then(|p| p.list.as_ref());
        list.map_or(0, |l| l.height)
    }
    /// What the prompt's line starts with: how far through the candidates you are, as vertico
    /// shows it, and the label.
    pub(crate) fn prompt_lead(&self) -> Option<String> {
        let p = self.prompt.as_ref()?;
        let label = &p.spec.label;
        Some(match &p.list {
            Some(l) => {
                let total = l.matches.len();
                let at = l.chosen.map_or("*".to_string(), |c| (c + 1).to_string());
                format!("{:<6} {label}", format!("{at}/{total}"))
            }
            None => label.clone(),
        })
    }
    /// What shows after the prompt's text, for now.
    pub(crate) fn prompt_note(&self) -> &'static str {
        let Some(p) = self.prompt.as_ref() else {
            return "";
        };
        match p.reading {
            Some(Reading::Register) => "\"",
            Some(Reading::Literal) => "^",
            None if p.refused => " [Match required]",
            None => "",
        }
    }
    /// The list's rows, `room` at most, scrolled to the chosen one: each candidate, the characters
    /// of it that matched, and whether it's chosen.
    pub(crate) fn prompt_rows(&mut self, room: usize) -> Vec<(String, Vec<usize>, bool)> {
        let Some(p) = self.prompt.as_mut() else {
            return vec![];
        };
        let Some(l) = p.list.as_mut() else {
            return vec![];
        };
        if room == 0 {
            return vec![];
        }
        if let Some(chosen) = l.chosen {
            l.top = l.top.min(chosen).max((chosen + 1).saturating_sub(room));
        }
        let query = l.query(&p.text);
        let shown = l.matches.iter().enumerate().skip(l.top).take(room);
        shown
            .map(|(at, &i)| {
                let item = &l.items[i];
                (
                    item.clone(),
                    flex::positions(query, item),
                    Some(at) == l.chosen,
                )
            })
            .collect()
    }
    /// The text before the prompt's cursor.
    pub(crate) fn prompt_before_cursor(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| &p.text[..p.cursor])
    }
    /// The answers remembered under a history's name, oldest first.
    pub fn prompt_history(&self, name: &str) -> &[String] {
        self.histories.get(name).map_or(&[], Vec::as_slice)
    }
    /// How a prompt reads a register, for `<C-r>` and `<C-y>`: `rule` takes the register's name,
    /// a control character standing for its key, and gives the text, or an error to report with
    /// the prompt staying open. Which registers there are is the keymap's business, as in
    /// [`Ctx::set_cursor_rest`]. With no rule, they're the ones [`Ctx::register`] holds.
    pub fn set_prompt_register(&mut self, rule: fn(&mut Ctx, char) -> Result<String>) {
        self.prompt_register = Some(rule);
    }
    /// Put this at the prompt's cursor.
    fn prompt_insert(&mut self, s: &str) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        let before = p.text.clone();
        p.replace(p.cursor..p.cursor, s);
        self.prompt_changed(&before);
    }
    /// Paste this register at the prompt's cursor. A register that can't be read is an error for
    /// the key, as in your Evil, and the prompt stays open.
    fn prompt_paste(&mut self, name: Option<char>) {
        let Some(ext) = self.prompt.as_ref().map(|p| p.ext) else {
            return;
        };
        let rule = self.prompt_register;
        let read = name
            .ok_or_else(|| Error::Msg("Non-character input-event".into()))
            .and_then(|c| match rule {
                Some(rule) => self.guarded(ext, |ctx| rule(ctx, c)),
                None => self.register(c).map(|r| r.text.clone()).ok_or_else(|| {
                    let shown = c.escape_debug();
                    Error::Msg(format!("Register {shown} is empty"))
                }),
            });
        match read {
            Ok(text) => self.prompt_insert(&text),
            Err(Error::Panicked { .. }) => self.drop_fed(),
            Err(e) => {
                self.drop_fed();
                self.log_error(e.to_string());
            }
        }
    }
    /// Queue the answer. `keyed` says a key submitted it, so it's part of the key stream (contract
    /// §6): if it fails, the rest of what was fed drops.
    fn finish_prompt(&mut self, answer: Option<String>, keyed: bool) {
        let Some(p) = self.prompt.take() else { return };
        self.set_state(p.prev);
        if let (Some(name), Some(text)) = (&p.spec.history, &answer) {
            let past = self.histories.entry(name.clone()).or_default();
            if !text.is_empty() && past.last() != Some(text) {
                past.push(text.clone());
                if past.len() > HISTORY {
                    past.remove(0);
                }
            }
        }
        let f = p.f;
        self.queue.push_back(Queued {
            ext: p.ext,
            alive: None,
            keyed,
            f: Box::new(move |ctx| f(ctx, answer)),
        });
    }

    pub(crate) fn prompt_key(&mut self, ev: KeyEvent) {
        let c = ev.chord();
        let (ctrl, alt, shift) = (c.mods.ctrl, c.mods.alt, c.mods.shift);
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        match p.reading.take() {
            Some(Reading::Register) => return self.prompt_paste(key_char(&ev)),
            Some(Reading::Literal) => {
                if let Some(k) = key_char(&ev) {
                    self.prompt_insert(k.encode_utf8(&mut [0; 4]));
                }
                return;
            }
            None => {}
        }
        let (at, end) = (p.cursor, p.text.len());
        let evil = p.spec.evil_keys;
        let before = p.text.clone();
        let listed = p.list.is_some();
        p.refused = false;
        match c.key {
            Key::Enter if (alt || p.chosen_line().is_none()) && p.refuses() => p.refused = true,
            Key::Enter if alt => return self.finish_prompt(Some(before), true),
            Key::Enter => match p.chosen_line() {
                Some((line, true)) if p.spec.descends => p.set_line(line),
                Some((line, _)) => return self.finish_prompt(Some(line), true),
                None => return self.finish_prompt(Some(before), true),
            },
            Key::Tab => {
                if let Some((line, _)) = p.tab_line() {
                    p.set_line(line);
                }
            }
            Key::Esc => return self.finish_prompt(None, true),
            Key::Char('v') if c.mods.super_ => {
                if let Some(r) = self.clipboard() {
                    self.prompt_insert(r.text.trim_end_matches('\n'));
                }
                return;
            }
            Key::Char('g' | 'c') if ctrl => return self.finish_prompt(None, true),
            Key::Backspace if end == 0 && p.spec.backspace_cancels => {
                return self.finish_prompt(None, true);
            }
            Key::Left if ctrl || alt || shift => p.cursor = p.word_before(),
            Key::Right if ctrl || alt || shift => p.cursor = p.word_after(),
            Key::Left => p.cursor = p.before(),
            Key::Right => p.cursor = p.after(),
            Key::Home => p.cursor = 0,
            Key::End => p.cursor = end,
            Key::Backspace if alt => p.replace(p.word_before()..at, ""),
            Key::Backspace => p.replace(p.before()..at, ""),
            Key::Delete => p.replace(at..p.after(), ""),
            Key::Up if listed => p.step(-1),
            Key::Down if listed => p.step(1),
            Key::PageUp if listed => p.step(-p.page()),
            Key::PageDown if listed => p.step(p.page()),
            Key::Up => self.prompt_recall(true, evil),
            Key::Down => self.prompt_recall(false, evil),
            Key::PageUp => self.prompt_recall(true, false),
            Key::PageDown => self.prompt_recall(false, false),
            Key::Char(k) if ctrl => match k {
                'b' if evil => p.cursor = 0,
                'b' => p.cursor = p.before(),
                'f' => p.cursor = p.after(),
                'a' => p.cursor = 0,
                'e' => p.cursor = end,
                'h' => p.replace(p.before()..at, ""),
                'd' => p.replace(at..p.after(), ""),
                'w' => p.replace(p.word_before()..at, ""),
                'u' if evil => p.replace(0..end, ""),
                'u' => p.replace(0..at, ""),
                'k' => p.replace(at..end, ""),
                'p' if listed => p.step(-1),
                'n' if listed => p.step(1),
                'r' => p.reading = Some(Reading::Register),
                'y' => return self.prompt_paste(Some('"')),
                'v' if evil => p.reading = Some(Reading::Literal),
                'v' => p.step(p.page()),
                'p' => self.prompt_recall(true, false),
                'n' => self.prompt_recall(false, false),
                _ => {}
            },
            Key::Char(k) if alt => match k {
                'b' => p.cursor = p.word_before(),
                'f' => p.cursor = p.word_after(),
                'd' => p.replace(at..p.word_after(), ""),
                'v' => p.step(-p.page()),
                '<' => {
                    if let Some(l) = p.list.as_mut() {
                        l.first();
                    }
                }
                '>' => p.step(isize::MAX),
                'p' => self.prompt_recall(true, false),
                'n' => self.prompt_recall(false, false),
                _ => {}
            },
            _ => {
                if let Some(t) = ev.printable() {
                    p.replace(at..at, &t);
                }
            }
        }
        self.prompt_changed(&before);
    }

    fn prompt_recall(&mut self, older: bool, matching: bool) {
        let Some(mut p) = self.prompt.take() else {
            return;
        };
        let name = p.spec.history.as_deref().unwrap_or_default();
        p.recall(self.prompt_history(name), older, matching);
        self.prompt = Some(p);
    }

    /// Match the candidates against the text, asking for them again if they depend on it. The
    /// asking is extension code, so it's guarded, and if it panics the list is empty.
    fn prompt_refilter(&mut self) {
        let Some(p) = self.prompt.as_ref() else {
            return;
        };
        let Some(l) = p.list.as_ref() else {
            return;
        };
        let text = p.text.clone();
        let (ext, live) = (p.ext, l.live.clone());
        let asked = live.map(|f| self.guarded(ext, |_| Ok((f.borrow_mut())(&text))));
        let Some(l) = self.prompt.as_mut().and_then(|p| p.list.as_mut()) else {
            return;
        };
        match asked {
            Some(Ok((base, items))) => (l.base, l.items) = (base, items),
            Some(Err(_)) => (l.base, l.items) = (0, vec![]),
            None => {}
        }
        l.filter(&text);
    }

    /// Match the candidates again and queue the `on_change` call, if the key changed the text.
    fn prompt_changed(&mut self, before: &str) {
        if self.prompt.as_ref().is_none_or(|p| p.text == before) {
            return;
        }
        self.prompt_refilter();
        let Some(p) = self.prompt.as_ref() else {
            return;
        };
        let Some(f) = p.spec.on_change.clone() else {
            return;
        };
        let (ext, text) = (p.ext, p.text.clone());
        self.queue.push_back(Queued {
            ext,
            alive: None,
            keyed: false,
            f: Box::new(move |ctx| (f.borrow_mut())(ctx, &text)),
        });
    }
}
