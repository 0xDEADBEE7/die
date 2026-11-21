//! The raster, the SVG writer and the theme: pixels at known points, damage, opacity,
//! gradients, hairlines at scale 1 and 2, shadows, the SVG's structure, and the theme resolved
//! from a harness's faces.

use die_test_cap as _;

use die_core::{Harness, Rgb, Style};
use die_fig::raster::{self, BookMeasure, PixelBox};
use die_fig::scene::{
    Anchor, Cap, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Weight,
};
use die_fig::style::{ColorSpec, Status};
use die_fig::svg;
use die_fig::theme::{self, Color, Theme};

const RED: Color = Color(255, 0, 0);
const BLUE: Color = Color(0, 0, 255);
const WHITE: Color = Color(255, 255, 255);
const GREEN: Color = Color(0, 255, 0);

/// A picture's pixels: premultiplied RGBA8, `w × 4` bytes a row.
struct Shot {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Shot {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(
            x < self.w && y < self.h,
            "({x}, {y}) is outside {}×{}",
            self.w,
            self.h
        );
        let i = (y * self.w + x) as usize * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    fn alpha(&self, x: u32, y: u32) -> u8 {
        self.at(x, y)[3]
    }
}

/// The scene drawn whole on transparent, at its scale.
fn shoot(scene: &Scene) -> Shot {
    shoot_on(scene, None)
}

fn shoot_on(scene: &Scene, clear: Option<Color>) -> Shot {
    let w = (scene.width * scene.scale).ceil() as u32;
    let h = (scene.height * scene.scale).ceil() as u32;
    let mut px = vec![0xee; w as usize * h as usize * 4];
    raster::draw(scene, &mut px, w, h, w as usize * 4, None, clear, None);
    Shot { w, h, px }
}

fn rect(r: Rect, color: Color, alpha: f32) -> Item {
    Item::Path {
        path: PathData::rect(r),
        fill: Some(Paint::solid(color, alpha)),
        stroke: None,
    }
}

fn line(pts: &[(f32, f32)], stroke: Stroke) -> Item {
    Item::Path {
        path: PathData::polyline(pts),
        fill: None,
        stroke: Some(stroke),
    }
}

fn near(a: u8, b: u8, by: u8) -> bool {
    a.abs_diff(b) <= by
}

/// What `f` gives, on a thread of its own, failing if it takes more than `secs`: the raster
/// runs on the editor's main thread, so one that never finishes freezes it.
fn finishes<T: Send + 'static>(what: &str, secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(secs)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{what} didn't finish in {secs} s")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("{what} panicked"),
    }
}

/// A row's runs from `x0` to `x1`, left to right: whether each is inked, and how long it is.
/// Fails on a pixel that is neither full nor empty.
fn runs(shot: &Shot, y: u32, x0: u32, x1: u32) -> Vec<(bool, u32)> {
    let mut out: Vec<(bool, u32)> = Vec::new();
    for x in x0..x1 {
        let a = shot.alpha(x, y);
        assert!(
            a == 0 || a == 255,
            "({x}, {y}) is {a}, neither full nor empty"
        );
        match out.last_mut() {
            Some((on, n)) if *on == (a == 255) => *n += 1,
            _ => out.push((a == 255, 1)),
        }
    }
    out
}

/// The same for column `x`, from `y0` to `y1`.
fn col_runs(shot: &Shot, x: u32, y0: u32, y1: u32) -> Vec<(bool, u32)> {
    let t = Shot {
        w: shot.h,
        h: shot.w,
        px: (0..shot.w)
            .flat_map(|xx| (0..shot.h).flat_map(move |yy| shot.at(xx, yy)))
            .collect(),
    };
    runs(&t, x, y0, y1)
}

// ------------------------------------------------------------------ paths

#[test]
fn a_filled_rect_is_its_colour_inside_and_nothing_outside() {
    for scale in [1.0f32, 2.0] {
        let mut s = Scene::new(20.0, 20.0, scale);
        s.push(rect(Rect::new(5.0, 5.0, 10.0, 10.0), RED, 1.0));
        let shot = shoot(&s);
        let p = |v: f32| (v * scale) as u32;
        assert_eq!(
            shot.at(p(10.0), p(10.0)),
            [255, 0, 0, 255],
            "inside at {scale}"
        );
        assert_eq!(shot.at(p(5.0), p(5.0)), [255, 0, 0, 255], "its first pixel");
        assert_eq!(
            shot.at(p(15.0) - 1, p(15.0) - 1),
            [255, 0, 0, 255],
            "its last"
        );
        assert_eq!(
            shot.at(p(5.0) - 1, p(10.0)),
            [0; 4],
            "left of it at {scale}"
        );
        assert_eq!(shot.at(p(15.0), p(10.0)), [0; 4], "right of it");
        assert_eq!(shot.at(p(10.0), p(15.0)), [0; 4], "under it");
        assert_eq!(shot.at(0, 0), [0; 4], "a corner of the picture");
    }
}

#[test]
fn colours_are_premultiplied_by_their_alpha() {
    let mut s = Scene::new(10.0, 10.0, 1.0);
    s.push(rect(Rect::new(0.0, 0.0, 10.0, 10.0), BLUE, 0.5));
    let [r, g, b, a] = shoot(&s).at(5, 5);
    assert!(
        r == 0 && g == 0 && near(b, 128, 1) && near(a, 128, 1),
        "{r} {g} {b} {a}"
    );
    let [r, g, b, a] = shoot_on(&s, Some(WHITE)).at(5, 5);
    assert!(
        near(r, 127, 1) && near(g, 127, 1) && b == 255 && a == 255,
        "{r} {g} {b} {a}"
    );
}

#[test]
fn rounded_rects_and_circles_leave_their_corners() {
    let mut s = Scene::new(40.0, 20.0, 1.0);
    let r = Rect::new(0.0, 0.0, 20.0, 20.0);
    s.push(Item::Path {
        path: PathData::rounded(r, [8.0, 0.0, 8.0, 8.0]),
        fill: Some(Paint::solid(RED, 1.0)),
        stroke: None,
    });
    s.push(Item::Path {
        path: PathData::circle(30.0, 10.0, 5.0),
        fill: Some(Paint::solid(BLUE, 1.0)),
        stroke: None,
    });
    let shot = shoot(&s);
    assert_eq!(shot.alpha(0, 0), 0, "a rounded corner");
    assert_eq!(shot.alpha(19, 19), 0, "another");
    assert_eq!(shot.alpha(0, 19), 0, "and another");
    assert_eq!(
        shot.at(19, 0),
        [255, 0, 0, 255],
        "a square corner keeps its pixel"
    );
    assert_eq!(shot.at(10, 10), [255, 0, 0, 255]);
    assert_eq!(shot.at(0, 10), [255, 0, 0, 255], "the middle of an edge");
    assert_eq!(shot.at(30, 10), [0, 0, 255, 255], "the circle's middle");
    assert_eq!(shot.alpha(25, 5), 0, "the circle's corner of its box");
    assert_eq!(shot.alpha(36, 10), 0, "past its right");
}

/// The scales the raster is checked at: whole, a fractional one, and Retina's.
const SCALES: [f32; 3] = [1.0, 1.5, 2.0];

#[test]
fn a_gradient_runs_from_its_first_stop_to_its_last() {
    for scale in SCALES {
        let mut s = Scene::new(100.0, 20.0, scale);
        // The stops out of order: they're sorted.
        let colours = Paint::Linear {
            from: (0.0, 0.0),
            to: (100.0, 0.0),
            stops: vec![(1.0, BLUE, 1.0), (0.0, RED, 1.0)],
        };
        s.push(Item::Path {
            path: PathData::rect(Rect::new(0.0, 0.0, 100.0, 10.0)),
            fill: Some(colours),
            stroke: None,
        });
        let fade = Paint::Linear {
            from: (0.0, 10.0),
            to: (100.0, 10.0),
            stops: vec![(0.0, GREEN, 0.0), (1.0, GREEN, 1.0)],
        };
        s.push(Item::Path {
            path: PathData::rect(Rect::new(0.0, 10.0, 100.0, 10.0)),
            fill: Some(fade),
            stroke: None,
        });
        let shot = shoot(&s);
        let p = |v: f32| (v * scale) as u32;
        let last = shot.w - 1;
        let [r, _, b, a] = shot.at(0, p(5.0));
        assert!(
            r >= 250 && b <= 5 && a == 255,
            "red at the start at {scale}: {r} {b} {a}"
        );
        let [r, _, b, a] = shot.at(last, p(5.0));
        assert!(
            r <= 5 && b >= 250 && a == 255,
            "blue at the end at {scale}: {r} {b} {a}"
        );
        let [r, _, b, _] = shot.at(p(50.0), p(5.0));
        assert!(
            near(r, 128, 6) && near(b, 128, 6),
            "halfway at {scale}: {r} {b}"
        );
        assert!(shot.alpha(0, p(15.0)) <= 3, "transparent at the start");
        assert!(shot.alpha(last, p(15.0)) >= 252, "opaque at the end");
        assert!(near(shot.alpha(p(50.0), p(15.0)), 128, 6), "half at half");
    }
}

