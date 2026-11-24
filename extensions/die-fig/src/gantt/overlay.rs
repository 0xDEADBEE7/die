//! The gantt's overlay: what follows the pointer and the cursor, drawn over the base so a hover
//! repaints nothing else.
//!
//! - **A task lit** (the one under the pointer, the cursor's, a search's or the panel's): its
//!   row's strip as a bar chart's band, its label in the strong text, its bar brightened with
//!   a faint outline, its links at 1.5 px in the text colour, and the labels of the tasks at
//!   their other ends in the strong text too.
//! - **The cursor's task** wears a 2 px ring in the strong text's colour round its bar, as a
//!   graph's node does.
//! - **The card**: the hovered task's, else the cursor's: its label over its start, end (or
//!   when, for a milestone), duration, progress, group, what it comes after and the spec's
//!   `tooltip` fields, values first; a conflict says by how much in the warning colour. Placed
//!   as a graph's card, clear of the bar.
//! - **The date under the pointer**, where it's over the plot and on no bar: a hairline down
//!   the plot and the time in the cursor's chip on the axis.
//! - **A brush**: the chart's, the plot's whole height, with what it spans.

use super::calendar::{self, DAY};
use super::draw::{bar, diamond, link, link_in_view, summary};
use super::geom::{BAR, GanttLayout, RADIUS, Shape, TaskGeom};
use super::{GanttModel, Target};
use crate::data::SeriesKey;
use crate::graph::overlay::{Card, CardArea, Keep, Row};
use crate::interact::{Cursor, View};
use crate::plot::layout::TICK;
use crate::plot::overlay::{CHIP_H, CHIP_PAD, chip, lift, readout_at};
use crate::scene::{Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Weight};
use crate::theme::Theme;

/// The cursor's ring: how far past the bar its middle is, and its width.
pub const CURSOR_RING: (f32, f32) = (3.5, 2.0);
/// The most of the spec's `tooltip` fields a card lists.
pub const CARD_FIELDS: usize = 4;

/// What the overlay shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayState {
    /// The task under the pointer: it's lit and its card shows.
    pub hover: Option<Target>,
    /// The keyboard's cursor, a [`Cursor::Task`]: lit, with its ring.
    pub cursor: Option<Cursor>,
    /// Where the pointer is, in logical px: the card goes beside it, and off any bar the date
    /// there is said.
    pub pointer: Option<(f32, f32)>,
    /// A task a search's best match or a row of the detail panel names: lit as a hover is,
    /// without the card.
    pub emphasis: Option<Cursor>,
    /// A name whose legend key is under the pointer: its bars brighten.
    pub name: Option<SeriesKey>,
    /// A drag across the plot, from one x to another in logical px.
    pub brush: Option<(f32, f32)>,
    /// Whether the cursor's task shows its card while nothing is hovered (off while the detail
    /// panel says the same).
    pub cursor_card: bool,
    /// The tasks a search typed so far matches: their labels and bars brighten.
    pub matches: Vec<usize>,
}

