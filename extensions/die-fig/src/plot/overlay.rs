//! The overlay: crosshair, dots, the card, the axes' chips, the brush, the reference and the
//! emphasis wash, drawn over the base and kept close so its damage stays small (ruling 10).
//!
//! Hover is the heart of the mouse (contract-p2 §0): the crosshair snaps to the hovered sample,
//! a dot with a surface ring marks each series' value there, and one card lists them all,
//! values first. The series nearest the pointer comes forward: the rest fade under a wash of
//! the surface over the plot and it's drawn again on top, its dot larger and its card row
//! marked. A bar or point under the pointer lifts. The keyboard cursor looks the same, with an
//! inverted chip naming its x on the axis; while the pointer hovers elsewhere the cursor keeps a
//! dashed rule and its chip, and its card gives way to the hover's (mouse-ux §3.3).
//!
//! Everything is placed in the area's logical px. Nothing here changes the view: hover is
//! purely visual.

use super::hover::{Column, column_at, sample_near};
use super::layout::{BODY, ChartLayout, Role, TICK};
use super::marks::{self, Look, dot, mix};
use super::nav::cursor_data;
use super::{AxisKind, ChartModel};
use crate::data::SeriesKey;
use crate::interact::consts::CARD_GAP;
use crate::interact::{BrushKind, Cursor, View};
use crate::scale::{format_fixed, format_time_point, format_value, format_with};
use crate::scene::{Anchor, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Weight};
use crate::spec::{MarkKind, Stack};
use crate::theme::{Color, Theme};

/// What the overlay shows.
pub struct OverlayState {
    /// What the pointer is over.
    pub hover: Option<Cursor>,
    /// The keyboard's cursor.
    pub cursor: Option<Cursor>,
    /// Where the pointer is, in logical px.
    pub pointer: Option<(f32, f32)>,
    /// A brush being drawn: its kind and corners.
    #[allow(clippy::type_complexity)]
    pub brush: Option<(BrushKind, (f32, f32), (f32, f32))>,
    /// The series emphasised: the one nearest the pointer on a hover, or a legend key's.
    pub emphasis: Option<SeriesKey>,
}

/// How much of the surface the emphasis wash lays over the plot: the rest show at about 35 %.
pub const WASH: f32 = 0.65;
/// The most series rows a card lists before "+N more" (ruling 4).
pub const CARD_ROWS: usize = 12;
/// A card's corner radius.
const CARD_RADIUS: f32 = 8.0;
/// A card row's height.
const CARD_ROW: f32 = 21.0;
/// A dot's radius on the crosshair, and the emphasised one's.
const DOT: f32 = 4.0;
const DOT_BIG: f32 = 5.5;
/// A chip's height and its padding across.
const CHIP_H: f32 = 19.0;
const CHIP_PAD: f32 = 7.0;
/// How far inside the area's edge the card keeps.
const INSET: f32 = 4.0;

/// The overlay's scene.
pub fn overlay(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    st: &OverlayState,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(layout.size.0, layout.size.1, layout.scale);
    let brushing = st.brush.is_some();
    let hover = if brushing {
        None
    } else {
        st.hover.as_ref().filter(|c| valid(model, view, c))
    };
    let cursor = st.cursor.as_ref().filter(|c| valid(model, view, c));
    let emphasis = st
        .emphasis
        .as_ref()
        .filter(|k| model.visible(view, k) && model.series_of(k).is_some());
    let visible = model
        .keys()
        .into_iter()
        .filter(|k| model.visible(view, k))
        .count();
    if let Some(k) = emphasis
        && visible > 1
    {
        wash(model, view, layout, theme, &mut scene, k, st.pointer, m);
    }
    // The one the crosshair, dots and card follow: the hover, else the cursor.
    let target = hover.or(if brushing { None } else { cursor });
    if let Some(r) = view.reference.as_ref().filter(|c| valid(model, view, c)) {
        reference(model, view, layout, theme, &mut scene, r, target, m);
    }
    if let (Some(c), Some(h)) = (cursor, hover)
        && c != h
    {
        marker(model, view, layout, theme, &mut scene, c, false, m);
    }
    if let Some(t) = target {
        let mark = match (hover.is_some(), t) {
            (true, Cursor::Band { series, .. }) => series.clone().or_else(|| emphasis.cloned()),
            (true, Cursor::Point { series, .. }) if model.is_scatter() => Some(series.clone()),
            (true, _) => emphasis.cloned(),
            (false, Cursor::Point { series, .. }) => Some(series.clone()),
            (false, Cursor::Band { series, .. }) => series.clone(),
            _ => None,
        };
        let anchor_y = if hover.is_some() {
            st.pointer.map(|p| p.1)
        } else {
            None
        };
        moment(
            model,
            view,
            layout,
            theme,
            &mut scene,
            t,
            mark.as_ref(),
            hover.is_none(),
            anchor_y,
            m,
        );
    } else if let Some(c) = cursor.filter(|_| brushing) {
        marker(model, view, layout, theme, &mut scene, c, false, m);
    }
    if let Some((kind, from, to)) = st.brush {
        brush(model, layout, theme, &mut scene, kind, from, to, m);
    }
    scene
}

/// Whether a cursor names something the model has and shows.
fn valid(model: &ChartModel, view: &View, c: &Cursor) -> bool {
    match c {
        Cursor::Point { series, row } => model.index_of(series).is_some_and(|si| {
            (*row as usize) < model.series[si].xs.len() && model.visible(view, series)
        }),
        Cursor::Band { band, .. } => (*band as usize) < model.x.bands.len().max(1),
        Cursor::Cell { .. } => model.is_heatmap(),
        Cursor::Node(_) => false,
    }
}

/// The surface over the plot, so every series but `key` fades, and `key` drawn again on top.
#[allow(clippy::too_many_arguments)]
fn wash(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    key: &SeriesKey,
    pointer: Option<(f32, f32)>,
    m: &mut dyn Measure,
) {
    scene.push(Item::Path {
        path: PathData::rect(l.clip),
        fill: Some(Paint::solid(theme.surface, WASH)),
        stroke: None,
    });
    // The other series' end labels fade with them.
    for e in l.ends.iter().filter(|e| &e.key != key) {
        let r = e.name.bounds(m).union(&e.value.bounds(m)).inflate(1.0);
        scene.push(Item::Path {
            path: PathData::rect(r),
            fill: Some(Paint::solid(theme.surface, WASH)),
            stroke: None,
        });
    }
    scene.push(Item::PushClip(l.clip));
    for (si, s) in model.series.iter().enumerate() {
        if &s.key == key && model.visible(view, &s.key) {
            marks::series(model, l, theme, scene, si, Look::Emphasis);
        }
    }
    scene.push(Item::PopClip);
    // A legend key under the pointer is underlined: the affordance, with no pointer shape.
    if let Some((px, py)) = pointer
        && let Some(item) = l.items.iter().find(|i| &i.key == key)
        && l.legend.iter().any(|(k, r)| k == key && r.contains(px, py))
    {
        let b = item.name.bounds(m);
        let y = item.name.y + 3.0;
        scene.push(Item::Path {
            path: PathData::polyline(&[(b.x, y), (b.right(), y)]),
            fill: None,
            stroke: Some(Stroke::hairline(theme.text, 0.7)),
        });
    }
}

