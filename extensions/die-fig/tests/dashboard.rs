//! Dashboards: the model (each panel its own result, sources read once, nested files and
//! borrowed data with their depth and cycles, what needs a yes), the layout (m4's header and
//! rows, tiles at their natural height, wrapping, columns, groups, full size, the scroll),
//! linked time and the chip, the per-panel error figure and the stale badge, hit and hover, the
//! linked moment in the overlay, and the `.fig` layout's shapes read and written back.

use die_test_cap as _;

use die_fig::dashboard::layout::{PanelFigure, share};
use die_fig::dashboard::{
    self, DashHit, DashLayout, DashModel, DashView, LiveState, LiveStatus, NeedsTrust, Origin,
    PanelHover, PanelKind, PanelState, SourceState, Sources, Want,
};
use die_fig::data::ReadOptions;
use die_fig::error::{ErrorKind, FigError, Result};
use die_fig::scene::{FixedMeasure, Hit, Item, Rect, Scene};
use die_fig::spec::{self, DashboardSpec, DataRef, Figure, Panel, Tooltip};
use die_fig::theme::Theme;
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/dash")
}

fn opt() -> ReadOptions {
    ReadOptions::new(TimeZone::UTC)
}

/// A dashboard spec from a fixture.
fn spec_of(name: &str) -> DashboardSpec {
    let text = std::fs::read_to_string(dir().join(name)).expect(name);
    let p = spec::parse(&text, name, &dir(), &opt()).expect("it parses");
    assert_eq!(p.warnings, [], "{name}");
    match p.figure {
        Figure::Dashboard(d) => d,
        other => panic!("{name} is {other:?}"),
    }
}

/// A fixture's model, its files read as the build asks for them.
fn model_of(name: &str) -> DashModel {
    let spec = spec_of(name);
    let file = dir().join(name);
    let (d, zone, o) = (dir(), TimeZone::UTC, opt());
    let resolve = |p: &Path| dashboard::open(p, &o);
    let origin = Origin {
        file: Some(&file),
        dir: &d,
        zone: &zone,
    };
    let first = DashModel::build(&spec, &Sources::new(), &resolve, &origin);
    let sources = dashboard::read(first.wants(), &o);
    DashModel::build(&spec, &sources, &resolve, &origin)
}

fn lay(model: &DashModel, view: &DashView, size: (f32, f32)) -> DashLayout {
    dashboard::layout(
        model,
        view,
        size,
        2.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    )
}

/// A model from spec texts by path, with no files: `files` is what `resolve` finds.
fn model_from(text: &str, files: &[(&str, &str)], sources: &Sources) -> DashModel {
    let specs = PathBuf::from("/specs");
    let parse = |t: &str, name: &str, d: &Path| spec::parse(t, name, d, &opt()).map(|p| p.figure);
    let Figure::Dashboard(spec) = parse(text, "main.fig", &specs).expect("it parses") else {
        panic!("not a dashboard")
    };
    let resolve = |p: &Path| -> Result<Figure> {
        let (_, t) = files
            .iter()
            .find(|(f, _)| Path::new(f) == p)
            .ok_or_else(|| FigError::new(format!("{}: no such file", p.display())))?;
        parse(t, "x.fig", p.parent().unwrap())
    };
    let zone = TimeZone::UTC;
    let main = specs.join("main.fig");
    let origin = Origin {
        file: Some(&main),
        dir: &specs,
        zone: &zone,
    };
    DashModel::build(&spec, sources, &resolve, &origin)
}

/// Seconds at a time of 2026-11-21 in UTC.
fn at(h: i8, m: i8) -> f64 {
    jiff::civil::date(2026, 11, 21)
        .at(h, m, 0, 0)
        .to_zoned(TimeZone::UTC)
        .unwrap()
        .timestamp()
        .as_second() as f64
}

fn failure(model: &DashModel, i: usize) -> &FigError {
    match &model.panels[i].state {
        PanelState::Failed(f) => &f.errors[0],
        _ => panic!("panel {i} drew"),
    }
}

