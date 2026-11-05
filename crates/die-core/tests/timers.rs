//! K5: the timer heap, the next deadline, and the real and fake clocks (contract §12, §16).
//!
//! Order:
//! - `timers_fire_in_deadline_order`: not the order they were set.
//! - `ties_fire_in_the_order_they_were_set`.
//! - `repeats_interleave_by_deadline`: a repeat's firings take their turn among other timers.
//! - `advance_runs_each_timer_at_its_own_time`: a callback reads its own deadline from the clock.
//! - `a_timer_set_from_a_callback_counts_from_then`: a retry chain runs inside one `advance`.
//!
//! Cancelling:
//! - `a_timer_cancelled_when_already_due_doesnt_fire`: by an earlier callback in the same turn.
//! - `a_repeating_timer_can_cancel_itself`.
//! - `cancelled_timers_dont_pile_up`: what they hold is freed before their deadlines.
//! - `a_cancelled_repeat_is_freed_when_it_comes_due`: it isn't set again.
//!
//! Limits:
//! - `every_with_no_period_lets_the_loop_go_idle`: a zero period runs as one millisecond.
//! - `a_huge_delay_doesnt_overflow`: `Duration::MAX` as a delay, a period and an advance.
//!
//! The next deadline:
//! - `next_timer_is_none_with_nothing_set`, and after the last one fires.
//! - `next_timer_is_the_earliest_live_deadline`: skips what was cancelled, and moves on as they fire.
//! - `next_timer_follows_a_repeat`.
//! - `next_timer_forgets_what_shutdown_cancelled`.
//! - `a_new_earlier_timer_moves_next_timer`.
//!
//! Clocks:
//! - `the_fake_clock_moves_only_when_advanced`: from any start, and turns of the loop don't move it.
//! - `the_real_clock_runs_timers_with_no_advance`: and `next_timer` shows one overdue.
//! - `advance_doesnt_move_a_real_clock`.
//! - `a_repeat_that_fell_behind_skips_what_it_missed`: on a real clock, after a sleep or a stall.
//! - `a_repeat_keeps_its_phase`: being a little late doesn't shift the ones after.
//!
//! Idle timers (`run-with-idle-timer`):
//! - `an_idle_timer_fires_once_per_idle_period`.
//! - `a_key_restarts_the_idle_wait`.
//! - `an_idle_timer_waits_its_whole_period_when_set`.
//! - `dropping_an_idle_handle_cancels_it`, and shutdown does too.
//! - `next_timer_counts_idle_timers`, and forgets one that has fired until the next key.
//! - `a_panic_in_an_idle_timer_is_logged`.
use die_core::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

const MS: Duration = Duration::from_millis(1);

type Log = Rc<RefCell<Vec<String>>>;

fn ctx() -> (Ctx, Log) {
    (Ctx::new(vec![]).unwrap(), Rc::default())
}

