//! The cursor's moves between nodes and the view's changes, for keys and the pointer.
//!
//! The cursor is a [`Cursor::Node`]. `h` `j` `k` `l` go to the nearest node in a 90° cone that
//! way on screen ([`step`]); `]` `[` turn through a node's neighbours by angle
//! ([`neighbour`]); `gg` `G` go to the first and last node ([`first`], [`last`]). The view is a
//! window onto the layout's units: [`fit`] shows the whole graph, [`zoom_about`] keeps the
//! point under the pointer still, [`pan`] moves the content with the hand, [`reveal`] keeps the
//! cursor's node in view by recentring on it at the same zoom (fig's viewport rule), and
//! [`centre`] recentres outright (`zz`). Zoom stays within [`MAX_ZOOM`] of fit each way in and
//! [`MIN_ZOOM`] out, and a pan keeps the window's middle over the fitted graph. [`drag_node`]
//! moves a node under the pointer and pins it, along its rank only in a tree or layers, and
//! nothing else moves: the window holds still while the graph's box changes under it.

use super::GraphModel;
use super::geom::GraphLayout;
use crate::interact::{Action, Cursor, View};
use std::f32::consts::TAU;

/// The furthest in a view zooms: this many times fit.
pub const MAX_ZOOM: f64 = 32.0;
/// The furthest out: the window this many times fit's.
pub const MIN_ZOOM: f64 = 2.0;
/// How far inside the plot's edge the cursor's node is kept, as a share of its size.
const REVEAL_MARGIN: f32 = 0.06;

/// A way on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Leftwards.
    Left,
    /// Rightwards.
    Right,
    /// Upwards.
    Up,
    /// Downwards.
    Down,
}

/// The nearest shown node from node `from` in a 90° cone towards `dir` on screen, scored by
/// its distance along that way plus twice its distance across (so a node straight ahead beats
/// one nearer but off to the side); None when the cone is empty.
pub fn step(_model: &GraphModel, layout: &GraphLayout, from: usize, dir: Dir) -> Option<usize> {
    let a = layout.node(from)?.at;
    let (ux, uy) = match dir {
        Dir::Left => (-1.0, 0.0),
        Dir::Right => (1.0, 0.0),
        Dir::Up => (0.0, -1.0),
        Dir::Down => (0.0, 1.0),
    };
    let mut best: Option<(f32, usize)> = None;
    for (i, g) in layout.nodes.iter().enumerate() {
        let Some(g) = g else { continue };
        if i == from {
            continue;
        }
        let (dx, dy) = (g.at.0 - a.0, g.at.1 - a.1);
        let along = dx * ux + dy * uy;
        let across = (dx * uy - dy * ux).abs();
        if along <= 0.0 || across > along {
            continue;
        }
        let score = along + 2.0 * across;
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i)
}

/// The angle of `to` about `from` on screen, clockwise from straight up, in [0, 2π).
fn bearing(from: (f32, f32), to: (f32, f32)) -> f32 {
    let a = (to.0 - from.0).atan2(-(to.1 - from.1));
    if a < 0.0 { a + TAU } else { a }
}

/// The neighbours of `anchor` in turn by their bearing about it, clockwise from straight up:
/// `steps` on from `current` (backwards when negative, round the ends), or from before the
/// first (`steps` > 0) or after the last when `current` isn't one of them. The surface keeps
/// the anchor while `]` and `[` repeat. None when it has no shown neighbour.
pub fn neighbour(
    model: &GraphModel,
    layout: &GraphLayout,
    anchor: usize,
    current: Option<usize>,
    steps: i32,
) -> Option<usize> {
    let a = layout.node(anchor)?.at;
    let mut around: Vec<(f32, usize)> = model
        .neighbours(anchor)
        .into_iter()
        .filter_map(|j| layout.node(j).map(|g| (bearing(a, g.at), j)))
        .collect();
    if around.is_empty() {
        return None;
    }
    around.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    let n = around.len() as i64;
    let at = current.and_then(|c| around.iter().position(|&(_, j)| j == c));
    let i = match at {
        Some(p) => (p as i64 + i64::from(steps)).rem_euclid(n),
        None if steps > 0 => (i64::from(steps) - 1).rem_euclid(n),
        None => (n + i64::from(steps)).rem_euclid(n),
    };
    Some(around[i as usize].1)
}

/// The first shown node, in the graph's order.
pub fn first(model: &GraphModel, layout: &GraphLayout) -> Option<usize> {
    (0..model.len()).find(|&i| layout.node(i).is_some())
}

/// The last shown node, in the graph's order.
pub fn last(model: &GraphModel, layout: &GraphLayout) -> Option<usize> {
    (0..model.len()).rev().find(|&i| layout.node(i).is_some())
}