/// A target's x on screen along the independent axis, and its column of samples on an x-chart.
fn column_of(model: &ChartModel, view: &View, c: &Cursor) -> Option<Column> {
    let (x, _) = cursor_data(model, c)?;
    match c {
        Cursor::Point { .. } if !model.is_scatter() && !model.is_heatmap() && !model.is_band() => {
            Some(column_at(model, None, view, x))
        }
        _ => None,
    }
}

/// A target's crosshair, dots, lift, chips and card.
#[allow(clippy::too_many_arguments)]
fn moment(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    t: &Cursor,
    mark: Option<&SeriesKey>,
    keyboard: bool,
    anchor_y: Option<f32>,
    m: &mut dyn Measure,
) {
    let p = l.plot;
    match t {
        Cursor::Band { band, .. } => {
            let Some(slot) = band_slot(model, l, *band) else {
                return;
            };
            // A faint column behind the band, then its bars lifted.
            let col = if l.horizontal {
                Rect::new(p.x - 6.0, slot.y - 2.0, p.w + 6.0, slot.h + 4.0)
            } else {
                Rect::new(slot.x - 2.0, p.y - 6.0, slot.w + 4.0, p.h + 6.0)
            };
            let col = col.intersect(&l.clip.inflate(6.0)).unwrap_or(col);
            scene.push(Item::Path {
                path: PathData::rounded(col, [6.0, 6.0, 0.0, 0.0]),
                fill: Some(Paint::solid(
                    theme.text,
                    if theme.dark { 0.035 } else { 0.05 },
                )),
                stroke: None,
            });
            scene.push(Item::PushClip(l.clip));
            for b in l.bars.iter().filter(|b| b.band == *band) {
                let c = model.color(&model.series[b.series], theme);
                marks::bar(scene, b, lift(c, theme));
            }
            scene.push(Item::PopClip);
            bright_tick(l, theme, scene, f64::from(*band) + 0.5);
            if keyboard {
                let name = model
                    .x
                    .bands
                    .get(*band as usize)
                    .map_or(String::new(), |b| b.to_string());
                chip_x(l, theme, scene, l.ix(f64::from(*band)), &name, true, m);
            }
            let rows = band_rows(model, view, l, *band, mark);
            let header = model.x_label(f64::from(*band), 0.0);
            let total = stacked_total(model, view, l, &rows);
            let delta = view.reference.as_ref().and_then(|r| cursor_data(model, r));
            let card = Card::new(
                model,
                view,
                header,
                rows,
                total,
                delta.map(|(x, _)| x),
                f64::from(*band) + 0.5,
                theme,
                m,
            );
            let (left, right) = if l.horizontal {
                (slot.right(), slot.right())
            } else {
                (slot.x - 2.0, slot.right() + 2.0)
            };
            let ay = anchor_y.unwrap_or(if l.horizontal { slot.y } else { p.y + 4.0 });
            card.place_beside(l, scene, left, right, ay, theme, m);
        }
        Cursor::Cell { row, col } => {
            let Some(si) = model.series.iter().position(|s| s.mark == MarkKind::Rect) else {
                return;
            };
            let s = &model.series[si];
            let Some(i) =
                (0..s.xs.len()).find(|&i| s.xs[i] as u32 == *col && s.ys[i] as u32 == *row)
            else {
                return;
            };
            let Some(r) = marks::cell_rect(l, s, i) else {
                return;
            };
            scene.push(Item::Path {
                path: PathData::rounded(r.inflate(1.0), [3.0; 4]),
                fill: None,
                stroke: Some(Stroke::solid(1.5, theme.strong, 0.85)),
            });
            let x = model
                .x
                .bands
                .get(*col as usize)
                .map_or(String::new(), |b| b.to_string());
            let y = model
                .y
                .bands
                .get(*row as usize)
                .map_or(String::new(), |b| b.to_string());
            let v = s.sizes.as_ref().map_or(f64::NAN, |v| v[i]);
            let rows = vec![Row {
                key: s.key.clone(),
                color: model.color(s, theme),
                shape: KeyShape::Square,
                name: s.name.clone(),
                value: format_fixed(v, s.decimals),
                raw: v,
                decimals: s.decimals,
                marked: false,
                missing: v.is_nan(),
            }];
            let card = Card::new(
                model,
                view,
                format!("{x} · {y}"),
                rows,
                None,
                None,
                0.0,
                theme,
                m,
            );
            card.place_point(
                l,
                scene,
                (r.x + r.w / 2.0, r.y + r.h / 2.0),
                r.w.max(r.h) / 2.0,
                theme,
                m,
            );
        }
        Cursor::Point { series, row } if model.is_scatter() => {
            let Some(si) = model.index_of(series) else {
                return;
            };
            let s = &model.series[si];
            let i = *row as usize;
            let pt = l.at(s.xs[i], s.ys[i]);
            let r = marks::point_radius(model, s, i, marks::size_extent(model));
            // Guide ticks on both axes, then the point lifted: its ring in the foreground.
            hair(
                scene,
                (pt.0, p.bottom() - 6.0),
                (pt.0, p.bottom()),
                theme.text,
                0.6,
            );
            hair(scene, (p.x, pt.1), (p.x + 6.0, pt.1), theme.text, 0.6);
            let c = model.color(s, theme);
            dot(scene, pt, r, lift(c, theme), theme.strong, 2.0);
            let xl = model.x_value(s.xs[i]);
            chip_x(l, theme, scene, pt.0, &xl, keyboard, m);
            chip_y(l, theme, scene, pt.1, &model.y_label(s.ys[i]), keyboard, m);
            let rows = point_rows(model, view, si, i, theme);
            let mut header = s.name.clone();
            if let Some(ref_xy) = view.reference.as_ref().and_then(|r| cursor_data(model, r)) {
                let d = (s.xs[i] - ref_xy.0, s.ys[i] - ref_xy.1);
                header = format!(
                    "{header} · Δ {}, {}",
                    signed(d.0, s.decimals.max(1)),
                    signed(d.1, s.decimals.max(1))
                );
            }
            let card = Card::new(model, view, header, rows, None, None, 0.0, theme, m);
            card.place_point(l, scene, pt, r + 2.0, theme, m);
        }
        Cursor::Point { .. } => {
            let Some(col) = column_of(model, view, t) else {
                return;
            };
            let x = l.ix(col.x);
            if !x.is_finite() || x < p.x - 1.0 || x > p.right() + 1.0 {
                return;
            }
            crosshair(l, theme, scene, x, false);
            // Bars at the x lift; every other series gets a dot.
            scene.push(Item::PushClip(l.clip.inflate(DOT_BIG + 2.0)));
            for b in l.bars.iter() {
                if col
                    .samples
                    .iter()
                    .any(|&(si, i)| i == Some(b.sample) && si == b.series)
                    || (model.series[b.series].xs[b.sample] - col.x).abs() < 1e-9
                {
                    let c = model.color(&model.series[b.series], theme);
                    marks::bar(scene, b, lift(c, theme));
                }
            }
            for &(si, i) in &col.samples {
                let (Some(i), s) = (i, &model.series[si]) else {
                    continue;
                };
                if s.mark == MarkKind::Bar {
                    continue;
                }
                let v = model.top(&l.stacks, si, i);
                let pt = l.at(s.xs[i], v);
                if !pt.1.is_finite() {
                    continue;
                }
                let big = mark == Some(&s.key);
                dot(
                    scene,
                    (x, pt.1),
                    if big { DOT_BIG } else { DOT },
                    model.color(s, theme),
                    theme.surface,
                    2.0,
                );
            }
            scene.push(Item::PopClip);
            if keyboard {
                let step = interval(model, &col);
                chip_x(
                    l,
                    theme,
                    scene,
                    x,
                    &x_chip_label(model, col.x, step),
                    true,
                    m,
                );
            }
            let rows = column_rows(model, view, l, &col, mark, theme);
            let header = model.x_label(col.x, interval(model, &col));
            let total = stacked_total(model, view, l, &rows);
            let delta = view
                .reference
                .as_ref()
                .and_then(|r| cursor_data(model, r))
                .map(|(x, _)| x);
            let card = Card::new(model, view, header, rows, total, delta, col.x, theme, m);
            let ay = anchor_y.unwrap_or(p.y + 4.0 - CARD_GAP);
            card.place_beside(l, scene, x, x, ay, theme, m);
        }
        Cursor::Node(_) => {}
    }
}

