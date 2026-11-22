//! The graph figure: layouts (deterministic, fig's stress to its own positions, the one auto
//! picks), labels that never overlap a label or a node, the card clear of its node, nav (cones,
//! neighbours, first and last, zoom, pan, reveal, a drag along its rank), the path, hover, the
//! tables and names, a tree from a table, emphasis, and every fixture drawn at three sizes,
//! dark and light.

use die_test_cap as _;

use die_fig::data::ReadOptions;
use die_fig::graph::nav::{self, Dir};
use die_fig::graph::overlay::card_rect;
use die_fig::graph::{self, Emphasis, GraphLayout, GraphModel, OverlayState, Target};
use die_fig::interact::{Action, Cursor, View};
use die_fig::scene::{FixedMeasure, Item, Measure, Rect, Scene};
use die_fig::spec::{self, Figure, GraphSpec, Layout};
use die_fig::theme::Theme;
use jiff::tz::TimeZone;
use std::path::Path;

const FIXTURES: &[&str] = &[
    "services.json",
    "fig-graph.json",
    "org.csv",
    "pipeline.json",
    "big.json",
    "cycle.json",
];

fn dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/graphs")
}

fn opt() -> ReadOptions {
    ReadOptions::new(TimeZone::fixed(jiff::tz::Offset::UTC))
}

/// A graph spec from text, as `\v` reads it.
fn spec_of(text: &str, name: &str) -> GraphSpec {
    match spec::parse(text, name, Path::new("."), &opt())
        .expect("it parses")
        .figure
    {
        Figure::Graph(g) => g,
        other => panic!("not a graph: {other:?}"),
    }
}

fn fixture(name: &str) -> GraphModel {
    let text = std::fs::read_to_string(dir().join(name)).expect("the fixture reads");
    GraphModel::build(spec_of(&text, name))
        .expect("it builds")
        .named(name)
}

fn model(text: &str) -> GraphModel {
    GraphModel::build(spec_of(text, "g.json")).expect("it builds")
}

fn base(m: &GraphModel, view: &View, size: (f32, f32)) -> (Scene, GraphLayout) {
    graph::base(m, view, size, 1.0, &Theme::mono_dark(), &mut FixedMeasure)
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() - 0.01
        && b.x < a.right() - 0.01
        && a.y < b.bottom() - 0.01
        && b.y < a.bottom() - 0.01
}

fn circle_meets(c: (f32, f32), r: f32, rect: &Rect) -> bool {
    let nx = c.0.clamp(rect.x, rect.right());
    let ny = c.1.clamp(rect.y, rect.bottom());
    (c.0 - nx).hypot(c.1 - ny) < r - 0.01
}

fn close(got: &[(f64, f64)], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9,
            "{g:?} is not {w:?}"
        );
    }
}

// ---------------------------------------------------------------- layouts

#[test]
fn layouts_are_deterministic() {
    for f in FIXTURES {
        let (a, b) = (fixture(f), fixture(f));
        assert_eq!(a.positions, b.positions, "{f}");
        assert_eq!(a.bends, b.bends, "{f}");
        assert_eq!(a.layout, b.layout, "{f}");
    }
}

#[test]
fn the_force_layout_is_figs_stress_on_figs_example_graph() {
    // fig's examples/graph.json: 0 api, 1 worker, 2 database, 3 cache; 0-1, 0-2, 1-3.
    let m = model(
        r#"{"layout": "force", "nodes": [{"id": "api"}, {"id": "worker"}, {"id": "database"},
            {"id": "cache"}], "edges": [{"source": "api", "target": "worker"},
            {"source": "api", "target": "database"}, {"source": "worker", "target": "cache"}]}"#,
    );
    assert_eq!(m.layout, Layout::Force);
    close(
        &m.positions,
        &[
            (-0.039908450688, -0.326829508501),
            (-0.081983877665, 0.349288178060),
            (-0.056958136831, -1.000000000000),
            (0.178850465183, 0.977541330441),
        ],
    );
}

#[test]
fn the_force_layout_is_figs_stress_for_components_and_an_isolated_node() {
    let m = model(
        r#"{"nodes": [{"id": "0"}, {"id": "1"}, {"id": "2"}, {"id": "3"}, {"id": "4"}, {"id": "5"}],
            "edges": [{"from": "0", "to": "1"}, {"from": "1", "to": "2"}, {"from": "3", "to": "4"},
            {"from": "4", "to": "0"}, {"from": "2", "to": "4"}], "layout": "force"}"#,
    );
    let edges = [(0, 1), (1, 2), (3, 4), (4, 0), (2, 4)];
    close(&m.positions, &die_fig::algo::stress(6, &edges));
    let m = model(
        r#"{"layout": "force", "nodes": [{"id": "0"}, {"id": "1"}, {"id": "2"}, {"id": "3"},
            {"id": "4"}, {"id": "5"}], "edges": [{"from": "0", "to": "1"}, {"from": "1", "to": "2"},
            {"from": "3", "to": "4"}]}"#,
    );
    close(
        &m.positions,
        &[
            (0.687498714967, 0.328656822663),
            (0.055221041278, -0.327432769900),
            (-0.563862029475, -0.828441994245),
            (-0.281407316765, 1.000000000000),
            (-0.877835950919, 0.537197683153),
            (0.980385540914, -0.709979741672),
        ],
    );
}

#[test]
fn auto_picks_a_tree_for_a_forest_layers_for_a_dag_and_stress_otherwise() {
    let tree = model(
        r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}, {"id": "d"}],
            "edges": [{"from": "a", "to": "b"}, {"from": "a", "to": "c"}, {"from": "c", "to": "d"}]}"#,
    );
    assert_eq!(tree.layout, Layout::Tree { root: None });
    // Roots on top: y is up, depth down.
    assert!(tree.positions[0].1 > tree.positions[1].1);
    assert_eq!(tree.positions[1].1, tree.positions[2].1);
    let dag = model(
        r#"{"directed": true, "nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}, {"id": "d"}],
            "edges": [{"from": "a", "to": "b"}, {"from": "a", "to": "c"}, {"from": "b", "to": "d"},
            {"from": "c", "to": "d"}]}"#,
    );
    assert_eq!(dag.layout, Layout::Layered);
    let cyclic = model(
        r#"{"directed": true, "nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
            "edges": [{"from": "a", "to": "b"}, {"from": "b", "to": "c"}, {"from": "c", "to": "a"}]}"#,
    );
    assert_eq!(cyclic.layout, Layout::Force);
    // No edges at all: stress, not a row of roots.
    assert_eq!(
        model(r#"{"nodes": [{"id": "a"}, {"id": "b"}]}"#).layout,
        Layout::Force
    );
    // The spec's layout wins.
    let forced = model(
        r#"{"layout": "force", "nodes": [{"id": "a"}, {"id": "b"}], "edges": [{"from": "a", "to": "b"}]}"#,
    );
    assert_eq!(forced.layout, Layout::Force);
    assert_eq!(fixture("org.csv").layout, Layout::Tree { root: None });
    assert_eq!(fixture("pipeline.json").layout, Layout::Layered);
    assert_eq!(fixture("services.json").layout, Layout::Force);
    assert_eq!(fixture("cycle.json").layout, Layout::Force);
}

