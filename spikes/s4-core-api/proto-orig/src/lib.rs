//! Throwaway prototype: do the extension-contract shapes compile and behave on rustc 1.96?
//! Dependency-free on purpose. Not a design proposal in every detail.
#![allow(dead_code)]

#[cfg(panic = "abort")]
compile_error!("DIE requires panic = \"unwind\" (C9). Set it in the config crate's profile.");

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

// ---------------------------------------------------------------- C4 generational ids
pub struct Id<T> { idx: u32, generation: u32, _t: PhantomData<fn() -> T> }
impl<T> Clone for Id<T> { fn clone(&self) -> Self { *self } }
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> { fn eq(&self, o: &Self) -> bool { self.idx == o.idx && self.generation == o.generation } }
impl<T> std::fmt::Debug for Id<T> { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, "{}v{}", self.idx, self.generation) } }
// Ids are Copy + Send + 'static so they can be captured by 'static callbacks and moved to threads.

pub struct Arena<T> { slots: Vec<(u32, Option<T>)>, free: Vec<u32> }
impl<T> Default for Arena<T> { fn default() -> Self { Self { slots: vec![], free: vec![] } } }
impl<T> Arena<T> {
    pub fn insert(&mut self, v: T) -> Id<T> {
        if let Some(i) = self.free.pop() { let s = &mut self.slots[i as usize]; s.0 += 1; s.1 = Some(v); Id { idx: i, generation: s.0, _t: PhantomData } }
        else { self.slots.push((0, Some(v))); Id { idx: self.slots.len() as u32 - 1, generation: 0, _t: PhantomData } }
    }
    pub fn get(&self, id: Id<T>) -> Option<&T> { self.slots.get(id.idx as usize).filter(|s| s.0 == id.generation).and_then(|s| s.1.as_ref()) }
    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> { self.slots.get_mut(id.idx as usize).filter(|s| s.0 == id.generation).and_then(|s| s.1.as_mut()) }
    pub fn remove(&mut self, id: Id<T>) -> Option<T> {
        let s = self.slots.get_mut(id.idx as usize).filter(|s| s.0 == id.generation)?; let v = s.1.take(); self.free.push(id.idx); v
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> { self.slots.iter_mut().filter_map(|s| s.1.as_mut()) }
}

pub type BufferId = Id<Buffer>;
pub type WindowId = Id<Window>;
pub struct Window { pub buffer: BufferId, pub cols: u16, pub rows: u16 }

// ---------------------------------------------------------------- errors
#[derive(Debug, PartialEq)]
pub enum Checkout { Gone, Busy, Missing }
#[derive(Debug)]
pub enum Error { Checkout(Checkout), Panicked { ext: &'static str, msg: String }, Msg(String) }
impl From<Checkout> for Error { fn from(c: Checkout) -> Self { Error::Checkout(c) } }
pub type Result<T = ()> = std::result::Result<T, Error>;

// ---------------------------------------------------------------- Extension
pub trait Extension: Any {
    fn name(&self) -> &'static str;
    fn register(&mut self, reg: &mut Registry);
    fn init(&mut self, _ctx: &mut Ctx) -> Result { Ok(()) }
    fn shutdown(&mut self, _ctx: &mut Ctx) {}
}

// ---------------------------------------------------------------- Mode
pub struct Change { pub start: usize, pub old_end: usize, pub new_text: String, pub rev: u64 }
pub struct Decorations { pub faces: Vec<(std::ops::Range<usize>, &'static str)> }

pub trait Mode: Any {
    fn name(&self) -> &'static str;
    fn on_edit(&mut self, _change: &Change, _text: &TextBuf) {}
    /// Render-time provider. No Ctx. &mut self so it can cache by revision.
    fn decorate(&mut self, _text: &TextBuf, _visible: std::ops::Range<usize>, _out: &mut Decorations) {}
    /// Must be dyn-compatible: concrete value type, not `fn save<S: Serializer>`.
    fn save(&self) -> Option<String> { None }
}

// ---------------------------------------------------------------- Surface
pub struct Frame { pub cells: Vec<char> }
pub trait Surface: Any {
    fn kind(&self) -> &'static str;
    fn render(&mut self, frame: &mut Frame);
    fn event(&mut self, _ev: &str, _ctx: &mut Ctx) -> bool { false }
}

// ---------------------------------------------------------------- Text (core-owned, never checked out)
#[derive(Default)]
pub struct TextBuf { pub s: String, pub rev: u64, pub log: Vec<Change> }
impl TextBuf {
    pub fn changes_since(&self, rev: u64) -> Option<&[Change]> {
        let i = self.log.iter().position(|c| c.rev > rev).unwrap_or(self.log.len());
        if rev + (self.log.len() - i) as u64 == self.rev { Some(&self.log[i..]) } else { None }
    }
}

// ---------------------------------------------------------------- Buffer
type Slot<T> = Option<T>; // None == checked out
pub struct ModeSlot { type_id: TypeId, owner: ExtIdx, obj: Slot<Box<dyn Mode>>, missed: Vec<u64> }
pub enum Content { Text(TextBuf), Custom { type_id: TypeId, owner: ExtIdx, obj: Slot<Box<dyn Surface>> } }
pub struct Buffer { pub name: String, pub content: Content, pub modes: Vec<ModeSlot>, pub dead: bool }

// ---------------------------------------------------------------- Commands, args
#[derive(Default, Clone)]
pub struct Args { pub count: Option<usize>, pub register: Option<char>, pub text: String, pub keys: Vec<Key>, pub window: Option<WindowId> }
pub type CommandFn = Rc<dyn Fn(&mut Ctx, &Args) -> Result>;
pub struct Command { pub name: String, pub doc: &'static str, pub owner: Option<ExtIdx>, pub f: CommandFn }

// ---------------------------------------------------------------- Keymaps with <char>
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key { Char(char), Named(&'static str) }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pat { Key(Key), AnyChar }
#[derive(Default)]
pub struct Trie { pub cmd: Option<String>, pub next: HashMap<Pat, Trie> }
pub enum Lookup<'a> { Command(&'a str), Prefix, None }
impl Trie {
    pub fn bind(&mut self, seq: &[Pat], cmd: &str) {
        let mut n = self; for p in seq { n = n.next.entry(*p).or_default(); } n.cmd = Some(cmd.into());
    }
    pub fn lookup(&self, keys: &[Key]) -> Lookup<'_> {
        let mut n = self;
        for k in keys {
            n = match n.next.get(&Pat::Key(*k)) {
                Some(x) => x,
                None => match (k, n.next.get(&Pat::AnyChar)) { (Key::Char(c), Some(x)) if !c.is_control() => x, _ => return Lookup::None },
            };
        }
        match &n.cmd { Some(c) => Lookup::Command(c), None if !n.next.is_empty() => Lookup::Prefix, None => Lookup::None }
    }
}

// ---------------------------------------------------------------- Registry
pub type ExtIdx = usize;
pub type ModeFactory = Rc<dyn Fn() -> Box<dyn Mode>>;
#[derive(Default)]
pub struct Registry { cur: ExtIdx, pub commands: Vec<Command>, pub kind_maps: HashMap<(&'static str, &'static str), Trie>, pub modes: HashMap<&'static str, ModeFactory> }
impl Registry {
    pub fn command(&mut self, name: &str, doc: &'static str, f: impl Fn(&mut Ctx, &Args) -> Result + 'static) {
        self.commands.push(Command { name: name.into(), doc, owner: Some(self.cur), f: Rc::new(f) });
    }
    /// Typed helper: method on the extension struct, reached via checkout. Generic on a concrete type: fine for dyn.
    pub fn ext_command<E: Extension>(&mut self, name: &str, doc: &'static str, f: fn(&mut E, &mut Ctx, &Args) -> Result) {
        self.command(name, doc, move |ctx, args| ctx.with_ext(|e: &mut E, ctx| f(e, ctx, args))?);
    }
    pub fn mode<M: Mode>(&mut self, name: &'static str, make: impl Fn() -> M + 'static) {
        self.modes.insert(name, Rc::new(move || Box::new(make()) as Box<dyn Mode>));
    }
    pub fn keymap(&mut self, kind: &'static str, state: &'static str) -> &mut Trie { self.kind_maps.entry((kind, state)).or_default() }
}

// ---------------------------------------------------------------- Async (C3)
type Done = Box<dyn FnOnce(&mut Ctx, Box<dyn Any + Send>) -> Result>;
pub enum Owner { Ext(ExtIdx), Buffer(BufferId) }
pub struct Job { owner: Owner, ext: ExtIdx, done: Done }
pub type JobId = Id<Job>;

// ---------------------------------------------------------------- Pixel producer
pub struct PixelShared { pub back: Mutex<(u32, u32, Vec<u8>)>, pub presented: AtomicU64, pub closed: std::sync::atomic::AtomicBool }
pub struct PixelProducer { shared: Arc<PixelShared>, wake: Arc<dyn Fn() + Send + Sync> }
impl PixelProducer {
    pub fn present(&self, f: impl FnOnce(u32, u32, &mut [u8]) -> (u32, u32, u32, u32)) -> std::result::Result<(), Checkout> {
        if self.shared.closed.load(Ordering::Acquire) { return Err(Checkout::Gone); }
        let mut g = self.shared.back.lock().unwrap(); let (w, h) = (g.0, g.1);
        let _damage = f(w, h, &mut g.2); drop(g);
        self.shared.presented.fetch_add(1, Ordering::Release); (self.wake)(); Ok(())
    }
}
const _: () = { const fn send<T: Send + 'static>() {} send::<PixelProducer>(); send::<BufferId>(); };

// ---------------------------------------------------------------- Ctx (lifetime-free on purpose)
struct ExtSlot { name: &'static str, type_id: TypeId, obj: Slot<Box<dyn Extension>>, panics: u32 }
pub struct Ctx {
    exts: Vec<ExtSlot>,
    pub buffers: Arena<Buffer>,
    pub windows: Arena<Window>,
    commands: HashMap<String, Command>,
    jobs: Arena<Job>,
    tx: mpsc::Sender<(JobId, Box<dyn Any + Send>)>,
    rx: mpsc::Receiver<(JobId, Box<dyn Any + Send>)>,
    ext_stack: Vec<ExtIdx>,
    pub messages: Vec<String>,
}

fn panic_msg(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "<non-string panic>".into())
}

impl Ctx {
    pub fn new(mut exts: Vec<Box<dyn Extension>>) -> (Ctx, Registry) {
        let (tx, rx) = mpsc::channel();
        let mut reg = Registry::default();
        let mut slots = vec![];
        for (i, e) in exts.iter_mut().enumerate() {
            reg.cur = i;
            let tid = (**e).type_id(); // NB: (**e) — calling type_id() on the Box gives Box's TypeId
            // register can panic too: wrap it.
            let _ = catch_unwind(AssertUnwindSafe(|| e.register(&mut reg)));
            slots.push(tid);
        }
        let exts = exts.into_iter().zip(slots).map(|(e, t)| ExtSlot { name: e.name(), type_id: t, obj: Some(e), panics: 0 }).collect();
        let mut ctx = Ctx { exts, buffers: Arena::default(), windows: Arena::default(), commands: HashMap::new(), jobs: Arena::default(), tx, rx, ext_stack: vec![], messages: vec![] };
        for c in reg.commands.drain(..) { ctx.commands.insert(c.name.clone(), c); }
        (ctx, reg)
    }

    /// The one place core calls extension code. Attributes the panic, keeps unwinding contained.
    fn guarded<R>(&mut self, ext: ExtIdx, f: impl FnOnce(&mut Ctx) -> Result<R>) -> Result<R> {
        self.ext_stack.push(ext);
        let r = catch_unwind(AssertUnwindSafe(|| f(self)));
        self.ext_stack.pop();
        match r {
            Ok(r) => r,
            Err(p) => { let s = &mut self.exts[ext]; s.panics += 1; Err(Error::Panicked { ext: s.name, msg: panic_msg(&*p) }) }
        }
    }

    // --- C2 checkout: extension
    pub fn with_ext<E: Extension, R>(&mut self, f: impl FnOnce(&mut E, &mut Ctx) -> R) -> std::result::Result<R, Checkout> {
        let i = self.exts.iter().position(|s| s.type_id == TypeId::of::<E>()).ok_or(Checkout::Missing)?;
        let mut obj = self.exts[i].obj.take().ok_or(Checkout::Busy)?;
        // trait upcasting (stable since 1.86): &mut dyn Extension -> &mut dyn Any
        let any: &mut dyn Any = &mut *obj;
        let e = any.downcast_mut::<E>().expect("type id matched");
        // catch here so the object is ALWAYS put back, then keep unwinding to the outer guard
        let r = catch_unwind(AssertUnwindSafe(|| f(e, self)));
        self.exts[i].obj = Some(obj);
        Ok(r.unwrap_or_else(|p| resume_unwind(p)))
    }

    // --- C2 checkout: mode on a buffer
    pub fn with_mode<M: Mode, R>(&mut self, buf: BufferId, f: impl FnOnce(&mut M, &mut Ctx) -> R) -> std::result::Result<R, Checkout> {
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        let slot = b.modes.iter_mut().find(|m| m.type_id == TypeId::of::<M>()).ok_or(Checkout::Missing)?;
        let mut obj = slot.obj.take().ok_or(Checkout::Busy)?;
        let m = (&mut *obj as &mut dyn Any).downcast_mut::<M>().unwrap();
        let r = catch_unwind(AssertUnwindSafe(|| f(m, self)));
        // check-in: buffer may have been killed or the mode disabled meanwhile
        if let Some(b) = self.buffers.get_mut(buf) {
            if let Some(slot) = b.modes.iter_mut().find(|m| m.type_id == TypeId::of::<M>() && m.obj.is_none()) {
                // deliver edits this mode missed while checked out (its own edits included)
                let missed = std::mem::take(&mut slot.missed);
                if let Content::Text(t) = &b.content {
                    for rev in missed { if let Some(c) = t.log.iter().find(|c| c.rev == rev) { obj.on_edit(c, t); } }
                }
                slot.obj = Some(obj);
            } // else: dropped here; on_disable/kill cleanup for it would run now
        }
        Ok(r.unwrap_or_else(|p| resume_unwind(p)))
    }

    // --- C2 checkout: custom surface
    pub fn with_surface<S: Surface, R>(&mut self, buf: BufferId, f: impl FnOnce(&mut S, &mut Ctx) -> R) -> std::result::Result<R, Checkout> {
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        let Content::Custom { type_id, obj, .. } = &mut b.content else { return Err(Checkout::Missing) };
        if *type_id != TypeId::of::<S>() { return Err(Checkout::Missing); }
        let mut o = obj.take().ok_or(Checkout::Busy)?;
        let s = (&mut *o as &mut dyn Any).downcast_mut::<S>().unwrap();
        let r = catch_unwind(AssertUnwindSafe(|| f(s, self)));
        if let Some(Buffer { content: Content::Custom { obj, .. }, .. }) = self.buffers.get_mut(buf) { *obj = Some(o); }
        Ok(r.unwrap_or_else(|p| resume_unwind(p)))
    }

    // --- text: core-owned, borrowed directly, no checkout
    pub fn edit(&mut self, buf: BufferId, at: usize, ins: &str) -> Result {
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        let Content::Text(t) = &mut b.content else { return Err(Error::Msg("not text".into())) };
        t.s.insert_str(at, ins); t.rev += 1;
        t.log.push(Change { start: at, old_end: at, new_text: ins.into(), rev: t.rev });
        let rev = t.rev;
        // synchronous push to modes that are present; record for checked-out ones
        let (content, modes) = (&b.content, &mut b.modes); // disjoint field borrows
        let Content::Text(t) = content else { unreachable!() };
        let c = t.log.last().unwrap();
        for m in modes.iter_mut() {
            match &mut m.obj {
                Some(o) => { let o = &mut **o; let _ = catch_unwind(AssertUnwindSafe(|| o.on_edit(c, t))); }
                None => m.missed.push(rev),
            }
        }
        Ok(())
    }

    pub fn create_text(&mut self, name: &str) -> BufferId { self.buffers.insert(Buffer { name: name.into(), content: Content::Text(TextBuf::default()), modes: vec![], dead: false }) }
    pub fn create_surface<S: Surface>(&mut self, name: &str, s: S) -> BufferId {
        let owner = *self.ext_stack.last().unwrap_or(&0);
        self.buffers.insert(Buffer { name: name.into(), content: Content::Custom { type_id: TypeId::of::<S>(), owner, obj: Some(Box::new(s)) }, modes: vec![], dead: false })
    }
    pub fn enable_mode<M: Mode>(&mut self, buf: BufferId, m: M) -> Result {
        let owner = *self.ext_stack.last().unwrap_or(&0);
        let b = self.buffers.get_mut(buf).ok_or(Checkout::Gone)?;
        b.modes.push(ModeSlot { type_id: TypeId::of::<M>(), owner, obj: Some(Box::new(m)), missed: vec![] }); Ok(())
    }
    pub fn kill(&mut self, buf: BufferId) {
        if self.buffers.remove(buf).is_some() {
            // cancel buffer-owned subscriptions
            let dead: Vec<JobId> = self.jobs.slots.iter().enumerate()
                .filter_map(|(i, s)| match &s.1 { Some(Job { owner: Owner::Buffer(b), .. }) if *b == buf => Some(Id { idx: i as u32, generation: s.0, _t: PhantomData }), _ => None }).collect();
            for j in dead { self.jobs.remove(j); }
        }
    }

    // --- commands, re-entrant run
    pub fn run(&mut self, name: &str, args: &Args) -> Result {
        let (f, owner) = { let c = self.commands.get(name).ok_or_else(|| Error::Msg(format!("no command {name}")))?; (c.f.clone(), c.owner.unwrap_or(0)) };
        self.guarded(owner, |ctx| f(ctx, args))
    }

    // --- C3 async
    pub fn spawn<T: Send + 'static>(&mut self, owner: Option<BufferId>, work: impl FnOnce() -> T + Send + 'static,
                                    done: impl FnOnce(&mut Ctx, T) -> Result + 'static) -> JobId {
        let ext = *self.ext_stack.last().unwrap_or(&0);
        let owner = owner.map(Owner::Buffer).unwrap_or(Owner::Ext(ext));
        let id = self.jobs.insert(Job { owner, ext, done: Box::new(move |ctx, v| done(ctx, *v.downcast::<T>().expect("job type"))) });
        let tx = self.tx.clone();
        std::thread::spawn(move || { let v = work(); let _ = tx.send((id, Box::new(v))); });
        id
    }
    pub fn pump(&mut self) -> usize {
        let mut n = 0;
        while let Ok((id, v)) = self.rx.recv_timeout(std::time::Duration::from_millis(200)) {
            if let Some(job) = self.jobs.remove(id) { let ext = job.ext; let r = self.guarded(ext, |ctx| (job.done)(ctx, v)); if let Err(e) = r { self.messages.push(format!("{e:?}")); } n += 1; }
            if self.jobs.slots.iter().all(|s| s.1.is_none()) { break; }
        }
        n
    }

    // --- render: modes' providers + custom surfaces, no Ctx
    pub fn render(&mut self) -> Vec<String> {
        let mut out = vec![];
        for b in self.buffers.iter_mut() {
            let Buffer { content, modes, name, .. } = b; // disjoint borrows: text (&) and modes (&mut)
            match content {
                Content::Text(t) => {
                    let mut d = Decorations { faces: vec![] };
                    for m in modes.iter_mut() {
                        if let Some(o) = &mut m.obj {
                            if catch_unwind(AssertUnwindSafe(|| o.decorate(t, 0..t.s.len(), &mut d))).is_err() { out.push(format!("{name}: provider panicked")); }
                        }
                    }
                    out.push(format!("{name}: {:?} faces={}", t.s, d.faces.len()));
                }
                Content::Custom { obj: Some(s), .. } => {
                    let mut f = Frame { cells: vec![] };
                    if catch_unwind(AssertUnwindSafe(|| s.render(&mut f))).is_err() { out.push(format!("{name}: <render panicked>")); }
                    else { out.push(format!("{name}: {}", f.cells.iter().collect::<String>())); }
                }
                Content::Custom { obj: None, .. } => out.push(format!("{name}: <checked out during render?!>")),
            }
        }
        out
    }
}

pub fn shared<T>(v: T) -> Rc<RefCell<T>> { Rc::new(RefCell::new(v)) }
