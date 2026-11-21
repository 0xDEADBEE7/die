//! A figure's data: a [`Table`] of typed [`Column`]s, read from CSV, TSV, JSON or JSONL, and
//! the transforms that reshape it.
//!
//! - [`csv`]: CSV and TSV;
//! - [`rows`]: JSON (an array of objects, an object of arrays, an array of arrays with a header
//!   row) and JSONL, through [`crate::json`] so columns keep the file's order;
//! - [`infer`]: what each column is, from all of its cells;
//! - [`time`]: times parsed in an injected zone (ruling 2);
//! - [`transform`]: fold, filter, bin, aggregate, cumulative, window, sort and top.
//!
//! Data is `f64`. Times are seconds since the Unix epoch, UTC. A table and everything in it is
//! `Send + Sync`, so readers run in jobs off the main thread.

pub mod csv;
pub mod infer;
pub mod rows;
pub mod time;
pub mod transform;

use crate::error::{FigError, Result};
use crate::json::Node;
use crate::style::FigStyle;
use std::collections::BTreeMap;
use std::sync::Arc;

/// What a column holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    /// Numbers.
    Number,
    /// Times.
    Time,
    /// A few distinct strings.
    Category,
    /// Strings.
    Text,
}

/// A column's values.
#[derive(Clone, Debug, PartialEq)]
pub enum Values {
    /// Numbers; NaN is missing.
    Number(Arc<[f64]>),
    /// Times in UTC seconds since the epoch; NaN is missing.
    Time(Arc<[f64]>),
    /// Strings from a small set: each row's code into `dict`, `u32::MAX` missing. `dict` is in
    /// first-seen order.
    Category {
        /// Each row's code.
        codes: Arc<[u32]>,
        /// The distinct strings, as first seen.
        dict: Arc<[Box<str>]>,
    },
    /// Strings.
    Text(Arc<[Box<str>]>),
}

/// A named column.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    /// Its name: a header, a key.
    pub name: Box<str>,
    /// Its values.
    pub values: Values,
}

/// Columns of equal length, with unique names.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Table {
    /// The columns, in document order.
    pub columns: Vec<Column>,
    /// How many rows each column has.
    pub rows: usize,
    /// Each row's `fig` block, when the rows came from JSON objects that had one.
    pub styles: Option<Arc<[FigStyle]>>,
    /// Each row's source object, for the detail panel, when the rows came from JSON.
    pub raw: Option<Arc<[Node]>>,
}

/// A series' identity: the nth series (from 0) with this display name, in source order
/// (ruling 3). Hidden sets, colour slots, the cursor, undo and the session key on it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SeriesKey {
    /// Its display name.
    pub name: Box<str>,
    /// Which of the series with that name, from 0.
    pub nth: u32,
}

impl SeriesKey {
    /// The nth series named `name`.
    pub fn new(name: &str, nth: u32) -> SeriesKey {
        SeriesKey {
            name: name.into(),
            nth,
        }
    }
}

impl Column {
    /// What it holds.
    pub fn kind(&self) -> ColumnKind {
        match self.values {
            Values::Number(_) => ColumnKind::Number,
            Values::Time(_) => ColumnKind::Time,
            Values::Category { .. } => ColumnKind::Category,
            Values::Text(_) => ColumnKind::Text,
        }
    }

    /// A column of numbers, NaN missing.
    pub fn number(name: &str, v: Vec<f64>) -> Column {
        Column {
            name: name.into(),
            values: Values::Number(v.into()),
        }
    }

    /// A column of times in UTC seconds, NaN missing.
    pub fn time(name: &str, v: Vec<f64>) -> Column {
        Column {
            name: name.into(),
            values: Values::Time(v.into()),
        }
    }

