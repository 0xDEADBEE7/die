//! The chart: its model, layout, marks, overlay, hover and navigation, over the synthetic
//! fixtures in `examples/data` and tables made here, measured with `FixedMeasure` so no font is
//! needed. Times are in UTC.

use die_test_cap as _;

use die_fig::data::{Column, ReadOptions, SeriesKey, Table};
use die_fig::interact::consts::{BRUSH_MIN, HIT_POINT, NEAREST_SERIES};
use die_fig::interact::{Action, Axis, BrushKind, Cursor, View};
use die_fig::plot::layout::{Role, text_box};
use die_fig::plot::marks::on_fill;
use die_fig::plot::overlay::WASH;
use die_fig::plot::{self, ChartLayout, ChartModel, OverlayState};
use die_fig::scene::{FixedMeasure, Hit, Item, Paint, Rect, Scene, Verb};
use die_fig::spec::{self, Figure};
use die_fig::theme::{Color, Theme};
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FIXTURES: &[&str] = &[
    "requests.csv",
    "requests.fig.json",
    "latency.json",
    "sine.json",
    "services-bars.csv",
    "scatter.csv",
    "many.json",
    "stacked-area.json",
    "thresholds.json",
    "gaps.csv",
];
const SIZES: &[(f32, f32)] = &[(400.0, 240.0), (880.0, 470.0), (1800.0, 1000.0)];

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
    let parsed = spec::parse(&text, name, &dir, &opt()).expect("it parses");
    let Figure::Chart(chart) = parsed.figure else {
        panic!("{name} is a chart");
    };
    let tables = plot::read_tables(&chart, Some(&text), &opt()).expect("its data");
    ChartModel::build(chart, tables, &TimeZone::UTC).expect("its model")
}

/// A model from a table made here, by the guess.
fn guessed(table: Table) -> ChartModel {
    let chart = spec::guess(&table).expect("a guess");
    ChartModel::build(chart, vec![Arc::new(table)], &TimeZone::UTC).expect("a model")
}

/// A model from spec text whose data is inline.
fn from_spec(text: &str) -> ChartModel {
    let parsed = spec::parse(text, "t.json", &data_dir(), &opt()).expect("it parses");
    let Figure::Chart(chart) = parsed.figure else {
        panic!("a chart");
    };
    let tables = plot::read_tables(&chart, Some(text), &opt()).expect("its data");
    ChartModel::build(chart, tables, &TimeZone::UTC).expect("its model")
}

fn lay(
    model: &ChartModel,
    view: &View,
    size: (f32, f32),
) -> (Scene, ChartLayout, die_fig::algo::HitIndex) {
    plot::base(
        model,
        view,
        size,
        1.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    )
}

fn texts(scene: &Scene) -> Vec<(String, Rect)> {
    let mut m = FixedMeasure;
    scene
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
            } => Some((
                text.to_string(),
                text_box(text, *x, *y, *spec, *anchor, &mut m),
            )),
            _ => None,
        })
        .collect()
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.intersect(b).is_some_and(|r| r.w > 0.5 && r.h > 0.5)
}

fn key(name: &str) -> SeriesKey {
    SeriesKey::new(name, 0)
}

fn overlay(model: &ChartModel, l: &ChartLayout, view: &View, st: &OverlayState) -> Scene {
    plot::overlay(model, l, view, st, &Theme::mono_dark(), &mut FixedMeasure)
}

fn state() -> OverlayState {
    OverlayState {
        hover: None,
        cursor: None,
        pointer: None,
        brush: None,
        emphasis: None,
    }
}

fn shadow(scene: &Scene) -> Option<Rect> {
    scene.items.iter().find_map(|i| match i {
        Item::Shadow { rect, .. } => Some(*rect),
        _ => None,
    })
}

fn has_text(scene: &Scene, s: &str) -> bool {
    texts(scene).iter().any(|(t, _)| t == s)
}

// ---------------------------------------------------------------- layout

#[test]
fn every_fixture_lays_out_without_overlaps_inside_the_area_at_three_sizes() {
    for name in FIXTURES {
        let model = load(name);
        let view = model.initial_view();
        for &size in SIZES {
            let (scene, l, _) = lay(&model, &view, size);
            let area = Rect::new(0.0, 0.0, size.0, size.1);
            let p = l.plot;
            assert!(
                p.w >= 0.5 * size.0 && p.h >= 0.35 * size.1,
                "{name} {size:?}: the plot {p:?}"
            );
            assert!(
                p.x >= 0.0 && p.y >= 0.0 && p.right() <= size.0 && p.bottom() <= size.1,
                "{name} {size:?}"
            );
            let boxes = texts(&scene);
            for (t, b) in &boxes {
                assert!(
                    b.x >= -0.5
                        && b.y >= -0.5
                        && b.right() <= area.right() + 0.5
                        && b.bottom() <= area.bottom() + 0.5,
                    "{name} {size:?}: {t:?} at {b:?} leaves the area"
                );
            }
            for (i, (ta, a)) in boxes.iter().enumerate() {
                for (tb, b) in &boxes[i + 1..] {
                    assert!(
                        !overlaps(a, b),
                        "{name} {size:?}: {ta:?} {a:?} overlaps {tb:?} {b:?}"
                    );
                }
            }
            // Regions: the plot, the axes' strips beside it, a key a series in the legend.
            assert!(
                scene
                    .regions
                    .iter()
                    .any(|r| r.hit == Hit::Plot(0) && r.rect == p),
                "{name}: the plot's region"
            );
            assert_eq!(
                scene.hit(1.0, 1.0),
                Some(&Hit::Chrome),
                "{name}: the corner is chrome"
            );
            if model.chart.view.axes {
                assert_eq!(l.x_axis.y, p.bottom(), "{name}");
                assert_eq!(l.y_axis.right(), p.x, "{name}");
                assert_eq!(
                    scene.hit(p.x - 2.0, p.y + p.h / 2.0),
                    Some(&Hit::AxisY(0)),
                    "{name}"
                );
                assert_eq!(
                    scene.hit(p.x + p.w / 2.0, p.bottom() + 4.0),
                    Some(&Hit::AxisX(0)),
                    "{name}"
                );
            }
            for (k, r) in &l.legend {
                let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                assert_eq!(
                    scene.hit(at.0, at.1),
                    Some(&Hit::LegendKey(k.clone())),
                    "{name} {size:?}"
                );
                assert!(
                    r.right() <= size.0 + 2.5,
                    "{name} {size:?}: legend key {k:?} past the edge"
                );
            }
        }
    }
}