#[test]
fn caps_end_where_they_should() {
    for scale in SCALES {
        let row = |cap: Cap| {
            let mut s = Scene::new(40.0, 20.0, scale);
            let st = Stroke {
                cap,
                ..Stroke::solid(4.0, WHITE, 1.0)
            };
            s.push(line(&[(10.0, 10.0), (30.0, 10.0)], st));
            shoot(&s)
        };
        let p = |v: f32| (v * scale) as u32;
        let butt = row(Cap::Butt);
        let (y, at) = (p(10.0), format!("at {scale}"));
        assert_eq!(butt.alpha(p(10.0), y), 255, "its first pixel {at}");
        assert_eq!(butt.alpha(p(10.0) - 1, y), 0, "nothing before it {at}");
        assert_eq!(butt.alpha(p(30.0) - 1, y), 255, "its last {at}");
        assert_eq!(butt.alpha(p(30.0), y), 0, "nothing past it {at}");
        // Four px across: from 8 to 12.
        let mid = p(20.0);
        assert_eq!(butt.alpha(mid, p(8.0)), 255, "its top row {at}");
        assert_eq!(butt.alpha(mid, p(12.0) - 1), 255, "its bottom row {at}");
        assert_eq!(butt.alpha(mid, p(8.0) - 1), 0, "above it {at}");
        assert_eq!(butt.alpha(mid, p(12.0)), 0, "below it {at}");
        let square = row(Cap::Square);
        assert_eq!(
            square.alpha(p(8.0), y),
            255,
            "half the width past the start {at}"
        );
        assert_eq!(square.alpha(p(8.0) - 1, y), 0, "{at}");
        assert_eq!(square.alpha(p(32.0) - 1, y), 255, "and past the end {at}");
        assert_eq!(square.alpha(p(32.0), y), 0, "{at}");
        let round = row(Cap::Round);
        assert!(round.alpha(p(9.0), y) > 0, "round past the start {at}");
        assert_eq!(round.alpha(p(8.0) - 1, y), 0, "{at}");
        assert_eq!(
            square.alpha(p(8.0), p(8.0)),
            255,
            "a square cap's corner {at}"
        );
        assert!(
            round.alpha(p(8.0), p(8.0)) < 128,
            "a round one's is mostly empty {at}"
        );
    }
}

#[test]
fn dashes_are_on_then_off() {
    for scale in SCALES {
        let mut s = Scene::new(40.0, 10.0, scale);
        let st = Stroke {
            dash: Some((6.0, 4.0)),
            ..Stroke::solid(2.0, WHITE, 1.0)
        };
        s.push(line(&[(0.0, 5.0), (40.0, 5.0)], st));
        let shot = shoot(&s);
        // 6 on and 4 off a logical px, so 12 and 8 physical at 2; each edge on a whole pixel
        // at these scales, so nothing between.
        let (on, off) = ((6.0 * scale) as u32, (4.0 * scale) as u32);
        assert_eq!(
            runs(&shot, (5.0 * scale) as u32, 0, shot.w),
            [(true, on), (false, off)].repeat(4),
            "at {scale}"
        );
    }
}

#[test]
fn a_mitre_fills_the_corner_a_bevel_cuts() {
    let corner = |join: Join| {
        let mut s = Scene::new(40.0, 40.0, 1.0);
        let st = Stroke {
            join,
            ..Stroke::solid(6.0, WHITE, 1.0)
        };
        s.push(line(&[(10.0, 30.0), (10.0, 10.0), (30.0, 10.0)], st));
        shoot(&s).alpha(7, 7)
    };
    assert_eq!(corner(Join::Miter), 255);
    assert!(corner(Join::Bevel) < 40, "only antialiasing's spill");
    assert!(corner(Join::Round) < 40);
}

// ------------------------------------------------------------------ hairlines

/// The rows of column `x` that have any ink, and whether each is full.
fn inked_rows(shot: &Shot, x: u32) -> Vec<(u32, u8)> {
    (0..shot.h)
        .map(|y| (y, shot.alpha(x, y)))
        .filter(|&(_, a)| a > 0)
        .collect()
}

fn inked_cols(shot: &Shot, y: u32) -> Vec<(u32, u8)> {
    (0..shot.w)
        .map(|x| (x, shot.alpha(x, y)))
        .filter(|&(_, a)| a > 0)
        .collect()
}

#[test]
fn hairlines_are_whole_pixels_at_scale_1_and_2() {
    // (scale, its width in px, the rows a line at y = 10 covers, those at y = 10.3)
    let cases: [(f32, &[u32], &[u32]); 3] = [
        (1.0, &[10], &[10]),
        (1.5, &[15], &[15]),
        (2.0, &[19, 20], &[20, 21]),
    ];
    for (scale, at_10, at_10_3) in cases {
        let mut s = Scene::new(40.0, 40.0, scale);
        s.push(line(
            &[(5.0, 10.0), (35.0, 10.0)],
            Stroke::hairline(WHITE, 1.0),
        ));
        s.push(line(
            &[(5.0, 30.3), (35.0, 30.3)],
            Stroke::hairline(WHITE, 1.0),
        ));
        s.push(line(
            &[(20.0, 2.0), (20.0, 38.0)],
            Stroke::hairline(WHITE, 1.0),
        ));
        let shot = shoot(&s);
        let p = |v: f32| (v * scale) as u32;
        let rows = inked_rows(&shot, p(12.0));
        let full: Vec<u32> = at_10
            .iter()
            .copied()
            .chain(at_10_3.iter().map(|r| r + p(20.0)))
            .collect();
        assert_eq!(
            rows,
            full.iter().map(|&r| (r, 255)).collect::<Vec<_>>(),
            "the level lines' rows at scale {scale}, each full"
        );
        // Butt ends at whole pixels, each rounded: from x = 5 to 35.
        let edge = |v: f32| (v * scale + 0.5).floor() as u32;
        let cols = inked_cols(&shot, at_10[0]);
        assert_eq!(
            cols.first(),
            Some(&(edge(5.0), 255)),
            "its start at {scale}"
        );
        assert_eq!(
            cols.last(),
            Some(&(edge(35.0) - 1, 255)),
            "its end at {scale}"
        );
        assert!(cols.iter().all(|&(_, a)| a == 255), "every pixel full");
        // The plumb line: as wide as a level one, at x = 20.
        let cols = inked_cols(&shot, p(25.0));
        let want: Vec<(u32, u8)> = at_10.iter().map(|r| (r + p(20.0) - p(10.0), 255)).collect();
        assert_eq!(cols, want, "the plumb line's columns at {scale}");
    }
}

#[test]
fn a_hairline_box_has_its_corners() {
    for scale in [1.0f32, 2.0] {
        let mut s = Scene::new(40.0, 30.0, scale);
        s.push(Item::Path {
            path: PathData::rect(Rect::new(5.0, 5.0, 20.0, 10.0)),
            fill: None,
            stroke: Some(Stroke::hairline(WHITE, 1.0)),
        });
        let shot = shoot(&s);
        let w = scale as u32;
        let p = |v: f32| (v * scale) as u32;
        // A line's pixels: at 1 the one after its edge, at 2 one each side of it. The outer
        // corner is the first of the top and left edges and the last of the bottom and right.
        let first = |v: f32| if w == 1 { p(v) } else { p(v) - 1 };
        let last = |v: f32| p(v);
        for (x, y) in [
            (first(5.0), first(5.0)),
            (last(25.0), first(5.0)),
            (last(25.0), last(15.0)),
            (first(5.0), last(15.0)),
        ] {
            assert_eq!(shot.alpha(x, y), 255, "a corner ({x}, {y}) at {scale}");
        }
        assert_eq!(shot.alpha(p(15.0), p(10.0)), 0, "inside is empty");
        let rows = inked_rows(&shot, p(15.0));
        assert_eq!(
            rows.len(),
            2 * w as usize,
            "two edges, {w} px each, at {scale}"
        );
        assert!(rows.iter().all(|&(_, a)| a == 255), "crisp: {rows:?}");
    }
}

