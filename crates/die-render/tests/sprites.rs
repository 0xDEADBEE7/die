//! Renderer-drawn glyphs, as coverage bitmaps. No font and no GPU, so these run
//! under `cargo xtask ci --linux`.

use die_core::Underline;
use die_render::sprite::{Line, draw, line};

/// Every code point of the box-drawing block with its Unicode name, less "BOX
/// DRAWINGS". Unicode's names, not the renderer's table, say what each glyph's arms
/// are, so a slip in that table fails.
const NAMES: &str = "\
2500 LIGHT HORIZONTAL
2501 HEAVY HORIZONTAL
2502 LIGHT VERTICAL
2503 HEAVY VERTICAL
250C LIGHT DOWN AND RIGHT
250D DOWN LIGHT AND RIGHT HEAVY
250E DOWN HEAVY AND RIGHT LIGHT
250F HEAVY DOWN AND RIGHT
2510 LIGHT DOWN AND LEFT
2511 DOWN LIGHT AND LEFT HEAVY
2512 DOWN HEAVY AND LEFT LIGHT
2513 HEAVY DOWN AND LEFT
2514 LIGHT UP AND RIGHT
2515 UP LIGHT AND RIGHT HEAVY
2516 UP HEAVY AND RIGHT LIGHT
2517 HEAVY UP AND RIGHT
2518 LIGHT UP AND LEFT
2519 UP LIGHT AND LEFT HEAVY
251A UP HEAVY AND LEFT LIGHT
251B HEAVY UP AND LEFT
251C LIGHT VERTICAL AND RIGHT
251D VERTICAL LIGHT AND RIGHT HEAVY
251E UP HEAVY AND RIGHT DOWN LIGHT
251F DOWN HEAVY AND RIGHT UP LIGHT
2520 VERTICAL HEAVY AND RIGHT LIGHT
2521 DOWN LIGHT AND RIGHT UP HEAVY
2522 UP LIGHT AND RIGHT DOWN HEAVY
2523 HEAVY VERTICAL AND RIGHT
2524 LIGHT VERTICAL AND LEFT
2525 VERTICAL LIGHT AND LEFT HEAVY
2526 UP HEAVY AND LEFT DOWN LIGHT
2527 DOWN HEAVY AND LEFT UP LIGHT
2528 VERTICAL HEAVY AND LEFT LIGHT
2529 DOWN LIGHT AND LEFT UP HEAVY
252A UP LIGHT AND LEFT DOWN HEAVY
252B HEAVY VERTICAL AND LEFT
252C LIGHT DOWN AND HORIZONTAL
252D LEFT HEAVY AND RIGHT DOWN LIGHT
252E RIGHT HEAVY AND LEFT DOWN LIGHT
252F DOWN LIGHT AND HORIZONTAL HEAVY
2530 DOWN HEAVY AND HORIZONTAL LIGHT
2531 RIGHT LIGHT AND LEFT DOWN HEAVY
2532 LEFT LIGHT AND RIGHT DOWN HEAVY
2533 HEAVY DOWN AND HORIZONTAL
2534 LIGHT UP AND HORIZONTAL
2535 LEFT HEAVY AND RIGHT UP LIGHT
2536 RIGHT HEAVY AND LEFT UP LIGHT
2537 UP LIGHT AND HORIZONTAL HEAVY
2538 UP HEAVY AND HORIZONTAL LIGHT
2539 RIGHT LIGHT AND LEFT UP HEAVY
253A LEFT LIGHT AND RIGHT UP HEAVY
253B HEAVY UP AND HORIZONTAL
253C LIGHT VERTICAL AND HORIZONTAL
253D LEFT HEAVY AND RIGHT VERTICAL LIGHT
253E RIGHT HEAVY AND LEFT VERTICAL LIGHT
253F VERTICAL LIGHT AND HORIZONTAL HEAVY
2540 UP HEAVY AND DOWN HORIZONTAL LIGHT
2541 DOWN HEAVY AND UP HORIZONTAL LIGHT
2542 VERTICAL HEAVY AND HORIZONTAL LIGHT
2543 LEFT UP HEAVY AND RIGHT DOWN LIGHT
2544 RIGHT UP HEAVY AND LEFT DOWN LIGHT
2545 LEFT DOWN HEAVY AND RIGHT UP LIGHT
2546 RIGHT DOWN HEAVY AND LEFT UP LIGHT
2547 DOWN LIGHT AND UP HORIZONTAL HEAVY
2548 UP LIGHT AND DOWN HORIZONTAL HEAVY
2549 RIGHT LIGHT AND LEFT VERTICAL HEAVY
254A LEFT LIGHT AND RIGHT VERTICAL HEAVY
254B HEAVY VERTICAL AND HORIZONTAL
2550 DOUBLE HORIZONTAL
2551 DOUBLE VERTICAL
2552 DOWN SINGLE AND RIGHT DOUBLE
2553 DOWN DOUBLE AND RIGHT SINGLE
2554 DOUBLE DOWN AND RIGHT
2555 DOWN SINGLE AND LEFT DOUBLE
2556 DOWN DOUBLE AND LEFT SINGLE
2557 DOUBLE DOWN AND LEFT
2558 UP SINGLE AND RIGHT DOUBLE
2559 UP DOUBLE AND RIGHT SINGLE
255A DOUBLE UP AND RIGHT
255B UP SINGLE AND LEFT DOUBLE
255C UP DOUBLE AND LEFT SINGLE
255D DOUBLE UP AND LEFT
255E VERTICAL SINGLE AND RIGHT DOUBLE
255F VERTICAL DOUBLE AND RIGHT SINGLE
2560 DOUBLE VERTICAL AND RIGHT
2561 VERTICAL SINGLE AND LEFT DOUBLE
2562 VERTICAL DOUBLE AND LEFT SINGLE
2563 DOUBLE VERTICAL AND LEFT
2564 DOWN SINGLE AND HORIZONTAL DOUBLE
2565 DOWN DOUBLE AND HORIZONTAL SINGLE
2566 DOUBLE DOWN AND HORIZONTAL
2567 UP SINGLE AND HORIZONTAL DOUBLE
2568 UP DOUBLE AND HORIZONTAL SINGLE
2569 DOUBLE UP AND HORIZONTAL
256A VERTICAL SINGLE AND HORIZONTAL DOUBLE
256B VERTICAL DOUBLE AND HORIZONTAL SINGLE
256C DOUBLE VERTICAL AND HORIZONTAL
256D LIGHT ARC DOWN AND RIGHT
256E LIGHT ARC DOWN AND LEFT
256F LIGHT ARC UP AND LEFT
2570 LIGHT ARC UP AND RIGHT
2574 LIGHT LEFT
2575 LIGHT UP
2576 LIGHT RIGHT
2577 LIGHT DOWN
2578 HEAVY LEFT
2579 HEAVY UP
257A HEAVY RIGHT
257B HEAVY DOWN
257C LIGHT LEFT AND HEAVY RIGHT
257D LIGHT UP AND HEAVY DOWN
257E HEAVY LEFT AND LIGHT RIGHT
257F HEAVY UP AND LIGHT DOWN";

