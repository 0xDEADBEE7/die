//! K10b, files write side. Each test works in its own temporary directory.
//!
//! Test plan, by rule:
//!
//! - **The plan row.** A symlink and mode 0755 survive a save. A byte-exact round trip, BOM
//!   included. A rename-over reverts. `:w` refuses and `:w!` writes. A rewrite with the same
//!   bytes leaves no undo step.
//! - **The plan's risks.** A revert during an insert, and a deleted file: one test each.
//! - **Saving through links.** A chain of symlinks, one into another directory, one dangling
//!   (which creates its target). A hard link is written in place: same inode, and the other
//!   name sees the text. Nothing is left beside the file.
//! - **What a save keeps.** Modes 0755 and 0600, the group, and the fingerprint, so the next
//!   poll sees no change. A file in a directory DIE can't write to is written in place.
//! - **What a save refuses.** Files that opened read-only, for `:w` and `:w!` alike. A file you
//!   can't write, for `:w`. Something that's no longer a regular file. A buffer with no file.
//! - **When `:w` does nothing.** An unmodified buffer's file isn't touched. `:w!` writes it. A
//!   file that's gone is written again, as Emacs's `basic-save-buffer` does.
//! - **Reverting.** The poll runs on the clock and stops with the last file. A modified buffer
//!   isn't reloaded, and says so once. A reload is one undo step that `u` takes back. Text
//!   between two changes keeps its marker. A file that appears reloads an empty buffer. A
//!   reload re-checks UTF-8 both ways and tracks the BOM. `:e!` reloads a modified buffer.
//!
//! `tests/prop.rs` runs random sequences of these against a model of the disk, which is also
//! what catches this package's planted bugs.

use die_test_cap as _;

use die_core::*;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, chown, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A fresh directory for one test, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("die-core-save-{}-{name}", std::process::id()));
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
    /// Every name in the directory, sorted, so a stray temporary file shows.
    fn names(&self) -> Vec<String> {
        names_in(&self.0)
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let names = fs::read_dir(dir).unwrap();
    let mut v: Vec<String> = names
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&self.0);
    }
}

const POLL: Duration = Duration::from_secs(1);

fn ctx() -> Ctx {
    Ctx::new(vec![]).unwrap()
}

fn text_of(c: &Ctx, b: BufferId) -> String {
    c.text(b).unwrap().rope().to_string()
}

fn append(c: &mut Ctx, b: BufferId, s: &str) {
    let end = c.text(b).unwrap().len();
    let t = c.text_mut(b).unwrap();
    t.undo_boundary();
    t.tx(Origin::User).insert(end, s).commit().unwrap();
}

/// A change an outside program makes that a fingerprint must see, however coarse the clock:
/// new bytes in place, with a modification time of its own.
fn rewrite(p: &Path, bytes: &[u8]) {
    let later = fs::metadata(p).map_or(SystemTime::now(), |m| m.modified().unwrap());
    fs::write(p, bytes).unwrap();
    let f = fs::File::options().write(true).open(p).unwrap();
    f.set_modified(later + Duration::from_secs(5)).unwrap();
}

/// What `cargo fmt` and most tools do: a new file renamed over the old one.
fn rename_over(p: &Path, bytes: &[u8]) {
    let tmp = p.with_extension("incoming");
    fs::write(&tmp, bytes).unwrap();
    fs::rename(&tmp, p).unwrap();
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o7777
}

fn said(c: &Ctx, words: &[&str]) -> usize {
    c.messages
        .iter()
        .filter(|m| words.iter().all(|w| m.contains(w)))
        .count()
}

// ---- the plan row

#[test]
fn a_symlink_and_mode_0755_survive_a_save() {
    let d = Dir::new("link-mode");
    let real = d.file("real/claude-session", b"#!/bin/sh\n");
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    let link = d.0.join("bin-claude-session");
    symlink(&real, &link).unwrap();
    let mut c = ctx();
    let b = c.open_file(&link).unwrap();
    append(&mut c, b, "exit 0\n");
    c.save(b).unwrap();
    let meta = fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_symlink(), "still a symlink");
    assert_eq!(fs::read_link(&link).unwrap(), real, "to the same place");
    assert_eq!(fs::read(&real).unwrap(), b"#!/bin/sh\nexit 0\n");
    assert_eq!(mode(&real), 0o755);
    assert_eq!(
        c.file(b).unwrap().path,
        link,
        "the buffer still visits the link"
    );
}

