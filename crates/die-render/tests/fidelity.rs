//! Offscreen fidelity: a real scene through Core Text and Metal to a texture, with the
//! pixels read back and measured. Ioskeley Mono Term at 16 pt on a 2x display, so the
//! cell is 19×40. These skip where there's no GPU backend, and the Linux cross-check
//! compiles them against the stub, so this file carries no platform cfg. Set
//! `DIE_RENDER_PNG_DIR` to have them write the pictures they measure.

use die_core::{Cell, Cursor, CursorShape, Glyph, Rgb, Scene, Underline};
use die_render::driver::{Config, Renderer};
use die_render::font::{Draw, RangeRule};
use die_render::font_dir;
use die_render::frame::{GlyphBitmap, Image, Plane};
use die_render::platform::backend::metal::Gpu;
use die_render::rows::Frames;
use std::path::PathBuf;

mod support;
use support::{BLUE, GREEN, RED, SoftGpu, blank, cell2x, cursor, fake_bitmap, put_with};

const WHITE: Rgb = Rgb(255, 255, 255);
const BLACK: Rgb = Rgb(0, 0, 0);
const CW: u32 = 19;
const CH: u32 = 40;

/// A renderer, or `None` to skip where this platform can't draw. A font that won't load
/// on a platform that can draw is a failure.
fn renderer(config: &Config) -> Option<Renderer> {
    match Renderer::new(config) {
        Ok(r) => Some(r),
        Err(e) if e == "no window backend on this platform yet" || e == "no Metal device" => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

fn term() -> Option<Renderer> {
    renderer(&Config::term(16.0, 2.0))
}

/// A blank grid, white on black.
fn page(cols: usize, rows: usize) -> Scene {
    let mut s = blank(cols, rows);
    s.background = BLACK;
    for c in &mut s.cells {
        *c = Cell::new(Glyph::Char(' '), WHITE, BLACK);
    }
    s
}

/// Write graphemes from a column, white on black.
fn text(s: &mut Scene, row: usize, col: usize, graphemes: &[&str]) {
    put_with(s, row, col, graphemes, |c| {
        c.fg = WHITE;
        c.bg = BLACK;
        c.underline_color = WHITE;
    });
}

/// Write each character of `line`, white on black.
fn line(s: &mut Scene, row: usize, col: usize, line: &str) {
    let chars: Vec<String> = line.chars().map(String::from).collect();
    let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
    text(s, row, col, &refs);
}

fn draw(r: &mut Renderer, s: &Scene) -> Image {
    let (w, h) = r.cell().grid(s.cols, s.rows);
    r.render(s, w, h).unwrap()
}

/// How much white a pixel of a white-on-black picture has, from 0 to 1.
fn cov(img: &Image, x: u32, y: u32) -> f64 {
    f64::from(img.rgb(x, y)[0]) / 255.0
}

/// A pixel rectangle's ink: the box around pixels that aren't near black.
fn ink_box(img: &Image, x0: u32, y0: u32, x1: u32, y1: u32) -> Option<(u32, u32, u32, u32)> {
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for y in y0..y1.min(img.h) {
        for x in x0..x1.min(img.w) {
            if img.rgb(x, y).iter().any(|&v| v > 40) {
                b = Some(match b {
                    None => (x, y, x + 1, y + 1),
                    Some((a, c, d, e)) => (a.min(x), c.min(y), d.max(x + 1), e.max(y + 1)),
                });
            }
        }
    }
    b
}

/// How many pixels of a rectangle aren't near black.
fn ink_in(img: &Image, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| img.rgb(x, y).iter().any(|&v| v > 40))
        .count()
}

/// A cell's ink, with one cell's room on each side, so ink that strays is counted.
fn cell_ink(img: &Image, row: u32, col: u32, span: u32) -> (u32, u32, u32, u32) {
    let (x0, y0) = (col * CW, row * CH);
    let around = x0.saturating_sub(CW);
    ink_box(img, around, y0, x0 + (span + 1) * CW, y0 + CH).expect("ink")
}

/// A PNG with no compression, so the tests need no dependency for it.
fn write_png(img: &Image, name: &str) {
    let Some(dir) = std::env::var_os("DIE_RENDER_PNG_DIR") else {
        return;
    };
    fn crc(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = vec![];
    for y in 0..img.h {
        raw.push(0);
        for x in 0..img.w {
            raw.extend_from_slice(&img.rgb(x, y));
        }
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&img.w.to_be_bytes());
    ihdr.extend_from_slice(&img.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::create_dir_all(&dir).unwrap();
    let path = PathBuf::from(dir).join(name);
    std::fs::write(&path, out).unwrap();
    eprintln!("wrote {}", path.display());
}

// Cells, advances and metrics.

#[test]
fn term_gives_a_19_by_40_cell_at_2x_and_a_9_by_20_one_at_1x() {
    let Some(r) = term() else { return };
    assert_eq!((r.cell().w, r.cell().h), (CW, CH));
    let r = renderer(&Config::term(16.0, 1.0)).unwrap();
    assert_eq!((r.cell().w, r.cell().h), (9, 20));
}

#[test]
fn a_font_that_will_not_load_is_an_error_that_names_it() {
    if term().is_none() {
        return;
    }
    let mut c = Config::term(16.0, 2.0);
    c.faces[2] = font_dir().join("no-such-font.ttf");
    let e = Renderer::new(&c).err().unwrap();
    assert!(e.contains("no-such-font.ttf"), "{e}");
    let e = Renderer::new(&Config::term(0.0, 2.0)).err().unwrap();
    assert_eq!(e, "font size 0 isn't a positive size");
}

#[test]
fn dashes_dots_arrows_and_circled_digits_keep_to_one_cell_in_term() {
    let Some(mut r) = term() else { return };
    let mut s = page(12, 1);
    for (col, g) in [(1, "—"), (4, "…"), (7, "→"), (10, "①")] {
        text(&mut s, 0, col, &[g]);
    }
    let img = draw(&mut r, &s);
    for col in [1, 4, 7, 10] {
        let (x0, y0, x1, y1) = cell_ink(&img, 0, col, 1);
        // S1 measured an advance of 1.00 cell, and an em dash that reaches a pixel past.
        assert!(
            x0 + 1 >= col * CW && x1 <= (col + 1) * CW + 1,
            "col {col}: {x0}..{x1}"
        );
        assert!(y1 <= CH && y0 < y1, "col {col}");
    }
}

#[test]
fn a_long_arrow_that_term_draws_two_cells_wide_is_squeezed_into_one() {
    let Some(mut r) = term() else { return };
    let mut s = page(5, 1);
    text(&mut s, 0, 2, &["\u{27F5}"]);
    let img = draw(&mut r, &s);
    let (x0, _, x1, _) = cell_ink(&img, 0, 2, 1);
    assert!(x0 >= 2 * CW && x1 <= 3 * CW + 1, "{x0}..{x1}");
    assert!(x1 - x0 > CW / 2, "and it's still an arrow, not a dot");
}

// Box and block joins.

/// The rows with full ink at every pixel from x0 to x1.
fn full_rows(img: &Image, y0: u32, y1: u32, x0: u32, x1: u32) -> Vec<u32> {
    (y0..y1)
        .filter(|&y| (x0..x1).all(|x| cov(img, x, y) > 0.98))
        .collect()
}

/// The columns with full ink at every pixel from y0 to y1.
fn full_cols(img: &Image, x0: u32, x1: u32, y0: u32, y1: u32) -> Vec<u32> {
    (x0..x1)
        .filter(|&x| (y0..y1).all(|y| cov(img, x, y) > 0.98))
        .collect()
}

#[test]
fn box_lines_join_across_cells_with_no_seam() {
    let Some(mut r) = term() else { return };
    let mut s = page(14, 4);
    line(&mut s, 0, 0, &"─".repeat(12));
    for row in 1..4 {
        line(&mut s, row, 0, "│");
        line(&mut s, row, 2, "┼┼┼┼");
        line(&mut s, row, 8, "╬╬╬╬");
    }
    let img = draw(&mut r, &s);
    write_png(&img, "joins.png");
    let thin = r.cell().thin as usize;
    assert_eq!(
        full_rows(&img, 0, CH, 0, 12 * CW).len(),
        thin,
        "─ across 12 cells"
    );
    assert_eq!(
        full_cols(&img, 0, CW, CH, 4 * CH).len(),
        thin,
        "│ down 3 cells"
    );
    let (grid_x, grid_y) = (2 * CW, CH);
    assert_eq!(
        full_rows(&img, grid_y, 2 * CH, grid_x, grid_x + 4 * CW).len(),
        thin
    );
    assert_eq!(full_cols(&img, grid_x, grid_x + CW, CH, 4 * CH).len(), thin);
    // ╬'s rails run unbroken across no joins: each cell's center cross is open.
    let (x, y) = (8 * CW + CW / 2, CH + CH / 2);
    assert_eq!(cov(&img, x, y), 0.0, "╬'s center is open");
    assert_eq!(
        cov(&img, x - CW / 2, y),
        0.0,
        "and so is the gap at its edge"
    );
}

#[test]
fn a_heavy_line_is_twice_a_light_one_and_joins_too() {
    let Some(mut r) = term() else { return };
    let mut s = page(8, 3);
    line(&mut s, 0, 0, "━━━━");
    for row in 1..3 {
        line(&mut s, row, 0, "┃");
    }
    let img = draw(&mut r, &s);
    let thin = 2 * r.cell().thin as usize;
    assert_eq!(full_rows(&img, 0, CH, 0, 4 * CW).len(), thin);
    assert_eq!(full_cols(&img, 0, CW, CH, 3 * CH).len(), thin);
}

#[test]
fn blocks_fill_their_cells_and_shades_stay_even_across_a_join() {
    let Some(mut r) = term() else { return };
    let mut s = page(9, 3);
    for row in 0..3 {
        line(&mut s, row, 0, "████");
        line(&mut s, row, 5, "▒▒▒▒");
    }
    let img = draw(&mut r, &s);
    for y in 0..3 * CH {
        for x in 0..4 * CW {
            assert_eq!(img.rgb(x, y), [255; 3], "█ at {x},{y}");
        }
        for x in 5 * CW..9 * CW {
            assert_eq!(img.rgb(x, y), img.rgb(5 * CW, 0), "▒ at {x},{y}");
        }
    }
    assert!((cov(&img, 5 * CW, 0) - 0.5).abs() < 0.01);
}

#[test]
fn a_range_handed_back_to_the_font_tiles_with_the_font_own_glyphs() {
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![];
    let Some(mut r) = renderer(&config) else {
        return;
    };
    let mut s = page(8, 4);
    line(&mut s, 0, 0, &"─".repeat(8));
    for row in 1..4 {
        line(&mut s, row, 0, "│");
        line(&mut s, row, 2, "██");
        line(&mut s, row, 5, "▒");
    }
    let img = draw(&mut r, &s);
    write_png(&img, "joins-font.png");
    let min_along = |y: u32| (0..8 * CW).map(|x| cov(&img, x, y)).fold(1.0, f64::min);
    assert!(
        (0..CH).any(|y| min_along(y) >= 0.7),
        "─ from the font joins"
    );
    let min_down = |x: u32| (CH..4 * CH).map(|y| cov(&img, x, y)).fold(1.0, f64::min);
    assert!((0..CW).any(|x| min_down(x) >= 0.7), "│ from the font joins");
    // The font's ▒ is a hatch, so it isn't even: that's how to tell who drew it.
    let shades: Vec<u8> = (5 * CW..6 * CW).map(|x| img.rgb(x, 2 * CH)[0]).collect();
    assert!(
        shades.iter().any(|&v| v != shades[0]),
        "the font drew it: {shades:?}"
    );
}

// Wide characters, fallback and marks.

#[test]
fn wide_characters_from_fallback_faces_take_two_cells_and_stay_in_them() {
    let Some(mut r) = term() else { return };
    let mut s = page(14, 1);
    for (col, g) in [(1, "中"), (5, "한"), (9, "😀")] {
        text(&mut s, 0, col, &[g]);
    }
    let img = draw(&mut r, &s);
    write_png(&img, "wide.png");
    for col in [1, 5, 9] {
        let (x0, y0, x1, y1) = cell_ink(&img, 0, col, 2);
        assert!(
            x0 >= col * CW && x1 <= (col + 2) * CW,
            "col {col}: {x0}..{x1}"
        );
        assert!(y1 <= CH, "col {col}: down to {y0}..{y1}");
        let (left, right) = (x0 - col * CW, (col + 2) * CW - x1);
        assert!(
            left.abs_diff(right) <= 2,
            "col {col} is centered: {left} and {right}"
        );
        assert!(x1 - x0 > CW, "col {col} uses both cells: {x0}..{x1}");
    }
}

#[test]
fn a_wide_character_term_draws_one_cell_wide_is_centered_across_its_two() {
    let Some(mut r) = term() else { return };
    let mut s = page(6, 1);
    text(&mut s, 0, 1, &["☰"]);
    let img = draw(&mut r, &s);
    write_png(&img, "wide-from-term.png");
    let (x0, _, x1, _) = cell_ink(&img, 0, 1, 2);
    assert!(x0 >= CW && x1 <= 3 * CW, "{x0}..{x1}");
    let (left, right) = (x0 - CW, 3 * CW - x1);
    assert!(left.abs_diff(right) <= 2, "{left} and {right}");
}

#[test]
fn a_fallback_glyph_whose_ink_starts_left_of_its_pen_stays_in_its_cell() {
    let Some(mut r) = term() else { return };
    // Thai sara am, whose ring reaches back over the character before it.
    let mut s = page(5, 1);
    text(&mut s, 0, 2, &["\u{E33}"]);
    let img = draw(&mut r, &s);
    let (x0, _, x1, _) = cell_ink(&img, 0, 2, 1);
    assert!(x0 >= 2 * CW && x1 <= 3 * CW, "{x0}..{x1}");
}

#[test]
fn emoji_keep_their_own_colors_and_text_takes_the_cell_color() {
    let Some(mut r) = term() else { return };
    let mut s = page(6, 1);
    put_with(&mut s, 0, 0, &["😀"], |c| {
        c.fg = GREEN;
        c.bg = BLACK;
    });
    put_with(&mut s, 0, 3, &["M"], |c| {
        c.fg = GREEN;
        c.bg = BLACK;
    });
    let img = draw(&mut r, &s);
    let colors = |x0: u32, x1: u32| -> Vec<[u8; 3]> {
        (0..CH)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| img.rgb(x, y))
            .collect()
    };
    let yellow = |p: &[u8; 3]| p[0] > 180 && p[1] > 140 && p[2] < 120;
    assert!(
        colors(0, 2 * CW).iter().any(yellow),
        "😀 is yellow, not green"
    );
    assert!(
        colors(3 * CW, 4 * CW).contains(&[GREEN.0, GREEN.1, GREEN.2]),
        "M is green"
    );
    assert!(!colors(3 * CW, 4 * CW).iter().any(yellow));
}

/// A cluster's ink beside its plain base's, rendered in one frame.
fn with_and_without(r: &mut Renderer, cluster: &str, base: &str) -> Image {
    let mut s = page(7, 1);
    text(&mut s, 0, 1, &[cluster]);
    text(&mut s, 0, 4, &[base]);
    draw(r, &s)
}

#[test]
fn a_combining_mark_sits_above_its_base_inside_the_cell() {
    let Some(mut r) = term() else { return };
    let img = with_and_without(&mut r, "e\u{301}", "e");
    write_png(&img, "mark.png");
    let (mx0, my0, mx1, my1) = cell_ink(&img, 0, 1, 1);
    let (bx0, by0, bx1, by1) = cell_ink(&img, 0, 4, 1);
    assert!(my0 < by0 - 3, "the mark rises above the e: {my0} and {by0}");
    assert_eq!(my1, by1, "on the same baseline");
    assert!(mx0 >= CW && mx1 <= 2 * CW + 1, "in its cell: {mx0}..{mx1}");
    assert_eq!(
        (mx0 - CW, mx1 - CW),
        (bx0 - 4 * CW, bx1 - 4 * CW),
        "over the e, not beside it"
    );
    // Under the mark, the e is the same e.
    for y in by0..by1 {
        for x in 0..CW {
            assert_eq!(img.rgb(CW + x, y), img.rgb(4 * CW + x, y), "at {x},{y}");
        }
    }
}

#[test]
fn a_decomposed_letter_looks_like_the_precomposed_one() {
    let Some(mut r) = term() else { return };
    let mut s = page(4, 1);
    text(&mut s, 0, 0, &["e\u{301}"]);
    text(&mut s, 0, 2, &["é"]);
    let img = draw(&mut r, &s);
    let (a, b) = (cell_ink(&img, 0, 0, 1), cell_ink(&img, 0, 2, 1));
    assert_eq!((a.1, a.3), (b.1, b.3), "the same height");
    assert!(
        a.0.abs_diff(b.0 - 2 * CW) <= 1,
        "the same place: {a:?} {b:?}"
    );
}

#[test]
fn stacked_marks_go_both_above_and_below() {
    let Some(mut r) = term() else { return };
    let img = with_and_without(&mut r, "a\u{323}\u{301}", "a");
    let (_, my0, _, my1) = cell_ink(&img, 0, 1, 1);
    let (_, by0, _, by1) = cell_ink(&img, 0, 4, 1);
    assert!(my0 < by0, "above: {my0} and {by0}");
    assert!(my1 > by1, "below: {my1} and {by1}");
}

#[test]
fn clusters_the_primary_font_lacks_come_from_the_system_whole() {
    let Some(mut r) = term() else { return };
    // Devanagari with a vowel sign, a family emoji joined by ZWJ, a flag, and a heart
    // asking for its emoji form.
    let mut s = page(20, 1);
    text(&mut s, 0, 1, &["क\u{93F}"]);
    text(&mut s, 0, 5, &["👩\u{200D}💻"]);
    text(&mut s, 0, 9, &["🇦🇺"]);
    text(&mut s, 0, 13, &["\u{2764}\u{FE0F}"]);
    let img = draw(&mut r, &s);
    write_png(&img, "clusters.png");
    let colored = |col: u32| {
        (0..CH).any(|y| {
            (col * CW..(col + 2) * CW).any(|x| {
                let [a, b, c] = img.rgb(x, y);
                a.abs_diff(b) > 40 || b.abs_diff(c) > 40
            })
        })
    };
    assert!(
        ink_in(&img, CW, 0, 3 * CW, CH) > 50,
        "the vowel sign's cluster draws"
    );
    for col in [5, 9, 13] {
        let (x0, _, x1, _) = cell_ink(&img, 0, col, 2);
        assert!(
            x0 >= col * CW && x1 <= (col + 2) * CW,
            "col {col}: {x0}..{x1}"
        );
        assert!(colored(col), "col {col} is in color");
    }
}

#[test]
fn a_face_the_rules_pin_goes_first_and_one_without_the_glyph_falls_through() {
    let italic = font_dir().join("IoskeleyMonoTerm-Italic.ttf");
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![
        RangeRule::new('A'..='A', Draw::Files(vec![italic.clone()])),
        RangeRule::new('中'..='中', Draw::Files(vec![italic])),
    ];
    let Some(mut pinned) = renderer(&config) else {
        return;
    };
    let mut plain = renderer(&Config::term(16.0, 2.0)).unwrap();
    let mut s = page(6, 1);
    line(&mut s, 0, 0, "A");
    text(&mut s, 0, 2, &["中"]);
    let got = draw(&mut pinned, &s);
    let mut want = page(6, 1);
    put_with(&mut want, 0, 0, &["A"], |c| {
        c.fg = WHITE;
        c.bg = BLACK;
        c.italic = true;
    });
    text(&mut want, 0, 2, &["中"]);
    assert!(
        got == draw(&mut plain, &want),
        "A from the pinned italic, 中 from the system"
    );
    let mut roman = page(6, 1);
    line(&mut roman, 0, 0, "A");
    text(&mut roman, 0, 2, &["中"]);
    assert!(
        got != draw(&mut plain, &roman),
        "and it isn't the regular A"
    );
}

/// Each letter in its own style, drawn by a renderer with no rules.
fn styled_letters(letters: &[(&str, bool, bool)]) -> Image {
    let mut plain = renderer(&Config::term(16.0, 2.0)).unwrap();
    let mut s = page(letters.len() * 2, 1);
    for (i, &(letter, bold, italic)) in letters.iter().enumerate() {
        put_with(&mut s, 0, i * 2, &[letter], |c| {
            c.fg = WHITE;
            c.bg = BLACK;
            c.bold = bold;
            c.italic = italic;
        });
    }
    draw(&mut plain, &s)
}

#[test]
fn each_rule_draws_with_the_files_it_pins_and_no_other_rule_s() {
    let (italic, bold) = (
        font_dir().join("IoskeleyMonoTerm-Italic.ttf"),
        font_dir().join("IoskeleyMonoTerm-Bold.ttf"),
    );
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![
        RangeRule::new('A'..='A', Draw::Files(vec![italic])),
        RangeRule::new('B'..='B', Draw::Files(vec![bold])),
    ];
    let Some(mut pinned) = renderer(&config) else {
        return;
    };
    let mut s = page(6, 1);
    line(&mut s, 0, 0, "A");
    line(&mut s, 0, 2, "B");
    line(&mut s, 0, 4, "C");
    let want = styled_letters(&[("A", false, true), ("B", true, false), ("C", false, false)]);
    assert!(draw(&mut pinned, &s) == want);
}

#[test]
fn a_rule_tries_its_files_in_order_and_goes_on_past_one_without_the_glyph() {
    let (italic, bold) = (
        font_dir().join("IoskeleyMonoTerm-Italic.ttf"),
        font_dir().join("IoskeleyMonoTerm-Bold.ttf"),
    );
    let emoji = PathBuf::from("/System/Library/Fonts/Apple Color Emoji.ttc");
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![
        RangeRule::new('A'..='A', Draw::Files(vec![bold.clone(), italic.clone()])),
        RangeRule::new('B'..='B', Draw::Files(vec![italic.clone(), bold])),
        RangeRule::new('C'..='C', Draw::Files(vec![emoji, italic])),
    ];
    let Some(mut pinned) = renderer(&config) else {
        return;
    };
    let mut s = page(6, 1);
    line(&mut s, 0, 0, "A");
    line(&mut s, 0, 2, "B");
    line(&mut s, 0, 4, "C");
    let want = styled_letters(&[("A", true, false), ("B", false, true), ("C", false, true)]);
    assert!(draw(&mut pinned, &s) == want);
}

#[test]
fn a_face_the_rules_pin_is_centered_in_its_cell_as_a_fallback_is() {
    let avenir = PathBuf::from("/System/Library/Fonts/Avenir.ttc");
    if !avenir.exists() {
        eprintln!("skipped: no Avenir here");
        return;
    }
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![RangeRule::new('i'..='i', Draw::Files(vec![avenir]))];
    let Some(mut r) = renderer(&config) else {
        return;
    };
    let mut s = page(3, 1);
    line(&mut s, 0, 1, "i");
    let img = draw(&mut r, &s);
    let (x0, _, x1, _) = cell_ink(&img, 0, 1, 1);
    let (left, right) = (x0 - CW, 2 * CW - x1);
    assert!(left.abs_diff(right) <= 2, "{left} and {right}");
    assert!(x1 - x0 < CW / 2, "and it's Avenir's narrow i: {x0}..{x1}");
}

#[test]
fn a_box_glyph_with_a_mark_comes_from_the_font_since_the_renderer_has_no_marks() {
    let Some(mut r) = term() else { return };
    let img = with_and_without(&mut r, "─\u{301}", "─");
    let (_, y0, _, _) = cell_ink(&img, 0, 4, 1);
    assert!(
        ink_in(&img, CW, 0, 2 * CW, y0) > 0,
        "the mark, above the line"
    );
    assert_eq!(
        ink_in(&img, 4 * CW, 0, 5 * CW, y0),
        0,
        "and none on the plain line"
    );
}

// Braille.

/// The vertical centers of braille's dots, over the rows of a column of cells.
fn dot_rows(img: &Image, col0: u32, col1: u32, rows: u32) -> Vec<f64> {
    let ink: Vec<f64> = (0..rows * CH)
        .map(|y| (col0 * CW..col1 * CW).map(|x| cov(img, x, y)).sum())
        .collect();
    let threshold = ink.iter().copied().fold(0.0, f64::max) * 0.3;
    let (mut centers, mut y) = (vec![], 0);
    while y < ink.len() {
        if ink[y] > threshold {
            let start = y;
            while y < ink.len() && ink[y] > threshold {
                y += 1;
            }
            centers.push((start + y) as f64 / 2.0);
        } else {
            y += 1;
        }
    }
    centers
}

/// Three rows of four ⣿, and its dots' centers.
fn braille_centers(r: &mut Renderer, png: &str) -> Vec<f64> {
    let mut s = page(4, 3);
    for row in 0..3 {
        line(&mut s, row, 0, "⣿⣿⣿⣿");
    }
    let img = draw(r, &s);
    write_png(&img, png);
    dot_rows(&img, 0, 4, 3)
}

#[test]
fn braille_stays_in_the_font_by_default_and_each_dot_stays_inside_its_cell() {
    let Some(mut r) = term() else { return };
    let centers = braille_centers(&mut r, "braille-font.png");
    assert_eq!(centers.len(), 12, "{centers:?}");
    for (i, c) in centers.iter().enumerate() {
        let inside = c % f64::from(CH);
        assert!(
            inside > 1.0 && inside < f64::from(CH) - 1.0,
            "dot {i} at {c}"
        );
    }
    // S1's measure of the font: 9 px apart in a cell and 13 across its edge. The
    // renderer's own would be an even 10, so this says who drew them.
    assert_eq!(centers[1] - centers[0], 9.0, "{centers:?}");
    assert_eq!(centers[4] - centers[3], 13.0, "{centers:?}");
}

#[test]
fn a_rule_can_give_braille_to_the_renderer_at_an_even_pitch() {
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![RangeRule::new('\u{2800}'..='\u{28FF}', Draw::Renderer)];
    let Some(mut r) = renderer(&config) else {
        return;
    };
    let centers = braille_centers(&mut r, "braille-sprite.png");
    assert_eq!(centers.len(), 12, "{centers:?}");
    for pair in centers.windows(2) {
        assert_eq!(pair[1] - pair[0], 10.0, "{centers:?}");
    }
}

// Cursors.

const CELL_BG: Rgb = Rgb(30, 60, 90);

/// An H in white on a colored cell at (1, 1), with and without a cursor.
fn cursor_frames(r: &mut Renderer, shape: CursorShape) -> (Image, Image) {
    cursor_frames_on(r, shape, "H", false)
}

/// The same with this grapheme, italic or not.
fn cursor_frames_on(
    r: &mut Renderer,
    shape: CursorShape,
    grapheme: &str,
    italic: bool,
) -> (Image, Image) {
    let mut s = page(3, 3);
    put_with(&mut s, 1, 1, &[grapheme], |c| {
        c.fg = WHITE;
        c.bg = CELL_BG;
        c.italic = italic;
    });
    let without = draw(r, &s);
    s.cursors = vec![cursor(1, 1, shape)];
    let with = draw(r, &s);
    assert_eq!(r.rebuilt(), [] as [usize; 0], "a cursor rebuilds no row");
    (without, with)
}

const RGB_RED: [u8; 3] = [RED.0, RED.1, RED.2];
const RGB_BG: [u8; 3] = [CELL_BG.0, CELL_BG.1, CELL_BG.2];

/// Check every pixel of a 3×3 frame: outside the cursor's cell nothing changed, and
/// inside it `want(x, y, before, after)` holds, with x and y from the cell's corner.
fn each_pixel(without: &Image, with: &Image, want: impl Fn(u32, u32, [u8; 3], [u8; 3])) {
    for y in 0..3 * CH {
        for x in 0..3 * CW {
            let (a, b) = (without.rgb(x, y), with.rgb(x, y));
            if (CW..2 * CW).contains(&x) && (CH..2 * CH).contains(&y) {
                want(x - CW, y - CH, a, b);
            } else {
                assert_eq!(a, b, "a pixel outside the cell moved, at {x},{y}");
            }
        }
    }
}

#[test]
fn a_block_cursor_fills_the_cell_and_draws_the_glyph_in_the_cell_background() {
    let Some(mut r) = term() else { return };
    let (without, with) = cursor_frames(&mut r, CursorShape::Block);
    write_png(&with, "cursor-block.png");
    // The glyph's solid pixels turn to the cell's background, and the rest to the cursor.
    each_pixel(&without, &with, |x, y, a, b| {
        if a == [255; 3] {
            assert_eq!(b, RGB_BG, "the H's own pixel at {x},{y}");
        }
        if a == RGB_BG {
            assert_eq!(b, RGB_RED, "the cell around the H, at {x},{y}");
        }
    });
    assert!(
        (0..CH * CW).any(|i| with.rgb(CW + i % CW, CH + i / CW) == RGB_BG),
        "an H to see"
    );
}

#[test]
fn a_bar_cursor_covers_4_physical_px_at_the_cell_left_edge_and_leaves_the_rest() {
    let Some(mut r) = term() else { return };
    let (without, with) = cursor_frames(&mut r, CursorShape::Bar);
    write_png(&with, "cursor-bar.png");
    each_pixel(&without, &with, |x, y, a, b| {
        if x < 4 {
            assert_eq!(b, RGB_RED, "the bar, at {x},{y}");
        } else {
            assert_eq!(a, b, "beside the bar, at {x},{y}");
        }
    });
}

#[test]
fn an_underline_cursor_covers_4_physical_px_at_the_cell_bottom_and_leaves_the_rest() {
    let Some(mut r) = term() else { return };
    let (without, with) = cursor_frames(&mut r, CursorShape::Underline);
    write_png(&with, "cursor-underline.png");
    each_pixel(&without, &with, |x, y, a, b| {
        if y >= CH - 4 {
            assert_eq!(b, RGB_RED, "the bar, at {x},{y}");
        } else {
            assert_eq!(a, b, "above the bar, at {x},{y}");
        }
    });
}

#[test]
fn a_half_block_cursor_hides_the_lower_half_of_the_glyph_and_leaves_the_upper() {
    let Some(mut r) = term() else { return };
    let (without, with) = cursor_frames(&mut r, CursorShape::HalfBlock);
    write_png(&with, "cursor-half.png");
    each_pixel(&without, &with, |x, y, a, b| {
        if y >= CH / 2 {
            assert_eq!(b, RGB_RED, "the half, at {x},{y}");
        } else {
            assert_eq!(a, b, "above the half, at {x},{y}");
        }
    });
}

#[test]
fn a_hollow_cursor_outlines_the_cell_and_draws_the_glyph_as_it_is() {
    let Some(mut r) = term() else { return };
    let (without, with) = cursor_frames(&mut r, CursorShape::Hollow);
    write_png(&with, "cursor-hollow.png");
    each_pixel(&without, &with, |x, y, a, b| {
        let inside = (2..CW - 2).contains(&x) && (2..CH - 2).contains(&y);
        if inside {
            assert_eq!(a, b, "inside the outline, at {x},{y}");
        } else {
            assert_eq!(b, RGB_RED, "the outline, at {x},{y}");
        }
    });
}

#[test]
fn a_block_cursor_changes_no_pixel_outside_its_cell_whatever_its_glyph_overruns() {
    let Some(mut r) = term() else { return };
    for (grapheme, italic) in [
        ("x\u{20DD}", false),
        ("a\u{301}\u{301}\u{301}\u{301}", false),
        ("—", false),
        ("f", true),
        ("j", true),
    ] {
        let (without, with) = cursor_frames_on(&mut r, CursorShape::Block, grapheme, italic);
        write_png(&with, "cursor-overrun.png");
        // This also says the part of it inside the cell is where it was.
        each_pixel(&without, &with, |x, y, a, b| {
            if a == [255; 3] {
                assert_eq!(b, RGB_BG, "{grapheme}'s own pixel at {x},{y}");
            }
        });
    }
}

#[test]
fn a_block_cursor_on_a_wide_character_keeps_its_lines_across_both_cells() {
    let Some(mut r) = term() else { return };
    let mut s = page(4, 1);
    put_with(&mut s, 0, 1, &["中"], |c| {
        c.fg = WHITE;
        c.bg = CELL_BG;
        c.underline = Underline::Single;
        c.underline_color = GREEN;
        c.strike = true;
    });
    s.cursors = vec![cursor(0, 1, CursorShape::Block)];
    let img = draw(&mut r, &s);
    write_png(&img, "cursor-wide-lines.png");
    for y in [r.cell().underline_y as u32, r.cell().strike_y as u32] {
        assert_eq!(
            columns_in(&img, y, CW, 3 * CW, CELL_BG).len() as u32,
            2 * CW
        );
    }
}

/// mono-dark's text and background colors.
const MONO_FG: Rgb = Rgb(0xCC, 0xCC, 0xCC);
const MONO_BG: Rgb = Rgb(0x1A, 0x1A, 0x1A);

/// Its current search match with a cursor on it: an H in the background's color on a
/// cell of the cursor's color. What die-core gives the match that starts at a cursor.
fn on_a_current_match(r: &mut Renderer, shape: CursorShape) -> (Image, Image) {
    let mut s = page(3, 3);
    s.background = MONO_BG;
    for c in &mut s.cells {
        c.bg = MONO_BG;
    }
    put_with(&mut s, 1, 1, &["H"], |c| {
        c.fg = MONO_BG;
        c.bg = MONO_FG;
    });
    let without = draw(r, &s);
    s.cursors = vec![Cursor {
        color: MONO_FG,
        ..cursor(1, 1, shape)
    }];
    (without, draw(r, &s))
}

#[test]
fn every_cursor_on_a_search_match_of_its_own_color_stays_visible_and_keeps_the_letter() {
    let Some(mut r) = term() else { return };
    let (fg, bg) = ([MONO_FG.0; 3], [MONO_BG.0; 3]);
    let (without, with) = on_a_current_match(&mut r, CursorShape::Block);
    write_png(&with, "cursor-match-block.png");
    each_pixel(&without, &with, |x, y, a, b| {
        if a == fg {
            assert_eq!(b, bg, "around the H, at {x},{y}");
        }
        if a == bg {
            assert_eq!(b, fg, "the H's own pixel, at {x},{y}");
        }
    });
    let mut letter = 0;
    for y in CH..2 * CH {
        letter += columns_in(&with, y, CW, 2 * CW, MONO_FG).len();
    }
    assert!(letter > 50, "an H to see: {letter} pixels");
    for (shape, covered) in [
        (CursorShape::Bar, (|x, _| x < 4) as fn(u32, u32) -> bool),
        (CursorShape::Underline, |_, y| y >= CH - 4),
        (CursorShape::HalfBlock, |_, y| y >= CH / 2),
        (CursorShape::Hollow, |x, y| {
            !((2..CW - 2).contains(&x) && (2..CH - 2).contains(&y))
        }),
    ] {
        let (without, with) = on_a_current_match(&mut r, shape);
        each_pixel(&without, &with, |x, y, a, b| {
            if covered(x, y) {
                assert_eq!(b, bg, "{shape:?} in the cell's foreground, at {x},{y}");
            } else {
                assert_eq!(a, b, "beside {shape:?}, at {x},{y}");
            }
        });
    }
}

#[test]
fn at_1x_the_bar_and_outline_are_their_logical_pixels_wide() {
    let Some(mut r) = renderer(&Config::term(16.0, 1.0)) else {
        return;
    };
    let (cw, ch) = (r.cell().w, r.cell().h);
    let mut s = page(3, 3);
    s.cursors = vec![cursor(1, 1, CursorShape::Bar)];
    let img = draw(&mut r, &s);
    assert_eq!(img.rgb(cw, ch), RGB_RED);
    assert_eq!(img.rgb(cw + 1, ch), RGB_RED);
    assert_eq!(img.rgb(cw + 2, ch), [0; 3]);
    s.cursors = vec![cursor(1, 1, CursorShape::Hollow)];
    let img = draw(&mut r, &s);
    assert_eq!(img.rgb(cw, ch + 5), RGB_RED);
    assert_eq!(img.rgb(cw + 1, ch + 5), [0; 3]);
}

#[test]
fn a_cursor_on_a_wide_character_covers_it_and_on_an_emoji_leaves_its_colors() {
    let Some(mut r) = term() else { return };
    let mut s = page(6, 1);
    text(&mut s, 0, 1, &["中"]);
    text(&mut s, 0, 3, &["😀"]);
    s.cursors = vec![
        cursor(0, 1, CursorShape::Block),
        cursor(0, 3, CursorShape::Block),
    ];
    let img = draw(&mut r, &s);
    write_png(&img, "cursor-wide.png");
    assert_eq!(img.rgb(CW, 0), RGB_RED, "its first cell");
    assert_eq!(img.rgb(3 * CW - 1, CH - 1), RGB_RED, "its second cell");
    assert_eq!(img.rgb(3 * CW, 0), RGB_RED, "the emoji's fill");
    let center = img.rgb(4 * CW, CH / 2);
    assert!(
        center != RGB_RED && center != [0; 3],
        "the emoji on top: {center:?}"
    );
}

// Attributes.

#[test]
fn bold_is_heavier_and_italic_leans_and_bold_italic_is_both() {
    let Some(mut r) = term() else { return };
    let mut s = page(8, 1);
    let styled = |bold, italic| {
        move |c: &mut Cell| {
            c.fg = WHITE;
            c.bg = BLACK;
            c.bold = bold;
            c.italic = italic;
        }
    };
    put_with(&mut s, 0, 0, &["l"], styled(false, false));
    put_with(&mut s, 0, 2, &["l"], styled(true, false));
    put_with(&mut s, 0, 4, &["l"], styled(false, true));
    put_with(&mut s, 0, 6, &["l"], styled(true, true));
    let img = draw(&mut r, &s);
    write_png(&img, "faces.png");
    let total = |col: u32| -> f64 {
        (0..CH)
            .flat_map(|y| (col * CW..(col + 1) * CW).map(move |x| (x, y)))
            .map(|(x, y)| cov(&img, x, y))
            .sum()
    };
    assert!(
        total(2) > total(0) * 1.15,
        "bold has more ink: {} {}",
        total(2),
        total(0)
    );
    assert!(
        total(6) > total(4) * 1.15,
        "bold italic too: {} {}",
        total(6),
        total(4)
    );
    // The middle of the ink, at the top of the stem and at its bottom.
    let middle = |col: u32, y0: u32, y1: u32| -> f64 {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in y0..y1 {
            for x in col * CW..(col + 1) * CW {
                sum += cov(&img, x, y) * f64::from(x);
                n += cov(&img, x, y);
            }
        }
        sum / n
    };
    let lean = |col: u32| middle(col, 8, 14) - middle(col, 20, 26);
    assert!(
        lean(4) > lean(0) + 1.0,
        "italic leans right: {} {}",
        lean(4),
        lean(0)
    );
    assert!(
        lean(6) > lean(2) + 1.0,
        "bold italic too: {} {}",
        lean(6),
        lean(2)
    );
}

#[test]
fn a_glyph_takes_the_cell_colors_and_a_blank_cell_shows_its_background() {
    let Some(mut r) = term() else { return };
    let mut s = page(4, 1);
    put_with(&mut s, 0, 1, &["█"], |c| {
        c.fg = GREEN;
        c.bg = BLUE;
    });
    put_with(&mut s, 0, 2, &[" "], |c| {
        c.fg = GREEN;
        c.bg = BLUE;
    });
    let img = draw(&mut r, &s);
    assert_eq!(img.rgb(CW + 3, 3), [GREEN.0, GREEN.1, GREEN.2]);
    assert_eq!(img.rgb(2 * CW + 3, 3), [BLUE.0, BLUE.1, BLUE.2]);
    assert_eq!(img.rgb(3, 3), [0; 3]);
}

/// A row of a cell's pixels in a color, as a set of its columns.
fn columns_in(img: &Image, y: u32, x0: u32, x1: u32, c: Rgb) -> Vec<u32> {
    (x0..x1)
        .filter(|&x| img.rgb(x, y) == [c.0, c.1, c.2])
        .collect()
}

/// Rows of a rectangle that hold the color.
fn rows_with(img: &Image, x0: u32, x1: u32, c: Rgb) -> Vec<u32> {
    (0..img.h)
        .filter(|&y| !columns_in(img, y, x0, x1, c).is_empty())
        .collect()
}

#[test]
fn each_underline_style_is_drawn_in_its_own_color_below_the_baseline() {
    let Some(mut r) = term() else { return };
    let styles = [
        Underline::Single,
        Underline::Double,
        Underline::Curly,
        Underline::Dotted,
        Underline::Dashed,
    ];
    let mut s = page(16, 1);
    for (i, ul) in styles.iter().enumerate() {
        let graphemes = ["u", "n", "d"];
        put_with(&mut s, 0, i * 3 + 1, &graphemes, |c| {
            c.fg = WHITE;
            c.bg = BLACK;
            c.underline = *ul;
            c.underline_color = RED;
        });
    }
    let img = draw(&mut r, &s);
    write_png(&img, "underlines.png");
    let at = |i: u32| (i * 3 + 1) * CW;
    let y = r.cell().underline_y as u32;
    let thin = r.cell().thin;
    let baseline = r.cell().baseline.round() as u32;
    let single = rows_with(&img, at(0), at(0) + 3 * CW, RED);
    assert_eq!(single, (y..y + thin).collect::<Vec<_>>(), "single");
    assert!(y >= baseline, "below the baseline");
    assert_eq!(
        columns_in(&img, y, at(0), at(0) + 3 * CW, RED).len() as u32,
        3 * CW,
        "unbroken"
    );
    let double = rows_with(&img, at(1), at(1) + 3 * CW, RED);
    assert_eq!(double.len() as u32, 2 * thin, "double: {double:?}");
    assert!(
        double[double.len() - 1] - double[0] >= 3 * thin - 1,
        "with a gap: {double:?}"
    );
    let curly: Vec<u32> = (at(2)..at(2) + 3 * CW)
        .map(|x| {
            (0..CH)
                .map(|py| u32::from(img.rgb(x, py)[0]) * py)
                .sum::<u32>()
        })
        .collect();
    assert!(curly.iter().any(|&v| v != curly[0]), "curly waves");
    let dotted = columns_in(&img, y, at(3), at(3) + 3 * CW, RED).len() as u32;
    assert!(
        dotted > CW && dotted < 2 * CW,
        "dotted is about half lit: {dotted}"
    );
    let dashed = columns_in(&img, y, at(4), at(4) + 3 * CW, RED).len() as u32;
    assert!(
        dashed > 2 * CW && dashed < 3 * CW,
        "dashed is about 3/4 lit: {dashed}"
    );
}

#[test]
fn a_struck_cell_has_a_band_of_its_foreground_across_its_middle() {
    let Some(mut r) = term() else { return };
    let mut s = page(6, 1);
    put_with(&mut s, 0, 1, &["s", "t"], |c| {
        c.fg = GREEN;
        c.bg = BLACK;
        c.strike = true;
    });
    let img = draw(&mut r, &s);
    write_png(&img, "strike.png");
    let (y, thin) = (r.cell().strike_y as u32, r.cell().thin);
    for row in y..y + thin {
        let lit = columns_in(&img, row, CW, 3 * CW, GREEN).len() as u32;
        assert_eq!(lit, 2 * CW, "row {row}");
    }
    assert!(
        y < r.cell().baseline as u32 && y > 4,
        "in the letters' middle: {y}"
    );
}

#[test]
fn underlining_a_wide_character_covers_both_of_its_cells() {
    let Some(mut r) = term() else { return };
    let mut s = page(4, 1);
    put_with(&mut s, 0, 1, &["中"], |c| {
        c.fg = WHITE;
        c.bg = BLACK;
        c.underline = Underline::Single;
        c.underline_color = RED;
    });
    let img = draw(&mut r, &s);
    let y = r.cell().underline_y as u32 + r.cell().thin - 1;
    assert_eq!(columns_in(&img, y, CW, 3 * CW, RED).len() as u32, 2 * CW);
}

// The frame as a whole.

#[test]
fn the_pixels_around_the_grid_are_the_scene_background() {
    let Some(mut r) = term() else { return };
    let mut s = page(2, 1);
    s.background = BLUE;
    line(&mut s, 0, 0, "█");
    let img = r.render(&s, 3 * CW, 2 * CH).unwrap();
    assert_eq!((img.w, img.h), (3 * CW, 2 * CH));
    assert_eq!(img.rgb(5, 5), [255; 3], "the grid is at the top left");
    assert_eq!(
        img.rgb(3 * CW - 1, 5),
        [BLUE.0, BLUE.1, BLUE.2],
        "right of it"
    );
    assert_eq!(img.rgb(5, 2 * CH - 1), [BLUE.0, BLUE.1, BLUE.2], "below it");
}

#[test]
fn a_target_with_no_pixels_is_an_empty_image() {
    let Some(mut r) = term() else { return };
    let img = r.render(&page(2, 1), 0, 40).unwrap();
    assert_eq!((img.w, img.h, img.bgra.len()), (0, 40, 0));
    let img = r.render(&page(2, 1), 40, 0).unwrap();
    assert_eq!((img.w, img.h, img.bgra.len()), (40, 0, 0));
}

#[test]
fn a_frame_with_no_instances_is_cleared_to_the_background() {
    let Some(mut r) = term() else { return };
    let mut s = page(2, 1);
    s.background = GREEN;
    for c in &mut s.cells {
        c.bg = GREEN;
    }
    let img = r.render(&s, 4, 4).unwrap();
    assert!(
        img.bgra
            .chunks(4)
            .all(|p| p == [GREEN.2, GREEN.1, GREEN.0, 255])
    );
}

#[test]
fn frames_on_the_gpu_match_a_fresh_renderer_as_rows_change_and_the_buffer_grows() {
    let Some(mut r) = term() else { return };
    let mut small = page(4, 1);
    line(&mut small, 0, 0, "ab");
    draw(&mut r, &small);
    // Many more instances than the first frame's buffer holds.
    let mut big = page(60, 30);
    for row in 0..30 {
        line(
            &mut big,
            row,
            0,
            &"the quick brown fox jumps over the lazy dog ".repeat(2)[..60],
        );
    }
    let got = draw(&mut r, &big);
    let want = draw(&mut term().unwrap(), &big);
    assert!(got == want, "the bigger frame");
    line(&mut big, 7, 3, "XYZ");
    let got = draw(&mut r, &big);
    assert_eq!(r.rebuilt(), [7]);
    let want = draw(&mut term().unwrap(), &big);
    assert!(got == want, "the row that changed");
}

#[test]
fn a_frame_that_fills_the_atlas_on_the_gpu_starts_over_and_draws_every_glyph() {
    let Some(mut r) = term() else { return };
    // About 2,500 of these fit: the driver has to ask again for what the atlas forgot.
    let page_from = |first: u32| {
        let mut s = page(100, 40);
        for row in 0..40 {
            for i in 0..50 {
                let c = char::from_u32(first + (row * 50 + i) as u32).unwrap();
                text(&mut s, row, i * 2, &[c.encode_utf8(&mut [0; 4])]);
            }
        }
        s
    };
    draw(&mut r, &page_from(0x4E00));
    let next = page_from(0x4E00 + 2000);
    let got = draw(&mut r, &next);
    assert_eq!(r.rebuilt().len(), 40);
    assert!(
        got == draw(&mut term().unwrap(), &next),
        "the same as a fresh renderer's"
    );
}

#[test]
fn a_character_no_font_has_draws_the_system_placeholder_inside_its_cell() {
    let Some(mut r) = term() else { return };
    let mut s = page(5, 1);
    text(&mut s, 0, 2, &["\u{10FFFD}"]);
    let img = draw(&mut r, &s);
    write_png(&img, "no-font.png");
    let (x0, _, x1, y1) = ink_box(&img, 0, 0, 5 * CW, CH).expect("the placeholder");
    assert!(
        x0 >= 2 * CW && x1 <= 3 * CW && y1 <= CH,
        "{x0}..{x1}, down to {y1}"
    );
    assert!(x1 - x0 > CW / 2, "squeezed, not a speck: {x0}..{x1}");
}

/// What a frame of stand-in bitmaps takes through both paints: the real GPU and the
/// plain Rust that states the rules. A color glyph is in it, half transparent.
#[test]
fn the_shader_blends_as_the_written_rules_say() {
    let Some(gpu) = Gpu::system_default() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let size = 512;
    let mut pipeline = gpu.pipeline(size, size).unwrap();
    let mut frames = Frames::new(cell2x(), size, size);
    let mut soft = SoftGpu::new(size);
    let mut s = blank(10, 3);
    for (row, t) in ["ab─┼█▒", "xyz", "M"].iter().enumerate() {
        support::put(&mut s, row, 1, t);
    }
    s.cells[12].bg = RED;
    s.cells[13].underline = Underline::Curly;
    s.cells[14].strike = true;
    s.cells[25].glyph = Glyph::Char('😀');
    s.cells[26].glyph = Glyph::Continuation;
    s.cursors = vec![
        cursor(0, 1, CursorShape::Block),
        cursor(2, 1, CursorShape::Hollow),
    ];
    let cell = *frames.cell();
    for key in frames.diff(&s) {
        let bitmap = match &key {
            die_render::atlas::GlyphKey::Text {
                glyph: Glyph::Char('😀'),
                ..
            } => {
                let mut data = vec![];
                for i in 0..16u32 * 16 {
                    // Premultiplied: half covered, and red as bright as half can be.
                    let a = if i % 2 == 0 { 128u8 } else { 255 };
                    data.extend_from_slice(&[0, 0, a, a]);
                }
                Some(GlyphBitmap {
                    w: 16,
                    h: 16,
                    left: 2,
                    top: 4,
                    plane: Plane::Color,
                    data,
                })
            }
            k => fake_bitmap(k, &cell),
        };
        frames.supply(&key, bitmap);
    }
    let f = frames.build(&s);
    for u in &f.uploads {
        pipeline.upload(u).unwrap();
        soft.upload(u);
    }
    let (w, h) = cell.grid(s.cols, s.rows);
    let got = pipeline.render(f.quads, f.clear, w, h).unwrap();
    let want = soft.draw(f.quads, f.clear, w, h);
    write_png(&got, "shader-gpu.png");
    write_png(&want, "shader-soft.png");
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (got.rgb(x, y), want.rgb(x, y));
            for i in 0..3 {
                assert!(
                    a[i].abs_diff(b[i]) <= 1,
                    "at {x},{y}: GPU {a:?}, written rules {b:?}"
                );
            }
        }
    }
}

