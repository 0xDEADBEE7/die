//! The cell renderer's screen for a small board, as text: box glyphs for a rectangle and an
//! arrow, braille for an ellipse and a stroke, a label as text, the ink's colour per cell, and
//! the newest element winning a cell. Cells are asked for (`pixels: false`), as a host without
//! pixels gets them; the pixel renderer's tests are in `tests/pixels.rs`.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Color, Rgb, Theme};
use die_whiteboard::model::{Point, Shape};

#[test]
fn a_small_board_as_cells() {
    let mut s = setup_cells("render");
    assert!(
        s.h.scene().placements.is_empty(),
        "no pixel buffer in cells"
    );
    {
        let shared = board(&mut s.h, s.buf);
        let mut st = shared.borrow_mut();
        // Cells are 5 × 10 board units: a rectangle from cell (1, 1) to (8, 3).
        st.board.add(
            Shape::Rect {
                a: Point::new(5.0, 10.0),
                b: Point::new(40.0, 30.0),
            },
            1,
            1,
        );
        st.board.add(
            Shape::Line {
                a: Point::new(45.0, 20.0),
                b: Point::new(75.0, 20.0),
                arrow: true,
                from: None,
                to: None,
            },
            2,
            1,
        );
        st.board.add(
            Shape::Label {
                at: Point::new(10.0, 50.0),
                text: "box".into(),
            },
            3,
            1,
        );
        st.board.add(
            Shape::Ellipse {
                a: Point::new(80.0, 10.0),
                b: Point::new(120.0, 40.0),
            },
            4,
            1,
        );
        st.board.add(
            Shape::Stroke {
                points: vec![Point::new(10.0, 70.0), Point::new(60.0, 90.0)],
                highlighter: false,
            },
            5,
            1,
        );
        st.changed(
            std::time::Duration::ZERO,
            die_whiteboard::render::Damage::All,
        );
    }
    let rows = area(&mut s);
    assert!(rows[1].starts_with(" ┌──────┐ "), "{rows:#?}");
    let head = |r: &str, n: usize| r.chars().take(n).collect::<String>();
    assert_eq!(head(&rows[2], 16), " │      │╶─────▶");
    assert_eq!(head(&rows[3], 9), " └──────┘");
    assert!(rows[5].starts_with("  box"), "{:?}", rows[5]);
    let braille = |r: &str| {
        r.chars()
            .filter(|c| ('\u{2800}'..='\u{28ff}').contains(c))
            .count()
    };
    assert!(braille(&rows[1]) >= 4, "the ellipse's top: {:?}", rows[1]);
    assert!(
        braille(&rows[7]) >= 4 && braille(&rows[8]) >= 2,
        "the stroke: {rows:#?}"
    );
    // Each cell has its ink's colour: ink 2 is the theme's red, ink 1 the default.
    let (r0, c0) = origin(&mut s.h, s.win);
    let scene = s.h.scene();
    let theme = Theme::default();
    assert_eq!(scene.row(r0 + 2)[c0 + 15].fg, theme.ansi[1]);
    assert_eq!(scene.row(r0 + 1)[c0 + 1].fg, scene.row(r0 + 20)[c0 + 1].fg);
    assert_eq!(scene.row(r0 + 5)[c0 + 2].fg, theme.ansi[3], "yellow label");
    // A newer element takes the cell: a stroke through the rectangle's corner.
    {
        let shared = board(&mut s.h, s.buf);
        let mut st = shared.borrow_mut();
        st.board.add(
            Shape::Stroke {
                points: vec![Point::new(6.0, 11.0), Point::new(8.0, 13.0)],
                highlighter: false,
            },
            6,
            1,
        );
        st.changed(
            std::time::Duration::ZERO,
            die_whiteboard::render::Damage::All,
        );
    }
    let rows = area(&mut s);
    assert!(!rows[1].starts_with(" ┌"), "{:?}", rows[1]);
    assert!(braille(&rows[1]) >= 5);
    // The highlighter is faint: its cells mix the foreground toward the background.
    {
        let shared = board(&mut s.h, s.buf);
        let mut st = shared.borrow_mut();
        st.board.add(
            Shape::Stroke {
                points: vec![Point::new(10.0, 150.0), Point::new(60.0, 150.0)],
                highlighter: true,
            },
            1,
            1,
        );
        st.changed(
            std::time::Duration::ZERO,
            die_whiteboard::render::Damage::All,
        );
    }
    let scene = s.h.scene();
    let faint = &scene.row(r0 + 15)[c0 + 5];
    let plain = &scene.row(r0 + 1)[c0 + 8];
    assert_ne!(faint.fg, plain.fg, "{faint:?}");
    assert_eq!(Color::Rgb(Rgb(1, 2, 3)), Color::Rgb(Rgb(1, 2, 3)));
}
