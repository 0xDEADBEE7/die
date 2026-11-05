//! Criterion 8 (C16 + layout) and criterion 13 (Hide reveal policy).
mod support;
use die_core::*;
use support::*;

fn lines(n: usize) -> String {
    (0..n).map(|i| format!("l{i}\n")).collect()
}

#[test]
fn j_k_skip_a_fold_made_with_a_stored_hide() {
    let (mut ctx, b, w) = vim_ctx(&lines(10), 20, 10);
    ctx.enable_mode::<Fold>(b).unwrap();
    fold_lines(&mut ctx, b, 3, 5, "+-- 3 lines");
    ctx.type_keys("2jl");
    assert_eq!(lc(&ctx, b, w), (2, 1));
    ctx.type_keys("j");
    assert_eq!(lc(&ctx, b, w), (3, 0), "on the fold line");
    ctx.type_keys("j");
    assert_eq!(
        lc(&ctx, b, w),
        (6, 1),
        "skipped the hidden lines, goal column kept"
    );
    ctx.type_keys("k");
    assert_eq!(lc(&ctx, b, w), (3, 0));
    ctx.type_keys("k");
    assert_eq!(lc(&ctx, b, w), (2, 1));
    let f = screen(&mut ctx, w);
    assert_eq!(&f.rows[..5], ["l0", "l1", "l2", "+-- 3 lines", "l6"]);
    assert_eq!(f.cursor, Some((2, 1)));
    let t = ctx.text(b).unwrap();
    assert_eq!(t.next_visible(2, Dir::Forward), Some(3));
    assert_eq!(t.next_visible(3, Dir::Forward), Some(6));
    assert_eq!(
        t.next_visible(4, Dir::Forward),
        Some(6),
        "a hidden line belongs to the fold's display line"
    );
    assert_eq!(t.next_visible(6, Dir::Backward), Some(3));
    assert_eq!(t.next_visible(0, Dir::Backward), None);
    // dd on the fold line deletes the whole fold, as Vim does on a closed fold.
    ctx.type_keys("jdd");
    assert_eq!(text(&ctx, b), "l0\nl1\nl2\nl6\nl7\nl8\nl9\n");
    // Undo brings the lines back but not the fold: decorations aren't part of undo history.
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), lines(10));
    let f = screen(&mut ctx, w);
    assert_eq!(&f.rows[2..7], ["l2", "l3", "l4", "l5", "l6"]);
}

#[test]
fn gj_gk_over_soft_wrap_with_centered_max_width_and_gutter() {
    let long: String = (0..200).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    let (mut ctx, b, w) = vim_ctx(&format!("{long}\nshort"), 120, 10);
    let s = ctx.text_mut(b).unwrap().settings_mut();
    s.max_width = Some(80);
    s.center = true;
    s.line_numbers = LineNumbers::Absolute;
    let g = ctx.text(b).unwrap().geometry(w).unwrap();
    assert_eq!((g.gutter, g.text_w, g.pad, g.text_x), (4, 80, 18, 22));
    ctx.type_keys("5l");
    ctx.type_keys("gj");
    assert_eq!(cursor(&ctx, b, w), 85);
    assert_eq!(screen(&mut ctx, w).cursor, Some((1, 27)));
    ctx.type_keys("gj");
    assert_eq!(cursor(&ctx, b, w), 165);
    ctx.type_keys("gj");
    assert_eq!(
        lc(&ctx, b, w),
        (1, 5),
        "past the wrapped rows onto the next line"
    );
    ctx.type_keys("gkgkgk");
    assert_eq!(cursor(&ctx, b, w), 5);
    ctx.type_keys("j");
    assert_eq!(
        lc(&ctx, b, w),
        (1, 4),
        "j moves by logical line, not screen row"
    );
    let f = screen(&mut ctx, w);
    let pad = " ".repeat(18);
    assert_eq!(f.rows[0], format!("{pad}  1 {}", &long[..80]));
    assert_eq!(f.rows[1], format!("{pad}    {}", &long[80..160]));
    assert_eq!(f.rows[2], format!("{pad}    {}", &long[160..]));
    assert_eq!(f.rows[3], format!("{pad}  2 short"));
    let vp = ctx.text(b).unwrap().viewport(w).unwrap();
    assert_eq!(vp.rows.len(), 4);
    assert_eq!(
        vp.rows
            .iter()
            .map(|r| (r.first_line, r.row))
            .collect::<Vec<_>>(),
        [(0, 0), (0, 1), (0, 2), (1, 0)]
    );
}

