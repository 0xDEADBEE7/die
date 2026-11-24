//! The cursor's moves between tasks and the view's changes, for keys and the pointer.
//!
//! The cursor is a [`Cursor::Task`]. `j` `k` go down and up the rows; `h` `l` to the task that
//! starts before or after this one (the bar to its left or right); `]` `[` to a task that
//! comes after this one, or one it comes after ([`neighbour`] turns through them while the
//! surface keeps the anchor); `w` `b` to the next or previous milestone; `gg` `G` to the first
//! and last task; `H` `M` `L` to the top, middle and bottom task in view ([`motion`]). The
//! view's `x` is the time shown and its `y` the rows: [`fit`] shows the whole plan,
//! [`zoom_about`] keeps the time under the pointer still, [`pan`] moves the content with the
//! hand, [`wheel`] scrolls the rows where they overflow, [`reveal`] brings the cursor's row
//! (and its bar, when it's wholly out of the time shown) into view, [`centre`] centres it
//! (`zz`), and [`brush`] zooms the time to a drag. The time shown is never less than
//! [`MIN_SPAN`] nor more than [`MAX_OUT`] times the plan's, and a pan keeps its middle over
//! the plan.

use super::GanttModel;
use super::geom::{GanttLayout, RowGeom, RowKind};
use crate::interact::consts::BRUSH_MIN;
use crate::interact::{Action, Cursor, View};

/// The least time across the plot, in seconds: a minute.
pub const MIN_SPAN: f64 = 60.0;
/// The most time across the plot: this many times the plan's fitted span.
pub const MAX_OUT: f64 = 20.0;

/// The tasks with a row, in the order their bars start (the earlier row first of two that
/// start together).
fn by_start(model: &GanttModel, layout: &GanttLayout) -> Vec<usize> {
    let mut order = layout.order();
    let row = |i: usize| layout.task(i).map_or(0, |g| g.row);
    order.sort_by(|&a, &b| {
        (model.tasks[a].start)
            .total_cmp(&model.tasks[b].start)
            .then(row(a).cmp(&row(b)))
    });
    order
}

/// Whether a row's middle is inside the plot.
fn in_view(layout: &GanttLayout, r: &RowGeom) -> bool {
    let mid = r.y + r.h / 2.0;
    mid >= layout.plot.y && mid < layout.plot.bottom()
}

/// The tasks whose rows are in view, in row order.
pub fn tasks_in_view(layout: &GanttLayout) -> Vec<usize> {
    (layout.rows.iter())
        .filter(|r| in_view(layout, r))
        .filter_map(|r| match r.kind {
            RowKind::Task(i) => Some(i as usize),
            RowKind::Heading(_) => None,
        })
        .collect()
}

/// The first task with a row, in row order.
pub fn first(layout: &GanttLayout) -> Option<usize> {
    layout.order().first().copied()
}

/// The last task with a row.
pub fn last(layout: &GanttLayout) -> Option<usize> {
    layout.order().last().copied()
}

/// The tasks that come after `anchor` (`steps` > 0) or that it comes after (`steps` < 0), in
/// turn: `steps` on from `current` when it's one of them (round the ends), else from before
/// the first. The surface keeps the anchor while `]` and `[` repeat. None when it has none
/// with a row.
pub fn neighbour(
    model: &GanttModel,
    layout: &GanttLayout,
    anchor: usize,
    current: Option<usize>,
    steps: i32,
) -> Option<usize> {
    let t = model.tasks.get(anchor)?;
    let list = if steps >= 0 { &t.then } else { &t.after };
    let mut shown: Vec<usize> = (list.iter().map(|&i| i as usize))
        .filter(|&i| layout.task(i).is_some())
        .collect();
    // Down the rows, so `]` again goes to the next one below.
    shown.sort_by_key(|&i| layout.task(i).map_or(0, |g| g.row));
    if shown.is_empty() {
        return None;
    }
    let n = shown.len() as i64;
    let by = i64::from(steps.unsigned_abs());
    let at = current.and_then(|c| shown.iter().position(|&i| i == c));
    let i = match at {
        Some(p) => (p as i64 + by).rem_euclid(n),
        None => (by - 1).rem_euclid(n),
    };
    Some(shown[i as usize])
}

