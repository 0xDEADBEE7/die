//! Transforms: a table reshaped before it's drawn, in the order a spec lists them.
//!
//! What each does at the edges:
//!
//! - Groups (`by`) are the distinct combinations of their columns' values, in the order first
//!   met; a missing value is a group of its own. Every transform that keeps or reorders rows
//!   keeps their `fig` blocks and source objects, and re-codes category columns so each
//!   dictionary holds only the values left, in the order first met (colour follows source
//!   order).
//! - Missing numbers are skipped by every sum, mean, extreme and percentile; a group with no
//!   values gives NaN (a gap), and a count of none is 0. A missing input row stays missing in
//!   `cumulative` and `window`, and the running total carries on past it.
//! - `fold` puts each row's fields one after another (row 1's, then row 2's), the kept columns
//!   first and `key` and `value` last; its fields must all be numbers, all times, or all
//!   strings.
//! - `filter`'s `in` and `not-in` test strings, or numbers by value; `not-in` keeps missing
//!   values. A range is inclusive and drops missing values.
//! - `bin` takes a nice step (1, 2 or 5 times a power of ten) giving at most `max_bins` bins,
//!   aligned on multiples of the step; the largest value, when it falls on a bin's edge, goes
//!   in the bin below (the last bin is closed).
//! - `aggregate` gives one row a group with the `by` columns and `out`; `count` without a field
//!   counts rows, with one counts its values. Percentiles interpolate between the closest ranks.
//! - `sort` is stable, missing values last either way; strings by code point.
//! - `top` keeps the rows of the `n` groups with the largest totals (ties to the first met), and
//!   sums the rest into rows named [`OTHER`], one for each combination of the other columns'
//!   values, after every kept row so "Other" is the last series. A missing group or one already
//!   named "Other" is never kept. With `n` groups or fewer the table is unchanged; the rows'
//!   `fig` blocks and objects are dropped when rows merge, as `aggregate` drops them.
//!
//! Errors name the transform and the column ("fold: no column \"apii\"; the columns are …").

use super::infer::parse_number;
use super::{Column, Table, Values};
use crate::error::{FigError, Result};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The group `top` sums the rest into.
pub const OTHER: &str = "Other";

/// The most distinct strings a folded value column holds as a category (as the readers').
const CATEGORY_LIMIT: usize = 50;

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

/// An op's name in a spec, for messages.
fn op_name(op: AggOp) -> &'static str {
    match op {
        AggOp::Count => "count",
        AggOp::Sum => "sum",
        AggOp::Mean => "mean",
        AggOp::Min => "min",
        AggOp::Max => "max",
        AggOp::P50 => "p50",
        AggOp::P95 => "p95",
    }
}

/// The table after each transform in turn.
pub fn apply(table: &Table, ts: &[Transform]) -> Result<Table> {
    let mut t = table.clone();
    for tr in ts {
        t = one(&t, tr)?;
    }
    Ok(t)
}

fn one(t: &Table, tr: &Transform) -> Result<Table> {
    match tr {
        Transform::Fold { fields, key, value } => fold(t, fields, key, value),
        Transform::Filter { field, op } => filter(t, field, op),
        Transform::Bin {
            field,
            step,
            max_bins,
            start,
            end,
        } => bin(t, field, *step, *max_bins, start, end),
        Transform::Aggregate { op, field, by, out } => aggregate(t, *op, field.as_deref(), by, out),
        Transform::Cumulative { field, by, out } => cumulative(t, field, by, out),
        Transform::Window {
            field,
            size,
            by,
            out,
        } => window(t, field, *size, by, out),
        Transform::Sort { field, descending } => sort(t, field, *descending),
        Transform::Top { n, by, field } => top(t, *n, by, field),
    }
}

// ------------------------------------------------------------------ the transforms