#[test]
fn the_title_is_semibold_strong_the_subtitle_muted_and_the_ticks_tabular() {
    let model = load("requests.fig.json");
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let title = l
        .labels
        .iter()
        .find(|x| x.role == Role::Title && x.text == "Requests per minute")
        .expect("the title");
    assert_eq!(title.spec.size, 15.0);
    assert_eq!(title.spec.weight, die_fig::scene::Weight::Semibold);
    let sub = l
        .labels
        .iter()
        .find(|x| x.text.starts_with("Last 6 hours"))
        .expect("the subtitle");
    assert_eq!(sub.spec.size, 12.0);
    assert_eq!(sub.ink, plot::layout::Ink::Muted);
    for t in l
        .labels
        .iter()
        .filter(|x| matches!(x.role, Role::XTick | Role::YTick))
    {
        assert_eq!(t.spec.size, 11.0);
        assert!(t.spec.tabular, "{}", t.text);
        assert_eq!(t.ink, plot::layout::Ink::Muted);
    }
    // The y axis's title sits above the ticks, over the plot's top.
    let yt = l
        .labels
        .iter()
        .find(|x| x.text == "req / min")
        .expect("the y title");
    assert!(yt.y < l.plot.y);
    assert!(l.title.contains(title.x + 1.0, title.y - 2.0));
}

#[test]
fn a_small_area_drops_the_subtitle_then_the_legend_rows_then_the_end_labels() {
    let model = load("requests.fig.json");
    let (_, big, _) = lay(&model, &View::default(), (880.0, 470.0));
    assert!(
        big.labels
            .iter()
            .any(|x| x.text.starts_with("Last 6 hours"))
    );
    assert_eq!(
        big.ends.len(),
        3,
        "three lines that don't collide get end labels"
    );
    let (_, small, _) = lay(&model, &View::default(), (400.0, 200.0));
    assert!(
        !small
            .labels
            .iter()
            .any(|x| x.text.starts_with("Last 6 hours")),
        "the subtitle went first"
    );
    assert!(!small.items.is_empty(), "the legend stays");
    let many = load("many.json");
    let (_, l, _) = lay(&many, &View::default(), (400.0, 240.0));
    let rows: std::collections::BTreeSet<i32> =
        l.items.iter().map(|i| i.swatch.y.round() as i32).collect();
    assert_eq!(rows.len(), 1, "one legend row at the smallest size");
    assert!(
        l.labels.iter().any(|x| x.text.ends_with("more")),
        "and \"+N more\""
    );
    assert!(l.ends.is_empty(), "twelve lines get no end labels");
}

#[test]
fn x_labels_are_at_least_64_px_apart_and_y_ticks_about_48() {
    for name in FIXTURES {
        let model = load(name);
        for &size in SIZES {
            let (_, l, _) = lay(&model, &View::default(), size);
            let xs: Vec<f32> = l
                .labels
                .iter()
                .filter(|x| x.role == Role::XTick)
                .map(|x| x.x)
                .collect();
            if !model.is_band() {
                let centres: Vec<f32> = l.x_ticks.iter().map(|t| l.x.map(t.value)).collect();
                for w in centres.windows(2) {
                    assert!(w[1] - w[0] >= 63.5, "{name} {size:?}: x ticks {centres:?}");
                }
            }
            assert!(!xs.is_empty(), "{name} {size:?}: some x labels");
            if !model.horizontal && !model.is_heatmap() {
                let ys: Vec<f32> = l.y_ticks.iter().map(|t| l.y.map(t.value)).collect();
                assert!(ys.len() >= 2, "{name} {size:?}: {ys:?}");
                let gap = (ys[0] - ys[1]).abs();
                assert!(
                    (24.0..=110.0).contains(&gap),
                    "{name} {size:?}: y ticks {gap} apart"
                );
            }
        }
    }
}

#[test]
fn an_axis_that_reaches_ten_thousand_is_compact() {
    let t = Table::new(vec![
        Column::time("t", (0..20).map(|i| f64::from(i) * 60.0).collect()),
        Column::number(
            "hits",
            (0..20).map(|i| 1000.0 + f64::from(i) * 2500.0).collect(),
        ),
    ])
    .expect("a table");
    let model = guessed(t);
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let ticks: Vec<&str> = l
        .labels
        .iter()
        .filter(|x| x.role == Role::YTick)
        .map(|x| x.text.as_str())
        .collect();
    assert!(ticks.iter().any(|t| t.ends_with('K')), "{ticks:?}");
    assert!(!ticks.iter().any(|t| t.contains(',')), "{ticks:?}");
}

#[test]
fn one_series_has_no_legend_two_have_line_keys_bars_have_squares() {
    let t = Table::new(vec![
        Column::time("t", (0..10).map(|i| f64::from(i) * 60.0).collect()),
        Column::number("v", (0..10).map(f64::from).collect()),
    ])
    .expect("a table");
    let (_, l, _) = lay(&guessed(t), &View::default(), (880.0, 470.0));
    assert!(l.items.is_empty() && l.legend.is_empty());
    let (_, l, _) = lay(&load("requests.csv"), &View::default(), (880.0, 470.0));
    assert!(
        l.items
            .iter()
            .all(|i| i.shape == plot::layout::KeyShape::Line)
    );
    let (_, l, _) = lay(&load("latency.json"), &View::default(), (880.0, 470.0));
    assert!(
        l.items
            .iter()
            .all(|i| i.shape == plot::layout::KeyShape::Square && i.swatch.w == 10.0)
    );
    let (_, l, _) = lay(&load("scatter.csv"), &View::default(), (880.0, 470.0));
    assert!(
        l.items
            .iter()
            .all(|i| i.shape == plot::layout::KeyShape::Dot)
    );
}

#[test]
fn a_hidden_series_keeps_its_slot_and_its_key_goes_hollow() {
    let model = load("requests.csv");
    let theme = Theme::mono_dark();
    let mut view = View::default();
    view.hidden.insert(key("api"));
    assert_eq!(
        model.color_of_key(&key("worker"), &theme),
        theme.series[1],
        "colour follows the entity"
    );
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let api = l
        .items
        .iter()
        .find(|i| i.key == key("api"))
        .expect("its key");
    assert!(api.hidden && api.name.alpha < 1.0);
    let all = View {
        hidden: model.keys().into_iter().collect(),
        ..View::default()
    };
    let (scene, _, _) = lay(&model, &all, (880.0, 470.0));
    assert!(has_text(&scene, "All series hidden"));
}

#[test]
fn series_past_the_eighth_draw_in_the_neutral() {
    let model = load("many.json");
    let theme = Theme::mono_dark();
    let keys = model.keys();
    assert_eq!(keys.len(), 12);
    for (i, k) in keys.iter().enumerate() {
        let c = model.color_of_key(k, &theme);
        if i < 8 {
            assert_eq!(c, theme.series[i], "{k:?}");
        } else {
            assert_eq!(c, theme.other, "{k:?}");
        }
    }
}

// ---------------------------------------------------------------- marks

fn strokes(scene: &Scene, color: Color) -> Vec<&Vec<Verb>> {
    scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Path {
                path,
                stroke: Some(s),
                ..
            } if s.paint == Paint::solid(color, 1.0) && !s.hairline => Some(&path.verbs),
            _ => None,
        })
        .collect()
}

