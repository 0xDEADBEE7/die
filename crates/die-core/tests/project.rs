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

// ---------------------------------------------------------------- grep

fn grep_args(line: &str) -> Args {
    Args {
        text: Some(line.to_string()),
        ..Args::default()
    }
}

/// Search and wait for the search to end.
fn grep(ctx: &mut Harness, line: &str) {
    ctx.run("grep", &grep_args(line)).unwrap();
    ctx.run_until_idle();
}

fn results(ctx: &Ctx) -> String {
    let buf = ctx.find_buffer("*grep*").expect("*grep*");
    ctx.text(buf).unwrap().rope().to_string()
}

fn written(d: &Dir, files: &[(&str, &[u8])]) {
    for (name, bytes) in files {
        let p = d.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
    }
}

/// Where the focused window is, as its file relative to `d`, a line and a column, all from 1.
fn at(ctx: &Ctx, d: &Dir) -> (String, usize, usize) {
    let (w, b) = (ctx.focused().unwrap(), ctx.focused_buffer().unwrap());
    let rel = ctx.file(b).unwrap().path.strip_prefix(&d.0).unwrap();
    let t = ctx.text(b).unwrap();
    let pos = t.cursor(w).unwrap();
    let line = t.line_of(pos);
    let col = t.rope().byte_slice(t.line_start(line)..pos).chars().count();
    (rel.display().to_string(), line + 1, col + 1)
}

#[test]
fn grep_writes_rows_and_skips_binary_big_and_ignored_files() {
    let d = Dir::new("grep-rows");
    written(
        &d,
        &[
            (
                "a.rs",
                "one\n  a Needle needle\nnone\nneedle\r\n".as_bytes(),
            ),
            ("sub/é.rs", "éé needle".as_bytes()),
            ("target/skip", b"needle\n"),
            ("binary", b"needle\0\n"),
            ("empty", b""),
        ],
    );
    let big = fs::File::create(d.0.join("big")).unwrap();
    big.set_len(10 * 1024 * 1024 + 1).unwrap();
    fs::write(d.0.join("long"), format!("{}needle", "x".repeat(600))).unwrap();
    let mut ctx = harness();
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("needle {root}"));
    let long = format!("long:1:601: {} ...", "x".repeat(500));
    assert_eq!(
        results(&ctx),
        format!(
            "Grep for needle in {root}\n\
             a.rs:2:5:   a Needle needle\n\
             a.rs:4:1: needle\n\
             {long}\n\
             sub/é.rs:1:4: éé needle\n\
             4 matches in 3 files.\n"
        ),
        "smartcase, one row a line, and columns in characters"
    );
    grep(&mut ctx, &format!("Needle {root}"));
    assert!(results(&ctx).ends_with(":5:   a Needle needle\n1 match in 1 file.\n"));
    grep(&mut ctx, &format!("^ {root}"));
    assert!(results(&ctx).contains("a.rs:4:1: "));
    assert!(!results(&ctx).contains("a.rs:5:"), "no line after the last");
    assert!(
        !results(&ctx).contains("empty:"),
        "and none in an empty file"
    );
    grep(&mut ctx, &format!("no such text {root}"));
    assert!(results(&ctx).ends_with("No matches.\n"));
    let grep_buf = ctx.find_buffer("*grep*").unwrap();
    assert!(ctx.text(grep_buf).unwrap().settings().read_only);
    assert!(!ctx.text(grep_buf).unwrap().modified());
    assert!(ctx.run("grep", &grep_args("(")).is_err(), "a bad pattern");
    assert!(results(&ctx).ends_with("No matches.\n"), "changes nothing");
}

