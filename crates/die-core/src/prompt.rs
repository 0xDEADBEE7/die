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
//!   again, as vertico's `directory` preselect has it. `<S-Tab>` goes up a directory.
//! - History is on `<M-p>` and `<M-n>`, which also work with no candidates.
//!
//! With [`PromptSpec::matches_part`], the candidates match only part of the line, as `:` matches
//! a command's name after its range. No row is chosen until you move to one, so `<CR>` answers
//! with the line as typed, and choosing a row or `<Tab>` puts it in that part and keeps the rest.
//! While the line has no such part, there's no list, and the keys are those of a prompt with none.
//! From the line, `<Up>` `<C-p>` `<PageUp>` go back in the history as they do with no list, and
//! once you're in it, the keys going forward do too, until another key changes the line.
//!
//! The cursor moves by what the scene draws as one glyph, a grapheme cluster. A word is a run of
//! alphanumerics and `_`, or a run of other non-blanks, as in Vim, so `<C-w>` on `a/b` takes `b`.
#![warn(missing_docs)]
mod draw;
pub(crate) mod flex;
pub use flex::flex_rank;

use crate::error::*;
use crate::ext::*;
use crate::keymap::*;
use crate::keys::*;
use crate::project::{Feed, Ranked};
use crate::rt::*;
use std::cell::RefCell;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;

type PromptFn = Box<dyn FnOnce(&mut Ctx, Option<String>) -> Result>;
type ChangeFn = Rc<RefCell<dyn FnMut(&mut Ctx, &str) -> Result>>;
type LiveFn = Rc<RefCell<dyn FnMut(&str) -> (usize, Vec<String>)>>;
type PartFn = Rc<dyn Fn(&str) -> Option<Range<usize>>>;
type TierFn = Rc<dyn Fn(&str, &str) -> usize>;

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
    part: Option<PartFn>,
    tiers: Option<TierFn>,
}

