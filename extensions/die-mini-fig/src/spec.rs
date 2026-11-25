//! A `.fig` spec read into mini-fig's own types.
//!
//! serde_json reads the JSON; one pass then makes every figure a dashboard: a lone chart, tile
//! or tree is a dashboard of one panel, its title the figure's. Keys mini-fig doesn't know are
//! ignored. What it can't read is an error of one line saying why: a dashboard's panel that
//! can't be read is a [`Kind::Failed`] panel saying it, and the rest are read.
//!
//! A panel is `"type"`: `line`, `bar`, `scatter`, `heatmap`, `stat` or `graph` (a tree), or
//! `chart` with a `"mark"` of `line`, `bar` or `point`. Data is a name in the dashboard's
//! `data` map, a CSV path relative to the spec, or `{"values": [rows]}`; each becomes a named
//! [`Source`] the panel refers to.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A figure: always a dashboard.
#[derive(Clone, Debug, Default)]
pub struct Fig {
    /// Its title, and the line under it.
    pub title: String,
    pub subtitle: String,
    /// Every table its panels and layers read, by name.
    pub sources: BTreeMap<String, Source>,
    /// The dashboard's time: its field, and how much of the newest of it to show.
    pub time: Option<TimeRange>,
    /// Its rows of panels, top to bottom.
    pub rows: Vec<Row>,
}

/// Where a table's rows come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A CSV file, its path resolved against the spec's folder.
    File(PathBuf),
    /// Rows written in the spec, as JSON objects.
    Rows(Vec<Value>),
}

/// `"time": {"field": "time", "range": "7d"}`.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeRange {
    /// The time column panels share.
    pub field: String,
    /// The range in seconds, ending at the newest sample; None for all of it.
    pub secs: Option<f64>,
}

/// A row of a dashboard.
#[derive(Clone, Debug)]
pub struct Row {
    /// Its height relative to a row of 1 (a row of tiles is shorter, whatever it says).
    pub height: f32,
    /// Its cells, left to right.
    pub cells: Vec<Cell>,
}

/// A place in a row: one panel, or a column of panels stacked.
#[derive(Clone, Debug)]
pub struct Cell {
    /// Its width relative to its neighbours'.
    pub span: f32,
    /// Its panels, top to bottom.
    pub panels: Vec<Panel>,
}

/// What a panel draws.
#[derive(Clone, Debug, Default)]
pub enum Kind {
    #[default]
    Line,
    Bar,
    Scatter,
    Heatmap,
    Stat(Stat),
    Tree(Graph),
    /// A panel that can't be read, and why.
    Failed(String),
}

/// A panel: a chart, a tile or a tree.
#[derive(Clone, Debug, Default)]
pub struct Panel {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    /// The name of its table in [`Fig::sources`].
    pub data: Option<String>,
    pub x: Option<Channel>,
    pub y: Option<Channel>,
    /// A series per value of this field, or a heatmap's colour.
    pub color: Option<Channel>,
    pub mark: Mark,
    /// The card's fields, in order; empty for x, y and series.
    pub tooltip: Vec<String>,
    /// A colour and dash by series name.
    pub series: BTreeMap<String, Style>,
    pub transform: Vec<Transform>,
    pub layers: Vec<Layer>,
}

/// A field a channel reads, and how.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Channel {
    pub field: String,
    /// The axis or legend title; `Some("")` for none.
    pub title: Option<String>,
    /// `"type": "category"`: one band per value, even of numbers or dates.
    pub category: bool,
    /// `"scale": "log"`.
    pub log: bool,
    /// `"scale": "diverging"`: a colour scale centred on zero.
    pub diverging: bool,
    /// `"zero"`: whether a linear axis takes in zero (None: the kind decides).
    pub zero: Option<bool>,
}

/// A chart's mark options.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mark {
    /// A dot at each point of a line.
    pub points: bool,
    /// `"curve": "step"`.
    pub step: bool,
    /// Bars along x, categories down y.
    pub horizontal: bool,
    /// Bars of one category stacked, as die-fig has them unless `"stack": false`.
    pub stack: bool,
}

/// A series' look: a palette colour by name ("red", "dim") and a dash.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Style {
    pub color: Option<String>,
    pub dash: bool,
}

/// A transform, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Transform {
    /// Keep the rows whose field passes.
    Filter { field: String, keep: Keep },
    /// Order the rows by a field.
    Sort { field: String, descending: bool },
}

