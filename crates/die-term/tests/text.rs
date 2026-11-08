//! Copy state's text: lines with their rows, columns that count wide characters as drawn, the
//! cursor's place, marks that follow new output, and showing the viewport at any row.

use die_test_cap as _;

use die_term::{Term, Text};

fn term(cols: u16, rows: u16) -> Term {
    Term::new(cols, rows, (18, 36)).unwrap()
}

fn feed(t: &mut Term, s: &str) {
    let mut b = s.as_bytes();
    while !b.is_empty() {
        b = &b[t.feed(b)..];
    }
}

fn texts(text: &Text) -> Vec<&str> {
    text.lines.iter().map(|l| l.text.as_str()).collect()
}

#[test]
fn the_scrollback_and_screen_as_lines() {
    let mut t = term(20, 4);
    feed(&mut t, "one\r\ntwo  \r\n\x1b[1mthree\x1b[0m\r\nfour\r\n$ ");
    let text = t.text();
    // "one" is in the scrollback, the rest on the screen. Trailing blanks go, but the cursor's
    // row keeps them through the cursor's cell, so the cursor has a character to be on.
    assert_eq!(texts(&text), ["one", "two", "three", "four", "$  "]);
    let rows: Vec<u64> = text.lines.iter().map(|l| l.row).collect();
    assert_eq!(rows, [0, 1, 2, 3, 4], "absolute rows");
    assert_eq!(text.cursor, Some((4, 2)));
    assert!(!text.alt_screen);
}

#[test]
fn columns_count_wide_characters_as_drawn() {
    let mut t = term(20, 3);
    feed(&mut t, "a\u{4e16}b e\u{301}x\u{1F44D}\u{1F3FD}z\r\n");
    let text = t.text();
    let line = &text.lines[0];
    assert_eq!(line.text, "a\u{4e16}b e\u{301}x\u{1F44D}\u{1F3FD}z");
    // Byte offsets and the columns they're drawn in.
    let at = |s: &str| line.text.find(s).unwrap();
    let cols: Vec<u16> = ["a", "\u{4e16}", "b", " ", "e", "x", "\u{1F44D}", "z"]
        .iter()
        .map(|s| line.col(at(s)))
        .collect();
    assert_eq!(cols, [0, 1, 3, 4, 5, 6, 7, 9]);
    assert_eq!(
        line.col(at("\u{301}")),
        5,
        "a combining mark is in its base's cell"
    );
    assert_eq!(line.at(1), at("\u{4e16}"));
    assert_eq!(
        line.at(2),
        at("\u{4e16}"),
        "a wide character's second column"
    );
    assert_eq!(line.at(3), at("b"));
    assert_eq!(line.at(8), at("\u{1F44D}"));
    assert_eq!(line.at(9), at("z"));
    assert_eq!((line.width(), line.col(line.text.len())), (10, 10));
    assert_eq!(line.at(15), line.text.len(), "past the end");
    // Round trips for every character.
    for (i, _) in line
        .text
        .char_indices()
        .filter(|(i, _)| *i != at("\u{301}"))
    {
        let i = if i == at("\u{1F3FD}") {
            at("\u{1F44D}")
        } else {
            i
        };
        assert_eq!(line.at(line.col(i)), i);
    }
}

#[test]
fn soft_wraps_are_marked() {
    let mut t = term(10, 4);
    feed(&mut t, "0123456789abcde\r\nnext\r\n");
    let text = t.text();
    assert_eq!(texts(&text)[..3], ["0123456789", "abcde", "next"]);
    let wrapped: Vec<bool> = text.lines.iter().map(|l| l.wrapped).collect();
    assert_eq!(wrapped[..3], [true, false, false]);
}

#[test]
fn marks_follow_new_output() {
    let mut t = term(20, 4);
    for i in 0..10 {
        feed(&mut t, &format!("line {i}\r\n"));
    }
    let text = t.text();
    let last_history = text.lines.len() - 4;
    let mark = text.mark;
    assert_eq!(t.added_since(mark), Some(0));
    feed(&mut t, "new 1\r\nnew 2\r\nnew 3\r\n");
    assert_eq!(
        t.added_since(mark),
        Some(3),
        "three lines went into history"
    );
    // Read only from the first row that was on the screen.
    let first = text.lines[last_history].row;
    let more = t.text_from(first);
    assert_eq!(more.lines[0].row, first);
    assert_eq!(
        texts(&more),
        ["line 7", "line 8", "line 9", "new 1", "new 2", "new 3", " "]
    );
    assert_eq!(t.added_since(more.mark), Some(0));
}

