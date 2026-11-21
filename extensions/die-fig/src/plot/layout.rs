//! The chart's anatomy: the title, the legend, the axes and the plot's rectangle, at a size.
//!
//! From the top: the title (15 px semibold), the subtitle (12 px muted), the legend under them
//! (a key and a name each, wrapped to rows; none for one series), the y axis's title above its
//! ticks, then the plot with its ticks (11 px muted tabular figures, y about every 48 px, x
//! labels at least 64 px apart) and the x axis's title under the x labels. End labels take a
//! column right of the plot when they fit. Small areas degrade in order: the subtitle goes, then
//! the legend keeps one row with "+N more", then the end labels, so nothing overlaps.

use super::{AxisKind, ChartModel, Stacks};
use crate::data::SeriesKey;
use crate::interact::View;
use crate::scale::{self, Scale, Tick, format_with};
use crate::scene::{
    Anchor, Hit, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Weight,
};
use crate::spec::{LegendPos, MarkKind, Stack};
use crate::theme::{Color, Theme};
use std::cell::Cell;

/// The title's size.
pub const TITLE: f32 = 15.0;
/// The subtitle's, the legend's and the end labels'.
pub const BODY: f32 = 12.0;
/// The ticks' and the axes' titles'.
pub const TICK: f32 = 11.0;
/// A legend row's height.
const ROW: f32 = 20.0;
/// From a tick label to the plot.
const TICK_GAP: f32 = 10.0;
/// Between legend items.
const ITEM_GAP: f32 = 18.0;
/// A legend key's width, and from it to its name.
const KEY_W: f32 = 14.0;
const KEY_GAP: f32 = 6.0;
/// The least room between two x labels' centres, and between their ends.
const X_SPACING: f32 = 64.0;
const X_CLEAR: f32 = 12.0;
/// About how far apart y ticks are.
const Y_SPACING: f32 = 48.0;
/// How far a mark may reach past the plot's edges at fit before it's clipped: a point's ring.
pub const BLEED: f32 = 6.0;

/// Which of the theme's text colours a piece of chrome wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The title.
    Strong,
    /// Names.
    Text,
    /// Ticks, subtitles, values.
    Muted,
}

impl Ink {
    /// Its colour in a theme.
    pub fn color(self, theme: &Theme) -> Color {
        match self {
            Ink::Strong => theme.strong,
            Ink::Text => theme.text,
            Ink::Muted => theme.muted,
        }
    }
}

/// What a piece of the chrome's text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The title or the subtitle.
    Title,
    /// A legend name, or the legend's "+N more".
    Legend,
    /// An axis's title.
    AxisTitle,
    /// A tick's label across the screen.
    XTick,
    /// A tick's label up the screen.
    YTick,
    /// An end label's name or value.
    End,
}

/// A piece of text the layout placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// What it is.
    pub role: Role,
    /// What it says, already cut to fit.
    pub text: String,
    /// Its anchor point's x.
    pub x: f32,
    /// Its baseline.
    pub y: f32,
    /// Its type.
    pub spec: TextSpec,
    /// Which end of it is at x.
    pub anchor: Anchor,
    /// Its colour.
    pub ink: Ink,
    /// Its opacity.
    pub alpha: f32,
}

impl Label {
    /// The scene item, in `theme`'s colours.
    pub fn item(&self, theme: &Theme) -> Item {
        Item::Text {
            text: self.text.as_str().into(),
            x: self.x,
            y: self.y,
            spec: self.spec,
            anchor: self.anchor,
            color: self.ink.color(theme),
            alpha: self.alpha,
            max_width: None,
        }
    }

    /// The box it takes, measured by `m`.
    pub fn bounds(&self, m: &mut dyn Measure) -> Rect {
        text_box(&self.text, self.x, self.y, self.spec, self.anchor, m)
    }
}

/// A text's box: its advance across, its ascent and descent down.
pub fn text_box(
    text: &str,
    x: f32,
    y: f32,
    spec: TextSpec,
    anchor: Anchor,
    m: &mut dyn Measure,
) -> Rect {
    let w = m.width(text, spec);
    let (a, d) = (m.ascent(spec), m.descent(spec));
    let left = match anchor {
        Anchor::Start => x,
        Anchor::Middle => x - w / 2.0,
        Anchor::End => x - w,
    };
    Rect::new(left, y - a, w, a + d)
}

/// A legend key's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyShape {
    /// A short line, for lines.
    Line,
    /// A 10 px rounded square, for bars and areas.
    Square,
    /// A dot, for points.
    Dot,
}

/// One legend entry: where its key and its name went.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendItem {
    /// Its series.
    pub key: SeriesKey,
    /// Its key's shape.
    pub shape: KeyShape,
    /// The key's box.
    pub swatch: Rect,
    /// The name.
    pub name: Label,
    /// Whether the series is hidden: a hollow key and a faint name.
    pub hidden: bool,
}

/// A line's end label: its name in the text colour and its last value muted.
#[derive(Clone, Debug, PartialEq)]
pub struct EndLabel {
    /// Its series.
    pub key: SeriesKey,
    /// The name.
    pub name: Label,
    /// The value.
    pub value: Label,
}