fn texts(scene: &Scene) -> Vec<String> {
    scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn health_builds_every_panel_linked_over_two_reads() {
    let m = model_of("health.fig");
    let kinds: Vec<PanelKind> = m.panels.iter().map(|p| p.kind).collect();
    use PanelKind::{Chart, Tile};
    assert_eq!(kinds, [Tile, Tile, Tile, Tile, Chart, Chart]);
    assert!(m.panels.iter().all(|p| p.ready()), "every panel draws");
    assert_eq!(m.linked(), [0, 1, 2, 3, 4, 5]);
    // Two files, each read once for every panel on it.
    let names: Vec<String> = m.wants().iter().map(Want::name).collect();
    assert_eq!(names, ["requests.csv", "service.csv"]);
    assert_eq!(m.trust(), []);
    assert!(!m.is_live());
    assert_eq!(m.newest(), Some(at(12, 0)));
    assert_eq!(m.extent(), Some((at(0, 0), at(12, 0))));
    let titles: Vec<Option<&str>> = m.panels[4..].iter().map(|p| p.title.as_deref()).collect();
    assert_eq!(
        titles,
        [Some("Requests per minute"), Some("Errors per minute")]
    );
    assert_eq!(m.panels[5].subtitle.as_deref(), Some("all services"));
}

#[test]
fn the_header_and_rows_land_where_m4_has_them() {
    let mut m = model_of("health.fig");
    m.live = Some(LiveStatus::updated(12.0));
    let v = DashView::initial(&m);
    let l = lay(&m, &v, (880.0, 610.0));
    let title = l.header.title.as_ref().unwrap();
    assert_eq!(
        (title.text.as_str(), title.x, title.y),
        ("Service health", 28.0, 38.0)
    );
    let chip = l.header.chip.as_ref().unwrap();
    assert_eq!(chip.text.text, "Last 6 hours");
    assert_eq!((chip.rect.x, chip.rect.y, chip.rect.h), (28.0, 52.0, 24.0));
    let live = l.header.live.as_ref().unwrap();
    let words = live.text.as_ref().unwrap();
    assert_eq!(
        (words.text.as_str(), words.x, words.y),
        ("live · updated 12 s ago", 852.0, 68.0)
    );
    assert_eq!(live.state, LiveState::Live);
    assert_eq!(l.header.bottom, 92.0);
    let frames: Vec<Rect> = l.panels.iter().flatten().map(|p| p.frame).collect();
    // Four tiles 100 px tall from y 92, 12 px apart, as wide as each other.
    for (i, f) in frames[..4].iter().enumerate() {
        assert_eq!((f.y, f.h), (92.0, 100.0), "tile {i}");
        assert!((f.w - frames[0].w).abs() <= 1.0, "tile {i} is {f:?}");
    }
    for w in frames[..4].windows(2) {
        assert_eq!(w[1].x - w[0].right(), 12.0);
    }
    assert_eq!(frames[0].x, 28.0);
    assert_eq!(frames[3].right(), 852.0);
    // The charts 16 px under the tiles, to 16 px over the foot.
    for f in &frames[4..] {
        assert_eq!((f.y, f.bottom()), (208.0, 594.0), "{f:?}");
    }
    assert_eq!(frames[5].x - frames[4].right(), 12.0);
    for (i, a) in frames.iter().enumerate() {
        for b in &frames[i + 1..] {
            assert!(
                a.intersect(b).is_none_or(|r| r.w <= 0.0 || r.h <= 0.0),
                "{a:?} {b:?}"
            );
        }
    }
    assert_eq!((l.max_scroll, l.scrollbar), (0.0, None));
    // The charts' plots line up across their row, and the panels' titles sit 16 px in.
    let plots: Vec<Rect> = l.panels[4..]
        .iter()
        .flatten()
        .map(|p| {
            let (cl, _) = p.chart().unwrap();
            Rect::new(
                cl.plot.x + p.frame.x,
                cl.plot.y + p.frame.y,
                cl.plot.w,
                cl.plot.h,
            )
        })
        .collect();
    assert_eq!(plots[0].y, plots[1].y);
    assert_eq!(plots[0].bottom(), plots[1].bottom());
    let t = l.panel(4).unwrap().title.unwrap();
    assert_eq!(t.x, 28.0 + 16.0);
    // No end labels in a panel unasked: the legend says the same.
    let (cl, _) = l.panel(4).unwrap().chart().unwrap();
    assert!(cl.ends.is_empty());
    assert_eq!(cl.items.len(), 2);
}

#[test]
fn the_live_status_says_less_where_it_would_meet_the_chip() {
    let mut m = model_of("health.fig");
    let v = DashView::initial(&m);
    m.live = Some(LiveStatus::error(130.0));
    let wide = lay(&m, &v, (880.0, 610.0));
    let live = wide.header.live.as_ref().unwrap();
    assert_eq!(live.state, LiveState::Error);
    assert_eq!(live.text.as_ref().unwrap().text, "error · 2 m ago");
    let narrow = lay(&m, &v, (240.0, 400.0));
    let live = narrow.header.live.as_ref().unwrap();
    let chip = narrow.header.chip.as_ref().unwrap();
    assert!(live.rect.x >= chip.rect.right(), "{live:?} {chip:?}");
    assert_eq!(live.text.as_ref().map(|t| t.text.as_str()), Some("error"));
    assert_eq!(LiveStatus::updating().text, "updating…");
    assert_eq!(LiveStatus::paused().state, LiveState::Paused);
    assert_eq!(LiveStatus::updated(0.4).text, "live · updated just now");
}

#[test]
fn linked_panels_share_one_time_and_the_rest_keep_their_own() {
    let m = model_of("health.fig");
    let v = DashView::initial(&m);
    assert_eq!(v.time, Some((at(6, 0), at(12, 0))));
    assert_eq!(v.focused, None);
    // A zoom on one linked chart moves every linked chart's x, as one change of the view.
    let mut zoomed = v.panel(&m, 4);
    zoomed.x = Some((at(9, 0), at(10, 0)));
    let after = v.with_panel(&m, 4, zoomed);
    assert_eq!(after.time, Some((at(9, 0), at(10, 0))));
    assert_eq!(after.panel(&m, 5).x, Some((at(9, 0), at(10, 0))));
    let l = lay(&m, &after, (880.0, 610.0));
    let (cl, _) = l.panel(5).unwrap().chart().unwrap();
    assert_eq!(cl.domain.0, (at(9, 0), at(10, 0)));
    // Back to the range, from the newest sample.
    assert_eq!(after.reset_time(&m).time, v.time);
    // On ops, the graph and the heatmap aren't linked; the column's two charts are.
    let ops = model_of("ops.fig");
    assert_eq!(ops.linked(), [5, 6]);
    let ov = DashView::initial(&ops);
    assert_eq!(ov.time, None, "no range: all the data");
    assert_eq!(ov.shared(&ops), Some((at(10, 0), at(12, 0))));
    let mut heat = ov.panel(&ops, 4);
    heat.y = Some((0.0, 3.0));
    let after = ov.with_panel(&ops, 4, heat.clone());
    assert_eq!((after.time, after.panels[4].view.y), (None, heat.y));
}

#[test]
fn the_chip_names_the_range_or_the_ends_of_a_custom_one() {
    let m = model_of("health.fig");
    let v = DashView::initial(&m);
    assert_eq!(v.chip(&m).as_deref(), Some("Last 6 hours"));
    let keys: Vec<&str> = dashboard::RANGES.iter().map(|r| r.key).collect();
    assert_eq!(keys, ["15m", "1h", "6h", "24h", "7d", "30d", "all"]);
    let hour = v.with_range(&m, Some(3600.0));
    assert_eq!(hour.time, Some((at(11, 0), at(12, 0))));
    assert_eq!(hour.chip(&m).as_deref(), Some("Last hour"));
    assert_eq!(v.with_range(&m, None).chip(&m).as_deref(), Some("All time"));
    let custom = DashView {
        time: Some((at(10, 5), at(11, 40))),
        ..v.clone()
    };
    assert_eq!(custom.chip(&m).as_deref(), Some("10:05 – 11:40"));
    let l = lay(&m, &custom, (880.0, 610.0));
    assert_eq!(l.header.chip.unwrap().text.text, "10:05 – 11:40");
    assert_eq!(dashboard::time::last_label(7200.0), "Last 2 hours");
    assert_eq!(dashboard::time::last_label(5400.0), "Last 90 minutes");
    // A range chosen eases: halfway there, halfway between.
    let mid = dashboard::tween(&v, &hour, 0.5);
    assert_eq!(mid.time, Some((at(8, 30), at(12, 0))));
    // Without a `time`, no chip.
    let ops_free = model_from(
        r#"{"type": "dashboard", "rows": [[{"type": "stat", "data": {"values": [{"a": 1}]}, "value": {"sum": "a"}}]]}"#,
        &[],
        &Sources::new(),
    );
    assert_eq!(DashView::initial(&ops_free).chip(&ops_free), None);
}

#[test]
fn a_bad_panel_shows_its_error_figure_and_the_rest_draw() {
    let m = model_of("broken-panel.fig");
    let ready: Vec<bool> = m.panels.iter().map(|p| p.ready()).collect();
    assert_eq!(ready, [true, false, true, true, true, false]);
    let missing = failure(&m, 1);
    assert_eq!(missing.kind, ErrorKind::Unreadable);
    assert_eq!(missing.msg, "deploys-missing.csv: no such file");
    let mismatch = failure(&m, 5);
    assert_eq!(mismatch.kind, ErrorKind::Mismatch);
    assert_eq!(mismatch.msg, "no column \"rpm\" in the data");
    assert_eq!(mismatch.at.map(|p| p.line), Some(17));
    let l = lay(&m, &DashView::initial(&m), (880.0, 610.0));
    let p = l.panel(5).unwrap();
    assert!(matches!(p.figure, PanelFigure::Failed));
    assert!(p.scene.regions.iter().any(|r| r.hit == Hit::ErrorFigure));
    let said = texts(&p.scene);
    assert!(
        said.contains(&"Can't draw Throughput".to_string()),
        "{said:?}"
    );
    // The failed tile keeps its label, quiet as a tile's.
    assert!(texts(&l.panel(1).unwrap().scene).contains(&"Deploys".to_string()));
    // `!` turns one panel's figure over to its log; the others stay as they are.
    let mut v = DashView::initial(&m);
    v.panels[5].error = die_fig::plot::ErrorView::Log { top: 0, mark: 0 };
    let l = lay(&m, &v, (880.0, 610.0));
    let p = l.panel(5).unwrap();
    assert!(p.scene.regions.iter().any(|r| r.hit == Hit::LogEntry(0)));
    assert!(
        !l.panel(1)
            .unwrap()
            .scene
            .regions
            .iter()
            .any(|r| matches!(r.hit, Hit::LogEntry(_)))
    );
}

#[test]
fn a_failed_rebuild_keeps_the_panel_it_had_with_its_badge() {
    let text = r#"{"type": "dashboard", "data": {"a": "a.csv", "b": "b.csv"}, "rows": [[
        {"type": "line", "title": "A", "data": "a", "x": "t", "y": "v"},
        {"type": "line", "title": "B", "data": "b", "x": "t", "y": "v"}]]}"#;
    let table = |vs: &[f64]| {
        let t = die_fig::data::Column::number("t", (0..vs.len()).map(|i| i as f64).collect());
        let v = die_fig::data::Column::number("v", vs.to_vec());
        std::sync::Arc::new(die_fig::data::Table::new(vec![t, v]).unwrap())
    };
    let first = model_from(text, &[], &Sources::new());
    let wants = first.wants().to_vec();
    assert_eq!(wants.len(), 2);
    let mut good = Sources::new();
    good.ready(wants[0].clone(), table(&[1.0, 3.0, 2.0]));
    good.ready(wants[1].clone(), table(&[4.0, 1.0, 5.0]));
    let before = model_from(text, &[], &good);
    assert!(before.panels.iter().all(|p| p.ready()));
    let mut bad = good.clone();
    bad.insert(
        wants[1].clone(),
        SourceState::Failed(FigError::new("b.csv: no such file").with_kind(ErrorKind::Unreadable)),
    );
    let after = model_from(text, &[], &bad).carry(before);
    assert!(after.panels[1].ready(), "the figure before is kept");
    assert_eq!(
        after.panels[1].stale.as_ref().unwrap().msg,
        "b.csv: no such file"
    );
    assert_eq!(after.panels[0].stale, None);
    let l = lay(&after, &DashView::initial(&after), (880.0, 470.0));
    assert!(
        l.panel(1)
            .unwrap()
            .scene
            .regions
            .iter()
            .any(|r| r.hit == Hit::Badge)
    );
    assert!(
        !l.panel(0)
            .unwrap()
            .scene
            .regions
            .iter()
            .any(|r| r.hit == Hit::Badge)
    );
}

