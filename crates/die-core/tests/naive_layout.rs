//! The naive layout (`support/naive_layout.rs`) against contract §9: first every worked example,
//! then each rule the examples don't reach.

// This reaches tests/support/naive_layout.rs without tests/support/mod.rs, whose fixtures use
// die-core's API. The layout shares nothing with die-core, and neither does this file. The lint
// bans the `path` attribute, so a test that has `mod support;` already declares it in mod.rs.
mod support {
    pub mod naive_layout;
}

use support::naive_layout::*;

#[track_caller]
fn check_rows(input: Input, rows: &str) {
    assert_eq!(layout(&input).render(), rows, "{input:?}");
}

/// The rows, and every cursor position as (byte, row, column).
#[track_caller]
fn check(input: Input, rows: &str, positions: &[(usize, usize, usize)]) {
    let l = layout(&input);
    assert_eq!(l.render(), rows, "{input:?}");
    let got: Vec<_> = l.positions.iter().map(|p| (p.pos, p.row, p.col)).collect();
    assert_eq!(got, positions, "{input:?}");
}

#[track_caller]
fn check_numbers(input: Input, numbers: &[Option<usize>]) {
    assert_eq!(layout(&input).numbers(), numbers, "{input:?}");
}

/// A cluster wider than any row used below: spacing marks extend a cluster and take a cell each.
/// `oversize_cluster_is_five_cells` pins that.
const WIDE: &str = "a\u{903}\u{903}\u{903}\u{903}";

// ---------------------------------------------------------------- worked examples: rows

