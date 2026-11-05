//! The 8 tests from the review's prototype (dieproto/tests/contract.rs), ported to the S4 API.
//! Same intent; test 4 changed from push `on_edit` to pull `edits_since` (C15).

use die_test_cap as _;

mod support;
use die_core::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use support::screen;

struct Orchid {
    opened: u32,
}
impl Extension for Orchid {
    fn name(&self) -> &'static str {
        "orchid"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.ext_command::<Orchid>("orchid-open", "open", |o, _ctx, _a| {
            o.opened += 1;
            Ok(())
        });
        reg.ext_command::<Orchid>("orchid-boom", "panic inside checkout", |_o, _ctx, _a| {
            panic!("orchid bug")
        });
        reg.ext_command::<Orchid>("orchid-nested", "", |_o, ctx, a| ctx.run("term-poke", a));
    }
}

struct Term {
    buf: Option<BufferId>,
}
struct TermSurface {
    lines: u32,
}
impl Surface for TermSurface {
    fn kind(&self) -> &'static str {
        "term"
    }
    fn render(&mut self, _c: usize, _r: usize, out: &mut Vec<String>) {
        if self.lines == 99 {
            panic!("render bug")
        }
        out.push("$ ".into());
    }
}
impl Extension for Term {
    fn name(&self) -> &'static str {
        "term"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("term-poke", "", |ctx, _| {
            let buf = ctx
                .with_ext(|t: &mut Term, _| t.buf)?
                .ok_or(Error::Msg("no term".into()))?;
            ctx.with_surface(buf, |s: &mut TermSurface, _| s.lines += 1)?;
            Ok(())
        });
        reg.command("term-reenter", "", |ctx, a| {
            let buf = ctx.with_ext(|t: &mut Term, _| t.buf)?.unwrap();
            ctx.with_surface(buf, |_s: &mut TermSurface, ctx| ctx.run("term-poke", a))?
        });
        reg.command("term-kill-self", "", |ctx, _| {
            let buf = ctx.with_ext(|t: &mut Term, _| t.buf)?.unwrap();
            ctx.with_surface(buf, |_s: &mut TermSurface, ctx| ctx.kill(buf))?;
            Ok(())
        });
        let k = reg.kind_keymap("text");
        k.bind(State::Normal, "f<char>", "vim-find-char");
        k.bind(State::Normal, "ff", "special-ff");
        k.bind(State::Normal, "gg", "goto-top");
        for name in ["vim-find-char", "special-ff", "goto-top"] {
            reg.command(name, "", move |ctx, a| {
                ctx.messages.push(format!("{name} {:?}", a.char()));
                Ok(())
            });
        }
    }
}

#[derive(Default)]
struct Chat {
    seen: Vec<String>,
}
impl Mode for Chat {}
struct ChatExt;
impl Extension for ChatExt {
    fn name(&self) -> &'static str {
        "chat"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Chat>("chat", Chat::default);
    }
}

fn setup() -> (Ctx, BufferId) {
    let mut ctx = Ctx::new(vec![
        Box::new(Orchid { opened: 0 }),
        Box::new(Term { buf: None }),
        Box::new(ChatExt),
    ])
    .unwrap();
    let b = ctx.create_surface("term", TermSurface { lines: 0 });
    ctx.with_ext(|t: &mut Term, _| t.buf = Some(b)).unwrap();
    (ctx, b)
}

#[test]
fn typed_ext_command_and_downcast() {
    let (mut ctx, _) = setup();
    ctx.run("orchid-open", &Args::default()).unwrap();
    assert_eq!(ctx.with_ext(|o: &mut Orchid, _| o.opened).unwrap(), 1);
}

#[test]
fn reentrant_checkout_is_busy_not_gone() {
    let (mut ctx, _) = setup();
    let r = ctx.run("term-reenter", &Args::default());
    assert_eq!(r, Err(Error::Checkout(Checkout::Busy)));
    assert!(
        ctx.run("orchid-nested", &Args::default()).is_ok(),
        "cross-extension nesting is fine"
    );
}

#[test]
fn panic_inside_checkout_restores_object_and_attributes() {
    support::quiet_panics();
    let (mut ctx, _) = setup();
    let r = ctx.run("orchid-boom", &Args::default());
    assert!(
        matches!(r, Err(Error::Panicked { ext: "orchid", .. })),
        "{r:?}"
    );
    assert!(
        ctx.run("orchid-open", &Args::default()).is_ok(),
        "object was put back"
    );
}

