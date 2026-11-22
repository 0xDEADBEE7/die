//! A dashboard in a window (contract-p4 §5; briefs/mouse-ux.md §4.6 and §4.7): every panel a
//! figure of its own, driven as a lone figure's window drives it, and the dashboard's own ways
//! over them.
//!
//! - **Building.** The figure's build job parses the spec. A job then plans the dashboard (its
//!   panels, and what they read), drawn at once with every panel still waiting as its frame and
//!   title; each data file is read in a job of its own, and each that lands builds the panels
//!   again in a job, so the panels appear as their data comes. A command's panels wait for what
//!   it prints ([`super::feeds`]). A panel that fails shows its error figure; one whose rebuild
//!   fails keeps its figure with the stale badge.
//! - **Panels.** Each panel's view, cursor, hover and gesture live in a pane of its own, as a lone
//!   chart's or graph's window has them, so a panel keeps every key and gesture it has alone.
//!   Its layout is the dashboard's, lent to its pane for each act.
//! - **Linked time.** The panels on the dashboard's `time.field` share one x domain: a zoom, a pan,
//!   a brush or a key on one moves them all, eased together, as one undo step (a dashboard has
//!   one history, mouse-ux §9). The time follows the newest sample while its right edge is there.
//! - **Focus.** `Tab` and `S-Tab` go through the panels in reading order, then the detail panel's
//!   stop; a click on a panel focuses it and puts its cursor there (on its title, focus only). The
//!   focused panel's ring is the border grey. `f` shows it full size and back.
//! - **The pointer** (mouse-ux §4.6, §3): the panel under the pointer is hovered, its title
//!   brightened and its figure hovered as alone, the linked moment drawn in the others; the press
//!   that focuses the window only focuses it; a press is held by its panel and its button; a
//!   scroll burst keeps its first meaning (the page while the rows overflow and the wheel goes up
//!   or down, else the panel under the pointer: a linked chart's scroll is the shared time's); the
//!   hover is worked out again when the view moves; the pointer leaving clears it.

use super::{Act, FigState, GraphAct, Keys, Kit, Pane, changed, key_card, measured, ms, target};
use crate::dashboard::layout::{DashLayout, PanelFigure, PanelLayout};
use crate::dashboard::{
    self, DashHit, DashHover, DashModel, DashView, LiveState, LiveStatus, Origin, PanelHover,
    PanelState, RANGES, SourceState, Sources, Want,
};
use crate::data::ReadOptions;
use crate::detail::Item;
use crate::figure::{self, TileHover};
use crate::interact::consts::{CLICK_SLOP, MOTION_MS, ZOOM_STEP};
use crate::interact::{Action, Cursor, Gesture, Steps, Toggles, Tween, View};
use crate::live::{self, FollowChange};
use crate::plot::{self, ErrorView};
use crate::raster::{self, PixelBox};
use crate::scene::{Hit, Item as SceneItem, Paint, PathData, Rect, Scene};
use crate::source::{self, Failed, Snapshot};
use crate::spec::{DashboardSpec, DataRef, Figure, Panel as SpecPanel, PanelSpec, Row, Tooltip};
use crate::theme::Theme;
use die_core::{Button, CellPixels, Mods, PixelRect, PointerKind, Sub, SurfacePointer};
use jiff::tz::TimeZone;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// The keys `?` shows on a dashboard, a pair a row.
pub const DASH_KEY_CARD: [(&str, &str); 19] = [
    ("Tab S-Tab", "next, previous panel"),
    ("f", "the panel full size, and back"),
    ("T", "the time range"),
    ("R", "run the commands again"),
    ("G", "follow the newest"),
    ("h l j k", "the panel's cursor"),
    ("+ - 0", "zoom in, out; the time's range"),
    ("zh zl", "scroll the time"),
    ("x o X", "hide, only, show all"),
    ("u C-r", "undo, redo a view change"),
    ("⏎", "detail panel"),
    ("/ t", "find in the panel; its data as TSV"),
    ("!", "a panel's errors, as a log"),
    ("Esc", "cancel, clear, leave full size"),
    ("drag", "zoom every linked panel"),
    ("scroll", "the page, or the time"),
    ("⌘-scroll", "zoom about the pointer"),
    ("click", "focus the panel, its cursor"),
    ("double-click", "the time's range; a title full size"),
];

// ------------------------------------------------------------------ the figure's dashboard

/// What an undo step of a dashboard puts back: the shared time, each panel's view (not its
/// cursor, nor its reference, mouse-ux §9), the panel shown full size, and whether the time
/// followed the newest.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DashSnap {
    /// The shared time.
    pub time: Option<(f64, f64)>,
    /// Each panel's view.
    pub views: Vec<View>,
    /// The panel shown full size.
    pub full: Option<usize>,
    /// Whether the time followed.
    pub follow: bool,
}

/// A figure that's a dashboard: its spec, where it is, the data read for it, its model, and
/// the jobs under way.
pub struct DashState {
    /// The spec as parsed.
    pub spec: DashboardSpec,
    /// The spec's own file, if it has one: `file` panels naming it are a cycle, and your yeses
    /// are kept by it.
    pub file: Option<PathBuf>,
    /// Its directory: data paths and commands start there.
    pub dir: PathBuf,
    /// Its panels as last built; None until the first plan lands.
    pub model: Option<DashModel>,
    /// Goes up with each model.
    pub version: u64,
    /// What's been read so far: each file and command's output, and the file panels you said
    /// yes to.
    pub sources: Sources,
    /// Each panel's settings turned by `s`, by number.
    pub toggles: Vec<Toggles>,
    /// What a restart kept of the windows' views, for the first window drawn.
    pub seed: Option<die_core::session::Value>,
    planning: Option<Sub>,
    reading: Vec<(Want, Sub)>,
    building: Option<Sub>,
    /// Built again once the build under way is done: what it read has changed since.
    dirty: bool,
}

impl DashState {
    /// A dashboard from its spec, at `file` in `dir`.
    pub fn new(spec: DashboardSpec, file: Option<PathBuf>, dir: PathBuf) -> DashState {
        DashState {
            spec,
            file,
            dir,
            model: None,
            version: 0,
            sources: Sources::new(),
            toggles: vec![],
            seed: None,
            planning: None,
            reading: vec![],
            building: None,
            dirty: false,
        }
    }

    /// Whether a job of it is under way.
    pub fn busy(&self) -> bool {
        self.planning.is_some() || self.building.is_some() || !self.reading.is_empty()
    }

    fn job(&self, open: Vec<(PathBuf, Snapshot)>, zone: &TimeZone) -> DashJob {
        DashJob {
            spec: self.spec.clone(),
            sources: self.sources.clone(),
            file: self.file.clone(),
            dir: self.dir.clone(),
            zone: zone.clone(),
            open,
            toggles: self.toggles.clone(),
        }
    }
}

/// A tile alone, as a dashboard of one panel; a dashboard, as it is; None for another figure.
pub fn spec_of(figure: &Figure) -> Option<DashboardSpec> {
    match figure {
        Figure::Dashboard(d) => Some(d.clone()),
        tile @ (Figure::Stat(_) | Figure::Meter(_) | Figure::Sparkline(_)) => Some(DashboardSpec {
            title: None,
            data: vec![],
            time: None,
            tooltip: Tooltip::Single,
            rows: vec![Row {
                panels: vec![PanelSpec {
                    panel: SpecPanel::Figure(Box::new(tile.clone())),
                    span: 1,
                    at: Default::default(),
                }],
                height: None,
                at: Default::default(),
            }],
        }),
        _ => None,
    }
}

/// What a dashboard's job is given to build its panels: the spec, the data in hand, where the
/// spec is, the zone, the buffers' texts, and each panel's settings.
struct DashJob {
    spec: DashboardSpec,
    sources: Sources,
    file: Option<PathBuf>,
    dir: PathBuf,
    zone: TimeZone,
    open: Vec<(PathBuf, Snapshot)>,
    toggles: Vec<Toggles>,
}

/// Files read from disk, and when each last changed: polled while a window shows the figure.
type Polled = Vec<(PathBuf, Option<SystemTime>)>;

/// What a dashboard's job made: its panels, and the nested specs it read, from disk (polled)
/// and from buffers (followed).
pub(super) struct DashBuilt {
    model: DashModel,
    files: Vec<(PathBuf, Option<SystemTime>)>,
    followed: Vec<PathBuf>,
}

impl DashJob {
    /// Build every panel: off the main thread.
    fn run(self) -> DashBuilt {
        let opt = ReadOptions::new(self.zone.clone());
        let read: RefCell<(Polled, Vec<PathBuf>)> = RefCell::default();
        let open = &self.open;
        let resolve = |p: &Path| -> crate::error::Result<Figure> {
            match open.iter().find(|(q, _)| source::same_path(q, p)) {
                Some((q, text)) => {
                    read.borrow_mut().1.push(q.clone());
                    let text = text.to_string();
                    let dir = p.parent().unwrap_or(Path::new("."));
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                    crate::spec::parse(&text, name.as_deref().unwrap_or(""), dir, &opt)
                        .map(|x| x.figure)
                        .map_err(|e| e.in_file(p, Some(&text)))
                }
                None => {
                    read.borrow_mut()
                        .0
                        .push((p.to_path_buf(), source::modified(p)));
                    dashboard::open(p, &opt)
                }
            }
        };
        let origin = Origin {
            file: self.file.as_deref(),
            dir: &self.dir,
            zone: &self.zone,
        };
        let mut model = DashModel::build(&self.spec, &self.sources, &resolve, &origin);
        apply_toggles(&mut model, &self.toggles, &self.zone);
        let (files, followed) = read.into_inner();
        DashBuilt {
            model,
            files,
            followed,
        }
    }
}

/// Each panel's settings (`s`) applied: a chart's model built again from its tables with them,
/// a graph's laid out again (its labels, its layout).
fn apply_toggles(model: &mut DashModel, toggles: &[Toggles], zone: &TimeZone) {
    for (p, t) in model.panels.iter_mut().zip(toggles) {
        if *t == Toggles::default() {
            continue;
        }
        match &p.state {
            PanelState::Chart(cm) => {
                if let Ok(m) =
                    crate::plot::ChartModel::build(t.apply(&cm.chart), cm.tables.clone(), zone)
                {
                    p.state = PanelState::Chart(m);
                }
            }
            PanelState::Graph(g) => {
                let spec = t.apply_graph(&g.spec);
                if let Ok(m) = crate::graph::GraphModel::build_as(spec, t.graph_layout_for(&g.spec))
                {
                    p.state = PanelState::Graph(Arc::new(m));
                }
            }
            _ => {}
        }
    }
}

/// A graph panel's spec as the dashboard gave it, before its settings turned its labels.
fn spec_before(g: &crate::graph::GraphModel, t: &Toggles) -> crate::spec::GraphSpec {
    let mut spec = g.spec.clone();
    if let Some(on) = t.labels {
        // Turned to `on` only where the spec said otherwise.
        spec.view.labels = match on {
            true => crate::spec::Labels::None,
            false => crate::spec::Labels::Auto,
        };
    }
    spec
}

/// One data file a dashboard wants, read: from its buffer if one visits it (followed), else
/// from disk (polled). Off the main thread.
fn read_want(
    w: &Want,
    open: Option<Snapshot>,
    zone: &TimeZone,
) -> (SourceState, Option<(PathBuf, Option<SystemTime>)>) {
    let DataRef::File(path) = &w.data.source else {
        return (SourceState::Waiting(String::new()), None);
    };
    let opt = ReadOptions::new(zone.clone());
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let (text, polled) = match open {
        Some(t) => (Ok(t.to_string()), None),
        None => (
            source::read_file(path),
            Some((path.clone(), source::modified(path))),
        ),
    };
    let state = match text {
        Err(e) => SourceState::Failed(e.in_file(path, None)),
        Ok(text) => match source::read_data(&text, &w.data, &name, &opt) {
            Ok(t) => SourceState::Ready(Arc::new(t)),
            Err(e) => {
                SourceState::Failed(e.in_file(path, Some(&text)).note(format!("read {name}")))
            }
        },
    };
    (state, polled)
}

/// The buffers that visit files, and their texts, for a job: each written out on the job's
/// thread, and only where it's wanted.
fn open_snapshots(ctx: &die_core::Ctx) -> Vec<(PathBuf, Snapshot)> {
    let mut open: Vec<(PathBuf, Snapshot)> = vec![];
    for b in ctx.buffer_list() {
        let Some(path) = ctx.file(b).map(|f| f.path.clone()) else {
            continue;
        };
        if let Ok(t) = ctx.text(b) {
            open.push((path, Box::new(t.rope().clone())));
        }
    }
    open
}

