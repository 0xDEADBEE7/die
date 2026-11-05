//! K4's speed budget: core's share of a keystroke is at most 1 ms (m0-plan K15), on a 10k-line file
//! and on a 1 MB line of any script, and a cursor position costs the same whatever its line's length
//! (DR-09). Each limit gets three tries, so a busy machine doesn't fail it. Laying a long line out
//! again on every edit misses them by far more than a machine's load.

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

/// The median of `n` calls of `f`, in µs.
fn median_us(n: usize, mut f: impl FnMut()) -> f64 {
    let mut us: Vec<f64> = (0..n)
        .map(|_| {
            let t = std::time::Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    us.sort_by(f64::total_cmp);
    us[n / 2]
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
    println!("{tries:.3?}, limit {limit}");
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
        ("中", Wrap::Word),
        ("e\u{301}", Wrap::Word),
        ("中文 ", Wrap::Word),
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
    for unit in ["word ", "中", "e\u{301}", "中文 "] {
        let text = one_mb_line(unit);
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
}

/// The cache's bound: an edit moves every cached line after it, so after a walk through a big file
/// a keystroke at its top costs what it did before the walk.
#[test]
fn a_keystroke_after_visiting_every_line_costs_at_most_1ms() {
    let (mut ctx, b, w) = editor(&"x\n".repeat(100_000), 80, 40);
    let t = ctx.text_mut(b).unwrap();
    for line in 0..t.line_count() {
        std::hint::black_box(t.display_col(t.line_start(line)));
    }
    within_three_tries(1.0, || {
        let us = median_us(21, || {
            t.tx(Origin::User).insert(0, "y").commit().unwrap();
            std::hint::black_box(t.screen_pos(w, 0));
        });
        us / 1e3
    });
}

/// A Face shapes no rows, so adding or removing one keeps what's laid out, even a whole 1 MB line.
#[test]
fn a_face_decoration_costs_no_layout() {
    let (mut ctx, b, w) = editor(&one_mb_line("word "), 80, 40);
    let t = ctx.text_mut(b).unwrap();
    let end = t.len() - 1;
    t.set_cursor(w, end);
    std::hint::black_box(t.screen_pos(w, end));
    within_three_tries(1.0, || {
        let us = median_us(21, || {
            let face = t.decorate(0..1, Deco::Face("error")).unwrap();
            std::hint::black_box(t.screen_pos(w, end));
            t.undecorate(face);
            std::hint::black_box(t.screen_pos(w, end));
        });
        us / 1e3
    });
}

/// DR-09's root cause: `normalize_views`, `snap` and `set_selection` must not lay out the line.
#[test]
fn a_cursor_position_costs_the_same_whatever_the_lines_length() {
    let measure = |len: usize| {
        let (mut ctx, b, w) = editor(&format!("{}\n", "x".repeat(len)), 80, 40);
        let t = ctx.text_mut(b).unwrap();
        let mut p = len / 2;
        median_us(201, || {
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

/// What a cursor position costs when `snap` has to look for one, in µs: every direction, `ahead`
/// bytes past an edit. The edit drops whatever layout there was, and moves that position along.
fn snap_us(t: &mut TextBuf, mut edit_at: usize, ahead: usize) -> f64 {
    median_us(201, || {
        t.tx(Origin::Extension)
            .insert(edit_at, "y")
            .commit()
            .unwrap();
        edit_at += 1;
        let p = edit_at + ahead;
        for dir in [None, Some(Dir::Forward), Some(Dir::Backward)] {
            std::hint::black_box(t.snap(p, dir));
        }
        std::hint::black_box(t.is_stop(p));
    })
}

/// DR-09 again, where the position isn't a cursor position, so `snap` has work to do. The test above
/// stands on positions that already are, and `is_stop` answers those at once.
#[test]
fn a_position_that_isnt_a_cursor_position_costs_the_same_whatever_the_lines_length() {
    let in_cluster = |len: usize| {
        let (mut ctx, b, _) = editor(&format!("{}\n", "e\u{301}".repeat(len / 3)), 80, 40);
        // Between an e and its mark.
        snap_us(ctx.text_mut(b).unwrap(), len / 6 * 3, 1)
    };
    let hidden = |len: usize, replacement: Option<&str>| {
        let (mut ctx, b, _) = editor(&format!("{}\n", "x".repeat(len)), 80, 40);
        let t = ctx.text_mut(b).unwrap();
        let start = len / 2;
        let deco = Deco::Hide {
            replacement: replacement.map(Into::into),
            reveal: Reveal::Never,
        };
        t.decorate(start..start + 10, deco).unwrap();
        // The edit is just before the Hide, and the position is 5 bytes into it.
        snap_us(t, start - 1, 6)
    };
    within_three_tries(4.0, || in_cluster(1 << 20) / in_cluster(1 << 10));
    within_three_tries(4.0, || hidden(1 << 20, None) / hidden(1 << 10, None));
    within_three_tries(4.0, || {
        hidden(1 << 20, Some("fold")) / hidden(1 << 10, Some("fold"))
    });
}

/// Replaying a count costs each replayed edit the same, however long the line has grown (DR-09). The
/// same edits on a new line each time: on one growing line, a cost that goes with the length would
/// differ too little from one run to the next to show.
#[test]
fn edits_cost_the_same_on_a_1_kb_line_and_a_1_mb_line() {
    let measure = |len: usize| {
        let (mut ctx, b, w) = editor(&format!("{}\n", "x".repeat(len)), 80, 40);
        let t = ctx.text_mut(b).unwrap();
        t.set_cursor(w, len);
        let t0 = std::time::Instant::now();
        for _ in 0..500 {
            let p = t.cursor(w).unwrap();
            t.tx(Origin::User).insert(p, "x").commit().unwrap();
            t.set_cursor(w, p + 1);
        }
        t0.elapsed().as_secs_f64()
    };
    within_three_tries(4.0, || measure(1 << 20) / measure(1 << 10));
}
