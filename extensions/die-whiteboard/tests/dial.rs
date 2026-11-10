//! The tool dial on the harness: Space held and a direction arm the tool as its key would, the
//! dead zone and a repeat change nothing, a click while it's open picks and leaves no dot, Esc,
//! the focus going, a resize and any other key close it, Space types into a label and does
//! nothing mid-gesture, the cells screen shows the eight names with the hovered one reversed,
//! the pixel renderer draws the disc over the picture and presents its rectangle alone,
//! describe-key answers for Space, and the right button held is the dial too, erasing nothing.
//! After the review: the dial opens under the pointer after the wheel or a zoom moved the board,
//! a right click mid-gesture ends nothing, Shift joining Space held changes nothing, and the
//! focus passing between two windows on the board leaves the dial open.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{
    Button, Input, Key, KeyAction, KeyEvent, KeyKind, Mods, PixelRect, PixelSinkCall, PointerKind,
    State,
};
use die_whiteboard::dial::{Dial, Opener};
use die_whiteboard::model::Shape;
use die_whiteboard::surface::Tool;

fn space(s: &mut Setup, kind: KeyKind) {
    let ev = KeyEvent::press(Key::Char(' ')).kind(kind);
    let errors = s.h.send(Input::Key(ev));
    assert!(errors.is_empty(), "{kind:?}: {errors:?}");
}

fn mv(s: &mut Setup, at: (f32, f32)) {
    pointer(s, PointerKind::Move, at, Mods::default(), 0);
}

fn right(s: &mut Setup, kind: PointerKind, at: (f32, f32)) {
    pointer(s, kind, at, Mods::default(), 1);
}

fn dial(s: &mut Setup) -> Option<Dial> {
    board(&mut s.h, s.buf).borrow().dial()
}

fn tool(s: &mut Setup) -> Tool {
    board(&mut s.h, s.buf).borrow().tool
}

fn braille(row: &str) -> usize {
    row.chars()
        .filter(|c| ('\u{2800}'..='\u{28ff}').contains(c))
        .count()
}

fn presents(s: &mut Setup) -> Vec<PixelRect> {
    s.h.fake_pixels()
        .take_calls()
        .into_iter()
        .filter_map(|c| match c {
            PixelSinkCall::Present(_, r) => Some(r),
            _ => None,
        })
        .collect()
}

#[test]
fn space_held_and_a_direction_arm_the_tool_as_its_key_would() {
    let mut s = setup("dial");
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    let d = dial(&mut s).expect("open on the press");
    assert_eq!(
        (d.centre.x, d.centre.y, d.hovered, d.by),
        (100.0, 100.0, None, Opener::Space)
    );
    assert_eq!(
        (d.radius, d.dead),
        (30.0, 10.0),
        "three cell heights, one cell's larger side"
    );
    mv(&mut s, (120.0, 80.0));
    assert_eq!(dial(&mut s).unwrap().hovered, Some(1), "north-east: rect");
    space(&mut s, KeyKind::Release);
    assert_eq!(dial(&mut s), None);
    assert_eq!(tool(&mut s), Tool::Rect);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 40.0)]);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 1);
    assert!(matches!(els[0].shape, Shape::Rect { .. }), "{els:?}");
    assert_eq!(tool(&mut s), Tool::Pen, "one shot, as r is");
    // The dial opens where the pointer was last seen, and the eraser toggles, as e does.
    for expected in [Tool::Eraser, Tool::Pen] {
        mv(&mut s, (100.0, 100.0));
        space(&mut s, KeyKind::Press);
        assert_eq!(dial(&mut s).unwrap().centre.x, 100.0);
        mv(&mut s, (60.0, 100.0));
        space(&mut s, KeyKind::Release);
        assert_eq!(tool(&mut s), expected);
    }
    // Select stays, as v does; a release inside the dead zone changes nothing.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (80.0, 80.0));
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Select);
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (104.0, 104.0));
    assert_eq!(dial(&mut s).unwrap().hovered, None, "inside the dead zone");
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Select, "unchanged");
    // Repeats of Space change nothing: the dial stays, hovering where it was.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 130.0));
    space(&mut s, KeyKind::Repeat);
    space(&mut s, KeyKind::Repeat);
    assert_eq!(dial(&mut s).unwrap().hovered, Some(4), "south: line");
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Line);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "the board is as it was");
    keys(&mut s.h, "<Esc>");
    assert_eq!(tool(&mut s), Tool::Pen);
    // The dials made no entry: one undo takes the rectangle, and there's nothing after it.
    keys(&mut s.h, "u");
    assert_eq!(elements(&mut s.h, s.buf).len(), 0);
    assert_eq!(keys_errors(&mut s.h, "u"), ["nothing to undo"]);
    // With the pointer never seen, the dial opens at the area's centre.
    let mut s = setup("dial-centre");
    space(&mut s, KeyKind::Press);
    let g = s.h.surface_area(s.win).unwrap();
    let d = dial(&mut s).unwrap();
    assert_eq!(
        (d.centre.x, d.centre.y),
        (g.text_w as f32 * 2.5, g.rows as f32 * 5.0)
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Pen);
}

