//! Running orchid 0.2.2 (its `docs/user-guide/cli.md` and `src/main.rs`): one short call that
//! prints one JSON object, on stdout when it works and on stderr as
//! `{"error":"command_failed","message":…}` with exit 1 when it doesn't.
//!
//! - [`Cmd`] is what the client runs, and [`argv`] its arguments: `--config ROOT` first, always,
//!   so the root is never orchid's guess, and a message after `--`, so one starting with `-` is
//!   still a message (the Emacs client's A6).
//! - A [`Runner`] runs a [`Call`] to a [`Reply`]. [`ProcessRunner`] is the real one, which runs on a
//!   job's thread; [`FakeRunner`] records what it's asked and answers as a test says.
//! - [`Cli`] runs a command in a job (`Ctx::spawn`) and hands the result to a callback on the main
//!   thread. An error says orchid's `message` before its `error` (A5).

use die_core::{Ctx, Result, Sub};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The environment a child of DIE's mustn't inherit: an orchid tool that started DIE would make
/// every new session its child, and point every command at its own root.
pub const SCRUBBED: [&str; 3] = ["ORCHID_SESSION_ID", "ORCHID_CONFIG", "ORCHID_HOOK_DEPTH"];

// ------------------------------------------------------------------ commands

/// An orchid command, as the client runs them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// `create [--agent A] [--label L] [--working-dir D]`: `{"id","status","agent"}`.
    Create {
        /// The agent, or orchid's `default`.
        agent: Option<String>,
        /// The label.
        label: Option<String>,
        /// Where its tools run. It must exist.
        working_dir: Option<PathBuf>,
    },
    /// `send --id ID -- MESSAGE`: `{"id","status":"running","pid"}`, or `"queued"` while a
    /// worker runs, which is as good.
    Send {
        /// The session.
        id: String,
        /// The message.
        message: String,
    },
    /// `stop ID`: `{"id","status"}`.
    Stop {
        /// The session.
        id: String,
    },
    /// `delete ID`, which archives it: `{"id","archived":true}`.
    Delete {
        /// The session.
        id: String,
    },
    /// `list`: `{"sessions":[{"id","label"}]}`, oldest first.
    List,
    /// `agent`: `{"agents":[{"name","prompt","connection","valid","error"?}]}`.
    Agent,
    /// `set ID [--label L] [--working-dir D]`: `{"id","updated":true}`.
    Set {
        /// The session.
        id: String,
        /// A new label.
        label: Option<String>,
        /// A new working directory. It must exist.
        working_dir: Option<PathBuf>,
    },
    /// `--version`: plain text, `orchid 0.2.2 (codex compatibility X)`.
    Version,
}

impl Cmd {
    /// Its name, as orchid's argv has it: what a [`FakeRunner`] answers by.
    pub fn name(&self) -> &'static str {
        match self {
            Cmd::Create { .. } => "create",
            Cmd::Send { .. } => "send",
            Cmd::Stop { .. } => "stop",
            Cmd::Delete { .. } => "delete",
            Cmd::List => "list",
            Cmd::Agent => "agent",
            Cmd::Set { .. } => "set",
            Cmd::Version => "--version",
        }
    }

    /// Its arguments, after `--config ROOT`.
    pub fn args(&self) -> Vec<OsString> {
        let mut out: Vec<OsString> = vec![self.name().into()];
        fn dir(d: &Option<PathBuf>) -> Option<&OsStr> {
            d.as_deref().map(Path::as_os_str)
        }
        fn text(s: &Option<String>) -> Option<&OsStr> {
            s.as_deref().map(OsStr::new)
        }
        match self {
            Cmd::Create {
                agent,
                label,
                working_dir,
            } => {
                flag(&mut out, "--agent", text(agent));
                flag(&mut out, "--label", text(label));
                flag(&mut out, "--working-dir", dir(working_dir));
            }
            Cmd::Send { id, message } => {
                flag(&mut out, "--id", Some(OsStr::new(id)));
                out.push("--".into());
                out.push(message.into());
            }
            Cmd::Set {
                id,
                label,
                working_dir,
            } => {
                out.push(id.into());
                flag(&mut out, "--label", text(label));
                flag(&mut out, "--working-dir", dir(working_dir));
            }
            Cmd::Stop { id } | Cmd::Delete { id } => out.push(id.into()),
            Cmd::List | Cmd::Agent | Cmd::Version => {}
        }
        out
    }
}