impl FigState {
    /// The figure is a dashboard (or a tile alone): plan it again from its spec, the panels
    /// drawn as they were until what they read lands.
    pub(super) fn dash_built(&mut self, ctx: &mut die_core::Ctx, spec: DashboardSpec) {
        let file = self
            .file
            .clone()
            .map(|f| std::path::absolute(&f).unwrap_or(f));
        let dir = self.dir.clone();
        match self.dash.as_mut() {
            Some(d) => {
                d.spec = spec;
                (d.file, d.dir) = (file, dir);
            }
            None => {
                let mut d = DashState::new(spec, file, dir);
                d.seed = self.dash_seed.take();
                self.dash = Some(d);
            }
        }
        self.dash_plan(ctx);
    }

    /// Plan the dashboard in a job: its panels over what's read so far, and what they want.
    fn dash_plan(&mut self, ctx: &mut die_core::Ctx) {
        let open = open_snapshots(ctx);
        let zone = self.zone.clone();
        let Some(d) = self.dash.as_mut() else { return };
        let job = d.job(open, &zone);
        let me = self.me.clone();
        d.planning = Some(ctx.spawn(
            move || job.run(),
            move |ctx, out: DashBuilt| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    if let Some(d) = st.dash.as_mut() {
                        d.planning = None;
                    }
                    st.dash_planned(ctx, out);
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// The plan landed: it's drawn (its panels waiting for what isn't read yet), each data file
    /// it wants is read in a job of its own, and its commands asked for and started.
    fn dash_planned(&mut self, ctx: &mut die_core::Ctx, out: DashBuilt) {
        self.dash_files(out.files, out.followed);
        let wants: Vec<Want> = out
            .model
            .wants()
            .iter()
            .filter(|w| matches!(w.data.source, DataRef::File(_)))
            .cloned()
            .collect();
        self.dash_install(out.model);
        let zone = self.zone.clone();
        let mut open = open_snapshots(ctx);
        for w in wants {
            let DataRef::File(path) = &w.data.source else {
                continue;
            };
            let text = open
                .iter()
                .position(|(p, _)| source::same_path(p, path))
                .map(|i| open.remove(i).1);
            let me = self.me.clone();
            let zone = zone.clone();
            let (want, key) = (w.clone(), w.clone());
            let sub = ctx.spawn(
                move || read_want(&want, text, &zone),
                move |ctx, (state, polled)| {
                    if let Some(s) = me.upgrade() {
                        let mut st = s.borrow_mut();
                        st.dash_read(ctx, &w, state, polled);
                        st.refresh(ctx);
                    }
                    Ok(())
                },
            );
            // A read of the same file under way is no more use: this one reads it as it is now.
            if let Some(d) = self.dash.as_mut() {
                d.reading.retain(|(r, _)| *r != key);
                d.reading.push((key, sub));
            }
        }
        self.live_sync(ctx);
        self.keep_polling(ctx);
    }

    /// A data file read: its table (or why not) to the sources, and the panels built again.
    fn dash_read(
        &mut self,
        ctx: &mut die_core::Ctx,
        w: &Want,
        state: SourceState,
        polled: Option<(PathBuf, Option<SystemTime>)>,
    ) {
        let followed = match (&polled, &w.data.source) {
            (None, DataRef::File(p)) => vec![p.clone()],
            _ => vec![],
        };
        self.dash_files(polled.into_iter().collect(), followed);
        let Some(d) = self.dash.as_mut() else { return };
        d.reading.retain(|(r, _)| r != w);
        d.sources.insert(w.clone(), state);
        self.dash_rebuild(ctx);
        self.keep_polling(ctx);
    }

    /// The files a dashboard read: polled while shown, or followed in their buffers.
    fn dash_files(&mut self, files: Vec<(PathBuf, Option<SystemTime>)>, followed: Vec<PathBuf>) {
        for (p, t) in files {
            if !self.files.iter().any(|(q, _)| source::same_path(q, &p)) {
                self.files.push((p, t));
            }
        }
        for p in followed {
            if !self.followed.iter().any(|q| source::same_path(q, &p)) {
                self.followed.push(p);
            }
        }
    }

    /// The panels built again in a job from what's read now; with one under way, again once
    /// it's done.
    pub(super) fn dash_rebuild(&mut self, ctx: &mut die_core::Ctx) {
        let open = open_snapshots(ctx);
        let zone = self.zone.clone();
        let Some(d) = self.dash.as_mut() else { return };
        if d.building.is_some() {
            d.dirty = true;
            return;
        }
        let job = d.job(open, &zone);
        let me = self.me.clone();
        d.building = Some(ctx.spawn(
            move || job.run(),
            move |ctx, out: DashBuilt| {
                if let Some(s) = me.upgrade() {
                    let mut st = s.borrow_mut();
                    let again = match st.dash.as_mut() {
                        Some(d) => {
                            d.building = None;
                            std::mem::take(&mut d.dirty)
                        }
                        None => false,
                    };
                    st.dash_files(out.files, out.followed);
                    st.dash_install(out.model);
                    if again {
                        st.dash_rebuild(ctx);
                    }
                    st.live_sync(ctx);
                    st.refresh(ctx);
                }
                Ok(())
            },
        ));
    }

    /// A model to draw: each window's panes fit to it (cursors kept on their samples, a
    /// following time slid to the newest), and each panel that failed (or waits again) kept as
    /// it was with its badge.
    fn dash_install(&mut self, model: DashModel) {
        let Some(d) = self.dash.as_mut() else { return };
        let old = d.model.take();
        for p in self.panes.values_mut() {
            if let Some(dp) = p.dash.as_mut() {
                dp.fit(old.as_ref(), &model);
            }
        }
        let model = match old {
            Some(old) => model.carry(old),
            None => model,
        };
        d.toggles.resize(model.panels.len(), Toggles::default());
        d.model = Some(model);
        d.version += 1;
        self.rev += 1;
    }
}

// ------------------------------------------------------------------ a window's dashboard

/// A press on the dashboard's own chrome.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    /// None.
    None,
    /// The focusing press, or one on a gap: its drags and release do nothing.
    Swallowed,
    /// On the time-range chip: a click opens the ranges.
    Chip((f32, f32)),
    /// On the live status: a click runs the commands again.
    Live((f32, f32)),
    /// On the scrollbar's thumb: a drag scrolls, from where it was.
    Thumb { from: f32, then: f32 },
    /// On a panel's error figure (its symbolic figure or its log's header): a click turns it
    /// over.
    Error { panel: usize, at: (f32, f32) },
}

/// A scroll burst on a dashboard: its first event's meaning (the page, a panel, or nothing),
/// kept to its end (mouse-ux §3.2, call C4).
struct Burst {
    page: bool,
    panel: Option<usize>,
    zoom: bool,
    /// Ends it when the wheel is quiet, where no panel's own burst holds the timer.
    _timer: Option<Sub>,
}

/// What a pointer event on a dashboard asks of the figure, past the window.
pub(super) enum Then {
    /// Nothing more.
    None,
    /// The time ranges, as `T` asks.
    Range,
    /// The commands run again, as `R`.
    Rerun,
    /// A graph node's or edge's detail, a double click's.
    Open(Item),
    /// To the place of the focused panel's error: its log's entry, else its first.
    GotoError(Option<usize>),
}

/// What a dashboard window's base shows: drawn again when this changes.
#[derive(Clone, Debug, PartialEq)]
struct DashKey {
    version: u64,
    fail: u64,
    theme: u64,
    size: (u32, u32),
    scale: u32,
    time: Option<(f64, f64)>,
    views: Vec<View>,
    errors: Vec<ErrorView>,
    focused: Option<usize>,
    full: Option<usize>,
    scroll: u32,
    live: Option<LiveStatus>,
    detail: bool,
}

/// What a dashboard window's overlay shows: drawn again when this changes.
#[derive(Clone, Debug, PartialEq)]
struct OverKey {
    base: u64,
    hover: Option<DashHover>,
    cursors: Vec<Option<Cursor>>,
    emphasis: Vec<Option<crate::data::SeriesKey>>,
    tiles: Vec<Option<usize>>,
    help: bool,
    strip: Option<Rect>,
}

/// A dashboard in one window: its view, each panel's pane, where everything landed, one undo
/// history, and what the pointer is doing.
pub struct DashPane {
    /// The shared time, the focus, full size and the scroll (each panel's own view is its
    /// pane's; its error figure's state is here).
    pub view: DashView,
    /// Each panel's pane, by number: its view, cursor, hover and gesture.
    pub subs: Vec<Pane>,
    /// Where everything landed, as last drawn.
    pub layout: Option<DashLayout>,
    /// The dashboard's undo history: one for every panel.
    pub undo: Steps<DashSnap>,
    /// The panel under the pointer.
    pub hovered: Option<usize>,
    /// Whether the shared time follows the newest sample.
    pub follow: bool,
    /// A tile's cursor on its trend, by number: its point.
    pub tile_points: Vec<Option<usize>>,
    /// The panel whose press is under way.
    held: Option<usize>,
    page: Page,
    burst: Option<Burst>,
    /// What a page scroll hasn't moved yet, in physical px: less than one.
    carry: f32,
    time_ease: Option<Tween<(f64, f64)>>,
    /// The view before the gesture or burst under way, for its undo step.
    before: Option<DashSnap>,
    drawn: Option<DashKey>,
    over_key: Option<OverKey>,
    tile_hover: Option<(usize, TileHover)>,
    /// The panels in reading order as the dashboard lays them out whole (Tab's order, kept
    /// while one is full size).
    order: Vec<usize>,
    /// New data came while a linked chart's press was held: the following time slides once
    /// the press lets go, so a brush never moves under the hand.
    follow_due: bool,
}

impl DashPane {
    /// A window's dashboard as it opens: `time.range` ending at the newest sample, nothing
    /// focused.
    pub fn new(model: &DashModel) -> DashPane {
        let view = DashView::initial(model);
        let subs = view
            .panels
            .iter()
            .map(|p| Pane::new(p.view.clone(), None, Steps::default()))
            .collect();
        let mut dp = DashPane {
            tile_points: vec![None; model.panels.len()],
            view,
            subs,
            layout: None,
            undo: Steps::default(),
            hovered: None,
            follow: true,
            held: None,
            page: Page::None,
            burst: None,
            carry: 0.0,
            time_ease: None,
            before: None,
            drawn: None,
            over_key: None,
            tile_hover: None,
            order: Vec::new(),
            follow_due: false,
        };
        dp.sync_linked(model);
        dp
    }

    /// The pointer's hover gone from every panel until it moves again (the window changed
    /// size, or a key moved another panel's cursor): each pane's hover, card and emphasis.
    pub(super) fn drop_hover(&mut self) {
        self.hover_to(None);
        self.tile_hover = None;
        for s in &mut self.subs {
            (s.hover, s.near, s.legend, s.strip, s.target) = (None, None, None, None, None);
            s.row_hover = None;
        }
    }

    /// Whether panel `i` shows its error figure: it failed, or its log is up over the figure it
    /// kept with the stale badge.
    fn error_shown(&self, model: &DashModel, i: usize) -> bool {
        let Some(p) = model.panels.get(i) else {
            return false;
        };
        let log = matches!(
            self.view.panels.get(i).map(|v| v.error),
            Some(ErrorView::Log { .. })
        );
        matches!(p.state, PanelState::Failed(_)) || (p.stale.is_some() && log)
    }

    /// Panel `i`'s error figure turned over, symbolic and log (a panel with the stale badge:
    /// its log over the figure, or the figure again). False where it has no error.
    fn flip_error(&mut self, model: &DashModel, i: usize) -> bool {
        if model.panels.get(i).and_then(|p| p.errors()).is_none() {
            return false;
        }
        let Some(pv) = self.view.panels.get_mut(i) else {
            return false;
        };
        pv.error = match pv.error {
            ErrorView::Symbolic => ErrorView::Log { top: 0, mark: 0 },
            ErrorView::Log { .. } => ErrorView::Symbolic,
        };
        self.drawn = None;
        true
    }

    /// A following time slid to the newest after a press let go, where new data came while it
    /// was held.
    fn catch_up(&mut self, model: &DashModel) {
        if !std::mem::take(&mut self.follow_due) || self.held.is_some() || !self.follow {
            return;
        }
        let f = live::follow(
            self.view.time,
            model.newest(),
            true,
            0.0,
            FollowChange::Data,
        );
        self.view.time = f.x;
        self.sync_linked(model);
        self.drawn = None;
    }

    /// What it drew is to be drawn again: something else drew over it (the log).
    pub fn forget(&mut self) {
        self.drawn = None;
        self.over_key = None;
    }

    /// Whether a press on a panel is under way.
    pub fn gesturing(&self) -> bool {
        self.held.is_some()
    }

    /// The dashboard's undo step as it stands.
    pub fn snapshot(&self) -> DashSnap {
        DashSnap {
            time: self.view.time,
            views: self
                .subs
                .iter()
                .map(|s| View {
                    reference: None,
                    ..s.view.clone()
                })
                .collect(),
            full: self.view.full,
            follow: self.follow,
        }
    }

    /// Fit to a model built again: as many panes as panels (a new one at its figure's opening
    /// view), each cursor kept on its sample where its panel is the same kind, a panel that
    /// came to draw at its opening view, and a following time slid to the newest.
    fn fit(&mut self, old: Option<&DashModel>, new: &DashModel) {
        self.view.fit(new);
        let n = new.panels.len();
        self.subs.truncate(n);
        self.tile_points.resize(n, None);
        for i in 0..n {
            if i >= self.subs.len() {
                let v = self.view.panels[i].view.clone();
                self.subs.push(Pane::new(v, None, Steps::default()));
                continue;
            }
            let Some(old) = old.and_then(|o| o.panels.get(i)) else {
                continue;
            };
            let sub = &mut self.subs[i];
            match (&old.state, &new.panels[i].state) {
                (PanelState::Chart(oc), PanelState::Chart(nc)) => {
                    sub.cursor = (sub.cursor.take()).and_then(|c| live::keep_cursor(oc, nc, &c));
                    sub.view.reference =
                        (sub.view.reference.take()).and_then(|c| live::keep_cursor(oc, nc, &c));
                }
                (PanelState::Graph(_), PanelState::Graph(ng)) => {
                    let has = |c: &Option<Cursor>| match c {
                        Some(Cursor::Node(id)) => ng.index_of(id).is_some(),
                        _ => false,
                    };
                    if !has(&sub.cursor) {
                        sub.cursor = None;
                    }
                    if !has(&sub.view.reference) {
                        sub.view.reference = None;
                    }
                    sub.view.pinned.retain(|id, _| ng.index_of(id).is_some());
                }
                (PanelState::Tile(_), PanelState::Tile(_)) => {}
                // Still waiting, or failed (kept as it was where it drew): as it was.
                (_, PanelState::Waiting(_) | PanelState::Failed(_)) => {}
                // Waiting until now (its data, a yes): it draws, with the view and the cursor it
                // was given (a restart's) where they fit, else its figure's opening view.
                (PanelState::Waiting(_) | PanelState::Failed(_), next) => {
                    let (fits, initial) = match next {
                        PanelState::Chart(m) => (
                            sub.cursor.as_ref().is_none_or(|c| cursor_fits(m, c)),
                            m.initial_view(),
                        ),
                        PanelState::Graph(g) => (
                            match &sub.cursor {
                                Some(Cursor::Node(id)) => g.index_of(id).is_some(),
                                c => c.is_none(),
                            },
                            g.initial_view(),
                        ),
                        _ => (sub.cursor.is_none(), View::default()),
                    };
                    if !fits {
                        sub.cursor = None;
                    }
                    if sub.view == View::default() {
                        sub.view = initial;
                    }
                }
                (_, next) => {
                    let view = match next {
                        PanelState::Chart(m) => m.initial_view(),
                        PanelState::Graph(g) => g.initial_view(),
                        _ => View::default(),
                    };
                    *sub = Pane::new(view, None, Steps::default());
                    self.tile_points[i] = None;
                }
            }
            sub.rehover = true;
        }
        if self.hovered.is_some_and(|h| h >= n) {
            self.hovered = None;
        }
        if self.held.is_some_and(|h| h >= n) {
            self.held = None;
        }
        // The time's range is set once there's data to end it at: the plan's panels had none.
        let had_data = old.is_some_and(|o| o.newest().is_some());
        if !had_data && self.view.time.is_none() && new.newest().is_some() {
            self.view.time = DashView::initial(new).time;
        }
        // New data slides a following time to the newest, its width kept; not under a press
        // held on a linked chart (a brush, a pan), which slides it once it lets go.
        let holding = self.held.is_some_and(|h| is_linked_chart(new, h));
        if self.follow && holding {
            self.follow_due = true;
        } else if self.follow {
            let f = live::follow(self.view.time, new.newest(), true, 0.0, FollowChange::Data);
            self.view.time = f.x;
        }
        self.sync_linked(new);
        self.drawn = None;
    }

    /// The linked charts' panes on the shared time: their x is the dashboard's.
    fn sync_linked(&mut self, model: &DashModel) {
        let shared = self.view.shared(model);
        for (i, sub) in self.subs.iter_mut().enumerate() {
            if is_linked_chart(model, i) && sub.view.x != shared {
                sub.view.x = shared;
                sub.ease = None;
                sub.rehover = true;
            }
        }
    }

    /// The shared time as shown now: on its way while it eases.
    fn shown_time(&self, now: u64) -> Option<(f64, f64)> {
        match (&self.time_ease, self.view.time) {
            (Some(t), Some(_)) if !t.done(now) => {
                let k = f64::from(t.progress(now));
                let lerp = |a: f64, b: f64| a + (b - a) * k;
                Some((lerp(t.from.0, t.to.0), lerp(t.from.1, t.to.1)))
            }
            (_, time) => time,
        }
    }

    /// The view as drawn now: the time on its way, each panel's own view as its pane shows it,
    /// and each cursor.
    fn shown(&self, now: u64) -> DashView {
        DashView {
            time: self.shown_time(now),
            panels: (self.subs.iter().enumerate())
                .map(|(i, s)| dashboard::PanelView {
                    view: s.shown(now),
                    cursor: s.cursor.clone(),
                    error: self.view.panels.get(i).map(|p| p.error).unwrap_or_default(),
                    point: self.tile_points.get(i).copied().flatten(),
                    emphasis: s.found.clone(),
                })
                .collect(),
            ..self.view.clone()
        }
    }

    /// Whether anything eases: the time, or a panel's own view.
    fn easing(&self) -> bool {
        self.time_ease.is_some() || self.subs.iter().any(|s| s.ease.is_some())
    }

    /// The panel under the pointer is `over` (its number, and the point in its figure's px), or
    /// none: its pane hovers, the others let go.
    fn hover_to(&mut self, over: Option<(usize, (f32, f32))>) {
        for (j, s) in self.subs.iter_mut().enumerate() {
            match over {
                Some((i, at)) if i == j => {
                    s.pointer = Some(at);
                    s.hovering = true;
                    s.rehover = true;
                }
                _ if s.pointer.is_some() || s.hover.is_some() || s.legend.is_some() => {
                    (s.pointer, s.hovering) = (None, false);
                    s.rehover = true;
                }
                _ => {}
            }
        }
        self.hovered = over.map(|o| o.0);
        if self
            .tile_hover
            .is_some_and(|(i, _)| Some(i) != self.hovered)
        {
            self.tile_hover = None;
        }
    }

    /// Focus panel `i`, bringing it into view.
    fn focus(&mut self, i: usize) {
        if self.view.focused != Some(i) {
            self.view.focused = Some(i);
            if let Some(l) = &self.layout {
                self.view.scroll = l.reveal(i);
            }
        }
    }

    /// The x a pixel is on the linked charts, for follow's slack.
    fn time_px(&self, model: &DashModel) -> f64 {
        let Some(l) = &self.layout else { return 0.0 };
        let (Some((a, b)), Some(w)) = (
            self.view.shared(model),
            (model.linked().into_iter())
                .find_map(|i| l.panel(i).and_then(|p| p.chart()).map(|(c, _)| c.plot.w)),
        ) else {
            return 0.0;
        };
        (b - a).abs() / f64::from(w.max(1.0))
    }

    /// A change to the shared time by the dashboard itself (a range chosen, a reset, undo),
    /// eased from where it's shown now, the linked panes on it.
    fn set_time(&mut self, model: &DashModel, time: Option<(f64, f64)>, now: u64, motion: bool) {
        let from = self.shown_time(now).or_else(|| model.extent());
        let to = time.or_else(|| model.extent());
        self.time_ease = match (from, to, motion) {
            (Some(f), Some(t), true) if f != t => Some(Tween::new(f, t, now, MOTION_MS)),
            _ => None,
        };
        self.view.time = time;
        let px = self.time_px(model);
        self.follow = live::follow(to, model.newest(), self.follow, px, FollowChange::Moved).on;
        self.sync_linked(model);
    }

    /// After an act on panel `i`'s pane: its own undo step becomes the dashboard's (from the
    /// view before its gesture, if one), and a linked chart's x becomes the time every linked
    /// panel shows, eased as its pane eases, following the newest only at the end.
    fn after_act(
        &mut self,
        model: &DashModel,
        i: usize,
        before: DashSnap,
        had: usize,
        shared: Option<(f64, f64)>,
    ) {
        let Some(sub) = self.subs.get_mut(i) else {
            return;
        };
        if sub.undo.undoable() > had {
            sub.undo.retract();
            let snap = self.before.take().unwrap_or(before);
            self.undo.push(snap);
        }
        if !is_linked_chart(model, i) {
            return;
        }
        let sub = &self.subs[i];
        if sub.view.x == shared {
            return;
        }
        let x = sub.view.x;
        let ease = sub.ease.as_ref().and_then(|e| {
            let (f, t) = (e.tween.from.x?, e.tween.to.x?);
            Some(Tween::new(f, t, e.tween.start_ms, e.tween.ms))
        });
        // A reset to fit on a linked panel (`0`, a double click) is the time's range.
        let x = match x {
            None => DashView::reset_time(&self.view, model).time,
            x => x,
        };
        self.view.time = x;
        self.time_ease = ease;
        let px = self.time_px(model);
        let shown = self.view.shared(model);
        self.follow = live::follow(shown, model.newest(), self.follow, px, FollowChange::Moved).on;
        self.sync_linked(model);
    }

    /// Put back an undo step: the time (eased), each panel's view (its reference kept), full
    /// size and following.
    fn restore(&mut self, model: &DashModel, snap: DashSnap, now: u64) {
        let follow = snap.follow;
        for (sub, v) in self.subs.iter_mut().zip(snap.views) {
            sub.view = View {
                reference: sub.view.reference.clone(),
                ..v
            };
            sub.ease = None;
            sub.rehover = true;
        }
        self.view.full = snap.full;
        self.set_time(model, snap.time, now, true);
        self.follow = follow;
    }

    /// A scroll burst ends: one undo step for what it did.
    pub fn end_burst(&mut self) -> bool {
        if self.burst.take().is_none() {
            return false;
        }
        for s in &mut self.subs {
            s.burst = None;
            s.carry = (0.0, 0.0);
        }
        if let Some(before) = self.before.take()
            && before != self.snapshot()
        {
            self.undo.push(before);
        }
        true
    }

    /// The hover as the dashboard draws it: the panel under the pointer (or the one whose
    /// press is under way), the pointer in the area, what it's on as its figure has it, the
    /// moment on the shared time over a linked panel, and a brush being drawn.
    fn hover(&self, model: &DashModel, layout: &DashLayout) -> Option<DashHover> {
        let i = self.held.or(self.hovered)?;
        let sub = self.subs.get(i)?;
        let local = sub.pointer?;
        let pl = layout.panel(i)?;
        let pm = model.panels.get(i)?;
        let (ox, oy) = pl.origin();
        let brush = sub
            .brush_shape()
            .map(|(_, a, b)| ((a.0 + ox, a.1 + oy), (b.0 + ox, b.1 + oy)));
        let mut time = None;
        let target = match sub.hit_at(local) {
            Hit::PanelTitle(_) => PanelHover::Title,
            Hit::Badge => PanelHover::Badge,
            Hit::ErrorFigure | Hit::LogEntry(_) => PanelHover::Error,
            _ => match &pm.state {
                PanelState::Chart(cm) => {
                    if pm.linked
                        && let Some((cl, _)) = pl.chart()
                        && brush.is_none()
                        && sub.hovering
                    {
                        let snapped = (sub.hover.as_ref())
                            .filter(|c| matches!(c, Cursor::Point { .. }))
                            .and_then(|c| plot::nav::cursor_data(cm, c))
                            .map(|(x, _)| x);
                        let inside = cl.plot.contains(local.0, local.1);
                        time = snapped.or_else(|| inside.then(|| cl.x.invert(local.0)));
                    }
                    PanelHover::Chart {
                        cursor: sub.hover.clone(),
                        emphasis: sub.legend.clone().or_else(|| sub.near.clone()),
                    }
                }
                PanelState::Graph(_) => PanelHover::Graph(sub.target),
                PanelState::Tile(_) => {
                    let th = self.tile_hover.filter(|(j, _)| *j == i).map(|(_, h)| h);
                    if pm.linked
                        && let (Some(TileHover::Trend { point, .. }), Some(tm)) = (th, pl.tile())
                    {
                        time = tm.trend.get(point).map(|p| p.x);
                    }
                    PanelHover::Tile(th)
                }
                _ => PanelHover::Nothing,
            },
        };
        Some(DashHover {
            panel: i,
            at: (local.0 + ox, local.1 + oy),
            target,
            time,
            brush,
        })
    }
}

/// Whether a cursor names a sample, a band or a cell the chart has.
fn cursor_fits(m: &crate::plot::ChartModel, c: &Cursor) -> bool {
    match c {
        Cursor::Point { series, row } => (m.series.iter())
            .find(|s| &s.key == series)
            .is_some_and(|s| (*row as usize) < s.xs.len()),
        Cursor::Band { band, .. } => (*band as usize) < m.x.bands.len().max(m.y.bands.len()),
        Cursor::Cell { row, col } => {
            (*row as usize) < m.y.bands.len() && (*col as usize) < m.x.bands.len()
        }
        Cursor::Node(_) => false,
    }
}

/// Whether panel `i` is a chart on the shared time.
fn is_linked_chart(model: &DashModel, i: usize) -> bool {
    model
        .panels
        .get(i)
        .is_some_and(|p| p.linked && p.chart().is_some())
}

/// Lend panel `i`'s laid-out figure to its pane, for an act or a hover; [`take_back`] returns
/// it.
fn lend(layout: &mut DashLayout, i: usize, sub: &mut Pane) {
    let Some(p) = layout.panels.get_mut(i).and_then(Option::as_mut) else {
        return;
    };
    match std::mem::replace(&mut p.figure, PanelFigure::Failed) {
        PanelFigure::Chart { layout, hits } => {
            sub.layout = Some(layout);
            sub.hits = Some(hits);
        }
        PanelFigure::Graph { layout } => sub.graph = Some(layout),
        other => p.figure = other,
    }
}

/// Take panel `i`'s figure back from its pane.
fn take_back(layout: &mut DashLayout, i: usize, sub: &mut Pane) {
    let (l, h, g) = (sub.layout.take(), sub.hits.take(), sub.graph.take());
    let Some(p) = layout.panels.get_mut(i).and_then(Option::as_mut) else {
        return;
    };
    match (l, h, g) {
        (Some(layout), Some(hits), _) => p.figure = PanelFigure::Chart { layout, hits },
        (_, _, Some(layout)) => p.figure = PanelFigure::Graph { layout },
        _ => {}
    }
}

/// A panel's regions in its figure's own px: what its pane's pointer asks.
fn regions_of(p: &PanelLayout) -> Scene {
    let mut s = Scene::new(p.frame.w, p.frame.h, p.scene.scale);
    for r in &p.scene.regions {
        let rect = Rect::new(r.rect.x - p.body.x, r.rect.y - p.body.y, r.rect.w, r.rect.h);
        s.region(rect, r.hit.clone());
    }
    s
}

impl DashPane {
    /// A pointer event on the dashboard: see the module's docs. `arm` sets the timer that ends a
    /// scroll burst.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pointer(
        &mut self,
        model: &DashModel,
        toggles: &[Toggles],
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
        now: Duration,
        cell: CellPixels,
        kit: &mut Kit,
        arm: &mut dyn FnMut() -> Sub,
    ) -> Then {
        let Some(mut layout) = self.layout.take() else {
            return Then::None;
        };
        let then = self.pointer_on(
            &mut layout,
            model,
            toggles,
            p,
            at,
            swallow,
            now,
            cell,
            kit,
            arm,
        );
        self.layout = Some(layout);
        then
    }

    #[allow(clippy::too_many_arguments)]
    fn pointer_on(
        &mut self,
        layout: &mut DashLayout,
        model: &DashModel,
        toggles: &[Toggles],
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
        now: Duration,
        cell: CellPixels,
        kit: &mut Kit,
        arm: &mut dyn FnMut() -> Sub,
    ) -> Then {
        let local = |layout: &DashLayout, i: usize| match layout.panel(i) {
            Some(pl) => pl.local(at.0, at.1),
            None => at,
        };
        match p.kind {
            PointerKind::Move => {
                if self.held.is_some() || self.page != Page::None {
                    return Then::None;
                }
                let hit = dashboard::hit(layout, at.0, at.1);
                let over = match &hit {
                    Some(DashHit::Panel { panel, at, .. }) => Some((*panel, *at)),
                    _ => None,
                };
                self.hover_to(over);
                // A log's entry under the pointer is marked, as alone.
                if let Some(DashHit::Panel {
                    panel,
                    hit: Hit::LogEntry(e),
                    ..
                }) = hit
                    && let Some(pv) = self.view.panels.get_mut(panel)
                    && let ErrorView::Log { top, .. } = pv.error
                {
                    pv.error = ErrorView::Log {
                        top,
                        mark: e as usize,
                    };
                }
                if let Some((i, at)) = over
                    && let Some(tm) = layout.panel(i).and_then(|pl| pl.tile())
                {
                    let rect = layout.panel(i).map_or(Rect::default(), |pl| {
                        Rect::new(0.0, 0.0, pl.frame.w, pl.frame.h)
                    });
                    let scale = layout.scale;
                    let th = measured(kit.book.as_mut(), scale, |m| {
                        figure::draw::hit_with(tm, rect, at, m)
                    });
                    self.tile_hover = th.map(|h| (i, h));
                }
                Then::None
            }
            PointerKind::Press(b @ (Button::Left | Button::Middle)) => {
                if let Some(i) = self.held {
                    // A second button mid-drag, or the same again after a lost release: the
                    // panel's pane knows which.
                    let l = local(layout, i);
                    return self.on_panel(layout, model, toggles, i, p, l, false, now, kit, arm);
                }
                if swallow {
                    self.page = Page::Swallowed;
                    return Then::None;
                }
                self.end_burst();
                match dashboard::hit(layout, at.0, at.1) {
                    Some(DashHit::Chip) => self.page = Page::Chip(at),
                    Some(DashHit::Live) => self.page = Page::Live(at),
                    Some(DashHit::Scrollbar { thumb }) => {
                        if !thumb && let Some((track, th)) = layout.scrollbar {
                            // On the track: the thumb's middle goes there.
                            let free = (track.h - th.h).max(1.0);
                            let k = ((at.1 - track.y - th.h / 2.0) / free).clamp(0.0, 1.0);
                            self.view.scroll = (k * layout.max_scroll).round();
                        }
                        self.page = Page::Thumb {
                            from: at.1,
                            then: self.view.scroll,
                        };
                    }
                    Some(DashHit::Panel {
                        panel: i,
                        hit,
                        at: l,
                    }) => {
                        return self
                            .press_panel(layout, model, toggles, i, hit, l, p, b, now, kit, arm);
                    }
                    _ => self.page = Page::Swallowed,
                }
                Then::None
            }
            PointerKind::Press(Button::Right) => Then::None,
            PointerKind::Drag(_) => {
                if let Some(i) = self.held {
                    let l = local(layout, i);
                    return self.on_panel(layout, model, toggles, i, p, l, false, now, kit, arm);
                }
                if let Page::Thumb { from, then } = self.page {
                    self.view.scroll = layout.drag_scroll(from, at.1, then).round();
                }
                Then::None
            }
            PointerKind::Release(b) => {
                if let Some(i) = self.held {
                    let l = local(layout, i);
                    let then = self.on_panel(layout, model, toggles, i, p, l, false, now, kit, arm);
                    if self.subs.get(i).is_none_or(|s| s.held.is_none()) {
                        self.held = None;
                    }
                    self.catch_up(model);
                    let _ = b;
                    return then;
                }
                let page = std::mem::replace(&mut self.page, Page::None);
                let near = |a: (f32, f32)| (a.0 - at.0).hypot(a.1 - at.1) < CLICK_SLOP;
                match (page, dashboard::hit(layout, at.0, at.1)) {
                    (Page::Chip(a), Some(DashHit::Chip)) if near(a) => Then::Range,
                    (Page::Live(a), Some(DashHit::Live)) if near(a) => Then::Rerun,
                    (
                        Page::Error { panel, at: a },
                        Some(DashHit::Panel {
                            panel: q,
                            hit: Hit::ErrorFigure,
                            ..
                        }),
                    ) if near(a) && q == panel => {
                        self.flip_error(model, panel);
                        Then::None
                    }
                    _ => Then::None,
                }
            }
            PointerKind::Scroll { dx, dy } => self.scroll(
                layout, model, toggles, p, at, dx, dy, p.mods, now, cell, kit, arm,
            ),
        }
    }

    /// A press on panel `i`: it takes the focus; a double click on its title or a tile shows it
    /// full size and back, on a linked plot puts the shared time back to its range; a click on a
    /// tile puts its cursor on the trend's newest point; on a title, the focus is all; on a
    /// chart or a graph, the press is its pane's.
    #[allow(clippy::too_many_arguments)]
    fn press_panel(
        &mut self,
        layout: &mut DashLayout,
        model: &DashModel,
        toggles: &[Toggles],
        i: usize,
        hit: Hit,
        l: (f32, f32),
        p: &SurfacePointer,
        b: Button,
        now: Duration,
        kit: &mut Kit,
        arm: &mut dyn FnMut() -> Sub,
    ) -> Then {
        self.focus(i);
        let pm = &model.panels[i];
        let tile = pm.tile().is_some() || matches!(pm.kind, dashboard::PanelKind::Tile);
        let title = matches!(hit, Hit::PanelTitle(_));
        if p.clicks >= 2 && b == Button::Left {
            if title || tile {
                let before = self.snapshot();
                self.view.full = match self.view.full {
                    Some(_) => None,
                    None => Some(i),
                };
                self.undo.push(before);
                self.page = Page::Swallowed;
                return Then::None;
            }
            let plot = matches!(hit, Hit::Plot(_) | Hit::Bar { .. } | Hit::Cell { .. });
            if plot && is_linked_chart(model, i) {
                let before = self.snapshot();
                let time = self.view.reset_time(model).time;
                if time != self.view.time {
                    self.undo.push(before);
                }
                self.set_time(model, time, ms(now), true);
                self.follow = true;
                self.page = Page::Swallowed;
                return Then::None;
            }
        }
        // A panel's error figure answers as alone: the badge opens its log over the figure, a
        // click on the symbolic figure or the log's header turns it over, a press on an entry
        // marks it and a double click goes to its place.
        match hit {
            Hit::Badge if b == Button::Left => {
                self.flip_error(model, i);
                self.page = Page::Swallowed;
                return Then::None;
            }
            Hit::ErrorFigure if b == Button::Left => {
                let o = layout.panel(i).map_or((0.0, 0.0), |pl| pl.origin());
                self.page = Page::Error {
                    panel: i,
                    at: (l.0 + o.0, l.1 + o.1),
                };
                return Then::None;
            }
            Hit::LogEntry(e) if b == Button::Left => {
                let e = e as usize;
                self.page = Page::Swallowed;
                if let Some(pv) = self.view.panels.get_mut(i)
                    && let ErrorView::Log { top, mark } = pv.error
                {
                    pv.error = ErrorView::Log { top, mark: e };
                    if p.clicks >= 2 && mark == e {
                        return Then::GotoError(Some(e));
                    }
                }
                return Then::None;
            }
            _ if self.error_shown(model, i) => {
                self.page = Page::Swallowed;
                return Then::None;
            }
            _ => {}
        }
        if tile {
            if let Some(tm) = layout.panel(i).and_then(|pl| pl.tile())
                && !tm.trend.is_empty()
            {
                self.tile_points[i] = Some(tm.trend.len() - 1);
            }
            self.page = Page::Swallowed;
            return Then::None;
        }
        if title || pm.chart().is_none() && pm.graph().is_none() {
            self.page = Page::Swallowed;
            return Then::None;
        }
        self.before = Some(self.snapshot());
        let then = self.on_panel(layout, model, toggles, i, p, l, false, now, kit, arm);
        self.held = self.subs[i].held.map(|_| i);
        then
    }

    /// A pointer event for panel `i`'s pane, at `l` in its figure's px: as its figure alone has
    /// it, its layout lent; then the dashboard's share of what it did ([`DashPane::after_act`]).
    #[allow(clippy::too_many_arguments)]
    fn on_panel(
        &mut self,
        layout: &mut DashLayout,
        model: &DashModel,
        toggles: &[Toggles],
        i: usize,
        p: &SurfacePointer,
        l: (f32, f32),
        swallow: bool,
        now: Duration,
        kit: &mut Kit,
        arm: &mut dyn FnMut() -> Sub,
    ) -> Then {
        let before = self.snapshot();
        let shared = self.view.shared(model);
        let Some(sub) = self.subs.get_mut(i) else {
            return Then::None;
        };
        let had = sub.undo.undoable();
        if is_linked_chart(model, i) {
            sub.view.x = shared;
        }
        let t = toggles.get(i).copied().unwrap_or_default();
        lend(layout, i, sub);
        let mut open = None;
        match &model.panels[i].state {
            PanelState::Chart(cm) => {
                let mut a = Act {
                    pane: sub,
                    model: cm,
                    toggles: t,
                    motion: cm.chart.view.motion,
                    now,
                };
                a.pointer(p, l, swallow, &mut *arm);
            }
            PanelState::Graph(g) => {
                let mut a = GraphAct {
                    pane: sub,
                    model: g,
                    toggles: t,
                    motion: g.spec.view.motion,
                    now,
                    kit,
                    say: None,
                };
                open = a.pointer(p, l, swallow, &mut *arm);
            }
            _ => {}
        }
        take_back(layout, i, sub);
        self.after_act(model, i, before, had, shared);
        match open {
            Some(item) => Then::Open(item),
            None => Then::None,
        }
    }

    /// The wheel (mouse-ux §3.2, call C4): a burst keeps its first event's meaning. While the
    /// rows overflow, the wheel up and down scrolls the page, by whole physical pixels; across,
    /// with ⇧, or with ⌘, it's the panel's under the pointer: a chart's or a graph's as alone (a
    /// linked chart's the shared time's), and ⌘ on a linked tile zooms the shared time. When the
    /// rows fit, it's always the panel's.
    #[allow(clippy::too_many_arguments)]
    fn scroll(
        &mut self,
        layout: &mut DashLayout,
        model: &DashModel,
        toggles: &[Toggles],
        p: &SurfacePointer,
        at: (f32, f32),
        dx: f32,
        dy: f32,
        mods: Mods,
        now: Duration,
        cell: CellPixels,
        kit: &mut Kit,
        arm: &mut dyn FnMut() -> Sub,
    ) -> Then {
        if self.held.is_some() || !dx.is_finite() || !dy.is_finite() {
            return Then::None;
        }
        // Linux gives a mouse's Shift-wheel as up and down; macOS turns it across itself.
        let (sdx, sdy) = if mods.shift && dx == 0.0 {
            (dy, 0.0)
        } else {
            (dx, dy)
        };
        if self.burst.is_none() {
            let zoom = mods.super_ || mods.ctrl;
            let page = layout.max_scroll > 0.0 && !zoom && sdy.abs() >= sdx.abs();
            let panel = match dashboard::hit(layout, at.0, at.1) {
                Some(DashHit::Panel { panel, .. }) if !page => Some(panel),
                _ => None,
            };
            self.before = Some(self.snapshot());
            self.burst = Some(Burst {
                page,
                panel,
                zoom,
                _timer: None,
            });
            self.carry = 0.0;
        }
        let Some(b) = self.burst.as_ref() else {
            return Then::None;
        };
        let (page, panel, zoom) = (b.page, b.panel, b.zoom);
        let own_timer = |me: &mut DashPane, arm: &mut dyn FnMut() -> Sub| {
            if let Some(b) = me.burst.as_mut() {
                b._timer = Some(arm());
            }
        };
        if page {
            own_timer(self, arm);
            let s = layout.scale.max(0.01);
            let t = sdy * cell.height as f32 + self.carry;
            let whole = t.trunc();
            self.carry = t - whole;
            self.view.scroll = (self.view.scroll + whole / s).clamp(0.0, layout.max_scroll);
            return Then::None;
        }
        let Some(i) = panel else {
            own_timer(self, arm);
            return Then::None;
        };
        let pm = &model.panels[i];
        if self.error_shown(model, i) {
            own_timer(self, arm);
            // Its log scrolls by lines, the rest carried; the symbolic figure doesn't move.
            if let (Some(ErrorView::Log { top, mark }), Some((errors, name)), Some(pl)) = (
                self.view.panels.get(i).map(|v| v.error),
                pm.errors(),
                layout.panel(i),
            ) {
                let lines = plot::log_layout(&errors, "", &name);
                let page = plot::log_page((pl.body.w, pl.body.h));
                let rows = cell.height as f32 / layout.scale.max(0.01);
                let by = dy * rows / plot::errors::LOG_LINE + self.carry;
                let whole = by.trunc();
                self.carry = by - whole;
                let top = (top as f32 + whole).clamp(0.0, lines.max_top(page) as f32) as usize;
                self.view.panels[i].error = ErrorView::Log { top, mark };
            }
            return Then::None;
        }
        if pm.chart().is_some() || pm.graph().is_some() {
            let l = layout.panel(i).map_or(at, |pl| pl.local(at.0, at.1));
            return self.on_panel(layout, model, toggles, i, p, l, false, now, kit, arm);
        }
        own_timer(self, arm);
        // ⌘ over a linked tile zooms the shared time about its middle.
        if zoom
            && pm.linked
            && let Some((a, b)) = self.view.shared(model)
        {
            let factor = ZOOM_STEP.powf(f64::from(-dy) / 4.0);
            let (lo, hi) = model.extent().unwrap_or((a, b));
            let mid = (a + b) / 2.0;
            let half = ((b - a) / 2.0 / factor).clamp(0.5, ((hi - lo) / 2.0).max(0.5));
            let (mut x0, mut x1) = (mid - half, mid + half);
            if x0 < lo {
                (x0, x1) = (lo, lo + 2.0 * half);
            }
            if x1 > hi {
                (x0, x1) = (hi - 2.0 * half, hi);
            }
            let time = Some((x0.max(lo), x1.min(hi)));
            self.time_ease = None;
            self.view.time = time;
            let px = self.time_px(model);
            let shown = self.view.shared(model);
            self.follow =
                live::follow(shown, model.newest(), self.follow, px, FollowChange::Moved).on;
            self.sync_linked(model);
        }
        Then::None
    }

    /// End the gesture under way where it stands, as a key or the focus going ends it
    /// (mouse-ux §8); a scroll burst ends.
    fn finish(&mut self, model: &DashModel, toggles: &[Toggles], now: Duration, kit: &mut Kit) {
        self.end_burst();
        self.page = Page::None;
        let Some(i) = self.held.take() else { return };
        let Some(mut layout) = self.layout.take() else {
            return;
        };
        let before = self.snapshot();
        let shared = self.view.shared(model);
        let had = self.subs[i].undo.undoable();
        let t = toggles.get(i).copied().unwrap_or_default();
        let sub = &mut self.subs[i];
        lend(&mut layout, i, sub);
        match &model.panels[i].state {
            PanelState::Chart(cm) => Act {
                pane: sub,
                model: cm,
                toggles: t,
                motion: cm.chart.view.motion,
                now,
            }
            .finish(),
            PanelState::Graph(g) => GraphAct {
                pane: sub,
                model: g,
                toggles: t,
                motion: g.spec.view.motion,
                now,
                kit,
                say: None,
            }
            .finish(),
            _ => {}
        }
        sub.held = None;
        take_back(&mut layout, i, sub);
        self.layout = Some(layout);
        self.after_act(model, i, before, had, shared);
        self.catch_up(model);
    }

    /// `Esc` (mouse-ux §8): a brush is dropped, a pan or a node's drag goes back; else the full
    /// size goes, then the focused panel's reference. False with nothing to cancel.
    fn cancel(&mut self, model: &DashModel) -> bool {
        self.end_burst();
        if let Some(i) = self.held.take() {
            let sub = &mut self.subs[i];
            match std::mem::replace(&mut sub.gesture, Gesture::None) {
                Gesture::Pan { before, .. }
                | Gesture::AxisPan { before, .. }
                | Gesture::NodeDrag { before, .. } => {
                    sub.view = before;
                    sub.ease = None;
                }
                _ => {}
            }
            (sub.gesture, sub.frozen, sub.held) = (Gesture::Swallowed, None, None);
            sub.rehover = true;
            self.before = None;
            if is_linked_chart(model, i) {
                let x = self.subs[i].view.x;
                if x != self.view.shared(model) {
                    self.view.time = x;
                    self.sync_linked(model);
                }
            }
            self.catch_up(model);
            return true;
        }
        // A search's find goes before full size does, as it goes before the detail alone.
        if let Some(sub) = self.view.focused.and_then(|f| self.subs.get_mut(f))
            && (sub.found.is_some() || sub.found_band.is_some() || sub.found_node.is_some())
        {
            (sub.found, sub.found_band, sub.found_node) = (None, None, None);
            (sub.matches, sub.lit, sub.lit_bands) = (vec![], vec![], vec![]);
            sub.rehover = true;
            return true;
        }
        if self.view.full.take().is_some() {
            return true;
        }
        if let Some(sub) = self.view.focused.and_then(|f| self.subs.get_mut(f))
            && sub.view.reference.take().is_some()
        {
            return true;
        }
        false
    }
}

// ------------------------------------------------------------------ drawing

/// Draw a dashboard window: its base where what it shows changed, the hover worked out again
/// where the view moved, and its overlay where what it shows changed. Whether anything eases.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    p: &mut Pane,
    d: &mut DashState,
    live: Option<LiveStatus>,
    gens: (u64, u64),
    failed: Option<&Failed>,
    theme: &Theme,
    kit: &mut Kit,
    now: u64,
) -> bool {
    let Some(model) = d.model.as_mut() else {
        return false;
    };
    let mut dp = match p.dash.take() {
        Some(dp) => dp,
        None => {
            let mut dp = Box::new(DashPane::new(model));
            if let Some(seed) = d.seed.take() {
                restore_view(&mut dp, model, &seed);
            }
            dp
        }
    };
    // A live dashboard not following says so where it says it's live.
    model.live = match (live, dp.follow) {
        (Some(l), false) if matches!(l.state, LiveState::Live | LiveState::Updating) => {
            Some(LiveStatus::paused())
        }
        (l, _) => l,
    };
    let model: &DashModel = model;
    for s in &mut dp.subs {
        s.cell = p.cell;
        if let Some(e) = &s.ease
            && e.tween.done(now)
        {
            s.ease = None;
            s.rehover = true;
        }
    }
    if dp.time_ease.as_ref().is_some_and(|t| t.done(now)) {
        dp.time_ease = None;
        for s in &mut dp.subs {
            s.rehover = true;
        }
    }
    // The detail panel follows the focused panel's cursor and its data.
    if p.panel.is_some()
        && let Some(f) = dp.view.focused
        && let (Some(pm), Some(sub)) = (model.panels.get(f), dp.subs.get(f))
    {
        p.cursor = sub.cursor.clone();
        p.view = View {
            x: None,
            y: None,
            ..sub.view.clone()
        };
        let tile = (dp.layout.as_ref())
            .and_then(|l| l.panel(f))
            .and_then(|pl| pl.tile())
            .or_else(|| pm.tile().map(|t| &t.model));
        let src = super::panel::Source {
            chart: pm.chart(),
            graph: pm.graph(),
            tile,
        };
        p.sync_panel(src, d.version);
    }
    let mut shown = dp.shown(now);
    // The detail panel's row under the pointer brings its series forward in the focused panel.
    if let (Some(f), Some(k)) = (
        shown.focused,
        p.row_hover.as_ref().and_then(|i| i.series().cloned()),
    ) && let Some(pv) = shown.panels.get_mut(f)
    {
        pv.emphasis = Some(k);
    }
    let scale = p.scale();
    let key = DashKey {
        version: d.version,
        fail: gens.1,
        theme: gens.0,
        size: p.area_px(),
        scale: scale.to_bits(),
        time: shown.time,
        views: shown.panels.iter().map(|v| v.view.clone()).collect(),
        errors: shown.panels.iter().map(|v| v.error).collect(),
        focused: shown.focused,
        full: shown.full,
        scroll: shown.scroll.to_bits(),
        live: model.live.clone(),
        detail: p.panel.is_some(),
    };
    let panel_key = super::panel::panel_key_of(p, gens.0);
    if dp.drawn.as_ref() != Some(&key) {
        draw_base(p, &mut dp, model, &shown, failed, theme, kit);
        dp.drawn = Some(key);
        p.panel_drawn = panel_key;
    } else if p.panel_drawn != panel_key {
        super::panel::draw_panel_only(p, theme, kit);
        p.panel_drawn = panel_key;
    }
    // The hover worked out again where the view moved under a still pointer.
    if let Some(mut layout) = dp.layout.take() {
        for i in 0..dp.subs.len() {
            if !dp.subs[i].rehover {
                continue;
            }
            let sub = &mut dp.subs[i];
            lend(&mut layout, i, sub);
            match &model.panels[i].state {
                PanelState::Chart(cm) => sub.hover_again(Some(cm)),
                PanelState::Graph(g) => sub.graph_hover(g),
                _ => sub.rehover = false,
            }
            take_back(&mut layout, i, sub);
        }
        dp.layout = Some(layout);
    }
    draw_overlay(p, &mut dp, model, &shown, theme, kit);
    let easing = dp.easing();
    p.dash = Some(dp);
    easing
}

