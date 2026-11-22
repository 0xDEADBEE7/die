//! The tiles: a stat's value, delta and trend over a range, a meter's share and status, a
//! sparkline's points; their layout at m4's size and smaller (measured with `FixedMeasure`),
//! the hit, the cards, the linked time's crosshair and the detail tree; the specs written back.

use die_test_cap as _;

use die_fig::data::{Column, ReadOptions, Table};
use die_fig::error::ErrorKind;
use die_fig::figure::draw::{self, Role, TileLayout};
use die_fig::figure::{self, TileHover, TileKind, TileModel, short_number};
use die_fig::scene::{FixedMeasure, Item, Rect, Scene};
use die_fig::spec::{self, Figure};
use die_fig::style::Status;
use die_fig::theme::Theme;
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};

const HOUR: f64 = 3600.0;
/// 2026-11-21 00:00 UTC.
const T0: f64 = 1_795_219_200.0;

fn tiles_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("data")
        .join("tiles")
}

fn figure_of(text: &str) -> Figure {
    let opt = ReadOptions::new(TimeZone::UTC);
    spec::parse(text, "t.json", &tiles_dir(), &opt)
        .expect("it parses")
        .figure
}

/// Twelve hours a minute apart: rpm rising by one a minute from 100, errors alternating 0 and
/// 2, and a category column.
fn day() -> Table {
    let n = 721;
    let time: Vec<f64> = (0..n).map(|i| T0 + 60.0 * i as f64).collect();
    let rpm: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
    let errors: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 2.0 }).collect();
    let host: Vec<&str> = (0..n).map(|i| if i % 3 == 0 { "a" } else { "b" }).collect();
    Table::new(vec![
        Column::time("time", time),
        Column::number("rpm", rpm),
        Column::number("errors", errors),
        Column::category("host", &host),
    ])
    .expect("a table")
}

fn build(spec_text: &str, table: &Table, range: Option<(f64, f64)>) -> TileModel {
    TileModel::build_in(&figure_of(spec_text), table, range, &TimeZone::UTC).expect("a tile")
}

/// The last six hours of [`day`], both ends in: 06:00 to 12:00.
fn last6() -> Option<(f64, f64)> {
    Some((T0 + 6.0 * HOUR, T0 + 12.0 * HOUR))
}

fn text_boxes(l: &TileLayout) -> Vec<(String, Rect)> {
    let mut m = FixedMeasure;
    l.pieces
        .iter()
        .map(|p| (p.text.clone(), p.bounds(&mut m)))
        .collect()
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.intersect(b).is_some_and(|r| r.w > 0.5 && r.h > 0.5)
}

fn scene_texts(scene: &Scene) -> Vec<String> {
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
fn a_stat_aggregates_over_the_range() {
    let t = day();
    let s = |op: &str| {
        build(
            &format!(
                r#"{{"type": "stat", "data": "x.csv", "value": {{"{op}": "rpm"}}, "trend": "time"}}"#
            ),
            &t,
            last6(),
        )
    };
    // 06:00 to 12:00: minutes 360 to 720, rpm 460 to 820, 361 rows.
    assert_eq!(s("sum").rows, 361);
    assert_eq!(s("sum").value, (460..=820).map(f64::from).sum::<f64>());
    assert_eq!(s("mean").value, 640.0);
    assert_eq!(s("min").value, 460.0);
    assert_eq!(s("max").value, 820.0);
    assert_eq!(s("p50").value, 640.0);
    assert!((s("p95").value - 802.0).abs() < 1e-9);
    assert_eq!(s("last").value, 820.0);
    let count = build(
        r#"{"type": "stat", "data": "x.csv", "value": "count"}"#,
        &t,
        last6(),
    );
    assert_eq!(count.value, 361.0);
    // Every row without a range.
    let all = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"max": "rpm"}}"#,
        &t,
        None,
    );
    assert_eq!((all.value, all.rows, all.given), (820.0, 721, false));
}

#[test]
fn last_is_the_newest_by_the_trend_not_the_last_row() {
    let t = Table::new(vec![
        Column::time("time", vec![T0 + 300.0, T0, T0 + 600.0, T0 + 120.0]),
        Column::number("v", vec![3.0, 1.0, 9.0, 2.0]),
    ])
    .unwrap();
    let m = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"last": "v"}, "trend": "time"}"#,
        &t,
        None,
    );
    assert_eq!(m.value, 9.0);
    assert_eq!(m.aggregate_text(), "newest v");
}