/// A callback that logs its name and the clock's time.
fn note(log: &Log, name: &'static str) -> impl FnMut(&mut Ctx) -> Result + 'static {
    let log = log.clone();
    move |ctx| {
        let at = ctx.now().as_millis();
        log.borrow_mut().push(format!("{name}@{at}"));
        Ok(())
    }
}

fn counter() -> (Rc<Cell<u32>>, impl FnMut(&mut Ctx) -> Result + 'static) {
    let c = Rc::new(Cell::new(0));
    let c2 = c.clone();
    (c, move |_: &mut Ctx| {
        c2.set(c2.get() + 1);
        Ok(())
    })
}

#[test]
fn timers_fire_in_deadline_order() {
    let (mut ctx, log) = ctx();
    let subs = [
        ctx.after(30 * MS, note(&log, "c")),
        ctx.after(10 * MS, note(&log, "a")),
        ctx.after(20 * MS, note(&log, "b")),
    ];
    ctx.advance(30 * MS);
    assert_eq!(*log.borrow(), ["a@10", "b@20", "c@30"]);
    drop(subs);
}

#[test]
fn ties_fire_in_the_order_they_were_set() {
    let (mut ctx, log) = ctx();
    let subs = [
        ctx.after(10 * MS, note(&log, "first")),
        ctx.every(10 * MS, note(&log, "second")),
        ctx.after(10 * MS, note(&log, "third")),
    ];
    ctx.advance(10 * MS);
    assert_eq!(*log.borrow(), ["first@10", "second@10", "third@10"]);
    drop(subs);
}

#[test]
fn repeats_interleave_by_deadline() {
    let (mut ctx, log) = ctx();
    let subs = [
        ctx.every(10 * MS, note(&log, "a")),
        ctx.every(15 * MS, note(&log, "b")),
        ctx.after(12 * MS, note(&log, "c")),
    ];
    ctx.advance(30 * MS);
    // At 30 the two repeats tie, and b was set again first: at 15, where a was at 20.
    assert_eq!(
        *log.borrow(),
        ["a@10", "c@12", "b@15", "a@20", "b@30", "a@30"]
    );
    drop(subs);
}

#[test]
fn advance_runs_each_timer_at_its_own_time() {
    let (mut ctx, log) = ctx();
    let t = ctx.every(7 * MS, note(&log, "t"));
    ctx.advance(20 * MS);
    assert_eq!(*log.borrow(), ["t@7", "t@14"]);
    assert_eq!(ctx.now(), 20 * MS, "it ends where it was asked to");
    drop(t);
}

#[test]
fn a_timer_set_from_a_callback_counts_from_then() {
    fn retry(ctx: &mut Ctx, log: Log, left: u32) -> Result {
        log.borrow_mut()
            .push(format!("try@{}", ctx.now().as_millis()));
        if left > 0 {
            ctx.after(10 * MS, move |ctx| retry(ctx, log, left - 1))
                .detach();
        }
        Ok(())
    }
    let (mut ctx, log) = ctx();
    let l = log.clone();
    ctx.after(10 * MS, move |ctx| retry(ctx, l, 5)).detach();
    ctx.advance(35 * MS);
    assert_eq!(*log.borrow(), ["try@10", "try@20", "try@30"]);
    assert_eq!(ctx.next_timer(), Some(40 * MS));
}

#[test]
fn a_timer_cancelled_when_already_due_doesnt_fire() {
    let (mut ctx, _) = ctx();
    let victim: Rc<RefCell<Option<Sub>>> = Rc::default();
    let v = victim.clone();
    let first = ctx.after(10 * MS, move |_| {
        v.borrow_mut().take();
        Ok(())
    });
    let (n, f) = counter();
    *victim.borrow_mut() = Some(ctx.every(10 * MS, f));
    ctx.advance(30 * MS);
    assert_eq!(n.get(), 0);
    drop(first);
}

#[test]
fn a_repeating_timer_can_cancel_itself() {
    let (mut ctx, _) = ctx();
    let own: Rc<RefCell<Option<Sub>>> = Rc::default();
    let (o, n) = (own.clone(), Rc::new(Cell::new(0)));
    let m = n.clone();
    *own.borrow_mut() = Some(ctx.every(10 * MS, move |_| {
        m.set(m.get() + 1);
        if m.get() == 2 {
            o.borrow_mut().take();
        }
        Ok(())
    }));
    ctx.advance(100 * MS);
    assert_eq!(n.get(), 2);
    assert_eq!(ctx.next_timer(), None);
}

#[test]
fn cancelled_timers_dont_pile_up() {
    let (mut ctx, _) = ctx();
    let held = Rc::new(());
    for _ in 0..10_000 {
        let h = held.clone();
        drop(ctx.after(Duration::from_secs(3600), move |_| {
            let _ = &h;
            Ok(())
        }));
    }
    assert!(
        Rc::strong_count(&held) < 2_000,
        "{} still held, an hour before any is due",
        Rc::strong_count(&held)
    );
}

#[test]
fn a_cancelled_repeat_is_freed_when_it_comes_due() {
    let mut c = with_clock(Clock::real());
    let held = Rc::new(());
    let h = held.clone();
    let t = c.every(MS, move |_| {
        let _ = &h;
        Ok(())
    });
    drop(t);
    sleep(5 * MS);
    while c.step() {}
    assert_eq!(Rc::strong_count(&held), 1);
}

#[test]
fn every_with_no_period_lets_the_loop_go_idle() {
    let (mut ctx, _) = ctx();
    let (n, f) = counter();
    let t = ctx.every(Duration::ZERO, f);
    ctx.advance(3 * MS);
    assert_eq!(n.get(), 3, "as one millisecond");
    drop(t);
}

#[test]
fn a_huge_delay_doesnt_overflow() {
    let (mut ctx, _) = ctx();
    ctx.advance(Duration::from_secs(1));
    let (n, f) = counter();
    let a = ctx.after(Duration::MAX, f);
    let (m, g) = counter();
    let b = ctx.every(Duration::MAX, g);
    let (k, h) = counter();
    let c = ctx.idle(Duration::MAX, h);
    ctx.advance(Duration::from_secs(1));
    assert_eq!((n.get(), m.get(), k.get()), (0, 0, 0));
    ctx.input(KeyEvent::press(Key::Char('z'))); // restarts the idle wait
    ctx.run_until_idle();
    assert!(ctx.next_timer().is_some());
    ctx.advance(Duration::MAX);
    assert_eq!(
        ctx.now(),
        Duration::MAX,
        "the clock stops at the end of time"
    );
    assert_eq!((n.get(), m.get(), k.get()), (1, 1, 1));
    ctx.advance(Duration::from_secs(1));
    assert_eq!(
        (n.get(), m.get(), k.get()),
        (1, 1, 1),
        "and doesn't fire them forever"
    );
    drop((a, b, c));
}

#[test]
fn next_timer_is_none_with_nothing_set() {
    let (mut ctx, _) = ctx();
    assert_eq!(ctx.next_timer(), None);
    let (_, f) = counter();
    let t = ctx.after(5 * MS, f);
    assert_eq!(ctx.next_timer(), Some(5 * MS));
    ctx.advance(5 * MS);
    assert_eq!(ctx.next_timer(), None, "the last one has fired");
    drop(t);
}

#[test]
fn next_timer_is_the_earliest_live_deadline() {
    let (mut ctx, log) = ctx();
    let late = ctx.after(30 * MS, note(&log, "late"));
    let soon = ctx.after(10 * MS, note(&log, "soon"));
    let middle = ctx.after(20 * MS, note(&log, "middle"));
    assert_eq!(ctx.next_timer(), Some(10 * MS));
    drop(soon);
    assert_eq!(ctx.next_timer(), Some(20 * MS), "cancelled, so skipped");
    ctx.advance(20 * MS);
    assert_eq!(ctx.next_timer(), Some(30 * MS), "that one fired");
    assert_eq!(*log.borrow(), ["middle@20"]);
    drop((late, middle));
    assert_eq!(ctx.next_timer(), None);
}

#[test]
fn next_timer_follows_a_repeat() {
    let (mut ctx, _) = ctx();
    let (_, f) = counter();
    let t = ctx.every(10 * MS, f);
    assert_eq!(ctx.next_timer(), Some(10 * MS));
    ctx.advance(10 * MS);
    assert_eq!(ctx.next_timer(), Some(20 * MS));
    ctx.advance(3 * MS);
    assert_eq!(ctx.next_timer(), Some(20 * MS), "not due, so not moved");
    drop(t);
}

#[test]
fn next_timer_forgets_what_shutdown_cancelled() {
    struct Ext;
    impl Extension for Ext {
        fn name(&self) -> &'static str {
            "ext"
        }
        fn register(&mut self, _: &mut Registry) {}
        fn init(&mut self, ctx: &mut Ctx) -> Result {
            ctx.every(10 * MS, |_| Ok(())).detach();
            ctx.idle(10 * MS, |_| Ok(())).detach();
            Ok(())
        }
    }
    let mut ctx = Ctx::new(vec![Box::new(Ext)]).unwrap();
    assert_eq!(ctx.next_timer(), Some(10 * MS));
    ctx.shutdown();
    assert_eq!(ctx.next_timer(), None);
}

