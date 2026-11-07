//! U2: the walker's skips, and the finder prompt on a temp tree and on half a million paths.

use die_test_cap as _;

use die_core::*;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// A fresh directory for one test, removed when dropped. It's canonical, as a buffer's is.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("die-project-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Dir(p.canonicalize().unwrap())
    }
    fn files(&self, names: &[&str]) {
        for name in names {
            let p = self.0.join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, name).unwrap();
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn harness() -> Harness {
    let mut ctx = Harness::new(vec![]).unwrap();
    let b = ctx.create_text("t", "");
    let w = ctx.open_window(b, 80, 24).unwrap();
    ctx.focus(w);
    ctx
}

/// Take turns until the condition holds, as a host does when the finder wakes it.
fn until(ctx: &mut Ctx, what: &str, f: impl Fn(&Ctx) -> bool) {
    let start = Instant::now();
    while !f(ctx) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "waiting for {what}, with {:?}",
            ctx.prompt_candidates()
        );
        if !ctx.step() {
            sleep(Duration::from_millis(1));
        }
    }
}

fn walked(root: &Path) -> Vec<String> {
    let mut all = vec![];
    walk(root, &AtomicBool::new(false), &mut |b| all.extend(b));
    all
}

/// A source the test feeds by hand, which says when it was stopped.
fn by_hand() -> (mpsc::Sender<Vec<String>>, Arc<AtomicBool>, PathSource) {
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    let stopped = Arc::new(AtomicBool::new(false));
    let saw = stopped.clone();
    let source: PathSource = Box::new(move |stop, sink| {
        while !stop.load(Ordering::Relaxed) {
            if let Ok(batch) = rx.recv_timeout(Duration::from_millis(1)) {
                sink(batch);
            }
        }
        saw.store(true, Ordering::SeqCst);
    });
    (tx, stopped, source)
}

fn paths(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

fn key(ctx: &mut Ctx, key: &str) {
    ctx.handle(Input::Key(parse_keys(key).unwrap()[0].clone()));
}

#[test]
fn the_walk_skips_what_it_should_and_finds_hidden_files() {
    let d = Dir::new("skips");
    d.files(&[
        "a.rs",
        ".hidden",
        ".config/deep/init.el",
        "src/b.rs",
        "target/debug/x",
        "sub/target/y",
        ".git/config",
        "src/.git",
        "node_modules/m.js",
        ".Trash/t",
        "Library/l",
        ".cache/c",
        ".cargo/registry/r",
        ".cargo/config.toml",
        ".rustup/u",
        "registry",
    ]);
    let elsewhere = Dir::new("skips-elsewhere");
    elsewhere.files(&["through-link", "to-file"]);
    symlink(&elsewhere.0, d.0.join("link-dir")).unwrap();
    symlink(elsewhere.0.join("to-file"), d.0.join("link-file")).unwrap();
    symlink(d.0.join("gone"), d.0.join("broken")).unwrap();
    assert_eq!(
        walked(&d.0),
        [
            ".hidden",
            "a.rs",
            "link-file",
            "registry",
            ".cargo/config.toml",
            "src/b.rs",
            ".config/deep/init.el",
        ],
        "shallowest first, by name within a directory"
    );
}

#[test]
fn the_project_root_is_the_nearest_with_a_git() {
    let d = Dir::new("root");
    d.files(&[
        "outer/.git/config",
        "outer/sub/.git",
        "outer/sub/a/b/c",
        "bare/d/e",
    ]);
    let sub = d.0.join("outer/sub");
    assert_eq!(project_root(&sub.join("a/b")), sub, "a .git file counts");
    assert_eq!(project_root(&d.0.join("outer")), d.0.join("outer"));
    let bare = d.0.join("bare/d");
    assert_eq!(project_root(&bare), bare, "nothing above a temp directory");
}

#[test]
fn the_project_command_walks_from_the_root_and_cr_opens_the_file() {
    let d = Dir::new("project");
    d.files(&[
        "p/.git/config",
        "p/crates/core/src/main.rs",
        "p/README",
        "other/not-this",
    ]);
    let mut ctx = harness();
    let main = d.0.join("p/crates/core/src/main.rs");
    let file = ctx.open_file(&main).unwrap();
    ctx.show(file, ShowIn::Current).unwrap();
    ctx.run("find-file-project", &Args::default()).unwrap();
    until(&mut ctx, "the walk", |c| c.prompt_candidates().len() == 2);
    assert_eq!(
        ctx.prompt_candidates(),
        ["README", "crates/core/src/main.rs"]
    );
    let _ = ctx.type_keys("readme");
    until(&mut ctx, "the match", |c| {
        c.prompt_candidates() == ["README"]
    });
    let _ = ctx.type_keys("<CR>");
    assert_eq!(ctx.prompt_text(), None);
    let here = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.file(here).unwrap().path, d.0.join("p/README"));
}

/// `HOME` is the process's, so this runs in a process of its own.
#[test]
fn the_home_command_walks_from_home() {
    const CHILD: &str = "DIE_TEST_FINDER_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let d = Dir::new("home");
        d.files(&[".emacs.d/init.el", "Library/no"]);
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "the_home_command_walks_from_home"])
            .env(CHILD, "1")
            .env("HOME", &d.0)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let mut ctx = harness();
    ctx.run("find-file-home", &Args::default()).unwrap();
    until(&mut ctx, "the walk", |c| !c.prompt_candidates().is_empty());
    assert_eq!(ctx.prompt_candidates(), [".emacs.d/init.el"]);
    let _ = ctx.type_keys("<Esc>");
}

