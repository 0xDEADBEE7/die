//! K13, through the keys: what die-vim keeps in core comes back with the session.

use die_test_cap as _;

use die_core::{Harness, State};
use die_vim::Vim;

fn editor() -> Harness {
    Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
}

/// Type `keys`, as the owner would, and report what they logged.
fn typed(h: &mut Harness, keys: &str) {
    let log = h.type_keys(keys).unwrap();
    let errors: Vec<_> = log.iter().filter(|k| !k.errors.is_empty()).collect();
    assert!(errors.is_empty(), "{keys}: {errors:?}");
}

fn text(h: &Harness) -> String {
    let buf = h.focused_buffer().unwrap();
    h.text(buf).unwrap().rope().to_string()
}

fn restored(a: &Harness) -> Harness {
    let saved = a.session_json();
    let mut b = editor();
    b.restore_session(&saved).unwrap();
    assert_eq!(b.session_json(), saved);
    b
}

#[test]
fn registers_and_macros_come_back_and_run() {
    let mut a = editor();
    a.open_text("t", "one two\nthree\nfour\n").unwrap();
    typed(&mut a, "\"ayw"); // characters
    typed(&mut a, "j\"byy"); // a line
    typed(&mut a, "qcx0q"); // a macro, whose first run cut the t
    let mut b = restored(&a);
    assert_eq!(text(&b), "one two\nhree\nfour\n");

    typed(&mut b, "G@c");
    assert_eq!(text(&b), "one two\nhree\nour\n");
    typed(&mut b, "\"bp");
    assert_eq!(text(&b), "one two\nhree\nour\nthree\n");
    typed(&mut b, "gg\"aP");
    assert_eq!(text(&b), "one one two\nhree\nour\nthree\n");
}

#[test]
fn histories_come_back_and_are_recalled() {
    let mut a = editor();
    a.open_text("t", "aa\nbb\ncc\n").unwrap();
    typed(&mut a, "/bb<CR>");
    typed(&mut a, ":3<CR>");
    let mut b = restored(&a);
    assert_eq!(b.prompt_history("search"), a.prompt_history("search"));
    assert_eq!(b.prompt_history("ex"), a.prompt_history("ex"));

    // `@:` is the last ex line, and `/` with <Up> is the last search.
    typed(&mut b, "gg@:x");
    assert_eq!(text(&b), "aa\nbb\nc\n");
    typed(&mut b, "gg/<Up><CR>x");
    assert_eq!(text(&b), "aa\nb\nc\n");
}

#[test]
fn a_visual_selection_comes_back_and_goes_on() {
    let mut a = editor();
    a.open_text("t", "one two\nthree\nfour\n").unwrap();
    typed(&mut a, "vll");
    let mut b = restored(&a);
    assert_eq!(b.state(), State::Visual);
    typed(&mut b, "ld");
    assert_eq!(text(&b), "two\nthree\nfour\n");
    assert_eq!(b.state(), State::Normal);

    let mut a = editor();
    a.open_text("t", "one two\nthree\nfour\n").unwrap();
    typed(&mut a, "Vj");
    let mut b = restored(&a);
    typed(&mut b, "d");
    assert_eq!(text(&b), "four\n");
}

/// Insert state and a half-typed operator wait for what's gone, so they don't come back.
#[test]
fn other_states_do_not_come_back() {
    let mut a = editor();
    a.open_text("t", "text\n").unwrap();
    typed(&mut a, "ihello");
    let saved = a.session_json();
    let mut b = editor();
    b.restore_session(&saved).unwrap();
    assert_eq!(b.state(), State::Normal);
    assert_eq!(text(&b), "hellotext\n");
    typed(&mut b, "x"); // a command, not a character
    assert_eq!(text(&b), "helloext\n");
}