/// A key's move, `count` times, as a graph's [`crate::graph::nav::motion`]: `Series` down (+)
/// or up (−) the rows, `Step` to the task that starts after (+) or before (−), `Neighbour` to
/// a task that comes after (+) or that it comes after (−), `Peak` to the next (+) or previous
/// (−) milestone, `First` and `Last`, and `ViewEdge` to the top (−1), middle (0) or bottom (1)
/// task in view. With no cursor, a row's or a step's move puts it on the first task in view.
/// None when there's nowhere to go.
pub fn motion(
    model: &GanttModel,
    layout: &GanttLayout,
    cursor: Option<&Cursor>,
    action: &Action,
    count: u32,
) -> Option<Cursor> {
    let at = cursor
        .and_then(|c| model.task_of(c))
        .filter(|&i| layout.task(i).is_some());
    let n = i64::from(count.max(1));
    let to = |i: usize| Some(model.cursor(i));
    // Along a list from where the cursor is on it, stopping at its ends.
    let along = |list: &[usize], by: i64| -> Option<Cursor> {
        let Some(i) = at else {
            return tasks_in_view(layout)
                .first()
                .copied()
                .or(first(layout))
                .and_then(to);
        };
        let p = list.iter().position(|&x| x == i)? as i64;
        let q = (p + by).clamp(0, list.len() as i64 - 1);
        (q != p).then(|| model.cursor(list[q as usize]))
    };
    match action {
        Action::Series(by) => along(&layout.order(), i64::from(*by) * n),
        Action::Step(by) => along(&by_start(model, layout), i64::from(*by) * n),
        Action::Neighbour(by) => {
            let steps = by.signum() * count.max(1) as i32;
            neighbour(model, layout, at?, None, steps).and_then(to)
        }
        Action::Peak(by) => {
            let order = by_start(model, layout);
            let stones: Vec<usize> = (order.iter().copied())
                .filter(|&i| model.tasks[i].milestone)
                .collect();
            let Some(i) = at else {
                let pick = if *by >= 0 {
                    stones.first()
                } else {
                    stones.last()
                };
                return pick.copied().and_then(to);
            };
            // The milestones past the cursor's task, in the order they come.
            let p = order.iter().position(|&x| x == i)?;
            let ahead: Vec<usize> = if *by >= 0 {
                (order[p + 1..].iter().copied())
                    .filter(|x| stones.contains(x))
                    .collect()
            } else {
                (order[..p].iter().rev().copied())
                    .filter(|x| stones.contains(x))
                    .collect()
            };
            let k = (n as usize).min(ahead.len());
            k.checked_sub(1).and_then(|k| to(ahead[k]))
        }
        Action::First => first(layout).and_then(to),
        Action::Last => last(layout).and_then(to),
        Action::ViewEdge(edge) => {
            let seen = tasks_in_view(layout);
            let pick = match edge.signum() {
                -1 => seen.first(),
                1 => seen.last(),
                _ => seen.get(seen.len().saturating_sub(1) / 2),
            };
            pick.copied().and_then(to)
        }
        _ => None,
    }
}

/// The view fitting the whole plan in time; the rows stay where they are.
pub fn fit(view: &View) -> View {
    View {
        x: None,
        ..view.clone()
    }
}

/// A span of time within the zoom's limits for this plan.
fn limited(layout: &GanttLayout, span: f64) -> f64 {
    let most = ((layout.fit.1 - layout.fit.0) * MAX_OUT).max(MIN_SPAN);
    span.clamp(MIN_SPAN, most)
}

/// A window of time moved, if it must be, so its middle is over the plan.
fn over_plan(layout: &GanttLayout, (lo, hi): (f64, f64)) -> (f64, f64) {
    let mid = (lo + hi) / 2.0;
    let shift = mid.clamp(layout.fit.0, layout.fit.1) - mid;
    (lo + shift, hi + shift)
}

/// The view with its rows scrolled to `top` px, within what there is to scroll.
fn scrolled(layout: &GanttLayout, view: &View, top: f32) -> View {
    let top = top.clamp(0.0, layout.max_scroll());
    let mut v = view.clone();
    v.y = match top <= 0.0 && view.y.is_none() {
        true => None,
        false => Some((f64::from(top), f64::from(top + layout.plot.h))),
    };
    v
}

/// Zoom the time by `factor` (> 1 zooms in) about `at` (px), else the cursor's task where
/// it's in the time shown, else the plot's middle: the time there stays there. No closer than
/// [`MIN_SPAN`] across the plot, no further out than [`MAX_OUT`] times the plan.
pub fn zoom_about(
    model: &GanttModel,
    layout: &GanttLayout,
    view: &View,
    factor: f64,
    at: Option<(f32, f32)>,
    cursor: Option<&Cursor>,
) -> View {
    if !(factor.is_finite() && factor > 0.0) {
        return view.clone();
    }
    let (lo, hi) = layout.domain;
    let of_cursor = || {
        let t = &model.tasks[cursor.and_then(|c| model.task_of(c))?];
        let mid = (t.start + t.end) / 2.0;
        (mid >= lo && mid <= hi).then_some(mid)
    };
    let anchor = match at {
        Some((x, _)) => layout.time(x.clamp(layout.plot.x, layout.plot.right())),
        None => of_cursor().unwrap_or((lo + hi) / 2.0),
    };
    let span = (hi - lo).max(1e-9);
    let want = limited(layout, span / factor);
    let k = want / span;
    let mut v = view.clone();
    v.x = Some(over_plan(
        layout,
        (anchor - (anchor - lo) * k, anchor + (hi - anchor) * k),
    ));
    v
}

/// Pan by (dx, dy) px of pointer motion: the content follows the hand, the time across and
/// the rows down, the time's middle kept over the plan and the rows within their scroll.
pub fn pan(_model: &GanttModel, layout: &GanttLayout, view: &View, dx: f32, dy: f32) -> View {
    let mut v = view.clone();
    if dx != 0.0 {
        let dt = f64::from(dx) * layout.secs_per_px();
        let (lo, hi) = layout.domain;
        v.x = Some(over_plan(layout, (lo - dt, hi - dt)));
    }
    if dy != 0.0 {
        v = scrolled(layout, &v, layout.scroll - dy);
    }
    v
}

