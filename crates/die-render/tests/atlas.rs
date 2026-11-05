//! The glyph atlas, as data: bitmaps in, slots and uploads out.

use die_core::{Glyph, Underline};
use die_render::atlas::{Atlas, GlyphKey, Slot};
use die_render::frame::{GlyphBitmap, Plane};
use die_render::sprite::Line;

fn key(c: char) -> GlyphKey {
    GlyphKey::Text {
        glyph: Glyph::Char(c),
        bold: false,
        italic: false,
        span: 1,
    }
}

fn nth(i: u32) -> GlyphKey {
    key(char::from_u32(0x4E00 + i).unwrap())
}

fn bitmap(w: u32, h: u32, plane: Plane) -> GlyphBitmap {
    let bytes = if plane == Plane::Gray { 1 } else { 4 };
    GlyphBitmap {
        w,
        h,
        left: -1,
        top: 3,
        plane,
        data: vec![7; (w * h * bytes) as usize],
    }
}

fn slot(a: &Atlas, k: &GlyphKey) -> Slot {
    a.get(k).unwrap().unwrap()
}

#[test]
fn a_glyph_nobody_supplied_is_unknown_and_a_blank_one_is_known_and_takes_no_room() {
    let mut a = Atlas::new(64, 64);
    assert_eq!(a.get(&key('a')), None);
    a.insert(&key(' '), None).unwrap();
    assert_eq!(a.get(&key(' ')), Some(None));
    let empty = GlyphBitmap {
        w: 0,
        h: 0,
        left: 0,
        top: 0,
        plane: Plane::Gray,
        data: vec![],
    };
    a.insert(&key('\u{A0}'), Some(empty)).unwrap();
    assert_eq!(a.get(&key('\u{A0}')), Some(None));
    assert!(a.take_uploads().is_empty(), "nothing to copy");
    a.insert(&key('a'), Some(bitmap(5, 5, Plane::Gray)))
        .unwrap();
    assert_eq!(
        (slot(&a, &key('a')).x, slot(&a, &key('a')).y),
        (0, 0),
        "the room is all there"
    );
}

#[test]
fn a_slot_says_where_its_upload_went_and_where_the_glyph_goes_in_its_cell() {
    let mut a = Atlas::new(64, 64);
    let b = bitmap(5, 6, Plane::Gray);
    a.insert(&key('a'), Some(b.clone())).unwrap();
    a.insert(&key('b'), Some(bitmap(4, 4, Plane::Gray)))
        .unwrap();
    let s = slot(&a, &key('b'));
    assert_eq!((s.w, s.h, s.left, s.top), (4, 4, -1, 3));
    let up = a.take_uploads();
    assert_eq!(up.len(), 2);
    assert_eq!((up[0].x, up[0].y, up[0].w, up[0].h), (0, 0, 5, 6));
    assert_eq!(up[0].data, b.data);
    assert_eq!((up[1].x, up[1].y), (s.x, s.y));
    assert!(a.take_uploads().is_empty(), "each upload once");
}

#[test]
fn glyphs_keep_a_pixel_apart_and_never_overlap_or_leave_the_plane() {
    let size = 100;
    let mut a = Atlas::new(size, size);
    let mut placed = vec![];
    for i in 0..200 {
        let (w, h) = (3 + i % 9, 2 + (i * 7) % 11);
        if a.insert(&nth(i), Some(bitmap(w, h, Plane::Gray))).is_err() {
            break;
        }
        placed.push(slot(&a, &nth(i)));
    }
    assert!(placed.len() > 30, "only {} fit", placed.len());
    for (i, p) in placed.iter().enumerate() {
        assert!(
            p.x + p.w <= size && p.y + p.h <= size,
            "{p:?} leaves the plane"
        );
        for q in &placed[..i] {
            let apart = p.x > q.x + q.w || q.x > p.x + p.w || p.y > q.y + q.h || q.y > p.y + p.h;
            assert!(apart, "{p:?} and {q:?} touch");
        }
    }
}

