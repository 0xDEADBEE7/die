//! F4a and K12: DIE.app, launched as you'd launch it, with `open -g` so that it takes no focus.
//! Each test uses a bundle of its own under a test bundle id and a state directory of its
//! own, so your DIE.app and its lock aren't touched, and quits what it started. They bundle
//! the dev build, which `cargo test` mostly has already; `cargo xtask app` bundles release.
#![cfg(target_os = "macos")]

use die_test_cap as _;

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};
use xtask::util::root;

/// A session with text that's on no disk. It's compact, so a copy of it that DIE wrote, which is
/// pretty-printed, can be told from this one.
const SESSION: &str = r#"{"version":1,"focus":0,"buffers":[
    {"name":"*notes*","text":"unsaved words","modified":true,"cursor":3}
]}"#;

/// A restart's limit here: the plan's first check on the 1 s it has.
const RESTART_MS: f64 = 500.0;

/// One DIE at a time: LaunchServices may join up two apps with the same bundle id.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct App {
    dir: PathBuf,
    app: PathBuf,
    state: PathBuf,
    stderr: PathBuf,
}

/// Where LaunchServices's tool is. A test bundle comes out of its list when the test is over,
/// so that Finder's Open With doesn't offer it.
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/\
LaunchServices.framework/Versions/A/Support/lsregister";

impl App {
    fn build(name: &str) -> App {
        App::build_as(name, "dev.die.DIE.test")
    }

