//! `FigSurface`: a figure as a DIE surface, and the state its buffer carries ([`FigState`]),
//! shared with the extension ([`Shared`]) so its commands, timers and jobs reach it without
//! checking the surface out.
//!
//! Each window showing a figure is a [`Pane`]: a base buffer at z −2 (opaque, the chart) and an
//! overlay at z −1 (transparent: the crosshair, the card, the brush), sized to the window's area
//! and grown, never shrunk (ruling 11), with its own view, cursor, hover, gesture and undo. The
//! base is drawn again when the data, the view, the size, the scale or the theme change; the
//! overlay when the hover, the cursor, the brush or the emphasis do, and presents only the union
//! of what it drew before and draws now, so a hover costs what the card covers.
//!
//! The pointer, as a dashboard's (briefs/mouse-ux.md, contract-p2 §0): hover needs no click and
//! no focus and has no delay; the crosshair follows every sample ([`Surface::fine_motion`]) and
//! snaps to the nearest sample, the card lists every series there and the nearest one comes
//! forward; a legend key under the pointer brings its series forward; the pointer leaving
//! clears it all. A click puts the cursor there, ⇧-click sets the reference, a drag brushes, ⌥-
//! or middle-drag pans, a drag on an axis pans that axis, the wheel pans and ⌘-wheel zooms about
//! the pointer, a double-click resets, and the legend's keys toggle and solo. The press that
//! focuses the window only focuses it ([`FOCUS_GRACE_MS`]). The right button, Space and
//! Ctrl-click open nothing: there is no dial (the owner, 2026-11-22).
//!
//! A figure is a chart, a graph or a gantt. A graph is shown as a chart is, with a graph's
//! gestures and keys ([`graph`]), and so is a gantt with its own ([`gantt`]: a brush that zooms
//! the time, rows that scroll, the wheel's two meanings); the detail panel opens beside any of
//! them, the figure laid out in what it leaves ([`panel`]). A dashboard (or
//! a tile alone) is a figure of figures, each panel driven as it would be alone ([`dash`]); the
//! commands a figure's data comes from are asked for, run and streamed while it shows
//! ([`feeds`]).

pub mod dash;
mod feeds;
pub mod gantt;
pub mod graph;
pub mod panel;

pub use gantt::GanttAct;
pub use graph::GraphAct;
pub use panel::{Kill, Panel};

use crate::algo::HitIndex;
use crate::data::{ColumnKind, SeriesKey, Table};
use crate::detail::Item as Mark;
use crate::error::{ErrorKind, FigError};
use crate::gantt::{GanttLayout, GanttModel};
use crate::graph::{GraphLayout, GraphModel, Target};
use crate::interact::consts::{
    CLICK_SLOP, FOCUS_GRACE_MS, MOTION_MS, NEAREST_SERIES, SCROLL_BURST_MS, ZOOM_STEP,
};
use crate::interact::{Action, Axis, BrushKind, Cursor, Gesture, Steps, Toggles, Tween, View};
use crate::plot::{self, ChartLayout, ChartModel, ErrorView, OverlayState};
use crate::raster::{self, BookMeasure, PixelBox};
use crate::scene::{
    Anchor, FixedMeasure, Hit, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec,
    Weight,
};
use crate::search::Search;
use crate::source::{self, Failed, Job, Snapshot, Source, Text};
use crate::spec::{Chart, Figure, GanttSpec, GraphSpec, Layout};
use crate::theme::{self, Theme};
use die_core::{
    BufferId, Button, CellPixels, Ctx, Event, Frame, KeyEvent, KeyKind, Mods, PixelProducer,
    PixelRect, PointerKind, RenderInfo, Sub, Surface, SurfacePointer, WindowId,
};
use die_draw::Book;
use jiff::tz::TimeZone;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// The surface kind, which names the keymap.
pub const KIND: &str = "fig";

/// How often an ease draws a frame.
const FRAME: Duration = Duration::from_millis(16);
/// How long the source buffer is quiet before an edit is charted.
const QUIET: Duration = Duration::from_millis(150);
/// How often a file a figure reads from disk is looked at while a window shows the figure.
const POLL: Duration = Duration::from_secs(1);
/// How many of the last lines a failed command wrote to its standard error the log shows.
const STDERR_TAIL: usize = 6;

/// The time on the workspace's clock, in ms.
fn ms(d: Duration) -> u64 {
    d.as_millis().min(u128::from(u64::MAX)) as u64
}

// ------------------------------------------------------------------ the fonts every figure shares

/// What every figure draws its text with: one [`Book`], loaded on the main thread the first time
/// a figure needs pixels (ruling 12), shared by every figure of the extension.
#[derive(Default)]
pub struct Kit {
    /// Your `fonts` option.
    pub fonts: Option<PathBuf>,
    book: Option<Book>,
    tried: bool,
}

/// The kit, shared.
pub type SharedKit = Rc<RefCell<Kit>>;

impl Kit {
    /// The kit for your fonts option.
    pub fn new(fonts: Option<PathBuf>) -> SharedKit {
        Rc::new(RefCell::new(Kit {
            fonts,
            book: None,
            tried: false,
        }))
    }

    /// Whether there are fonts: loaded at the first ask, and a failure said once, after which
    /// figures are drawn without their text.
    pub fn ensure(&mut self, ctx: &mut Ctx) -> bool {
        if !self.tried {
            self.tried = true;
            match Book::load(self.fonts.as_deref()) {
                Ok(book) => self.book = Some(book),
                Err(why) => ctx.messages.push(format!(
                    "fig: no fonts, so figures are drawn without text: {why}"
                )),
            }
        }
        self.book.is_some()
    }
}

/// Run `f` with a measure at `scale`: the fonts' if they're loaded, else the fixed one.
fn measured<R>(book: Option<&mut Book>, scale: f32, f: impl FnOnce(&mut dyn Measure) -> R) -> R {
    match book {
        Some(book) => f(&mut BookMeasure { book, scale }),
        None => f(&mut FixedMeasure),
    }
}

// ------------------------------------------------------------------ the view's undo

/// What an undo step puts back: a window's view and the figure's settings.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snap {
    /// The view.
    pub view: View,
    /// The settings `s` turned.
    pub toggles: Toggles,
}

/// How a view change is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A discrete change: eased, one undo step.
    Step,
    /// A key's zoom or scroll: eased, one undo step with those before it within
    /// [`SCROLL_BURST_MS`].
    Keyed,
    /// Following the hand (a pan, a wheel's zoom, a drag): never eased; the gesture or the
    /// burst makes the undo step.
    Live,
    /// The view following the cursor or live data: eased, no undo step.
    Follow,
}

/// A scroll burst: scroll events less than [`SCROLL_BURST_MS`] apart, which keep the meaning
/// their first gave them and are one undo step (mouse-ux §3.2).
struct Burst {
    /// Zooming, or panning.
    zoom: bool,
    /// One axis only: the burst began over its strip.
    axis: Option<Axis>,
    /// On an x-chart, which of the wheel's components pans x: across (true) or up (false).
    across: Option<bool>,
    /// It began over the detail panel, which it scrolls.
    panel: bool,
    /// The view before it, for its undo step.
    before: View,
    /// Ends it when the wheel is quiet.
    _timer: Sub,
}

/// An ease from one view to another, with both domains made explicit so they can be tweened;
/// a graph's pins glide with it, and a gantt's rows.
struct Ease {
    tween: Tween<View>,
    /// The graph whose pinned nodes glide, when it's a graph's.
    graph: Option<Arc<GraphModel>>,
    /// The gantt whose time and rows glide, when it's a gantt's.
    gantt: Option<Arc<GanttModel>>,
}

impl Ease {
    /// The view part way, at `now`.
    fn at(&self, now: u64) -> View {
        let t = self.tween.progress(now);
        match (&self.graph, &self.gantt) {
            (Some(g), _) => crate::graph::nav::tween(g, &self.tween.from, &self.tween.to, t),
            (None, Some(g)) => crate::gantt::nav::tween(g, &self.tween.from, &self.tween.to, t),
            (None, None) => plot::nav::tween(&self.tween.from, &self.tween.to, t),
        }
    }
}

// ------------------------------------------------------------------ a window's pane

/// What the base shows: redrawn when this changes.
#[derive(Clone, Debug, PartialEq)]
struct BaseKey {
    model: u64,
    view: View,
    size: (u32, u32),
    scale: u32,
    theme: u64,
    /// The failure shown (its generation) and the log's place, if it's up.
    fail: u64,
    log: Option<(usize, usize)>,
    /// The figure laid out beside the detail panel.
    panel: bool,
}

/// What the overlay shows: redrawn when this changes.
#[derive(Clone, Debug, PartialEq)]
struct OverKey {
    base: u64,
    hover: Option<Cursor>,
    cursor: Option<Cursor>,
    pointer: Option<(f32, f32)>,
    #[allow(clippy::type_complexity)]
    brush: Option<(BrushKind, (f32, f32), (f32, f32))>,
    emphasis: Option<SeriesKey>,
    reference: Option<Cursor>,
    help: bool,
    strip: Option<Rect>,
    /// A graph's overlay, whole.
    graph: Option<crate::graph::OverlayState>,
    /// A gantt's overlay, whole.
    gantt: Option<crate::gantt::OverlayState>,
    /// What a chart's search lights, and whether its cursor's card shows.
    lit: plot::overlay::Lit,
}

/// One window's figure: its pixels, its layout, its view and what the pointer is doing there.
pub struct Pane {
    /// The area, in cells.
    pub cols: usize,
    /// Its rows.
    pub rows: usize,
    /// A cell in physical pixels, and the scale.
    pub cell: CellPixels,
    base: Option<PixelProducer>,
    over: Option<PixelProducer>,
    /// The base's scene as last drawn, its regions only: what the pointer asks first.
    scene: Option<Scene>,
    /// Where the chart landed, as last drawn.
    pub layout: Option<ChartLayout>,
    hits: Option<HitIndex>,
    drawn: Option<BaseKey>,
    /// Goes up each time the base is drawn.
    pub base_gen: u64,
    over_key: Option<OverKey>,
    over_bounds: Option<Rect>,
    /// What the overlay drew last: the next draws only where it differs.
    over_items: Vec<Item>,
    /// The view, as the last change left it (an ease shows its way there).
    pub view: View,
    ease: Option<Ease>,
    /// The undo history of the view and the settings.
    pub undo: Steps<Snap>,
    /// The keyboard's cursor on the data.
    pub cursor: Option<Cursor>,
    /// Where the pointer is over the area, in logical px; None once it has left.
    pub pointer: Option<(f32, f32)>,
    /// Whether the pointer was the last to move: a key moving the cursor hides the hover until
    /// the next move.
    hovering: bool,
    /// What the pointer is on, as a cursor.
    pub hover: Option<Cursor>,
    /// The series nearest the pointer, brought forward.
    pub near: Option<SeriesKey>,
    /// The legend key under the pointer.
    pub legend: Option<SeriesKey>,
    /// What the pointer is doing.
    pub gesture: Gesture,
    /// Where a pan's last sample was.
    drag_last: (f32, f32),
    /// The button whose press the gesture is: only its release ends it.
    held: Option<Button>,
    burst: Option<Burst>,
    /// What a scroll pan hasn't moved yet, in physical px: less than one.
    carry: (f32, f32),
    /// When a key last zoomed or scrolled: the next within the burst is the same undo step.
    keyed: Option<Duration>,
    /// The last legend click: a double click's second press takes its toggle back.
    last_click: Option<SeriesKey>,
    /// The key card, shown until any key.
    pub help: bool,
    /// The series a search found, brought forward until `Esc`.
    pub found: Option<SeriesKey>,
    /// The category a search found on a band chart, brought forward until `Esc`.
    pub found_band: Option<u32>,
    /// The series a search typed so far matches on a chart, lit while it's typed.
    pub lit: Vec<SeriesKey>,
    /// The categories it matches on a band chart, lit with them.
    pub lit_bands: Vec<u32>,
    /// The cursor whose emphasis `Esc` let go (a graph's): its ring stays, nothing dims, until
    /// the cursor moves, a click or a search puts it somewhere again.
    pub calm: Option<Cursor>,
    /// The hover is to be worked out again: the view moved under a still pointer.
    rehover: bool,
    /// The axis strip under the pointer, lit faintly as the affordance (mouse-ux §4.9).
    strip: Option<Rect>,
    /// The modifiers of the drag's last sample: Shift constrains a box brush.
    drag_mods: Mods,
    /// The view a brush holds still while it's drawn, so live data doesn't slide under the
    /// hand (mouse-ux §3.7): what the base shows until its release.
    frozen: Option<View>,
    /// The area changed since the overlay last drew: its next draw clears the whole buffer,
    /// since what it drew at the old size isn't where the new one draws.
    over_wipe: bool,
    /// The error figure's log, if it's up in this window: its first line shown and the entry
    /// marked. It shows over the figure, a kept one too.
    pub log: Option<(usize, usize)>,
    /// What a wheel's scroll of the log hasn't moved yet, in lines: less than one.
    log_carry: f32,
    /// The entry the last press on the log was on: a double click goes to an entry only where
    /// both its presses were on it, not one whose first click turned the figure over.
    log_pressed: Option<u32>,
    /// Where a graph landed, as last drawn.
    pub graph: Option<GraphLayout>,
    /// The node or edge of a graph under the pointer.
    pub target: Option<Target>,
    /// The node the hover brought into focus once it dwelt there.
    pub dwell: Option<Cursor>,
    /// What the dwell's timer will bring into focus, and the timer.
    dwell_next: Option<(Option<Cursor>, Sub)>,
    /// `]` and `[`'s anchor: the node (a gantt's task) they turn about, and the one they left
    /// the cursor on.
    anchor: Option<(Box<str>, Box<str>)>,
    /// The node a search found on a graph (a gantt's task), in focus until `Esc`.
    pub found_node: Option<Cursor>,
    /// The nodes (a gantt's tasks) the search typed so far matches, lit while it's typed.
    pub matches: Vec<usize>,
    /// Where a gantt landed, as last drawn.
    pub gantt: Option<GanttLayout>,
    /// The task of a gantt under the pointer.
    pub task: Option<crate::gantt::Target>,
    /// The detail panel, if it's open.
    pub panel: Option<Panel>,
    /// The panel as last drawn.
    panel_drawn: Option<panel::PanelKey2>,
    /// How many of the base scene's regions are the figure's; the panel's come after.
    fig_regions: usize,
    /// The mark named by the panel's row under the pointer: it comes forward in the figure.
    pub row_hover: Option<Mark>,
    /// The press under way began on the panel: its drags and release are the panel's.
    panel_press: bool,
    /// The dashboard in this window, when the figure is one.
    pub dash: Option<Box<dash::DashPane>>,
}

impl Pane {
    fn new(view: View, cursor: Option<Cursor>, undo: Steps<Snap>) -> Pane {
        Pane {
            cols: 0,
            rows: 0,
            cell: CellPixels::default(),
            base: None,
            over: None,
            scene: None,
            layout: None,
            hits: None,
            drawn: None,
            base_gen: 0,
            over_key: None,
            over_bounds: None,
            over_items: vec![],
            view,
            ease: None,
            undo,
            cursor,
            pointer: None,
            hovering: false,
            hover: None,
            near: None,
            legend: None,
            gesture: Gesture::None,
            drag_last: (0.0, 0.0),
            held: None,
            burst: None,
            carry: (0.0, 0.0),
            keyed: None,
            last_click: None,
            help: false,
            found: None,
            found_band: None,
            lit: vec![],
            lit_bands: vec![],
            calm: None,
            rehover: false,
            strip: None,
            drag_mods: Mods::default(),
            frozen: None,
            over_wipe: false,
            log: None,
            log_carry: 0.0,
            log_pressed: None,
            graph: None,
            target: None,
            dwell: None,
            dwell_next: None,
            anchor: None,
            found_node: None,
            matches: vec![],
            gantt: None,
            task: None,
            panel: None,
            panel_drawn: None,
            fig_regions: 0,
            row_hover: None,
            panel_press: false,
            dash: None,
        }
    }

    /// Physical pixels to a logical one: 1 while unknown.
    pub fn scale(&self) -> f32 {
        let s = self.cell.scale as f32;
        if s.is_finite() && s > 0.0 { s } else { 1.0 }
    }

    /// The area in physical pixels.
    pub fn area_px(&self) -> (u32, u32) {
        (
            self.cols as u32 * self.cell.width,
            self.rows as u32 * self.cell.height,
        )
    }

    /// The area in logical px.
    pub fn size(&self) -> (f32, f32) {
        let (w, h) = self.area_px();
        let s = self.scale();
        (w as f32 / s, h as f32 / s)
    }

    /// The base and overlay buffers' ids, once made.
    pub fn buffers(&self) -> Option<(die_core::PixelBufferId, die_core::PixelBufferId)> {
        Some((self.base.as_ref()?.id(), self.over.as_ref()?.id()))
    }

    /// The view as shown now: on its way there while it eases, held still while a brush is
    /// drawn.
    pub fn shown(&self, now: u64) -> View {
        match (&self.ease, &self.frozen) {
            (Some(e), _) => e.at(now),
            (None, Some(f)) => f.clone(),
            (None, None) => self.view.clone(),
        }
    }

    /// Whether it's easing.
    pub fn easing(&self) -> bool {
        self.ease.is_some()
    }

    /// Where the overlay last drew, in logical px: what its next present clears.
    pub fn overlay_bounds(&self) -> Option<Rect> {
        self.over_bounds
    }