/// The overlay's scene: see the module's docs.
pub fn overlay(
    model: &GanttModel,
    layout: &GanttLayout,
    view: &View,
    st: &OverlayState,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let _ = view;
    let l = layout;
    let mut scene = Scene::new(l.size.0, l.size.1, l.scale);
    let plot = l.plot;
    let rows = Rect::new(l.gutter.x, plot.y, plot.right() - l.gutter.x, plot.h);
    // A task that has a row in sight.
    let seen = |i: usize| {
        l.task(i)
            .filter(|g| l.row_in_view(&l.rows[g.row as usize]))
            .map(|_| i)
    };
    let task_of = |c: &Option<Cursor>| c.as_ref().and_then(|c| model.task_of(c)).and_then(seen);
    let cursor = task_of(&st.cursor);
    let hover = st.hover.map(Target::task).and_then(seen);
    let emphasis = task_of(&st.emphasis);
    let mut lit: Vec<usize> = Vec::new();
    for i in [cursor, emphasis, hover].into_iter().flatten() {
        if !lit.contains(&i) {
            lit.push(i);
        }
    }
    // What brightens without its row lit: a search's matches, a legend key's tasks.
    let mut bright: Vec<usize> = Vec::new();
    let named = (st.name.as_ref()).filter(|k| model.names.iter().any(|n| **n == *k.name));
    for i in 0..model.len() {
        let of_name = named.is_some_and(|k| model.task_key(i) == *k);
        if (st.matches.contains(&i) || of_name) && seen(i).is_some() && !lit.contains(&i) {
            bright.push(i);
        }
    }
    // The links of the lit tasks, and the tasks at their other ends.
    let links: Vec<usize> = (l.links.iter().enumerate())
        .filter(|(_, k)| lit.contains(&(k.from as usize)) || lit.contains(&(k.to as usize)))
        .filter(|(_, k)| link_in_view(l, k))
        .map(|(n, _)| n)
        .collect();
    let mut ends: Vec<usize> = Vec::new();
    for &n in &links {
        for i in [l.links[n].from as usize, l.links[n].to as usize] {
            if !lit.contains(&i) && !ends.contains(&i) && seen(i).is_some() {
                ends.push(i);
            }
        }
    }

    scene.push(Item::PushClip(
        rows.inflate(6.0)
            .intersect(&Rect::new(0.0, plot.y, l.size.0, plot.h))
            .unwrap_or(rows),
    ));
    // The labels drawn again in the strong text: the base's cleared first, under the strip.
    let relabel: Vec<usize> = (lit.iter().chain(&ends).chain(&st.matches))
        .copied()
        .filter(|&i| seen(i).is_some())
        .collect();
    for &i in &relabel {
        let Some(g) = l.task(i) else { continue };
        let b = l.rows[g.row as usize].label.bounds(m).inflate(1.0);
        scene.push(Item::Path {
            path: PathData::rect(b),
            fill: Some(Paint::solid(theme.surface, 1.0)),
            stroke: None,
        });
    }
    for &i in &lit {
        let Some(g) = l.task(i) else { continue };
        let band = Rect::new(
            rows.x - 6.0,
            g.strip.y + 1.0,
            rows.w + 12.0,
            g.strip.h - 2.0,
        );
        scene.push(Item::Path {
            path: PathData::rounded(band, [6.0; 4]),
            fill: Some(Paint::solid(
                theme.text,
                if theme.dark { 0.04 } else { 0.05 },
            )),
            stroke: None,
        });
    }
    let mut done: Vec<usize> = Vec::new();
    for &i in &relabel {
        let Some(g) = l.task(i) else { continue };
        if done.contains(&i) {
            continue;
        }
        done.push(i);
        let label = &l.rows[g.row as usize].label;
        scene.push(Item::Text {
            text: label.text.as_str().into(),
            x: label.x,
            y: label.y,
            spec: label.spec,
            anchor: label.anchor,
            color: theme.strong,
            alpha: 1.0,
            max_width: None,
        });
    }
    scene.push(Item::PopClip);

    scene.push(Item::PushClip(plot));
    for &n in &links {
        let k = &l.links[n];
        let color = if k.conflict {
            theme.status[1]
        } else {
            theme.text
        };
        link(&mut scene, k, color, 1.0, true);
    }
    // The tasks at the links' other ends come forward over them, as they are.
    for &i in &ends {
        if let Some(g) = l.task(i) {
            bar(&mut scene, model, i, g, g.color, theme);
        }
    }
    for &i in &bright {
        if let Some(g) = l.task(i) {
            bar(&mut scene, model, i, g, lift(g.color, theme), theme);
        }
    }
    for &i in &lit {
        let Some(g) = l.task(i) else { continue };
        if Some(i) == cursor {
            // The ring's room: nothing of the base shows between the bar and its ring.
            scene.push(Item::Path {
                path: outline(g, CURSOR_RING.0 + CURSOR_RING.1 / 2.0),
                fill: Some(Paint::solid(theme.surface, 1.0)),
                stroke: None,
            });
        }
        bar(&mut scene, model, i, g, lift(g.color, theme), theme);
        scene.push(Item::Path {
            path: outline(g, 0.5),
            fill: None,
            stroke: Some(Stroke::solid(1.0, theme.strong, 0.6)),
        });
        if Some(i) == cursor {
            let mut ring = Stroke::solid(CURSOR_RING.1, theme.strong, 1.0);
            ring.join = Join::Round;
            scene.push(Item::Path {
                path: outline(g, CURSOR_RING.0),
                fill: None,
                stroke: Some(ring),
            });
        }
    }
    scene.push(Item::PopClip);

    // The date under the pointer, off any bar.
    let over_plot =
        (st.pointer).filter(|p| plot.contains(p.0, p.1) && hover.is_none() && st.brush.is_none());
    if let Some((x, _)) = over_plot {
        let foot = (l.rows.last())
            .map_or(plot.bottom(), |r| (r.y + r.h).min(plot.bottom()))
            .max(plot.y);
        let top = if l.axis.is_empty() {
            plot.y
        } else {
            l.axis.bottom()
        };
        scene.push(Item::Path {
            path: PathData::polyline(&[(x, top), (x, foot)]),
            fill: None,
            stroke: Some(Stroke::hairline(theme.muted, 0.7)),
        });
        if !l.axis.is_empty() {
            let text = time_text(model, l.time(x));
            let spec = TextSpec::new(TICK).weight(Weight::Semibold).tabular();
            let w = m.width(&text, spec) + 2.0 * CHIP_PAD;
            let left = (x - w / 2.0).clamp(4.0, (l.size.0 - 4.0 - w).max(4.0));
            let r = Rect::new(
                left,
                (l.axis.y + (l.axis.h - CHIP_H) / 2.0).round(),
                w,
                CHIP_H,
            );
            // The tick labels it would run into go under the surface's colour.
            let clear = r.inflate(5.0);
            for label in &l.tick_labels {
                let b = label.bounds(m).inflate(1.0);
                if b.intersect(&clear).is_some() {
                    scene.push(Item::Path {
                        path: PathData::rect(b),
                        fill: Some(Paint::solid(theme.surface, 1.0)),
                        stroke: None,
                    });
                }
            }
            chip(theme, &mut scene, r, &text, spec, true, m);
        }
    }

    if let Some((a, b)) = st.brush {
        let (x0, x1) = (
            a.min(b).clamp(plot.x, plot.right()),
            a.max(b).clamp(plot.x, plot.right()),
        );
        let r = Rect::new(x0, plot.y, (x1 - x0).max(1.0), plot.h);
        scene.push(Item::Path {
            path: PathData::rect(r),
            fill: Some(Paint::solid(theme.text, 0.08)),
            stroke: None,
        });
        for x in [x0, x1] {
            scene.push(Item::Path {
                path: PathData::polyline(&[(x, plot.y), (x, plot.bottom())]),
                fill: None,
                stroke: Some(Stroke::hairline(theme.text, 0.4)),
            });
        }
        let text = span_text(model, l.time(x0), l.time(x1));
        readout_at(
            l.size.0,
            theme,
            &mut scene,
            (x0 + x1) / 2.0,
            plot.y,
            &text,
            m,
        );
    }

    // The card: the hover's, else the cursor's.
    let card_of = hover.or(cursor.filter(|_| st.cursor_card && st.brush.is_none()));
    if let Some(i) = card_of
        && let Some(g) = l.task(i)
    {
        let card = card(model, i, theme, m);
        // What's seen of the bar: the card goes by it, off it.
        let shown = (g.bar.intersect(&plot)).unwrap_or(Rect::new(
            g.bar.x.clamp(plot.x, plot.right()),
            g.bar.y,
            1.0,
            g.bar.h,
        ));
        let ax = match st.pointer.filter(|_| hover == Some(i)) {
            Some((px, _)) => px.clamp(shown.x, shown.right()),
            None => shown.x + shown.w / 2.0,
        };
        let reach = BAR / 2.0 + CURSOR_RING.0 + CURSOR_RING.1;
        let mut avoid: Vec<(Keep, f32)> = vec![(Keep::Rect(shown.inflate(3.0)), 24.0)];
        let label_of = |j: usize, m: &mut dyn Measure| {
            l.task(j).map(|t| l.rows[t.row as usize].label.bounds(m))
        };
        if let Some(b) = label_of(i, m) {
            avoid.push((Keep::Rect(b), 10.0));
        }
        for &j in &ends {
            if let Some(t) = l.task(j)
                && let Some(b) = t.bar.intersect(&plot)
            {
                avoid.push((Keep::Rect(b.inflate(2.0)), 6.0));
            }
            if let Some(b) = label_of(j, m) {
                avoid.push((Keep::Rect(b), 4.0));
            }
        }
        if !l.title.is_empty() {
            avoid.push((Keep::Rect(l.title.inflate(4.0)), 12.0));
        }
        for (_, r) in &l.legend {
            avoid.push((Keep::Rect(*r), 6.0));
        }
        // Every other bar a little, so of two places the one that covers less wins.
        for (j, t) in l.tasks.iter().enumerate() {
            if let Some(t) = t
                && j != i
                && !ends.contains(&j)
                && let Some(b) = t.bar.intersect(&plot)
            {
                avoid.push((Keep::Rect(b), 1.5));
            }
        }
        // The axis and the marks' labels, less than a row of the card left out: a card says
        // all of what it has to before it keeps off them.
        for strip in [l.axis, l.strip] {
            if !strip.is_empty() {
                avoid.push((Keep::Rect(strip), 1.5));
            }
        }
        let lines: Vec<(Vec<(f32, f32)>, f32)> = (links.iter())
            .map(|&n| (l.links[n].pts.clone(), 3.0))
            .collect();
        let area = CardArea {
            size: l.size,
            cell_width: l.cell_width,
            flipped: &l.flipped,
        };
        let (card, r) = card.place(&area, (ax, g.mid()), reach, &avoid, &lines, m);
        // A label the card would cut goes whole, so no piece of it peeks out at its edge.
        let cut = |b: Rect| b.intersect(&r).is_some_and(|o| o.w > 0.5 && o.h > 0.5);
        let spec = TextSpec::new(TICK);
        let mut cuts: Vec<Rect> = (l.tick_labels.iter().map(|t| t.bounds(m))).collect();
        for mk in &l.marks {
            if let Some((text, x, y, anchor)) = &mk.label {
                cuts.push(crate::plot::layout::text_box(text, *x, *y, spec, *anchor, m));
            }
        }
        for b in cuts.into_iter().filter(|b| cut(*b)) {
            scene.push(Item::Path {
                path: PathData::rect(b.inflate(1.0)),
                fill: Some(Paint::solid(theme.surface, 1.0)),
                stroke: None,
            });
        }
        card.draw(&mut scene, r, theme, m);
    }
    scene
}

