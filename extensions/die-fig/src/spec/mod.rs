//! What a buffer's text says to draw: a [`Figure`], from a fig file, a spec, or bare data with a
//! guessed figure.
//!
//! - [`fig`]: fig's own `graph`, `line` and `histogram` shapes, found by their fields (`nodes`;
//!   `series` with `points`; `buckets`) and desugared into these types;
//! - [`chart`]: the grammar: marks, encodings, layers, transforms, inheritance;
//! - [`graph`]: the graph spec beyond fig's;
//! - [`dash`]: dashboards, stats and meters;
//! - [`tile`]: the sparkline figure;
//! - [`gantt`]: gantt charts: tasks over time, inline or from a table;
//! - [`guess`]: a figure from a bare table, by the design's inference rules;
//! - [`write`]: a figure back to JSON text, for `:fig-spec`.
//!
//! `"type"` is one of `chart`, `line`, `area`, `bar`, `scatter`, `heatmap`, `histogram`,
//! `graph`, `gantt`, `stat`, `meter`, `sparkline` and `dashboard`. Unknown fields are tolerated: a node's and an
//! edge's are kept in their `raw` object, and data rows keep theirs in [`Table::raw`]. A key
//! within two edits of a known one (one for a known key of three or four letters, none for one
//! or two) is a warning that names the key it may have meant, but in a node or an edge, whose
//! other keys are its data. Every error has its place.
//!
//! # What a buffer is
//!
//! [`parse`] goes by the file's extension first (`.csv`, `.tsv` and `.tab`, `.jsonl` and
//! `.ndjson`, `.json`), then by the text: JSON starting `{` or `[`, JSONL when the whole text
//! isn't one value but its first line is an object, else CSV or TSV. A JSON object is a spec
//! when it has a string `"type"`, a `"mark"`, `"layers"`, `"nodes"`, `"tasks"` or `"buckets"`, or
//! fig's
//! `"series"` with `"points"`; an object whose every value is an array is data, as is a JSON
//! array. One line of JSONL whose `"type"` is no figure's and that has none of a figure's
//! `data`, `mark`, `layers`, `nodes`, `edges`, `series`, `buckets`, `rows` or `tasks` is a row of
//! data,
//! when it makes a figure; else the spec's error stands. Bare data is read and [`guess`]ed, and
//! the guess's data is the table itself; but a table with a `start` column of times and an `end`
//! or a `duration` is a gantt ([`gantt::from_table`]), and one with an `id` and a `parent`
//! column is a tree, a [`GraphSpec`] of a node a row ([`graph::from_table`]).
//!
//! `dir` is made absolute against the working directory first (an empty one stays empty), so a
//! file's path, which [`write`] gives as it was resolved, reads back as the same path.
//!
//! # The grammar
//!
//! A chart (`"type": "chart"`, or a shorthand: `line`, `area`, `bar`, `scatter` for points and
//! `heatmap` for rects) has these keys, all optional but `data` and a mark:
//!
//! - `title`, `subtitle`: strings;
//! - `data`: a file's name, relative to the spec's directory (`"-"` is the buffer itself, and
//!   in a dashboard a name of its `data` is that data), or an object with one of `file`,
//!   `values` (rows as JSON, or CSV text), `run` (a command, with `every`: `"30s"`, `"5m"`,
//!   `"1h"`, `"500ms"`, `"1d"`) or `stream`, and the reading hints `format` (`csv`, `tsv`,
//!   `json`, `jsonl`), `delimiter`, `header`, `thousands` and `time_format` (strtime);
//! - `transform`: a list, each an object with one of `fold` (`["a", "b"]`, `"as": [key,
//!   value]`), `filter` (a field with `in`, `not_in`, `range: [lo, hi]` or `min`/`max`), `bin`
//!   (a field, `step`, `max_bins`, `"as": [start, end]`), `aggregate` (`"count"` or `{"sum":
//!   field}`, `by`, `as`), `cumulative` (a field, `by`, `as`), `window` (a field, `size`, `by`,
//!   `as`), `sort` (a field, `descending`) or `top` (a count, `by`, `field`);
//! - the encodings `x`, `y`, `y2`, `color`, `size` and `label`: each a field's name, a number
//!   (a constant), or an object of `field` or `value`, `type` (`number`, `time`, `category`,
//!   `text`), `unit` (`s` or `ms`, for epoch times), `title`, `scale` (`auto`, `linear`, `log`,
//!   `time`, `band`), `domain` (`[lo, hi]`), `zero` and `format`; and `tooltip` and `detail`, a
//!   field or a list of them. A heatmap's `color` takes `"scale": "diverging"` or `{"type":
//!   "diverging", "mid": 0}` for values either side of a midpoint (0 unless given), and
//!   `"sequential"` (the default) for a ramp from the least to the most;
//! - `mark`: `line`, `area`, `bar`, `point`, `rect`, `rule`, `band` or `text`, or an object of
//!   `type`, `curve` (`linear`, `step`), `points`, `stack` (`none`, `zero`, `normalize`, or a
//!   boolean) and `horizontal`. Bars and areas stack from zero unless told otherwise. This is
//!   layer 0, and the top's `fig` block is its block;
//! - `layers`: more layers, each with `mark`, its own encodings, `data`, `transform` and `fig`.
//!   A layer takes every encoding of the top's that it doesn't give itself; `null` drops one;
//! - `series`: each series' `fig` block by its name, `{"api": {"color": "red"}}`; a list of
//!   blocks for several series of one name, the nth of the list for the nth series;
//! - `view`: `grid`, `axes`, `legend` (`top`, `right`, `none`), `labels` (`auto`, `ends`,
//!   `none`), `render_options` and `motion`. fig's `fig.menu` (`plane.axis_lines`,
//!   `plane.grid_lines`, `render_options`) is read first, and `view` over it.
//!
//! `"type": "histogram"` without fig's `buckets` counts `x` in bins (`step`, or at most `bins`
//! of them, 20 by default), or sums `y` in them, as stacked bars from each bin's start.
//!
//! # fig's shapes
//!
//! fig's `line` is a long table `(series, x, y)` drawn as one line layer coloured by series:
//! its `series` column is a category with one entry a series, in the file's order, so two
//! series of one name stay two (group by the code, not the string). Each series' `fig` block
//! goes to [`Chart::series_fig`] under the [`SeriesKey`] of its name in the data (its
//! `"label"`) and which of that name it is. fig's `histogram` is a table `(bucket, series,
//! value)` drawn as stacked bars on a band x, one band a bucket (one entry a bucket, too).
//! `x_label` and `y_label` are the channels' titles. A graph is [`GraphSpec`], with `source`
//! and `target` for `from` and `to`, and fig's messages.