/// The cursor's marker while the hover is elsewhere: a dashed rule and its inverted chip.
#[allow(clippy::too_many_arguments)]
fn marker(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    c: &Cursor,
    _solid: bool,
    m: &mut dyn Measure,
) {
    let p = l.plot;
    match c {
        Cursor::Point { series, row } if model.is_scatter() => {
            let Some(si) = model.index_of(series) else {
                return;
            };
            let s = &model.series[si];
            let i = *row as usize;
            let pt = l.at(s.xs[i], s.ys[i]);
            let r = marks::point_radius(model, s, i, marks::size_extent(model)) + 4.0;
            scene.push(Item::Path {
                path: PathData::circle(pt.0, pt.1, r),
                fill: None,
                stroke: Some(Stroke {
                    dash: Some((3.0, 2.0)),
                    ..Stroke::solid(1.0, theme.text, 0.7)
                }),
            });
        }
        Cursor::Point { .. } => {
            let Some(col) = column_of(model, view, c) else {
                return;
            };
            let x = l.ix(col.x);
            if x < p.x - 1.0 || x > p.right() + 1.0 {
                return;
            }
            crosshair(l, theme, scene, x, true);
            chip_x(
                l,
                theme,
                scene,
                x,
                &x_chip_label(model, col.x, interval(model, &col)),
                true,
                m,
            );
        }
        Cursor::Band { band, .. } => {
            let Some(slot) = band_slot(model, l, *band) else {
                return;
            };
            let r = slot.inflate(3.0);
            scene.push(Item::Path {
                path: PathData::rounded(r, [4.0; 4]),
                fill: None,
                stroke: Some(Stroke {
                    dash: Some((3.0, 2.0)),
                    ..Stroke::solid(1.0, theme.text, 0.6)
                }),
            });
            let name = model
                .x
                .bands
                .get(*band as usize)
                .map_or(String::new(), |b| b.to_string());
            if !l.horizontal {
                chip_x(l, theme, scene, slot.x + slot.w / 2.0, &name, true, m);
            }
        }
        _ => {}
    }
}

/// The step a column's x is written at: its series' interval.
fn interval(model: &ChartModel, col: &Column) -> f64 {
    col.samples
        .iter()
        .map(|&(si, _)| model.series[si].interval)
        .filter(|&i| i > 0.0)
        .fold(f64::INFINITY, f64::min)
        .min(f64::MAX)
}

/// The chip's text for an x: a time at its step's precision, else the card's.
fn x_chip_label(model: &ChartModel, x: f64, step: f64) -> String {
    match (model.x.kind, &model.x.format) {
        (_, Some(f)) => format_with(x, f, &model.zone),
        (AxisKind::Time, None) => format_time_point(
            x,
            if step.is_finite() { step } else { f64::NAN },
            &model.zone,
        ),
        _ => model.x_value(x),
    }
}

/// The crosshair: a hairline up the plot, dashed for a cursor the hover has left behind.
fn crosshair(l: &ChartLayout, theme: &Theme, scene: &mut Scene, x: f32, dashed: bool) {
    let p = l.plot;
    scene.push(Item::Path {
        path: PathData::polyline(&[(x, p.y - 4.0), (x, p.bottom())]),
        fill: None,
        stroke: Some(Stroke {
            dash: dashed.then_some((3.0, 3.0)),
            ..Stroke::hairline(theme.muted, if dashed { 0.55 } else { 0.7 })
        }),
    });
}

fn hair(scene: &mut Scene, a: (f32, f32), b: (f32, f32), color: Color, alpha: f32) {
    scene.push(Item::Path {
        path: PathData::polyline(&[a, b]),
        fill: None,
        stroke: Some(Stroke::hairline(color, alpha)),
    });
}

/// A colour lifted: lighter on a dark theme, a shade deeper on a light one.
pub fn lift(c: Color, theme: &Theme) -> Color {
    if theme.dark {
        mix(c, Color(255, 255, 255), 0.16)
    } else {
        mix(c, Color(0, 0, 0), 0.1)
    }
}

/// A band's slot: its bars' extent across, or the band's own where it has none.
fn band_slot(model: &ChartModel, l: &ChartLayout, band: u32) -> Option<Rect> {
    let mut r: Option<Rect> = None;
    for b in l.bars.iter().filter(|b| b.band == band) {
        r = Some(r.map_or(b.rect, |o| o.union(&b.rect)));
    }
    if let Some(r) = r {
        // The slot runs the plot's length along the values.
        return Some(if l.horizontal {
            Rect::new(l.plot.x, r.y, l.plot.w, r.h)
        } else {
            Rect::new(r.x, l.plot.y, r.w, l.plot.h)
        });
    }
    let (a, w) = l.ind().band_of(band as usize)?;
    let _ = model;
    Some(if l.horizontal {
        Rect::new(l.plot.x, a, l.plot.w, w)
    } else {
        Rect::new(a, l.plot.y, w, l.plot.h)
    })
}

