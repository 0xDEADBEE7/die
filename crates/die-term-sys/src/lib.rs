//! die-term-sys: the engine's C API for libghostty-rs, which the workspace's `[patch]` points at
//! this crate instead of its own sys crate (D17). The archive it links is Ghostty's signed `tip`
//! build, cut down to one target by `cargo xtask ghostty bump` and checked by `cargo xtask ghostty
//! verify` (docs/records/die-term-engine.md §5). The wrapper expects exactly this API.

#[cfg(test)]
use die_test_cap as _;

// The FFI boundary, and the only module of the crate that may use `unsafe` (scope §12).
#[allow(unsafe_code)]
mod ffi;

pub use ffi::*;
