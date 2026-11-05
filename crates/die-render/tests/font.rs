//! The font policy, as data: no font is opened, so these run under `ci --linux`.

use die_test_cap as _;

use die_render::driver::Config;
use die_render::font::{
    CellMetrics, Draw, FaceMetrics, Fit, Ink, RangeRule, Source, chain, covering, fit, pins,
    source, styled_face,
};
use die_render::font_dir;
use die_render::frame::Plane;
use die_render::sprite::Line;
use std::path::PathBuf;

mod support;
use support::{cell2x, term_metrics};

#[test]
fn term_at_16_pt_on_a_2x_display_has_the_19_by_40_cell() {
    let c = cell2x();
    assert_eq!((c.w, c.h), (19, 40));
    assert_eq!(
        c.baseline, 30.8799,
        "the baseline is the ascent, as S1's was"
    );
    assert_eq!(c.thin, 2);
}

#[test]
fn an_advance_a_hair_under_a_whole_pixel_keeps_that_pixel() {
    let c = CellMetrics::new(term_metrics(), 18.9995, 2.0);
    assert_eq!(c.w, 19);
    let c = CellMetrics::new(term_metrics(), 18.99, 2.0);
    assert_eq!(c.w, 18);
}

#[test]
fn a_cell_has_some_size_however_small_the_font() {
    let tiny = FaceMetrics {
        ascent: 0.1,
        descent: 0.0,
        leading: 0.0,
        underline_position: 0.0,
        underline_thickness: 0.0,
        x_height: 0.0,
    };
    let c = CellMetrics::new(tiny, 0.1, 1.0);
    assert_eq!((c.w, c.h, c.thin), (1, 1, 1));
}

#[test]
fn the_underline_sits_below_the_baseline_and_the_strike_at_half_the_x_height() {
    let c = cell2x();
    // Center 3.2 below the baseline, 1.6 thick: the top is 2.4 below it.
    assert_eq!(c.underline_y, (30.8799_f64 + 2.4).round() as i32);
    // Half the x-height above the baseline, and half a line's thickness more.
    assert_eq!(c.strike_y, (30.8799_f64 - 8.5 - 1.0).round() as i32);
    let hugging = FaceMetrics {
        underline_position: -9.0,
        ..term_metrics()
    };
    let low = CellMetrics::new(hugging, 19.2, 2.0);
    assert_eq!(
        low.underline_y, 36,
        "two lines' room stays above the cell's bottom"
    );
}

#[test]
fn line_bitmaps_are_the_cell_and_take_their_line_from_the_metrics() {
    let c = cell2x();
    let strike = c.line_bitmap(Line::Strike);
    assert_eq!(
        (strike.w, strike.h, strike.left, strike.top),
        (19, 40, 0, 0)
    );
    assert_eq!(strike.plane, Plane::Gray);
    let row = c.strike_y as usize;
    assert_eq!(strike.data[row * 19], 255);
    assert_eq!(strike.data[(row + 2) * 19], 0);
}

#[test]
fn logical_pixels_scale_to_whole_physical_ones() {
    let at = |scale: f64| CellMetrics::new(term_metrics(), 19.2, scale);
    assert_eq!(at(2.0).logical(2.0), 4);
    assert_eq!(at(1.0).logical(2.0), 2);
    assert_eq!(at(2.0).logical(1.0), 2);
    assert_eq!(at(1.0).logical(1.0), 1);
    assert_eq!(at(1.5).logical(1.0), 2, "rounded to stay crisp");
    assert_eq!(at(0.25).logical(1.0), 1, "at least one");
}

fn ink(x: f64, y: f64, w: f64, h: f64) -> Ink {
    Ink { x, y, w, h }
}

#[test]
fn ink_with_nothing_in_it_has_no_place() {
    let c = cell2x();
    assert_eq!(fit(ink(0.0, 0.0, 0.0, 10.0), 19.2, 1, &c, false), None);
    assert_eq!(fit(ink(0.0, 0.0, 10.0, 0.0), 19.2, 1, &c, true), None);
}

