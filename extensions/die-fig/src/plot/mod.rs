//! A chart drawn: its model resolved against the data, its anatomy laid out, its marks and
//! chrome into a base scene, and the overlay that follows the pointer and the cursor.
//!
//! - this file: [`ChartModel`], the chart resolved against its tables (series with keys and
//!   colour slots, the axes' kinds, the rules and notes), what fits a [`View`], and [`base`];
//! - [`layout`]: the title, the legend, the axes and the plot's rectangle at a size;
//! - [`marks`]: line, area, bar, point, rule, band, rect and text marks into the scene;
//! - [`overlay`]: the crosshair, dots, card, chips, brush, reference and the emphasis wash;
//! - [`nav`]: the cursor's motions and the view's zoom, pan, brush and reset, within limits;
//! - [`hover`]: what the pointer is on;
//! - [`ramp`]: a heatmap's colour scale, sequential or diverging, and its legend;
//! - [`detail`]: the detail panel's trees for a point, a series, a band and a cell, and the
//!   names a search looks through.
//!
//! Positions are logical px in the figure's area. A model's x is its independent axis and its y
//! the values, whichever way they're drawn: horizontal bars draw x (the bands) up the screen,
//! and a [`View`]'s `x` and `y` are the model's, not the screen's. [`ChartLayout`]'s scales are
//! the screen's.

pub mod detail;
pub mod errors;
pub mod hover;
pub mod layout;
pub mod marks;
pub mod nav;
pub mod overlay;
pub mod ramp;

pub use errors::{ErrorView, LogLayout, error_figure, log_layout, log_page, stale_badge};
pub use hover::{hover, nearest_series};
pub use layout::ChartLayout;
pub use nav::{brush, brush_bands, brush_kind, centre, motion, pan, reset, reveal, tween, zoom};
pub use overlay::{OverlayState, overlay};

use crate::algo::{HitIndex, Pyramid};
use crate::data::transform::{Transform, apply};
use crate::data::{Column, ColumnKind, ReadOptions, SeriesKey, Table, Values};
use crate::error::{FigError, Result};
use crate::interact::{Cursor, View};
use crate::scale::{self, format_time_full, format_value, format_with};
use crate::scene::{Measure, Scene};
use crate::spec::{
    Channel, ChannelValue, Chart, DataFormat, DataRef, DataSpec, MarkKind, ScaleKind, Stack,
};
use crate::style::{ColorSpec, FigStyle};
use crate::theme::{Color, Theme, color_of};
use jiff::tz::TimeZone;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// How an axis of the model places its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisKind {
    /// Numbers, linearly.
    Linear,
    /// Positive numbers, by their logarithm.
    Log,
    /// Times: seconds since the epoch, shown in the model's zone.
    Time,
    /// Categories, a band each, at 0, 1, 2 … in the model's values.
    Band,
}

/// One of the model's axes: its kind, title, format, categories and what the spec pins.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisInfo {
    /// How it places values.
    pub kind: AxisKind,
    /// Its title as shown, if it has one.
    pub title: Option<String>,
    /// The field it shows, if one.
    pub field: Option<String>,
    /// An explicit format for its values.
    pub format: Option<String>,
    /// Its categories, in first-seen order, when it's a band axis.
    pub bands: Vec<Box<str>>,
    /// The domain the spec pins, over the data's.
    pub domain: Option<(f64, f64)>,
    /// Whether the spec says the domain takes in zero.
    pub zero: Option<bool>,
}

/// A rule: a line across the plot at a value, a threshold or a target.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleMark {
    /// Up the plot at an x (true), or across it at a y.
    pub vertical: bool,
    /// Where, in data.
    pub value: f64,
    /// What it's called, drawn at its end; dashed when it has one.
    pub label: Option<String>,
    /// Its colour, from its `fig` block.
    pub color: Option<ColorSpec>,
    /// Dashed whatever its label.
    pub dash: bool,
    /// Its layer.
    pub layer: u16,
}

/// A note: text at a place in the data.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMark {
    /// Across, in data (a band's index on a band axis).
    pub x: f64,
    /// Up, in data.
    pub y: f64,
    /// What it says.
    pub text: Box<str>,
    /// Its layer.
    pub layer: u16,
}

/// A chart resolved against its data: series with keys and slots, scales' data extents, the
/// pyramid per series. Built off the main thread when data or spec change.
pub struct ChartModel {
    /// The chart.
    pub chart: Chart,
    /// Its tables as given: the chart's, then each layer's own.
    pub tables: Vec<Arc<Table>>,
    /// Each layer's table as drawn, its transforms applied: what a series' `rows` index, for
    /// the detail panel's row of the data.
    pub drawn: Vec<Arc<Table>>,
    /// Its series, in source order: each layer's in turn, the nth of a name across the chart
    /// (ruling 3). A band and a line of one name in two layers (a line and its band, coloured
    /// by one field) share a key: one series to the legend, the colour and the hidden set.
    pub series: Vec<SeriesInfo>,
    /// The zone its times are shown in.
    pub zone: TimeZone,
    /// Its independent axis.
    pub x: AxisInfo,
    /// Its value axis.
    pub y: AxisInfo,
    /// Bars across: the bands go up the screen and the values across.
    pub horizontal: bool,
    /// Its rules.
    pub rules: Vec<RuleMark>,
    /// Its notes.
    pub texts: Vec<TextMark>,
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
    /// What it's called: its `fig` block's label, else its key's name.
    pub name: String,
    /// How it's drawn.
    pub mark: MarkKind,
    /// Its samples' x, ascending on a number or time axis (a band's index on a band axis).
    pub xs: Arc<[f64]>,
    /// Its samples' values; NaN is missing (a gap in a line).
    pub ys: Arc<[f64]>,
    /// A band's other end, per sample.
    pub y2s: Option<Arc<[f64]>>,
    /// A bubble's size, per sample.
    pub sizes: Option<Arc<[f64]>>,
    /// A histogram bin's end on x, per sample.
    pub ends: Option<Arc<[f64]>>,
    /// Each sample's row in its layer's table.
    pub rows: Arc<[u32]>,
    /// The min/max pyramid of a line or area.
    pub pyramid: Option<Pyramid>,
    /// The median distance between its samples' x (0 when it has fewer than two).
    pub interval: f64,
    /// Its layer stacks it on the others (bars and areas).
    pub stack: Stack,
    /// The decimals its values need to show their own precision, for the card's column.
    pub decimals: usize,
    /// The decimals its x needs, on a number axis.
    pub x_decimals: usize,
}

impl SeriesInfo {
    /// Whether its samples are x-sorted numbers a line runs through.
    fn continuous(&self) -> bool {
        matches!(self.mark, MarkKind::Line | MarkKind::Area | MarkKind::Band)
    }

    /// The index of the first and last sample with a value.
    fn present_ends(&self) -> Option<(usize, usize)> {
        let first = (0..self.ys.len()).find(|&i| !self.ys[i].is_nan() && !self.xs[i].is_nan())?;
        let last = (0..self.ys.len())
            .rev()
            .find(|&i| !self.ys[i].is_nan() && !self.xs[i].is_nan())?;
        Some((first, last))
    }
}

/// Each series' stacked extent under a view: the bottom and top of each sample, for the series
/// a layer stacks; None for one it doesn't, or that's hidden.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stacks {
    /// Each series' samples' bottoms.
    pub lo: Vec<Option<Arc<[f64]>>>,
    /// Their tops.
    pub hi: Vec<Option<Arc<[f64]>>>,
}

/// A float as a key that sorts and compares exactly, -0 and 0 one.
fn bits(v: f64) -> u64 {
    if v == 0.0 { 0 } else { v.to_bits() }
}

/// What a channel holds once read: numbers (times as seconds), categories, or a constant.
enum Read {
    Numbers(Vec<f64>),
    Strings(Vec<Option<Box<str>>>),
    Const(f64),
    ConstStr(String),
    Nothing,
}