/// Where everything landed: the plot rect, scales with ranges, legend rows.
pub struct ChartLayout {
    /// The plot's rectangle.
    pub plot: Rect,
    /// The scale across the screen, onto the plot.
    pub x: Scale,
    /// The scale up the screen, onto the plot.
    pub y: Scale,
    /// Each legend key's rectangle: its key and name, inflated 2 px, the row's height.
    pub legend: Vec<(SeriesKey, Rect)>,
    /// The x axis's tick-label strip: drag and scroll there act on x alone.
    pub x_axis: Rect,
    /// The y axis's.
    pub y_axis: Rect,
    /// The title and subtitle's box.
    pub title: Rect,
    /// The area's size.
    pub size: (f32, f32),
    /// Its scale: physical px to a logical px.
    pub scale: f32,
    /// The model's x and y domains shown (bands in band units).
    pub domain: ((f64, f64), (f64, f64)),
    /// Bars across: `y` is the bands' scale and `x` the values'.
    pub horizontal: bool,
    /// The ticks across the screen, and up it.
    pub x_ticks: Vec<Tick>,
    /// The ticks up the screen.
    pub y_ticks: Vec<Tick>,
    /// The stacks under the view.
    pub stacks: Stacks,
    /// A cell's width: how far past the area's edge the card goes before it flips back
    /// (`CARD_HYSTERESIS`). 10 unless the surface says.
    pub cell_width: f32,
    /// The legend's entries.
    pub items: Vec<LegendItem>,
    /// The end labels drawn.
    pub ends: Vec<EndLabel>,
    /// The chrome's text: title, subtitle, axis titles, tick labels, "+N more".
    pub labels: Vec<Label>,
    /// Where the marks are clipped: the plot, with room for a point's ring where the data
    /// reaches its edge at fit.
    pub clip: Rect,
    /// The bars and their segments, where they're drawn.
    pub bars: Vec<super::marks::BarGeom>,
    /// Whether the card was last put left of its anchor, for its hysteresis.
    pub(crate) flipped: Cell<bool>,
}

impl ChartLayout {
    /// The independent axis's scale (the bands', or x's).
    pub fn ind(&self) -> &Scale {
        if self.horizontal { &self.y } else { &self.x }
    }

    /// The value axis's scale.
    pub fn val(&self) -> &Scale {
        if self.horizontal { &self.x } else { &self.y }
    }

    /// Where a model x falls along the independent axis: a band's centre on a band axis.
    pub fn ix(&self, x: f64) -> f32 {
        let s = self.ind();
        if matches!(s, Scale::Band { .. }) {
            s.map(x + 0.5)
        } else {
            s.map(x)
        }
    }

    /// Where a value falls along the value axis (a band's centre on a band axis).
    pub fn vy(&self, y: f64) -> f32 {
        let s = self.val();
        if matches!(s, Scale::Band { .. }) {
            s.map(y + 0.5)
        } else {
            s.map(y)
        }
    }

    /// A model point on the screen.
    pub fn at(&self, x: f64, y: f64) -> (f32, f32) {
        if self.horizontal {
            (self.vy(y), self.ix(x))
        } else {
            (self.ix(x), self.vy(y))
        }
    }

    /// The model x under a screen point: a band's place in band units on a band axis.
    pub fn data_x(&self, px: f32, py: f32) -> f64 {
        if self.horizontal {
            self.y.invert(py)
        } else {
            self.x.invert(px)
        }
    }

    /// The model value under a screen point.
    pub fn data_y(&self, px: f32, py: f32) -> f64 {
        if self.horizontal {
            self.x.invert(px)
        } else {
            self.y.invert(py)
        }
    }

    /// The band under a screen point, along the independent axis.
    pub fn band_at(&self, px: f32, py: f32) -> Option<usize> {
        if self.horizontal {
            self.y.band_at(py)
        } else {
            self.x.band_at(px)
        }
    }

    /// The plot's length along the independent axis, and the value axis's.
    pub fn lengths(&self) -> (f32, f32) {
        if self.horizontal {
            (self.plot.h, self.plot.w)
        } else {
            (self.plot.w, self.plot.h)
        }
    }
}

/// The area's paddings: across, at the top and at the bottom.
fn paddings(w: f32, h: f32) -> (f32, f32, f32) {
    (
        (w * 0.032).clamp(10.0, 28.0),
        (h * 0.05).clamp(8.0, 24.0),
        (h * 0.035).clamp(6.0, 18.0),
    )
}

/// A scale for an axis of the model over `domain`, onto `range`.
fn scale_for(
    model: &ChartModel,
    kind: AxisKind,
    domain: (f64, f64),
    range: (f32, f32),
    bands: &[Box<str>],
    rect: bool,
) -> Scale {
    match kind {
        AxisKind::Band => {
            let n = bands.len().max(1) as f64;
            let (a, b) = (domain.0.min(domain.1), domain.0.max(domain.1));
            let span = (b - a).max(1e-9);
            let k = (range.1 - range.0) as f64 / span;
            let r0 = range.0 as f64 - a * k;
            let r1 = r0 + n * k;
            let inner = if rect { 0.06 } else { 0.2 };
            Scale::Band {
                domain: bands.to_vec(),
                range: (r0 as f32, r1 as f32),
                inner,
                outer: inner / 2.0,
            }
        }
        AxisKind::Log if domain.0 > 0.0 && domain.1 > 0.0 => Scale::Log {
            domain,
            range,
            base: 10.0,
        },
        AxisKind::Time => Scale::Time {
            domain,
            range,
            zone: model.zone.clone(),
        },
        _ => Scale::Linear { domain, range },
    }
}

