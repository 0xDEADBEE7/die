//! The spec: fig's line, histogram and graph files desugared with fig's messages, the grammar
//! (shorthand types, marks, channels, data, transforms, views and series blocks) with top-level
//! encodings inherited by layers, dashboards and their tiles, errors and warnings at their
//! places, the guess at a bare table, and `write`'s text read back as the same figure.
//!
//! Every spec and table is made here; bare data goes through the data module's readers.

use die_test_cap as _;

use die_fig::data::transform::{AggOp, FilterOp, Transform};
use die_fig::data::{Column, ColumnKind, ReadOptions, SeriesKey, Table, Values};
use die_fig::spec::{
    Aggregate, Channel, ChannelValue, Chart, Curve, DataFormat, DataRef, Delta, Figure, GoodWhen,
    Labels, Layout, LegendPos, Mark, MarkKind, Panel, Parsed, ReadHints, ScaleKind, Stack,
    ViewSettings, guess, parse, write,
};
use die_fig::style::{ColorSpec, FigStyle, Status};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const DIR: &str = "/specs";

fn opt() -> ReadOptions {
    ReadOptions::new(jiff::tz::TimeZone::UTC)
}

fn read(text: &str) -> Parsed {
    match parse(text, "figure.fig.json", Path::new(DIR), &opt()) {
        Ok(p) => p,
        Err(e) => panic!("{e} in\n{text}"),
    }
}

fn chart(text: &str) -> Chart {
    match read(text).figure {
        Figure::Chart(c) => c,
        f => panic!("not a chart: {f:?}"),
    }
}

/// The error a spec gives: its message and its line and column.
fn fails(text: &str) -> (String, (u32, u32)) {
    let e = parse(text, "figure.fig.json", Path::new(DIR), &opt()).expect_err(text);
    let at = e.at.unwrap_or_else(|| panic!("no place for {:?}", e.msg));
    (e.msg, (at.line, at.col))
}

/// Where the `n`th (from 0) `needle` starts in `text`: its line and column from 1, in chars.
fn nth(text: &str, needle: &str, n: usize) -> (u32, u32) {
    let byte = text
        .match_indices(needle)
        .nth(n)
        .unwrap_or_else(|| panic!("no {needle:?}"))
        .0;
    let before = &text[..byte];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().unwrap_or("").chars().count() as u32 + 1;
    (line, col)
}

fn place(text: &str, needle: &str) -> (u32, u32) {
    nth(text, needle, 0)
}

fn inline(c: &Chart) -> &Table {
    match &c.data.source {
        DataRef::Inline(t) => t,
        d => panic!("not inline: {d:?}"),
    }
}

fn codes(t: &Table, name: &str) -> (Vec<u32>, Vec<String>) {
    match &t.column(name).expect(name).values {
        Values::Category { codes, dict } => {
            (codes.to_vec(), dict.iter().map(|s| s.to_string()).collect())
        }
        v => panic!("{name} isn't a category: {v:?}"),
    }
}

fn nums(t: &Table, name: &str) -> Vec<f64> {
    let c = t.column(name).expect(name);
    (0..c.len()).map(|i| c.f64_at(i)).collect()
}

/// A channel on a field, as a spec at `at` gives it.
fn on(field: &str) -> Channel {
    Channel {
        field: Some(field.into()),
        ..Channel::default()
    }
}

fn unplaced(c: &Option<Channel>) -> Option<Channel> {
    c.clone().map(|c| Channel { at: None, ..c })
}

// fig's line.

#[test]
fn a_fig_line_is_a_long_table_coloured_by_series_with_its_blocks_by_key() {
    let text = r#"{
  "type": "line",
  "x_label": "Event", "y_label": "Tokens",
  "series": [
    {"label": "a", "points": [{"x": 0, "y": 1}, {"x": 1, "y": 3}], "fig": {"color": "red", "size": 2}},
    {"label": "b", "points": [{"x": 0, "y": 2}]},
    {"label": "a", "points": [{"x": 0, "y": 5}, {"x": 2, "y": 4}], "fig": {"dash": true, "label": "a again"}},
    {"label": "c", "points": [{"x": 0, "y": 0}], "fig": {}}
  ]
}"#;
    let p = read(text);
    assert!(!p.inferred);
    assert_eq!(p.warnings, []);
    let Figure::Chart(c) = p.figure else { panic!() };
    let t = inline(&c);
    let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
    assert_eq!(names, ["series", "x", "y"]);
    // One entry a series, in the file's order: the two "a"s stay two.
    assert_eq!(
        codes(t, "series"),
        (
            vec![0, 0, 1, 2, 2, 3],
            vec!["a".into(), "b".into(), "a".into(), "c".into()]
        )
    );
    assert_eq!(nums(t, "x"), [0.0, 1.0, 0.0, 0.0, 2.0, 0.0]);
    assert_eq!(nums(t, "y"), [1.0, 3.0, 2.0, 5.0, 4.0, 0.0]);
    assert_eq!((t.raw.clone(), t.styles.clone()), (None, None));
    assert_eq!(c.layers.len(), 1);
    let l = &c.layers[0];
    assert_eq!(l.mark, Mark::new(MarkKind::Line));
    assert_eq!(
        l.mark,
        Mark {
            kind: MarkKind::Line,
            curve: Curve::Linear,
            points: false,
            stack: Stack::None,
            horizontal: false
        }
    );
    assert_eq!(
        l.enc.x,
        Some(Channel {
            title: Some("Event".into()),
            ..on("x")
        })
    );
    assert_eq!(
        l.enc.y,
        Some(Channel {
            title: Some("Tokens".into()),
            ..on("y")
        })
    );
    assert_eq!(l.enc.color, Some(on("series")));
    // A block that says nothing is no block.
    assert_eq!(
        c.series_fig,
        [
            (
                SeriesKey::new("a", 0),
                FigStyle {
                    color: Some(ColorSpec::Slot(7)),
                    size: Some(2),
                    ..FigStyle::default()
                }
            ),
            (
                SeriesKey::new("a", 1),
                FigStyle {
                    dash: true,
                    label: Some("a again".into()),
                    ..FigStyle::default()
                }
            ),
        ]
    );
    assert_eq!(c.view, ViewSettings::default());
    assert_eq!((c.title.clone(), c.transform.clone()), (None, vec![]));
}

#[test]
fn a_fig_line_keeps_figs_rules_leniently() {
    let none = r#"{"type": "line", "series": []}"#;
    assert_eq!(
        fails(none),
        (
            "a line chart needs at least one series".into(),
            place(none, "[]")
        )
    );
    let empty = r#"{"type": "line", "series": [{"label": "a", "points": []}]}"#;
    assert_eq!(
        fails(empty),
        (
            "line series \"a\" must contain at least one point".into(),
            place(empty, "[]")
        )
    );
    let bad = r#"{"type": "line", "series": [{"label": "a", "points": [{"x": 1, "y": "2"}]}]}"#;
    assert_eq!(
        fails(bad),
        (
            "\"y\" must be a number, not a string".into(),
            place(bad, "\"2\"")
        )
    );
    let no_x = r#"{"type": "line", "series": [{"label": "a", "points": [{"y": 2}]}]}"#;
    assert_eq!(
        fails(no_x),
        ("a point needs \"x\"".into(), place(no_x, "{\"y\""))
    );
    let pair = r#"{"type": "line", "series": [{"label": "a", "points": [[1, 2]]}]}"#;
    assert_eq!(
        fails(pair).1,
        place(pair, "[1, 2]"),
        "fig's points are objects"
    );
    let no_label = r#"{"type": "line", "series": [{"points": [{"x": 1, "y": 2}]}]}"#;
    assert_eq!(
        fails(no_label),
        (
            "a line series needs a \"label\"".into(),
            place(no_label, "{\"points")
        )
    );

    // One point is a dot, and x out of order is drawn in order, with a warning there.
    let text = r#"{"type": "line", "series": [
  {"label": "dot", "points": [{"x": 5, "y": 1}]},
  {"label": "back", "points": [{"x": 2, "y": 20}, {"x": 1, "y": 10}, {"x": 3, "y": 30}]}
]}"#;
    let p = read(text);
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(
        p.warnings[0].msg,
        "line series \"back\": line series x values must be ordered; drawn in x order"
    );
    let at = p.warnings[0].at.unwrap();
    assert_eq!((at.line, at.col), place(text, "{\"x\": 1"));
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(nums(inline(&c), "x"), [5.0, 1.0, 2.0, 3.0]);
    assert_eq!(nums(inline(&c), "y"), [1.0, 10.0, 20.0, 30.0]);

    // Equal x is a vertical step, which is in order; without "type", the fields say it's a line.
    let steps =
        chart(r#"{"series": [{"label": "s", "points": [{"x": 1, "y": 1}, {"x": 1, "y": 2}]}]}"#);
    assert_eq!(nums(inline(&steps), "y"), [1.0, 2.0]);
}

// fig's histogram.

#[test]
fn a_fig_histogram_is_stacked_bars_on_a_band_of_buckets() {
    let c = chart(
        r#"{
  "type": "histogram",
  "x_label": "Latency", "y_label": "Count",
  "series": [{ "label": "success" }, { "label": "failure", "fig": {"color": "critical"} }],
  "buckets": [
    { "label": "0–10", "values": { "success": 42, "failure": 3 } },
    { "label": "10–25", "values": { "success": 67 } },
    { "label": "0–10" }
  ]
}"#,
    );
    let t = inline(&c);
    let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
    assert_eq!(names, ["bucket", "series", "value"]);
    assert_eq!(
        codes(t, "bucket"),
        (
            vec![0, 0, 1, 1, 2, 2],
            vec!["0–10".into(), "10–25".into(), "0–10".into()]
        )
    );
    assert_eq!(
        codes(t, "series"),
        (
            vec![0, 1, 0, 1, 0, 1],
            vec!["success".into(), "failure".into()]
        )
    );
    // A missing value counts as 0.
    assert_eq!(nums(t, "value"), [42.0, 3.0, 67.0, 0.0, 0.0, 0.0]);
    let l = &c.layers[0];
    assert_eq!(l.mark, Mark::new(MarkKind::Bar));
    assert_eq!(l.mark.stack, Stack::Zero);
    assert_eq!(
        l.enc.x,
        Some(Channel {
            title: Some("Latency".into()),
            scale: Some(ScaleKind::Band),
            ..on("bucket")
        })
    );
    assert_eq!(
        l.enc.y,
        Some(Channel {
            title: Some("Count".into()),
            ..on("value")
        })
    );
    assert_eq!(l.enc.color, Some(on("series")));
    assert_eq!(
        c.series_fig,
        [(
            SeriesKey::new("failure", 0),
            FigStyle {
                color: Some(ColorSpec::Status(Status::Critical)),
                ..FigStyle::default()
            }
        )]
    );
}

