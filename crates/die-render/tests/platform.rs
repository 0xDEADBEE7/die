//! R1's checks on the macOS `platform` wrappers. These need a Mac with Metal, and
//! the fonts from `cargo xtask fonts`.
#![cfg(target_os = "macos")]

use die_render::font_dir;
use die_render::platform::backend::metal::Gpu;
use die_render::platform::backend::text::Face;
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