#[test]
fn a_new_earlier_timer_moves_next_timer() {
    let (mut ctx, log) = ctx();
    let late = ctx.after(100 * MS, note(&log, "late"));
    assert_eq!(ctx.next_timer(), Some(100 * MS));
    let soon = ctx.after(5 * MS, note(&log, "soon"));
    assert_eq!(ctx.next_timer(), Some(5 * MS));
    drop((late, soon));
}

fn with_clock(clock: Clock) -> Ctx {
    let cfg = Config {
        clock,
        ..Default::default()
    };
    Ctx::with_config(vec![], cfg).unwrap()
}

#[test]
fn the_fake_clock_moves_only_when_advanced() {
    let (mut c, log) = ctx();
    assert_eq!(c.now(), Duration::ZERO);
    c.defer(|_| Ok(()));
    c.run_until_idle();
    sleep(2 * MS);
    while c.step() {}
    assert_eq!(c.now(), Duration::ZERO, "time doesn't pass on its own");
    c.advance(5 * MS);
    assert_eq!(c.now(), 5 * MS);

    let mut c = with_clock(Clock::Fake(100 * MS));
    assert_eq!(c.now(), 100 * MS);
    let t = c.after(5 * MS, note(&log, "t"));
    assert_eq!(c.next_timer(), Some(105 * MS));
    c.advance(5 * MS);
    assert_eq!(*log.borrow(), ["t@105"]);
    drop(t);
}

