//! Search and moving between files where the Evil oracle can't reach: what's drawn, the view
//! while a pattern is typed, hidden text, `.`, and `:e` and `:b`, which need other buffers.

use die_test_cap as _;

use die_core::{BufferId, Deco, Harness, Reveal, State, WindowId};
use die_vim::Vim;

fn editor(text: &str) -> (Harness, BufferId, WindowId) {
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let (b, w) = h.open_text("t", text).expect("a window");
    h.text_mut(b).unwrap().set_cursor(w, 0);
    (h, b, w)
}

fn cursor(h: &Harness, b: BufferId, w: WindowId) -> usize {
    h.text(b).unwrap().cursor(w).unwrap()
}

fn typed(h: &mut Harness, keys: &str) -> Vec<String> {
    let log = h.type_keys(keys).expect("the notation parses");
    log.into_iter().flat_map(|k| k.errors).collect()
}

#[test]
fn the_cursor_follows_the_pattern_as_it_is_typed_and_a_cancel_puts_it_back() {
    let (mut h, b, w) = editor("foo bar baz\n");
    assert!(typed(&mut h, "/ba").is_empty());
    assert_eq!(cursor(&h, b, w), 4);
    assert!(typed(&mut h, "z").is_empty());
    assert_eq!(cursor(&h, b, w), 8);
    assert!(typed(&mut h, "<Esc>").is_empty());
    assert_eq!(cursor(&h, b, w), 0);
    assert_eq!(h.state(), State::Normal);
}

#[test]
fn every_match_is_highlighted_and_the_one_under_the_cursor_differs() {
    let (mut h, _, _) = editor("ab x ab y ab\n");
    typed(&mut h, "/ab<CR>");
    let s = h.scene();
    let bg = |x: usize| s.row(0)[x].bg;
    assert_eq!(bg(0), bg(10), "two matches");
    assert_ne!(bg(0), bg(3), "a match and plain text");
    assert_ne!(bg(5), bg(0), "the current match");
}

#[test]
fn a_cancelled_pattern_takes_its_highlight_with_it() {
    let (mut h, _, _) = editor("ab x\n");
    let plain = h.scene().row(0)[1].bg;
    typed(&mut h, "/x");
    assert_ne!(h.scene().row(0)[3].bg, plain, "while it's typed");
    typed(&mut h, "<Esc>");
    assert_eq!(h.scene().row(0)[3].bg, plain);
}

#[test]
fn an_operator_goes_when_its_search_is_cancelled_or_finds_nothing() {
    let (mut h, b, _) = editor("foo bar\n");
    typed(&mut h, "d/<Esc>");
    assert_eq!(h.state(), State::Normal);
    assert_eq!(typed(&mut h, "d/zzz<CR>"), ["Search failed: zzz"]);
    assert_eq!(h.state(), State::Normal);
    assert_eq!(typed(&mut h, "d/(<CR>").len(), 1, "a bad pattern");
    assert_eq!(h.state(), State::Normal);
    assert_eq!(h.text(b).unwrap().rope().to_string(), "foo bar\n");
}

#[test]
fn dot_repeats_an_operator_with_what_was_typed_into_its_search() {
    let (mut h, b, _) = editor("a x b x c\n");
    typed(&mut h, "d/x y<BS><BS><CR>");
    assert_eq!(h.text(b).unwrap().rope().to_string(), "x b x c\n");
    typed(&mut h, "l.");
    assert_eq!(h.text(b).unwrap().rope().to_string(), "xx c\n");
}

#[test]
fn n_gets_past_a_match_on_the_newline_the_cursor_rests_before() {
    let (mut h, b, w) = editor("ab\ncd\n");
    typed(&mut h, "/$<CR>");
    assert_eq!(cursor(&h, b, w), 1);
    typed(&mut h, "n");
    assert_eq!(cursor(&h, b, w), 4);
}

#[test]
fn star_goes_by_die_vims_word_test_and_n_keeps_it() {
    let (mut h, b, w) = editor("foo foobar foo_bar foo\n");
    typed(&mut h, "*");
    assert_eq!(cursor(&h, b, w), 11, "inside foo_bar, but not foobar");
    typed(&mut h, "n");
    assert_eq!(cursor(&h, b, w), 19);
    typed(&mut h, "#");
    assert_eq!(cursor(&h, b, w), 11, "# goes back, by the same test");
}

#[test]
fn a_regex_is_rusts() {
    let (mut h, b, w) = editor("a foo b bar\n");
    typed(&mut h, "/(bar|baz)<CR>");
    assert_eq!(cursor(&h, b, w), 8, "a group needs no backslash");
}

#[test]
fn a_match_in_hidden_text_is_revealed() {
    let (mut h, b, w) = editor("one\ntwo\nfoo\nthree\n");
    h.text_mut(b)
        .unwrap()
        .decorate(
            4..12,
            Deco::Hide {
                reveal: Reveal::Remove,
                replacement: None,
            },
        )
        .unwrap();
    typed(&mut h, "/foo<CR>");
    assert_eq!(cursor(&h, b, w), 8);
}

#[test]
fn colon_e_opens_a_file_from_the_buffers_directory() {
    let dir = std::env::temp_dir().join(format!("die-vim-search-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hello.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let (mut h, b, w) = editor("x\n");
    h.set_directory(b, &dir);
    assert!(typed(&mut h, ":e hello.txt<CR>").is_empty());
    let opened = h.window_buffer(w).unwrap();
    assert_ne!(opened, b);
    assert_eq!(h.text(opened).unwrap().rope().to_string(), "hello\n");
    assert_eq!(typed(&mut h, ":e<CR>").len(), 1, "no name");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn colon_b_goes_by_name_part_of_a_name_or_to_the_last_buffer() {
    let (mut h, a, w) = editor("a\n");
    let notes = h.create_text("notes.md", "n\n");
    let nope = h.create_text("nope.md", "n\n");
    assert!(typed(&mut h, ":b notes.md<CR>").is_empty());
    assert_eq!(h.window_buffer(w), Some(notes));
    assert!(typed(&mut h, ":b ope<CR>").is_empty());
    assert_eq!(h.window_buffer(w), Some(nope));
    assert!(typed(&mut h, ":b<CR>").is_empty());
    assert_eq!(h.window_buffer(w), Some(notes));
    assert_eq!(typed(&mut h, ":b .md<CR>").len(), 1, "more than one");
    assert_eq!(typed(&mut h, ":b zzz<CR>").len(), 1, "none");
    assert_eq!(h.window_buffer(w), Some(notes));
    assert!(h.is_live(a));
}
