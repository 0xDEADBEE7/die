//! K5: every panic path logs to `*messages*` and leaves the editor usable (C9, contract §15).
//! `usable` runs after each one: no call is left on the stack, the objects are back, a key still
//! runs its command, a timer still fires, and text still edits.
//!
//! Each test below names one call from core into extension code:
//! - `register`: a startup error, with no `Ctx`. `init`: logged, and the next extension inits.
//!   `shutdown`: logged, and the rest shut down.
//! - A command: from a key, from `run` (returned as well), and nested in a command that passes the
//!   error on, which still logs once. `reveal`'s command.
//! - Queued calls: `defer`, a timer that goes on repeating, a kill hook, a prompt's answer.
//! - Jobs: `work` on its thread, whose panic reaches the loop so idle doesn't wait for it, even
//!   with its handle cancelled; `done` on the main thread.
//! - Surfaces: `event` for an unbound key, in passthrough, and for a resize, attributed to the
//!   extension that made the surface. `render`: a placeholder, logged once however many frames.
//! - Modes: the factory, `on_enable`, `on_disable`, `on_resize`, and `decorate`.
//! - Key observers: the key still dispatches.
//! - `Drop`: a buffer's object panicking as `kill` drops it. The buffer is dead and hooks still run.
//!   What a cancelled job, timer, idle timer or hook holds, wherever core comes to drop it: a
//!   job's value, its `done` when its work panicked, at the deadline, while queued, in the sweep,
//!   when `next_timer` looks, and when a turn tidies up.
//! - Who a panic names: `a_key_observers_panic_names_its_extension` and
//!   `a_prompts_answer_panic_names_the_extension_that_asked`.
//! - `panics_taking_turns_dont_flood_the_log`: two objects panicking on every frame, and what
//!   logging something else does.
//! - `a_long_run_of_panic_lines_stops_being_compared`: what that check costs has a limit.
//! - `a_render_panic_keeps_its_line_when_a_command_panics_next`: the hook's list and the guard
//!   match panics by message, including a panic that comes with no hook call.
//! - `a_message_names_the_extension_the_place_and_restarting`: what the line says.
//! - `a_panic_only_the_host_caught_is_logged_at_the_next_turn`: the hook catches what no guard saw.
//! - `a_new_ctx_doesnt_log_a_panic_from_before_it_existed`: as when a property test shrinks, one
//!   case's panic isn't the next case's.
//! - `a_loop_that_never_goes_idle_is_reported_not_a_panic`: core's own panic on a self-feeding
//!   callback becomes a message.
use die_core::*;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::{Duration, Instant};

const MS: Duration = Duration::from_millis(1);

#[derive(Default)]
struct Flags {
    init: Cell<bool>,
    shutdown: Cell<bool>,
    ok_runs: Cell<u32>,
}