/// What a filter keeps.
#[derive(Clone, Debug, PartialEq)]
pub enum Keep {
    In(Vec<String>),
    NotIn(Vec<String>),
    /// At least the first, at most the second.
    Range(Option<f64>, Option<f64>),
}

/// A layer drawn over a chart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    /// A rule across the plot (else a line through its own rows).
    pub rule: bool,
    /// A rule's place on x or y.
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub label: String,
    /// A palette colour by name.
    pub color: Option<String>,
    pub dash: bool,
    /// A line's table, in [`Fig::sources`], and the field that splits it into series.
    pub data: Option<String>,
    pub by: Option<String>,
}

/// A stat tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stat {
    pub label: String,
    /// The field whose newest value it shows (`"value": {"last": field}`).
    pub field: String,
    pub unit: String,
    /// A d3-like format: ".2f", ",.0f".
    pub format: Option<String>,
    /// The x of its sparkline.
    pub trend: Option<String>,
    /// `"delta": "previous"`: the change from the row before.
    pub delta: bool,
    pub good: Good,
}

/// Which way a stat's change is good.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Good {
    #[default]
    Up,
    Down,
    Neither,
}

/// A graph, laid out as a tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub directed: bool,
    /// The tree's root: `layout.root`, else the first node nothing points to.
    pub root: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// A graph's node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub group: String,
    pub size: f64,
    /// Its other fields as the card shows them, in the spec's order.
    pub fields: Vec<(String, String)>,
}

/// A graph's edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: String,
}

impl Fig {
    /// The data files it reads, for watching.
    pub fn files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.sources.values().filter_map(|s| match s {
            Source::File(p) => Some(p.clone()),
            Source::Rows(_) => None,
        })
    }

    /// Every panel, row by row, cell by cell, top to bottom: the order keys move in.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        let cells = self.rows.iter().flat_map(|r| &r.cells);
        cells.flat_map(|c| &c.panels)
    }
}

/// The spec in a file.
pub fn read(path: &Path) -> Result<Fig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text, path.parent().unwrap_or(Path::new(".")))
}

/// A spec's text, its relative paths against `dir`.
pub fn parse(text: &str, dir: &Path) -> Result<Fig, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let mut b = Builder {
        dir,
        fig: Fig::default(),
    };
    if v.get("type").and_then(Value::as_str) == Some("dashboard") {
        let d: RawDash = from(v, "the dashboard")?;
        (b.fig.title, b.fig.subtitle) = (d.title, d.subtitle);
        for (name, path) in d.data {
            b.fig.sources.insert(name, Source::File(dir.join(path)));
        }
        if let Some(t) = d.time {
            let secs = match t.range.as_deref() {
                None | Some("all") => None,
                Some(r) => Some(duration(r).ok_or(format!("\"range\": \"{r}\" isn't a time"))?),
            };
            b.fig.time = Some(TimeRange {
                field: t.field,
                secs,
            });
        }
        for row in d.rows {
            let row = b.row(row)?;
            b.fig.rows.push(row);
        }
    } else {
        let mut p: RawPanel = from(v, "the figure")?;
        b.fig.title = std::mem::take(&mut p.title);
        b.fig.subtitle = std::mem::take(&mut p.subtitle);
        let panels = vec![b.read(p)?];
        let cells = vec![Cell { span: 1.0, panels }];
        b.fig.rows.push(Row { height: 1.0, cells });
    }
    Ok(b.fig)
}

/// "7d", "6h", "15m", "30s", "2w" in seconds.
pub fn duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let unit = match s.chars().last()? {
        's' => 1.0,
        'm' => 60.0,
        'h' => 3600.0,
        'd' => 86_400.0,
        'w' => 604_800.0,
        _ => return None,
    };
    let n: f64 = s[..s.len() - 1].trim().parse().ok()?;
    (n.is_finite() && n > 0.0).then_some(n * unit)
}

/// What serde reads of a dashboard.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDash {
    title: String,
    subtitle: String,
    data: BTreeMap<String, String>,
    time: Option<RawTime>,
    rows: Vec<Value>,
}

#[derive(Deserialize)]
struct RawTime {
    field: String,
    range: Option<String>,
}

/// A row written as an object.
#[derive(Deserialize)]
struct RawRow {
    height: Option<f32>,
    panels: Vec<RawPanel>,
}

