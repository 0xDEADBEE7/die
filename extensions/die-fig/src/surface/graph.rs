//! A graph in a window (briefs/mouse-ux.md §4.5, the design's controls, graph column), as
//! [`super::Act`] is a chart's: hover a node and then an edge, the node in focus after
//! [`DWELL_EMPHASIS_MS`] and let go after as long; a click puts the cursor on a node (on an
//! edge, its nearer end), ⇧-click sets the reference and the path shows, ⌥-click shows only the
//! node's group, a double-click opens a node's or an edge's detail and on the background fits;
//! a drag on a node moves and pins it (one undo step, Esc puts it back), on the background pans
//! 1:1, as ⌥- and middle-drags do anywhere; the wheel pans in two dimensions and ⌘-wheel zooms
//! about the pointer, a burst keeping its first meaning. Every rule of mouse-ux §3 the chart
//! follows holds: the focusing press only focuses, the press is held by its window and button,
//! the hover is worked out again after the view moves, and the pointer leaving clears it.

use super::{Burst, Change, Ease, FigState, Keys, Kit, Pane, Snap, measured, ms};
use crate::detail::Item;
use crate::graph::{self, Frame, GraphLayout, GraphModel, Target, nav};
use crate::interact::consts::{
    CLICK_SLOP, DWELL_EMPHASIS_MS, MOTION_MS, SCROLL_BURST_MS, ZOOM_STEP,
};
use crate::interact::{Action, Cursor, Gesture, Toggles, Tween, View};
use crate::scene::Hit;
use die_core::{Button, Ctx, Mods, PointerKind, Sub, SurfacePointer};
use std::cell::RefCell;
use std::rc::Weak;
use std::sync::Arc;
use std::time::Duration;

/// A graph's pane with what acting on it needs.
pub struct GraphAct<'a> {
    /// The pane.
    pub pane: &'a mut Pane,
    /// The graph.
    pub model: &'a Arc<GraphModel>,
    /// The figure's settings, for undo.
    pub toggles: Toggles,
    /// Whether discrete changes ease.
    pub motion: bool,
    /// The clock.
    pub now: Duration,
    /// The fonts, which say where a fitted window lands for an ease.
    pub kit: &'a mut Kit,
    /// What the pointer's act has to say, for `*messages*`.
    pub say: Option<String>,
}

