//! Macros where the Evil oracle can't reach: what the screen shows while `q` records, the
//! `*registers*` buffer `:reg` opens, `:norm!`, a typed `C-g`, keys typed while `:norm` runs and a
//! search inside it, and keys a line leaves waiting. tools/evil-oracle/cases/macros.json holds the
//! rest.

use die_core::{BufferId, Harness, Input, RegKind, Register, State, parse_keys};
use die_vim::Vim;
use std::time::Duration;

use die_test_cap as _;

fn editor(text: &str) -> (Harness, BufferId) {
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    h.set_window_size(40, 6);
    let (b, _) = h.open_text("t", text).unwrap();
    (h, b)
}

fn keys(h: &mut Harness, k: &str) -> Vec<String> {
    let log = h.type_keys(k).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

fn bottom(h: &mut Harness) -> String {
    h.screen().lines().last().unwrap_or_default().to_string()
}

/// Queue keys as if typed all at once, before any of them has run.
fn queue(h: &mut Harness, k: &str) {
    for k in parse_keys(k).unwrap() {
        h.handle(Input::Key(k));
    }
}

fn watchdog(secs: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        eprintln!("hung for {secs} s");
        std::process::abort();
    });
}

#[test]
fn recording_shows_on_the_bottom_row_until_q_stops_it() {
    let (mut h, _) = editor("one\ntwo\n");
    assert!(!bottom(&mut h).contains("recording"));
    keys(&mut h, "qa");
    assert!(bottom(&mut h).ends_with("recording @a"), "{:?}", h.screen());
    // A message goes beside it, and the next key takes the message away but not the note.
    assert_eq!(keys(&mut h, "k").len(), 1);
    let row = bottom(&mut h);
    assert!(row.starts_with("Beginning of buffer") && row.ends_with("recording @a"));
    keys(&mut h, "jx");
    assert_eq!(bottom(&mut h).trim(), "recording @a");
    // A prompt has the row while it's open.
    keys(&mut h, ":");
    assert!(!bottom(&mut h).contains("recording"));
    keys(&mut h, "<Esc>");
    assert!(bottom(&mut h).ends_with("recording @a"));
    keys(&mut h, "q");
    assert!(!bottom(&mut h).contains("recording"), "{:?}", h.screen());
    keys(&mut h, "qA");
    assert!(bottom(&mut h).ends_with("recording @A"));
    keys(&mut h, "q");
    assert_eq!(h.register('a').map(|r| r.text.as_str()), Some("kjx:<Esc>"));
}

#[test]
fn a_long_message_stops_short_of_the_note() {
    let (mut h, _) = editor("one\n");
    keys(&mut h, "qz:frobnicate-every-last-thing-in-sight<CR>");
    let row = bottom(&mut h);
    assert!(row.ends_with(" recording @z"), "{row:?}");
    assert_eq!(row.chars().count(), 40);
}

#[test]
fn reg_lists_every_register_as_evils_does() {
    let (mut h, b) = editor("one two\nthree\n");
    h.set_register(
        'a',
        Register {
            kind: RegKind::Char,
            text: "hello\nworld".into(),
        },
    );
    keys(&mut h, "qbA!<Esc>jq");
    keys(&mut h, "yiw");
    assert!(keys(&mut h, ":reg<CR>").is_empty());
    let shown = h.focused_buffer().unwrap();
    assert_ne!(shown, b);
    assert_eq!(h.buffer_name(shown), Some("*registers*"));
    let want = [
        "Register   Value",
        "\"          three",
        "#          ",
        "%          ",
        "*          ",
        "+          three",
        "-          ",
        ".          !",
        "/          ",
        "0          three",
        "1          three",
        "2          ",
        "3          ",
        "4          ",
        "5          ",
        "6          ",
        "7          ",
        "8          ",
        "9          ",
        ":          reg",
        "=          ",
        "a          hello^Jworld",
        "b          A!<Esc>j",
        "",
    ];
    assert_eq!(text(&h, shown), want.join("\n"));
    assert!(h.text(shown).unwrap().settings().read_only);
    // Named registers only, and the same buffer again.
    keys(&mut h, ":reg ab<CR>");
    assert_eq!(h.focused_buffer(), Some(shown));
    let want = "Register   Value\na          hello^Jworld\nb          A!<Esc>j\n";
    assert_eq!(text(&h, shown), want);
    keys(&mut h, ":di b<CR>");
    assert_eq!(text(&h, shown), "Register   Value\nb          A!<Esc>j\n");
    assert_eq!(h.window_list().len(), 2, "one window for it");
}

