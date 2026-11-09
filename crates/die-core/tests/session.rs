//! K12 and K13: the session file, restoring from it, and when it's written.

use die_test_cap as _;

use die_core::session::{FILE, PREVIOUS};
use die_core::{
    Config, Ctx, Extension, Harness, Input, Origin, RegKind, Register, Registry, Selection, Shape,
    ShowIn, Side, Uneditable,
};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// A fresh directory for one test, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("die-session-{}-{n}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Dir(dir.canonicalize().unwrap())
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fresh() -> Harness {
    Harness::new(vec![]).unwrap()
}

/// A harness keeping its session in `dir`, restored from it.
fn kept_in(dir: &Path) -> Harness {
    let config = Config {
        session: Some(dir.to_path_buf()),
        ..Config::default()
    };
    Harness::with_config(vec![], config).unwrap()
}

fn edit(ctx: &mut Ctx, name: &str, at: usize, text: &str) {
    let buf = ctx.find_buffer(name).unwrap();
    let t = ctx.text_mut(buf).unwrap();
    t.tx(Origin::User).insert(at, text).commit().unwrap();
}

fn text_of(ctx: &Ctx, name: &str) -> String {
    let buf = ctx.find_buffer(name).unwrap();
    ctx.text(buf).unwrap().rope().to_string()
}

fn modified(ctx: &Ctx, name: &str) -> bool {
    let buf = ctx.find_buffer(name).unwrap();
    ctx.text(buf).unwrap().modified()
}

fn said(ctx: &Ctx, what: &str) -> bool {
    ctx.messages.iter().any(|m| m.contains(what))
}

#[test]
fn every_field_round_trips() {
    let dir = Dir::new();
    let unsaved = dir.file("unsaved.txt", "\u{feff}on disk\n");
    let clean = dir.file("clean.txt", "one\ntwo\nthree\n");
    let gone = dir.file("gone.txt", "kept text\n");
    let crlf = dir.file("crlf.txt", "a\r\nb\r\n");
    let turned = dir.file("turned.txt", "a\n");
    let new = dir.0.join("new.txt");
    // APFS won't make such a name, but other file systems do.
    let odd = PathBuf::from(std::ffi::OsStr::from_bytes(b"/nowhere/odd\xff.txt"));
    let oddly = r#"{"buffers":[{"name":"odd","text":"odd\n","modified":true,
        "file":{"path":{"bytes":[47,110,111,119,104,101,114,101,47,111,100,100,255,46,116,120,116]}}}]}"#;

    let mut a = fresh();
    a.set_window_size(40, 10);
    a.restore_session(oddly).unwrap();
    for path in [&clean, &gone, &crlf, &turned, &new] {
        a.open_file(path).unwrap();
    }
    let flagged = a.find_buffer("clean.txt").unwrap();
    a.text_mut(flagged).unwrap().settings_mut().read_only = true;
    let scratch = a.create_text("*notes*", "scratch text");
    a.text_mut(scratch).unwrap().settings_mut().read_only = true;
    a.set_directory(scratch, "/tmp");
    fs::remove_file(&gone).unwrap();
    a.check_files(); // the text stays, and now only the session has it
    let shown = a.open_file(&unsaved).unwrap();
    a.show(shown, ShowIn::Current).unwrap();
    let w = a.focused().unwrap();
    edit(&mut a, "unsaved.txt", 0, "EDIT ");
    let t = a.text_mut(shown).unwrap();
    t.set_cursor(w, 7);
    t.set_scroll(w, 0, 0);
    let before = a.file(shown).unwrap().clone();

    let saved = a.session_json();
    fs::write(&turned, "a\r\nb\r\n").unwrap();

    let mut b = fresh();
    b.set_window_size(40, 10);
    b.restore_session(&saved).unwrap();

    let buf = b.find_buffer("unsaved.txt").unwrap();
    assert_eq!(text_of(&b, "unsaved.txt"), "EDIT on disk\n");
    assert!(modified(&b, "unsaved.txt"));
    assert_eq!(b.file(buf), Some(&before), "path, fingerprint and BOM");
    let w = b.focused().unwrap();
    assert_eq!(b.window_buffer(w), Some(buf));
    assert_eq!(b.text(buf).unwrap().cursor(w), Some(7));

    assert_eq!(text_of(&b, "clean.txt"), "one\ntwo\nthree\n");
    assert!(!modified(&b, "clean.txt"));
    let read_only = |name| {
        b.text(b.find_buffer(name).unwrap())
            .unwrap()
            .settings()
            .read_only
    };
    assert!(read_only("clean.txt"), "a clean file keeps its flag");
    assert!(read_only("crlf.txt"), "as it opened");
    assert!(read_only("turned.txt"), "it can't be edited now");
    assert_eq!(text_of(&b, "gone.txt"), "kept text\n");
    let g = b.find_buffer("gone.txt").unwrap();
    assert_eq!(b.file(g).unwrap().fingerprint, None);
    let c = b.find_buffer("crlf.txt").unwrap();
    assert_eq!(b.file(c).unwrap().read_only, Some(Uneditable::Crlf));
    assert_eq!(b.file(b.find_buffer("new.txt").unwrap()).unwrap().path, new);
    let o = b.find_buffer("odd\u{fffd}.txt").unwrap();
    assert_eq!(b.file(o).unwrap().path, odd);
    assert_eq!(text_of(&b, "odd\u{fffd}.txt"), "odd\n");

    let n = b.find_buffer("*notes*").unwrap();
    assert_eq!(text_of(&b, "*notes*"), "scratch text");
    assert!(b.text(n).unwrap().settings().read_only);
    assert_eq!(b.directory(n), Some(Path::new("/tmp")));
    // The most recently used comes first, as it did.
    assert_eq!(b.buffer_list()[0], buf);
}

