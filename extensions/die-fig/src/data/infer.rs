//! What a column is, from all of its cells: Number if every cell that isn't missing parses as
//! one, Time if every one parses as a time (ruling 2), Category if there are no more distinct
//! strings than [`ReadOptions::category_limit`], else Text. A missing cell is `None` or one of
//! [`MISSING`].
//!
//! - A column with no values at all is a Number column of NaN: it draws nothing and gaps.
//! - With a [`ReadOptions::time_format`], a column whose every value matches the format is Time
//!   even when its cells look like numbers (`"%Y%m%d"` over `20261121`): the format says so.
//! - A number is ASCII digits with an optional sign, point and exponent (`-1.5`, `.5`, `1e3`);
//!   `inf`, `12%` and `$5` are not numbers. A thousands separator only with
//!   [`ReadOptions::thousands`], in whole groups of three (`1,234,567`); a `.` separator makes
//!   `,` the decimal mark (`1.234,5`).
//! - A missing cell is NaN in a Number or Time column, `u32::MAX` in a Category and the empty
//!   string in Text.

use super::time::{parse_time, with_format};
use super::{Column, ReadOptions, Values};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// The cells that mean "no value".
pub const MISSING: [&str; 6] = ["", "NA", "N/A", "null", "-", "NaN"];

/// Whether a cell means "no value", spaces around it aside.
pub fn is_missing(s: &str) -> bool {
    MISSING.contains(&s.trim())
}