/// The base: the dashboard laid out (beside the detail panel when it's open), drawn opaque,
/// presented whole; the stale badge while the spec's last build failed.
fn draw_base(
    p: &mut Pane,
    dp: &mut DashPane,
    model: &DashModel,
    shown: &DashView,
    failed: Option<&Failed>,
    theme: &Theme,
    kit: &mut Kit,
) {
    let scale = p.scale();
    let size = p.size();
    let fsize = p.figure_size();
    // The card's side, kept across redraws (its hysteresis).
    let flipped: Vec<Option<bool>> = (0..model.panels.len())
        .map(|i| {
            (dp.layout.as_ref())
                .and_then(|l| l.panel(i))
                .and_then(|pl| pl.chart())
                .map(|(c, _)| c.flipped.get())
        })
        .collect();
    let (mut scene, layout) = measured(kit.book.as_mut(), scale, |m| {
        let layout = dashboard::layout(model, shown, fsize, scale, theme, m);
        let mut scene = dashboard::base(&layout, shown, theme, m);
        if let Some(f) = failed {
            plot::stale_badge(&mut scene, f.first(), theme, m);
        }
        (scene, layout)
    });
    for (i, f) in flipped.into_iter().enumerate() {
        if let (Some(f), Some((c, _))) = (f, layout.panel(i).and_then(|pl| pl.chart())) {
            c.flipped.set(f);
        }
    }
    let cw = p.cell.width as f32 / scale;
    for (i, sub) in dp.subs.iter_mut().enumerate() {
        sub.scene = layout.panel(i).map(regions_of);
        sub.rehover = true;
    }
    if layout.full.is_none() {
        dp.order = layout.lines.iter().flatten().copied().collect();
    }
    let mut layout = layout;
    for pl in layout.panels.iter_mut().flatten() {
        if let PanelFigure::Chart { layout: c, .. } = &mut pl.figure {
            c.cell_width = cw;
        }
        if let PanelFigure::Graph { layout: g } = &mut pl.figure {
            g.cell_width = cw;
        }
    }
    (scene.width, scene.height) = size;
    p.fig_regions = scene.regions.len();
    if let Some(panel) = super::panel::panel_scene(p, theme, kit) {
        scene.items.extend(panel.items);
        scene.regions.extend(panel.regions);
    }
    let (aw, ah) = p.area_px();
    if let Some(producer) = p.base.as_mut() {
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
    }
    scene.items.clear();
    p.scene = Some(scene);
    p.drawn = None;
    p.layout = None;
    p.graph = None;
    dp.layout = Some(layout);
    p.base_gen += 1;
}

