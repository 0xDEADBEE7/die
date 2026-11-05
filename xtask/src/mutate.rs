//! `cargo xtask mutate`: die-core's planted bugs (from spike S4's `mutate.sh`).
//! Each mutant breaks the core in one specific way, in a throwaway copy of the
//! working tree, and the property tests must catch it.
//!
//! There are three ways to catch one, and the report names which: tests that fail by
//! name, the test binary reaching its memory limit (`die-test-cap`'s line), or the run
//! reaching its time limit. A run that ends any other way catches nothing. A kernel kill,
//! say, means memory the count can't see grew to the kernel's limit, or the machine ran
//! out, and passing that off as a catch once hid it, so it's an error that stops the run.
//!
//! A run has a process group of its own. On expiry the whole group is killed, because
//! killing cargo alone leaves the test binary running. That group doesn't get a terminal's
//! ^C, so a small watchdog in it kills the group if xtask itself goes away.
//!
//! A mutant needs catching, not explaining, so the first test to fail ends a run, the tests
//! don't shrink what they find, and the build has no debug info. A mutant's time is then
//! nearly all its build, which debug info made about 3 times the CPU.
//!
//! `--changed` runs only the mutants whose file changed since the merge base with main.

use crate::util::{
    Result, TempDir, cargo, changed_in_worktree, copy_worktree, read, show, task_dir, write,
};
use serde::Deserialize;
use std::fs::File;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// A mutant's time limit is this many times the unmutated run's, and at least `LEAST_TIME`.
const TIME_FACTOR: u32 = 5;
const LEAST_TIME: Duration = Duration::from_secs(120);

/// The unmutated run has nothing to go by, and may have to build every dependency.
const FIRST_RUN_TIME: Duration = Duration::from_secs(1800);

