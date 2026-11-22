//! Live data (contract-p4 §4): commands run (`{"run": cmd, "every": "30s"}`) and streamed
//! (`{"stream": cmd}`), the yeses they need first (ruling 13, [`trust`]), and follow mode
//! ([`follow`]), as pure code the editor drives.
//!
//! The editor keeps a [`Live`] per figure and drives it from its own timer:
//!
//! - [`Live::set`] says which commands the figure wants now, each by its [`LiveKey`] (the spec
//!   that names it and the command line) and its [`LiveSpec`] (the command, its directory, and
//!   how to read what it prints): the new start, the ones no longer wanted stop, and one whose
//!   reading changed starts again. [`Live::start`] and [`Live::stop`] do one;
//! - [`Live::poll`], on the editor's timer, says what came, in order ([`Event`]): a run begun,
//!   a run's whole output or a stream's rows so far as a table, a failure, a stream's end;
//! - [`Live::next_poll`] says when to poll next: soon while a stream is open or a run is going,
//!   at the next run's tick otherwise, and None when nothing is due (no timer at rest);
//! - [`Live::rerun`] is `R`; [`Live::status`] is what the status line and a dashboard's live
//!   status say of a source.
//!
//! **Runs.** `/bin/sh -c cmd` in the spec's directory, stdin null, the environment inherited
//! (the PATH your login shell sets with [`PathFrom::Login`], as DIE's other commands have it).
//! Its stdout is the data, read as a file would be (CSV, TSV, JSON or JSON lines, by the
//! spec's format or by sniffing). It runs when started, each `every` on a grid from then, and
//! on `R`; one at a time: a tick while it runs is skipped, and `R` says it's still running.
//! Output past [`MAX_OUTPUT`] stops it, unreadable; a non-zero exit is unreadable, the log
//! having the status and the last [`STDERR_LINES`] lines of stderr.
//!
//! **Streams.** Each stdout line is a row, grown into a table as it comes ([`grow`]); the rows
//! are folded into a new table at most [`FOLDS`] times a second, the newest [`MAX_ROWS`]
//! kept. When the command ends the data stays and [`Event::Ended`] says how; `R` starts it
//! again.
//!
//! **Lifetimes.** Every command runs in a process group of its own, started, read and waited
//! for on threads it owns, never on the main thread. Stopping one (and dropping a `Live` stops
//! all of its) sends the group SIGTERM, then SIGKILL after [`KILL_GRACE`] if the command is
//! still there; it's always waited for, so no zombie is left, and what it left in its group
//! when it ended goes the same way. [`Stopped::wait`] waits for them to be gone (DIE's quit).
//!
//! Time is the caller's: every call that needs it is given `now` (the editor's clock), so the
//! fold's pace and the runs' ticks are the same in a test as on screen.

pub mod follow;
pub mod grow;
mod proc;
pub mod trust;

pub use follow::{Follow, FollowChange, at_end, follow, keep_cursor};
pub use grow::Grow;
pub use trust::{Answer, Trust, not_run};

use crate::data::{ReadOptions, Table};
use crate::error::{ErrorKind, FigError};
use crate::source::{format_label, lexical, read_data, sniff};
use crate::spec::chart::with_hints;
use crate::spec::{DataRef, DataSpec};
use jiff::tz::TimeZone;
use proc::{Outcome, Proc, Sink};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The most a run may print before it's stopped, unreadable: 64 MB.
pub const MAX_OUTPUT: usize = 64 * 1024 * 1024;
/// The most rows a stream keeps; older ones drop from the front.
pub const MAX_ROWS: usize = 200_000;
/// How many times a second, at most, a stream's rows are folded into a new table.
pub const FOLDS: u32 = 30;
/// The least time between two folds of a stream: a thirtieth of a second.
pub const FOLD_EVERY: Duration = Duration::from_nanos(1_000_000_000 / FOLDS as u64);
/// How long a command has after SIGTERM before SIGKILL.
pub const KILL_GRACE: Duration = Duration::from_secs(1);
/// How many of stderr's last lines a failure's log keeps.
pub const STDERR_LINES: usize = 20;