#[test]
fn a_view_comes_back_on_a_buffer_shown_later() {
    let dir = Dir::new();
    let long: String = (0..100).map(|i| format!("line {i}\n")).collect();
    let path = dir.file("long.txt", &long);
    let mut a = fresh();
    a.set_window_size(40, 10);
    let buf = a.open_file(&path).unwrap();
    let (_, w) = a.open_text("other", "x").unwrap();
    a.show(buf, die_core::ShowIn::Current).unwrap();
    let t = a.text_mut(buf).unwrap();
    let pos = t.line_start(50);
    t.set_cursor(w, pos);
    t.set_scroll(w, pos, 0);
    a.top_line(w);
    a.open_text("third", "y").unwrap();

    let mut b = fresh();
    b.set_window_size(40, 10);
    b.restore_session(&a.session_json()).unwrap();
    let buf = b.find_buffer("long.txt").unwrap();
    let w = b.focused().unwrap();
    b.show(buf, die_core::ShowIn::Current).unwrap();
    assert_eq!(b.text(buf).unwrap().cursor(w), Some(pos));
    assert_eq!(b.top_line(w), Some(50));
}

#[test]
fn a_file_that_changed_while_down_makes_the_restored_text_refuse() {
    let dir = Dir::new();
    let path = dir.file("f.txt", "old\n");
    let mut a = fresh();
    a.open_file(&path).unwrap();
    edit(&mut a, "f.txt", 0, "mine ");
    let saved = a.session_json();

    // Unchanged on disk: the fingerprint matches and `:w` writes.
    let mut b = fresh();
    b.restore_session(&saved).unwrap();
    let buf = b.find_buffer("f.txt").unwrap();
    b.check_files();
    assert_eq!(text_of(&b, "f.txt"), "mine old\n");
    b.save(buf).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine old\n");

    // Changed on disk: the unsaved text stays, and `:w` refuses.
    fs::write(&path, "someone else's, and longer\n").unwrap();
    let mut c = fresh();
    c.restore_session(&saved).unwrap();
    let buf = c.find_buffer("f.txt").unwrap();
    c.check_files();
    assert_eq!(text_of(&c, "f.txt"), "mine old\n");
    let refused = c.save(buf).unwrap_err().to_string();
    assert!(refused.contains("changed on disk"), "{refused}");
    assert!(fs::read_to_string(&path).unwrap().starts_with("someone"));
}

#[test]
fn a_clean_file_reads_the_disk_and_the_cursor_fits() {
    let dir = Dir::new();
    let path = dir.file("f.txt", "a long first version\n");
    let gone = dir.file("gone.txt", "gone\n");
    let mut a = fresh();
    a.open_file(&gone).unwrap();
    let buf = a.open_file(&path).unwrap();
    let w = a.open_window(buf, 80, 24).unwrap();
    a.text_mut(buf).unwrap().set_cursor(w, 15);
    let saved = a.session_json();
    assert!(!saved.contains("a long first version"), "{saved}");

    fs::write(&path, "é\n").unwrap();
    fs::remove_file(&gone).unwrap();
    let mut b = fresh();
    b.restore_session(&saved).unwrap();
    assert_eq!(text_of(&b, "f.txt"), "é\n");
    let buf = b.find_buffer("f.txt").unwrap();
    let w = b.focused().unwrap();
    let t = b.text(buf).unwrap();
    assert!(t.cursor(w).unwrap() <= t.len());
    // A clean buffer whose file went has no text to keep.
    assert_eq!(b.find_buffer("gone.txt"), None);
    assert!(said(&b, "gone.txt is gone from disk"));
}

