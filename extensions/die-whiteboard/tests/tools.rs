//! The tools, worked with the pointer and the keys on the harness: the pen with its smoothing
//! and simplification, each shape tool with Shift and its return to the pen, the eraser, the
//! selection's move, nudge, delete and duplicate, labels with Esc, undo and redo per gesture,
//! the press that focuses, the wheel, and the status line.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Button, Mods, PointerKind, State};
use die_whiteboard::model::{Point, Shape, WIDTHS};
use die_whiteboard::surface::Tool;
use std::time::Duration;

fn stroke_points(el: &die_whiteboard::model::Element) -> Vec<Point> {
    match &el.shape {
        Shape::Stroke { points, .. } => points.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_pen_draws_a_smoothed_simplified_stroke() {
    let mut s = setup("pen");
    assert!(status(&mut s).contains("pen.svg  pen·1  medium  0 elements"));
    let samples: Vec<(f32, f32)> = (0..10).map(|i| (10.0 + i as f32 * 8.0, 40.0)).collect();
    gesture(&mut s, &samples);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 1);
    let points = stroke_points(&els[0]);
    assert_eq!(
        points.first(),
        Some(&Point::new(10.0, 40.0)),
        "the first sample as it is"
    );
    assert_eq!(
        points.len(),
        2,
        "a straight run simplifies to its ends: {points:?}"
    );
    assert_eq!(
        points[1],
        Point::new(82.0, 40.0),
        "and ends where the pointer lifted"
    );
    // The low-pass shows between: a corner comes out rounded short of the sample.
    gesture(&mut s, &[(10.0, 200.0), (40.0, 200.0), (40.0, 230.0)]);
    let bent = stroke_points(&elements(&mut s.h, s.buf)[1]);
    assert_eq!(
        bent,
        [
            Point::new(10.0, 200.0),
            Point::new(25.0, 200.0),
            Point::new(40.0, 230.0)
        ],
        "half way to each sample, the end where the pointer lifted, the rest simplified away"
    );
    keys(&mut s.h, "u");
    assert_eq!((els[0].ink, els[0].width), (1, 1));
    assert!(status(&mut s).contains("1 element *"), "{}", status(&mut s));
    // A curve keeps its corner.
    gesture(
        &mut s,
        &[
            (10.0, 60.0),
            (40.0, 60.0),
            (70.0, 60.0),
            (70.0, 90.0),
            (70.0, 120.0),
        ],
    );
    let points = stroke_points(&elements(&mut s.h, s.buf)[1]);
    assert!(points.len() >= 3, "{points:?}");
    // A click alone is a dot.
    click(&mut s, (100.0, 100.0));
    let dot = stroke_points(&elements(&mut s.h, s.buf)[2]);
    assert_eq!(dot, [Point::new(100.0, 100.0)]);
    assert!(inked(&mut s, (100.0, 100.0)), "the dot is drawn");
    assert!(!inked(&mut s, (100.0, 130.0)));
}

#[test]
fn inks_widths_and_the_highlighter() {
    let mut s = setup("inks");
    keys(&mut s.h, "3]");
    assert!(
        status(&mut s).contains("pen·3  thick"),
        "{}",
        status(&mut s)
    );
    gesture(&mut s, &[(10.0, 10.0), (50.0, 10.0)]);
    let el = &elements(&mut s.h, s.buf)[0];
    assert_eq!((el.ink, el.width, el.stroke_width()), (3, 2, WIDTHS[2]));
    keys(&mut s.h, "[[[");
    assert!(status(&mut s).contains("thin"));
    keys(&mut s.h, "h");
    assert!(
        status(&mut s).contains("highlighter·3"),
        "{}",
        status(&mut s)
    );
    gesture(&mut s, &[(10.0, 30.0), (50.0, 30.0)]);
    let el = &elements(&mut s.h, s.buf)[1];
    assert!(matches!(
        el.shape,
        Shape::Stroke {
            highlighter: true,
            ..
        }
    ));
    assert_eq!(el.stroke_width(), 16.0);
    keys(&mut s.h, "h");
    assert!(status(&mut s).contains("pen·3"), "h again is the pen");
    let errors = keys_errors(&mut s.h, "7");
    assert_eq!(errors.len(), 1, "7 isn't an ink: {errors:?}");
    keys(&mut s.h, "1");
    assert!(status(&mut s).contains("pen·1"));
}

#[test]
fn each_shape_is_one_shot_and_shift_constrains() {
    let mut s = setup("shapes");
    keys(&mut s.h, "r");
    assert!(status(&mut s).contains("rect·1"));
    gesture(&mut s, &[(10.0, 10.0), (30.0, 20.0), (60.0, 40.0)]);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[0].shape,
        Shape::Rect {
            a: Point::new(10.0, 10.0),
            b: Point::new(60.0, 40.0)
        }
    );
    assert!(
        status(&mut s).contains("pen·1"),
        "back to the pen: {}",
        status(&mut s)
    );
    keys(&mut s.h, "c");
    gesture_with(&mut s, &[(100.0, 10.0), (140.0, 20.0)], SHIFT, Button::Left);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[1].shape,
        Shape::Ellipse {
            a: Point::new(100.0, 10.0),
            b: Point::new(140.0, 50.0)
        },
        "a circle"
    );
    keys(&mut s.h, "r");
    gesture_with(
        &mut s,
        &[(100.0, 100.0), (80.0, 110.0)],
        SHIFT,
        Button::Left,
    );
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[2].shape,
        Shape::Rect {
            a: Point::new(100.0, 100.0),
            b: Point::new(80.0, 120.0)
        },
        "a square, the way it was dragged"
    );
    keys(&mut s.h, "l");
    gesture_with(&mut s, &[(10.0, 200.0), (90.0, 215.0)], SHIFT, Button::Left);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[3].shape,
        Shape::Line {
            a: Point::new(10.0, 200.0),
            b: Point::new(90.0, 200.0),
            arrow: false,
            from: None,
            to: None
        },
        "held to the axis"
    );
    keys(&mut s.h, "a");
    gesture(&mut s, &[(10.0, 250.0), (90.0, 290.0)]);
    let els = elements(&mut s.h, s.buf);
    assert!(matches!(els[4].shape, Shape::Line { arrow: true, .. }));
    // A drag that goes nowhere draws nothing, and still returns the pen.
    keys(&mut s.h, "r");
    click(&mut s, (300.0, 200.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 5);
    assert!(status(&mut s).contains("pen·1"));
    // Drawn: the rectangle's corner and edge, the circle's top, the arrow's head, and nothing
    // where nothing is.
    for at in [(10.0, 10.0), (35.0, 10.0), (120.0, 10.0), (80.0, 285.0)] {
        assert!(inked(&mut s, at), "{at:?}");
    }
    assert!(!inked(&mut s, (35.0, 25.0)), "inside the rectangle");
    assert!(!inked(&mut s, (300.0, 200.0)));
}

