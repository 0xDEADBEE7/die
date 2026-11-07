//! V5's ex commands where the oracle can't reach: files on disk, leaving, and what `:s` does with
//! the case of a match. Each test works in its own temporary directory.

use die_test_cap as _;

use die_core::{BufferId, Harness, Side, WindowId};
use die_vim::Vim;
use std::path::{Path, PathBuf};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("die-vim-ex-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn editor() -> Harness {
    Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers")
}

/// An editor on a file holding `text`.
fn on_file(dir: &Dir, text: &str) -> (Harness, PathBuf, BufferId, WindowId) {
    let path = dir.0.join("f.txt");
    std::fs::write(&path, text).unwrap();
    let mut h = editor();
    let b = h.open_file(&path).unwrap();
    let w = h.open_window(b, 80, 24).unwrap();
    (h, path, b, w)
}

/// Type `keys` and return the errors they gave.
fn keys(h: &mut Harness, keys: &str) -> Vec<String> {
    let log = h.type_keys(keys).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

#[test]
fn w_writes_the_buffer_and_clears_modified() {
    let dir = Dir::new("w");
    let (mut h, path, b, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>:w<CR>").is_empty());
    assert_eq!(read(&path), "xone\n");
    assert!(!h.text(b).unwrap().modified());
}

#[test]
fn w_refuses_a_file_that_changed_and_w_bang_overwrites_it() {
    let dir = Dir::new("force");
    let (mut h, path, b, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>").is_empty());
    std::fs::write(&path, "from a formatter, longer\n").unwrap();
    let errors = keys(&mut h, ":w<CR>");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("changed on disk"), "{errors:?}");
    assert_eq!(read(&path), "from a formatter, longer\n");
    assert_eq!(text(&h, b), "xone\n");
    assert!(keys(&mut h, ":w!<CR>").is_empty());
    assert_eq!(read(&path), "xone\n");
}

#[test]
fn e_bang_reloads_over_changes_and_u_brings_them_back() {
    let dir = Dir::new("revert");
    let (mut h, path, b, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>").is_empty());
    std::fs::write(&path, "two\n").unwrap();
    assert!(keys(&mut h, ":e!<CR>").is_empty());
    assert_eq!(text(&h, b), "two\n");
    assert!(keys(&mut h, "u").is_empty());
    assert_eq!(text(&h, b), "xone\n");
}

#[test]
fn q_closes_a_split_and_leaves_only_from_the_last_window() {
    let dir = Dir::new("split");
    let (mut h, _, _, w) = on_file(&dir, "one\n");
    h.split_window(w, Side::Below).unwrap();
    assert!(keys(&mut h, "ix<Esc>:q<CR>").is_empty());
    assert_eq!(h.window_list().len(), 1);
    assert!(!h.quit_wanted(), "a split with unsaved text just closes");
}

#[test]
fn q_refuses_unsaved_changes_and_q_bang_drops_them() {
    let dir = Dir::new("quit");
    let (mut h, path, _, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>").is_empty());
    let errors = keys(&mut h, ":q<CR>");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("unsaved"), "{errors:?}");
    assert!(!h.quit_wanted());
    assert!(keys(&mut h, ":q!<CR>").is_empty());
    assert!(h.quit_wanted());
    assert_eq!(read(&path), "one\n");
}

#[test]
fn wq_writes_and_leaves_but_stays_when_the_write_is_refused() {
    let dir = Dir::new("wq");
    let (mut h, path, _, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>").is_empty());
    std::fs::write(&path, "two, from elsewhere\n").unwrap();
    assert_eq!(keys(&mut h, ":wq<CR>").len(), 1);
    assert!(!h.quit_wanted());
    assert_eq!(read(&path), "two, from elsewhere\n");
    assert!(keys(&mut h, ":w!<CR>:wq<CR>").is_empty());
    assert!(h.quit_wanted());
    assert_eq!(read(&path), "xone\n");
}

/// Found by typing into the bundle: these fell through to "no such command".
#[test]
fn wa_writes_every_file_and_qa_leaves_only_with_nothing_unsaved() {
    let dir = Dir::new("all");
    let (mut h, path, b, _) = on_file(&dir, "one\n");
    let other = dir.0.join("g.txt");
    std::fs::write(&other, "two\n").unwrap();
    assert!(keys(&mut h, "ix<Esc>").is_empty());
    assert!(keys(&mut h, &format!(":e {}<CR>iy<Esc>", other.display())).is_empty());
    assert_eq!(keys(&mut h, ":qa<CR>").len(), 1);
    assert!(!h.quit_wanted());
    assert!(keys(&mut h, ":wa<CR>").is_empty());
    assert_eq!(
        (read(&path), read(&other)),
        ("xone\n".into(), "ytwo\n".into())
    );
    assert!(!h.text(b).unwrap().modified());
    assert!(keys(&mut h, ":qa<CR>").is_empty());
    assert!(h.quit_wanted());
}

#[test]
fn wqa_writes_and_leaves_and_qa_bang_drops() {
    let dir = Dir::new("wqa");
    let (mut h, path, _, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>:qa!<CR>").is_empty());
    assert!(h.quit_wanted());
    assert_eq!(read(&path), "one\n");
    let (mut h, path, _, _) = on_file(&dir, "one\n");
    assert!(keys(&mut h, "ix<Esc>:xa<CR>").is_empty());
    assert!(h.quit_wanted());
    assert_eq!(read(&path), "xone\n");
}

#[test]
fn file_commands_refuse_an_argument_and_a_buffer_with_no_file() {
    let mut h = editor();
    let (_, _) = h.open_text("scratch", "one\n").unwrap();
    assert_eq!(keys(&mut h, ":w other.txt<CR>").len(), 1);
    assert_eq!(keys(&mut h, ":w<CR>").len(), 1);
}

#[test]
fn s_keeps_a_matchs_case_as_emacs_does_and_takes_any_delimiter() {
    let mut h = editor();
    let (b, _) = h.open_text("t", "foo Foo FOO\n").unwrap();
    assert!(keys(&mut h, ":s/foo/bar/g<CR>").is_empty());
    assert_eq!(text(&h, b), "bar Bar BAR\n");
    assert!(keys(&mut h, ":s#bar#a/$$#<CR>").is_empty());
    assert_eq!(text(&h, b), "a/$ Bar BAR\n");
}

#[test]
fn q_bang_at_the_last_window_leaves_the_changes_out_of_the_session() {
    let dir = Dir::new("q-session");
    let (mut h, path, _, _) = on_file(&dir, "a\n");
    assert!(keys(&mut h, "idiscard me <Esc>:q!<CR>").is_empty());
    assert!(h.quit_wanted());
    let mut next = editor();
    next.restore_session(&h.session_json()).unwrap();
    let b = next.find_buffer("f.txt").unwrap();
    assert_eq!(text(&next, b), "a\n");
    assert!(!next.text(b).unwrap().modified());
    assert_eq!(read(&path), "a\n");
}

#[test]
fn q_bang_drops_the_text_of_a_file_that_isnt_there() {
    let dir = Dir::new("q-new");
    let (mut h, _, _, _) = on_file(&dir, "a\n");
    assert!(keys(&mut h, ":e new.txt<CR>idiscard me<Esc>:q!<CR>").is_empty());
    let mut next = editor();
    next.restore_session(&h.session_json()).unwrap();
    let b = next.find_buffer("new.txt").unwrap();
    assert_eq!(text(&next, b), "");
    assert!(!next.text(b).unwrap().modified());
}

#[test]
fn wq_bang_forces_the_write_but_not_the_quit() {
    let dir = Dir::new("wq-bang");
    let (mut h, _, _, _) = on_file(&dir, "one\n");
    std::fs::write(dir.0.join("c.txt"), "c\n").unwrap();
    assert!(keys(&mut h, "ix<Esc>:e c.txt<CR>").is_empty());
    let errors = keys(&mut h, ":wq!<CR>");
    assert!(errors.iter().any(|e| e.contains("f.txt")), "{errors:?}");
    assert!(!h.quit_wanted());
}

#[test]
fn s_treats_the_end_of_text_with_no_final_newline_as_a_line() {
    let mut h = editor();
    let (b, _) = h.open_text("t", "abc").unwrap();
    assert!(keys(&mut h, ":s/$/;/<CR>").is_empty());
    assert_eq!(text(&h, b), "abc;");
    let (b, _) = h.open_text("u", "a\nb").unwrap();
    assert!(keys(&mut h, ":%s/$/;/<CR>").is_empty());
    assert_eq!(text(&h, b), "a;\nb;");
}

/// `:'<,'>w` writes only the selected lines in Evil, so a whole-file write would be wrong.
#[test]
fn a_selections_range_is_refused_by_w_and_ignored_by_commands_that_take_none() {
    let dir = Dir::new("range");
    let (mut h, path, _, _) = on_file(&dir, "one\ntwo\n");
    assert!(keys(&mut h, "ix<Esc>").is_empty());
    assert_eq!(keys(&mut h, "V:w<CR>").len(), 1);
    assert_eq!(read(&path), "one\ntwo\n");
    assert!(keys(&mut h, "V:<CR>").is_empty());
    assert!(keys(&mut h, ":%vsplit<CR>").is_empty());
    assert_eq!(h.window_list().len(), 2);
}

/// A project on disk, with an editor on `src/a.txt`, whose lines name other files.
fn in_project(name: &str, text: &str) -> (Dir, Harness, BufferId) {
    let dir = Dir::new(name);
    let root = dir.0.canonicalize().unwrap();
    for sub in [".git", "src", "lib"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    std::fs::write(root.join("src/a.txt"), text).unwrap();
    std::fs::write(root.join("src/near.txt"), "one\ntwo\nthree needle\n").unwrap();
    std::fs::write(root.join("lib/far.txt"), "one\ntwo\nthree\nneedle\n").unwrap();
    let mut h = editor();
    let b = h.open_file(&root.join("src/a.txt")).unwrap();
    h.open_window(b, 80, 24).unwrap();
    (dir, h, b)
}

/// The focused file's name, and its cursor's line and byte in it, from 1.
fn at_file(h: &Harness) -> (String, usize, usize) {
    let (w, b) = (h.focused().unwrap(), h.focused_buffer().unwrap());
    let name = h.buffer_name(b).unwrap().to_string();
    let t = h.text(b).unwrap();
    let pos = t.cursor(w).unwrap();
    let line = t.line_of(pos);
    (name, line + 1, pos - t.line_start(line) + 1)
}

#[test]
fn gf_opens_from_this_directory_then_the_project_with_its_line_and_column() {
    let text = "near.txt:2:3: rustc\n\
                see lib/far.txt(3,2) there\n\
                lib/far.txt:\n\
                near.txt(2)\n\
                nowhere.txt\n\
                ../src\n";
    let (_dir, mut h, b) = in_project("gf", text);
    assert_eq!(keys(&mut h, "gf"), Vec::<String>::new());
    assert_eq!(at_file(&h), ("near.txt".to_string(), 2, 3));
    keys(&mut h, "<C-o>");
    assert_eq!(h.focused_buffer(), Some(b), "C-o comes back");

    keys(&mut h, "0jfbgf");
    let far = ("far.txt".to_string(), 3, 2);
    assert_eq!(at_file(&h), far, "from the project, with parentheses");
    keys(&mut h, "<C-o>0jgF");
    assert_eq!(
        at_file(&h),
        far,
        "gF too, and no position leaves the cursor"
    );
    keys(&mut h, "<C-o>0jgf");
    assert_eq!(at_file(&h), ("near.txt".to_string(), 2, 1));
    keys(&mut h, "<C-o>0j");
    assert_eq!(keys(&mut h, "gf"), ["File does not exist."]);
    keys(&mut h, "j");
    assert_eq!(keys(&mut h, "gf"), ["File does not exist."], "a directory");
    assert_eq!(h.focused_buffer(), Some(b));
}

#[test]
fn grep_results_take_motions_search_and_cn_with_a_jump_to_go_back() {
    let (_dir, mut h, b) = in_project("grep", "needle here\n");
    assert_eq!(keys(&mut h, ":grep needle<CR>"), Vec::<String>::new());
    h.run_until_idle();
    assert_eq!(h.focused_buffer(), Some(b), "the focus stays");
    keys(&mut h, ":cn<CR>");
    assert_eq!(
        at_file(&h),
        ("far.txt".to_string(), 4, 1),
        "by directory, then name"
    );
    keys(&mut h, "<C-o>");
    assert_eq!(h.focused_buffer(), Some(b), "C-o comes back");
    keys(&mut h, ":cnext<CR>");
    assert_eq!(at_file(&h), ("a.txt".to_string(), 1, 1));
    keys(&mut h, ":cn<CR>");
    assert_eq!(at_file(&h), ("near.txt".to_string(), 3, 7));
    assert_eq!(keys(&mut h, ":cn<CR>"), ["Moved past last match"]);
    keys(&mut h, ":cp<CR>");
    assert_eq!(at_file(&h), ("a.txt".to_string(), 1, 1));

    keys(&mut h, "<C-w>w");
    let results = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(results), Some("*grep*"));
    keys(&mut h, "ggj");
    assert_eq!(at_file(&h).1, 2, "motions");
    keys(&mut h, "/near<CR>");
    assert_eq!(at_file(&h).1, 4, "search");
    let before = h.text(results).unwrap().rope().to_string();
    assert!(!keys(&mut h, "x").is_empty(), "read-only");
    assert_eq!(h.text(results).unwrap().rope().to_string(), before);
    keys(&mut h, "0<CR>");
    assert_eq!(at_file(&h), ("near.txt".to_string(), 3, 7));
}
