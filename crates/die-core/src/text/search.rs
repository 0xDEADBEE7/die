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
    /// A `word` pattern: a match stands only where its neighbours aren't word characters.
    is_word: Option<fn(char) -> bool>,
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
            is_word: None,
        })
    }

    /// A literal pattern for `*` and `#`: `text` matches only where the characters on either
    /// side aren't word characters by `is_word` (Evil's in fundamental-mode: alphanumerics, not
    /// `_`, so `foo` matches inside `foo_bar` but not `foobar`). Rust's `\b` can't say this,
    /// since its word characters include `_`. Errors: `BadPattern`.
    pub fn word(text: &str, ignore_case: bool, is_word: fn(char) -> bool) -> Result<Pattern> {
        Ok(Pattern {
            is_word: Some(is_word),
            ..Pattern::new(&Pattern::escape(text), ignore_case)?
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

    /// Whether a match counts: for a `word` pattern, its neighbours aren't word characters.
    fn stands_alone(&self, text: &str, m: &Range<usize>) -> bool {
        self.is_word.is_none_or(|is_word| {
            !text[..m.start].chars().next_back().is_some_and(is_word)
                && !text[m.end..].chars().next().is_some_and(is_word)
        })
    }

    /// The first match that counts, starting in `at..end`. A match that doesn't count can
    /// overlap one that does, so the search goes on from the next character after its start.
    fn first_from(&self, text: &str, mut at: usize, end: usize) -> Option<Range<usize>> {
        while at <= end {
            let m = self.re.search(&Input::new(text).range(at..end))?.range();
            if self.stands_alone(text, &m) {
                return Some(m);
            }
            at = after_start(text, &m);
        }
        None
    }

    /// Every match that counts, starting in `..end`, left to right and without overlaps. It
    /// finds them as they're asked for, so stopping early doesn't scan a long line to its end.
    fn all<'a>(&'a self, text: &'a str, end: usize) -> Box<dyn Iterator<Item = Range<usize>> + 'a> {
        if self.is_word.is_none() {
            let input = Input::new(text).range(..end);
            return Box::new(self.re.find_iter(input).map(|m| m.range()));
        }
        let mut at = 0;
        Box::new(std::iter::from_fn(move || {
            let m = self.first_from(text, at, end)?;
            at = if m.is_empty() {
                after_start(text, &m)
            } else {
                m.end
            };
            Some(m)
        }))
    }

    /// `last_before`, skipping matches that don't count and looking further left for one.
    fn last_from(&self, text: &str, mut hi: usize) -> Option<Range<usize>> {
        loop {
            let m = last_before(&self.re, text, hi)?;
            if self.stands_alone(text, &m) {
                return Some(m);
            }
            // The next match must start before `hi`, so an empty one looks from where it is.
            hi = if m.is_empty() {
                m.start
            } else {
                m.end.checked_sub(1)?
            };
        }
    }
}

/// Just past the character a match starts on.
fn after_start(text: &str, m: &Range<usize>) -> usize {
    m.start + text[m.start..].chars().next().map_or(1, char::len_utf8)
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
        let text = &*text;
        let from = from.min(text.len());
        let near = match dir {
            // Matches start on character boundaries, so `from + 1` works inside one too.
            Dir::Forward => (from < text.len())
                .then(|| pat.first_from(text, from + 1, text.len()))
                .flatten(),
            Dir::Backward => pat.last_from(text, from),
        };
        if let Some(range) = near {
            return Some(Found {
                range,
                wrapped: false,
            });
        }
        let far = match dir {
            Dir::Forward => pat.first_from(text, 0, text.len()),
            Dir::Backward => pat.last_from(text, text.len() + 1),
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
        for m in pat.all(&text, end - start) {
            let m = start + m.start..start + m.end;
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
            .map(|m| m.range())
            .filter(|m| pat.stands_alone(&text, m))
            .map(|m| start + m.start..start + m.end)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The scene draws this as the current match, so it's one `n` can land on.
    #[test]
    fn the_match_at_a_position_follows_a_word_patterns_rule() {
        let t = TextBuf::new("foo foobar\nfoo_bar\n");
        let word = Pattern::word("foo", false, char::is_alphanumeric).unwrap();
        assert_eq!(t.match_at(&word, 0), Some(0..3));
        assert_eq!(t.match_at(&word, 4), None, "inside foobar");
        assert_eq!(t.match_at(&word, 11), Some(11..14), "before the _");
        let plain = Pattern::new("foo", false).unwrap();
        assert_eq!(t.match_at(&plain, 4), Some(4..7));
    }
}
