//! The dashboard: rows of panels over shared data and a shared time range, linked by the
//! pointer, with focus stops in reading order (phase 4; contract-p4 §3).
//!
//! The editor drives a dashboard as it drives a chart:
//!
//! - [`DashModel::build`] resolves a [`DashboardSpec`] against its data, already read
//!   ([`Sources`]), every panel with its own result: a chart's, a graph's or a tile's model, a
//!   nested `.fig` file's figure (a dashboard there is a nested group), a panel still waiting
//!   for its data (a command not yet run), or the errors that keep it from drawing. One bad
//!   panel shows its error figure and the others draw. The model lists what it wants read
//!   ([`DashModel::wants`]) and what needs your yes before it's run or opened
//!   ([`DashModel::trust`], ruling 13); build it once with no sources to learn them;
//! - [`layout`](fn@layout) places the header and the rows at a size, and lays out each
//!   panel's figure inside its frame for a [`DashView`] (each figure by its own layout, so a
//!   panel keeps every look rule it has alone);
//! - [`base`] draws the frames, the header and every panel at rest; [`overlay`] draws what
//!   follows the pointer: the hovered panel's own overlay and, in every other linked panel,
//!   the moment's line and dots ([`time`]);
//! - [`hit`](fn@hit) says what's under the pointer, and [`hover`](fn@hover) what it shows
//!   there.
//!
//! Panels are numbered in reading order: rows top to bottom, a row's panels left to right, a
//! column's top to bottom before the panel right of it, a nested group's in its own reading
//! order where it stands. [`DashView::panels`] and the focus go by that number.
//!
//! # Linked time
//!
//! A chart whose x is the dashboard's `time.field` (a time axis) is linked, and so is a stat
//! whose `trend` is that field. Linked panels share one x domain, [`DashView::time`]: a zoom,
//! a pan or a brush on one changes it for all. It opens on `time.range` ending at the newest
//! sample among them, or on all of their data without a range.

pub mod draw;
pub mod hit;
pub mod layout;
pub mod time;

pub use draw::{base, overlay};
pub use hit::{DashHit, DashHover, PanelHover, hit, hover};
pub use layout::{DashLayout, layout, layout_full};
pub use time::{DashView, PanelView, RANGES, Range, tween};

use crate::data::{ReadOptions, Table};
use crate::error::{ErrorKind, FigError, Pos, Result};
use crate::figure::TileModel;
use crate::graph::GraphModel;
use crate::plot::{AxisKind, ChartModel};
use crate::spec::chart::parse_duration;
use crate::spec::{DashboardSpec, DataRef, DataSpec, Figure, Panel, PanelSpec, TimeSpec, Tooltip};
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How deep `file` panels and `#name` references nest.
pub const MAX_DEPTH: usize = 4;

/// One source a dashboard wants read: a data file, or a command run or streamed, with the
/// spec's hints for reading it. Panels on the same source share its one read.
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    /// What to read: its `source` a [`DataRef::File`], [`DataRef::Run`] or
    /// [`DataRef::Stream`] (inline data, names and references are resolved by the build).
    pub data: DataSpec,
    /// Where a command runs: the directory of the spec that names it. Empty for a file.
    pub dir: PathBuf,
}

impl Want {
    /// What it is, as a panel waiting for it says: the file's name, or the command.
    pub fn name(&self) -> String {
        match &self.data.source {
            DataRef::File(p) => p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            DataRef::Run { cmd, .. } | DataRef::Stream { cmd } => cmd.clone(),
            _ => String::new(),
        }
    }

    /// Whether it's a command's: live data.
    pub fn is_command(&self) -> bool {
        matches!(
            self.data.source,
            DataRef::Run { .. } | DataRef::Stream { .. }
        )
    }
}

/// Where a wanted source is.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceState {
    /// Read.
    Ready(Arc<Table>),
    /// Still coming, and what the panel would say (it draws its frame and title only).
    Waiting(String),
    /// It can't be had: a missing file, a command that failed or that you said no to.
    Failed(FigError),
}

/// A dashboard's data as the build is given it: each [`Want`]'s state, the file panels outside
/// the spec's directory you've said yes to, and the header's live status. A want it doesn't
/// have is waiting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    entries: Vec<(Want, SourceState)>,
    trusted: Vec<PathBuf>,
    /// What the header says of live sources, if any is live.
    pub live: Option<LiveStatus>,
}

impl Sources {
    /// No sources: every panel that reads one waits.
    pub fn new() -> Sources {
        Sources::default()
    }

