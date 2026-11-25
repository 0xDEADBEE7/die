//! Drawing: every one of the owner's specs draws each of its panels with hits in key order, a
//! dashboard lays its rows out by height and its cells by span, and the pointer finds the mark
//! under it with the card's fields. Skipped without the fonts, or the owner's folder for the
//! first.

use die_test_cap as _;

use die_draw::Book;
use die_mini_fig::canvas::{Canvas, Rect, Theme};
use die_mini_fig::{dash, data, hit, spec};
use std::path::Path;

const FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/specs");

fn book() -> Option<Book> {
    let book = Book::load(None).ok();
    if book.is_none() {
        eprintln!("skipped: no fonts");
    }
    book
}

#[test]
fn each_of_the_owners_specs_draws_every_panel_with_hits_in_key_order() {
    let Ok(entries) = std::fs::read_dir(FOLDER) else {
        eprintln!("skipped: {FOLDER} isn't here");
        return;
    };
    let Some(mut book) = book() else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.retain(|p| p.extension().is_some_and(|e| e == "fig"));
    paths.sort();
    let th = Theme::default();
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy();
        let fig = spec::read(path).unwrap();
        let tables = data::load(&fig).unwrap();
        let mut c = Canvas::new(&mut book, (1600.0, 1000.0), 1.0, th.surface).unwrap();
        let drawn = dash::draw(&mut c, &th, &fig, &tables, 0.0);
        assert_eq!(drawn.frames.len(), fig.panels().count(), "{name}");
        for i in 0..drawn.frames.len() {
            let any = drawn.hits.iter().any(|h| h.key.0 == i);
            assert!(any, "{name}: panel {i} drew no marks");
        }
        assert!(
            drawn.hits.windows(2).all(|w| w[0].key <= w[1].key),
            "{name}"
        );
        let last = drawn.hits.last().unwrap();
        hit::card(&mut c, &th, last, Rect::new(0.0, 0.0, 1600.0, 1000.0));
    }
}

#[test]
fn a_dashboard_lays_out_its_rows_by_height_and_its_cells_by_span() {
    let stat = r#"{"type": "stat", "data": {"values": [{"v": 1}]}, "value": {"last": "v"}}"#;
    let chart = r#"{"type": "line", "title": "T", "x": "a", "y": "b"}"#;
    let spec = format!(
        r#"{{"type": "dashboard", "title": "D", "rows": [[{stat}, {stat}],
            {{"height": 2, "panels": [{{"span": 2, "column": [{chart}]}}, {{"column": [{chart}, {chart}]}}]}},
            {{"height": 1, "panels": [{chart}]}}]}}"#
    );
    let fig = spec::parse(&spec, Path::new(".")).unwrap();
    let (frames, end) = dash::layout(&fig, (1000.0, 1000.0));
    assert_eq!(frames.len(), 6);
    let (tile, wide, top, under, last) = (frames[0], frames[2], frames[3], frames[4], frames[5]);
    assert_eq!((tile.y, tile.h), (56.0, 100.0));
    assert_eq!(frames[1].x, tile.right() + 12.0);
    // The rows share what's left two to one, the second no shorter than a readable share.
    let room = 1000.0 - 56.0 - 16.0 - 2.0 * 16.0 - 100.0;
    assert!((wide.h - room * 2.0 / 3.0).abs() < 0.01 && last.h == 320.0);
    assert!((wide.w - 2.0 * top.w).abs() < 0.01);
    assert_eq!((under.y, under.h), (top.bottom() + 12.0, top.h));
    assert_eq!(end, last.bottom() + 16.0);
}

#[test]
fn the_pointer_finds_the_mark_under_it_and_the_card_gives_its_fields() {
    let Some(mut book) = book() else {
        return;
    };
    let spec = r#"{"type": "bar", "title": "B", "x": {"field": "k", "type": "category"},
        "y": "n", "color": "g", "tooltip": ["k", "n"],
        "data": {"values": [{"k": "a", "n": 3, "g": "x"}, {"k": "a", "n": 5, "g": "y"},
                            {"k": "b", "n": 2, "g": "x"}]}}"#;
    let fig = spec::parse(spec, Path::new(".")).unwrap();
    let tables = data::load(&fig).unwrap();
    let th = Theme::default();
    let mut c = Canvas::new(&mut book, (600.0, 400.0), 1.0, th.surface).unwrap();
    let drawn = dash::draw(&mut c, &th, &fig, &tables, 0.0);
    // Series x's two bars, then series y's one: in key order.
    let keys: Vec<_> = drawn.hits.iter().map(|h| h.key).collect();
    assert_eq!(keys, [(0, 0, 0), (0, 0, 1), (0, 1, 0)]);
    let y = &drawn.hits[2];
    let (x, top) = y.anchor();
    assert_eq!(hit::at(&drawn.hits, x, top + 2.0), Some(2));
    assert_eq!(y.title, "y");
    assert_eq!(y.rows, [("k".into(), "a".into()), ("n".into(), "5".into())]);
    // Band a is shared by its two bars; band b's one bar has it to itself.
    let w = |i: usize| match drawn.hits[i].shape {
        hit::Shape::Rect(r) => r.w,
        hit::Shape::Point(..) => 0.0,
    };
    assert!((w(1) - 2.0 * w(0)).abs() < 0.01 || w(1) == 32.0);
    assert_eq!(hit::at(&drawn.hits, 1.0, 1.0), None);
}
