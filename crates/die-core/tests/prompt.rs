//! K11a: the command line's editing keys, history and answers, and `*messages*`.

use die_test_cap as _;

use die_core::{
    Args, Ctx, CursorShape, Error, Extension, Origin, PromptSpec, RegKind, Register, Registry, Rgb,
    ShowIn, Side, State, Style, Theme,
};
use die_core::{Input, Mods, PointerEvent, PointerKind};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn ctx() -> Ctx {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "");
    ctx.open_window(b, 40, 5).unwrap();
    ctx
}

type Answers = Rc<RefCell<Vec<Option<String>>>>;

fn ask(ctx: &mut Ctx, spec: impl Into<PromptSpec>) -> Answers {
    let answers = Answers::default();
    let a = answers.clone();
    ctx.prompt(spec, move |_, answer| {
        a.borrow_mut().push(answer);
        Ok(())
    });
    answers
}

/// Core alone has only [`Ctx::register`]'s registers. A name it can't read is an error for that
/// key, and the prompt stays. `<D-v>` takes the clipboard as it is, up to the newlines at its end.
#[test]
fn registers_and_the_clipboard_paste_at_the_cursor() {
    let mut ctx = ctx();
    let reg = |text: &str| Register {
        kind: RegKind::Char,
        text: text.into(),
    };
    ctx.set_register('a', reg("A"));
    ctx.set_register('"', reg("K"));
    ctx.write_clipboard(&reg("one\ntwo\n\n"));
    let answers = ask(&mut ctx, "x");
    ctx.type_keys("()<Left><C-r>a<C-y>");
    assert_eq!(ctx.prompt_text(), Some("(AK)"));
    let before = ctx.messages.len();
    ctx.type_keys("<C-r>z");
    assert_eq!(ctx.messages.len(), before + 1, "says so");
    assert_eq!(ctx.prompt_text(), Some("(AK)"), "and stays");
    ctx.type_keys("<D-v><CR>");
    assert_eq!(*answers.borrow(), [Some("(AKone\ntwo)".to_string())]);
}

/// The keys typed after `abc def`, and the line they leave. `X` shows where the cursor is.
#[test]
fn each_editing_key_edits_the_line_at_the_cursor() {
    for (keys, want) in [
        ("<Left>X", "abc deXf"),
        ("<C-b>X", "abc deXf"),
        ("<Left><Left><Right>X", "abc deXf"),
        ("<Left><Left><C-f>X", "abc deXf"),
        ("<Home>X", "Xabc def"),
        ("<C-a>X", "Xabc def"),
        ("<Home><End>X", "abc defX"),
        ("<C-a><C-e>X", "abc defX"),
        ("<M-b>X", "abc Xdef"),
        ("<M-Left>X", "abc Xdef"),
        ("<C-Left><C-Left>X", "Xabc def"),
        ("<Home><M-f>X", "abcX def"),
        ("<Home><M-Right><M-f>X", "abc defX"),
        ("<Home><C-Right>X", "abcX def"),
        ("<BS>X", "abc deX"),
        ("<C-h>X", "abc deX"),
        ("<Left><Del>X", "abc deX"),
        ("<Left><C-d>X", "abc deX"),
        ("<C-w>X", "abc X"),
        ("<C-w><C-w>X", "X"),
        ("<M-BS>X", "abc X"),
        ("<Home><M-d>X", "X def"),
        ("<Left><C-u>X", "Xf"),
        ("<Left><C-k>X", "abc deX"),
        // Nothing to do at either end.
        ("<Home><BS><Left>X", "Xabc def"),
        ("<End><Del><Right>X", "abc defX"),
        // A key with no meaning here types nothing.
        ("<C-z><F1><Tab>X", "abc defX"),
    ] {
        let mut ctx = ctx();
        ask(&mut ctx, "?");
        ctx.type_keys("abc def");
        ctx.type_keys(keys);
        assert_eq!(ctx.prompt_text(), Some(want), "{keys}");
    }
}

#[test]
fn a_word_is_a_run_of_one_kind_and_a_grapheme_moves_as_one() {
    let mut ctx = ctx();
    ask(&mut ctx, "?");
    ctx.type_keys("a/b");
    ctx.type_keys("<C-w>");
    assert_eq!(ctx.prompt_text(), Some("a/"));
    ctx.type_keys("<C-w>");
    assert_eq!(ctx.prompt_text(), Some("a"));
    ctx.type_keys("<C-u>e\u{301}x<Left><Left>Y");
    assert_eq!(ctx.prompt_text(), Some("Ye\u{301}x"));
    ctx.type_keys("<Right><BS>");
    assert_eq!(ctx.prompt_text(), Some("Yx"));
}

#[test]
fn cancel_gives_none_and_remembers_nothing() {
    for key in ["<Esc>", "<C-g>", "<C-c>"] {
        let mut ctx = ctx();
        let answers = ask(&mut ctx, PromptSpec::new("?").history("h"));
        ctx.type_keys("abc");
        ctx.type_keys(key);
        assert_eq!(*answers.borrow(), [None], "{key}");
        assert_eq!(ctx.prompt_text(), None);
        assert_eq!(ctx.state(), State::Normal);
        assert!(ctx.prompt_history("h").is_empty());
    }
}

#[test]
fn history_goes_back_and_forward_and_keeps_what_was_typed() {
    let mut ctx = ctx();
    for answer in ["one", "two", "two", ""] {
        ask(&mut ctx, PromptSpec::new("?").history("h"));
        ctx.type_keys(answer);
        ctx.type_keys("<CR>");
    }
    assert_eq!(
        ctx.prompt_history("h"),
        ["one", "two"],
        "no repeat, no empty"
    );
    ask(&mut ctx, PromptSpec::new("?").history("other"));
    ctx.type_keys("<Up>");
    assert_eq!(ctx.prompt_text(), Some(""), "a history per name");
    ctx.type_keys("<Esc>");

    ask(&mut ctx, PromptSpec::new("?").history("h"));
    ctx.type_keys("<Down>dra");
    for (key, want) in [
        ("<Up>", "two"),
        ("<C-p>", "one"),
        ("<Up>", "one"),
        ("<Down>", "two"),
        ("<C-n>", "dra"),
        ("<Down>", "dra"),
    ] {
        ctx.type_keys(key);
        assert_eq!(ctx.prompt_text(), Some(want), "{key}");
    }
    ctx.type_keys("<Up>X");
    assert_eq!(
        ctx.prompt_text(),
        Some("twoX"),
        "the cursor lands at the end"
    );
}

