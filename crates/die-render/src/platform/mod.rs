//! Per-platform code: the only `unsafe` in die-render (scope §12), which you
//! review by hand, and the only place in it that branches on `target_os`
//! (docs/records/platforms.md §5.2; `cargo xtask lint` checks both).
//!
//! The rule: thin wrappers only. Each function makes one framework call or a few,
//! checks what it can, and returns safe types. Anything with logic of its own
//! (the atlas, the row cache, instance building, key mapping) lives outside this
//! module and calls in.
//!
//! macOS has `macos`. Other targets have no window backend yet: Linux gets the
//! portable backend and `platform::linux`, and R3's `run()` gets a stub there that
//! says "no window backend on this platform yet".

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;