#[test]
fn a_row_wraps_when_the_next_glyph_does_not_fit_and_a_plane_can_fill_exactly() {
    let mut a = Atlas::new(10, 10);
    a.insert(&key('a'), Some(bitmap(4, 3, Plane::Gray)))
        .unwrap();
    a.insert(&key('b'), Some(bitmap(5, 2, Plane::Gray)))
        .unwrap();
    assert_eq!(
        (slot(&a, &key('b')).x, slot(&a, &key('b')).y),
        (5, 0),
        "beside it, to the very edge"
    );
    a.insert(&key('c'), Some(bitmap(4, 6, Plane::Gray)))
        .unwrap();
    assert_eq!(
        (slot(&a, &key('c')).x, slot(&a, &key('c')).y),
        (0, 4),
        "next row, under the row's tallest, down to the very edge"
    );
    assert!(
        a.insert(&key('d'), Some(bitmap(6, 1, Plane::Gray)))
            .is_err()
    );
    assert_eq!(
        a.get(&key('d')),
        None,
        "a glyph that didn't fit isn't noted"
    );
}

#[test]
fn the_two_planes_fill_apart() {
    let mut a = Atlas::new(64, 8);
    a.insert(&key('😀'), Some(bitmap(8, 8, Plane::Color)))
        .unwrap();
    assert!(
        a.insert(&key('😁'), Some(bitmap(8, 8, Plane::Color)))
            .is_err()
    );
    a.insert(&key('a'), Some(bitmap(8, 8, Plane::Gray)))
        .unwrap();
    let (g, c) = (slot(&a, &key('a')), slot(&a, &key('😀')));
    assert_eq!((g.plane, c.plane), (Plane::Gray, Plane::Color));
    assert_eq!(
        (g.x, g.y),
        (c.x, c.y),
        "each plane counts from its own corner"
    );
}

#[test]
fn clearing_forgets_every_glyph_and_every_upload_and_makes_the_room_again() {
    let mut a = Atlas::new(8, 8);
    a.insert(&key('a'), Some(bitmap(8, 8, Plane::Gray)))
        .unwrap();
    assert!(
        a.insert(&key('b'), Some(bitmap(8, 8, Plane::Gray)))
            .is_err()
    );
    a.clear();
    assert_eq!(a.get(&key('a')), None);
    assert!(a.take_uploads().is_empty());
    a.insert(&key('b'), Some(bitmap(8, 8, Plane::Gray)))
        .unwrap();
    assert_eq!((slot(&a, &key('b')).x, slot(&a, &key('b')).y), (0, 0));
}

#[test]
fn a_glyph_bigger_than_its_plane_draws_nothing_since_clearing_would_not_help() {
    let mut a = Atlas::new(8, 4);
    a.insert(&key('a'), Some(bitmap(9, 1, Plane::Gray)))
        .unwrap();
    a.insert(&key('b'), Some(bitmap(1, 9, Plane::Gray)))
        .unwrap();
    a.insert(&key('c'), Some(bitmap(5, 5, Plane::Color)))
        .unwrap();
    for c in ['a', 'b', 'c'] {
        assert_eq!(a.get(&key(c)), Some(None), "{c}");
    }
    assert!(a.take_uploads().is_empty());
}

#[test]
fn full_gives_the_bitmap_back_for_a_second_try() {
    let mut a = Atlas::new(8, 8);
    a.insert(&key('a'), Some(bitmap(8, 8, Plane::Gray)))
        .unwrap();
    let b = bitmap(8, 8, Plane::Gray);
    let e = a.insert(&key('b'), Some(b.clone())).unwrap_err();
    assert_eq!(e.0, b);
}

#[test]
fn keys_tell_styles_spans_and_lines_apart() {
    let mut a = Atlas::new(64, 64);
    let style = |bold, italic, span| GlyphKey::Text {
        glyph: Glyph::Char('x'),
        bold,
        italic,
        span,
    };
    let keys = [
        style(false, false, 1),
        style(true, false, 1),
        style(false, true, 1),
        style(true, true, 1),
        style(false, false, 2),
        GlyphKey::Text {
            glyph: Glyph::Cluster("x\u{301}".into()),
            bold: false,
            italic: false,
            span: 1,
        },
        GlyphKey::Line(Line::Strike),
        GlyphKey::Line(Line::Under(Underline::Single)),
        GlyphKey::Line(Line::Under(Underline::Curly)),
    ];
    for (i, k) in keys.iter().enumerate() {
        assert_eq!(a.get(k), None, "{k:?} has a slot of its own");
        a.insert(k, Some(bitmap(2 + i as u32, 2, Plane::Gray)))
            .unwrap();
    }
}