fn fold(t: &Table, fields: &[String], key: &str, value: &str) -> Result<Table> {
    const W: &str = "fold";
    if fields.is_empty() {
        return Err(FigError::new("fold: no fields to fold"));
    }
    let mut folded: Vec<&Column> = Vec::with_capacity(fields.len());
    for f in fields {
        let c = column(t, f, W)?;
        if folded.iter().any(|o| o.name == c.name) {
            return Err(FigError::new(format!("fold: \"{f}\" is listed twice")));
        }
        folded.push(c);
    }
    if key == value {
        return Err(FigError::new(format!(
            "fold: the key and the value are both named \"{key}\""
        )));
    }
    let kept: Vec<&Column> = t
        .columns
        .iter()
        .filter(|c| !fields.iter().any(|f| **f == *c.name))
        .collect();
    for name in [key, value] {
        if kept.iter().any(|c| *c.name == *name) {
            return Err(FigError::new(format!(
                "fold: a column is already named \"{name}\""
            )));
        }
    }
    let (n, k) = (t.rows, folded.len());
    let idx: Vec<usize> = (0..n).flat_map(|r| std::iter::repeat_n(r, k)).collect();
    let mut columns: Vec<Column> = kept.iter().map(|c| take_col(c, &idx)).collect();
    let codes: Vec<u32> = (0..n).flat_map(|_| 0..k as u32).collect();
    let dict: Vec<Box<str>> = match n {
        0 => Vec::new(),
        _ => fields.iter().map(|f| f.as_str().into()).collect(),
    };
    columns.push(Column {
        name: key.into(),
        values: Values::Category {
            codes: codes.into(),
            dict: dict.into(),
        },
    });
    let kinds: Vec<Family> = folded.iter().map(|c| family(c)).collect();
    let values = if kinds.iter().all(|f| *f == kinds[0]) {
        let at = |r: usize, j: usize| folded[j].f64_at(r);
        let nums = || (0..n).flat_map(move |r| (0..k).map(move |j| at(r, j)));
        match kinds[0] {
            Family::Number => Column::number(value, nums().collect()),
            Family::Time => Column::time(value, nums().collect()),
            Family::Strings => {
                let strs: Vec<Option<&str>> = (0..n)
                    .flat_map(|r| {
                        folded
                            .iter()
                            .map(move |c| c.str_at(r).filter(|s| !s.is_empty()))
                    })
                    .collect();
                strings(value, &strs)
            }
        }
    } else {
        let a = folded[0];
        let b = folded[kinds.iter().position(|f| *f != kinds[0]).unwrap_or(0)];
        return Err(FigError::new(format!(
            "fold: \"{}\" holds {} and \"{}\" {}; fold fields of one kind",
            a.name,
            holds(a),
            b.name,
            holds(b)
        )));
    };
    columns.push(values);
    Ok(Table {
        columns,
        rows: n * k,
        styles: t.styles.as_ref().map(|s| pick(s, &idx)),
        raw: t.raw.as_ref().map(|s| pick(s, &idx)),
    })
}

fn filter(t: &Table, field: &str, op: &FilterOp) -> Result<Table> {
    const W: &str = "filter";
    let c = column(t, field, W)?;
    let rows = 0..t.rows;
    let keep: Vec<usize> = match (op, &c.values) {
        (FilterOp::In(list) | FilterOp::NotIn(list), Values::Number(v)) => {
            let want = matches!(op, FilterOp::In(_));
            let mut nums = Vec::with_capacity(list.len());
            for s in list {
                let Some(x) = parse_number(s, None) else {
                    return Err(FigError::new(format!(
                        "filter: \"{s}\" is not a number, and \"{field}\" holds numbers"
                    )));
                };
                nums.push(x);
            }
            rows.filter(|&r| nums.contains(&v[r]) == want).collect()
        }
        (FilterOp::In(_) | FilterOp::NotIn(_), Values::Time(_)) => {
            return Err(FigError::new(format!(
                "filter: \"{field}\" holds times; filter them with a range"
            )));
        }
        (FilterOp::In(list) | FilterOp::NotIn(list), _) => {
            let want = matches!(op, FilterOp::In(_));
            rows.filter(|&r| {
                let hit = c.str_at(r).is_some_and(|s| list.iter().any(|l| l == s));
                hit == want
            })
            .collect()
        }
        (FilterOp::Range(lo, hi), Values::Number(v) | Values::Time(v)) => rows
            .filter(|&r| {
                let x = v[r];
                !x.is_nan() && lo.is_none_or(|l| x >= l) && hi.is_none_or(|h| x <= h)
            })
            .collect(),
        (FilterOp::Range(..), _) => {
            return Err(FigError::new(format!(
                "filter: a range needs numbers or times, and \"{field}\" holds {}",
                holds(c)
            )));
        }
    };
    Ok(take(t, &keep))
}

