//! Restarts, as `:rebuild` makes them: the session handed over to a file, DIE gone, and a new DIE
//! started on the same file, twice in a row. Each board comes back as a working board from its
//! file (written at the hand-over), with its view, tool, ink, width and selection, the one used
//! last in a window.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Config, Harness, State};
use die_vim::Vim;
use die_whiteboard::Whiteboard;
use die_whiteboard::surface::Tool;
use die_whiteboard::svg;
use std::time::Duration;

fn up(state: &TempDir) -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> =
        vec![Box::new(Vim::default()), Box::new(Whiteboard::for_tests())];
    let config = Config {
        session: Some(state.path().to_path_buf()),
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    let lost: Vec<&String> = h
        .messages
        .iter()
        .filter(|m| m.contains("didn't come back") || m.contains("dropped"))
        .collect();
    assert!(lost.is_empty(), "{lost:?}");
    h
}

fn restart(h: Harness, state: &TempDir) -> Harness {
    let mut h = h;
    h.hand_over_session().unwrap();
    drop(h);
    up(state)
}

fn boards(h: &mut Harness) -> Vec<String> {
    let mut names: Vec<String> = h
        .buffer_list()
        .into_iter()
        .filter_map(|b| h.buffer_name(b).map(String::from))
        .filter(|n| n.starts_with("*whiteboard"))
        .collect();
    names.sort();
    names
}

#[test]
fn boards_come_back_working_after_two_restarts() {
    let state = TempDir::new("session-state");
    let dir = TempDir::new("session-boards");
    let mut h = up(&state);
    h.open_text("notes", "notes\n").unwrap();
    open(&mut h, &dir.path().join("one.svg"));
    open(&mut h, &dir.path().join("two.svg"));
    let two = h.focused_buffer().unwrap();
    let win = h.focused().unwrap();
    let mut s = Setup {
        h,
        dir,
        buf: two,
        win,
        path: std::path::PathBuf::new(),
    };
    keys(&mut s.h, "3]r");
    gesture(&mut s, &[(10.0, 10.0), (50.0, 50.0)]);
    gesture(&mut s, &[(10.0, 100.0), (60.0, 100.0)]);
    keys(&mut s.h, "v");
    click(&mut s, (30.0, 30.0));
    assert!(
        !s.dir.path().join("two.svg").exists(),
        "not yet a second old"
    );
    let dir = s.dir;
    let h = restart(s.h, &state);
    let mut h = h;
    assert!(
        dir.path().join("two.svg").exists(),
        "written at the hand-over"
    );
    assert_eq!(
        boards(&mut h),
        ["*whiteboard: one.svg*", "*whiteboard: two.svg*"]
    );
    let two = h.find_buffer("*whiteboard: two.svg*").unwrap();
    assert_eq!(h.focused_buffer(), Some(two), "the one used last is shown");
    assert_eq!(h.state(), State::Normal);
    {
        let shared = board(&mut h, two);
        let st = shared.borrow();
        assert_eq!(st.board.len(), 2);
        assert_eq!((st.tool, st.ink, st.width), (Tool::Select, 3, 2));
        assert_eq!(st.selection, [st.board.elements[0].id]);
        assert!(!st.dirty);
    }
    let one = h.find_buffer("*whiteboard: one.svg*").unwrap();
    assert!(
        !h.window_list()
            .iter()
            .any(|w| h.window_buffer(*w) == Some(one))
    );
    // It works: a nudge moves the selection, autosaves, and survives another restart.
    let win = h.focused().unwrap();
    let mut s = Setup {
        h,
        dir,
        buf: two,
        win,
        path: std::path::PathBuf::new(),
    };
    keys(&mut s.h, "j");
    assert!(s.h.next_timer().is_some(), "autosave armed");
    keys(&mut s.h, "p");
    gesture(&mut s, &[(10.0, 150.0), (60.0, 150.0)]);
    s.h.advance(Duration::from_secs(2));
    let dir = s.dir;
    let mut h = restart(s.h, &state);
    let two = h.find_buffer("*whiteboard: two.svg*").unwrap();
    {
        let shared = board(&mut h, two);
        let st = shared.borrow();
        assert_eq!(st.board.len(), 3);
        let die_whiteboard::model::Shape::Rect { a, .. } = st.board.elements[0].shape else {
            panic!()
        };
        assert_eq!(
            a,
            die_whiteboard::model::Point::new(10.0, 20.0),
            "nudged a cell down"
        );
    }
    assert_eq!(h.focused_buffer(), Some(two));
    // Drawn, in pixels: the board places its buffer and the rectangle's corner is inked.
    let win = h.focused().unwrap();
    let mut s = Setup {
        h,
        dir,
        buf: two,
        win,
        path: std::path::PathBuf::new(),
    };
    let scene = s.h.scene();
    assert_eq!(scene.placements.len(), 1, "{}", scene.snapshot());
    assert!(inked(&mut s, (10.0, 20.0)), "the nudged rectangle's corner");
}

#[test]
fn a_hand_over_that_cant_write_brings_the_unsaved_board_back_and_writes_it_once_it_can() {
    use std::os::unix::fs::PermissionsExt;
    let state = TempDir::new("session-ro-state");
    let dir = TempDir::new("session-ro-boards");
    let ro = dir.path().join("ro");
    std::fs::create_dir_all(&ro).unwrap();
    let path = ro.join("ro.svg");
    let mut h = up(&state);
    h.open_text("notes", "notes\n").unwrap();
    open(&mut h, &path);
    let buf = h.focused_buffer().unwrap();
    let win = h.focused().unwrap();
    let mut s = Setup {
        h,
        dir,
        buf,
        win,
        path: path.clone(),
    };
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    s.h.advance(Duration::from_secs(2));
    assert_eq!(
        svg::read(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .len(),
        1
    );
    // The directory can't be written to now: two more strokes stay unsaved.
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    gesture(&mut s, &[(10.0, 30.0), (60.0, 30.0)]);
    gesture(&mut s, &[(10.0, 50.0), (60.0, 50.0)]);
    s.h.advance(Duration::from_secs(2));
    assert!(
        status(&mut s).contains("ro.svg: Permission denied"),
        "{}",
        status(&mut s)
    );
    let dir = s.dir;
    let mut h = restart(s.h, &state);
    assert_eq!(
        svg::read(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .len(),
        1,
        "the file is stale still"
    );
    let two = h.find_buffer("*whiteboard: ro.svg*").unwrap();
    {
        let shared = board(&mut h, two);
        let st = shared.borrow();
        assert_eq!(st.board.len(), 3, "the unsaved board is back");
        assert!(st.dirty, "and still unsaved");
        assert!(
            st.error
                .as_deref()
                .is_some_and(|e| e.contains("Permission denied")),
            "{:?}",
            st.error
        );
    }
    assert!(
        h.messages
            .iter()
            .any(|m| m.contains("couldn't be written at the hand-over") && m.contains("ro.svg")),
        "{:?}",
        h.messages
    );
    assert!(
        h.next_timer().is_some(),
        "autosave armed, to write it once it can"
    );
    // Once it can be written, it is.
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    h.advance(Duration::from_secs(2));
    assert_eq!(
        svg::read(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .len(),
        3
    );
    assert!(!board(&mut h, two).borrow().dirty);
    drop(dir);
}
