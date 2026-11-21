//! The cursor's motions and the view's changes, data-aware, with limits.

use super::{ChartLayout, ChartModel};
use crate::interact::{Action, Axis, BrushKind, Cursor, View};

/// Step, Series, Peak, First, Last and ViewEdge, `count` times.
pub fn motion(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _view: &View,
    _cursor: Option<&Cursor>,
    _action: &Action,
    _count: u32,
) -> Option<Cursor> {
    todo!("P2-plot: plot::nav::motion")
}

/// The view scrolled so `cursor` sits at least 10 % inside the plot.
pub fn reveal(_model: &ChartModel, _layout: &ChartLayout, _view: &View, _cursor: &Cursor) -> View {
    todo!("P2-plot: plot::nav::reveal")
}

/// Zoom by `factor` (> 1 zooms in) about `at`, on one axis or the figure's zoomable axes.
#[allow(clippy::too_many_arguments)]
pub fn zoom(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _view: &View,
    _factor: f64,
    _at: Option<(f32, f32)>,
    _cursor: Option<&Cursor>,
    _axis: Option<Axis>,
) -> View {
    todo!("P2-plot: plot::nav::zoom")
}

/// Pan by (dx, dy) logical px of pointer motion, clamped to the extent.
pub fn pan(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _view: &View,
    _dx: f32,
    _dy: f32,
    _axis: Option<Axis>,
) -> View {
    todo!("P2-plot: plot::nav::pan")
}

/// The view a released brush asks for.
pub fn brush(
    _model: &ChartModel,
    _layout: &ChartLayout,
    _view: &View,
    _kind: BrushKind,
    _from: (f32, f32),
    _to: (f32, f32),
) -> Option<View> {
    todo!("P2-plot: plot::nav::brush")
}

/// The brush a left drag on the plot makes here.
pub fn brush_kind(_model: &ChartModel) -> BrushKind {
    todo!("P2-plot: plot::nav::brush_kind")
}

/// Reset one axis or both to fit.
pub fn reset(_model: &ChartModel, _view: &View, _axis: Option<Axis>) -> View {
    todo!("P2-plot: plot::nav::reset")
}

/// Centre the view on the cursor (zz).
pub fn centre(_model: &ChartModel, _layout: &ChartLayout, _view: &View, _cursor: &Cursor) -> View {
    todo!("P2-plot: plot::nav::centre")
}

/// A view part way between two, for 180 ms eases.
pub fn tween(_from: &View, _to: &View, _t: f32) -> View {
    todo!("P2-plot: plot::nav::tween")
}
