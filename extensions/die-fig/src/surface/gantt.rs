//! A gantt in a window, as [`super::GraphAct`] is a graph's and [`super::Act`] a chart's: hover a
//! task (its bar, or its label) and its card shows, off any bar the date under the pointer does;
//! a click puts the cursor on a task, a double-click opens its detail and on the background
//! fits the plan; a drag across the plot brushes a span of time and its release zooms to it,
//! ⌥- and middle-drags pan the time and the rows, a drag on the axis the time alone; the wheel
//! scrolls the rows where they overflow and it goes up or down, else pans the time, and ⌘-wheel
//! zooms the time about the pointer, a burst keeping its first meaning; the legend's keys hide
//! and show their names. Discrete changes ease through [`nav::tween`], the hand's never do.
//! Every rule of mouse-ux §3 the chart follows holds: the focusing press only focuses, the
//! press is held by its window and button, the hover is worked out again after the view moves,
//! and the pointer leaving clears it.

use super::{Burst, Change, Ease, FigState, Keys, Pane, Snap, ms};
use crate::data::SeriesKey;
use crate::detail::Item;
use crate::gantt::{self, GanttLayout, GanttModel, Target, nav};
use crate::interact::consts::{CLICK_SLOP, MOTION_MS, SCROLL_BURST_MS, ZOOM_STEP};
use crate::interact::{Action, Axis, BrushKind, Cursor, Gesture, Toggles, Tween, View};
use crate::scene::Hit;
use die_core::{Button, Ctx, Mods, PointerKind, Sub, SurfacePointer};
use std::sync::Arc;
use std::time::Duration;

/// A gantt's pane with what acting on it needs.
pub struct GanttAct<'a> {
    /// The pane.
    pub pane: &'a mut Pane,
    /// The gantt.
    pub model: &'a Arc<GanttModel>,
    /// The figure's settings, for undo.
    pub toggles: Toggles,
    /// Whether discrete changes ease.
    pub motion: bool,
    /// The clock.
    pub now: Duration,
}