#[test]
fn a_fig_histogram_says_what_refs_did_and_where() {
    let cases: [(&str, &str, &str); 7] = [
        (
            r#"{"type": "histogram", "series": [], "buckets": [{"label": "b"}]}"#,
            "a histogram must contain at least one series",
            "[]",
        ),
        (
            r#"{"type": "histogram", "series": [{"label": "s"}], "buckets": []}"#,
            "a histogram must contain at least one bucket",
            "[]",
        ),
        (
            r#"{"type": "histogram", "series": [{"label": "s"}, {"label": "s"}], "buckets": [{"label": "b"}]}"#,
            "histogram series labels must be non-empty and unique",
            "\"s\"}]",
        ),
        (
            r#"{"type": "histogram", "series": [{"label": ""}], "buckets": [{"label": "b"}]}"#,
            "histogram series labels must be non-empty and unique",
            "\"\"",
        ),
        (
            r#"{"type": "histogram", "series": [{"label": "s"}], "buckets": [{"label": ""}]}"#,
            "histogram bucket labels cannot be empty",
            "\"\"",
        ),
        (
            r#"{"type": "histogram", "series": [{"label": "s"}], "buckets": [{"label": "b", "values": {"t": 1}}]}"#,
            "histogram bucket \"b\" references unknown series \"t\"",
            "\"t\"",
        ),
        (
            r#"{"type": "histogram", "series": [{"label": "s"}], "buckets": [{"label": "b", "values": {"s": -1.5}}]}"#,
            "histogram value -1.5 in bucket \"b\" must be finite and non-negative",
            "-1.5",
        ),
    ];
    for (text, msg, at) in cases {
        let want = if msg.contains("non-empty and unique") && at == "\"s\"}]" {
            nth(text, "\"s\"", 1)
        } else {
            place(text, at)
        };
        assert_eq!(fails(text), (msg.to_string(), want), "{text}");
    }
}

// fig's graph.

#[test]
fn a_fig_graph_reads_figs_fields_its_aliases_and_keeps_the_rest() {
    let text = r#"{"type": "graph",
 "nodes": [
  {"id": "a", "label": "Alpha", "metadata": {"k": 1}, "state": "open", "kind": "plan"},
  {"id": "b", "fig": {"color": "green", "show-label": true}},
  {"id": 3}
 ],
 "edges": [{"from": "a", "to": "b"}, {"source": "b", "target": "3", "fig": {"color": "dim"}}, {"from": "a", "to": "a"}]}"#;
    let p = read(text);
    // `kind` is data, not a misspelt `id`.
    assert_eq!(p.warnings, []);
    let Figure::Graph(g) = p.figure else { panic!() };
    let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "3"]);
    assert_eq!(g.nodes[0].label.as_deref(), Some("Alpha"));
    assert_eq!(g.nodes[1].label, None);
    assert_eq!(
        g.nodes[1].fig,
        FigStyle {
            color: Some(ColorSpec::Slot(5)),
            show_label: Some(true),
            ..FigStyle::default()
        }
    );
    // The node's whole object is kept, its unknown fields in their order.
    let keys: Vec<&str> = g.nodes[0]
        .raw
        .as_obj()
        .unwrap()
        .iter()
        .map(|(k, _)| &*k.name)
        .collect();
    assert_eq!(keys, ["id", "label", "metadata", "state", "kind"]);
    assert_eq!(
        g.nodes[0].raw.get("metadata").map(|m| m.to_text()),
        Some("{\"k\":1}".into())
    );
    assert_eq!(
        (g.nodes[2].at.line, g.nodes[2].at.col),
        place(text, "{\"id\": 3}")
    );
    let ends: Vec<(&str, &str)> = g
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    assert_eq!(ends, [("a", "b"), ("b", "3"), ("a", "a")]);
    assert_eq!(g.edges[1].fig.color, Some(ColorSpec::Dim));
    assert!(g.edges.iter().all(|e| !e.directed && e.weight.is_none()));
    // A graph has no grid and no axes unless it says.
    assert_eq!(g.view, ViewSettings::graph());
    assert!(!g.view.grid && !g.view.axes && g.view.render_options);
    assert_eq!(g.layout, Layout::Auto);

    // Without "type", nodes make a graph; fig.menu sets the plane.
    let Figure::Graph(g) = read(
        r#"{"nodes": [{"id": "x"}], "fig": {"menu": {"plane": {"grid_lines": true}, "render_options": false}}}"#,
    )
    .figure
    else {
        panic!()
    };
    assert!(g.view.grid && !g.view.axes && !g.view.render_options);
}

#[test]
fn a_fig_graph_says_what_fig_does_where_it_went_wrong() {
    let no_nodes = r#"{"type": "graph", "nodes": []}"#;
    assert_eq!(
        fails(no_nodes),
        (
            "a graph needs at least one node".into(),
            place(no_nodes, "[]")
        )
    );
    let missing = r#"{"type": "graph"}"#;
    assert_eq!(
        fails(missing),
        ("a graph needs at least one node".into(), (1, 1))
    );
    let blank = r#"{"type": "graph", "nodes": [{"id": " "}]}"#;
    assert_eq!(
        fails(blank),
        ("node IDs cannot be empty".into(), place(blank, "\" \""))
    );
    let twice = r#"{"type": "graph", "nodes": [{"id": "a"}, {"id": "b"}, {"id": "a"}]}"#;
    assert_eq!(
        fails(twice),
        (
            "node ids must be unique: a is given twice".into(),
            nth(twice, "\"a\"", 1)
        )
    );
    let source =
        r#"{"type": "graph", "nodes": [{"id": "a"}], "edges": [{"from": "z", "to": "a"}]}"#;
    assert_eq!(
        fails(source),
        ("unknown edge source: z".into(), place(source, "\"z\""))
    );
    let target =
        r#"{"type": "graph", "nodes": [{"id": "a"}], "edges": [{"source": "a", "target": "q"}]}"#;
    assert_eq!(
        fails(target),
        ("unknown edge target: q".into(), place(target, "\"q\""))
    );
    let both = r#"{"type": "graph", "nodes": [{"id": "a"}], "edges": [{"from": "a", "source": "a", "to": "a"}]}"#;
    assert_eq!(
        fails(both),
        (
            "an edge's \"from\" is given twice, as \"from\" and \"source\"".into(),
            place(both, "\"source\"")
        )
    );
    let no_to = r#"{"type": "graph", "nodes": [{"id": "a"}], "edges": [{"from": "a"}]}"#;
    assert_eq!(
        fails(no_to),
        (
            "an edge needs \"to\" (or \"target\")".into(),
            place(no_to, "{\"from\"")
        )
    );
    let bad_id = r#"{"type": "graph", "nodes": [{"id": true}]}"#;
    assert_eq!(
        fails(bad_id),
        (
            "\"id\" must be a string, not a boolean".into(),
            place(bad_id, "true")
        )
    );
    let no_id = r#"{"type": "graph", "nodes": [{"label": "x"}]}"#;
    assert_eq!(
        fails(no_id),
        ("a node needs an \"id\"".into(), place(no_id, "{\"label\""))
    );
}

#[test]
fn a_graph_spec_has_groups_sizes_weights_directions_and_a_layout() {
    let Figure::Graph(g) = read(
        r#"{"type": "graph", "title": "Org", "directed": true, "layout": {"type": "tree", "root": "ceo"},
 "nodes": [{"id": "ceo", "group": "exec", "size": 2.5}, {"id": "cto", "group": 7}],
 "edges": [{"from": "ceo", "to": "cto", "label": "reports", "weight": 0.5}, {"from": "cto", "to": "ceo", "directed": false}]}"#,
    )
    .figure
    else {
        panic!()
    };
    assert_eq!(g.title.as_deref(), Some("Org"));
    assert_eq!(
        g.layout,
        Layout::Tree {
            root: Some("ceo".into())
        }
    );
    assert_eq!(
        (g.nodes[0].group.as_deref(), g.nodes[0].size),
        (Some("exec"), Some(2.5))
    );
    assert_eq!(g.nodes[1].group.as_deref(), Some("7"));
    assert_eq!(
        (
            g.edges[0].label.as_deref(),
            g.edges[0].weight,
            g.edges[0].directed
        ),
        (Some("reports"), Some(0.5), true)
    );
    assert!(!g.edges[1].directed);
    let layered = |l: &str| match read(&format!(
        r#"{{"type": "graph", "layout": {l}, "nodes": [{{"id": "a"}}]}}"#
    ))
    .figure
    {
        Figure::Graph(g) => g.layout,
        _ => panic!(),
    };
    assert_eq!(layered("\"layered\""), Layout::Layered);
    assert_eq!(layered("\"tree\""), Layout::Tree { root: None });
    assert_eq!(layered("\"force\""), Layout::Force);
    let root = r#"{"type": "graph", "layout": {"type": "tree", "root": "nobody"}, "nodes": [{"id": "a"}]}"#;
    assert_eq!(
        fails(root),
        (
            "unknown layout root: nobody".into(),
            place(root, "\"nobody\"")
        )
    );
    let bad = r#"{"type": "graph", "layout": "radial", "nodes": [{"id": "a"}]}"#;
    assert_eq!(fails(bad).1, place(bad, "\"radial\""));
    let size = r#"{"type": "graph", "nodes": [{"id": "a", "size": 0}]}"#;
    assert_eq!(
        fails(size),
        (
            "\"size\" must be a number above 0".into(),
            place(size, "0}")
        )
    );
}

// The grammar.

const REQUESTS: &str = r#"{
  "type": "chart",
  "title": "Requests per minute",
  "data": "requests.csv",
  "transform": [{ "fold": ["api", "worker", "batch"],
                  "as": ["service", "rpm"] }],
  "x": { "field": "time", "type": "time" },
  "y": { "field": "rpm", "title": "Requests / min" },
  "color": "service",
  "mark": "line",
  "layers": [{ "mark": "rule", "y": { "value": 1000 },
               "fig": { "color": "warning", "label": "capacity" } }]
}"#;