/// Which live source: the spec that names it (its absolute path; None for one with no file)
/// and the exact command line. The trust store keeps yeses by the same two.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LiveKey {
    /// The spec's path, `.` and `..` taken out.
    pub spec: Option<PathBuf>,
    /// The command line.
    pub cmd: String,
}

impl LiveKey {
    /// The key of `cmd` named by the spec at `spec`.
    pub fn new(spec: Option<&Path>, cmd: &str) -> LiveKey {
        LiveKey {
            spec: spec.map(lexical),
            cmd: cmd.to_string(),
        }
    }
}

/// What to run and how to read it: a data spec whose source is a `run` or a `stream`, and the
/// directory of the spec that names it, where it runs.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveSpec {
    /// The data spec: its source a [`DataRef::Run`] or [`DataRef::Stream`], with the format
    /// and reading hints for what it prints.
    pub data: DataSpec,
    /// Where it runs.
    pub dir: PathBuf,
}

impl LiveSpec {
    /// The live spec of `data` in `dir`; None unless its source is a command's.
    pub fn of(data: &DataSpec, dir: &Path) -> Option<LiveSpec> {
        matches!(data.source, DataRef::Run { .. } | DataRef::Stream { .. }).then(|| LiveSpec {
            data: data.clone(),
            dir: dir.to_path_buf(),
        })
    }

    /// The command line.
    pub fn cmd(&self) -> &str {
        match &self.data.source {
            DataRef::Run { cmd, .. } | DataRef::Stream { cmd } => cmd,
            _ => "",
        }
    }

    /// How often a run runs again, if it does.
    pub fn every(&self) -> Option<Duration> {
        match &self.data.source {
            DataRef::Run { every, .. } => *every,
            _ => None,
        }
    }

    /// Whether it's streamed rather than run.
    pub fn is_stream(&self) -> bool {
        matches!(self.data.source, DataRef::Stream { .. })
    }

    /// How what it prints is read in `zone`: the spec's hints over the defaults.
    pub fn options(&self, zone: &TimeZone) -> ReadOptions {
        with_hints(&ReadOptions::new(zone.clone()), &self.data.read)
    }

    /// What the log says was done: ``ran `./errors.sh` in /w/dash``.
    fn ran(&self) -> String {
        let verb = if self.is_stream() { "streamed" } else { "ran" };
        format!("{verb} `{}` in {}", self.cmd(), self.dir.display())
    }
}

/// Where a command's PATH comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathFrom {
    /// The PATH your login shell sets, as DIE's other commands get it (die-core's
    /// `process::path`): DIE.app's own is launchd's.
    Login,
    /// DIE's own environment as it is (tests).
    Inherited,
}

/// What came of the live sources, in the order it came.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A run's tick began a run: the header says `updating…` until it's done, and nothing
    /// dims.
    Began(LiveKey),
    /// New data: a run's whole output, or a stream's rows so far (the newest [`MAX_ROWS`]).
    Data {
        /// Whose.
        key: LiveKey,
        /// The table.
        table: Arc<Table>,
    },
    /// A run that failed (a figure that drew keeps its data, with the stale badge), or a
    /// stream that couldn't start or whose output can't be read.
    Failed {
        /// Whose.
        key: LiveKey,
        /// What went wrong: unreadable, or malformed, with what was tried.
        error: FigError,
    },
    /// A stream's command ended by itself; the data stays and `R` starts it again.
    Ended {
        /// Whose.
        key: LiveKey,
        /// How: `exit 0`, `exit 3`, `signal 9`, as `stream ended (exit 0)` says it.
        how: String,
        /// When it ended badly: unreadable, with its status and the end of its stderr.
        error: Option<FigError>,
    },
}

