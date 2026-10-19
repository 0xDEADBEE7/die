//! Criterion 2 (C2): checkout returns Busy, Gone and Missing; panics put the object back and are
//! attributed; killing a buffer during checkout drops the object at check-in.
mod support;
use s4core::*;
use std::cell::Cell;
use std::rc::Rc;

struct Drops(Rc<Cell<u32>>);
impl Drop for Drops {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct Surf {
    n: u32,
    _d: Option<Drops>,
}
impl Surface for Surf {
    fn kind(&self) -> &'static str {
        "surf"
    }
    fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
}
struct OtherSurf;
impl Surface for OtherSurf {
    fn kind(&self) -> &'static str {
        "other"
    }
    fn render(&mut self, _: usize, _: usize, _: &mut Vec<String>) {}
}

#[derive(Default)]
struct M1 {
    n: u32,
    d: Option<Drops>,
}
impl Mode for M1 {}
#[derive(Default)]
struct M2;
impl Mode for M2 {}

struct Ext {
    n: u32,
}
struct Unregistered;
impl Extension for Unregistered {
    fn name(&self) -> &'static str {
        "nope"
    }
    fn register(&mut self, _: &mut Registry) {}
}

impl Extension for Ext {
    fn name(&self) -> &'static str {
        "ext"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<M1>("m1", M1::default);
        reg.mode::<M2>("m2", || M2);
        reg.command("panic-ext", "", |ctx, _| {
            ctx.with_ext(|e: &mut Ext, _| {
                e.n += 1;
                panic!("ext bug")
            })?;
            Ok(())
        });
        reg.command("panic-mode", "", |ctx, a| {
            ctx.with_mode(a.buffer.unwrap(), |m: &mut M1, _| {
                m.n += 1;
                panic!("mode bug")
            })?;
            Ok(())
        });
        reg.command("panic-surface", "", |ctx, a| {
            ctx.with_surface(a.buffer.unwrap(), |s: &mut Surf, _| {
                s.n += 1;
                panic!("surface bug")
            })?;
            Ok(())
        });
    }
}

fn setup() -> (Ctx, BufferId, BufferId) {
    let mut ctx = Ctx::new(vec![Box::new(Ext { n: 0 })]).unwrap();
    let s = ctx.create_surface("s", Surf { n: 0, _d: None });
    let t = ctx.create_text("t", "text");
    ctx.enable_mode::<M1>(t).unwrap();
    (ctx, s, t)
}

#[test]
fn busy_on_reentry() {
    let (mut ctx, s, t) = setup();
    let r = ctx
        .with_ext(|_: &mut Ext, ctx| ctx.with_ext(|_: &mut Ext, _| ()).err())
        .unwrap();
    assert_eq!(r, Some(Checkout::Busy));
    let r = ctx
        .with_mode(t, |_: &mut M1, ctx| {
            ctx.with_mode(t, |_: &mut M1, _| ()).err()
        })
        .unwrap();
    assert_eq!(r, Some(Checkout::Busy));
    let r = ctx
        .with_surface(s, |_: &mut Surf, ctx| {
            ctx.with_surface(s, |_: &mut Surf, _| ()).err()
        })
        .unwrap();
    assert_eq!(r, Some(Checkout::Busy));
    // Other objects stay available during a checkout.
    let r = ctx
        .with_mode(t, |_: &mut M1, ctx| {
            ctx.with_surface(s, |_: &mut Surf, _| 7)
        })
        .unwrap();
    assert_eq!(r, Ok(7));
}

#[test]
fn gone_after_kill() {
    let (mut ctx, s, t) = setup();
    ctx.kill(s);
    ctx.kill(t);
    assert_eq!(
        ctx.with_surface(s, |_: &mut Surf, _| ()).err(),
        Some(Checkout::Gone)
    );
    assert_eq!(
        ctx.with_mode(t, |_: &mut M1, _| ()).err(),
        Some(Checkout::Gone)
    );
    assert_eq!(ctx.text(t).err(), Some(Error::Checkout(Checkout::Gone)));
}

#[test]
fn missing_for_unknown() {
    let (mut ctx, s, t) = setup();
    assert_eq!(
        ctx.with_ext(|_: &mut Unregistered, _| ()).err(),
        Some(Checkout::Missing)
    );
    assert_eq!(
        ctx.with_mode(t, |_: &mut M2, _| ()).err(),
        Some(Checkout::Missing),
        "mode not enabled"
    );
    assert_eq!(
        ctx.with_surface(s, |_: &mut OtherSurf, _| ()).err(),
        Some(Checkout::Missing),
        "wrong surface type"
    );
    assert_eq!(
        ctx.with_surface(t, |_: &mut Surf, _| ()).err(),
        Some(Checkout::Missing),
        "text buffer has no custom surface"
    );
    assert_eq!(
        ctx.enable_mode::<M1>(s),
        Err(Error::NotText),
        "C17: modes only on text buffers"
    );
}

#[test]
fn panic_restores_object_and_is_attributed() {
    support::quiet_panics();
    let (mut ctx, s, t) = setup();
    for (cmd, buf) in [
        ("panic-ext", None),
        ("panic-mode", Some(t)),
        ("panic-surface", Some(s)),
    ] {
        let r = ctx.run(
            cmd,
            &Args {
                buffer: buf,
                ..Default::default()
            },
        );
        assert!(
            matches!(&r, Err(Error::Panicked { ext: "ext", msg }) if msg.ends_with("bug")),
            "{cmd}: {r:?}"
        );
    }
    // Each object was put back, with the change made before the panic.
    assert_eq!(ctx.with_ext(|e: &mut Ext, _| e.n), Ok(1));
    assert_eq!(ctx.with_mode(t, |m: &mut M1, _| m.n), Ok(1));
    assert_eq!(ctx.with_surface(s, |x: &mut Surf, _| x.n), Ok(1));
}

#[test]
fn kill_during_checkout_drops_at_checkin() {
    let (mut ctx, _, t) = setup();
    let drops = Rc::new(Cell::new(0));
    let s = ctx.create_surface(
        "s",
        Surf {
            n: 0,
            _d: Some(Drops(drops.clone())),
        },
    );
    let during = ctx
        .with_surface(s, |_: &mut Surf, ctx| {
            ctx.kill(s);
            (
                drops.get(),
                ctx.with_surface(s, |_: &mut Surf, _| ()).err(),
                ctx.is_live(s),
            )
        })
        .unwrap();
    assert_eq!(
        during,
        (0, Some(Checkout::Gone), false),
        "dead at once, not dropped yet"
    );
    assert_eq!(drops.get(), 1, "dropped at check-in");

    let mdrops = Rc::new(Cell::new(0));
    ctx.with_mode(t, |m: &mut M1, _| m.d = Some(Drops(mdrops.clone())))
        .unwrap();
    let during = ctx
        .with_mode(t, |_: &mut M1, ctx| {
            ctx.kill(t);
            (mdrops.get(), ctx.with_mode(t, |_: &mut M1, _| ()).err())
        })
        .unwrap();
    assert_eq!(during, (0, Some(Checkout::Gone)));
    assert_eq!(mdrops.get(), 1);
}