#[test]
fn a_corrupt_buffer_is_dropped_alone_and_kept_aside() {
    let dir = Dir::new();
    let session = r#"{"version":1,"focus":2,"buffers":[
        {"name":"a","text":"first"},
        {"name":"bad","text":17},
        {"name":"c","text":"third","cursor":99999,"top":"nonsense"},
        {"name":"d","text":"fourth","modified":true,"cursor":99999}
    ]}"#;
    fs::write(dir.0.join(FILE), session).unwrap();
    let h = kept_in(&dir.0);
    assert_eq!(text_of(&h, "a"), "first");
    assert_eq!(text_of(&h, "d"), "fourth");
    assert!(modified(&h, "d"));
    assert_eq!(h.find_buffer("bad"), None);
    assert_eq!(h.find_buffer("c"), None);
    assert!(said(&h, "dropped bad"));
    let names: Vec<String> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let kept = names.iter().find(|n| n.starts_with("session.rejected-"));
    let kept = fs::read_to_string(dir.0.join(kept.unwrap())).unwrap();
    assert_eq!(kept, session);
}

#[test]
fn a_corrupt_file_falls_back_on_the_previous_copy() {
    let dir = Dir::new();
    fs::write(dir.0.join(FILE), "{\"version\":1,\"buffers\":[{\"na").unwrap();
    let previous = r#"{"version":1,"buffers":[{"name":"saved","text":"from before"}]}"#;
    fs::write(dir.0.join(PREVIOUS), previous).unwrap();
    let h = kept_in(&dir.0);
    assert_eq!(text_of(&h, "saved"), "from before");
    assert!(said(&h, "session.json can't be read"));

    // With both unreadable, nothing starts up from them, and each is kept aside on its own.
    let dir = Dir::new();
    fs::write(dir.0.join(FILE), "{ the newer").unwrap();
    fs::write(dir.0.join(PREVIOUS), "{ the older").unwrap();
    kept_in(&dir.0);
    let kept = fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().path());
    let kept: Vec<String> = kept
        .filter(|p| p.to_string_lossy().contains("session.rejected-"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept.contains(&"{ the newer".to_string()), "{kept:?}");
}

#[test]
fn a_crash_between_the_renames_leaves_the_previous_copy() {
    let dir = Dir::new();
    let previous = r#"{"version":1,"buffers":[{"name":"saved","text":"from before"}]}"#;
    fs::write(dir.0.join(PREVIOUS), previous).unwrap();
    fs::write(dir.0.join("session.tmp"), "half a wri").unwrap();
    let h = kept_in(&dir.0);
    assert_eq!(text_of(&h, "saved"), "from before");
    assert!(h.messages.is_empty(), "{:?}", h.messages);
}

/// Build N reading build N+1's file: a newer version, fields it doesn't know, at both levels.
#[test]
fn a_build_reads_a_newer_builds_file() {
    let mut h = fresh();
    let newer = r#"{"version":2,"tabs":[{"x":1}],"focus":0,"buffers":[
        {"name":"n","text":"newer","modified":true,"selection":{"anchor":1},"cursor":2,
         "file":null,"future":{"deep":[1,2,3]}}
    ]}"#;
    h.restore_session(newer).unwrap();
    assert_eq!(text_of(&h, "n"), "newer");
    assert!(modified(&h, "n"));
    let w = h.focused().unwrap();
    let buf = h.find_buffer("n").unwrap();
    assert_eq!(h.text(buf).unwrap().cursor(w), Some(2));
}