/// What a live source is doing, for the status line and a dashboard's live status.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Status {
    /// Whether it's a stream.
    pub stream: bool,
    /// A run going, or a stream's command still there.
    pub running: bool,
    /// The command's pid (its process group's id) while it runs.
    pub pid: Option<u32>,
    /// How many times it was started: runs, or a stream's starts.
    pub runs: u32,
    /// Ticks skipped because a run was still going.
    pub skipped: u32,
    /// When data last came.
    pub updated: Option<Duration>,
    /// The last failure and when, until data comes again.
    pub error: Option<(Duration, FigError)>,
    /// How a stream's command ended, once it has.
    pub ended: Option<String>,
    /// The rows a stream keeps.
    pub rows: usize,
    /// The rows a stream dropped from the front, in all.
    pub dropped: usize,
    /// The lines a stream skipped as no row, and why the first was.
    pub bad: (usize, Option<FigError>),
}

impl Status {
    /// What the status line says of it: `updating…`, `keeping the newest 200,000 rows`,
    /// `stream ended (exit 0)`.
    pub fn notes(&self) -> Vec<String> {
        let mut out = vec![];
        if self.running && !self.stream {
            out.push("updating…".to_string());
        }
        if self.dropped > 0 {
            let n = crate::scale::format_value(MAX_ROWS as f64);
            out.push(format!("keeping the newest {n} rows"));
        }
        if let Some(how) = &self.ended {
            out.push(format!("stream ended ({how})"));
        }
        out
    }
}

/// What `R` did to a source.
#[derive(Clone, Debug, PartialEq)]
pub enum Rerun {
    /// Ran it now, or started the stream again.
    Started(LiveKey),
    /// Its run is still going: nothing started.
    StillRunning(LiveKey),
    /// Its stream is still open: nothing to do.
    Streaming(LiveKey),
}

impl Rerun {
    /// What `*messages*` says, if anything: `fig: ./errors.sh is still running`.
    pub fn message(&self) -> Option<String> {
        match self {
            Rerun::StillRunning(k) => Some(format!("fig: {} is still running", k.cmd)),
            _ => None,
        }
    }
}

/// Commands stopped, to wait on until they're gone.
#[derive(Default)]
pub struct Stopped {
    procs: Vec<Arc<proc::Shared>>,
}

impl Stopped {
    /// These and those.
    pub fn join(&mut self, other: Stopped) {
        self.procs.extend(other.procs);
    }

    /// Wait up to `timeout` for every one to be gone: waited for, nothing left in its group.
    /// True if they all are.
    pub fn wait(&self, timeout: Duration) -> bool {
        let step = Duration::from_millis(5);
        for _ in 0..=(timeout.as_millis() / step.as_millis()) {
            if self.procs.iter().all(|p| proc::gone(p)) {
                return true;
            }
            std::thread::sleep(step);
        }
        self.procs.iter().all(|p| proc::gone(p))
    }
}

/// A figure's live sources.
pub struct Live {
    zone: TimeZone,
    path: PathFrom,
    entries: Vec<Entry>,
    /// What came before a poll could say it: a run that ended as `R` started the next.
    pending: Vec<Event>,
}

struct Entry {
    key: LiveKey,
    spec: LiveSpec,
    state: State,
}

enum State {
    Run(Run),
    Stream(Stream),
}

struct Run {
    proc: Option<Proc>,
    /// Where the ticks' grid starts.
    origin: Duration,
    due: Option<Duration>,
    runs: u32,
    skipped: u32,
    updated: Option<Duration>,
    error: Option<(Duration, FigError)>,
}

