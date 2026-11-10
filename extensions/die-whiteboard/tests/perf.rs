//! A board of 1,000 strokes: how long opening it, a key, a pointer sample during a stroke (its
//! segment rasterized and presented), a full re-raster, a wheel pan and a full cell render take.
//! The numbers are printed (`--nocapture`) and held to loose budgets, since the test profile is
//! lightly optimized and the machine may be busy.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Mods, PointerKind};
use die_whiteboard::model::{Board, Point, Shape};
use die_whiteboard::svg::{self, Look};
use die_whiteboard::{Options, Whiteboard};
use std::time::{Duration, Instant};

fn thousand() -> Board {
    let mut b = Board::default();
    let mut seed: u32 = 7;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for i in 0..1000 {
        let (mut x, mut y) = (rnd() * 400.0, rnd() * 280.0);
        let mut points = vec![Point::new(x, y)];
        for _ in 0..19 {
            x += rnd() * 6.0 - 3.0;
            y += rnd() * 6.0 - 3.0;
            points.push(Point::new(x, y));
        }
        b.add(
            Shape::Stroke {
                points,
                highlighter: false,
            },
            (i % 6 + 1) as u8,
            1,
        );
    }
    b
}

struct Numbers {
    opened: Duration,
    key: Duration,
    sample: Duration,
    render: Duration,
    pan: Duration,
}

/// Open the board, press a key, draw a stroke timing its third sample, draw it all again, and
/// pan by a line, with the whiteboard's `options`.
fn measure(options: Options) -> (Numbers, usize) {
    let dir = TempDir::new("perf");
    let path = dir.path().join("big.svg");
    let text = svg::write(&thousand(), &Look::default(), true);
    std::fs::write(&path, &text).unwrap();
    let mut h = harness_with(options);
    let t = Instant::now();
    open(&mut h, &path);
    let opened = t.elapsed();
    let buf = h.focused_buffer().unwrap();
    assert_eq!(elements(&mut h, buf).len(), 1000);
    let win = h.focused().unwrap();
    let mut s = Setup {
        h,
        dir,
        buf,
        win,
        path,
    };
    let t = Instant::now();
    keys(&mut s.h, "2");
    let key = t.elapsed();
    press(&mut s, (10.0, 10.0));
    drag(&mut s, (12.0, 12.0));
    let t = Instant::now();
    drag(&mut s, (14.0, 14.0));
    let sample = t.elapsed();
    release(&mut s, (16.0, 16.0));
    {
        let shared = board(&mut s.h, s.buf);
        shared.borrow_mut().redraw();
    }
    let t = Instant::now();
    s.h.scene();
    let render = t.elapsed();
    let t = Instant::now();
    pointer(
        &mut s,
        PointerKind::Scroll { dx: 0.0, dy: 1.0 },
        (50.0, 50.0),
        Mods::default(),
        0,
    );
    s.h.scene();
    let pan = t.elapsed();
    (
        Numbers {
            opened,
            key,
            sample,
            render,
            pan,
        },
        text.len(),
    )
}

#[test]
fn a_thousand_strokes_open_key_sample_and_render_within_budget() {
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let (px, bytes) = measure(Whiteboard::for_tests().options);
    println!(
        "1,000 strokes in pixels: open {:.1} ms, key {:.1} ms, pointer sample (rasterize + present) {:.2} ms, full re-raster {:.1} ms, wheel pan {:.1} ms ({bytes} bytes of SVG)",
        ms(px.opened),
        ms(px.key),
        ms(px.sample),
        ms(px.render),
        ms(px.pan),
    );
    let (cells, _) = measure(Options {
        pixels: false,
        ..Whiteboard::for_tests().options
    });
    println!(
        "1,000 strokes in cells: open {:.1} ms, key {:.1} ms, pointer sample {:.2} ms, full cell render {:.1} ms, wheel pan {:.1} ms",
        ms(cells.opened),
        ms(cells.key),
        ms(cells.sample),
        ms(cells.render),
        ms(cells.pan),
    );
    for n in [&px, &cells] {
        assert!(n.opened < Duration::from_millis(500), "open {:?}", n.opened);
        assert!(n.key < Duration::from_millis(200), "key {:?}", n.key);
        assert!(
            n.sample < Duration::from_millis(100),
            "sample {:?}",
            n.sample
        );
        assert!(
            n.render < Duration::from_millis(500),
            "render {:?}",
            n.render
        );
        assert!(n.pan < Duration::from_millis(500), "pan {:?}", n.pan);
    }
}
