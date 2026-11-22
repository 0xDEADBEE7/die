//! Heatmaps: the colour scales and their legend, the cells' gaps, the hover's ring and card,
//! the cursor's motions by cell and the box brush snapped to whole cells, over the fixtures
//! `heatmap.json` (requests by hour and weekday) and `heatmap-diverging.json` (a change from
//! last week), measured with `FixedMeasure`.

use die_test_cap as _;

use die_fig::interact::{Action, BrushKind, Cursor, View};
use die_fig::plot::layout::{Role, text_box};
use die_fig::plot::nav;
use die_fig::plot::ramp::{self, ColorScale};
use die_fig::plot::{self, ChartLayout, ChartModel, OverlayState};
use die_fig::scene::{FixedMeasure, Item, Paint, Rect, Scene, Stroke};
use die_fig::spec::{self, Figure, LegendPos};
use die_fig::theme::{Color, Theme};
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};

const SIZES: &[(f32, f32)] = &[(400.0, 240.0), (880.0, 470.0), (1800.0, 1000.0)];

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("data")
}

fn model_of(text: &str, name: &str) -> ChartModel {
    let opt = die_fig::data::ReadOptions::new(TimeZone::UTC);
    let parsed = spec::parse(text, name, &data_dir(), &opt).expect("it parses");
    let Figure::Chart(chart) = parsed.figure else {
        panic!("{name} is a chart");
    };
    let tables = plot::read_tables(&chart, Some(text), &opt).expect("its data");
    ChartModel::build(chart, tables, &TimeZone::UTC).expect("its model")
}

fn load(name: &str) -> ChartModel {
    let text = std::fs::read_to_string(data_dir().join(name)).expect("the fixture");
    model_of(&text, name)
}

fn lay(model: &ChartModel, view: &View, size: (f32, f32), theme: &Theme) -> (Scene, ChartLayout) {
    let (s, l, _) = plot::base(model, view, size, 1.0, theme, &mut FixedMeasure);
    (s, l)
}

fn state(hover: Option<Cursor>, cursor: Option<Cursor>) -> OverlayState {
    OverlayState {
        hover,
        cursor,
        pointer: None,
        brush: None,
        emphasis: None,
    }
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

/// The filled rounded cells of a base scene, in drawing order.
fn cells(scene: &Scene, l: &ChartLayout) -> Vec<(Rect, Color)> {
    scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Path {
                path,
                fill: Some(Paint::Solid { color, .. }),
                stroke: None,
            } => {
                let b = path_bounds(path)?;
                (l.plot.inflate(0.5).contains(b.x, b.y)
                    && l.plot.inflate(0.5).contains(b.right(), b.bottom()))
                .then_some((b, *color))
            }
            _ => None,
        })
        .collect()
}

fn path_bounds(p: &die_fig::scene::PathData) -> Option<Rect> {
    use die_fig::scene::Verb;
    let mut pts = Vec::new();
    for v in &p.verbs {
        match *v {
            Verb::Move(x, y) | Verb::Line(x, y) => pts.push((x, y)),
            Verb::Quad(_, _, x, y) => pts.push((x, y)),
            Verb::Cubic(_, _, _, _, x, y) => pts.push((x, y)),
            Verb::Close => {}
        }
    }
    let (x0, x1) = pts
        .iter()
        .fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (y0, y1) = pts
        .iter()
        .fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    (!pts.is_empty()).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// OKLab lightness, near enough to order colours.
fn lightness(c: Color) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(c.0), lin(c.1), lin(c.2));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s
}

#[test]
fn a_sequential_ramp_runs_one_way_and_flips_on_dark() {
    let sc = ColorScale::Sequential { lo: 0.0, hi: 100.0 };
    for theme in [Theme::light(), Theme::mono_dark()] {
        let ls: Vec<f32> = (0..=20)
            .map(|i| lightness(sc.color(f64::from(i) * 5.0, &theme).unwrap()))
            .collect();
        // Light: darker as values grow; dark: lighter. Never back.
        let rising = ls.windows(2).all(|w| w[1] >= w[0] - 1e-4);
        let falling = ls.windows(2).all(|w| w[1] <= w[0] + 1e-4);
        assert!(if theme.dark { rising } else { falling }, "{ls:?}");
        // The ends are the documented steps.
        let steps = sc.steps(&theme);
        assert_eq!(sc.color(0.0, &theme), Some(steps[0]));
        assert_eq!(sc.color(100.0, &theme), Some(steps[steps.len() - 1]));
        // The least recedes toward the surface: nearer it in lightness than the most is.
        let surface = lightness(theme.surface);
        assert!((ls[0] - surface).abs() < (ls[20] - surface).abs());
    }
    assert_eq!(sc.color(f64::NAN, &Theme::light()), None);
    assert_eq!(sc.position(250.0), Some(1.0));
}

