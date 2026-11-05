//! Insert sessions where the Evil oracle can't reach: two buffers, read-only
//! text, a buffer's own tab width, the changes an edit logs, and DIE's own screen
//! rows until K4's word wrap matches the rows your Evil's fixtures record.

use die_core::{BufferId, Ctx, State, WindowId};
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

#[test]
fn each_buffer_has_its_own_insert_session() {
    let mut ctx = editor();
    let (a, wa) = open(&mut ctx, "aaa\n");
    let (b, wb) = open(&mut ctx, "bbb\n");
    ctx.focus(wb);
    ctx.type_keys("ibar");
    ctx.focus(wa);
    ctx.type_keys("3ifoo");
    ctx.focus(wb);
    ctx.type_keys("<Esc>");
    assert_eq!(
        text(&ctx, b),
        "barbbb\n",
        "B's Escape replays B's session only"
    );
    assert_eq!(ctx.state(), State::Normal);

    ctx.focus(wa);
    assert_eq!(ctx.state(), State::Insert);
    ctx.type_keys("<Esc>");
    assert_eq!(text(&ctx, a), "foofoofooaaa\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, a), "aaa\n", "A's session is one undo step");
    ctx.focus(wb);
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "bbb\n", "and so is B's");
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn a_failed_o_leaves_no_undo_group_open() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "a\nb\n");
    ctx.text_mut(b).unwrap().settings_mut().read_only = true;
    ctx.type_keys("o");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal);

    ctx.text_mut(b).unwrap().settings_mut().read_only = false;
    ctx.type_keys("ix<Esc>iy<Esc>u");
    assert_eq!(text(&ctx, b), "xa\nb\n", "u undoes only the last session");
}

#[test]
fn escape_ends_the_session_when_dropping_indentation_fails() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "  a\n");
    ctx.type_keys("o");
    assert_eq!(text(&ctx, b), "  a\n  \n");
    ctx.text_mut(b).unwrap().settings_mut().read_only = true;
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert_eq!(ctx.state(), State::Normal);

    ctx.text_mut(b).unwrap().settings_mut().read_only = false;
    ctx.type_keys("ix<Esc>u");
    assert_eq!(text(&ctx, b), "  a\n  \n", "u undoes only the x");
}

#[test]
fn tab_stops_follow_the_buffers_tab_width() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "ab\n");
    ctx.text_mut(b).unwrap().settings_mut().tab_width = 4;
    ctx.type_keys("a<Tab><Esc>");
    assert_eq!(text(&ctx, b), "a   b\n");
}

/// A line with no break points wraps at the window's width under any wrap rule.
fn long_line(ctx: &mut Ctx) -> BufferId {
    let line = "x".repeat(170);
    let (b, w) = open(ctx, &format!("{line}\nnext\n"));
    ctx.focus(w);
    b
}

#[test]
fn ctrl_a_goes_to_the_start_of_the_screen_row() {
    for (keys, want) in [
        ("$<C-a>", 160),
        ("<C-a>", 0),
        ("2<C-a>", 80),
        ("3<C-a>", 160),
        ("4<C-a>", 171),
        ("9<C-a>", 171),
    ] {
        let mut ctx = editor();
        let b = long_line(&mut ctx);
        ctx.type_keys(keys);
        let w = ctx.focused().unwrap();
        assert_eq!(ctx.text(b).unwrap().cursor(w), Some(want), "{keys}");
        assert!(ctx.messages.is_empty(), "{keys}: {:?}", ctx.messages);
    }
}

#[test]
fn insert_state_arrows_go_by_screen_row() {
    for (keys, want) in [
        ("i<Down>X<Esc>", 80),
        ("i<Down><Down>X<Esc>", 160),
        ("$i<Up>X<Esc>", 89),
    ] {
        let mut ctx = editor();
        let b = long_line(&mut ctx);
        ctx.type_keys(keys);
        let t = text(&ctx, b);
        assert_eq!(t.find('X'), Some(want), "{keys}");
        assert!(ctx.messages.is_empty(), "{keys}: {:?}", ctx.messages);
    }
}

/// The last row of a wrapped last line ends at the text's end, and Down goes there
/// before it fails. The column stays for an Up, and it's still the first row's column.
#[test]
fn down_from_a_wrapped_last_row_goes_to_the_end_and_keeps_the_column() {
    let mut ctx = editor();
    let (b, w) = open(&mut ctx, &"x".repeat(100));
    ctx.type_keys("10li<Down><Down>");
    assert_eq!(ctx.text(b).unwrap().cursor(w), Some(100));
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert!(ctx.messages[0].contains("End of buffer"));
    ctx.type_keys("<Up>X<Esc>");
    assert_eq!(text(&ctx, b).find('X'), Some(10));
}

/// Run `f` on another thread and fail if it takes more than 10 s, so that a count
/// that hangs fails the test and doesn't hang it. Key by key, `50000ix` took 40 s.
fn quickly<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("done within 10 s")
}

/// `keys` on "\n": the text's length, and what die-vim said.
fn counted(keys: &'static str) -> (usize, Vec<String>) {
    quickly(move || {
        let mut ctx = editor();
        let (b, _) = open(&mut ctx, "\n");
        ctx.type_keys(keys);
        assert_eq!(ctx.state(), State::Normal, "{keys}");
        (text(&ctx, b).len(), ctx.messages.clone())
    })
}

#[test]
fn a_large_count_on_typed_text_goes_in_at_once() {
    let (len, messages) = counted("50000ix<Esc>");
    assert_eq!(len, 50_001);
    assert!(messages.is_empty(), "{messages:?}");
    let (len, _) = counted("1048577ix<Esc>");
    assert_eq!(len, (1 << 20) + 2, "the most a count may replay");
}

#[test]
fn a_count_past_the_caps_fails_and_keeps_the_first_copy() {
    for keys in [
        "1048578ix<Esc>",
        "99999999999999999999ix<Esc>",
        "10002ix<Left><Esc>",
    ] {
        let (len, messages) = counted(keys);
        assert_eq!(len, 2, "{keys}");
        assert_eq!(messages.len(), 1, "{keys}: {messages:?}");
        assert!(messages[0].contains("Count too large"), "{messages:?}");
    }
    let (len, messages) = counted("99999o<Esc>");
    assert_eq!(len, 2, "o's line");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("Count too large"), "{messages:?}");
}

#[test]
fn an_empty_session_replays_nothing_at_any_count() {
    let (len, messages) = counted("99999999999999999999i<Esc>");
    assert_eq!(len, 1);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn a_replay_inside_the_caps_still_goes_key_by_key() {
    let (len, messages) = counted("5000ix<Left><Esc>");
    assert_eq!(len, 5001);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn a_counted_session_with_nothing_typed_leaves_no_undo_step() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "abc\n");
    ctx.type_keys("ifoo<Esc>3i<Esc>u");
    assert_eq!(text(&ctx, b), "abc\n", "u undoes ifoo; 3i<Esc> did nothing");
}

#[test]
fn opening_an_unindented_line_makes_one_change() {
    let mut ctx = editor();
    let (b, _) = open(&mut ctx, "a\n");
    let rev = ctx.text(b).unwrap().rev();
    ctx.type_keys("o");
    let t = ctx.text(b).unwrap();
    let changes: Vec<_> = t.edits_since(rev).expect("in the log").collect();
    assert_eq!(changes.len(), 1, "{changes:?}");
}