/// A task's outline, `by` px past what's drawn: a bar's rounded box, a milestone's diamond, a
/// summary's shape (its box, past half a px).
fn outline(g: &TaskGeom, by: f32) -> PathData {
    match g.shape {
        Shape::Milestone => diamond(g.bar.inflate(by * std::f32::consts::SQRT_2)),
        Shape::Summary if by <= 0.5 => summary(g.bar, None),
        _ => PathData::rounded(g.bar.inflate(by), [RADIUS + by; 4]),
    }
}

/// The time under the pointer as the chip says it: the day in a plan of whole days, else the
/// minute.
fn time_text(model: &GanttModel, t: f64) -> String {
    if model.whole_days {
        calendar::day_text(t, model.years(), &model.zone)
    } else {
        calendar::moment_text((t / 60.0).floor() * 60.0, model.years(), &model.zone)
    }
}

/// What a brush spans, as its chip says it: "9 Nov – 20 Nov · 12 days", "10:15 – 11:45 ·
/// 1 h 30 min".
fn span_text(model: &GanttModel, a: f64, b: f64) -> String {
    let zone = &model.zone;
    if model.whole_days {
        let days = ((b - a) / DAY).round().max(1.0) as i64;
        // The last day it takes in, not the one it stops at the start of.
        let last = (b - 1.0).max(a);
        let (from, to) = (
            calendar::short_day(a, zone),
            calendar::short_day(last, zone),
        );
        let n = calendar::days_text(days, i64::MAX);
        return match from == to {
            true => format!("{from} · {n}"),
            false => format!("{from} – {to} · {n}"),
        };
    }
    let same_day = model.days.date(a) == model.days.date(b);
    let at = |t: f64| match same_day {
        true => calendar::clock_text(t, zone),
        false => format!(
            "{} {}",
            calendar::short_day(t, zone),
            calendar::clock_text(t, zone)
        ),
    };
    format!("{} – {} · {}", at(a), at(b), calendar::span_text(b - a))
}

