//! The tool dial on the harness: Space held and a direction arm the tool as its key would, the
//! dead zone and a repeat change nothing, a click while it's open picks and leaves no dot, Esc,
//! the focus going, a resize and any other key close it, Space types into a label and does
//! nothing mid-gesture, describe-key answers for Space, and the right button held is the dial
//! too, erasing nothing. After the review: the dial opens under the pointer after the wheel or a
//! zoom moved the board, a right click mid-gesture ends nothing, Shift joining Space held changes
//! nothing, and the focus passing between two windows on the board leaves the dial open.
//!
//! The dial after the reel: past the ring, an inky tool's ink arc sets the ink and width as the
//! keys would, one entry for a selection and none when nothing changes, and stage one never
//! changes them; the cells screen shows the names around a `+`, the hovered one a chip in the
//! accent, the readout under them, and in stage two the inks as swatches; the pixel renderer
//! draws line art and a scrim, no disc, over the picture, keeps the chip's pixels clear, and on
//! a move draws again and presents only the dial's rectangle; and the colours are the theme's
//! `whiteboard-dial` faces.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{
    Button, Input, Key, KeyAction, KeyEvent, KeyKind, Mods, PixelRect, PixelSinkCall, PointerKind,
    Rgb, State, Style, Theme,
};
use die_whiteboard::dial::{Dial, Opener, Stage, toward};
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
        (25.0, 10.0),
        "the ring two and a half cell heights out, the dead zone a cell's larger side"
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
    mv(&mut s, (100.0, 127.0));
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
    mv(&mut s, (100.0, 127.0));
    click(&mut s, (100.0, 127.0));
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

/// The default theme's colours the dial takes: `shadow`'s grey, the ANSI red, the background,
/// and ink 3, the ANSI yellow.
const FACE: Rgb = Rgb(0x70, 0x70, 0x70);
const ACCENT: Rgb = Rgb(0xcd, 0x00, 0x00);
const BG: Rgb = Rgb(0x10, 0x10, 0x10);
const YELLOW: Rgb = Rgb(0xcd, 0xcd, 0x00);

/// The board's tool, ink and width.
fn armed(s: &mut Setup) -> (Tool, u8, u8) {
    let b = board(&mut s.h, s.buf);
    let b = b.borrow();
    (b.tool, b.ink, b.width)
}

/// The frame's cell at a row and column of the area.
fn cell(s: &mut Setup, row: usize, col: usize) -> die_core::Cell {
    let (r0, c0) = origin(&mut s.h, s.win);
    s.h.scene().row(r0 + row)[c0 + col].clone()
}

/// The point `r` units out from (100, 100), `deg` clockwise from north.
fn out(deg: f32, r: f32) -> (f32, f32) {
    let p = toward(deg) * r;
    (100.0 + p.x, 100.0 + p.y)
}

#[test]
fn past_the_ring_the_ink_arc_sets_the_ink_and_width_as_the_keys_would() {
    let mut s = setup("dial-ink");
    // Stage one never changes them: north, inside where stage two opens (30), arms the pen as
    // it is.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 76.0));
    assert_eq!(dial(&mut s).unwrap().stage, Stage::Tools);
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Pen, 1, 1));
    // Straight out keeps the ink, and the distance picks the width: thick from 60.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 85.0));
    mv(&mut s, (100.0, 35.0));
    let d = dial(&mut s).unwrap();
    assert_eq!(
        (d.tool(), d.stage),
        (
            Some(Tool::Pen),
            Stage::Ink {
                ink: 1,
                width: Some(2)
            }
        )
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Pen, 1, 2));
    assert!(
        status(&mut s).contains("pen·1  thick"),
        "{}",
        status(&mut s)
    );
    // Out, then round to 40°, in the thin band: ink 3's segment. The dot is ink 3.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 85.0));
    mv(&mut s, (100.0, 65.0));
    mv(&mut s, out(40.0, 35.0));
    assert_eq!(
        dial(&mut s).unwrap().stage,
        Stage::Ink {
            ink: 3,
            width: Some(0)
        }
    );
    let dot = pixel_at(&mut s, out(40.0, 35.0)).unwrap();
    assert_eq!(
        dot,
        [YELLOW.0, YELLOW.1, YELLOW.2, 255],
        "the preview's ink"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Pen, 3, 0));
    // A shape: east is the ellipse, whose arc has ink 3 centred on it now, so ink 5 is at 130°;
    // 50 out is medium. It's one shot, and draws in what was picked.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (115.0, 100.0));
    mv(&mut s, (140.0, 100.0));
    mv(&mut s, out(130.0, 50.0));
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Ellipse, 5, 1));
    gesture(&mut s, &[(10.0, 150.0), (60.0, 190.0)]);
    let els = elements(&mut s.h, s.buf);
    assert!(matches!(els[0].shape, Shape::Ellipse { .. }), "{els:?}");
    assert_eq!((els[0].ink, els[0].width), (5, 1));
    assert_eq!(armed(&mut s).0, Tool::Pen);
    // Text has no width: south-west, ink 2 at 165° with ink 5 centred on 225°; the width stays.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, out(225.0, 15.0));
    mv(&mut s, out(225.0, 40.0));
    mv(&mut s, out(165.0, 40.0));
    assert_eq!(
        dial(&mut s).unwrap().stage,
        Stage::Ink {
            ink: 2,
            width: None
        }
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Label, 2, 1));
    keys(&mut s.h, "<Esc>");
    // The eraser and select have no second stage, however far out.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (60.0, 100.0));
    mv(&mut s, (20.0, 100.0));
    let d = dial(&mut s).unwrap();
    assert_eq!((d.tool(), d.stage), (Some(Tool::Eraser), Stage::Tools));
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Eraser, 2, 1));
    keys(&mut s.h, "e");
    // A left click in stage two picks there and leaves no dot: ink 2 is straight out now.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 85.0));
    mv(&mut s, (100.0, 50.0));
    click(&mut s, (100.0, 50.0));
    assert_eq!(dial(&mut s), None);
    assert_eq!(armed(&mut s), (Tool::Pen, 2, 1));
    assert_eq!(elements(&mut s.h, s.buf).len(), 1, "no dot");
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Pen, 2, 1), "nothing left to pick");
}

