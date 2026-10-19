//! Regex search (D4: Rust's regex syntax). It runs over the text in one piece, copied
//! from the rope once per revision and kept until the next edit.

use super::TextBuf;
use crate::error::*;
use crate::layout::Dir;
use regex::Regex;
use std::cell::Ref;
use std::ops::Range;

/// A compiled search pattern in Rust's regex syntax, where `^` and `$` match at every
/// line's start and end. Case folding is the caller's choice: die-vim decides smartcase.
#[derive(Clone, Debug)]
pub struct Pattern(Regex);

impl Pattern {
    /// Compile `src`, ignoring case when `ignore_case`. Errors: `BadPattern`.
    pub fn new(src: &str, ignore_case: bool) -> Result<Pattern> {
        regex::RegexBuilder::new(src)
            .multi_line(true)
            .case_insensitive(ignore_case)
            .build()
            .map(Pattern)
            .map_err(|e| Error::BadPattern(e.to_string()))
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A match from [`TextBuf::search`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The match, in bytes. It can be empty (`^`).
    pub range: Range<usize>,
    /// The search went past the end of the buffer (or its start, going backward) to find it.
    pub wrapped: bool,
}

/// The first char boundary after `i`, or `len + 1` past the end.
fn next_boundary(text: &str, i: usize) -> usize {
    (i + 1..=text.len())
        .find(|&j| text.is_char_boundary(j))
        .unwrap_or(text.len() + 1)
}

/// The match with the last start in `[lo, hi)`, trying every start position, as Emacs's
/// backward search does, so `aa` in `aaa` is found at 1.
fn last_start(re: &Regex, text: &str, lo: usize, hi: usize) -> Option<Range<usize>> {
    let mut best = None;
    let mut p = lo;
    while p <= text.len() {
        let Some(m) = re.find_at(text, p) else { break };
        if m.start() >= hi {
            break;
        }
        best = Some(m.range());
        p = next_boundary(text, m.start());
    }
    best
}

impl TextBuf {
    /// The next match: going `Forward`, the first that starts after `from`; going
    /// `Backward`, the last that starts before it. When there's none, the search wraps
    /// around the buffer and may find a match at `from` itself. Matches inside hidden
    /// text count; revealing them is the caller's (`Ctx::reveal`).
    pub fn search(&self, pat: &Pattern, from: usize, dir: Dir) -> Option<Found> {
        let text = self.flat();
        let (re, from) = (&pat.0, from.min(text.len()));
        let (near, far) = match dir {
            Dir::Forward => {
                let start = next_boundary(&text, from);
                let near = (start <= text.len())
                    .then(|| re.find_at(&text, start))
                    .flatten();
                (near.map(|m| m.range()), re.find(&text).map(|m| m.range()))
            }
            Dir::Backward => (
                last_start(re, &text, 0, from),
                last_start(re, &text, from, text.len() + 1),
            ),
        };
        match (near, far) {
            (Some(range), _) => Some(Found {
                range,
                wrapped: false,
            }),
            (None, Some(range)) => Some(Found {
                range,
                wrapped: true,
            }),
            (None, None) => None,
        }
    }

    /// Every match that overlaps `range`, in order and without overlaps, for highlighting.
    /// Matching starts at the beginning of `range`'s first line, so a match that starts
    /// before `range` on that line is found; one that starts on an earlier line isn't.
    pub fn matches(&self, pat: &Pattern, range: Range<usize>) -> Vec<Range<usize>> {
        let text = self.flat();
        let start = self.line_start(self.line_of(range.start));
        let mut out = vec![];
        let mut p = start;
        while p <= text.len() {
            let Some(m) = pat.0.find_at(&text, p) else {
                break;
            };
            if m.start() >= range.end && !(m.is_empty() && m.start() == range.start) {
                break;
            }
            if m.end() > range.start || (m.is_empty() && m.start() >= range.start) {
                out.push(m.range());
            }
            p = if m.is_empty() {
                next_boundary(&text, m.start())
            } else {
                m.end()
            };
        }
        out
    }

    /// The text in one piece, made at most once per revision.
    fn flat(&self) -> Ref<'_, str> {
        let fresh = self.flat.borrow().0 == Some(self.rev);
        if !fresh {
            let mut f = self.flat.borrow_mut();
            f.1.clear();
            f.1.reserve(self.rope.len_bytes());
            for chunk in self.rope.chunks() {
                f.1.push_str(chunk);
            }
            f.0 = Some(self.rev);
        }
        Ref::map(self.flat.borrow(), |f| f.1.as_str())
    }
}