/// Evil's `<Up>` and `<Down>`: only entries that start with what's before the cursor, which
/// stays where it is. `<C-p>`, `<C-n>` and the page keys walk every entry, on any prompt.
#[test]
fn evil_keys_recall_by_what_is_before_the_cursor() {
    let mut ctx = ctx();
    for answer in ["foo", "bar", "fig"] {
        ask(&mut ctx, PromptSpec::new("?").history("h"));
        ctx.type_keys(answer);
        ctx.type_keys("<CR>");
    }
    ask(&mut ctx, PromptSpec::new("?").history("h").evil_keys());
    ctx.type_keys("f");
    for (key, want) in [
        ("<Up>", "fig"),
        ("<Up>", "foo"),
        ("<Up>", "foo"),
        ("<Down>", "fig"),
        ("<Down>", "f"),
        ("<C-p>", "fig"),
        ("<C-p>", "bar"),
        ("<PageUp>", "foo"),
        ("<PageDown>", "bar"),
        ("<C-n>", "fig"),
    ] {
        ctx.type_keys(key);
        assert_eq!(ctx.prompt_text(), Some(want), "{key}");
    }
    ctx.type_keys("<Esc>");
    ask(&mut ctx, PromptSpec::new("?").history("h").evil_keys());
    ctx.type_keys("f<Up>X");
    assert_eq!(ctx.prompt_text(), Some("fXig"), "the cursor stays");
    ctx.type_keys("<Esc>");
    ask(&mut ctx, PromptSpec::new("?").history("h"));
    ctx.type_keys("f<Up><Up>");
    assert_eq!(
        ctx.prompt_text(),
        Some("bar"),
        "a plain prompt walks every entry"
    );
    ctx.type_keys("<PageDown>");
    assert_eq!(ctx.prompt_text(), Some("fig"), "and has the page keys");
}

/// `evil-command-line-map`'s line keys, and the Shift word moves every prompt has.
#[test]
fn evil_keys_take_the_whole_line_and_shift_moves_by_word() {
    for (evil, keys, want) in [
        (true, "<Left><C-u>X", "X"),
        (true, "<C-b>X", "Xabc def"),
        (true, "<S-Left>X", "abc Xdef"),
        (true, "<Home><S-Right>X", "abcX def"),
        (false, "<S-Left>X", "abc Xdef"),
        (false, "<Home><S-Right>X", "abcX def"),
    ] {
        let mut ctx = ctx();
        let spec = PromptSpec::new("?");
        ask(&mut ctx, if evil { spec.evil_keys() } else { spec });
        ctx.type_keys("abc def");
        ctx.type_keys(keys);
        assert_eq!(ctx.prompt_text(), Some(want), "{keys}");
    }
}

#[test]
fn backspace_on_an_empty_line_cancels_only_when_asked() {
    let mut ctx = ctx();
    let answers = ask(&mut ctx, PromptSpec::new("/").backspace_cancels());
    ctx.type_keys("a<BS>");
    assert!(answers.borrow().is_empty(), "the line had text");
    ctx.type_keys("<BS>");
    assert_eq!(*answers.borrow(), [None]);

    let answers = ask(&mut ctx, "?");
    ctx.type_keys("<BS>");
    assert!(answers.borrow().is_empty());
    assert_eq!(ctx.prompt_text(), Some(""));
    ctx.type_keys("<Esc>");
}

#[test]
fn on_change_hears_each_edit_with_no_power_over_the_keys() {
    let mut ctx = ctx();
    let heard = Rc::new(RefCell::new(vec![]));
    let h = heard.clone();
    let answers = ask(
        &mut ctx,
        PromptSpec::new("/").initial("a").on_change(move |_, text| {
            h.borrow_mut().push(text.to_string());
            Err(Error::Msg("heard".into()))
        }),
    );
    assert_eq!(ctx.prompt_text(), Some("a"));
    ctx.feed_keys("b<Left><Home>c<CR>").unwrap();
    ctx.run_until_idle();
    assert_eq!(*heard.borrow(), ["ab", "cab"], "moves change no text");
    assert_eq!(
        *answers.borrow(),
        [Some("cab".to_string())],
        "errors drop no keys"
    );
    assert_eq!(ctx.messages, ["heard", "heard"]);
}

#[test]
fn the_cursor_shows_where_it_is_and_a_long_line_scrolls_to_it() {
    let mut ctx = ctx();
    ctx.set_frame_size(6, 3);
    ask(&mut ctx, ":");
    ctx.type_keys("ab<Left>");
    let s = ctx.scene();
    assert_eq!(s.row_text(2), ":ab");
    assert_eq!(
        (s.cursors[0].col, s.cursors[0].shape),
        (2, CursorShape::Bar)
    );

    ctx.type_keys("<End>cdefgh");
    let s = ctx.scene();
    assert_eq!(s.row_text(2), "defgh", "the end, and room for the cursor");
    assert_eq!(s.cursors[0].col, 5);
    ctx.type_keys("<Left><Left>");
    assert_eq!(ctx.scene().cursors[0].col, 5, "still past the row");
    ctx.type_keys("<Home>");
    let s = ctx.scene();
    assert_eq!(s.row_text(2), ":abcde", "the start");
    assert_eq!(s.cursors[0].col, 1);
}