/// A glyph's arms as its name says: [up, right, down, left], with 0 none, 1 light,
/// 2 heavy, 3 double. A clause's weight comes from the first clause if it has none.
fn arms_from_name(name: &str) -> [u8; 4] {
    let weight_of = |w: &str| match w {
        "LIGHT" | "SINGLE" => Some(1),
        "HEAVY" => Some(2),
        "DOUBLE" => Some(3),
        _ => None,
    };
    let mut arms = [0; 4];
    let mut first = 0;
    for clause in name.split(" AND ") {
        let weight = clause.split(' ').find_map(weight_of).unwrap_or(first);
        if first == 0 {
            first = weight;
        }
        for word in clause.split(' ') {
            let on: &[usize] = match word {
                "UP" => &[0],
                "RIGHT" => &[1],
                "DOWN" => &[2],
                "LEFT" => &[3],
                "VERTICAL" => &[0, 2],
                "HORIZONTAL" => &[1, 3],
                _ => &[],
            };
            for &a in on {
                arms[a] = weight;
            }
        }
    }
    arms
}

/// Which positions across `n` an arm of `weight` lights at the cell's edge.
fn expected_edge(n: i32, t: i32, weight: u8) -> Vec<i32> {
    let thick = |k: i32| ((n - k) / 2..(n - k) / 2 + k).collect::<Vec<_>>();
    match weight {
        0 => vec![],
        1 => thick(t),
        2 => thick(2 * t),
        _ => {
            let a = (n - 3 * t) / 2;
            (a..a + t).chain(a + 2 * t..a + 3 * t).collect()
        }
    }
}