/// What `die-test-cap` writes when a test binary reaches its limit.
const CAP_LINE: &str = "die-test-cap: over the memory limit";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mutants {
    #[serde(rename = "mutant")]
    mutants: Vec<Mutant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutant {
    pub name: String,
    /// Relative to `CRATE`.
    pub file: String,
    old: String,
    new: String,
}

/// The crate the mutants live in, and `file` is relative to.
const CRATE: &str = "crates/die-core";

/// `DIE_PROP_CASES` in a run with no `--cases`. Since the first failure ends a run, this costs
/// little but the unmutated run. It stays here for M48, which took about 600 cases in both runs
/// measured, so 1,000 would often miss it. The other 47 failed within about 200.
pub const DEFAULT_CASES: u32 = 2000;

/// What `cargo xtask mutate` was asked for.
#[derive(Debug, PartialEq)]
pub struct Options {
    pub cases: u32,
    /// Only the mutants whose file changed since the merge base with main.
    pub changed: bool,
}

impl Options {
    /// `None` if these aren't `mutate`'s arguments.
    pub fn parse(args: &[&str]) -> Option<Options> {
        let mut options = Options {
            cases: DEFAULT_CASES,
            changed: false,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match *arg {
                "--changed" => options.changed = true,
                "--cases" => options.cases = args.next()?.parse().ok()?,
                _ => return None,
            }
        }
        Some(options)
    }
}

pub fn parse(text: &str) -> Result<Vec<Mutant>> {
    let m: Mutants = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(m.mutants)
}

pub fn load(spec: &Path) -> Result<Vec<Mutant>> {
    parse(&read(spec)?).map_err(|e| format!("{}: {e}", spec.display()))
}

/// The mutants a run takes, and how many it leaves out.
pub struct Selection {
    pub run: Vec<Mutant>,
    pub skipped: usize,
}

/// With `changed`, paths from the repository's root, only the mutants on those files.
/// With none, all of them.
pub fn select(mutants: Vec<Mutant>, changed: Option<&[String]>) -> Selection {
    let total = mutants.len();
    let run: Vec<Mutant> = match changed {
        None => mutants,
        Some(changed) => mutants
            .into_iter()
            .filter(|m| {
                let path = format!("{CRATE}/{}", m.file);
                changed.contains(&path)
            })
            .collect(),
    };
    let skipped = total - run.len();
    Selection { run, skipped }
}

pub fn run(root: &Path, options: Options) -> Result {
    let start = Instant::now();
    let cases = options.cases;
    let all = load(&root.join(CRATE).join("tests/mutants.toml"))?;
    let changed = match options.changed {
        false => None,
        // What can't be told can't be skipped.
        true => changed_in_worktree(root)
            .inspect_err(|e| println!("mutate: running them all, for git can't say: {e}"))
            .ok(),
    };
    let Selection {
        run: mutants,
        skipped,
    } = select(all, changed.as_deref());
    if changed.is_some() {
        println!(
            "mutate --changed: {} mutants on changed files, {skipped} skipped",
            mutants.len()
        );
        if mutants.is_empty() {
            return Ok(());
        }
    }
    let work = task_dir(root, "mutate");
    let tmp = TempDir::at(work.join("tree"))?;
    let tree = tmp.path();
    copy_worktree(root, tree)?;
    let target = work.join("target");
    let log = work.join("output.txt");

    println!("mutate: the unmutated tree first");
    let first = Instant::now();
    let unmutated = prop_tests(tree, &target, cases, FIRST_RUN_TIME, &log)?;
    let first = first.elapsed();
    let built = build_secs(&unmutated.text).map_or(Duration::ZERO, Duration::from_secs_f64);
    match unmutated.verdict() {
        Verdict::Passed => {}
        Verdict::Caught(why) | Verdict::Broken(why) => {
            return Err(format!(
                "mutate: the property tests fail without a mutant: {why}"
            ));
        }
    }
    let limit = time_limit(first, built);
    println!(
        "mutate: the unmutated run took {}, and each mutant gets {} s",
        timing(first, &unmutated.text),
        limit.as_secs()
    );

    let mut missed = Vec::new();
    for mu in &mutants {
        let t = Instant::now();
        let path = tree.join(CRATE).join(&mu.file);
        let orig = read(&path)?;
        if orig.matches(&mu.old).count() != 1 {
            return Err(format!(
                "mutate: {}: the text to replace must occur exactly once in {}",
                mu.name, mu.file
            ));
        }
        write(&path, &orig.replacen(&mu.old, &mu.new, 1))?;
        let res = prop_tests(tree, &target, cases, limit, &log);
        write(&path, &orig)?;
        let res = res?;
        let time = timing(t.elapsed(), &res.text);
        match res.verdict() {
            Verdict::Caught(how) => println!("caught      {}  <- {how}  ({time})", mu.name),
            Verdict::Passed => {
                println!("NOT CAUGHT  {}  ({time})", mu.name);
                missed.push(mu.name.clone());
            }
            Verdict::Broken(why) => return Err(format!("mutate: {}: {why}", mu.name)),
        }
    }
    let secs = start.elapsed().as_secs();
    if missed.is_empty() {
        let n = mutants.len();
        println!("mutate: {n} of {n} caught in {secs} s, {skipped} skipped");
        Ok(())
    } else {
        Err(format!(
            "mutate: not caught: {} (after {secs} s)",
            missed.join(", ")
        ))
    }
}

/// A run's time, and how much of it cargo spent building.
fn timing(total: Duration, text: &str) -> String {
    match build_secs(text) {
        Some(build) => format!("{:.1} s, {build:.1} s of it building", total.as_secs_f64()),
        None => format!("{:.1} s", total.as_secs_f64()),
    }
}

/// How long cargo says it took to build: `Finished ... target(s) in 1m 02s`, or `in 2.51s`.
fn build_secs(text: &str) -> Option<f64> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("Finished "))?;
    let (_, time) = line.rsplit_once(" in ")?;
    let (minutes, secs) = match time.split_once("m ") {
        Some((m, s)) => (m.parse::<f64>().ok()?, s),
        None => (0.0, time),
    };
    Some(minutes * 60.0 + secs.strip_suffix('s')?.parse::<f64>().ok()?)
}