/// The overlay where what it shows changed: the hovered panel's own overlay, every cursor, the
/// linked moment and brush (dashboard's), the axis strip under the pointer, a tile's cursor,
/// and the key card; the union of what it drew and draws presented alone (ruling 10).
fn draw_overlay(
    p: &mut Pane,
    dp: &mut DashPane,
    model: &DashModel,
    shown: &DashView,
    theme: &Theme,
    kit: &mut Kit,
) {
    let Some(layout) = dp.layout.as_ref() else {
        return;
    };
    let hover = dp.hover(model, layout);
    let strip = (dp.hovered).and_then(|i| {
        let s = dp.subs.get(i)?.strip?;
        let (ox, oy) = layout.panel(i)?.origin();
        Some(Rect::new(s.x + ox, s.y + oy, s.w, s.h))
    });
    let key = OverKey {
        base: p.base_gen,
        hover: hover.clone(),
        cursors: shown.panels.iter().map(|v| v.cursor.clone()).collect(),
        emphasis: shown.panels.iter().map(|v| v.emphasis.clone()).collect(),
        tiles: dp.tile_points.clone(),
        help: p.help,
        strip,
    };
    if dp.over_key.as_ref() == Some(&key) {
        return;
    }
    let scale = p.scale();
    let size = p.size();
    let focused = shown.focused;
    let mut scene = measured(kit.book.as_mut(), scale, |m| {
        let mut scene = dashboard::overlay(model, layout, shown, hover.as_ref(), theme, m);
        // A tile's cursor on its trend where the pointer isn't: its mini crosshair, and its
        // card on the focused tile.
        for (i, point) in dp.tile_points.iter().enumerate() {
            let (Some(point), Some(pl)) = (point, layout.panel(i)) else {
                continue;
            };
            let Some(tm) = pl.tile() else { continue };
            if hover.as_ref().is_some_and(|h| h.panel == i) {
                continue;
            }
            let th = TileHover::Trend {
                point: (*point).min(tm.trend.len().saturating_sub(1)),
                card: focused == Some(i),
            };
            let rect = Rect::new(0.0, 0.0, pl.frame.w, pl.frame.h);
            let (ox, oy) = pl.origin();
            let area = Rect::new(-ox, -oy, size.0, size.1);
            let over = figure::overlay_in(tm, Some(th), rect, area, theme, m);
            dashboard::draw::put(&mut scene, over, (ox, oy), None);
        }
        scene
    });
    (scene.width, scene.height, scene.scale) = (size.0, size.1, scale);
    if let Some(r) = strip {
        scene.items.insert(
            0,
            SceneItem::Path {
                path: PathData::rect(r),
                fill: Some(Paint::solid(theme.text, 0.06)),
                stroke: None,
            },
        );
    }
    if p.help {
        measured(kit.book.as_mut(), scale, |m| {
            key_card(&mut scene, &DASH_KEY_CARD, theme, m)
        });
    }
    let now = scene.bounds();
    let damage = changed(&p.over_items, &scene.items, p.over_bounds, now);
    p.over_bounds = now;
    dp.over_key = Some(key);
    p.over_items = scene.items.clone();
    p.over_key = None;
    let (aw, ah) = p.area_px();
    let wipe = std::mem::take(&mut p.over_wipe);
    let Some(producer) = p.over.as_mut() else {
        return;
    };
    let (w, h, stride, px) = target(producer);
    let b = match (wipe, damage) {
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

// ------------------------------------------------------------------ the keys

/// A tile's keys: `h` `l` along its trend, `gg` `G` to its ends; nothing else moves on it.
struct TileKeys<'a> {
    point: &'a mut Option<usize>,
    len: usize,
}

impl Keys for TileKeys<'_> {
    fn move_cursor(&mut self, action: &Action, count: u32) -> bool {
        if self.len == 0 {
            return false;
        }
        let last = self.len - 1;
        let at = self.point.unwrap_or(last);
        let n = count.max(1) as usize;
        let to = match action {
            Action::Step(by) if *by < 0 => at.saturating_sub(n),
            Action::Step(_) => (at + n).min(last),
            Action::First | Action::ViewEdge(-1) => 0,
            Action::Last | Action::ViewEdge(1) => last,
            Action::ViewEdge(_) => last / 2,
            _ => return false,
        };
        let moved = *self.point != Some(to);
        *self.point = Some(to);
        moved
    }
    fn zoom_keys(&mut self, _: f64) {}
    fn scroll_keys(&mut self, _: f32, _: f32) {}
    fn centre(&mut self) {}
    fn fit(&mut self) {}
    fn hide(&mut self) -> bool {
        false
    }
    fn only(&mut self) -> bool {
        false
    }
    fn show_all(&mut self) {}
    fn reference(&mut self) -> bool {
        false
    }
    fn to_reference(&mut self) -> bool {
        false
    }
}