#[test]
fn a_selection_takes_stage_twos_ink_and_width_as_one_entry_and_the_same_again_logs_none() {
    let mut s = setup("dial-selection");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    gesture(&mut s, &[(10.0, 30.0), (60.0, 30.0)]);
    keys(&mut s.h, "v");
    gesture(&mut s, &[(0.0, 0.0), (80.0, 50.0)]);
    assert_eq!(board(&mut s.h, s.buf).borrow().selection.len(), 2);
    let styles = |s: &mut Setup| -> Vec<(u8, u8)> {
        elements(&mut s.h, s.buf)
            .iter()
            .map(|e| (e.ink, e.width))
            .collect()
    };
    // The pen's arc at (150, 150): ink 4 at 60°, thick at 70 out.
    let pick = |s: &mut Setup, deg: f32| {
        mv(s, (150.0, 150.0));
        space(s, KeyKind::Press);
        mv(s, (150.0, 135.0));
        mv(s, (150.0, 115.0));
        let p = toward(deg) * 70.0;
        mv(s, (150.0 + p.x, 150.0 + p.y));
        space(s, KeyKind::Release);
    };
    pick(&mut s, 60.0);
    assert_eq!(armed(&mut s), (Tool::Pen, 4, 2));
    assert_eq!(styles(&mut s), [(4, 2), (4, 2)]);
    keys(&mut s.h, "u");
    assert_eq!(styles(&mut s), [(1, 1), (1, 1)], "one entry for both");
    keys(&mut s.h, "<C-r>");
    assert_eq!(styles(&mut s), [(4, 2), (4, 2)]);
    assert_eq!(board(&mut s.h, s.buf).borrow().selection.len(), 2);
    // Ink 4 is centred on the pen now, so straight out is ink 4 thick again: nothing changes,
    // and nothing is logged, so one `u` still takes the restyle back.
    pick(&mut s, 0.0);
    assert_eq!(styles(&mut s), [(4, 2), (4, 2)]);
    keys(&mut s.h, "u");
    assert_eq!(styles(&mut s), [(1, 1), (1, 1)]);
}

