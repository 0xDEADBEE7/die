//! The gantt at rest: the header, the time axis, the shaded days and the grid, the rows'
//! labels, the links, the bars, the marks and today's line, with their regions; and the pieces
//! the overlay draws again on top.

use super::GanttModel;
use super::geom::{
    DIAMOND_RING, GanttLayout, LinkGeom, RADIUS, RowGeom, RowKind, SUMMARY, Shape, TaskGeom,
    lay_out,
};
use crate::interact::View;
use crate::plot::layout::{hair, legend_key};
use crate::plot::marks::mix;
use crate::scene::{Hit, Item, Measure, Paint, PathData, Rect, Scene, Stroke, Verb};
use crate::theme::{Color, Theme};

/// How much of the surface shows through a bar's part not yet done, where the plan tracks
/// progress: the bar's colour at 35 %.
pub const UNDONE: f32 = 0.65;
/// The dash of a tentative task's outline, and of a mark's line.
pub const DASH: (f32, f32) = (4.0, 3.0);
/// The narrowest a day is shaded at, in px.
pub const DAY_MIN: f32 = 6.0;
/// The most days shaded at once: past that they'd be under [`DAY_MIN`] on any screen.
const DAYS_MOST: usize = 2000;

/// The base layer: the header, the axis, then the rows clipped under it, at rest; or with
/// every name hidden the chart's note that they are. Regions: the area as chrome, the plot,
/// the axis strip, each task's bar and its label in the gutter, the legend's keys and the
/// title.
pub fn base(
    model: &GanttModel,
    view: &View,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, GanttLayout) {
    base_beside(model, view, size, size.0, scale, theme, m)
}