    /// Sets a want's state, over what it was.
    pub fn insert(&mut self, want: Want, state: SourceState) {
        match self.entries.iter_mut().find(|(w, _)| *w == want) {
            Some((_, s)) => *s = state,
            None => self.entries.push((want, state)),
        }
    }

    /// A want's table, read.
    pub fn ready(&mut self, want: Want, table: Arc<Table>) {
        self.insert(want, SourceState::Ready(table));
    }

    /// A want's state, if it has one.
    pub fn get(&self, want: &Want) -> Option<&SourceState> {
        self.entries.iter().find(|(w, _)| w == want).map(|(_, s)| s)
    }

    /// A `file` panel outside the spec's directory may be opened: you said yes.
    pub fn trust_file(&mut self, path: impl Into<PathBuf>) {
        let p = path.into();
        if !self.trusted.contains(&p) {
            self.trusted.push(p);
        }
    }

    /// Whether you said yes to opening this file.
    pub fn trusted(&self, path: &Path) -> bool {
        self.trusted
            .iter()
            .any(|p| crate::source::same_path(p, path))
    }
}

/// What a dashboard asks your yes for before it runs or opens it (ruling 13): a command (run
/// or streamed, always asked, once a spec and command), or a `file` panel outside its spec's
/// directory.
#[derive(Clone, Debug, PartialEq)]
pub enum NeedsTrust {
    /// A command.
    Command {
        /// The spec that names it: its file, None for the dashboard's own text when it has no
        /// file.
        spec: Option<PathBuf>,
        /// The command line.
        cmd: String,
        /// How often it runs, for a run.
        every: Option<Duration>,
        /// Whether it's streamed.
        stream: bool,
    },
    /// A `file` panel outside its spec's directory.
    File {
        /// The spec that names it.
        spec: Option<PathBuf>,
        /// The file.
        path: PathBuf,
        /// Where the panel is in that spec.
        at: Pos,
    },
}

impl NeedsTrust {
    /// The question to ask, naming the spec as `name` when it has no file: `Run "./errors.sh"
    /// every 30s for health.fig? (y/n)`.
    pub fn question(&self, name: &str) -> String {
        let of = |spec: &Option<PathBuf>| {
            spec.as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(|| name.to_string(), |n| n.to_string_lossy().into_owned())
        };
        match self {
            NeedsTrust::Command {
                spec,
                cmd,
                every,
                stream,
            } => {
                let verb = if *stream { "Stream" } else { "Run" };
                let every = every.map_or_else(String::new, |e| {
                    format!(" every {}", crate::spec::chart::format_duration(e))
                });
                format!("{verb} \"{cmd}\"{every} for {}? (y/n)", of(spec))
            }
            NeedsTrust::File { spec, path, .. } => {
                let dir = spec.as_ref().and_then(|s| s.parent());
                let shown = dir.map_or_else(
                    || path.display().to_string(),
                    |d| crate::source::relative(path, d),
                );
                format!("Open \"{shown}\" for {}? (y/n)", of(spec))
            }
        }
    }
}

/// What the header's live status says, at the right.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveStatus {
    /// Its state: the dot's colour, or the error's icon.
    pub state: LiveState,
    /// What it says: `live · updated 12 s ago`.
    pub text: String,
}

/// The live status's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveState {
    /// Up to date: a dot in the `good` colour.
    Live,
    /// A run is going: the dot stays, the words say so (nothing dims).
    Updating,
    /// Not following: a muted dot.
    Paused,
    /// The last run failed: the critical icon.
    Error,
}

impl LiveStatus {
    /// `live · updated 12 s ago`, `ago` seconds after the last update.
    pub fn updated(ago: f64) -> LiveStatus {
        let text = match ago_text(ago) {
            None => "live · updated just now".into(),
            Some(a) => format!("live · updated {a}"),
        };
        LiveStatus {
            state: LiveState::Live,
            text,
        }
    }

    /// `updating…`, while a run is going.
    pub fn updating() -> LiveStatus {
        LiveStatus {
            state: LiveState::Updating,
            text: "updating…".into(),
        }
    }

    /// `paused`.
    pub fn paused() -> LiveStatus {
        LiveStatus {
            state: LiveState::Paused,
            text: "paused".into(),
        }
    }

    /// `error · 2 m ago`, `ago` seconds after the run that failed.
    pub fn error(ago: f64) -> LiveStatus {
        let text = match ago_text(ago) {
            None => "error · just now".into(),
            Some(a) => format!("error · {a}"),
        };
        LiveStatus {
            state: LiveState::Error,
            text,
        }
    }
}