impl ChartModel {
    /// The model of a chart over its tables, times in `zone`.
    ///
    /// `tables` are the chart's data and then each layer's own, in layer order, as read; a
    /// table the spec holds itself (`Inline`) needn't be given. The chart's transforms run on
    /// its data, then each layer's on what it draws.
    pub fn build(chart: Chart, tables: Vec<Arc<Table>>, zone: &TimeZone) -> Result<ChartModel> {
        let opt = ReadOptions::new(zone.clone());
        let mut given = tables.iter().cloned();
        let inline = |d: &DataSpec| match &d.source {
            DataRef::Inline(t) => Some(Arc::clone(t)),
            _ => None,
        };
        let chart_raw = given
            .next()
            .or_else(|| inline(&chart.data))
            .ok_or_else(|| FigError::new("no data for the chart"))?;
        let chart_table = if chart.transform.is_empty() {
            Arc::clone(&chart_raw)
        } else {
            Arc::new(apply(&chart_raw, &chart.transform)?)
        };
        let horizontal = chart
            .layers
            .iter()
            .find(|l| !annotation(l.mark.kind))
            .is_some_and(|l| l.mark.horizontal && l.mark.kind == MarkKind::Bar);
        let mut b = Builder::new(horizontal, opt);
        let mut drawn = Vec::with_capacity(chart.layers.len());
        for (li, layer) in chart.layers.iter().enumerate() {
            let base = match &layer.data {
                None => Arc::clone(&chart_table),
                Some(d) => given
                    .next()
                    .or_else(|| inline(d))
                    .ok_or_else(|| FigError::new(format!("no data for layer {}", li + 1)))?,
            };
            let t = if layer.transform.is_empty() {
                base
            } else {
                Arc::new(apply(&base, &layer.transform)?)
            };
            let ts: Vec<&Transform> = chart.transform.iter().chain(&layer.transform).collect();
            b.layer(li as u16, layer, &t, &ts, &chart)?;
            drawn.push(t);
        }
        let (x, y) = b.axes(&chart);
        let mut series = b.series;
        give_slots(&mut series);
        Ok(ChartModel {
            chart,
            tables,
            drawn,
            series,
            zone: zone.clone(),
            x,
            y,
            horizontal,
            rules: b.rules,
            texts: b.texts,
        })
    }

    /// An x-chart: x is time or an ordered number and y fits the visible data (lines, areas,
    /// bars over time). Scroll pans x in both directions there (mouse-ux §3.2).
    pub fn is_x_chart(&self) -> bool {
        if self.is_band() {
            return false;
        }
        self.x.kind == AxisKind::Time
            || self.series.iter().any(|s| {
                matches!(
                    s.mark,
                    MarkKind::Line | MarkKind::Area | MarkKind::Bar | MarkKind::Band
                )
            })
    }

    /// A band chart: x (or y when horizontal) is categories.
    pub fn is_band(&self) -> bool {
        self.x.kind == AxisKind::Band
    }

    /// A scatter: points over two number axes, free in both.
    pub fn is_scatter(&self) -> bool {
        !self.is_band() && !self.is_x_chart()
    }

    /// A heatmap: rects over two band axes.
    pub fn is_heatmap(&self) -> bool {
        self.series.iter().any(|s| s.mark == MarkKind::Rect)
    }

    /// Whether the value axis starts at zero and never zooms: bars and stacked areas.
    pub fn fixed_y(&self) -> bool {
        self.series.iter().any(|s| {
            s.mark == MarkKind::Bar || (s.mark == MarkKind::Area && s.stack != Stack::None)
        })
    }

    /// Whether `key` shows under `view`.
    pub fn visible(&self, view: &View, key: &SeriesKey) -> bool {
        !view.hidden.contains(key) && view.only.as_ref().is_none_or(|o| o == key)
    }

    /// The series keys in legend order.
    pub fn keys(&self) -> Vec<SeriesKey> {
        let mut out: Vec<SeriesKey> = Vec::new();
        for s in &self.series {
            if !out.contains(&s.key) {
                out.push(s.key.clone());
            }
        }
        out
    }

    /// The first series of a key.
    pub fn series_of(&self, key: &SeriesKey) -> Option<&SeriesInfo> {
        self.series.iter().find(|s| &s.key == key)
    }

    /// The index in [`ChartModel::series`] of the series the cursor's key names, preferring a
    /// line's (the one the cursor walks).
    pub fn index_of(&self, key: &SeriesKey) -> Option<usize> {
        let mut first = None;
        for (i, s) in self.series.iter().enumerate() {
            if &s.key == key {
                if s.mark != MarkKind::Band {
                    return Some(i);
                }
                first.get_or_insert(i);
            }
        }
        first
    }

    /// The view a figure opens with: the series its `fig` blocks hide hidden (ruling 15).
    pub fn initial_view(&self) -> View {
        let mut v = View::default();
        if self.chart.view.render_options {
            for s in &self.series {
                if s.style.hidden {
                    v.hidden.insert(s.key.clone());
                }
            }
        }
        v
    }

    /// A series' colour: its `fig` block's, else its slot's, else (series 9 and up, and a
    /// scatter's fourth on) `other` (ruling 4).
    pub fn color(&self, s: &SeriesInfo, theme: &Theme) -> Color {
        if self.chart.view.render_options
            && let Some(c) = s.style.color
        {
            return color_of(c, theme);
        }
        let cap = if s.mark == MarkKind::Point { 3 } else { 8 };
        match s.slot {
            Some(i) if i < cap => theme.series[usize::from(i)],
            _ => theme.other,
        }
    }

    /// The colour of the series a key names.
    pub fn color_of_key(&self, key: &SeriesKey, theme: &Theme) -> Color {
        self.series_of(key)
            .map_or(theme.other, |s| self.color(s, theme))
    }

    /// The line width a series draws at: 2, 3 or 4 px by its `fig` size (ruling 15).
    pub fn line_width(&self, s: &SeriesInfo) -> f32 {
        match self.size(s) {
            2 => 3.0,
            3 => 4.0,
            _ => 2.0,
        }
    }

    /// A point's diameter: 8, 11 or 14 px by its `fig` size.
    pub fn point_size(&self, s: &SeriesInfo) -> f32 {
        match self.size(s) {
            2 => 11.0,
            3 => 14.0,
            _ => 8.0,
        }
    }

    fn size(&self, s: &SeriesInfo) -> u8 {
        if self.chart.view.render_options {
            s.style.size.unwrap_or(1).clamp(1, 3)
        } else {
            1
        }
    }

    /// Each stacked series' bottoms and tops under `view`: hidden series take no room.
    pub fn stacks(&self, view: &View) -> Stacks {
        let n = self.series.len();
        let mut out = Stacks {
            lo: vec![None; n],
            hi: vec![None; n],
        };
        let mut layers: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
        for (i, s) in self.series.iter().enumerate() {
            if s.stack != Stack::None
                && matches!(s.mark, MarkKind::Bar | MarkKind::Area)
                && self.visible(view, &s.key)
            {
                layers.entry(s.layer).or_default().push(i);
            }
        }
        for members in layers.values() {
            let normalize = self.series[members[0]].stack == Stack::Normalize;
            let mut total: BTreeMap<u64, f64> = BTreeMap::new();
            if normalize {
                for &i in members {
                    let s = &self.series[i];
                    for (x, y) in s.xs.iter().zip(s.ys.iter()) {
                        if y.is_finite() {
                            *total.entry(bits(*x)).or_default() += y.abs();
                        }
                    }
                }
            }
            let mut pos: BTreeMap<u64, f64> = BTreeMap::new();
            let mut neg: BTreeMap<u64, f64> = BTreeMap::new();
            for &i in members {
                let s = &self.series[i];
                let (mut lo, mut hi) = (Vec::with_capacity(s.ys.len()), Vec::new());
                hi.reserve(s.ys.len());
                for (x, y) in s.xs.iter().zip(s.ys.iter()) {
                    let k = bits(*x);
                    let mut v = if y.is_finite() { *y } else { 0.0 };
                    if normalize {
                        let t = total.get(&k).copied().unwrap_or(0.0);
                        v = if t > 0.0 { v / t } else { 0.0 };
                    }
                    let acc = if v < 0.0 { &mut neg } else { &mut pos };
                    let b = acc.entry(k).or_default();
                    if y.is_nan() {
                        lo.push(f64::NAN);
                        hi.push(f64::NAN);
                        continue;
                    }
                    lo.push(*b);
                    *b += v;
                    hi.push(*b);
                }
                out.lo[i] = Some(lo.into());
                out.hi[i] = Some(hi.into());
            }
        }
        out
    }

