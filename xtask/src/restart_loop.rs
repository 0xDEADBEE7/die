//! `cargo xtask restart-loop N`, exit test (b): N rebuilds and restarts of DIE.app over one
//! session. The bundle and the state directory are its own, under a bundle id of its own, so
//! your DIE.app and its lock aren't touched. It launches with `open -g` and quits what it starts.
//!
//! Under `--restart-loop` DIE writes a digest of its session after each restore and waits for a
//! go-ahead file, which gives this time to put a fresh build in the bundle, and then restarts.
//! Equal digests alone would pass if the session had been lost, so the session they are digests
//! of must also still hold what it started with: files, splits, each window's cursor and scroll,
//! unsaved text and registers.

use crate::app::{self, Profile};
use crate::util::Result;
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

/// Exit test (b)'s limit, from `restart` to the new instance's first frame.
pub const LIMIT_MS: f64 = 1000.0;
/// How long one restore may take to be seen before something has gone wrong.
const PATIENCE: Duration = Duration::from_secs(20);
const DIGESTS: &str = "restart-loop.digests";
const GO: &str = "restart-loop.go";
const RESTART_LOG: &str = "restart.log";

/// A line of the files, whose length gives each cursor's place.
const LINE: &str = "a line of the file\n";
const LINES: usize = 200;
const HEAD_A: usize = 150;
const HEAD_B: usize = 120;

/// How the release loop differs from a test's.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    pub restarts: usize,
    pub profile: Profile,
    pub bundle_id: &'a str,
}

/// The task: `restarts` of the release build, with each restart's time left in the task's log.
pub fn run(root: &Path, restarts: usize) -> Result {
    let options = Options {
        restarts,
        profile: Profile::Release,
        bundle_id: "dev.die.DIE.restart-loop",
    };
    let times = drive(root, options)?;
    let dir = crate::util::task_dir(root, "restart-loop");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let log = dir.join(RESTART_LOG);
    let lines: Vec<String> = times.iter().map(|ms| format!("{ms:.1}\n")).collect();
    fs::write(&log, lines.concat()).map_err(|e| format!("{}: {e}", log.display()))?;
    println!("restart-loop: log in {}", log.display());
    Ok(())
}

/// Run the loop and check it. Each restart's time in ms, if it passes. If it fails, the error
/// names the state directory, which is kept.
pub fn drive(root: &Path, options: Options) -> Result<Vec<f64>> {
    let mut run = Loop::new(root, options)?;
    let result = run.turns().and_then(|()| run.check());
    result.map_err(|e| {
        run.keep = true;
        format!("{e} (state kept in {})", run.state.display())
    })
}

/// What the driver reads after the last restore: every digest equal, one more than the restarts
/// as the first launch restores too, and each restart in time. The times in ms.
pub fn verify(digests: &str, log: &str, restarts: usize, limit_ms: f64) -> Result<Vec<f64>> {
    let digests: Vec<&str> = digests.lines().collect();
    if digests.len() != restarts + 1 {
        let n = digests.len();
        return Err(format!("{n} digests for {restarts} restarts"));
    }
    if let Some(i) = digests.iter().position(|d| *d != digests[0]) {
        let (first, other) = (digests[0], digests[i]);
        return Err(format!("restore {i}'s digest is {other}, not {first}"));
    }
    let times = log
        .lines()
        .map(|l| l.parse().map_err(|e| format!("restart log: {l:?}: {e}")))
        .collect::<Result<Vec<f64>>>()?;
    if times.len() != restarts {
        let n = times.len();
        return Err(format!("{n} logged times for {restarts} restarts"));
    }
    if let Some((i, ms)) = times.iter().enumerate().find(|(_, ms)| **ms >= limit_ms) {
        return Err(format!("restart {} took {ms} ms", i + 1));
    }
    Ok(times)
}

/// A session with every kind of thing exit test (b) names. `a` and `b` are files.
pub fn seed(a: &Path, b: &Path) -> Result<Value> {
    let meta = fs::metadata(b).map_err(|e| format!("{}: {e}", b.display()))?;
    let view = |window: usize, line: usize| {
        let (at, top) = (line * LINE.len(), (line - 5) * LINE.len());
        json!({"window": window, "anchor": at, "head": at, "shape": "Char", "goal": null,
               "top": top, "top_row": 0})
    };
    let window = |window: usize| json!({"window": window});
    Ok(json!({
        "version": 1,
        "focus": 1,
        "buffers": [
            {"name": "a.txt", "file": {"path": a}, "views": [view(0, HEAD_A)]},
            {"name": "b.txt", "modified": true, "text": LINE.repeat(LINES),
             "file": {"path": b, "fingerprint": {"secs": meta.mtime(),
                "nanos": meta.mtime_nsec(), "len": meta.len(), "inode": meta.ino()}},
             "views": [view(1, HEAD_B)]},
            {"name": "*notes*", "text": "notes on no disk", "modified": true},
        ],
        "layout": {"axis": "Cols", "kids": [
            {"size": 2, "node": window(0)},
            {"size": 1, "node": window(1)},
        ]},
        "windows": [{"buffer": 0}, {"buffer": 1}],
        "focused_window": 1,
        "registers": [{"name": "z", "kind": "Line", "text": "a register\n"}],
        "histories": {"search": ["needle"]},
    }))
}