const SIZES: [(u32, u32, u32); 5] = [
    (19, 40, 2),
    (9, 20, 1),
    (20, 41, 3),
    (10, 21, 1),
    (8, 16, 1),
];

#[test]
fn every_box_glyph_meets_its_neighbour_at_each_edge_as_its_name_says() {
    // Two glyphs with the same arm weight on the edge they share light the same
    // pixels there, so this is the join, for every pair in the block at once.
    for (w, h, t) in SIZES {
        let (wi, hi, ti) = (w as i32, h as i32, t as i32);
        for line in NAMES.lines() {
            let (hex, name) = line.split_once(' ').unwrap();
            let ch = char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap();
            let [up, right, down, left] = arms_from_name(name);
            let px = draw(ch, w, h, t).unwrap();
            let lit = |x: i32, y: i32| px[(y * wi + x) as usize] == 255;
            let col = |x: i32| -> Vec<i32> { (0..hi).filter(|&y| lit(x, y)).collect::<Vec<_>>() };
            let row = |y: i32| -> Vec<i32> { (0..wi).filter(|&x| lit(x, y)).collect::<Vec<_>>() };
            let at = format!("{hex} {name} at {w}x{h}");
            assert_eq!(row(0), expected_edge(wi, ti, up), "top of {at}");
            assert_eq!(col(wi - 1), expected_edge(hi, ti, right), "right of {at}");
            assert_eq!(row(hi - 1), expected_edge(wi, ti, down), "bottom of {at}");
            assert_eq!(col(0), expected_edge(hi, ti, left), "left of {at}");
        }
    }
}

