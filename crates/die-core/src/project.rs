//! Finding files under a root: [`walk`] lists them, and a finder prompt ranks them while it does.
//! `find-file-home` and `find-file-project` are the owner's fzf over `find .`.
//!
//! Three threads make a finder, so a key never waits on the disk or on half a million paths:
//! - The walker runs [`walk`], or a test's own source, until the prompt closes.
//! - The ranker holds the paths, flex matches them against the newest text, and leaves the best
//!   [`KEEP`] on a board. A new text stops the scan it interrupts.
//! - The main thread sends the text on each key and takes what's on the board at the top of each
//!   turn (`Ctx::step`). Only `<CR>` and `<Tab>` wait for the ranker (up to [`SETTLE`]), because
//!   they act on the list, and it should be the list for what's on the line.
#![warn(missing_docs)]
mod grep;
pub(crate) use grep::{GREP, Grep, parse_row};

use crate::commands::{line_for, open_in_window};
use crate::ext::{Ctx, Registry};
use crate::prompt::{PromptSpec, flex};
use crate::rt::Waker;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Directories no walk goes into, by name, wherever they are. `.cargo/registry` and
/// `.claude/worktrees`, which holds whole other checkouts of a project, are in [`skips`].
const SKIP: [&str; 7] = [
    ".git",
    "target",
    "node_modules",
    ".Trash",
    "Library",
    ".cache",
    ".rustup",
];
/// A batch goes out at this size, or once this long has passed since the last, between directories.
const BATCH: usize = 4096;
const BATCH_EVERY: Duration = Duration::from_millis(20);
/// How many matches reach the main thread. The count shows them all, and paging stops here.
const KEEP: usize = 1000;
/// How many paths the ranker scores before it looks for a newer text.
const SLICE: usize = 4096;
/// How often a board changes while paths arrive or a long scan runs.
const PUBLISH_EVERY: Duration = Duration::from_millis(33);
/// The longest `<CR>` waits for the ranker. It takes this long only if the ranker has died or
/// the machine has no time to give it, and then `<CR>` does nothing.
const SETTLE: Duration = Duration::from_secs(2);

/// What feeds a finder, on the walker's thread: it stops when the flag is set, and hands over
/// batches of paths relative to the root.
pub type PathSource = Box<dyn FnOnce(&AtomicBool, &mut dyn FnMut(Vec<String>)) + Send>;

fn skips(parent: &str, name: &str) -> bool {
    let dir = parent.trim_end_matches('/').rsplit('/').next();
    SKIP.contains(&name)
        || dir == Some(".cargo") && name == "registry"
        || dir == Some(".claude") && name == "worktrees"
}

/// Every file under `root`, in batches, as paths relative to it. Shallowest first, and by name
/// within a directory. Hidden files count. It stays out of [`SKIP`] and doesn't follow a symlink
/// to a directory, though a symlink to a file is a file. A path that isn't UTF-8 can't go on a
/// line, and a socket or a pipe can't be opened as text, so neither is listed. An unreadable
/// directory is passed over. It looks at `stop` before each directory.
pub fn walk(root: &Path, stop: &AtomicBool, sink: &mut dyn FnMut(Vec<String>)) {
    let mut dirs = VecDeque::from([String::new()]);
    let mut batch = Vec::new();
    let mut sent = Instant::now();
    while let Some(rel) = dirs.pop_front() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let dir = root.join(&rel);
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let named = |e: std::fs::DirEntry| Some((e.file_name().into_string().ok()?, e));
        let mut entries: Vec<_> = read.flatten().filter_map(named).collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for (name, entry) in entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !skips(&rel, &name) {
                    dirs.push_back(format!("{rel}{name}/"));
                }
            } else if name == ".git" {
                // A linked worktree's or a submodule's `.git` is a file, and no more worth opening.
            } else if kind.is_file() || kind.is_symlink() && dir.join(&name).is_file() {
                batch.push(format!("{rel}{name}"));
            }
        }
        if batch.len() >= BATCH || sent.elapsed() >= BATCH_EVERY && !batch.is_empty() {
            sink(std::mem::take(&mut batch));
            sent = Instant::now();
        }
    }
    if !batch.is_empty() {
        sink(batch);
    }
}

/// Where `find-file-project` walks from: the nearest of `dir` and its ancestors with a `.git`,
/// which is a file in a linked worktree, or else `dir`.
pub fn project_root(dir: &Path) -> PathBuf {
    let found = dir.ancestors().find(|d| d.join(".git").exists());
    found.unwrap_or(dir).to_path_buf()
}

enum Msg {
    Ask(u64, String),
    Batch(Vec<String>),
    WalkDone,
    Stop,
}