#[test]
fn nested_files_draw_as_groups_and_borrowed_data_reads_once() {
    let m = model_of("nested.fig");
    assert_eq!(m.panels.len(), 4);
    assert!(m.panels.iter().all(|p| p.ready()), "every panel draws");
    // health.fig#req is health.fig's requests.csv, which no one else here reads.
    let names: Vec<String> = m.wants().iter().map(Want::name).collect();
    assert_eq!(names, ["requests.csv", "service.csv"]);
    assert!(
        m.panels[1]
            .file
            .as_ref()
            .unwrap()
            .ends_with("parts/errors.fig")
    );
    assert_eq!(m.panels[1].title.as_deref(), Some("Errors per minute"));
    let l = lay(&m, &DashView::initial(&m), (880.0, 610.0));
    assert_eq!(l.groups.len(), 1);
    let g = &l.groups[0];
    assert_eq!(g.title.as_ref().unwrap().text, "Latency");
    assert!(g.file.ends_with("parts/latency.fig"));
    for i in [2, 3] {
        let f = l.panel(i).unwrap().frame;
        assert!(
            g.frame.inflate(-1.0).intersect(&f) == Some(f),
            "panel {i} in the group"
        );
    }
}

#[test]
fn files_that_include_themselves_are_an_error_at_the_ref() {
    let main = r#"{"type": "dashboard", "rows": [[{"file": "a.fig"}, {"type": "stat", "data": {"values": [{"a": 2}]}, "value": {"sum": "a"}}]]}"#;
    let a = r#"{"type": "dashboard", "title": "A", "rows": [[{"file": "b.fig"}]]}"#;
    let b = r#"{"type": "dashboard", "title": "B", "rows": [[
    {"file": "a.fig"}]]}"#;
    let m = model_from(
        main,
        &[("/specs/a.fig", a), ("/specs/b.fig", b)],
        &Sources::new(),
    );
    let e = failure(&m, 0);
    assert_eq!(
        e.msg,
        "a.fig includes itself: main.fig → a.fig → b.fig → a.fig"
    );
    assert_eq!(e.kind, ErrorKind::Mismatch);
    assert_eq!(e.file.as_deref(), Some(Path::new("/specs/b.fig")));
    // At the ref: the file's name in b.fig.
    assert_eq!(e.at.map(|p| (p.line, p.col)), Some((2, 14)));
    assert!(m.panels[1].ready(), "the rest draws");
    // Deeper than four is an error too.
    let chain: Vec<(String, String)> = (0..6)
        .map(|i| {
            (
                format!("/specs/d{i}.fig"),
                format!(
                    r#"{{"type": "dashboard", "rows": [[{{"file": "d{}.fig"}}]]}}"#,
                    i + 1
                ),
            )
        })
        .collect();
    let files: Vec<(&str, &str)> = chain
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let m = model_from(
        r#"{"type": "dashboard", "rows": [[{"file": "d0.fig"}]]}"#,
        &files,
        &Sources::new(),
    );
    assert_eq!(failure(&m, 0).msg, "files nest at most 4 deep");
    assert_eq!(
        failure(&m, 0).file.as_deref(),
        Some(Path::new("/specs/d3.fig"))
    );
}

