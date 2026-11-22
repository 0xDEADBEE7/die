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
    "hbars.json",
    "profit.csv",
    "deploys.json",
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
                // Counts tick at whole steps however much room there is.
                let step = (l.y_ticks[1].value - l.y_ticks[0].value).abs();
                let most = if model.whole_values(&View::default()) && step == 1.0 {
                    f32::INFINITY
                } else {
                    110.0
                };
                assert!(
                    gap >= 24.0 && gap <= most,
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
    let (scene, l, _) = lay(&model, &all, (880.0, 470.0));
    // The empty kind, inside the plot: its status, what's so, how to bring them back.
    assert!(
        has_text(&scene, "Every series is hidden"),
        "{:?}",
        texts(&scene)
    );
    assert!(has_text(&scene, "Empty") && has_text(&scene, "shows them all"));
    for (t, r) in texts(&scene) {
        if ["Every series is hidden", "Empty", "shows them all"].contains(&t.as_str()) {
            assert!(
                r.x >= l.plot.x && r.right() <= l.plot.right() && r.y >= l.plot.y,
                "{t:?} at {r:?} in the plot {:?}",
                l.plot
            );
        }
    }
    // The legend stays, to click.
    assert!(!l.legend.is_empty());
    tidy(&scene, "every series hidden");
    // Small: the words alone, or less, never out of the plot.
    for size in [(400.0, 240.0), (200.0, 120.0)] {
        let (scene, l, _) = lay(&model, &all, size);
        tidy(&scene, "every series hidden, small");
        for (_, r) in texts(&scene) {
            if l.plot.contains(r.x + r.w / 2.0, r.y + r.h / 2.0) {
                assert!(r.x >= l.plot.x - 0.5 && r.right() <= l.plot.right() + 0.5);
            }
        }
    }
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
    assert!(has_text(&o, "no data"));
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
    let ((x0, x1), _) = sc.fit(&view);
    // A zoom halves what's shown: the data's extent padded by the dots' reach.
    let ((sx0, sx1), (sy0, sy1)) = l.domain;
    let v = plot::zoom(&sc, &l, &view, 2.0, None, None, None);
    let ((a, b), (c, d)) = (v.x.expect("x"), v.y.expect("y"));
    assert!(
        ((b - a) - (sx1 - sx0) / 2.0).abs() < 1e-9 && ((d - c) - (sy1 - sy0) / 2.0).abs() < 1e-9
    );
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

// ---------------------------------------------------------------- after the review

/// What the pointer at `p` shows: the hover, the series it brings forward, the overlay.
fn hovered(
    model: &ChartModel,
    l: &ChartLayout,
    hits: &die_fig::algo::HitIndex,
    view: &View,
    p: (f32, f32),
) -> (Option<Cursor>, Scene) {
    let hover = plot::hover(model, l, hits, view, p.0, p.1);
    let emphasis = plot::nearest_series(model, l, hits, view, p.0, p.1);
    let o = overlay(
        model,
        l,
        view,
        &OverlayState {
            hover: hover.clone(),
            pointer: Some(p),
            emphasis,
            ..state()
        },
    );
    (hover, o)
}

/// Each circle a scene draws, as its centre and radius, with its item's index.
fn circles(scene: &Scene) -> Vec<(usize, (f32, f32), f32)> {
    scene
        .items
        .iter()
        .enumerate()
        .filter_map(|(k, i)| match i {
            Item::Path { path, fill, .. }
                if fill.is_some()
                    && path.verbs.last() == Some(&Verb::Close)
                    && path
                        .verbs
                        .iter()
                        .filter(|v| matches!(v, Verb::Cubic(..)))
                        .count()
                        == 4
                    && path.verbs.len() == 6 =>
            {
                let b = path.bounds()?;
                Some((k, (b.x + b.w / 2.0, b.y + b.h / 2.0), b.w / 2.0))
            }
            _ => None,
        })
        .collect()
}

fn bands_spec(n: usize, horizontal: bool) -> String {
    let rows: Vec<String> = (0..n)
        .map(|i| format!("{{\"k\": \"c{i:02}\", \"v\": {}}}", 10 + (i * 7) % 23))
        .collect();
    let (x, y) = if horizontal { ("v", "k") } else { ("k", "v") };
    format!(
        "{{\"type\": \"bar\", \"data\": {{\"values\": [{}]}}, \"x\": \"{x}\", \"y\": \"{y}\", \
         \"mark\": {{\"type\": \"bar\", \"horizontal\": {horizontal}}}}}",
        rows.join(", ")
    )
}

#[test]
fn horizontal_bars_lay_their_bands_down_the_screen_and_their_values_across() {
    let model = load("hbars.json");
    assert!(model.horizontal && model.is_band());
    let view = View::default();
    for &size in SIZES {
        let (scene, l, hits) = lay(&model, &view, size);
        assert!(
            matches!(l.y, die_fig::scale::Scale::Band { .. })
                && matches!(l.x, die_fig::scale::Scale::Linear { .. }),
            "{size:?}: x across is the values', y up the bands'"
        );
        assert_eq!(l.bars.len(), 6, "{size:?}: a bar a service");
        let p = l.plot;
        for b in &l.bars {
            let r = b.rect;
            assert!(
                r.x >= p.x - 0.5
                    && r.right() <= p.right() + 0.5
                    && r.y >= p.y - 0.5
                    && r.bottom() <= p.bottom() + 0.5,
                "{size:?}: {r:?} inside {p:?}"
            );
            assert!((r.x - p.x).abs() < 0.5, "{size:?}: from the zero baseline");
        }
        // The longest bar is inventory's (144), across most of the plot.
        let widest = l.bars.iter().map(|b| b.rect.w).fold(0.0, f32::max);
        assert!(widest > p.w * 0.6, "{size:?}: {widest} of {}", p.w);
        // The value ticks run across under the plot, the bands' names down its left.
        assert!(
            l.labels
                .iter()
                .any(|x| x.role == Role::YTick && x.text == "auth")
        );
        assert!(
            l.labels
                .iter()
                .any(|x| x.role == Role::XTick && x.text == "100")
        );
        // The grid runs up the plot at the value ticks, never across the bands' names.
        for item in &scene.items {
            if let Item::Path {
                path,
                stroke: Some(st),
                ..
            } = item
                && st.hairline
                && let Some(b) = path.bounds()
            {
                assert!(
                    b.x >= p.x - 0.5,
                    "{size:?}: a hairline at {b:?} left of the plot"
                );
            }
        }
        // Hover finds each bar, and its card keeps clear of it.
        for b in &l.bars {
            let at = (b.rect.x + b.rect.w * 0.6, b.rect.y + b.rect.h / 2.0);
            let (h, o) = hovered(&model, &l, &hits, &view, at);
            assert_eq!(
                h,
                Some(Cursor::Band {
                    band: b.band,
                    series: Some(key("errors"))
                }),
                "{size:?}"
            );
            let card = shadow(&o).expect("a card");
            assert!(
                card.intersect(&b.rect.inflate(-0.5)).is_none(),
                "{size:?}: the card {card:?} over the bar {:?}",
                b.rect
            );
            let a = Rect::new(0.0, 0.0, size.0, size.1);
            assert!(a.inflate(0.5).contains(card.x, card.y), "{size:?}");
            assert!(card.right() <= size.0 + 0.5 && card.bottom() <= size.1 + 0.5);
        }
    }
    // A number field on the bands' axis is read as categories, in data order.
    let years = from_spec(
        r#"{"type": "bar", "data": {"values": [{"year": 2021, "n": 4}, {"year": 2022, "n": 9},
            {"year": 2023, "n": 7}, {"year": 2024, "n": 12}]}, "y": "year", "x": "n",
            "mark": {"type": "bar", "horizontal": true}}"#,
    );
    assert!(years.is_band() && years.horizontal);
    assert_eq!(years.x.bands.len(), 4);
    let (_, l, _) = lay(&years, &View::default(), (880.0, 470.0));
    assert_eq!(l.bars.len(), 4);
    assert!(
        l.bars
            .iter()
            .all(|b| l.plot.inflate(0.5).contains(b.rect.x, b.rect.y))
    );
    assert!(
        l.labels
            .iter()
            .any(|x| x.role == Role::YTick && x.text == "2024")
    );
}

#[test]
fn horizontal_bars_brush_along_y() {
    let model = from_spec(&bands_spec(40, true));
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (400.0, 240.0));
    assert_eq!(plot::brush_kind(&model), BrushKind::Bands);
    let centre = |i: u32| {
        let b = l.bars.iter().find(|b| b.band == i).expect("a bar");
        (l.plot.x + 20.0, b.rect.y + b.rect.h / 2.0)
    };
    let (from, to) = (centre(10), centre(20));
    let v = plot::brush(&model, &l, &view, BrushKind::Bands, from, to).expect("a zoom");
    assert_eq!(v.x, Some((10.0, 21.0)), "the bands it touched, whole");
    assert_eq!(v.y, None, "the values never zoom");
    // The preview spans those bands down the plot, across it.
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            brush: Some((BrushKind::Bands, from, to)),
            ..state()
        },
    );
    assert!(has_text(&o, "c10 – c20 · 11 bands"), "{:?}", texts(&o));
}