/// A tick's label on the value axis: the axis's format, a percentage on a normalised stack,
/// else the scale's.
fn value_label(model: &ChartModel, t: &Tick) -> String {
    if let Some(f) = &model.y.format {
        return format_with(t.value, f, &model.zone);
    }
    if model.series.iter().any(|s| s.stack == Stack::Normalize) {
        return format!("{} %", scale::format_value((t.value * 100.0).round()));
    }
    t.label.clone()
}

/// A tick's label on the independent axis.
fn x_tick_label(model: &ChartModel, t: &Tick) -> String {
    match (&model.x.format, model.x.kind) {
        (Some(f), AxisKind::Linear | AxisKind::Log | AxisKind::Time) => {
            format_with(t.value, f, &model.zone)
        }
        _ => t.label.clone(),
    }
}

/// Whether a chart draws a legend: two or more series that aren't annotations.
fn legend_keys(model: &ChartModel) -> Vec<SeriesKey> {
    if model.chart.view.legend == LegendPos::None || model.is_heatmap() {
        return Vec::new();
    }
    let keys = model.keys();
    if keys.len() < 2 { Vec::new() } else { keys }
}

fn shape_of(model: &ChartModel, key: &SeriesKey) -> KeyShape {
    match model.index_of(key).map(|i| model.series[i].mark) {
        Some(MarkKind::Line) => KeyShape::Line,
        Some(MarkKind::Point) => KeyShape::Dot,
        _ => KeyShape::Square,
    }
}

/// How much a layout attempt keeps: each level drops one more thing.
#[derive(Clone, Copy)]
struct Keep {
    subtitle: bool,
    legend_rows: usize,
    ends: bool,
}

