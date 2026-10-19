//! K10a, files read side: opening a file into a buffer, the UTF-8 check, the
//! fingerprint, and per-path settings. Each test works in its own temporary directory.

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