/// The tick label at a band brightened: the hovered category reads as the target.
fn bright_tick(l: &ChartLayout, theme: &Theme, scene: &mut Scene, at: f64) {
    let px = l.ix(at - 0.5);
    let role = if l.horizontal {
        Role::YTick
    } else {
        Role::XTick
    };
    if let Some(label) = l.labels.iter().filter(|lb| lb.role == role).min_by(|a, b| {
        let da = if l.horizontal {
            (a.y - px).abs()
        } else {
            (a.x - px).abs()
        };
        let db = if l.horizontal {
            (b.y - px).abs()
        } else {
            (b.x - px).abs()
        };
        da.total_cmp(&db)
    }) {
        let near = if l.horizontal {
            (label.y - px).abs() < 16.0
        } else {
            (label.x - px).abs() < 2.0
        };
        if near {
            let mut item = label.item(theme);
            if let Item::Text { color, .. } = &mut item {
                *color = theme.strong;
            }
            scene.push(item);
        }
    }
}

/// A chip on the x axis: inverted for the keyboard's cursor, outlined for a hover. It hides
/// the tick labels it would collide with under the surface's colour.
fn chip_x(
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    x: f32,
    text: &str,
    inverted: bool,
    m: &mut dyn Measure,
) {
    if text.is_empty() || !l.x_axis.h.is_finite() || l.x_axis.h < CHIP_H {
        return;
    }
    let spec = TextSpec::new(TICK).weight(Weight::Semibold).tabular();
    let w = m.width(text, spec) + 2.0 * CHIP_PAD;
    let left = (x - w / 2.0).clamp(INSET, (l.size.0 - INSET - w).max(INSET));
    let r = Rect::new(left, l.plot.bottom() + 5.0, w, CHIP_H);
    cover(l, theme, scene, r, Role::XTick, m);
    chip(theme, scene, r, text, spec, inverted, m);
}

/// A chip on the y axis, right-aligned against the plot.
fn chip_y(
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    y: f32,
    text: &str,
    inverted: bool,
    m: &mut dyn Measure,
) {
    let spec = TextSpec::new(TICK).weight(Weight::Semibold).tabular();
    let w = m.width(text, spec) + 2.0 * CHIP_PAD;
    let right = l.plot.x - 4.0;
    let left = (right - w).max(INSET);
    let top = (y - CHIP_H / 2.0).clamp(INSET, l.size.1 - INSET - CHIP_H);
    let r = Rect::new(left, top, right - left, CHIP_H);
    cover(l, theme, scene, r, Role::YTick, m);
    chip(theme, scene, r, text, spec, inverted, m);
}

/// The surface over the tick labels of `role` that `r` would collide with.
fn cover(
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    r: Rect,
    role: Role,
    m: &mut dyn Measure,
) {
    let clear = r.inflate(4.0);
    for label in l.labels.iter().filter(|lb| lb.role == role) {
        let b = label.bounds(m).inflate(1.0);
        if b.intersect(&clear).is_some() {
            scene.push(Item::Path {
                path: PathData::rect(b),
                fill: Some(Paint::solid(theme.surface, 1.0)),
                stroke: None,
            });
        }
    }
}

fn chip(
    theme: &Theme,
    scene: &mut Scene,
    r: Rect,
    text: &str,
    spec: TextSpec,
    inverted: bool,
    m: &mut dyn Measure,
) {
    let (fill, ink, ring) = if inverted {
        (theme.chip, theme.chip_text, None)
    } else {
        (
            theme.tooltip,
            theme.strong,
            Some(Stroke::hairline(theme.ring, 1.0)),
        )
    };
    scene.push(Item::Path {
        path: PathData::rounded(r, [4.0; 4]),
        fill: Some(Paint::solid(fill, 1.0)),
        stroke: ring,
    });
    scene.push(Item::Text {
        text: text.into(),
        x: r.x + r.w / 2.0,
        y: r.y + r.h / 2.0 + m.cap(spec) / 2.0,
        spec,
        anchor: Anchor::Middle,
        color: ink,
        alpha: 1.0,
        max_width: None,
    });
}

/// The reference: a dashed rule at 40 % with an outlined chip `A` on the x axis (a dashed ring
/// for a point on a scatter), and the measure band from it to the target with Δx at its top.
#[allow(clippy::too_many_arguments)]
fn reference(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    r: &Cursor,
    target: Option<&Cursor>,
    m: &mut dyn Measure,
) {
    let p = l.plot;
    let Some((rx, ry)) = cursor_data(model, r) else {
        return;
    };
    if model.is_scatter() {
        let pt = l.at(rx, ry);
        scene.push(Item::Path {
            path: PathData::circle(pt.0, pt.1, 9.0),
            fill: None,
            stroke: Some(Stroke {
                dash: Some((3.0, 2.0)),
                ..Stroke::solid(1.0, theme.text, 0.6)
            }),
        });
        return;
    }
    let x = if model.is_band() {
        l.ix(rx - 0.5)
    } else {
        l.ix(rx)
    };
    if let Some((tx, _)) = target.and_then(|t| cursor_data(model, t)) {
        let t = if model.is_band() {
            l.ix(tx - 0.5)
        } else {
            l.ix(tx)
        };
        let (a, b) = (x.min(t).max(p.x), x.max(t).min(p.right()));
        if b - a >= 1.0 {
            scene.push(Item::Path {
                path: PathData::rect(Rect::new(a, p.y, b - a, p.h)),
                fill: Some(Paint::solid(theme.text, 0.06)),
                stroke: None,
            });
            let spec = TextSpec::new(TICK).tabular();
            let text = delta_x(model, rx, tx);
            let w = m.width(&text, spec);
            if !text.is_empty() && w + 10.0 <= b - a {
                scene.push(Item::Text {
                    text: text.into(),
                    x: (a + b) / 2.0,
                    y: p.y + 6.0 + m.ascent(spec),
                    spec,
                    anchor: Anchor::Middle,
                    color: theme.text,
                    alpha: 1.0,
                    max_width: None,
                });
            }
        }
    }
    if x < p.x - 1.0 || x > p.right() + 1.0 {
        return;
    }
    scene.push(Item::Path {
        path: PathData::polyline(&[(x, p.y), (x, p.bottom())]),
        fill: None,
        stroke: Some(Stroke {
            dash: Some((4.0, 3.0)),
            ..Stroke::hairline(theme.text, 0.4)
        }),
    });
    chip_x(l, theme, scene, x, "A", false, m);
    let _ = view;
}

