//! Flex matching, as Emacs 30's `completion-styles '(flex)` ranks it: the query's characters in
//! order, each as early as it can be, scored by `completion--flex-score-1`. A hole of `L`
//! characters between two matched ones costs `1 + (L-1)^(1/3)`, holes before the first and after
//! the last cost nothing, and the score is `query / (candidate * (1 + holes))` in characters. Of
//! equal scores, one that matches in the query's case comes first, and then the order they came in.
//!
//! The one difference is case. Emacs matches it exactly by default. Here a query with no capitals
//! matches either case, and one with capitals matches exactly, as `smartcase` does.

/// `flex-score-match-tightness`.
const TIGHTNESS: f64 = 3.0;

/// Whether two characters match. A finder does this tens of millions of times for one key, so
/// ASCII doesn't go through Unicode's tables.
#[inline]
fn same(fold: bool, c: char, q: char) -> bool {
    c == q
        || fold
            && match c.is_ascii() && q.is_ascii() {
                true => c.eq_ignore_ascii_case(&q),
                false => c.to_lowercase().eq(q.to_lowercase()),
            }
}

/// Call `each` with the character index of each character of `query` in `candidate`, and return
/// the candidate's length in characters, or None if the query isn't all there.
fn walk(fold: bool, query: &str, candidate: &str, mut each: impl FnMut(usize)) -> Option<usize> {
    let mut want = query.chars().peekable();
    let mut len = 0;
    for (i, c) in candidate.chars().enumerate() {
        len = i + 1;
        if want.peek().is_some_and(|&q| same(fold, c, q)) {
            want.next();
            each(i);
        }
    }
    want.peek().is_none().then_some(len)
}

/// Whether this query ignores case: it has no capitals.
fn folds(query: &str) -> bool {
    !query.chars().any(char::is_uppercase)
}

fn score(query: &str, candidate: &str) -> Option<f64> {
    let (mut count, mut holes, mut last) = (0usize, 0.0, None);
    let len = walk(folds(query), query, candidate, |i| {
        if let Some(l) = last.filter(|&l: &usize| l + 1 < i) {
            holes += 1.0 + ((i - l - 2) as f64).powf(1.0 / TIGHTNESS);
        }
        count += 1;
        last = Some(i);
    })?;
    Some(count as f64 / (len as f64 * (1.0 + holes)))
}

/// Which characters of `candidate` the query matched, by index. Empty if it doesn't match.
pub(crate) fn positions(query: &str, candidate: &str) -> Vec<usize> {
    let mut at = vec![];
    match walk(folds(query), query, candidate, |i| at.push(i)) {
        Some(_) => at,
        None => vec![],
    }
}

/// How good a match this is: its score, and whether it matches in the query's case. None if it
/// doesn't match.
pub(crate) fn rate(query: &str, candidate: &str) -> Option<(f64, bool)> {
    let score = score(query, candidate)?;
    Some((score, walk(false, query, candidate, |_| {}).is_some()))
}

/// The indices of the candidates that match, best first. An empty query keeps them all in order.
pub(crate) fn rank<'a>(query: &str, candidates: impl Iterator<Item = &'a str>) -> Vec<usize> {
    if query.is_empty() {
        return candidates.enumerate().map(|(i, _)| i).collect();
    }
    let mut scored: Vec<(f64, bool, usize)> = candidates
        .enumerate()
        .filter_map(|(i, c)| {
            let (score, in_case) = rate(query, c)?;
            Some((score, in_case, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    scored.into_iter().map(|(_, _, i)| i).collect()
}

/// The candidates that match `query`, best first: what a prompt's list shows.
pub fn flex_rank<'a>(query: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let order = rank(query, candidates.iter().copied());
    order.into_iter().map(|i| candidates[i]).collect()
}