/// The layout of `model` under `view` in an area of `size`.
pub(crate) fn lay_out(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    scale: f32,
    m: &mut dyn Measure,
) -> ChartLayout {
    let levels = [
        Keep {
            subtitle: true,
            legend_rows: 3,
            ends: true,
        },
        Keep {
            subtitle: false,
            legend_rows: 3,
            ends: true,
        },
        Keep {
            subtitle: false,
            legend_rows: 1,
            ends: true,
        },
        Keep {
            subtitle: false,
            legend_rows: 1,
            ends: false,
        },
    ];
    let stacks = model.stacks(view);
    let domain = model.fit(view);
    let mut last = None;
    for keep in levels {
        let l = attempt(model, view, size, scale, keep, &stacks, domain, m);
        let (w, h) = size;
        if l.plot.h >= (0.42 * h).max(40.0).min(h * 0.8) && l.plot.w >= 0.5 * w {
            return l;
        }
        last = Some(l);
    }
    last.expect("there are levels")
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    scale: f32,
    keep: Keep,
    stacks: &Stacks,
    domain: ((f64, f64), (f64, f64)),
    m: &mut dyn Measure,
) -> ChartLayout {
    let (w, h) = size;
    let (pad, pad_top, pad_bottom) = paddings(w, h);
    let title_spec = TextSpec::new(TITLE).weight(Weight::Semibold);
    let body = TextSpec::new(BODY);
    let tick = TextSpec::new(TICK).tabular();
    let tick_plain = TextSpec::new(TICK);
    let mut labels: Vec<Label> = Vec::new();
    let mut title = Rect::default();
    let mut y = pad_top;
    let mut header = false;
    let wide = (w - 2.0 * pad).max(0.0);
    if let Some(t) = model.chart.title.as_deref().filter(|t| !t.is_empty()) {
        let text = m.fit(t, title_spec, wide);
        let base = y + m.ascent(title_spec);
        let l = Label {
            role: Role::Title,
            text,
            x: pad,
            y: base,
            spec: title_spec,
            anchor: Anchor::Start,
            ink: Ink::Strong,
            alpha: 1.0,
        };
        title = l.bounds(m);
        labels.push(l);
        y = base + m.descent(title_spec);
        header = true;
    }
    if keep.subtitle
        && let Some(t) = model.chart.subtitle.as_deref().filter(|t| !t.is_empty())
    {
        let text = m.fit(t, body, wide);
        let base = if header {
            y + 20.0 - m.descent(title_spec)
        } else {
            y + m.ascent(body)
        };
        let l = Label {
            role: Role::Title,
            text,
            x: pad,
            y: base,
            spec: body,
            anchor: Anchor::Start,
            ink: Ink::Muted,
            alpha: 1.0,
        };
        let b = l.bounds(m);
        title = if header { title.union(&b) } else { b };
        labels.push(l);
        y = base + m.descent(body);
        header = true;
    }
    if header {
        y += 10.0;
    }

    // The legend, on top: keys and names wrapped into rows.
    let keys = legend_keys(model);
    let mut items: Vec<LegendItem> = Vec::new();
    let mut legend: Vec<(SeriesKey, Rect)> = Vec::new();
    let right_legend = model.chart.view.legend == LegendPos::Right;
    let mut right_w = 0.0;
    if !keys.is_empty() && !right_legend {
        let max_name = (wide - KEY_W - KEY_GAP).clamp(0.0, 220.0);
        let entries: Vec<(SeriesKey, String, f32)> = keys
            .iter()
            .map(|k| {
                let name = model
                    .series_of(k)
                    .map_or(k.name.to_string(), |s| s.name.clone());
                let name = m.fit(&name, body, max_name);
                let iw = KEY_W + KEY_GAP + m.width(&name, body);
                (k.clone(), name, iw)
            })
            .collect();
        let rows = pack(&entries, pad, w - pad, keep.legend_rows, m, body);
        let more = entries.len() - rows.iter().map(Vec::len).sum::<usize>();
        for (r, row) in rows.iter().enumerate() {
            let top = y + r as f32 * ROW;
            let mid = top + ROW / 2.0;
            let base = mid + m.cap(body) / 2.0;
            for &(i, x) in row {
                let (key, name, iw) = &entries[i];
                let shape = shape_of(model, key);
                let swatch = match shape {
                    KeyShape::Line => Rect::new(x, mid - 1.0, KEY_W, 2.0),
                    KeyShape::Square => Rect::new(x + 2.0, mid - 5.0, 10.0, 10.0),
                    KeyShape::Dot => Rect::new(x + 3.0, mid - 4.0, 8.0, 8.0),
                };
                let hidden = !model.visible(view, key);
                let l = Label {
                    role: Role::Legend,
                    text: name.clone(),
                    x: x + KEY_W + KEY_GAP,
                    y: base,
                    spec: body,
                    anchor: Anchor::Start,
                    ink: Ink::Text,
                    alpha: if hidden { 0.45 } else { 1.0 },
                };
                legend.push((key.clone(), Rect::new(x, top, *iw, ROW).inflate(2.0)));
                items.push(LegendItem {
                    key: key.clone(),
                    shape,
                    swatch,
                    name: l,
                    hidden,
                });
            }
        }
        if more > 0 {
            let last = rows.len().saturating_sub(1);
            let end = rows
                .last()
                .and_then(|r| r.last())
                .map_or(pad, |&(i, x)| x + entries[i].2 + ITEM_GAP);
            let top = y + last as f32 * ROW;
            labels.push(Label {
                role: Role::Legend,
                text: format!("+{more} more"),
                x: end,
                y: top + ROW / 2.0 + m.cap(body) / 2.0,
                spec: body,
                anchor: Anchor::Start,
                ink: Ink::Muted,
                alpha: 1.0,
            });
        }
        y += rows.len() as f32 * ROW;
    } else if !keys.is_empty() {
        right_w = keys
            .iter()
            .map(|k| {
                let name = model
                    .series_of(k)
                    .map_or(k.name.to_string(), |s| s.name.clone());
                KEY_W + KEY_GAP + m.width(&name, body)
            })
            .fold(0.0, f32::max)
            .min(w * 0.25);
    }

    // The y axis's title, above its ticks.
    let axes = model.chart.view.axes;
    let (xd, yd) = domain;
    let vertical_values = !model.horizontal;
    let value_title = if vertical_values {
        &model.y.title
    } else {
        &model.x.title
    };
    let across_title = if vertical_values {
        &model.x.title
    } else {
        &model.y.title
    };
    let mut ytitle: Option<Label> = None;
    if axes && let Some(t) = value_title.as_deref().filter(|t| !t.is_empty()) {
        let gap = if header || !items.is_empty() {
            6.0
        } else {
            0.0
        };
        let base = y + gap + m.ascent(tick_plain);
        ytitle = Some(Label {
            role: Role::AxisTitle,
            text: m.fit(t, tick_plain, wide),
            x: pad,
            y: base,
            spec: tick_plain,
            anchor: Anchor::Start,
            ink: Ink::Muted,
            alpha: 1.0,
        });
        y = base + 12.0 - m.ascent(tick_plain) * 0.0;
    } else {
        // Room for the top tick label's upper half.
        y += if axes {
            m.ascent(tick) * 0.6 + 2.0
        } else {
            4.0
        };
    }
    let plot_top = y.ceil();

    // The bottom: the x labels, then the x title.
    let line = m.ascent(tick) + m.descent(tick);
    let x_title_h = if axes && across_title.as_deref().is_some_and(|t| !t.is_empty()) {
        6.0 + line
    } else {
        0.0
    };
    let labels_h = if axes { TICK_GAP + line } else { 0.0 };
    let plot_bottom = (h - pad_bottom - x_title_h - labels_h)
        .floor()
        .max(plot_top + 1.0);
    let plot_h = plot_bottom - plot_top;

    // Up the screen: the values' ticks, or the bands'.
    let (up_kind, up_domain, up_bands, up_rect) = if vertical_values {
        (model.y.kind, yd, &model.y.bands, model.is_heatmap())
    } else {
        (model.x.kind, xd, &model.x.bands, false)
    };
    let up_scale = scale_for(
        model,
        up_kind,
        up_domain,
        (plot_bottom, plot_top),
        up_bands,
        up_rect,
    );
    let up_scale = if up_kind == AxisKind::Band {
        // Bands go down the screen: the first at the top.
        scale_for(
            model,
            up_kind,
            up_domain,
            (plot_top, plot_bottom),
            up_bands,
            up_rect,
        )
    } else {
        up_scale
    };
    let mut y_ticks: Vec<Tick> = Vec::new();
    let mut y_text: Vec<(String, f32)> = Vec::new();
    if up_kind == AxisKind::Band {
        let step = up_scale.band_of(0).map_or(0.0, |(_, bw)| bw) / 0.8;
        let every = ((14.0 / step.max(0.1)).ceil() as usize).max(1);
        let max_w = w * 0.3;
        for (i, b) in up_bands.iter().enumerate().step_by(every) {
            let py = up_scale.map(i as f64 + 0.5);
            if py < plot_top - 0.5 || py > plot_bottom + 0.5 {
                continue;
            }
            y_ticks.push(Tick {
                value: i as f64 + 0.5,
                label: b.to_string(),
                major: false,
            });
            y_text.push((m.fit(b, tick_plain, max_w), py));
        }
    } else {
        let count = ((plot_h / Y_SPACING).round() as usize).max(2);
        for t in scale::ticks(&up_scale, count) {
            let py = up_scale.map(t.value);
            if py < plot_top - 0.5 || py > plot_bottom + 0.5 {
                continue;
            }
            let text = if vertical_values {
                value_label(model, &t)
            } else {
                x_tick_label(model, &t)
            };
            y_text.push((text, py));
            y_ticks.push(t);
        }
    }
    let ytick_spec = if up_kind == AxisKind::Band {
        tick_plain
    } else {
        tick
    };
    let ytick_w = if axes {
        y_text
            .iter()
            .map(|(t, _)| m.width(t, ytick_spec))
            .fold(0.0, f32::max)
    } else {
        0.0
    };
    let plot_x = (pad + if axes { ytick_w + TICK_GAP } else { 0.0 }).ceil();

    // Right: end labels for a few lines, when they fit and don't collide.
    let mut ends: Vec<EndLabel> = Vec::new();
    let labels_rule = model.chart.view.labels;
    let lines: Vec<usize> = model
        .series
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            matches!(s.mark, MarkKind::Line | MarkKind::Area) && model.visible(view, &s.key)
        })
        .map(|(i, _)| i)
        .collect();
    let want_ends = keep.ends
        && vertical_values
        && !lines.is_empty()
        && match labels_rule {
            crate::spec::Labels::None => false,
            crate::spec::Labels::Ends => true,
            crate::spec::Labels::Auto => lines.len() <= 4 && legend_keys(model).len() >= 2,
        };
    let mut right = w
        - pad
        - if right_legend && right_w > 0.0 {
            right_w + 16.0
        } else {
            0.0
        };
    if want_ends {
        let (a, b) = (xd.0.min(xd.1), xd.0.max(xd.1));
        let mut cands: Vec<(usize, f64, f64)> = Vec::new();
        for &si in &lines {
            let s = &model.series[si];
            let q = s.xs.partition_point(|&x| x <= b);
            let last = (0..q).rev().find(|&i| s.xs[i] >= a && !s.ys[i].is_nan());
            if let Some(i) = last {
                cands.push((si, model.top(stacks, si, i), s.ys[i]));
            }
        }
        let mut ys: Vec<(f32, usize)> = cands
            .iter()
            .enumerate()
            .map(|(k, c)| (up_scale.map(c.1), k))
            .collect();
        ys.sort_by(|a, b| a.0.total_cmp(&b.0));
        let clear = m.ascent(body) + m.descent(body) + 1.0;
        let collide = ys.windows(2).any(|p| p[1].0 - p[0].0 < clear)
            || ys
                .iter()
                .any(|(py, _)| *py < plot_top - 4.0 || *py > plot_bottom + 4.0);
        if !collide && !cands.is_empty() {
            let vspec = TextSpec::new(BODY).tabular();
            let widths: Vec<(String, String, f32, f32)> = cands
                .iter()
                .map(|&(si, _, v)| {
                    let name = m.fit(&model.series[si].name, body, w * 0.18);
                    let value = model.value_label(v, model.series[si].decimals);
                    let (nw, vw) = (m.width(&name, body), m.width(&value, vspec));
                    (name, value, nw, vw)
                })
                .collect();
            let need = widths
                .iter()
                .map(|(_, _, nw, vw)| nw + 6.0 + vw)
                .fold(0.0, f32::max);
            let candidate = w - (pad * 0.5).max(8.0) - need - 10.0;
            if candidate - plot_x >= 0.55 * w {
                right = candidate.min(right);
                for (k, &(si, top, _)) in cands.iter().enumerate() {
                    let (name, value, nw, _) = &widths[k];
                    let base = up_scale.map(top) + m.cap(body) / 2.0;
                    let x0 = right + 10.0;
                    ends.push(EndLabel {
                        key: model.series[si].key.clone(),
                        name: Label {
                            role: Role::End,
                            text: name.clone(),
                            x: x0,
                            y: base,
                            spec: body,
                            anchor: Anchor::Start,
                            ink: Ink::Text,
                            alpha: 1.0,
                        },
                        value: Label {
                            role: Role::End,
                            text: value.clone(),
                            x: x0 + nw + 6.0,
                            y: base,
                            spec: vspec,
                            anchor: Anchor::Start,
                            ink: Ink::Muted,
                            alpha: 1.0,
                        },
                    });
                }
            }
        }
    }
    let plot_right = right.floor().max(plot_x + 1.0);
    let plot = Rect::new(plot_x, plot_top, plot_right - plot_x, plot_h);

    // Across the screen: the independent axis (or the values for horizontal bars).
    let (across_kind, across_domain, across_bands) = if vertical_values {
        (model.x.kind, xd, &model.x.bands)
    } else {
        (model.y.kind, yd, &model.y.bands)
    };
    let across = scale_for(
        model,
        across_kind,
        across_domain,
        (plot.x, plot.right()),
        across_bands,
        model.is_heatmap(),
    );
    let mut x_ticks: Vec<Tick> = Vec::new();
    let base_y = plot.bottom() + TICK_GAP + m.ascent(tick);
    if axes {
        let placed = if across_kind == AxisKind::Band {
            band_labels(&across, across_bands, &plot, tick_plain, m)
        } else {
            value_ticks(model, &across, &plot, tick, vertical_values, w, m)
        };
        for (t, text, x, anchor) in placed {
            let spec = if across_kind == AxisKind::Band {
                tick_plain
            } else {
                tick
            };
            labels.push(Label {
                role: Role::XTick,
                text,
                x,
                y: base_y,
                spec,
                anchor,
                ink: Ink::Muted,
                alpha: 1.0,
            });
            x_ticks.push(t);
        }
        for (text, py) in &y_text {
            labels.push(Label {
                role: Role::YTick,
                text: text.clone(),
                x: plot.x - TICK_GAP,
                y: py + m.cap(ytick_spec) / 2.0,
                spec: ytick_spec,
                anchor: Anchor::End,
                ink: Ink::Muted,
                alpha: 1.0,
            });
        }
        if let Some(mut l) = ytitle {
            // Over the ticks' column, right-aligned with it, unless it would pass the padding.
            let tw = m.width(&l.text, tick_plain);
            if plot.x - TICK_GAP - tw >= pad * 0.5 {
                l.x = plot.x - TICK_GAP;
                l.anchor = Anchor::End;
            }
            labels.push(l);
        }
        if let Some(t) = across_title.as_deref().filter(|t| !t.is_empty()) {
            let b = base_y + m.descent(tick) + 6.0 + m.ascent(tick_plain);
            labels.push(Label {
                role: Role::AxisTitle,
                text: m.fit(t, tick_plain, plot.w),
                x: plot.right(),
                y: b,
                spec: tick_plain,
                anchor: Anchor::End,
                ink: Ink::Muted,
                alpha: 1.0,
            });
        }
    }
    let x_axis = Rect::new(plot.x, plot.bottom(), plot.w, (h - plot.bottom()).max(0.0));
    let y_axis = Rect::new(0.0, plot.y, plot.x, plot.h);

    // The legend on the right, a key a row from the plot's top.
    if right_legend && !keys.is_empty() {
        let x0 = plot.right() + 16.0;
        let fit_rows = ((plot.h / ROW).floor() as usize).max(1);
        let shown = if keys.len() > fit_rows {
            fit_rows.saturating_sub(1)
        } else {
            keys.len()
        };
        for (r, k) in keys.iter().take(shown).enumerate() {
            let top = plot.y + r as f32 * ROW;
            let mid = top + ROW / 2.0;
            let shape = shape_of(model, k);
            let swatch = match shape {
                KeyShape::Line => Rect::new(x0, mid - 1.0, KEY_W, 2.0),
                KeyShape::Square => Rect::new(x0 + 2.0, mid - 5.0, 10.0, 10.0),
                KeyShape::Dot => Rect::new(x0 + 3.0, mid - 4.0, 8.0, 8.0),
            };
            let name = model
                .series_of(k)
                .map_or(k.name.to_string(), |s| s.name.clone());
            let name = m.fit(&name, body, (w - pad - x0 - KEY_W - KEY_GAP).max(0.0));
            let hidden = !model.visible(view, k);
            let iw = KEY_W + KEY_GAP + m.width(&name, body);
            legend.push((k.clone(), Rect::new(x0, top, iw, ROW).inflate(2.0)));
            items.push(LegendItem {
                key: k.clone(),
                shape,
                swatch,
                name: Label {
                    role: Role::Legend,
                    text: name,
                    x: x0 + KEY_W + KEY_GAP,
                    y: mid + m.cap(body) / 2.0,
                    spec: body,
                    anchor: Anchor::Start,
                    ink: Ink::Text,
                    alpha: if hidden { 0.45 } else { 1.0 },
                },
                hidden,
            });
        }
        if shown < keys.len() {
            let top = plot.y + shown as f32 * ROW;
            labels.push(Label {
                role: Role::Legend,
                text: format!("+{} more", keys.len() - shown),
                x: x0,
                y: top + ROW / 2.0 + m.cap(body) / 2.0,
                spec: body,
                anchor: Anchor::Start,
                ink: Ink::Muted,
                alpha: 1.0,
            });
        }
    }

    // The marks bleed past the plot's edges where the data reaches them at fit.
    let (xs_scale, ys_scale) = if vertical_values {
        (across, up_scale)
    } else {
        (up_scale, across)
    };
    let fit_x = view.x.is_none();
    let fit_y = view.y.is_none();
    let (bx, by) = if vertical_values {
        (fit_x, fit_y)
    } else {
        (fit_y, fit_x)
    };
    let clip = Rect::new(
        plot.x - if bx { BLEED } else { 0.0 },
        plot.y - if by { BLEED } else { 0.0 },
        plot.w + if bx { 2.0 * BLEED } else { 0.0 },
        plot.h + if by { BLEED } else { 0.0 },
    );
    ChartLayout {
        plot,
        x: xs_scale,
        y: ys_scale,
        legend,
        x_axis,
        y_axis,
        title,
        size,
        scale,
        domain,
        horizontal: model.horizontal,
        x_ticks,
        y_ticks,
        stacks: stacks.clone(),
        cell_width: 10.0,
        items,
        ends,
        labels,
        clip,
        bars: Vec::new(),
        flipped: Cell::new(false),
    }
}

