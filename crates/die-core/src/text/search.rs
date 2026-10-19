//! Regex search (D4: Rust's regex syntax), finding what Emacs's searches find under Evil's
//! `/ ? n N * #`. `search` runs over the text in one piece, copied from the rope at the
//! first search after an edit; `matches` copies only the lines it scans.

use super::TextBuf;
use crate::error::*;
use crate::layout::Dir;
use regex_automata::meta::{BuildError, Regex};
use regex_automata::util::syntax;
use regex_automata::{Anchored, Input};
use std::cell::Ref;
use std::ops::Range;

/// A compiled search pattern in Rust's regex syntax, where `^` and `$` match at every
/// line's start and end, before `\r\n` too. Case folding is the caller's choice: die-vim
/// decides smartcase.
#[derive(Clone, Debug)]
pub struct Pattern {
    re: Regex,
    src: String,
}

impl Pattern {
    /// Compile `src`, ignoring case when `ignore_case`. Errors: `BadPattern`.
    pub fn new(src: &str, ignore_case: bool) -> Result<Pattern> {
        let syntax = syntax::Config::new()
            .multi_line(true)
            .crlf(true)
            .case_insensitive(ignore_case);
        let re = Regex::builder().syntax(syntax).build(src).map_err(bad)?;
        Ok(Pattern {
            re,
            src: src.to_string(),
        })
    }

    /// `text` with every regex metacharacter escaped, so that a pattern made from it
    /// matches it literally, as `*` and `#` need for the word under the cursor.
    pub fn escape(text: &str) -> String {
        regex_syntax::escape(text)
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.src
    }
}

fn bad(e: BuildError) -> Error {
    Error::BadPattern(match (e.syntax_error(), e.size_limit()) {
        (Some(e), _) => e.to_string(),
        (_, Some(n)) => format!("the compiled pattern is over the size limit of {n} bytes"),
        _ => e.to_string(),
    })
}

/// A match from [`TextBuf::search`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The match, in bytes. It can be empty (`^`).
    pub range: Range<usize>,
    /// The search went past the end of the buffer (or its start, going backward) to find it.
    pub wrapped: bool,
}

/// The match that starts last before `hi` among those that end by `hi`, as Emacs's
/// `re-search-backward` finds it. `hi` bounds the match itself, so `a+` from `aa|a` finds
/// the second `a`, but look-around sees past it, so `foo$` doesn't match `foo|bar`.
fn last_before(re: &Regex, text: &str, hi: usize) -> Option<Range<usize>> {
    let end = hi.min(text.len());
    // Where the leftmost match at or after `p` starts, if that's before `b`.
    let start_in = |p: usize, b: usize| {
        re.search(&Input::new(text).range(p..end))
            .map(|m| m.start())
            .filter(|&s| s < b)
    };
    // Look in windows that double leftward from `hi`, so a nearby match costs little...
    let (mut top, mut width) = (hi, 256);
    let mut a = loop {
        if top == 0 {
            return None;
        }
        let lo = top.saturating_sub(width);
        if let Some(s) = start_in(lo, top) {
            break s;
        }
        (top, width) = (lo, width * 2);
    };
    // ...then halve the window: a match starts at `a`, and none at or after `b`. Trying
    // each start instead is quadratic in a long run, where every start matches to its end.
    let mut b = top;
    while b - a > 1 {
        let mid = a + (b - a) / 2;
        match start_in(mid, b) {
            Some(s) => a = s,
            None => b = mid,
        }
    }
    let at_a = Input::new(text).range(a..end).anchored(Anchored::Yes);
    re.search(&at_a).map(|m| m.range())
}

impl TextBuf {
    /// The next match: going `Forward`, the first that starts after `from`; going
    /// `Backward`, the one that starts last among those that start before `from` and end
    /// by it, as in Emacs. When there's none, the search wraps around the buffer, to the
    /// first match or the last, which may be at `from` itself. Matches inside hidden text
    /// count; revealing them is the caller's (`Ctx::reveal`).
    pub fn search(&self, pat: &Pattern, from: usize, dir: Dir) -> Option<Found> {
        let text = self.flat();
        let (re, text) = (&pat.re, &*text);
        let from = from.min(text.len());
        let near = match dir {
            // Matches start on character boundaries, so `from + 1` works inside one too.
            Dir::Forward => (from < text.len())
                .then(|| re.search(&Input::new(text).range(from + 1..)))
                .flatten()
                .map(|m| m.range()),
            Dir::Backward => last_before(re, text, from),
        };
        if let Some(range) = near {
            return Some(Found {
                range,
                wrapped: false,
            });
        }
        let far = match dir {
            Dir::Forward => re.find(text).map(|m| m.range()),
            Dir::Backward => last_before(re, text, text.len() + 1),
        };
        far.map(|range| Found {
            range,
            wrapped: true,
        })
    }

    /// Every match that overlaps `range`, in order and without overlaps, for highlighting.
    /// Matching sees only the lines `range` touches, as Emacs's lazy highlighting stops at
    /// the window's end: a match that starts before `range` on its first line is found, but
    /// not one that starts on an earlier line or ends past the last line's newline.
    pub fn matches(&self, pat: &Pattern, range: Range<usize>) -> Vec<Range<usize>> {
        let (start, text, end) = self.lines(&range);
        let mut out = vec![];
        for m in pat.re.find_iter(Input::new(&text).range(..end - start)) {
            let m = start + m.start()..start + m.end();
            if m.start >= range.end && !(m.is_empty() && m.start == range.start) {
                break;
            }
            if m.end > range.start || (m.is_empty() && m.start >= range.start) {
                out.push(m);
            }
        }
        out
    }

    /// The match that starts at `pos`, seeing only its line as `matches` does. The scene
    /// draws it as the current match.
    pub(crate) fn match_at(&self, pat: &Pattern, pos: usize) -> Option<Range<usize>> {
        let pos = pos.min(self.len());
        let (start, text, end) = self.lines(&(pos..pos));
        let at = Input::new(&text)
            .range(pos - start..end - start)
            .anchored(Anchored::Yes);
        pat.re
            .search(&at)
            .map(|m| start + m.start()..start + m.end())
    }

    /// The lines `range` touches, for `matches` and `match_at`: where they start, their
    /// text and where they end. The text has one character more, so look-around at the
    /// end (`$`, `\b`) sees what follows it.
    fn lines(&self, range: &Range<usize>) -> (usize, String, usize) {
        let start = self.line_start(self.line_of(range.start));
        let end = self.line_start(self.line_of(range.end) + 1);
        let more = self
            .rope
            .byte_slice(end..)
            .chars()
            .next()
            .map_or(0, char::len_utf8);
        (
            start,
            String::from(self.rope.byte_slice(start..end + more)),
            end,
        )
    }

    /// The text in one piece, copied at the first search after an edit.
    fn flat(&self) -> Ref<'_, str> {
        if self.flat.borrow().is_none() {
            *self.flat.borrow_mut() = Some(String::from(&self.rope));
        }
        Ref::map(self.flat.borrow(), |f| f.as_deref().unwrap_or_default())
    }
}