pub mod chart;
pub mod dash;
pub mod fig;
pub mod gantt;
pub mod graph;
pub mod guess;
pub mod tile;
pub mod write;

pub use guess::guess;
pub use tile::SparklineSpec;
pub use write::write;

use crate::data::transform::{AggOp, Transform};
use crate::data::{ColumnKind, ReadOptions, SeriesKey, Table};
use crate::error::{FigError, Pos, Result};
use crate::json::{self, Key, Kind, Node};
use crate::style::{FigStyle, Status};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Where a figure's data comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum DataRef {
    /// In the spec: fig's series or buckets, or `"data": {"values": …}`.
    Inline(Arc<Table>),
    /// A file, relative to the spec's directory, resolved.
    File(PathBuf),
    /// A command's output, run again `every` so long if given.
    Run {
        /// The command line.
        cmd: String,
        /// How often it runs again.
        every: Option<Duration>,
    },
    /// A command whose output goes on, read as it comes.
    Stream {
        /// The command line.
        cmd: String,
    },
    /// A dashboard's data, by its key.
    Named(String),
    /// The buffer itself is the data.
    Source,
    /// Another dashboard's data, by its key: `"x.fig#req"`, the file relative to the spec's
    /// directory, resolved. A dashboard's panels and data may borrow it.
    Borrow {
        /// The `.fig` file whose dashboard has the data.
        file: PathBuf,
        /// Its key in that dashboard's `data`.
        name: String,
        /// Where the reference is in the spec.
        at: Pos,
    },
}

/// A figure's data: where from, in what format, and how to read it.
#[derive(Clone, Debug, PartialEq)]
pub struct DataSpec {
    /// Where from.
    pub source: DataRef,
    /// The format, if the spec says; else by the name or the text.
    pub format: Option<DataFormat>,
    /// How to read it.
    pub read: ReadHints,
}

/// A data format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFormat {
    /// Comma-separated.
    Csv,
    /// Tab-separated.
    Tsv,
    /// JSON rows.
    Json,
    /// A JSON object a line.
    Jsonl,
}

/// What a spec says about reading its data, over [`ReadOptions`]'s defaults.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ReadHints {
    /// The delimiter.
    pub delimiter: Option<u8>,
    /// Whether the first row is a header.
    pub header: Option<bool>,
    /// A thousands separator.
    pub thousands: Option<char>,
    /// A strtime format for times.
    pub time_format: Option<String>,
}

/// A mark's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// Lines through the points.
    Line,
    /// The area under a line.
    Area,
    /// Bars.
    Bar,
    /// Points.
    Point,
    /// Rectangles: a heatmap's cells.
    Rect,
    /// A rule: a reference line.
    Rule,
    /// A band between two values.
    Band,
    /// Text at the points.
    Text,
}

/// How a line goes from point to point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// Straight.
    Linear,
    /// In steps.
    Step,
}

/// How marks of several series stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    /// They don't.
    None,
    /// On each other from zero.
    Zero,
    /// On each other, as parts of the whole.
    Normalize,
}

/// A layer's mark.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// What it is.
    pub kind: MarkKind,
    /// How a line goes.
    pub curve: Curve,
    /// Points drawn on a line.
    pub points: bool,
    /// How series stack.
    pub stack: Stack,
    /// Bars across rather than up.
    pub horizontal: bool,
}

/// A channel's scale, as a spec asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleKind {
    /// By the data.
    Auto,
    /// Linear.
    Linear,
    /// Logarithmic.
    Log,
    /// Time.
    Time,
    /// Bands, one a category.
    Band,
}

/// A constant a channel takes in place of a field.
#[derive(Clone, Debug, PartialEq)]
pub enum ChannelValue {
    /// A number.
    Num(f64),
    /// A string.
    Str(String),
}

/// One encoding channel: a field or a constant, and how it's read and shown.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Channel {
    /// The column it shows.
    pub field: Option<String>,
    /// A constant, in place of a field.
    pub value: Option<ChannelValue>,
    /// What the column is taken as, over inference.
    pub kind: Option<ColumnKind>,
    /// For epoch times: `"s"` or `"ms"`.
    pub unit: Option<String>,
    /// Its axis or legend title.
    pub title: Option<String>,
    /// Its scale.
    pub scale: Option<ScaleKind>,
    /// Its domain, over the data's.
    pub domain: Option<(f64, f64)>,
    /// Whether the domain takes in zero.
    pub zero: Option<bool>,
    /// How its values are written.
    pub format: Option<String>,
    /// A colour scale's midpoint: Some makes a heatmap's colours diverge either side of it
    /// (`"scale": {"type": "diverging", "mid": 0}`), None is a sequential ramp.
    pub mid: Option<f64>,
    /// Where it is in the spec.
    pub at: Option<Pos>,
}