struct Stream {
    proc: Option<Proc>,
    grow: Arc<Mutex<Grow>>,
    last_fold: Option<Duration>,
    runs: u32,
    updated: Option<Duration>,
    error: Option<(Duration, FigError)>,
    ended: Option<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Live {
    /// No sources yet: times read in `zone`, commands' PATH from `path`.
    pub fn new(zone: TimeZone, path: PathFrom) -> Live {
        Live {
            zone,
            path,
            entries: vec![],
            pending: vec![],
        }
    }

    /// Start one source now: a run runs (and again each `every`), a stream opens. False if
    /// `key` is already started.
    pub fn start(&mut self, key: LiveKey, spec: LiveSpec, now: Duration) -> bool {
        if self.entries.iter().any(|e| e.key == key) {
            return false;
        }
        let state = match spec.is_stream() {
            true => {
                let opt = spec.options(&self.zone);
                let grow = Grow::new(opt, spec.data.format, MAX_ROWS, spec.cmd());
                State::Stream(Stream {
                    proc: None,
                    grow: Arc::new(Mutex::new(grow)),
                    last_fold: None,
                    runs: 0,
                    updated: None,
                    error: None,
                    ended: None,
                })
            }
            false => State::Run(Run {
                proc: None,
                origin: now,
                due: spec.every().map(|e| now + e),
                runs: 0,
                skipped: 0,
                updated: None,
                error: None,
            }),
        };
        let mut e = Entry { key, spec, state };
        e.begin(&self.zone, self.path);
        self.entries.push(e);
        true
    }

    /// The sources wanted now: each new one starts, each no longer wanted stops, and one whose
    /// reading changed starts again (a run whose `every` or reading changed keeps going, from
    /// its next run). The keys started, in order.
    pub fn set(&mut self, wants: &[(LiveKey, LiveSpec)], now: Duration) -> Vec<LiveKey> {
        let mut kept = vec![];
        for mut e in std::mem::take(&mut self.entries) {
            match wants.iter().find(|(k, _)| *k == e.key) {
                Some((_, s)) if e.keeps(s) => {
                    e.update(s, now);
                    kept.push(e);
                }
                // Dropped: its command is stopped.
                _ => drop(e),
            }
        }
        self.entries = kept;
        let mut started = vec![];
        for (k, s) in wants {
            if self.start(k.clone(), s.clone(), now) {
                started.push(k.clone());
            }
        }
        started
    }

    /// Stop one source, its command killed (SIGTERM, then SIGKILL after the grace): what to
    /// wait on for it to be gone.
    pub fn stop(&mut self, key: &LiveKey) -> Stopped {
        let (gone, kept): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|e| &e.key == key);
        self.entries = kept;
        stopped(gone)
    }

    /// Stop every source (the figure's last window closed, or DIE quits).
    pub fn stop_all(&mut self) -> Stopped {
        stopped(std::mem::take(&mut self.entries))
    }

    /// `R`: run each run source now (or only `which`), and start each stream that ended again.
    /// A run still going isn't run twice ([`Rerun::StillRunning`]).
    pub fn rerun(&mut self, which: Option<&LiveKey>, now: Duration) -> Vec<Rerun> {
        let (zone, path) = (self.zone.clone(), self.path);
        let mut out = vec![];
        for e in &mut self.entries {
            if which.is_some_and(|k| k != &e.key) {
                continue;
            }
            // An end no poll has said yet is said first.
            e.collect(now, &mut self.pending);
            let key = e.key.clone();
            // A stream whose command has ended is started again, even with its last rows
            // still to fold (they stay in its table).
            let busy = match &e.state {
                State::Run(r) => r.proc.is_some(),
                State::Stream(s) => s.proc.as_ref().is_some_and(|p| !p.done()),
            };
            out.push(match (busy, e.spec.is_stream()) {
                (true, false) => Rerun::StillRunning(key),
                (true, true) => Rerun::Streaming(key),
                (false, _) => {
                    if let State::Stream(s) = &mut e.state {
                        lock(&s.grow).restart();
                        s.ended = None;
                    }
                    e.begin(&zone, path);
                    Rerun::Started(key)
                }
            });
        }
        out
    }

