//! The extension machinery (contract §2, §4, §8, §10; C1, C2, C9): the traits extensions
//! implement, the registry they declare into, `Ctx` and its startup, checkout, modes and the guard
//! around every call into extension code. Buffers and registers are in `workspace`, and windows in
//! `windows`.
#![warn(missing_docs)]
use crate::app::Input;
use crate::dispatch::*;
use crate::error::*;
use crate::faces::{Faces, Style, Theme};
use crate::files::SettingsFn;
use crate::harness::FakeClipboard;
use crate::ids::*;
use crate::keymap::*;
use crate::keys::*;
use crate::pointer::Drawn;
use crate::prompt::Prompt;
use crate::rt::*;
use crate::text::*;
use crate::windows::Window;
use crate::workspace::tree::Tree;
use crate::workspace::{Clip, Clipboard, Register};
use slotmap::SlotMap;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind, set_hook, take_hook};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Once, mpsc};

/// An extension's place in the startup order.
pub type ExtIdx = usize;
pub(crate) const CORE: ExtIdx = usize::MAX;
pub(crate) const CONFIG: ExtIdx = usize::MAX - 1;

type ConfigRegisterFn = Box<dyn FnOnce(&mut Registry)>;
type StartupFn = Box<dyn FnOnce(&mut Ctx) -> Result>;

/// Your config, as core applies it at startup (contract §4). It comes after every extension's
/// `register`, so it wins within each keymap it binds into, and before any extension's `init`.
/// The environment comes with M2. The default is no config at all.
#[derive(Default)]
pub struct Config {
    /// Your bindings, commands, faces and leader, declared on the [`Registry`] the extensions
    /// used, after them. Every bound name is checked once this has run (C8). A panic here is a
    /// startup error.
    pub register: Option<ConfigRegisterFn>,
    /// The theme. Faces it styles that no one declared are reported to `*messages*`.
    pub theme: Option<Theme>,
    /// The settings every text buffer starts with.
    pub text_defaults: Settings,
    /// Settings by path, on top of `text_defaults` ([`Ctx::set_file_settings`]).
    pub file_settings: Option<SettingsFn>,
    /// The clock. The default is a fake one, as tests want, so a real DIE passes [`Clock::real`].
    pub clock: Clock,
    /// The system clipboard. The default is one in memory, as tests want.
    pub clipboard: Option<Box<dyn Clipboard>>,
    /// Runs once every extension has initialized, and the session is restored (K12), with
    /// `Ctx`. An error or a panic is logged, and the editor still starts.
    pub startup: Option<StartupFn>,
}

// ------------------------------------------------------------------ traits

/// An extension: a typed value in your config (contract §4). Its struct is its global
/// state, reached with [`Ctx::with_ext`].
pub trait Extension: Any {
    /// Its name, as messages show it, and the prefix for its commands and faces.
    fn name(&self) -> &'static str;
    /// Declarations only: commands, keymaps, modes. No editor access.
    fn register(&mut self, reg: &mut Registry);
    /// Start watchers, timers and background state (contract §4, step 4). An error or
    /// panic is logged, and startup goes on.
    fn init(&mut self, _ctx: &mut Ctx) -> Result {
        Ok(())
    }
    /// Called in reverse order at shutdown. What the extension detached is cancelled after.
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
    /// What the session keeps for this mode (contract §14).
    fn save(&self) -> Option<String> {
        None
    }
}

/// Input a surface receives through [`Surface::event`].
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A key no keymap took, or any key in passthrough.
    Key(KeyEvent),
    /// A window showing the surface changed size, in cells.
    Resize {
        /// Its new width.
        cols: usize,
        /// Its new height.
        rows: usize,
    },
    /// A window showing the surface gained or lost the focus.
    Focus(bool),
}

/// A custom surface: what a buffer shows when it isn't text (contract §10).
pub trait Surface: Any {
    /// Its kind (`"term"`), which names its keymap.
    fn kind(&self) -> &'static str;
    /// Write its rows. No `Ctx`, so it runs synchronously (C3).
    fn render(&mut self, cols: usize, rows: usize, out: &mut Vec<String>);
    /// Input keymaps didn't take; every key (and release) in passthrough. Checked out (C2).
    fn event(&mut self, _ev: &Event, _ctx: &mut Ctx) -> bool {
        false
    }
    /// The state a buffer showing it starts in.
    fn initial_state(&self) -> State {
        State::Normal
    }
}

