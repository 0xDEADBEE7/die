//! What the oracle's geometry cases don't reach: a motion that leaves the window, the text's end
//! under `C-f` and `C-e`, and a wrapped line scrolled past its first row.

use die_test_cap as _;

use die_core::{Harness, WindowId};
use die_vim::Vim;

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i:02}\n")).collect()
}

fn open(text: &str) -> (Harness, die_core::BufferId, WindowId) {
    let mut ctx = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    ctx.set_window_size(40, 10);
    let (b, w) = ctx.open_text("t", text).expect("a window");
    (ctx, b, w)
}

fn errors(ctx: &mut Harness, keys: &str) -> Vec<String> {
    let log = ctx.type_keys(keys).expect("keys parse");
    log.into_iter().flat_map(|k| k.errors).collect()
}

#[test]
fn a_motion_off_the_bottom_recentres_as_your_emacs_does() {
    let (mut ctx, b, w) = open(&lines(40));
    errors(&mut ctx, "9j");
    assert_eq!(ctx.top_line(w), Some(0));
    errors(&mut ctx, "j");
    let line = ctx
        .text(b)
        .unwrap()
        .line_of(ctx.text(b).unwrap().cursor(w).unwrap());
    assert_eq!((line, ctx.top_line(w)), (10, Some(5)));
}

#[test]
fn c_f_past_the_end_goes_to_the_last_line_then_fails() {
    let (mut ctx, b, w) = open(&lines(10));
    assert!(errors(&mut ctx, "<C-f>").is_empty());
    assert!(errors(&mut ctx, "<C-f>").is_empty());
    let line = ctx
        .text(b)
        .unwrap()
        .line_of(ctx.text(b).unwrap().cursor(w).unwrap());
    assert_eq!(line, 9, "the last line, not the empty row after it");
    assert_eq!(errors(&mut ctx, "<C-f>").len(), 1);
}

#[test]
fn a_c_e_count_past_the_end_fails_without_scrolling() {
    let (mut ctx, b, w) = open(&lines(20));
    assert_eq!(errors(&mut ctx, "99<C-e>").len(), 1);
    assert_eq!(ctx.top_line(w), Some(0));
    assert_eq!(errors(&mut ctx, "19<C-e>"), Vec::<String>::new());
    assert_eq!(ctx.top_line(w), Some(19));
    let line = ctx
        .text(b)
        .unwrap()
        .line_of(ctx.text(b).unwrap().cursor(w).unwrap());
    assert_eq!(line, 19);
    assert_eq!(errors(&mut ctx, "<C-e>").len(), 1);
}

/// A column that isn't on the row the window now starts with must not bring the view back.
#[test]
fn c_e_into_a_wrapped_line_leaves_the_view_there() {
    let text = format!("{}\nafter\n", "a".repeat(100));
    let (mut ctx, b, w) = open(&text);
    errors(&mut ctx, "3l<C-e>");
    let t = ctx.text(b).unwrap();
    assert_eq!(t.cursor(w), Some(40));
    assert_eq!(t.scroll(w), Some((0, 1)));
}

/// Short of the count is fine once the cursor moved; the last row itself still fails.
#[test]
fn a_gj_count_that_moves_part_of_the_way_succeeds() {
    let (mut ctx, b, w) = open("short\ntail\n");
    assert_eq!(errors(&mut ctx, "9gj"), Vec::<String>::new());
    let t = ctx.text(b).unwrap();
    assert_eq!(t.line_of(t.cursor(w).unwrap()), 1);
    assert_eq!(errors(&mut ctx, "9gj").len(), 1);
}
