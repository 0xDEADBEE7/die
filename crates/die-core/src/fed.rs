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

    /// A key failed: what's left of the fed keys goes.
    pub(crate) fn drop_fed(&mut self) {
        self.fed.clear();
        let hooks: Vec<_> = self
            .subs
            .iter()
            .filter_map(|e| match &e.kind {
                SubKind::Dropped(f) if e.inner.alive.get() => Some((e.inner.clone(), f.clone())),
                _ => None,
            })
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