    /// A bundle under an id of its own, for a test that opens files through LaunchServices, which
    /// sends them to any running app with the id: another package's tests may have one.
    fn build_as(name: &str, id: &str) -> App {
        let dir = std::env::temp_dir().join(format!("die-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("DIE.app");
        let profile = xtask::app::Profile::Dev;
        xtask::app::build(&root(), &app, id, profile).unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        App {
            state: dir.join("state"),
            stderr: dir.join("stderr"),
            dir,
            app,
        }
    }

    /// `open -g`, with any more options. It takes the state directory by `--env`, since a
    /// launch gets its environment from LaunchServices rather than from this process.
    fn open(&self, options: &[&str], args: &[&str]) {
        // An instance that has only just started can make `open` fail with -600 until
        // LaunchServices has caught up, so that one error gets a few more tries.
        for tries_left in (0..5).rev() {
            let out = Command::new("open")
                .arg("-g")
                .args(options)
                .arg("--env")
                .arg(format!("DIE_STATE_DIR={}", self.state.display()))
                .arg("--stderr")
                .arg(&self.stderr)
                .arg(&self.app)
                .arg("--args")
                .args(args)
                .output()
                .unwrap();
            if out.status.success() {
                return;
            }
            let why = String::from_utf8_lossy(&out.stderr);
            assert!(tries_left > 0 && why.contains("error -600"), "{why}");
            sleep(Duration::from_millis(200));
        }
    }

    fn pids(&self) -> Vec<u32> {
        let exe = self.app.join("Contents/MacOS/die");
        let out = Command::new("pgrep").arg("-f").arg(exe).output().unwrap();
        let out = String::from_utf8(out.stdout).unwrap();
        out.lines().map(|l| l.parse().unwrap()).collect()
    }

    /// Wait for a write after the handover's: the previous copy is then the handover's own,
    /// which DIE wrote, rather than the session the test started with.
    fn wait_for_a_second_write(&self) -> String {
        let previous = self.state.join("session.prev.json");
        self.wait_for("a write after the handover", || {
            std::fs::read_to_string(&previous).is_ok_and(|p| p.contains("\"modified\": true"))
        });
        // A write renames session.json to session.prev.json before it renames the new copy
        // in, so for a moment there's no session.json.
        let file = self.state.join("session.json");
        self.wait_for("session.json back after a write", || file.exists());
        std::fs::read_to_string(&file).unwrap()
    }

    fn holder(&self) -> Option<u32> {
        let lock = std::fs::read_to_string(self.state.join("lock")).ok()?;
        lock.trim().parse().ok()
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    fn wait_for(&self, what: &str, mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "no {what} in 20 s, with stderr {:?}, and running {:?}",
                self.stderr(),
                self.pids()
            );
            sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        for pid in self.pids() {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
        let mut unregister = Command::new(LSREGISTER);
        unregister.arg("-u").arg(&self.app).stderr(Stdio::null());
        let _ = unregister.stdout(Stdio::null()).status();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn the_bundle_has_its_id_its_fonts_and_a_signature() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("bundle");
    let plist = std::fs::read_to_string(app.app.join("Contents/Info.plist")).unwrap();
    assert!(
        plist.contains("<string>dev.die.DIE.test</string>"),
        "{plist}"
    );
    // It says it opens any file, at the rank that makes it the opener of none. What Finder makes
    // of that isn't tried: LaunchServices disables a bundle in a temporary directory.
    let types = "<array><string>public.data</string></array>";
    assert!(plist.contains(types) && plist.contains("<string>Alternate</string>"));
    let lint = Command::new("plutil")
        .arg("-lint")
        .arg(app.app.join("Contents/Info.plist"))
        .output();
    assert!(lint.unwrap().status.success());
    for (name, _) in xtask::fonts::FILES {
        let font = app.app.join("Contents/Resources/fonts").join(name);
        assert!(font.exists(), "{name}");
    }
    let checkout = std::fs::read_to_string(app.app.join("Contents/Resources/checkout"));
    assert_eq!(
        checkout.unwrap(),
        format!("{}\n", root().display()),
        "for `rebuild`"
    );
    let verify = Command::new("codesign")
        .arg("--verify")
        .arg(&app.app)
        .status();
    assert!(verify.unwrap().success());
}

#[test]
fn two_launches_leave_one_instance() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("two");
    app.open(&[], &[]);
    app.wait_for("first instance", || {
        app.holder().is_some() && !app.pids().is_empty()
    });
    let first = app.holder().unwrap();
    // Plain `open` goes to the instance that's there, and `-n` starts one that finds the lock.
    app.open(&[], &[]);
    app.open(&["-n"], &[]);
    app.wait_for("refusal", || app.stderr().contains("already running"));
    app.wait_for("second instance gone", || app.pids() == [first]);
    assert_eq!(app.holder(), Some(first));
}

/// The new instance restores the session, takes the lock, and then writes the same text itself.
#[test]
fn a_restart_carries_unsaved_text_across() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("carry");
    std::fs::write(app.state.join("session.json"), SESSION).unwrap();
    app.open(&[], &["--restart-after-frame"]);
    app.wait_for("restart", || app.state.join("restart.log").exists());
    app.wait_for("old instance gone", || app.pids().len() == 1);
    let new = app.pids()[0];
    app.wait_for("lock handed over", || app.holder() == Some(new));
    let text = app.wait_for_a_second_write();
    assert!(text.contains("unsaved words"), "{text}");
    assert!(text.contains("\"modified\": true"), "{text}");
    assert_eq!(app.pids(), [new]);
}

/// A new build that panics before its first frame leaves the old one running, lock and all, and
/// costs no unsaved text. The old one writes again by itself.
#[test]
fn a_build_that_panics_before_its_first_frame_leaves_the_old_one_running() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("panics");
    std::fs::write(app.state.join("session.json"), SESSION).unwrap();
    app.open(
        &["--env", "DIE_PANIC_ON_RESTART=1"],
        &["--restart-after-frame"],
    );
    app.wait_for("the failure", || {
        app.stderr().contains("died before its first frame")
    });
    let old = app.holder().unwrap();
    app.wait_for("the new instance gone", || app.pids() == [old]);
    assert!(!app.state.join("restart.log").exists());
    let text = app.wait_for_a_second_write();
    assert!(text.contains("unsaved words"), "{text}");
    assert_eq!(app.pids(), [old]);
    assert_eq!(app.holder(), Some(old));
}

/// F4b: the loop itself, shorter, with the dev build. `cargo xtask restart-loop 20` is the proof.
#[test]
fn the_restart_loop_finds_equal_digests_and_logs_each_time() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let options = xtask::restart_loop::Options {
        restarts: 2,
        profile: xtask::app::Profile::Dev,
        bundle_id: "dev.die.DIE.test",
    };
    let times = xtask::restart_loop::drive(&root(), options).unwrap();
    assert_eq!(times.len(), 2);
}