struct Ext {
    flags: Rc<Flags>,
}
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "ext"
    }
    fn register(&mut self, reg: &mut Registry) {
        let flags = self.flags.clone();
        reg.command("ok", "", move |_, _| {
            flags.ok_runs.set(flags.ok_runs.get() + 1);
            Ok(())
        });
        reg.command("boom", "", |_, _| panic!("boom"));
        reg.command("passes-it-on", "", |ctx, a| ctx.run("boom", a));
        reg.command("make-surface", "", |ctx, _| {
            let b = ctx.create_surface("from-ext", Surf::default());
            ctx.open_window(b, 20, 2)?;
            Ok(())
        });
        reg.command("job-from-ext", "", |ctx, _| {
            ctx.spawn(|| -> u8 { panic!("work bug") }, |_, _| Ok(()))
                .detach();
            Ok(())
        });
        reg.command("observe-from-ext", "", |ctx, _| {
            ctx.observe_keys(|_, _| panic!("observer bug")).detach();
            Ok(())
        });
        reg.command("ask-from-ext", "", |ctx, _| {
            ctx.prompt("q", |_, _| panic!("answer bug"));
            Ok(())
        });
        reg.command("rethrows", "", |_, _| {
            std::panic::resume_unwind(Box::new("rethrown"))
        });
        reg.mode::<Bad>("bad", Bad::default);
        reg.mode::<BadFactory>("bad-factory", || panic!("factory bug"));
        let k = reg.kind_keymap("text");
        k.bind(State::Normal, "z", "ok")
            .bind(State::Normal, "P", "boom")
            .bind(State::Normal, "N", "passes-it-on")
            .bind(State::Normal, "S", "make-surface")
            .bind(State::Normal, "J", "job-from-ext")
            .bind(State::Normal, "O", "observe-from-ext")
            .bind(State::Normal, "A", "ask-from-ext")
            .bind(State::Normal, "R", "rethrows");
        let k = reg.kind_keymap("surf");
        k.bind(State::Normal, "z", "ok");
    }
    fn init(&mut self, _: &mut Ctx) -> Result {
        if self.flags.init.get() {
            panic!("init bug");
        }
        Ok(())
    }
    fn shutdown(&mut self, _: &mut Ctx) {
        if self.flags.shutdown.get() {
            panic!("shutdown bug");
        }
    }
}

/// A second extension, to show startup and shutdown go on past the first.
struct Other(Rc<Cell<u32>>);
impl Extension for Other {
    fn name(&self) -> &'static str {
        "other"
    }
    fn register(&mut self, _: &mut Registry) {}
    fn init(&mut self, _: &mut Ctx) -> Result {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
    fn shutdown(&mut self, _: &mut Ctx) {
        self.0.set(self.0.get() + 10);
    }
}

#[derive(Default)]
struct Bad {
    fail_disable: bool,
    fail_resize: bool,
    fail_decorate: Rc<Cell<bool>>,
    enabled: u32,
}
impl Mode for Bad {
    fn on_enable(&mut self, _: BufferId, _: &mut Ctx) -> Result {
        self.enabled += 1;
        if self.enabled == 1 {
            panic!("on_enable bug");
        }
        Ok(())
    }
    fn on_disable(&mut self, _: BufferId, _: &mut Ctx) {
        if self.fail_disable {
            panic!("on_disable bug");
        }
    }
    fn on_resize(&mut self, _: WindowId, _: usize, _: usize, _: &mut Ctx) {
        if self.fail_resize {
            panic!("on_resize bug");
        }
    }
    fn decorate(
        &mut self,
        _: &TextBuf,
        _: std::ops::Range<usize>,
        _: &mut Vec<(std::ops::Range<usize>, &'static str)>,
    ) {
        if self.fail_decorate.get() {
            panic!("decorate bug");
        }
    }
}
struct BadFactory;
impl Mode for BadFactory {}

#[derive(Default)]
struct Surf {
    passthrough: bool,
    fail_render: Rc<Cell<bool>>,
    seen: Rc<Cell<u32>>,
    _bad_drop: Option<BadDrop>,
}
impl Surface for Surf {
    fn kind(&self) -> &'static str {
        "surf"
    }
    fn render(&mut self, _: usize, _: usize, out: &mut Vec<String>) {
        if self.fail_render.get() {
            panic!("render bug");
        }
        out.push("drawn".into());
    }
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        self.seen.set(self.seen.get() + 1);
        match ev {
            Event::Key(k) if k.key == Key::Char('x') => panic!("event bug"),
            Event::Resize { .. } => panic!("resize bug"),
            _ => true,
        }
    }
    fn initial_state(&self) -> State {
        if self.passthrough {
            State::Passthrough
        } else {
            State::Normal
        }
    }
}
struct BadDrop;
impl Drop for BadDrop {
    fn drop(&mut self) {
        panic!("drop bug");
    }
}

fn frame_text(f: &Scene) -> String {
    (0..f.rows).map(|r| f.row_text(r) + "\n").collect()
}

struct World {
    ctx: Ctx,
    flags: Rc<Flags>,
    buf: BufferId,
    win: WindowId,
}