/// `12 s ago`, `2 m ago`, `3 h ago`, `2 d ago`; None under a second.
fn ago_text(ago: f64) -> Option<String> {
    if ago.is_nan() || ago < 1.0 {
        return None;
    }
    let (n, unit) = match ago {
        a if a < 60.0 => (a, "s"),
        a if a < 3600.0 => (a / 60.0, "m"),
        a if a < 86_400.0 => (a / 3600.0, "h"),
        a => (a / 86_400.0, "d"),
    };
    Some(format!("{} {unit} ago", n.floor() as u64))
}

/// Where a dashboard's spec is, and the zone its times are in.
#[derive(Clone, Copy, Debug)]
pub struct Origin<'a> {
    /// The spec's own file, if it has one: a nested panel naming it is a cycle.
    pub file: Option<&'a Path>,
    /// Its directory: commands run there, and `file` panels outside it need your yes.
    pub dir: &'a Path,
    /// The zone times without an offset are in.
    pub zone: &'a TimeZone,
}

/// What kind of figure a panel is, which sizes it whatever its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    /// A chart.
    Chart,
    /// A network graph.
    Graph,
    /// A stat or a meter: a row of tiles takes their natural height.
    Tile,
    /// Unknown: a `file` panel that couldn't be read.
    Other,
}

/// A tile panel: the figure, its data, and its model over all of it (the layout builds it
/// again over the dashboard's time when it's linked).
pub struct Tile {
    /// The stat or meter.
    pub figure: Figure,
    /// Its data.
    pub table: Arc<Table>,
    /// Its model over every row.
    pub model: TileModel,
}

/// Why a panel can't draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// What went wrong: never empty.
    pub errors: Vec<FigError>,
    /// What the error figure's title names: the file, else "this panel".
    pub name: String,
}

/// What a panel shows.
pub enum PanelState {
    /// A chart.
    Chart(ChartModel),
    /// A graph, laid out.
    Graph(GraphModel),
    /// A tile.
    Tile(Tile),
    /// Its data is still coming: its frame and title only.
    Waiting(String),
    /// It can't draw: its error figure.
    Failed(Failure),
}

/// One panel of a dashboard.
pub struct PanelModel {
    /// Its title and subtitle, drawn in its header (the figure's own, which it then doesn't
    /// draw itself).
    pub title: Option<String>,
    /// The line under the title.
    pub subtitle: Option<String>,
    /// What kind of figure it is.
    pub kind: PanelKind,
    /// What it shows.
    pub state: PanelState,
    /// Its x is the dashboard's time: the time view, the hover's moment and the brush are
    /// shared.
    pub linked: bool,
    /// The file it's from, for a `file` panel or a panel of a nested file.
    pub file: Option<PathBuf>,
    /// Where it is in its spec.
    pub at: Pos,
    /// The error of the build that failed while the figure before it is kept: the stale
    /// badge, on this panel ([`DashModel::carry`]).
    pub stale: Option<FigError>,
}

impl PanelModel {
    /// Its chart, if it shows one.
    pub fn chart(&self) -> Option<&ChartModel> {
        match &self.state {
            PanelState::Chart(m) => Some(m),
            _ => None,
        }
    }

    /// Its graph, if it shows one.
    pub fn graph(&self) -> Option<&GraphModel> {
        match &self.state {
            PanelState::Graph(g) => Some(g),
            _ => None,
        }
    }

    /// Its tile, if it shows one.
    pub fn tile(&self) -> Option<&Tile> {
        match &self.state {
            PanelState::Tile(t) => Some(t),
            _ => None,
        }
    }

    /// Whether it draws a figure (not waiting, not failed).
    pub fn ready(&self) -> bool {
        matches!(
            self.state,
            PanelState::Chart(_) | PanelState::Graph(_) | PanelState::Tile(_)
        )
    }
}

/// A row of a dashboard, or of a nested group, as built.
pub struct Row {
    /// Its share of the height, against the other flexible rows'.
    pub height: f32,
    /// Its cells, left to right.
    pub cells: Vec<Cell>,
}

/// One place in a row: a panel, a column of them, or a nested group.
pub struct Cell {
    /// Its share of the row's width.
    pub span: f32,
    /// What it holds.
    pub kind: CellKind,
}

/// What a cell holds.
pub enum CellKind {
    /// A panel, by its number.
    Panel(usize),
    /// Cells stacked down a column.
    Column(Vec<Cell>),
    /// A nested dashboard from a `file` panel.
    Group(Group),
}

/// A nested dashboard: its title over its rows, in a frame.
pub struct Group {
    /// Its title.
    pub title: Option<String>,
    /// Its rows.
    pub rows: Vec<Row>,
    /// Its file.
    pub file: PathBuf,
}

