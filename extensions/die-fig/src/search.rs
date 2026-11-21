//! Search's model: a figure's names (series, categories, nodes, legend entries, each with the
//! [`Item`] it names) ranked against a query with fig's fuzzy match (briefs/fig-port.md §6,
//! [`crate::algo::search::score`]), the matched characters for highlighting, and `n` and `N`
//! over the matches. The keys and the prompt are the surface's; this is the model under them.
//!
//! Ranking is fig's whole: exact, then prefix (shorter labels first), then substring (earlier
//! first), then subsequence (fewer bytes skipped first), ties by the label's own order (byte
//! order, so "Zed" comes before "alpha"), equal labels in the order they were given. An empty
//! query matches every name, in that order.

use crate::algo::search::score;
use crate::detail::Item;
use std::ops::Range;

/// One name a search found.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    /// Its index among the names given.
    pub index: usize,
    /// The name.
    pub label: String,
    /// The mark it names.
    pub item: Item,
    /// fig's score: the tier (0 exact, 1 prefix, 2 substring, 3 subsequence) and the
    /// tie-break within it. Lower ranks first.
    pub score: (usize, usize),
    /// The matched characters, as byte ranges of `label` on character boundaries, in order and
    /// apart: what a highlight draws.
    pub ranges: Vec<Range<usize>>,
}

/// A search over a figure's names: the query, its matches in rank order, and where `n` and
/// `N` are among them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Search {
    names: Vec<(String, Item)>,
    query: String,
    matches: Vec<Match>,
    current: Option<usize>,
}

impl Search {
    /// A search over `names`, with no query yet (every name matches).
    pub fn new(names: Vec<(String, Item)>) -> Search {
        let mut s = Search {
            names,
            ..Search::default()
        };
        s.set_query("");
        s
    }

    /// A search over `names` for `query`.
    pub fn with(names: Vec<(String, Item)>, query: &str) -> Search {
        let mut s = Search::new(names);
        s.set_query(query);
        s
    }

    /// The names it searches.
    pub fn names(&self) -> &[(String, Item)] {
        &self.names
    }

    /// Ranks the names against `query` (fig ranks again at every key) and puts `n`'s place on
    /// the best match, where the cursor goes as you type.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        let mut out: Vec<Match> = self
            .names
            .iter()
            .enumerate()
            .filter_map(|(index, (label, item))| {
                let score = score(label, query)?;
                Some(Match {
                    index,
                    label: label.clone(),
                    item: item.clone(),
                    score,
                    ranges: ranges(label, query),
                })
            })
            .collect();
        // Stable: equal labels keep the order they were given in.
        out.sort_by(|a, b| a.score.cmp(&b.score).then_with(|| a.label.cmp(&b.label)));
        self.matches = out;
        self.current = (!self.matches.is_empty()).then_some(0);
    }

    /// The query.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The matches, best first.
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// The best match.
    pub fn best(&self) -> Option<&Match> {
        self.matches.first()
    }

    /// The match `n` and `N` step from: the best after a query, then wherever they went.
    pub fn current(&self) -> Option<&Match> {
        self.matches.get(self.current?)
    }

    /// `n`: the match after the current one, the first after the last.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&Match> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        let i = self.current.map_or(0, |c| (c + 1) % n);
        self.current = Some(i);
        self.matches.get(i)
    }

    /// `N`: the match before the current one, the last before the first.
    pub fn prev(&mut self) -> Option<&Match> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        let i = self.current.map_or(n - 1, |c| (c + n - 1) % n);
        self.current = Some(i);
        self.matches.get(i)
    }

    /// Puts `n`'s place on the match naming `item` ([`Item::same`]), when the figure's cursor
    /// went there some other way; its rank, or None (the place unchanged) when no match
    /// names it.
    pub fn select(&mut self, item: &Item) -> Option<usize> {
        let i = self.matches.iter().position(|m| m.item.same(item))?;
        self.current = Some(i);
        Some(i)
    }

    /// The match that names `item`, for its highlight.
    pub fn match_of(&self, item: &Item) -> Option<&Match> {
        self.matches.iter().find(|m| m.item.same(item))
    }
}

/// The characters of `label` that fig's match of `query` covers, as byte ranges on character
/// boundaries, merged where they touch: the whole label when exact, its start for a prefix,
/// the first occurrence for a substring, and the leftmost characters a subsequence takes.
/// Case is folded as fig folds it (the whole string lowercased); empty when nothing matches or
/// the query is empty.
pub fn ranges(label: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let lower = label.to_lowercase();
    let q = query.to_lowercase();
    // Each of the label's characters and where its lowercase starts in `lower`: lowercasing a
    // character alone takes the bytes it takes in the whole (a final sigma is σ or ς, both two
    // bytes), so the offsets agree.
    let mut starts: Vec<(usize, Range<usize>)> = Vec::new();
    let mut at = 0;
    for (i, c) in label.char_indices() {
        starts.push((at, i..i + c.len_utf8()));
        at += c.to_lowercase().map(char::len_utf8).sum::<usize>();
    }
    if at != lower.len() {
        return Vec::new();
    }
    // The label's characters a lowercased byte range covers.
    let cover = |a: usize, b: usize| -> Option<Range<usize>> {
        let first = starts.iter().rposition(|(s, _)| *s <= a)?;
        let last = starts.iter().rposition(|(s, _)| *s < b)?;
        Some(starts[first].1.start..starts[last].1.end)
    };
    let found: Vec<(usize, usize)> = if lower == q {
        vec![(0, lower.len())]
    } else if lower.starts_with(&q) {
        vec![(0, q.len())]
    } else if let Some(i) = lower.find(&q) {
        vec![(i, i + q.len())]
    } else {
        let mut pos = 0;
        let mut out = Vec::new();
        for c in q.chars() {
            let Some(f) = lower[pos..].find(c) else {
                return Vec::new();
            };
            out.push((pos + f, pos + f + c.len_utf8()));
            pos += f + c.len_utf8();
        }
        out
    };
    let mut merged: Vec<Range<usize>> = Vec::new();
    for (a, b) in found {
        let Some(r) = cover(a, b) else { continue };
        match merged.last_mut() {
            Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}