/// What each channel of a layer shows.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Encoding {
    /// Across.
    pub x: Option<Channel>,
    /// Up.
    pub y: Option<Channel>,
    /// A second y: a band's or a rule's other end.
    pub y2: Option<Channel>,
    /// The series, by colour.
    pub color: Option<Channel>,
    /// A point's size.
    pub size: Option<Channel>,
    /// A label.
    pub label: Option<Channel>,
    /// The fields the card shows.
    pub tooltip: Vec<String>,
    /// The fields the detail panel shows.
    pub detail: Vec<String>,
}

/// One layer of a chart.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// Its mark.
    pub mark: Mark,
    /// Its encodings, the chart's inherited.
    pub enc: Encoding,
    /// Its own data, over the chart's.
    pub data: Option<DataSpec>,
    /// Its own transforms, after the chart's.
    pub transform: Vec<Transform>,
    /// Its `fig` block.
    pub fig: FigStyle,
}

/// How a figure is shown, from the spec's `fig.menu` and the settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSettings {
    /// Gridlines.
    pub grid: bool,
    /// Axes.
    pub axes: bool,
    /// Where the legend is.
    pub legend: LegendPos,
    /// Which labels are drawn on the marks.
    pub labels: Labels,
    /// Whether the data's `fig` blocks are read (fig's data-driven rendering).
    pub render_options: bool,
    /// Whether discrete changes ease.
    pub motion: bool,
}

impl Default for ViewSettings {
    /// Grid, axes, the legend on top, labels by the figure's rule, the data's own look, and
    /// motion. An absent `fig.menu.render_options` is true, as fig's app has it.
    fn default() -> ViewSettings {
        ViewSettings {
            grid: true,
            axes: true,
            legend: LegendPos::Top,
            labels: Labels::Auto,
            render_options: true,
            motion: true,
        }
    }
}

/// Where a legend is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LegendPos {
    /// Above the plot.
    #[default]
    Top,
    /// To its right.
    Right,
    /// Nowhere.
    None,
}

/// Which labels marks carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Labels {
    /// The figure's rule.
    #[default]
    Auto,
    /// Each line's name at its end.
    Ends,
    /// None.
    None,
}

/// A chart: layers of marks over shared data.
#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    /// Its title.
    pub title: Option<String>,
    /// The line under the title.
    pub subtitle: Option<String>,
    /// Its data.
    pub data: DataSpec,
    /// The transforms on it, in order.
    pub transform: Vec<Transform>,
    /// The layers, the top-level encodings already inherited.
    pub layers: Vec<Layer>,
    /// Each series' `fig` block, as fig's line series carry them.
    pub series_fig: Vec<(SeriesKey, FigStyle)>,
    /// How it's shown.
    pub view: ViewSettings,
}

/// How a graph's nodes are laid out.
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    /// The spec doesn't say: the graph picks (a tree for a forest or a table's parents,
    /// layers for a directed acyclic graph, else fig's stress layout; see
    /// [`crate::graph::GraphModel::build`]).
    Auto,
    /// fig's stress layout.
    Force,
    /// A tidy tree from a root, or from every node with no parent.
    Tree {
        /// The root's id.
        root: Option<String>,
    },
    /// Layers, for a DAG.
    Layered,
}

/// A graph's node.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    /// Its id, unique.
    pub id: String,
    /// Its label, else its id.
    pub label: Option<String>,
    /// Its group, which colours it.
    pub group: Option<String>,
    /// Its size.
    pub size: Option<f64>,
    /// Its `fig` block.
    pub fig: FigStyle,
    /// Its object in the spec, for the detail panel.
    pub raw: Node,
    /// Where it is in the spec.
    pub at: Pos,
}

/// A graph's edge.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEdge {
    /// Where it starts: a node's id.
    pub from: String,
    /// Where it ends: a node's id.
    pub to: String,
    /// Its label.
    pub label: Option<String>,
    /// Its weight.
    pub weight: Option<f64>,
    /// Whether it has a direction.
    pub directed: bool,
    /// Its `fig` block.
    pub fig: FigStyle,
    /// Its object in the spec.
    pub raw: Node,
    /// Where it is in the spec.
    pub at: Pos,
}

/// A network graph.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphSpec {
    /// Its title.
    pub title: Option<String>,
    /// The line under the title.
    pub subtitle: Option<String>,
    /// Its nodes.
    pub nodes: Vec<GraphNode>,
    /// Its edges.
    pub edges: Vec<GraphEdge>,
    /// How it's laid out.
    pub layout: Layout,
    /// How it's shown.
    pub view: ViewSettings,
}

/// Which column of a gantt's rows is what. Each is a channel naming a field (`"start":
/// "begins"`, or `{"field": "ts", "type": "time", "unit": "s"}` for epoch numbers); None finds
/// the column by its usual names ([`gantt::names`]).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GanttFields {
    /// A task's id, which `after` and `parent` name.
    pub id: Option<Channel>,
    /// Its name, written at its row.
    pub label: Option<Channel>,
    /// When it starts.
    pub start: Option<Channel>,
    /// When it ends.
    pub end: Option<Channel>,
    /// How long it takes, in place of a start or an end: `"3d"`, `"1w"`, `"36h"`, or a number
    /// of days.
    pub duration: Option<Channel>,
    /// The tasks it comes after: an id, or several.
    pub after: Option<Channel>,
    /// Its group: a heading its tasks sit under.
    pub group: Option<Channel>,
    /// The task it's a part of.
    pub parent: Option<Channel>,
    /// The share of it done: 0 to 1, or 0 to 100 when any task's is above 1.
    pub progress: Option<Channel>,
    /// Whether it's a milestone: a moment, not a span.
    pub milestone: Option<Channel>,
    /// What colours the bars, in place of the group.
    pub color: Option<Channel>,
}

