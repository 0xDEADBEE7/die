//! The one poll timer, on the harness's fake clock: slow while anything subscribes, fast while
//! one says so, and none at all with no orchid buffer, since a subscription kept in a buffer's
//! mode goes when the buffer is killed.

use die_test_cap as _;

use die_core::{BufferId, Ctx, Extension, Harness, Mode, Registry, Result};
use die_orchid_client::Orchid;
use die_orchid_client::cli::FakeRunner;
use die_orchid_client::fixtures::TempRoot;
use die_orchid_client::poll::{self, FAST, Polling, SLOW};
use die_vim::Vim;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// A buffer's mode that holds its subscription, as a chat's will.
struct Follows(Option<Polling>);

impl Mode for Follows {}

/// An extension of the test's own that declares the mode.
struct Test;

impl Extension for Test {
    fn name(&self) -> &'static str {
        "test"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode("follows", || Follows(None));
    }
}

fn harness(root: &TempRoot) -> Harness {
    let orchid = Orchid::for_tests(root.path(), FakeRunner::new());
    Harness::new(vec![
        Box::new(Vim::default()),
        Box::new(orchid),
        Box::new(Test),
    ])
    .unwrap()
}

/// A buffer whose mode subscribes, counting its ticks in `ticks`.
fn following(h: &mut Harness, name: &str, ticks: &Rc<Cell<u32>>) -> BufferId {
    let buf = h.create_text(name, "");
    h.enable_mode::<Follows>(buf).unwrap();
    let count = ticks.clone();
    let tick = move |_: &mut Ctx| -> Result {
        count.set(count.get() + 1);
        Ok(())
    };
    let p = poll::subscribe(h, tick).unwrap();
    h.with_mode(buf, |m: &mut Follows, _| m.0 = Some(p))
        .unwrap();
    buf
}

fn period(h: &mut Harness) -> Option<Duration> {
    h.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap()
}

fn set_fast(h: &mut Harness, buf: BufferId, fast: bool) {
    let p = h
        .with_mode(buf, |m: &mut Follows, _| m.0.take())
        .unwrap()
        .unwrap();
    poll::set_fast(h, &p, fast).unwrap();
    h.with_mode(buf, |m: &mut Follows, _| m.0 = Some(p))
        .unwrap();
}

#[test]
fn no_orchid_buffer_no_timer() {
    let root = TempRoot::new("poll-none");
    let mut h = harness(&root);
    h.run_until_idle();
    assert_eq!(h.next_timer(), None);
    assert_eq!(period(&mut h), None);
    let ticks = Rc::new(Cell::new(0));
    let buf = following(&mut h, "chat", &ticks);
    assert_eq!(period(&mut h), Some(SLOW));
    assert_eq!(h.next_timer(), Some(SLOW));
    h.advance(SLOW * 3);
    assert_eq!(ticks.get(), 3);
    // Killed, its subscription goes with its mode, and the timer at once.
    assert!(h.kill(buf));
    h.run_until_idle();
    assert_eq!(period(&mut h), None);
    assert_eq!(h.next_timer(), None);
    h.advance(SLOW * 3);
    assert_eq!(ticks.get(), 3);
    assert!(h.take_errors().is_empty());
}

#[test]
fn fast_while_any_subscriber_says_so() {
    let root = TempRoot::new("poll-fast");
    let mut h = harness(&root);
    let (a_ticks, b_ticks) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let a = following(&mut h, "a", &a_ticks);
    let b = following(&mut h, "b", &b_ticks);
    assert_eq!(
        h.with_ext(|o: &mut Orchid, _| o.poll.subscribers())
            .unwrap(),
        2
    );
    h.advance(SLOW);
    assert_eq!(
        (a_ticks.get(), b_ticks.get()),
        (1, 1),
        "one timer, both subscribers"
    );
    set_fast(&mut h, a, true);
    assert_eq!(period(&mut h), Some(FAST));
    h.advance(FAST * 4);
    assert_eq!((a_ticks.get(), b_ticks.get()), (5, 5));
    // The fast one gone, the rest is slow again.
    assert!(h.kill(a));
    h.run_until_idle();
    assert_eq!(period(&mut h), Some(SLOW));
    h.advance(SLOW);
    assert_eq!(b_ticks.get(), 6);
    set_fast(&mut h, b, true);
    set_fast(&mut h, b, false);
    assert_eq!(period(&mut h), Some(SLOW));
}

#[test]
fn a_dropped_subscription_stops_the_timer_at_the_next_tick() {
    let root = TempRoot::new("poll-drop");
    let mut h = harness(&root);
    let ticks = Rc::new(Cell::new(0));
    let buf = following(&mut h, "chat", &ticks);
    let p = h.with_mode(buf, |m: &mut Follows, _| m.0.take()).unwrap();
    drop(p);
    assert_eq!(period(&mut h), Some(SLOW), "nothing has told it yet");
    h.advance(SLOW);
    assert_eq!(ticks.get(), 0);
    assert_eq!(period(&mut h), None);
    assert_eq!(h.next_timer(), None);
}

#[test]
fn a_failing_subscriber_doesnt_stop_the_others() {
    let root = TempRoot::new("poll-fail");
    let mut h = harness(&root);
    let failing = poll::subscribe(&mut h, |_| {
        Err(die_core::Error::Msg("this one fails".into()))
    })
    .unwrap();
    let ticks = Rc::new(Cell::new(0));
    following(&mut h, "chat", &ticks);
    let errors = h.advance(SLOW);
    assert_eq!(errors, ["this one fails"]);
    assert_eq!(ticks.get(), 1);
    drop(failing);
}
