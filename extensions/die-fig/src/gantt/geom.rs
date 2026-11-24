//! Where a gantt lands in an area: the header (as a graph's), the time axis along the top, the
//! gutter of labels, the rows under them, each task's bar and each link's path.
//!
//! From the top: the title, the subtitle and the legend; the ticks' labels; a line of the
//! marks' labels (`Today`, a deadline's name) when there are any; then the rows, [`ROW`] px
//! each ([`HEADING`] for a group's heading), which don't stretch to fill the area: a short plan
//! leaves the rest empty, and a long one scrolls under the axis. The gutter at the left is as
//! wide as its widest label, within bounds. Time maps onto the plot's width by a
//! [`Scale::Time`], so a bar's ends are its task's start and end.
//!
//! A link runs from the earlier task's end to the later one's start in right angles: out
//! [`LINK_OUT`] px, down or up to the other's row, and in; where the later starts too close
//! (or before), it goes round between the rows.

use super::GanttModel;
use crate::data::SeriesKey;
use crate::graph::geom::{HeaderAsk, Keep, LEVELS, header_of, side_pad};
use crate::interact::View;
use crate::plot::layout::{BODY, Ink, KeyShape, Label, LegendItem, Role, TICK, paddings};
use crate::scale::{self, Scale, Tick};
use crate::scene::{Anchor, Measure, Rect, TextSpec, Weight};
use crate::spec::LegendPos;
use crate::theme::{Color, Theme, color_of};
use std::cell::Cell;

/// A task's row's height.
pub const ROW: f32 = 28.0;
/// A group heading's row's height.
pub const HEADING: f32 = 30.0;
/// A bar's height, and its corners' radius.
pub const BAR: f32 = 14.0;
/// A bar's corners' radius.
pub const RADIUS: f32 = 3.0;
/// The narrowest a bar is drawn.
pub const BAR_MIN: f32 = 3.0;
/// How far a label is indented for each parent above it.
pub const INDENT: f32 = 14.0;
/// Between the gutter and the plot.
pub const GUTTER_GAP: f32 = 16.0;
/// The narrowest the gutter is.
pub const GUTTER_MIN: f32 = 72.0;
/// A milestone's diamond, across; and the ring it wears in the surface's colour.
pub const DIAMOND: f32 = 12.0;
/// The ring a milestone wears in the surface's colour.
pub const DIAMOND_RING: f32 = 2.0;
/// A summary's bar's height, and how far its end ticks reach under it.
pub const SUMMARY: (f32, f32) = (6.0, 4.0);
/// How far a link runs out of a bar before it turns.
pub const LINK_OUT: f32 = 8.0;
/// A link whose later task starts less than this right of where the earlier ends goes round
/// between the rows.
pub const LINK_NEAR: f32 = 16.0;
/// A link's arrowhead: its length and half its width.
pub const ARROW: (f32, f32) = (5.0, 2.75);
/// The ticks' labels' strip's height, and the marks' labels' under it.
const AXIS_H: f32 = 22.0;
const MARKS_H: f32 = 17.0;
/// About how far apart the ticks are.
const TICK_SPACING: f32 = 64.0;
/// The least room between two tick labels' ends.
const TICK_CLEAR: f32 = 10.0;
/// A legend row's height, its key's slot and gaps: the header's.
const LEGEND_ROW: f32 = 20.0;
const KEY_W: f32 = 14.0;
const KEY_GAP: f32 = 6.0;
/// The scrollbar's width.
const SCROLLBAR: f32 = 4.0;
/// How far past the plot a bar's end is laid out at the most: past that it's out of sight
/// whatever the zoom, and the numbers stay ones a rasteriser likes.
const REACH: f32 = 4096.0;

/// What a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// A group's heading: its index among the model's groups.
    Heading(u32),
    /// A task: its index among the model's tasks.
    Task(u32),
}

