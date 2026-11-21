//! The detail panel: a tree for each kind of item (a chart's point, series, band and cell, a
//! graph's node and edge), folding, the panel's keys and what they yank, where the panel goes,
//! its rows inside their rectangle and clear of each other in dark and light, and the table
//! views' lines. Over the synthetic fixtures in `examples/data` and specs and facts made here;
//! measured with `FixedMeasure`. Times are in UTC.

use die_test_cap as _;

use die_fig::data::{ReadOptions, SeriesKey};
use die_fig::detail::{
    self, DetailRow, DetailTree, Dock, EdgeFacts, Item, Neighbour, NodeFacts, PanelAction,
    PanelKey, PanelState, RowKind, Swatch,
};
use die_fig::interact::{Cursor, View};
use die_fig::json;
use die_fig::plot::layout::text_box;
use die_fig::plot::{self, ChartModel};
use die_fig::scene::{FixedMeasure, Hit, Item as Draw, Rect, Scene};
use die_fig::spec::{self, Figure};
use die_fig::style::ColorSpec;
use die_fig::theme::Theme;
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("data")
}

fn opt() -> ReadOptions {
    ReadOptions::new(TimeZone::UTC)
}

/// A fixture's model, read as `\v` reads it.
fn load(name: &str) -> ChartModel {
    let dir = data_dir();
    let text = std::fs::read_to_string(dir.join(name)).expect("the fixture");
    model_of(&text, name)
}

/// A model from spec text whose data is inline.
fn model_of(text: &str, name: &str) -> ChartModel {
    let parsed = spec::parse(text, name, &data_dir(), &opt()).expect("it parses");
    let Figure::Chart(chart) = parsed.figure else {
        panic!("a chart");
    };
    let tables = plot::read_tables(&chart, Some(text), &opt()).expect("its data");
    ChartModel::build(chart, tables, &TimeZone::UTC).expect("its model")
}

fn key(name: &str) -> SeriesKey {
    SeriesKey::new(name, 0)
}

/// The rows as (kind, depth, key, value), for asserting a tree whole.
fn rows(t: &DetailTree) -> Vec<(RowKind, u16, String, String)> {
    t.rows
        .iter()
        .map(|r| (r.kind, r.depth, r.key.clone(), r.value.clone()))
        .collect()
}

fn row<'a>(t: &'a DetailTree, k: &str) -> &'a DetailRow {
    t.rows
        .iter()
        .find(|r| r.key == k)
        .unwrap_or_else(|| panic!("no row {k:?} in {:#?}", rows(t)))
}

const DEPLOYS: &str = r#"{"type": "bar", "data": {"values": [
    {"time": "2026-11-21T06:00", "deploys": 3, "who": "ana", "fig": {"color": "red"}},
    {"time": "2026-11-21T07:00", "deploys": 5, "who": "bo", "meta": {"sha": "9f2c", "files": [1, 2]}},
    {"time": "2026-11-21T08:00", "deploys": 2, "who": null}
  ]}, "x": "time", "y": "deploys"}"#;

const HEAT: &str = r#"{"type": "heatmap", "data": {"values": [{"day": "Mon", "hour": "6am", "load": 90.6},
    {"day": "Mon", "hour": "8am", "load": 18.2}, {"day": "Tue", "hour": "6am", "load": 80.4},
    {"day": "Tue", "hour": "8am", "load": 51}, {"day": "Wed", "hour": "8am", "load": 7}]},
    "x": "hour", "y": "day", "color": "load"}"#;

// ---------------------------------------------------------------- chart trees