impl GanttAct<'_> {
    /// Where the gantt landed, as last drawn.
    pub fn layout(&self) -> Option<&GanttLayout> {
        self.pane.gantt.as_ref()
    }

    /// Change the view, as `how` says: a discrete change eases (the time and the rows), the
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
        self.pane.ease = if how == Change::Live || !self.motion {
            None
        } else {
            // A fitted time and unscrolled rows tween as the plan's and the top: no need to
            // spell them out first.
            Some(Ease {
                tween: Tween::new(self.pane.shown(t), v.clone(), t, MOTION_MS),
                graph: None,
                gantt: Some(self.model.clone()),
            })
        };
        self.pane.view = v;
        self.pane.rehover = true;
        true
    }

    /// The name that colours the cursor's task, where it has one.
    fn cursor_name(&self) -> Option<SeriesKey> {
        let i = self.model.task_of(self.pane.cursor.as_ref()?)?;
        self.model.tasks[i].name?;
        Some(self.model.task_key(i))
    }

    /// Only the name `k`, or every name again if it's shown alone.
    pub fn solo(&mut self, k: &SeriesKey) {
        let mut v = self.pane.view.clone();
        if v.only.as_ref() == Some(k) {
            v.only = None;
        } else {
            v.only = Some(k.clone());
            v.hidden.remove(k);
        }
        self.drop_hidden_cursor(&v);
        self.set(v, Change::Step);
    }

    /// The name `k` hidden, or shown again; a name shown alone becomes the rest hidden first.
    pub fn toggle_hidden(&mut self, k: &SeriesKey) {
        let mut v = self.pane.view.clone();
        if let Some(only) = v.only.take() {
            v.hidden = (self.model.keys().into_iter())
                .filter(|s| *s != only)
                .collect();
        }
        if !v.hidden.remove(k) {
            v.hidden.insert(k.clone());
        }
        self.drop_hidden_cursor(&v);
        self.set(v, Change::Step);
    }

    /// The cursor let go where `v` leaves its task no row: a cursor is never on what isn't
    /// drawn.
    fn drop_hidden_cursor(&mut self, v: &View) {
        let gone = (self.pane.cursor.as_ref())
            .and_then(|c| self.model.task_of(c))
            .is_some_and(|i| !self.model.visible(v, i));
        if gone {
            (self.pane.cursor, self.pane.anchor) = (None, None);
        }
    }

    /// The cursor on a task, the view keeping it in sight, the hover hidden until the pointer
    /// moves: a search's find, a jump from the panel.
    pub fn put_cursor(&mut self, c: Cursor) {
        let v = match self.pane.gantt.as_ref() {
            Some(l) => nav::reveal(self.model, l, &self.pane.view, &c),
            None => self.pane.view.clone(),
        };
        self.pane.cursor = Some(c);
        self.pane.anchor = None;
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
    }

    // ---------------------------------------------------------- the pointer

    /// A point held inside the plot, across.
    fn in_plot(&self, at: (f32, f32)) -> (f32, f32) {
        match self.layout() {
            Some(l) => (at.0.clamp(l.plot.x, l.plot.right()), at.1),
            None => at,
        }
    }

    /// A drag sample: a press past the slop becomes its gesture; a brush reaches to the
    /// pointer, a pan follows the hand.
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
        let held = self.in_plot(at);
        match &mut self.pane.gesture {
            Gesture::Brush { to, .. } => {
                *to = held;
                self.pane.rehover = true;
            }
            Gesture::Pan { .. } => self.pan_by(at, true),
            Gesture::AxisPan { .. } => self.pan_by(at, false),
            _ => {}
        }
    }

    /// A press that travelled the slop: ⌥ and the middle button pan the time and the rows
    /// anywhere, a drag on the axis the time alone, a drag that starts on the plot (on a bar or
    /// off one) brushes a span of time; on the gutter, the legend or the header, nothing.
    fn start(&mut self, from: (f32, f32), hit: Hit, button: Button, mods: Mods) {
        self.pane.settle(ms(self.now));
        let before = self.pane.view.clone();
        self.pane.drag_last = from;
        let on_plot = (self.layout()).is_some_and(|l| l.plot.contains(from.0, from.1));
        self.pane.gesture = match hit {
            _ if button == Button::Middle || mods.alt => Gesture::Pan {
                panel: 0,
                from,
                before,
            },
            Hit::AxisX(panel) => Gesture::AxisPan {
                panel,
                axis: Axis::X,
                from,
                before,
            },
            Hit::Plot(_) | Hit::Task(_) if on_plot => Gesture::Brush {
                panel: 0,
                kind: BrushKind::X,
                from,
                to: from,
            },
            _ => Gesture::Swallowed,
        };
        self.pane.rehover = true;
    }

    /// A pan by the hand's motion since the last sample, 1:1: the time across, and the rows
    /// down where `rows` says.
    fn pan_by(&mut self, at: (f32, f32), rows: bool) {
        let (dx, dy) = (at.0 - self.pane.drag_last.0, at.1 - self.pane.drag_last.1);
        self.pane.drag_last = at;
        let dy = if rows { dy } else { 0.0 };
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let Some(l) = self.pane.gantt.as_ref() else {
            return;
        };
        let v = nav::pan(self.model, l, &self.pane.view, dx, dy);
        self.set(v, Change::Live);
    }

    /// A release: of the gesture's button, the gesture's end (a click, a brush's zoom, or a
    /// pan's undo step); of another, nothing.
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
            Gesture::Brush { from, to, .. } => {
                let v = (self.pane.gantt.as_ref())
                    .and_then(|l| nav::brush(self.model, l, &self.pane.view, from.0, to.0));
                if let Some(v) = v {
                    self.set(v, Change::Step);
                }
            }
            g @ (Gesture::Pan { .. } | Gesture::AxisPan { .. }) => {
                self.pane.gesture = g;
                self.finish();
                self.pane.gesture = Gesture::None;
            }
            _ => {}
        }
        self.pane.rehover = true;
    }

    /// The gesture ends where it stands, as a key or the focus going ends it (mouse-ux §8): a
    /// brush's preview goes, a pan stays as one undo step; a press does nothing more.
    pub fn finish(&mut self) {
        match std::mem::replace(&mut self.pane.gesture, Gesture::None) {
            Gesture::Brush { .. } | Gesture::Pressed { .. } => {
                self.pane.gesture = Gesture::Swallowed;
            }
            Gesture::Pan { before, .. } | Gesture::AxisPan { before, .. } => {
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

    /// The task at a point, as the hover finds it: its bar, or its label in the gutter.
    fn task_at(&self, at: (f32, f32)) -> Option<usize> {
        let l = self.pane.gantt.as_ref()?;
        gantt::hover(self.model, l, at.0, at.1).map(Target::task)
    }

    /// A click, on its release: a task takes the cursor (a heading and the background do
    /// nothing); a legend key hides or shows its name (⌥ shows it alone); the stale badge
    /// brings up the log.
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
            Hit::Plot(_) | Hit::Task(_) => {
                if let Some(i) = self.task_at(at) {
                    self.pane.cursor = Some(self.model.cursor(i));
                    self.pane.anchor = None;
                }
                self.pane.last_click = None;
            }
            Hit::Badge => {
                (self.pane.log, self.pane.log_pressed) = (Some((0, 0)), None);
                (self.pane.hover, self.pane.task) = (None, None);
            }
            _ => {}
        }
    }

    /// A double click's second press, at once: on a task, the item whose detail opens (the
    /// first click put the cursor there); on the plot's background or the axis, the plan fits
    /// again; on a legend key, only its name, the first click's toggle taken back.
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
                self.drop_hidden_cursor(&v);
                self.set(v, Change::Step);
                None
            }
            Hit::Plot(_) | Hit::Task(_) => match self.task_at(at) {
                Some(i) => Some(self.model.item(i)),
                None => {
                    if matches!(hit, Hit::Plot(_)) {
                        self.fit();
                    }
                    None
                }
            },
            Hit::AxisX(_) => {
                self.fit();
                None
            }
            _ => None,
        }
    }

    /// A pointer event on the gantt at `at` (logical px in its area), as its window has it, or
    /// a dashboard's panel: as a chart's ([`super::Act::pointer`]), with a gantt's gestures.
    /// The item a double click opens the detail of, if one.
    pub fn pointer(
        &mut self,
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
        arm: impl FnOnce() -> Sub,
    ) -> Option<Item> {
        match p.kind {
            PointerKind::Move => {
                self.pane.hovering = true;
                self.pane.rehover = true;
            }
            PointerKind::Press(b @ (Button::Left | Button::Middle)) => {
                self.pane.help = false;
                if self.pane.gesturing() && self.pane.held != Some(b) {
                    // A second button mid-drag: it ends nothing and starts nothing.
                    return None;
                }
                if self.pane.gesturing() {
                    // The same button again: its release was lost. What it did stays.
                    self.finish();
                }
                self.pane.held = Some(b);
                self.pane.drag_mods = p.mods;
                if swallow {
                    self.pane.gesture = Gesture::Swallowed;
                    return None;
                }
                let hit = self.pane.hit_at(at);
                if p.clicks >= 2 && b == Button::Left {
                    let open = self.double(&hit, at);
                    self.pane.gesture = Gesture::Swallowed;
                    return open;
                }
                self.pane.gesture = Gesture::Pressed {
                    at,
                    hit,
                    button: b,
                    mods: p.mods,
                    clicks: p.clicks,
                };
                self.pane.drag_last = at;
            }
            PointerKind::Press(Button::Right) => {}
            PointerKind::Drag(_) => self.drag(at),
            PointerKind::Release(b) => self.release(b, at),
            PointerKind::Scroll { dx, dy } => self.scroll(dx, dy, at, p.mods, arm),
        }
        None
    }

    /// The wheel: a burst keeps the meaning of its first event. ⌘ (or Ctrl) zooms the time
    /// about the pointer; plain, a first event that goes up or down more than across scrolls
    /// the rows where they overflow, and any other pans the time (by the wheel's up and down
    /// where the rows all fit, so a mouse's wheel moves it); by whole physical pixels with the
    /// rest carried.
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
        // Linux gives a mouse's Shift-wheel as up and down; macOS turns it across itself.
        let (dx, dy) = if mods.shift && dx == 0.0 {
            (dy, 0.0)
        } else {
            (dx, dy)
        };
        let timer = arm();
        if self.pane.burst.is_none() {
            self.pane.settle(ms(self.now));
            let overflow = self.layout().is_some_and(|l| l.max_scroll() > 0.0);
            let up_down = dy.abs() > dx.abs();
            self.pane.burst = Some(Burst {
                zoom: mods.super_ || mods.ctrl,
                // What it moves: the rows (y) or the time (x).
                axis: Some(if up_down && overflow {
                    Axis::Y
                } else {
                    Axis::X
                }),
                // Which of the wheel's components moves it.
                across: Some(!up_down),
                panel: false,
                before: self.pane.view.clone(),
                _timer: timer,
            });
            self.pane.carry = (0.0, 0.0);
        } else if let Some(b) = &mut self.pane.burst {
            b._timer = timer;
        }
        let Some(b) = &self.pane.burst else { return };
        let (zoom, rows, across) = (b.zoom, b.axis == Some(Axis::Y), b.across == Some(true));
        let Some(l) = self.pane.gantt.as_ref() else {
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
            // The wheel says how the view moves, right and down positive; a pan, how the
            // content does.
            let (px, py) = match (rows, across) {
                (true, _) => (0.0, -wy / s),
                (false, true) => (-wx / s, 0.0),
                (false, false) => (-wy / s, 0.0),
            };
            if px == 0.0 && py == 0.0 {
                return;
            }
            nav::wheel(self.model, l, &self.pane.view, px, py)
        };
        self.set(v, Change::Live);
    }
}

