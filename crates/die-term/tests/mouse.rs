//! The pointer in, bytes out: every tracking mode and encoding, SGR pixels included, the wheel's
//! three ways, and the modes a surface routes the pointer by.

use die_test_cap as _;

use die_core::Mods;
use die_term::{MouseFormat, MouseTracking, Pointer, PointerAction, PointerButton, Term};

const CW: u32 = 18;
const CH: u32 = 36;

fn term() -> Term {
    Term::new(80, 24, (CW, CH)).unwrap()
}

/// A pointer event in cell (col, row), a few pixels in.
fn at(action: PointerAction, button: Option<PointerButton>, col: u32, row: u32) -> Pointer {
    Pointer {
        action,
        button,
        mods: Mods::default(),
        x: (col * CW + 4) as f32,
        y: (row * CH + 7) as f32,
    }
}

fn sent(t: &mut Term, p: Pointer) -> String {
    t.mouse(&p).escape_ascii().to_string()
}

const OFF: &str =
    "\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l";

/// Press at (2, 3), drag to (5, 3), release, move bare to (6, 4), and wheel up, in a mode and
/// format.
fn gestures(t: &mut Term, mode: u16, format: u16) -> [String; 5] {
    let fmt = if format == 0 {
        String::new()
    } else {
        format!("\x1b[?{format}h")
    };
    t.feed(format!("{OFF}\x1b[?{mode}h{fmt}").as_bytes());
    use PointerAction::*;
    let left = Some(PointerButton::Left);
    [
        sent(t, at(Press, left, 2, 3)),
        sent(t, at(Motion, left, 5, 3)),
        sent(t, at(Release, left, 5, 3)),
        sent(t, at(Motion, None, 6, 4)),
        sent(t, at(Press, Some(PointerButton::WheelUp), 2, 3)),
    ]
}

#[test]
fn a_terminal_made_before_its_cells_are_known_still_reports() {
    // A surface may make its Term before it has the font's metrics: a zero cell is one pixel.
    let mut t = Term::new(80, 24, (0, 0)).unwrap();
    assert_eq!(t.cell_px(), (1, 1));
    t.feed(b"\x1b[?1000h\x1b[?1006h");
    let p = Pointer {
        action: PointerAction::Press,
        button: Some(PointerButton::Left),
        mods: Mods::default(),
        x: 2.0,
        y: 3.0,
    };
    assert_eq!(t.mouse(&p), b"\x1b[<0;3;4M");
}

#[test]
fn tracking_modes() {
    let mut t = term();
    assert_eq!(t.modes().mouse, MouseTracking::None);
    assert_eq!(
        sent(
            &mut t,
            at(PointerAction::Press, Some(PointerButton::Left), 1, 1)
        ),
        "",
        "untracked, the pointer is the surface's"
    );
    let x10 = gestures(&mut t, 9, 0);
    assert_eq!(t.modes().mouse, MouseTracking::X10);
    assert_eq!(
        x10,
        ["\\x1b[M #$", "", "", "", ""],
        "presses only, and no wheel"
    );
    let normal = gestures(&mut t, 1000, 1006);
    assert_eq!(t.modes().mouse, MouseTracking::Normal);
    assert_eq!(
        normal,
        ["\\x1b[<0;3;4M", "", "\\x1b[<0;6;4m", "", "\\x1b[<64;3;4M"],
        "presses and releases"
    );
    let button = gestures(&mut t, 1002, 1006);
    assert_eq!(t.modes().mouse, MouseTracking::Button);
    assert_eq!(button[1], "\\x1b[<32;6;4M", "a drag");
    assert_eq!(button[3], "", "but not bare motion");
    let any = gestures(&mut t, 1003, 1006);
    assert_eq!(t.modes().mouse, MouseTracking::Any);
    assert_eq!(any[3], "\\x1b[<35;7;5M", "bare motion too");
    t.feed(OFF.as_bytes());
    assert_eq!(t.modes().mouse, MouseTracking::None);
}