#[test]
fn norm_bang_is_norm_as_in_vim() {
    let (mut h, b) = editor("abc\ndef\n");
    assert!(keys(&mut h, ":%norm! x<CR>").is_empty());
    assert_eq!(text(&h, b), "bc\nef\n");
    keys(&mut h, ":%normal!  x<CR>");
    assert_eq!(text(&h, b), "c\nf\n");
}

#[test]
fn a_typed_c_g_stops_the_whole_norm() {
    watchdog(120);
    let (mut h, b) = editor("abc\ndef\nghi\n");
    for k in parse_keys(":%norm Ax<CR><C-g>").unwrap() {
        h.handle(Input::Key(k));
    }
    while h.run_ready() {}
    assert_eq!(text(&h, b), "abc\ndef\nghi\n", "no line after the quit");
    assert_eq!(h.messages.last().map(String::as_str), Some("Quit"));
    assert_eq!(h.state(), State::Normal);
    // Nothing of it is left: the next keys are as typed, and a failure says so again.
    keys(&mut h, "x");
    assert_eq!(text(&h, b), "bc\ndef\nghi\n");
    assert_eq!(keys(&mut h, "gg0h").len(), 1);
}

#[test]
fn a_typed_c_g_after_a_line_stops_the_rest() {
    watchdog(120);
    let (mut h, b) = editor("abc\ndef\nghi\n");
    queue(&mut h, ":%norm A!<C-v><Esc><CR>");
    // A turn at a time, until the first line's keys have run.
    while text(&h, b) == "abc\ndef\nghi\n" {
        assert!(h.step(), "the first line never ran");
    }
    while h.state() != State::Normal {
        assert!(h.step());
    }
    queue(&mut h, "<C-g>");
    while h.run_ready() {}
    assert_eq!(text(&h, b), "abc!\ndef\nghi\n", "no line after the quit");
    assert_eq!(h.messages.last().map(String::as_str), Some("Quit"));
    assert_eq!(h.state(), State::Normal);
    // The line that ran is one step for `u`, and what's typed next isn't hushed.
    keys(&mut h, "u");
    assert_eq!(text(&h, b), "abc\ndef\nghi\n");
    assert_eq!(keys(&mut h, "0h").len(), 1);
}

#[test]
fn keys_typed_while_a_norm_runs_wait_for_it() {
    // They're queued before the run starts, so they're there when a line's keys run out, a
    // prompt still open. The prompt takes its line as it is, and they go after.
    let (mut h, b) = editor("abc\ndef\nghi\n");
    queue(&mut h, ":%norm :s/./X/<CR>x");
    while h.run_ready() {}
    assert_eq!(text(&h, b), "Xbc\nXef\nhi\n");
    assert_eq!(h.state(), State::Normal);
    queue(&mut h, ":%norm f<CR>x");
    while h.run_ready() {}
    assert_eq!(
        text(&h, b),
        "Xbc\nXef\ni\n",
        "x after the f, not its character"
    );
}

#[test]
fn a_search_runs_on_each_line_of_a_norm() {
    // The oracle's batch Emacs dies on a search in `:norm`.
    let (mut h, b) = editor("abc\ndef\ncgh\n");
    assert!(keys(&mut h, ":%norm /c<C-v><CR>x<CR>").is_empty());
    // From the second line the search finds the third's c, and from the third there's none left,
    // which fails without a word.
    assert_eq!(text(&h, b), "ab\ndef\ngh\n");
    assert_eq!(h.state(), State::Normal);
    keys(&mut h, "u");
    assert_eq!(text(&h, b), "abc\ndef\ncgh\n");
}