#[test]
fn lines_are_2_px_round_and_a_long_series_is_decimated_to_the_pixel_columns() {
    let n = 200_000;
    let t = Table::new(vec![
        Column::number("x", (0..n).map(f64::from).collect()),
        Column::number(
            "y",
            (0..n)
                .map(|i| (f64::from(i) / 300.0).sin() * 50.0 + f64::from(i % 7))
                .collect(),
        ),
    ])
    .expect("a table");
    let model = guessed(t);
    let theme = Theme::mono_dark();
    let (scene, l, _) = lay(&model, &View::default(), (400.0, 240.0));
    let paths: Vec<_> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Path {
                path,
                stroke: Some(s),
                ..
            } if s.paint == Paint::solid(theme.series[0], 1.0) => Some((path, s)),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 1);
    let (path, s) = paths[0];
    assert_eq!(s.width, 2.0);
    assert_eq!(s.cap, die_fig::scene::Cap::Round);
    assert_eq!(s.join, die_fig::scene::Join::Round);
    assert!(
        path.verbs.len() <= (l.plot.w as usize + 2) * 4,
        "{} verbs over {} columns",
        path.verbs.len(),
        l.plot.w
    );
}

#[test]
fn a_missing_value_breaks_a_line() {
    let model = load("gaps.csv");
    let theme = Theme::mono_dark();
    let (scene, _, _) = lay(&model, &View::default(), (880.0, 470.0));
    // inbound misses rows 14-18 and 30: three runs; outbound misses 22-25: two.
    assert_eq!(strokes(&scene, theme.series[0]).len(), 3);
    assert_eq!(strokes(&scene, theme.series[1]).len(), 2);
    // Where a series has a gap, its row in the card says so.
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let c = Cursor::Point {
        series: key("outbound"),
        row: 16,
    };
    let o = overlay(
        &model,
        &l,
        &View::default(),
        &OverlayState {
            cursor: Some(c),
            ..state()
        },
    );
    assert!(has_text(&o, "—"));
}

#[test]
fn an_area_is_a_wash_from_16_to_3_percent_and_a_stack_is_flat() {
    let t = Table::new(vec![
        Column::time("t", (0..10).map(|i| f64::from(i) * 60.0).collect()),
        Column::number("v", (0..10).map(|i| f64::from(i) + 1.0).collect()),
    ])
    .expect("a table");
    let chart = {
        let mut c = spec::guess(&t).expect("a guess");
        c.layers[0].mark.kind = die_fig::spec::MarkKind::Area;
        c
    };
    let model = ChartModel::build(chart, vec![Arc::new(t)], &TimeZone::UTC).expect("a model");
    let (scene, _, _) = lay(&model, &View::default(), (880.0, 470.0));
    let wash = scene.items.iter().find_map(|i| match i {
        Item::Path {
            fill: Some(Paint::Linear { stops, .. }),
            ..
        } => Some(stops.clone()),
        _ => None,
    });
    let stops = wash.expect("a gradient");
    assert_eq!((stops[0].2, stops[1].2), (0.16, 0.03));
    let stacked = load("stacked-area.json");
    let (scene, l, _) = lay(&stacked, &View::default(), (880.0, 470.0));
    assert!(l.stacks.hi.iter().all(Option::is_some));
    let flats = scene
        .items
        .iter()
        .filter(|i| matches!(i, Item::Path { fill: Some(Paint::Solid { alpha, .. }), stroke: None, .. } if *alpha == plot::marks::STACK_WASH))
        .count();
    assert_eq!(flats, 4);
}

#[test]
fn bars_are_at_most_24_px_with_a_rounded_data_end_and_a_2_px_gap_between_segments() {
    let model = load("latency.json");
    for &size in SIZES {
        let (scene, l, _) = lay(&model, &View::default(), size);
        assert_eq!(l.bars.len(), 12, "six bands, two segments each");
        for b in &l.bars {
            assert!(b.rect.w <= 24.0 + 1e-3, "{size:?}: {:?}", b.rect);
            assert_eq!(b.radii[2], 0.0, "square at the baseline");
            assert_eq!(b.radii[3], 0.0);
            let hit = scene.hit(b.rect.x + b.rect.w / 2.0, b.rect.y + b.rect.h / 2.0);
            assert!(
                matches!(hit, Some(Hit::Bar { band, .. }) if *band == b.band),
                "{hit:?}"
            );
        }
        for band in 0..6 {
            let seg: Vec<_> = l.bars.iter().filter(|b| b.band == band).collect();
            let success = seg
                .iter()
                .find(|b| model.series[b.series].key == key("success"))
                .expect("success");
            let failure = seg
                .iter()
                .find(|b| model.series[b.series].key == key("failure"))
                .expect("failure");
            assert!(failure.end && !success.end);
            assert!(
                failure.radii[0] > 0.0 && success.radii[0] == 0.0,
                "only the stack's end is rounded"
            );
            assert!(
                (success.rect.y - failure.rect.bottom() - 2.0).abs() < 0.01,
                "the surface's gap"
            );
            assert!(
                (success.rect.bottom() - l.plot.bottom()).abs() < 0.5,
                "on the baseline"
            );
        }
    }
    // Grouped bars stand side by side, 2 px apart.
    let model = load("services-bars.csv");
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let pair: Vec<_> = l.bars.iter().filter(|b| b.band == 0).collect();
    assert_eq!(pair.len(), 2);
    assert!((pair[1].rect.x - pair[0].rect.right() - 2.0).abs() < 0.01);
    assert!(pair.iter().all(|b| b.radii[0] == 4.0 && b.rect.w <= 24.0));
}

#[test]
fn points_are_8_px_with_a_2_px_surface_ring() {
    let model = load("scatter.csv");
    let theme = Theme::mono_dark();
    let (scene, _, hits) = lay(&model, &View::default(), (880.0, 470.0));
    let mut rings = 0;
    for w in scene.items.windows(2) {
        if let (
            Item::Path {
                path: a,
                fill: Some(Paint::Solid { color: ca, .. }),
                ..
            },
            Item::Path {
                path: b,
                fill: Some(Paint::Solid { color: cb, .. }),
                ..
            },
        ) = (&w[0], &w[1])
            && *ca == theme.surface
            && *cb == theme.series[0]
        {
            let r = |p: &die_fig::scene::PathData| {
                let xs: Vec<f32> = p
                    .verbs
                    .iter()
                    .filter_map(|v| match v {
                        Verb::Move(x, _) | Verb::Line(x, _) => Some(*x),
                        Verb::Cubic(_, _, _, _, x, _) | Verb::Quad(_, _, x, _) => Some(*x),
                        Verb::Close => None,
                    })
                    .collect();
                (xs.iter().cloned().fold(f32::MIN, f32::max)
                    - xs.iter().cloned().fold(f32::MAX, f32::min))
                    / 2.0
            };
            assert!((r(b) - 4.0).abs() < 0.05 && (r(a) - 6.0).abs() < 0.05);
            rings += 1;
        }
    }
    assert_eq!(rings, 40, "every web point");
    assert!(hits.nearest_point(0.0, 0.0, 1.0).is_none());
}

