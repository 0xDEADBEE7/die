//! Operators where the Evil oracle can't reach. Escape and most cancels are a quit, which
//! ends batch Emacs's keyboard macro. And two buffers, read-only text, huge counts, and a
//! window that isn't focused have no oracle case. Everything else is in the oracle.

use die_core::{Args, BufferId, Ctx, State, WindowId};
use die_vim::Vim;

fn editor() -> Ctx {
    Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
}

fn open(ctx: &mut Ctx, text: &str) -> (BufferId, WindowId) {
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 80, 24).expect("a window");
    (b, w)
}

fn text(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).expect("a text buffer").rope().to_string()
}

/// The text, cursor and messages after keys from a cursor position.
fn run(start: &str, cursor: usize, keys: &str) -> (String, usize, Vec<String>, State) {
    let mut ctx = editor();
    let (b, w) = open(&mut ctx, start);
    ctx.focus(w);
    assert_eq!(ctx.text_mut(b).unwrap().set_cursor(w, cursor), Some(cursor));
    ctx.type_keys(keys);
    let at = ctx.text(b).unwrap().cursor(w).unwrap();
    (text(&ctx, b), at, ctx.messages.clone(), ctx.state())
}

/// Each of these leaves everything as it was, and normal state, with the count gone: the
/// `l` after moves once. None of them says anything.
#[test]
fn keys_that_cancel_an_operator_do_so_quietly_and_take_the_counts() {
    for op in ["d", "c", "y", ">", "<lt>", "g~", "gU", "gu"] {
        for keys in [
            "<Esc>", "2<Esc>", "<F5>", "2<F5>", "i<Esc>", "a<Esc>", "iq", "ax", "i<F5>", "g<Esc>",
            "gq", "f<Esc>", "F<Esc>", "t<Esc>", "T<Esc>", "f<F5>", "\"",
        ] {
            let all = format!("3{op}{keys}");
            let keys = format!("{all}{}", if keys == "\"" { "<Esc>l" } else { "l" });
            let (t, at, messages, state) = run("abcdef\n", 0, &keys);
            assert_eq!(t, "abcdef\n", "{keys}");
            assert_eq!(state, State::Normal, "{keys}");
            assert!(messages.is_empty(), "{keys}: {messages:?}");
            assert_eq!(at, 1, "{keys}: l moves once, so the counts went");
        }
    }
}

/// Only the operator's own key means whole lines. Another operator's cancels it. So do
/// commands that aren't motions, which run as motions in your Evil (deviations.json).
#[test]
fn another_command_cancels_an_operator_and_doesnt_run() {
    for keys in [
        "dc", "dy", "cd", "yd", "d>", "dg~", "dgU", "dgu", "d~", "dU", "du", "dx", "dX", "ds",
        "dS", "dD", "dC", "dY", "dr", "dJ", "dp", "dP", "dA", "dI", "do", "dO", "dq", "d<C-r>",
        "g~d", "gUd",
    ] {
        let (t, at, messages, state) = run("ab cd\nef\n", 1, keys);
        assert_eq!(t, "ab cd\nef\n", "{keys}");
        assert_eq!(at, 1, "{keys}");
        assert_eq!(state, State::Normal, "{keys}");
        assert!(messages.is_empty(), "{keys}: {messages:?}");
    }
}

#[test]
fn a_cancelled_operator_forgets_the_goal_column() {
    for keys in ["d<Esc>", "d<F5>", "diq", "dy", "\"<Esc>"] {
        let (_, at, messages, _) = run("abcdef\nab\nabcdef\n", 0, &format!("4lj{keys}j"));
        assert!(messages.is_empty(), "{keys}: {messages:?}");
        assert_eq!(at, 11, "{keys}: j goes to the column the cursor is in");
    }
}

#[test]
fn a_key_after_a_register_name_can_cancel_it() {
    let (t, at, messages, _) = run("abc\n", 0, "\"<Esc>x\"ap");
    assert_eq!(t, "bc\n", "x still works, into no register");
    assert_eq!(at, 0);
    assert_eq!(messages.len(), 1, "a is empty: {messages:?}");
}

#[test]
fn r_with_a_key_that_isnt_text_does_nothing() {
    for key in ["<Esc>", "<F5>", "<C-x>", "<BS>"] {
        let keys = format!("3r{key}l");
        let (t, at, messages, _) = run("abcd\n", 0, &keys);
        assert_eq!(t, "abcd\n", "{keys}");
        assert_eq!(at, 1, "{keys}: the count went");
        assert!(messages.is_empty(), "{keys}: {messages:?}");
    }
}

/// A signal stops the operator, and normal state's next key is a command again.
#[test]
fn a_motion_that_stops_an_operator_leaves_normal_state() {
    for keys in [
        "dh", "d5h", "dk", "db", "d{", "dfz", "dFz", "dtz", "d%", "d;", "d,",
    ] {
        let (t, _, messages, state) = run("abc\n", 0, keys);
        assert_eq!(t, "abc\n", "{keys}");
        assert_eq!(state, State::Normal, "{keys}");
        assert_eq!(messages.len(), 1, "{keys}: {messages:?}");
    }
}