fn world() -> World {
    let flags: Rc<Flags> = Rc::default();
    let mut ctx = Ctx::new(vec![Box::new(Ext {
        flags: flags.clone(),
    })])
    .unwrap();
    let buf = ctx.create_text("t", "text");
    let win = ctx.open_window(buf, 80, 24).unwrap();
    World {
        ctx,
        flags,
        buf,
        win,
    }
}

impl World {
    /// The panic was logged exactly once, and the editor goes on working.
    fn logged_once_and_usable(&mut self, msg: &str) {
        self.ctx.run_until_idle();
        let lines: Vec<&String> = self
            .ctx
            .messages
            .iter()
            .filter(|m| m.contains(msg))
            .collect();
        assert_eq!(lines.len(), 1, "{msg}: {:?}", self.ctx.messages);
        usable(self);
    }
}

fn usable(w: &mut World) {
    let ctx = &mut w.ctx;
    assert_eq!(ctx.nesting(), 0, "a call was left on the stack");
    assert_eq!(
        ctx.with_ext(|_: &mut Ext, _| 1),
        Ok(1),
        "the extension is back"
    );
    ctx.focus(w.win);
    let before = w.flags.ok_runs.get();
    ctx.type_keys("z");
    assert_eq!(
        w.flags.ok_runs.get(),
        before + 1,
        "a key still runs its command"
    );
    let fired = Rc::new(Cell::new(false));
    let f = fired.clone();
    let t = ctx.after(MS, move |_| {
        f.set(true);
        Ok(())
    });
    ctx.advance(MS);
    assert!(fired.get(), "a timer still fires");
    drop(t);
    let t = ctx.text_mut(w.buf).unwrap();
    t.tx(Origin::Extension).insert(0, "e").commit().unwrap();
}

#[test]
fn a_panic_in_register_is_a_startup_error() {
    struct Bad;
    impl Extension for Bad {
        fn name(&self) -> &'static str {
            "bad"
        }
        fn register(&mut self, _: &mut Registry) {
            panic!("register bug");
        }
    }
    let errs = Ctx::new(vec![Box::new(Bad)]).err().unwrap();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("bad"), "{errs:?}");
    assert!(errs[0].contains("register bug"), "{errs:?}");
}

#[test]
fn a_panic_in_init_is_logged_and_the_next_extension_still_inits() {
    let flags: Rc<Flags> = Rc::default();
    flags.init.set(true);
    let others = Rc::new(Cell::new(0));
    let mut ctx = Ctx::new(vec![
        Box::new(Ext {
            flags: flags.clone(),
        }),
        Box::new(Other(others.clone())),
    ])
    .unwrap();
    assert_eq!(others.get(), 1);
    let buf = ctx.create_text("t", "text");
    let win = ctx.open_window(buf, 80, 24).unwrap();
    let mut w = World {
        ctx,
        flags,
        buf,
        win,
    };
    w.logged_once_and_usable("init bug");
}