#[test]
fn tabs_and_wide_characters_have_correct_display_columns() {
    let (mut ctx, b, w) = vim_ctx("a\tb中c\nabcdefghi中j\nabcdefgh\tX", 10, 6);
    // S4's rows: character wrap, now that word wrap is the default.
    ctx.text_mut(b).unwrap().settings_mut().wrap = Wrap::Char;
    let t = ctx.text(b).unwrap();
    let cols: Vec<usize> = [0, 1, 2, 3, 6, 7]
        .iter()
        .map(|p| t.display_col(*p))
        .collect();
    assert_eq!(cols, [0, 1, 8, 9, 11, 12], "a, tab, b, 中, c, end of line");
    assert_eq!(
        t.pos_at_vcol(0, 10),
        6,
        "a column inside a wide char lands after it (LAYOUT-4)"
    );
    assert_eq!(
        t.pos_at_vcol(0, 5),
        2,
        "a column inside a tab lands after it (LAYOUT-4)"
    );
    let f = screen(&mut ctx, w);
    assert_eq!(
        f.rows[0], "a       b",
        "中 doesn't fit in the last column: filler, then wrap"
    );
    assert_eq!(f.rows[1], "中c");
    assert_eq!(ctx.text(b).unwrap().screen_pos(w, 3), Some((1, 0)));
    // A wide char that doesn't fit at the end of a row moves to the next row behind a filler cell.
    let l1 = ctx.text(b).unwrap().line_start(1);
    assert_eq!(f.rows[2], "abcdefghi");
    assert_eq!(f.rows[3], "中j");
    assert_eq!(ctx.text(b).unwrap().screen_pos(w, l1 + 9), Some((3, 0)));
    assert_eq!(
        ctx.text(b).unwrap().display_col(l1 + 12),
        11,
        "vcol counts the char, not the filler"
    );
    // A tab splits across the wrap.
    let l2 = ctx.text(b).unwrap().line_start(2);
    assert_eq!(f.rows[5], "      X");
    assert_eq!(ctx.text(b).unwrap().screen_pos(w, l2 + 9), Some((5, 6)));
    assert_eq!(ctx.text(b).unwrap().display_col(l2 + 9), 16);
    ctx.text_mut(b).unwrap().settings_mut().tab_width = 4;
    assert_eq!(ctx.text(b).unwrap().display_col(2), 4);
}

#[test]
fn sub_line_hides_and_virtual_text() {
    // Markdown-style markup hiding, with no replacement, plus an end-of-line hint.
    let (mut ctx, b, w) = vim_ctx("a **b** c\nnext", 40, 4);
    let t = ctx.text_mut(b).unwrap();
    for r in [2..4, 5..7] {
        t.decorate(
            r,
            Deco::Hide {
                replacement: None,
                reveal: Reveal::Remove,
            },
        )
        .unwrap();
    }
    t.decorate(9..9, Deco::Virtual(" [3 tokens]".into()))
        .unwrap();
    let stops: Vec<usize> = t.display_line(0).stops().iter().map(|s| s.pos).collect();
    assert_eq!(stops, [0, 1, 4, 7, 8, 9]);
    assert_eq!(t.display_col(4), 2);
    ctx.type_keys("ll");
    assert_eq!(cursor(&ctx, b, w), 4, "l skips the hidden **");
    ctx.type_keys("l");
    assert_eq!(cursor(&ctx, b, w), 7);
    let f = screen(&mut ctx, w);
    assert_eq!(f.rows[0], "a b c [3 tokens]");
    // Typing inside the hidden range from code: the cursor never ends up inside it.
    let t = ctx.text_mut(b).unwrap();
    t.set_cursor(w, 3);
    assert_eq!(t.cursor(w), Some(4));
}

