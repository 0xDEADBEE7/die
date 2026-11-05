//! K11a: the command line's editing keys, history and answers, and `*messages*`.

use die_test_cap as _;

use die_core::{Ctx, CursorShape, Error, PromptSpec, State};
use std::cell::RefCell;
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