#[test]
fn the_overlay_never_panics_in_a_tiny_or_odd_area() {
    let actions = [
        Action::Step(1),
        Action::Step(-1),
        Action::Series(1),
        Action::Series(-1),
        Action::Peak(1),
        Action::First,
        Action::Last,
        Action::ViewEdge(0),
        Action::ViewEdge(1),
    ];
    for name in FIXTURES {
        let model = load(name);
        let view = model.initial_view();
        for size in [
            (300.0, 21.0),
            (300.0, 26.0),
            (60.0, 40.0),
            (1.0, 1.0),
            (120.0, 60.0),
        ] {
            let (_, l, hits) = lay(&model, &view, size);
            let p = l.plot;
            let kind = plot::brush_kind(&model);
            let mut cursor = None;
            for a in &actions {
                if let Some(c) = plot::motion(&model, &l, &view, cursor.as_ref(), a, 1) {
                    cursor = Some(c);
                }
            }
            for fx in 0..=6 {
                for fy in 0..=3 {
                    let at = (p.x + p.w * fx as f32 / 6.0, p.y + p.h * fy as f32 / 3.0);
                    let hover = plot::hover(&model, &l, &hits, &view, at.0, at.1);
                    let emphasis = plot::nearest_series(&model, &l, &hits, &view, at.0, at.1);
                    let mut v = view.clone();
                    v.reference = cursor.clone();
                    for brush in [None, Some((kind, (p.x, p.y), at))] {
                        overlay(
                            &model,
                            &l,
                            &v,
                            &OverlayState {
                                hover: hover.clone(),
                                cursor: cursor.clone(),
                                pointer: Some(at),
                                brush,
                                emphasis: emphasis.clone(),
                            },
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_series_with_no_samples_has_no_cursor_and_a_stale_one_moves_nowhere_wrong() {
    let nan = f64::NAN;
    let t = Table::new(vec![
        Column::time("t", vec![0.0, 60.0, 120.0]),
        Column::number("a", vec![1.0, 2.0, 3.0]),
        Column::number("b", vec![nan, nan, nan]),
    ])
    .expect("a table");
    let model = guessed(t);
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    assert!(
        model.labels().iter().all(|(n, _)| n != "b"),
        "nothing to put a cursor on: {:?}",
        model.labels()
    );
    let on_b = Cursor::Point {
        series: key("b"),
        row: 0,
    };
    for d in [1, -1] {
        assert_eq!(
            plot::motion(&model, &l, &view, Some(&on_b), &Action::Step(d), 1),
            None
        );
    }
    // A cursor whose row a re-parse took away.
    let stale = Cursor::Point {
        series: key("a"),
        row: 7,
    };
    for a in [
        Action::Step(1),
        Action::Step(-1),
        Action::Series(1),
        Action::Series(-1),
        Action::Peak(1),
        Action::First,
        Action::Last,
        Action::ViewEdge(-1),
    ] {
        let to = plot::motion(&model, &l, &view, Some(&stale), &a, 1);
        if let Some(Cursor::Point { row, .. }) = to {
            assert!(row < 3, "{a:?} to row {row}");
        }
    }
}

#[test]
fn a_brush_over_times_reads_in_the_zone() {
    let day = |zone: &TimeZone, d: i8, h: i8| {
        jiff::civil::date(2026, 11, d)
            .at(h, 0, 0, 0)
            .to_zoned(zone.clone())
            .expect("a time")
            .timestamp()
            .as_second() as f64
    };
    let brushed = |zone: &TimeZone, ts: Vec<f64>, a: f64, b: f64| {
        let t = Table::new(vec![
            Column::time("t", ts.clone()),
            Column::number("v", (0..ts.len()).map(|i| (i % 4) as f64).collect()),
        ])
        .expect("a table");
        let chart = spec::guess(&t).expect("a guess");
        let model = ChartModel::build(chart, vec![Arc::new(t)], zone).expect("a model");
        let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
        let (pa, pb) = (l.x.map(a), l.x.map(b));
        let o = overlay(
            &model,
            &l,
            &View::default(),
            &OverlayState {
                brush: Some((BrushKind::X, (pa, l.plot.y), (pb, l.plot.bottom()))),
                ..state()
            },
        );
        texts(&o).into_iter().map(|(t, _)| t).collect::<Vec<_>>()
    };
    let ny = TimeZone::get("America/New_York").expect("the system's zoneinfo");
    let days: Vec<f64> = (16..24).map(|d| day(&ny, d, 0)).collect();
    let t = brushed(&ny, days, day(&ny, 19, 0), day(&ny, 21, 0));
    assert!(t.iter().any(|s| s == "19 Nov – 21 Nov · 2d"), "{t:?}");
    let kolkata = TimeZone::fixed(jiff::tz::Offset::from_seconds(5 * 3600 + 1800).expect("+5:30"));
    let hours: Vec<f64> = (0..12).map(|h| day(&kolkata, 21, h)).collect();
    let t = brushed(&kolkata, hours, day(&kolkata, 21, 7), day(&kolkata, 21, 9));
    assert!(t.iter().any(|s| s == "07:00 – 09:00 · 2h"), "{t:?}");
}

/// Two line layers, both of a field "latency", and a band layer under a line by colour.
const TWO_LATENCIES: &str = r#"{"type": "chart",
  "data": {"values": [{"t": 0, "latency": 10}, {"t": 1, "latency": 11}, {"t": 2, "latency": 12}]},
  "x": "t", "y": "latency", "mark": "line",
  "layers": [{"mark": "line", "data": {"values": [{"t": 0, "latency": 40}, {"t": 1, "latency": 42},
              {"t": 2, "latency": 41}]}}]}"#;

#[test]
fn a_name_in_two_layers_is_two_series_but_a_band_shares_its_line_s() {
    let model = from_spec(TWO_LATENCIES);
    assert_eq!(model.series.len(), 2);
    assert_eq!(
        model.keys(),
        [SeriesKey::new("latency", 0), SeriesKey::new("latency", 1)]
    );
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    assert_eq!(l.items.len(), 2, "both in the legend");
    let labels: Vec<String> = model.labels().into_iter().map(|(n, _)| n).collect();
    assert_eq!(labels, ["latency", "latency ‹2›"]);
    let upper = l.at(1.0, 42.0);
    let (h, o) = hovered(&model, &l, &hits, &view, upper);
    assert_eq!(
        h,
        Some(Cursor::Point {
            series: SeriesKey::new("latency", 1),
            row: 1
        })
    );
    assert!(has_text(&o, "42") && has_text(&o, "11"), "{:?}", texts(&o));
    // A band coloured as its line, in a layer after it, is the line's series.
    let banded = from_spec(
        r#"{"type": "chart",
  "data": {"values": [{"t": 0, "s": "api", "v": 10, "lo": 8, "hi": 12},
                      {"t": 1, "s": "api", "v": 11, "lo": 9, "hi": 14},
                      {"t": 0, "s": "web", "v": 20, "lo": 18, "hi": 23},
                      {"t": 1, "s": "web", "v": 21, "lo": 19, "hi": 22}]},
  "x": "t", "color": "s", "mark": "line", "y": "v",
  "layers": [{"mark": "band", "y": "lo", "y2": "hi"}]}"#,
    );
    assert_eq!(banded.series.len(), 4);
    assert_eq!(banded.keys(), [key("api"), key("web")]);
}

#[test]
fn a_band_peak_looks_past_the_view() {
    let n = 40;
    let rows: Vec<String> = (0..n)
        .map(|i| {
            let v = match i {
                1 => 90,
                35 => 80,
                _ => 10,
            };
            format!("{{\"k\": \"c{i:02}\", \"v\": {v}}}")
        })
        .collect();
    let model = from_spec(&format!(
        "{{\"type\": \"bar\", \"data\": {{\"values\": [{}]}}, \"x\": \"k\", \"y\": \"v\"}}",
        rows.join(", ")
    ));
    let view = View {
        x: Some((0.0, 28.0)),
        ..View::default()
    };
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let at = |b: u32| Cursor::Band {
        band: b,
        series: None,
    };
    for from in [1, 20] {
        assert_eq!(
            plot::motion(&model, &l, &view, Some(&at(from)), &Action::Peak(1), 1),
            Some(at(35)),
            "w from band {from}"
        );
    }
    assert_eq!(
        plot::motion(&model, &l, &view, Some(&at(35)), &Action::Peak(-1), 1),
        Some(at(1))
    );
}

#[test]
fn a_normalised_stack_s_card_gives_shares_and_its_deltas_in_points() {
    let area = from_spec(
        r#"{"type": "chart", "data": {"values": [{"t": 0, "s": "a", "v": 100}, {"t": 0, "s": "b", "v": 100},
            {"t": 1, "s": "a", "v": 300}, {"t": 1, "s": "b", "v": 100}]},
            "x": "t", "y": "v", "color": "s", "mark": {"type": "area", "stack": "normalize"}}"#,
    );
    let view = View {
        reference: Some(Cursor::Point {
            series: key("a"),
            row: 0,
        }),
        ..View::default()
    };
    let (_, l, _) = lay(&area, &view, (880.0, 470.0));
    let o = overlay(
        &area,
        &l,
        &view,
        &OverlayState {
            cursor: Some(Cursor::Point {
                series: key("a"),
                row: 1,
            }),
            ..state()
        },
    );
    for t in ["75 %", "25 %", "▲ 25 pp", "▼ 25 pp"] {
        assert!(has_text(&o, t), "{t}: {:?}", texts(&o));
    }
    let bars = from_spec(
        r#"{"type": "bar", "data": {"values": [
            {"week": "W1", "status": "ok", "n": 120}, {"week": "W1", "status": "slow", "n": 108},
            {"week": "W1", "status": "error", "n": 82}, {"week": "W2", "status": "ok", "n": 191},
            {"week": "W2", "status": "slow", "n": 41}, {"week": "W2", "status": "error", "n": 111}]},
            "x": "week", "y": "n", "color": "status", "mark": {"type": "bar", "stack": "normalize"}}"#,
    );
    let view = View {
        reference: Some(Cursor::Band {
            band: 0,
            series: None,
        }),
        ..View::default()
    };
    let (_, l, _) = lay(&bars, &view, (880.0, 470.0));
    let o = overlay(
        &bars,
        &l,
        &view,
        &OverlayState {
            cursor: Some(Cursor::Band {
                band: 1,
                series: None,
            }),
            ..state()
        },
    );
    // W2: ok 191 of 343, slow 41, error 111; W1: ok 120 of 310.
    for t in ["55.7 %", "12 %", "32.4 %", "▲ 17 pp"] {
        assert!(has_text(&o, t), "{t}: {:?}", texts(&o));
    }
}