#[test]
fn a_save_round_trips_every_byte() {
    let big = "line of text with some é in it\n".repeat(4000);
    for (i, content) in [
        "",
        "\n",
        "\n\n\n",
        "no final newline",
        "lone\rreturn\r",
        "tab\tand ünïcödé 🦀 and \u{2028} and \u{85}\n",
        "   trailing spaces   \n\n",
        "\u{FEFF}a BOM inside the text is text\n",
        big.as_str(),
    ]
    .iter()
    .enumerate()
    {
        for bom in [false, true] {
            let d = Dir::new(&format!("round-{i}-{bom}"));
            let bytes = [
                if bom { &b"\xEF\xBB\xBF"[..] } else { b"" },
                content.as_bytes(),
            ]
            .concat();
            let p = d.file("f", &bytes);
            let mut c = ctx();
            let b = c.open_file(&p).unwrap();
            // An edit and its undo leave the same text, and a forced save writes it.
            append(&mut c, b, "x");
            c.text_mut(b).unwrap().undo().unwrap();
            c.save_force(b).unwrap();
            assert_eq!(fs::read(&p).unwrap(), bytes, "case {i}, bom {bom}");
            assert_eq!(d.names(), ["f"]);
        }
    }
}

#[test]
fn a_rename_over_reverts_an_unmodified_buffer() {
    let d = Dir::new("rename-over");
    let p = d.file("a.rs", b"fn main( ){}\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    rename_over(&p, b"fn main() {}\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "fn main() {}\n");
    assert_eq!(said(&c, &["Reverted", "a.rs"]), 1, "{:?}", c.messages);
}

#[test]
fn w_refuses_a_file_changed_on_disk_and_w_bang_writes() {
    let d = Dir::new("refuse");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "mine\n");
    rewrite(&p, b"theirs\n");
    let e = c.save(b).unwrap_err();
    let Error::Msg(why) = e else { panic!("{e:?}") };
    assert!(
        why.contains("changed on disk") && why.contains(":w!"),
        "{why}"
    );
    assert_eq!(fs::read(&p).unwrap(), b"theirs\n", "nothing was written");
    assert_eq!(text_of(&c, b), "one\nmine\n", "the text stays");
    assert!(c.save(b).is_err(), "and it keeps refusing");
    c.save_force(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"one\nmine\n");
    append(&mut c, b, "more\n");
    c.save(b).unwrap();
    assert_eq!(
        fs::read(&p).unwrap(),
        b"one\nmine\nmore\n",
        ":w works again"
    );
}

#[test]
fn w_refuses_a_rename_over_with_the_same_time_and_size() {
    let d = Dir::new("refuse-inode");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "mine\n");
    let time = fs::metadata(&p).unwrap().modified().unwrap();
    let other = d.file("b.txt", b"ONE\n");
    fs::File::options()
        .write(true)
        .open(&other)
        .unwrap()
        .set_modified(time)
        .unwrap();
    fs::rename(&other, &p).unwrap();
    assert!(c.save(b).is_err());
    assert_eq!(fs::read(&p).unwrap(), b"ONE\n");
}

#[test]
fn a_rewrite_with_the_same_bytes_leaves_no_undo_step() {
    let d = Dir::new("same-bytes");
    let p = d.file("a.txt", b"same\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let rev = c.text(b).unwrap().rev();
    let before = c.messages.len();
    rewrite(&p, b"same\n");
    c.advance(POLL);
    assert_eq!(c.text(b).unwrap().rev(), rev, "no revision");
    assert_eq!(c.text_mut(b).unwrap().undo(), Err(Error::NothingToUndo));
    assert_eq!(c.messages.len(), before, "nothing to say: {:?}", c.messages);
    // The new fingerprint is on record, so `:w` has nothing to refuse.
    append(&mut c, b, "x\n");
    c.save(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"same\nx\n");
}

#[test]
fn a_rewrite_with_the_same_bytes_keeps_what_redo_has() {
    let d = Dir::new("same-bytes-redo");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "two\n");
    c.save(b).unwrap();
    rewrite(&p, b"one\ntwo\n");
    c.advance(POLL);
    c.text_mut(b).unwrap().undo().unwrap();
    assert_eq!(text_of(&c, b), "one\n");
    c.text_mut(b).unwrap().redo().unwrap();
    assert_eq!(text_of(&c, b), "one\ntwo\n");
}

