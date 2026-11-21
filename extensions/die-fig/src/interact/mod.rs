//! Interaction: the keyboard's cursor on the data, the pointer's gestures, the view and its
//! undo, fig's semantic actions both produce, and easing (briefs/mouse-ux.md).

pub mod action;
pub mod consts;
pub mod cursor;
pub mod gesture;
pub mod motion;
pub mod view;

pub use action::Action;
pub use cursor::Cursor;
pub use gesture::{Axis, BrushKind, Gesture};
pub use motion::{Tween, ease_out_cubic};
pub use view::{View, ViewStack};
