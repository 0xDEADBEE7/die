//! What the pointer is on: the nearest x on an x-chart (the crosshair's snap), a bar's band and
//! segment, a heatmap's cell, the nearest point on a scatter; and the series nearest the pointer,
//! which hover brings forward (contract-p2 §0).
//!
//! The continuous marks are found through the [`HitIndex`] (a binary search over each visible
//! series' raw samples, a grid hash for points), never through the scene's rectangles, so a
//! hover over a million samples costs a few searches (ruling 5).

use super::ChartModel;
use super::layout::ChartLayout;
use crate::algo::HitIndex;
use crate::data::SeriesKey;
use crate::interact::consts::{HIT_DENSE, HIT_POINT, NEAREST_SERIES};
use crate::interact::{Cursor, View};
use crate::spec::MarkKind;

/// How far outside the plot a pointer still counts as on it: the plot's edge pixel.
const EDGE: f32 = 1.0;

/// Whether (x, y) is on the plot, its edge pixel included.
pub(crate) fn on_plot(layout: &ChartLayout, x: f32, y: f32) -> bool {
    let p = layout.plot;
    x >= p.x - EDGE && x <= p.right() + EDGE && y >= p.y - EDGE && y <= p.bottom() + EDGE
}

/// What the pointer at (x, y) is on, as a cursor: on x-charts the nearest x of the visible
/// series (and the nearest series within NEAREST_SERIES for emphasis), on scatter the nearest
/// point within HIT_POINT (HIT_DENSE when dense), a bar's band and segment, a heatmap cell.
/// None off the plot.
///
/// On an x-chart the cursor is the sample nearest the pointer's x across the visible series,
/// on the series nearest the pointer in y there with no limit (a click puts the cursor on it,
/// mouse-ux §4.1); in a stacked area, the band whose stack holds the pointer. A row is a
/// sample's index in its [`SeriesInfo`](super::SeriesInfo)'s samples.
pub fn hover(
    model: &ChartModel,
    layout: &ChartLayout,
    hits: &HitIndex,
    view: &View,
    x: f32,
    y: f32,
) -> Option<Cursor> {
    if !on_plot(layout, x, y) {
        return None;
    }
    if model.is_heatmap() {
        let col = layout.x.band_at(x)?;
        let row = layout.y.band_at(y)?;
        return Some(Cursor::Cell {
            row: row as u32,
            col: col as u32,
        });
    }
    if model.is_band() {
        let band = layout.band_at(x, y)?;
        let series = segment_at(model, layout, view, band as u32, x, y);
        return Some(Cursor::Band {
            band: band as u32,
            series,
        });
    }
    if model.is_scatter() {
        let reach = if dense(model, layout, view) {
            HIT_DENSE
        } else {
            HIT_POINT
        };
        let (series, row) = hits.nearest_point(x, y, reach)?;
        return Some(Cursor::Point { series, row });
    }
    let at = snap(model, layout, hits, view, x, y)?;
    // The values run across for bars across.
    let along = if layout.horizontal { x } else { y };
    let (key, row) = nearest_y(model, layout, &at, along, None)?;
    Some(Cursor::Point { series: key, row })
}

/// The series nearest the pointer at a hover, for emphasis; None past NEAREST_SERIES.
///
/// On an x-chart, the series whose sample at the snapped x is nearest the pointer in y, within
/// [`NEAREST_SERIES`]; in a stacked area, the band that holds the pointer. On bars, the segment
/// under the pointer (above a stack, its top one); on a scatter, the hovered point's series.
pub fn nearest_series(
    model: &ChartModel,
    layout: &ChartLayout,
    hits: &HitIndex,
    view: &View,
    x: f32,
    y: f32,
) -> Option<SeriesKey> {
    if !on_plot(layout, x, y) || model.is_heatmap() {
        return None;
    }
    let visible = model
        .keys()
        .into_iter()
        .filter(|k| model.visible(view, k))
        .count();
    if visible < 2 {
        return None;
    }
    match hover(model, layout, hits, view, x, y)? {
        // A bar's series comes forward only with the pointer on one of its segments.
        Cursor::Band { band, series } => series.filter(|k| {
            layout.bars.iter().any(|b| {
                b.band == band
                    && &model.series[b.series].key == k
                    && b.rect.inflate(1.0).contains(x, y)
            }) || !layout.bars.iter().any(|b| b.band == band)
        }),
        Cursor::Point { series, .. } if model.is_scatter() => Some(series),
        Cursor::Point { .. } => {
            let at = snap(model, layout, hits, view, x, y)?;
            let along = if layout.horizontal { x } else { y };
            nearest_y(model, layout, &at, along, Some(NEAREST_SERIES)).map(|(k, _)| k)
        }
        _ => None,
    }
}

