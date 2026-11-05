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
    assert_eq!(find(&t, "foo", 11, Dir::Backward), Some((8..11, false)));
    // The match at 8 runs past 9, so it doesn't count.
    assert_eq!(find(&t, "foo", 9, Dir::Backward), Some((0..3, false)));
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
fn matches_see_only_the_lines_the_range_touches() {
    let none = Vec::<std::ops::Range<usize>>::new();
    let t = buf("ab\ncd\n");
    // A match must end by the last line's newline, as Emacs's lazy highlighting stops at
    // the window's end.
    assert_eq!(t.matches(&pat(r"b\nc"), 1..2), none);
    assert_eq!(t.matches(&pat(r"b\nc"), 1..4), vec![1..4]);
    assert_eq!(t.matches(&pat(r"b\n"), 1..2), vec![1..3]);
    // Look-around at the end still sees the next line: `$` after a newline needs an empty line.
    assert_eq!(t.matches(&pat(r"b\n$"), 1..2), none);
    assert_eq!(buf("ab\n\n").matches(&pat(r"b\n$"), 1..2), vec![1..3]);
}

#[test]
fn escape_makes_a_literal_pattern() {
    // `*` on `a.b`: the word, escaped, between word boundaries.
    let t = buf("axb a.b a.bc");
    let p = Pattern::new(&format!(r"\b{}\b", Pattern::escape("a.b")), false).unwrap();
    assert_eq!(t.search(&p, 0, Dir::Forward).map(|f| f.range), Some(4..7));
    assert_eq!(t.matches(&p, 0..t.len()), vec![4..7]);
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
    assert_eq!(find(&t, ".", 4, Dir::Backward), Some((0..2, false)));
}

#[test]
fn backward_takes_only_matches_that_end_by_from() {
    // Emacs's `re-search-backward`, under Evil's `? N #` (D4): a match must end at or
    // before `from`, and `from` bounds the match itself. Each case was checked in the oracle.
    // `?ab` on `aba|b`: the match at 2 runs past the cursor.
    assert_eq!(
        find(&buf("abab"), "ab", 3, Dir::Backward),
        Some((0..2, false))
    );
    // `a+` stops at `from`.
    assert_eq!(
        find(&buf("aaa"), "a+", 2, Dir::Backward),
        Some((1..2, false))
    );
    // `#` on `foo foobar f|oo`: the word under the cursor doesn't count.
    let t = buf("foo foobar foo");
    assert_eq!(find(&t, r"\bfoo\b", 12, Dir::Backward), Some((0..3, false)));
    // `/ab` then `0fbN` on `xx abab`: nothing ends by the cursor, so it wraps to the last match.
    assert_eq!(
        find(&buf("xx abab"), "ab", 4, Dir::Backward),
        Some((5..7, true))
    );
    // Look-around still sees past `from`: `foo$` doesn't match `foo|bar`, and `?o$` from
    // `fo|obar` goes to the line above.
    let t = buf("xo\nfoobar\n");
    assert_eq!(find(&t, "o$", 5, Dir::Backward), Some((1..2, false)));
    assert_eq!(find(&buf("foobar"), "foo$", 3, Dir::Backward), None);
    assert_eq!(find(&buf("foobar"), r"foo\b", 3, Dir::Backward), None);
}