fn bin(
    t: &Table,
    field: &str,
    step: Option<f64>,
    max_bins: usize,
    start: &str,
    end: &str,
) -> Result<Table> {
    const W: &str = "bin";
    let v = numbers(column(t, field, W)?, W)?;
    fresh(t, start, W)?;
    fresh(t, end, W)?;
    if start == end {
        return Err(FigError::new(format!(
            "bin: the start and the end are both named \"{start}\""
        )));
    }
    let span = extent(v);
    let step = match step {
        Some(s) if s.is_finite() && s > 0.0 => s,
        Some(s) => {
            return Err(FigError::new(format!(
                "bin: a step is a positive number, not {s}"
            )));
        }
        None => span.map_or(1.0, |(lo, hi)| nice_step(lo, hi, max_bins.max(1))),
    };
    let grid = Grid::new(step);
    let (mut starts, mut ends) = (Vec::with_capacity(v.len()), Vec::with_capacity(v.len()));
    for &x in v.iter() {
        let Some((lo, hi)) = span.filter(|_| x.is_finite()) else {
            starts.push(f64::NAN);
            ends.push(f64::NAN);
            continue;
        };
        let k = grid.bin(x, lo, hi);
        starts.push(grid.at(k));
        ends.push(grid.at(k + 1.0));
    }
    let mut columns = t.columns.clone();
    columns.push(Column::number(start, starts));
    columns.push(Column::number(end, ends));
    Ok(Table {
        columns,
        ..t.clone()
    })
}

fn aggregate(t: &Table, op: AggOp, field: Option<&str>, by: &[String], out: &str) -> Result<Table> {
    const W: &str = "aggregate";
    let by_cols = by_columns(t, by, W)?;
    if by.iter().any(|b| b == out) {
        return Err(FigError::new(format!(
            "aggregate: a column is already named \"{out}\""
        )));
    }
    let f = match field {
        Some(name) => Some(column(t, name, W)?),
        None if op == AggOp::Count => None,
        None => {
            return Err(FigError::new(format!(
                "aggregate: {} needs a field",
                op_name(op)
            )));
        }
    };
    let time = f.is_some_and(|c| matches!(c.values, Values::Time(_)));
    if op != AggOp::Count
        && let Some(c) = f
    {
        match c.values {
            Values::Number(_) => {}
            Values::Time(_) if op != AggOp::Sum => {}
            _ => {
                return Err(FigError::new(format!(
                    "aggregate: {} needs numbers{}, and \"{}\" holds {}",
                    op_name(op),
                    if op == AggOp::Sum { "" } else { " or times" },
                    c.name,
                    holds(c)
                )));
            }
        }
    }
    let groups = groups(&by_cols, 0..t.rows);
    let firsts: Vec<usize> = groups.iter().filter_map(|g| g.first().copied()).collect();
    let mut columns: Vec<Column> = by_cols.iter().map(|c| take_col(c, &firsts)).collect();
    let vals: Vec<f64> = groups.iter().map(|g| reduce(op, f, g)).collect();
    columns.push(match (time, op) {
        (true, op) if op != AggOp::Count => Column::time(out, vals),
        _ => Column::number(out, vals),
    });
    Table::new(columns)
}