/// `name value`, if there's a value.
fn flag(out: &mut Vec<OsString>, name: &str, value: Option<&OsStr>) {
    if let Some(v) = value {
        out.push(name.into());
        out.push(v.to_owned());
    }
}

/// The whole argv for `cmd` with this root: `--config ROOT` first, which orchid takes out
/// wherever it is, and it takes the first, so a message's own `--config` stays a message.
pub fn argv(root: &Path, cmd: &Cmd) -> Vec<OsString> {
    let mut out: Vec<OsString> = vec!["--config".into(), root.into()];
    out.extend(cmd.args());
    out
}

// ------------------------------------------------------------------ running

/// One run of orchid: the program, its whole argv, where it runs, and how long it may take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// The program: a name looked up on the PATH, or a path.
    pub program: PathBuf,
    /// Its arguments, `--config ROOT` first.
    pub args: Vec<OsString>,
    /// Where it runs, or DIE's own directory.
    pub cwd: Option<PathBuf>,
    /// How long it may run before it's killed.
    pub timeout: Duration,
}

impl Call {
    /// The command's name: the argument after `--config ROOT`.
    pub fn command(&self) -> &str {
        let after = self
            .args
            .iter()
            .position(|a| a == "--config")
            .map_or(0, |i| i + 2);
        self.args.get(after).and_then(|a| a.to_str()).unwrap_or("")
    }

    /// The arguments as text, for a test to compare.
    pub fn strings(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }
}

/// What a run gave back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// The exit code. None if it didn't exit by itself: killed, or never started.
    pub code: Option<i32>,
    /// What it printed on stdout.
    pub stdout: String,
    /// What it printed on stderr.
    pub stderr: String,
    /// Why there's no exit code: it couldn't start, or it ran past its timeout.
    pub failure: Option<String>,
}

/// A failed run, said for a person, with what it gave back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError {
    /// orchid's own `message`, or else its `error`, or else what it printed, or else its exit.
    pub message: String,
    /// What it gave back.
    pub reply: Reply,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Reply {
    /// A run that printed `stdout` and exited 0.
    pub fn ok(stdout: &str) -> Reply {
        Reply {
            code: Some(0),
            stdout: format!("{stdout}\n"),
            ..Reply::default()
        }
    }

    /// A run that failed as orchid does: its error object on stderr, and exit 1.
    pub fn error(message: &str) -> Reply {
        let e = serde_json::json!({"error": "command_failed", "message": message});
        Reply {
            code: Some(1),
            stderr: format!("{e}\n"),
            ..Reply::default()
        }
    }

    /// What it means: stdout's JSON (or its text, for `--version`) if it exited 0, else an error.
    pub fn result(self) -> std::result::Result<Value, CliError> {
        if self.code == Some(0) && self.failure.is_none() {
            let out = self.stdout.trim();
            return Ok(serde_json::from_str(out).unwrap_or_else(|_| Value::String(out.into())));
        }
        let message = self.error_text();
        Err(CliError {
            message,
            reply: self,
        })
    }

    /// Why it failed, for a person.
    fn error_text(&self) -> String {
        if let Some(f) = &self.failure {
            return f.clone();
        }
        let said = |text: &str| -> Option<String> {
            let v: Value = serde_json::from_str(text.trim().lines().last()?).ok()?;
            let field = |k| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
            field("message")
                .or_else(|| field("error"))
                .map(String::from)
        };
        let (err, out) = (self.stderr.trim(), self.stdout.trim());
        said(err)
            .or_else(|| said(out))
            .or_else(|| (!err.is_empty()).then(|| err.to_string()))
            .or_else(|| (!out.is_empty()).then(|| out.to_string()))
            .unwrap_or_else(|| match self.code {
                Some(c) => format!("orchid exited with code {c}"),
                None => "orchid didn't exit".into(),
            })
    }
}

/// What runs orchid. It's called on a job's thread, so it may block.
pub trait Runner: Send + Sync + 'static {
    /// Run `call` to its end, or its timeout.
    fn run(&self, call: &Call) -> Reply;
    /// Where `program` would run from, as Emacs's `executable-find` says, or None.
    fn find(&self, program: &Path) -> Option<PathBuf>;
}

