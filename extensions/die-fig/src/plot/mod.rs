//! A chart drawn: its model resolved against the data, its anatomy laid out, its marks and
//! chrome into a base scene, and the overlay that follows the pointer and the cursor.
//!
//! - [`layout`]: title, legend, axes and the plot's rectangle;
//! - [`marks`]: line, area, bar, point, rule, band, rect and text marks into the scene;
//! - [`overlay`]: crosshair, dots, card, chips, brush, reference and the emphasis wash;
//! - [`nav`]: the cursor's motions and the view's changes, data-aware, with limits;
//! - [`hover`]: what the pointer is on.
//!
//! **A stand-in, in fig/surface's branch only.** contract-p2 §2's signatures, with bodies just
//! big enough for the surface's harness tests to run before fig/plot lands: one layer, its
//! series split by colour, plain lines, a plain card. fig/plot's `plot/` replaces these files
//! whole at the landing; nothing outside `plot/` reads what isn't pinned, except
//! `SeriesInfo::{xs, ys}`, which the surface reads a cursor's values by.

pub mod hover;
pub mod layout;
pub mod marks;
pub mod nav;
pub mod overlay;

pub use layout::ChartLayout;
pub use overlay::{OverlayState, overlay};

use crate::algo::HitIndex;
use crate::data::transform::apply;
use crate::data::{ColumnKind, SeriesKey, Table, Values};
use crate::error::{FigError, Result};
use crate::interact::{Cursor, View};
use crate::scene::{Hit, Item, Measure, PathData, Rect, Scene, Stroke, TextSpec};
use crate::spec::{Chart, DataRef, MarkKind};
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
    kind: XKind,
    bands: Vec<Box<str>>,
    zone: TimeZone,
    mark: MarkKind,
}