#[test]
fn a_primary_glyph_stays_where_its_font_puts_it() {
    let c = cell2x();
    // 'M': a slightly wider advance than the cell isn't a reason to squeeze.
    let f = fit(ink(2.0, 0.0, 15.0, 22.0), 19.2, 1, &c, false).unwrap();
    assert_eq!(f.scale, 1.0);
    // The pen's on the cell's left edge, which is 1 px right of the bitmap's start
    // less the ink's offset.
    assert_eq!(f.left, 1);
    assert_eq!(f.pen_x, -1.0);
    // The bitmap's bottom edge, less the baseline: the pen is on the baseline.
    let top_of_cell_y = f64::from(f.top) + f64::from(f.h) - f.pen_y;
    assert!((top_of_cell_y - c.baseline).abs() < 1e-9, "{top_of_cell_y}");
}

/// Where the pen ends up from the cell's left edge.
fn pen(f: &Fit) -> f64 {
    f64::from(f.left) + f.pen_x
}

/// Where a fitted glyph's ink ends up across its span, from the cell's left edge.
fn across(f: &Fit, i: Ink) -> (f64, f64) {
    (pen(f) + i.x * f.scale, pen(f) + (i.x + i.w) * f.scale)
}

#[test]
fn a_primary_glyph_is_not_squeezed_for_the_pixel_rounding_took_from_its_cell() {
    // The cell is the advance rounded down, and these are sizes R3's scale rules reach:
    // 16 pt at 1x, then 14, 13 and 18 pt at 2x, then the 16 pt at 2x you use.
    for (advance, w) in [(9.6, 9), (16.8, 16), (15.6, 15), (21.6, 21), (19.2, 19)] {
        let c = CellMetrics { w, ..cell2x() };
        let f = fit(ink(1.0, 0.0, advance - 2.0, 20.0), advance, 1, &c, false).unwrap();
        assert_eq!(f.scale, 1.0, "{advance} across {w}");
        assert_eq!(
            pen(&f),
            0.0,
            "{advance} across {w}: the pen stays at the left"
        );
    }
    let c = CellMetrics { w: 9, ..cell2x() };
    let at = |advance: f64| fit(ink(0.0, 0.0, 8.0, 20.0), advance, 1, &c, false).unwrap();
    assert_eq!(
        at(10.0).scale,
        1.0,
        "a whole pixel over is what rounding can lose"
    );
    assert!(at(10.1).scale < 1.0, "and past that it's too wide");
    let f = at(14.4);
    assert!(
        (f.scale - 9.0 / 14.4).abs() < 1e-9,
        "Term's shortest long glyph, at 1x"
    );
    // Per cell: 2 of 19.2 is 38.4 across 38, and it stays.
    let f = fit(ink(0.0, 0.0, 36.0, 20.0), 38.4, 2, &cell2x(), false).unwrap();
    assert_eq!(f.scale, 1.0);
}

#[test]
fn a_primary_glyph_narrower_than_its_two_cells_is_centered_across_them() {
    let c = cell2x();
    // What Term has for 288 wide characters: one cell's advance, in two cells.
    let f = fit(ink(2.0, 0.0, 15.0, 22.0), 19.2, 2, &c, false).unwrap();
    assert_eq!(f.scale, 1.0);
    assert!((pen(&f) - 9.4).abs() < 1e-9, "{f:?}");
    // One that fills both stays at the left, with no room to center in.
    let f = fit(ink(1.0, 0.0, 36.0, 22.0), 38.4, 2, &c, false).unwrap();
    assert_eq!(pen(&f), 0.0, "{f:?}");
    let f = fit(ink(1.0, 0.0, 15.0, 22.0), 19.2, 1, &c, false).unwrap();
    assert_eq!(pen(&f), 0.0, "and a narrow one is where its font puts it");
}

#[test]
fn a_fallback_glyph_is_squeezed_past_half_a_pixel_over_as_ever() {
    let c = cell2x();
    let f = fit(ink(0.0, 0.0, 10.0, 20.0), 19.4, 1, &c, true).unwrap();
    assert_eq!(f.scale, 1.0);
    let f = fit(ink(0.0, 0.0, 10.0, 20.0), 19.6, 1, &c, true).unwrap();
    assert!((f.scale - 19.0 / 19.6).abs() < 1e-9, "{f:?}");
}