/// What serde reads of a panel. Keys that take more than one shape stay JSON until
/// [`Builder::panel`] reads them.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPanel {
    #[serde(rename = "type")]
    kind: String,
    mark: Value,
    title: String,
    subtitle: String,
    data: Value,
    x: Value,
    y: Value,
    color: Value,
    tooltip: Value,
    series: BTreeMap<String, Style>,
    transform: Vec<Map<String, Value>>,
    layers: Vec<RawLayer>,
    span: Option<f32>,
    column: Option<Vec<RawPanel>>,
    label: String,
    value: Value,
    unit: String,
    format: Option<String>,
    trend: Option<String>,
    delta: Value,
    good: String,
    directed: bool,
    layout: Value,
    nodes: Vec<Map<String, Value>>,
    edges: Vec<Map<String, Value>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawLayer {
    mark: String,
    x: Value,
    y: Value,
    color: Value,
    data: Value,
    fig: RuleLook,
}

/// A layer's `fig`: a label as well as a look.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RuleLook {
    label: String,
    color: Option<String>,
    dash: bool,
}

/// What a parse is building.
struct Builder<'a> {
    dir: &'a Path,
    fig: Fig,
}

impl Builder<'_> {
    /// A row: an array of panels, or `{height, panels}`.
    fn row(&mut self, v: Value) -> Result<Row, String> {
        let raw = match v {
            Value::Array(_) => RawRow {
                height: None,
                panels: from(v, "a row")?,
            },
            _ => from(v, "a row")?,
        };
        let mut cells = Vec::new();
        for p in raw.panels {
            let span = p.span.unwrap_or(1.0).clamp(0.1, 100.0);
            let panels = match p.column {
                Some(column) => column.into_iter().map(|p| self.panel(p)).collect(),
                None => vec![self.panel(p)],
            };
            cells.push(Cell { span, panels });
        }
        let height = raw.height.unwrap_or(1.0).clamp(0.1, 100.0);
        Ok(Row { height, cells })
    }

    /// A dashboard's panel, or one that says why it can't be read.
    fn panel(&mut self, p: RawPanel) -> Panel {
        self.read(p).unwrap_or_else(|why| Panel {
            kind: Kind::Failed(why),
            ..Panel::default()
        })
    }

    /// A panel, or why it can't be read.
    fn read(&mut self, p: RawPanel) -> Result<Panel, String> {
        let name = (if p.title.is_empty() {
            &p.label
        } else {
            &p.title
        })
        .clone();
        let at = |e: String| match name.as_str() {
            "" => e,
            name => format!("panel \"{name}\": {e}"),
        };
        let mark = p.mark.as_str().unwrap_or("");
        let what = if p.kind == "chart" || p.kind.is_empty() {
            mark
        } else {
            &p.kind
        };
        let kind = match what {
            "line" | "area" => Kind::Line,
            "bar" => Kind::Bar,
            "scatter" | "point" | "circle" => Kind::Scatter,
            "heatmap" | "rect" => Kind::Heatmap,
            "stat" => Kind::Stat(stat(&p).map_err(at)?),
            "graph" => Kind::Tree(graph(&p).map_err(at)?),
            "" => return Err(at("no \"type\"".into())),
            other => return Err(at(format!("mini-fig doesn't draw \"{other}\""))),
        };
        let data = self.source(&p.data).map_err(at)?;
        let flag = |k: &str| p.mark.get(k).and_then(Value::as_bool).unwrap_or(false);
        let curve = p.mark.get("curve").and_then(Value::as_str).unwrap_or("");
        let mark = Mark {
            points: flag("points"),
            step: curve.starts_with("step"),
            horizontal: flag("horizontal"),
            stack: p.mark.get("stack").and_then(Value::as_bool) != Some(false),
        };
        let transform: Result<Vec<_>, _> = p.transform.iter().map(transform).collect();
        let mut layers = Vec::new();
        for l in p.layers {
            if let Some(l) = self.layer(l).map_err(at)? {
                layers.push(l);
            }
        }
        Ok(Panel {
            kind,
            title: p.title,
            subtitle: p.subtitle,
            data,
            x: channel(&p.x).map_err(|e| at(format!("\"x\" {e}")))?,
            y: channel(&p.y).map_err(|e| at(format!("\"y\" {e}")))?,
            color: channel(&p.color).map_err(|e| at(format!("\"color\" {e}")))?,
            mark,
            tooltip: strings(&p.tooltip),
            series: p.series,
            transform: transform.map_err(at)?,
            layers,
        })
    }

    /// A layer; None for a mark mini-fig doesn't draw over a chart.
    fn layer(&mut self, l: RawLayer) -> Result<Option<Layer>, String> {
        let rule = match l.mark.as_str() {
            "rule" => true,
            "line" => false,
            _ => return Ok(None),
        };
        Ok(Some(Layer {
            rule,
            x: l.x.as_f64(),
            y: l.y.as_f64(),
            label: l.fig.label,
            color: l.fig.color,
            dash: l.fig.dash,
            data: self.source(&l.data)?,
            by: l.color.as_str().map(str::to_string),
        }))
    }

    /// The name of a panel's or layer's table, added to the sources if it's new.
    fn source(&mut self, v: &Value) -> Result<Option<String>, String> {
        let sources = &mut self.fig.sources;
        match v {
            Value::Null => Ok(None),
            Value::String(s) if sources.contains_key(s) => Ok(Some(s.clone())),
            Value::String(s) => {
                sources.insert(s.clone(), Source::File(self.dir.join(s)));
                Ok(Some(s.clone()))
            }
            Value::Object(o) => {
                let Some(Value::Array(rows)) = o.get("values") else {
                    return Err("\"data\" must be a name, a path or {\"values\": [rows]}".into());
                };
                let name = format!("#{}", sources.len());
                sources.insert(name.clone(), Source::Rows(rows.clone()));
                Ok(Some(name))
            }
            _ => Err("\"data\" must be a name, a path or {\"values\": [rows]}".into()),
        }
    }
}