impl FigState {
    /// Whether the figure is a dashboard drawn in a window, its log not up.
    pub fn dash_drawn(&self, w: die_core::WindowId) -> bool {
        self.dash.as_ref().is_some_and(|d| d.model.is_some())
            && self
                .panes
                .get(&w)
                .is_some_and(|p| p.dash.is_some() && p.log.is_none())
    }

    /// A pointer event on a dashboard's window, past the detail panel: see the module's docs.
    pub(super) fn dash_pointer(
        &mut self,
        ctx: &mut die_core::Ctx,
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
    ) {
        let now = ctx.now();
        let w = p.window;
        let me = self.me.clone();
        let kit = self.kit.clone();
        let then = {
            let FigState { dash, panes, .. } = &mut *self;
            let (Some(d), Some(pane)) = (dash.as_mut(), panes.get_mut(&w)) else {
                return;
            };
            let Some(model) = d.model.as_ref() else {
                return;
            };
            let Some(mut dp) = pane.dash.take() else {
                return;
            };
            if p.kind == PointerKind::Move {
                pane.hovering = true;
            }
            let cell = pane.cell;
            let mut kit = kit.borrow_mut();
            let mut arm = || {
                let me = me.clone();
                ctx.after(
                    Duration::from_millis(crate::interact::consts::SCROLL_BURST_MS),
                    move |ctx| {
                        if let Some(s) = me.upgrade() {
                            let mut st = s.borrow_mut();
                            st.end_burst(w);
                            st.refresh(ctx);
                        }
                        Ok(())
                    },
                )
            };
            let then = dp.pointer(
                model, &d.toggles, p, at, swallow, now, cell, &mut kit, &mut arm,
            );
            pane.dash = Some(dp);
            then
        };
        match then {
            Then::None => {}
            Then::Range => {
                if let Some(s) = me.upgrade() {
                    time_prompt(ctx, s, w);
                }
            }
            Then::Rerun => self.rerun(ctx),
            Then::Open(item) => {
                if let Err(why) = self.open_panel(w, Some(item)) {
                    ctx.messages.push(why);
                }
            }
            Then::GotoError(entry) => {
                let args = die_core::Args {
                    window: Some(w),
                    buffer: self.buf,
                    count: entry.map(|e| e + 1),
                    ..die_core::Args::default()
                };
                ctx.defer(move |ctx| ctx.run("fig-goto-error", &args));
            }
        }
    }

