//! `grep`: search the text of a project into a read-only `*grep*`, whose rows `next-error` and
//! `compile-goto-error` go to (`process::errors`). As with the finder, no key waits on the
//! disk: one thread walks and reads, and leaves rows on a board at most every
//! [`PUBLISH_EVERY`]. Each turn of the main thread takes what's there (`Ctx::step`).
//!
//! A row is one line of a file, at its first match, however many it holds, as Emacs's grep gives.
//! The buffer is the header, the rows in the order the walk finds them, and a last line, so a
//! row's number in the results is its line in the buffer. It isn't kept by the session: nothing
//! could go back to the run that made it.
use super::{PUBLISH_EVERY, here_dir, project_root, walk};
use crate::error::Result;
use crate::ext::{Ctx, Registry};
use crate::files::normalize;
use crate::ids::BufferId;
use crate::keymap::State;
use crate::process::Results;
use crate::prompt::PromptSpec;
use crate::rt::{Sub, Waker};
use crate::text::{Origin, Pattern};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) const GREP: &str = "*grep*";
/// The rows a search stops at.
const MAX_ROWS: usize = 10_000;
/// A file bigger than this isn't read.
const MAX_FILE: u64 = 10 * 1024 * 1024;
/// A NUL this near the start makes a file binary.
const BINARY_WITHIN: usize = 8 * 1024;
/// A row shows this many characters of its line, so a minified file can't make a huge buffer.
const MAX_TEXT: usize = 500;

/// Rows not yet in the buffer.
#[derive(Default)]
struct Rows {
    text: String,
    count: usize,
}

#[derive(Default)]
struct Board {
    rows: Mutex<Rows>,
    /// `rows` has something, so a turn with no news costs one atomic.
    fresh: AtomicBool,
}

impl Board {
    fn take(&self) -> Rows {
        let mut rows = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        self.fresh.store(false, Ordering::Release);
        std::mem::take(&mut *rows)
    }
    fn put(&self, new: Rows, waker: &Waker) {
        let mut rows = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        rows.text.push_str(&new.text);
        rows.count += new.count;
        self.fresh.store(true, Ordering::Release);
        drop(rows);
        waker.wake();
    }
}

/// A search under way. Dropping it stops the thread, and `done` doesn't run.
struct Run {
    board: Arc<Board>,
    stop: Arc<AtomicBool>,
    _job: Sub,
}

