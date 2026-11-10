//! die-core: DIE's headless editor (scope §5, the extension contract).
//!
//! M0 starts from spike S4's prototype, ported as it was (work package K1; you
//! accepted its API as A1-A24). Its spike-quality parts, `ctx.rs` (now `ext`,
//! `dispatch` and `rt`), the layout cache and its character wrap, the key type and
//! `render.rs`, are rebuilt under these tests during M0 (docs/m0-plan.md §2).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

#[cfg(panic = "abort")]
compile_error!("DIE requires panic = \"unwind\" (C9). Set it in the config crate's profile.");

mod app;
mod clients;
mod commands;
mod dispatch;
mod error;
mod ext;
mod faces;
mod fed;
mod files;
mod harness;
mod ids;
pub mod instance;
mod keymap;
mod keys;
mod layout;
mod menu;
mod messages;
mod pixels;
mod pointer;
pub mod process;
mod project;
mod prompt;
pub mod pty;
mod rt;
mod scene;
pub mod session;
mod surface;
pub mod sys;
mod text;
mod undo;
mod windows;
mod workspace;

pub use app::{Button, Input, PointerEvent, PointerKind, TURN_LIMIT};
pub use clients::{Outcome, Waited};
pub use commands::{CommandDescription, KeyAction, KeyDescription};
pub use dispatch::*;
pub use error::*;
pub use ext::*;
pub use faces::{BASE_FACES, Rgb, Style, Theme, UI_FACES, Underline};
pub use files::{FileInfo, FileTarget, Fingerprint, StatusTextFn, Uneditable};
pub use harness::{FakeClipboard, FakePixelSink, Harness, KeyLog, PixelSinkCall};
pub use ids::*;
pub use keymap::*;
pub use keys::*;
pub use layout::{Dir, DisplayLine, Geometry, Stop, ViewRow, Viewport};
pub use pixels::{PIXEL_MAX_SIDE, PixelBufferId, PixelProducer, PixelRect, PixelSink};
pub use pointer::{CellHit, WindowPart};
pub use project::{PathSource, project_root, walk};
pub use prompt::{PromptSpec, flex_rank, recent_first};
pub use pty::{Pty, PtyCommand, PtySize};
pub use rt::*;
pub use scene::{Cell, Cursor, CursorShape, Glyph, Placement, Scene, WindowPlacement};
pub use surface::{
    CellPixels, Color, Frame, FrameCell, RenderInfo, SurfaceCursor, SurfacePointer, SurfaceText,
};
pub use text::*;
pub use windows::{Place, PlaceList};
pub use workspace::{Clipboard, RegKind, Register, ShowIn, Side};
