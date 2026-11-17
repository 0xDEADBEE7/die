//! The typesetter alone, with no DIE round it: that a page is where GitHub's sheet puts it, and
//! that any text at any width has a page that paints.
//!
//! The fonts are whatever the machine has. With none at all a page can't be made, and each test
//! says so and passes: what's held here is the layout, not which faces a machine carries.

use die_test_cap as _;

use die_markdown::plan::{Options, plan};
use die_markdown::typeset::{Env, Fonts, Marks, Page, Palette, Picture, Pictures};
use std::path::{Path, PathBuf};

fn fonts() -> Option<Fonts> {
    Fonts::load(None)
        .inspect_err(|why| eprintln!("skipped: {why}"))
        .ok()
}

fn env<'a>(width: u32, scale: f32, palette: &'a Palette, dir: Option<&'a Path>) -> Env<'a> {
    Env {
        width,
        scale,
        palette,
        dir,
        front_matter: false,
        tab_width: 8,
    }
}

/// A page of `text` in an area wide enough for the whole column, at scale 1: the column's
/// content starts 155 pixels in (110 of margin, 45 of padding) and 45 down.
fn page(text: &str, fonts: &mut Fonts) -> Page {
    let palette = Palette::mono_dark();
    Page::new(
        text,
        &env(1200, 1.0, &palette, None),
        fonts,
        &Pictures::new(),
    )
}

/// The top of the line that shows the first `needle` in `text`.
fn top_of(page: &Page, text: &str, needle: &str) -> f32 {
    page.y_of(text.find(needle).expect("the needle is in the text"))
}

#[track_caller]
fn near(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() <= 1.0,
        "{what}: {got}, the sheet says {want}"
    );
}

#[test]
fn the_sheets_worked_page_is_where_it_says() {
    let Some(mut fonts) = fonts() else { return };
    let text = "# Title\n\nPara\n\n## Sec\n\n- a\n- b\n\n```\ncode\n```\n";
    let page = page(text, &mut fonts);
    near(top_of(&page, text, "Title"), 45.0, "the first heading");
    near(
        top_of(&page, text, "Para"),
        111.6,
        "a paragraph, 16 under the heading's rule",
    );
    near(
        top_of(&page, text, "Sec"),
        159.6,
        "a heading, 24 under a paragraph",
    );
    near(
        top_of(&page, text, "- a"),
        213.8,
        "a list, 16 under the heading's rule",
    );
    near(top_of(&page, text, "- b"), 241.8, "the next item, 4 on");
    near(
        top_of(&page, text, "code"),
        297.8,
        "a code block's first line, 16 inside it",
    );
    near(page.height(), 378.52, "the page's end");
    let headings: Vec<(u8, &str)> = page
        .headings()
        .iter()
        .map(|h| (h.level, h.slug.as_str()))
        .collect();
    assert_eq!(headings, [(1, "title"), (2, "sec")]);
    near(page.headings()[1].y, 159.6, "the second heading");
}

#[test]
fn nested_lists_and_quotes_keep_githubs_rhythm_and_indents() {
    let Some(mut fonts) = fonts() else { return };
    // Every line a link, so the page says where its text starts.
    let text = "- [a](u)\n  - [b](u)\n  - [c](u)\n- [d](u)\n\n> [q1](u)\n>\n> [q2](u)\n";
    let page = page(text, &mut fonts);
    let at: Vec<(f32, f32)> = page
        .links()
        .iter()
        .map(|l| (l.rects[0].x, l.rects[0].y))
        .collect();
    assert_eq!(at.len(), 6);
    let (left, first) = (155.0, at[0].1);
    let want = [
        (32.0, 0.0, "a"),
        (64.0, 24.0, "b: no gap before a nested list"),
        (64.0, 52.0, "c: 4 between tight items"),
        (32.0, 80.0, "d"),
        (20.0, 120.0, "q1: 16 under the list"),
        (20.0, 160.0, "q2: 16 between a quote's paragraphs"),
    ];
    for ((x, y), (in_by, down, what)) in at.iter().zip(want) {
        near(*x, left + in_by, what);
        near(*y - first, down, what);
    }
}

#[test]
fn a_heading_a_code_block_and_a_table_row_are_as_tall_as_the_sheet_says() {
    let Some(mut fonts) = fonts() else { return };
    // A one-line first heading: 40 of line, 9.6 of padding, its rule; then 16.
    let text = "# T\n\nunder\n";
    near(
        top_of(&page(text, &mut fonts), text, "under"),
        45.0 + 50.6 + 16.0,
        "under an h1",
    );
    // A code block: 16 of padding above and below its lines, each 19.72, which a whole
    // number of pixels makes 20 at this scale.
    for lines in [1usize, 7, 40] {
        let text = format!("```\n{}```\n", "x\n".repeat(lines));
        let height = page(&text, &mut fonts).height() - 90.0;
        assert_eq!(height, 32.0 + 20.0 * lines as f32, "{lines} lines of code");
        assert!((height - (32.0 + 19.72 * lines as f32)).abs() <= 0.28 * lines as f32 + 0.01);
    }
    // A table: 37 a row, its line included, and one line more to close it.
    for rows in [1usize, 2, 9] {
        let text = format!("| a | b |\n|---|---|\n{}", "| 1 | 2 |\n".repeat(rows));
        let height = page(&text, &mut fonts).height() - 90.0;
        assert_eq!(
            height,
            37.0 * (rows + 1) as f32 + 1.0,
            "a header and {rows} rows"
        );
    }
}