/// orchid as a process: stdout and stderr apart, nothing on stdin, [`SCRUBBED`] taken out of its
/// environment, and killed at its timeout.
#[derive(Clone, Debug, Default)]
pub struct ProcessRunner {
    /// The PATH it gets and is looked up on. None is `die_core::process::path()`, your login
    /// shell's, which DIE.app from the Dock needs to find `~/.local/bin/orchid`; asking it can
    /// start your shell, so a test gives one.
    pub path: Option<OsString>,
    /// More environment, after the scrub: a test's own HOME.
    pub env: Vec<(OsString, OsString)>,
}

impl ProcessRunner {
    /// The PATH it runs with.
    fn path(&self) -> OsString {
        self.path
            .clone()
            .unwrap_or_else(|| die_core::process::path().to_owned())
    }
}

impl Runner for ProcessRunner {
    fn find(&self, program: &Path) -> Option<PathBuf> {
        which(program, &self.path())
    }

    fn run(&self, call: &Call) -> Reply {
        let path = self.path();
        let Some(program) = which(&call.program, &path) else {
            return failed(format!("{} isn't on the PATH", call.program.display()));
        };
        let mut cmd = Command::new(program);
        cmd.args(&call.args).env("PATH", &path);
        for name in SCRUBBED {
            cmd.env_remove(name);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &call.cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match cmd.spawn() {
            Ok(child) => wait(child, call.timeout),
            Err(e) => failed(format!("orchid couldn't start: {e}")),
        }
    }
}

fn failed(why: String) -> Reply {
    Reply {
        failure: Some(why),
        ..Reply::default()
    }
}

/// `child`'s output and exit, killing it at `timeout`. Each stream is read on a thread of its own,
/// so neither fills its pipe while the other is read; one a grandchild holds open is given up on
/// at the timeout.
fn wait(mut child: std::process::Child, timeout: Duration) -> Reply {
    let start = Instant::now();
    let out = read_all(child.stdout.take());
    let err = read_all(child.stderr.take());
    let mut reply = Reply::default();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                reply.code = status.code();
                break;
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let limit = timeout.as_secs_f64();
                reply.failure = Some(format!(
                    "orchid took longer than {limit} s, and was stopped"
                ));
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                reply.failure = Some(format!("orchid couldn't be waited for: {e}"));
                break;
            }
        }
    }
    let left = timeout
        .saturating_sub(start.elapsed())
        .max(Duration::from_millis(100));
    reply.stdout = out.recv_timeout(left).unwrap_or_default();
    reply.stderr = err.recv_timeout(left).unwrap_or_default();
    reply
}

/// All of a stream, read on a thread, as text.
fn read_all(stream: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = vec![];
        if let Some(mut s) = stream {
            let _ = s.read_to_end(&mut bytes);
        }
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    rx
}

/// Where `program` is: itself if it names a directory, else the first executable file by that name
/// on `path`, as Emacs's `executable-find` looks.
pub fn which(program: &Path, path: &OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.components().count() > 1 {
        return runnable(program).then(|| program.to_path_buf());
    }
    std::env::split_paths(path)
        .map(|d| d.join(program))
        .find(|p| runnable(p))
}

// ------------------------------------------------------------------ the fake

type Answer = Arc<dyn Fn(&Call) -> Reply + Send + Sync>;

#[derive(Default)]
struct FakeState {
    calls: Vec<Call>,
    answers: HashMap<String, Answer>,
    missing: bool,
}

/// A [`Runner`] for tests: it records each call, and answers each command as told, or else with
/// an orchid error that says it wasn't told. Its clones share what they record.
#[derive(Clone, Default)]
pub struct FakeRunner(Arc<Mutex<FakeState>>);

impl FakeRunner {
    /// One that hasn't been told anything.
    pub fn new() -> FakeRunner {
        FakeRunner::default()
    }

    /// Answer every `command` (`"send"`, `"--version"`) with `reply`.
    pub fn reply(&self, command: &str, reply: Reply) {
        self.on(command, move |_| reply.clone());
    }

    /// Answer every `command` with what `f` makes of the call: to write a session's files as
    /// `create` would, or to answer differently each time.
    pub fn on(&self, command: &str, f: impl Fn(&Call) -> Reply + Send + Sync + 'static) {
        self.0
            .lock()
            .unwrap()
            .answers
            .insert(command.into(), Arc::new(f));
    }