#[test]
fn a_point_tree_has_its_x_each_series_value_naming_its_sample_in_legend_order() {
    let model = load("requests.csv");
    let cursor = Cursor::Point {
        series: key("api"),
        row: 53,
    };
    let t = model.detail(&View::default(), &cursor).expect("a tree");
    assert_eq!(t.title, "10:25 · Sat 21 Nov");
    assert_eq!(t.kind, "api · point 54 of 73");
    assert_eq!(t.item, Item::of_cursor(&cursor));
    assert_eq!(t.swatch.map(|s| s.color), Some(ColorSpec::Slot(0)));
    let r = rows(&t);
    assert_eq!(
        r[0],
        (RowKind::Field, 0, "time".into(), "2026-11-21 10:25".into())
    );
    assert_eq!(r[1].0, RowKind::Section);
    let names: Vec<&str> = r[2..].iter().map(|x| x.2.as_str()).collect();
    assert_eq!(names, ["api", "worker", "batch"]);
    for (i, name) in ["api", "worker", "batch"].iter().enumerate() {
        let si = model.index_of(&key(name)).unwrap();
        let s = &model.series[si];
        assert_eq!(t.rows[2 + i].value, model.value_label(s.ys[53], s.decimals));
        assert_eq!(
            t.rows[2 + i].item,
            Some(Item::Point {
                series: key(name),
                row: 53
            })
        );
        assert!(matches!(t.rows[2 + i].kind, RowKind::Mark(Some(_))));
    }
    // The panel opens on the cursor's own series.
    assert_eq!(PanelState::open(&t, t.item.as_ref()).row, 2);
    // A hidden series is left out.
    let mut view = View::default();
    view.hidden.insert(key("worker"));
    let t = model.detail(&view, &cursor).unwrap();
    assert!(t.rows.iter().all(|r| r.key != "worker"));
}

#[test]
fn a_point_tree_shows_its_row_s_other_fields_nested_ones_as_folded_branches() {
    let model = model_of(DEPLOYS, "d.json");
    let t = model
        .detail(
            &View::default(),
            &Cursor::Point {
                series: key("deploys"),
                row: 1,
            },
        )
        .unwrap();
    let r = rows(&t);
    let at = r
        .iter()
        .position(|x| x.0 == RowKind::Section && x.2 == "Row 2")
        .unwrap_or_else(|| panic!("{r:#?}"));
    assert_eq!(
        &r[at + 1..],
        &[
            (RowKind::Field, 0, "who".into(), "bo".into()),
            (RowKind::Branch, 0, "meta".into(), "{2}".into()),
            (RowKind::Field, 1, "sha".into(), "9f2c".into()),
            (RowKind::Branch, 1, "files".into(), "[2]".into()),
            (RowKind::Field, 2, "0".into(), "1".into()),
            (RowKind::Field, 2, "1".into(), "2".into()),
        ]
    );
    assert!(t.rows[at + 2].folded);
    // Folded, the branch's members aren't shown.
    let shown: Vec<&str> = t.lines().iter().map(|&i| t.rows[i].key.as_str()).collect();
    assert!(
        shown.contains(&"meta") && !shown.contains(&"sha"),
        "{shown:?}"
    );
    // Its `y` is the object's JSON.
    assert_eq!(t.rows[at + 2].yank(), r#"{"sha":"9f2c","files":[1,2]}"#);
    // A null is quiet; the x and y fields and the fig block aren't repeated.
    let t0 = model
        .detail(
            &View::default(),
            &Cursor::Point {
                series: key("deploys"),
                row: 2,
            },
        )
        .unwrap();
    assert!(row(&t0, "who").quiet);
    assert_eq!(row(&t0, "who").value, "null");
    let at = t0.rows.iter().position(|r| r.key == "Row 3").unwrap();
    assert!(
        t0.rows[at..]
            .iter()
            .all(|r| !["time", "deploys", "fig"].contains(&r.key.as_str())),
        "{:#?}",
        rows(&t0)
    );
}

#[test]
fn a_scatter_point_tree_names_its_series_and_has_its_x_and_y() {
    let model = load("scatter.csv");
    let k = model.keys()[0].clone();
    let t = model
        .detail(
            &View::default(),
            &Cursor::Point {
                series: k.clone(),
                row: 0,
            },
        )
        .unwrap();
    assert_eq!(t.title, model.display_name(&k));
    assert!(t.kind.starts_with("point 1 of "), "{}", t.kind);
    let r = rows(&t);
    assert_eq!(r[0].2, "cpu");
    assert_eq!(r[1].2, "memory");
}

#[test]
fn a_series_tree_has_count_min_max_mean_last_and_its_fig_block() {
    let model = model_of(
        r#"{"type": "line", "series": [
            {"label": "a", "fig": {"color": "red", "dash": true},
             "points": [{"x": 0, "y": 3}, {"x": 1, "y": 9}, {"x": 2, "y": 1}, {"x": 3, "y": 4}]}]}"#,
        "s.json",
    );
    let t = model.series_detail(&key("a")).expect("a tree");
    assert_eq!(t.title, "a");
    assert_eq!(t.kind, "line series");
    assert_eq!(t.swatch.map(|s| s.color), ColorSpec::parse("red"));
    assert_eq!(row(&t, "count").value, "4");
    assert_eq!(row(&t, "min").value, "1 at 2");
    assert_eq!(
        row(&t, "min").item,
        Some(Item::Point {
            series: key("a"),
            row: 2
        })
    );
    assert_eq!(row(&t, "max").value, "9 at 1");
    assert_eq!(row(&t, "mean").value, model.value_label(4.25, 1));
    assert_eq!(row(&t, "last").value, "4 at 3");
    let fig = t
        .rows
        .iter()
        .position(|r| r.kind == RowKind::Section && r.key == "fig")
        .expect("its fig block");
    assert_eq!(t.rows[fig + 1].key, "color");
    assert_eq!(t.rows[fig + 1].value, "red");
    assert_eq!(t.rows[fig + 2].key, "dash");
    // As an item.
    assert_eq!(
        model.detail_of(&View::default(), &Item::Series(key("a"))),
        Some(t)
    );
}