fn cumulative(t: &Table, field: &str, by: &[String], out: &str) -> Result<Table> {
    const W: &str = "cumulative";
    let v = numbers(column(t, field, W)?, W)?;
    let by_cols = by_columns(t, by, W)?;
    fresh(t, out, W)?;
    let mut res = vec![f64::NAN; t.rows];
    for g in groups(&by_cols, 0..t.rows) {
        let mut total = 0.0;
        for r in g {
            if !v[r].is_nan() {
                total += v[r];
                res[r] = total;
            }
        }
    }
    Ok(with(t, Column::number(out, res)))
}

fn window(t: &Table, field: &str, size: usize, by: &[String], out: &str) -> Result<Table> {
    const W: &str = "window";
    let v = numbers(column(t, field, W)?, W)?;
    let by_cols = by_columns(t, by, W)?;
    fresh(t, out, W)?;
    if size == 0 {
        return Err(FigError::new("window: a window is at least 1 row"));
    }
    let mut res = vec![f64::NAN; t.rows];
    for g in groups(&by_cols, 0..t.rows) {
        for (i, &r) in g.iter().enumerate() {
            if v[r].is_nan() {
                continue;
            }
            let from = (i + 1).saturating_sub(size);
            let (sum, count) = g[from..=i]
                .iter()
                .map(|&q| v[q])
                .filter(|x| !x.is_nan())
                .fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
            res[r] = sum / count as f64;
        }
    }
    Ok(with(t, Column::number(out, res)))
}

fn sort(t: &Table, field: &str, descending: bool) -> Result<Table> {
    let c = column(t, field, "sort")?;
    let mut idx: Vec<usize> = (0..t.rows).collect();
    let order = |o: Ordering| if descending { o.reverse() } else { o };
    match &c.values {
        Values::Number(v) | Values::Time(v) => {
            idx.sort_by(|&a, &b| match (v[a].is_nan(), v[b].is_nan()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ => order(v[a].partial_cmp(&v[b]).unwrap_or(Ordering::Equal)),
            })
        }
        _ => {
            let s = |r: usize| c.str_at(r).filter(|s| !s.is_empty());
            idx.sort_by(|&a, &b| match (s(a), s(b)) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(x), Some(y)) => order(x.cmp(y)),
            });
        }
    }
    Ok(take(t, &idx))
}

fn top(t: &Table, n: usize, by: &str, field: &str) -> Result<Table> {
    const W: &str = "top";
    let b = column(t, by, W)?;
    if matches!(b.values, Values::Number(_) | Values::Time(_)) {
        return Err(FigError::new(format!(
            "top: \"{by}\" holds {}; top ranks the groups of a category or text column",
            holds(b)
        )));
    }
    let f = column(t, field, W)?;
    let v = numbers(f, W)?;
    let groups = groups(&[b], 0..t.rows);
    let name = |g: &[usize]| b.str_at(g[0]).filter(|s| !s.is_empty() && *s != OTHER);
    let mut ranked: Vec<(usize, f64)> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| name(g).is_some())
        .map(|(i, g)| (i, g.iter().map(|&r| v[r]).filter(|x| !x.is_nan()).sum()))
        .collect();
    if ranked.len() <= n {
        return Ok(t.clone());
    }
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    let mut kept_group = vec![false; groups.len()];
    for &(g, _) in &ranked[..n] {
        kept_group[g] = true;
    }
    let mut group_of = vec![0usize; t.rows];
    for (i, g) in groups.iter().enumerate() {
        for &r in g {
            group_of[r] = i;
        }
    }
    let kept: Vec<usize> = (0..t.rows).filter(|&r| kept_group[group_of[r]]).collect();
    let keys: Vec<&Column> = t
        .columns
        .iter()
        .filter(|c| c.name != b.name && c.name != f.name)
        .collect();
    let rest = groups_of(&keys, (0..t.rows).filter(|&r| !kept_group[group_of[r]]));
    let rest: Vec<Vec<usize>> = rest.into_iter().filter(|g| !g.is_empty()).collect();
    let src: Vec<usize> = kept
        .iter()
        .copied()
        .chain(rest.iter().map(|g| g[0]))
        .collect();
    let columns = t
        .columns
        .iter()
        .map(|c| {
            if c.name == b.name {
                let names: Vec<Option<&str>> = kept
                    .iter()
                    .map(|&r| b.str_at(r))
                    .chain(rest.iter().map(|_| Some(OTHER)))
                    .collect();
                match b.values {
                    Values::Text(_) => text(by, &names),
                    _ => category(by, &names),
                }
            } else if c.name == f.name {
                let sums = rest.iter().map(|g| sum(g.iter().map(|&r| v[r])));
                Column::number(field, kept.iter().map(|&r| v[r]).chain(sums).collect())
            } else {
                take_col(c, &src)
            }
        })
        .collect();
    Table::new(columns)
}