/// A row as laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct RowGeom {
    /// What it is.
    pub kind: RowKind,
    /// Its top, in the area's px: above or below the plot when it's scrolled out.
    pub y: f32,
    /// Its height.
    pub h: f32,
    /// Its label in the gutter.
    pub label: Label,
    /// A heading's line across its group's span: its ends across, and its colour.
    pub line: Option<(f32, f32, Color)>,
}

/// How a task is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A bar from its start to its end.
    Bar,
    /// A diamond at its moment.
    Milestone,
    /// A thin bar with a tick down at each end: a task with children.
    Summary,
}

/// A task as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskGeom {
    /// Its row, an index into the layout's rows.
    pub row: u32,
    /// Its row's strip, across the gutter and the plot.
    pub strip: Rect,
    /// How it's drawn.
    pub shape: Shape,
    /// Its bar; a milestone's diamond's box; a summary's bar and ticks' box.
    pub bar: Rect,
    /// How much of the bar is done, from its left, in px; None where the plan doesn't track
    /// progress.
    pub done: Option<f32>,
    /// Its colour.
    pub color: Color,
    /// Whether it's an outline, dashed (its `fig` says so).
    pub dash: bool,
    /// A milestone's date beside it, where there's room.
    pub date: Option<Label>,
}

impl TaskGeom {
    /// The middle of its row, down the screen.
    pub fn mid(&self) -> f32 {
        self.strip.y + self.strip.h / 2.0
    }

    /// Where a link leaves it: its right end, at its row's middle.
    pub fn out(&self) -> (f32, f32) {
        (self.bar.right(), self.mid())
    }

    /// Where a link enters it: its left end.
    pub fn into(&self) -> (f32, f32) {
        (self.bar.x, self.mid())
    }

    /// What the pointer finds it by: its bar and a little round it.
    pub fn reach(&self) -> Rect {
        let pad = match self.shape {
            Shape::Milestone => 5.0,
            _ => 4.0,
        };
        let r = self.bar.inflate(pad);
        if r.w < 14.0 {
            Rect::new(r.x + r.w / 2.0 - 7.0, r.y, 14.0, r.h)
        } else {
            r
        }
    }
}

/// A link as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkGeom {
    /// The earlier task.
    pub from: u32,
    /// The later one.
    pub to: u32,
    /// Its path, from the earlier task's end to the base of its arrowhead.
    pub pts: Vec<(f32, f32)>,
    /// Its arrowhead: the tip at the later task's start, then its two back corners.
    pub arrow: [(f32, f32); 3],
    /// Whether the later starts before the earlier ends.
    pub conflict: bool,
}

/// A mark on the time as drawn: today's line, a mark's line, or a mark's span.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkGeom {
    /// Where its line is, or where its span starts.
    pub x: f32,
    /// Where a span ends.
    pub x1: Option<f32>,
    /// Its colour.
    pub color: Color,
    /// Its label: the text, its anchor's x, its baseline and which end the anchor is.
    pub label: Option<(String, f32, f32, Anchor)>,
    /// Whether it's the today line.
    pub today: bool,
}

