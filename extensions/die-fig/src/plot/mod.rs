//! A chart drawn: its model resolved against the data, its anatomy laid out, its marks and
//! chrome into a base scene, and the overlay that follows the pointer and the cursor.
//!
//! - [`layout`]: title, legend, axes and the plot's rectangle;
//! - [`marks`]: line, area, bar, point, rule, band, rect and text marks into the scene;
//! - [`overlay`]: crosshair, dots, card, chips, brush, reference and the emphasis wash;
//! - [`nav`]: the cursor's motions and the view's changes, data-aware, with limits;
//! - [`hover`]: what the pointer is on.
//!
//! P2-surface's worktree: contract-p2 §2's signatures with no bodies yet, for the surface to
//! build against. fig/plot's bodies replace this file whole.

pub mod hover;
pub mod layout;
pub mod marks;
pub mod nav;
pub mod overlay;

pub use layout::ChartLayout;
pub use overlay::{OverlayState, overlay};

use crate::algo::HitIndex;
use crate::data::{SeriesKey, Table};
use crate::error::Result;
use crate::interact::{Cursor, View};
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
    /// Its x values, sorted.
    pub xs: Arc<[f64]>,
    /// Its y values, NaN where missing.
    pub ys: Arc<[f64]>,
}

impl ChartModel {
    /// The model of a chart over its tables, times in `zone`.
    pub fn build(_chart: Chart, _tables: Vec<Arc<Table>>, _zone: &TimeZone) -> Result<ChartModel> {
        todo!("P2-plot: ChartModel::build")
    }

    /// An x-chart: x is time or an ordered number and y fits the visible data.
    pub fn is_x_chart(&self) -> bool {
        todo!("P2-plot: ChartModel::is_x_chart")
    }

    /// A band chart: x (or y when horizontal) is categories.
    pub fn is_band(&self) -> bool {
        todo!("P2-plot: ChartModel::is_band")
    }

    /// The data's extent, which zoom-out and pan never pass.
    pub fn extent(&self) -> ((f64, f64), (f64, f64)) {
        todo!("P2-plot: ChartModel::extent")
    }

    /// The x and y domains that fit what's visible under `view`.
    pub fn fit(&self, _view: &View) -> ((f64, f64), (f64, f64)) {
        todo!("P2-plot: ChartModel::fit")
    }

    /// Labels a search can find, with where the cursor goes.
    pub fn labels(&self) -> Vec<(String, Cursor)> {
        todo!("P2-plot: ChartModel::labels")
    }

    /// The data as TSV, in the view's order, hidden series left out; the row a cursor is on.
    pub fn tsv(&self, _view: &View) -> (String, Option<usize>) {
        todo!("P2-plot: ChartModel::tsv")
    }

    /// The newest x, for live follow.
    pub fn newest(&self) -> Option<f64> {
        todo!("P2-plot: ChartModel::newest")
    }

    /// The series keys in legend order.
    pub fn keys(&self) -> Vec<SeriesKey> {
        todo!("P2-plot: ChartModel::keys")
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
