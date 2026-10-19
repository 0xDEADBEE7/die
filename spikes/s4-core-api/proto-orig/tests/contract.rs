use dieproto::*;
use std::cell::Cell;
use std::rc::Rc;

struct Orchid { opened: u32 }
impl Extension for Orchid {
    fn name(&self) -> &'static str { "orchid" }
    fn register(&mut self, reg: &mut Registry) {
        reg.ext_command::<Orchid>("orchid-open", "open", |o, _ctx, _a| { o.opened += 1; Ok(()) });
        reg.ext_command::<Orchid>("orchid-boom", "panic inside checkout", |_o, _ctx, _a| panic!("orchid bug"));
        // command that runs another extension's command while holding its own checkout
        reg.ext_command::<Orchid>("orchid-nested", "", |_o, ctx, a| ctx.run("term-poke", a));
    }
}

struct Term { buf: Option<BufferId>, pokes: u32 }
struct TermSurface { lines: u32 }
impl Surface for TermSurface {
    fn kind(&self) -> &'static str { "term" }
    fn render(&mut self, f: &mut Frame) { if self.lines == 99 { panic!("render bug") } f.cells.extend("$ ".chars()); }
}
impl Extension for Term {
    fn name(&self) -> &'static str { "term" }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("term-poke", "", |ctx, _| {
            let buf = ctx.with_ext(|t: &mut Term, _| t.buf)?.ok_or(Error::Msg("no term".into()))?;
            ctx.with_surface(buf, |s: &mut TermSurface, _| s.lines += 1)?;
            Ok(())
        });
        // a command that checks out the surface and then runs a command that checks out the same surface
        reg.command("term-reenter", "", |ctx, a| {
            let buf = ctx.with_ext(|t: &mut Term, _| t.buf)?.unwrap();
            let inner = ctx.with_surface(buf, |_s: &mut TermSurface, ctx| ctx.run("term-poke", a))?;
            inner
        });
        reg.command("term-kill-self", "", |ctx, _| {
            let buf = ctx.with_ext(|t: &mut Term, _| t.buf)?.unwrap();
            ctx.with_surface(buf, |_s: &mut TermSurface, ctx| ctx.kill(buf))?;
            Ok(())
        });
        reg.keymap("text", "normal").bind(&[Pat::Key(Key::Char('f')), Pat::AnyChar], "vim-find-char");
        reg.keymap("text", "normal").bind(&[Pat::Key(Key::Char('f')), Pat::Key(Key::Char('f'))], "special-ff");
        reg.keymap("text", "normal").bind(&[Pat::Key(Key::Char('g')), Pat::Key(Key::Char('g'))], "goto-top");
    }
}

struct Fold { seen: Rc<Cell<u32>>, rev: u64 }
impl Mode for Fold {
    fn name(&self) -> &'static str { "fold" }
    fn on_edit(&mut self, c: &Change, _t: &TextBuf) { self.seen.set(self.seen.get() + 1); self.rev = c.rev; }
    fn decorate(&mut self, t: &TextBuf, _r: std::ops::Range<usize>, out: &mut Decorations) { if !t.s.is_empty() { out.faces.push((0..1, "fold")); } }
}

fn setup() -> (Ctx, Registry, BufferId) {
    let (mut ctx, reg) = Ctx::new(vec![Box::new(Orchid { opened: 0 }), Box::new(Term { buf: None, pokes: 0 })]);
    let b = ctx.create_surface("term<1>", TermSurface { lines: 0 });
    ctx.with_ext(|t: &mut Term, _| t.buf = Some(b)).unwrap();
    (ctx, reg, b)
}

#[test]
fn typed_ext_command_and_downcast() {
    let (mut ctx, _, _) = setup();
    ctx.run("orchid-open", &Args::default()).unwrap();
    assert_eq!(ctx.with_ext(|o: &mut Orchid, _| o.opened).unwrap(), 1);
}

