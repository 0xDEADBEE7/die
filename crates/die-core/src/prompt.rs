//! S4's prompt, moved from `ctx.rs` as it was. K11a builds the real one here (contract §12).
#![warn(missing_docs)]
use crate::error::*;
use crate::ext::*;
use crate::keymap::*;
use crate::keys::*;
use crate::rt::*;

type PromptFn = Box<dyn FnOnce(&mut Ctx, Option<String>) -> Result>;

pub(crate) struct Prompt {
    label: String,
    text: String,
    ext: ExtIdx,
    prev: State,
    f: PromptFn,
}

impl Ctx {
    /// A command-line prompt. `f` runs from the top of the loop with the answer, or None on cancel.
    pub fn prompt(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Ctx, Option<String>) -> Result + 'static,
    ) {
        if self.prompt.is_some() {
            self.finish_prompt(None, false);
        }
        let prev = self.state();
        self.set_state(State::CommandLine);
        self.prompt = Some(Prompt {
            label: label.into(),
            text: String::new(),
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
        self.prompt.as_ref().map(|p| p.label.as_str())
    }
    /// Queue the answer. `keyed` says a key submitted it, so it's part of the key stream (contract
    /// §6): if it fails, the rest of what was fed drops. K11a's rebuild keeps this.
    fn finish_prompt(&mut self, answer: Option<String>, keyed: bool) {
        let Some(p) = self.prompt.take() else { return };
        self.set_state(p.prev);
        let f = p.f;
        self.queue.push_back(Queued {
            ext: p.ext,
            alive: None,
            keyed,
            f: Box::new(move |ctx| f(ctx, answer)),
        });
    }

    pub(crate) fn prompt_key(&mut self, ev: KeyEvent) {
        match ev.chord().key {
            Key::Enter => {
                let t = self.prompt.as_ref().map(|p| p.text.clone());
                self.finish_prompt(t, true);
            }
            Key::Esc => self.finish_prompt(None, true),
            Key::Backspace => {
                self.prompt.as_mut().unwrap().text.pop();
            }
            _ => {
                if let Some(t) = ev.printable() {
                    self.prompt.as_mut().unwrap().text.push_str(&t);
                }
            }
        }
    }
}