#[test]
fn backward_on_a_long_run_is_fast() {
    // Each start position matches to the end of the run: trying them all from the
    // buffer's start is quadratic (seconds here), from `from` it's immediate.
    let t = buf(&"x".repeat(32 << 10));
    let start = std::time::Instant::now();
    assert_eq!(
        find(&t, r"\S+", t.len(), Dir::Backward),
        Some((t.len() - 1..t.len(), false))
    );
    assert!(
        start.elapsed().as_millis() < 100,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn line_ends_match_before_crlf() {
    // CRLF files open read-only (M0), but can be searched.
    let t = buf("a;\r\nb;\r\n");
    assert_eq!(find(&t, ";$", 0, Dir::Forward), Some((1..2, false)));
    assert_eq!(find(&t, ";$", 1, Dir::Forward), Some((5..6, false)));
    assert_eq!(find(&t, ";$", 5, Dir::Forward), Some((1..2, true)));
    assert_eq!(find(&t, "^b", 0, Dir::Forward), Some((4..5, false)));
}

// ---------------------------------------------------------------- `*` and `#` (SEARCH-1)

/// Evil's word characters in fundamental-mode: `_` isn't one.
fn evil_word(c: char) -> bool {
    c.is_alphanumeric()
}

fn word(p: &str) -> Pattern {
    Pattern::word(p, false, evil_word).unwrap()
}

fn find_word(
    t: &TextBuf,
    p: &str,
    from: usize,
    dir: Dir,
) -> Option<(std::ops::Range<usize>, bool)> {
    t.search(&word(p), from, dir).map(|f| (f.range, f.wrapped))
}

/// Your Evil, in fundamental-mode (evil/star.el): `*` from the last foo of `x foo_bar foo`
/// lands at 2, and from 0 in `foo foobar foo_bar` it lands at 11.
#[test]
fn a_word_pattern_matches_as_evils_star_does() {
    let t = buf("x foo_bar foo");
    assert_eq!(find_word(&t, "foo", 10, Dir::Forward), Some((2..5, true)));
    let t = buf("foo foobar foo_bar");
    assert_eq!(find_word(&t, "foo", 0, Dir::Forward), Some((11..14, false)));
}

#[test]
fn a_word_pattern_skips_matches_going_backward_too() {
    let t = buf("foo foobar barfoo foo_bar foo");
    assert_eq!(
        find_word(&t, "foo", 26, Dir::Backward),
        Some((18..21, false))
    );
    assert_eq!(find_word(&t, "foo", 18, Dir::Backward), Some((0..3, false)));
    assert_eq!(find_word(&t, "foo", 0, Dir::Backward), Some((26..29, true)));
    assert_eq!(find_word(&buf("foobar"), "foo", 6, Dir::Backward), None);
    assert_eq!(find_word(&buf("foobar"), "foo", 0, Dir::Forward), None);
}

#[test]
fn a_rejected_match_moves_on_a_character_not_a_byte() {
    // `é` is a word character of two bytes, and the search goes on from just after it.
    let t = buf("éaa aa");
    assert_eq!(find_word(&t, "aa", 0, Dir::Forward), Some((5..7, false)));
    let t = buf("ééé é");
    assert_eq!(find_word(&t, "é", 0, Dir::Forward), Some((7..9, false)));
    assert_eq!(find_word(&t, "é", 9, Dir::Backward), Some((7..9, false)));
}

/// A match that doesn't count can overlap one that does, when `text` isn't all word characters.
#[test]
fn a_rejected_match_doesnt_hide_one_it_overlaps() {
    let t = buf("x---");
    assert_eq!(find_word(&t, "--", 0, Dir::Forward), Some((2..4, false)));
    assert_eq!(t.matches(&word("--"), 0..4), vec![2..4]);
    let t = buf("---x");
    assert_eq!(find_word(&t, "--", 4, Dir::Backward), Some((0..2, false)));
    assert_eq!(t.matches(&word("--"), 0..4), vec![0..2]);
}

#[test]
fn a_word_pattern_is_literal_and_can_ignore_case() {
    let p = Pattern::word("a.c", false, evil_word).unwrap();
    assert_eq!(
        buf("abc a.c").search(&p, 0, Dir::Forward).unwrap().range,
        4..7
    );
    assert_eq!(p.as_str(), "a\\.c");
    let p = Pattern::word("foo", true, evil_word).unwrap();
    assert_eq!(
        buf("x FOO").search(&p, 0, Dir::Forward).unwrap().range,
        2..5
    );
}

/// Highlighting takes the same pattern, so every highlight is a match `n` can land on.
#[test]
fn matches_and_match_at_use_the_same_rule() {
    let t = buf("foo foobar barfoo foo_bar\nfoo\n");
    assert_eq!(t.matches(&word("foo"), 0..t.len()), [0..3, 18..21, 26..29]);
    // Past the range's last line, the next character still counts as a neighbour.
    let t = buf("foo\nfoobar");
    assert_eq!(
        t.matches(&word("foo"), 4..7),
        Vec::<std::ops::Range<usize>>::new()
    );
    assert_eq!(t.matches(&word("foo"), 0..3), vec![0..3]);
    // What a plain pattern finds is unchanged.
    assert_eq!(t.matches(&pat("foo"), 0..t.len()), [0..3, 4..7]);
}

#[test]
fn a_plain_pattern_is_not_filtered() {
    let t = buf("foobar");
    assert_eq!(find(&t, "foo", 6, Dir::Backward), Some((0..3, false)));
    assert_eq!(find(&t, "foo", 6, Dir::Forward), Some((0..3, true)));
}

#[test]
fn an_empty_word_pattern_terminates() {
    let t = buf("ab");
    assert_eq!(t.search(&word(""), 0, Dir::Forward), None);
    assert_eq!(t.search(&word(""), 2, Dir::Backward), None);
    let t = buf("a  b");
    let hit = t.search(&word(""), 0, Dir::Forward).unwrap();
    assert_eq!(hit.range, 2..2);
    assert_eq!(t.matches(&word(""), 0..4), vec![2..2]);
}

/// An empty word pattern stands at each position with no word character beside it. Every
/// small text, from every position, going both ways: a rejected match mustn't skip one.
#[test]
fn an_empty_word_pattern_finds_every_position_it_stands_at() {
    assert_eq!(
        buf("\n\n\n\na").search(&word(""), 5, Dir::Backward),
        Some(Found {
            range: 3..3,
            wrapped: false
        })
    );
    let mut texts = vec![String::new()];
    for _ in 0..4 {
        let longer = texts.iter().flat_map(|t| {
            ['a', '\u{4e2d}', '\n'].map(|c| {
                let mut t = t.clone();
                t.push(c);
                t
            })
        });
        texts = longer.chain(texts.iter().cloned()).collect();
        texts.sort();
        texts.dedup();
    }
    let empty = word("");
    for text in &texts {
        let stands: Vec<usize> = (0..=text.len())
            .filter(|&p| {
                text.is_char_boundary(p)
                    && !text[..p].chars().next_back().is_some_and(evil_word)
                    && !text[p..].chars().next().is_some_and(evil_word)
            })
            .collect();
        let t = buf(text);
        for from in 0..=text.len() {
            let forward = stands.iter().find(|&&p| p > from && from < text.len());
            let found = t.search(&empty, from, Dir::Forward).map(|f| f.range.start);
            let want = forward.or(stands.first()).copied();
            assert_eq!(found, want, "{text:?} forward from {from}");
            let backward = stands.iter().rev().find(|&&p| p < from);
            let found = t.search(&empty, from, Dir::Backward).map(|f| f.range.start);
            let want = backward.or(stands.last()).copied();
            assert_eq!(found, want, "{text:?} backward from {from}");
        }
    }
}