// ---- the plan's risks

#[test]
fn a_change_during_an_insert_waits_for_the_insert_to_end() {
    let d = Dir::new("during-insert");
    let p = d.file("a.txt", b"old\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    c.text_mut(b).unwrap().begin_undo_group();
    rewrite(&p, b"new\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "old\n", "not under the typing");
    c.text_mut(b).unwrap().end_undo_group();
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "new\n", "the next poll after it");
}

#[test]
fn text_typed_in_an_insert_stops_the_reload_and_the_save() {
    let d = Dir::new("typed-in-insert");
    let p = d.file("a.txt", b"old\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let t = c.text_mut(b).unwrap();
    t.begin_undo_group();
    t.tx(Origin::User).insert(0, "typed ").commit().unwrap();
    rewrite(&p, b"new\n");
    c.advance(POLL);
    let t = c.text_mut(b).unwrap();
    t.tx(Origin::User).insert(6, "more ").commit().unwrap();
    t.end_undo_group();
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "typed more old\n");
    assert!(c.save(b).is_err());
    c.text_mut(b).unwrap().undo().unwrap();
    assert_eq!(text_of(&c, b), "old\n", "the insert is one step");
}

#[test]
fn a_deleted_file_keeps_its_text_and_a_save_writes_it_again() {
    for modified in [false, true] {
        let d = Dir::new(&format!("deleted-{modified}"));
        let p = d.file("a.txt", b"text\n");
        let mut c = ctx();
        let b = c.open_file(&p).unwrap();
        if modified {
            append(&mut c, b, "more\n");
        }
        let want = text_of(&c, b);
        fs::remove_file(&p).unwrap();
        c.advance(POLL);
        c.advance(POLL);
        assert_eq!(text_of(&c, b), want);
        assert_eq!(c.file(b).unwrap().fingerprint, None);
        assert_eq!(said(&c, &["a.txt", "deleted"]), 1, "{:?}", c.messages);
        c.save(b).unwrap();
        assert_eq!(fs::read(&p).unwrap(), want.as_bytes());
        assert!(c.file(b).unwrap().fingerprint.is_some());
    }
}

#[test]
fn a_file_deleted_and_back_reloads_an_unmodified_buffer_and_refuses_a_modified_one() {
    let d = Dir::new("deleted-back");
    let p = d.file("a.txt", b"old\n");
    let q = d.file("b.txt", b"old\n");
    let mut c = ctx();
    let (a, b) = (c.open_file(&p).unwrap(), c.open_file(&q).unwrap());
    append(&mut c, b, "mine\n");
    fs::remove_file(&p).unwrap();
    fs::remove_file(&q).unwrap();
    c.advance(POLL);
    fs::write(&p, b"back\n").unwrap();
    fs::write(&q, b"back\n").unwrap();
    c.advance(POLL);
    assert_eq!(text_of(&c, a), "back\n");
    assert_eq!(text_of(&c, b), "old\nmine\n");
    assert!(c.save(b).is_err(), "there's a file there to lose again");
    assert_eq!(fs::read(&q).unwrap(), b"back\n");
}

// ---- saving through links

#[test]
fn a_chain_of_relative_symlinks_into_another_directory_is_followed() {
    let d = Dir::new("chain");
    let real = d.file("store/real.txt", b"one\n");
    fs::create_dir_all(d.0.join("links")).unwrap();
    symlink("../store/real.txt", d.0.join("links/hop")).unwrap();
    symlink("hop", d.0.join("links/last")).unwrap();
    let mut c = ctx();
    let b = c.open_file(&d.0.join("links/last")).unwrap();
    append(&mut c, b, "two\n");
    c.save(b).unwrap();
    assert_eq!(fs::read(&real).unwrap(), b"one\ntwo\n");
    for (name, to) in [("links/hop", "../store/real.txt"), ("links/last", "hop")] {
        assert_eq!(fs::read_link(d.0.join(name)).unwrap(), Path::new(to));
    }
    assert_eq!(names_in(&d.0.join("links")), ["hop", "last"]);
    assert_eq!(names_in(&d.0.join("store")), ["real.txt"]);
}

#[test]
fn a_dangling_symlink_creates_its_target() {
    let d = Dir::new("dangling");
    let link = d.0.join("link");
    symlink("target.txt", &link).unwrap();
    let mut c = ctx();
    let b = c.open_file(&link).unwrap();
    append(&mut c, b, "new\n");
    c.save(b).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(d.0.join("target.txt")).unwrap(), b"new\n");
    assert!(c.file(b).unwrap().fingerprint.is_some());
}