    /// A sample's extent on the value axis under `stacks`: a stacked one's bottom and top, a
    /// bar's or area's from zero, a band's two ends, else its value twice.
    pub fn span(&self, stacks: &Stacks, si: usize, i: usize) -> (f64, f64) {
        let s = &self.series[si];
        if let (Some(lo), Some(hi)) = (&stacks.lo[si], &stacks.hi[si]) {
            return (lo[i], hi[i]);
        }
        let y = s.ys[i];
        match s.mark {
            MarkKind::Bar | MarkKind::Area => (0.0, y),
            MarkKind::Band => {
                let y2 = s.y2s.as_ref().map_or(y, |v| v[i]);
                (y.min(y2), y.max(y2))
            }
            _ => (y, y),
        }
    }

    /// The value a sample shows at: a stacked one's top, else its own.
    pub fn top(&self, stacks: &Stacks, si: usize, i: usize) -> f64 {
        match &stacks.hi[si] {
            Some(hi) => hi[i],
            None => self.series[si].ys[i],
        }
    }

    /// The data's extent, which zoom-out and pan never pass (mouse-ux §3.2's limits). A
    /// scatter's reaches half its span past the data on each side, so it zooms out to half.
    pub fn extent(&self) -> ((f64, f64), (f64, f64)) {
        let all = View::default();
        let x = if self.is_band() {
            (0.0, self.x.bands.len().max(1) as f64)
        } else {
            self.x_fit(&all, true)
        };
        let y = if self.y.kind == AxisKind::Band {
            (0.0, self.y.bands.len().max(1) as f64)
        } else {
            self.y_fit(&all, None, true, 5)
        };
        if self.is_scatter() || self.is_heatmap() {
            let pad = |(a, b): (f64, f64)| {
                let h = (b - a) / 2.0;
                (a - h, b + h)
            };
            return (pad(x), pad(y));
        }
        (x, y)
    }

    /// The x and y domains that fit what's visible under `view` (hidden series excluded, y fit
    /// to the x window on x-charts, zero included for bars and stacked areas). The view's own
    /// domain stands where it has one. Number domains are widened to nice numbers, but x on an
    /// x-chart, which runs edge to edge.
    pub fn fit(&self, view: &View) -> ((f64, f64), (f64, f64)) {
        self.fit_ticks(view, 5)
    }

    /// As [`ChartModel::fit`], y's number domain widened to nice numbers for about `count`
    /// ticks: the layout fits y to the ticks its plot's height has room for, so the top tick is
    /// the plot's top.
    pub fn fit_ticks(&self, view: &View, count: usize) -> ((f64, f64), (f64, f64)) {
        let x = match view.x {
            Some(x) => x,
            None if self.is_band() => (0.0, self.x.bands.len().max(1) as f64),
            None => self.x_fit(view, false),
        };
        let y = match view.y {
            Some(y) => y,
            None if self.y.kind == AxisKind::Band => (0.0, self.y.bands.len().max(1) as f64),
            None => {
                let window = (self.is_x_chart() || self.is_band()).then_some(x);
                self.y_fit(view, window, false, count)
            }
        };
        (x, y)
    }

    /// x's extent over the visible series (all of them with `all`).
    fn x_fit(&self, view: &View, all: bool) -> (f64, f64) {
        if let Some(d) = self.x.domain {
            return d;
        }
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for s in &self.series {
            if !all && !self.visible(view, &s.key) {
                continue;
            }
            // A bar over a number or a time is centred on its x and an interval wide: the fit
            // takes in half of one past each end, so the first and last are drawn whole.
            let half = if s.mark == MarkKind::Bar && s.ends.is_none() && !self.is_band() {
                s.interval / 2.0
            } else {
                0.0
            };
            for (i, &x) in s.xs.iter().enumerate() {
                if x.is_finite() && !s.ys[i].is_nan() {
                    lo = lo.min(x - half);
                    hi = hi.max(s.ends.as_ref().map_or(x, |e| e[i].max(x)) + half);
                }
            }
        }
        for r in &self.rules {
            if r.vertical && r.value.is_finite() {
                lo = lo.min(r.value);
                hi = hi.max(r.value);
            }
        }
        // A scatter's x is the data's own: the layout pads it by its dots' reach, not a tick.
        settle(lo, hi, self.x.kind)
    }

    /// y's fit over the visible series inside `window` on x (anywhere with None).
    fn y_fit(
        &self,
        view: &View,
        window: Option<(f64, f64)>,
        all: bool,
        count: usize,
    ) -> (f64, f64) {
        if let Some(d) = self.y.domain {
            return d;
        }
        let stacks = if all {
            self.stacks(&View::default())
        } else {
            self.stacks(view)
        };
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut whole_lo, mut whole_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let normalize = self.series.iter().any(|s| s.stack == Stack::Normalize);
        for (si, s) in self.series.iter().enumerate() {
            if !all && !self.visible(view, &s.key) {
                continue;
            }
            let (a, b) = match window {
                Some((a, b)) if s.continuous() || s.mark == MarkKind::Bar => {
                    let (a, b) = (a.min(b), a.max(b));
                    if self.is_band() {
                        // A band's samples are at its index; it shows from a to b in bands.
                        ((a - 0.999).floor(), b)
                    } else {
                        (a, b)
                    }
                }
                _ => (f64::NEG_INFINITY, f64::INFINITY),
            };
            let (start, end) = if self.is_band() || !s.continuous() {
                (0, s.xs.len())
            } else {
                // A line's samples just outside the window draw into it, so they count.
                let p = s.xs.partition_point(|&x| x < a);
                let q = s.xs.partition_point(|&x| x <= b);
                (p.saturating_sub(1), (q + 1).min(s.xs.len()))
            };
            for i in 0..s.xs.len() {
                let (u, v) = self.span(&stacks, si, i);
                if !(u.is_finite() && v.is_finite()) {
                    continue;
                }
                // A bar or segment below zero runs from its top down to its bottom.
                let (l, h) = (u.min(v), u.max(v));
                whole_lo = whole_lo.min(l);
                whole_hi = whole_hi.max(h);
                let x = s.xs[i];
                let inside = if self.is_band() || !s.continuous() {
                    x >= a && x <= b
                } else {
                    i >= start && i < end
                };
                if inside {
                    lo = lo.min(l);
                    hi = hi.max(h);
                }
            }
        }
        for r in &self.rules {
            if !r.vertical && r.value.is_finite() {
                lo = lo.min(r.value);
                hi = hi.max(r.value);
            }
        }
        for t in &self.texts {
            if t.y.is_finite() {
                lo = lo.min(t.y);
                hi = hi.max(t.y);
            }
        }
        if !lo.is_finite() {
            (lo, hi) = (whole_lo, whole_hi);
        }
        if normalize {
            return (0.0, 1.0);
        }
        let zero = match self.y.zero {
            Some(z) => z,
            None if self.fixed_y() => true,
            // Lines take in zero when the whole of what's visible is on one side of it and
            // nearer it than its own span: decided on the whole data, so a pan never flips it.
            None if self.series.iter().any(|s| s.continuous()) => {
                (whole_lo >= 0.0 && whole_lo <= whole_hi * 0.5)
                    || (whole_hi <= 0.0 && whole_hi >= whole_lo * 0.5)
            }
            None => false,
        };
        if zero && self.y.kind != AxisKind::Log {
            lo = lo.min(0.0);
            hi = hi.max(0.0);
        }
        let (lo, hi) = settle(lo, hi, self.y.kind);
        if self.is_scatter() {
            // As x: the data's own, padded by the dots' reach in the layout.
            return (lo, hi);
        }
        nice_domain(lo, hi, self.y.kind, &self.zone, count)
    }

