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

/// What a chart's overlay shows past [`OverlayState`]: the series and categories a search
/// lights (they stay bright, the rest fade under the wash), and whether the keyboard cursor's
/// card shows (not while the detail panel says the same; a hover's card always does).
#[derive(Clone, Debug, PartialEq)]
pub struct Lit {
    /// The series lit.
    pub series: Vec<SeriesKey>,
    /// The categories lit, on a band chart: their bars.
    pub bands: Vec<u32>,
    /// Whether the keyboard cursor's card shows.
    pub cursor_card: bool,
}

impl Default for Lit {
    fn default() -> Lit {
        Lit {
            series: Vec::new(),
            bands: Vec::new(),
            cursor_card: true,
        }
    }
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
    overlay_lit(model, layout, view, st, &Lit::default(), theme, m)
}

/// The overlay's scene with what a search lights and the cursor's card as `lit` says.
pub fn overlay_lit(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    st: &OverlayState,
    lit: &Lit,
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
    // What comes forward: the emphasised series and every one the search lights, and the
    // categories it lights, the rest under the wash.
    let mut forward: Vec<&SeriesKey> = emphasis.into_iter().collect();
    for k in &lit.series {
        if !forward.contains(&k) && model.visible(view, k) && model.series_of(k).is_some() {
            forward.push(k);
        }
    }
    let bands: Vec<u32> = (lit.bands.iter().copied())
        .filter(|&b| (b as usize) < model.x.bands.len())
        .collect();
    if (!forward.is_empty() && visible > 1) || !bands.is_empty() {
        wash(
            model, view, layout, theme, &mut scene, &forward, &bands, st.pointer, m,
        );
    }
    // The one the crosshair, dots and card follow: the hover, else the cursor.
    let target = hover.or(if brushing { None } else { cursor });
    let mut measure_label = None;
    if let Some(r) = view.reference.as_ref().filter(|c| valid(model, view, c)) {
        measure_label = reference(model, layout, theme, &mut scene, r, target, m);
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
        let anchor = if hover.is_some() { st.pointer } else { None };
        moment(
            model,
            view,
            layout,
            theme,
            &mut scene,
            t,
            mark.as_ref(),
            hover.is_none(),
            anchor,
            hover.is_some() || lit.cursor_card,
            m,
        );
    } else if let Some(c) = cursor.filter(|_| brushing) {
        marker(model, view, layout, theme, &mut scene, c, false, m);
    }
    // Δx over the measure band, where the card leaves it clear (the card's header says it too).
    if let Some((label, b)) = measure_label {
        let card = scene.items.iter().find_map(|i| match i {
            Item::Shadow { rect, .. } => Some(*rect),
            _ => None,
        });
        if card.is_none_or(|c| c.inflate(4.0).intersect(&b).is_none()) {
            scene.push(label);
        }
    }
    if let Some((kind, from, to)) = st.brush {
        brush(model, view, layout, theme, &mut scene, kind, from, to, m);
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

/// The surface over the plot, so every series but `keys` fades, and `keys` drawn again on
/// top; and the bars of `bands` (a search's categories) with them.
#[allow(clippy::too_many_arguments)]
fn wash(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    keys: &[&SeriesKey],
    bands: &[u32],
    pointer: Option<(f32, f32)>,
    m: &mut dyn Measure,
) {
    scene.push(Item::Path {
        path: PathData::rect(l.clip),
        fill: Some(Paint::solid(theme.surface, WASH)),
        stroke: None,
    });
    // The other series' end labels fade with them.
    for e in l.ends.iter().filter(|e| !keys.contains(&&e.key)) {
        let r = e.name.bounds(m).union(&e.value.bounds(m)).inflate(1.0);
        scene.push(Item::Path {
            path: PathData::rect(r),
            fill: Some(Paint::solid(theme.surface, WASH)),
            stroke: None,
        });
    }
    scene.push(Item::PushClip(l.clip));
    for (si, s) in model.series.iter().enumerate() {
        if keys.contains(&&s.key) && model.visible(view, &s.key) {
            marks::series(model, l, theme, scene, si, Look::Emphasis);
        }
    }
    for b in l.bars.iter().filter(|b| bands.contains(&b.band)) {
        marks::bar(scene, b, model.color(&model.series[b.series], theme));
    }
    scene.push(Item::PopClip);
    // A legend key under the pointer is underlined: the affordance, with no pointer shape.
    if let Some((px, py)) = pointer
        && let Some(item) = l.items.iter().find(|i| {
            keys.contains(&&i.key)
                && l.legend
                    .iter()
                    .any(|(k, r)| k == &i.key && r.contains(px, py))
        })
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
    pointer: Option<(f32, f32)>,
    card_shown: bool,
    m: &mut dyn Measure,
) {
    let p = l.plot;
    let anchor_y = pointer.map(|a| a.1);
    match t {
        Cursor::Band { band, .. } => {
            let Some(slot) = band_step(l, *band).or_else(|| band_slot(model, l, *band)) else {
                return;
            };
            // A faint column over the band's whole slot (the target is the slot, mouse-ux
            // §4.2), then its bars lifted.
            let col = if l.horizontal {
                Rect::new(p.x - 6.0, slot.y, p.w + 6.0, slot.h)
            } else {
                Rect::new(slot.x, p.y - 6.0, slot.w, p.h + 6.0)
            };
            let col = col.intersect(&l.clip.inflate(6.0)).unwrap_or(col);
            let radii = if l.horizontal {
                [6.0, 0.0, 0.0, 6.0]
            } else {
                [6.0, 6.0, 0.0, 0.0]
            };
            scene.push(Item::Path {
                path: PathData::rounded(col, radii),
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
                let name = band_name(model, *band);
                let at = l.ix(f64::from(*band));
                if l.horizontal {
                    chip_y(l, theme, scene, at, &name, true, m);
                } else {
                    chip_x(l, theme, scene, at, &name, true, m);
                }
            }
            let reference = view.reference.as_ref().and_then(|r| cursor_data(model, r));
            let rows = band_rows(model, view, l, *band, mark, reference.map(|r| r.0));
            let header = model.x_label(f64::from(*band), 0.0);
            let total = stacked_total(model, view, l, &rows);
            let mut card = Card::new(
                model,
                l,
                header,
                rows,
                total,
                reference.map(|(x, _)| x),
                f64::from(*band) + 0.5,
                theme,
                m,
            );
            card.shown = card_shown;
            // Where there's room, the card covers no bar, no total and not the pointer: over
            // the shorter neighbours, beside the lower ones, or over the band's own empty head.
            card.avoid = l.bars.iter().map(|b| b.rect).collect();
            card.avoid.extend(l.totals.iter().map(|t| t.1));
            if let Some((px, py)) = pointer {
                // Kept CARD_GAP from the pointer, as the card's first place is.
                let r = CARD_GAP - MARK_CLEAR;
                card.avoid.push(Rect::new(px - r, py - r, 2.0 * r, 2.0 * r));
            }
            card.home = Some(if l.horizontal {
                Home {
                    centre: slot.y + slot.h / 2.0,
                    step: slot.h.max(1.0),
                    across: false,
                }
            } else {
                Home {
                    centre: slot.x + slot.w / 2.0,
                    step: slot.w.max(1.0),
                    across: true,
                }
            });
            // What the card keeps clear of: the band's bars, from their ends to the baseline.
            let bars = l
                .bars
                .iter()
                .filter(|b| b.band == *band)
                .fold(None, |r: Option<Rect>, b| {
                    Some(r.map_or(b.rect, |o| o.union(&b.rect)))
                });
            if l.horizontal {
                // Bands go down the screen: the card goes past the bar's end and its total,
                // level with the band, or under the band (or over it) where that's no room.
                let end = bars.map_or(p.x, |b| b.right());
                let end = l
                    .totals
                    .iter()
                    .filter(|t| t.0 == *band)
                    .fold(end, |e, t| e.max(t.1.right()));
                let ax = pointer.map_or(end, |a| a.0.max(end));
                card.place_level(l, scene, (slot.y, slot.bottom()), ax, theme, m);
            } else {
                let ay = anchor_y.unwrap_or(p.y + 4.0);
                let span = bars.map_or((p.y, p.bottom()), |b| (b.y, b.bottom()));
                card.place_beside(l, scene, slot.x, slot.right(), ay, span, theme, m);
            }
        }
        Cursor::Cell { row, col } => {
            let Some((si, i)) = model.cell_at(*row, *col) else {
                return;
            };
            let s = &model.series[si];
            let Some(r) = marks::cell_rect(l, s, i) else {
                return;
            };
            cell_ring(scene, r, theme);
            let (xb, yb) = cell_names(model, *row, *col);
            if keyboard {
                chip_x(l, theme, scene, r.x + r.w / 2.0, &xb, true, m);
                chip_y(l, theme, scene, r.y + r.h / 2.0, &yb, true, m);
            } else {
                bright_tick(l, theme, scene, f64::from(*col) + 0.5);
                bright_y_tick(l, theme, scene, r.y + r.h / 2.0, m);
            }
            // The value first, keyed by its cell's colour, then the row's other fields.
            let v = s.sizes.as_ref().map_or(f64::NAN, |v| v[i]);
            let color = super::ramp::scale_of(model)
                .and_then(|sc| sc.color(v, theme))
                .unwrap_or_default();
            let name = Some(super::ramp::field_name(model))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "value".into());
            let mut rows = vec![Row {
                key: s.key.clone(),
                color,
                shape: if v.is_nan() {
                    KeyShape::Dot
                } else {
                    KeyShape::Square
                },
                name,
                value: cell_value(model, v, s.decimals),
                raw: v,
                base: f64::NAN,
                decimals: s.decimals,
                marked: false,
                missing: v.is_nan(),
            }];
            for (key, value, quiet) in model.other_fields(s, i) {
                rows.push(Row {
                    key: s.key.clone(),
                    color: Color::default(),
                    shape: KeyShape::Dot,
                    name: key,
                    value,
                    raw: f64::NAN,
                    base: f64::NAN,
                    decimals: 0,
                    marked: false,
                    missing: quiet,
                });
            }
            let mut card = Card::new(
                model,
                l,
                cell_header(model, &xb, &yb),
                rows,
                None,
                None,
                0.0,
                theme,
                m,
            );
            card.shown = card_shown;
            // Where there's room it leaves its cell, the title, the colour legend and the axes'
            // titles clear, covering other cells rather than what names them.
            card.avoid = vec![r.inflate(2.0)];
            if !l.title.is_empty() {
                card.avoid.push(l.title);
            }
            if let Some(legend) = &l.ramp {
                card.avoid.push(legend.bounds);
            }
            for t in l.labels.iter().filter(|t| t.role == Role::AxisTitle) {
                card.avoid.push(t.bounds(m));
            }
            card.place_point(
                l,
                scene,
                (r.x + r.w / 2.0, r.y + r.h / 2.0),
                r.w.max(r.h) / 2.0 + 2.0,
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
            // The chips say what the card says, at the data's precision.
            let yl = card_value(model, s.ys[i], s.decimals);
            chip_y(l, theme, scene, pt.1, &yl, keyboard, m);
            let rows = point_rows(model, view, si, i, theme);
            let mut header = s.name.clone();
            // Δx and Δy from the reference, each at its own axis's precision. No distance: a
            // scatter's two axes rarely share a unit.
            if let Some(ref_xy) = view.reference.as_ref().and_then(|r| cursor_data(model, r)) {
                let d = (s.xs[i] - ref_xy.0, s.ys[i] - ref_xy.1);
                header = format!(
                    "{header} · Δ {}, {}",
                    signed(d.0, s.x_decimals),
                    signed(d.1, s.decimals)
                );
            }
            let mut card = Card::new(model, l, header, rows, None, None, 0.0, theme, m);
            card.shown = card_shown;
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
            // The marked series' dot last, over any it meets.
            let mut dots: Vec<((f32, f32), bool, Color)> = Vec::new();
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
                dots.push((pt, mark == Some(&s.key), model.color(s, theme)));
            }
            dots.sort_by_key(|d| d.1);
            for &(pt, big, color) in &dots {
                let r = if big { DOT_BIG } else { DOT };
                dot(scene, pt, r, color, theme.surface, 2.0);
            }
            scene.push(Item::PopClip);
            // The dots' span, rings and all, and the lifted bars': what the card keeps clear of.
            let mut span = dots
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), d| {
                    let r = if d.1 { DOT_BIG } else { DOT } + 2.0;
                    (a.min(d.0.1 - r), b.max(d.0.1 + r))
                });
            for b in l
                .bars
                .iter()
                .filter(|b| (model.series[b.series].xs[b.sample] - col.x).abs() < 1e-9)
            {
                span = (span.0.min(b.rect.y), span.1.max(b.rect.bottom()));
            }
            if span.0 > span.1 {
                span = (p.y, p.bottom());
            }
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
            let delta = view
                .reference
                .as_ref()
                .and_then(|r| cursor_data(model, r))
                .map(|(x, _)| x);
            let rows = column_rows(model, view, l, &col, mark, theme, delta);
            let header = model.x_label(col.x, interval(model, &col));
            let total = stacked_total(model, view, l, &rows);
            let mut card = Card::new(model, l, header, rows, total, delta, col.x, theme, m);
            card.shown = card_shown;
            let ay = anchor_y.unwrap_or(p.y + 4.0 - CARD_GAP);
            // Beside the crosshair, clear of its dots (CARD_GAP is more than a dot's reach) and
            // of any bar it lifted.
            let reach = lifted_half(l, model, col.x, x);
            if !l.bars.is_empty() {
                // Over bars, as on a band chart: clear of every bar and of the crosshair's
                // column where there's room.
                card.avoid = l.bars.iter().map(|b| b.rect).collect();
                card.avoid.extend(l.totals.iter().map(|t| t.1));
                let r = reach + CARD_GAP - MARK_CLEAR;
                card.avoid
                    .push(Rect::new(x - r, p.y - 6.0, 2.0 * r, p.h + 6.0));
            }
            card.place_beside(l, scene, x - reach, x + reach, ay, span, theme, m);
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
            let name = band_name(model, *band);
            if l.horizontal {
                chip_y(l, theme, scene, slot.y + slot.h / 2.0, &name, true, m);
            } else {
                chip_x(l, theme, scene, slot.x + slot.w / 2.0, &name, true, m);
            }
        }
        Cursor::Cell { row, col } => {
            let Some((si, i)) = model.cell_at(*row, *col) else {
                return;
            };
            let Some(r) = marks::cell_rect(l, &model.series[si], i) else {
                return;
            };
            cell_ring(scene, r, theme);
            let (xb, yb) = cell_names(model, *row, *col);
            chip_x(l, theme, scene, r.x + r.w / 2.0, &xb, true, m);
            chip_y(l, theme, scene, r.y + r.h / 2.0, &yb, true, m);
        }
        _ => {}
    }
}

