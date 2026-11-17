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
        opened: &[],
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
                inside && r.w > 0.0 && r.h > 0.0,
                "{what}: {r:?} is outside the page, or nothing"
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

#[test]
fn hairlines_are_a_css_pixel_at_every_scale() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::mono_dark();
    // A table at scale 2: 36 CSS pixels a row and a two pixel line, so 74 a row and two more.
    for rows in [1usize, 3] {
        let text = format!("| a | b |\n|---|---|\n{}", "| 1 | 2 |\n".repeat(rows));
        let page = Page::new(
            &text,
            &env(2400, 2.0, &palette, None),
            &mut fonts,
            &Pictures::new(),
        );
        assert_eq!(
            page.height() - 180.0,
            74.0 * (rows + 1) as f32 + 2.0,
            "{rows} rows at scale 2"
        );
        // At 1.5 a CSS pixel is snapped down to one: 9 + 36 + 9 a row and its line.
        let page = Page::new(
            &text,
            &env(2400, 1.5, &palette, None),
            &mut fonts,
            &Pictures::new(),
        );
        assert_eq!(
            page.height() - 136.0,
            55.0 * (rows + 1) as f32 + 1.0,
            "{rows} rows at scale 1.5"
        );
    }
    // A first heading's rule is two pixels at scale 2, and one at 1.5, a pixel and a half
    // snapped down as a browser snaps a border.
    for (scale, pad, rule) in [(2.0f32, 90.0f32, 2.0f32), (1.5, 68.0, 1.0)] {
        let text = "# T\n\nunder\n";
        let page = Page::new(
            text,
            &env(2400, scale, &palette, None),
            &mut fonts,
            &Pictures::new(),
        );
        let line = (40.0 * scale).round();
        near(
            top_of(&page, text, "under"),
            pad + line + 9.6 * scale + rule + 16.0 * scale,
            "under an h1",
        );
    }
}

