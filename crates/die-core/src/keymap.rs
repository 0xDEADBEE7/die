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
    cmd: Option<Rc<str>>,
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
    fn bind(&mut self, pats: &[Pat], cmd: &str) {
        let mut n = self;
        for p in pats {
            n = match p {
                Pat::Chord(c) => n.exact.entry(*c).or_default(),
                Pat::AnyChar => n.any_char.get_or_insert_with(Default::default),
                Pat::Any => n.any.get_or_insert_with(Default::default),
            };
        }
        n.cmd = Some(cmd.into());
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
        let leaf = n.exact.is_empty() && n.any_char.is_none() && n.any.is_none();
        match (&n.cmd, leaf) {
            (Some(c), true) => Lookup::Command(c.clone()),
            (_, false) => Lookup::Prefix, // a binding that is also a prefix waits (production: timeout)
            (None, true) => Lookup::None,
        }
    }
}

/// A keymap: bindings per modal state, from key notation to command names (C8).
#[derive(Default)]
pub struct Keymap {
    tries: HashMap<State, Trie>,
    bindings: Vec<(State, String, String)>,
    pub(crate) errors: Vec<String>,
}

impl Keymap {
    pub fn bind(&mut self, state: State, keys: &str, cmd: &str) -> &mut Self {
        match parse_pats(keys) {
            Ok(p) => {
                self.tries.entry(state).or_default().bind(&p, cmd);
                self.bindings.push((state, keys.into(), cmd.into()));
            }
            Err(e) => self.errors.push(e),
        }
        self
    }
    pub fn lookup(&self, state: State, keys: &[KeyEvent]) -> Lookup {
        self.tries
            .get(&state)
            .map_or(Lookup::None, |t| t.lookup(keys))
    }
    pub fn bindings(&self) -> impl Iterator<Item = &(State, String, String)> {
        self.bindings.iter()
    }
}
