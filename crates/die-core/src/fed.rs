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
        let end = self.fed.len();
        let from = if self.nesting() > 0 { self.fed_at } else { end };
        let at = from.min(end);
        let rest = self.fed.split_off(at);
        self.fed.extend(evs);
        self.fed_at = self.fed.len();
        self.fed.extend(rest);
        Ok(())
    }
    /// Run `f` from the top of the loop, before the next key is looked up, whenever a key's
    /// failure ends what was fed (contract §6). It runs for any key that fails, even with
    /// nothing left to drop, since the last key of a macro can fail too.
    pub fn on_fed_dropped(&mut self, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        self.new_sub(SubKind::Dropped(Rc::new(RefCell::new(f))))
    }

    /// Run `f` from the top of the loop, before the next key is looked up, whenever a command
    /// has run from a key, whoever's it is, a prompt has given its answer, or a pointer press has
    /// moved the cursor (contract §6). A key that only starts a longer one runs nothing.
    pub fn on_command_ended(&mut self, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        self.new_sub(SubKind::Ended(Rc::new(RefCell::new(f))))
    }

    /// A key failed: what's left of the fed keys goes.
    pub(crate) fn drop_fed(&mut self) {
        self.fed.clear();
        self.queue_hooks(|kind| match kind {
            SubKind::Dropped(f) => Some(f.clone()),
            _ => None,
        });
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

    /// Empty a register, as if nothing had ever been put in it.
    pub fn clear_register(&mut self, c: char) {
        self.registers.remove(&c);
        self.registers_saved.take();
    }
}
