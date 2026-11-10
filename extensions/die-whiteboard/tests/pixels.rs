//! The pixel renderer, through the harness's fake sink: one buffer the size of the area placed
//! under the glyphs; a drag inks its path anti-aliased and nowhere else; each sample presents
//! the new segment's rectangle alone; a resize re-makes the buffer; undo clears the pixels and
//! redo brings them back; zoom scales them; a wheel pan moves the picture exactly; the
//! highlighter doesn't double up at its joints; labels stay cells over the pixels and a
//! selection is outlined; two windows on a board place the same buffer; a theme change
//! recolours the ink; and cells when asked.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Mods, PixelRect, PixelSinkCall, PointerKind, Rgb, Style, Theme};
use die_whiteboard::render::Damage;
use std::time::Duration;

/// The red ink 2 is, in the default theme.
fn red() -> Rgb {
    Theme::default().ansi[1]
}

#[test]
fn a_board_places_one_buffer_the_size_of_its_area_under_the_glyphs() {
    let mut s = setup("place");
    let g = s.h.surface_area(s.win).unwrap();
    let (r0, c0) = origin(&mut s.h, s.win);
    let scene = s.h.scene();
    assert_eq!(scene.placements.len(), 1, "{}", scene.snapshot());
    let p = scene.placements[0];
    assert_eq!((p.col, p.row), (c0, r0));
    assert_eq!((p.cols, p.rows), (g.text_w, g.rows));
    assert_eq!(p.z, -1, "under the glyphs, over the backgrounds");
    let (id, w, h, px) = buffer(&mut s);
    assert_eq!(id, p.buffer);
    assert_eq!((w, h), (g.text_w as u32 * 10, g.rows as u32 * 20));
    assert!(
        px.iter().all(|b| *b == 0),
        "transparent until something is drawn"
    );
    let calls = s.h.fake_pixels().take_calls();
    assert!(
        calls.iter().any(
            |c| matches!(c, PixelSinkCall::Create(i, cw, ch) if *i == id && (*cw, *ch) == (w, h))
        ),
        "{calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, PixelSinkCall::Present(..))),
        "nothing presented for an empty board: {calls:?}"
    );
}

#[test]
fn a_drag_inks_its_path_anti_aliased_and_nowhere_else() {
    let mut s = setup("ink");
    keys(&mut s.h, "2");
    // At y = 10.25, which is 20.5 px: medium is 4 board units, 8 px at 2x, so the line covers
    // 16.5 to 24.5 and the rows at its edges by half.
    let y = 10.25;
    gesture(
        &mut s,
        &[
            (10.0, y),
            (20.0, y),
            (30.0, y),
            (40.0, y),
            (50.0, y),
            (60.0, y),
        ],
    );
    let on = pixel_px(&mut s, (60.0, 20.0)).unwrap();
    let r = red();
    assert_eq!(on, [r.0, r.1, r.2, 255], "the ink, opaque, on the line");
    assert_eq!(
        pixel_px(&mut s, (60.0, 23.0)).unwrap()[3],
        255,
        "inside the width"
    );
    assert_eq!(
        pixel_px(&mut s, (60.0, 40.0)),
        Some([0, 0, 0, 0]),
        "clear off it"
    );
    let column: Vec<u8> = (10..30)
        .map(|y| pixel_px(&mut s, (60.0, y as f32)).unwrap()[3])
        .collect();
    assert!(
        column.iter().any(|a| *a > 0 && *a < 255),
        "an anti-aliased edge: {column:?}"
    );
    // Premultiplied: a half-covered pixel's colour is scaled by its alpha.
    let edge = (10..30)
        .map(|y| pixel_px(&mut s, (60.0, y as f32)).unwrap())
        .find(|p| p[3] > 0 && p[3] < 255)
        .unwrap();
    assert!(
        edge[0] <= edge[3] && edge[1] <= edge[3] && edge[2] <= edge[3],
        "{edge:?}"
    );
    // Nothing outside the stroke's box: 10..60 by 10, 2 units of width and the caps around.
    let (_, w, h, px) = buffer(&mut s);
    let outside = px
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, p)| {
            let (x, y) = ((*i as u32 % w) as f32, (*i as u32 / w) as f32);
            p[3] > 0 && (!(14.0..=126.0).contains(&x) || !(14.0..=26.0).contains(&y))
        })
        .count();
    assert_eq!(outside, 0, "{w}x{h}");
}