/// [`base`] in part of an area `area_w` wide (beside the detail panel): its paddings are the
/// whole area's, so the title and the rows stay where they were as the panel opens.
pub fn base_beside(
    model: &GanttModel,
    view: &View,
    size: (f32, f32),
    area_w: f32,
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, GanttLayout) {
    let l = lay_out(model, view, size, area_w, scale, theme, m);
    let mut scene = Scene::new(size.0, size.1, scale);
    scene.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
    scene.region(l.plot, Hit::Plot(0));
    let strips = l.axis.union(&l.strip);
    if !strips.is_empty() {
        scene.region(strips, Hit::AxisX(0));
    }
    for item in &l.items {
        let n = (model.names.iter())
            .position(|x| **x == *item.key.name)
            .unwrap_or(0);
        legend_key(&mut scene, item, model.name_color(n, theme), theme);
        scene.push(item.name.item(theme));
    }
    for label in l.chrome.iter().chain(&l.tick_labels) {
        scene.push(label.item(theme));
    }
    let plot = l.plot;
    let rows = Rect::new(l.gutter.x, plot.y, plot.right() - l.gutter.x, plot.h);
    if l.rows.is_empty() {
        // Every name hidden: the plot says so, as a chart's does, and how to bring them back.
        crate::plot::errors::all_hidden(&mut scene, rows, theme, m);
        regions(&mut scene, model, &l);
        return (scene, l);
    }
    // How far down the rows reach: the grid, the shading and the marks stop with them.
    let foot = (l.rows.last())
        .map_or(plot.bottom(), |r| (r.y + r.h).min(plot.bottom()))
        .max(plot.y);
    let body = Rect::new(plot.x, plot.y, plot.w, foot - plot.y);
    let grid = model.spec.view.grid;

    scene.push(Item::PushClip(body));
    if grid {
        shade_days(&mut scene, model, &l, body, theme);
    }
    for mk in l.marks.iter() {
        if let Some(x1) = mk.x1 {
            scene.push(Item::Path {
                path: PathData::rect(Rect::new(mk.x, body.y, (x1 - mk.x).max(1.0), body.h)),
                fill: Some(Paint::solid(mk.color, 0.10)),
                stroke: None,
            });
        }
    }
    if grid {
        for t in &l.ticks {
            let x = l.px(t.value);
            if x < plot.x - 0.5 || x > plot.right() + 0.5 {
                continue;
            }
            let color = if t.major { theme.baseline } else { theme.grid };
            hair(&mut scene, (x, body.y), (x, body.bottom()), color);
        }
    }
    scene.push(Item::PopClip);

    scene.push(Item::PushClip(rows));
    if grid {
        // Under each group's last row.
        for (k, r) in l.rows.iter().enumerate() {
            let last_of_group = matches!(r.kind, RowKind::Task(_))
                && (l.rows.get(k + 1)).is_none_or(|n| matches!(n.kind, RowKind::Heading(_)))
                && !model.groups.is_empty();
            if last_of_group && l.row_in_view(r) {
                let y = r.y + r.h;
                hair(&mut scene, (rows.x, y), (rows.right(), y), theme.grid);
            }
        }
    }
    for r in l.rows.iter().filter(|r| l.row_in_view(r)) {
        scene.push(r.label.item(theme));
    }
    scene.push(Item::PopClip);

    scene.push(Item::PushClip(plot));
    for r in l.rows.iter().filter(|r| l.row_in_view(r)) {
        heading_line(&mut scene, r);
    }
    for link in l.links.iter().filter(|k| link_in_view(&l, k)) {
        self::link(&mut scene, link, link_color(link, theme), 1.0, false);
    }
    for (i, g) in l.tasks.iter().enumerate() {
        let Some(g) = g else { continue };
        if !l.row_in_view(&l.rows[g.row as usize]) {
            continue;
        }
        bar(&mut scene, model, i, g, g.color, theme);
        if let Some(d) = &g.date {
            scene.push(d.item(theme));
        }
    }
    scene.push(Item::PopClip);

    // The marks' lines and today's, over the bars, from their labels down.
    for mk in &l.marks {
        let top = if l.strip.is_empty() || mk.label.is_none() {
            body.y
        } else {
            l.strip.bottom() - 1.0
        };
        if mk.x1.is_none() {
            let mut stroke = Stroke::hairline(mk.color, 1.0);
            if !mk.today {
                stroke.dash = Some(DASH);
            }
            scene.push(Item::Path {
                path: PathData::polyline(&[(mk.x, top), (mk.x, body.bottom())]),
                fill: None,
                stroke: Some(stroke),
            });
        }
        if let Some((text, x, y, anchor)) = &mk.label {
            scene.push(Item::Text {
                text: text.as_str().into(),
                x: *x,
                y: *y,
                spec: crate::scene::TextSpec::new(crate::plot::layout::TICK),
                anchor: *anchor,
                color: mk.color,
                alpha: 1.0,
                max_width: None,
            });
        }
    }
    if let Some((_, thumb)) = l.scrollbar {
        scene.push(Item::Path {
            path: PathData::rounded(thumb, [thumb.w / 2.0; 4]),
            fill: Some(Paint::solid(theme.muted, 0.55)),
            stroke: None,
        });
    }
    regions(&mut scene, model, &l);
    (scene, l)
}

/// The regions over the plot's: each task's label in the gutter and its bar, the legend's
/// keys and the title.
fn regions(scene: &mut Scene, model: &GanttModel, l: &GanttLayout) {
    let gutter = Rect::new(l.gutter.x, l.plot.y, l.plot.x - l.gutter.x - 4.0, l.plot.h);
    for (i, g) in l.tasks.iter().enumerate() {
        let Some(g) = g else { continue };
        let key: Box<str> = model.key(i).into();
        let cell = Rect::new(gutter.x, g.strip.y, gutter.w, g.strip.h);
        if let Some(r) = cell.intersect(&gutter) {
            scene.region(r, Hit::Task(key.clone()));
        }
        if let Some(r) = g.reach().intersect(&l.plot) {
            scene.region(r, Hit::Task(key));
        }
    }
    for (k, r) in &l.legend {
        scene.region(*r, Hit::LegendKey(k.clone()));
    }
    if !l.title.is_empty() {
        scene.region(l.title, Hit::PanelTitle(0));
    }
}