/// What the ranker last left for the main thread.
#[derive(Default)]
pub(crate) struct Ranked {
    pub(crate) items: Vec<String>,
    /// How many paths match, which can be more than `items`.
    pub(crate) total: usize,
    /// Which text this answers.
    pub(crate) asked: u64,
    /// All the paths so far were scored for that text.
    settled: bool,
    pub(crate) walking: bool,
}

#[derive(Default)]
struct Board {
    ranked: Mutex<Ranked>,
    changed: Condvar,
    /// `ranked` hasn't been taken, so a turn with no news costs one atomic.
    fresh: AtomicBool,
}

impl Board {
    fn lock(&self) -> MutexGuard<'_, Ranked> {
        self.ranked.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The main thread's end of a finder. Dropping it, as closing the prompt does, stops the threads.
pub(crate) struct Feed {
    tx: Sender<Msg>,
    board: Arc<Board>,
    stop: Arc<AtomicBool>,
    asked: u64,
}

impl Feed {
    fn start(source: PathSource, waker: Waker) -> Feed {
        let (tx, rx) = mpsc::channel();
        let board = Arc::new(Board::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (walk_tx, walk_stop) = (DoneOnDrop(tx.clone()), stop.clone());
        std::thread::spawn(move || {
            source(&walk_stop, &mut |batch| {
                let _ = walk_tx.0.send(Msg::Batch(batch));
            });
        });
        let rank_board = board.clone();
        std::thread::spawn(move || rank(&rx, &rank_board, &waker));
        Feed {
            tx,
            board,
            stop,
            asked: 0,
        }
    }
    /// The line is now this.
    pub(crate) fn ask(&mut self, text: &str) {
        self.asked += 1;
        let _ = self.tx.send(Msg::Ask(self.asked, text.to_string()));
    }
    /// The board answers the line as it is.
    pub(crate) fn settled(&self) -> bool {
        let ranked = self.board.lock();
        ranked.asked == self.asked && ranked.settled
    }
    /// What's new on the board. With `settle`, first wait for the answer to the line as it is.
    pub(crate) fn take(&self, settle: bool) -> Option<Ranked> {
        if !settle && !self.board.fresh.load(Ordering::Acquire) {
            return None;
        }
        let mut ranked = self.board.lock();
        let start = Instant::now();
        while settle && !(ranked.asked == self.asked && ranked.settled) {
            let Some(left) = SETTLE.checked_sub(start.elapsed()) else {
                break;
            };
            let waited = self.board.changed.wait_timeout(ranked, left);
            ranked = waited.unwrap_or_else(|e| e.into_inner()).0;
        }
        let fresh = self.board.fresh.swap(false, Ordering::AcqRel);
        fresh.then(|| Ranked {
            items: std::mem::take(&mut ranked.items),
            total: ranked.total,
            asked: ranked.asked,
            settled: ranked.settled,
            walking: ranked.walking,
        })
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Stop);
    }
}

/// Tells the ranker the walk has ended, even if the source panicked.
struct DoneOnDrop(Sender<Msg>);

impl Drop for DoneOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(Msg::WalkDone);
    }
}

struct Hit {
    score: f64,
    in_case: bool,
    at: usize,
}

/// The ranker's state: the paths, and how far the newest text has got through them.
struct Scan {
    paths: Vec<String>,
    query: String,
    asked: u64,
    scanned: usize,
    hits: Vec<Hit>,
    total: usize,
    walking: bool,
    /// Something has changed since the last board.
    dirty: bool,
    /// The newest text has had a settled board.
    answered: bool,
}

