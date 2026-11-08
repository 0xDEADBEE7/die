//! `die --wait`'s files. `:wq`, `:x`, `ZZ` and `:q` on one say you're done with it, as your Evil's
//! `evil-quit` runs `server-edit`, and so does killing it. A file made for the client goes, its
//! window shows what DIE shows after a kill, and the client is told once it has no file left.
//! `:cq` makes it fail, and a restart hands it to the next instance. Each test works in its own
//! directory, and a client is what it was told, which the test watches.

use die_test_cap as _;

use die_core::{BufferId, FileTarget, Harness, Outcome, ShowIn, Waited};
use die_vim::Vim;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("die-vim-wait-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An editor showing `*before*`, which a client's file then takes the place of.
fn editor() -> (Harness, BufferId) {
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let (before, _) = h.open_text("*before*", "what was there\n").unwrap();
    (h, before)
}

/// What a client was told, once it's told.
#[derive(Default)]
struct Told(Rc<RefCell<Option<Outcome>>>);

impl Told {
    /// Whether it's been told.
    fn get(&self) -> bool {
        self.0.borrow().is_some()
    }

    fn outcome(&self) -> Option<Outcome> {
        self.0.borrow().clone()
    }

    /// What tells it, once.
    fn tell(&self) -> impl FnOnce(Outcome) + 'static {
        let set = self.0.clone();
        move |outcome| assert!(set.replace(Some(outcome)).is_none(), "told twice")
    }
}

/// A client waiting on these files, as `die --wait` opens them.
fn client(h: &mut Harness, paths: &[&Path]) -> Told {
    let told = Told::default();
    let targets: Vec<FileTarget> = paths
        .iter()
        .map(|p| FileTarget {
            path: p.to_path_buf(),
            line: None,
            col: None,
        })
        .collect();
    h.open_for_client(&targets, told.tell());
    told
}

fn buffer_of(h: &Harness, path: &Path) -> BufferId {
    let list = h.buffer_list().into_iter();
    list.into_iter()
        .find(|b| h.file(*b).is_some_and(|f| f.path == path))
        .unwrap()
}

