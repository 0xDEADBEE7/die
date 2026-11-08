//! `describe-key` and `describe-command` (contract §6): what a key does and why, and what a
//! command is and which keys reach it, shown in `*help*`. A description follows the lookup
//! order as dispatch does, so it can't disagree with what the key would do.
use crate::error::*;
use crate::ext::*;
use crate::ids::BufferId;
use crate::keymap::*;
use crate::keys::*;
use crate::prompt::{PromptSpec, recent_first};
use crate::text::Origin;
use crate::workspace::{ShowIn, Side};
use std::fmt;

const HELP: &str = "*help*";

/// What a key sequence does in the focused window: the first step of the lookup order to take it.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    /// It runs a command.
    Command {
        /// The command's name.
        name: String,
        /// The command's doc.
        doc: &'static str,
        /// Whoever registered the command.
        owner: &'static str,
        /// The keymap it was found in, by a name such as `mode fold` or `kind text`.
        keymap: String,
    },
    /// It starts a longer binding in this keymap.
    Prefix(String),
    /// It's the DIE layer's Esc, or a plain Esc the surface doesn't take now, which leaves
    /// passthrough for normal state before any keymap and never reaches the surface.
    LeavesPassthrough,
    /// No keymap takes it, so it goes to the surface's `event()`.
    Surface {
        /// The surface's kind.
        kind: &'static str,
        /// The surface declares it handles this key ([`Surface::handles`]).
        declared: bool,
    },
    /// Nothing takes it.
    Undefined,
}

/// What [`Ctx::describe_key`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyDescription {
    /// The keys, in notation.
    pub keys: String,
    /// The modal state they were looked up in.
    pub state: State,
    /// What they do.
    pub action: KeyAction,
    /// Each keymap's name with a binding hidden there: first what later registration or
    /// `unbind` hid in any keymap on the way, then what the keymaps below the one that took the
    /// keys bind.
    pub shadowed: Vec<(String, Shadowed)>,
}

/// What [`Ctx::describe_command`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandDescription {
    /// Its name.
    pub name: String,
    /// Its doc.
    pub doc: &'static str,
    /// Whoever registered it.
    pub owner: &'static str,
    /// Every key bound to it as (keymap, state, keys), from each keymap in the registry and the
    /// focused buffer's own. A binding since hidden or unbound isn't listed.
    pub keys: Vec<(String, State, String)>,
}

/// Whether a binding's patterns and these keys agree as far as both go, so one starts the other
/// or they're the same.
fn overlaps(pats: &[Pat], keys: &[KeyEvent]) -> bool {
    pats.iter().zip(keys).all(|(p, k)| match p {
        Pat::Chord(c) => *c == k.chord(),
        Pat::AnyChar => k.printable().is_some(),
        Pat::Any => true,
        Pat::Leader => false,
    })
}

impl Keymap {
    fn overlapping(&self, state: State, written: &str, keys: &[KeyEvent]) -> bool {
        self.pats(state, written).is_ok_and(|p| overlaps(&p, keys))
    }

    /// The bindings in force, as (state, keys, command), in registration order.
    fn in_force(&self) -> Vec<(State, &str, &str)> {
        let mut live = self.live();
        live.sort_unstable();
        let log = live.into_iter().map(|i| &self.log[i]);
        log.filter_map(|(s, k, c)| Some((*s, k.as_str(), c.as_deref()?)))
            .collect()
    }
}