#[test]
fn a_band_tree_has_its_category_each_part_naming_it_and_the_total() {
    let model = load("latency.json");
    let t = model
        .detail(
            &View::default(),
            &Cursor::Band {
                band: 3,
                series: None,
            },
        )
        .unwrap();
    assert_eq!(t.title, "50–100");
    assert_eq!(t.kind, "bar · 2 series");
    assert_eq!(
        rows(&t),
        [
            (RowKind::Field, 0, "Latency, ms".into(), "50–100".into()),
            (RowKind::Section, 0, "Values".into(), String::new()),
            (
                RowKind::Mark(Some(Swatch {
                    color: ColorSpec::Slot(0),
                    shape: detail::Shape::Square
                })),
                0,
                "success".into(),
                "520".into()
            ),
            (
                RowKind::Mark(Some(Swatch {
                    color: ColorSpec::Slot(1),
                    shape: detail::Shape::Square
                })),
                0,
                "failure".into(),
                "88".into()
            ),
            (RowKind::Sum, 0, "total".into(), "608".into()),
        ]
    );
    assert_eq!(
        t.rows[3].item,
        Some(Item::Band {
            band: 3,
            series: Some(key("failure"))
        })
    );
    // On a part, the panel opens on its row.
    let part = Item::Band {
        band: 3,
        series: Some(key("failure")),
    };
    assert_eq!(PanelState::open(&t, Some(&part)).row, 3);
}

#[test]
fn a_cell_tree_has_its_categories_its_value_and_its_row() {
    let model = model_of(HEAT, "h.json");
    let t = model
        .detail(&View::default(), &Cursor::Cell { row: 1, col: 0 })
        .unwrap();
    assert_eq!(t.title, "6am · Tue");
    assert_eq!(t.kind, "cell");
    let r = rows(&t);
    assert_eq!(r[0], (RowKind::Field, 0, "hour".into(), "6am".into()));
    assert_eq!(r[1], (RowKind::Field, 0, "day".into(), "Tue".into()));
    assert_eq!(r[2], (RowKind::Field, 0, "load".into(), "80.4".into()));
    assert!(
        model
            .detail(&View::default(), &Cursor::Cell { row: 2, col: 0 })
            .is_none()
    );
}

#[test]
fn names_are_series_and_categories_and_put_the_cursor_where_a_search_does() {
    let model = load("services-bars.csv");
    let names = model.names();
    let labels = model.labels();
    assert_eq!(names.len(), labels.len());
    for ((n, item), (l, c)) in names.iter().zip(&labels) {
        assert_eq!(n, l);
        assert_eq!(model.cursor_of(item).as_ref(), Some(c));
        assert!(item.holds(c), "{item:?} holds {c:?}");
    }
    assert!(
        names
            .iter()
            .any(|(_, i)| matches!(i, Item::Band { series: None, .. }))
    );
}

// ---------------------------------------------------------------- graph trees

fn nb(index: u32, id: &str, degree: u32) -> Neighbour {
    Neighbour {
        index,
        id: id.into(),
        name: id.to_uppercase(),
        swatch: Some(Swatch::dot(ColorSpec::Slot(1))),
        degree,
    }
}