#[test]
fn a_symlink_loop_is_an_error() {
    let d = Dir::new("loop");
    let p = d.file("a.txt", b"x\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    fs::remove_file(&p).unwrap();
    symlink("b.txt", &p).unwrap();
    symlink("a.txt", d.0.join("b.txt")).unwrap();
    assert!(c.save_force(b).is_err());
    assert_eq!(d.names(), ["a.txt", "b.txt"]);
}

#[test]
fn a_hard_linked_file_is_written_in_place() {
    let d = Dir::new("hard");
    let p = d.file("a.txt", b"one\n");
    let other = d.0.join("other.txt");
    fs::hard_link(&p, &other).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
    let inode = fs::metadata(&p).unwrap().ino();
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "two\n");
    c.save(b).unwrap();
    let m = fs::metadata(&p).unwrap();
    assert_eq!(
        (m.ino(), m.nlink()),
        (inode, 2),
        "the same file, both names"
    );
    assert_eq!(fs::read(&other).unwrap(), b"one\ntwo\n");
    assert_eq!(mode(&other), 0o640);
    assert_eq!(d.names(), ["a.txt", "other.txt"]);
    let (ours, disk) = (c.file(b).unwrap().fingerprint, fingerprint(&p));
    assert_eq!(ours, Some(disk));
}

#[test]
fn a_hard_linked_file_that_shrinks_has_no_tail_left() {
    let d = Dir::new("hard-shrink");
    let p = d.file("a.txt", b"one\ntwo\nthree\n");
    fs::hard_link(&p, d.0.join("other.txt")).unwrap();
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let t = c.text_mut(b).unwrap();
    t.tx(Origin::User).delete(3..14).commit().unwrap();
    c.save(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"one");
}

// ---- what a save keeps

fn fingerprint(p: &Path) -> Fingerprint {
    let mut c = ctx();
    let b = c.open_file(p).unwrap();
    c.file(b).unwrap().fingerprint.unwrap()
}

#[test]
fn a_save_keeps_the_mode_and_leaves_nothing_beside_the_file() {
    for m in [0o755, 0o644, 0o600, 0o640, 0o444] {
        let d = Dir::new(&format!("mode-{m:o}"));
        let p = d.file("a", b"one\n");
        fs::set_permissions(&p, fs::Permissions::from_mode(m)).unwrap();
        let mut c = ctx();
        let b = c.open_file(&p).unwrap();
        append(&mut c, b, "two\n");
        c.save_force(b).unwrap();
        assert_eq!(mode(&p), m, "{m:o}");
        assert_eq!(fs::read(&p).unwrap(), b"one\ntwo\n");
        assert_eq!(d.names(), ["a"]);
    }
}

#[test]
fn a_save_keeps_the_group() {
    let groups = std::process::Command::new("id").arg("-G").output().unwrap();
    let groups = String::from_utf8(groups.stdout).unwrap();
    let d = Dir::new("group");
    let p = d.file("a", b"one\n");
    let start = fs::metadata(&p).unwrap().gid();
    let other = groups
        .split_whitespace()
        .filter_map(|g| g.parse::<u32>().ok())
        .find(|g| *g != start);
    // With one group there's nothing to tell apart.
    let Some(other) = other else { return };
    chown(&p, None, Some(other)).unwrap();
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "two\n");
    c.save(b).unwrap();
    assert_eq!(fs::metadata(&p).unwrap().gid(), other);
}