/// The time limit for a mutant, given how long the unmutated run took and how much of that
/// was building. Only the tests' part is multiplied: a first run in a cold cache once gave
/// each mutant 76 minutes, nearly all of it for a build the mutants don't repeat.
fn time_limit(unmutated: Duration, built: Duration) -> Duration {
    (built + unmutated.saturating_sub(built) * TIME_FACTOR).max(LEAST_TIME)
}

/// Run the property tests, for at most `limit`.
fn prop_tests(tree: &Path, target: &Path, cases: u32, limit: Duration, log: &Path) -> Result<Run> {
    let mut cmd = Command::new(cargo());
    cmd.current_dir(tree)
        .args([
            "test",
            "--offline",
            "-p",
            "die-core",
            "--test",
            "prop",
            "--target-dir",
        ])
        .arg(target)
        // A test `ci` leaves out for its time still catches mutants here.
        .args(["--", "--include-ignored"])
        .env("DIE_PROP_CASES", cases.to_string())
        .env("DIE_PROP_SHRINK", "0")
        .env("CARGO_PROFILE_TEST_DEBUG", "false");
    run_limited(&mut cmd, limit, log)
}

/// How a run ended.
#[derive(Debug)]
struct Run {
    status: ExitStatus,
    /// It reached its time limit, and was killed for it.
    timed_out: bool,
    /// A test failed, and the run was killed for that rather than left to finish.
    stopped_at_failure: bool,
    /// Its stdout and stderr.
    text: String,
}

#[derive(Debug, PartialEq)]
enum Verdict {
    /// The tests passed.
    Passed,
    /// The run caught a mutant, in the way named here.
    Caught(String),
    /// The run says nothing about a mutant, for the reason given here.
    Broken(String),
}

impl Run {
    fn verdict(&self) -> Verdict {
        if self.timed_out {
            return Verdict::Caught("time limit".into());
        }
        if self.stopped_at_failure {
            return Verdict::Caught(failed_tests(&self.text).unwrap_or_default());
        }
        if self.status.success() {
            return Verdict::Passed;
        }
        let text = &self.text;
        if text.contains("error[E") || text.contains("could not compile") {
            return Verdict::Broken(format!("the tree doesn't compile:\n{text}"));
        }
        if let Some(signal) = self.status.signal() {
            return Verdict::Broken(format!(
                "cargo was killed by signal {signal}, which catches nothing:\n{}",
                tail(text)
            ));
        }
        let capped = text.contains(CAP_LINE);
        match killed_by(text) {
            Some(signal) if capped && signal.contains("SIGABRT") => {
                Verdict::Caught("memory limit".into())
            }
            Some(signal) => Verdict::Broken(format!(
                "the test binary was killed ({signal}), which catches nothing:\n{}",
                tail(text)
            )),
            None if capped => Verdict::Caught("memory limit".into()),
            None => match failed_tests(text) {
                Some(names) => Verdict::Caught(names),
                None => Verdict::Broken(format!(
                    "the run failed ({}) and named no test, which catches nothing:\n{}",
                    self.status,
                    tail(text)
                )),
            },
        }
    }
}

/// The signal that killed a test binary, in cargo's words: `signal: 9, SIGKILL: kill`.
fn killed_by(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once("process didn't exit successfully: ")?;
    let line = rest.lines().next()?;
    let start = line.find("(signal: ")? + 1;
    line[start..].split_once(')').map(|(signal, _)| signal)
}

/// The names of the tests that failed, if any did.
fn failed_tests(text: &str) -> Option<String> {
    let failed: Vec<&str> = text
        .lines()
        .filter_map(|l| {
            l.strip_prefix("test ")
                .and_then(|l| l.strip_suffix(" ... FAILED"))
        })
        .collect();
    (!failed.is_empty()).then(|| failed.join(" "))
}

/// The end of a run's output, for an error message.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