/// Δx from the reference to `x`: "+35m" on a time axis, "Δ 12.5" on a number axis, the bands
/// between on a band axis.
fn delta_x(model: &ChartModel, from: f64, to: f64) -> String {
    let d = to - from;
    match model.x.kind {
        AxisKind::Time => {
            let sign = if d < 0.0 { "\u{2212}" } else { "+" };
            format!("{sign}{}", duration(d.abs()))
        }
        AxisKind::Band => {
            let n = d.round() as i64;
            if n == 0 {
                String::new()
            } else {
                format!("{}{} bands", if n > 0 { "+" } else { "\u{2212}" }, n.abs())
            }
        }
        _ => format!("Δ {}", format_value(d)),
    }
}

/// A duration as the brush and the reference write it: "35m", "1h 5m", "2d 3h", "45s".
pub fn duration(secs: f64) -> String {
    if !secs.is_finite() {
        return String::new();
    }
    let s = secs.round() as i64;
    if secs < 1.0 && secs > 0.0 {
        return format!("{}ms", (secs * 1000.0).round());
    }
    let (d, h, mi, se) = (s / 86_400, (s / 3600) % 24, (s / 60) % 60, s % 60);
    match (d, h, mi, se) {
        (0, 0, 0, se) => format!("{se}s"),
        (0, 0, mi, 0) => format!("{mi}m"),
        (0, 0, mi, se) => format!("{mi}m {se}s"),
        (0, h, 0, _) => format!("{h}h"),
        (0, h, mi, _) => format!("{h}h {mi}m"),
        (d, 0, _, _) => format!("{d}d"),
        (d, h, _, _) => format!("{d}d {h}h"),
    }
}

/// A brush's preview: a band (or box) in the foreground at 8 % with 1 px edges at 40 %, and its
/// readout chip at its top.
#[allow(clippy::too_many_arguments)]
fn brush(
    model: &ChartModel,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    kind: BrushKind,
    from: (f32, f32),
    to: (f32, f32),
    m: &mut dyn Measure,
) {
    let p = l.plot;
    let cx = |x: f32| x.clamp(p.x, p.right());
    let cy = |y: f32| y.clamp(p.y, p.bottom());
    let (mut x0, mut x1) = (cx(from.0).min(cx(to.0)), cx(from.0).max(cx(to.0)));
    let (mut y0, mut y1) = (cy(from.1).min(cy(to.1)), cy(from.1).max(cy(to.1)));
    match kind {
        BrushKind::X => {
            y0 = p.y;
            y1 = p.bottom();
        }
        BrushKind::Y => {
            x0 = p.x;
            x1 = p.right();
        }
        BrushKind::Bands => {
            // Snapped to whole bands as it goes.
            let (a, b) = if l.horizontal { (y0, y1) } else { (x0, x1) };
            let (da, db) = (l.ind().invert(a).floor(), l.ind().invert(b).ceil());
            let (sa, sb) = (l.ind().map(da), l.ind().map(db));
            if l.horizontal {
                (y0, y1) = (sa.min(sb).max(p.y), sa.max(sb).min(p.bottom()));
                (x0, x1) = (p.x, p.right());
            } else {
                (x0, x1) = (sa.min(sb).max(p.x), sa.max(sb).min(p.right()));
                (y0, y1) = (p.y, p.bottom());
            }
        }
        BrushKind::Box => {}
    }
    let r = Rect::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0));
    scene.push(Item::Path {
        path: PathData::rect(r),
        fill: Some(Paint::solid(theme.text, 0.08)),
        stroke: None,
    });
    let edges: Vec<[(f32, f32); 2]> = match kind {
        BrushKind::X => vec![[(x0, y0), (x0, y1)], [(x1, y0), (x1, y1)]],
        BrushKind::Bands if !l.horizontal => vec![[(x0, y0), (x0, y1)], [(x1, y0), (x1, y1)]],
        BrushKind::Y => vec![[(x0, y0), (x1, y0)], [(x0, y1), (x1, y1)]],
        BrushKind::Bands => vec![[(x0, y0), (x1, y0)], [(x0, y1), (x1, y1)]],
        BrushKind::Box => vec![
            [(x0, y0), (x1, y0)],
            [(x1, y0), (x1, y1)],
            [(x1, y1), (x0, y1)],
            [(x0, y1), (x0, y0)],
        ],
    };
    for [a, b] in edges {
        hair(scene, a, b, theme.text, 0.4);
    }
    // The readout: the span selected.
    let text = readout(model, l, kind, (x0, y0), (x1, y1));
    if text.is_empty() {
        return;
    }
    let spec = TextSpec::new(TICK).tabular();
    let w = m.width(&text, spec) + 2.0 * CHIP_PAD;
    let left = ((x0 + x1) / 2.0 - w / 2.0).clamp(INSET, (l.size.0 - INSET - w).max(INSET));
    let chip_r = Rect::new(left, y0 + 6.0, w, CHIP_H);
    scene.push(Item::Path {
        path: PathData::rounded(chip_r, [4.0; 4]),
        fill: Some(Paint::solid(theme.tooltip, 1.0)),
        stroke: Some(Stroke::hairline(theme.ring, 1.0)),
    });
    scene.push(Item::Text {
        text: text.into(),
        x: chip_r.x + chip_r.w / 2.0,
        y: chip_r.y + chip_r.h / 2.0 + m.cap(spec) / 2.0,
        spec,
        anchor: Anchor::Middle,
        color: theme.strong,
        alpha: 1.0,
        max_width: None,
    });
}

/// What a brush selects, written: "06:05 – 06:40 · 35m", "12 – 48", "b – e · 4 bands".
fn readout(
    model: &ChartModel,
    l: &ChartLayout,
    kind: BrushKind,
    a: (f32, f32),
    b: (f32, f32),
) -> String {
    let span_x = |lo: f64, hi: f64| -> String {
        match model.x.kind {
            AxisKind::Time => {
                let step = model.min_interval(&View::default());
                let step = if step.is_finite() && step > 0.0 {
                    step
                } else {
                    (hi - lo) / 100.0
                };
                format!(
                    "{} – {} · {}",
                    format_time_point(lo, step, &model.zone),
                    format_time_point(hi, step, &model.zone),
                    duration(hi - lo)
                )
            }
            AxisKind::Band => {
                let (i, j) = (
                    lo.floor().max(0.0) as usize,
                    (hi.ceil() as usize).saturating_sub(1),
                );
                let name = |k: usize| {
                    model
                        .x
                        .bands
                        .get(k)
                        .map_or(String::new(), |s| s.to_string())
                };
                let n = j + 1 - i.min(j + 1);
                if n <= 1 {
                    name(i)
                } else {
                    format!("{} – {} · {n} bands", name(i), name(j))
                }
            }
            _ => format!("{} – {}", short(lo, hi - lo), short(hi, hi - lo)),
        }
    };
    let span_y = |lo: f64, hi: f64| format!("{} – {}", short(lo, hi - lo), short(hi, hi - lo));
    let (dx0, dx1) = (l.data_x(a.0, a.1), l.data_x(b.0, b.1));
    let (dy0, dy1) = (l.data_y(a.0, a.1), l.data_y(b.0, b.1));
    match kind {
        BrushKind::X | BrushKind::Bands => span_x(dx0.min(dx1), dx0.max(dx1)),
        BrushKind::Y => span_y(dy0.min(dy1), dy0.max(dy1)),
        BrushKind::Box => format!(
            "{}  ×  {}",
            span_x(dx0.min(dx1), dx0.max(dx1)),
            span_y(dy0.min(dy1), dy0.max(dy1))
        ),
    }
}

