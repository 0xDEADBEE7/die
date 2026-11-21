//! fig's own shapes, desugared: a `graph` (`nodes` and `edges`), a `line` (`series` with
//! `points`) and a `histogram` (`buckets`), found by their fields and turned into the spec's
//! types with fig's validation messages and its `fig` blocks kept (briefs/fig-port.md §1).
//!
//! A line is the long table `(series, x, y)`: one line layer, x and y titled by `x_label` and
//! `y_label`, coloured by `series`, whose column is a category with one entry a series in the
//! file's order, so that series of one name stay apart. Each series' points are in x order: a
//! series out of order is sorted, with a warning (fig's `line series x values must be ordered`
//! made lenient, as ref joined points in any order). A series of one point is a dot.
//!
//! A histogram is the table `(bucket, series, value)`, a row for every bucket and series (a
//! missing value is 0), drawn as bars stacked from zero on a band x, a band a bucket.
//!
//! A graph checks fig's rules (and ref's empty id) and points at the place: `a graph needs at
//! least one node`, `node IDs cannot be empty`, `node ids must be unique`, `unknown edge
//! source: …`, `unknown edge target: …`. An id may be a number, as a CSV's are.

use super::chart::{self, FIG};
use super::{
    Channel, Chart, Cx, DataRef, DataSpec, Encoding, GraphEdge, GraphNode, GraphSpec, Layer, Mark,
    MarkKind, ReadHints, ScaleKind, ViewSettings, arr, field, graph, id_of, obj, opt_string,
    string, wrong,
};
use crate::data::{Column, SeriesKey, Table, Values};
use crate::error::{FigError, Result};
use crate::json::Node;
use crate::style::FigStyle;
use std::collections::BTreeSet;
use std::sync::Arc;

/// A fig line's keys.
pub const LINE: &[&str] = &[
    "type", "title", "subtitle", "x_label", "y_label", "series", "fig", "view",
];
/// A fig histogram's keys.
pub const HISTOGRAM: &[&str] = &[
    "type", "title", "subtitle", "x_label", "y_label", "series", "buckets", "fig", "view",
];
/// A graph's keys.
pub const GRAPH: &[&str] = &[
    "type", "title", "subtitle", "nodes", "edges", "layout", "directed", "fig", "view",
];
/// A node's keys: fig's, then the graph spec's.
pub const NODE: &[&str] = &["id", "label", "fig", "metadata", "group", "size"];
/// An edge's keys: fig's (with their aliases), then the graph spec's.
pub const EDGE: &[&str] = &[
    "from", "to", "source", "target", "fig", "label", "weight", "directed",
];

/// Whether a `series` is fig's line series: a list of objects with `points`.
pub(crate) fn is_line_series(n: Option<&Node>) -> bool {
    n.and_then(Node::as_arr)
        .is_some_and(|a| a.iter().any(|s| s.get("points").is_some()))
}

