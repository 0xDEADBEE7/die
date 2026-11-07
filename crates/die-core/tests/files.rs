//! K10a, files read side: opening a file into a buffer, the UTF-8 check, the
//! fingerprint, and per-path settings. Each test works in its own temporary directory.

use die_test_cap as _;

use die_core::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

/// A fresh directory for one test, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("die-core-files-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ctx() -> Ctx {
    Ctx::new(vec![]).unwrap()
}

fn text_of(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).unwrap().rope().to_string()
}

#[test]
fn opens_utf8_text_editable() {
    let d = Dir::new("utf8");
    let p = d.file("main.rs", "fn main() {}\n// é\n".as_bytes());
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert_eq!(text_of(&c, b), "fn main() {}\n// é\n");
    assert_eq!(c.buffer_name(b), Some("main.rs"));
    let f = c.file(b).unwrap();
    assert_eq!(f.path, p);
    assert_eq!(f.read_only, None);
    assert!(!f.bom);
    assert!(f.fingerprint.is_some());
    assert!(!c.text(b).unwrap().settings().read_only);
}

#[test]
fn opening_again_returns_the_same_buffer() {
    let d = Dir::new("again");
    let p = d.file("a.txt", b"x\n");
    let link = d.0.join("link.txt");
    symlink(&p, &link).unwrap();
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert_eq!(c.open_file(&p).unwrap(), b);
    d.file("sub/x", b"");
    assert_eq!(c.open_file(&d.0.join("sub/../a.txt")).unwrap(), b);
    assert_eq!(
        c.open_file(&link).unwrap(),
        b,
        "a symlink to an open file finds its buffer"
    );
}

#[test]
fn a_buffer_with_the_exact_path_wins_over_one_reaching_the_file_another_way() {
    let d = Dir::new("exact");
    let (p, alias) = (d.0.join("new.rs"), d.0.join("alias.rs"));
    symlink(&p, &alias).unwrap();
    // Each Ctx hashes its buffers in a different order, so a pick that depends on
    // the order fails in some of these rounds.
    for _ in 0..10 {
        let _ = fs::remove_file(&p);
        let mut c = ctx();
        let a = c.open_file(&alias).unwrap();
        let b = c.open_file(&p).unwrap();
        assert_ne!(a, b, "neither file existed, so nothing tied them together");
        fs::write(&p, b"x\n").unwrap();
        assert_eq!(c.open_file(&p).unwrap(), b);
        assert_eq!(c.open_file(&alias).unwrap(), a);
    }
}

#[test]
fn dot_dot_is_resolved_by_name_as_emacs_does() {
    let d = Dir::new("dotdot");
    let top = d.file("f.txt", b"top\n");
    d.file("real/f.txt", b"real\n");
    d.file("real/sub/x", b"");
    symlink(d.0.join("real/sub"), d.0.join("link")).unwrap();
    let mut c = ctx();
    let b = c.open_file(&d.0.join("link/../f.txt")).unwrap();
    assert_eq!(c.file(b).unwrap().path, top, "link/.. is link's directory");
    assert_eq!(text_of(&c, b), "top\n");
}

#[test]
fn same_names_in_different_directories_are_unique() {
    let d = Dir::new("names");
    let a = d.file("one/lib.rs", b"a\n");
    let b = d.file("two/lib.rs", b"b\n");
    let mut c = ctx();
    let ba = c.open_file(&a).unwrap();
    let bb = c.open_file(&b).unwrap();
    assert_eq!(c.buffer_name(ba), Some("lib.rs"));
    assert_eq!(c.buffer_name(bb), Some("lib.rs<2>"));
}

