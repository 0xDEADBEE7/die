//! The runtime (contract §12, C3, C4): subscription handles, the queue of calls that receive
//! `Ctx`, the timer heap and clocks, jobs with their channel and `Waker`, and the loop.
#![warn(missing_docs)]
use crate::dispatch::*;
use crate::error::*;
use crate::ext::*;
use crate::ids::*;
use crate::keys::*;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ clocks and the waker

/// Where the runtime reads the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    /// Real time, counted from this instant. DIE runs on this one (F3's `die::run`).
    Real(Instant),
    /// Time that moves only when [`Ctx::advance`] moves it, so timer-driven code runs instantly
    /// in tests (contract §16). This is the time it stands at.
    Fake(Duration),
}

impl Default for Clock {
    /// A fake clock at zero.
    fn default() -> Clock {
        Clock::Fake(Duration::ZERO)
    }
}

impl Clock {
    /// Real time, from now.
    pub fn real() -> Clock {
        Clock::Real(Instant::now())
    }
    /// The time on this clock.
    pub fn now(&self) -> Duration {
        match self {
            Clock::Real(start) => start.elapsed(),
            Clock::Fake(t) => *t,
        }
    }
}

#[derive(Default)]
struct WakeState {
    /// The host has been woken since the loop last took a turn.
    pending: AtomicBool,
    host: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

/// Tells whatever runs the loop that another thread has left it work (contract §12). Any thread
/// can hold one and call it. It names no event loop: the host connects its own with
/// [`Ctx::set_waker`].
#[derive(Clone, Default)]
pub struct Waker(Arc<WakeState>);

impl Waker {
    /// Wake the host, unless it's already been woken since the loop's last turn: however many
    /// jobs finish between two turns, the host hears once, and that turn finds them all. Before
    /// the host connects, this does nothing, and its first turn finds what's waiting.
    pub fn wake(&self) {
        if let Some(host) = self.0.host.get()
            && !self.0.pending.swap(true, Ordering::AcqRel)
        {
            host();
        }
    }
}

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

/// A timer waiting in the heap.
pub(crate) struct Timer {
    sub: Rc<SubInner>,
    every: Option<Duration>,
    f: TimerFn,
}

/// The timer heap: deadlines in order, ties in the order they were set. It's an ordered map, which
/// costs a heap's log n per timer with no ordering code to write.
pub(crate) type Timers = BTreeMap<(Duration, u64), Timer>;

/// An idle timer. `at` is when it's next due, or None once it's fired for this idle period.
pub(crate) struct Idle {
    at: Option<Duration>,
    d: Duration,
    sub: Rc<SubInner>,
    f: TimerFn,
}

/// The shortest period a repeating timer can have, so one with none can't hold the loop.
const MIN_PERIOD: Duration = Duration::from_millis(1);

/// A timer's call, for the queue. It still checks its handle when its turn comes.
fn fire(sub: &Rc<SubInner>, f: &TimerFn) -> Queued {
    let f = f.clone();
    Queued {
        ext: sub.ext,
        alive: Some(sub.clone()),
        f: Box::new(move |ctx| (f.borrow_mut())(ctx)),
    }
}

type KilledFn = Rc<RefCell<dyn FnMut(&mut Ctx, BufferId) -> Result>>;
type KeysFn = RefCell<Box<dyn FnMut(&KeyEvent, KeySource)>>;

pub(crate) enum SubKind {
    Killed(KilledFn),
    Job { id: u64, f: JobDone },
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
    fn new_handle(&self) -> Rc<SubInner> {
        Rc::new(SubInner {
            alive: Cell::new(true),
            detached: Cell::new(false),
            ext: self.cur_ext(),
        })
    }
    fn new_sub(&mut self, kind: SubKind) -> Sub {
        let inner = self.new_handle();
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
    /// Run `f` every `d`, from the top of the loop (C3). A period under a millisecond counts as
    /// one. Each firing counts from the last deadline, so being late doesn't shift the next. One
    /// that has fallen a whole period behind, as after a laptop's sleep, skips what it missed.
    pub fn every(&mut self, d: Duration, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        let d = d.max(MIN_PERIOD);
        self.timer(d, Some(d), f)
    }
    /// Run `f` when no key has come for `d`, once each time that happens, as Emacs's
    /// `run-with-idle-timer` does with a repeat. It counts from now, however long the editor has
    /// already been idle.
    pub fn idle(&mut self, d: Duration, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Sub {
        let sub = self.new_handle();
        self.idle.push(Idle {
            at: Some(self.clock.now().saturating_add(d)),
            d,
            sub: sub.clone(),
            f: Rc::new(RefCell::new(f)),
        });
        Sub(sub)
    }
    fn timer(
        &mut self,
        d: Duration,
        every: Option<Duration>,
        f: impl FnMut(&mut Ctx) -> Result + 'static,
    ) -> Sub {
        let sub = self.new_handle();
        let t = Timer {
            sub: sub.clone(),
            every,
            f: Rc::new(RefCell::new(f)),
        };
        self.arm(self.clock.now().saturating_add(d), t);
        Sub(sub)
    }
    /// Put a timer in the heap. A cancelled one goes when it comes due, or in the sweep here, so
    /// a long timer set and cancelled on every key can't pile up.
    fn arm(&mut self, at: Duration, t: Timer) {
        self.next_id += 1;
        if self.next_id.is_multiple_of(1024) {
            self.timers.retain(|_, t| t.sub.alive.get());
        }
        self.timers.insert((at, self.next_id), t);
    }
    /// When the next timer is due, on the clock [`Ctx::now`] reads: a time already past if one is
    /// overdue, and None with no timer set. A host sleeps until then or until it's woken.
    pub fn next_timer(&mut self) -> Option<Duration> {
        while let Some(e) = self.timers.first_entry() {
            if e.get().sub.alive.get() {
                break;
            }
            e.remove();
        }
        let heap = self.timers.keys().next().map(|k| k.0);
        let idle = self.idle.iter().filter(|t| t.sub.alive.get());
        idle.filter_map(|t| t.at).chain(heap).min()
    }
    /// The time on this workspace's clock.
    pub fn now(&self) -> Duration {
        self.clock.now()
    }
    /// A key came: every idle timer's wait starts again.
    pub(crate) fn restart_idle(&mut self) {
        let now = self.clock.now();
        for t in &mut self.idle {
            t.at = Some(now.saturating_add(t.d));
        }
    }
    /// A handle other threads can wake the loop with.
    pub fn waker(&self) -> Waker {
        self.waker.clone()
    }
    /// Connect what wakes the loop, once: R3 gives it a function that wakes the event loop. It
    /// runs on whichever thread called [`Waker::wake`], so it should only pass the news on. A
    /// second call is an error.
    pub fn set_waker(&mut self, f: impl Fn() + Send + Sync + 'static) -> Result {
        let connected = self.waker.0.host.set(Box::new(f));
        connected.map_err(|_| Error::Msg("the waker is already connected".into()))
    }
    /// Run `f` after a buffer is killed, from the top of the loop (C3).
    pub fn on_buffer_killed(
        &mut self,
        f: impl FnMut(&mut Ctx, BufferId) -> Result + 'static,
    ) -> Sub {
        self.new_sub(SubKind::Killed(Rc::new(RefCell::new(f))))
    }
    /// `work` runs on a thread of its own; `done` runs on the main thread from the top of the
    /// loop. When `work` ends, the loop is woken. If it panics, the panic is logged for the
    /// extension that spawned it, and `done` doesn't run.
    pub fn spawn<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Ctx, T) -> Result + 'static,
    ) -> Sub {
        self.next_id += 1;
        let id = self.next_id;
        let f: JobDone = Box::new(move |ctx, v| done(ctx, *v.downcast::<T>().expect("job type")));
        let ext = self.cur_ext();
        let (tx, waker) = (self.jobs_tx.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let v = catch(work).map(|v| Box::new(v) as Box<dyn Any + Send>);
            let _ = tx.send((id, ext, v));
            waker.wake();
        });
        self.outstanding += 1;
        self.new_sub(SubKind::Job { id, f })
    }
    /// See every key before dispatch. No Ctx, so it runs synchronously (C3). For `q` recording.
    pub fn observe_keys(&mut self, f: impl FnMut(&KeyEvent, KeySource) + 'static) -> Sub {
        self.new_sub(SubKind::Keys(RefCell::new(Box::new(f))))
    }