/// fig's line chart.
pub(crate) fn line(node: &Node, cx: &mut Cx<'_>) -> Result<Chart> {
    let o = obj(node, "a line chart")?;
    cx.check_top(o, LINE);
    let x_label = opt_string(node, "x_label")?;
    let y_label = opt_string(node, "y_label")?;
    let Some(series) = field(node, "series") else {
        return Err(FigError::at(
            "a line chart needs at least one series",
            node.at,
        ));
    };
    let series = arr(series, "series")?;
    if series.is_empty() {
        let at = field(node, "series").map_or(node.at, |s| s.at);
        return Err(FigError::at("a line chart needs at least one series", at));
    }
    let mut dict: Vec<Box<str>> = Vec::new();
    let (mut codes, mut xs, mut ys) = (Vec::new(), Vec::new(), Vec::new());
    let mut series_fig = Vec::new();
    for (i, s) in series.iter().enumerate() {
        let so = obj(s, "a line series")?;
        cx.check(so, &["label", "points", "fig"]);
        let Some(label) = field(s, "label") else {
            return Err(FigError::at("a line series needs a \"label\"", s.at));
        };
        let label = string(label, "label")?;
        let Some(points) = field(s, "points") else {
            return Err(FigError::at(
                format!("line series {label:?} must contain at least one point"),
                s.at,
            ));
        };
        let pts = arr(points, "points")?;
        if pts.is_empty() {
            return Err(FigError::at(
                format!("line series {label:?} must contain at least one point"),
                points.at,
            ));
        }
        let mut row: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
        let mut unordered = None;
        for p in pts {
            if p.as_obj().is_none() {
                return Err(wrong(p, "points", "points like {\"x\": 1, \"y\": 2}"));
            }
            let coord = |k: &str| -> Result<f64> {
                match p.get(k) {
                    None => Err(FigError::at(format!("a point needs \"{k}\""), p.at)),
                    Some(v) => v.as_f64().ok_or_else(|| wrong(v, k, "a number")),
                }
            };
            let (x, y) = (coord("x")?, coord("y")?);
            if unordered.is_none() && row.last().is_some_and(|&(px, _)| x < px) {
                unordered = Some(p.at);
            }
            row.push((x, y));
        }
        if let Some(at) = unordered {
            cx.warnings.push(FigError::at(
                format!(
                    "line series {label:?}: line series x values must be ordered; drawn in x order"
                ),
                at,
            ));
            row.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let nth = dict.iter().filter(|d| ***d == *label).count() as u32;
        if let Some(f) = field(s, "fig") {
            let st = chart::fig_block(f, cx);
            if st != FigStyle::default() {
                series_fig.push((SeriesKey::new(&label, nth), st));
            }
        }
        dict.push(label.into());
        codes.extend(std::iter::repeat_n(i as u32, row.len()));
        for (x, y) in row {
            xs.push(x);
            ys.push(y);
        }
    }
    let table = Table::new(vec![
        Column {
            name: "series".into(),
            values: Values::Category {
                codes: codes.into(),
                dict: dict.into(),
            },
        },
        Column::number("x", xs),
        Column::number("y", ys),
    ])?;
    let layer = Layer {
        mark: Mark::new(MarkKind::Line),
        enc: Encoding {
            x: Some(titled("x", x_label, None)),
            y: Some(titled("y", y_label, None)),
            color: Some(titled("series", None, None)),
            ..Encoding::default()
        },
        data: None,
        transform: Vec::new(),
        fig: top_style(node, cx),
    };
    Ok(Chart {
        title: opt_string(node, "title")?,
        subtitle: opt_string(node, "subtitle")?,
        data: inline(table),
        transform: Vec::new(),
        layers: vec![layer],
        series_fig,
        view: chart::view_settings(node, ViewSettings::default(), cx)?,
    })
}

/// fig's histogram (ref's): buckets of values by series, as stacked bars.
pub(crate) fn histogram(node: &Node, cx: &mut Cx<'_>) -> Result<Chart> {
    let o = obj(node, "a histogram")?;
    cx.check_top(o, HISTOGRAM);
    let x_label = opt_string(node, "x_label")?;
    let y_label = opt_string(node, "y_label")?;
    let series_node = field(node, "series");
    let series = match series_node {
        Some(s) => arr(s, "series")?,
        None => &[],
    };
    if series.is_empty() {
        return Err(FigError::at(
            "a histogram must contain at least one series",
            series_node.map_or(node.at, |s| s.at),
        ));
    }
    let mut names: Vec<Box<str>> = Vec::new();
    let mut series_fig = Vec::new();
    for s in series {
        let so = obj(s, "a histogram series")?;
        cx.check(so, &["label", "fig"]);
        let label = match field(s, "label") {
            Some(l) => string(l, "label")?,
            None => String::new(),
        };
        if label.is_empty() || names.iter().any(|n| **n == *label) {
            let at = field(s, "label").map_or(s.at, |l| l.at);
            return Err(FigError::at(
                "histogram series labels must be non-empty and unique",
                at,
            ));
        }
        if let Some(f) = field(s, "fig") {
            let st = chart::fig_block(f, cx);
            if st != FigStyle::default() {
                series_fig.push((SeriesKey::new(&label, 0), st));
            }
        }
        names.push(label.into());
    }
    let buckets_node = field(node, "buckets");
    let buckets = match buckets_node {
        Some(b) => arr(b, "buckets")?,
        None => &[],
    };
    if buckets.is_empty() {
        return Err(FigError::at(
            "a histogram must contain at least one bucket",
            buckets_node.map_or(node.at, |b| b.at),
        ));
    }
    let mut labels: Vec<Box<str>> = Vec::new();
    let (mut bucket_codes, mut series_codes, mut values) = (Vec::new(), Vec::new(), Vec::new());
    for (bi, b) in buckets.iter().enumerate() {
        let bo = obj(b, "a histogram bucket")?;
        cx.check(bo, &["label", "values"]);
        let label = match field(b, "label") {
            Some(l) => string(l, "label")?,
            None => String::new(),
        };
        if label.is_empty() {
            let at = field(b, "label").map_or(b.at, |l| l.at);
            return Err(FigError::at("histogram bucket labels cannot be empty", at));
        }
        let mut row = vec![0.0; names.len()];
        if let Some(vs) = field(b, "values") {
            for (k, v) in obj(vs, "\"values\"")? {
                let Some(si) = names.iter().position(|n| **n == *k.name) else {
                    return Err(FigError::at(
                        format!(
                            "histogram bucket {label:?} references unknown series {:?}",
                            &*k.name
                        ),
                        k.at,
                    ));
                };
                let Some(x) = v.as_f64() else {
                    return Err(wrong(v, &k.name, "a number"));
                };
                if !(x.is_finite() && x >= 0.0) {
                    return Err(FigError::at(
                        format!(
                            "histogram value {x:?} in bucket {label:?} must be finite and non-negative"
                        ),
                        v.at,
                    ));
                }
                row[si] = x;
            }
        }
        labels.push(label.into());
        for (si, x) in row.into_iter().enumerate() {
            bucket_codes.push(bi as u32);
            series_codes.push(si as u32);
            values.push(x);
        }
    }
    let table = Table::new(vec![
        Column {
            name: "bucket".into(),
            values: Values::Category {
                codes: bucket_codes.into(),
                dict: labels.into(),
            },
        },
        Column {
            name: "series".into(),
            values: Values::Category {
                codes: series_codes.into(),
                dict: names.into(),
            },
        },
        Column::number("value", values),
    ])?;
    let layer = Layer {
        mark: Mark::new(MarkKind::Bar),
        enc: Encoding {
            x: Some(titled("bucket", x_label, Some(ScaleKind::Band))),
            y: Some(titled("value", y_label, None)),
            color: Some(titled("series", None, None)),
            ..Encoding::default()
        },
        data: None,
        transform: Vec::new(),
        fig: top_style(node, cx),
    };
    Ok(Chart {
        title: opt_string(node, "title")?,
        subtitle: opt_string(node, "subtitle")?,
        data: inline(table),
        transform: Vec::new(),
        layers: vec![layer],
        series_fig,
        view: chart::view_settings(node, ViewSettings::default(), cx)?,
    })
}

/// A channel on a field of a desugared table, with a title and a scale if fig's file gives
/// them. It has no place: the table is made here.
fn titled(f: &str, title: Option<String>, scale: Option<ScaleKind>) -> Channel {
    Channel {
        field: Some(f.into()),
        title,
        scale,
        ..Channel::default()
    }
}

fn inline(table: Table) -> DataSpec {
    DataSpec {
        source: DataRef::Inline(Arc::new(table)),
        format: None,
        read: ReadHints::default(),
    }
}

/// The top's `fig` block, `menu` aside.
fn top_style(node: &Node, cx: &mut Cx<'_>) -> FigStyle {
    match field(node, "fig") {
        Some(f) => {
            if let Some(o) = f.as_obj() {
                cx.check(o, FIG);
            }
            FigStyle::from_node(f)
        }
        None => FigStyle::default(),
    }
}

/// fig's graph, with the graph spec's extras ([`graph`]).
pub(crate) fn graph(node: &Node, cx: &mut Cx<'_>) -> Result<GraphSpec> {
    let o = obj(node, "a graph")?;
    cx.check_top(o, GRAPH);
    let nodes_node = field(node, "nodes");
    let list = match nodes_node {
        Some(n) => arr(n, "nodes")?,
        None => &[],
    };
    if list.is_empty() {
        return Err(FigError::at(
            "a graph needs at least one node",
            nodes_node.map_or(node.at, |n| n.at),
        ));
    }
    let mut nodes = Vec::with_capacity(list.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for n in list {
        let no = obj(n, "a node")?;
        cx.check(no, NODE);
        let Some(idn) = field(n, "id") else {
            return Err(FigError::at("a node needs an \"id\"", n.at));
        };
        let id = id_of(idn, "id")?;
        if id.trim().is_empty() {
            return Err(FigError::at("node IDs cannot be empty", idn.at));
        }
        if !seen.insert(id.clone()) {
            return Err(FigError::at("node ids must be unique", idn.at));
        }
        let fig = field(n, "fig")
            .map(|f| chart::fig_block(f, cx))
            .unwrap_or_default();
        let (group, size) = graph::node_extras(n)?;
        nodes.push(GraphNode {
            id,
            label: opt_string(n, "label")?,
            group,
            size,
            fig,
            raw: n.clone(),
            at: n.at,
        });
    }
    let directed = match field(node, "directed") {
        Some(d) => super::boolean(d, "directed")?,
        None => false,
    };
    let mut edges = Vec::new();
    if let Some(es) = field(node, "edges") {
        for e in arr(es, "edges")? {
            edges.push(edge(e, directed, &seen, cx)?);
        }
    }
    Ok(GraphSpec {
        title: opt_string(node, "title")?,
        subtitle: opt_string(node, "subtitle")?,
        layout: graph::layout(field(node, "layout"), &seen)?,
        nodes,
        edges,
        view: chart::view_settings(node, ViewSettings::graph(), cx)?,
    })
}

/// One edge: `from` (or `source`) and `to` (or `target`), both known nodes.
fn edge(e: &Node, directed: bool, ids: &BTreeSet<String>, cx: &mut Cx<'_>) -> Result<GraphEdge> {
    let eo = obj(e, "an edge")?;
    cx.check(eo, EDGE);
    let end = |name: &str, alias: &str| -> Result<(String, Node)> {
        match (field(e, name), field(e, alias)) {
            (Some(_), Some(_)) => {
                let at = eo
                    .iter()
                    .filter(|(k, _)| &*k.name == name || &*k.name == alias)
                    .nth(1)
                    .map_or(e.at, |(k, _)| k.at);
                Err(FigError::at(
                    format!("an edge's \"{name}\" is given twice, as \"{name}\" and \"{alias}\""),
                    at,
                ))
            }
            (Some(v), None) => Ok((id_of(v, name)?, v.clone())),
            (None, Some(v)) => Ok((id_of(v, alias)?, v.clone())),
            (None, None) => Err(FigError::at(
                format!("an edge needs \"{name}\" (or \"{alias}\")"),
                e.at,
            )),
        }
    };
    let (from, from_node) = end("from", "source")?;
    let (to, to_node) = end("to", "target")?;
    if !ids.contains(&from) {
        return Err(FigError::at(
            format!("unknown edge source: {from}"),
            from_node.at,
        ));
    }
    if !ids.contains(&to) {
        return Err(FigError::at(
            format!("unknown edge target: {to}"),
            to_node.at,
        ));
    }
    let fig = field(e, "fig")
        .map(|f| chart::fig_block(f, cx))
        .unwrap_or_default();
    let (label, weight, directed) = graph::edge_extras(e, directed)?;
    Ok(GraphEdge {
        from,
        to,
        label,
        weight,
        directed,
        fig,
        raw: e.clone(),
        at: e.at,
    })
}