impl Scan {
    fn caught_up(&self) -> bool {
        self.scanned >= self.paths.len()
    }
    /// Take a message in. False means stop.
    fn take(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Stop => return false,
            Msg::WalkDone => self.walking = false,
            Msg::Batch(batch) => self.paths.extend(batch),
            Msg::Ask(asked, query) => {
                self.answered = false;
                self.asked = asked;
                self.scanned = 0;
                self.total = 0;
                self.hits.clear();
                self.query = query;
            }
        }
        self.dirty = true;
        true
    }
    /// Score the next slice. With no query, every path matches, in the order it came.
    fn step(&mut self) {
        if self.query.is_empty() {
            self.hits.clear();
            self.total = self.paths.len();
            self.scanned = self.total;
            let first = 0..self.total.min(KEEP);
            self.hits.extend(first.map(|at| Hit {
                score: 0.0,
                in_case: true,
                at,
            }));
            return;
        }
        let end = self.paths.len().min(self.scanned + SLICE);
        for at in self.scanned..end {
            if let Some((score, in_case)) = flex::rate(&self.query, &self.paths[at]) {
                self.hits.push(Hit { score, in_case, at });
                self.total += 1;
            }
        }
        self.dirty |= end > self.scanned;
        self.scanned = end;
        if self.hits.len() >= 2 * KEEP {
            self.best();
        }
    }
    /// Cut the hits down to the best, in order.
    fn best(&mut self) {
        self.hits.sort_unstable_by(|a, b| {
            let by_score = b.score.total_cmp(&a.score);
            let by_case = by_score.then(b.in_case.cmp(&a.in_case));
            by_case.then(a.at.cmp(&b.at))
        });
        self.hits.truncate(KEEP);
    }
    fn publish(&mut self, board: &Board, waker: &Waker) {
        self.best();
        let settled = self.caught_up();
        let ranked = Ranked {
            items: self.hits.iter().map(|h| self.paths[h.at].clone()).collect(),
            total: self.total,
            asked: self.asked,
            settled,
            walking: self.walking,
        };
        self.dirty = false;
        self.answered = settled;
        // Dropped after the lock goes: the main thread can be waiting on it.
        let old = std::mem::replace(&mut *board.lock(), ranked);
        board.fresh.store(true, Ordering::Release);
        board.changed.notify_all();
        waker.wake();
        drop(old);
    }
}

fn rank(rx: &mpsc::Receiver<Msg>, board: &Board, waker: &Waker) {
    let mut s = Scan {
        paths: vec![],
        query: String::new(),
        asked: 0,
        scanned: 0,
        hits: vec![],
        total: 0,
        walking: true,
        dirty: false,
        answered: false,
    };
    let mut last = Instant::now();
    loop {
        if s.caught_up() {
            let left = PUBLISH_EVERY.saturating_sub(last.elapsed());
            if s.dirty && (!s.answered || left.is_zero()) {
                s.publish(board, waker);
                last = Instant::now();
            }
            let got = match s.dirty {
                true => rx.recv_timeout(left),
                false => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match got {
                Ok(msg) => {
                    if !s.take(msg) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => continue,
            }
        }
        while let Ok(msg) = rx.try_recv() {
            if !s.take(msg) {
                return;
            }
        }
        s.step();
        // A long scan shows what it has so far, unless that would blank the list.
        if !s.caught_up() && last.elapsed() >= PUBLISH_EVERY && !s.hits.is_empty() {
            s.publish(board, waker);
            last = Instant::now();
        }
    }
}

impl Ctx {
    /// A prompt that finds a file under `root` from what `source` sends, as `find-file-home` does
    /// with [`walk`]. Candidates come in while typing goes on, and the prompt shows `+` after the
    /// count until `source` returns. Closing the prompt sets the flag `source` was given. `<CR>`
    /// opens the file as `:e` would. A line that matches nothing stays on the line.
    pub fn find_under(&mut self, root: PathBuf, source: PathSource) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let label = format!("{}: ", line_for(&root, home.as_deref()));
        let feed = Feed::start(source, self.waker());
        let spec = PromptSpec::new(&label)
            .history("project-file")
            .require_match()
            .fed(feed);
        self.prompt(spec, move |ctx, answer| match answer {
            Some(path) if !path.is_empty() => open_in_window(ctx, &root.join(path)),
            _ => Ok(()),
        });
    }
    fn walk_for_file(&mut self, root: PathBuf) {
        let from = root.clone();
        let source = move |stop: &AtomicBool, sink: &mut dyn FnMut(Vec<String>)| {
            walk(&from, stop, sink);
        };
        self.find_under(root, Box::new(source));
    }
}

/// The directory a project command starts from: this buffer's, or where DIE started. An app
/// started from the Dock is in `/`, which is no project, so there it's the home directory.
pub(crate) fn here_dir(ctx: &Ctx) -> PathBuf {
    let here = ctx.focused_buffer().and_then(|b| ctx.directory(b));
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    let dir = here.map_or(cwd, Path::to_path_buf);
    match (dir.parent(), std::env::var_os("HOME")) {
        (None, Some(home)) => PathBuf::from(home),
        _ => dir,
    }
}

pub(crate) fn register_commands(reg: &mut Registry) {
    grep::register_commands(reg);
    crate::process::register_commands(reg);
    reg.command(
        "find-file-home",
        "Open a file found anywhere under the home directory, hidden ones included",
        |ctx, _| {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
            ctx.walk_for_file(home.unwrap_or(cwd));
            Ok(())
        },
    );
    reg.command(
        "find-file-project",
        "Open a file found anywhere under this buffer's project: the nearest directory with a .git",
        |ctx, _| {
            ctx.walk_for_file(project_root(&here_dir(ctx)));
            Ok(())
        },
    );
}
