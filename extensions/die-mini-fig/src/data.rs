//! Tables: one columnar type of strings, numbers and times.
//!
//! A table is read from CSV or from a spec's inline rows, and each column's kind is inferred
//! from all of its cells: Number when every cell that isn't missing is a plain number, Time when
//! every one is an ISO time (read by jiff, a time without an offset in the local zone), else
//! Text. Every column keeps its cells as written, so a column of hours ("00", "01") can still be
//! a category, and a text column its numbers, so a chart can read past a stray "abc". Filter
//! and sort are written once, here, as is the dashboard's time range.

use crate::spec::{Fig, Keep, Source, Transform};
use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// A column's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    /// UTC seconds since the Unix epoch.
    Time,
}

/// A column.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub kind: Kind,
    /// Each cell as written, "" when missing.
    pub text: Vec<String>,
    /// Each cell's number or time; NaN when missing or not a number.
    pub num: Vec<f64>,
}

/// A table: columns of equal length.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub columns: Vec<Column>,
}

/// A figure's tables by their names in [`Fig::sources`], or why each can't be read.
pub type Tables = BTreeMap<String, Result<Table, String>>;

/// The cells that mean "no value", in any case.
const MISSING: [&str; 6] = ["", "NA", "N/A", "null", "NaN", "-"];

/// Whether a cell means "no value".
fn missing(s: &str) -> bool {
    MISSING.iter().any(|m| m.eq_ignore_ascii_case(s.trim()))
}

impl Column {
    /// A column from its cells as written, its kind inferred.
    pub fn new(name: &str, text: Vec<String>, zone: &TimeZone) -> Column {
        let present = || text.iter().filter(|s| !missing(s));
        let numbers = text.iter().map(|s| number(s).unwrap_or(f64::NAN));
        let (kind, num) = if present().all(|s| number(s).is_some()) {
            (Kind::Number, numbers.collect())
        } else if present().all(|s| time(s, zone).is_some()) {
            let at = |s: &String| time(s, zone).unwrap_or(f64::NAN);
            (Kind::Time, text.iter().map(at).collect())
        } else {
            (Kind::Text, numbers.collect())
        };
        let text = text
            .into_iter()
            .map(|s| if missing(&s) { String::new() } else { s });
        Column {
            name: name.to_string(),
            kind,
            text: text.collect(),
            num,
        }
    }

    /// Whether it holds numbers or times.
    pub fn numeric(&self) -> bool {
        self.kind != Kind::Text
    }

    /// Whether a chart reads it along a scale: numbers or times, or text that's mostly numbers
    /// (its stray cells gaps).
    pub fn numbers(&self) -> bool {
        let present = self.text.iter().filter(|s| !s.is_empty()).count();
        self.numeric() || self.num.iter().filter(|v| v.is_finite()).count() * 2 > present
    }

    /// Its finite values' least and greatest.
    pub fn extent(&self) -> Option<(f64, f64)> {
        extent(self.num.iter().copied())
    }

    /// Its distinct cells in the order they first come, missing ones left out: a band scale's
    /// categories, or a chart's series.
    pub fn distinct(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let cells = self.text.iter().filter(|s| !s.is_empty());
        let fresh = cells.filter(|s| seen.insert(s.as_str()));
        fresh.cloned().collect()
    }
}

impl Table {
    /// A CSV file.
    pub fn read(path: &Path) -> Result<Table, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Table::from_csv(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// CSV text: a header row, then rows of as many cells, quoted as RFC 4180 quotes them.
    pub fn from_csv(text: &str) -> Result<Table, String> {
        let mut records = csv(text).into_iter();
        let Some(header) = records.next() else {
            return Ok(Table::default());
        };
        let mut cells: Vec<Vec<String>> = vec![Vec::new(); header.len()];
        for (i, record) in records.enumerate() {
            if record.len() == 1 && record[0].is_empty() {
                continue;
            }
            if record.len() != header.len() {
                let (n, want) = (record.len(), header.len());
                return Err(format!("row {} has {n} cells, the header {want}", i + 2));
            }
            for (column, cell) in cells.iter_mut().zip(record) {
                column.push(cell);
            }
        }
        Ok(Table::of(header.iter().zip(cells)))
    }

    /// A spec's inline rows: JSON objects, their keys the columns in the order they first come.
    pub fn from_rows(rows: &[Value]) -> Table {
        let mut names: Vec<&str> = Vec::new();
        for key in rows
            .iter()
            .filter_map(Value::as_object)
            .flat_map(|o| o.keys())
        {
            if !names.contains(&key.as_str()) {
                names.push(key);
            }
        }
        let cell = |row: &Value, name: &str| match row.get(name) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
        };
        let columns = names
            .iter()
            .map(|&n| (n, rows.iter().map(|r| cell(r, n)).collect()));
        Table::of(columns)
    }

