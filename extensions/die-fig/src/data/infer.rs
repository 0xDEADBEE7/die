//! What a column is, from all of its cells: Number if every cell that isn't missing parses as
//! one, Time if every one parses as a time (ruling 2), Category if there are no more distinct
//! strings than [`ReadOptions::category_limit`], else Text. A missing cell is `None` or one of
//! [`MISSING`].

use super::{Column, ReadOptions};

/// The cells that mean "no value".
pub const MISSING: [&str; 6] = ["", "NA", "N/A", "null", "-", "NaN"];

/// A column from its name and its cells, its type inferred.
pub fn infer(_name: &str, _cells: &[Option<&str>], _opt: &ReadOptions) -> Column {
    todo!("P1-data: infer")
}