/// A dashboard resolved against its data: see the module's docs.
pub struct DashModel {
    /// Its title.
    pub title: Option<String>,
    /// The time its linked panels share.
    pub time: Option<TimeSpec>,
    /// `time.range` in seconds.
    pub range: Option<f64>,
    /// Which panels show a card on a hover.
    pub tooltip: Tooltip,
    /// Its rows.
    pub rows: Vec<Row>,
    /// Its panels, in reading order.
    pub panels: Vec<PanelModel>,
    /// The sources it wants read, in the order it met them.
    pub wants: Vec<Want>,
    /// What needs your yes before it's run or opened.
    pub trust: Vec<NeedsTrust>,
    /// What the header's live status says.
    pub live: Option<LiveStatus>,
    /// The zone its times are shown in.
    pub zone: TimeZone,
}

/// What a source came to, for a panel.
enum Got {
    Table(Arc<Table>, Option<String>),
    Waiting(String),
    Failed(FigError),
}

/// The data names in scope, and the spec they're in.
struct Scope<'s> {
    data: &'s [(String, DataSpec)],
    file: Option<PathBuf>,
    dir: PathBuf,
}

impl Scope<'_> {
    /// An error made in this scope's text: in its file, if it isn't the dashboard's own.
    fn placed(&self, e: FigError) -> FigError {
        match (&self.file, &e.file) {
            (Some(f), None) if e.at.is_some() => e.in_file(f, None),
            _ => e,
        }
    }
}

struct Builder<'a> {
    sources: &'a Sources,
    origin_file: Option<PathBuf>,
    resolve: &'a dyn Fn(&Path) -> Result<Figure>,
    zone: &'a TimeZone,
    time_field: Option<String>,
    panels: Vec<PanelModel>,
    wants: Vec<Want>,
    trust: Vec<NeedsTrust>,
}

impl DashModel {
    /// Builds every panel's model from `spec` over the sources read so far; `resolve` reads
    /// and parses a `file` panel's figure (and a `#name` reference's file) from its own
    /// directory. Never fails as a whole: each panel has its own result.
    pub fn build(
        spec: &DashboardSpec,
        data: &Sources,
        resolve: &dyn Fn(&Path) -> Result<Figure>,
        origin: &Origin<'_>,
    ) -> DashModel {
        let mut b = Builder {
            sources: data,
            origin_file: origin.file.map(Path::to_path_buf),
            resolve,
            zone: origin.zone,
            time_field: spec.time.as_ref().map(|t| t.field.clone()),
            panels: Vec::new(),
            wants: Vec::new(),
            trust: Vec::new(),
        };
        let scope = Scope {
            data: &spec.data,
            file: origin.file.map(Path::to_path_buf),
            dir: origin.dir.to_path_buf(),
        };
        let chain: Vec<PathBuf> = origin.file.map(Path::to_path_buf).into_iter().collect();
        let rows = b.rows(&spec.rows, &scope, &chain);
        let range = spec
            .time
            .as_ref()
            .and_then(|t| t.range.as_deref())
            .and_then(parse_duration)
            .map(|d| d.as_secs_f64());
        DashModel {
            title: spec.title.clone(),
            time: spec.time.clone(),
            range,
            tooltip: spec.tooltip,
            rows,
            panels: b.panels,
            wants: b.wants,
            trust: b.trust,
            live: data.live.clone(),
            zone: origin.zone.clone(),
        }
    }

    /// The sources it wants read.
    pub fn wants(&self) -> &[Want] {
        &self.wants
    }

    /// What needs your yes.
    pub fn trust(&self) -> &[NeedsTrust] {
        &self.trust
    }

    /// The panels that are linked, by number.
    pub fn linked(&self) -> Vec<usize> {
        (0..self.panels.len())
            .filter(|&i| self.panels[i].linked)
            .collect()
    }

    /// The newest sample among the linked panels: where `time.range` ends.
    pub fn newest(&self) -> Option<f64> {
        self.linked_extents().map(|(_, hi)| hi).reduce(f64::max)
    }