#[test]
fn a_tree_from_a_root_and_a_wide_tree_stacks_its_leaves() {
    let m = model(
        r#"{"layout": {"type": "tree", "root": "c"}, "nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
            "edges": [{"from": "a", "to": "b"}, {"from": "b", "to": "c"}]}"#,
    );
    // c on top, then b, then a.
    assert!(m.positions[2].1 > m.positions[1].1 && m.positions[1].1 > m.positions[0].1);
    let org = fixture("org.csv");
    // Ben (6) manages five leaves, stacked under him, the same x, a step apart.
    let kids: Vec<usize> = (0..org.len())
        .filter(|&i| org.stacked[i] == org.index_of("6").map(|p| p as u32))
        .collect();
    assert_eq!(kids.len(), 5);
    let xs: Vec<f64> = kids.iter().map(|&i| org.positions[i].0).collect();
    assert!(xs.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-12));
    let ys: Vec<f64> = kids.iter().map(|&i| org.positions[i].1).collect();
    assert!(
        ys.windows(2)
            .all(|w| (w[0] - w[1] - graph::STACK_STEP).abs() < 1e-12)
    );
}

#[test]
fn the_sparse_stress_is_deterministic_and_keeps_links_short() {
    // Six rings of 50, each node linked to its next and to one across, rings joined in a chain.
    let mut edges = Vec::new();
    for c in 0..6usize {
        for j in 0..50usize {
            edges.push((c * 50 + j, c * 50 + (j + 1) % 50));
            edges.push((c * 50 + j, c * 50 + (j + 17) % 50));
        }
        if c > 0 {
            edges.push(((c - 1) * 50, c * 50));
        }
    }
    let a = die_fig::algo::stress_sparse(300, &edges);
    assert_eq!(a, die_fig::algo::stress_sparse(300, &edges));
    let d = |i: usize, j: usize| (a[i].0 - a[j].0).hypot(a[i].1 - a[j].1);
    let linked = edges.iter().map(|&(i, j)| d(i, j)).sum::<f64>() / edges.len() as f64;
    let mut all = 0.0;
    let mut n = 0.0;
    for i in (0..300).step_by(7) {
        for j in (0..300).step_by(11) {
            if i != j {
                all += d(i, j);
                n += 1.0;
            }
        }
    }
    assert!(
        linked < all / n / 2.0,
        "links {linked} against all {}",
        all / n
    );
    assert!(
        a.iter()
            .all(|p| p.0.abs() <= 1.0 + 1e-9 && p.1.abs() <= 1.0 + 1e-9)
    );
}

// ---------------------------------------------------------------- the model

