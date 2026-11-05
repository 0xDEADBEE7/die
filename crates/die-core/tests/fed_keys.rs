//! K5: an error drops the rest of the fed keys (contract §6), so a macro stops where it fails, as
//! Emacs's keyboard macros do.
//!
//! - `an_error_drops_the_rest_of_the_fed_keys`: `Err` from a key's command; `b` never runs.
//! - `a_panic_drops_the_rest_of_the_fed_keys`: the same for a command that panics.
//! - `an_undefined_fed_key_drops_the_rest`: a key nothing handles, and a prefix it ends.
//! - `an_error_in_a_surfaces_event_drops_the_rest`: a panic in `event`, in and out of passthrough.
//! - `a_key_a_surface_handles_drops_nothing`: `event` returning true isn't an error.
//! - `typed_keys_survive_the_drop`: only fed keys go.
//! - `a_typed_key_that_fails_drops_what_it_fed`: `@a` feeding keys, then failing.
//! - `what_a_later_command_feeds_runs_again`: the drop isn't sticky.
//! - `a_callback_error_drops_nothing`: a timer failing mid-macro isn't part of the key stream.
//! - `an_error_the_command_handles_drops_nothing`: a nested `run` that failed, then `Ok`.
//! - `the_keys_a_drop_removes_are_never_observed`: a key observer sees only what dispatched.
mod support;
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

type Log = Rc<RefCell<Vec<String>>>;

struct Ext {
    log: Log,
}
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "ext"
    }
    fn register(&mut self, reg: &mut Registry) {
        let k = reg.kind_keymap("text");
        k.bind(State::Normal, "a", "note-a")
            .bind(State::Normal, "b", "note-b")
            .bind(State::Normal, "c", "note-c")
            .bind(State::Normal, "e", "fail")
            .bind(State::Normal, "p", "boom")
            .bind(State::Normal, "f", "feed-then-fail")
            .bind(State::Normal, "g", "feed-b")
            .bind(State::Normal, "h", "handles-a-failure")
            .bind(State::Normal, "zz", "note-c");
        for name in ["note-a", "note-b", "note-c"] {
            let log = self.log.clone();
            reg.command(name, "", move |_, _| {
                log.borrow_mut().push(name.into());
                Ok(())
            });
        }
        reg.command("fail", "", |_, _| Err(Error::Msg("failed".into())));
        reg.command("boom", "", |_, _| panic!("boom"));
        reg.command("feed-then-fail", "", |ctx, _| {
            ctx.feed_keys("b")?;
            Err(Error::Msg("failed".into()))
        });
        reg.command("feed-b", "", |ctx, _| ctx.feed_keys("b"));
        let log = self.log.clone();
        reg.command("handles-a-failure", "", move |ctx, a| {
            let r = ctx.run("fail", a);
            log.borrow_mut().push(format!("handled {}", r.is_err()));
            Ok(())
        });
    }
}

fn setup() -> (Ctx, Log) {
    support::quiet_panics();
    let log: Log = Rc::default();
    let mut ctx = Ctx::new(vec![Box::new(Ext { log: log.clone() })]).unwrap();
    let b = ctx.create_text("t", "text");
    ctx.open_window(b, 80, 24).unwrap();
    (ctx, log)
}

fn run_fed(ctx: &mut Ctx, keys: &str) {
    ctx.feed_keys(keys).unwrap();
    ctx.run_until_idle();
}

#[test]
fn an_error_drops_the_rest_of_the_fed_keys() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "aeb");
    assert_eq!(*log.borrow(), ["note-a"]);
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn a_panic_drops_the_rest_of_the_fed_keys() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "apb");
    assert_eq!(*log.borrow(), ["note-a"]);
}