/// A task's card: its dates, how long, how far, its group, what it waits for, the spec's
/// `tooltip` fields, and a conflict's note.
fn card(model: &GanttModel, i: usize, theme: &Theme, m: &mut dyn Measure) -> Card {
    let t = &model.tasks[i];
    let row = |value: String, name: &str| Row {
        key: None,
        value,
        name: name.to_string(),
    };
    let mut rows = Vec::new();
    if t.end <= t.start {
        rows.push(row(model.start_text(i), "When"));
    } else {
        rows.push(row(model.start_text(i), "Start"));
        rows.push(row(model.end_text(i), "End"));
        rows.push(row(model.duration_text(i), "Duration"));
    }
    let keep = rows.len();
    let done = match t.children.is_empty() {
        true => t.progress,
        false => t.done,
    };
    if let Some(p) = done.filter(|_| t.end > t.start) {
        rows.push(row(format!("{} %", (p * 100.0).round()), "Progress"));
    }
    let color = model.color(i, theme);
    if let Some(g) = t.group {
        rows.push(Row {
            key: model.by_group.then_some(color),
            ..row(model.groups[g as usize].clone(), "Group")
        });
    }
    // What colours it, where that isn't its group: the field's own row, with its key.
    let color_field = (model.columns.color.as_deref()).filter(|_| !model.by_group);
    if let (Some(field), Some(n)) = (color_field, t.name) {
        rows.push(Row {
            key: Some(color),
            ..row(model.names[n as usize].clone(), field)
        });
    }
    if !t.after.is_empty() {
        let names: Vec<&str> = t.after.iter().map(|&p| model.label(p as usize)).collect();
        rows.push(row(names.join(", "), "After"));
    }
    let listed = (model.spec.tooltip.iter())
        .filter(|f| Some(f.as_str()) != color_field)
        .filter_map(|f| model.field(i, f).map(|v| row(v, f)))
        .take(CARD_FIELDS);
    rows.extend(listed);
    let note = model.late_text(i).map(|n| (n, theme.status[1]));
    Card::new(t.label.clone(), rows, keep, note, m)
}
