//! V4 where the Evil oracle can't reach: what the 67 oracle cases don't say.
//!
//! Test plan. The oracle holds die-vim to your Evil on visual state, `.`, macros and `R`.
//! These hold the rest:
//!
//! - **The selection as core keeps it**, since a host draws it: `<C-v>$` sets the goal the
//!   scene reads (TEXT-3), `v` and Escape put back a plain cursor.
//! - **`@` on text `q` didn't record** types it literally (KEYS-2), and what `q` records is
//!   marked as keys, whatever's in it.
//! - **A macro is one undo step**, as `evil-with-single-undo` makes it, and the step ends
//!   when keys are typed again, even after a macro that failed, or a buffer that went away.
//! - **A macro that calls itself forever ends**, with an error. Emacs ends it when its Lisp
//!   nesting runs out.
//! - **`gv` follows the text** as it's edited, as your Evil's markers do.
//! - **Two buffers**: `q` stops in a buffer it didn't start in, and a selection in one
//!   leaves the other alone.
//! - **`.`'s own rules**: `;` keeps the search from before it, a count lasts for one `.`,
//!   and `.` in visual state isn't bound.
//! - **Text a block crosses**: wide characters and tabs, which have no oracle case.
//! - **Failure leaves things sane**: read-only text in visual state and replace state.
//! - **Nothing panics**, whatever the keys, with or without a Hide in the text, and the
//!   selection stays inside it. `DIE_PROP_CASES` says how many runs. V3's merge found a panic
//!   only a fold could reach.

use die_test_cap as _;

use die_core::{BufferId, Ctx, Deco, RegKind, Register, Reveal, Shape, State, WindowId};
use die_vim::Vim;

fn editor(text: &str) -> (Ctx, BufferId, WindowId) {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 80, 24).expect("a window");
    ctx.focus(w);
    (ctx, b, w)
}

fn text(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).expect("a text buffer").rope().to_string()
}

fn cursor(ctx: &Ctx, b: BufferId, w: WindowId) -> usize {
    ctx.text(b).unwrap().cursor(w).unwrap()
}

/// The text after these keys.
fn after(start: &str, keys: &str) -> String {
    let (mut ctx, b, _) = editor(start);
    ctx.type_keys(keys);
    text(&ctx, b)
}

fn chars(text: &str) -> Register {
    Register {
        kind: RegKind::Char,
        text: text.into(),
    }
}

// ---------------------------------------------------------------- the selection

#[test]
fn v_and_escape_leave_a_plain_cursor() {
    let (mut ctx, b, w) = editor("abc\n");
    ctx.type_keys("vl");
    let sel = ctx.text(b).unwrap().selection(w).unwrap();
    assert_eq!((sel.anchor, sel.head, sel.shape), (0, 1, Shape::Char));
    assert_eq!(ctx.state(), State::Visual);
    ctx.type_keys("<Esc>");
    let sel = ctx.text(b).unwrap().selection(w).unwrap();
    assert_eq!((sel.anchor, sel.head, sel.shape), (1, 1, Shape::Char));
    assert_eq!(ctx.state(), State::Normal);
}

#[test]
fn the_same_key_again_stops_and_another_changes_the_shape() {
    let (mut ctx, b, w) = editor("abc\ndef\n");
    ctx.type_keys("vj");
    ctx.type_keys("V");
    assert_eq!(
        ctx.text(b).unwrap().selection(w).unwrap().shape,
        Shape::Line
    );
    assert_eq!(ctx.state(), State::Visual);
    ctx.type_keys("V");
    assert_eq!(ctx.state(), State::Normal);
}

#[test]
fn dollar_in_a_block_asks_for_each_lines_end() {
    let (mut ctx, b, w) = editor("ab\nabcd\n");
    ctx.type_keys("<C-v>j$");
    let sel = ctx.text(b).unwrap().selection(w).unwrap();
    assert_eq!(sel.goal, Some(usize::MAX), "what the scene reads");
    assert_eq!(sel.head, 7, "on the newline: visual state can rest there");
    ctx.type_keys("d");
    assert_eq!(text(&ctx, b), "\n\n");
}