#[test]
fn bars_over_time_and_below_zero_are_drawn_whole_inside_the_plot() {
    for name in ["deploys.json", "profit.csv"] {
        let model = load(name);
        for &size in SIZES {
            let (_, l, _) = lay(&model, &View::default(), size);
            assert_eq!(
                l.bars.len(),
                if name == "profit.csv" { 8 } else { 6 },
                "{name}"
            );
            let p = l.plot.inflate(0.5);
            for b in &l.bars {
                let r = b.rect;
                assert!(
                    r.x >= p.x && r.right() <= p.right() && r.y >= p.y && r.bottom() <= p.bottom(),
                    "{name} {size:?}: {r:?} past {p:?}"
                );
            }
        }
    }
    let model = load("profit.csv");
    let (scene, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let (lo, _) = l.y.domain();
    assert!(
        lo <= -160.0,
        "the domain reaches the lowest: {:?}",
        l.y.domain()
    );
    let zero = l.y.map(0.0);
    assert!(
        zero < l.plot.bottom() - 20.0,
        "the baseline is zero, above the plot's foot"
    );
    // Feb, Apr and Jul hang down from it.
    let down: Vec<u32> = l
        .bars
        .iter()
        .filter(|b| (b.rect.y - zero).abs() < 0.5 && b.rect.bottom() > zero + 5.0)
        .map(|b| b.band)
        .collect();
    assert_eq!(down, [1, 3, 6]);
    assert!(
        texts(&scene).iter().any(|(t, _)| t == "−160"),
        "its label under it"
    );
    // Integer counts get whole ticks.
    let deploys = load("deploys.json");
    for &size in SIZES {
        let (_, l, _) = lay(&deploys, &View::default(), size);
        assert!(
            l.y_ticks.iter().all(|t| t.value.fract() == 0.0),
            "{size:?}: {:?}",
            l.y_ticks
        );
    }
}

#[test]
fn a_band_brush_reads_the_bands_it_zooms_to_or_says_it_cannot() {
    let model = from_spec(&bands_spec(40, false));
    let view = View::default();
    for scale in [1.0, 2.0] {
        let (_, l, _) = plot::base(
            &model,
            &view,
            (400.0, 240.0),
            scale,
            &Theme::mono_dark(),
            &mut FixedMeasure,
        );
        let p = l.plot;
        for k in 0..30 {
            let a = p.x + 3.0 + k as f32 * 7.3;
            let b = (a + 41.7 + k as f32 * 3.1).min(p.right());
            let (from, to) = ((a, p.y + 10.0), (b, p.bottom() - 10.0));
            let (i, j) = plot::nav::brush_bands(&model, &l, &view, from, to).expect("zoomable");
            let v = plot::brush(&model, &l, &view, BrushKind::Bands, from, to).expect("a zoom");
            let (x0, x1) = v.x.expect("x zoomed");
            assert!(
                x0 <= i as f64 && x1 >= j as f64,
                "{scale}: {i}..{j} in {x0}..{x1}"
            );
            let o = overlay(
                &model,
                &l,
                &view,
                &OverlayState {
                    brush: Some((BrushKind::Bands, from, to)),
                    ..state()
                },
            );
            let want = format!("c{i:02} – c{:02} · {} bands", j - 1, j - i);
            assert!(has_text(&o, &want), "{scale}: {want} in {:?}", texts(&o));
        }
    }
    // Bands already at full width can't zoom: the preview says so and promises nothing.
    let latency = load("latency.json");
    let (_, l, _) = lay(&latency, &view, (880.0, 470.0));
    let p = l.plot;
    let (from, to) = ((p.x + 100.0, p.y + 10.0), (p.x + 400.0, p.y + 50.0));
    assert_eq!(
        plot::brush(&latency, &l, &view, BrushKind::Bands, from, to),
        None
    );
    let o = overlay(
        &latency,
        &l,
        &view,
        &OverlayState {
            brush: Some((BrushKind::Bands, from, to)),
            ..state()
        },
    );
    assert!(
        texts(&o).iter().all(|(t, _)| !t.contains("bands")),
        "{:?}",
        texts(&o)
    );
    assert!(has_text(&o, "Every band shows"), "{:?}", texts(&o));
}

#[test]
fn a_card_never_covers_its_crosshair_or_its_dots_and_never_slices_a_tick() {
    let theme = Theme::mono_dark();
    for name in [
        "requests.fig.json",
        "sine.json",
        "gaps.csv",
        "thresholds.json",
        "requests.csv",
    ] {
        let model = load(name);
        let first = model.keys()[0].clone();
        let view = View {
            reference: Some(Cursor::Point {
                series: first,
                row: 5,
            }),
            ..model.initial_view()
        };
        for size in [(400.0, 240.0), (880.0, 470.0)] {
            let (_, l, hits) = lay(&model, &view, size);
            let p = l.plot;
            for k in 1..20 {
                for fy in [0.2, 0.5, 0.8] {
                    let at = (p.x + p.w * k as f32 / 20.0, p.y + p.h * fy);
                    let (h, o) = hovered(&model, &l, &hits, &view, at);
                    let Some(Cursor::Point { series, row }) = h else {
                        continue;
                    };
                    let si = model.index_of(&series).expect("its series");
                    let x = l.x.map(model.series[si].xs[row as usize]);
                    let card = shadow(&o).expect("a card");
                    // The dots on the crosshair: the circles centred on it.
                    let dots: Vec<(f32, f32)> = circles(&o)
                        .into_iter()
                        .filter(|(_, c, _)| (c.0 - x).abs() < 0.5)
                        .map(|(_, c, r)| (c.1, r))
                        .collect();
                    assert!(!dots.is_empty(), "{name} {size:?} at {at:?}");
                    let clear_x = card.right() <= x - 6.0 || card.x >= x + 6.0;
                    let clear_y = dots
                        .iter()
                        .all(|&(y, r)| card.bottom() <= y - r || card.y >= y + r);
                    assert!(
                        clear_x || clear_y,
                        "{name} {size:?} at {at:?}: the card {card:?} over the crosshair at {x}"
                    );
                    // A tick label the card meets is wholly under it or wholly covered.
                    for t in l.labels.iter().filter(|t| t.role == Role::YTick) {
                        let b = t.bounds(&mut FixedMeasure);
                        let Some(i) = card.intersect(&b) else {
                            continue;
                        };
                        let under = (i.w - b.w).abs() < 0.5 && (i.h - b.h).abs() < 0.5;
                        let covered = o.items.iter().any(|it| {
                            matches!(it,
                            Item::Path { fill: Some(Paint::Solid { color, alpha }), path, .. }
                            if *color == theme.surface && *alpha == 1.0
                                && path.bounds().is_some_and(|pb| pb.inflate(0.5).contains(b.x, b.y)
                                    && pb.inflate(0.5).contains(b.right(), b.bottom())))
                        });
                        assert!(
                            under || covered,
                            "{name} {size:?}: the tick {:?} sliced by the card {card:?}",
                            t.text
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_card_stays_inside_a_small_area_and_keeps_the_marked_row() {
    let model = load("many.json");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (400.0, 240.0));
    let area = Rect::new(0.0, 0.0, 400.0, 240.0);
    let p = l.plot;
    for k in 1..10 {
        for fy in [0.1, 0.5, 0.9] {
            let at = (p.x + p.w * k as f32 / 10.0, p.y + p.h * fy);
            let (_, o) = hovered(&model, &l, &hits, &view, at);
            let card = shadow(&o).expect("a card");
            assert!(
                card.x >= 0.0 && card.y >= 0.0 && card.right() <= 400.0 && card.bottom() <= 240.0,
                "at {at:?}: {card:?}"
            );
            for (t, b) in texts(&o) {
                assert!(
                    area.inflate(0.5).contains(b.x, b.y)
                        && b.right() <= 400.5
                        && b.bottom() <= 240.5,
                    "{t:?} at {b:?}"
                );
            }
            assert!(texts(&o).iter().any(|(t, _)| t.ends_with(" more")));
        }
    }
    // The keyboard's cursor on the first series: its row is in the card.
    let first = model.keys()[0].clone();
    let name = model.series_of(&first).expect("a series").name.clone();
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            cursor: Some(Cursor::Point {
                series: first,
                row: 3,
            }),
            ..state()
        },
    );
    assert!(has_text(&o, &name), "{name}: {:?}", texts(&o));
    let card = shadow(&o).expect("a card");
    assert!(card.bottom() <= 240.0);
}

#[test]
fn y_ticks_keep_a_calm_grid() {
    let model = load("requests.csv");
    for &size in SIZES {
        let (_, l, _) = lay(&model, &View::default(), size);
        let ys: Vec<f32> = l.y_ticks.iter().map(|t| l.y.map(t.value)).collect();
        let gap = (ys[0] - ys[1]).abs();
        assert!((40.0..=100.0).contains(&gap), "{size:?}: {gap}");
        if size.0 == 880.0 {
            assert!(
                gap >= 48.0,
                "0 to 1,200 by 200 over 0 to 1,100 by 100: {gap}"
            );
        }
    }
}

#[test]
fn a_step_line_s_markers_sit_on_its_samples() {
    let model = from_spec(
        r#"{"type": "chart", "data": {"values": [{"t": 0, "n": 0}, {"t": 1, "n": 5}, {"t": 2, "n": 2},
            {"t": 3, "n": 5}, {"t": 4, "n": 1}, {"t": 5, "n": 3}]},
            "x": "t", "y": "n", "mark": {"type": "line", "curve": "step", "points": true}}"#,
    );
    let (scene, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let s = &model.series[0];
    let markers: Vec<(f32, f32)> = circles(&scene)
        .into_iter()
        .filter(|(_, _, r)| (*r - 3.0).abs() < 0.01)
        .map(|(_, c, _)| c)
        .collect();
    assert_eq!(markers.len(), 6, "{markers:?}");
    for (i, m) in markers.iter().enumerate() {
        let want = l.at(s.xs[i], s.ys[i]);
        assert!((m.0 - want.0).abs() < 0.5 && (m.1 - want.1).abs() < 0.5);
    }
    assert!(
        l.y_ticks.iter().all(|t| t.value.fract() == 0.0),
        "{:?}",
        l.y_ticks
    );
}

#[test]
fn the_marked_dot_is_drawn_over_the_others() {
    let model = load("thresholds.json");
    let view = View::default();
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    for k in model.keys() {
        let o = overlay(
            &model,
            &l,
            &view,
            &OverlayState {
                cursor: Some(Cursor::Point {
                    series: k.clone(),
                    row: 12,
                }),
                ..state()
            },
        );
        let cs = circles(&o);
        let big = cs
            .iter()
            .filter(|(_, _, r)| (*r - 5.5).abs() < 0.01)
            .map(|(i, _, _)| *i)
            .max()
            .expect("the cursor's dot");
        for (i, _, r) in &cs {
            if (*r - 4.0).abs() < 0.01 {
                assert!(*i < big, "{k:?}: a dot over the cursor's");
            }
        }
    }
}

#[test]
fn a_band_reference_keeps_its_name_and_clear_of_the_totals() {
    let model = load("latency.json");
    let last = model.x.bands.len() as u32 - 1;
    let view = View {
        reference: Some(Cursor::Band {
            band: last,
            series: None,
        }),
        ..View::default()
    };
    let (scene, l, _) = lay(&model, &view, (880.0, 470.0));
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            cursor: Some(Cursor::Band {
                band: last - 1,
                series: None,
            }),
            ..state()
        },
    );
    let name = model.x.bands[last as usize].to_string();
    assert!(has_text(&o, &format!("A · {name}")), "{:?}", texts(&o));
    assert!(
        texts(&o)
            .iter()
            .any(|(t, _)| t.ends_with("· \u{2212}1 band")),
        "{:?}",
        texts(&o)
    );
    // The reference's rule stops short of the band's total.
    let total: Vec<Rect> = texts(&scene)
        .into_iter()
        .filter(|(t, b)| {
            t.chars().all(|c| c.is_ascii_digit() || c == ',')
                && b.y < l.plot.bottom()
                && b.y > l.plot.y - 2.0
        })
        .map(|(_, b)| b)
        .collect();
    assert!(!total.is_empty());
    for item in &o.items {
        if let Item::Path {
            path,
            stroke: Some(st),
            ..
        } = item
            && st.dash == Some((4.0, 3.0))
        {
            let b = path.bounds().expect("a rule");
            for t in &total {
                assert!(
                    b.intersect(t).is_none_or(|i| i.h < 0.5),
                    "{b:?} through {t:?}"
                );
            }
        }
    }
}

#[test]
fn every_series_hidden_draws_no_grid_and_no_ticks() {
    let model = from_spec(
        r#"{"type": "line", "series": [
            {"label": "a", "fig": {"hidden": true}, "points": [{"x": 0, "y": 0}, {"x": 1, "y": 4}]},
            {"label": "b", "fig": {"hidden": true}, "points": [{"x": 0, "y": 1}, {"x": 1, "y": 2}]}]}"#,
    );
    let theme = Theme::mono_dark();
    let view = model.initial_view();
    let (scene, l, _) = lay(&model, &view, (880.0, 470.0));
    assert!(has_text(&scene, "Every series is hidden"));
    assert!(
        l.labels
            .iter()
            .all(|t| !matches!(t.role, Role::XTick | Role::YTick)),
        "no ticks"
    );
    let grid = scene.items.iter().any(|i| {
        matches!(i, Item::Path { stroke: Some(st), .. }
            if matches!(st.paint, Paint::Solid { color, .. } if color == theme.grid))
    });
    assert!(!grid, "no grid");
    assert_eq!(l.items.len(), 2, "the legend stays, to bring them back");
}