#[test]
fn the_cells_screen_shows_the_names_around_a_plus_the_hovered_one_a_chip_and_the_readout() {
    let mut s = setup_cells("dial-cells");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    // The centre at cell (20, 10) and the ring 25 units out: the names outside it on whole
    // cells, clear of its ticks; the readout under them, dimmed with nothing hovered.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    let rows = area(&mut s);
    assert_eq!(&rows[5][19..22], "pen", "{rows:#?}");
    assert_eq!(&rows[7][8..30], "select            rect", "{rows:#?}");
    assert_eq!(
        &rows[10][6..35],
        "eraser        +       ellipse",
        "{rows:#?}"
    );
    assert_eq!(&rows[12][10..31], "text            arrow", "{rows:#?}");
    assert_eq!(&rows[14][18..22], "line", "{rows:#?}");
    assert_eq!(rows[16], format!("{}pen: ink 1 · medium", " ".repeat(11)));
    assert!(
        braille(&rows[1]) >= 4,
        "the stroke is still drawn: {:?}",
        rows[1]
    );
    let name = cell(&mut s, 5, 19);
    assert_eq!((name.fg, name.bg), (FACE, BG), "the dial's grey");
    assert_ne!(cell(&mut s, 16, 11).fg, FACE, "dimmed");
    // Hovering rect: its name a chip, the accent behind the background, a cell either side.
    mv(&mut s, (120.0, 80.0));
    let rows = area(&mut s);
    assert_eq!(&rows[7][25..30], " rect", "{rows:#?}");
    for col in 25..31 {
        let c = cell(&mut s, 7, col);
        assert_eq!((c.fg, c.bg), (BG, ACCENT), "column {col}");
    }
    assert_eq!(cell(&mut s, 5, 19).bg, BG, "pen isn't a chip");
    assert_eq!(&rows[16][15..25], "rect: once", "{rows:#?}");
    assert_eq!(cell(&mut s, 16, 15).fg, FACE, "the key in the dial's grey");
    assert_eq!(cell(&mut s, 16, 21).fg, ACCENT, "the value in the accent");
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Rect);
    let rows = area(&mut s);
    assert!(
        rows.iter()
            .all(|r| !r.contains("eraser") && !r.contains("once")),
        "gone with the release: {rows:#?}"
    );
    keys(&mut s.h, "<Esc>");
    // Stage two: the inks as swatches along the arc, ink 1 straight out between marks; the
    // names its arcs cross give way, the rest dim; no width labels, since cells draw no guides.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (100.0, 76.0));
    mv(&mut s, (100.0, 40.0));
    let rows = area(&mut s);
    assert_eq!(rows[2].chars().skip(19).take(4).collect::<String>(), "[██]");
    let marks = (
        cell(&mut s, 2, 19),
        cell(&mut s, 2, 20),
        cell(&mut s, 2, 26),
    );
    assert_eq!(marks.0.fg, ACCENT, "the hovered ink's marks");
    assert_eq!(
        marks.1.fg,
        Rgb(0xd0, 0xd0, 0xd0),
        "ink 1, the default foreground"
    );
    assert_eq!(marks.2.fg, ACCENT, "ink 2, the ANSI red");
    let swatches: usize = rows.iter().map(|r| r.matches("██").count()).sum();
    assert_eq!(swatches, 6, "{rows:#?}");
    assert!(
        rows.iter()
            .all(|r| !r.contains("rect") && !r.contains("ellipse")),
        "{rows:#?}"
    );
    assert!(rows[10].contains("eraser"), "{rows:#?}");
    assert_ne!(cell(&mut s, 10, 6).fg, FACE, "dimmed");
    assert!(
        rows[4..8].iter().all(|r| !r.contains(['2', '4', '8'])),
        "{rows:#?}"
    );
    assert_eq!(rows[5].trim(), "pen", "the chip stays");
    assert!(
        rows.iter().any(|r| r.trim() == "pen: ink 1 · thick"),
        "{rows:#?}"
    );
    space(&mut s, KeyKind::Release);
    assert_eq!(armed(&mut s), (Tool::Pen, 1, 2));
}