#[test]
fn batches_arrive_while_typing_and_a_moved_row_stays_chosen() {
    let mut ctx = harness();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = wakes.clone();
    ctx.set_waker(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    let (tx, _, source) = by_hand();
    ctx.find_under("/nowhere".into(), source);
    tx.send(paths(&["one/alpha", "two/beta"])).unwrap();
    let start = Instant::now();
    while wakes.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(20), "no wake");
        sleep(Duration::from_millis(1));
    }
    assert!(ctx.step(), "a sleeping host is woken, and its turn is news");
    until(&mut ctx, "the first", |c| c.prompt_candidates().len() == 2);
    let _ = ctx.type_keys("al");
    until(&mut ctx, "the filter", |c| {
        c.prompt_candidates() == ["one/alpha"]
    });
    tx.send(paths(&["three/gamma", "four/al"])).unwrap();
    until(&mut ctx, "the second batch", |c| {
        c.prompt_candidates() == ["four/al", "one/alpha"]
    });
    assert_eq!(ctx.prompt_chosen(), Some("four/al"), "the best row follows");
    let _ = ctx.type_keys("<Down>");
    assert_eq!(ctx.prompt_chosen(), Some("one/alpha"));
    tx.send(paths(&["al"])).unwrap();
    until(&mut ctx, "the third batch", |c| {
        c.prompt_candidates().len() == 3
    });
    assert_eq!(ctx.prompt_chosen(), Some("one/alpha"), "a moved row stays");
    let _ = ctx.type_keys("<Esc>");
}

#[test]
fn cr_right_after_typing_takes_the_match_for_what_is_typed() {
    let d = Dir::new("settle");
    d.files(&["aaa", "bbb"]);
    let mut ctx = harness();
    let from = d.0.clone();
    let source = move |stop: &AtomicBool, sink: &mut dyn FnMut(Vec<String>)| {
        walk(&from, stop, sink);
    };
    ctx.find_under(d.0.clone(), Box::new(source));
    until(&mut ctx, "the walk", |c| c.prompt_candidates().len() == 2);
    // `<CR>` comes before any turn can take the list for `b` from the board.
    key(&mut ctx, "b");
    key(&mut ctx, "<CR>");
    ctx.run_until_idle();
    assert_eq!(ctx.prompt_text(), None, "{:?}", ctx.messages);
    let here = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.file(here).unwrap().path, d.0.join("bbb"));
}

#[test]
fn esc_stops_the_walk_opens_nothing_and_no_match_stays_on_the_line() {
    let mut ctx = harness();
    let before = ctx.focused_buffer();
    let (tx, stopped, source) = by_hand();
    ctx.find_under("/nowhere".into(), source);
    tx.send(paths(&["a"])).unwrap();
    until(&mut ctx, "the batch", |c| c.prompt_candidates().len() == 1);
    let _ = ctx.type_keys("zzz<CR>");
    assert_eq!(ctx.prompt_text(), Some("zzz"), "there's nothing to open");
    assert!(!stopped.load(Ordering::SeqCst));
    let _ = ctx.type_keys("<Esc>");
    assert_eq!(ctx.prompt_text(), None);
    assert_eq!(ctx.focused_buffer(), before);
    let start = Instant::now();
    while !stopped.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(5), "it never stopped");
        sleep(Duration::from_millis(1));
    }
}

/// 500,000 paths: what a key costs the main thread, and how long until the list answers it. Run
/// it with `--release` for a number to quote.
#[test]
#[ignore = "speed: runs in ci --full"]
fn half_a_million_paths_cost_a_key_under_a_frame() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "");
    let w = ctx.open_window(b, 80, 24).unwrap();
    ctx.focus(w);
    let all: Vec<String> = (0..500_000)
        .map(|i| {
            let (p, m) = (i % 97, i % 1009);
            format!("workspace/project-{p}/crates/module-{m}/src/deep/file_{i}.rs")
        })
        .collect();
    let (tx, _, source) = by_hand();
    ctx.find_under("/nowhere".into(), source);
    tx.send(all).unwrap();
    until(&mut ctx, "the paths", |c| {
        c.prompt_candidates().len() == 1000
    });
    let placed = [WindowPlacement {
        window: w,
        col: 0,
        row: 0,
    }];
    let (mut keys, mut news, mut answers) = (vec![], vec![], vec![]);
    let typed = [
        "m", "o", "d", "1", "2", "f", "9", "<BS>", "<BS>", "<BS>", "<BS>", "<BS>", "<BS>", "<BS>",
    ];
    for round in 0..10 {
        for k in typed {
            let start = Instant::now();
            key(&mut ctx, k);
            while ctx.step() {}
            ctx.compose(80, 26, &placed);
            keys.push(start.elapsed());
            // Half the rounds time the answer. The others let it arrive and time what a host's
            // turn does with it: taking it from the board, and the frame.
            if round % 2 == 0 {
                let start = Instant::now();
                let _ = ctx.prompt_settle();
                answers.push(start.elapsed());
                ctx.compose(80, 26, &placed);
            } else {
                sleep(Duration::from_millis(100));
                let start = Instant::now();
                while ctx.step() {}
                ctx.compose(80, 26, &placed);
                news.push(start.elapsed());
            }
        }
    }
    for v in [&mut keys, &mut news, &mut answers] {
        v.sort();
    }
    let show = |name: &str, v: &[Duration]| {
        eprintln!(
            "{name}: p50 {:?}, p99 {:?}, max {:?}",
            v[v.len() / 2],
            v[v.len() * 99 / 100],
            v[v.len() - 1]
        );
    };
    show("a key and its frame on the main thread", &keys);
    show("the ranker's answer after that", &answers);
    show("the turn and frame that show it", &news);
    ctx.type_keys("<Esc>");
    for v in [&keys, &news] {
        assert!(v[v.len() * 99 / 100] < Duration::from_millis(16));
    }
}