#[test]
fn grep_with_no_directory_searches_the_project_and_a_last_word_can_be_the_directory() {
    let d = Dir::new("grep-root");
    written(
        &d,
        &[
            ("p/.git/config", b"x\n"),
            ("p/src/here.rs", b"two words\n"),
            ("p/lib/there.rs", b"two lib\ntwo\n"),
            ("outside", b"two words\n"),
        ],
    );
    let mut ctx = harness();
    let file = ctx.open_file(&d.0.join("p/src/here.rs")).unwrap();
    ctx.show(file, ShowIn::Current).unwrap();
    grep(&mut ctx, "two words");
    let root = d.0.join("p");
    assert_eq!(
        results(&ctx),
        format!(
            "Grep for two words in {}\nsrc/here.rs:1:1: two words\n1 match in 1 file.\n",
            root.display()
        )
    );
    grep(&mut ctx, "two lib");
    assert!(
        results(&ctx).contains("Grep for two in "),
        "`lib` is a directory from the project's root"
    );
    assert!(results(&ctx).contains("there.rs:2:1: two\n"));
    ctx.run("grep", &Args::default()).unwrap();
    assert_eq!(ctx.prompt_text(), Some(""), "with no pattern, it asks");
    let _ = ctx.type_keys("words<CR>");
    ctx.run_until_idle();
    assert!(results(&ctx).contains("src/here.rs:1:5:"));
    assert_eq!(ctx.focused_buffer(), Some(file), "the focus stays");
}

#[test]
fn cr_opens_in_another_window_and_splits_when_there_is_only_one() {
    let d = Dir::new("grep-cr");
    written(&d, &[("a.txt", b"zero\none\n  needle\n")]);
    let mut ctx = harness();
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("needle {root}"));
    assert_eq!(ctx.window_list().len(), 2, "the results get a window");
    let source = ctx.focused().unwrap();
    let list = ctx.find_buffer("*grep*").unwrap();
    let results_window = ctx
        .window_list()
        .into_iter()
        .find(|w| ctx.window_buffer(*w) == Some(list))
        .unwrap();
    ctx.focus(results_window);
    let header = ctx.type_keys("<CR>").unwrap();
    assert!(
        header[0].errors.iter().any(|e| e.contains("No match here")),
        "{header:?}"
    );
    let row = ctx.text(list).unwrap().line_start(1);
    ctx.text_mut(list).unwrap().set_cursor(results_window, row);
    let _ = ctx.type_keys("<CR>");
    assert_eq!(ctx.focused(), Some(source), "the other window takes it");
    assert_eq!(at(&ctx, &d), ("a.txt".to_string(), 3, 3));
    assert_eq!(ctx.window_list().len(), 2);

    ctx.only_window(results_window).unwrap();
    ctx.focus(results_window);
    ctx.text_mut(list).unwrap().set_cursor(results_window, row);
    let _ = ctx.type_keys("<CR>");
    assert_eq!(ctx.window_list().len(), 2, "alone, it splits");
    assert_eq!(at(&ctx, &d), ("a.txt".to_string(), 3, 3));
    assert_eq!(ctx.window_buffer(results_window), Some(list));
}

#[test]
fn next_and_previous_step_from_any_window_and_say_so_at_either_end() {
    let d = Dir::new("grep-step");
    written(&d, &[("a.txt", b"hit\nmiss\nhit\n"), ("b.txt", b"x hit\n")]);
    let mut ctx = harness();
    let step = |ctx: &mut Harness, name: &str| ctx.run(name, &Args::default());
    assert!(step(&mut ctx, "next-error").is_err(), "no results yet");
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("hit {root}"));
    let moved_back = step(&mut ctx, "previous-error").unwrap_err().to_string();
    assert!(moved_back.contains("before first"), "{moved_back}");
    step(&mut ctx, "next-error").unwrap();
    assert_eq!(at(&ctx, &d), ("a.txt".to_string(), 1, 1));
    let scratch = ctx.create_text("elsewhere", "");
    ctx.show(scratch, ShowIn::Current).unwrap();
    step(&mut ctx, "next-error").unwrap();
    assert_eq!(at(&ctx, &d), ("a.txt".to_string(), 3, 1));
    step(&mut ctx, "next-error").unwrap();
    assert_eq!(at(&ctx, &d), ("b.txt".to_string(), 1, 3));
    let past = step(&mut ctx, "next-error").unwrap_err().to_string();
    assert!(past.contains("past last"), "{past}");
    assert_eq!(at(&ctx, &d), ("b.txt".to_string(), 1, 3), "it stays");
    step(&mut ctx, "previous-error").unwrap();
    assert_eq!(at(&ctx, &d), ("a.txt".to_string(), 3, 1));
    let list = ctx.find_buffer("*grep*").unwrap();
    let mut shown = ctx.window_list().into_iter();
    let w = shown.find(|w| ctx.window_buffer(*w) == Some(list));
    let t = ctx.text(list).unwrap();
    let row = t.line_of(t.cursor(w.unwrap()).unwrap());
    assert_eq!(row, 2, "the row follows");
}