/// What's true of a mode whichever buffer it's on, declared once in the registry.
pub struct ModeSpec {
    /// Its name.
    pub name: &'static str,
    type_id: TypeId,
    pub(crate) owner: ExtIdx,
    /// The state a buffer enters when the mode is enabled, if any.
    pub initial_state: Option<State>,
    /// Its keymap, looked up before the surface kind's.
    pub keymap: Keymap,
    new: Rc<dyn Fn() -> Box<dyn Mode>>,
}

impl ModeSpec {
    /// Bind `keys` to `cmd` in `state`.
    pub fn bind(&mut self, state: State, keys: &str, cmd: &str) -> &mut Self {
        self.keymap.bind(state, keys, cmd);
        self
    }
    /// Set the state a buffer enters when the mode is enabled.
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
    /// Declare a command. It runs guarded, and its errors go to `*messages*`.
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
    /// Declare a mode, with what makes each buffer's instance.
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
    /// The global keymap, looked up last.
    pub fn global_keymap(&mut self) -> &mut Keymap {
        &mut self.global
    }
    /// The keymap for a surface kind ("text", "term"), like a major-mode map.
    pub fn kind_keymap(&mut self, kind: &'static str) -> &mut Keymap {
        self.kinds.entry(kind).or_default()
    }
    /// A registered mode's keymap, so your config can bind in it (C8). None if no mode has that
    /// name.
    pub fn mode_keymap(&mut self, name: &str) -> Option<&mut Keymap> {
        let spec = self.modes.iter_mut().find(|m| m.name == name)?;
        Some(&mut spec.keymap)
    }
    /// A named keymap, for keymap decorations on ranges.
    pub fn keymap(&mut self, name: &str) -> &mut Keymap {
        self.named.entry(name.into()).or_default()
    }
    /// Set the key `<leader>` stands for, in notation (`\`, `<Space>`, `,`). It applies to
    /// every keymap once all extensions have registered, so your config, applied last, decides
    /// it. The default is `\`. Anything but one key is a startup error.
    pub fn set_leader(&mut self, key: &str) {
        self.leader = Some(key.into());
    }
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
        /// The extension that made it, for its panics.
        owner: ExtIdx,
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
    pub directory: PathBuf,
    /// When it was made or last shown or focused.
    pub used: u64,
    pub content: Content,
    pub modes: Vec<ModeSlot>,
    pub local: Keymap,
    pub state: State,
}

/// The workspace, passed mutably to every command, callback and hook (C1).
pub struct Ctx {
    pub(crate) exts: Vec<ExtSlot>,
    pub(crate) commands: HashMap<String, Command>,
    pub(crate) global: Keymap,
    pub(crate) kinds: HashMap<&'static str, Keymap>,
    pub(crate) modes: Vec<ModeSpec>,
    pub(crate) named: HashMap<String, Keymap>,
    pub(crate) buffers: SlotMap<BufferId, Buffer>,
    pub(crate) windows: SlotMap<WindowId, Window>,
    pub(crate) text_defaults: Settings,
    pub(crate) focused: Option<WindowId>,
    pub(crate) ext_stack: Vec<ExtIdx>,
    pub(crate) queue: VecDeque<Queued>,
    pub(crate) subs: Vec<SubEntry>,
    pub(crate) next_id: u64,
    pub(crate) jobs_tx: mpsc::Sender<JobMsg>,
    pub(crate) jobs_rx: mpsc::Receiver<JobMsg>,
    pub(crate) outstanding: usize,
    pub(crate) typed: VecDeque<Input>,
    pub(crate) fed: VecDeque<KeyEvent>,
    /// Where the next key fed from inside a call goes. Each turn starts it at the front.
    pub(crate) fed_at: usize,
    pub(crate) pending: Vec<KeyEvent>,
    pub(crate) swallow: Vec<Key>,
    pub(crate) leader: Chord,
    pub(crate) prefix: Prefix,
    pub(crate) prompt: Option<Prompt>,
    /// What prompts remember, by history name (K11a).
    pub(crate) histories: HashMap<String, Vec<String>>,
    pub(crate) clock: Clock,
    pub(crate) timers: Timers,
    pub(crate) timer_seq: u64,
    /// Timers have been queued, and no key has had a turn since nor the host been told.
    pub(crate) timers_ran: bool,
    pub(crate) idle: Vec<Idle>,
    pub(crate) waker: Waker,
    pub(crate) cmd_seq: u64,
    /// The window the running command acts for. With none, the focused window acts.
    pub(crate) cur_window: Option<WindowId>,
    pub(crate) registers: HashMap<char, Register>,
    pub(crate) clip: Clip,
    /// The split tree and the frame it divides.
    pub(crate) tree: Tree,
    /// The last number given out for when a window or buffer was used.
    pub(crate) tick: u64,
    /// DIE's OS window has the keyboard focus (K9).
    pub(crate) app_focused: bool,
    /// Whether a turn has taken input since [`Ctx::run_ready`] began.
    pub(crate) input_ran: bool,
    /// What the last composed frame's cells showed.
    pub(crate) drawn: Drawn,
    /// `run_ready` hit its turn limit and has said so.
    pub(crate) over_limit: bool,
    pub(crate) files: crate::files::Files,
    /// The log: errors, panics and messages, oldest first. `*messages*` shows it, the newest goes
    /// on the command row, and the oldest go past 1,000.
    pub messages: Vec<String>,
    pub(crate) log: crate::messages::Log,
    pub(crate) faces: Faces,
    pub(crate) search_highlight: Option<Pattern>,
}