#[test]
fn a_dashed_hairline_is_dashes_of_whole_pixels() {
    let mut s = Scene::new(40.0, 20.0, 1.0);
    let st = Stroke {
        dash: Some((4.0, 4.0)),
        ..Stroke::hairline(WHITE, 1.0)
    };
    s.push(line(&[(0.0, 10.0), (40.0, 10.0)], st));
    let shot = shoot(&s);
    for x in 0..40 {
        let on = (x / 4) % 2 == 0;
        assert_eq!(shot.alpha(x, 10), if on { 255 } else { 0 }, "x {x}");
    }
    assert!(inked_cols(&shot, 9).is_empty() && inked_cols(&shot, 11).is_empty());
}

#[test]
fn a_dashed_hairline_finishes_at_any_scale_in_dashes_of_whole_pixels() {
    // Scales and dashes whose physical lengths f32 can't hold exactly, and some it can.
    let cases: [(f32, (f32, f32)); 9] = [
        (1.1, (6.0, 4.0)),
        (1.2, (6.0, 4.0)),
        (4.0 / 3.0, (6.0, 4.0)),
        (1.25, (6.0, 4.0)),
        (1.5, (6.0, 4.0)),
        (2.0, (1.2, 0.8)),
        (2.0, (3.0, 1.2)),
        (2.0, (6.0, 4.0)),
        (1.0, (2.4, 1.6)),
    ];
    for (scale, (on, off)) in cases {
        let shot = finishes(
            &format!("a dashed hairline at {scale} with ({on}, {off})"),
            10,
            move || {
                let mut s = Scene::new(1000.0, 20.0, scale);
                let st = Stroke {
                    dash: Some((on, off)),
                    ..Stroke::hairline(WHITE, 1.0)
                };
                s.push(line(&[(0.0, 10.0), (1000.0, 10.0)], st));
                shoot(&s)
            },
        );
        let y = (10.0 * scale).floor() as u32;
        let rs = runs(&shot, y, 0, shot.w);
        let between =
            |n: u32, v: f32| n >= (v * scale).floor() as u32 && n <= (v * scale).ceil() as u32;
        // The first and last runs are cut by the line's ends and the picture's.
        for &(lit, n) in &rs[1..rs.len() - 1] {
            let want = if lit { on } else { off };
            assert!(
                between(n, want),
                "a {} run of {n} px at {scale} with ({on}, {off}): {rs:?}",
                if lit { "lit" } else { "dark" }
            );
        }
        let dashes = rs.iter().filter(|r| r.0).count() as f32;
        let want = 1000.0 / (on + off);
        assert!(
            (dashes - want).abs() <= 1.0,
            "{dashes} dashes over 1000 px at {scale}, about {want}"
        );
    }
}

#[test]
fn a_dashed_hairline_far_past_the_picture_dashes_only_what_shows() {
    let dashed = Stroke {
        dash: Some((6.0, 4.0)),
        ..Stroke::hairline(WHITE, 1.0)
    };
    let solid = Stroke::hairline(WHITE, 1.0);
    // At scale 2 a 6-on-4-off dash is 12 px lit and 8 dark wherever it starts, since every
    // end is at a whole number of periods from 0.
    for (from, to, clipped) in [
        (-1e8f32, 1e8f32, true),
        (-1e8, 1e8, false),
        (0.0, f32::INFINITY, false),
        (-1e8, 50.0, true),
        (1e8, -1e8, false),
    ] {
        let st = dashed.clone();
        let shot = finishes(
            &format!("a dashed hairline from {from} to {to}"),
            5,
            move || {
                let mut s = Scene::new(100.0, 100.0, 2.0);
                if clipped {
                    s.push(Item::PushClip(Rect::new(0.0, 0.0, 100.0, 100.0)));
                }
                s.push(line(&[(from, 10.0), (to, 10.0)], st.clone()));
                s.push(line(&[(50.0, from), (50.0, to)], st));
                if clipped {
                    s.push(Item::PopClip);
                }
                shoot(&s)
            },
        );
        let end = (from.max(to).clamp(0.0, 100.0) * 2.0) as u32;
        let across = runs(&shot, 20, 0, end.min(90));
        let down = col_runs(&shot, 100, 30, end.min(190));
        for (what, rs) in [("across", &across), ("down", &down)] {
            assert!(rs.len() > 4, "{what} from {from} to {to}: {rs:?}");
            for &(lit, n) in &rs[1..rs.len() - 1] {
                assert_eq!(
                    n,
                    if lit { 12 } else { 8 },
                    "{what} from {from} to {to}: {rs:?}"
                );
            }
        }
        // Rightward, a dash starts at 0; leftward from 2e8, one ends there.
        let first = if from < to { (true, 12) } else { (false, 8) };
        assert_eq!(across[0], first, "from {from} to {to}: {across:?}");
    }
    // Plain hairlines to or from infinity show what's on screen; a dashed one from it has no
    // place to start its pattern, so nothing.
    let shot = finishes("a hairline to infinity", 5, move || {
        let mut s = Scene::new(100.0, 50.0, 2.0);
        s.push(line(&[(10.0, 10.0), (f32::INFINITY, 10.0)], solid.clone()));
        s.push(line(&[(f32::NEG_INFINITY, 20.0), (50.0, 20.0)], solid));
        s.push(line(&[(f32::NEG_INFINITY, 30.0), (50.0, 30.0)], dashed));
        shoot(&s)
    });
    assert_eq!(runs(&shot, 20, 0, 200), vec![(false, 20), (true, 180)]);
    assert_eq!(runs(&shot, 40, 0, 200), vec![(true, 100), (false, 100)]);
    assert_eq!(runs(&shot, 60, 0, 200), vec![(false, 200)]);
}

#[test]
fn a_dash_finer_than_a_pixel_is_drawn_solid() {
    let shot = finishes("a dash of a millionth of a px", 5, || {
        let mut s = Scene::new(100.0, 20.0, 1.0);
        let st = Stroke {
            dash: Some((1e-6, 1e-6)),
            ..Stroke::hairline(WHITE, 1.0)
        };
        s.push(line(&[(10.0, 10.0), (90.0, 10.0)], st));
        shoot(&s)
    });
    assert_eq!(
        runs(&shot, 10, 0, 100),
        vec![(false, 10), (true, 80), (false, 10)]
    );
}

#[test]
fn a_slanted_hairline_is_stroked() {
    let mut s = Scene::new(20.0, 20.0, 2.0);
    s.push(line(
        &[(0.0, 0.0), (20.0, 20.0)],
        Stroke::hairline(WHITE, 1.0),
    ));
    let shot = shoot(&s);
    assert!(shot.alpha(20, 20) > 100, "on the diagonal");
    assert_eq!(shot.alpha(30, 10), 0, "off it");
}

// ------------------------------------------------------------------ clips and opacity

#[test]
fn clips_cut_nest_and_pop() {
    for scale in SCALES {
        let mut s = Scene::new(30.0, 30.0, scale);
        s.push(Item::PushClip(Rect::new(5.0, 5.0, 10.0, 10.0)));
        s.push(rect(Rect::new(0.0, 0.0, 30.0, 30.0), RED, 1.0));
        s.push(Item::PushClip(Rect::new(10.0, 0.0, 30.0, 30.0)));
        s.push(rect(Rect::new(0.0, 0.0, 30.0, 30.0), BLUE, 1.0));
        s.push(Item::PopClip);
        s.push(Item::PopClip);
        s.push(rect(Rect::new(20.0, 20.0, 5.0, 5.0), GREEN, 1.0));
        let shot = shoot(&s);
        // A clip's edges are at their nearest physical pixels.
        let n = |v: f32| (v * scale).round() as u32;
        let p = |v: f32| (v * scale) as u32;
        let at = format!("at {scale}");
        assert_eq!(shot.alpha(n(5.0) - 1, n(10.0)), 0, "left of the clip {at}");
        assert_eq!(shot.alpha(n(10.0), n(5.0) - 1), 0, "above it {at}");
        assert_eq!(shot.at(n(5.0), n(5.0)), [255, 0, 0, 255], "inside it {at}");
        assert_eq!(shot.at(n(10.0) - 1, n(15.0) - 1), [255, 0, 0, 255], "{at}");
        assert_eq!(shot.alpha(n(15.0), n(15.0)), 0, "past it {at}");
        assert_eq!(shot.alpha(n(10.0), n(15.0)), 0, "under it {at}");
        assert_eq!(
            shot.at(n(10.0), n(5.0)),
            [0, 0, 255, 255],
            "in both clips {at}"
        );
        assert_eq!(shot.at(n(15.0) - 1, n(15.0) - 1), [0, 0, 255, 255], "{at}");
        assert_eq!(
            shot.alpha(n(15.0), n(10.0)),
            0,
            "the inner clip is within the outer {at}"
        );
        assert_eq!(
            shot.at(p(22.0), p(22.0)),
            [0, 255, 0, 255],
            "popped, nothing clips {at}"
        );
    }
}