/// Build N+1 reading build N's file: whatever fields it lacks take their defaults.
#[test]
fn a_build_reads_an_older_builds_file() {
    let dir = Dir::new();
    let path = dir.file("f.txt", "on disk\n");
    let older = format!(
        r#"{{"version":1,"buffers":[
            {{"name":"only a name"}},
            {{"name":"f.txt","file":{{"path":{:?}}},"text":"unsaved","modified":true}}
        ]}}"#,
        path.to_str().unwrap()
    );
    let mut h = fresh();
    h.restore_session(&older).unwrap();
    assert_eq!(text_of(&h, "only a name"), "");
    assert_eq!(text_of(&h, "f.txt"), "unsaved");
    let f = h.find_buffer("f.txt").unwrap();
    // No fingerprint is what a file that didn't exist has, so `:w` doesn't overwrite blindly.
    assert_eq!(h.file(f).unwrap().fingerprint, None);
}

/// The idle write, on the fake clock: not until a quiet time after the last change, and only
/// once it's resumed.
#[test]
fn the_idle_write_fires_on_the_fake_clock() {
    let dir = Dir::new();
    let file = dir.0.join(FILE);
    let mut h = kept_in(&dir.0);
    h.open_text("t", "start").unwrap();
    h.advance(Duration::from_secs(5));
    h.flush_session();
    assert!(!file.exists(), "paused until the lock is held");

    h.resume_session();
    h.advance(Duration::from_secs(2));
    h.flush_session();
    assert!(fs::read_to_string(&file).unwrap().contains("start"));

    // Changes keep putting it off, and quiet lets it through.
    for i in 0..6 {
        edit(&mut h, "t", 0, &i.to_string());
        h.advance(Duration::from_millis(600));
    }
    h.flush_session();
    let read = || fs::read_to_string(&file).unwrap();
    assert!(!read().contains("543210"), "written during the changes");
    h.advance(Duration::from_millis(1600));
    h.flush_session();
    assert!(read().contains("543210start"));

    // Nothing changed, nothing written: the previous copy stays what it was.
    let previous = fs::read_to_string(dir.0.join(PREVIOUS)).unwrap();
    h.advance(Duration::from_secs(10));
    h.flush_session();
    assert_eq!(fs::read_to_string(dir.0.join(PREVIOUS)).unwrap(), previous);
    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "it holds your unsaved text");

    // Changes that never pause still can't keep what changed off the disk for long.
    h.create_text("notes", "");
    h.create_text("log", "");
    edit(&mut h, "notes", 0, "precious");
    for _ in 0..20 {
        edit(&mut h, "log", 0, ".");
        h.advance(Duration::from_millis(400));
    }
    h.flush_session();
    assert!(read().contains("precious"), "at most about 5 s behind");

    // A handover stops writes until it fails.
    edit(&mut h, "t", 0, "z");
    h.hand_over_session().unwrap();
    assert!(read().contains("z543210start"), "written at once");
    edit(&mut h, "t", 0, "y");
    h.advance(Duration::from_secs(5));
    h.shutdown();
    h.flush_session();
    assert!(!read().contains("yz"), "the new instance owns the file");
}

#[test]
fn quitting_writes_what_changed() {
    let dir = Dir::new();
    let mut h = kept_in(&dir.0);
    h.resume_session();
    h.open_text("t", "typed just before quitting").unwrap();
    h.shutdown();
    let h = kept_in(&dir.0);
    assert_eq!(text_of(&h, "t"), "typed just before quitting");
}

#[test]
fn a_failed_write_is_said_once_and_leaves_the_last_session() {
    let dir = Dir::new();
    let mut h = kept_in(&dir.0);
    h.resume_session();
    h.open_text("t", "saved").unwrap();
    h.save_session().unwrap();
    let good = fs::read_to_string(dir.0.join(FILE)).unwrap();

    // The temporary file can't be opened: a directory is in its way.
    fs::create_dir(dir.0.join("session.tmp")).unwrap();
    edit(&mut h, "t", 0, "more ");
    assert!(h.hand_over_session().is_err(), "a restart mustn't go on");
    h.advance(Duration::from_secs(5));
    h.flush_session();
    h.advance(Duration::from_secs(5));
    h.flush_session();
    assert_eq!(fs::read_to_string(dir.0.join(FILE)).unwrap(), good);
    let lines = h.messages.iter().filter(|m| m.contains("not saved"));
    assert_eq!(lines.count(), 1, "{:?}", h.messages);

    // It tries again by itself.
    fs::remove_dir(dir.0.join("session.tmp")).unwrap();
    h.advance(Duration::from_secs(5));
    h.flush_session();
    let now = fs::read_to_string(dir.0.join(FILE)).unwrap();
    assert!(now.contains("more saved"), "{now}");
}

