//! The file: the dialect round trips a board exactly, a foreign SVG is refused, autosave runs a
//! second after the last change and no timer runs while every board is clean, a kill writes, and
//! `\w`'s prompt descends and takes a new name.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, Input, Key, KeyEvent, Mods, ShowIn};
use die_whiteboard::model::{Board, Point, Shape};
use die_whiteboard::svg::{self, Look, Refused};
use std::time::Duration;

/// A board with one of everything, by hand.
fn everything() -> Board {
    let mut b = Board::default();
    b.add(
        Shape::Stroke {
            points: vec![
                Point::new(1.5, 2.25),
                Point::new(10.125, 3.0),
                Point::new(0.1, 0.2),
            ],
            highlighter: false,
        },
        1,
        0,
    );
    b.add(
        Shape::Stroke {
            points: vec![Point::new(7.0, 7.0)],
            highlighter: true,
        },
        6,
        2,
    );
    let (_, rect) = b.add(
        Shape::Rect {
            a: Point::new(50.0, 40.0),
            b: Point::new(20.0, 90.0),
        },
        2,
        1,
    );
    let (_, ell) = b.add(
        Shape::Ellipse {
            a: Point::new(100.0, 100.0),
            b: Point::new(160.0, 130.0),
        },
        3,
        1,
    );
    b.add(
        Shape::Line {
            a: Point::new(50.0, 65.0),
            b: Point::new(100.0, 115.0),
            arrow: true,
            from: Some(rect),
            to: Some(ell),
        },
        4,
        2,
    );
    b.add(
        Shape::Line {
            a: Point::new(0.0, 200.0),
            b: Point::new(33.3, 200.7),
            arrow: false,
            from: None,
            to: None,
        },
        5,
        0,
    );
    b.add(
        Shape::Label {
            at: Point::new(12.0, 240.0),
            text: "a <b> & \"c\"\n\n  'd'  ".into(),
        },
        2,
        1,
    );
    b
}

#[test]
fn the_dialect_round_trips_exactly_and_an_export_carries_no_dialect() {
    let board = everything();
    let look = Look::default();
    let text = svg::write(&board, &look, true);
    assert!(text.starts_with("<?xml"));
    assert!(text.contains("data-wb-version=\"1\""));
    assert!(text.contains("<marker id=\"arrow-4\""));
    assert!(text.contains("marker-end=\"url(#arrow-4)\""));
    assert!(text.contains("stroke-opacity=\"0.35\""));
    assert!(text.contains("<tspan"));
    assert_eq!(svg::read(&text), Ok(board.clone()));
    let export = svg::write(&board, &look, false);
    assert!(!export.contains("data-wb"), "{export}");
    assert_eq!(svg::read(&export), Err(Refused::Foreign));
    // Through a buffer: saved by S, read by a second DIE.
    let dir = TempDir::new("roundtrip");
    let path = dir.path().join("all.svg");
    let mut h = harness();
    open(&mut h, &path);
    let buf = h.focused_buffer().unwrap();
    {
        let shared = common::board(&mut h, buf);
        let mut st = shared.borrow_mut();
        st.board = board.clone();
        st.changed(Duration::ZERO, die_whiteboard::render::Damage::All);
    }
    keys(&mut h, "S");
    assert!(
        h.messages
            .last()
            .unwrap()
            .contains("Wrote *whiteboard: all.svg*")
    );
    let mut again = harness();
    open(&mut again, &path);
    let buf2 = again.focused_buffer().unwrap();
    assert_eq!(elements(&mut again, buf2), board.elements);
    assert_eq!(
        common::board(&mut again, buf2).borrow().board.next_id,
        board.next_id
    );
}

#[test]
fn a_foreign_svg_is_refused_and_a_missing_file_is_a_new_board() {
    let dir = TempDir::new("foreign");
    let path = dir.path().join("logo.svg");
    std::fs::write(
        &path,
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle r=\"5\"/></svg>",
    )
    .unwrap();
    let mut h = harness();
    let args = Args {
        text: Some(path.display().to_string()),
        ..Args::default()
    };
    let e = h.run("whiteboard", &args).unwrap_err();
    assert!(
        e.to_string()
            .ends_with("not a whiteboard file; open it in a browser"),
        "{e}"
    );
    assert!(h.find_buffer("*whiteboard: logo.svg*").is_none());
    let fresh = dir.path().join("fresh");
    let args = Args {
        text: Some(fresh.display().to_string()),
        ..Args::default()
    };
    h.run("whiteboard", &args).unwrap();
    let buf = h.focused_buffer().unwrap();
    assert_eq!(
        h.buffer_name(buf),
        Some("*whiteboard: fresh.svg*"),
        "`.svg` added"
    );
    assert!(elements(&mut h, buf).is_empty());
    assert!(
        !fresh.with_extension("svg").exists(),
        "nothing written until something's drawn"
    );
    assert_eq!(h.directory(buf), Some(dir.path()));
}

