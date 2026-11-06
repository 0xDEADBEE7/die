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
//! backend: it asks the faces for bitmaps and hands the frame to the GPU. `window`
//! is the loop around it, and `check` holds that loop to S1's rules.
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

pub mod atlas;
pub mod check;
pub mod driver;
pub mod font;
pub mod frame;
pub mod input;
pub mod latency;
pub mod platform;
pub mod rows;
pub mod sprite;
pub mod window;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where DIE's fonts are: `$DIE_FONT_DIR`, else the app bundle's, else `fonts/` at the
/// workspace root, which `cargo xtask fonts` fills.
pub fn font_dir() -> PathBuf {
    let exe = std::env::current_exe().ok();
    font_dir_from(std::env::var_os("DIE_FONT_DIR"), exe.as_deref())
}

/// [`font_dir`], given `$DIE_FONT_DIR` and this executable's path.
pub fn font_dir_from(var: Option<OsString>, exe: Option<&Path>) -> PathBuf {
    match (var, exe.and_then(bundle_of)) {
        (Some(d), _) => PathBuf::from(d),
        (None, Some(bundle)) => bundle.join("Contents/Resources/fonts"),
        (None, None) => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts")),
    }
}

/// The app bundle holding this executable, if it's at `<name>.app/Contents/MacOS/<exe>`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = macos.ends_with("MacOS") && contents.ends_with("Contents");
    (named && bundle.extension()? == "app").then(|| bundle.to_path_buf())
}