#[test]
fn an_arrow_binds_to_the_shapes_at_its_ends_and_follows_them() {
    let mut s = setup("bind");
    keys(&mut s.h, "r");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 60.0)]);
    keys(&mut s.h, "c");
    gesture(&mut s, &[(200.0, 10.0), (260.0, 60.0)]);
    keys(&mut s.h, "a");
    gesture(&mut s, &[(66.0, 35.0), (194.0, 35.0)]);
    let els = elements(&mut s.h, s.buf);
    let Shape::Line { a, b, from, to, .. } = els[2].shape else {
        panic!()
    };
    assert_eq!((from, to), (Some(els[0].id), Some(els[1].id)));
    assert_eq!(a, Point::new(60.0, 35.0), "on the rectangle's edge");
    assert!(
        (b.x - 200.0).abs() < 1e-3 && (b.y - 35.0).abs() < 1e-3,
        "on the ellipse's outline: {b:?}"
    );
    // Select the rectangle and nudge it: the arrow's start follows.
    keys(&mut s.h, "v");
    click(&mut s, (30.0, 30.0));
    assert_eq!(board(&mut s.h, s.buf).borrow().selection, [els[0].id]);
    keys(&mut s.h, "j");
    let els = elements(&mut s.h, s.buf);
    let Shape::Line { a, b, .. } = els[2].shape else {
        panic!()
    };
    assert_eq!(a, Point::new(60.0, 45.0));
    assert!(
        (b.x - 200.0).abs() < 1e-3 && (b.y - 35.0).abs() < 1e-3,
        "{b:?}"
    );
    // Deleting the ellipse unbinds the end; undo binds it again.
    keys(&mut s.h, "<Esc>v");
    click(&mut s, (230.0, 30.0));
    keys(&mut s.h, "x");
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 2);
    assert!(matches!(els[1].shape, Shape::Line { to: None, .. }));
    keys(&mut s.h, "u");
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 3);
    assert!(
        matches!(els[2].shape, Shape::Line { to: Some(_), .. }),
        "{:?}",
        els[2]
    );
}