impl Ctx {
    /// What `keys` would do in the focused window, or None with no window or no keys.
    pub fn describe_key(&self, keys: &[KeyEvent]) -> Option<KeyDescription> {
        let last = keys.last()?;
        let buf = self.focused_buffer()?;
        let state = self.state_of(buf);
        let layers = self.layers(buf);
        let passthrough = state == State::Passthrough;
        // In passthrough only the DIE layer, held with super, reaches a keymap (contract §7).
        let hit = layers
            .iter()
            .enumerate()
            .filter(|_| !passthrough || last.mods.super_)
            .map(|(i, (_, km))| (i, km.lookup(state, keys)))
            .find(|(_, l)| *l != Lookup::None);
        let plain = last.mods == Mods::default() && !self.surface_takes_esc(buf);
        let (action, by) = match &hit {
            _ if passthrough && last.key == Key::Esc && (last.mods.super_ || plain) => {
                (KeyAction::LeavesPassthrough, None)
            }
            Some((i, Lookup::Command(name))) => {
                let (doc, owner) = self.command_doc(name).unwrap_or(("", "nobody"));
                let action = KeyAction::Command {
                    name: name.to_string(),
                    doc,
                    owner,
                    keymap: layers[*i].0.clone(),
                };
                (action, Some(name.to_string()))
            }
            Some((i, _)) if !passthrough => {
                let keymap = &layers[*i].0;
                let by = format!("the longer bindings in {keymap}");
                (KeyAction::Prefix(keymap.clone()), Some(by))
            }
            _ => (self.past_keymaps(buf, last), None),
        };
        let mut shadowed = vec![];
        for (name, km) in &layers {
            let hidden = km.shadowed().iter();
            let hidden =
                hidden.filter(|s| s.state == state && km.overlapping(state, &s.keys, keys));
            shadowed.extend(hidden.map(|s| (name.clone(), s.clone())));
        }
        if let (Some((winner, _)), Some(by)) = (&hit, by) {
            for (name, km) in &layers[winner + 1..] {
                for (s, written, cmd) in km.in_force() {
                    if s == state && km.overlapping(state, written, keys) {
                        let hidden = Shadowed {
                            state,
                            keys: written.into(),
                            cmd: cmd.into(),
                            by: Some(by.clone()),
                        };
                        shadowed.push((name.clone(), hidden));
                    }
                }
            }
        }
        Some(KeyDescription {
            keys: key_notation(keys),
            state,
            action,
            shadowed,
        })
    }

    /// The step of the lookup order after the keymaps.
    fn past_keymaps(&self, buf: BufferId, last: &KeyEvent) -> KeyAction {
        match self.buffers.get(buf).map(|b| &b.content) {
            Some(Content::Custom { kind, obj, .. }) => KeyAction::Surface {
                kind,
                declared: obj.as_ref().is_some_and(|o| o.handles(last)),
            },
            _ => KeyAction::Undefined,
        }
    }

    /// What `name` is and which keys reach it, or None if no command has that name.
    pub fn describe_command(&self, name: &str) -> Option<CommandDescription> {
        let (doc, owner) = self.command_doc(name)?;
        let mut maps = self.all_keymaps();
        if let Some(b) = self.focused_buffer() {
            maps.push(("buffer-local".into(), &self.buffers[b].local));
        }
        let mut keys = vec![];
        for (map, km) in maps {
            let bound = km.in_force().into_iter().filter(|(.., c)| *c == name);
            keys.extend(bound.map(|(s, k, _)| (map.clone(), s, k.to_owned())));
        }
        Some(CommandDescription {
            name: name.into(),
            doc,
            owner,
            keys,
        })
    }

    /// `describe-key` has the keys: take this one, and describe the sequence once it's whole. A
    /// key still held from the command that asked isn't an answer.
    pub(crate) fn describe_key_read(&mut self, ev: KeyEvent) {
        if ev.kind != KeyKind::Press {
            return;
        }
        let Some(mut keys) = self.describing.take() else {
            return;
        };
        let state = self.state();
        keys.push(ev);
        let more = state != State::Passthrough
            && self
                .focused_buffer()
                .is_some_and(|b| self.layers_lookup(b, state, &keys) == Lookup::Prefix);
        if more {
            self.log.echo = Some(format!("Describe key: {}", key_notation(&keys)));
            self.describing = Some(keys);
            return;
        }
        if let Some(d) = self.describe_key(&keys) {
            let r = self.show_help(&d.to_string());
            self.report(r);
        }
    }