const WORKER: &str = r#"{"id": "worker", "group": "platform", "port": 8080, "owner": "platform-team",
    "tags": ["a", "b"], "fig": {"color": "blue"},
    "metadata": {"region": "ap-southeast-2", "deploy": {"image": "worker:1.4", "replicas": 6}}}"#;

fn worker_tree() -> DetailTree {
    let raw = json::parse(WORKER).unwrap();
    detail::node_tree(&NodeFacts {
        index: 8,
        id: "worker",
        label: None,
        group: Some("platform"),
        swatch: Some(Swatch::dot(ColorSpec::Slot(0))),
        degree_in: 2,
        degree_out: 1,
        directed: true,
        neighbours: vec![nb(4, "api", 6), nb(9, "queue", 3), nb(4, "api", 6)],
        fields: Some(&raw),
    })
}

#[test]
fn a_node_tree_has_its_fields_neighbours_naming_them_and_its_metadata() {
    let t = worker_tree();
    assert_eq!(t.title, "worker");
    assert_eq!(t.kind, "platform · degree 3");
    assert_eq!(
        t.item,
        Some(Item::Node {
            index: 8,
            id: "worker".into()
        })
    );
    let r = rows(&t);
    let f = |k: &str, v: &str| (RowKind::Field, 0, k.to_string(), v.to_string());
    assert_eq!(r[0], f("id", "worker"));
    assert_eq!(r[1], f("group", "platform"));
    assert_eq!(r[2], f("degree", "3 · 2 in, 1 out"));
    assert_eq!(r[3], f("port", "8080"));
    assert_eq!(r[4], f("owner", "platform-team"));
    assert_eq!(r[5], (RowKind::Branch, 0, "tags".into(), "[2]".into()));
    let nbs = t
        .rows
        .iter()
        .position(|r| r.kind == RowKind::Section && r.key == "Neighbours")
        .unwrap();
    // A neighbour met twice is listed once.
    assert_eq!(t.rows[nbs + 1].key, "API");
    assert_eq!(t.rows[nbs + 1].value, "6 edges");
    assert!(t.rows[nbs + 1].quiet);
    assert_eq!(t.rows[nbs + 1].item, Some(nb(4, "api", 6).item()));
    assert_eq!(t.rows[nbs + 1].yank(), "api");
    assert_eq!(t.rows[nbs + 2].key, "QUEUE");
    assert_eq!(t.rows[nbs + 3].kind, RowKind::Section);
    assert_eq!(t.rows[nbs + 3].key, "Metadata");
    assert_eq!(
        &r[nbs + 4..nbs + 8],
        &[
            f("region", "ap-southeast-2"),
            (RowKind::Branch, 0, "deploy".into(), "{2}".into()),
            (RowKind::Field, 1, "image".into(), "worker:1.4".into()),
            (RowKind::Field, 1, "replicas".into(), "6".into()),
        ]
    );
    assert_eq!(t.rows[nbs + 8].key, "fig");
    assert_eq!(t.rows[nbs + 9], DetailRow::field("color", "blue"));
    assert_eq!(
        t.row_of(&Item::Node {
            index: 0,
            id: "queue".into()
        }),
        Some(nbs + 2)
    );
}

