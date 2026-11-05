//! The row cache, diff and instance building, as data: scenes in, instances out,
//! with a stand-in for the faces. These run under `cargo xtask ci --linux`.

use die_core::{Cell, Cursor, CursorShape, Glyph, Rgb, Scene, Underline};
use die_render::atlas::GlyphKey;
use die_render::font::{CellMetrics, FaceMetrics};
use die_render::frame::{GlyphBitmap, Plane, Quad, kind, rgba};
use die_render::rows::Frames;
use die_render::sprite::Line;
use std::mem::{offset_of, size_of};

mod support;
use support::*;

const ATLAS: u32 = 1024;

fn frames() -> Frames {
    Frames::new(cell2x(), ATLAS, ATLAS)
}

/// Rows rebuilt by a frame of `scene`.
fn rebuilt(f: &mut Frames, scene: &Scene) -> Vec<usize> {
    frame(f, scene);
    f.rebuilt().to_vec()
}

fn of_kind(quads: &[Quad], k: u32) -> Vec<Quad> {
    quads.iter().filter(|q| q.kind == k).copied().collect()
}

#[test]
fn an_instance_is_48_bytes_with_its_fields_where_the_shader_reads_them() {
    assert_eq!(size_of::<Quad>(), 48);
    assert_eq!(offset_of!(Quad, rect), 0);
    assert_eq!(offset_of!(Quad, uv), 16);
    assert_eq!(offset_of!(Quad, color), 32);
    assert_eq!(offset_of!(Quad, kind), 36);
    assert_eq!(offset_of!(Quad, pad), 40);
}

#[test]
fn colors_are_rgba_in_a_u32() {
    assert_eq!(rgba(Rgb(0x12, 0x34, 0x56)), 0x123456FF);
}

#[test]
fn the_first_frame_builds_every_row() {
    let mut f = frames();
    assert_eq!(rebuilt(&mut f, &blank(4, 3)), [0, 1, 2]);
}

#[test]
fn an_unchanged_frame_rebuilds_nothing_and_wants_no_glyph() {
    let mut f = frames();
    let mut s = blank(10, 3);
    put(&mut s, 1, 0, "hello");
    let (first, ..) = frame(&mut f, &s);
    assert!(f.diff(&s).is_empty(), "every glyph is in the atlas");
    let (again, _, uploads) = frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [] as [usize; 0]);
    assert_eq!(first, again, "and the same instances");
    assert!(uploads.is_empty());
}

#[test]
fn a_changed_cell_rebuilds_only_its_row() {
    let mut f = frames();
    let mut s = blank(10, 5);
    frame(&mut f, &s);
    put(&mut s, 3, 2, "x");
    assert_eq!(rebuilt(&mut f, &s), [3]);
}

#[test]
fn a_change_to_any_field_of_a_cell_rebuilds_its_row() {
    let changes: [fn(&mut Cell); 8] = [
        |c| c.glyph = Glyph::Char('y'),
        |c| c.fg = RED,
        |c| c.bg = RED,
        |c| c.bold = true,
        |c| c.italic = true,
        |c| c.strike = true,
        |c| c.underline = Underline::Curly,
        |c| c.underline_color = RED,
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut f = frames();
        let mut s = blank(6, 3);
        put(&mut s, 1, 2, "x");
        frame(&mut f, &s);
        change(&mut s.cells[6 + 2]);
        assert_eq!(rebuilt(&mut f, &s), [1], "change {i}");
    }
}

#[test]
fn moving_a_cursor_rebuilds_no_row_and_the_cursor_moves() {
    let mut f = frames();
    let mut s = blank(6, 3);
    s.cursors = vec![cursor(0, 0, CursorShape::Bar)];
    let (a, ..) = frame(&mut f, &s);
    s.cursors = vec![cursor(2, 3, CursorShape::Bar)];
    let (b, ..) = frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [] as [usize; 0]);
    assert_ne!(a, b);
    assert_eq!(b[0].rect[..2], [3.0 * 19.0, 2.0 * 40.0]);
}

#[test]
fn a_new_size_rebuilds_every_row() {
    let mut f = frames();
    frame(&mut f, &blank(6, 3));
    assert_eq!(rebuilt(&mut f, &blank(6, 4)), [0, 1, 2, 3]);
    assert_eq!(rebuilt(&mut f, &blank(7, 4)), [0, 1, 2, 3]);
    assert_eq!(rebuilt(&mut f, &blank(3, 2)), [0, 1]);
}

