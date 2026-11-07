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

/// Insert and replace state wait for what's gone, so they don't come back: a restore leaves them
/// as Escape does, cursor included.
#[test]
fn other_states_do_not_come_back() {
    for (start, keys) in [
        ("text\n", "ihello"),
        ("one\ntwo\n", "Ahello"),
        ("text\n", "i"),
        ("text\n", "Rab"),
    ] {
        let mut a = editor();
        a.open_text("t", start).unwrap();
        typed(&mut a, keys);
        let saved = a.session_json();
        let mut b = editor();
        b.restore_session(&saved).unwrap();
        assert_eq!(b.state(), State::Normal, "{keys}");
        assert_eq!(text(&b), text(&a), "{keys}");
        typed(&mut a, "<Esc>x");
        typed(&mut b, "x"); // a command, not a character
        assert_eq!(text(&b), text(&a), "{keys}");
    }
}

/// The idle write catches an insert session, whose cursor can be on a newline. Normal state's can't.
#[test]
fn a_cursor_saved_in_an_insert_session_comes_back_where_normal_state_rests() {
    let mut a = editor();
    a.open_text("t", "abc\ndef\n").unwrap();
    typed(&mut a, "AZ");
    let mut b = editor();
    b.restore_session(&a.session_json()).unwrap();
    assert_eq!(b.state(), State::Normal);
    typed(&mut b, "x");
    assert_eq!(text(&b), "abc\ndef\n");
}

fn cursor(h: &Harness) -> usize {
    let buf = h.focused_buffer().unwrap();
    let win = h.focused().unwrap();
    h.text(buf).unwrap().selection(win).unwrap().head
}

/// U8: every kind of `.`: its keys, its count and register, and a selection's size.
#[test]
fn dot_comes_back_whole() {
    let mut a = editor();
    a.open_text("t", "one two\none two\none two\none two\none two\n")
        .unwrap();
    typed(&mut a, "cwnew<Esc>");
    let mut b = restored(&a);
    typed(&mut b, "j0.");
    assert_eq!(text(&b), "new two\nnew two\none two\none two\none two\n");

    typed(&mut b, "j0\"a2x");
    let mut c = restored(&b);
    typed(&mut c, "j0.");
    assert_eq!(text(&c), "new two\nnew two\ne two\ne two\none two\n");
    typed(&mut c, "\"ap");
    assert_eq!(text(&c), "new two\nnew two\ne two\neon two\none two\n");

    // A count on `.` lasts for that `.`, and a restart doesn't make it the change's own.
    let mut d = editor();
    d.open_text("t", "abcdef\nabcdef\nabcdef\n").unwrap();
    typed(&mut d, "x3.");
    let mut e = restored(&d);
    typed(&mut e, "j0.");
    assert_eq!(text(&e), "ef\nbcdef\nabcdef\n");

    // A visual operator's `.` selects as much again.
    let mut f = editor();
    f.open_text("t", "abcdef\nabcdef\n").unwrap();
    typed(&mut f, "vld");
    let mut g = restored(&f);
    typed(&mut g, "j0.");
    assert_eq!(text(&g), "cdef\ncdef\n");
}

#[test]
fn at_at_comes_back() {
    let mut a = editor();
    a.open_text("t", "abc\nabc\nabc\n").unwrap();
    typed(&mut a, "qqxq");
    typed(&mut a, "j0@q");
    let mut b = restored(&a);
    typed(&mut b, "j0@@");
    assert_eq!(text(&b), "bc\nbc\nbc\n");
}

/// `;` as `.` left it, too.
#[test]
fn semicolon_comes_back() {
    let mut a = editor();
    a.open_text("t", "a,b,c,d\n").unwrap();
    typed(&mut a, "f,");
    let mut b = restored(&a);
    typed(&mut b, ";");
    assert_eq!(cursor(&b), 3);

    let mut c = editor();
    c.open_text("t", "xa,b,xa,b,c\n").unwrap();
    typed(&mut c, "dfaf,.");
    assert_eq!(text(&c), ",b,b,c\n");
    let mut d = restored(&c);
    typed(&mut d, "0;");
    assert_eq!(cursor(&d), 2);
}

#[test]
fn comma_comes_back() {
    let mut a = editor();
    a.open_text("t", "a,b,c,d\n").unwrap();
    typed(&mut a, "$F,");
    assert_eq!(cursor(&a), 5);
    let mut b = restored(&a);
    typed(&mut b, ";");
    assert_eq!(cursor(&b), 3);
    typed(&mut b, ",");
    assert_eq!(cursor(&b), 5);
}