// ------------------------------------------------------------------ helpers

/// The column named `name`, or an error naming the transform and the columns there are.
fn column<'t>(t: &'t Table, name: &str, what: &str) -> Result<&'t Column> {
    t.column(name).ok_or_else(|| {
        let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
        let there = match names.len() {
            0 => "the table has none".to_string(),
            _ => format!("the columns are {}", names.join(", ")),
        };
        FigError::new(format!("{what}: no column \"{name}\"; {there}"))
    })
}

fn by_columns<'t>(t: &'t Table, by: &[String], what: &str) -> Result<Vec<&'t Column>> {
    by.iter().map(|b| column(t, b, what)).collect()
}

/// An error if a column is already named `name`.
fn fresh(t: &Table, name: &str, what: &str) -> Result<()> {
    match t.column(name) {
        Some(_) => Err(FigError::new(format!(
            "{what}: a column is already named \"{name}\""
        ))),
        None => Ok(()),
    }
}

/// What a column holds, for a message.
fn holds(c: &Column) -> &'static str {
    match c.values {
        Values::Number(_) => "numbers",
        Values::Time(_) => "times",
        Values::Category { .. } => "categories",
        Values::Text(_) => "text",
    }
}

/// A number column's values, or an error naming the transform.
fn numbers<'c>(c: &'c Column, what: &str) -> Result<&'c [f64]> {
    match &c.values {
        Values::Number(v) => Ok(v),
        _ => Err(FigError::new(format!(
            "{what}: \"{}\" holds {}, not numbers",
            c.name,
            holds(c)
        ))),
    }
}

/// The kinds fold can put in one value column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Number,
    Time,
    Strings,
}

fn family(c: &Column) -> Family {
    match c.values {
        Values::Number(_) => Family::Number,
        Values::Time(_) => Family::Time,
        _ => Family::Strings,
    }
}

/// The table with one more column, its rows as they were.
fn with(t: &Table, c: Column) -> Table {
    let mut columns = t.columns.clone();
    columns.push(c);
    Table {
        columns,
        ..t.clone()
    }
}

fn pick<T: Clone>(v: &Arc<[T]>, idx: &[usize]) -> Arc<[T]> {
    idx.iter().map(|&i| v[i].clone()).collect()
}

/// The rows at `idx`, in that order, with their `fig` blocks and objects.
fn take(t: &Table, idx: &[usize]) -> Table {
    Table {
        columns: t.columns.iter().map(|c| take_col(c, idx)).collect(),
        rows: idx.len(),
        styles: t.styles.as_ref().map(|s| pick(s, idx)),
        raw: t.raw.as_ref().map(|s| pick(s, idx)),
    }
}

/// A column's rows at `idx`; a category re-coded to the values left, in the order first met.
fn take_col(c: &Column, idx: &[usize]) -> Column {
    let values = match &c.values {
        Values::Number(v) => Values::Number(idx.iter().map(|&i| v[i]).collect()),
        Values::Time(v) => Values::Time(idx.iter().map(|&i| v[i]).collect()),
        Values::Text(v) => Values::Text(idx.iter().map(|&i| v[i].clone()).collect()),
        Values::Category { codes, dict } => {
            let mut remap = vec![u32::MAX; dict.len()];
            let mut fresh: Vec<Box<str>> = Vec::new();
            let codes: Arc<[u32]> = idx
                .iter()
                .map(|&i| {
                    let code = codes[i];
                    let Some(slot) = remap.get_mut(code as usize) else {
                        return u32::MAX;
                    };
                    if *slot == u32::MAX {
                        *slot = fresh.len() as u32;
                        fresh.push(dict[code as usize].clone());
                    }
                    *slot
                })
                .collect();
            Values::Category {
                codes,
                dict: fresh.into(),
            }
        }
    };
    Column {
        name: c.name.clone(),
        values,
    }
}