#[test]
fn a_click_while_open_picks_and_leaves_no_dot() {
    let mut s = setup("dial-click");
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 130.0));
    click(&mut s, (100.0, 130.0));
    assert_eq!(dial(&mut s), None, "the click picked");
    assert_eq!(tool(&mut s), Tool::Line);
    assert_eq!(elements(&mut s.h, s.buf).len(), 0, "no dot");
    space(&mut s, KeyKind::Release);
    assert_eq!(
        tool(&mut s),
        Tool::Line,
        "the release has nothing left to do"
    );
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let els = elements(&mut s.h, s.buf);
    assert!(
        matches!(els[0].shape, Shape::Line { arrow: false, .. }),
        "{els:?}"
    );
    assert_eq!(tool(&mut s), Tool::Pen);
    // A click in the dead zone cancels, and leaves no dot either.
    space(&mut s, KeyKind::Press);
    click(&mut s, (62.0, 12.0));
    assert_eq!(dial(&mut s), None);
    assert_eq!(tool(&mut s), Tool::Pen);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    space(&mut s, KeyKind::Release);
}

#[test]
fn esc_the_focus_going_a_resize_and_any_other_key_close_it() {
    let mut s = setup("dial-cancel");
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    keys(&mut s.h, "<Esc>");
    assert_eq!(dial(&mut s), None);
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Pen, "Esc chose nothing");
    space(&mut s, KeyKind::Press);
    assert!(s.h.send(Input::Focus(false)).is_empty());
    assert_eq!(dial(&mut s), None, "the focus went");
    assert!(s.h.send(Input::Focus(true)).is_empty());
    space(&mut s, KeyKind::Release);
    space(&mut s, KeyKind::Press);
    assert!(dial(&mut s).is_some());
    s.h.set_window_size(90, 30);
    s.h.run_until_idle();
    assert_eq!(dial(&mut s), None, "a resize");
    space(&mut s, KeyKind::Release);
    s.h.set_window_size(100, 30);
    s.h.run_until_idle();
    // A bound key runs its command and closes the dial; an unbound one is undefined, as ever.
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    keys(&mut s.h, "3");
    assert_eq!(dial(&mut s), None);
    assert_eq!(board(&mut s.h, s.buf).borrow().ink, 3);
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Pen, "rect wasn't picked");
    space(&mut s, KeyKind::Press);
    assert_eq!(keys_errors(&mut s.h, "z"), ["z is undefined"]);
    assert_eq!(dial(&mut s), None);
    space(&mut s, KeyKind::Release);
}

#[test]
fn space_types_into_a_label_and_does_nothing_mid_gesture() {
    let mut s = setup("dial-label");
    keys(&mut s.h, "t");
    click(&mut s, (20.0, 20.0));
    assert_eq!(s.h.state(), State::Insert);
    keys(&mut s.h, "a");
    space(&mut s, KeyKind::Press);
    space(&mut s, KeyKind::Release);
    keys(&mut s.h, "b<Esc>");
    let els = elements(&mut s.h, s.buf);
    assert!(
        matches!(&els[0].shape, Shape::Label { text, .. } if text == "a b"),
        "{els:?}"
    );
    assert_eq!(dial(&mut s), None);
    // Mid-stroke, Space is swallowed and the stroke goes on.
    press(&mut s, (10.0, 60.0));
    drag(&mut s, (30.0, 60.0));
    space(&mut s, KeyKind::Press);
    assert_eq!(dial(&mut s), None);
    space(&mut s, KeyKind::Release);
    drag(&mut s, (50.0, 60.0));
    release(&mut s, (60.0, 60.0));
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 2);
    assert!(matches!(els[1].shape, Shape::Stroke { .. }));
    assert_eq!(tool(&mut s), Tool::Pen);
}

