//! A chart drawn: its model resolved against the data, its anatomy laid out, its marks and
//! chrome into a base scene, and the overlay that follows the pointer and the cursor.
//!
//! - [`layout`]: title, legend, axes and the plot's rectangle;
//! - [`marks`]: line, area, bar, point, rule, band, rect and text marks into the scene;
//! - [`overlay`]: crosshair, dots, card, chips, brush, reference and the emphasis wash.

pub mod layout;
pub mod marks;
pub mod overlay;

pub use layout::ChartLayout;
pub use overlay::{OverlayState, overlay};

use crate::algo::HitIndex;
use crate::data::{SeriesKey, Table};
use crate::error::Result;
use crate::interact::View;
use crate::scene::{Measure, Scene};
use crate::spec::Chart;
use crate::style::FigStyle;
use crate::theme::Theme;
use jiff::tz::TimeZone;
use std::sync::Arc;

/// A chart resolved against its data: series with keys and slots, scales' data extents, the
/// pyramid per series. Built off the main thread when data or spec change.
pub struct ChartModel {
    /// The chart.
    pub chart: Chart,
    /// Its tables: the chart's, then each layer's own.
    pub tables: Vec<Arc<Table>>,
    /// Its series, in source order.
    pub series: Vec<SeriesInfo>,
}

/// One series of a chart.
pub struct SeriesInfo {
    /// Its identity.
    pub key: SeriesKey,
    /// Its colour slot, given in source order (ruling 4); None past the eighth.
    pub slot: Option<u8>,
    /// Its `fig` block.
    pub style: FigStyle,
    /// The layer it's drawn in.
    pub layer: u16,
}

impl ChartModel {
    /// The model of a chart over its tables, times in `zone`.
    pub fn build(_chart: Chart, _tables: Vec<Arc<Table>>, _zone: &TimeZone) -> Result<ChartModel> {
        todo!("P2-plot: ChartModel::build")
    }
}

/// The base layer: chrome and marks, with regions.
pub fn base(
    _model: &ChartModel,
    _view: &View,
    _size: (f32, f32),
    _scale: f32,
    _theme: &Theme,
    _m: &mut dyn Measure,
) -> (Scene, ChartLayout, HitIndex) {
    todo!("P2-plot: plot::base")
}
