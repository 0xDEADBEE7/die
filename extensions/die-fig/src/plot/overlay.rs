//! The overlay: crosshair, dots, the card, the axes' chips, the brush, the reference and the
//! emphasis wash, drawn over the base and kept close so its damage stays small (ruling 10).
//! The stand-in's (see [`super`]): a crosshair, a dot a series, a plain card, the brush's band
//! and a wash for emphasis.

use super::{ChartLayout, ChartModel};
use crate::data::SeriesKey;
use crate::interact::consts::CARD_GAP;
use crate::interact::{BrushKind, Cursor, View};
use crate::scale::{format_time_full, format_value};
use crate::scene::{Anchor, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec};
use crate::theme::Theme;

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
    /// The series emphasised.
    pub emphasis: Option<SeriesKey>,
}

/// The overlay's scene.
pub fn overlay(
    model: &ChartModel,
    layout: &ChartLayout,
    view: &View,
    st: &OverlayState,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(0.0, 0.0, 1.0);
    let plot = layout.plot;
    if let Some(key) = &st.emphasis {
        scene.push(Item::Path {
            path: PathData::rect(plot),
            fill: Some(Paint::solid(theme.surface, 0.35)),
            stroke: None,
        });
        let _ = key;
    }
    if let Some((_, a, b)) = st.brush {
        let r = Rect::new(a.0.min(b.0), plot.y, (a.0 - b.0).abs(), plot.h);
        scene.push(Item::Path {
            path: PathData::rect(r),
            fill: Some(Paint::solid(theme.strong, 0.08)),
            stroke: None,
        });
    }
    let Some(Cursor::Point { series, row }) = st.hover.as_ref().or(st.cursor.as_ref()) else {
        return scene;
    };
    let Some(s) = model.series.iter().find(|s| &s.key == series) else {
        return scene;
    };
    let Some(&x) = s.xs.get(*row as usize) else {
        return scene;
    };
    let px = layout.x.map(x).round() + 0.5;
    scene.push(Item::Path {
        path: PathData::polyline(&[(px, plot.y), (px, plot.bottom())]),
        fill: None,
        stroke: Some(Stroke::hairline(theme.muted, 0.6)),
    });
    let mut lines = vec![];
    for t in model.series.iter().filter(|t| view.shows(&t.key)) {
        let i = t.xs.partition_point(|v| *v < x);
        let Some((&tx, &ty)) = t.xs.get(i).zip(t.ys.get(i)) else {
            continue;
        };
        if tx != x || !ty.is_finite() {
            continue;
        }
        let color = t.slot.map_or(theme.other, |i| theme.series[usize::from(i)]);
        scene.push(Item::Path {
            path: PathData::circle(px, layout.y.map(ty), 4.0),
            fill: Some(Paint::solid(color, 1.0)),
            stroke: Some(Stroke::solid(2.0, theme.surface, 1.0)),
        });
        lines.push(format!("{}  {}", format_value(ty), t.key.name));
    }
    let head = match model.time_x() {
        true => format_time_full(x, 60.0, model.zone()),
        false => format_value(x),
    };
    lines.insert(0, head);
    let spec = TextSpec::new(12.0);
    let w = lines.iter().map(|l| m.width(l, spec)).fold(0.0, f32::max) + 16.0;
    let h = lines.len() as f32 * 16.0 + 8.0;
    let y = st
        .pointer
        .map_or(plot.y, |p| p.1)
        .clamp(plot.y, plot.bottom() - h);
    let x = if px + CARD_GAP + w <= plot.right() {
        px + CARD_GAP
    } else {
        px - CARD_GAP - w
    };
    let card = Rect::new(x, y, w, h);
    scene.push(Item::Path {
        path: PathData::rounded(card, [4.0; 4]),
        fill: Some(Paint::solid(theme.tooltip, 1.0)),
        stroke: Some(Stroke::hairline(theme.ring, 1.0)),
    });
    for (i, l) in lines.iter().enumerate() {
        scene.push(Item::Text {
            text: l.as_str().into(),
            x: card.x + 8.0,
            y: card.y + 16.0 * (i as f32 + 1.0),
            spec,
            anchor: Anchor::Start,
            color: theme.strong,
            alpha: 1.0,
            max_width: None,
        });
    }
    scene
}