#[test]
fn borrowed_data_says_what_is_missing_and_where_it_loops() {
    let main = r#"{"type": "dashboard", "data": {"req": "a.fig#req"}, "rows": [[
        {"type": "stat", "data": "req", "value": "count"},
        {"type": "stat", "data": "a.fig#nope", "value": "count"},
        {"type": "stat", "data": "chart.fig#req", "value": "count"}]]}"#;
    let a = r#"{"type": "dashboard", "data": {"req": "b.fig#req", "other": "o.csv"}, "rows": [[{"type": "stat", "data": "req", "value": "count"}]]}"#;
    let b = r#"{"type": "dashboard", "data": {"req": "a.fig#req"}, "rows": [[{"type": "stat", "data": "req", "value": "count"}]]}"#;
    let chart = r#"{"type": "line", "data": "o.csv", "x": "a", "y": "b"}"#;
    let files = [
        ("/specs/a.fig", a),
        ("/specs/b.fig", b),
        ("/specs/chart.fig", chart),
    ];
    let m = model_from(main, &files, &Sources::new());
    assert_eq!(
        failure(&m, 0).msg,
        "a.fig#req refers back to itself: a.fig#req → b.fig#req → a.fig#req"
    );
    assert_eq!(
        failure(&m, 1).msg,
        "a.fig has no data \"nope\"; it has \"req\", \"other\""
    );
    assert_eq!(failure(&m, 1).at.map(|p| p.line), Some(3));
    assert_eq!(
        failure(&m, 2).msg,
        "chart.fig is a chart, not a dashboard: it has no data to borrow"
    );
}

#[test]
fn commands_and_files_outside_wait_for_a_yes() {
    let main = r#"{"type": "dashboard", "data": {"err": {"run": "./errors.sh", "every": "30s"}, "tick": {"stream": "./ticker.sh"}}, "rows": [[
        {"type": "line", "title": "Errors", "data": "err", "x": "t", "y": "n"},
        {"type": "stat", "label": "Ticks", "data": "tick", "value": "count"},
        {"file": "/elsewhere/x.fig"}]]}"#;
    let x =
        r#"{"type": "stat", "label": "X", "data": {"values": [{"a": 1}]}, "value": {"sum": "a"}}"#;
    let files = [("/elsewhere/x.fig", x)];
    let m = model_from(main, &files, &Sources::new());
    assert!(m.is_live());
    let qs: Vec<String> = m.trust().iter().map(|t| t.question("this")).collect();
    assert_eq!(
        qs,
        [
            "Run \"./errors.sh\" every 30s for main.fig? (y/n)",
            "Stream \"./ticker.sh\" for main.fig? (y/n)",
            "Open \"/elsewhere/x.fig\" for main.fig? (y/n)",
        ]
    );
    assert!(
        matches!(&m.trust()[2], NeedsTrust::File { path, .. } if path == Path::new("/elsewhere/x.fig"))
    );
    // Each waits: a command's panel for the command, the file for the yes.
    let waits: Vec<String> = m
        .panels
        .iter()
        .map(|p| match &p.state {
            PanelState::Waiting(w) => w.clone(),
            _ => panic!("{:?} drew", p.title),
        })
        .collect();
    assert_eq!(
        waits,
        [
            "waiting for ./errors.sh",
            "waiting for ./ticker.sh",
            "waiting for your yes to open x.fig"
        ]
    );
    assert_eq!(
        m.wants()[0].dir,
        Path::new("/specs"),
        "commands run in the spec's directory"
    );
    // A waiting panel draws its frame and its title: no figure, no spinner.
    let l = lay(&m, &DashView::initial(&m), (880.0, 470.0));
    let p = l.panel(0).unwrap();
    assert!(matches!(p.figure, PanelFigure::Waiting(_)));
    assert_eq!(texts(&p.scene), ["Errors"]);
    assert_eq!(texts(&l.panel(1).unwrap().scene), ["Ticks"]);
    // The yes: the file opens, the command's rows come.
    let mut yes = Sources::new();
    yes.trust_file("/elsewhere/x.fig");
    let t = die_fig::data::Table::new(vec![
        die_fig::data::Column::number("t", vec![0.0, 1.0]),
        die_fig::data::Column::number("n", vec![3.0, 4.0]),
    ])
    .unwrap();
    yes.ready(m.wants()[0].clone(), std::sync::Arc::new(t));
    let m = model_from(main, &files, &yes);
    let ready: Vec<bool> = m.panels.iter().map(|p| p.ready()).collect();
    assert_eq!(ready, [true, false, true]);
    assert_eq!(m.trust().len(), 2, "the file asks no more");
}

