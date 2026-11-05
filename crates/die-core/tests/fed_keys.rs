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
//! - `a_queued_calls_error_drops_nothing`: nor is a deferred call, a job's `done` or a hook.
//! - `an_error_the_command_handles_drops_nothing`: a nested `run` that failed, then `Ok`.
//! - `the_keys_a_drop_removes_are_never_observed`: a key observer sees only what dispatched.
//! - `a_key_a_surface_ignores_in_passthrough_drops_nothing`: `event` returning false there.
//!
//! A prompt's answer is part of the key stream:
//! - `a_failing_prompt_answer_drops_the_rest`: `/nomatch<CR>dd`'s shape.
//! - `a_panicking_prompt_answer_drops_the_rest`.
//! - `a_prompt_answer_that_succeeds_drops_nothing`.
//! - `a_cancelled_prompt_whose_answer_fails_drops_the_rest`: `<Esc>` submits too.
//! - `a_prompt_a_new_prompt_cancels_drops_nothing`: no key submitted that answer.
//!
//! The order keys run in:
//! - `fed_keys_go_ahead_of_typed_ones`.
//! - `a_macro_inside_a_macro_runs_before_the_rest_of_it`: `@a` with `@b` inside it, as in Evil.
//! - `keys_fed_twice_in_one_turn_keep_their_order`: and both go ahead of what's left.
//! - `macros_nested_three_deep_finish_innermost_first`.
//! - `an_error_in_a_macro_inside_a_macro_ends_both`: as in Evil.
//! - `keys_the_host_feeds_go_behind_what_is_left`: outside any call, it adds to the end.
//! - `keys_fed_after_a_nested_turn_emptied_the_queue_still_run`: where they go is kept in range.

use die_test_cap as _;

mod support;
use die_core::*;
use std::cell::{Cell, RefCell};
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
            .bind(State::Normal, "zz", "note-c")
            .bind(State::Normal, ":", "ask-fails")
            .bind(State::Normal, "!", "ask-panics")
            .bind(State::Normal, "?", "ask-succeeds")
            .bind(State::Normal, "t", "feed-twice")
            .bind(State::Normal, "n", "feed-a-failure")
            .bind(State::Normal, "i", "feed-g-then-c")
            .bind(State::Normal, "N", "nested-turn-drops");
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
        reg.command("feed-twice", "", |ctx, _| {
            ctx.feed_keys("b")?;
            ctx.feed_keys("c")
        });
        reg.command("feed-a-failure", "", |ctx, _| ctx.feed_keys("e"));
        reg.command("feed-g-then-c", "", |ctx, _| ctx.feed_keys("gc"));
        reg.command("nested-turn-drops", "", |ctx, _| {
            ctx.feed_keys("f")?;
            ctx.step(); // f feeds b, fails, and so empties the queue
            ctx.feed_keys("c")
        });
        reg.command("ask-fails", "", |ctx, _| {
            ctx.prompt(":", |_, _| Err(Error::Msg("no match".into())));
            Ok(())
        });
        reg.command("ask-panics", "", |ctx, _| {
            ctx.prompt(":", |_, _| panic!("answer bug"));
            Ok(())
        });
        let log = self.log.clone();
        reg.command("ask-succeeds", "", move |ctx, _| {
            let log = log.clone();
            ctx.prompt("?", move |_, answer| {
                log.borrow_mut().push(format!("answer {answer:?}"));
                Ok(())
            });
            Ok(())
        });
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
    struct Boom {
        calls: Rc<Cell<u32>>,
        passthrough: bool,
    }
    impl Surface for Boom {
        fn kind(&self) -> &'static str {
            "boom"
        }
        fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
        fn event(&mut self, _: &Event, _: &mut Ctx) -> bool {
            self.calls.set(self.calls.get() + 1);
            panic!("event bug")
        }
        fn initial_state(&self) -> State {
            if self.passthrough {
                State::Passthrough
            } else {
                State::Normal
            }
        }
    }
    for passthrough in [false, true] {
        let (mut ctx, _) = setup();
        let calls = Rc::new(Cell::new(0));
        let surface = Boom {
            calls: calls.clone(),
            passthrough,
        };
        let b = ctx.create_surface("s", surface);
        ctx.open_window(b, 80, 24).unwrap();
        run_fed(&mut ctx, "xyz");
        assert_eq!(calls.get(), 1, "x only, passthrough {passthrough}");
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
fn a_queued_calls_error_drops_nothing() {
    fn failed(_: &mut Ctx) -> Result {
        Err(Error::Msg("queued call failed".into()))
    }
    let (mut ctx, log) = setup();
    ctx.defer(failed);
    run_fed(&mut ctx, "a");
    assert_eq!(*log.borrow(), ["note-a"], "a deferred call");
    log.borrow_mut().clear();

    let (tx, rx) = std::sync::mpsc::channel();
    ctx.set_waker(move || tx.send(()).unwrap()).unwrap();
    let job = ctx.spawn(|| (), |ctx, _| failed(ctx));
    ctx.feed_keys("a").unwrap();
    rx.recv().unwrap(); // so its done runs ahead of the key
    ctx.run_until_idle();
    assert_eq!(*log.borrow(), ["note-a"], "a job's done");
    log.borrow_mut().clear();

    let hook = ctx.on_buffer_killed(|ctx, _| failed(ctx));
    let b = ctx.create_text("other", "text");
    ctx.kill(b);
    run_fed(&mut ctx, "a");
    assert_eq!(*log.borrow(), ["note-a"], "a hook");
    drop((job, hook));
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
    let sub = ctx.observe_keys(move |ev, src| s.borrow_mut().push((ev.key, src)));
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

#[test]
fn a_key_a_surface_ignores_in_passthrough_drops_nothing() {
    struct Ignores(Rc<Cell<u32>>);
    impl Surface for Ignores {
        fn kind(&self) -> &'static str {
            "ignores"
        }
        fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
        fn event(&mut self, _: &Event, _: &mut Ctx) -> bool {
            self.0.set(self.0.get() + 1);
            false
        }
        fn initial_state(&self) -> State {
            State::Passthrough
        }
    }
    let (mut ctx, _) = setup();
    let calls = Rc::new(Cell::new(0));
    let b = ctx.create_surface("s", Ignores(calls.clone()));
    ctx.open_window(b, 80, 24).unwrap();
    run_fed(&mut ctx, "xyz");
    assert_eq!(calls.get(), 3, "a terminal ignoring a key isn't a failure");
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn a_failing_prompt_answer_drops_the_rest() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, ":x<CR>b");
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert!(ctx.messages[0].contains("no match"), "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal);
}

#[test]
fn a_panicking_prompt_answer_drops_the_rest() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "!x<CR>b");
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn a_prompt_answer_that_succeeds_drops_nothing() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "?x<CR>b");
    assert_eq!(*log.borrow(), [r#"answer Some("x")"#, "note-b"]);
}

#[test]
fn a_cancelled_prompt_whose_answer_fails_drops_the_rest() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, ":x<Esc>b");
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn a_prompt_a_new_prompt_cancels_drops_nothing() {
    let (mut ctx, _) = setup();
    ctx.prompt("first", |_, _| Err(Error::Msg("cancelled".into())));
    ctx.feed_keys("xy").unwrap();
    ctx.prompt("second", |_, _| Ok(()));
    ctx.run_until_idle();
    assert_eq!(ctx.prompt_text(), Some("xy"), "{:?}", ctx.messages);
}

