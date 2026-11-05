//! K5: the channel and its `Waker` (contract §12). Other threads tell the loop there's work without
//! knowing what runs it: R3 connects the wake function to the event loop.
//!
//! - `a_finished_job_wakes_the_loop`: with no turn of the loop in between.
//! - `wakes_coalesce_until_the_loop_takes_a_turn`: many wakes, one call, then it arms again.
//! - `jobs_that_finish_between_turns_wake_once`: three jobs, one wake, and every `done` runs.
//! - `a_wake_from_a_callback_wakes_again`: the turn it runs in has already looked.
//! - `a_job_that_ends_as_the_loop_looks_wakes_again`: the loop arms the waker, then looks.
//! - `a_job_finished_before_the_waker_connects_is_still_delivered`: nothing is lost at startup.
//! - `connecting_a_second_waker_is_an_error`: and the first stays connected.
//! - `a_waker_works_from_any_thread_and_outlives_the_ctx`: it's `Send`, `Sync` and `Clone`.
//! - `no_wake_is_lost_under_load`: a host that sleeps until woken sees every job finish.
//! - `step_never_waits_for_a_job`: what K9's loop builds on. `run_until_idle` still waits.

use die_test_cap as _;

use die_core::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::sleep;
use std::time::{Duration, Instant};

const SECOND: Duration = Duration::from_secs(1);