#[test]
fn a_file_or_a_borrow_out_of_the_directory_by_dotdot_or_a_link_asks_first() {
    let x =
        r#"{"type": "stat", "label": "X", "data": {"values": [{"a": 1}]}, "value": {"sum": "a"}}"#;
    let o = r#"{"type": "dashboard", "data": {"d": {"values": [{"a": 1}, {"a": 2}]}}, "rows": [[{"type": "stat", "data": "d", "value": "count"}]]}"#;
    let main = r#"{"type": "dashboard", "rows": [[
        {"file": "../outside/x.fig"},
        {"type": "stat", "label": "Borrowed", "data": "/elsewhere/o.fig#d", "value": "count"},
        {"file": "inside/../x.fig"}]]}"#;
    let files = [
        ("/outside/x.fig", x),
        ("/elsewhere/o.fig", o),
        ("/specs/x.fig", x),
    ];
    let m = model_from(main, &files, &Sources::new());
    let asked: Vec<String> = m.trust().iter().map(|t| t.question("this")).collect();
    assert_eq!(
        asked,
        [
            "Open \"/outside/x.fig\" for main.fig? (y/n)",
            "Open \"/elsewhere/o.fig\" for main.fig? (y/n)",
        ],
        "`..` out of /specs and a borrow from elsewhere ask; `inside/..` stays in"
    );
    let state = |i: usize| match &m.panels[i].state {
        PanelState::Waiting(w) => w.clone(),
        PanelState::Failed(f) => format!("failed: {}", f.errors[0].msg),
        _ => "drew".to_string(),
    };
    assert_eq!(state(0), "waiting for your yes to open x.fig");
    assert_eq!(state(1), "waiting for your yes to read o.fig#d");
    // A no says so on the panel, as a command's does, until R.
    let mut no = Sources::new();
    no.refuse_file("/specs/../outside/x.fig");
    let m = model_from(main, &files, &no);
    let e = failure(&m, 0);
    assert_eq!(
        (e.msg.as_str(), e.kind),
        ("not opened: you said no", ErrorKind::Unreadable)
    );
    assert!(no.forget_refusals());
    // A link in the directory to a file outside it asks too (on disk, links followed).
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dash-links");
    let _ = std::fs::remove_dir_all(&base);
    let (dash, out) = (base.join("dash"), base.join("elsewhere"));
    std::fs::create_dir_all(&dash).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("x.fig"), x).unwrap();
    std::fs::write(dash.join("own.fig"), x).unwrap();
    std::os::unix::fs::symlink(out.join("x.fig"), dash.join("link.fig")).unwrap();
    let text = r#"{"type": "dashboard", "rows": [[{"file": "link.fig"}, {"file": "own.fig"}]]}"#;
    let Figure::Dashboard(spec) = spec::parse(text, "main.fig", &dash, &opt()).unwrap().figure
    else {
        panic!("a dashboard")
    };
    let (zone, o2) = (TimeZone::UTC, opt());
    let resolve = |p: &Path| dashboard::open(p, &o2);
    let file = dash.join("main.fig");
    let origin = Origin {
        file: Some(&file),
        dir: &dash,
        zone: &zone,
    };
    let m = DashModel::build(&spec, &Sources::new(), &resolve, &origin);
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        matches!(m.trust(), [NeedsTrust::File { path, .. }] if path.ends_with("link.fig")),
        "{:?}",
        m.trust()
    );
    assert!(!m.panels[0].ready() && m.panels[1].ready());
}

#[test]
fn files_nest_four_deep_with_or_without_a_file_of_its_own() {
    let chain: Vec<(String, String)> = (0..7)
        .map(|i| {
            (
                format!("/specs/d{i}.fig"),
                format!(
                    r#"{{"type": "dashboard", "rows": [[{{"file": "d{}.fig"}}]]}}"#,
                    i + 1
                ),
            )
        })
        .collect();
    let text = r#"{"type": "dashboard", "rows": [[{"file": "d0.fig"}]]}"#;
    let Figure::Dashboard(spec) = spec::parse(text, "", Path::new("/specs"), &opt())
        .unwrap()
        .figure
    else {
        panic!("a dashboard")
    };
    let resolve = |p: &Path| -> Result<Figure> {
        let (_, t) = (chain.iter().find(|(f, _)| Path::new(f) == p))
            .ok_or_else(|| FigError::new("no such file"))?;
        spec::parse(t, "x.fig", p.parent().unwrap(), &opt()).map(|p| p.figure)
    };
    let zone = TimeZone::UTC;
    let origin = Origin {
        file: None,
        dir: Path::new("/specs"),
        zone: &zone,
    };
    let m = DashModel::build(&spec, &Sources::new(), &resolve, &origin);
    assert_eq!(failure(&m, 0).msg, "files nest at most 4 deep");
    assert_eq!(
        failure(&m, 0).file.as_deref(),
        Some(Path::new("/specs/d3.fig")),
        "as deep as with a file of its own"
    );
}

/// A day of minutes with two time columns, `created` a day before `time`, an `hour` number and
/// a `v` of one.
fn two_times() -> die_fig::data::Table {
    use die_fig::data::{Column, Table};
    let n = 24 * 60;
    let time: Vec<f64> = (0..n).map(|i| at(0, 0) + 60.0 * i as f64).collect();
    Table::new(vec![
        Column::time("created", time.iter().map(|t| t - 86_400.0).collect()),
        Column::time("time", time),
        Column::number("hour", (0..n).map(|i| (i / 60) as f64).collect()),
        Column::number("v", vec![1.0; n]),
    ])
    .unwrap()
}

#[test]
fn a_tiles_range_goes_on_the_dashboards_time_field() {
    let main = r#"{"type": "dashboard", "data": {"d": "d.csv"}, "time": {"field": "time", "range": "6h"},
      "rows": [[
        {"type": "line", "title": "V", "data": "d", "x": "time", "y": "v"},
        {"type": "stat", "label": "no trend", "data": "d", "value": {"sum": "v"}},
        {"type": "stat", "label": "trend hour", "data": "d", "value": {"sum": "v"}, "trend": "hour"},
        {"type": "stat", "label": "trend time", "data": "d", "value": {"sum": "v"}, "trend": "time"},
        {"type": "meter", "label": "meter", "data": "d", "value": {"sum": "v"}, "max": 1000}
      ]]}"#;
    let first = model_from(main, &[], &Sources::new());
    let mut s = Sources::new();
    for w in first.wants() {
        s.ready(w.clone(), std::sync::Arc::new(two_times()));
    }
    let m = model_from(main, &[], &s);
    let v = DashView::initial(&m);
    let l = lay(&m, &v, (1400.0, 600.0));
    for i in 1..5 {
        let PanelFigure::Tile { model } = &l.panel(i).unwrap().figure else {
            panic!("tile {i}")
        };
        // The last six hours of minutes on `time`: 360 rows after 18:00 to 23:59.
        assert_eq!(
            (model.value, model.rows, model.along.as_deref()),
            (360.0, 360, Some("time")),
            "{}",
            model.label
        );
    }
}

