//! F4a and K12: DIE.app, launched as you'd launch it, with `open -g` so that it takes no focus.
//! Each test uses a bundle of its own under a test bundle id and a state directory of its
//! own, so your DIE.app and its lock aren't touched, and quits what it started. They bundle
//! the dev build, which `cargo test` mostly has already; `cargo xtask app` bundles release.
#![cfg(target_os = "macos")]

use die_test_cap as _;

use std::path::PathBuf;
use std::process::Command;
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

impl App {
    fn build(name: &str) -> App {
        let dir = std::env::temp_dir().join(format!("die-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("DIE.app");
        let profile = xtask::app::Profile::Dev;
        xtask::app::build(&root(), &app, "dev.die.DIE.test", profile).unwrap();
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
        std::fs::read_to_string(self.state.join("session.json")).unwrap()
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
    for (name, _) in xtask::fonts::FILES {
        let font = app.app.join("Contents/Resources/fonts").join(name);
        assert!(font.exists(), "{name}");
    }
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