/// A category column of these strings, codes in the order first met, None missing.
fn category(name: &str, vals: &[Option<&str>]) -> Column {
    let mut dict: Vec<Box<str>> = Vec::new();
    let mut index: BTreeMap<&str, u32> = BTreeMap::new();
    let codes: Arc<[u32]> = vals
        .iter()
        .map(|v| match v {
            None => u32::MAX,
            Some(s) => *index.entry(s).or_insert_with(|| {
                dict.push((*s).into());
                (dict.len() - 1) as u32
            }),
        })
        .collect();
    Column {
        name: name.into(),
        values: Values::Category {
            codes,
            dict: dict.into(),
        },
    }
}

/// A text column of these strings, None as the empty string.
fn text(name: &str, vals: &[Option<&str>]) -> Column {
    let v: Arc<[Box<str>]> = vals.iter().map(|s| s.unwrap_or("").into()).collect();
    Column {
        name: name.into(),
        values: Values::Text(v),
    }
}

/// A category column if there are few distinct strings, else text.
fn strings(name: &str, vals: &[Option<&str>]) -> Column {
    let c = category(name, vals);
    match &c.values {
        Values::Category { dict, .. } if dict.len() > CATEGORY_LIMIT => text(name, vals),
        _ => c,
    }
}

/// One part of a group's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part<'a> {
    Num(u64),
    Code(u32),
    Str(&'a str),
}

fn part(c: &Column, row: usize) -> Part<'_> {
    match &c.values {
        Values::Number(v) | Values::Time(v) => {
            let x = v[row];
            Part::Num(match x {
                x if x.is_nan() => u64::MAX,
                0.0 => 0,
                x => x.to_bits(),
            })
        }
        Values::Category { codes, .. } => Part::Code(codes[row]),
        Values::Text(v) => Part::Str(&v[row]),
    }
}

/// The rows of each group of `by`'s values, groups in the order first met; with no `by`, all
/// the rows as one group (an empty one if there are none).
fn groups(by: &[&Column], rows: impl Iterator<Item = usize>) -> Vec<Vec<usize>> {
    let g = groups_of(by, rows);
    if g.is_empty() && by.is_empty() {
        return vec![Vec::new()];
    }
    g
}

fn groups_of(by: &[&Column], rows: impl Iterator<Item = usize>) -> Vec<Vec<usize>> {
    if by.is_empty() {
        let all: Vec<usize> = rows.collect();
        return if all.is_empty() {
            Vec::new()
        } else {
            vec![all]
        };
    }
    let mut index: BTreeMap<Vec<Part>, usize> = BTreeMap::new();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for r in rows {
        let key: Vec<Part> = by.iter().map(|c| part(c, r)).collect();
        let g = *index.entry(key).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[g].push(r);
    }
    out
}

/// Whether a row has a value in a column.
fn present(c: &Column, r: usize) -> bool {
    match &c.values {
        Values::Number(v) | Values::Time(v) => !v[r].is_nan(),
        Values::Category { codes, .. } => codes[r] != u32::MAX,
        Values::Text(v) => !v[r].is_empty(),
    }
}

/// The sum of the values that aren't missing; NaN if none.
fn sum(vals: impl Iterator<Item = f64>) -> f64 {
    let mut out = None;
    for x in vals.filter(|x| !x.is_nan()) {
        *out.get_or_insert(0.0) += x;
    }
    out.unwrap_or(f64::NAN)
}