#[test]
fn a_motion_can_rest_on_a_newline_only_in_visual_state() {
    let (mut ctx, b, w) = editor("abc\n");
    ctx.type_keys("$");
    assert_eq!(cursor(&ctx, b, w), 2);
    ctx.type_keys("v$");
    assert_eq!(cursor(&ctx, b, w), 3);
    ctx.type_keys("<Esc>");
    assert_eq!(cursor(&ctx, b, w), 2, "and Escape steps back off it");
}

// ---------------------------------------------------------------- what @ types

#[test]
fn at_types_text_q_didnt_record_literally() {
    let (mut ctx, b, _) = editor("x\n");
    ctx.set_register('a', chars("iVec<String>"));
    ctx.type_keys("@a<Esc>");
    assert_eq!(text(&ctx, b), "Vec<String>x\n");
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn at_doesnt_press_keys_named_in_plain_text() {
    let (mut ctx, b, _) = editor("x\n");
    ctx.set_register('a', chars("i<Esc>"));
    ctx.type_keys("@a<Esc>");
    assert_eq!(text(&ctx, b), "<Esc>x\n");
}

#[test]
fn q_marks_what_it_records_as_keys() {
    let (mut ctx, _, _) = editor("x\n");
    ctx.type_keys("qaia<Esc>q");
    assert_eq!(
        ctx.register('a'),
        Some(&Register {
            kind: RegKind::Macro,
            text: "ia<Esc>".into()
        })
    );
}

#[test]
fn yanking_over_a_recording_makes_it_text_again() {
    let (mut ctx, _, _) = editor("x\n");
    ctx.type_keys("qaia<Esc>q\"ayl");
    assert_eq!(ctx.register('a').map(|r| r.kind), Some(RegKind::Char));
}

#[test]
fn an_upper_case_q_appends_to_the_recording() {
    let (mut ctx, _, _) = editor("x\n");
    ctx.type_keys("qaxqqAxq");
    assert_eq!(ctx.register('a').map(|r| r.text.as_str()), Some("xx"));
    assert_eq!(ctx.register('a').map(|r| r.kind), Some(RegKind::Macro));
}

#[test]
fn an_empty_register_fails_unless_recording() {
    let (mut ctx, b, _) = editor("x\n");
    ctx.type_keys("@a");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    ctx.messages.clear();
    ctx.type_keys("qb@zxq");
    assert_eq!(ctx.messages, Vec::<String>::new());
    assert_eq!(text(&ctx, b), "\n");
}

#[test]
fn at_at_with_no_macro_yet_fails() {
    let (mut ctx, _, _) = editor("x\n");
    ctx.type_keys("@@");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn a_macro_register_must_be_a_letter_or_a_digit() {
    let (mut ctx, _, _) = editor("x\n");
    ctx.type_keys("q!");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal, "and q has stopped waiting");
    ctx.messages.clear();
    ctx.type_keys("q<Esc>");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal);
}

// ---------------------------------------------------------------- undo of a macro

#[test]
fn a_macro_is_one_undo_step() {
    let (mut ctx, b, _) = editor("abcd\n");
    ctx.type_keys("qaxxq");
    assert_eq!(text(&ctx, b), "cd\n");
    ctx.type_keys("u");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "abcd\n", "typed, so two steps");
    ctx.type_keys("@a");
    assert_eq!(text(&ctx, b), "cd\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "abcd\n", "@a is one");
}

