//! The markdown view through your config: it comes in through `die::Config.extensions`, `\m` is
//! a key in a markdown file and on its page and nowhere else, a page takes mono-dark's colours
//! as your preview's stylesheet has them, and so does the view in place, where there are no
//! pixels to draw a page with. Nothing is written outside a temporary directory.

use die_test_cap as _;

use die::core::{Cell, CellPixels, Extension, Harness, Rgb, ShowIn, Underline};
use die_markdown::typeset::Palette;
use die_markdown::{Markdown, Options};
use std::path::PathBuf;

const NOTES: &str = "# Notes\n\nA [link](https://example.com), `code` and **bold**.\n\n---\n";

/// Your cells on your panel: 19 by 40 pixels at 2x.
const CELL: CellPixels = CellPixels {
    width: 19,
    height: 40,
    scale: 2.0,
};

/// Your config, with the markdown extension as it has it, or the one given in its place.
fn harness(markdown: Option<Markdown>) -> Harness {
    let config = die_dev::config(vec![]);
    let mut exts = die::built_in();
    for ext in config.extensions {
        match &markdown {
            Some(_) if ext.name() == "markdown" => {}
            _ => exts.push(ext),
        }
    }
    exts.extend(markdown.map(|m| Box::new(m) as Box<dyn Extension>));
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.open_text("notes", "# my notes\n").unwrap();
    h.set_window_size(120, 40);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-dev-markdown-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// Type keys that must all work.
fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

/// The cells of a row from where its text starts: your 80 columns are centred in 120.
fn row(h: &mut Harness, r: usize) -> Vec<Cell> {
    h.scene().row(r)[20..100].to_vec()
}

#[test]
fn leader_m_shows_a_markdown_file_as_a_page_and_the_file_again_and_is_no_key_elsewhere() {
    let dir = tree("leader");
    std::fs::write(dir.join("notes.md"), NOTES).unwrap();
    let mut h = harness(None);
    assert_eq!(
        h.command_doc("markdown-toggle").map(|d| d.1),
        Some("markdown")
    );

    // A buffer that isn't a markdown file: `\m` is undefined, as any key no one bound.
    let typed = h.type_keys("\\m").unwrap();
    assert_eq!(typed[1].errors, ["\\m is undefined"], "{typed:?}");
    assert_eq!(h.screen().lines().next().unwrap().trim(), "# my notes");

    // In a markdown file it shows the page: a buffer of its own in this window, drawn in
    // pixels over the window's whole width, on mono-dark's paper.
    let file = h.open_file(&dir.join("notes.md")).unwrap();
    h.show(file, ShowIn::Current).unwrap();
    keys(&mut h, "\\m");
    let page = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(page), Some("notes.md *page*"));
    let placed = h.scene().placements[0];
    assert_eq!((placed.col, placed.cols, placed.rows), (0, 120, 40));
    let (wide, tall, px) = h.pixels(placed.buffer).unwrap();
    assert_eq!((wide, tall), (120 * CELL.width, 40 * CELL.height));
    assert_eq!(px[..4], [0x1a, 0x1a, 0x1a, 255]);
    assert!(px.chunks_exact(4).any(|p| p != [0x1a, 0x1a, 0x1a, 255]));
    // Your `\z` narrows a page to your 80 columns, as it does any buffer.
    keys(&mut h, "\\z");
    let placed = h.scene().placements[0];
    assert_eq!((placed.col, placed.cols), (20, 80));

    // And `\m` on the page is the file again, raw.
    keys(&mut h, "\\m");
    assert_eq!(h.focused_buffer(), Some(file));
    assert_eq!(h.scene().row_text(0).trim(), "# Notes");
    assert!(h.scene().placements.is_empty());
    assert_eq!(h.find_buffer("notes.md *page*"), None);
}

#[test]
fn a_page_in_your_themes_colours_is_in_the_stylesheets() {
    let h = harness(None);
    let (read, sheet) = (die_markdown::theme_palette(&h), Palette::mono_dark());
    // Field for field, so a face that drifts from the stylesheet is the one named.
    assert_eq!(read.canvas, sheet.canvas, "canvas");
    assert_eq!(read.panel, sheet.panel, "panel");
    assert_eq!(read.pill, sheet.pill, "pill");
    assert_eq!(read.text, sheet.text, "text");
    assert_eq!(read.muted, sheet.muted, "muted");
    assert_eq!(read.subtle, sheet.subtle, "subtle");
    assert_eq!(read.border, sheet.border, "border");
    assert_eq!(read.hairline, sheet.hairline, "hairline");
    assert_eq!(read.headings, sheet.headings, "headings");
    assert_eq!(read.strong, sheet.strong, "strong");
    assert_eq!(read.em, sheet.em, "em");
    assert_eq!(read.del, sheet.del, "del");
    assert_eq!(read.link, sheet.link, "link");
    assert_eq!(read.link_underline, sheet.link_underline, "link_underline");
    assert_eq!(read.code, sheet.code, "code");
    assert_eq!(read.code_block, sheet.code_block, "code_block");
    assert_eq!(read.quote, sheet.quote, "quote");
    assert_eq!(read.table_header, sheet.table_header, "table_header");
    assert_eq!(read.marker, sheet.marker, "marker");
    assert_eq!(read.check, sheet.check, "check");
    assert_eq!(read.alerts, sheet.alerts, "alerts");
    // And whatever field comes next.
    assert_eq!(read, sheet);
}

#[test]
fn without_a_page_leader_m_renders_in_place_in_mono_darks_colours() {
    let dir = tree("in-place");
    std::fs::write(dir.join("notes.md"), NOTES).unwrap();
    let in_place = Options {
        page: false,
        ..Options::default()
    };
    let mut h = harness(Some(Markdown::new(in_place)));
    let buf = h.open_file(&dir.join("notes.md")).unwrap();
    h.show(buf, ShowIn::Current).unwrap();
    keys(&mut h, "\\m");
    assert_eq!(h.focused_buffer(), Some(buf));
    let text = |h: &mut Harness, r: usize| h.scene().row_text(r).trim().to_string();
    assert_eq!(text(&mut h, 0), "Notes");
    assert_eq!(text(&mut h, 2), "A link, code and bold.");
    assert_eq!(text(&mut h, 4), "─".repeat(80));

    let hex = Rgb::hex;
    let (paper, pill, border) = (hex("#1a1a1a"), hex("#2e2e2e"), hex("#3a3a3a"));
    // h1: bright and bold, over two lines in the border's colour.
    let title = &row(&mut h, 0)[0];
    assert_eq!((title.fg, title.bold), (hex("#eeeeee"), true));
    assert_eq!(
        (title.underline, title.underline_color),
        (Underline::Double, border)
    );
    let line = row(&mut h, 2);
    // a: red, bold and underlined.
    let link = &line[2];
    assert_eq!((link.fg, link.bold), (hex("#cc4444"), true));
    assert_eq!(link.underline, Underline::Single);
    // code: dimmer, on the highlight grey; the text beside it on the paper.
    let code = &line[8];
    assert_eq!((code.fg, code.bg), (hex("#888888"), pill));
    assert_eq!(line[7].bg, paper);
    // strong: bright and bold.
    let strong = &line[17];
    assert_eq!((strong.fg, strong.bold), (hex("#eeeeee"), true));
    // hr: the border's colour.
    assert_eq!(row(&mut h, 4)[0].fg, border);

    // And raw again.
    keys(&mut h, "\\m");
    assert_eq!(text(&mut h, 0), "# Notes");
    assert!(!row(&mut h, 0)[2].bold);
}
