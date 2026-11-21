//! The cursor's motions and the view's changes, data-aware, with limits (mouse-ux §3.2, §6, §9).
//!
//! A [`View`]'s domains are the model's: `x` the independent axis (bands in band units, `i` to
//! `i + 1` for band `i`) and `y` the values, whichever way they're drawn. A domain of None fits
//! the data; every change here returns None again where the result is the fit, so a zoom out to
//! the whole data is the same view as a reset.
//!
//! Limits: x zooms in no further than one interval of the densest visible series and 1 s on a
//! time axis or a millionth of the extent on a number axis; a band axis no further than two
//! bands, or the bands at full width (24 px bars). x zooms out and pans no further than the
//! data's extent: no overscroll. Scatters and heatmaps zoom from half the fit to 32 times it.
//! The value axis of bars and stacked areas never leaves zero: it never zooms or pans.

use super::hover::{column_at, nearest_present, walked};
use super::layout::ChartLayout;
use super::{AxisKind, ChartModel};
use crate::interact::consts::BRUSH_MIN;
use crate::interact::{Action, Axis, BrushKind, Cursor, View};
use crate::spec::MarkKind;

/// The most a 2-D figure zooms in, against its fit.
pub const MAX_ZOOM_2D: f64 = 32.0;
/// A band's slot at which its bar is at full width (24 px bars over 80 % of the slot).
const FULL_BAND: f32 = 30.0;

/// Step, Series, Peak, First, Last and ViewEdge, `count` times. None when there's nothing to
/// move to, or `cursor` is None and the action isn't one that places it (h/l/j/k/gg/G/H/M/L all
/// place it: h/l from the right/left edge of the view, j/k on the series nearest the middle).
///
/// On an x-chart `Step` walks a series' present samples, `Series(1)` goes to the next series
/// down the screen at the cursor's x (the next smaller value, or the next lower segment of a
/// stack) and `Series(-1)` up, `Peak` to the next local maximum of the cursor's series that
/// stands out over its neighbourhood. On a band chart `Step` walks bands and `Series` the
/// band's segments, top first (left first for grouped bars); on a scatter `Step` walks a
/// series' points in x order and `Series` goes to the next series' point nearest the cursor.
/// `Step(1)` with no cursor places it at the view's left edge and `Step(-1)` at its right;
/// `Series` with none, at the middle of the view on the series nearest the plot's middle.
pub fn motion(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    cursor: Option<&Cursor>,
    action: &Action,
    count: u32,
) -> Option<Cursor> {
    let n = i64::from(count.max(1));
    if model.is_heatmap() {
        return cell_motion(model, view, cursor, action, n);
    }
    if model.is_band() {
        return band_motion(model, layout, view, cursor, action, n);
    }
    point_motion(model, layout, view, cursor, action, n)
}

/// The cursor's place in the data: its x and value (a band's centre, a cell's).
pub(crate) fn cursor_data(model: &ChartModel, cursor: &Cursor) -> Option<(f64, f64)> {
    match cursor {
        Cursor::Point { series, row } => {
            let si = model.index_of(series)?;
            let s = &model.series[si];
            let i = *row as usize;
            let x = *s.xs.get(i)?;
            let y = *s.ys.get(i)?;
            Some((x, y))
        }
        Cursor::Band { band, .. } => Some((f64::from(*band) + 0.5, f64::NAN)),
        Cursor::Cell { row, col } => Some((f64::from(*col) + 0.5, f64::from(*row) + 0.5)),
        Cursor::Node(_) => None,
    }
}

/// The present samples of a series, in order.
fn present(model: &ChartModel, si: usize) -> impl Iterator<Item = usize> + '_ {
    let s = &model.series[si];
    (0..s.xs.len().min(s.ys.len())).filter(move |&i| !s.ys[i].is_nan() && !s.xs[i].is_nan())
}