#[test]
fn a_rule_with_a_label_is_dashed_and_says_it() {
    let model = load("thresholds.json");
    assert_eq!(model.rules.len(), 1);
    let (scene, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let y = l.y.map(250.0);
    let dashed = scene.items.iter().any(|i| {
        matches!(i, Item::Path { stroke: Some(s), path, .. }
        if s.dash.is_some() && path.verbs.first() == Some(&Verb::Move(l.plot.x, y)))
    });
    assert!(dashed);
    let (_, b) = texts(&scene)
        .into_iter()
        .find(|(t, _)| t == "SLO 250 ms")
        .expect("its label");
    assert!(
        b.bottom() <= y && b.right() <= l.plot.right(),
        "above the line, at its end"
    );
}

#[test]
fn a_label_on_a_filled_mark_is_white_or_near_black_by_luminance() {
    assert_eq!(on_fill(Color(0x39, 0x87, 0xe5)), Color(255, 255, 255));
    assert_eq!(on_fill(Color(0x1a, 0x1a, 0x1a)), Color(255, 255, 255));
    assert_eq!(on_fill(Color(0xed, 0xa1, 0x00)), Color(17, 17, 17));
    assert_eq!(on_fill(Color(0xee, 0xee, 0xee)), Color(17, 17, 17));
}

#[test]
fn band_totals_sit_over_their_bars_in_muted_text() {
    let model = load("latency.json");
    let (scene, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let ts = texts(&scene);
    let (_, b) = ts
        .iter()
        .find(|(t, _)| t == "2,672")
        .expect("10–25's total");
    let bar = l
        .bars
        .iter()
        .find(|g| g.band == 1 && g.end)
        .expect("its end");
    assert!(b.bottom() <= bar.rect.y && b.x < bar.rect.right() && b.right() > bar.rect.x);
}

// ---------------------------------------------------------------- the model

#[test]
fn the_model_knows_its_kind_keys_extent_and_newest() {
    let r = load("requests.csv");
    assert!(r.is_x_chart() && !r.is_band());
    assert_eq!(r.keys(), vec![key("api"), key("worker"), key("batch")]);
    let ((x0, x1), (y0, _)) = r.extent();
    let start = jiff::civil::date(2026, 11, 21)
        .at(6, 0, 0, 0)
        .to_zoned(TimeZone::UTC)
        .expect("a time")
        .timestamp()
        .as_second() as f64;
    assert_eq!((x0, x1), (start, start + 72.0 * 300.0));
    assert_eq!(y0, 0.0, "lines near zero take it in");
    assert_eq!(r.newest(), Some(start + 72.0 * 300.0));
    let l = load("latency.json");
    assert!(l.is_band() && !l.is_x_chart());
    assert_eq!(l.extent().0, (0.0, 6.0));
    assert_eq!(l.newest(), None);
    let s = load("scatter.csv");
    assert!(!s.is_band() && !s.is_x_chart());
    assert_eq!(plot::brush_kind(&r), BrushKind::X);
    assert_eq!(plot::brush_kind(&l), BrushKind::Bands);
    assert_eq!(plot::brush_kind(&s), BrushKind::Box);
}

#[test]
fn fit_leaves_hidden_series_out_and_fits_y_to_the_x_window() {
    let model = load("requests.csv");
    let all = model.fit(&View::default());
    let mut view = View::default();
    view.hidden.insert(key("api"));
    let without = model.fit(&view);
    assert!(
        without.1.1 < all.1.1,
        "api was the top: {without:?} {all:?}"
    );
    // Bars include zero; a stacked area's y is the stack's.
    let lat = load("latency.json");
    assert_eq!(lat.fit(&View::default()).1.0, 0.0);
    assert!(lat.fit(&View::default()).1.1 >= 2672.0);
    // A window on x fits y to what's in it.
    let ((x0, x1), _) = all;
    let v = View {
        x: Some((x0, x0 + (x1 - x0) * 0.1)),
        ..View::default()
    };
    let w = model.fit(&v);
    assert_eq!(w.0, v.x.expect("set"));
    assert!(w.1.1 <= all.1.1);
}

#[test]
fn labels_name_each_series_and_category_with_where_the_cursor_goes() {
    let model = load("latency.json");
    let ls = model.labels();
    let names: Vec<&str> = ls.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "success",
            "failure",
            "0–10",
            "10–25",
            "25–50",
            "50–100",
            "100–250",
            "250+"
        ]
    );
    assert_eq!(
        ls[3].1,
        Cursor::Band {
            band: 1,
            series: None
        }
    );
    let text = r#"{"type": "line", "series": [
        {"label": "a", "points": [{"x": 0, "y": 1}, {"x": 1, "y": 2}]},
        {"label": "a", "points": [{"x": 0, "y": 3}, {"x": 1, "y": 4}]}]}"#;
    let dup = from_spec(text);
    let ls = dup.labels();
    assert_eq!(ls[0].0, "a");
    assert_eq!(ls[1].0, "a ‹2›");
    assert_eq!(
        ls[1].1,
        Cursor::Point {
            series: SeriesKey::new("a", 1),
            row: 1
        }
    );
}

#[test]
fn tsv_writes_the_view_hidden_series_out_and_the_cursor_s_line() {
    let model = load("requests.csv");
    let (text, line) = model.tsv(&View::default());
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "time\tapi\tworker\tbatch");
    assert_eq!(lines.len(), 74);
    assert!(lines[1].starts_with("2026-11-21T06:00\t"));
    assert_eq!(line, None);
    let mut view = View::default();
    view.hidden.insert(key("worker"));
    let cursor = Cursor::Point {
        series: key("api"),
        row: 10,
    };
    let (text, line) = model.tsv_at(&view, Some(&cursor));
    assert_eq!(text.lines().next(), Some("time\tapi\tbatch"));
    assert_eq!(line, Some(11));
    assert!(
        text.lines()
            .nth(11)
            .expect("the line")
            .starts_with("2026-11-21T06:50\t")
    );
}

// ---------------------------------------------------------------- hover

#[test]
fn hover_snaps_to_the_nearest_x_on_the_series_nearest_in_y() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let s = &model.series[model.index_of(&key("worker")).expect("worker")];
    let i = 30;
    let (px, py) = l.at(s.xs[i], s.ys[i]);
    let c = plot::hover(&model, &l, &hits, &view, px + 2.0, py + 3.0).expect("a hover");
    assert_eq!(
        c,
        Cursor::Point {
            series: key("worker"),
            row: i as u32
        }
    );
    assert_eq!(
        plot::nearest_series(&model, &l, &hits, &view, px + 2.0, py + 3.0),
        Some(key("worker"))
    );
    // Far from every line in y: still the column's nearest, but no emphasis.
    let empty_y = l.plot.bottom() - 2.0;
    let far = plot::hover(&model, &l, &hits, &view, px, empty_y);
    assert!(matches!(far, Some(Cursor::Point { row: 30, .. })));
    let batch = &model.series[model.index_of(&key("batch")).expect("batch")];
    let by = l.at(batch.xs[i], batch.ys[i]).1;
    if (empty_y - by).abs() > NEAREST_SERIES {
        assert_eq!(
            plot::nearest_series(&model, &l, &hits, &view, px, empty_y),
            None
        );
    }
    // Off the plot: nothing.
    assert_eq!(
        plot::hover(&model, &l, &hits, &view, l.plot.x - 10.0, py),
        None
    );
    assert_eq!(
        plot::hover(&model, &l, &hits, &view, px, l.plot.y - 10.0),
        None
    );
}

