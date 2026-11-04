//! The workspace and `Ctx` (C1): extensions, buffers, windows, checkout (C2), queued calls (C3),
//! subscription handles (C4), commands, keymap dispatch, the prefix argument, `feed_keys`,
//! key observers and prompts.
use crate::error::*;
use crate::faces::{Faces, Style};
use crate::ids::*;
use crate::keymap::*;
use crate::keys::*;
use crate::text::*;
use slotmap::SlotMap;
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub type ExtIdx = usize;
const CORE: ExtIdx = usize::MAX;

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

struct Command {
    doc: &'static str,
    owner: ExtIdx,
    f: CommandFn,
}

pub struct ModeSpec {
    pub name: &'static str,
    type_id: TypeId,
    owner: ExtIdx,
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

// ------------------------------------------------------------------ subscriptions (C4)

struct SubInner {
    alive: Cell<bool>,
    detached: Cell<bool>,
    ext: ExtIdx,
}

/// A subscription handle. Dropping it cancels the subscription; `detach` ties it to the extension.
#[must_use = "dropping a Sub cancels it; call .detach() to keep it for the extension's lifetime"]
pub struct Sub(Rc<SubInner>);

impl Sub {
    pub fn detach(self) {
        self.0.detached.set(true);
    }
    pub fn is_active(&self) -> bool {
        self.0.alive.get()
    }
}

impl Drop for Sub {
    fn drop(&mut self) {
        if !self.0.detached.get() {
            self.0.alive.set(false);
        }
    }
}

type Callback = Box<dyn FnOnce(&mut Ctx) -> Result>;
type JobDone = Box<dyn FnOnce(&mut Ctx, Box<dyn Any + Send>) -> Result>;

enum SubKind {
    Timer {
        at: Cell<Duration>,
        every: Option<Duration>,
        fired: Cell<bool>,
        f: Rc<RefCell<dyn FnMut(&mut Ctx) -> Result>>,
    },
    Killed(Rc<RefCell<dyn FnMut(&mut Ctx, BufferId) -> Result>>),
    Job {
        id: u64,
        f: RefCell<Option<JobDone>>,
    },
    Keys(RefCell<Box<dyn FnMut(&KeyEvent, KeySource)>>),
}

struct SubEntry {
    inner: Rc<SubInner>,
    kind: SubKind,
}

/// A call that receives Ctx, waiting for the top of the loop (C3).
struct Queued {
    ext: ExtIdx,
    alive: Option<Rc<SubInner>>,
    f: Callback,
}

// ------------------------------------------------------------------ workspace

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

struct ExtSlot {
    name: &'static str,
    type_id: TypeId,
    obj: Option<Box<dyn Extension>>,
    panics: u32,
}

pub(crate) enum Content {
    Text(TextBuf),
    Custom {
        type_id: TypeId,
        kind: &'static str,
        obj: Option<Box<dyn Surface>>,
    },
}

pub(crate) struct ModeSlot {
    type_id: TypeId,
    spec: usize,
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

struct Prompt {
    label: String,
    text: String,
    ext: ExtIdx,
    prev: State,
    f: Box<dyn FnOnce(&mut Ctx, Option<String>) -> Result>,
}

pub struct Ctx {
    exts: Vec<ExtSlot>,
    commands: HashMap<String, Command>,
    global: Keymap,
    kinds: HashMap<&'static str, Keymap>,
    modes: Vec<ModeSpec>,
    named: HashMap<String, Keymap>,
    pub(crate) buffers: SlotMap<BufferId, Buffer>,
    pub(crate) windows: SlotMap<WindowId, Window>,
    focused: Option<WindowId>,
    ext_stack: Vec<ExtIdx>,
    queue: VecDeque<Queued>,
    subs: Vec<SubEntry>,
    next_id: u64,
    jobs_tx: mpsc::Sender<(u64, Box<dyn Any + Send>)>,
    jobs_rx: mpsc::Receiver<(u64, Box<dyn Any + Send>)>,
    outstanding: usize,
    typed: VecDeque<KeyEvent>,
    fed: VecDeque<KeyEvent>,
    pending: Vec<KeyEvent>,
    swallow: Vec<Key>,
    leader: Chord,
    prefix: Prefix,
    prompt: Option<Prompt>,
    clock: Duration,
    cmd_seq: u64,
    cur_window: Option<WindowId>,
    registers: HashMap<char, Register>,
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

