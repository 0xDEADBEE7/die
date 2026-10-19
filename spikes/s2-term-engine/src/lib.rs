//! Spike S2: die-term's exact engine wiring, and the helper-process shape against the
//! thread-per-terminal shape. Throwaway prototype.
pub mod bench;
pub mod client;
pub mod demo;
pub mod dterm;
pub mod engine;
pub mod workgen;
pub mod helper;
pub mod hook;
pub mod host;
pub mod inproc;
pub mod proto;
pub mod pty;
pub mod shm;
pub mod surface;
pub mod util;

#[cfg(feature = "link-frameworks")]
mod frameworks {
    // Make "the same binary as DIE" load what DIE's renderer links (D13), so the helper's
    // memory includes dyld's cost for them.
    #[link(name = "AppKit", kind = "framework")]
    #[link(name = "Metal", kind = "framework")]
    #[link(name = "QuartzCore", kind = "framework")]
    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        pub fn MTLCreateSystemDefaultDevice() -> *mut std::ffi::c_void;
        pub fn NSApplicationLoad() -> bool;
        pub fn CTFontGetSize(f: *const std::ffi::c_void) -> f64;
        pub fn CACurrentMediaTime() -> f64;
    }
}

/// Keep the framework symbols referenced (only with `link-frameworks`).
pub fn touch_frameworks() -> usize {
    #[cfg(feature = "link-frameworks")]
    {
        let v = [
            frameworks::MTLCreateSystemDefaultDevice as *const () as usize,
            frameworks::NSApplicationLoad as *const () as usize,
            frameworks::CTFontGetSize as *const () as usize,
            frameworks::CACurrentMediaTime as *const () as usize,
        ];
        return std::hint::black_box(v).iter().sum();
    }
    #[allow(unreachable_code)]
    0
}