#[test]
fn the_walk_stays_out_of_other_checkouts_in_claude_worktrees() {
    let d = Dir::new("worktrees");
    d.files(&[
        ".claude/settings.json",
        ".claude/worktrees/wf/src/a.rs",
        "src/worktrees/b.rs",
    ]);
    assert_eq!(
        walked(&d.0),
        [".claude/settings.json", "src/worktrees/b.rs"]
    );
}

#[test]
fn cr_goes_to_the_window_that_has_the_file() {
    let d = Dir::new("grep-has-file");
    written(&d, &[("a.txt", b"hit\n"), ("b.txt", b"hit\n")]);
    let mut ctx = harness();
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("hit {root}"));
    let list = ctx.find_buffer("*grep*").unwrap();
    let results_window = ctx
        .window_list()
        .into_iter()
        .find(|w| ctx.window_buffer(*w) == Some(list))
        .unwrap();
    let a = ctx.open_file(&d.0.join("a.txt")).unwrap();
    let b = ctx.open_file(&d.0.join("b.txt")).unwrap();
    ctx.show(a, ShowIn::Current).unwrap();
    let on_a = ctx.focused().unwrap();
    let on_b = ctx.show(b, ShowIn::Split(Side::Left)).unwrap().unwrap();
    ctx.focus(on_a);
    ctx.focus(results_window);
    let row = ctx.text(list).unwrap().line_start(2);
    ctx.text_mut(list).unwrap().set_cursor(results_window, row);
    let _ = ctx.type_keys("<CR>");
    assert_eq!(ctx.focused(), Some(on_b), "it has b.txt, the row's file");
    assert_eq!(ctx.window_buffer(on_a), Some(a), "this one was used last");
}

#[test]
fn a_result_whose_file_is_gone_says_so_and_makes_no_buffer() {
    let d = Dir::new("grep-gone");
    written(&d, &[("a.txt", b"hit\n"), ("b.txt", b"hit\n")]);
    let mut ctx = harness();
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("hit {root}"));
    fs::remove_file(d.0.join("a.txt")).unwrap();
    let here = ctx.focused_buffer();
    let gone = ctx.run("next-error", &Args::default()).unwrap_err();
    assert!(gone.to_string().contains("a.txt"), "{gone}");
    assert_eq!(ctx.focused_buffer(), here);
    assert_eq!(ctx.find_buffer("a.txt"), None);
    ctx.run("next-error", &Args::default()).unwrap();
    assert_eq!(at(&ctx, &d), ("b.txt".to_string(), 1, 1), "on past it");
}

#[test]
fn a_last_line_with_no_newline_has_nothing_after_it_to_match() {
    let d = Dir::new("grep-last-line");
    written(&d, &[("a.txt", b"a\nab")]);
    let mut ctx = harness();
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("a|$ {root}"));
    let found = results(&ctx);
    let rows: Vec<_> = found.lines().skip(1).collect();
    assert_eq!(
        rows,
        ["a.txt:1:1: a", "a.txt:2:1: ab", "2 matches in 1 file."]
    );
}