#[test]
fn a_diverging_scale_is_grey_at_its_midpoint_and_even_either_side() {
    let sc = ColorScale::Diverging {
        lo: -20.0,
        mid: 0.0,
        hi: 40.0,
    };
    // Equal distances from the midpoint take places as far either side of the middle.
    assert_eq!(sc.span(), (-40.0, 40.0));
    assert_eq!(sc.position(0.0), Some(0.5));
    assert_eq!(sc.position(-20.0), Some(0.25));
    assert_eq!(sc.position(20.0), Some(0.75));
    for theme in [Theme::light(), Theme::mono_dark()] {
        let mid = sc.color(0.0, &theme).unwrap();
        assert_eq!(mid, sc.steps(&theme)[3], "the neutral grey");
        let (r, g, b) = (i32::from(mid.0), i32::from(mid.1), i32::from(mid.2));
        assert!((r - g).abs() < 8 && (g - b).abs() < 8, "grey: {mid:?}");
        let neg = sc.color(-40.0, &theme).unwrap();
        let pos = sc.color(40.0, &theme).unwrap();
        assert!(neg.0 > neg.2, "red below: {neg:?}");
        assert!(pos.2 > pos.0, "blue above: {pos:?}");
        // The arms match in lightness.
        assert!((lightness(neg) - lightness(pos)).abs() < 0.02);
    }
}

#[test]
fn the_fixtures_take_their_scales() {
    match ramp::scale_of(&load("heatmap.json")) {
        Some(ColorScale::Sequential { lo, hi }) => assert!(lo < hi && lo > 0.0),
        other => panic!("sequential: {other:?}"),
    }
    match ramp::scale_of(&load("heatmap-diverging.json")) {
        Some(ColorScale::Diverging { lo, mid, hi }) => {
            assert_eq!(mid, 0.0);
            assert!(lo < 0.0 && hi > 0.0);
        }
        other => panic!("diverging: {other:?}"),
    }
    assert_eq!(
        ramp::field_name(&load("heatmap-diverging.json")),
        "Change, %"
    );
}

#[test]
fn the_legend_sits_under_the_title_clear_of_everything() {
    for (name, ticks) in [("heatmap.json", 2), ("heatmap-diverging.json", 3)] {
        let model = load(name);
        for theme in [Theme::mono_dark(), Theme::light()] {
            let (scene, l) = lay(&model, &View::default(), (880.0, 470.0), &theme);
            let r = l.ramp.as_ref().expect("a legend at 880 x 470");
            assert_eq!((r.bar.w, r.bar.h), (160.0, 8.0));
            assert_eq!(r.ticks.len(), ticks, "{name}");
            assert!(r.name.is_some());
            // Under the title, over the plot, inside the area.
            assert!(r.bounds.y >= l.title.bottom(), "{name}");
            assert!(r.bounds.bottom() <= l.plot.y, "{name}");
            assert!(r.bounds.right() <= 880.0 && r.bounds.x >= 0.0);
            // No text in the scene overlaps another.
            let t = texts(&scene);
            for (i, (ta, a)) in t.iter().enumerate() {
                for (tb, b) in &t[i + 1..] {
                    assert!(!overlaps(a, b), "{name}: {ta:?} over {tb:?}");
                }
            }
            // The ramp is drawn as a gradient over its bar.
            assert!(scene.items.iter().any(|i| matches!(
                i,
                Item::Path {
                    fill: Some(Paint::Linear { .. }),
                    ..
                }
            )));
        }
    }
}