#[test]
fn letters_are_kerned() {
    let Some(mut fonts) = fonts() else { return };
    // DIE's own monospace, the last resort, has nothing to kern.
    if fonts.found().starts_with("Ioskeley") {
        return;
    }
    let (kerned, plain) = fonts.measure("To", 2.0);
    assert!(
        kerned < plain - 0.5,
        "To is {kerned} wide, and {plain} unkerned"
    );
}

/// The markdown files to lay out: the repository's documents and the two sample pages.
fn documents() -> Vec<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in [here.join("../../docs"), here.join("tests/pages")] {
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let paths = entries.filter_map(|e| Some(e.ok()?.path()));
        files.extend(paths.filter(|p| p.extension().is_some_and(|e| e == "md")));
    }
    files.sort();
    assert!(files.len() >= 3, "the documents and the samples are there");
    files
}

/// A picture file for the sample page to show: a small PNG with some of it see-through.
fn sample_picture(dir: &Path) {
    std::fs::create_dir_all(dir).expect("a directory for the picture");
    let file = std::fs::File::create(dir.join("pic.png")).expect("the picture's file");
    let mut encoder = png::Encoder::new(file, 60, 40);
    encoder.set_color(png::ColorType::GrayscaleAlpha);
    encoder.set_depth(png::BitDepth::Sixteen);
    let pixels: Vec<u8> = (0..60 * 40u32)
        .flat_map(|i| [(i % 251) as u8, 0, (i % 199) as u8, 0])
        .collect();
    let mut writer = encoder.write_header().expect("the picture's header");
    writer
        .write_image_data(&pixels)
        .expect("the picture's pixels");
}