#[test]
fn the_cells_screen_shows_the_eight_names_with_the_hovered_one_reversed() {
    let mut s = setup_cells("dial-cells");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    // The centre at cell (20, 10): the names on whole cells around it.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    let rows = area(&mut s);
    assert_eq!(&rows[8][19..22], "pen", "{rows:#?}");
    assert_eq!(&rows[9][12..27], "select     rect", "{rows:#?}");
    assert_eq!(&rows[10][12..30], "eraser  +  ellipse", "{rows:#?}");
    assert_eq!(&rows[11][14..28], "text     arrow", "{rows:#?}");
    assert_eq!(&rows[12][18..22], "line", "{rows:#?}");
    assert!(
        braille(&rows[1]) >= 4,
        "the stroke is still drawn: {:?}",
        rows[1]
    );
    let (r0, c0) = origin(&mut s.h, s.win);
    let scene = s.h.scene();
    let rect = &scene.row(r0 + 9)[c0 + 23];
    let pen = &scene.row(r0 + 8)[c0 + 19];
    assert_eq!(rect.glyph.to_string(), "r");
    assert_eq!(
        (rect.fg, rect.bg),
        (pen.bg, pen.fg),
        "rect is reversed, pen isn't"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Rect);
    let rows = area(&mut s);
    assert!(
        !rows[10].contains("eraser"),
        "gone with the release: {rows:#?}"
    );
    assert!(braille(&rows[1]) >= 4, "{:?}", rows[1]);
}

#[test]
fn the_pixel_renderer_draws_the_disc_over_the_picture_and_presents_its_rectangle_alone() {
    let mut s = setup("dial-pixels");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let picture = inked_count(&mut s);
    assert!(picture > 0);
    mv(&mut s, (100.0, 100.0));
    presents(&mut s);
    space(&mut s, KeyKind::Press);
    // The disc, 30 units around (100, 100), at 2x with 2 px of edge: the present on opening.
    let disc = PixelRect::new(138, 138, 124, 124);
    assert_eq!(presents(&mut s), [disc]);
    assert!(
        inked(&mut s, (100.0, 80.0)),
        "the disc is translucent, not clear"
    );
    assert!(inked(&mut s, (30.0, 10.0)), "the stroke is under it still");
    assert!(!inked(&mut s, (150.0, 100.0)), "nothing outside the disc");
    let plain = pixel_at(&mut s, (100.0, 80.0)).unwrap()[3];
    mv(&mut s, (120.0, 80.0));
    assert_eq!(
        presents(&mut s),
        [disc],
        "a hover change presents the disc alone"
    );
    let hovered = pixel_at(&mut s, (115.0, 85.0)).unwrap()[3];
    assert!(
        hovered > plain,
        "the hovered sector is filled: {hovered} over {plain}"
    );
    let rows = area(&mut s);
    assert_eq!(
        &rows[9][12..27],
        "select     rect",
        "the names are cells: {rows:#?}"
    );
    mv(&mut s, (121.0, 81.0));
    assert_eq!(
        presents(&mut s),
        [],
        "a move within the sector presents nothing"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Rect);
    assert_eq!(presents(&mut s), [disc], "the picture put back under it");
    assert!(!inked(&mut s, (100.0, 80.0)));
    assert_eq!(
        inked_count(&mut s),
        picture,
        "the committed picture is as it was"
    );
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    assert!(area(&mut s).iter().all(|r| !r.contains("rect")));
}

#[test]
fn describe_key_answers_for_space() {
    let s = setup("dial-describe");
    let d =
        s.h.describe_key(&[KeyEvent::press(Key::Char(' '))])
            .unwrap();
    assert_eq!(
        d.action,
        KeyAction::Surface {
            kind: "whiteboard",
            declared: true
        }
    );
    // The key is written as `key_notation` writes a plain Space, a blank, since the notation
    // is a macro register's text: describe-key's own `<Space>` is a later cosmetic.
    assert!(
        d.to_string()
            .starts_with("  in Normal goes to the whiteboard surface, which handles it."),
        "{d}"
    );
    let shifted = KeyEvent::press(Key::Char(' ')).mods(SHIFT);
    let d = s.h.describe_key(&[shifted]).unwrap();
    assert_eq!(
        d.action,
        KeyAction::Surface {
            kind: "whiteboard",
            declared: false
        },
        "Shift-Space is nobody's yet"
    );
}