/// A channel: a field's name, `{"field": …}`, or null for none.
fn channel(v: &Value) -> Result<Option<Channel>, String> {
    let o = match v {
        Value::Null => return Ok(None),
        Value::String(f) => {
            let field = f.clone();
            return Ok(Some(Channel {
                field,
                ..Channel::default()
            }));
        }
        Value::Object(o) => o,
        _ => return Err("must be a field's name or {\"field\": …}".into()),
    };
    let text = |k: &str| o.get(k).and_then(Value::as_str);
    let Some(field) = text("field") else {
        return Err("needs a \"field\"".into());
    };
    let kind = text("type").unwrap_or("");
    Ok(Some(Channel {
        field: field.to_string(),
        title: text("title").map(str::to_string),
        category: matches!(kind, "category" | "nominal" | "ordinal"),
        log: text("scale") == Some("log"),
        diverging: text("scale") == Some("diverging"),
        zero: o.get("zero").and_then(Value::as_bool),
    }))
}

/// A transform: `{"filter": field, "in" | "not_in" | "min" | "max": …}` or `{"sort": field,
/// "descending": bool}`.
fn transform(o: &Map<String, Value>) -> Result<Transform, String> {
    let text = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(field) = text("sort") {
        let descending = o.get("descending").and_then(Value::as_bool) == Some(true);
        return Ok(Transform::Sort { field, descending });
    }
    let Some(field) = text("filter") else {
        return Err("mini-fig reads only \"filter\" and \"sort\" transforms".into());
    };
    let keep = if let Some(v) = o.get("in") {
        Keep::In(strings(v))
    } else if let Some(v) = o.get("not_in") {
        Keep::NotIn(strings(v))
    } else {
        let end = |k: &str| o.get(k).and_then(Value::as_f64);
        if end("min").is_none() && end("max").is_none() {
            return Err(format!(
                "the filter on \"{field}\" needs \"in\", \"min\" or \"max\""
            ));
        }
        Keep::Range(end("min"), end("max"))
    };
    Ok(Transform::Filter { field, keep })
}

/// A stat tile's own keys.
fn stat(p: &RawPanel) -> Result<Stat, String> {
    let Some(field) = p.value.get("last").and_then(Value::as_str) else {
        return Err("mini-fig reads only \"value\": {\"last\": field}".into());
    };
    let good = match p.good.as_str() {
        "down" | "lower" => Good::Down,
        "neither" | "none" => Good::Neither,
        _ => Good::Up,
    };
    Ok(Stat {
        label: (if p.label.is_empty() {
            &p.title
        } else {
            &p.label
        })
        .clone(),
        field: field.to_string(),
        unit: p.unit.clone(),
        format: p.format.clone(),
        trend: p.trend.clone(),
        delta: p.delta.as_str() == Some("previous"),
        good,
    })
}