#[test]
fn edits_and_new_hides_move_the_cursor_out() {
    let (mut ctx, b, w) = vim_ctx(&lines(10), 20, 10);
    ctx.enable_mode::<Fold>(b).unwrap();
    ctx.type_keys("4jl");
    assert_eq!(lc(&ctx, b, w), (4, 1));
    fold_lines(&mut ctx, b, 3, 5, "+--");
    assert_eq!(
        lc(&ctx, b, w),
        (3, 0),
        "folding around the cursor puts it on the fold line"
    );
}

// ---------------------------------------------------------------- criterion 13

#[test]
fn search_into_a_fold_runs_the_owners_reveal_command() {
    let (mut ctx, b, w) = vim_ctx("l0\nl1\nl2\nl3\nfind the needle\nl5\nl6\n", 30, 10);
    ctx.enable_mode::<Fold>(b).unwrap();
    let id = fold_lines(&mut ctx, b, 3, 5, "+-- 3 lines");
    ctx.type_keys("/needle<CR>");
    let (folds, revealed) = ctx
        .with_mode(b, |f: &mut Fold, _| (f.folds.clone(), f.revealed.clone()))
        .unwrap();
    assert_eq!(revealed, [id], "the owner got its decoration id");
    assert!(folds.is_empty(), "and updated its own fold state");
    assert!(ctx.text(b).unwrap().decoration(id).is_none());
    assert_eq!(lc(&ctx, b, w), (4, 9), "cursor on the match");
}

#[test]
fn reveal_never_and_remove() {
    let (mut ctx, b, w) = vim_ctx("l0\nsecret\nl2\n", 30, 10);
    let t = ctx.text_mut(b).unwrap();
    let never = t
        .decorate(
            3..9,
            Deco::Hide {
                replacement: Some("[hidden]".into()),
                reveal: Reveal::Never,
            },
        )
        .unwrap();
    ctx.type_keys("/secret<CR>");
    assert_eq!(
        lc(&ctx, b, w),
        (1, 0),
        "stays hidden; cursor on the replacement"
    );
    assert!(ctx.text(b).unwrap().decoration(never).is_some());
    let t = ctx.text_mut(b).unwrap();
    t.undecorate(never);
    t.set_cursor(w, 0);
    let rm = t
        .decorate(
            3..9,
            Deco::Hide {
                replacement: None,
                reveal: Reveal::Remove,
            },
        )
        .unwrap();
    ctx.type_keys("/cret<CR>");
    assert!(ctx.text(b).unwrap().decoration(rm).is_none());
    assert_eq!(lc(&ctx, b, w), (1, 2));
}

fn editor(text: &str) -> (Ctx, BufferId, WindowId) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 20, 3).unwrap();
    (ctx, b, w)
}

fn row_text(ctx: &mut Ctx, w: WindowId) -> String {
    screen(ctx, w).rows[0].clone()
}

/// A freed slot is reused, so a decoration's id doesn't say when it was added.
#[test]
fn virtual_text_at_one_position_shows_in_the_order_added() {
    let virt = |s: &str| Deco::Virtual(s.into());
    let (mut ctx, b, w) = editor("abc");
    let t = ctx.text_mut(b).unwrap();
    let first = t.decorate(1..1, virt("1")).unwrap();
    t.decorate(1..1, virt("2")).unwrap();
    t.undecorate(first).unwrap();
    t.decorate(1..1, virt("3")).unwrap();
    assert_eq!(row_text(&mut ctx, w), "a23bc");
}