#[test]
fn an_edge_tree_is_titled_by_its_ends_which_its_rows_name() {
    let raw =
        json::parse(r#"{"from": "api", "to": "worker", "weight": 2.5, "proto": "grpc"}"#).unwrap();
    let e = EdgeFacts {
        index: 3,
        from: nb(4, "api", 6),
        to: nb(8, "worker", 5),
        label: None,
        weight: Some(2.5),
        directed: false,
        fields: Some(&raw),
    };
    let t = detail::edge_tree(&e);
    assert_eq!(t.title, "API — WORKER");
    assert_eq!(t.kind, "edge");
    assert_eq!(
        rows(&t)[..2],
        [
            (RowKind::Field, 0, "weight".into(), "2.5".into()),
            (RowKind::Field, 0, "proto".into(), "grpc".into()),
        ]
    );
    let ends = t.rows.iter().position(|r| r.key == "Ends").unwrap();
    assert_eq!(t.rows[ends + 1].item, Some(e.from.item()));
    assert_eq!(t.rows[ends + 2].item, Some(e.to.item()));
    let directed = detail::edge_tree(&EdgeFacts {
        directed: true,
        label: Some("jobs"),
        ..e
    });
    assert_eq!(directed.title, "API → WORKER");
    assert_eq!(directed.kind, "directed edge · jobs");
    assert_eq!(
        directed.rows[0],
        DetailRow::field("label", "jobs").copying("jobs")
    );
}

// ---------------------------------------------------------------- items

#[test]
fn items_name_marks_and_the_places_a_cursor_goes() {
    let p = Cursor::Point {
        series: key("a"),
        row: 4,
    };
    assert_eq!(
        Item::of_cursor(&p).and_then(|i| i.cursor()),
        Some(p.clone())
    );
    assert!(Item::Series(key("a")).holds(&p));
    assert!(!Item::Series(key("b")).holds(&p));
    let part = Cursor::Band {
        band: 2,
        series: Some(key("a")),
    };
    assert!(
        Item::Band {
            band: 2,
            series: None
        }
        .holds(&part)
    );
    assert!(Item::Series(key("a")).holds(&part));
    let node = Item::Node {
        index: 3,
        id: "db".into(),
    };
    assert!(node.holds(&Cursor::Node("db".into())));
    assert!(node.same(&Item::Node {
        index: 99,
        id: "db".into()
    }));
    assert_eq!(node.cursor(), Some(Cursor::Node("db".into())));
    assert_eq!(Item::of_cursor(&Cursor::Node("db".into())), None);
    let edge = Item::Edge {
        index: 1,
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(edge.cursor(), None);
    assert_eq!(Item::Series(key("a")).series(), Some(&key("a")));
}

// ---------------------------------------------------------------- keys

#[test]
fn j_and_k_step_over_rows_shown_skipping_headings_and_stop_at_the_ends() {
    let mut t = worker_tree();
    let mut st = PanelState::open(&t, None);
    assert_eq!(st.row, 0);
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Up),
        PanelAction::None
    );
    let mut seen = vec![st.row];
    while detail::keys(&mut t, &mut st, PanelKey::Down) == PanelAction::Moved {
        seen.push(st.row);
    }
    assert!(seen.iter().all(|&i| t.rows[i].kind != RowKind::Section));
    // Folded branches' members are stepped over.
    assert!(!seen.contains(&6), "tags' first member is hidden: {seen:?}");
    assert_eq!(
        seen,
        t.lines()
            .into_iter()
            .filter(|&i| t.rows[i].kind != RowKind::Section)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::First),
        PanelAction::Moved
    );
    assert_eq!(st.row, 0);
    detail::keys(&mut t, &mut st, PanelKey::Last);
    assert_eq!(st.row, *seen.last().unwrap());
    assert_eq!(PanelKey::from_name("gg"), Some(PanelKey::First));
    assert_eq!(PanelKey::from_name("<CR>"), Some(PanelKey::Enter));
    assert_eq!(PanelKey::from_name("x"), None);
}

#[test]
fn h_and_l_fold_unfold_go_in_and_come_out() {
    let mut t = worker_tree();
    let deploy = t.rows.iter().position(|r| r.key == "deploy").unwrap();
    let mut st = PanelState {
        row: deploy,
        ..PanelState::default()
    };
    assert!(t.rows[deploy].folded);
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Unfold),
        PanelAction::Folded
    );
    assert!(!t.rows[deploy].folded);
    // l on an open branch goes in.
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Unfold),
        PanelAction::Moved
    );
    assert_eq!(st.row, deploy + 1);
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Down),
        PanelAction::Moved
    );
    assert_eq!(t.rows[st.row].key, "replicas");
    // h on a member goes out to its branch; h on the open branch folds it.
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Fold),
        PanelAction::Moved
    );
    assert_eq!(st.row, deploy);
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Fold),
        PanelAction::Folded
    );
    assert!(t.rows[deploy].folded);
    // ⏎ on a branch folds and unfolds too.
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Enter),
        PanelAction::Folded
    );
    assert!(!t.rows[deploy].folded);
    // A fold that hides the cursor brings it to the branch.
    st.row = deploy + 2;
    assert!(detail::toggle(&mut t, &mut st, deploy));
    assert_eq!(st.row, deploy);
    assert!(!detail::toggle(&mut t, &mut st, 0), "a field doesn't fold");
    t.unfold_all();
    assert!(t.rows.iter().all(|r| !r.folded));
}