/// A `*` search stays a search for that word, which a pattern alone wouldn't.
#[test]
fn n_comes_back() {
    let mut a = editor();
    a.open_text("t", "foo x\nfoo y\nfoo z\n").unwrap();
    typed(&mut a, "/foo<CR>");
    assert_eq!(cursor(&a), 6);
    let mut b = restored(&a);
    typed(&mut b, "n");
    assert_eq!(cursor(&b), 12);

    let mut c = editor();
    c.open_text("t", "foo foobar foo\n").unwrap();
    typed(&mut c, "*");
    assert_eq!(cursor(&c), 11);
    let mut d = restored(&c);
    typed(&mut d, "0n");
    assert_eq!(cursor(&d), 11, "foobar isn't foo");

    // `&` uses `n`'s pattern, so its replacement has to come back with it, or `&` would delete.
    let mut e = editor();
    e.open_text("t", "aa\naa\n").unwrap();
    typed(&mut e, ":s/a/x/<CR>");
    let mut f = restored(&e);
    typed(&mut f, "j&");
    assert_eq!(text(&f), "xa\nxa\n");
}

/// `?` went backward, so `N` goes forward, and `gd`'s symbol comes back a symbol.
#[test]
fn big_n_comes_back() {
    let mut a = editor();
    a.open_text("t", "foo x\nfoo y\nfoo z\n").unwrap();
    typed(&mut a, "G?foo<CR>");
    assert_eq!(cursor(&a), 6);
    typed(&mut a, "gg");
    let mut b = restored(&a);
    typed(&mut b, "N");
    assert_eq!(cursor(&b), 6);
    typed(&mut b, "n");
    assert_eq!(cursor(&b), 0);

    let mut c = editor();
    c.open_text("t", "a-b a-b a-bc\n").unwrap();
    typed(&mut c, "gd");
    assert_eq!(cursor(&c), 0);
    let mut d = restored(&c);
    typed(&mut d, "N");
    assert_eq!(cursor(&d), 4, "a-bc isn't a-b");
}

#[test]
fn gv_comes_back_in_each_buffer_with_its_shape() {
    let mut a = editor();
    a.open_text("one", "aaa\nbbb\nccc\n").unwrap();
    typed(&mut a, "Vj<Esc>G");
    a.open_text("two", "abcd\nabcd\nabcd\n").unwrap();
    typed(&mut a, "l<C-v>jl<Esc>");
    let mut b = restored(&a);
    typed(&mut b, "gvd");
    assert_eq!(text(&b), "ad\nad\nabcd\n");
    typed(&mut b, ":b one<CR>gvd");
    assert_eq!(text(&b), "ccc\n");
}

/// A session from before U8 has none of it, and one with nonsense in it costs only that.
#[test]
fn a_session_without_them_or_with_nonsense_still_restores() {
    let mut a = editor();
    a.restore_session(r#"{"version":1,"focus":0,"buffers":[{"name":"b","text":"x y\n"}]}"#)
        .unwrap();
    assert_eq!(text(&a), "x y\n");
    for keys in [".", "@@", ";", ",", "n", "N"] {
        let log = a.type_keys(keys).unwrap();
        let said: Vec<_> = log.iter().flat_map(|k| &k.errors).collect();
        assert!(said.len() <= 1, "{keys}: {said:?}");
    }
    typed(&mut a, "gv");
    assert_eq!(
        a.state_of(a.focused_buffer().unwrap()),
        State::Normal,
        "no selection yet"
    );

    let mut b = editor();
    let json = r#"{"version":1,"focus":0,
        "extensions":{"die-vim":{"dot":{"keys":7},"find":"x","search":{"pattern":"(","forward":true},
            "macro":"q"}},
        "buffers":[{"name":"b","text":"x y\n","extensions":{"die-vim":{"gv":{"anchor":"no"}}}},
                   {"name":"c","text":"z\n","extensions":12}]}"#;
    b.restore_session(json).unwrap();
    assert!(b.find_buffer("b").is_some() && b.find_buffer("c").is_some());
    let log = b.type_keys(".").unwrap();
    assert!(log.iter().any(|k| !k.errors.is_empty()), "no `.` to repeat");
}
