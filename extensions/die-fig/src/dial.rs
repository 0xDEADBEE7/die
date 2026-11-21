//! The die-fig dial, the context menu: a port of the whiteboard's (`die-whiteboard/src/dial.rs`)
//! drawn into a [`Scene`], all its text in pixels. Its geometry, constants, flick and close rules
//! are the whiteboard's; its eight sectors are north detail, north-east compare, east only,
//! south-east table or pin, south view, south-west export, west hide and north-west settings
//! (briefs/mouse-ux.md §7.3), dimmed where they don't apply, with stage two by angle for view,
//! export and settings.

use crate::scene::{Measure, Scene};
use crate::theme::Theme;

/// An open dial: its anchor, its stage, the sector hovered, stage two's choice, whether a
/// release picks.
pub struct DialState {
    _anchor: (f32, f32),
    _cell: (f32, f32),
}

/// What the dial offers here.
pub struct DialSpec {
    /// The sectors, north first then clockwise.
    pub sectors: [Sector; 8],
}

/// One sector: its name, whether it applies, and its stage-two choices if it has any.
pub struct Sector {
    /// What it says.
    pub name: &'static str,
    /// Whether it applies to the target; dimmed if not.
    pub enabled: bool,
    /// Its stage-two choices, by angle along the arc.
    pub stage2: Vec<&'static str>,
}

/// What a release or a press picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialPick {
    /// Nothing: the dead zone, a dimmed sector, or the dial stays open.
    None,
    /// A sector.
    Sector(u8),
    /// A sector's stage-two choice.
    Stage2(u8, u8),
}

impl DialState {
    /// A dial opened at `anchor`, for cells of `cell` logical px.
    pub fn open(_anchor: (f32, f32), _cell: (f32, f32)) -> DialState {
        todo!("P1-dial: DialState::open")
    }

    /// The pointer moved to `to`.
    pub fn moved(&mut self, _to: (f32, f32), _spec: &DialSpec) {
        todo!("P1-dial: DialState::moved")
    }

    /// The button that opened it came up.
    pub fn release(&mut self, _spec: &DialSpec) -> DialPick {
        todo!("P1-dial: DialState::release")
    }

    /// A left press while it's open.
    pub fn press(&mut self, _at: (f32, f32), _spec: &DialSpec) -> DialPick {
        todo!("P1-dial: DialState::press")
    }
}

/// Draws the dial into `scene`, with its regions.
pub fn draw(
    _st: &DialState,
    _spec: &DialSpec,
    _theme: &Theme,
    _scene: &mut Scene,
    _m: &mut dyn Measure,
) {
    todo!("P1-dial: dial::draw")
}