/// Whether a restored session still holds what [`seed`] started it with.
pub fn holds_the_seed(session: &str) -> Result {
    let s: Value = serde_json::from_str(session).map_err(|e| format!("the session: {e}"))?;
    let buffer = |name: &str| {
        let mut buffers = s["buffers"].as_array().into_iter().flatten();
        buffers.find(|b| b["name"] == name)
    };
    let view = |name: &str, window: usize, line: usize| {
        let head = line * LINE.len();
        let views = buffer(name).and_then(|b| b["views"].as_array());
        views.is_some_and(|views| {
            let scrolled = |v: &Value| v["top"].as_u64().is_some_and(|top| top > 0);
            let at = |v: &Value| v["window"] == window && v["head"] == head;
            views.iter().any(|v| at(v) && scrolled(v))
        })
    };
    let clean =
        buffer("a.txt").is_some_and(|b| b["file"]["path"].is_string() && b["text"].is_null());
    let unsaved = buffer("b.txt").is_some_and(|b| {
        b["text"] == LINE.repeat(LINES)
            && b["modified"] == true
            && b["file"]["fingerprint"].is_object()
    });
    let notes = buffer("*notes*").is_some_and(|b| b["text"] == "notes on no disk");
    let split = s["layout"]["axis"] == "Cols" && s["windows"].as_array().map(Vec::len) == Some(2);
    let register = s["registers"].as_array().is_some_and(|registers| {
        let z = |r: &Value| r["name"] == "z" && r["kind"] == "Line";
        registers.iter().any(z)
    });
    let checks = [
        ("the clean file", clean),
        ("the unsaved text", unsaved),
        ("the buffer with no file", notes),
        (
            "the first window's cursor or scroll",
            view("a.txt", 0, HEAD_A),
        ),
        (
            "the second window's cursor or scroll",
            view("b.txt", 1, HEAD_B),
        ),
        ("the split", split),
        ("the register", register),
        ("the history", s["histories"]["search"][0] == "needle"),
    ];
    match checks.iter().find(|(_, there)| !there) {
        Some((what, _)) => Err(format!("the session lost {what}: {session}")),
        None => Ok(()),
    }
}

/// A loop under way, and everything of it to take away afterwards.
struct Loop<'a> {
    root: &'a Path,
    options: Options<'a>,
    dir: PathBuf,
    app: PathBuf,
    state: PathBuf,
    keep: bool,
}

impl<'a> Loop<'a> {
    fn new(root: &'a Path, options: Options<'a>) -> Result<Loop<'a>> {
        let name = format!("die-restart-loop-{}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        let state = dir.join("state");
        fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
        Ok(Loop {
            root,
            options,
            app: dir.join("DIE.app"),
            state,
            dir,
            keep: false,
        })
    }

    fn turns(&self) -> Result {
        self.build()?;
        let (a, b) = (self.dir.join("a.txt"), self.dir.join("b.txt"));
        let text = LINE.repeat(LINES);
        for (file, text) in [(&a, text.as_str()), (&b, "what the disk has\n")] {
            fs::write(file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        let session = seed(&a, &b)?.to_string();
        fs::write(self.state.join("session.json"), session).map_err(|e| e.to_string())?;
        self.open()?;
        self.wait_for_restores(1)?;
        for turn in 1..=self.options.restarts {
            self.rebuild()?;
            let go = self.state.join(GO);
            fs::write(&go, "").map_err(|e| format!("{}: {e}", go.display()))?;
            self.wait_for_restores(turn + 1)?;
            if let Some(ms) = self.read(RESTART_LOG).lines().last() {
                println!("restart {turn}: {ms} ms");
            }
        }
        Ok(())
    }

    fn check(&self) -> Result<Vec<f64>> {
        let digests = self.read(DIGESTS);
        let log = self.read(RESTART_LOG);
        let times = verify(&digests, &log, self.options.restarts, LIMIT_MS)?;
        let digest = digests.lines().next().unwrap_or_default();
        holds_the_seed(&self.read(&format!("restart-loop-{digest}.json")))?;
        let worst = times.iter().copied().fold(0.0, f64::max);
        println!(
            "restart-loop: {} equal digests, slowest of {} restarts {worst:.1} ms",
            digests.lines().count(),
            times.len(),
        );
        Ok(times)
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.state.join(name)).unwrap_or_default()
    }

    fn build(&self) -> Result {
        let (profile, id) = (self.options.profile, self.options.bundle_id);
        app::build(self.root, &self.app, id, profile)
    }

    /// A rebuild, as a change would make one: the bin compiles and links again, and a new
    /// bundle replaces the one that's running.
    fn rebuild(&self) -> Result {
        let main = self.root.join("apps/die-dev/src/main.rs");
        fs::File::options()
            .write(true)
            .open(&main)
            .and_then(|f| f.set_modified(SystemTime::now()))
            .map_err(|e| format!("{}: {e}", main.display()))?;
        self.build()
    }

    fn open(&self) -> Result {
        let status = Command::new("open")
            .arg("-g")
            .arg("--env")
            .arg(format!("DIE_STATE_DIR={}", self.state.display()))
            .arg(&self.app)
            .args(["--args", "--restart-loop"])
            .status()
            .map_err(|e| format!("open: {e}"))?;
        match status.success() {
            true => Ok(()),
            false => Err(format!("open: {status}")),
        }
    }

    /// A digest is written with the old instance gone and its time logged.
    fn wait_for_restores(&self, n: usize) -> Result {
        let start = Instant::now();
        while self.read(DIGESTS).lines().count() < n {
            if start.elapsed() > PATIENCE {
                return Err(format!("no restore {n} in {PATIENCE:?}"));
            }
            sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}

impl Drop for Loop<'_> {
    fn drop(&mut self) {
        let exe = self.app.join("Contents/MacOS/die");
        let found = Command::new("pgrep").arg("-f").arg(exe).output();
        let pids = found.map(|o| o.stdout).unwrap_or_default();
        for pid in String::from_utf8_lossy(&pids).lines() {
            let _ = Command::new("kill").arg(pid).status();
        }
        if !self.keep {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}