    /// What the overlay is showing, as plot draws it.
    pub fn overlay(&self) -> OverlayState {
        self.overlay_state()
    }

    /// What's under a point of the area, in logical px, as the pointer finds it.
    pub fn hit(&self, at: (f32, f32)) -> Hit {
        self.hit_at(at)
    }

    /// Whether the card was last put left of its anchor (its hysteresis), once drawn.
    pub fn card_flipped(&self) -> Option<bool> {
        self.layout.as_ref().map(|l| l.flipped.get())
    }

    /// The axis strip the pointer is over, lit as the affordance.
    pub fn lit_strip(&self) -> Option<Rect> {
        self.strip
    }

    /// The ease stops where it is now: what a hand takes over from (mouse-ux §10).
    fn settle(&mut self, now: u64) {
        if let Some(e) = self.ease.take() {
            let reached = e.tween.done(now);
            let at = e.at(now);
            if !reached {
                self.view = View {
                    x: at.x,
                    y: at.y,
                    ..self.view.clone()
                };
            }
        }
    }

    /// What's under a point: the base's regions, last pushed first.
    fn hit_at(&self, (x, y): (f32, f32)) -> Hit {
        let hit = self.scene.as_ref().and_then(|s| s.hit(x, y));
        hit.cloned().unwrap_or(Hit::Chrome)
    }

    /// Whether a press is under way that its drags and release act on.
    fn gesturing(&self) -> bool {
        !matches!(self.gesture, Gesture::None | Gesture::Swallowed)
    }

    /// Change the view, as `how` says: `model` makes `None` domains explicit for an ease.
    fn set_view(
        &mut self,
        v: View,
        how: Change,
        toggles: Toggles,
        model: &ChartModel,
        motion: bool,
        now: Duration,
    ) -> bool {
        if v == self.view {
            return false;
        }
        let before = Snap {
            view: self.view.clone(),
            toggles,
        };
        match how {
            Change::Step => {
                self.undo.push(before);
                // A key's zoom after this is a step of its own.
                self.keyed = None;
            }
            Change::Keyed => {
                let within = self
                    .keyed
                    .is_some_and(|t| now < t + Duration::from_millis(SCROLL_BURST_MS));
                if !within {
                    self.undo.push(before);
                }
                self.keyed = Some(now);
            }
            Change::Live | Change::Follow => {}
        }
        let t = ms(now);
        if how == Change::Live || !motion {
            self.ease = None;
        } else {
            let from = explicit(model, &self.shown(t));
            let to = explicit(model, &v);
            self.ease = Some(Ease {
                tween: Tween::new(from, to, t, MOTION_MS),
                graph: None,
                gantt: None,
            });
        }
        self.view = v;
        self.rehover = true;
        true
    }

    /// The hover worked out again from where the pointer is: hidden during a gesture, and
    /// after a key moved the cursor until the pointer moves.
    fn hover_again(&mut self, model: Option<&ChartModel>) {
        self.rehover = false;
        let mut strip = None;
        let (hover, near, legend) = match (self.pointer, self.hovering && !self.gesturing()) {
            (Some(p), true) => {
                let hit = self.hit_at(p);
                match (hit, model, &self.layout, &self.hits) {
                    (Hit::LegendKey(k), ..) => (None, None, Some(k)),
                    (
                        Hit::Plot(_) | Hit::Bar { .. } | Hit::Cell { .. },
                        Some(m),
                        Some(l),
                        Some(h),
                    ) => {
                        let near = plot::hover::nearest_series(m, l, h, &self.view, p.0, p.1);
                        (
                            plot::hover::hover(m, l, h, &self.view, p.0, p.1),
                            self.steady_near(m, l, near, p),
                            None,
                        )
                    }
                    (hit @ (Hit::AxisX(_) | Hit::AxisY(_)), Some(m), Some(l), _) => {
                        strip = Some(match hit {
                            Hit::AxisX(_) => l.x_axis,
                            _ => l.y_axis,
                        });
                        (category_at(m, l, &hit, p), None, None)
                    }
                    _ => (None, None, None),
                }
            }
            _ => (None, None, None),
        };
        (self.hover, self.near, self.legend) = (hover, near, legend);
        self.strip = strip;
    }

    /// The series the hover brings forward, kept while the pointer stays near its line: another
    /// takes over only once it's nearer by [`NEAR_MARGIN`], and it lets go only past
    /// [`NEAREST_SERIES`] and [`NEAR_KEEP`], measured against what the line reaches over the
    /// pointer's pixel columns. So a hand that trembles over a dense series doesn't flicker
    /// the plot-wide wash on and off, each change of which draws the whole plot again.
    fn steady_near(
        &self,
        model: &ChartModel,
        l: &ChartLayout,
        near: Option<SeriesKey>,
        p: (f32, f32),
    ) -> Option<SeriesKey> {
        let Some(cur) = self.near.clone() else {
            return near;
        };
        if near.as_ref() == Some(&cur) || !self.view.shows(&cur) {
            return near;
        }
        let Some(dc) = line_distance(model, l, &cur, p) else {
            return near;
        };
        if dc > NEAREST_SERIES + NEAR_KEEP {
            return near;
        }
        match &near {
            None => Some(cur),
            Some(n) => match line_distance(model, l, n, p) {
                Some(dn) if dn + NEAR_MARGIN < dc => near,
                _ => Some(cur),
            },
        }
    }

    /// The brush as it's drawn and applied: a box brush with Shift held, once it's past twice
    /// the slop, only along its larger side (mouse-ux §4.3).
    #[allow(clippy::type_complexity)]
    fn brush_shape(&self) -> Option<(BrushKind, (f32, f32), (f32, f32))> {
        let Gesture::Brush { kind, from, to, .. } = &self.gesture else {
            return None;
        };
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let kind = match kind {
            BrushKind::Box if self.drag_mods.shift && dx.hypot(dy) >= 2.0 * CLICK_SLOP => {
                if dx.abs() >= dy.abs() {
                    BrushKind::X
                } else {
                    BrushKind::Y
                }
            }
            k => *k,
        };
        Some((kind, *from, *to))
    }

    /// The overlay's state as it stands. The pointer goes with a hover, and with a legend key
    /// under it, which the overlay underlines.
    fn overlay_state(&self) -> OverlayState {
        let pointed = self.hover.is_some() || self.legend.is_some();
        OverlayState {
            hover: self.hover.clone(),
            cursor: self.cursor.clone(),
            pointer: self.pointer.filter(|_| pointed),
            brush: self.brush_shape(),
            emphasis: (self.legend.clone())
                .or_else(|| self.row_hover.as_ref().and_then(|i| i.series().cloned()))
                .or_else(|| self.near.clone())
                .or_else(|| self.found.clone()),
        }
    }

    /// What a chart's overlay shows past [`OverlayState`]: the series and categories a search
    /// lights (as typed, or found), and the cursor's card, which goes while the detail panel
    /// says the same (a hover's card stays).
    pub fn chart_lit(&self) -> plot::overlay::Lit {
        let bands = match (self.lit_bands.is_empty(), self.found_band) {
            (true, Some(b)) if self.lit.is_empty() => vec![b],
            _ => self.lit_bands.clone(),
        };
        plot::overlay::Lit {
            series: self.lit.clone(),
            bands,
            cursor_card: self.panel.is_none(),
        }
    }