/// One group's value.
fn reduce(op: AggOp, f: Option<&Column>, rows: &[usize]) -> f64 {
    let Some(c) = f else {
        return rows.len() as f64;
    };
    if op == AggOp::Count {
        return rows.iter().filter(|&&r| present(c, r)).count() as f64;
    }
    let mut v: Vec<f64> = rows
        .iter()
        .map(|&r| c.f64_at(r))
        .filter(|x| !x.is_nan())
        .collect();
    if v.is_empty() {
        return f64::NAN;
    }
    match op {
        AggOp::Count => v.len() as f64,
        AggOp::Sum => v.iter().sum(),
        AggOp::Mean => v.iter().sum::<f64>() / v.len() as f64,
        AggOp::Min => v.iter().copied().fold(f64::INFINITY, f64::min),
        AggOp::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        AggOp::P50 => quantile(&mut v, 0.5),
        AggOp::P95 => quantile(&mut v, 0.95),
    }
}

/// The `p` quantile, interpolated between the closest ranks (numpy's and d3's default).
pub fn quantile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let h = (v.len() - 1) as f64 * p.clamp(0.0, 1.0);
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(v.len() - 1);
    v[lo] + (h - lo as f64) * (v[hi] - v[lo])
}

/// The least and the most finite value.
fn extent(v: &[f64]) -> Option<(f64, f64)> {
    v.iter()
        .copied()
        .filter(|x| x.is_finite())
        .fold(None, |acc, x| match acc {
            None => Some((x, x)),
            Some((lo, hi)) => Some((lo.min(x), hi.max(x))),
        })
}

/// A power of ten times `f`, exactly where the result is a short decimal.
fn decimal(f: f64, e: i32) -> f64 {
    if e < 0 {
        f / 10f64.powi(-e)
    } else {
        f * 10f64.powi(e)
    }
}

/// The smallest step of 1, 2 or 5 times a power of ten that bins `lo..=hi` in at most `bins`.
fn nice_step(lo: f64, hi: f64, bins: usize) -> f64 {
    let span = if hi > lo {
        hi - lo
    } else if hi != 0.0 {
        hi.abs()
    } else {
        1.0
    };
    let raw = span / bins as f64;
    let mut e = raw.log10().floor() as i32;
    loop {
        for f in [1.0, 2.0, 5.0] {
            let step = decimal(f, e);
            if step < raw * (1.0 - 1e-9) {
                continue;
            }
            let grid = Grid::new(step);
            if grid.bin(hi, lo, hi) - grid.bin(lo, lo, hi) < bins as f64 {
                return step;
            }
        }
        e += 1;
    }
}

/// Bins on multiples of a step, their edges computed as decimals where the step is one, so a
/// step of 0.1 has an edge at 0.3 and not 0.30000000000000004.
struct Grid {
    step: f64,
    /// The step is `m / 10^d`.
    m: f64,
    d: i32,
}

impl Grid {
    fn new(step: f64) -> Grid {
        for d in 0..=15 {
            let s = step * 10f64.powi(d);
            let r = s.round();
            if r != 0.0 && (s - r).abs() <= 1e-9 * s.abs() {
                return Grid { step, m: r, d };
            }
        }
        Grid {
            step,
            m: step,
            d: 0,
        }
    }

    /// The edge `k` steps from zero.
    fn at(&self, k: f64) -> f64 {
        match self.d {
            0 => k * self.m,
            d => k * self.m / 10f64.powi(d),
        }
    }

    /// The bin `x` falls in, as steps from zero, over data from `lo` to `hi`: the largest value
    /// on an edge goes in the bin below.
    fn bin(&self, x: f64, lo: f64, hi: f64) -> f64 {
        let edge = |x: f64| {
            let q = x / self.step;
            (q + q.abs().max(1.0) * 1e-10).floor()
        };
        let k = edge(x);
        if x == hi && k > edge(lo) && (self.at(k) - x).abs() <= self.step * 1e-9 {
            return k - 1.0;
        }
        k
    }
}