#[test]
fn the_designs_spec_is_two_layers_the_rule_inheriting_x_and_colour() {
    let p = read(REQUESTS);
    assert_eq!(p.warnings, []);
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(c.title.as_deref(), Some("Requests per minute"));
    assert_eq!(
        c.data.source,
        DataRef::File(PathBuf::from("/specs/requests.csv"))
    );
    assert_eq!(
        (c.data.format, c.data.read.clone()),
        (None, ReadHints::default())
    );
    assert_eq!(
        c.transform,
        [Transform::Fold {
            fields: vec!["api".into(), "worker".into(), "batch".into()],
            key: "service".into(),
            value: "rpm".into()
        }]
    );
    assert_eq!(c.layers.len(), 2);
    let (line, rule) = (&c.layers[0], &c.layers[1]);
    assert_eq!(line.mark, Mark::new(MarkKind::Line));
    assert_eq!(
        unplaced(&line.enc.x),
        Some(Channel {
            kind: Some(ColumnKind::Time),
            ..on("time")
        })
    );
    assert_eq!(
        line.enc.x.as_ref().unwrap().at.map(|a| (a.line, a.col)),
        Some(place(REQUESTS, "{ \"field\": \"time\""))
    );
    assert_eq!(
        unplaced(&line.enc.y),
        Some(Channel {
            title: Some("Requests / min".into()),
            ..on("rpm")
        })
    );
    assert_eq!(unplaced(&line.enc.color), Some(on("service")));
    assert_eq!(line.fig, FigStyle::default());
    assert_eq!(rule.mark, Mark::new(MarkKind::Rule));
    // The rule takes x and colour from the top, and its own y.
    assert_eq!(rule.enc.x, line.enc.x);
    assert_eq!(rule.enc.color, line.enc.color);
    assert_eq!(
        unplaced(&rule.enc.y),
        Some(Channel {
            value: Some(ChannelValue::Num(1000.0)),
            ..Channel::default()
        })
    );
    assert_eq!(
        rule.fig,
        FigStyle {
            color: Some(ColorSpec::Status(Status::Warning)),
            label: Some("capacity".into()),
            ..FigStyle::default()
        }
    );
    assert_eq!(c.series_fig, []);
    assert_eq!(c.view, ViewSettings::default());
}

#[test]
fn a_layer_takes_the_tops_encodings_but_those_it_gives_or_drops() {
    let c = chart(
        r#"{"type": "chart", "data": "d.csv",
  "x": "t", "y": "v", "color": "host", "tooltip": ["host", "v"],
  "layers": [
    {"mark": "line"},
    {"mark": "point", "y": {"field": "w", "zero": false}, "color": null, "tooltip": "w"},
    {"mark": "text", "label": "note", "detail": ["note"], "x": 3}
  ]}"#,
    );
    assert_eq!(c.layers.len(), 3);
    let [a, b, t] = &c.layers[..] else { panic!() };
    assert_eq!(
        (
            unplaced(&a.enc.x),
            unplaced(&a.enc.y),
            unplaced(&a.enc.color)
        ),
        (Some(on("t")), Some(on("v")), Some(on("host")))
    );
    assert_eq!(a.enc.tooltip, ["host", "v"]);
    // A channel given is the layer's whole channel; null drops one.
    assert_eq!(
        unplaced(&b.enc.y),
        Some(Channel {
            zero: Some(false),
            ..on("w")
        })
    );
    assert_eq!(b.enc.color, None);
    assert_eq!(b.enc.tooltip, ["w"]);
    assert_eq!(unplaced(&t.enc.label), Some(on("note")));
    assert_eq!(t.enc.detail, ["note"]);
    assert_eq!(
        unplaced(&t.enc.x),
        Some(Channel {
            value: Some(ChannelValue::Num(3.0)),
            ..Channel::default()
        })
    );
    assert_eq!(t.enc.tooltip, ["host", "v"]);

    let no_mark = r#"{"type": "chart", "data": "d.csv", "layers": [{"x": "t"}]}"#;
    assert_eq!(
        fails(no_mark),
        ("a layer needs a \"mark\"".into(), place(no_mark, "{\"x\""))
    );
    let nothing = r#"{"type": "chart", "data": "d.csv", "x": "t"}"#;
    assert_eq!(
        fails(nothing),
        ("a chart needs a \"mark\" or \"layers\"".into(), (1, 1))
    );
    let no_data = r#"{"type": "line", "x": "t"}"#;
    assert_eq!(fails(no_data), ("a figure needs \"data\"".into(), (1, 1)));
}

#[test]
fn a_shorthand_type_is_a_chart_with_that_mark() {
    let kind = |t: &str| {
        chart(&format!(
            r#"{{"type": "{t}", "data": "d.csv", "x": "a", "y": "b"}}"#
        ))
        .layers[0]
            .mark
            .clone()
    };
    assert_eq!(kind("line"), Mark::new(MarkKind::Line));
    assert_eq!(kind("area").stack, Stack::Zero);
    assert_eq!(kind("bar"), Mark::new(MarkKind::Bar));
    assert_eq!(kind("bar").stack, Stack::Zero);
    assert_eq!(kind("scatter").kind, MarkKind::Point);
    assert_eq!(kind("heatmap").kind, MarkKind::Rect);
    assert_eq!(kind("line").stack, Stack::None);

    // A mark object refines the shorthand's, and may not change it.
    let c = chart(
        r#"{"type": "bar", "data": "d.csv", "x": "a", "y": "b", "mark": {"horizontal": true, "stack": "normalize"}}"#,
    );
    assert_eq!(
        c.layers[0].mark,
        Mark {
            horizontal: true,
            stack: Stack::Normalize,
            ..Mark::new(MarkKind::Bar)
        }
    );
    let c = chart(
        r#"{"type": "chart", "data": "d.csv", "mark": {"type": "line", "curve": "step", "points": true}}"#,
    );
    assert_eq!(
        c.layers[0].mark,
        Mark {
            curve: Curve::Step,
            points: true,
            ..Mark::new(MarkKind::Line)
        }
    );
    let c = chart(r#"{"type": "area", "data": "d.csv", "mark": {"stack": false}}"#);
    assert_eq!(c.layers[0].mark.stack, Stack::None);
    let clash = r#"{"type": "bar", "data": "d.csv", "mark": "line"}"#;
    assert_eq!(
        fails(clash),
        (
            "the mark is \"line\" but the chart's type draws \"bar\"".into(),
            place(clash, "\"line\"")
        )
    );
    let pie = r#"{"type": "chart", "data": "d.csv", "mark": "pie"}"#;
    assert_eq!(
        fails(pie),
        (
            "unknown mark \"pie\": expected line, area, bar, point, rect, rule, band or text"
                .into(),
            place(pie, "\"pie\"")
        )
    );
}

#[test]
fn a_histogram_without_buckets_counts_x_in_bins() {
    let c = chart(
        r#"{"type": "histogram", "data": "lat.csv", "x": {"field": "latency", "title": "Latency (ms)"}, "bins": 30}"#,
    );
    let l = &c.layers[0];
    assert_eq!(l.mark, Mark::new(MarkKind::Bar));
    assert_eq!(
        l.transform,
        [
            Transform::Bin {
                field: "latency".into(),
                step: None,
                max_bins: 30,
                start: "latency_start".into(),
                end: "latency_end".into()
            },
            Transform::Aggregate {
                op: AggOp::Count,
                field: None,
                by: vec!["latency_start".into(), "latency_end".into()],
                out: "count".into()
            }
        ]
    );
    assert_eq!(
        unplaced(&l.enc.x),
        Some(Channel {
            title: Some("Latency (ms)".into()),
            scale: Some(ScaleKind::Linear),
            ..on("latency_start")
        })
    );
    assert_eq!(unplaced(&l.enc.y), Some(on("count")));
    assert_eq!(c.transform, []);

    // A y is summed, a colour splits the bins, a step sets their width.
    let c = chart(
        r#"{"type": "histogram", "data": "d.csv", "x": "size", "y": "bytes", "color": "kind", "step": 10}"#,
    );
    let l = &c.layers[0];
    assert_eq!(
        l.transform[1],
        Transform::Aggregate {
            op: AggOp::Sum,
            field: Some("bytes".into()),
            by: vec!["size_start".into(), "size_end".into(), "kind".into()],
            out: "bytes".into()
        }
    );
    assert!(
        matches!(l.transform[0], Transform::Bin { step: Some(s), max_bins: 20, .. } if s == 10.0)
    );
    let no_x = r#"{"type": "histogram", "data": "d.csv"}"#;
    assert_eq!(
        fails(no_x),
        (
            "a histogram needs \"x\": the field it counts".into(),
            (1, 1)
        )
    );
}

#[test]
fn channels_read_every_key_and_say_what_is_wrong() {
    let c = chart(
        r#"{"type": "line", "data": "d.csv",
  "x": {"field": "t", "type": "time", "unit": "ms", "scale": "time", "format": "%H:%M"},
  "y": {"field": "v", "scale": "log", "domain": [1, 1000], "zero": false, "title": "V"},
  "color": {"value": "everything"}}"#,
    );
    let e = &c.layers[0].enc;
    assert_eq!(
        unplaced(&e.x),
        Some(Channel {
            kind: Some(ColumnKind::Time),
            unit: Some("ms".into()),
            scale: Some(ScaleKind::Time),
            format: Some("%H:%M".into()),
            ..on("t")
        })
    );
    assert_eq!(
        unplaced(&e.y),
        Some(Channel {
            scale: Some(ScaleKind::Log),
            domain: Some((1.0, 1000.0)),
            zero: Some(false),
            title: Some("V".into()),
            ..on("v")
        })
    );
    assert_eq!(
        unplaced(&e.color).and_then(|c| c.value),
        Some(ChannelValue::Str("everything".into()))
    );
    let cases: [(&str, &str, &str); 8] = [
        (
            r#"{"type": "line", "data": "d", "x": {"field": "a", "value": 1}}"#,
            "\"x\" has a \"field\" and a \"value\"; give one",
            "{\"field",
        ),
        (
            r#"{"type": "line", "data": "d", "x": {"title": "T"}}"#,
            "\"x\" needs a \"field\" or a \"value\"",
            "{\"title",
        ),
        (
            r#"{"type": "line", "data": "d", "x": {"field": "a", "type": "date"}}"#,
            "\"type\" must be \"number\", \"time\", \"category\" or \"text\"",
            "\"date\"",
        ),
        (
            r#"{"type": "line", "data": "d", "x": {"field": "a", "scale": "sqrt"}}"#,
            "\"scale\" must be \"auto\", \"linear\", \"log\", \"time\" or \"band\"",
            "\"sqrt\"",
        ),
        (
            r#"{"type": "line", "data": "d", "x": {"field": "a", "unit": "us"}}"#,
            "\"unit\" must be \"s\" or \"ms\"",
            "\"us\"",
        ),
        (
            r#"{"type": "line", "data": "d", "x": {"field": "a", "domain": [1]}}"#,
            "\"domain\" must be two numbers or two times",
            "[1]",
        ),
        (
            r#"{"type": "line", "data": "d", "x": {"field": "a", "domain": [2, 2]}}"#,
            "\"domain\" needs two different ends",
            "[2, 2]",
        ),
        (
            r#"{"type": "line", "data": "d", "x": true}"#,
            "\"x\" must be a field's name, a number or an object, not a boolean",
            "true",
        ),
    ];
    for (text, msg, at) in cases {
        assert_eq!(fails(text), (msg.to_string(), place(text, at)), "{text}");
    }
}

