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

/// Every case ends well within this, or fails. A cap that held would have stopped it.
const DEADLINE: Duration = Duration::from_secs(60);

/// A case that grows this far past its limit has shown that the cap doesn't hold. It stops
/// here with this exit code, to spare the machine.
pub const NOT_STOPPED: i32 = 3;

/// `abort()` raises it, on every Unix.
const SIGABRT: i32 = 6;

#[derive(Debug)]
pub struct Run {
    status: ExitStatus,
    stderr: String,
    stdout: String,
}

/// Run `body` in a child of this test binary, then return how the child went. `name` is
/// the test that's calling, which is how the child finds its way back here.
pub fn in_child(name: &str, body: impl FnOnce()) -> Run {
    if std::env::var_os(CHILD).is_some() {
        body();
        exit(0);
    }
    let exe = std::env::current_exe().unwrap();
    let child = Command::new(exe)
        .args([name, "--exact"])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id().to_string();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || done.send(child.wait_with_output()));
    let Ok(out) = finished.recv_timeout(DEADLINE) else {
        let _ = Command::new("kill").args(["-s", "KILL", &pid]).status();
        panic!("{name} was still running after {DEADLINE:?}");
    };
    let out = out.unwrap();
    Run {
        status: out.status,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

/// In a child: note this process's resident memory now. It goes straight to stdout, past
/// the harness's capture, for `Run::peak_kib`. `ps` is the one way std can ask, on both
/// macOS and Linux.
pub fn sample_memory() {
    use std::io::Write;
    let pid = std::process::id().to_string();
    let ps = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .expect("ps runs");
    let line = format!("rss {}\n", String::from_utf8_lossy(&ps.stdout).trim());
    std::io::stdout().write_all(line.as_bytes()).unwrap();
}

impl Run {
    /// The child aborted, having written `line`, and that once.
    #[track_caller]
    pub fn died_with(&self, line: &str) {
        assert_eq!(self.status.signal(), Some(SIGABRT), "{self:#?}");
        self.wrote(&[line]);
    }

    /// The child finished by itself.
    #[track_caller]
    pub fn lived(&self) {
        assert!(self.status.success(), "{self:#?}");
        self.wrote(&[]);
    }

    /// The child finished by itself, having written `line` once.
    #[track_caller]
    pub fn lived_with(&self, line: &str) {
        assert!(self.status.success(), "{self:#?}");
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