    /// Labels a search can find: each series by display name (duplicates as `name ‹2›`) and each
    /// category, with where the cursor goes.
    pub fn labels(&self) -> Vec<(String, Cursor)> {
        let mut out = Vec::new();
        for key in self.keys() {
            let Some(si) = self.index_of(&key) else {
                continue;
            };
            let s = &self.series[si];
            // A series with no sample has nowhere for the cursor: a search can't find it.
            let Some((_, last)) = s.present_ends() else {
                continue;
            };
            let label = if key.nth == 0 {
                s.name.clone()
            } else {
                format!("{} ‹{}›", s.name, key.nth + 1)
            };
            let cursor = if self.is_band() {
                let band =
                    s.xs.iter()
                        .zip(s.ys.iter())
                        .find(|(_, y)| !y.is_nan())
                        .map_or(0, |(x, _)| *x as u32);
                Cursor::Band {
                    band,
                    series: Some(key.clone()),
                }
            } else {
                Cursor::Point {
                    series: key.clone(),
                    row: last as u32,
                }
            };
            out.push((label, cursor));
        }
        if self.is_band() {
            for (i, b) in self.x.bands.iter().enumerate() {
                out.push((
                    b.to_string(),
                    Cursor::Band {
                        band: i as u32,
                        series: None,
                    },
                ));
            }
        }
        out
    }

    /// The data as TSV, in the view's order, hidden series left out; the row a cursor is on.
    /// The pinned signature has no cursor: see [`ChartModel::tsv_at`].
    pub fn tsv(&self, view: &View) -> (String, Option<usize>) {
        self.tsv_at(view, None)
    }

    /// The data as TSV and the line (from 0, the header's) the cursor's sample is on: one row a
    /// distinct x and one column a visible series on x-charts; one row a category, every one in
    /// the axis's order, on band charts; on a heatmap, one row a y category and one column an x
    /// category, the cells' values between; on a scatter, one row a point.
    pub fn tsv_at(&self, view: &View, cursor: Option<&Cursor>) -> (String, Option<usize>) {
        let keys: Vec<SeriesKey> = self
            .keys()
            .into_iter()
            .filter(|k| self.visible(view, k))
            .collect();
        let xname = self
            .x
            .title
            .clone()
            .or_else(|| self.x.field.clone())
            .unwrap_or_else(|| "x".into());
        let mut text = String::new();
        let mut line = None;
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        if self.is_heatmap() && self.y.kind == AxisKind::Band {
            return self.tsv_cells(cursor);
        }
        if self.is_scatter() || self.is_heatmap() {
            let yname = self
                .y
                .title
                .clone()
                .or_else(|| self.y.field.clone())
                .unwrap_or_else(|| "y".into());
            text.push_str(&format!("{}\t{}\tseries\n", clean(&xname), clean(&yname)));
            let mut n = 1;
            for k in &keys {
                let Some(si) = self.index_of(k) else { continue };
                let s = &self.series[si];
                for i in 0..s.xs.len() {
                    if let Some(Cursor::Point { series, row }) = cursor
                        && series == k
                        && *row as usize == i
                    {
                        line = Some(n);
                    }
                    text.push_str(&format!(
                        "{}\t{}\t{}\n",
                        self.x_text(s.xs[i]),
                        tsv_number(s.ys[i]),
                        clean(&s.name)
                    ));
                    n += 1;
                }
            }
            return (text, line);
        }
        text.push_str(&clean(&xname));
        for k in &keys {
            let name = self
                .series_of(k)
                .map_or(k.name.to_string(), |s| s.name.clone());
            text.push('\t');
            if k.nth == 0 {
                text.push_str(&clean(&name));
            } else {
                text.push_str(&format!("{} ‹{}›", clean(&name), k.nth + 1));
            }
        }
        text.push('\n');
        // Every distinct x of the visible series, ascending, with each series' value there;
        // every category on a band chart, even one no visible series has.
        let mut rows: BTreeMap<u64, (f64, Vec<f64>)> = BTreeMap::new();
        let order = |x: f64| -> u64 {
            // Sorts floats in order as unsigned keys.
            let b = bits(x);
            if x < 0.0 { !b } else { b | (1 << 63) }
        };
        if self.is_band() {
            for b in 0..self.x.bands.len() {
                let x = b as f64;
                rows.insert(order(x), (x, vec![f64::NAN; keys.len()]));
            }
        }
        for (c, k) in keys.iter().enumerate() {
            let Some(si) = self.index_of(k) else { continue };
            let s = &self.series[si];
            for i in 0..s.xs.len() {
                let x = s.xs[i];
                if x.is_nan() {
                    continue;
                }
                let e = rows
                    .entry(order(x))
                    .or_insert_with(|| (x, vec![f64::NAN; keys.len()]));
                e.1[c] = if s.mark == MarkKind::Bar && !e.1[c].is_nan() {
                    e.1[c] + s.ys[i]
                } else {
                    s.ys[i]
                };
            }
        }
        let at_x = match cursor {
            Some(Cursor::Point { series, row }) => self
                .index_of(series)
                .and_then(|si| self.series[si].xs.get(*row as usize).copied()),
            Some(Cursor::Band { band, .. }) => Some(f64::from(*band)),
            _ => None,
        };
        for (n, (_, (x, vals))) in rows.iter().enumerate() {
            if at_x.is_some_and(|a| bits(a) == bits(*x)) {
                line = Some(n + 1);
            }
            text.push_str(&self.x_text(*x));
            for v in vals {
                text.push('\t');
                text.push_str(&tsv_number(*v));
            }
            text.push('\n');
        }
        (text, line)
    }

    /// A heatmap as TSV: a row a y category, a column an x category, each cell's value (blank
    /// where there's none); the cursor's cell's row.
    fn tsv_cells(&self, cursor: Option<&Cursor>) -> (String, Option<usize>) {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let (nx, ny) = (self.x.bands.len(), self.y.bands.len());
        let mut cells = vec![f64::NAN; nx * ny];
        for s in self.series.iter().filter(|s| s.mark == MarkKind::Rect) {
            for i in 0..s.xs.len().min(s.ys.len()) {
                let (x, y) = (s.xs[i], s.ys[i]);
                if x.is_nan() || y.is_nan() || x < 0.0 || y < 0.0 {
                    continue;
                }
                let (c, r) = (x as usize, y as usize);
                if c < nx && r < ny {
                    cells[r * nx + c] = s.sizes.as_ref().map_or(f64::NAN, |v| v[i]);
                }
            }
        }
        let corner = self
            .y
            .title
            .clone()
            .or_else(|| self.y.field.clone())
            .unwrap_or_else(|| "y".into());
        let mut text = clean(&corner);
        for b in &self.x.bands {
            text.push('\t');
            text.push_str(&clean(b));
        }
        text.push('\n');
        for (r, b) in self.y.bands.iter().enumerate() {
            text.push_str(&clean(b));
            for c in 0..nx {
                text.push('\t');
                text.push_str(&tsv_number(cells[r * nx + c]));
            }
            text.push('\n');
        }
        let line = match cursor {
            Some(Cursor::Cell { row, .. }) if (*row as usize) < ny => Some(*row as usize + 1),
            _ => None,
        };
        (text, line)
    }

    /// An x in TSV: a time in the zone to the second, a band's name, or a number in full.
    fn x_text(&self, x: f64) -> String {
        match self.x.kind {
            AxisKind::Time => scale::time::zoned(x, &self.zone).map_or_else(String::new, |z| {
                let s = z.strftime("%Y-%m-%dT%H:%M:%S").to_string();
                s.strip_suffix(":00").map_or(s.clone(), str::to_string)
            }),
            AxisKind::Band => self
                .x
                .bands
                .get(x as usize)
                .map_or_else(String::new, |b| b.replace(['\t', '\n'], " ")),
            _ => tsv_number(x),
        }
    }

    /// The newest x, for live follow.
    pub fn newest(&self) -> Option<f64> {
        if self.is_band() {
            return None;
        }
        self.series
            .iter()
            .filter_map(|s| s.present_ends().map(|(_, l)| s.xs[l]))
            .reduce(f64::max)
    }