#[test]
fn data_is_a_file_the_buffer_a_command_a_stream_or_inline() {
    let data = |d: &str| {
        chart(&format!(
            r#"{{"type": "line", "data": {d}, "x": "a", "y": "b"}}"#
        ))
        .data
    };
    assert_eq!(
        data(r#""sub/r.csv""#).source,
        DataRef::File(PathBuf::from("/specs/sub/r.csv"))
    );
    assert_eq!(
        data(r#""/abs/r.csv""#).source,
        DataRef::File(PathBuf::from("/abs/r.csv"))
    );
    assert_eq!(data(r#""-""#).source, DataRef::Source);
    let d = data(
        r#"{"file": "eu.csv", "format": "csv", "delimiter": ";", "header": false, "thousands": ".", "time_format": "%d/%m/%Y"}"#,
    );
    assert_eq!(d.source, DataRef::File(PathBuf::from("/specs/eu.csv")));
    assert_eq!(d.format, Some(DataFormat::Csv));
    assert_eq!(
        d.read,
        ReadHints {
            delimiter: Some(b';'),
            header: Some(false),
            thousands: Some('.'),
            time_format: Some("%d/%m/%Y".into())
        }
    );
    let tab = data(r#"{"file": "t.txt", "delimiter": "\t", "format": "tsv"}"#);
    assert_eq!(
        (tab.read.delimiter, tab.format),
        (Some(b'\t'), Some(DataFormat::Tsv))
    );
    assert_eq!(
        data(r#"{"run": "./errors.sh", "every": "30s"}"#).source,
        DataRef::Run {
            cmd: "./errors.sh".into(),
            every: Some(Duration::from_secs(30))
        }
    );
    assert_eq!(
        data(r#"{"run": "date", "every": "1h30m"}"#).source,
        DataRef::Run {
            cmd: "date".into(),
            every: Some(Duration::from_secs(5400))
        }
    );
    assert_eq!(
        data(r#"{"run": "date", "every": 0.5}"#).source,
        DataRef::Run {
            cmd: "date".into(),
            every: Some(Duration::from_millis(500))
        }
    );
    assert_eq!(
        data(r#"{"run": "ls"}"#).source,
        DataRef::Run {
            cmd: "ls".into(),
            every: None
        }
    );
    let s = data(r#"{"stream": "tail -f log.jsonl", "format": "jsonl"}"#);
    assert_eq!(
        (s.source, s.format),
        (
            DataRef::Stream {
                cmd: "tail -f log.jsonl".into()
            },
            Some(DataFormat::Jsonl)
        )
    );
    let cases: [(&str, &str, &str); 7] = [
        (
            r#"{"type": "line", "data": {"file": "a", "run": "b"}, "x": "a"}"#,
            "\"data\" has \"file\" and \"run\"; give one",
            "\"run\"",
        ),
        (
            r#"{"type": "line", "data": {"format": "csv"}, "x": "a"}"#,
            "\"data\" needs one of \"file\", \"values\", \"run\" or \"stream\"",
            "{\"format",
        ),
        (
            r#"{"type": "line", "data": {"run": "x", "every": "soon"}, "x": "a"}"#,
            "\"every\" must be a time like \"30s\", \"5m\" or \"1h\"",
            "\"soon\"",
        ),
        (
            r#"{"type": "line", "data": {"file": "x", "every": "5s"}, "x": "a"}"#,
            "\"every\" goes with \"run\"",
            "\"every\"",
        ),
        (
            r#"{"type": "line", "data": {"file": "x", "delimiter": ";;"}, "x": "a"}"#,
            "\"delimiter\" must be one character",
            "\";;\"",
        ),
        (
            r#"{"type": "line", "data": {"file": "x", "format": "xml"}, "x": "a"}"#,
            "\"format\" must be \"csv\", \"tsv\", \"json\" or \"jsonl\"",
            "\"xml\"",
        ),
        (
            r#"{"type": "line", "data": 5, "x": "a"}"#,
            "\"data\" must be a file's name or an object, not a number",
            "5",
        ),
    ];
    for (text, msg, at) in cases {
        assert_eq!(fails(text), (msg.to_string(), place(text, at)), "{text}");
    }
}

#[test]
fn every_transform_reads_with_its_defaults() {
    let ts = chart(
        r#"{"type": "line", "data": "d.csv", "x": "t", "y": "v", "transform": [
  {"fold": ["a", "b"]},
  {"filter": "host", "in": ["web", 2]},
  {"filter": "host", "not_in": ["db"]},
  {"filter": "v", "range": [0, null]},
  {"filter": "v", "max": 10},
  {"bin": "v"},
  {"bin": "v", "step": 5, "max_bins": 8, "as": ["lo", "hi"]},
  {"aggregate": "count", "by": "host"},
  {"aggregate": {"p95": "v"}, "by": ["host", "t"], "as": "v95"},
  {"aggregate": {"count": "v"}},
  {"cumulative": "v", "by": "host"},
  {"window": "v", "size": 5, "as": "smooth"},
  {"sort": "t"},
  {"sort": "v", "descending": true},
  {"top": 8, "by": "host", "field": "v"}
]}"#,
    )
    .transform;
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        ts,
        [
            Transform::Fold {
                fields: s(&["a", "b"]),
                key: "key".into(),
                value: "value".into()
            },
            Transform::Filter {
                field: "host".into(),
                op: FilterOp::In(s(&["web", "2"]))
            },
            Transform::Filter {
                field: "host".into(),
                op: FilterOp::NotIn(s(&["db"]))
            },
            Transform::Filter {
                field: "v".into(),
                op: FilterOp::Range(Some(0.0), None)
            },
            Transform::Filter {
                field: "v".into(),
                op: FilterOp::Range(None, Some(10.0))
            },
            Transform::Bin {
                field: "v".into(),
                step: None,
                max_bins: 20,
                start: "v_start".into(),
                end: "v_end".into()
            },
            Transform::Bin {
                field: "v".into(),
                step: Some(5.0),
                max_bins: 8,
                start: "lo".into(),
                end: "hi".into()
            },
            Transform::Aggregate {
                op: AggOp::Count,
                field: None,
                by: s(&["host"]),
                out: "count".into()
            },
            Transform::Aggregate {
                op: AggOp::P95,
                field: Some("v".into()),
                by: s(&["host", "t"]),
                out: "v95".into()
            },
            Transform::Aggregate {
                op: AggOp::Count,
                field: Some("v".into()),
                by: vec![],
                out: "v".into()
            },
            Transform::Cumulative {
                field: "v".into(),
                by: s(&["host"]),
                out: "v_cumulative".into()
            },
            Transform::Window {
                field: "v".into(),
                size: 5,
                by: vec![],
                out: "smooth".into()
            },
            Transform::Sort {
                field: "t".into(),
                descending: false
            },
            Transform::Sort {
                field: "v".into(),
                descending: true
            },
            Transform::Top {
                n: 8,
                by: "host".into(),
                field: "v".into()
            },
        ]
    );
    let cases: [(&str, &str, &str); 6] = [
        (
            r#"{"type": "line", "data": "d", "transform": [{"pivot": "a"}]}"#,
            "a transform needs one of \"fold\", \"filter\", \"bin\", \"aggregate\", \"cumulative\", \"window\", \"sort\" or \"top\"",
            "{\"pivot",
        ),
        (
            r#"{"type": "line", "data": "d", "transform": [{"fold": ["a"], "sort": "b"}]}"#,
            "a transform does one thing: \"fold\" and \"sort\" are both here",
            "\"sort\"",
        ),
        (
            r#"{"type": "line", "data": "d", "transform": [{"aggregate": "sum"}]}"#,
            "\"aggregate\" must be \"count\" or one of count, sum, mean, min, max, p50 and p95 over a field, as {\"sum\": \"rpm\"}",
            "\"sum\"",
        ),
        (
            r#"{"type": "line", "data": "d", "transform": [{"window": "v", "size": 0}]}"#,
            "\"size\" must be a whole number of at least 1",
            "0}",
        ),
        (
            r#"{"type": "line", "data": "d", "transform": [{"filter": "v"}]}"#,
            "a filter needs \"in\", \"not_in\", \"range\", \"min\" or \"max\"",
            "{\"filter",
        ),
        (
            r#"{"type": "line", "data": "d", "transform": [{"top": 3, "by": "h"}]}"#,
            "\"top\" needs \"field\"",
            "{\"top",
        ),
    ];
    for (text, msg, at) in cases {
        assert_eq!(fails(text), (msg.to_string(), place(text, at)), "{text}");
    }
}

#[test]
fn view_and_series_blocks_and_the_tops_fig() {
    let c = chart(
        r##"{"type": "line", "data": "d.csv", "x": "t", "y": "v", "color": "host",
  "fig": {"menu": {"plane": {"axis_lines": false}}, "size": 3},
  "view": {"legend": "right", "labels": "ends", "motion": false, "grid": false},
  "series": {"web": {"color": "#00ff88"}, "db": [{"hidden": true}, null, {"dash": true}], "idle": {}}}"##,
    );
    assert_eq!(
        c.view,
        ViewSettings {
            grid: false,
            axes: false,
            legend: LegendPos::Right,
            labels: Labels::Ends,
            render_options: true,
            motion: false
        }
    );
    // The top's fig block styles the top's mark.
    assert_eq!(c.layers[0].fig.size, Some(3));
    assert_eq!(
        c.series_fig,
        [
            (
                SeriesKey::new("web", 0),
                FigStyle {
                    color: Some(ColorSpec::Rgb(0, 0xff, 0x88)),
                    ..FigStyle::default()
                }
            ),
            (
                SeriesKey::new("db", 0),
                FigStyle {
                    hidden: true,
                    ..FigStyle::default()
                }
            ),
            (
                SeriesKey::new("db", 2),
                FigStyle {
                    dash: true,
                    ..FigStyle::default()
                }
            ),
        ]
    );
    let legend = r#"{"type": "line", "data": "d", "view": {"legend": "left"}}"#;
    assert_eq!(
        fails(legend),
        (
            "\"legend\" must be \"top\", \"right\" or \"none\"".into(),
            place(legend, "\"left\"")
        )
    );
    let grid = r#"{"type": "line", "data": "d", "view": {"grid": "yes"}}"#;
    assert_eq!(
        fails(grid),
        (
            "\"grid\" must be true or false, not a string".into(),
            place(grid, "\"yes\"")
        )
    );
}

// Errors and warnings.

#[test]
fn every_error_has_its_place() {
    let cases: [(&str, &str, &str); 6] = [
        (
            r#"{"type": "pie", "data": "d"}"#,
            "unknown figure type \"pie\": expected chart, line, area, bar, scatter, heatmap, histogram, graph, stat, meter or dashboard",
            "\"pie\"",
        ),
        (
            "{\n  \"type\": \"line\",\n  \"title\": 42,\n  \"data\": \"d\"\n}",
            "\"title\" must be a string, not a number",
            "42",
        ),
        (
            r#"{"type": 3}"#,
            "\"type\" must be a string, not a number",
            "3",
        ),
        (
            r#"{"title": "no type", "x": 1}"#,
            "a figure needs a \"type\": chart, line, area, bar, scatter, heatmap, histogram, graph, stat, meter or dashboard",
            "{",
        ),
        // The reader's own errors keep their places.
        (
            "{\"type\": \"line\",\n \"data\": \"d\" \"x\": 1}",
            "expected ',' or '}'",
            "\"x\"",
        ),
        (
            r#"{"type": "line", "type": "bar"}"#,
            "duplicate key \"type\"",
            "\"type\": \"bar",
        ),
    ];
    for (text, msg, at) in cases {
        assert_eq!(fails(text), (msg.to_string(), place(text, at)), "{text}");
    }
    let empty = parse(" \n\t", "x.json", Path::new(DIR), &opt()).unwrap_err();
    assert_eq!(empty.msg, "nothing to draw: the buffer is empty");
    // About the whole text: no place, and the kind that draws as nothing to draw.
    assert_eq!(empty.at, None);
    assert_eq!(empty.kind, die_fig::ErrorKind::Empty);
}

#[test]
fn a_misspelt_key_is_a_warning_naming_the_key_it_means() {
    let text = r#"{"type": "line", "titel": "T", "data": "d.csv", "x": {"feild": "t"}, "y": "v", "colour": "h",
 "mark": {"type": "line", "curv": "step"}, "fig": {"colr": "red"}, "nonsense": 1, "z": 2}"#;
    // "x" needs its field: the misspelt one doesn't count.
    let e = parse(text, "f.json", Path::new(DIR), &opt()).unwrap_err();
    assert_eq!(e.msg, "\"x\" needs a \"field\" or a \"value\"");
    let text = text.replace(r#"{"feild": "t"}"#, r#"{"field": "t", "tilte": "T"}"#);
    let p = read(&text);
    let warned: Vec<(String, (u32, u32))> = p
        .warnings
        .iter()
        .map(|w| (w.msg.clone(), w.at.map(|a| (a.line, a.col)).unwrap()))
        .collect();
    assert_eq!(
        warned,
        [
            (
                "unknown field \"titel\"; did you mean \"title\"?".to_string(),
                place(&text, "\"titel\"")
            ),
            (
                "unknown field \"colour\"; did you mean \"color\"?".to_string(),
                place(&text, "\"colour\"")
            ),
            (
                "unknown field \"tilte\"; did you mean \"title\"?".to_string(),
                place(&text, "\"tilte\"")
            ),
            (
                "unknown field \"colr\"; did you mean \"color\"?".to_string(),
                place(&text, "\"colr\"")
            ),
            (
                "unknown field \"curv\"; did you mean \"curve\"?".to_string(),
                place(&text, "\"curv\"")
            ),
        ]
    );
    // The misspelt keys are tolerated, unread.
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(c.title, None);
    assert_eq!(c.layers[0].mark.curve, Curve::Linear);
}

// Dashboards and tiles.

#[test]
fn a_dashboard_is_rows_of_panels_over_named_data() {
    let text = r#"{
  "type": "dashboard",
  "title": "Service health",
  "data": { "req": "requests.csv",
            "err": { "run": "./errors.sh", "every": "30s" } },
  "time": { "field": "time", "range": "6h" },
  "rows": [
    [{ "type": "stat", "data": "req", "label": "Requests",
       "value": { "sum": "rpm" }, "trend": "time" },
     { "type": "meter", "data": "err", "label": "Errors", "value": "count", "max": 100, "format": ".0f" },
     { "type": "stat", "data": "other.csv", "value": {"mean": "ms"}, "delta": "previous", "good": "down", "span": 2 }],
    [{ "type": "line", "data": "req", "x": "time",
       "y": "rpm", "color": "service", "span": 3 },
     { "file": "errors.fig.json" }]
  ]
}"#;
    let p = read(text);
    assert_eq!(p.warnings, []);
    let Figure::Dashboard(d) = p.figure else {
        panic!()
    };
    assert_eq!(d.title.as_deref(), Some("Service health"));
    let keys: Vec<&str> = d.data.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["req", "err"]);
    assert_eq!(
        d.data[0].1.source,
        DataRef::File(PathBuf::from("/specs/requests.csv"))
    );
    assert!(
        matches!(&d.data[1].1.source, DataRef::Run { every: Some(e), .. } if *e == Duration::from_secs(30))
    );
    let t = d.time.as_ref().unwrap();
    assert_eq!((t.field.as_str(), t.range.as_deref()), ("time", Some("6h")));
    assert_eq!(d.rows.len(), 2);
    let spans: Vec<Vec<u16>> = d
        .rows
        .iter()
        .map(|r| r.iter().map(|p| p.span).collect())
        .collect();
    assert_eq!(spans, [vec![1, 1, 2], vec![3, 1]]);
    let Panel::Figure(f) = &d.rows[0][0].panel else {
        panic!()
    };
    let Figure::Stat(s) = &**f else { panic!() };
    assert_eq!(s.label, "Requests");
    assert_eq!(s.data.source, DataRef::Named("req".into()));
    assert_eq!(
        s.value,
        Aggregate {
            op: AggOp::Sum,
            field: Some("rpm".into())
        }
    );
    assert_eq!(
        (s.trend.as_deref(), s.delta.clone(), s.good),
        (Some("time"), None, GoodWhen::Up)
    );
    let Panel::Figure(f) = &d.rows[0][1].panel else {
        panic!()
    };
    let Figure::Meter(m) = &**f else { panic!() };
    assert_eq!(
        (m.label.as_str(), m.max, m.format.as_deref()),
        ("Errors", 100.0, Some(".0f"))
    );
    assert_eq!(
        m.value,
        Aggregate {
            op: AggOp::Count,
            field: None
        }
    );
    let Panel::Figure(f) = &d.rows[0][2].panel else {
        panic!()
    };
    let Figure::Stat(s) = &**f else { panic!() };
    // A name that isn't the dashboard's is a file; a stat without a label is its field.
    assert_eq!(
        s.data.source,
        DataRef::File(PathBuf::from("/specs/other.csv"))
    );
    assert_eq!(
        (s.label.as_str(), s.delta.clone(), s.good),
        ("ms", Some(Delta::Previous), GoodWhen::Down)
    );
    let Panel::Figure(f) = &d.rows[1][0].panel else {
        panic!()
    };
    let Figure::Chart(c) = &**f else { panic!() };
    assert_eq!(c.data.source, DataRef::Named("req".into()));
    assert_eq!(
        d.rows[1][1].panel,
        Panel::File(PathBuf::from("/specs/errors.fig.json"))
    );
}

#[test]
fn a_dashboards_panels_and_tiles_say_what_is_wrong() {
    // A panel without data takes the dashboard's only data.
    let one = r#"{"type": "dashboard", "data": {"req": "r.csv"}, "rows": [[{"type": "stat", "value": "count", "delta": 5}]]}"#;
    let Figure::Dashboard(d) = read(one).figure else {
        panic!()
    };
    let Panel::Figure(f) = &d.rows[0][0].panel else {
        panic!()
    };
    let Figure::Stat(s) = &**f else { panic!() };
    assert_eq!(
        (s.data.source.clone(), s.label.as_str(), s.delta.clone()),
        (
            DataRef::Named("req".into()),
            "count",
            Some(Delta::Value(5.0))
        )
    );
    let cases: [(&str, &str, &str); 8] = [
        (
            r#"{"type": "dashboard", "data": {"a": "a.csv", "b": "b.csv"}, "rows": [[{"type": "stat", "value": "count"}]]}"#,
            "a panel needs \"data\": one of \"a\", \"b\"",
            "{\"type\": \"stat",
        ),
        (
            r#"{"type": "dashboard", "rows": [[{"type": "dashboard", "rows": [[{"file": "x"}]]}]]}"#,
            "a panel can't be a dashboard",
            "{\"type\": \"dashboard\", \"rows\": [[{\"file",
        ),
        (
            r#"{"type": "dashboard", "rows": []}"#,
            "a dashboard needs \"rows\" of panels",
            "[]",
        ),
        (
            r#"{"type": "dashboard", "rows": [[]]}"#,
            "a row needs a panel",
            "[]]",
        ),
        (
            r#"{"type": "dashboard", "time": {"field": "t", "range": "a while"}, "rows": [[{"file": "x"}]]}"#,
            "\"range\" must be a time like \"6h\" or \"7d\"",
            "\"a while\"",
        ),
        (
            r#"{"type": "meter", "data": "d.csv", "value": {"max": "v"}, "max": 0}"#,
            "\"max\" must be a number above 0",
            "0}",
        ),
        (
            r#"{"type": "stat", "data": "d.csv"}"#,
            "a tile needs a \"value\", as {\"sum\": \"rpm\"}",
            "{",
        ),
        (
            r#"{"type": "dashboard", "rows": [[{"file": "x", "span": 0}]]}"#,
            "\"span\" must be a whole number of at least 1",
            "0}",
        ),
    ];
    for (text, msg, at) in cases {
        assert_eq!(fails(text), (msg.to_string(), place(text, at)), "{text}");
    }
}

// The guess.

fn table(columns: Vec<Column>) -> Table {
    Table::new(columns).unwrap()
}

fn only_layer(c: &Chart) -> (MarkKind, Option<Channel>, Option<Channel>, Option<Channel>) {
    assert_eq!(c.layers.len(), 1);
    let l = &c.layers[0];
    (
        l.mark.kind,
        l.enc.x.clone(),
        l.enc.y.clone(),
        l.enc.color.clone(),
    )
}

#[test]
fn a_time_column_makes_lines_one_a_number_column() {
    let t = table(vec![
        Column::time("time", vec![0.0, 300.0, 600.0]),
        Column::number("api", vec![642.0, 655.0, 610.0]),
        Column::number("worker", vec![431.0, 436.0, 440.0]),
        Column::number("batch", vec![388.0, 176.0, 200.0]),
    ]);
    let c = guess(&t).unwrap();
    assert_eq!(
        only_layer(&c),
        (
            MarkKind::Line,
            Some(on("time")),
            Some(on("value")),
            Some(on("series"))
        )
    );
    assert_eq!(
        c.transform,
        [Transform::Fold {
            fields: vec!["api".into(), "worker".into(), "batch".into()],
            key: "series".into(),
            value: "value".into()
        }]
    );
    // The guess's data is the table itself.
    assert_eq!(c.data.source, DataRef::Inline(Arc::new(t.clone())));
    assert_eq!((c.data.format, c.title.clone()), (None, None));

    // One number, coloured by the category beside it: long data.
    let long = table(vec![
        Column::category("service", &["api", "db", "api"]),
        Column::time("at", vec![0.0, 0.0, 60.0]),
        Column::number("rpm", vec![1.0, 2.0, 3.0]),
    ]);
    let c = guess(&long).unwrap();
    assert_eq!(
        only_layer(&c),
        (
            MarkKind::Line,
            Some(on("at")),
            Some(on("rpm")),
            Some(on("service"))
        )
    );
    assert_eq!(c.transform, []);
    // The folded columns' names keep clear of the table's own.
    let taken = table(vec![
        Column::time("t", vec![0.0, 1.0]),
        Column::number("series", vec![1.0, 2.0]),
        Column::number("value", vec![3.0, 4.0]),
    ]);
    assert!(
        matches!(&guess(&taken).unwrap().transform[0], Transform::Fold { key, value, .. } if key == "series_2" && value == "value_2")
    );
    let no_numbers = table(vec![
        Column::time("t", vec![0.0]),
        Column::category("c", &["a"]),
    ]);
    assert_eq!(
        guess(&no_numbers).unwrap_err().msg,
        "nothing to draw: no number column to draw over \"t\""
    );
}

#[test]
fn a_category_first_makes_bars_summed_where_it_repeats() {
    let t = table(vec![
        Column::category("region", &["eu", "us", "ap"]),
        Column::number("q1", vec![1.0, 2.0, 3.0]),
        Column::number("q2", vec![4.0, 5.0, 6.0]),
    ]);
    let c = guess(&t).unwrap();
    let band = Channel {
        scale: Some(ScaleKind::Band),
        ..on("region")
    };
    assert_eq!(
        only_layer(&c),
        (
            MarkKind::Bar,
            Some(band.clone()),
            Some(on("value")),
            Some(on("series"))
        )
    );
    // Grouped side by side: several measures don't add up.
    assert_eq!(c.layers[0].mark.stack, Stack::None);
    assert_eq!(c.transform.len(), 1);

    let repeated = table(vec![
        Column::text("host", &["a", "b", "a"]),
        Column::number("ms", vec![1.0, 2.0, 3.0]),
    ]);
    let c = guess(&repeated).unwrap();
    assert_eq!(
        c.transform,
        [Transform::Aggregate {
            op: AggOp::Sum,
            field: Some("ms".into()),
            by: vec!["host".into()],
            out: "ms".into()
        }]
    );
    // With one measure, a second category colours the bars; only repeated pairs are summed.
    let pairs = table(vec![
        Column::category("host", &["a", "a", "b"]),
        Column::category("kind", &["get", "put", "get"]),
        Column::number("n", vec![1.0, 2.0, 3.0]),
    ]);
    let c = guess(&pairs).unwrap();
    assert_eq!(unplaced(&c.layers[0].enc.color), Some(on("kind")));
    assert_eq!(c.transform, []);
}

#[test]
fn ordered_numbers_make_lines_unordered_ones_points_and_one_a_histogram() {
    let ordered = table(vec![
        Column::number("event", vec![0.0, 1.0, 1.0, 2.0]),
        Column::number("tokens", vec![5.0, 3.0, 8.0, 1.0]),
    ]);
    assert_eq!(
        only_layer(&guess(&ordered).unwrap()),
        (MarkKind::Line, Some(on("event")), Some(on("tokens")), None)
    );
    // `species` isn't first, so it colours points rather than making bars.
    let unordered = table(vec![
        Column::number("width", vec![3.0, 1.0, 2.0]),
        Column::category("species", &["a", "b", "a"]),
        Column::number("height", vec![1.0, 2.0, 3.0]),
        Column::number("depth", vec![9.0, 9.0, 9.0]),
    ]);
    let names: Vec<&str> = unordered.columns.iter().map(|c| &*c.name).collect();
    assert_eq!(names, ["width", "species", "height", "depth"]);
    assert_eq!(
        only_layer(&guess(&unordered).unwrap()),
        (
            MarkKind::Point,
            Some(on("width")),
            Some(on("height")),
            Some(on("species"))
        )
    );
    // A column that never changes isn't an order.
    let flat = table(vec![
        Column::number("k", vec![1.0, 1.0]),
        Column::number("v", vec![2.0, 3.0]),
    ]);
    assert_eq!(only_layer(&guess(&flat).unwrap()).0, MarkKind::Point);

    let one = table(vec![
        Column::text("note", &["x", "y", "z"]),
        Column::number("latency", vec![3.0, 10.0, 12.0]),
    ]);
    // A first column of text with a number is bars; a number alone is a histogram.
    assert_eq!(only_layer(&guess(&one).unwrap()).0, MarkKind::Bar);
    let alone = table(vec![Column::number("latency", vec![3.0, 10.0, 12.0])]);
    let c = guess(&alone).unwrap();
    assert_eq!(
        only_layer(&c),
        (
            MarkKind::Bar,
            Some(Channel {
                title: Some("latency".into()),
                scale: Some(ScaleKind::Linear),
                ..on("latency_start")
            }),
            Some(on("count")),
            None
        )
    );
    assert_eq!(c.transform.len(), 2);
    assert!(
        matches!(&c.transform[0], Transform::Bin { field, max_bins: 20, .. } if field == "latency")
    );

    let words = table(vec![Column::text("w", &["a"])]);
    assert_eq!(
        guess(&words).unwrap_err().msg,
        "nothing to draw: the data has no number column"
    );
    let empty = table(vec![Column::number("n", vec![])]);
    assert_eq!(
        guess(&empty).unwrap_err().msg,
        "nothing to draw: the data has no rows"
    );
    assert_eq!(
        guess(&Table::default()).unwrap_err().msg,
        "nothing to draw: the data has no columns"
    );
}

// Writing.

/// Write, read, write: the figure read back is the figure but for places, and its text is the
/// same text. Compared as their debug text, where a missing number (NaN) is equal to itself.
fn round_trip(f: &Figure) -> String {
    let text = write(f);
    let back = match parse(&text, "spec.fig.json", Path::new(DIR), &opt()) {
        Ok(p) => p,
        Err(e) => panic!("{e} in\n{text}"),
    };
    assert_eq!(back.warnings, [], "{text}");
    assert_eq!(
        format!("{:?}", back.figure.without_places()),
        format!("{:?}", f.without_places()),
        "{text}"
    );
    assert_eq!(write(&back.figure), text);
    text
}

#[test]
fn every_spec_written_reads_back_as_itself() {
    let specs = [
        REQUESTS,
        r#"{"type": "line", "x_label": "x", "series": [{"label": "a", "points": [{"x": 0, "y": 1}, {"x": 1.5, "y": -2}], "fig": {"color": "blue"}}, {"label": "a", "points": [{"x": 0, "y": 3}]}], "fig": {"menu": {"render_options": false}}}"#,
        r#"{"type": "histogram", "y_label": "n", "series": [{"label": "ok"}, {"label": "bad", "fig": {"hidden": true}}], "buckets": [{"label": "0", "values": {"bad": 2}}, {"label": "1"}]}"#,
        r##"{"type": "graph", "title": "G", "layout": "layered",
          "nodes": [{"label": "A", "id": "a", "extra": [1, {"deep": null}]}, {"id": 2, "group": "g", "size": 3}, {"id": "c", "fig": {"color": "#102030", "size": 2}}],
          "edges": [{"source": "a", "target": "2", "weight": 1.5}, {"from": "c", "to": "a", "directed": true, "label": "back"}], "view": {"axes": true}}"##,
        r#"{"type": "graph", "directed": true, "layout": {"type": "tree", "root": "a"}, "nodes": [{"id": "a"}, {"id": "b"}], "edges": [{"from": "a", "to": "b"}, {"from": "b", "to": "a", "directed": false}]}"#,
        r#"{"type": "scatter", "subtitle": "s", "data": {"run": "./gen.sh", "every": "90s", "format": "csv", "header": false},
          "x": {"field": "a", "scale": "log", "domain": [1, 100]}, "y": {"field": "b", "zero": true, "format": ",.2f"},
          "size": "c", "label": {"value": "dot"}, "tooltip": ["a", "b"], "detail": "c", "fig": {"size": 2, "dash": true}}"#,
        r#"{"type": "chart", "data": {"stream": "tail -f x", "format": "jsonl", "time_format": "%s"},
          "layers": [{"mark": {"type": "area", "stack": "normalize"}, "x": "t", "y": "v", "color": "k"},
                     {"mark": {"type": "line", "curve": "step", "points": true}, "x": "t", "y": "v", "transform": [{"filter": "k", "in": ["a"]}]},
                     {"mark": "band", "x": "t", "y": "lo", "y2": "hi", "data": "bands.csv"}]}"#,
        r#"{"type": "bar", "data": {"file": "-", "delimiter": "|"}, "x": {"field": "k", "type": "category", "scale": "band"}, "y": "v",
          "mark": {"horizontal": true, "stack": "none"},
          "transform": [{"fold": ["a", "b"], "as": ["k", "v"]}, {"filter": "v", "range": [null, 5]}, {"filter": "k", "not_in": ["x"]},
                        {"bin": "v", "step": 2, "max_bins": 7, "as": ["s", "e"]}, {"aggregate": {"mean": "v"}, "by": ["k"], "as": "m"},
                        {"aggregate": "count"}, {"cumulative": "m", "by": "k", "as": "c"}, {"window": "m", "size": 3},
                        {"sort": "m", "descending": true}, {"top": 3, "by": "k", "field": "m"}],
          "series": {"a": {"color": "violet"}, "b": [{"label": "B"}, {"show-label": false}]},
          "view": {"legend": "none", "labels": "none", "render_options": false, "motion": false, "grid": false, "axes": false}}"#,
        r#"{"type": "histogram", "data": "lat.csv", "x": "ms", "step": 5}"#,
        r#"{"type": "heatmap", "data": "h.csv", "x": {"field": "day", "type": "time", "unit": "s"}, "y": "hour", "color": "n"}"#,
        r#"{"type": "dashboard", "title": "D", "data": {"req": {"file": "r.csv", "thousands": ","}, "err": {"run": "e", "every": "1500ms"}},
          "time": {"field": "time", "range": "6h"},
          "rows": [[{"type": "stat", "data": "req", "label": "R", "value": {"sum": "rpm"}, "delta": -3, "trend": "time", "format": ",", "good": "neither"},
                    {"type": "meter", "data": "err", "label": "E", "value": {"count": "id"}, "max": 50, "span": 2}],
                   [{"type": "line", "data": "req", "x": "time", "y": "rpm", "span": 3}, {"file": "/abs/other.fig.json", "span": 2}],
                   [{"type": "graph", "nodes": [{"id": "x"}]}, {"type": "line", "series": [{"label": "s", "points": [{"x": 1, "y": 1}]}]}]]}"#,
    ];
    for s in specs {
        round_trip(&read(s).figure);
    }
}