impl GraphAct<'_> {
    /// Where the graph landed, as last drawn.
    pub fn layout(&self) -> Option<&GraphLayout> {
        self.pane.graph.as_ref()
    }

    /// The view with its window spelled out, as the frame shows it: what fits where it says
    /// None, measured as the base measures it, so an ease to fit lands where the fit draws.
    fn explicit(&mut self, v: &View) -> View {
        let Some(plot) = self.pane.graph.as_ref().map(|l| l.plot) else {
            return v.clone();
        };
        let (x, y) = match (v.x, v.y) {
            (Some(x), Some(y)) => (x, y),
            _ => {
                let scale = self.pane.scale();
                let model = self.model;
                let fit = measured(self.kit.book.as_mut(), scale, |m| {
                    graph::geom::fit(model, v, plot, m)
                });
                (v.x.unwrap_or(fit.0), v.y.unwrap_or(fit.1))
            }
        };
        let d = Frame::new(plot, x, y, self.model.uniform()).domain();
        View {
            x: Some(d.0),
            y: Some(d.1),
            ..v.clone()
        }
    }

    /// Change the view, as `how` says: a discrete change eases (the window and the pins), the
    /// hand's never does.
    pub fn set(&mut self, v: View, how: Change) -> bool {
        if v == self.pane.view {
            return false;
        }
        let before = Snap {
            view: self.pane.view.clone(),
            toggles: self.toggles,
        };
        match how {
            Change::Step => {
                self.pane.undo.push(before);
                self.pane.keyed = None;
            }
            Change::Keyed => {
                let within = (self.pane.keyed)
                    .is_some_and(|t| self.now < t + Duration::from_millis(SCROLL_BURST_MS));
                if !within {
                    self.pane.undo.push(before);
                }
                self.pane.keyed = Some(self.now);
            }
            Change::Live | Change::Follow => {}
        }
        let t = ms(self.now);
        let ease = if how == Change::Live || !self.motion {
            None
        } else {
            let shown = self.pane.shown(t);
            let (from, to) = (self.explicit(&shown), self.explicit(&v));
            Some(Ease {
                tween: Tween::new(from, to, t, MOTION_MS),
                graph: Some(self.model.clone()),
            })
        };
        self.pane.ease = ease;
        self.pane.view = v;
        self.pane.rehover = true;
        true
    }

    /// The group key of the node the cursor is on, where it has a group.
    fn cursor_group(&self) -> Option<crate::data::SeriesKey> {
        let i = self.model.node_of(self.pane.cursor.as_ref()?)?;
        self.model.group_of[i].map(|g| self.model.group_key(g as usize))
    }

    /// Only the group `k`, or every group again if it's shown alone.
    pub fn solo(&mut self, k: &crate::data::SeriesKey) {
        let mut v = self.pane.view.clone();
        if v.only.as_ref() == Some(k) {
            v.only = None;
        } else {
            v.only = Some(k.clone());
            v.hidden.remove(k);
        }
        self.set(v, Change::Step);
    }

    /// The group `k` hidden, or shown again; a group shown alone becomes the rest hidden first.
    pub fn toggle_hidden(&mut self, k: &crate::data::SeriesKey) {
        let mut v = self.pane.view.clone();
        if let Some(only) = v.only.take() {
            v.hidden = (self.model.keys().into_iter())
                .filter(|s| *s != only)
                .collect();
        }
        if !v.hidden.remove(k) {
            v.hidden.insert(k.clone());
        }
        self.set(v, Change::Step);
    }

    /// Every pin let go, eased back to the layout's places: `relayout`. False with none.
    pub fn unpin_all(&mut self) -> bool {
        let mut v = self.pane.view.clone();
        if v.pinned.is_empty() {
            return false;
        }
        v.pinned.clear();
        self.set(v, Change::Step)
    }

    /// The cursor on a node, the view keeping it in sight, the hover hidden until the pointer
    /// moves: a search's find, a jump from the panel.
    pub fn put_cursor(&mut self, c: Cursor) {
        let v = match self.pane.graph.as_ref() {
            Some(l) => nav::reveal(self.model, l, &self.pane.view, &c),
            None => self.pane.view.clone(),
        };
        self.pane.cursor = Some(c);
        (self.pane.anchor, self.pane.calm) = (None, None);
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
    }

    // ---------------------------------------------------------- the pointer

    /// A drag sample: a press past the slop becomes its gesture; a pan follows the hand, a
    /// dragged node keeps its grab under the pointer.
    pub fn drag(&mut self, at: (f32, f32)) {
        if let Gesture::Pressed {
            at: from,
            hit,
            button,
            mods,
            ..
        } = &self.pane.gesture
        {
            if (at.0 - from.0).hypot(at.1 - from.1) < CLICK_SLOP {
                return;
            }
            let (from, hit, button, mods) = (*from, hit.clone(), *button, *mods);
            self.start(from, hit, button, mods);
        }
        match &self.pane.gesture {
            Gesture::Pan { .. } => self.pan_by(at),
            Gesture::NodeDrag { node, grab, .. } => {
                let (node, grab) = (node.clone(), *grab);
                let Some(l) = self.pane.graph.as_ref() else {
                    return;
                };
                // Kept inside the graph's region (as the core keeps a drag in the window), so
                // it never goes under the panel or off the figure where it couldn't be taken
                // again.
                let r = (self.model.index_of(&node).and_then(|i| l.node(i))).map_or(0.0, |g| g.r);
                let b = l.region;
                let (lo_x, hi_x) = (b.x + r, (b.right() - r).max(b.x + r));
                let (lo_y, hi_y) = (b.y + r, (b.bottom() - r).max(b.y + r));
                let c = (
                    (at.0 - grab.0).clamp(lo_x, hi_x),
                    (at.1 - grab.1).clamp(lo_y, hi_y),
                );
                let at = (c.0 + grab.0, c.1 + grab.1);
                let v = nav::drag_node(self.model, l, &self.pane.view, &node, at, grab);
                self.set(v, Change::Live);
            }
            _ => {}
        }
    }

    /// A press that travelled the slop: a node moves, the background pans, as ⌥ and the middle
    /// button do anywhere; on the legend or the header, nothing.
    fn start(&mut self, from: (f32, f32), hit: Hit, button: Button, mods: Mods) {
        self.pane.settle(ms(self.now));
        let before = self.pane.view.clone();
        self.pane.drag_last = from;
        let on = || match self.pane.graph.as_ref() {
            Some(l) => graph::hover(self.model, l, from.0, from.1),
            None => None,
        };
        self.pane.gesture = match hit {
            _ if button == Button::Middle || mods.alt => Gesture::Pan {
                panel: 0,
                from,
                before,
            },
            Hit::Plot(_) | Hit::Node(_) => match on() {
                Some(Target::Node(i)) => {
                    let id: Box<str> = self.model.id(i as usize).into();
                    let grab = match self.pane.graph.as_ref() {
                        Some(l) => nav::grab(self.model, l, &id, from),
                        None => (0.0, 0.0),
                    };
                    Gesture::NodeDrag {
                        node: id,
                        grab,
                        before,
                    }
                }
                _ => Gesture::Pan {
                    panel: 0,
                    from,
                    before,
                },
            },
            _ => Gesture::Swallowed,
        };
        self.pane.rehover = true;
    }

    /// A pan by the hand's motion since the last sample, 1:1.
    fn pan_by(&mut self, at: (f32, f32)) {
        let (dx, dy) = (at.0 - self.pane.drag_last.0, at.1 - self.pane.drag_last.1);
        self.pane.drag_last = at;
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let Some(l) = self.pane.graph.as_ref() else {
            return;
        };
        let v = nav::pan(self.model, l, &self.pane.view, dx, dy);
        self.set(v, Change::Live);
    }

    /// A release: of the gesture's button, the gesture's end (a click, or a drag's undo step);
    /// of another, nothing.
    pub fn release(&mut self, b: Button, at: (f32, f32)) {
        if self.pane.held.is_some_and(|h| h != b) {
            return;
        }
        self.pane.held = None;
        match std::mem::replace(&mut self.pane.gesture, Gesture::None) {
            Gesture::Pressed {
                hit,
                mods,
                button: Button::Left,
                ..
            } => self.click(hit, mods, at),
            g @ (Gesture::Pan { .. } | Gesture::NodeDrag { .. }) => {
                self.pane.gesture = g;
                self.finish();
                self.pane.gesture = Gesture::None;
            }
            _ => {}
        }
        self.pane.rehover = true;
    }

    /// The gesture ends where it stands, as a key or the focus going ends it (mouse-ux §8): a
    /// pan, or a node moved and pinned, one undo step; a press does nothing more.
    pub fn finish(&mut self) {
        match std::mem::replace(&mut self.pane.gesture, Gesture::None) {
            Gesture::Pressed { .. } => self.pane.gesture = Gesture::Swallowed,
            Gesture::Pan { before, .. } | Gesture::NodeDrag { before, .. } => {
                if before != self.pane.view {
                    self.pane.undo.push(Snap {
                        view: before,
                        toggles: self.toggles,
                    });
                }
                self.pane.gesture = Gesture::Swallowed;
            }
            other => self.pane.gesture = other,
        }
        self.pane.rehover = true;
    }

    /// A click, on its release: a node or an edge's nearer end takes the cursor (⇧ the
    /// reference, ⌥ shows only its group); the background does nothing; a legend key toggles
    /// its group (⌥ solos it); the stale badge brings up the log.
    fn click(&mut self, hit: Hit, mods: Mods, at: (f32, f32)) {
        match hit {
            Hit::LegendKey(k) => {
                if mods.alt {
                    self.solo(&k);
                    self.pane.last_click = None;
                } else {
                    self.toggle_hidden(&k);
                    self.pane.last_click = Some(k);
                }
            }
            Hit::Plot(_) | Hit::Node(_) => {
                let Some(l) = self.pane.graph.as_ref() else {
                    return;
                };
                let t = graph::hover(self.model, l, at.0, at.1);
                let c = t.and_then(|t| t.cursor(self.model, l, at));
                if mods.shift {
                    // The reference: set, moved, or cleared on itself. Not an undo step.
                    if c.is_some() {
                        let r = &mut self.pane.view.reference;
                        *r = if *r == c { None } else { c };
                    }
                } else if mods.alt {
                    let i = c.as_ref().and_then(|c| self.model.node_of(c));
                    match i.map(|i| self.model.group_of[i]) {
                        Some(Some(g)) => {
                            let k = self.model.group_key(g as usize);
                            self.solo(&k);
                        }
                        // As x and o say it.
                        Some(None) => self.say = Some("fig: the node has no group".into()),
                        None => {}
                    }
                } else if c.is_some() {
                    self.pane.cursor = c;
                    (self.pane.anchor, self.pane.calm) = (None, None);
                }
                self.pane.last_click = None;
            }
            Hit::Badge => {
                (self.pane.log, self.pane.log_pressed) = (Some((0, 0)), None);
                (self.pane.hover, self.pane.target) = (None, None);
            }
            _ => {}
        }
    }

    /// A double click's second press, at once: on a node or an edge, the item whose detail
    /// opens (the first click put the cursor there); on the background, fit; on a legend key,
    /// only its group, the first click's toggle taken back.
    pub fn double(&mut self, hit: &Hit, at: (f32, f32)) -> Option<Item> {
        let last = self.pane.last_click.take();
        match hit {
            Hit::LegendKey(k) => {
                let mut v = self.pane.view.clone();
                if last.as_ref() == Some(k)
                    && let Some(before) = self.pane.undo.retract()
                {
                    v = before.view;
                    self.pane.view = v.clone();
                }
                v.only = Some(k.clone());
                v.hidden.remove(k);
                self.set(v, Change::Step);
                None
            }
            Hit::Plot(_) | Hit::Node(_) => {
                let l = self.pane.graph.as_ref()?;
                match graph::hover(self.model, l, at.0, at.1) {
                    Some(Target::Node(i)) => Some(self.model.item(i as usize)),
                    Some(Target::Edge(k)) => Some(self.model.edge_item(k as usize)),
                    None => {
                        self.fit();
                        None
                    }
                }
            }
            _ => None,
        }
    }

    /// The wheel: a burst keeps the meaning of its first event; plain pans in two dimensions
    /// by whole physical pixels with the rest carried, ⌘ (or Ctrl) zooms about the pointer.
    pub fn scroll(
        &mut self,
        dx: f32,
        dy: f32,
        at: (f32, f32),
        mods: Mods,
        arm: impl FnOnce() -> Sub,
    ) {
        if self.pane.gesturing() || !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let (dx, dy) = if mods.shift && dx == 0.0 {
            (dy, 0.0)
        } else {
            (dx, dy)
        };
        let timer = arm();
        if self.pane.burst.is_none() {
            self.pane.settle(ms(self.now));
            self.pane.burst = Some(Burst {
                zoom: mods.super_ || mods.ctrl,
                axis: None,
                across: None,
                panel: false,
                before: self.pane.view.clone(),
                _timer: timer,
            });
            self.pane.carry = (0.0, 0.0);
        } else if let Some(b) = &mut self.pane.burst {
            b._timer = timer;
        }
        let zoom = self.pane.burst.as_ref().is_some_and(|b| b.zoom);
        let Some(l) = self.pane.graph.as_ref() else {
            return;
        };
        let v = if zoom {
            let factor = ZOOM_STEP.powf(f64::from(-dy) / 4.0);
            if factor == 1.0 {
                return;
            }
            nav::zoom_about(self.model, l, &self.pane.view, factor, Some(at), None)
        } else {
            let (cw, ch) = (self.pane.cell.width as f32, self.pane.cell.height as f32);
            let s = self.pane.scale();
            let (tx, ty) = (dx * cw + self.pane.carry.0, dy * ch + self.pane.carry.1);
            let (wx, wy) = (tx.trunc(), ty.trunc());
            self.pane.carry = (tx - wx, ty - wy);
            if wx == 0.0 && wy == 0.0 {
                return;
            }
            // The wheel says how the view moves; a pan, how the content does.
            nav::pan(self.model, l, &self.pane.view, -wx / s, -wy / s)
        };
        self.set(v, Change::Live);
    }
}