/// The days that don't count, shaded behind the bars: Saturdays and Sundays and the
/// calendar's days off, in a plan of whole days, where a day is [`DAY_MIN`] px wide or more.
fn shade_days(scene: &mut Scene, model: &GanttModel, l: &GanttLayout, body: Rect, theme: &Theme) {
    let day_px = (super::calendar::DAY / l.secs_per_px()) as f32;
    if !model.whole_days || day_px < DAY_MIN {
        return;
    }
    let Some(days) = model.days.each(l.domain.0, l.domain.1, DAYS_MOST) else {
        return;
    };
    // Runs of shaded days as one rectangle each, so no seam shows between two.
    let mut run: Option<(f32, f32)> = None;
    let flush = |run: &mut Option<(f32, f32)>, scene: &mut Scene| {
        if let Some((a, b)) = run.take() {
            let (a, b) = (a.max(body.x), b.min(body.right()));
            if b > a {
                scene.push(Item::Path {
                    path: PathData::rect(Rect::new(a, body.y, b - a, body.h)),
                    fill: Some(Paint::solid(theme.grid, 0.35)),
                    stroke: None,
                });
            }
        }
    };
    for (d, from, to) in days {
        if model.days.shaded(d) {
            let (a, b) = (l.px(from), l.px(to));
            run = Some(match run {
                Some((s, _)) => (s, b),
                None => (a, b),
            });
        } else {
            flush(&mut run, scene);
        }
    }
    flush(&mut run, scene);
}

/// A heading's line across its group's span.
fn heading_line(scene: &mut Scene, r: &RowGeom) {
    let Some((a, b, color)) = r.line else { return };
    let y = (r.y + r.h / 2.0).round();
    scene.push(Item::Path {
        path: PathData::rect(Rect::new(a, y - 1.0, b - a, 2.0)),
        fill: Some(Paint::solid(color, 0.5)),
        stroke: None,
    });
}

/// Whether some of a link's path is in the plot, down the screen.
pub(crate) fn link_in_view(l: &GanttLayout, k: &LinkGeom) -> bool {
    let ys = k.pts.iter().map(|p| p.1);
    let (lo, hi) = ys.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), y| {
        (a.min(y), b.max(y))
    });
    hi >= l.plot.y && lo <= l.plot.bottom()
}

/// A link's colour at rest: the baseline's grey a step towards the muted text (as a graph's
/// edges), or the warning's where it's in conflict.
pub(crate) fn link_color(k: &LinkGeom, theme: &Theme) -> Color {
    if k.conflict {
        theme.status[1]
    } else {
        crate::graph::edge_grey(theme)
    }
}

/// A link in a colour: a hairline (or 1.5 px, `lit`) with its arrowhead.
pub(crate) fn link(scene: &mut Scene, k: &LinkGeom, color: Color, alpha: f32, lit: bool) {
    let stroke = if lit {
        Stroke::solid(1.5, color, alpha)
    } else {
        Stroke::hairline(color, alpha)
    };
    scene.push(Item::Path {
        path: PathData::polyline(&k.pts),
        fill: None,
        stroke: Some(stroke),
    });
    let mut head = PathData::polyline(&k.arrow);
    head.verbs.push(Verb::Close);
    scene.push(Item::Path {
        path: head,
        fill: Some(Paint::solid(color, alpha)),
        stroke: None,
    });
}

/// A diamond in a box.
pub(crate) fn diamond(r: Rect) -> PathData {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let mut p = PathData::polyline(&[(cx, r.y), (r.right(), cy), (cx, r.bottom()), (r.x, cy)]);
    p.verbs.push(Verb::Close);
    p
}

