//! fig's tiered search score (briefs/fig-port.md §6), for `PromptSpec::tiers`: lower is better.
//!
//! `/` asks with a DIE prompt over the figure's labels, which flex matches; [`tier`] then puts
//! fig's order on top: exact, then prefix, then substring, then subsequence, each tier in flex's
//! order. [`score`] is fig's function whole, with its tie-break inside a tier.

/// The tier for a label that fig's search filters out: after every tier fig ranks. [`tier`]
/// folds case a character at a time, as flex does, so a label flex keeps never lands here.
pub const UNMATCHED: usize = 4;

/// The tier a label is in for a query, for `PromptSpec::tiers(|q, l| tier(q, l))`: 0 exact (and
/// every label for an empty query), 1 prefix, 2 substring, 3 subsequence, [`UNMATCHED`] none.
/// Case is ignored, each character lowercased on its own as flex compares them, where fig
/// lowercases whole strings: "ΟΔΟΣ" holds "σ" here (a substring), though fig's lowercasing ends
/// it in a final "ς" and [`score`] filters it out.
pub fn tier(query: &str, label: &str) -> usize {
    let fold = |s: &str| s.chars().flat_map(char::to_lowercase).collect::<String>();
    rank(&fold(label), &fold(query)).map_or(UNMATCHED, |(t, _)| t)
}

/// fig's score, verbatim: `(tier, tie-break)`, lower first, None when filtered out. Within the
/// prefix tier the tie-break is the lowercased label's length in bytes; within substring, the
/// byte offset of the first match; within subsequence, the bytes skipped over, counting those
/// before the first match.
pub fn score(label: &str, query: &str) -> Option<(usize, usize)> {
    rank(&label.to_lowercase(), &query.to_lowercase())
}

/// fig's tiers over a label and query already lowercased.
fn rank(label: &str, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }
    if label == query {
        return Some((0, 0));
    }
    if label.starts_with(query) {
        return Some((1, label.len()));
    }
    if let Some(at) = label.find(query) {
        return Some((2, at));
    }
    let (mut pos, mut gaps) = (0, 0);
    for c in query.chars() {
        let f = label[pos..].find(c)?;
        gaps += f;
        pos += f + c.len_utf8();
    }
    Some((3, gaps))
}