/// A heatmap cell lifted, for the hover and the keyboard's cursor alike: a 2 px ring in the
/// strong foreground outside it, a 2 px band of the surface between (the gap to its
/// neighbours, kept clear), so the ring reads against the hottest cell and the coldest alike:
/// the band parts it from the cell, whatever the cell's colour.
pub fn cell_ring(scene: &mut Scene, r: Rect, theme: &Theme) {
    scene.push(Item::Path {
        path: PathData::rounded(r.inflate(1.0), [marks::CELL_RADIUS + 1.0; 4]),
        fill: None,
        stroke: Some(Stroke::solid(2.0, theme.surface, 1.0)),
    });
    scene.push(Item::Path {
        path: PathData::rounded(r.inflate(3.0), [marks::CELL_RADIUS + 3.0; 4]),
        fill: None,
        stroke: Some(Stroke::solid(2.0, theme.strong, 1.0)),
    });
}

/// A heatmap cell's x and y categories.
fn cell_names(model: &ChartModel, row: u32, col: u32) -> (String, String) {
    let name = |bands: &[Box<str>], i: u32| {
        bands
            .get(i as usize)
            .map_or(String::new(), |b| b.to_string())
    };
    (name(&model.x.bands, col), name(&model.y.bands, row))
}

/// A heatmap cell's card header: its x and y categories, a bare number with its axis's name
/// ("Hour 10 · Thu"), as quiet as a chart's time.
fn cell_header(model: &ChartModel, x: &str, y: &str) -> String {
    let named = |axis: &super::AxisInfo, b: &str| -> String {
        let name = axis.title.as_deref().or(axis.field.as_deref());
        match name {
            Some(n) if b.trim().parse::<f64>().is_ok() => format!("{n} {b}"),
            _ => b.to_string(),
        }
    };
    format!("{} \u{b7} {}", named(&model.x, x), named(&model.y, y))
}

