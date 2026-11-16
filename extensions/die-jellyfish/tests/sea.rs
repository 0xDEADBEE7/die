//! The sea by itself, with no DIE round it: whatever it's asked it draws something, a moment is
//! one picture, and its colours suit a dark paper and a light one.

use die_test_cap as _;

use die_jellyfish::sea::{Extras, Sea, Tones};

/// Your theme's background and text, and a light theme's.
const DARK: (u32, u32) = (0x1a1a1a, 0xcccccc);
const LIGHT: (u32, u32) = (0xf5f5f0, 0x1a1a1a);

const WATER: Extras = Extras {
    water: true,
    shoal: false,
};
const EVERYTHING: Extras = Extras {
    water: true,
    shoal: true,
};

/// A fresh sea's picture of one moment: the letters and their colours.
fn picture(cols: usize, rows: usize, aspect: f64, t: f64) -> (Vec<u16>, Vec<u32>) {
    let mut sea = Sea::new();
    let tones = Tones::for_theme(DARK.0, DARK.1);
    sea.draw(cols, rows, aspect, tones, EVERYTHING, t);
    (sea.glyphs().to_vec(), sea.inks().to_vec())
}

/// How bright a colour is, as WCAG counts it, and the contrast of two.
fn luminance(c: u32) -> f64 {
    let channel = |shift: u32| {
        let v = f64::from((c >> shift) & 255) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

fn contrast(a: u32, b: u32) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn no_size_time_or_cell_shape_panics_and_every_glyph_is_printable_ascii() {
    let tones = Tones::for_theme(DARK.0, DARK.1);
    let mut sea = Sea::new();
    let aspects = [0.0, -2.0, f64::NAN, 0.3, 0.5, 1.0, 1e9, f64::INFINITY];
    let times = [
        f64::NAN,
        f64::NEG_INFINITY,
        -1e300,
        0.0,
        0.45,
        9.6,
        18.4,
        1e18,
        1e300,
        f64::INFINITY,
    ];
    for cols in [0, 1, 9, 40, 80, 200] {
        for rows in [0, 1, 7, 24, 60, 120] {
            for aspect in aspects {
                for t in times {
                    sea.draw(cols, rows, aspect, tones, EVERYTHING, t);
                    let glyphs = sea.glyphs();
                    assert_eq!(glyphs.len(), cols * rows);
                    let printable = |g: &u16| *g == 0 || (0x21..=0x7e).contains(g);
                    assert!(
                        glyphs.iter().all(printable),
                        "{cols}x{rows}, a cell {aspect} wide, at {t}"
                    );
                }
            }
        }
    }
    // What it makes of nonsense: no time is the start, the far future is as far as it goes, and
    // a cell of no shape is your program's 0.5, within what `--aspect` allowed.
    assert!(picture(80, 24, 0.5, f64::NAN) == picture(80, 24, 0.5, 0.0));
    assert!(picture(80, 24, 0.5, -3.0) == picture(80, 24, 0.5, 0.0));
    assert!(picture(80, 24, 0.5, f64::INFINITY) == picture(80, 24, 0.5, 0.0));
    assert!(picture(80, 24, 0.5, 1e300) == picture(80, 24, 0.5, 1e12));
    assert!(picture(80, 24, f64::NAN, 9.6) == picture(80, 24, 0.5, 9.6));
    assert!(picture(80, 24, 0.0, 9.6) == picture(80, 24, 0.5, 9.6));
    assert!(picture(80, 24, 1e9, 9.6) == picture(80, 24, 1.0, 9.6));
    assert!(picture(80, 24, 1e-9, 9.6) == picture(80, 24, 0.3, 9.6));
}

/// 50 x 30 at 9.6 s, the moment your `--still` took, with the water and without the shoal. It
/// has drifted left, and one of the water's glints is behind its tentacles.
const STILL: &str = r#"
                    .
               . /----\
             /--/&&88u{\-\
     .      /0QB&]QQ8erzzo\\                    .
           |00<>nn@@]xxzccx|
           |&8$$&a{{>}[x=t=;|
           |%#[]]nnn}t>{c!l!|
           |@>Q+8>eut+_vl_c!|+   *
           |>oB/_//\-/\\-/\_\|       *
        *  |n\//;||#|x|\|1+|\\        *
        .   /|||=|||||/\|_||||        *
    +       |\\|/|\|c||Q|||||||     .      ..
             \\| |8\\|||||||| |* +
     **       \|  \\\x|\||||| ||       +        *
            *  ||*|\\\|||||||  |
        .     +|| | 0\||!|\||| |
               ||\| |\||||\\\|||
          +    |\ \\ \||/| \\||\
               |\\| \\{||\   ||\\\ +
       .       | \\  \|//    |\\\\\|
 .             || \\\///     |   \|\\\
               \\ \\*//\|   ||    |.||
     .         |\\\ \|\ \   ||\     |
  .           \|/  \\|\\\\\*| \\\\ ||
       - - - - * - -\*|-| +      \\*
              /|\     \\\ + *
               |       /
                                             .
                             .            .
 ."#;

#[test]
fn a_moment_is_one_picture_whatever_was_drawn_before() {
    // One sea draws moments out of order, at three sizes, on two papers, with and without its
    // extras: each is what a new sea draws, down to the colour of a cell with nothing in it.
    let sizes = [(100, 40), (80, 24), (50, 30)];
    let moments = [18.6, 0.6, 9.6, 3.4, 303.0, 0.0, 18.6, 44.0, 2.5, 9.6];
    let mut old = Sea::new();
    for (i, t) in moments.into_iter().enumerate() {
        let (cols, rows) = sizes[i % 3];
        let (paper, text) = if i % 2 == 0 { DARK } else { LIGHT };
        let tones = Tones::for_theme(paper, text);
        let extras = Extras {
            water: i % 4 < 2,
            shoal: i % 3 == 0,
        };
        old.draw(cols, rows, 0.5, tones, extras, t);
        let mut new = Sea::new();
        new.draw(cols, rows, 0.5, tones, extras, t);
        assert!(old.glyphs() == new.glyphs(), "the letters at {t}");
        assert!(old.inks() == new.inks(), "their colours at {t}");
    }

    let mut sea = Sea::new();
    sea.draw(50, 30, 0.5, Tones::for_theme(DARK.0, DARK.1), WATER, 9.6);
    let letter = |g: &u16| char::from_u32(u32::from(*g).max(0x20)).unwrap();
    let rows: Vec<String> = sea
        .glyphs()
        .chunks(50)
        .map(|row| row.iter().map(letter).collect::<String>())
        .collect();
    let rows: Vec<&str> = rows.iter().map(|row| row.trim_end()).collect();
    assert!(rows.join("\n") == STILL[1..], "\n{}", rows.join("\n"));
}

#[test]
fn inks_stay_on_the_papers_far_side_and_the_animal_stays_readable_dark_or_light() {
    // On a dark paper your colours as they were; on a light one highlights go to the text's
    // colour and pigments deepen, since white wouldn't show.
    let dark = Tones::for_theme(DARK.0, DARK.1);
    let light = Tones::for_theme(LIGHT.0, LIGHT.1);
    assert_eq!((dark.paper, light.paper), (DARK.0, LIGHT.0));
    assert_eq!((dark.bright, dark.deepen), (0xffffff, 1.0));
    assert_eq!((light.bright, light.deepen), (LIGHT.1, 0.62));
    for tones in [dark, light] {
        let paper = luminance(tones.paper);
        let mut sea = Sea::new();
        let (mut body, mut faintest) = (0, f64::MAX);
        // 44 s: the opening, and two rounds of the eight coats with their fireworks.
        for frame in 0..=220 {
            let t = f64::from(frame) * 0.2;
            sea.draw(120, 49, 0.5, tones, EVERYTHING, t);
            for (cell, (&code, &ink)) in sea.glyphs().iter().zip(sea.inks()).enumerate() {
                if code == 0 {
                    continue;
                }
                // A fade ends at the paper, never beyond it.
                let beyond = if tones.paper == DARK.0 {
                    luminance(ink) < paper
                } else {
                    luminance(ink) > paper
                };
                assert!(!beyond, "{ink:06x} on {:06x} at {t}", tones.paper);
                if sea.is_animal(cell) {
                    body += 1;
                    faintest = contrast(ink, tones.paper).min(faintest);
                }
            }
        }
        assert!(body > 100_000, "an animal: {body} cells of it");
        assert!(
            faintest >= 1.15,
            "its faintest cell on {:06x}: {faintest:.3}",
            tones.paper
        );
    }
}
