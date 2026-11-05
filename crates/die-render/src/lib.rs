//! die-render: DIE's window and renderer (scope §5.11, D13). It runs the window
//! through winit and, on macOS, draws die-core's scene with Metal and shapes text
//! with Core Text.
//!
//! `platform`'s per-platform modules are the only code allowed `unsafe` (scope §12),
//! and they hold thin wrappers only. The glyph atlas, row cache and instance building
//! (R2), the window and loop (R3) and input (R4) live outside them.
//!
//! A frame goes `Scene`, `rows::Frames`, then `frame::Quad`s and `frame::Upload`s.
//! All of that is data in and data out, with glyph bitmaps and metrics as values, so
//! its tests run on any platform. `driver::Renderer` is the only code that calls the
//! backend: it asks the faces for bitmaps and hands the frame to the GPU.
//!
//! That code is neutral: it doesn't branch on the platform (`cargo xtask lint`).
//! It calls the platform through `platform::backend` (see `platform`). Until
//! `platform::linux` lands, neutral code that only the macOS backend calls back
//! into is dead on Linux, which the Linux cross-check's `-D warnings` rejects. Mark
//! it `#[cfg_attr(not(target_os = "macos"), allow(dead_code))]`: a `cfg_attr` that
//! only sets a lint level is the one platform cfg neutral code may carry.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod atlas;
pub mod driver;
pub mod font;
pub mod frame;
pub mod platform;
pub mod rows;
pub mod sprite;

use std::path::PathBuf;

/// Where DIE's fonts are: `$DIE_FONT_DIR`, else `fonts/` at the workspace root,
/// which `cargo xtask fonts` fills.
pub fn font_dir() -> PathBuf {
    match std::env::var_os("DIE_FONT_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts")),
    }
}
