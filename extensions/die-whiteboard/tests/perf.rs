//! A board of 1,000 strokes: how long opening it, a key, a pointer sample during a stroke and a
//! full cell render take. The numbers are printed (`--nocapture`) and held to loose budgets,
//! since the test profile is lightly optimized and the machine may be busy.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Button, Mods, PointerKind};
use die_whiteboard::model::{Board, Point, Shape};
use die_whiteboard::svg::{self, Look};
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

#[test]
fn a_thousand_strokes_open_key_sample_and_render_within_budget() {
    let dir = TempDir::new("perf");
    let path = dir.path().join("big.svg");
    let text = svg::write(&thousand(), &Look::default(), true);
    std::fs::write(&path, &text).unwrap();
    let mut h = harness();
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
    println!(
        "1,000 strokes: open {:.1} ms, key {:.1} ms, pointer sample {:.1} ms, full cell render {:.1} ms ({} bytes of SVG)",
        opened.as_secs_f64() * 1e3,
        key.as_secs_f64() * 1e3,
        sample.as_secs_f64() * 1e3,
        render.as_secs_f64() * 1e3,
        text.len()
    );
    assert!(opened < Duration::from_millis(500), "open {opened:?}");
    assert!(key < Duration::from_millis(200), "key {key:?}");
    assert!(sample < Duration::from_millis(100), "sample {sample:?}");
    assert!(render < Duration::from_millis(200), "render {render:?}");
    let _ = PointerKind::Press(Button::Left);
    let _ = Mods::default();
}
