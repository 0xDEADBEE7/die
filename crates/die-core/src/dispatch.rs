//! Commands and key dispatch (contract §5-§7): `Args`, the prefix argument, modal state, `run`,
//! `feed_keys` and the lookup order.
#![warn(missing_docs)]
use crate::app::Input;
use crate::error::*;
use crate::ext::*;
use crate::ids::*;
use crate::keymap::*;
use crate::keys::*;
use crate::rt::*;
use crate::text::*;
use std::rc::Rc;

// ------------------------------------------------------------------ commands and registry

/// What a command was invoked with (contract §6).
#[derive(Clone, Default, Debug)]
pub struct Args {
    /// The prefix argument's count, if set.
    pub count: Option<usize>,
    /// The prefix argument's register, if set.
    pub register: Option<char>,
    /// The rest of the line, when called as `:orchid-send hello`.
    pub text: Option<String>,
    /// The key sequence that invoked the command (Emacs's last-command-event).
    pub keys: Vec<KeyEvent>,
    /// The window it was invoked from, so async code doesn't depend on focus.
    pub window: Option<WindowId>,
    /// The buffer it was invoked on.
    pub buffer: Option<BufferId>,
    /// For a Hide's reveal command: the decoration being revealed.
    pub deco: Option<DecorationId>,
}

impl Args {
    /// The text typed by the last key, for `<char>` bindings.
    pub fn typed(&self) -> Option<String> {
        self.keys.last().and_then(KeyEvent::printable)
    }
    /// The first character the last key typed, for `<char>` bindings.
    pub fn char(&self) -> Option<char> {
        self.typed().and_then(|s| s.chars().next())
    }
}

/// A command's body, as `Registry::command` stores it.
pub type CommandFn = Rc<dyn Fn(&mut Ctx, &Args) -> Result>;

pub(crate) struct Command {
    pub(crate) doc: &'static str,
    pub(crate) owner: ExtIdx,
    pub(crate) f: CommandFn,
}

/// The prefix argument (contract §6). Core stores it and doesn't parse Vim syntax.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// The count.
    pub count: Option<usize>,
    /// The register.
    pub register: Option<char>,
}

/// Where a key came from, for key observers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// From the OS or `Ctx::input`.
    Typed,
    /// From `Ctx::feed_keys`.
    Fed,
}

impl Ctx {
    // -------------------------------------------------------------- modal state, prefix

    /// The focused buffer's modal state, or `Normal` with nothing focused. Modal state is per
    /// buffer: use `state_of` for a command's `Args` buffer, which may not be the focused one.
    pub fn state(&self) -> State {
        self.focused_buffer()
            .map_or(State::Normal, |b| self.state_of(b))
    }
    /// Set the focused buffer's modal state.
    pub fn set_state(&mut self, s: State) {
        if let Some(b) = self.focused_buffer() {
            self.set_state_of(b, s);
        }
    }
    /// `buf`'s modal state. A dead buffer reads as `Normal`.
    pub fn state_of(&self, buf: BufferId) -> State {
        self.buffers.get(buf).map_or(State::Normal, |b| b.state)
    }
    /// Set `buf`'s modal state. A dead buffer is ignored. A surface's buffer leaving
    /// passthrough takes copy state's snapshot of its text, and going back drops it (scope §5.5).
    pub fn set_state_of(&mut self, buf: BufferId, s: State) {
        if let Some(b) = self.buffers.get_mut(buf) {
            b.state = s;
        }
        self.copy_sync(buf, false);
    }
    /// The prefix argument as it stands.
    pub fn prefix(&self) -> Prefix {
        self.prefix
    }
    /// die-vim's digit and `"x` commands set this; the next command receives it in `Args` (D9).
    pub fn set_prefix(&mut self, p: Prefix) {
        self.prefix = p;
    }

    // -------------------------------------------------------------- commands

    /// Run a command now. The one explicit way to nest extension calls (C3). It acts for
    /// `args.window`, or with none, for whichever window its caller acts for (contract §9).
    pub fn run(&mut self, name: &str, args: &Args) -> Result {
        let (f, owner) = {
            let c = self
                .commands
                .get(name)
                .ok_or_else(|| Error::NoCommand(name.into()))?;
            (c.f.clone(), c.owner)
        };
        let outer = self.cur_window;
        self.cur_window = args.window.or(outer);
        let r = self.guarded(owner, |ctx| f(ctx, args));
        self.cur_window = outer;
        r
    }