    /// What came since the last poll, in order: runs that ended and began, streams' rows
    /// folded (at most [`FOLDS`] times a second each), streams that ended.
    pub fn poll(&mut self, now: Duration) -> Vec<Event> {
        let mut out = std::mem::take(&mut self.pending);
        let (zone, path) = (self.zone.clone(), self.path);
        for e in &mut self.entries {
            e.collect(now, &mut out);
            // A run's tick: run it, or skip the tick while the last run is still going.
            let begin = match &mut e.state {
                State::Run(r) if r.due.is_some_and(|d| now >= d) => {
                    r.due = e.spec.every().map(|every| tick(r.origin, every, now));
                    let busy = r.proc.is_some();
                    r.skipped += u32::from(busy);
                    !busy
                }
                _ => false,
            };
            if begin {
                e.begin(&zone, path);
                out.push(Event::Began(e.key.clone()));
            }
        }
        out
    }

    /// When to poll next: a fold's pace after now while a stream is open or a run is going,
    /// else the next run's tick; None when nothing is due.
    pub fn next_poll(&self, now: Duration) -> Option<Duration> {
        let mut next: Option<Duration> = (!self.pending.is_empty()).then_some(now);
        let mut at = |t: Duration| next = Some(next.map_or(t, |n| n.min(t)));
        for e in &self.entries {
            match &e.state {
                State::Run(r) => {
                    if r.proc.is_some() {
                        at(now + FOLD_EVERY);
                    }
                    if let Some(d) = r.due {
                        at(d.max(now));
                    }
                }
                State::Stream(s) => {
                    if lock(&s.grow).fresh() {
                        at(s.last_fold.map_or(now, |t| (t + FOLD_EVERY).max(now)));
                    } else if s.proc.is_some() {
                        at(now + FOLD_EVERY);
                    }
                }
            }
        }
        next
    }

    /// What a source is doing; None for one not started.
    pub fn status(&self, key: &LiveKey) -> Option<Status> {
        let e = self.entries.iter().find(|e| &e.key == key)?;
        Some(match &e.state {
            State::Run(r) => Status {
                stream: false,
                running: r.proc.is_some(),
                pid: r.proc.as_ref().and_then(Proc::pid),
                runs: r.runs,
                skipped: r.skipped,
                updated: r.updated,
                error: r.error.clone(),
                ..Status::default()
            },
            State::Stream(s) => {
                let g = lock(&s.grow);
                let (n, first) = g.bad();
                Status {
                    stream: true,
                    running: s.proc.is_some(),
                    pid: s.proc.as_ref().and_then(Proc::pid),
                    runs: s.runs,
                    updated: s.updated,
                    error: s.error.clone(),
                    ended: s.ended.clone(),
                    rows: g.rows(),
                    dropped: g.dropped(),
                    bad: (n, first.cloned()),
                    ..Status::default()
                }
            }
        })
    }

    /// The sources started, in the order they were.
    pub fn keys(&self) -> Vec<LiveKey> {
        self.entries.iter().map(|e| e.key.clone()).collect()
    }