/// A number to about three significant digits of a span.
fn short(v: f64, span: f64) -> String {
    let step = if span > 0.0 { span / 100.0 } else { 0.0 };
    crate::scale::format_number(v, step, crate::scale::NumberStyle::Full)
}

/// A key's shape in the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyShape {
    Line,
    Square,
    Dot,
}

/// One row of a card.
#[derive(Clone, Debug)]
struct Row {
    key: SeriesKey,
    color: Color,
    shape: KeyShape,
    name: String,
    value: String,
    raw: f64,
    decimals: usize,
    marked: bool,
    missing: bool,
}

fn shape_of(mark: MarkKind) -> KeyShape {
    match mark {
        MarkKind::Line => KeyShape::Line,
        MarkKind::Point => KeyShape::Dot,
        _ => KeyShape::Square,
    }
}

/// A value as the card writes it: the axis's format, else in full at its series' precision.
fn card_value(model: &ChartModel, v: f64, decimals: usize) -> String {
    if v.is_nan() {
        return "—".into();
    }
    if model.y.format.is_some() || model.series.iter().any(|s| s.stack == Stack::Normalize) {
        return model.y_label(v);
    }
    format_fixed(v, decimals)
}

/// The rows of an x-chart's column: unstacked series by value, high first; stacked ones top to
/// bottom as drawn.
fn column_rows(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    col: &Column,
    mark: Option<&SeriesKey>,
    theme: &Theme,
) -> Vec<Row> {
    let mut rows: Vec<(f64, usize, Row)> = Vec::new();
    for &(si, i) in &col.samples {
        let s = &model.series[si];
        let raw = i.map_or(f64::NAN, |i| {
            if model.series.iter().any(|o| o.stack == Stack::Normalize) {
                // A normalised stack's card says each series' share.
                match (&l.stacks.lo[si], &l.stacks.hi[si]) {
                    (Some(lo), Some(hi)) => hi[i] - lo[i],
                    _ => s.ys[i],
                }
            } else {
                s.ys[i]
            }
        });
        let order = i.map_or(f64::NEG_INFINITY, |i| model.top(&l.stacks, si, i));
        rows.push((
            if order.is_nan() {
                f64::NEG_INFINITY
            } else {
                order
            },
            si,
            Row {
                key: s.key.clone(),
                color: model.color(s, theme),
                shape: shape_of(s.mark),
                name: s.name.clone(),
                value: card_value(model, raw, s.decimals),
                raw,
                decimals: s.decimals,
                marked: mark == Some(&s.key),
                missing: i.is_none() || raw.is_nan(),
            },
        ));
    }
    let _ = view;
    rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    rows.into_iter().map(|(_, _, r)| r).collect()
}

/// The rows of a band: every visible series' value there, a stack's top first.
fn band_rows(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    band: u32,
    mark: Option<&SeriesKey>,
) -> Vec<Row> {
    let mut rows: Vec<(f64, usize, Row)> = Vec::new();
    for si in super::hover::walked(model, view) {
        let s = &model.series[si];
        let i = (0..s.xs.len()).find(|&i| s.xs[i] as u32 == band && !s.xs[i].is_nan());
        let raw = i.map_or(f64::NAN, |i| s.ys[i]);
        let order = match (i, &l.stacks.hi[si]) {
            (Some(i), Some(hi)) => hi[i],
            _ => -(si as f64),
        };
        rows.push((
            if order.is_nan() {
                f64::NEG_INFINITY
            } else {
                order
            },
            si,
            Row {
                key: s.key.clone(),
                color: Color::default(),
                shape: shape_of(s.mark),
                name: s.name.clone(),
                value: card_value(model, raw, s.decimals),
                raw,
                decimals: s.decimals,
                marked: mark == Some(&s.key),
                missing: raw.is_nan(),
            },
        ));
    }
    let stacked = l.stacks.hi.iter().any(Option::is_some);
    if stacked {
        rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    rows.into_iter().map(|(_, _, r)| r).collect()
}

/// A point's row on a scatter: its series and its x and y.
fn point_rows(model: &ChartModel, view: &View, si: usize, i: usize, theme: &Theme) -> Vec<Row> {
    let s = &model.series[si];
    let xname = model
        .x
        .title
        .clone()
        .or_else(|| model.x.field.clone())
        .unwrap_or_else(|| "x".into());
    let yname = model
        .y
        .title
        .clone()
        .or_else(|| model.y.field.clone())
        .unwrap_or_else(|| "y".into());
    let mut out = vec![
        Row {
            key: s.key.clone(),
            color: model.color(s, theme),
            shape: KeyShape::Dot,
            name: xname,
            value: model.x_value(s.xs[i]),
            raw: s.xs[i],
            decimals: 0,
            marked: false,
            missing: false,
        },
        Row {
            key: s.key.clone(),
            color: model.color(s, theme),
            shape: KeyShape::Dot,
            name: yname,
            value: card_value(model, s.ys[i], s.decimals),
            raw: s.ys[i],
            decimals: s.decimals,
            marked: false,
            missing: false,
        },
    ];
    if let Some(sz) = &s.sizes {
        out.push(Row {
            key: s.key.clone(),
            color: model.color(s, theme),
            shape: KeyShape::Dot,
            name: "size".into(),
            value: format_value(sz[i]),
            raw: sz[i],
            decimals: 0,
            marked: false,
            missing: sz[i].is_nan(),
        });
    }
    let _ = view;
    // Only the first row wears the key; the rest are the same point.
    for r in out.iter_mut().skip(1) {
        r.color = Color::default();
    }
    out
}

/// The total of a column's stacked rows, when it stacks two or more.
fn stacked_total(model: &ChartModel, view: &View, l: &ChartLayout, rows: &[Row]) -> Option<String> {
    let stacked: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            model
                .index_of(&r.key)
                .is_some_and(|si| l.stacks.hi[si].is_some())
        })
        .collect();
    if stacked.len() < 2 || model.series.iter().any(|s| s.stack == Stack::Normalize) {
        return None;
    }
    let total: f64 = stacked
        .iter()
        .map(|r| r.raw)
        .filter(|v| v.is_finite())
        .sum();
    let dec = stacked.iter().map(|r| r.decimals).max().unwrap_or(0);
    let _ = view;
    Some(card_value(model, total, dec))
}