    /// The linked panels' data from the oldest sample to the newest: the time view without a
    /// range.
    pub fn extent(&self) -> Option<(f64, f64)> {
        self.linked_extents()
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    fn linked_extents(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        let field = self.time.as_ref().map(|t| t.field.as_str());
        self.panels
            .iter()
            .filter(|p| p.linked)
            .filter_map(move |p| match &p.state {
                PanelState::Chart(m) => {
                    let (x, _) = m.extent();
                    let newest = m.newest()?;
                    Some((x.0.min(newest), newest))
                }
                PanelState::Tile(t) => {
                    let col = t.table.column(field?)?;
                    let vs = (0..col.len())
                        .map(|r| col.f64_at(r))
                        .filter(|v| v.is_finite());
                    vs.fold(None, |acc: Option<(f64, f64)>, v| {
                        Some(acc.map_or((v, v), |(a, b)| (a.min(v), b.max(v))))
                    })
                }
                _ => None,
            })
    }

    /// Whether any source is live: a command's.
    pub fn is_live(&self) -> bool {
        self.wants.iter().any(Want::is_command)
    }

    /// The model with each panel that failed (or waits again) while the old model's drew
    /// kept as the old one had it, its badge saying why: the panel whose rebuild failed keeps
    /// its figure, as a figure alone does while its source fails to parse. Panels match by
    /// number, so only when both have as many.
    pub fn carry(mut self, old: DashModel) -> DashModel {
        if old.panels.len() != self.panels.len() {
            return self;
        }
        for (new, old) in self.panels.iter_mut().zip(old.panels) {
            if new.ready() || !old.ready() || new.kind != old.kind {
                continue;
            }
            let stale = match &new.state {
                PanelState::Failed(f) => f.errors.first().cloned(),
                _ => old.stale.clone(),
            };
            *new = PanelModel { stale, ..old };
        }
        self
    }
}

impl Builder<'_> {
    fn rows(
        &mut self,
        rows: &[crate::spec::Row],
        scope: &Scope<'_>,
        chain: &[PathBuf],
    ) -> Vec<Row> {
        rows.iter()
            .map(|r| Row {
                height: r.height.unwrap_or(1.0) as f32,
                cells: r
                    .panels
                    .iter()
                    .map(|p| self.cell(p, scope, chain))
                    .collect(),
            })
            .collect()
    }

    fn cell(&mut self, p: &PanelSpec, scope: &Scope<'_>, chain: &[PathBuf]) -> Cell {
        let kind = match &p.panel {
            Panel::Figure(f) => CellKind::Panel(self.leaf(f, scope, p.at)),
            Panel::Column(ps) => {
                CellKind::Column(ps.iter().map(|q| self.cell(q, scope, chain)).collect())
            }
            Panel::File(path) => self.file(path, p.at, scope, chain),
        };
        Cell {
            span: f32::from(p.span.max(1)),
            kind,
        }
    }

    /// A `file` panel: the figure in the file, a dashboard there a nested group.
    fn file(&mut self, path: &Path, at: Pos, scope: &Scope<'_>, chain: &[PathBuf]) -> CellKind {
        let name = shown(path);
        let failed = |b: &mut Builder<'_>, e: FigError| {
            let i = b.push(PanelModel {
                title: None,
                subtitle: None,
                kind: PanelKind::Other,
                state: PanelState::Failed(Failure {
                    errors: vec![e],
                    name: name.clone(),
                }),
                linked: false,
                file: Some(path.to_path_buf()),
                at,
                stale: None,
            });
            CellKind::Panel(i)
        };
        if chain.iter().any(|c| crate::source::same_path(c, path)) {
            let mut names: Vec<String> = chain.iter().map(|c| shown(c)).collect();
            names.push(name.clone());
            let e = FigError::at(format!("{name} includes itself: {}", names.join(" → ")), at);
            return failed(self, scope.placed(e));
        }
        if chain.len() > MAX_DEPTH {
            let e = FigError::at(format!("files nest at most {MAX_DEPTH} deep"), at);
            return failed(self, scope.placed(e));
        }
        let inside = path.starts_with(&scope.dir) && !scope.dir.as_os_str().is_empty();
        if !inside && !self.sources.trusted(path) {
            let ask = NeedsTrust::File {
                spec: scope.file.clone(),
                path: path.to_path_buf(),
                at,
            };
            if !self.trust.contains(&ask) {
                self.trust.push(ask);
            }
            let i = self.push(PanelModel {
                title: None,
                subtitle: None,
                kind: PanelKind::Other,
                state: PanelState::Waiting(format!("waiting for your yes to open {name}")),
                linked: false,
                file: Some(path.to_path_buf()),
                at,
                stale: None,
            });
            return CellKind::Panel(i);
        }
        let figure = match (self.resolve)(path) {
            Ok(f) => f,
            Err(e) => {
                let e = match e.file {
                    Some(_) => e,
                    None => e.in_file(path, None),
                };
                return failed(self, e);
            }
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut deeper = chain.to_vec();
        deeper.push(path.to_path_buf());
        match &figure {
            Figure::Dashboard(d) => {
                let inner = Scope {
                    data: &d.data,
                    file: Some(path.to_path_buf()),
                    dir,
                };
                CellKind::Group(Group {
                    title: d.title.clone(),
                    rows: self.rows(&d.rows, &inner, &deeper),
                    file: path.to_path_buf(),
                })
            }
            other => {
                let inner = Scope {
                    data: &[],
                    file: Some(path.to_path_buf()),
                    dir,
                };
                let i = self.leaf(other, &inner, at);
                self.panels[i].file = Some(path.to_path_buf());
                CellKind::Panel(i)
            }
        }
    }