/// A heatmap cell's value as its card writes it: the colour channel's format, else in full at
/// the data's precision; "no data" for a missing one.
fn cell_value(model: &ChartModel, v: f64, decimals: usize) -> String {
    if v.is_nan() {
        return NO_DATA.into();
    }
    let format = model
        .series
        .iter()
        .find(|s| s.mark == MarkKind::Rect)
        .and_then(|s| model.chart.layers.get(usize::from(s.layer)))
        .and_then(|layer| layer.enc.color.as_ref())
        .and_then(|c| c.format.clone());
    match format {
        Some(f) => format_with(v, &f, &model.zone),
        None => format_fixed(v, decimals),
    }
}

/// The y tick label level with `y` brightened: a hovered cell's row reads as the target.
fn bright_y_tick(l: &ChartLayout, theme: &Theme, scene: &mut Scene, y: f32, m: &mut dyn Measure) {
    let near = l
        .labels
        .iter()
        .filter(|lb| lb.role == Role::YTick)
        .map(|lb| (lb, (lb.y - m.cap(lb.spec) / 2.0 - y).abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((label, d)) = near
        && d < 2.0
    {
        let mut item = label.item(theme);
        if let Item::Text { color, .. } = &mut item {
            *color = theme.strong;
        }
        scene.push(item);
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

/// A band's whole slot: its band and half the gap to each neighbour, less a pixel each side so
/// two slots never touch.
fn band_step(l: &ChartLayout, band: u32) -> Option<Rect> {
    let s = l.ind();
    let crate::scale::Scale::Band { inner, .. } = s else {
        return None;
    };
    let (a, w) = s.band_of(band as usize)?;
    let step = if *inner < 1.0 { w / (1.0 - inner) } else { w };
    let (a, w) = (a - (step - w) / 2.0 + 1.0, (step - 2.0).max(w));
    Some(if l.horizontal {
        Rect::new(l.plot.x, a, l.plot.w, w)
    } else {
        Rect::new(a, l.plot.y, w, l.plot.h)
    })
}

/// A band's name.
fn band_name(model: &ChartModel, band: u32) -> String {
    model
        .x
        .bands
        .get(band as usize)
        .map_or(String::new(), |b| b.to_string())
}

/// How far either side of the crosshair at `x` (model `at`) the bars it lifts reach.
fn lifted_half(l: &ChartLayout, model: &ChartModel, at: f64, x: f32) -> f32 {
    l.bars
        .iter()
        .filter(|b| (model.series[b.series].xs[b.sample] - at).abs() < 1e-9)
        .map(|b| (x - b.rect.x).abs().max((b.rect.right() - x).abs()))
        .fold(0.0, f32::max)
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
    // Its text on the tick labels' baseline, as m1's chip sits in the row of times.
    let top = l
        .labels
        .iter()
        .find(|lb| lb.role == Role::XTick)
        .map_or(l.plot.bottom() + 5.0, |lb| {
            lb.y - CHIP_H / 2.0 - m.cap(spec) / 2.0
        })
        .max(l.plot.bottom() + 3.0);
    let r = Rect::new(left, top, w, CHIP_H);
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
    let top = (y - CHIP_H / 2.0).clamp(INSET, (l.size.1 - INSET - CHIP_H).max(INSET));
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
    // Up the y axis the labels can be a band apart (a heatmap's rows): the chip hides only
    // those it overlaps, not a neighbour whose box (its ascent has the font's leading) only
    // grazes it.
    let dy = if role == Role::YTick { -1.5 } else { 4.0 };
    let clear = Rect::new(r.x - 6.0, r.y - dy, r.w + 12.0, r.h + 2.0 * dy);
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

/// The surface over every tick label, end label and bar total a card at `r` would cut, so none
/// shows a fragment (a "200" cut to "20"): a label it meets is hidden whole.
fn clear_ticks(l: &ChartLayout, theme: &Theme, scene: &mut Scene, r: Rect, m: &mut dyn Measure) {
    let mut cut: Vec<Rect> = Vec::new();
    if !l.plot.contains(r.x, r.y) || !l.plot.contains(r.right(), r.bottom()) {
        for label in l
            .labels
            .iter()
            .filter(|lb| matches!(lb.role, Role::YTick | Role::XTick))
        {
            cut.push(label.bounds(m));
        }
        for e in &l.ends {
            cut.push(e.name.bounds(m).union(&e.value.bounds(m)));
        }
    }
    cut.extend(l.totals.iter().map(|t| t.1));
    for b in cut {
        let b = b.inflate(1.0);
        if b.intersect(&r).is_some() {
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

/// The reference: a dashed rule at 40 % with an outlined chip `A` on the independent axis (a
/// dashed ring for a point on a scatter), and the measure band from it to the target; the Δx
/// label for its top, and its box, to draw once the card is placed. On a band axis the chip
/// says the band's name after the `A`, so the name it covers still reads, and the rule breaks
/// around the band's total.
#[allow(clippy::too_many_arguments)]
fn reference(
    model: &ChartModel,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    r: &Cursor,
    target: Option<&Cursor>,
    m: &mut dyn Measure,
) -> Option<(Item, Rect)> {
    let p = l.plot;
    let (rx, ry) = cursor_data(model, r)?;
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
        return None;
    }
    let band = model.is_band();
    let across = l.horizontal;
    let at = |v: f64| if band { l.ix(v - 0.5) } else { l.ix(v) };
    // Along the independent axis: the plot's extent that way.
    let (lo_edge, hi_edge) = if across {
        (p.y, p.bottom())
    } else {
        (p.x, p.right())
    };
    let x = at(rx);
    let mut label = None;
    if let Some((tx, _)) = target.and_then(|t| cursor_data(model, t)) {
        let t = at(tx);
        let (a, b) = (x.min(t).max(lo_edge), x.max(t).min(hi_edge));
        if b - a >= 1.0 {
            let rect = if across {
                Rect::new(p.x, a, p.w, b - a)
            } else {
                Rect::new(a, p.y, b - a, p.h)
            };
            scene.push(Item::Path {
                path: PathData::rect(rect),
                fill: Some(Paint::solid(theme.text, 0.06)),
                stroke: None,
            });
            let spec = TextSpec::new(TICK).tabular();
            let text = delta_x(model, rx, tx);
            let w = m.width(&text, spec);
            if !across && !text.is_empty() && w + 10.0 <= b - a {
                let (asc, desc) = (m.ascent(spec), m.descent(spec));
                let y = p.y + 6.0 + asc;
                let bounds = Rect::new((a + b - w) / 2.0, y - asc, w, asc + desc);
                let item = Item::Text {
                    text: text.into(),
                    x: (a + b) / 2.0,
                    y,
                    spec,
                    anchor: Anchor::Middle,
                    color: theme.text,
                    alpha: 1.0,
                    max_width: None,
                };
                label = Some((item, bounds));
            }
        }
    }
    if x < lo_edge - 1.0 || x > hi_edge + 1.0 {
        return label;
    }
    // The rule, broken around the band's total so its figures read.
    let mut cuts: Vec<(f32, f32)> = Vec::new();
    if band {
        let me = (rx - 0.5).round() as u32;
        for &(b, r) in &l.totals {
            if b == me {
                let r = r.inflate(3.0);
                cuts.push(if across {
                    (r.x, r.right())
                } else {
                    (r.y, r.bottom())
                });
            }
        }
    }
    let (from, to) = if across {
        (p.x, p.right())
    } else {
        (p.y, p.bottom())
    };
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pieces: Vec<(f32, f32)> = Vec::new();
    let mut start = from;
    for (a, b) in cuts {
        if a > start {
            pieces.push((start, a.min(to)));
        }
        start = start.max(b);
    }
    if start < to {
        pieces.push((start, to));
    }
    for (a, b) in pieces {
        if b - a < 1.0 {
            continue;
        }
        let ends = if across {
            [(a, x), (b, x)]
        } else {
            [(x, a), (x, b)]
        };
        scene.push(Item::Path {
            path: PathData::polyline(&ends),
            fill: None,
            stroke: Some(Stroke {
                dash: Some((4.0, 3.0)),
                ..Stroke::hairline(theme.text, 0.4)
            }),
        });
    }
    let name = if band {
        format!(
            "A · {}",
            band_name(model, (rx - 0.5).round().max(0.0) as u32)
        )
    } else {
        "A".to_string()
    };
    if across {
        chip_y(l, theme, scene, x, &name, false, m);
    } else {
        chip_x(l, theme, scene, x, &name, false, m);
    }
    label
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
                let sign = if n > 0 { "+" } else { "\u{2212}" };
                let unit = if n.abs() == 1 { "band" } else { "bands" };
                format!("{sign}{} {unit}", n.abs())
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
/// readout chip at its top. A bands brush snaps to the whole bands it touches; where the bands
/// already show at full width it can't zoom, and its chip says so over no band at all.
#[allow(clippy::too_many_arguments)]
fn brush(
    model: &ChartModel,
    view: &View,
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
    let mut bands = None;
    let mut cells = None;
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
            // Snapped to the whole bands it touches, as the zoom will read them.
            let Some((i, j)) = super::nav::brush_bands(model, l, view, from, to) else {
                let text = "Every band shows";
                readout_chip(l, theme, scene, (x0 + x1) / 2.0, y0, text, m);
                return;
            };
            bands = Some((i, j));
            let (sa, sb) = (l.ind().map(i as f64), l.ind().map(j as f64));
            if l.horizontal {
                (y0, y1) = (sa.min(sb).max(p.y), sa.max(sb).min(p.bottom()));
                (x0, x1) = (p.x, p.right());
            } else {
                (x0, x1) = (sa.min(sb).max(p.x), sa.max(sb).min(p.right()));
                (y0, y1) = (p.y, p.bottom());
            }
        }
        BrushKind::Box => {
            // A heatmap's box snapped to the whole cells it takes, as the zoom will read them.
            if let Some(((c0, c1), (r0, r1))) = super::nav::brush_cells(model, l, view, from, to) {
                let (xa, xb) = (l.x.map(c0 as f64), l.x.map(c1 as f64));
                let (ya, yb) = (l.y.map(r0 as f64), l.y.map(r1 as f64));
                (x0, x1) = (xa.min(xb).max(p.x), xa.max(xb).min(p.right()));
                (y0, y1) = (ya.min(yb).max(p.y), ya.max(yb).min(p.bottom()));
                cells = Some(((c0, c1), (r0, r1)));
            }
        }
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
    let text = match (bands, cells) {
        (Some((i, j)), _) => bands_readout(model, i, j),
        (None, Some((cols, rows))) => cells_readout(model, cols, rows),
        (None, None) => readout(model, view, l, kind, (x0, y0), (x1, y1)),
    };
    if !text.is_empty() {
        readout_chip(l, theme, scene, (x0 + x1) / 2.0, y0, &text, m);
    }
}

/// A brush's readout: a chip centred on `cx`, its top 6 px under `top`.
fn readout_chip(
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    cx: f32,
    top: f32,
    text: &str,
    m: &mut dyn Measure,
) {
    let spec = TextSpec::new(TICK).tabular();
    let w = m.width(text, spec) + 2.0 * CHIP_PAD;
    let left = (cx - w / 2.0).clamp(INSET, (l.size.0 - INSET - w).max(INSET));
    let chip_r = Rect::new(left, top + 6.0, w, CHIP_H);
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

/// Bands `i` to `j` (not included), written: "b – e · 4 bands", or the one band's name.
fn bands_readout(model: &ChartModel, i: usize, j: usize) -> String {
    let name = |k: usize| band_name(model, k as u32);
    let n = j.saturating_sub(i);
    if n <= 1 {
        name(i)
    } else {
        format!("{} – {} · {n} bands", name(i), name(j - 1))
    }
}

/// The cells a heatmap's brush takes, written: "09 – 16 · Mon – Fri", a single category by its
/// name.
fn cells_readout(model: &ChartModel, cols: (usize, usize), rows: (usize, usize)) -> String {
    let span = |bands: &[Box<str>], (i, j): (usize, usize)| -> String {
        let name = |k: usize| bands.get(k).map_or(String::new(), |b| b.to_string());
        if j <= i + 1 {
            name(i)
        } else {
            format!("{} \u{2013} {}", name(i), name(j - 1))
        }
    };
    format!(
        "{} \u{b7} {}",
        span(&model.x.bands, cols),
        span(&model.y.bands, rows)
    )
}

/// A time rounded to the data's step in the zone's own clock and calendar: to the day at a
/// local midnight, to the hour at a local hour (a half-hour zone's 07:00, not 07:30).
fn round_time(v: f64, step: f64, zone: &jiff::tz::TimeZone) -> f64 {
    use jiff::{RoundMode, Unit, ZonedRound};
    let Some(z) = crate::scale::time::zoned(v, zone) else {
        return v;
    };
    let whole = |unit: f64, per: i64| -> i64 {
        let n = (step / unit).round().max(1.0) as i64;
        if per % n == 0 { n } else { 1 }
    };
    let (unit, increment) = if step >= 86_400.0 {
        (Unit::Day, 1)
    } else if step >= 3600.0 {
        (Unit::Hour, whole(3600.0, 24))
    } else if step >= 60.0 {
        (Unit::Minute, whole(60.0, 60))
    } else if step >= 1.0 {
        (Unit::Second, whole(1.0, 60))
    } else {
        return v;
    };
    let how = ZonedRound::new()
        .smallest(unit)
        .increment(increment)
        .mode(RoundMode::HalfExpand);
    z.round(how)
        .map_or(v, |r| crate::scale::time::seconds(r.timestamp()))
}

/// What a brush selects, written: "06:05 – 06:40 · 35m", "12 – 48", "b – e · 4 bands".
fn readout(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    kind: BrushKind,
    a: (f32, f32),
    b: (f32, f32),
) -> String {
    let span_x = |lo: f64, hi: f64| -> String {
        match model.x.kind {
            AxisKind::Time => {
                let step = model.min_interval(view);
                let step = if step.is_finite() && step > 0.0 {
                    step
                } else {
                    (hi - lo) / 100.0
                };
                // At the data's own step, rounded to it in the zone, so a brush from 06:05
                // reads 06:05 and one from a local midnight reads that day.
                let (lo, hi) = (
                    round_time(lo, step, &model.zone),
                    round_time(hi, step, &model.zone),
                );
                format!(
                    "{} – {} · {}",
                    format_time_point(lo, step, &model.zone),
                    format_time_point(hi, step, &model.zone),
                    duration(hi - lo)
                )
            }
            AxisKind::Band => {
                let n = model.x.bands.len();
                let i = ((lo + 1e-6).floor().max(0.0) as usize).min(n.saturating_sub(1));
                let j = ((hi - 1e-6).ceil().max(0.0) as usize).clamp(i + 1, n.max(i + 1));
                bands_readout(model, i, j)
            }
            _ => format!("{} – {}", model.x_value(lo), model.x_value(hi)),
        }
    };
    // The values at the data's precision: the most decimals of the visible series.
    let decimals = model.series.iter().map(|s| s.decimals).max().unwrap_or(0);
    let span_y = |lo: f64, hi: f64| {
        format!(
            "{} – {}",
            model.value_label(lo, decimals),
            model.value_label(hi, decimals)
        )
    };
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
    /// The same quantity at the reference, for the delta: NaN without one.
    base: f64,
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

/// What a card's row says where its series has no sample: words, in the muted ink and the
/// names' weight, never a dash that reads as one more line key.
const NO_DATA: &str = "no data";

/// A value as the card writes it: the axis's format, else in full at its series' precision.
fn card_value(model: &ChartModel, v: f64, decimals: usize) -> String {
    if v.is_nan() {
        return NO_DATA.into();
    }
    if model.y.format.is_some() || model.series.iter().any(|s| s.stack == Stack::Normalize) {
        return model.y_label(v);
    }
    format_fixed(v, decimals)
}

/// Whether the chart's stacks are normalised: its values are shares.
fn normalised(model: &ChartModel) -> bool {
    model.series.iter().any(|s| s.stack == Stack::Normalize)
}

/// The value a card shows for a sample: a normalised stack's share of its column, else its own.
fn shown_value(model: &ChartModel, l: &ChartLayout, si: usize, i: usize) -> f64 {
    let s = &model.series[si];
    if normalised(model)
        && let (Some(lo), Some(hi)) = (&l.stacks.lo[si], &l.stacks.hi[si])
        && i < lo.len()
    {
        return hi[i] - lo[i];
    }
    s.ys.get(i).copied().unwrap_or(f64::NAN)
}

/// The rows of an x-chart's column: unstacked series by value, high first; stacked ones top to
/// bottom as drawn.
#[allow(clippy::too_many_arguments)]
fn column_rows(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    col: &Column,
    mark: Option<&SeriesKey>,
    theme: &Theme,
    reference: Option<f64>,
) -> Vec<Row> {
    let mut rows: Vec<(f64, usize, Row)> = Vec::new();
    for &(si, i) in &col.samples {
        let s = &model.series[si];
        let raw = i.map_or(f64::NAN, |i| shown_value(model, l, si, i));
        let base = reference
            .and_then(|rx| sample_near(model, None, si, rx))
            .map_or(f64::NAN, |j| shown_value(model, l, si, j));
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
                base,
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
    reference: Option<f64>,
) -> Vec<Row> {
    let mut rows: Vec<(f64, usize, Row)> = Vec::new();
    for si in super::hover::walked(model, view) {
        let s = &model.series[si];
        let at =
            |b: f64| (0..s.xs.len()).find(|&i| !s.xs[i].is_nan() && (s.xs[i] - b).abs() < 1e-9);
        let i = at(f64::from(band));
        let raw = i.map_or(f64::NAN, |i| shown_value(model, l, si, i));
        // The reference is at a band's centre.
        let base = reference
            .and_then(|rx| at(rx - 0.5))
            .map_or(f64::NAN, |j| shown_value(model, l, si, j));
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
                base,
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
            base: f64::NAN,
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
            base: f64::NAN,
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
            base: f64::NAN,
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

/// A card: its header, rows and total, measured, and what it keeps clear of where it can.
struct Card {
    /// Whether it's drawn: the keyboard cursor's isn't while the detail panel says the same.
    shown: bool,
    /// The marks it covers only where there's no room clear of them (a bar chart's bars, their
    /// totals, the pointer): empty where it goes where it's put.
    avoid: Vec<Rect>,
    /// The band it's about, on a band chart: it stays by it where there's room.
    home: Option<Home>,
    header: String,
    rows: Vec<Row>,
    more: usize,
    total: Option<String>,
    deltas: Vec<String>,
    val_w: f32,
    delta_w: f32,
    name_w: f32,
    header_w: f32,
    w: f32,
    h: f32,
}

/// The band a card is about: its middle and its step along the band axis, across the screen
/// (`across`) or down it.
#[derive(Clone, Copy)]
struct Home {
    centre: f32,
    step: f32,
    across: bool,
}

/// The most places along the rects' narrow axis a card tries to sit clear of them, nearest
/// first: every one on a chart of a few dozen bars, enough near the hover on a day of minutes.
const CLEAR_TRIES: usize = 96;

const PAD: f32 = 12.0;
/// How far a card keeps from a mark it avoids.
const MARK_CLEAR: f32 = 8.0;
const KEY_W: f32 = 14.0;
const GAP: f32 = 9.0;
/// The least a card's name column narrows to, its names cut with '…', before the Δ column goes.
const NAME_MIN: f32 = 44.0;
/// A card's height past its rows: its header above them and its padding under.
const CARD_CHROME: f32 = 12.0 + 13.0 + 7.0 + 7.0;

/// Where a card goes beside its anchor.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Right,
    Left,
}

impl Card {
    /// A card's rows measured, at most twelve and no more than the area's height holds (the
    /// marked row always among them, the rest as "+N more"), with Δs against the reference at
    /// `reference_x` when there's one.
    #[allow(clippy::too_many_arguments)]
    fn new(
        model: &ChartModel,
        l: &ChartLayout,
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
        let total_h = if total.is_some() { 9.0 + CARD_ROW } else { 0.0 };
        let room = ((l.size.1 - 2.0 * INSET - CARD_CHROME - total_h) / CARD_ROW).floor();
        let room = if room.is_finite() {
            room.max(1.0) as usize
        } else {
            1
        };
        let cap = if rows.len() <= CARD_ROWS.min(room) {
            rows.len()
        } else {
            // Room for "+N more" under them.
            CARD_ROWS.min(room.saturating_sub(1)).max(1)
        };
        let mut more = 0;
        if rows.len() > cap {
            more = rows.len() - cap;
            if let Some(k) = rows.iter().position(|r| r.marked)
                && k >= cap
            {
                let r = rows.remove(k);
                rows.insert(cap - 1, r);
            }
            rows.truncate(cap);
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
            let points = normalised(model);
            for r in &rows {
                deltas.push(delta(r.raw, r.base, r.decimals, points));
            }
        }
        let vspec = TextSpec::new(13.0).weight(Weight::Semibold).tabular();
        let nspec = TextSpec::new(BODY);
        let dspec = TextSpec::new(BODY).tabular();
        let hspec = TextSpec::new(TICK).tabular();
        let mut val_w: f32 = 0.0;
        let mut name_w: f32 = 0.0;
        for r in &rows {
            let spec = if r.missing { nspec } else { vspec };
            val_w = val_w.max(m.width(&r.value, spec));
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
        let header_w = m.width(&header, hspec);
        let n = rows.len() + usize::from(more > 0);
        let h = CARD_CHROME + n as f32 * CARD_ROW + total_h;
        let mut card = Card {
            shown: true,
            avoid: Vec::new(),
            home: None,
            header,
            rows,
            more,
            total,
            deltas,
            val_w,
            delta_w,
            name_w: name_w.min(260.0),
            header_w,
            w: 0.0,
            h,
        };
        card.w = card.natural();
        card
    }

    /// The width its columns ask for, the header's too.
    fn natural(&self) -> f32 {
        self.body(self.name_w).max(self.header_w + 2.0 * PAD).ceil()
    }

    /// The width of its rows with the names `name_w` wide.
    fn body(&self, name_w: f32) -> f32 {
        PAD + KEY_W
            + GAP
            + self.val_w
            + if self.delta_w > 0.0 {
                GAP + self.delta_w
            } else {
                0.0
            }
            + GAP
            + name_w
            + 14.0
    }

    /// The narrowest it can be: names cut to `NAME_MIN` (or their own width), no Δ column.
    fn narrowest(&self) -> f32 {
        let names = self.name_w.min(NAME_MIN);
        let deltas = if self.delta_w > 0.0 {
            GAP + self.delta_w
        } else {
            0.0
        };
        self.body(names) - deltas
    }

    /// Narrowed to `max` if it can be: the names cut first (the header with them), then the Δ
    /// column dropped. False when even that won't fit, the card left as it was.
    fn narrow(&mut self, max: f32) -> bool {
        if self.w <= max {
            return true;
        }
        if self.body(self.name_w.min(NAME_MIN)) <= max {
            self.w = max.floor();
            return true;
        }
        if self.narrowest() <= max {
            self.deltas.clear();
            self.delta_w = 0.0;
            self.w = max.floor();
            return true;
        }
        false
    }

    /// Beside a crosshair or a band, never over what's between `left` and `right` from `span.0`
    /// to `span.1` (its dots, its bars): right of `right` (left of `left` once it would pass the
    /// plot's right edge, back only with a cell to spare), its top `CARD_GAP` below `ay`, slid
    /// up to fit the area. Where neither side has room for it whole, it narrows; where neither
    /// has room for it narrowed, it goes under the span or over it.
    #[allow(clippy::too_many_arguments)]
    fn place_beside(
        &mut self,
        l: &ChartLayout,
        scene: &mut Scene,
        left: f32,
        right: f32,
        ay: f32,
        span: (f32, f32),
        theme: &Theme,
        m: &mut dyn Measure,
    ) {
        let (aw, ah) = l.size;
        // The plot's right edge, so the card leaves the end labels be, unless only past it does
        // the card fit.
        let soft = (l.plot.right() + 8.0).min(aw - INSET);
        let room_right = |limit: f32| limit - (right + CARD_GAP);
        let room_left = left - CARD_GAP - INSET;
        let spare = if l.flipped.get() { l.cell_width } else { 0.0 };
        let side = if self.w + spare <= room_right(soft) {
            Some(Side::Right)
        } else if self.w <= room_left {
            Some(Side::Left)
        } else if self.w <= room_right(aw - INSET) {
            Some(Side::Right)
        } else {
            // Narrowed, on the side with more room.
            let (side, room) = if room_left >= room_right(aw - INSET) {
                (Side::Left, room_left)
            } else {
                (Side::Right, room_right(aw - INSET))
            };
            self.narrow(room).then_some(side)
        };
        let Some(side) = side else {
            self.place_off(l, scene, span, right, theme, m);
            return;
        };
        l.flipped.set(side == Side::Left);
        let x = match side {
            Side::Right => right + CARD_GAP,
            Side::Left => left - CARD_GAP - self.w,
        };
        // Over the plot, not the axis under it, where it fits: clear of the baseline, so its
        // edge never merges with the axis's line.
        let floor = if self.h <= l.plot.bottom() - 2.0 * INSET {
            l.plot.bottom() - INSET
        } else {
            ah - INSET
        };
        let y = (ay + CARD_GAP).min(floor - self.h).max(INSET);
        let r = self.clear_of(l, Rect::new(x.round(), y.round(), self.w, self.h));
        clear_ticks(l, theme, scene, r, m);
        self.draw(scene, r, theme, m);
    }

    /// Right of `ax`, level with the span from `span.0` to `span.1` down the screen (a band
    /// across), where it fits there; else under the span or over it.
    #[allow(clippy::too_many_arguments)]
    fn place_level(
        &mut self,
        l: &ChartLayout,
        scene: &mut Scene,
        span: (f32, f32),
        ax: f32,
        theme: &Theme,
        m: &mut dyn Measure,
    ) {
        let (aw, ah) = l.size;
        let x = ax + CARD_GAP;
        if x + self.w > aw - INSET {
            self.place_off(l, scene, span, ax.min(l.plot.x + l.plot.w / 2.0), theme, m);
            return;
        }
        let mid = (span.0 + span.1) / 2.0;
        let y = (mid - self.h / 2.0).min(ah - INSET - self.h).max(INSET);
        let r = self.clear_of(l, Rect::new(x.round(), y.round(), self.w, self.h));
        clear_ticks(l, theme, scene, r, m);
        self.draw(scene, r, theme, m);
    }

    /// Under the span from `span.0` to `span.1` down the screen (a band across, a crosshair's
    /// dots), or over it where there's more room there, starting right of `ax`.
    #[allow(clippy::too_many_arguments)]
    fn place_off(
        &mut self,
        l: &ChartLayout,
        scene: &mut Scene,
        span: (f32, f32),
        ax: f32,
        theme: &Theme,
        m: &mut dyn Measure,
    ) {
        let (aw, ah) = l.size;
        self.narrow(aw - 2.0 * INSET);
        let below = span.1 + CARD_GAP;
        let above = span.0 - CARD_GAP - self.h;
        let y = if below + self.h <= ah - INSET {
            below
        } else if above >= INSET {
            above
        } else if ah - span.1 >= span.0 {
            below.min(ah - INSET - self.h).max(INSET)
        } else {
            above.max(INSET)
        };
        let x = (ax + CARD_GAP).min(aw - INSET - self.w).max(INSET);
        let r = self.clear_of(l, Rect::new(x.round(), y.round(), self.w, self.h));
        clear_ticks(l, theme, scene, r, m);
        self.draw(scene, r, theme, m);
    }

    /// Beside a point, never over it: right-below, right-above, left-below, then left-above.
    fn place_point(
        &mut self,
        l: &ChartLayout,
        scene: &mut Scene,
        p: (f32, f32),
        r: f32,
        theme: &Theme,
        m: &mut dyn Measure,
    ) {
        let (aw, ah) = l.size;
        self.narrow(aw - 2.0 * INSET);
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
        let rect = self.clear_of(l, Rect::new(x.round(), y.round(), self.w, self.h));
        clear_ticks(l, theme, scene, rect, m);
        self.draw(scene, rect, theme, m);
    }

    /// `r` where it covers none of what the card avoids (kept `MARK_CLEAR` px from each);
    /// else the nearest place that does, inside the area and over the plot where it fits, the
    /// legend and the title costing a little to cover, and a band's card costing a band's step
    /// for each band past its neighbour it strays (so it stays with its band where that has
    /// room); else `r` after all.
    ///
    /// The places tried are where an edge of the card meets an edge of what it avoids, or of
    /// the area: at most [`CLEAR_TRIES`] across the rects' narrow axis, nearest first, each
    /// tested against the rects that reach it by a sweep, so a day of minute bars costs no more
    /// than a dozen.
    fn clear_of(&self, l: &ChartLayout, r: Rect) -> Rect {
        let inflated: Vec<Rect> = self.avoid.iter().map(|a| a.inflate(MARK_CLEAR)).collect();
        let meets = |c: &Rect, a: &Rect| c.intersect(a).is_some_and(|i| i.w > 0.01 && i.h > 0.01);
        if inflated.is_empty() || !inflated.iter().any(|a| meets(&r, a)) {
            return r;
        }
        let (aw, ah) = l.size;
        let (w, h) = (r.w, r.h);
        let floor = if h <= l.plot.bottom() - 2.0 * INSET {
            l.plot.bottom() - INSET
        } else {
            ah - INSET
        };
        let (x0, x1, y0, y1) = (INSET, aw - INSET - w, INSET, floor - h);
        if x1 < x0 || y1 < y0 {
            return r;
        }
        // Bars down the screen are narrow across it, bars across narrow down it: the search
        // runs along the axis the rects are narrow on, `u`, and tries every place down the
        // other, `v`.
        let widest = |f: fn(&Rect) -> f32| inflated.iter().map(f).fold(0.0, f32::max);
        let across = widest(|a| a.w) <= widest(|a| a.h);
        // (u0, u1, v0, v1) of a rect, (u, v) of a point, and back.
        let uv = |a: &Rect| match across {
            true => (a.x, a.right(), a.y, a.bottom()),
            false => (a.y, a.bottom(), a.x, a.right()),
        };
        let (su, sv) = if across { (w, h) } else { (h, w) };
        let (u_lo, u_hi, v_lo, v_hi) = if across {
            (x0, x1, y0, y1)
        } else {
            (y0, y1, x0, x1)
        };
        let (ru, rv) = if across { (r.x, r.y) } else { (r.y, r.x) };
        let point = |u: f32, v: f32| if across { (u, v) } else { (v, u) };
        // A pixel past the inflated rect: MARK_CLEAR and one from the mark itself.
        let gap = 1.0;
        let mut blocks: Vec<(f32, f32, f32, f32)> = inflated.iter().map(uv).collect();
        blocks.sort_by(|a, b| a.0.total_cmp(&b.0));
        let longest = blocks.iter().map(|b| b.1 - b.0).fold(0.0, f32::max);
        // The places: rounded as the card is placed, each once.
        let places = |lo: f32, hi: f32, from: f32, size: f32, along_u: bool| {
            let mut out: Vec<f32> = vec![from.round(), lo.round(), hi.round()];
            for b in &blocks {
                let (a, z) = if along_u { (b.0, b.1) } else { (b.2, b.3) };
                out.extend([(z + gap).round(), (a - gap - size).round()]);
            }
            out.retain(|&v| v >= lo - 0.51 && v <= hi + 0.51);
            out.sort_by(f32::total_cmp);
            out.dedup();
            out
        };
        let mut us = places(u_lo, u_hi, ru, su, true);
        let mut vs = places(v_lo, v_hi, rv, sv, false);
        // A band's card also tries the places that centre it on its band and on each
        // neighbour, and the furthest that still reads as its neighbour's.
        let home = self.home;
        let mut homes: Vec<f32> = Vec::new();
        if let Some(hm) = home {
            let size = if hm.across { w } else { h };
            for k in [0.0, -1.0, 1.0, -1.45, 1.45] {
                homes.push((hm.centre + k * hm.step - size / 2.0).round());
            }
            let (lo, hi, list) = if hm.across == across {
                (u_lo, u_hi, &mut us)
            } else {
                (v_lo, v_hi, &mut vs)
            };
            homes.retain(|&v| v >= lo - 0.51 && v <= hi + 0.51);
            list.extend(homes.iter().copied());
            list.sort_by(f32::total_cmp);
            list.dedup();
        }
        us.sort_by(|a, b| (a - ru).abs().total_cmp(&(b - ru).abs()));
        // The area's edges and the band's own places stay among them however many there are,
        // in their order.
        let mut kept: Vec<f32> = us.iter().copied().take(CLEAR_TRIES).collect();
        for e in us.iter().skip(CLEAR_TRIES) {
            if *e == u_lo.round() || *e == u_hi.round() || homes.contains(e) {
                kept.push(*e);
            }
        }
        let cost = |x: f32, y: f32| {
            let (dx, dy) = (x - r.x, y - r.y);
            // Over the legend or the title above the plot costs a little more than over the
            // plot's empty space.
            let above = (l.plot.y - y).max(0.0);
            // A band's card past its neighbour reads as another band's.
            let strays = home.map_or(0.0, |hm| {
                let centre = if hm.across { x + w / 2.0 } else { y + h / 2.0 };
                let bands = ((centre - hm.centre).abs() / hm.step - 0.5).ceil().max(0.0);
                (bands - 1.0).max(0.0) * hm.step
            });
            (dx * dx + dy * dy).sqrt() + above + strays
        };
        let mut best: Option<(f32, Rect)> = None;
        let mut spans: Vec<(f32, f32)> = Vec::new();
        for &u in &kept {
            // Nothing further along can cost less than this place's distance already does.
            if best.as_ref().is_some_and(|b| (u - ru).abs() >= b.0) {
                break;
            }
            // What reaches across [u, u + su], as spans down v, merged.
            spans.clear();
            let first = blocks.partition_point(|b| b.0 <= u + 0.01 - longest);
            for b in &blocks[first..] {
                if b.0 >= u + su - 0.01 {
                    break;
                }
                if b.1 > u + 0.01 {
                    spans.push((b.2, b.3));
                }
            }
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f32, f32)> = Vec::with_capacity(spans.len());
            for &(a, z) in &spans {
                match merged.last_mut() {
                    Some(m) if a <= m.1 => m.1 = m.1.max(z),
                    _ => merged.push((a, z)),
                }
            }
            for &v in &vs {
                // The first span that ends past the card's start; free if it starts past its end.
                let i = merged.partition_point(|m| m.1 <= v + 0.01);
                if merged.get(i).is_some_and(|m| m.0 < v + sv - 0.01) {
                    continue;
                }
                let (x, y) = point(u, v);
                let k = cost(x, y);
                if best.as_ref().is_none_or(|b| k < b.0) {
                    best = Some((k, Rect::new(x, y, w, h)));
                }
            }
        }
        best.map_or(r, |b| b.1)
    }

    fn draw(&self, scene: &mut Scene, r: Rect, theme: &Theme, m: &mut dyn Measure) {
        if !self.shown {
            return;
        }
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
                key(scene, row.shape, r.x + PAD, mid, row.color, theme);
            }
            let base = y0 + 14.5;
            scene.push(Item::Text {
                text: row.value.as_str().into(),
                x: vx,
                y: base,
                spec: if row.missing { nspec } else { vspec },
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

/// A key in the card: a short line, a 10 px rounded square, or a dot. A square that would
/// barely show on the card's ground (a diverging scale's cell near its midpoint, on dark) gets
/// a faint ring in the text colour.
fn key(scene: &mut Scene, shape: KeyShape, x: f32, mid: f32, color: Color, theme: &Theme) {
    let path = match shape {
        KeyShape::Line => PathData::rounded(Rect::new(x, mid - 1.0, KEY_W, 2.0), [1.0; 4]),
        KeyShape::Square => PathData::rounded(Rect::new(x + 2.0, mid - 5.0, 10.0, 10.0), [2.0; 4]),
        KeyShape::Dot => PathData::circle(x + 7.0, mid, 4.5),
    };
    let faint = shape == KeyShape::Square && super::errors::contrast(color, theme.tooltip) < 1.6;
    scene.push(Item::Path {
        path,
        fill: Some(Paint::solid(color, 1.0)),
        stroke: faint.then(|| Stroke::hairline(theme.text, 0.45)),
    });
}

/// A delta against the reference: "▲ 38 (6.3 %)", "▼ 12 (1.9 %)", "= 0"; between two shares
/// (`points`), in percentage points: "▲ 25 pp".
fn delta(v: f64, base: f64, decimals: usize, points: bool) -> String {
    if !(v.is_finite() && base.is_finite()) {
        return String::new();
    }
    let d = v - base;
    if d == 0.0 {
        return "= 0".into();
    }
    let arrow = if d > 0.0 { "▲" } else { "▼" };
    if points {
        let pp = (d.abs() * 1000.0).round() / 10.0;
        return format!("{arrow} {} pp", format_value(pp));
    }
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
