//! The chart's anatomy: the title, the legend, the axes and the plot's rectangle, at a size.
//!
//! From the top: the title (15 px semibold), the subtitle (12 px muted), the legend under them
//! (a key and a name each, wrapped to rows; none for one series), the y axis's title above its
//! ticks, then the plot with its ticks (11 px muted tabular figures, y about every 48 px, x
//! labels at least 64 px apart) and the x axis's title under the x labels. End labels take a
//! column right of the plot when they fit. Small areas degrade in order: the subtitle goes, then
//! the legend keeps one row with "+N more", then the end labels, then (where the plot would
//! still be a sliver: beside the detail panel, in a tile) the legend, the axes' titles and the
//! title, so nothing overlaps and nothing is cut at the area's edge.

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
/// The least room between an end x label and the area's edge.
const EDGE: f32 = 4.0;
/// About how far apart y ticks are, and the least. A grid from 48 to 64 px apart is calm; one
/// denser reads busier than one as much sparser (`DENSE_COST`).
const Y_SPACING: f32 = 48.0;
const Y_CALM: f32 = 64.0;
const Y_MIN_GAP: f32 = 32.0;
const DENSE_COST: f32 = 1.25;
/// The most ticks an axis asks for however much room it has: a tall plot's grid opens up
/// rather than filling with lines, a wide one's labels rather than crowding.
const Y_MOST: usize = 10;
const X_MOST: usize = 12;
/// A plot shorter than this whose fitted y would reach past the data by more than this share
/// of it ends at the data's top instead of a tick.
const SHORT_PLOT: f32 = 120.0;
const SHORT_HEADROOM: f64 = 1.3;
/// What a domain's headroom over the tightest one costs, against a grid's calm.
const HEADROOM_COST: f32 = 4.0;
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
    /// The model's fit under the view the layout is of ([`ChartModel::fit`]), which `domain`
    /// refines: y niced to the plot's ticks, a scatter's padded for its points.
    pub fit: ((f64, f64), (f64, f64)),
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
    /// The bands' totals past their bars, where they're drawn: each band and its label's box.
    pub totals: Vec<(u32, Rect)>,
    /// Whether the card was last put left of its anchor, for its hysteresis.
    pub(crate) flipped: Cell<bool>,
    /// A heatmap's colour legend, where it's drawn.
    pub ramp: Option<super::ramp::RampLegend>,
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

/// Paddings a caller gives in place of the area's own: a dashboard's panel, whose frame and
/// title are its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    /// Across, each side.
    pub x: f32,
    /// At the top: where the legend, or the plot, starts.
    pub top: f32,
    /// At the bottom.
    pub bottom: f32,
    /// Whether end labels show where the chart's own rule would put them (`labels` auto);
    /// false keeps them to a spec that asks (`"labels": "ends"`), as a panel's legend says the
    /// same in less room.
    pub ends: bool,
}