    fn push(&mut self, p: PanelModel) -> usize {
        self.panels.push(p);
        self.panels.len() - 1
    }

    /// A panel of a figure.
    fn leaf(&mut self, fig: &Figure, scope: &Scope<'_>, at: Pos) -> usize {
        let field = self.time_field.clone();
        let mut panel = PanelModel {
            title: None,
            subtitle: None,
            kind: PanelKind::Other,
            state: PanelState::Waiting(String::new()),
            linked: false,
            file: scope.file.clone(),
            at,
            stale: None,
        };
        // What its error figure names: its title, else its file's name if it's another
        // file's, else "this panel" (never the dashboard's own file, which isn't what failed).
        let title = match fig {
            Figure::Chart(c) => c.title.clone(),
            Figure::Graph(g) => g.title.clone(),
            Figure::Stat(s) => Some(s.label.clone()),
            Figure::Meter(m) => Some(m.label.clone()),
            Figure::Sparkline(l) => Some(l.label.clone()),
            Figure::Dashboard(d) => d.title.clone(),
        };
        let own = |f: &Path| {
            self.origin_file
                .as_deref()
                .is_some_and(|o| crate::source::same_path(o, f))
        };
        let name = title.filter(|t| !t.is_empty()).unwrap_or_else(|| {
            match scope.file.as_deref().filter(|f| !own(f)) {
                Some(f) => shown(f),
                None => "this panel".to_string(),
            }
        });
        let fail = |errors: Vec<FigError>, name: &str| {
            PanelState::Failed(Failure {
                errors,
                name: name.to_string(),
            })
        };
        panel.state = match fig {
            Figure::Chart(c) => {
                panel.kind = PanelKind::Chart;
                let mut chart = c.clone();
                panel.title = chart.title.take().filter(|t| !t.is_empty());
                panel.subtitle = chart.subtitle.take().filter(|t| !t.is_empty());
                let own = chart.layers.iter().filter_map(|l| l.data.as_ref());
                let specs: Vec<DataSpec> =
                    std::iter::once(&chart.data).chain(own).cloned().collect();
                let mut tables = Vec::with_capacity(specs.len());
                let mut notes = Vec::new();
                let mut state = None;
                for d in &specs {
                    match self.table(d, scope, &[]) {
                        Got::Table(t, note) => {
                            tables.push(t);
                            notes.extend(note);
                        }
                        Got::Waiting(w) => {
                            state = Some(PanelState::Waiting(w));
                            break;
                        }
                        Got::Failed(e) => {
                            state = Some(fail(vec![e], &name));
                            break;
                        }
                    }
                }
                match state {
                    Some(s) => s,
                    None => self.chart(chart, tables, notes, &name, scope),
                }
            }
            Figure::Graph(g) => {
                panel.kind = PanelKind::Graph;
                let mut g = g.clone();
                panel.title = g.title.take().filter(|t| !t.is_empty());
                panel.subtitle = g.subtitle.take().filter(|t| !t.is_empty());
                match GraphModel::build(g) {
                    Ok(m) => PanelState::Graph(m),
                    Err(e) => fail(vec![scope.placed(e)], &name),
                }
            }
            Figure::Stat(_) | Figure::Meter(_) | Figure::Sparkline(_) => {
                panel.kind = PanelKind::Tile;
                let data = match fig {
                    Figure::Stat(s) => &s.data,
                    Figure::Meter(m) => &m.data,
                    Figure::Sparkline(l) => &l.data,
                    _ => unreachable!("matched above"),
                };
                // A stat whose trend, or a sparkline whose x, is the dashboard's time is
                // linked: it shows the linked moment as its mini crosshair.
                match fig {
                    Figure::Stat(s) => panel.linked = s.trend.is_some() && s.trend == field,
                    Figure::Sparkline(l) => {
                        panel.linked = field.as_deref() == Some(l.x.as_str());
                    }
                    _ => {}
                }
                // A tile's label is its title while it can't draw itself.
                let label = match fig {
                    Figure::Stat(s) => s.label.clone(),
                    Figure::Meter(m) => m.label.clone(),
                    Figure::Sparkline(l) => l.label.clone(),
                    _ => unreachable!("matched above"),
                };
                panel.title = Some(label).filter(|l| !l.is_empty());
                match self.table(data, scope, &[]) {
                    Got::Table(t, note) => match TileModel::build_in(fig, &t, None, self.zone) {
                        Ok(model) => PanelState::Tile(Tile {
                            figure: fig.clone(),
                            table: t,
                            model,
                        }),
                        Err(e) => {
                            let e = note.into_iter().fold(scope.placed(e), FigError::note);
                            fail(vec![e], &name)
                        }
                    },
                    Got::Waiting(w) => PanelState::Waiting(w),
                    Got::Failed(e) => fail(vec![e], &name),
                }
            }
            Figure::Dashboard(_) => fail(
                vec![FigError::at("a panel can't be a dashboard", at)],
                &name,
            ),
        };
        if let PanelState::Chart(m) = &panel.state {
            panel.linked = field
                .as_deref()
                .is_some_and(|f| m.x.kind == AxisKind::Time && m.x.field.as_deref() == Some(f));
        }
        self.push(panel)
    }