fn hide(replacement: Option<&str>) -> Deco {
    Deco::Hide {
        replacement: replacement.map(Into::into),
        reveal: Reveal::Never,
    }
}

/// Contract §9: where one hidden range ends and another starts, the start's rule applies.
#[test]
fn virtual_text_where_two_hides_touch_follows_the_later_ones_start() {
    for (replacement, want) in [(None, "af"), (Some("R"), "aVRf")] {
        let (mut ctx, b, w) = editor("abcdef");
        let t = ctx.text_mut(b).unwrap();
        t.decorate(1..3, hide(None)).unwrap();
        t.decorate(3..5, hide(replacement)).unwrap();
        t.decorate(3..3, Deco::Virtual("V".into())).unwrap();
        assert_eq!(row_text(&mut ctx, w), want);
    }
}

/// Contract §9: with too few rows it goes as far as there are, and with none it stays.
#[test]
fn move_visual_goes_as_far_as_there_are_rows() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "abc\ndef\n");
    let w = ctx.open_window(b, 10, 5).unwrap();
    let t = ctx.text(b).unwrap();
    assert_eq!(t.move_visual(w, 1, 99, None), Some((8, 1)));
    assert_eq!(t.move_visual(w, 1, -5, None), Some((1, 1)));
    assert_eq!(t.move_visual(w, 8, 1, None), Some((8, 0)));
    assert_eq!(
        t.move_visual(w, 8, 1, Some(7)),
        Some((8, 7)),
        "the goal it used"
    );
}

/// Contract §9: a position that isn't a cursor position answers for the one at or before it,
/// but a cursor left there goes on to the next.
#[test]
fn a_position_that_isnt_a_cursor_position_answers_for_the_one_before_it() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "aae\u{301}xx yy\nbcd");
    let w = ctx.open_window(b, 20, 5).unwrap();
    let t = ctx.text_mut(b).unwrap();
    t.decorate(7..9, hide(None)).unwrap();
    t.decorate(11..13, hide(None)).unwrap();
    assert_eq!(t.screen_pos(w, 3), t.screen_pos(w, 2), "inside a cluster");
    assert_eq!(t.display_col(3), t.display_col(2));
    assert_eq!(t.screen_pos(w, 8), t.screen_pos(w, 6), "inside hidden text");
    assert_eq!(t.display_col(8), t.display_col(6));
    assert_eq!(
        t.screen_pos(w, 12),
        t.screen_pos(w, 13),
        "that line's first"
    );
    assert_eq!(t.display_col(12), t.display_col(13));
    assert_eq!(t.set_cursor(w, 3), Some(5));
    assert_eq!(t.set_cursor(w, 8), Some(9));
}

#[test]
fn an_empty_replacement_is_no_replacement() {
    let (mut ctx, b, w) = editor("abc");
    let id = ctx
        .text_mut(b)
        .unwrap()
        .decorate(1..2, hide(Some("")))
        .unwrap();
    assert_eq!(row_text(&mut ctx, w), "ac");
    let stored = ctx.text(b).unwrap().decoration(id).unwrap();
    assert!(matches!(
        &stored.deco,
        Deco::Hide {
            replacement: None,
            ..
        }
    ));
}

/// Overlapping Hides merge, and among equals the one added first gives the replacement.
#[test]
fn among_equal_hides_the_first_added_gives_the_replacement() {
    let hide = |s: &str| Deco::Hide {
        replacement: Some(s.into()),
        reveal: Reveal::Never,
    };
    let (mut ctx, b, w) = editor("abcd");
    let t = ctx.text_mut(b).unwrap();
    let gone = t.decorate(1..3, hide("x")).unwrap();
    t.decorate(1..3, hide("y")).unwrap();
    t.undecorate(gone).unwrap();
    t.decorate(1..3, hide("z")).unwrap();
    assert_eq!(row_text(&mut ctx, w), "ayd");
}

