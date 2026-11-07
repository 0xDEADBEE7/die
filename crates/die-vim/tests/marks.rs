//! Marks and the jump list where the Evil oracle can't reach: more than one buffer or window, a
//! killed buffer, and a restart.

use die_test_cap as _;

use die_core::{BufferId, Ctx, FileTarget, ShowIn, Side, State, WindowId};
use die_vim::Vim;

fn editor() -> Ctx {
    Ctx::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
}

/// Two buffers, in one window that shows the first.
fn two_buffers(ctx: &mut Ctx) -> (BufferId, BufferId, WindowId) {
    let a = ctx.create_text("a", "a0\na1\na2\na3\n");
    let b = ctx.create_text("b", "b0\nb1\nb2\nb3\n");
    let w = ctx.open_window(a, 80, 24).expect("a window");
    ctx.focus(w);
    (a, b, w)
}

fn at(ctx: &Ctx, w: WindowId) -> (BufferId, usize) {
    let b = ctx.window_buffer(w).expect("the window");
    let t = ctx.text(b).expect("text");
    let c = t.cursor(w).expect("a cursor");
    (b, t.line_of(c))
}

#[test]
fn an_uppercase_mark_is_one_across_buffers_and_goes_there() {
    let mut ctx = editor();
    let (a, b, w) = two_buffers(&mut ctx);
    ctx.type_keys("jjmA");
    ctx.type_keys(":b b<CR>");
    ctx.type_keys("jmB'A");
    assert_eq!(at(&ctx, w), (a, 2), "'A goes to the other buffer");
    ctx.type_keys("`B");
    assert_eq!(at(&ctx, w), (b, 1), "and `B back");

    ctx.type_keys("mA");
    ctx.type_keys("gg'A");
    assert_eq!(at(&ctx, w), (b, 1), "there's one A, the newest");
    assert_eq!(ctx.text(a).unwrap().mark('A'), None);
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn a_lowercase_mark_is_this_buffers() {
    let mut ctx = editor();
    let (_, b, w) = two_buffers(&mut ctx);
    ctx.type_keys("jma:b b<CR>");
    ctx.type_keys("`a");
    assert_eq!(at(&ctx, w), (b, 0));
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn c_o_and_tab_go_across_buffers_and_a_change_of_buffer_is_a_jump() {
    let mut ctx = editor();
    let (a, b, w) = two_buffers(&mut ctx);
    ctx.type_keys("jj:b b<CR>");
    ctx.type_keys("3G");
    assert_eq!(at(&ctx, w), (b, 2));
    ctx.type_keys("<C-o>");
    assert_eq!(at(&ctx, w), (b, 0), "where the buffer showed its cursor");
    ctx.type_keys("<C-o>");
    assert_eq!(at(&ctx, w), (a, 2), "where :b left the other buffer");
    ctx.type_keys("<Tab>");
    assert_eq!(at(&ctx, w), (b, 0));
    ctx.type_keys("<C-i>");
    assert_eq!(at(&ctx, w), (b, 2));
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn an_operator_doesnt_reach_into_another_buffer() {
    let mut ctx = editor();
    let (a, b, w) = two_buffers(&mut ctx);
    ctx.type_keys(":b b<CR>");
    ctx.type_keys("jmZ:b a<CR>");
    ctx.type_keys("d`Z");
    assert_eq!(at(&ctx, w).0, a);
    assert_eq!(ctx.state(), die_core::State::Normal);
    assert_eq!(ctx.text(a).unwrap().rope().to_string(), "a0\na1\na2\na3\n");
    assert_eq!(ctx.text(b).unwrap().rope().to_string(), "b0\nb1\nb2\nb3\n");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

#[test]
fn a_split_starts_with_its_windows_jumps_and_each_then_goes_its_own_way() {
    let mut ctx = editor();
    let (a, _, w) = two_buffers(&mut ctx);
    ctx.type_keys("3Ggg");
    ctx.type_keys("<C-w>s");
    let new = ctx.focused().expect("the new window");
    assert_ne!(new, w);
    ctx.type_keys("<C-o>");
    assert_eq!(at(&ctx, new), (a, 2));
    assert_eq!(at(&ctx, w), (a, 0), "the first window didn't move");
    ctx.type_keys("<C-w>w<C-o>");
    assert_eq!(at(&ctx, w), (a, 2), "and its list still has it");
}

#[test]
fn a_jump_into_a_killed_buffer_is_gone() {
    let mut ctx = editor();
    let (a, b, w) = two_buffers(&mut ctx);
    ctx.type_keys("jjmA:b b<CR>");
    ctx.type_keys("3G");
    assert!(ctx.kill(a));
    ctx.run_until_idle();
    ctx.type_keys("<C-o><C-o><C-o>");
    assert_eq!(at(&ctx, w), (b, 0), "only this buffer's jumps are left");
    ctx.type_keys("`A");
    assert_eq!(at(&ctx, w).0, b);
    assert_eq!(ctx.messages.len(), 1, "A went with it: {:?}", ctx.messages);
}

/// What a restart keeps: every mark, moved by the edits made since it was set, and the jump
/// list with where the window had got to in it. The digest is of this JSON, so exit test (b)
/// holds it to itself.
#[test]
fn marks_and_jumps_come_back_after_a_restart() {
    let mut ctx = editor();
    let (_, b, w) = two_buffers(&mut ctx);
    ctx.type_keys("3Gma");
    ctx.type_keys("jmQ");
    ctx.type_keys("ggOnew<Esc>");
    ctx.type_keys(":b b<CR>");
    ctx.type_keys("3Gvl<Esc>ix<Esc>");
    ctx.type_keys("<C-o>");
    let (bb, _) = at(&ctx, w);
    assert_eq!(bb, b);
    let saved = ctx.session_json();
    assert!(saved.contains("\"marks\""), "{saved}");
    assert!(saved.contains("\"places\""), "{saved}");
    let before = |ctx: &Ctx, name: &str, c: char| {
        let id = ctx.find_buffer(name).unwrap();
        ctx.text(id).unwrap().mark(c)
    };
    let was: Vec<_> = "aQ^.<>"
        .chars()
        .map(|c| (c, before(&ctx, "a", c), before(&ctx, "b", c)))
        .collect();
    let list = ctx.places(w).unwrap().clone();

    let mut ctx2 = editor();
    ctx2.restore_session(&saved).expect("it restores");
    let w2 = ctx2.focused().expect("a window");
    let now: Vec<_> = "aQ^.<>"
        .chars()
        .map(|c| (c, before(&ctx2, "a", c), before(&ctx2, "b", c)))
        .collect();
    assert_eq!(now, was);
    let list2 = ctx2.places(w2).unwrap();
    assert_eq!(list2.index, list.index);
    assert_eq!(list2.places.len(), list.places.len());

    // It's the same list: going back and forward lands where it would have.
    ctx2.type_keys("<C-o>");
    let older = at(&ctx2, w2);
    ctx.type_keys("<C-o>");
    let (_, line) = at(&ctx, w);
    assert_eq!(older.1, line);
    ctx2.type_keys("<C-o><C-o><C-o><C-o>`Q");
    assert_eq!(at(&ctx2, w2).1, 4, "Q moved down the line ggO made");
}

/// A session from before there were marks or jumps has neither, and loses nothing else.
#[test]
fn a_session_without_them_still_restores() {
    let mut ctx = editor();
    let json = r#"{"version":1,"focus":0,"buffers":[{"name":"b","text":"x\ny\n"}]}"#;
    ctx.restore_session(json).expect("it restores");
    assert!(ctx.find_buffer("b").is_some());
    ctx.type_keys("`a");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    ctx.type_keys("<C-o>");
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
}

/// `die file:line` from the terminal in the next tile goes far with no key, in the buffer on
/// screen as much as to another: `C-o` comes back.
#[test]
fn a_file_opened_at_a_line_from_outside_leaves_a_jump() {
    let dir = std::env::temp_dir().join(format!("die-vim-marks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path_a, path_b) = (dir.join("a.txt"), dir.join("b.txt"));
    std::fs::write(&path_a, "a0\na1\na2\na3\na4\n").unwrap();
    std::fs::write(&path_b, "b0\nb1\nb2\nb3\n").unwrap();
    let target = |path: &std::path::Path, line| FileTarget {
        path: path.to_path_buf(),
        line: Some(line),
        col: None,
    };
    let mut ctx = editor();
    let a = ctx.open_file(&path_a).unwrap();
    let w = ctx.open_window(a, 80, 24).unwrap();
    ctx.focus(w);
    ctx.type_keys("j");
    let term = ctx.create_text("term", "");
    let right = ctx.split_window(w, Side::Right).unwrap();
    ctx.focus(right);
    ctx.show(term, ShowIn::Current).unwrap();
    ctx.set_state_of(term, State::Passthrough);

    ctx.open_target(&target(&path_a, 4)).unwrap();
    assert_eq!(at(&ctx, w), (a, 3));
    ctx.type_keys("<C-o>");
    assert_eq!(at(&ctx, w), (a, 1), "back in the buffer it never left");

    // Twice with no key between, and each place it left is still there to go back to.
    let b = ctx.open_target(&target(&path_b, 3)).unwrap();
    ctx.open_target(&target(&path_a, 5)).unwrap();
    ctx.type_keys("<C-o>");
    assert_eq!(at(&ctx, w), (b, 2), "the file it was in just now");
    ctx.type_keys("<C-o>");
    assert_eq!(at(&ctx, w), (a, 1), "and the one before");
    let _ = std::fs::remove_dir_all(&dir);
}