#[test]
fn hover_on_bars_is_the_band_and_the_segment_under_the_pointer() {
    let model = load("latency.json");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let success = l
        .bars
        .iter()
        .find(|b| b.band == 2 && model.series[b.series].key == key("success"))
        .expect("a bar");
    let on = (success.rect.x + 3.0, success.rect.y + 10.0);
    assert_eq!(
        plot::hover(&model, &l, &hits, &view, on.0, on.1),
        Some(Cursor::Band {
            band: 2,
            series: Some(key("success"))
        })
    );
    assert_eq!(
        plot::nearest_series(&model, &l, &hits, &view, on.0, on.1),
        Some(key("success"))
    );
    // Above the stack: the band, its top segment marked, nothing brought forward.
    let above = (success.rect.x + 3.0, l.plot.y + 2.0);
    assert_eq!(
        plot::hover(&model, &l, &hits, &view, above.0, above.1),
        Some(Cursor::Band {
            band: 2,
            series: Some(key("failure"))
        })
    );
    assert_eq!(
        plot::nearest_series(&model, &l, &hits, &view, above.0, above.1),
        None
    );
}

#[test]
fn hover_on_a_scatter_finds_the_nearest_point_within_reach() {
    let model = load("scatter.csv");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let s = &model.series[0];
    let (px, py) = l.at(s.xs[5], s.ys[5]);
    let c = plot::hover(&model, &l, &hits, &view, px + 3.0, py - 2.0).expect("a point");
    let Cursor::Point { series, row } = c else {
        panic!()
    };
    let t = &model.series[model.index_of(&series).expect("its series")];
    let q = l.at(t.xs[row as usize], t.ys[row as usize]);
    assert!((q.0 - px - 3.0).hypot(q.1 - py + 2.0) <= HIT_POINT);
    // An empty corner of the plot finds nothing.
    assert_eq!(
        plot::hover(
            &model,
            &l,
            &hits,
            &view,
            l.plot.x + 2.0,
            l.plot.bottom() - 2.0
        ),
        None
    );
}

// ---------------------------------------------------------------- nav

fn walk(
    model: &ChartModel,
    l: &ChartLayout,
    view: &View,
    c: Option<&Cursor>,
    a: Action,
    n: u32,
) -> Option<Cursor> {
    plot::motion(model, l, view, c, &a, n)
}

#[test]
fn motions_place_the_cursor_and_walk_the_data() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    // l places it at the view's left edge, h at its right.
    let start = walk(&model, &l, &view, None, Action::Step(1), 1).expect("placed");
    assert!(matches!(start, Cursor::Point { row: 0, .. }));
    let end = walk(&model, &l, &view, None, Action::Step(-1), 1).expect("placed");
    assert!(matches!(end, Cursor::Point { row: 72, .. }));
    // w and b need a cursor; j and k place it.
    assert_eq!(walk(&model, &l, &view, None, Action::Peak(1), 1), None);
    assert!(walk(&model, &l, &view, None, Action::Series(1), 1).is_some());
    let c = Cursor::Point {
        series: key("api"),
        row: 10,
    };
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Step(1), 3),
        Some(Cursor::Point {
            series: key("api"),
            row: 13
        })
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Step(-1), 100),
        Some(Cursor::Point {
            series: key("api"),
            row: 0
        })
    );
    let first = Cursor::Point {
        series: key("api"),
        row: 0,
    };
    assert_eq!(
        walk(&model, &l, &view, Some(&first), Action::Step(-1), 1),
        None,
        "nothing before the first"
    );
    // Series(1) goes down the screen: api is on top, then worker.
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Series(1), 1),
        Some(Cursor::Point {
            series: key("worker"),
            row: 10
        })
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Series(-1), 1),
        None,
        "nothing above api"
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Series(1), 2),
        Some(Cursor::Point {
            series: key("batch"),
            row: 10
        })
    );
    // gg and G, H M L.
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Last, 1),
        Some(Cursor::Point {
            series: key("api"),
            row: 72
        })
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::First, 1),
        Some(first.clone())
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::ViewEdge(0), 1),
        Some(Cursor::Point {
            series: key("api"),
            row: 36
        })
    );
    // The batch spikes every twelve samples are its peaks.
    let b = Cursor::Point {
        series: key("batch"),
        row: 1,
    };
    assert_eq!(
        walk(&model, &l, &view, Some(&b), Action::Peak(1), 1),
        Some(Cursor::Point {
            series: key("batch"),
            row: 12
        })
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&b), Action::Peak(1), 2),
        Some(Cursor::Point {
            series: key("batch"),
            row: 24
        })
    );
}

#[test]
fn a_missing_sample_is_stepped_over() {
    let model = load("gaps.csv");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let c = Cursor::Point {
        series: key("inbound"),
        row: 13,
    };
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Step(1), 1),
        Some(Cursor::Point {
            series: key("inbound"),
            row: 19
        })
    );
}

#[test]
fn band_motions_walk_bands_and_segments() {
    let model = load("latency.json");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    assert_eq!(
        walk(&model, &l, &view, None, Action::Step(1), 1),
        Some(Cursor::Band {
            band: 0,
            series: None
        })
    );
    assert_eq!(
        walk(&model, &l, &view, None, Action::Step(-1), 1),
        Some(Cursor::Band {
            band: 5,
            series: None
        })
    );
    let c = Cursor::Band {
        band: 2,
        series: Some(key("failure")),
    };
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Step(1), 2),
        Some(Cursor::Band {
            band: 4,
            series: Some(key("failure"))
        })
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Series(1), 1),
        Some(Cursor::Band {
            band: 2,
            series: Some(key("success"))
        })
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Series(-1), 1),
        None,
        "failure is the top segment"
    );
    assert_eq!(
        walk(&model, &l, &view, Some(&c), Action::Last, 1),
        Some(Cursor::Band {
            band: 5,
            series: Some(key("failure"))
        })
    );
    // 10–25 is the tallest: the peak after 0–10.
    let z = Cursor::Band {
        band: 0,
        series: None,
    };
    assert_eq!(
        walk(&model, &l, &view, Some(&z), Action::Peak(1), 1),
        Some(Cursor::Band {
            band: 1,
            series: None
        })
    );
}