/// Legend entries packed into at most `rows` rows between `x0` and `x1`: each row's entries
/// as (index, left). Entries that don't fit leave room on the last row for "+N more".
fn pack(
    entries: &[(SeriesKey, String, f32)],
    x0: f32,
    x1: f32,
    rows: usize,
    m: &mut dyn Measure,
    body: TextSpec,
) -> Vec<Vec<(usize, f32)>> {
    let mut out: Vec<Vec<(usize, f32)>> = vec![Vec::new()];
    let mut x = x0;
    for (i, e) in entries.iter().enumerate() {
        if x + e.2 > x1 && x > x0 {
            if out.len() == rows {
                break;
            }
            out.push(Vec::new());
            x = x0;
        }
        out.last_mut().expect("a row").push((i, x));
        x += e.2 + ITEM_GAP;
    }
    let placed: usize = out.iter().map(Vec::len).sum();
    if placed < entries.len() {
        // Room for "+N more" at the end of the last row.
        loop {
            let left = entries.len() - out.iter().map(Vec::len).sum::<usize>();
            let more = m.width(&format!("+{left} more"), body);
            let row = out.last_mut().expect("a row");
            let end = row.last().map_or(x0, |&(i, x)| x + entries[i].2 + ITEM_GAP);
            if end + more <= x1 || row.len() <= 1 {
                break;
            }
            row.pop();
        }
    }
    out
}

