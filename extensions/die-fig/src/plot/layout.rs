//! The chart's anatomy: the title, the legend, the axes and the plot's rectangle, at a size.

use crate::data::SeriesKey;
use crate::scale::Scale;
use crate::scene::{Measure, Rect, Scene};
use crate::theme::Theme;

/// Where everything landed: the plot rect, scales with ranges, legend rows.
pub struct ChartLayout {
    /// The plot's rectangle.
    pub plot: Rect,
    /// The x scale, onto the plot.
    pub x: Scale,
    /// The y scale, onto the plot.
    pub y: Scale,
    /// Each legend key's rectangle.
    pub legend: Vec<(SeriesKey, Rect)>,
    /// The x axis's tick-label strip: drag and scroll there act on x alone.
    pub x_axis: Rect,
    /// The y axis's tick-label strip: drag and scroll there act on y alone.
    pub y_axis: Rect,
    /// The title's box.
    pub title: Rect,
}

/// The error banner over the top of the area.
pub fn banner(_scene: &mut Scene, _text: &str, _theme: &Theme, _m: &mut dyn Measure) {
    todo!("P2-plot: plot::layout::banner")
}

/// What an empty or failed figure shows in place of a chart.
pub fn empty(
    _size: (f32, f32),
    _scale: f32,
    _text: &str,
    _theme: &Theme,
    _m: &mut dyn Measure,
) -> Scene {
    todo!("P2-plot: plot::layout::empty")
}