#[test]
fn messages_reach_a_read_only_buffer_and_the_command_row_until_the_next_key() {
    let mut ctx = ctx();
    ctx.set_frame_size(20, 3);
    assert_eq!(
        ctx.find_buffer("*messages*"),
        None,
        "made with the first one"
    );
    ctx.messages.push("first".into());
    ctx.messages.push("second\nmore".into());
    assert_eq!(ctx.scene().row_text(2), "second", "its first line");

    let b = ctx.find_buffer("*messages*").unwrap();
    let t = ctx.text(b).unwrap();
    assert_eq!(t.rope().to_string(), "first\nsecond\nmore\n");
    assert!(t.settings().read_only);
    assert!(!t.modified());
    assert!(ctx.text_mut(b).unwrap().undo().is_err(), "no undo");
    assert_ne!(ctx.buffer_list()[0], b, "last in the list");

    ctx.type_keys("<F1>");
    let row = ctx.scene().row_text(2);
    assert_eq!(row, "<F1> is undefined", "a key's own message replaces it");
    ask(&mut ctx, ":");
    assert_eq!(ctx.scene().row_text(2), ":", "a prompt goes over it");
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.scene().row_text(2), "", "and a key clears it");
}

#[test]
fn messages_are_never_fallen_back_on_and_come_back_when_killed() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "");
    let w = ctx.open_window(b, 40, 5).unwrap();
    ctx.messages.push("one".into());
    ctx.scene();
    let log = ctx.find_buffer("*messages*").unwrap();
    assert!(ctx.kill(b));
    let there = ctx.window_buffer(w).unwrap();
    assert_ne!(there, log);
    assert_eq!(ctx.buffer_name(there), Some("*scratch*"));

    assert!(ctx.kill(log));
    ctx.messages.push("two".into());
    ctx.scene();
    let log = ctx.find_buffer("*messages*").unwrap();
    assert_eq!(ctx.text(log).unwrap().rope().to_string(), "two\n");
}

/// As Emacs's `*Messages*`: opened at its end, and a cursor there stays there, in any window.
#[test]
fn messages_follow_their_end_in_a_window_at_the_end() {
    let mut ctx = ctx();
    ctx.set_frame_size(40, 12);
    let w = ctx.focused().unwrap();
    for i in 0..30 {
        ctx.messages.push(format!("line {i}"));
    }
    ctx.scene();
    let log = ctx.find_buffer("*messages*").unwrap();
    let start_of = |ctx: &Ctx, line| ctx.text(log).unwrap().line_start(line);

    ctx.show(log, ShowIn::Current).unwrap();
    assert_eq!(ctx.text(log).unwrap().cursor(w), Some(start_of(&ctx, 29)));
    let other = ctx.split_window(w, Side::Below).unwrap();
    ctx.focus(other);
    let t = ctx.create_text("t2", "");
    ctx.show(t, ShowIn::Current).unwrap();
    assert_eq!(ctx.focused(), Some(other));

    ctx.messages.push("newest".into());
    let scene = ctx.scene();
    let shown = (0..scene.rows).map(|r| scene.row_text(r)).collect::<Vec<_>>();
    assert!(shown.iter().any(|r| r == "newest"), "{shown:?}");
    assert_eq!(ctx.text(log).unwrap().cursor(w), Some(start_of(&ctx, 30)));

    ctx.text_mut(log).unwrap().set_cursor(w, 0);
    ctx.messages.push("one more".into());
    ctx.scene();
    assert_eq!(ctx.text(log).unwrap().cursor(w), Some(0), "a cursor elsewhere stays");
}

#[test]
fn the_log_keeps_its_last_thousand_lines() {
    let mut ctx = ctx();
    for i in 0..1500 {
        ctx.messages.push(i.to_string());
    }
    ctx.scene();
    assert_eq!(ctx.messages.len(), 1000);
    assert_eq!(ctx.messages[0], "500");
    let log = ctx.find_buffer("*messages*").unwrap();
    let text = ctx.text(log).unwrap().rope().to_string();
    assert!(text.starts_with("500\n501\n") && text.ends_with("1499\n"));
    assert_eq!(text.lines().count(), 1000);
}

// ------------------------------------------------------------------ candidates (K11b)

/// Each order is what Emacs 30.2 printed for `completion-styles '(flex)`, candidates in the order
/// given, as `completion--flex-adjust-metadata`'s sort leaves them.
#[test]
fn flex_ranks_as_emacs_does() {
    for (query, candidates, want) in [
        (
            "ff",
            &[
                "diff",
                "find-file",
                "buffer-file",
                "off",
                "fringe-face",
                "of-find",
            ][..],
            &[
                "off",
                "diff",
                "buffer-file",
                "of-find",
                "find-file",
                "fringe-face",
            ][..],
        ),
        (
            "foo",
            &[
                "fabrobazo",
                "fbarbazoo",
                "barfoobaz",
                "foo",
                "food",
                "f-o-o",
            ],
            &[
                "foo",
                "food",
                "barfoobaz",
                "f-o-o",
                "fbarbazoo",
                "fabrobazo",
            ],
        ),
        (
            "sb",
            &[
                "switch-buffer",
                "sb",
                "subscribe",
                "kill-buffer",
                "absorb",
                "s-b",
            ],
            &["sb", "s-b", "subscribe", "absorb", "switch-buffer"],
        ),
        (
            "kb",
            &["kill-buffer", "keyboard", "kb", "knob", "k-x-b"],
            &["kb", "knob", "k-x-b", "keyboard", "kill-buffer"],
        ),
        (
            "cp",
            &["command-palette", "copy", "cp", "scope", "cap", "c-p"],
            &["cp", "cap", "c-p", "copy", "scope", "command-palette"],
        ),
        (
            "fnd",
            &["find", "fund", "find-file", "friend", "fine-tuned"],
            &["find", "fund", "find-file", "friend", "fine-tuned"],
        ),
        (
            "mt",
            &["mt", "at-mt", "m-t", "mid-text", "market", "ampt", "tm"],
            &["mt", "at-mt", "m-t", "ampt", "market", "mid-text"],
        ),
        (
            "aa",
            &["abab", "aab", "baab", "aba", "a-a"],
            &["aab", "baab", "aba", "a-a", "abab"],
        ),
        (
            "rs",
            &["src/main.rs", "src/rs", "docs.rs", "cargo.rs", "rust"],
            &["docs.rs", "rust", "src/rs", "cargo.rs", "src/main.rs"],
        ),
        ("zz", &["abc", "zebra"], &[]),
        ("é", &["café", "cafe", "éa"], &["éa", "café"]),
        // Not Emacs: smartcase, where Emacs matches case exactly.
        ("rd", &["README", "bird", "rod"], &["bird", "rod", "README"]),
        ("Rd", &["README", "bird", "Rod"], &["Rod"]),
        // With nothing typed, everything, in the order given.
        ("", &["b", "a"], &["b", "a"]),
        // A tie goes to the one in the query's case.
        ("readme", &["README", "readme"], &["readme", "README"]),
    ] {
        assert_eq!(die_core::flex_rank(query, candidates), want, "{query}");
    }
}

