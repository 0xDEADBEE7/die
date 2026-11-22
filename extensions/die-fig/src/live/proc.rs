//! One command's process: `/bin/sh -c cmd` in a process group of its own, started, read and
//! waited for on threads it owns, so the main thread never forks, reads or waits.
//!
//! - A thread starts it and waits for it: the command is always waited for (no zombie), and once
//!   it has ended, whatever it left running in its group is sent SIGTERM, then SIGKILL after
//!   [`KILL_GRACE`] if still there, so nothing it started outlives it unseen.
//! - Its stdout goes to a [`Sink`]: a run's is kept whole up to [`MAX_OUTPUT`] and read as data
//!   on that thread when it ends (past the cap it's stopped); a stream's lines go into its
//!   [`Grow`] as they come. The last [`STDERR_LINES`] lines of stderr are kept for an error.
//! - Dropping a [`Proc`] stops it: its group gets SIGTERM, and SIGKILL after [`KILL_GRACE`]
//!   if the command hasn't ended, from a thread of its own.
//!
//! die-fig has no `unsafe`, so `/bin/kill` sends the signals, as die-core's commands' are sent.

use super::grow::{Grow, MAX_LINE};
use super::{KILL_GRACE, MAX_OUTPUT, PathFrom, STDERR_LINES};
use crate::data::Table;
use crate::error::Result;
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;

/// How long, once a command has ended and its group is gone, what it wrote last has to come out
/// of its pipes. A process outside its group holding a pipe isn't waited for.
const DRAIN: Duration = Duration::from_millis(500);
/// How often a stop looks to see whether the command has ended.
const LOOK: Duration = Duration::from_millis(10);
/// The longest stderr line an error keeps, in chars.
const STDERR_WIDTH: usize = 400;

/// How a run's whole output is read as data, on the command's thread.
pub(crate) type ReadOutput = Box<dyn FnOnce(&str) -> Result<Table> + Send>;

/// Where a command's stdout goes.
pub(crate) enum Sink {
    /// A run's: kept whole, then read as data by this on the command's thread.
    Run(ReadOutput),
    /// A stream's: each line into its table as it comes.
    Stream(Arc<Mutex<Grow>>),
}

/// How a command ended, once it has.
pub(crate) struct Outcome {
    /// Its exit status; None if it never started.
    pub status: Option<ExitStatus>,
    /// Why it couldn't start, if it couldn't.
    pub error: Option<String>,
    /// The last lines of its stderr.
    pub stderr: Vec<String>,
    /// It printed more than [`MAX_OUTPUT`] and was stopped.
    pub too_large: bool,
    /// It was stopped (its `Proc` dropped, or its stream's header unreadable), not ended by
    /// itself.
    pub stopped: bool,
    /// A run's output read as data, when it ended well and printed something.
    pub table: Option<Result<Table>>,
    /// A run printed nothing but blanks.
    pub silent: bool,
}

#[derive(Default)]
struct Inner {
    /// The group's id (the command's pid), once started.
    pgid: u32,
    /// A stop was asked for.
    stop: bool,
    /// The stop's thread is on its way.
    killing: bool,
    /// The command was waited for.
    reaped: bool,
    /// The command was waited for and its group is gone.
    gone: bool,
    /// Everything's done: the outcome is in.
    done: bool,
    outcome: Option<Outcome>,
}

#[derive(Default)]
pub(crate) struct Shared {
    inner: Mutex<Inner>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ask the command to stop: its group gets SIGTERM, then SIGKILL after the grace if it
    /// hasn't ended, from a thread of its own. Once only; before it has started, the thread
    /// that starts it stops it.
    fn stop(self: &Arc<Shared>) {
        let pgid = {
            let mut i = self.lock();
            i.stop = true;
            if i.pgid == 0 || i.reaped || i.killing {
                return;
            }
            i.killing = true;
            i.pgid
        };
        self.kill(pgid);
    }

    fn kill(self: &Arc<Shared>, pgid: u32) {
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("fig-live-stop".into())
            .spawn(move || {
                signal(pgid, "TERM");
                for _ in 0..steps(KILL_GRACE) {
                    if me.lock().reaped {
                        return;
                    }
                    std::thread::sleep(LOOK);
                }
                if !me.lock().reaped {
                    signal(pgid, "KILL");
                }
            });
        if spawned.is_err() {
            // No thread to wait with: straight to the kill.
            signal(pgid, "KILL");
        }
    }
}

/// A command running, or ended and not yet looked at.
pub(crate) struct Proc {
    shared: Arc<Shared>,
}

