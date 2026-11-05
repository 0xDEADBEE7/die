//! The extension machinery (contract §2, §4, §8, §10; C1, C2, C9): the traits extensions
//! implement, the registry they declare into, `Ctx` and its startup, checkout, modes and the guard
//! around every call into extension code. Buffers and windows are here until K6 moves them.
use crate::dispatch::*;
use crate::error::*;
use crate::faces::{Faces, Style};
use crate::ids::*;
use crate::keymap::*;
use crate::keys::*;
use crate::rt::*;
use crate::text::*;
use slotmap::SlotMap;
use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub type ExtIdx = usize;
pub(crate) const CORE: ExtIdx = usize::MAX;

// ------------------------------------------------------------------ traits

pub trait Extension: Any {
    fn name(&self) -> &'static str;
    /// Declarations only: commands, keymaps, modes. No editor access.
    fn register(&mut self, reg: &mut Registry);
    fn init(&mut self, _ctx: &mut Ctx) -> Result {
        Ok(())
    }
    fn shutdown(&mut self, _ctx: &mut Ctx) {}
}

/// Per-buffer mode state (text buffers only, C17). Type-level facts live in `ModeSpec`.
pub trait Mode: Any {
    /// Queued (C3).
    fn on_enable(&mut self, _buf: BufferId, _ctx: &mut Ctx) -> Result {
        Ok(())
    }
    /// Queued (C3). Not called when the buffer is killed; the mode is just dropped.
    fn on_disable(&mut self, _buf: BufferId, _ctx: &mut Ctx) {}
    /// Queued (C3): a window showing the buffer changed size.
    fn on_resize(&mut self, _win: WindowId, _cols: usize, _rows: usize, _ctx: &mut Ctx) {}
    /// Render-time face provider (C16): faces only, no Ctx.
    fn decorate(
        &mut self,
        _text: &TextBuf,
        _visible: Range<usize>,
        _out: &mut Vec<(Range<usize>, &'static str)>,
    ) {
    }
    fn save(&self) -> Option<String> {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Key(KeyEvent),
    Resize { cols: usize, rows: usize },
    Focus(bool),
}

pub trait Surface: Any {
    fn kind(&self) -> &'static str;
    fn render(&mut self, cols: usize, rows: usize, out: &mut Vec<String>);
    /// Input keymaps didn't take; every key (and release) in passthrough. Checked out (C2).
    fn event(&mut self, _ev: &Event, _ctx: &mut Ctx) -> bool {
        false
    }
    fn initial_state(&self) -> State {
        State::Normal
    }
}

pub struct ModeSpec {
    pub name: &'static str,
    type_id: TypeId,
    pub(crate) owner: ExtIdx,
    pub initial_state: Option<State>,
    pub keymap: Keymap,
    new: Rc<dyn Fn() -> Box<dyn Mode>>,
}

impl ModeSpec {
    pub fn bind(&mut self, state: State, keys: &str, cmd: &str) -> &mut Self {
        self.keymap.bind(state, keys, cmd);
        self
    }
    pub fn initial_state(&mut self, s: State) -> &mut Self {
        self.initial_state = Some(s);
        self
    }
}

/// Declarations made in `Extension::register`.
#[derive(Default)]
pub struct Registry {
    cur: ExtIdx,
    commands: Vec<(String, Command)>,
    global: Keymap,
    kinds: HashMap<&'static str, Keymap>,
    modes: Vec<ModeSpec>,
    named: HashMap<String, Keymap>,
    leader: Option<String>,
    faces: Vec<(String, Option<String>, Style)>,
}

impl Registry {
    /// Declare a face with its default style and an optional parent face
    /// (contract §13). A theme can restyle it by name.
    pub fn face(&mut self, name: &str, parent: Option<&str>, style: Style) -> &mut Self {
        self.faces
            .push((name.into(), parent.map(String::from), style));
        self
    }
    pub fn command(
        &mut self,
        name: &str,
        doc: &'static str,
        f: impl Fn(&mut Ctx, &Args) -> Result + 'static,
    ) {
        self.commands.push((
            name.into(),
            Command {
                doc,
                owner: self.cur,
                f: Rc::new(f),
            },
        ));
    }
    /// A command implemented as a method on the extension struct, reached by checkout.
    pub fn ext_command<E: Extension>(
        &mut self,
        name: &str,
        doc: &'static str,
        f: fn(&mut E, &mut Ctx, &Args) -> Result,
    ) {
        self.command(name, doc, move |ctx, a| {
            ctx.with_ext(|e: &mut E, ctx| f(e, ctx, a))?
        });
    }
    pub fn mode<M: Mode>(
        &mut self,
        name: &'static str,
        new: impl Fn() -> M + 'static,
    ) -> &mut ModeSpec {
        self.modes.push(ModeSpec {
            name,
            type_id: TypeId::of::<M>(),
            owner: self.cur,
            initial_state: None,
            keymap: Keymap::default(),
            new: Rc::new(move || Box::new(new())),
        });
        self.modes.last_mut().unwrap()
    }
    pub fn global_keymap(&mut self) -> &mut Keymap {
        &mut self.global
    }
    /// The keymap for a surface kind ("text", "term"), like a major-mode map.
    pub fn kind_keymap(&mut self, kind: &'static str) -> &mut Keymap {
        self.kinds.entry(kind).or_default()
    }
    /// A named keymap, for keymap decorations on ranges.
    pub fn keymap(&mut self, name: &str) -> &mut Keymap {
        self.named.entry(name.into()).or_default()
    }
    /// Set the key `<leader>` stands for, in notation (`\`, `<Space>`, `,`). It applies to
    /// every keymap once all extensions have registered, so your config, registered last,
    /// decides it. The default is `\`. Anything but one key is a startup error.
    pub fn set_leader(&mut self, key: &str) {
        self.leader = Some(key.into());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegKind {
    Char,
    Line,
    Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Register {
    pub kind: RegKind,
    pub text: String,
}

pub(crate) struct ExtSlot {
    name: &'static str,
    type_id: TypeId,
    obj: Option<Box<dyn Extension>>,
    panics: u32,
}

pub(crate) enum Content {
    Text(Box<TextBuf>),
    Custom {
        type_id: TypeId,
        kind: &'static str,
        obj: Option<Box<dyn Surface>>,
    },
}

pub(crate) struct ModeSlot {
    pub(crate) type_id: TypeId,
    pub(crate) spec: usize,
    pub(crate) obj: Option<Box<dyn Mode>>,
}

pub(crate) struct Buffer {
    pub name: String,
    pub content: Content,
    pub modes: Vec<ModeSlot>,
    pub local: Keymap,
    pub state: State,
}

pub(crate) struct Window {
    pub buffer: BufferId,
    pub cols: usize,
    pub rows: usize,
}

pub struct Ctx {
    pub(crate) exts: Vec<ExtSlot>,
    pub(crate) commands: HashMap<String, Command>,
    pub(crate) global: Keymap,
    pub(crate) kinds: HashMap<&'static str, Keymap>,
    pub(crate) modes: Vec<ModeSpec>,
    pub(crate) named: HashMap<String, Keymap>,
    pub(crate) buffers: SlotMap<BufferId, Buffer>,
    pub(crate) windows: SlotMap<WindowId, Window>,
    pub(crate) focused: Option<WindowId>,
    pub(crate) ext_stack: Vec<ExtIdx>,
    pub(crate) queue: VecDeque<Queued>,
    pub(crate) subs: Vec<SubEntry>,
    pub(crate) next_id: u64,
    pub(crate) jobs_tx: mpsc::Sender<(u64, Box<dyn Any + Send>)>,
    pub(crate) jobs_rx: mpsc::Receiver<(u64, Box<dyn Any + Send>)>,
    pub(crate) outstanding: usize,
    pub(crate) typed: VecDeque<KeyEvent>,
    pub(crate) fed: VecDeque<KeyEvent>,
    pub(crate) pending: Vec<KeyEvent>,
    pub(crate) swallow: Vec<Key>,
    pub(crate) leader: Chord,
    pub(crate) prefix: Prefix,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) clock: Duration,
    pub(crate) cmd_seq: u64,
    /// The window the running command acts for. With none, the focused window acts.
    pub(crate) cur_window: Option<WindowId>,
    pub(crate) registers: HashMap<char, Register>,
    pub(crate) files: crate::files::Files,
    pub messages: Vec<String>,
    pub(crate) faces: Faces,
    pub(crate) search_highlight: Option<Pattern>,
}

pub(crate) fn panic_msg(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic>".into())
}

impl Ctx {
    /// Register extensions in order, apply bindings, check every bound name exists (C8), then init.
    pub fn new(exts: Vec<Box<dyn Extension>>) -> std::result::Result<Ctx, Vec<String>> {
        let mut reg = Registry::default();
        let mut errors = vec![];
        let mut slots: Vec<ExtSlot> = vec![];
        for mut e in exts {
            let type_id = (*e).type_id();
            if slots.iter().any(|s| s.type_id == type_id) {
                errors.push(format!("extension {} registered twice", e.name()));
                continue;
            }
            reg.cur = slots.len();
            if catch_unwind(AssertUnwindSafe(|| e.register(&mut reg))).is_err() {
                errors.push(format!("{} panicked in register", e.name()));
            }
            slots.push(ExtSlot {
                name: e.name(),
                type_id,
                obj: Some(e),
                panics: 0,
            });
        }
        let faces = Faces::new(std::mem::take(&mut reg.faces)).unwrap_or_else(|e| {
            errors.extend(e);
            Faces::new(vec![]).expect("core's own faces are valid")
        });
        let (jobs_tx, jobs_rx) = mpsc::channel();
        let mut ctx = Ctx {
            exts: slots,
            commands: HashMap::new(),
            global: reg.global,
            kinds: reg.kinds,
            modes: reg.modes,
            named: reg.named,
            buffers: SlotMap::with_key(),
            windows: SlotMap::with_key(),
            focused: None,
            ext_stack: vec![],
            queue: VecDeque::new(),
            subs: vec![],
            next_id: 0,
            jobs_tx,
            jobs_rx,
            outstanding: 0,
            typed: VecDeque::new(),
            fed: VecDeque::new(),
            pending: vec![],
            swallow: vec![],
            leader: default_leader(),
            prefix: Prefix::default(),
            prompt: None,
            clock: Duration::ZERO,
            cmd_seq: 0,
            cur_window: None,
            registers: HashMap::new(),
            files: Default::default(),
            messages: vec![],
            faces,
            search_highlight: None,
        };
        for (name, c) in reg.commands {
            if ctx.commands.insert(name.clone(), c).is_some() {
                errors.push(format!("command {name} registered twice"));
            }
        }
        if let Some(k) = &reg.leader {
            match typed(k, None).as_deref() {
                Ok([ev]) if !matches!(ev.key, Key::Modifier(_)) => ctx.set_leader(ev.chord()),
                _ => errors.push(format!("the leader must be one key, not a modifier: {k:?}")),
            }
        }
        errors.extend(ctx.check_bindings());
        if !errors.is_empty() {
            return Err(errors);
        }
        for i in 0..ctx.exts.len() {
            let r = ctx.guarded(i, |ctx| ctx.with_ext_idx(i, |e, ctx| e.init(ctx))?);
            ctx.report(r);
        }
        Ok(ctx)
    }

    fn set_leader(&mut self, leader: Chord) {
        self.leader = leader;
        let maps = std::iter::once(&mut self.global)
            .chain(self.kinds.values_mut())
            .chain(self.modes.iter_mut().map(|m| &mut m.keymap))
            .chain(self.named.values_mut());
        for m in maps {
            m.set_leader(leader);
        }
    }

    /// The key `<leader>` stands for (`Registry::set_leader`).
    pub fn leader(&self) -> Chord {
        self.leader
    }

    fn check_bindings(&self) -> Vec<String> {
        let mut maps: Vec<(String, &Keymap)> = vec![("global".into(), &self.global)];
        maps.extend(self.kinds.iter().map(|(k, m)| (format!("kind {k}"), m)));
        maps.extend(
            self.modes
                .iter()
                .map(|m| (format!("mode {}", m.name), &m.keymap)),
        );
        maps.extend(self.named.iter().map(|(k, m)| (format!("keymap {k}"), m)));
        let mut out = vec![];
        for (name, m) in maps {
            out.extend(m.errors.iter().map(|e| format!("{name}: {e}")));
            for (_, keys, cmd) in m.bindings() {
                if !self.commands.contains_key(cmd) {
                    out.push(format!("{name}: {keys} is bound to missing command {cmd}"));
                }
            }
        }
        out.sort();
        out
    }

    pub(crate) fn ext_name(&self, i: ExtIdx) -> &'static str {
        self.exts.get(i).map_or("core", |s| s.name)
    }
    pub(crate) fn cur_ext(&self) -> ExtIdx {
        self.ext_stack.last().copied().unwrap_or(CORE)
    }
    /// How many extension calls are on the stack. Queued calls run at depth 1 (C3).
    pub fn nesting(&self) -> usize {
        self.ext_stack.len()
    }
    pub fn command_doc(&self, name: &str) -> Option<(&'static str, &'static str)> {
        self.commands
            .get(name)
            .map(|c| (c.doc, self.ext_name(c.owner)))
    }

    /// The one place core calls extension code: attributes panics (C9).
    pub(crate) fn guarded<R>(
        &mut self,
        ext: ExtIdx,
        f: impl FnOnce(&mut Ctx) -> Result<R>,
    ) -> Result<R> {
        self.ext_stack.push(ext);
        let r = catch_unwind(AssertUnwindSafe(|| f(self)));
        self.ext_stack.pop();
        r.unwrap_or_else(|p| {
            let (name, msg) = (self.ext_name(ext), panic_msg(&*p));
            if let Some(s) = self.exts.get_mut(ext) {
                s.panics += 1;
            }
            Err(Error::Panicked { ext: name, msg })
        })
    }
    pub(crate) fn report(&mut self, r: Result) {
        if let Err(e) = r {
            self.messages.push(format!("{e:?}"));
        }
    }

    // -------------------------------------------------------------- C2 checkout

    pub fn with_ext<E: Extension, R>(
        &mut self,
        f: impl FnOnce(&mut E, &mut Ctx) -> R,
    ) -> Result<R, Checkout> {
        let i = self
            .exts
            .iter()
            .position(|s| s.type_id == TypeId::of::<E>())
            .ok_or(Checkout::Missing)?;
        self.with_ext_idx(i, |e, ctx| {
            f(
                (e as &mut dyn Any)
                    .downcast_mut::<E>()
                    .expect("type id matched"),
                ctx,
            )
        })
    }
    pub(crate) fn with_ext_idx<R>(
        &mut self,
        i: ExtIdx,
        f: impl FnOnce(&mut dyn Extension, &mut Ctx) -> R,
    ) -> Result<R, Checkout> {
        let mut obj = self
            .exts
            .get_mut(i)
            .ok_or(Checkout::Missing)?
            .obj
            .take()
            .ok_or(Checkout::Busy)?;
        // Catch so the object always goes back, then keep unwinding to the outer guard.
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut *obj, self)));
        self.exts[i].obj = Some(obj);
        Ok(r.unwrap_or_else(|p| resume_unwind(p)))
    }

