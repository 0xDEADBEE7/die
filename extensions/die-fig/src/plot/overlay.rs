//! The overlay: crosshair, dots, the card, the axes' chips, the brush, the reference and the
//! emphasis wash, drawn over the base and kept close so its damage stays small (ruling 10).

use super::{ChartLayout, ChartModel};
use crate::data::SeriesKey;
use crate::interact::{BrushKind, Cursor, View};
use crate::scene::{Measure, Scene};
use crate::theme::Theme;

/// What the overlay shows.
pub struct OverlayState {
    /// What the pointer is over.
    pub hover: Option<Cursor>,
    /// The keyboard's cursor.
    pub cursor: Option<Cursor>,
    /// Where the pointer is, in logical px.
    pub pointer: Option<(f32, f32)>,
    /// A brush being drawn: its kind and corners.
    #[allow(clippy::type_complexity)]
    pub brush: Option<(BrushKind, (f32, f32), (f32, f32))>,
    /// The series emphasised.
    pub emphasis: Option<SeriesKey>,
}

/// The overlay's scene.
pub fn overlay(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _view: &View,
    _st: &OverlayState,
    _theme: &Theme,
    _m: &mut dyn Measure,
) -> Scene {
    todo!("P2-plot: plot::overlay")
}
