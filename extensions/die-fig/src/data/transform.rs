//! Transforms: a table reshaped before it's drawn, in the order a spec lists them.

use super::Table;
use crate::error::Result;

/// One reshaping of a table.
#[derive(Clone, Debug, PartialEq)]
pub enum Transform {
    /// Wide to long: `fields` become rows of `key` (the field's name) and `value`.
    Fold {
        /// The columns folded.
        fields: Vec<String>,
        /// The new column of their names.
        key: String,
        /// The new column of their values.
        value: String,
    },
    /// Only the rows whose `field` passes `op`.
    Filter {
        /// The column tested.
        field: String,
        /// The test.
        op: FilterOp,
    },
    /// `field` into bins, each row's bin as `start` and `end`.
    Bin {
        /// The column binned.
        field: String,
        /// The bins' width; None picks a nice one for `max_bins`.
        step: Option<f64>,
        /// The most bins a picked step makes.
        max_bins: usize,
        /// The new column of each bin's start.
        start: String,
        /// The new column of each bin's end.
        end: String,
    },
    /// Rows grouped `by` columns into one each, `field` aggregated by `op` into `out`.
    Aggregate {
        /// How.
        op: AggOp,
        /// The column aggregated; None only for `Count`.
        field: Option<String>,
        /// The columns grouped by.
        by: Vec<String>,
        /// The new column.
        out: String,
    },
    /// The running total of `field` within each group, into `out`.
    Cumulative {
        /// The column summed.
        field: String,
        /// The columns grouped by.
        by: Vec<String>,
        /// The new column.
        out: String,
    },
    /// The trailing mean of `size` rows of `field` within each group, into `out`.
    Window {
        /// The column averaged.
        field: String,
        /// How many rows, this one included.
        size: usize,
        /// The columns grouped by.
        by: Vec<String>,
        /// The new column.
        out: String,
    },
    /// Rows sorted by `field`.
    Sort {
        /// The column sorted by.
        field: String,
        /// Largest first.
        descending: bool,
    },
    /// The `n` groups of `by` with the largest total `field`; the rest summed into "Other".
    Top {
        /// How many groups are kept.
        n: usize,
        /// The column whose values are the groups.
        by: String,
        /// The column summed to rank them.
        field: String,
    },
}

/// A filter's test.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterOp {
    /// The value is one of these.
    In(Vec<String>),
    /// The value is none of these.
    NotIn(Vec<String>),
    /// The number is at least the first and at most the second, where given.
    Range(Option<f64>, Option<f64>),
}

/// How an aggregate makes one value of many.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggOp {
    /// How many rows.
    Count,
    /// Their sum.
    Sum,
    /// Their mean.
    Mean,
    /// The least.
    Min,
    /// The most.
    Max,
    /// The median.
    P50,
    /// The 95th percentile.
    P95,
}

/// The table after each transform in turn.
pub fn apply(_table: &Table, _ts: &[Transform]) -> Result<Table> {
    todo!("P1-data: apply")
}