    /// An x as the card's header writes it: a time in full at the series' interval, a band's
    /// name, or a number with the axis's format.
    pub fn x_label(&self, x: f64, step: f64) -> String {
        match self.x.kind {
            AxisKind::Time => match &self.x.format {
                Some(f) => format_with(x, f, &self.zone),
                None => format_time_full(x, step, &self.zone),
            },
            AxisKind::Band => self
                .x
                .bands
                .get(x as usize)
                .map_or_else(String::new, |b| b.to_string()),
            _ => match &self.x.format {
                Some(f) => format_with(x, f, &self.zone),
                None => {
                    let v = self.x_value(x);
                    // As quiet as a time's header: the axis's name, then the value.
                    match self.x.title.as_deref().filter(|t| !t.is_empty()) {
                        Some(t) => format!("{t} {v}"),
                        None => v,
                    }
                }
            },
        }
    }

    /// A number x alone, at the data's precision (or the axis's format), for a chip or a row
    /// that names the axis itself.
    pub fn x_value(&self, x: f64) -> String {
        match (&self.x.format, self.x.kind) {
            (_, AxisKind::Time | AxisKind::Band) => self.x_label(x, 0.0),
            (Some(f), _) => format_with(x, f, &self.zone),
            (None, _) => {
                let decimals = self.series.iter().map(|s| s.x_decimals).max().unwrap_or(0);
                scale::format_fixed(x, decimals)
            }
        }
    }

    /// A value as the card writes it: the axis's format, else in full.
    pub fn y_label(&self, y: f64) -> String {
        match &self.y.format {
            Some(f) => format_with(y, f, &self.zone),
            None if self.series.iter().any(|s| s.stack == Stack::Normalize) => {
                format!("{} %", format_value((y * 1000.0).round() / 10.0))
            }
            None => format_value(y),
        }
    }

    /// A value at a series' precision: the axis's format, a normalised stack's percentage, else
    /// in full with `decimals` decimals (a column of the series' values lines up).
    pub fn value_label(&self, y: f64, decimals: usize) -> String {
        if self.y.format.is_some() || self.series.iter().any(|s| s.stack == Stack::Normalize) {
            return self.y_label(y);
        }
        scale::format_fixed(y, decimals)
    }

    /// Whether every value the visible series show is a whole number (counts): the value axis
    /// then ticks at whole steps, never at 0.5. Not on a log axis, a normalised stack or an
    /// axis with its own format.
    pub fn whole_values(&self, view: &View) -> bool {
        if self.y.kind != AxisKind::Linear
            || self.y.format.is_some()
            || self.series.iter().any(|s| s.stack == Stack::Normalize)
        {
            return false;
        }
        let mut any = false;
        for s in self.series.iter().filter(|s| self.visible(view, &s.key)) {
            if s.mark == MarkKind::Rect {
                return false;
            }
            let more = s.y2s.iter().flat_map(|v| v.iter());
            for &v in s.ys.iter().chain(more) {
                if v.is_finite() {
                    if v.fract() != 0.0 {
                        return false;
                    }
                    any = true;
                }
            }
        }
        any
    }

    /// The smallest interval between samples of the visible series, for the zoom's limit.
    pub fn min_interval(&self, view: &View) -> f64 {
        self.series
            .iter()
            .filter(|s| self.visible(view, &s.key) && s.interval > 0.0)
            .map(|s| s.interval)
            .fold(f64::INFINITY, f64::min)
    }
}

/// Rules and notes are a chart's annotations: no series, no slot, no legend key.
fn annotation(k: MarkKind) -> bool {
    matches!(k, MarkKind::Rule | MarkKind::Text)
}

/// A domain that has two ends: the data's, or one around a lone value.
fn settle(lo: f64, hi: f64, kind: AxisKind) -> (f64, f64) {
    if !(lo.is_finite() && hi.is_finite()) {
        return match kind {
            AxisKind::Log => (1.0, 10.0),
            AxisKind::Time => (0.0, 3600.0),
            _ => (0.0, 1.0),
        };
    }
    if lo < hi {
        return (lo, hi);
    }
    match kind {
        AxisKind::Log => (lo / 10.0, lo * 10.0),
        AxisKind::Time => (lo - 1800.0, lo + 1800.0),
        _ => {
            let d = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
            (lo - d, lo + d)
        }
    }
}

/// A number domain widened to nice numbers for about five ticks; a time's to its calendar
/// unit; a log's to powers.
fn nice_domain(lo: f64, hi: f64, kind: AxisKind, zone: &TimeZone, count: usize) -> (f64, f64) {
    use crate::scale::Scale;
    let s = match kind {
        AxisKind::Log if lo > 0.0 => Scale::Log {
            domain: (lo, hi),
            range: (0.0, 1.0),
            base: 10.0,
        },
        AxisKind::Time => Scale::Time {
            domain: (lo, hi),
            range: (0.0, 1.0),
            zone: zone.clone(),
        },
        _ => Scale::Linear {
            domain: (lo, hi),
            range: (0.0, 1.0),
        },
    };
    let d = s.nice(count.max(2)).domain();
    if d.0.is_finite() && d.1.is_finite() && d.0 < d.1 {
        d
    } else {
        (lo, hi)
    }
}

/// A number as TSV writes it: in full without separators; nothing for a missing one.
fn tsv_number(v: f64) -> String {
    if v.is_nan() {
        return String::new();
    }
    format_value(v).replace(',', "").replace('\u{2212}', "-")
}

/// Slots in source order, one each distinct key of a series that isn't an annotation; a key
/// seen again (a line's band) takes its first slot.
fn give_slots(series: &mut [SeriesInfo]) {
    let mut slots: BTreeMap<SeriesKey, u8> = BTreeMap::new();
    let mut next: u16 = 0;
    for s in series.iter_mut() {
        let slot = *slots.entry(s.key.clone()).or_insert_with(|| {
            let n = next.min(255) as u8;
            next += 1;
            n
        });
        s.slot = (slot < 8).then_some(slot);
    }
}

/// Builds a model's series, axes, rules and notes, a layer at a time.
struct Builder {
    horizontal: bool,
    opt: ReadOptions,
    series: Vec<SeriesInfo>,
    rules: Vec<RuleMark>,
    texts: Vec<TextMark>,
    /// The independent axis: its kind once a layer decides it, its categories.
    xkind: Option<AxisKind>,
    xbands: Vec<Box<str>>,
    xband_of: BTreeMap<Box<str>, usize>,
    ykind: Option<AxisKind>,
    ybands: Vec<Box<str>>,
    yband_of: BTreeMap<Box<str>, usize>,
    xch: Option<Channel>,
    ych: Option<Channel>,
    /// How many keys each display name has had, across the layers in source order (ruling 3).
    names: BTreeMap<String, u32>,
}

impl Builder {
    fn new(horizontal: bool, opt: ReadOptions) -> Builder {
        Builder {
            horizontal,
            opt,
            series: Vec::new(),
            rules: Vec::new(),
            texts: Vec::new(),
            xkind: None,
            xbands: Vec::new(),
            xband_of: BTreeMap::new(),
            ykind: None,
            ybands: Vec::new(),
            yband_of: BTreeMap::new(),
            xch: None,
            ych: None,
            names: BTreeMap::new(),
        }
    }

    /// The key of the `k`th series named `name` in layer `li`: the next of its name across the
    /// chart (ruling 3), but a band and a line of one name, the `k`th of it in their layers,
    /// are one series, the line's band, so they share the key the first of them took.
    fn key_for(&mut self, name: &str, k: u32, band: bool, li: u16) -> SeriesKey {
        let pair = SeriesKey::new(name, k);
        let mut holders = self.series.iter().filter(|s| s.key == pair);
        if let (Some(h), None) = (holders.next(), holders.next())
            && (h.mark == MarkKind::Band) != band
            && h.layer != li
        {
            return pair;
        }
        let n = self.names.entry(name.to_string()).or_default();
        *n += 1;
        SeriesKey::new(name, *n - 1)
    }