#[test]
fn a_save_records_what_it_wrote_so_the_next_poll_sees_no_change() {
    for hard in [false, true] {
        let d = Dir::new(&format!("record-{hard}"));
        let p = d.file("a.txt", b"one\n");
        if hard {
            fs::hard_link(&p, d.0.join("other")).unwrap();
        }
        let mut c = ctx();
        let b = c.open_file(&p).unwrap();
        append(&mut c, b, "two\n");
        c.save(b).unwrap();
        assert_eq!(c.file(b).unwrap().fingerprint, Some(fingerprint(&p)));
        let rev = c.text(b).unwrap().rev();
        c.advance(POLL);
        assert_eq!(c.text(b).unwrap().rev(), rev);
        c.text_mut(b).unwrap().undo().unwrap();
        assert_eq!(text_of(&c, b), "one\n", "a save leaves the history alone");
    }
}

#[test]
fn a_save_makes_the_buffer_unmodified_so_a_later_change_reloads() {
    let d = Dir::new("saved-reloads");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "two\n");
    c.save(b).unwrap();
    rewrite(&p, b"three\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "three\n");
}

#[test]
fn a_file_in_a_directory_that_cant_be_written_is_written_in_place() {
    let d = Dir::new("locked-dir");
    let p = d.file("a.txt", b"one\n");
    let inode = fs::metadata(&p).unwrap().ino();
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "two\n");
    fs::set_permissions(&d.0, fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes in any directory, so a replacement is fine there (`cargo xtask ci --linux`
    // runs its tests as root).
    let probe = d.0.join("probe");
    let locked = fs::write(&probe, b"").is_err();
    let saved = c.save(b);
    fs::set_permissions(&d.0, fs::Permissions::from_mode(0o755)).unwrap();
    saved.unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"one\ntwo\n");
    if locked {
        assert_eq!(fs::metadata(&p).unwrap().ino(), inode);
    }
    assert_eq!(c.file(b).unwrap().fingerprint, Some(fingerprint(&p)));
}

#[test]
fn a_new_file_is_created_and_a_missing_directory_is_an_error() {
    let d = Dir::new("new");
    let p = d.0.join("new.rs");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    c.save(b).unwrap();
    assert_eq!(
        fs::read(&p).unwrap(),
        b"",
        "a new empty file, as Emacs writes it"
    );
    assert!(c.file(b).unwrap().fingerprint.is_some());
    let q = d.0.join("no/such/dir/x.rs");
    let b = c.open_file(&q).unwrap();
    append(&mut c, b, "x");
    assert!(c.save(b).is_err());
    assert!(!d.0.join("no").exists());
    assert_eq!(text_of(&c, b), "x");
}

// ---- what a save refuses

#[test]
fn files_that_opened_read_only_refuse_w_and_w_bang() {
    let d = Dir::new("uneditable");
    for (name, bytes) in [
        ("latin1.txt", &b"caf\xE9\n"[..]),
        ("nul.bin", &b"a\0b\n"[..]),
        ("dos.txt", &b"one\r\n"[..]),
    ] {
        let p = d.file(name, bytes);
        let mut c = ctx();
        let b = c.open_file(&p).unwrap();
        c.text_mut(b).unwrap().settings_mut().read_only = false;
        append(&mut c, b, "x");
        for r in [c.save(b), c.save_force(b)] {
            let e = format!("{:?}", r.unwrap_err());
            assert!(e.contains(name) && e.contains("not saved"), "{e}");
        }
        assert_eq!(fs::read(&p).unwrap(), bytes, "{name}");
    }
}

#[test]
fn w_refuses_a_file_you_cant_write_and_w_bang_replaces_it() {
    let d = Dir::new("write-protected");
    let p = d.file("a.txt", b"one\n");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
    // Root writes any file, so the refusal only means something when the open really fails
    // (`cargo xtask ci --linux` runs its tests as root).
    let can_write = fs::File::options().write(true).open(&p).is_ok();
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "two\n");
    if !can_write {
        assert!(c.save(b).is_err());
        assert_eq!(fs::read(&p).unwrap(), b"one\n");
    }
    c.save_force(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"one\ntwo\n");
    assert_eq!(mode(&p), 0o444);
}