fn keys(h: &mut Harness, keys: &str) -> Vec<String> {
    let log = h.type_keys(keys).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

fn on_screen(h: &Harness) -> BufferId {
    h.focused_buffer().unwrap()
}

#[test]
fn wq_x_zz_and_q_finish_the_file_and_the_window_goes_back() {
    for done in [
        ":wq<CR>", ":x<CR>", "ZZ", ":q<CR>", ":q!<CR>", "ZQ", "<C-w>q",
    ] {
        let dir = Dir::new("each");
        let path = dir.file("COMMIT_EDITMSG", "\n# Please enter the commit message\n");
        let (mut h, before) = editor();
        let told = client(&mut h, &[&path]);
        let file = on_screen(&h);
        assert_ne!(file, before);
        assert!(h.is_client_buffer(file) && !told.get(), "{done}");
        assert!(keys(&mut h, done).is_empty(), "{done}");
        assert_eq!(told.outcome(), Some(Outcome::Done), "{done}");
        assert!(!h.quit_wanted(), "{done}: DIE stays");
        assert!(!h.is_live(file), "{done}: made for the client, it goes");
        assert_eq!(
            on_screen(&h),
            before,
            "{done}: the window shows what it did"
        );
        assert_eq!(h.window_list().len(), 1, "{done}");
    }
}

#[test]
fn wq_writes_first_and_q_refuses_unsaved_text_which_q_bang_drops() {
    let dir = Dir::new("unsaved");
    let path = dir.file("msg", "\n");
    let (mut h, before) = editor();
    let told = client(&mut h, &[&path]);
    let file = on_screen(&h);
    assert!(keys(&mut h, "iFix it<Esc>").is_empty());
    for refused in [":q<CR>", "ZQ", "<C-w>q"] {
        let errors = keys(&mut h, refused);
        let said = errors
            .first()
            .is_some_and(|e| e.contains("has unsaved changes"));
        assert!(said, "{refused}: {errors:?}");
        assert!(!told.get() && h.is_live(file), "{refused}");
    }
    assert!(keys(&mut h, ":x<CR>").is_empty());
    assert!(told.get());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Fix it\n");
    assert_eq!(on_screen(&h), before);

    let (mut h, _) = editor();
    let told = client(&mut h, &[&path]);
    assert!(keys(&mut h, "ccLost<Esc>:q!<CR>").is_empty());
    assert!(told.get());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Fix it\n");
}

/// Your Evil closes a split and leaves the client waiting, and only the last window's `:q` runs
/// `server-edit`. DIE finishes it in any window, so `git commit` isn't left waiting.
#[test]
fn in_a_split_wq_finishes_the_file_rather_than_close_the_window() {
    let dir = Dir::new("split");
    let path = dir.file("msg", "text\n");
    let (mut h, before) = editor();
    assert!(keys(&mut h, "<C-w>v").is_empty());
    let told = client(&mut h, &[&path]);
    assert!(keys(&mut h, ":wq<CR>").is_empty());
    assert!(told.get());
    assert_eq!(h.window_list().len(), 2, "the split stays");
    assert_eq!(on_screen(&h), before);
    assert!(keys(&mut h, ":q<CR>").is_empty());
    assert_eq!(h.window_list().len(), 1, "and `:q` closes it as before");
}

/// As `server-kill-new-buffers` has it: a file open before the client asked stays, on screen and
/// last in the buffer list, and only a buffer made for the client goes.
#[test]
fn a_file_open_before_stays_and_goes_last_in_the_buffer_list() {
    let dir = Dir::new("open-before");
    let path = dir.file("notes.txt", "mine\n");
    let (mut h, before) = editor();
    let mine = h.open_file(&path).unwrap();
    let told = client(&mut h, &[&path]);
    assert_eq!(on_screen(&h), mine);
    assert!(keys(&mut h, "ZZ").is_empty());
    assert!(told.get());
    assert!(h.is_live(mine) && !h.is_client_buffer(mine));
    assert_eq!(on_screen(&h), mine, "it stays on screen");
    let list = h.buffer_list();
    let at = |b| list.iter().position(|x| *x == b);
    assert!(at(mine) > at(before), "it went last: {list:?}");
    let said = h.messages.last().cloned().unwrap_or_default();
    assert_eq!(said, "No server buffers remain to edit");
    assert!(keys(&mut h, ":q<CR>").is_empty());
    assert!(h.quit_wanted(), "once done, `:q` quits as before");
}

#[test]
fn killing_the_file_finishes_it() {
    for kill in [":bd<CR>", ":bd!<CR>"] {
        let dir = Dir::new("kill");
        let path = dir.file("msg", "text\n");
        let (mut h, before) = editor();
        let told = client(&mut h, &[&path]);
        assert!(keys(&mut h, kill).is_empty(), "{kill}");
        assert!(told.get(), "{kill}");
        assert_eq!(on_screen(&h), before, "{kill}");
    }
    let dir = Dir::new("kill-buffer");
    let path = dir.file("msg", "text\n");
    let (mut h, _) = editor();
    let told = client(&mut h, &[&path]);
    let file = on_screen(&h);
    h.kill(file);
    assert!(told.get());
}

/// `die --wait a b` shows `b`, the last, as `emacsclient` does. Done with it, `a` comes up, and the
/// client is told only when both are done.
#[test]
fn with_two_files_the_next_comes_up_and_the_client_waits_for_both() {
    let dir = Dir::new("two");
    let (a, b) = (dir.file("a", "a\n"), dir.file("b", "b\n"));
    let (mut h, before) = editor();
    let told = client(&mut h, &[&a, &b]);
    let shown_b = on_screen(&h);
    assert_eq!(h.file(shown_b).unwrap().path, b);
    assert!(keys(&mut h, ":wq<CR>").is_empty());
    assert!(!told.get());
    assert_eq!(h.file(on_screen(&h)).unwrap().path, a);
    assert!(keys(&mut h, ":wq<CR>").is_empty());
    assert!(told.get());
    assert_eq!(on_screen(&h), before);
}

#[test]
fn clients_waiting_at_once_are_each_told_for_their_own_files() {
    let dir = Dir::new("several");
    let (a, b) = (dir.file("a", "a\n"), dir.file("b", "b\n"));
    let (mut h, _) = editor();
    let first = client(&mut h, &[&a]);
    let second = client(&mut h, &[&b]);
    assert!(keys(&mut h, ":wq<CR>").is_empty());
    assert!(second.get() && !first.get());
    let next = on_screen(&h);
    assert_eq!(
        h.file(next).unwrap().path,
        a,
        "the other client's file comes up"
    );
    assert!(keys(&mut h, "ZZ").is_empty());
    assert!(first.get());

    // Two waiting on one file are both told when you're done with it.
    let (mut h, _) = editor();
    let (one, two) = (client(&mut h, &[&a]), client(&mut h, &[&a]));
    assert!(keys(&mut h, ":q<CR>").is_empty());
    assert!(one.get() && two.get());
}

/// It fails, as `emacsclient` does, so git stops.
#[test]
fn a_client_whose_files_wont_open_is_told_at_once_that_it_failed() {
    let dir = Dir::new("unopened");
    let (mut h, before) = editor();
    let told = client(&mut h, &[&dir.0]);
    assert_eq!(told.outcome(), Some(Outcome::Failed));
    assert_eq!(on_screen(&h), before);
    let name = dir.0.display().to_string();
    assert!(
        h.messages.iter().any(|m| m.contains(&name)),
        "{:?}",
        h.messages
    );
}

/// git writes the message file again for each commit: an unchanged buffer still open on it has
/// what's on disk when the client asks.
#[test]
fn a_buffer_open_on_the_file_is_read_again_first() {
    let dir = Dir::new("again");
    let path = dir.file("COMMIT_EDITMSG", "first\n");
    let (mut h, _) = editor();
    let open = h.open_file(&path).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&path, "second, and longer\n").unwrap();
    client(&mut h, &[&path]);
    assert_eq!(on_screen(&h), open);
    assert_eq!(
        h.text(open).unwrap().rope().to_string(),
        "second, and longer\n"
    );
}