    /// Say the program isn't on the PATH, or is again.
    pub fn set_missing(&self, missing: bool) {
        self.0.lock().unwrap().missing = missing;
    }

    /// Every call so far, oldest first.
    pub fn calls(&self) -> Vec<Call> {
        self.0.lock().unwrap().calls.clone()
    }

    /// Every call's arguments so far, as text.
    pub fn argvs(&self) -> Vec<Vec<String>> {
        self.calls().iter().map(Call::strings).collect()
    }
}

impl Runner for FakeRunner {
    fn find(&self, program: &Path) -> Option<PathBuf> {
        (!self.0.lock().unwrap().missing).then(|| program.to_path_buf())
    }

    fn run(&self, call: &Call) -> Reply {
        let answer = {
            let mut s = self.0.lock().unwrap();
            s.calls.push(call.clone());
            s.answers.get(call.command()).cloned()
        };
        match answer {
            Some(f) => f(call),
            None => Reply::error(&format!(
                "the fake runner wasn't told how to answer {}",
                call.command()
            )),
        }
    }
}

// ------------------------------------------------------------------ running from DIE

/// What it takes to run orchid, cheap to clone into a job: the program, the root, the runner
/// and the timeout. [`crate::Orchid::cli`] gives one.
#[derive(Clone)]
pub struct Cli {
    /// The program.
    pub program: PathBuf,
    /// The config root, passed as `--config` to every command.
    pub root: PathBuf,
    /// The runner.
    pub runner: Arc<dyn Runner>,
    /// Each call's timeout.
    pub timeout: Duration,
}

/// A command's result: its JSON, or why it failed.
pub type Outcome = std::result::Result<Value, CliError>;

impl Cli {
    /// The call for `cmd`, run in `cwd`.
    pub fn call(&self, cmd: &Cmd, cwd: Option<&Path>) -> Call {
        Call {
            program: self.program.clone(),
            args: argv(&self.root, cmd),
            cwd: cwd.map(Path::to_path_buf),
            timeout: self.timeout,
        }
    }

    /// Where the program would run from, or None if it isn't on the PATH. The real runner asks
    /// your login shell for its PATH the first time, so call this in a job too.
    pub fn find(&self) -> Option<PathBuf> {
        self.runner.find(&self.program)
    }

    /// Run `cmd` now, on this thread: for a job of your own. It blocks for as long as orchid
    /// runs, so never on the main thread.
    pub fn run_now(&self, cmd: &Cmd, cwd: Option<&Path>) -> Outcome {
        self.runner.run(&self.call(cmd, cwd)).result()
    }

    /// Run `cmd` in `cwd` on a thread of its own, then `done` with its result on the main thread
    /// (C3). Keep the `Sub` while you want `done` to run, or `detach` it to fire and forget.
    pub fn run(
        &self,
        ctx: &mut Ctx,
        cmd: Cmd,
        cwd: Option<PathBuf>,
        done: impl FnOnce(&mut Ctx, Outcome) -> Result + 'static,
    ) -> Sub {
        let cli = self.clone();
        ctx.spawn(move || cli.run_now(&cmd, cwd.as_deref()), done)
    }