/// What the loop would be fooled by: a digest that moved, a slow restart, a restart left out, or
/// equal digests of a session that was lost.
#[test]
fn the_restart_loop_refuses_what_is_wrong() {
    use xtask::restart_loop::{holds_the_seed, seed, verify};
    assert!(verify("a\na\na\n", "10\n20\n", 2, 1000.0).is_ok());
    assert!(verify("a\na\nb\n", "10\n20\n", 2, 1000.0).is_err());
    assert!(verify("a\na\na\n", "10\n1000\n", 2, 1000.0).is_err());
    assert!(verify("a\na\n", "10\n", 2, 1000.0).is_err());
    assert!(verify("a\na\na\n", "10\n", 2, 1000.0).is_err());
    let lost = r#"{"version":1,"buffers":[]}"#;
    assert!(holds_the_seed(lost).unwrap_err().contains("lost"));
    let this = std::env::current_exe().unwrap();
    let text = seed(&this, &this).unwrap().to_string();
    assert_eq!(holds_the_seed(&text), Ok(()));
    assert!(holds_the_seed(&text.replace("Cols", "Rows")).is_err());
}

/// The `die` a terminal runs, through a link as `~/.local/bin/die` is, from `dir`. It has to
/// print nothing and exit 0.
fn die_from_a_terminal(app: &App, args: &[&str]) {
    let link = app.dir.join("die");
    if !link.exists() {
        std::os::unix::fs::symlink(app.app.join("Contents/MacOS/die"), &link).unwrap();
    }
    let out = Command::new(link)
        .arg("--background")
        .args(args)
        .current_dir(&app.dir)
        .env_remove("PWD")
        .env("DIE_STATE_DIR", &app.state)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{}: {said}", out.status);
    assert!(said.is_empty() && out.stdout.is_empty(), "{said}");
}

/// The bytes before this line of `text`.
fn offset_of_line(text: &str, line: usize) -> u64 {
    let lines = text.split_inclusive('\n').take(line - 1);
    lines.map(str::len).sum::<usize>() as u64
}

/// The session's cursor in the buffer of this name, once it has one.
fn cursor_in(app: &App, name: &str) -> Option<u64> {
    let session = std::fs::read_to_string(app.state.join("session.json")).ok()?;
    let session: serde_json::Value = serde_json::from_str(&session).ok()?;
    let buffers = session["buffers"].as_array()?;
    let found = buffers.iter().find(|b| b["name"] == name)?;
    found["cursor"].as_u64()
}

/// With nothing running the terminal's `die` starts an instance through `open`, and exits. After
/// that it hands each file to that instance, at its line, and is gone: one process stays.
#[test]
#[ignore = "window: runs in ci --full"]
fn die_file_starts_an_instance_and_then_opens_files_in_it_at_their_lines() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("client");
    let text = "one\ntwo\nthree\nfour\n";
    std::fs::write(app.dir.join("a.txt"), text).unwrap();
    std::fs::write(app.dir.join("b.txt"), text).unwrap();
    die_from_a_terminal(&app, &["a.txt:2"]);
    app.wait_for("the instance to listen", || {
        app.state.join("open.sock").exists()
    });
    let first = app.holder().unwrap();
    app.wait_for("the first file at its line", || {
        cursor_in(&app, "a.txt") == Some(offset_of_line(text, 2))
    });
    die_from_a_terminal(&app, &["b.txt:3:2:"]);
    app.wait_for("the second file at its line and column", || {
        cursor_in(&app, "b.txt") == Some(offset_of_line(text, 3) + 1)
    });
    // A launch through `open` that finds the lock held sends its files on too, and goes.
    std::fs::write(app.dir.join("c.txt"), text).unwrap();
    let c = app.dir.join("c.txt:4");
    app.open(&["-n"], &["--background", c.to_str().unwrap()]);
    app.wait_for("the third file at its line", || {
        cursor_in(&app, "c.txt") == Some(offset_of_line(text, 4))
    });
    app.wait_for("one process", || app.pids() == [first]);
}