#[test]
fn a_fallback_glyph_stays_in_its_span_whichever_side_its_ink_leaves_by() {
    let c = cell2x();
    // Wider than the cell, starting left of the pen, as a lone mark's ink can.
    let i = ink(-15.0, 0.0, 30.0, 10.0);
    let f = fit(i, 19.0, 1, &c, true).unwrap();
    assert!((f.scale - 19.0 / 30.0).abs() < 1e-9, "{f:?}");
    let (left, right) = across(&f, i);
    assert!(left >= -1e-9 && right <= 19.0 + 1e-9, "{left}..{right}");
    // Narrow enough to fit, so moved rather than squeezed: left of the span,
    let i = ink(-15.0, 0.0, 10.0, 10.0);
    let f = fit(i, 0.0, 1, &c, true).unwrap();
    assert_eq!(f.scale, 1.0);
    assert_eq!(across(&f, i), (0.0, 10.0));
    // and right of it, where centering by the advance alone would put it.
    let i = ink(5.0, 0.0, 16.0, 10.0);
    let f = fit(i, 12.0, 1, &c, true).unwrap();
    assert_eq!(f.scale, 1.0);
    assert_eq!(across(&f, i), (3.0, 19.0));
    // Ink already inside stays centered by its advance.
    let i = ink(2.0, 0.0, 10.0, 10.0);
    let f = fit(i, 14.0, 1, &c, true).unwrap();
    assert_eq!(across(&f, i), (4.5, 14.5));
    // So does ink up to half a pixel over, as an italic's can be.
    let i = ink(0.0, 0.0, 19.4, 10.0);
    let f = fit(i, 19.2, 1, &c, true).unwrap();
    assert_eq!((f.scale, across(&f, i)), (1.0, (0.0, 19.4)));
    let i = ink(-0.4, 0.0, 10.0, 10.0);
    let f = fit(i, 19.0, 1, &c, true).unwrap();
    assert_eq!(across(&f, i), (-0.4, 9.6));
}

#[test]
fn a_primary_glyph_too_wide_for_its_cell_is_squeezed_and_centered() {
    let c = cell2x();
    // Term's U+27F5, about 1.9 cells.
    let f = fit(ink(1.0, 5.0, 36.0, 10.0), 38.0, 1, &c, false).unwrap();
    assert!((f.scale - 0.5).abs() < 1e-9, "{}", f.scale);
    let x0 = f64::from(f.left) + f.pen_x + 1.0 * f.scale;
    let x1 = x0 + 36.0 * f.scale;
    assert!(x0 >= 0.0 && x1 <= 19.0, "{x0}..{x1} is in the cell");
}

#[test]
fn a_squeezed_primary_glyph_whose_ink_overruns_its_advance_is_centered_by_its_advance() {
    let c = cell2x();
    let f = fit(ink(0.0, 0.0, 40.0, 10.0), 38.0, 1, &c, false).unwrap();
    let scale = 19.0 / 40.0;
    assert!((f.scale - scale).abs() < 1e-9);
    let pen = f64::from(f.left) + f.pen_x;
    assert!((pen - (19.0 - 38.0 * scale) / 2.0).abs() < 1e-9, "{pen}");
}

#[test]
fn a_fallback_glyph_is_centered_across_its_span_and_not_squeezed_if_it_fits() {
    let c = cell2x();
    // 中 from PingFang: 32 px across 38.
    let f = fit(ink(2.0, -3.0, 28.0, 30.0), 32.0, 2, &c, true).unwrap();
    assert_eq!(f.scale, 1.0);
    let pen = f64::from(f.left) + f.pen_x;
    assert_eq!(pen, 3.0);
}

#[test]
fn a_fallback_glyph_wider_than_its_span_is_squeezed_by_its_ink_or_advance() {
    let c = cell2x();
    let f = fit(ink(0.0, 0.0, 40.0, 30.0), 40.0, 2, &c, true).unwrap();
    assert!((f.scale - 38.0 / 40.0).abs() < 1e-9);
    // Its ink overruns though its advance doesn't.
    let f = fit(
        ink(0.0, 0.0, 19.0, 10.0),
        10.0,
        1,
        &CellMetrics { w: 12, ..c },
        true,
    )
    .unwrap();
    assert!((f.scale - 12.0 / 19.0).abs() < 1e-9);
}