    /// A chart's model from its tables, or why not.
    fn chart(
        &self,
        chart: crate::spec::Chart,
        tables: Vec<Arc<Table>>,
        notes: Vec<String>,
        name: &str,
        scope: &Scope<'_>,
    ) -> PanelState {
        let fail = |e: FigError| {
            let e = notes.iter().fold(scope.placed(e), |e, n| e.note(n.clone()));
            PanelState::Failed(Failure {
                errors: vec![e],
                name: name.to_string(),
            })
        };
        if !tables.is_empty() && tables.iter().all(|t| t.rows == 0) {
            return fail(
                FigError::new("nothing to draw: the data has no rows").with_kind(ErrorKind::Empty),
            );
        }
        match ChartModel::build(chart, tables, self.zone) {
            Ok(m)
                if m.series.iter().all(|s| s.xs.is_empty())
                    && m.rules.is_empty()
                    && m.texts.is_empty() =>
            {
                fail(FigError::new("nothing to draw: no rows to chart").with_kind(ErrorKind::Empty))
            }
            Ok(m) => PanelState::Chart(m),
            Err(e) => fail(e),
        }
    }

    /// The table a data spec comes to, in `scope`; `refs` are the `#name` references followed
    /// to get here, for their cycles.
    fn table(&mut self, d: &DataSpec, scope: &Scope<'_>, refs: &[(PathBuf, String)]) -> Got {
        match &d.source {
            DataRef::Inline(t) => Got::Table(t.clone(), None),
            DataRef::File(_) => self.want(Want {
                data: d.clone(),
                dir: PathBuf::new(),
            }),
            DataRef::Run { cmd, every } => self.command(d, cmd, *every, false, scope),
            DataRef::Stream { cmd } => self.command(d, cmd, None, true, scope),
            DataRef::Named(n) => match scope.data.iter().find(|(k, _)| k == n) {
                Some((_, ds)) => self.table(&ds.clone(), scope, refs),
                None => {
                    let have: Vec<String> =
                        scope.data.iter().map(|(k, _)| format!("\"{k}\"")).collect();
                    let msg = match have.is_empty() {
                        true => format!("no data \"{n}\" here: this dashboard names none"),
                        false => format!("no data \"{n}\" here; it has {}", have.join(", ")),
                    };
                    Got::Failed(scope.placed(FigError::new(msg)))
                }
            },
            DataRef::Borrow { file, name, at } => self.borrow(file, name, *at, scope, refs),
            DataRef::Source => Got::Failed(scope.placed(FigError::new(
                "a panel's data can't be \"-\": the buffer is the dashboard's spec",
            ))),
        }
    }

    /// A command's output: asked for, and wanted.
    fn command(
        &mut self,
        d: &DataSpec,
        cmd: &str,
        every: Option<Duration>,
        stream: bool,
        scope: &Scope<'_>,
    ) -> Got {
        let ask = NeedsTrust::Command {
            spec: scope.file.clone(),
            cmd: cmd.to_string(),
            every,
            stream,
        };
        if !self.trust.contains(&ask) {
            self.trust.push(ask);
        }
        self.want(Want {
            data: d.clone(),
            dir: scope.dir.clone(),
        })
    }