#[test]
fn a_picture_is_wanted_then_shown_at_its_own_size() {
    let Some(mut fonts) = fonts() else { return };
    let dir = std::env::temp_dir().join(format!("die-markdown-picture-{}", std::process::id()));
    sample_picture(&dir);
    let palette = Palette::mono_dark();
    let env = env(1200, 2.0, &palette, Some(&dir));
    let text = "![here](pic.png)\n\n![gone](none.png) ![far](https://x.y/z.png)\n";
    let mut pictures = Pictures::new();
    // A local picture with no answer yet is wanted; one that's somewhere else never is.
    let before = Page::new(text, &env, &mut fonts, &pictures);
    assert_eq!(before.wants(), [dir.join("pic.png"), dir.join("none.png")]);
    let picture = Picture::load(&dir.join("pic.png")).expect("sixteen-bit grey with alpha reads");
    assert_eq!((picture.width, picture.height), (60, 40));
    let grey = |p: &[u8]| p[0] == p[1] && p[1] == p[2] && p[0] <= p[3];
    assert!(picture.pixels.len() == 60 * 40 * 4 && picture.pixels.chunks_exact(4).all(grey));
    pictures.insert(dir.join("pic.png"), picture);
    assert!(Picture::load(&dir.join("none.png")).is_err());
    pictures.fail(dir.join("none.png"));
    let after = Page::new(text, &env, &mut fonts, &pictures);
    assert!(after.wants().is_empty());
    // Each is a link to itself. The one that's here is its own size in CSS pixels: twice
    // that on this display.
    let links: Vec<(&str, bool)> = after
        .links()
        .iter()
        .map(|l| (l.dest.as_str(), l.image))
        .collect();
    let dests = [
        ("pic.png", true),
        ("none.png", true),
        ("https://x.y/z.png", true),
    ];
    assert_eq!(links, dests);
    let at = after.links()[0].rects[0];
    assert_eq!((at.w, at.h), (120.0, 80.0));
    // And it's drawn: where it is, the buffer isn't all canvas.
    let mut pixels = vec![0u8; 1200 * 80 * 4];
    after.paint(
        &mut fonts,
        &pictures,
        &mut pixels,
        1200,
        80,
        at.y as i32,
        0..80,
        &Marks::default(),
    );
    let canvas = [palette.canvas.0, palette.canvas.1, palette.canvas.2, 255];
    let drawn = pixels.chunks_exact(4).filter(|px| **px != canvas).count();
    assert!(drawn > 1000, "{drawn} pixels of the picture");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Lays `text` out, answers its pictures, and holds the page to what any page must be.
fn check(text: &str, name: &str, width: u32, scale: f32, dir: &Path, fonts: &mut Fonts) {
    let palette = Palette::mono_dark();
    let env = env(width, scale, &palette, Some(dir));
    let mut pictures = Pictures::new();
    let wants = Page::new(text, &env, fonts, &pictures).wants().to_vec();
    for path in wants {
        match Picture::load(&path) {
            Ok(picture) => pictures.insert(path, picture),
            Err(_) => pictures.fail(path),
        }
    }
    let page = Page::new(text, &env, fonts, &pictures);
    let what = format!("{name} at {width} wide, scale {scale}");
    assert!(
        page.wants().is_empty(),
        "{what}: every picture was answered"
    );
    let height = page.height();
    assert!(height.is_finite() && height > 0.0, "{what}: {height} tall");
    // Links are inside the page, headings go down it.
    for link in page.links() {
        assert!(!link.rects.is_empty(), "{what}: a link with no place");
        for r in &link.rects {
            let inside =
                r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= width as f32 && r.y + r.h <= height;
            assert!(
                inside && r.w >= 0.0 && r.h >= 0.0,
                "{what}: {r:?} is outside the page"
            );
        }
    }
    let ys: Vec<f32> = page.headings().iter().map(|h| h.y).collect();
    assert!(ys.is_sorted(), "{what}: headings out of order");
    assert!(
        ys.iter().all(|y| *y >= 0.0 && *y <= height),
        "{what}: a heading off the page"
    );
    // The source map goes one way, and each side undoes the other to within a line.
    let step = (text.len() / 400).max(1);
    let down: Vec<f32> = (0..=text.len())
        .step_by(step)
        .map(|at| page.y_of(at))
        .collect();
    assert!(down.is_sorted(), "{what}: y_of goes back up");
    for y in &down {
        assert_eq!(page.y_of(page.source_at(*y)), *y, "{what}: the line at {y}");
    }
    let rows = (0..=40).map(|i| height * i as f32 / 40.0);
    let sources: Vec<usize> = rows.map(|y| page.source_at(y)).collect();
    assert!(sources.is_sorted(), "{what}: source_at goes back");
    assert!(
        sources.iter().all(|at| *at <= text.len()),
        "{what}: a byte past the text"
    );
    // A buffer's asked rows are written whole and opaque, wherever on the page it is, and the
    // rest is left alone.
    let (w, h) = (width.min(1200), 96u32);
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    let last = height as i32;
    for top in [-40, 0, last / 3, last * 2 / 3, last - 50, last + 500] {
        pixels.fill(7);
        let marks = Marks {
            focus: page.links().len().checked_sub(1),
        };
        page.paint(fonts, &pictures, &mut pixels, w, h, top, 8..h - 8, &marks);
        for (row, bytes) in pixels.chunks_exact((w as usize * 4).max(1)).enumerate() {
            let asked = (8..h as usize - 8).contains(&row);
            let whole = match asked {
                true => bytes.chunks_exact(4).all(|px| px[3] == 255),
                false => bytes.iter().all(|b| *b == 7),
            };
            assert!(whole, "{what}: row {row} of the buffer at {top}");
        }
    }
}

#[test]
fn every_document_lays_out_and_paints_at_any_width() {
    let Some(mut fonts) = fonts() else { return };
    let dir = std::env::temp_dir().join(format!("die-markdown-typeset-{}", std::process::id()));
    sample_picture(&dir);
    for file in documents() {
        let text = std::fs::read_to_string(&file).expect("a document reads");
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        // The largest document, half a megabyte, at two widths: the rest at every one.
        let large = text.len() > 200_000;
        for (width, scale) in [
            (0, 1.0),
            (1, 2.0),
            (300, 1.0),
            (800, 2.0),
            (1800, 1.0),
            (3024, 2.0),
        ] {
            if !large || width == 300 || width == 3024 {
                check(&text, &name, width, scale, &dir, &mut fonts);
            }
        }
        // The page's anchors are the plan's: a link to a heading finds it in either view.
        let options = Options {
            width: 80,
            tab_width: 8,
            front_matter: false,
        };
        let plan = plan(&text, &options);
        let palette = Palette::mono_dark();
        let page = Page::new(
            &text,
            &env(1800, 2.0, &palette, None),
            &mut fonts,
            &Pictures::new(),
        );
        let of_plan: Vec<&str> = plan.headings.iter().map(|h| h.slug.as_str()).collect();
        let of_page: Vec<&str> = page.headings().iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(of_page, of_plan, "{name}: the anchors");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn odd_scales_and_cut_off_text_make_a_page_all_the_same() {
    let Some(mut fonts) = fonts() else { return };
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = here.join("tests/pages");
    let kitchen = std::fs::read_to_string(dir.join("kitchen.md")).expect("the sample page");
    for scale in [0.0, f32::NAN, f32::INFINITY, -3.0, 1.5, 100.0] {
        check(&kitchen, "kitchen.md", 700, scale, &dir, &mut fonts);
    }
    check("", "nothing", 500, 2.0, &dir, &mut fonts);
    // Every 97th byte, the text ends there: in the middle of a table, a fence, a tag.
    for end in (0..kitchen.len()).step_by(97) {
        let end = (end..=kitchen.len())
            .find(|at| kitchen.is_char_boundary(*at))
            .unwrap_or(0);
        check(
            &kitchen[..end],
            "a cut kitchen.md",
            640,
            1.0,
            &dir,
            &mut fonts,
        );
    }
}