/// Where everything landed.
pub struct GanttLayout {
    /// The area's size in logical px.
    pub size: (f32, f32),
    /// Physical px to a logical px.
    pub scale: f32,
    /// The area under the header.
    pub region: Rect,
    /// The labels' column, as tall as the plot.
    pub gutter: Rect,
    /// The ticks' labels' strip, over the plot; empty where the axis isn't drawn.
    pub axis: Rect,
    /// The marks' labels' strip, between the axis and the plot; empty where there are none.
    pub strip: Rect,
    /// Where the bars are: right of the gutter, under the axis.
    pub plot: Rect,
    /// Time onto the plot's width.
    pub x: Scale,
    /// The time shown.
    pub domain: (f64, f64),
    /// The time that fits the plan: what zoom and pan are limited by.
    pub fit: (f64, f64),
    /// The ticks: a gridline each.
    pub ticks: Vec<Tick>,
    /// The ticks' labels drawn.
    pub tick_labels: Vec<Label>,
    /// Every row under the view, in order, scrolled or not.
    pub rows: Vec<RowGeom>,
    /// Each task as drawn; None where it has no row.
    pub tasks: Vec<Option<TaskGeom>>,
    /// The links drawn.
    pub links: Vec<LinkGeom>,
    /// Today's line and the marks, those in view.
    pub marks: Vec<MarkGeom>,
    /// Each legend entry's rectangle, its key and name inflated 2 px.
    pub legend: Vec<(SeriesKey, Rect)>,
    /// The legend's entries.
    pub items: Vec<LegendItem>,
    /// The header's text: title, subtitle, the legend's "+N more".
    pub chrome: Vec<Label>,
    /// The title and subtitle's box.
    pub title: Rect,
    /// How far the rows are scrolled, in px.
    pub scroll: f32,
    /// The rows' whole height.
    pub content: f32,
    /// The scrollbar's track and thumb while the rows overflow.
    pub scrollbar: Option<(Rect, Rect)>,
    /// Where the card may go: the area, a couple of px in.
    pub bounds: Rect,
    /// A cell's width: how far past the area's edge the card goes before it flips back. 10
    /// unless the surface says.
    pub cell_width: f32,
    /// Whether the card was last put left of its anchor, for its hysteresis.
    pub(crate) flipped: Cell<bool>,
}

impl GanttLayout {
    /// Task `i`'s geometry, if it has a row.
    pub fn task(&self, i: usize) -> Option<&TaskGeom> {
        self.tasks.get(i).and_then(Option::as_ref)
    }

    /// Where a time is across the area.
    pub fn px(&self, t: f64) -> f32 {
        self.x.map(t)
    }

    /// The time at a place across the area.
    pub fn time(&self, x: f32) -> f64 {
        self.x.invert(x)
    }

    /// The tasks with a row, in row order.
    pub fn order(&self) -> Vec<usize> {
        (self.rows.iter())
            .filter_map(|r| match r.kind {
                RowKind::Task(i) => Some(i as usize),
                RowKind::Heading(_) => None,
            })
            .collect()
    }

    /// Whether a row is in sight: some of it inside the plot, down the screen.
    pub fn row_in_view(&self, r: &RowGeom) -> bool {
        r.y + r.h > self.plot.y && r.y < self.plot.bottom()
    }

    /// The furthest the rows scroll.
    pub fn max_scroll(&self) -> f32 {
        (self.content - self.plot.h).max(0.0)
    }

    /// Seconds to a px across the plot.
    pub fn secs_per_px(&self) -> f64 {
        (self.domain.1 - self.domain.0) / f64::from(self.plot.w.max(1.0))
    }
}

/// What the area keeps at a size: the header's level, and whether the ticks are labelled.
#[derive(Clone, Copy)]
struct Level {
    keep: Keep,
    axis: bool,
}

/// The rows under a view, before they're placed: each heading and task with its height.
fn rows_of(model: &GanttModel, view: &View) -> Vec<(RowKind, f32)> {
    let mut out = Vec::new();
    for s in &model.sections {
        let shown: Vec<u32> = (s.tasks.iter().copied())
            .filter(|&i| model.visible(view, i as usize))
            .collect();
        if shown.is_empty() {
            continue;
        }
        if let Some(g) = s.group {
            out.push((RowKind::Heading(g), HEADING));
        }
        out.extend(shown.into_iter().map(|i| (RowKind::Task(i), ROW)));
    }
    out
}

/// A row's label's type: a heading's in semibold.
fn label_spec(kind: RowKind) -> TextSpec {
    match kind {
        RowKind::Heading(_) => TextSpec::new(BODY).weight(Weight::Semibold),
        RowKind::Task(_) => TextSpec::new(BODY),
    }
}