#[test]
fn bubbles_stay_whole_inside_the_plot() {
    let rows: Vec<String> = (0..30)
        .map(|i| {
            let x = f64::from(i * 37 % 100);
            let y = f64::from(i * 53 % 50) + 0.4;
            let n = (i * 97) % 450 + 10;
            format!("{{\"x\": {x}, \"y\": {y}, \"n\": {n}}}")
        })
        .collect();
    let model = from_spec(&format!(
        "{{\"type\": \"scatter\", \"data\": {{\"values\": [{}]}}, \"x\": \"x\", \"y\": \"y\", \"size\": \"n\"}}",
        rows.join(", ")
    ));
    for &size in SIZES {
        let (scene, l, _) = lay(&model, &View::default(), size);
        let p = l.plot.inflate(1.0);
        let bubbles: Vec<_> = circles(&scene)
            .into_iter()
            .filter(|(_, _, r)| *r > 3.5)
            .collect();
        assert_eq!(bubbles.len(), 30, "{size:?}");
        for (_, c, r) in bubbles {
            assert!(
                c.0 - r >= p.x && c.0 + r <= p.right() && c.1 - r >= p.y && c.1 + r <= p.bottom(),
                "{size:?}: a bubble at {c:?} r {r} past {p:?}"
            );
        }
    }
}

#[test]
fn a_hovered_band_is_tinted_across_its_slot() {
    let model = load("services-bars.csv");
    let view = View::default();
    let theme = Theme::mono_dark();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let (a, w) = l.x.band_of(2).expect("billing");
    let step = w / 0.8;
    let (h, o) = hovered(
        &model,
        &l,
        &hits,
        &view,
        (a + w + step * 0.08, l.plot.y + 20.0),
    );
    assert!(matches!(h, Some(Cursor::Band { band: 2, .. })), "{h:?}");
    let tint = o
        .items
        .iter()
        .find_map(|i| match i {
            Item::Path {
                path,
                fill: Some(Paint::Solid { color, .. }),
                stroke: None,
            } if *color == theme.text => path.bounds(),
            _ => None,
        })
        .expect("the band's tint");
    assert!(tint.w >= 0.85 * step, "{} of {step}", tint.w);
}

#[test]
fn a_heatmap_card_keeps_its_value_s_decimals_and_end_labels_the_axis_s() {
    let model = from_spec(
        r#"{"type": "heatmap", "data": {"values": [{"day": "Mon", "hour": "00", "load": 90.6},
            {"day": "Mon", "hour": "02", "load": 18.2}, {"day": "Tue", "hour": "00", "load": 80.4},
            {"day": "Tue", "hour": "02", "load": 51}]}, "x": "hour", "y": "day", "color": "load"}"#,
    );
    let (_, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    let o = overlay(
        &model,
        &l,
        &View::default(),
        &OverlayState {
            cursor: Some(Cursor::Cell { row: 1, col: 0 }),
            ..state()
        },
    );
    assert!(has_text(&o, "80.4"), "{:?}", texts(&o));
    let sine = load("sine.json");
    let (_, l, _) = lay(&sine, &View::default(), (880.0, 470.0));
    assert!(!l.ends.is_empty());
    for e in &l.ends {
        let decimals = e.value.text.split_once('.').map_or(0, |(_, f)| f.len());
        assert!(decimals <= 2, "{:?}", e.value.text);
    }
}

#[test]
fn a_scatter_reference_gives_dx_at_x_s_precision() {
    let model = load("scatter.csv");
    let view = View {
        reference: Some(Cursor::Point {
            series: model.keys()[0].clone(),
            row: 0,
        }),
        ..View::default()
    };
    let (_, l, _) = lay(&model, &view, (880.0, 470.0));
    let k = model.keys()[0].clone();
    let s = model.series_of(&k).expect("a series");
    let o = overlay(
        &model,
        &l,
        &view,
        &OverlayState {
            cursor: Some(Cursor::Point {
                series: k.clone(),
                row: 3,
            }),
            ..state()
        },
    );
    let dx = s.xs[3] - s.xs[0];
    let want = format!("{}{:.1}", if dx < 0.0 { "\u{2212}" } else { "+" }, dx.abs());
    assert!(
        texts(&o)
            .iter()
            .any(|(t, _)| t.contains(&format!("Δ {want}, "))),
        "{want}: {:?}",
        texts(&o)
    );
}

// ---------------------------------------------------------------- the error figure (phase 2b)

use die_fig::error::{ErrorKind, FigError, Pos};
use die_fig::plot::errors::{LOG_HEADER, LOG_LINE, badge_text, log_entries, title};
use die_fig::plot::{ErrorView, error_figure, log_layout, log_page, stale_badge};

/// The figure's text, and an error of each kind in it (or in a data file beside it).
const SPEC: &str = "{\n  \"type\": \"line\",\n  \"data\": \"requests-old.csv\",\n  \"x\": \"time\",\n  \"y\": \"latency\"\n}\n";