    /// The pointer left a dashboard's window: every panel's hover goes.
    pub(super) fn dash_left(&mut self, w: die_core::WindowId) {
        if let Some(dp) = self.panes.get_mut(&w).and_then(|p| p.dash.as_mut()) {
            dp.hover_to(None);
            dp.tile_hover = None;
        }
    }

    /// End a dashboard window's gesture where it stands (a key, the focus going).
    pub(super) fn dash_finish(&mut self, w: die_core::WindowId, now: Duration) {
        let kit = self.kit.clone();
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return;
        };
        dp.finish(model, &d.toggles, now, &mut kit.borrow_mut());
    }

    /// The scroll burst in a dashboard's window ends: one undo step.
    pub(super) fn dash_end_burst(&mut self, w: die_core::WindowId) -> bool {
        match self.panes.get_mut(&w).and_then(|p| p.dash.as_mut()) {
            Some(dp) => dp.end_burst(),
            None => false,
        }
    }

    /// `Esc` on a dashboard: its gesture, full size, the focused panel's reference. False with
    /// nothing.
    pub(super) fn dash_cancel(&mut self, w: die_core::WindowId) -> bool {
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return false;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return false;
        };
        dp.cancel(model)
    }

    /// A key's action on the dashboard's focused panel (the first that draws a chart, a graph or
    /// a tile takes the focus if none has it): as its figure alone has it, its layout lent; a
    /// linked chart's moves are the shared time's, and its undo step the dashboard's.
    pub(super) fn dash_keys<R>(
        &mut self,
        w: die_core::WindowId,
        now: Duration,
        f: impl FnOnce(&mut dyn Keys) -> R,
    ) -> Option<R> {
        let kit = self.kit.clone();
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return None;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return None;
        };
        let i = match dp.view.focused {
            Some(i) => i,
            None => {
                let i = (0..model.panels.len()).find(|&i| model.panels[i].ready())?;
                dp.focus(i);
                i
            }
        };
        // A key in the focused panel is the last input: another panel's hover and card go
        // until the pointer moves (one card at a time, mouse-ux §3.3).
        if dp.hovered.is_some_and(|h| h != i) {
            dp.drop_hover();
        }
        let mut layout = dp.layout.take()?;
        let before = dp.snapshot();
        let shared = dp.view.shared(model);
        let t = d.toggles.get(i).copied().unwrap_or_default();
        let had = dp.subs.get(i)?.undo.undoable();
        if is_linked_chart(model, i) {
            dp.subs[i].view.x = shared;
        }
        let r = match &model.panels[i].state {
            PanelState::Chart(cm) => {
                let sub = &mut dp.subs[i];
                lend(&mut layout, i, sub);
                let mut a = Act {
                    pane: sub,
                    model: cm,
                    toggles: t,
                    motion: cm.chart.view.motion,
                    now,
                };
                let r = f(&mut a);
                take_back(&mut layout, i, sub);
                Some(r)
            }
            PanelState::Graph(g) => {
                let sub = &mut dp.subs[i];
                lend(&mut layout, i, sub);
                let mut kit = kit.borrow_mut();
                let mut a = GraphAct {
                    pane: sub,
                    model: g,
                    toggles: t,
                    motion: g.spec.view.motion,
                    now,
                    kit: &mut kit,
                    say: None,
                };
                let r = f(&mut a);
                take_back(&mut layout, i, sub);
                Some(r)
            }
            PanelState::Tile(_) => {
                let len = layout
                    .panel(i)
                    .and_then(|pl| pl.tile())
                    .map_or(0, |tm| tm.trend.len());
                let mut k = TileKeys {
                    point: &mut dp.tile_points[i],
                    len,
                };
                Some(f(&mut k))
            }
            _ => None,
        };
        dp.layout = Some(layout);
        dp.after_act(model, i, before, had, shared);
        self.rev += 1;
        r
    }

    /// `u` or `C-r` on a dashboard: the step before or after, eased. False with none.
    pub(super) fn dash_undo(&mut self, w: die_core::WindowId, now: Duration, redo: bool) -> bool {
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return false;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return false;
        };
        let current = dp.snapshot();
        let snap = match redo {
            true => dp.undo.redo(&current),
            false => dp.undo.undo(&current),
        };
        let Some(snap) = snap else { return false };
        dp.restore(model, snap, ms(now));
        self.rev += 1;
        true
    }

    /// `Tab` and `S-Tab` on a dashboard (ruling 7): the focus to the next or previous panel in
    /// reading order, then the detail panel's stop when it's open, and round; the panel brought
    /// into view, shown full size if one is.
    pub fn dash_focus_step(&mut self, w: die_core::WindowId, by: i32) -> bool {
        let n = match self.dash.as_ref().and_then(|d| d.model.as_ref()) {
            Some(m) => m.panels.len(),
            None => return false,
        };
        let Some(p) = self.panes.get_mut(&w) else {
            return false;
        };
        let detail = p.panel.is_some();
        let in_detail = p.panel_focused();
        let Some(dp) = p.dash.as_mut() else {
            return false;
        };
        if n == 0 {
            return false;
        }
        // Reading order on screen: the lines as laid out whole, top to bottom (kept while a
        // panel is full size, whose layout has only it).
        let mut order: Vec<usize> = match dp.order.is_empty() {
            false => dp.order.clone(),
            true => (dp.layout.as_ref())
                .filter(|l| l.full.is_none())
                .map(|l| l.lines.iter().flatten().copied().collect())
                .unwrap_or_default(),
        };
        for i in 0..n {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        let stops = order.len() + usize::from(detail);
        let here = match (in_detail, dp.view.focused) {
            (true, _) => Some(order.len()),
            (false, Some(f)) => order.iter().position(|&o| o == f),
            (false, None) => None,
        };
        let next = match here {
            None if by > 0 => 0,
            None => stops - 1,
            Some(h) => (h as i64 + i64::from(by.signum())).rem_euclid(stops as i64) as usize,
        };
        let to_detail = next == order.len();
        if let Some(panel) = p.panel.as_mut()
            && panel.state.focused != to_detail
        {
            panel.state.focused = to_detail;
            panel.rev += 1;
        }
        let Some(dp) = p.dash.as_mut() else {
            return false;
        };
        if !to_detail {
            let i = order[next];
            dp.focus(i);
            if dp.view.full.is_some() {
                dp.view.full = Some(i);
            }
        }
        self.rev += 1;
        true
    }

    /// `f`: the focused panel full size, or the dashboard again; one undo step. False with no
    /// panel focused.
    pub fn dash_full(&mut self, w: die_core::WindowId) -> bool {
        let Some(dp) = self.panes.get_mut(&w).and_then(|p| p.dash.as_mut()) else {
            return false;
        };
        let before = dp.snapshot();
        dp.view.full = match (dp.view.full, dp.view.focused) {
            (Some(_), _) => None,
            (None, Some(f)) => Some(f),
            (None, None) => return false,
        };
        dp.undo.push(before);
        self.rev += 1;
        true
    }

    /// `G` on a dashboard: the shared time slides to the newest and follows it (the focused
    /// chart's cursor to its last point too).
    pub fn dash_follow(&mut self, w: die_core::WindowId, now: Duration) {
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return;
        };
        let f = live::follow(
            dp.view.time,
            model.newest(),
            dp.follow,
            0.0,
            FollowChange::Resume,
        );
        dp.set_time(model, f.x, ms(now), true);
        dp.follow = true;
        self.rev += 1;
    }

    /// `!` on a dashboard: the error figure of the focused panel (else the one under the
    /// pointer) turned over, symbolic and log. False where neither shows one.
    pub fn dash_log(&mut self, w: die_core::WindowId) -> bool {
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return false;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return false;
        };
        // A failed panel's, or one kept with the stale badge (its log over its figure).
        let erred = |i: usize| model.panels.get(i).is_some_and(|p| p.errors().is_some());
        let Some(i) = [dp.view.focused, dp.hovered]
            .into_iter()
            .flatten()
            .find(|&i| erred(i))
        else {
            return false;
        };
        if !dp.flip_error(model, i) {
            return false;
        }
        self.rev += 1;
        true
    }

    /// The error a focused panel's error figure goes to (`⏎`, a double click): its log's entry
    /// `entry` (from 0), else its marked one, else its first. None unless the focused panel
    /// failed or has its log up over the figure it kept.
    pub fn dash_error(
        &self,
        w: die_core::WindowId,
        entry: Option<usize>,
    ) -> Option<crate::error::FigError> {
        if !self.dash_drawn(w) {
            return None;
        }
        let model = self.dash.as_ref()?.model.as_ref()?;
        let dp = self.panes.get(&w)?.dash.as_ref()?;
        let i = dp.view.focused?;
        if !dp.error_shown(model, i) {
            return None;
        }
        let (errors, _) = model.panels.get(i)?.errors()?;
        let mark = match dp.view.panels.get(i).map(|v| v.error) {
            Some(ErrorView::Log { mark, .. }) => Some(mark),
            _ => None,
        };
        let at = entry.or(mark).unwrap_or(0);
        errors.get(at).or(errors.first()).cloned()
    }

    /// Whether the focused panel's log is up in window `w`: its keys are the log's.
    pub fn dash_log_up(&self, w: die_core::WindowId) -> bool {
        let model = self.dash.as_ref().and_then(|d| d.model.as_ref());
        let dp = self.panes.get(&w).and_then(|p| p.dash.as_ref());
        match (model, dp, self.dash_drawn(w)) {
            (Some(m), Some(dp), true) => dp.view.focused.is_some_and(|f| {
                dp.error_shown(m, f)
                    && matches!(
                        dp.view.panels.get(f).map(|v| v.error),
                        Some(ErrorView::Log { .. })
                    )
            }),
            _ => false,
        }
    }

    /// A move in the focused panel's log, as the log's keys move in a figure's own
    /// ([`crate::log_step`]).
    pub fn dash_log_move(&mut self, w: die_core::WindowId, action: &Action, n: u32) {
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return;
        };
        let Some(i) = dp.view.focused else { return };
        let (Some(ErrorView::Log { top, mark }), Some((errors, name))) = (
            dp.view.panels.get(i).map(|v| v.error),
            model.panels.get(i).and_then(|p| p.errors()),
        ) else {
            return;
        };
        let body = (dp.layout.as_ref())
            .and_then(|l| l.panel(i))
            .map_or((0.0, 0.0), |pl| (pl.body.w, pl.body.h));
        let lines = plot::log_layout(&errors, "", &name);
        let page = plot::log_page(body);
        let (top, mark) = crate::log_step(&lines, page, (top, mark), action, n);
        dp.view.panels[i].error = ErrorView::Log { top, mark };
        self.rev += 1;
    }

    /// The dashboard's view of window `w` with a new shared time, eased, one undo step: a
    /// range chosen.
    pub fn dash_set_time(
        &mut self,
        w: die_core::WindowId,
        time: Option<(f64, f64)>,
        now: Duration,
    ) {
        let FigState { dash, panes, .. } = &mut *self;
        let (Some(d), Some(pane)) = (dash.as_ref(), panes.get_mut(&w)) else {
            return;
        };
        let (Some(model), Some(dp)) = (d.model.as_ref(), pane.dash.as_mut()) else {
            return;
        };
        if time == dp.view.time {
            return;
        }
        let before = dp.snapshot();
        dp.undo.push(before);
        dp.set_time(model, time, ms(now), true);
        self.rev += 1;
    }

    /// The focused panel's chart and its pane in window `w`, if a chart panel has the focus.
    pub fn dash_focused(&self, w: die_core::WindowId) -> Option<(usize, &Pane)> {
        let dp = self.panes.get(&w)?.dash.as_ref()?;
        let i = dp.view.focused?;
        Some((i, dp.subs.get(i)?))
    }

    /// The focused panel's figure in window `w`, for the detail panel: its chart or graph, and
    /// its pane. None with no chart or graph focused.
    pub(super) fn dash_source(
        &self,
        w: die_core::WindowId,
    ) -> Option<(super::panel::Source<'_>, &Pane, u64)> {
        let d = self.dash.as_ref()?;
        let model = d.model.as_ref()?;
        let (i, sub) = self.dash_focused(w)?;
        let pm = model.panels.get(i)?;
        // A tile's numbers as drawn (over the visible time), else as built.
        let tile = (self.panes.get(&w)?.dash.as_ref())
            .and_then(|dp| dp.layout.as_ref())
            .and_then(|l| l.panel(i))
            .and_then(|pl| pl.tile())
            .or_else(|| pm.tile().map(|t| &t.model));
        let src = super::panel::Source {
            chart: pm.chart(),
            graph: pm.graph(),
            tile,
        };
        (src.chart.is_some() || src.graph.is_some() || src.tile.is_some())
            .then_some((src, sub, d.version))
    }

    /// What the focused panel of window `w` is, for a key that doesn't apply to it to say so:
    /// `a stat`, `a meter`, `a heatmap`, `waiting`.
    pub fn dash_focused_kind(&self, w: die_core::WindowId) -> Option<&'static str> {
        let model = self.dash.as_ref()?.model.as_ref()?;
        let (i, _) = self.dash_focused(w)?;
        let p = model.panels.get(i)?;
        Some(match &p.state {
            PanelState::Chart(_) => "a chart",
            PanelState::Graph(_) => "a graph",
            PanelState::Tile(t) => match t.model.kind {
                figure::TileKind::Stat => "a stat",
                figure::TileKind::Meter => "a meter",
                figure::TileKind::Sparkline => "a sparkline",
            },
            PanelState::Waiting(_) => "still waiting for its data",
            PanelState::Failed(_) => "an error",
        })
    }

    /// The focused graph panel's settings (`s`): its number, its toggles and its spec.
    pub fn dash_graph_settings(
        &self,
        w: die_core::WindowId,
    ) -> Option<(
        usize,
        Toggles,
        crate::spec::GraphSpec,
        Option<crate::spec::Layout>,
    )> {
        let d = self.dash.as_ref()?;
        let (i, _) = self.dash_focused(w)?;
        let g = d.model.as_ref()?.panels.get(i)?.graph()?;
        let t = d.toggles.get(i).copied().unwrap_or_default();
        Some((i, t, spec_before(g, &t), Some(g.layout.clone())))
    }

    /// The focused graph panel's settings turned by `f`, its model built again; whether they
    /// changed.
    pub fn dash_graph_turn(
        &mut self,
        ctx: &mut die_core::Ctx,
        i: usize,
        f: impl FnOnce(&mut Toggles, &crate::spec::GraphSpec),
    ) -> bool {
        let Some(d) = self.dash.as_mut() else {
            return false;
        };
        let Some(g) = d.model.as_ref().and_then(|m| m.panels.get(i)?.graph()) else {
            return false;
        };
        let t = d.toggles.get(i).copied().unwrap_or_default();
        let spec = spec_before(g, &t);
        if d.toggles.len() <= i {
            d.toggles.resize(i + 1, Toggles::default());
        }
        let before = d.toggles[i];
        f(&mut d.toggles[i], &spec);
        if d.toggles[i] == before {
            return false;
        }
        self.rev += 1;
        self.dash_rebuild(ctx);
        true
    }

    /// `relayout` on the focused graph panel: every pin let go, one undo step. Whether one was.
    pub fn dash_unpin(&mut self, w: die_core::WindowId) -> bool {
        let Some(dp) = self.panes.get_mut(&w).and_then(|p| p.dash.as_mut()) else {
            return false;
        };
        let Some(i) = dp.view.focused else {
            return false;
        };
        let before = dp.snapshot();
        let Some(sub) = dp.subs.get_mut(i).filter(|s| !s.view.pinned.is_empty()) else {
            return false;
        };
        sub.view.pinned.clear();
        sub.rehover = true;
        dp.undo.push(before);
        self.rev += 1;
        true
    }

    /// The cursor a search's or the detail panel's item names on the focused panel.
    pub fn dash_cursor_of(&self, w: die_core::WindowId, item: &Item) -> Option<Cursor> {
        let model = self.dash.as_ref()?.model.as_ref()?;
        let (i, _) = self.dash_focused(w)?;
        match &model.panels.get(i)?.state {
            PanelState::Chart(cm) => cm.cursor_of(item),
            PanelState::Graph(_) => item.cursor(),
            _ => None,
        }
    }

    /// The names a search on the focused panel looks through: a chart's series and categories,
    /// a graph's nodes, what its view hides left out.
    pub fn dash_names(&self, w: die_core::WindowId) -> Option<Vec<(String, Item)>> {
        let model = self.dash.as_ref()?.model.as_ref()?;
        let (i, sub) = self.dash_focused(w)?;
        let names = match &model.panels.get(i)?.state {
            PanelState::Chart(cm) => (cm.names().into_iter())
                .filter(|(_, it)| match it {
                    Item::Series(k) => cm.visible(&sub.view, k),
                    _ => true,
                })
                .collect(),
            PanelState::Graph(g) => (g.search_names().into_iter())
                .filter(|(_, it)| match it {
                    Item::Node { id, .. } => {
                        g.index_of(id).is_some_and(|n| g.visible(&sub.view, n))
                    }
                    _ => true,
                })
                .collect(),
            _ => return None,
        };
        Some(names)
    }

    /// The focused chart panel's settings (`s`) and its chart, by number.
    pub fn dash_settings(
        &self,
        w: die_core::WindowId,
    ) -> Option<(usize, Toggles, crate::spec::Chart)> {
        let d = self.dash.as_ref()?;
        let (i, _) = self.dash_focused(w)?;
        let cm = d.model.as_ref()?.panels.get(i)?.chart()?;
        Some((
            i,
            d.toggles.get(i).copied().unwrap_or_default(),
            cm.chart.clone(),
        ))
    }

    /// The focused chart panel's setting `s` turned over, its model built again.
    pub fn dash_flip(
        &mut self,
        ctx: &mut die_core::Ctx,
        i: usize,
        setting: crate::interact::Setting,
    ) {
        let Some(d) = self.dash.as_mut() else { return };
        let Some(chart) = d
            .model
            .as_ref()
            .and_then(|m| m.panels.get(i)?.chart())
            .map(|c| c.chart.clone())
        else {
            return;
        };
        if d.toggles.len() <= i {
            d.toggles.resize(i + 1, Toggles::default());
        }
        d.toggles[i].flip(&chart, setting);
        self.rev += 1;
        self.dash_rebuild(ctx);
    }

    /// The focused panel's data as TSV, its line for the cursor: a chart's (in its view's
    /// order, hidden series left out) or a graph's two tables. None with no chart or graph
    /// focused.
    pub fn dash_table(&self, w: die_core::WindowId) -> Option<(String, Option<usize>)> {
        let model = self.dash.as_ref()?.model.as_ref()?;
        let (i, sub) = self.dash_focused(w)?;
        match &model.panels.get(i)?.state {
            PanelState::Chart(cm) => Some(cm.tsv_at(&sub.view, sub.cursor.as_ref())),
            PanelState::Graph(g) => {
                let t = g.tsv(&sub.view, sub.cursor.as_ref());
                Some((format!("{}\n{}", t.nodes, t.edges), t.line))
            }
            _ => None,
        }
    }

    /// The dashboard as window `w` shows it (else at 880 × 610), laid out with the fonts at 2×
    /// and at 1×, for a job to export: the PNG's scene, and the SVG's.
    pub(super) fn dash_scenes(&self, w: Option<die_core::WindowId>) -> Option<(Scene, Scene)> {
        let model = self.dash.as_ref()?.model.as_ref()?;
        let pane = w.and_then(|w| self.panes.get(&w));
        let view = match pane.and_then(|p| p.dash.as_ref()) {
            Some(dp) => {
                let mut v = dp.shown(u64::MAX);
                for p in &mut v.panels {
                    p.view.reference = None;
                }
                v
            }
            None => DashView::initial(model),
        };
        let size = pane.map_or((880.0, 610.0), Pane::size);
        let mut kit = self.kit.borrow_mut();
        let mut at = |scale: f32| {
            measured(kit.book.as_mut(), scale, |m| {
                let layout = dashboard::layout(model, &view, size, scale, &self.theme, m);
                dashboard::base(&layout, &view, &self.theme, m)
            })
        };
        Some((at(2.0), at(1.0)))
    }

    /// What a restart keeps of the dashboard's view: the last window's, else what a restart
    /// gave and no window has taken yet.
    pub fn kept_dash(&self) -> Option<die_core::session::Value> {
        let dp = self
            .last
            .and_then(|w| self.panes.get(&w))
            .and_then(|p| p.dash.as_ref());
        match dp {
            Some(dp) => Some(keep_view(dp)),
            None => (self.dash.as_ref())
                .and_then(|d| d.seed.clone())
                .or_else(|| self.dash_seed.clone()),
        }
    }

    /// Whether the dashboard has live sources: a command's.
    pub fn dash_live(&self) -> bool {
        self.dash
            .as_ref()
            .and_then(|d| d.model.as_ref())
            .is_some_and(DashModel::is_live)
    }
}