#[test]
fn an_opacity_group_fades_once_and_nested_ones_multiply() {
    for scale in SCALES {
        let mut s = Scene::new(40.0, 30.0, scale);
        s.push(Item::PushOpacity(0.5));
        s.push(rect(Rect::new(0.0, 0.0, 10.0, 10.0), RED, 1.0));
        s.push(rect(Rect::new(5.0, 0.0, 10.0, 10.0), RED, 1.0));
        s.push(Item::PopOpacity);
        s.push(Item::PushOpacity(0.5));
        s.push(Item::PushOpacity(0.5));
        s.push(rect(Rect::new(20.0, 0.0, 10.0, 10.0), RED, 1.0));
        s.push(Item::PopOpacity);
        s.push(Item::PopOpacity);
        s.push(Item::PushOpacity(0.0));
        s.push(rect(Rect::new(0.0, 15.0, 10.0, 10.0), RED, 1.0));
        s.push(Item::PopOpacity);
        s.push(Item::PopOpacity); // one too many: nothing
        s.push(Item::PushOpacity(0.5)); // never popped: laid down at the end
        s.push(rect(Rect::new(20.0, 15.0, 10.0, 10.0), BLUE, 1.0));
        let shot = shoot(&s);
        let p = |v: f32| (v * scale) as u32;
        let at = |x: f32, y: f32| shot.at(p(x), p(y));
        let half = at(2.0, 5.0);
        assert!(
            near(half[0], 128, 1) && near(half[3], 128, 1),
            "{half:?} at {scale}"
        );
        assert_eq!(
            at(7.0, 5.0),
            half,
            "where the two cross, no darker at {scale}"
        );
        assert_eq!(at(12.0, 5.0), half);
        assert_eq!(at(0.0, 0.0), half, "to the group's first pixel");
        let past = (15.0 * scale).ceil() as u32;
        assert_eq!(shot.alpha(past, p(5.0)), 0, "and nothing past its last");
        let quarter = at(25.0, 5.0);
        assert!(near(quarter[3], 64, 2), "half of a half: {quarter:?}");
        assert_eq!(at(5.0, 20.0)[3], 0, "at no opacity, nothing");
        assert!(
            near(at(25.0, 20.0)[3], 128, 1),
            "an open group is laid down at the end"
        );
    }
}

// ------------------------------------------------------------------ shadows

#[test]
fn a_shadow_is_dark_under_its_card_and_fades_by_twice_its_blur() {
    for scale in [1.0f32, 2.0] {
        let mut s = Scene::new(100.0, 90.0, scale);
        s.push(Item::Shadow {
            rect: Rect::new(20.0, 20.0, 40.0, 20.0),
            radius: 4.0,
            blur: 8.0,
            dy: 2.0,
            alpha: 0.5,
        });
        let shot = shoot(&s);
        let at = |x: f32, y: f32| shot.at((x * scale) as u32, (y * scale) as u32);
        let [r, g, b, a] = at(40.0, 32.0);
        assert!((r, g, b) == (0, 0, 0), "black");
        assert!(
            (118..=128).contains(&a),
            "about half under the card at {scale}: {a}"
        );
        let below = at(40.0, 45.0)[3];
        let above = at(40.0, 17.0)[3];
        assert!(below > 0 && below < a, "soft below: {below}");
        assert!(below > above, "it falls down: {below} over {above}");
        assert_eq!(
            at(40.0, 42.0 + 16.5)[3],
            0,
            "nothing twice the blur past it"
        );
        assert_eq!(at(3.0, 32.0)[3], 0);
    }
}

#[test]
fn a_sharp_shadow_is_its_rounded_rect() {
    let mut s = Scene::new(60.0, 60.0, 1.0);
    s.push(Item::Shadow {
        rect: Rect::new(10.0, 10.0, 30.0, 30.0),
        radius: 6.0,
        blur: 0.0,
        dy: 0.0,
        alpha: 1.0,
    });
    s.push(Item::PushClip(Rect::new(0.0, 0.0, 25.0, 60.0)));
    s.push(Item::Shadow {
        rect: Rect::new(10.0, 45.0, 30.0, 10.0),
        radius: 0.0,
        blur: 4.0,
        dy: 0.0,
        alpha: 1.0,
    });
    s.push(Item::PopClip);
    let shot = shoot(&s);
    assert_eq!(shot.at(20, 20), [0, 0, 0, 255]);
    assert_eq!(shot.alpha(10, 10), 0, "its rounded corner");
    assert_eq!(shot.alpha(10, 25), 255, "its edge");
    assert!(shot.alpha(20, 50) > 200, "the clipped one, inside the clip");
    assert_eq!(shot.alpha(30, 50), 0, "and nothing past the clip");
}

#[test]
fn a_shadow_past_any_size_never_overflows_and_a_huge_blur_is_held_to_the_most() {
    let card = Rect::new(20.0, 20.0, 40.0, 20.0);
    let cases = [
        (card, 1e10f32, 0.0f32),
        (card, f32::MAX, 0.0),
        (card, f32::INFINITY, 0.0),
        (card, 1e3, 0.0),
        (Rect::new(-1e10, 20.0, 1e10, 20.0), 8.0, 0.0),
        (Rect::new(1e10, 20.0, 40.0, 20.0), 8.0, 0.0),
        (Rect::new(-1e30, -1e30, 2e30, 2e30), 8.0, 0.0),
        (card, 8.0, 1e30),
        (card, 8.0, -3e9),
    ];
    for (rect, blur, dy) in cases {
        let shot = finishes(
            &format!("a shadow of {rect:?} at blur {blur} and dy {dy}"),
            10,
            move || {
                let mut s = Scene::new(100.0, 60.0, 2.0);
                s.push(Item::Shadow {
                    rect,
                    radius: 4.0,
                    blur,
                    dy,
                    alpha: 0.5,
                });
                shoot(&s)
            },
        );
        if rect == card && dy == 0.0 {
            // Each blur here is past the most, so it's drawn at the most: a haze over the card.
            let most = raster::MAX_BLUR;
            let mut s = Scene::new(100.0, 60.0, 2.0);
            s.push(Item::Shadow {
                rect,
                radius: 4.0,
                blur: most,
                dy,
                alpha: 0.5,
            });
            assert!(shot.alpha(80, 60) > 0, "something under the card at {blur}");
            assert!(shot.px == shoot(&s).px, "blur {blur} is drawn at {most}");
        }
    }
}

#[test]
fn an_opacity_group_lays_down_everything_it_drew() {
    // Marks that reach past their paths, apart, in a group at a half: each as if drawn at half.
    let marks = |alpha: f32| {
        let thick = |cap, join| Stroke {
            cap,
            join,
            ..Stroke::solid(6.0, WHITE, alpha)
        };
        vec![
            // A mitre's spike, four half-widths past its corner.
            line(
                &[(10.0, 40.0), (25.0, 12.0), (40.0, 40.0)],
                thick(Cap::Butt, Join::Miter),
            ),
            line(
                &[(55.0, 20.0), (75.0, 20.0)],
                thick(Cap::Square, Join::Round),
            ),
            line(
                &[(55.0, 35.0), (75.0, 35.0)],
                thick(Cap::Round, Join::Round),
            ),
            line(
                &[(85.0, 5.0), (115.0, 5.0), (115.0, 30.0)],
                Stroke {
                    dash: Some((3.0, 2.0)),
                    cap: Cap::Square,
                    ..Stroke::hairline(WHITE, alpha)
                },
            ),
            line(
                &[(85.0, 45.0), (110.0, 38.0)],
                Stroke::hairline(WHITE, alpha),
            ),
            Item::Path {
                path: PathData::circle(20.0, 52.0, 5.5),
                fill: Some(Paint::solid(WHITE, alpha)),
                stroke: None,
            },
            Item::Shadow {
                rect: Rect::new(50.0, 45.0, 20.0, 8.0),
                radius: 3.0,
                blur: 4.0,
                dy: 1.0,
                alpha: alpha * 0.8,
            },
        ]
    };
    for scale in [1.0f32, 1.5, 2.0] {
        let mut grouped = Scene::new(120.0, 60.0, scale);
        grouped.push(Item::PushOpacity(0.5));
        for m in marks(1.0) {
            grouped.push(m);
        }
        grouped.push(Item::PopOpacity);
        let mut each = Scene::new(120.0, 60.0, scale);
        for m in marks(0.5) {
            each.push(m);
        }
        let (g, e) = (shoot(&grouped), shoot(&each));
        for y in 0..g.h {
            for x in 0..g.w {
                let (a, b) = (g.at(x, y), e.at(x, y));
                assert!(
                    a.iter().zip(b).all(|(&p, q)| near(p, q, 2)),
                    "({x}, {y}) at {scale}: {a:?} in the group, {b:?} alone"
                );
            }
        }
    }
}