#[test]
fn the_real_clock_runs_timers_with_no_advance() {
    let mut c = with_clock(Clock::real());
    let (n, f) = counter();
    let t = c.after(MS, f);
    let due = c.next_timer().unwrap();
    while c.now() < due {
        sleep(MS / 4);
    }
    assert!(c.now() >= due);
    assert_eq!(c.next_timer(), Some(due), "overdue, so a time already past");
    while c.step() {}
    assert_eq!(n.get(), 1);
    assert_eq!(c.next_timer(), None);
    drop(t);
}

#[test]
fn advance_doesnt_move_a_real_clock() {
    let mut c = with_clock(Clock::real());
    let (n, f) = counter();
    let t = c.after(Duration::from_secs(3600), f);
    c.advance(Duration::from_secs(7200));
    assert!(c.now() < Duration::from_secs(3600));
    assert_eq!(n.get(), 0);
    drop(t);
}

#[test]
fn a_repeat_that_fell_behind_skips_what_it_missed() {
    let mut c = with_clock(Clock::real());
    let (n, f) = counter();
    let t = c.every(20 * MS, f);
    sleep(400 * MS);
    let woke = c.now();
    while c.step() {}
    // Catching up in full fires 20 times here. A stall in this test may let a few through.
    assert!((1..=5).contains(&n.get()), "{} firings", n.get());
    assert!(c.next_timer().unwrap() > woke, "and it's on time again");
    drop(t);
}

#[test]
fn a_repeat_keeps_its_phase() {
    let mut c = with_clock(Clock::real());
    let (n, f) = counter();
    let t = c.every(200 * MS, f);
    let first = c.next_timer().unwrap();
    while c.now() < first {
        sleep(MS);
    }
    while c.step() {}
    assert_eq!(n.get(), 1);
    assert_eq!(
        c.next_timer(),
        Some(first + 200 * MS),
        "the next counts from the deadline, not from when it ran"
    );
    drop(t);
}