/// `T`, or a click on the time-range chip: the ranges (`15m` … `all`), then `custom…`, which
/// takes `10:00-11:30` (times on the newest sample's day) or `-2h` (so long to the newest); the
/// view eases there, one undo step.
pub fn time_prompt(ctx: &mut die_core::Ctx, state: super::Shared, w: die_core::WindowId) {
    let mut items: Vec<String> = RANGES
        .iter()
        .map(|r| format!("{}: {}", r.key, r.label))
        .collect();
    items.push("custom…".into());
    // What's typed, kept: a range of its own (`2h`, `-90m`, `10:00-11:30`) is taken as typed,
    // never as the preset it happens to match (`2h` isn't `24h`).
    let typed = std::rc::Rc::new(RefCell::new(String::new()));
    let keep = typed.clone();
    let spec = die_core::PromptSpec::new("Time range: ")
        .candidates(items)
        .history("fig-time")
        .on_change(move |_, text| {
            *keep.borrow_mut() = text.to_string();
            Ok(())
        });
    ctx.prompt(spec, move |ctx, answer| {
        let own = typed.borrow().trim().to_string();
        let answer = match own_range(&own) {
            true => Some(own),
            false => answer,
        };
        let Some(answer) = answer
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
        else {
            return Ok(());
        };
        if answer.starts_with("custom") {
            let s = state.clone();
            let spec = die_core::PromptSpec::new("Custom range (10:00-11:30, or -2h): ")
                .history("fig-time");
            ctx.prompt(spec, move |ctx, answer| {
                let Some(answer) = answer else { return Ok(()) };
                choose_range(ctx, &s, w, &answer)
            });
            return Ok(());
        }
        choose_range(ctx, &state, w, &answer)
    });
}