    /// Make or grow the two buffers for the area. False if the host gave none.
    fn buffers_for(&mut self, ctx: &mut Ctx) -> std::result::Result<(), String> {
        let (w, h) = self.area_px();
        for slot in [&mut self.base, &mut self.over] {
            match slot {
                None => {
                    let (_, p) = ctx.pixel_buffer(w, h).map_err(|e| e.to_string())?;
                    *slot = Some(p);
                }
                Some(p) => {
                    let (bw, bh) = p.size();
                    if w > bw || h > bh {
                        p.resize(w.max(bw), h.max(bh)).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// How much further than [`NEAREST_SERIES`] the series the hover brought forward stays forward.
const NEAR_KEEP: f32 = 8.0;
/// How much nearer another series must be to take over from it.
const NEAR_MARGIN: f32 = 4.0;

/// How far the pointer is from a series' line, in px: from the least and most the line
/// reaches over the pixel columns either side of the pointer, so a dense series' samples,
/// which jump about within a column, don't count one at a time. None where it isn't a line
/// across the screen (a stack's band, bars across), or has no samples there.
fn line_distance(
    model: &ChartModel,
    l: &ChartLayout,
    key: &SeriesKey,
    (x, y): (f32, f32),
) -> Option<f32> {
    if l.horizontal || !model.is_x_chart() {
        return None;
    }
    let si = model.index_of(key)?;
    if l.stacks.lo.get(si).is_some_and(Option::is_some) {
        return None;
    }
    let s = &model.series[si];
    let n = s.xs.len().min(s.ys.len());
    let (xs, ys) = (&s.xs[..n], &s.ys[..n]);
    let (a, b) = (l.x.invert(x - 1.5), l.x.invert(x + 1.5));
    let (a, b) = (a.min(b), a.max(b));
    let (i0, i1) = (
        xs.partition_point(|&v| v < a),
        xs.partition_point(|&v| v <= b),
    );
    let present = |i: usize| xs[i].is_finite() && ys[i].is_finite();
    let mut reach: Vec<f64> = (i0..i1).filter(|&i| present(i)).map(|i| ys[i]).collect();
    // Where the line crosses the columns' edges, between the samples either side.
    for (at, i) in [(a, i0), (b, i1)] {
        if i > 0 && i < n && present(i - 1) && present(i) && xs[i] > xs[i - 1] {
            let t = (at - xs[i - 1]) / (xs[i] - xs[i - 1]);
            reach.push(ys[i - 1] + (ys[i] - ys[i - 1]) * t.clamp(0.0, 1.0));
        }
    }
    let px = reach.into_iter().map(|v| l.vy(v)).filter(|v| v.is_finite());
    let (lo, hi) = px.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    if !lo.is_finite() {
        return None;
    }
    Some(if y < lo {
        lo - y
    } else if y > hi {
        y - hi
    } else {
        0.0
    })
}

/// The model's axis an axis strip moves: the strip under the plot is x, the one beside it y,
/// the other way about for bars across.
fn strip_axis(hit: &Hit, horizontal: bool) -> Option<Axis> {
    match (hit, horizontal) {
        (Hit::AxisX(_), false) | (Hit::AxisY(_), true) => Some(Axis::X),
        (Hit::AxisY(_), false) | (Hit::AxisX(_), true) => Some(Axis::Y),
        _ => None,
    }
}

/// On a band chart's category axis, the band whose label is under the pointer (mouse-ux §4.9).
fn category_at(
    model: &ChartModel,
    l: &ChartLayout,
    hit: &Hit,
    (x, y): (f32, f32),
) -> Option<Cursor> {
    if !model.is_band() || strip_axis(hit, l.horizontal) != Some(Axis::X) {
        return None;
    }
    let p = l.plot;
    let band = l.band_at(x.clamp(p.x, p.right()), y.clamp(p.y, p.bottom()))?;
    Some(Cursor::Band {
        band: band as u32,
        series: None,
    })
}

/// Whether two targets are one moment: on an x-chart, samples at one x, which the reference
/// marks with one rule across every series; on bars, one band, whichever its segment; else
/// the same target.
fn same_moment(model: &ChartModel, a: &Cursor, b: &Cursor) -> bool {
    let x = |c: &Cursor| match c {
        Cursor::Point { series, row } => {
            (model.series_of(series)).and_then(|s| s.xs.get(*row as usize).copied())
        }
        _ => None,
    };
    match (a, b) {
        (Cursor::Band { band: p, .. }, Cursor::Band { band: q, .. }) => p == q,
        (Cursor::Point { .. }, Cursor::Point { .. }) if model.is_x_chart() => {
            x(a).is_some() && x(a) == x(b)
        }
        _ => a == b,
    }
}

/// The content motion, in logical px, that moves one of the model's axes by `v` px towards its
/// later or higher values as [`plot::nav::pan`] reads it with that axis named, across or down
/// the screen as the chart lies.
fn along(axis: Axis, v: f32, horizontal: bool) -> (f32, f32) {
    match (axis, horizontal) {
        (Axis::X, false) => (-v, 0.0),
        (Axis::X, true) => (0.0, -v),
        (Axis::Y, false) => (0.0, v),
        (Axis::Y, true) => (-v, 0.0),
    }
}

/// A view with its domains spelled out: what fits where it says None.
fn explicit(model: &ChartModel, v: &View) -> View {
    let (fx, fy) = model.fit(v);
    View {
        x: Some(v.x.unwrap_or(fx)),
        y: Some(v.y.unwrap_or(fy)),
        ..v.clone()
    }
}

/// A pane's pixels for drawing into: the buffer's bytes, size and stride.
fn target(p: &mut PixelProducer) -> (u32, u32, usize, &mut [u8]) {
    let (w, h) = p.size();
    let stride = p.stride();
    (w, h, stride, p.pixels_mut())
}

// ------------------------------------------------------------------ a figure's state

/// A figure: where its text comes from, what it parsed to, its model, and its panes.
pub struct FigState {
    /// Its buffer, once made.
    pub buf: Option<BufferId>,
    /// Where its text comes from.
    pub source: Source,
    /// Its name: the file's, `selection`, or the command.
    pub name: String,
    /// Where relative paths start.
    pub dir: PathBuf,
    /// The buffer it was opened from, which `\v` goes back to when its source is no buffer.
    pub from: Option<BufferId>,
    /// The file its text came from, if any: `fig-spec` names it in place of inline data.
    pub file: Option<PathBuf>,
    kit: SharedKit,
    zone: TimeZone,
    me: Weak<RefCell<FigState>>,
    /// The figure as last parsed.
    pub figure: Option<Figure>,
    /// Whether it was guessed from bare data.
    pub inferred: bool,
    /// Its chart as parsed, before the settings.
    pub chart: Option<Chart>,
    /// The chart's tables.
    pub tables: Vec<Arc<Table>>,
    /// The chart's model, as last built.
    pub model: Option<Arc<ChartModel>>,
    /// The graph's model, laid out, as last built: a figure is a chart, a graph or a gantt.
    pub graph: Option<Arc<GraphModel>>,
    /// The gantt's model, its dates worked out, as last built.
    pub gantt: Option<Arc<GanttModel>>,
    /// Goes up with each model.
    pub model_gen: u64,
    /// Why the last build failed: the error figure says so, or the stale badge over the last
    /// figure, which stays.
    pub error: Option<Failed>,
    /// Goes up with each failure and each recovery: the base shows the newest.
    pub fail_gen: u64,
    /// The figure's own text as the last build read it, for the log's caret lines.
    pub text: Arc<str>,
    /// The warnings last said, so a build says them once.
    warned: Vec<FigError>,
    /// The failure last said in `*messages*`, so a run of the same says it once.
    said: Option<String>,
    /// The files read from disk, polled while shown.
    files: Vec<(PathBuf, Option<SystemTime>)>,
    /// The data files read from their buffers, followed as edited.
    pub followed: Vec<PathBuf>,
    /// The settings `s` turned.
    pub toggles: Toggles,
    /// A build from the text under way (or the command that gives it): it applies the
    /// settings it was given, and builds again from its tables if they've turned since.
    building: Option<Sub>,
    /// A model built again from the tables in hand, for a setting turned: never in place of a
    /// build from the text, which would lose what it read.
    rebuilding: Option<Sub>,
    /// The key card was put up by the command just run, so the settle after it keeps it.
    pub help_new: bool,
    /// How many builds have been started.
    pub builds: u64,
    /// Waiting to be built again when a window shows it.
    pub behind: bool,
    debounce: Option<Sub>,
    poll: Option<Sub>,
    motion: Option<Sub>,
    /// The colours, as last resolved.
    pub theme: Theme,
    theme_gen: u64,
    /// Each window's pane.
    pub panes: BTreeMap<WindowId, Pane>,
    /// The window that last had a pane: a new window starts from its view.
    pub last: Option<WindowId>,
    /// What the last pane left, for the next window to show it.
    parked: Option<(View, Option<Cursor>, Steps<Snap>)>,
    /// A window showing it has the focus.
    pub focused: bool,
    focused_at: Option<Duration>,
    /// Said once: the host shows no pixels.
    refused: bool,
    /// The last search: its text, for `n` and `N`.
    pub query: Option<String>,
    /// The last search's matches, for `n` and `N`; while `/` is typed, the one under way.
    pub search: Option<Search>,
    /// A restart's word that the panel was open: it opens again in the first window drawn.
    pub panel_seed: bool,
    /// How a yank from the panel reaches the registers.
    pub kill: Option<Kill>,
    /// Goes up with what a restart keeps: the session's cache key.
    pub rev: u64,
    /// Views an undo of a layout (or labels) setting put back, each with its window: applied
    /// when the graph laid out again lands, so pins in the old layout's units never draw on
    /// the new one, and the switch's own clearing doesn't take them.
    pending: Vec<(WindowId, View)>,
    /// The layout `auto` picked for the graph, kept while its spec still says `auto` and no
    /// setting chooses one, so an edit that follows the source doesn't lay it out anew.
    auto_pick: Option<Layout>,
    /// The dashboard, when the figure is one (or a tile alone).
    pub dash: Option<dash::DashState>,
    /// The figure's commands, run and streamed while a window shows it.
    pub live: Option<crate::live::Live>,
    /// Where its commands' PATH comes from (these and `:fig !cmd`'s): your login shell's, but a
    /// test's own.
    pub path: crate::live::PathFrom,
    live_timer: Option<Sub>,
    /// Your yeses (ruling 13), shared by every figure and kept by the session.
    pub trust: Rc<RefCell<crate::live::Trust>>,
    /// Goes up with each answer: the session's cache key.
    pub trust_rev: u64,
    /// A question is open in the prompt.
    asking: bool,
    /// The `file` panels of a dashboard with no file of its own, and what you said, this
    /// session.
    files_said: Vec<(PathBuf, bool)>,
    /// What a restart kept of a dashboard's view, for when it's built.
    pub dash_seed: Option<die_core::session::Value>,
    /// The chart's tables that come from commands.
    chart_feeds: Vec<source::Feed>,
    /// What each command has printed so far, for a build of the chart.
    live_tables: Vec<(crate::live::LiveSpec, Arc<Table>)>,
}

/// A figure's state, shared by its surface, the extension and its timers.
pub type Shared = Rc<RefCell<FigState>>;

impl FigState {
    /// A figure from `source`, named `name`, paths from `dir`, times in `zone`, shown with
    /// `view` and `cursor` when its first window opens.
    pub fn new(
        source: Source,
        name: String,
        dir: PathBuf,
        kit: SharedKit,
        zone: TimeZone,
        seed: Option<(View, Option<Cursor>)>,
    ) -> Shared {
        Rc::new_cyclic(|me| {
            RefCell::new(FigState {
                buf: None,
                source,
                name,
                dir,
                from: None,
                file: None,
                kit,
                zone,
                me: me.clone(),
                figure: None,
                inferred: false,
                chart: None,
                tables: vec![],
                model: None,
                graph: None,
                gantt: None,
                model_gen: 0,
                error: None,
                fail_gen: 0,
                text: Arc::from(""),
                warned: vec![],
                said: None,
                files: vec![],
                followed: vec![],
                toggles: Toggles::default(),
                building: None,
                rebuilding: None,
                help_new: false,
                builds: 0,
                behind: false,
                debounce: None,
                poll: None,
                motion: None,
                theme: Theme::mono_dark(),
                theme_gen: 0,
                panes: BTreeMap::new(),
                last: None,
                parked: seed.map(|(v, c)| (v, c, Steps::default())),
                focused: true,
                focused_at: None,
                refused: false,
                query: None,
                search: None,
                panel_seed: false,
                kill: None,
                rev: 1,
                pending: vec![],
                auto_pick: None,
                dash: None,
                live: None,
                path: crate::live::PathFrom::Login,
                live_timer: None,
                trust: Rc::new(RefCell::new(crate::live::Trust::new())),
                trust_rev: 0,
                asking: false,
                files_said: vec![],
                dash_seed: None,
                chart_feeds: vec![],
                live_tables: vec![],
            })
        })
    }

    /// Whether a window shows it.
    pub fn shown(&self, ctx: &Ctx) -> bool {
        let Some(buf) = self.buf else { return false };
        ctx.window_list()
            .into_iter()
            .any(|w| ctx.window_buffer(w) == Some(buf))
    }

    /// The pane of the window a command came from, or the last one.
    pub fn pane_of(&self, w: Option<WindowId>) -> Option<WindowId> {
        w.filter(|w| self.panes.contains_key(w))
            .or(self.last.filter(|w| self.panes.contains_key(w)))
            .or_else(|| self.panes.keys().next().copied())
    }

    /// The view a restart keeps, and the cursor: the last window's.
    pub fn kept_view(&self) -> (View, Option<Cursor>) {
        if let Some(p) = self.last.and_then(|w| self.panes.get(&w)) {
            return (p.view.clone(), p.cursor.clone());
        }
        match &self.parked {
            Some((v, c, _)) => (v.clone(), c.clone()),
            None => (View::default(), None),
        }
    }

    /// Whether a restart opens the detail panel again: the last window's is open.
    pub fn kept_panel(&self) -> bool {
        match self.last.and_then(|w| self.panes.get(&w)) {
            Some(p) => p.panel.is_some(),
            None => self.panel_seed,
        }
    }

    /// The colours as the theme has them now: a change draws every base again.
    pub fn look(&mut self, ctx: &Ctx) {
        let theme = theme::resolve(ctx);
        if theme != self.theme {
            self.theme = theme;
            self.theme_gen += 1;
        }
    }

    /// Whether discrete changes ease: the spec's `motion`.
    fn motion(&self) -> bool {
        match (&self.graph, &self.gantt) {
            (Some(g), _) => g.spec.view.motion,
            (None, Some(g)) => g.spec.view.motion,
            (None, None) => self.chart.as_ref().is_none_or(|c| c.view.motion),
        }
    }

    /// The gantt's spec as parsed, before the settings.
    pub fn gantt_spec(&self) -> Option<&GanttSpec> {
        match &self.figure {
            Some(Figure::Gantt(g)) => Some(g),
            _ => None,
        }
    }

    /// The graph's spec as parsed, before the settings.
    pub fn graph_spec(&self) -> Option<&GraphSpec> {
        match &self.figure {
            Some(Figure::Graph(g)) => Some(g),
            _ => None,
        }
    }

    /// What the stale badge says while the last build failed: `Error · line 14`.
    pub fn badge(&self) -> Option<String> {
        let f = self.error.as_ref()?;
        Some(plot::errors::badge_text(f.first()))
    }

    /// The last good build's warnings.
    pub fn warnings(&self) -> &[FigError] {
        &self.warned
    }

    /// What the log lists: the last build's errors and warnings, or the warnings of the good
    /// build shown.
    pub fn log_errors(&self) -> Vec<FigError> {
        match &self.error {
            Some(f) => f.all(),
            None => self.warned.iter().map(|w| w.clone().warn()).collect(),
        }
    }

    /// The log's layout as it stands, for the keys that move in it.
    pub fn log_layout(&self) -> plot::LogLayout {
        plot::log_layout(&self.log_errors(), &self.text, &self.name)
    }

    /// The status line: the name, how many series (a graph's nodes and edges, a gantt's tasks),
    /// how many hidden, whether it follows live data, and why the last build failed.
    pub fn status(&self) -> String {
        let mut s = self.name.clone();
        if let Some(g) = &self.graph {
            s.push_str(&format!("  {} nodes  {} edges", g.len(), g.ends.len()));
            if let Some(p) = self.last.and_then(|w| self.panes.get(&w)) {
                let hidden = g.keys().iter().filter(|k| !p.view.shows(k)).count();
                if hidden > 0 {
                    s.push_str(&format!("  {hidden} groups hidden"));
                }
                if !p.view.pinned.is_empty() {
                    s.push_str(&format!("  {} pinned", p.view.pinned.len()));
                }
            }
        }
        if let Some(g) = &self.gantt {
            let n = g.len();
            s.push_str(&format!("  {n} task{}", if n == 1 { "" } else { "s" }));
            if let Some(p) = self.last.and_then(|w| self.panes.get(&w)) {
                let hidden = n - g.shown(&p.view).len();
                if hidden > 0 {
                    s.push_str(&format!("  {hidden} hidden"));
                }
            }
        }
        if let Some(d) = self.dash.as_ref().and_then(|d| d.model.as_ref()) {
            let n = d.panels.len();
            let pane = self.last.and_then(|w| self.panes.get(&w));
            match pane
                .and_then(|p| p.dash.as_ref())
                .and_then(|dp| dp.view.focused)
            {
                Some(f) => s.push_str(&format!("  panel {}/{n}", f + 1)),
                None => s.push_str(&format!("  {n} panels")),
            }
            if d.is_live() && pane.is_some_and(Pane::following) {
                s.push_str("  following");
            }
        }
        for n in self.live_notes() {
            s.push_str("  ");
            s.push_str(&n);
        }
        if let Some(m) = &self.model {
            let n = m.keys().len();
            s.push_str(&format!("  {n} series"));
            if let Some(p) = self.last.and_then(|w| self.panes.get(&w)) {
                let hidden = m.keys().iter().filter(|k| !p.view.shows(k)).count();
                if hidden > 0 {
                    s.push_str(&format!("  {hidden} hidden"));
                }
                if p.view.follow {
                    s.push_str("  following");
                }
            }
        }
        if let Some(b) = self.badge() {
            s.push_str("  ");
            s.push_str(&b);
        }
        s
    }

    // -------------------------------------------------------------- building

    /// Build the figure again, off the main thread, from its text as it is now: a buffer's
    /// snapshot, a file, a command's output. A build started later wins.
    pub fn build(&mut self, ctx: &mut Ctx) {
        self.behind = false;
        self.debounce = None;
        let text = match &self.source {
            Source::Buffer(b) => match ctx.text(*b) {
                Ok(t) => Text::Of(Box::new(t.rope().clone())),
                Err(_) => return,
            },
            Source::File(p) => Text::File(p.clone()),
            Source::Text(s) => Text::Given(s.clone()),
            Source::Command { out: Some(s), .. } => Text::Given(s.clone()),
            Source::Command { out: None, .. } => return self.run_command(ctx),
        };
        let mut open: Vec<(PathBuf, Snapshot)> = vec![];
        for b in ctx.buffer_list() {
            let Some(path) = ctx.file(b).map(|f| f.path.clone()) else {
                continue;
            };
            if let Ok(t) = ctx.text(b) {
                open.push((path, Box::new(t.rope().clone())));
            }
        }
        let toggles = self.toggles;
        let job = Job {
            text,
            name: self.format_name(ctx),
            dir: self.dir.clone(),
            zone: self.zone.clone(),
            open,
            toggles,
            auto: self.auto_pick.clone(),
            now: crate::source::clock(),
        };
        self.builds += 1;
        // This build applies the settings as they are now: a rebuild for them is no more use.
        self.rebuilding = None;
        let me = self.me.clone();
        let live = self.live_tables.clone();
        self.building = Some(ctx.spawn(
            move || source::build_with(job, &live),
            move |ctx, out| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    st.building = None;
                    st.built(ctx, out);
                    // A setting turned (or was undone) while it ran: again from its tables.
                    if st.toggles != toggles {
                        st.rebuild(ctx);
                    }
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// The name a build reads the format from: the source file's, else the buffer's.
    fn format_name(&self, ctx: &Ctx) -> String {
        if let Some(name) = self.file.as_ref().and_then(|p| p.file_name()) {
            return name.to_string_lossy().into_owned();
        }
        match &self.source {
            Source::Buffer(b) => ctx.buffer_name(*b).unwrap_or_default().to_string(),
            _ => String::new(),
        }
    }

    /// `:fig !cmd`'s command, run once in a job: its standard output is the figure's text.
    fn run_command(&mut self, ctx: &mut Ctx) {
        let Source::Command { cmd, dir, .. } = &self.source else {
            return;
        };
        let (cmd, dir, path) = (cmd.clone(), dir.clone(), self.path);
        let me = self.me.clone();
        self.builds += 1;
        self.building = Some(ctx.spawn(
            move || -> std::result::Result<String, FigError> {
                // Your shell with your login PATH; a test's /bin/sh with its own, so no
                // startup file under your HOME is read.
                let mut c = if path == crate::live::PathFrom::Login {
                    let mut c = die_core::process::shell(&cmd, &dir);
                    c.env("PATH", die_core::process::path());
                    c
                } else {
                    let mut c = std::process::Command::new("/bin/sh");
                    c.arg("-c").arg(&cmd).current_dir(&dir);
                    c
                };
                c.stdin(std::process::Stdio::null());
                let ran = format!("ran `{cmd}` in {}", dir.display());
                let unreadable = |msg: String| FigError::new(msg).with_kind(ErrorKind::Unreadable);
                let out = c
                    .output()
                    .map_err(|e| unreadable(format!("{cmd}: {e}")).note(ran.clone()))?;
                let silent = out.stdout.iter().all(u8::is_ascii_whitespace);
                if silent {
                    // Failed, it's unreadable; ran well and printed nothing, there's nothing
                    // to draw, and the log says it ran, not that a buffer was empty.
                    let mut e = match out.status.success() {
                        false => unreadable(format!("{cmd} failed and printed nothing")),
                        true => FigError::new(format!("!{cmd} printed nothing"))
                            .with_kind(ErrorKind::Empty),
                    }
                    .note(format!("{ran}: {}", out.status));
                    // The end of what it said, a line each.
                    let err = String::from_utf8_lossy(&out.stderr);
                    let lines: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
                    for l in &lines[lines.len().saturating_sub(STDERR_TAIL)..] {
                        e = e.note(format!("stderr: {}", l.trim_end()));
                    }
                    return Err(e);
                }
                Ok(String::from_utf8_lossy(&out.stdout).into_owned())
            },
            move |ctx, out| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    st.building = None;
                    match out {
                        Ok(text) => {
                            if let Source::Command { out, .. } = &mut st.source {
                                *out = Some(text.into());
                            }
                            st.build(ctx);
                        }
                        Err(e) => {
                            st.failed(ctx, Failed::one(e));
                            st.rev += 1;
                        }
                    }
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// Build the model again from the chart and tables already read: the settings changed.
    /// With a build from the text under way, that build does it when it's done, from what it
    /// read (it compares the settings it was given with these).
    fn rebuild(&mut self, ctx: &mut Ctx) {
        if self.building.is_some() {
            return;
        }
        if let Some(spec) = self.graph_spec().cloned() {
            return self.rebuild_graph(ctx, spec);
        }
        if let Some(spec) = self.gantt_spec().cloned() {
            return self.rebuild_gantt(ctx, spec);
        }
        let Some(chart) = self.chart.clone() else {
            return;
        };
        let (tables, toggles, zone) = (self.tables.clone(), self.toggles, self.zone.clone());
        let me = self.me.clone();
        self.rebuilding = Some(ctx.spawn(
            move || source::rebuild(&chart, tables, toggles, &zone),
            move |ctx, out| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    st.rebuilding = None;
                    match out {
                        Ok(model) => st.new_model(model, ctx.now()),
                        Err(error) => {
                            let f = Failed {
                                text: st.text.clone(),
                                ..Failed::one(error)
                            };
                            st.failed(ctx, f);
                        }
                    }
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// The graph laid out again from its spec as parsed, for a setting turned (its labels, its
    /// layout): in a job, the last frame up until it's done.
    fn rebuild_graph(&mut self, ctx: &mut Ctx, spec: GraphSpec) {
        let (toggles, name) = (self.toggles, self.format_name(ctx));
        let auto = self.auto_pick.clone();
        let me = self.me.clone();
        self.rebuilding = Some(ctx.spawn(
            move || source::graph_model(&spec, toggles, &name, auto.as_ref()),
            move |ctx, out| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    st.rebuilding = None;
                    match out {
                        Ok(model) => st.new_graph(model),
                        Err(error) => {
                            let f = Failed {
                                text: st.text.clone(),
                                ..Failed::one(error)
                            };
                            st.failed(ctx, f);
                        }
                    }
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// The gantt's model again from its spec as parsed and its table as read, for a setting
    /// turned (its grid, axis, links, today line): in a job, the last frame up until it's done.
    /// No file is read: the table is the one the last build from the text read.
    fn rebuild_gantt(&mut self, ctx: &mut Ctx, spec: GanttSpec) {
        let Some(table) = self.tables.first().cloned() else {
            return;
        };
        let (toggles, zone) = (self.toggles, self.zone.clone());
        let source = self.gantt.as_ref().and_then(|g| g.source.clone());
        let now = source::clock();
        let me = self.me.clone();
        self.rebuilding = Some(ctx.spawn(
            move || source::gantt_model(&spec, table, toggles, &zone, now, source),
            move |ctx, out| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    st.rebuilding = None;
                    match out {
                        Ok(model) => st.new_gantt(model),
                        Err(error) => {
                            let f = Failed {
                                text: st.text.clone(),
                                ..Failed::one(error)
                            };
                            st.failed(ctx, f);
                        }
                    }
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// A build's outcome: the new figure, or the last one kept with the stale badge saying
    /// why not (the error figure where there's none).
    fn built(&mut self, ctx: &mut Ctx, out: std::result::Result<source::Built, Failed>) {
        self.rev += 1;
        match out {
            Ok(b) => {
                if b.warnings != self.warned {
                    for w in &b.warnings {
                        ctx.messages.push(format!("fig: {}: {w}", self.name));
                    }
                    self.warned = b.warnings.clone();
                }
                let dash = dash::spec_of(&b.figure);
                self.figure = Some(b.figure);
                self.inferred = b.inferred;
                self.chart = b.chart;
                self.tables = b.tables;
                self.files = b.files;
                self.followed = b.followed;
                self.text = b.text;
                if self.error.take().is_some() {
                    self.fail_gen += 1;
                }
                self.said = None;
                // The log stays up where it has warnings to show; otherwise, the figure.
                let lines = self.log_layout();
                for p in self.panes.values_mut() {
                    p.log = p
                        .log
                        .filter(|_| !self.warned.is_empty())
                        .map(|(top, mark)| (top, mark.min(lines.starts.len().saturating_sub(1))));
                }
                self.chart_feeds = b.feeds;
                match (dash, b.model, b.graph, b.gantt) {
                    (Some(spec), ..) => {
                        if self.model.is_some() || self.graph.is_some() || self.gantt.is_some() {
                            (self.model, self.graph, self.gantt) = (None, None, None);
                            self.model_gen += 1;
                            for p in self.panes.values_mut() {
                                (p.layout, p.graph, p.hover, p.target) = (None, None, None, None);
                                (p.gantt, p.task) = (None, None);
                                (p.cursor, p.panel, p.ease) = (None, None, None);
                                p.gesture = Gesture::None;
                            }
                        }
                        self.dash_built(ctx, spec);
                    }
                    (None, Some(m), ..) => {
                        self.leave_dash();
                        self.new_model(m, ctx.now());
                    }
                    (None, None, Some(g), _) => {
                        self.leave_dash();
                        self.new_graph(g);
                    }
                    (None, None, None, Some(g)) => {
                        self.leave_dash();
                        self.new_gantt(g);
                    }
                    (None, None, None, None) => {
                        self.leave_dash();
                        (self.model, self.graph, self.gantt) = (None, None, None);
                        self.model_gen += 1;
                    }
                }
            }
            Err(f) => {
                // The figure stays; the files it read before failing are polled, so a fix to
                // one of them brings it back.
                for (p, t) in &f.files {
                    if !self.files.iter().any(|(q, _)| q == p) {
                        self.files.push((p.clone(), *t));
                    }
                }
                if !f.feeds.is_empty() {
                    self.chart_feeds = f.feeds.clone();
                }
                self.failed(ctx, f);
            }
        }
        self.keep_polling(ctx);
        self.live_sync(ctx);
    }

    /// The figure is no dashboard (any more): its windows' dashboards go.
    fn leave_dash(&mut self) {
        if self.dash.take().is_some() {
            for p in self.panes.values_mut() {
                p.dash = None;
                p.drawn = None;
                p.over_key = None;
            }
        }
    }

    /// A build failed: the error figure or the badge says so from now on, and `*messages*` does
    /// once, a run of the same failure (the same error at the same place) said once. A log up
    /// keeps its place within the new one.
    pub fn failed(&mut self, ctx: &mut Ctx, f: Failed) {
        let first = f.first();
        let said = match first.place() {
            Some(p) => format!("fig: {}: {p}: {}", self.name, first.msg),
            None => format!("fig: {}: {}", self.name, first.msg),
        };
        if self.said.as_ref() != Some(&said) {
            ctx.messages.push(said.clone());
            self.said = Some(said);
        }
        if !f.text.is_empty() {
            self.text = f.text.clone();
        }
        self.error = Some(f);
        self.fail_gen += 1;
        self.rev += 1;
        let lines = self.log_layout();
        for p in self.panes.values_mut() {
            if let Some((top, mark)) = p.log {
                p.log = Some((top, mark.min(lines.starts.len().saturating_sub(1))));
            }
        }
    }

    /// A model to draw: the cursors and references that no longer name a sample, a band or a
    /// cell let go, a zoomed view the data has left fits it again, and a view that follows
    /// live data slides to its newest.
    fn new_model(&mut self, model: ChartModel, now: Duration) {
        // Live data moves rows: a cursor stays on its sample, by its series and x.
        if let (false, Some(old)) = (self.chart_feeds.is_empty(), self.model.as_deref()) {
            for p in self.panes.values_mut() {
                p.cursor =
                    (p.cursor.take()).and_then(|c| crate::live::keep_cursor(old, &model, &c));
                p.view.reference = (p.view.reference.take())
                    .and_then(|c| crate::live::keep_cursor(old, &model, &c));
            }
        }
        let keys = model.keys();
        let len = |key: &SeriesKey| {
            let s = model.series.iter().find(|s| &s.key == key);
            s.map_or(0, |s| s.xs.len())
        };
        let (cols, rows) = (model.x.bands.len(), model.y.bands.len());
        let fits = |c: &Option<Cursor>| match c {
            Some(Cursor::Point { series, row }) => {
                keys.contains(series) && (*row as usize) < len(series)
            }
            Some(Cursor::Band { band, series }) => {
                (*band as usize) < cols && series.as_ref().is_none_or(|k| keys.contains(k))
            }
            Some(Cursor::Cell { row, col }) => (*row as usize) < rows && (*col as usize) < cols,
            Some(Cursor::Node(_) | Cursor::Task(_)) => false,
            None => true,
        };
        let (from_graph, from_gantt) = (self.graph.is_some(), self.gantt.is_some());
        self.auto_pick = None;
        let (ext_x, ext_y) = model.extent();
        // A domain that no longer meets the data's extent shows nothing: it fits again.
        let meets = |d: Option<(f64, f64)>, e: (f64, f64)| {
            d.is_none_or(|(a, b)| a.max(b) >= e.0.min(e.1) && a.min(b) <= e.0.max(e.1))
        };
        let newest = model.newest();
        let motion = self.motion();
        let toggles = self.toggles;
        for p in self.panes.values_mut() {
            if !fits(&p.cursor) {
                p.cursor = None;
            }
            if !fits(&p.view.reference) {
                p.view.reference = None;
            }
            if from_graph || from_gantt {
                // From a graph to a chart: its window (in layout units), its pins and its
                // groups mean nothing here. Nor do a gantt's time, rows and names.
                (p.view.x, p.view.y, p.view.follow, p.ease) = (None, None, false, None);
                (p.view.hidden, p.view.only) = (Default::default(), None);
                p.view.pinned.clear();
                (p.calm, p.panel) = (None, None);
                if let Gesture::NodeDrag { .. } = p.gesture {
                    p.gesture = Gesture::Swallowed;
                }
                if from_gantt && p.gesturing() {
                    p.gesture = Gesture::Swallowed;
                }
            }
            if p.found_band.is_some_and(|b| b as usize >= cols) {
                p.found_band = None;
            }
            let (left_x, left_y) = (!meets(p.view.x, ext_x), !meets(p.view.y, ext_y));
            if (left_x || left_y) && !p.gesturing() {
                if left_x {
                    (p.view.x, p.view.follow) = (None, false);
                }
                if left_y {
                    p.view.y = None;
                }
                p.ease = None;
            }
            if let (true, Some((a, b)), Some(n), false) =
                (p.view.follow, p.view.x, newest, p.gesturing())
                && n > b
            {
                let v = View {
                    x: Some((n - (b - a), n)),
                    ..p.view.clone()
                };
                p.set_view(v, Change::Follow, toggles, &model, motion, now);
            }
            p.rehover = true;
            (p.graph, p.target, p.dwell, p.dwell_next, p.anchor) = (None, None, None, None, None);
            (p.found_node, p.matches) = (None, vec![]);
            (p.gantt, p.task) = (None, None);
        }
        self.model = Some(Arc::new(model));
        (self.graph, self.gantt) = (None, None);
        self.model_gen += 1;
        self.rev += 1;
    }

    /// A graph to draw: what names a node that's gone lets go (the cursor, the reference, a
    /// pin, the search's find), and the rest stays as it was, the view and the pins of every
    /// node whose id survived the edit among them.
    fn new_graph(&mut self, model: GraphModel) {
        let has = |c: &Option<Cursor>| match c {
            Some(Cursor::Node(id)) => model.index_of(id).is_some(),
            Some(_) => false,
            None => true,
        };
        for p in self.panes.values_mut() {
            if !has(&p.cursor) {
                p.cursor = None;
            }
            if !has(&p.view.reference) {
                p.view.reference = None;
            }
            if !has(&p.found_node) {
                p.found_node = None;
            }
            if !has(&p.dwell) {
                (p.dwell, p.dwell_next) = (None, None);
            }
            p.view.pinned.retain(|id, _| model.index_of(id).is_some());
            if self.model.is_some() || self.gantt.is_some() {
                // From a chart to a graph: its domains mean nothing here. Nor do a gantt's.
                (p.view.x, p.view.y, p.ease) = (None, None, None);
                (p.view.hidden, p.view.only) = (Default::default(), None);
                if self.gantt.is_some() && p.gesturing() {
                    p.gesture = Gesture::Swallowed;
                }
            }
            (p.layout, p.hits, p.hover, p.near, p.anchor) = (None, None, None, None, None);
            (p.target, p.matches) = (None, vec![]);
            (p.gantt, p.task) = (None, None);
            p.rehover = true;
        }
        // Laid out another way: the old layout's units, and the pins in them, mean nothing,
        // unless an undo put back a view of this layout, which lands with it.
        let relaid = self
            .graph
            .as_ref()
            .is_some_and(|g| g.layout != model.layout);
        let pending = std::mem::take(&mut self.pending);
        for (w, p) in self.panes.iter_mut() {
            if let Some((_, v)) = pending.iter().find(|(pw, _)| pw == w) {
                p.view = View {
                    reference: p.view.reference.clone(),
                    ..v.clone()
                };
                p.view.pinned.retain(|id, _| model.index_of(id).is_some());
                p.ease = None;
            } else if relaid {
                (p.view.x, p.view.y, p.ease) = (None, None, None);
                p.view.pinned.clear();
            }
        }
        // What `auto` picked, kept while the spec still says `auto` and no setting chooses.
        if model.spec.layout != Layout::Auto {
            self.auto_pick = None;
        } else if self.toggles.layout.is_none() {
            self.auto_pick = Some(model.layout.clone());
        }
        self.graph = Some(Arc::new(model));
        (self.model, self.gantt) = (None, None);
        self.model_gen += 1;
        self.rev += 1;
    }

    /// A gantt to draw: what names a task that's gone lets go (the cursor, the search's find,
    /// `]` and `[`'s anchor), names that are gone leave the hidden set and the one shown
    /// alone, and the rest stays as it was: the time shown and the rows' scroll, where the
    /// figure was a gantt already (an edit followed, a setting turned, a file polled). From a
    /// chart or a graph, their window and what they hid mean nothing here: the plan fits.
    fn new_gantt(&mut self, model: GanttModel) {
        let has = |c: &Option<Cursor>| match c {
            Some(c @ Cursor::Task(_)) => model.task_of(c).is_some(),
            Some(_) => false,
            None => true,
        };
        let keys = model.keys();
        let from_other = self.model.is_some() || self.graph.is_some();
        for p in self.panes.values_mut() {
            if !has(&p.cursor) {
                p.cursor = None;
            }
            if !has(&p.found_node) {
                p.found_node = None;
            }
            if from_other {
                (p.view.x, p.view.y, p.ease) = (None, None, None);
                (p.view.hidden, p.view.only) = (Default::default(), None);
                (p.calm, p.panel) = (None, None);
                if p.gesturing() {
                    p.gesture = Gesture::Swallowed;
                }
            }
            // A gantt has no reference, pins or follow: what a kept view said of them goes.
            (p.view.reference, p.view.follow) = (None, false);
            p.view.pinned.clear();
            p.view.hidden.retain(|k| keys.contains(k));
            if p.view.only.as_ref().is_some_and(|k| !keys.contains(k)) {
                p.view.only = None;
            }
            let gone = |k: &str| model.index_of(k).is_none();
            if p.anchor.as_ref().is_some_and(|(a, b)| gone(a) || gone(b)) {
                p.anchor = None;
            }
            (p.layout, p.hits, p.near) = (None, None, None);
            (p.graph, p.target, p.dwell, p.dwell_next) = (None, None, None, None);
            (p.hover, p.task, p.matches) = (None, None, vec![]);
            p.rehover = true;
        }
        self.auto_pick = None;
        self.gantt = Some(Arc::new(model));
        (self.model, self.graph) = (None, None);
        self.model_gen += 1;
        self.rev += 1;
    }

    /// An edit to the buffer it follows: charted once the buffer has been quiet a while, and
    /// only while a window shows the figure; otherwise when one does again.
    pub fn edited(&mut self, ctx: &mut Ctx) {
        if !self.shown(ctx) {
            self.behind = true;
            return;
        }
        let me = self.me.clone();
        self.debounce = Some(ctx.after(QUIET, move |ctx| {
            if let Some(s) = me.upgrade() {
                s.borrow_mut().build(ctx);
            }
            Ok(())
        }));
    }

    /// Whether it's the figure of this buffer: the one it follows, the one it was opened from,
    /// or the file it charts, which `buf` visits at `path`.
    pub fn is_of(&self, buf: BufferId, path: Option<&std::path::Path>) -> bool {
        self.source == Source::Buffer(buf)
            || self.from == Some(buf)
            || matches!((&self.file, path), (Some(f), Some(p)) if source::same_path(f, p))
    }

    /// Whether the figure follows this file's buffer: its own text, or a data file it reads.
    pub fn follows(&self, buf: BufferId, path: Option<&std::path::Path>) -> bool {
        if self.source == Source::Buffer(buf) {
            return true;
        }
        path.is_some_and(|p| self.followed.iter().any(|f| source::same_path(f, p)))
    }

    /// Whether it polls files on disk now.
    pub fn polling(&self) -> bool {
        self.poll.is_some()
    }

    /// The files it reads from disk.
    pub fn files(&self) -> Vec<PathBuf> {
        self.files.iter().map(|f| f.0.clone()).collect()
    }

    /// The poll of the files it reads from disk runs only while a window shows it.
    pub fn keep_polling(&mut self, ctx: &mut Ctx) {
        let wanted = self.shown(ctx) && !self.files.is_empty();
        if !wanted {
            self.poll = None;
            return;
        }
        if self.poll.is_some() {
            return;
        }
        let me = self.me.clone();
        self.poll = Some(ctx.every(POLL, move |ctx| {
            if let Some(s) = me.upgrade() {
                let mut st = s.borrow_mut();
                let changed = st.files.iter().any(|(p, t)| source::modified(p) != *t);
                if changed {
                    // Taken as seen, so a file that keeps failing isn't read every second.
                    for (p, t) in &mut st.files {
                        *t = source::modified(p);
                    }
                    st.build(ctx);
                }
            }
            Ok(())
        }));
    }

    // -------------------------------------------------------------- windows

    /// After a command, and anything else that may have changed what windows show: the panes of
    /// windows that are gone or show something else are freed (ruling 11), the poll runs only
    /// while shown, a figure behind its source is built when shown, and the theme is compared.
    /// The key card goes after any command but the one that put it up, whoever's it is (`:`,
    /// `C-w v`, `\z`): it shows until any key.
    pub fn settle(&mut self, ctx: &mut Ctx) {
        let Some(buf) = self.buf else { return };
        if !std::mem::take(&mut self.help_new) {
            for p in self.panes.values_mut() {
                p.help = false;
            }
        }
        let gone: Vec<WindowId> = (self.panes.keys())
            .filter(|w| ctx.window_buffer(**w) != Some(buf))
            .copied()
            .collect();
        for w in gone {
            if let Some(p) = self.panes.remove(&w)
                && self.panes.is_empty()
            {
                self.parked = Some((p.view, p.cursor, p.undo));
            }
            if self.last == Some(w) {
                self.last = self.panes.keys().next().copied();
            }
        }
        let shown = self.shown(ctx);
        if shown && self.behind {
            self.build(ctx);
        }
        self.keep_polling(ctx);
        self.live_sync(ctx);
        self.refresh(ctx);
    }

    /// A window showing it has an area this size: its pane is made the first time the cells
    /// have a size in pixels, with its two buffers, and grown when the area outgrows them.
    fn resized(&mut self, ctx: &mut Ctx, w: WindowId, cols: usize, rows: usize, cell: CellPixels) {
        if cell.width == 0 || cell.height == 0 || ctx.window_buffer(w) != self.buf {
            return;
        }
        if !ctx.pixels_supported() {
            if !self.refused {
                self.refused = true;
                ctx.messages.push(crate::NO_PIXELS.into());
            }
            return;
        }
        self.kit.borrow_mut().ensure(ctx);
        if !self.panes.contains_key(&w) {
            let seed = self.last.and_then(|l| self.panes.get(&l));
            // A dashboard's new window opens on the last window's view (its time, focus, full
            // size and each panel's own), as a chart's new window takes the last one's.
            if let Some(v) = seed.and_then(|p| p.dash.as_deref()).map(dash::keep_view)
                && let Some(d) = self.dash.as_mut()
            {
                d.seed = Some(v);
            }
            let pane = match (seed, self.parked.take()) {
                (Some(p), _) => Pane::new(p.view.clone(), p.cursor.clone(), Steps::default()),
                (None, Some((v, c, u))) => Pane::new(v, c, u),
                (None, None) => Pane::new(View::default(), None, Steps::default()),
            };
            self.panes.insert(w, pane);
        }
        self.last = Some(w);
        let Some(p) = self.panes.get_mut(&w) else {
            return;
        };
        if (p.cols, p.rows, p.cell) != (cols, rows, cell) {
            (p.cols, p.rows, p.cell) = (cols, rows, cell);
            p.drawn = None;
            p.over_key = None;
            // What the pointer was over is somewhere else now: the hover and its card go until
            // it moves again. The keyboard's cursor stays.
            (p.pointer, p.hovering) = (None, false);
            (p.hover, p.near, p.legend, p.strip) = (None, None, None, None);
            (p.target, p.task, p.row_hover) = (None, None, None);
            // A dashboard's panels let go of theirs too, until the pointer moves.
            if let Some(dp) = p.dash.as_mut() {
                dp.drop_hover();
            }
            // What the overlay drew at the old size stays in its buffer, which never shrinks,
            // where the new layout draws something else: the next draw clears it all.
            p.over_wipe |= p.over_bounds.is_some() || !p.over_items.is_empty();
            p.over_bounds = None;
            p.over_items.clear();
        }
        if let Err(e) = p.buffers_for(ctx)
            && !self.refused
        {
            self.refused = true;
            ctx.messages
                .push(format!("fig: no pixel buffer for the figure: {e}"));
        }
        if self.behind {
            self.build(ctx);
        }
        self.keep_polling(ctx);
    }

    // -------------------------------------------------------------- drawing

    /// Bring every shown pane's pictures up to date: the base where what it shows changed, the
    /// hover where the view moved under a still pointer, and the overlay where what it shows
    /// changed. The ease's frames run on a timer only while something eases.
    pub fn refresh(&mut self, ctx: &mut Ctx) {
        self.look(ctx);
        let now = ms(ctx.now());
        let Some(buf) = self.buf else { return };
        // A restart's panel opens in the first window drawn, for the cursor it kept.
        if self.panel_seed && (self.model.is_some() || self.graph.is_some() || self.gantt.is_some())
        {
            let ws: Vec<WindowId> = (self.panes.iter())
                .filter(|(_, p)| p.base.is_some() && p.cursor.is_some())
                .map(|(w, _)| *w)
                .collect();
            if let Some(w) = ws.first() {
                self.panel_seed = false;
                let _ = self.open_panel(*w, None);
            }
        }
        let log_errors = self.log_errors();
        let failure = Failure {
            failed: self.error.as_ref(),
            log: &log_errors,
            text: &self.text,
            name: &self.name,
        };
        let (model, graph, gantt) = (self.model.clone(), self.graph.clone(), self.gantt.clone());
        let shows = match (&model, &graph, &gantt) {
            (_, Some(g), _) => Shows::Graph(g),
            (_, None, Some(g)) => Shows::Gantt(g),
            (Some(m), None, None) => Shows::Chart(m),
            (None, None, None) => Shows::Nothing,
        };
        let src = panel::Source {
            chart: model.as_deref(),
            graph: graph.as_deref(),
            gantt: gantt.as_deref(),
            tile: None,
        };
        let me = self.me.clone();
        let model_gen = self.model_gen;
        let live = self.live_status(ctx.now());
        let mut kit = self.kit.borrow_mut();
        let mut easing = false;
        for (w, p) in &mut self.panes {
            if ctx.window_buffer(*w) != Some(buf) || p.base.is_none() {
                continue;
            }
            if p.log.is_none()
                && let Some(d) = self.dash.as_mut().filter(|d| d.model.is_some())
            {
                let gens = (self.theme_gen, self.fail_gen);
                let failed = self.error.as_ref().filter(|f| !f.is_waiting());
                easing |= dash::draw(p, d, live.clone(), gens, failed, &self.theme, &mut kit, now);
                continue;
            }
            if let Some(dp) = p.dash.as_mut() {
                dp.forget();
            }
            if let Some(e) = &p.ease
                && e.tween.done(now)
            {
                p.ease = None;
                p.rehover = true;
            }
            easing |= p.ease.is_some();
            p.sync_panel(src, model_gen);
            let shown = p.shown(now);
            let key = BaseKey {
                model: self.model_gen,
                view: View {
                    reference: None,
                    ..shown.clone()
                },
                size: p.area_px(),
                scale: p.scale().to_bits(),
                theme: self.theme_gen,
                fail: self.fail_gen,
                log: p.log,
                panel: p.panel.is_some() && p.log.is_none(),
            };
            let panel_key = panel::panel_key_of(p, self.theme_gen).filter(|_| p.log.is_none());
            if p.drawn.as_ref() != Some(&key) {
                draw_base(p, shows, &shown, &self.theme, &failure, &mut kit);
                p.drawn = Some(key);
                p.panel_drawn = panel_key;
                p.rehover = true;
            } else if p.panel_drawn != panel_key {
                panel::draw_panel_only(p, &self.theme, &mut kit);
                p.panel_drawn = panel_key;
            }
            if p.rehover {
                match shows {
                    Shows::Graph(g) => p.graph_hover(g),
                    Shows::Gantt(g) => p.gantt_hover(g),
                    _ => p.hover_again(model.as_deref()),
                }
            }
            if let Shows::Graph(g) = shows {
                p.dwell_on(g, *w, &me, ctx);
            }
            draw_overlay(p, shows, &shown, &self.theme, &mut kit);
        }
        drop(kit);
        match (easing, self.motion.is_some()) {
            (true, false) => {
                let me = self.me.clone();
                self.motion = Some(ctx.every(FRAME, move |ctx| {
                    if let Some(s) = me.upgrade() {
                        s.borrow_mut().refresh(ctx);
                    }
                    Ok(())
                }));
            }
            (false, true) => self.motion = None,
            _ => {}
        }
    }

    // -------------------------------------------------------------- the pointer

    /// Whether this press is the one that activated the app: the focus came within
    /// [`FOCUS_GRACE_MS`] with no press having brought it. Once.
    fn activating(&mut self, now: Duration) -> bool {
        self.focused_at
            .take()
            .is_some_and(|at| now < at + Duration::from_millis(FOCUS_GRACE_MS))
    }

    /// A pointer event over (or dragging from) a window's area.
    fn pointer(&mut self, ctx: &mut Ctx, p: &SurfacePointer) {
        let now = ctx.now();
        let press = matches!(p.kind, PointerKind::Press(Button::Left | Button::Middle));
        let swallow = press && (!self.focused || self.activating(now));
        let model = self.model.clone();
        let (toggles, motion) = (self.toggles, self.motion());
        let me = self.me.clone();
        let buf = self.buf;
        if p.kind != PointerKind::Move {
            // What a restart keeps may have changed: the view, the cursor.
            self.rev += 1;
        }
        let Some(pane) = self.panes.get_mut(&p.window) else {
            return;
        };
        let s = pane.scale();
        let at = (p.x / s, p.y / s);
        pane.pointer = Some(at);
        if matches!(p.kind, PointerKind::Drag(_) | PointerKind::Release(_)) {
            pane.drag_mods = p.mods;
        }
        let dashboard = self.dash_drawn(p.window);
        let Some(pane) = self.panes.get_mut(&p.window) else {
            return;
        };
        let drawn = model.is_some() || self.graph.is_some() || self.gantt.is_some() || dashboard;
        if pane.log.is_none() && drawn {
            // The panel takes what's over it, then a graph's gestures, else a chart's.
            if self.panel_pointer(ctx, p, at, swallow) {
                return;
            }
            // A press in the figure takes the keys back from the panel, as one on the panel
            // gives them to it.
            if press
                && !swallow
                && let Some(panel) = self.panes.get_mut(&p.window).and_then(|p| p.panel.as_mut())
                && panel.state.focused
            {
                panel.state.focused = false;
                panel.rev += 1;
            }
            if dashboard {
                return self.dash_pointer(ctx, p, at, swallow);
            }
            if self.graph.is_some() {
                return self.graph_pointer(ctx, p, at, swallow);
            }
            if self.gantt.is_some() {
                return self.gantt_pointer(ctx, p, at, swallow);
            }
        }
        let Some(pane) = self.panes.get_mut(&p.window) else {
            return;
        };
        let Some(model) = model.filter(|_| pane.log.is_none()) else {
            // The error figure, or its log: a click turns it over, an entry under the pointer
            // is marked, a double click goes to it, and the wheel scrolls the log.
            let lines = (pane.log.is_some()).then(|| self.log_layout());
            let Some(pane) = self.panes.get_mut(&p.window) else {
                return;
            };
            match on_error(pane, p, at, swallow, lines.as_ref()) {
                ErrorAct::None => {}
                ErrorAct::Goto(entry) => {
                    let args = die_core::Args {
                        window: Some(p.window),
                        buffer: buf,
                        count: entry.map(|e| e + 1),
                        ..die_core::Args::default()
                    };
                    ctx.defer(move |ctx| ctx.run("fig-goto-error", &args));
                }
            }
            return;
        };
        let mut cx = Act {
            pane,
            model: &model,
            toggles,
            motion,
            now,
        };
        let w = p.window;
        cx.pointer(p, at, swallow, || {
            let me = me.clone();
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

    /// The scroll burst in a window ends: one undo step for what it did.
    pub fn end_burst(&mut self, w: WindowId) {
        if self.dash_end_burst(w) {
            self.rev += 1;
            return;
        }
        let toggles = self.toggles;
        if let Some(p) = self.panes.get_mut(&w)
            && let Some(b) = p.burst.take()
            && b.before != p.view
        {
            p.undo.push(Snap {
                view: b.before,
                toggles,
            });
            self.rev += 1;
        }
    }

    /// The focus came or went. Gained with no press having brought it (the press that focuses
    /// a window from another of DIE's comes first): the app was activated, and the press that
    /// did it is on its way, to do nothing. Lost mid-gesture: the gesture ends where it stands.
    fn focus(&mut self, ctx: &mut Ctx, gained: bool) {
        let now = ctx.now();
        // Any press under way, the swallowed one that focused a window from another of DIE's
        // among them, as the whiteboard counts it: that press brought the focus.
        let gesturing = (self.panes.values()).any(|p| !matches!(p.gesture, Gesture::None));
        self.focused_at = (gained && !self.focused && !gesturing).then_some(now);
        self.focused = gained;
        if !gained {
            let ws: Vec<WindowId> = self.panes.keys().copied().collect();
            for w in ws {
                self.finish(w, now);
            }
        }
    }

    /// End whatever the pointer is in the middle of in a window, as a key or the focus going
    /// does (mouse-ux §8): a brush's preview goes, a pan stays as one undo step, a scroll burst
    /// ends.
    pub fn finish(&mut self, w: WindowId, now: Duration) {
        self.end_burst(w);
        if let Some(p) = self.panes.get_mut(&w)
            && std::mem::take(&mut p.panel_press)
        {
            (p.gesture, p.held) = (Gesture::Swallowed, None);
            return;
        }
        if self.dash_drawn(w) {
            self.dash_finish(w, now);
            return;
        }
        if self.graph.is_some() {
            self.graph_act(w, now, |g| g.finish());
            return;
        }
        if self.gantt.is_some() {
            self.gantt_act(w, now, |g| g.finish());
            return;
        }
        let (Some(model), toggles, motion) = (self.model.clone(), self.toggles, self.motion())
        else {
            return;
        };
        if let Some(pane) = self.panes.get_mut(&w) {
            Act {
                pane,
                model: &model,
                toggles,
                motion,
                now,
            }
            .finish();
        }
    }

    /// `Esc` in a window (mouse-ux §8): a brush is dropped and its release does nothing; a pan
    /// goes back where it started, and a node's drag eases back (mouse-ux §10); a scroll burst
    /// ends. With nothing under way, the first of the key card, the search's emphasis, the
    /// detail panel, the reference and (on a graph) the cursor's emphasis goes, the graph at
    /// rest again with the cursor's ring still on its node (fig-port §9.1-2). False if there
    /// was nothing.
    pub fn cancel(&mut self, w: WindowId, now: Duration) -> bool {
        self.end_burst(w);
        if self.dash_drawn(w) {
            let Some(p) = self.panes.get_mut(&w) else {
                return false;
            };
            let held = p.dash.as_ref().is_some_and(|d| d.gesturing());
            let done = if held {
                return self.dash_cancel(w);
            } else if p.help {
                p.help = false;
                true
            } else if p.panel.take().is_some() {
                p.row_hover = None;
                true
            } else {
                return self.dash_cancel(w) && {
                    self.rev += 1;
                    true
                };
            };
            self.rev += 1;
            return done;
        }
        let graph = self.graph.is_some();
        if let Some(p) = self.panes.get_mut(&w)
            && p.log.is_none()
            && let Gesture::NodeDrag { before, .. } = &p.gesture
        {
            let before = before.clone();
            p.gesture = Gesture::Swallowed;
            self.graph_act(w, now, |g| {
                g.set(before, Change::Follow);
            });
            if let Some(p) = self.panes.get_mut(&w) {
                p.rehover = true;
            }
            self.rev += 1;
            return true;
        }
        let Some(p) = self.panes.get_mut(&w) else {
            return false;
        };
        // The log goes first: the figure again.
        if p.log.take().is_some() {
            p.rehover = true;
            self.rev += 1;
            return true;
        }
        match std::mem::replace(&mut p.gesture, Gesture::None) {
            Gesture::Brush { .. } => (p.gesture, p.frozen) = (Gesture::Swallowed, None),
            Gesture::Pan { before, .. }
            | Gesture::AxisPan { before, .. }
            | Gesture::NodeDrag { before, .. } => {
                p.view = before;
                p.ease = None;
                p.gesture = Gesture::Swallowed;
            }
            Gesture::Pressed { .. } => p.gesture = Gesture::Swallowed,
            Gesture::None | Gesture::Swallowed => {
                let searched = p.found.is_some()
                    || p.found_band.is_some()
                    || p.found_node.is_some()
                    || !p.matches.is_empty()
                    || !p.lit.is_empty()
                    || !p.lit_bands.is_empty();
                if p.help {
                    p.help = false;
                } else if searched {
                    (p.found, p.found_band, p.found_node) = (None, None, None);
                    (p.matches, p.lit, p.lit_bands) = (vec![], vec![], vec![]);
                } else if p.panel.is_some() {
                    p.panel = None;
                    p.row_hover = None;
                } else if p.view.reference.is_some() {
                    p.view.reference = None;
                } else if graph && p.cursor.is_some() && p.calm != p.cursor {
                    p.calm = p.cursor.clone();
                } else {
                    return false;
                }
            }
        }
        p.rehover = true;
        self.rev += 1;
        true
    }

    /// The pointer left a window's area: its hover, card and emphasis go.
    fn left(&mut self, w: WindowId) {
        self.dash_left(w);
        if let Some(p) = self.panes.get_mut(&w) {
            p.pointer = None;
            p.hovering = false;
            p.row_hover = None;
            p.rehover = true;
        }
    }

    /// A key no keymap took: with the key card up, any key closes it and is taken for that;
    /// otherwise it isn't the figure's, Space included, which does nothing here.
    fn key(&mut self, k: &KeyEvent) -> bool {
        let open = self.panes.values().any(|p| p.help);
        if k.kind == KeyKind::Press && open {
            for p in self.panes.values_mut() {
                p.help = false;
            }
            return true;
        }
        false
    }

    /// Act on a window's pane with the model, as the keys and the pointer do. None without a
    /// model or a pane.
    pub fn act<R>(
        &mut self,
        w: WindowId,
        now: Duration,
        f: impl FnOnce(&mut Act) -> R,
    ) -> Option<R> {
        let model = self.model.clone()?;
        let (toggles, motion) = (self.toggles, self.motion());
        let pane = self.panes.get_mut(&w)?;
        let mut a = Act {
            pane,
            model: &model,
            toggles,
            motion,
            now,
        };
        let r = f(&mut a);
        self.rev += 1;
        Some(r)
    }

    /// `u` or `C-r` in a window: the view (and the settings) of the change before or after,
    /// eased; the reference stays as it is, being no part of undo (mouse-ux §9). False with
    /// nothing to undo or redo.
    pub fn undo(&mut self, ctx: &mut Ctx, w: WindowId, redo: bool) -> bool {
        let now = ctx.now();
        if self.dash_drawn(w) {
            return self.dash_undo(w, now, redo);
        }
        let toggles = self.toggles;
        let step = |pane: &mut Pane| {
            let current = Snap {
                view: pane.view.clone(),
                toggles,
            };
            let snap = match redo {
                true => pane.undo.redo(&current),
                false => pane.undo.undo(&current),
            }?;
            let v = View {
                reference: pane.view.reference.clone(),
                ..snap.view.clone()
            };
            Some((snap, v))
        };
        if self.graph.is_some() {
            let Some((snap, v)) = self.panes.get_mut(&w).and_then(step) else {
                return false;
            };
            self.rev += 1;
            if snap.toggles.layout != self.toggles.layout {
                // The layout goes back first; the view, its pins in that layout's units, lands
                // with it.
                self.pending = vec![(w, v)];
            } else {
                self.graph_act(w, now, |g| g.set(v, Change::Follow));
            }
            if snap.toggles != self.toggles {
                self.toggles = snap.toggles;
                self.rebuild(ctx);
            }
            return true;
        }
        if self.gantt.is_some() {
            let Some((snap, v)) = self.panes.get_mut(&w).and_then(step) else {
                return false;
            };
            self.rev += 1;
            self.gantt_act(w, now, |g| {
                // A name shown again may bring the cursor's task back; one hidden takes it.
                g.set(v, Change::Follow);
                let gone = (g.pane.cursor.as_ref())
                    .and_then(|c| g.model.task_of(c))
                    .is_some_and(|i| !g.model.visible(&g.pane.view, i));
                if gone {
                    g.pane.cursor = None;
                }
            });
            if snap.toggles != self.toggles {
                self.toggles = snap.toggles;
                self.rebuild(ctx);
            }
            return true;
        }
        let Some(snap) = self
            .act(w, now, |a| {
                let current = Snap {
                    view: a.pane.view.clone(),
                    toggles,
                };
                let snap = match redo {
                    true => a.pane.undo.redo(&current),
                    false => a.pane.undo.undo(&current),
                }?;
                let v = View {
                    reference: a.pane.view.reference.clone(),
                    ..snap.view.clone()
                };
                a.set(v, Change::Follow);
                Some(snap)
            })
            .flatten()
        else {
            return false;
        };
        if snap.toggles != self.toggles {
            self.toggles = snap.toggles;
            self.rebuild(ctx);
        }
        true
    }

    /// A graph's settings turned, one undo step, the graph laid out again: `labels` on or off,
    /// or the layout `s` picked.
    pub fn turn_graph(
        &mut self,
        ctx: &mut Ctx,
        w: WindowId,
        f: impl FnOnce(&mut Toggles, &GraphSpec),
    ) {
        let Some(spec) = self.graph_spec().cloned() else {
            return;
        };
        let before = self.toggles;
        f(&mut self.toggles, &spec);
        if self.toggles == before {
            return;
        }
        if let Some(p) = self.panes.get_mut(&w) {
            p.undo.push(Snap {
                view: p.view.clone(),
                toggles: before,
            });
        }
        self.rev += 1;
        self.rebuild(ctx);
    }

    /// A gantt's `s`: a setting turned over (its grid, axis, links or today line), one undo
    /// step, the model built again from the table in hand.
    pub fn flip_gantt(
        &mut self,
        ctx: &mut Ctx,
        w: WindowId,
        setting: crate::interact::GanttSetting,
    ) {
        let Some(spec) = self.gantt_spec().cloned() else {
            return;
        };
        let before = self.toggles;
        self.toggles.flip_gantt(&spec, setting);
        if let Some(p) = self.panes.get_mut(&w) {
            p.undo.push(Snap {
                view: p.view.clone(),
                toggles: before,
            });
        }
        self.rev += 1;
        self.rebuild(ctx);
    }

    /// `s`'s answer: a setting turned over, one undo step, the model built again.
    pub fn flip(&mut self, ctx: &mut Ctx, w: WindowId, setting: crate::interact::Setting) {
        let Some(chart) = &self.chart else { return };
        let before = self.toggles;
        self.toggles.flip(chart, setting);
        if let Some(p) = self.panes.get_mut(&w) {
            p.undo.push(Snap {
                view: p.view.clone(),
                toggles: before,
            });
        }
        self.rev += 1;
        self.rebuild(ctx);
    }
}

/// What a pointer event on the error figure or its log asks of the figure.
enum ErrorAct {
    /// Nothing more than the pane's change.
    None,
    /// Go to an entry's place (the first error's with None).
    Goto(Option<usize>),
}

/// A pointer event on the error figure (symbolic, or its log over the figure): a click on the
/// symbolic figure, the log's header or the badge turns it over; a press on an entry marks it
/// and a double click goes to its place; the pointer over an entry marks it; the wheel scrolls
/// the log by lines, the rest carried. `lines` is the log's layout while it's up.
fn on_error(
    pane: &mut Pane,
    p: &SurfacePointer,
    at: (f32, f32),
    swallow: bool,
    lines: Option<&plot::LogLayout>,
) -> ErrorAct {
    let hit = pane.hit_at(at);
    match p.kind {
        PointerKind::Move => {
            if let (Some((top, _)), Hit::LogEntry(i)) = (pane.log, &hit) {
                pane.log = Some((top, *i as usize));
            }
            pane.hovering = true;
            ErrorAct::None
        }
        PointerKind::Press(Button::Left) if !swallow => {
            pane.held = Some(Button::Left);
            let before = std::mem::take(&mut pane.log_pressed);
            if let (Some((top, _)), Hit::LogEntry(i)) = (pane.log, &hit) {
                pane.log = Some((top, *i as usize));
                pane.log_pressed = Some(*i);
                if p.clicks >= 2 && before == Some(*i) {
                    pane.gesture = Gesture::Swallowed;
                    return ErrorAct::Goto(Some(*i as usize));
                }
            }
            pane.gesture = Gesture::Pressed {
                at,
                hit,
                button: Button::Left,
                mods: p.mods,
                clicks: p.clicks,
            };
            ErrorAct::None
        }
        PointerKind::Press(Button::Left) => {
            // The press that focuses only focuses, but it's a double click's first all the same.
            pane.held = Some(Button::Left);
            pane.gesture = Gesture::Swallowed;
            pane.log_pressed = match (pane.log, &hit) {
                (Some(_), Hit::LogEntry(i)) => Some(*i),
                _ => None,
            };
            ErrorAct::None
        }
        PointerKind::Release(Button::Left) => {
            pane.held = None;
            let g = std::mem::replace(&mut pane.gesture, Gesture::None);
            if let Gesture::Pressed {
                hit: pressed,
                at: from,
                ..
            } = g
                && (from.0 - at.0).hypot(from.1 - at.1) < CLICK_SLOP
                && matches!(pressed, Hit::ErrorFigure | Hit::Badge)
                && pane.hit_at(at) == pressed
            {
                pane.log = match pane.log {
                    Some(_) => None,
                    None => Some((0, 0)),
                };
                (pane.log_carry, pane.log_pressed) = (0.0, None);
            }
            ErrorAct::None
        }
        PointerKind::Scroll { dy, .. } => {
            if let (Some((top, mark)), Some(lines)) = (pane.log, lines)
                && dy.is_finite()
            {
                let cell = pane.cell.height as f32 / pane.scale();
                let by = dy * cell / plot::errors::LOG_LINE + pane.log_carry;
                let whole = by.trunc();
                pane.log_carry = by - whole;
                let page = plot::log_page(pane.size());
                let top = (top as f32 + whole).clamp(0.0, lines.max_top(page) as f32) as usize;
                pane.log = Some((top, mark));
            }
            ErrorAct::None
        }
        _ => ErrorAct::None,
    }
}

/// A pane with what acting on it needs.
pub struct Act<'a> {
    /// The pane.
    pub pane: &'a mut Pane,
    /// The figure's model.
    pub model: &'a ChartModel,
    /// The figure's settings.
    pub toggles: Toggles,
    /// Whether changes ease.
    pub motion: bool,
    /// The clock.
    pub now: Duration,
}

impl Act<'_> {
    /// Change the view, as `how` says.
    pub fn set(&mut self, v: View, how: Change) -> bool {
        self.pane
            .set_view(v, how, self.toggles, self.model, self.motion, self.now)
    }

    /// The layout, once drawn.
    pub fn layout(&self) -> Option<&ChartLayout> {
        self.pane.layout.as_ref()
    }

    /// The view follows live data's end when its right edge is within a pixel of the newest
    /// sample, and stops when a change leaves the newest off screen (mouse-ux §3.2). A view
    /// that fits the data follows it anyway.
    pub fn follow_check(&mut self) {
        let (Some((a, b)), Some(n), Some(l)) = (
            self.pane.view.x,
            self.model.newest(),
            self.pane.layout.as_ref(),
        ) else {
            self.pane.view.follow = false;
            return;
        };
        let px = (b - a) / f64::from(l.plot.w.max(1.0));
        self.pane.view.follow = (n - b).abs() <= px;
    }

    /// A pointer event on the chart at `at` (logical px in its area), as its window has it, or
    /// a dashboard's panel: hover needs no click, a press is held by its button (a second
    /// button mid-drag ends nothing and starts nothing; the same again ends what its lost
    /// release left), the focusing press (`swallow`) only focuses, a double click acts at
    /// once, and the wheel's burst ends on the timer `arm` sets.
    pub fn pointer(
        &mut self,
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
        arm: impl FnOnce() -> Sub,
    ) {
        match p.kind {
            PointerKind::Move => {
                self.pane.hovering = true;
                self.pane.rehover = true;
            }
            PointerKind::Press(b @ (Button::Left | Button::Middle)) => {
                self.pane.help = false;
                if self.pane.gesturing() && self.pane.held != Some(b) {
                    // A second button mid-drag: it ends nothing and starts nothing.
                    return;
                }
                if self.pane.gesturing() {
                    // The same button again: its release was lost. What it did stays.
                    self.finish();
                }
                self.pane.held = Some(b);
                (self.pane.frozen, self.pane.drag_mods) = (None, p.mods);
                if swallow {
                    self.pane.gesture = Gesture::Swallowed;
                    return;
                }
                let hit = self.pane.hit_at(at);
                if p.clicks >= 2 && b == Button::Left {
                    self.double(&hit);
                    self.pane.gesture = Gesture::Swallowed;
                    return;
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
            // The right button opens nothing: there is no dial.
            PointerKind::Press(Button::Right) => {}
            PointerKind::Drag(_) => self.drag(at),
            PointerKind::Release(b) => self.release(b, at),
            PointerKind::Scroll { dx, dy } => self.scroll(dx, dy, at, p.mods, arm),
        }
    }

    fn finish(&mut self) {
        match std::mem::replace(&mut self.pane.gesture, Gesture::None) {
            Gesture::Brush { .. } | Gesture::Pressed { .. } => {
                (self.pane.gesture, self.pane.frozen) = (Gesture::Swallowed, None);
            }
            Gesture::Pan { before, .. } | Gesture::AxisPan { before, .. } => {
                if before != self.pane.view {
                    self.pane.undo.push(Snap {
                        view: before,
                        toggles: self.toggles,
                    });
                }
                self.follow_check();
                self.pane.gesture = Gesture::Swallowed;
            }
            other => self.pane.gesture = other,
        }
        self.pane.rehover = true;
    }

    /// A point held inside the plot.
    fn in_plot(&self, (x, y): (f32, f32)) -> (f32, f32) {
        match self.layout() {
            Some(l) => (
                x.clamp(l.plot.x, l.plot.right()),
                y.clamp(l.plot.y, l.plot.bottom()),
            ),
            None => (x, y),
        }
    }

    fn drag(&mut self, at: (f32, f32)) {
        if let Gesture::Pressed {
            at: from,
            hit,
            button,
            mods,
            ..
        } = &self.pane.gesture
        {
            let (dx, dy) = (at.0 - from.0, at.1 - from.1);
            if (dx * dx + dy * dy).sqrt() < CLICK_SLOP {
                return;
            }
            let (from, hit, button, mods) = (*from, hit.clone(), *button, *mods);
            self.start(from, hit, button, mods);
        }
        let held = self.in_plot(at);
        match &mut self.pane.gesture {
            Gesture::Brush { to, .. } => *to = held,
            Gesture::Pan { .. } => self.pan_by(at, None),
            Gesture::AxisPan { axis, .. } => {
                let axis = *axis;
                self.pan_by(at, Some(axis));
            }
            _ => {}
        }
    }

    /// A press that travelled the slop becomes the gesture its button, modifiers and target
    /// make it (mouse-ux §13 row 1).
    fn start(&mut self, from: (f32, f32), hit: Hit, button: Button, mods: Mods) {
        self.pane.settle(ms(self.now));
        let before = self.pane.view.clone();
        self.pane.drag_last = from;
        let strip = strip_axis(&hit, self.model.horizontal);
        self.pane.gesture = match hit {
            _ if button == Button::Middle || mods.alt => Gesture::Pan {
                panel: 0,
                from,
                before,
            },
            Hit::AxisX(panel) | Hit::AxisY(panel) if strip.is_some() => Gesture::AxisPan {
                panel,
                axis: strip.unwrap_or(Axis::X),
                from,
                before,
            },
            Hit::Plot(panel) | Hit::Bar { panel, .. } | Hit::Cell { panel, .. } => {
                let kind = match plot::nav::brush_kind(self.model) {
                    // ⌘-drag on an x-chart: a box, which lets y leave the fit.
                    BrushKind::X if mods.super_ => BrushKind::Box,
                    kind => kind,
                };
                let frozen = self.freeze(kind);
                if let Some(f) = &frozen {
                    // The same domains spelled out: the base as drawn shows them already.
                    let strip = |v: &View| View {
                        reference: None,
                        ..v.clone()
                    };
                    let now = strip(&self.pane.view);
                    if let Some(d) = self.pane.drawn.as_mut().filter(|d| d.view == now) {
                        d.view = strip(f);
                    }
                }
                self.pane.frozen = frozen;
                let from = self.in_plot(from);
                Gesture::Brush {
                    panel,
                    kind,
                    from,
                    to: from,
                }
            }
            _ => Gesture::Swallowed,
        };
        self.pane.rehover = true;
    }

    /// The view a brush draws over, its domains as the base shows them now (mouse-ux §3.7):
    /// x, and y too for a brush that reads it. Live data still draws, but nothing slides under
    /// the hand, and the release zooms to what was brushed. None before the base is drawn.
    fn freeze(&self, kind: BrushKind) -> Option<View> {
        let l = self.pane.layout.as_ref()?;
        let v = &self.pane.view;
        let reads_y = kind != BrushKind::X && kind != BrushKind::Bands;
        Some(View {
            x: v.x.or(Some(l.domain.0)),
            y: v.y.or(reads_y.then_some(l.domain.1)),
            ..v.clone()
        })
    }

    /// Whether the chart moves along one axis alone: an x-chart or a band chart whose value
    /// axis fits the data. Once y is set by hand, it pans in two dimensions (mouse-ux §4.1).
    fn one_axis(&self) -> bool {
        (self.model.is_x_chart() || self.model.is_band()) && self.pane.view.y.is_none()
    }

    /// A pan follows the hand from the last sample, by whole pixels as they came.
    fn pan_by(&mut self, at: (f32, f32), axis: Option<Axis>) {
        let (dx, dy) = (at.0 - self.pane.drag_last.0, at.1 - self.pane.drag_last.1);
        self.pane.drag_last = at;
        let Some(l) = self.pane.layout.as_ref() else {
            return;
        };
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        // Along one axis, a drag pans by its motion along that axis alone: across the screen,
        // or down it for bars across.
        let (dx, dy) = match (axis.is_none() && self.one_axis(), l.horizontal) {
            (true, false) => (dx, 0.0),
            (true, true) => (0.0, dy),
            (false, _) => (dx, dy),
        };
        let v = plot::nav::pan(self.model, l, &self.pane.view, dx, dy, axis);
        self.set(v, Change::Live);
    }

    /// A release: of the gesture's button, the gesture's end; of another, nothing.
    fn release(&mut self, b: Button, at: (f32, f32)) {
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
            g @ Gesture::Brush { .. } => {
                self.pane.gesture = g;
                let shape = self.pane.brush_shape();
                self.pane.gesture = Gesture::None;
                let v = shape.and_then(|(kind, from, to)| {
                    let to = self.in_plot(to);
                    let l = self.layout()?;
                    plot::nav::brush(self.model, l, &self.pane.view, kind, from, to)
                });
                // The ease starts from the domain held still, which goes after it.
                if let Some(v) = v {
                    self.set(v, Change::Step);
                    self.follow_check();
                }
                self.pane.frozen = None;
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

    /// A click, on its release: by what's under it (mouse-ux §4). On the stale badge, the log
    /// comes up over the figure.
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
            Hit::AxisX(_) | Hit::AxisY(_) => {
                // A category's label: the cursor to its band (mouse-ux §4.9).
                let c =
                    (self.pane.layout.as_ref()).and_then(|l| category_at(self.model, l, &hit, at));
                if c.is_some() {
                    self.pane.cursor = c;
                }
            }
            Hit::Plot(_) | Hit::Bar { .. } | Hit::Cell { .. } => {
                let (Some(l), Some(h)) = (self.pane.layout.as_ref(), self.pane.hits.as_ref())
                else {
                    return;
                };
                let c = plot::hover::hover(self.model, l, h, &self.pane.view, at.0, at.1);
                let near =
                    plot::hover::nearest_series(self.model, l, h, &self.pane.view, at.0, at.1);
                if mods.shift {
                    // The reference, A: set here, moved here, or cleared on itself, which on an
                    // x-chart is its moment, one rule across every series (mouse-ux §6). Not
                    // an undo step (mouse-ux §9).
                    let same = match (&self.pane.view.reference, &c) {
                        (Some(r), Some(c)) => same_moment(self.model, r, c),
                        (r, c) => r == c,
                    };
                    self.pane.view.reference = if same { None } else { c };
                } else if mods.alt {
                    let key = near.or(match c {
                        Some(Cursor::Point { series, .. }) => Some(series),
                        Some(Cursor::Band { series, .. }) => series,
                        _ => None,
                    });
                    if let Some(k) = key {
                        self.solo(&k);
                    }
                } else if c.is_some() {
                    self.pane.cursor = c;
                }
                self.pane.last_click = None;
            }
            Hit::Badge => {
                (self.pane.log, self.pane.log_pressed) = (Some((0, 0)), None);
                (self.pane.hover, self.pane.near) = (None, None);
            }
            _ => {}
        }
    }

    /// A double click's second press, at once: on a legend key, only that series (the first
    /// click's toggle taken back, so the two are one undo step); on an axis, that axis fits
    /// again; on the plot, the whole view does.
    fn double(&mut self, hit: &Hit) {
        match hit {
            Hit::LegendKey(k) => {
                let mut v = self.pane.view.clone();
                if self.pane.last_click.as_ref() == Some(k)
                    && let Some(before) = self.pane.undo.retract()
                {
                    v = before.view;
                    self.pane.view = v.clone();
                }
                v.only = Some(k.clone());
                v.hidden.remove(k);
                self.set(v, Change::Step);
            }
            Hit::AxisX(_) | Hit::AxisY(_) => {
                let axis = strip_axis(hit, self.model.horizontal);
                self.reset(axis.or(Some(Axis::X)));
            }
            Hit::Plot(_) | Hit::Bar { .. } | Hit::Cell { .. } => self.reset(None),
            _ => {}
        }
        self.pane.last_click = None;
    }

    /// Fit one axis or both, eased, one undo step.
    pub fn reset(&mut self, axis: Option<Axis>) {
        let mut v = plot::nav::reset(self.model, &self.pane.view, axis);
        if v.x.is_none() {
            v.follow = false;
        }
        self.set(v, Change::Step);
    }

    /// Only `k`, or every series again if it's shown alone.
    pub fn solo(&mut self, k: &SeriesKey) {
        let mut v = self.pane.view.clone();
        if v.only.as_ref() == Some(k) {
            v.only = None;
        } else {
            v.only = Some(k.clone());
            v.hidden.remove(k);
        }
        self.set(v, Change::Step);
    }

    /// `k` hidden, or shown again. A series shown alone becomes the rest hidden first.
    pub fn toggle_hidden(&mut self, k: &SeriesKey) {
        let mut v = self.pane.view.clone();
        if let Some(only) = v.only.take() {
            v.hidden = self
                .model
                .keys()
                .into_iter()
                .filter(|s| *s != only)
                .collect();
        }
        if !v.hidden.remove(k) {
            v.hidden.insert(k.clone());
        }
        self.set(v, Change::Step);
    }

    /// The wheel (mouse-ux §3.2): a burst keeps the meaning of its first event; plain pans by
    /// whole physical pixels with the rest carried, ⌘ (or Ctrl) zooms about the pointer, and
    /// over an axis's strip either acts on that axis alone. `arm` re-arms the burst's end.
    fn scroll(&mut self, dx: f32, dy: f32, at: (f32, f32), mods: Mods, arm: impl FnOnce() -> Sub) {
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
            let axis = strip_axis(&self.pane.hit_at(at), self.model.horizontal);
            // Along one axis (an axis's strip, an x-chart or bars whose values fit), the
            // larger of the first event's components moves it, so a mouse's wheel works.
            let across = (axis.is_some() || self.one_axis()).then(|| dx.abs() >= dy.abs());
            self.pane.burst = Some(Burst {
                zoom: mods.super_ || mods.ctrl,
                axis,
                across,
                panel: false,
                before: self.pane.view.clone(),
                _timer: timer,
            });
            self.pane.carry = (0.0, 0.0);
        } else if let Some(b) = &mut self.pane.burst {
            b._timer = timer;
        }
        let Some(b) = &self.pane.burst else { return };
        let (zoom, axis, across) = (b.zoom, b.axis, b.across);
        let Some(l) = self.pane.layout.as_ref() else {
            return;
        };
        let v = if zoom {
            let factor = ZOOM_STEP.powf(f64::from(-dy) / 4.0);
            if factor == 1.0 {
                return;
            }
            plot::nav::zoom(self.model, l, &self.pane.view, factor, Some(at), None, axis)
        } else {
            let (cw, ch) = (self.pane.cell.width as f32, self.pane.cell.height as f32);
            let s = self.pane.scale();
            let (tx, ty) = (dx * cw + self.pane.carry.0, dy * ch + self.pane.carry.1);
            let (wx, wy) = (tx.trunc(), ty.trunc());
            self.pane.carry = (tx - wx, ty - wy);
            let (px, py) = (wx / s, wy / s);
            // The wheel says how the view moves, right and down positive; a pan, how the
            // content does. Along one axis, right and down move x later (mouse-ux §3.2) and
            // y lower, named to the pan so its reading of a free pan can't turn them about.
            let ((dx, dy), axis) = match across {
                None => ((-px, -py), axis),
                Some(across) => {
                    let v = if across { px } else { py };
                    let a = axis.unwrap_or(Axis::X);
                    let v = if a == Axis::Y { -v } else { v };
                    (along(a, v, l.horizontal), Some(a))
                }
            };
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            plot::nav::pan(self.model, l, &self.pane.view, dx, dy, axis)
        };
        if self.set(v, Change::Live) {
            self.follow_check();
        }
    }
}

/// The keys' actions on a pane (the design's Controls).
impl Act<'_> {
    /// `h l j k w b gg G H M L`: the cursor moved as `action` says, `count` times, the view
    /// following it as a window follows the text cursor; the hover hides until the pointer
    /// moves. `G` on live data follows the newest too. False if it didn't move.
    ///
    /// `H M L` land in the view by what they are, and so does the cursor's first place by `h l
    /// j k`, at the view's edge or middle: the view stays as you saw it, with no scroll to put
    /// them a tenth inside.
    pub fn move_cursor(&mut self, action: &crate::interact::Action, count: u32) -> bool {
        use crate::interact::Action;
        let Some(l) = self.pane.layout.as_ref() else {
            return false;
        };
        let cursor = self.pane.cursor.as_ref();
        let placed = cursor.is_none() && matches!(action, Action::Step(_) | Action::Series(_));
        let Some(c) = plot::nav::motion(self.model, l, &self.pane.view, cursor, action, count)
        else {
            return false;
        };
        let v = match placed || matches!(action, Action::ViewEdge(_)) {
            true => self.pane.view.clone(),
            false => plot::nav::reveal(self.model, l, &self.pane.view, &c),
        };
        self.pane.cursor = Some(c);
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
        if *action == crate::interact::Action::Last {
            self.follow_newest();
        }
        true
    }

    /// The view's right edge on the newest sample, following it from now on (`G`).
    fn follow_newest(&mut self) {
        if let (Some((a, b)), Some(n)) = (self.pane.view.x, self.model.newest()) {
            let v = View {
                x: Some((n - (b - a), n)),
                follow: true,
                ..self.pane.view.clone()
            };
            self.set(v, Change::Follow);
        }
    }

    /// `+ - K J`: zoom by `factor` about the cursor (or the middle), eased; a run of them within
    /// a scroll burst is one undo step.
    pub fn zoom_keys(&mut self, factor: f64) {
        let Some(l) = self.pane.layout.as_ref() else {
            return;
        };
        let cursor = self.pane.cursor.as_ref();
        let v = plot::nav::zoom(self.model, l, &self.pane.view, factor, None, cursor, None);
        if self.set(v, Change::Keyed) {
            self.follow_check();
        }
    }

    /// `zh zl zH zL zj zk`: scroll the view by fractions of the plot along x (right positive,
    /// later) or y (up positive, higher values), as Vim scrolls sideways.
    pub fn scroll_keys(&mut self, fx: f32, fy: f32) {
        let Some(l) = self.pane.layout.as_ref() else {
            return;
        };
        let (xlen, ylen) = l.lengths();
        let (axis, v) = match fy != 0.0 {
            true => (Axis::Y, fy * ylen),
            false => (Axis::X, fx * xlen),
        };
        let (dx, dy) = along(axis, v, l.horizontal);
        let v = plot::nav::pan(self.model, l, &self.pane.view, dx, dy, Some(axis));
        if self.set(v, Change::Keyed) {
            self.follow_check();
        }
    }

    /// `zz`: the view centred on the cursor.
    pub fn centre(&mut self) {
        let (Some(l), Some(c)) = (self.pane.layout.as_ref(), self.pane.cursor.as_ref()) else {
            return;
        };
        let v = plot::nav::centre(self.model, l, &self.pane.view, c);
        if self.set(v, Change::Keyed) {
            self.follow_check();
        }
    }

    /// The series the cursor is on.
    pub fn cursor_series(&self) -> Option<SeriesKey> {
        match &self.pane.cursor {
            Some(Cursor::Point { series, .. }) => Some(series.clone()),
            Some(Cursor::Band { series, .. }) => series.clone(),
            _ => None,
        }
    }

    /// `x`: the cursor's series hidden; the cursor goes with it.
    pub fn hide(&mut self) -> bool {
        let Some(k) = self.cursor_series() else {
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

    /// `o`: only the cursor's series, or all again.
    pub fn only(&mut self) -> bool {
        let Some(k) = self.cursor_series() else {
            return false;
        };
        self.solo(&k);
        true
    }

    /// `X`: every series shown.
    pub fn show_all(&mut self) {
        let mut v = self.pane.view.clone();
        v.hidden.clear();
        v.only = None;
        self.set(v, Change::Step);
    }

    /// `m`: the reference set at the cursor, or cleared when it's there already. False with no
    /// cursor.
    pub fn reference(&mut self) -> bool {
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

    /// `` ` ``: the cursor to the reference. False with none.
    pub fn to_reference(&mut self) -> bool {
        let Some(r) = self.pane.view.reference.clone() else {
            return false;
        };
        let v = match self.pane.layout.as_ref() {
            Some(l) => plot::nav::reveal(self.model, l, &self.pane.view, &r),
            None => self.pane.view.clone(),
        };
        self.pane.cursor = Some(r);
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
        true
    }

    /// The cursor on a place, the view keeping it in sight, the hover hidden until the pointer
    /// moves: a jump from the panel.
    pub fn put_cursor(&mut self, c: Cursor) {
        let v = match self.pane.layout.as_ref() {
            Some(l) => plot::nav::reveal(self.model, l, &self.pane.view, &c),
            None => self.pane.view.clone(),
        };
        self.pane.cursor = Some(c);
        self.pane.hovering = false;
        self.pane.rehover = true;
        self.set(v, Change::Follow);
    }

    /// A search's find: the cursor there, its series brought forward until `Esc`.
    pub fn found(&mut self, c: Cursor) {
        self.pane.cursor = Some(c);
        self.pane.found = self.cursor_series();
        self.pane.hovering = false;
        let v = match self.pane.layout.as_ref() {
            Some(l) => plot::nav::reveal(
                self.model,
                l,
                &self.pane.view,
                self.pane.cursor.as_ref().unwrap(),
            ),
            None => self.pane.view.clone(),
        };
        self.set(v, Change::Follow);
    }
}

/// The keys' actions, on a chart or a graph alike (the design's controls).
pub trait Keys {
    /// `h l j k w b gg G H M L ] [`: the cursor moved as `action` says, `count` times, the view
    /// following it. False if it didn't move.
    fn move_cursor(&mut self, action: &Action, count: u32) -> bool;
    /// `+ - K J`: zoom by `factor` about the cursor (or the middle), eased.
    fn zoom_keys(&mut self, factor: f64);
    /// `zh zl zH zL zj zk`: scroll by fractions of the plot, right and up positive.
    fn scroll_keys(&mut self, fx: f32, fy: f32);
    /// `zz`: the view centred on the cursor.
    fn centre(&mut self);
    /// `0`: the view fits the data again.
    fn fit(&mut self);
    /// `x`: the cursor's series (a graph's group) hidden. False with none.
    fn hide(&mut self) -> bool;
    /// `o`: only the cursor's series (group), or all again. False with none.
    fn only(&mut self) -> bool;
    /// `X`: everything shown.
    fn show_all(&mut self);
    /// `m`: the reference at the cursor, or cleared there. False with no cursor.
    fn reference(&mut self) -> bool;
    /// `` ` ``: the cursor to the reference. False with none.
    fn to_reference(&mut self) -> bool;
    /// A search's find: the cursor there, the view keeping it in sight, its mark brought
    /// forward until `Esc`.
    fn find(&mut self, _c: Cursor) {}
    /// The cursor and the view put back as they were (a search let go).
    fn back(&mut self, _cursor: Option<Cursor>, _view: View) {}
    /// The cursor on a place, the view keeping it in sight: a jump from the detail panel.
    fn put(&mut self, _c: Cursor) {}
}

impl Keys for Act<'_> {
    fn move_cursor(&mut self, action: &Action, count: u32) -> bool {
        Act::move_cursor(self, action, count)
    }
    fn zoom_keys(&mut self, factor: f64) {
        Act::zoom_keys(self, factor)
    }
    fn scroll_keys(&mut self, fx: f32, fy: f32) {
        Act::scroll_keys(self, fx, fy)
    }
    fn centre(&mut self) {
        Act::centre(self)
    }
    fn fit(&mut self) {
        self.reset(None)
    }
    fn hide(&mut self) -> bool {
        Act::hide(self)
    }
    fn only(&mut self) -> bool {
        Act::only(self)
    }
    fn show_all(&mut self) {
        Act::show_all(self)
    }
    fn reference(&mut self) -> bool {
        Act::reference(self)
    }
    fn to_reference(&mut self) -> bool {
        Act::to_reference(self)
    }
    fn find(&mut self, c: Cursor) {
        self.found(c);
    }
    fn back(&mut self, cursor: Option<Cursor>, view: View) {
        self.pane.cursor = cursor;
        self.pane.found = None;
        self.set(view, Change::Follow);
    }
    fn put(&mut self, c: Cursor) {
        self.put_cursor(c);
    }
}

impl FigState {
    /// A key's action on a window's figure, a chart's, a graph's or a gantt's. None with none
    /// drawn.
    pub fn keys<R>(
        &mut self,
        w: WindowId,
        now: Duration,
        f: impl FnOnce(&mut dyn Keys) -> R,
    ) -> Option<R> {
        if self.dash_drawn(w) {
            return self.dash_keys(w, now, f);
        }
        if self.graph.is_some() {
            return self.graph_act(w, now, |g| f(g));
        }
        if self.gantt.is_some() {
            return self.gantt_act(w, now, |g| f(g));
        }
        self.act(w, now, |a| f(a))
    }

    /// Whether the figure is a graph.
    pub fn is_graph(&self) -> bool {
        self.graph.is_some()
    }

    /// Whether the figure is a gantt.
    pub fn is_gantt(&self) -> bool {
        self.gantt.is_some()
    }

    /// Whether the keys in window `w` are a gantt's: the figure's own, or a dashboard's
    /// focused panel's.
    pub fn gantt_here(&self, w: WindowId) -> bool {
        match self.dash_drawn(w) {
            true => self.dash_focused_kind(w) == Some("a gantt"),
            false => self.gantt.is_some(),
        }
    }
}

/// What a job draws a figure's picture from: everything a model is built of (or a graph's model,
/// laid out, or a gantt's), the view, the size and the colours, with fonts of its own (ruling
/// 12).
pub struct Picture {
    /// A dashboard, laid out on the main thread at 2× and 1×.
    dash: Option<(Scene, Scene)>,
    graph: Option<Arc<GraphModel>>,
    gantt: Option<Arc<GanttModel>>,
    chart: Option<Chart>,
    tables: Vec<Arc<Table>>,
    toggles: Toggles,
    zone: TimeZone,
    view: View,
    size: (f32, f32),
    theme: Theme,
    fonts: Option<PathBuf>,
}

impl Picture {
    /// The chart's base scene at `scale`, with the fonts loaded for it.
    fn scene(&self, scale: f32) -> std::result::Result<(Scene, Option<Book>), String> {
        if let Some((two, one)) = &self.dash {
            let book = Book::load(self.fonts.as_deref()).ok();
            let scene = if scale >= 1.5 { two } else { one };
            return Ok((scene.clone(), book));
        }
        if let Some(g) = &self.graph {
            let mut book = Book::load(self.fonts.as_deref()).ok();
            let (scene, _) = measured(book.as_mut(), scale, |m| {
                crate::graph::base(g, &self.view, self.size, scale, &self.theme, m)
            });
            return Ok((scene, book));
        }
        if let Some(g) = &self.gantt {
            let mut book = Book::load(self.fonts.as_deref()).ok();
            let (scene, _) = measured(book.as_mut(), scale, |m| {
                crate::gantt::base(g, &self.view, self.size, scale, &self.theme, m)
            });
            return Ok((scene, book));
        }
        let chart = self.chart.as_ref().ok_or("nothing is drawn yet")?;
        let chart = self.toggles.apply(chart);
        let model =
            ChartModel::build(chart, self.tables.clone(), &self.zone).map_err(|e| e.to_string())?;
        let mut book = Book::load(self.fonts.as_deref()).ok();
        let (scene, ..) = measured(book.as_mut(), scale, |m| {
            plot::base(&model, &self.view, self.size, scale, &self.theme, m)
        });
        Ok((scene, book))
    }

    /// A PNG at `scale`, opaque on the theme's ground, and its size in pixels.
    pub fn png(&self, scale: f32) -> std::result::Result<(Vec<u8>, u32, u32), String> {
        let (scene, mut book) = self.scene(scale)?;
        let bytes = raster::png(&scene, self.theme.surface, book.as_mut())?;
        let (w, h) = (
            (self.size.0 * scale).ceil() as u32,
            (self.size.1 * scale).ceil() as u32,
        );
        Ok((bytes, w, h))
    }

    /// An SVG at 1×.
    pub fn svg(&self) -> std::result::Result<String, String> {
        let (scene, _) = self.scene(1.0)?;
        Ok(crate::svg::write(&scene))
    }
}

impl FigState {
    /// What a job needs to draw the figure as a window shows it (or at 880 × 470 with no window).
    pub fn picture(&self, w: Option<WindowId>) -> Option<Picture> {
        if let Some(scenes) = self.dash_scenes(w) {
            let pane = w.and_then(|w| self.panes.get(&w));
            return Some(Picture {
                dash: Some(scenes),
                graph: None,
                gantt: None,
                chart: None,
                tables: vec![],
                toggles: self.toggles,
                zone: self.zone.clone(),
                view: View::default(),
                size: pane.map_or((880.0, 610.0), Pane::size),
                theme: self.theme.clone(),
                fonts: self.kit.borrow().fonts.clone(),
            });
        }
        let (graph, gantt) = (self.graph.clone(), self.gantt.clone());
        let chart = match (graph.is_some() || gantt.is_some(), &self.chart, &self.model) {
            (true, ..) => None,
            (false, Some(c), Some(_)) => Some(c.clone()),
            _ => return None,
        };
        let pane = w.and_then(|w| self.panes.get(&w));
        let view = pane.map_or_else(View::default, |p| View {
            reference: None,
            ..p.view.clone()
        });
        let size = pane.map_or((880.0, 470.0), Pane::size);
        Some(Picture {
            dash: None,
            graph,
            gantt,
            chart,
            tables: self.tables.clone(),
            toggles: self.toggles,
            zone: self.zone.clone(),
            view,
            size,
            theme: self.theme.clone(),
            fonts: self.kit.borrow().fonts.clone(),
        })
    }
}

/// Why a figure can't draw, as its base shows it: the last build's failure, what the log lists
/// (its errors and warnings, or a good build's warnings), the text they're in, and the name.
struct Failure<'a> {
    failed: Option<&'a Failed>,
    log: &'a [FigError],
    text: &'a str,
    name: &'a str,
}

/// What a figure shows: a chart, a graph, a gantt, or nothing yet.
#[derive(Clone, Copy)]
enum Shows<'a> {
    Chart(&'a ChartModel),
    Graph(&'a GraphModel),
    Gantt(&'a GanttModel),
    Nothing,
}

impl Shows<'_> {
    fn some(&self) -> bool {
        !matches!(self, Shows::Nothing)
    }
}

/// Draw a pane's base, opaque on the surface's colour, presented whole: the log where it's up;
/// else the chart, graph or gantt (beside the detail panel when it's open), with the stale badge over
/// it while the last build failed; else the error figure; else, while the first build runs,
/// nothing.
fn draw_base(
    p: &mut Pane,
    shows: Shows<'_>,
    view: &View,
    theme: &Theme,
    why: &Failure<'_>,
    kit: &mut Kit,
) {
    let scale = p.scale();
    let size = p.size();
    let fsize = p.figure_size();
    let log = p.log;
    let (mut glayout, mut tlayout) = (None, None);
    let (mut scene, layout, hits) = measured(kit.book.as_mut(), scale, |m| {
        let figure = |errors: &[FigError], state: ErrorView, m: &mut dyn Measure| {
            plot::error_figure(errors, why.text, why.name, state, size, scale, theme, m)
        };
        match (log, shows, why.failed) {
            (Some((top, mark)), ..) => {
                (figure(why.log, ErrorView::Log { top, mark }, m), None, None)
            }
            (None, Shows::Graph(g), failed) => {
                let (mut s, l) = crate::graph::base_beside(g, view, fsize, size.0, scale, theme, m);
                if let Some(f) = failed {
                    plot::stale_badge(&mut s, f.first(), theme, m);
                }
                glayout = Some(l);
                (s, None, None)
            }
            (None, Shows::Gantt(g), failed) => {
                let (mut s, l) = crate::gantt::base_beside(g, view, fsize, size.0, scale, theme, m);
                if let Some(f) = failed {
                    plot::stale_badge(&mut s, f.first(), theme, m);
                }
                tlayout = Some(l);
                (s, None, None)
            }
            (None, Shows::Chart(model), failed) => {
                let (mut s, l, h) = plot::base_beside(model, view, fsize, size.0, scale, theme, m);
                if let Some(f) = failed {
                    plot::stale_badge(&mut s, f.first(), theme, m);
                }
                (s, Some(l), Some(h))
            }
            (None, Shows::Nothing, Some(f)) if f.is_waiting() => {
                (waiting(size, scale, f, theme, m), None, None)
            }
            (None, Shows::Nothing, Some(f)) => {
                (figure(&f.all(), ErrorView::Symbolic, m), None, None)
            }
            (None, Shows::Nothing, None) => {
                let mut s = Scene::new(size.0, size.1, scale);
                s.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
                (s, None, None)
            }
        }
    });
    (scene.width, scene.height) = size;
    p.fig_regions = scene.regions.len();
    if log.is_none()
        && shows.some()
        && let Some(panel) = panel::panel_scene(p, theme, kit)
    {
        scene.items.extend(panel.items);
        scene.regions.extend(panel.regions);
    }
    let (aw, ah) = p.area_px();
    let Some(producer) = p.base.as_mut() else {
        return;
    };
    let (w, h, stride, px) = target(producer);
    let damage = PixelBox {
        x: 0,
        y: 0,
        w: aw.min(w),
        h: ah.min(h),
    };
    raster::draw(
        &scene,
        px,
        w,
        h,
        stride,
        Some(damage),
        Some(theme.surface),
        kit.book.as_mut(),
    );
    producer.present(PixelRect::new(0, 0, damage.w, damage.h));
    scene.items.clear();
    p.scene = Some(scene);
    // The card's hysteresis is a cell wide, and the side it was put on lasts across redraws
    // of the base (a pan, an ease's frame, live data), or it would come back each time.
    let flipped = (p.layout.as_ref()).map(|l| l.flipped.get());
    p.layout = layout.map(|mut l| {
        l.cell_width = p.cell.width as f32 / scale;
        if let Some(f) = flipped {
            l.flipped.set(f);
        }
        l
    });
    let gflipped = (p.graph.as_ref()).map(|l| l.flipped.get());
    p.graph = glayout.map(|mut l| {
        l.cell_width = p.cell.width as f32 / scale;
        if let Some(f) = gflipped {
            l.flipped.set(f);
        }
        l
    });
    let tflipped = (p.gantt.as_ref()).map(|l| l.flipped.get());
    p.gantt = tlayout.map(|mut l| {
        l.cell_width = p.cell.width as f32 / scale;
        if let Some(f) = tflipped {
            l.flipped.set(f);
        }
        l
    });
    p.hits = hits;
    p.base_gen += 1;
}

/// A figure waiting for its commands to print (not a failure): quiet, a muted line where a
/// title would be (`waiting for ./ticker.sh`), and nothing else, no spinner.
fn waiting(size: (f32, f32), scale: f32, f: &Failed, theme: &Theme, m: &mut dyn Measure) -> Scene {
    let mut s = Scene::new(size.0, size.1, scale);
    s.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
    let spec = TextSpec::new(12.0);
    let x = crate::dashboard::layout::margin(size.0);
    let text = m.fit(&f.first().msg, spec, (size.0 - 2.0 * x).max(0.0));
    s.push(Item::Text {
        text: text.into(),
        x,
        y: 38.0,
        spec,
        anchor: Anchor::Start,
        color: theme.muted,
        alpha: 1.0,
        max_width: None,
    });
    s
}

/// Draw a pane's overlay where what it shows changed: it clears and draws the union of what it
/// drew last and draws now, and presents that alone.
fn draw_overlay(p: &mut Pane, shows: Shows<'_>, view: &View, theme: &Theme, kit: &mut Kit) {
    let st = p.overlay_state();
    let gst = match shows {
        Shows::Graph(g) if p.log.is_none() => Some(p.graph_overlay(g)),
        _ => None,
    };
    let tst = match shows {
        Shows::Gantt(g) if p.log.is_none() => Some(p.gantt_overlay(g)),
        _ => None,
    };
    let key = OverKey {
        base: p.base_gen,
        hover: st.hover.clone(),
        cursor: st.cursor.clone(),
        pointer: st.pointer,
        brush: st.brush,
        emphasis: st.emphasis.clone(),
        reference: view.reference.clone(),
        help: p.help,
        strip: p.strip,
        graph: gst.clone(),
        gantt: tst.clone(),
        lit: p.chart_lit(),
    };
    if p.over_key.as_ref() == Some(&key) {
        return;
    }
    let scale = p.scale();
    let size = p.size();
    let mut scene = measured(kit.book.as_mut(), scale, |m| {
        match (shows, &p.layout, &p.graph) {
            (Shows::Graph(g), _, Some(l)) => match &gst {
                Some(gst) => crate::graph::overlay(g, l, view, gst, theme, m),
                None => Scene::default(),
            },
            (Shows::Gantt(g), ..) => match (&tst, &p.gantt) {
                (Some(tst), Some(l)) => crate::gantt::overlay(g, l, view, tst, theme, m),
                _ => Scene::default(),
            },
            (Shows::Chart(model), Some(l), _) if p.log.is_none() => {
                plot::overlay::overlay_lit(model, l, view, &st, &key.lit, theme, m)
            }
            _ => Scene::default(),
        }
    });
    (scene.width, scene.height, scene.scale) = (size.0, size.1, scale);
    if let Some(r) = p.strip {
        // The axis strip under the pointer, lit faintly, under everything else.
        let fill = Item::Path {
            path: PathData::rect(r),
            fill: Some(Paint::solid(theme.text, 0.06)),
            stroke: None,
        };
        scene.items.insert(0, fill);
    }
    if p.help {
        let card: &[(&str, &str)] = match shows {
            Shows::Graph(_) => &GRAPH_KEY_CARD,
            Shows::Gantt(_) => &GANTT_KEY_CARD,
            _ => &KEY_CARD,
        };
        measured(kit.book.as_mut(), scale, |m| {
            key_card(&mut scene, card, theme, m)
        });
    }
    let now = scene.bounds();
    let damage = changed(&p.over_items, &scene.items, p.over_bounds, now);
    p.over_bounds = now;
    p.over_key = Some(key);
    p.over_items = scene.items.clone();
    let (aw, ah) = p.area_px();
    let wipe = std::mem::take(&mut p.over_wipe);
    let Some(producer) = p.over.as_mut() else {
        return;
    };
    let (w, h, stride, px) = target(producer);
    let b = match (wipe, damage) {
        // After a resize: all of it, cleared, what was drawn at the old size with it.
        (true, _) => PixelBox { x: 0, y: 0, w, h },
        (false, Some(damage)) => raster::to_physical(damage, scale, aw.min(w), ah.min(h)),
        (false, None) => return,
    };
    if b.is_empty() {
        return;
    }
    raster::draw(&scene, px, w, h, stride, Some(b), None, kit.book.as_mut());
    producer.present(b.into());
}

/// Where an overlay must be drawn again: where its items differ from those it drew last. Items
/// drawn in both, as they were, needn't be (the emphasis wash, its series, under a crosshair that
/// moved), since drawing a rectangle again draws everything in it. If the clips or opacity groups
/// differ, the union of the two scenes' bounds, as ruling 10 has it. None when nothing changed.
fn changed(old: &[Item], new: &[Item], was: Option<Rect>, now: Option<Rect>) -> Option<Rect> {
    let frame = |i: &&Item| {
        matches!(
            i,
            Item::PushClip(_) | Item::PopClip | Item::PushOpacity(_) | Item::PopOpacity
        )
    };
    let union = |a: Option<Rect>, b: Option<Rect>| match (a, b) {
        (Some(a), Some(b)) => Some(a.union(&b)),
        (a, b) => a.or(b),
    };
    if !old.iter().filter(frame).eq(new.iter().filter(frame)) {
        return union(was, now);
    }
    let mut used = vec![false; old.len()];
    let mut fresh = Scene::default();
    for item in new {
        let same = (!frame(&item))
            .then(|| (0..old.len()).find(|&i| !used[i] && old[i] == *item))
            .flatten();
        match same {
            Some(i) => used[i] = true,
            None => fresh.push(item.clone()),
        }
    }
    let mut gone = Scene::default();
    for (i, item) in old.iter().enumerate() {
        if frame(&item) || !used[i] {
            gone.push(item.clone());
        }
    }
    union(fresh.bounds(), gone.bounds())
}

/// The keys `?` shows, a pair a row.
pub const KEY_CARD: [(&str, &str); 21] = [
    ("h l", "previous, next point"),
    ("j k", "series below, above"),
    ("w b", "next, previous peak"),
    ("gg G", "first, last point; G follows"),
    ("H M L", "to the view's left, middle, right"),
    ("+ - 0", "zoom in, out, reset"),
    ("zh zl zz", "scroll left, right; centre"),
    ("x o X", "hide, only, show all"),
    ("u C-r", "undo, redo a view change"),
    ("/ n N", "find a series, next, previous"),
    ("t", "the data as TSV beside"),
    ("s", "settings"),
    ("E C", "export, copy the picture"),
    ("m `", "reference here, cursor to it"),
    ("⏎ Tab", "detail panel, into it and back"),
    ("!", "errors and warnings, as a log"),
    ("Esc", "cancel, clear"),
    ("drag", "zoom to the brush"),
    ("⌥-drag", "pan"),
    ("scroll ⌘-scroll", "pan, zoom about the pointer"),
    ("click ⇧-click", "cursor, reference"),
];

/// The keys `?` shows on a graph, a pair a row.
pub const GRAPH_KEY_CARD: [(&str, &str); 19] = [
    ("h j k l", "nearest node that way"),
    ("] [", "next, previous neighbour"),
    ("gg G", "first, last node"),
    ("+ - 0", "zoom in, out, fit"),
    ("zh zl zj zk zz", "scroll; centre"),
    ("x o X", "hide, only, show all: groups"),
    ("u C-r", "undo, redo a view change"),
    ("/ n N", "find a node, next, previous"),
    ("⏎ Tab", "detail panel, into it and back"),
    ("t", "node and edge tables beside"),
    ("s", "labels, layout, relayout"),
    ("E C", "export, copy the picture"),
    ("m `", "reference here, cursor to it"),
    ("Esc", "cancel, clear"),
    ("drag a node", "move and pin it"),
    ("drag", "pan"),
    ("scroll ⌘-scroll", "pan, zoom about the pointer"),
    ("click ⇧-click", "cursor, reference and path"),
    ("double-click", "detail, or fit"),
];

/// The keys `?` shows on a gantt, a pair a row.
pub const GANTT_KEY_CARD: [(&str, &str); 21] = [
    ("j k", "task below, above"),
    ("h l", "task that starts before, after"),
    ("] [", "a task that comes after; one it comes after"),
    ("w b", "next, previous milestone"),
    ("gg G", "first, last task"),
    ("H M L", "top, middle, bottom task in view"),
    ("+ - 0", "zoom the time in, out; fit the plan"),
    ("zh zl zj zk zz", "scroll the time, the rows; centre"),
    ("x o X", "hide, only, show all: by colour name"),
    ("u C-r", "undo, redo a view change"),
    ("/ n N", "find a task, next, previous"),
    ("⏎ Tab", "detail panel, into it and back"),
    ("t", "the tasks as TSV beside"),
    ("s", "grid, axes, links, today"),
    ("E C", "export, copy the picture"),
    ("!", "errors and warnings, as a log"),
    ("Esc", "cancel, clear"),
    ("drag", "zoom the time to the brush"),
    ("⌥-drag", "pan the time and the rows"),
    ("scroll ⌘-scroll", "rows or time; zoom about the pointer"),
    ("click double-click", "cursor; detail, or fit"),
];

/// The key card, centred over the area: a chart's keys, a graph's or a gantt's.
fn key_card(scene: &mut Scene, rows: &[(&str, &str)], theme: &Theme, m: &mut dyn Measure) {
    let (keys, what) = (
        TextSpec::new(12.0).weight(Weight::Semibold),
        TextSpec::new(12.0),
    );
    let kw = rows.iter().map(|k| m.width(k.0, keys)).fold(0.0, f32::max);
    let ww = rows.iter().map(|k| m.width(k.1, what)).fold(0.0, f32::max);
    let row = 18.0;
    let (w, h) = (kw + ww + 40.0, rows.len() as f32 * row + 16.0);
    let x = ((scene.width - w) / 2.0).max(4.0).round();
    let y = ((scene.height - h) / 2.0).max(4.0).round();
    let card = Rect::new(x, y, w.min(scene.width - 8.0), h.min(scene.height - 8.0));
    scene.push(Item::Shadow {
        rect: card,
        radius: 6.0,
        blur: 8.0,
        dy: 2.0,
        alpha: 0.3,
    });
    scene.push(Item::Path {
        path: PathData::rounded(card, [6.0; 4]),
        fill: Some(Paint::solid(theme.tooltip, 1.0)),
        stroke: Some(Stroke::hairline(theme.ring, 1.0)),
    });
    scene.push(Item::PushClip(card));
    for (i, (k, d)) in rows.iter().enumerate() {
        let base = y + 8.0 + row * (i as f32 + 1.0) - 5.0;
        for (text, at, spec, color) in [
            (*k, x + 12.0, keys, theme.strong),
            (*d, x + 28.0 + kw, what, theme.text),
        ] {
            scene.push(Item::Text {
                text: text.into(),
                x: at,
                y: base,
                spec,
                anchor: Anchor::Start,
                color,
                alpha: 1.0,
                max_width: None,
            });
        }
    }
    scene.push(Item::PopClip);
}

/// What's in a column of the chart's first table: for writing the cursor's x.
pub fn column_kind(model: &ChartModel, field: &str) -> Option<ColumnKind> {
    model.tables.first()?.column(field).map(|c| c.kind())
}

// ------------------------------------------------------------------ the surface

/// A figure's buffer: a surface that shows each window's two pixel buffers over its whole area
/// and writes no cells. It gives no text, so there's no copy state: the pointer is the figure's.
pub struct FigSurface {
    /// The figure.
    pub state: Shared,
}

impl FigSurface {
    /// The surface of a figure.
    pub fn new(state: Shared) -> FigSurface {
        FigSurface { state }
    }
}

impl Surface for FigSurface {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// The window's base at z −2 and its overlay at z −1, at every render: a frame keeps its
    /// cells, not its placements. They were drawn and presented in the turn that changed them.
    fn render(&mut self, frame: &mut Frame, info: &RenderInfo) {
        let mut st = self.state.borrow_mut();
        if info.focused && st.last != Some(info.window) {
            // What a restart keeps is the focused window's view: it changed with the focus.
            st.last = Some(info.window);
            st.rev += 1;
        }
        if let Some(p) = st.panes.get(&info.window) {
            if let Some(b) = &p.base {
                frame.place(b.id(), -2);
            }
            if let Some(o) = &p.over {
                frame.place(o.id(), -1);
            }
        }
    }

    fn status(&self) -> Option<String> {
        Some(self.state.borrow().status())
    }

    /// A size, the pointer and its leaving, the focus, and a key no keymap took.
    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        let mut st = self.state.borrow_mut();
        let took = match ev {
            Event::Resize {
                window,
                cols,
                rows,
                cell,
            } => {
                st.resized(ctx, *window, *cols, *rows, *cell);
                true
            }
            Event::Pointer(p) => {
                st.pointer(ctx, p);
                true
            }
            Event::PointerLeft { window } => {
                st.left(*window);
                true
            }
            Event::Focus(gained) => {
                st.focus(ctx, *gained);
                true
            }
            Event::Key(k) => st.key(k),
            _ => false,
        };
        st.refresh(ctx);
        took
    }

    /// Every pointer sample, not one a cell: the crosshair follows the hand.
    fn fine_motion(&self) -> bool {
        true
    }

    fn takes_wheel(&self) -> bool {
        true
    }
}