#[test]
fn the_legend_goes_right_when_asked_and_away_when_there_is_no_room() {
    let text = std::fs::read_to_string(data_dir().join("heatmap.json")).unwrap();
    let right = text.replacen(
        "\"type\": \"heatmap\",",
        "\"type\": \"heatmap\", \"view\": {\"legend\": \"right\"},",
        1,
    );
    let model = model_of(&right, "right.json");
    assert_eq!(model.chart.view.legend, LegendPos::Right);
    let (_, l) = lay(
        &model,
        &View::default(),
        (880.0, 470.0),
        &Theme::mono_dark(),
    );
    let r = l.ramp.as_ref().expect("a legend at the right");
    assert!(r.bar.x >= l.plot.right(), "{:?} {:?}", r.bar, l.plot);
    assert!(r.bounds.right() <= 880.0);
    // A small heatmap gives its plot the room.
    let (_, small) = lay(
        &load("heatmap.json"),
        &View::default(),
        (200.0, 120.0),
        &Theme::mono_dark(),
    );
    assert!(small.ramp.is_none());
}

#[test]
fn the_colour_legend_outlasts_the_axes_titles_at_small_sizes() {
    // At 400 × 240 both heatmaps keep their ramp: its colours can't be read without it. The
    // axes' titles go first.
    for name in ["heatmap.json", "heatmap-diverging.json"] {
        for theme in [Theme::mono_dark(), Theme::light()] {
            let (scene, l) = lay(&load(name), &View::default(), (400.0, 240.0), &theme);
            assert!(l.ramp.is_some(), "{name}: a legend at 400 x 240");
            let t = texts(&scene);
            for (i, (ta, a)) in t.iter().enumerate() {
                for (tb, b) in &t[i + 1..] {
                    assert!(!overlaps(a, b), "{name}: {ta:?} over {tb:?}");
                }
            }
        }
    }
}

#[test]
fn a_swatch_that_barely_shows_on_the_card_gets_a_faint_ring() {
    // The diverging scale's cell nearest its midpoint, on dark: its key in the card is rung.
    let model = load("heatmap-diverging.json");
    let theme = Theme::mono_dark();
    let (_, l) = lay(&model, &View::default(), (880.0, 470.0), &theme);
    let sc = ramp::scale_of(&model).expect("a ramp");
    let s = &model.series[0];
    let sizes = s.sizes.as_ref().expect("cell values");
    let (_, i) = (0..sizes.len())
        .filter(|&i| sizes[i].is_finite())
        .map(|i| (sizes[i].abs(), i))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("a cell");
    let cell = (0..model.y.bands.len() as u32)
        .flat_map(|row| (0..model.x.bands.len() as u32).map(move |col| (row, col)))
        .find(|&(row, col)| model.cell_at(row, col) == Some((0, i)))
        .expect("its place");
    let st = state(
        Some(Cursor::Cell {
            row: cell.0,
            col: cell.1,
        }),
        None,
    );
    let over = plot::overlay(&model, &l, &View::default(), &st, &theme, &mut FixedMeasure);
    let swatch = sc.color(sizes[i], &theme).unwrap();
    let rung = over.items.iter().any(|it| matches!(
        it,
        Item::Path { fill: Some(Paint::Solid { color, .. }), stroke: Some(_), .. } if *color == swatch
    ));
    assert!(rung, "the near-midpoint swatch {swatch:?} is rung");
}