#[test]
fn a_bom_is_kept_out_of_the_text_and_remembered() {
    let d = Dir::new("bom");
    let p = d.file("bom.txt", b"\xEF\xBB\xBFhello\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert_eq!(text_of(&c, b), "hello\n");
    assert!(c.file(b).unwrap().bom);
    assert_eq!(c.file(b).unwrap().read_only, None);
}

#[test]
fn files_that_cant_be_edited_safely_open_read_only_with_a_message() {
    let d = Dir::new("ro");
    for (name, bytes, why) in [
        ("latin1.txt", &b"caf\xE9\n"[..], Uneditable::NotUtf8),
        ("nul.bin", &b"a\0b\n"[..], Uneditable::NulBytes),
        ("dos.txt", &b"one\r\ntwo\r\n"[..], Uneditable::Crlf),
    ] {
        let p = d.file(name, bytes);
        let mut c = ctx();
        let b = c.open_file(&p).unwrap();
        assert_eq!(c.file(b).unwrap().read_only, Some(why), "{name}");
        assert!(c.text(b).unwrap().settings().read_only, "{name}");
        assert!(
            c.messages
                .iter()
                .any(|m| m.contains(name) && m.contains("read-only")),
            "{name}: {:?}",
            c.messages
        );
        let tx = c
            .text_mut(b)
            .unwrap()
            .tx(Origin::User)
            .insert(0, "x")
            .commit();
        assert!(tx.is_err(), "{name}: a user edit is refused");
    }
}

#[test]
fn not_utf8_text_shows_with_replacement_characters() {
    let d = Dir::new("lossy");
    let p = d.file("latin1.txt", b"caf\xE9\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert_eq!(text_of(&c, b), "caf\u{FFFD}\n");
}

#[test]
fn a_missing_file_opens_empty_with_no_fingerprint() {
    let d = Dir::new("missing");
    let p = d.0.join("new.rs");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert_eq!(text_of(&c, b), "");
    let f = c.file(b).unwrap();
    assert_eq!(f.fingerprint, None);
    assert_eq!(f.path, p);
    assert!(!p.exists(), "opening doesn't create the file");
    let q = d.0.join("other.rs");
    let b2 = c.open_file(&q).unwrap();
    assert_ne!(b2, b, "two missing files are two buffers");
    assert_eq!(c.file(b2).unwrap().path, q);
}

#[test]
fn paths_that_arent_readable_regular_files_are_errors() {
    let d = Dir::new("errors");
    let mut c = ctx();
    assert!(
        c.open_file(Path::new("relative.rs")).is_err(),
        "paths must be absolute"
    );
    assert!(c.open_file(&d.0).is_err(), "a directory");
    let p = d.file("secret.txt", b"x\n");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads any file, so this only means something when the read really fails
    // (`cargo xtask ci --linux` runs its tests as root).
    if fs::read(&p).is_err() {
        assert!(c.open_file(&p).is_err(), "a file you can't read");
    }
    // Opening a FIFO to read waits for a writer, so a regression hangs: allow 2 s.
    let fifo = d.0.join("fifo");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    assert!(made.unwrap().success());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ctx().open_file(&fifo).is_err());
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(true), "a FIFO");
}

#[test]
fn file_settings_apply_by_path_on_open() {
    let d = Dir::new("settings");
    let rs = d.file("a.rs", b"x\n");
    let md = d.file("a.md", b"x\n");
    let mut c = ctx();
    c.set_file_settings(|path, s| {
        if path.extension().is_some_and(|e| e == "rs") {
            s.line_numbers = LineNumbers::Relative;
            s.tab_width = 4;
        }
    });
    let b = c.open_file(&rs).unwrap();
    let s = c.text(b).unwrap().settings().clone();
    assert_eq!((s.line_numbers, s.tab_width), (LineNumbers::Relative, 4));
    let b = c.open_file(&md).unwrap();
    assert_eq!(c.text(b).unwrap().settings().line_numbers, LineNumbers::Off);
}

#[test]
fn file_settings_cant_make_a_read_only_file_editable() {
    let d = Dir::new("settings-ro");
    let p = d.file("dos.txt", b"a\r\n");
    let mut c = ctx();
    c.set_file_settings(|_, s| s.read_only = false);
    let b = c.open_file(&p).unwrap();
    assert!(c.text(b).unwrap().settings().read_only);
}

#[test]
fn a_panic_in_file_settings_opens_the_file_with_the_defaults() {
    let d = Dir::new("settings-panic");
    let p = d.file("Makefile", b"all:\n");
    let mut c = ctx();
    c.set_file_settings(|path, s| {
        s.tab_width = 2;
        assert!(path.extension().is_some(), "config bug");
    });
    let b = c.open_file(&p).unwrap();
    assert_eq!(c.file(b).unwrap().path, p, "the buffer visits the file");
    let tab_width = c.text(b).unwrap().settings().tab_width;
    assert_eq!(tab_width, Settings::default().tab_width);
    assert!(
        c.messages
            .iter()
            .any(|m| m.contains("Makefile") && m.contains("config bug")),
        "{:?}",
        c.messages
    );
}