#[test]
fn each_sample_presents_the_new_segments_rectangle_alone() {
    let mut s = setup("damage");
    s.h.fake_pixels().take_calls();
    press(&mut s, (10.0, 10.0));
    let calls = s.h.fake_pixels().take_calls();
    let dots: Vec<PixelRect> = calls
        .iter()
        .filter_map(|c| match c {
            PixelSinkCall::Present(_, r) => Some(*r),
            _ => None,
        })
        .collect();
    assert_eq!(dots.len(), 1, "one present for the press: {calls:?}");
    // A dot of radius 2 at (20, 20) px, 2 px of edge around.
    assert_eq!(dots[0], PixelRect::new(14, 14, 12, 12));
    drag(&mut s, (30.0, 10.0));
    let calls = s.h.fake_pixels().take_calls();
    let segs: Vec<PixelRect> = calls
        .iter()
        .filter_map(|c| match c {
            PixelSinkCall::Present(_, r) => Some(*r),
            _ => None,
        })
        .collect();
    assert_eq!(segs.len(), 1, "one present for the sample: {calls:?}");
    // The low-pass puts the sample at (20, 10): the segment from (10, 10), 2 units of width
    // and 2 px of edge around, in pixels at 2x.
    assert_eq!(segs[0], PixelRect::new(14, 14, 32, 12));
    // Only that rectangle changed: nothing is inked to the right of it.
    assert_eq!(pixel_px(&mut s, (50.0, 20.0)), Some([0, 0, 0, 0]));
    assert!(pixel_px(&mut s, (40.0, 20.0)).unwrap()[3] > 0);
    // A sample that moves nothing presents nothing.
    drag(&mut s, (30.0, 10.0));
    let again = s.h.fake_pixels().take_calls();
    let presents = again
        .iter()
        .filter(|c| matches!(c, PixelSinkCall::Present(..)))
        .count();
    assert!(presents <= 1, "{again:?}");
    release(&mut s, (60.0, 10.0));
    // The release puts the committed stroke (simplified, ending where the pointer lifted) in
    // place of the live one: the whole stroke's region.
    let calls = s.h.fake_pixels().take_calls();
    let last = calls.iter().rev().find_map(|c| match c {
        PixelSinkCall::Present(_, r) => Some(*r),
        _ => None,
    });
    let last = last.expect("a present at the release");
    assert!(last.x <= 14 && last.x + last.width >= 126, "{last:?}");
    assert!(
        pixel_px(&mut s, (110.0, 20.0)).unwrap()[3] > 0,
        "to the end"
    );
}