fn window(ctx: &mut Ctx) {
    let b = ctx.create_text("t", "text");
    ctx.open_window(b, 80, 24).unwrap();
}

#[test]
fn an_idle_timer_fires_once_per_idle_period() {
    let (mut ctx, _) = ctx();
    window(&mut ctx);
    let (n, f) = counter();
    let t = ctx.idle(10 * MS, f);
    ctx.advance(9 * MS);
    assert_eq!(n.get(), 0);
    ctx.advance(MS);
    assert_eq!(n.get(), 1);
    ctx.advance(100 * MS);
    assert_eq!(n.get(), 1, "the same idle period");
    ctx.type_keys("z");
    ctx.advance(10 * MS);
    assert_eq!(n.get(), 2, "a key began another");
    drop(t);
}

#[test]
fn a_key_restarts_the_idle_wait() {
    let (mut ctx, _) = ctx();
    window(&mut ctx);
    let (n, f) = counter();
    let t = ctx.idle(10 * MS, f);
    ctx.advance(8 * MS);
    ctx.type_keys("z");
    ctx.advance(8 * MS);
    assert_eq!(n.get(), 0, "16 ms since it was set, but 8 since the key");
    ctx.feed_keys("z").unwrap();
    ctx.run_until_idle();
    ctx.advance(8 * MS);
    assert_eq!(n.get(), 0, "a fed key counts as well");
    ctx.advance(2 * MS);
    assert_eq!(n.get(), 1);
    drop(t);
}

#[test]
fn an_idle_timer_waits_its_whole_period_when_set() {
    let (mut ctx, _) = ctx();
    window(&mut ctx);
    ctx.advance(100 * MS);
    let (n, f) = counter();
    let t = ctx.idle(10 * MS, f);
    ctx.run_until_idle();
    assert_eq!(
        n.get(),
        0,
        "already idle for 100 ms, but it counts from now"
    );
    ctx.advance(10 * MS);
    assert_eq!(n.get(), 1);
    drop(t);
}

#[test]
fn dropping_an_idle_handle_cancels_it() {
    let (mut ctx, _) = ctx();
    let (n, f) = counter();
    let t = ctx.idle(10 * MS, f);
    assert_eq!(ctx.next_timer(), Some(10 * MS));
    drop(t);
    assert_eq!(ctx.next_timer(), None);
    ctx.advance(50 * MS);
    assert_eq!(n.get(), 0);
}

#[test]
fn next_timer_counts_idle_timers() {
    let (mut ctx, _) = ctx();
    window(&mut ctx);
    let (_, f) = counter();
    let (_, g) = counter();
    let idle = ctx.idle(10 * MS, f);
    let later = ctx.after(50 * MS, g);
    assert_eq!(ctx.next_timer(), Some(10 * MS));
    ctx.advance(10 * MS);
    assert_eq!(
        ctx.next_timer(),
        Some(50 * MS),
        "it has fired for this idle period"
    );
    ctx.type_keys("z");
    assert_eq!(ctx.next_timer(), Some(20 * MS), "a key sets it again");
    drop((idle, later));
}

#[test]
fn a_panic_in_an_idle_timer_is_logged() {
    let (mut ctx, _) = ctx();
    window(&mut ctx);
    let t = ctx.idle(10 * MS, |_| panic!("idle bug"));
    ctx.advance(10 * MS);
    let lines = ctx.messages.iter().filter(|m| m.contains("idle bug"));
    assert_eq!(lines.count(), 1, "{:?}", ctx.messages);
    ctx.type_keys("z");
    ctx.advance(10 * MS);
    let fired = ctx.messages.iter().filter(|m| m.contains("idle bug"));
    assert_eq!(fired.count(), 2, "it goes on: {:?}", ctx.messages);
    drop(t);
}
