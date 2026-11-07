//! `grep`, `compile-goto-error`, `next-error` and `previous-error`: search the text of a project
//! into a read-only `*grep*`, and go to what it found. As with the finder, no key waits on the
//! disk: one thread walks and reads, and leaves rows on a board at most every
//! [`PUBLISH_EVERY`]. Each turn of the main thread takes what's there (`Ctx::step`).
//!
//! A row is one line of a file, at its first match, however many it holds, as Emacs's grep gives.
//! The buffer is the header, the rows in the order the walk finds them, and a last line, so a
//! row's number in the results is its line in the buffer. It isn't kept by the session: nothing
//! could go back to the run that made it.
use super::{PUBLISH_EVERY, here_dir, project_root, walk};
use crate::dispatch::Args;
use crate::error::{Error, Result};
use crate::ext::{Ctx, Registry};
use crate::files::{FileTarget, normalize};
use crate::ids::{BufferId, WindowId};
use crate::keymap::State;
use crate::prompt::PromptSpec;
use crate::rt::{Sub, Waker};
use crate::text::{Origin, Pattern};
use crate::workspace::{ShowIn, Side};
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
    buf: BufferId,
    root: PathBuf,
    /// How many rows the buffer has, in lines 1 to this.
    rows: usize,
    /// The row last gone to.
    current: Option<usize>,
    run: Option<Run>,
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
        if out.count >= limit {
            return;
        }
        (line, line_at) = (line + 1, (end + 1).min(text.len()));
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
            let Some(text) = read_text(&root.join(&rel)) else {
                continue;
            };
            let had = rows.count;
            scan(&rel, &text, pat, had + MAX_ROWS - ended.rows, &mut rows);
            if rows.count == had {
                continue;
            }
            ended.files += 1;
            ended.rows += rows.count - had;
            if ended.rows >= MAX_ROWS {
                stop.store(true, Ordering::Relaxed);
                return;
            }
            if published.is_none_or(|at| at.elapsed() >= PUBLISH_EVERY) {
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
fn parse_row(row: &str) -> Option<(&str, usize, usize)> {
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
        self.grep = None;
        let buf = match self.find_buffer(GREP) {
            Some(buf) => buf,
            None => self.grep_buffer()?,
        };
        let header = format!("Grep for {src} in {}\n", root.display());
        let t = self.text_mut(buf)?;
        let all = 0..t.len();
        let tx = t.tx(Origin::Extension).silent();
        tx.replace(all, header).commit()?;
        t.set_modified(false);
        for w in self.windows_showing(buf) {
            self.text_mut(buf)?.set_cursor(w, 0);
        }
        self.set_directory(buf, &root);
        self.grep_show(buf)?;
        let board = Arc::new(Board::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (b, s, waker, from) = (board.clone(), stop.clone(), self.waker(), root.clone());
        let job = self.spawn(
            move || search(&from, &pat, &s, &b, &waker),
            |ctx, ended| ctx.grep_ended(&ended),
        );
        self.grep = Some(Grep {
            buf,
            root,
            rows: 0,
            current: None,
            run: Some(Run {
                board,
                stop,
                _job: job,
            }),
        });
        Ok(())
    }

    fn grep_buffer(&mut self) -> Result<BufferId> {
        let buf = self.create_text(GREP, "");
        let settings = self.text_mut(buf)?.settings_mut();
        settings.read_only = true;
        settings.max_width = None;
        settings.center = false;
        let local = &mut self.buffers[buf].local;
        local.bind(State::Normal, "<CR>", "compile-goto-error");
        Ok(buf)
    }

    /// Show the results as Emacs does, with the focus staying: in a window that has them, or
    /// another window that's no terminal, or else a split.
    fn grep_show(&mut self, buf: BufferId) -> Result {
        if !self.windows_showing(buf).is_empty() {
            return Ok(());
        }
        let here = self.focused;
        let others = self.windows_by_use().into_iter();
        let mut others = others.filter(|w| Some(*w) != here);
        if let Some(w) = others.find(|w| self.takes_source(*w, buf)) {
            self.set_window_buffer(w, buf);
            return Ok(());
        }
        let shown = self.show(buf, ShowIn::Split(Side::Below));
        shown.or_else(|_| self.show(buf, ShowIn::Current))?;
        if let Some(w) = here.filter(|w| self.windows.contains_key(*w)) {
            self.focus(w);
        }
        Ok(())
    }

    /// Put what the search has left into the buffer, as each turn does. True if that was news.
    pub(crate) fn grep_take(&mut self) -> bool {
        let Some(run) = self.grep.as_ref().and_then(|g| g.run.as_ref()) else {
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
        let Some(g) = self.grep.as_mut() else { return };
        g.rows += rows;
        let buf = g.buf;
        let Ok(t) = self.text_mut(buf) else {
            self.grep = None; // Killed, so the search is for no one.
            return;
        };
        let end = t.len();
        let _ = t.tx(Origin::Extension).silent().insert(end, text).commit();
        t.set_modified(false);
    }

    fn grep_ended(&mut self, ended: &Ended) -> Result {
        self.grep_take();
        self.grep_append(&ended.last_line(), 0);
        if let Some(g) = self.grep.as_mut() {
            g.run = None;
        }
        Ok(())
    }

    /// Whether a file can go in this window without hiding the results or a terminal.
    fn takes_source(&self, w: WindowId, results: BufferId) -> bool {
        let shows = self.window_buffer(w);
        shows.is_some_and(|b| b != results && !self.is_terminal(b))
    }

    /// Go to the result in line `row` of the buffer, as Emacs's `compilation-goto-locus` does: in
    /// the focused window unless that has the results or a terminal, then in the one used last
    /// that has neither, or else a split. That window takes the focus.
    fn grep_visit(&mut self, row: usize) -> Result {
        let no_results = || Error::Msg("No grep results".into());
        let g = self.grep.as_ref().ok_or_else(no_results)?;
        let (buf, path_from) = (g.buf, g.root.clone());
        let t = self.text(buf)?;
        let at = t.line_start(row);
        let text = t.rope().line(row).to_string();
        let parsed = parse_row(&text);
        let (path, line, col) = parsed.ok_or_else(|| Error::Msg("No match here".into()))?;
        let target = FileTarget {
            path: path_from.join(path),
            line: Some(line),
            col: Some(col),
        };
        for w in self.windows_showing(buf) {
            self.text_mut(buf)?.set_cursor(w, at);
        }
        if let Some(g) = self.grep.as_mut() {
            g.current = Some(row);
        }
        if !self.focused.is_some_and(|w| self.takes_source(w, buf)) {
            let mut others = self.windows_by_use().into_iter();
            match (others.find(|w| self.takes_source(*w, buf)), self.focused) {
                (Some(w), _) => self.focus(w),
                (None, Some(w)) => {
                    if let Ok(new) = self.split_window(w, Side::Below) {
                        self.focus(new);
                    }
                }
                (None, None) => {}
            }
        }
        self.open_target(&target).map(drop)
    }

    /// The row this cursor is on, in `*grep*`.
    fn grep_row_here(&self, a: &Args) -> Result<usize> {
        let no = || Error::Msg("No match here".into());
        let w = a.window.or(self.focused).ok_or_else(no)?;
        let g = self
            .grep
            .as_ref()
            .filter(|g| self.windows.get(w).is_some_and(|win| win.buffer == g.buf));
        let g = g.ok_or_else(no)?;
        let t = self.text(g.buf)?;
        let row = t.line_of(t.cursor(w).ok_or_else(no)?);
        (1..=g.rows).contains(&row).then_some(row).ok_or_else(no)
    }

    /// `:cn` and `:cp`.
    fn grep_step(&mut self, forward: bool) -> Result {
        let live = self.grep.as_ref().filter(|g| self.is_live(g.buf));
        let g = live.ok_or_else(|| Error::Msg("No grep results".into()))?;
        let next = match (g.current, forward) {
            (None, true) => 1,
            (None, false) => 0,
            (Some(at), true) => at + 1,
            (Some(at), false) => at - 1,
        };
        if next > g.rows {
            return Err(Error::Msg(match (g.run.is_some(), g.rows) {
                (true, _) => "No more matches yet".into(),
                (false, 0) => "No matches".into(),
                (false, _) => "Moved past last match".into(),
            }));
        }
        if next == 0 {
            return Err(Error::Msg("Moved back before first match".into()));
        }
        self.grep_visit(next)
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
    reg.command(
        "compile-goto-error",
        "In *grep*: open the file of the match on this line, in another window",
        |ctx, a| {
            let row = ctx.grep_row_here(a)?;
            ctx.grep_visit(row)
        },
    );
    reg.command(
        "next-error",
        "Go to the next match in *grep*, from any window",
        |ctx, _| ctx.grep_step(true),
    );
    reg.command(
        "previous-error",
        "Go to the previous match in *grep*, from any window",
        |ctx, _| ctx.grep_step(false),
    );
}