#[test]
fn a_resize_grows_the_buffer_and_draws_the_board_again_and_a_narrowing_keeps_it() {
    let mut s = setup("resize");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let (id, w0, h0, _) = buffer(&mut s);
    s.h.fake_pixels().take_calls();
    // Smaller: the buffer stays, shown from its top-left, and nothing is drawn again.
    s.h.set_window_size(60, 20);
    s.h.run_until_idle();
    let (id1, w, h, _) = buffer(&mut s);
    assert_eq!((id1, w, h), (id, w0, h0), "kept");
    let g = s.h.surface_area(s.win).unwrap();
    assert!((g.text_w as u32 * 10, g.rows as u32 * 20) < (w0, h0));
    let calls = s.h.fake_pixels().take_calls();
    assert!(
        !calls.iter().any(|c| matches!(c, PixelSinkCall::Resize(..))),
        "{calls:?}"
    );
    assert!(inked(&mut s, (35.0, 10.0)));
    // Taller: the buffer grows to the area and the board is drawn again.
    s.h.set_window_size(100, 40);
    s.h.run_until_idle();
    let (id2, w1, h1, _) = buffer(&mut s);
    assert_eq!(id, id2, "the same buffer, resized");
    assert_ne!((w0, h0), (w1, h1));
    let g = s.h.surface_area(s.win).unwrap();
    assert_eq!((w1, h1), (g.text_w as u32 * 10, g.rows as u32 * 20));
    let calls = s.h.fake_pixels().take_calls();
    assert!(
        calls.iter().any(
            |c| matches!(c, PixelSinkCall::Resize(i, cw, ch) if *i == id && (*cw, *ch) == (w1, h1))
        ),
        "{calls:?}"
    );
    assert!(inked(&mut s, (35.0, 10.0)), "drawn again at the new size");
    // Killing the board destroys the buffer, and the placement goes with it.
    assert!(s.h.kill(s.buf));
    s.h.run_until_idle();
    let calls = s.h.fake_pixels().take_calls();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, PixelSinkCall::Destroy(i) if *i == id)),
        "{calls:?}"
    );
    assert!(s.h.scene().placements.is_empty());
}

#[test]
fn undo_clears_the_pixels_and_redo_brings_them_back() {
    let mut s = setup("undo");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "r");
    gesture(&mut s, &[(10.0, 50.0), (60.0, 90.0)]);
    assert!(inked(&mut s, (35.0, 10.0)) && inked(&mut s, (10.0, 70.0)));
    keys(&mut s.h, "u");
    assert!(inked(&mut s, (35.0, 10.0)));
    assert!(!inked(&mut s, (10.0, 70.0)), "the rectangle went");
    keys(&mut s.h, "u");
    assert_eq!(inked_count(&mut s), 0, "nothing left");
    keys(&mut s.h, "<C-r><C-r>");
    assert!(inked(&mut s, (35.0, 10.0)) && inked(&mut s, (10.0, 70.0)));
    // The eraser takes one, and the other's pixels stay whole.
    keys(&mut s.h, "e");
    gesture(&mut s, &[(30.0, 0.0), (30.0, 20.0)]);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    assert!(!inked(&mut s, (35.0, 10.0)));
    assert!(inked(&mut s, (10.0, 70.0)) && inked(&mut s, (60.0, 90.0)));
}

#[test]
fn zoom_scales_the_picture_about_the_pointer() {
    let mut s = setup("zoom");
    gesture(&mut s, &[(10.0, 5.0), (60.0, 5.0)]);
    let thickness = |s: &mut Setup| {
        (0..60)
            .filter(|y| pixel_px(s, (70.0, *y as f32)).unwrap()[3] > 0)
            .count()
    };
    let before = thickness(&mut s);
    assert!((7..=10).contains(&before), "8 px at 2x: {before}");
    // Cmd+wheel about the origin: the picture grows from there.
    pointer(
        &mut s,
        PointerKind::Scroll { dx: 0.0, dy: -4.0 },
        (0.0, 0.0),
        Mods {
            super_: true,
            ..Mods::default()
        },
        0,
    );
    let view = board(&mut s.h, s.buf).borrow().view;
    assert_eq!(view.zoom, 1.25);
    assert_eq!(view.pan, (0.0, 0.0), "about the origin");
    assert!(inked(&mut s, (35.0, 5.0)), "through the view");
    let after = (0..60)
        .filter(|y| pixel_px(&mut s, (87.0, *y as f32)).unwrap()[3] > 0)
        .count();
    assert!(after > before, "{after} > {before}");
    // The keys zoom about the area's centre; 0 takes it all back.
    keys(&mut s.h, "-");
    assert_eq!(board(&mut s.h, s.buf).borrow().view.zoom, 1.0);
    keys(&mut s.h, "++0");
    assert_eq!(board(&mut s.h, s.buf).borrow().view, Default::default());
    assert_eq!(thickness(&mut s), before);
}