/// The labels of a band axis across the plot: each band's name, cut to its step, or every
/// nth when even short names won't fit.
fn band_labels(
    s: &Scale,
    bands: &[Box<str>],
    plot: &Rect,
    spec: TextSpec,
    m: &mut dyn Measure,
) -> Vec<(Tick, String, f32, Anchor)> {
    let step = s.band_of(0).map_or(0.0, |(_, bw)| bw) / 0.8;
    let widest = bands.iter().map(|b| m.width(b, spec)).fold(0.0, f32::max);
    let every = if widest + 8.0 <= step || step >= 44.0 {
        1
    } else {
        let per = (widest.min(80.0) + X_CLEAR) / step.max(0.1);
        (per.ceil() as usize).max(1)
    };
    let room = (step * every as f32 - 8.0).max(0.0);
    let mut out = Vec::new();
    for (i, b) in bands.iter().enumerate().step_by(every) {
        let px = s.map(i as f64 + 0.5);
        if px < plot.x - 0.5 || px > plot.right() + 0.5 {
            continue;
        }
        let text = m.fit(b, spec, room);
        if text.is_empty() {
            continue;
        }
        out.push((
            Tick {
                value: i as f64 + 0.5,
                label: b.to_string(),
                major: false,
            },
            text,
            px,
            Anchor::Middle,
        ));
    }
    out
}