    pub fn with_mode<M: Mode, R>(
        &mut self,
        buf: BufferId,
        f: impl FnOnce(&mut M, &mut Ctx) -> R,
    ) -> Result<R, Checkout> {
        self.checkout_mode(buf, TypeId::of::<M>(), |m, ctx| {
            f((m as &mut dyn Any).downcast_mut::<M>().unwrap(), ctx)
        })
    }
    pub(crate) fn checkout_mode<R>(
        &mut self,
        buf: BufferId,
        tid: TypeId,
        f: impl FnOnce(&mut dyn Mode, &mut Ctx) -> R,
    ) -> Result<R, Checkout> {
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        let slot = b
            .modes
            .iter_mut()
            .find(|m| m.type_id == tid)
            .ok_or(Checkout::Missing)?;
        let mut obj = slot.obj.take().ok_or(Checkout::Busy)?;
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut *obj, self)));
        // Check-in. If the buffer was killed or the mode disabled meanwhile, the object drops here.
        let slot = self.buffers.get_mut(buf).and_then(|b| {
            b.modes
                .iter_mut()
                .find(|m| m.type_id == tid && m.obj.is_none())
        });
        match slot {
            Some(s) => s.obj = Some(obj),
            None => drop(obj),
        }
        Ok(r.unwrap_or_else(|p| resume_unwind(p)))
    }

    pub fn with_surface<S: Surface, R>(
        &mut self,
        buf: BufferId,
        f: impl FnOnce(&mut S, &mut Ctx) -> R,
    ) -> Result<R, Checkout> {
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        match &b.content {
            Content::Custom { type_id, .. } if *type_id == TypeId::of::<S>() => {}
            _ => return Err(Checkout::Missing),
        }
        self.checkout_surface(buf, |s, ctx| {
            f((s as &mut dyn Any).downcast_mut::<S>().unwrap(), ctx)
        })
    }
    pub(crate) fn checkout_surface<R>(
        &mut self,
        buf: BufferId,
        f: impl FnOnce(&mut dyn Surface, &mut Ctx) -> R,
    ) -> Result<R, Checkout> {
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        let Content::Custom { obj, .. } = &mut b.content else {
            return Err(Checkout::Missing);
        };
        let mut o = obj.take().ok_or(Checkout::Busy)?;
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut *o, self)));
        if let Some(Buffer {
            content: Content::Custom { obj, .. },
            ..
        }) = self.buffers.get_mut(buf)
        {
            *obj = Some(o);
        }
        Ok(r.unwrap_or_else(|p| resume_unwind(p)))
    }

    // -------------------------------------------------------------- buffers and windows

    fn unique_name(&self, name: &str) -> String {
        let taken = |n: &str| self.buffers.values().any(|b| b.name == n);
        if !taken(name) {
            return name.into();
        }
        (2..)
            .map(|i| format!("{name}<{i}>"))
            .find(|n| !taken(n))
            .unwrap()
    }
    pub fn create_text(&mut self, name: &str, text: &str) -> BufferId {
        let name = self.unique_name(name);
        let b = Buffer {
            name,
            content: Content::Text(Box::new(TextBuf::new(text))),
            modes: vec![],
            local: Keymap::with_leader(self.leader),
            state: State::Normal,
        };
        self.buffers.insert(b)
    }
    pub fn create_surface<S: Surface>(&mut self, name: &str, s: S) -> BufferId {
        let name = self.unique_name(name);
        let (kind, state) = (s.kind(), s.initial_state());
        let content = Content::Custom {
            type_id: TypeId::of::<S>(),
            kind,
            obj: Some(Box::new(s)),
        };
        self.buffers.insert(Buffer {
            name,
            content,
            modes: vec![],
            local: Keymap::with_leader(self.leader),
            state,
        })
    }
    pub fn find_buffer(&self, name: &str) -> Option<BufferId> {
        self.buffers
            .iter()
            .find(|(_, b)| b.name == name)
            .map(|(id, _)| id)
    }
    pub fn buffer_name(&self, buf: BufferId) -> Option<&str> {
        self.buffers.get(buf).map(|b| b.name.as_str())
    }
    pub fn is_live(&self, buf: BufferId) -> bool {
        self.buffers.contains_key(buf)
    }
    /// Kill a buffer. It is dead at once; a checked-out surface or mode drops at check-in.
    /// Dropping its modes drops the `Sub` handles they hold, which cancels them (C4).
    pub fn kill(&mut self, buf: BufferId) -> bool {
        let Some(b) = self.buffers.remove(buf) else {
            return false;
        };
        drop(b);
        self.files.forget(buf);
        let hooks: Vec<_> = self
            .subs
            .iter()
            .filter_map(|e| match &e.kind {
                SubKind::Killed(f) if e.inner.alive.get() => Some((e.inner.clone(), f.clone())),
                _ => None,
            })
            .collect();
        for (inner, f) in hooks {
            let ext = inner.ext;
            self.queue.push_back(Queued {
                ext,
                alive: Some(inner),
                f: Box::new(move |ctx| (f.borrow_mut())(ctx, buf)),
            });
        }
        true
    }

    pub fn text(&self, buf: BufferId) -> Result<&TextBuf> {
        match &self.buffers.get(buf).ok_or(Checkout::Gone)?.content {
            Content::Text(t) => Ok(t),
            _ => Err(Error::NotText),
        }
    }
    /// Core-owned text is borrowed by id, never checked out (C2).
    pub fn text_mut(&mut self, buf: BufferId) -> Result<&mut TextBuf> {
        let actor = self
            .cur_window
            .or(self.focused)
            .filter(|w| self.windows.get(*w).is_some_and(|w| w.buffer == buf));
        let cmd = self.cmd_seq;
        match &mut self.buffers.get_mut(buf).ok_or(Checkout::Gone)?.content {
            Content::Text(t) => {
                t.cmd = cmd;
                t.actor = actor;
                Ok(t)
            }
            _ => Err(Error::NotText),
        }
    }

    pub fn open_window(&mut self, buf: BufferId, cols: usize, rows: usize) -> Result<WindowId> {
        let b = self.buffers.get(buf).ok_or(Checkout::Gone)?;
        let _ = b;
        let w = self.windows.insert(Window {
            buffer: buf,
            cols,
            rows,
        });
        if let Content::Text(t) = &mut self.buffers[buf].content {
            t.add_view(w, cols, rows);
        }
        self.focus(w);
        Ok(w)
    }
    pub fn focus(&mut self, w: WindowId) {
        if self.windows.contains_key(w) {
            self.focused = Some(w);
            // A running command goes on acting for the window it moved the focus to.
            self.cur_window = self.cur_window.and(Some(w));
        }
    }
    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }
    pub fn focused_buffer(&self) -> Option<BufferId> {
        self.focused
            .and_then(|w| self.windows.get(w))
            .map(|w| w.buffer)
    }
    pub fn window_buffer(&self, w: WindowId) -> Option<BufferId> {
        self.windows.get(w).map(|w| w.buffer)
    }
    pub fn window_size(&self, w: WindowId) -> Option<(usize, usize)> {
        self.windows.get(w).map(|w| (w.cols, w.rows))
    }
    /// Resize a window. Each mode's `on_resize` and the surface's Resize event are queued (C3).
    pub fn resize(&mut self, w: WindowId, cols: usize, rows: usize) -> Result {
        let win = self.windows.get_mut(w).ok_or(Checkout::Gone)?;
        win.cols = cols;
        win.rows = rows;
        let buf = win.buffer;
        let Some(b) = self.buffers.get_mut(buf) else {
            return Ok(());
        };
        if let Content::Text(t) = &mut b.content {
            t.resize_view(w, cols, rows);
        } else {
            self.defer_as(CORE, move |ctx| {
                ctx.checkout_surface(buf, |s, ctx| s.event(&Event::Resize { cols, rows }, ctx))?;
                Ok(())
            });
        }
        let modes: Vec<(TypeId, ExtIdx)> = self.buffers[buf]
            .modes
            .iter()
            .map(|m| (m.type_id, self.modes[m.spec].owner))
            .collect();
        for (tid, owner) in modes {
            self.defer_as(owner, move |ctx| {
                ctx.checkout_mode(buf, tid, |m, ctx| m.on_resize(w, cols, rows, ctx))?;
                Ok(())
            });
        }
        Ok(())
    }

    // -------------------------------------------------------------- modes

    pub fn enable_mode<M: Mode>(&mut self, buf: BufferId) -> Result {
        let tid = TypeId::of::<M>();
        let spec = self
            .modes
            .iter()
            .position(|s| s.type_id == tid)
            .ok_or_else(|| Error::Msg("mode not registered".into()))?;
        let b = self.buffers.get(buf).ok_or(Checkout::Gone)?;
        if !matches!(b.content, Content::Text(_)) {
            return Err(Error::NotText); // C17
        }
        if b.modes.iter().any(|m| m.type_id == tid) {
            return Ok(());
        }
        let (owner, new, init) = (
            self.modes[spec].owner,
            self.modes[spec].new.clone(),
            self.modes[spec].initial_state,
        );
        let obj = self.guarded(owner, |_| Ok(new()))?;
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        b.modes.push(ModeSlot {
            type_id: tid,
            spec,
            obj: Some(obj),
        });
        if let Some(s) = init {
            b.state = s;
        }
        self.defer_as(owner, move |ctx| {
            ctx.checkout_mode(buf, tid, |m, ctx| m.on_enable(buf, ctx))?
        });
        Ok(())
    }
    pub fn disable_mode<M: Mode>(&mut self, buf: BufferId) -> Result {
        let tid = TypeId::of::<M>();
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        let i = b
            .modes
            .iter()
            .position(|m| m.type_id == tid)
            .ok_or(Checkout::Missing)?;
        let slot = b.modes.remove(i);
        if let Some(mut obj) = slot.obj {
            let owner = self.modes[slot.spec].owner;
            self.defer_as(owner, move |ctx| {
                obj.on_disable(buf, ctx);
                Ok(())
            });
        }
        Ok(())
    }

    // -------------------------------------------------------------- registers

    pub fn register(&self, c: char) -> Option<&Register> {
        self.registers.get(&c)
    }
    pub fn set_register(&mut self, c: char, r: Register) {
        self.registers.insert(c, r);
    }
}