#[test]
fn rows_show_while_the_search_goes_on_through_files_with_none() {
    let d = Dir::new("grep-lull");
    written(&d, &[("a.txt", b"needle\n"), ("b.txt", b"needle\n")]);
    let seed = d.0.join("seed");
    fs::write(&seed, "nothing here\n".repeat(3000)).unwrap();
    for i in 0..20_000 {
        fs::hard_link(&seed, d.0.join(format!("z{i:05}"))).unwrap();
    }
    let mut ctx = harness();
    let root = d.0.display().to_string();
    ctx.run("grep", &grep_args(&format!("needle {root}")))
        .unwrap();
    until(&mut ctx, "the second row", |ctx| {
        results(ctx).contains("b.txt")
    });
    ctx.step();
    ctx.step();
    assert!(
        !results(&ctx).contains("2 matches"),
        "the row waited for the search to end"
    );
}

#[test]
fn results_stop_at_the_limit_and_a_new_search_replaces_the_old() {
    let d = Dir::new("grep-limit");
    written(&d, &[("many", "hit\n".repeat(10_001).as_bytes())]);
    let mut ctx = harness();
    let root = d.0.display().to_string();
    grep(&mut ctx, &format!("hit {root}"));
    let text = results(&ctx);
    assert!(text.ends_with("many:10000:1: hit\nStopped at 10000 matches, in 1 file.\n"));

    // The first search is still walking when the second starts.
    let far = Dir::new("grep-cancel");
    written(&far, &[("seed", "old\n".repeat(50).as_bytes())]);
    for i in 0..500 {
        fs::hard_link(far.0.join("seed"), far.0.join(format!("f{i}"))).unwrap();
    }
    let old = grep_args(&format!("old {}", far.0.display()));
    ctx.run("grep", &old).unwrap();
    grep(&mut ctx, &format!("hit {root}"));
    let text = results(&ctx);
    assert!(text.starts_with("Grep for hit in"), "{}", &text[..40]);
    assert!(!text.contains("seed:"), "nothing of the first search");
    assert_eq!(text.matches("Stopped at").count(), 1, "one last line");

    ctx.run("grep", &old).unwrap();
    let list = ctx.find_buffer("*grep*").unwrap();
    assert!(ctx.kill(list), "killed mid-run");
    ctx.run_until_idle();
    assert!(ctx.find_buffer("*grep*").is_none(), "it doesn't come back");
}

#[test]
fn a_big_search_doesnt_hold_up_typing_and_its_rows_arrive_as_it_goes() {
    let d = Dir::new("grep-typing");
    let seed = "fn needle() {}\n".to_string() + &"filler line of text\n".repeat(2000);
    written(&d, &[("seed", seed.as_bytes())]);
    for i in 0..4000 {
        fs::hard_link(d.0.join("seed"), d.0.join(format!("f{i:04}"))).unwrap();
    }
    let mut ctx = harness();
    let line = format!("needle {}", d.0.display());
    ctx.run("grep", &grep_args(&line)).unwrap();
    ctx.prompt(PromptSpec::new("typing: "), |_, _| Ok(()));
    let done = |c: &Ctx| results(c).contains("matches in");
    let list = ctx.find_buffer("*grep*").unwrap();
    let (mut typed, mut slowest, mut partial) = (String::new(), Duration::ZERO, false);
    while !done(&ctx) {
        let start = Instant::now();
        key(&mut ctx, "x");
        while ctx.step() {}
        slowest = slowest.max(start.elapsed());
        typed.push('x');
        assert_eq!(ctx.prompt_text(), Some(typed.as_str()), "a key is answered");
        let rows = ctx.text(list).unwrap().line_count();
        partial |= rows > 2 && !done(&ctx);
        sleep(Duration::from_millis(2));
    }
    assert!(partial, "rows came in before the search ended");
    assert!(typed.len() > 3, "the search outlasts three keys");
    assert!(
        slowest < Duration::from_millis(100),
        "a key took {slowest:?}"
    );
    let _ = ctx.type_keys("<Esc>");
}