#[test]
fn zoom_keeps_the_value_under_the_pointer_within_its_limits() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let ((x0, x1), _) = model.fit(&view);
    let at = (l.plot.x + l.plot.w * 0.25, l.plot.y + 20.0);
    let under = l.x.invert(at.0);
    let v = plot::zoom(&model, &l, &view, 2.0, Some(at), None, None);
    let (a, b) = v.x.expect("zoomed");
    assert!(((b - a) - (x1 - x0) / 2.0).abs() < 1e-6);
    assert!(
        ((under - a) / (b - a) - 0.25).abs() < 1e-6,
        "the value under the pointer stays"
    );
    assert_eq!(v.y, None, "y keeps fitting on an x-chart");
    // In no further than one interval (five minutes), out no further than the data: the fit.
    let deep = plot::zoom(&model, &l, &view, 1e9, Some(at), None, None);
    let (a, b) = deep.x.expect("zoomed");
    assert!(((b - a) - 300.0).abs() < 1e-6, "{}", b - a);
    assert_eq!(
        plot::zoom(&model, &l, &v, 0.01, Some(at), None, None).x,
        None,
        "out past the data is the fit"
    );
    // About the cursor when there's no pointer.
    let c = Cursor::Point {
        series: key("api"),
        row: 60,
    };
    let k = plot::zoom(&model, &l, &view, 4.0, None, Some(&c), None);
    let (a, b) = k.x.expect("zoomed");
    let cx = model.series[0].xs[60];
    assert!(a <= cx && cx <= b);
}

#[test]
fn bars_never_zoom_their_values_and_scatters_zoom_both_from_half_to_32_times() {
    let lat = load("latency.json");
    let (_, l, _) = lay(&lat, &View::default(), (880.0, 470.0));
    let v = plot::zoom(&lat, &l, &View::default(), 2.0, None, None, Some(Axis::Y));
    assert_eq!(v.y, None);
    let sc = load("scatter.csv");
    let view = View::default();
    let (_, l, _) = lay(&sc, &view, (880.0, 470.0));
    let ((x0, x1), (y0, y1)) = sc.fit(&view);
    let v = plot::zoom(&sc, &l, &view, 2.0, None, None, None);
    let ((a, b), (c, d)) = (v.x.expect("x"), v.y.expect("y"));
    assert!(((b - a) - (x1 - x0) / 2.0).abs() < 1e-9 && ((d - c) - (y1 - y0) / 2.0).abs() < 1e-9);
    let deep = plot::zoom(&sc, &l, &view, 1e6, None, None, None);
    let (a, b) = deep.x.expect("x");
    assert!(((b - a) - (x1 - x0) / 32.0).abs() < 1e-9);
    let out = plot::zoom(&sc, &l, &view, 1e-6, None, None, None);
    let (a, b) = out.x.expect("x");
    assert!(((b - a) - (x1 - x0) * 2.0).abs() < 1e-9);
    // On one axis only.
    let only_y = plot::zoom(&sc, &l, &view, 2.0, None, None, Some(Axis::Y));
    assert_eq!(only_y.x, None);
    assert!(only_y.y.is_some());
}

#[test]
fn pan_follows_the_hand_and_never_passes_the_extent() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    assert_eq!(
        plot::pan(&model, &l, &view, 50.0, 0.0, None),
        view,
        "the fit can't pan"
    );
    let zoomed = plot::zoom(&model, &l, &view, 4.0, None, None, None);
    let (a, b) = zoomed.x.expect("zoomed");
    let k = (b - a) / f64::from(l.plot.w);
    let right = plot::pan(&model, &l, &zoomed, 40.0, 0.0, None);
    let (c, d) = right.x.expect("panned");
    assert!(
        (c - (a - 40.0 * k)).abs() < 1e-6 && (d - (b - 40.0 * k)).abs() < 1e-6,
        "a drag right shows earlier"
    );
    let scroll = plot::pan(&model, &l, &zoomed, 0.0, 40.0, None);
    let (c, _) = scroll.x.expect("panned");
    assert!(
        (c - (a + 40.0 * k)).abs() < 1e-6,
        "dy > 0 moves later in time on an x-chart"
    );
    let ((x0, x1), _) = model.extent();
    let far = plot::pan(&model, &l, &zoomed, 1e6, 0.0, None);
    assert_eq!(far.x.expect("panned").0, x0, "clamped at the start");
    let far = plot::pan(&model, &l, &zoomed, -1e6, 0.0, None);
    let end = far.x.expect("panned");
    assert!((end.1 - x1).abs() < 1e-6, "clamped at the end");
    // Back at the end, follow comes on; a pan away turns it off.
    assert!(far.follow);
    let away = plot::pan(&model, &l, &far, 100.0, 0.0, None);
    assert!(!away.follow);
    // A 2-D figure pans both; bars never pan their values.
    let sc = load("scatter.csv");
    let (_, sl, _) = lay(&sc, &View::default(), (880.0, 470.0));
    let z = plot::zoom(&sc, &sl, &View::default(), 2.0, None, None, None);
    let p = plot::pan(&sc, &sl, &z, 10.0, 10.0, None);
    assert!(p.x != z.x && p.y != z.y);
    let lat = load("latency.json");
    let (_, bl, _) = lay(&lat, &View::default(), (880.0, 470.0));
    assert_eq!(
        plot::pan(&lat, &bl, &View::default(), 0.0, 30.0, Some(Axis::Y)).y,
        None
    );
}