#[test]
fn the_eraser_takes_what_a_drag_crosses_and_the_right_button_erases_nothing() {
    let mut s = setup("eraser");
    for y in [20.0, 60.0, 100.0] {
        gesture(&mut s, &[(10.0, y), (100.0, y)]);
    }
    assert_eq!(elements(&mut s.h, s.buf).len(), 3);
    keys(&mut s.h, "e");
    assert!(status(&mut s).contains("eraser·1"));
    gesture(&mut s, &[(50.0, 10.0), (50.0, 40.0), (50.0, 70.0)]);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "two crossed");
    assert!(status(&mut s).contains("eraser·1"), "the eraser stays on");
    keys(&mut s.h, "e");
    assert!(status(&mut s).contains("pen·1"));
    // The right button held is the tool dial (tests/dial.rs), not the eraser: a right drag
    // across the stroke leaves it, and picks the tool in its direction.
    gesture_with(
        &mut s,
        &[(50.0, 90.0), (50.0, 110.0)],
        Mods::default(),
        Button::Right,
    );
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "nothing erased");
    assert!(status(&mut s).contains("line·1"), "{}", status(&mut s));
    keys(&mut s.h, "<Esc>");
    keys(&mut s.h, "u");
    assert_eq!(elements(&mut s.h, s.buf).len(), 3, "one gesture, one entry");
    keys(&mut s.h, "<C-r>");
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
}