// ---------------------------------------------------------------- K4
//
// K4's test plan: besides these, `emacs_layout.rs` holds tty Emacs's answers, `prop.rs` the
// property against the naive layout, `naive_layout.rs` §9's worked examples, `layout_speed.rs` the
// speed budget, and `scene.rs` what's drawn.

fn window(text: &str, cols: usize, rows: usize) -> (Ctx, BufferId, WindowId) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, cols, rows).unwrap();
    ctx.focus(w);
    (ctx, b, w)
}

fn numbers(ctx: &mut Ctx, b: BufferId, w: WindowId) -> Vec<Option<usize>> {
    screen(ctx, w);
    let vp = ctx.text(b).unwrap().viewport(w).unwrap();
    vp.rows.iter().map(|r| r.number).collect()
}

/// WRAP-2: as in Emacs. S4 gave a full line a blank row after it, and `gj` stopped there.
#[test]
fn an_exactly_full_line_gets_no_blank_row() {
    let (mut ctx, b, w) = window("aaaaaaaaaa\nb\n", 10, 5);
    let f = screen(&mut ctx, w);
    assert_eq!(f.rows[..4], ["aaaaaaaaaa", "b", "", ""]);
    let t = ctx.text(b).unwrap();
    assert_eq!(
        t.screen_pos(w, 10),
        Some((0, 10)),
        "just past the row's last cell"
    );
    assert_eq!(t.move_visual(w, 0, 1, None), Some((11, 0)), "gj goes to b");
    let vp = t.viewport(w).unwrap();
    assert_eq!(vp.rows.len(), 3, "the full line, b and the empty last row");
}

/// dev-4: the row counts for scrolling and window positions, and it has no number.
#[test]
fn the_empty_line_after_a_final_newline_has_a_row_and_no_number() {
    let (mut ctx, b, w) = window("one\ntwo\n", 10, 5);
    ctx.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    let t = ctx.text_mut(b).unwrap();
    assert_eq!(t.set_cursor(w, 8), Some(8));
    assert_eq!(
        t.screen_pos(w, 8),
        Some((2, 4)),
        "the cursor can be drawn there"
    );
    assert_eq!(numbers(&mut ctx, b, w), [Some(1), Some(2), None]);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Visual;
    assert_eq!(
        numbers(&mut ctx, b, w),
        [Some(2), Some(1), None],
        "it still counts"
    );
    let t = ctx.text_mut(b).unwrap();
    t.tx(Origin::User).delete(0..8).commit().unwrap();
    assert_eq!(numbers(&mut ctx, b, w), [None], "an empty text's only row");
}

/// LAYOUT-3: the gutter is sized by the lines that can have a number.
#[test]
fn the_gutter_counts_lines_not_the_empty_last_one() {
    for (text, gutter) in [
        (String::new(), 4),
        ("x\n".repeat(999), 4),
        (format!("{}x", "x\n".repeat(999)), 5),
        ("x\n".repeat(1000), 5),
    ] {
        let (mut ctx, b, w) = window(&text, 20, 5);
        ctx.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Relative;
        let g = ctx.text(b).unwrap().geometry(w).unwrap();
        assert_eq!(g.gutter, gutter, "{} bytes", text.len());
    }
}

/// §9's table, with its text and width, in each style and with the cursor where it says.
#[test]
fn line_numbers_match_the_contracts_table() {
    let text = "one\na line that wraps\nthree\nfour\n";
    let (mut ctx, b, w) = window(text, 16, 8); // 12 text cells, beside a gutter of 4.
    let (three, wraps, empty) = (22, 16, text.len());
    let n = |s: [usize; 5]| {
        let mut v = vec![Some(s[0]), Some(s[1]), None, Some(s[2]), Some(s[3]), None];
        v[2] = (s[4] > 0).then_some(s[4]);
        v
    };
    for (cursor, absolute, relative, visual) in [
        (three, [1, 2, 3, 4, 0], [2, 1, 3, 1, 0], [3, 2, 3, 1, 1]),
        (wraps, [1, 2, 3, 4, 0], [1, 2, 1, 2, 0], [2, 1, 1, 2, 2]),
        (empty, [1, 2, 3, 4, 0], [4, 3, 2, 1, 0], [5, 4, 2, 1, 3]),
    ] {
        for (style, want) in [
            (LineNumbers::Absolute, n(absolute)),
            (LineNumbers::Relative, n(relative)),
            (LineNumbers::Visual, n(visual)),
        ] {
            let t = ctx.text_mut(b).unwrap();
            t.settings_mut().line_numbers = style;
            t.set_cursor(w, cursor);
            assert_eq!(
                numbers(&mut ctx, b, w),
                want,
                "{style:?}, cursor at {cursor}"
            );
        }
    }
}