#[test]
fn fed_keys_go_ahead_of_typed_ones() {
    let (mut ctx, log) = setup();
    ctx.input(KeyEvent::press(Key::Char('c')));
    ctx.feed_keys("a").unwrap();
    ctx.run_until_idle();
    assert_eq!(*log.borrow(), ["note-a", "note-c"]);
}

#[test]
fn a_macro_inside_a_macro_runs_before_the_rest_of_it() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "agc"); // g feeds b
    assert_eq!(*log.borrow(), ["note-a", "note-b", "note-c"]);
}

#[test]
fn keys_fed_twice_in_one_turn_keep_their_order() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "ta"); // t feeds b, then c
    assert_eq!(*log.borrow(), ["note-b", "note-c", "note-a"]);
}

#[test]
fn macros_nested_three_deep_finish_innermost_first() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "ia"); // i feeds g and c, and g feeds b
    assert_eq!(*log.borrow(), ["note-b", "note-c", "note-a"]);
}

#[test]
fn an_error_in_a_macro_inside_a_macro_ends_both() {
    let (mut ctx, log) = setup();
    run_fed(&mut ctx, "nb"); // n feeds e, which fails
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn keys_the_host_feeds_go_behind_what_is_left() {
    let (mut ctx, log) = setup();
    ctx.feed_keys("ab").unwrap();
    assert!(ctx.step(), "a's turn");
    ctx.feed_keys("c").unwrap();
    ctx.run_until_idle();
    assert_eq!(*log.borrow(), ["note-a", "note-b", "note-c"]);
}

#[test]
fn keys_fed_after_a_nested_turn_emptied_the_queue_still_run() {
    let (mut ctx, log) = setup();
    ctx.type_keys("N");
    assert_eq!(*log.borrow(), ["note-c"], "{:?}", ctx.messages);
}

#[test]
fn the_drop_hook_runs_before_the_next_key_is_looked_up() {
    let (mut ctx, log) = setup();
    let seen = log.clone();
    let hook = ctx.on_fed_dropped(move |_| {
        seen.borrow_mut().push("dropped".into());
        Ok(())
    });
    ctx.feed_keys("ea").unwrap();
    ctx.type_keys("b");
    assert_eq!(*log.borrow(), ["dropped", "note-b"]);
    log.borrow_mut().clear();
    ctx.type_keys("e");
    assert_eq!(
        *log.borrow(),
        ["dropped"],
        "a key that fails, even with nothing fed"
    );
    log.borrow_mut().clear();
    ctx.type_keys("a");
    assert_eq!(
        *log.borrow(),
        ["note-a"],
        "a key that doesn't fail drops nothing"
    );
    log.borrow_mut().clear();
    drop(hook);
    ctx.type_keys("e");
    assert!(log.borrow().is_empty());
}