#[test]
fn a_written_spec_has_its_keys_in_a_fixed_order() {
    let a = read(
        r#"{"mark": "line", "y": "v", "data": "d.csv", "x": "t", "title": "T", "type": "chart"}"#,
    );
    let b = read(r#"{"title": "T", "type": "line", "x": "t", "y": "v", "data": "d.csv"}"#);
    assert_eq!(write(&a.figure), write(&b.figure));
    assert_eq!(
        write(&a.figure),
        "{ \"type\": \"chart\", \"title\": \"T\", \"data\": \"/specs/d.csv\", \"x\": \"t\", \"y\": \"v\", \"mark\": \"line\" }\n"
    );
    // A long one breaks over lines, two spaces an indent.
    let text = write(&read(REQUESTS).figure);
    assert_eq!(
        text,
        r#"{
  "type": "chart",
  "title": "Requests per minute",
  "data": "/specs/requests.csv",
  "transform": [{ "fold": ["api", "worker", "batch"], "as": ["service", "rpm"] }],
  "x": { "field": "time", "type": "time" },
  "color": "service",
  "layers": [
    { "mark": "line", "y": { "field": "rpm", "title": "Requests / min" } },
    { "mark": "rule", "y": { "value": 1000 }, "fig": { "color": "warning", "label": "capacity" } }
  ]
}
"#
    );
}

