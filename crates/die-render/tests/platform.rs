//! R1's checks on the macOS `platform` wrappers. These need a Mac with Metal, and
//! the fonts from `cargo xtask fonts`.
#![cfg(target_os = "macos")]

use die_core::Rgb;
use die_render::font::{CellMetrics, fit};
use die_render::font_dir;
use die_render::frame::{Plane, Quad, Upload};
use die_render::platform::backend::metal::Gpu;
use die_render::platform::backend::text::Face;
use std::mem::{offset_of, size_of};
use std::path::Path;

fn regular() -> Face {
    let path = font_dir().join("IoskeleyMonoTerm-Regular.ttf");
    assert!(
        path.exists(),
        "{} is missing: run `cargo xtask fonts`",
        path.display()
    );
    Face::from_file(&path, 32.0).expect("the font loads")
}

#[test]
fn metal_device_and_queue() {
    let gpu = Gpu::system_default().expect("a Metal device");
    assert!(!gpu.name().is_empty());
}

#[test]
fn font_loads_from_its_file() {
    let f = regular();
    assert_eq!(f.postscript_name(), "Ioskeley-Mono-Term");
    assert!(!f.is_color());
    let m = f.metrics();
    assert!(
        m.ascent > 0.0 && m.descent > 0.0 && m.underline_thickness > 0.0,
        "{m:?}"
    );
}

#[test]
fn monospace_advances() {
    let f = regular();
    let adv = |c| f.advance(f.glyph(c).unwrap_or_else(|| panic!("no glyph for {c:?}")));
    let m = adv('M');
    assert!(m > 0.0);
    for c in ['i', 'W', '0', '_', '│'] {
        assert_eq!(adv(c), m, "{c:?}");
    }
}

#[test]
fn missing_glyphs_and_files() {
    let f = regular();
    assert_eq!(f.glyph('\u{10FFFD}'), None);
    assert!(Face::from_file(&font_dir().join("no-such-font.ttf"), 16.0).is_err());
}

#[test]
fn astral_characters_map() {
    // Two UTF-16 units in, through pointers that cover both, and a color face.
    let emoji = Path::new("/System/Library/Fonts/Apple Color Emoji.ttc");
    let f = Face::from_file(emoji, 32.0).expect("the system's emoji font loads");
    assert!(f.is_color());
    assert!(f.glyph('😀').is_some());
    assert_eq!(regular().glyph('😀'), None, "and a face without it says so");
}

#[test]
fn a_font_size_must_be_positive() {
    let path = font_dir().join("IoskeleyMonoTerm-Regular.ttf");
    for px in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let e = Face::from_file(&path, px).err().expect("an error");
        assert_eq!(e, format!("font size {px} isn't a positive size"));
    }
}

#[test]
fn the_compiled_shader_lays_out_its_instance_as_quad_does() {
    let gpu = Gpu::system_default().expect("a Metal device");
    let (size, members) = gpu.instance_layout().expect("the shader's reflection");
    assert_eq!(size, size_of::<Quad>());
    let member = |name: &str| members.iter().find(|m| m.0 == name).map(|m| m.1);
    assert_eq!(members.len(), 5, "{members:?}");
    assert_eq!(member("rect"), Some(offset_of!(Quad, rect)), "{members:?}");
    assert_eq!(member("uv"), Some(offset_of!(Quad, uv)), "{members:?}");
    assert_eq!(
        member("color"),
        Some(offset_of!(Quad, color)),
        "{members:?}"
    );
    assert_eq!(member("kind"), Some(offset_of!(Quad, kind)), "{members:?}");
    assert_eq!(member("pad"), Some(offset_of!(Quad, pad)), "{members:?}");
}