/// The ticks of a number or time axis across the plot: as many as keep labels 64 px apart and
/// clear of each other, the end ones kept inside the area.
fn value_ticks(
    model: &ChartModel,
    s: &Scale,
    plot: &Rect,
    spec: TextSpec,
    independent: bool,
    area_w: f32,
    m: &mut dyn Measure,
) -> Vec<(Tick, String, f32, Anchor)> {
    let mut count = ((plot.w / 90.0).round() as usize).max(2);
    loop {
        let mut placed: Vec<(Tick, String, f32, Anchor, Rect)> = Vec::new();
        for t in scale::ticks(s, count) {
            let px = s.map(t.value);
            if px < plot.x - 0.5 || px > plot.right() + 0.5 {
                continue;
            }
            let text = if independent {
                x_tick_label(model, &t)
            } else {
                value_label(model, &t)
            };
            let tw = m.width(&text, spec);
            let (x, anchor) = if px + tw / 2.0 > area_w - 2.0 {
                (area_w - 2.0, Anchor::End)
            } else if px - tw / 2.0 < 2.0 {
                (2.0, Anchor::Start)
            } else {
                (px, Anchor::Middle)
            };
            let left = match anchor {
                Anchor::Start => x,
                Anchor::Middle => x - tw / 2.0,
                Anchor::End => x - tw,
            };
            placed.push((t, text, x, anchor, Rect::new(left, 0.0, tw, 1.0)));
        }
        let ok = placed.windows(2).all(|p| {
            let (a, b) = (&p[0], &p[1]);
            s.map(b.0.value) - s.map(a.0.value) >= X_SPACING && b.4.x - a.4.right() >= X_CLEAR
        });
        if ok || count <= 2 {
            let mut out: Vec<(Tick, String, f32, Anchor)> = placed
                .into_iter()
                .map(|(t, l, x, a, _)| (t, l, x, a))
                .collect();
            if !ok {
                // Even two collide: keep the ends' first.
                out.truncate(1);
            }
            return out;
        }
        count -= 1;
    }
}

