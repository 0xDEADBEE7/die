//! What a buffer's text says to draw: a [`Figure`], from a fig file, a spec, or bare data with a
//! guessed figure.
//!
//! - [`fig`]: fig's own `graph`, `line` and `histogram` shapes, found by their fields (`nodes`;
//!   `series` with `points`; `buckets`) and desugared into these types;
//! - [`chart`]: the grammar: marks, encodings, layers, transforms, inheritance;
//! - [`graph`]: the graph spec beyond fig's;
//! - [`dash`]: dashboards, stats and meters;
//! - [`guess`]: a figure from a bare table, by the design's inference rules;
//! - [`write`]: a figure back to JSON text, for `:fig-spec`.
//!
//! `"type"` is one of `chart`, `line`, `area`, `bar`, `scatter`, `heatmap`, `histogram`,
//! `graph`, `stat`, `meter` and `dashboard`. Unknown fields are tolerated and kept in `raw`; a
//! misspelt known one is a warning.

pub mod chart;
pub mod dash;
pub mod fig;
pub mod graph;
pub mod guess;
pub mod write;

pub use guess::guess;
pub use write::write;

use crate::data::transform::{AggOp, Transform};
use crate::data::{ColumnKind, ReadOptions, SeriesKey, Table};
use crate::error::{FigError, Pos, Result};
use crate::json::Node;
use crate::style::FigStyle;
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
}

/// A dashboard panel's content.
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    /// A figure in the spec.
    Figure(Box<Figure>),
    /// A spec in another file, relative to this one's directory.
    File(PathBuf),
}

/// A dashboard panel and how many columns of its row it spans.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelSpec {
    /// What it shows.
    pub panel: Panel,
    /// The columns it spans.
    pub span: u16,
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
    /// Its rows of panels.
    pub rows: Vec<Vec<PanelSpec>>,
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
    /// A stat tile.
    Stat(StatSpec),
    /// A meter tile.
    Meter(MeterSpec),
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
/// `dir` resolves relative paths.
pub fn parse(_text: &str, _name: &str, _dir: &Path, _opt: &ReadOptions) -> Result<Parsed> {
    todo!("P1-spec: spec::parse")
}