/// The wheel's or the trackpad's scroll, the content moving (dx, dy) px: up and down scroll
/// the rows where they overflow, else pan the time; sideways pans the time.
pub fn wheel(model: &GanttModel, layout: &GanttLayout, view: &View, dx: f32, dy: f32) -> View {
    if layout.max_scroll() > 0.0 {
        pan(model, layout, view, dx, dy)
    } else {
        pan(model, layout, view, dx + dy, 0.0)
    }
}

/// The view keeping the cursor's task in sight: its row scrolled into the plot (with its
/// group's heading, where it's the first under one), and its bar brought into the time shown
/// when it's wholly outside.
pub fn reveal(model: &GanttModel, layout: &GanttLayout, view: &View, cursor: &Cursor) -> View {
    let Some(i) = model.task_of(cursor) else {
        return view.clone();
    };
    let Some(g) = layout.task(i) else {
        return view.clone();
    };
    let plot = layout.plot;
    let mut v = view.clone();
    // The row's place among the rows, whatever the scroll.
    let row = &layout.rows[g.row as usize];
    let headed = (g.row.checked_sub(1))
        .map(|p| &layout.rows[p as usize])
        .filter(|p| matches!(p.kind, RowKind::Heading(_)));
    let top = headed.map_or(row.y, |p| p.y) - plot.y + layout.scroll;
    let bottom = row.y + row.h - plot.y + layout.scroll;
    if top < layout.scroll {
        v = scrolled(layout, &v, top);
    } else if bottom > layout.scroll + plot.h {
        v = scrolled(layout, &v, bottom - plot.h);
    }
    let t = &model.tasks[i];
    let (lo, hi) = layout.domain;
    if t.end < lo || t.start > hi {
        let span = hi - lo;
        // Whole where it fits, else from its start a tenth in.
        let from = if t.span() <= span * 0.8 {
            (t.start + t.end) / 2.0 - span / 2.0
        } else {
            t.start - span * 0.1
        };
        v.x = Some((from, from + span));
    }
    v
}

/// The view centred on the cursor's task at the same zoom (`zz`): its middle at the middle of
/// the time shown, its row at the middle of the plot where the rows scroll that far.
pub fn centre(model: &GanttModel, layout: &GanttLayout, view: &View, cursor: &Cursor) -> View {
    let Some(i) = model.task_of(cursor) else {
        return view.clone();
    };
    let t = &model.tasks[i];
    let (lo, hi) = layout.domain;
    let half = (hi - lo) / 2.0;
    let mid = (t.start + t.end) / 2.0;
    let mut v = view.clone();
    v.x = Some((mid - half, mid + half));
    if let Some(g) = layout.task(i) {
        let at = g.mid() - layout.plot.y + layout.scroll;
        v = scrolled(layout, &v, at - layout.plot.h / 2.0);
    }
    v
}

/// The view a brush from `x0` to `x1` (px) zooms to: the time between them, no less than
/// [`MIN_SPAN`]. None for a drag under [`BRUSH_MIN`] px, which is a click.
pub fn brush(
    _model: &GanttModel,
    layout: &GanttLayout,
    view: &View,
    x0: f32,
    x1: f32,
) -> Option<View> {
    let p = layout.plot;
    let (a, b) = (
        x0.min(x1).clamp(p.x, p.right()),
        x0.max(x1).clamp(p.x, p.right()),
    );
    if b - a < BRUSH_MIN {
        return None;
    }
    let (mut lo, mut hi) = (layout.time(a), layout.time(b));
    if hi - lo < MIN_SPAN {
        let mid = (lo + hi) / 2.0;
        (lo, hi) = (mid - MIN_SPAN / 2.0, mid + MIN_SPAN / 2.0);
    }
    let mut v = view.clone();
    v.x = Some((lo, hi));
    Some(v)
}

/// A view part way between two (for the 180 ms eases): the time and the rows interpolated, a
/// fitted time taken as the plan's and unscrolled rows as the top; everything else `to`'s.
pub fn tween(model: &GanttModel, from: &View, to: &View, t: f32) -> View {
    let t = f64::from(t.clamp(0.0, 1.0));
    let mut v = to.clone();
    if t >= 1.0 {
        return v;
    }
    let lerp = |a: f64, b: f64| a + (b - a) * t;
    if from.x != to.x {
        let (a, b) = (
            from.x.unwrap_or_else(|| model.fit()),
            to.x.unwrap_or_else(|| model.fit()),
        );
        v.x = Some((lerp(a.0, b.0), lerp(a.1, b.1)));
    }
    if from.y != to.y {
        let high = from.y.or(to.y).map_or(0.0, |y| y.1 - y.0);
        let (a, b) = (from.y.unwrap_or((0.0, high)), to.y.unwrap_or((0.0, high)));
        v.y = Some((lerp(a.0, b.0), lerp(a.1, b.1)));
    }
    v
}