    /// Run `cmds` one after another in one job, as the marks' batch does, then `done` with each
    /// result, in order.
    pub fn run_all(
        &self,
        ctx: &mut Ctx,
        cmds: Vec<Cmd>,
        cwd: Option<PathBuf>,
        done: impl FnOnce(&mut Ctx, Vec<Outcome>) -> Result + 'static,
    ) -> Sub {
        let cli = self.clone();
        let work = move || {
            cmds.iter()
                .map(|c| cli.run_now(c, cwd.as_deref()))
                .collect()
        };
        ctx.spawn(work, done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: Vec<OsString>) -> Vec<String> {
        v.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    #[test]
    fn config_comes_first_and_a_message_after_two_dashes() {
        let root = Path::new("/tmp/root");
        let send = Cmd::Send {
            id: "s1".into(),
            message: "-x --config elsewhere".into(),
        };
        assert_eq!(
            strings(argv(root, &send)),
            [
                "--config",
                "/tmp/root",
                "send",
                "--id",
                "s1",
                "--",
                "-x --config elsewhere"
            ]
        );
        let create = Cmd::Create {
            agent: Some("developer".into()),
            label: None,
            working_dir: Some("/tmp/w".into()),
        };
        assert_eq!(
            strings(argv(root, &create)),
            [
                "--config",
                "/tmp/root",
                "create",
                "--agent",
                "developer",
                "--working-dir",
                "/tmp/w"
            ]
        );
        let set = Cmd::Set {
            id: "s1".into(),
            label: Some("default-die".into()),
            working_dir: None,
        };
        assert_eq!(strings(set.args()), ["set", "s1", "--label", "default-die"]);
        assert_eq!(
            strings(Cmd::Stop { id: "s1".into() }.args()),
            ["stop", "s1"]
        );
        assert_eq!(
            strings(Cmd::Delete { id: "s1".into() }.args()),
            ["delete", "s1"]
        );
        assert_eq!(strings(Cmd::List.args()), ["list"]);
        assert_eq!(strings(Cmd::Agent.args()), ["agent"]);
        assert_eq!(
            strings(argv(root, &Cmd::Version)),
            ["--config", "/tmp/root", "--version"]
        );
    }

    #[test]
    fn a_reply_means_its_json_or_orchids_message() {
        let ok = Reply::ok(r#"{"id":"s1","status":"running","pid":42}"#)
            .result()
            .unwrap();
        assert_eq!(ok["status"], "running");
        let version = Reply::ok("orchid 0.2.2 (codex compatibility 1)")
            .result()
            .unwrap();
        assert_eq!(
            version,
            Value::String("orchid 0.2.2 (codex compatibility 1)".into())
        );
        let e = Reply::error("send requires a message")
            .result()
            .unwrap_err();
        assert_eq!(e.message, "send requires a message", "message before error");
        let only_error = Reply {
            code: Some(1),
            stderr: r#"{"error":"command_failed"}"#.into(),
            ..Reply::default()
        };
        assert_eq!(only_error.result().unwrap_err().message, "command_failed");
        let text = Reply {
            code: Some(2),
            stderr: "thread 'main' panicked\n".into(),
            ..Reply::default()
        };
        assert_eq!(text.result().unwrap_err().message, "thread 'main' panicked");
        let silent = Reply {
            code: Some(3),
            ..Reply::default()
        };
        assert_eq!(
            silent.result().unwrap_err().message,
            "orchid exited with code 3"
        );
        let stopped = failed("orchid couldn't start: no such file".into());
        assert_eq!(
            stopped.result().unwrap_err().message,
            "orchid couldn't start: no such file"
        );
    }

    #[test]
    fn the_fake_records_and_answers_by_command() {
        let fake = FakeRunner::new();
        fake.reply("list", Reply::ok(r#"{"sessions":[]}"#));
        fake.on("send", |c| {
            Reply::ok(&format!(
                r#"{{"id":"{}","status":"queued"}}"#,
                c.strings()[4]
            ))
        });
        let cli = Cli {
            program: "orchid".into(),
            root: "/tmp/r".into(),
            runner: Arc::new(fake.clone()),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            cli.run_now(&Cmd::List, None).unwrap()["sessions"],
            serde_json::json!([])
        );
        let sent = cli.run_now(
            &Cmd::Send {
                id: "s9".into(),
                message: "hi".into(),
            },
            Some(Path::new("/w")),
        );
        assert_eq!(sent.unwrap()["id"], "s9");
        let e = cli.run_now(&Cmd::Agent, None).unwrap_err();
        assert_eq!(e.message, "the fake runner wasn't told how to answer agent");
        let calls = fake.calls();
        assert_eq!(
            calls.iter().map(Call::command).collect::<Vec<_>>(),
            ["list", "send", "agent"]
        );
        assert_eq!(calls[1].cwd.as_deref(), Some(Path::new("/w")));
        assert_eq!(fake.argvs()[0], ["--config", "/tmp/r", "list"]);
    }

    #[test]
    fn which_finds_executables_on_the_path() {
        let d = std::env::temp_dir().join(format!("orchid-which-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let tool = d.join("tool");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::write(d.join("plain"), "").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(["/nowhere".into(), d.clone()]).unwrap();
        assert_eq!(which(Path::new("tool"), &path), Some(tool.clone()));
        assert_eq!(which(Path::new("plain"), &path), None, "not executable");
        assert_eq!(which(Path::new("missing"), &path), None);
        assert_eq!(
            which(&tool, OsStr::new("")),
            Some(tool.clone()),
            "a path is itself"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