#[test]
fn a_wheel_pan_moves_the_picture_exactly() {
    let mut s = setup("pan");
    gesture(&mut s, &[(10.0, 5.0), (60.0, 35.0), (20.0, 70.0)]);
    keys(&mut s.h, "c");
    gesture(&mut s, &[(100.0, 40.0), (160.0, 90.0)]);
    keys(&mut s.h, "a");
    gesture(&mut s, &[(30.0, 120.0), (120.0, 100.0)]);
    for (dx, dy) in [(0.0, 1.0), (1.0, 0.0), (0.0, -3.0), (-2.0, 1.0)] {
        pointer(
            &mut s,
            PointerKind::Scroll { dx, dy },
            (50.0, 50.0),
            Mods::default(),
            0,
        );
        let (_, _, _, shifted) = buffer(&mut s);
        // Drawn again from nothing, through the same view: the same pixels.
        {
            let shared = board(&mut s.h, s.buf);
            shared.borrow_mut().changed(Duration::ZERO, Damage::All);
        }
        let (_, _, _, redrawn) = buffer(&mut s);
        // The same picture: tiny-skia's anti-aliasing isn't bit-exact under a translation, so
        // a few edge pixels differ by a coverage step or two.
        let (_, w, _, _) = buffer(&mut s);
        let differ: Vec<(u32, u32, [u8; 4], [u8; 4])> = shifted
            .chunks_exact(4)
            .zip(redrawn.chunks_exact(4))
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| {
                (
                    i as u32 % w,
                    i as u32 / w,
                    [a[0], a[1], a[2], a[3]],
                    [b[0], b[1], b[2], b[3]],
                )
            })
            .collect();
        let inked = redrawn.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(inked > 2000, "{inked}");
        assert!(
            differ.len() < inked / 50,
            "after a wheel of ({dx}, {dy}): {} of {inked} differ: {differ:?}",
            differ.len()
        );
        for (x, y, a, b) in &differ {
            let worst = (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap();
            assert!(worst <= 64, "({x}, {y}): {a:?} vs {b:?}");
        }
    }
    assert_eq!(board(&mut s.h, s.buf).borrow().view.pan, (10.0, 20.0));
}

#[test]
fn the_highlighter_is_even_at_its_joints_and_a_shape_follows_the_drag() {
    let mut s = setup("highlighter");
    keys(&mut s.h, "h");
    assert!(status(&mut s).contains("highlighter"));
    press(&mut s, (20.0, 50.0));
    for x in [40.0, 60.0, 80.0, 100.0] {
        drag(&mut s, (x, 50.0));
    }
    // Live, before the release: the alpha on the line is the same at a joint and between.
    let at = |s: &mut Setup, x: f32| pixel_px(s, (x, 100.0)).unwrap()[3];
    let (joint, between) = (at(&mut s, 60.0), at(&mut s, 70.0));
    assert_eq!(joint, between, "no double alpha where segments meet");
    assert!((80..=100).contains(&joint), "35 %: {joint}");
    release(&mut s, (120.0, 50.0));
    assert_eq!(at(&mut s, 60.0), joint, "committed the same");
    // A shape is put back and drawn again as it grows: nothing stays where it was.
    keys(&mut s.h, "r");
    press(&mut s, (10.0, 150.0));
    drag(&mut s, (60.0, 190.0));
    assert!(inked(&mut s, (60.0, 170.0)), "the right edge now");
    drag(&mut s, (40.0, 190.0));
    assert!(
        !inked(&mut s, (60.0, 170.0)),
        "gone when the drag came back"
    );
    assert!(inked(&mut s, (40.0, 170.0)));
    release(&mut s, (40.0, 190.0));
    assert!(inked(&mut s, (40.0, 170.0)) && !inked(&mut s, (60.0, 170.0)));
}