/// The harness's save and restore (K7): a fresh workspace draws the same screen.
#[test]
fn a_harness_restores_into_a_fresh_workspace() {
    let mut a = fresh();
    a.set_window_size(40, 10);
    let (buf, w) = a.open_text("t", "one\ntwo\nthree\n").unwrap();
    a.text_mut(buf).unwrap().set_cursor(w, 4);
    let mut b = fresh();
    b.set_window_size(40, 10);
    b.restore_session(&a.session_json()).unwrap();
    assert_eq!(b.screen(), a.screen());
    assert_eq!(b.style_dump(), a.style_dump());
    assert!(b.restore_session("not json").is_err());
}

fn numbered(lines: usize) -> String {
    (0..lines).map(|i| format!("line {i}\n")).collect()
}

/// K13's proof: a nested tree with a moved border, two windows on one buffer with their own
/// selections and scroll, a view waiting in a buffer that's out of sight, and registers.
#[test]
fn a_harness_save_and_restore_gives_equal_state() {
    let mut a = fresh();
    a.set_window_size(100, 30);
    let (one, w1) = a.open_text("one", &numbered(200)).unwrap();
    let two = a.create_text("two", &numbered(50));
    let w2 = a.split_window(w1, Side::Right).unwrap();
    let w3 = a.split_window(w2, Side::Below).unwrap();
    a.resize(w1, 30, 28).unwrap();

    let t = a.text_mut(one).unwrap();
    t.set_cursor(w1, 700);
    t.set_scroll(w1, 650, 0);
    let line_sel = Selection {
        anchor: 40,
        head: 90,
        shape: Shape::Line,
        goal: Some(3),
    };
    t.set_selection(w2, line_sel);
    a.focus(w3);
    a.show(two, ShowIn::Current).unwrap();
    a.text_mut(two).unwrap().set_cursor(w3, 33);
    a.show(one, ShowIn::Current).unwrap();
    a.text_mut(one).unwrap().set_cursor(w3, 1000);
    a.focus(w2);
    let kept = Register {
        kind: RegKind::Line,
        text: "kept\n".into(),
    };
    a.set_register('a', kept.clone());
    a.screen(); // a frame, as there's one between keystrokes: scrolling follows the cursor
    let saved = a.session_json();

    let mut b = fresh();
    b.set_window_size(100, 30);
    b.restore_session(&saved).unwrap();
    assert_eq!(b.session_json(), saved);
    assert_eq!(b.screen(), a.screen());
    assert_eq!(b.style_dump(), a.style_dump());
    assert_eq!(b.window_list().len(), 3);
    assert_eq!(b.register('a'), Some(&kept));

    // Each window by what it shows on screen: the focus, and the second's line selection.
    let b2 = b.focused().unwrap();
    let one = b.find_buffer("one").unwrap();
    assert_eq!(b.text(one).unwrap().selection(b2), Some(line_sel));
    // The view waiting in the buffer out of sight is that window's, once it goes back.
    let b3 = b.windows_by_use()[1];
    assert_eq!(b.text(one).unwrap().cursor(b3), Some(1000));
    b.focus(b3);
    let two = b.find_buffer("two").unwrap();
    b.show(two, ShowIn::Current).unwrap();
    assert_eq!(b.text(two).unwrap().cursor(b3), Some(33));
}

/// In a frame of another size the tree scales, as the design it keeps says it should.
#[test]
fn a_tree_comes_back_in_a_frame_of_another_size() {
    let mut a = fresh();
    a.set_window_size(100, 30);
    let (_, w1) = a.open_text("t", "text\n").unwrap();
    let w2 = a.split_window(w1, Side::Right).unwrap();
    a.resize(w2, 25, 28).unwrap();
    let mut b = fresh();
    b.set_window_size(200, 30);
    b.restore_session(&a.session_json()).unwrap();
    let mut cols: Vec<usize> = b
        .window_list()
        .iter()
        .map(|w| b.window_size(*w).unwrap().0)
        .collect();
    cols.sort();
    assert!(cols[1] >= 2 * cols[0] && cols[0] > 25, "{cols:?}");
}

