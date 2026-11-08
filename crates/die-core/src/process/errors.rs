//! `next-error`, `previous-error` and `compile-goto-error` over `*grep*`'s rows and a
//! compilation's errors, as Emacs's go through `next-error-last-buffer`: the results made or gone
//! to last. A line names a place, and going to it opens the file there.
use super::compile;
use crate::dispatch::Args;
use crate::error::{Error, Result};
use crate::ext::{Ctx, Registry};
use crate::files::FileTarget;
use crate::ids::{BufferId, WindowId};
use crate::project::{parse_row, project_root};
use crate::workspace::Side;
use std::path::{Path, PathBuf};

/// How a results buffer's lines name places.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `*grep*`: lines 1 to its row count are `path:line:col: text`.
    Grep,
    /// A compilation's output (`compile::locus`).
    Compile,
}

/// A buffer `next-error` can go through.
pub(crate) struct Results {
    pub(crate) buf: BufferId,
    /// Where its file names are from.
    dir: PathBuf,
    kind: Kind,
    /// The line last gone to.
    current: Option<usize>,
}

impl Results {
    pub(crate) fn grep(buf: BufferId, dir: PathBuf) -> Results {
        let (kind, current) = (Kind::Grep, None);
        Results {
            buf,
            dir,
            kind,
            current,
        }
    }
    pub(crate) fn compile(buf: BufferId, dir: PathBuf) -> Results {
        let (kind, current) = (Kind::Compile, None);
        Results {
            buf,
            dir,
            kind,
            current,
        }
    }
    /// `match` or `error`, as Emacs's grep and compile call what they find, and its plural.
    fn noun(&self) -> (&'static str, &'static str) {
        match self.kind {
            Kind::Grep => ("match", "matches"),
            Kind::Compile => ("error", "errors"),
        }
    }
}

/// A place a line names. A note's isn't stopped at by `next-error`, as Emacs's
/// `compilation-skip-threshold` skips them.
pub(crate) struct Locus {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) col: Option<usize>,
    pub(crate) note: bool,
    /// Its path is rustc's, which under cargo is from the workspace's root.
    pub(crate) cargo: bool,
}

impl Ctx {
    /// These results first for `next-error`, in place of any for the same buffer.
    pub(crate) fn set_results(&mut self, r: Results) {
        let mut kept = std::mem::take(&mut self.procs.results);
        kept.retain(|o| o.buf != r.buf && self.is_live(o.buf));
        kept.insert(0, r);
        self.procs.results = kept;
    }

    /// The place line `at` of these results names.
    fn locus(&self, r: &Results, at: usize) -> Option<Locus> {
        let t = self.text(r.buf).ok()?;
        let text = t.rope().get_line(at)?.to_string();
        match r.kind {
            Kind::Grep => {
                let rows = self.procs.grep.as_ref().filter(|g| g.buf == r.buf);
                let rows = rows.map_or(0, |g| g.rows);
                let (path, line, col) = parse_row(&text).filter(|_| (1..=rows).contains(&at))?;
                let (path, col) = (path.to_string(), Some(col));
                Some(Locus {
                    path,
                    line,
                    col,
                    note: false,
                    cargo: false,
                })
            }
            Kind::Compile => compile::locus(t, at, &text),
        }
    }

    /// Whether what fills these results is still going.
    fn results_running(&self, r: &Results) -> bool {
        let grep = self.procs.grep.as_ref().filter(|g| g.buf == r.buf);
        grep.is_some_and(|g| g.is_running()) || self.procs.streams.iter().any(|s| s.buf == r.buf)
    }