#[test]
fn a_panic_in_shutdown_is_logged_and_the_rest_still_shut_down() {
    let flags: Rc<Flags> = Rc::default();
    flags.shutdown.set(true);
    let others = Rc::new(Cell::new(0));
    let mut ctx = Ctx::new(vec![
        Box::new(Other(others.clone())),
        Box::new(Ext {
            flags: flags.clone(),
        }),
    ])
    .unwrap();
    ctx.shutdown();
    assert_eq!(others.get(), 11, "init, then shutdown, past the panic");
    let lines = ctx.messages.iter().filter(|m| m.contains("shutdown bug"));
    assert_eq!(lines.count(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.nesting(), 0);
}

#[test]
fn a_panic_in_a_command_from_a_key() {
    let mut w = world();
    w.ctx.type_keys("P");
    w.logged_once_and_usable("boom");
}

#[test]
fn a_panic_in_a_command_from_run_is_returned_and_logged() {
    let mut w = world();
    let r = w.ctx.run("boom", &Args::default());
    assert!(
        matches!(&r, Err(Error::Panicked { ext: "ext", msg }) if msg == "boom"),
        "{r:?}"
    );
    w.logged_once_and_usable("boom");
}

#[test]
fn a_panic_passed_on_by_the_calling_command_is_logged_once() {
    let mut w = world();
    w.ctx.type_keys("N");
    w.logged_once_and_usable("boom");
}

#[test]
fn a_panic_in_a_reveal_command() {
    let mut w = world();
    let (buf, win) = (w.buf, w.win);
    w.ctx
        .text_mut(buf)
        .unwrap()
        .decorate(
            0..2,
            Deco::Hide {
                replacement: None,
                reveal: Reveal::Command("boom".into()),
            },
        )
        .unwrap();
    assert!(w.ctx.reveal(buf, 1).is_err());
    w.logged_once_and_usable("boom");
    assert_eq!(w.ctx.window_buffer(win), Some(buf));
}

#[test]
fn a_panic_in_a_deferred_call() {
    let mut w = world();
    w.ctx.defer(|_| panic!("deferred bug"));
    w.logged_once_and_usable("deferred bug");
}

#[test]
fn a_panic_in_a_repeating_timer_is_logged_each_time_and_it_goes_on() {
    let mut w = world();
    let n = Rc::new(Cell::new(0));
    let m = n.clone();
    let t = w.ctx.every(10 * MS, move |_| {
        m.set(m.get() + 1);
        panic!("timer bug {}", m.get())
    });
    w.ctx.advance(25 * MS);
    assert_eq!(n.get(), 2, "disabling it after repeated panics is M1's");
    w.logged_once_and_usable("timer bug 1");
    w.logged_once_and_usable("timer bug 2");
    drop(t);
}

#[test]
fn a_panic_in_a_kill_hook() {
    let mut w = world();
    let sub = w.ctx.on_buffer_killed(|_, _| panic!("hook bug"));
    let other = w.ctx.create_text("o", "");
    assert!(w.ctx.kill(other));
    w.logged_once_and_usable("hook bug");
    drop(sub);
}

#[test]
fn a_panic_in_a_prompts_answer_leaves_the_state_restored() {
    let mut w = world();
    w.ctx.prompt("name", |_, _| panic!("answer bug"));
    assert_eq!(w.ctx.state(), State::CommandLine);
    w.ctx.type_keys("<Esc>");
    assert_eq!(w.ctx.state(), State::Normal);
    assert_eq!(w.ctx.prompt_text(), None);
    w.logged_once_and_usable("answer bug");
}

#[test]
fn a_panic_in_a_jobs_work_reaches_the_loop() {
    let mut w = world();
    let ran = Rc::new(Cell::new(false));
    let r = ran.clone();
    let sub = w.ctx.spawn(
        || -> u8 { panic!("work bug") },
        move |_, _| {
            r.set(true);
            Ok(())
        },
    );
    let t = Instant::now();
    w.ctx.run_until_idle();
    assert!(
        t.elapsed() < Duration::from_secs(4),
        "idle waited for a thread that had died"
    );
    assert!(!ran.get(), "done has no value to run with");
    w.logged_once_and_usable("work bug");
    drop(sub);
}

#[test]
fn a_panic_in_a_jobs_work_names_the_extension_that_spawned_it() {
    let mut w = world();
    w.ctx.type_keys("J");
    let line = w.ctx.messages.iter().find(|m| m.contains("work bug"));
    assert!(
        line.is_some_and(|l| l.starts_with("ext panicked: work bug")),
        "{:?}",
        w.ctx.messages
    );
    w.logged_once_and_usable("work bug");
}

#[test]
fn a_panic_in_a_cancelled_jobs_work_is_still_logged() {
    let mut w = world();
    let sub = w.ctx.spawn(|| -> u8 { panic!("work bug") }, |_, _| Ok(()));
    drop(sub);
    w.logged_once_and_usable("work bug");
}

#[test]
fn a_panic_in_a_jobs_done() {
    let mut w = world();
    w.ctx.spawn(|| 1u8, |_, _| panic!("done bug")).detach();
    w.logged_once_and_usable("done bug");
}

#[test]
fn a_panic_in_a_surfaces_event_for_an_unbound_key() {
    let mut w = world();
    w.ctx.type_keys("S");
    assert_eq!(w.ctx.state(), State::Normal);
    let s = w.ctx.find_buffer("from-ext").unwrap();
    w.ctx.type_keys("x");
    let line = w.ctx.messages.iter().find(|m| m.contains("event bug"));
    assert!(
        line.is_some_and(|l| l.contains("ext panicked")),
        "attributed to the extension that made it: {:?}",
        w.ctx.messages
    );
    assert!(
        w.ctx.messages.iter().all(|m| !m.contains("undefined")),
        "a panic isn't an undefined key"
    );
    assert_eq!(
        w.ctx.with_surface(s, |_: &mut Surf, _| 1),
        Ok(1),
        "the surface is back"
    );
    w.logged_once_and_usable("event bug");
}

#[test]
fn a_panic_in_a_surfaces_event_in_passthrough() {
    let mut w = world();
    let seen = Rc::new(Cell::new(0));
    let s = w.ctx.create_surface(
        "p",
        Surf {
            passthrough: true,
            seen: seen.clone(),
            ..Default::default()
        },
    );
    w.ctx.open_window(s, 20, 2).unwrap();
    w.ctx.type_keys("xy");
    assert_eq!(seen.get(), 2, "the next key still reaches it");
    w.logged_once_and_usable("event bug");
}

#[test]
fn a_panic_in_a_resize_event() {
    let mut w = world();
    w.ctx.type_keys("S");
    let s = w.ctx.find_buffer("from-ext").unwrap();
    let win = w.ctx.focused().unwrap();
    w.ctx.resize(win, 30, 3).unwrap();
    w.ctx.run_until_idle();
    let line = w.ctx.messages.iter().find(|m| m.contains("resize bug"));
    assert!(
        line.is_some_and(|l| l.starts_with("ext panicked: resize bug")),
        "attributed to the extension that made it: {:?}",
        w.ctx.messages
    );
    assert_eq!(w.ctx.with_surface(s, |_: &mut Surf, _| 1), Ok(1));
    w.logged_once_and_usable("resize bug");
}

#[test]
fn a_panic_in_render_draws_a_placeholder_and_is_logged_once() {
    let mut w = world();
    let bug = Rc::new(Cell::new(true));
    let s = w.ctx.create_surface(
        "s",
        Surf {
            fail_render: bug.clone(),
            ..Default::default()
        },
    );
    let win = w.ctx.open_window(s, 20, 2).unwrap();
    let place = WindowPlacement {
        window: win,
        col: 0,
        row: 0,
    };
    for _ in 0..3 {
        let f = frame_text(&w.ctx.compose(20, 4, &[place]));
        assert!(f.contains("render panicked"), "{f}");
    }
    bug.set(false);
    let f = frame_text(&w.ctx.compose(20, 4, &[place]));
    assert!(f.contains("drawn"), "until the next good render: {f}");
    w.logged_once_and_usable("render bug");
}

#[test]
fn a_panic_in_a_mode_factory_is_returned_and_logged() {
    let mut w = world();
    let r = w.ctx.enable_mode::<BadFactory>(w.buf);
    assert!(
        matches!(r, Err(Error::Panicked { ext: "ext", .. })),
        "{r:?}"
    );
    assert_eq!(
        w.ctx.with_mode(w.buf, |_: &mut BadFactory, _| ()),
        Err(Checkout::Missing)
    );
    w.logged_once_and_usable("factory bug");
}

#[test]
fn panics_in_on_enable_on_disable_and_on_resize() {
    let mut w = world();
    let buf = w.buf;
    w.ctx.enable_mode::<Bad>(buf).unwrap();
    w.logged_once_and_usable("on_enable bug");
    w.ctx
        .with_mode(buf, |m: &mut Bad, _| m.fail_resize = true)
        .unwrap();
    w.ctx.resize(w.win, 40, 10).unwrap();
    w.logged_once_and_usable("on_resize bug");
    assert_eq!(
        w.ctx.with_mode(buf, |_: &mut Bad, _| 1),
        Ok(1),
        "the mode is back"
    );
    w.ctx
        .with_mode(buf, |m: &mut Bad, _| m.fail_disable = true)
        .unwrap();
    w.ctx.disable_mode::<Bad>(buf).unwrap();
    w.logged_once_and_usable("on_disable bug");
}

#[test]
fn a_panic_in_decorate_is_logged_once() {
    let mut w = world();
    let buf = w.buf;
    w.ctx.enable_mode::<Bad>(buf).unwrap();
    w.logged_once_and_usable("on_enable bug");
    w.ctx
        .with_mode(buf, |m: &mut Bad, _| m.fail_decorate.set(true))
        .unwrap();
    let place = WindowPlacement {
        window: w.win,
        col: 0,
        row: 0,
    };
    for _ in 0..3 {
        let f = frame_text(&w.ctx.compose(80, 26, &[place]));
        assert!(f.contains("text"), "the text still draws: {f}");
    }
    w.logged_once_and_usable("decorate bug");
}

#[test]
fn a_panic_in_a_key_observer_leaves_the_key_dispatching() {
    let mut w = world();
    let sub = w.ctx.observe_keys(|_, _| panic!("observer bug"));
    let before = w.flags.ok_runs.get();
    w.ctx.type_keys("z");
    assert_eq!(w.flags.ok_runs.get(), before + 1, "the key still ran");
    let lines = w.ctx.messages.iter().filter(|m| m.contains("observer bug"));
    assert_eq!(lines.count(), 1, "{:?}", w.ctx.messages);
    drop(sub);
    w.logged_once_and_usable("observer bug");
}

#[test]
fn a_panic_in_a_drop_during_kill() {
    let mut w = world();
    let killed = Rc::new(Cell::new(0));
    let k = killed.clone();
    let sub = w.ctx.on_buffer_killed(move |_, _| {
        k.set(k.get() + 1);
        Ok(())
    });
    let s = w.ctx.create_surface(
        "s",
        Surf {
            _bad_drop: Some(BadDrop),
            ..Default::default()
        },
    );
    assert!(w.ctx.kill(s));
    assert!(!w.ctx.is_live(s), "dead, whatever its Drop did");
    w.logged_once_and_usable("drop bug");
    assert_eq!(killed.get(), 1, "the hooks still ran");
    drop(sub);
}

#[test]
fn a_message_names_the_extension_the_place_and_restarting() {
    let mut w = world();
    w.ctx.type_keys("P");
    let line = w.ctx.messages.last().unwrap().clone();
    assert!(line.starts_with("ext panicked: boom"), "{line}");
    assert!(line.contains("tests/panics.rs:"), "{line}");
    assert!(line.contains("restart"), "{line}");
}

#[test]
fn a_panic_only_the_host_caught_is_logged_at_the_next_turn() {
    let mut w = world();
    let r = catch_unwind(AssertUnwindSafe(|| {
        w.ctx
            .with_ext(|_: &mut Ext, _| -> () { panic!("host saw it") })
    }));
    assert!(r.is_err(), "with_ext lets a panic go on");
    assert!(w.ctx.messages.is_empty());
    w.logged_once_and_usable("host saw it");
}

#[test]
fn a_new_ctx_doesnt_log_a_panic_from_before_it_existed() {
    let earlier = world(); // the hook is in place
    let r = catch_unwind(|| panic!("before the new one"));
    assert!(r.is_err());
    let mut later = world();
    later.ctx.run_until_idle();
    assert!(later.ctx.messages.is_empty(), "{:?}", later.ctx.messages);
    drop(earlier);
}

#[test]
fn a_loop_that_never_goes_idle_is_reported_not_a_panic() {
    fn again(ctx: &mut Ctx) -> Result {
        if ctx.prompt_text().is_none() {
            ctx.defer(again);
        }
        Ok(())
    }
    let mut w = world();
    w.ctx.defer(again);
    w.ctx.run_until_idle();
    assert!(
        w.ctx.messages.iter().any(|m| m.contains("never went idle")),
        "{:?}",
        w.ctx.messages
    );
    // Stop it through the prompt, so the checks below can go idle.
    w.ctx.prompt("stop", |_, _| Ok(()));
    w.ctx.run_until_idle();
    w.ctx.type_keys("<Esc>");
    let log = w.ctx.messages.len();
    usable(&mut w);
    assert_eq!(w.ctx.messages.len(), log);
}

#[test]
fn a_panicked_observer_can_run_again() {
    let mut w = world();
    let seen = Rc::new(Cell::new(0));
    let s = seen.clone();
    let sub = w.ctx.observe_keys(move |_, _| {
        s.set(s.get() + 1);
        panic!("observer bug");
    });
    w.ctx.type_keys("zz");
    assert_eq!(seen.get(), 2);
    drop(sub);
}

/// A callback that panics when it's dropped, not when it's called.
fn drops_badly() -> impl FnMut(&mut Ctx) -> Result + 'static {
    let d = BadDrop;
    move |_| {
        let _ = &d;
        Ok(())
    }
}