#[test]
fn a_brush_zooms_to_its_span_and_a_small_one_does_nothing() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let (x0, x1) = (l.plot.x + 100.0, l.plot.x + 300.0);
    let v = plot::brush(
        &model,
        &l,
        &view,
        BrushKind::X,
        (x0, l.plot.y),
        (x1, l.plot.bottom()),
    )
    .expect("a zoom");
    let (a, b) = v.x.expect("x");
    assert!((a - l.x.invert(x0)).abs() < 1e-3 && (b - l.x.invert(x1)).abs() < 1e-3);
    assert_eq!(v.y, None);
    assert_eq!(
        plot::brush(
            &model,
            &l,
            &view,
            BrushKind::X,
            (x0, 0.0),
            (x0 + BRUSH_MIN - 1.0, 0.0)
        ),
        None
    );
    // At no less than the narrowest span, centred.
    let narrow = plot::brush(
        &model,
        &l,
        &view,
        BrushKind::X,
        (x0, 0.0),
        (x0 + BRUSH_MIN, 0.0),
    )
    .expect("a zoom");
    let (a, b) = narrow.x.expect("x");
    assert!((b - a) >= 300.0 - 1e-6);
    // A box on a scatter: both axes; thin in y, x only.
    let sc = load("scatter.csv");
    let (_, sl, _) = lay(&sc, &View::default(), (880.0, 470.0));
    let p = sl.plot;
    let both = plot::brush(
        &sc,
        &sl,
        &View::default(),
        BrushKind::Box,
        (p.x + 50.0, p.y + 50.0),
        (p.x + 200.0, p.y + 150.0),
    )
    .expect("a zoom");
    assert!(both.x.is_some() && both.y.is_some());
    let thin = plot::brush(
        &sc,
        &sl,
        &View::default(),
        BrushKind::Box,
        (p.x + 50.0, p.y + 50.0),
        (p.x + 200.0, p.y + 53.0),
    )
    .expect("x only");
    assert!(thin.x.is_some() && thin.y.is_none());
    assert_eq!(
        plot::brush(
            &sc,
            &sl,
            &View::default(),
            BrushKind::Box,
            (p.x + 50.0, p.y + 50.0),
            (p.x + 52.0, p.y + 53.0)
        ),
        None
    );
    // Bands snap to whole bands, no fewer than show at full width (24 px bars).
    let names: Vec<String> = (0..80).map(|i| format!("c{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let t = Table::new(vec![
        Column::category("name", &refs),
        Column::number("v", (0..80).map(|i| f64::from(i % 9) + 1.0).collect()),
    ])
    .expect("a table");
    let bars = guessed(t);
    let (_, bl, _) = lay(&bars, &View::default(), (880.0, 470.0));
    let v = plot::brush(
        &bars,
        &bl,
        &View::default(),
        BrushKind::Bands,
        (bl.x.map(5.3), 200.0),
        (bl.x.map(40.6), 200.0),
    )
    .expect("a zoom");
    assert_eq!(v.x, Some((5.0, 41.0)));
    let one = plot::brush(
        &bars,
        &bl,
        &View::default(),
        BrushKind::Bands,
        (bl.x.map(20.2), 0.0),
        (bl.x.map(21.5), 0.0),
    );
    let (a, b) = one.expect("a zoom").x.expect("x");
    assert!(
        b - a >= f64::from(bl.plot.w / 30.0).floor() && a <= 20.0 && b >= 22.0,
        "{a} {b}"
    );
    assert_eq!(a.fract(), 0.0, "whole bands");
    // Six bands already at full width: nothing to zoom.
    let lat = load("latency.json");
    let (_, ll, _) = lay(&lat, &View::default(), (880.0, 470.0));
    assert_eq!(
        plot::brush(
            &lat,
            &ll,
            &View::default(),
            BrushKind::Bands,
            (ll.x.map(1.3), 0.0),
            (ll.x.map(3.6), 0.0)
        ),
        None
    );
}

#[test]
fn reset_reveal_centre_and_tween() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let zoomed = plot::zoom(
        &model,
        &l,
        &view,
        4.0,
        Some((l.plot.x + 1.0, l.plot.y + 1.0)),
        None,
        None,
    );
    assert!(zoomed.x.is_some());
    assert_eq!(plot::reset(&model, &zoomed, None).x, None);
    assert_eq!(plot::reset(&model, &zoomed, Some(Axis::Y)).x, zoomed.x);
    // The cursor off to the right: reveal scrolls it 10 % inside.
    let c = Cursor::Point {
        series: key("api"),
        row: 60,
    };
    let r = plot::reveal(&model, &l, &zoomed, &c);
    let (a, b) = r.x.expect("scrolled");
    let cx = model.series[0].xs[60];
    assert!((cx - (b - 0.1 * (b - a))).abs() < 1e-6, "{a} {b} {cx}");
    assert_eq!(
        plot::reveal(&model, &l, &r, &c),
        r,
        "inside already: no change"
    );
    // zz centres it, within the extent.
    let z = plot::centre(&model, &l, &zoomed, &c);
    let (a, b) = z.x.expect("centred");
    assert!(((a + b) / 2.0 - cx).abs() < 1e-6);
    // Half way between two views; past the end, the end itself.
    let from = View {
        x: Some((0.0, 10.0)),
        ..View::default()
    };
    let mut to = View {
        x: Some((10.0, 30.0)),
        ..View::default()
    };
    to.hidden.insert(key("api"));
    let mid = plot::tween(&from, &to, 0.5);
    assert_eq!(mid.x, Some((5.0, 20.0)));
    assert_eq!(mid.hidden, to.hidden);
    assert_eq!(plot::tween(&from, &to, 1.0), to);
    let fitted = plot::nav::fixed(&model, &view);
    assert_eq!(fitted.x, Some(model.fit(&view).0));
}

// ---------------------------------------------------------------- overlay

#[test]
fn nothing_to_show_is_an_empty_overlay() {
    let model = load("requests.csv");
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let o = overlay(&model, &l, &View::default(), &state());
    assert!(o.items.is_empty());
    assert_eq!(o.bounds(), None);
}

#[test]
fn the_cursor_shows_a_crosshair_dots_an_inverted_chip_and_the_card() {
    let model = load("requests.fig.json");
    let view = View::default();
    let theme = Theme::mono_dark();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let c = Cursor::Point {
        series: key("api"),
        row: 53,
    };
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            cursor: Some(c),
            ..state()
        },
    );
    assert!(has_text(&o, "10:25"), "the chip names the time");
    assert!(has_text(&o, "10:25 · Sat 21 Nov"), "the card's header");
    let chip = o.items.iter().any(|i| matches!(i, Item::Path { fill: Some(Paint::Solid { color, .. }), .. } if *color == theme.chip));
    assert!(chip, "the chip is inverted");
    // A dot a series, each ringed in the surface colour.
    let rings = o.items.iter().filter(|i| matches!(i, Item::Path { fill: Some(Paint::Solid { color, alpha }), .. } if *color == theme.surface && *alpha == 1.0)).count();
    assert!(rings >= 3);
    // The card lists every series, values first, the cursor's row marked; inside the area.
    let card = shadow(&o).expect("a card");
    let b = o.bounds().expect("bounds");
    assert!(
        b.x >= 0.0 && b.y >= 0.0 && b.right() <= 880.0 && b.bottom() <= 470.0,
        "{b:?}"
    );
    let x = l.x.map(model.series[0].xs[53]);
    assert!(
        card.x > x || card.right() < x,
        "beside the crosshair, never over it"
    );
    for name in ["api", "worker", "batch"] {
        assert!(has_text(&o, name));
    }
}

#[test]
fn a_hover_brings_the_nearest_series_forward_under_the_wash() {
    let model = load("requests.fig.json");
    let view = View::default();
    let theme = Theme::mono_dark();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let s = &model.series[0];
    let (px, py) = l.at(s.xs[20], s.ys[20]);
    let hover = plot::hover(&model, &l, &hits, &view, px, py);
    let emphasis = plot::nearest_series(&model, &l, &hits, &view, px, py);
    assert_eq!(emphasis, Some(key("api")));
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            hover,
            pointer: Some((px, py)),
            emphasis,
            ..state()
        },
    );
    let wash = o.items.iter().any(|i| matches!(i, Item::Path { fill: Some(Paint::Solid { color, alpha }), path, .. }
        if *color == theme.surface && *alpha == WASH && path == &die_fig::scene::PathData::rect(l.clip)));
    assert!(wash, "the surface over the plot");
    assert!(!has_text(&o, "10:25"), "no chip for a hover");
    let card = shadow(&o).expect("a card");
    assert!(card.y >= py, "right and below the pointer");
    assert!(
        card.bottom() <= l.plot.bottom() + 0.5,
        "over the plot while it fits"
    );
}