/// A scatter is dense when its visible points pass the plot's area over 24².
pub(crate) fn dense(model: &ChartModel, layout: &ChartLayout, view: &View) -> bool {
    let n: usize = model
        .series
        .iter()
        .filter(|s| s.mark == MarkKind::Point && model.visible(view, &s.key))
        .map(|s| s.xs.len())
        .sum();
    n as f32 > layout.plot.w * layout.plot.h / (24.0 * 24.0)
}

/// Each visible series' sample nearest one x: the crosshair's column.
pub(crate) struct Column {
    /// The x the crosshair snapped to, in data.
    pub x: f64,
    /// Each visible series (by its index in the model) and its sample nearest `x`, if it has
    /// one within twice its median interval.
    pub samples: Vec<(usize, Option<usize>)>,
}

/// The visible continuous series, one per key: the series the cursor walks.
pub(crate) fn walked(model: &ChartModel, view: &View) -> Vec<usize> {
    model
        .keys()
        .into_iter()
        .filter(|k| model.visible(view, k))
        .filter_map(|k| model.index_of(&k))
        .filter(|&si| {
            !matches!(
                model.series[si].mark,
                MarkKind::Rect | MarkKind::Rule | MarkKind::Text
            )
        })
        .collect()
}

/// The sample of series `si` nearest `x`, within twice its interval, through the hit index or,
/// where it hasn't the series, by a binary search.
pub(crate) fn sample_near(
    model: &ChartModel,
    hits: Option<&HitIndex>,
    si: usize,
    x: f64,
) -> Option<usize> {
    let s = &model.series[si];
    let i = match hits.and_then(|h| h.nearest_x(&s.key, x)) {
        Some(r) => r as usize,
        None => nearest_present(&s.xs, &s.ys, x)?,
    };
    let reach = if s.interval > 0.0 {
        2.0 * s.interval
    } else {
        f64::INFINITY
    };
    // A missing sample nearer x than the nearest present one: a gap there, which says "—".
    let len = s.xs.len().min(s.ys.len());
    let at = s.xs[..len].partition_point(|&v| v < x);
    let d = (s.xs[i] - x).abs();
    let gap = [at.wrapping_sub(1), at]
        .into_iter()
        .filter(|&j| j < len)
        .any(|j| s.ys[j].is_nan() && (s.xs[j] - x).abs() < d);
    if gap {
        return None;
    }
    ((s.xs[i] - x).abs() <= reach * (1.0 + 1e-9)).then_some(i)
}

/// The present sample nearest `x` in x-sorted samples, the earlier on a tie.
pub(crate) fn nearest_present(xs: &[f64], ys: &[f64], x: f64) -> Option<usize> {
    if x.is_nan() {
        return None;
    }
    let len = xs.len().min(ys.len());
    let at = xs[..len].partition_point(|&v| v < x);
    let mut best: Option<(f64, usize)> = None;
    let mut consider = |i: usize| {
        let d = (xs[i] - x).abs();
        if best.is_none_or(|(bd, bi)| d < bd || (d == bd && i < bi)) {
            best = Some((d, i));
        }
    };
    if let Some(i) = (0..at).rev().find(|&i| !ys[i].is_nan() && !xs[i].is_nan()) {
        consider(i);
    }
    if let Some(i) = (at..len).find(|&i| !ys[i].is_nan() && !xs[i].is_nan()) {
        consider(i);
    }
    best.map(|(_, i)| i)
}