    /// Go to the place line `line` of the first results names, as Emacs's
    /// `compilation-goto-locus` does: in the focused window unless that has the results or a
    /// terminal, then in the one that has the file, or the one used last that has neither, or else
    /// a split. That window takes the focus. A file that's gone since is an error, not a new empty
    /// buffer, and the line still counts as visited, so `:cn` goes on past it. A path is tried
    /// from the results' directory, then from its project. rustc's are tried from the cargo
    /// workspace first, since cargo runs rustc there wherever it's run from.
    fn visit_locus(&mut self, line: usize) -> Result {
        let r = self.procs.results.first().ok_or_else(no_results)?;
        let (buf, noun, dir) = (r.buf, r.noun().0, r.dir.clone());
        let here = self
            .locus(r, line)
            .ok_or_else(|| Error::Msg(format!("No {noun} here")))?;
        let (project, workspace) = (project_root(&dir), cargo_root(&dir));
        let tries = workspace.filter(|_| here.cargo).into_iter();
        let tries = tries.chain([dir.as_path(), &project]);
        let found = tries.map(|d| d.join(&here.path)).find(|p| p.is_file());
        let at = self.text(buf)?.line_start(line);
        for w in self.windows_showing(buf) {
            self.text_mut(buf)?.set_cursor(w, at);
        }
        self.procs.results[0].current = Some(line);
        let lost = || format!("Can't find {} from {}", here.path, dir.display());
        let path = found.ok_or_else(|| Error::Msg(lost()))?;
        let target = FileTarget {
            path,
            line: Some(here.line),
            col: here.col,
        };
        let file = self.open_file(&target.path)?;
        if !self.focused.is_some_and(|w| self.takes_source(w, buf)) {
            let by_use = self.windows_by_use();
            let has_file = by_use
                .iter()
                .find(|w| self.window_buffer(**w) == Some(file));
            let other = has_file.or_else(|| by_use.iter().find(|w| self.takes_source(**w, buf)));
            match (other.copied(), self.focused) {
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

    /// Whether a file can go in this window without hiding the results or a terminal.
    pub(crate) fn takes_source(&self, w: WindowId, results: BufferId) -> bool {
        let shows = self.window_buffer(w);
        shows.is_some_and(|b| b != results && !self.is_terminal(b))
    }

    /// The results this window shows, put first, and the line its cursor is on.
    fn results_here(&mut self, a: &Args) -> Result<usize> {
        let no = || Error::Msg("Not in grep or compilation results".into());
        let w = a.window.or(self.focused).ok_or_else(no)?;
        let buf = self.window_buffer(w).ok_or_else(no)?;
        let at = self
            .procs
            .results
            .iter()
            .position(|r| r.buf == buf)
            .ok_or_else(no)?;
        let r = self.procs.results.remove(at);
        self.procs.results.insert(0, r);
        let t = self.text(buf)?;
        Ok(t.line_of(t.cursor(w).ok_or_else(no)?))
    }

    /// The next line after `from`, or before it, that names a place `next-error` stops at, in the
    /// first results. Its error says why there's none.
    fn next_locus(&self, from: Option<usize>, forward: bool) -> Result<usize> {
        let r = self.procs.results.first().ok_or_else(no_results)?;
        let lines = self.text(r.buf)?.line_count();
        let stops = |l: &usize| self.locus(r, *l).is_some_and(|l| !l.note);
        let found = match (from, forward) {
            (None, true) => (0..lines).find(stops),
            (Some(at), true) => (at + 1..lines).find(stops),
            (None, false) => None,
            (Some(at), false) => (0..at).rev().find(stops),
        };
        found.ok_or_else(|| {
            let (one, more) = r.noun();
            Error::Msg(match (forward, self.results_running(r)) {
                (false, _) => format!("Moved back before first {one}"),
                (true, true) => format!("No more {more} yet"),
                (true, false) if (0..lines).any(|l| stops(&l)) => format!("Moved past last {one}"),
                (true, false) => format!("No {more}"),
            })
        })
    }

    /// `:cn` and `:cp`.
    fn step_results(&mut self, forward: bool) -> Result {
        let buffers = &self.buffers;
        self.procs.results.retain(|r| buffers.contains_key(r.buf));
        let current = self.procs.results.first().ok_or_else(no_results)?.current;
        let line = self.next_locus(current, forward)?;
        self.visit_locus(line)
    }
}

/// The cargo workspace `dir` is in, as cargo finds it: the nearest directory up with a
/// `Cargo.toml` that has a `[workspace]`, or else with a `Cargo.toml`.
fn cargo_root(dir: &Path) -> Option<&Path> {
    let workspace = |d: &Path| {
        let toml = std::fs::read_to_string(d.join("Cargo.toml")).unwrap_or_default();
        let mut tables = toml.lines().map(str::trim_start);
        tables.any(|l| l.starts_with("[workspace]") || l.starts_with("[workspace."))
    };
    let mut manifests = dir.ancestors().filter(|d| d.join("Cargo.toml").is_file());
    let nearest = manifests.next()?;
    let root = std::iter::once(nearest)
        .chain(manifests)
        .find(|d| workspace(d));
    Some(root.unwrap_or(nearest))
}

fn no_results() -> Error {
    Error::Msg("No grep or compilation results".into())
}

pub(super) fn register(reg: &mut Registry) {
    reg.command(
        "compile-goto-error",
        "In *grep* or *compilation*: open the file of the match or error on this line, in another \
         window",
        |ctx, a| {
            let line = ctx.results_here(a)?;
            ctx.visit_locus(line)
        },
    );
    reg.command(
        "next-error",
        "Go to the next match in *grep* or error in *compilation*, whichever was made or gone to \
         last, from any window",
        |ctx, _| ctx.step_results(true),
    );
    reg.command(
        "previous-error",
        "Go to the previous match in *grep* or error in *compilation*, from any window",
        |ctx, _| ctx.step_results(false),
    );
    for (name, forward) in [
        ("compilation-next-error", true),
        ("compilation-previous-error", false),
    ] {
        let doc = match forward {
            true => "In *compilation*: move to the next error, without going there",
            false => "In *compilation*: move to the previous error, without going there",
        };
        reg.command(name, doc, move |ctx, a| {
            let line = ctx.results_here(a)?;
            let w = a.window.or(ctx.focused).ok_or_else(no_results)?;
            let to = ctx.next_locus(Some(line), forward)?;
            let buf = ctx.procs.results[0].buf;
            let at = ctx.text(buf)?.line_start(to);
            ctx.text_mut(buf)?.set_cursor(w, at);
            Ok(())
        });
    }
}
