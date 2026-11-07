//! Where a name on a command line or in a compiler's output says to go: `path`, `path:line`,
//! `path:line:col`, each with the trailing colon rustc and rg print, and `+line[:col] path`.
//! Lines and columns count from 1, and columns count characters. A file that really exists with
//! the name, colons included, wins over reading them as numbers.

use crate::text::TextBuf;
use crate::{BufferId, Ctx, Result, ShowIn, Side, State};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// A file, and where in it to put the cursor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileTarget {
    /// The file, joined to the directory the name was read against.
    pub path: PathBuf,
    /// The line, from 1. None leaves the cursor where the window last had it.
    pub line: Option<usize>,
    /// The column in characters, from 1. None is the line's start.
    pub col: Option<usize>,
}

impl FileTarget {
    /// Read `path[:line[:col]][:]` against the directory `base`. This never fails: a name that
    /// holds no numbers is a name, whether or not the file is there.
    pub fn parse(name: &OsStr, base: &Path) -> FileTarget {
        let target = |path: &[u8], line, col| FileTarget {
            path: base.join(OsStr::from_bytes(path)),
            line,
            col,
        };
        let exists = |path: &[u8]| base.join(OsStr::from_bytes(path)).exists();
        let name = name.as_bytes();
        if exists(name) {
            return target(name, None, None);
        }
        let body = name
            .strip_suffix(b":")
            .filter(|b| !b.is_empty())
            .unwrap_or(name);
        let Some((head, last)) = split_number(body).filter(|_| !exists(body)) else {
            return target(body, None, None);
        };
        match split_number(head).filter(|_| !exists(head)) {
            Some((path, line)) => target(path, Some(line), Some(last)),
            None => target(head, Some(last), None),
        }
    }

    /// Read a command line's names against `base`: each is `+line[:col]` and the name it applies
    /// to, which is then taken as it stands, or what [`FileTarget::parse`] reads.
    pub fn parse_args(
        args: impl IntoIterator<Item = OsString>,
        base: &Path,
    ) -> std::result::Result<Vec<FileTarget>, String> {
        let mut targets = vec![];
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let position = plus_position(arg.as_bytes()).filter(|_| !base.join(&arg).exists());
            let Some((line, col)) = position else {
                targets.push(FileTarget::parse(&arg, base));
                continue;
            };
            let Some(path) = args.next() else {
                return Err(format!("{} needs a file", arg.to_string_lossy()));
            };
            targets.push(FileTarget {
                path: base.join(path),
                line: Some(line),
                col,
            });
        }
        Ok(targets)
    }

    /// What [`FileTarget::parse_args`] reads back as this target, given a path that's absolute.
    pub fn to_args(&self) -> Vec<OsString> {
        let path = self.path.clone().into_os_string();
        match (self.line, self.col) {
            (Some(line), Some(col)) => vec![format!("+{line}:{col}").into(), path],
            (Some(line), None) => vec![format!("+{line}").into(), path],
            (None, _) => vec![path],
        }
    }
}

/// `head:123` as `head` and 123, if `head` isn't empty.
fn split_number(name: &[u8]) -> Option<(&[u8], usize)> {
    let colon = name.iter().rposition(|c| *c == b':').filter(|at| *at > 0)?;
    Some((&name[..colon], number(&name[colon + 1..])?))
}

/// Only digits: `parse` would take `+5` for 5.
fn number(digits: &[u8]) -> Option<usize> {
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

fn plus_position(arg: &[u8]) -> Option<(usize, Option<usize>)> {
    let rest = arg.strip_prefix(b"+")?;
    match rest.iter().position(|c| *c == b':') {
        Some(colon) => Some((number(&rest[..colon])?, Some(number(&rest[colon + 1..])?))),
        None => Some((number(rest)?, None)),
    }
}

/// Where a line and column are, as near as the text has. A last line that's only the end of the
/// final newline doesn't count, as `:999` doesn't count it.
fn position(t: &TextBuf, line: usize, col: usize) -> usize {
    let last = t.line_count() - 1;
    let last = last - usize::from(last > 0 && t.line_start(last) == t.len());
    let line = line.saturating_sub(1).min(last);
    let chars = t.rope().line(line).chars().take(col.saturating_sub(1));
    let width: usize = chars.take_while(|c| *c != '\n').map(char::len_utf8).sum();
    t.line_start(line) + width
}

impl Ctx {
    /// Whether a buffer is a terminal, which no file should replace in its window.
    pub(crate) fn is_terminal(&self, b: BufferId) -> bool {
        matches!(self.state_of(b), State::Passthrough | State::Copy)
    }

    /// Open a target as `:e` would, in the focused window, with a missing file as a new buffer,
    /// and put the cursor there, keeping the place it leaves by [`Ctx::set_jump_rule`]. A terminal
    /// isn't replaced: if the focused window is in passthrough or copy state, the one used last
    /// that isn't takes the file and the focus. With none, a new one on its left does, as `C-w v`
    /// makes, unless it's too small to split.
    pub fn open_target(&mut self, target: &FileTarget) -> Result<BufferId> {
        let buf = self.open_file(&target.path)?;
        self.leave_terminal();
        if let (Some(_), Some(w)) = (target.line, self.focused()) {
            self.keep_place(w);
        }
        let shown = self.show(buf, ShowIn::Current)?;
        if let (Some(w), Some(line)) = (shown, target.line) {
            let pos = position(self.text(buf)?, line, target.col.unwrap_or(1));
            self.reveal(buf, pos)?;
            let pos = self.rested(buf, pos);
            self.text_mut(buf)?.set_cursor(w, pos);
        }
        Ok(buf)
    }

    fn leave_terminal(&mut self) {
        let in_terminal = self.focused_buffer().is_some_and(|b| self.is_terminal(b));
        let Some(here) = self.focused().filter(|_| in_terminal) else {
            return;
        };
        let others = self.windows.iter();
        let others = others.filter(|(_, w)| !self.is_terminal(w.buffer));
        let other = others.max_by_key(|(_, w)| w.used).map(|(id, _)| id);
        if let Some(w) = other.or_else(|| self.split_window(here, Side::Left).ok()) {
            self.focus(w);
        }
    }
}
