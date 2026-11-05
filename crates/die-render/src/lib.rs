//! die-render: DIE's window and renderer (scope §5.11, D13). It runs the window
//! through winit and, on macOS, draws die-core's scene with Metal and shapes text
//! with Core Text.
//!
//! R1 is the crate and its boundary: `platform`'s per-platform modules are the
//! only code allowed `unsafe` (scope §12), and they hold thin wrappers only. The
//! glyph atlas, row cache and instance building (R2), the window and loop (R3) and
//! input (R4) live outside them.
//!
//! That code is neutral: it doesn't branch on the platform (`cargo xtask lint`).
//! It calls the platform through `platform::backend` (see `platform`). Until
//! `platform::linux` lands, neutral code that only the macOS backend calls back
//! into is dead on Linux, which the Linux cross-check's `-D warnings` rejects. Mark
//! it `#[cfg_attr(not(target_os = "macos"), allow(dead_code))]`: a `cfg_attr` that
//! only sets a lint level is the one platform cfg neutral code may carry.
#![deny(unsafe_code)]

#[cfg(test)]
use die_test_cap as _;

pub mod platform;

use std::path::PathBuf;

/// Where DIE's fonts are: `$DIE_FONT_DIR`, else `fonts/` at the workspace root,
/// which `cargo xtask fonts` fills.
pub fn font_dir() -> PathBuf {
    match std::env::var_os("DIE_FONT_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts")),
    }
}