fn numbered(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("item{i}")).collect()
}

#[test]
fn twenty_rows_show_under_the_line_and_scroll_to_the_chosen_one() {
    let mut ctx = ctx();
    ctx.set_frame_size(30, 30);
    ask(&mut ctx, PromptSpec::new("? ").candidates(numbered(50)));
    let s = ctx.scene();
    assert_eq!(s.row_text(9), "1/50   ?", "above the 20 rows");
    assert_eq!(s.row_text(10), "item0");
    assert_eq!(s.row_text(29), "item19");
    assert_eq!((s.cursors[0].row, s.cursors[0].col), (9, 9));

    ctx.type_keys("<PageDown><C-n><C-n>");
    assert_eq!(ctx.prompt_chosen(), Some("item22"));
    let s = ctx.scene();
    assert_eq!(s.row_text(9), "23/50  ?");
    assert_eq!(s.row_text(10), "item3");
    assert_eq!(s.row_text(29), "item22");
    ctx.type_keys("<M-v><Up>");
    assert_eq!(ctx.prompt_chosen(), Some("item1"));
    assert_eq!(
        ctx.scene().row_text(10),
        "item1",
        "back up, it stays in view"
    );

    ctx.type_keys("<PageUp><Up>");
    assert_eq!(ctx.prompt_chosen(), Some("item0"), "stops at the first");
    for _ in 0..3 {
        ctx.type_keys("<PageDown>");
    }
    ctx.type_keys("<Down>");
    assert_eq!(ctx.prompt_chosen(), Some("item49"), "stops at the last");

    ctx.type_keys("item1");
    assert_eq!(ctx.prompt_chosen(), Some("item1"), "typing starts over");
    assert_eq!(
        ctx.prompt_candidates().len(),
        14,
        "item1, item10.., item21.."
    );
    let s = ctx.scene();
    assert_eq!(s.row_text(9), "1/14   ? item1", "the list doesn't shrink");
    assert_eq!(s.row_text(11), "item10", "ties in the order given");
    assert_eq!(s.row_text(24), "");
}

#[test]
fn the_list_covers_the_windows_and_marks_the_chosen_row_and_the_match() {
    let mut ctx = ctx();
    ctx.set_frame_size(20, 8);
    let red = Rgb::hex("#c80000");
    let blue = Rgb::hex("#0000c8");
    let theme = Theme::new("t", [Rgb::hex("#000000"); 16])
        .face("prompt-current", Style::new().bg(red))
        .face("prompt-match", Style::new().fg(blue));
    ctx.set_theme(theme);

    ask(&mut ctx, PromptSpec::new("").candidates(["abc", "xbz"]));
    ctx.type_keys("b");
    let s = ctx.scene();
    assert_eq!(s.row_text(5), "1/2    b");
    assert_eq!(s.row_text(6), "abc");
    assert_eq!(s.row_text(7), "xbz");
    assert_eq!(s.position(6, 0), None, "nothing to click on under it");
    assert_eq!(s.cursors.len(), 2, "the window's cursor is above the list");
    assert_eq!(s.row(6)[19].bg, red, "the chosen row, all the way across");
    assert_eq!(s.row(6)[1].fg, blue, "the match");
    assert_ne!(s.row(6)[0].fg, blue);
    assert_ne!(s.row(7)[0].bg, red);
    assert_eq!(s.row(7)[1].fg, blue);

    ctx.set_frame_size(20, 2);
    let s = ctx.scene();
    assert_eq!(s.row_text(0), "1/2    b", "a small frame gets what fits");
    assert_eq!(s.row_text(1), "abc");
    assert_eq!(s.cursors.len(), 1, "the window's cursor is under the list");

    ctx.type_keys("<Esc>");
    ctx.set_frame_size(20, 8);
    let long = ctx.create_text("long", &"x\n".repeat(50));
    ctx.show(long, ShowIn::Current).unwrap();
    let w = ctx.focused().unwrap();
    ask(&mut ctx, PromptSpec::new("").candidates(["abc", "xbz"]));
    ctx.scene();
    let scroll = |ctx: &mut Ctx, row| {
        ctx.handle(Input::Pointer(PointerEvent {
            kind: PointerKind::Scroll { dx: 0.0, dy: 3.0 },
            row,
            col: 0,
            clicks: 0,
            mods: Mods::default(),
        }));
        ctx.run_ready();
        ctx.scene();
        ctx.text(long).unwrap().scroll(w).unwrap().0
    };
    assert_eq!(scroll(&mut ctx, 7), 0, "over the list, nothing under it moves");
    assert_ne!(scroll(&mut ctx, 0), 0, "above it, the window scrolls");
}