#[test]
fn the_right_button_held_is_the_dial_too_and_erases_nothing() {
    let mut s = setup("dial-right");
    gesture(&mut s, &[(10.0, 50.0), (100.0, 50.0)]);
    right(&mut s, PointerKind::Press(Button::Right), (50.0, 20.0));
    let d = dial(&mut s).expect("open on the press");
    assert_eq!(
        (d.centre.x, d.centre.y, d.by),
        (50.0, 20.0, Opener::RightButton)
    );
    right(&mut s, PointerKind::Drag(Button::Right), (30.0, 0.0));
    assert_eq!(dial(&mut s).unwrap().hovered, Some(7), "north-west: select");
    right(&mut s, PointerKind::Release(Button::Right), (30.0, 0.0));
    assert_eq!(dial(&mut s), None);
    assert_eq!(tool(&mut s), Tool::Select);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    // A right click without motion changes nothing and erases nothing.
    right(&mut s, PointerKind::Press(Button::Right), (60.0, 50.0));
    right(&mut s, PointerKind::Release(Button::Right), (60.0, 50.0));
    assert_eq!(tool(&mut s), Tool::Select);
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    // A right drag across the stroke leaves it: the eraser is a sector and e now.
    gesture_with(
        &mut s,
        &[(50.0, 30.0), (50.0, 70.0)],
        Mods::default(),
        Button::Right,
    );
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "not erased");
    assert_eq!(tool(&mut s), Tool::Line, "south picked the line");
    keys(&mut s.h, "<Esc>");
    // Space while the right button holds the dial does nothing to it.
    right(&mut s, PointerKind::Press(Button::Right), (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    space(&mut s, KeyKind::Release);
    assert_eq!(dial(&mut s).map(|d| d.by), Some(Opener::RightButton));
    right(&mut s, PointerKind::Drag(Button::Right), (100.0, 60.0));
    right(&mut s, PointerKind::Release(Button::Right), (100.0, 60.0));
    assert_eq!(tool(&mut s), Tool::Pen);
    // And a right press while Space holds it does nothing either.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    right(&mut s, PointerKind::Press(Button::Right), (140.0, 100.0));
    right(&mut s, PointerKind::Release(Button::Right), (140.0, 100.0));
    let d = dial(&mut s).expect("still open");
    assert_eq!((d.centre.x, d.by), (100.0, Opener::Space));
    mv(&mut s, (140.0, 100.0));
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Ellipse);
    keys(&mut s.h, "<Esc>");
    // A left press while the right button holds it picks, and draws nothing.
    right(&mut s, PointerKind::Press(Button::Right), (100.0, 100.0));
    right(&mut s, PointerKind::Drag(Button::Right), (100.0, 130.0));
    press(&mut s, (100.0, 130.0));
    assert_eq!(dial(&mut s), None);
    assert_eq!(tool(&mut s), Tool::Line);
    release(&mut s, (100.0, 130.0));
    right(&mut s, PointerKind::Release(Button::Right), (100.0, 130.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "no dot");
    assert_eq!(tool(&mut s), Tool::Line);
    // The dial never touched the undo log: one entry, the stroke's.
    keys(&mut s.h, "u");
    assert_eq!(elements(&mut s.h, s.buf).len(), 0);
    assert_eq!(keys_errors(&mut s.h, "u"), ["nothing to undo"]);
}

#[test]
fn the_dial_opens_under_the_pointer_after_the_wheel_and_a_zoom() {
    let mut s = setup("dial-view");
    // The wheel moves the board a cell under a pointer that stays, and sends no Move: the
    // dial opens under the pointer, not where the board point it last reported is now.
    mv(&mut s, (100.0, 100.0));
    pointer(
        &mut s,
        PointerKind::Scroll { dx: 0.0, dy: 1.0 },
        (100.0, 100.0),
        Mods::default(),
        0,
    );
    assert_eq!(board(&mut s.h, s.buf).borrow().view.pan, (0.0, -20.0));
    space(&mut s, KeyKind::Press);
    let d = dial(&mut s).expect("open");
    assert_eq!(
        (d.centre.x, d.centre.y),
        (100.0, 110.0),
        "a cell down the board"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Pen, "a tap picks nothing still");
    keys(&mut s.h, "0");
    // The zoom keys: the pointer at the area's physical (100, 80), two `+` about the centre,
    // and the dial opens on the board point under that pixel now, about 8 cells from the stale
    // one. A move north-east of the pointer hovers rect, as the directions are the pointer's.
    mv(&mut s, (50.0, 40.0));
    keys(&mut s.h, "++");
    let view = board(&mut s.h, s.buf).borrow().view;
    assert!(view.zoom > 1.5);
    let under = view.to_board(100.0, 80.0, 2.0);
    assert!(under.dist(die_whiteboard::model::Point::new(50.0, 40.0)) > 50.0);
    space(&mut s, KeyKind::Press);
    let d = dial(&mut s).expect("open");
    assert!(d.centre.dist(under) < 0.01, "{:?} for {under:?}", d.centre);
    assert_eq!(d.hovered, None);
    mv(&mut s, (70.0, 20.0));
    assert_eq!(
        dial(&mut s).unwrap().hovered,
        Some(1),
        "north-east of the pointer"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Rect);
    keys(&mut s.h, "<Esc>0");
}

#[test]
fn a_right_click_mid_gesture_ends_nothing() {
    let mut s = setup("dial-mid");
    // The right press mid-stroke is refused the dial; its release doesn't end the stroke, which
    // goes on to where the left button lifts.
    press(&mut s, (10.0, 60.0));
    drag(&mut s, (30.0, 60.0));
    right(&mut s, PointerKind::Press(Button::Right), (30.0, 60.0));
    assert_eq!(dial(&mut s), None, "refused mid-gesture");
    right(&mut s, PointerKind::Release(Button::Right), (30.0, 60.0));
    drag(&mut s, (50.0, 60.0));
    drag(&mut s, (70.0, 60.0));
    release(&mut s, (80.0, 60.0));
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 1, "{els:?}");
    let Shape::Stroke { points, .. } = &els[0].shape else {
        panic!("{els:?}");
    };
    assert_eq!(
        points.last().map(|p| p.x),
        Some(80.0),
        "to the lift: {points:?}"
    );
    // An erase the same way crosses the stroke after the right click.
    keys(&mut s.h, "e");
    press(&mut s, (50.0, 10.0));
    drag(&mut s, (50.0, 40.0));
    right(&mut s, PointerKind::Press(Button::Right), (50.0, 40.0));
    right(&mut s, PointerKind::Release(Button::Right), (50.0, 40.0));
    drag(&mut s, (50.0, 70.0));
    release(&mut s, (50.0, 80.0));
    assert_eq!(elements(&mut s.h, s.buf).len(), 0, "erased after the click");
    keys(&mut s.h, "e");
    keys(&mut s.h, "u");
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "one erase, one entry");
    // The middle button's release ends nothing either.
    press(&mut s, (10.0, 100.0));
    drag(&mut s, (30.0, 100.0));
    pointer(
        &mut s,
        PointerKind::Release(Button::Middle),
        (30.0, 100.0),
        Mods::default(),
        1,
    );
    drag(&mut s, (50.0, 100.0));
    release(&mut s, (60.0, 100.0));
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 2);
    let Shape::Stroke { points, .. } = &els[1].shape else {
        panic!("{els:?}");
    };
    assert_eq!(points.last().map(|p| p.x), Some(60.0));
}