#[test]
fn details_are_closed_until_opened_and_their_summary_is_a_toggle() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::mono_dark();
    let text = "<details>\n<summary>Sum</summary>\n\nHidden [in](u) here.\n\n</details>\n\n<details open>\n<summary>Open</summary>\n\nShown [out](v) here.\n\n</details>\n";
    let at = |page: &Page| -> Vec<String> { page.links().iter().map(|l| l.dest.clone()).collect() };
    let second = text.rfind("<details").expect("the second tag");
    let closed = Page::new(
        text,
        &env(1200, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    // Closed, the first shows its summary alone; the second, open by its tag, its content.
    assert_eq!(
        at(&closed),
        ["details:0", &format!("details:{second}"), "v"]
    );
    assert!(closed.links().iter().all(|l| !l.image));
    near(
        closed.height(),
        45.0 + 24.0 + 16.0 + 24.0 + 24.0 + 45.0,
        "one line, then two",
    );
    // The summary line is the toggle: a click anywhere along it finds the link.
    assert_eq!(closed.link_at(600.0, 50.0), Some(0));
    // Opened by the reader, the first shows its content; so turned, the second hides its own.
    let opened = [0usize, second];
    let mut env = env(1200, 1.0, &palette, None);
    env.opened = &opened;
    let turned = Page::new(text, &env, &mut fonts, &Pictures::new());
    assert_eq!(
        at(&turned),
        ["details:0", "u", &format!("details:{second}")]
    );
    near(
        turned.height(),
        closed.height(),
        "the same two lines the other way round",
    );
}

#[test]
fn spacers_nested_alerts_and_back_links_are_as_a_browser_has_them() {
    let Some(mut fonts) = fonts() else { return };
    // A lone <br> block is an empty line in a box with no margins, as is one after a </p> in
    // the same block; two are two lines; a paragraph of only a no-break space is a paragraph,
    // a line and its margin.
    let plain = page("a\n\nb\n", &mut fonts).height();
    let one = page("a\n\n<br>\n\nb\n", &mut fonts).height();
    let two = page("a\n\n<br>\n<br>\n\nb\n", &mut fonts).height();
    let after = page("a\n\n<p>p</p>\n<br>\n\nb\n", &mut fonts).height();
    let nbsp = page("a\n\n&nbsp;\n\nb\n", &mut fonts).height();
    near(one - plain, 24.0, "a line for one <br>");
    near(two - plain, 48.0, "two lines for two");
    near(
        after - plain,
        40.0 + 24.0,
        "a paragraph, then a line for the <br>",
    );
    near(nbsp - plain, 40.0, "a line and a gap for a no-break space");
    // Before a heading the heading's own margin is what's between.
    let heading = page("a\n\n# h\n", &mut fonts).height();
    let spaced = page("a\n\n<br>\n\n# h\n", &mut fonts).height();
    near(
        spaced - heading,
        24.0 + 16.0,
        "the line, and the heading's 24 for the paragraph's 16",
    );
    // An alert's marker in a list item is a quote's text: the same page as writing it plain.
    let nested = page("- > [!TIP]\n  > x\n", &mut fonts).height();
    let written = page("- > [!TIP] x\n", &mut fonts).height();
    let alert = page("> [!TIP]\n> x\n", &mut fonts).height();
    assert_eq!(nested, written, "a nested alert is a quote");
    assert!(alert > nested + 30.0, "a top-level one is an alert");
    // A note referred to twice has two ways back, each to its own reference.
    let text = "a[^n] b[^n]\n\n[^n]: c\n";
    let page = page(text, &mut fonts);
    let dests: Vec<&str> = page.links().iter().map(|l| l.dest.as_str()).collect();
    assert_eq!(dests, ["#fn-n", "#fn-n", "#fnref-n", "#fnref-n-2"]);
    assert_eq!(page.anchor("fnref-n"), page.anchor("fnref-n-2"));
    assert!(
        page.anchor("fnref-n-2")
            .is_some_and(|y| y < page.anchor("fn-n").unwrap_or(0.0))
    );
}

#[test]
fn ideographs_break_between_and_wide_tables_keep_their_words() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::mono_dark();
    // Sixty ideographs as one link, in a narrow column: several lines, all inside it.
    let han: String = "漢".repeat(60);
    let text = format!("[{han}](u)\n");
    let page = Page::new(
        &text,
        &env(300, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    let rects = &page.links()[0].rects;
    assert!(rects.len() > 2, "{} lines of ideographs", rects.len());
    assert!(rects.iter().all(|r| r.x + r.w <= 300.0 - 15.0 + 1.0));
    // A table whose longest word is wider than the column runs past it, into the margin, the
    // word whole; one that can't fit the area either way breaks the word.
    let word: String = "W".repeat(64);
    let text = format!("| a | b |\n|---|---|\n| 1 | [{word}](u) |\n");
    let page = Page::new(
        &text,
        &env(1200, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    let rects = &page.links()[0].rects;
    assert_eq!(rects.len(), 1, "the word is whole");
    assert!(
        rects[0].x + rects[0].w > 155.0 + 890.0,
        "past the column's edge"
    );
    assert!(
        rects[0].x + rects[0].w <= 1200.0 - 45.0,
        "inside the page's padding"
    );
    let page = Page::new(
        &text,
        &env(800, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    assert!(
        page.links()[0].rects.len() > 1,
        "broken where there's no room"
    );
}

/// The source bytes the lines of a page start at, top to bottom.
fn line_starts(page: &Page) -> Vec<usize> {
    let mut starts: Vec<usize> = Vec::new();
    let mut y = 0.0;
    while y < page.height() {
        let at = page.source_at(y);
        if starts.last() != Some(&at) {
            starts.push(at);
        }
        y += 1.0;
    }
    starts
}

#[test]
fn a_line_ends_after_a_hyphen_whatever_is_before_it_but_not_before_a_digit() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::mono_dark();
    // Pieces that each fit the narrowest column, joined by hyphens after digits: every line
    // after the first starts just after a hyphen, and no word is cut.
    let pieces: Vec<String> = (0..40).map(|i| format!("aarch{i}")).collect();
    let word = pieces.join("-");
    let text = format!("x86_64-unknown-linux-gnu utf8-encoded AT-SPI2-bridge {word}\n");
    let page = Page::new(
        &text,
        &env(0, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    let starts = line_starts(&page);
    assert!(starts.len() > 4, "{starts:?}");
    for at in &starts[1..] {
        let before = text[..*at].chars().next_back();
        assert!(
            matches!(before, Some('-' | ' ')),
            "a line starts at {:?}",
            &text[*at..]
        );
    }
    // A hyphen before a digit, or one that starts a word, isn't a place to end a line: such
    // a word too wide for the line is cut by letters instead.
    let text = format!("{} -{}\n", "1-2-3-4-5-6-7-8-9-0".repeat(3), "x".repeat(10));
    let page = Page::new(
        &text,
        &env(0, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    let starts = line_starts(&page);
    assert!(
        starts[1..]
            .iter()
            .any(|at| !text[..*at].ends_with(['-', ' '])),
        "cut, not broken at a hyphen: {starts:?}"
    );
    assert!(
        !starts.iter().any(|at| text[..*at].ends_with(" -")),
        "-x is whole"
    );
}

#[test]
fn a_zero_width_space_is_a_break_and_format_marks_take_no_room() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::mono_dark();
    let text = format!("{}\n", ["alphabet"; 12].join("\u{200b}"));
    let broken = Page::new(
        &text,
        &env(0, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    let starts = line_starts(&broken);
    assert!(starts.len() > 2, "{starts:?}");
    for at in &starts[1..] {
        assert!(text[..*at].ends_with('\u{200b}'), "{:?}", &text[*at..]);
    }
    // Isolates draw nothing, as the other marks of direction.
    let marked = page("x\u{2066}y\u{2069}z \u{2067}a\u{2069}\n", &mut fonts);
    let plain = page("xyz a\n", &mut fonts);
    assert_eq!(marked.height(), plain.height());
}

#[test]
fn a_word_cut_over_lines_maps_to_the_bytes_of_its_letters() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::mono_dark();
    let text = format!("Слово: {}\n", "жизнь".repeat(400));
    for width in [500, 600, 700, 703, 720] {
        let page = Page::new(
            &text,
            &env(width, 1.0, &palette, None),
            &mut fonts,
            &Pictures::new(),
        );
        let starts = line_starts(&page);
        assert!(starts.len() > 10, "{width}: {starts:?}");
        for at in &starts {
            assert!(text.is_char_boundary(*at), "{width}: {at} is in a letter");
        }
        // Each line starts as many letters on as the lines before it hold, so a line's start
        // is where its letters are: the starts go evenly down the word.
        let steps: Vec<usize> = starts[2..].windows(2).map(|w| w[1] - w[0]).collect();
        let (least, most) = (steps.iter().min(), steps.iter().max());
        assert!(
            most.zip(least).is_some_and(|(m, l)| m - l <= 4),
            "{width}: {steps:?}"
        );
    }
}

#[test]
fn a_table_in_a_centred_block_is_centred_and_a_shrunk_one_is_the_columns_width() {
    let Some(mut fonts) = fonts() else { return };
    let table = "| [A](a) | B |\n|---|---|\n| one | two |\n";
    let plain = page(table, &mut fonts);
    let centred = page(
        &format!("<div align=\"center\">\n\n{table}\n</div>\n"),
        &mut fonts,
    );
    let left = plain.links()[0].rects[0].x;
    let moved = centred.links()[0].rects[0].x;
    // The column is 890 wide from 155: the table, a hundred or so wide, is in its middle.
    assert!(left < 200.0, "left at {left}");
    assert!(moved > 155.0 + 890.0 / 2.0 - 120.0, "centred at {moved}");
    // A table whose text can't fit on single lines fills the column exactly, as the rule of
    // a first heading does: the grid's right edge meets the rule's.
    let long = "word ".repeat(60);
    let text = format!("# T\n\n| a | b | c |\n|---|---|---|\n| {long} | {long} | {long} |\n");
    let palette = Palette::mono_dark();
    let page = Page::new(
        &text,
        &env(1200, 1.0, &palette, None),
        &mut fonts,
        &Pictures::new(),
    );
    let rule = page.headings()[0].y + 50.0;
    let grid = top_of(&page, &text, "| a");
    let mut right = |y: f32| {
        let mut pixels = vec![0u8; 1200 * 4];
        page.paint(
            &mut fonts,
            &Pictures::new(),
            &mut pixels,
            1200,
            1,
            y as i32,
            0..1,
            &Marks::default(),
        );
        let canvas = [palette.canvas.0, palette.canvas.1, palette.canvas.2, 255];
        let px = pixels.chunks_exact(4).collect::<Vec<_>>();
        px.iter().rposition(|p| *p != canvas)
    };
    let (rule_end, grid_end) = (right(rule.round()), right(grid.round()));
    assert!(rule_end.is_some(), "the rule is drawn");
    assert_eq!(
        grid_end, rule_end,
        "the table's top edge ends where the rule does"
    );
}

#[test]
fn html_names_places_and_a_closed_details_says_what_it_hides() {
    let Some(mut fonts) = fonts() else { return };
    let text = "<a id=\"readme-top\"></a>\n<a name=\"old-top\"></a>\n\n# Project\n\n<h2 id=\"custom\">Custom</h2>\n\nText <span id=\"spot\">here</span>.\n\n<p align=\"right\">(<a href=\"#readme-top\">back to top</a>)</p>\n";
    let named = page(text, &mut fonts);
    let project = named.anchor("project").expect("the heading");
    for name in ["readme-top", "old-top"] {
        near(named.anchor(name).unwrap_or(-1.0), project, name);
    }
    near(
        named.anchor("custom").unwrap_or(-1.0),
        top_of(&named, text, "<h2"),
        "an HTML heading's id is its line",
    );
    near(
        named.anchor("spot").unwrap_or(-1.0),
        top_of(&named, text, "Text"),
        "an inline tag's id is its line",
    );
    assert_eq!(named.folded("readme-top"), None, "it's shown");
    // A heading in a closed details isn't shown, but it's there: at the summary line, and
    // the page says which block to open. Its slug still counts, so the next is usage-1.
    let text = "[to usage](#usage)\n\n<details>\n<summary>More</summary>\n\n## Usage\n\n<details>\n<summary>Inner</summary>\n\n<a id=\"deep\"></a>\n\n</details>\n\n</details>\n\n## Usage\n\nVisible.\n";
    let outer = text.find("<details>").expect("the outer tag");
    let inner = text.rfind("<details>").expect("the inner tag");
    let closed = page(text, &mut fonts);
    let summary = top_of(&closed, text, "<summary>More");
    near(
        closed.anchor("usage").unwrap_or(-1.0),
        summary,
        "a hidden heading",
    );
    assert_eq!(closed.folded("usage"), Some(&[outer][..]));
    assert_eq!(closed.folded("deep"), Some(&[outer, inner][..]));
    assert!(closed.anchor("usage-1").is_some_and(|y| y > summary + 20.0));
    assert_eq!(closed.folded("usage-1"), None);
    assert_eq!(closed.anchor("nowhere"), None);
    // Opened, the heading is shown where it is, and the inner block now hides the anchor.
    let palette = Palette::mono_dark();
    let opened = [outer];
    let mut env = env(1200, 1.0, &palette, None);
    env.opened = &opened;
    let open = Page::new(text, &env, &mut fonts, &Pictures::new());
    assert_eq!(open.folded("usage"), None);
    assert!(open.anchor("usage").is_some_and(|y| y > summary + 20.0));
    assert_eq!(open.folded("deep"), Some(&[inner][..]));
}

#[test]
fn a_picture_from_the_top_is_from_the_repositorys_top() {
    let Some(mut fonts) = fonts() else { return };
    let base = std::env::temp_dir().join(format!("die-markdown-root-{}", std::process::id()));
    let (repo, docs) = (base.join("repo"), base.join("repo/docs/deep"));
    std::fs::create_dir_all(repo.join(".git")).expect("a repository");
    std::fs::create_dir_all(&docs).expect("its documents");
    let palette = Palette::mono_dark();
    let text = "![a](/shot.png) ![b](here.png)\n";
    let wants = |dir: &Path, fonts: &mut Fonts| {
        let page = Page::new(
            text,
            &env(1200, 1.0, &palette, Some(dir)),
            fonts,
            &Pictures::new(),
        );
        page.wants().to_vec()
    };
    let top = std::fs::canonicalize(&repo).expect("the repository's path");
    assert_eq!(
        wants(&docs, &mut fonts),
        [top.join("shot.png"), docs.join("here.png")]
    );
    // With no repository round it, the file's own directory.
    let alone = base.join("alone");
    std::fs::create_dir_all(&alone).expect("a directory");
    assert_eq!(
        wants(&alone, &mut fonts),
        [alone.join("shot.png"), alone.join("here.png")]
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn deep_lists_keep_their_text_and_an_empty_heading_has_no_line() {
    let Some(mut fonts) = fonts() else { return };
    // Forty levels: every item is shown, each below the one before, and the text after the
    // list is under them all.
    let mut text = String::new();
    for level in 0..40 {
        text.push_str(&format!("{}- item {level}\n", "  ".repeat(level)));
    }
    text.push_str("\nAfter the list.\n");
    let deep = page(&text, &mut fonts);
    let mut last = -1.0;
    for level in 0..40 {
        let y = top_of(&deep, &text, &format!("item {level}\n"));
        assert!(y > last, "item {level} at {y}, after {last}");
        last = y;
    }
    assert!(top_of(&deep, &text, "After") > last);
    // An empty first heading is its padding and its rule, 24 under what's above.
    let text = "Above\n\n#\n\nBelow\n";
    let empty = page(text, &mut fonts);
    near(
        top_of(&empty, text, "Below") - top_of(&empty, text, "Above"),
        24.0 + 24.0 + 9.6 + 1.0 + 16.0,
        "under an empty h1",
    );
}

#[test]
fn a_rust_block_is_in_its_languages_colours_and_an_unnamed_one_is_plain() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::github_dark();
    let text = "```rust\nfn main() { // say hello\n    let n = 42;\n}\n```\n\n\
                ```\nfn plain() { // none\n    let n = 42;\n}\n```\n";
    let pictures = Pictures::new();
    let page = Page::new(text, &env(800, 2.0, &palette, None), &mut fonts, &pictures);
    let (w, h) = (800u32, page.height().ceil() as u32);
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    page.paint(
        &mut fonts,
        &pictures,
        &mut pixels,
        w,
        h,
        0,
        0..h,
        &Marks::default(),
    );
    // The second block's rows start where its first line is.
    let split = page.y_of(text.find("fn plain").unwrap()) as usize;
    let rows = pixels.chunks_exact(w as usize * 4).enumerate();
    let (mut first, mut second) = (Vec::new(), Vec::new());
    for (row, bytes) in rows {
        let into = if row < split { &mut first } else { &mut second };
        into.extend(bytes.chunks_exact(4).map(|p| (p[0], p[1], p[2])));
    }
    let count = |of: &[(u8, u8, u8)], c: die_markdown::typeset::Rgb| {
        of.iter().filter(|p| **p == (c.0, c.1, c.2)).count()
    };
    let syntax = palette.syntax;
    for (what, ink) in [
        ("keyword", syntax.keyword),
        ("comment", syntax.comment),
        ("number", syntax.number),
        ("function", syntax.function),
    ] {
        assert!(count(&first, ink) > 20, "the rust block's {what}s: {ink:?}");
        assert_eq!(
            count(&second, ink),
            0,
            "an unnamed block has no {what} colour"
        );
    }
    assert!(
        count(&second, palette.code_block) > 20,
        "it's in the code's own colour"
    );
}

#[test]
fn colours_never_change_where_a_code_line_wraps() {
    let Some(mut fonts) = fonts() else { return };
    let palette = Palette::github_dark();
    let pictures = Pictures::new();
    // Lines whose every word is another colour, and long ones, so a wrap lands where the
    // colour changes.
    let body = "fn f(x: u32) -> u32 { let y = \"s\"; /* c */ x.max(1_000) + y.len() as u32 + 0x1F }\n\
                pub const MAX: usize = 1; #[derive(Debug)] struct S { a: Vec<String>, b: Option<i64> }\n";
    for width in [300, 420, 560, 700, 900] {
        for scale in [1.0, 2.0] {
            let mut height = |lang: &str| {
                let text = format!("```{lang}\n{body}```\n");
                let env = env(width, scale, &palette, None);
                Page::new(&text, &env, &mut fonts, &pictures).height()
            };
            let (coloured, plain) = (height("rust"), height(""));
            assert_eq!(coloured, plain, "{width} wide at {scale}");
        }
    }
}