#[test]
fn select_moves_nudges_deletes_and_duplicates() {
    let mut s = setup("select");
    keys(&mut s.h, "r");
    gesture(&mut s, &[(10.0, 10.0), (50.0, 50.0)]);
    keys(&mut s.h, "r");
    gesture(&mut s, &[(100.0, 10.0), (150.0, 50.0)]);
    gesture(&mut s, &[(10.0, 100.0), (60.0, 100.0)]);
    let ids: Vec<_> = elements(&mut s.h, s.buf).iter().map(|e| e.id).collect();
    keys(&mut s.h, "v");
    // A click selects; a drag on it moves it.
    click(&mut s, (30.0, 30.0));
    assert_eq!(board(&mut s.h, s.buf).borrow().selection, [ids[0]]);
    gesture(&mut s, &[(30.0, 30.0), (40.0, 35.0), (50.0, 40.0)]);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[0].shape,
        Shape::Rect {
            a: Point::new(30.0, 20.0),
            b: Point::new(70.0, 60.0)
        }
    );
    // A marquee selects what it holds whole.
    gesture(&mut s, &[(0.0, 0.0), (80.0, 80.0), (170.0, 110.0)]);
    assert_eq!(board(&mut s.h, s.buf).borrow().selection, ids);
    gesture(&mut s, &[(0.0, 0.0), (80.0, 80.0)]);
    assert_eq!(board(&mut s.h, s.buf).borrow().selection, [ids[0]]);
    // Nudges are a cell: 5 by 10 board units here.
    keys(&mut s.h, "lj");
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[0].shape,
        Shape::Rect {
            a: Point::new(35.0, 30.0),
            b: Point::new(75.0, 70.0)
        }
    );
    keys(&mut s.h, "hk");
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[0].shape,
        Shape::Rect {
            a: Point::new(30.0, 20.0),
            b: Point::new(70.0, 60.0)
        }
    );
    // y duplicates a cell down and right and selects the copy.
    keys(&mut s.h, "y");
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 4);
    assert_eq!(
        els[3].shape,
        Shape::Rect {
            a: Point::new(35.0, 30.0),
            b: Point::new(75.0, 70.0)
        }
    );
    assert_eq!(board(&mut s.h, s.buf).borrow().selection, [els[3].id]);
    // x deletes it; Esc clears the selection and goes back to the pen.
    keys(&mut s.h, "x");
    assert_eq!(elements(&mut s.h, s.buf).len(), 3);
    keys(&mut s.h, "v");
    click(&mut s, (120.0, 30.0));
    assert_eq!(board(&mut s.h, s.buf).borrow().selection.len(), 1);
    keys(&mut s.h, "<Esc>");
    assert!(board(&mut s.h, s.buf).borrow().selection.is_empty());
    assert!(status(&mut s).contains("pen·1"));
    let errors = keys_errors(&mut s.h, "x");
    assert_eq!(errors, ["Nothing is selected"]);
    // The selection is outlined, 3 board units outside the element's bounds.
    assert!(!(0..8).any(|i| inked(&mut s, (95.0, 26.0 + i as f32))));
    keys(&mut s.h, "v");
    click(&mut s, (120.0, 30.0));
    assert!(
        (0..8).any(|i| inked(&mut s, (95.0, 26.0 + i as f32))),
        "the outline's left edge"
    );
    // Undo takes each gesture back in turn: the delete, the duplicate, four nudges, the move.
    keys(&mut s.h, "<Esc>");
    for n in [4, 3, 3, 3, 3, 3, 3] {
        keys(&mut s.h, "u");
        assert_eq!(elements(&mut s.h, s.buf).len(), n);
    }
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[0].shape,
        Shape::Rect {
            a: Point::new(10.0, 10.0),
            b: Point::new(50.0, 50.0)
        },
        "as drawn"
    );
}

#[test]
fn labels_are_typed_in_insert_state_until_esc() {
    let mut s = setup("labels");
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 20.0));
    assert_eq!(s.h.state(), State::Insert);
    assert!(status(&mut s).ends_with("<I>"));
    keys(&mut s.h, "hi there<S-CR>two");
    let cursors = s.h.scene().cursors.clone();
    let (r0, c0) = origin(&mut s.h, s.win);
    assert!(
        cursors.iter().any(|c| (c.row, c.col) == (r0 + 3, c0 + 7)),
        "the caret after `two`, a row down: {cursors:?}"
    );
    keys(&mut s.h, "<BS>o<Esc>");
    assert_eq!(s.h.state(), State::Normal);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(
        els[0].shape,
        Shape::Label {
            at: Point::new(20.0, 20.0),
            text: "hi there\ntwo".into()
        }
    );
    assert!(status(&mut s).contains("pen·1"), "{}", status(&mut s));
    let rows = area(&mut s);
    assert!(
        rows[2].contains("hi there") && rows[3].contains("two"),
        "{rows:#?}"
    );
    assert!(s.h.scene().cursors.is_empty(), "no caret in normal state");
    // A double click on it edits it; RET ends it too.
    keys(&mut s.h, "v");
    pointer(
        &mut s,
        PointerKind::Press(Button::Left),
        (25.0, 22.0),
        Mods::default(),
        2,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Left),
        (25.0, 22.0),
        Mods::default(),
        2,
    );
    assert_eq!(s.h.state(), State::Insert);
    keys(&mut s.h, "!<CR>");
    assert_eq!(s.h.state(), State::Normal);
    let els = elements(&mut s.h, s.buf);
    assert!(matches!(&els[0].shape, Shape::Label { text, .. } if text == "hi there\ntwo!"));
    // Undo is per label session.
    keys(&mut s.h, "u");
    let els = elements(&mut s.h, s.buf);
    assert!(matches!(&els[0].shape, Shape::Label { text, .. } if text == "hi there\ntwo"));
    keys(&mut s.h, "u");
    assert!(elements(&mut s.h, s.buf).is_empty());
    keys(&mut s.h, "<C-r>");
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    // An empty label goes.
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 100.0));
    keys(&mut s.h, "<Esc>");
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    assert_eq!(s.h.state(), State::Normal);
    // Cmd-Z undoes while a label is typed, as a kind binding.
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 150.0));
    keys(&mut s.h, "ab<D-z>");
    assert!(elements(&mut s.h, s.buf).len() <= 2);
}

