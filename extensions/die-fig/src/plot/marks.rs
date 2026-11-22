//! The marks: line, area, bar, point, rule, band, rect and text, from the model's series into the
//! base scene, with the regions of the discrete ones.
//!
//! The look is the design's: lines 2 px with round joins and caps, decimated to four samples a
//! pixel column through the series' pyramid and broken at each missing sample; an area's wash
//! from 16 % at the plot's top to 3 % at its baseline under its line; bars at most 24 px across
//! with a 4 px rounded data end, square at the baseline, and a 2 px surface gap between stacked
//! segments; points 8 px with a 2 px ring in the surface colour; bands at 12 %; a rule solid
//! unless it carries a label, which makes it a threshold, dashed, its label at its end. Text
//! never wears a series colour, but a label inside a filled mark, in white or near black by the
//! fill's luminance (ruling 18).

use super::layout::{ChartLayout, TICK};
use super::{ChartModel, SeriesInfo};
use crate::algo::HitIndex;
use crate::interact::View;
use crate::scene::{
    Anchor, Cap, Hit, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec,
};
use crate::spec::{Curve, Labels, MarkKind, Stack};
use crate::theme::{Color, Theme};

/// The widest a bar is.
pub const BAR_MAX: f32 = 24.0;
/// The radius of a bar's data end.
pub const BAR_RADIUS: f32 = 4.0;
/// The surface gap between stacked segments and between touching bars.
pub const BAR_GAP: f32 = 2.0;
/// An area's wash at the plot's top and at its baseline.
pub const WASH: (f32, f32) = (0.16, 0.03);
/// A stacked area band's flat wash.
pub const STACK_WASH: f32 = 0.16;
/// A band's fill.
pub const BAND_ALPHA: f32 = 0.12;
/// A point's ring in the surface colour.
pub const RING: f32 = 2.0;
/// A bubble's radius at the smallest size and at the largest.
pub const BUBBLE_MIN: f32 = 4.0;
/// The largest bubble's radius.
pub const BUBBLE_MAX: f32 = 18.0;
/// A threshold's dashes.
pub const RULE_DASH: (f32, f32) = (4.0, 3.0);
/// A dashed series' dashes (`dash: true`, ruling 15).
pub const SERIES_DASH: (f32, f32) = (6.0, 4.0);

/// A bar or a stacked segment, where it's drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct BarGeom {
    /// Its series, an index into [`ChartModel::series`].
    pub series: usize,
    /// Its sample in the series.
    pub sample: usize,
    /// Its band on a band axis, else its sample.
    pub band: u32,
    /// Its rectangle.
    pub rect: Rect,
    /// Its corners' radii: top-left, top-right, bottom-right, bottom-left.
    pub radii: [f32; 4],
    /// Whether it's the end of its stack (or a bar alone): its value label goes past it.
    pub end: bool,
}

/// How a series is drawn: in the base, or again over the emphasis wash.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Look {
    /// The base layer.
    Base,
    /// Over the wash, at full strength.
    Emphasis,
}

/// Draws every visible series, rule and note into `scene`, layer by layer, and indexes what the
/// pointer finds.
pub(crate) fn draw(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    hits: &mut HitIndex,
    m: &mut dyn Measure,
) {
    let layers = model
        .series
        .iter()
        .map(|s| s.layer)
        .chain(model.rules.iter().map(|r| r.layer))
        .chain(model.texts.iter().map(|t| t.layer))
        .max();
    scene.push(Item::PushClip(l.clip));
    if let Some(top) = layers {
        for li in 0..=top {
            // Bands under the rest of their layer, then areas, then lines and the rest.
            for pass in 0..3 {
                for (si, s) in model.series.iter().enumerate() {
                    let order = match s.mark {
                        MarkKind::Band => 0,
                        MarkKind::Area => 1,
                        _ => 2,
                    };
                    if s.layer != li || order != pass || !model.visible(view, &s.key) {
                        continue;
                    }
                    series(model, l, theme, scene, si, Look::Base);
                }
            }
            for r in model.rules.iter().filter(|r| r.layer == li) {
                rule_line(l, theme, scene, r);
            }
        }
    }
    scene.push(Item::PopClip);
    index(model, view, l, hits);
    for b in &l.bars {
        let s = &model.series[b.series];
        scene.region(
            b.rect.inflate(1.0),
            Hit::Bar {
                panel: 0,
                band: b.band,
                series: Some(s.key.clone()),
            },
        );
    }
    cells(model, view, l, scene);
    for r in &model.rules {
        rule_label(model, view, l, theme, scene, r, m);
    }
    for t in &model.texts {
        let (x, y) = l.at(t.x, t.y);
        if !l.clip.contains(x, y) {
            continue;
        }
        scene.push(Item::Text {
            text: t.text.clone(),
            x,
            y: y - 6.0,
            spec: TextSpec::new(TICK),
            anchor: Anchor::Middle,
            color: theme.text,
            alpha: 1.0,
            max_width: Some(l.plot.w),
        });
    }
    bar_labels(model, view, l, theme, scene, m);
    if !model.series.is_empty() && model.keys().iter().all(|k| !model.visible(view, k)) {
        super::errors::all_hidden(scene, l.plot, theme, m);
    }
}