#[test]
fn the_delta_compares_with_the_range_before_or_a_number() {
    let t = day();
    let m = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"mean": "rpm"}, "delta": "previous", "trend": "time"}"#,
        &t,
        last6(),
    );
    // 00:00 to 05:59: rpm 100 to 459, mean 279.5.
    let c = m.change.as_ref().expect("a change");
    assert_eq!(c.against, 279.5);
    assert_eq!(c.period, "previous 6h");
    assert_eq!(c.direction(), 1);
    assert_eq!(c.good, Some(true));
    assert_eq!(
        m.change_text(),
        Some(("129 %".into(), "129 % vs previous 6h".into()))
    );
    // Down is good for errors; neither says nothing.
    let down = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"mean": "rpm"}, "delta": "previous", "trend": "time", "good": "down"}"#,
        &t,
        last6(),
    );
    assert_eq!(down.change.unwrap().good, Some(false));
    let neither = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"mean": "rpm"}, "delta": "previous", "trend": "time", "good": "neither"}"#,
        &t,
        last6(),
    );
    assert_eq!(neither.change.unwrap().good, None);
    // Against a number, in the unit: points for a percentage, the unit otherwise.
    let fixed = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"mean": "errors"}, "unit": "%", "delta": 1.077, "good": "down"}"#,
        &t,
        last6(),
    );
    assert_eq!(fixed.change_text().unwrap().0, "0.08 pt");
    assert_eq!(fixed.change.as_ref().unwrap().good, Some(true));
    let ms = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"max": "rpm"}, "unit": "ms", "delta": 808}"#,
        &t,
        last6(),
    );
    assert_eq!(ms.change_text().unwrap().1, "12 ms vs 808 ms");
    // No rows before the range: the tile says there's nothing to compare.
    let all = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"sum": "rpm"}, "delta": "previous", "trend": "time"}"#,
        &t,
        None,
    );
    assert!(all.change.as_ref().unwrap().missing());
    assert_eq!(all.change_text(), None);
}

#[test]
fn the_trend_is_twelve_parts_of_the_range() {
    let m = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"sum": "rpm"}, "trend": "time"}"#,
        &day(),
        last6(),
    );
    assert_eq!(m.trend.len(), 12);
    assert!(
        m.trend
            .windows(2)
            .all(|w| w[0].x < w[1].x && w[0].y < w[1].y)
    );
    let part = 6.0 * HOUR / 12.0;
    assert_eq!(m.trend[0].span, (T0 + 6.0 * HOUR, T0 + 6.0 * HOUR + part));
    // The linked crosshair finds the part a time falls in, and nothing far off.
    assert_eq!(m.trend_at(T0 + 6.0 * HOUR + 1.5 * part), Some(1));
    assert_eq!(
        figure::hover_at(&m, T0 + 6.0 * HOUR + 1.5 * part),
        Some(TileHover::Trend {
            point: 1,
            card: false
        })
    );
    assert_eq!(figure::hover_at(&m, T0 - 2.0 * HOUR), None);
}

#[test]
fn numbers_are_abbreviated_as_the_format_rules_say() {
    for (v, s) in [
        (1_281_437.0, "1.28M"),
        (12_900.0, "12.9K"),
        (3412.0, "3,412"),
        (182.4, "182"),
        (12.34, "12.3"),
        (0.412, "0.41"),
        (0.004_12, "0.0041"),
        (-41.0, "\u{2212}41"),
        (0.0, "0"),
    ] {
        assert_eq!(short_number(v), s, "{v}");
    }
    let f = build(
        r#"{"type": "stat", "data": "x.csv", "value": {"mean": "rpm"}, "format": ".1f"}"#,
        &day(),
        None,
    );
    assert_eq!(f.text(), "460.0");
}

