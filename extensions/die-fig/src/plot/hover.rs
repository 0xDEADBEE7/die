//! What the pointer is on. The stand-in's (see [`super`]): the nearest x of the visible series,
//! the series nearest in y there.

use super::{ChartLayout, ChartModel};
use crate::algo::HitIndex;
use crate::data::SeriesKey;
use crate::interact::consts::NEAREST_SERIES;
use crate::interact::{Cursor, View};

/// Each visible series' sample nearest the pointer's x: its key, row, and its distance from the
/// pointer across and up, in px.
fn nearest(
    model: &ChartModel,
    layout: &ChartLayout,
    hits: &HitIndex,
    view: &View,
    x: f32,
    y: f32,
) -> Vec<(SeriesKey, u32, f32, f32)> {
    if !layout.plot.contains(x, y) {
        return vec![];
    }
    let at = layout.x.invert(x);
    let at = if model.is_band() { at.floor() } else { at };
    let mut out = vec![];
    for s in model.series.iter().filter(|s| view.shows(&s.key)) {
        let Some(row) = hits.nearest_x(&s.key, at) else {
            continue;
        };
        let i = row as usize;
        let px = layout.x.map(s.xs[i]);
        let py = layout.y.map(s.ys[i]);
        out.push((s.key.clone(), row, (px - x).abs(), (py - y).abs()));
    }
    out
}

/// What the pointer at (x, y) is on, as a cursor: on x-charts the nearest x of the visible
/// series (and the nearest series within NEAREST_SERIES for emphasis), on scatter the nearest
/// point within HIT_POINT (HIT_DENSE when dense), a bar's band and segment, a heatmap cell.
/// None off the plot.
pub fn hover(
    model: &ChartModel,
    layout: &ChartLayout,
    hits: &HitIndex,
    view: &View,
    x: f32,
    y: f32,
) -> Option<Cursor> {
    let all = nearest(model, layout, hits, view, x, y);
    let dx = all.iter().map(|n| n.2).fold(f32::INFINITY, f32::min);
    let (series, row, ..) = all
        .into_iter()
        .filter(|n| n.2 <= dx + 0.5)
        .min_by(|a, b| a.3.total_cmp(&b.3))?;
    Some(Cursor::Point { series, row })
}

/// The series nearest the pointer at a hover, for emphasis; None past NEAREST_SERIES.
pub fn nearest_series(
    model: &ChartModel,
    layout: &ChartLayout,
    hits: &HitIndex,
    view: &View,
    x: f32,
    y: f32,
) -> Option<SeriesKey> {
    let all = nearest(model, layout, hits, view, x, y);
    let best = all.into_iter().min_by(|a, b| a.3.total_cmp(&b.3))?;
    (best.3 <= NEAREST_SERIES).then_some(best.0)
}