    /// A category column: each string's code, the distinct strings in the order first seen.
    /// Every string is a value, the empty one too.
    pub fn category(name: &str, v: &[&str]) -> Column {
        let mut dict: Vec<Box<str>> = Vec::new();
        let mut index: BTreeMap<&str, u32> = BTreeMap::new();
        let codes: Vec<u32> = v
            .iter()
            .map(|s| {
                *index.entry(*s).or_insert_with(|| {
                    dict.push((*s).into());
                    (dict.len() - 1) as u32
                })
            })
            .collect();
        Column {
            name: name.into(),
            values: Values::Category {
                codes: codes.into(),
                dict: dict.into(),
            },
        }
    }

    /// A column of strings.
    pub fn text(name: &str, v: &[&str]) -> Column {
        let v: Vec<Box<str>> = v.iter().map(|s| (*s).into()).collect();
        Column {
            name: name.into(),
            values: Values::Text(v.into()),
        }
    }

    /// How many rows it has.
    pub fn len(&self) -> usize {
        match &self.values {
            Values::Number(v) | Values::Time(v) => v.len(),
            Values::Category { codes, .. } => codes.len(),
            Values::Text(v) => v.len(),
        }
    }

    /// Whether it has no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A row's number or time, NaN for a missing one, another kind of column, or a row past
    /// the end.
    pub fn f64_at(&self, row: usize) -> f64 {
        match &self.values {
            Values::Number(v) | Values::Time(v) => v.get(row).copied().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }

    /// A row's string in a category or text column. None for a missing one, another kind of
    /// column, or a row past the end.
    pub fn str_at(&self, row: usize) -> Option<&str> {
        match &self.values {
            Values::Category { codes, dict } => {
                let code = *codes.get(row)?;
                dict.get(code as usize).map(|s| &**s)
            }
            Values::Text(v) => v.get(row).map(|s| &**s),
            _ => None,
        }
    }
}

impl Table {
    /// A table of columns, which must be of equal length and have unique names.
    pub fn new(columns: Vec<Column>) -> Result<Table> {
        let rows = columns.first().map_or(0, Column::len);
        for (i, c) in columns.iter().enumerate() {
            if columns[..i].iter().any(|o| o.name == c.name) {
                return Err(FigError::new(format!(
                    "two columns are named \"{}\"",
                    c.name
                )));
            }
            if c.len() != rows {
                return Err(FigError::new(format!(
                    "column \"{}\" has {} rows where \"{}\" has {rows}",
                    c.name,
                    c.len(),
                    columns[0].name
                )));
            }
        }
        Ok(Table {
            columns,
            rows,
            styles: None,
            raw: None,
        })
    }

    /// The column named `name`.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| &*c.name == name)
    }

    /// Where the column named `name` is.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| &*c.name == name)
    }
}

/// How readers interpret cells.
#[derive(Clone, Debug)]
pub struct ReadOptions {
    /// The zone a time without an offset is in.
    pub zone: jiff::tz::TimeZone,
    /// The delimiter; None sniffs ',', '\t' and ';' from the first line.
    pub delimiter: Option<u8>,
    /// Whether the first row is a header (true by default).
    pub header: bool,
    /// A thousands separator; None means "1,234" is not a number.
    pub thousands: Option<char>,
    /// A strtime format for every time column.
    pub time_format: Option<String>,
    /// More distinct strings than this (50 by default) make a text column, not a category.
    pub category_limit: usize,
}