/// The keys' actions on a graph (the design's controls, graph column).
impl Keys for GraphAct<'_> {
    /// `h j k l` to the nearest node that way, `]` `[` round the anchor's neighbours (the anchor
    /// kept while they repeat), `gg` `G` to the first and last node; the view reveals the
    /// cursor. With no cursor, `h j k l` put it on the node nearest the middle, the view as it
    /// is. False if it didn't move.
    fn move_cursor(&mut self, action: &Action, count: u32) -> bool {
        let Some(l) = self.pane.graph.as_ref() else {
            return false;
        };
        let cursor = self.pane.cursor.clone();
        let placed = cursor.is_none() && matches!(action, Action::Step(_) | Action::Series(_));
        let moved = match action {
            Action::Neighbour(by) => {
                let Some(cur) = cursor.as_ref().and_then(|c| self.model.node_of(c)) else {
                    return false;
                };
                let kept = match &self.pane.anchor {
                    Some((a, left)) if cursor.as_ref() == Some(&Cursor::Node(left.clone())) => {
                        self.model.index_of(a)
                    }
                    _ => None,
                };
                let (anchor, current) = match kept {
                    Some(a) => (a, Some(cur)),
                    None => (cur, None),
                };
                let steps = by.signum() * count.clamp(1, 10_000) as i32;
                nav::neighbour(self.model, l, anchor, current, steps).map(|j| {
                    self.pane.anchor =
                        Some((self.model.id(anchor).into(), self.model.id(j).into()));
                    self.model.cursor(j)
                })
            }
            _ => nav::motion(self.model, l, cursor.as_ref(), action, count).inspect(|_| {
                self.pane.anchor = None;
            }),
        };
        let Some(c) = moved else {
            return false;
        };
        let v = match placed {
            true => self.pane.view.clone(),
            false => nav::reveal(self.model, l, &self.pane.view, &c),
        };
        self.pane.cursor = Some(c);
        self.pane.calm = None;
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
        true
    }

    fn zoom_keys(&mut self, factor: f64) {
        let Some(l) = self.pane.graph.as_ref() else {
            return;
        };
        let cursor = self.pane.cursor.as_ref();
        let v = nav::zoom_about(self.model, l, &self.pane.view, factor, None, cursor);
        self.set(v, Change::Keyed);
    }

    /// `zh zl zH zL zj zk`: the view scrolled by fractions of the plot, right and up positive.
    fn scroll_keys(&mut self, fx: f32, fy: f32) {
        let Some(l) = self.pane.graph.as_ref() else {
            return;
        };
        let (dx, dy) = (-fx * l.plot.w, fy * l.plot.h);
        let v = nav::pan(self.model, l, &self.pane.view, dx, dy);
        self.set(v, Change::Keyed);
    }

    fn centre(&mut self) {
        let (Some(l), Some(c)) = (self.pane.graph.as_ref(), self.pane.cursor.as_ref()) else {
            return;
        };
        let v = nav::centre(self.model, l, &self.pane.view, c);
        self.set(v, Change::Keyed);
    }

    fn fit(&mut self) {
        let v = nav::fit(&self.pane.view);
        self.set(v, Change::Step);
    }

    /// `x`: the cursor's group hidden; the cursor goes with it. False where its node has no
    /// group.
    fn hide(&mut self) -> bool {
        let Some(k) = self.cursor_group() else {
            return false;
        };
        let mut v = self.pane.view.clone();
        if v.only.as_ref() == Some(&k) {
            v.only = None;
        }
        v.hidden.insert(k);
        self.pane.cursor = None;
        self.set(v, Change::Step);
        true
    }

    fn only(&mut self) -> bool {
        let Some(k) = self.cursor_group() else {
            return false;
        };
        self.solo(&k);
        true
    }

    fn show_all(&mut self) {
        let mut v = self.pane.view.clone();
        v.hidden.clear();
        v.only = None;
        self.set(v, Change::Step);
    }

    fn reference(&mut self) -> bool {
        let Some(c) = self.pane.cursor.clone() else {
            return false;
        };
        let r = &mut self.pane.view.reference;
        *r = if r.as_ref() == Some(&c) {
            None
        } else {
            Some(c)
        };
        true
    }

    fn to_reference(&mut self) -> bool {
        let Some(r) = self.pane.view.reference.clone() else {
            return false;
        };
        self.put_cursor(r);
        true
    }
}