/// Where a gantt's today line is.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Today {
    /// At the clock's now: the default.
    #[default]
    Now,
    /// At a time the spec gives, in UTC seconds: a plan read later, a picture that mustn't
    /// change.
    At(f64),
    /// Nowhere.
    Off,
}

/// A time a gantt's spec gives, with the text it was written as: a date without a time of day
/// takes in its whole day where it ends a span in a plan of whole days, and is written back as
/// it was.
#[derive(Clone, Debug, PartialEq)]
pub struct When {
    /// The time, in UTC seconds.
    pub secs: f64,
    /// As the spec wrote it.
    pub text: Box<str>,
}

impl When {
    /// Whether it was written as a date alone, with no time of day.
    pub fn is_date(&self) -> bool {
        !self.text.contains(['T', 't', ':'])
    }
}

/// The days a gantt's durations count, in a plan of whole days: every day unless it says.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Calendar {
    /// Monday to Friday only: `"calendar": "weekdays"`.
    pub weekdays: bool,
    /// Days off besides: each a day, or every day from the first to the second.
    pub off: Vec<(When, Option<When>)>,
}

/// A mark on a gantt's time: a line at a moment (a deadline, a release), or a shaded span (a
/// freeze, a holiday) when it has an end.
#[derive(Clone, Debug, PartialEq)]
pub struct GanttMark {
    /// Where the line is (`at`), or where the span starts (`start`).
    pub at: When,
    /// Where a span ends; None for a line.
    pub end: Option<When>,
    /// What it's called.
    pub label: Option<String>,
    /// Its `fig` block.
    pub fig: FigStyle,
    /// Where it is in the spec.
    pub pos: Pos,
}

/// A gantt chart: tasks over time, a row each, with what each comes after.
///
/// The tasks are rows of a table: `tasks` inline (JSON objects, kept in [`Table::raw`]) or
/// `data` as any chart's. [`crate::gantt::GanttModel`] resolves them: see its docs for what a
/// task's fields mean.
#[derive(Clone, Debug, PartialEq)]
pub struct GanttSpec {
    /// Its title.
    pub title: Option<String>,
    /// The line under the title.
    pub subtitle: Option<String>,
    /// Its tasks' rows.
    pub data: DataSpec,
    /// Whether the rows were written as `tasks` rather than `data`: [`write`] writes them back
    /// so.
    pub tasks: bool,
    /// The transforms on the rows, in order.
    pub transform: Vec<Transform>,
    /// Which column is what.
    pub fields: GanttFields,
    /// The days its durations count.
    pub calendar: Calendar,
    /// The today line.
    pub today: Today,
    /// Lines and spans on its time.
    pub marks: Vec<GanttMark>,
    /// The fields the card shows, after its own.
    pub tooltip: Vec<String>,
    /// The fields the detail panel shows; empty shows them all.
    pub detail: Vec<String>,
    /// The arrows from each task to those that come after it (`view.links`).
    pub links: bool,
    /// How it's shown: `grid`, `axes`, `legend`, `render_options` and `motion`, as a chart's.
    pub view: ViewSettings,
}

/// A stat tile: one number, its change and its trend.
#[derive(Clone, Debug, PartialEq)]
pub struct StatSpec {
    /// What it's of.
    pub label: String,
    /// Its data.
    pub data: DataSpec,
    /// The value shown.
    pub value: Aggregate,
    /// The change shown beside it.
    pub delta: Option<Delta>,
    /// The field of a sparkline under it.
    pub trend: Option<String>,
    /// How the value is written.
    pub format: Option<String>,
    /// What the value is counted in, written small after it: `"ms"`, `"%"`.
    pub unit: Option<String>,
    /// Which way is good.
    pub good: GoodWhen,
}

/// One value made from a column.
#[derive(Clone, Debug, PartialEq)]
pub struct Aggregate {
    /// How.
    pub op: AggOp,
    /// The column; None only for a count.
    pub field: Option<String>,
}

/// What a stat's change is against.
#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    /// The previous period.
    Previous,
    /// A fixed value.
    Value(f64),
}

/// Which way a stat's change is good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoodWhen {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Neither.
    Neither,
}

/// A meter tile: a value against a maximum.
#[derive(Clone, Debug, PartialEq)]
pub struct MeterSpec {
    /// What it's of.
    pub label: String,
    /// Its data.
    pub data: DataSpec,
    /// The value shown.
    pub value: Aggregate,
    /// The full meter's value.
    pub max: f64,
    /// How the value is written.
    pub format: Option<String>,
    /// What the value is counted in: `"GB"`.
    pub unit: Option<String>,
    /// Where the meter's fill changes status, lowest first.
    pub thresholds: Vec<Threshold>,
}

/// A meter's threshold: past `at` (a fraction of the meter's max) its fill takes `status`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Threshold {
    /// Where, as a fraction of the max: 0 to 1.
    pub at: f64,
    /// The status the fill takes past it.
    pub status: Status,
}

/// A dashboard panel's content.
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    /// A figure in the spec.
    Figure(Box<Figure>),
    /// A spec in another file, relative to this one's directory: that whole figure, its paths
    /// from its own directory, a dashboard drawn as a nested group.
    File(PathBuf),
    /// Panels stacked down one column of the row.
    Column(Vec<PanelSpec>),
}

/// A dashboard panel and its share of its row's width.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelSpec {
    /// What it shows.
    pub panel: Panel,
    /// Its share of the row's width, against the other panels' (1 by default).
    pub span: u16,
    /// Where it is in the spec.
    pub at: Pos,
}