#[test]
fn the_step_ends_when_keys_are_typed_again() {
    let (mut ctx, b, _) = editor("abcd\n");
    ctx.set_register(
        'a',
        Register {
            kind: RegKind::Macro,
            text: "x".into(),
        },
    );
    ctx.type_keys("@ax");
    assert_eq!(text(&ctx, b), "cd\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "bcd\n", "only the typed x");
}

#[test]
fn a_failed_macro_still_ends_its_step() {
    let (mut ctx, b, _) = editor("abcd\n");
    ctx.set_register(
        'a',
        Register {
            kind: RegKind::Macro,
            text: "xkx".into(),
        },
    );
    ctx.type_keys("@a");
    assert_eq!(text(&ctx, b), "bcd\n", "k fails on the first line");
    ctx.type_keys("x");
    assert_eq!(text(&ctx, b), "cd\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "bcd\n");
}

#[test]
fn a_buffer_killed_with_a_macros_step_open_is_no_trouble() {
    let (mut ctx, b, _) = editor("abcd\n");
    let w1 = ctx.focused().unwrap();
    let other = ctx.create_text("u", "efg\n");
    let w = ctx.open_window(other, 80, 24).expect("a window");
    ctx.set_register(
        'a',
        Register {
            kind: RegKind::Macro,
            text: "x".into(),
        },
    );
    ctx.focus(w1);
    ctx.type_keys("@a");
    assert!(ctx.kill(b));
    ctx.run_until_idle();
    ctx.focus(w);
    ctx.type_keys("x");
    assert_eq!(text(&ctx, other), "fg\n");
}

#[test]
fn a_macro_that_calls_itself_forever_ends_with_an_error() {
    let (mut ctx, _, _) = editor("abcd\n");
    ctx.set_register(
        'a',
        Register {
            kind: RegKind::Macro,
            text: "lh@a".into(),
        },
    );
    ctx.type_keys("@a");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert!(ctx.messages[0].contains("macro"), "{:?}", ctx.messages);
}

// ---------------------------------------------------------------- gv

#[test]
fn gv_follows_the_text() {
    let (mut ctx, b, w) = editor("abc def\n");
    ctx.type_keys("wve<Esc>");
    ctx.type_keys("0ixx<Esc>");
    assert_eq!(text(&ctx, b), "xxabc def\n");
    ctx.type_keys("gv");
    let sel = ctx.text(b).unwrap().selection(w).unwrap();
    assert_eq!((sel.anchor, sel.head), (6, 8), "still on def");
    assert_eq!(ctx.state(), State::Visual);
}

#[test]
fn gv_with_no_selection_yet_or_in_visual_state_does_nothing() {
    let (mut ctx, b, w) = editor("abc\n");
    ctx.type_keys("lgv");
    assert_eq!(ctx.state(), State::Normal);
    ctx.type_keys("vlgv");
    let sel = ctx.text(b).unwrap().selection(w).unwrap();
    assert_eq!((sel.anchor, sel.head), (1, 2));
    assert_eq!(ctx.state(), State::Visual);
}

#[test]
fn killing_a_buffer_forgets_its_selection() {
    let (mut ctx, b, _) = editor("abc\n");
    ctx.type_keys("v<Esc>");
    assert!(ctx.kill(b));
    ctx.run_until_idle();
    assert_eq!(ctx.messages, Vec::<String>::new());
}

// ---------------------------------------------------------------- two buffers

#[test]
fn q_stops_in_a_buffer_it_didnt_start_in() {
    let (mut ctx, _, w1) = editor("abc\n");
    let other = ctx.create_text("u", "efg\n");
    let w2 = ctx.open_window(other, 80, 24).expect("a window");
    ctx.focus(w1);
    ctx.type_keys("qax");
    ctx.focus(w2);
    ctx.type_keys("xq");
    assert_eq!(ctx.register('a').map(|r| r.text.as_str()), Some("xx"));
    assert_eq!(ctx.state(), State::Normal);
}

#[test]
fn a_selection_in_one_buffer_leaves_another_alone() {
    let (mut ctx, b, w1) = editor("abc\n");
    let other = ctx.create_text("u", "efg\n");
    let w2 = ctx.open_window(other, 80, 24).expect("a window");
    ctx.focus(w1);
    ctx.type_keys("v");
    ctx.focus(w2);
    assert_eq!(ctx.state(), State::Normal);
    assert_eq!(ctx.state_of(b), State::Visual);
    ctx.type_keys("x");
    assert_eq!(text(&ctx, other), "fg\n");
    assert_eq!(text(&ctx, b), "abc\n");
}

// ---------------------------------------------------------------- .

#[test]
fn dot_keeps_the_search_semicolon_repeats() {
    let (mut ctx, b, _) = editor("axbxcydxey\n");
    ctx.type_keys("fy");
    ctx.type_keys("0dfx");
    assert_eq!(text(&ctx, b), "bxcydxey\n");
    ctx.type_keys("0fy");
    ctx.type_keys("0.");
    assert_eq!(text(&ctx, b), "cydxey\n");
    ctx.type_keys("0;");
    assert_eq!(cursor(&ctx, b, ctx.focused().unwrap()), 1, "y, not x");
}

#[test]
fn dot_isnt_bound_in_visual_state() {
    let (mut ctx, b, _) = editor("abcd\n");
    ctx.type_keys("xv.");
    assert_eq!(text(&ctx, b), "bcd\n");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn dot_with_nothing_to_repeat_fails() {
    let (mut ctx, _, _) = editor("abcd\n");
    ctx.type_keys(".");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn a_change_that_failed_isnt_what_dot_repeats() {
    let (mut ctx, b, _) = editor("abc\n");
    ctx.type_keys("x");
    ctx.type_keys("dk");
    assert_eq!(text(&ctx, b), "bc\n", "dk fails on the first line");
    ctx.messages.clear();
    ctx.type_keys(".");
    assert_eq!(text(&ctx, b), "c\n", "x again");
}

#[test]
fn an_operator_cancelled_isnt_what_dot_repeats() {
    assert_eq!(after("abc def\n", "xd<Esc>."), "c def\n");
}

// ---------------------------------------------------------------- what a block crosses

#[test]
fn a_block_takes_a_tab_it_touches_whole() {
    assert_eq!(after("a\tb\ncdefghij\n", "l<C-v>jd"), "ab\ncefghij\n");
}

#[test]
fn a_block_register_holds_a_tab_as_spaces() {
    let (mut ctx, _, _) = editor("a\tb\ncdefghij\n");
    ctx.type_keys("l<C-v>jly");
    let held = ctx.register('"').cloned().expect("a yank");
    assert_eq!(held.kind, RegKind::Block);
    assert_eq!(held.text, "       \nde", "the tab it touches, whole");
}

#[test]
fn a_block_register_pads_a_short_line() {
    let (mut ctx, _, _) = editor("abcd\na\nabcd\n");
    ctx.type_keys("ll<C-v>jjly");
    assert_eq!(
        ctx.register('"').map(|r| r.text.as_str()),
        Some("cd\n  \ncd")
    );
}

#[test]
fn a_block_goes_across_a_wide_character_by_its_columns() {
    assert_eq!(after("世界x\nabcde\n", "<C-v>jld"), "界x\ncde\n");
    assert_eq!(
        after("ab\n世界\n", "l<C-v>jd"),
        "a\n\n",
        "j lands past a wide character whose column it wanted, so the block reaches that one"
    );
}

#[test]
fn a_block_paste_adds_the_lines_it_needs() {
    let (mut ctx, b, _) = editor("ab\ncd\nxy");
    ctx.type_keys("<C-v>jly");
    ctx.type_keys("jj$p");
    assert_eq!(text(&ctx, b), "ab\ncd\nxyab\n  cd");
}

#[test]
fn a_block_paste_leaves_a_short_line_alone_if_it_has_nothing_to_add() {
    let (mut ctx, b, _) = editor("abc\n\nabc\nxxxx\n\nxxxx\n");
    ctx.type_keys("l<C-v>jjy");
    assert_eq!(ctx.register('"').map(|r| r.text.as_str()), Some("b\n \nb"));
    ctx.type_keys("jjjllp");
    assert_eq!(text(&ctx, b), "abc\n\nabc\nxxxxb\n\nxxxxb\n");
}

// ---------------------------------------------------------------- failure

#[test]
fn a_read_only_buffer_refuses_and_leaves_visual_state() {
    let (mut ctx, b, _) = editor("abc\n");
    ctx.text_mut(b).unwrap().settings_mut().read_only = true;
    ctx.type_keys("vld");
    assert_eq!(text(&ctx, b), "abc\n");
    assert_eq!(ctx.state(), State::Normal);
}

#[test]
fn replace_state_backspace_goes_back_past_where_it_began() {
    let (mut ctx, b, w) = editor("abc\n");
    ctx.type_keys("lR<BS>");
    assert_eq!(cursor(&ctx, b, w), 0, "nothing to put back: it only moves");
    assert_eq!(text(&ctx, b), "abc\n");
    ctx.type_keys("<BS>");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn replace_state_takes_a_whole_grapheme_and_puts_it_back() {
    let (mut ctx, b, _) = editor("e\u{301}x\n");
    ctx.type_keys("Rz");
    assert_eq!(text(&ctx, b), "zx\n");
    ctx.type_keys("<BS>");
    assert_eq!(text(&ctx, b), "e\u{301}x\n");
}

#[test]
fn replace_state_undoes_a_newline_too() {
    let (mut ctx, b, _) = editor("abc\n");
    ctx.type_keys("Rx<CR>");
    assert_eq!(text(&ctx, b), "x\nbc\n");
    ctx.type_keys("<BS>");
    assert_eq!(text(&ctx, b), "xbc\n");
    ctx.type_keys("<BS>");
    assert_eq!(text(&ctx, b), "abc\n");
}

// ---------------------------------------------------------------- any keys at all

/// Keys of every kind this package added or meets, in pieces a run strings together.
const KEYS: &[&str] = &[
    "v", "V", "<C-v>", "gv", "<Esc>", "<Esc>", "o", "O", "h", "j", "k", "l", "w", "b", "e", "$",
    "0", "^", "G", "gg", "}", "{", "d", "c", "y", "p", "P", "x", "X", "D", "C", "S", "s", "Y", "J",
    "~", "U", "u", "rq", "r<CR>", "I", "A", "a", "i", "R", "z", ".", ".", "3.", "@a", "@b", "@@",
    "qc", "q", "iw", "aw", "ip", "i(", "a\"", "<BS>", "<CR>", "2", "3", "\"a", "\"b", "<C-r>", ">",
    "<lt>", "gU", "g~", "fa", ";", "gu", "<Tab>",
];

const TEXTS: &[&str] = &[
    "abc def\n\nghi (jk) \"l\"\n",
    "a\tb\n世界x\ne\u{301}y\n\n",
    "x",
    "",
    "  in\n  dent\n\ttab\nzz",
];

/// The next number from a small generator, so a failure names its seed.
fn next(seed: &mut u64) -> usize {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (*seed >> 33) as usize
}

#[test]
fn no_keys_make_it_panic_or_leave_the_text() {
    let runs = std::env::var("DIE_PROP_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(500u64);
    for run in 0..runs {
        let mut seed = run;
        let text0 = TEXTS[next(&mut seed) % TEXTS.len()];
        let (mut ctx, b, w) = editor(text0);
        ctx.set_register(
            'a',
            Register {
                kind: RegKind::Macro,
                text: "lvld.".into(),
            },
        );
        ctx.set_register('b', chars("x<j>\ny"));
        let len = text0.len();
        if len > 3 && next(&mut seed).is_multiple_of(2) {
            let (from, to) = (next(&mut seed) % (len - 1), next(&mut seed) % len);
            let (from, to) = (from.min(to), from.max(to) + 1);
            let r = (text0.floor_char_boundary(from))..(text0.ceil_char_boundary(to));
            let replacement = next(&mut seed)
                .is_multiple_of(2)
                .then(|| "F".to_owned().into());
            let hide = Deco::Hide {
                replacement,
                reveal: Reveal::Never,
            };
            let _ = ctx.text_mut(b).unwrap().decorate(r, hide);
        }
        let mut typed = String::new();
        for _ in 0..24 {
            let key = KEYS[next(&mut seed) % KEYS.len()];
            typed.push_str(key);
            ctx.type_keys(key);
            let t = ctx.text(b).unwrap();
            let sel = t.selection(w).unwrap();
            let bad = ctx.messages.iter().find(|m| m.contains("panicked"));
            assert!(bad.is_none(), "seed {run}: {typed}: {bad:?}");
            assert!(
                sel.head <= t.len() && sel.anchor <= t.len(),
                "seed {run}: {typed}: {sel:?} in {} bytes",
                t.len()
            );
            ctx.messages.clear();
        }
    }
}