#[test]
fn a_key_left_waiting_ends_with_its_line() {
    let (mut h, b) = editor("abc\ndefghij\n");
    for line in [
        ":%norm f<CR>",
        ":%norm @<CR>",
        ":%norm \"<CR>",
        ":%norm g<CR>",
        ":%norm dg<CR>",
    ] {
        assert!(keys(&mut h, line).is_empty(), "{line}");
        assert_eq!(h.state(), State::Normal, "{line}");
        keys(&mut h, "x");
    }
    assert_eq!(
        text(&h, b),
        "abc\nij\n",
        "each x deleted one character, as typed"
    );
}

#[test]
fn norm_and_g_go_through_thousands_of_lines() {
    watchdog(120);
    let lines: String = (0..5_000).map(|i| format!("line {i}\n")).collect();
    let (mut h, b) = editor(&lines);
    h.set_register(
        'a',
        Register {
            kind: RegKind::Macro,
            text: "A;<Esc>".into(),
        },
    );
    assert!(keys(&mut h, ":%norm @a<CR>").is_empty());
    assert!(text(&h, b).lines().all(|l| l.ends_with(';')));
    assert!(keys(&mut h, ":g/7/norm x<CR>").is_empty());
    let t = text(&h, b);
    assert_eq!(t.lines().filter(|l| l.starts_with("ine")).count(), 1_355);
    keys(&mut h, "uu");
    assert_eq!(text(&h, b), lines);
}

#[test]
fn a_norm_that_starts_itself_ends() {
    watchdog(120);
    let (mut h, b) = editor("abc\n");
    h.set_register(
        'a',
        Register {
            kind: RegKind::Macro,
            text: ":norm @a<CR>".into(),
        },
    );
    keys(&mut h, ":norm @a<CR>");
    assert_eq!(h.state(), State::Normal);
    assert!(keys(&mut h, "x").is_empty());
    assert_eq!(text(&h, b), "bc\n");
    assert_eq!(keys(&mut h, "0h").len(), 1, "a failure says so again");
}

#[test]
fn using_dies_examples_do_what_it_says() {
    let (mut h, b) = editor("a\n\nb\nc\n");
    assert!(keys(&mut h, ":v/^$/norm A;<CR>").is_empty());
    assert_eq!(text(&h, b), "a;\n\nb;\nc;\n");
    // A macro edited as text, then played. It's recorded on the last line, where `:norm` ended.
    keys(&mut h, "qqA!<Esc>q");
    keys(&mut h, "Go<Esc>\"qp$i?<Esc>0\"qy$dd");
    assert_eq!(h.register('q').map(|r| r.text.as_str()), Some("A!?\u{1b}"));
    keys(&mut h, "gg@q");
    assert_eq!(text(&h, b), "a;!?\n\nb;\nc;!\n");
    let (mut h, b) = editor("TODO a\nb\nTODO c\nd\ne\nf\n");
    assert!(keys(&mut h, ":%norm A;<CR>").is_empty());
    assert_eq!(text(&h, b), "TODO a;\nb;\nTODO c;\nd;\ne;\nf;\n");
    // Recorded on the last line, where `:norm` ended.
    keys(&mut h, "qqI-<Esc>q");
    assert!(keys(&mut h, ":g/TODO/norm @q<CR>").is_empty());
    assert_eq!(text(&h, b), "-TODO a;\nb;\n-TODO c;\nd;\ne;\n-f;\n");
    assert!(keys(&mut h, "jVj:norm @q<CR>").is_empty());
    assert_eq!(text(&h, b), "-TODO a;\nb;\n-TODO c;\n-d;\n-e;\n-f;\n");
    keys(&mut h, "ggx");
    assert!(keys(&mut h, ":5,$norm .<CR>").is_empty());
    assert_eq!(text(&h, b), "TODO a;\nb;\n-TODO c;\n-d;\ne;\nf;\n");
}