/// The page S1 rendered, plus what S1 didn't draw, to compare by eye with
/// `spikes/s1-renderer/results/fidelity-term-*.png`.
fn the_page() -> Scene {
    let mut s = page(40, 22);
    line(&mut s, 0, 0, &"─".repeat(12));
    for row in 1..4 {
        line(&mut s, row, 0, "│");
        line(&mut s, row, 2, "┼┼┼┼");
        line(&mut s, row, 8, "████");
        line(&mut s, row, 14, "⣿⣿⣿⣿");
        line(&mut s, row, 20, "╬╬╬╬");
        line(&mut s, row, 26, "▒▒▒▒");
    }
    for (col, g) in [
        (1, "中"),
        (5, "😀"),
        (9, "—"),
        (12, "…"),
        (15, "→"),
        (18, "①"),
        (21, "한"),
    ] {
        text(&mut s, 5, col, &[g]);
    }
    line(&mut s, 7, 0, "fn main() { let x = 42; } // — … → ← ≠");
    line(&mut s, 8, 0, "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ▁▂▃▄▅▆▇█ ░▒▓ ▖▗▘▙▚▛▜▝▞▟");
    line(&mut s, 9, 0, "┌──┬──┐ ╭──╮ ╔══╦══╗ ┏━━┓ ┄┄┈┈");
    line(&mut s, 10, 0, "│  │  │ │  │ ║  ║  ║ ┃  ┃ ╱╲╳");
    line(&mut s, 11, 0, "└──┴──┘ ╰──╯ ╚══╩══╝ ┗━━┛ ╌╌");
    let styles = [
        Underline::Single,
        Underline::Double,
        Underline::Curly,
        Underline::Dotted,
        Underline::Dashed,
    ];
    for (i, ul) in styles.iter().enumerate() {
        let graphemes = ["u", "l", "i", "n", "e", "d"];
        put_with(&mut s, 12, i * 7, &graphemes, |c| {
            c.fg = WHITE;
            c.bg = BLACK;
            c.underline = *ul;
            c.underline_color = RED;
        });
    }
    line(
        &mut s,
        13,
        0,
        "\u{E0B0}\u{E0B1}\u{E0B2}\u{E0B3}\u{E0B4}\u{E0B6} powerline",
    );
    let mut styled = |col: usize, t: &[&str], f: &dyn Fn(&mut Cell)| {
        put_with(&mut s, 14, col, t, |c| {
            c.fg = WHITE;
            c.bg = BLACK;
            f(c);
        });
    };
    styled(0, &["b", "o", "l", "d"], &|c| c.bold = true);
    styled(5, &["i", "t", "a", "l", "i", "c"], &|c| c.italic = true);
    styled(12, &["b", "o", "t", "h"], &|c| {
        c.bold = true;
        c.italic = true;
    });
    styled(17, &["s", "t", "r", "i", "k", "e"], &|c| c.strike = true);
    put_with(&mut s, 14, 24, &["c", "o", "l", "o", "r"], |c| {
        c.fg = GREEN;
        c.bg = BLUE;
    });
    text(
        &mut s,
        16,
        0,
        &[
            "e\u{301}",
            " ",
            "n\u{303}",
            " ",
            "a\u{323}\u{301}",
            " ",
            "x\u{20DD}",
            " ",
            "क\u{93F}",
            " ",
            "👩\u{200D}💻",
            " ",
            "🇦🇺",
            " ",
            "\u{2764}\u{FE0F}",
        ],
    );
    line(&mut s, 18, 1, "b a r  u n d e r  h a l f  h o l l o w");
    line(&mut s, 20, 1, "block");
    text(&mut s, 20, 8, &["中"]);
    s.cursors = vec![
        cursor(18, 1, CursorShape::Bar),
        cursor(18, 8, CursorShape::Underline),
        cursor(18, 19, CursorShape::HalfBlock),
        cursor(18, 30, CursorShape::Hollow),
        cursor(20, 1, CursorShape::Block),
        cursor(20, 8, CursorShape::Block),
    ];
    s
}

#[test]
fn the_page_s1_drew_still_measures_as_s1_measured_it() {
    let Some(mut r) = term() else { return };
    let s = the_page();
    let img = draw(&mut r, &s);
    write_png(&img, "page-sprites.png");
    // S1's check: ─ across its 12 cells, ┼'s lines, and the block.
    assert_eq!(full_rows(&img, 0, CH, 0, 12 * CW).len(), 2);
    assert_eq!(full_cols(&img, 2 * CW, 3 * CW, CH, 4 * CH).len(), 2);
    for y in CH..4 * CH {
        for x in 8 * CW..12 * CW {
            assert_eq!(img.rgb(x, y), [255; 3], "█ at {x},{y}");
        }
    }
    let mut config = Config::term(16.0, 2.0);
    config.rules = vec![];
    let mut font_only = renderer(&config).unwrap();
    let img = draw(&mut font_only, &s);
    write_png(&img, "page-font_only.png");
}