/// The keys' actions on a gantt (the gantt's contract §3).
impl Keys for GanttAct<'_> {
    /// `j k` down and up the rows, `h l` to the task that starts before or after, `]` to a task
    /// that comes after the cursor's and `[` to one it comes after (again goes round the same
    /// task's, the anchor kept while the key repeats and has another to go to), `w b` to the next and previous milestone, `gg` `G` to the first and last
    /// task, `H M L` to the top, middle and bottom task in view; the view reveals the cursor:
    /// its row scrolled into sight, and its bar brought into the time shown when it's wholly
    /// outside. With no cursor, `h j k l` put it on the first task in view. False if it didn't
    /// move.
    fn move_cursor(&mut self, action: &Action, count: u32) -> bool {
        let Some(l) = self.pane.gantt.as_ref() else {
            return false;
        };
        let cursor = self.pane.cursor.clone();
        let moved = match action {
            Action::Neighbour(by) => {
                let Some(cur) = cursor.as_ref().and_then(|c| self.model.task_of(c)) else {
                    return false;
                };
                let kept = match &self.pane.anchor {
                    Some((a, left)) if cursor.as_ref() == Some(&Cursor::Task(left.clone())) => {
                        self.model.index_of(a)
                    }
                    _ => None,
                };
                let steps = by.signum() * count.clamp(1, 10_000) as i32;
                // The anchor's turn goes on while the cursor is on one of the tasks this key
                // turns through and there's another to go to; otherwise a turn starts about
                // the cursor's own task, so `]` again walks on down a chain.
                let turning = kept.filter(|&a| {
                    let t = &self.model.tasks[a];
                    let list = if steps >= 0 { &t.then } else { &t.after };
                    list.contains(&(cur as u32))
                });
                let on = turning.and_then(|a| {
                    nav::neighbour(self.model, l, a, Some(cur), steps)
                        .filter(|&j| j != cur)
                        .map(|j| (a, j))
                });
                let found = on
                    .or_else(|| nav::neighbour(self.model, l, cur, None, steps).map(|j| (cur, j)));
                found.map(|(a, j)| {
                    self.pane.anchor = Some((self.model.key(a).into(), self.model.key(j).into()));
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
        let v = nav::reveal(self.model, l, &self.pane.view, &c);
        self.pane.cursor = Some(c);
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
        true
    }

    fn zoom_keys(&mut self, factor: f64) {
        let Some(l) = self.pane.gantt.as_ref() else {
            return;
        };
        let cursor = self.pane.cursor.as_ref();
        let v = nav::zoom_about(self.model, l, &self.pane.view, factor, None, cursor);
        self.set(v, Change::Keyed);
    }

    /// `zh zl zH zL`: the time scrolled by fractions of the plot, right positive (later);
    /// `zj zk`: the rows, up positive.
    fn scroll_keys(&mut self, fx: f32, fy: f32) {
        let Some(l) = self.pane.gantt.as_ref() else {
            return;
        };
        let (dx, dy) = (-fx * l.plot.w, fy * l.plot.h);
        let v = nav::pan(self.model, l, &self.pane.view, dx, dy);
        self.set(v, Change::Keyed);
    }

    fn centre(&mut self) {
        let (Some(l), Some(c)) = (self.pane.gantt.as_ref(), self.pane.cursor.as_ref()) else {
            return;
        };
        let v = nav::centre(self.model, l, &self.pane.view, c);
        self.set(v, Change::Keyed);
    }

    /// `0`: the whole plan's time again; the rows stay where they are.
    fn fit(&mut self) {
        let v = nav::fit(&self.pane.view);
        self.set(v, Change::Step);
    }

    /// `x`: the name that colours the cursor's task hidden, its tasks' rows with it; the
    /// cursor goes with them. False where its task has no name (or there's no cursor).
    fn hide(&mut self) -> bool {
        let Some(k) = self.cursor_name() else {
            return false;
        };
        let mut v = self.pane.view.clone();
        if v.only.as_ref() == Some(&k) {
            v.only = None;
        }
        v.hidden.insert(k);
        (self.pane.cursor, self.pane.anchor) = (None, None);
        self.set(v, Change::Step);
        true
    }

    fn only(&mut self) -> bool {
        let Some(k) = self.cursor_name() else {
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

    /// A gantt has no reference (the contract's §4).
    fn reference(&mut self) -> bool {
        false
    }

    fn to_reference(&mut self) -> bool {
        false
    }

    fn find(&mut self, c: Cursor) {
        self.pane.found_node = Some(c.clone());
        self.put_cursor(c);
    }

    fn back(&mut self, cursor: Option<Cursor>, view: View) {
        self.pane.cursor = cursor;
        self.pane.found_node = None;
        self.set(view, Change::Follow);
    }

    fn put(&mut self, c: Cursor) {
        self.put_cursor(c);
    }
}

impl Pane {
    /// What the pointer is on in a gantt, worked out again from where it is: a task (by its
    /// bar or its label), a name's legend key; nothing during a gesture, or after a key moved
    /// the cursor until the pointer moves.
    pub(super) fn gantt_hover(&mut self, model: &GanttModel) {
        self.rehover = false;
        let (mut target, mut legend) = (None, None);
        if let (Some(p), true) = (self.pointer, self.hovering && !self.gesturing()) {
            match self.hit_at(p) {
                Hit::LegendKey(k) => legend = Some(k),
                Hit::Plot(_) | Hit::Task(_) => {
                    target = (self.gantt.as_ref()).and_then(|l| gantt::hover(model, l, p.0, p.1));
                }
                _ => {}
            }
        }
        self.hover = target.map(|t| t.cursor(model));
        (self.task, self.legend, self.near, self.strip) = (target, legend, None, None);
    }

    /// What a gantt's overlay shows: the hover and its card, the cursor and its card (not
    /// while the panel says the same, nor mid-drag), the pointer while it hovers (off any bar,
    /// the date under it is said), the task a panel row's under the pointer names, else a
    /// search's find, a name's legend key under the pointer, a brush being drawn, and the
    /// tasks a search typed so far lights.
    pub fn gantt_overlay(&self, model: &GanttModel) -> gantt::OverlayState {
        let _ = model;
        let row = match &self.row_hover {
            Some(Item::Task { id, .. }) => Some(Cursor::Task(id.clone())),
            _ => None,
        };
        let brush = match &self.gesture {
            Gesture::Brush { from, to, .. } => Some((from.0, to.0)),
            _ => None,
        };
        gantt::OverlayState {
            hover: self.task,
            cursor: self.cursor.clone(),
            pointer: self.pointer.filter(|_| self.hovering && !self.gesturing()),
            emphasis: row.or_else(|| self.found_node.clone()),
            name: self.legend.clone(),
            brush,
            cursor_card: self.panel.is_none()
                && !matches!(self.gesture, Gesture::Pan { .. } | Gesture::AxisPan { .. }),
            matches: self.matches.clone(),
        }
    }
}

impl FigState {
    /// A pointer event on a gantt's window, past the panel and the error figure: as a chart's,
    /// with a gantt's gestures.
    pub(super) fn gantt_pointer(
        &mut self,
        ctx: &mut Ctx,
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
    ) {
        let Some(model) = self.gantt.clone() else {
            return;
        };
        let (toggles, motion) = (self.toggles, self.motion());
        let me = self.me.clone();
        let now = ctx.now();
        let w = p.window;
        let Some(pane) = self.panes.get_mut(&w) else {
            return;
        };
        let mut g = GanttAct {
            pane,
            model: &model,
            toggles,
            motion,
            now,
        };
        let open = g.pointer(p, at, swallow, || {
            ctx.after(Duration::from_millis(SCROLL_BURST_MS), move |ctx| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    st.end_burst(w);
                    st.refresh(ctx);
                }
                Ok(())
            })
        });
        if let Some(item) = open
            && let Err(why) = self.open_panel(w, Some(item))
        {
            ctx.messages.push(why);
        }
    }

    /// Act on a gantt's pane, as the keys and the pointer do. None without a gantt or a pane.
    pub fn gantt_act<R>(
        &mut self,
        w: die_core::WindowId,
        now: Duration,
        f: impl FnOnce(&mut GanttAct) -> R,
    ) -> Option<R> {
        let model = self.gantt.clone()?;
        let (toggles, motion) = (self.toggles, self.motion());
        let pane = self.panes.get_mut(&w)?;
        let mut g = GanttAct {
            pane,
            model: &model,
            toggles,
            motion,
            now,
        };
        let r = f(&mut g);
        self.rev += 1;
        Some(r)
    }
}