/// The chrome under the marks and around them: grid, baseline, the legend, titles and ticks.
pub(crate) fn chrome(
    model: &ChartModel,
    view: &View,
    l: &ChartLayout,
    theme: &Theme,
    scene: &mut Scene,
    _m: &mut dyn Measure,
) {
    let p = l.plot;
    let grid = model.chart.view.grid && !model.is_heatmap();
    if grid {
        // Hairlines along the value axis's ticks: across the plot, or up it for bars across.
        if model.horizontal {
            for t in &l.x_ticks {
                let x = l.x.map(t.value);
                hair(scene, (x, p.y), (x, p.bottom()), theme.grid);
            }
        } else {
            for t in &l.y_ticks {
                let y = l.y.map(t.value);
                if (y - p.bottom()).abs() < 0.5 {
                    continue;
                }
                hair(scene, (p.x, y), (p.right(), y), theme.grid);
            }
        }
    }
    if model.chart.view.axes && !model.is_heatmap() {
        let val = l.val();
        let (lo, hi) = val.domain();
        let zero = lo.min(hi) <= 0.0
            && lo.max(hi) >= 0.0
            && !matches!(val, Scale::Log { .. } | Scale::Band { .. });
        if model.horizontal {
            let x = if zero { l.x.map(0.0) } else { p.x };
            hair(scene, (x, p.y), (x, p.bottom()), theme.baseline);
        } else {
            let y = if zero { l.y.map(0.0) } else { p.bottom() };
            hair(scene, (p.x, y), (p.right(), y), theme.baseline);
        }
    }
    for item in &l.items {
        let color = model.color_of_key(&item.key, theme);
        legend_key(scene, item, color, theme);
        scene.push(item.name.item(theme));
    }
    for label in &l.labels {
        scene.push(label.item(theme));
    }
    for e in &l.ends {
        scene.push(e.name.item(theme));
        scene.push(e.value.item(theme));
    }
    let _ = view;
}

/// A legend key: a line, a rounded square or a dot; hollow when its series is hidden.
pub fn legend_key(scene: &mut Scene, item: &LegendItem, color: Color, theme: &Theme) {
    let r = item.swatch;
    let (fill, stroke) = if item.hidden {
        (None, Some(Stroke::solid(1.0, theme.muted, 0.8)))
    } else {
        (Some(Paint::solid(color, 1.0)), None)
    };
    let path = match item.shape {
        KeyShape::Line if item.hidden => PathData::rounded(r.inflate(0.5), [1.0; 4]),
        KeyShape::Line => PathData::rounded(r, [1.0; 4]),
        KeyShape::Square => PathData::rounded(r, [2.5; 4]),
        KeyShape::Dot => PathData::circle(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0),
    };
    scene.push(Item::Path { path, fill, stroke });
}

/// A one-logical-px line, snapped to whole physical px.
pub fn hair(scene: &mut Scene, a: (f32, f32), b: (f32, f32), color: Color) {
    scene.push(Item::Path {
        path: PathData::polyline(&[a, b]),
        fill: None,
        stroke: Some(Stroke::hairline(color, 1.0)),
    });
}

/// The regions under the marks: the whole area as chrome, then the plot.
pub(crate) fn regions_under(l: &ChartLayout, scene: &mut Scene) {
    scene.region(Rect::new(0.0, 0.0, l.size.0, l.size.1), Hit::Chrome);
    scene.region(l.plot, Hit::Plot(0));
}

/// The regions over the marks: the axes' strips, the legend's keys and the title.
pub(crate) fn regions(model: &ChartModel, l: &ChartLayout, scene: &mut Scene) {
    if model.chart.view.axes {
        scene.region(l.x_axis, Hit::AxisX(0));
        scene.region(l.y_axis, Hit::AxisY(0));
    }
    for (k, r) in &l.legend {
        scene.region(*r, Hit::LegendKey(k.clone()));
    }
    if !l.title.is_empty() {
        scene.region(l.title, Hit::PanelTitle(0));
    }
}

/// The error banner over the top of the area (a parse error with its line and column, a missing
/// file): drawn by the surface onto the base when the last parse failed but a figure is kept.
pub fn banner(scene: &mut Scene, text: &str, theme: &Theme, m: &mut dyn Measure) {
    let w = scene.width;
    let inset = 8.0f32.min(w / 8.0);
    let spec = TextSpec::new(BODY);
    let h = (m.ascent(spec) + m.descent(spec) + 14.0).round();
    let r = Rect::new(inset, inset, (w - 2.0 * inset).max(0.0), h);
    scene.push(Item::Shadow {
        rect: r,
        radius: 6.0,
        blur: 8.0,
        dy: 2.0,
        alpha: if theme.dark { 0.5 } else { 0.18 },
    });
    scene.push(Item::Path {
        path: PathData::rounded(r, [6.0; 4]),
        fill: Some(Paint::solid(theme.tooltip, 1.0)),
        stroke: Some(Stroke::hairline(theme.ring, 1.0)),
    });
    let bar = Rect::new(r.x, r.y, 3.0, r.h);
    scene.push(Item::Path {
        path: PathData::rounded(bar, [6.0, 0.0, 0.0, 6.0]),
        fill: Some(Paint::solid(theme.status[3], 1.0)),
        stroke: None,
    });
    let x = r.x + 14.0;
    let fitted = m.fit(text, spec, (r.right() - 12.0 - x).max(0.0));
    scene.push(Item::Text {
        text: fitted.into(),
        x,
        y: r.y + r.h / 2.0 + m.cap(spec) / 2.0,
        spec,
        anchor: Anchor::Start,
        color: theme.text,
        alpha: 1.0,
        max_width: None,
    });
    scene.region(r, Hit::Banner);
}

/// What an empty or failed figure shows in place of a chart ("No data in requests.csv").
pub fn empty(
    size: (f32, f32),
    scale: f32,
    text: &str,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(size.0, size.1, scale);
    scene.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
    let spec = TextSpec::new(13.0);
    let fitted = m.fit(text, spec, (size.0 - 24.0).max(0.0));
    scene.push(Item::Text {
        text: fitted.into(),
        x: size.0 / 2.0,
        y: size.1 / 2.0 + m.cap(spec) / 2.0,
        spec,
        anchor: Anchor::Middle,
        color: theme.muted,
        alpha: 1.0,
        max_width: None,
    });
    scene
}