struct RowExample {
    /// The Text column, as the contract writes it.
    doc: &'static str,
    text: &'static str,
    hide: Option<std::ops::Range<usize>>,
    virtual_text: Option<(usize, &'static str)>,
    wrap: Wrap,
    /// The Rows column.
    rows: &'static str,
}

const fn row(doc: &'static str, text: &'static str, rows: &'static str) -> RowExample {
    RowExample {
        doc,
        text,
        hide: None,
        virtual_text: None,
        wrap: Wrap::Word,
        rows,
    }
}

/// "Examples, at 10 cells with `tab_width` 8", in the contract's order.
const ROW_EXAMPLES: &[RowExample] = &[
    row("aaaa bbbb cccc", "aaaa bbbb cccc", "aaaa·bbbb·|cccc"),
    row("aaaa bbbbb cccc", "aaaa bbbbb cccc", "aaaa·|bbbbb·cccc"),
    row("aaaa bbbb  cccc", "aaaa bbbb  cccc", "aaaa·|bbbb··cccc"),
    row(
        "aaaa + 8 spaces + bbbb",
        "aaaa        bbbb",
        "aaaa······|··bbbb",
    ),
    row("aaaaaaaaaa bbbb", "aaaaaaaaaa bbbb", "aaaaaaaaaa|·bbbb"),
    row("aa bbbbbbbbbbbb", "aa bbbbbbbbbbbb", "aa·|bbbbbbbbbb|bb"),
    row("    bbbbbbbbbbbb", "    bbbbbbbbbbbb", "····|bbbbbbbbbb|bb"),
    row(" bbbbbbbbbbbb", " bbbbbbbbbbbb", "·|bbbbbbbbbb|bb"),
    row(
        "aaaaaaaaaa bbbbbbbbbbbb",
        "aaaaaaaaaa bbbbbbbbbbbb",
        "aaaaaaaaaa|·|bbbbbbbbbb|bb",
    ),
    row(
        "aaaaaaaaaaaaaa bb ccc",
        "aaaaaaaaaaaaaa bb ccc",
        "aaaaaaaaaa|aaaa·bb·|ccc",
    ),
    row("aaaa bbbb cc→dd", "aaaa bbbb cc\tdd", "aaaa·bbbb·|cc→→→→dd"),
    row("aaaaaaaaa→bb", "aaaaaaaaa\tbb", "aaaaaaaaa→|→→→→→→bb"),
    row("aa bbbbbb→cc", "aa bbbbbb\tcc", "aa·|bbbbbb→→→→|→→→cc"),
    row(
        "aaaaaaaaa→bbbbb",
        "aaaaaaaaa\tbbbbb",
        "aaaaaaaaa→|→→→→→→|bbbbb",
    ),
    row(
        "aaaaaaaaaa→bbbbb",
        "aaaaaaaaaa\tbbbbb",
        "aaaaaaaaaa|→→→→→→|bbbbb",
    ),
    row("aaaaaaaaaa→b", "aaaaaaaaaa\tb", "aaaaaaaaaa|→→→→→→b"),
    row("aaaaaaaaa中b", "aaaaaaaaa中b", "aaaaaaaaa_|中b"),
    row("aaaa bbbb中c", "aaaa bbbb中c", "aaaa·|bbbb中c"),
    row("aaaaaaaa 中b", "aaaaaaaa 中b", "aaaaaaaa·|中b"),
    row("aaaaaaaaa中→b", "aaaaaaaaa中\tb", "aaaaaaaaa_|中→→→→→b"),
    row("aaaaaaaaa^Abb", "aaaaaaaaa\u{1}bb", "aaaaaaaaa^|Abb"),
    row(
        "aaaaaaaaa^A + 11 b",
        "aaaaaaaaa\u{1}bbbbbbbbbbb",
        "aaaaaaaaa^|Abbbbbbbbb|bb",
    ),
    row("aaaaaaaa ^Ab", "aaaaaaaa \u{1}b", "aaaaaaaa·|^Ab"),
    RowExample {
        hide: Some(7..8),
        ..row("aaaaaaa[ ]bbbb", "aaaaaaa bbbb", "aaaaaaabbb|b")
    },
    RowExample {
        virtual_text: Some((8, "xxxx")),
        ..row("aaaaaaaa b, with xxxx", "aaaaaaaa b", "aaaaaaaaxx|xx·b")
    },
    RowExample {
        virtual_text: Some((5, "xx yy ")),
        ..row("aaaa bbbb, with xx yy", "aaaa bbbb", "aaaa·xx·|yy·bbbb")
    },
    row(
        "aaaa + space+mark + bbbbbbb",
        "aaaa \u{301}bbbbbbb",
        "aaaa·|bbbbbbb",
    ),
    RowExample {
        wrap: Wrap::Char,
        ..row(
            "aaaa bbbbb cccc, Char",
            "aaaa bbbbb cccc",
            "aaaa·bbbbb|·cccc",
        )
    },
];

#[test]
fn row_examples() {
    let mut wrong = vec![];
    for e in ROW_EXAMPLES {
        let mut input = Input::new(e.text, 10).wrap(e.wrap);
        if let Some(h) = &e.hide {
            input = input.hide(h.clone(), None);
        }
        if let Some((pos, text)) = e.virtual_text {
            input = input.virtual_text(pos, text);
        }
        let got = layout(&input).render();
        if got != e.rows {
            wrong.push(format!("{}: want {} got {got}", e.doc, e.rows));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

// ---------------------------------------------------------------- worked examples: cursors

struct CursorExample {
    doc: &'static str,
    text: &'static str,
    rows: &'static str,
    /// The positions the contract lists, as (byte, row, column).
    positions: &'static [(usize, usize, usize)],
}

/// "Cursor positions on full rows and last rows, at 10 cells", in the contract's order.
const CURSOR_EXAMPLES: &[CursorExample] = &[
    CursorExample {
        doc: r"aaaaaaaaaa\nb\n",
        text: "aaaaaaaaaa\nb\n",
        rows: "aaaaaaaaaa|b|(empty)",
        positions: &[(9, 0, 9), (10, 0, 10), (11, 1, 0), (13, 2, 0)],
    },
    CursorExample {
        doc: "aaaaaaaaaa",
        text: "aaaaaaaaaa",
        rows: "aaaaaaaaaa",
        positions: &[(10, 0, 10)],
    },
    CursorExample {
        doc: r"a\tb\n",
        text: "a\tb\n",
        rows: "a→→→→→→→b|(empty)",
        positions: &[(1, 0, 1), (2, 0, 8), (3, 0, 9), (4, 1, 0)],
    },
    CursorExample {
        doc: r"aaaaaaaaaa\tb",
        text: "aaaaaaaaaa\tb",
        rows: "aaaaaaaaaa|→→→→→→b",
        positions: &[(10, 1, 0), (11, 1, 6), (12, 1, 7)],
    },
    CursorExample {
        doc: "(empty text)",
        text: "",
        rows: "(empty)",
        positions: &[(0, 0, 0)],
    },
];

#[test]
fn cursor_examples() {
    let mut wrong = vec![];
    for e in CURSOR_EXAMPLES {
        let l = layout(&Input::new(e.text, 10));
        if l.render() != e.rows {
            wrong.push(format!("{}: want {} got {}", e.doc, e.rows, l.render()));
        }
        for &(pos, row, col) in e.positions {
            if l.at(pos) != Some((row, col)) {
                wrong.push(format!(
                    "{} at {pos}: want ({row}, {col}) got {:?}",
                    e.doc,
                    l.at(pos)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

// ---------------------------------------------------------------- worked examples: numbers

const NUMBERED: &str = "one\na line that wraps\nthree\nfour\n";

/// One column of §9's numbers table: a style, the cursor, and the rows' numbers. The rows are
/// one, a line that, wraps, three, four and the empty row.
const NUMBER_COLUMNS: &[(LineNumbers, &str, [Option<usize>; 6])] = {
    use LineNumbers::*;
    const fn n(v: usize) -> Option<usize> {
        Some(v)
    }
    const X: Option<usize> = None;
    &[
        (Absolute, "three", [n(1), n(2), X, n(3), n(4), X]),
        (Relative, "three", [n(2), n(1), X, n(3), n(1), X]),
        (Visual, "three", [n(3), n(2), n(1), n(3), n(1), X]),
        (Absolute, "wraps", [n(1), n(2), X, n(3), n(4), X]),
        (Relative, "wraps", [n(1), n(2), X, n(1), n(2), X]),
        (Visual, "wraps", [n(2), n(1), n(2), n(1), n(2), X]),
        (Absolute, "", [n(1), n(2), X, n(3), n(4), X]),
        (Relative, "", [n(4), n(3), X, n(2), n(1), X]),
        (Visual, "", [n(5), n(4), n(3), n(2), n(1), X]),
    ]
};

/// The table gives neither its text nor its width, only row labels. These rows come from any
/// width from 12 to 16, so check both ends.
#[test]
fn line_number_table() {
    for width in [12, 16] {
        for (style, on, numbers) in NUMBER_COLUMNS {
            // An empty `on` is the cursor on the empty row: the end of the text.
            let cursor = if on.is_empty() {
                NUMBERED.len()
            } else {
                NUMBERED.find(on).unwrap()
            };
            let input = Input::new(NUMBERED, width).numbers(*style, cursor);
            let l = layout(&input);
            assert_eq!(l.render(), "one|a·line·that·|wraps|three|four|(empty)");
            assert_eq!(l.numbers(), numbers, "{style:?} with the cursor on {on:?}");
        }
    }
}

#[test]
fn fold_line_numbers() {
    use LineNumbers::*;
    // "With lines `a` to `e`, `b` to `d` folded, and the cursor on `a`". A whole-line fold stops
    // before the newline that ends its last line. The example works with a replacement or not.
    for replacement in [None, Some("folded")] {
        let fold = |style| {
            Input::new("a\nb\nc\nd\ne", 10)
                .hide(2..7, replacement)
                .numbers(style, 0)
        };
        let row = replacement.unwrap_or("(empty)");
        assert_eq!(layout(&fold(Absolute)).render(), format!("a|{row}|e"));
        check_numbers(fold(Absolute), &[Some(1), Some(2), Some(5)]);
        check_numbers(fold(Relative), &[Some(1), Some(1), Some(2)]);
        check_numbers(fold(Visual), &[Some(1), Some(1), Some(2)]);
    }
}

// ---------------------------------------------------------------- worked examples: gutter

#[test]
fn gutter_examples() {
    use LineNumbers::*;
    // "`cols` 120 with numbers on, under 1,000 lines, `max_width` 80 and centered gives a gutter
    // of 4, `text_w` 80, `pad` 18 and `text_x` 22."
    let g = geometry("a\n", Absolute, 120, Some(80), true);
    assert_eq!(
        (g.gutter, g.text_w, g.pad, g.text_x),
        (4, 80, 18, 22),
        "{g:?}"
    );
    // "A text of 999 lines with a final newline has `n` = 999, so a gutter of 4."
    let text = "x\n".repeat(999);
    assert_eq!(number_of_lines(&text), 999);
    assert_eq!(gutter_width(&text, Absolute), 4);
}

// ---------------------------------------------------------------- the tables are the contract's

fn contract_table(marker: &str) -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/extension-contract.md"
    );
    let contract = std::fs::read_to_string(path).unwrap();
    let after = &contract[contract.find(marker).expect(marker)..];
    let block = after.split("```").nth(1).expect("a code block");
    block
        .trim_matches('\n')
        .lines()
        .skip(1)
        .map(String::from)
        .collect()
}

/// If this fails, the contract's examples changed: change the tables above to match, not the
/// other way round.
#[test]
fn tables_match_the_contract() {
    let lines = contract_table("**Examples,** at 10 cells");
    assert_eq!(lines.len(), ROW_EXAMPLES.len());
    for (line, e) in lines.iter().zip(ROW_EXAMPLES) {
        let rest = line
            .strip_prefix(e.doc)
            .unwrap_or_else(|| panic!("{line}: {}", e.doc));
        assert!(rest.trim_start().starts_with(e.rows), "{line}: {}", e.rows);
    }

    let lines = contract_table("Cursor positions on full rows and last rows");
    assert_eq!(lines.len(), CURSOR_EXAMPLES.len());
    for (line, e) in lines.iter().zip(CURSOR_EXAMPLES) {
        let rest = line
            .strip_prefix(e.doc)
            .unwrap_or_else(|| panic!("{line}: {}", e.doc));
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(e.rows)
            .unwrap_or_else(|| panic!("{line}: {}", e.rows));
        let mut listed = vec![];
        for chunk in rest.split("    ").map(str::trim).filter(|c| !c.is_empty()) {
            let (head, tail) = chunk.split_once(" → ").unwrap();
            let pos: usize = head.split(',').next().unwrap().parse().unwrap();
            let (row, col) = tail.trim_matches(['(', ')']).split_once(", ").unwrap();
            listed.push((pos, row.parse().unwrap(), col.parse().unwrap()));
        }
        assert_eq!(listed, e.positions, "{line}");
    }
}

// ---------------------------------------------------------------- "Display lines"

#[test]
fn only_a_newline_ends_a_line() {
    // "so `\r` and U+2028 are characters like any other". A run stops at the newline, so `\r\n`
    // isn't one cluster either.
    check(
        Input::new("a\r\nb", 10),
        "a^M|b",
        &[(0, 0, 0), (1, 0, 1), (2, 0, 3), (3, 1, 0), (4, 1, 1)],
    );
    check(
        Input::new("a\u{2028}b", 10),
        "a\u{2028}b",
        &[(0, 0, 0), (1, 0, 1), (4, 0, 2), (5, 0, 3)],
    );
}

#[test]
fn a_hidden_newline_joins_lines() {
    use LineNumbers::*;
    let input = Input::new("ab\ncd\nef", 10).hide(2..3, None);
    check(
        input.clone(),
        "abcd|ef",
        &[
            (0, 0, 0),
            (1, 0, 1),
            (3, 0, 2),
            (4, 0, 3),
            (5, 0, 4),
            (6, 1, 0),
            (7, 1, 1),
            (8, 1, 2),
        ],
    );
    // "A display line's *first line* is the line it starts on."
    check_numbers(input.numbers(Absolute, 0), &[Some(1), Some(3)]);
    // With a replacement too, and across a line's start.
    check(
        Input::new("ab\ncd", 10).hide(1..4, Some("X")),
        "aXd",
        &[(0, 0, 0), (1, 0, 1), (4, 0, 2), (5, 0, 3)],
    );
    // Hiding a display line's start doesn't change its first line.
    check_numbers(
        Input::new("ab\ncd\nef", 10)
            .hide(0..3, None)
            .numbers(Absolute, 3),
        &[Some(1), Some(3)],
    );
}

// ---------------------------------------------------------------- "Hides merge first"

#[test]
fn an_empty_hide_hides_nothing_and_shows_nothing() {
    // Its replacement doesn't show, its position isn't a fold line, and no run stops there.
    check(
        Input::new("e\u{301}x", 10).hide(1..1, Some("R")),
        "e\u{301}x",
        &[(0, 0, 0), (3, 0, 1), (4, 0, 2)],
    );
}

#[test]
fn overlapping_hides_merge() {
    const T: &str = "abcdefghij";
    type Hides = &'static [(usize, usize, Option<&'static str>)];
    let cases: &[(&str, Hides, &str)] = &[
        (
            "two overlap: the first's replacement",
            &[(1, 4, Some("X")), (3, 6, Some("Y"))],
            "aXghij",
        ),
        (
            "added the other way round: still the one that starts first",
            &[(3, 6, Some("Y")), (1, 4, Some("X"))],
            "aXghij",
        ),
        (
            "a chain merges into one",
            &[(1, 3, Some("X")), (2, 5, Some("Y")), (4, 7, Some("Z"))],
            "aXhij",
        ),
        ("nested", &[(1, 6, Some("X")), (2, 3, Some("Y"))], "aXghij"),
        (
            "nested twice: the second is still inside the first",
            &[(1, 8, Some("X")), (2, 3, Some("Y")), (5, 6, Some("Z"))],
            "aXij",
        ),
        (
            "past a nested one, a Hide that overlaps only the first still merges",
            &[(1, 8, Some("X")), (2, 3, Some("Y")), (6, 10, Some("Z"))],
            "aX",
        ),
        (
            "no replacement that starts first hides a later one's",
            &[(1, 4, None), (2, 5, Some("Y"))],
            "afghij",
        ),
        (
            "same start: one with a replacement, though shorter",
            &[(1, 3, None), (1, 2, Some("Y"))],
            "aYdefghij",
        ),
        (
            "and whichever was added first",
            &[(1, 2, Some("Y")), (1, 3, None)],
            "aYdefghij",
        ),
        (
            "same start: then the longest",
            &[(1, 3, Some("X")), (1, 5, Some("Y"))],
            "aYfghij",
        ),
        (
            "same start and length: then the one added first",
            &[(1, 3, Some("X")), (1, 3, Some("Y"))],
            "aXdefghij",
        ),
        (
            "an empty replacement is no replacement",
            &[(1, 3, Some("")), (1, 2, Some("Y"))],
            "aYdefghij",
        ),
    ];
    for (why, hides, rows) in cases {
        let mut input = Input::new(T, 20);
        for (start, end, r) in *hides {
            input = input.hide(*start..*end, *r);
        }
        assert_eq!(layout(&input).render(), *rows, "{why}");
    }
    // A merged range has one cursor position at most: its start.
    let l = layout(
        &Input::new(T, 20)
            .hide(1..4, Some("X"))
            .hide(3..6, Some("Y")),
    );
    assert_eq!(l.at(1), Some((0, 1)));
    for inside in 2..6 {
        assert_eq!(l.at(inside), None);
    }
    assert_eq!(l.at(6), Some((0, 2)));
}

#[test]
fn touching_hides_stay_two() {
    check(
        Input::new("abcdef", 10)
            .hide(1..3, Some("X"))
            .hide(3..5, Some("Y")),
        "aXYf",
        &[(0, 0, 0), (1, 0, 1), (3, 0, 2), (5, 0, 3), (6, 0, 4)],
    );
    // Chained ranges that only touch the next also stop there.
    check(
        Input::new("abcdef", 10)
            .hide(1..3, Some("X"))
            .hide(2..4, None)
            .hide(4..5, Some("Y")),
        "aXYf",
        &[(0, 0, 0), (1, 0, 1), (4, 0, 2), (5, 0, 3), (6, 0, 4)],
    );
}

// ---------------------------------------------------------------- "Items"

#[test]
fn a_hidden_edge_inside_a_cluster_splits_it() {
    // The start of a hidden range: "e" is left alone.
    check(
        Input::new("e\u{301}x", 10).hide(1..3, None),
        "ex",
        &[(0, 0, 0), (3, 0, 1), (4, 0, 2)],
    );
    // The end of one: the mark is left alone, and is 1 cell.
    check(
        Input::new("e\u{301}x", 10).hide(0..1, None),
        "\u{301}x",
        &[(1, 0, 0), (3, 0, 1), (4, 0, 2)],
    );
}

#[test]
fn the_text_on_two_sides_of_a_hidden_range_never_forms_one_cluster() {
    check(
        Input::new("ex\u{301}", 10).hide(1..2, None),
        "e\u{301}",
        &[(0, 0, 0), (2, 0, 1), (4, 0, 2)],
    );
    // Nor does a replacement with the text after it.
    check(
        Input::new("xy\u{301}", 10).hide(0..2, Some("e")),
        "e\u{301}",
        &[(0, 0, 0), (2, 0, 1), (4, 0, 2)],
    );
}

#[test]
fn virtual_text_shows_where_the_contract_says() {
    type Positions = &'static [(usize, usize, usize)];
    let cases: &[(&str, Input, &str, Positions)] = &[
        (
            "before a cluster",
            Input::new("ab", 10).virtual_text(1, "V"),
            "aVb",
            &[(0, 0, 0), (1, 0, 2), (2, 0, 3)],
        ),
        (
            "before a newline",
            Input::new("a\nb", 10).virtual_text(1, "V"),
            "aV|b",
            &[(0, 0, 0), (1, 0, 2), (2, 1, 0), (3, 1, 1)],
        ),
        (
            "before the end of the text",
            Input::new("a", 10).virtual_text(1, "V"),
            "aV",
            &[(0, 0, 0), (1, 0, 2)],
        ),
        (
            "on the empty last row",
            Input::new("a\n", 10).virtual_text(2, "V"),
            "a|V",
            &[(0, 0, 0), (1, 0, 1), (2, 1, 1)],
        ),
        (
            "at a hidden range's start, before the replacement",
            Input::new("abc", 10)
                .hide(1..2, Some("R"))
                .virtual_text(1, "V"),
            "aVRc",
            &[(0, 0, 0), (1, 0, 2), (2, 0, 3), (3, 0, 4)],
        ),
        (
            "at a hidden range's start with no replacement, not at all",
            Input::new("abc", 10).hide(1..2, None).virtual_text(1, "V"),
            "ac",
            &[(0, 0, 0), (2, 0, 1), (3, 0, 2)],
        ),
        (
            "at a hidden range's end, as at a visible position",
            Input::new("abc", 10).hide(1..2, None).virtual_text(2, "V"),
            "aVc",
            &[(0, 0, 0), (2, 0, 2), (3, 0, 3)],
        ),
        (
            "at a hidden range's end where another starts: its start rule, replacement",
            Input::new("abcd", 10)
                .hide(1..2, None)
                .hide(2..3, Some("R"))
                .virtual_text(2, "V"),
            "aVRd",
            &[(0, 0, 0), (2, 0, 2), (3, 0, 3), (4, 0, 4)],
        ),
        (
            "at a hidden range's end where another starts: its start rule, none",
            Input::new("abcd", 10)
                .hide(1..2, Some("R"))
                .hide(2..3, None)
                .virtual_text(2, "V"),
            "aRd",
            &[(0, 0, 0), (1, 0, 1), (3, 0, 2), (4, 0, 3)],
        ),
        (
            "at a hidden range's end at the newline",
            Input::new("ab\n", 10).hide(1..2, None).virtual_text(2, "V"),
            "aV|(empty)",
            &[(0, 0, 0), (2, 0, 2), (3, 1, 0)],
        ),
        (
            "strictly inside a hidden range, not at all",
            Input::new("abcd", 10)
                .hide(1..3, Some("R"))
                .virtual_text(2, "V"),
            "aRd",
            &[(0, 0, 0), (1, 0, 1), (3, 0, 2), (4, 0, 3)],
        ),
        (
            "strictly inside a cluster, not at all",
            Input::new("e\u{301}x", 10).virtual_text(1, "V"),
            "e\u{301}x",
            &[(0, 0, 0), (3, 0, 1), (4, 0, 2)],
        ),
        (
            "at one place, in the order added",
            Input::new("ab", 10)
                .virtual_text(1, "2")
                .virtual_text(1, "1"),
            "a21b",
            &[(0, 0, 0), (1, 0, 3), (2, 0, 4)],
        ),
        (
            "clusters are found in each virtual text on its own",
            Input::new("ab", 10)
                .virtual_text(1, "e")
                .virtual_text(1, "\u{301}"),
            "ae\u{301}b",
            &[(0, 0, 0), (1, 0, 3), (2, 0, 4)],
        ),
        (
            "and it doesn't join the text beside it",
            Input::new("ab", 10).virtual_text(1, "\u{301}"),
            "a\u{301}b",
            &[(0, 0, 0), (1, 0, 2), (2, 0, 3)],
        ),
    ];
    for (why, input, rows, positions) in cases {
        let l = layout(input);
        assert_eq!(l.render(), *rows, "{why}");
        let got: Vec<_> = l.positions.iter().map(|p| (p.pos, p.row, p.col)).collect();
        assert_eq!(got, *positions, "{why}");
    }
}

#[test]
fn replacements_and_virtual_text_hold_no_tabs_or_newlines() {
    // "`decorate` turns their tabs and newlines into spaces". A space in one is whitespace.
    check_rows(Input::new("abc", 10).hide(1..2, Some("x\ty\nz")), "ax·y·zc");
    check_rows(Input::new("ab", 10).virtual_text(1, "x\ty\nz"), "ax·y·zb");
    check_rows(
        Input::new("aaaa bbbb", 10).virtual_text(5, "cc\tdd"),
        "aaaa·cc·|ddbbbb",
    );
    // "an empty replacement is no replacement": no cursor position either.
    check(
        Input::new("abc", 10).hide(1..2, Some("")),
        "ac",
        &[(0, 0, 0), (2, 0, 1), (3, 0, 2)],
    );
}

// ---------------------------------------------------------------- "Widths"

#[test]
fn tabs_reach_the_next_multiple_of_tab_width() {
    check_rows(Input::new("a\tb", 10).tab_width(4), "a→→→b");
    check_rows(Input::new("aaaa\tb", 10).tab_width(4), "aaaa→→→→b");
    // "where 0 counts as 1".
    check_rows(Input::new("aa\tb", 10).tab_width(0), "aa→b");
    check_rows(Input::new("aa\tb", 10).tab_width(1), "aa→b");
    // Every item before it counts, of any kind, and hidden text doesn't.
    check_rows(
        Input::new("xy\tb", 10).tab_width(4).hide(0..2, Some("R")),
        "R→→→b",
    );
    check_rows(
        Input::new("xy\tb", 10).tab_width(4).hide(0..2, None),
        "→→→→b",
    );
    check_rows(
        Input::new("\tb", 10).tab_width(4).virtual_text(0, "VV"),
        "VV→→b",
    );
    check_rows(Input::new("中\tb", 10).tab_width(4), "中→→b");
    check_rows(Input::new("\u{1}\tb", 10).tab_width(4), "^A→→b");
    // Across joined lines too: a display line's cells, not a line's.
    check_rows(
        Input::new("ab\ncd\te", 12).tab_width(8).hide(2..3, None),
        "abcd→→→→e",
    );
}

#[test]
fn a_display_lines_columns_dont_change_with_the_width() {
    let text = "aaaaaaaaa中\tbb cc\u{1}d";
    let wide = layout(&Input::new(text, 80));
    for width in 2..12 {
        for wrap in [Wrap::Word, Wrap::Char, Wrap::Off] {
            let l = layout(&Input::new(text, width).wrap(wrap));
            let got: Vec<_> = l.positions.iter().map(|p| (p.pos, p.display_col)).collect();
            let want: Vec<_> = wide
                .positions
                .iter()
                .map(|p| (p.pos, p.display_col))
                .collect();
            assert_eq!(got, want, "width {width}, {wrap:?}");
        }
    }
}

#[test]
fn control_characters_are_drawn_as_emacs_draws_them() {
    let cases = [
        ('\u{0}', "^@"),
        ('\u{1}', "^A"),
        ('\r', "^M"),
        ('\u{1b}', "^["),
        ('\u{1f}', "^_"),
        ('\u{7f}', "^?"),
        ('\u{80}', "\\200"),
        ('\u{85}', "\\205"),
        ('\u{9f}', "\\237"),
    ];
    for (c, drawing) in cases {
        let text = format!("{c}b");
        let l = layout(&Input::new(&text, 10));
        assert_eq!(l.render(), format!("{drawing}b"), "{c:?}");
        // A cell per character, and b follows them.
        assert_eq!(l.at(c.len_utf8()), Some((0, drawing.len())), "{c:?}");
    }
    // Any item: a replacement or virtual text can hold one, since only tabs and newlines go.
    check_rows(Input::new("abc", 10).hide(1..2, Some("\r")), "a^Mc");
    check_rows(Input::new("ab", 10).virtual_text(1, "\u{1}"), "a^Ab");
}

#[test]
fn other_items_take_their_clusters_width_and_at_least_one_cell() {
    // (text before `x`, the cells it takes)
    let cases = [
        ("中", 2),
        ("👍🏽", 2),
        ("\u{200b}", 1),
        ("\u{301}", 1),
        ("e\u{301}", 1),
        ("\u{3000}", 2),
        ("\u{a0}", 1),
    ];
    for (before, cells) in cases {
        let text = format!("{before}x");
        let l = layout(&Input::new(&text, 10));
        assert_eq!(l.at(before.len()), Some((0, cells)), "{before:?}");
        // "It's drawn in its first cell, and its other cells continue it."
        let row = &l.rows[0].cells;
        assert_eq!(row[0], Cell::Cluster(before.to_string()));
        for continued in &row[1..cells] {
            assert_eq!(*continued, Cell::Continue);
        }
    }
}

#[test]
fn oversize_cluster_is_five_cells() {
    // The cluster the rule 3 tests use. If the Unicode crates ever change their minds, those
    // tests need another cluster, not another answer.
    assert_eq!(layout(&Input::new(WIDE, 80)).at(WIDE.len()), Some((0, 5)));
}

// ---------------------------------------------------------------- "Where soft wrap breaks"

#[test]
fn only_a_space_or_a_tab_first_makes_whitespace() {
    // "A no-break space, U+200B, U+3000 and `-` aren't whitespace, and nothing breaks at them."
    check_rows(Input::new("aaaa\u{a0}bbbbbbbb", 10), "aaaa\u{a0}bbbbb|bbb");
    check_rows(
        Input::new("aaaa\u{200b}bbbbbbbb", 10),
        "aaaa\u{200b}bbbbb|bbb",
    );
    check_rows(
        Input::new("aaaa\u{3000}bbbbbbbb", 10),
        "aaaa\u{3000}bbbb|bbbb",
    );
    check_rows(Input::new("aaaa-bbbbbbbb", 10), "aaaa-bbbbb|bbb");
    // A tab is, as a space is.
    check_rows(Input::new("aa\tbbbbbbbb", 10).tab_width(4), "aa→→|bbbbbbbb");
    // A cluster starts with what's first in it: a prepended mark before a space isn't whitespace.
    check_rows(
        Input::new("aaaa\u{600} bbbbbbbb", 12),
        "aaaa\u{600} bbbbbb|bb",
    );
}

/// Reading: "unless that item is the first thing on its row". That item is the one that isn't
/// whitespace, so whitespace first on its row still makes a break point after it. Reading it as
/// the whitespace item would give `·bbbbbbbbb|bbb` for the first of these.
#[test]
fn whitespace_first_on_its_row_still_makes_a_break_point() {
    check_rows(Input::new(" bbbbbbbbbbbb", 10), "·|bbbbbbbbbb|bb");
    check_rows(Input::new("\tbbbbbbbbbbbb", 10), "→→→→→→→→|bbbbbbbbbb|bb");
    check_rows(
        Input::new("aaaaaaaaaa bbbbbbbbbbbb", 10),
        "aaaaaaaaaa|·|bbbbbbbbbb|bb",
    );
}

#[test]
fn the_first_thing_on_a_row_is_no_break_point() {
    // After the break, b is first, so the long word breaks anywhere and leaves no empty row.
    check_rows(
        Input::new("aaaa bbbbbbbbbbbbbbbbbbbbbbbb", 10),
        "aaaa·|bbbbbbbbbb|bbbbbbbbbb|bbbb",
    );
    // A wide cluster first on its row: no break point before it, so it stays.
    check_rows(Input::new("aaaa 中中中中中中", 10), "aaaa·|中中中中中|中");
}

#[test]
fn hidden_text_doesnt_stop_a_break_point() {
    check_rows(
        Input::new("aaaa xbbbbbbb", 10).hide(5..6, None),
        "aaaa·|bbbbbbb",
    );
    check_rows(
        Input::new("aaaa \nbbbbbbb", 10).hide(5..6, None),
        "aaaa·|bbbbbbb",
    );
    // A replacement's items break like text, before and after.
    check_rows(
        Input::new("aaaa xbbbb", 10).hide(5..6, Some("RRRRRRR")),
        "aaaa·|RRRRRRRbbb|b",
    );
    check_rows(
        Input::new("aaaaaaaaxbbbb", 10).hide(8..9, Some(" ")),
        "aaaaaaaa·|bbbb",
    );
}

/// Reading: "The rest of a split tab or control character counts as being on the row, so the item
/// after it can be a break point." A control character isn't whitespace, so what follows one isn't
/// a break point, split or not. The other reading gives `aaaaaaaaa^|A|bbbbbbbbbb|b` for the first.
#[test]
fn the_item_after_a_control_characters_rest_is_no_break_point() {
    check_rows(
        Input::new("aaaaaaaaa\u{1}bbbbbbbbbbb", 10),
        "aaaaaaaaa^|Abbbbbbbbb|bb",
    );
    // But whitespace after the rest makes one as usual.
    check_rows(
        Input::new("aaaaaaaaa\u{1} bbbbbbbbbbb", 10),
        "aaaaaaaaa^|A·|bbbbbbbbbb|b",
    );
}

#[test]
fn a_wide_item_moves_whole_and_a_narrow_one_leaves_no_filler() {
    let l = layout(&Input::new("aaaaaaaaa中", 10));
    assert_eq!(l.render(), "aaaaaaaaa_|中");
    assert_eq!(l.rows[0].cells.len(), 10);
    assert_eq!(l.rows[0].cells[9], Cell::Filler);
    assert_eq!(l.at(9), Some((1, 0)));
    // Whitespace wider than a cell is no tab, so it moves whole too.
    check_rows(Input::new("aaaaaaaaa \u{903}", 10), "aaaaaaaaa_|·");
    // No filler from a one-cell item, and none on a row a break point ended.
    check_rows(Input::new("aaaaaaaaaab", 10), "aaaaaaaaaa|b");
    assert_eq!(
        layout(&Input::new("aaaaaaaa 中", 10)).rows[0].cells.len(),
        9
    );
}

#[test]
fn a_tab_or_control_character_splits_over_several_rows() {
    // Row 1 is all the tab's rest, so no position lands on it.
    check(Input::new("\t", 3), "→→→|→→→|→→", &[(0, 0, 0), (1, 2, 2)]);
    // Exactly full: no blank row after it.
    check(Input::new("\t", 4), "→→→→|→→→→", &[(0, 0, 0), (1, 1, 4)]);
    check(Input::new("\u{80}", 3), "\\20|0", &[(0, 0, 0), (2, 1, 1)]);
    check(Input::new("\u{80}", 2), "\\2|00", &[(0, 0, 0), (2, 1, 2)]);
    // No cell of it fits: its position is on the next row.
    check(
        Input::new("aaaaaaaaaa\u{1}", 10),
        "aaaaaaaaaa|^A",
        &[
            (0, 0, 0),
            (1, 0, 1),
            (2, 0, 2),
            (3, 0, 3),
            (4, 0, 4),
            (5, 0, 5),
            (6, 0, 6),
            (7, 0, 7),
            (8, 0, 8),
            (9, 0, 9),
            (10, 1, 0),
            (11, 1, 2),
        ],
    );
}

#[test]
fn a_cluster_wider_than_a_row_stays_at_its_rows_start() {
    let cells = |text: &str, wrap| {
        layout(&Input::new(text, 3).wrap(wrap)).rows[0]
            .cells
            .clone()
    };
    let cut = vec![
        Cell::Cluster(WIDE.to_string()),
        Cell::Continue,
        Cell::Continue,
    ];
    // "Its cells past the row's end aren't drawn."
    assert_eq!(cells(WIDE, Wrap::Word), cut);
    assert_eq!(cells(WIDE, Wrap::Char), cut);
    // "a line end there is at column `text_w`".
    check(Input::new(WIDE, 3), WIDE, &[(0, 0, 0), (WIDE.len(), 0, 3)]);
    let newline = format!("{WIDE}\n");
    assert_eq!(
        layout(&Input::new(&newline, 3)).at(WIDE.len()),
        Some((0, 3))
    );
    // "the next item starts a new row".
    let after = format!("{WIDE}x");
    check(
        Input::new(&after, 3),
        &format!("{WIDE}|x"),
        &[(0, 0, 0), (WIDE.len(), 1, 0), (WIDE.len() + 1, 1, 1)],
    );
    // It moves to its row's start first, as any wide item does, leaving filler.
    let before = format!("x{WIDE}");
    check(
        Input::new(&before, 3),
        &format!("x__|{WIDE}"),
        &[(0, 0, 0), (1, 1, 0), (1 + WIDE.len(), 1, 3)],
    );
    // A break point comes first: no filler.
    let word = format!("x {WIDE}");
    check_rows(Input::new(&word, 3), &format!("x·|{WIDE}"));
    // Reading: its undrawn cells still count toward tab stops, so the tab runs from column 5 to 8.
    // Counting only the cells drawn would give it 5 cells.
    let tab = format!("{WIDE}\tb");
    let l = layout(&Input::new(&tab, 3));
    assert_eq!(l.render(), format!("{WIDE}|→→→|b"));
    assert_eq!(
        l.positions
            .iter()
            .find(|p| p.pos == WIDE.len() + 1)
            .unwrap()
            .display_col,
        8
    );
    // Replacements and virtual text can be this wide too.
    check_rows(
        Input::new("ab", 3).hide(0..1, Some(WIDE)),
        &format!("{WIDE}|b"),
    );
    check_rows(
        Input::new("b", 3).virtual_text(0, WIDE),
        &format!("{WIDE}|b"),
    );
}

#[test]
fn char_wrap_is_the_same_rule_with_no_break_points() {
    let char_wrap = |text| Input::new(text, 10).wrap(Wrap::Char);
    check_rows(char_wrap("aaaaaaaaa中b"), "aaaaaaaaa_|中b");
    check_rows(char_wrap("aaaaaaaaa\tbb"), "aaaaaaaaa→|→→→→→→bb");
    check_rows(char_wrap("aaaaaaaa \u{1}b"), "aaaaaaaa·^|Ab");
    check_rows(char_wrap("aa bbbbbbbb cc"), "aa·bbbbbbb|b·cc");
    check_rows(char_wrap("aaaaaaaaaa bbbb"), "aaaaaaaaaa|·bbbb");
}

#[test]
fn wrap_off_makes_each_display_line_one_row() {
    let off = |text| Input::new(text, 10).wrap(Wrap::Off);
    check_rows(
        off("aaaaaaaaaa bbbbbbbbbbbbbb"),
        "aaaaaaaaaa·bbbbbbbbbbbbbb",
    );
    // Nothing moves or splits, so no filler, and a full row isn't full.
    let l = layout(&off("aaaaaaaaa中\tb"));
    assert_eq!(l.render(), "aaaaaaaaa中→→→→→b");
    assert_eq!(l.at(9), Some((0, 9)));
    assert_eq!(l.at("aaaaaaaaa中\tb".len()), Some((0, 17)));
    // Past a row's width, a cluster isn't cut either.
    let wide = layout(&Input::new(WIDE, 3).wrap(Wrap::Off));
    assert_eq!(wide.rows[0].cells.len(), 5);
    assert_eq!(wide.at(WIDE.len()), Some((0, 5)));
    // A newline still ends a display line.
    check_rows(off("a\nb"), "a|b");
}

// ---------------------------------------------------------------- "Line ends", "The empty last row"

#[test]
fn a_line_end_stays_on_its_full_row() {
    // Full by whitespace, a tab, a wide cluster or virtual text: no blank row.
    check_rows(Input::new("aaaaaaaaa ", 10), "aaaaaaaaa·");
    assert_eq!(layout(&Input::new("aaaaaaaaa ", 10)).at(10), Some((0, 10)));
    let l = layout(&Input::new("aa\t", 10).tab_width(10));
    assert_eq!(l.render(), "aa→→→→→→→→");
    assert_eq!(l.at(3), Some((0, 10)));
    let l = layout(&Input::new("aaaaaaaa中", 10));
    assert_eq!(l.at("aaaaaaaa中".len()), Some((0, 10)));
    let l = layout(&Input::new("aaaa", 10).virtual_text(4, "xxxxxx"));
    assert_eq!(l.render(), "aaaaxxxxxx");
    assert_eq!(l.at(4), Some((0, 10)));
}

#[test]
fn virtual_text_at_a_line_end_wraps_and_the_line_end_follows_it() {
    check(
        Input::new("aaaaaaaaaa", 10).virtual_text(10, "x"),
        "aaaaaaaaaa|x",
        &[
            (0, 0, 0),
            (1, 0, 1),
            (2, 0, 2),
            (3, 0, 3),
            (4, 0, 4),
            (5, 0, 5),
            (6, 0, 6),
            (7, 0, 7),
            (8, 0, 8),
            (9, 0, 9),
            (10, 1, 1),
        ],
    );
}

#[test]
fn a_final_newline_makes_a_row_of_its_own() {
    check(
        Input::new("a\n", 10),
        "a|(empty)",
        &[(0, 0, 0), (1, 0, 1), (2, 1, 0)],
    );
    check(
        Input::new("\n", 10),
        "(empty)|(empty)",
        &[(0, 0, 0), (1, 1, 0)],
    );
    // "and so does an empty line anywhere".
    check(
        Input::new("\n\nb", 10),
        "(empty)|(empty)|b",
        &[(0, 0, 0), (1, 1, 0), (2, 2, 0), (3, 2, 1)],
    );
    // With Wrap::Off too.
    check_rows(Input::new("a\n", 10).wrap(Wrap::Off), "a|(empty)");
}

#[test]
fn a_hide_over_the_final_newline_leaves_no_empty_row() {
    check(
        Input::new("a\n", 10).hide(1..2, None),
        "a",
        &[(0, 0, 0), (2, 0, 1)],
    );
    check(
        Input::new("a\n", 10).hide(0..2, None),
        "(empty)",
        &[(2, 0, 0)],
    );
    check(
        Input::new("a\n", 10).hide(1..2, Some("R")),
        "aR",
        &[(0, 0, 0), (1, 0, 1), (2, 0, 2)],
    );
}

// ---------------------------------------------------------------- "Cursor positions"

#[test]
fn hidden_text_has_cursor_positions_only_at_a_replacement() {
    // "the start of a hidden range with a replacement, drawn on the replacement's first cell", and
    // "a replacement's later clusters have none".
    check(
        Input::new("abc", 10).hide(1..2, Some("RR")),
        "aRRc",
        &[(0, 0, 0), (1, 0, 1), (2, 0, 3), (3, 0, 4)],
    );
    // A wide replacement that moves: its first cell is on the next row.
    check(
        Input::new("aaaaaaaaaXb", 10).hide(9..10, Some("中")),
        "aaaaaaaaa_|中b",
        &[
            (0, 0, 0),
            (1, 0, 1),
            (2, 0, 2),
            (3, 0, 3),
            (4, 0, 4),
            (5, 0, 5),
            (6, 0, 6),
            (7, 0, 7),
            (8, 0, 8),
            (9, 1, 0),
            (10, 1, 2),
            (11, 1, 3),
        ],
    );
    // "A hidden range with no replacement has no cursor position at its start or inside it".
    check(
        Input::new("abcd", 10).hide(1..3, None),
        "ad",
        &[(0, 0, 0), (3, 0, 1), (4, 0, 2)],
    );
    // The end of the text has one, even with all the text before it hidden.
    check(
        Input::new("abc", 10).hide(0..3, None),
        "(empty)",
        &[(3, 0, 0)],
    );
    // A row with none: the end of a long replacement.
    check(
        Input::new("abc", 10).hide(1..2, Some(&"R".repeat(25))),
        "aRRRRRRRRR|RRRRRRRRRR|RRRRRRc",
        &[(0, 0, 0), (1, 0, 1), (2, 2, 6), (3, 2, 7)],
    );
}

#[test]
fn a_position_inside_a_cluster_isnt_one() {
    let l = layout(&Input::new("👍🏽x\u{301}", 10));
    let got: Vec<_> = l.positions.iter().map(|p| p.pos).collect();
    assert_eq!(got, [0, 8, 11]);
}

#[test]
fn a_whole_line_fold_keeps_the_next_lines_row() {
    check(
        Input::new("a\nb\nc\n", 10).hide(2..3, Some("…")),
        "a|…|c|(empty)",
        &[
            (0, 0, 0),
            (1, 0, 1),
            (2, 1, 0),
            (3, 1, 1),
            (4, 2, 0),
            (5, 2, 1),
            (6, 3, 0),
        ],
    );
    // With no replacement, its newline keeps a position.
    check(
        Input::new("a\nb\nc", 10).hide(2..3, None),
        "a|(empty)|c",
        &[(0, 0, 0), (1, 0, 1), (3, 1, 0), (4, 2, 0), (5, 2, 1)],
    );
}

// ---------------------------------------------------------------- "Gutter and width"

#[test]
fn geometry_follows_its_formulas() {
    use LineNumbers::*;
    // (numbers, cols, max_width, centered) -> (gutter, avail, text_w, pad, text_x)
    let cases = [
        ((Off, 120, None, false), (0, 120, 120, 0, 0)),
        ((Off, 120, None, true), (0, 120, 120, 0, 0)),
        ((Off, 120, Some(80), false), (0, 120, 80, 0, 0)),
        ((Off, 120, Some(80), true), (0, 120, 80, 20, 20)),
        ((Off, 121, Some(80), true), (0, 121, 80, 20, 20)),
        ((Off, 40, Some(80), true), (0, 40, 40, 0, 0)),
        ((Absolute, 120, None, false), (4, 116, 116, 0, 4)),
        ((Relative, 120, Some(80), false), (4, 116, 80, 0, 4)),
        ((Visual, 120, Some(80), true), (4, 116, 80, 18, 22)),
        // "at least 2", twice.
        ((Absolute, 3, None, false), (4, 2, 2, 0, 4)),
        ((Absolute, 0, None, true), (4, 2, 2, 0, 4)),
        ((Off, 1, None, false), (0, 2, 2, 0, 0)),
        ((Off, 120, Some(0), false), (0, 120, 2, 0, 0)),
        ((Off, 120, Some(1), true), (0, 120, 2, 59, 59)),
    ];
    for ((numbers, cols, max_width, centered), want) in cases {
        let g = geometry("a\n", numbers, cols, max_width, centered);
        assert_eq!(
            (g.gutter, g.avail, g.text_w, g.pad, g.text_x),
            want,
            "{numbers:?} {cols} {max_width:?} {centered}"
        );
    }
}

#[test]
fn the_gutter_counts_lines_not_an_empty_last_one() {
    use LineNumbers::*;
    assert_eq!(number_of_lines(""), 0);
    assert_eq!(number_of_lines("a"), 1);
    assert_eq!(number_of_lines("a\n"), 1);
    assert_eq!(number_of_lines("\n"), 1);
    assert_eq!(number_of_lines("a\nb"), 2);
    assert_eq!(number_of_lines("a\n\n"), 2);
    // Only a newline makes a line.
    assert_eq!(number_of_lines("a\rb\u{2028}c"), 1);

    assert_eq!(gutter_width("", Off), 0);
    assert_eq!(gutter_width("", Absolute), 4);
    assert_eq!(gutter_width(&"x\n".repeat(999), Off), 0);
    assert_eq!(gutter_width(&"x\n".repeat(999), Visual), 4);
    assert_eq!(gutter_width(&"x\n".repeat(1000), Absolute), 5);
    assert_eq!(
        gutter_width(&format!("{}x", "x\n".repeat(999)), Relative),
        5
    );
}

#[test]
fn a_gutter_right_aligns_its_number_and_keeps_its_last_cell_blank() {
    use LineNumbers::*;
    let gutters = |input: Input| -> Vec<String> {
        layout(&input)
            .rows
            .iter()
            .map(|r| r.gutter.clone())
            .collect()
    };
    assert_eq!(gutters(Input::new("a\nb", 10)), ["", ""]);
    assert_eq!(
        gutters(Input::new("a\nb", 10).numbers(Absolute, 0)),
        ["  1 ", "  2 "]
    );
    // A row with no number is still the gutter's width.
    assert_eq!(
        gutters(Input::new("aaaaaaaaaaaa\n", 10).numbers(Absolute, 0)),
        ["  1 ", "    ", "    "]
    );
    let text = "x\n".repeat(1000);
    let l = layout(&Input::new(&text, 10).numbers(Absolute, 0));
    assert_eq!(l.gutter, 5);
    assert_eq!(l.rows[0].gutter, "   1 ");
    assert_eq!(l.rows[999].gutter, "1000 ");
}

/// Reading: "The gutter draws a number right-aligned in its first `gutter − 1` cells". Visual's
/// distances count rows, so one can have more digits than those cells. Its last digits show.
#[test]
fn a_visual_number_wider_than_its_gutter_shows_its_last_digits() {
    let text = "a".repeat(2004);
    let l = layout(&Input::new(&text, 2).numbers(LineNumbers::Visual, 0));
    assert_eq!(l.gutter, 4);
    assert_eq!(l.rows.len(), 1002);
    assert_eq!(l.rows[1000].number, Some(1000));
    assert_eq!(l.rows[1000].gutter, "000 ");
    assert_eq!(l.rows[999].gutter, "999 ");
}

// ---------------------------------------------------------------- "Line numbers"

#[test]
fn a_display_line_starting_at_the_end_of_the_text_has_no_number() {
    use LineNumbers::*;
    for style in [Absolute, Relative, Visual] {
        check_numbers(Input::new("", 10).numbers(style, 0), &[None]);
        // Even with the cursor on it.
        check_numbers(Input::new("a\n", 10).numbers(style, 2), &[Some(1), None]);
    }
    // "It still counts when other rows measure their distance from it."
    check_numbers(
        Input::new("a\nb\n", 10).numbers(Relative, 4),
        &[Some(2), Some(1), None],
    );
    check_numbers(
        Input::new("a\nb\n", 10).numbers(Visual, 4),
        &[Some(2), Some(1), None],
    );
    check_numbers(
        Input::new("a\nb\n", 10).numbers(Absolute, 4),
        &[Some(1), Some(2), None],
    );
}

/// Reading: "In every style, a display line that starts at the end of the text has no number" is
/// about all its rows, though Visual gives every other row a distance.
#[test]
fn no_row_of_that_display_line_has_a_number() {
    use LineNumbers::*;
    let long = "x".repeat(15);
    let input = Input::new("a\n", 10).virtual_text(2, &long);
    assert_eq!(
        layout(&input).render(),
        format!("a|{}|{}", &long[..10], &long[10..])
    );
    check_numbers(input.clone().numbers(Visual, 0), &[Some(1), None, None]);
    check_numbers(input.clone().numbers(Absolute, 0), &[Some(1), None, None]);
    // The cursor at the end of the text is on its last row.
    check_numbers(input.numbers(Visual, 2), &[Some(2), None, None]);
}

/// Reading: the rule is about where a display line starts, not about what shows. With the last
/// line hidden, the row looks like the empty last row but starts before the end of the text.
#[test]
fn a_hidden_last_line_keeps_its_number() {
    use LineNumbers::*;
    let hidden = |style| Input::new("a\nb", 10).hide(2..3, None).numbers(style, 0);
    assert_eq!(layout(&hidden(Absolute)).render(), "a|(empty)");
    check_numbers(hidden(Absolute), &[Some(1), Some(2)]);
}

#[test]
fn absolute_and_relative_number_only_a_display_lines_first_row() {
    use LineNumbers::*;
    let text = "aaaaaaaaaaaa\nb\ncccccccccccc";
    check_numbers(
        Input::new(text, 10).numbers(Absolute, 0),
        &[Some(1), None, Some(2), Some(3), None],
    );
    // The cursor on a continuation row: its display line's first row shows the number.
    check_numbers(
        Input::new(text, 10).numbers(Relative, 11),
        &[Some(1), None, Some(1), Some(2), None],
    );
    check_numbers(
        Input::new(text, 10).numbers(Relative, text.len()),
        &[Some(2), None, Some(1), Some(3), None],
    );
}

#[test]
fn visual_counts_rows_with_no_cursor_position() {
    // The tab's middle row has none, and b's row counts it.
    let input = Input::new("\t\nb", 3).numbers(LineNumbers::Visual, 2);
    assert_eq!(layout(&input).render(), "→→→|→→→|→→|b");
    check_numbers(input, &[Some(3), Some(2), Some(1), Some(2)]);
}

/// §9: with the cursor on `c`, where a Hide joins it to `a`.
#[test]
fn numbers_on_a_fold_joining_lines() {
    use LineNumbers::*;
    let input = Input::new("a\nb\nc\nd\n", 10).hide(1..4, None);
    for (style, cursor, want) in [
        (Absolute, 4, [Some(1), Some(4), None]),
        (Relative, 4, [Some(1), Some(1), None]),
        (Visual, 4, [Some(3), Some(1), None]),
        (Visual, 0, [Some(1), Some(1), None]),
    ] {
        check_numbers(input.clone().numbers(style, cursor), &want);
    }
}

#[test]
fn visual_on_a_continuation_row_shows_its_display_lines_number() {
    let text = "x\naaaaaaaaaaaa";
    check_numbers(
        Input::new(text, 10).numbers(LineNumbers::Visual, 13),
        &[Some(2), Some(1), Some(2)],
    );
}

#[test]
fn numbers_read_where_core_moves_the_cursor() {
    use LineNumbers::*;
    // Inside a range with a replacement: its fold line.
    check_numbers(
        Input::new("a\nb\nc\nd\ne", 10)
            .hide(2..7, Some("F"))
            .numbers(Relative, 4),
        &[Some(1), Some(2), Some(1)],
    );
    // That's its first row, not the next visible position, which here is a row later.
    let wide_fold = "R".repeat(12);
    check_numbers(
        Input::new("x\nyyyy", 10)
            .hide(2..5, Some(&wide_fold))
            .numbers(Visual, 3),
        &[Some(1), Some(2), Some(1)],
    );
    // At or inside a range with none: the next visible position.
    check_numbers(
        Input::new("a\nb\nc", 10)
            .hide(2..3, None)
            .numbers(Visual, 2),
        &[Some(1), Some(2), Some(1)],
    );
    // The next visible position can be on the next row: w, past the hidden run.
    check_numbers(
        Input::new("a\nxxxxxxxxxxyzw", 10)
            .hide(12..14, None)
            .numbers(Visual, 12),
        &[Some(2), Some(1), Some(2)],
    );
    // Inside a cluster: the next cursor position, here on the next row.
    check_numbers(
        Input::new("x\nae\u{301}b", 2).numbers(Visual, 4),
        &[Some(2), Some(1), Some(2)],
    );
    // A fold line elsewhere on the display line doesn't take the cursor.
    check_numbers(
        Input::new("xxxxxxxxxxxx yz\nb", 10)
            .hide(0..1, Some("F"))
            .hide(13..14, None)
            .numbers(Visual, 13),
        &[Some(1), Some(1), Some(1)],
    );
}

// ---------------------------------------------------------------- everything together

/// What must hold for any input, over a grid of awkward ones. It also shows the layout ends on
/// each, which K4's property test relies on.
#[test]
fn invariants_hold_over_a_grid_of_inputs() {
    let texts = [
        "",
        "a",
        "\n",
        "\n\n",
        "a\n",
        "a b\n\nc  d\t",
        "\tb\t\t c",
        "中文 中文\t中文",
        "e\u{301} \u{301}x\r\n\u{301}",
        "\u{1}\u{80}\u{7f} \u{1}",
        "👍🏽 👍🏽\u{200b}\u{3000}x",
        WIDE,
        "x a\u{903}\u{903}\u{903}\u{903} y",
    ];
    for text in texts {
        let mut bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        bounds.push(text.len());
        let mut inputs = vec![Input::new(text, 2)];
        for &start in &bounds {
            inputs.push(Input::new(text, 2).virtual_text(start, "v v"));
            for &end in &bounds {
                if start < end {
                    inputs.push(Input::new(text, 2).hide(start..end, None));
                    inputs.push(Input::new(text, 2).hide(start..end, Some("r r")));
                }
            }
        }
        for input in inputs {
            for tab_width in [0, 3, 8] {
                let mut columns = None;
                for wrap in [Wrap::Word, Wrap::Char, Wrap::Off] {
                    for text_w in 2..9 {
                        let mut input = input.clone();
                        input.tab_width = tab_width;
                        input.wrap = wrap;
                        input.text_w = text_w;
                        input.numbers = LineNumbers::Visual;
                        let got = check_invariants(&input);
                        assert_eq!(*columns.get_or_insert(got.clone()), got, "{input:?}");
                    }
                }
            }
        }
    }
}

/// Check one layout, and return its positions' display columns, which no width or wrap can change.
fn check_invariants(input: &Input) -> Vec<(usize, usize)> {
    let l = layout(input);
    assert!(!l.rows.is_empty(), "{input:?}");
    // The end of the text is always the last cursor position, whatever's hidden.
    assert_eq!(
        l.positions.last().unwrap().pos,
        input.text.len(),
        "{input:?}"
    );
    for pair in l.positions.windows(2) {
        // In byte order, and each drawn after the one before.
        assert!(pair[0].pos < pair[1].pos, "{input:?}");
        assert!(
            (pair[0].row, pair[0].col) < (pair[1].row, pair[1].col),
            "{input:?}"
        );
        // Display columns start again on each display line.
        if l.rows[pair[0].row].display_line == l.rows[pair[1].row].display_line {
            assert!(pair[0].display_col < pair[1].display_col, "{input:?}");
        }
    }
    for p in &l.positions {
        let cells = &l.rows[p.row].cells;
        // On a cell that's an item's first, or at a row's next column for a line end.
        assert!(p.col <= cells.len(), "{input:?}");
        if let Some(cell) = cells.get(p.col) {
            assert!(!matches!(cell, Cell::Continue | Cell::Filler), "{input:?}");
        }
    }
    for (r, row) in l.rows.iter().enumerate() {
        if input.wrap != Wrap::Off {
            assert!(row.cells.len() <= input.text_w, "{input:?}");
        }
        // A row is empty only if it's a whole display line's only row.
        if row.cells.is_empty() {
            let alone = (r == 0 || l.rows[r - 1].display_line != row.display_line)
                && l.rows
                    .get(r + 1)
                    .is_none_or(|n| n.display_line != row.display_line);
            assert!(alone, "{input:?}");
        }
        // Filler only ends a row, and only a full one.
        if let Some(first) = row.cells.iter().position(|c| *c == Cell::Filler) {
            assert!(
                row.cells[first..].iter().all(|c| *c == Cell::Filler),
                "{input:?}"
            );
            assert_eq!(row.cells.len(), input.text_w, "{input:?}");
        }
        assert_eq!(row.gutter.chars().count(), l.gutter, "{input:?}");
    }
    if input.wrap == Wrap::Off {
        let lines = l.rows.last().unwrap().display_line + 1;
        assert_eq!(l.rows.len(), lines, "{input:?}");
    }
    l.positions.iter().map(|p| (p.pos, p.display_col)).collect()
}