fn panic_msg(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic>".into())
}

/// A panic: its message and, when the hook saw it, where it happened.
pub(crate) struct Caught {
    pub(crate) msg: String,
    place: String,
}

/// How every panic line ends.
const RESTART: &str = ". Its state may be half-updated: restart if it misbehaves.";

thread_local! {
    /// The panics the hook saw on this thread that no guard has claimed yet.
    static NOTICED: RefCell<Vec<Caught>> = const { RefCell::new(Vec::new()) };
}

/// The panic hook (contract §15), once per process and ahead of the hook already there. It runs
/// on the panicking thread. Panicking in a hook would abort, so it doesn't insist on the list.
fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = take_hook();
        set_hook(Box::new(move |info| {
            let seen = Caught {
                msg: panic_msg(info.payload()),
                place: info
                    .location()
                    .map_or(String::new(), |l| format!(" at {l}")),
            };
            let _ = NOTICED.try_with(|n| n.try_borrow_mut().map(|mut n| n.push(seen)));
            prev(info);
        }));
    });
}

/// Run `f`, catching a panic and claiming what the hook saw of it.
pub(crate) fn catch<R>(f: impl FnOnce() -> R) -> std::result::Result<R, Caught> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        let msg = panic_msg(&*p);
        let seen = NOTICED.with_borrow_mut(|n| {
            n.pop_if(|c| c.msg == msg)
                .map_or(String::new(), |c| c.place)
        });
        Caught { msg, place: seen }
    })
}

impl Ctx {
    /// [`Ctx::with_config`] with no config.
    pub fn new(exts: Vec<Box<dyn Extension>>) -> std::result::Result<Ctx, Vec<String>> {
        Ctx::with_config(exts, Config::default())
    }