#[test]
fn a_panic_dropping_a_cancelled_jobs_value() {
    let mut w = world();
    drop(w.ctx.spawn(|| BadDrop, |_, _| Ok(())));
    w.logged_once_and_usable("drop bug");
}

#[test]
fn a_panic_dropping_the_done_of_a_job_whose_work_panicked() {
    let mut w = world();
    let d = BadDrop;
    let job = w.ctx.spawn(
        || -> u8 { panic!("work bug") },
        move |_, _| {
            let _ = &d;
            Ok(())
        },
    );
    w.ctx.run_until_idle();
    w.logged_once_and_usable("work bug");
    w.logged_once_and_usable("drop bug");
    drop(job);
}

#[test]
fn a_panic_dropping_a_cancelled_timers_closure_at_its_deadline() {
    let mut w = world();
    drop(w.ctx.after(Duration::ZERO, drops_badly()));
    assert!(!w.ctx.step(), "nothing else was ready");
    w.logged_once_and_usable("drop bug");
}

#[test]
fn a_panic_dropping_a_closure_cancelled_while_it_was_queued() {
    let mut w = world();
    let victim: Rc<std::cell::RefCell<Option<Sub>>> = Rc::default();
    let v = victim.clone();
    let first = w.ctx.after(Duration::ZERO, move |_| {
        v.borrow_mut().take();
        Ok(())
    });
    let mut f = drops_badly();
    *victim.borrow_mut() = Some(w.ctx.after(Duration::ZERO, move |ctx| f(ctx)));
    while w.ctx.step() {}
    w.logged_once_and_usable("drop bug");
    drop(first);
}