#[test]
fn an_upload_that_does_not_fit_its_atlas_is_refused() {
    let gpu = Gpu::system_default().expect("a Metal device");
    let p = gpu.pipeline(8, 8).expect("a pipeline");
    let upload = |x, w, h, bytes| Upload {
        plane: Plane::Gray,
        x,
        y: 0,
        w,
        h,
        data: vec![255; bytes],
    };
    assert_eq!(p.upload(&upload(4, 4, 8, 32)), Ok(()), "up to the edge");
    assert!(p.upload(&upload(5, 4, 8, 32)).is_err(), "past the edge");
    assert!(p.upload(&upload(0, 4, 8, 31)).is_err(), "too few bytes");
    assert!(p.upload(&upload(0, 4, 8, 33)).is_err(), "too many bytes");
    let color = Upload {
        plane: Plane::Color,
        ..upload(0, 2, 2, 4)
    };
    assert!(p.upload(&color).is_err(), "a color pixel is 4 bytes");
    // Sums that wrap to something small mustn't get past the check.
    assert!(p.upload(&upload(u32::MAX, 2, 1, 2)).is_err(), "x + w wraps");
    let low = Upload {
        y: u32::MAX,
        ..upload(0, 1, 2, 2)
    };
    assert!(p.upload(&low).is_err(), "y + h wraps");
    // 2^31 is past the edge, and each sum wraps to 0, as does 2^31 × 4 bytes × 2^31.
    let huge = 1 << 31;
    let all = Upload {
        plane: Plane::Color,
        x: huge,
        y: huge,
        w: huge,
        h: huge,
        data: vec![],
    };
    assert!(p.upload(&all).is_err(), "the length wraps too");
    // Metal asserts on a region with no pixels.
    assert!(p.upload(&upload(0, 0, 8, 0)).is_err(), "no width");
    assert!(p.upload(&upload(0, 4, 0, 0)).is_err(), "no height");
    assert!(
        p.upload(&upload(8, 0, 8, 0)).is_err(),
        "no width, at the edge"
    );
}

#[test]
fn a_texture_metal_cannot_make_is_an_error_since_metal_would_stop_the_process() {
    let gpu = Gpu::system_default().expect("a Metal device");
    assert!(Gpu::system_default().unwrap().pipeline(0, 8).is_err());
    assert!(Gpu::system_default().unwrap().pipeline(8, 0).is_err());
    let mut p = gpu.pipeline(8, 8).expect("a pipeline");
    let black = Rgb(0, 0, 0);
    assert_eq!(
        p.render(&[], black, 0, 4).err().unwrap(),
        "Metal has no 0x4 texture"
    );
    assert!(p.render(&[], black, 4, 0).is_err());
    assert!(p.render(&[], black, 16385, 1).is_err());
    assert!(p.render(&[], black, 1, 16385).is_err());
    assert_eq!(p.render(&[], black, 16384, 1).map(|i| i.w), Ok(16384));
    assert_eq!(p.render(&[], black, 4, 4).map(|i| i.w), Ok(4));
}

#[test]
fn shaping_counts_each_glyph_where_the_font_puts_it() {
    let f = regular();
    let advance = f.advance(f.glyph('i').unwrap());
    let top = |t: &str| {
        let i = f.shape(t).ink;
        i.y + i.h
    };
    assert!(
        f.shape("ii").ink.w > advance,
        "the second glyph's ink is past the first's advance"
    );
    assert!(
        top("e\u{301}\u{301}") > top("e\u{301}") + 3.0,
        "a second mark stacks on the first: {} {}",
        top("e\u{301}\u{301}"),
        top("e\u{301}")
    );
}

#[test]
fn a_squeezed_glyph_is_drawn_smaller_and_keeps_its_margin() {
    let f = regular();
    let cell = CellMetrics::new(f.metrics(), f.advance(f.glyph('M').unwrap()), 2.0);
    let arrow = f.shape("\u{27F5}");
    let at = fit(arrow.ink, arrow.advance, 1, &cell, false).unwrap();
    assert!(at.scale < 0.7, "{}", at.scale);
    let b = arrow.draw(&at).unwrap();
    let w = b.w as usize;
    // Drawn at full size it would run off the bitmap, whose size allows for the squeeze.
    assert!(
        b.data.chunks(w).all(|r| r[0] == 0 && r[w - 1] == 0),
        "the sides are clear"
    );
    assert!(
        b.data.chunks(w).any(|r| r[1] > 0 || r[2] > 0),
        "and ink reaches the left"
    );
    assert!(
        b.data.chunks(w).any(|r| r[w - 2] > 0 || r[w - 3] > 0),
        "and the right"
    );
}