#[test]
fn autosave_a_second_after_the_last_change_and_no_timer_when_clean() {
    let mut s = setup("autosave");
    assert_eq!(
        s.h.next_timer(),
        None,
        "nothing runs while the board is clean"
    );
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let t0 = s.h.now();
    assert_eq!(s.h.next_timer(), Some(t0 + Duration::from_secs(1)));
    assert!(!s.path.exists());
    s.h.advance(Duration::from_millis(600));
    gesture(&mut s, &[(10.0, 30.0), (60.0, 30.0)]);
    s.h.advance(Duration::from_millis(500));
    assert!(!s.path.exists(), "the second change moved the save on");
    assert!(status(&mut s).contains(" *"));
    s.h.advance(Duration::from_millis(600));
    assert!(s.path.exists(), "a second after the last change");
    assert!(!status(&mut s).contains(" *"));
    assert_eq!(s.h.next_timer(), None, "and no timer again");
    let text = std::fs::read_to_string(&s.path).unwrap();
    assert_eq!(svg::read(&text).unwrap().len(), 2);
    // A save that fails shows in the status line and the messages.
    {
        let shared = board(&mut s.h, s.buf);
        let mut st = shared.borrow_mut();
        st.path = s.dir.path().join("gone").join("x.svg");
    }
    gesture(&mut s, &[(10.0, 50.0), (60.0, 50.0)]);
    s.h.advance(Duration::from_secs(2));
    let error = board(&mut s.h, s.buf).borrow().error.clone();
    assert!(
        error.as_deref().is_some_and(|e| e.contains("No such file")),
        "{error:?}"
    );
    assert!(
        status(&mut s).contains("x.svg: No such file"),
        "{}",
        status(&mut s)
    );
    assert!(s.h.messages.iter().any(|m| m.contains("No such file")));
}

#[test]
fn a_kill_writes_what_is_behind() {
    let mut s = setup("kill");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    assert!(!s.path.exists());
    keys(&mut s.h, ":bd<CR>");
    assert!(!s.h.is_live(s.buf));
    assert_eq!(
        svg::read(&std::fs::read_to_string(&s.path).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s.h.next_timer(), None);
    s.h.advance(Duration::from_secs(2));
    assert!(s.h.take_errors().is_empty());
}

#[test]
fn the_prompt_descends_lists_boards_and_takes_a_new_name() {
    let dir = TempDir::new("prompt");
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("old.svg"), "").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "").unwrap();
    // A board's own export, which it would refuse as not a whiteboard file, isn't listed.
    std::fs::write(dir.path().join("old.export.svg"), "").unwrap();
    let mut h = harness();
    let notes = h.find_buffer("notes").unwrap();
    h.set_directory(notes, dir.path());
    h.run("whiteboard", &Args::default()).unwrap();
    let line = h.prompt_text().unwrap().to_string();
    assert!(line.ends_with('/'), "{line}");
    assert_eq!(
        h.prompt_candidates(),
        ["old.svg", "sub/"],
        "boards, then directories, and no export"
    );
    keys(&mut h, "sub/");
    assert_eq!(h.prompt_candidates(), Vec::<&str>::new());
    keys(&mut h, "<S-Tab>");
    assert_eq!(h.prompt_candidates(), ["old.svg", "sub/"]);
    keys(&mut h, "fresh");
    assert!(h.prompt_candidates().is_empty());
    h.send(Input::Key(KeyEvent::press(Key::Enter).mods(Mods {
        alt: true,
        ..Mods::default()
    })));
    h.run_until_idle();
    let buf = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(buf), Some("*whiteboard: fresh.svg*"));
    // From a board, `whiteboard` asks again; from text with a board open, it goes to it.
    h.run("whiteboard", &Args::default()).unwrap();
    assert!(h.prompt_text().is_some());
    keys(&mut h, "<Esc>");
    h.show(notes, ShowIn::Current).unwrap();
    h.run("whiteboard", &Args::default()).unwrap();
    assert_eq!(h.focused_buffer(), Some(buf), "the board used last");
    assert_eq!(h.prompt_text(), None);
    // The one answered last lists first, once it's a file.
    h.run("whiteboard-save", &Args::default()).unwrap();
    h.show(notes, ShowIn::Current).unwrap();
    h.run("whiteboard-new", &Args::default()).unwrap();
    assert_eq!(h.prompt_candidates(), ["fresh.svg", "old.svg", "sub/"]);
    keys(&mut h, "old.svg<CR>");
    let buf = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(buf), Some("*whiteboard: old.svg*"));
    h.show(notes, ShowIn::Current).unwrap();
    h.run("whiteboard-new", &Args::default()).unwrap();
    assert_eq!(h.prompt_candidates(), ["old.svg", "fresh.svg", "sub/"]);
    keys(&mut h, "<Esc>");
    h.show(buf, ShowIn::Current).unwrap();
    // `:whiteboard NAME` opens directly, relative to the buffer's directory.
    keys(&mut h, ":whiteboard sub/deep<CR>");
    let buf = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(buf), Some("*whiteboard: deep.svg*"));
    assert_eq!(h.directory(buf), Some(dir.path().join("sub").as_path()));
}