/// A restart moves the listener: what's sent after one reaches the new instance.
#[test]
#[ignore = "window: runs in ci --full"]
fn die_file_reaches_the_new_instance_after_a_restart() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("client-restart");
    std::fs::write(app.dir.join("a.txt"), "one\ntwo\n").unwrap();
    app.open(&[], &["--restart-after-frame"]);
    app.wait_for("restart", || app.state.join("restart.log").exists());
    die_from_a_terminal(&app, &["a.txt:2"]);
    app.wait_for("the file at its line", || {
        cursor_in(&app, "a.txt") == Some(4)
    });
    app.wait_for("old instance gone", || app.pids().len() == 1);
    assert_eq!(app.pids(), [app.holder().unwrap()]);
}

/// A session with unsaved text in a split, whose focus and cursors have to come back.
const SPLIT_SESSION: &str = r#"{"version":1,"focus":0,"buffers":[
    {"name":"*notes*","text":"unsaved words","modified":true,"cursor":3,"views":[
        {"window":0,"anchor":3,"head":3,"shape":"Char","goal":null,"top":0,"top_row":0},
        {"window":1,"anchor":8,"head":8,"shape":"Char","goal":null,"top":0,"top_row":0}]}],
    "layout":{"axis":"Cols","kids":[{"size":1,"node":{"window":0}},{"size":1,"node":{"window":1}}]},
    "windows":[{"buffer":0,"messages":false},{"buffer":0,"messages":false}],
    "focused_window":1}"#;

/// The rebuild you'll do all day: a new bundle goes in under a running instance, whose `restart`
/// then starts the new code with everything it had. The new build differs in a constant, which its
/// own restart shows: it logs under another name.
#[test]
#[ignore = "window: runs in ci --full"]
fn a_new_build_put_in_under_a_running_instance_restarts_with_everything() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("rebuild");
    std::fs::write(app.state.join("session.json"), SPLIT_SESSION).unwrap();
    app.open(&[], &["--restart-loop"]);
    let digests = || std::fs::read_to_string(app.state.join("restart-loop.digests"));
    let restored = |n: usize| digests().is_ok_and(|d| d.lines().count() == n);
    app.wait_for("the first restore", || restored(1));
    let old = app.holder().unwrap();

    let exe = app.app.join("Contents/MacOS/die");
    let mut code = std::fs::read(&exe).unwrap();
    let (name, other) = (b"restart.log", b"restart.lob");
    let at = code.windows(name.len()).position(|w| w == name).unwrap();
    code[at..at + other.len()].copy_from_slice(other);
    let built = app.dir.join("die-new");
    std::fs::copy(&exe, &built).unwrap();
    std::fs::write(&built, code).unwrap();
    let fonts = xtask::fonts::dir(&root());
    xtask::app::bundle(&built, &fonts, &app.app, "dev.die.DIE.test", &root()).unwrap();
    assert_eq!(app.pids(), [old], "the bundle went in under it");

    let go = || std::fs::write(app.state.join("restart-loop.go"), "").unwrap();
    go();
    app.wait_for("the old code's restart", || {
        app.state.join("restart.log").exists()
    });
    app.wait_for("the second restore", || restored(2));
    app.wait_for("old instance gone", || app.pids().len() == 1);
    let new = app.pids()[0];
    assert_ne!(new, old);
    app.wait_for("lock handed over", || app.holder() == Some(new));
    go();
    app.wait_for("the new code's restart", || {
        app.state.join("restart.lob").exists()
    });
    app.wait_for("the third restore", || restored(3));

    let digests = digests().unwrap();
    let digests: Vec<&str> = digests.lines().collect();
    assert_eq!(digests[0], digests[1], "{digests:?}");
    assert_eq!(digests[0], digests[2], "{digests:?}");
    let name = format!("restart-loop-{}.json", digests[0]);
    let session = std::fs::read_to_string(app.state.join(name)).unwrap();
    let session: serde_json::Value = serde_json::from_str(&session).unwrap();
    assert_eq!(session["buffers"][0]["text"], "unsaved words");
    assert_eq!(session["focused_window"], 1);
    let views = session["buffers"][0]["views"].as_array().unwrap();
    let heads: Vec<_> = views.iter().map(|v| v["head"].as_u64().unwrap()).collect();
    assert_eq!(heads, [3, 8]);
    let kids = session["layout"]["kids"].as_array().unwrap();
    assert_eq!(kids[0]["size"], kids[1]["size"], "{kids:?}");
    let left = std::fs::read_dir(&app.state).unwrap().flatten();
    let names: Vec<_> = left.map(|e| e.file_name()).collect();
    assert!(
        !names
            .iter()
            .any(|n| n.to_string_lossy().starts_with("handover-")),
        "{names:?}"
    );
}

