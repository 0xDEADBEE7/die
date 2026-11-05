//! K4's speed budget: core's share of a keystroke is at most 1 ms (m0-plan K15), on a 10k-line file
//! and on a 1 MB line, and a cursor position costs the same whatever its line's length (DR-09).
//! Each limit gets three tries, so a busy machine doesn't fail it. Laying a long line out again on
//! every edit misses them by far more than a machine's load.

use die_core::*;

fn editor(text: &str, cols: usize, rows: usize) -> (Ctx, BufferId, WindowId) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, cols, rows).unwrap();
    ctx.focus(w);
    (ctx, b, w)
}

fn compose(ctx: &mut Ctx, w: WindowId, cols: usize, rows: usize) -> Scene {
    ctx.compose(
        cols,
        rows + 2,
        &[WindowPlacement {
            window: w,
            col: 0,
            row: 0,
        }],
    )
}

/// The median cost of a keystroke, in ms: insert a character at the cursor, then compose a frame.
fn keystroke_ms(ctx: &mut Ctx, b: BufferId, w: WindowId, cols: usize, rows: usize) -> f64 {
    let mut ms: Vec<f64> = (0..41)
        .map(|_| {
            let t0 = std::time::Instant::now();
            let t = ctx.text_mut(b).unwrap();
            let p = t.cursor(w).unwrap();
            t.tx(Origin::User).insert(p, "x").commit().unwrap();
            t.set_cursor(w, p + 1);
            std::hint::black_box(compose(ctx, w, cols, rows));
            t0.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    ms[20]
}

fn within_three_tries(limit: f64, mut measure: impl FnMut() -> f64) {
    let tries: Vec<f64> = (0..3).map(|_| measure()).collect();
    assert!(
        tries.iter().any(|&m| m <= limit),
        "{tries:.3?} are all over {limit}"
    );
}

fn ten_k_lines() -> String {
    (0..10_000)
        .map(|i| format!("    let line_{i} = compute(some_value, another_value) + {i};\n"))
        .collect()
}

fn one_mb_line(unit: &str) -> String {
    let mut s = unit.repeat((1 << 20) / unit.len());
    s.push('\n');
    s
}

#[test]
fn a_keystroke_on_a_10k_line_file_costs_at_most_1ms() {
    let text = ten_k_lines();
    let (mut ctx, b, w) = editor(&text, 100, 50);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Visual;
    let middle = t.line_start(5_000);
    t.set_cursor(w, middle);
    within_three_tries(1.0, || keystroke_ms(&mut ctx, b, w, 100, 50));
}

#[test]
fn a_keystroke_at_either_end_of_a_1_mb_line_costs_at_most_1ms() {
    for (unit, wrap) in [
        ("word ", Wrap::Word),
        ("x", Wrap::Word),
        ("word ", Wrap::Char),
        ("word ", Wrap::Off),
    ] {
        let text = one_mb_line(unit);
        let (mut ctx, b, w) = editor(&text, 80, 40);
        let t = ctx.text_mut(b).unwrap();
        t.settings_mut().wrap = wrap;
        t.set_cursor(w, 0);
        within_three_tries(1.0, || keystroke_ms(&mut ctx, b, w, 80, 40));
        let t = ctx.text_mut(b).unwrap();
        let end = t.len() - 1;
        t.set_cursor(w, end);
        // The first frame there lays the line out once. It isn't a keystroke.
        compose(&mut ctx, w, 80, 40);
        within_three_tries(1.0, || keystroke_ms(&mut ctx, b, w, 80, 40));
    }
}

#[test]
fn moving_to_the_far_end_of_a_1_mb_line_costs_at_most_100ms() {
    let text = one_mb_line("word ");
    let (mut ctx, b, w) = editor(&text, 80, 40);
    within_three_tries(100.0, || {
        let t = ctx.text_mut(b).unwrap();
        // An edit at the start leaves no row to reuse.
        t.tx(Origin::User).insert(0, "x").commit().unwrap();
        let end = t.len() - 1;
        t.set_cursor(w, end);
        let t0 = std::time::Instant::now();
        std::hint::black_box(compose(&mut ctx, w, 80, 40));
        t0.elapsed().as_secs_f64() * 1e3
    });
}

/// The median of 201 calls of `f`, in µs.
fn median_us(mut f: impl FnMut()) -> f64 {
    let mut us: Vec<f64> = (0..201)
        .map(|_| {
            let t = std::time::Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    us.sort_by(f64::total_cmp);
    us[100]
}

/// DR-09's root cause: `normalize_views`, `snap` and `set_selection` must not lay out the line.
#[test]
fn a_cursor_position_costs_the_same_whatever_the_lines_length() {
    let measure = |len: usize| {
        let (mut ctx, b, w) = editor(&format!("{}\n", "x".repeat(len)), 80, 40);
        let t = ctx.text_mut(b).unwrap();
        let mut p = len / 2;
        median_us(|| {
            // An edit drops whatever layout there was, so the next call can't lean on a cache.
            t.tx(Origin::Extension).insert(p, "y").commit().unwrap();
            p += 1;
            std::hint::black_box(t.snap(p, Some(Dir::Backward)));
            std::hint::black_box(t.is_stop(p));
            t.set_cursor(w, p);
        })
    };
    within_three_tries(4.0, || measure(1 << 20) / measure(1 << 10));
}

/// Replaying a count costs each replayed edit the same, however long the line grows (DR-09).
#[test]
fn edits_on_a_growing_line_cost_the_same() {
    let (mut ctx, b, w) = editor("\n", 80, 40);
    let t = ctx.text_mut(b).unwrap();
    within_three_tries(4.0, || {
        let t0 = std::time::Instant::now();
        for _ in 0..20_000 {
            let p = t.cursor(w).unwrap();
            t.tx(Origin::User).insert(p, "x").commit().unwrap();
            t.set_cursor(w, p + 1);
        }
        let first = t0.elapsed().as_secs_f64();
        let t0 = std::time::Instant::now();
        for _ in 0..20_000 {
            let p = t.cursor(w).unwrap();
            t.tx(Origin::User).insert(p, "x").commit().unwrap();
            t.set_cursor(w, p + 1);
        }
        t0.elapsed().as_secs_f64() / first
    });
}