impl Pane {
    /// What the pointer is on in a graph, worked out again from where it is: a node and then an
    /// edge over the graph, a group's legend key; nothing during a gesture, or after a key
    /// moved the cursor until the pointer moves.
    pub(super) fn graph_hover(&mut self, model: &GraphModel) {
        self.rehover = false;
        let (mut target, mut legend) = (None, None);
        if let (Some(p), true) = (self.pointer, self.hovering && !self.gesturing()) {
            match self.hit_at(p) {
                Hit::LegendKey(k) => legend = Some(k),
                Hit::Plot(_) | Hit::Node(_) => {
                    target = (self.graph.as_ref()).and_then(|l| graph::hover(model, l, p.0, p.1));
                }
                _ => {}
            }
        }
        self.hover = target.and_then(Target::node).map(|i| model.cursor(i));
        (self.target, self.legend, self.near, self.strip) = (target, legend, None, None);
    }

    /// What a graph's overlay shows: the hover, the cursor and its card (not while the panel
    /// says the same, nor mid-drag), the node in focus (a dragged node, a panel row's under the
    /// pointer, else a search's find, else the hover's after its dwell), a group's legend key under the pointer, a
    /// dragged node's ghost where it was, and the nodes a search typed so far lights.
    pub fn graph_overlay(&self, model: &GraphModel) -> graph::OverlayState {
        let ghost = match &self.gesture {
            Gesture::NodeDrag { node, before, .. } => model
                .index_of(node)
                .map(|i| (node.clone(), model.position(before, i))),
            _ => None,
        };
        // The node dragged is the one in focus, its edges bright as they follow it; else a
        // panel row's under the pointer.
        let row = match (&self.gesture, &self.row_hover) {
            (Gesture::NodeDrag { node, .. }, _) => Some(Cursor::Node(node.clone())),
            (_, Some(Item::Node { id, .. })) => Some(Cursor::Node(id.clone())),
            _ => None,
        };
        let calm = self.calm.is_some() && self.calm == self.cursor;
        graph::OverlayState {
            hover: self.target,
            cursor: self.cursor.clone(),
            pointer: self
                .pointer
                .filter(|_| self.target.is_some() || self.legend.is_some()),
            emphasis: row
                .or_else(|| self.found_node.clone())
                .or_else(|| self.dwell.clone()),
            group: self.legend.clone(),
            // Mid-drag the card goes: the dragged node's own label says enough (mouse-ux §3.3).
            cursor_card: self.panel.is_none()
                && !calm
                && !matches!(self.gesture, Gesture::NodeDrag { .. } | Gesture::Pan { .. }),
            ghost,
            matches: self.matches.clone(),
            calm,
        }
    }

