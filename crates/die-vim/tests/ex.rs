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
