//! The chart's anatomy: the title, the legend, the axes and the plot's rectangle, at a size.

use crate::data::SeriesKey;
use crate::scale::Scale;
use crate::scene::Rect;

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
}