/// The shown node nearest the plot's middle: where a first move puts the cursor.
pub fn nearest_middle(layout: &GraphLayout) -> Option<usize> {
    let p = layout.plot;
    let c = (p.x + p.w / 2.0, p.y + p.h / 2.0);
    let mut best: Option<(f32, usize)> = None;
    for (i, g) in layout.nodes.iter().enumerate() {
        if let Some(g) = g {
            let d = (g.at.0 - c.0).hypot(g.at.1 - c.1);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// A key's move, `count` times, as charts' [`crate::plot::nav::motion`]: `Step` left (−) or
/// right (+), `Series` down (+) or up (−), `Neighbour` the first (+) or last (−) neighbour by
/// bearing (repeating `]` goes through [`neighbour`] with the anchor kept), `First` and `Last`.
/// With no cursor, a step or a series places it on the node nearest the plot's middle. None
/// when there's nowhere to go.
pub fn motion(
    model: &GraphModel,
    layout: &GraphLayout,
    cursor: Option<&Cursor>,
    action: &Action,
    count: u32,
) -> Option<Cursor> {
    let at = cursor
        .and_then(|c| model.node_of(c))
        .filter(|&i| layout.node(i).is_some());
    let n = count.max(1);
    let go = |dir: Dir, by: i32| -> Option<Cursor> {
        let Some(mut i) = at else {
            return nearest_middle(layout).map(|i| model.cursor(i));
        };
        let dir = match (dir, by < 0) {
            (Dir::Right, true) => Dir::Left,
            (Dir::Down, true) => Dir::Up,
            (d, _) => d,
        };
        let mut moved = false;
        for _ in 0..n.saturating_mul(by.unsigned_abs()) {
            match step(model, layout, i, dir) {
                Some(j) => {
                    i = j;
                    moved = true;
                }
                None => break,
            }
        }
        moved.then(|| model.cursor(i))
    };
    match action {
        Action::Step(by) => go(Dir::Right, *by),
        Action::Series(by) => go(Dir::Down, *by),
        Action::Neighbour(by) => {
            let i = at?;
            neighbour(model, layout, i, None, by.signum() * n as i32).map(|j| model.cursor(j))
        }
        Action::First => first(model, layout).map(|i| model.cursor(i)),
        Action::Last => last(model, layout).map(|i| model.cursor(i)),
        _ => None,
    }
}

/// The view fitting the whole graph (its domains forgotten); pins stay.
pub fn fit(view: &View) -> View {
    View {
        x: None,
        y: None,
        ..view.clone()
    }
}

/// The view with its window set explicitly to what `layout` shows.
fn held(layout: &GraphLayout, view: &View) -> View {
    View {
        x: Some(layout.domain.0),
        y: Some(layout.domain.1),
        ..view.clone()
    }
}

/// Zoom by `factor` (> 1 zooms in) about `at` (px), else the cursor's node, else the plot's
/// middle: the layout point there stays there. Within [`MAX_ZOOM`] in and [`MIN_ZOOM`] out of
/// fit.
pub fn zoom_about(
    model: &GraphModel,
    layout: &GraphLayout,
    view: &View,
    factor: f64,
    at: Option<(f32, f32)>,
    cursor: Option<&Cursor>,
) -> View {
    if !(factor.is_finite() && factor > 0.0) {
        return view.clone();
    }
    let p = layout.plot;
    let anchor_px = at
        .or_else(|| {
            cursor
                .and_then(|c| model.node_of(c))
                .and_then(|i| layout.node(i))
                .map(|g| g.at)
        })
        .unwrap_or((p.x + p.w / 2.0, p.y + p.h / 2.0));
    let anchor = layout.frame.unit(anchor_px);
    let ((x0, x1), (y0, y1)) = layout.domain;
    let fit_span = (layout.fit.0.1 - layout.fit.0.0).abs().max(1e-12);
    let span = (x1 - x0).abs().max(1e-12);
    let want = (span / factor).clamp(fit_span / MAX_ZOOM, fit_span * MIN_ZOOM);
    let f = span / want;
    let scale = |lo: f64, hi: f64, c: f64| (c + (lo - c) / f, c + (hi - c) / f);
    let mut v = view.clone();
    v.x = Some(scale(x0, x1, anchor.0));
    v.y = Some(scale(y0, y1, anchor.1));
    clamp(layout, &mut v);
    v
}

/// Pan by (dx, dy) px of pointer motion: the content follows the hand, the window's middle kept
/// over the fitted graph.
pub fn pan(_model: &GraphModel, layout: &GraphLayout, view: &View, dx: f32, dy: f32) -> View {
    let (kx, ky) = layout.frame.k;
    let ((x0, x1), (y0, y1)) = layout.domain;
    let mx = f64::from(dx) / kx;
    let my = f64::from(dy) / ky;
    let mut v = view.clone();
    v.x = Some((x0 - mx, x1 - mx));
    v.y = Some((y0 + my, y1 + my));
    clamp(layout, &mut v);
    v
}

/// Keeps a view's window's middle over the fitted graph's window.
fn clamp(layout: &GraphLayout, v: &mut View) {
    let (fx, fy) = layout.fit;
    for (d, f) in [(&mut v.x, fx), (&mut v.y, fy)] {
        if let Some((lo, hi)) = d {
            let mid = (*lo + *hi) / 2.0;
            let (a, b) = (f.0.min(f.1), f.0.max(f.1));
            let shift = mid.clamp(a, b) - mid;
            *lo += shift;
            *hi += shift;
        }
    }
}

/// The view keeping the cursor's node in sight: unchanged while it's inside the plot (a little
/// in from its edges), else recentred on it at the same zoom.
pub fn reveal(model: &GraphModel, layout: &GraphLayout, view: &View, cursor: &Cursor) -> View {
    let Some(g) = model.node_of(cursor).and_then(|i| layout.node(i)) else {
        return view.clone();
    };
    let p = layout.plot;
    let mx = p.w * REVEAL_MARGIN;
    let my = p.h * REVEAL_MARGIN;
    let inside = g.at.0 >= p.x + mx
        && g.at.0 <= p.right() - mx
        && g.at.1 >= p.y + my
        && g.at.1 <= p.bottom() - my;
    if inside {
        return view.clone();
    }
    centre(model, layout, view, cursor)
}

/// The view centred on the cursor's node at the same zoom (`zz`).
pub fn centre(model: &GraphModel, layout: &GraphLayout, view: &View, cursor: &Cursor) -> View {
    let Some(i) = model.node_of(cursor) else {
        return view.clone();
    };
    let at = model.position(view, i);
    let ((x0, x1), (y0, y1)) = layout.domain;
    let (hx, hy) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let mut v = view.clone();
    v.x = Some((at.0 - hx, at.0 + hx));
    v.y = Some((at.1 - hy, at.1 + hy));
    v
}

/// Where on node `id` the pointer at `at` took it, from its centre: what a drag keeps under the
/// pointer.
pub fn grab(model: &GraphModel, layout: &GraphLayout, id: &str, at: (f32, f32)) -> (f32, f32) {
    match model.index_of(id).and_then(|i| layout.node(i)) {
        Some(g) => (at.0 - g.at.0, at.1 - g.at.1),
        None => (0.0, 0.0),
    }
}

/// The view with node `id` moved so the place it was taken by (`grab`) is under the pointer at
/// `to`, and pinned there; in a tree or layers it slides along its rank only. The window is
/// held where it is, so nothing else moves while the graph's box changes.
pub fn drag_node(
    model: &GraphModel,
    layout: &GraphLayout,
    view: &View,
    id: &str,
    to: (f32, f32),
    grab: (f32, f32),
) -> View {
    let Some(i) = model.index_of(id) else {
        return view.clone();
    };
    let mut v = held(layout, view);
    let mut p = layout.frame.unit((to.0 - grab.0, to.1 - grab.1));
    if model.ranked() {
        p.1 = model.position(view, i).1;
    }
    v.pinned.insert(id.into(), p);
    v
}

/// The view with node `id` unpinned, back in its layout's place.
pub fn unpin(view: &View, id: &str) -> View {
    let mut v = view.clone();
    v.pinned.remove(id);
    v
}

/// A view part way between two (for the 180 ms eases): the windows and the pinned nodes'
/// places interpolated (a pin coming or going from or to the layout's place), everything else
/// `to`'s.
pub fn tween(model: &GraphModel, from: &View, to: &View, t: f32) -> View {
    let t = f64::from(t.clamp(0.0, 1.0));
    let lerp = |a: f64, b: f64| a + (b - a) * t;
    let pair = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| match (a, b) {
        (Some(a), Some(b)) => Some((lerp(a.0, b.0), lerp(a.1, b.1))),
        (_, b) => b,
    };
    let mut v = to.clone();
    v.x = pair(from.x, to.x);
    v.y = pair(from.y, to.y);
    if t < 1.0 {
        let ids: Vec<Box<str>> = from
            .pinned
            .keys()
            .chain(to.pinned.keys())
            .cloned()
            .collect();
        for id in ids {
            let Some(i) = model.index_of(&id) else {
                continue;
            };
            let (a, b) = (model.position(from, i), model.position(to, i));
            if a != b {
                v.pinned.insert(id, (lerp(a.0, b.0), lerp(a.1, b.1)));
            }
        }
    }
    v
}