#[test]
fn a_panic_dropping_a_closure_in_the_sweep() {
    let mut w = world();
    drop(w.ctx.after(Duration::from_secs(3600), drops_badly()));
    for _ in 0..1_024 {
        drop(w.ctx.after(Duration::from_secs(3600), |_| Ok(())));
    }
    w.logged_once_and_usable("drop bug");
}

#[test]
fn a_panic_dropping_a_closure_when_next_timer_looks() {
    let mut w = world();
    let last = w.ctx.after(Duration::from_secs(3600), |_| Ok(()));
    drop(w.ctx.after(MS, drops_badly()));
    assert_eq!(w.ctx.next_timer(), Some(Duration::from_secs(3600)));
    w.logged_once_and_usable("drop bug");
    drop(last);
}

#[test]
fn a_panic_dropping_a_cancelled_idle_timers_closure() {
    let mut w = world();
    drop(w.ctx.idle(Duration::from_secs(3600), drops_badly()));
    assert!(!w.ctx.step());
    w.logged_once_and_usable("drop bug");
}

#[test]
fn a_panic_dropping_a_cancelled_hook() {
    let mut w = world();
    let mut f = drops_badly();
    drop(w.ctx.on_buffer_killed(move |ctx, _| f(ctx)));
    assert!(!w.ctx.step());
    w.logged_once_and_usable("drop bug");
}