#[test]
fn fig_files_are_written_as_figs_own_shapes() {
    let line = read(
        r#"{"type": "line", "series": [{"label": "a", "points": [{"x": 0, "y": 1}]}, {"label": "a", "points": [{"x": 0, "y": 2}]}]}"#,
    );
    let text = round_trip(&line.figure);
    assert!(
        text.starts_with("{\n  \"type\": \"line\",\n  \"series\": ["),
        "{text}"
    );
    let hist = read(
        r#"{"type": "histogram", "series": [{"label": "s"}], "buckets": [{"label": "b", "values": {"s": 1}}]}"#,
    );
    let text = round_trip(&hist.figure);
    assert!(text.contains("\"buckets\""), "{text}");
    // Once it's no longer fig's line, it's a chart with its rows.
    let Figure::Chart(mut c) = line.figure else {
        panic!()
    };
    c.layers[0].mark.points = true;
    assert!(write(&Figure::Chart(c)).contains("\"values\""));
}

#[test]
fn a_guess_written_with_its_file_reads_back_as_itself() {
    let t = table(vec![
        Column::time("time", vec![0.0, 300.0]),
        Column::number("api", vec![1.0, 2.0]),
        Column::number("db", vec![3.0, 4.0]),
    ]);
    for t in [
        t,
        table(vec![Column::number("ms", vec![1.0, 2.0, 9.0])]),
        table(vec![
            Column::category("k", &["a", "a"]),
            Column::number("v", vec![1.0, 2.0]),
        ]),
    ] {
        let mut c = guess(&t).unwrap();
        c.data.source = DataRef::File(PathBuf::from("/data/requests.csv"));
        c.data.format = Some(DataFormat::Csv);
        let text = round_trip(&Figure::Chart(c));
        assert!(
            text.contains("\"file\": \"/data/requests.csv\", \"format\": \"csv\""),
            "{text}"
        );
    }
}