#[test]
fn labels_are_cells_over_the_pixels_and_a_selection_is_outlined() {
    let mut s = setup("labels");
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 20.0));
    keys(&mut s.h, "hi<Esc>");
    let rows = area(&mut s);
    assert!(rows[2].starts_with("    hi"), "{rows:#?}");
    assert_eq!(inked_count(&mut s), 0, "a label puts nothing in the pixels");
    keys(&mut s.h, "v");
    click(&mut s, (22.0, 22.0));
    assert_eq!(board(&mut s.h, s.buf).borrow().selection.len(), 1);
    let (r0, c0) = origin(&mut s.h, s.win);
    let scene = s.h.scene();
    let h_cell = &scene.row(r0 + 2)[c0 + 4];
    let blank = &scene.row(r0 + 10)[c0 + 40];
    assert_eq!(h_cell.glyph.to_string(), "h");
    assert_eq!((h_cell.fg, h_cell.bg), (blank.bg, blank.fg), "reversed");
    // Its bounds are two cells by one (10 by 10 units), outlined 3 units outside.
    assert!(inked_count(&mut s) > 0, "the outline");
    assert!((0..6).any(|i| inked(&mut s, (17.0, 20.0 + i as f32))));
    assert!(!inked(&mut s, (25.0, 25.0)), "nothing inside");
    keys(&mut s.h, "<Esc>");
    assert_eq!(
        inked_count(&mut s),
        0,
        "the outline went with the selection"
    );
}

#[test]
fn two_windows_on_a_board_place_the_same_buffer() {
    let mut s = setup("two");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "<C-w>v");
    assert_eq!(s.h.window_list().len(), 2);
    let scene = s.h.scene();
    assert_eq!(scene.placements.len(), 2, "{}", scene.snapshot());
    assert_eq!(scene.placements[0].buffer, scene.placements[1].buffer);
    assert_ne!(scene.placements[0].col, scene.placements[1].col);
    assert!(inked(&mut s, (35.0, 10.0)));
}

#[test]
fn a_theme_change_recolours_the_ink() {
    let mut s = setup("theme");
    keys(&mut s.h, "2");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let r = red();
    assert_eq!(pixel_px(&mut s, (60.0, 20.0)), Some([r.0, r.1, r.2, 255]));
    let theme = Theme::default().face("whiteboard-ink-2", Style::new().fg(Rgb(0, 0, 255)));
    s.h.set_theme(theme);
    // The inks are read at the next event or command: a key will do.
    keys(&mut s.h, "p");
    assert_eq!(
        pixel_px(&mut s, (60.0, 20.0)),
        Some([0, 0, 255, 255]),
        "drawn again in the theme's colour"
    );
}

#[test]
fn cells_when_asked_and_the_renderer_in_the_status() {
    let mut s = setup_cells("cells");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    assert!(s.h.scene().placements.is_empty(), "no buffer in cells");
    let calls = s.h.fake_pixels().take_calls();
    assert!(calls.is_empty(), "{calls:?}");
    let rows = area(&mut s);
    assert!(
        rows[1]
            .chars()
            .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
        "braille: {rows:#?}"
    );
}

#[test]
fn a_split_doesnt_remake_the_buffer_and_each_window_shows_its_part() {
    let mut s = setup("split");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let (id, w, h, _) = buffer(&mut s);
    s.h.fake_pixels().take_calls();
    // Two, then three windows of two sizes on one board: the buffer, sized to the first, is
    // shown from its top-left in each (C10) rather than re-made at every layout change.
    keys(&mut s.h, "<C-w>s<C-w>v");
    assert_eq!(s.h.window_list().len(), 3);
    let calls = s.h.fake_pixels().take_calls();
    assert!(
        !calls.iter().any(|c| matches!(c, PixelSinkCall::Resize(..))),
        "{calls:?}"
    );
    let scene = s.h.scene();
    assert_eq!(scene.placements.len(), 3, "{}", scene.snapshot());
    assert!(scene.placements.iter().all(|p| p.buffer == id));
    assert!(scene.placements.iter().any(|p| (p.cols as u32 * 10) < w));
    let (id2, w2, h2, _) = buffer(&mut s);
    assert_eq!((id2, w2, h2), (id, w, h));
    assert!(inked(&mut s, (35.0, 10.0)));
}