/// A clean file that went has no buffer, so the window on it goes and the others take its cells.
#[test]
fn a_window_on_a_buffer_that_did_not_come_back_goes() {
    let dir = Dir::new();
    let gone = dir.file("gone.txt", "gone\n");
    let mut a = fresh();
    a.set_window_size(100, 30);
    let (_, w1) = a.open_text("stays", "text\n").unwrap();
    let w2 = a.split_window(w1, Side::Right).unwrap();
    let g = a.open_file(&gone).unwrap();
    a.focus(w2);
    a.show(g, ShowIn::Current).unwrap();
    let saved = a.session_json();
    fs::remove_file(&gone).unwrap();

    let mut b = fresh();
    b.set_window_size(100, 30);
    b.restore_session(&saved).unwrap();
    let w = b.focused().unwrap();
    assert_eq!(b.window_list(), [w]);
    assert_eq!(b.window_size(w).unwrap().0, 100);
    assert_eq!(b.window_buffer(w), b.find_buffer("stays"));

    // A | (G over (C | D)), a border moved: the siblings take its cells, as `C-w c` gives them.
    let gone = dir.file("gone.txt", "gone\n");
    let mut a = fresh();
    a.set_window_size(120, 40);
    let (_, left) = a.open_text("stays", "text\n").unwrap();
    let g_win = a.split_window(left, Side::Right).unwrap();
    let c = a.split_window(g_win, Side::Below).unwrap();
    a.split_window(c, Side::Right).unwrap();
    let g = a.open_file(&gone).unwrap();
    a.focus(g_win);
    a.show(g, ShowIn::Current).unwrap();
    a.balance_frame();
    a.resize(left, 30, 38).unwrap();
    let saved = a.session_json();
    fs::remove_file(&gone).unwrap();

    let mut b = fresh();
    b.set_window_size(120, 40);
    b.restore_session(&saved).unwrap();
    let sizes: Vec<_> = b
        .window_list()
        .iter()
        .map(|w| b.window_size(*w).unwrap())
        .collect();
    assert_eq!(sizes, [(30, 40), (44, 40), (44, 40)]);
}

/// `*messages*` isn't saved, but it's always there, so a window on it keeps its place.
#[test]
fn a_window_on_the_messages_comes_back() {
    let mut a = fresh();
    a.set_window_size(100, 30);
    let (_, w1) = a.open_text("stays", "text\n").unwrap();
    let w2 = a.split_window(w1, Side::Right).unwrap();
    a.messages.push("something to log".into());
    a.scene(); // the log reaches its buffer
    let log = a.find_buffer("*messages*").unwrap();
    a.focus(w2);
    a.show(log, ShowIn::Current).unwrap();
    let saved = a.session_json();
    assert!(!saved.contains("something to log"));

    let mut b = fresh();
    b.set_window_size(100, 30);
    b.restore_session(&saved).unwrap();
    assert_eq!(b.window_list().len(), 2);
    assert!(b.focused_buffer().is_some());
    assert_eq!(b.focused_buffer(), b.find_buffer("*messages*"));
    assert_eq!(b.session_json(), saved);
}

/// `*grep*` isn't saved, nor is its window, as no search stands behind it after a restart.
#[test]
fn a_window_on_grep_results_goes() {
    let mut a = fresh();
    a.set_window_size(100, 30);
    a.open_text("stays", "text\n").unwrap();
    let dir = Dir::new();
    dir.file("f", "needle\n");
    let args = die_core::Args {
        text: Some(format!("needle {}", dir.0.display())),
        ..Default::default()
    };
    a.run("grep", &args).unwrap();
    a.run_until_idle();
    assert_eq!(a.window_list().len(), 2);
    let saved = a.session_json();
    assert!(!saved.contains("f:1:1"));

    let mut b = fresh();
    b.set_window_size(100, 30);
    b.restore_session(&saved).unwrap();
    assert_eq!(b.window_list().len(), 1);
    assert_eq!(b.buffer_name(b.focused_buffer().unwrap()), Some("stays"));
    assert_eq!(b.find_buffer("*grep*"), None);
}