/// Whether typed text is a range of its own, not a preset's name: a length (`2h`, `-90m`) or
/// two times of day (`10:00-11:30`).
pub fn own_range(t: &str) -> bool {
    let t = t.trim();
    let length = crate::spec::chart::parse_duration(t.trim_start_matches('-').trim()).is_some();
    let times = t.contains(':') && t.contains(['-', '–']) && !t.starts_with("custom");
    (length || times) && !RANGES.iter().any(|r| r.key == t)
}

/// A range's answer applied to window `w`'s dashboard, or why not.
fn choose_range(
    ctx: &mut die_core::Ctx,
    state: &super::Shared,
    w: die_core::WindowId,
    answer: &str,
) -> die_core::Result {
    let mut st = state.borrow_mut();
    let (newest, extent, zone) = match st.dash.as_ref().and_then(|d| d.model.as_ref()) {
        Some(m) => (m.newest(), m.extent(), m.zone.clone()),
        None => return Err(die_core::Error::Msg("fig: nothing is drawn yet".into())),
    };
    let time = parse_range(answer, newest, extent, &zone).ok_or_else(|| {
        die_core::Error::Msg(format!(
            "fig: {answer}: a range (15m, 1h, 6h, 24h, 7d, 30d, all), 10:00-11:30 or -2h"
        ))
    })?;
    st.dash_set_time(w, time, ctx.now());
    st.refresh(ctx);
    Ok(())
}

/// What a time range's text says: a preset (`6h` or its words), `-2h` or `2h` (so long to the
/// newest sample), or `10:00-11:30` (times of day on the newest sample's day). The outer None
/// where it says none of these; the inner None is all the data.
pub fn parse_range(
    text: &str,
    newest: Option<f64>,
    extent: Option<(f64, f64)>,
    zone: &TimeZone,
) -> Option<Option<(f64, f64)>> {
    let t = text.trim();
    let key = t.split(':').next().unwrap_or(t).trim();
    if let Some(r) = RANGES
        .iter()
        .find(|r| r.key == key || r.label.eq_ignore_ascii_case(t))
    {
        return Some(r.secs.and_then(|s| newest.map(|n| (n - s, n))));
    }
    if let Some(d) = crate::spec::chart::parse_duration(t.trim_start_matches('-').trim()) {
        let n = newest?;
        return Some(Some((n - d.as_secs_f64(), n)));
    }
    // Times of day: `10:00-11:30`, `10:00 – 11:30`.
    let parts: Vec<&str> = t.split(['-', '–']).map(str::trim).collect();
    if let [a, b] = parts[..] {
        let n = newest?;
        let day = jiff::Timestamp::from_microsecond((n * 1e6) as i64)
            .ok()?
            .to_zoned(zone.clone())
            .date();
        let at = |s: &str| -> Option<f64> {
            let tm: jiff::civil::Time = s.parse().ok()?;
            let z = day.to_datetime(tm).to_zoned(zone.clone()).ok()?;
            Some(z.timestamp().as_microsecond() as f64 / 1e6)
        };
        let (x0, x1) = (at(a)?, at(b)?);
        if x1 > x0 {
            let (lo, hi) = extent.unwrap_or((x0, x1));
            let x0 = x0.max(lo.min(x1));
            let x1 = x1.min(hi.max(x0));
            return (x1 > x0).then_some(Some((x0, x1)));
        }
    }
    None
}

// ------------------------------------------------------------------ what a restart keeps

/// A dashboard's view as a restart keeps it: the time, the focus, full size, the scroll,
/// following, and each panel's own view and cursor.
pub fn keep_view(dp: &DashPane) -> die_core::session::Value {
    use die_core::session::json;
    let panels: Vec<die_core::session::Value> = dp
        .subs
        .iter()
        .map(|s| crate::view_value_of(&s.view, &s.cursor))
        .collect();
    json!({
        "time": dp.view.time.map(|(a, b)| json!([a, b])),
        "focused": dp.view.focused,
        "full": dp.view.full,
        "scroll": dp.view.scroll,
        "follow": dp.follow,
        "panels": panels,
    })
}

/// Put a kept dashboard view back on a window's new dashboard: its panels by number, as many as
/// both have.
fn restore_view(dp: &mut DashPane, model: &DashModel, v: &die_core::session::Value) {
    use die_core::session::Value;
    let time = v.get("time").and_then(Value::as_array).and_then(|a| {
        let (lo, hi) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
        (lo.is_finite() && hi.is_finite() && lo < hi).then_some((lo, hi))
    });
    let n = model.panels.len();
    let index = |k: &str| {
        v.get(k)
            .and_then(Value::as_u64)
            .map(|i| i as usize)
            .filter(|&i| i < n)
    };
    dp.follow = v.get("follow").and_then(Value::as_bool).unwrap_or(true);
    dp.view.time = if dp.follow {
        // Following: the same width, at the newest as it is now.
        live::follow(time, model.newest(), true, 0.0, FollowChange::Resume)
            .x
            .or(dp.view.time)
    } else {
        time.or(dp.view.time)
    };
    dp.view.focused = index("focused");
    dp.view.full = index("full");
    dp.view.scroll = v.get("scroll").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let panels = v.get("panels").and_then(Value::as_array);
    for (sub, pv) in dp.subs.iter_mut().zip(panels.into_iter().flatten()) {
        let (view, cursor) = crate::view_of_value(pv);
        sub.view = view;
        sub.cursor = cursor;
    }
    dp.sync_linked(model);
}