#[test]
fn something_thats_no_longer_a_regular_file_is_refused_and_left_alone() {
    let d = Dir::new("not-regular");
    let p = d.file("a", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    fs::remove_file(&p).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&p)
            .status()
            .unwrap()
            .success()
    );
    append(&mut c, b, "two\n");
    c.advance(POLL);
    assert!(c.save(b).is_err(), "a FIFO");
    assert!(c.save_force(b).is_err(), "a FIFO");
    assert!(fs::metadata(&p).unwrap().file_type().is_fifo());
    fs::remove_file(&p).unwrap();
    fs::create_dir(&p).unwrap();
    assert!(c.save_force(b).is_err(), "a directory");
    assert!(p.is_dir());
    assert_eq!(text_of(&c, b), "one\ntwo\n");
    assert_eq!(d.names(), ["a"]);
}

#[test]
fn a_buffer_with_no_file_cant_be_saved() {
    let mut c = ctx();
    let b = c.create_text("scratch", "x");
    for r in [c.save(b), c.save_force(b), c.revert_file(b)] {
        let e = format!("{:?}", r.unwrap_err());
        assert!(e.contains("scratch") && e.contains("no file"), "{e}");
    }
}

// ---- when `:w` does nothing

#[test]
fn w_leaves_an_unmodified_files_time_alone_and_w_bang_writes() {
    let d = Dir::new("unmodified");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let fp = fingerprint(&p);
    c.save(b).unwrap();
    assert_eq!(fingerprint(&p), fp, "not touched");
    assert_eq!(said(&c, &["No changes"]), 1, "{:?}", c.messages);
    c.save_force(b).unwrap();
    assert_ne!(fingerprint(&p), fp, "written");
    assert_eq!(fs::read(&p).unwrap(), b"one\n");
}

#[test]
fn w_on_an_unmodified_buffer_doesnt_refuse_a_change_it_wouldnt_overwrite() {
    let d = Dir::new("unmodified-changed");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    rewrite(&p, b"theirs\n");
    c.save(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"theirs\n");
}

// ---- reverting