#[test]
fn the_answer_is_the_chosen_candidate_or_on_request_what_was_typed() {
    let list = ["alpha", "beta"];
    for (keys, want) in [
        ("<CR>", "alpha"),
        ("<C-n><CR>", "beta"),
        ("<Down><Up><CR>", "alpha"),
        ("bet<CR>", "beta"),
        ("zzz<CR>", "zzz"),
        ("be<M-CR>", "be"),
    ] {
        let mut ctx = ctx();
        let answers = ask(&mut ctx, PromptSpec::new("?").candidates(list));
        ctx.type_keys(keys);
        assert_eq!(*answers.borrow(), [Some(want.to_string())], "{keys}");
    }

    let mut ctx = ctx();
    let answers = ask(&mut ctx, PromptSpec::new("?").candidates(list));
    ctx.type_keys("<Down><Tab>");
    assert_eq!(ctx.prompt_text(), Some("beta"), "<Tab> doesn't answer");
    assert!(answers.borrow().is_empty());
    assert_eq!(ctx.prompt_candidates(), ["beta"], "and matches again");
}

#[test]
fn meta_angle_brackets_go_to_the_ends_of_the_list() {
    let mut ctx = ctx();
    ask(&mut ctx, PromptSpec::new("?").candidates(numbered(50)));
    ctx.type_keys("<M->>");
    assert_eq!(ctx.prompt_chosen(), Some("item49"));
    ctx.type_keys("<M-<>");
    assert_eq!(ctx.prompt_chosen(), Some("item0"));
}

#[test]
fn a_directory_on_the_line_is_chosen_until_a_row_is() {
    let spec = || {
        PromptSpec::new("Find: ")
            .initial("src/")
            .descends()
            .candidates_from(|text| {
                let base = text.rfind('/').map_or(0, |i| i + 1);
                let items = match &text[..base] {
                    "src/" => vec!["core/", "main.rs"],
                    _ => vec![],
                };
                (base, items.into_iter().map(String::from).collect())
            })
    };
    for (keys, want) in [
        ("<CR>", "src/"),
        ("<Down><Up><CR>", "src/"),
        ("<Tab><CR>", "src/core/"),
        ("<Down><CR><CR>", "src/core/"),
        ("<Down><Down><CR>", "src/main.rs"),
        ("<M->><M-<><M-<><CR>", "src/"),
        ("m<BS><CR>", "src/"),
    ] {
        let mut ctx = ctx();
        let answers = ask(&mut ctx, spec());
        ctx.type_keys(keys);
        assert_eq!(*answers.borrow(), [Some(want.to_string())], "{keys}");
    }

    let mut ctx = ctx();
    ctx.set_frame_size(40, 30);
    ask(&mut ctx, spec());
    assert_eq!(ctx.prompt_chosen(), None);
    assert_eq!(ctx.scene().row_text(27), "*/2    Find: src/");
    ctx.type_keys("<Down>");
    assert_eq!(ctx.prompt_chosen(), Some("core/"));
    assert_eq!(ctx.scene().row_text(27), "1/2    Find: src/");
}

#[test]
fn history_stays_on_meta_keys_beside_a_list() {
    let mut ctx = ctx();
    for list in [["one", "two"], ["two", "one"]] {
        ask(&mut ctx, PromptSpec::new("?").history("h").candidates(list));
        ctx.type_keys("<CR>");
    }
    assert_eq!(ctx.prompt_history("h"), ["one", "two"], "what was answered");
    ask(
        &mut ctx,
        PromptSpec::new("?").history("h").candidates(["x"]),
    );
    ctx.type_keys("<Up><M-p>");
    assert_eq!(ctx.prompt_text(), Some("two"));
    ctx.type_keys("<M-p><M-n><M-n><M-n>");
    assert_eq!(ctx.prompt_text(), Some(""));
    ctx.type_keys("<Esc>");
    ask(&mut ctx, PromptSpec::new("?").history("h"));
    ctx.type_keys("<M-p>");
    assert_eq!(ctx.prompt_text(), Some("two"), "and with no list");
}

#[test]
fn candidates_can_follow_the_text_and_a_directory_descends() {
    let mut ctx = ctx();
    let asked = Rc::new(RefCell::new(vec![]));
    let a = asked.clone();
    let spec = PromptSpec::new("Find: ")
        .initial("src/")
        .descends()
        .candidates_from(move |text| {
            a.borrow_mut().push(text.to_string());
            let base = text.rfind('/').map_or(0, |i| i + 1);
            let items = match &text[..base] {
                "src/" => vec!["core/", "main.rs"],
                "src/core/" => vec!["lib.rs"],
                _ => vec![],
            };
            (base, items.into_iter().map(String::from).collect())
        });
    let answers = ask(&mut ctx, spec);
    assert_eq!(
        ctx.prompt_candidates(),
        ["core/", "main.rs"],
        "from the start"
    );

    ctx.type_keys("c<CR>");
    assert_eq!(ctx.prompt_text(), Some("src/core/"), "stays open");
    assert!(answers.borrow().is_empty());
    assert_eq!(ctx.prompt_candidates(), ["lib.rs"]);

    ctx.type_keys("lb<CR>");
    assert_eq!(*answers.borrow(), [Some("src/core/lib.rs".to_string())]);
    assert_eq!(
        *asked.borrow(),
        ["src/", "src/c", "src/core/", "src/core/l", "src/core/lb"]
    );

    let answers = ask(
        &mut ctx,
        PromptSpec::new("?").candidates_from(|text| match text {
            "" => (0, vec!["ok".to_string()]),
            _ => panic!("no list for you"),
        }),
    );
    ctx.type_keys("x");
    assert!(ctx.prompt_candidates().is_empty(), "a panic empties it");
    assert_eq!(ctx.prompt_text(), Some("x"));
    assert!(ctx.messages.iter().any(|m| m.contains("no list for you")));
    ctx.type_keys("<CR>");
    assert_eq!(*answers.borrow(), [Some("x".to_string())]);
}

struct Commands(Rc<RefCell<Vec<Option<usize>>>>);

impl Extension for Commands {
    fn name(&self) -> &'static str {
        "commands"
    }
    fn register(&mut self, reg: &mut Registry) {
        let log = self.0.clone();
        reg.command("say-hello", "", move |_, a| {
            log.borrow_mut().push(a.count);
            Ok(())
        });
        reg.global_keymap()
            .bind(State::CommandLine, "<D-h>", "say-hello");
    }
}