/// Run `cmd` in a process group of its own, for at most `limit`, with its output in `log`,
/// and until a test fails. A file can't block as a pipe can, if something in the group
/// outlives the run. When the run ends, for any reason, whatever's left in the group is killed.
fn run_limited(cmd: &mut Command, limit: Duration, log: &Path) -> Result<Run> {
    let what = show(cmd);
    let err = |e: std::io::Error| format!("mutate: {what}: {e}");
    let stdout = File::create(log).map_err(err)?;
    let stderr = stdout.try_clone().map_err(err)?;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0)
        .spawn()
        .map_err(err)?;
    let group = child.id();
    let mut watchdog = watchdog(group, std::process::id()).map_err(err)?;
    let start = Instant::now();
    let mut stopped_at_failure = false;
    let finished = loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            break Some(status);
        }
        if start.elapsed() >= limit {
            break None;
        }
        let text = std::fs::read(log).map_err(err)?;
        if failed_tests(&String::from_utf8_lossy(&text)).is_some() {
            stopped_at_failure = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    kill_group(group);
    let timed_out = finished.is_none() && !stopped_at_failure;
    let status = match finished {
        Some(status) => status,
        None => {
            let _ = child.kill();
            child.wait().map_err(err)?
        }
    };
    let _ = watchdog.kill();
    let _ = watchdog.wait();
    let text = std::fs::read(log).map_err(err)?;
    Ok(Run {
        status,
        timed_out,
        stopped_at_failure,
        text: String::from_utf8_lossy(&text).into_owned(),
    })
}

