//! CSV and TSV: quoted fields with `""` escapes and line breaks inside them, a delimiter sniffed
//! from the first line unless given, and a header row unless told otherwise. Each column's type
//! is inferred from all of its cells ([`super::infer`]).

use super::{ReadOptions, Table};
use crate::error::Result;

/// A table from CSV or TSV text.
pub fn read_csv(_text: &str, _opt: &ReadOptions) -> Result<Table> {
    todo!("P1-data: read_csv")
}