    /// The hover's node comes into focus once it has dwelt there, and lets go after as long
    /// (mouse-ux §3.6): a crossing between two nodes doesn't flash. Timers only while it
    /// changes.
    pub(super) fn dwell_on(
        &mut self,
        model: &GraphModel,
        w: die_core::WindowId,
        me: &Weak<RefCell<FigState>>,
        ctx: &mut Ctx,
    ) {
        let want = self.target.and_then(Target::node).map(|i| model.cursor(i));
        if want == self.dwell {
            self.dwell_next = None;
            return;
        }
        if self.dwell_next.as_ref().is_some_and(|(c, _)| *c == want) {
            return;
        }
        let me = me.clone();
        let next = want.clone();
        let sub = ctx.after(Duration::from_millis(DWELL_EMPHASIS_MS), move |ctx| {
            if let Some(s) = me.upgrade() {
                let mut st = s.borrow_mut();
                if let Some(p) = st.panes.get_mut(&w) {
                    p.dwell = next;
                    p.dwell_next = None;
                }
                st.refresh(ctx);
            }
            Ok(())
        });
        self.dwell_next = Some((want, sub));
    }
}

impl FigState {
    /// A pointer event on a graph's window, past the panel and the error figure: as a chart's,
    /// with a graph's gestures.
    pub(super) fn graph_pointer(
        &mut self,
        ctx: &mut Ctx,
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
    ) {
        let Some(model) = self.graph.clone() else {
            return;
        };
        let (toggles, motion) = (self.toggles, self.motion());
        let me = self.me.clone();
        let kit = self.kit.clone();
        let mut kit = kit.borrow_mut();
        let now = ctx.now();
        let w = p.window;
        let Some(pane) = self.panes.get_mut(&w) else {
            return;
        };
        let mut g = GraphAct {
            pane,
            model: &model,
            toggles,
            motion,
            now,
            kit: &mut kit,
            say: None,
        };
        let mut open = None;
        match p.kind {
            PointerKind::Move => {
                g.pane.hovering = true;
                g.pane.rehover = true;
            }
            PointerKind::Press(b @ (Button::Left | Button::Middle)) => {
                g.pane.help = false;
                if g.pane.gesturing() && g.pane.held != Some(b) {
                    return;
                }
                if g.pane.gesturing() {
                    g.finish();
                }
                g.pane.held = Some(b);
                g.pane.drag_mods = p.mods;
                if swallow {
                    g.pane.gesture = Gesture::Swallowed;
                    return;
                }
                let hit = g.pane.hit_at(at);
                if p.clicks >= 2 && b == Button::Left {
                    open = g.double(&hit, at);
                    g.pane.gesture = Gesture::Swallowed;
                } else {
                    g.pane.gesture = Gesture::Pressed {
                        at,
                        hit,
                        button: b,
                        mods: p.mods,
                        clicks: p.clicks,
                    };
                    g.pane.drag_last = at;
                }
            }
            PointerKind::Press(Button::Right) => {}
            PointerKind::Drag(_) => g.drag(at),
            PointerKind::Release(b) => g.release(b, at),
            PointerKind::Scroll { dx, dy } => {
                g.scroll(dx, dy, at, p.mods, || {
                    ctx.after(Duration::from_millis(SCROLL_BURST_MS), move |ctx| {
                        if let Some(s) = me.upgrade() {
                            let mut st = s.borrow_mut();
                            st.end_burst(w);
                            st.refresh(ctx);
                        }
                        Ok(())
                    })
                });
            }
        }
        if let Some(said) = g.say.take() {
            ctx.messages.push(said);
        }
        drop(kit);
        if let Some(item) = open
            && let Err(why) = self.open_panel(w, Some(item))
        {
            ctx.messages.push(why);
        }
    }

    /// Act on a graph's pane, as the keys and the pointer do. None without a graph or a pane.
    pub fn graph_act<R>(
        &mut self,
        w: die_core::WindowId,
        now: Duration,
        f: impl FnOnce(&mut GraphAct) -> R,
    ) -> Option<R> {
        let model = self.graph.clone()?;
        let (toggles, motion) = (self.toggles, self.motion());
        let kit = self.kit.clone();
        let mut kit = kit.borrow_mut();
        let pane = self.panes.get_mut(&w)?;
        let mut g = GraphAct {
            pane,
            model: &model,
            toggles,
            motion,
            now,
            kit: &mut kit,
            say: None,
        };
        let r = f(&mut g);
        self.rev += 1;
        Some(r)
    }
}