    /// Run a Hide's reveal policy for every Hide covering `pos`. Returns whether `pos` is visible.
    pub fn reveal(&mut self, buf: BufferId, pos: usize) -> Result<bool> {
        let hides: Vec<(DecorationId, Reveal)> = self
            .text(buf)?
            .decorations_at(pos)
            .filter_map(|(id, d)| match &d.deco {
                Deco::Hide { reveal, .. } => Some((id, reveal.clone())),
                _ => None,
            })
            .collect();
        for (id, how) in hides {
            match how {
                Reveal::Never => {}
                Reveal::Remove => {
                    self.text_mut(buf)?.undecorate(id);
                }
                Reveal::Command(name) => {
                    let args = Args {
                        deco: Some(id),
                        buffer: Some(buf),
                        window: self.focused,
                        ..Default::default()
                    };
                    self.run(&name, &args)?;
                }
            }
        }
        Ok(self.text(buf)?.is_stop(pos))
    }

    // -------------------------------------------------------------- the key queues

    /// Harness: one key event from the OS.
    pub fn input(&mut self, ev: KeyEvent) {
        self.typed.push_back(Input::Key(ev));
    }
    /// Harness: type keys in notation, then run until idle.
    pub fn type_keys(&mut self, keys: &str) {
        for ev in typed(keys, Some(self.leader)).expect("bad key notation") {
            self.typed.push_back(Input::Key(ev));
        }
        self.run_until_idle();
    }

    // -------------------------------------------------------------- key dispatch (contract §7)

    pub(crate) fn dispatch(&mut self, ev: KeyEvent, src: KeySource) {
        self.cmd_seq += 1;
        if ev.kind != KeyKind::Release && !matches!(ev.key, Key::Modifier(_)) {
            self.sync_messages();
            self.log.echo = None;
        }
        // A key DIE took in passthrough swallows its own repeat and release.
        if let Some(i) = self.swallow.iter().position(|k| *k == ev.key) {
            match ev.kind {
                KeyKind::Repeat => return,
                KeyKind::Release => {
                    self.swallow.remove(i);
                    return;
                }
                KeyKind::Press => {
                    self.swallow.remove(i);
                }
            }
        }
        let Some(win) = self.focused else { return };
        let Some(buf) = self.window_buffer(win) else {
            return;
        };
        let Some(state) = self.buffers.get(buf).map(|b| b.state) else {
            return;
        };
        if state == State::Passthrough && self.describing.is_none() {
            return self.dispatch_passthrough(win, buf, ev);
        }
        if ev.kind == KeyKind::Release || matches!(ev.key, Key::Modifier(_) | Key::Unidentified) {
            return; // release events, lone modifier keys and unnamed keys never reach keymaps
        }
        self.copy_sync(buf, true);
        let mut caught = vec![];
        for e in &self.subs {
            if let SubKind::Keys(f) = &e.kind
                && e.inner.alive.get()
                && let Err(c) = catch(|| (f.borrow_mut())(&ev, src))
            {
                caught.push((e.inner.ext, c));
            }
        }
        for (ext, c) in caught {
            self.panicked(ext, c);
        }
        if self.describing.is_some() {
            return self.describe_key_read(ev);
        }
        if self.prompt.is_some() {
            // The DIE layer stays in reach: `<D-q>` isn't a key to type into a prompt.
            let bound = |(_, km): (String, &Keymap)| km.command_for_exactly(state, &ev);
            let layer = ev
                .mods
                .super_
                .then(|| self.layers(buf).into_iter().find_map(bound));
            if let Some(Some(name)) = layer {
                return self.run_key(&name, vec![ev], win, buf);
            }
            return self.prompt_key(ev);
        }
        self.pending.push(ev);
        match self.lookup(buf, state) {
            Lookup::Prefix => {}
            Lookup::Command(name) => {
                let keys = std::mem::take(&mut self.pending);
                self.run_key(&name, keys, win, buf);
            }
            Lookup::None => {
                // An undefined key ends the command it began, count and register too.
                self.prefix = Prefix::default();
                let keys = std::mem::take(&mut self.pending);
                let last = keys.last().cloned().unwrap();
                let handled = self.surface_event(buf, &Event::Key(last));
                if !matches!(handled, Ok(true)) {
                    self.drop_fed();
                }
                if !matches!(handled, Ok(true) | Err(Error::Panicked { .. })) && !self.hushed {
                    self.log_error(format!("{} is undefined", key_notation(&keys)));
                }
            }
        }
    }

