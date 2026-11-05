//! Commands and key dispatch (contract §5-§7): `Args`, the prefix argument, modal state, `run`,
//! `feed_keys` and the lookup order. The prompt is here until K11a moves it.
use crate::error::*;
use crate::ext::*;
use crate::ids::*;
use crate::keymap::*;
use crate::keys::*;
use crate::rt::*;
use crate::text::*;
use std::rc::Rc;

// ------------------------------------------------------------------ commands and registry

#[derive(Clone, Default, Debug)]
pub struct Args {
    pub count: Option<usize>,
    pub register: Option<char>,
    pub text: Option<String>,
    /// The key sequence that invoked the command (Emacs's last-command-event).
    pub keys: Vec<KeyEvent>,
    pub window: Option<WindowId>,
    pub buffer: Option<BufferId>,
    /// For a Hide's reveal command: the decoration being revealed.
    pub deco: Option<DecorationId>,
}

impl Args {
    /// The text typed by the last key, for `<char>` bindings.
    pub fn typed(&self) -> Option<String> {
        self.keys.last().and_then(KeyEvent::printable)
    }
    pub fn char(&self) -> Option<char> {
        self.typed().and_then(|s| s.chars().next())
    }
}

pub type CommandFn = Rc<dyn Fn(&mut Ctx, &Args) -> Result>;

pub(crate) struct Command {
    pub(crate) doc: &'static str,
    pub(crate) owner: ExtIdx,
    pub(crate) f: CommandFn,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Prefix {
    pub count: Option<usize>,
    pub register: Option<char>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Typed,
    Fed,
}

pub(crate) struct Prompt {
    label: String,
    text: String,
    ext: ExtIdx,
    prev: State,
    f: Box<dyn FnOnce(&mut Ctx, Option<String>) -> Result>,
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
    /// Set `buf`'s modal state. A dead buffer is ignored.
    pub fn set_state_of(&mut self, buf: BufferId, s: State) {
        if let Some(b) = self.buffers.get_mut(buf) {
            b.state = s;
        }
    }
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

    /// A command-line prompt. `f` runs from the top of the loop with the answer, or None on cancel.
    pub fn prompt(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Ctx, Option<String>) -> Result + 'static,
    ) {
        if self.prompt.is_some() {
            self.finish_prompt(None);
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
    pub fn prompt_text(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.text.as_str())
    }
    pub(crate) fn prompt_label(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.label.as_str())
    }
    fn finish_prompt(&mut self, answer: Option<String>) {
        let Some(p) = self.prompt.take() else { return };
        self.set_state(p.prev);
        let f = p.f;
        self.queue.push_back(Queued {
            ext: p.ext,
            alive: None,
            f: Box::new(move |ctx| f(ctx, answer)),
        });
    }

    // -------------------------------------------------------------- the key queues

    /// Queue keys as if typed (contract §6). die-vim's `@a` uses this. `keys` is notation,
    /// as `key_notation` writes it, so plain text with a `<` in it needs `<lt>`.
    pub fn feed_keys(&mut self, keys: &str) -> Result {
        let evs = typed(keys, Some(self.leader)).map_err(Error::Msg)?;
        self.fed.extend(evs);
        Ok(())
    }
    /// Harness: one key event from the OS.
    pub fn input(&mut self, ev: KeyEvent) {
        self.typed.push_back(ev);
    }
    /// Harness: type keys in notation, then run until idle.
    pub fn type_keys(&mut self, keys: &str) {
        for ev in typed(keys, Some(self.leader)).expect("bad key notation") {
            self.typed.push_back(ev);
        }
        self.run_until_idle();
    }

    // -------------------------------------------------------------- key dispatch (contract §7)

    pub(crate) fn dispatch(&mut self, ev: KeyEvent, src: KeySource) {
        self.cmd_seq += 1;
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
        if state == State::Passthrough {
            return self.dispatch_passthrough(win, buf, ev);
        }
        if ev.kind == KeyKind::Release || matches!(ev.key, Key::Modifier(_) | Key::Unidentified) {
            return; // release events, lone modifier keys and unnamed keys never reach keymaps
        }
        for e in &self.subs {
            if let SubKind::Keys(f) = &e.kind {
                if e.inner.alive.get() {
                    (f.borrow_mut())(&ev, src);
                }
            }
        }
        if self.prompt.is_some() {
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
                let handled = self
                    .checkout_surface(buf, |s, ctx| s.event(&Event::Key(last), ctx))
                    .unwrap_or(false);
                if !handled {
                    self.messages
                        .push(format!("{} is undefined", key_notation(&keys)));
                }
            }
        }
    }

    fn dispatch_passthrough(&mut self, win: WindowId, buf: BufferId, ev: KeyEvent) {
        if ev.key == Key::Esc && ev.mods.super_ && ev.kind == KeyKind::Press {
            self.buffers[buf].state = State::Normal;
            self.swallow.push(Key::Esc);
            return;
        }
        let named = !matches!(ev.key, Key::Modifier(_) | Key::Unidentified);
        if named && ev.mods.super_ && ev.kind != KeyKind::Release {
            if let Lookup::Command(name) =
                self.layers_lookup(buf, State::Passthrough, &[ev.clone()])
            {
                self.swallow.push(ev.key);
                return self.run_key(&name, vec![ev], win, buf);
            }
        }
        let _ = self.checkout_surface(buf, |s, ctx| s.event(&Event::Key(ev), ctx));
    }

    fn run_key(&mut self, name: &str, keys: Vec<KeyEvent>, win: WindowId, buf: BufferId) {
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
        self.report(r);
    }

    fn lookup(&self, buf: BufferId, state: State) -> Lookup {
        self.layers_lookup(buf, state, &self.pending)
    }

    /// Lookup order: range keymap at the cursor, buffer-local, modes (newest first), kind, global.
    fn layers_lookup(&self, buf: BufferId, state: State, keys: &[KeyEvent]) -> Lookup {
        let b = &self.buffers[buf];
        let mut layers: Vec<&Keymap> = vec![];
        let (kind, ranges): (&str, Vec<&Keymap>) = match &b.content {
            Content::Text(t) => {
                let cur = self.focused.and_then(|w| t.cursor(w));
                let names = cur.map(|c| {
                    t.decorations_at(c)
                        .filter_map(|(_, d)| match &d.deco {
                            Deco::Keymap(n) => self.named.get(n),
                            _ => None,
                        })
                        .collect()
                });
                ("text", names.unwrap_or_default())
            }
            Content::Custom { kind, .. } => (kind, vec![]),
        };
        layers.extend(ranges);
        layers.push(&b.local);
        layers.extend(b.modes.iter().rev().map(|m| &self.modes[m.spec].keymap));
        layers.extend(self.kinds.get(kind));
        layers.push(&self.global);
        for km in layers {
            match km.lookup(state, keys) {
                Lookup::None => continue,
                r => return r,
            }
        }
        Lookup::None
    }

    fn prompt_key(&mut self, ev: KeyEvent) {
        match ev.chord().key {
            Key::Enter => {
                let t = self.prompt.as_ref().map(|p| p.text.clone());
                self.finish_prompt(t);
            }
            Key::Esc => self.finish_prompt(None),
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