#[test]
fn the_card_flips_at_the_right_edge_and_stays_flipped_until_it_fits_with_a_cell_to_spare() {
    let model = load("requests.csv");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let at = |x: f32| {
        let h = plot::hover(&model, &l, &hits, &view, x, l.plot.y + 30.0);
        shadow(&overlay(
            &model,
            &l,
            &view,
            &OverlayState {
                hover: h.clone(),
                pointer: Some((x, l.plot.y + 30.0)),
                ..state()
            },
        ))
        .map(|r| (r, h))
    };
    let (left, _) = at(l.plot.x + 40.0).expect("a card");
    assert!(left.x > l.plot.x + 40.0, "right of the crosshair");
    let near = l.plot.right() - 30.0;
    let (r, _) = at(near).expect("a card");
    assert!(r.right() < near, "flipped left");
    // Moving back left a little keeps it flipped.
    let w = r.w;
    let back = l.plot.right() + 8.0 - 12.0 - w - 3.0;
    let (r2, _) = at(back).expect("a card");
    assert!(r2.right() < back + 6.0, "still left inside the hysteresis");
    let (r3, _) = at(back - 2.0 * l.cell_width - 12.0).expect("a card");
    assert!(
        r3.x > back - 2.0 * l.cell_width - 12.0,
        "back right with room to spare"
    );
}

#[test]
fn a_card_lists_twelve_rows_then_more() {
    let n = 15;
    let mut cols = vec![Column::time(
        "t",
        (0..5).map(|i| f64::from(i) * 60.0).collect(),
    )];
    for k in 0..n {
        cols.push(Column::number(
            &format!("s{k:02}"),
            (0..5).map(|i| f64::from(i + k)).collect(),
        ));
    }
    let model = guessed(Table::new(cols).expect("a table"));
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let c = Cursor::Point {
        series: key("s00"),
        row: 2,
    };
    let o = overlay(
        &model,
        &l,
        &View::default(),
        &OverlayState {
            cursor: Some(c),
            ..state()
        },
    );
    assert!(has_text(&o, "+3 more"));
    assert!(
        has_text(&o, "s00"),
        "the cursor's series is listed even when it's the smallest"
    );
    let names = texts(&o)
        .into_iter()
        .filter(|(t, _)| t.starts_with('s') && t.len() == 3)
        .count();
    assert_eq!(names, 12);
}

#[test]
fn a_reference_adds_deltas_and_the_measure_band() {
    let model = load("requests.fig.json");
    let view = View {
        reference: Some(Cursor::Point {
            series: key("api"),
            row: 30,
        }),
        ..View::default()
    };
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let c = Cursor::Point {
        series: key("api"),
        row: 53,
    };
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            cursor: Some(c),
            ..state()
        },
    );
    assert!(has_text(&o, "A"), "the reference's chip");
    // Δx over the measure band where the card leaves it clear, and in the card's header.
    let card = shadow(&o).expect("a card");
    let label = texts(&o).into_iter().find(|(t, _)| t == "+1h 55m");
    assert!(
        label.is_none_or(|(_, b)| card.intersect(&b).is_none()),
        "never under the card"
    );
    let low = (l.x.map(model.series[0].xs[53]), l.plot.bottom() - 10.0);
    let hover = Some(Cursor::Point {
        series: key("api"),
        row: 53,
    });
    let o2 = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            hover,
            pointer: Some(low),
            ..state()
        },
    );
    assert!(has_text(&o2, "+1h 55m"), "over the band with the card low");
    assert!(
        texts(&o)
            .iter()
            .any(|(t, _)| t.starts_with("10:25 · Sat 21 Nov · +1h 55m"))
    );
    let api = &model.series[0];
    let d = api.ys[53] - api.ys[30];
    let arrow = if d >= 0.0 { "▲" } else { "▼" };
    assert!(
        texts(&o)
            .iter()
            .any(|(t, _)| t.starts_with(&format!("{arrow} {}", d.abs())) && t.ends_with("%)"))
    );
}

#[test]
fn bars_lift_their_band_and_a_scatter_s_card_never_covers_the_point() {
    let model = load("latency.json");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let c = Cursor::Band {
        band: 3,
        series: Some(key("failure")),
    };
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            hover: Some(c),
            pointer: Some((500.0, 200.0)),
            ..state()
        },
    );
    assert!(has_text(&o, "50–100"));
    assert!(
        has_text(&o, "608") && has_text(&o, "total"),
        "a stack's total under a hairline"
    );
    let sc = load("scatter.csv");
    let (_, sl, hits) = lay(&sc, &View::default(), (880.0, 470.0));
    for i in [0usize, 7, 33] {
        let s = &sc.series[0];
        let p = sl.at(s.xs[i], s.ys[i]);
        let h = plot::hover(&sc, &sl, &hits, &View::default(), p.0, p.1);
        let o = overlay(
            &sc,
            &sl,
            &View::default(),
            &OverlayState {
                hover: h,
                pointer: Some(p),
                ..state()
            },
        );
        let card = shadow(&o).expect("a card");
        assert!(
            !card.inflate(4.0).contains(p.0, p.1),
            "the card is clear of the point"
        );
    }
}

#[test]
fn a_brush_shows_its_band_and_readout_and_hides_the_hover() {
    let model = load("requests.fig.json");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let (a, b) = (
        l.x.map(model.series[0].xs[23]),
        l.x.map(model.series[0].xs[30]),
    );
    let hover = plot::hover(&model, &l, &hits, &view, a, l.plot.y + 50.0);
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            hover,
            brush: Some((BrushKind::X, (a, l.plot.y), (b, l.plot.bottom()))),
            ..state()
        },
    );
    assert!(has_text(&o, "07:55 – 08:30 · 35m"), "{:?}", texts(&o));
    assert_eq!(shadow(&o), None, "no card while brushing");
    let bounds = o.bounds().expect("bounds");
    assert!(
        bounds.x <= a && bounds.right() >= b && bounds.w < l.plot.w * 0.4,
        "kept close: {bounds:?}"
    );
}

#[test]
fn the_banner_and_the_empty_figure() {
    let theme = Theme::mono_dark();
    let mut scene = Scene::new(600.0, 300.0, 2.0);
    plot::banner(
        &mut scene,
        "line 3, column 7: expected ',' or '}'",
        &theme,
        &mut FixedMeasure,
    );
    assert!(scene.regions.iter().any(|r| r.hit == Hit::Banner));
    assert!(has_text(&scene, "line 3, column 7: expected ',' or '}'"));
    let e = plot::empty(
        (600.0, 300.0),
        2.0,
        "No data in requests.csv",
        &theme,
        &mut FixedMeasure,
    );
    assert!(has_text(&e, "No data in requests.csv"));
    assert_eq!(e.hit(300.0, 150.0), Some(&Hit::Chrome));
}
