//! The markdown view through your config: it comes in through `die::Config.extensions`, `\m` is
//! a key in a markdown file and nowhere else, and the rendered view draws in mono-dark as your
//! preview's stylesheet has it. Nothing is written outside a temporary directory.

use die_test_cap as _;

use die::core::{Cell, Harness, Rgb, ShowIn, Underline};
use std::path::PathBuf;

const NOTES: &str = "# Notes\n\nA [link](https://example.com), `code` and **bold**.\n\n---\n";

fn harness() -> Harness {
    let config = die_dev::config(vec![]);
    let mut exts = die::built_in();
    exts.extend(config.extensions);
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.open_text("notes", "# my notes\n").unwrap();
    h.set_window_size(120, 40);
    h
}

fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-dev-markdown-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// The cells of a row from where its text starts: your 80 columns are centred in 120.
fn row(h: &mut Harness, r: usize) -> Vec<Cell> {
    h.scene().row(r)[20..100].to_vec()
}

#[test]
fn leader_m_renders_a_markdown_file_in_mono_darks_colours_and_is_no_key_elsewhere() {
    let dir = tree("leader");
    std::fs::write(dir.join("notes.md"), NOTES).unwrap();
    let mut h = harness();
    assert_eq!(
        h.command_doc("markdown-toggle").map(|d| d.1),
        Some("markdown")
    );

    // A buffer that isn't a markdown file: `\m` is undefined, as any key no one bound.
    let typed = h.type_keys("\\m").unwrap();
    assert_eq!(typed[1].errors, ["\\m is undefined"], "{typed:?}");
    assert_eq!(h.screen().lines().next().unwrap().trim(), "# my notes");

    let buf = h.open_file(&dir.join("notes.md")).unwrap();
    h.show(buf, ShowIn::Current).unwrap();
    for k in h.type_keys("\\m").unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
    let text = |h: &mut Harness, r: usize| h.scene().row_text(r).trim().to_string();
    assert_eq!(text(&mut h, 0), "Notes");
    assert_eq!(text(&mut h, 2), "A link, code and bold.");
    assert_eq!(text(&mut h, 4), "─".repeat(80));

    let hex = Rgb::hex;
    let (paper, subtle, border) = (hex("#1a1a1a"), hex("#252525"), hex("#3a3a3a"));
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
    // code: dimmer, on the subtle background; the text beside it on the paper.
    let code = &line[8];
    assert_eq!((code.fg, code.bg), (hex("#888888"), subtle));
    assert_eq!(line[7].bg, paper);
    // strong: bright and bold.
    let strong = &line[17];
    assert_eq!((strong.fg, strong.bold), (hex("#eeeeee"), true));
    // hr: the border's colour.
    assert_eq!(row(&mut h, 4)[0].fg, border);

    // And raw again.
    h.type_keys("\\m").unwrap();
    assert_eq!(text(&mut h, 0), "# Notes");
    assert!(!row(&mut h, 0)[2].bold);
}
