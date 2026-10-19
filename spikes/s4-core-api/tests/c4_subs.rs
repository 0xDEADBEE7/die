//! Criterion 4 (C4): subscription handles cancel on drop; `.detach()` ties them to the extension;
//! a handle stored in a mode is cancelled when its buffer is killed.
mod support;
use s4core::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

const MS: Duration = Duration::from_millis(1);

#[derive(Default)]
struct Poller {
    timer: Option<Sub>,
}
impl Mode for Poller {}

struct Ext {
    ticks: Rc<Cell<u32>>,
}
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "poller"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Poller>("poller", Poller::default);
    }
    fn init(&mut self, ctx: &mut Ctx) -> Result {
        let t = self.ticks.clone();
        // Extension-lifetime watcher, like orchid's file watch.
        ctx.every(10 * MS, move |_| {
            t.set(t.get() + 1);
            Ok(())
        })
        .detach();
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
fn drop_cancels() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let (n, mut f) = counter();
    let sub = ctx.after(10 * MS, move |c| f(c));
    drop(sub);
    ctx.advance(20 * MS);
    assert_eq!(n.get(), 0);
    let (n, mut f) = counter();
    let sub = ctx.after(10 * MS, move |c| f(c));
    ctx.advance(20 * MS);
    assert_eq!(n.get(), 1, "a kept handle fires");
    drop(sub);
}

#[test]
fn detach_ties_to_the_extension() {
    let ticks = Rc::new(Cell::new(0));
    let mut ctx = Ctx::new(vec![Box::new(Ext {
        ticks: ticks.clone(),
    })])
    .unwrap();
    ctx.advance(35 * MS);
    assert_eq!(ticks.get(), 3, "detached timer runs with no handle held");
    ctx.shutdown();
    ctx.advance(50 * MS);
    assert_eq!(ticks.get(), 3, "cancelled when its extension shuts down");
}

#[test]
fn handle_in_mode_cancelled_when_buffer_killed() {
    let mut ctx = Ctx::new(vec![Box::new(Ext {
        ticks: Rc::default(),
    })])
    .unwrap();
    let b = ctx.create_text("chat", "");
    ctx.enable_mode::<Poller>(b).unwrap();
    let (n, f) = counter();
    let sub = ctx.every(10 * MS, f);
    ctx.with_mode(b, |p: &mut Poller, _| p.timer = Some(sub))
        .unwrap();
    ctx.advance(10 * MS);
    assert_eq!(n.get(), 1);
    ctx.kill(b);
    ctx.advance(50 * MS);
    assert_eq!(
        n.get(),
        1,
        "the mode was dropped with its buffer, and its handle with it"
    );
}

#[test]
fn handle_in_checked_out_mode_cancelled_at_checkin() {
    let mut ctx = Ctx::new(vec![Box::new(Ext {
        ticks: Rc::default(),
    })])
    .unwrap();
    let b = ctx.create_text("chat", "");
    ctx.enable_mode::<Poller>(b).unwrap();
    let (n, f) = counter();
    let sub = ctx.every(10 * MS, f);
    let active = ctx
        .with_mode(b, |p: &mut Poller, ctx| {
            p.timer = Some(sub);
            ctx.kill(b);
            p.timer.as_ref().unwrap().is_active() // still held by the checked-out mode
        })
        .unwrap();
    assert!(active);
    ctx.advance(50 * MS);
    assert_eq!(n.get(), 0, "cancelled when the mode dropped at check-in");
}