#[test]
fn a_new_size_with_the_same_cell_count_is_a_new_grid() {
    let mut f = frames();
    let mut wide = blank(6, 2);
    put(&mut wide, 1, 4, "z");
    frame(&mut f, &wide);
    let mut tall = blank(3, 4);
    put(&mut tall, 2, 1, "z");
    let (quads, ..) = frame(&mut f, &tall);
    let glyph = of_kind(&quads, kind::GRAY);
    assert_eq!(glyph.len(), 1);
    assert_eq!(glyph[0].rect[..2], [19.0 + 1.0, 2.0 * 40.0 + 2.0]);
}

#[test]
fn a_new_background_rebuilds_every_row_since_a_cell_in_it_has_no_instance() {
    let mut f = frames();
    let mut s = blank(4, 2);
    frame(&mut f, &s);
    assert!(
        of_kind(&f_quads(&mut f, &s), kind::SOLID).is_empty(),
        "cells in the background's color"
    );
    s.background = GREEN;
    let (quads, clear, _) = frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [0, 1]);
    assert_eq!(clear, GREEN);
    let bg = of_kind(&quads, kind::SOLID);
    assert_eq!(
        bg.len(),
        2,
        "those cells aren't in the background's color now"
    );
    assert!(bg.iter().all(|q| q.color == rgba(BG)));
}

fn f_quads(f: &mut Frames, s: &Scene) -> Vec<Quad> {
    frame(f, s).0
}

#[test]
fn a_run_of_one_background_is_one_quad_and_the_scene_background_has_none() {
    let mut f = frames();
    let mut s = blank(10, 1);
    for col in 2..5 {
        s.cells[col].bg = RED;
    }
    s.cells[5].bg = BLUE;
    s.cells[8].bg = RED;
    let quads = f_quads(&mut f, &s);
    let bg = of_kind(&quads, kind::SOLID);
    assert_eq!(bg.len(), 3);
    assert_eq!(bg[0].rect, [2.0 * 19.0, 0.0, 3.0 * 19.0, 40.0]);
    assert_eq!(bg[0].color, rgba(RED));
    assert_eq!(bg[1].rect, [5.0 * 19.0, 0.0, 19.0, 40.0]);
    assert_eq!(bg[1].color, rgba(BLUE));
    assert_eq!(bg[2].rect, [8.0 * 19.0, 0.0, 19.0, 40.0]);
}

#[test]
fn a_run_reaching_the_row_end_is_drawn_to_it() {
    let mut f = frames();
    let mut s = blank(4, 2);
    for col in 2..4 {
        s.cells[4 + col].bg = RED;
    }
    let bg = of_kind(&f_quads(&mut f, &s), kind::SOLID);
    assert_eq!(bg.len(), 1);
    assert_eq!(bg[0].rect, [2.0 * 19.0, 40.0, 2.0 * 19.0, 40.0]);
}

#[test]
fn every_background_comes_before_every_glyph_and_the_cursors_come_last() {
    let mut f = frames();
    let mut s = blank(6, 3);
    for r in 0..3 {
        put(&mut s, r, 0, "ab");
        s.cells[r * 6 + 3].bg = RED;
    }
    s.cursors = vec![cursor(0, 5, CursorShape::Bar)];
    let quads = f_quads(&mut f, &s);
    let kinds: Vec<u32> = quads.iter().map(|q| q.kind).collect();
    assert_eq!(&kinds[..3], [kind::SOLID; 3], "{kinds:?}");
    assert_eq!(&kinds[3..9], [kind::GRAY; 6], "{kinds:?}");
    assert_eq!(kinds[9], kind::SOLID, "{kinds:?}");
    assert_eq!(quads[9].color, rgba(RED), "the cursor");
    assert_eq!(quads.len(), 10);
}

#[test]
fn a_glyph_goes_where_its_slot_says_in_its_cell() {
    let mut f = frames();
    let mut s = blank(6, 3);
    put(&mut s, 2, 3, "q");
    let quads = f_quads(&mut f, &s);
    assert_eq!(quads.len(), 1);
    let q = quads[0];
    assert_eq!(q.kind, kind::GRAY);
    assert_eq!(q.color, rgba(FG));
    // The stand-in face's bitmap is 17×36 at (1, 2) in the cell.
    assert_eq!(q.rect, [3.0 * 19.0 + 1.0, 2.0 * 40.0 + 2.0, 17.0, 36.0]);
    assert_eq!(q.uv, [0.0, 0.0, 17.0, 36.0]);
}

#[test]
fn a_color_glyph_comes_from_the_color_plane() {
    let cell = cell2x();
    let mut f = Frames::new(cell, ATLAS, ATLAS);
    let mut s = blank(4, 1);
    put(&mut s, 0, 0, "😀");
    let wanted = f.diff(&s);
    assert_eq!(wanted.len(), 1);
    f.supply(
        &wanted[0],
        Some(GlyphBitmap {
            w: 2,
            h: 2,
            left: 0,
            top: 0,
            plane: Plane::Color,
            data: vec![255; 16],
        }),
    );
    assert_eq!(f.wanted(), []);
    let frame = f.build(&s);
    assert_eq!(frame.quads[0].kind, kind::COLOR);
    assert_eq!(frame.uploads[0].plane, Plane::Color);
}