#[test]
fn enter_jumps_to_the_mark_a_row_names_and_y_yanks() {
    let mut t = worker_tree();
    let api = t.rows.iter().position(|r| r.key == "API").unwrap();
    let mut st = PanelState {
        row: api,
        ..PanelState::default()
    };
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Enter),
        PanelAction::Jump(nb(4, "api", 6).item())
    );
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Yank),
        PanelAction::Yank("api".into())
    );
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::YankPair),
        PanelAction::Yank("API: api".into())
    );
    st.row = 3;
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::YankPair),
        PanelAction::Yank("port: 8080".into())
    );
    let tags = t.rows.iter().position(|r| r.key == "tags").unwrap();
    st.row = tags;
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Yank),
        PanelAction::Yank(r#"["a","b"]"#.into())
    );
    st.row = 0;
    assert_eq!(
        detail::keys(&mut t, &mut st, PanelKey::Enter),
        PanelAction::None
    );
    // A string put on one line yanks whole.
    let mut out = Vec::new();
    detail::json_rows(
        "note",
        &json::parse("\"two\\nlines\"").unwrap(),
        0,
        &mut out,
    );
    assert_eq!(out[0].value, "two lines");
    assert_eq!(out[0].yank(), "two\nlines");
    // An empty tree takes no key.
    let mut empty = DetailTree::new("x", "y");
    assert_eq!(
        detail::keys(&mut empty, &mut PanelState::default(), PanelKey::Yank),
        PanelAction::None
    );
}

// ---------------------------------------------------------------- where it goes

#[test]
fn the_panel_takes_the_right_third_within_bounds_or_the_bottom_when_narrow() {
    let (fig, p, d) = detail::split((880.0, 470.0));
    assert_eq!(d, Dock::Right);
    assert_eq!(p, Rect::new(607.0, 0.0, 273.0, 470.0));
    assert_eq!(fig, Rect::new(0.0, 0.0, 607.0, 470.0));
    assert_eq!(detail::split((1800.0, 1000.0)).1.w, 420.0);
    assert_eq!(detail::split((600.0, 400.0)).1.w, 260.0);
    let (fig, p, d) = detail::split((500.0, 400.0));
    assert_eq!(d, Dock::Bottom);
    assert_eq!(p, Rect::new(0.0, 240.0, 500.0, 160.0));
    assert_eq!(fig, Rect::new(0.0, 0.0, 500.0, 240.0));
}

#[test]
fn reveal_keeps_the_cursor_s_row_in_view_and_scroll_stays_in_range() {
    let mut t = worker_tree();
    t.unfold_all();
    let rect = Rect::new(0.0, 0.0, 280.0, 190.0);
    let last = t.rows.len() - 1;
    let mut st = PanelState {
        row: last,
        ..PanelState::default()
    };
    detail::reveal(&t, &mut st, rect);
    let l = detail::layout(&t, &st, rect);
    assert!(l.shown.iter().any(|(i, _)| *i == last), "{l:?}");
    assert_eq!(st.top, l.max_top);
    st.row = 0;
    detail::reveal(&t, &mut st, rect);
    assert_eq!(st.top, 0);
    detail::scroll(&t, &mut st, rect, 100);
    assert_eq!(st.top, l.max_top);
    detail::scroll(&t, &mut st, rect, -100);
    assert_eq!(st.top, 0);
    // A heading never shows alone at the foot.
    for top in 0..=l.max_top {
        let l = detail::layout(&t, &PanelState { top, ..st }, rect);
        let (last, _) = *l.shown.last().unwrap();
        assert!(
            t.rows[last].kind != RowKind::Section,
            "a heading at the foot at top {top}"
        );
    }
}

// ---------------------------------------------------------------- the look

/// Every text's box and every region's rectangle, and the theme's series colours.
fn texts(scene: &Scene) -> Vec<(String, Rect, die_fig::theme::Color)> {
    let mut m = FixedMeasure;
    scene
        .items
        .iter()
        .filter_map(|i| match i {
            Draw::Text {
                text,
                x,
                y,
                spec,
                anchor,
                color,
                ..
            } => Some((
                text.to_string(),
                text_box(text, *x, *y, *spec, *anchor, &mut m),
                *color,
            )),
            _ => None,
        })
        .collect()
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() - 0.01
        && b.x < a.right() - 0.01
        && a.y < b.bottom() - 0.01
        && b.y < a.bottom() - 0.01
}