#[test]
fn command_palette_runs_what_is_chosen_with_its_count() {
    let log = Rc::new(RefCell::new(vec![]));
    let mut ctx = Ctx::new(vec![Box::new(Commands(log.clone()))]).unwrap();
    let b = ctx.create_text("t", "");
    ctx.open_window(b, 40, 5).unwrap();
    let args = Args {
        count: Some(3),
        ..Args::default()
    };

    ctx.run("command-palette", &args).unwrap();
    assert!(ctx.prompt_candidates().contains(&"toggle-max-width"));
    assert!(
        ctx.prompt_candidates()
            .is_sorted_by_key(|n| (n.chars().count(), n.to_string())),
        "shortest first, as vertico sorts"
    );
    ctx.type_keys("<Esc>");
    assert!(log.borrow().is_empty(), "a cancel runs nothing");

    ctx.run("command-palette", &Args::default()).unwrap();
    ctx.type_keys("to<D-h>");
    assert_eq!(*log.borrow(), [None], "the DIE layer runs with a prompt open");
    assert_eq!(ctx.prompt_text(), Some("to"), "and the prompt stays");
    ctx.type_keys("<Esc>");
    log.borrow_mut().clear();

    ctx.run("command-palette", &args).unwrap();
    ctx.type_keys("shl");
    assert_eq!(ctx.prompt_chosen(), Some("say-hello"));
    ctx.type_keys("<CR>");
    assert_eq!(*log.borrow(), [Some(3)]);
    assert_eq!(ctx.state(), State::Normal);

    ctx.run("command-palette", &args).unwrap();
    assert_eq!(
        ctx.prompt_chosen(),
        Some("say-hello"),
        "the recent one first"
    );
    ctx.type_keys("<CR>");
    assert_eq!(*log.borrow(), [Some(3), Some(3)], "<CR> runs it again");

    ctx.run("command-palette", &args).unwrap();
    ctx.type_keys("toggle-max<CR>");
    ctx.run("command-palette", &args).unwrap();
    assert_eq!(
        ctx.prompt_candidates()[..2],
        ["toggle-max-width", "say-hello"],
        "the newest first"
    );
    ctx.type_keys("<Esc>");

    ctx.run("command-palette", &Args::default()).unwrap();
    ctx.type_keys("zzzz<CR>");
    assert_eq!(
        ctx.prompt_text(),
        Some("zzzz"),
        "a name that isn't one stays"
    );
}

/// The whole way down this repository, from a buffer at its root to a file in a crate.
#[test]
fn find_file_descends_into_a_crate_and_opens_the_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap();
    let mut ctx = ctx();
    let start = ctx.open_file(&root.join("Cargo.toml")).unwrap();
    ctx.show(start, ShowIn::Current).unwrap();

    ctx.run("find-file", &Args::default()).unwrap();
    let line = ctx.prompt_text().unwrap().to_string();
    assert!(line.ends_with('/'), "{line}");
    assert!(ctx.prompt_candidates().contains(&"crates/"));

    ctx.type_keys("crates<CR>");
    assert_eq!(ctx.prompt_text().unwrap(), format!("{line}crates/"));
    ctx.type_keys("die-core<CR>");
    assert_eq!(
        ctx.prompt_text().unwrap(),
        format!("{line}crates/die-core/")
    );
    let shown = ctx.prompt_candidates();
    for entry in ["Cargo.toml", "src/", "tests/"] {
        assert!(shown.contains(&entry), "{entry} in {shown:?}");
    }

    ctx.type_keys("src<CR>");
    ctx.type_keys("commands.rs<CR>");
    assert_eq!(ctx.prompt_text(), None, "answered");
    let opened = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(opened), Some("commands.rs"));
    assert_eq!(
        ctx.directory(opened),
        Some(root.join("crates/die-core/src").as_path())
    );
}

/// Hidden entries come last, a symlink to a directory descends, a new name is a new file, and a
/// cancel opens nothing.
#[test]
fn find_file_lists_a_directory_and_takes_a_new_name() {
    let dir = std::env::temp_dir().join(format!("die-find-file-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("b.txt"), "b").unwrap();
    fs::write(dir.join(".hidden"), "").unwrap();
    fs::write(dir.join("sub/deep.txt"), "deep").unwrap();
    std::os::unix::fs::symlink(dir.join("sub"), dir.join("link")).unwrap();

    let mut ctx = ctx();
    let start = ctx.focused_buffer().unwrap();
    ctx.set_directory(start, &dir);
    ctx.run("find-file", &Args::default()).unwrap();
    assert_eq!(
        ctx.prompt_candidates(),
        ["b.txt", "link/", "sub/", ".hidden"]
    );

    ctx.type_keys("<Esc>");
    assert_eq!(ctx.focused_buffer(), Some(start), "a cancel opens nothing");

    ctx.run("find-file", &Args::default()).unwrap();
    ctx.type_keys("link<CR>");
    assert_eq!(ctx.prompt_candidates(), ["deep.txt"]);
    let line = ctx.prompt_text().map(str::to_string);
    ctx.type_keys("<CR>");
    assert_eq!(
        ctx.prompt_text().map(str::to_string),
        line,
        "a directory stays"
    );
    ctx.type_keys("new.txt<M-CR>");
    let opened = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(opened), Some("new.txt"));
    assert!(!dir.join("sub/new.txt").exists(), "not on disk until saved");

    fs::remove_dir_all(&dir).unwrap();
}

/// A focused buffer in `here/`, inside a new directory of the test's own, which is returned.
fn in_new_dir(name: &str) -> (Ctx, PathBuf) {
    let dir = std::env::temp_dir().join(format!("die-ff-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("here")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let mut ctx = ctx();
    let start = ctx.focused_buffer().unwrap();
    ctx.set_directory(start, dir.join("here"));
    (ctx, dir)
}

/// Typing over the starting line is how to go elsewhere: `//` starts again from the root.
#[test]
fn find_file_starts_again_at_a_double_slash() {
    let (mut ctx, dir) = in_new_dir("root");
    fs::write(dir.join("far.txt"), "far").unwrap();
    ctx.run("find-file", &Args::default()).unwrap();
    let start = ctx.prompt_text().unwrap().to_string();

    ctx.type_keys(&format!("{}/", dir.display()));
    assert_eq!(ctx.prompt_candidates(), ["far.txt", "here/"]);
    ctx.type_keys("far<Tab>");
    assert_eq!(
        ctx.prompt_text().unwrap(),
        format!("{start}{}/far.txt", dir.display()),
        "what's in front stays on the line"
    );
    ctx.type_keys("<CR>");
    let opened = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.file(opened).unwrap().path, dir.join("far.txt"));
    assert_eq!(ctx.text(opened).unwrap().rope().to_string(), "far");
    fs::remove_dir_all(&dir).unwrap();
}

/// `~/` starts again from the home directory. Nothing is written there.
#[test]
fn find_file_starts_again_at_a_tilde() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let known = fs::read_dir(&home)
        .unwrap()
        .flatten()
        .find_map(|e| e.file_name().into_string().ok());
    let (mut ctx, dir) = in_new_dir("home");
    ctx.run("find-file", &Args::default()).unwrap();
    ctx.type_keys("~/");
    if let Some(known) = known {
        let shown = ctx.prompt_candidates();
        assert!(
            shown.iter().any(|c| c.trim_end_matches('/') == known),
            "{known} in {shown:?}"
        );
    }
    ctx.type_keys("die-no-such-file-k11b<M-CR>");
    let opened = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.directory(opened), Some(home.as_path()));
    fs::remove_dir_all(&dir).unwrap();
}

