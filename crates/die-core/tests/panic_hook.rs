//! K5: the panic hook (contract §15). A hook is process-wide, so this file is its own test binary,
//! and holds one test.
//!
//! - `the_hook_keeps_the_one_before_it`: a hook set before the first `Ctx` still runs, and the
//!   line in `*messages*` gets the place from core's. A hook set after it replaces core's: the
//!   line is still logged, from the panic's own message, without the place.
use die_core::*;
use std::panic::set_hook;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Ext;
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "ext"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("boom", "", |_, _| panic!("boom"));
    }
}

#[test]
fn the_hook_keeps_the_one_before_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    set_hook(Box::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    }));
    let mut ctx = Ctx::new(vec![Box::new(Ext)]).unwrap();
    let _ = ctx.run("boom", &Args::default());
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the earlier hook ran");
    let line = ctx.messages.last().unwrap();
    assert!(line.contains("tests/panic_hook.rs:"), "{line}");

    // A second Ctx doesn't chain core's hook onto itself: the earlier hook runs once a panic.
    let mut second = Ctx::new(vec![Box::new(Ext)]).unwrap();
    let _ = second.run("boom", &Args::default());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(second.messages.len(), 1, "{:?}", second.messages);

    // A hook set afterwards replaces core's, as the tests' `quiet_panics` does.
    set_hook(Box::new(|_| {}));
    let mut third = Ctx::new(vec![Box::new(Ext)]).unwrap();
    let _ = third.run("boom", &Args::default());
    let line = third.messages.last().unwrap();
    assert!(line.starts_with("ext panicked: boom"), "{line}");
    assert!(!line.contains("panic_hook.rs"), "{line}");
}
