//! Running a test's body as a child of the test binary, and reading how it ended. A case
//! that dies takes only its child with it, and runs under the harness's output capture,
//! which the cap's line must get past.
#![allow(dead_code)]

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus, Stdio, exit};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Set in the child, which is how a test knows to run its body.
const CHILD: &str = "DIE_TEST_CAP_CHILD";

/// Lowers the kernel's limit in a child, which goes under it at its start.
const LOWER_LIMIT: &str = "DIE_TEST_CAP_SYSTEM_LIMIT_MIB";

/// Every case ends well within this, or fails. A cap that held would have stopped it.
const DEADLINE: Duration = Duration::from_secs(60);

/// A child exits with this when its body has run to the end. If no test has the name it
/// was given, it runs nothing and exits 0, and that mustn't pass for a case that lived.
const BODY_RAN: i32 = 7;

/// A case that grows this far past its limit has shown that the cap doesn't hold. It stops
/// here with this exit code, to spare the machine.
pub const NOT_STOPPED: i32 = 3;

/// `abort()` raises it, on every Unix.
const SIGABRT: i32 = 6;

/// How the kernel ends a process at its limit.
const SIGKILL: i32 = 9;

#[derive(Debug)]
pub struct Run {
    pid: u32,
    status: ExitStatus,
    stderr: String,
    stdout: String,
}

/// Run `body` in a child of this test binary, then return how the child went. `name` is
/// the test that's calling, which is how the child finds its way back here.
pub fn in_child(name: &str, body: impl FnOnce()) -> Run {
    run_body(name, None, body)
}

/// The same, with the kernel's limit on the child lowered to `mib`.
pub fn in_child_under(name: &str, mib: usize, body: impl FnOnce()) -> Run {
    run_body(name, Some(mib), body)
}

fn run_body(name: &str, mib: Option<usize>, body: impl FnOnce()) -> Run {
    if std::env::var_os(CHILD).is_some() {
        body();
        exit(BODY_RAN);
    }
    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(exe);
    if let Some(mib) = mib {
        cmd.env(LOWER_LIMIT, mib.to_string());
    }
    let child = cmd
        .args([name, "--exact"])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || done.send(child.wait_with_output()));
    let Ok(out) = finished.recv_timeout(DEADLINE) else {
        let _ = Command::new("kill")
            .args(["-s", "KILL", &pid.to_string()])
            .status();
        panic!("{name} was still running after {DEADLINE:?}");
    };
    let out = out.unwrap();
    Run {
        pid,
        status: out.status,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

/// In a child: write `line` straight to stdout, past the harness's capture.
pub fn note(line: &str) {
    use std::io::Write;
    std::io::stdout()
        .write_all(format!("{line}\n").as_bytes())
        .unwrap();
}

/// In a child: note this process's resident memory now, for `Run::peak_kib`. `ps` is the
/// one way std can ask, on both macOS and Linux.
pub fn sample_memory() {
    let pid = std::process::id().to_string();
    let ps = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .expect("ps runs");
    note(&format!(
        "rss {}",
        String::from_utf8_lossy(&ps.stdout).trim()
    ));
}

impl Run {
    /// The child aborted, having written `line`, and that once.
    #[track_caller]
    pub fn died_with(&self, line: &str) {
        assert_eq!(self.status.signal(), Some(SIGABRT), "{self:#?}");
        self.wrote(&[line]);
    }

    /// The kernel killed the child, with no line from the cap.
    #[track_caller]
    pub fn killed_by_the_kernel(&self) {
        assert_eq!(self.status.signal(), Some(SIGKILL), "{self:#?}");
        self.wrote(&[]);
    }

    /// The child's body ran to its end.
    #[track_caller]
    pub fn lived(&self) {
        assert_eq!(self.status.code(), Some(BODY_RAN), "{self:#?}");
        self.wrote(&[]);
    }

    /// The child's body ran to its end, having written `line` once.
    #[track_caller]
    pub fn lived_with(&self, line: &str) {
        assert_eq!(self.status.code(), Some(BODY_RAN), "{self:#?}");
        self.wrote(&[line]);
    }

    /// The cap's lines on stderr are exactly `lines`.
    #[track_caller]
    fn wrote(&self, lines: &[&str]) {
        let found: Vec<&str> = self
            .stderr
            .lines()
            .filter(|l| l.contains("die-test-cap:"))
            .collect();
        assert_eq!(found, lines, "{self:#?}");
    }

    /// The child's pid, as it was when it started.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// What the child wrote to stdout, with the harness's output and all.
    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    /// The most memory the child was seen to hold, in KiB.
    #[track_caller]
    pub fn peak_kib(&self) -> usize {
        self.stdout
            .lines()
            .filter_map(|l| l.strip_prefix("rss "))
            .map(|kib| kib.parse().expect("ps prints a number"))
            .max()
            .unwrap_or_else(|| panic!("no samples: {self:#?}"))
    }
}