#[test]
fn the_rows_scroll_when_their_least_heights_overflow() {
    let m = model_of("tall.fig");
    let mut v = DashView::initial(&m);
    let l = lay(&m, &v, (880.0, 610.0));
    assert!(l.max_scroll > 0.0);
    let (track, thumb) = l.scrollbar.unwrap();
    assert_eq!(track.w, 4.0);
    assert!(
        track.x > 852.0 && track.right() < 880.0,
        "in the right margin: {track:?}"
    );
    assert!(thumb.h < track.h && thumb.y == track.y);
    // Every chart at least 180 px tall, and the last panel past the foot.
    for p in l.panels.iter().flatten().skip(3) {
        assert!(p.frame.h >= 180.0, "{:?}", p.frame);
    }
    let last = l.panels.len() - 1;
    assert!(l.panel(last).unwrap().frame.bottom() > 610.0);
    // Scrolled past the end, it stops at the end; the rows move up by the scroll.
    v.scroll = 1e6;
    let end = lay(&m, &v, (880.0, 610.0));
    assert_eq!(end.scroll, l.max_scroll);
    let moved = end.panel(4).unwrap().frame.y - l.panel(4).unwrap().frame.y;
    assert_eq!(moved, -l.max_scroll);
    assert_eq!(end.panel(last).unwrap().frame.bottom(), 610.0 - 16.0);
    let (_, thumb) = end.scrollbar.unwrap();
    assert_eq!(thumb.bottom(), track.bottom());
    // Revealing the last panel from the top scrolls just far enough.
    assert_eq!(l.reveal(last), l.max_scroll);
    assert_eq!(end.reveal(0), 0.0);
    // A panel scrolled under the header doesn't take the pointer there.
    let p0 = end.panel(0).unwrap().frame;
    assert!(p0.bottom() < end.viewport.y);
    assert!(p0.x < 200.0 && p0.right() > 200.0);
    assert_eq!(dashboard::hit(&end, 200.0, 84.0), Some(DashHit::Chrome));
}

#[test]
fn a_narrow_area_wraps_a_row_its_panels_cant_share() {
    let m = model_of("health.fig");
    let l = lay(&m, &DashView::initial(&m), (400.0, 600.0));
    let frames: Vec<Rect> = l.panels.iter().flatten().map(|p| p.frame).collect();
    // Four tiles as two lines of two, then each chart on a line of its own.
    let ys: Vec<f32> = frames.iter().map(|f| f.y).collect();
    assert_eq!(ys[0], ys[1]);
    assert_eq!(ys[2], ys[3]);
    assert_eq!(ys[2] - frames[0].bottom(), 16.0);
    assert!(ys[4] > frames[2].bottom() && ys[5] > frames[4].bottom());
    assert_eq!(frames[4].w, frames[5].w);
    assert_eq!(frames[4].x, frames[0].x);
    assert_eq!(frames[4].right(), frames[1].right());
    assert!(
        l.max_scroll > 0.0,
        "it scrolls rather than squeeze a chart under 180 px"
    );
    assert_eq!(l.lines, [vec![0, 1], vec![2, 3], vec![4], vec![5]]);
}

#[test]
fn full_size_lays_out_one_panel_under_the_header() {
    let m = model_of("health.fig");
    let mut v = DashView::initial(&m);
    v.full = Some(5);
    let l = lay(&m, &v, (880.0, 610.0));
    let shown: Vec<usize> = l.panels.iter().flatten().map(|p| p.index).collect();
    assert_eq!(shown, [5]);
    assert_eq!(
        l.panel(5).unwrap().frame,
        Rect::new(28.0, 92.0, 824.0, 502.0)
    );
    assert_eq!(l.full, Some(5));
    let same = dashboard::layout_full(
        &m,
        &DashView::initial(&m),
        5,
        (880.0, 610.0),
        2.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    );
    assert_eq!(same.panel(5).unwrap().frame, l.panel(5).unwrap().frame);
}

#[test]
fn columns_stack_and_a_graph_and_a_heatmap_share_their_row() {
    let m = model_of("ops.fig");
    let l = lay(&m, &DashView::initial(&m), (880.0, 610.0));
    let f = |i: usize| l.panel(i).unwrap().frame;
    // Meters across the top, at a meter's height.
    assert!((0..3).all(|i| f(i).y == 92.0 && f(i).h == 64.0));
    // The graph, the heatmap and the column across the second row; the column's two charts
    // 12 px apart down it.
    assert_eq!((f(3).y, f(4).y, f(5).y), (172.0, 172.0, 172.0));
    assert_eq!(f(6).y - f(5).bottom(), 12.0);
    assert_eq!((f(5).x, f(5).w), (f(6).x, f(6).w));
    assert_eq!(f(6).bottom(), f(3).bottom());
    assert!(matches!(
        l.panel(3).unwrap().figure,
        PanelFigure::Graph { .. }
    ));
    assert!(l.panel(4).unwrap().chart().is_some());
}

#[test]
fn hit_says_the_chip_the_live_status_a_title_and_a_plot() {
    let mut m = model_of("health.fig");
    m.live = Some(LiveStatus::updated(3.0));
    let l = lay(&m, &DashView::initial(&m), (880.0, 610.0));
    assert_eq!(dashboard::hit(&l, 40.0, 64.0), Some(DashHit::Chip));
    assert_eq!(dashboard::hit(&l, 840.0, 64.0), Some(DashHit::Live));
    assert_eq!(dashboard::hit(&l, 500.0, 30.0), Some(DashHit::Chrome));
    assert_eq!(dashboard::hit(&l, -1.0, 30.0), None);
    let p = l.panel(4).unwrap();
    let t = p.title.unwrap();
    assert!(matches!(
        dashboard::hit(&l, t.x + 4.0, t.y + t.h / 2.0),
        Some(DashHit::Panel {
            panel: 4,
            hit: Hit::PanelTitle(4),
            ..
        })
    ));
    let (cl, _) = p.chart().unwrap();
    let (x, y) = (p.frame.x + cl.plot.x + 40.0, p.frame.y + cl.plot.y + 40.0);
    match dashboard::hit(&l, x, y) {
        Some(DashHit::Panel {
            panel: 4,
            hit: Hit::Plot(4),
            at,
        }) => {
            assert_eq!(at, (cl.plot.x + 40.0, cl.plot.y + 40.0));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        dashboard::hit(&l, 60.0, 140.0),
        Some(DashHit::Panel {
            panel: 0,
            hit: Hit::Plot(0),
            ..
        })
    ));
    // Between panels: chrome.
    assert_eq!(dashboard::hit(&l, 440.0, 400.0), Some(DashHit::Chrome));
}

