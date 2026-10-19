//! K2: regex search over the text (D4: Rust's regex syntax). What die-vim's `/ ? n N * #`
//! and search highlighting need: the next or previous match, wrapping, and every match in a range.

use die_core::*;

fn buf(s: &str) -> TextBuf {
    TextBuf::new(s)
}

fn pat(p: &str) -> Pattern {
    Pattern::new(p, false).unwrap()
}

fn find(t: &TextBuf, p: &str, from: usize, dir: Dir) -> Option<(std::ops::Range<usize>, bool)> {
    t.search(&pat(p), from, dir).map(|f| (f.range, f.wrapped))
}

#[test]
fn forward_finds_the_next_match_after_from() {
    let t = buf("foo bar foo baz foo");
    assert_eq!(find(&t, "foo", 0, Dir::Forward), Some((8..11, false)));
    assert_eq!(find(&t, "foo", 7, Dir::Forward), Some((8..11, false)));
    assert_eq!(find(&t, "foo", 8, Dir::Forward), Some((16..19, false)));
    assert_eq!(find(&t, "ba.", 0, Dir::Forward), Some((4..7, false)));
}

#[test]
fn forward_wraps_to_the_first_match() {
    let t = buf("foo bar foo");
    assert_eq!(find(&t, "foo", 8, Dir::Forward), Some((0..3, true)));
    assert_eq!(find(&t, "foo", 11, Dir::Forward), Some((0..3, true)));
    // The only match is under the cursor: found again, after wrapping.
    let t = buf("xx foo xx");
    assert_eq!(find(&t, "foo", 3, Dir::Forward), Some((3..6, true)));
}

#[test]
fn backward_finds_the_previous_match_and_wraps() {
    let t = buf("foo bar foo baz foo");
    assert_eq!(find(&t, "foo", 16, Dir::Backward), Some((8..11, false)));
    assert_eq!(find(&t, "foo", 9, Dir::Backward), Some((8..11, false)));
    assert_eq!(find(&t, "foo", 8, Dir::Backward), Some((0..3, false)));
    assert_eq!(find(&t, "foo", 0, Dir::Backward), Some((16..19, true)));
    // Every start position counts, not only non-overlapping matches.
    let t = buf("aaa");
    assert_eq!(find(&t, "aa", 3, Dir::Backward), Some((1..3, false)));
}

#[test]
fn backward_crosses_lines() {
    let t = buf("foo\nbar\nbaz\n");
    assert_eq!(find(&t, "foo", 9, Dir::Backward), Some((0..3, false)));
    assert_eq!(find(&t, "ba", 8, Dir::Backward), Some((4..6, false)));
}

#[test]
fn no_match_and_empty_buffers() {
    assert_eq!(find(&buf("abc"), "x", 0, Dir::Forward), None);
    assert_eq!(find(&buf("abc"), "x", 2, Dir::Backward), None);
    assert_eq!(find(&buf(""), "x", 0, Dir::Forward), None);
    assert!(buf("").matches(&pat("x"), 0..0).is_empty());
}

#[test]
fn line_anchors_match_at_every_line() {
    let t = buf("ab\nbc\nb\n");
    assert_eq!(find(&t, "^b", 0, Dir::Forward), Some((3..4, false)));
    assert_eq!(find(&t, "c$", 0, Dir::Forward), Some((4..5, false)));
    // An empty match moves on: `^` from a line start goes to the next line start.
    assert_eq!(find(&t, "^", 0, Dir::Forward), Some((3..3, false)));
    assert_eq!(find(&t, "^", 3, Dir::Forward), Some((6..6, false)));
    // A pattern can span lines.
    assert_eq!(find(&t, "b\nb", 0, Dir::Forward), Some((1..4, false)));
}

#[test]
fn ignore_case_is_the_callers_choice() {
    let t = buf("Foo FOO foo");
    let ic = Pattern::new("foo", true).unwrap();
    assert_eq!(t.search(&ic, 0, Dir::Forward).map(|f| f.range), Some(4..7));
    assert_eq!(find(&t, "foo", 0, Dir::Forward), Some((8..11, false)));
    assert_eq!(ic.as_str(), "foo");
}

#[test]
fn matches_cover_a_range() {
    let t = buf("ab ab\nab ab ab\nab\n");
    // Every match overlapping 4..12, including the one that starts before it on its line.
    assert_eq!(t.matches(&pat("ab"), 4..12), vec![3..5, 6..8, 9..11]);
    assert_eq!(
        t.matches(&pat("ab"), 4..13),
        vec![3..5, 6..8, 9..11, 12..14]
    );
    assert_eq!(t.matches(&pat("ab"), 0..t.len()).len(), 6);
    assert_eq!(
        t.matches(&pat("b a"), 0..t.len()),
        vec![1..4, 7..10, 10..13]
    );
}

#[test]
fn search_sees_hidden_text() {
    let mut t = buf("one two three");
    t.decorate(
        4..8,
        Deco::Hide {
            replacement: None,
            reveal: Reveal::Remove,
        },
    )
    .unwrap();
    assert_eq!(find(&t, "two", 0, Dir::Forward), Some((4..7, false)));
}

#[test]
fn bad_patterns_are_errors() {
    assert!(matches!(
        Pattern::new("(", false),
        Err(Error::BadPattern(_))
    ));
    assert!(matches!(
        Pattern::new("a{2,1}", false),
        Err(Error::BadPattern(_))
    ));
}

#[test]
fn search_follows_edits() {
    let mut t = buf("abc");
    assert_eq!(find(&t, "x", 0, Dir::Forward), None);
    t.tx(Origin::User).insert(1, "x").commit().unwrap();
    assert_eq!(find(&t, "x", 0, Dir::Forward), Some((1..2, false)));
    t.undo().unwrap();
    assert_eq!(find(&t, "x", 0, Dir::Forward), None);
}

#[test]
fn offsets_are_bytes_on_char_boundaries() {
    let t = buf("é中x中");
    assert_eq!(find(&t, "中", 0, Dir::Forward), Some((2..5, false)));
    assert_eq!(find(&t, "中", 2, Dir::Forward), Some((6..9, false)));
    assert_eq!(find(&t, "中", 6, Dir::Backward), Some((2..5, false)));
    // `from` inside a character rounds the way the search goes.
    assert_eq!(find(&t, "中", 3, Dir::Forward), Some((6..9, false)));
    assert_eq!(find(&t, ".", 4, Dir::Backward), Some((2..5, false)));
}