#[test]
fn a_fallback_glyph_taller_than_the_cell_is_squeezed_and_stays_in_it() {
    let c = cell2x();
    let f = fit(ink(1.0, -10.0, 10.0, 80.0), 12.0, 1, &c, true).unwrap();
    assert!((f.scale - 0.5).abs() < 1e-9);
    assert!(
        f.top >= -1 && f64::from(f.top) + f64::from(f.h) <= 41.0,
        "{f:?}"
    );
}

#[test]
fn a_fallback_glyph_that_would_leave_the_cell_moves_to_its_middle() {
    let c = cell2x();
    // 38 px tall, starting 20 under the baseline: it would run off the bottom.
    let f = fit(ink(0.0, -20.0, 10.0, 38.0), 12.0, 1, &c, true).unwrap();
    assert_eq!(f.scale, 1.0);
    let middle = f64::from(f.top) + f64::from(f.h) / 2.0;
    assert!((middle - 20.0).abs() <= 1.0, "{f:?}");
}

#[test]
fn a_primary_glyph_may_leave_the_cell_upward_or_downward() {
    // A stack of marks from the primary font reaches into the row above.
    let c = cell2x();
    let f = fit(ink(2.0, 0.0, 15.0, 45.0), 19.2, 1, &c, false).unwrap();
    assert_eq!(f.scale, 1.0);
    assert!(f.top < 0);
}

fn holds_ink(f: &Fit, i: Ink) {
    let (left, right) = (f.pen_x + i.x * f.scale, f.pen_x + (i.x + i.w) * f.scale);
    let (bottom, top) = (f.pen_y + i.y * f.scale, f.pen_y + (i.y + i.h) * f.scale);
    assert!(
        left >= 1.0 - 1e-9 && right <= f64::from(f.w) - 1.0 + 1e-9,
        "{f:?} {i:?}"
    );
    assert!(
        bottom >= 1.0 - 1e-9 && top <= f64::from(f.h) - 1.0 + 1e-9,
        "{f:?} {i:?}"
    );
}

#[test]
fn the_bitmap_holds_the_ink_with_a_pixel_to_spare() {
    let c = cell2x();
    let mut rng = support::Rng(0x9E37_79B9_7F4A_7C15);
    let mut r = |n: usize| rng.below(n) as f64 / 4.0;
    for _ in 0..2000 {
        let i = ink(r(40) - 5.0, r(60) - 10.0, r(160) + 0.25, r(160) + 0.25);
        let (advance, span, fallback) = (r(200), 1 + r(8) as u32 % 2, r(8) >= 1.0);
        if let Some(f) = fit(i, advance, span, &c, fallback) {
            holds_ink(&f, i);
        }
    }
}

#[test]
fn a_union_of_ink_covers_both_and_ignores_empty_ink() {
    let a = ink(1.0, 2.0, 3.0, 4.0);
    let b = ink(-1.0, 5.0, 3.0, 4.0);
    assert_eq!(a.union(b), ink(-1.0, 2.0, 5.0, 7.0));
    assert_eq!(Ink::default().union(a), a);
    assert_eq!(a.union(Ink::default()), a);
}

#[test]
fn by_default_the_renderer_draws_boxes_blocks_and_powerline_and_the_font_draws_braille() {
    let rules = RangeRule::defaults();
    let of = |c: char| source(&rules, c.encode_utf8(&mut [0; 4]));
    for c in ['─', '╬', '█', '▟', '\u{E0B0}', '\u{E0B6}'] {
        assert_eq!(of(c).sprite, Some(c), "{c:?}");
    }
    let braille = of('⣿');
    assert_eq!(braille.sprite, None);
    assert_eq!(rules[braille.rule.unwrap()].draw, Draw::Primary);
    for c in ['a', '中', '😀', '→'] {
        let none = Source {
            sprite: None,
            rule: None,
        };
        assert_eq!(of(c), none, "{c:?}");
    }
}

#[test]
fn the_first_rule_that_covers_a_character_decides_it() {
    let rules = vec![
        RangeRule::new(
            'a'..='f',
            Draw::Files(vec![PathBuf::from("/fonts/other.ttf")]),
        ),
        RangeRule::new('a'..='z', Draw::Renderer),
        RangeRule::new('a'..='z', Draw::Primary),
    ];
    assert_eq!(source(&rules, "c").rule, Some(0));
    assert_eq!(
        source(&rules, "c").sprite,
        None,
        "the first rule pins a file"
    );
    assert_eq!(source(&rules, "g").rule, Some(1));
    assert_eq!(
        source(&rules, "g").sprite,
        Some('g'),
        "the second gives it away"
    );
    assert_eq!(source(&rules, "A").rule, None);
}