#[test]
fn shift_joining_space_held_changes_nothing() {
    let mut s = setup("dial-shift");
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    // Shift pressed ahead of a constrained shape: the OS repeats Space shifted now. Each repeat
    // is the dial's, not an undefined key, and the shifted release picks.
    let shifted = KeyEvent::press(Key::Char(' ')).mods(SHIFT);
    for _ in 0..3 {
        let errors = s.h.send(Input::Key(shifted.clone().kind(KeyKind::Repeat)));
        assert_eq!(errors, Vec::<String>::new());
    }
    assert_eq!(dial(&mut s).unwrap().hovered, Some(1), "still open on rect");
    let errors = s.h.send(Input::Key(shifted.kind(KeyKind::Release)));
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(dial(&mut s), None);
    assert_eq!(tool(&mut s), Tool::Rect);
    // With no dial of Space's open, a shifted Space is nobody's, as before.
    let shifted = KeyEvent::press(Key::Char(' ')).mods(SHIFT);
    let errors = s.h.send(Input::Key(shifted.clone().kind(KeyKind::Repeat)));
    assert_eq!(errors, ["  is undefined"]);
    assert_eq!(tool(&mut s), Tool::Rect);
}

#[test]
fn the_focus_passing_between_two_windows_on_the_board_leaves_the_dial_open() {
    let mut s = setup("dial-two");
    keys(&mut s.h, "<C-w>s");
    assert_eq!(s.h.window_list().len(), 2);
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    assert_eq!(dial(&mut s).unwrap().hovered, Some(1));
    let was = s.h.focused().unwrap();
    keys(&mut s.h, "<C-w>w");
    assert_ne!(s.h.focused(), Some(was), "the other window");
    assert_eq!(
        dial(&mut s).map(|d| d.hovered),
        Some(Some(1)),
        "the board never lost the focus, so the dial stays"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Rect);
    // And no activation grace is armed: the next press draws.
    gesture(&mut s, &[(10.0, 10.0), (60.0, 40.0)]);
    let els = elements(&mut s.h, s.buf);
    assert_eq!(els.len(), 1, "{els:?}");
    assert!(matches!(els[0].shape, Shape::Rect { .. }));
}