/// One series' mark.
pub(crate) fn series(
    model: &ChartModel,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    si: usize,
    look: Look,
) {
    let s = &model.series[si];
    let color = model.color(s, theme);
    match s.mark {
        MarkKind::Line => line(model, l, theme, scene, si, color, look),
        MarkKind::Area => {
            area(model, l, scene, si, color, look);
            line(model, l, theme, scene, si, color, look);
        }
        MarkKind::Band => band(model, l, scene, si, color, look),
        MarkKind::Bar => {
            for b in l.bars.iter().filter(|b| b.series == si) {
                bar(scene, b, color);
            }
        }
        MarkKind::Point => points(model, l, theme, scene, si, color, look),
        MarkKind::Rect => {
            let lo_hi = rect_extent(model);
            for i in 0..s.xs.len() {
                if let Some(r) = cell_rect(l, s, i) {
                    let v = s.sizes.as_ref().map_or(f64::NAN, |v| v[i]);
                    let c = ramp(theme, color, lo_hi, v);
                    scene.push(Item::Path {
                        path: PathData::rounded(r, [2.0; 4]),
                        fill: Some(Paint::solid(c, 1.0)),
                        stroke: None,
                    });
                }
            }
        }
        MarkKind::Rule | MarkKind::Text => {}
    }
}

/// A bar or segment.
pub(crate) fn bar(scene: &mut Scene, b: &BarGeom, color: Color) {
    scene.push(Item::Path {
        path: PathData::rounded(b.rect, b.radii),
        fill: Some(Paint::solid(color, 1.0)),
        stroke: None,
    });
}