    /// Columns from names and cells, each kind inferred in the local zone.
    fn of<S: AsRef<str>>(columns: impl Iterator<Item = (S, Vec<String>)>) -> Table {
        let zone = TimeZone::system();
        let columns = columns.map(|(name, text)| Column::new(name.as_ref(), text, &zone));
        Table {
            columns: columns.collect(),
        }
    }

    /// How many rows.
    pub fn len(&self) -> usize {
        self.columns.first().map_or(0, |c| c.text.len())
    }

    /// Whether it has no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A column by name.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// A column by name, or an error that says it isn't there.
    pub fn need(&self, name: &str) -> Result<&Column, String> {
        self.column(name)
            .ok_or_else(|| format!("no column \"{name}\""))
    }

    /// The rows at `rows`, in that order.
    pub fn pick(&self, rows: &[usize]) -> Table {
        let column = |c: &Column| Column {
            name: c.name.clone(),
            kind: c.kind,
            text: rows.iter().map(|&r| c.text[r].clone()).collect(),
            num: rows.iter().map(|&r| c.num[r]).collect(),
        };
        Table {
            columns: self.columns.iter().map(column).collect(),
        }
    }

    /// The rows a spec's transforms leave, in their order.
    pub fn transform(&self, steps: &[Transform]) -> Result<Table, String> {
        let mut out = self.clone();
        for step in steps {
            out = match step {
                Transform::Filter { field, keep } => {
                    let c = out.need(field)?;
                    let rows: Vec<usize> = (0..out.len()).filter(|&r| kept(c, r, keep)).collect();
                    out.pick(&rows)
                }
                Transform::Sort { field, descending } => {
                    let c = out.need(field)?;
                    let mut rows: Vec<usize> = (0..out.len()).collect();
                    rows.sort_by(|&a, &b| order(c, a, b, *descending));
                    out.pick(&rows)
                }
            };
        }
        Ok(out)
    }

    /// The rows whose time in `field` is after `lo` and up to `hi`: a time range's. A table
    /// without that time column is kept whole.
    pub fn within(&self, field: &str, (lo, hi): (f64, f64)) -> Table {
        match self.column(field).filter(|c| c.kind == Kind::Time) {
            Some(c) => {
                let inside = |&r: &usize| c.num[r] > lo && c.num[r] <= hi;
                let rows: Vec<usize> = (0..self.len()).filter(inside).collect();
                self.pick(&rows)
            }
            None => self.clone(),
        }
    }
}

/// Every table a figure reads, or why it can't be read: one that can't spoils only its panels.
pub fn load(fig: &Fig) -> Tables {
    let read = |source: &Source| match source {
        Source::File(path) => Table::read(path),
        Source::Rows(rows) => Ok(Table::from_rows(rows)),
    };
    let tables = fig.sources.iter().map(|(name, s)| (name.clone(), read(s)));
    tables.collect()
}

/// The dashboard's time range: `secs` back from the newest time in its field in any table, as
/// (start, end]. None without a range or such a column.
pub fn window(fig: &Fig, tables: &Tables) -> Option<(f64, f64)> {
    let time = fig.time.as_ref()?;
    let secs = time.secs?;
    let columns = tables
        .values()
        .flatten()
        .filter_map(|t| t.column(&time.field));
    let times = columns.filter(|c| c.kind == Kind::Time);
    let newest = times
        .filter_map(Column::extent)
        .map(|(_, hi)| hi)
        .reduce(f64::max)?;
    Some((newest - secs, newest))
}

/// The least and greatest of the finite values.
pub fn extent(values: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    let finite = values.filter(|v| v.is_finite());
    finite.fold(None, |acc, v| match acc {
        None => Some((v, v)),
        Some((lo, hi)) => Some((v.min(lo), v.max(hi))),
    })
}

/// Whether row `r` of `c` passes a filter. A cell is in a list by its text, or by its number
/// when both are numbers ("5" is 5.0).
fn kept(c: &Column, r: usize, keep: &Keep) -> bool {
    let listed = |list: &[String]| {
        let n = c.num[r];
        list.iter()
            .any(|s| *s == c.text[r] || number(s).is_some_and(|v| v == n))
    };
    match keep {
        Keep::In(list) => listed(list),
        Keep::NotIn(list) => !listed(list),
        Keep::Range(lo, hi) => {
            let v = c.num[r];
            !v.is_nan() && lo.is_none_or(|lo| v >= lo) && hi.is_none_or(|hi| v <= hi)
        }
    }
}