#[test]
fn the_link_goes_on_a_path_that_has_its_directory_and_nothing_is_overwritten() {
    use xtask::app::link_command;
    let home = std::env::temp_dir().join(format!("die-app-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let app = home.join("Applications/DIE.app");
    let link = home.join(".local/bin/die");
    let line = link_command(&home, &app, "/usr/bin".as_ref()).unwrap();
    assert!(line.contains("export PATH="), "{line}");
    assert!(!link.exists() && !link.is_symlink());
    let on_path = std::env::join_paths(["/usr/bin".as_ref(), home.join(".local/bin").as_path()]);
    let on_path = on_path.unwrap();
    // Twice: a link already there is replaced.
    for _ in 0..2 {
        link_command(&home, &app, &on_path).unwrap();
        let to = std::fs::read_link(&link).unwrap();
        assert_eq!(to, app.join("Contents/MacOS/die"));
    }
    std::fs::remove_file(&link).unwrap();
    std::fs::write(&link, "mine").unwrap();
    let line = link_command(&home, &app, &on_path).unwrap();
    assert!(line.contains("left alone"), "{line}");
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "mine");
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_restart_hands_over_in_time() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("restart");
    app.open(&[], &["--restart-after-frame"]);
    let log = app.state.join("restart.log");
    app.wait_for("restart", || log.exists());
    app.wait_for("old instance gone", || app.pids().len() == 1);
    let new = app.pids()[0];
    app.wait_for("lock handed over", || app.holder() == Some(new));
    let log = std::fs::read_to_string(&log).unwrap();
    let took: f64 = log.trim().parse().unwrap();
    println!("restart: {took} ms");
    assert!(took < RESTART_MS, "the restart took {took} ms");
}

/// `die --background --wait` from a terminal, as `git commit` runs `$EDITOR`, through the link as
/// [`die_from_a_terminal`] has it. It runs until it's done.
fn die_waiting(app: &App, args: &[&str]) -> std::process::Child {
    let link = app.dir.join("die");
    if !link.exists() {
        std::os::unix::fs::symlink(app.app.join("Contents/MacOS/die"), &link).unwrap();
    }
    Command::new(link)
        .args(["--background", "--wait"])
        .args(args)
        .current_dir(&app.dir)
        .env_remove("PWD")
        .env("DIE_STATE_DIR", &app.state)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Wait for a `die --wait` to return: whether it succeeded, and what it said.
fn returned(app: &App, mut child: std::process::Child) -> (bool, String) {
    let mut status = None;
    app.wait_for("die --wait to return", || {
        status = child.try_wait().unwrap();
        status.is_some()
    });
    let mut said = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut said)
        .unwrap();
    (status.unwrap().success(), said)
}

/// With nothing running, `die --wait` starts an instance, which opens the file at its line, and
/// waits on it. The instance going, killed here, fails it, so git stops.
#[test]
#[ignore = "window: runs in ci --full"]
fn die_wait_starts_an_instance_and_fails_when_it_goes() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("wait");
    std::fs::write(app.dir.join("COMMIT_EDITMSG"), "one\ntwo\n").unwrap();
    let mut waiting = die_waiting(&app, &["COMMIT_EDITMSG:2"]);
    app.wait_for("the file at its line", || {
        cursor_in(&app, "COMMIT_EDITMSG") == Some(4)
    });
    sleep(Duration::from_millis(300));
    assert!(waiting.try_wait().unwrap().is_none(), "it waits");
    let pid = app.holder().unwrap();
    Command::new("kill").arg(pid.to_string()).status().unwrap();
    let (ok, said) = returned(&app, waiting);
    assert!(!ok, "{said}");
    assert!(said.contains("DIE quit before you were done"), "{said}");
}