/// What the stand-in's x is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XKind {
    Number,
    Time,
    Band,
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
    pub fn build(chart: Chart, tables: Vec<Arc<Table>>, zone: &TimeZone) -> Result<ChartModel> {
        let raw = match (&chart.data.source, tables.first()) {
            (_, Some(t)) => t.clone(),
            (DataRef::Inline(t), None) => t.clone(),
            _ => return Err(FigError::new("no data for the chart")),
        };
        let table = apply(&raw, &chart.transform)?;
        let layer = chart
            .layers
            .first()
            .ok_or_else(|| FigError::new("nothing to draw: no layers"))?;
        let field = |c: &Option<crate::spec::Channel>| c.as_ref().and_then(|c| c.field.clone());
        let x = field(&layer.enc.x).ok_or_else(|| FigError::new("no x"))?;
        let y = field(&layer.enc.y).ok_or_else(|| FigError::new("no y"))?;
        let xc = table
            .column(&x)
            .ok_or_else(|| FigError::new(format!("no column \"{x}\"")))?;
        let yc = table
            .column(&y)
            .ok_or_else(|| FigError::new(format!("no column \"{y}\"")))?;
        let (kind, bands) = match &xc.values {
            Values::Time(_) => (XKind::Time, vec![]),
            Values::Category { dict, .. } => (XKind::Band, dict.to_vec()),
            _ => (XKind::Number, vec![]),
        };
        let xv = |i: usize| match &xc.values {
            Values::Category { codes, .. } => match codes[i] {
                u32::MAX => f64::NAN,
                c => f64::from(c),
            },
            _ => xc.f64_at(i),
        };
        let color = field(&layer.enc.color).and_then(|c| table.column(&c).cloned());
        let mut groups: Vec<(String, Vec<(f64, f64)>)> = vec![];
        for i in 0..table.rows {
            let name = match &color {
                Some(c) if c.kind() != ColumnKind::Number => c.str_at(i).unwrap_or("").to_string(),
                _ => y.clone(),
            };
            let at = match groups.iter().position(|g| g.0 == name) {
                Some(at) => at,
                None => {
                    groups.push((name, vec![]));
                    groups.len() - 1
                }
            };
            groups[at].1.push((xv(i), yc.f64_at(i)));
        }
        let mut series = vec![];
        for (n, (name, mut pts)) in groups.into_iter().enumerate() {
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let nth = series
                .iter()
                .filter(|s: &&SeriesInfo| &*s.key.name == name.as_str())
                .count() as u32;
            series.push(SeriesInfo {
                key: SeriesKey::new(&name, nth),
                slot: (n < 8).then_some(n as u8),
                style: FigStyle::default(),
                layer: 0,
                xs: pts.iter().map(|p| p.0).collect(),
                ys: pts.iter().map(|p| p.1).collect(),
            });
        }
        if series.is_empty() {
            return Err(FigError::new("nothing to draw: no rows"));
        }
        Ok(ChartModel {
            mark: layer.mark.kind,
            chart,
            tables,
            series,
            kind,
            bands,
            zone: zone.clone(),
        })
    }

    /// An x-chart: x is time or an ordered number and y fits the visible data (lines, areas,
    /// bars over time). Scroll pans x in both directions there (mouse-ux §3.2).
    pub fn is_x_chart(&self) -> bool {
        self.kind != XKind::Band && self.mark != MarkKind::Point
    }

    /// A band chart: x (or y when horizontal) is categories.
    pub fn is_band(&self) -> bool {
        self.kind == XKind::Band
    }

    /// The data's extent, which zoom-out and pan never pass (mouse-ux §3.2's limits).
    pub fn extent(&self) -> ((f64, f64), (f64, f64)) {
        let all = |f: &dyn Fn(&SeriesInfo) -> &[f64]| {
            let vals = self.series.iter().flat_map(|s| f(s).iter().copied());
            span(vals.filter(|v| v.is_finite()))
        };
        let x = match self.kind {
            XKind::Band => (0.0, self.bands.len().max(1) as f64),
            _ => all(&|s| &s.xs),
        };
        (x, with_zero(all(&|s| &s.ys), self.mark))
    }

    /// The x and y domains that fit what's visible under `view` (hidden series excluded, y fit to
    /// the x window on x-charts, zero included for bars and stacked areas).
    pub fn fit(&self, view: &View) -> ((f64, f64), (f64, f64)) {
        let (ex, ey) = self.extent();
        let shown: Vec<&SeriesInfo> = self.series.iter().filter(|s| view.shows(&s.key)).collect();
        let x = match self.kind {
            XKind::Band => ex,
            _ => {
                let vals = shown.iter().flat_map(|s| s.xs.iter().copied());
                let x = span(vals.filter(|v| v.is_finite()));
                if shown.is_empty() { ex } else { x }
            }
        };
        let window = view.x.unwrap_or(x);
        let vals = shown.iter().flat_map(|s| {
            s.xs.iter()
                .zip(s.ys.iter())
                .filter(|(x, _)| **x >= window.0 && **x <= window.1)
                .map(|(_, y)| *y)
        });
        let y = span(vals.filter(|v| v.is_finite()));
        let y = if shown.is_empty() {
            ey
        } else {
            with_zero(y, self.mark)
        };
        (x, y)
    }

    /// Labels a search can find: each series by display name (duplicates as `name ‹2›`) and each
    /// category, with where the cursor goes.
    pub fn labels(&self) -> Vec<(String, Cursor)> {
        let mut out: Vec<(String, Cursor)> = self
            .series
            .iter()
            .map(|s| {
                let name = match s.key.nth {
                    0 => s.key.name.to_string(),
                    n => format!("{} ‹{}›", s.key.name, n + 1),
                };
                let cursor = Cursor::Point {
                    series: s.key.clone(),
                    row: 0,
                };
                (name, cursor)
            })
            .collect();
        for (i, b) in self.bands.iter().enumerate() {
            let cursor = Cursor::Band {
                band: i as u32,
                series: None,
            };
            out.push((b.to_string(), cursor));
        }
        out
    }

    /// The data as TSV, in the view's order, hidden series left out; the row a cursor is on.
    pub fn tsv(&self, view: &View) -> (String, Option<usize>) {
        let mut out = String::from("series\tx\ty\n");
        for s in self.series.iter().filter(|s| view.shows(&s.key)) {
            for (x, y) in s.xs.iter().zip(s.ys.iter()) {
                out.push_str(&format!("{}\t{x}\t{y}\n", s.key.name));
            }
        }
        (out, None)
    }

    /// The newest x, for live follow.
    pub fn newest(&self) -> Option<f64> {
        let vals = self.series.iter().flat_map(|s| s.xs.iter().copied());
        vals.filter(|v| v.is_finite()).reduce(f64::max)
    }

    /// The series keys in legend order.
    pub fn keys(&self) -> Vec<SeriesKey> {
        self.series.iter().map(|s| s.key.clone()).collect()
    }

    /// The zone times are shown in.
    pub(crate) fn zone(&self) -> &TimeZone {
        &self.zone
    }

    /// Whether x is time.
    pub(crate) fn time_x(&self) -> bool {
        self.kind == XKind::Time
    }
}