/// The column the crosshair snaps to at the pointer's x: the sample nearest it across the
/// visible series, in screen px.
pub(crate) fn snap(
    model: &ChartModel,
    layout: &ChartLayout,
    hits: &HitIndex,
    view: &View,
    x: f32,
    y: f32,
) -> Option<Column> {
    let want = layout.data_x(x, y);
    let series = walked(model, view);
    let mut best: Option<(f32, f64)> = None;
    for &si in &series {
        let s = &model.series[si];
        let Some(i) = hits
            .nearest_x(&s.key, want)
            .map(|r| r as usize)
            .or_else(|| nearest_present(&s.xs, &s.ys, want))
        else {
            continue;
        };
        let px = layout.ix(s.xs[i]);
        let d = if layout.horizontal {
            (px - y).abs()
        } else {
            (px - x).abs()
        };
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, s.xs[i]));
        }
    }
    let (_, sx) = best?;
    Some(column_at(model, Some(hits), view, sx))
}

/// Each visible series' sample nearest `x`.
pub(crate) fn column_at(
    model: &ChartModel,
    hits: Option<&HitIndex>,
    view: &View,
    x: f64,
) -> Column {
    let samples = walked(model, view)
        .into_iter()
        .map(|si| (si, sample_near(model, hits, si, x)))
        .collect();
    Column { x, samples }
}

/// Of a column's series, the one nearest the pointer's y (within `reach` if given): in a
/// stacked area or bar, the one whose stack holds it.
fn nearest_y(
    model: &ChartModel,
    layout: &ChartLayout,
    col: &Column,
    y: f32,
    reach: Option<f32>,
) -> Option<(SeriesKey, u32)> {
    let mut best: Option<(f32, usize, usize)> = None;
    for &(si, i) in &col.samples {
        let Some(i) = i else {
            continue;
        };
        let s = &model.series[si];
        // Only series sampled at the snapped x itself compete, so the crosshair stays put.
        if (s.xs[i] - col.x).abs() > s.interval.max(0.0) * 0.5 {
            continue;
        }
        if let (Some(lo), Some(hi)) = (&layout.stacks.lo[si], &layout.stacks.hi[si]) {
            let (a, b) = (layout.vy(lo[i]), layout.vy(hi[i]));
            if y >= a.min(b) && y <= a.max(b) {
                return Some((s.key.clone(), i as u32));
            }
        }
        let py = layout.vy(model.top(&layout.stacks, si, i));
        let d = (py - y).abs();
        if !d.is_finite() {
            continue;
        }
        if best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, si, i));
        }
    }
    let (d, si, i) = best?;
    if reach.is_some_and(|r| d > r) {
        return None;
    }
    Some((model.series[si].key.clone(), i as u32))
}

/// The segment of a band's bars under the pointer: in a stack, the one whose extent holds the
/// pointer, else the top one; among grouped bars, the one under the pointer, else the nearest.
fn segment_at(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    band: u32,
    x: f32,
    y: f32,
) -> Option<SeriesKey> {
    let bars: Vec<&super::marks::BarGeom> = layout.bars.iter().filter(|b| b.band == band).collect();
    if bars.is_empty() {
        // Lines or points over bands: the one nearest in y.
        let col = column_at(model, None, view, f64::from(band));
        return nearest_y(
            model,
            layout,
            &col,
            if layout.horizontal { x } else { y },
            None,
        )
        .map(|(k, _)| k);
    }
    if let Some(b) = bars.iter().find(|b| b.rect.inflate(1.0).contains(x, y)) {
        return Some(model.series[b.series].key.clone());
    }
    // Past a stack's end, or between grouped bars: the nearest along the band, then the end.
    let along = |b: &&super::marks::BarGeom| {
        let r = b.rect;
        if layout.horizontal {
            dist(y, r.y, r.bottom())
        } else {
            dist(x, r.x, r.right())
        }
    };
    let across = |b: &&super::marks::BarGeom| {
        let r = b.rect;
        if layout.horizontal {
            dist(x, r.x, r.right())
        } else {
            dist(y, r.y, r.bottom())
        }
    };
    let near = bars.iter().map(along).fold(f32::INFINITY, f32::min);
    bars.iter()
        .filter(|b| along(b) <= near + 0.5)
        .min_by(|a, b| across(a).total_cmp(&across(b)))
        .map(|b| model.series[b.series].key.clone())
}

fn dist(v: f32, a: f32, b: f32) -> f32 {
    if v < a {
        a - v
    } else if v > b {
        v - b
    } else {
        0.0
    }
}
