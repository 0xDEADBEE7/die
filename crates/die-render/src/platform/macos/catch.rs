//! Catching Objective-C exceptions, which AppKit raises when, say, the pasteboard server can't be
//! reached. Rust can't: one that reaches a `catch_unwind`, and winit puts one round every event
//! handler, aborts with "Rust cannot catch foreign exceptions". So a call that can raise goes
//! through [`guarded`], and the caller reports the error.
//!
//! objc2's `exception` feature does this by compiling a `.m` file through `cc`, which D16 keeps
//! out of the build. This is that file instead, as clang writes it for aarch64, the one Mac target
//! (D12), in `catch.s`:
//!
//! ```objc
//! unsigned char die_objc_try(void (*f)(void *), void *ctx, void **out) {
//!     @try { f(ctx); return 0; } @catch (id e) { *out = objc_retain(e); return 1; }
//! }
//! ```
//!
//! A Rust panic isn't an Objective-C object, so it goes through this untouched.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::NSException;
use std::ffi::c_void;
use std::ptr::null_mut;

#[cfg(not(target_arch = "aarch64"))]
compile_error!("die_objc_try is written for aarch64: the only Mac target (D12)");

std::arch::global_asm!(include_str!("catch.s"));

unsafe extern "C-unwind" {
    fn die_objc_try(
        f: extern "C-unwind" fn(*mut c_void),
        ctx: *mut c_void,
        out: *mut *mut AnyObject,
    ) -> u8;
}

/// Run `f`, once. What it raised, if it raised, and it may be nil.
fn catch<F: FnOnce()>(f: F) -> Result<(), Option<Retained<AnyObject>>> {
    extern "C-unwind" fn call<F: FnOnce()>(ctx: *mut c_void) {
        // SAFETY: `ctx` is the `slot` below, of this `F`, and nothing else has it during this call.
        let f = unsafe { &mut *ctx.cast::<Option<F>>() }.take();
        if let Some(f) = f {
            f();
        }
    }
    let mut slot = Some(f);
    let mut caught = null_mut();
    // SAFETY: it calls `call` with the pointer, and `slot` outlives it. It writes `caught` only to
    // hand over an object it retained.
    let raised = unsafe { die_objc_try(call::<F>, (&raw mut slot).cast(), &raw mut caught) };
    match raised {
        0 => Ok(()),
        // SAFETY: as above: that retain is this one to release, or it's nil.
        _ => Err(unsafe { Retained::from_raw(caught) }),
    }
}

/// Run an AppKit call. If it raises an Objective-C exception, an error that says it was `what` and
/// what it raised.
pub fn guarded<R>(what: &str, f: impl FnOnce() -> R) -> Result<R, String> {
    let mut value = None;
    catch(|| value = Some(f())).map_err(|e| format!("{what}: {}", describe(e.as_deref())))?;
    value.ok_or_else(|| format!("{what}: it returned nothing"))
}

fn describe(e: Option<&AnyObject>) -> String {
    match e.and_then(|e| e.downcast_ref::<NSException>()) {
        Some(e) => match e.reason() {
            Some(reason) => format!("{}: {reason}", e.name()),
            None => e.name().to_string(),
        },
        None => "an Objective-C exception that isn't an NSException".into(),
    }
}