#[test]
fn the_pixel_renderer_draws_line_art_on_a_scrim_keeps_the_chip_clear_and_presents_the_dial_alone() {
    let mut s = setup("dial-pixels");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let picture = inked_count(&mut s);
    assert!(picture > 0);
    mv(&mut s, (100.0, 100.0));
    presents(&mut s);
    space(&mut s, KeyKind::Press);
    // The present on opening: the dial's square about the centre, (200, 200) in pixels, and
    // not the buffer's 1000 × 600.
    let opened = presents(&mut s);
    assert_eq!(opened.len(), 1, "{opened:?}");
    let r = opened[0];
    assert!(r.x < 200 && r.x + r.width > 200 && r.y < 200 && r.y + r.height > 200);
    assert!(r.width < 500 && r.height < 500, "{r:?}");
    assert!(inked(&mut s, (30.0, 10.0)), "the stroke is under it still");
    // No disc and no wedge: off the lines, the scrim is the background, mostly opaque.
    let scrim = pixel_at(&mut s, (88.0, 88.0)).unwrap();
    assert!(
        scrim[3] > 180 && scrim[0].abs_diff(scrim[1]) < 2 && scrim[0] < 20,
        "{scrim:?}"
    );
    // On the ring at 30°: in rect's sector, and off the needle to (120, 80) and its crosshair.
    let ring = out(30.0, 25.0);
    assert!(
        pixel_at(&mut s, ring).unwrap()[0] < 100,
        "nothing hovered: no stretch"
    );
    // Hovering rect: its stretch of the ring thick in the accent, the needle's dot at the
    // pointer in the accent, and the chip's pixels clear, so its cells show solid.
    mv(&mut s, (120.0, 80.0));
    let hovered = presents(&mut s);
    assert_eq!(hovered.len(), 1, "{hovered:?}");
    assert!(hovered[0].width < 500, "{hovered:?}");
    let stretch = pixel_at(&mut s, ring).unwrap();
    assert_eq!(stretch, [ACCENT.0, ACCENT.1, ACCENT.2, 255], "the stretch");
    let dot = pixel_at(&mut s, (120.0, 80.0)).unwrap();
    assert_eq!(dot, [ACCENT.0, ACCENT.1, ACCENT.2, 255], "the dot");
    // The chip is cells 25 to 30 of row 7: pixels 250 to 310 across, 140 to 160 down.
    assert_eq!(pixel_px(&mut s, (280.0, 150.0)).unwrap()[3], 0, "clear");
    assert!(
        pixel_px(&mut s, (240.0, 150.0)).unwrap()[3] > 0,
        "the scrim beside it"
    );
    let rows = area(&mut s);
    assert_eq!(&rows[7][8..30], "select            rect", "{rows:#?}");
    // A move within the sector: the needle follows, so it draws and presents, within the
    // dial's own rectangle.
    mv(&mut s, (121.0, 82.0));
    let moved = presents(&mut s);
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert!(moved[0].width <= hovered[0].width.max(r.width), "{moved:?}");
    let dot = pixel_at(&mut s, (121.0, 82.0)).unwrap();
    assert_eq!(dot, [ACCENT.0, ACCENT.1, ACCENT.2, 255]);
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Rect);
    assert_eq!(presents(&mut s).len(), 1, "the picture put back under it");
    assert!(!inked(&mut s, (88.0, 88.0)));
    assert!(!inked(&mut s, (121.0, 82.0)));
    assert_eq!(
        inked_count(&mut s),
        picture,
        "the committed picture is as it was"
    );
    assert_eq!(elements(&mut s.h, s.buf).len(), 1);
    assert!(area(&mut s).iter().all(|r| !r.contains("rect")));
}

#[test]
fn the_dial_takes_its_colours_from_the_whiteboard_dial_faces_and_a_theme_restyles_it() {
    let mut s = setup_cells("dial-faces");
    // Unstyled, the grey is `shadow`'s and the accent the theme's ANSI red.
    let look = die_whiteboard::dial_look(&s.h);
    assert_eq!((look.face, look.accent, look.bg), (FACE, ACCENT, BG));
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    assert_eq!(cell(&mut s, 7, 26).bg, ACCENT);
    space(&mut s, KeyKind::Release);
    // A theme that styles them: the next dial is in its colours.
    let (grey, blue) = (Rgb(0x88, 0x88, 0x88), Rgb(0x00, 0x88, 0xff));
    s.h.set_theme(
        Theme::default()
            .face("whiteboard-dial", Style::new().fg(grey))
            .face("whiteboard-dial-accent", Style::new().fg(blue)),
    );
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    mv(&mut s, (120.0, 80.0));
    assert_eq!(cell(&mut s, 7, 26).bg, blue);
    assert_eq!(cell(&mut s, 5, 19).fg, grey, "pen, in the theme's grey");
    space(&mut s, KeyKind::Release);
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
        &[(50.0, 30.0), (50.0, 57.0)],
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
    right(&mut s, PointerKind::Drag(Button::Right), (100.0, 73.0));
    right(&mut s, PointerKind::Release(Button::Right), (100.0, 73.0));
    assert_eq!(tool(&mut s), Tool::Pen);
    // And a right press while Space holds it does nothing either.
    mv(&mut s, (100.0, 100.0));
    space(&mut s, KeyKind::Press);
    right(&mut s, PointerKind::Press(Button::Right), (127.0, 100.0));
    right(&mut s, PointerKind::Release(Button::Right), (127.0, 100.0));
    let d = dial(&mut s).expect("still open");
    assert_eq!((d.centre.x, d.by), (100.0, Opener::Space));
    mv(&mut s, (127.0, 100.0));
    space(&mut s, KeyKind::Release);
    assert_eq!(tool(&mut s), Tool::Ellipse);
    keys(&mut s.h, "<Esc>");
    // A left press while the right button holds it picks, and draws nothing.
    right(&mut s, PointerKind::Press(Button::Right), (100.0, 100.0));
    right(&mut s, PointerKind::Drag(Button::Right), (100.0, 127.0));
    press(&mut s, (100.0, 127.0));
    assert_eq!(dial(&mut s), None);
    assert_eq!(tool(&mut s), Tool::Line);
    release(&mut s, (100.0, 127.0));
    right(&mut s, PointerKind::Release(Button::Right), (100.0, 127.0));
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
