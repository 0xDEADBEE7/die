//! The one poll timer all of orchid shares. The Emacs client ran a 0.5 s timer per open chat, a
//! 1 s one per indicator and a 300 s idle one from startup; DIE aims at no work while idle, so
//! here there's one timer, and only while something wants it:
//!
//! - every [`FAST`] (0.5 s) while a subscriber says it's fast: a chat whose session is running,
//!   or whose send hasn't come back;
//! - every [`SLOW`] (2 s) while any subscriber is left, to see what changed elsewhere, as a turn
//!   you started from Emacs;
//! - not at all with none, as when no orchid buffer is open.
//!
//! A subscription is a [`Polling`] handle: dropping it unsubscribes, so one kept in a buffer's
//! mode goes when the buffer is killed (C4). The timer stops at the kill itself, as the extension
//! hears of every kill, or else at the next tick. Its subscribers run in the order they came,
//! with the extension put back, so they can check it out themselves; a tick runs each of them
//! even if one fails, then reports the first failure.

use crate::Orchid;
use die_core::{Ctx, Result, Sub};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

/// The period while a subscriber is fast.
pub const FAST: Duration = Duration::from_millis(500);
/// The period otherwise.
pub const SLOW: Duration = Duration::from_secs(2);

type TickFn = RefCell<Box<dyn FnMut(&mut Ctx) -> Result>>;

struct Entry {
    f: TickFn,
    fast: Cell<bool>,
}

/// A subscription to the poll. Dropping it unsubscribes. Don't capture it in its own callback,
/// or it never drops.
#[must_use = "dropping a Polling unsubscribes it; keep it in the buffer's mode"]
pub struct Polling(Rc<Entry>);

impl Polling {
    /// Whether it wants the fast period.
    pub fn is_fast(&self) -> bool {
        self.0.fast.get()
    }
}

/// The poll: its subscribers and its timer.
#[derive(Default)]
pub struct Poller {
    subs: Vec<Weak<Entry>>,
    timer: Option<(Sub, Duration)>,
}

impl Poller {
    /// Run `f` at each tick from now on, at the slow period until [`Poller::set_fast`] says
    /// otherwise. The first subscription starts the timer.
    pub fn subscribe(
        &mut self,
        ctx: &mut Ctx,
        f: impl FnMut(&mut Ctx) -> Result + 'static,
    ) -> Polling {
        let entry = Rc::new(Entry {
            f: RefCell::new(Box::new(f)),
            fast: Cell::new(false),
        });
        self.subs.push(Rc::downgrade(&entry));
        self.settle(ctx);
        Polling(entry)
    }

    /// Say whether `p` wants the fast period, and set the timer to what all of them want.
    pub fn set_fast(&mut self, ctx: &mut Ctx, p: &Polling, fast: bool) {
        p.0.fast.set(fast);
        self.settle(ctx);
    }

    /// The period it ticks at, or None with no timer.
    pub fn period(&self) -> Option<Duration> {
        self.timer.as_ref().map(|t| t.1)
    }

    /// How many subscriptions are left.
    pub fn subscribers(&self) -> usize {
        self.subs.iter().filter(|w| w.strong_count() > 0).count()
    }

    /// Forget the subscriptions dropped, and set the timer to what's left wants. A timer whose
    /// period stays is left as it is, so its ticks keep their phase.
    pub fn settle(&mut self, ctx: &mut Ctx) {
        self.subs.retain(|w| w.strong_count() > 0);
        let fast = self
            .subs
            .iter()
            .filter_map(Weak::upgrade)
            .any(|e| e.fast.get());
        let want = match (self.subs.is_empty(), fast) {
            (true, _) => None,
            (false, true) => Some(FAST),
            (false, false) => Some(SLOW),
        };
        if self.period() != want {
            self.timer = want.map(|d| (ctx.every(d, tick), d));
        }
    }

    /// The live subscriptions, in the order they came.
    fn live(&self) -> Vec<Rc<Entry>> {
        self.subs.iter().filter_map(Weak::upgrade).collect()
    }
}

/// One tick: each subscriber in turn, with the extension put back, then the timer set again for
/// what they want now.
fn tick(ctx: &mut Ctx) -> Result {
    let live = ctx.with_ext(|o: &mut Orchid, ctx| {
        o.poll.settle(ctx);
        o.poll.live()
    })?;
    let mut failed = None;
    for e in live {
        // One already running is calling the loop itself: it isn't run again inside.
        let Ok(mut f) = e.f.try_borrow_mut() else {
            continue;
        };
        if let Err(why) = f(ctx) {
            failed.get_or_insert(why);
        }
    }
    ctx.with_ext(|o: &mut Orchid, ctx| o.poll.settle(ctx))?;
    failed.map_or(Ok(()), Err)
}

/// [`Poller::subscribe`] on orchid's poll, from outside the extension.
pub fn subscribe(ctx: &mut Ctx, f: impl FnMut(&mut Ctx) -> Result + 'static) -> Result<Polling> {
    Ok(ctx.with_ext(|o: &mut Orchid, ctx| o.poll.subscribe(ctx, f))?)
}

/// [`Poller::set_fast`] on orchid's poll, from outside the extension.
pub fn set_fast(ctx: &mut Ctx, p: &Polling, fast: bool) -> Result {
    Ok(ctx.with_ext(|o: &mut Orchid, ctx| o.poll.set_fast(ctx, p, fast))?)
}

/// [`Poller::settle`] on orchid's poll, from outside the extension.
pub fn settle(ctx: &mut Ctx) -> Result {
    Ok(ctx.with_ext(|o: &mut Orchid, ctx| o.poll.settle(ctx))?)
}