/// §9: with lines a to e, b to d folded, and the cursor on a. A fold's row counts once.
#[test]
fn a_fold_counts_once_in_relative_and_visual_numbers() {
    let (mut ctx, b, w) = window("a\nb\nc\nd\ne", 10, 5);
    for repl in [None, Some("fold")] {
        let t = ctx.text_mut(b).unwrap();
        let id = t.decorate(2..7, hide(repl)).unwrap();
        for (style, want) in [
            (LineNumbers::Absolute, [1, 2, 5]),
            (LineNumbers::Relative, [1, 1, 2]),
            (LineNumbers::Visual, [1, 1, 2]),
        ] {
            ctx.text_mut(b).unwrap().settings_mut().line_numbers = style;
            assert_eq!(
                numbers(&mut ctx, b, w),
                want.map(Some),
                "{style:?}, {repl:?}"
            );
        }
        ctx.text_mut(b).unwrap().undecorate(id);
    }
}

#[test]
fn numbers_for_a_cursor_off_screen_are_still_its_distance() {
    let text = "x\n".repeat(30);
    let (mut ctx, b, w) = window(&text, 20, 3);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Relative;
    t.set_scroll(w, 20, 0).unwrap(); // Line 10 is the window's first, and the cursor is on line 0.
    let num = |t: &TextBuf| -> Vec<Option<usize>> {
        t.viewport(w)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.number)
            .collect()
    };
    assert_eq!(
        num(t),
        [Some(10), Some(11), Some(12)],
        "the cursor above the window"
    );
    t.settings_mut().line_numbers = LineNumbers::Visual;
    assert_eq!(num(t), [Some(10), Some(11), Some(12)]);
    t.set_cursor(w, 40); // Line 20.
    t.settings_mut().line_numbers = LineNumbers::Relative;
    assert_eq!(num(t), [Some(10), Some(9), Some(8)], "the cursor below it");
    t.settings_mut().line_numbers = LineNumbers::Visual;
    assert_eq!(num(t), [Some(10), Some(9), Some(8)]);
}

/// LAYOUT-4: Evil's `j` and `k` land after what spans the goal, and a fold's display line counts
/// as one, so the column can land on a later line of it.
#[test]
fn pos_at_vcol_lands_after_what_spans_the_goal() {
    let (mut ctx, b, _) = window("a\tb\na中b\nab\ncdef\nxy", 20, 5);
    let t = ctx.text_mut(b).unwrap();
    assert_eq!(t.pos_at_vcol(0, 0), 0);
    assert_eq!(t.pos_at_vcol(0, 1), 1, "the tab's first column");
    assert_eq!(t.pos_at_vcol(0, 3), 2, "inside the tab: after it");
    assert_eq!(t.pos_at_vcol(0, 99), 3, "past the line: its end");
    assert_eq!(
        t.pos_at_vcol(1, 2),
        8,
        "inside the wide character: after it"
    );
    t.decorate(12..13, hide(None)).unwrap(); // ab's newline.
    assert_eq!(t.pos_at_vcol(2, 2), 13, "a joined line's column runs on");
    assert_eq!(
        t.pos_at_vcol(3, 0),
        10,
        "any line of it asks for the display line"
    );
}