fn point_motion(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    cursor: Option<&Cursor>,
    action: &Action,
    n: i64,
) -> Option<Cursor> {
    let series = walked(model, view);
    if series.is_empty() {
        return None;
    }
    let ((x0, x1), (y0, y1)) = model.fit(view);
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let mid_y = (y0 + y1) / 2.0;
    let current = cursor.and_then(|c| match c {
        Cursor::Point { series: k, row } => {
            let si = model.index_of(k)?;
            series.contains(&si).then_some((si, *row as usize))
        }
        _ => None,
    });
    let point = |si: usize, i: usize| Cursor::Point {
        series: model.series[si].key.clone(),
        row: i as u32,
    };
    let scatter = model.is_scatter();
    // Where a cursor goes with none: the series nearest the plot's middle at an x.
    let place = |want: f64, edge: i8| -> Option<Cursor> {
        let mut best: Option<(f64, usize, usize)> = None;
        for &si in &series {
            let s = &model.series[si];
            let i = match edge {
                -1 => present(model, si).find(|&i| s.xs[i] >= x0),
                1 => present(model, si).filter(|&i| s.xs[i] <= x1).last(),
                _ => nearest_present(&s.xs, &s.ys, want),
            };
            let Some(i) = i else { continue };
            // On a scatter, the leftmost or rightmost point over every series; else the series
            // nearest the middle.
            let d = if scatter && edge != 0 {
                if edge < 0 { s.xs[i] } else { -s.xs[i] }
            } else if scatter {
                let p = layout.at(s.xs[i], s.ys[i]);
                let c = (
                    layout.plot.x + layout.plot.w / 2.0,
                    layout.plot.y + layout.plot.h / 2.0,
                );
                f64::from((p.0 - c.0).hypot(p.1 - c.1))
            } else {
                (model.top(&layout.stacks, si, i) - mid_y).abs()
            };
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, si, i));
            }
        }
        best.map(|(_, si, i)| point(si, i))
    };
    match action {
        Action::Step(d) => {
            let Some((si, i)) = current else {
                return place(0.0, if *d >= 0 { -1 } else { 1 });
            };
            let all: Vec<usize> = present(model, si).collect();
            let at = all.partition_point(|&j| j < i);
            let to = (at as i64 + i64::from(*d) * n).clamp(0, all.len() as i64 - 1) as usize;
            (all.get(to) != Some(&i)).then(|| point(si, all[to]))
        }
        Action::Series(d) => {
            let Some((si, i)) = current else {
                return place((x0 + x1) / 2.0, 0);
            };
            let s = &model.series[si];
            if scatter {
                let k = series.iter().position(|&o| o == si)? as i64;
                let to = (k + i64::from(*d) * n).clamp(0, series.len() as i64 - 1) as usize;
                if series[to] == si {
                    return None;
                }
                let o = series[to];
                let from = layout.at(s.xs[i], s.ys[i]);
                let os = &model.series[o];
                let j = present(model, o).min_by(|&a, &b| {
                    let pa = layout.at(os.xs[a], os.ys[a]);
                    let pb = layout.at(os.xs[b], os.ys[b]);
                    (pa.0 - from.0)
                        .hypot(pa.1 - from.1)
                        .total_cmp(&(pb.0 - from.0).hypot(pb.1 - from.1))
                })?;
                return Some(point(o, j));
            }
            // The series at the cursor's x, from the top of the screen down.
            let col = column_at(model, None, view, s.xs[i]);
            let mut order: Vec<(f64, usize, usize)> = col
                .samples
                .iter()
                .filter_map(|&(o, j)| j.map(|j| (model.top(&layout.stacks, o, j), o, j)))
                .filter(|(v, _, _)| v.is_finite())
                .collect();
            order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let k = order.iter().position(|&(_, o, _)| o == si)? as i64;
            let to = (k + i64::from(*d) * n).clamp(0, order.len() as i64 - 1) as usize;
            let (_, o, j) = order[to];
            (o != si).then(|| point(o, j))
        }
        Action::Peak(d) => {
            let (si, i) = current?;
            let peaks = peaks(model, si);
            let mut at = i;
            for _ in 0..n {
                let next = if *d >= 0 {
                    peaks.iter().copied().find(|&p| p > at)
                } else {
                    peaks.iter().copied().rev().find(|&p| p < at)
                };
                match next {
                    Some(p) => at = p,
                    None => break,
                }
            }
            (at != i).then(|| point(si, at))
        }
        Action::First | Action::Last => {
            let first = matches!(action, Action::First);
            let si = match current {
                Some((si, _)) => si,
                None => match place((x0 + x1) / 2.0, 0)? {
                    Cursor::Point { series: k, .. } => model.index_of(&k)?,
                    _ => return None,
                },
            };
            let i = if first {
                present(model, si).next()
            } else {
                present(model, si).last()
            }?;
            (current != Some((si, i))).then(|| point(si, i))
        }
        Action::ViewEdge(e) => {
            let Some((si, _)) = current else {
                return place((x0 + x1) / 2.0, (*e).signum());
            };
            let s = &model.series[si];
            let i = match e.signum() {
                -1 => present(model, si).find(|&i| s.xs[i] >= x0),
                1 => present(model, si).filter(|&i| s.xs[i] <= x1).last(),
                _ => nearest_present(&s.xs, &s.ys, (x0 + x1) / 2.0),
            }?;
            (current != Some((si, i))).then(|| point(si, i))
        }
        _ => None,
    }
}