/// A row of a dashboard: its panels, left to right, and its share of the height.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its panels.
    pub panels: Vec<PanelSpec>,
    /// Its share of the height left after the rows of tiles, against the other rows' (1 when
    /// None: the shorthand, a row written as a list of panels).
    pub height: Option<f64>,
    /// Where it is in the spec.
    pub at: Pos,
}

/// Which panels show a card when the pointer is over a linked panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tooltip {
    /// The hovered panel alone; the others draw the moment's line and dots.
    #[default]
    Single,
    /// Every linked panel (`"tooltip": "shared"`).
    Shared,
}

/// A dashboard: rows of panels over shared data and a shared time range.
#[derive(Clone, Debug, PartialEq)]
pub struct DashboardSpec {
    /// Its title.
    pub title: Option<String>,
    /// Its data, by key, in the spec's order.
    pub data: Vec<(String, DataSpec)>,
    /// The time range its panels share.
    pub time: Option<TimeSpec>,
    /// Which panels show a card on a hover.
    pub tooltip: Tooltip,
    /// Its rows of panels.
    pub rows: Vec<Row>,
}

/// A dashboard's shared time range.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeSpec {
    /// The time field the panels share.
    pub field: String,
    /// The range shown, as `"24h"`.
    pub range: Option<String>,
}

/// Something to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Figure {
    /// A chart.
    Chart(Chart),
    /// A network graph.
    Graph(GraphSpec),
    /// A gantt chart.
    Gantt(GanttSpec),
    /// A stat tile.
    Stat(StatSpec),
    /// A meter tile.
    Meter(MeterSpec),
    /// A sparkline: one series' trend, small.
    Sparkline(SparklineSpec),
    /// A dashboard.
    Dashboard(DashboardSpec),
}

/// What a buffer's text turned out to be.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    /// The figure.
    pub figure: Figure,
    /// Whether it was guessed from bare data.
    pub inferred: bool,
    /// What was odd but not wrong: a misspelt field, a value ignored.
    pub warnings: Vec<FigError>,
}

/// Parses a buffer's text: a fig file, a spec, or bare data (CSV, TSV, JSON rows, JSONL), which
/// is read and given a guessed figure. `name` is the file name (for the format by extension),
/// `dir` resolves relative paths, made absolute against the working directory first.
pub fn parse(text: &str, name: &str, dir: &Path, opt: &ReadOptions) -> Result<Parsed> {
    let dir = absolute(dir);
    let mut cx = Cx::new(&dir, opt);
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = body
        .trim_start_matches([' ', '\t', '\r', '\n'])
        .chars()
        .next();
    let Some(first) = first else {
        return Err(FigError::new("nothing to draw: the buffer is empty")
            .with_kind(crate::error::ErrorKind::Empty));
    };
    let (figure, inferred) = match format_of_name(name) {
        Some(f @ (DataFormat::Csv | DataFormat::Tsv)) => (bare_csv(text, f, true, opt)?, true),
        Some(DataFormat::Jsonl) => (bare_jsonl(text, opt)?, true),
        Some(DataFormat::Json) => json_text(text, &mut cx)?,
        None if matches!(first, '{' | '[') => json_text(text, &mut cx)?,
        None => (bare_csv(text, sniff(body), false, opt)?, true),
    };
    Ok(Parsed {
        figure,
        inferred,
        warnings: cx.warnings,
    })
}

/// `dir` against the working directory if it's relative; an empty one, or one that can't be,
/// as it is.
fn absolute(dir: &Path) -> PathBuf {
    if dir.is_absolute() {
        return dir.to_path_buf();
    }
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// Where a text starts: the place of an error about the whole of it.
pub(crate) const START: Pos = Pos {
    line: 1,
    col: 1,
    byte: 0,
};

/// The format a file's name says, by its extension in any case.
pub fn format_of_name(name: &str) -> Option<DataFormat> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "csv" => Some(DataFormat::Csv),
        "tsv" | "tab" => Some(DataFormat::Tsv),
        "jsonl" | "ndjson" => Some(DataFormat::Jsonl),
        "json" => Some(DataFormat::Json),
        _ => None,
    }
}

/// CSV or TSV by the first line: TSV when it has more tabs than commas.
fn sniff(body: &str) -> DataFormat {
    let line = body.lines().next().unwrap_or("");
    let count = |c: char| line.chars().filter(|&x| x == c).count();
    if count('\t') > count(',') {
        DataFormat::Tsv
    } else {
        DataFormat::Csv
    }
}