#[test]
#[ignore = "speed"]
fn a_whole_redraw_with_faded_series_costs_about_what_one_without_does() {
    // A chart 2880 × 1800: a legend's swatches and an axis outside the plot, and in its clip
    // gridlines, 60 opaque bars and 20 series, each in a faded group or not.
    let scene = |groups: bool| {
        let mut s = Scene::new(1440.0, 900.0, 2.0);
        for i in 0..20 {
            s.push(rect(
                Rect::new(60.0 + i as f32 * 60.0, 20.0, 10.0, 10.0),
                BLUE,
                1.0,
            ));
        }
        s.push(line(
            &[(50.0, 860.0), (1420.0, 860.0)],
            Stroke::hairline(WHITE, 1.0),
        ));
        s.push(Item::PushClip(Rect::new(50.0, 50.0, 1370.0, 810.0)));
        for i in 0..10 {
            let y = 80.0 + i as f32 * 80.0;
            s.push(line(
                &[(50.0, y), (1420.0, y)],
                Stroke::hairline(GREEN, 1.0),
            ));
        }
        for i in 0..60 {
            let h = 50.0 + (i * 37 % 300) as f32;
            s.push(rect(
                Rect::new(60.0 + i as f32 * 22.0, 860.0 - h, 16.0, h),
                BLUE,
                1.0,
            ));
        }
        for i in 0..20 {
            if groups {
                s.push(Item::PushOpacity(0.3));
            }
            let pts: Vec<(f32, f32)> = (0..200)
                .map(|k| {
                    let x = 50.0 + k as f32 * 6.9;
                    (x, 450.0 + 200.0 * ((x + i as f32 * 40.0) / 90.0).sin())
                })
                .collect();
            s.push(line(&pts, Stroke::solid(2.0, RED, 1.0)));
            if groups {
                s.push(Item::PopOpacity);
            }
        }
        s.push(Item::PopClip);
        s
    };
    let time = |s: &Scene| {
        let mut px = vec![0u8; 2880 * 1800 * 4];
        let runs = 5;
        let t = std::time::Instant::now();
        for _ in 0..runs {
            raster::draw(s, &mut px, 2880, 1800, 2880 * 4, None, Some(WHITE), None);
        }
        t.elapsed() / runs
    };
    let (plain, faded) = (time(&scene(false)), time(&scene(true)));
    eprintln!("a whole redraw: {plain:?} with no groups, {faded:?} with 20");
    assert!(faded < plain * 2, "{faded:?} against {plain:?}");
}

// ------------------------------------------------------------------ damage

/// A scene with one of everything the raster draws without fonts.
fn busy(scale: f32) -> Scene {
    let mut s = Scene::new(80.0, 60.0, scale);
    s.push(Item::Shadow {
        rect: Rect::new(30.0, 20.0, 30.0, 20.0),
        radius: 4.0,
        blur: 6.0,
        dy: 3.0,
        alpha: 0.6,
    });
    s.push(Item::Path {
        path: PathData::rounded(Rect::new(30.0, 20.0, 30.0, 20.0), [4.0; 4]),
        fill: Some(Paint::Linear {
            from: (30.0, 20.0),
            to: (60.0, 40.0),
            stops: vec![(0.0, RED, 1.0), (1.0, BLUE, 0.5)],
        }),
        stroke: Some(Stroke::solid(1.5, WHITE, 0.8)),
    });
    s.push(Item::PushClip(Rect::new(5.0, 5.0, 50.0, 40.0)));
    for i in 0..6 {
        let y = 8.0 + i as f32 * 7.0;
        s.push(line(&[(0.0, y), (80.0, y)], Stroke::hairline(GREEN, 0.7)));
    }
    s.push(Item::PushOpacity(0.4));
    s.push(line(
        &[(0.0, 50.0), (20.0, 10.0), (40.0, 30.0), (80.0, 0.0)],
        Stroke {
            cap: Cap::Round,
            join: Join::Round,
            ..Stroke::solid(3.0, RED, 1.0)
        },
    ));
    s.push(Item::PopOpacity);
    s.push(Item::PopClip);
    s.push(Item::Path {
        path: PathData::circle(70.0, 50.0, 6.0),
        fill: Some(Paint::solid(BLUE, 0.9)),
        stroke: None,
    });
    // Opaque and clipped by nothing, as a legend's swatch, a card's ground or a bar is: an
    // opaque paint's edges are where tiny-skia, unmasked, would lay pixels down otherwise.
    s.push(Item::Path {
        path: PathData::circle(22.3, 37.1, 17.3),
        fill: Some(Paint::solid(RED, 1.0)),
        stroke: None,
    });
    s.push(line(
        &[(2.0, 58.0), (37.5, 21.25), (61.0, 55.5), (79.0, 3.0)],
        Stroke {
            cap: Cap::Round,
            join: Join::Round,
            ..Stroke::solid(2.5, WHITE, 1.0)
        },
    ));
    s.push(Item::Path {
        path: PathData::rounded(Rect::new(62.3, 8.4, 13.1, 9.7), [3.0; 4]),
        fill: Some(Paint::solid(GREEN, 1.0)),
        stroke: Some(Stroke::solid(1.0, Color(0x3a, 0x3a, 0x3a), 1.0)),
    });
    s
}

const SENTINEL: [u8; 4] = [1, 2, 3, 4];