#[test]
fn cells_keep_a_two_pixel_surface_gap_on_whole_pixels() {
    let model = load("heatmap.json");
    for scale in [1.0, 2.0] {
        let (scene, l, _) = plot::base(
            &model,
            &View::default(),
            (880.0, 470.0),
            scale,
            &Theme::mono_dark(),
            &mut FixedMeasure,
        );
        let cs = cells(&scene, &l);
        assert_eq!(cs.len(), 24 * 7);
        let mut xs: Vec<f32> = cs.iter().map(|c| c.0.x).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        assert_eq!(xs.len(), 24);
        for c in &cs {
            for v in [c.0.x, c.0.right(), c.0.y, c.0.bottom()] {
                let px = v * scale;
                assert!((px - px.round()).abs() < 0.01, "{v} at {scale}x");
            }
        }
        // Neighbours across and down: 2 px apart, give or take one physical pixel's rounding.
        for a in &cs {
            if let Some(b) = cs
                .iter()
                .filter(|b| (b.0.y - a.0.y).abs() < 0.01 && b.0.x > a.0.x)
                .min_by(|p, q| p.0.x.total_cmp(&q.0.x))
            {
                let gap = b.0.x - a.0.right();
                assert!(
                    (gap - 2.0).abs() <= 1.0 / scale + 0.01,
                    "gap {gap} at {scale}x"
                );
            }
            if let Some(b) = cs
                .iter()
                .filter(|b| (b.0.x - a.0.x).abs() < 0.01 && b.0.y > a.0.y)
                .min_by(|p, q| p.0.y.total_cmp(&q.0.y))
            {
                let gap = b.0.y - a.0.bottom();
                assert!(
                    (gap - 2.0).abs() <= 1.0 / scale + 0.01,
                    "gap {gap} at {scale}x"
                );
            }
        }
    }
}

#[test]
fn a_missing_value_draws_a_hollow_cell() {
    let model = model_of(
        r#"{"type": "heatmap", "data": {"values": [
            {"x": "a", "y": "p", "v": 1}, {"x": "b", "y": "p", "v": null},
            {"x": "a", "y": "q", "v": 3}, {"x": "b", "y": "q", "v": 4}]},
          "x": "x", "y": "y", "color": "v"}"#,
        "gap.json",
    );
    let (scene, l) = lay(
        &model,
        &View::default(),
        (400.0, 300.0),
        &Theme::mono_dark(),
    );
    assert_eq!(cells(&scene, &l).len(), 3);
    let hollow = scene.items.iter().filter(|i| {
        matches!(i, Item::Path { fill: None, stroke: Some(Stroke { width, .. }), .. } if *width == 1.0)
    });
    assert!(hollow.count() >= 1);
}

#[test]
fn the_hovered_cell_lifts_with_a_ring_and_its_card_says_what_it_is() {
    let model = load("heatmap.json");
    let theme = Theme::mono_dark();
    let (_, l, hits) = plot::base(
        &model,
        &View::default(),
        (880.0, 470.0),
        1.0,
        &theme,
        &mut FixedMeasure,
    );
    // The cell for 10:00 on Thursday, by the pointer at its middle.
    let (col, row) = (10u32, 3u32);
    let (bx, bw) = l.x.band_of(col as usize).unwrap();
    let (by, bh) = l.y.band_of(row as usize).unwrap();
    let at = (bx + bw / 2.0, by + bh / 2.0);
    let hover = plot::hover(&model, &l, &hits, &View::default(), at.0, at.1);
    assert_eq!(hover, Some(Cursor::Cell { row, col }));
    let mut st = state(hover, None);
    st.pointer = Some(at);
    let over = plot::overlay(&model, &l, &View::default(), &st, &theme, &mut FixedMeasure);
    // A 2 px ring in the strong foreground, outside a 2 px band of the surface that parts it
    // from the cell: it reads on the hottest cell as on the coldest.
    let stroked = |c: Color| {
        over.items.iter().any(|i| matches!(
            i,
            Item::Path { fill: None, stroke: Some(Stroke { width, paint: Paint::Solid { color, .. }, .. }), .. }
                if *width == 2.0 && *color == c
        ))
    };
    assert!(stroked(theme.strong) && stroked(theme.surface));
    let t: Vec<String> = texts(&over).into_iter().map(|t| t.0).collect();
    assert!(t.iter().any(|s| s == "Hour 10 \u{b7} Thu"), "{t:?}");
    assert!(t.iter().any(|s| s == "requests"), "{t:?}");
    assert!(
        t.iter().any(|s| s == "errors"),
        "the row's other field: {t:?}"
    );
    // The card never covers its cell.
    let card = over.items.iter().find_map(|i| match i {
        Item::Shadow { rect, .. } => Some(*rect),
        _ => None,
    });
    let cell = Rect::new(bx, by, bw, bh);
    assert!(!overlaps(&card.expect("a card"), &cell));
}