/// The samples a line runs through over the view, as unbroken runs of sample indices: four a
/// pixel column through the pyramid where there is one, each run ending at a missing sample,
/// and the first and last joined to the samples beyond the view so it runs on to the edges.
pub(crate) fn runs(model: &ChartModel, l: &ChartLayout, si: usize) -> Vec<Vec<usize>> {
    let s = &model.series[si];
    let (a, b) = l.domain.0;
    let (x0, x1) = (a.min(b), a.max(b));
    let len = s.xs.len().min(s.ys.len());
    let present = |i: usize| !s.ys[i].is_nan() && !s.xs[i].is_nan();
    let mut out: Vec<Vec<usize>> = Vec::new();
    match &s.pyramid {
        Some(p) if !p.is_empty() => {
            let columns = ((l.lengths().0 * l.scale).ceil() as usize).max(1);
            let pieces = p.line(&s.xs, &s.ys, x0, x1, columns);
            let (below, above) = p.beyond(&s.xs, x0, x1);
            let mut cur: Vec<usize> = Vec::new();
            if let Some(b) = below {
                cur.push(b as usize);
            }
            for (_, [f, lo, hi, last]) in pieces {
                let (lo, hi) = (lo.min(hi), lo.max(hi));
                if let Some(&prev) = cur.last()
                    && p.gap(prev as u32, f)
                {
                    out.push(std::mem::take(&mut cur));
                }
                for i in [f, lo, hi, last] {
                    if cur.last() != Some(&(i as usize)) {
                        cur.push(i as usize);
                    }
                }
            }
            if let Some(a) = above {
                match cur.last() {
                    Some(&prev) if p.gap(prev as u32, a) => {
                        out.push(std::mem::take(&mut cur));
                    }
                    _ => {}
                }
                if !cur.is_empty() {
                    cur.push(a as usize);
                }
            }
            if !cur.is_empty() {
                out.push(cur);
            }
        }
        _ => {
            // Every sample in the window and one either side, broken at each missing one.
            let lo = s.xs[..len].partition_point(|&x| x < x0);
            let hi = s.xs[..len].partition_point(|&x| x <= x1);
            let (lo, hi) = (lo.saturating_sub(1), (hi + 1).min(len));
            let mut cur: Vec<usize> = Vec::new();
            for i in lo..hi {
                if present(i) {
                    cur.push(i);
                } else if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            if !cur.is_empty() {
                out.push(cur);
            }
        }
    }
    out
}

/// A sample's value where it's drawn: a stacked one's top.
fn value(model: &ChartModel, l: &ChartLayout, si: usize, i: usize) -> f64 {
    model.top(&l.stacks, si, i)
}

/// The screen points of a run, with a step curve's corners.
fn points_of(model: &ChartModel, l: &ChartLayout, si: usize, run: &[usize]) -> Vec<(f32, f32)> {
    let s = &model.series[si];
    let step = model
        .chart
        .layers
        .get(usize::from(s.layer))
        .is_some_and(|ly| ly.mark.curve == Curve::Step);
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(run.len() * if step { 2 } else { 1 });
    for &i in run {
        let p = l.at(s.xs[i], value(model, l, si, i));
        if !(p.0.is_finite() && p.1.is_finite()) {
            continue;
        }
        if step && let Some(&(_, py)) = pts.last() {
            pts.push((p.0, py));
        }
        pts.push(p);
    }
    pts
}

fn line(
    model: &ChartModel,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    si: usize,
    color: Color,
    look: Look,
) {
    let s = &model.series[si];
    let width = model.line_width(s);
    let dash = (model.chart.view.render_options && s.style.dash).then_some(SERIES_DASH);
    let marked = model
        .chart
        .layers
        .get(usize::from(s.layer))
        .is_some_and(|ly| ly.mark.points);
    for run in runs(model, l, si) {
        let pts = points_of(model, l, si, &run);
        if pts.len() == 1 {
            // A lone sample between gaps is a dot, so it shows.
            dot(scene, pts[0], width + 1.0, color, theme.surface, 1.5);
            continue;
        }
        scene.push(Item::Path {
            path: PathData::polyline(&pts),
            fill: None,
            stroke: Some(Stroke {
                width,
                paint: Paint::solid(color, 1.0),
                cap: Cap::Round,
                join: Join::Round,
                dash,
                hairline: false,
            }),
        });
        if marked {
            // Markers on the samples (not a step's corners), where they're far enough apart
            // to tell.
            let samples: Vec<(f32, f32)> = run
                .iter()
                .map(|&i| l.at(s.xs[i], value(model, l, si, i)))
                .filter(|p| p.0.is_finite() && p.1.is_finite())
                .collect();
            let spaced = samples.windows(2).all(|w| (w[1].0 - w[0].0).abs() >= 8.0);
            if spaced {
                for &p in &samples {
                    dot(scene, p, 3.0, color, theme.surface, 1.5);
                }
            }
        }
    }
    let _ = look;
}

/// A filled dot with a ring in the surface colour.
pub(crate) fn dot(
    scene: &mut Scene,
    p: (f32, f32),
    r: f32,
    color: Color,
    surface: Color,
    ring: f32,
) {
    if ring > 0.0 {
        scene.push(Item::Path {
            path: PathData::circle(p.0, p.1, r + ring),
            fill: Some(Paint::solid(surface, 1.0)),
            stroke: None,
        });
    }
    scene.push(Item::Path {
        path: PathData::circle(p.0, p.1, r),
        fill: Some(Paint::solid(color, 1.0)),
        stroke: None,
    });
}

/// Where the value axis's zero is drawn, inside the plot.
pub(crate) fn baseline(l: &ChartLayout) -> f32 {
    let v = l.vy(0.0);
    let (a, b) = if l.horizontal {
        (l.plot.x, l.plot.right())
    } else {
        (l.plot.y, l.plot.bottom())
    };
    if v.is_finite() {
        v.clamp(a.min(b), a.max(b))
    } else if l.horizontal {
        a
    } else {
        b
    }
}

fn area(
    model: &ChartModel,
    l: &ChartLayout,
    scene: &mut Scene,
    si: usize,
    color: Color,
    look: Look,
) {
    let s = &model.series[si];
    let stacked = l.stacks.lo[si].is_some();
    let base = baseline(l);
    let (top, bottom) = match look {
        Look::Base => WASH,
        Look::Emphasis => (0.24, 0.06),
    };
    // A stack's bands are flat washes, so the lower ones read as well as the upper.
    let paint = if stacked {
        Paint::solid(
            color,
            if look == Look::Emphasis {
                STACK_WASH * 1.7
            } else {
                STACK_WASH
            },
        )
    } else {
        Paint::Linear {
            from: (0.0, l.plot.y),
            to: (0.0, base.max(l.plot.y + 1.0)),
            stops: vec![(0.0, color, top), (1.0, color, bottom)],
        }
    };
    let step = model
        .chart
        .layers
        .get(usize::from(s.layer))
        .is_some_and(|ly| ly.mark.curve == Curve::Step);
    for run in runs(model, l, si) {
        let pts = points_of(model, l, si, &run);
        if pts.len() < 2 {
            continue;
        }
        let mut poly = pts.clone();
        if let (true, Some(lo)) = (stacked, &l.stacks.lo[si]) {
            let mut under: Vec<(f32, f32)> = Vec::with_capacity(run.len() * 2);
            for &i in &run {
                let p = l.at(s.xs[i], lo[i]);
                if step && let Some(&(_, py)) = under.last() {
                    under.push((p.0, py));
                }
                under.push(p);
            }
            poly.extend(under.into_iter().rev());
        } else {
            let (first, last) = (pts[0], pts[pts.len() - 1]);
            poly.push((last.0, base));
            poly.push((first.0, base));
        }
        let mut path = PathData::polyline(&poly);
        path.verbs.push(crate::scene::Verb::Close);
        scene.push(Item::Path {
            path,
            fill: Some(paint.clone()),
            stroke: None,
        });
    }
}

fn band(
    model: &ChartModel,
    l: &ChartLayout,
    scene: &mut Scene,
    si: usize,
    color: Color,
    look: Look,
) {
    let s = &model.series[si];
    let Some(y2) = &s.y2s else {
        return;
    };
    let alpha = match look {
        Look::Base => BAND_ALPHA,
        Look::Emphasis => BAND_ALPHA * 1.6,
    };
    for run in runs(model, l, si) {
        let upper: Vec<(f32, f32)> = run.iter().map(|&i| l.at(s.xs[i], s.ys[i])).collect();
        let lower: Vec<(f32, f32)> = run
            .iter()
            .rev()
            .filter(|&&i| !y2[i].is_nan())
            .map(|&i| l.at(s.xs[i], y2[i]))
            .collect();
        if upper.len() < 2 || lower.len() < 2 {
            continue;
        }
        let mut poly = upper;
        poly.extend(lower);
        let mut path = PathData::polyline(&poly);
        path.verbs.push(crate::scene::Verb::Close);
        scene.push(Item::Path {
            path,
            fill: Some(Paint::solid(color, alpha)),
            stroke: None,
        });
    }
}

/// A point's radius: its `fig` size's, or a bubble's by its size field.
pub(crate) fn point_radius(
    model: &ChartModel,
    s: &SeriesInfo,
    i: usize,
    sizes: Option<(f64, f64)>,
) -> f32 {
    match (&s.sizes, sizes) {
        (Some(v), Some((lo, hi))) if v[i].is_finite() => {
            let t = if hi > lo {
                ((v[i] - lo) / (hi - lo)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            BUBBLE_MIN + (BUBBLE_MAX - BUBBLE_MIN) * t.sqrt() as f32
        }
        _ => model.point_size(s) / 2.0,
    }
}

/// The extent of every point series' sizes, for bubbles.
pub(crate) fn size_extent(model: &ChartModel) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in model.series.iter().filter(|s| s.mark == MarkKind::Point) {
        for &v in s.sizes.iter().flat_map(|v| v.iter()) {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    lo.is_finite().then_some((lo, hi))
}

fn points(
    model: &ChartModel,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    si: usize,
    color: Color,
    look: Look,
) {
    let s = &model.series[si];
    let sizes = size_extent(model);
    let bubbles = s.sizes.is_some() && sizes.is_some();
    let mut order: Vec<usize> = (0..s.xs.len()).collect();
    if bubbles {
        // The big ones first, so the small ones stay on top and findable.
        let r = |i: usize| s.sizes.as_ref().map_or(0.0, |v| v[i]);
        order.sort_by(|&a, &b| r(b).total_cmp(&r(a)));
    }
    let area = l.clip;
    for i in order {
        if s.ys[i].is_nan() || s.xs[i].is_nan() {
            continue;
        }
        let p = l.at(s.xs[i], s.ys[i]);
        let r = point_radius(model, s, i, sizes);
        if !area.inflate(r + RING).contains(p.0, p.1) {
            continue;
        }
        if bubbles {
            scene.push(Item::Path {
                path: PathData::circle(p.0, p.1, r),
                fill: Some(Paint::solid(
                    color,
                    if look == Look::Emphasis { 0.85 } else { 0.7 },
                )),
                stroke: Some(Stroke::solid(1.0, theme.surface, 1.0)),
            });
        } else {
            dot(scene, p, r, color, theme.surface, RING);
        }
    }
}

/// The value extent of a heatmap's cells.
fn rect_extent(model: &ChartModel) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in model.series.iter().filter(|s| s.mark == MarkKind::Rect) {
        for &v in s.sizes.iter().flat_map(|v| v.iter()) {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    (lo, hi)
}

/// A sequential colour: from the surface's tint of `color` to `color` itself.
fn ramp(theme: &Theme, color: Color, (lo, hi): (f64, f64), v: f64) -> Color {
    let t = if v.is_nan() {
        0.0
    } else if hi > lo {
        ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32
    } else {
        1.0
    };
    mix(theme.surface, color, 0.12 + 0.88 * t)
}

/// `a` mixed toward `b` by `t`.
pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    let f = |x: u8, y: u8| {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// A heatmap cell's rectangle, with a 2 px surface gap.
pub(crate) fn cell_rect(l: &ChartLayout, s: &SeriesInfo, i: usize) -> Option<Rect> {
    let (x, y) = (s.xs[i], s.ys[i]);
    if x.is_nan() || y.is_nan() {
        return None;
    }
    let (bx, bw) = l.x.band_of(x as usize)?;
    let (by, bh) = l.y.band_of(y as usize)?;
    Some(Rect::new(bx, by, bw, bh))
}

fn cells(model: &ChartModel, view: &View, l: &ChartLayout, scene: &mut Scene) {
    for s in model
        .series
        .iter()
        .filter(|s| s.mark == MarkKind::Rect && model.visible(view, &s.key))
    {
        for i in 0..s.xs.len() {
            if let Some(r) = cell_rect(l, s, i) {
                scene.region(
                    r.inflate(1.0),
                    Hit::Cell {
                        panel: 0,
                        row: s.ys[i] as u32,
                        col: s.xs[i] as u32,
                    },
                );
            }
        }
    }
}

/// The bars of every visible bar series, and the segments of stacked ones, under `view`.
pub(crate) fn bars(model: &ChartModel, view: &View, l: &ChartLayout) -> Vec<BarGeom> {
    let bar_series: Vec<usize> = (0..model.series.len())
        .filter(|&i| {
            model.series[i].mark == MarkKind::Bar && model.visible(view, &model.series[i].key)
        })
        .collect();
    if bar_series.is_empty() {
        return Vec::new();
    }
    // Columns side by side within a band or interval: one a stacked layer, one an unstacked
    // series.
    let mut columns: Vec<Vec<usize>> = Vec::new();
    let mut layer_col: Vec<(u16, usize)> = Vec::new();
    for &si in &bar_series {
        let s = &model.series[si];
        if l.stacks.lo[si].is_some() {
            if let Some(&(_, c)) = layer_col.iter().find(|(ly, _)| *ly == s.layer) {
                columns[c].push(si);
                continue;
            }
            layer_col.push((s.layer, columns.len()));
        }
        columns.push(vec![si]);
    }
    let k = columns.len() as f32;
    let base = baseline(l);
    let band_axis = model.is_band();
    let (ind_lo, ind_hi) = if l.horizontal {
        (l.plot.y, l.plot.bottom())
    } else {
        (l.plot.x, l.plot.right())
    };
    let mut out = Vec::new();
    for (c, members) in columns.iter().enumerate() {
        for &si in members {
            let s = &model.series[si];
            for i in 0..s.xs.len() {
                let x = s.xs[i];
                if x.is_nan() || s.ys[i].is_nan() {
                    continue;
                }
                // The slot along the independent axis: a band's, or the interval's, or a bin's.
                let (start, width, band) = if band_axis {
                    let Some((a, w)) = l.ind().band_of(x as usize) else {
                        continue;
                    };
                    (a, w, x as u32)
                } else if let Some(end) = s.ends.as_ref().map(|e| e[i]).filter(|e| e.is_finite()) {
                    let (a, b) = (l.ix(x), l.ix(end));
                    let (a, b) = (a.min(b), a.max(b));
                    // Bins touch, with the surface's gap between them.
                    (a + BAR_GAP / 2.0, (b - a - BAR_GAP).max(1.0), i as u32)
                } else {
                    let step = if s.interval > 0.0 {
                        (l.ix(x + s.interval) - l.ix(x)).abs()
                    } else {
                        (ind_hi - ind_lo) / 8.0
                    };
                    let w = (step * 0.8).max(1.0);
                    (l.ix(x) - w / 2.0, w, i as u32)
                };
                if start > ind_hi + 1.0 || start + width < ind_lo - 1.0 {
                    continue;
                }
                let binned = !band_axis && s.ends.is_some();
                let cw = if binned {
                    ((width - BAR_GAP * (k - 1.0)) / k).max(1.0)
                } else {
                    ((width - BAR_GAP * (k - 1.0)) / k).clamp(1.0, BAR_MAX)
                };
                let group = cw * k + BAR_GAP * (k - 1.0);
                let lead = start + (width - group) / 2.0 + c as f32 * (cw + BAR_GAP);
                let (lo, hi) = model.span(&l.stacks, si, i);
                if !(lo.is_finite() && hi.is_finite()) || lo == hi {
                    continue;
                }
                let negative = s.ys[i] < 0.0;
                // The end toward zero, and the end away from it.
                let near0 = if lo == 0.0 { base } else { l.vy(lo) };
                let far = l.vy(hi);
                let toward = if far >= near0 { 1.0 } else { -1.0 };
                // Something under it in the stack: the surface's gap, and at least a pixel past it.
                let near = if lo != 0.0 {
                    near0 + toward * BAR_GAP
                } else {
                    near0
                };
                let len = ((far - near) * toward).max(1.0);
                let (a, b) = if toward > 0.0 {
                    (near, near + len)
                } else {
                    (near - len, near)
                };
                let end = is_end(model, l, si, i, members);
                let r = if end {
                    BAR_RADIUS.min(cw / 2.0).min(len)
                } else {
                    0.0
                };
                let (rect, radii) = if l.horizontal {
                    let rect = Rect::new(a, lead, b - a, cw);
                    let radii = if negative {
                        [r, 0.0, 0.0, r]
                    } else {
                        [0.0, r, r, 0.0]
                    };
                    (rect, radii)
                } else {
                    let rect = Rect::new(lead, a, cw, b - a);
                    let radii = if negative {
                        [0.0, 0.0, r, r]
                    } else {
                        [r, r, 0.0, 0.0]
                    };
                    (rect, radii)
                };
                out.push(BarGeom {
                    series: si,
                    sample: i,
                    band,
                    rect,
                    radii,
                    end,
                });
            }
        }
    }
    out
}

/// Whether a sample's segment is the end of its stack on its side of zero: nothing visible in
/// its column sits past it.
fn is_end(model: &ChartModel, l: &ChartLayout, si: usize, i: usize, members: &[usize]) -> bool {
    let Some(hi) = l.stacks.hi[si].as_ref() else {
        return true;
    };
    let top = hi[i];
    let x = model.series[si].xs[i];
    let positive = model.series[si].ys[i] >= 0.0;
    !members.iter().any(|&o| {
        if o == si {
            return false;
        }
        let os = &model.series[o];
        let Some(ohi) = l.stacks.hi[o].as_ref() else {
            return false;
        };
        os.xs.iter().enumerate().any(|(j, &ox)| {
            ox.to_bits() == x.to_bits()
                && os.ys[j].is_finite()
                && os.ys[j] != 0.0
                && (os.ys[j] >= 0.0) == positive
                && if positive { ohi[j] > top } else { ohi[j] < top }
        })
    })
}

/// A rule's line across the plot (or up it).
fn rule_line(l: &ChartLayout, theme: &Theme, scene: &mut Scene, r: &super::RuleMark) {
    let color = r
        .color
        .map_or(theme.muted, |c| crate::theme::color_of(c, theme));
    let dashed = r.dash || r.label.is_some();
    let p = l.plot;
    let (a, b) = if r.vertical {
        let x = l.ix(r.value);
        if !x.is_finite() || x < p.x - 0.5 || x > p.right() + 0.5 {
            return;
        }
        ((x, p.y), (x, p.bottom()))
    } else {
        let y = l.vy(r.value);
        if !y.is_finite() || y < p.y - 0.5 || y > p.bottom() + 0.5 {
            return;
        }
        ((p.x, y), (p.right(), y))
    };
    scene.push(Item::Path {
        path: PathData::polyline(&[a, b]),
        fill: None,
        stroke: Some(Stroke {
            dash: dashed.then_some(RULE_DASH),
            ..Stroke::hairline(color, 0.9)
        }),
    });
}

/// How many visible unstacked lines and areas run through `r` on the screen.
fn crossings(model: &ChartModel, view: &View, l: &ChartLayout, r: &Rect) -> usize {
    if l.horizontal {
        return 0;
    }
    let r = r.inflate(1.5);
    let through = |s: &SeriesInfo| {
        let mut prev: Option<(f32, f32)> = None;
        for (&x, &y) in s.xs.iter().zip(s.ys.iter()) {
            if !y.is_finite() || !x.is_finite() {
                prev = None;
                continue;
            }
            let p = l.at(x, y);
            if let Some(q) = prev
                && !(q.0 < r.x && p.0 < r.x)
                && !(q.0 > r.right() && p.0 > r.right())
                && crate::graph::label::seg_meets(q, p, &r)
            {
                return true;
            }
            prev = Some(p);
        }
        false
    };
    model
        .series
        .iter()
        .filter(|s| matches!(s.mark, MarkKind::Line | MarkKind::Area) && s.stack == Stack::None)
        .filter(|s| model.visible(view, &s.key) && through(s))
        .count()
}

/// A rule's label at its end, in the text's colours: a horizontal one's at the right end above
/// the line, else below it, else at the left end, wherever the fewest lines run through it.
fn rule_label(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    r: &super::RuleMark,
    m: &mut dyn Measure,
) {
    let Some(text) = r.label.as_deref().filter(|t| !t.is_empty()) else {
        return;
    };
    let spec = TextSpec::new(TICK);
    let p = l.plot;
    if r.vertical {
        let x = l.ix(r.value);
        if !x.is_finite() || x < p.x - 0.5 || x > p.right() + 0.5 {
            return;
        }
        let room = (p.right() - x - 6.0).max(0.0);
        let (tx, anchor, max) = if room >= 60.0 {
            (x + 4.0, Anchor::Start, room)
        } else {
            (x - 4.0, Anchor::End, (x - p.x - 6.0).max(0.0))
        };
        let text = m.fit(text, spec, max);
        scene.push(Item::Text {
            text: text.into(),
            x: tx,
            y: p.y + m.ascent(spec) + 2.0,
            spec,
            anchor,
            color: theme.text,
            alpha: 1.0,
            max_width: None,
        });
    } else {
        let y = l.vy(r.value);
        if !y.is_finite() || y < p.y - 0.5 || y > p.bottom() + 0.5 {
            return;
        }
        let text = m.fit(text, spec, p.w * 0.5);
        let w = m.width(&text, spec);
        let (asc, desc) = (m.ascent(spec), m.descent(spec));
        // Above the line or below it, at the right end or the left, where it stays in the plot
        // and the fewest lines cross it; a surface plate under it so a line that still crosses
        // it doesn't cut the words.
        let spot = |above: bool, right: bool| {
            let base = if above { y - 4.0 } else { y + 4.0 + asc };
            let (x, anchor) = if right {
                (p.right() - 4.0, Anchor::End)
            } else {
                (p.x + 4.0, Anchor::Start)
            };
            let left = if right { x - w } else { x };
            let plate = Rect::new(left - 3.0, base - asc - 1.0, w + 6.0, asc + desc + 2.0);
            (base, x, anchor, plate)
        };
        let fits = |plate: &Rect| plate.y >= p.y && plate.bottom() <= p.bottom();
        let mut best: Option<(usize, (f32, f32, Anchor, Rect))> = None;
        for (above, right) in [(true, true), (false, true), (true, false), (false, false)] {
            let s = spot(above, right);
            if !fits(&s.3) {
                continue;
            }
            let n = crossings(model, view, l, &s.3);
            if best.as_ref().is_none_or(|(b, _)| n < *b) {
                best = Some((n, s));
            }
            if n == 0 {
                break;
            }
        }
        let (base, x, anchor, plate) =
            best.map_or_else(|| spot(y - 4.0 - asc >= p.y, true), |(_, s)| s);
        scene.push(Item::Path {
            path: PathData::rounded(plate, [3.0; 4]),
            fill: Some(Paint::solid(theme.surface, 0.85)),
            stroke: None,
        });
        scene.push(Item::Text {
            text: text.into(),
            x,
            y: base,
            spec,
            anchor,
            color: theme.text,
            alpha: 1.0,
            max_width: None,
        });
    }
}

/// Each band's total past its bar, when every band has one column and the labels fit: m2's.
fn bar_labels(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    m: &mut dyn Measure,
) {
    for (_, item, _) in bar_totals(model, view, l, theme, m) {
        scene.push(item);
    }
}

/// The bands' totals past their bars, as [`bar_labels`] draws them: each band, its text and
/// its box. None at all when one doesn't fit.
pub(crate) fn bar_totals(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Vec<(u32, Item, Rect)> {
    let none = Vec::new();
    if !model.is_band() || l.bars.is_empty() || model.chart.view.labels == Labels::None {
        return none;
    }
    if model.series.iter().any(|s| s.stack == Stack::Normalize) {
        return none;
    }
    // One column a band: a stacked layer, or one series.
    let mut columns = 0;
    let mut stacked_layers: Vec<u16> = Vec::new();
    for (si, s) in model.series.iter().enumerate() {
        if s.mark != MarkKind::Bar || !model.visible(view, &s.key) {
            continue;
        }
        if l.stacks.lo[si].is_some() {
            if !stacked_layers.contains(&s.layer) {
                stacked_layers.push(s.layer);
                columns += 1;
            }
        } else {
            columns += 1;
        }
    }
    if columns != 1 {
        return none;
    }
    let spec = TextSpec::new(TICK).tabular();
    let step = l.ind().band_of(0).map_or(0.0, |(_, w)| w / 0.8);
    let mut placed: Vec<(u32, String, f32, f32, Anchor, Color)> = Vec::new();
    for b in l.bars.iter().filter(|b| b.end) {
        // The band's total of what's visible.
        let total: f64 = l
            .bars
            .iter()
            .filter(|o| o.band == b.band)
            .map(|o| model.series[o.series].ys[o.sample])
            .filter(|v| v.is_finite())
            .sum();
        let text = model.y_label(total);
        let tw = m.width(&text, spec);
        if tw + 6.0 > step && !l.horizontal {
            return none;
        }
        let (a, d) = (m.ascent(spec), m.descent(spec));
        if l.horizontal {
            let x = b.rect.right() + 6.0;
            if x + tw > l.plot.right() + 2.0 {
                return none;
            }
            placed.push((
                b.band,
                text,
                x,
                b.rect.y + b.rect.h / 2.0 + m.cap(spec) / 2.0,
                Anchor::Start,
                theme.muted,
            ));
        } else {
            let negative = model.series[b.series].ys[b.sample] < 0.0;
            let cx = b.rect.x + b.rect.w / 2.0;
            let y = if negative {
                b.rect.bottom() + 6.0 + a
            } else {
                b.rect.y - 6.0
            };
            if y - a < l.plot.y - 2.0 || y + d > l.plot.bottom() + 2.0 + a + d {
                // No room past the bar: inside it if it's tall enough, else none at all.
                if b.rect.h >= a + d + 10.0 && b.rect.w >= tw + 4.0 {
                    let fill = model.color(&model.series[b.series], theme);
                    placed.push((
                        b.band,
                        text,
                        cx,
                        b.rect.y + 5.0 + a,
                        Anchor::Middle,
                        on_fill(fill),
                    ));
                    continue;
                }
                return none;
            }
            placed.push((b.band, text, cx, y, Anchor::Middle, theme.muted));
        }
    }
    placed
        .into_iter()
        .map(|(band, text, x, y, anchor, color)| {
            let r = super::layout::text_box(&text, x, y, spec, anchor, m);
            let item = Item::Text {
                text: text.into(),
                x,
                y,
                spec,
                anchor,
                color,
                alpha: 1.0,
                max_width: None,
            };
            (band, item, r)
        })
        .collect()
}

/// Text on a filled mark: white, or near black on a light fill (ruling 18).
pub fn on_fill(fill: Color) -> Color {
    if crate::theme::is_dark(fill) || luma(fill) < 0.62 {
        Color(255, 255, 255)
    } else {
        Color(17, 17, 17)
    }
}

fn luma(c: Color) -> f32 {
    (0.299 * f32::from(c.0) + 0.587 * f32::from(c.1) + 0.114 * f32::from(c.2)) / 255.0
}

/// The hit index: each visible continuous series by x, each visible point by its place.
fn index(model: &ChartModel, view: &View, l: &ChartLayout, hits: &mut HitIndex) {
    let sizes = size_extent(model);
    for key in model.keys() {
        if !model.visible(view, &key) {
            continue;
        }
        let Some(si) = model.index_of(&key) else {
            continue;
        };
        let s = &model.series[si];
        match s.mark {
            MarkKind::Point => {
                for i in 0..s.xs.len() {
                    if s.ys[i].is_nan() || s.xs[i].is_nan() {
                        continue;
                    }
                    let p = l.at(s.xs[i], s.ys[i]);
                    let r = point_radius(model, s, i, sizes);
                    if l.clip.inflate(r).contains(p.0, p.1) {
                        hits.add_point(key.clone(), i as u32, p.0, p.1);
                    }
                }
            }
            MarkKind::Rect => {}
            _ if model.is_band() => {}
            _ => match &s.pyramid {
                Some(p) => hits.add_series_with(key.clone(), s.xs.clone(), s.ys.clone(), p),
                None => hits.add_series(key.clone(), s.xs.clone(), s.ys.clone()),
            },
        }
    }
}
