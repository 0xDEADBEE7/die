//! The ex line commands and insert state's Emacs keys where the oracle can't reach: messages,
//! `*help*`, a range given backward, `C-g`, which ends the oracle's batch Emacs, and how long `:g`
//! takes on a big text. The oracle holds the rest, in `ex-lines.json`, `insert-emacs.json` and
//! `insert-emacs-tty.json`.

use die_test_cap as _;

use die_core::{Args, BufferId, Ctx, Extension, Harness, Registry, Result, State, WindowId};
use die_vim::Vim;
use std::time::{Duration, Instant};

fn editor(text: &str, cursor: usize) -> (Harness, BufferId, WindowId) {
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let (b, w) = h.open_text("t", text).unwrap();
    assert_eq!(h.text_mut(b).unwrap().set_cursor(w, cursor), Some(cursor));
    (h, b, w)
}

/// Type `keys` and return the errors they gave.
fn keys(h: &mut Harness, keys: &str) -> Vec<String> {
    let log = h.type_keys(keys).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

#[test]
fn a_range_given_backward_is_refused() {
    let (mut h, b, _) = editor("a\nb\nc\n", 0);
    for line in [":3,1d<CR>", ":3,1m0<CR>", ":2,1sort<CR>", ":3,1><CR>"] {
        let errors = keys(&mut h, line);
        assert_eq!(errors.len(), 1, "{line}: {errors:?}");
        assert!(errors[0].contains("Backward range"), "{line}: {errors:?}");
        assert_eq!(text(&h, b), "a\nb\nc\n", "{line}");
    }
    assert!(h.register('"').is_none(), "the refused :d kept nothing");
}

#[test]
fn a_bad_sort_pattern_changes_nothing() {
    let (mut h, b, _) = editor("b\na\n", 0);
    assert_eq!(keys(&mut h, ":sort /(/<CR>").len(), 1);
    assert_eq!(text(&h, b), "b\na\n");
}

/// An extension with a command whose name starts with `move`, as `:m`'s does.
struct MoveIt;

impl Extension for MoveIt {
    fn name(&self) -> &'static str {
        "move-it"
    }
    fn register(&mut self, reg: &mut Registry) {
        fn say(ctx: &mut Ctx, a: &Args) -> Result {
            ctx.messages.push(format!("move-it ran with {:?}", a.text));
            Ok(())
        }
        reg.command("move-it", "Says it ran.", say);
    }
}

#[test]
fn a_name_that_goes_on_is_another_command() {
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(MoveIt)]).unwrap();
    let (b, _) = h.open_text("t", "a\nb\n").unwrap();
    assert!(keys(&mut h, ":move-it far<CR>").is_empty());
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("move-it ran with Some(\"far\")")
    );
    assert_eq!(text(&h, b), "a\nb\n");
    // With no command of that name it's `:m` with an address of `-nope`, as in your Evil
    // (the oracle's `name-dash-unknown` has it fail too).
    assert_eq!(keys(&mut h, ":move-nope<CR>"), ["Invalid address"]);
    assert_eq!(text(&h, b), "a\nb\n");
}

#[test]
fn g_moving_every_line_of_a_big_text_is_quick() {
    let lines: Vec<String> = (0..5_000).map(|i| format!("line {i}")).collect();
    let (mut h, b, _) = editor(&(lines.join("\n") + "\n"), 0);
    let start = Instant::now();
    assert!(keys(&mut h, ":g/^/m0<CR>").is_empty());
    let took = start.elapsed();
    let reversed: Vec<String> = lines.into_iter().rev().collect();
    assert_eq!(text(&h, b), reversed.join("\n") + "\n");
    assert!(took < Duration::from_secs(10), "took {took:?}");
    assert!(keys(&mut h, "u").is_empty());
    assert!(
        text(&h, b).starts_with("line 0\nline 1\n"),
        "one u undoes it all"
    );
}