#[test]
fn the_poll_runs_on_the_clock_and_stops_with_the_last_file() {
    let d = Dir::new("clock");
    let p = d.file("a.txt", b"one\n");
    let q = d.file("b.txt", b"one\n");
    let mut c = ctx();
    assert_eq!(c.next_timer(), None, "no file, no poll");
    let (a, b) = (c.open_file(&p).unwrap(), c.open_file(&q).unwrap());
    let due = c.next_timer().unwrap();
    assert_eq!(due, c.now() + POLL);
    rewrite(&p, b"two\n");
    c.advance(POLL - Duration::from_millis(1));
    assert_eq!(text_of(&c, a), "one\n", "not yet");
    c.advance(Duration::from_millis(1));
    assert_eq!(text_of(&c, a), "two\n");
    rewrite(&p, b"three\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, a), "three\n", "and it repeats");
    c.kill(a);
    assert!(c.next_timer().is_some(), "one file left");
    c.kill(b);
    assert_eq!(c.next_timer(), None, "no file left");
    let a = c.open_file(&p).unwrap();
    rewrite(&p, b"four\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, a), "four\n", "a new file starts it again");
}

/// M0's criterion (c), with the real tool: `rustfmt` on an open file reloads an unmodified
/// buffer, and `:w` refuses on a modified one.
#[test]
fn rustfmt_reloads_an_unmodified_buffer_and_w_refuses_a_modified_one() {
    let d = Dir::new("rustfmt");
    let before = "fn one( ){}\n\nfn keep() {\n    stay();\n}\n\nfn two( ){}\n";
    let after = "fn one() {}\n\nfn keep() {\n    stay();\n}\n\nfn two() {}\n";
    let (p, q) = (
        d.file("a.rs", before.as_bytes()),
        d.file("b.rs", before.as_bytes()),
    );
    let mut c = ctx();
    let (a, b) = (c.open_file(&p).unwrap(), c.open_file(&q).unwrap());
    let cursor = c
        .text_mut(a)
        .unwrap()
        .add_marker(before.find("stay").unwrap(), Gravity::Left);
    let cursor = cursor.unwrap();
    append(&mut c, b, "// mine\n");
    // The toolchain's `rustfmt`, as `cargo fmt` runs it. `ci --linux`'s image doesn't have it.
    let done = std::process::Command::new("rustfmt")
        .args([&p, &q])
        .status();
    if cfg!(target_os = "linux") && !done.as_ref().is_ok_and(|d| d.success()) {
        return;
    }
    assert!(done.unwrap().success());
    assert_eq!(fs::read(&p).unwrap(), after.as_bytes());
    c.advance(POLL);
    assert_eq!(text_of(&c, a), after);
    assert_eq!(c.text(a).unwrap().marker(cursor), after.find("stay"));
    assert_eq!(text_of(&c, b), format!("{before}// mine\n"));
    assert!(c.save(b).is_err());
    assert_eq!(fs::read(&q).unwrap(), after.as_bytes());
}

#[test]
fn check_files_looks_now_without_the_clock() {
    let d = Dir::new("check-now");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    rewrite(&p, b"two\n");
    c.check_files();
    assert_eq!(text_of(&c, b), "two\n");
}

#[test]
fn a_modified_buffer_isnt_reloaded_and_says_so_once() {
    let d = Dir::new("modified");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "mine\n");
    rewrite(&p, b"theirs\n");
    for _ in 0..3 {
        c.advance(POLL);
    }
    assert_eq!(text_of(&c, b), "one\nmine\n");
    assert_eq!(
        said(&c, &["a.txt", "changed on disk"]),
        1,
        "{:?}",
        c.messages
    );
    // The news is good until something settles it: a forced save, say.
    c.save_force(b).unwrap();
    rewrite(&p, b"theirs again\n");
    append(&mut c, b, "more\n");
    c.advance(POLL);
    assert_eq!(
        said(&c, &["a.txt", "changed on disk"]),
        2,
        "{:?}",
        c.messages
    );
}

#[test]
fn a_reload_is_one_undo_step_of_its_own() {
    let d = Dir::new("undo");
    let p = d.file("a.txt", b"one\ntwo\nthree\nfour\nfive\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "six\n");
    c.save(b).unwrap();
    rewrite(&p, b"ONE\ntwo\nthree\nfour\nFIVE\nsix\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "ONE\ntwo\nthree\nfour\nFIVE\nsix\n");
    c.text_mut(b).unwrap().undo().unwrap();
    assert_eq!(
        text_of(&c, b),
        "one\ntwo\nthree\nfour\nfive\nsix\n",
        "the reload"
    );
    c.text_mut(b).unwrap().undo().unwrap();
    assert_eq!(
        text_of(&c, b),
        "one\ntwo\nthree\nfour\nfive\n",
        "the edit before it"
    );
}

#[test]
fn text_between_two_changes_keeps_its_markers() {
    let d = Dir::new("markers");
    let before = "use a;\n\nfn one( ) {}\n\nfn two() {\n    keep();\n}\n\nfn three( ) {}\n";
    let after = "use a;\n\nfn one() {}\n\nfn two() {\n    keep();\n}\n\nfn three() {}\n";
    let p = d.file("a.rs", before.as_bytes());
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let at = before.find("keep").unwrap();
    let t = c.text_mut(b).unwrap();
    let m = t.add_marker(at, Gravity::Right).unwrap();
    let on_changed_line = t
        .add_marker(before.find("three").unwrap(), Gravity::Right)
        .unwrap();
    rewrite(&p, after.as_bytes());
    c.advance(POLL);
    assert_eq!(text_of(&c, b), after);
    let t = c.text(b).unwrap();
    assert_eq!(t.marker(m), after.find("keep"), "between the changes");
    assert_eq!(
        t.marker(on_changed_line),
        after.find("three"),
        "on a changed line"
    );
}

#[test]
fn a_change_inside_a_character_stays_on_character_boundaries() {
    // Each pair shares bytes with a character's start or end, but no whole character.
    for (i, (old, new)) in [
        ("é\n", "è\n"),
        ("é\n", "ĩ\n"),
        ("aé", "aè"),
        ("éa", "ĩa"),
        ("🦀", "🦁"),
        ("é", ""),
        ("", "é"),
    ]
    .iter()
    .enumerate()
    {
        let d = Dir::new(&format!("boundary-{i}"));
        let p = d.file("a.txt", old.as_bytes());
        let mut c = ctx();
        let b = c.open_file(&p).unwrap();
        rewrite(&p, new.as_bytes());
        c.advance(POLL);
        assert_eq!(text_of(&c, b), *new);
        c.text_mut(b).unwrap().undo().unwrap();
        assert_eq!(text_of(&c, b), *old);
    }
}

#[test]
fn moved_lines_reload_exactly() {
    let d = Dir::new("moved");
    let p = d.file("a.txt", "é\nb\nc\nd\nd\ne\n".as_bytes());
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let after = "e\nd\nc\n🦀\nd\nb\né\nno newline";
    rewrite(&p, after.as_bytes());
    c.advance(POLL);
    assert_eq!(text_of(&c, b), after);
    c.text_mut(b).unwrap().undo().unwrap();
    assert_eq!(text_of(&c, b), "é\nb\nc\nd\nd\ne\n");
}

#[test]
fn a_file_that_appears_reloads_an_empty_buffer() {
    let d = Dir::new("appears");
    let p = d.0.join("new.rs");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "");
    fs::write(&p, b"fn main() {}\n").unwrap();
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "fn main() {}\n");
    assert!(c.file(b).unwrap().fingerprint.is_some());
}

#[test]
fn a_reload_checks_utf8_both_ways_and_tracks_the_bom() {
    let d = Dir::new("recheck");
    let p = d.file("a.txt", b"caf\xC3\xA9\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    rewrite(&p, b"caf\xE9\n");
    c.advance(POLL);
    assert_eq!(c.file(b).unwrap().read_only, Some(Uneditable::NotUtf8));
    assert!(c.text(b).unwrap().settings().read_only);
    assert_eq!(
        said(&c, &["a.txt", "not UTF-8", "read-only"]),
        1,
        "{:?}",
        c.messages
    );
    assert_eq!(text_of(&c, b), "caf\u{FFFD}\n");
    assert!(c.save_force(b).is_err());
    assert_eq!(fs::read(&p).unwrap(), b"caf\xE9\n");

    rewrite(&p, b"\xEF\xBB\xBFcaf\xC3\xA9\n");
    c.advance(POLL);
    assert_eq!(c.file(b).unwrap().read_only, None);
    assert!(!c.text(b).unwrap().settings().read_only, "editable again");
    assert!(c.file(b).unwrap().bom);
    assert_eq!(text_of(&c, b), "café\n");
    append(&mut c, b, "x\n");
    c.save(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"\xEF\xBB\xBFcaf\xC3\xA9\nx\n");
}

#[test]
fn only_the_bom_changing_is_no_undo_step_and_a_save_follows_it() {
    let d = Dir::new("bom-only");
    let p = d.file("a.txt", b"x\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    let rev = c.text(b).unwrap().rev();
    rewrite(&p, b"\xEF\xBB\xBFx\n");
    c.advance(POLL);
    assert_eq!(c.text(b).unwrap().rev(), rev);
    assert!(c.file(b).unwrap().bom);
    c.save_force(b).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"\xEF\xBB\xBFx\n");
}

#[test]
fn a_read_only_buffer_still_reloads() {
    let d = Dir::new("ro-reloads");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    c.text_mut(b).unwrap().settings_mut().read_only = true;
    rewrite(&p, b"two\n");
    c.advance(POLL);
    assert_eq!(text_of(&c, b), "two\n");
    assert!(
        c.text(b).unwrap().settings().read_only,
        "and stays read-only"
    );
}

#[test]
fn revert_file_reloads_a_modified_buffer_and_u_takes_it_back() {
    let d = Dir::new("e-bang");
    let p = d.file("a.txt", b"one\n");
    let mut c = ctx();
    let b = c.open_file(&p).unwrap();
    append(&mut c, b, "mine\n");
    rewrite(&p, b"theirs\n");
    c.revert_file(b).unwrap();
    assert_eq!(text_of(&c, b), "theirs\n");
    append(&mut c, b, "x\n");
    c.save(b).unwrap();
    assert_eq!(
        fs::read(&p).unwrap(),
        b"theirs\nx\n",
        "nothing left to refuse"
    );
    c.text_mut(b).unwrap().undo().unwrap();
    c.text_mut(b).unwrap().undo().unwrap();
    assert_eq!(text_of(&c, b), "one\nmine\n");
    // With the file gone there's nothing to reload, and the text stays.
    fs::remove_file(&p).unwrap();
    assert!(c.revert_file(b).is_err());
    assert_eq!(text_of(&c, b), "one\nmine\n");
}