/// Your Evil's `:q!` on a client's file runs only `server-buffer-done`, not `server-edit`: no
/// other file comes up, and nothing says none are left. A file open before stays on screen.
#[test]
fn q_bang_finishes_the_file_and_brings_nothing_else_up() {
    let dir = Dir::new("q-bang");
    let (a, b) = (dir.file("a", "a\n"), dir.file("b", "b\n"));
    let (mut h, _) = editor();
    let mine = h.open_file(&b).unwrap();
    let told = client(&mut h, &[&a, &b]);
    assert_eq!(on_screen(&h), mine);
    assert!(keys(&mut h, "x:q!<CR>").is_empty());
    assert!(!told.get() && !h.is_client_buffer(mine));
    assert_eq!(on_screen(&h), mine, "a doesn't come up, as `:q` brings it");
    assert!(h.is_client_buffer(buffer_of(&h, &a)));

    let (mut h, _) = editor();
    let mine = h.open_file(&b).unwrap();
    let told = client(&mut h, &[&b]);
    assert!(keys(&mut h, ":q!<CR>").is_empty());
    assert_eq!(told.outcome(), Some(Outcome::Done));
    assert_eq!(on_screen(&h), mine);
    let said = |m: &String| m == "No server buffers remain to edit";
    assert!(!h.messages.iter().any(said), "{:?}", h.messages);
}

