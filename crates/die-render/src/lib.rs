//! die-render: DIE's window and renderer (scope §5.11, D13). It draws die-core's
//! scene with Metal, shapes text with Core Text, and runs the window through winit.
//!
//! R1 is the crate and its boundary: `platform` is the only module allowed
//! `unsafe` (scope §12), and it holds thin wrappers only. The glyph atlas, row
//! cache and instance building (R2), the window and loop (R3) and input (R4) live
//! outside it.
#![deny(unsafe_code)]

#[allow(unsafe_code)]
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