enum Source {
    Fixed(Vec<String>),
    Live(LiveFn),
    /// Another thread matches, and the list shows what it last said.
    Fed(Feed),
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
            part: None,
            tiers: None,
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
    /// Candidates and their matching come from a finder's threads.
    pub(crate) fn fed(mut self, feed: Feed) -> Self {
        self.source = Some(Source::Fed(feed));
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
    /// Match the candidates against only the part of the line `part` names, as a byte range: for
    /// `:%no`, `Some(1..3)`. None means the line has no such part, as once an argument follows a
    /// command's name, and then there's no list. No row is chosen until you move to one, so `<CR>`
    /// answers with the line as typed. A chosen row, and `<Tab>`, go in place of the part. The
    /// history keeps the keys that go back from the line, and all of them while you're in it.
    pub fn matches_part(mut self, part: impl Fn(&str) -> Option<Range<usize>> + 'static) -> Self {
        self.part = Some(Rc::new(part));
        self
    }
    /// Rank the matches in tiers, the lowest first and each in flex's order: `f` gives a
    /// candidate's tier for the text it's matched against, as `:` puts first the command a short
    /// name runs and then the names it starts. It runs on each change, guarded, and a panic leaves
    /// flex's order.
    pub fn tiers(mut self, f: impl Fn(&str, &str) -> usize + 'static) -> Self {
        self.tiers = Some(Rc::new(f));
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
    part: Option<PartFn>,
    tiers: Option<TierFn>,
    feed: Option<Feed>,
    /// With a feed: the text its last answer was for, how many matches didn't come with it, and
    /// whether more paths may come.
    answered: u64,
    more: usize,
    walking: bool,
    items: Vec<String>,
    /// How many bytes of the text aren't matched against `items`, and with a part, where it ends.
    base: usize,
    end: Option<usize>,
    /// The line has no part to match, so there's no list.
    hidden: bool,
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
    fn new(source: Source, line_row: bool, part: Option<PartFn>, tiers: Option<TierFn>) -> List {
        let (live, feed, items) = match source {
            Source::Fixed(items) => (None, None, items),
            Source::Live(f) => (Some(f), None, vec![]),
            Source::Fed(feed) => (None, Some(feed), vec![]),
        };
        List {
            live,
            line_row: line_row || part.is_some(),
            part,
            tiers,
            end: None,
            hidden: false,
            walking: feed.is_some(),
            feed,
            answered: 0,
            more: 0,
            items,
            base: 0,
            matches: vec![],
            chosen: None,
            top: 0,
            height: 0,
        }
    }
    fn query<'a>(&self, text: &'a str) -> &'a str {
        let end = self.end.unwrap_or(text.len());
        text.get(self.base..end).unwrap_or(text)
    }
    /// Match the text, with each item's tier if there are tiers.
    fn filter(&mut self, text: &str, tiers: Option<&[usize]>) {
        let query = self.query(text);
        let items = self.items.iter().filter(|_| !self.hidden);
        self.matches = flex::rank(query, items.map(String::as_str));
        if let Some(t) = tiers {
            self.matches.sort_by_key(|&i| t.get(i).copied());
        }
        let on_line = self.line_row && (self.part.is_some() || query.is_empty());
        self.chosen = (!self.matches.is_empty() && !on_line).then_some(0);
        self.top = 0;
        self.height = self.height.max(self.matches.len().min(ROWS));
    }
    /// Show what a feed answered. While it's answering the same text, a row you moved to stays
    /// chosen as the rows shift. Otherwise the best row is.
    fn install(&mut self, r: Ranked) {
        let same = r.asked == self.answered;
        let keep = (same && self.chosen.is_some_and(|c| c > 0))
            .then(|| self.chosen_item().map(str::to_string))
            .flatten();
        (self.answered, self.walking) = (r.asked, r.walking);
        self.more = r.total - r.items.len();
        self.items = r.items;
        self.matches = (0..self.items.len()).collect();
        let kept = keep.and_then(|k| self.items.iter().position(|i| *i == k));
        self.chosen = kept.or((!self.items.is_empty()).then_some(0));
        if !same {
            self.top = 0;
        }
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
    /// The line is what a history key left there, until another key changes it.
    in_history: bool,
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
    /// `<S-Tab>` on a file prompt: up a directory, as `vertico-directory-up` does. A name being
    /// typed goes, or on a directory, that directory, back to the `/` before it. `~/` goes to
    /// home's parent, and at the root nothing happens.
    fn up_dir(&mut self) {
        let at = self.cursor;
        let line = &self.text[..at];
        let dir = line.strip_suffix('/').unwrap_or(line);
        if dir.is_empty() || dir.ends_with('/') {
            return;
        }
        let start = dir.rfind('/').map_or(0, |i| i + 1);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let parent = home
            .as_deref()
            .and_then(Path::parent)
            .map(|p| p.display().to_string());
        match parent.filter(|_| &dir[start..] == "~" && line.ends_with('/')) {
            Some(p) => self.replace(start..at, &format!("{}/", p.trim_end_matches('/'))),
            None => self.replace(start..at, ""),
        }
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
        self.in_history = false;
    }
    fn set_line(&mut self, s: String) {
        self.cursor = s.len();
        self.text = s;
        self.in_history = false;
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
        let l = self.list.as_ref()?;
        let fixed = self.text.get(..l.base).unwrap_or_default();
        let rest = l.end.and_then(|e| self.text.get(e..)).unwrap_or_default();
        Some((format!("{fixed}{item}{rest}"), item.ends_with('/')))
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
                self.in_history = true;
            }
            return;
        };
        if self.recalled.is_none() {
            self.draft = self.text.clone();
        }
        self.recalled = Some(found);
        self.text = back(found).to_string();
        self.in_history = true;
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
        let (source, part, tiers) = (spec.source.take(), spec.part.take(), spec.tiers.take());
        let list = source.map(|s| List::new(s, spec.descends, part, tiers));
        let text = spec.initial.clone();
        self.prompt = Some(Prompt {
            cursor: text.len(),
            text,
            spec,
            recalled: None,
            draft: String::new(),
            in_history: false,
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
    /// Choose the row `by` rows on in the open prompt's list.
    pub(crate) fn prompt_step(&mut self, by: isize) {
        if let Some(p) = self.prompt.as_mut() {
            p.step(by);
        }
    }
    /// The rows the open prompt's list has taken, none while it has no part to match.
    pub(crate) fn prompt_list_height(&self) -> usize {
        let list = self.prompt.as_ref().and_then(|p| p.list.as_ref());
        list.filter(|l| !l.hidden).map_or(0, |l| l.height)
    }
    /// What the prompt's line starts with: how far through the candidates you are, as vertico
    /// shows it, and the label.
    pub(crate) fn prompt_lead(&self) -> Option<String> {
        let p = self.prompt.as_ref()?;
        let label = &p.spec.label;
        Some(match &p.list {
            // The line stays where it was with the list.
            Some(l) if l.hidden => format!("{:7}{label}", ""),
            Some(l) => {
                let total = l.matches.len() + l.more;
                let total = format!("{total}{}", if l.walking { "+" } else { "" });
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
                self.report_key(Err(e));
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
        self.command_ended();
    }

    pub(crate) fn prompt_key(&mut self, ev: KeyEvent) {
        let c = ev.chord();
        // Better a key that does nothing than one that opens the match for an older line.
        let up = c.key == Key::Tab && c.mods.shift;
        if matches!(c.key, Key::Enter | Key::Tab) && !up && !self.prompt_settle() {
            return;
        }
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
        // A part prompt's line keeps the history on the keys that go back, as Evil's `:` line has
        // it, and on those going forward too while you're going through it.
        let shown = p.list.as_ref().filter(|l| !l.hidden);
        let part = shown.is_some_and(|l| l.part.is_some());
        let listed = shown.is_some() && !(part && p.in_history);
        let listed_up = listed && !(part && shown.is_some_and(|l| l.chosen.is_none()));
        p.refused = false;
        match c.key {
            Key::Enter if (alt || p.chosen_line().is_none()) && p.refuses() => p.refused = true,
            Key::Enter if alt => return self.finish_prompt(Some(before), true),
            Key::Enter => match p.chosen_line() {
                Some((line, true)) if p.spec.descends => p.set_line(line),
                Some((line, _)) => return self.finish_prompt(Some(line), true),
                None => return self.finish_prompt(Some(before), true),
            },
            Key::Tab if shift && p.spec.descends => p.up_dir(),
            Key::Tab => {
                if let Some((line, _)) = p.tab_line() {
                    p.set_line(line);
                }
            }
            Key::Esc => return self.finish_prompt(None, true),
            Key::Char('v') if c.mods.super_ => {
                if let Some(r) = self.clipboard() {
                    self.prompt_insert(r.text.trim_end_matches(['\n', '\r']));
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
            Key::Up if listed_up => p.step(-1),
            Key::Down if listed => p.step(1),
            Key::PageUp if listed_up => p.step(-p.page()),
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
                'p' if listed_up => p.step(-1),
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

    /// Show what the open prompt's finder has answered, if it has news, as each turn does.
    pub(crate) fn prompt_take_feed(&mut self) -> bool {
        self.prompt_install(false)
    }
    /// Wait for the open prompt's finder to answer the line as it is, and show that. `<CR>` and
    /// `<Tab>` do this first, so they act on the list for what's on the line. False if the finder
    /// ran out of time, with its list still for an older line.
    pub fn prompt_settle(&mut self) -> bool {
        self.prompt_install(true);
        let list = self.prompt.as_ref().and_then(|p| p.list.as_ref());
        let feed = list.and_then(|l| l.feed.as_ref());
        feed.is_none_or(Feed::settled)
    }
    fn prompt_install(&mut self, settle: bool) -> bool {
        let list = self.prompt.as_mut().and_then(|p| p.list.as_mut());
        let Some(l) = list else {
            return false;
        };
        let news = l.feed.as_ref().and_then(|f| f.take(settle));
        news.map(|r| l.install(r)).is_some()
    }

    /// Match the candidates against the text, asking for them again if they depend on it. The
    /// asking is extension code, so it's guarded, and if it panics the list is empty.
    fn prompt_refilter(&mut self) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        let Some(l) = p.list.as_mut() else {
            return;
        };
        if let Some(feed) = l.feed.as_mut() {
            return feed.ask(&p.text);
        }
        let text = p.text.clone();
        let (ext, live, part) = (p.ext, l.live.clone(), l.part.clone());
        let asked = live.map(|f| self.guarded(ext, |_| Ok((f.borrow_mut())(&text))));
        let span = part.map(|f| self.guarded(ext, |_| Ok(f(&text))).ok().flatten());
        let Some(l) = self.prompt.as_mut().and_then(|p| p.list.as_mut()) else {
            return;
        };
        match asked {
            Some(Ok((base, items))) => (l.base, l.items) = (base, items),
            Some(Err(_)) => (l.base, l.items) = (0, vec![]),
            None => {}
        }
        if let Some(span) = span {
            let r = span.clone().unwrap_or(0..0);
            (l.hidden, l.base, l.end) = (span.is_none(), r.start, Some(r.end));
        }
        let tiers = l.tiers.clone().filter(|_| !l.hidden);
        let (query, items) = (l.query(&text).to_string(), std::mem::take(&mut l.items));
        let tiers = tiers.and_then(|f| {
            let rank = |_: &mut Ctx| Ok(items.iter().map(|i| f(&query, i)).collect::<Vec<_>>());
            self.guarded(ext, rank).ok()
        });
        let Some(l) = self.prompt.as_mut().and_then(|p| p.list.as_mut()) else {
            return;
        };
        l.items = items;
        l.filter(&text, tiers.as_deref());
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