#[test]
fn a_wide_glyph_is_fitted_to_its_cells_and_its_right_half_draws_nothing() {
    let mut f = frames();
    let mut s = blank(6, 1);
    put(&mut s, 0, 1, "中");
    let wanted = f.diff(&s);
    assert_eq!(wanted.len(), 1);
    assert!(
        matches!(wanted[0], GlyphKey::Text { span: 2, .. }),
        "{wanted:?}"
    );
    for key in &wanted {
        f.supply(key, fake_bitmap(key, &cell2x()));
    }
    let frame = f.build(&s);
    assert_eq!(frame.quads.len(), 1);
    assert_eq!(frame.quads[0].rect[0], 19.0 + 1.0);
}

#[test]
fn a_padding_cell_after_a_narrow_glyph_does_not_widen_it() {
    // A continuation with no wide character before it. Layout's own padding is a
    // space, so this comes only from another producer.
    let mut s = blank(3, 1);
    put(&mut s, 0, 1, "a");
    s.cells[2].glyph = Glyph::Continuation;
    let mut f = frames();
    let wanted = f.diff(&s);
    assert!(
        matches!(wanted[0], GlyphKey::Text { span: 1, .. }),
        "{wanted:?}"
    );
}

#[test]
fn a_wide_glyph_with_no_room_is_fitted_to_what_it_has() {
    let mut s = blank(3, 1);
    put(&mut s, 0, 2, "a");
    s.cells[2].glyph = Glyph::Char('中');
    let mut f = frames();
    let wanted = f.diff(&s);
    assert!(
        matches!(wanted[0], GlyphKey::Text { span: 1, .. }),
        "{wanted:?}"
    );
}

#[test]
fn a_cluster_keeps_its_marks_and_its_width() {
    let mut s = blank(5, 1);
    put_with(&mut s, 0, 0, &["e\u{301}", "👩\u{200D}💻"], |_| {});
    let mut f = frames();
    let wanted = f.diff(&s);
    assert_eq!(wanted.len(), 2);
    let want = |text: &str, n: u16| GlyphKey::Text {
        glyph: Glyph::Cluster(text.into()),
        bold: false,
        italic: false,
        span: n,
    };
    assert_eq!(wanted[0], want("e\u{301}", 1));
    assert_eq!(wanted[1], want("👩\u{200D}💻", 2));
}

#[test]
fn a_space_wants_no_glyph_but_its_lines_are_drawn() {
    let mut s = blank(4, 1);
    let mut f = frames();
    assert_eq!(f.diff(&s), []);
    s.cells[1].underline = Underline::Single;
    s.cells[2].strike = true;
    let mut f = frames();
    assert_eq!(
        f.diff(&s),
        [
            GlyphKey::Line(Line::Under(Underline::Single)),
            GlyphKey::Line(Line::Strike)
        ]
    );
}

#[test]
fn wanted_lists_each_glyph_once_and_bold_and_italic_apart() {
    let mut s = blank(8, 2);
    put(&mut s, 0, 0, "aa");
    put(&mut s, 1, 0, "a");
    s.cells[8 + 1] = {
        let mut c = Cell::new(Glyph::Char('a'), FG, BG);
        c.bold = true;
        c
    };
    s.cells[8 + 2] = {
        let mut c = Cell::new(Glyph::Char('a'), FG, BG);
        c.italic = true;
        c
    };
    let mut f = frames();
    assert_eq!(f.diff(&s).len(), 3);
}

#[test]
fn a_cell_lines_go_over_its_glyph_in_their_own_colors() {
    let mut f = frames();
    let mut s = blank(4, 1);
    put_with(&mut s, 0, 0, &["x"], |c| {
        c.fg = GREEN;
        c.underline = Underline::Double;
        c.underline_color = BLUE;
        c.strike = true;
    });
    let quads = f_quads(&mut f, &s);
    assert_eq!(quads.len(), 3);
    assert_eq!(quads[0].color, rgba(GREEN), "the glyph");
    assert_eq!(quads[1].color, rgba(BLUE), "the underline's own color");
    assert_eq!(
        quads[2].color,
        rgba(GREEN),
        "the strike takes the foreground"
    );
    assert_eq!(
        quads[1].rect,
        [0.0, 0.0, 19.0, 40.0],
        "a line covers its cell"
    );
}

