//! Keymaps: per-state tries with `<char>` and `<any>`. Specificity applies within one keymap;
//! order applies across keymaps (contract §7).
#![warn(missing_docs)]

use crate::keys::*;
use std::collections::HashMap;
use std::rc::Rc;

/// A modal state (scope §5.6), kept per buffer. Each keymap binds keys per state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum State {
    /// Vim's normal state.
    Normal,
    /// Typing inserts text.
    Insert,
    /// Replace (`R`): typed text overwrites. Its cursor is `CursorShape::Underline`.
    Replace,
    /// A selection grows with the motions.
    Visual,
    /// An operator waits for its motion.
    OperatorPending,
    /// A prompt has the keys.
    CommandLine,
    /// Normal and visual over a read-only snapshot of a surface (scope D19).
    Copy,
    /// Every key outside the DIE layer goes to the surface (scope D18).
    Passthrough,
}

/// Every node but the root has a binding at or below it.
#[derive(Default)]
struct Trie {
    /// The command bound here, and its index in `Keymap::log`.
    cmd: Option<(Rc<str>, usize)>,
    exact: HashMap<Chord, Trie>,
    any_char: Option<Box<Trie>>,
    any: Option<Box<Trie>>,
}

/// What a key sequence means in a keymap.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// It's bound to this command.
    Command(Rc<str>),
    /// The keys so far are a prefix of a longer binding.
    Prefix,
    /// Nothing is bound to it here.
    None,
}

impl Trie {
    fn children(&self) -> impl Iterator<Item = &Trie> {
        let wild = self.any_char.iter().chain(&self.any).map(|t| &**t);
        self.exact.values().chain(wild)
    }

    /// The bindings at or below this node.
    fn bound(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.cmd.iter().map(|(_, i)| *i).collect();
        out.extend(self.children().flat_map(Trie::bound));
        out
    }

    /// Bind `pats` to binding `idx`. Ok(the binding it replaced), or Err(a binding that
    /// is a prefix of `pats`, or that `pats` is a prefix of), in which case nothing is bound.
    fn bind(&mut self, pats: &[Pat], cmd: &str, idx: usize) -> Result<Option<usize>, usize> {
        if let Some(other) = self.prefix_conflict(pats) {
            return Err(other);
        }
        let mut n = self;
        for p in pats {
            n = match p {
                Pat::Chord(c) => n.exact.entry(*c).or_default(),
                Pat::AnyChar => n.any_char.get_or_insert_with(Default::default),
                Pat::Any => n.any.get_or_insert_with(Default::default),
                Pat::Leader => unreachable!("the keymap resolves the leader first"),
            };
        }
        Ok(n.cmd.replace((cmd.into(), idx)).map(|(_, old)| old))
    }

    /// The binding `pats` passes through, or else the earliest one that starts with `pats`.
    fn prefix_conflict(&self, pats: &[Pat]) -> Option<usize> {
        let mut n = self;
        for p in pats {
            if let Some((_, i)) = &n.cmd {
                return Some(*i);
            }
            n = match p {
                Pat::Chord(c) => n.exact.get(c),
                Pat::AnyChar => n.any_char.as_deref(),
                Pat::Any => n.any.as_deref(),
                Pat::Leader => None,
            }?;
        }
        n.children().flat_map(Trie::bound).min()
    }

    /// Remove every binding that starts with `pats`, and the nodes left with nothing bound
    /// below them, so they aren't prefixes any more. Returns the removed bindings.
    fn unbind(&mut self, pats: &[Pat]) -> Vec<usize> {
        let Some((p, rest)) = pats.split_first() else {
            return std::mem::take(self).bound();
        };
        let child = match p {
            Pat::Chord(c) => self.exact.get_mut(c),
            Pat::AnyChar => self.any_char.as_deref_mut(),
            Pat::Any => self.any.as_deref_mut(),
            Pat::Leader => None,
        };
        let Some(child) = child else { return vec![] };
        let gone = child.unbind(rest);
        if child.bound().is_empty() {
            match p {
                Pat::Chord(c) => drop(self.exact.remove(c)),
                Pat::AnyChar => self.any_char = None,
                Pat::Any => self.any = None,
                Pat::Leader => {}
            }
        }
        gone
    }

    /// Exact beats `<char>`, which beats `<any>`, at every step. No backtracking.
    fn lookup(&self, keys: &[KeyEvent]) -> Lookup {
        let mut n = self;
        for k in keys {
            let next = n
                .exact
                .get(&k.chord())
                .or_else(|| k.printable().and(n.any_char.as_deref()))
                .or(n.any.as_deref());
            match next {
                Some(x) => n = x,
                None => return Lookup::None,
            }
        }
        // Registration refuses a binding that is also a prefix, so a node has one or the other.
        match &n.cmd {
            Some((c, _)) => Lookup::Command(c.clone()),
            None if n.children().next().is_none() => Lookup::None,
            None => Lookup::Prefix,
        }
    }
}

/// A binding hidden by a later one for the same keys and state, or removed by `unbind`.
/// Within a keymap, later registration wins (contract §7); `describe-key` lists what it hid.
#[derive(Clone, Debug, PartialEq)]
pub struct Shadowed {
    /// The state it was bound in.
    pub state: State,
    /// The hidden binding's keys, as it was written.
    pub keys: String,
    /// The hidden binding's command.
    pub cmd: String,
    /// The command that hides it, or None if it was unbound.
    pub by: Option<String>,
}

/// A keymap: bindings per modal state, from key notation to command names (C8).
pub struct Keymap {
    tries: HashMap<State, Trie>,
    /// Every bind and unbind (no command), in order, replayed when the leader changes.
    pub(crate) log: Vec<(State, String, Option<String>)>,
    shadowed: Vec<Shadowed>,
    leader: Chord,
    pub(crate) errors: Vec<String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::with_leader(default_leader())
    }
}