/// `..` goes by name in the list, as it does when opening, so the list shows what will open.
#[test]
fn find_file_lists_what_it_opens_past_a_symlink() {
    let (mut ctx, dir) = in_new_dir("link");
    fs::create_dir_all(dir.join("away/place")).unwrap();
    fs::write(dir.join("away/only-there.txt"), "real").unwrap();
    fs::write(dir.join("here/beside.txt"), "beside").unwrap();
    std::os::unix::fs::symlink(dir.join("away/place"), dir.join("here/link")).unwrap();

    ctx.run("find-file", &Args::default()).unwrap();
    ctx.type_keys("link<CR>../");
    assert_eq!(ctx.prompt_candidates(), ["beside.txt", "link/"]);
    ctx.type_keys("bes<CR>");
    let opened = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.text(opened).unwrap().rope().to_string(), "beside");
    fs::remove_dir_all(&dir).unwrap();
}

/// `<CR>` on a directory with nothing to choose stays in the prompt, on that directory.
#[test]
fn find_file_stays_on_a_directory_with_nothing_to_choose() {
    let (mut ctx, dir) = in_new_dir("stay");
    fs::create_dir_all(dir.join("here/empty")).unwrap();
    ctx.run("find-file", &Args::default()).unwrap();
    let line = ctx.prompt_text().unwrap().to_string();

    ctx.type_keys("..<CR>");
    assert_eq!(ctx.prompt_text().unwrap(), format!("{line}../"));
    ctx.type_keys("<C-u>");
    ctx.type_keys(&format!("{line}empty<CR><CR>"));
    assert_eq!(ctx.prompt_text().unwrap(), format!("{line}empty/"));
    ctx.type_keys("<C-u>~<CR>");
    assert_eq!(ctx.prompt_text(), Some("~/"));
    ctx.type_keys("x//<CR>");
    assert_eq!(ctx.prompt_text(), Some("~/x//"), "still from the root");
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
    fs::remove_dir_all(&dir).unwrap();
}

/// A new file under a missing directory offers to make it. Without it the open says so, and with
/// it the first save works.
#[test]
fn find_file_offers_to_make_a_missing_directory() {
    let (mut ctx, dir) = in_new_dir("new");
    let missing = dir.join("here/new/deep");

    ctx.run("find-file", &Args::default()).unwrap();
    ctx.type_keys("new/deep/new.txt<M-CR>n<CR>");
    assert_eq!(ctx.prompt_text(), None);
    let opened = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(opened), Some("new.txt"));
    assert!(!missing.exists(), "not asked for");
    let said = format!("Directory {} doesn't exist", missing.display());
    assert_eq!(ctx.messages, [said]);
    ctx.kill(opened);

    let start = ctx.focused_buffer().unwrap();
    ctx.set_directory(start, dir.join("here"));
    ctx.run("find-file", &Args::default()).unwrap();
    ctx.type_keys("new/deep/new.txt<M-CR>y<CR>");
    let opened = ctx.focused_buffer().unwrap();
    assert!(missing.is_dir());
    let t = ctx.text_mut(opened).unwrap();
    t.tx(Origin::User).insert(0, "text").commit().unwrap();
    ctx.save(opened).unwrap();
    assert_eq!(fs::read_to_string(missing.join("new.txt")).unwrap(), "text");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn require_match_keeps_the_prompt_open_on_what_is_not_a_candidate() {
    let mut ctx = ctx();
    ctx.set_frame_size(40, 30);
    let spec = || {
        PromptSpec::new("?")
            .candidates(["alpha", "beta"])
            .require_match()
    };
    let answers = ask(&mut ctx, spec());
    ctx.type_keys("zzz<CR>");
    assert_eq!(ctx.state(), State::CommandLine);
    assert_eq!(ctx.scene().row_text(27), "*/0    ?zzz [Match required]");
    ctx.type_keys("<BS>");
    assert_eq!(ctx.scene().row_text(27), "*/0    ?zz", "until the next key");
    ctx.type_keys("<C-u>al<M-CR>");
    assert_eq!(ctx.prompt_text(), Some("al"), "<M-CR> is held to it too");
    assert!(answers.borrow().is_empty());
    ctx.type_keys("<Tab><M-CR>");
    assert_eq!(*answers.borrow(), [Some("alpha".to_string())]);

    for (keys, want) in [("<CR>", "alpha"), ("<C-n><CR>", "beta"), ("<M-CR>", "")] {
        let answers = ask(&mut ctx, spec());
        ctx.type_keys(keys);
        assert_eq!(*answers.borrow(), [Some(want.to_string())], "{keys}");
    }
}