impl Proc {
    /// Start `cmd` in `dir`, its stdout to `sink`, on a thread of its own.
    pub(crate) fn start(cmd: String, dir: PathBuf, path: PathFrom, sink: Sink) -> Proc {
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("fig-live".into())
            .spawn(move || supervise(&s, &cmd, &dir, path, sink));
        if let Err(e) = spawned {
            let mut i = shared.lock();
            i.outcome = Some(Outcome::failed(format!("no thread to run it on: {e}")));
            (i.reaped, i.gone, i.done) = (true, true, true);
        }
        Proc { shared }
    }

    /// The command's pid, which is its group's id, while it runs.
    pub(crate) fn pid(&self) -> Option<u32> {
        let i = self.shared.lock();
        (i.pgid != 0 && !i.reaped).then_some(i.pgid)
    }

    /// Whether it has ended and its outcome is in.
    pub(crate) fn done(&self) -> bool {
        self.shared.lock().done
    }

    /// How it ended, once: None while it runs.
    pub(crate) fn take(&self) -> Option<Outcome> {
        let mut i = self.shared.lock();
        i.done.then(|| i.outcome.take()).flatten()
    }

    /// What to wait on for it to be gone.
    pub(crate) fn handle(&self) -> Arc<Shared> {
        self.shared.clone()
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

/// Whether a command is gone: waited for, and nothing left in its group.
pub(crate) fn gone(s: &Shared) -> bool {
    s.lock().gone
}

impl Outcome {
    fn failed(error: String) -> Outcome {
        Outcome {
            status: None,
            error: Some(error),
            stderr: vec![],
            too_large: false,
            stopped: false,
            table: None,
            silent: false,
        }
    }
}

/// What a run's reader keeps: the bytes, and whether there were too many.
#[derive(Default)]
struct Bytes {
    out: Vec<u8>,
    too_large: bool,
}

/// The command's own thread: start it, read it, wait for it, sweep its group, and say how it
/// ended.
fn supervise(s: &Arc<Shared>, line: &str, dir: &std::path::Path, path: PathFrom, sink: Sink) {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(line)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if path == PathFrom::Login {
        cmd.env("PATH", die_core::process::path());
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let mut i = s.lock();
            i.outcome = Some(Outcome::failed(e.to_string()));
            (i.reaped, i.gone, i.done) = (true, true, true);
            return;
        }
    };
    let pgid = child.id();
    let stop_now = {
        let mut i = s.lock();
        i.pgid = pgid;
        let now = i.stop && !i.killing;
        i.killing |= now;
        now
    };
    if stop_now {
        s.kill(pgid);
    }
    let (tx, rx) = mpsc::channel::<()>();
    // stderr: its last lines.
    let tail = Arc::new(Mutex::new(Tail::default()));
    if let Some(err) = child.stderr.take() {
        let (tail, tx) = (tail.clone(), tx.clone());
        let _ = std::thread::Builder::new()
            .name("fig-live-err".into())
            .spawn(move || {
                read_tail(err, &tail);
                let _ = tx.send(());
            });
    }
    // stdout: the data.
    let bytes = Arc::new(Mutex::new(Bytes::default()));
    let (sink, parse) = match sink {
        Sink::Run(parse) => (None, Some(parse)),
        Sink::Stream(g) => (Some(g), None),
    };
    if let Some(out) = child.stdout.take() {
        let (s, tx, bytes, grow) = (s.clone(), tx.clone(), bytes.clone(), sink.clone());
        let _ = std::thread::Builder::new()
            .name("fig-live-out".into())
            .spawn(move || {
                match grow {
                    Some(g) => read_lines(out, &g, &s),
                    None => read_bytes(out, &bytes, &s),
                }
                let _ = tx.send(());
            });
    }
    drop(tx);
    let status = child.wait().ok();
    s.lock().reaped = true;
    sweep(pgid);
    s.lock().gone = true;
    for _ in 0..2 {
        if rx.recv_timeout(DRAIN).is_err() {
            break;
        }
    }
    let stderr = lock(&tail).lines();
    let stopped = s.lock().stop;
    let (out, too_large) = {
        let mut b = lock(&bytes);
        (std::mem::take(&mut b.out), b.too_large)
    };
    let ok = status.is_some_and(|st| st.success());
    let text = String::from_utf8_lossy(&out);
    let silent = parse.is_some() && text.chars().all(char::is_whitespace);
    let table = match parse {
        Some(read) if ok && !too_large && !stopped && !silent => Some(read(&text)),
        _ => None,
    };
    let mut i = s.lock();
    i.outcome = Some(Outcome {
        status,
        error: None,
        stderr,
        too_large,
        stopped: stopped && !too_large,
        table,
        silent,
    });
    i.done = true;
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A run's stdout, kept up to the cap; past it the command is stopped and the rest dropped.
fn read_bytes(mut out: impl Read, bytes: &Mutex<Bytes>, s: &Arc<Shared>) {
    let mut chunk = vec![0; 64 * 1024];
    loop {
        match out.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                let mut b = lock(bytes);
                if b.out.len() + n > MAX_OUTPUT {
                    b.too_large = true;
                    drop(b);
                    s.stop();
                    return;
                }
                b.out.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// A stream's stdout, a line at a time into its table, as many lines as came at once under one
/// lock. A header that can't be one stops the command.
fn read_lines(out: impl Read, grow: &Mutex<Grow>, s: &Arc<Shared>) {
    let mut r = BufReader::with_capacity(64 * 1024, out);
    let mut line: Vec<u8> = Vec::new();
    // Each line whole, None for one longer than a row may be.
    let mut lines: Vec<Option<String>> = Vec::new();
    // Past MAX_LINE without a break: the rest of that line is dropped, and it counts as one.
    let mut long = false;
    loop {
        let buf = match r.fill_buf() {
            Ok([]) => break,
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let n = buf.len();
        let mut rest = buf;
        while let Some(at) = rest.iter().position(|&b| b == b'\n') {
            if !long {
                line.extend_from_slice(&rest[..at]);
            }
            lines.push(take_line(&mut line, &mut long));
            rest = &rest[at + 1..];
        }
        if !long {
            line.extend_from_slice(rest);
            if line.len() > MAX_LINE {
                (long, line) = (true, Vec::new());
            }
        }
        r.consume(n);
        if !feed(grow, &mut lines) {
            s.stop();
            return;
        }
    }
    if !line.is_empty() || long {
        lines.push(take_line(&mut line, &mut long));
    }
    if !feed(grow, &mut lines) {
        s.stop();
    }
}

/// The line so far as text; None for one too long to be a row.
fn take_line(line: &mut Vec<u8>, long: &mut bool) -> Option<String> {
    let text = match std::mem::take(long) {
        true => None,
        false => Some(String::from_utf8_lossy(line).into_owned()),
    };
    line.clear();
    text
}

/// Lines into the table: false once it can't read them.
fn feed(grow: &Mutex<Grow>, lines: &mut Vec<Option<String>>) -> bool {
    if lines.is_empty() {
        return true;
    }
    let mut g = lock(grow);
    for l in lines.drain(..) {
        let read = match l {
            Some(l) => g.line(&l),
            None => g.too_long(),
        };
        if read.is_err() {
            return false;
        }
    }
    true
}

/// The last lines of a command's stderr, each cut to a width.
#[derive(Default)]
struct Tail {
    lines: std::collections::VecDeque<String>,
}

impl Tail {
    fn push(&mut self, l: &str) {
        let l = l.trim_end();
        if l.trim().is_empty() {
            return;
        }
        let mut s: String = l.chars().take(STDERR_WIDTH).collect();
        if s.len() < l.len() {
            s.push('…');
        }
        self.lines.push_back(s);
        if self.lines.len() > STDERR_LINES {
            self.lines.pop_front();
        }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

fn read_tail(err: impl Read, tail: &Mutex<Tail>) {
    let mut r = BufReader::new(err);
    let mut line = Vec::new();
    loop {
        line.clear();
        // A line at most so long: the rest of a longer one comes as the next.
        match r.by_ref().take(64 * 1024).read_until(b'\n', &mut line) {
            Ok(0) => return,
            Ok(_) => lock(tail).push(&String::from_utf8_lossy(&line)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// Once the command has ended: whatever it left in its group gets SIGTERM, and SIGKILL after
/// the grace if it's still there.
fn sweep(pgid: u32) {
    if !alive(pgid) {
        return;
    }
    signal(pgid, "TERM");
    let look = Duration::from_millis(20);
    for _ in 0..(KILL_GRACE.as_millis() / look.as_millis()).max(1) {
        std::thread::sleep(look);
        if !alive(pgid) {
            return;
        }
    }
    signal(pgid, "KILL");
}

/// Send `sig` to the group `pgid`.
fn signal(pgid: u32, sig: &str) {
    let _ = Command::new("/bin/kill")
        .args(["-s", sig, "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Whether anything is left in the group `pgid`.
fn alive(pgid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-s", "0", "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// How many [`LOOK`]s make `d`.
fn steps(d: Duration) -> u128 {
    (d.as_millis() / LOOK.as_millis()).max(1)
}