#[test]
fn checked_out_mode_sees_its_own_edits() {
    let (mut ctx, _) = setup();
    let b = ctx.create_text("chat", "");
    ctx.enable_mode::<Chat>(b).unwrap();
    ctx.text_mut(b)
        .unwrap()
        .tx(Origin::Extension)
        .insert(0, "hello")
        .commit()
        .unwrap();
    // orchid-style streaming: the mode edits its own buffer while checked out, then reads its edits.
    ctx.with_mode(b, |m: &mut Chat, ctx| {
        let rev = ctx.text(b).unwrap().rev();
        let t = ctx.text_mut(b).unwrap();
        t.tx(Origin::Extension)
            .silent()
            .insert(5, " world")
            .commit()
            .unwrap();
        t.tx(Origin::Extension)
            .silent()
            .insert(0, ">")
            .commit()
            .unwrap();
        m.seen = t
            .edits_since(rev)
            .unwrap()
            .map(|c| c.text.clone())
            .collect();
    })
    .unwrap();
    assert_eq!(
        ctx.with_mode(b, |m: &mut Chat, _| m.seen.clone()).unwrap(),
        vec![" world", ">"]
    );
}

#[test]
fn kill_during_checkout_and_generations() {
    let (mut ctx, b) = setup();
    ctx.run("term-kill-self", &Args::default()).unwrap();
    assert_eq!(
        ctx.with_surface(b, |_s: &mut TermSurface, _| ()).err(),
        Some(Checkout::Gone)
    );
    let b2 = ctx.create_surface("term", TermSurface { lines: 0 });
    assert_ne!(b, b2);
    assert_eq!(
        ctx.with_surface(b, |_s: &mut TermSurface, _| ()).err(),
        Some(Checkout::Gone),
        "stale id stays dead after slot reuse"
    );
}

#[derive(Default)]
struct Holder {
    job: Option<Sub>,
}
impl Mode for Holder {}
struct HolderExt;
impl Extension for HolderExt {
    fn name(&self) -> &'static str {
        "holder"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Holder>("holder", Holder::default);
    }
}

#[test]
fn job_handle_in_mode_cancelled_on_kill() {
    let mut ctx = Ctx::new(vec![Box::new(HolderExt)]).unwrap();
    let b = ctx.create_text("orchid-chat", "");
    ctx.enable_mode::<Holder>(b).unwrap();
    let ran = Rc::new(Cell::new(false));
    let r2 = ran.clone();
    let sub = ctx.spawn(
        || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            42u32
        },
        move |_ctx, v| {
            assert_eq!(v, 42);
            r2.set(true);
            Ok(())
        },
    );
    ctx.with_mode(b, |h: &mut Holder, _| h.job = Some(sub))
        .unwrap();
    let ran3 = Rc::new(Cell::new(false));
    let r4 = ran3.clone();
    ctx.spawn(
        || 7u8,
        move |_ctx, _v| {
            r4.set(true);
            Ok(())
        },
    )
    .detach();
    ctx.kill(b);
    ctx.run_until_idle();
    assert!(
        !ran.get(),
        "callback whose handle lived in the killed buffer's mode must not run"
    );
    assert!(ran3.get(), "detached callback runs");
}

#[test]
fn render_panics_contained() {
    support::quiet_panics();
    let (mut ctx, b) = setup();
    ctx.with_surface(b, |s: &mut TermSurface, _| s.lines = 99)
        .unwrap();
    let w = ctx.open_window(b, 20, 2).unwrap();
    let f = screen(&mut ctx, w);
    assert!(f.rows[0].contains("render panicked"), "{:?}", f.rows);
}

#[test]
fn keymap_char_wildcard() {
    let (mut ctx, _) = setup();
    let b = ctx.create_text("t", "abc");
    ctx.open_window(b, 20, 3).unwrap();
    ctx.type_keys("fx");
    ctx.type_keys("ff");
    ctx.type_keys("f<Esc>");
    let log: Vec<&str> = ctx.messages.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        log,
        [
            "vim-find-char Some('x')",
            "special-ff Some('f')",
            "f<Esc> is undefined"
        ]
    );
    let _ = RefCell::new(()); // keep imports honest
}