/// Where panel `i`'s plot shows time `t`, halfway up, in the area's px.
fn point_at(l: &DashLayout, i: usize, t: f64) -> (f32, f32) {
    let p = l.panel(i).unwrap();
    let (cl, _) = p.chart().unwrap();
    let (ox, oy) = p.origin();
    (cl.x.map(t) + ox, cl.plot.y + cl.plot.h / 2.0 + oy)
}

/// The dashed hairlines in a scene and their x.
fn dashed(scene: &Scene) -> Vec<f32> {
    scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Path {
                path,
                stroke: Some(s),
                ..
            } if s.dash.is_some() => path.bounds().map(|b| b.x),
            _ => None,
        })
        .collect()
}

#[test]
fn the_hovered_moment_shows_in_every_other_linked_panel() {
    let m = model_of("health.fig");
    let v = DashView::initial(&m);
    let l = lay(&m, &v, (880.0, 610.0));
    let (x, y) = point_at(&l, 4, at(10, 5));
    let h = dashboard::hover(&m, &l, x, y).unwrap();
    assert_eq!((h.panel, h.time), (4, Some(at(10, 5))));
    assert!(matches!(
        h.target,
        PanelHover::Chart {
            cursor: Some(_),
            ..
        }
    ));
    let o = dashboard::overlay(&m, &l, &v, Some(&h), &Theme::mono_dark(), &mut FixedMeasure);
    // The right panel's dashed line at 10:05, inside its frame; one card, the hovered one's.
    let right = l.panel(5).unwrap().frame;
    let (rx, _) = point_at(&l, 5, at(10, 5));
    let lines = dashed(&o);
    assert!(
        lines
            .iter()
            .any(|&lx| (lx - rx).abs() < 1.0 && right.contains(lx, right.y + 100.0)),
        "{lines:?} for {rx}"
    );
    let cards = o
        .items
        .iter()
        .filter(|i| matches!(i, Item::Shadow { .. }))
        .count();
    assert_eq!(cards, 1);
    // Its value beside its dot, as m4's "17".
    let said = texts(&o);
    assert!(said.iter().any(|t| t.parse::<f64>().is_ok()), "{said:?}");
    // Shared: each linked chart shows its card at the moment.
    let mut shared = m;
    shared.tooltip = Tooltip::Shared;
    let l = lay(&shared, &v, (880.0, 610.0));
    let o = dashboard::overlay(
        &shared,
        &l,
        &v,
        Some(&h),
        &Theme::mono_dark(),
        &mut FixedMeasure,
    );
    let cards = o
        .items
        .iter()
        .filter(|i| matches!(i, Item::Shadow { .. }))
        .count();
    assert_eq!(cards, 2);
}

#[test]
fn the_focused_panels_cursor_drives_the_line_with_nothing_hovered() {
    let m = model_of("health.fig");
    let mut v = DashView::initial(&m);
    let cm = m.panels[4].chart().unwrap();
    let key = cm.keys()[0].clone();
    let si = cm.index_of(&key).unwrap();
    let row = cm.series[si]
        .xs
        .iter()
        .position(|&x| x == at(9, 30))
        .unwrap();
    v.focused = Some(4);
    v.panels[4].cursor = Some(die_fig::interact::Cursor::Point {
        series: key,
        row: row as u32,
    });
    let l = lay(&m, &v, (880.0, 610.0));
    let o = dashboard::overlay(&m, &l, &v, None, &Theme::mono_dark(), &mut FixedMeasure);
    let (rx, _) = point_at(&l, 5, at(9, 30));
    assert!(dashed(&o).iter().any(|&lx| (lx - rx).abs() < 1.0));
    // Nothing hovered and no cursor: an empty overlay.
    let calm = DashView::initial(&m);
    let o = dashboard::overlay(&m, &l, &calm, None, &Theme::mono_dark(), &mut FixedMeasure);
    assert_eq!(o.bounds(), None);
}