fn inside(r: &Rect, outer: &Rect) -> bool {
    r.x >= outer.x - 0.01
        && r.y >= outer.y - 0.01
        && r.right() <= outer.right() + 0.01
        && r.bottom() <= outer.bottom() + 0.01
}

/// The trees the look is checked on: a point, a band, a long node unfolded, an edge, a series.
fn trees() -> Vec<DetailTree> {
    let requests = load("requests.csv");
    let many = load("many.json");
    let latency = load("latency.json");
    let mut node = worker_tree();
    node.unfold_all();
    let raw = json::parse(r#"{"from": "a", "to": "b", "note": "a very long note that will never fit beside its key in a narrow panel"}"#).unwrap();
    vec![
        requests
            .detail(
                &View::default(),
                &Cursor::Point {
                    series: key("api"),
                    row: 53,
                },
            )
            .unwrap(),
        many.detail(
            &View::default(),
            &Cursor::Point {
                series: key("eu-west"),
                row: 30,
            },
        )
        .unwrap(),
        latency
            .detail(
                &View::default(),
                &Cursor::Band {
                    band: 3,
                    series: None,
                },
            )
            .unwrap(),
        node,
        detail::edge_tree(&EdgeFacts {
            index: 0,
            from: nb(0, "a-service-with-a-very-long-name", 1),
            to: nb(1, "b", 1),
            label: None,
            weight: None,
            directed: true,
            fields: Some(&raw),
        }),
        requests.series_detail(&key("api")).unwrap(),
    ]
}

#[test]
fn panel_rows_never_overlap_nor_leave_the_rect_in_dark_and_light() {
    let mut m = FixedMeasure;
    for theme in [Theme::mono_dark(), Theme::light()] {
        for (w, h) in [(280.0, 470.0), (400.0, 190.0)] {
            for (x, y) in [(0.0, 0.0), (600.0, 0.0), (0.0, 300.0)] {
                let rect = Rect::new(x, y, w, h);
                for t in trees() {
                    let lines = t.lines();
                    for focused in [false, true] {
                        for &row in lines.iter().step_by(3) {
                            let mut st = PanelState {
                                row,
                                focused,
                                ..PanelState::default()
                            };
                            detail::reveal(&t, &mut st, rect);
                            let s = detail::panel(&t, st, rect, 2.0, &theme, &mut m);
                            let at = format!("{} at {rect:?}, row {row}", t.title);
                            let boxes = texts(&s);
                            for (text, b, color) in &boxes {
                                assert!(inside(b, &rect), "{text:?} {b:?} leaves {at}");
                                assert!(
                                    !theme.series.contains(color),
                                    "{text:?} wears a series colour in {at}"
                                );
                            }
                            for (i, (ta, a, _)) in boxes.iter().enumerate() {
                                for (tb, b, _) in &boxes[i + 1..] {
                                    assert!(
                                        !overlap(a, b),
                                        "{ta:?} {a:?} meets {tb:?} {b:?} in {at}"
                                    );
                                }
                            }
                            let rows: Vec<&Rect> = s
                                .regions
                                .iter()
                                .filter(|r| matches!(r.hit, Hit::Detail(_)))
                                .map(|r| &r.rect)
                                .collect();
                            for (i, a) in rows.iter().enumerate() {
                                assert!(inside(a, &rect), "row {a:?} leaves {at}");
                                for b in &rows[i + 1..] {
                                    assert!(!overlap(a, b), "rows {a:?} {b:?} meet in {at}");
                                }
                            }
                            // The cursor's row is shown, and has its region.
                            assert!(
                                s.regions.iter().any(|r| r.hit == Hit::Detail(row as u32)),
                                "row {row} not shown in {at}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_panel_has_a_region_per_row_and_per_fold_triangle_over_its_ground() {
    let t = worker_tree();
    let rect = Rect::new(600.0, 0.0, 280.0, 470.0);
    let st = PanelState::open(&t, None);
    let s = detail::panel(&t, st, rect, 1.0, &Theme::mono_dark(), &mut FixedMeasure);
    assert_eq!(s.regions[0].hit, Hit::Chrome);
    assert_eq!(s.regions[0].rect, rect);
    let l = detail::layout(&t, &st, rect);
    for (i, r) in &l.shown {
        let mid = (r.x + r.w / 2.0, r.bottom() - 11.0);
        if t.rows[*i].kind == RowKind::Section {
            assert_eq!(s.hit(mid.0, mid.1), Some(&Hit::Chrome));
            continue;
        }
        assert_eq!(s.hit(mid.0, mid.1), Some(&Hit::Detail(*i as u32)));
        assert_eq!(l.row_at(mid.0, mid.1), Some(*i));
    }
    let tags = t.rows.iter().position(|r| r.key == "tags").unwrap();
    let fold = s
        .regions
        .iter()
        .find(|r| r.hit == Hit::DetailFold(tags as u32))
        .expect("the triangle's region");
    let c = (
        fold.rect.x + fold.rect.w / 2.0,
        fold.rect.y + fold.rect.h / 2.0,
    );
    assert_eq!(s.hit(c.0, c.1), Some(&Hit::DetailFold(tags as u32)));
    // The figure beside it isn't the panel's.
    assert_eq!(s.hit(100.0, 100.0), None);
}

#[test]
fn a_tall_tree_scrolls_with_a_scrollbar_and_a_short_one_has_none() {
    let mut t = worker_tree();
    t.unfold_all();
    let rect = Rect::new(0.0, 0.0, 280.0, 190.0);
    let l = detail::layout(&t, &PanelState::default(), rect);
    assert!(l.max_top > 0);
    assert!(l.hint.is_none(), "no room for the hint");
    let tall = Rect::new(0.0, 0.0, 280.0, 900.0);
    let l = detail::layout(&t, &PanelState::default(), tall);
    assert_eq!(l.max_top, 0);
    assert!(l.hint.is_some());
    assert_eq!(l.shown.len(), l.lines.len());
}

// ---------------------------------------------------------------- table views

#[test]
fn a_band_chart_s_table_has_every_category_and_the_cursor_s_line() {
    let model = model_of(
        r#"{"type": "bar", "data": {"values": [{"s": "a", "v": 1}, {"s": "b", "v": null},
            {"s": "c", "v": 3}]}, "x": "s", "y": "v"}"#,
        "b.json",
    );
    let (text, line) = model.tsv_at(
        &View::default(),
        Some(&Cursor::Band {
            band: 2,
            series: None,
        }),
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["s\tv", "a\t1", "b\t", "c\t3"]);
    assert_eq!(line, Some(3));
    let latency = load("latency.json");
    let (text, line) = latency.tsv_at(
        &View::default(),
        Some(&Cursor::Band {
            band: 1,
            series: Some(key("failure")),
        }),
    );
    assert_eq!(text.lines().count(), latency.x.bands.len() + 1);
    assert_eq!(line, Some(2));
    assert!(text.lines().nth(2).unwrap().starts_with("10–25\t"));
}

#[test]
fn a_heatmap_s_table_is_a_row_a_category_and_a_column_the_other() {
    let model = model_of(HEAT, "h.json");
    let (text, line) = model.tsv_at(&View::default(), Some(&Cursor::Cell { row: 2, col: 1 }));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            "day\t6am\t8am",
            "Mon\t90.6\t18.2",
            "Tue\t80.4\t51",
            "Wed\t\t7"
        ]
    );
    assert_eq!(line, Some(3));
    assert_eq!(model.tsv(&View::default()).1, None);
}

#[test]
fn an_x_chart_s_and_a_scatter_s_tables_put_the_cursor_on_its_sample() {
    let model = load("requests.csv");
    let (text, line) = model.tsv_at(
        &View::default(),
        Some(&Cursor::Point {
            series: key("batch"),
            row: 53,
        }),
    );
    let line = line.expect("a line");
    assert!(
        text.lines()
            .nth(line)
            .unwrap()
            .starts_with("2026-11-21T10:25\t")
    );
    let scatter = load("scatter.csv");
    let k = scatter.keys()[1].clone();
    let (text, line) = scatter.tsv_at(
        &View::default(),
        Some(&Cursor::Point {
            series: k.clone(),
            row: 2,
        }),
    );
    let line = line.expect("a line");
    let si = scatter.index_of(&k).unwrap();
    assert!(
        text.lines()
            .nth(line)
            .unwrap()
            .ends_with(&format!("\t{}", scatter.series[si].name))
    );
}