#[test]
fn a_key_observers_panic_names_its_extension() {
    let mut w = world();
    w.ctx.type_keys("Oz");
    let line = w.ctx.messages.iter().find(|m| m.contains("observer bug"));
    assert!(
        line.is_some_and(|l| l.starts_with("ext panicked: observer bug")),
        "{:?}",
        w.ctx.messages
    );
}

#[test]
fn a_prompts_answer_panic_names_the_extension_that_asked() {
    let mut w = world();
    w.ctx.type_keys("A<CR>");
    let line = w.ctx.messages.last().unwrap();
    assert!(line.starts_with("ext panicked: answer bug"), "{line}");
}

#[test]
fn panics_taking_turns_dont_flood_the_log() {
    struct Bad(&'static str);
    impl Surface for Bad {
        fn kind(&self) -> &'static str {
            "bad"
        }
        fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {
            panic!("{}", self.0);
        }
    }
    let mut ctx = Ctx::new(vec![]).unwrap();
    let mut place = |name, row| {
        let s = ctx.create_surface(name, Bad(name));
        let window = ctx.open_window(s, 20, 2).unwrap();
        WindowPlacement {
            window,
            col: 0,
            row,
        }
    };
    let p = [place("bug one", 0), place("bug two", 2)];
    let frames = |ctx: &mut Ctx, n| {
        for _ in 0..n {
            let _ = ctx.compose(20, 6, &p);
            ctx.step();
        }
    };
    frames(&mut ctx, 100);
    assert_eq!(ctx.messages.len(), 2, "{:?}", ctx.messages);
    ctx.messages.push("something else".into());
    frames(&mut ctx, 100);
    assert_eq!(
        ctx.messages.len(),
        5,
        "they're news again after it: {:?}",
        ctx.messages
    );
}