/// LAYOUT-5: Off and Char, against Word's row.
#[test]
fn wrap_off_word_and_char_each_break_as_they_say() {
    let text = "aaaa bbbbb cccc\n";
    for (wrap, want) in [
        (Wrap::Word, vec!["aaaa", "bbbbb cccc", ""]),
        (Wrap::Char, vec!["aaaa bbbbb", " cccc", ""]),
        (Wrap::Off, vec!["aaaa bbbbb", ""]),
    ] {
        let (mut ctx, b, w) = window(text, 10, 3);
        ctx.text_mut(b).unwrap().settings_mut().wrap = wrap;
        let f = screen(&mut ctx, w);
        assert_eq!(f.rows[..want.len()], want[..], "{wrap:?}");
    }
    assert_eq!(Settings::default().wrap, Wrap::Word);
}

/// LAYOUT-6: V6's scrolling keys read and set this.
#[test]
fn scroll_reads_and_sets_a_windows_first_row() {
    let long = "x".repeat(25);
    let (mut ctx, b, w) = window(&format!("{long}\nb\nc\nd\n"), 10, 2);
    let t = ctx.text_mut(b).unwrap();
    assert_eq!(t.scroll(w), Some((0, 0)));
    assert_eq!(t.set_scroll(w, 5, 1), Some(()));
    assert_eq!(
        t.scroll(w),
        Some((0, 1)),
        "top moves to its display line's start"
    );
    let vp = t.viewport(w).unwrap();
    assert_eq!((vp.top, vp.top_row, vp.rows[0].row), (0, 1, 1));
    t.set_scroll(w, 0, 99).unwrap();
    assert_eq!(
        t.scroll(w),
        Some((0, 2)),
        "the row stops at the line's last"
    );
    t.set_scroll(w, 26, 0).unwrap();
    assert_eq!(t.scroll(w), Some((26, 0)));
    let other = WindowId::default();
    assert_eq!(t.scroll(other), None);
    assert_eq!(t.set_scroll(other, 0, 0), None);
}

/// §9: a command that scrolls also leaves the cursor on a visible row, or core scrolls back.
#[test]
fn core_scrolls_back_to_the_cursor_before_a_render() {
    let (mut ctx, b, w) = window("a\nb\nc\nd\ne\nf\n", 10, 2);
    let t = ctx.text_mut(b).unwrap();
    t.set_scroll(w, 6, 0).unwrap();
    assert_eq!(t.screen_pos(w, 0), None, "the cursor is off-screen for now");
    let f = screen(&mut ctx, w);
    assert_eq!(f.rows, ["a", "b"]);
    let t = ctx.text_mut(b).unwrap();
    t.set_scroll(w, 2, 0).unwrap();
    t.set_cursor(w, 4);
    let f = screen(&mut ctx, w);
    assert_eq!(
        f.rows,
        ["b", "c"],
        "the cursor's still on a visible row: it stays"
    );
}

/// LAYOUT-7: what die-vim and H, M and L read, through methods.
#[test]
fn a_display_line_and_a_view_row_are_read_through_methods() {
    let (ctx, b, w) = window("ab cd\nef\n", 3, 5);
    let t = ctx.text(b).unwrap();
    let d = t.display_line(1);
    assert_eq!(
        (d.start(), d.end(), d.lines(), d.eof()),
        (0, 6, 0..=0, false)
    );
    let stops: Vec<(usize, usize, usize)> =
        d.stops().iter().map(|s| (s.pos, s.vcol, s.col)).collect();
    assert_eq!(stops[..3], [(0, 0, 0), (1, 1, 1), (2, 2, 2)]);
    assert_eq!(stops[5], (5, 5, 5), "unwrapped: the newline's column");
    let last = t.display_line(9);
    assert_eq!((last.start(), last.end(), last.eof()), (9, 9, true));
    let vp = t.viewport(w).unwrap();
    let row: Vec<(usize, usize, usize)> = vp.rows[1]
        .stops()
        .iter()
        .map(|s| (s.pos, s.row, s.col))
        .collect();
    assert_eq!(
        row,
        [(3, 1, 0), (4, 1, 1), (5, 1, 2)],
        "wrapped: this row's"
    );
}