    /// The axes, from the first layer that decided each and its channel.
    fn axes(&self, chart: &Chart) -> (AxisInfo, AxisInfo) {
        let x = self.xch.clone().unwrap_or_default();
        let y = self.ych.clone().unwrap_or_default();
        let one = self
            .series
            .iter()
            .map(|s| &s.key)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            <= 1;
        let points =
            self.series.iter().all(|s| s.mark == MarkKind::Point) && !self.series.is_empty();
        let xkind = self.xkind.unwrap_or(AxisKind::Linear);
        let ykind = self.ykind.unwrap_or(AxisKind::Linear);
        // Titles are shown where the spec gives them; a scatter's axes and a lone series' values
        // are named by their fields, which nothing else on screen names.
        // A heatmap's two axes are alike: each named by its field, as its colour legend is.
        let cells = self.series.iter().all(|s| s.mark == MarkKind::Rect) && !self.series.is_empty();
        let x_title = x.title.clone().or_else(|| {
            (points
                || cells
                || (xkind == AxisKind::Linear && self.series.iter().any(|s| s.ends.is_some())))
            .then(|| x.field.clone())
            .flatten()
        });
        let y_title = y.title.clone().or_else(|| {
            (points || (one && chart.layers.len() == 1))
                .then(|| y.field.clone())
                .flatten()
        });
        let axis = |c: &Channel, kind, bands: &Vec<Box<str>>, title| AxisInfo {
            kind,
            title,
            field: c.field.clone(),
            format: c.format.clone(),
            bands: bands.clone(),
            domain: c
                .domain
                .filter(|(a, b)| a.is_finite() && b.is_finite() && a < b),
            zero: c.zero,
        };
        (
            axis(&x, xkind, &self.xbands, x_title),
            axis(&y, ykind, &self.ybands, y_title),
        )
    }

    /// Reads a channel of `t`: numbers (times as seconds when the channel or the axis says
    /// time), strings, a constant, or nothing.
    fn read(&self, ch: Option<&Channel>, t: &Table, band: bool, time: bool) -> Result<Read> {
        let Some(ch) = ch else {
            return Ok(Read::Nothing);
        };
        if let Some(v) = &ch.value {
            return Ok(match v {
                ChannelValue::Num(n) => Read::Const(*n),
                ChannelValue::Str(s) => {
                    if time && let Some(t) = crate::data::time::parse_time(s, &self.opt) {
                        Read::Const(t)
                    } else {
                        Read::ConstStr(s.clone())
                    }
                }
            });
        }
        let Some(f) = &ch.field else {
            return Ok(Read::Nothing);
        };
        let col = t.column(f).ok_or_else(|| match ch.at {
            Some(at) => FigError::at(format!("no column \"{f}\" in the data"), at),
            None => FigError::new(format!("no column \"{f}\" in the data")),
        })?;
        if ch.kind == Some(ColumnKind::Number)
            && let Some((row, v)) = not_a_number(col, self.opt.thousands)
        {
            let msg = format!("\"{v}\" in \"{f}\" is not a number");
            let e = match ch.at {
                Some(at) => FigError::at(msg, at),
                None => FigError::new(msg),
            };
            return Err(e.note(format!("row {} of the data", row + 1)));
        }
        let wants_time = ch.kind == Some(ColumnKind::Time) || ch.scale == Some(ScaleKind::Time);
        let col = if wants_time && col.kind() != ColumnKind::Time {
            crate::data::time::as_time(col, ch.unit.as_deref(), &self.opt)?
        } else {
            col.clone()
        };
        let as_strings = band
            || ch.scale == Some(ScaleKind::Band)
            || matches!(ch.kind, Some(ColumnKind::Category | ColumnKind::Text));
        Ok(match (&col.values, as_strings) {
            (Values::Number(v) | Values::Time(v), false) => Read::Numbers(v.to_vec()),
            (Values::Time(v), true) => {
                // Times as categories: each at the precision their spacing needs.
                let mut sorted: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
                sorted.sort_by(f64::total_cmp);
                sorted.dedup();
                let step = median_interval(&sorted);
                Read::Strings(
                    v.iter()
                        .map(|x| {
                            (!x.is_nan())
                                .then(|| scale::format_time(*x, step, &self.opt.zone).into())
                        })
                        .collect(),
                )
            }
            (Values::Number(v), true) => Read::Strings(
                v.iter()
                    .map(|x| (!x.is_nan()).then(|| tsv_number(*x).into()))
                    .collect(),
            ),
            (Values::Category { .. } | Values::Text(_), _) => Read::Strings(
                (0..col.len())
                    .map(|r| col.str_at(r).map(Into::into))
                    .collect(),
            ),
        })
    }