    /// Another dashboard's data, `file#name`.
    fn borrow(
        &mut self,
        file: &Path,
        name: &str,
        at: Pos,
        scope: &Scope<'_>,
        refs: &[(PathBuf, String)],
    ) -> Got {
        let what = format!("{}#{name}", shown(file));
        let err = |msg: String| Got::Failed(scope.placed(FigError::at(msg, at)));
        if refs
            .iter()
            .any(|(f, n)| n == name && crate::source::same_path(f, file))
        {
            let mut path: Vec<String> = refs
                .iter()
                .map(|(f, n)| format!("{}#{n}", shown(f)))
                .collect();
            path.push(what.clone());
            return err(format!(
                "{what} refers back to itself: {}",
                path.join(" → ")
            ));
        }
        if refs.len() >= MAX_DEPTH {
            return err(format!("data references nest at most {MAX_DEPTH} deep"));
        }
        let figure = match (self.resolve)(file) {
            Ok(f) => f,
            Err(e) => {
                let e = match e.file {
                    Some(_) => e,
                    None => e.in_file(file, None),
                };
                return Got::Failed(e.note(format!("borrowing {what}")));
            }
        };
        let Figure::Dashboard(d) = &figure else {
            return err(format!(
                "{} is {}, not a dashboard: it has no data to borrow",
                shown(file),
                kind_name(&figure)
            ));
        };
        let Some((_, ds)) = d.data.iter().find(|(k, _)| k == name) else {
            let have: Vec<String> = d.data.iter().map(|(k, _)| format!("\"{k}\"")).collect();
            return err(match have.is_empty() {
                true => format!("{} names no data", shown(file)),
                false => format!(
                    "{} has no data \"{name}\"; it has {}",
                    shown(file),
                    have.join(", ")
                ),
            });
        };
        let inner = Scope {
            data: &d.data,
            file: Some(file.to_path_buf()),
            dir: file.parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        let mut deeper = refs.to_vec();
        deeper.push((file.to_path_buf(), name.to_string()));
        self.table(&ds.clone(), &inner, &deeper)
    }

    /// A source read, waiting or failed: wanted either way.
    fn want(&mut self, w: Want) -> Got {
        if !self.wants.contains(&w) {
            self.wants.push(w.clone());
        }
        match self.sources.get(&w) {
            Some(SourceState::Ready(t)) => {
                let note = (!w.is_command()).then(|| {
                    let rows = if t.rows == 1 { "row" } else { "rows" };
                    let cols: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
                    format!("read {}: {} {rows}; {}", w.name(), t.rows, cols.join(", "))
                });
                Got::Table(t.clone(), note)
            }
            Some(SourceState::Waiting(s)) => Got::Waiting(s.clone()),
            Some(SourceState::Failed(e)) => Got::Failed(e.clone()),
            None if w.is_command() => Got::Waiting(format!("waiting for {}", w.name())),
            None => Got::Waiting(format!("reading {}", w.name())),
        }
    }
}

/// Reads each data file `wants` names, from disk, each to its table or why not. A command's
/// output isn't read here: a panel on one waits for it.
pub fn read(wants: &[Want], opt: &ReadOptions) -> Sources {
    let mut out = Sources::new();
    for w in wants {
        let DataRef::File(path) = &w.data.source else {
            continue;
        };
        let name = shown(path);
        let state = match crate::source::read_file(path) {
            Err(e) => SourceState::Failed(e.in_file(path, None)),
            Ok(text) => match crate::source::read_data(&text, &w.data, &name, opt) {
                Ok(t) => SourceState::Ready(Arc::new(t)),
                Err(e) => {
                    SourceState::Failed(e.in_file(path, Some(&text)).note(format!("read {name}")))
                }
            },
        };
        out.insert(w.clone(), state);
    }
    out
}

/// A `.fig` file's figure, read and parsed from its own directory: what a `file` panel or a
/// `#name` reference comes to.
pub fn open(path: &Path, opt: &ReadOptions) -> Result<Figure> {
    let text = crate::source::read_file(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    crate::spec::parse(&text, &shown(path), dir, opt)
        .map(|p| p.figure)
        .map_err(|e| e.in_file(path, Some(&text)))
}

/// A file's name, or its whole path if it has none.
fn shown(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// What a figure is, with its article, for a message.
fn kind_name(f: &Figure) -> &'static str {
    match f {
        Figure::Chart(_) => "a chart",
        Figure::Graph(_) => "a graph",
        Figure::Stat(_) => "a stat",
        Figure::Meter(_) => "a meter",
        Figure::Sparkline(_) => "a sparkline",
        Figure::Dashboard(_) => "a dashboard",
    }
}