#[test]
fn a_meter_fills_to_its_share_in_its_highest_status() {
    let t = Table::new(vec![Column::number("gb", vec![10.9])]).unwrap();
    let m = build(
        r#"{"type": "meter", "label": "Memory", "data": "x.csv", "value": {"max": "gb"}, "max": 16, "unit": "GB",
            "thresholds": [{"at": 0.9, "status": "critical"}, {"at": 0.6, "status": "warning"}, {"at": 0, "status": "good"}]}"#,
        &t,
        None,
    );
    assert_eq!(m.kind, TileKind::Meter);
    assert!((m.fraction() - 10.9 / 16.0).abs() < 1e-12);
    assert_eq!(m.status(), Some(Status::Warning));
    // Sorted lowest first.
    let ats: Vec<f64> = m.thresholds.iter().map(|t| t.at).collect();
    assert_eq!(ats, vec![0.0, 0.6, 0.9]);
    // A threshold past the max is an error at its place.
    let bad = spec::parse(
        r#"{"type": "meter", "data": "x.csv", "value": "count", "max": 1, "thresholds": [{"at": 1.5, "status": "good"}]}"#,
        "t.json",
        &tiles_dir(),
        &ReadOptions::new(TimeZone::UTC),
    );
    assert!(bad.unwrap_err().msg.contains("fraction of the max"));
}

#[test]
fn a_sparkline_sums_rows_that_share_an_x_and_shows_the_newest() {
    let t = Table::new(vec![
        Column::time("time", vec![T0, T0 + 60.0, T0 + 60.0, T0 + 120.0]),
        Column::number("rpm", vec![5.0, 3.0, 4.0, 8.0]),
    ])
    .unwrap();
    let m = build(
        r#"{"type": "sparkline", "data": "x.csv", "x": "time", "y": "rpm"}"#,
        &t,
        None,
    );
    let ys: Vec<f64> = m.trend.iter().map(|p| p.y).collect();
    assert_eq!(ys, vec![5.0, 7.0, 8.0]);
    assert_eq!((m.value, m.label.as_str()), (8.0, "rpm"));
}