/// The area's paddings: across, at the top and at the bottom.
pub(crate) fn paddings(w: f32, h: f32) -> (f32, f32, f32) {
    (
        (w * 0.032).clamp(10.0, 28.0),
        (h * 0.05).clamp(8.0, 24.0),
        (h * 0.045).clamp(6.0, 22.0),
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
            // A heatmap's cells fill their slots, each keeping its own 2 px gap.
            let inner = if rect { 0.0 } else { 0.2 };
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

/// What a dashboard panel's chart leaves over its top tick, past a chart's own room (m4's).
const PANEL_GAP: f32 = 7.0;

/// The least plot a layout keeps by dropping the legend, the axes' titles and the title.
const MIN_PLOT: f32 = 48.0;

/// How much a layout attempt keeps: each level drops one more thing.
#[derive(Clone, Copy)]
struct Keep {
    subtitle: bool,
    /// The legend's rows at most; none, no legend.
    legend_rows: usize,
    ends: bool,
    /// The axes' titles.
    axis_titles: bool,
    /// A heatmap's colour legend, which outlasts the series legend and the axes' titles (its
    /// colours can't be read without it).
    ramp: bool,
    title: bool,
}

/// The layout of `model` under `view` in an area of `size`, its paddings across those of an
/// area `pad_w` wide (the whole window's beside the detail panel, so nothing moves as the
/// panel opens).
pub(crate) fn lay_out(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
    pad_w: f32,
    insets: Option<Insets>,
    scale: f32,
    m: &mut dyn Measure,
) -> ChartLayout {
    let full = Keep {
        subtitle: true,
        legend_rows: 3,
        ends: true,
        axis_titles: true,
        ramp: true,
        title: true,
    };
    let levels = [
        full,
        Keep {
            subtitle: false,
            ..full
        },
        Keep {
            subtitle: false,
            legend_rows: 1,
            ..full
        },
        Keep {
            subtitle: false,
            legend_rows: 1,
            ends: false,
            ..full
        },
        Keep {
            subtitle: false,
            legend_rows: 0,
            ends: false,
            ..full
        },
        Keep {
            subtitle: false,
            legend_rows: 0,
            ends: false,
            axis_titles: false,
            ..full
        },
        Keep {
            subtitle: false,
            legend_rows: 0,
            ends: false,
            axis_titles: false,
            ramp: false,
            ..full
        },
        Keep {
            subtitle: false,
            legend_rows: 0,
            ends: false,
            axis_titles: false,
            ramp: false,
            title: false,
        },
    ];
    let stacks = model.stacks(view);
    let domain = model.fit(view);
    let mut last = None;
    for (n, keep) in levels.into_iter().enumerate() {
        let l = attempt(
            model, view, size, pad_w, insets, scale, keep, &stacks, domain, m,
        );
        let (w, h) = size;
        // The first four levels hold out for a good share of the area; past them, only for a
        // plot that can be read at all.
        let enough = match n {
            0..4 => (0.42 * h).max(40.0).min(h * 0.8),
            _ => MIN_PLOT.min(h * 0.5),
        };
        if l.plot.h >= enough && (l.plot.w >= 0.5 * w || n >= 4) {
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
    pad_w: f32,
    insets: Option<Insets>,
    scale: f32,
    keep: Keep,
    stacks: &Stacks,
    domain: ((f64, f64), (f64, f64)),
    m: &mut dyn Measure,
) -> ChartLayout {
    let (w, h) = size;
    let (pad, pad_top, pad_bottom) = match insets {
        Some(i) => (i.x, i.top, i.bottom),
        None => {
            let (_, top, bottom) = paddings(w, h);
            let pad = paddings(pad_w.max(w), h)
                .0
                .min(w * 0.06)
                .max(paddings(w, h).0);
            (pad, top, bottom)
        }
    };
    let title_spec = TextSpec::new(TITLE).weight(Weight::Semibold);
    let body = TextSpec::new(BODY);
    let tick = TextSpec::new(TICK).tabular();
    let tick_plain = TextSpec::new(TICK);
    let mut labels: Vec<Label> = Vec::new();
    let mut title = Rect::default();
    let mut y = pad_top;
    let mut header = false;
    let wide = (w - 2.0 * pad).max(0.0);
    if let Some(t) = (model.chart.title.as_deref())
        .filter(|t| !t.is_empty())
        .filter(|_| keep.title)
    {
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
    if !keys.is_empty() && !right_legend && keep.legend_rows > 0 {
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
    } else if !keys.is_empty() && keep.legend_rows > 0 {
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

    // A heatmap's colour legend, where the legend goes: under the title, or (placed once the
    // plot is) at the right.
    let ramp_scale = (keep.ramp && model.chart.view.legend != LegendPos::None)
        .then(|| super::ramp::scale_of(model))
        .flatten();
    let mut ramp = None;
    if let Some(sc) = ramp_scale {
        if right_legend {
            right_w = super::ramp::RAMP_W.min(w * 0.25);
        } else if let Some(r) = super::ramp::RampLegend::place(model, sc, pad, y, wide, true, m) {
            y = r.bounds.bottom().max(y);
            ramp = Some(r);
        }
    }

    // The y axis's title, above its ticks. With every series hidden there's nothing to read
    // off an axis: no ticks, no grid, the plot left to say so.
    let axes = model.chart.view.axes && !nothing_shown(model, view);
    let (xd, mut yd) = domain;
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
    if axes
        && keep.axis_titles
        && let Some(t) = value_title.as_deref().filter(|t| !t.is_empty())
    {
        let gap = if header || !items.is_empty() || ramp.is_some() {
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
        // Room for the top tick label's upper half; in a dashboard's panel, as much again as
        // m4 leaves under its legend or subtitle, so the top tick doesn't crowd them.
        y += if axes {
            m.ascent(tick) * 0.6 + 2.0
        } else {
            4.0
        };
        if insets.is_some() && axes {
            y += PANEL_GAP;
        }
    }
    let plot_top = y.ceil();

    // The bottom: the x labels, then the x title.
    let line = m.ascent(tick) + m.descent(tick);
    let x_title_h =
        if axes && keep.axis_titles && across_title.as_deref().is_some_and(|t| !t.is_empty()) {
            6.0 + line
        } else {
            0.0
        };
    let labels_h = if axes { TICK_GAP + line } else { 0.0 };
    let plot_bottom = (h - pad_bottom - x_title_h - labels_h)
        .floor()
        .max(plot_top + 1.0);
    let plot_h = plot_bottom - plot_top;
    let whole = model.whole_values(view);
    // A fitted y is niced for the ticks the plot has room for, so its top is a tick: of the
    // counts near the room's, the one whose grid is calmest (48 to 64 px apart), its headroom
    // over the tightest counting against it; whole steps only for whole numbers.
    let mut y_count = ((plot_h / Y_SPACING).round() as usize).clamp(2, Y_MOST);
    if vertical_values
        && view.y.is_none()
        && model.y.domain.is_none()
        && model.y.kind == AxisKind::Linear
        && (model.is_x_chart() || model.is_band())
    {
        let mut cands: Vec<((f64, f64), usize, f32, f64)> = Vec::new();
        for c in 2..=y_count + 3 {
            let d = model.fit_ticks(view, c).1;
            let span = d.1 - d.0;
            let s = Scale::Linear {
                domain: d,
                range: (plot_bottom, plot_top),
            };
            let ts = scale::ticks(&s, c);
            if ts.len() < 2 || span.is_nan() || span <= 0.0 {
                continue;
            }
            let step = (ts[1].value - ts[0].value).abs();
            if whole && step < 1.0 - 1e-9 {
                continue;
            }
            let gap = plot_h * (step / span) as f32;
            if gap >= Y_MIN_GAP {
                cands.push((d, c, gap, span));
            }
        }
        let tightest = cands.iter().map(|c| c.3).fold(f64::INFINITY, f64::min);
        let cost = |&(_, _, gap, span): &((f64, f64), usize, f32, f64)| -> f32 {
            let grid = if gap < Y_SPACING {
                DENSE_COST * (Y_SPACING / gap).ln()
            } else if gap > Y_CALM {
                (gap / Y_CALM).ln()
            } else {
                0.0
            };
            grid + HEADROOM_COST * (span / tightest).ln() as f32
        };
        let best = cands.iter().min_by(|a, b| {
            cost(a)
                .total_cmp(&cost(b))
                .then(a.3.total_cmp(&b.3))
                .then((a.2 - Y_SPACING).abs().total_cmp(&(b.2 - Y_SPACING).abs()))
        });
        if let Some(&(d, c, _, _)) = best {
            yd = loosened(model, view, d, c, (plot_bottom, plot_top), whole);
            y_count = c;
        }
        // A short plot whose calmest grid would leave much of it empty over the data (an 84 px
        // panel's 0 to 2,000 for data to 1,060): the data's own top, a little over, its ticks
        // the nice ones under it, so the marks keep the plot's height.
        let (dlo, dhi) = domain.1;
        let floor = yd.0.min(dlo);
        if plot_h < SHORT_PLOT
            && dhi > floor
            && dhi.is_finite()
            && (yd.1 - floor) > SHORT_HEADROOM * (dhi - floor)
        {
            yd = (yd.0, dhi + (dhi - floor) * 0.04);
            y_count = 2;
        }
    }
    // A scatter's points keep their whole dot inside the plot at its fit, and its top tick
    // labels most of them.
    let reach = point_reach(model, view);
    if model.is_scatter()
        && view.y.is_none()
        && model.y.domain.is_none()
        && let Some(ext) = points_extent(model, view, false)
    {
        yd = pad_for(yd, ext, plot_h, reach, model.y.kind);
        if model.y.kind == AxisKind::Linear {
            let range = (plot_bottom, plot_top);
            yd = to_next_tick(yd, ext, y_count, range, whole);
        }
    }
    // A plot too short to pad by the dots' reach lets them past its edges, whole.
    let dots_bleed = model.is_scatter() && plot_h <= 4.0 * reach;

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
        let count = if vertical_values {
            y_count
        } else {
            ((plot_h / Y_SPACING).round() as usize).clamp(2, Y_MOST)
        };
        let floor = no_negatives(model, view, false);
        for t in whole_ticks(&up_scale, count, vertical_values && whole) {
            if floor && t.value < 0.0 {
                continue;
            }
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
        // In a short plot the ticks come closer than a line of their text: they drop from the
        // bottom up until each clears the one below it (call 1432).
        let clear = m.ascent(tick) + m.descent(tick) + 1.0;
        let mut order: Vec<usize> = (0..y_text.len()).collect();
        order.sort_by(|a, b| y_text[*b].1.total_cmp(&y_text[*a].1));
        let mut kept = vec![false; y_text.len()];
        let mut last: Option<f32> = None;
        for i in order {
            let py = y_text[i].1;
            if last.is_none_or(|l| l - py >= clear) {
                kept[i] = true;
                last = Some(py);
            }
        }
        let mut k = kept.iter();
        y_text.retain(|_| *k.next().unwrap_or(&true));
        let mut k = kept.iter();
        y_ticks.retain(|_| *k.next().unwrap_or(&true));
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
            crate::spec::Labels::Auto => {
                insets.is_none_or(|i| i.ends) && lines.len() <= 4 && legend_keys(model).len() >= 2
            }
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
            // An end label is read against the axis: one decimal past its ticks' at most (the
            // card keeps the data's own precision).
            let tick_decimals = match y_ticks.as_slice() {
                [a, b, ..] => scale::data_decimals(&[(b.value - a.value).abs()]),
                _ => 6,
            };
            let widths: Vec<(String, String, f32, f32)> = cands
                .iter()
                .map(|&(si, _, v)| {
                    let name = m.fit(&model.series[si].name, body, w * 0.18);
                    let decimals = model.series[si].decimals.min(tick_decimals + 1);
                    let value = model.value_label(v, decimals);
                    let (nw, vw) = (m.width(&name, body), m.width(&value, vspec));
                    (name, value, nw, vw)
                })
                .collect();
            let need = widths
                .iter()
                .map(|(_, _, nw, vw)| nw + 6.0 + vw)
                .fold(0.0, f32::max);
            // The labels end at the padding, as the title starts at it on the left.
            let candidate = w - pad.max(8.0) - need - 10.0;
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
    let mut xd = xd;
    if !vertical_values
        && view.y.is_none()
        && model.y.domain.is_none()
        && model.y.kind == AxisKind::Linear
    {
        yd = across_values(model, view, &plot, m).unwrap_or(yd);
    }
    if model.is_scatter()
        && view.x.is_none()
        && model.x.domain.is_none()
        && let Some(ext) = points_extent(model, view, true)
    {
        xd = pad_for(xd, ext, plot.w, reach, model.x.kind);
    }
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
        let mut placed = if across_kind == AxisKind::Band {
            band_labels(&across, across_bands, &plot, tick_plain, m)
        } else {
            value_ticks(model, &across, &plot, tick, vertical_values, whole, w, m)
        };
        if no_negatives(model, view, true) {
            placed.retain(|t| t.0.value >= 0.0);
        }
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
            // Over the ticks' column, right-aligned with it, where it fits in the column; a
            // wider one starts at the padding with the title and the legend.
            let tw = m.width(&l.text, tick_plain);
            if plot.x - TICK_GAP - tw >= pad - 1.0 {
                l.x = plot.x - TICK_GAP;
                l.anchor = Anchor::End;
            }
            labels.push(l);
        }
        if let Some(t) = (across_title.as_deref())
            .filter(|t| !t.is_empty())
            .filter(|_| keep.axis_titles)
        {
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
    if right_legend && !keys.is_empty() && keep.legend_rows > 0 {
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

    // The colour legend at the right, from the plot's top.
    if right_legend && let Some(sc) = ramp_scale {
        let x0 = plot.right() + 16.0;
        ramp = super::ramp::RampLegend::place(model, sc, x0, plot.y, w - pad - x0, false, m);
    }

    // The marks bleed past the plot's edges where the data reaches them at fit. The scales
    // are the screen's: across it, and up it.
    let (xs_scale, ys_scale) = (across, up_scale);
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
    let clip = if dots_bleed {
        let top = clip.y.min(plot.y - reach);
        Rect::new(clip.x, top, clip.w, plot.bottom() + reach - top)
    } else {
        clip
    };
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
        domain: (xd, yd),
        fit: domain,
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
        totals: Vec::new(),
        flipped: Cell::new(false),
        ramp,
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

/// About `count` ticks over a scale, at whole steps when `whole` (fewer, rather than 0.5s).
fn whole_ticks(s: &Scale, count: usize, whole: bool) -> Vec<Tick> {
    let mut c = count.max(1);
    loop {
        let ts = scale::ticks(s, c);
        let fine = matches!(ts.as_slice(), [a, b, ..] if (b.value - a.value).abs() < 1.0 - 1e-9);
        if !(whole && fine) || c <= 1 {
            return ts;
        }
        c -= 1;
    }
}

/// A fitted y domain `d` (niced for `count` ticks) whose end tick stands far past the data, as
/// a short plot's coarse step leaves it (0 to 1,500 by 500 for data to 1,060), stopped a little
/// past the data instead, at a fifth of the step (0 to 1,200, ticks 0, 500, 1,000): the plot
/// isn't a third empty. Its ticks keep their step; where they wouldn't, `d` stays.
fn loosened(
    model: &ChartModel,
    view: &View,
    d: (f64, f64),
    count: usize,
    range: (f32, f32),
    whole: bool,
) -> (f64, f64) {
    let step_of = |dom: (f64, f64)| {
        let ts = whole_ticks(&Scale::Linear { domain: dom, range }, count, whole);
        match ts.as_slice() {
            [a, b, ..] => (b.value - a.value).abs(),
            _ => f64::NAN,
        }
    };
    let step = step_of(d);
    let span = d.1 - d.0;
    // The data's own extent, zero and all, at a fine step.
    let tight = model.fit_ticks(view, 40).1;
    if !(step.is_finite() && span > 0.0 && tight.0 < tight.1) {
        return d;
    }
    // A little past the data: a twentieth of it, and over a bar room for its total.
    let per_px = span / f64::from((range.0 - range.1).abs().max(1.0));
    let bars = model.series.iter().any(|s| s.mark == MarkKind::Bar);
    let pad = (0.05 * (tight.1 - tight.0)).max(if bars { 24.0 * per_px } else { 0.0 });
    let unit = step / 5.0;
    let mut out = d;
    if d.1 - tight.1 > 0.25 * span {
        out.1 = (((tight.1 + pad) / unit).ceil() * unit).min(d.1);
    }
    if tight.0 - d.0 > 0.25 * span {
        out.0 = (((tight.0 - pad) / unit).floor() * unit).max(d.0);
    }
    if out != d && (step_of(out) - step).abs() <= step * 1e-9 {
        out
    } else {
        d
    }
}

/// Bars across: their values fit to the ticks the plot's width has room for, as a plot's height
/// does for values up it (0 to 160 by 20 for a top of 144, not 0 to 200 by 50), with room past
/// the longest bar for its total.
fn across_values(
    model: &ChartModel,
    view: &View,
    plot: &Rect,
    m: &mut dyn Measure,
) -> Option<(f64, f64)> {
    let tight = model.fit_ticks(view, 40).1;
    let room = 8.0 + m.width(&model.y_label(tight.1), TextSpec::new(TICK).tabular());
    let most = ((plot.w / 90.0).round() as usize).clamp(2, X_MOST);
    (2..=most).rev().find_map(|c| {
        let d = model.fit_ticks(view, c).1;
        let end = plot.w * ((tight.1 - d.0) / (d.1 - d.0)) as f32;
        (d.1 > d.0 && end + room <= plot.w).then_some(d)
    })
}

/// Whether the chart has series and every one is hidden.
pub(crate) fn nothing_shown(model: &ChartModel, view: &View) -> bool {
    !model.series.is_empty() && model.keys().iter().all(|k| !model.visible(view, k))
}

/// How far past its centre a scatter's point reaches, in px: its radius and its ring, a
/// bubble's largest.
fn point_reach(model: &ChartModel, view: &View) -> f32 {
    let bubbles = super::marks::size_extent(model).is_some();
    model
        .series
        .iter()
        .filter(|s| s.mark == MarkKind::Point && model.visible(view, &s.key))
        .map(|s| {
            if bubbles && s.sizes.is_some() {
                super::marks::BUBBLE_MAX + 1.0
            } else {
                model.point_size(s) / 2.0 + super::marks::RING
            }
        })
        .fold(0.0, f32::max)
        + 1.0
}

/// Whether a scatter's axis (x, or y) holds no negative value: its dots' padding may reach
/// below zero, but no tick there says a value it hasn't.
fn no_negatives(model: &ChartModel, view: &View, x: bool) -> bool {
    model.is_scatter() && points_extent(model, view, x).is_some_and(|e| e.0 >= 0.0)
}

/// The visible points' extent along x (or y).
fn points_extent(model: &ChartModel, view: &View, x: bool) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in model
        .series
        .iter()
        .filter(|s| s.mark == MarkKind::Point && model.visible(view, &s.key))
    {
        for (a, b) in s.xs.iter().zip(s.ys.iter()) {
            if a.is_finite() && b.is_finite() {
                let v = if x { *a } else { *b };
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// A scatter's domain `d` whose last tick labels little of its data (5 of a 0 to 8.8, the top
/// 43 % of the plot unlabelled): on to the next tick, so its top is labelled, where the ticks
/// keep a step that does; the same at the bottom where that doesn't go below zero.
fn to_next_tick(
    d: (f64, f64),
    data: (f64, f64),
    count: usize,
    range: (f32, f32),
    whole: bool,
) -> (f64, f64) {
    let ticks = |dom: (f64, f64)| whole_ticks(&Scale::Linear { domain: dom, range }, count, whole);
    let span = data.1 - data.0;
    let ts = ticks(d);
    let (Some(first), Some(last), true) = (ts.first(), ts.last(), ts.len() >= 2 && span > 0.0)
    else {
        return d;
    };
    let step = (ts[1].value - ts[0].value).abs();
    let mut out = d;
    if last.value < data.0 + 0.75 * span {
        out.1 = out.1.max(last.value + step);
    }
    let floor = if data.0 >= 0.0 {
        0.0
    } else {
        f64::NEG_INFINITY
    };
    if first.value > data.1 - 0.75 * span && first.value - step >= floor {
        out.0 = out.0.min(first.value - step);
    }
    if out == d {
        return d;
    }
    // Only where the ticks then reach as far: a step that labels the new ends.
    let after = ticks(out);
    let covers = after
        .last()
        .is_some_and(|t| t.value >= data.0 + 0.75 * span)
        && after
            .first()
            .is_some_and(|t| t.value <= data.1 - 0.75 * span);
    if covers { out } else { d }
}

/// A domain widened so that data from `data.0` to `data.1`, over `len` px, keeps `reach` px
/// inside each end: its ticks stay where they were, inside it.
fn pad_for(d: (f64, f64), data: (f64, f64), len: f32, reach: f32, kind: AxisKind) -> (f64, f64) {
    if kind != AxisKind::Linear || len <= 4.0 * reach || d.1 <= d.0 {
        return d;
    }
    let need = f64::from(reach) * (d.1 - d.0) / f64::from(len - 2.0 * reach);
    (d.0.min(data.0 - need), d.1.max(data.1 + need))
}

/// The ticks of a number or time axis across the plot: as many as keep labels 64 px apart and
/// clear of each other, the end ones kept inside the area.
#[allow(clippy::too_many_arguments)]
fn value_ticks(
    model: &ChartModel,
    s: &Scale,
    plot: &Rect,
    spec: TextSpec,
    independent: bool,
    whole: bool,
    area_w: f32,
    m: &mut dyn Measure,
) -> Vec<(Tick, String, f32, Anchor)> {
    let mut count = ((plot.w / 90.0).round() as usize).clamp(2, X_MOST);
    // Bars over a time or a number tick no finer than they come: an hour's bars get no half
    // hours between them (a month's still take calendar months).
    let bar_step = if independent && !model.is_band() {
        model
            .series
            .iter()
            .filter(|s| s.mark == MarkKind::Bar && s.interval > 0.0)
            .map(|s| s.interval)
            .fold(f64::INFINITY, f64::min)
    } else {
        f64::INFINITY
    };
    loop {
        let ts = whole_ticks(s, count, !independent && whole);
        if bar_step.is_finite()
            && count > 2
            && matches!(ts.as_slice(), [a, b, ..] if (b.value - a.value).abs() < 0.75 * bar_step)
        {
            count -= 1;
            continue;
        }
        let mut placed: Vec<(Tick, String, f32, Anchor, Rect)> = Vec::new();
        for t in ts {
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
            let (x, anchor) = if px + tw / 2.0 > area_w - EDGE {
                (area_w - EDGE, Anchor::End)
            } else if px - tw / 2.0 < EDGE {
                (EDGE, Anchor::Start)
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
        type Placed = (Tick, String, f32, Anchor, Rect);
        let clear = |p: &[Placed]| {
            p.windows(2).all(|p| {
                let (a, b) = (&p[0], &p[1]);
                s.map(b.0.value) - s.map(a.0.value) >= X_SPACING && b.4.x - a.4.right() >= X_CLEAR
            })
        };
        let ok = clear(&placed);
        if ok || count <= 2 {
            if !ok {
                // The fewest the scale gives still collide: every other one, every third...,
                // while two at least clear; else the first alone.
                let thinned = (2..placed.len().max(2)).find_map(|by| {
                    let pick: Vec<Placed> = placed.iter().step_by(by).cloned().collect();
                    (pick.len() >= 2 && clear(&pick)).then_some(pick)
                });
                placed = thinned.unwrap_or_else(|| placed.into_iter().take(1).collect());
            }
            return placed
                .into_iter()
                .map(|(t, l, x, a, _)| (t, l, x, a))
                .collect();
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
    let nothing = nothing_shown(model, view);
    let grid = model.chart.view.grid && !model.is_heatmap() && !nothing;
    let axes = model.chart.view.axes && !model.is_heatmap() && !nothing;
    // The baseline: at zero where the values cross it, else along the plot's bottom (its left
    // for bars across).
    let val = l.val();
    let (lo, hi) = val.domain();
    let zero = lo.min(hi) <= 0.0
        && lo.max(hi) >= 0.0
        && !matches!(val, Scale::Log { .. } | Scale::Band { .. });
    let base_at = if model.horizontal {
        if zero { l.x.map(0.0) } else { p.x }
    } else if zero {
        l.y.map(0.0)
    } else {
        p.bottom()
    };
    if grid {
        // Hairlines along the value axis's ticks: across the plot, or up it for bars across.
        // The one under the baseline gives way to it; a tick at the bottom below zero keeps its.
        if model.horizontal {
            for t in &l.x_ticks {
                let x = l.x.map(t.value);
                if axes && (x - base_at).abs() < 0.5 {
                    continue;
                }
                hair(scene, (x, p.y), (x, p.bottom()), theme.grid);
            }
        } else {
            for t in &l.y_ticks {
                let y = l.y.map(t.value);
                if (axes && (y - base_at).abs() < 0.5) || (!axes && (y - p.bottom()).abs() < 0.5) {
                    continue;
                }
                hair(scene, (p.x, y), (p.right(), y), theme.grid);
            }
        }
    }
    if axes {
        if model.horizontal {
            hair(scene, (base_at, p.y), (base_at, p.bottom()), theme.baseline);
        } else {
            hair(scene, (p.x, base_at), (p.right(), base_at), theme.baseline);
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
    if let Some(r) = &l.ramp {
        r.draw(scene, theme);
    }
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