#[test]
fn the_cursor_is_the_same_ring_with_its_categories_in_chips() {
    let model = load("heatmap.json");
    let theme = Theme::light();
    let (_, l) = lay(&model, &View::default(), (880.0, 470.0), &theme);
    let st = state(None, Some(Cursor::Cell { row: 4, col: 15 }));
    let over = plot::overlay(&model, &l, &View::default(), &st, &theme, &mut FixedMeasure);
    let ring = |i: &&Item| {
        matches!(
            i,
            Item::Path { fill: None, stroke: Some(Stroke { width, paint: Paint::Solid { color, .. }, .. }), .. }
                if *width == 2.0 && *color == theme.strong
        )
    };
    let rings = over.items.iter().filter(ring);
    assert_eq!(rings.count(), 1);
    let t: Vec<String> = texts(&over).into_iter().map(|t| t.0).collect();
    assert!(t.iter().any(|s| s == "Fri"), "the y chip: {t:?}");
    assert!(t.iter().any(|s| s == "15"), "the x chip: {t:?}");
    // With the pointer on another cell, the cursor keeps its ring: two now.
    let st = state(
        Some(Cursor::Cell { row: 1, col: 3 }),
        Some(Cursor::Cell { row: 4, col: 15 }),
    );
    let over = plot::overlay(&model, &l, &View::default(), &st, &theme, &mut FixedMeasure);
    let rings = over.items.iter().filter(ring);
    assert_eq!(rings.count(), 2);
}