/// A JSON text: a spec, bare JSON rows, or JSONL that isn't one value.
fn json_text(text: &str, cx: &mut Cx<'_>) -> Result<(Figure, bool)> {
    match json::parse(text) {
        Ok(node) if is_spec(&node) => {
            let before = cx.warnings.len();
            match figure(&node, cx) {
                Ok(f) => Ok((f, false)),
                Err(e) if is_row(&node) => match bare_jsonl(text, cx.opt) {
                    Ok(f) => {
                        cx.warnings.truncate(before);
                        Ok((f, true))
                    }
                    Err(_) => Err(e),
                },
                Err(e) => Err(e),
            }
        }
        Ok(node) => {
            let table = crate::data::rows::read_json(&node, cx.opt)?;
            Ok((inferred(table, DataFormat::Json)?, true))
        }
        Err(e) if looks_like_jsonl(text) => match bare_jsonl(text, cx.opt) {
            Ok(f) => Ok((f, true)),
            Err(_) => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// Whether a text that isn't one JSON value is JSONL: more than one line that isn't blank, the
/// first of them an object.
fn looks_like_jsonl(text: &str) -> bool {
    let lines = json::parse_lines(text);
    lines.len() > 1 && matches!(&lines[0], Ok(n) if n.as_obj().is_some())
}

fn bare_csv(text: &str, f: DataFormat, by_name: bool, opt: &ReadOptions) -> Result<Figure> {
    let mut o = opt.clone();
    if by_name && f == DataFormat::Tsv && o.delimiter.is_none() {
        o.delimiter = Some(b'\t');
    }
    let table = crate::data::csv::read_csv(text, &o)?;
    inferred(table, f)
}

fn bare_jsonl(text: &str, opt: &ReadOptions) -> Result<Figure> {
    let table = crate::data::rows::read_jsonl(text, opt)?;
    inferred(table, DataFormat::Jsonl)
}

/// The guess at a table read from bare data, its format kept: a gantt when it has a start
/// column of times and an end or a duration ([`gantt::from_table`]), a tree when it has an id
/// and a parent column ([`graph::from_table`]), else a chart.
fn inferred(table: Table, f: DataFormat) -> Result<Figure> {
    if let Some(mut g) = gantt::from_table(&table) {
        g.data.format = Some(f);
        return Ok(Figure::Gantt(g));
    }
    if let Some(g) = graph::from_table(&table) {
        return g.map(Figure::Graph);
    }
    let mut chart = guess(&table).map_err(|e| placed(e, START))?;
    chart.data.format = Some(f);
    Ok(Figure::Chart(chart))
}

/// An error given a place if it has none; not one of nothing to draw, which is about the
/// whole of the text.
pub(crate) fn placed(e: FigError, at: Pos) -> FigError {
    match e.at {
        Some(_) => e,
        None if e.kind == crate::error::ErrorKind::Empty => e,
        None => FigError { at: Some(at), ..e },
    }
}

/// Whether a JSON value is a spec rather than data: see the module's docs.
fn is_spec(node: &Node) -> bool {
    let Some(obj) = node.as_obj() else {
        return false;
    };
    let rows_of_objects = |k: &str| matches!(node.get(k).and_then(Node::as_arr), Some(a) if a.first().is_none_or(|e| e.as_obj().is_some()));
    if node.get("type").and_then(Node::as_str).is_some()
        || node
            .get("mark")
            .is_some_and(|m| m.as_str().is_some() || m.as_obj().is_some())
        || rows_of_objects("layers")
        || rows_of_objects("nodes")
        || rows_of_objects("tasks")
        || rows_of_objects("buckets")
        || fig::is_line_series(node.get("series"))
    {
        return true;
    }
    obj.is_empty() || !obj.iter().all(|(_, v)| v.as_arr().is_some())
}

/// Every figure type, for messages.
const TYPES: &str = "chart, line, area, bar, scatter, heatmap, histogram, graph, gantt, stat, meter, sparkline or dashboard";

/// Whether an object with a `"type"` is more likely a row of data than a spec: its type is no
/// figure's, and it has none of the keys a figure draws from.
fn is_row(node: &Node) -> bool {
    let figure_type = |t: &str| {
        chart::Shorthand::from_name(t).is_some()
            || ["graph", "gantt", "stat", "meter", "sparkline", "dashboard"].contains(&t)
    };
    let draws = [
        "data", "mark", "layers", "nodes", "edges", "series", "buckets", "rows", "tasks",
    ];
    node.get("type")
        .and_then(Node::as_str)
        .is_some_and(|t| !figure_type(t))
        && draws.iter().all(|k| node.get(k).is_none())
}

/// The figure a spec's object describes, by its `"type"` or, failing that, by fig's fields.
pub(crate) fn figure(node: &Node, cx: &mut Cx<'_>) -> Result<Figure> {
    obj(node, "a figure")?;
    let ty = match node.get("type") {
        Some(t) if !t.is_null() => Some((string(t, "type")?, t.at)),
        _ => None,
    };
    let has = |k: &str| field(node, k).is_some();
    let Some((ty, at)) = ty else {
        return if has("nodes") {
            fig::graph(node, cx).map(Figure::Graph)
        } else if has("tasks") {
            gantt::parse(node, cx).map(Figure::Gantt)
        } else if has("buckets") {
            fig::histogram(node, cx).map(Figure::Chart)
        } else if field(node, "series").is_some_and(|s| s.as_arr().is_some()) {
            fig::line(node, cx).map(Figure::Chart)
        } else if has("mark") || has("layers") {
            chart::parse(node, chart::Shorthand::Chart, cx).map(Figure::Chart)
        } else {
            Err(FigError::at(
                format!("a figure needs a \"type\": {TYPES}"),
                node.at,
            ))
        };
    };
    match ty.as_str() {
        "graph" => fig::graph(node, cx).map(Figure::Graph),
        "gantt" => gantt::parse(node, cx).map(Figure::Gantt),
        "line" if field(node, "series").is_some_and(|s| s.as_arr().is_some()) => {
            fig::line(node, cx).map(Figure::Chart)
        }
        "histogram" if has("buckets") => fig::histogram(node, cx).map(Figure::Chart),
        "stat" => dash::stat(node, cx).map(Figure::Stat),
        "meter" => dash::meter(node, cx).map(Figure::Meter),
        "sparkline" => tile::sparkline(node, cx).map(Figure::Sparkline),
        "dashboard" => dash::dashboard(node, cx).map(Figure::Dashboard),
        other => match chart::Shorthand::from_name(other) {
            Some(sh) => chart::parse(node, sh, cx).map(Figure::Chart),
            None => Err(FigError::at(
                format!("unknown figure type \"{other}\": expected {TYPES}"),
                at,
            )),
        },
    }
}

/// What reading a spec carries along: where it is, how data is read, the warnings so far, and
/// whether it's a dashboard's panel and which data names it may use.
pub(crate) struct Cx<'a> {
    /// The spec's directory.
    pub(crate) dir: &'a Path,
    /// How data is read.
    pub(crate) opt: &'a ReadOptions,
    /// What was odd but not wrong.
    pub(crate) warnings: Vec<FigError>,
    /// The dashboard's data names, in a panel.
    pub(crate) names: Option<Vec<String>>,
    /// Whether the figure being read is a dashboard's panel, which may have a `span`.
    pub(crate) panel: bool,
}

impl<'a> Cx<'a> {
    pub(crate) fn new(dir: &'a Path, opt: &'a ReadOptions) -> Cx<'a> {
        Cx {
            dir,
            opt,
            warnings: Vec::new(),
            names: None,
            panel: false,
        }
    }

    /// Warns of each key of `obj` that isn't `known` but is near a known one.
    pub(crate) fn check(&mut self, obj: &[(Key, Node)], known: &[&str]) {
        for (key, _) in obj {
            if known.contains(&&*key.name) {
                continue;
            }
            if let Some(near) = near(&key.name, known) {
                self.warnings.push(FigError::at(
                    format!("unknown field \"{}\"; did you mean \"{near}\"?", key.name),
                    key.at,
                ));
            }
        }
    }

    /// [`Cx::check`] for a figure's own object, which in a panel may also have `span`.
    pub(crate) fn check_top(&mut self, obj: &[(Key, Node)], known: &[&str]) {
        if self.panel {
            let mut k = known.to_vec();
            k.push("span");
            self.check(obj, &k);
        } else {
            self.check(obj, known);
        }
    }
}

/// The known key `key` is a misspelling of, if any: within two edits, one for a known key of
/// three or four chars, none for a shorter one; the nearest, the first of equals.
fn near<'k>(key: &str, known: &[&'k str]) -> Option<&'k str> {
    let mut best: Option<(usize, &str)> = None;
    for k in known {
        let len = k.chars().count();
        let cap = match len {
            0..=2 => 0,
            3..=4 => 1,
            _ => 2,
        };
        if key.chars().count().abs_diff(len) > cap {
            continue;
        }
        let d = distance(key, k);
        if d <= cap && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

/// The edits from `a` to `b`, in chars: insertions, deletions, substitutions, and swaps of two
/// neighbours (optimal string alignment).
pub(crate) fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let w = b.len() + 1;
    let mut d = vec![0usize; (a.len() + 1) * w];
    for i in 0..=a.len() {
        d[i * w] = i;
    }
    for (j, v) in d.iter_mut().enumerate().take(w) {
        *v = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[(i - 1) * w + j] + 1)
                .min(d[i * w + j - 1] + 1)
                .min(d[(i - 1) * w + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * w + j - 2] + 1);
            }
            d[i * w + j] = v;
        }
    }
    d[a.len() * w + b.len()]
}