    fn layer(
        &mut self,
        li: u16,
        layer: &crate::spec::Layer,
        t: &Table,
        ts: &[&Transform],
        chart: &Chart,
    ) -> Result<()> {
        let kind = layer.mark.kind;
        let (ich, vch) = if self.horizontal && kind == MarkKind::Bar {
            (layer.enc.y.as_ref(), layer.enc.x.as_ref())
        } else {
            (layer.enc.x.as_ref(), layer.enc.y.as_ref())
        };
        // The independent axis: decided by the first layer with a field on it.
        let decides = !annotation(kind) && ich.is_some_and(|c| c.field.is_some());
        if decides && self.xkind.is_none() {
            let c = ich.expect("decides needs a channel");
            let col = c.field.as_deref().and_then(|f| t.column(f));
            let k = match (c.scale, c.kind, col.map(Column::kind)) {
                // Bars across put their categories down the screen, whatever the field holds:
                // the crosshair and the brush run along x only.
                _ if self.horizontal && kind == MarkKind::Bar => AxisKind::Band,
                (Some(ScaleKind::Band), _, _) => AxisKind::Band,
                (Some(ScaleKind::Log), _, _) => AxisKind::Log,
                (Some(ScaleKind::Time), _, _) | (_, Some(ColumnKind::Time), _) => AxisKind::Time,
                (_, Some(ColumnKind::Category | ColumnKind::Text), _) => AxisKind::Band,
                (_, _, Some(ColumnKind::Time)) => AxisKind::Time,
                (_, _, Some(ColumnKind::Category | ColumnKind::Text)) => AxisKind::Band,
                _ if kind == MarkKind::Rect => AxisKind::Band,
                _ => AxisKind::Linear,
            };
            self.xkind = Some(k);
            self.xch = Some(c.clone());
        }
        let vdecides = !annotation(kind) && vch.is_some_and(|c| c.field.is_some());
        if vdecides && self.ykind.is_none() {
            let c = vch.expect("vdecides needs a channel");
            let col = c.field.as_deref().and_then(|f| t.column(f));
            let k = match (c.scale, col.map(Column::kind)) {
                (Some(ScaleKind::Log), _) => AxisKind::Log,
                (Some(ScaleKind::Band), _) => AxisKind::Band,
                (_, Some(ColumnKind::Category | ColumnKind::Text)) if kind == MarkKind::Rect => {
                    AxisKind::Band
                }
                _ if kind == MarkKind::Rect && c.scale != Some(ScaleKind::Linear) => AxisKind::Band,
                _ => AxisKind::Linear,
            };
            self.ykind = Some(k);
            self.ych = Some(c.clone());
        }
        let xband = self.xkind == Some(AxisKind::Band);
        let xtime = self.xkind == Some(AxisKind::Time);
        let yband = self.ykind == Some(AxisKind::Band) && kind == MarkKind::Rect;
        let xs = self.read(ich, t, xband, xtime)?;
        let ys = self.read(vch, t, yband, false)?;
        let n = t.rows;

        if kind == MarkKind::Rule {
            return self.rules_of(li, layer, &xs, &ys, n, ich.is_some());
        }
        let xvals = self.positions(&xs, n, true);
        let yvals = match kind {
            MarkKind::Rect if yband => self.positions(&ys, n, false),
            _ => numbers(&ys, n),
        };
        if kind == MarkKind::Text {
            let labels = self.read(layer.enc.label.as_ref(), t, true, false)?;
            for r in 0..n {
                let text = match &labels {
                    Read::Strings(v) => v[r].clone(),
                    Read::ConstStr(s) => Some(s.as_str().into()),
                    Read::Const(c) => Some(format_value(*c).into()),
                    Read::Numbers(v) => Some(format_value(v[r]).into()),
                    Read::Nothing => None,
                };
                if let Some(text) = text {
                    self.texts.push(TextMark {
                        x: xvals[r],
                        y: yvals[r],
                        text,
                        layer: li,
                    });
                }
            }
            return Ok(());
        }
        let y2 = match layer.enc.y2.as_ref() {
            Some(c) => Some(numbers(&self.read(Some(c), t, false, false)?, n)),
            None => None,
        };
        let size = match layer.enc.size.as_ref() {
            Some(c) if kind == MarkKind::Point => {
                Some(numbers(&self.read(Some(c), t, false, false)?, n))
            }
            _ => None,
        };
        let rect_value = match (kind, layer.enc.color.as_ref()) {
            (MarkKind::Rect, Some(c)) => Some(numbers(&self.read(Some(c), t, false, false)?, n)),
            _ => None,
        };
        // A histogram's bars run to their bin's end.
        let ends = ich.and_then(|c| c.field.as_deref()).and_then(|f| {
            ts.iter().rev().find_map(|tr| match tr {
                Transform::Bin { start, end, .. } if start == f => t.column(end),
                _ => None,
            })
        });
        let ends: Option<Vec<f64>> = ends.map(|c| (0..n).map(|r| c.f64_at(r)).collect());

        // The groups: by the colour field's values in order of first appearance (a category by
        // its code, so two series of one name stay two), else one.
        let groups = if kind == MarkKind::Rect {
            vec![(String::new(), (0..n as u32).collect::<Vec<u32>>())]
        } else {
            self.groups(layer.enc.color.as_ref(), t, layer, vch)?
        };
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        for (name, rows) in groups {
            let k = {
                let c = seen.entry(name.clone()).or_default();
                *c += 1;
                *c - 1
            };
            let key = self.key_for(&name, k, kind == MarkKind::Band, li);
            let style = series_style(chart, layer, &key);
            let mut order = rows;
            let sort = !xband && kind != MarkKind::Rect;
            if sort {
                order.sort_by(|&a, &b| xvals[a as usize].total_cmp(&xvals[b as usize]));
                // NaN sorts last with total_cmp only when positive; push every NaN x to the end.
                let (mut ok, nan): (Vec<u32>, Vec<u32>) =
                    order.iter().partition(|&&r| !xvals[r as usize].is_nan());
                ok.extend(nan);
                order = ok;
            }
            let pick = |v: &[f64]| -> Arc<[f64]> { order.iter().map(|&r| v[r as usize]).collect() };
            let (mut sx, mut sy) = (pick(&xvals), pick(&yvals));
            let mut srows: Arc<[u32]> = order.iter().copied().collect();
            if xband && kind == MarkKind::Bar {
                // One bar a band: rows of one band summed.
                let mut per: BTreeMap<usize, (f64, u32)> = BTreeMap::new();
                for (i, &x) in sx.iter().enumerate() {
                    if x.is_nan() {
                        continue;
                    }
                    let e = per.entry(x as usize).or_insert((f64::NAN, srows[i]));
                    if sy[i].is_finite() {
                        e.0 = if e.0.is_nan() { sy[i] } else { e.0 + sy[i] };
                    }
                }
                sx = per.keys().map(|&b| b as f64).collect();
                sy = per.values().map(|v| v.0).collect();
                srows = per.values().map(|v| v.1).collect();
            }
            let interval = median_interval(&sx);
            // A heatmap's values are its colour field's; its y is the rows' places.
            let decimals = match &rect_value {
                Some(v) => scale::data_decimals(&pick(v)),
                None => scale::data_decimals(&sy),
            };
            let x_decimals = if xband { 0 } else { scale::data_decimals(&sx) };
            let mark = kind;
            let pyramid = (sort && matches!(mark, MarkKind::Line | MarkKind::Area))
                .then(|| Pyramid::build(&sx, &sy));
            let name_shown = if chart.view.render_options {
                style.label.clone().unwrap_or_else(|| name.clone())
            } else {
                name.clone()
            };
            let sub = |v: &Option<Vec<f64>>| v.as_ref().map(|v| pick(v));
            self.series.push(SeriesInfo {
                key,
                slot: None,
                style,
                layer: li,
                name: name_shown,
                mark,
                xs: sx,
                ys: if mark == MarkKind::Rect {
                    sy.clone()
                } else {
                    sy
                },
                y2s: sub(&y2),
                sizes: sub(&size).or_else(|| sub(&rect_value)),
                ends: if xband { None } else { sub(&ends) },
                rows: srows,
                pyramid,
                interval,
                stack: if mark == MarkKind::Bar || mark == MarkKind::Area {
                    layer.mark.stack
                } else {
                    Stack::None
                },
                decimals,
                x_decimals,
            });
        }
        // A layer of one series doesn't stack.
        let members: Vec<usize> = (0..self.series.len())
            .filter(|&i| self.series[i].layer == li)
            .collect();
        if members.len() == 1 {
            self.series[members[0]].stack = Stack::None;
        }
        Ok(())
    }

    /// The series of a layer: (display name, rows) in order of first appearance.
    fn groups(
        &self,
        color: Option<&Channel>,
        t: &Table,
        layer: &crate::spec::Layer,
        vch: Option<&Channel>,
    ) -> Result<Vec<(String, Vec<u32>)>> {
        let lone = || {
            let name = layer
                .fig
                .label
                .clone()
                .or_else(|| vch.and_then(|c| c.title.clone().or_else(|| c.field.clone())))
                .unwrap_or_else(|| layer.mark.kind.name().to_string());
            Ok(vec![(name, (0..t.rows as u32).collect())])
        };
        let Some(field) = color.and_then(|c| c.field.as_deref()) else {
            return lone();
        };
        let Some(col) = t.column(field) else {
            return Err(match color.and_then(|c| c.at) {
                Some(at) => FigError::at(format!("no column \"{field}\" in the data"), at),
                None => FigError::new(format!("no column \"{field}\" in the data")),
            });
        };
        let mut out: Vec<(String, Vec<u32>)> = Vec::new();
        match &col.values {
            Values::Category { codes, dict } => {
                let mut at: BTreeMap<u32, usize> = BTreeMap::new();
                for (r, &c) in codes.iter().enumerate() {
                    if c == u32::MAX {
                        continue;
                    }
                    let i = *at.entry(c).or_insert_with(|| {
                        out.push((
                            dict.get(c as usize).map_or("", |s| &**s).to_string(),
                            Vec::new(),
                        ));
                        out.len() - 1
                    });
                    out[i].1.push(r as u32);
                }
            }
            _ => {
                let mut at: BTreeMap<String, usize> = BTreeMap::new();
                for r in 0..t.rows {
                    let name = match &col.values {
                        Values::Text(v) => v[r].to_string(),
                        _ => {
                            let v = col.f64_at(r);
                            if v.is_nan() {
                                continue;
                            }
                            format_value(v)
                        }
                    };
                    let i = *at.entry(name.clone()).or_insert_with(|| {
                        out.push((name, Vec::new()));
                        out.len() - 1
                    });
                    out[i].1.push(r as u32);
                }
            }
        }
        Ok(out)
    }

    /// Positions on a band axis (a category's index, added if new) or numbers.
    fn positions(&mut self, r: &Read, n: usize, x: bool) -> Vec<f64> {
        match r {
            Read::Strings(v) => v
                .iter()
                .map(|s| match s {
                    Some(s) => self.band(s, x) as f64,
                    None => f64::NAN,
                })
                .collect(),
            Read::ConstStr(s) => {
                let b = self.band(s, x) as f64;
                vec![b; n]
            }
            other => numbers(other, n),
        }
    }

