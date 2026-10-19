//! Keymaps: per-state tries with `<char>` and `<any>`. Specificity applies within one keymap;
//! order applies across keymaps (contract §7).
use crate::keys::*;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum State {
    Normal,
    Insert,
    Visual,
    OperatorPending,
    CommandLine,
    Copy,
    Passthrough,
}

#[derive(Default)]
struct Trie {
    /// The command bound here, and its index in `Keymap::bindings`.
    cmd: Option<(Rc<str>, usize)>,
    exact: HashMap<Chord, Trie>,
    any_char: Option<Box<Trie>>,
    any: Option<Box<Trie>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    Command(Rc<str>),
    /// The keys so far are a prefix of a longer binding.
    Prefix,
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

/// A binding hidden by a later one for the same keys and state. Within a keymap, later
/// registration wins (contract §7); `describe-key` lists what it hid.
#[derive(Clone, Debug, PartialEq)]
pub struct Shadowed {
    pub state: State,
    /// The hidden binding's keys, as it was written.
    pub keys: String,
    /// The hidden binding's command.
    pub cmd: String,
    /// The command that hides it.
    pub by: String,
}

/// A keymap: bindings per modal state, from key notation to command names (C8).
pub struct Keymap {
    tries: HashMap<State, Trie>,
    bindings: Vec<(State, String, String)>,
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
            bindings: vec![],
            shadowed: vec![],
            leader,
            errors: vec![],
        }
    }

    /// Bind `keys` in `state` to the command `cmd`. A later binding for the same keys
    /// replaces an earlier one. Keys that are bound alone and as a prefix of a longer
    /// binding (`g` and `gg`) are an error, reported at startup with the C8 check.
    pub fn bind(&mut self, state: State, keys: &str, cmd: &str) -> &mut Self {
        let idx = self.bindings.len();
        self.bindings.push((state, keys.into(), cmd.into()));
        let pats = match self.pats(state, keys) {
            Ok(p) => p,
            Err(e) => {
                self.errors.push(format!("{e}, bound to {cmd}"));
                return self;
            }
        };
        match self.tries.entry(state).or_default().bind(&pats, cmd, idx) {
            Ok(None) => {}
            Ok(Some(old)) => {
                let (_, keys, cmd_old) = &self.bindings[old];
                self.shadowed.push(Shadowed {
                    state,
                    keys: keys.clone(),
                    cmd: cmd_old.clone(),
                    by: cmd.into(),
                });
            }
            Err(other) => {
                let (_, other_keys, other_cmd) = &self.bindings[other];
                self.errors.push(format!(
                    "{keys:?} (bound to {cmd}) and {other_keys:?} (bound to {other_cmd}) in \
                     {state:?}: one is a prefix of the other, which a keymap can't hold \
                     (contract §7)"
                ));
            }
        }
        self
    }

    /// `keys` as patterns, with the leader resolved.
    fn pats(&self, state: State, keys: &str) -> Result<Vec<Pat>, String> {
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

    /// Use `leader` for `<leader>`, rebuilding the bindings made so far.
    pub(crate) fn set_leader(&mut self, leader: Chord) {
        if leader == self.leader {
            return;
        }
        let bindings = std::mem::take(&mut self.bindings);
        *self = Keymap::with_leader(leader);
        for (state, keys, cmd) in bindings {
            self.bind(state, &keys, &cmd);
        }
    }

    pub fn lookup(&self, state: State, keys: &[KeyEvent]) -> Lookup {
        self.tries
            .get(&state)
            .map_or(Lookup::None, |t| t.lookup(keys))
    }

    /// Every binding, in registration order, including shadowed ones.
    pub fn bindings(&self) -> impl Iterator<Item = &(State, String, String)> {
        self.bindings.iter()
    }

    /// The bindings hidden by later ones for the same keys, in the order they were hidden.
    pub fn shadowed(&self) -> &[Shadowed] {
        &self.shadowed
    }
}
