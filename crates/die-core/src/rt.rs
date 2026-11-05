//! The runtime (contract §12, C3, C4): subscription handles, the queue of calls that receive
//! `Ctx`, timers, jobs and the loop.
#![warn(missing_docs)]
use crate::dispatch::*;
use crate::error::*;
use crate::ext::*;
use crate::ids::*;
use crate::keys::*;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

// ------------------------------------------------------------------ subscriptions (C4)

pub(crate) struct SubInner {
    pub(crate) alive: Cell<bool>,
    detached: Cell<bool>,
    pub(crate) ext: ExtIdx,
}

/// A subscription handle. Dropping it cancels the subscription; `detach` ties it to the extension.
#[must_use = "dropping a Sub cancels it; call .detach() to keep it for the extension's lifetime"]
pub struct Sub(Rc<SubInner>);

impl Sub {
    /// Keep the subscription until the extension shuts down.
    pub fn detach(self) {
        self.0.detached.set(true);
    }
    /// Whether the subscription hasn't been cancelled.
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

/// What a job's thread sends: its id, who spawned it, and its value or its panic.
pub(crate) type JobMsg = (
    u64,
    ExtIdx,
    std::result::Result<Box<dyn Any + Send>, Caught>,
);

pub(crate) type Callback = Box<dyn FnOnce(&mut Ctx) -> Result>;
type JobDone = Box<dyn FnOnce(&mut Ctx, Box<dyn Any + Send>) -> Result>;

type TimerFn = Rc<RefCell<dyn FnMut(&mut Ctx) -> Result>>;
type KilledFn = Rc<RefCell<dyn FnMut(&mut Ctx, BufferId) -> Result>>;
type KeysFn = RefCell<Box<dyn FnMut(&KeyEvent, KeySource)>>;

pub(crate) enum SubKind {
    Timer {
        at: Cell<Duration>,
        every: Option<Duration>,
        fired: Cell<bool>,
        f: TimerFn,
    },
    Killed(KilledFn),
    Job {
        id: u64,
        f: JobDone,
    },
    Keys(KeysFn),
}

pub(crate) struct SubEntry {
    pub(crate) inner: Rc<SubInner>,
    pub(crate) kind: SubKind,
}

/// A call that receives Ctx, waiting for the top of the loop (C3).
pub(crate) struct Queued {
    pub(crate) ext: ExtIdx,
    pub(crate) alive: Option<Rc<SubInner>>,
    pub(crate) f: Callback,
}

impl Ctx {
    // -------------------------------------------------------------- async (C3, C4)

    pub(crate) fn defer_as(&mut self, ext: ExtIdx, f: impl FnOnce(&mut Ctx) -> Result + 'static) {
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
    /// Run `f` once, `d` from now, from the top of the loop (C3).
    pub fn after(&mut self, d: Duration, f: impl FnOnce(&mut Ctx) -> Result + 'static) -> Sub {
        let mut f = Some(f);
        self.timer(d, None, move |ctx| f.take().map_or(Ok(()), |f| f(ctx)))
    }
    /// Run `f` every `d`, from the top of the loop (C3).
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
    /// Run `f` after a buffer is killed, from the top of the loop (C3).
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
        let ext = self.cur_ext();
        let tx = self.jobs_tx.clone();
        std::thread::spawn(move || {
            let v = catch(work).map(|v| Box::new(v) as Box<dyn Any + Send>);
            let _ = tx.send((id, ext, v));
        });
        self.outstanding += 1;
        self.new_sub(SubKind::Job { id, f })
    }
    /// See every key before dispatch. No Ctx, so it runs synchronously (C3). For `q` recording.
    pub fn observe_keys(&mut self, f: impl FnMut(&KeyEvent, KeySource) + 'static) -> Sub {
        self.new_sub(SubKind::Keys(RefCell::new(Box::new(f))))
    }

    // -------------------------------------------------------------- the loop

    /// Harness: advance the fake clock, then run until idle.
    pub fn advance(&mut self, d: Duration) {
        self.clock += d;
        self.run_until_idle();
    }

    /// A job's thread is done: queue its `done`, unless it was cancelled or panicked.
    fn accept_job(&mut self, (id, ext, result): JobMsg) {
        self.outstanding -= 1;
        let i = self
            .subs
            .iter()
            .position(|e| matches!(&e.kind, SubKind::Job { id: j, .. } if *j == id));
        let entry = i.map(|i| self.subs.remove(i));
        match (result, entry) {
            (Err(c), _) => {
                self.panicked(ext, c);
            }
            (
                Ok(v),
                Some(SubEntry {
                    inner,
                    kind: SubKind::Job { f, .. },
                }),
            ) if inner.alive.get() => self.queue.push_back(Queued {
                ext: inner.ext,
                alive: Some(inner),
                f: Box::new(move |ctx| f(ctx, v)),
            }),
            _ => {}
        }
    }

    fn collect_due(&mut self) {
        self.log_noticed();
        while let Ok(m) = self.jobs_rx.try_recv() {
            self.accept_job(m);
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

    /// Harness: take turns until nothing is ready and no job is out.
    pub fn run_until_idle(&mut self) {
        for _ in 0..1_000_000 {
            if self.step() {
                continue;
            }
            if self.outstanding == 0 {
                return;
            }
            match self.jobs_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(m) => self.accept_job(m),
                Err(_) => return,
            }
        }
        self.messages
            .push("The loop never went idle in 1,000,000 turns, so this run stopped".into());
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
}