#[test]
fn the_press_that_focuses_the_window_draws_nothing() {
    let mut s = setup("focus");
    keys(&mut s.h, "<C-w>s");
    assert_eq!(
        s.h.window_list().len(),
        2,
        "die-vim's C-w s works on a board"
    );
    let notes = s.h.find_buffer("notes").unwrap();
    s.h.show(notes, die_core::ShowIn::Current).unwrap();
    s.h.run_until_idle();
    assert_ne!(s.h.focused(), Some(s.win));
    assert_eq!(s.h.focused_buffer(), Some(notes));
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    assert_eq!(s.h.focused(), Some(s.win), "the press focused it");
    assert!(elements(&mut s.h, s.buf).is_empty(), "and drew nothing");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "focused, a press draws");
}

#[test]
fn the_wheel_pans_and_the_keys_zoom() {
    let mut s = setup("wheel");
    gesture(&mut s, &[(10.0, 5.0), (60.0, 5.0)]);
    assert!(pixel_px(&mut s, (60.0, 10.0)).is_some_and(|p| p[3] > 0));
    pointer(
        &mut s,
        PointerKind::Scroll { dx: 0.0, dy: 2.0 },
        (50.0, 50.0),
        Mods::default(),
        0,
    );
    assert_eq!(board(&mut s.h, s.buf).borrow().view.pan, (0.0, -40.0));
    assert_eq!(
        pixel_px(&mut s, (60.0, 10.0)),
        Some([0, 0, 0, 0]),
        "scrolled up two rows"
    );
    keys(&mut s.h, "+");
    assert_eq!(board(&mut s.h, s.buf).borrow().view.zoom, 1.25);
    // The keys zoom about the area's centre at any zoom: the board point under it stays.
    let g = s.h.surface_area(s.win).unwrap();
    let centre = (g.text_w as f32 * 5.0, g.rows as f32 * 10.0);
    let under = |s: &mut Setup| {
        let view = board(&mut s.h, s.buf).borrow().view;
        view.to_board(centre.0, centre.1, 2.0)
    };
    let before = under(&mut s);
    keys(&mut s.h, "+");
    let after = under(&mut s);
    assert!(before.dist(after) < 0.01, "{before:?} moved to {after:?}");
    keys(&mut s.h, "0");
    assert_eq!(board(&mut s.h, s.buf).borrow().view, Default::default());
    assert!(pixel_px(&mut s, (60.0, 10.0)).is_some_and(|p| p[3] > 0));
    // The same board in cells: the wheel pans, and zoom says it needs pixels.
    let mut s = setup_cells("wheel-cells");
    gesture(&mut s, &[(10.0, 5.0), (60.0, 5.0)]);
    let before = area(&mut s);
    assert!(!before[0].is_empty(), "{before:#?}");
    pointer(
        &mut s,
        PointerKind::Scroll { dx: 0.0, dy: 2.0 },
        (50.0, 50.0),
        Mods::default(),
        0,
    );
    let after = area(&mut s);
    assert!(after[0].is_empty(), "scrolled up two rows: {after:#?}");
    let errors = keys_errors(&mut s.h, "+");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("pixel renderer"), "{errors:?}");
    keys(&mut s.h, "0");
    assert_eq!(area(&mut s), before);
}