#[test]
fn a_model_counts_degrees_groups_and_lanes_and_checks_its_ends() {
    let m = fixture("cycle.json");
    let running = m.index_of("running").expect("running");
    assert_eq!(m.degree_out[running], 4);
    assert_eq!(m.degree_in[running], 2);
    // running→paused twice and paused→running: three lanes.
    let lanes: Vec<(u16, u16)> = m
        .ends
        .iter()
        .zip(&m.lanes)
        .filter(|((a, b), _)| {
            let p = m.index_of("paused").expect("paused") as u32;
            (*a == running as u32 && *b == p) || (*a == p && *b == running as u32)
        })
        .map(|(_, l)| *l)
        .collect();
    assert_eq!(lanes, [(0, 3), (1, 3), (2, 3)]);
    let s = fixture("services.json");
    assert_eq!(s.groups, ["platform", "edge", "data"]);
    // Radii on a square-root scale from 0: 3 to 9 px.
    let most = s.radius.iter().copied().fold(0.0, f32::max);
    assert!((most - 9.0).abs() < 1e-4);
    assert!(s.radius.iter().all(|&r| (3.0..=9.0).contains(&r)));
    // An unknown end is a mismatch at the edge.
    let mut g = spec_of(r#"{"nodes": [{"id": "a"}], "edges": []}"#, "g.json");
    let mut e = spec_of(
        r#"{"nodes": [{"id": "a"}, {"id": "b"}], "edges": [{"from": "a", "to": "b"}]}"#,
        "g.json",
    )
    .edges
    .remove(0);
    e.to = "zz".into();
    g.edges.push(e);
    let err = GraphModel::build(g).expect_err("an unknown end");
    assert_eq!(err.msg, "unknown edge target: zz");
    assert_eq!(err.kind, die_fig::error::ErrorKind::Mismatch);
}

#[test]
fn a_table_with_an_id_and_a_parent_column_is_a_tree() {
    let org = fixture("org.csv");
    assert_eq!(org.len(), 39);
    assert_eq!(org.ends.len(), 38);
    assert_eq!(org.label(org.index_of("2").expect("Ravi")), "Ravi");
    assert_eq!(org.groups[0], "exec");
    // The card's fields: the parent by its name, no name twice.
    let ravi = org.fields(org.index_of("2").expect("Ravi"));
    assert_eq!(
        ravi,
        [
            ("parent".to_string(), "Maya".to_string()),
            ("role".to_string(), "VP engineering".to_string())
        ]
    );
    let bad = spec::parse("id,parent\n1,\n2,9\n", "t.csv", Path::new("."), &opt())
        .expect_err("an unknown parent");
    assert_eq!(bad.msg, "unknown parent: 9");
    assert_eq!(bad.at.map(|p| p.line), Some(3));
    let twice = spec::parse("id,parent\n1,\n1,\n", "t.csv", Path::new("."), &opt())
        .expect_err("an id twice");
    assert!(
        twice.msg.starts_with("node ids must be unique"),
        "{}",
        twice.msg
    );
    // Without a parent column a table is a chart, as before.
    let chart =
        spec::parse("id,value\n1,3\n2,4\n", "t.csv", Path::new("."), &opt()).expect("parses");
    assert!(matches!(chart.figure, Figure::Chart(_)));
}

#[test]
fn the_path_is_the_shortest_either_way() {
    let m = fixture("services.json");
    let at = |id: &str| m.index_of(id).expect(id);
    let p = m.path(at("web"), at("analytics")).expect("a path");
    assert_eq!(p.first(), Some(&at("web")));
    assert_eq!(p.last(), Some(&at("analytics")));
    assert_eq!(p.len() - 1, 4);
    assert_eq!(m.path_edges(&p).len(), 4);
    assert_eq!(m.path(at("db"), at("db")), Some(vec![at("db")]));
    let apart = model(r#"{"nodes": [{"id": "a"}, {"id": "b"}]}"#);
    assert_eq!(apart.path(0, 1), None);
}

#[test]
fn emphasis_focuses_relates_and_dims_and_fig_colours_opt_out() {
    let m = fixture("services.json");
    let w = m.index_of("worker").expect("worker");
    let (nodes, edges) = m.emphasis(Some(w));
    assert_eq!(nodes[w], Emphasis::Focused);
    for j in m.neighbours(w) {
        assert_eq!(nodes[j], Emphasis::Related);
    }
    let cdn = m.index_of("cdn").expect("cdn");
    assert_eq!(nodes[cdn], Emphasis::Dimmed);
    for (k, &(a, b)) in m.ends.iter().enumerate() {
        let touches = a as usize == w || b as usize == w;
        assert_eq!(edges[k] == Emphasis::Related, touches);
    }
    // fig's rule: a colour or override-focus keeps its look.
    let f = fixture("fig-graph.json");
    let (nodes, edges) = f.emphasis(f.index_of("cache"));
    for id in ["database", "billing", "notify", "auth"] {
        assert_eq!(nodes[f.index_of(id).expect(id)], Emphasis::Normal, "{id}");
    }
    assert_eq!(
        nodes[f.index_of("ledger").expect("ledger")],
        Emphasis::Dimmed
    );
    let red = f
        .spec
        .edges
        .iter()
        .position(|e| e.from == "api" && e.to == "billing");
    assert_eq!(edges[red.expect("the red edge")], Emphasis::Normal);
    // A focused coloured node is focused all the same.
    let (nodes, _) = f.emphasis(f.index_of("billing"));
    assert_eq!(
        nodes[f.index_of("billing").expect("billing")],
        Emphasis::Focused
    );
}

#[test]
fn the_tables_list_nodes_then_edges_and_the_cursors_line() {
    let m = fixture("services.json");
    let t = m.tsv(&View::default(), Some(&Cursor::Node("db".into())));
    let head: Vec<&str> = t
        .nodes
        .lines()
        .next()
        .expect("a header")
        .split('\t')
        .collect();
    assert_eq!(&head[..6], ["id", "label", "group", "degree", "in", "out"]);
    assert!(head.contains(&"port") && head.contains(&"owner"));
    assert_eq!(t.nodes.lines().count(), m.len() + 1);
    let line = t.line.expect("the cursor's line");
    assert!(
        t.nodes
            .lines()
            .nth(line)
            .expect("its line")
            .starts_with("db\t")
    );
    assert_eq!(t.edges.lines().count(), m.ends.len() + 1);
    assert!(t.edges.starts_with("from\tto\tlabel\tweight\tdirected"));
    // A hidden group's nodes and their edges are left out.
    let mut v = View::default();
    v.hidden.insert(m.group_key(1));
    let t = m.tsv(&v, None);
    assert!(!t.nodes.contains("\tedge\t"));
    assert!(t.edges.lines().count() < m.ends.len() + 1);
}

#[test]
fn names_give_each_nodes_label_and_its_id_where_it_differs() {
    let f = fixture("fig-graph.json");
    let names = f.names();
    assert!(names.contains(&("API gateway".to_string(), Cursor::Node("api".into()))));
    assert!(names.contains(&("api".to_string(), Cursor::Node("api".into()))));
    let s = fixture("services.json");
    assert_eq!(s.names().len(), s.len());
}

// ---------------------------------------------------------------- the look

#[test]
fn no_label_overlaps_another_label_or_a_node() {
    for f in FIXTURES {
        let m = fixture(f);
        for size in [(880.0, 470.0), (400.0, 240.0), (1800.0, 1000.0)] {
            let (_, l) = base(&m, &View::default(), size);
            let labels: Vec<&graph::NodeLabel> = l.labels.iter().flatten().collect();
            assert!(
                !labels.is_empty() || size.0 < 300.0,
                "{f} at {size:?} has no labels"
            );
            for (i, a) in labels.iter().enumerate() {
                for b in &labels[i + 1..] {
                    assert!(
                        !overlaps(&a.rect, &b.rect),
                        "{f} at {size:?}: {} on {}",
                        a.text,
                        b.text
                    );
                }
                assert!(
                    l.bounds.contains(a.rect.x, a.rect.y)
                        && a.rect.right() <= l.bounds.right() + 0.01,
                    "{f}: {} leaves the area",
                    a.text
                );
            }
            for (j, a) in l.labels.iter().enumerate() {
                let Some(a) = a else { continue };
                for (i, g) in l.nodes.iter().enumerate() {
                    let Some(g) = g else { continue };
                    let reach = if i == j { g.r + 1.0 } else { g.r + 2.0 };
                    assert!(
                        !circle_meets(g.at, reach, &a.rect),
                        "{f} at {size:?}: {} on a node",
                        a.text
                    );
                }
            }
        }
    }
}

#[test]
fn labels_cross_no_edge_where_a_free_spot_exists_and_hubs_keep_theirs() {
    // A star: every leaf's label has room; the hub's is placed first.
    let mut nodes = vec![r#"{"id": "hub"}"#.to_string()];
    let mut edges = Vec::new();
    for i in 0..8 {
        nodes.push(format!(r#"{{"id": "leaf{i}"}}"#));
        edges.push(format!(r#"{{"from": "hub", "to": "leaf{i}"}}"#));
    }
    let m = model(&format!(
        r#"{{"layout": "force", "nodes": [{}], "edges": [{}]}}"#,
        nodes.join(","),
        edges.join(",")
    ));
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    assert!(l.labels.iter().all(Option::is_some));
    // Each leaf has one edge and room beside it; the hub's edges go every way.
    assert!(l.labels.iter().skip(1).flatten().all(|x| !x.backed));
    // On big.json at fit there isn't room for all: the hubs keep theirs before the leaves.
    let big = fixture("big.json");
    let (_, l) = base(&big, &View::default(), (880.0, 470.0));
    let shown = l.labels.iter().flatten().count();
    assert!(shown > 10 && shown < big.len());
}

#[test]
fn every_fixture_draws_at_three_sizes_dark_and_light() {
    for f in FIXTURES {
        let m = fixture(f);
        for theme in [Theme::mono_dark(), Theme::light()] {
            for size in [(200.0, 120.0), (880.0, 470.0), (1800.0, 1000.0)] {
                let (scene, l) =
                    graph::base(&m, &View::default(), size, 2.0, &theme, &mut FixedMeasure);
                assert!(scene.items.len() > m.len(), "{f} at {size:?}");
                assert!(l.region.h > 0.0 && l.plot.w > 0.0);
                // Every node drawn is inside the area at fit.
                for g in l.nodes.iter().flatten() {
                    assert!(
                        g.at.0 >= 0.0
                            && g.at.0 <= size.0
                            && g.at.1 >= l.region.y
                            && g.at.1 <= size.1,
                        "{f} at {size:?}: a node at {:?}",
                        g.at
                    );
                }
                let st = OverlayState {
                    cursor: Some(m.cursor(0)),
                    cursor_card: true,
                    ..Default::default()
                };
                let over = graph::overlay(&m, &l, &View::default(), &st, &theme, &mut FixedMeasure);
                let png = die_fig::raster::png(&scene, theme.surface, None).expect("a png");
                assert!(png.len() > 100);
                let png = die_fig::raster::png(&over, theme.surface, None).expect("a png");
                assert!(png.len() > 100);
            }
        }
    }
}

#[test]
fn the_card_never_covers_its_node_and_stays_in_the_area() {
    for f in FIXTURES {
        let m = fixture(f);
        for size in [(880.0, 470.0), (400.0, 240.0)] {
            let (_, l) = base(&m, &View::default(), size);
            let step = (m.len() / 40).max(1);
            for i in (0..m.len()).step_by(step) {
                let Some(g) = l.node(i) else { continue };
                if !l.region.contains(g.at.0, g.at.1) {
                    continue;
                }
                for focus in [false, true] {
                    let st = OverlayState {
                        hover: Some(Target::Node(i as u32)),
                        pointer: Some(g.at),
                        emphasis: focus.then(|| m.cursor(i)),
                        ..Default::default()
                    };
                    let over = graph::overlay(
                        &m,
                        &l,
                        &View::default(),
                        &st,
                        &Theme::mono_dark(),
                        &mut FixedMeasure,
                    );
                    let card = card_rect(&over).expect("a card");
                    assert!(
                        !circle_meets(g.at, g.r + 1.0, &card),
                        "{f} {size:?}: the card covers {}",
                        m.id(i)
                    );
                    assert!(
                        card.x >= 0.0
                            && card.y >= 0.0
                            && card.right() <= size.0 + 0.5
                            && card.bottom() <= size.1 + 0.5,
                        "{f} {size:?}: the card leaves the area at {}",
                        m.id(i)
                    );
                }
            }
        }
    }
}

#[test]
fn overlay_labels_never_overlap_each_other() {
    for (f, zoom, id) in [
        ("big.json", 3.0, "auth-01"),
        ("services.json", 1.0, "worker"),
        ("big.json", 1.0, "media-01"),
        ("pipeline.json", 2.0, "features"),
    ] {
        let m = fixture(f);
        let (_, l) = base(&m, &View::default(), (880.0, 470.0));
        let cursor = Cursor::Node(id.into());
        let v = nav::zoom_about(&m, &l, &View::default(), zoom, None, Some(&cursor));
        let (_, l) = base(&m, &v, (880.0, 470.0));
        let st = OverlayState {
            cursor: Some(cursor),
            ..Default::default()
        };
        let over = graph::overlay(&m, &l, &v, &st, &Theme::mono_dark(), &mut FixedMeasure);
        let mut boxes: Vec<(String, Rect)> = Vec::new();
        for item in &over.items {
            if let Item::Text {
                text,
                x,
                y,
                spec,
                anchor,
                ..
            } = item
            {
                let r = die_fig::plot::layout::text_box(
                    text,
                    *x,
                    *y,
                    *spec,
                    *anchor,
                    &mut FixedMeasure,
                );
                let r = Rect::new(r.x, r.y + r.h * 0.2, r.w, r.h * 0.75);
                for (other, o) in &boxes {
                    assert!(!overlaps(&r, o), "{f} at {zoom}: {text} on {other}");
                }
                boxes.push((text.to_string(), r));
            }
        }
        assert!(boxes.len() > 2, "{f}: its labels show");
    }
}

// ---------------------------------------------------------------- hover and nav

#[test]
fn hover_finds_a_node_then_an_edge_and_nothing_off_the_graph() {
    let m = fixture("services.json");
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    let w = m.index_of("worker").expect("worker");
    let g = *l.node(w).expect("drawn");
    assert_eq!(
        graph::hover(&m, &l, g.at.0, g.at.1),
        Some(Target::Node(w as u32))
    );
    assert_eq!(
        graph::hover(&m, &l, g.at.0 + g.r + 3.0, g.at.1),
        Some(Target::Node(w as u32))
    );
    // The middle of an edge with no node near.
    let (k, e) = l
        .edges
        .iter()
        .enumerate()
        .filter_map(|(k, e)| e.as_ref().map(|e| (k, e)))
        .max_by(|a, b| {
            let len = |e: &graph::EdgeGeom| {
                let (p, q) = (e.pts[0], *e.pts.last().expect("a point"));
                (p.0 - q.0).hypot(p.1 - q.1)
            };
            len(a.1).total_cmp(&len(b.1))
        })
        .expect("an edge");
    let (p, q) = (e.pts[0], *e.pts.last().expect("a point"));
    let mid = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
    assert_eq!(
        graph::hover(&m, &l, mid.0, mid.1 + 2.0),
        Some(Target::Edge(k as u32))
    );
    assert_eq!(graph::hover(&m, &l, 5.0, 5.0), None);
    // A click on an edge puts the cursor on its nearer end.
    let (a, _) = m.ends[k];
    let near_a = (p.0 * 0.8 + q.0 * 0.2, p.1 * 0.8 + q.1 * 0.2);
    assert_eq!(
        Target::Edge(k as u32).cursor(&m, &l, near_a),
        Some(m.cursor(a as usize))
    );
}

#[test]
fn steps_go_to_the_nearest_node_in_a_cone() {
    let m = model(
        r#"{"layout": "force", "nodes": [{"id": "c"}, {"id": "r"}, {"id": "l"}, {"id": "u"}, {"id": "d"}, {"id": "far"}],
            "edges": [{"from": "c", "to": "r"}, {"from": "c", "to": "l"}, {"from": "c", "to": "u"},
            {"from": "c", "to": "d"}, {"from": "r", "to": "far"}]}"#,
    );
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    let at = |i: usize| l.node(i).expect("drawn").at;
    let c = 0;
    for dir in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
        if let Some(j) = nav::step(&m, &l, c, dir) {
            let (dx, dy) = (at(j).0 - at(c).0, at(j).1 - at(c).1);
            let (along, across) = match dir {
                Dir::Left => (-dx, dy.abs()),
                Dir::Right => (dx, dy.abs()),
                Dir::Up => (-dy, dx.abs()),
                Dir::Down => (dy, dx.abs()),
            };
            assert!(
                along > 0.0 && across <= along,
                "{dir:?} went to {}",
                m.id(j)
            );
            // Nothing in the cone scores better.
            for (i, g) in l.nodes.iter().enumerate() {
                let Some(g) = g else { continue };
                if i == c || i == j {
                    continue;
                }
                let (ex, ey) = (g.at.0 - at(c).0, g.at.1 - at(c).1);
                let (a2, c2) = match dir {
                    Dir::Left => (-ex, ey.abs()),
                    Dir::Right => (ex, ey.abs()),
                    Dir::Up => (-ey, ex.abs()),
                    Dir::Down => (ey, ex.abs()),
                };
                if a2 > 0.0 && c2 <= a2 {
                    assert!(a2 + 2.0 * c2 >= along + 2.0 * across);
                }
            }
        }
    }
    // A first move with no cursor lands on the node nearest the middle; Step and Series move.
    let first = nav::motion(&m, &l, None, &Action::Step(1), 1).expect("placed");
    assert_eq!(
        Some(first.clone()),
        nav::nearest_middle(&l).map(|i| m.cursor(i))
    );
    assert_eq!(
        nav::motion(&m, &l, None, &Action::First, 1),
        Some(m.cursor(0))
    );
    assert_eq!(
        nav::motion(&m, &l, None, &Action::Last, 1),
        Some(m.cursor(5))
    );
}

#[test]
fn neighbours_turn_clockwise_from_up_and_wrap() {
    let m = fixture("services.json");
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    let w = m.index_of("worker").expect("worker");
    let c = l.node(w).expect("drawn").at;
    let bearing = |j: usize| {
        let p = l.node(j).expect("drawn").at;
        let a = (p.0 - c.0).atan2(-(p.1 - c.1));
        if a < 0.0 {
            a + std::f32::consts::TAU
        } else {
            a
        }
    };
    let mut seen = Vec::new();
    let mut at = None;
    for _ in 0..m.neighbours(w).len() {
        at = nav::neighbour(&m, &l, w, at, 1);
        seen.push(at.expect("a neighbour"));
    }
    let mut sorted = m.neighbours(w);
    sorted.sort_by(|a, b| bearing(*a).total_cmp(&bearing(*b)));
    assert_eq!(seen, sorted);
    // Round the end, and back.
    assert_eq!(nav::neighbour(&m, &l, w, at, 1), Some(sorted[0]));
    assert_eq!(nav::neighbour(&m, &l, w, Some(sorted[0]), -1), at);
    assert_eq!(nav::neighbour(&m, &l, w, None, -1), sorted.last().copied());
}

#[test]
fn zoom_pan_fit_reveal_and_centre_keep_their_limits() {
    let m = fixture("services.json");
    let v0 = View::default();
    let (_, l) = base(&m, &v0, (880.0, 470.0));
    let p = l.plot;
    let at = (p.x + p.w * 0.3, p.y + p.h * 0.6);
    let before = l.frame.unit(at);
    let v = nav::zoom_about(&m, &l, &v0, 2.0, Some(at), None);
    let (_, l2) = base(&m, &v, (880.0, 470.0));
    let after = l2.frame.unit(at);
    assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
    assert!((l2.frame.k.0 / l.frame.k.0 - 2.0).abs() < 1e-6);
    // Within 32× in and 2× out.
    let deep = nav::zoom_about(&m, &l, &v0, 1e6, Some(at), None);
    let (_, ld) = base(&m, &deep, (880.0, 470.0));
    assert!((ld.frame.k.0 / l.frame.k.0 - nav::MAX_ZOOM).abs() < 1e-3);
    let out = nav::zoom_about(&m, &l, &v0, 1e-6, None, None);
    let (_, lo) = base(&m, &out, (880.0, 470.0));
    assert!((l.frame.k.0 / lo.frame.k.0 - nav::MIN_ZOOM).abs() < 1e-3);
    // A pan moves the content with the hand, and stops with the window's middle over the graph.
    let w = m.index_of("worker").expect("worker");
    let panned = nav::pan(&m, &l2, &v, 30.0, -20.0);
    let (_, l3) = base(&m, &panned, (880.0, 470.0));
    let (g2, g3) = (l2.node(w).expect("drawn").at, l3.node(w).expect("drawn").at);
    assert!((g3.0 - g2.0 - 30.0).abs() < 1e-3 && (g3.1 - g2.1 + 20.0).abs() < 1e-3);
    let gone = nav::pan(&m, &l2, &v, 1e6, 1e6);
    let (fx, fy) = l2.fit;
    let mid = |d: Option<(f64, f64)>| d.map(|(a, b)| (a + b) / 2.0).expect("set");
    assert!(mid(gone.x) >= fx.0.min(fx.1) - 1e-9 && mid(gone.x) <= fx.0.max(fx.1) + 1e-9);
    assert!(mid(gone.y) >= fy.0.min(fy.1) - 1e-9 && mid(gone.y) <= fy.0.max(fy.1) + 1e-9);
    // Fit forgets the window; reveal leaves a node in view alone and recentres one out of it.
    assert_eq!(nav::fit(&v).x, None);
    let c = m.cursor(w);
    assert_eq!(nav::reveal(&m, &l2, &v, &c), v);
    let far = nav::pan(&m, &l2, &v, -400.0, 0.0);
    let (_, l4) = base(&m, &far, (880.0, 470.0));
    if !l4.plot.contains(
        l4.node(w).expect("drawn").at.0,
        l4.node(w).expect("drawn").at.1,
    ) {
        let back = nav::reveal(&m, &l4, &far, &c);
        let (_, l5) = base(&m, &back, (880.0, 470.0));
        let g = l5.node(w).expect("drawn").at;
        assert!((g.0 - (p.x + p.w / 2.0)).abs() < 1.0 && (g.1 - (p.y + p.h / 2.0)).abs() < 1.0);
        // The same zoom.
        assert!((l5.frame.k.0 - l4.frame.k.0).abs() < 1e-9);
    }
    let centred = nav::centre(&m, &l2, &v, &c);
    let (_, l6) = base(&m, &centred, (880.0, 470.0));
    let g = l6.node(w).expect("drawn").at;
    assert!((g.0 - (p.x + p.w / 2.0)).abs() < 1.0);
}

#[test]
fn a_drag_keeps_the_grab_pins_the_node_and_moves_nothing_else() {
    let m = fixture("services.json");
    let v0 = View::default();
    let (_, l) = base(&m, &v0, (880.0, 470.0));
    let w = m.index_of("worker").expect("worker");
    let g = *l.node(w).expect("drawn");
    let grab = nav::grab(&m, &l, "worker", (g.at.0 + 2.0, g.at.1 - 1.0));
    assert_eq!(grab, (2.0, -1.0));
    let v = nav::drag_node(&m, &l, &v0, "worker", (300.0, 300.0), grab);
    assert!(m.pinned(&v, w));
    let (_, l2) = base(&m, &v, (880.0, 470.0));
    let at = l2.node(w).expect("drawn").at;
    assert!((at.0 - 298.0).abs() < 1e-3 && (at.1 - 301.0).abs() < 1e-3);
    for i in (0..m.len()).filter(|&i| i != w) {
        let (a, b) = (l.node(i).expect("drawn").at, l2.node(i).expect("drawn").at);
        assert!(
            (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3,
            "{} moved",
            m.id(i)
        );
    }
    // Unpinned, it goes back; a tween glides it.
    let back = nav::unpin(&v, "worker");
    assert!(!m.pinned(&back, w));
    let half = nav::tween(&m, &v, &back, 0.5);
    let mid = m.position(&half, w);
    let (a, b) = (m.position(&v, w), m.positions[w]);
    assert!((mid.0 - (a.0 + b.0) / 2.0).abs() < 1e-9);
    // In a tree a node slides along its rank only.
    let org = fixture("org.csv");
    let (_, l) = base(&org, &v0, (880.0, 470.0));
    let lena = org.index_of("3").expect("Lena");
    let y0 = l.node(lena).expect("drawn").at.1;
    let v = nav::drag_node(&org, &l, &v0, "3", (500.0, 420.0), (0.0, 0.0));
    let (_, l2) = base(&org, &v, (880.0, 470.0));
    let at = l2.node(lena).expect("drawn").at;
    assert!((at.1 - y0).abs() < 1e-3 && (at.0 - 500.0).abs() < 1e-3);
}

#[test]
fn with_every_group_hidden_the_graph_says_so() {
    let m = fixture("services.json");
    let mut v = View::default();
    v.hidden.extend(m.keys());
    let (scene, l) = base(&m, &v, (880.0, 470.0));
    assert!(l.nodes.iter().all(Option::is_none));
    let said = scene
        .items
        .iter()
        .any(|i| matches!(i, Item::Text { text, .. } if text.contains("hidden")));
    assert!(said);
    // The legend still offers its keys, hollow.
    assert_eq!(l.items.len(), 3);
    assert!(l.items.iter().all(|i| i.hidden));
}

#[test]
fn hidden_groups_take_their_nodes_and_edges() {
    let m = fixture("services.json");
    let mut v = View::default();
    v.hidden.insert(m.group_key(2));
    let (_, l) = base(&m, &v, (880.0, 470.0));
    for i in 0..m.len() {
        assert_eq!(l.node(i).is_some(), m.group_of[i] != Some(2));
    }
    for (k, &(a, b)) in m.ends.iter().enumerate() {
        let hidden = m.group_of[a as usize] == Some(2) || m.group_of[b as usize] == Some(2);
        assert_eq!(l.edges[k].is_some(), !hidden);
    }
}

#[test]
#[ignore = "speed"]
fn the_overlay_repaints_a_300_node_hover_within_its_budget() {
    // The overlay's scene within 2 ms (the design's hover budget, what the graph's code
    // decides); the whole repaint, its damage rasterised at 2×, within the 8 ms charts allow a
    // hover move (tests/pointer.rs): the emphasis wash and the card's shadow cost by the pixel
    // in raster.rs.
    let m = fixture("big.json");
    let theme = Theme::mono_dark();
    let mut book = die_draw::Book::load(None).expect("fonts");
    let (_, l) = {
        let mut bm = die_fig::raster::BookMeasure {
            book: &mut book,
            scale: 2.0,
        };
        graph::base(&m, &View::default(), (880.0, 470.0), 2.0, &theme, &mut bm)
    };
    let mut pixels = vec![0u8; 1760 * 940 * 4];
    let (mut scene_worst, mut worst) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    for (n, i) in (0..m.len()).step_by(15).enumerate() {
        let Some(g) = l.node(i) else { continue };
        let st = OverlayState {
            hover: Some(Target::Node(i as u32)),
            pointer: Some(g.at),
            emphasis: Some(m.cursor(i)),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let over = {
            let mut bm = die_fig::raster::BookMeasure {
                book: &mut book,
                scale: 2.0,
            };
            graph::overlay(&m, &l, &View::default(), &st, &theme, &mut bm)
        };
        let built = t.elapsed();
        let damage = over
            .bounds()
            .map(|r| die_fig::raster::to_physical(r, 2.0, 1760, 940));
        die_fig::raster::draw(
            &over,
            &mut pixels,
            1760,
            940,
            1760 * 4,
            damage,
            None,
            Some(&mut book),
        );
        // The first draws its glyphs into the cache.
        if n > 0 {
            scene_worst = scene_worst.max(built);
            worst = worst.max(t.elapsed());
        }
    }
    eprintln!("the overlay's scene: {scene_worst:?} at worst; with its raster: {worst:?}");
    assert!(
        scene_worst.as_secs_f64() < 0.002,
        "the scene took {scene_worst:?}"
    );
    assert!(worst.as_secs_f64() < 0.008, "the repaint took {worst:?}");
}

#[test]
fn a_path_goes_only_through_what_is_drawn() {
    let m = model(
        r#"{"nodes": [{"id": "a", "group": "x"}, {"id": "b", "group": "y"}, {"id": "c", "group": "x"}, {"id": "d", "group": "x"}],
            "edges": [{"from": "a", "to": "b"}, {"from": "b", "to": "c"}, {"from": "a", "to": "d"}]}"#,
    );
    let mut view = View::default();
    assert_eq!(m.path_shown(&view, 0, 2), Some(vec![0, 1, 2]));
    view.hidden.insert(die_fig::data::SeriesKey::new("y", 0));
    assert_eq!(
        m.path_shown(&view, 0, 2),
        None,
        "b is hidden: no path to see"
    );
    assert_eq!(m.path_shown(&view, 0, 3), Some(vec![0, 3]));
    assert_eq!(m.path(0, 2), Some(vec![0, 1, 2]), "the graph's own path");
    // The card says no hops, and nothing is lit along a path no one can see.
    view.reference = Some(m.cursor(0));
    let (_, l) = base(&m, &view, (600.0, 400.0));
    let st = OverlayState {
        hover: Some(Target::Node(2)),
        pointer: Some((10.0, 10.0)),
        ..OverlayState::default()
    };
    let o = graph::overlay(&m, &l, &view, &st, &Theme::mono_dark(), &mut FixedMeasure);
    let said: Vec<String> = (o.items.iter())
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect();
    assert!(!said.iter().any(|t| t.contains("hop")), "{said:?}");
}

#[test]
fn a_graph_with_no_nodes_is_nothing_to_draw_and_a_twice_given_id_is_named() {
    let bad = |t: &str| spec::parse(t, "g.json", Path::new("."), &opt()).expect_err("an error");
    let e = bad(r#"{"nodes": []}"#);
    assert_eq!(e.kind, die_fig::error::ErrorKind::Empty, "{e:?}");
    let e = bad(r#"{"nodes": [{"id": "a"}, {"id": "a"}]}"#);
    assert_eq!(e.msg, "node ids must be unique: a is given twice");
}

#[test]
fn a_table_nodes_detail_says_its_name_once_and_its_parent_by_label() {
    let m = fixture("org.csv");
    let i = m.index_of("2").unwrap();
    let t = m.node_detail(i);
    let keys: Vec<&str> = t.rows.iter().map(|r| r.key.as_str()).collect();
    assert!(keys.contains(&"label"), "{keys:?}");
    assert!(
        !keys.contains(&"name"),
        "the name column is the label: {keys:?}"
    );
    let parent = t
        .rows
        .iter()
        .find(|r| r.key == "parent")
        .expect("its parent");
    assert_eq!(parent.value, "Maya");
    assert_eq!(
        parent.item,
        Some(m.item(m.index_of("1").unwrap())),
        "it names the node"
    );
    assert_eq!(parent.copy.as_deref(), Some("1"), "y yanks the id");
    // As the card has them.
    let card = m.fields(i);
    assert!(
        card.iter().any(|(k, v)| k == "parent" && v == "Maya"),
        "{card:?}"
    );
}

// ---------------------------------------------------------------- the review's look fixes

/// How far a point is from a rectangle.
fn rect_distance(c: (f32, f32), r: &Rect) -> f32 {
    let nx = c.0.clamp(r.x, r.right());
    let ny = c.1.clamp(r.y, r.bottom());
    (c.0 - nx).hypot(c.1 - ny)
}

/// How far a point is from a polyline.
fn line_distance(c: (f32, f32), pts: &[(f32, f32)]) -> f32 {
    pts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = (dx * dx + dy * dy).max(1e-9);
            let t = (((c.0 - a.0) * dx + (c.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
            (c.0 - a.0 - t * dx).hypot(c.1 - a.1 - t * dy)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The gap from a label's box to a node's ring.
fn gap(l: &GraphLayout, i: usize, r: &Rect) -> f32 {
    let g = l.node(i).unwrap();
    rect_distance(g.at, r) - (g.r + graph::geom::RING + 1.0)
}

#[test]
fn a_label_at_rest_is_clearly_its_own_nodes_and_a_space_from_one_level_with_it() {
    let (margin, share) = graph::label::OWN_MARGIN;
    for f in FIXTURES {
        let m = fixture(f);
        for size in [(880.0, 470.0), (400.0, 240.0)] {
            let (_, l) = base(&m, &View::default(), size);
            let placed: Vec<(usize, &graph::NodeLabel)> = (0..m.len())
                .filter_map(|i| l.labels[i].as_ref().map(|x| (i, x)))
                .collect();
            for &(i, lab) in &placed {
                if m.show_label(i) == Some(true) {
                    continue;
                }
                let mine = gap(&l, i, &lab.rect).max(0.0);
                for j in (0..m.len()).filter(|&j| j != i && l.node(j).is_some()) {
                    assert!(
                        gap(&l, j, &lab.rect) >= mine + margin.max(mine * share) - 0.01,
                        "{f} {size:?}: {} is as near {} as its own",
                        lab.text,
                        m.label(j)
                    );
                }
            }
            for (a, (_, x)) in placed.iter().enumerate() {
                for (_, y) in &placed[a + 1..] {
                    let level = x.rect.y < y.rect.bottom() && y.rect.y < x.rect.bottom();
                    let apart = (y.rect.x - x.rect.right()).max(x.rect.x - y.rect.right());
                    assert!(
                        !level || apart >= 6.99,
                        "{f} {size:?}: {} and {} run together ({apart} px)",
                        x.text,
                        y.text
                    );
                }
            }
        }
    }
}

#[test]
fn a_neighbours_label_in_focus_is_nearer_its_own_node_than_any_bright_one() {
    let (margin, share) = graph::label::OWN_MARGIN;
    let m = fixture("services.json");
    for size in [(880.0, 470.0), (640.0, 470.0)] {
        let (_, l) = base(&m, &View::default(), size);
        for c in 0..m.len() {
            let st = OverlayState {
                cursor: Some(m.cursor(c)),
                ..Default::default()
            };
            let o = graph::overlay(
                &m,
                &l,
                &View::default(),
                &st,
                &Theme::mono_dark(),
                &mut FixedMeasure,
            );
            let bright: Vec<usize> = std::iter::once(c).chain(m.neighbours(c)).collect();
            for item in &o.items {
                let Item::Text {
                    text,
                    x,
                    y,
                    spec,
                    anchor,
                    ..
                } = item
                else {
                    continue;
                };
                let Some(&i) = bright.iter().find(|&&i| m.label(i) == &**text) else {
                    continue;
                };
                if i == c {
                    continue;
                }
                let r = die_fig::plot::layout::text_box(
                    text,
                    *x,
                    *y,
                    *spec,
                    *anchor,
                    &mut FixedMeasure,
                );
                let r = Rect::new(r.x, r.y + r.h * 0.2, r.w, r.h * 0.6);
                let mine = gap(&l, i, &r).max(0.0);
                for &k in bright.iter().filter(|&&k| k != i) {
                    assert!(
                        gap(&l, k, &r) >= mine + margin.max(mine * share) - 2.5,
                        "{size:?}, cursor {}: {text} reads as {}'s",
                        m.id(c),
                        m.label(k)
                    );
                }
            }
        }
    }
}

#[test]
fn an_edge_between_two_nodes_of_one_rank_arcs_clear_of_the_nodes_between() {
    let text = std::fs::read_to_string(dir().join("services.json")).unwrap();
    let m = GraphModel::build_as(
        spec_of(&text, "services.json"),
        Some(Layout::Tree { root: None }),
    )
    .unwrap();
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    let mut level = 0;
    for (k, e) in l.edges.iter().enumerate() {
        let Some(e) = e else { continue };
        let (a, b) = (m.ends[k].0 as usize, m.ends[k].1 as usize);
        let (pa, pb) = (l.node(a).unwrap().at, l.node(b).unwrap().at);
        if (pa.1 - pb.1).abs() >= 1.0 {
            continue;
        }
        level += 1;
        for (i, g) in l.nodes.iter().enumerate() {
            let Some(g) = g else { continue };
            if i == a || i == b {
                continue;
            }
            assert!(
                line_distance(g.at, &e.pts) > g.r + graph::geom::RING,
                "{} – {} runs through {}",
                m.id(a),
                m.id(b),
                m.id(i)
            );
        }
    }
    assert!(
        level >= 3,
        "services.json as a tree has edges along a rank: {level}"
    );
}

#[test]
fn a_self_loop_goes_in_the_widest_gap_between_its_nodes_edges() {
    let m = fixture("cycle.json");
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    let k = (0..m.ends.len())
        .find(|&k| m.ends[k].0 == m.ends[k].1)
        .expect("a self-loop");
    let i = m.ends[k].0 as usize;
    let c = l.node(i).unwrap().at;
    let e = l.edges[k].as_ref().unwrap();
    let far = e.pts.iter().copied().fold(c, |f, p| {
        if (p.0 - c.0).hypot(p.1 - c.1) > (f.0 - c.0).hypot(f.1 - c.1) {
            p
        } else {
            f
        }
    });
    let dir = (far.1 - c.1).atan2(far.0 - c.0);
    for j in m.neighbours(i) {
        let o = l.node(j).unwrap().at;
        let t = (o.1 - c.1).atan2(o.0 - c.0);
        let d = (t - dir).rem_euclid(std::f32::consts::TAU);
        let d = d.min(std::f32::consts::TAU - d);
        assert!(
            d > 0.6,
            "the loop runs along the edge to {} ({d} rad)",
            m.id(j)
        );
    }
}

#[test]
fn the_card_stays_by_its_anchor_drops_rows_before_covering_neighbours_and_clears_cut_labels() {
    // A small area: rows go before the card covers a neighbour.
    let m = fixture("services.json");
    let w = m.index_of("worker").unwrap();
    let (_, l) = base(&m, &View::default(), (400.0, 240.0));
    let st = OverlayState {
        cursor: Some(m.cursor(w)),
        cursor_card: true,
        ..Default::default()
    };
    let o = graph::overlay(
        &m,
        &l,
        &View::default(),
        &st,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    );
    let card = card_rect(&o).expect("a card");
    for j in m.neighbours(w) {
        let g = l.node(j).unwrap();
        assert!(
            !circle_meets(g.at, g.r + 1.0, &card),
            "the card covers {}",
            m.id(j)
        );
    }
    // Every base label the card would cut is cleared whole, under it.
    let theme = Theme::mono_dark();
    for lab in l.labels.iter().flatten() {
        let cut = lab
            .rect
            .intersect(&card)
            .is_some_and(|o| o.w > 0.5 && o.h > 0.5);
        let cleared = o.items.iter().any(|it| {
            matches!(it, Item::Path { path, fill: Some(die_fig::scene::Paint::Solid { color, alpha }), .. }
                if *color == theme.surface && *alpha == 1.0 && path.bounds().is_some_and(|b| b.contains(lab.rect.x + 0.5, lab.rect.y + 0.5) && b.contains(lab.rect.right() - 0.5, lab.rect.bottom() - 0.5)))
        });
        assert!(!cut || cleared, "{} is cut and left", lab.text);
    }
    // In a dense graph zoomed in, it stays by its node rather than flee to the edge.
    let big = fixture("big.json");
    let a = big.index_of("auth-01").unwrap();
    let (_, l0) = base(&big, &View::default(), (880.0, 470.0));
    let v = nav::zoom_about(&big, &l0, &View::default(), 2.0, None, Some(&big.cursor(a)));
    let (_, l) = base(&big, &v, (880.0, 470.0));
    let st = OverlayState {
        cursor: Some(big.cursor(a)),
        cursor_card: true,
        ..Default::default()
    };
    let o = graph::overlay(&big, &l, &v, &st, &Theme::mono_dark(), &mut FixedMeasure);
    let card = card_rect(&o).expect("a card");
    let at = l.node(a).unwrap().at;
    assert!(
        rect_distance(at, &card) < 150.0,
        "{card:?} strays from {at:?}"
    );
    // A hovered edge's card keeps off the edge.
    let org = fixture("org.csv");
    let (_, l) = base(&org, &View::default(), (880.0, 470.0));
    let (x, y) = (
        org.index_of("1").unwrap() as u32,
        org.index_of("2").unwrap() as u32,
    );
    let k = org
        .ends
        .iter()
        .position(|&e| e == (x, y) || e == (y, x))
        .unwrap();
    let e = l.edges[k].as_ref().unwrap();
    let st = OverlayState {
        hover: Some(Target::Edge(k as u32)),
        pointer: Some(e.pts[e.pts.len() / 2]),
        ..Default::default()
    };
    let o = graph::overlay(
        &org,
        &l,
        &View::default(),
        &st,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    );
    let card = card_rect(&o).expect("a card");
    let crosses = e.pts.windows(2).any(|w| {
        (0..=20).any(|t| {
            let t = t as f32 / 20.0;
            card.contains(
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            )
        })
    });
    assert!(!crosses, "the card covers its own edge");
}

#[test]
fn beside_the_panel_a_graph_keeps_the_areas_paddings() {
    let m = fixture("services.json");
    let (_, whole) = base(&m, &View::default(), (880.0, 470.0));
    let (_, beside) = graph::base_beside(
        &m,
        &View::default(),
        (607.0, 470.0),
        880.0,
        1.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    );
    assert_eq!(
        beside.title.x, whole.title.x,
        "the title doesn't move as the panel opens"
    );
}

/// The look pass after phase 3: worker's semibold label once hid billing under its ground, and
/// in a dense cluster covered a neighbour.
#[test]
fn a_focused_label_covers_and_hides_no_node_where_there_is_room() {
    for f in [
        "services.json",
        "fig-graph.json",
        "pipeline.json",
        "cycle.json",
    ] {
        let m = fixture(f);
        let (_, l) = base(&m, &View::default(), (880.0, 470.0));
        for c in 0..m.len() {
            let st = OverlayState {
                cursor: Some(m.cursor(c)),
                ..Default::default()
            };
            let o = graph::overlay(
                &m,
                &l,
                &View::default(),
                &st,
                &Theme::mono_dark(),
                &mut FixedMeasure,
            );
            let label = o.items.iter().find_map(|i| match i {
                Item::Text {
                    text,
                    x,
                    y,
                    spec,
                    anchor,
                    ..
                } if spec.weight == die_fig::scene::Weight::Semibold && **text == *m.label(c) => {
                    Some(die_fig::plot::layout::text_box(
                        text,
                        *x,
                        *y,
                        *spec,
                        *anchor,
                        &mut FixedMeasure,
                    ))
                }
                _ => None,
            });
            let Some(r) = label else { continue };
            let r = Rect::new(r.x, r.y + r.h * 0.2, r.w, r.h * 0.6);
            for (i, g) in l.nodes.iter().enumerate() {
                let Some(g) = g else { continue };
                assert!(
                    i == c || !circle_meets(g.at, g.r + graph::geom::RING * 0.5, &r),
                    "{f}, cursor {}: its label lies over {}",
                    m.id(c),
                    m.id(i)
                );
            }
        }
    }
}

/// The look pass after phase 3: inside a ring only its node shows, so an arrowhead into the
/// cursor's node moves out to touch the ring rather than run under it.
#[test]
fn an_arrowhead_into_the_cursors_node_ends_at_its_ring() {
    use die_fig::graph::overlay::FOCUS_RING;
    use die_fig::scene::Verb;
    let m = fixture("pipeline.json");
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    for c in 0..m.len() {
        let st = OverlayState {
            cursor: Some(m.cursor(c)),
            ..Default::default()
        };
        let o = graph::overlay(
            &m,
            &l,
            &View::default(),
            &st,
            &Theme::mono_dark(),
            &mut FixedMeasure,
        );
        let g = l.node(c).unwrap();
        let ring_out = g.r + FOCUS_RING.0 + FOCUS_RING.1 * 0.5;
        let dist = |p: (f32, f32)| (p.0 - g.at.0).hypot(p.1 - g.at.1);
        // The arrowheads drawn (a filled triangle with no stroke, its tip first), less those a
        // triangle of the surface's colour clears after.
        let tip = |path: &die_fig::scene::PathData| match (path.verbs.len(), path.verbs[0]) {
            (4, Verb::Move(x, y)) if matches!(path.verbs[3], Verb::Close) => Some((x, y)),
            _ => None,
        };
        let cleared: Vec<(f32, f32)> = o
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Path {
                    path,
                    stroke: Some(_),
                    ..
                } => tip(path),
                _ => None,
            })
            .collect();
        let tips: Vec<(f32, f32)> = o
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Path {
                    path,
                    fill: Some(_),
                    stroke: None,
                } => tip(path),
                _ => None,
            })
            .filter(|&p| dist(p) < ring_out + 3.0)
            .filter(|p| {
                !cleared
                    .iter()
                    .any(|q| (q.0 - p.0).abs() + (q.1 - p.1).abs() < 0.01)
            })
            .collect();
        let incoming = (0..m.ends.len())
            .filter(|&k| m.ends[k].1 as usize == c && l.edges[k].is_some())
            .count();
        assert_eq!(tips.len(), incoming, "{}: an arrowhead each", m.id(c));
        for t in tips {
            assert!(
                dist(t) >= ring_out - 0.01,
                "{}: an arrowhead's tip at {} runs under the ring at {ring_out}",
                m.id(c),
                dist(t)
            );
        }
    }
}

/// The look pass after phase 3: the card's edge once sat against a neighbour's label (Ben's
/// card against Priya).
#[test]
fn a_cards_edge_keeps_room_from_a_neighbours_label_it_doesnt_cover() {
    let m = fixture("org.csv");
    let (_, l) = base(&m, &View::default(), (880.0, 470.0));
    for c in 0..m.len() {
        let id = m.id(c);
        let st = OverlayState {
            cursor: Some(m.cursor(c)),
            cursor_card: true,
            ..Default::default()
        };
        let o = graph::overlay(
            &m,
            &l,
            &View::default(),
            &st,
            &Theme::mono_dark(),
            &mut FixedMeasure,
        );
        let card = card_rect(&o).expect("the cursor's card");
        // The neighbours' labels, at full strength beside it.
        let (nodes, _) = m.emphasis(Some(c));
        let lit = (0..m.len()).filter(|&i| nodes[i] == Emphasis::Related);
        for lab in lit.filter_map(|i| l.labels[i].as_ref()) {
            let r = lab.rect;
            let apart = (r.x - card.right())
                .max(card.x - r.right())
                .max(r.y - card.bottom())
                .max(card.y - r.bottom());
            assert!(
                overlaps(&r, &card) || apart >= 7.5,
                "{id}: the card comes {apart} px from {}",
                lab.text
            );
        }
    }
}

/// The look pass after phase 4: an edge's card that cut billing's label left billing unnamed
/// beside it. A label the card cuts, its node in view, moves beside its node clear of the card.
#[test]
fn a_label_the_card_cuts_moves_beside_its_node_clear_of_the_card() {
    let mut m = FixedMeasure;
    let mut moved = 0;
    for f in ["services.json", "pipeline.json", "org.csv"] {
        let g = fixture(f);
        let (_, l) = base(&g, &View::default(), (880.0, 470.0));
        for (k, e) in l.edges.iter().enumerate() {
            let Some(e) = e else { continue };
            let st = OverlayState {
                hover: Some(Target::Edge(k as u32)),
                pointer: Some(e.pts[e.pts.len() / 2]),
                ..Default::default()
            };
            let o = graph::overlay(&g, &l, &View::default(), &st, &Theme::mono_dark(), &mut m);
            let card = card_rect(&o).expect("a card");
            let shown: Vec<(String, Rect)> = o
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Text {
                        text,
                        x,
                        y,
                        spec,
                        anchor,
                        ..
                    } => {
                        let w = m.width(text, *spec);
                        let left = match anchor {
                            die_fig::scene::Anchor::Start => *x,
                            die_fig::scene::Anchor::Middle => x - w / 2.0,
                            die_fig::scene::Anchor::End => x - w,
                        };
                        let a = m.cap(*spec);
                        Some((text.to_string(), Rect::new(left, y - a, w, a)))
                    }
                    _ => None,
                })
                .collect();
            for (i, lab) in l.labels.iter().enumerate() {
                let Some(lab) = lab else { continue };
                let n = l.node(i).expect("a labelled node is drawn");
                if !overlaps(&lab.rect, &card) || card.inflate(-1.0).contains(n.at.0, n.at.1) {
                    continue;
                }
                for (_, b) in shown.iter().filter(|(t, _)| *t == lab.text) {
                    assert!(
                        !overlaps(b, &card),
                        "{f}: {:?} moved under the card",
                        lab.text
                    );
                    let near = (b.x + b.w / 2.0 - n.at.0).hypot(b.y + b.h / 2.0 - n.at.1);
                    assert!(
                        near < 90.0,
                        "{f}: {:?} strays {near} from its node",
                        lab.text
                    );
                    moved += 1;
                }
            }
        }
    }
    assert!(moved > 0, "some card cuts a label, which moves");
}