#[test]
fn the_focused_panel_is_ringed_in_the_border_grey() {
    let m = model_of("health.fig");
    let mut v = DashView::initial(&m);
    let theme = Theme::mono_dark();
    let rings = |v: &DashView| -> Vec<die_fig::theme::Color> {
        let l = lay(&m, v, (880.0, 610.0));
        let b = dashboard::base(&l, v, &theme, &mut FixedMeasure);
        b.items
            .iter()
            .filter_map(|i| match i {
                Item::Path {
                    stroke: Some(s),
                    fill: None,
                    path,
                } if path.bounds().is_some_and(|r| {
                    l.panels.iter().flatten().any(|p| {
                        (r.x - p.frame.x).abs() < 0.01
                            && (r.w - p.frame.w).abs() < 0.01
                            && (r.y - p.frame.y).abs() < 0.01
                    })
                }) =>
                {
                    match &s.paint {
                        die_fig::scene::Paint::Solid { color, .. } => Some(*color),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(rings(&v), vec![theme.grid; 6]);
    v.focused = Some(2);
    let r = rings(&v);
    assert_eq!(r[2], dashboard::draw::focus_ring(&theme));
    assert_eq!(r[2], die_fig::theme::Color(0x5a, 0x5a, 0x5a));
    assert_ne!(r[2], theme.accent);
    assert_eq!(r.iter().filter(|c| **c == theme.grid).count(), 5);
}

#[test]
fn heights_share_what_tiles_leave_by_weight_each_at_least_its_least() {
    // Tiles at 100, two flexible lines by 1 and 2 in 600 with 16 px gaps.
    let h = share(
        &[Some(100.0), None, None],
        &[100.0, 180.0, 180.0],
        &[1.0, 1.0, 2.0],
        600.0,
        16.0,
    );
    assert_eq!(h[0], 100.0);
    assert_eq!(h.iter().sum::<f32>(), 600.0 - 32.0);
    assert!(h[2] > h[1]);
    // A weight that would leave one under its least gives it its least.
    let h = share(&[None, None], &[180.0, 180.0], &[1.0, 9.0], 500.0, 16.0);
    assert_eq!(h[0], 180.0);
    assert_eq!(h[1], 500.0 - 16.0 - 180.0);
    // Leasts that don't fit: each its least (the rows scroll).
    let h = share(&[None, None, None], &[180.0; 3], &[1.0; 3], 300.0, 16.0);
    assert_eq!(h, [180.0; 3]);
}

#[test]
fn the_fig_layouts_shapes_read_and_write_back_the_same() {
    let text = r#"{"type": "dashboard", "title": "Shapes",
      "data": {"req": "requests.csv", "far": "other.fig#req"},
      "time": {"field": "time", "range": "6h"},
      "tooltip": "shared",
      "rows": [
        [{"type": "stat", "data": "req", "value": {"sum": "total"}}, {"file": "parts/errors.fig", "span": 2}],
        {"height": 2, "panels": [
          {"type": "line", "data": "far", "x": "time", "y": "api", "span": 3},
          {"column": [{"type": "line", "data": "req", "x": "time", "y": "api"},
                      {"type": "stat", "data": "x.fig#req", "value": "count"}], "span": 2}]}
      ]}"#;
    let p = spec::parse(text, "shapes.fig", Path::new("/specs"), &opt()).unwrap();
    assert_eq!(p.warnings, []);
    let Figure::Dashboard(d) = &p.figure else {
        panic!()
    };
    assert_eq!(d.tooltip, Tooltip::Shared);
    assert_eq!((d.rows[0].height, d.rows[1].height), (None, Some(2.0)));
    assert!(
        matches!(&d.data[1].1.source, DataRef::Borrow { file, name, .. }
        if file == Path::new("/specs/other.fig") && name == "req")
    );
    let Panel::Column(col) = &d.rows[1].panels[1].panel else {
        panic!("a column")
    };
    assert_eq!((col.len(), d.rows[1].panels[1].span), (2, 2));
    let Panel::Figure(f) = &col[1].panel else {
        panic!()
    };
    let Figure::Stat(s) = &**f else { panic!() };
    assert!(matches!(&s.data.source, DataRef::Borrow { name, .. } if name == "req"));
    // Written, it reads back as the same figure, and writes the same text again.
    let written = spec::write(&p.figure);
    let again = spec::parse(&written, "shapes.fig", Path::new("/specs"), &opt()).unwrap();
    assert_eq!(spec::write(&again.figure), written);
    assert!(written.contains("\"height\": 2"), "{written}");
    assert!(written.contains("\"column\""), "{written}");
    assert!(written.contains("/specs/other.fig#req"), "{written}");
    assert!(written.contains("\"tooltip\": \"shared\""), "{written}");
    // Every fixture writes back to what it reads.
    for name in [
        "health.fig",
        "ops.fig",
        "nested.fig",
        "broken-panel.fig",
        "tall.fig",
    ] {
        let f = Figure::Dashboard(spec_of(name));
        let w = spec::write(&f);
        let back = spec::parse(&w, name, &dir(), &opt()).unwrap();
        assert_eq!(spec::write(&back.figure), w, "{name}");
    }
}

#[test]
fn the_fig_layouts_shapes_say_what_is_wrong() {
    let cases = [
        (
            r#"{"type": "dashboard", "rows": [{"panels": []}]}"#,
            "a row needs a panel",
        ),
        (
            r#"{"type": "dashboard", "rows": [{"height": 0, "panels": [{"file": "a.fig"}]}]}"#,
            "\"height\" must be a number above 0",
        ),
        (
            r#"{"type": "dashboard", "rows": [{"height": 1}]}"#,
            "a row needs \"panels\"",
        ),
        (
            r#"{"type": "dashboard", "rows": [[{"column": []}]]}"#,
            "a column needs a panel",
        ),
        (
            r#"{"type": "dashboard", "tooltip": "all", "rows": [[{"file": "a.fig"}]]}"#,
            "\"tooltip\" must be \"single\" or \"shared\"",
        ),
        (
            r#"{"type": "dashboard", "rows": [7]}"#,
            "a row must be a list of panels or {\"height\": …, \"panels\": […]}, not a number",
        ),
    ];
    for (text, want) in cases {
        let e = spec::parse(text, "x.fig", Path::new("/specs"), &opt()).unwrap_err();
        assert_eq!(e.msg, want, "{text}");
        assert!(e.at.is_some(), "{text}");
    }
}

#[test]
fn a_brush_on_a_linked_chart_shows_in_every_linked_chart() {
    let m = model_of("health.fig");
    let v = DashView::initial(&m);
    let l = lay(&m, &v, (880.0, 610.0));
    let (a, b) = (point_at(&l, 4, at(8, 0)), point_at(&l, 4, at(9, 0)));
    let mut h = dashboard::hover(&m, &l, b.0, b.1).unwrap();
    h.brush = Some((a, b));
    let o = dashboard::overlay(&m, &l, &v, Some(&h), &Theme::mono_dark(), &mut FixedMeasure);
    // The brush's wash in each chart, from 08:00 to 09:00 on its own axis.
    let washes: Vec<Rect> = o
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Path {
                path,
                fill: Some(die_fig::scene::Paint::Solid { alpha, .. }),
                stroke: None,
            } if (*alpha - 0.08).abs() < 1e-6 => path.bounds(),
            _ => None,
        })
        .collect();
    assert_eq!(washes.len(), 2, "{washes:?}");
    for (w, i) in washes.iter().zip([4, 5]) {
        let (x0, _) = point_at(&l, i, at(8, 0));
        let (x1, _) = point_at(&l, i, at(9, 0));
        assert!(
            (w.x - x0).abs() < 1.0 && (w.right() - x1).abs() < 1.0,
            "panel {i}: {w:?}"
        );
    }
    // No card while brushing, nor a moment's line.
    assert!(!o.items.iter().any(|i| matches!(i, Item::Shadow { .. })));
}
