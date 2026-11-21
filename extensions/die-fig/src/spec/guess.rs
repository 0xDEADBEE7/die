//! A figure from a bare table, by the design's rules: a time or ordered number column is x and
//! the other numeric columns are series, as lines; a category x gives bars; two unordered
//! numbers a scatter; a single number column a histogram.

use super::Chart;
use crate::data::Table;
use crate::error::Result;

/// The chart a table's columns suggest.
pub fn guess(_table: &Table) -> Result<Chart> {
    todo!("P1-spec: guess")
}
