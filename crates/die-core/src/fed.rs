//! The fed keys: what `feed_keys` queues, and what happens when a failure drops them. Lives
//! apart from dispatch so the key loop's module keeps its size.

use crate::error::*;
use crate::ext::*;
use crate::keys::*;
use crate::rt::*;
use std::cell::RefCell;
use std::rc::Rc;

impl Ctx {
    /// Queue keys as if typed (contract §6). die-vim's `@a` uses this. `keys` is notation,
    /// as `key_notation` writes it, so plain text with a `<` in it needs `<lt>`. They run ahead
    /// of typed keys, and a key's command failing, or a key nothing handles, drops the rest.
    /// Fed from inside a call, they also run ahead of what's left, in the order they're fed, so
    /// a macro inside a macro runs before the rest of it. The host's go behind what's left.
    pub fn feed_keys(&mut self, keys: &str) -> Result {
        let evs = typed(keys, Some(self.leader)).map_err(Error::Msg)?;
        if self.nesting() == 0 {
            // Behind what any feed kept apart is waiting on.
            self.fences.iter_mut().for_each(|f| *f += evs.len());
            self.fed.extend(evs);
            self.fed_at = self.fed.len();
            return Ok(());
        }
        // At the front, moving only what this turn has fed, so a macro that keeps calling macros
        // doesn't move everything still waiting each time.
        let at = self.fed_at.min(self.fed.len());
        let this_turn: Vec<_> = self.fed.drain(..at).collect();
        self.fed_at = at + evs.len();
        evs.into_iter().rev().for_each(|ev| self.fed.push_front(ev));
        for ev in this_turn.into_iter().rev() {
            self.fed.push_front(ev);
        }
        Ok(())
    }
    /// Feed keys as [`Ctx::feed_keys`] does, kept apart from the fed keys behind them, as Emacs
    /// runs a keyboard macro inside `ignore-errors` (contract §6): a failure among them, or among
    /// keys they feed, drops only those. Once they've all run, `on_fed_run_out` calls run before
    /// the next key, so what they left waiting can be ended. die-vim's `:norm` feeds each line so.
    pub fn feed_keys_apart(&mut self, keys: &str) -> Result {
        let behind = match self.nesting() {
            0 => 0,
            _ => self.fed.len() - self.fed_at.min(self.fed.len()),
        };
        self.feed_keys(keys)?;
        self.fences.push(behind);
        Ok(())
    }
    /// Run `f` from the top of the loop, before the next key is looked up, whenever keys fed by
    /// [`Ctx::feed_keys_apart`] have all run, and no failure dropped them (contract §6).
    pub fn on_fed_run_out(&mut self, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        self.new_sub(SubKind::RunOut(Rc::new(RefCell::new(f))))
    }
    /// Whether the keys so far only start a longer key, as `f` waits for its character.
    pub fn keys_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    /// Run `f` from the top of the loop, before the next key is looked up, whenever a key's
    /// failure ends what was fed (contract §6). It runs for any key that fails, even with
    /// nothing left to drop, since the last key of a macro can fail too.
    pub fn on_fed_dropped(&mut self, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        self.new_sub(SubKind::Dropped(Rc::new(RefCell::new(f))))
    }

    /// Whether a typed `C-g` ended what was fed, and not a key's failure, since this last looked,
    /// so an `on_fed_dropped` call can tell a quit from an error, as Emacs's `quit` isn't an
    /// `error`. die-vim's `:norm` goes on to the next line after an error, and stops at a quit.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// While on, a key that fails, or that nothing handles, or a prompt's answer that fails, ends
    /// what was fed as ever, but says nothing, as Emacs's `ignore-errors` round a keyboard macro.
    /// die-vim's `:norm` turns it on while a line's keys run. Typed keys are hushed too.
    pub fn hush_key_errors(&mut self, on: bool) {
        self.hushed = on;
    }

    /// Run `f` from the top of the loop, before the next key is looked up, whenever a command
    /// has run from a key, whoever's it is, a prompt has given its answer, or a pointer press has
    /// moved the cursor (contract §6). A key that only starts a longer one runs nothing.
    pub fn on_command_ended(&mut self, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        self.new_sub(SubKind::Ended(Rc::new(RefCell::new(f))))
    }

    /// A key failed: what's left of the fed keys goes, as far as the innermost feed kept apart.
    pub(crate) fn drop_fed(&mut self) {
        let keep = self.fences.pop().unwrap_or(0).min(self.fed.len());
        self.fed.drain(..self.fed.len() - keep);
        self.queue_hooks(|kind| match kind {
            SubKind::Dropped(f) => Some(f.clone()),
            _ => None,
        });
    }

    /// Before the next key: if the innermost feed kept apart has all run, say so.
    pub(crate) fn fed_ran_out(&mut self) -> bool {
        if self.fences.last().is_none_or(|&b| self.fed.len() > b) {
            return false;
        }
        self.fences.pop();
        self.queue_hooks(|kind| match kind {
            SubKind::RunOut(f) => Some(f.clone()),
            _ => None,
        });
        true
    }

    /// A command is over.
    pub(crate) fn command_ended(&mut self) {
        self.queue_hooks(|kind| match kind {
            SubKind::Ended(f) => Some(f.clone()),
            _ => None,
        });
    }

    fn queue_hooks(&mut self, which: impl Fn(&SubKind) -> Option<TimerFn>) {
        let hooks: Vec<_> = self
            .subs
            .iter()
            .filter(|e| e.inner.alive.get())
            .filter_map(|e| which(&e.kind).map(|f| (e.inner.clone(), f)))
            .collect();
        for (inner, f) in hooks {
            self.queue.push_back(Queued {
                ext: inner.ext,
                alive: Some(inner),
                keyed: false,
                f: Box::new(move |ctx| (f.borrow_mut())(ctx)),
            });
        }
    }

    /// Give a register's text to another name, without copying it. An empty one changes nothing.
    pub fn move_register(&mut self, from: char, to: char) {
        if let Some(r) = self.registers.remove(&from) {
            self.set_register(to, r);
        }
    }
    /// Empty a register, as if nothing had ever been put in it.
    pub fn clear_register(&mut self, c: char) {
        self.registers.remove(&c);
        self.registers_saved.take();
    }
}