#[test]
fn a_wide_glyph_lines_cover_both_of_its_cells() {
    let mut f = frames();
    let mut s = blank(4, 1);
    put_with(&mut s, 0, 0, &["中"], |c| c.underline = Underline::Single);
    let lines = |quads: &[Quad]| -> Vec<f32> {
        let lines = quads.iter().filter(|q| q.rect[2] == 19.0);
        lines.map(|q| q.rect[0]).collect()
    };
    assert_eq!(lines(&f_quads(&mut f, &s)), [0.0, 19.0], "an underline");
    let mut f = frames();
    let mut s = blank(4, 1);
    put_with(&mut s, 0, 0, &["中"], |c| c.strike = true);
    assert_eq!(lines(&f_quads(&mut f, &s)), [0.0, 19.0], "a strike-through");
}

/// A frame of a cursor on an "x" at (1, 1), and just its instances.
fn cursor_quads(cell: CellMetrics, shape: CursorShape) -> Vec<Quad> {
    let mut f = Frames::new(cell, ATLAS, ATLAS);
    let mut s = blank(3, 3);
    put(&mut s, 1, 1, "x");
    let without = frame(&mut f, &s).0.len();
    s.cursors = vec![cursor(1, 1, shape)];
    frame(&mut f, &s).0[without..].to_vec()
}

fn rects(quads: &[Quad]) -> Vec<[f32; 4]> {
    quads.iter().map(|q| q.rect).collect()
}

/// A cell of `CursorShape`'s pictures: 19×40, at (19, 40) in the grid.
const X: f32 = 19.0;
const Y: f32 = 40.0;

#[test]
fn a_block_cursor_fills_the_cell_and_redraws_the_glyph_in_the_cell_background() {
    let q = cursor_quads(cell2x(), CursorShape::Block);
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].kind, kind::SOLID);
    assert_eq!(q[0].rect, [X, Y, 19.0, 40.0]);
    assert_eq!(q[0].color, rgba(RED));
    assert_eq!(q[1].kind, kind::GRAY);
    assert_eq!(
        q[1].color,
        rgba(BG),
        "the glyph in the cell's background color"
    );
    assert_eq!(q[1].rect[..2], [X + 1.0, Y + 2.0], "where the glyph is");
}

#[test]
fn a_block_cursor_draws_its_cell_lines_in_the_cell_background_too() {
    let mut f = frames();
    let mut s = blank(3, 1);
    put_with(&mut s, 0, 0, &["x"], |c| {
        c.strike = true;
        c.underline = Underline::Single;
    });
    let without = frame(&mut f, &s).0.len();
    s.cursors = vec![cursor(0, 0, CursorShape::Block)];
    let (quads, ..) = frame(&mut f, &s);
    let q = &quads[without..];
    assert_eq!(q.len(), 4, "the fill, the glyph and two lines");
    assert!(q[1..].iter().all(|q| q.color == rgba(BG)));
}

#[test]
fn a_bar_cursor_is_2_logical_px_wide_at_the_cell_left_edge_over_the_glyph() {
    let q = cursor_quads(cell2x(), CursorShape::Bar);
    assert_eq!(rects(&q), [[X, Y, 4.0, 40.0]], "4 physical px at 2x");
    assert_eq!(q[0].color, rgba(RED));
    let q = cursor_quads(cell_at(1.0), CursorShape::Bar);
    assert_eq!(rects(&q), [[9.0, 20.0, 2.0, 20.0]], "2 physical px at 1x");
}

#[test]
fn an_underline_cursor_is_2_logical_px_high_at_the_cell_bottom_over_the_glyph() {
    let q = cursor_quads(cell2x(), CursorShape::Underline);
    assert_eq!(rects(&q), [[X, Y + 36.0, 19.0, 4.0]]);
    let q = cursor_quads(cell_at(1.0), CursorShape::Underline);
    assert_eq!(rects(&q), [[9.0, 38.0, 9.0, 2.0]]);
}

#[test]
fn a_half_block_cursor_fills_the_lower_half_over_the_glyph() {
    let q = cursor_quads(cell2x(), CursorShape::HalfBlock);
    assert_eq!(rects(&q), [[X, Y + 20.0, 19.0, 20.0]]);
    assert_eq!(q[0].kind, kind::SOLID, "it hides the glyph there");
    // Evil's half cursor is half the height rounded down, at the bottom.
    let odd = CellMetrics { h: 41, ..cell2x() };
    let q = cursor_quads(odd, CursorShape::HalfBlock);
    assert_eq!(rects(&q), [[X, 41.0 + 21.0, 19.0, 20.0]]);
}