/// The fingerprint core records when it opens `p` now.
fn fingerprint(p: &Path) -> Fingerprint {
    let mut c = ctx();
    let b = c.open_file(p).unwrap();
    c.file(b).unwrap().fingerprint.unwrap()
}

/// Modification time, size and inode, as the file system reports them.
fn stat(p: &Path) -> (SystemTime, u64, u64) {
    let m = fs::metadata(p).unwrap();
    (m.modified().unwrap(), m.len(), m.ino())
}

fn set_modified(p: &Path, t: SystemTime) {
    let f = fs::File::options().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

#[test]
fn the_fingerprint_changes_with_size_time_or_inode() {
    let d = Dir::new("fp");
    let p = d.file("f.txt", b"one\n");
    let fp = fingerprint(&p);
    assert_eq!(fingerprint(&p), fp, "stable when nothing changes");
    // Each step changes one of the three and keeps the other two.
    let (time, len, inode) = stat(&p);

    let later = time + Duration::from_secs(5);
    set_modified(&p, later);
    assert_eq!(stat(&p), (later, len, inode));
    assert_ne!(fingerprint(&p), fp, "modification time");

    fs::write(&p, b"one two\n").unwrap();
    set_modified(&p, time);
    assert_eq!(stat(&p), (time, len + 4, inode));
    assert_ne!(fingerprint(&p), fp, "size");

    fs::write(&p, b"one\n").unwrap();
    set_modified(&p, time);
    assert_eq!(fingerprint(&p), fp, "all three as they were");

    let other = d.file("other.txt", b"ONE\n");
    set_modified(&other, time);
    fs::rename(&other, &p).unwrap();
    let new_inode = stat(&p).2;
    assert_ne!(new_inode, inode);
    assert_eq!(stat(&p), (time, len, new_inode));
    assert_ne!(
        fingerprint(&p),
        fp,
        "a rename over the file changes the inode"
    );
}

#[test]
fn killing_a_buffer_forgets_its_file() {
    let d = Dir::new("kill");
    let p = d.file("k.txt", b"x\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert!(c.kill(b));
    assert_eq!(c.file(b), None);
    let b2 = c.open_file(&p).unwrap();
    assert_ne!(b2, b, "a new buffer, not the killed one");
}

fn at(path: PathBuf, line: Option<usize>, col: Option<usize>) -> FileTarget {
    FileTarget { path, line, col }
}

#[test]
fn a_name_says_a_line_and_column_with_or_without_the_compilers_colon() {
    let base = Path::new("/nowhere");
    let read = |name: &str| FileTarget::parse(name.as_ref(), base);
    let foo = base.join("foo.rs");
    assert_eq!(read("foo.rs"), at(foo.clone(), None, None));
    assert_eq!(read("foo.rs:"), at(foo.clone(), None, None));
    assert_eq!(read("foo.rs:12"), at(foo.clone(), Some(12), None));
    assert_eq!(read("foo.rs:12:"), at(foo.clone(), Some(12), None));
    assert_eq!(read("foo.rs:12:3"), at(foo.clone(), Some(12), Some(3)));
    assert_eq!(read("foo.rs:12:3:"), at(foo.clone(), Some(12), Some(3)));
    // Not numbers, or nothing before them, so these are names.
    assert_eq!(read("foo:bar"), at(base.join("foo:bar"), None, None));
    assert_eq!(read("foo:+5"), at(base.join("foo:+5"), None, None));
    assert_eq!(read(":12"), at(base.join(":12"), None, None));
    assert_eq!(
        read("/abs/foo:7"),
        at(PathBuf::from("/abs/foo"), Some(7), None)
    );
}

#[test]
fn a_file_that_exists_with_the_colons_in_its_name_wins() {
    let d = Dir::new("colons");
    d.file("a:12:3", b"");
    d.file("b:12", b"");
    d.file("c:", b"");
    let read = |name: &str| FileTarget::parse(name.as_ref(), &d.0);
    assert_eq!(read("a:12:3"), at(d.0.join("a:12:3"), None, None));
    assert_eq!(read("a:12:3:"), at(d.0.join("a:12:3"), None, None));
    assert_eq!(read("b:12:7"), at(d.0.join("b:12"), Some(7), None));
    assert_eq!(read("c:"), at(d.0.join("c:"), None, None));
    assert_eq!(read("a:12"), at(d.0.join("a"), Some(12), None));
}

#[test]
fn plus_line_goes_with_the_name_after_it_and_comes_back_from_its_arguments() {
    let base = Path::new("/nowhere");
    let read = |names: &[&str]| {
        let names = names.iter().map(std::ffi::OsString::from);
        FileTarget::parse_args(names, base)
    };
    let named = |line, col, name: &str| at(base.join(name), line, col);
    assert_eq!(
        read(&["+12", "foo"]),
        Ok(vec![named(Some(12), None, "foo")])
    );
    assert_eq!(
        read(&["+12:3", "foo:9"]),
        Ok(vec![named(Some(12), Some(3), "foo:9")])
    );
    assert_eq!(
        read(&["+x", "foo:9"]),
        Ok(vec![named(None, None, "+x"), named(Some(9), None, "foo")])
    );
    assert!(read(&["a", "+12"]).unwrap_err().contains("+12"));
    let targets = read(&["+12:3", "foo:9", "bar:4", "baz"]).unwrap();
    let args = targets.iter().flat_map(FileTarget::to_args);
    assert_eq!(
        FileTarget::parse_args(args, Path::new("/")).unwrap(),
        targets
    );
}

#[test]
fn a_target_opens_in_the_focused_window_at_its_line_and_column_and_clamps() {
    let d = Dir::new("target");
    let p = d.file("t.txt", "one\néa b\nlast\n".as_bytes());
    let mut c = ctx();
    let scratch = c.create_text("scratch", "");
    let w = c.open_window(scratch, 80, 24).unwrap();
    let mut go = |line, col| {
        let b = c.open_target(&at(p.clone(), line, col)).unwrap();
        assert_eq!(c.focused(), Some(w));
        assert_eq!(c.focused_buffer(), Some(b));
        c.text(b).unwrap().cursor(w).unwrap()
    };
    assert_eq!(go(None, None), 0);
    assert_eq!(go(Some(2), None), 4);
    assert_eq!(go(Some(2), Some(3)), 7, "a column counts characters");
    assert_eq!(go(Some(2), Some(99)), 9, "past the line's end, its end");
    assert_eq!(
        go(Some(99), Some(2)),
        11,
        "past the last line, the last line"
    );
    assert_eq!(go(Some(0), Some(0)), 0);
    assert_eq!(go(None, None), 0, "no position leaves the cursor");
}

#[test]
fn a_target_takes_a_missing_file_as_a_new_buffer_and_leaves_a_terminal_alone() {
    let d = Dir::new("terminal");
    let mut c = ctx();
    let code = c.create_text("code", "");
    let term = c.create_text("term", "");
    let left = c.open_window(code, 80, 24).unwrap();
    let right = c.split_window(left, Side::Right).unwrap();
    c.focus(right);
    c.show(term, ShowIn::Current).unwrap();
    c.set_state_of(term, State::Passthrough);
    let b = c
        .open_target(&at(d.0.join("new.txt"), Some(5), None))
        .unwrap();
    assert_eq!(c.focused(), Some(left));
    assert_eq!(c.focused_buffer(), Some(b));
    assert_eq!(text_of(&c, b), "");
    assert_eq!(c.text(b).unwrap().cursor(left), Some(0));
}

#[test]
fn a_target_splits_a_terminal_that_has_no_other_window_to_go_to() {
    let d = Dir::new("only-terminal");
    let mut c = ctx();
    let term = c.create_text("term", "");
    let w = c.open_window(term, 80, 24).unwrap();
    c.focus(w);
    c.set_state_of(term, State::Passthrough);
    let b = c.open_target(&at(d.0.join("new.txt"), None, None)).unwrap();
    assert_eq!(c.window_buffer(w), Some(term), "it stays on screen");
    assert_eq!(c.focused_buffer(), Some(b));
    assert_ne!(c.focused(), Some(w));
}
