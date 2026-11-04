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
    let t = ctx.text(b).unwrap();
    let cols: Vec<usize> = [0, 1, 2, 3, 6, 7]
        .iter()
        .map(|p| t.display_col(*p))
        .collect();
    assert_eq!(cols, [0, 1, 8, 9, 11, 12], "a, tab, b, 中, c, end of line");
    assert_eq!(
        t.pos_at_vcol(0, 10),
        3,
        "a column inside a wide char lands on it"
    );
    assert_eq!(t.pos_at_vcol(0, 5), 1, "a column inside a tab lands on it");
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
    let stops: Vec<usize> = t.display_line(0).stops.iter().map(|s| s.pos).collect();
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

fn row_text(t: &TextBuf) -> String {
    let d = t.display_line(0);
    d.rows[0].iter().map(|c| c.text.as_str()).collect()
}

/// A freed slot is reused, so a decoration's id doesn't say when it was added.
#[test]
fn virtual_text_at_one_position_shows_in_the_order_added() {
    let virt = |s: &str| Deco::Virtual(s.into());
    let mut t = TextBuf::new("abc");
    let first = t.decorate(1..1, virt("1")).unwrap();
    t.decorate(1..1, virt("2")).unwrap();
    t.undecorate(first).unwrap();
    t.decorate(1..1, virt("3")).unwrap();
    assert_eq!(row_text(&t), "a23bc");
}

/// Overlapping Hides merge, and among equals the one added first gives the replacement.
#[test]
fn among_equal_hides_the_first_added_gives_the_replacement() {
    let hide = |s: &str| Deco::Hide {
        replacement: Some(s.into()),
        reveal: Reveal::Never,
    };
    let mut t = TextBuf::new("abcd");
    let gone = t.decorate(1..3, hide("x")).unwrap();
    t.decorate(1..3, hide("y")).unwrap();
    t.undecorate(gone).unwrap();
    t.decorate(1..3, hide("z")).unwrap();
    assert_eq!(row_text(&t), "ayd");
}
