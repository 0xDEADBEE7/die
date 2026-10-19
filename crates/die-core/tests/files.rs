//! K10a, files read side: opening a file into a buffer, the UTF-8 check, the
//! fingerprint, and per-path settings. Each test works in its own temporary directory.

use die_core::*;
use std::fs;
use std::path::{Path, PathBuf};

/// A fresh directory for one test, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("die-core-files-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        // Canonical, so paths compare equal to what core resolves (/var is /private/var).
        Dir(p.canonicalize().unwrap())
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
    assert_eq!(f.fingerprint, Some(Fingerprint::of(&p).unwrap()));
    assert!(!c.text(b).unwrap().settings().read_only);
}

#[test]
fn opening_again_returns_the_same_buffer() {
    let d = Dir::new("again");
    let p = d.file("a.txt", b"x\n");
    let link = d.0.join("link.txt");
    std::os::unix::fs::symlink(&p, &link).unwrap();
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
    assert_eq!(c.find_file_buffer(&p), Some(b));
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
        ("latin1.txt", &b"caf\xE9\n"[..], ReadOnly::NotUtf8),
        ("nul.bin", &b"a\0b\n"[..], ReadOnly::NulBytes),
        ("dos.txt", &b"one\r\ntwo\r\n"[..], ReadOnly::Crlf),
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
}

#[test]
fn a_directory_or_unreadable_path_is_an_error() {
    let d = Dir::new("dir");
    let mut c = ctx();
    assert!(c.open_file(&d.0).is_err());
    assert!(
        c.open_file(Path::new("relative.rs")).is_err(),
        "paths must be absolute"
    );
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
fn the_fingerprint_changes_with_size_time_or_inode() {
    let d = Dir::new("fp");
    let p = d.file("f.txt", b"one\n");
    let fp = Fingerprint::of(&p).unwrap();
    assert_eq!(
        Fingerprint::of(&p).unwrap(),
        fp,
        "stable when nothing changes"
    );

    fs::write(&p, b"one two\n").unwrap();
    assert_ne!(Fingerprint::of(&p).unwrap(), fp, "size");

    let same_size = Fingerprint::of(&p).unwrap();
    let other = d.file("other.txt", b"ONE TWO\n");
    fs::rename(&other, &p).unwrap();
    assert_ne!(
        Fingerprint::of(&p).unwrap(),
        same_size,
        "a rename over the file changes the inode"
    );

    let before = Fingerprint::of(&p).unwrap();
    let f = fs::File::options().write(true).open(&p).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
        .unwrap();
    assert_ne!(Fingerprint::of(&p).unwrap(), before, "modification time");
}

#[test]
fn killing_a_buffer_forgets_its_file() {
    let d = Dir::new("kill");
    let p = d.file("k.txt", b"x\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    assert!(c.kill(b));
    assert_eq!(c.file(b), None);
    assert_eq!(c.find_file_buffer(&p), None);
    let b2 = c.open_file(&p).unwrap();
    assert_ne!(b2, b);
}
