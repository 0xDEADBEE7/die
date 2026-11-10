//! Export: `E` asks png or svg; the PNG beside the file decodes to the picture's size with ink
//! where the stroke is, in the theme's colours, at the scale and on the background the options
//! say; the SVG export carries no dialect.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, Config, Harness, Rgb, Style, Theme};
use die_vim::Vim;
use die_whiteboard::export::decode_png;
use die_whiteboard::{Background, Options, Whiteboard};

fn run(h: &mut Harness, name: &str, text: &str) {
    let args = Args {
        text: Some(text.into()),
        ..Args::default()
    };
    h.run(name, &args).unwrap();
    h.run_until_idle();
}

#[test]
fn png_export_decodes_to_the_picture_with_ink_where_the_stroke_is() {
    let mut s = setup("png");
    keys(&mut s.h, "2");
    gesture(&mut s, &[(0.0, 0.0), (100.0, 0.0)]);
    keys(&mut s.h, "E");
    assert_eq!(s.h.prompt_text(), Some(""));
    assert_eq!(s.h.prompt_candidates(), ["png", "svg"]);
    keys(&mut s.h, "png<CR>");
    s.h.run_until_idle();
    let out = s.dir.path().join("png.png");
    assert!(out.exists(), "{:?}", s.h.messages);
    let said = s.h.messages.last().unwrap();
    assert!(
        said.starts_with("Wrote ") && said.contains("png.png (136×36)"),
        "{said}"
    );
    let raster = decode_png(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(
        (raster.width, raster.height),
        (136, 36),
        "1x in tests, 16 of pad"
    );
    let on = raster.pixel(68, 18).unwrap();
    let theme = Theme::default();
    let red = theme.ansi[1];
    assert_eq!(
        &on[..3],
        &[red.0, red.1, red.2],
        "ink 2 is the theme's red: {on:?}"
    );
    let off = raster.pixel(68, 3).unwrap();
    assert_eq!(off, [0x10, 0x10, 0x10, 255], "the theme's background");
    assert!(!status(&mut s).contains("failed"));
}

#[test]
fn the_background_and_scale_options_and_a_label_through_the_font() {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Whiteboard::new(Options {
            png_scale: 2.0,
            export_background: Background::Transparent,
            ..Options::default()
        })),
    ];
    let theme = Theme::default().face("whiteboard-ink-1", Style::new().fg(Rgb(0, 0, 255)));
    let config = Config {
        theme: Some(theme),
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.open_text("notes", "").unwrap();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    let dir = TempDir::new("options");
    let path = dir.path().join("b.svg");
    open(&mut h, &path);
    let buf = h.focused_buffer().unwrap();
    {
        let shared = board(&mut h, buf);
        let mut st = shared.borrow_mut();
        st.board.add(
            die_whiteboard::model::Shape::Label {
                at: die_whiteboard::model::Point::new(0.0, 0.0),
                text: "Hi".into(),
            },
            1,
            1,
        );
        st.board.add(
            die_whiteboard::model::Shape::Rect {
                a: die_whiteboard::model::Point::new(0.0, 0.0),
                b: die_whiteboard::model::Point::new(50.0, 30.0),
            },
            1,
            1,
        );
        st.changed(
            std::time::Duration::ZERO,
            die_whiteboard::render::Damage::All,
        );
    }
    run(&mut h, "whiteboard-export", "png");
    let out = dir.path().join("b.png");
    let raster = decode_png(&std::fs::read(&out).unwrap()).unwrap();
    // 50 × 30 plus half the width (2) and 16 of pad either side, at 2x.
    assert_eq!((raster.width, raster.height), (172, 132));
    assert_eq!(raster.pixel(4, 4).unwrap()[3], 0, "transparent off the ink");
    let edge = raster.pixel(36, 60).unwrap();
    assert!(
        edge[2] > 200 && edge[3] > 200,
        "the rectangle's left edge, blue: {edge:?}"
    );
    // The label is rasterized through the font: ink inside the first cell, where `H` is.
    let inside: Vec<[u8; 4]> = (38..48)
        .flat_map(|x| (38..54).map(move |y| (x, y)))
        .filter_map(|(x, y)| raster.pixel(x, y))
        .filter(|p| p[3] > 100)
        .collect();
    assert!(!inside.is_empty(), "no glyph pixels; is DIE_FONT_DIR set?");
    // The SVG export is the picture alone, and the file keeps the dialect.
    run(&mut h, "whiteboard-export", "svg");
    let export = std::fs::read_to_string(dir.path().join("b.export.svg")).unwrap();
    assert!(!export.contains("data-wb"));
    assert!(export.contains("<rect class=\"i1\""));
    assert!(export.contains("#0000ff"), "the themed ink: {export}");
    run(&mut h, "whiteboard-save", "");
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("data-wb-version")
    );
    let args = Args {
        text: Some("gif".into()),
        ..Args::default()
    };
    let e = h.run("whiteboard-export", &args).unwrap_err();
    assert!(e.to_string().contains("png or svg"));
}