/// Rows `a` and `b` in `c`'s order: by number or time, else by text; missing cells last either
/// way.
fn order(c: &Column, a: usize, b: usize, descending: bool) -> std::cmp::Ordering {
    let (x, y) = if descending { (b, a) } else { (a, b) };
    if c.numeric() {
        let (va, vb) = (c.num[a].is_nan(), c.num[b].is_nan());
        if va || vb {
            return va.cmp(&vb);
        }
        c.num[x].total_cmp(&c.num[y])
    } else {
        let (ea, eb) = (c.text[a].is_empty(), c.text[b].is_empty());
        if ea || eb {
            return ea.cmp(&eb);
        }
        c.text[x].cmp(&c.text[y])
    }
}

/// A cell's number: ASCII digits with an optional sign, point and exponent; never "inf",
/// "12%" or "$5".
pub fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    let b = s.as_bytes();
    let plain = b.iter().any(u8::is_ascii_digit)
        && matches!(b.first(), Some(b'0'..=b'9' | b'+' | b'-' | b'.'))
        && b.iter()
            .all(|c| matches!(c, b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E'));
    if !plain {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A cell's time in UTC seconds: an instant with an offset or `Z`, else a civil date and time,
/// or a date (its midnight), in `zone`.
pub fn time(s: &str, zone: &TimeZone) -> Option<f64> {
    let s = s.trim();
    if !s.starts_with(|c: char| c.is_ascii_digit()) || number(s).is_some() {
        return None;
    }
    let ts = if let Ok(ts) = s.parse::<Timestamp>() {
        ts
    } else if let Ok(dt) = s.parse::<DateTime>() {
        zone.to_timestamp(dt).ok()?
    } else {
        zone.to_timestamp(
            s.parse::<Date>()
                .ok()?
                .to_datetime(jiff::civil::Time::midnight()),
        )
        .ok()?
    };
    Some(ts.as_millisecond() as f64 / 1000.0)
}

/// CSV's records, each a list of cells: fields split at commas outside quotes, `""` a quote
/// inside them, records ended by a newline (CRLF too) outside quotes.
fn csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (mut records, mut record, mut cell) = (Vec::new(), Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                record.push(std::mem::take(&mut cell));
                records.push(std::mem::take(&mut record));
            }
            (false, c) => cell.push(c),
        }
    }
    if !cell.is_empty() || !record.is_empty() {
        record.push(cell);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_inferred_from_every_cell() {
        let t = Table::from_csv(
            "time,n,hour,day,s\n\
             2026-11-22T10:25Z,1.5,00,2026-11-16,\"sold, 24 h\"\r\n\
             2026-11-22T11:25:00Z,,01,2026-11-17,\"a \"\"b\"\"\"\n",
        )
        .unwrap();
        let kinds: Vec<Kind> = t.columns.iter().map(|c| c.kind).collect();
        use Kind::*;
        assert_eq!(kinds, [Time, Number, Number, Time, Text]);
        let time = t.need("time").unwrap();
        assert_eq!(time.num[1] - time.num[0], 3600.0);
        assert!(t.need("n").unwrap().num[1].is_nan());
        assert_eq!(t.need("hour").unwrap().text[0], "00");
        assert_eq!(t.need("s").unwrap().text, ["sold, 24 h", "a \"b\""]);
        // "n/a" in any case is missing; a stray "abc" leaves the rest numbers to a chart.
        let t = Table::from_csv("a,b\n5,5\nn/a,abc\n7,7\n").unwrap();
        let (a, b) = (t.need("a").unwrap(), t.need("b").unwrap());
        assert_eq!((a.kind, b.kind), (Number, Text));
        assert!(b.numbers() && b.num[0] == 5.0 && b.num[1].is_nan() && b.text[1] == "abc");
    }

    #[test]
    fn filter_and_sort_keep_the_rows_asked_for() {
        let t = Table::from_csv("g,v\na,3\nb,1\na,\nc,2\n").unwrap();
        let only = Keep::In(vec!["a".into(), "c".into()]);
        let steps = [
            Transform::Filter {
                field: "g".into(),
                keep: only,
            },
            Transform::Sort {
                field: "v".into(),
                descending: true,
            },
        ];
        let out = t.transform(&steps).unwrap();
        assert_eq!(out.need("g").unwrap().text, ["a", "c", "a"]);
        assert_eq!(out.need("v").unwrap().text, ["3", "2", ""]);
        assert!(
            t.transform(&[Transform::Sort {
                field: "x".into(),
                descending: false
            }])
            .is_err()
        );
    }
}