/// Two clients wait at once, and a restart hands both to the new instance, which has their files
/// open from the session: they go on waiting on it, until it goes too.
#[test]
#[ignore = "window: runs in ci --full"]
fn a_restart_hands_every_waiting_client_to_the_new_instance() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build("wait-restart");
    app.open(&[], &["--restart-loop"]);
    let digests = || std::fs::read_to_string(app.state.join("restart-loop.digests"));
    let restored = |n: usize| digests().is_ok_and(|d| d.lines().count() == n);
    app.wait_for("the first restore", || restored(1));
    std::fs::write(app.dir.join("a.txt"), "a\n").unwrap();
    std::fs::write(app.dir.join("b.txt"), "b\n").unwrap();
    let (mut a, mut b) = (die_waiting(&app, &["a.txt"]), die_waiting(&app, &["b.txt"]));
    app.wait_for("both open", || {
        cursor_in(&app, "a.txt").is_some() && cursor_in(&app, "b.txt").is_some()
    });
    assert!(a.try_wait().unwrap().is_none() && b.try_wait().unwrap().is_none());
    let first = app.holder().unwrap();
    std::fs::write(app.state.join("restart-loop.go"), "").unwrap();
    app.wait_for("the second restore", || restored(2));
    app.wait_for("the new instance to listen", || {
        app.holder().is_some_and(|h| h != first) && app.state.join("open.sock").exists()
    });
    sleep(Duration::from_millis(500));
    assert!(a.try_wait().unwrap().is_none() && b.try_wait().unwrap().is_none());
    Command::new("kill")
        .arg(app.holder().unwrap().to_string())
        .status()
        .unwrap();
    for client in [a, b] {
        let (ok, said) = returned(&app, client);
        assert!(
            !ok && said.contains("DIE quit before you were done"),
            "{said}"
        );
    }
}

/// `open -a DIE file`, which sends the Apple event a Finder double-click and a drop on the Dock
/// icon send. With nothing running it starts an instance that opens the file. After that the file
/// goes to the instance there, and DIE doesn't bring itself to the front: `open -g` doesn't
/// activate it, so whether it takes the keyboard when Finder does isn't tried here. A name with
/// an accent comes composed, as typed, though AppKit's file URLs decompose it.
#[test]
#[ignore = "window: runs in ci --full"]
fn open_a_starts_an_instance_with_the_file_and_then_sends_files_to_it() {
    let _one = ONE_AT_A_TIME.lock().unwrap();
    let app = App::build_as("open-a", "dev.die.DIE.test.open");
    let open_files = |files: &[&str]| {
        let out = Command::new("open")
            .arg("-g")
            .arg("--env")
            .arg(format!("DIE_STATE_DIR={}", app.state.display()))
            .arg("--stderr")
            .arg(&app.stderr)
            .arg("-a")
            .arg(&app.app)
            .args(files.iter().map(|f| app.dir.join(f)))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let accented = "caf\u{e9} \u{65e5}\u{672c}.txt";
    for name in ["a.txt", "b.txt", accented] {
        std::fs::write(app.dir.join(name), "text\n").unwrap();
    }
    open_files(&["a.txt"]);
    app.wait_for("the first file", || cursor_in(&app, "a.txt").is_some());
    let first = app.holder().unwrap();
    open_files(&["b.txt", accented]);
    app.wait_for("the others", || {
        cursor_in(&app, "b.txt").is_some() && cursor_in(&app, accented).is_some()
    });
    assert_eq!(app.pids(), [first]);
    assert_ne!(front_app_pid(), Some(first), "it came to the front");
}

/// The pid of the app in front, as LaunchServices has it. This reads, and moves nothing.
fn front_app_pid() -> Option<u32> {
    let run = |args: &[&str]| {
        let out = Command::new("lsappinfo").args(args).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let front = run(&["front"])?;
    let pid = run(&["info", "-only", "pid", &front])?;
    pid.split_once('=')?.1.trim().parse().ok()
}