/// A series' peaks: samples that stand out over the lowest ground between them and the nearest
/// higher sample on each side (their topographic prominence) by at least
/// [`PEAK_PROMINENCE`] of the series' range, the first of a plateau. Found in `O(n)` with a
/// stack from each side, so `w` over a long series costs one pass.
pub(crate) fn peaks(model: &ChartModel, si: usize) -> Vec<usize> {
    let s = &model.series[si];
    let all: Vec<usize> = present(model, si).collect();
    let ys: Vec<f64> = all.iter().map(|&i| s.ys[i]).collect();
    let n = ys.len();
    if n < 3 {
        return Vec::new();
    }
    let (lo, hi) = ys
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &y| {
            (a.min(y), b.max(y))
        });
    let need = (hi - lo) * PEAK_PROMINENCE;
    if need <= 0.0 {
        return Vec::new();
    }
    // The lowest ground between each sample and the nearest higher one on a side (the whole
    // side where there's none), the sample itself included: a stack of (y, lowest since the
    // entry under it) that pops what isn't higher.
    let ground = |order: &mut dyn Iterator<Item = usize>, strict: bool| -> Vec<f64> {
        let mut out = vec![0.0; n];
        let mut stack: Vec<(f64, f64)> = Vec::new();
        for k in order {
            let y = ys[k];
            let mut low = y;
            while let Some(&(ty, tlow)) = stack.last() {
                let lower = if strict { ty < y } else { ty <= y };
                if !lower {
                    break;
                }
                low = low.min(ty).min(tlow);
                stack.pop();
            }
            out[k] = if stack.is_empty() && low == y { y } else { low };
            stack.push((y, low));
        }
        out
    };
    let left = ground(&mut (0..n), false);
    let right = ground(&mut (0..n).rev(), true);
    (0..n)
        .filter(|&k| k > 0 && k + 1 < n && ys[k - 1] != ys[k])
        .filter(|&k| ys[k] - left[k].max(right[k]) >= need)
        .map(|k| all[k])
        .collect()
}

/// How far a peak stands out over its ground, as a share of its series' range.
pub const PEAK_PROMINENCE: f64 = 0.15;