/// A signed number for a delta: "+38", "−12.5".
fn signed(d: f64, decimals: usize) -> String {
    let s = format_fixed(d.abs(), decimals);
    if d < 0.0 {
        format!("\u{2212}{s}")
    } else {
        format!("+{s}")
    }
}

/// A card: its header, rows and total, measured.
struct Card {
    header: String,
    rows: Vec<Row>,
    more: usize,
    total: Option<String>,
    deltas: Vec<String>,
    val_w: f32,
    delta_w: f32,
    w: f32,
    h: f32,
}

const PAD: f32 = 12.0;
const KEY_W: f32 = 14.0;
const GAP: f32 = 9.0;

impl Card {
    #[allow(clippy::too_many_arguments)]
    fn new(
        model: &ChartModel,
        view: &View,
        header: String,
        mut rows: Vec<Row>,
        total: Option<String>,
        reference_x: Option<f64>,
        x: f64,
        theme: &Theme,
        m: &mut dyn Measure,
    ) -> Card {
        // Colours from the model for the band rows that were made without a theme.
        for r in rows.iter_mut() {
            if r.color == Color::default()
                && let Some(s) = model.series_of(&r.key)
                && r.shape != KeyShape::Dot
            {
                r.color = model.color(s, theme);
            }
        }
        // At most twelve, the marked one among them.
        let mut more = 0;
        if rows.len() > CARD_ROWS {
            more = rows.len() - CARD_ROWS;
            if let Some(k) = rows.iter().position(|r| r.marked)
                && k >= CARD_ROWS
            {
                let r = rows.remove(k);
                rows.insert(CARD_ROWS - 1, r);
            }
            rows.truncate(CARD_ROWS);
        }
        let mut header = header;
        let mut deltas = Vec::new();
        if let Some(rx) = reference_x {
            let dx = delta_x(
                model,
                if model.is_band() { rx - 0.5 } else { rx },
                if model.is_band() { x - 0.5 } else { x },
            );
            if !dx.is_empty() {
                header = format!("{header} · {dx}");
            }
            for r in &rows {
                let Some(si) = model.index_of(&r.key) else {
                    deltas.push(String::new());
                    continue;
                };
                let base = if model.is_band() {
                    let s = &model.series[si];
                    (0..s.xs.len())
                        .find(|&i| (s.xs[i] + 0.5 - rx).abs() < 1e-9)
                        .map_or(f64::NAN, |i| s.ys[i])
                } else {
                    sample_near(model, None, si, rx).map_or(f64::NAN, |i| model.series[si].ys[i])
                };
                deltas.push(delta(r.raw, base, r.decimals));
            }
        }
        let _ = view;
        let vspec = TextSpec::new(13.0).weight(Weight::Semibold).tabular();
        let nspec = TextSpec::new(BODY);
        let dspec = TextSpec::new(BODY).tabular();
        let hspec = TextSpec::new(TICK).tabular();
        let mut val_w: f32 = 0.0;
        let mut name_w: f32 = 0.0;
        for r in &rows {
            val_w = val_w.max(m.width(&r.value, vspec));
            name_w = name_w.max(m.width(&r.name, nspec));
        }
        if let Some(t) = &total {
            val_w = val_w.max(m.width(t, vspec));
            name_w = name_w.max(m.width("total", nspec));
        }
        if more > 0 {
            name_w = name_w.max(m.width(&format!("+{more} more"), nspec));
        }
        let delta_w = deltas.iter().map(|d| m.width(d, dspec)).fold(0.0, f32::max);
        let name_w = name_w.min(260.0);
        let body_w = PAD
            + KEY_W
            + GAP
            + val_w
            + if delta_w > 0.0 { GAP + delta_w } else { 0.0 }
            + GAP
            + name_w
            + 14.0;
        let w = body_w.max(m.width(&header, hspec) + 2.0 * PAD).ceil();
        let n = rows.len() + usize::from(more > 0);
        let h = 12.0
            + 13.0
            + 7.0
            + n as f32 * CARD_ROW
            + if total.is_some() { 9.0 + CARD_ROW } else { 0.0 }
            + 7.0;
        Card {
            header,
            rows,
            more,
            total,
            deltas,
            val_w,
            delta_w,
            w,
            h,
        }
    }

    /// Beside a crosshair or a band: right of `right` (or left of `left` once it would pass the
    /// area's edge, back only with a cell to spare), its top `CARD_GAP` below `ay`, slid up to
    /// fit the area.
    #[allow(clippy::too_many_arguments)]
    fn place_beside(
        &self,
        l: &ChartLayout,
        scene: &mut Scene,
        left: f32,
        right: f32,
        ay: f32,
        theme: &Theme,
        m: &mut dyn Measure,
    ) {
        let (aw, ah) = l.size;
        // The plot's right edge, so the card leaves the end labels be, unless only past it does
        // the card fit.
        let limit = (l.plot.right() + 8.0).min(aw - INSET);
        let fits_right = |spare: f32| right + CARD_GAP + self.w + spare <= limit;
        let flipped = if l.flipped.get() {
            !fits_right(l.cell_width)
        } else {
            !fits_right(0.0)
        };
        let x = if flipped && left - CARD_GAP - self.w >= INSET {
            left - CARD_GAP - self.w
        } else if flipped {
            // Neither side has room: the side with more, clamped.
            if left > aw - right {
                INSET
            } else {
                (aw - INSET - self.w).max(INSET)
            }
        } else {
            right + CARD_GAP
        };
        l.flipped.set(flipped);
        // Over the plot, not the axis under it, where it fits.
        let floor = if self.h <= l.plot.bottom() - INSET {
            l.plot.bottom()
        } else {
            ah - INSET
        };
        let y = (ay + CARD_GAP).min(floor - self.h).max(INSET);
        self.draw(
            scene,
            Rect::new(x.round(), y.round(), self.w, self.h),
            theme,
            m,
        );
    }