impl ReadOptions {
    /// The defaults in a zone: sniff the delimiter, a header row, no thousands separator, no
    /// time format, and up to 50 categories.
    pub fn new(zone: jiff::tz::TimeZone) -> ReadOptions {
        ReadOptions {
            zone,
            delimiter: None,
            header: true,
            thousands: None,
            time_format: None,
            category_limit: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_sync<T: Send + Sync>() {}

    #[test]
    fn a_table_and_its_options_go_to_jobs() {
        send_sync::<Table>();
        send_sync::<ReadOptions>();
        let opt = ReadOptions::new(jiff::tz::TimeZone::UTC);
        assert_eq!(
            (
                opt.delimiter,
                opt.header,
                opt.thousands,
                opt.time_format,
                opt.category_limit
            ),
            (None, true, None, None, 50)
        );
    }

    #[test]
    fn a_table_needs_equal_lengths_and_unique_names() {
        let t = Table::new(vec![
            Column::time("time", vec![0.0, 60.0, 120.0]),
            Column::number("api", vec![1.0, f64::NAN, 3.0]),
            Column::category("host", &["a", "b", "a"]),
            Column::text("note", &["x", "y", "z"]),
        ])
        .unwrap();
        assert_eq!(t.rows, 3);
        assert_eq!((t.styles.clone(), t.raw.clone()), (None, None));
        let kinds: Vec<ColumnKind> = t.columns.iter().map(Column::kind).collect();
        use ColumnKind::*;
        assert_eq!(kinds, [Time, Number, Category, Text]);
        assert_eq!(t.index("host"), Some(2));
        assert_eq!(t.column("api").map(|c| c.len()), Some(3));
        assert_eq!(t.index("none"), None);
        assert!(t.column("none").is_none());

        let short = Table::new(vec![
            Column::number("a", vec![1.0, 2.0]),
            Column::number("b", vec![1.0]),
        ]);
        assert_eq!(
            short.unwrap_err().to_string(),
            "column \"b\" has 1 rows where \"a\" has 2"
        );
        let twice = Table::new(vec![
            Column::number("a", vec![1.0]),
            Column::text("a", &["x"]),
        ]);
        assert_eq!(
            twice.unwrap_err().to_string(),
            "two columns are named \"a\""
        );

        let empty = Table::new(vec![]).unwrap();
        assert_eq!((empty.rows, empty.columns.len()), (0, 0));
        assert!(Column::number("n", vec![]).is_empty());
    }

    #[test]
    fn categories_are_coded_in_first_seen_order() {
        let c = Column::category("host", &["web", "db", "web", "", "cache", "db"]);
        let Values::Category { codes, dict } = &c.values else {
            panic!("{c:?}")
        };
        assert_eq!(&**codes, [0, 1, 0, 2, 3, 1]);
        let dict: Vec<&str> = dict.iter().map(|s| &**s).collect();
        assert_eq!(dict, ["web", "db", "", "cache"]);
        assert_eq!(c.len(), 6);
        assert_eq!(c.str_at(4), Some("cache"));
        assert_eq!(c.str_at(3), Some(""));
        assert_eq!(c.str_at(6), None);
        // A missing code is no string.
        let missing = Column {
            name: "m".into(),
            values: Values::Category {
                codes: vec![0, u32::MAX].into(),
                dict: vec!["a".into()].into(),
            },
        };
        assert_eq!((missing.str_at(0), missing.str_at(1)), (Some("a"), None));
    }

    #[test]
    fn values_by_row() {
        let n = Column::number("n", vec![1.5, f64::NAN]);
        assert_eq!(n.f64_at(0), 1.5);
        assert!(n.f64_at(1).is_nan());
        assert!(n.f64_at(2).is_nan());
        assert_eq!(n.str_at(0), None);
        let t = Column::time("t", vec![86400.0]);
        assert_eq!(t.f64_at(0), 86400.0);
        let s = Column::text("s", &["a", "é"]);
        assert_eq!((s.str_at(1), s.str_at(2)), (Some("é"), None));
        assert!(s.f64_at(0).is_nan());
        let c = Column::category("c", &["x"]);
        assert!(c.f64_at(0).is_nan());
    }

    #[test]
    fn series_keys_order_by_name_then_nth() {
        let mut keys = vec![
            SeriesKey::new("b", 0),
            SeriesKey::new("a", 1),
            SeriesKey::new("a", 0),
        ];
        keys.sort();
        assert_eq!(
            keys,
            [
                SeriesKey::new("a", 0),
                SeriesKey::new("a", 1),
                SeriesKey::new("b", 0)
            ]
        );
    }
}