/// The leader until config sets one: `\`.
pub(crate) fn default_leader() -> Chord {
    KeyEvent::press(Key::Char('\\')).chord()
}

impl Keymap {
    pub(crate) fn with_leader(leader: Chord) -> Self {
        Keymap {
            tries: HashMap::new(),
            log: vec![],
            shadowed: vec![],
            leader,
            errors: vec![],
        }
    }

    /// Bind `keys` in `state` to the command `cmd`. A later binding for the same keys
    /// replaces an earlier one. Keys that are bound alone and as a prefix of a longer
    /// binding (`g` and `gg`) are an error, reported at startup with the C8 check.
    pub fn bind(&mut self, state: State, keys: &str, cmd: &str) -> &mut Self {
        let idx = self.log.len();
        self.log.push((state, keys.into(), Some(cmd.into())));
        let pats = match self.pats(state, keys) {
            Ok(p) => p,
            Err(e) => {
                self.errors.push(format!("{e}, bound to {cmd}"));
                return self;
            }
        };
        match self.tries.entry(state).or_default().bind(&pats, cmd, idx) {
            Ok(None) => {}
            Ok(Some(old)) => self.shadow(old, Some(cmd)),
            Err(other) => {
                let (_, other_keys, other_cmd) = &self.log[other];
                self.errors.push(format!(
                    "{keys:?} (bound to {cmd}) and {other_keys:?} (bound to {}) in {state:?}: \
                     one is a prefix of the other, which a keymap can't hold (contract §7)",
                    other_cmd.as_deref().unwrap_or_default()
                ));
            }
        }
        self
    }

    /// Remove the binding for `keys` in `state`, and every longer one that starts with
    /// `keys`. This is how your config frees a key an extension bound, to bind it alone
    /// where the extension uses it as a prefix, or the reverse. The removed bindings are
    /// listed as shadowed. Unbinding keys that start no binding is a startup error.
    pub fn unbind(&mut self, state: State, keys: &str) -> &mut Self {
        self.log.push((state, keys.into(), None));
        let gone = match self.pats(state, keys) {
            Ok(pats) => self.tries.entry(state).or_default().unbind(&pats),
            Err(e) => {
                self.errors.push(format!("{e}, unbound"));
                return self;
            }
        };
        if gone.is_empty() {
            self.errors.push(format!(
                "{keys:?} in {state:?} is unbound, but nothing is bound to it"
            ));
        }
        for i in gone {
            self.shadow(i, None);
        }
        self
    }

    /// `keys` as patterns, with the leader resolved.
    pub(crate) fn pats(&self, state: State, keys: &str) -> Result<Vec<Pat>, String> {
        let mut pats = parse_pats(keys)?;
        if pats.is_empty() {
            return Err(format!("no keys in {state:?}"));
        }
        let modifier = |p: &Pat| {
            matches!(
                p,
                Pat::Chord(Chord {
                    key: Key::Modifier(_),
                    ..
                })
            )
        };
        if pats.iter().any(modifier) {
            return Err(format!(
                "{keys:?} in {state:?} has a modifier key alone, which never reaches a keymap"
            ));
        }
        for p in &mut pats {
            if *p == Pat::Leader {
                *p = Pat::Chord(self.leader);
            }
        }
        Ok(pats)
    }

    /// List binding `idx` as shadowed `by` a command, or unbound.
    fn shadow(&mut self, idx: usize, by: Option<&str>) {
        let (state, keys, cmd) = &self.log[idx];
        self.shadowed.push(Shadowed {
            state: *state,
            keys: keys.clone(),
            cmd: cmd.clone().unwrap_or_default(),
            by: by.map(Into::into),
        });
    }

    /// Use `leader` for `<leader>`, rebuilding the bindings made so far.
    pub(crate) fn set_leader(&mut self, leader: Chord) {
        if leader == self.leader {
            return;
        }
        let log = std::mem::take(&mut self.log);
        *self = Keymap::with_leader(leader);
        for (state, keys, cmd) in log {
            match cmd {
                Some(cmd) => self.bind(state, &keys, &cmd),
                None => self.unbind(state, &keys),
            };
        }
    }

    /// What `keys` mean in `state`. An exact key beats `<char>`, which beats `<any>`, at every
    /// step, with no backtracking: a key that fails under a prefix isn't tried against the
    /// root's wildcards (contract §7).
    pub fn lookup(&self, state: State, keys: &[KeyEvent]) -> Lookup {
        self.tries
            .get(&state)
            .map_or(Lookup::None, |t| t.lookup(keys))
    }

    /// The command bound to this one key by name, leaving out `<any>` and `<char>`.
    pub(crate) fn command_for_exactly(&self, state: State, key: &KeyEvent) -> Option<Rc<str>> {
        let node = self.tries.get(&state)?.exact.get(&key.chord())?;
        Some(node.cmd.as_ref()?.0.clone())
    }

    /// Every binding as (state, keys, command), in registration order, including shadowed
    /// and unbound ones.
    pub fn bindings(&self) -> impl Iterator<Item = (State, &str, &str)> {
        self.log
            .iter()
            .filter_map(|(s, k, c)| Some((*s, k.as_str(), c.as_deref()?)))
    }

    /// The indices in `log` of the bindings in force.
    pub(crate) fn live(&self) -> Vec<usize> {
        self.tries.values().flat_map(Trie::bound).collect()
    }

    /// The bindings hidden by later ones for the same keys, or unbound, in that order.
    pub fn shadowed(&self) -> &[Shadowed] {
        &self.shadowed
    }
}
