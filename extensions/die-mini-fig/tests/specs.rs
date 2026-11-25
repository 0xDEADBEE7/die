//! The owner's specs: every one in their folder parses, its data loads, and every field a
//! panel names is a column of its table. Skipped when the folder isn't there.

use die_test_cap as _;

use die_mini_fig::data::{self, Table};
use die_mini_fig::spec::{self, Kind, Panel};
use std::path::Path;

const FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/specs");

/// The fields a panel reads from its table.
fn fields(p: &Panel) -> Vec<String> {
    let channels = [&p.x, &p.y, &p.color].into_iter().flatten();
    let mut out: Vec<String> = channels.map(|c| c.field.clone()).collect();
    out.extend(p.tooltip.iter().cloned());
    if let Kind::Stat(s) = &p.kind {
        out.push(s.field.clone());
        out.extend(s.trend.clone());
    }
    out
}

#[test]
fn each_of_the_owners_specs_parses_and_its_data_loads() {
    let Ok(entries) = std::fs::read_dir(FOLDER) else {
        eprintln!("skipped: {FOLDER} isn't here");
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.retain(|p| p.extension().is_some_and(|e| e == "fig"));
    paths.sort();
    assert!(!paths.is_empty(), "no .fig in {FOLDER}");
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy();
        let fig = spec::read(path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let tables = data::load(&fig);
        for (t, read) in &tables {
            assert!(read.is_ok(), "{name}: {t}: {read:?}");
        }
        assert!(fig.panels().count() > 0, "{name}: no panels");
        if fig.time.as_ref().is_some_and(|t| t.secs.is_some()) {
            assert!(
                data::window(&fig, &tables).is_some(),
                "{name}: no time range"
            );
        }
        for p in fig.panels() {
            let what = format!("{name}, panel \"{}\"", p.title);
            if let Kind::Tree(g) = &p.kind {
                assert!(g.nodes.len() > 1 && !g.edges.is_empty(), "{what}");
                continue;
            }
            let source = p.data.as_ref().unwrap_or_else(|| panic!("{what}: no data"));
            let table = tables[source].as_ref().unwrap();
            let table = table.transform(&p.transform).unwrap();
            assert!(!table.is_empty(), "{what}: no rows");
            for f in fields(p) {
                assert!(table.column(&f).is_some(), "{what}: no column \"{f}\"");
            }
            for l in p.layers.iter().filter(|l| !l.rule) {
                let t: &Table = tables[l.data.as_ref().expect("a line layer's data")]
                    .as_ref()
                    .unwrap();
                assert!(
                    t.column(l.by.as_deref().unwrap()).is_some(),
                    "{what}: a layer"
                );
            }
        }
    }
}

/// A dashboard of a stat tile, a line chart over a time range and a tree, from a temporary
/// folder, as the owner's specs are written.
#[test]
fn a_dashboard_reads_its_tables_and_its_time_range() {
    let dir = std::env::temp_dir().join(format!("mini-fig-spec-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let csv =
        "time,v,g\n2026-11-20T00:00:00Z,1,a\n2026-11-21T00:00:00Z,2,b\n2026-11-22T00:00:00Z,3,a\n";
    std::fs::write(dir.join("data/t.csv"), csv).unwrap();
    let spec = r#"{"type": "dashboard", "title": "D", "data": {"t": "data/t.csv"},
        "time": {"field": "time", "range": "1d"},
        "rows": [[{"type": "stat", "label": "V", "data": "t", "value": {"last": "v"},
                   "unit": "s", "format": ".2f", "trend": "time", "delta": "previous"}],
                 {"height": 1.5, "panels": [
                   {"type": "line", "data": "t", "x": "time", "y": {"field": "v", "scale": "log"},
                    "color": "g", "span": 2,
                    "layers": [{"mark": "rule", "x": null, "y": 2, "fig": {"label": "two", "color": "dim", "dash": true}}]},
                   {"column": [{"type": "graph", "nodes": [{"id": 1}, {"id": 2, "cost": 5}],
                                "edges": [{"from": 1, "to": 2}]}]}]}]}"#;
    std::fs::write(dir.join("d.fig"), spec).unwrap();
    let fig = spec::read(&dir.join("d.fig")).unwrap();
    let tables = data::load(&fig);
    let (lo, hi) = data::window(&fig, &tables).unwrap();
    assert_eq!(hi - lo, 86_400.0);
    let t = tables["t"].as_ref().unwrap();
    assert_eq!(t.within("time", (lo, hi)).len(), 1);
    assert_eq!(fig.rows.len(), 2);
    assert_eq!((fig.rows[1].height, fig.rows[1].cells[0].span), (1.5, 2.0));
    let line = &fig.rows[1].cells[0].panels[0];
    assert!(line.y.as_ref().unwrap().log && line.layers[0].rule && line.layers[0].dash);
    let Kind::Tree(g) = &fig.rows[1].cells[1].panels[0].kind else {
        panic!("not a tree");
    };
    assert_eq!(g.root, "1");
    assert_eq!(g.nodes[1].fields, [("cost".to_string(), "5".to_string())]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(spec::read(Path::new("/nonexistent/x.fig")).is_err());
}
