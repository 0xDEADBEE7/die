//! A flood of output is parsed a bounded piece at a time, so a terminal inside DIE never holds
//! the main thread for long, and nothing is lost between pieces.

use die_test_cap as _;

use die_term::{FEED_MAX, Term};
use std::time::{Duration, Instant};

/// 16 MB of a colored build log, with sequences that the pieces' edges cut through.
fn flood() -> Vec<u8> {
    let mut out = Vec::with_capacity(16 << 20);
    let mut i = 0u64;
    while out.len() < 16 << 20 {
        out.extend_from_slice(
            format!(
                "\x1b[3{}m   Compiling crate-{i} v0.{}.0 \x1b[1m(\u{4e16}\u{754c})\x1b[0m e\u{301}\r\n",
                i % 8,
                i % 97
            )
            .as_bytes(),
        );
        i += 1;
    }
    out.extend_from_slice(b"\x1b[32mdone\x1b[0m");
    out
}

#[test]
fn each_feed_takes_at_most_its_bound() {
    let mut t = Term::new(120, 40, (18, 36)).unwrap();
    let all = flood();
    let mut rest = &all[..];
    let (mut calls, mut slowest) = (0, Duration::ZERO);
    while !rest.is_empty() {
        let start = Instant::now();
        let n = t.feed(rest);
        slowest = slowest.max(start.elapsed());
        assert!(n > 0 && n <= FEED_MAX, "{n}");
        assert!(
            n == FEED_MAX || n == rest.len(),
            "a whole piece unless it's the last"
        );
        rest = &rest[n..];
        calls += 1;
        // A frame now and then, as vsync takes one.
        if calls % 16 == 0 {
            t.frame(false);
        }
    }
    assert_eq!(calls, all.len().div_ceil(FEED_MAX));
    // A piece is about 0.2 ms here; the limit is loose for a loaded machine, and only says a
    // piece's work is bounded, not that it's fast.
    assert!(slowest < Duration::from_millis(250), "{slowest:?}");
    // Nothing was lost at the pieces' edges: the last lines are whole, in color and wide.
    let text = t.text();
    let n = text.lines.len();
    assert_eq!(text.lines[n - 1].text.trim_end(), "done");
    let before = &text.lines[n - 2].text;
    assert!(
        before.starts_with("   Compiling crate-")
            && before.ends_with("(\u{4e16}\u{754c}) e\u{301}")
    );
    let f = t.frame(true).unwrap();
    let last = &f.changed[39].1;
    assert_eq!(last[0].glyph.to_string(), "d");
    assert_ne!(last[0].fg, f.foreground, "in its color");
    assert!(t.take_replies().is_empty());
}

#[test]
fn a_sequence_split_between_feeds_continues() {
    let mut t = Term::new(40, 5, (18, 36)).unwrap();
    for piece in [
        &b"\x1b[38;2;1"[..],
        b"0;20;30mX\x1b]2;ti",
        b"tle\x07\xe4\xb8",
        b"\x96",
    ] {
        assert_eq!(t.feed(piece), piece.len());
    }
    let f = t.frame(true).unwrap();
    let row = &f.changed[0].1;
    assert_eq!(
        (row[0].glyph.to_string(), row[0].fg),
        ("X".into(), die_core::Rgb(10, 20, 30))
    );
    assert_eq!(row[1].glyph.to_string(), "\u{4e16}");
    assert_eq!(t.title(), "title");
}
