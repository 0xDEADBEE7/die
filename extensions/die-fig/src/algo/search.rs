//! fig's tiered search score (briefs/fig-port.md §6), for `PromptSpec::tiers`: lower is better.

/// The tier a label is in for a query: exact, prefix, word prefix, substring, subsequence, or
/// none.
pub fn tier(_query: &str, _label: &str) -> usize {
    todo!("P1-algo: tier")
}