    /// Beside a point, never over it: right-below, right-above, left-below, then left-above.
    fn place_point(
        &self,
        l: &ChartLayout,
        scene: &mut Scene,
        p: (f32, f32),
        r: f32,
        theme: &Theme,
        m: &mut dyn Measure,
    ) {
        let (aw, ah) = l.size;
        let g = CARD_GAP + r;
        let tries = [
            (p.0 + g, p.1 + g),
            (p.0 + g, p.1 - g - self.h),
            (p.0 - g - self.w, p.1 + g),
            (p.0 - g - self.w, p.1 - g - self.h),
        ];
        let fits = |(x, y): (f32, f32)| {
            x >= INSET && y >= INSET && x + self.w <= aw - INSET && y + self.h <= ah - INSET
        };
        let (x, y) = tries.iter().copied().find(|&t| fits(t)).unwrap_or_else(|| {
            let (x, y) = tries[0];
            (
                x.min(aw - INSET - self.w).max(INSET),
                y.min(ah - INSET - self.h).max(INSET),
            )
        });
        self.draw(
            scene,
            Rect::new(x.round(), y.round(), self.w, self.h),
            theme,
            m,
        );
    }

    fn draw(&self, scene: &mut Scene, r: Rect, theme: &Theme, m: &mut dyn Measure) {
        scene.push(Item::Shadow {
            rect: r,
            radius: CARD_RADIUS,
            blur: 8.0,
            dy: 6.0,
            alpha: if theme.dark { 0.55 } else { 0.16 },
        });
        scene.push(Item::Path {
            path: PathData::rounded(r, [CARD_RADIUS; 4]),
            fill: Some(Paint::solid(theme.tooltip, 1.0)),
            stroke: Some(Stroke::hairline(theme.ring, 1.0)),
        });
        let hspec = TextSpec::new(TICK).tabular();
        scene.push(Item::Text {
            text: self.header.as_str().into(),
            x: r.x + PAD,
            y: r.y + 22.0,
            spec: hspec,
            anchor: Anchor::Start,
            color: theme.muted,
            alpha: 1.0,
            max_width: Some(r.w - 2.0 * PAD),
        });
        let vspec = TextSpec::new(13.0).weight(Weight::Semibold).tabular();
        let nspec = TextSpec::new(BODY);
        let dspec = TextSpec::new(BODY).tabular();
        let vx = r.x + PAD + KEY_W + GAP + self.val_w;
        let dx = vx + GAP;
        let nx = if self.delta_w > 0.0 {
            dx + self.delta_w + GAP
        } else {
            vx + GAP
        };
        let name_max = (r.right() - 14.0 - nx).max(0.0);
        let hi = mix(
            theme.tooltip,
            theme.strong,
            if theme.dark { 0.055 } else { 0.045 },
        );
        for (k, row) in self.rows.iter().enumerate() {
            let y0 = r.y + 32.0 + k as f32 * CARD_ROW;
            if row.marked {
                scene.push(Item::Path {
                    path: PathData::rounded(Rect::new(r.x + 5.0, y0, r.w - 10.0, 20.0), [4.0; 4]),
                    fill: Some(Paint::solid(hi, 1.0)),
                    stroke: None,
                });
            }
            let mid = y0 + 10.0;
            if row.color != Color::default() {
                key(scene, row.shape, r.x + PAD, mid, row.color);
            }
            let base = y0 + 14.5;
            scene.push(Item::Text {
                text: row.value.as_str().into(),
                x: vx,
                y: base,
                spec: vspec,
                anchor: Anchor::End,
                color: if row.missing {
                    theme.muted
                } else {
                    theme.strong
                },
                alpha: 1.0,
                max_width: None,
            });
            if let Some(d) = self.deltas.get(k).filter(|d| !d.is_empty()) {
                scene.push(Item::Text {
                    text: d.as_str().into(),
                    x: dx,
                    y: base,
                    spec: dspec,
                    anchor: Anchor::Start,
                    color: theme.text,
                    alpha: 1.0,
                    max_width: None,
                });
            }
            let name = m.fit(&row.name, nspec, name_max);
            scene.push(Item::Text {
                text: name.into(),
                x: nx,
                y: base,
                spec: nspec,
                anchor: Anchor::Start,
                color: theme.text,
                alpha: 1.0,
                max_width: Some(name_max.max(1.0)),
            });
        }
        let mut y = r.y + 32.0 + self.rows.len() as f32 * CARD_ROW;
        if self.more > 0 {
            scene.push(Item::Text {
                text: format!("+{} more", self.more).into(),
                x: nx,
                y: y + 14.5,
                spec: nspec,
                anchor: Anchor::Start,
                color: theme.muted,
                alpha: 1.0,
                max_width: None,
            });
            y += CARD_ROW;
        }
        if let Some(t) = &self.total {
            let ly = y + 4.0;
            scene.push(Item::Path {
                path: PathData::polyline(&[(r.x + PAD, ly), (r.right() - PAD, ly)]),
                fill: None,
                stroke: Some(Stroke::hairline(theme.ring, 1.0)),
            });
            scene.push(Item::Text {
                text: t.as_str().into(),
                x: vx,
                y: ly + 19.5,
                spec: vspec,
                anchor: Anchor::End,
                color: theme.strong,
                alpha: 1.0,
                max_width: None,
            });
            scene.push(Item::Text {
                text: "total".into(),
                x: nx,
                y: ly + 19.5,
                spec: nspec,
                anchor: Anchor::Start,
                color: theme.muted,
                alpha: 1.0,
                max_width: None,
            });
        }
    }
}

/// A key in the card: a short line, a 10 px rounded square, or a dot.
fn key(scene: &mut Scene, shape: KeyShape, x: f32, mid: f32, color: Color) {
    let path = match shape {
        KeyShape::Line => PathData::rounded(Rect::new(x, mid - 1.0, KEY_W, 2.0), [1.0; 4]),
        KeyShape::Square => PathData::rounded(Rect::new(x + 2.0, mid - 5.0, 10.0, 10.0), [2.0; 4]),
        KeyShape::Dot => PathData::circle(x + 7.0, mid, 4.5),
    };
    scene.push(Item::Path {
        path,
        fill: Some(Paint::solid(color, 1.0)),
        stroke: None,
    });
}

/// A delta against the reference: "▲ 38 (6.3 %)", "▼ 12 (1.9 %)", "= 0".
fn delta(v: f64, base: f64, decimals: usize) -> String {
    if !(v.is_finite() && base.is_finite()) {
        return String::new();
    }
    let d = v - base;
    if d == 0.0 {
        return "= 0".into();
    }
    let arrow = if d > 0.0 { "▲" } else { "▼" };
    let amount = format_fixed(d.abs(), decimals);
    if base != 0.0 {
        let pct = d.abs() / base.abs() * 100.0;
        format!(
            "{arrow} {amount} ({} %)",
            format_fixed(pct, if pct < 10.0 { 1 } else { 0 })
        )
    } else {
        format!("{arrow} {amount}")
    }
}
