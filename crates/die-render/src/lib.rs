//! die-render: DIE's window and renderer (scope §5.11, D13). It runs the window
//! through winit and, on macOS, draws die-core's scene with Metal and shapes text
//! with Core Text.
//!
//! R1 is the crate and its boundary: `platform`'s per-platform modules are the
//! only code allowed `unsafe` (scope §12), and they hold thin wrappers only. The
//! glyph atlas, row cache and instance building (R2), the window and loop (R3) and
//! input (R4) live outside them.
#![deny(unsafe_code)]

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