#[test]
fn encodings() {
    let mut t = term();
    let x10 = gestures(&mut t, 1000, 0);
    assert_eq!(t.modes().mouse_format, MouseFormat::X10);
    assert_eq!(
        (x10[0].as_str(), x10[2].as_str()),
        ("\\x1b[M #$", "\\x1b[M#&$")
    );
    let utf8 = gestures(&mut t, 1000, 1005);
    assert_eq!(t.modes().mouse_format, MouseFormat::Utf8);
    assert_eq!(utf8[0], "\\x1b[M #$");
    let urxvt = gestures(&mut t, 1000, 1015);
    assert_eq!(t.modes().mouse_format, MouseFormat::Urxvt);
    assert_eq!(urxvt[0], "\\x1b[32;3;4M");
    let pixels = gestures(&mut t, 1000, 1016);
    assert_eq!(t.modes().mouse_format, MouseFormat::SgrPixels);
    // Pixels from 0, as Ghostty sends them.
    assert_eq!(
        pixels[0],
        format!("\\x1b[<0;{};{}M", 2 * CW + 4, 3 * CH + 7)
    );
    // UTF-8 coordinates past column 95, where X10's byte would overflow: column 151 is 32 + 151,
    // U+00B7.
    let mut wide = Term::new(200, 24, (CW, CH)).unwrap();
    wide.feed(b"\x1b[?1000h\x1b[?1005h");
    let far = wide.mouse(&at(PointerAction::Press, Some(PointerButton::Left), 150, 0));
    assert_eq!(far, "\x1b[M \u{b7}!".as_bytes());
}

#[test]
fn modifiers_and_buttons() {
    let mut t = term();
    t.feed(b"\x1b[?1000h\x1b[?1006h");
    let mut right = at(PointerAction::Press, Some(PointerButton::Right), 2, 3);
    right.mods = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    assert_eq!(
        sent(&mut t, right),
        "\\x1b[<22;3;4M",
        "right is 2, shift 4, ctrl 16"
    );
    let middle = at(PointerAction::Press, Some(PointerButton::Middle), 0, 0);
    assert_eq!(sent(&mut t, middle), "\\x1b[<1;1;1M");
    let down = at(PointerAction::Press, Some(PointerButton::WheelDown), 0, 0);
    assert_eq!(sent(&mut t, down), "\\x1b[<65;1;1M");
    let back = at(PointerAction::Press, Some(PointerButton::Back), 0, 0);
    assert_eq!(sent(&mut t, back), "\\x1b[<128;1;1M");
}

#[test]
fn the_wheel_goes_to_the_program_the_arrows_or_the_scrollback() {
    let mut t = term();
    for i in 0..60 {
        t.feed(format!("line {i}\r\n").as_bytes());
    }
    let none = Mods::default();
    // A shell: the viewport scrolls, and nothing is sent.
    assert_eq!(t.wheel(-3, 10.0, 10.0, none), b"");
    assert!(!t.viewport().at_bottom);
    assert_eq!(t.viewport().top, 34, "{:?}", t.viewport());
    t.wheel(3, 10.0, 10.0, none);
    assert!(t.viewport().at_bottom);
    // less on the alternate screen, which doesn't track the mouse: arrow keys (mode 1007).
    t.feed(b"\x1b[?1049h");
    assert!(t.modes().alt_scroll, "Ghostty has 1007 on by default");
    assert_eq!(t.wheel(-2, 10.0, 10.0, none), b"\x1b[A\x1b[A");
    t.feed(b"\x1b[?1h");
    assert_eq!(
        t.wheel(1, 10.0, 10.0, none),
        b"\x1bOB",
        "in the cursor keys' mode"
    );
    // htop tracks the mouse: wheel buttons.
    t.feed(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(t.wheel(-2, 4.0, 7.0, none), b"\x1b[<64;1;1M\x1b[<64;1;1M");
    assert_eq!(t.wheel(1, 4.0, 7.0, none), b"\x1b[<65;1;1M");
}