#[test]
fn reentrant_checkout_is_busy_not_gone() {
    let (mut ctx, _, _) = setup();
    let r = ctx.run("term-reenter", &Args::default());
    println!("reenter -> {r:?}");
    assert!(matches!(r, Err(Error::Checkout(Checkout::Busy))));
    // cross-extension nesting: orchid holds orchid, runs term's command: fine
    assert!(ctx.run("orchid-nested", &Args::default()).is_ok());
}

#[test]
fn panic_inside_checkout_restores_object_and_attributes() {
    let (mut ctx, _, _) = setup();
    let r = ctx.run("orchid-boom", &Args::default());
    println!("boom -> {r:?}");
    assert!(matches!(r, Err(Error::Panicked { ext: "orchid", .. })));
    // object was put back
    assert!(ctx.run("orchid-open", &Args::default()).is_ok());
}

#[test]
fn checked_out_mode_gets_its_own_edits_at_checkin() {
    let (mut ctx, _, _) = setup();
    let b = ctx.create_text("chat");
    let seen = Rc::new(Cell::new(0));
    ctx.enable_mode(b, Fold { seen: seen.clone(), rev: 0 }).unwrap();
    ctx.edit(b, 0, "hello").unwrap();
    assert_eq!(seen.get(), 1);
    // orchid-style streaming: the mode edits its own buffer while checked out
    ctx.with_mode(b, |_f: &mut Fold, ctx| { ctx.edit(b, 5, " world").unwrap(); ctx.edit(b, 0, ">").unwrap(); }).unwrap();
    assert_eq!(seen.get(), 3, "missed edits delivered at check-in");
    let r = ctx.render();
    println!("{r:?}");
}

#[test]
fn kill_during_checkout_and_generations() {
    let (mut ctx, _, b) = setup();
    ctx.run("term-kill-self", &Args::default()).unwrap();
    assert_eq!(ctx.with_surface(b, |_s: &mut TermSurface, _| ()).err(), Some(Checkout::Gone));
    let b2 = ctx.create_surface("term<2>", TermSurface { lines: 0 });
    assert_ne!(b, b2);
    assert_eq!(ctx.with_surface(b, |_s: &mut TermSurface, _| ()).err(), Some(Checkout::Gone), "stale id stays dead after slot reuse");
}

#[test]
fn buffer_owned_job_cancelled_on_kill() {
    let (mut ctx, _, _) = setup();
    let b = ctx.create_text("orchid-chat");
    let ran = Rc::new(Cell::new(false));
    let r2 = ran.clone();
    ctx.spawn(Some(b), || { std::thread::sleep(std::time::Duration::from_millis(20)); 42u32 }, move |_ctx, v| { assert_eq!(v, 42); r2.set(true); Ok(()) });
    let ran3 = Rc::new(Cell::new(false)); let r4 = ran3.clone();
    ctx.spawn(None, || 7u8, move |_ctx, _v| { r4.set(true); Ok(()) });
    ctx.kill(b);
    ctx.pump();
    assert!(!ran.get(), "buffer-owned callback must not run");
    assert!(ran3.get());
}

#[test]
fn render_panics_contained() {
    let (mut ctx, _, b) = setup();
    ctx.with_surface(b, |s: &mut TermSurface, _| s.lines = 99).unwrap();
    let r = ctx.render();
    println!("{r:?}");
    assert!(r.iter().any(|l| l.contains("render panicked")));
}

#[test]
fn keymap_char_wildcard() {
    let (_ctx, reg, _) = setup();
    let t = reg.kind_maps.get(&("text", "normal")).unwrap();
    assert!(matches!(t.lookup(&[Key::Char('f'), Key::Char('x')]), Lookup::Command("vim-find-char")));
    assert!(matches!(t.lookup(&[Key::Char('f'), Key::Char('f')]), Lookup::Command("special-ff")));
    assert!(matches!(t.lookup(&[Key::Char('f')]), Lookup::Prefix));
    assert!(matches!(t.lookup(&[Key::Char('f'), Key::Named("Esc")]), Lookup::None));
}