/// A cell's number, spaces around it aside, if it is one: finite, ASCII, with `thousands`
/// between whole groups of three if given.
pub fn parse_number(s: &str, thousands: Option<char>) -> Option<f64> {
    let s = s.trim();
    let plain: Cow<str> = match thousands {
        Some(sep) if s.contains(sep) || (sep == '.' && s.contains(',')) => {
            Cow::Owned(ungroup(s, sep)?)
        }
        _ => Cow::Borrowed(s),
    };
    let b = plain.as_bytes();
    let ok = b.iter().any(u8::is_ascii_digit)
        && matches!(b.first(), Some(b'0'..=b'9' | b'+' | b'-' | b'.'))
        && b.iter()
            .all(|c| matches!(c, b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E'));
    if !ok {
        return None;
    }
    plain.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `1,234.5` as `1234.5` for the separator `,`; `1.234,5` likewise for `.`, whose decimal mark
/// is `,`. None if the groups aren't whole threes after a first of one to three digits.
fn ungroup(s: &str, sep: char) -> Option<String> {
    let mark = if sep == '.' { ',' } else { '.' };
    let (sign, body) = match s.strip_prefix(['+', '-']) {
        Some(rest) => (&s[..1], rest),
        None => ("", s),
    };
    let end = body.find([mark, 'e', 'E']).unwrap_or(body.len());
    let (int, rest) = body.split_at(end);
    let mut out = String::with_capacity(s.len());
    out.push_str(sign);
    for (i, group) in int.split(sep).enumerate() {
        let digits = group.bytes().all(|c| c.is_ascii_digit());
        let fits = if i == 0 {
            (1..=3).contains(&group.len()) || !int.contains(sep)
        } else {
            group.len() == 3
        };
        if !digits || !fits {
            return None;
        }
        out.push_str(group);
    }
    if rest.contains(sep) {
        return None;
    }
    match rest.strip_prefix(mark) {
        Some(frac) => {
            out.push('.');
            out.push_str(frac);
        }
        None => out.push_str(rest),
    }
    Some(out)
}

/// A cell as the readers hand it to inference: missing, a number already (JSON's), or text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cell<'a> {
    /// No value.
    Missing,
    /// A number that needs no parsing.
    Num(f64),
    /// Text, which may still be a number, a time or a missing token.
    Str(Cow<'a, str>),
}

impl Cell<'_> {
    /// The cell's text if it has a value; a number written as Rust writes the shortest `f64`.
    fn text(&self) -> Option<Cow<'_, str>> {
        match self {
            Cell::Missing => None,
            Cell::Num(v) => Some(Cow::Owned(format!("{v}"))),
            Cell::Str(s) if is_missing(s) => None,
            Cell::Str(s) => Some(Cow::Borrowed(s)),
        }
    }

    fn missing(&self) -> bool {
        match self {
            Cell::Missing => true,
            Cell::Num(v) => v.is_nan(),
            Cell::Str(s) => is_missing(s),
        }
    }
}

/// A column from its name and its cells, its type inferred.
pub fn infer(name: &str, cells: &[Option<&str>], opt: &ReadOptions) -> Column {
    let cells: Vec<Cell> = cells
        .iter()
        .map(|c| match c {
            Some(s) => Cell::Str(Cow::Borrowed(*s)),
            None => Cell::Missing,
        })
        .collect();
    infer_cells(name, &cells, opt)
}

/// [`infer`] over cells that may already be numbers.
pub(crate) fn infer_cells(name: &str, cells: &[Cell], opt: &ReadOptions) -> Column {
    if cells.iter().all(Cell::missing) {
        return Column::number(name, vec![f64::NAN; cells.len()]);
    }
    if let Some(fmt) = opt.time_format.as_deref() {
        let read = |s: &str| with_format(s, fmt, &opt.zone);
        if let Some(v) = all_times(cells, read) {
            return Column::time(name, v);
        }
    }
    if let Some(v) = all_numbers(cells, opt.thousands) {
        return Column::number(name, v);
    }
    if let Some(v) = all_times(cells, |s| parse_time(s, opt)) {
        return Column::time(name, v);
    }
    strings(name, cells, opt.category_limit)
}

/// Every cell's number, NaN where missing, if every one that isn't missing is a number.
fn all_numbers(cells: &[Cell], thousands: Option<char>) -> Option<Vec<f64>> {
    cells
        .iter()
        .map(|c| match c {
            Cell::Missing => Some(f64::NAN),
            Cell::Num(v) => Some(*v),
            Cell::Str(s) if is_missing(s) => Some(f64::NAN),
            Cell::Str(s) => parse_number(s, thousands),
        })
        .collect()
}

/// Every cell's time, NaN where missing, if every one that isn't missing is text that `read`
/// takes. A number is never a time.
fn all_times(cells: &[Cell], read: impl Fn(&str) -> Option<f64>) -> Option<Vec<f64>> {
    cells
        .iter()
        .map(|c| match c {
            Cell::Missing => Some(f64::NAN),
            Cell::Num(v) if v.is_nan() => Some(f64::NAN),
            Cell::Num(_) => None,
            Cell::Str(s) if is_missing(s) => Some(f64::NAN),
            Cell::Str(s) => read(s),
        })
        .collect()
}

/// A Category column if there are at most `limit` distinct strings, else Text.
fn strings(name: &str, cells: &[Cell], limit: usize) -> Column {
    let texts: Vec<Option<Cow<str>>> = cells.iter().map(Cell::text).collect();
    let mut dict: Vec<Box<str>> = Vec::new();
    let mut index: BTreeMap<&str, u32> = BTreeMap::new();
    let mut codes: Vec<u32> = Vec::with_capacity(texts.len());
    for t in &texts {
        let Some(s) = t.as_deref() else {
            codes.push(u32::MAX);
            continue;
        };
        if let Some(&code) = index.get(s) {
            codes.push(code);
            continue;
        }
        if dict.len() == limit {
            let all: Vec<Box<str>> = texts
                .iter()
                .map(|t| t.as_deref().unwrap_or("").into())
                .collect();
            return Column {
                name: name.into(),
                values: Values::Text(all.into()),
            };
        }
        let code = dict.len() as u32;
        dict.push(s.into());
        index.insert(s, code);
        codes.push(code);
    }
    Column {
        name: name.into(),
        values: Values::Category {
            codes: codes.into(),
            dict: dict.into(),
        },
    }
}