#[test]
fn a_cluster_goes_by_its_first_character_and_only_a_lone_one_goes_to_the_renderer() {
    let rules = RangeRule::defaults();
    let boxed = source(&rules, "─");
    assert_eq!(boxed.sprite, Some('─'));
    let marked = source(&rules, "─\u{301}");
    assert_eq!(marked.sprite, None, "the renderer draws no marks");
    assert_eq!(marked.rule, boxed.rule, "and the same rule decides it");
    let after = source(&rules, "a─");
    assert_eq!((after.sprite, after.rule), (None, None));
    let empty = source(&rules, "");
    assert_eq!((empty.sprite, empty.rule), (None, None));
}

#[test]
fn each_rule_pins_its_own_files_in_order_after_the_faces_before_them() {
    let file = |name: &str| PathBuf::from(format!("/fonts/{name}.ttf"));
    let rules = vec![
        RangeRule::new('a'..='b', Draw::Files(vec![file("one"), file("two")])),
        RangeRule::new('c'..='d', Draw::Primary),
        RangeRule::new('e'..='f', Draw::Files(vec![file("three")])),
        RangeRule::new('g'..='h', Draw::Renderer),
        RangeRule::new('i'..='j', Draw::Files(vec![])),
    ];
    assert_eq!(rules[0].files(), [file("one"), file("two")]);
    assert_eq!(rules[1].files(), [] as [PathBuf; 0]);
    assert_eq!(rules[3].files(), [] as [PathBuf; 0]);
    let pinned = pins(&rules, 4);
    assert_eq!(pinned, [vec![4, 5], vec![], vec![6], vec![], vec![]]);
    assert_eq!(pins(&[], 4), [] as [Vec<usize>; 0]);
}

#[test]
fn faces_are_tried_pinned_then_styled_then_regular() {
    assert_eq!(styled_face(false, false), 0);
    assert_eq!(styled_face(true, false), 1);
    assert_eq!(styled_face(false, true), 2);
    assert_eq!(styled_face(true, true), 3);
    assert_eq!(chain(&[], 0), [0]);
    assert_eq!(chain(&[], 3), [3, 0]);
    assert_eq!(chain(&[4], 0), [4, 0]);
    assert_eq!(chain(&[5], 1), [5, 1, 0]);
    assert_eq!(chain(&[6, 4], 2), [6, 4, 2, 0], "in the rule's order");
}

#[test]
fn a_grid_is_its_cells_and_nothing_around_them() {
    let c = cell2x();
    assert_eq!(c.grid(159, 49), (159 * 19, 49 * 40));
    assert_eq!(c.grid(1, 1), (19, 40));
    assert_eq!(c.grid(0, 3), (0, 120));
}

#[test]
fn term_names_its_four_files_in_the_order_the_styles_use() {
    let c = Config::term(16.0, 2.0);
    assert_eq!((c.points, c.scale), (16.0, 2.0));
    assert_eq!(c.rules, RangeRule::defaults());
    let file = |s: &str| font_dir().join(format!("IoskeleyMonoTerm-{s}.ttf"));
    assert_eq!(c.faces[styled_face(false, false)], file("Regular"));
    assert_eq!(c.faces[styled_face(true, false)], file("Bold"));
    assert_eq!(c.faces[styled_face(false, true)], file("Italic"));
    assert_eq!(c.faces[styled_face(true, true)], file("BoldItalic"));
}

#[test]
fn a_cluster_stays_in_the_first_face_that_has_all_of_it() {
    // Face 1 has the base only, face 2 has both, and face 3 has the mark only.
    let has = |face: usize, c: char| match c {
        'a' => face <= 2,
        _ => face >= 2,
    };
    assert_eq!(covering(&[1, 2, 3], "a\u{301}", has), Some(2));
    assert_eq!(
        covering(&[1, 3], "a\u{301}", has),
        None,
        "no face has it whole"
    );
    assert_eq!(covering(&[1, 2], "a", has), Some(1), "the order decides");
    assert_eq!(covering(&[], "a", has), None);
}