fn band_motion(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    cursor: Option<&Cursor>,
    action: &Action,
    n: i64,
) -> Option<Cursor> {
    let count = model.x.bands.len();
    if count == 0 {
        return None;
    }
    let ((x0, x1), _) = model.fit(view);
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let first_in = (x0.ceil().max(0.0) as usize).min(count - 1);
    let last_in = ((x1.floor() as i64 - 1).max(first_in as i64) as usize).min(count - 1);
    let current = cursor.and_then(|c| match c {
        Cursor::Band { band, series } => Some((*band as usize, series.clone())),
        _ => None,
    });
    let segments = |band: usize| -> Vec<crate::data::SeriesKey> {
        let mut bars: Vec<&super::marks::BarGeom> = layout
            .bars
            .iter()
            .filter(|b| b.band as usize == band)
            .collect();
        if layout.horizontal {
            bars.sort_by(|a, b| {
                b.rect
                    .right()
                    .total_cmp(&a.rect.right())
                    .then(a.rect.y.total_cmp(&b.rect.y))
            });
        } else {
            bars.sort_by(|a, b| {
                a.rect
                    .x
                    .total_cmp(&b.rect.x)
                    .then(a.rect.y.total_cmp(&b.rect.y))
            });
        }
        let mut keys: Vec<crate::data::SeriesKey> = Vec::new();
        for b in bars {
            let k = model.series[b.series].key.clone();
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        if keys.is_empty() {
            keys = walked(model, view)
                .into_iter()
                .map(|si| model.series[si].key.clone())
                .collect();
        }
        keys
    };
    let band = |b: usize, series: Option<crate::data::SeriesKey>| Cursor::Band {
        band: b as u32,
        series,
    };
    match action {
        Action::Step(d) => {
            let Some((b, s)) = current else {
                let at = if *d >= 0 { first_in } else { last_in };
                return Some(band(at, None));
            };
            let to = (b as i64 + i64::from(*d) * n).clamp(0, count as i64 - 1) as usize;
            (to != b).then(|| band(to, s))
        }
        Action::Series(d) => {
            let (b, s) = match current.clone() {
                Some(c) => c,
                None => ((first_in + last_in) / 2, None),
            };
            let segs = segments(b);
            if segs.is_empty() {
                return None;
            }
            let to = match s.as_ref().and_then(|k| segs.iter().position(|o| o == k)) {
                Some(k) => (k as i64 + i64::from(*d) * n).clamp(0, segs.len() as i64 - 1) as usize,
                None if *d >= 0 => 0,
                None => segs.len() - 1,
            };
            let next = Some(segs[to].clone());
            (current.as_ref().map(|(_, s)| s) != Some(&next)).then(|| band(b, next))
        }
        Action::Peak(d) => {
            let (b, s) = current?;
            let totals: Vec<f64> = (0..count)
                .map(|i| {
                    layout
                        .bars
                        .iter()
                        .filter(|g| g.band as usize == i)
                        .map(|g| model.series[g.series].ys[g.sample])
                        .filter(|v| v.is_finite())
                        .sum()
                })
                .collect();
            let peak = |i: usize| {
                let left = i == 0 || totals[i - 1] < totals[i];
                let right = i + 1 == count || totals[i + 1] <= totals[i];
                left && right
            };
            let mut at = b;
            for _ in 0..n {
                let next = if *d >= 0 {
                    (at + 1..count).find(|&i| peak(i))
                } else {
                    (0..at).rev().find(|&i| peak(i))
                };
                match next {
                    Some(p) => at = p,
                    None => break,
                }
            }
            (at != b).then(|| band(at, s))
        }
        Action::First | Action::Last => {
            let to = if matches!(action, Action::First) {
                0
            } else {
                count - 1
            };
            let s = current.as_ref().and_then(|c| c.1.clone());
            (current.as_ref().map(|c| c.0) != Some(to)).then(|| band(to, s))
        }
        Action::ViewEdge(e) => {
            let to = match e.signum() {
                -1 => first_in,
                1 => last_in,
                _ => (((x0 + x1) / 2.0).floor().max(0.0) as usize).min(count - 1),
            };
            let s = current.as_ref().and_then(|c| c.1.clone());
            (current.as_ref().map(|c| c.0) != Some(to)).then(|| band(to, s))
        }
        _ => None,
    }
}

fn cell_motion(
    model: &ChartModel,
    view: &View,
    cursor: Option<&Cursor>,
    action: &Action,
    n: i64,
) -> Option<Cursor> {
    let cols = model.x.bands.len();
    let rows = model.y.bands.len();
    if cols == 0 || rows == 0 {
        return None;
    }
    let ((x0, x1), _) = model.fit(view);
    let current = cursor.and_then(|c| match c {
        Cursor::Cell { row, col } => Some((*row as i64, *col as i64)),
        _ => None,
    });
    let cell = |r: i64, c: i64| Cursor::Cell {
        row: r.clamp(0, rows as i64 - 1) as u32,
        col: c.clamp(0, cols as i64 - 1) as u32,
    };
    let (r, c) = current.unwrap_or((0, x0.max(0.0).ceil() as i64));
    let next = match action {
        Action::Step(d) if current.is_some() => cell(r, c + i64::from(*d) * n),
        Action::Series(d) if current.is_some() => cell(r + i64::from(*d) * n, c),
        Action::Step(_) | Action::Series(_) => cell(r, c),
        Action::First => cell(r, 0),
        Action::Last => cell(r, cols as i64 - 1),
        Action::ViewEdge(e) => match e.signum() {
            -1 => cell(r, x0.ceil() as i64),
            1 => cell(r, x1.floor() as i64 - 1),
            _ => cell(r, ((x0 + x1) / 2.0).floor() as i64),
        },
        _ => return None,
    };
    (current.map(|(r, c)| cell(r, c)) != Some(next.clone())).then_some(next)
}

/// How an axis's values are spaced: logarithms on a log axis.
fn fwd(kind: AxisKind, v: f64) -> f64 {
    if kind == AxisKind::Log {
        v.max(f64::MIN_POSITIVE).ln()
    } else {
        v
    }
}

fn inv(kind: AxisKind, v: f64) -> f64 {
    if kind == AxisKind::Log { v.exp() } else { v }
}

/// A domain in an axis's spacing, and back.
fn fwd2(kind: AxisKind, d: (f64, f64)) -> (f64, f64) {
    (fwd(kind, d.0), fwd(kind, d.1))
}

fn inv2(kind: AxisKind, d: (f64, f64)) -> (f64, f64) {
    (inv(kind, d.0), inv(kind, d.1))
}

/// `d` moved inside `ext` without changing its span, or `ext` itself if it's no smaller.
fn clamp_into(d: (f64, f64), ext: (f64, f64)) -> (f64, f64) {
    let (lo, hi) = (d.0.min(d.1), d.0.max(d.1));
    let (elo, ehi) = (ext.0.min(ext.1), ext.0.max(ext.1));
    let span = hi - lo;
    if span >= ehi - elo {
        return (elo, ehi);
    }
    if lo < elo {
        (elo, elo + span)
    } else if hi > ehi {
        (ehi - span, ehi)
    } else {
        (lo, hi)
    }
}

/// Whether two domains are the same to a millionth of the larger span.
fn same(a: (f64, f64), b: (f64, f64)) -> bool {
    let tol = (a.1 - a.0)
        .abs()
        .max((b.1 - b.0).abs())
        .max(f64::MIN_POSITIVE)
        * 1e-6;
    (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
}

/// Whether the model's y never leaves zero: bars, stacked areas, a normalised stack.
fn y_fixed(model: &ChartModel) -> bool {
    model.fixed_y()
        || model
            .series
            .iter()
            .any(|s| s.stack == crate::spec::Stack::Normalize)
}

/// A 2-D figure: free in both axes.
fn two_d(model: &ChartModel) -> bool {
    model.is_scatter() || model.is_heatmap()
}

/// The axes a zoom or a pan with no axis named acts on: x, and y on 2-D figures and where y is
/// already manual.
fn axes_of(model: &ChartModel, view: &View, axis: Option<Axis>) -> (bool, bool) {
    match axis {
        Some(Axis::X) => (true, false),
        Some(Axis::Y) => (false, !y_fixed(model)),
        None => (true, !y_fixed(model) && (two_d(model) || view.y.is_some())),
    }
}

/// The domains on screen: the view's, or the fit, y as the layout niced it for its ticks when
/// the layout is of this x.
fn shown(model: &ChartModel, layout: &ChartLayout, view: &View) -> ((f64, f64), (f64, f64)) {
    let (x, y) = model.fit(view);
    let y = if view.y.is_none() && same(layout.domain.0, x) {
        layout.domain.1
    } else {
        y
    };
    (x, y)
}

/// The fit's domains with nothing zoomed: what x and y show with None.
fn fit_of(model: &ChartModel, view: &View) -> ((f64, f64), (f64, f64)) {
    let mut v = view.clone();
    v.x = None;
    v.y = None;
    model.fit(&v)
}

/// The narrowest x may get, in the axis's spacing.
fn min_span_x(model: &ChartModel, layout: &ChartLayout, view: &View) -> f64 {
    let ext = fwd2(model.x.kind, model.extent().0);
    let whole = (ext.1 - ext.0).abs();
    if model.is_band() || model.x.kind == AxisKind::Band {
        let n = model.x.bands.len().max(1) as f64;
        let full = f64::from(layout.lengths().0 / FULL_BAND);
        return full.max(2.0).min(n);
    }
    if two_d(model) {
        let fit = fwd2(model.x.kind, fit_of(model, view).0);
        return (fit.1 - fit.0).abs() / MAX_ZOOM_2D;
    }
    let floor = match model.x.kind {
        AxisKind::Time => 1.0,
        _ => whole * 1e-6,
    };
    let interval = model.min_interval(view);
    let interval = if interval.is_finite() && model.x.kind != AxisKind::Log {
        interval
    } else {
        0.0
    };
    interval.max(floor).max(f64::MIN_POSITIVE)
}

/// The narrowest y may get.
fn min_span_y(model: &ChartModel, view: &View) -> f64 {
    let fit = fwd2(model.y.kind, fit_of(model, view).1);
    let span = (fit.1 - fit.0).abs();
    if two_d(model) {
        span / MAX_ZOOM_2D
    } else {
        (span * 1e-6).max(f64::MIN_POSITIVE)
    }
}

/// A domain zoomed about `anchor` by `factor`, within `min` and the extent, in the axis's
/// spacing.
fn zoomed(d: (f64, f64), anchor: f64, factor: f64, min: f64, ext: (f64, f64)) -> (f64, f64) {
    let (lo, hi) = (d.0.min(d.1), d.0.max(d.1));
    let span = hi - lo;
    let mut new = span / factor;
    let whole = (ext.1 - ext.0).abs();
    new = new
        .max(min.min(span.max(min)))
        .min(whole.max(span.min(whole)));
    if new >= whole {
        return (ext.0.min(ext.1), ext.0.max(ext.1));
    }
    let t = if span > 0.0 {
        ((anchor - lo) / span).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let lo2 = anchor - t * new;
    clamp_into((lo2, lo2 + new), ext)
}

/// The view with `x` set, None where it's the fit.
fn set_x(model: &ChartModel, view: &View, v: &mut View, d: (f64, f64)) {
    let fit = fit_of(model, view).0;
    v.x = if same(d, fit) && !two_d(model) {
        None
    } else {
        Some(d)
    };
}

fn set_y(model: &ChartModel, view: &View, v: &mut View, d: (f64, f64)) {
    if model.is_x_chart() || model.is_band() {
        // On an x-chart y fits the x window; a y set by hand stays set.
        v.y = Some(d);
        return;
    }
    let fit = fit_of(model, view).1;
    v.y = if same(d, fit) && !two_d(model) {
        None
    } else {
        Some(d)
    };
}

/// Follow goes off when the newest sample leaves the view on the right; with `on`, a view that
/// ends within a pixel of it turns follow on (a pan back to the end, as `G`).
fn follow(model: &ChartModel, layout: &ChartLayout, v: &mut View, on: bool) {
    if !model.is_x_chart() {
        return;
    }
    let Some(newest) = model.newest() else {
        return;
    };
    let ((x0, x1), _) = model.fit(v);
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let px = (x1 - x0) / f64::from(layout.lengths().0.max(1.0));
    if newest > x1 + px * 0.5 {
        v.follow = false;
    } else if on && (x1 - newest).abs() <= px {
        v.follow = true;
    }
}

/// Zoom by `factor` (> 1 zooms in) about `at` (a point in the area; None = the cursor, else the
/// middle), on one axis or the figure's zoomable axes, within the limits.
///
/// With no axis named, x zooms, and y with it on a scatter or a heatmap and where y is already
/// set by hand; a y that never leaves zero (bars, stacked areas) never zooms. The value under
/// `at` stays under it. Zooming out to the extent is the fit (None).
pub fn zoom(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    factor: f64,
    at: Option<(f32, f32)>,
    cursor: Option<&Cursor>,
    axis: Option<Axis>,
) -> View {
    if !(factor.is_finite() && factor > 0.0) {
        return view.clone();
    }
    let (zx, zy) = axes_of(model, view, axis);
    let ((x0, x1), (y0, y1)) = shown(model, layout, view);
    let anchor = match at {
        Some((px, py)) => Some((layout.data_x(px, py), layout.data_y(px, py))),
        None => cursor.and_then(|c| cursor_data(model, c)),
    };
    let (ax, ay) = anchor.unwrap_or(((x0 + x1) / 2.0, (y0 + y1) / 2.0));
    let ax = if ax.is_finite() { ax } else { (x0 + x1) / 2.0 };
    let ay = if ay.is_finite() { ay } else { (y0 + y1) / 2.0 };
    let (ext_x, ext_y) = model.extent();
    let mut v = view.clone();
    if zx {
        let k = model.x.kind;
        let d = zoomed(
            fwd2(k, (x0, x1)),
            fwd(k, ax),
            factor,
            min_span_x(model, layout, view),
            fwd2(k, ext_x),
        );
        set_x(model, view, &mut v, inv2(k, d));
    }
    if zy {
        let k = model.y.kind;
        let d = zoomed(
            fwd2(k, (y0, y1)),
            fwd(k, ay),
            factor,
            min_span_y(model, view),
            fwd2(k, ext_y),
        );
        set_y(model, view, &mut v, inv2(k, d));
    }
    follow(model, layout, &mut v, false);
    v
}

/// Pan by (dx, dy) logical px of pointer motion (the content follows the hand), clamped to the
/// extent; on x-charts both components pan x unless `axis` says otherwise.
///
/// The content follows the hand: dx > 0 shows what's to the left (earlier), dy > 0 shows
/// what's above. On an x-chart whose y fits (no axis named), x moves by `dy - dx` px, so a drag
/// to the right and a scroll with dy > 0 (the design's "later in time") each move as the hand
/// does. With y set by hand, and on 2-D figures, dx pans x and dy pans y. A pan back until the
/// newest sample is within a pixel of the right edge turns follow on; one that leaves it off
/// screen turns it off.
pub fn pan(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    dx: f32,
    dy: f32,
    axis: Option<Axis>,
) -> View {
    let ((x0, x1), (y0, y1)) = shown(model, layout, view);
    let (ext_x, ext_y) = model.extent();
    let free = two_d(model) || view.y.is_some();
    // In px along each model axis's screen direction, as the content moves: x across the
    // screen (down it for bars across, whose first band is at the top), y up it (across).
    let (sx, sy) = match (axis, layout.horizontal) {
        (Some(Axis::X), false) => (-dx, 0.0),
        (Some(Axis::X), true) => (-dy, 0.0),
        (Some(Axis::Y), false) => (0.0, dy),
        (Some(Axis::Y), true) => (0.0, -dx),
        (None, false) if free => (-dx, dy),
        (None, true) if free => (-dy, -dx),
        (None, false) => (dy - dx, 0.0),
        (None, true) => (-dy - dx, 0.0),
    };
    let (xlen, ylen) = layout.lengths();
    let mut v = view.clone();
    if sx != 0.0 && xlen > 0.0 {
        let k = model.x.kind;
        let d = fwd2(k, (x0, x1));
        let shift = f64::from(sx) * (d.1 - d.0) / f64::from(xlen);
        let moved = clamp_into((d.0 + shift, d.1 + shift), fwd2(k, ext_x));
        let moved = inv2(k, moved);
        if !same(moved, (x0, x1)) {
            set_x(model, view, &mut v, moved);
        }
    }
    if sy != 0.0 && ylen > 0.0 && !y_fixed(model) && (free || axis == Some(Axis::Y)) {
        let k = model.y.kind;
        let d = fwd2(k, (y0, y1));
        let shift = f64::from(sy) * (d.1 - d.0) / f64::from(ylen);
        let ext = fwd2(k, ext_y);
        // y pans over the data's extent or what shows, whichever is wider.
        let ext = (ext.0.min(d.0), ext.1.max(d.1));
        let moved = inv2(k, clamp_into((d.0 + shift, d.1 + shift), ext));
        if !same(moved, (y0, y1)) {
            set_y(model, view, &mut v, moved);
        }
    }
    if v.x == view.x && v.y == view.y {
        return view.clone();
    }
    follow(model, layout, &mut v, true);
    v
}

/// The view a released brush asks for. A brush smaller than BRUSH_MIN in a dimension
/// collapses to the other; one smaller in both is no change (Some only for a real change).
///
/// An x brush zooms x to its span (at least the narrowest x may be, centred on it); a box
/// zooms both (y goes manual on an x-chart), and a y brush y; a bands brush zooms to the whole
/// bands it touches, at least two, and does nothing when the bands already show at full
/// width. Corners are in the area's logical px and are clamped to the plot.
pub fn brush(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    kind: BrushKind,
    from: (f32, f32),
    to: (f32, f32),
) -> Option<View> {
    let p = layout.plot;
    let cx = |x: f32| x.clamp(p.x, p.right());
    let cy = |y: f32| y.clamp(p.y, p.bottom());
    let (a, b) = ((cx(from.0), cy(from.1)), (cx(to.0), cy(to.1)));
    let (w, h) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    let (mut along_x, mut along_y) = match kind {
        BrushKind::X | BrushKind::Bands => (true, false),
        BrushKind::Y => (false, true),
        BrushKind::Box => (true, true),
    };
    // Along the screen: which pixels measure each model axis.
    let (len_x, len_y) = if layout.horizontal { (h, w) } else { (w, h) };
    if kind == BrushKind::Box {
        along_x = len_x >= BRUSH_MIN;
        along_y = len_y >= BRUSH_MIN && !y_fixed(model);
    } else {
        along_x &= len_x >= BRUSH_MIN;
        along_y &= len_y >= BRUSH_MIN && !y_fixed(model);
    }
    if !along_x && !along_y {
        return None;
    }
    let mut v = view.clone();
    if along_x {
        let (d0, d1) = (layout.data_x(a.0, a.1), layout.data_x(b.0, b.1));
        let (lo, hi) = (d0.min(d1), d0.max(d1));
        let d = if kind == BrushKind::Bands || model.is_band() {
            let n = model.x.bands.len() as f64;
            let (blo, bhi) = (lo.floor().max(0.0), hi.ceil().min(n));
            let min = min_span_x(model, layout, view);
            let shown = model.fit(view).0;
            if (shown.1 - shown.0).abs() <= min + 1e-9 {
                return None;
            }
            let span = (bhi - blo).max(min.ceil().min(n));
            let mid = ((blo + bhi) / 2.0 - span / 2.0).round();
            clamp_into((mid, mid + span), (0.0, n))
        } else {
            let k = model.x.kind;
            let (flo, fhi) = (fwd(k, lo), fwd(k, hi));
            let min = min_span_x(model, layout, view);
            let span = (fhi - flo).max(min);
            let mid = (flo + fhi) / 2.0;
            inv2(
                k,
                clamp_into(
                    (mid - span / 2.0, mid + span / 2.0),
                    fwd2(k, model.extent().0),
                ),
            )
        };
        set_x(model, view, &mut v, d);
    }
    if along_y {
        let (d0, d1) = (layout.data_y(a.0, a.1), layout.data_y(b.0, b.1));
        let k = model.y.kind;
        let (flo, fhi) = (fwd(k, d0.min(d1)), fwd(k, d0.max(d1)));
        let min = min_span_y(model, view);
        let span = (fhi - flo).max(min);
        let mid = (flo + fhi) / 2.0;
        set_y(
            model,
            view,
            &mut v,
            inv2(k, (mid - span / 2.0, mid + span / 2.0)),
        );
    }
    follow(model, layout, &mut v, false);
    (v != *view).then_some(v)
}

/// The brush a left drag on the plot makes here: X on x-charts, Box on scatter, Bands on bands.
pub fn brush_kind(model: &ChartModel) -> BrushKind {
    if model.is_heatmap() || model.is_scatter() {
        BrushKind::Box
    } else if model.is_band() {
        BrushKind::Bands
    } else {
        BrushKind::X
    }
}

/// Reset one axis or both to fit.
pub fn reset(_model: &ChartModel, view: &View, axis: Option<Axis>) -> View {
    let mut v = view.clone();
    match axis {
        Some(Axis::X) => v.x = None,
        Some(Axis::Y) => v.y = None,
        None => {
            v.x = None;
            v.y = None;
        }
    }
    v
}

/// The view scrolled (x only on x-charts) so `cursor` sits at least 10 % inside the plot.
pub fn reveal(model: &ChartModel, layout: &ChartLayout, view: &View, cursor: &Cursor) -> View {
    let _ = layout;
    let Some((cx, cy)) = cursor_data(model, cursor) else {
        return view.clone();
    };
    let ((x0, x1), (y0, y1)) = shown(model, layout, view);
    let (ext_x, ext_y) = model.extent();
    let mut v = view.clone();
    let inside = |d: (f64, f64), c: f64, ext: (f64, f64), k: AxisKind| -> Option<(f64, f64)> {
        if !c.is_finite() {
            return None;
        }
        let (lo, hi) = fwd2(k, (d.0.min(d.1), d.0.max(d.1)));
        let c = fwd(k, c);
        let margin = 0.1 * (hi - lo);
        let shift = if c < lo + margin {
            c - (lo + margin)
        } else if c > hi - margin {
            c - (hi - margin)
        } else {
            return None;
        };
        let moved = inv2(k, clamp_into((lo + shift, hi + shift), fwd2(k, ext)));
        (!same(moved, d)).then_some(moved)
    };
    if let Some(d) = inside((x0, x1), cx, ext_x, model.x.kind) {
        set_x(model, view, &mut v, d);
    }
    if two_d(model)
        && let Some(d) = inside((y0, y1), cy, ext_y, model.y.kind)
    {
        set_y(model, view, &mut v, d);
    }
    v
}

/// Centre the view on the cursor (zz).
pub fn centre(model: &ChartModel, layout: &ChartLayout, view: &View, cursor: &Cursor) -> View {
    let _ = layout;
    let Some((cx, cy)) = cursor_data(model, cursor) else {
        return view.clone();
    };
    let ((x0, x1), (y0, y1)) = shown(model, layout, view);
    let (ext_x, ext_y) = model.extent();
    let mut v = view.clone();
    let centred = |d: (f64, f64), c: f64, ext: (f64, f64), k: AxisKind| -> Option<(f64, f64)> {
        if !c.is_finite() {
            return None;
        }
        let (lo, hi) = fwd2(k, d);
        let half = (hi - lo) / 2.0;
        let c = fwd(k, c);
        let moved = inv2(k, clamp_into((c - half, c + half), fwd2(k, ext)));
        (!same(moved, d)).then_some(moved)
    };
    if let Some(d) = centred((x0, x1), cx, ext_x, model.x.kind) {
        set_x(model, view, &mut v, d);
    }
    if two_d(model)
        && let Some(d) = centred((y0, y1), cy, ext_y, model.y.kind)
    {
        set_y(model, view, &mut v, d);
    }
    v
}

/// A view part way between two (for 180 ms eases): domains interpolated, everything else `to`'s.
///
/// `t` is the eased fraction, 0 to 1 (1 and past give `to` itself). A domain that's None (the
/// fit) at either end can't be interpolated without the model, and comes from `to`: ease a
/// reset between [`fixed`] views and land on `to` at the end.
pub fn tween(from: &View, to: &View, t: f32) -> View {
    if t >= 1.0 || t.is_nan() {
        return to.clone();
    }
    let t = f64::from(t.max(0.0));
    let lerp = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| match (a, b) {
        (Some(a), Some(b)) => Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)),
        (_, b) => b,
    };
    View {
        x: lerp(from.x, to.x),
        y: lerp(from.y, to.y),
        ..to.clone()
    }
}

/// The view with its fitted domains made explicit, for easing between a fit and a zoom.
pub fn fixed(model: &ChartModel, view: &View) -> View {
    let (x, y) = model.fit(view);
    View {
        x: Some(x),
        y: Some(y),
        ..view.clone()
    }
}

/// Whether a series is drawn as a mark the cursor visits.
#[allow(dead_code)]
fn visitable(model: &ChartModel, si: usize) -> bool {
    !matches!(model.series[si].mark, MarkKind::Rule | MarkKind::Text)
}