#[test]
fn tools_by_name_and_the_status_line() {
    let mut s = setup("status");
    for t in Tool::ALL {
        let args = die_core::Args {
            text: Some(t.name().into()),
            ..Default::default()
        };
        s.h.run("whiteboard-tool", &args).unwrap();
        assert!(status(&mut s).contains(&format!("{}·1", t.name())));
    }
    let args = die_core::Args {
        text: Some("crayon".into()),
        ..Default::default()
    };
    let e = s.h.run("whiteboard-tool", &args).unwrap_err();
    assert!(e.to_string().contains("pen, highlighter"), "{e}");
    let notes = s.h.find_buffer("notes").unwrap();
    s.h.show(notes, die_core::ShowIn::Current).unwrap();
    let e = s.h.run("whiteboard-undo", &Default::default()).unwrap_err();
    assert_eq!(e.to_string(), "Not in a whiteboard");
}

#[test]
fn a_click_that_ends_a_label_draws_nothing() {
    let mut s = setup("label-end");
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 20.0));
    keys(&mut s.h, "hello");
    // Clicking elsewhere ends the label, and that's all it does: no dot where it landed.
    gesture(&mut s, &[(200.0, 200.0), (210.0, 210.0)]);
    assert_eq!(s.h.state(), State::Normal);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 1, "{els:?}");
    assert!(matches!(&els[0].shape, Shape::Label { text, .. } if text == "hello"));
    // `t` and a double click at one point: the empty label goes, and no dot is left.
    keys(&mut s.h, "t");
    click(&mut s, (100.0, 100.0));
    assert_eq!(s.h.state(), State::Insert);
    pointer(
        &mut s,
        PointerKind::Press(Button::Left),
        (100.0, 100.0),
        Mods::default(),
        2,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Left),
        (100.0, 100.0),
        Mods::default(),
        2,
    );
    assert_eq!(s.h.state(), State::Normal);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "no dot");
    // The right button ends it too, erasing nothing.
    keys(&mut s.h, "t");
    click(&mut s, (100.0, 150.0));
    keys(&mut s.h, "x");
    gesture_with(
        &mut s,
        &[(25.0, 22.0), (25.0, 24.0)],
        Mods::default(),
        Button::Right,
    );
    assert_eq!(s.h.state(), State::Normal);
    assert_eq!(elements(&mut s.h, s.buf).len(), 2, "hello wasn't erased");
}

fn rect_a(s: &mut Setup) -> Point {
    let els = elements(&mut s.h, s.buf);
    match els[0].shape {
        Shape::Rect { a, .. } => a,
        ref other => panic!("{other:?}"),
    }
}

fn undoable(s: &mut Setup) -> usize {
    board(&mut s.h, s.buf).borrow().history.undoable()
}

