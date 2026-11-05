//! Criterion 3 (C3): calls that receive Ctx (hooks, callbacks, on_resize, on_enable) are queued and
//! run from the top of the loop, never nested. `prompt` delivers None on cancel.

use die_test_cap as _;

mod support;
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

type Log = Rc<RefCell<Vec<String>>>;

#[derive(Default)]
struct Browser {
    resized: Vec<(usize, usize, usize)>,
    seen_during_checkout: Option<usize>,
    enabled_at_depth: Option<usize>,
}
impl Mode for Browser {
    fn on_enable(&mut self, _: BufferId, ctx: &mut Ctx) -> Result {
        self.enabled_at_depth = Some(ctx.nesting());
        Ok(())
    }
    fn on_resize(&mut self, _: WindowId, cols: usize, rows: usize, ctx: &mut Ctx) {
        self.resized.push((cols, rows, ctx.nesting()));
    }
}

struct Ext {
    log: Log,
}
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "ext"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Browser>("browser", Browser::default);
        let log = self.log.clone();
        reg.command("kill-other", "", move |ctx, _| {
            let b = ctx.find_buffer("victim").unwrap();
            ctx.kill(b);
            log.borrow_mut().push(format!("cmd-end@{}", ctx.nesting()));
            Ok(())
        });
        reg.command(
            "browser-resize",
            "resize from inside the browser's own checkout",
            |ctx, a| {
                let (b, w) = (a.buffer.unwrap(), a.window.unwrap());
                ctx.with_mode(b, |m: &mut Browser, ctx| {
                    ctx.resize(w, 50, 10).unwrap();
                    m.seen_during_checkout = Some(m.resized.len());
                })?;
                Ok(())
            },
        );
        let log = self.log.clone();
        reg.command(
            "new-filter",
            "a two-step prompt, as scrib-new-filter",
            move |ctx, _| {
                let log = log.clone();
                ctx.prompt("name", move |ctx, name| {
                    log.borrow_mut()
                        .push(format!("name={name:?}@{}", ctx.nesting()));
                    let Some(_) = name else { return Ok(()) };
                    let log = log.clone();
                    ctx.prompt("query", move |ctx, q| {
                        log.borrow_mut()
                            .push(format!("query={q:?}@{}", ctx.nesting()));
                        Ok(())
                    });
                    Ok(())
                });
                Ok(())
            },
        );
        let k = reg.kind_keymap("text");
        k.bind(State::Normal, "K", "kill-other")
            .bind(State::Normal, "R", "browser-resize")
            .bind(State::Normal, "N", "new-filter");
    }
}

fn setup() -> (Ctx, Log, BufferId, WindowId) {
    let log: Log = Rc::default();
    let mut ctx = Ctx::new(vec![Box::new(Ext { log: log.clone() })]).unwrap();
    let b = ctx.create_text("main", "x");
    let w = ctx.open_window(b, 80, 24).unwrap();
    (ctx, log, b, w)
}

#[test]
fn hooks_are_queued_not_nested() {
    let (mut ctx, log, _, _) = setup();
    ctx.create_text("victim", "");
    let l = log.clone();
    let sub = ctx.on_buffer_killed(move |ctx, _| {
        l.borrow_mut().push(format!("hook@{}", ctx.nesting()));
        Ok(())
    });
    ctx.type_keys("K");
    assert_eq!(
        *log.borrow(),
        ["cmd-end@1", "hook@1"],
        "the hook ran after the command, at the top of the loop"
    );
    drop(sub);
}

#[test]
fn on_resize_and_on_enable_are_queued() {
    let (mut ctx, _, b, _) = setup();
    ctx.enable_mode::<Browser>(b).unwrap();
    assert_eq!(
        ctx.with_mode(b, |m: &mut Browser, _| m.enabled_at_depth)
            .unwrap(),
        None,
        "on_enable not run yet"
    );
    ctx.run_until_idle();
    assert_eq!(
        ctx.with_mode(b, |m: &mut Browser, _| m.enabled_at_depth)
            .unwrap(),
        Some(1)
    );
    // The mode resizes its window while checked out. A synchronous on_resize would find it Busy.
    ctx.type_keys("R");
    let (resized, during) = ctx
        .with_mode(b, |m: &mut Browser, _| {
            (m.resized.clone(), m.seen_during_checkout)
        })
        .unwrap();
    assert_eq!(during, Some(0), "not called inside the checkout");
    assert_eq!(
        resized,
        [(50, 10, 1)],
        "called once, from the top of the loop"
    );
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn timer_and_job_callbacks_run_from_the_top() {
    let (mut ctx, log, _, _) = setup();
    let (l1, l2) = (log.clone(), log.clone());
    let t = ctx.after(Duration::from_millis(10), move |ctx| {
        l1.borrow_mut().push(format!("timer@{}", ctx.nesting()));
        Ok(())
    });
    let j = ctx.spawn(
        || 5,
        move |ctx, v: i32| {
            l2.borrow_mut().push(format!("job{v}@{}", ctx.nesting()));
            Ok(())
        },
    );
    ctx.run_until_idle();
    ctx.advance(Duration::from_millis(10));
    assert_eq!(*log.borrow(), ["job5@1", "timer@1"]);
    drop((t, j));
}

#[test]
fn prompt_delivers_none_on_cancel() {
    let (mut ctx, log, _, _) = setup();
    ctx.type_keys("Nwork<CR>");
    assert_eq!(ctx.state(), State::CommandLine, "second prompt open");
    ctx.type_keys("<Esc>");
    assert_eq!(*log.borrow(), ["name=Some(\"work\")@1", "query=None@1"]);
    assert_eq!(ctx.state(), State::Normal);
    log.borrow_mut().clear();
    ctx.type_keys("Nab<BS><Esc>");
    assert_eq!(
        *log.borrow(),
        ["name=None@1"],
        "cancel at the first step lets the flow clean up"
    );
}