#[test]
fn the_ring_reads_on_every_step_of_the_ramp_dark_and_light() {
    // Each step's cell, then the surface band, then the ring: the band parts the ring from
    // the cell at 3 to 1 or more however hot the cell, and the ring reads on the band.
    let lum = |c: Color| {
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
    };
    let contrast = |a: Color, b: Color| {
        let (a, b) = (lum(a), lum(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    };
    for theme in [Theme::mono_dark(), Theme::light()] {
        assert!(contrast(theme.strong, theme.surface) >= 3.0);
        for file in ["heatmap.json", "heatmap-diverging.json"] {
            let model = load(file);
            let sc = ramp::scale_of(&model).expect("a ramp");
            for &c in sc.steps(&theme) {
                // The ring on the cell where it reads, else the band round the cell.
                let best = contrast(c, theme.strong).max(contrast(c, theme.surface));
                assert!(best >= 3.0, "{file}: {c:?} on {}", theme.dark);
            }
        }
    }
}

#[test]
fn hjkl_move_by_cell_and_stop_at_the_edges() {
    let model = load("heatmap.json");
    let (_, l) = lay(
        &model,
        &View::default(),
        (880.0, 470.0),
        &Theme::mono_dark(),
    );
    let v = View::default();
    let go = |c: &Cursor, a: Action, n: u32| nav::motion(&model, &l, &v, Some(c), &a, n);
    let start = nav::motion(&model, &l, &v, None, &Action::Step(1), 1).expect("placed");
    assert_eq!(start, Cursor::Cell { row: 0, col: 0 });
    let c = Cursor::Cell { row: 3, col: 10 };
    assert_eq!(
        go(&c, Action::Step(1), 1),
        Some(Cursor::Cell { row: 3, col: 11 })
    );
    assert_eq!(
        go(&c, Action::Step(-1), 3),
        Some(Cursor::Cell { row: 3, col: 7 })
    );
    assert_eq!(
        go(&c, Action::Series(1), 1),
        Some(Cursor::Cell { row: 4, col: 10 })
    );
    assert_eq!(
        go(&c, Action::Series(-1), 2),
        Some(Cursor::Cell { row: 1, col: 10 })
    );
    // Past an edge it stops there; at the edge it doesn't move.
    assert_eq!(
        go(&c, Action::Step(1), 99),
        Some(Cursor::Cell { row: 3, col: 23 })
    );
    assert_eq!(
        go(&Cursor::Cell { row: 6, col: 0 }, Action::Series(1), 1),
        None
    );
    assert_eq!(
        go(&c, Action::Last, 1),
        Some(Cursor::Cell { row: 3, col: 23 })
    );
    assert_eq!(
        go(&c, Action::First, 1),
        Some(Cursor::Cell { row: 3, col: 0 })
    );
}

#[test]
fn the_box_brush_snaps_to_whole_cells_and_zooms_to_them() {
    let model = load("heatmap.json");
    let v = View::default();
    let (_, l) = lay(&model, &v, (880.0, 470.0), &Theme::mono_dark());
    assert_eq!(plot::brush_kind(&model), BrushKind::Box);
    // From inside the cell for (Tue, 7) to inside (Sat, 15): columns 7..16, rows 1..6.
    let mid = |col: usize, row: usize, fx: f32, fy: f32| {
        let (bx, bw) = l.x.band_of(col).unwrap();
        let (by, bh) = l.y.band_of(row).unwrap();
        (bx + bw * fx, by + bh * fy)
    };
    let (a, b) = (mid(7, 1, 0.3, 0.2), mid(15, 5, 0.6, 0.9));
    assert_eq!(
        nav::brush_cells(&model, &l, &v, a, b),
        Some(((7, 16), (1, 6)))
    );
    let z = nav::brush(&model, &l, &v, BrushKind::Box, a, b).expect("a zoom");
    assert_eq!(z.x, Some((7.0, 16.0)));
    assert_eq!(z.y, Some((1.0, 6.0)));
    // The zoomed view shows exactly those cells, whole.
    let (scene, zl) = lay(&model, &z, (880.0, 470.0), &Theme::mono_dark());
    assert_eq!(cells(&scene, &zl).len(), 9 * 5);
    // The preview's box lies on the cells' slots.
    let mut st = state(None, None);
    st.brush = Some((BrushKind::Box, a, b));
    let over = plot::overlay(&model, &l, &v, &st, &Theme::mono_dark(), &mut FixedMeasure);
    let wash = over.items.iter().find_map(|i| match i {
        Item::Path {
            path,
            fill: Some(Paint::Solid { .. }),
            stroke: None,
        } => path_bounds(path),
        _ => None,
    });
    let w = wash.expect("the brush's wash");
    assert!((w.x - l.x.map(7.0)).abs() < 0.01 && (w.right() - l.x.map(16.0)).abs() < 0.01);
    assert!((w.y - l.y.map(1.0)).abs() < 0.01 && (w.bottom() - l.y.map(6.0)).abs() < 0.01);
    let t: Vec<String> = texts(&over).into_iter().map(|t| t.0).collect();
    assert!(
        t.iter()
            .any(|s| s == "7 \u{2013} 15 \u{b7} Tue \u{2013} Sat"),
        "{t:?}"
    );
    // A brush flat in y keeps the rows that show; one too small both ways is no brush.
    let flat = nav::brush_cells(&model, &l, &v, mid(2, 2, 0.5, 0.5), mid(5, 2, 0.5, 0.52));
    assert_eq!(flat, Some(((2, 6), (0, 7))));
    let tiny = nav::brush_cells(&model, &l, &v, a, (a.0 + 3.0, a.1 + 3.0));
    assert_eq!(tiny, None);
}

#[test]
fn every_heatmap_lays_out_at_three_sizes_dark_and_light() {
    for name in ["heatmap.json", "heatmap-diverging.json"] {
        let model = load(name);
        for &size in SIZES {
            for theme in [Theme::mono_dark(), Theme::light()] {
                let (scene, l) = lay(&model, &View::default(), size, &theme);
                let t = texts(&scene);
                for (i, (ta, a)) in t.iter().enumerate() {
                    assert!(
                        a.x >= -0.5 && a.right() <= size.0 + 0.5 && a.bottom() <= size.1 + 0.5,
                        "{name} {size:?}: {ta:?} at {a:?}"
                    );
                    for (tb, b) in &t[i + 1..] {
                        assert!(!overlaps(a, b), "{name} {size:?}: {ta:?} over {tb:?}");
                    }
                }
                // Tick labels clear the cells.
                for lb in l
                    .labels
                    .iter()
                    .filter(|lb| matches!(lb.role, Role::XTick | Role::YTick))
                {
                    let b = lb.bounds(&mut FixedMeasure);
                    assert!(!overlaps(&b, &l.plot), "{name} {size:?}: {:?}", lb.text);
                }
                assert!(!cells(&scene, &l).is_empty());
            }
        }
    }
}