/// A JSON value's kind with its article, for a message: "a string", "an object", "null".
pub(crate) fn a(n: &Node) -> &'static str {
    match n.kind {
        Kind::Null => "null",
        Kind::Bool(_) => "a boolean",
        Kind::Num(_) => "a number",
        Kind::Str(_) => "a string",
        Kind::Arr(_) => "an array",
        Kind::Obj(_) => "an object",
    }
}

/// The value under `key`, unless it's absent or `null`.
pub(crate) fn field<'a>(n: &'a Node, key: &str) -> Option<&'a Node> {
    n.get(key).filter(|v| !v.is_null())
}

/// An object's keys and values, or an error naming `what` it should have been.
pub(crate) fn obj<'a>(n: &'a Node, what: &str) -> Result<&'a [(Key, Node)]> {
    n.as_obj()
        .ok_or_else(|| FigError::at(format!("{what} must be an object, not {}", a(n)), n.at))
}

/// The array under a key, or an error.
pub(crate) fn arr<'a>(n: &'a Node, key: &str) -> Result<&'a [Node]> {
    n.as_arr().ok_or_else(|| wrong(n, key, "an array"))
}

/// "`key` must be `want`, not …", at the value.
pub(crate) fn wrong(n: &Node, key: &str, want: &str) -> FigError {
    FigError::at(format!("\"{key}\" must be {want}, not {}", a(n)), n.at)
}

/// The string a value is, or an error.
pub(crate) fn string(n: &Node, key: &str) -> Result<String> {
    n.as_str()
        .map(String::from)
        .ok_or_else(|| wrong(n, key, "a string"))
}

/// The string under `key` in `o`, None if it's absent or `null`.
pub(crate) fn opt_string(o: &Node, key: &str) -> Result<Option<String>> {
    field(o, key).map(|v| string(v, key)).transpose()
}

/// The boolean a value is, or an error.
pub(crate) fn boolean(n: &Node, key: &str) -> Result<bool> {
    n.as_bool().ok_or_else(|| wrong(n, key, "true or false"))
}

/// The boolean under `key` in `o`, None if it's absent or `null`.
pub(crate) fn opt_bool(o: &Node, key: &str) -> Result<Option<bool>> {
    field(o, key).map(|v| boolean(v, key)).transpose()
}

/// The number a value is, or an error.
pub(crate) fn number(n: &Node, key: &str) -> Result<f64> {
    n.as_f64().ok_or_else(|| wrong(n, key, "a number"))
}

/// A whole number of at least `min`, or an error.
pub(crate) fn whole(n: &Node, key: &str, min: u64) -> Result<u64> {
    match n.as_f64() {
        Some(v) if v.fract() == 0.0 && v >= min as f64 && v <= u32::MAX as f64 => Ok(v as u64),
        _ => Err(FigError::at(
            format!("\"{key}\" must be a whole number of at least {min}"),
            n.at,
        )),
    }
}