/// End of line and End of buffer don't stop one. Those motions took what they reached.
#[test]
fn end_of_line_and_end_of_buffer_dont_stop_an_operator() {
    for (keys, want, reg) in [
        ("d9l", "\n", Some("abc")),
        ("dw", "abc", Some("")),
        ("de", "ab", Some("c")),
        ("d}", "abc", Some("")),
    ] {
        let mut ctx = editor();
        let (b, w) = open(&mut ctx, "abc");
        ctx.focus(w);
        if keys != "d9l" {
            ctx.type_keys("$");
        }
        ctx.type_keys(keys);
        let want = want.strip_suffix('\n').unwrap_or(want);
        assert_eq!(text(&ctx, b), want, "{keys}");
        assert_eq!(ctx.register('"').map(|r| r.text.as_str()), reg, "{keys}");
        assert!(ctx.messages.is_empty(), "{keys}: {:?}", ctx.messages);
    }
}

#[test]
fn each_buffer_has_its_own_waiting_operator() {
    let mut ctx = editor();
    let (a, wa) = open(&mut ctx, "one two\n");
    let (b, wb) = open(&mut ctx, "three four\n");
    ctx.focus(wa);
    ctx.type_keys("d");
    assert_eq!(ctx.state(), State::OperatorPending);
    ctx.focus(wb);
    assert_eq!(ctx.state(), State::Normal, "B isn't waiting");
    ctx.type_keys("w");
    assert_eq!(
        ctx.text(b).unwrap().cursor(wb),
        Some(6),
        "w is just a motion"
    );
    ctx.focus(wa);
    assert_eq!(ctx.state(), State::OperatorPending, "A still is");
    ctx.type_keys("w");
    assert_eq!(text(&ctx, a), "two\n");
    assert_eq!(text(&ctx, b), "three four\n");
}

#[test]
fn an_operator_acts_for_the_window_it_was_run_for() {
    let mut ctx = editor();
    let (a, wa) = open(&mut ctx, "aaa\n");
    let (b, wb) = open(&mut ctx, "bbb\n");
    ctx.focus(wa);
    let args = Args {
        window: Some(wb),
        buffer: Some(b),
        ..Default::default()
    };
    ctx.run("vim-delete-char", &args).unwrap();
    assert_eq!(text(&ctx, b), "bb\n");
    assert_eq!(text(&ctx, a), "aaa\n");
    assert_eq!(ctx.state(), State::Normal);
    ctx.run("vim-delete", &args).unwrap();
    assert_eq!(ctx.state_of(b), State::OperatorPending);
    assert_eq!(ctx.state_of(a), State::Normal, "not the focused buffer's");
    ctx.run("vim-delete", &args).unwrap();
    assert_eq!(text(&ctx, b), "", "dd");
    assert_eq!(ctx.state_of(b), State::Normal);
}

/// Read-only text can't change, and a change on it must not start an insert session or
/// leave an undo group open. The yank still happens, as in your Evil.
#[test]
fn operators_on_read_only_text_fail_and_change_nothing() {
    for keys in [
        "dw", "dd", "x", "X", "D", "J", "rx", "r<CR>", "~", "g~w", "gUw", "guw", ">>", "cw", "cc",
        "C", "s", "S", "p", "P", "diw",
    ] {
        let mut ctx = editor();
        let (b, w) = open(&mut ctx, "abc def\nghi\n");
        ctx.focus(w);
        ctx.type_keys("l");
        ctx.type_keys("yy");
        ctx.text_mut(b).unwrap().settings_mut().read_only = true;
        ctx.type_keys(keys);
        assert_eq!(text(&ctx, b), "abc def\nghi\n", "{keys}");
        assert_eq!(ctx.state(), State::Normal, "{keys}: no insert session");
        assert_eq!(ctx.messages.len(), 1, "{keys}: {:?}", ctx.messages);
        // Writable again, the next edit is its own undo step: no group was left open.
        ctx.text_mut(b).unwrap().settings_mut().read_only = false;
        ctx.type_keys("x");
        ctx.type_keys("x");
        ctx.type_keys("u");
        assert_ne!(text(&ctx, b), "abc def\nghi\n", "{keys}: u undid one x");
    }
}

/// Mistyped counts must fail or stop at the text's edge, not hang.
#[test]
fn huge_counts_stop_at_the_edges() {
    const HUGE: &str = "99999999999999999999";
    let start = "ab cd\nef\n\ngh\n";
    for keys in [
        "x", "X", "s<Esc>", "S<Esc>", "D", "C<Esc>", "Y", "~", "rx", "J", "dd", "yy", ">>", "dw",
        "dj", "dk", "dgg", "dG", "d}", "d{", "d$", "diw", "daw", "dip", "dap", "di\"", "di(",
        "da(", "dfa", "gUw",
    ] {
        run(start, 1, &format!("{HUGE}{keys}"));
    }
    run(start, 0, &format!("{HUGE}d{HUGE}w"));
    run(start, 0, &format!("{HUGE}d{HUGE}d"));
    let (_, _, messages, _) = run(start, 0, &format!("yy{HUGE}p"));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("Count too large"), "{messages:?}");
}