/// A process in `group` that kills the group once process `parent` has gone.
fn watchdog(group: u32, parent: u32) -> std::io::Result<Child> {
    let script =
        format!("while kill -0 {parent} 2>/dev/null; do sleep 1; done; kill -s KILL -- -{group}");
    Command::new("sh")
        .args(["-c", &script])
        .process_group(group as i32)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Kill every process in `group`. std can only signal its own children, and `kill` has no
/// need of `unsafe`. An empty group is no error here.
fn kill_group(group: u32) {
    let _ = Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{group}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::TempDir;

    fn exited(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn signalled(signal: i32) -> ExitStatus {
        ExitStatus::from_raw(signal)
    }

    fn verdict(status: ExitStatus, text: &str) -> Verdict {
        let run = Run {
            status,
            timed_out: false,
            stopped_at_failure: false,
            text: text.into(),
        };
        run.verdict()
    }

    /// Cargo's words for a test binary that a signal killed.
    fn killed(signal: &str) -> String {
        format!(
            "running 4 tests\nerror: test failed, to rerun pass `-p die-core --test prop`\n\n\
             Caused by:\n  process didn't exit successfully: `/w/target/debug/deps/prop-1` \
             ({signal})\n"
        )
    }

    const SIGABRT: &str = "signal: 6, SIGABRT: process abort signal";
    const SIGKILL: &str = "signal: 9, SIGKILL: kill";
    const CAPPED: &str = "die-test-cap: over the memory limit of 4294967296 bytes (4096 MiB)\n";

    #[test]
    fn tests_that_pass_catch_nothing() {
        assert_eq!(verdict(exited(0), "test result: ok.\n"), Verdict::Passed);
        // A refused allocation the test handled, and passed.
        assert_eq!(verdict(exited(0), CAPPED), Verdict::Passed);
    }

    #[test]
    fn tests_that_fail_by_name_are_named() {
        let text = "test a ... ok\ntest b ... FAILED\ntest c::d ... FAILED\n\
                    error: test failed, to rerun pass `-p die-core --test prop`\n";
        assert_eq!(verdict(exited(101), text), Verdict::Caught("b c::d".into()));
    }

    #[test]
    fn the_caps_line_is_the_memory_limit() {
        let memory = Verdict::Caught("memory limit".into());
        let text = format!("{CAPPED}{}", killed(SIGABRT));
        assert_eq!(verdict(exited(101), &text), memory);
        // Harness output with no newline can come before it, in the middle of a line.
        let text = format!("test a ... {CAPPED}{}", killed(SIGABRT));
        assert_eq!(verdict(exited(101), &text), memory);
        // A test that took the refusal as an error rather than aborting.
        let text = format!("{CAPPED}test a ... FAILED\n");
        assert_eq!(verdict(exited(101), &text), memory);
    }

    #[test]
    fn a_kernel_kill_catches_nothing_and_says_which_signal() {
        let Verdict::Broken(why) = verdict(exited(101), &killed(SIGKILL)) else {
            panic!("a kernel kill counted as a catch");
        };
        assert!(why.contains(SIGKILL), "{why}");
    }

    #[test]
    fn a_kill_that_comes_after_the_caps_line_still_catches_nothing() {
        // A test handled a refusal, then the kernel killed the binary anyway.
        let text = format!("{CAPPED}{}", killed(SIGKILL));
        let Verdict::Broken(why) = verdict(exited(101), &text) else {
            panic!("a kernel kill counted as a catch");
        };
        assert!(why.contains(SIGKILL), "{why}");
    }

    #[test]
    fn an_abort_without_the_caps_line_catches_nothing() {
        let Verdict::Broken(why) = verdict(exited(101), &killed(SIGABRT)) else {
            panic!("an abort counted as a catch");
        };
        assert!(why.contains("SIGABRT"), "{why}");
    }

    #[test]
    fn cargo_itself_killed_catches_nothing_and_says_which_signal() {
        let Verdict::Broken(why) = verdict(signalled(9), "running 4 tests\n") else {
            panic!("a kill counted as a catch");
        };
        assert!(why.contains("signal 9"), "{why}");
    }

    #[test]
    fn a_failure_that_names_no_test_catches_nothing() {
        let text = "error: failed to get `ropey` as a dependency\n";
        let Verdict::Broken(why) = verdict(exited(101), text) else {
            panic!("a failure with no test counted as a catch");
        };
        assert!(
            why.contains("named no test") && why.contains("ropey"),
            "{why}"
        );
    }

    #[test]
    fn a_tree_that_doesnt_compile_catches_nothing() {
        for text in [
            "error[E0308]: mismatched types\n",
            "error: could not compile `die-core`\n",
        ] {
            let Verdict::Broken(why) = verdict(exited(101), text) else {
                panic!("a compile error counted as a catch");
            };
            assert!(why.contains("doesn't compile"), "{why}");
        }
    }

    #[test]
    fn the_time_limit_is_named_whatever_killed_the_run() {
        for status in [signalled(9), exited(101), exited(0)] {
            let run = Run {
                status,
                timed_out: true,
                stopped_at_failure: false,
                text: killed(SIGKILL),
            };
            assert_eq!(run.verdict(), Verdict::Caught("time limit".into()));
        }
    }

    #[test]
    fn the_signal_comes_from_cargos_sentence() {
        assert_eq!(killed_by(&killed(SIGKILL)), Some(SIGKILL));
        assert_eq!(killed_by(&killed(SIGABRT)), Some(SIGABRT));
        // A test's own output, with no such sentence around it.
        assert_eq!(killed_by("the bug (signal: 9) is here\n"), None);
    }

    #[test]
    fn a_mutant_gets_five_times_the_first_tests_and_at_least_two_minutes() {
        let limit = |secs, built| {
            time_limit(Duration::from_secs(secs), Duration::from_secs(built)).as_secs()
        };
        assert_eq!(limit(1, 0), 120);
        assert_eq!(limit(24, 0), 120);
        assert_eq!(limit(25, 0), 125);
        assert_eq!(limit(200, 0), 1000);
        // Only the tests' part is multiplied.
        assert_eq!(limit(900, 500), 500 + 5 * 400);
        assert_eq!(limit(60, 60), 120);
        // A build that says it took longer than the run isn't negative.
        assert_eq!(limit(200, 300), 300);
    }

    /// `pid` is gone within a few seconds. A killed process is a zombie until `init`
    /// reaps it, and `kill -0` still finds it until then.
    fn gone(pid: &str) -> bool {
        (0..100).any(|_| {
            let alive = Command::new("kill")
                .args(["-0", pid])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            if alive {
                std::thread::sleep(Duration::from_millis(50));
            }
            !alive
        })
    }

    /// A script with a `sleep` in the background, which is what a test binary is to cargo:
    /// it keeps running if only its parent is killed. It writes the pid to a file, then
    /// does `then`.
    fn with_orphan(dir: &TempDir, then: &str) -> Command {
        let pid = dir.path().join("pid");
        let script = format!(
            "sleep 60 & echo $! > {0}.new; mv {0}.new {0}; {then}",
            pid.display()
        );
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &script]);
        cmd
    }

    /// The orphan's pid, once its script has written it.
    fn orphan_pid(dir: &TempDir) -> String {
        let file = dir.path().join("pid");
        for _ in 0..200 {
            if let Ok(pid) = std::fs::read_to_string(&file) {
                return pid.trim().to_string();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the script wrote no pid");
    }

    #[test]
    fn on_expiry_the_whole_group_dies() {
        let dir = TempDir::new("mutate-expiry").unwrap();
        let log = dir.path().join("log");
        let mut cmd = with_orphan(&dir, "echo started; wait");
        let run = run_limited(&mut cmd, Duration::from_secs(2), &log).unwrap();
        assert!(run.timed_out, "{run:?}");
        assert_eq!(run.status.signal(), Some(9), "{run:?}");
        assert_eq!(run.text, "started\n");
        assert!(
            gone(&orphan_pid(&dir)),
            "the group's other process lives on"
        );
    }

    #[test]
    #[ignore = "speed: runs in ci --full"]
    fn the_first_failure_ends_a_run_and_the_whole_group() {
        let dir = TempDir::new("mutate-failure").unwrap();
        let log = dir.path().join("log");
        let mut cmd = with_orphan(&dir, "echo 'test a ... ok'; echo 'test b ... FAILED'; wait");
        let t = Instant::now();
        let run = run_limited(&mut cmd, Duration::from_secs(60), &log).unwrap();
        assert!(t.elapsed() < Duration::from_secs(50), "it ran to its limit");
        assert!(run.stopped_at_failure && !run.timed_out, "{run:?}");
        assert_eq!(run.verdict(), Verdict::Caught("b".into()));
        assert!(
            gone(&orphan_pid(&dir)),
            "the group's other process lives on"
        );
    }

    #[test]
    fn a_tests_output_that_mentions_failure_doesnt_end_a_run() {
        let dir = TempDir::new("mutate-mention").unwrap();
        let log = dir.path().join("log");
        let mut cmd = with_orphan(&dir, "echo 'it says FAILED'; echo 'test a ... ok'; exit 0");
        let run = run_limited(&mut cmd, Duration::from_secs(60), &log).unwrap();
        assert!(!run.stopped_at_failure, "{run:?}");
        assert_eq!(run.verdict(), Verdict::Passed);
    }

    #[test]
    fn a_runs_build_time_comes_from_cargos_sentence() {
        let finished = |time: &str| {
            format!(
                "   Compiling die-core v0.0.0\n    Finished `test` profile [optimized + \
                 debuginfo] target(s) in {time}\n     Running tests/prop.rs\n"
            )
        };
        assert_eq!(build_secs(&finished("2.51s")), Some(2.51));
        assert_eq!(build_secs(&finished("1m 02s")), Some(62.0));
        assert_eq!(build_secs("running 4 tests\n"), None);
    }

    #[test]
    fn a_run_that_ends_leaves_nothing_behind() {
        let dir = TempDir::new("mutate-end").unwrap();
        let log = dir.path().join("log");
        let mut cmd = with_orphan(&dir, "exit 7");
        let run = run_limited(&mut cmd, Duration::from_secs(60), &log).unwrap();
        assert!(!run.timed_out, "{run:?}");
        assert_eq!(run.status.code(), Some(7), "{run:?}");
        assert!(
            gone(&orphan_pid(&dir)),
            "the group's other process lives on"
        );
    }

    #[test]
    fn the_group_dies_if_its_parent_goes() {
        let dir = TempDir::new("mutate-parent").unwrap();
        let mut run = with_orphan(&dir, "wait")
            .process_group(0)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let orphan = orphan_pid(&dir);
        let mut parent = Command::new("sleep").arg("300").spawn().unwrap();
        let mut guard = watchdog(run.id(), parent.id()).unwrap();
        parent.kill().unwrap();
        parent.wait().unwrap();
        assert_eq!(run.wait().unwrap().signal(), Some(9));
        guard.wait().unwrap();
        assert!(gone(&orphan), "the group's other process lives on");
    }
}