#[test]
fn a_hollow_cursor_is_a_1_logical_px_outline_and_leaves_the_glyph() {
    let q = cursor_quads(cell2x(), CursorShape::Hollow);
    assert_eq!(
        rects(&q),
        [
            [X, Y, 19.0, 2.0],
            [X, Y + 38.0, 19.0, 2.0],
            [X, Y + 2.0, 2.0, 36.0],
            [X + 17.0, Y + 2.0, 2.0, 36.0],
        ]
    );
    let q = cursor_quads(cell_at(1.0), CursorShape::Hollow);
    assert_eq!(
        rects(&q),
        [
            [9.0, 20.0, 9.0, 1.0],
            [9.0, 39.0, 9.0, 1.0],
            [9.0, 21.0, 1.0, 18.0],
            [17.0, 21.0, 1.0, 18.0],
        ]
    );
}

#[test]
fn a_cursor_stays_in_a_cell_too_small_for_its_lines() {
    // 4 physical px of bar and 2 of outline at 2x, in a cell of less than that.
    let (x, y) = (1.0, 1.0);
    for (w, h) in [(1, 1), (3, 3), (1, 40), (19, 1)] {
        let cell = CellMetrics { w, h, ..cell2x() };
        for shape in [
            CursorShape::Block,
            CursorShape::Bar,
            CursorShape::Underline,
            CursorShape::HalfBlock,
            CursorShape::Hollow,
        ] {
            let mut f = Frames::new(cell, ATLAS, ATLAS);
            let mut s = blank(3, 3);
            let without = frame(&mut f, &s).0.len();
            s.cursors = vec![cursor(1, 1, shape)];
            let (quads, ..) = frame(&mut f, &s);
            let (w, h) = (w as f32, h as f32);
            for q in &quads[without..] {
                let [qx, qy, qw, qh] = q.rect;
                let inside = qx >= x * w
                    && qy >= y * h
                    && qx + qw <= (x + 1.0) * w
                    && qy + qh <= (y + 1.0) * h;
                assert!(inside, "{shape:?} in a {w}x{h} cell: {:?}", q.rect);
            }
        }
    }
}

/// Term's cell at another scale: half the pixels at 1x.
fn cell_at(scale: f64) -> CellMetrics {
    let m = term_metrics();
    let half = FaceMetrics {
        ascent: m.ascent / 2.0,
        descent: m.descent / 2.0,
        leading: m.leading / 2.0,
        underline_position: m.underline_position / 2.0,
        underline_thickness: m.underline_thickness / 2.0,
        x_height: m.x_height / 2.0,
    };
    CellMetrics::new(half, 9.6, scale)
}

#[test]
fn a_cursor_on_a_wide_character_covers_its_cells_and_on_its_right_half_takes_it_whole() {
    for col in [1, 2] {
        let mut f = frames();
        let mut s = blank(6, 1);
        put(&mut s, 0, 1, "中");
        let without = frame(&mut f, &s).0.len();
        s.cursors = vec![cursor(0, col, CursorShape::Block)];
        let (quads, ..) = frame(&mut f, &s);
        let q = &quads[without..];
        assert_eq!(q[0].rect, [19.0, 0.0, 38.0, 40.0], "at column {col}");
        assert_eq!(q[1].rect[0], 19.0 + 1.0, "the glyph, redrawn");
        s.cursors = vec![cursor(0, col, CursorShape::Bar)];
        let (quads, ..) = frame(&mut f, &s);
        assert_eq!(
            quads[without].rect,
            [19.0, 0.0, 4.0, 40.0],
            "a bar at its left edge"
        );
    }
}

/// A frame with a block cursor on an "x" at (1, 1) whose bitmap is 30×50 at this place
/// in its cell: the glyph's own instance, and the ones the cursor added.
fn block_on_a_big_glyph(left: i32, top: i32) -> (Quad, Vec<Quad>) {
    let mut f = frames();
    let mut s = blank(3, 3);
    put(&mut s, 1, 1, "x");
    let bitmap = GlyphBitmap {
        w: 30,
        h: 50,
        left,
        top,
        plane: Plane::Gray,
        data: vec![255; 1500],
    };
    for key in f.diff(&s) {
        f.supply(&key, Some(bitmap.clone()));
    }
    let glyph = f.build(&s).quads[0];
    s.cursors = vec![cursor(1, 1, CursorShape::Block)];
    f.diff(&s);
    let quads = f.build(&s).quads;
    (glyph, quads[1..].to_vec())
}

#[test]
fn a_block_cursor_cuts_its_redrawn_glyph_to_its_cell() {
    let (glyph, q) = block_on_a_big_glyph(-5, -3);
    assert_eq!(
        glyph.rect,
        [X - 5.0, Y - 3.0, 30.0, 50.0],
        "whole, without it"
    );
    assert_eq!(q[0].rect, [X, Y, 19.0, 40.0], "the fill");
    assert_eq!(q[1].rect, [X, Y, 19.0, 40.0], "the glyph, only in the cell");
    assert_eq!(q[1].uv, [5.0, 3.0, 19.0, 40.0], "and the same pixels of it");
    assert_eq!(q[1].color, rgba(BG));
    // Over two sides only: the left and top edges stay where the bitmap has them.
    let (_, q) = block_on_a_big_glyph(2, 4);
    assert_eq!(q[1].rect, [X + 2.0, Y + 4.0, 17.0, 36.0]);
    assert_eq!(q[1].uv, [0.0, 0.0, 17.0, 36.0]);
    // Ink all outside the cell leaves nothing to draw.
    let (_, q) = block_on_a_big_glyph(-40, 0);
    assert_eq!(q[1].rect[2], 0.0, "{:?}", q[1]);
}

