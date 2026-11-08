//! VT sequences in, die-core cells out: colors, attributes, wide characters, scroll regions,
//! the alternate screen, the cursor, and which rows a frame carries.

use die_test_cap as _;

use die_core::{Cell, CursorShape, Glyph, Rgb, Underline};
use die_term::{Frame, Term};

const FG: Rgb = Rgb(0xd0, 0xd0, 0xd0);
const BG: Rgb = Rgb(0x10, 0x10, 0x10);

fn ansi() -> [Rgb; 16] {
    std::array::from_fn(|i| Rgb(i as u8, 100 + i as u8, 200))
}

fn term() -> Term {
    let mut t = Term::new(40, 10, (18, 36)).unwrap();
    t.set_colors(FG, BG, None, &ansi());
    t
}

fn feed(t: &mut Term, s: &str) {
    let mut b = s.as_bytes();
    while !b.is_empty() {
        b = &b[t.feed(b)..];
    }
}

/// Every row of the viewport.
fn screen(t: &mut Term) -> Frame {
    t.frame(true).expect("a full frame")
}

fn row(f: &Frame, y: u16) -> &[Cell] {
    &f.changed.iter().find(|r| r.0 == y).expect("row in frame").1
}

fn text(cells: &[Cell]) -> String {
    cells
        .iter()
        .map(|c| c.glyph.to_string())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn colors_true_palette_and_default() {
    let mut t = term();
    feed(
        &mut t,
        "\x1b[38;2;1;2;3;48;2;4;5;6mT\x1b[0m\x1b[31;42mP\x1b[0m\x1b[38;5;196mX\x1b[0m\x1b[91mB\x1b[0mD",
    );
    let f = screen(&mut t);
    let r = row(&f, 0);
    assert_eq!(
        (r[0].fg, r[0].bg),
        (Rgb(1, 2, 3), Rgb(4, 5, 6)),
        "true color"
    );
    assert_eq!(
        (r[1].fg, r[1].bg),
        (ansi()[1], ansi()[2]),
        "the theme's ANSI colors"
    );
    assert_eq!(r[2].fg, Rgb(0xff, 0, 0), "the 256-color cube");
    assert_eq!(r[3].fg, ansi()[9], "bright colors are the theme's too");
    assert_eq!(
        (r[4].fg, r[4].bg),
        (FG, BG),
        "default colors are the theme's"
    );
    assert_eq!(
        (r[5].glyph.clone(), r[5].bg),
        (Glyph::Char(' '), BG),
        "blank cells"
    );
    assert_eq!((f.foreground, f.background), (FG, BG));
    assert_eq!(f.changed.len(), 10);
    assert!(f.changed.iter().all(|r| r.1.len() == 40));
}

#[test]
fn attributes() {
    let mut t = term();
    feed(
        &mut t,
        "\x1b[1mb\x1b[0m\x1b[3mi\x1b[0m\x1b[9ms\x1b[0m\x1b[2mf\x1b[0m\x1b[7mr\x1b[0m\x1b[8mh\x1b[0m\x1b[5mk\x1b[0m\
         \x1b[38;2;200;0;0;48;2;0;0;100;7mR\x1b[0m\x1b[38;2;250;250;250;2mF\x1b[0m",
    );
    let f = screen(&mut t);
    let r = row(&f, 0);
    assert!(r[0].bold && !r[0].italic);
    assert!(r[1].italic && !r[1].bold);
    assert!(r[2].strike);
    assert_eq!(
        r[3].fg,
        Rgb(140, 140, 140),
        "faint: 65 % foreground, 35 % background, as ghostel"
    );
    assert_eq!(
        (r[4].fg, r[4].bg),
        (BG, FG),
        "reverse swaps the default colors"
    );
    assert_eq!(r[5].glyph, Glyph::Char(' '), "invisible text is a space");
    assert_eq!(
        (r[6].glyph.clone(), r[6].fg),
        (Glyph::Char('k'), FG),
        "blink draws steadily"
    );
    assert_eq!(
        (r[7].fg, r[7].bg),
        (Rgb(0, 0, 100), Rgb(200, 0, 0)),
        "reverse swaps set colors"
    );
    assert_eq!(r[8].fg, Rgb(167, 167, 167), "faint on a set color");
}

#[test]
fn underline_styles_and_colors() {
    let mut t = term();
    feed(
        &mut t,
        "\x1b[4ma\x1b[0m\x1b[4:2mb\x1b[0m\x1b[4:3;58;2;255;0;0mc\x1b[0m\x1b[4:4;58;5;1md\x1b[0m\
         \x1b[4:5me\x1b[0m\x1b[21mf\x1b[0m\x1b[4;38;2;9;9;9mg\x1b[0m",
    );
    let f = screen(&mut t);
    let r = row(&f, 0);
    let got: Vec<Underline> = r[..7].iter().map(|c| c.underline).collect();
    use Underline::*;
    assert_eq!(got, [Single, Double, Curly, Dotted, Dashed, Double, Single]);
    assert_eq!(r[2].underline_color, Rgb(255, 0, 0));
    assert_eq!(r[3].underline_color, ansi()[1], "a palette underline color");
    assert_eq!(
        r[6].underline_color,
        Rgb(9, 9, 9),
        "unset, it's the foreground"
    );
    assert_eq!(r[7].underline, None);
}

#[test]
fn wide_characters_and_clusters() {
    let mut t = term();
    feed(&mut t, "a\u{4e16}\u{754c}b e\u{301} \u{1F44D}\u{1F3FD}|");
    let f = screen(&mut t);
    let r = row(&f, 0);
    assert_eq!(r[1].glyph, Glyph::Char('\u{4e16}'));
    assert_eq!(
        r[2].glyph,
        Glyph::Continuation,
        "a wide character's second cell"
    );
    assert_eq!(r[3].glyph, Glyph::Char('\u{754c}'));
    assert_eq!(r[4].glyph, Glyph::Continuation);
    assert_eq!(r[5].glyph, Glyph::Char('b'));
    assert_eq!(
        r[7].glyph,
        Glyph::Cluster("e\u{301}".into()),
        "a combining mark in its cell"
    );
    assert_eq!(
        r[9].glyph,
        Glyph::Cluster("\u{1F44D}\u{1F3FD}".into()),
        "mode 2027: one cluster"
    );
    assert_eq!(r[10].glyph, Glyph::Continuation, "and it's wide");
    assert_eq!(r[11].glyph, Glyph::Char('|'));
    // A wide character that doesn't fit at the end of a row wraps whole.
    let mut t = term();
    feed(&mut t, &format!("{}\u{4e16}", "x".repeat(39)));
    let f = screen(&mut t);
    assert_eq!(
        row(&f, 0)[39].glyph,
        Glyph::Char(' '),
        "the spacer at the row's end"
    );
    assert_eq!(row(&f, 1)[0].glyph, Glyph::Char('\u{4e16}'));
}

#[test]
fn scroll_regions() {
    let mut t = term();
    for i in 0..10 {
        feed(&mut t, &format!("\x1b[{};1Hline {i}", i + 1));
    }
    // Rows 3 to 6 scroll up by two; the rest stay.
    feed(&mut t, "\x1b[3;6r\x1b[6;1H\n\n\x1b[r");
    let f = screen(&mut t);
    let rows: Vec<String> = (0..10).map(|y| text(row(&f, y))).collect();
    assert_eq!(
        rows,
        [
            "line 0", "line 1", "line 4", "line 5", "", "", "line 6", "line 7", "line 8", "line 9"
        ]
    );
    // Reverse index at the region's top scrolls it down.
    feed(&mut t, "\x1b[2;4r\x1b[2;1H\x1bM\x1b[r");
    let f = screen(&mut t);
    assert_eq!(text(row(&f, 1)), "");
    assert_eq!(text(row(&f, 2)), "line 1");
    assert_eq!(text(row(&f, 4)), "", "the region's last row went");
    assert_eq!(
        t.viewport().history,
        0,
        "a partial region pushes nothing into history"
    );
}

#[test]
fn alternate_screen() {
    let mut t = term();
    feed(&mut t, "primary\r\nshell $ ");
    assert!(!t.alt_screen() && !t.modes().alt_screen);
    feed(&mut t, "\x1b[?1049h\x1b[2J\x1b[Hvim here");
    assert!(
        t.alt_screen() && t.modes().alt_screen,
        "the surface's Esc goes to the program"
    );
    let f = screen(&mut t);
    assert_eq!(text(row(&f, 0)), "vim here");
    assert_eq!(text(row(&f, 1)), "");
    feed(&mut t, "\x1b[?1049l");
    assert!(!t.alt_screen());
    let f = screen(&mut t);
    assert_eq!(text(row(&f, 0)), "primary");
    assert_eq!(text(row(&f, 1)), "shell $");
    assert_eq!(f.cursor.at, Some((1, 8)), "the cursor comes back too");
}

#[test]
fn cursor_position_shape_blink_and_visibility() {
    let mut t = term();
    feed(&mut t, "\x1b[3;5H");
    let c = screen(&mut t).cursor;
    assert_eq!(c.at, Some((2, 4)));
    assert!(c.visible);
    let shapes = [
        ("\x1b[1 q", CursorShape::Block, true),
        ("\x1b[2 q", CursorShape::Block, false),
        ("\x1b[3 q", CursorShape::Underline, true),
        ("\x1b[4 q", CursorShape::Underline, false),
        ("\x1b[5 q", CursorShape::Bar, true),
        ("\x1b[6 q", CursorShape::Bar, false),
    ];
    for (seq, shape, blink) in shapes {
        feed(&mut t, seq);
        let c = screen(&mut t).cursor;
        assert_eq!((c.shape, c.blink), (shape, blink), "{seq:?}");
    }
    feed(&mut t, "\x1b[?25l");
    assert!(!screen(&mut t).cursor.visible, "DECTCEM hides it");
    feed(&mut t, "\x1b[?25h\x1b]12;#00ff00\x07");
    let c = screen(&mut t).cursor;
    assert!(c.visible);
    assert_eq!(c.color, Some(Rgb(0, 255, 0)), "OSC 12");
    // On a wide character, the cursor is on its first cell.
    feed(&mut t, "\x1b[5;1H\u{4e16}\x1b[5;2H");
    assert_eq!(screen(&mut t).cursor.at, Some((4, 0)));
}

#[test]
fn frames_carry_what_changed() {
    let mut t = term();
    feed(&mut t, "hello");
    let first = t.frame(false).unwrap();
    assert!(
        first.full && first.changed.len() == 10,
        "the first frame is full"
    );
    assert_eq!(t.frame(false), None, "nothing changed");
    feed(&mut t, "\x1b[6;1Hzz");
    let f = t.frame(false).unwrap();
    let ys: Vec<u16> = f.changed.iter().map(|r| r.0).collect();
    assert!(!f.full && ys.contains(&5) && ys.len() <= 3, "{ys:?}");
    assert_eq!(text(row(&f, 5)), "zz");
    // A moved cursor alone is still a frame.
    feed(&mut t, "\x1b[1;1H");
    assert_eq!(t.frame(false).map(|f| f.cursor.at), Some(Some((0, 0))));
    // New colors redraw everything.
    t.set_colors(Rgb(1, 1, 1), Rgb(2, 2, 2), None, &ansi());
    let f = t.frame(false).unwrap();
    assert!(f.full);
    assert_eq!(row(&f, 0)[0].fg, Rgb(1, 1, 1));
}

#[test]
fn a_scrolled_back_viewport() {
    let mut t = term();
    for i in 0..30 {
        feed(&mut t, &format!("line {i}\r\n"));
    }
    assert_eq!(text(row(&screen(&mut t), 0)), "line 21");
    t.scroll(-5);
    let f = t.frame(false).unwrap();
    assert_eq!(text(row(&f, 0)), "line 16");
    assert_eq!(f.cursor.at, None, "the cursor is below the view");
    assert!(!t.viewport().at_bottom);
    t.scroll_to_row(0);
    assert_eq!(text(row(&screen(&mut t), 0)), "line 0");
    assert_eq!(t.viewport().top, 0);
    t.scroll_to_bottom();
    assert!(t.viewport().at_bottom);
    assert_eq!(text(row(&screen(&mut t), 0)), "line 21");
}

#[test]
fn resize_reflows_and_redraws() {
    let mut t = term();
    feed(&mut t, &format!("{}\r\n$ ", "0123456789".repeat(6)));
    t.frame(false);
    t.resize(30, 10, (20, 40));
    let f = t.frame(false).unwrap();
    assert!(f.full && f.cols == 30 && f.changed[0].1.len() == 30);
    assert_eq!(text(row(&f, 0)), "012345678901234567890123456789");
    assert_eq!(text(row(&f, 1)), "012345678901234567890123456789");
    assert_eq!(text(row(&f, 2)), "$");
    assert_eq!(t.size(), (30, 10));
    assert_eq!(t.cell_px(), (20, 40));
    feed(&mut t, "\x1b[14t");
    assert_eq!(
        t.take_replies(),
        b"\x1b[4;400;600t",
        "XTWINOPS follows, in physical pixels"
    );
}

#[test]
fn synchronized_output_holds_the_frame() {
    let mut t = term();
    t.frame(false);
    feed(&mut t, "\x1b[?2026hhalf a fr");
    assert_eq!(t.frame(false), None, "held while the program draws");
    feed(&mut t, "ame\x1b[?2026l");
    let f = t.frame(false).unwrap();
    assert_eq!(text(row(&f, 0)), "half a frame", "and shown whole");
    feed(&mut t, "\x1b[?2026hmore");
    assert!(t.frame(true).is_some(), "a full frame is never held");
    assert_eq!(t.frame(false), None, "and the program's hold goes on");
}