/// A tree that can't be one costs the windows and is kept aside. The buffers still come back.
#[test]
fn an_unreadable_tree_is_dropped_alone_and_kept_aside() {
    let kid = |w: usize| format!(r#"{{"size":1,"node":{{"window":{w}}}}}"#);
    for (layout, windows) in [
        (r#""junk""#.to_owned(), r#"[{"buffer":0}]"#),
        (r#"{"window":0}"#.to_owned(), r#""junk""#),
        (r#"{"window":7}"#.to_owned(), r#"[{"buffer":0}]"#),
        (
            format!(r#"{{"axis":"Cols","kids":[{},{}]}}"#, kid(0), kid(0)),
            r#"[{"buffer":0},{"buffer":0}]"#,
        ),
    ] {
        let dir = Dir::new();
        let session = format!(
            r#"{{"version":1,"focus":0,"layout":{layout},"windows":{windows},
                "buffers":[{{"name":"n","text":"kept","modified":true}}]}}"#
        );
        fs::write(dir.0.join(FILE), session).unwrap();
        let h = kept_in(&dir.0);
        assert!(
            said(&h, "dropped the windows"),
            "{layout}: {:?}",
            h.messages
        );
        assert_eq!(h.window_list().len(), 1, "{layout}");
        assert_eq!(h.focused_buffer(), h.find_buffer("n"), "{layout}");
        let names: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let rejected = names.iter().any(|n| n.starts_with("session.rejected-"));
        assert!(rejected, "{layout}: {names:?}");
    }
}

/// Build N reading build N+1's view or register: only that one is lost.
#[test]
fn an_unreadable_view_or_register_is_dropped_alone() {
    let mut h = fresh();
    h.restore_session(
        r#"{"version":1,"focus":0,"layout":{"window":0},"windows":[{"buffer":0}],
            "registers":[{"name":"a","kind":"Hologram","text":"x"},
                         {"name":"b","kind":"Char","text":"bee"}],
            "buffers":[{"name":"n","text":"some text",
              "views":[{"window":0,"anchor":"elsewhere"},
                       {"window":0,"anchor":2,"head":4,"shape":"Char","top":0,"top_row":0}]}]}"#,
    )
    .unwrap();
    assert_eq!(h.register('a'), None);
    assert_eq!(h.register('b').unwrap().text, "bee");
    assert!(said(&h, "dropped a register"));
    let buf = h.find_buffer("n").unwrap();
    let sel = h
        .text(buf)
        .unwrap()
        .selection(h.focused().unwrap())
        .unwrap();
    assert_eq!((sel.anchor, sel.head), (2, 4));
}

/// A field of the wrong shape costs that field: `views` nothing else and silently, as an unreadable
/// view does, and `registers` with a message and the file kept aside.
#[test]
fn a_field_of_the_wrong_shape_costs_only_that_field() {
    let dir = Dir::new();
    let session = r#"{"version":1,"focus":0,"registers":"junk","buffers":[{"name":"n",
        "text":"unsaved","modified":true,"views":{"from":"a newer build"}}]}"#;
    fs::write(dir.0.join(FILE), session).unwrap();
    let h = kept_in(&dir.0);
    assert_eq!(text_of(&h, "n"), "unsaved");
    assert!(modified(&h, "n"));
    assert!(said(&h, "dropped the registers"), "{:?}", h.messages);
    let kept = fs::read_dir(&dir.0).unwrap().any(|e| {
        let name = e.unwrap().file_name();
        name.to_string_lossy().starts_with("session.rejected-")
    });
    assert!(kept);
}

/// The newest kill was on the clipboard when DIE stopped, so it isn't news after a restart.
#[test]
fn the_newest_kill_still_on_the_clipboard_is_not_news() {
    let mut a = fresh();
    a.open_text("t", "text").unwrap();
    let copied = Register {
        kind: RegKind::Char,
        text: "copied".into(),
    };
    a.set_register('"', copied);
    let mut b = fresh();
    b.fake_clipboard().copy_from_outside("copied");
    b.restore_session(&a.session_json()).unwrap();
    assert_eq!(b.clipboard_news(), None);
    b.fake_clipboard().copy_from_outside("from another app");
    assert_eq!(b.clipboard_news().unwrap().text, "from another app");
}

/// A session with nothing to write doesn't keep the loop awake, and a change still reaches disk.
#[test]
fn looks_rest_when_nothing_changes_and_a_change_wakes_them() {
    let dir = Dir::new();
    let file = dir.0.join(FILE);
    let mut h = kept_in(&dir.0);
    h.resume_session();
    h.advance(Duration::from_secs(40));
    assert!(h.next_timer().unwrap() - h.now() > Duration::from_secs(1));
    h.flush_session();
    let before = fs::read_to_string(&file).unwrap();
    h.open_text("notes", "kept").unwrap();
    h.handle(Input::Focus(true));
    h.advance(Duration::from_secs(3));
    h.flush_session();
    assert_ne!(fs::read_to_string(&file).unwrap(), before);
}

