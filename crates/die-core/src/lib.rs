//! die-core: DIE's headless editor (scope §5, the extension contract).
//!
//! M0 starts from spike S4's prototype, ported as it was (work package K1; you
//! accepted its API as A1-A24). Its spike-quality parts, `ctx.rs` (now `ext`,
//! `dispatch` and `rt`), the layout cache and its character wrap, the key type and
//! `render.rs`, are rebuilt under these tests during M0 (docs/m0-plan.md §2).
#![forbid(unsafe_code)]

#[cfg(test)]
use die_test_cap as _;

#[cfg(panic = "abort")]
compile_error!("DIE requires panic = \"unwind\" (C9). Set it in the config crate's profile.");

mod dispatch;
mod error;
mod ext;
mod faces;
mod files;
mod ids;
mod keymap;
mod keys;
mod layout;
mod prompt;
mod rt;
mod scene;
mod text;
mod undo;
mod windows;

pub use dispatch::*;
pub use error::*;
pub use ext::*;
pub use faces::{BASE_FACES, Rgb, Style, Theme, UI_FACES, Underline};
pub use files::{FileInfo, Fingerprint, Uneditable};
pub use ids::*;
pub use keymap::*;
pub use keys::*;
pub use layout::{Dir, DisplayLine, Geometry, Stop, ViewRow, Viewport};
pub use rt::*;
pub use scene::{Cell, Cursor, CursorShape, Glyph, Scene, WindowPlacement};
pub use text::*;
