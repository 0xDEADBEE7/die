//! S4 spike: a headless prototype of DIE's core API (contract C2-C4, C14-C17).
//! Throwaway. Its tests carry over into die-core; its code doesn't.
#![forbid(unsafe_code)]

#[cfg(panic = "abort")]
compile_error!("DIE requires panic = \"unwind\" (C9). Set it in the config crate's profile.");

mod ctx;
mod error;
mod ids;
mod keymap;
mod keys;
mod layout;
mod render;
mod text;
mod undo;

pub use ctx::*;
pub use error::*;
pub use ids::*;
pub use keymap::*;
pub use keys::*;
pub use layout::{Cell, CellKind, Dir, DisplayLine, Geometry, Stop, ViewRow, Viewport};
pub use render::{Frame, FrameCell};
pub use text::*;