#[test]
fn c_g_in_insert_state_drops_a_count_and_stays() {
    let (mut h, b, _) = editor("ab\n", 0);
    assert!(keys(&mut h, "i<C-u>3<C-g>x").is_empty());
    assert_eq!(h.state(), State::Insert);
    assert_eq!(h.messages.last().map(String::as_str), Some("Quit"));
    assert!(keys(&mut h, "<Esc>").is_empty());
    assert_eq!(text(&h, b), "xab\n");
}

/// What a `C-g` in insert state does to `.`, a count and the other lines, as the owner's Evil did
/// it in batch Emacs, its keys typed one by one with the quit caught: `keyboard-quit`'s advice,
/// `evil-repeat-abort`, ends the session's recording there.
#[test]
fn c_g_ends_what_dot_and_a_count_repeat() {
    for (start, typed, want) in [
        // `.` repeats the change before.
        ("x\ny\n", "iab<Esc>j0iQ<C-g><Esc>k0.", "ababx\nQy\n"),
        // The count and the other lines put in what came before the `C-g`.
        ("x\n", "3ia<C-g>b<Esc>", "abaax\n"),
        ("x\n", "3oa<C-g>b<Esc>", "x\nab\na\na\n"),
        ("x\n", "3Oa<C-g>b<Esc>", "ab\na\na\nx\n"),
        ("xyzuvw\n", "3Ra<C-g>b<Esc>", "abaavw\n"),
        (
            "abc\ndef\nghi\n",
            "<C-v>jjIa<C-g>b<Esc>",
            "ababc\nadef\naghi\n",
        ),
        // Register `.` still has all of it.
        ("x\n", "ia<C-g>b<Esc>\".p", "ababx\n"),
    ] {
        let (mut h, b, _) = editor(start, 0);
        assert!(keys(&mut h, typed).is_empty(), "{typed}");
        assert_eq!(text(&h, b), want, "{typed}");
        assert_eq!(h.state(), State::Normal, "{typed}");
    }
    // With no change before, `.` has nothing to repeat, as your Evil's ring is empty.
    let (mut h, b, _) = editor("x\ny\n", 0);
    assert!(keys(&mut h, "iab<C-g>c<Esc>j0").is_empty());
    assert_eq!(keys(&mut h, ".").len(), 1, "nothing to repeat");
    assert_eq!(text(&h, b), "abcx\ny\n");
}

#[test]
fn c_c_and_s_tab_are_undefined_and_swallow_the_key_after_c_c() {
    let (mut h, b, _) = editor("ab\n", 0);
    assert!(keys(&mut h, "i<C-c>x").is_empty());
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("C-c x is undefined")
    );
    assert!(keys(&mut h, "<S-Tab>").is_empty());
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("<backtab> is undefined")
    );
    assert!(keys(&mut h, "y<Esc>").is_empty());
    assert_eq!(text(&h, b), "yab\n");
}

#[test]
fn c_h_k_describes_a_key_of_insert_state() {
    let (mut h, b, _) = editor("ab\n", 0);
    assert!(keys(&mut h, "i<C-h>k<C-p>").is_empty());
    assert_eq!(h.state_of(b), State::Insert, "still inserting");
    let help = h.find_buffer("*help*").expect("*help* is shown");
    let shown = text(&h, help);
    assert!(shown.contains("vim-complete-previous"), "{shown}");
}

#[test]
fn c_h_x_asks_for_a_command_and_others_are_not_built() {
    let (mut h, _, _) = editor("ab\n", 0);
    assert!(keys(&mut h, "i<C-h>x").is_empty());
    assert!(h.prompt_text().is_some(), "describe-command asks");
    assert!(keys(&mut h, "<C-g><Esc>").is_empty());
    let errors = keys(&mut h, "i<C-h>q");
    assert_eq!(errors.len(), 1, "{errors:?}");
}

#[test]
fn line_0_is_the_first_line_and_a_recording_puts_its_keys() {
    let (mut h, b, _) = editor("a\nb\n", 2);
    assert!(keys(&mut h, ":0d<CR>").is_empty());
    assert_eq!(text(&h, b), "b\n");
    assert!(keys(&mut h, "qqA!<Esc>q:pu q<CR>").is_empty());
    assert_eq!(text(&h, b), "b!\nA!\u{1b}\n");
}