/// The layout of `model` under `view` at `size`; its paddings across those of an area `pad_w`
/// wide.
pub(crate) fn lay_out(
    model: &GanttModel,
    view: &View,
    size: (f32, f32),
    pad_w: f32,
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> GanttLayout {
    let (w, h) = size;
    let small = w < 240.0 || h < 120.0;
    let rows = rows_of(model, view);
    let content: f32 = rows.iter().map(|r| r.1).sum();
    let mut levels: Vec<Level> = Vec::new();
    for (i, keep) in LEVELS.into_iter().enumerate() {
        if i == 0 && small {
            continue;
        }
        levels.push(Level { keep, axis: true });
    }
    levels.push(Level {
        keep: LEVELS[2],
        axis: false,
    });
    levels.push(Level {
        keep: LEVELS[3],
        axis: false,
    });
    let mut chosen = None;
    for level in levels {
        let f = frame(model, view, size, pad_w, level, m);
        // Room for the rows, or for three of them and most of the area where there are more.
        let enough = f.plot.h >= content.min((0.45 * h).max(3.0 * ROW));
        chosen = Some(f);
        if enough {
            break;
        }
    }
    let f = chosen.expect("there are levels");
    let plot = f.plot;
    let fit = model.fit();
    let domain = match view.x {
        Some((a, b)) if b > a => (a, b),
        _ => fit,
    };
    let x = Scale::Time {
        domain,
        range: (plot.x, plot.right()),
        zone: model.zone.clone(),
    };
    let count = ((plot.w / TICK_SPACING).floor() as usize).clamp(2, 12);
    let ticks = scale::ticks(&x, count);
    let tick_labels = match f.axis.is_empty() {
        true => Vec::new(),
        false => tick_labels(&ticks, &x, f.axis, w, m),
    };
    let max_scroll = (content - plot.h).max(0.0);
    let scroll = view.y.map_or(0.0, |y| y.0 as f32).clamp(0.0, max_scroll);

    // The rows, down from the plot's top, less the scroll.
    let px = |t: f64| x.map(t).clamp(plot.x - REACH, plot.right() + REACH);
    let label_room = (f.gutter.w).max(1.0);
    let mut placed: Vec<RowGeom> = Vec::with_capacity(rows.len());
    let mut tasks: Vec<Option<TaskGeom>> = vec![None; model.len()];
    let mut y = plot.y - scroll;
    let mut section: Option<(usize, f64, f64)> = None;
    for &(kind, rh) in &rows {
        let spec = label_spec(kind);
        let (text, indent, ink) = match kind {
            RowKind::Heading(g) => (model.groups[g as usize].as_str(), 0.0, Ink::Strong),
            RowKind::Task(i) => {
                let t = &model.tasks[i as usize];
                let room = (label_room - 24.0).max(0.0);
                let indent = (f32::from(t.depth) * INDENT).min(room);
                (t.label.as_str(), indent, Ink::Text)
            }
        };
        let label = Label {
            role: Role::YTick,
            text: m.fit(text, spec, (label_room - indent).max(0.0)),
            x: f.gutter.x + indent,
            y: (y + rh / 2.0 + m.cap(spec) / 2.0).round(),
            spec,
            anchor: Anchor::Start,
            ink,
            alpha: 1.0,
        };
        let at = placed.len();
        placed.push(RowGeom {
            kind,
            y,
            h: rh,
            label,
            line: None,
        });
        match kind {
            RowKind::Heading(_) => section = Some((at, f64::INFINITY, f64::NEG_INFINITY)),
            RowKind::Task(i) => {
                let t = &model.tasks[i as usize];
                if let Some(s) = &mut section {
                    (s.1, s.2) = (s.1.min(t.start), s.2.max(t.end));
                    let (a, b) = (px(s.1), px(s.2));
                    let color = match (model.by_group, placed[s.0].kind) {
                        (true, RowKind::Heading(g)) => model.name_color(g as usize, theme),
                        _ => theme.muted,
                    };
                    placed[s.0].line = Some((a, b.max(a + BAR_MIN), color));
                }
                let strip = Rect::new(f.gutter.x, y, plot.right() - f.gutter.x, rh);
                tasks[i as usize] = Some(task_geom(model, i as usize, at, strip, &px, theme));
            }
        }
        y += rh;
    }

    // The links between tasks that both have rows.
    let mut links = Vec::new();
    if model.spec.links {
        for l in &model.links {
            let (Some(a), Some(b)) = (&tasks[l.from as usize], &tasks[l.to as usize]) else {
                continue;
            };
            let (pts, arrow) = route(a, b);
            links.push(LinkGeom {
                from: l.from,
                to: l.to,
                pts,
                arrow,
                conflict: l.conflict,
            });
        }
    }
    // A milestone's date beside it: right of it, past a link that leaves it, where it fits
    // before the plot's edge.
    let date_spec = TextSpec::new(TICK).tabular();
    for i in 0..model.len() {
        let Some(g) = tasks[i].as_ref().filter(|g| g.shape == Shape::Milestone) else {
            continue;
        };
        let t = &model.tasks[i];
        let text = match model.whole_days {
            true => super::calendar::short_day(t.start, &model.zone),
            false => super::calendar::clock_text(t.start, &model.zone),
        };
        let leaves = links.iter().any(|l| l.from as usize == i);
        let at = g.bar.right() + if leaves { LINK_OUT + 6.0 } else { 7.0 };
        let wide = m.width(&text, date_spec);
        if at + wide > plot.right() - 2.0 || at < plot.x {
            continue;
        }
        let label = Label {
            role: Role::End,
            text,
            x: at,
            y: (g.mid() + m.cap(date_spec) / 2.0).round(),
            spec: date_spec,
            anchor: Anchor::Start,
            ink: Ink::Muted,
            alpha: 1.0,
        };
        if let Some(g) = tasks[i].as_mut() {
            g.date = Some(label);
        }
    }

    let marks = marks(model, &x, plot, f.strip, theme, m);
    let scrollbar = (max_scroll > 0.0).then(|| {
        let pad = side_pad(w, h, pad_w);
        let track = Rect::new(
            (w - pad / 2.0 - SCROLLBAR / 2.0).round(),
            plot.y,
            SCROLLBAR,
            plot.h,
        );
        let th = (track.h * plot.h / content).clamp(24.0_f32.min(track.h), track.h);
        let at = scroll / max_scroll;
        let thumb = Rect::new(track.x, track.y + (track.h - th) * at, track.w, th);
        (track, thumb)
    });
    GanttLayout {
        size,
        scale,
        region: f.region,
        gutter: f.gutter,
        axis: f.axis,
        strip: f.strip,
        plot,
        x,
        domain,
        fit,
        ticks,
        tick_labels,
        rows: placed,
        tasks,
        links,
        marks,
        legend: f.legend,
        items: f.items,
        chrome: f.chrome,
        title: f.title,
        scroll,
        content,
        scrollbar,
        bounds: Rect::new(2.0, 2.0, (w - 4.0).max(1.0), (h - 4.0).max(1.0)),
        cell_width: 10.0,
        flipped: Cell::new(false),
    }
}

/// The area's parts at a level: the header, the gutter, the strips and the plot.
struct Frame {
    chrome: Vec<Label>,
    title: Rect,
    legend: Vec<(SeriesKey, Rect)>,
    items: Vec<LegendItem>,
    region: Rect,
    gutter: Rect,
    axis: Rect,
    strip: Rect,
    plot: Rect,
}

fn frame(
    model: &GanttModel,
    view: &View,
    size: (f32, f32),
    pad_w: f32,
    level: Level,
    m: &mut dyn Measure,
) -> Frame {
    let (w, h) = size;
    let (_, _, pad_bottom) = paddings(w, h);
    let pad = side_pad(w, h, pad_w);
    let keys = model.keys();
    let legend_on = model.has_legend();
    let right = legend_on && model.spec.view.legend == LegendPos::Right;
    let entries: Vec<(SeriesKey, &str)> = match legend_on && !right {
        true => (keys.iter().cloned())
            .zip(model.names.iter().map(String::as_str))
            .collect(),
        false => Vec::new(),
    };
    let ask = HeaderAsk {
        title: model.spec.title.as_deref(),
        subtitle: match model.spec.subtitle.as_deref() {
            Some(s) => s.to_string(),
            None => model.summary(),
        },
        subtitle_given: model.spec.subtitle.is_some(),
        entries,
        shape: KeyShape::Square,
    };
    let hd = header_of(&ask, view, size, pad_w, level.keep, m);
    let region = hd.region;
    let (mut legend, mut items, mut chrome) = (hd.legend, hd.items, hd.chrome);
    let top = if region.y > 0.0 {
        region.y
    } else {
        paddings(w, h).1
    };

    // The gutter: as wide as its widest label, within bounds.
    let mut widest: f32 = 0.0;
    for (kind, _) in rows_of(model, view) {
        let (text, indent) = match kind {
            RowKind::Heading(g) => (model.groups[g as usize].as_str(), 0.0),
            RowKind::Task(i) => {
                let t = &model.tasks[i as usize];
                (t.label.as_str(), f32::from(t.depth) * INDENT)
            }
        };
        widest = widest.max(indent + m.width(text, label_spec(kind)));
    }
    let share = if w < 420.0 { 0.30 } else { 0.38 };
    let gutter_w = widest.ceil().min(w * share).max(GUTTER_MIN.min(w * 0.45));

    // A legend at the right: a column of its entries beside the plot.
    let mut right_w = 0.0;
    if right && level.keep.legend_rows > 0 {
        let body = TextSpec::new(BODY);
        let max_name = (w * 0.22).clamp(40.0, 180.0);
        let fitted: Vec<String> = (model.names.iter())
            .map(|n| m.fit(n, body, max_name))
            .collect();
        let col_w = (fitted.iter())
            .map(|n| KEY_W + KEY_GAP + m.width(n, body))
            .fold(0.0, f32::max);
        let room = ((h - pad_bottom - top) / LEGEND_ROW).floor() as usize;
        let shown = fitted.len().min(room);
        let more = fitted.len() - shown;
        let shown = if more > 0 {
            shown.saturating_sub(1)
        } else {
            shown
        };
        let x = w - pad - col_w;
        let cap = m.cap(body);
        for (i, name) in fitted.iter().enumerate().take(shown) {
            let rtop = top + i as f32 * LEGEND_ROW;
            let mid = rtop + LEGEND_ROW / 2.0;
            let hidden = !view.shows(&keys[i]);
            legend.push((
                keys[i].clone(),
                Rect::new(x, rtop, col_w, LEGEND_ROW).inflate(2.0),
            ));
            items.push(LegendItem {
                key: keys[i].clone(),
                shape: KeyShape::Square,
                swatch: Rect::new(x + 2.0, mid - 5.0, 10.0, 10.0),
                name: Label {
                    role: Role::Legend,
                    text: name.clone(),
                    x: x + KEY_W + KEY_GAP,
                    y: mid + cap / 2.0,
                    spec: body,
                    anchor: Anchor::Start,
                    ink: Ink::Text,
                    alpha: if hidden { 0.45 } else { 1.0 },
                },
                hidden,
            });
        }
        if fitted.len() > shown {
            chrome.push(Label {
                role: Role::Legend,
                text: format!("+{} more", fitted.len() - shown),
                x,
                y: top + shown as f32 * LEGEND_ROW + LEGEND_ROW / 2.0 + cap / 2.0,
                spec: body,
                anchor: Anchor::Start,
                ink: Ink::Muted,
                alpha: 1.0,
            });
        }
        right_w = col_w + GUTTER_GAP;
    }

    let plot_x = pad + gutter_w + GUTTER_GAP;
    let plot_w = (w - pad - right_w - plot_x).max(1.0);
    let mut y = top;
    let axis = match level.axis && model.spec.view.axes {
        true => {
            let r = Rect::new(plot_x, y, plot_w, AXIS_H);
            y += AXIS_H;
            r
        }
        false => Rect::default(),
    };
    let labelled = model.today().is_some() || model.marks.iter().any(|mk| mk.label.is_some());
    let strip = match level.axis && labelled {
        true => {
            let r = Rect::new(plot_x, y, plot_w, MARKS_H);
            y += MARKS_H;
            r
        }
        false => Rect::default(),
    };
    y += 4.0;
    let plot = Rect::new(plot_x, y, plot_w, (h - pad_bottom - y).max(1.0));
    Frame {
        chrome,
        title: hd.title,
        legend,
        items,
        region,
        gutter: Rect::new(pad, plot.y, gutter_w, plot.h),
        axis,
        strip,
        plot,
    }
}

/// The ticks' labels in the axis strip: each centred on its tick, a major one's in the text
/// colour, those that would run into a neighbour or out of the area left out (the major ones
/// kept first).
fn tick_labels(
    ticks: &[Tick],
    x: &Scale,
    axis: Rect,
    width: f32,
    m: &mut dyn Measure,
) -> Vec<Label> {
    let spec = TextSpec::new(TICK).tabular();
    let base = (axis.y + axis.h / 2.0 + m.cap(spec) / 2.0).round();
    let mut kept: Vec<(usize, Rect)> = Vec::new();
    for major in [true, false] {
        for (i, t) in ticks.iter().enumerate().filter(|(_, t)| t.major == major) {
            let at = x.map(t.value);
            let wide = m.width(&t.label, spec);
            let r = Rect::new(at - wide / 2.0, axis.y, wide, axis.h);
            if r.x < axis.x - 14.0 || r.right() > (axis.right() + 14.0).min(width - 4.0) {
                continue;
            }
            let clear = r.inflate(TICK_CLEAR / 2.0);
            if kept
                .iter()
                .any(|(_, k)| k.inflate(TICK_CLEAR / 2.0).intersect(&clear).is_some())
            {
                continue;
            }
            kept.push((i, r));
        }
    }
    kept.sort_by_key(|k| k.0);
    kept.into_iter()
        .map(|(i, _)| Label {
            role: Role::XTick,
            text: ticks[i].label.clone(),
            x: x.map(ticks[i].value),
            y: base,
            spec,
            anchor: Anchor::Middle,
            ink: if ticks[i].major {
                Ink::Text
            } else {
                Ink::Muted
            },
            alpha: 1.0,
        })
        .collect()
}

/// A task's bar, diamond or summary in its row.
fn task_geom(
    model: &GanttModel,
    i: usize,
    row: usize,
    strip: Rect,
    px: &dyn Fn(f64) -> f32,
    theme: &Theme,
) -> TaskGeom {
    let t = &model.tasks[i];
    let mid = strip.y + strip.h / 2.0;
    let (x0, x1) = (px(t.start), px(t.end));
    let shape = if !t.children.is_empty() && !t.milestone {
        Shape::Summary
    } else if t.milestone {
        Shape::Milestone
    } else {
        Shape::Bar
    };
    let wide = (x1 - x0).max(BAR_MIN);
    let bar = match shape {
        Shape::Bar => Rect::new(x0, mid - BAR / 2.0, wide, BAR),
        Shape::Milestone => Rect::new(x0 - DIAMOND / 2.0, mid - DIAMOND / 2.0, DIAMOND, DIAMOND),
        Shape::Summary => Rect::new(
            x0,
            mid - (SUMMARY.0 + SUMMARY.1) / 2.0,
            wide,
            SUMMARY.0 + SUMMARY.1,
        ),
    };
    let done = match shape {
        Shape::Milestone => None,
        _ => t.done.map(|d| (bar.w * d as f32).clamp(0.0, bar.w)),
    };
    TaskGeom {
        row: row as u32,
        strip,
        shape,
        bar,
        done,
        color: model.color(i, theme),
        dash: t.fig.dash,
        date: None,
    }
}

/// A link's path and arrowhead from task `a`'s end to task `b`'s start.
fn route(a: &TaskGeom, b: &TaskGeom) -> (Vec<(f32, f32)>, [(f32, f32); 3]) {
    // A milestone's ring is part of what the link leaves and meets.
    let ring = |g: &TaskGeom| match g.shape {
        Shape::Milestone => DIAMOND_RING * 0.5,
        _ => 0.0,
    };
    let (fx, fy) = (a.out().0 + ring(a), a.out().1);
    let (tx, ty) = (b.into().0 - ring(b), b.into().1);
    let out = fx + LINK_OUT;
    let base = tx - ARROW.0;
    let pts = if tx - fx >= LINK_NEAR {
        vec![(fx, fy), (out, fy), (out, ty), (base, ty)]
    } else {
        // Round between the rows: down (or up) past the earlier's row, back, then in.
        let between = if ty >= fy {
            a.strip.bottom()
        } else {
            a.strip.y
        };
        let back = tx - LINK_OUT;
        vec![
            (fx, fy),
            (out, fy),
            (out, between),
            (back, between),
            (back, ty),
            (base, ty),
        ]
    };
    let arrow = [(tx, ty), (base, ty - ARROW.1), (base, ty + ARROW.1)];
    (pts, arrow)
}

/// Today's line and the spec's marks, those in view, with their labels in the strip where
/// they fit clear of each other.
fn marks(
    model: &GanttModel,
    x: &Scale,
    plot: Rect,
    strip: Rect,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Vec<MarkGeom> {
    let spec = TextSpec::new(TICK);
    let base = (strip.y + strip.h / 2.0 + m.cap(spec) / 2.0).round();
    let inside = |px: f32| px >= plot.x && px <= plot.right();
    let mut taken: Vec<Rect> = Vec::new();
    let mut out: Vec<MarkGeom> = Vec::new();
    let mut place = |text: &str, tries: &[(f32, Anchor)], m: &mut dyn Measure| {
        if strip.is_empty() || text.is_empty() {
            return None;
        }
        let wide = m.width(text, spec);
        for &(at, anchor) in tries {
            let left = match anchor {
                Anchor::Start => at,
                Anchor::Middle => at - wide / 2.0,
                Anchor::End => at - wide,
            };
            let r = Rect::new(left, strip.y, wide, strip.h);
            if r.x < plot.x - 2.0 || r.right() > plot.right() + 2.0 {
                continue;
            }
            if taken.iter().any(|t| t.inflate(4.0).intersect(&r).is_some()) {
                continue;
            }
            taken.push(r);
            return Some((text.to_string(), at, base, anchor));
        }
        None
    };
    let beside = |px: f32| {
        [
            (px, Anchor::Middle),
            (px + 5.0, Anchor::Start),
            (px - 5.0, Anchor::End),
        ]
    };
    if let Some(now) = model.today() {
        let px = x.map(now);
        if inside(px) {
            out.push(MarkGeom {
                x: px,
                x1: None,
                color: theme.accent,
                label: place("Today", &beside(px), m),
                today: true,
            });
        }
    }
    for mk in &model.marks {
        let color = mk.color.map_or(theme.muted, |c| color_of(c, theme));
        let label = mk.label.as_deref().unwrap_or("");
        let a = x.map(mk.start);
        match mk.end {
            None if inside(a) => out.push(MarkGeom {
                x: a,
                x1: None,
                color,
                label: place(label, &beside(a), m),
                today: false,
            }),
            Some(end) => {
                let b = x.map(end);
                if b < plot.x || a > plot.right() {
                    continue;
                }
                let (a, b) = (a.max(plot.x), b.min(plot.right()));
                let tries = [(a + 1.0, Anchor::Start), (b - 1.0, Anchor::End)];
                out.push(MarkGeom {
                    x: a,
                    x1: Some(b),
                    color,
                    label: place(label, &tries, m),
                    today: false,
                });
            }
            None => {}
        }
    }
    out
}