/// The glyph at 9×9 with 1 px lines, where a line is one column and a double line's
/// rails are columns 3 and 5, as text.
fn art(ch: char) -> String {
    let px = draw(ch, 9, 9, 1).unwrap();
    px.chunks(9)
        .map(|r| {
            r.iter()
                .map(|&v| if v == 255 { '#' } else { '.' })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_art(ch: char, want: &[&str; 9]) {
    assert_eq!(art(ch), want.join("\n"), "{ch}");
}

#[test]
fn light_lines_cross_and_turn_square() {
    #[rustfmt::skip]
    assert_art('┼', &[
        "....#....",
        "....#....",
        "....#....",
        "....#....",
        "#########",
        "....#....",
        "....#....",
        "....#....",
        "....#....",
    ]);
    #[rustfmt::skip]
    assert_art('┌', &[
        ".........",
        ".........",
        ".........",
        ".........",
        "....#####",
        "....#....",
        "....#....",
        "....#....",
        "....#....",
    ]);
}

#[test]
fn heavy_arms_reach_across_a_light_line_and_a_half_line_stops_at_the_center() {
    #[rustfmt::skip]
    assert_art('┝', &[
        "....#....",
        "....#....",
        "....#....",
        "....#####",
        "....#####",
        "....#....",
        "....#....",
        "....#....",
        "....#....",
    ]);
    #[rustfmt::skip]
    assert_art('╴', &[
        ".........",
        ".........",
        ".........",
        ".........",
        "####.....",
        ".........",
        ".........",
        ".........",
        ".........",
    ]);
}

#[test]
fn opposite_half_lines_meet_at_the_center_and_make_the_whole_line() {
    for (a, b, whole) in [
        ('╴', '╶', '─'),
        ('╵', '╷', '│'),
        ('╸', '╺', '━'),
        ('╹', '╻', '┃'),
    ] {
        for (w, h, t) in SIZES {
            let (pa, pb) = (draw(a, w, h, t).unwrap(), draw(b, w, h, t).unwrap());
            let line = draw(whole, w, h, t).unwrap();
            for i in 0..pa.len() {
                assert!(pa[i] == 0 || pb[i] == 0, "{a} and {b} overlap at {w}x{h}");
                assert_eq!(pa[i].max(pb[i]), line[i], "{a}{b} and {whole} at {w}x{h}");
            }
        }
    }
}

#[test]
fn double_lines_leave_their_joins_open() {
    #[rustfmt::skip]
    assert_art('╬', &[
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "####.####",
        ".........",
        "####.####",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
    #[rustfmt::skip]
    assert_art('╦', &[
        ".........",
        ".........",
        ".........",
        "#########",
        ".........",
        "####.####",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
    #[rustfmt::skip]
    assert_art('╔', &[
        ".........",
        ".........",
        ".........",
        "...######",
        "...#.....",
        "...#.####",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
    #[rustfmt::skip]
    assert_art('╠', &[
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "...#.####",
        "...#.....",
        "...#.####",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
}

#[test]
fn a_light_line_joins_a_double_one_at_the_rail_it_meets() {
    #[rustfmt::skip]
    assert_art('╒', &[
        ".........",
        ".........",
        ".........",
        "....#####",
        "....#....",
        "....#####",
        "....#....",
        "....#....",
        "....#....",
    ]);
    #[rustfmt::skip]
    assert_art('╓', &[
        ".........",
        ".........",
        ".........",
        ".........",
        "...######",
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
    #[rustfmt::skip]
    assert_art('╤', &[
        ".........",
        ".........",
        ".........",
        "#########",
        ".........",
        "#########",
        "....#....",
        "....#....",
        "....#....",
    ]);
    #[rustfmt::skip]
    assert_art('╧', &[
        "....#....",
        "....#....",
        "....#....",
        "#########",
        ".........",
        "#########",
        ".........",
        ".........",
        ".........",
    ]);
    #[rustfmt::skip]
    assert_art('╪', &[
        "....#....",
        "....#....",
        "....#....",
        "#########",
        "....#....",
        "#########",
        "....#....",
        "....#....",
        "....#....",
    ]);
    #[rustfmt::skip]
    assert_art('╫', &[
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "#########",
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
    #[rustfmt::skip]
    assert_art('╢', &[
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "####.#...",
        "...#.#...",
        "...#.#...",
        "...#.#...",
        "...#.#...",
    ]);
}

#[test]
fn arcs_round_the_corner() {
    let (w, h) = (19, 40);
    let px = draw('╭', w, h, 2).unwrap();
    let at = |x: u32, y: u32| px[(y * w + x) as usize];
    // A square corner would light the place where the lines would meet.
    assert_eq!(at(8, 19), 0, "no square corner");
    assert_eq!(at(9, 20), 0, "no square corner");
    // The curve passes the diagonal between the lines' ends.
    let on_curve = (9..19).any(|d| at(d, 20 + d - 9 + 1) > 128 || at(d + 1, 20 + d - 9) > 128);
    assert!(on_curve, "a curve between the arms");
    // The other three are its mirror images.
    let mirror_x: Vec<u8> = draw('╮', 20, 40, 2).unwrap();
    let base: Vec<u8> = draw('╭', 20, 40, 2).unwrap();
    for y in 0..40 {
        for x in 0..20 {
            let (a, b) = (
                base[(y * 20 + x) as usize],
                mirror_x[(y * 20 + (19 - x)) as usize],
            );
            assert!(a.abs_diff(b) <= 16, "╮ mirrors ╭ at {x},{y}: {a} and {b}");
        }
    }
}

#[test]
fn dashed_lines_have_their_count_of_dashes_and_even_gaps() {
    let runs = |v: &[bool]| {
        let mut n = 0;
        let mut was = false;
        for &on in v {
            n += usize::from(on && !was);
            was = on;
        }
        n
    };
    for (ch, n, thick) in [
        ('┄', 3, 2),
        ('┅', 3, 4),
        ('┈', 4, 2),
        ('┉', 4, 4),
        ('╌', 2, 2),
        ('╍', 2, 4),
    ] {
        let px = draw(ch, 19, 40, 2).unwrap();
        let line_row = (40 - thick) / 2;
        let row: Vec<bool> = (0..19).map(|x| px[line_row * 19 + x] == 255).collect();
        assert_eq!(runs(&row), n, "{ch}");
        assert!(
            !row[0] && !row[18],
            "{ch} leaves a gap at each edge, for an even join"
        );
        let lit: usize = px.iter().filter(|&&v| v == 255).count();
        assert_eq!(lit % thick, 0, "{ch} is {thick} px thick");
    }
    for (ch, n) in [('┆', 3), ('┊', 4), ('╎', 2)] {
        let px = draw(ch, 19, 40, 2).unwrap();
        let col: Vec<bool> = (0..40).map(|y| px[y * 19 + 8] == 255).collect();
        assert_eq!(runs(&col), n, "{ch}");
        assert!(!col[0] && !col[39], "{ch}");
    }
}

#[test]
fn diagonals_join_at_the_corners_and_the_cross_is_both() {
    let (w, h) = (19usize, 40usize);
    let rising = draw('╱', 19, 40, 2).unwrap();
    let falling = draw('╲', 19, 40, 2).unwrap();
    let cross = draw('╳', 19, 40, 2).unwrap();
    assert!(rising[(h - 1) * w] > 128, "╱ starts at the bottom left");
    assert!(rising[w - 1] > 128, "╱ ends at the top right");
    assert_eq!(rising[0], 0);
    assert!(falling[0] > 128, "╲ starts at the top left");
    assert!(falling[h * w - 1] > 128, "╲ ends at the bottom right");
    assert_eq!(falling[w - 1], 0);
    for i in 0..w * h {
        let (a, b) = (rising[i], falling[i]);
        assert!(cross[i] >= a.max(b), "the cross holds both, at {i}");
        assert_eq!(cross[i] == 0, a == 0 && b == 0, "and nothing else, at {i}");
    }
}

fn lit_rows(ch: char, w: u32, h: u32) -> Vec<(u32, u32)> {
    let px = draw(ch, w, h, 1).unwrap();
    let mut out = vec![];
    for y in 0..h {
        for x in 0..w {
            if px[(y * w + x) as usize] == 255 {
                out.push((x, y));
            }
        }
    }
    out
}

/// The two glyphs light every pixel of the cell, and none twice.
fn assert_partition(a: char, b: char, w: u32, h: u32) {
    let (la, lb) = (lit_rows(a, w, h), lit_rows(b, w, h));
    assert_eq!(
        la.len() + lb.len(),
        (w * h) as usize,
        "{a} and {b} at {w}x{h}"
    );
    assert!(
        !la.iter().any(|p| lb.contains(p)),
        "{a} and {b} overlap at {w}x{h}"
    );
}

#[test]
fn blocks_split_a_cell_at_one_place_even_at_an_odd_size() {
    for (w, h) in [(19, 40), (20, 41), (9, 21), (1, 1)] {
        assert_partition('▀', '▄', w, h);
        assert_partition('▌', '▐', w, h);
        for (a, b) in [('▖', '▜'), ('▗', '▛'), ('▘', '▟'), ('▝', '▙'), ('▚', '▞')]
        {
            assert_partition(a, b, w, h);
        }
    }
}

#[test]
fn eighths_grow_from_their_edge() {
    let (w, h) = (19, 40);
    let eighth = |n: u32| h * n / 8;
    for (i, ch) in "▁▂▃▄▅▆▇█".chars().enumerate() {
        let n = i as u32 + 1;
        let lit = lit_rows(ch, w, h);
        assert_eq!(lit.len() as u32, w * eighth(n), "{ch}");
        assert!(
            lit.iter().all(|&(_, y)| y >= h - eighth(n)),
            "{ch} sits at the bottom"
        );
    }
    for (i, ch) in "▉▊▋▌▍▎▏".chars().enumerate() {
        let n = 7 - i as u32;
        let width = (w * n + 4) / 8;
        let lit = lit_rows(ch, w, h);
        assert_eq!(lit.len() as u32, width * h, "{ch}");
        assert!(lit.iter().all(|&(x, _)| x < width), "{ch} sits at the left");
    }
    assert!(lit_rows('▔', w, h).iter().all(|&(_, y)| y < eighth(1)));
    assert!(lit_rows('▕', w, h).iter().all(|&(x, _)| x >= w - 2));
    assert_eq!(lit_rows('█', w, h).len() as u32, w * h);
}

#[test]
fn shades_are_even_all_over_so_they_tile() {
    for (ch, v) in [('░', 64), ('▒', 128), ('▓', 192)] {
        let px = draw(ch, 19, 40, 2).unwrap();
        assert!(px.iter().all(|&p| p == v), "{ch}");
    }
}

#[test]
fn braille_dots_sit_where_unicode_numbers_them() {
    let (w, h) = (20usize, 40usize);
    let dot_at = |ch: char, col: usize, row: usize| {
        let px = draw(ch, w as u32, h as u32, 2).unwrap();
        let (cx, cy) = (col * 2 + 1, row * 2 + 1);
        px[(cy * h / 8) * w + cx * w / 4] == 255
    };
    assert!(dot_at('\u{2801}', 0, 0), "dot 1");
    assert!(dot_at('\u{2802}', 0, 1), "dot 2");
    assert!(dot_at('\u{2804}', 0, 2), "dot 3");
    assert!(dot_at('\u{2808}', 1, 0), "dot 4");
    assert!(dot_at('\u{2810}', 1, 1), "dot 5");
    assert!(dot_at('\u{2820}', 1, 2), "dot 6");
    assert!(dot_at('\u{2840}', 0, 3), "dot 7");
    assert!(dot_at('\u{2880}', 1, 3), "dot 8");
    assert!(!dot_at('\u{2801}', 1, 0), "only dot 1");
    let blank = draw('\u{2800}', 20, 40, 2).unwrap();
    assert!(blank.iter().all(|&p| p == 0), "U+2800 has no dots");
}

#[test]
fn braille_dots_keep_inside_the_cell_at_an_even_pitch() {
    let (w, h) = (19usize, 40usize);
    let px = draw('⣿', 19, 40, 2).unwrap();
    assert!(
        (0..w).all(|x| px[x] == 0 && px[(h - 1) * w + x] == 0),
        "top and bottom rows"
    );
    assert!(
        (0..h).all(|y| px[y * w] == 0 && px[y * w + w - 1] == 0),
        "left and right columns"
    );
    // The rows of ink, with two cells stacked: the gap across the edge matches.
    let ink_rows: Vec<usize> = (0..h)
        .filter(|&y| px[y * w..(y + 1) * w].iter().any(|&p| p > 128))
        .collect();
    let mut centers = vec![];
    let mut start = ink_rows[0];
    for pair in ink_rows.windows(2) {
        if pair[1] != pair[0] + 1 {
            centers.push((start + pair[0]) as f32 / 2.0);
            start = pair[1];
        }
    }
    centers.push((start + ink_rows[ink_rows.len() - 1]) as f32 / 2.0);
    assert_eq!(centers.len(), 4, "four rows of dots");
    let inside = centers[1] - centers[0];
    let across = centers[0] + h as f32 - centers[3];
    assert!(
        (inside - across).abs() <= 1.0,
        "pitch {inside} inside, {across} across the edge"
    );
}

#[test]
fn powerline_shapes_span_the_cell_and_mirror() {
    let (w, h) = (19usize, 40usize);
    let right = draw('\u{E0B0}', 19, 40, 2).unwrap();
    let left = draw('\u{E0B2}', 19, 40, 2).unwrap();
    assert!(
        right[(h / 2) * w + w - 1] > 128,
        "the tip reaches the cell's far side"
    );
    assert!(
        right[0] > 0 && right[(h - 1) * w] > 0,
        "the base spans the cell's height"
    );
    assert!(left[(h / 2) * w] > 128);
    assert_eq!(right[w - 1], 0, "and stops short of the far corners");
    for y in 0..h {
        for x in 0..w {
            assert_eq!(
                right[y * w + x],
                left[y * w + w - 1 - x],
                "mirror at {x},{y}"
            );
        }
    }
    for ch in ['\u{E0B1}', '\u{E0B3}', '\u{E0B4}', '\u{E0B6}'] {
        assert!(
            draw(ch, 19, 40, 2).unwrap().iter().any(|&p| p > 128),
            "{ch:?}"
        );
    }
}

#[test]
fn a_character_the_renderer_does_not_draw_is_none() {
    for ch in ['a', '中', '\u{E0B5}', '\u{24FF}', '\u{2900}'] {
        assert_eq!(draw(ch, 19, 40, 2), None, "{ch:?}");
    }
}

fn rows_lit(px: &[u8], w: usize) -> Vec<usize> {
    px.chunks(w)
        .enumerate()
        .filter(|(_, r)| r.contains(&255))
        .map(|(y, _)| y)
        .collect()
}

#[test]
fn a_single_underline_and_the_strike_are_one_band_across_the_cell() {
    for l in [Line::Under(Underline::Single), Line::Strike] {
        let px = line(l, 19, 40, 34, 2);
        assert_eq!(rows_lit(&px, 19), [34, 35], "{l:?}");
        assert_eq!(px.iter().filter(|&&p| p == 255).count(), 2 * 19, "{l:?}");
    }
}

#[test]
fn a_double_underline_is_two_bands_with_a_gap_inside_the_cell() {
    assert_eq!(
        rows_lit(&line(Line::Under(Underline::Double), 19, 40, 30, 2), 19),
        [30, 31, 34, 35]
    );
    assert_eq!(
        rows_lit(&line(Line::Under(Underline::Double), 19, 40, 38, 2), 19),
        [34, 35, 38, 39],
        "kept inside the cell"
    );
}

#[test]
fn a_curly_underline_waves_and_joins_itself_at_the_cell_edge() {
    let (w, h) = (19usize, 40usize);
    let px = line(Line::Under(Underline::Curly), 19, 40, 34, 2);
    let center = |x: usize| {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in 0..h {
            let v = f32::from(px[y * w + x]);
            sum += v * y as f32;
            n += v;
        }
        assert!(n > 0.0, "ink in column {x}");
        sum / n
    };
    let centers: Vec<f32> = (0..w).map(center).collect();
    let high = centers.iter().copied().fold(f32::MAX, f32::min);
    let low = centers.iter().copied().fold(0.0, f32::max);
    assert!(low - high >= 3.0, "it waves: {high}..{low}");
    assert!(
        (centers[0] - centers[w - 1]).abs() <= 1.5,
        "and joins: {centers:?}"
    );
    // Near the cell's bottom it stays whole: its trough isn't cut off by the cell.
    let low = line(Line::Under(Underline::Curly), 19, 40, 38, 2);
    for x in 0..w {
        assert!((0..h).any(|y| low[y * w + x] > 0), "ink in column {x}");
    }
    let lowest = (0..h)
        .rev()
        .find(|&y| low[y * w..(y + 1) * w].iter().any(|&p| p > 128));
    let highest = (0..h).find(|&y| low[y * w..(y + 1) * w].iter().any(|&p| p > 128));
    assert!(
        lowest.unwrap() - highest.unwrap() >= 5,
        "and it still waves"
    );
}

#[test]
fn dotted_and_dashed_underlines_break_evenly_and_tile() {
    let gaps = |px: &[u8]| -> Vec<(usize, bool)> {
        let row = &px[34 * 19..35 * 19];
        let mut runs = vec![];
        for &p in row {
            let on = p == 255;
            match runs.last_mut() {
                Some((n, was)) if *was == on => *n += 1,
                _ => runs.push((1usize, on)),
            }
        }
        runs
    };
    let dotted = gaps(&line(Line::Under(Underline::Dotted), 19, 40, 34, 2));
    assert!(dotted.iter().filter(|r| r.1).count() >= 3, "{dotted:?}");
    assert!(dotted.iter().all(|r| r.0 <= 3), "{dotted:?}");
    let dashed = gaps(&line(Line::Under(Underline::Dashed), 19, 40, 34, 2));
    assert_eq!(dashed.len(), 3, "dash, gap, dash: {dashed:?}");
    assert!(
        dashed[0].1 && !dashed[1].1 && dashed[2].1,
        "the dash runs across the edge: {dashed:?}"
    );
}

#[test]
fn a_line_past_the_bottom_stays_in_the_cell() {
    let px = line(Line::Under(Underline::Single), 19, 40, 39, 2);
    assert_eq!(rows_lit(&px, 19), [38, 39]);
    let none = line(Line::Under(Underline::None), 19, 40, 34, 2);
    assert!(none.iter().all(|&p| p == 0));
}