#[test]
fn a_block_cursor_on_a_wide_character_redraws_the_lines_of_both_of_its_cells() {
    for col in [0, 1] {
        let mut f = frames();
        let mut s = blank(4, 1);
        put_with(&mut s, 0, 0, &["中"], |c| {
            c.underline = Underline::Single;
            c.strike = true;
        });
        let without = frame(&mut f, &s).0.len();
        s.cursors = vec![cursor(0, col, CursorShape::Block)];
        let (quads, ..) = frame(&mut f, &s);
        let q = &quads[without..];
        assert_eq!(
            q.len(),
            6,
            "the fill, the glyph, and two lines on each cell"
        );
        let lines: Vec<f32> = q[2..].iter().map(|q| q.rect[0]).collect();
        assert_eq!(lines, [0.0, 0.0, 19.0, 19.0], "at column {col}");
        assert!(
            q[1..].iter().all(|q| q.color == rgba(BG)),
            "at column {col}"
        );
    }
}

/// A frame's instances for a cursor in `RED` on an "x" in these colors, on a scene of
/// this background.
fn cursor_on(shape: CursorShape, fg: Rgb, bg: Rgb, background: Rgb) -> Vec<Quad> {
    let mut f = frames();
    let mut s = blank(3, 1);
    s.background = background;
    put_with(&mut s, 0, 1, &["x"], |c| {
        c.fg = fg;
        c.bg = bg;
    });
    let without = frame(&mut f, &s).0.len();
    s.cursors = vec![cursor(0, 1, shape)];
    frame(&mut f, &s).0[without..].to_vec()
}

const THIN_SHAPES: [CursorShape; 4] = [
    CursorShape::Bar,
    CursorShape::Underline,
    CursorShape::HalfBlock,
    CursorShape::Hollow,
];

#[test]
fn a_cursor_the_color_of_its_cell_stays_visible() {
    // As mono-dark's current search match has it: the cursor is the cell's background.
    let q = cursor_on(CursorShape::Block, GREEN, RED, BG);
    assert_eq!(q[0].kind, kind::SOLID);
    assert_eq!(q[0].color, rgba(GREEN), "filled with the cell's foreground");
    assert_eq!(q[1].kind, kind::GRAY);
    assert_eq!(
        q[1].color,
        rgba(RED),
        "and its glyph in the cell's background"
    );
    for shape in THIN_SHAPES {
        let q = cursor_on(shape, GREEN, RED, BG);
        assert!(!q.is_empty(), "{shape:?}");
        for quad in &q {
            assert_eq!(
                quad.color,
                rgba(GREEN),
                "{shape:?} in the cell's foreground"
            );
        }
    }
}

#[test]
fn a_cursor_of_another_color_keeps_it_on_a_cell_of_its_foreground() {
    // The usual case, as a cursor in the text's color on plain text is.
    let q = cursor_on(CursorShape::Block, RED, GREEN, BG);
    assert_eq!(q[0].color, rgba(RED));
    assert_eq!(q[1].color, rgba(GREEN));
    for shape in THIN_SHAPES {
        let q = cursor_on(shape, RED, GREEN, BG);
        assert!(q.iter().all(|quad| quad.color == rgba(RED)), "{shape:?}");
    }
}

#[test]
fn a_cursor_shows_on_a_cell_with_no_other_color_in_it() {
    // A cell the cursor's color all through takes the scene's background.
    let q = cursor_on(CursorShape::Block, RED, RED, BG);
    assert_eq!(q[0].color, rgba(BG));
    assert_eq!(q[1].color, rgba(RED), "and its glyph can be seen on that");
    for shape in THIN_SHAPES {
        let q = cursor_on(shape, RED, RED, BG);
        assert!(q.iter().all(|quad| quad.color == rgba(BG)), "{shape:?}");
    }
    // A frame the cursor's color all through takes black, or white on black.
    let q = cursor_on(CursorShape::Bar, RED, RED, RED);
    assert_eq!(q[0].color, rgba(Rgb(0, 0, 0)));
    let mut f = frames();
    let black = Rgb(0, 0, 0);
    let mut s = blank(3, 1);
    s.background = black;
    for c in &mut s.cells {
        *c = Cell::new(Glyph::Char(' '), black, black);
    }
    s.cursors = vec![Cursor {
        color: black,
        ..cursor(0, 1, CursorShape::Bar)
    }];
    assert_eq!(f_quads(&mut f, &s)[0].color, rgba(Rgb(255, 255, 255)));
}