fn connected() -> (Ctx, Arc<AtomicUsize>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let n = Arc::new(AtomicUsize::new(0));
    let m = n.clone();
    ctx.set_waker(move || {
        m.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    (ctx, n)
}

/// Wait for a condition another thread makes true.
fn wait_for(what: &str, f: impl Fn() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(
            start.elapsed() < 10 * SECOND,
            "timed out waiting for {what}"
        );
        sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_finished_job_wakes_the_loop() {
    let (mut ctx, wakes) = connected();
    let sub = ctx.spawn(|| 1, |_, _: i32| Ok(()));
    wait_for("the wake", || wakes.load(Ordering::SeqCst) == 1);
    ctx.run_until_idle();
    drop(sub);
}

#[test]
fn wakes_coalesce_until_the_loop_takes_a_turn() {
    let (mut ctx, wakes) = connected();
    let w = ctx.waker();
    w.wake();
    w.wake();
    w.clone().wake();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(!ctx.step(), "a wake alone is no work");
    w.wake();
    w.wake();
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "the turn armed it again");
}

#[test]
fn jobs_that_finish_between_turns_wake_once() {
    let (mut ctx, wakes) = connected();
    let finished = Arc::new(AtomicUsize::new(0));
    let done = Rc::new(Cell::new(0));
    for _ in 0..3 {
        let (f, d) = (finished.clone(), done.clone());
        ctx.spawn(
            move || {
                f.fetch_add(1, Ordering::SeqCst);
            },
            move |_, _| {
                d.set(d.get() + 1);
                Ok(())
            },
        )
        .detach();
    }
    wait_for("the jobs", || finished.load(Ordering::SeqCst) == 3);
    wait_for("a wake", || wakes.load(Ordering::SeqCst) >= 1);
    sleep(Duration::from_millis(50)); // for a wake that shouldn't come
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    ctx.run_until_idle();
    assert_eq!(done.get(), 3);
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    let sub = ctx.spawn(|| (), |_, _| Ok(()));
    wait_for("a second wake", || wakes.load(Ordering::SeqCst) == 2);
    ctx.run_until_idle();
    drop(sub);
}

#[test]
fn a_wake_from_a_callback_wakes_again() {
    let (mut ctx, wakes) = connected();
    let w = ctx.waker();
    w.wake();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    ctx.defer(move |_| {
        w.wake();
        Ok(())
    });
    ctx.run_until_idle();
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
}

/// If the loop looked in the channel first and armed the waker after, a job ending in between
/// wouldn't wake the host, and its result would wait for the next event. A cancelled job's value
/// drops while the loop is looking, which is the one place a test can act in that gap.
#[test]
fn a_job_that_ends_as_the_loop_looks_wakes_again() {
    struct WakesOnDrop(Waker);
    impl Drop for WakesOnDrop {
        fn drop(&mut self) {
            self.0.wake();
        }
    }
    let (mut ctx, wakes) = connected();
    let w = ctx.waker();
    let sub = ctx.spawn(move || WakesOnDrop(w), |_, _| Ok(()));
    drop(sub);
    wait_for("the job's wake", || wakes.load(Ordering::SeqCst) == 1);
    while ctx.step() {}
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
}

#[test]
fn a_job_finished_before_the_waker_connects_is_still_delivered() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let finished = Arc::new(AtomicUsize::new(0));
    let f = finished.clone();
    let done = Rc::new(Cell::new(false));
    let d = done.clone();
    let sub = ctx.spawn(
        move || {
            f.fetch_add(1, Ordering::SeqCst);
        },
        move |_, _| {
            d.set(true);
            Ok(())
        },
    );
    wait_for("the job", || finished.load(Ordering::SeqCst) == 1);
    let wakes = Arc::new(AtomicUsize::new(0));
    let m = wakes.clone();
    ctx.set_waker(move || {
        m.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    ctx.run_until_idle();
    assert!(done.get(), "the host's first turn finds it");
    ctx.waker().wake();
    assert!(
        wakes.load(Ordering::SeqCst) >= 1,
        "and later wakes reach the host"
    );
    drop(sub);
}

#[test]
fn connecting_a_second_waker_is_an_error() {
    let (mut ctx, wakes) = connected();
    let other = Arc::new(AtomicUsize::new(0));
    let o = other.clone();
    let r = ctx.set_waker(move || {
        o.fetch_add(1, Ordering::SeqCst);
    });
    assert!(r.is_err());
    ctx.waker().wake();
    assert_eq!(wakes.load(Ordering::SeqCst), 1, "the first stays connected");
    assert_eq!(other.load(Ordering::SeqCst), 0);
}

#[test]
fn a_waker_works_from_any_thread_and_outlives_the_ctx() {
    fn is_handle<T: Send + Sync + Clone + 'static>(t: T) -> T {
        t
    }
    let (ctx, wakes) = connected();
    let w = is_handle(ctx.waker());
    drop(ctx);
    std::thread::spawn(move || w.wake()).join().unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
}

/// Sleep until woken, as a host's event loop does, and run what's ready. A lost wake leaves it
/// asleep with a finished job waiting, and the test times out.
#[test]
fn no_wake_is_lost_under_load() {
    let woken = Arc::new((Mutex::new(false), Condvar::new()));
    let mut ctx = Ctx::new(vec![]).unwrap();
    let w = woken.clone();
    ctx.set_waker(move || {
        *w.0.lock().unwrap() = true;
        w.1.notify_one();
    })
    .unwrap();
    let done = Rc::new(Cell::new(0));
    for _ in 0..200 {
        for _ in 0..20 {
            let d = done.clone();
            ctx.spawn(
                || (),
                move |_, _| {
                    d.set(d.get() + 1);
                    Ok(())
                },
            )
            .detach();
        }
        let target = done.get() + 20;
        while done.get() < target {
            let mut flag = woken.0.lock().unwrap();
            while !*flag {
                let (f, t) = woken.1.wait_timeout(flag, 10 * SECOND).unwrap();
                assert!(!t.timed_out(), "a wake was lost: {} done", done.get());
                flag = f;
            }
            *flag = false;
            drop(flag);
            while ctx.step() {}
        }
    }
}

#[test]
fn step_never_waits_for_a_job() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let (tx, rx) = mpsc::channel::<u8>();
    let done = Rc::new(Cell::new(0));
    let d = done.clone();
    let sub = ctx.spawn(
        move || rx.recv().unwrap(),
        move |_, v| {
            d.set(v);
            Ok(())
        },
    );
    let start = Instant::now();
    while ctx.step() {}
    assert!(
        start.elapsed() < SECOND,
        "the job can't finish until we send"
    );
    assert_eq!(done.get(), 0);
    tx.send(7).unwrap();
    ctx.run_until_idle();
    assert_eq!(done.get(), 7, "run_until_idle waits for it");
    drop(sub);
}