#[test]
fn nodes_and_edges_whose_objects_no_longer_fit_are_written_from_their_fields() {
    let Figure::Graph(mut g) = read(
        r#"{"type": "graph", "nodes": [{"note": "kept", "id": "a", "metadata": {"owner": "x"}}, {"id": "b"}], "edges": [{"from": "a", "to": "b", "why": "x"}]}"#,
    )
    .figure
    else {
        panic!()
    };
    g.nodes[0].label = Some("Alpha".into());
    g.edges[0].weight = Some(2.0);
    let text = write(&Figure::Graph(g.clone()));
    let Figure::Graph(back) = read(&text).figure else {
        panic!()
    };
    assert_eq!(back.nodes[0].label.as_deref(), Some("Alpha"));
    assert_eq!(back.edges[0].weight, Some(2.0));
    // The fields first, then the object's other keys in their order.
    let keys: Vec<&str> = back.nodes[0]
        .raw
        .as_obj()
        .unwrap()
        .iter()
        .map(|(k, _)| &*k.name)
        .collect();
    assert_eq!(keys, ["id", "label", "note", "metadata"]);
    // fig's metadata, which the detail panel shows, is the node's still.
    assert_eq!(
        back.nodes[0].raw.get("metadata").map(|m| m.to_text()),
        Some(r#"{"owner":"x"}"#.to_string())
    );
    let keys: Vec<&str> = back.edges[0]
        .raw
        .as_obj()
        .unwrap()
        .iter()
        .map(|(k, _)| &*k.name)
        .collect();
    assert_eq!(keys, ["from", "to", "weight", "why"]);
    // And from then on it reads back as itself.
    round_trip(&Figure::Graph(back));
}

#[test]
fn durations_read_and_write_in_the_largest_whole_unit() {
    use die_fig::spec::chart::{format_duration, parse_duration};
    let ms = |s: &str| parse_duration(s).map(|d| d.as_millis());
    assert_eq!(ms("30s"), Some(30_000));
    assert_eq!(ms("1h30m"), Some(5_400_000));
    assert_eq!(ms("1.5d"), Some(129_600_000));
    assert_eq!(ms("250ms"), Some(250));
    assert_eq!(
        (ms(""), ms("5"), ms("5x"), ms("s"), ms("1..2s")),
        (None, None, None, None, None)
    );
    let f = |ms: u64| format_duration(Duration::from_millis(ms));
    assert_eq!(
        [f(30_000), f(5_400_000), f(86_400_000), f(1500), f(0)],
        ["30s", "90m", "1d", "1500ms", "0ms"]
    );
    // A number of seconds has the same bound as the text, so whatever reads is written as text
    // that reads again.
    for big in ["1000000000000", "1e308"] {
        let text = format!(
            r#"{{"type": "line", "data": {{"run": "x", "every": {big}}}, "x": "t", "y": "v"}}"#
        );
        assert_eq!(
            fails(&text),
            (
                "\"every\" must be a time like \"30s\", \"5m\" or \"1h\"".to_string(),
                place(&text, big)
            )
        );
    }
    let c = chart(
        r#"{"type": "line", "data": {"run": "x", "every": 999999999999}, "x": "t", "y": "v"}"#,
    );
    round_trip(&Figure::Chart(c));
}

#[test]
fn a_relative_dir_is_made_absolute_so_paths_read_back_the_same() {
    let text = r#"{"type": "dashboard", "data": {"d": "d.csv"}, "rows": [[{"type": "line", "x": "t", "y": "v"}, {"file": "p.fig.json"}]]}"#;
    let read_in = |text: &str| {
        parse(text, "f.json", Path::new("specs"), &opt())
            .unwrap()
            .figure
    };
    let f = read_in(text);
    let Figure::Dashboard(d) = &f else { panic!() };
    let DataRef::File(p) = &d.data[0].1.source else {
        panic!()
    };
    assert!(p.is_absolute() && p.ends_with("specs/d.csv"), "{p:?}");
    let Panel::File(p) = &d.rows[0][1].panel else {
        panic!()
    };
    assert!(p.is_absolute() && p.ends_with("specs/p.fig.json"), "{p:?}");
    let back = read_in(&write(&f));
    assert_eq!(back.without_places(), f.without_places());
    // No directory: the paths stay as they are given.
    let f = parse(text, "f.json", Path::new(""), &opt()).unwrap().figure;
    let Figure::Dashboard(d) = &f else { panic!() };
    assert_eq!(d.data[0].1.source, DataRef::File(PathBuf::from("d.csv")));
}

#[test]
fn a_nodes_and_an_edges_other_keys_are_data_and_figs_axis_labels_are_pointed_to_titles() {
    // fig keeps any key a node or an edge has; near ones aren't misspellings.
    let p = read(
        r#"{"type": "graph", "nodes": [{"id": "a", "level": 1, "groups": 2, "sizes": 3, "lable": "A"}, {"id": "b"}],
            "edges": [{"from": "a", "to": "b", "height": 2}]}"#,
    );
    assert_eq!(p.warnings, []);
    // A chart's axis titles are its channels'.
    let text = r#"{"type": "line", "data": "d.csv", "titel": "T", "x": "t", "y_label": "ms", "y": "v", "x_label": "time"}"#;
    let p = read(text);
    let warned: Vec<(String, (u32, u32))> = p
        .warnings
        .iter()
        .map(|w| (w.msg.clone(), w.at.map(|a| (a.line, a.col)).unwrap()))
        .collect();
    assert_eq!(
        warned,
        [
            (
                "unknown field \"titel\"; did you mean \"title\"?".to_string(),
                place(text, "\"titel\"")
            ),
            (
                "\"y_label\" is fig's line's and histogram's; a chart's axis title is the channel's \"title\", as \"y\": {\"field\": …, \"title\": …}".to_string(),
                place(text, "\"y_label\"")
            ),
            (
                "\"x_label\" is fig's line's and histogram's; a chart's axis title is the channel's \"title\", as \"x\": {\"field\": …, \"title\": …}".to_string(),
                place(text, "\"x_label\"")
            ),
        ]
    );
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(unplaced(&c.layers[0].enc.x), Some(on("t")));
}