#[test]
fn switch_buffer_goes_to_the_chosen_buffer_and_makes_a_new_one() {
    let mut ctx = ctx();
    let t = ctx.focused_buffer().unwrap();
    let a = ctx.create_text("alpha", "");
    let b = ctx.create_text("beta", "");
    ctx.show(a, ShowIn::Current).unwrap();
    ctx.show(t, ShowIn::Current).unwrap();
    let w = ctx.focused().unwrap();

    ctx.run("switch-buffer", &Args::default()).unwrap();
    assert_eq!(
        ctx.prompt_candidates(),
        ["alpha", "beta", "t"],
        "the last buffer first, this one last"
    );
    ctx.type_keys("<CR>");
    assert_eq!(ctx.focused_buffer(), Some(a), "<CR> goes back");

    ctx.run("switch-buffer", &Args::default()).unwrap();
    ctx.type_keys("bt<CR>");
    assert_eq!(ctx.focused_buffer(), Some(b));
    assert_eq!(ctx.focused(), Some(w), "in this window");

    ctx.run("switch-buffer", &Args::default()).unwrap();
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.focused_buffer(), Some(b), "a cancel stays");

    ctx.run("switch-buffer", &Args::default()).unwrap();
    ctx.type_keys("new<CR>");
    let new = ctx
        .find_buffer("new")
        .expect("a name no buffer has makes one");
    assert_eq!(ctx.focused_buffer(), Some(new));
}

#[test]
fn kill_buffer_kills_this_one_by_default_or_the_chosen_one() {
    // The log, and then the last buffer there is: a window always has something to show.
    {
        let mut ctx = ctx();
        let t = ctx.focused_buffer().unwrap();
        ctx.messages.push("something".into());
        ctx.scene();
        let log = ctx.find_buffer("*messages*").unwrap();

        ctx.show(log, ShowIn::Current).unwrap();
        ctx.run("kill-buffer", &Args::default()).unwrap();
        ctx.type_keys("<CR>");
        assert!(!ctx.is_live(log), "<CR> takes the buffer it's on, even the log");
        assert_eq!(ctx.focused_buffer(), Some(t));

        ctx.messages.push("again".into());
        ctx.scene();
        let log = ctx.find_buffer("*messages*").expect("it comes back");
        assert_eq!(ctx.text(log).unwrap().rope().to_string(), "again\n");

        ctx.run("kill-buffer", &Args::default()).unwrap();
        ctx.type_keys("<CR>");
        assert_eq!(ctx.buffer_name(ctx.focused_buffer().unwrap()), Some("*scratch*"));
        assert_eq!(ctx.state(), State::Normal);

        ctx.run("switch-buffer", &Args::default()).unwrap();
        assert_eq!(ctx.prompt_candidates(), ["*messages*", "*scratch*"]);
        ctx.type_keys("mess<CR>");
        assert_eq!(ctx.focused_buffer(), Some(log), "the log is on offer");
        ctx.run("kill-buffer", &Args::default()).unwrap();
        ctx.type_keys("<CR>");
        assert_eq!(ctx.buffer_name(ctx.focused_buffer().unwrap()), Some("*scratch*"));
    }

    let mut ctx = ctx();
    let t = ctx.focused_buffer().unwrap();
    let a = ctx.create_text("alpha", "");
    let b = ctx.create_text("beta", "");
    ctx.show(t, ShowIn::Current).unwrap();

    ctx.run("kill-buffer", &Args::default()).unwrap();
    assert_eq!(ctx.prompt_candidates(), ["t", "beta", "alpha"]);
    ctx.type_keys("<Esc>");
    assert!(ctx.is_live(t), "a cancel kills nothing");

    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys("alp<CR>");
    assert!(!ctx.is_live(a));
    assert_eq!(
        ctx.focused_buffer(),
        Some(t),
        "another's kill leaves this window"
    );

    let before = ctx.messages.len();
    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys("zzzz<CR>");
    assert_eq!(
        ctx.messages.len(),
        before + 1,
        "a name that isn't one says so"
    );

    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys("<CR>");
    assert!(!ctx.is_live(t));
    assert_eq!(ctx.focused_buffer(), Some(b), "the window moves on");
}

/// Only a modified buffer with a file asks, and only `yes` kills it, as in Emacs.
#[test]
fn kill_buffer_asks_before_losing_changes_to_a_file() {
    let path = std::env::temp_dir().join(format!("die-kill-buffer-{}", std::process::id()));
    std::fs::write(&path, "text").unwrap();
    let mut ctx = ctx();
    let f = ctx.open_file(&path).unwrap();
    ctx.show(f, ShowIn::Current).unwrap();
    let name = ctx.buffer_name(f).unwrap().to_string();
    ctx.text_mut(f).unwrap().set_modified(true);

    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys("<CR>");
    assert_eq!(ctx.prompt_text(), Some(""), "asks");
    ctx.type_keys("no<CR>");
    assert!(ctx.is_live(f));
    assert_eq!(ctx.state(), State::Normal);

    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys("<CR><Esc>");
    assert!(ctx.is_live(f), "a cancel keeps it");

    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys(&format!("{name}<CR>yes<CR>"));
    assert!(!ctx.is_live(f));
    std::fs::remove_file(&path).unwrap();

    let scratch = ctx.create_text("scratch", "");
    ctx.show(scratch, ShowIn::Current).unwrap();
    ctx.text_mut(scratch).unwrap().set_modified(true);
    ctx.run("kill-buffer", &Args::default()).unwrap();
    ctx.type_keys("<CR>");
    assert!(
        !ctx.is_live(scratch),
        "no file, nothing to lose by Emacs's rule"
    );
}