#[test]
fn a_long_run_of_panic_lines_stops_being_compared() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    for i in [0].into_iter().chain(0..100).chain([0]) {
        ctx.defer(move |_| panic!("bug {i}"));
        ctx.run_until_idle();
    }
    let lines = ctx.messages.iter().filter(|m| m.contains("bug 0 at"));
    assert_eq!(
        lines.count(),
        2,
        "the second is close enough to the first to be skipped, and the last isn't"
    );
    assert_eq!(ctx.messages.len(), 101);
}

#[test]
fn a_render_panic_keeps_its_line_when_a_command_panics_next() {
    for (key, command_line) in [('P', "ext panicked: boom"), ('R', "ext panicked: rethrown")] {
        let mut w = world();
        let s = w.ctx.create_surface(
            "s",
            Surf {
                fail_render: Rc::new(Cell::new(true)),
                ..Default::default()
            },
        );
        let window = w.ctx.open_window(s, 20, 2).unwrap();
        let place = WindowPlacement {
            window,
            col: 0,
            row: 0,
        };
        let _ = w.ctx.compose(20, 4, &[place]);
        w.ctx.focus(w.win);
        w.ctx.input(KeyEvent::press(Key::Char(key)));
        w.ctx.run_until_idle();
        let has = |what: &str| w.ctx.messages.iter().filter(|m| m.contains(what)).count();
        assert_eq!(has("render bug"), 1, "{:?}", w.ctx.messages);
        assert_eq!(has(command_line), 1, "{:?}", w.ctx.messages);
    }
}