#[test]
fn a_missing_field_or_text_for_numbers_is_a_mismatch() {
    let t = day();
    let fig = figure_of(r#"{"type": "stat", "data": "x.csv", "value": {"sum": "nope"}}"#);
    let e = TileModel::build(&fig, &t, None).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Mismatch);
    assert!(
        e.msg
            .contains("\"nope\" isn't in the data: it has time, rpm, errors, host"),
        "{}",
        e.msg
    );
    let fig = figure_of(r#"{"type": "stat", "data": "x.csv", "value": {"sum": "host"}}"#);
    let e = TileModel::build(&fig, &t, None).unwrap_err();
    assert!(
        e.msg.contains("must hold numbers, and it holds categories"),
        "{}",
        e.msg
    );
    let chart = figure_of(r#"{"type": "line", "data": "x.csv", "x": "time", "y": "rpm"}"#);
    assert!(TileModel::build(&chart, &t, None).is_err());
}

/// m4's four tiles, from the fixtures, over their last six hours.
fn m4_tiles() -> Vec<TileModel> {
    let opt = ReadOptions::new(TimeZone::UTC);
    let text = std::fs::read_to_string(tiles_dir().join("service.csv")).unwrap();
    let table = die_fig::data::csv::read_csv(&text, &opt).unwrap();
    let end = T0 + 12.0 * HOUR;
    [
        "requests.json",
        "error-rate.json",
        "p95-latency.json",
        "sessions.json",
    ]
    .iter()
    .map(|f| {
        let spec = std::fs::read_to_string(tiles_dir().join(f)).unwrap();
        TileModel::build_in(
            &figure_of(&spec),
            &table,
            Some((end - 6.0 * HOUR, end)),
            &TimeZone::UTC,
        )
        .unwrap()
    })
    .collect()
}

#[test]
fn m4s_tiles_read_m4s_numbers() {
    let t = m4_tiles();
    let read: Vec<(String, Option<String>, String)> = t
        .iter()
        .map(|m| (m.text(), m.unit.clone(), m.change_text().unwrap().1))
        .collect();
    assert_eq!(
        read,
        vec![
            ("1.28M".into(), None, "4.2 % vs previous 6h".into()),
            (
                "0.41".into(),
                Some("%".into()),
                "0.08 pt vs previous 6h".into()
            ),
            (
                "182".into(),
                Some("ms".into()),
                "12 ms vs previous 6h".into()
            ),
            ("3,412".into(), None, "2.1 % vs previous 6h".into()),
        ]
    );
    let good: Vec<Option<bool>> = t.iter().map(|m| m.change.as_ref().unwrap().good).collect();
    assert_eq!(good, vec![Some(true), Some(true), Some(false), Some(true)]);
}

#[test]
fn a_stat_at_m4s_size_is_laid_out_as_m4s() {
    let r = Rect::new(28.0, 92.0, 197.0, 100.0);
    for m in m4_tiles() {
        let l = draw::layout(&m, r, &mut FixedMeasure);
        let at = |role: Role| l.pieces.iter().find(|p| p.role == role).expect("a piece");
        assert_eq!((at(Role::Label).x, at(Role::Label).y), (44.0, 118.0));
        assert_eq!(at(Role::Value).y, 152.0);
        assert_eq!(at(Role::Delta).y, 176.0);
        assert_eq!(l.value_size, 26.0);
        let trend = l.trend.expect("a trend");
        assert_eq!((trend.w, trend.h), (58.0, 22.0));
        assert_eq!(trend.right(), r.right() - 16.0);
        assert!(l.arrow.is_some(), "an arrow always: {}", m.label);
    }
}

#[test]
fn tiles_degrade_with_their_size_and_the_value_never_cuts() {
    for m in m4_tiles() {
        let mut last_value = 26.0;
        for w in [197.0, 160.0, 120.0, 98.0, 80.0, 64.0] {
            for h in [100.0, 72.0, 48.0] {
                let r = Rect::new(0.0, 0.0, w, h);
                let l = draw::layout(&m, r, &mut FixedMeasure);
                let value = l.pieces.iter().find(|p| p.role == Role::Value).unwrap();
                assert_eq!(value.text, m.text(), "never cut: {w}x{h}");
                assert!(l.value_size >= 18.0);
                // Nothing overlaps, nothing leaves the tile's inside.
                let boxes = text_boxes(&l);
                for (i, (ta, a)) in boxes.iter().enumerate() {
                    for (tb, b) in &boxes[i + 1..] {
                        assert!(!overlaps(a, b), "{} {w}x{h}: {ta} over {tb}", m.label);
                    }
                    if let Some(t) = l.trend {
                        assert!(!overlaps(a, &t), "{} {w}x{h}: {ta} over the trend", m.label);
                    }
                }
                if h == 100.0 {
                    assert!(l.value_size <= last_value);
                    last_value = l.value_size;
                }
            }
        }
        // Half m4's width: no trend, the delta's amount only.
        let half = draw::layout(&m, Rect::new(0.0, 0.0, 98.0, 100.0), &mut FixedMeasure);
        assert!(half.trend.is_none());
        let delta = half.pieces.iter().find(|p| p.role == Role::Delta).unwrap();
        assert!(!delta.text.contains("vs"), "{}", delta.text);
    }
}

#[test]
fn a_meter_keeps_its_status_icon_at_every_width() {
    let t = Table::new(vec![Column::number("gb", vec![14.6])]).unwrap();
    let m = build(
        r#"{"type": "meter", "label": "Memory", "data": "x.csv", "value": {"max": "gb"}, "max": 16, "unit": "GB",
            "thresholds": [{"at": 0.6, "status": "warning"}, {"at": 0.9, "status": "critical"}]}"#,
        &t,
        None,
    );
    for w in [420.0, 300.0, 200.0, 150.0, 120.0] {
        let l = draw::layout(&m, Rect::new(0.0, 0.0, w, 64.0), &mut FixedMeasure);
        assert_eq!(l.icon.map(|i| i.0), Some(Status::Critical), "{w}");
        let boxes = text_boxes(&l);
        let (_, (cx, cy)) = l.icon.unwrap();
        let icon = Rect::new(cx - 6.0, cy - 6.0, 12.0, 12.0);
        for (i, (ta, a)) in boxes.iter().enumerate() {
            assert!(
                a.x >= 16.0 - 0.5 && a.right() <= w - 16.0 + 0.5,
                "{w}: {ta}"
            );
            assert!(!overlaps(a, &icon), "{w}: {ta} over the icon");
            for (tb, b) in &boxes[i + 1..] {
                assert!(!overlaps(a, b), "{w}: {ta} over {tb}");
            }
        }
        let word = l.pieces.iter().any(|p| p.role == Role::Status);
        let of = l.pieces.iter().any(|p| p.text.contains(" of "));
        // `of max` goes before the word does.
        assert!(!of || word, "{w}");
        let track = l.track.unwrap();
        assert_eq!((track.h, track.w), (8.0, w - 32.0));
    }
    let wide = draw::layout(&m, Rect::new(0.0, 0.0, 420.0, 64.0), &mut FixedMeasure);
    let t: Vec<&str> = wide.pieces.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(
        t,
        vec!["Memory", " \u{b7} 14.6 of 16 GB", "91.2 %", "critical"]
    );
}

#[test]
fn the_pointer_finds_the_trend_or_the_tile() {
    let m = &m4_tiles()[0];
    let r = Rect::new(0.0, 0.0, 197.0, 100.0);
    let l = draw::layout(m, r, &mut FixedMeasure);
    let t = l.trend.unwrap();
    assert!(matches!(
        draw::hit_with(m, r, (t.right() - 1.0, t.y + t.h / 2.0), &mut FixedMeasure),
        Some(TileHover::Trend {
            point: 11,
            card: true
        })
    ));
    assert!(matches!(
        figure::hit(m, r, (30.0, 40.0)),
        Some(TileHover::Tile { .. })
    ));
    assert_eq!(figure::hit(m, r, (230.0, 40.0)), None);
}

#[test]
fn the_cards_say_what_the_value_is_and_keep_off_the_tile() {
    let m = &m4_tiles()[0];
    let theme = Theme::mono_dark();
    let r = Rect::new(16.0, 16.0, 197.0, 100.0);
    let area = Rect::new(0.0, 0.0, 600.0, 300.0);
    let over = figure::overlay_in(
        m,
        Some(TileHover::Tile { at: (60.0, 60.0) }),
        r,
        area,
        &theme,
        &mut FixedMeasure,
    );
    let t = scene_texts(&over);
    for want in ["1,281,437", "sum of rpm", "previous 6h", "change, good"] {
        assert!(t.iter().any(|s| s.contains(want)), "{want} in {t:?}");
    }
    let card = over.items.iter().find_map(|i| match i {
        Item::Shadow { rect, .. } => Some(*rect),
        _ => None,
    });
    let card = card.expect("a card");
    assert!(!overlaps(&card, &r) && area.contains(card.x, card.y));
    // Over the trend: the point's part and its value, the mini crosshair, no tile card.
    let over = figure::overlay_in(
        m,
        Some(TileHover::Trend {
            point: 8,
            card: true,
        }),
        r,
        area,
        &theme,
        &mut FixedMeasure,
    );
    let t = scene_texts(&over);
    assert!(
        t.iter().any(|s| s.starts_with("10:00 \u{2013} 10:30")),
        "{t:?}"
    );
    // A linked crosshair draws no card.
    let linked = figure::overlay(
        m,
        figure::hover_at(m, m.trend[8].x),
        r,
        &theme,
        &mut FixedMeasure,
    );
    assert!(scene_texts(&linked).is_empty());
    assert!(!linked.items.is_empty());
}

#[test]
fn the_detail_tree_has_the_value_its_range_and_its_change() {
    let m = &m4_tiles()[1];
    let d = figure::detail(m);
    let rows: Vec<(String, String)> = d
        .rows
        .iter()
        .map(|r| (r.key.clone(), r.value.clone()))
        .collect();
    let has = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.clone());
    assert!(has("value").unwrap().ends_with(" %"));
    assert_eq!(has("aggregate").as_deref(), Some("mean of error_pct"));
    assert!(has("range").unwrap().starts_with("06:00 \u{2013} 12:00"));
    assert!(has("previous 6h").is_some());
    assert_eq!(
        has("change").as_deref(),
        Some("\u{25bc} 0.08 pt \u{b7} good")
    );
}

#[test]
fn tiles_draw_inside_their_rect_dark_and_light() {
    for theme in [Theme::mono_dark(), Theme::light()] {
        for m in m4_tiles() {
            let r = Rect::new(10.0, 20.0, 197.0, 100.0);
            let s = figure::base(&m, r, &theme, &mut FixedMeasure);
            let b = s.bounds().expect("something drawn");
            assert!(
                r.inflate(0.5).contains(b.x, b.y) && r.inflate(0.5).contains(b.right(), b.bottom())
            );
            assert!(scene_texts(&s).contains(&m.label));
        }
    }
}

#[test]
fn tile_specs_write_back_the_same() {
    for f in [
        "requests.json",
        "error-rate.json",
        "sessions.json",
        "sparkline.json",
        "meter-warning.json",
        "meter-plain.json",
    ] {
        let text = std::fs::read_to_string(tiles_dir().join(f)).unwrap();
        let a = figure_of(&text);
        let again = figure_of(&spec::write(&a));
        assert_eq!(a, again, "{f}");
    }
}