/// A timer that keeps firing isn't you using the editor, as an extension's poll isn't: the looks
/// rest as they would without it, and what a timer changes is still written.
#[test]
fn an_extensions_repeating_timer_doesnt_keep_the_looks_awake() {
    let dir = Dir::new();
    let file = dir.0.join(FILE);
    let mut h = kept_in(&dir.0);
    let (buf, _) = h.open_text("notes", "").unwrap();
    h.resume_session();
    let _poll = h.every(Duration::from_secs(2), |_| Ok(()));
    let wakes = |h: &mut Harness, over: Duration| {
        let end = h.now() + over;
        let mut n = 0;
        while let Some(at) = h.next_timer().filter(|at| *at <= end) {
            h.advance(at.saturating_sub(h.now()).max(Duration::from_millis(1)));
            n += 1;
        }
        h.advance(end.saturating_sub(h.now()));
        n
    };
    wakes(&mut h, Duration::from_secs(40));
    let n = wakes(&mut h, Duration::from_secs(60));
    assert!(
        n <= 30 + 12,
        "the timer's 30 and the resting looks' 12, not {n}"
    );
    let _edit = h.after(Duration::from_secs(1), move |ctx| {
        let t = ctx.text_mut(buf)?;
        t.tx(Origin::Extension).insert(0, "a timer's").commit()?;
        Ok(())
    });
    h.advance(Duration::from_secs(8));
    h.flush_session();
    assert!(fs::read_to_string(&file).unwrap().contains("a timer's"));
}

/// What K13 adds to the file counts as a change, or the idle write wouldn't happen for it.
#[test]
fn a_change_beside_the_text_is_written() {
    let dir = Dir::new();
    let file = dir.0.join(FILE);
    let mut h = kept_in(&dir.0);
    let (buf, w) = h.open_text("t", &numbered(100)).unwrap();
    h.resume_session();
    let wrote = |h: &mut Harness, what: &str| {
        h.advance(Duration::from_secs(5));
        h.flush_session();
        let now = fs::read_to_string(&file).unwrap();
        assert_eq!(now, h.session_json(), "{what}");
        now
    };
    let mut last = wrote(&mut h, "the start");
    type Change = fn(&mut Harness, die_core::BufferId, die_core::WindowId);
    let changes: [(&str, Change); 3] = [
        ("a register", |h, _, _| {
            let text = "reg".to_owned();
            h.set_register(
                'z',
                Register {
                    kind: RegKind::Char,
                    text,
                },
            );
        }),
        ("a selection", |h, buf, w| {
            let sel = Selection {
                anchor: 3,
                head: 9,
                shape: Shape::Char,
                goal: None,
            };
            h.text_mut(buf).unwrap().set_selection(w, sel);
        }),
        ("a split", |h, _, w| {
            h.split_window(w, Side::Right).unwrap();
        }),
    ];
    for (what, change) in changes {
        change(&mut h, buf, w);
        let now = wrote(&mut h, what);
        assert_ne!(now, last, "{what}");
        last = now;
    }
}

/// With the focus on a buffer the session doesn't keep, the buffer used last goes on screen,
/// rather than everything coming back hidden.
#[test]
fn with_the_focus_on_what_is_not_saved_the_last_used_buffer_shows() {
    let mut a = fresh();
    a.open_text("older", "old\n").unwrap();
    a.open_text("notes", "unsaved\n").unwrap();
    let log = a.create_text("*messages*", "");
    a.show(log, ShowIn::Current).unwrap();

    let mut b = fresh();
    b.restore_session(&a.session_json()).unwrap();
    assert_eq!(b.focused_buffer(), b.find_buffer("notes"));
}

/// Makes `*log*` at init, as an extension's own buffer.
struct Logs;

impl Extension for Logs {
    fn name(&self) -> &'static str {
        "logs"
    }

    fn register(&mut self, _reg: &mut Registry) {}

    fn init(&mut self, ctx: &mut Ctx) -> die_core::Result {
        ctx.create_text("*log*", "boot\n");
        Ok(())
    }
}

/// An extension's own buffer takes the saved text, however it differs, and stays the one it has.
#[test]
fn an_extensions_buffer_takes_its_saved_text() {
    let logs = || Harness::new(vec![Box::new(Logs)]).unwrap();
    let mut a = logs();
    edit(&mut a, "*log*", 0, "ran\n");
    let mut b = logs();
    let own = b.find_buffer("*log*").unwrap();
    b.restore_session(&a.session_json()).unwrap();
    assert_eq!(b.find_buffer("*log*"), Some(own));
    assert_eq!(text_of(&b, "*log*"), "ran\nboot\n");
    assert_eq!(b.find_buffer("*log*<2>"), None);
    assert!(modified(&b, "*log*"));
}
