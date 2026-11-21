//! JSON and JSONL rows, read through [`crate::json`] so columns keep the file's order: an array
//! of objects, an object of arrays, or an array of arrays with a header row; and JSONL, an object
//! a line. A row's `fig` block goes to [`Table::styles`] and its object to [`Table::raw`].

use super::{ReadOptions, Table};
use crate::error::Result;
use crate::json::Node;

/// A table from a JSON value: an array of objects, an object of arrays, or an array of arrays
/// whose first is the header.
pub fn read_json(_node: &Node, _opt: &ReadOptions) -> Result<Table> {
    todo!("P1-data: read_json")
}

/// A table from JSONL text: an object a line, blank lines skipped.
pub fn read_jsonl(_text: &str, _opt: &ReadOptions) -> Result<Table> {
    todo!("P1-data: read_jsonl")
}