fn kinds() -> Vec<FigError> {
    let at = |line: u32, col: u32| Pos { line, col, byte: 0 };
    vec![
        FigError::at("requests-old.csv: no such file", at(3, 11))
            .with_kind(ErrorKind::Unreadable)
            .note("looked for requests-old.csv"),
        FigError::malformed("expected ',' or '}'", at(4, 3)),
        FigError::at("no column \"latency\" in the data", at(5, 8))
            .note("requests.csv has time, api, worker, batch"),
        FigError::new("nothing to draw: the data has no rows").with_kind(ErrorKind::Empty),
    ]
}

fn symbolic(errors: &[FigError], size: (f32, f32)) -> Scene {
    error_figure(
        errors,
        SPEC,
        "requests.fig",
        ErrorView::Symbolic,
        size,
        2.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    )
}

/// Every text's box inside the area, and no two overlapping.
fn tidy(scene: &Scene, what: &str) {
    let area = Rect::new(0.0, 0.0, scene.width, scene.height);
    let ts = texts(scene);
    for (t, r) in &ts {
        assert!(
            r.x >= -0.5 && r.y >= -0.5 && r.right() <= area.w + 0.5 && r.bottom() <= area.h + 0.5,
            "{what}: {t:?} at {r:?} leaves the area"
        );
    }
    for (i, (a, ra)) in ts.iter().enumerate() {
        for (b, rb) in &ts[i + 1..] {
            assert!(!overlaps(ra, rb), "{what}: {a:?} and {b:?} overlap");
        }
    }
}

fn paths(scene: &Scene) -> usize {
    scene
        .items
        .iter()
        .filter(|i| matches!(i, Item::Path { .. }))
        .count()
}

#[test]
fn the_symbolic_figure_says_the_kind_title_and_first_error_and_degrades_as_it_shrinks() {
    for e in kinds() {
        let what = e.kind.name();
        let one = std::slice::from_ref(&e);
        // All of it at the big size: the status, the title, the error with its place, the keys.
        let big = symbolic(one, (880.0, 470.0));
        tidy(&big, what);
        let t = title(e.kind, "requests.fig");
        assert!(has_text(&big, &t), "{what}: {:?}", texts(&big));
        let said = die_fig::plot::errors::summary(&e);
        assert!(has_text(&big, &said), "{what}: {said} in {:?}", texts(&big));
        assert!(has_text(&big, "details") && has_text(&big, "source"));
        let label = ["Unreadable", "Malformed", "Mismatch", "Empty"][e.kind as usize];
        assert!(
            has_text(&big, label),
            "the status's label, not colour alone"
        );
        assert_eq!(big.hit(440.0, 235.0), Some(&Hit::ErrorFigure));
        assert_eq!(
            big.hit(5.0, 5.0),
            Some(&Hit::ErrorFigure),
            "the whole area turns it over"
        );
        // Centred.
        let (_, tr) = texts(&big).into_iter().find(|x| x.0 == t).unwrap();
        assert!(
            ((tr.x + tr.w / 2.0) - 440.0).abs() < 1.0,
            "{what}: centred, {tr:?}"
        );
        // Medium: still the title and the error.
        let mid = symbolic(one, (400.0, 240.0));
        tidy(&mid, what);
        assert!(has_text(&mid, &t), "{what} at 400x240: {:?}", texts(&mid));
        // Small: the glyph and its status, which says it's a failure in words; narrower, the
        // glyph alone; then, too short for it, the title alone.
        let small = symbolic(one, (200.0, 120.0));
        tidy(&small, what);
        let words: Vec<String> = texts(&small).into_iter().map(|x| x.0).collect();
        assert_eq!(
            words,
            vec![label.to_string()],
            "{what}: the glyph and its status"
        );
        assert!(paths(&small) >= 3, "{what}: its glyph and the status icon");
        let narrow = symbolic(one, (60.0, 120.0));
        tidy(&narrow, what);
        assert!(
            texts(&narrow).is_empty(),
            "{what}: the glyph only, {:?}",
            texts(&narrow)
        );
        assert!(paths(&narrow) >= 2, "{what}: its glyph");
        let tiny = symbolic(one, (200.0, 44.0));
        tidy(&tiny, what);
        let words: Vec<String> = texts(&tiny).into_iter().map(|x| x.0).collect();
        assert_eq!(words.len(), 1, "{what}: the title only, {words:?}");
        assert!(words[0].starts_with(&t[..6]), "{words:?}");
        assert_eq!(paths(&tiny), 0, "no glyph");
    }
}

#[test]
fn the_symbolic_figure_drops_its_hint_before_the_message_s_second_line() {
    // A message of two lines at a size where both fit, or the hint and one: both lines.
    let e = FigError::at(
        "no column \"latency_p50\" in the data",
        Pos {
            line: 8,
            col: 40,
            byte: 0,
        },
    )
    .with_kind(ErrorKind::Mismatch);
    let said = die_fig::plot::errors::summary(&e);
    let mut found = false;
    for h in (150..=260).step_by(2) {
        let s = symbolic(std::slice::from_ref(&e), (340.0, h as f32));
        tidy(&s, "two lines");
        let ts = texts(&s);
        let lines: Vec<&String> = ts
            .iter()
            .map(|t| &t.0)
            .filter(|t| said.starts_with(t.as_str()) || said.ends_with(t.as_str()))
            .collect();
        let hint = has_text(&s, "details");
        if lines.len() == 2 && !hint {
            // The rung that matters: the whole message, the hint gone for it.
            let joined = format!("{} {}", lines[0], lines[1]);
            assert_eq!(joined, said, "at 340x{h}: the whole message");
            found = true;
        }
        // Never the hint with a message cut short.
        if hint {
            assert!(
                !ts.iter().any(|t| t.0.ends_with('…')),
                "at 340x{h}: the hint kept over the message: {ts:?}"
            );
        }
    }
    assert!(found, "some height shows both lines without the hint");
}