#[test]
fn the_system_finds_a_face_for_what_the_font_lacks() {
    let f = regular();
    assert_eq!(f.glyph('中'), None);
    let cjk = f.fallback_for("中");
    assert!(cjk.glyph('中').is_some(), "{}", cjk.postscript_name());
    assert_ne!(cjk.postscript_name(), f.postscript_name());
    let emoji = f.fallback_for("😀");
    assert!(emoji.is_color(), "{}", emoji.postscript_name());
}

#[test]
fn shaping_gives_a_cluster_one_advance_with_its_mark_on_the_base() {
    let f = regular();
    let (e, mark) = (f.shape("e"), f.shape("e\u{301}"));
    assert_eq!(e.advance, f.advance(f.glyph('e').unwrap()));
    assert_eq!(mark.advance, e.advance, "the mark takes no room");
    assert!(
        mark.ink.h > e.ink.h + 2.0,
        "and rises above: {:?} {:?}",
        mark.ink,
        e.ink
    );
    assert_eq!(mark.ink.y, e.ink.y);
    assert_eq!(e.plane, Plane::Gray);
    assert!(f.shape(" ").ink.is_empty(), "a space has no ink");
    assert!(f.shape("").ink.is_empty());
}

#[test]
fn shaping_finds_a_face_for_a_character_on_its_own_and_says_when_it_is_in_color() {
    let f = regular();
    let cjk = f.shape("中");
    assert!(!cjk.ink.is_empty(), "the system's face draws it");
    assert_eq!(cjk.plane, Plane::Gray);
    assert_eq!(f.shape("😀").plane, Plane::Color);
    assert_eq!(f.fallback_for("😀").shape("😀").plane, Plane::Color);
}

#[test]
fn a_drawn_glyph_is_coverage_where_its_fit_says_and_an_emoji_is_premultiplied_color() {
    let f = regular();
    let cell = CellMetrics::new(f.metrics(), f.advance(f.glyph('M').unwrap()), 2.0);
    let shaped = f.shape("H");
    let at = fit(shaped.ink, shaped.advance, 1, &cell, false).unwrap();
    let b = shaped.draw(&at).unwrap();
    assert_eq!((b.w, b.h, b.left, b.top), (at.w, at.h, at.left, at.top));
    assert_eq!(b.data.len(), (b.w * b.h) as usize);
    // A margin of nothing on all sides, and solid stems inside it.
    let w = b.w as usize;
    assert!(b.data[..w].iter().all(|&v| v == 0), "the top row");
    assert!(
        b.data[b.data.len() - w..].iter().all(|&v| v == 0),
        "the bottom row"
    );
    assert!(
        b.data.chunks(w).all(|r| r[0] == 0 && r[w - 1] == 0),
        "the sides"
    );
    assert!(b.data.contains(&255), "solid ink");

    let emoji = f.fallback_for("😀").shape("😀");
    let at = fit(emoji.ink, emoji.advance, 2, &cell, true).unwrap();
    let b = emoji.draw(&at).unwrap();
    assert_eq!(b.plane, Plane::Color);
    assert_eq!(b.data.len(), (b.w * b.h * 4) as usize);
    assert!(
        b.data
            .chunks(4)
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]),
        "no channel over its alpha: premultiplied"
    );
    assert!(
        b.data.chunks(4).any(|p| p[2] > p[0] + 60),
        "more red than blue: BGRA, in that order"
    );
}