#[test]
fn a_table_without_its_objects_is_written_as_its_columns() {
    // Times to the fewest decimals that read back as the same seconds, a missing cell null,
    // and the rows' blocks under "fig".
    let mut t = table(vec![
        Column::time(
            "time",
            vec![1_763_704_800.123, 1_700_000_000.999, f64::NAN, 0.0],
        ),
        Column {
            name: "k".into(),
            values: Values::Category {
                codes: vec![0, 1, 0, u32::MAX].into(),
                dict: vec!["a".into(), "b".into()].into(),
            },
        },
        Column::number("v", vec![1.0, 2.5, f64::NAN, -3.0]),
    ]);
    t.styles = Some(
        vec![
            FigStyle {
                dash: true,
                ..FigStyle::default()
            },
            FigStyle::default(),
            FigStyle::default(),
            FigStyle::default(),
        ]
        .into(),
    );
    let text = write(&Figure::Chart(guess(&t).unwrap()));
    assert!(
        text.contains(
            r#""time": ["2025-11-21T06:00:00.123Z", "2023-11-14T22:13:20.999Z", null, "1970-01-01T00:00:00Z"],"#
        ),
        "{text}"
    );
    assert!(text.contains(r#""k": ["a", "b", "a", null],"#), "{text}");
    assert!(text.contains(r#""v": [1, 2.5, null, -3],"#), "{text}");
    assert!(
        text.contains(r#""fig": [{ "dash": true }, {}, {}, {}]"#),
        "{text}"
    );
    // A column named "fig", which as a key would be the blocks: a header row, then a list a row.
    let t = table(vec![
        Column::category("k", &["a", "b"]),
        Column::number("fig", vec![1.0, 2.0]),
    ]);
    let text = write(&Figure::Chart(guess(&t).unwrap()));
    assert!(
        text.contains(r#""values": [["k", "fig"], ["a", 1], ["b", 2]]"#),
        "{text}"
    );
    // No rows: the columns are kept by their names.
    let t = table(vec![
        Column::number("a", vec![]),
        Column::number("b", vec![]),
    ]);
    let mut c = guess(&table(vec![
        Column::number("a", vec![1.0]),
        Column::number("b", vec![2.0]),
    ]))
    .unwrap();
    c.data.source = DataRef::Inline(Arc::new(t));
    assert!(write(&Figure::Chart(c)).contains(r#""values": { "a": [], "b": [] }"#));
    // To the nanosecond where it takes them; a time no timestamp holds is null, not a number
    // among times.
    let t = table(vec![
        Column::time("t", vec![1.5e-6, -0.5, 1e15]),
        Column::number("v", vec![1.0, 2.0, 3.0]),
    ]);
    let text = write(&Figure::Chart(guess(&t).unwrap()));
    assert!(
        text.contains(r#""t": ["1970-01-01T00:00:00.0000015Z", "1969-12-31T23:59:59.5Z", null]"#),
        "{text}"
    );
    // A long column fills its lines.
    let t = table(vec![Column::number("ms", (0..40).map(f64::from).collect())]);
    let text = write(&Figure::Chart(guess(&t).unwrap()));
    assert!(
        text.contains(
            "\"ms\": [\n        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,\n        25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39\n      ]"
        ),
        "{text}"
    );
    assert!(text.lines().all(|l| l.chars().count() <= 100), "{text}");
}

// Bare data, through the data module's readers.

#[test]
fn a_csv_buffer_is_read_and_guessed() {
    let csv = "time,api,worker,batch\n2026-11-21T06:00,642,431,388\n2026-11-21T06:05,655,436,176\n";
    let p = parse(csv, "requests.csv", Path::new(DIR), &opt()).unwrap();
    assert!(p.inferred);
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(c.data.format, Some(DataFormat::Csv));
    assert_eq!(inline(&c).rows, 2);
    assert_eq!(
        only_layer(&c),
        (
            MarkKind::Line,
            Some(on("time")),
            Some(on("value")),
            Some(on("series"))
        )
    );
    // No name: the text says CSV, or TSV by its tabs.
    let p = parse(&csv.replace(',', "\t"), "scratch", Path::new(DIR), &opt()).unwrap();
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(c.data.format, Some(DataFormat::Tsv));
    let p = parse(&csv.replace(',', "\t"), "r.tsv", Path::new(DIR), &opt()).unwrap();
    assert!(p.inferred);
}

#[test]
fn json_rows_and_jsonl_are_read_and_guessed() {
    let rows = r#"[{"k": "a", "v": 1}, {"k": "b", "v": 2}]"#;
    let p = parse(rows, "rows.json", Path::new(DIR), &opt()).unwrap();
    assert!(p.inferred);
    let Figure::Chart(c) = p.figure else { panic!() };
    assert_eq!(c.data.format, Some(DataFormat::Json));
    assert_eq!(only_layer(&c).0, MarkKind::Bar);
    // An object whose every value is an array is columns of data, not a spec.
    let cols = r#"{"time": ["2026-11-21T06:00:00Z", "2026-11-21T06:05:00Z"], "api": [1, 2]}"#;
    let p = parse(cols, "cols.json", Path::new(DIR), &opt()).unwrap();
    assert!(p.inferred);
    let lines = "{\"x\": 1, \"y\": 5}\n{\"x\": 2, \"y\": 3}\n";
    for name in ["log.jsonl", "log.json", "log"] {
        let p = parse(lines, name, Path::new(DIR), &opt()).unwrap();
        let Figure::Chart(c) = p.figure else { panic!() };
        assert_eq!(
            (p.inferred, c.data.format),
            (true, Some(DataFormat::Jsonl)),
            "{name}"
        );
    }
}

#[test]
fn inline_values_are_read_and_written_back() {
    let c = chart(
        r#"{"type": "bar", "data": {"values": [{"k": "a", "v": 1, "fig": {"color": "red"}}, {"k": "b", "v": 2}]}, "x": "k", "y": "v"}"#,
    );
    let t = inline(&c);
    assert_eq!((t.rows, t.columns.len()), (2, 2));
    assert!(t.raw.is_some());
    round_trip(&Figure::Chart(c));
    let csv =
        chart(r#"{"type": "line", "data": {"values": "x,y\n1,2\n2,3\n"}, "x": "x", "y": "y"}"#);
    assert_eq!(inline(&csv).rows, 2);
    // CSV text, written as its columns, reads back with no objects, as it was.
    let text = round_trip(&Figure::Chart(csv));
    assert!(
        text.contains(r#""values": { "x": [1, 2], "y": [2, 3] }"#),
        "{text}"
    );
    // A header and no rows keeps its columns; a column named "fig" stays a column; a missing
    // cell is null.
    for spec in [
        r#"{"type": "line", "data": {"values": "x,y\n1,\n2,3\n"}, "x": "x", "y": "y"}"#,
        r#"{"type": "line", "data": {"values": "k,v\n"}, "x": "k", "y": "v"}"#,
        r#"{"type": "bar", "data": {"values": "k,fig\na,1\nb,2\n"}, "x": "k", "y": "fig"}"#,
    ] {
        let c = chart(spec);
        assert_eq!(inline(&c).columns.len(), 2, "{spec}");
        round_trip(&Figure::Chart(c));
    }
    // Columns with their blocks, and JSON text read as JSON.
    let c = chart(
        r#"{"type": "bar", "data": {"values": {"k": ["a", "b"], "v": [1, 2], "fig": [{"color": "red"}, {}]}}, "x": "k", "y": "v"}"#,
    );
    assert!(inline(&c).styles.is_some() && inline(&c).raw.is_none());
    round_trip(&Figure::Chart(c));
    let c = chart(
        r#"{"type": "bar", "data": {"values": "[{\"k\": \"a\", \"v\": 1}]", "format": "json"}, "x": "k", "y": "v"}"#,
    );
    assert_eq!((inline(&c).rows, inline(&c).columns.len()), (1, 2));
    round_trip(&Figure::Chart(c));
    // A guess's table, written as its columns, times exact.
    let t = table(vec![
        Column::time("time", vec![1_763_704_800.123, 1_763_705_100.0]),
        Column::category("k", &["a", "b"]),
        Column::number("v", vec![1.0, 2.5]),
    ]);
    round_trip(&Figure::Chart(guess(&t).unwrap()));
    // And a bare buffer's, a column named "fig" among them.
    for (name, text) in [
        ("d.csv", "a,fig\nx,1\ny,2\nz,3\n"),
        ("d.json", r#"{"k": ["a", "b"], "v": [1, 2]}"#),
    ] {
        let p = parse(text, name, Path::new(DIR), &opt()).unwrap();
        round_trip(&p.figure);
    }
}

#[test]
fn an_error_in_values_text_is_at_the_string_with_its_place_in_the_text() {
    let cases = [
        (
            "{\n  \"type\": \"line\", \"x\": \"x\", \"y\": \"y\",\n  \"data\": {\"values\": \"x,y\\n1,\\\"2\\n\"}\n}",
            "in \"values\", line 2, column 3: unterminated quoted field",
        ),
        (
            "{\n  \"type\": \"line\", \"x\": \"x\", \"y\": \"y\",\n\n  \"data\": {\"values\": \"{\\\"x\\\": 1}\\n{3}\\n\", \"format\": \"jsonl\"}\n}",
            "in \"values\", line 2, column 2: expected a string key",
        ),
        (
            r#"{"type": "bar", "x": "k", "y": "v", "data": {"values": "[{\"k\": 1,]", "format": "json"}}"#,
            "in \"values\", line 1, column 10: expected a string key",
        ),
    ];
    for (text, msg) in cases {
        let at = text.find("\"values\": \"").unwrap() + "\"values\": ".len();
        assert_eq!(
            fails(text),
            (msg.to_string(), place(text, &text[at..at + 3])),
            "{text}"
        );
    }
}

#[test]
fn a_histograms_bins_are_named_clear_of_its_inline_columns() {
    use die_fig::data::transform::apply;
    let c = chart(
        r#"{"type": "histogram", "data": {"values": "ms,ms_start,count\n1,a,x\n2,b,y\n9,c,z\n"}, "x": "ms"}"#,
    );
    let l = &c.layers[0];
    assert_eq!(
        unplaced(&l.enc.x).and_then(|x| x.field),
        Some("ms_start_2".into())
    );
    assert_eq!(
        unplaced(&l.enc.y).and_then(|y| y.field),
        Some("count_2".into())
    );
    let binned = apply(inline(&c), &l.transform).unwrap();
    assert!(binned.column("ms_start_2").is_some() && binned.column("count_2").is_some());
    round_trip(&Figure::Chart(c));
}

#[test]
fn one_line_of_jsonl_with_a_type_no_figure_has_is_a_row() {
    for name in ["log", "log.json", "log.jsonl"] {
        let p = parse(
            "{\"type\": \"request\", \"ms\": 5}\n",
            name,
            Path::new(DIR),
            &opt(),
        )
        .unwrap();
        let Figure::Chart(c) = p.figure else { panic!() };
        assert_eq!(
            (p.inferred, c.data.format, inline(&c).rows),
            (true, Some(DataFormat::Jsonl), 1),
            "{name}"
        );
    }
    // A spec with a type no figure has is still an error: it draws from something, or it makes
    // no figure as a row.
    for text in [
        r#"{"type": "pie", "data": "d"}"#,
        r#"{"type": "bars"}"#,
        "{\n  \"type\": \"bars\",\n  \"n\": 1\n}",
    ] {
        let (msg, _) = fails(text);
        assert!(msg.starts_with("unknown figure type"), "{text}: {msg}");
    }
}

#[test]
fn a_domain_may_be_two_times() {
    let c = chart(
        r#"{"type": "line", "data": "d.csv", "x": {"field": "t", "domain": ["2026-11-21T00:00:00Z", "2026-11-22T00:00:00Z"]}}"#,
    );
    let d = c.layers[0].enc.x.as_ref().unwrap().domain.unwrap();
    assert_eq!(d.1 - d.0, 86_400.0);
}