#[test]
fn damage_is_cleared_and_drawn_and_nothing_else_is_touched() {
    let grounds = [Some(Color(9, 9, 9)), None, Some(Color(0xc8, 0xdc, 0xf0))];
    for (scale, ground) in [1.0f32, 2.0, 1.5]
        .into_iter()
        .flat_map(|s| grounds.map(|g| (s, g)))
    {
        let scene = busy(scale);
        let whole = shoot_on(&scene, ground);
        let (w, h) = (whole.w, whole.h);
        for (damage, pad) in [
            (
                PixelBox {
                    x: 23,
                    y: 17,
                    w: 41,
                    h: 29,
                },
                0usize,
            ),
            (
                PixelBox {
                    x: 0,
                    y: 11,
                    w,
                    h: 13,
                },
                0,
            ),
            (
                PixelBox {
                    x: 0,
                    y: 11,
                    w,
                    h: 13,
                },
                12,
            ),
            (
                PixelBox {
                    x: 31,
                    y: 2,
                    w: 9,
                    h: 40,
                },
                20,
            ),
            (
                PixelBox {
                    x: w - 10,
                    y: h - 5,
                    w: 100,
                    h: 100,
                },
                4,
            ),
        ] {
            let stride = w as usize * 4 + pad;
            let mut px = vec![0xab; stride * h as usize];
            for row in px.chunks_exact_mut(stride) {
                for p in row[..w as usize * 4].chunks_exact_mut(4) {
                    p.copy_from_slice(&SENTINEL);
                }
            }
            raster::draw(&scene, &mut px, w, h, stride, Some(damage), ground, None);
            let inside = |x: u32, y: u32| {
                x >= damage.x && x < damage.x + damage.w && y >= damage.y && y < damage.y + damage.h
            };
            for y in 0..h {
                let row = &px[y as usize * stride..(y as usize + 1) * stride];
                assert!(
                    row[w as usize * 4..].iter().all(|&b| b == 0xab),
                    "a row's padding is never written"
                );
                for x in 0..w {
                    let p = &row[x as usize * 4..x as usize * 4 + 4];
                    if inside(x, y) {
                        assert_eq!(
                            p,
                            whole.at(x, y),
                            "({x}, {y}) in {damage:?} at {scale} on {ground:?}"
                        );
                    } else {
                        assert_eq!(p, SENTINEL, "({x}, {y}) outside {damage:?} at {scale}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_shadow_redrawn_in_any_box_is_what_a_whole_redraw_puts_there() {
    // Boxes that start anywhere in a blur's rows and columns: its sums run from where each
    // box's reach begins, so they must come to the same wherever that is.
    let mut seed = 12345u64;
    let mut next = |n: u32| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as u32) % n
    };
    let ground = Some(Color(200, 220, 240));
    for (scale, blur, radius) in [(1.0f32, 8.0f32, 4.0f32), (2.0, 12.0, 8.0), (1.5, 6.0, 3.0)] {
        let mut s = Scene::new(150.0, 100.0, scale);
        s.push(Item::Shadow {
            rect: Rect::new(20.3, 15.7, 100.0, 60.0),
            radius,
            blur,
            dy: 3.0,
            alpha: 0.7,
        });
        let whole = shoot_on(&s, ground);
        let (w, h) = (whole.w, whole.h);
        for _ in 0..60 {
            let (x, y) = (next(w), next(h));
            let d = PixelBox {
                x,
                y,
                w: 1 + next(w - x),
                h: 1 + next(h - y),
            };
            let mut px = vec![0u8; (w * h * 4) as usize];
            raster::draw(&s, &mut px, w, h, w as usize * 4, Some(d), ground, None);
            let part = Shot { w, h, px };
            for yy in d.y..d.y + d.h {
                for xx in d.x..d.x + d.w {
                    assert_eq!(
                        part.at(xx, yy),
                        whole.at(xx, yy),
                        "({xx}, {yy}) in {d:?} at {scale}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_clear_is_a_ground_or_nothing() {
    let s = Scene::new(4.0, 4.0, 1.0);
    assert!(shoot(&s).px.iter().all(|&b| b == 0), "transparent");
    let shot = shoot_on(&s, Some(Color(10, 20, 30)));
    assert!(
        shot.px.chunks_exact(4).all(|p| p == [10, 20, 30, 255]),
        "opaque"
    );
    // A buffer too short for its size is left alone.
    let mut px = vec![7u8; 10];
    raster::draw(&s, &mut px, 4, 4, 16, None, Some(WHITE), None);
    assert!(px.iter().all(|&b| b == 7));
}

#[test]
fn to_physical_rounds_out_and_clips() {
    let b = raster::to_physical(Rect::new(1.2, 2.5, 3.0, 4.0), 2.0, 100, 100);
    assert_eq!(
        b,
        PixelBox {
            x: 2,
            y: 5,
            w: 7,
            h: 8
        }
    );
    let b = raster::to_physical(Rect::new(-5.0, -5.0, 10.0, 10.0), 1.0, 20, 20);
    assert_eq!(
        b,
        PixelBox {
            x: 0,
            y: 0,
            w: 5,
            h: 5
        }
    );
    let b = raster::to_physical(Rect::new(15.0, 15.0, 10.0, 10.0), 1.0, 20, 20);
    assert_eq!(
        b,
        PixelBox {
            x: 15,
            y: 15,
            w: 5,
            h: 5
        }
    );
    assert!(raster::to_physical(Rect::new(30.0, 0.0, 5.0, 5.0), 1.0, 20, 20).is_empty());
    assert!(raster::to_physical(Rect::new(1.0, 1.0, 0.0, 5.0), 1.0, 20, 20).is_empty());
    assert!(raster::to_physical(Rect::new(f32::NAN, 1.0, 3.0, 5.0), 1.0, 20, 20).is_empty());
    let a = PixelBox {
        x: 2,
        y: 2,
        w: 4,
        h: 4,
    };
    let c = PixelBox {
        x: 4,
        y: 0,
        w: 4,
        h: 3,
    };
    assert_eq!(
        a.union(&c),
        PixelBox {
            x: 2,
            y: 0,
            w: 6,
            h: 6
        }
    );
    assert_eq!(
        a.intersect(&c),
        PixelBox {
            x: 4,
            y: 2,
            w: 2,
            h: 1
        }
    );
    let r: die_core::PixelRect = a.into();
    assert_eq!(r, die_core::PixelRect::new(2, 2, 4, 4));
}

// ------------------------------------------------------------------ PNG

#[test]
fn a_png_is_the_scene_at_its_scale() {
    let mut s = Scene::new(10.0, 6.0, 2.0);
    s.push(rect(Rect::new(0.0, 0.0, 5.0, 6.0), RED, 0.5));
    let png = raster::png(&s, Color(20, 30, 40), None).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    assert_eq!(
        (be(&png[16..20]), be(&png[20..24])),
        (20, 12),
        "2× the scene"
    );
    assert_eq!((png[24], png[25]), (8, 6), "8-bit RGBA");
    assert!(raster::png(&Scene::new(0.0, 10.0, 1.0), WHITE, None).is_err());
    assert!(raster::png(&Scene::new(20_000.0, 10.0, 1.0), WHITE, None).is_err());
}

#[test]
fn a_png_is_opaque_on_its_background() {
    let mut s = Scene::new(10.0, 6.0, 2.0);
    s.push(rect(Rect::new(0.0, 0.0, 5.0, 6.0), RED, 0.5));
    let png = raster::png(&s, Color(20, 30, 40), None).unwrap();
    let img = die_draw::image::decode(&png).unwrap();
    assert_eq!((img.width, img.height), (20, 12));
    assert!(img.pixels.chunks_exact(4).all(|p| p[3] == 255), "opaque");
    let at = |x: usize, y: usize| &img.pixels[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4];
    assert_eq!(at(15, 5), [20, 30, 40, 255], "the background");
    assert!(
        near(at(5, 5)[0], 138, 2),
        "half red over it: {:?}",
        at(5, 5)
    );
}

// ------------------------------------------------------------------ text

fn book() -> Option<die_draw::Book> {
    die_draw::Book::load(Some(&die_draw::font_dir(None))).ok()
}

fn text(s: &str, x: f32, y: f32, anchor: Anchor, alpha: f32, max: Option<f32>) -> Item {
    Item::Text {
        text: s.into(),
        x,
        y,
        spec: TextSpec::new(16.0).weight(Weight::Semibold),
        anchor,
        color: WHITE,
        alpha,
        max_width: max,
    }
}

/// The columns of a shot with any ink in rows `y0..y1`.
fn ink_span(shot: &Shot, y0: u32, y1: u32) -> Option<(u32, u32)> {
    let cols: Vec<u32> = (0..shot.w)
        .filter(|&x| (y0..y1).any(|y| shot.alpha(x, y) > 0))
        .collect();
    Some((*cols.first()?, *cols.last()?))
}

#[test]
fn text_is_drawn_at_its_anchor_in_its_colour_at_its_alpha() {
    let mut fonts = book().expect("fonts");
    for scale in [1.0f32, 2.0] {
        let mut s = Scene::new(200.0, 100.0, scale);
        s.push(text("Hello", 100.0, 20.0, Anchor::Start, 1.0, None));
        s.push(text("Hello", 100.0, 45.0, Anchor::End, 1.0, None));
        s.push(text("Hello", 100.0, 70.0, Anchor::Middle, 0.5, None));
        s.push(Item::PushClip(Rect::new(0.0, 75.0, 30.0, 25.0)));
        s.push(text("Hello world", 10.0, 95.0, Anchor::Start, 1.0, None));
        s.push(Item::PopClip);
        let w = (200.0 * scale) as u32;
        let h = (100.0 * scale) as u32;
        let mut px = vec![0; (w * h * 4) as usize];
        raster::draw(
            &s,
            &mut px,
            w,
            h,
            w as usize * 4,
            None,
            None,
            Some(&mut fonts),
        );
        let shot = Shot { w, h, px };
        let p = |v: f32| (v * scale) as u32;
        let (l, r) = ink_span(&shot, p(5.0), p(25.0)).expect("start");
        assert!(l >= p(99.0) && r > p(120.0), "starts at x: {l}..{r}");
        let (l, r) = ink_span(&shot, p(30.0), p(50.0)).expect("end");
        assert!(r <= p(101.0) && l < p(80.0), "ends at x: {l}..{r}");
        let (l, r) = ink_span(&shot, p(55.0), p(75.0)).expect("middle");
        assert!(l < p(100.0) && r > p(100.0), "about x: {l}..{r}");
        let most = |y0: f32, y1: f32| {
            let mut m = 0;
            for y in p(y0)..p(y1) {
                for x in 0..w {
                    m = m.max(shot.alpha(x, y));
                }
            }
            m
        };
        assert_eq!(most(5.0, 25.0), 255, "solid white somewhere");
        assert!(near(most(55.0, 75.0), 128, 2), "half at half");
        let (_, r) = ink_span(&shot, p(76.0), p(100.0)).expect("clipped");
        assert!(r < p(30.0), "clipped at 30: {r}");
        let [r0, g0, b0, a0] = shot.at(ink_span(&shot, p(5.0), p(25.0)).unwrap().0, p(15.0));
        assert!(r0 <= a0 && g0 <= a0 && b0 <= a0, "premultiplied");
    }
}

#[test]
fn a_text_past_its_width_is_cut_as_the_measure_cuts_it() {
    let mut fonts = book().expect("fonts");
    let long = "A label far too long for its place";
    let mut m = BookMeasure {
        book: &mut fonts,
        scale: 2.0,
    };
    let spec = TextSpec::new(16.0).weight(Weight::Semibold);
    let cut = m.fit(long, spec, 80.0);
    assert!(cut.ends_with('…') && cut.len() < long.len(), "{cut}");
    let cut_width = m.width(&cut, spec);
    assert!(cut_width <= 80.0);
    let mut s = Scene::new(200.0, 40.0, 2.0);
    s.push(text(long, 10.0, 25.0, Anchor::Start, 1.0, Some(80.0)));
    let mut px = vec![0; 400 * 80 * 4];
    raster::draw(&s, &mut px, 400, 80, 1600, None, None, Some(&mut fonts));
    let shot = Shot { w: 400, h: 80, px };
    let (l, r) = ink_span(&shot, 0, 80).expect("ink");
    assert!(l >= 19 && r <= 20 + 160, "within its 80 px: {l}..{r}");
    assert!(
        r as f32 >= 20.0 + cut_width * 2.0 - 12.0,
        "as wide as the cut: {r}"
    );
}

#[test]
fn book_measure_is_in_logical_px() {
    let mut fonts = book().expect("fonts: copy fonts/ from the main checkout into this one");
    let spec = TextSpec::new(12.0);
    // A line break or a tab is measured as the space the raster and the SVG draw for it.
    let mut m = BookMeasure {
        book: &mut fonts,
        scale: 2.0,
    };
    assert_eq!(m.width("a\nb\tc", spec), m.width("a b c", spec));
    assert_eq!(m.fit("a\nb", spec, 500.0), "a b");
    let at = |fonts: &mut die_draw::Book, scale: f32, t: &str| {
        BookMeasure { book: fonts, scale }.width(t, spec)
    };
    let (one, two) = (
        at(&mut fonts, 1.0, "Requests"),
        at(&mut fonts, 2.0, "Requests"),
    );
    assert!(one > 20.0 && one < 100.0, "{one}");
    assert!(
        (one - two).abs() / one < 0.1,
        "about as wide at 2×: {one} {two}"
    );
    assert!(at(&mut fonts, 2.0, "Requests per second") > two);
    let mut m = BookMeasure {
        book: &mut fonts,
        scale: 2.0,
    };
    let (a, d, c) = (m.ascent(spec), m.descent(spec), m.cap(spec));
    assert!(a > c && c > 0.0 && d > 0.0 && a < 16.0, "{a} {d} {c}");
    let digits = spec.tabular();
    assert!((m.width("1111", digits) - m.width("8888", digits)).abs() < 0.01);
    assert_eq!(m.fit("Requests", spec, 500.0), "Requests");
    let cut = m.fit("Requests per second", spec, 50.0);
    assert!(cut.ends_with('…') && m.width(&cut, spec) <= 50.0, "{cut}");
    let longer = cut.trim_end_matches('…').chars().count() + 1;
    let next: String = "Requests per second".chars().take(longer).collect();
    assert!(
        m.width(&format!("{next}…"), spec) > 50.0,
        "the longest that fits"
    );
    assert_eq!(m.fit("Requests", spec, 1.0), "", "not even the '…'");
}

// ------------------------------------------------------------------ SVG

/// Checks every tag is closed in order; returns the tags opened, by name.
fn tags(doc: &str) -> Vec<String> {
    let mut open: Vec<String> = Vec::new();
    let mut all = Vec::new();
    let mut rest = doc;
    while let Some(i) = rest.find('<') {
        let j = rest[i..].find('>').expect("a tag ends") + i;
        let tag = &rest[i + 1..j];
        rest = &rest[j + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            assert_eq!(
                open.pop().as_deref(),
                Some(name),
                "</{name}> closes what's open"
            );
        } else {
            let name = tag.split([' ', '/']).next().unwrap().to_string();
            all.push(name.clone());
            if !tag.ends_with('/') {
                open.push(name);
            }
        }
    }
    assert!(open.is_empty(), "left open: {open:?}");
    all
}

/// The value of the first `attr="…"` after `from` in `doc`.
fn attr<'a>(doc: &'a str, from: &str, attr: &str) -> &'a str {
    let at = doc.find(from).unwrap_or_else(|| panic!("{from} in {doc}"));
    let key = format!("{attr}=\"");
    let i = doc[at..].find(&key).unwrap() + at + key.len();
    let j = doc[i..].find('"').unwrap() + i;
    &doc[i..j]
}

#[test]
fn svg_is_the_scene_as_elements() {
    let mut s = Scene::new(120.0, 80.0, 2.0);
    s.push(Item::Shadow {
        rect: Rect::new(10.0, 10.0, 50.0, 30.0),
        radius: 8.0,
        blur: 8.0,
        dy: 6.0,
        alpha: 0.55,
    });
    s.push(Item::Path {
        path: PathData::rounded(Rect::new(10.0, 10.0, 50.0, 30.0), [8.0; 4]),
        fill: Some(Paint::solid(Color(0x25, 0x25, 0x25), 1.0)),
        stroke: Some(Stroke::hairline(Color(0x3a, 0x3a, 0x3a), 1.0)),
    });
    s.push(Item::PushClip(Rect::new(0.0, 0.0, 100.0, 60.0)));
    s.push(Item::Path {
        path: PathData::polyline(&[(0.0, 60.0), (50.5, 20.25), (100.0, 30.0)]),
        fill: Some(Paint::Linear {
            from: (0.0, 20.0),
            to: (0.0, 60.0),
            stops: vec![(1.0, BLUE, 0.0), (0.0, BLUE, 0.1)],
        }),
        stroke: Some(Stroke {
            dash: Some((6.0, 4.0)),
            cap: Cap::Round,
            join: Join::Round,
            ..Stroke::solid(2.0, BLUE, 1.0)
        }),
    });
    s.push(Item::PushOpacity(0.5));
    s.push(Item::Text {
        text: "a < b & c".into(),
        x: 60.0,
        y: 70.0,
        spec: TextSpec::new(12.0).tabular(),
        anchor: Anchor::Middle,
        color: Color(0xcc, 0xcc, 0xcc),
        alpha: 0.8,
        max_width: None,
    });
    s.push(Item::PopOpacity);
    s.push(Item::PopClip);
    s.push(Item::Text {
        text: "12,904".into(),
        x: 118.0,
        y: 78.0,
        spec: TextSpec::new(11.0).mono().weight(Weight::Semibold),
        anchor: Anchor::End,
        color: WHITE,
        alpha: 1.0,
        max_width: Some(30.0),
    });
    let doc = svg::write(&s);
    assert!(
        doc.starts_with(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\" \
             viewBox=\"0 0 120 80\">\n"
        ),
        "at 1×, whatever the scale: {doc}"
    );
    assert!(doc.ends_with("</svg>\n"));
    let names = tags(&doc);
    let count = |n: &str| names.iter().filter(|t| *t == n).count();
    assert_eq!(count("path"), 2);
    assert_eq!(count("text"), 2);
    assert_eq!(count("linearGradient"), 1);
    assert_eq!(count("stop"), 2);
    assert_eq!(count("clipPath"), 2, "the clip's and the text's width");
    assert_eq!(count("filter"), 1);
    // The shadow: σ is half the blur, in black at its alpha, dy down.
    assert_eq!(attr(&doc, "<feGaussianBlur", "stdDeviation"), "4");
    let sh = doc.find("filter=\"url(#").map(|i| &doc[..i]).unwrap();
    let rect_at = sh.rfind("<rect").unwrap();
    assert_eq!(attr(&doc[rect_at..], "<rect", "y"), "16");
    assert_eq!(attr(&doc[rect_at..], "<rect", "fill"), "#000000");
    assert_eq!(attr(&doc[rect_at..], "<rect", "fill-opacity"), "0.55");
    // The card: a hairline crisp at any zoom.
    assert_eq!(attr(&doc, "<path", "fill"), "#252525");
    assert_eq!(attr(&doc, "<path", "stroke-width"), "1");
    assert_eq!(attr(&doc, "<path", "shape-rendering"), "crispEdges");
    // The gradient, defined before it's used, its stops in order.
    let gid = attr(&doc, "<linearGradient", "id");
    assert!(gid.starts_with("fig-g"));
    let used = format!("fill=\"url(#{gid})\"");
    assert!(doc.find(&used).unwrap() > doc.find("<linearGradient").unwrap());
    assert_eq!(
        attr(&doc, "<linearGradient", "gradientUnits"),
        "userSpaceOnUse"
    );
    assert_eq!(attr(&doc, "<stop", "offset"), "0");
    assert_eq!(attr(&doc, "<stop", "stop-opacity"), "0.1");
    assert_eq!(attr(&doc, "<stop", "stop-color"), "#0000ff");
    let line = doc.find(&used).unwrap();
    let line = &doc[doc[..line].rfind("<path").unwrap()..];
    assert_eq!(attr(line, "<path", "d"), "M0 60 L50.5 20.25 L100 30");
    assert_eq!(attr(line, "<path", "stroke-dasharray"), "6 4");
    assert_eq!(attr(line, "<path", "stroke-linecap"), "round");
    assert_eq!(attr(line, "<path", "stroke-linejoin"), "round");
    // The clip and the group.
    let cid = attr(&doc, "<clipPath", "id");
    assert!(doc.contains(&format!("<g clip-path=\"url(#{cid})\">")));
    assert!(doc.contains("<g opacity=\"0.5\">"));
    // The texts.
    assert!(doc.contains(">a &lt; b &amp; c</text>"), "escaped");
    assert_eq!(attr(&doc, "<text", "text-anchor"), "middle");
    assert_eq!(
        attr(&doc, "<text", "style"),
        "font-variant-numeric:tabular-nums"
    );
    assert_eq!(attr(&doc, "<text", "fill-opacity"), "0.8");
    assert_eq!(attr(&doc, "<text", "font-family"), svg::FONT_FAMILY);
    let num = &doc[doc
        .find(">12,904<")
        .map(|i| doc[..i].rfind("<text").unwrap())
        .unwrap()..];
    assert_eq!(attr(num, "<text", "font-family"), svg::MONO_FAMILY);
    assert_eq!(attr(num, "<text", "font-weight"), "600");
    assert_eq!(attr(num, "<text", "text-anchor"), "end");
    assert!(attr(num, "<text", "clip-path").starts_with("url(#fig-t"));
}

#[test]
fn svg_text_keeps_its_spaces_and_only_what_xml_can_hold() {
    let mut s = Scene::new(100.0, 40.0, 1.0);
    for (i, t) in [
        "  two  spaces ",
        "nonchar\u{FFFF}\u{FFFE}\u{1}\u{7f}\u{85}!",
        "nl\nline\ttab\rcr",
    ]
    .into_iter()
    .enumerate()
    {
        s.push(Item::Text {
            text: t.into(),
            x: 1.0,
            y: 10.0 * (i + 1) as f32,
            spec: TextSpec::new(10.0),
            anchor: Anchor::Start,
            color: WHITE,
            alpha: 1.0,
            max_width: None,
        });
    }
    let doc = svg::write(&s);
    tags(&doc);
    assert_eq!(
        doc.matches("<text ").count(),
        doc.matches("xml:space=\"preserve\"").count(),
        "every text keeps its spaces: {doc}"
    );
    assert!(doc.contains(">  two  spaces </text>"), "{doc}");
    assert!(doc.contains(">nonchar!</text>"), "{doc}");
    assert!(doc.contains(">nl line tab cr</text>"), "{doc}");
    // Every character in the document is one XML 1.0 can hold.
    assert!(
        doc.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | ' '..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)),
        "{doc:?}"
    );
}

#[test]
fn svg_groups_nest_whatever_order_they_pop_in() {
    let mut s = Scene::new(10.0, 10.0, 1.0);
    s.push(Item::PushClip(Rect::new(0.0, 0.0, 5.0, 5.0)));
    s.push(Item::PushOpacity(0.25));
    s.push(Item::PopClip);
    s.push(rect(Rect::new(0.0, 0.0, 2.0, 2.0), RED, 1.0));
    s.push(Item::PopOpacity);
    s.push(Item::PopOpacity);
    s.push(Item::PushOpacity(0.5));
    let doc = svg::write(&s);
    let names = tags(&doc);
    assert_eq!(names.iter().filter(|n| *n == "g").count(), 4, "{doc}");
    assert_eq!(
        doc.matches("<g opacity=\"0.25\">").count(),
        2,
        "opened again: {doc}"
    );
    let reopened = doc.rfind("<g opacity=\"0.25\">").unwrap();
    assert!(
        doc.find("<path").unwrap() > reopened,
        "the path is in the reopened group"
    );
}

// ------------------------------------------------------------------ the theme

fn harness() -> Harness {
    Harness::new(vec![Box::new(die_fig::Fig::default())]).unwrap()
}

fn ansi() -> [Rgb; 16] {
    die_core::Theme::default().ansi
}

#[test]
fn the_theme_comes_from_the_faces_of_cores_own() {
    let h = harness();
    let t = theme::resolve(&h);
    let dark = Theme::mono_dark();
    assert!(t.dark);
    assert_eq!(t.surface, Color(0x10, 0x10, 0x10), "default's ground");
    assert_eq!(t.text, Color(0xd0, 0xd0, 0xd0), "fig-text is default's");
    assert_eq!(t.muted, Color(0x70, 0x70, 0x70), "fig-tick is shadow's");
    assert_eq!(t.chip_text, t.surface);
    assert_eq!(t.series, dark.series);
    assert_eq!(t.status, dark.status);
    assert_eq!(
        (t.grid, t.baseline, t.tooltip, t.ring, t.chip, t.strong),
        (
            dark.grid,
            dark.baseline,
            dark.tooltip,
            dark.ring,
            dark.chip,
            dark.strong
        ),
        "unstyled, the defaults"
    );
}

#[test]
fn a_light_ground_takes_the_light_set_and_a_styled_face_wins() {
    let mut h = harness();
    let h6 = Rgb::hex;
    h.set_theme(
        die_core::Theme::new("paper", ansi())
            .face("default", Style::new().fg(h6("#333333")).bg(h6("#fafafa")))
            .face("fig-series-2", Style::new().fg(h6("#123456")))
            .face("fig-grid", Style::new().fg(h6("#abcdef")))
            .face("fig-good", Style::new().fg(h6("#00ff00")))
            .face("fig-cursor", Style::new().fg(h6("#202020"))),
    );
    assert!(
        h.messages.iter().all(|m| !m.contains("no one declared")),
        "{:?}",
        h.messages
    );
    let t = theme::resolve(&h);
    let light = Theme::light();
    assert!(!t.dark);
    assert_eq!(t.surface, Color(0xfa, 0xfa, 0xfa));
    assert_eq!(t.chip_text, t.surface);
    assert_eq!(t.text, Color(0x33, 0x33, 0x33));
    assert_eq!(t.series[0], light.series[0]);
    assert_eq!(t.series[1], Color(0x12, 0x34, 0x56));
    assert_eq!(t.series[2..], light.series[2..]);
    assert_eq!(t.grid, Color(0xab, 0xcd, 0xef));
    assert_eq!(t.status[0], Color(0, 0xff, 0));
    assert_eq!(t.status[1..], light.status[1..]);
    assert_eq!(t.chip, Color(0x20, 0x20, 0x20));
    assert_eq!(
        t.muted, light.muted,
        "shadow unstyled here: the light default"
    );
    assert_eq!((t.other, t.accent), (light.other, light.accent));
    // Back to a dark ground with no faces of fig's: the dark set again.
    h.set_theme(
        die_core::Theme::new("night", ansi())
            .face("default", Style::new().fg(h6("#cccccc")).bg(h6("#202428"))),
    );
    let t = theme::resolve(&h);
    assert!(t.dark);
    assert_eq!(t.series, Theme::mono_dark().series);
    assert_eq!(t.surface, Color(0x20, 0x24, 0x28));
}

#[test]
fn with_no_ground_the_dark_defaults_apply() {
    let mut h = harness();
    h.set_theme(
        die_core::Theme::new("bare", ansi()).face("default", Style::new().fg(Rgb::hex("#cccccc"))),
    );
    let t = theme::resolve(&h);
    assert!(t.dark);
    assert_eq!(t.surface, Theme::mono_dark().surface);
    assert_eq!(t.chip_text, t.surface);
    assert!(theme::is_dark(Color(0x7f, 0x7f, 0x7f)));
    assert!(!theme::is_dark(Color(0x80, 0x80, 0x80)));
    assert!(!theme::is_dark(Color(0xfc, 0xfc, 0xfb)));
}

#[test]
fn a_spec_colour_is_a_theme_colour() {
    let t = Theme::mono_dark();
    let of = |c| theme::color_of(c, &t);
    assert_eq!(of(ColorSpec::Slot(0)), t.series[0]);
    assert_eq!(of(ColorSpec::Slot(7)), t.series[7]);
    assert_eq!(of(ColorSpec::Status(Status::Good)), t.status[0]);
    assert_eq!(of(ColorSpec::Status(Status::Critical)), t.status[3]);
    assert_eq!(of(ColorSpec::Dim), t.other);
    assert_eq!(of(ColorSpec::Bright), t.strong);
    assert_eq!(of(ColorSpec::Accent), t.accent);
    assert_eq!(of(ColorSpec::Rgb(1, 2, 3)), Color(1, 2, 3));
    assert_eq!(
        of(ColorSpec::parse("red").unwrap()),
        t.series[7],
        "fig's red is slot 8"
    );
}