    /// Whether no source is started.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What to wait on for these entries' commands, which go as the entries do.
fn stopped(entries: Vec<Entry>) -> Stopped {
    let mut out = Stopped::default();
    for e in entries {
        let p = match &e.state {
            State::Run(r) => r.proc.as_ref(),
            State::Stream(s) => s.proc.as_ref(),
        };
        out.procs.extend(p.map(Proc::handle));
        // Dropping it stops its command.
        drop(e);
    }
    out
}

/// The first tick of the grid from `origin` every `every` that's after `now`.
fn tick(origin: Duration, every: Duration, now: Duration) -> Duration {
    let every = every.max(Duration::from_millis(1));
    let n = now.saturating_sub(origin).as_nanos() / every.as_nanos();
    let at = origin.as_nanos() + every.as_nanos() * (n + 1);
    Duration::from_nanos(u64::try_from(at).unwrap_or(u64::MAX))
}

impl Entry {
    /// Its command started now.
    fn begin(&mut self, zone: &TimeZone, path: PathFrom) {
        let (cmd, dir) = (self.spec.cmd().to_string(), self.spec.dir.clone());
        match &mut self.state {
            State::Run(r) => {
                let (data, zone) = (self.spec.data.clone(), zone.clone());
                let read = move |text: &str| {
                    read_data(text, &data, "", &ReadOptions::new(zone)).map_err(|e| {
                        let format = format_label(data.format.unwrap_or_else(|| sniff(text)));
                        e.note(format!("read what it printed as {format}"))
                    })
                };
                r.proc = Some(Proc::start(cmd, dir, path, Sink::Run(Box::new(read))));
                r.runs += 1;
            }
            State::Stream(s) => {
                s.proc = Some(Proc::start(cmd, dir, path, Sink::Stream(s.grow.clone())));
                s.runs += 1;
            }
        }
    }

    /// Whether it can go on as `spec` without starting again: a run always can (its next run
    /// reads as `spec` says); a stream only if it reads the same, in the same place.
    fn keeps(&self, spec: &LiveSpec) -> bool {
        self.spec.dir == spec.dir
            && self.spec.is_stream() == spec.is_stream()
            && (!spec.is_stream() || self.spec.data == spec.data)
    }

    /// Go on as `spec`: a run whose `every` changed ticks from now.
    fn update(&mut self, spec: &LiveSpec, now: Duration) {
        if let State::Run(r) = &mut self.state
            && spec.every() != self.spec.every()
        {
            r.origin = now;
            r.due = spec.every().map(|e| now + e);
        }
        self.spec = spec.clone();
    }

    /// What came of its command since the last look: a run's end, a stream's rows (at the
    /// fold's pace) and its end once its last rows are out.
    fn collect(&mut self, now: Duration, out: &mut Vec<Event>) {
        let key = &self.key;
        match &mut self.state {
            State::Run(r) => {
                if !r.proc.as_ref().is_some_and(Proc::done) {
                    return;
                }
                let o = r.proc.take().and_then(|p| p.take());
                match ran(&self.spec, o) {
                    Ok(t) => {
                        (r.updated, r.error) = (Some(now), None);
                        out.push(Event::Data {
                            key: key.clone(),
                            table: Arc::new(t),
                        });
                    }
                    Err(error) => {
                        r.error = Some((now, error.clone()));
                        out.push(Event::Failed {
                            key: key.clone(),
                            error,
                        });
                    }
                }
            }
            State::Stream(s) => {
                let done = s.proc.as_ref().is_some_and(Proc::done);
                let mut g = lock(&s.grow);
                let failed = g.failed().cloned();
                // A stream that ends having printed no row folds once all the same (an empty
                // table), so its figure can say there's nothing to draw.
                let fold = g.fresh() || done && s.last_fold.is_none() && failed.is_none();
                let paced = s.last_fold.is_none_or(|t| now >= t + FOLD_EVERY);
                if fold && paced {
                    let table = Arc::new(g.table());
                    (s.last_fold, s.updated, s.error) = (Some(now), Some(now), None);
                    out.push(Event::Data {
                        key: key.clone(),
                        table,
                    });
                }
                // Its end, once its last rows are out.
                let out_yet = !g.fresh() && (s.last_fold.is_some() || failed.is_some());
                drop(g);
                if !done || !out_yet {
                    return;
                }
                let o = s.proc.take().and_then(|p| p.take());
                match streamed(&self.spec, o, failed) {
                    Ok((how, error)) => {
                        if let Some(e) = &error {
                            s.error = Some((now, e.clone()));
                        }
                        s.ended = Some(how.clone());
                        out.push(Event::Ended {
                            key: key.clone(),
                            how,
                            error,
                        });
                    }
                    Err(error) => {
                        s.error = Some((now, error.clone()));
                        out.push(Event::Failed {
                            key: key.clone(),
                            error,
                        });
                    }
                }
            }
        }
    }
}

/// How a command ended, as `stream ended (…)` says it: `exit 0`, `signal 9`.
fn how(status: &ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(n), _) => format!("exit {n}"),
        (None, Some(s)) => format!("signal {s}"),
        (None, None) => "ended".into(),
    }
}