    /// Put `text` in `*help*`, in a window of its own, with the focus staying where it is.
    fn show_help(&mut self, text: &str) -> Result {
        let buf = match self.find_buffer(HELP) {
            Some(b) => b,
            None => {
                let b = self.create_text(HELP, "");
                self.text_mut(b)?.settings_mut().read_only = true;
                b
            }
        };
        let t = self.text_mut(buf)?;
        let end = t.len();
        let _ = t.tx(Origin::Extension).silent().delete(0..end).commit();
        let _ = t.tx(Origin::Extension).silent().insert(0, text).commit();
        t.set_modified(false);
        let here = self.focused;
        let windows = self.window_list();
        let how = if windows.iter().any(|w| self.window_buffer(*w) == Some(buf)) {
            ShowIn::Existing
        } else {
            ShowIn::Split(Side::Below)
        };
        let shown = self.show(buf, how);
        let shown = shown.or_else(|_| self.show(buf, ShowIn::Current))?;
        if let Some(w) = shown {
            self.text_mut(buf)?.set_cursor(w, 0);
        }
        if let Some(w) = here.filter(|w| self.windows.contains_key(*w)) {
            self.focus(w);
        }
        Ok(())
    }
}

pub(super) fn register(reg: &mut Registry) {
    reg.command(
        "describe-key",
        "Read a key sequence and show what it does: the command, the keymap it was found in, and \
         what it shadows",
        |ctx, _| {
            ctx.describing = Some(vec![]);
            ctx.log.echo = Some("Describe key: ".into());
            Ok(())
        },
    );
    reg.command(
        "describe-command",
        "Choose a command by name and show its doc, its owner and every key bound to it",
        |ctx, _| {
            let names = ctx.command_names().iter().map(|n| n.to_string()).collect();
            // A history of its own: what you looked up isn't what the palette should offer first.
            let spec = PromptSpec::new("Describe command: ")
                .history("describe-command")
                .candidates(recent_first(ctx.prompt_history("describe-command"), names));
            ctx.prompt(spec, |ctx, name| {
                let Some(name) = name.filter(|n| !n.is_empty()) else {
                    return Ok(());
                };
                let d = ctx.describe_command(&name).ok_or(Error::NoCommand(name))?;
                ctx.show_help(&d.to_string())
            });
            Ok(())
        },
    );
}

/// A state as you know it: passthrough is insert state on a surface (scope D18).
struct Shown(State);

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            State::Passthrough => f.write_str("Insert"),
            s => write!(f, "{s:?}"),
        }
    }
}

impl fmt::Display for KeyDescription {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (keys, state) = (&self.keys, Shown(self.state));
        match &self.action {
            KeyAction::Command {
                name,
                doc,
                owner,
                keymap,
            } => {
                writeln!(f, "{keys} in {state} runs {name}, found in {keymap}.")?;
                writeln!(f, "{name} is from {owner}.\n\n{doc}")?;
            }
            KeyAction::Prefix(keymap) => {
                writeln!(f, "{keys} in {state} starts longer bindings in {keymap}.")?;
            }
            KeyAction::LeavesPassthrough => writeln!(
                f,
                "{keys} in {state} goes to Normal state, before any keymap, and the surface doesn't get it."
            )?,
            KeyAction::Surface { kind, declared } => {
                let says = if *declared {
                    "handles it"
                } else {
                    "doesn't declare it"
                };
                writeln!(
                    f,
                    "{keys} in {state} goes to the {kind} surface, which {says}."
                )?;
                if self.state == State::Passthrough {
                    writeln!(
                        f,
                        "In insert state a surface gets every key but the DIE layer's, \
                         the leader's included. Keys go through the keymaps in normal state."
                    )?;
                }
            }
            KeyAction::Undefined => writeln!(f, "{keys} in {state} is undefined.")?,
        }
        if !self.shadowed.is_empty() {
            writeln!(f, "\nShadowed:")?;
        }
        for (map, s) in &self.shadowed {
            let how = match &s.by {
                Some(by) => format!("hidden by {by}"),
                None => "unbound".to_owned(),
            };
            writeln!(f, "  {} to {} in {map}: {how}", s.keys, s.cmd)?;
        }
        Ok(())
    }
}

impl fmt::Display for CommandDescription {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{} is from {}.\n\n{}\n", self.name, self.owner, self.doc)?;
        if self.keys.is_empty() {
            writeln!(f, "No key is bound to it.")?;
        }
        for (map, state, keys) in &self.keys {
            let on = if *state == State::Passthrough {
                " on a surface"
            } else {
                ""
            };
            writeln!(f, "{keys} in {}{on}, in {map}", Shown(*state))?;
        }
        Ok(())
    }
}