#[test]
fn a_gesture_cut_short_keeps_what_it_did_and_esc_takes_it_back() {
    let mut s = setup("cut-short");
    keys(&mut s.h, "r");
    gesture(&mut s, &[(10.0, 10.0), (50.0, 50.0)]);
    s.h.advance(Duration::from_secs(2));
    assert!(!board(&mut s.h, s.buf).borrow().dirty);
    // Esc mid-move: the rectangle goes back where it started, nothing to undo, nothing unsaved.
    keys(&mut s.h, "v");
    click(&mut s, (30.0, 30.0));
    press(&mut s, (30.0, 30.0));
    drag(&mut s, (60.0, 30.0));
    drag(&mut s, (90.0, 30.0));
    assert_eq!(rect_a(&mut s), Point::new(70.0, 10.0), "moved so far");
    keys(&mut s.h, "<Esc>");
    release(&mut s, (90.0, 30.0));
    assert_eq!(rect_a(&mut s), Point::new(10.0, 10.0), "back");
    assert_eq!(undoable(&mut s), 1);
    assert!(!board(&mut s.h, s.buf).borrow().dirty);
    assert_eq!(s.h.next_timer(), None);
    // The focus going mid-move (Cmd-Tab): the move so far stays, logged, and `u` takes it back.
    keys(&mut s.h, "v");
    click(&mut s, (30.0, 30.0));
    press(&mut s, (30.0, 30.0));
    drag(&mut s, (60.0, 30.0));
    assert!(s.h.send(die_core::Input::Focus(false)).is_empty());
    assert!(s.h.send(die_core::Input::Focus(true)).is_empty());
    release(&mut s, (90.0, 30.0));
    assert_eq!(
        rect_a(&mut s),
        Point::new(40.0, 10.0),
        "where it was when the focus went"
    );
    assert_eq!(undoable(&mut s), 2);
    // Past the grace in which a press after the focus is the one that activated the app.
    s.h.advance(Duration::from_millis(200));
    assert!(board(&mut s.h, s.buf).borrow().dirty);
    assert!(s.h.next_timer().is_some(), "autosave armed");
    keys(&mut s.h, "u");
    assert_eq!(rect_a(&mut s), Point::new(10.0, 10.0));
    // A tool key mid-erase: the erase so far is logged, so `u` brings the stroke back rather than
    // undo something else.
    keys(&mut s.h, "<Esc>");
    gesture(&mut s, &[(10.0, 100.0), (100.0, 100.0)]);
    gesture(&mut s, &[(10.0, 140.0), (100.0, 140.0)]);
    assert_eq!(elements(&mut s.h, s.buf).len(), 3);
    keys(&mut s.h, "e");
    press(&mut s, (50.0, 90.0));
    drag(&mut s, (50.0, 110.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 2, "one crossed");
    keys(&mut s.h, "p");
    release(&mut s, (50.0, 110.0));
    assert_eq!(
        elements(&mut s.h, s.buf).len(),
        2,
        "the release does nothing more"
    );
    keys(&mut s.h, "u");
    assert_eq!(
        elements(&mut s.h, s.buf).len(),
        3,
        "the erased stroke is back"
    );
    // Esc mid-erase puts what it took back, with nothing logged.
    keys(&mut s.h, "e");
    press(&mut s, (50.0, 130.0));
    drag(&mut s, (50.0, 150.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 2);
    let logged = undoable(&mut s);
    keys(&mut s.h, "<Esc>");
    release(&mut s, (50.0, 150.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 3, "put back");
    assert_eq!(undoable(&mut s), logged);
    // `u` mid-stroke commits the stroke so far, then takes it back.
    press(&mut s, (10.0, 200.0));
    drag(&mut s, (60.0, 200.0));
    keys(&mut s.h, "u");
    release(&mut s, (90.0, 200.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 3);
    keys(&mut s.h, "<C-r>");
    assert_eq!(
        elements(&mut s.h, s.buf).len(),
        4,
        "the stroke to where it stood"
    );
}

#[test]
fn the_press_that_activates_the_app_draws_nothing() {
    let mut s = setup("activate");
    // Cmd-Tab away and a click back: macOS activates the app, so the focus comes first, then the
    // press, in one burst. Neither draws.
    assert!(s.h.send(die_core::Input::Focus(false)).is_empty());
    assert!(s.h.send(die_core::Input::Focus(true)).is_empty());
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    assert!(elements(&mut s.h, s.buf).is_empty(), "the activating press");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "the next one draws");
    // Cmd-Tab back and a press later than any burst: a hand's press, which draws.
    assert!(s.h.send(die_core::Input::Focus(false)).is_empty());
    assert!(s.h.send(die_core::Input::Focus(true)).is_empty());
    s.h.advance(Duration::from_secs(1));
    gesture(&mut s, &[(10.0, 50.0), (60.0, 50.0)]);
    assert_eq!(elements(&mut s.h, s.buf).len(), 2);
}

#[test]
fn a_held_key_repeats_into_a_label() {
    let mut s = setup("repeat");
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 20.0));
    keys(&mut s.h, "a");
    for _ in 0..2 {
        let repeat = die_core::KeyEvent {
            kind: die_core::KeyKind::Repeat,
            ..die_core::KeyEvent::press(die_core::Key::Char('a'))
        };
        let errors = s.h.send(die_core::Input::Key(repeat));
        assert!(errors.is_empty(), "{errors:?}");
    }
    keys(&mut s.h, "<Esc>");
    let els = elements(&mut s.h, s.buf);
    assert!(
        matches!(&els[0].shape, Shape::Label { text, .. } if text == "aaa"),
        "{els:?}"
    );
}