    /// Startup, contract §4's steps 2 to 6: register extensions in order, apply your config,
    /// check every bound name exists (C8) and report every problem at once, then init each
    /// extension and run the startup hook. K12's restore goes between those last two.
    pub fn with_config(
        exts: Vec<Box<dyn Extension>>,
        config: Config,
    ) -> std::result::Result<Ctx, Vec<String>> {
        install_panic_hook();
        NOTICED.take(); // not this workspace's: they came before it
        let mut reg = Registry {
            cur: CORE,
            ..Default::default()
        };
        crate::layout::register_commands(&mut reg);
        let mut errors = vec![];
        let mut slots: Vec<ExtSlot> = vec![];
        for mut e in exts {
            let type_id = (*e).type_id();
            if slots.iter().any(|s| s.type_id == type_id) {
                errors.push(format!("extension {} registered twice", e.name()));
                continue;
            }
            reg.cur = slots.len();
            if let Err(c) = catch(|| e.register(&mut reg)) {
                let name = e.name();
                errors.push(format!("{name} panicked in register: {}{}", c.msg, c.place));
            }
            slots.push(ExtSlot {
                name: e.name(),
                type_id,
                obj: Some(e),
                panics: 0,
            });
        }
        reg.cur = CONFIG;
        if let Some(f) = config.register
            && let Err(c) = catch(|| f(&mut reg))
        {
            errors.push(format!("config panicked: {}{}", c.msg, c.place));
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
            text_defaults: config.text_defaults,
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
            fed_at: 0,
            pending: vec![],
            swallow: vec![],
            leader: default_leader(),
            prefix: Prefix::default(),
            prompt: None,
            histories: HashMap::new(),
            clock: config.clock,
            timers: Timers::new(),
            timer_seq: 0,
            timers_ran: false,
            idle: vec![],
            waker: Waker::default(),
            cmd_seq: 0,
            cur_window: None,
            registers: HashMap::new(),
            clip: Clip::new(
                config
                    .clipboard
                    .unwrap_or_else(|| Box::new(FakeClipboard::default())),
            ),
            tree: Tree::default(),
            tick: 0,
            app_focused: true,
            input_ran: false,
            drawn: Drawn::default(),
            over_limit: false,
            files: Default::default(),
            messages: vec![],
            log: Default::default(),
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
        if let Some(t) = config.theme {
            ctx.set_theme(t);
        }
        ctx.files.settings = config.file_settings;
        errors.extend(ctx.check_bindings());
        if !errors.is_empty() {
            return Err(errors);
        }
        for i in 0..ctx.exts.len() {
            let r = ctx.guarded(i, |ctx| ctx.with_ext_idx(i, |e, ctx| e.init(ctx))?);
            ctx.report(r);
        }
        if let Some(f) = config.startup {
            let r = ctx.guarded(CONFIG, f);
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
        match i {
            CONFIG => "config",
            _ => self.exts.get(i).map_or("core", |s| s.name),
        }
    }
    pub(crate) fn cur_ext(&self) -> ExtIdx {
        self.ext_stack.last().copied().unwrap_or(CORE)
    }
    /// How many extension calls are on the stack. Queued calls run at depth 1 (C3).
    pub fn nesting(&self) -> usize {
        self.ext_stack.len()
    }
    /// A command's doc and the name of whoever registered it.
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
        let r = catch(|| f(self));
        self.ext_stack.pop();
        r.unwrap_or_else(|c| Err(self.panicked(ext, c)))
    }
    /// A panic caught from `ext`'s code: count it, log it, and make it an error.
    pub(crate) fn panicked(&mut self, ext: ExtIdx, c: Caught) -> Error {
        let name = self.ext_name(ext);
        if let Some(s) = self.exts.get_mut(ext) {
            s.panics += 1;
        }
        self.log_panic(format!("{name} panicked: {}{}", c.msg, c.place));
        Error::Panicked {
            ext: name,
            msg: c.msg,
        }
    }
    /// A line already among the panic lines at the log's end, up to 32 back, isn't news, so panics
    /// on every frame can't flood the log, even from several objects taking turns. Disabling them
    /// is M1's.
    fn log_panic(&mut self, what: String) {
        let line = format!("{what}{RESTART}");
        let recent = self.messages.iter().rev().take(32);
        let mut run = recent.take_while(|m| m.ends_with(RESTART));
        if !run.any(|m| *m == line) {
            self.messages.push(line);
        }
    }
    /// Core's own tidying up of what an extension gave it: a `Drop` there is extension code too.
    pub(crate) fn cleanup(&mut self, f: impl FnOnce(&mut Ctx)) {
        if let Err(c) = catch(|| f(self)) {
            self.panicked(CORE, c);
        }
    }
    /// Log the panics no guard claimed: `render`, `decorate`, or what the host caught itself.
    pub(crate) fn log_noticed(&mut self) {
        for c in NOTICED.take() {
            self.log_panic(format!("A panic: {}{}", c.msg, c.place));
        }
    }
    /// Log an error. `guarded` has already logged a panic.
    pub(crate) fn report(&mut self, r: Result) {
        match r {
            Ok(()) | Err(Error::Panicked { .. }) => {}
            Err(e) => self.messages.push(e.to_string()),
        }
    }

    // -------------------------------------------------------------- C2 checkout

    /// Check an extension out (C2). `Busy` while it's already out, and `Missing` if it isn't
    /// registered. A panic in `f` puts it back, then goes on.
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

    /// Check a buffer's mode out (C2). `Gone` if the buffer was killed, `Busy` while it's
    /// already out, and `Missing` if the buffer doesn't have it. A panic in `f` puts it back,
    /// then goes on.
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

    /// Check a buffer's surface out (C2). `Gone` if the buffer was killed, `Busy` while it's
    /// already out, and `Missing` if it's another type. A panic in `f` puts it back, then
    /// goes on.
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

    // -------------------------------------------------------------- modes

    /// Enable a registered mode on a text buffer (C17). Its `on_enable` is queued (C3).
    /// Enabling it again does nothing.
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
    /// Disable a mode. Its `on_disable` is queued (C3), and it drops after that.
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
}