/// A pasted register larger than the replay cap is still pasted once.
#[test]
fn a_large_register_pastes_once_without_a_count() {
    let big = "x".repeat(2 << 20);
    let mut ctx = editor();
    let (b, w) = open(&mut ctx, &big);
    ctx.focus(w);
    ctx.type_keys("y$");
    ctx.type_keys("p");
    assert_eq!(text(&ctx, b).len(), 2 * big.len());
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
    ctx.type_keys("2p");
    assert_eq!(ctx.messages.len(), 1, "2p would be past the cap");
}

/// `y` doesn't change the text, so it makes no undo step.
#[test]
fn yank_makes_no_undo_step() {
    let (t, _, messages, _) = run("ab\n", 0, "yyu");
    assert_eq!(t, "ab\n");
    assert_eq!(messages.len(), 1, "nothing to undo: {messages:?}");
}

/// An operator and its insert session are one undo step, and the goal column doesn't come
/// back after `u`.
#[test]
fn a_change_and_its_insert_session_are_one_undo_step() {
    for keys in [
        "cwxy<Esc>",
        "ccx<CR>y<Esc>",
        "Sxy<Esc>",
        "Cxy<Esc>",
        "sxy<Esc>",
        "ciwxy<Esc>",
        "cipxy<Esc>",
        "c2jxy<Esc>",
    ] {
        let (t, at, messages, _) = run("ab cd\nef\ngh\nij\n", 1, &format!("{keys}u"));
        assert_eq!(t, "ab cd\nef\ngh\nij\n", "{keys}");
        assert_eq!(at, 1, "{keys}");
        assert!(messages.is_empty(), "{keys}: {messages:?}");
    }
}

/// A count on `c` is for its motion: the session it starts isn't replayed.
#[test]
fn a_count_on_c_doesnt_replay_the_insert() {
    let (t, ..) = run("a b c d\n", 0, "2cwX<Esc>");
    assert_eq!(t, "X c d\n");
    let (t, ..) = run("abcd\n", 0, "2sX<Esc>");
    assert_eq!(t, "Xcd\n");
    let (t, ..) = run("a\nb\nc\n", 0, "2SX<Esc>");
    assert_eq!(t, "X\nc\n");
    let (t, ..) = run("ab\ncd\n", 0, "2CX<Esc>");
    assert_eq!(t, "X\n");
}

/// Operators reach where motions in normal state don't, and on the way out the cursor is
/// where normal state allows. So nothing is left resting on a line's newline.
#[test]
fn the_cursor_is_never_left_on_a_newline() {
    for (start, at, keys, want) in [
        ("ab cd\n", 3, "dw", 2),
        ("abc\n", 2, "x", 1),
        ("abc\n", 1, "D", 0),
        ("abc\n", 1, "d$", 0),
        ("a\nb\n", 0, "yl$p", 1),
        ("a b\n", 2, "daw", 0),
        ("ab\ncd\n", 3, "dG", 0),
    ] {
        let (_, got, messages, _) = run(start, at, keys);
        assert_eq!(got, want, "{keys} in {start:?}");
        assert!(messages.is_empty(), "{keys}: {messages:?}");
    }
}

/// `dG` on the last line and `cc` at the end: rule 1's cursor, and `cc`'s new line.
#[test]
fn rule_1_after_an_operator_at_the_end_of_the_text() {
    let (t, at, ..) = run("a\nbc\n", 3, "dd");
    assert_eq!((t.as_str(), at), ("a\n", 0), "not on the empty line after");
    let (t, at, ..) = run("a\nbc\n", 3, "ccx<Esc>");
    assert_eq!((t.as_str(), at), ("a\nx\n", 2));
    // From the empty line an insert session left, operators act there.
    let (t, at, ..) = run("a\n", 0, "o<Esc>dd");
    assert_eq!((t.as_str(), at), ("a\n", 0));
    let (t, at, ..) = run("a\n", 0, "o<Esc>x");
    assert_eq!(
        (t.as_str(), at),
        ("a\n\n", 2),
        "x on an empty line does nothing"
    );
}

/// `gU` on `ß` makes it `SS`, so what's past it moved, and `~` on a letter with no other
/// single case leaves it. The cursor is never inside a character.
#[test]
fn case_changes_that_change_the_length() {
    let (t, at, ..) = run("aßb\n", 0, "gUw");
    assert_eq!((t.as_str(), at), ("ASSB\n", 0));
    let (t, at, ..) = run("ßb\n", 0, "2~");
    assert_eq!((t.as_str(), at), ("ßB\n", 2));
}
