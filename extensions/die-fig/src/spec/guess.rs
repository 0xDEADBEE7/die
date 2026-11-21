//! A figure from a bare table, by the design's rules: a time or ordered number column is x and
//! the other numeric columns are series, as lines; a category x gives bars; two unordered
//! numbers a scatter; a single number column a histogram.
//!
//! In order, the first that applies:
//!
//! 1. a time column: lines over the first one, a line a number column;
//! 2. a first column of categories or text, and a number column: bars, a band a category,
//!    grouped side by side, a bar a number column; rows of one category (and colour) summed;
//! 3. two number columns or more, the first ordered (never decreasing, and not all one value):
//!    lines over it, a line each other number column;
//! 4. two number columns or more: points, the first across and the second up;
//! 5. one number column: a histogram of it.
//!
//! Several number columns become series by folding them into `(series, value)`. With one
//! number column, lines, bars and points are coloured by the first category column there is
//! (another than x). The guess's data is the table itself.

use super::chart::{binned, unique};
use super::{
    Channel, Chart, DataRef, DataSpec, Encoding, Layer, Mark, MarkKind, ReadHints, ScaleKind,
    Stack, ViewSettings,
};
use crate::data::transform::{AggOp, Transform};
use crate::data::{Column, ColumnKind, Table};
use crate::error::{FigError, Result};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The chart a table's columns suggest.
pub fn guess(table: &Table) -> Result<Chart> {
    if table.columns.is_empty() {
        return Err(FigError::new("nothing to draw: the data has no columns"));
    }
    if table.rows == 0 {
        return Err(FigError::new("nothing to draw: the data has no rows"));
    }
    let taken: Vec<&str> = table.columns.iter().map(|c| &*c.name).collect();
    let numbers: Vec<&Column> = of(table, ColumnKind::Number);
    let categories: Vec<&Column> = of(table, ColumnKind::Category);
    let first = &table.columns[0];
    let colour_other_than = |x: &str| categories.iter().find(|c| &*c.name != x).map(|c| &*c.name);

    let mut transform = Vec::new();
    let (mark, enc) = if let Some(t) = of(table, ColumnKind::Time).first() {
        if numbers.is_empty() {
            return Err(FigError::new(format!(
                "nothing to draw: no number column to draw over \"{}\"",
                t.name
            )));
        }
        let names: Vec<&str> = numbers.iter().map(|c| &*c.name).collect();
        let (y, color) = series(&names, colour_other_than(&t.name), &taken, &mut transform);
        (Mark::new(MarkKind::Line), enc(on(&t.name), y, color))
    } else if matches!(first.kind(), ColumnKind::Category | ColumnKind::Text) && !numbers.is_empty()
    {
        let names: Vec<&str> = numbers.iter().map(|c| &*c.name).collect();
        let colour = if names.len() == 1 {
            colour_other_than(&first.name)
        } else {
            None
        };
        let (y, color) = series(&names, colour, &taken, &mut transform);
        if repeats(table, first, colour) {
            let mut by = vec![first.name.to_string()];
            by.extend(color.as_ref().and_then(|c| c.field.clone()));
            let f = y.field.clone().unwrap_or_default();
            transform.push(Transform::Aggregate {
                op: AggOp::Sum,
                field: Some(f.clone()),
                by,
                out: f,
            });
        }
        let x = Channel {
            scale: Some(ScaleKind::Band),
            ..on(&first.name)
        };
        let mark = Mark {
            stack: Stack::None,
            ..Mark::new(MarkKind::Bar)
        };
        (mark, enc(x, y, color))
    } else if numbers.len() >= 2 && ordered(numbers[0]) {
        let x = &numbers[0].name;
        let names: Vec<&str> = numbers[1..].iter().map(|c| &*c.name).collect();
        let (y, color) = series(&names, colour_other_than(x), &taken, &mut transform);
        (Mark::new(MarkKind::Line), enc(on(x), y, color))
    } else if numbers.len() >= 2 {
        let color = categories.first().map(|c| on(&c.name));
        (
            Mark::new(MarkKind::Point),
            enc(on(&numbers[0].name), on(&numbers[1].name), color),
        )
    } else if let [n] = numbers.as_slice() {
        let (ts, x, y) = binned(
            on(&n.name),
            None,
            None,
            None,
            super::chart::MAX_BINS,
            &taken,
        );
        transform = ts;
        (Mark::new(MarkKind::Bar), enc(x, y, None))
    } else {
        return Err(FigError::new(
            "nothing to draw: the data has no number column",
        ));
    };
    Ok(Chart {
        title: None,
        subtitle: None,
        data: DataSpec {
            source: DataRef::Inline(Arc::new(table.clone())),
            format: None,
            read: ReadHints::default(),
        },
        transform,
        layers: vec![Layer {
            mark,
            enc,
            data: None,
            transform: Vec::new(),
            fig: Default::default(),
        }],
        series_fig: Vec::new(),
        view: ViewSettings::default(),
    })
}

fn of(table: &Table, kind: ColumnKind) -> Vec<&Column> {
    table.columns.iter().filter(|c| c.kind() == kind).collect()
}

/// A channel on a field, nothing more.
fn on(field: &str) -> Channel {
    Channel {
        field: Some(field.into()),
        ..Channel::default()
    }
}

fn enc(x: Channel, y: Channel, color: Option<Channel>) -> Encoding {
    Encoding {
        x: Some(x),
        y: Some(y),
        color,
        ..Encoding::default()
    }
}

/// The y channel and the colour for number columns `ys`: one column is y, coloured by
/// `colour` if there is one; several are folded into `(series, value)` and coloured by series.
fn series(
    ys: &[&str],
    colour: Option<&str>,
    taken: &[&str],
    transform: &mut Vec<Transform>,
) -> (Channel, Option<Channel>) {
    if let [y] = ys {
        return (on(y), colour.map(on));
    }
    let key = unique("series", taken);
    let value = unique("value", taken);
    transform.push(Transform::Fold {
        fields: ys.iter().map(|s| s.to_string()).collect(),
        key: key.clone(),
        value: value.clone(),
    });
    (on(&value), Some(on(&key)))
}

/// Whether a column never decreases and has two values at least, missing cells aside.
fn ordered(c: &Column) -> bool {
    let mut last = f64::NEG_INFINITY;
    let mut rises = false;
    for i in 0..c.len() {
        let v = c.f64_at(i);
        if v.is_nan() {
            continue;
        }
        if v < last {
            return false;
        }
        if last != f64::NEG_INFINITY && v > last {
            rises = true;
        }
        last = v;
    }
    rises
}

/// Whether a value of `x` (with `colour`'s, if given) comes in more than one row.
fn repeats(table: &Table, x: &Column, colour: Option<&str>) -> bool {
    let c = colour.and_then(|n| table.column(n));
    let mut seen = BTreeSet::new();
    (0..table.rows).any(|i| {
        let key = (x.str_at(i), c.and_then(|c| c.str_at(i)));
        !seen.insert(key)
    })
}