#[test]
fn a_cursor_off_the_grid_draws_nothing() {
    let mut f = frames();
    let mut s = blank(3, 2);
    let without = frame(&mut f, &s).0.len();
    s.cursors = vec![
        cursor(2, 0, CursorShape::Block),
        cursor(0, 3, CursorShape::Block),
    ];
    assert_eq!(frame(&mut f, &s).0.len(), without);
}

#[test]
fn every_cursor_is_drawn_in_scene_order() {
    let mut f = frames();
    let mut s = blank(4, 2);
    let mut other = cursor(1, 2, CursorShape::Hollow);
    other.color = GREEN;
    s.cursors = vec![cursor(0, 0, CursorShape::Bar), other];
    let quads = f_quads(&mut f, &s);
    assert_eq!(quads.len(), 5);
    assert_eq!(quads[0].color, rgba(RED));
    assert!(quads[1..].iter().all(|q| q.color == rgba(GREEN)));
}

/// An atlas with room for 3 of the stand-in's 17×36 bitmaps, in one row.
fn small() -> Frames {
    Frames::new(cell2x(), 54, 8)
}

#[test]
fn a_full_atlas_starts_over_and_the_frame_still_has_every_glyph() {
    let mut f = small();
    let mut gpu = SoftGpu::new(54);
    let mut s = blank(3, 1);
    put(&mut s, 0, 0, "abc");
    paint(&mut f, &mut gpu, &s);
    put(&mut s, 0, 0, "def");
    let got = paint(&mut f, &mut gpu, &s);
    let want = paint(&mut small(), &mut SoftGpu::new(54), &s);
    assert!(
        got == want,
        "the new glyphs are where the old ones were, and in the texture"
    );
}

#[test]
fn starting_over_rebuilds_rows_that_did_not_change_since_their_glyphs_moved() {
    let mut f = small();
    let mut gpu = SoftGpu::new(54);
    let mut s = blank(3, 3);
    put(&mut s, 0, 0, "a");
    put(&mut s, 1, 0, "bc");
    paint(&mut f, &mut gpu, &s);
    assert_eq!(f.rebuilt(), [0, 1, 2]);
    put(&mut s, 1, 0, "de");
    let got = paint(&mut f, &mut gpu, &s);
    assert_eq!(
        f.rebuilt(),
        [0, 1, 2],
        "row 0 didn't change, and it's rebuilt"
    );
    assert!(f.wanted().is_empty());
    assert!(got == paint(&mut small(), &mut SoftGpu::new(54), &s));
}

#[test]
fn a_frame_that_needs_more_than_a_whole_atlas_drops_the_rest_and_the_next_frame_tries_again() {
    let mut f = small();
    let mut s = blank(5, 1);
    put(&mut s, 0, 0, "abcde");
    let (quads, ..) = frame(&mut f, &s);
    assert_eq!(of_kind(&quads, kind::GRAY).len(), 3, "as many as fit");
    assert!(f.wanted().is_empty(), "the frame finishes");
    let mut s = blank(5, 1);
    put(&mut s, 0, 0, "abc");
    let (quads, ..) = frame(&mut f, &s);
    assert_eq!(
        of_kind(&quads, kind::GRAY).len(),
        3,
        "and a page that fits is whole again"
    );
}

#[test]
fn a_page_with_more_glyphs_than_the_atlas_costs_nothing_until_it_changes() {
    let mut f = small();
    let mut s = blank(5, 2);
    put(&mut s, 0, 0, "abcde");
    let (first, ..) = frame(&mut f, &s);
    assert_eq!(of_kind(&first, kind::GRAY).len(), 3, "as many as fit");
    // Trying again would drop the same glyphs, so an unchanged page doesn't.
    let (again, _, uploads) = frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [] as [usize; 0], "the same page");
    assert_eq!(again, first, "and its picture");
    assert!(uploads.is_empty(), "and no glyph drawn again");
    assert!(f.diff(&s).is_empty());
    // Nor does a cursor moving over it.
    s.cursors = vec![cursor(1, 2, CursorShape::Bar)];
    let (moved, _, uploads) = frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [] as [usize; 0], "a cursor");
    assert_eq!(moved[..first.len()], first[..]);
    assert!(uploads.is_empty());
    // A change anywhere on it tries again, and every row comes with it.
    put(&mut s, 1, 4, "z");
    let (_, _, uploads) = frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [0, 1], "a change");
    assert_eq!(uploads.len(), 3, "the atlas started over");
    // That page still doesn't fit, so the frame after it costs nothing either.
    frame(&mut f, &s);
    assert_eq!(f.rebuilt(), [] as [usize; 0]);
}