#[test]
fn rows_keep_their_numbers_as_the_oldest_go() {
    let mut t = term(40, 5);
    for i in 0..30_000 {
        feed(&mut t, &format!("row {i}\r\n"));
    }
    let text = t.text();
    let first = &text.lines[0];
    let n: u64 = first.text.strip_prefix("row ").unwrap().parse().unwrap();
    assert_eq!(first.row, n, "a row's number is its line's");
    // The engine drops whole pages, so it keeps a little under 10,000 lines.
    assert!(
        (8_000..=10_005).contains(&text.lines.len()),
        "{}",
        text.lines.len()
    );
    let last = &text.lines[text.lines.len() - 2];
    assert_eq!((last.text.as_str(), last.row), ("row 29999", 29_999));
    // Showing the viewport at a row, by its number.
    t.scroll_to_row(n + 100);
    assert_eq!(t.viewport().top, n + 100);
    let f = t.frame(true).unwrap();
    let shown: String = f.changed[0].1.iter().map(|c| c.glyph.to_string()).collect();
    assert_eq!(shown.trim_end(), format!("row {}", n + 100));
}

#[test]
fn rows_keep_their_numbers_through_a_flood_in_one_read() {
    // `yes` or `seq` through DIE's PTY: a read of 64 KiB is 21,845 lines, more than the
    // scrollback holds, and it comes to `feed` whole.
    let mut t = term(80, 24);
    for i in 0..100 {
        feed(&mut t, &format!("row {i}\r\n"));
    }
    let mark = t.mark();
    let yes = "y\r\n".repeat(21_845);
    for _ in 0..4 {
        feed(&mut t, &yes);
    }
    feed(&mut t, &"\n".repeat(64 << 10));
    feed(&mut t, "\rlast");
    let added = 100 + 4 * 21_845 + (64 << 10);
    assert_eq!(t.added_since(mark), Some(added - 100), "every line counted");
    let text = t.text();
    let last = text.lines.last().unwrap();
    assert_eq!(
        (last.text.trim_end(), last.row),
        ("last", added),
        "and numbered"
    );
    let top = t.viewport().top;
    assert_eq!(top, added - 23, "the screen's top, by the same count");
}

#[test]
fn numbers_go_on_past_output_that_outruns_the_count() {
    // REP makes 819 lines of 9 bytes: 100 of them push out the whole scrollback between two
    // counts. Which rows went can't be told, so the count starts again, and the rows are
    // numbered on from past every number given so far: an old number names no line.
    let mut t = term(80, 24);
    for i in 0..50 {
        feed(&mut t, &format!("row {i}\r\n"));
    }
    let before = t.text();
    let given = before.lines.last().unwrap().row;
    feed(&mut t, &"a\x1b[65535b".repeat(100));
    feed(&mut t, "\r\nafter");
    assert_eq!(t.added_since(before.mark), None, "read it all again");
    let text = t.text();
    assert!(text.lines[0].row > given, "{} > {given}", text.lines[0].row);
    let rows: Vec<u64> = text.lines.iter().map(|l| l.row).collect();
    assert!(rows.windows(2).all(|w| w[1] == w[0] + 1), "in order");
    assert_eq!(t.viewport().top, rows[rows.len() - 24]);
    // After that, the count follows output again.
    let mark = text.mark;
    feed(&mut t, "\r\none\r\ntwo");
    assert_eq!(t.added_since(mark), Some(2));
    assert_eq!(t.text().lines.last().unwrap().row, rows[rows.len() - 1] + 2);
}

#[test]
fn a_reverse_index_at_the_top_keeps_the_history_numbered() {
    let mut t = term(20, 4);
    for i in 0..10 {
        feed(&mut t, &format!("row {i}\r\n"));
    }
    let before = t.text();
    // The screen scrolls down: the anchor's row moves, the history doesn't.
    feed(&mut t, "\x1b[H\x1bM");
    let after = t.text();
    let history = |t: &Text| t.lines[..t.lines.len() - 4].to_vec();
    assert_eq!(history(&after), history(&before));
    assert_eq!(after.lines[7].row, 7);
}

#[test]
fn marks_lapse_when_the_rows_change_under_them() {
    let mut t = term(20, 4);
    feed(&mut t, "a\r\nb\r\nc\r\nd\r\ne\r\n");
    let mark = t.mark();
    t.resize(10, 4, (18, 36));
    assert_eq!(t.added_since(mark), None, "a resize reflows");
    let mark = t.mark();
    feed(&mut t, "\x1b[?1049h\x1b[Hvim");
    assert_eq!(t.added_since(mark), None, "another screen");
    let alt = t.text();
    assert!(alt.alt_screen);
    assert_eq!(
        texts(&alt),
        ["vim ", "", "", ""],
        "the alternate screen has no history"
    );
    feed(&mut t, "\x1b[?1049l");
    let back = t.text();
    assert!(!back.alt_screen);
    assert_eq!(texts(&back)[..5], ["a", "b", "c", "d", "e"]);
}

#[test]
fn reading_the_text_leaves_the_frames_whole() {
    let mut t = term(20, 4);
    feed(&mut t, "before");
    t.frame(false);
    feed(&mut t, "\r\nchanged after the frame");
    let _ = t.text();
    let f = t.frame(false).unwrap();
    assert!(
        f.full,
        "the text's render state took the dirty rows: the frame redraws all"
    );
    let rows: Vec<String> = f
        .changed
        .iter()
        .map(|r| r.1.iter().map(|c| c.glyph.to_string()).collect::<String>())
        .collect();
    assert_eq!(rows[1], "changed after the fr");
    assert_eq!(rows[2].trim_end(), "ame");
}