/// A summary's bar with a tick down at each end, in a box `r`; `upto` px of it from the left
/// when given (its part done).
pub(crate) fn summary(r: Rect, upto: Option<f32>) -> PathData {
    let (x0, x1, y0) = (r.x, r.right(), r.y);
    let (flat, tip) = (y0 + SUMMARY.0, y0 + SUMMARY.0 + SUMMARY.1);
    let t = SUMMARY.1.min(r.w / 2.0).max(0.01);
    let slope = SUMMARY.1 / t;
    let mut pts = vec![(x0, y0)];
    match upto.map(|u| (x0 + u).clamp(x0, x1)) {
        None => pts.extend([(x1, y0), (x1, tip), (x1 - t, flat), (x0 + t, flat)]),
        // The ticks' slanted edges, where the part done ends in one.
        Some(xd) if xd <= x0 + t => {
            pts.extend([(xd, y0), (xd, tip - (xd - x0) * slope)]);
        }
        Some(xd) if xd <= x1 - t => pts.extend([(xd, y0), (xd, flat), (x0 + t, flat)]),
        Some(xd) => pts.extend([
            (xd, y0),
            (xd, flat + (xd - (x1 - t)) * slope),
            (x1 - t, flat),
            (x0 + t, flat),
        ]),
    }
    pts.push((x0, tip));
    let mut p = PathData::polyline(&pts);
    p.verbs.push(Verb::Close);
    p
}

/// A task as it's drawn, in `color`: a bar (its part done solid over the rest at 35 % where
/// the plan tracks progress; an outline, dashed, for a tentative one), a milestone's diamond
/// with its ring, or a summary's bar.
pub(crate) fn bar(
    scene: &mut Scene,
    model: &GanttModel,
    _i: usize,
    g: &TaskGeom,
    color: Color,
    theme: &Theme,
) {
    match g.shape {
        Shape::Milestone => scene.push(Item::Path {
            path: diamond(g.bar),
            fill: Some(Paint::solid(color, 1.0)),
            stroke: Some(Stroke::solid(DIAMOND_RING, theme.surface, 1.0)),
        }),
        Shape::Summary => {
            // A summary is the chrome's, not a colour's: muted, its part done in the text's.
            let (rest, done) = match color == g.color {
                true => (theme.muted, theme.text),
                false => (mix(theme.muted, theme.strong, 0.25), theme.strong),
            };
            scene.push(Item::Path {
                path: summary(g.bar, None),
                fill: Some(Paint::solid(rest, 1.0)),
                stroke: None,
            });
            if let Some(d) = g.done.filter(|d| *d > 0.25) {
                scene.push(Item::Path {
                    path: summary(g.bar, Some(d)),
                    fill: Some(Paint::solid(done, 1.0)),
                    stroke: None,
                });
            }
        }
        Shape::Bar if g.dash => {
            let mut stroke = Stroke::solid(1.25, color, 1.0);
            stroke.dash = Some(DASH);
            scene.push(Item::Path {
                path: PathData::rounded(g.bar.inflate(-0.625), [RADIUS; 4]),
                fill: Some(Paint::solid(mix(color, theme.surface, 0.86), 1.0)),
                stroke: Some(stroke),
            });
        }
        Shape::Bar => {
            let whole = PathData::rounded(g.bar, [RADIUS; 4]);
            if !model.tracks_progress {
                scene.push(Item::Path {
                    path: whole,
                    fill: Some(Paint::solid(color, 1.0)),
                    stroke: None,
                });
                return;
            }
            scene.push(Item::Path {
                path: whole,
                fill: Some(Paint::solid(mix(color, theme.surface, UNDONE), 1.0)),
                stroke: None,
            });
            let d = g.done.unwrap_or(0.0);
            if d > 0.25 {
                // Square where it meets the rest, round where the bar ends.
                let right = if d >= g.bar.w - 0.5 { RADIUS } else { 0.0 };
                let part = Rect::new(g.bar.x, g.bar.y, d, g.bar.h);
                scene.push(Item::Path {
                    path: PathData::rounded(part, [RADIUS, right, right, RADIUS]),
                    fill: Some(Paint::solid(color, 1.0)),
                    stroke: None,
                });
            }
        }
    }
}
