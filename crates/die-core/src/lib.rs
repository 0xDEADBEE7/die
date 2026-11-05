//! die-core: DIE's headless editor (scope §5, the extension contract).
//!
//! M0 starts from spike S4's prototype, ported as it was (work package K1; you
//! accepted its API as A1-A24). Its spike-quality parts, `ctx.rs`, the layout
//! cache and its character wrap, the key type and `render.rs`, are rebuilt under
//! these tests during M0 (docs/m0-plan.md §2).
#![forbid(unsafe_code)]

#[cfg(test)]
use die_test_cap as _;

#[cfg(panic = "abort")]
compile_error!("DIE requires panic = \"unwind\" (C9). Set it in the config crate's profile.");

// S4's code as ported (K1) has a few clippy findings; changing it here would
// break the accepted port and the planted bugs' exact text. K2 and K5 rebuild
// these modules and drop the allows.
#[allow(
    clippy::collapsible_if,
    clippy::type_complexity,
    clippy::large_enum_variant
)]
#[allow(clippy::cloned_ref_to_slice_refs)]
mod ctx;
mod error;
mod faces;
mod files;
mod ids;
mod keymap;
mod keys;
mod layout;
mod scene;
#[allow(clippy::collapsible_if, clippy::if_same_then_else)]
mod text;
#[allow(clippy::collapsible_if)]
mod undo;

pub use ctx::*;
pub use error::*;
pub use faces::{BASE_FACES, Rgb, Style, Theme, UI_FACES, Underline};
pub use files::{FileInfo, Fingerprint, Uneditable};
pub use ids::*;
pub use keymap::*;
pub use keys::*;
pub use layout::{CellKind, Dir, DisplayLine, Geometry, LayoutCell, Stop, ViewRow, Viewport};
pub use scene::{Cell, Cursor, CursorShape, Glyph, Scene, WindowPlacement};
pub use text::*;