/// How a command failed, after its name: `exited with status 3`, `was stopped by signal 9`.
fn failed_how(status: &ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(n), _) => format!("exited with status {n}"),
        (None, Some(s)) => format!("was stopped by signal {s}"),
        (None, None) => "failed".into(),
    }
}

fn unreadable(msg: String) -> FigError {
    FigError::new(msg).with_kind(ErrorKind::Unreadable)
}

/// The end of its stderr, a line each.
fn with_stderr(mut e: FigError, stderr: &[String]) -> FigError {
    for l in stderr {
        e = e.note(format!("stderr: {l}"));
    }
    e
}

/// A run's outcome as its table, or why not.
fn ran(spec: &LiveSpec, o: Option<Outcome>) -> Result<Table, FigError> {
    let (cmd, done) = (spec.cmd(), spec.ran());
    let Some(o) = o else {
        return Err(unreadable(format!("{cmd} ended without a word")).note(done));
    };
    if let Some(e) = &o.error {
        return Err(unreadable(format!("couldn't run {cmd}: {e}")).note(done));
    }
    if o.too_large {
        let mb = MAX_OUTPUT >> 20;
        let e = unreadable(format!("{cmd} printed more than {mb} MB"));
        return Err(e.note(format!("{done}: stopped at {mb} MB")));
    }
    let status = o.status.as_ref();
    if let Some(st) = status.filter(|s| !s.success()) {
        let e = unreadable(format!("{cmd} {}", failed_how(st)));
        return Err(with_stderr(
            e.note(format!("{done}: {}", how(st))),
            &o.stderr,
        ));
    }
    if o.silent {
        let e = FigError::new(format!("{cmd} printed nothing")).with_kind(ErrorKind::Empty);
        return Err(with_stderr(e.note(format!("{done}: exit 0")), &o.stderr));
    }
    match o.table {
        Some(Ok(t)) => Ok(t),
        Some(Err(e)) => {
            // Its place is in what the command printed, not in the spec: said, not pointed at.
            let e = match e.at {
                Some(at) => FigError {
                    msg: format!(
                        "line {}, column {} of what {cmd} printed: {}",
                        at.line, at.col, e.msg
                    ),
                    at: None,
                    ..e
                },
                None => e,
            };
            Err(e.note(done))
        }
        None => Err(unreadable(format!("{cmd} was stopped")).note(done)),
    }
}

/// A stream's end: how, and its error if it ended badly; or why it couldn't be read at all.
fn streamed(
    spec: &LiveSpec,
    o: Option<Outcome>,
    failed: Option<FigError>,
) -> Result<(String, Option<FigError>), FigError> {
    let (cmd, done) = (spec.cmd(), spec.ran());
    if let Some(e) = failed {
        let stderr = o.as_ref().map_or(&[][..], |o| &o.stderr[..]);
        return Err(with_stderr(e.note(done), stderr));
    }
    let Some(o) = o else {
        return Err(unreadable(format!("{cmd} ended without a word")).note(done));
    };
    if let Some(e) = &o.error {
        return Err(unreadable(format!("couldn't run {cmd}: {e}")).note(done));
    }
    let Some(st) = o.status else {
        return Ok(("ended".into(), None));
    };
    let error = (!st.success() && !o.stopped).then(|| {
        let e = unreadable(format!("{cmd} {}", failed_how(&st)));
        with_stderr(e.note(format!("{done}: {}", how(&st))), &o.stderr)
    });
    Ok((how(&st), error))
}