impl Drop for Run {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The search `*grep*` holds.
pub(crate) struct Grep {
    pub(crate) buf: BufferId,
    /// How many rows the buffer has, in lines 1 to this.
    pub(crate) rows: usize,
    run: Option<Run>,
}

impl Grep {
    pub(crate) fn is_running(&self) -> bool {
        self.run.is_some()
    }
}

/// How a search ended.
struct Ended {
    rows: usize,
    files: usize,
}

impl Ended {
    fn last_line(&self) -> String {
        let (rows, files) = (self.rows, self.files);
        let plural = if files == 1 { "" } else { "s" };
        match rows {
            0 => "No matches.\n".into(),
            1 => "1 match in 1 file.\n".into(),
            MAX_ROWS.. => format!("Stopped at {rows} matches, in {files} file{plural}.\n"),
            _ => format!("{rows} matches in {files} file{plural}.\n"),
        }
    }
}

/// A file's text, unless it's too big or binary. Bytes that aren't UTF-8 show as replacements.
fn read_text(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let head = &bytes[..bytes.len().min(BINARY_WITHIN)];
    if head.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Add a row to `out` for each line of `text` that a match starts on, up to `limit` rows in `out`.
fn scan(rel: &str, text: &str, pat: &Pattern, limit: usize, out: &mut Rows) {
    use std::fmt::Write;
    let (mut line, mut line_at) = (1, 0);
    // There's no line after the last `\n`, or in an empty file, for `^` to match on.
    let no_line_at_end = text.is_empty() || text.ends_with('\n');
    for m in pat.all(text, text.len()) {
        if m.start < line_at || (m.start == text.len() && no_line_at_end) {
            continue;
        }
        let before = &text[line_at..m.start];
        line += before.bytes().filter(|b| *b == b'\n').count();
        let start = before.rfind('\n').map_or(line_at, |at| line_at + at + 1);
        let rest = &text[m.start..];
        let end = rest.find('\n').map_or(text.len(), |at| m.start + at);
        let col = text[start..m.start].chars().count() + 1;
        let shown = text[start..end].trim_end_matches('\r');
        let cut = shown.char_indices().nth(MAX_TEXT);
        let (shown, more) = cut.map_or((shown, ""), |(at, _)| (&shown[..at], " ..."));
        let _ = writeln!(out.text, "{rel}:{line}:{col}: {shown}{more}");
        out.count += 1;
        if out.count >= limit || end == text.len() {
            return;
        }
        (line, line_at) = (line + 1, end + 1);
    }
}

/// The search, on its own thread.
fn search(root: &Path, pat: &Pattern, stop: &AtomicBool, board: &Board, waker: &Waker) -> Ended {
    let mut ended = Ended { rows: 0, files: 0 };
    let mut rows = Rows::default();
    let mut published: Option<Instant> = None;
    walk(root, stop, &mut |batch| {
        for rel in batch {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if let Some(text) = read_text(&root.join(&rel)) {
                let had = rows.count;
                scan(&rel, &text, pat, had + MAX_ROWS - ended.rows, &mut rows);
                ended.files += usize::from(rows.count > had);
                ended.rows += rows.count - had;
            }
            if ended.rows >= MAX_ROWS {
                stop.store(true, Ordering::Relaxed);
                return;
            }
            // Not only after a match: the rows held back may be the last for a long while.
            if rows.count > 0 && published.is_none_or(|at| at.elapsed() >= PUBLISH_EVERY) {
                board.put(std::mem::take(&mut rows), waker);
                published = Some(Instant::now());
            }
        }
    });
    if rows.count > 0 {
        board.put(rows, waker);
    }
    ended
}

/// A row's path, line and column. The first `:line:col: ` ends the path.
pub(crate) fn parse_row(row: &str) -> Option<(&str, usize, usize)> {
    row.match_indices(": ").find_map(|(at, _)| {
        let (head, col) = row[..at].rsplit_once(':')?;
        let (path, line) = head.rsplit_once(':')?;
        Some((path, line.parse().ok()?, col.parse().ok()?))
    })
}

impl Ctx {
    /// Search under `root`, stopping any search before. A bad pattern leaves things as they were.
    fn grep_start(&mut self, src: &str, root: PathBuf) -> Result {
        let pat = Pattern::new(src, Pattern::ignores_case(src))?;
        self.procs.grep = None;
        let new = self.find_buffer(GREP).is_none();
        let header = format!("Grep for {src} in {}\n", root.display());
        let buf = self.output_buffer(GREP, &header, &root)?;
        if new {
            self.buffers[buf]
                .local
                .bind(State::Normal, "<CR>", "compile-goto-error");
        }
        for w in self.windows_showing(buf) {
            self.text_mut(buf)?.set_cursor(w, 0);
        }
        self.show_results(buf)?;
        self.set_results(Results::grep(buf, root.clone()));
        let board = Arc::new(Board::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (b, s, waker) = (board.clone(), stop.clone(), self.waker());
        let job = self.spawn(
            move || search(&root, &pat, &s, &b, &waker),
            |ctx, ended| ctx.grep_ended(&ended),
        );
        let run = Some(Run {
            board,
            stop,
            _job: job,
        });
        self.procs.grep = Some(Grep { buf, rows: 0, run });
        Ok(())
    }

    /// Put what the search has left into the buffer, as each turn does. True if that was news.
    pub(crate) fn grep_take(&mut self) -> bool {
        let Some(run) = self.procs.grep.as_ref().and_then(|g| g.run.as_ref()) else {
            return false;
        };
        if !run.board.fresh.load(Ordering::Acquire) {
            return false;
        }
        let rows = run.board.take();
        self.grep_append(&rows.text, rows.count);
        true
    }

    fn grep_append(&mut self, text: &str, rows: usize) {
        let Some(g) = self.procs.grep.as_mut() else {
            return;
        };
        g.rows += rows;
        let buf = g.buf;
        let Ok(t) = self.text_mut(buf) else {
            self.procs.grep = None; // Killed, so the search is for no one.
            return;
        };
        let end = t.len();
        let _ = t.tx(Origin::Extension).silent().insert(end, text).commit();
        t.set_modified(false);
    }

    fn grep_ended(&mut self, ended: &Ended) -> Result {
        self.grep_take();
        self.grep_append(&ended.last_line(), 0);
        if let Some(g) = self.procs.grep.as_mut() {
            g.run = None;
        }
        Ok(())
    }
}

/// `line` is a pattern, then perhaps a directory: its last word, if that names one from `dir` or
/// else from `dir`'s project. So a pattern can have spaces, and can't end in such a word.
fn pattern_and_root(line: &str, dir: &Path) -> (String, PathBuf) {
    let line = line.trim();
    let root = project_root(dir);
    let Some((pat, last)) = line.rsplit_once(char::is_whitespace) else {
        return (line.to_string(), root);
    };
    let tilde = match (last.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(""), Some(home)) => Some(PathBuf::from(home)),
        (Some(rest), Some(home)) if rest.starts_with('/') => {
            Some(Path::new(&home).join(&rest[1..]))
        }
        _ => None,
    };
    let named = [dir, &root].map(|base| tilde.clone().unwrap_or_else(|| base.join(last)));
    match named.iter().find(|d| d.is_dir()) {
        Some(found) => (pat.trim_end().to_string(), normalize(found)),
        None => (line.to_string(), root),
    }
}

pub(super) fn register_commands(reg: &mut Registry) {
    reg.command(
        "grep",
        "Search the text of files under this buffer's project, or the directory after the \
         pattern, into *grep*. With no pattern, ask for one",
        |ctx, a| {
            let dir = here_dir(ctx);
            if let Some(line) = a.text.as_deref().filter(|t| !t.trim().is_empty()) {
                let (pat, root) = pattern_and_root(line, &dir);
                return ctx.grep_start(&pat, root);
            }
            let spec = PromptSpec::new("Grep: ").history("grep").evil_keys();
            ctx.prompt(spec, move |ctx, answer| {
                let Some(line) = answer.filter(|l| !l.trim().is_empty()) else {
                    return Ok(());
                };
                let (pat, root) = pattern_and_root(&line, &dir);
                ctx.grep_start(&pat, root)
            });
            Ok(())
        },
    );
}