    fn band(&mut self, s: &str, x: bool) -> usize {
        let (list, of) = if x {
            (&mut self.xbands, &mut self.xband_of)
        } else {
            (&mut self.ybands, &mut self.yband_of)
        };
        if let Some(&i) = of.get(s) {
            return i;
        }
        list.push(s.into());
        of.insert(s.into(), list.len() - 1);
        list.len() - 1
    }

    fn rules_of(
        &mut self,
        li: u16,
        layer: &crate::spec::Layer,
        xs: &Read,
        ys: &Read,
        n: usize,
        has_x: bool,
    ) -> Result<()> {
        let fig = &layer.fig;
        let mut push = |vertical: bool, value: f64| {
            if value.is_finite() {
                self.rules.push(RuleMark {
                    vertical,
                    value,
                    label: fig.label.clone(),
                    color: fig.color,
                    dash: fig.dash,
                    layer: li,
                });
            }
        };
        let mut values = |r: &Read, vertical: bool| match r {
            Read::Const(v) => push(vertical, *v),
            Read::Numbers(v) => {
                let mut seen: Vec<u64> = Vec::new();
                for &x in v.iter().take(n) {
                    if x.is_finite() && !seen.contains(&bits(x)) {
                        seen.push(bits(x));
                        push(vertical, x);
                    }
                }
            }
            _ => {}
        };
        match (ys, has_x) {
            (Read::Const(_) | Read::Numbers(_), _) if !matches!(xs, Read::Const(_)) => {
                values(ys, false)
            }
            _ => values(xs, true),
        }
        Ok(())
    }
}

/// A channel's numbers, NaN where there are none.
fn numbers(r: &Read, n: usize) -> Vec<f64> {
    match r {
        Read::Numbers(v) => v.clone(),
        Read::Const(c) => vec![*c; n],
        _ => vec![f64::NAN; n],
    }
}

/// The median gap between ascending x values (0 with fewer than two).
fn median_interval(xs: &[f64]) -> f64 {
    let mut d: Vec<f64> = xs
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| d.is_finite() && *d > 0.0)
        .collect();
    if d.is_empty() {
        return 0.0;
    }
    let mid = d.len() / 2;
    d.select_nth_unstable_by(mid, f64::total_cmp);
    d[mid]
}

/// A series' `fig` block: the chart's for its key, over its layer's.
fn series_style(chart: &Chart, layer: &crate::spec::Layer, key: &SeriesKey) -> FigStyle {
    let mut st = layer.fig.clone();
    // A layer's label names the layer's lone series; with a colour field it would name them all.
    if layer.enc.color.as_ref().is_some_and(|c| c.field.is_some()) {
        st.label = None;
    }
    if let Some((_, s)) = chart.series_fig.iter().find(|(k, _)| k == key) {
        let s = s.clone();
        st = FigStyle {
            color: s.color.or(st.color),
            size: s.size.or(st.size),
            override_focus: s.override_focus || st.override_focus,
            show_label: s.show_label.or(st.show_label),
            hidden: s.hidden || st.hidden,
            label: s.label.or(st.label),
            dash: s.dash || st.dash,
        };
    }
    st
}

/// The tables a chart draws from, read: the chart's data, then each layer's own, as
/// [`ChartModel::build`] takes them. Files are read and parsed here, by their format or name;
/// `source` is the buffer's text for data that's the buffer itself. A command's output and a
/// dashboard's named data aren't read here.
pub fn read_tables(
    chart: &Chart,
    source: Option<&str>,
    opt: &ReadOptions,
) -> Result<Vec<Arc<Table>>> {
    let mut out = vec![read_data(&chart.data, source, opt)?];
    for l in &chart.layers {
        if let Some(d) = &l.data {
            out.push(read_data(d, source, opt)?);
        }
    }
    Ok(out)
}

/// One data source, read.
pub fn read_data(d: &DataSpec, source: Option<&str>, opt: &ReadOptions) -> Result<Arc<Table>> {
    let opt = crate::spec::chart::with_hints(opt, &d.read);
    let parse = |text: &str, name: &str| -> Result<Table> {
        let format = d.format.or_else(|| crate::spec::format_of_name(name));
        match format {
            Some(DataFormat::Json) => {
                crate::data::rows::read_json(&crate::json::parse(text)?, &opt)
            }
            Some(DataFormat::Jsonl) => crate::data::rows::read_jsonl(text, &opt),
            Some(DataFormat::Tsv) => {
                let mut o = opt.clone();
                o.delimiter.get_or_insert(b'\t');
                crate::data::csv::read_csv(text, &o)
            }
            Some(DataFormat::Csv) => crate::data::csv::read_csv(text, &opt),
            None => {
                let first = text.trim_start().chars().next();
                if matches!(first, Some('{' | '[')) {
                    match crate::json::parse(text) {
                        Ok(n) => crate::data::rows::read_json(&n, &opt),
                        Err(_) => crate::data::rows::read_jsonl(text, &opt),
                    }
                } else {
                    crate::data::csv::read_csv(text, &opt)
                }
            }
        }
    };
    match &d.source {
        DataRef::Inline(t) => Ok(Arc::clone(t)),
        DataRef::File(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| FigError::new(format!("{}: {e}", p.display())))?;
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            parse(&text, name).map(Arc::new)
        }
        DataRef::Source => {
            let text = source.ok_or_else(|| FigError::new("no buffer to read the data from"))?;
            parse(text, "").map(Arc::new)
        }
        DataRef::Run { .. } | DataRef::Stream { .. } => Err(FigError::new(
            "run and stream sources arrive in a later build",
        )),
        DataRef::Named(n) => Err(FigError::new(format!(
            "\"{n}\" names a dashboard's data, which a chart alone can't read"
        ))),
        DataRef::Borrow { name, .. } => Err(FigError::new(format!(
            "\"#{name}\" borrows a dashboard's data, which a chart alone can't read"
        ))),
    }
}

/// The first cell of a column read as text that isn't a number, by its row, when a spec says
/// the column is numbers: what makes the column text.
pub fn not_a_number(col: &Column, thousands: Option<char>) -> Option<(usize, String)> {
    if !matches!(col.values, Values::Category { .. } | Values::Text(_)) {
        return None;
    }
    (0..col.len()).find_map(|r| {
        let s = col.str_at(r)?;
        let bad = !crate::data::infer::is_missing(s)
            && crate::data::infer::parse_number(s, thousands).is_none();
        bad.then(|| (r, s.to_string()))
    })
}

/// Whether a path is a file a chart could read as data, by its name.
pub fn is_data_file(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .and_then(crate::spec::format_of_name)
        .is_some()
}

/// The base layer: chrome and marks, with regions.
pub fn base(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, ChartLayout, HitIndex) {
    base_beside(model, view, size, size.0, scale, theme, m)
}

/// [`base`] in part of an area `area_w` wide (beside the detail panel): its paddings are the
/// whole area's, so the title and the plot stay where they were as the panel opens.
pub fn base_beside(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    area_w: f32,
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, ChartLayout, HitIndex) {
    base_with(model, view, size, area_w, None, scale, theme, m)
}

/// [`base`] with the paddings given ([`layout::Insets`]) rather than the area's own: a
/// dashboard's panel, whose frame and title are the panel's.
pub fn base_inset(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    insets: layout::Insets,
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, ChartLayout, HitIndex) {
    base_with(model, view, size, size.0, Some(insets), scale, theme, m)
}

#[allow(clippy::too_many_arguments)]
fn base_with(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    area_w: f32,
    insets: Option<layout::Insets>,
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, ChartLayout, HitIndex) {
    let mut layout = layout::lay_out(model, view, size, area_w, insets, scale, m);
    layout.bars = marks::bars(model, view, &layout);
    layout.totals = marks::bar_totals(model, view, &layout, theme, m)
        .into_iter()
        .map(|(b, _, r)| (b, r))
        .collect();
    let mut scene = Scene::new(size.0, size.1, scale);
    let mut hits = HitIndex::new();
    layout::regions_under(&layout, &mut scene);
    layout::chrome(model, view, &layout, theme, &mut scene, m);
    marks::draw(model, view, &layout, theme, &mut scene, &mut hits, m);
    layout::regions(model, &layout, &mut scene);
    (scene, layout, hits)
}
