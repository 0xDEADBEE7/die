//! The command line (contract §12, K11a): one line of text with a cursor, Emacs's and Vim's line
//! editing keys, and a history per name. It's what `:`, `/` and `ctx.prompt` type into.
//!
//! The keys, which a prompt takes before any keymap sees them:
//! - Answer: `<CR>`. Cancel: `<Esc>`, `<C-g>` and `<C-c>`.
//! - Move: `<Left>` `<C-b>`, `<Right>` `<C-f>`, `<Home>` `<C-a>`, `<End>` `<C-e>`, and by word
//!   `<M-b>` `<M-Left>` `<C-Left>`, `<M-f>` `<M-Right>` `<C-Right>`.
//! - Delete: `<BS>` `<C-h>`, `<Del>` `<C-d>`, a word with `<C-w>` `<M-BS>` and `<M-d>`, and to
//!   either end of the line with `<C-u>` and `<C-k>`.
//! - History: `<Up>` `<C-p>` for older, `<Down>` `<C-n>` for newer. Going past the newest brings
//!   back what you were typing.
//!
//! The cursor moves by what the scene draws as one glyph, a grapheme cluster. A word is a run of
//! alphanumerics and `_`, or a run of other non-blanks, as in Vim, so `<C-w>` on `a/b` takes `b`.
#![warn(missing_docs)]
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

/// How many answers a history keeps.
const HISTORY: usize = 100;

/// What a prompt shows and remembers. A plain `&str` is a label and nothing else.
pub struct PromptSpec {
    label: String,
    initial: String,
    history: Option<String>,
    backspace_cancels: bool,
    on_change: Option<ChangeFn>,
}

impl PromptSpec {
    /// A prompt with this label.
    pub fn new(label: &str) -> Self {
        PromptSpec {
            label: label.into(),
            initial: String::new(),
            history: None,
            backspace_cancels: false,
            on_change: None,
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
    /// Run `f` with the text each time a key changes it, for incremental search. Each call is
    /// queued (C3), in order with the answer, and isn't part of the key stream: an error from it
    /// is reported and drops nothing.
    pub fn on_change(mut self, f: impl FnMut(&mut Ctx, &str) -> Result + 'static) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(f)));
        self
    }
}

impl From<&str> for PromptSpec {
    fn from(label: &str) -> Self {
        PromptSpec::new(label)
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
    /// Go older or newer in `past`, which is oldest first. Past the oldest, nothing happens.
    fn recall(&mut self, past: &[String], older: bool) {
        let at = match (self.recalled, older) {
            (None, true) => 0,
            (None, false) => return,
            (Some(0), false) => {
                self.recalled = None;
                let draft = std::mem::take(&mut self.draft);
                return self.set_line(draft);
            }
            (Some(i), true) => i + 1,
            (Some(i), false) => i - 1,
        };
        let Some(entry) = past.iter().rev().nth(at) else {
            return;
        };
        if self.recalled.is_none() {
            self.draft = self.text.clone();
        }
        self.recalled = Some(at);
        self.set_line(entry.clone());
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
        let spec = spec.into();
        let text = spec.initial.clone();
        self.prompt = Some(Prompt {
            cursor: text.len(),
            text,
            spec,
            recalled: None,
            draft: String::new(),
            ext: self.cur_ext(),
            prev,
            f: Box::new(f),
        });
    }
    /// The text typed into the open prompt, if there is one.
    pub fn prompt_text(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.text.as_str())
    }
    pub(crate) fn prompt_label(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.spec.label.as_str())
    }
    /// The text before the prompt's cursor.
    pub(crate) fn prompt_before_cursor(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| &p.text[..p.cursor])
    }
    /// The answers remembered under a history's name, oldest first.
    pub fn prompt_history(&self, name: &str) -> &[String] {
        self.histories.get(name).map_or(&[], Vec::as_slice)
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
        let (ctrl, alt) = (c.mods.ctrl, c.mods.alt);
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        let (at, end) = (p.cursor, p.text.len());
        let before = p.text.clone();
        match c.key {
            Key::Enter => return self.finish_prompt(Some(before), true),
            Key::Esc => return self.finish_prompt(None, true),
            Key::Char('g' | 'c') if ctrl => return self.finish_prompt(None, true),
            Key::Backspace if end == 0 && p.spec.backspace_cancels => {
                return self.finish_prompt(None, true);
            }
            Key::Left if ctrl || alt => p.cursor = p.word_before(),
            Key::Right if ctrl || alt => p.cursor = p.word_after(),
            Key::Left => p.cursor = p.before(),
            Key::Right => p.cursor = p.after(),
            Key::Home => p.cursor = 0,
            Key::End => p.cursor = end,
            Key::Backspace if alt => p.replace(p.word_before()..at, ""),
            Key::Backspace => p.replace(p.before()..at, ""),
            Key::Delete => p.replace(at..p.after(), ""),
            Key::Up => self.prompt_recall(true),
            Key::Down => self.prompt_recall(false),
            Key::Char(k) if ctrl => match k {
                'b' => p.cursor = p.before(),
                'f' => p.cursor = p.after(),
                'a' => p.cursor = 0,
                'e' => p.cursor = end,
                'h' => p.replace(p.before()..at, ""),
                'd' => p.replace(at..p.after(), ""),
                'w' => p.replace(p.word_before()..at, ""),
                'u' => p.replace(0..at, ""),
                'k' => p.replace(at..end, ""),
                'p' => self.prompt_recall(true),
                'n' => self.prompt_recall(false),
                _ => {}
            },
            Key::Char(k) if alt => match k {
                'b' => p.cursor = p.word_before(),
                'f' => p.cursor = p.word_after(),
                'd' => p.replace(at..p.word_after(), ""),
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

    fn prompt_recall(&mut self, older: bool) {
        let Some(mut p) = self.prompt.take() else {
            return;
        };
        let name = p.spec.history.as_deref().unwrap_or_default();
        p.recall(self.prompt_history(name), older);
        self.prompt = Some(p);
    }

    /// Queue the `on_change` call, if the key changed the text.
    fn prompt_changed(&mut self, before: &str) {
        let Some(p) = self.prompt.as_ref() else {
            return;
        };
        let Some(f) = p.spec.on_change.clone().filter(|_| p.text != before) else {
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
