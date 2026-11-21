//! The cursor's motions and the view's changes, data-aware, with limits. The stand-in's (see
//! [`super`]): steps along the cursor's series, linear zoom and pan within the extent.

use super::{ChartLayout, ChartModel};
use crate::interact::consts::BRUSH_MIN;
use crate::interact::{Action, Axis, BrushKind, Cursor, View};

/// The visible series, in order.
fn shown<'a>(model: &'a ChartModel, view: &View) -> Vec<&'a super::SeriesInfo> {
    model.series.iter().filter(|s| view.shows(&s.key)).collect()
}

/// Step, Series, Peak, First, Last and ViewEdge, `count` times. None when there's nothing to
/// move to, or `cursor` is None and the action isn't one that places it (h/l/j/k/gg/G/H/M/L all
/// place it: h/l from the right/left edge of the view, j/k on the series nearest the middle).
pub fn motion(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    cursor: Option<&Cursor>,
    action: &Action,
    count: u32,
) -> Option<Cursor> {
    let series = shown(model, view);
    let first = series.first()?;
    let (key, row) = match cursor {
        Some(Cursor::Point { series, row }) => (series.clone(), *row as i64),
        _ => (first.key.clone(), -1),
    };
    let s = series.iter().find(|s| s.key == key).unwrap_or(first);
    let last = s.xs.len() as i64 - 1;
    let n = i64::from(count.max(1));
    let at_x = |x: f64| {
        let i = s.xs.partition_point(|v| *v < x) as i64;
        i.clamp(0, last)
    };
    let (key, row) = match action {
        Action::Step(d) if row < 0 => {
            let (a, b) = layout.x.domain();
            let edge = if *d > 0 { at_x(a) } else { at_x(b).min(last) };
            (s.key.clone(), edge)
        }
        Action::Step(d) | Action::Peak(d) => (s.key.clone(), row + i64::from(*d) * n),
        Action::Series(d) => {
            let i = series.iter().position(|x| x.key == s.key).unwrap_or(0) as i64;
            let j = (i + i64::from(*d) * n).rem_euclid(series.len() as i64) as usize;
            let t = series[j];
            let x = s.xs.get(row.max(0) as usize).copied().unwrap_or(0.0);
            let i = t.xs.partition_point(|v| *v < x).min(t.xs.len() - 1);
            (t.key.clone(), i as i64)
        }
        Action::First => (s.key.clone(), 0),
        Action::Last => (s.key.clone(), last),
        Action::ViewEdge(e) => {
            let (a, b) = layout.x.domain();
            let x = match e {
                -1 => a,
                0 => (a + b) / 2.0,
                _ => b,
            };
            (s.key.clone(), at_x(x))
        }
        _ => return None,
    };
    Some(Cursor::Point {
        series: key,
        row: row.clamp(0, last.max(0)) as u32,
    })
}

/// The cursor's x, if it's on a sample.
fn x_of(model: &ChartModel, cursor: &Cursor) -> Option<f64> {
    match cursor {
        Cursor::Point { series, row } => {
            let s = model.series.iter().find(|s| &s.key == series)?;
            s.xs.get(*row as usize).copied()
        }
        Cursor::Band { band, .. } => Some(f64::from(*band) + 0.5),
        _ => None,
    }
}

/// The view's x domain as it stands.
fn xd(model: &ChartModel, view: &View) -> (f64, f64) {
    view.x.unwrap_or_else(|| model.fit(view).0)
}

/// A domain moved to lie in `ext`, its span at most the extent's.
fn clamp(d: (f64, f64), ext: (f64, f64)) -> (f64, f64) {
    let span = (d.1 - d.0).min(ext.1 - ext.0);
    let lo = d.0.clamp(ext.0, ext.1 - span);
    (lo, lo + span)
}

/// The view scrolled (x only on x-charts) so `cursor` sits at least 10 % inside the plot.
pub fn reveal(model: &ChartModel, _layout: &ChartLayout, view: &View, cursor: &Cursor) -> View {
    let (Some(x), Some((a, b))) = (x_of(model, cursor), view.x) else {
        return view.clone();
    };
    let m = (b - a) * 0.1;
    let shift = if x < a + m {
        x - (a + m)
    } else if x > b - m {
        x - (b - m)
    } else {
        return view.clone();
    };
    let ext = model.extent().0;
    View {
        x: Some(clamp((a + shift, b + shift), ext)),
        ..view.clone()
    }
}