/// `:cq` gives up, as Emacs's `server-edit-abort` does: the client fails, so git stops, and the
/// file goes as `:q!` has it, unsaved text and all. Its client's other files made for it go too,
/// as `server-delete-client` has it, except one with unsaved changes. Elsewhere it refuses.
#[test]
fn cq_fails_the_client_and_its_files_go() {
    let dir = Dir::new("cq");
    let (a, b) = (dir.file("a", "a\n"), dir.file("b", "b\n"));
    let (mut h, before) = editor();
    assert_eq!(keys(&mut h, ":cq<CR>"), ["This buffer has no clients"]);
    assert!(!h.quit_wanted());
    let told = client(&mut h, &[&a, &b]);
    let (shown_b, made_a) = (on_screen(&h), buffer_of(&h, &a));
    assert!(keys(&mut h, "ccgiving up<Esc>:cq<CR>").is_empty());
    assert_eq!(told.outcome(), Some(Outcome::Failed));
    assert!(!h.is_live(shown_b) && !h.is_live(made_a));
    assert_eq!(on_screen(&h), before);
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "b\n");

    let (mut h, _) = editor();
    let told = client(&mut h, &[&a, &b]);
    let (shown_b, made_a) = (on_screen(&h), buffer_of(&h, &a));
    h.show(made_a, ShowIn::Current).unwrap();
    assert!(keys(&mut h, "xx").is_empty());
    h.show(shown_b, ShowIn::Current).unwrap();
    assert!(keys(&mut h, ":cquit<CR>").is_empty());
    assert_eq!(told.outcome(), Some(Outcome::Failed));
    assert!(
        h.is_live(made_a) && !h.is_client_buffer(made_a),
        "a has unsaved changes"
    );
    assert!(!h.is_live(shown_b));
}

/// A restart tells each client what it still waits on, and whether each file was made for it,
/// and forgets them. The next instance, with the files open from the session, waits on them
/// again: nothing moves on screen, and done, a file made for the client goes as it would have.
#[test]
fn a_restart_hands_each_client_over_and_the_next_instance_waits_again() {
    let dir = Dir::new("restart");
    let (a, b, notes) = (
        dir.file("a", "a\n"),
        dir.file("b", "b\n"),
        dir.file("notes", "n\n"),
    );
    let (mut h, _) = editor();
    let mine = h.open_file(&notes).unwrap();
    let (one, two) = (client(&mut h, &[&a, &b]), client(&mut h, &[&notes]));
    assert!(
        keys(&mut h, ":b b<CR>:wq<CR>").is_empty(),
        "{:?}",
        h.messages
    );
    h.hand_over_clients();
    let moved = |path: &Path, made| Waited {
        path: path.to_path_buf(),
        made,
    };
    assert_eq!(one.outcome(), Some(Outcome::Moved(vec![moved(&a, true)])));
    assert_eq!(
        two.outcome(),
        Some(Outcome::Moved(vec![moved(&notes, false)]))
    );
    assert!(!h.is_client_buffer(mine) && !h.is_client_buffer(buffer_of(&h, &a)));

    let (mut next, before) = editor();
    let (open_a, mine) = (next.open_file(&a).unwrap(), next.open_file(&notes).unwrap());
    next.show(before, ShowIn::Current).unwrap();
    let (one, two) = (Told::default(), Told::default());
    next.wait_again(vec![moved(&a, true)], one.tell());
    next.wait_again(vec![moved(&notes, false)], two.tell());
    assert_eq!(on_screen(&next), before, "nothing moves");
    assert!(next.is_client_buffer(open_a) && next.is_client_buffer(mine));
    next.show(open_a, ShowIn::Current).unwrap();
    assert!(keys(&mut next, ":wq<CR>").is_empty());
    assert_eq!(one.outcome(), Some(Outcome::Done));
    assert!(!next.is_live(open_a), "made for the client, it goes");
    assert_eq!(on_screen(&next), mine, "the other client's file comes up");
    assert!(keys(&mut next, "ZZ").is_empty());
    assert_eq!(two.outcome(), Some(Outcome::Done));
    assert!(next.is_live(mine));

    // A file the session didn't bring back is opened again, as made for the client.
    let gone = dir.file("gone", "g\n");
    let three = Told::default();
    next.wait_again(vec![moved(&gone, false)], three.tell());
    let opened = buffer_of(&next, &gone);
    next.show(opened, ShowIn::Current).unwrap();
    assert!(keys(&mut next, ":q<CR>").is_empty());
    assert_eq!(three.outcome(), Some(Outcome::Done));
    assert!(!next.is_live(opened));
}