#[test]
fn a_tall_file_holds_more_lines_than_the_cache_does() {
    let text: String = (0..2000)
        .map(|i| format!("{i:04} words on a line\n"))
        .collect();
    let (mut ctx, b, w) = window(&text, 10, 4);
    let t = ctx.text_mut(b).unwrap();
    let stride = 21;
    for line in 0..2000 {
        assert_eq!(t.screen_pos(w, t.line_start(line)).is_some(), line < 2);
        assert_eq!(t.display_col(t.line_start(line) + 5), 5);
        assert_eq!(t.line_start(line), line * stride);
    }
    t.tx(Origin::User).insert(0, "x").commit().unwrap();
    assert_eq!(t.display_col(t.line_start(1999) + 5), 5);
}

/// A plan row: `toggle-max-width`, your `visual-fill-column-mode` on `\z`.
#[test]
fn toggle_max_width_turns_the_width_off_and_back_on() {
    let (mut ctx, b, w) = window("text\n", 120, 5);
    let toggle = |ctx: &mut Ctx| ctx.run("toggle-max-width", &Args::default()).unwrap();
    let width = |ctx: &Ctx| ctx.text(b).unwrap().settings().max_width;
    let text_w = |ctx: &Ctx| ctx.text(b).unwrap().geometry(w).unwrap().text_w;
    toggle(&mut ctx);
    assert_eq!(
        (width(&ctx), text_w(&ctx)),
        (Some(80), 80),
        "80 if it never had one"
    );
    ctx.text_mut(b).unwrap().settings_mut().max_width = Some(100);
    toggle(&mut ctx);
    assert_eq!((width(&ctx), text_w(&ctx)), (None, 120), "off");
    toggle(&mut ctx);
    assert_eq!(
        (width(&ctx), text_w(&ctx)),
        (Some(100), 100),
        "the width it had"
    );
    // It acts on the window a command was run for.
    let other = ctx.create_text("other", "");
    let ow = ctx.open_window(other, 90, 5).unwrap();
    ctx.focus(w);
    let args = Args {
        window: Some(ow),
        ..Default::default()
    };
    ctx.run("toggle-max-width", &args).unwrap();
    assert_eq!(ctx.text(other).unwrap().settings().max_width, Some(80));
    assert_eq!(width(&ctx), Some(100), "not the focused one");
    assert!(
        ctx.command_doc("toggle-max-width")
            .is_some_and(|(_, owner)| owner == "core")
    );
    // With nothing to act on, it says so.
    let mut empty = Ctx::new(vec![]).unwrap();
    assert!(matches!(
        empty.run("toggle-max-width", &Args::default()),
        Err(Error::Msg(_))
    ));
}

/// A plain string stays one call, and the faces are the caller's.
#[test]
fn styled_text_comes_from_a_plain_string() {
    let s: Styled = "abc".into();
    assert_eq!((s.text.as_str(), s.faces.len()), ("abc", 0));
    let s: Styled = String::from("def").into();
    assert_eq!(
        s,
        Styled {
            text: "def".into(),
            faces: vec![]
        }
    );
    let (mut ctx, b, w) = window("abc", 20, 2);
    let t = ctx.text_mut(b).unwrap();
    let styled = Styled {
        text: "x\ty\nz".into(),
        faces: vec![(0..1, "bold")],
    };
    let id = t.decorate(1..1, Deco::Virtual(styled)).unwrap();
    let Deco::Virtual(v) = &t.decoration(id).unwrap().deco else {
        panic!("what was stored")
    };
    assert_eq!(v.text, "x y z", "one line, as a replacement is");
    assert_eq!(v.faces, [(0..1, "bold")]);
    assert_eq!(screen(&mut ctx, w).rows[0], "ax y zbc");
}
