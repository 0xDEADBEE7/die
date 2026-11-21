//! The pointer's state machine (briefs/mouse-ux.md §2.3), as the whiteboard's `Gesture`: a
//! press inside the slop, then a brush, a pan, an axis pan or a node drag, fixed at the press.

use super::view::View;
use crate::scene::Hit;
use die_core::{Button, Mods};

/// What the pointer is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    /// Nothing.
    None,
    /// The press that focused the window, a double click's second press (which acted at once),
    /// or a gesture `Esc` cut short: its drags and its release do nothing.
    Swallowed,
    /// A press still inside [`super::consts::CLICK_SLOP`].
    Pressed {
        /// Where, in logical px.
        at: (f32, f32),
        /// What it's on.
        hit: Hit,
        /// The button.
        button: Button,
        /// The modifiers.
        mods: Mods,
        /// 1 for a click, 2 for a double click.
        clicks: u8,
    },
    /// A brush, shown until the release applies it.
    Brush {
        /// The panel.
        panel: u16,
        /// What it selects.
        kind: BrushKind,
        /// Where it started.
        from: (f32, f32),
        /// Where it is.
        to: (f32, f32),
    },
    /// A pan of a panel's plot, live.
    Pan {
        /// The panel.
        panel: u16,
        /// Where it started.
        from: (f32, f32),
        /// The view at the press, for undo and Esc.
        before: View,
    },
    /// A pan of one axis, live.
    AxisPan {
        /// The panel.
        panel: u16,
        /// The axis.
        axis: Axis,
        /// Where it started.
        from: (f32, f32),
        /// The view at the press.
        before: View,
    },
    /// A node moved by hand, live; it stays pinned.
    NodeDrag {
        /// The node's id.
        node: Box<str>,
        /// Where on the node it was taken, from its centre.
        grab: (f32, f32),
        /// Where it was, and whether it was pinned.
        before: ((f64, f64), bool),
    },
}

/// What a brush selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushKind {
    /// A span of x.
    X,
    /// A span of y.
    Y,
    /// A box.
    Box,
    /// A run of bands.
    Bands,
}

/// One of a panel's axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Across.
    X,
    /// Up.
    Y,
}