#[test]
fn an_undefined_fed_key_drops_the_rest() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "aQb");
    assert_eq!(*log.borrow(), ["note-a"], "Q is bound to nothing");
    log.borrow_mut().clear();
    ctx.messages.clear();
    run_fed(&mut ctx, "zQb");
    assert!(log.borrow().is_empty(), "Q ends the prefix z began");
    assert_eq!(ctx.messages, ["zQ is undefined"]);
}

#[test]
fn an_error_in_a_surfaces_event_drops_the_rest() {
    struct Boom;
    impl Surface for Boom {
        fn kind(&self) -> &'static str {
            "boom"
        }
        fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
        fn event(&mut self, _: &Event, _: &mut Ctx) -> bool {
            panic!("event bug")
        }
    }
    struct Passing;
    impl Surface for Passing {
        fn kind(&self) -> &'static str {
            "passing"
        }
        fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
        fn event(&mut self, _: &Event, _: &mut Ctx) -> bool {
            panic!("event bug")
        }
        fn initial_state(&self) -> State {
            State::Passthrough
        }
    }
    for passthrough in [false, true] {
        let (mut ctx, _) = setup();
        let b = if passthrough {
            ctx.create_surface("s", Passing)
        } else {
            ctx.create_surface("s", Boom)
        };
        ctx.open_window(b, 80, 24).unwrap();
        run_fed(&mut ctx, "xyz");
        let panics = ctx.messages.iter().filter(|m| m.contains("event bug"));
        assert_eq!(panics.count(), 1, "x only: {:?}", ctx.messages);
    }
}

#[test]
fn a_key_a_surface_handles_drops_nothing() {
    struct Takes(Log);
    impl Surface for Takes {
        fn kind(&self) -> &'static str {
            "takes"
        }
        fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
        fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
            self.0
                .borrow_mut()
                .push(format!("{ev:?}").len().to_string());
            true
        }
    }
    let (mut ctx, _) = setup();
    let seen: Log = Rc::default();
    let b = ctx.create_surface("s", Takes(seen.clone()));
    ctx.open_window(b, 80, 24).unwrap();
    run_fed(&mut ctx, "xyz");
    assert_eq!(seen.borrow().len(), 3);
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn typed_keys_survive_the_drop() {
    let (mut ctx, log) = setup();
    ctx.input(KeyEvent::press(Key::Char('c')));
    ctx.feed_keys("eb").unwrap();
    ctx.run_until_idle();
    assert_eq!(
        *log.borrow(),
        ["note-c"],
        "fed keys go first, and only they go"
    );
}

#[test]
fn a_typed_key_that_fails_drops_what_it_fed() {
    let (mut ctx, log) = setup();
    ctx.type_keys("f");
    assert!(log.borrow().is_empty());
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn what_a_later_command_feeds_runs_again() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "eb");
    ctx.type_keys("g");
    assert_eq!(*log.borrow(), ["note-b"]);
}

#[test]
fn a_callback_error_drops_nothing() {
    let (mut ctx, log) = setup();
    let t = ctx.after(Duration::from_millis(1), |_| {
        Err(Error::Msg("callback failed".into()))
    });
    ctx.feed_keys("ab").unwrap();
    ctx.advance(Duration::from_millis(1));
    assert_eq!(*log.borrow(), ["note-a", "note-b"]);
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    drop(t);
}

#[test]
fn an_error_the_command_handles_drops_nothing() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "hb");
    assert_eq!(*log.borrow(), ["handled true", "note-b"]);
}

#[test]
fn the_keys_a_drop_removes_are_never_observed() {
    let (mut ctx, _) = setup();
    let seen = Rc::new(RefCell::new(vec![]));
    let s = seen.clone();
    let sub = ctx.observe_keys(move |ev, src| s.borrow_mut().push((ev.key.clone(), src)));
    run_fed(&mut ctx, "aeb");
    assert_eq!(
        *seen.borrow(),
        [
            (Key::Char('a'), KeySource::Fed),
            (Key::Char('e'), KeySource::Fed)
        ]
    );
    drop(sub);
}