/// A graph's nodes and edges, laid out as a tree.
fn graph(p: &RawPanel) -> Result<Graph, String> {
    let layout = p.layout.get("type").and_then(Value::as_str);
    if layout.is_some_and(|t| t != "tree") {
        return Err("mini-fig lays out only trees (\"layout\": {\"type\": \"tree\"})".into());
    }
    let mut nodes = Vec::new();
    for n in &p.nodes {
        let Some(id) = n.get("id").map(text) else {
            return Err("a node has no \"id\"".into());
        };
        let label = n.get("label").map_or(id.clone(), text);
        let fields = n.iter().filter(|(k, _)| !NODE_KEYS.contains(&k.as_str()));
        nodes.push(Node {
            label,
            group: n.get("group").map(text).unwrap_or_default(),
            size: n.get("size").and_then(Value::as_f64).unwrap_or(1.0),
            fields: fields.map(|(k, v)| (k.clone(), text(v))).collect(),
            id,
        });
    }
    let mut edges = Vec::new();
    for e in &p.edges {
        let (Some(from), Some(to)) = (e.get("from"), e.get("to")) else {
            return Err("an edge needs \"from\" and \"to\"".into());
        };
        let label = e.get("label").map(text).unwrap_or_default();
        let (from, to) = (text(from), text(to));
        edges.push(Edge { from, to, label });
    }
    let root = match p.layout.get("root") {
        Some(r) => text(r),
        None => {
            let pointed = |id: &str| edges.iter().any(|e| e.to == id);
            let top = nodes.iter().find(|n| !pointed(&n.id)).or(nodes.first());
            top.map(|n| n.id.clone()).unwrap_or_default()
        }
    };
    if !nodes.iter().any(|n| n.id == root) {
        return Err(format!("the root \"{root}\" isn't a node"));
    }
    Ok(Graph {
        directed: p.directed,
        root,
        nodes,
        edges,
    })
}

/// A node's keys that aren't shown as its fields.
const NODE_KEYS: [&str; 4] = ["id", "label", "group", "size"];

/// A JSON value as text: a string as it is, anything else as JSON writes it.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// An array's strings and numbers as text; anything else gives none.
fn strings(v: &Value) -> Vec<String> {
    let items = v.as_array().map(Vec::as_slice).unwrap_or_default();
    let items = items.iter().filter(|v| v.is_string() || v.is_number());
    items.map(text).collect()
}

/// `v` read as `T`, an error saying where.
fn from<T: serde::de::DeserializeOwned>(v: Value, what: &str) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_chart_is_a_dashboard_of_one_panel() {
        let spec = r#"{"type": "chart", "mark": "bar", "title": "T", "data": "d.csv",
            "x": {"field": "a", "type": "category", "title": ""}, "y": "b",
            "transform": [{"filter": "g", "in": ["x", 2]}, {"sort": "b", "descending": true}]}"#;
        let fig = parse(spec, Path::new("/tmp")).unwrap();
        assert_eq!(fig.title, "T");
        let p = fig.panels().next().unwrap();
        assert!(matches!(p.kind, Kind::Bar) && p.title.is_empty());
        assert_eq!(fig.sources["d.csv"], Source::File("/tmp/d.csv".into()));
        let x = p.x.as_ref().unwrap();
        assert!(x.category && x.title.as_deref() == Some(""));
        assert_eq!(p.y.as_ref().unwrap().field, "b");
        let keep = Keep::In(vec!["x".into(), "2".into()]);
        assert_eq!(
            p.transform[0],
            Transform::Filter {
                field: "g".into(),
                keep
            }
        );
    }

    #[test]
    fn what_it_cannot_read_says_why_in_a_line() {
        let e = parse(r#"{"type": "gantt"}"#, Path::new(".")).unwrap_err();
        assert!(e.contains("doesn't draw \"gantt\""), "{e}");
        let e = parse("{", Path::new(".")).unwrap_err();
        assert!(e.starts_with("not JSON") && !e.contains('\n'), "{e}");
        assert_eq!(duration("7d"), Some(604_800.0));
        assert_eq!(duration("x"), None);
    }
}