/// Zoom by `factor` (> 1 zooms in) about `at` (a point in the area; None = the cursor, else the
/// middle), on one axis or the figure's zoomable axes, within the limits.
pub fn zoom(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    factor: f64,
    at: Option<(f32, f32)>,
    cursor: Option<&Cursor>,
    axis: Option<Axis>,
) -> View {
    if !(factor.is_finite() && factor > 0.0) || axis == Some(Axis::Y) {
        return view.clone();
    }
    let (a, b) = xd(model, view);
    let ext = model.extent().0;
    let about = match (at, cursor.and_then(|c| x_of(model, c))) {
        (Some((x, _)), _) => layout.x.invert(x),
        (None, Some(x)) => x,
        _ => (a + b) / 2.0,
    };
    let span = ((b - a) / factor).min(ext.1 - ext.0);
    let lo = about - (about - a) / factor;
    let x = clamp((lo, lo + span), ext);
    View {
        x: (x != ext).then_some(x),
        ..view.clone()
    }
}

/// Pan by (dx, dy) logical px of pointer motion (the content follows the hand), clamped to the
/// extent; on x-charts both components pan x unless `axis` says otherwise.
pub fn pan(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    dx: f32,
    dy: f32,
    axis: Option<Axis>,
) -> View {
    if axis == Some(Axis::Y) {
        return view.clone();
    }
    let by = if axis.is_none() && dy.abs() > dx.abs() {
        dy
    } else {
        dx
    };
    let (a, b) = xd(model, view);
    let shift = -f64::from(by) * (b - a) / f64::from(layout.plot.w.max(1.0));
    let ext = model.extent().0;
    let x = clamp((a + shift, b + shift), ext);
    View {
        x: (x != ext).then_some(x),
        ..view.clone()
    }
}

/// The view a released brush asks for. A brush smaller than BRUSH_MIN in a dimension
/// collapses to the other; one smaller in both is no change (Some only for a real change).
pub fn brush(
    _model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    _kind: BrushKind,
    from: (f32, f32),
    to: (f32, f32),
) -> Option<View> {
    if (to.0 - from.0).abs() < BRUSH_MIN {
        return None;
    }
    let (a, b) = (
        layout.x.invert(from.0.min(to.0)),
        layout.x.invert(from.0.max(to.0)),
    );
    Some(View {
        x: Some((a, b)),
        ..view.clone()
    })
}

/// The brush a left drag on the plot makes here: X on x-charts, Box on scatter, Bands on bands.
pub fn brush_kind(model: &ChartModel) -> BrushKind {
    if model.is_band() {
        BrushKind::Bands
    } else if model.is_x_chart() {
        BrushKind::X
    } else {
        BrushKind::Box
    }
}

/// Reset one axis or both to fit.
pub fn reset(_model: &ChartModel, view: &View, axis: Option<Axis>) -> View {
    let mut v = view.clone();
    match axis {
        Some(Axis::X) => v.x = None,
        Some(Axis::Y) => v.y = None,
        None => (v.x, v.y) = (None, None),
    }
    v
}

/// Centre the view on the cursor (zz).
pub fn centre(model: &ChartModel, _layout: &ChartLayout, view: &View, cursor: &Cursor) -> View {
    let Some(x) = x_of(model, cursor) else {
        return view.clone();
    };
    let (a, b) = xd(model, view);
    let half = (b - a) / 2.0;
    let d = clamp((x - half, x + half), model.extent().0);
    View {
        x: (d != model.extent().0).then_some(d),
        ..view.clone()
    }
}

/// A view part way between two (for 180 ms eases): domains interpolated, everything else `to`'s.
pub fn tween(from: &View, to: &View, t: f32) -> View {
    let t = f64::from(t.clamp(0.0, 1.0));
    let mix = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| match (a, b) {
        (Some(a), Some(b)) => Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)),
        (_, b) => b,
    };
    View {
        x: mix(from.x, to.x),
        y: mix(from.y, to.y),
        ..to.clone()
    }
}