/// The least and most of some values, (0, 1) for none and widened for one.
fn span(vals: impl Iterator<Item = f64>) -> (f64, f64) {
    let (lo, hi) = vals.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    match (lo.is_finite(), lo == hi) {
        (false, _) => (0.0, 1.0),
        (true, true) => (lo - 0.5, hi + 0.5),
        (true, false) => (lo, hi),
    }
}

/// A value domain with zero in it for bars.
fn with_zero(d: (f64, f64), mark: MarkKind) -> (f64, f64) {
    match mark {
        MarkKind::Bar | MarkKind::Area => (d.0.min(0.0), d.1.max(0.0)),
        _ => d,
    }
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
    let layout = layout::lay_out(model, view, size);
    let mut scene = Scene::new(size.0, size.1, scale);
    let mut hits = HitIndex::new();
    scene.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
    scene.region(layout.plot, Hit::Plot(0));
    scene.region(layout.x_axis, Hit::AxisX(0));
    scene.region(layout.y_axis, Hit::AxisY(0));
    if let Some(t) = &model.chart.title {
        let spec = TextSpec::new(15.0);
        scene.push(text(
            t,
            layout.title.x,
            layout.title.bottom() - 4.0,
            spec,
            theme,
        ));
        scene.region(layout.title, Hit::PanelTitle(0));
    }
    scene.push(Item::PushClip(layout.plot));
    for s in model.series.iter().filter(|s| view.shows(&s.key)) {
        let pts: Vec<(f32, f32)> = (s.xs.iter().zip(s.ys.iter()))
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .map(|(x, y)| (layout.x.map(*x), layout.y.map(*y)))
            .collect();
        let color = s.slot.map_or(theme.other, |i| theme.series[usize::from(i)]);
        scene.push(Item::Path {
            path: PathData::polyline(&pts),
            fill: None,
            stroke: Some(Stroke::solid(2.0, color, 1.0)),
        });
        hits.add_series(s.key.clone(), s.xs.clone(), s.ys.clone());
    }
    scene.push(Item::PopClip);
    for (key, r) in &layout.legend {
        let name = &model
            .series
            .iter()
            .find(|s| &s.key == key)
            .unwrap()
            .key
            .name;
        let spec = TextSpec::new(11.0);
        scene.push(text(name, r.x + 14.0, r.bottom() - 4.0, spec, theme));
        scene.region(*r, Hit::LegendKey(key.clone()));
    }
    let _ = m.width("", TextSpec::new(11.0));
    (scene, layout, hits)
}

/// A line of text in the theme's text colour.
fn text(s: &str, x: f32, y: f32, spec: TextSpec, theme: &Theme) -> Item {
    Item::Text {
        text: s.into(),
        x,
        y,
        spec,
        anchor: crate::scene::Anchor::Start,
        color: theme.text,
        alpha: 1.0,
        max_width: None,
    }
}