#[test]
fn the_status_is_critical_for_failures_and_a_warning_for_nothing_to_draw() {
    let theme = Theme::mono_dark();
    let fills = |s: &Scene| -> Vec<Color> {
        s.items
            .iter()
            .filter_map(|i| match i {
                Item::Path {
                    fill: Some(Paint::Solid { color, .. }),
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    };
    for e in kinds() {
        let s = symbolic(std::slice::from_ref(&e), (880.0, 470.0));
        let want = if e.kind == ErrorKind::Empty {
            theme.status[1]
        } else {
            theme.status[3]
        };
        assert!(fills(&s).contains(&want), "{}'s icon", e.kind.name());
    }
    // Several errors: the first named, the count beside the status.
    let s = symbolic(&kinds()[..3], (880.0, 470.0));
    assert!(has_text(&s, " · 3 errors"), "{:?}", texts(&s));
}

fn logged(errors: &[FigError], size: (f32, f32), top: usize, mark: usize) -> Scene {
    error_figure(
        errors,
        SPEC,
        "requests.fig",
        ErrorView::Log { top, mark },
        size,
        2.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    )
}

#[test]
fn the_log_writes_each_entry_as_rustc_does_with_a_caret_under_its_column() {
    let mut errors = kinds();
    errors.push(
        FigError::at(
            "unknown field \"titel\"; did you mean \"title\"?",
            Pos {
                line: 2,
                col: 3,
                byte: 0,
            },
        )
        .warn(),
    );
    let s = logged(&errors, (880.0, 470.0), 0, 0);
    let ts = texts(&s);
    let has = |t: &str| ts.iter().any(|x| x.0 == t);
    assert!(
        has("unreadable") && has(": requests-old.csv: no such file"),
        "{ts:?}"
    );
    assert!(has("requests.fig:3:11"), "the file and line:column");
    assert!(has("  \"data\": \"requests-old.csv\","), "the source line");
    assert!(has("looked for requests-old.csv"), "what was tried");
    assert!(has("4 errors · 1 warning"), "the header's count: {ts:?}");
    // The caret under column 11, the quoted name underlined whole, in the monospace.
    let mono = die_fig::scene::TextSpec::new(12.0).mono();
    let cell = die_fig::scene::Measure::width(&mut FixedMeasure, "0", mono);
    let (_, src) = ts
        .iter()
        .find(|x| x.0 == "  \"data\": \"requests-old.csv\",")
        .unwrap();
    let (caret, cr) = ts.iter().find(|x| x.0.starts_with('^')).unwrap();
    assert_eq!(caret.len(), "\"requests-old.csv\"".len());
    assert!(
        (cr.x - (src.x + 10.0 * cell)).abs() < 0.5,
        "under its column: {cr:?} {src:?}"
    );
    // Each entry's region, the header's that turns it over, the first entry lit.
    let entries: Vec<u32> = s
        .regions
        .iter()
        .filter_map(|r| match r.hit {
            Hit::LogEntry(i) => Some(i),
            _ => None,
        })
        .collect();
    assert!(entries.starts_with(&[0, 1]), "{entries:?}");
    assert_eq!(s.hit(400.0, LOG_HEADER / 2.0), Some(&Hit::ErrorFigure));
    // No text leaves the area or overlaps another, at three sizes.
    for size in [(880.0, 470.0), (400.0, 240.0), (200.0, 120.0)] {
        let s = logged(&errors, size, 0, 0);
        for (t, r) in texts(&s) {
            assert!(
                r.x >= -0.5 && r.right() <= size.0 + 0.5 && r.bottom() <= size.1 + 0.5,
                "{t:?} at {r:?} leaves {size:?}"
            );
        }
    }
}

#[test]
fn the_log_scrolls_by_lines_and_keeps_a_long_line_s_caret_in_view() {
    let errors = kinds();
    let lines = log_layout(&errors, SPEC, "spec.fig");
    assert_eq!(lines.starts.len(), 4);
    assert_eq!(lines.starts[0], 0);
    for i in 1..4 {
        assert_eq!(
            lines.starts[i],
            lines.starts[i - 1] + lines.lens[i - 1] + 1,
            "a blank line between entries"
        );
    }
    assert_eq!(lines.entry_at(lines.starts[1]), Some(1));
    assert_eq!(lines.entry_at(lines.starts[1] - 1), None, "the blank");
    let page = log_page((880.0, 200.0));
    assert_eq!(page, ((200.0 - LOG_HEADER - 10.0) / LOG_LINE) as usize);
    assert_eq!(lines.max_top(page), lines.total.saturating_sub(page));
    let top = lines.reveal(0, 3, page);
    assert!(
        top + page > lines.starts[3] && top <= lines.starts[3],
        "{top}"
    );
    // A top past the end is the last page's.
    let s = logged(&errors, (880.0, 200.0), 999, 3);
    let last = texts(&s)
        .into_iter()
        .any(|x| x.0.contains("the data has no rows"));
    assert!(last, "{:?}", texts(&s));
    // A source line wider than the area: cut with '…', its caret still on screen.
    let long = format!("{{\"a\": \"{}\", \"y\": \"latency\"}}", "x".repeat(300));
    let e = FigError::at(
        "no column \"latency\" in the data",
        Pos {
            line: 1,
            col: 312,
            byte: 0,
        },
    );
    let s = error_figure(
        std::slice::from_ref(&e),
        &long,
        "wide.fig",
        ErrorView::Log { top: 0, mark: 0 },
        (400.0, 240.0),
        1.0,
        &Theme::mono_dark(),
        &mut FixedMeasure,
    );
    let ts = texts(&s);
    let (_, caret) = ts.iter().find(|x| x.0.starts_with('^')).expect("a caret");
    assert!(caret.right() <= 400.0, "the caret in view: {caret:?}");
    assert!(ts.iter().any(|x| x.0.starts_with('…')), "{ts:?}");
    let (entries, _) = log_entries(std::slice::from_ref(&e), &long, "wide.fig");
    assert_eq!(entries[0].source, Some(3));
}

#[test]
fn the_stale_badge_says_error_and_its_line_in_the_top_right_corner() {
    let theme = Theme::mono_dark();
    let e = FigError::malformed(
        "expected ',' or '}'",
        Pos {
            line: 14,
            col: 9,
            byte: 0,
        },
    );
    assert_eq!(badge_text(&e), "Error · line 14");
    let data = FigError::at(
        "\"slow\" is not a number",
        Pos {
            line: 6,
            col: 3,
            byte: 0,
        },
    )
    .in_file("/d/lat.csv", None);
    assert_eq!(badge_text(&data), "Error · lat.csv, line 6");
    let empty = FigError::new("nothing to draw").with_kind(ErrorKind::Empty);
    assert_eq!(badge_text(&empty), "Empty");
    for (w, h) in [
        (880.0, 470.0),
        (400.0, 240.0),
        (200.0, 120.0),
        (60.0, 120.0),
    ] {
        let mut s = Scene::new(w, h, 2.0);
        stale_badge(&mut s, &e, &theme, &mut FixedMeasure);
        let r = s
            .regions
            .iter()
            .find(|r| r.hit == Hit::Badge)
            .expect("its region")
            .rect;
        assert!(
            r.x >= 0.0 && r.right() <= w && r.y >= 0.0,
            "inside at {w}: {r:?}"
        );
        assert!(
            r.x + r.w / 2.0 > w / 2.0 && r.y < h / 4.0,
            "top right at {w}: {r:?}"
        );
        tidy(&s, "badge");
        for (t, tr) in texts(&s) {
            assert!(
                tr.right() <= r.right() && tr.x >= r.x,
                "{t:?} inside the badge"
            );
        }
        if w >= 400.0 {
            assert!(has_text(&s, "Error · line 14"), "{:?}", texts(&s));
        }
    }
    // Over a chart, the badge is pushed last: over everything, and hit first.
    let model = load("requests.csv");
    let (mut s, _, _) = lay(&model, &View::default(), (880.0, 470.0));
    stale_badge(&mut s, &e, &theme, &mut FixedMeasure);
    let r = s.regions.last().unwrap();
    assert_eq!(r.hit, Hit::Badge);
    assert_eq!(s.hit(r.rect.x + 4.0, r.rect.y + 4.0), Some(&Hit::Badge));
}

#[test]
fn the_stale_badge_never_covers_the_title_the_legend_or_a_label() {
    let theme = Theme::mono_dark();
    let line = FigError::at(
        "expected ',' or '}'",
        Pos {
            line: 14,
            col: 9,
            byte: 0,
        },
    );
    let data = FigError::at(
        "\"timeout\" in \"ms\" is not a number",
        Pos {
            line: 6,
            col: 22,
            byte: 0,
        },
    )
    .in_file(std::path::Path::new("/tmp/latency-ms.csv"), None);
    let empty = FigError::new("nothing to draw: the data has no rows").with_kind(ErrorKind::Empty);
    for name in [
        "requests.csv",
        "services-bars.csv",
        "thresholds.json",
        "many.json",
        "hbars.json",
        "latency.json",
    ] {
        let model = load(name);
        for size in [(880.0, 470.0), (400.0, 240.0), (200.0, 120.0)] {
            for e in [&line, &data, &empty] {
                let (mut s, _, _) = lay(&model, &View::default(), size);
                let n = s.items.len();
                let keys: Vec<Rect> = s
                    .regions
                    .iter()
                    .filter(|r| matches!(r.hit, Hit::LegendKey(_)))
                    .map(|r| r.rect)
                    .collect();
                stale_badge(&mut s, e, &theme, &mut FixedMeasure);
                let r = s.regions.last().unwrap();
                assert_eq!(r.hit, Hit::Badge);
                let b = r.rect;
                let what = format!("{name} at {size:?}, {:?}", e.msg);
                assert!(
                    b.x >= 0.0 && b.right() <= size.0 && b.y >= 0.0,
                    "{what}: inside: {b:?}"
                );
                // Every text of the chart's as it's drawn now (a title cut for the badge with
                // its ellipsis), the badge's own after them.
                let mut m = FixedMeasure;
                for item in &s.items[..n] {
                    let Item::Text {
                        text,
                        x,
                        y,
                        spec,
                        anchor,
                        max_width,
                        ..
                    } = item
                    else {
                        continue;
                    };
                    let mut tr = text_box(text, *x, *y, *spec, *anchor, &mut m);
                    if let Some(mw) = max_width {
                        tr.w = tr.w.min(*mw);
                    }
                    assert!(
                        !overlaps(&b, &tr),
                        "{what}: the badge {b:?} covers {text:?} at {tr:?}"
                    );
                }
                for k in &keys {
                    assert!(
                        !overlaps(&b, k),
                        "{what}: the badge {b:?} covers the key {k:?}"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the phase 2 gallery's fixes

/// Bars and totals as a card must keep clear of them where it can (the overlay's own margin).
fn clear(card: &Rect, avoid: &[Rect]) -> bool {
    avoid.iter().all(|a| !overlaps(card, &a.inflate(8.0)))
}

/// Whether a card of `w × h` has a spot in the area (4 px in, over the plot's bottom) that keeps
/// clear of `avoid`: searched on a 2 px grid.
fn free_spot_exists(l: &ChartLayout, w: f32, h: f32, avoid: &[Rect]) -> bool {
    free_spot_where(l, w, h, avoid, |_| true)
}

/// [`free_spot_exists`], among the spots `ok` takes.
fn free_spot_where(
    l: &ChartLayout,
    w: f32,
    h: f32,
    avoid: &[Rect],
    ok: impl Fn(&Rect) -> bool,
) -> bool {
    let (aw, ah) = l.size;
    let floor = if h <= l.plot.bottom() - 8.0 {
        l.plot.bottom() - 4.0
    } else {
        ah - 4.0
    };
    let mut y = 4.0;
    while y + h <= floor {
        let mut x = 4.0;
        while x + w <= aw - 4.0 {
            let r = Rect::new(x, y, w, h);
            if ok(&r) && clear(&r, avoid) {
                return true;
            }
            x += 2.0;
        }
        y += 2.0;
    }
    false
}

#[test]
fn a_bar_s_card_covers_no_bar_where_there_is_room() {
    for name in [
        "services-bars.csv",
        "latency.json",
        "hbars.json",
        "profit.csv",
        "deploys.json",
    ] {
        let model = load(name);
        let view = View::default();
        let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
        let bars: Vec<Rect> = l.bars.iter().map(|b| b.rect).collect();
        assert!(!bars.is_empty(), "{name} has bars");
        let mut seen = 0;
        for b in &l.bars {
            // The pointer on the bar's band at the plot's middle, across or down as it lies.
            let c = (b.rect.x + b.rect.w / 2.0, b.rect.y + b.rect.h / 2.0);
            let p = if l.horizontal {
                (l.plot.x + l.plot.w / 2.0, c.1)
            } else {
                (c.0, l.plot.y + l.plot.h / 2.0)
            };
            let hover = plot::hover(&model, &l, &hits, &view, p.0, p.1);
            let o = overlay(
                &model,
                &l,
                &view,
                &OverlayState {
                    hover,
                    pointer: Some(p),
                    ..state()
                },
            );
            let Some(card) = shadow(&o) else { continue };
            seen += 1;
            let mut avoid = bars.clone();
            avoid.extend(l.totals.iter().map(|t| t.1));
            avoid.push(Rect::new(p.0 - 6.0, p.1 - 6.0, 12.0, 12.0));
            if !l.horizontal && !model.is_band() {
                // Bars over time: the crosshair's column too.
                avoid.push(Rect::new(p.0 - 16.0, l.plot.y - 6.0, 32.0, l.plot.h + 6.0));
            }
            if free_spot_exists(&l, card.w, card.h, &avoid) {
                for r in &bars {
                    assert!(
                        !overlaps(&card, r),
                        "{name}: the card {card:?} covers the bar {r:?}, hovered at {p:?}"
                    );
                }
            }
        }
        assert!(seen > 0, "{name}: a card on some band");
    }
}

#[test]
fn a_band_s_card_stays_by_its_band_where_its_band_or_a_neighbour_has_room() {
    for name in ["services-bars.csv", "latency.json", "hbars.json"] {
        let model = load(name);
        let view = View::default();
        let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
        let bars: Vec<Rect> = l.bars.iter().map(|b| b.rect).collect();
        let scale = if l.horizontal { &l.y } else { &l.x };
        let (a0, _) = scale.band_of(0).unwrap();
        let (a1, _) = scale.band_of(1).unwrap();
        let step = (a1 - a0).abs();
        let mut band = 0;
        while let Some((a, bw)) = scale.band_of(band) {
            let home = a + bw / 2.0;
            // The pointer low on the band (as the gallery hovered services-bars), and in the
            // plot's middle.
            for along in [0.5, 0.85] {
                let p = if l.horizontal {
                    (l.plot.x + l.plot.w * (1.0 - along), home)
                } else {
                    (home, l.plot.y + l.plot.h * along)
                };
                let (hover, o) = hovered(&model, &l, &hits, &view, p);
                assert!(
                    matches!(hover, Some(Cursor::Band { .. })),
                    "{name}: {hover:?}"
                );
                let Some(card) = shadow(&o) else { continue };
                let mut avoid = bars.clone();
                avoid.extend(l.totals.iter().map(|t| t.1));
                avoid.push(Rect::new(p.0 - 4.0, p.1 - 4.0, 8.0, 8.0));
                let centre = |r: &Rect| {
                    if l.horizontal {
                        r.y + r.h / 2.0
                    } else {
                        r.x + r.w / 2.0
                    }
                };
                // Over its own band or its neighbour: no further than a band and a half.
                let near = |r: &Rect| (centre(r) - home).abs() < 1.5 * step;
                if free_spot_where(&l, card.w, card.h, &avoid, near) {
                    assert!(
                        near(&card),
                        "{name}: band {band}'s card at {card:?} strays from {home} (step {step})"
                    );
                }
            }
            band += 1;
        }
        assert!(band > 2, "{name}: bands");
    }
}

#[test]
fn a_scatter_pads_by_its_dots_and_non_negative_data_shows_no_negative_tick() {
    let model = load("scatter.csv");
    let (scene, l, _) = lay(&model, &View::default(), (880.0, 470.0));
    assert!(
        l.y_ticks.iter().all(|t| t.value >= 0.0),
        "no negative tick: {:?}",
        l.y_ticks
    );
    assert!(l.x_ticks.iter().all(|t| t.value >= 0.0));
    // The domain reaches past the data by about a dot, not by a tick.
    let lo = model
        .series
        .iter()
        .flat_map(|s| s.ys.iter().copied())
        .fold(f64::INFINITY, f64::min);
    let (y0, y1) = l.domain.1;
    let per_px = (y1 - y0) / f64::from(l.plot.h);
    let past = (lo - y0) / per_px;
    assert!(
        (4.0..=16.0).contains(&past),
        "{past} px under the lowest dot: {:?}",
        l.domain
    );
    // Every dot whole inside the plot (the legend's dots are above it).
    let p = l.plot;
    let near = p.inflate(12.0);
    for (_, (cx, cy), r) in circles(&scene) {
        if !near.contains(cx, cy) {
            continue;
        }
        assert!(
            cx - r >= p.x - 0.5 && cx + r <= p.right() + 0.5,
            "({cx}, {cy}) r {r} across {p:?}"
        );
        assert!(cy - r >= p.y - 0.5 && cy + r <= p.bottom() + 0.5);
    }
    // Smaller, the top tick still labels most of the dots (10 over 8.8, not 5), and a plot
    // too short to pad lets its dots past its edges whole rather than cutting them.
    let hi = model
        .series
        .iter()
        .flat_map(|s| s.ys.iter().copied())
        .fold(f64::NEG_INFINITY, f64::max);
    for size in [(400.0, 240.0), (340.0, 200.0)] {
        let (_, l, _) = lay(&model, &View::default(), size);
        let top = l.y_ticks.last().map_or(f64::NAN, |t| t.value);
        assert!(
            top >= lo + 0.75 * (hi - lo),
            "{size:?}: the top tick {top} labels most of {lo} to {hi}: {:?}",
            l.y_ticks
        );
        assert!(l.y_ticks.iter().all(|t| t.value >= 0.0));
        assert!(l.domain.1.1 >= top, "the tick inside the domain");
    }
    let (scene, l, _) = lay(&model, &View::default(), (200.0, 120.0));
    let p = l.plot;
    for (_, (cx, cy), r) in circles(&scene) {
        if !p.inflate(12.0).contains(cx, cy) || cy < p.y - r - 1.0 {
            continue;
        }
        assert!(
            cy - r >= l.clip.y - 0.5 && cy + r <= l.clip.bottom() + 0.5,
            "200x120: the dot at ({cx}, {cy}) r {r} whole in the clip {:?}",
            l.clip
        );
    }
}

#[test]
fn a_number_x_s_card_header_is_its_title_and_value_without_a_colon() {
    let model = load("many.json");
    let view = View::default();
    let (_, l, hits) = lay(&model, &view, (880.0, 470.0));
    let p = (l.plot.x + l.plot.w * 0.55, l.plot.y + l.plot.h / 2.0);
    let (hover, o) = hovered(&model, &l, &hits, &view, p);
    assert!(hover.is_some());
    let header = texts(&o)
        .into_iter()
        .map(|t| t.0)
        .find(|t| t.starts_with("Hour"))
        .expect("the header");
    assert!(!header.contains(':'), "{header}");
    assert!(header.starts_with("Hour "), "{header}");
}

#[test]
fn a_cut_ends_at_a_word_with_nothing_dangling_before_its_ellipsis() {
    use die_fig::plot::errors::fit_words;
    use die_fig::scene::TextSpec;
    let spec = TextSpec::new(10.0);
    let mut m = FixedMeasure;
    // 6 px a char: room for so many chars and the '…'.
    let room = |n: usize| n as f32 * 6.0 + 6.0;
    assert_eq!(
        fit_words("short", spec, 100.0, &mut m),
        "short",
        "whole when it fits"
    );
    // A space before the cut goes.
    assert_eq!(
        fit_words("malformed: expected ',' or '}'", spec, room(20), &mut m),
        "malformed: expected…"
    );
    // A full stop before it too: no four dots.
    assert_eq!(
        fit_words("Error · latency-ms.csv, line 6", spec, room(19), &mut m),
        "Error · latency-ms…"
    );
    // Mid-word with a word's end near: there.
    assert_eq!(
        fit_words(
            "\"timeout\" in \"ms\" is not a number",
            spec,
            room(19),
            &mut m
        ),
        "\"timeout\" in \"ms\"…"
    );
    // Mid-word with no end near: where it was cut.
    assert_eq!(
        fit_words("abcdefghijklmnopqrstuvwxyz", spec, room(10), &mut m),
        "abcdefghij…"
    );
    // In the log, a line cut leaves nothing dangling either.
    let e = FigError::at(
        "expected ',' or '}' after the value, as JSON objects are written",
        Pos {
            line: 2,
            col: 7,
            byte: 0,
        },
    );
    for w in (160..=420).step_by(4) {
        let s = error_figure(
            std::slice::from_ref(&e),
            "[{\"x\": 1, \"y\": 2},\n {\"x\" 2}]\n",
            "data.json",
            ErrorView::Log { top: 0, mark: 0 },
            (w as f32, 240.0),
            2.0,
            &Theme::mono_dark(),
            &mut FixedMeasure,
        );
        // The log draws each line's spans one after another: a span of '…' after a run
        // that ends in a space or a dot is what's wrong.
        let items: Vec<String> = texts(&s).into_iter().map(|t| t.0).collect();
        for pair in items.windows(2) {
            if pair[1] == "…" {
                let c = pair[0].chars().last().unwrap();
                assert!(
                    !(c.is_whitespace() || matches!(c, '.' | ',' | ':')),
                    "at {w}: {:?} before its '…'",
                    pair[0]
                );
            }
        }
    }
}

#[test]
fn a_long_source_line_in_the_log_shifts_by_the_least_that_shows_its_caret() {
    let line =
        "    {\"mark\": \"line\", \"x\": \"time\", \"y\": \"latency_p50\"}, {\"mark\": \"area\"},";
    let source = format!("{{\n  \"layers\": [\n{line}\n  ]\n}}\n");
    let c0 = line.find("\"latency_p50\"").unwrap();
    let n = "\"latency_p50\"".len();
    let e = FigError::at(
        "no column \"latency_p50\" in the data",
        Pos {
            line: 3,
            col: c0 as u32 + 1,
            byte: 0,
        },
    );
    let mut checked = 0;
    for w in (300..=640).step_by(2) {
        let s = error_figure(
            std::slice::from_ref(&e),
            &source,
            "many.fig",
            ErrorView::Log { top: 0, mark: 0 },
            (w as f32, 300.0),
            2.0,
            &Theme::mono_dark(),
            &mut FixedMeasure,
        );
        let ts = texts(&s);
        let i = ts
            .iter()
            .position(|t| t.0 == "3 | ")
            .expect("the source line's gutter");
        let shown = &ts[i + 1].0;
        // The log's cells as it counts them.
        let cell = die_fig::scene::Measure::width(
            &mut FixedMeasure,
            "0",
            die_fig::scene::TextSpec::new(12.0).mono(),
        );
        let pad = 20f32.min(w as f32 / 12.0).round();
        let cells = ((w as f32 - pad - 6.0 - (pad + 4.0)) / cell).floor() as usize;
        let room = cells - ("3 | ".len() + 1);
        if c0 + n <= room {
            assert!(
                line.starts_with(shown.as_str()),
                "at {w}: no shift: {shown:?}"
            );
            continue;
        }
        // The least shift that shows the caret and four cells after it.
        let shift = (c0 + n + 4 - room).min(c0 - 2);
        let rest: String = line.chars().skip(shift + 1).collect();
        assert_eq!(
            shown
                .strip_prefix('…')
                .map(|r| rest.starts_with(r.trim_end_matches('…'))),
            Some(true),
            "at {w}: {shown:?}, shifted {shift}"
        );
        let (_, caret) = ts.iter().find(|t| t.0.starts_with('^')).expect("a caret");
        assert!(caret.right() <= w as f32 - pad, "at {w}: the caret in view");
        checked += 1;
    }
    assert!(checked > 10, "the shift was seen");
}

#[test]
fn the_log_of_nothing_to_draw_says_so_not_one_error_and_its_entry_says_it_once() {
    let e = FigError::new("nothing to draw: the data has no rows").with_kind(ErrorKind::Empty);
    let s = logged(std::slice::from_ref(&e), (880.0, 470.0), 0, 0);
    let ts: Vec<String> = texts(&s).into_iter().map(|t| t.0).collect();
    assert!(ts.iter().any(|t| t == "Nothing to draw"), "{ts:?}");
    assert!(
        !ts.iter().any(|t| t.contains("error")),
        "not an error: {ts:?}"
    );
    assert!(ts.iter().any(|t| t == ": the data has no rows"), "{ts:?}");
    assert!(!ts.iter().any(|t| t.contains("nothing to draw:")), "{ts:?}");
    // A real error with it: counted as errors.
    let bad = FigError::new("no column \"y\" in the data");
    let s = logged(&[bad, e], (880.0, 470.0), 0, 0);
    assert!(has_text(&s, "2 errors"), "{:?}", texts(&s));
}

#[test]
fn the_log_s_keys_give_way_from_the_right_before_the_figure_s_name_is_cut() {
    let e = FigError::new("expected ',' or '}'");
    let keys = |s: &Scene| -> Vec<String> {
        let ts: Vec<String> = texts(s).into_iter().map(|t| t.0).collect();
        ["figure", "go to", "move"]
            .iter()
            .filter(|k| ts.iter().any(|t| t == *k))
            .map(|k| k.to_string())
            .collect()
    };
    // Wide: every key, the name whole.
    let s = logged(std::slice::from_ref(&e), (880.0, 470.0), 0, 0);
    assert!(has_text(&s, " in requests.fig"), "{:?}", texts(&s));
    assert_eq!(keys(&s), ["figure", "go to", "move"]);
    // Narrower: move goes first, then go to, and the name stays whole while ! figure fits.
    let mut seen = vec![];
    for w in (240..=880).rev().step_by(8) {
        let s = logged(std::slice::from_ref(&e), (w as f32, 240.0), 0, 0);
        let k = keys(&s);
        if !k.is_empty() {
            assert!(has_text(&s, " in requests.fig"), "at {w}: {:?}", texts(&s));
            assert_eq!(k[0], "figure", "at {w}: ! figure is the last to go");
        }
        if seen.last() != Some(&k.len()) {
            seen.push(k.len());
        }
    }
    assert!(seen.starts_with(&[3, 2, 1]), "{seen:?}");
}

#[test]
fn a_short_plot_drops_value_ticks_until_they_clear_each_other() {
    // Call 1432: at 200 × 120, thresholds.json and many.json drew their ticks on each other.
    for name in [
        "thresholds.json",
        "many.json",
        "requests.csv",
        "stacked-area.json",
    ] {
        let model = load(name);
        for size in [(200.0, 120.0), (220.0, 140.0), (260.0, 160.0)] {
            let (scene, l, _) = lay(&model, &View::default(), size);
            let ticks: Vec<Rect> = texts(&scene)
                .into_iter()
                .filter(|(t, _)| {
                    l.labels
                        .iter()
                        .any(|x| x.role == Role::YTick && x.text == *t)
                })
                .map(|(_, r)| r)
                .collect();
            for (i, a) in ticks.iter().enumerate() {
                for b in &ticks[i + 1..] {
                    assert!(!overlaps(a, b), "{name} at {size:?}: {a:?} and {b:?}");
                }
            }
        }
    }
}

#[test]
fn every_error_glyph_stays_inside_its_disc() {
    // The mismatch glyph's right piece read as poking past its disc (errors-mismatch.png).
    use die_fig::ErrorKind;
    let r = Rect::new(10.0, 10.0, 96.0, 96.0);
    let (cx, cy, radius) = (58.0, 58.0, 48.0);
    for kind in [
        ErrorKind::Unreadable,
        ErrorKind::Malformed,
        ErrorKind::Mismatch,
        ErrorKind::Empty,
    ] {
        let mut scene = Scene::new(120.0, 120.0, 1.0);
        plot::errors::glyph(&mut scene, kind, r, &Theme::mono_dark());
        // Past the disc itself, every point a path draws or bends towards, with half its
        // stroke, keeps within 82 % of the radius: the document and the axes reach about 80 %.
        for item in scene.items.iter().skip(1) {
            let Item::Path { path, stroke, .. } = item else {
                continue;
            };
            let half = stroke.as_ref().map_or(0.0, |s| s.width / 2.0);
            for v in &path.verbs {
                let pts: Vec<(f32, f32)> = match *v {
                    Verb::Move(x, y) | Verb::Line(x, y) => vec![(x, y)],
                    Verb::Quad(a, b, x, y) => vec![(a, b), (x, y)],
                    Verb::Cubic(a, b, c, d, x, y) => vec![(a, b), (c, d), (x, y)],
                    Verb::Close => vec![],
                };
                for (x, y) in pts {
                    let reach = (x - cx).hypot(y - cy) + half;
                    assert!(
                        reach <= radius * 0.82,
                        "{kind:?}: ({x}, {y}) reaches {reach}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_short_chart_drops_its_legend_axis_titles_and_title_before_its_plot() {
    // many.json: a title, twelve series' legend and both axes' titles. Beside the detail panel
    // at the bottom of a 400 × 240 area it has 144 px; in a tiny tile, 120.
    let model = load("many.json");
    for size in [(400.0, 144.0), (200.0, 120.0)] {
        let (scene, l, _) = lay(&model, &View::default(), size);
        assert!(l.plot.h >= 48.0, "{size:?}: a plot of {} px", l.plot.h);
        for (t, r) in texts(&scene) {
            assert!(
                r.x >= -0.5
                    && r.y >= -0.5
                    && r.right() <= size.0 + 0.5
                    && r.bottom() <= size.1 + 0.5,
                "{size:?}: {t} at {r:?} is cut at the edge"
            );
        }
    }
    // At 200 × 120 the x axis keeps two labels, thinned, not one.
    let (scene, l, _) = lay(&model, &View::default(), (200.0, 120.0));
    let under = texts(&scene)
        .into_iter()
        .filter(|(_, r)| r.y >= l.plot.bottom())
        .count();
    assert!(under >= 2, "{under} x labels");
    // It keeps what it has room for: at 880 × 470 everything.
    let (scene, _, _) = lay(&model, &View::default(), (880.0, 470.0));
    assert!(has_text(&scene, "Queue depth by region"));
}

#[test]
fn beside_the_panel_a_chart_keeps_the_areas_paddings() {
    let model = load("requests.csv");
    let (_, whole, _) = lay(&model, &View::default(), (880.0, 470.0));
    let (_, beside, _) = plot::base_beside(
        &model,
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

/// The look pass after phase 3: at 200x120 the SLO's label sat over checkout's spike.
#[test]
fn a_rules_label_goes_where_no_line_crosses_it() {
    let model = load("thresholds.json");
    for size in [(200.0, 120.0), (400.0, 240.0), (880.0, 470.0)] {
        let (scene, l, _) = lay(&model, &View::default(), size);
        let plate = scene
            .items
            .iter()
            .find_map(|i| match i {
                Item::Path {
                    path,
                    fill: Some(Paint::Solid { alpha, .. }),
                    stroke: None,
                } if (*alpha - 0.85).abs() < 1e-3 => {
                    let pts: Vec<(f32, f32)> = path
                        .verbs
                        .iter()
                        .flat_map(|v| match *v {
                            Verb::Move(x, y) | Verb::Line(x, y) => vec![(x, y)],
                            Verb::Quad(a, b, x, y) => vec![(a, b), (x, y)],
                            Verb::Cubic(a, b, c, d, x, y) => vec![(a, b), (c, d), (x, y)],
                            Verb::Close => vec![],
                        })
                        .collect();
                    let x0 = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                    let x1 = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                    let y0 = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                    let y1 = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
                    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
                }
                _ => None,
            })
            .expect("the SLO's label on its plate");
        for s in &model.series {
            let pts: Vec<(f32, f32)> =
                s.xs.iter()
                    .zip(s.ys.iter())
                    .map(|(&x, &y)| l.at(x, y))
                    .collect();
            for w in pts.windows(2) {
                // Sampled along the segment: none of it inside the plate.
                for k in 0..=16 {
                    let t = k as f32 / 16.0;
                    let p = (
                        w[0].0 + (w[1].0 - w[0].0) * t,
                        w[0].1 + (w[1].1 - w[0].1) * t,
                    );
                    assert!(
                        !plate.contains(p.0, p.1),
                        "{size:?}: {} runs through the SLO's label",
                        s.name
                    );
                }
            }
        }
    }
}