/// A field's name or a list of them.
pub(crate) fn strings(n: &Node, key: &str) -> Result<Vec<String>> {
    match &n.kind {
        Kind::Str(s) => Ok(vec![s.to_string()]),
        Kind::Arr(a) => a.iter().map(|v| string(v, key)).collect(),
        _ => Err(wrong(n, key, "a field's name or a list of them")),
    }
}

/// A node's id: a string, or a number as JSON writes it (CSV ids).
pub(crate) fn id_of(n: &Node, key: &str) -> Result<String> {
    match &n.kind {
        Kind::Str(s) => Ok(s.to_string()),
        Kind::Num(_) => Ok(n.to_text()),
        _ => Err(wrong(n, key, "a string")),
    }
}

impl Mark {
    /// A mark of this kind with its defaults: straight lines without points, bars and areas
    /// stacked from zero, upright.
    pub fn new(kind: MarkKind) -> Mark {
        Mark {
            kind,
            curve: Curve::Linear,
            points: false,
            stack: MarkKind::default_stack(kind),
            horizontal: false,
        }
    }
}

impl MarkKind {
    /// Every kind, in the order the docs list them.
    pub const ALL: [MarkKind; 8] = [
        MarkKind::Line,
        MarkKind::Area,
        MarkKind::Bar,
        MarkKind::Point,
        MarkKind::Rect,
        MarkKind::Rule,
        MarkKind::Band,
        MarkKind::Text,
    ];

    /// Its name in a spec.
    pub fn name(self) -> &'static str {
        match self {
            MarkKind::Line => "line",
            MarkKind::Area => "area",
            MarkKind::Bar => "bar",
            MarkKind::Point => "point",
            MarkKind::Rect => "rect",
            MarkKind::Rule => "rule",
            MarkKind::Band => "band",
            MarkKind::Text => "text",
        }
    }

    /// How a mark of this kind stacks unless the spec says: bars and areas from zero.
    pub fn default_stack(self) -> Stack {
        match self {
            MarkKind::Bar | MarkKind::Area => Stack::Zero,
            _ => Stack::None,
        }
    }
}

impl ViewSettings {
    /// A graph's defaults: no grid and no axes (fig's plane for a graph), the rest as a chart's.
    pub fn graph() -> ViewSettings {
        ViewSettings {
            grid: false,
            axes: false,
            ..ViewSettings::default()
        }
    }
}

impl Figure {
    /// The figure with every place in the text forgotten: channels', nodes' and edges' places,
    /// and the places inside kept objects. Two readings of one figure from different texts
    /// (a spec and [`write`]'s text of it) are equal this way.
    pub fn without_places(&self) -> Figure {
        let mut f = self.clone();
        unplace_figure(&mut f);
        f
    }
}

fn unplace_figure(f: &mut Figure) {
    match f {
        Figure::Chart(c) => {
            unplace_data(&mut c.data);
            for l in &mut c.layers {
                for ch in [
                    &mut l.enc.x,
                    &mut l.enc.y,
                    &mut l.enc.y2,
                    &mut l.enc.color,
                    &mut l.enc.size,
                    &mut l.enc.label,
                ]
                .into_iter()
                .flatten()
                {
                    ch.at = None;
                }
                if let Some(d) = &mut l.data {
                    unplace_data(d);
                }
            }
        }
        Figure::Graph(g) => {
            for n in &mut g.nodes {
                n.at = Pos::default();
                unplace_node(&mut n.raw);
            }
            for e in &mut g.edges {
                e.at = Pos::default();
                unplace_node(&mut e.raw);
            }
        }
        Figure::Gantt(g) => {
            unplace_data(&mut g.data);
            let f = &mut g.fields;
            for ch in [
                &mut f.id,
                &mut f.label,
                &mut f.start,
                &mut f.end,
                &mut f.duration,
                &mut f.after,
                &mut f.group,
                &mut f.parent,
                &mut f.progress,
                &mut f.milestone,
                &mut f.color,
            ]
            .into_iter()
            .flatten()
            {
                ch.at = None;
            }
            for m in &mut g.marks {
                m.pos = Pos::default();
            }
        }
        Figure::Stat(s) => unplace_data(&mut s.data),
        Figure::Meter(m) => unplace_data(&mut m.data),
        Figure::Sparkline(s) => unplace_data(&mut s.data),
        Figure::Dashboard(d) => {
            for (_, data) in &mut d.data {
                unplace_data(data);
            }
            for r in &mut d.rows {
                r.at = Pos::default();
                r.panels.iter_mut().for_each(unplace_panel);
            }
        }
    }
}

fn unplace_panel(p: &mut PanelSpec) {
    p.at = Pos::default();
    match &mut p.panel {
        Panel::Figure(f) => unplace_figure(f),
        Panel::Column(ps) => ps.iter_mut().for_each(unplace_panel),
        Panel::File(_) => {}
    }
}

fn unplace_data(d: &mut DataSpec) {
    if let DataRef::Borrow { at, .. } = &mut d.source {
        *at = Pos::default();
    }
    if let DataRef::Inline(t) = &mut d.source
        && let Some(raw) = &t.raw
    {
        let rows: Vec<Node> = raw
            .iter()
            .map(|n| {
                let mut n = n.clone();
                unplace_node(&mut n);
                n
            })
            .collect();
        Arc::make_mut(t).raw = Some(rows.into());
    }
}

fn unplace_node(n: &mut Node) {
    n.at = Pos::default();
    match &mut n.kind {
        Kind::Arr(a) => a.iter_mut().for_each(unplace_node),
        Kind::Obj(o) => {
            for (k, v) in o {
                k.at = Pos::default();
                unplace_node(v);
            }
        }
        _ => {}
    }
}