    fn ext_name(&self, i: ExtIdx) -> &'static str {
        self.exts.get(i).map_or("core", |s| s.name)
    }
    fn cur_ext(&self) -> ExtIdx {
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
    fn guarded<R>(&mut self, ext: ExtIdx, f: impl FnOnce(&mut Ctx) -> Result<R>) -> Result<R> {
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
    fn report(&mut self, r: Result) {
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
    fn with_ext_idx<R>(
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
    fn checkout_mode<R>(
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
    fn checkout_surface<R>(
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
            content: Content::Text(TextBuf::new(text)),
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
        self.focused = Some(w);
        self.cur_window = Some(w);
        Ok(w)
    }
    pub fn focus(&mut self, w: WindowId) {
        if self.windows.contains_key(w) {
            self.focused = Some(w);
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

    // -------------------------------------------------------------- modal state, prefix, registers

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
    pub fn register(&self, c: char) -> Option<&Register> {
        self.registers.get(&c)
    }
    pub fn set_register(&mut self, c: char, r: Register) {
        self.registers.insert(c, r);
    }

    // -------------------------------------------------------------- commands

    /// Run a command now. The one explicit way to nest extension calls (C3).
    pub fn run(&mut self, name: &str, args: &Args) -> Result {
        let (f, owner) = {
            let c = self
                .commands
                .get(name)
                .ok_or_else(|| Error::NoCommand(name.into()))?;
            (c.f.clone(), c.owner)
        };
        self.guarded(owner, |ctx| f(ctx, args))
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

    // -------------------------------------------------------------- async (C3, C4)

    fn defer_as(&mut self, ext: ExtIdx, f: impl FnOnce(&mut Ctx) -> Result + 'static) {
        self.queue.push_back(Queued {
            ext,
            alive: None,
            f: Box::new(f),
        });
    }
    /// Queue a call for the top of the loop.
    pub fn defer(&mut self, f: impl FnOnce(&mut Ctx) -> Result + 'static) {
        self.defer_as(self.cur_ext(), f);
    }
    fn new_sub(&mut self, kind: SubKind) -> Sub {
        let inner = Rc::new(SubInner {
            alive: Cell::new(true),
            detached: Cell::new(false),
            ext: self.cur_ext(),
        });
        self.subs.push(SubEntry {
            inner: inner.clone(),
            kind,
        });
        Sub(inner)
    }
    pub fn after(&mut self, d: Duration, f: impl FnOnce(&mut Ctx) -> Result + 'static) -> Sub {
        let mut f = Some(f);
        self.timer(d, None, move |ctx| f.take().map_or(Ok(()), |f| f(ctx)))
    }
    pub fn every(&mut self, d: Duration, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        self.timer(d, Some(d), f)
    }
    fn timer(
        &mut self,
        d: Duration,
        every: Option<Duration>,
        f: impl FnMut(&mut Ctx) -> Result + 'static,
    ) -> Sub {
        let at = Cell::new(self.clock + d);
        self.new_sub(SubKind::Timer {
            at,
            every,
            fired: Cell::new(false),
            f: Rc::new(RefCell::new(f)),
        })
    }
    pub fn on_buffer_killed(
        &mut self,
        f: impl FnMut(&mut Ctx, BufferId) -> Result + 'static,
    ) -> Sub {
        self.new_sub(SubKind::Killed(Rc::new(RefCell::new(f))))
    }
    /// `work` runs on a thread; `done` runs on the main thread from the top of the loop.
    pub fn spawn<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Ctx, T) -> Result + 'static,
    ) -> Sub {
        self.next_id += 1;
        let id = self.next_id;
        let f: JobDone = Box::new(move |ctx, v| done(ctx, *v.downcast::<T>().expect("job type")));
        let sub = self.new_sub(SubKind::Job {
            id,
            f: RefCell::new(Some(f)),
        });
        let tx = self.jobs_tx.clone();
        self.outstanding += 1;
        std::thread::spawn(move || {
            let v = work();
            let _ = tx.send((id, Box::new(v)));
        });
        sub
    }
    /// See every key before dispatch. No Ctx, so it runs synchronously (C3). For `q` recording.
    pub fn observe_keys(&mut self, f: impl FnMut(&KeyEvent, KeySource) + 'static) -> Sub {
        self.new_sub(SubKind::Keys(RefCell::new(Box::new(f))))
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

    // -------------------------------------------------------------- the loop

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
    /// Harness: advance the fake clock, then run until idle.
    pub fn advance(&mut self, d: Duration) {
        self.clock += d;
        self.run_until_idle();
    }

    fn collect_due(&mut self) {
        while let Ok((id, v)) = self.jobs_rx.try_recv() {
            self.outstanding -= 1;
            let mut v = Some(v);
            for e in &self.subs {
                if let SubKind::Job { id: jid, f } = &e.kind {
                    if *jid == id && e.inner.alive.get() {
                        if let Some(f) = f.borrow_mut().take() {
                            let v = v.take().unwrap();
                            let ext = e.inner.ext;
                            self.queue.push_back(Queued {
                                ext,
                                alive: Some(e.inner.clone()),
                                f: Box::new(move |ctx| f(ctx, v)),
                            });
                        }
                    }
                }
            }
        }
        for e in &self.subs {
            if let SubKind::Timer {
                at,
                every,
                fired,
                f,
            } = &e.kind
            {
                while e.inner.alive.get() && !fired.get() && at.get() <= self.clock {
                    let f = f.clone();
                    let inner = e.inner.clone();
                    self.queue.push_back(Queued {
                        ext: inner.ext,
                        alive: Some(inner),
                        f: Box::new(move |ctx| (f.borrow_mut())(ctx)),
                    });
                    match every {
                        Some(d) => at.set(at.get() + *d),
                        None => fired.set(true),
                    }
                }
            }
        }
        self.subs.retain(|e| {
            e.inner.alive.get() && !matches!(&e.kind, SubKind::Timer { fired, .. } if fired.get())
        });
    }

    /// One turn of the loop: queued calls first (C3), then one key (fed keys before typed ones).
    pub fn step(&mut self) -> bool {
        self.collect_due();
        if let Some(q) = self.queue.pop_front() {
            if q.alive.as_ref().is_none_or(|a| a.alive.get()) {
                self.cmd_seq += 1;
                self.cur_window = self.focused;
                let r = self.guarded(q.ext, q.f);
                self.report(r);
                // Code that runs between units (the host, a harness) gets a number of its own.
                self.cmd_seq += 1;
            }
            return true;
        }
        if let Some(ev) = self.fed.pop_front() {
            self.dispatch(ev, KeySource::Fed);
            self.cmd_seq += 1;
            return true;
        }
        if let Some(ev) = self.typed.pop_front() {
            self.dispatch(ev, KeySource::Typed);
            self.cmd_seq += 1;
            return true;
        }
        false
    }

    pub fn run_until_idle(&mut self) {
        for _ in 0..1_000_000 {
            if self.step() {
                continue;
            }
            if self.outstanding == 0 {
                return;
            }
            match self.jobs_rx.recv_timeout(Duration::from_secs(5)) {
                Ok((id, v)) => {
                    // Put it back through the normal path.
                    let tx = self.jobs_tx.clone();
                    let _ = tx.send((id, v));
                }
                Err(_) => return,
            }
        }
        panic!("loop never went idle");
    }

    /// Shut extensions down in reverse order and cancel what they detached (C4).
    pub fn shutdown(&mut self) {
        for i in (0..self.exts.len()).rev() {
            let r = self.guarded(i, |ctx| {
                ctx.with_ext_idx(i, |e, ctx| e.shutdown(ctx))?;
                Ok(())
            });
            self.report(r);
            for e in &self.subs {
                if e.inner.ext == i {
                    e.inner.alive.set(false);
                }
            }
        }
    }

    // -------------------------------------------------------------- key dispatch (contract §7)

    fn dispatch(&mut self, ev: KeyEvent, src: KeySource) {
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
        if ev.mods.super_ && ev.kind != KeyKind::Release {
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
        self.cur_window = Some(win);
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
