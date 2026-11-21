//! What the pointer is on.

use super::{ChartLayout, ChartModel};
use crate::algo::HitIndex;
use crate::data::SeriesKey;
use crate::interact::{Cursor, View};

/// What the pointer at (x, y) is on, as a cursor; None off the plot.
pub fn hover(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _hits: &HitIndex,
    _view: &View,
    _x: f32,
    _y: f32,
) -> Option<Cursor> {
    todo!("P2-plot: plot::hover::hover")
}

/// The series nearest the pointer at a hover, for emphasis; None past NEAREST_SERIES.
pub fn nearest_series(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _hits: &HitIndex,
    _view: &View,
    _x: f32,
    _y: f32,
) -> Option<SeriesKey> {
    todo!("P2-plot: plot::hover::nearest_series")
}