#[test]
fn a_page_that_fits_again_goes_back_to_rebuilding_only_what_changed() {
    let mut f = small();
    let mut s = blank(5, 2);
    put(&mut s, 0, 0, "abcde");
    frame(&mut f, &s);
    let mut s = blank(5, 2);
    put(&mut s, 0, 0, "ab");
    put(&mut s, 1, 0, "c");
    assert_eq!(rebuilt(&mut f, &s), [0, 1], "it tries again, and it fits");
    s.cells[5].fg = RED;
    assert_eq!(rebuilt(&mut f, &s), [1], "and starts over no more");
}

#[test]
fn a_glyph_too_big_for_any_atlas_draws_nothing_and_costs_no_restart() {
    let mut f = small();
    let mut s = blank(4, 1);
    put(&mut s, 0, 0, "a😀b");
    for key in f.diff(&s) {
        let b = if matches!(
            &key,
            GlyphKey::Text {
                glyph: Glyph::Char('😀'),
                ..
            }
        ) {
            Some(GlyphBitmap {
                w: 30,
                h: 30,
                left: 0,
                top: 0,
                plane: Plane::Color,
                data: vec![255; 3600],
            })
        } else {
            fake_bitmap(&key, &cell2x())
        };
        f.supply(&key, b);
    }
    assert!(f.wanted().is_empty(), "it doesn't ask forever");
    assert_eq!(f.build(&s).quads.len(), 2);
    assert!(f.diff(&s).is_empty());
    f.build(&s);
    assert_eq!(
        f.rebuilt(),
        [] as [usize; 0],
        "and the next frame has nothing to redo"
    );
}

/// Random edits, and each frame's picture checked against a renderer with no history:
/// a row that should have been rebuilt and wasn't shows up here. Returns how many
/// frames started the atlas over: all the rows rebuilt for at most two edits.
fn incremental_matches_fresh(grid: (usize, usize), atlas: u32, seed: u64) -> usize {
    let (cols, rows) = grid;
    let letters: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
    let mut rng = Rng(seed);
    let colors = [FG, BG, RED, GREEN, BLUE];
    let mut f = Frames::new(cell2x(), atlas, atlas);
    let mut gpu = SoftGpu::new(atlas);
    let mut s = blank(cols, rows);
    let mut restarts = 0;
    for step in 0..120 {
        // Most frames touch a few cells, some none, and some the whole page.
        let edits = [0, 1, 2, 5, 50][rng.below(5)];
        for _ in 0..edits {
            let at = rng.below(cols * rows);
            let c = &mut s.cells[at];
            match rng.below(7) {
                0 | 1 => c.glyph = Glyph::Char(letters[rng.below(letters.len())]),
                2 => c.fg = colors[rng.below(5)],
                3 => c.bg = colors[rng.below(5)],
                4 => c.bold = !c.bold,
                5 => c.strike = !c.strike,
                _ => {
                    c.underline_color = colors[rng.below(5)];
                    c.underline =
                        [Underline::None, Underline::Single, Underline::Curly][rng.below(3)];
                }
            }
        }
        if rng.below(4) == 0 {
            let shapes = [
                CursorShape::Block,
                CursorShape::Bar,
                CursorShape::Underline,
                CursorShape::HalfBlock,
                CursorShape::Hollow,
            ];
            s.cursors = vec![cursor(
                rng.below(rows),
                rng.below(cols),
                shapes[rng.below(5)],
            )];
        }
        let new_background = rng.below(20) == 0;
        if new_background {
            s.background = colors[rng.below(5)];
        }
        let got = paint(&mut f, &mut gpu, &s);
        restarts += usize::from(edits <= 2 && !new_background && f.rebuilt().len() == rows);
        let want = paint(
            &mut Frames::new(cell2x(), atlas, atlas),
            &mut SoftGpu::new(atlas),
            &s,
        );
        assert!(
            got == want,
            "step {step}: the row cache's picture differs from a fresh one"
        );
    }
    restarts
}

#[test]
fn frames_built_from_the_cache_paint_what_a_fresh_renderer_paints() {
    incremental_matches_fresh((8, 6), 1024, 0x2545_F491_4F6C_DD1D);
}

#[test]
fn so_do_frames_built_through_atlas_restarts() {
    // Twelve cells and three lines always fit in an atlas that holds about 20 glyphs,
    // and the alphabet in two weights doesn't.
    let restarts = incremental_matches_fresh((4, 3), 128, 0x1234_5678_9ABC_DEF1);
    assert!(restarts >= 1, "the atlas never started over");
}