    // -------------------------------------------------------------- the loop

    /// Harness: move a fake clock on by `d`, stopping at each timer's deadline and running until
    /// idle there, so a callback reads its own deadline and a timer it sets counts from it. A
    /// real clock can't be moved, so this then only runs until idle.
    pub fn advance(&mut self, d: Duration) {
        self.run_until_idle();
        let Clock::Fake(mut now) = self.clock else {
            return;
        };
        let end = now.saturating_add(d);
        while let Some(at) = self.next_timer().filter(|at| *at > now && *at <= end) {
            now = at;
            self.clock = Clock::Fake(now);
            self.run_until_idle();
        }
        self.clock = Clock::Fake(end);
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
        // Before looking in the channel, so a job that ends after this wakes the host again.
        self.waker.0.pending.swap(false, Ordering::AcqRel);
        while let Ok(m) = self.jobs_rx.try_recv() {
            self.accept_job(m);
        }
        let now = self.clock.now();
        while let Some(e) = self.timers.first_entry().filter(|e| e.key().0 <= now) {
            let ((at, _), t) = e.remove_entry();
            if !t.sub.alive.get() {
                continue;
            }
            self.queue.push_back(fire(&t.sub, &t.f));
            if let Some(d) = t.every {
                let on_time = at.saturating_add(d);
                let next = if on_time > now {
                    on_time
                } else {
                    now.saturating_add(d)
                };
                if next > now {
                    self.arm(next, t);
                }
            }
        }
        for t in &mut self.idle {
            if t.sub.alive.get() && t.at.is_some_and(|at| at <= now) {
                t.at = None;
                self.queue.push_back(fire(&t.sub, &t.f));
            }
        }
        self.idle.retain(|t| t.sub.alive.get());
        self.subs.retain(|e| e.inner.alive.get());
    }

    /// One turn of the loop: queued calls first (C3), then one key (fed keys before typed ones).
    /// False if nothing was ready. It never waits, so a host can call it until it returns false.
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

    /// Harness: take turns until nothing is ready and no job is out, waiting for jobs, up to 5 s
    /// for each. A host can't wait, so it uses [`Ctx::step`]. If a call keeps queueing another,
    /// this stops after a million turns and says so in `*messages*`.
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
            let handles = self.subs.iter().map(|e| &e.inner);
            let handles = handles.chain(self.timers.values().map(|t| &t.sub));
            let handles = handles.chain(self.idle.iter().map(|t| &t.sub));
            for h in handles.filter(|h| h.ext == i) {
                h.alive.set(false);
            }
        }
    }
}