    /// Passthrough is insert state on a surface (scope D18): the DIE layer's Esc leaves it for
    /// normal state, and so does a plain Esc the surface doesn't take now. Each swallows its
    /// repeats and release.
    fn dispatch_passthrough(&mut self, win: WindowId, buf: BufferId, ev: KeyEvent) {
        let plain = |ctx: &Ctx| ev.mods == Mods::default() && !ctx.surface_takes_esc(buf);
        if ev.key == Key::Esc && ev.kind == KeyKind::Press && (ev.mods.super_ || plain(self)) {
            self.set_state_of(buf, State::Normal);
            self.swallow.push(Key::Esc);
            return;
        }
        let named = !matches!(ev.key, Key::Modifier(_) | Key::Unidentified);
        if named
            && ev.mods.super_
            && ev.kind != KeyKind::Release
            && let Lookup::Command(name) =
                self.layers_lookup(buf, State::Passthrough, std::slice::from_ref(&ev))
        {
            self.swallow.push(ev.key);
            return self.run_key(&name, vec![ev], win, buf);
        }
        if let Err(Error::Panicked { .. }) = self.surface_event(buf, &Event::Key(ev)) {
            self.drop_fed();
        }
    }

    /// Give a surface input, guarded, for the extension that made it. A text buffer has no
    /// surface, so it handles nothing.
    pub(crate) fn surface_event(&mut self, buf: BufferId, ev: &Event) -> Result<bool> {
        let owner = match self.buffers.get(buf).map(|b| &b.content) {
            Some(Content::Custom { owner, .. }) => *owner,
            _ => return Ok(false),
        };
        self.guarded(owner, |ctx| {
            Ok(ctx.checkout_surface(buf, |s, ctx| s.event(ev, ctx))?)
        })
    }

    pub(crate) fn run_key(
        &mut self,
        name: &str,
        keys: Vec<KeyEvent>,
        win: WindowId,
        buf: BufferId,
    ) {
        let p = std::mem::take(&mut self.prefix);
        let args = Args {
            count: p.count,
            register: p.register,
            keys,
            window: Some(win),
            buffer: Some(buf),
            ..Default::default()
        };
        let r = self.run(name, &args);
        self.command_ended();
        if r.is_err() {
            self.drop_fed(); // a macro stops where it fails (contract §6)
        }
        self.report_key(r);
    }

    fn lookup(&self, buf: BufferId, state: State) -> Lookup {
        self.layers_lookup(buf, state, &self.pending)
    }

    /// Lookup order: range keymap at the cursor, buffer-local, modes (newest first), kind, global.
    pub(crate) fn layers_lookup(&self, buf: BufferId, state: State, keys: &[KeyEvent]) -> Lookup {
        for (_, km) in self.layers(buf) {
            match km.lookup(state, keys) {
                Lookup::None => continue,
                r => return r,
            }
        }
        Lookup::None
    }

    /// The keymaps a key in `buf` goes through, in lookup order, with their names.
    pub(crate) fn layers(&self, buf: BufferId) -> Vec<(String, &Keymap)> {
        let b = &self.buffers[buf];
        let mut layers: Vec<(String, &Keymap)> = vec![];
        let (kind, ranges): (&str, Vec<(String, &Keymap)>) = match &b.content {
            Content::Text(t) => {
                let cur = self.focused.and_then(|w| t.cursor(w));
                let names = cur.map(|c| {
                    t.decorations_at(c)
                        .filter_map(|(_, d)| match &d.deco {
                            Deco::Keymap(n) => Some((format!("keymap {n}"), self.named.get(n)?)),
                            _ => None,
                        })
                        .collect()
                });
                ("text", names.unwrap_or_default())
            }
            Content::Custom { kind, .. } => (kind, vec![]),
        };
        layers.extend(ranges);
        layers.push(("buffer-local".into(), &b.local));
        for m in b.modes.iter().rev() {
            let spec = &self.modes[m.spec];
            layers.push((format!("mode {}", spec.name), &spec.keymap));
        }
        layers.extend(self.kinds.get(kind).map(|m| (format!("kind {kind}"), m)));
        // Copy state is die-vim's grammar for text over the snapshot, under the kind's own keys.
        if kind != "text" && b.content.text().is_some() {
            layers.extend(self.kinds.get("text").map(|m| ("kind text".into(), m)));
        }
        layers.push(("global".into(), &self.global));
        layers
    }
}
