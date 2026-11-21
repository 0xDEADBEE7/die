//! JSON and JSONL rows, read through [`crate::json`] so columns keep the file's order: an array
//! of objects, an object of arrays, or an array of arrays with a header row; and JSONL, an object
//! a line. A row's `fig` block goes to [`Table::styles`] and its object to [`Table::raw`].
//!
//! - Array of objects: the columns are every key in the order first met, the first row's
//!   first; a row without a key is missing that cell. `fig` is never a column.
//! - Object of arrays: a column a key, in the object's order, each the same length; a `fig` key
//!   holding an array gives the rows' blocks.
//! - Array of arrays: the first is the header (strings, or numbers written as names) unless
//!   [`ReadOptions::header`] is false, which names the columns `column N`; a short row is
//!   missing its last cells, a long one an error at its first extra value.
//! - JSONL: a value a line, blank lines skipped; objects, or arrays with a header line first.
//!
//! A JSON number stays a number, `null` is missing, a string is inferred as CSV's cells are
//! (so `"12"` is a number and `"NA"` missing), `true` and `false` are strings, and an array or
//! object in a cell is its compact JSON text. Every error has the place of the value at fault.

use super::infer::{Cell, infer_cells};
use super::{ReadOptions, Table};
use crate::error::{FigError, Result};
use crate::json::{self, Kind, Node};
use crate::style::FigStyle;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A table from a JSON value: an array of objects, an object of arrays, or an array of arrays
/// whose first is the header.
pub fn read_json(node: &Node, opt: &ReadOptions) -> Result<Table> {
    match &node.kind {
        Kind::Arr(items) => rows(items, opt),
        Kind::Obj(fields) => columns(fields, opt),
        _ => Err(FigError::malformed(
            format!(
                "expected an array of rows or an object of columns, not {}",
                a(node)
            ),
            node.at,
        )),
    }
}

/// A table from JSONL text: an object a line, blank lines skipped.
pub fn read_jsonl(text: &str, opt: &ReadOptions) -> Result<Table> {
    let nodes: Vec<Node> = json::parse_lines(text).into_iter().collect::<Result<_>>()?;
    rows(&nodes, opt)
}

/// A value's kind with its article, for a message: "a number", "an object".
fn a(n: &Node) -> &'static str {
    match n.type_name() {
        "null" => "null",
        "boolean" => "a boolean",
        "number" => "a number",
        "string" => "a string",
        "array" => "an array",
        _ => "an object",
    }
}

/// A cell from a JSON value.
fn cell(n: &Node) -> Cell<'_> {
    match &n.kind {
        Kind::Null => Cell::Missing,
        Kind::Num(v) => Cell::Num(*v),
        Kind::Str(s) => Cell::Str(Cow::Borrowed(s)),
        Kind::Bool(b) => Cell::Str(Cow::Borrowed(if *b { "true" } else { "false" })),
        Kind::Arr(_) | Kind::Obj(_) => Cell::Str(Cow::Owned(n.to_text())),
    }
}

/// Rows: objects or arrays, as the first one is.
fn rows(items: &[Node], opt: &ReadOptions) -> Result<Table> {
    let Some(first) = items.first() else {
        return Ok(Table::default());
    };
    match first.kind {
        Kind::Obj(_) => objects(items, opt),
        Kind::Arr(_) => arrays(items, opt),
        _ => Err(FigError::malformed(
            format!("expected a row, an object or an array, not {}", a(first)),
            first.at,
        )),
    }
}

/// The error for a row that isn't what the first row is.
fn unlike(row: &Node, want: &str) -> FigError {
    FigError::malformed(
        format!("expected an {want} like the first row, not {}", a(row)),
        row.at,
    )
}

/// An array of objects.
fn objects(items: &[Node], opt: &ReadOptions) -> Result<Table> {
    let mut names: Vec<&str> = Vec::new();
    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    let mut cells: Vec<Vec<Cell>> = Vec::new();
    let mut styles: Vec<FigStyle> = Vec::with_capacity(items.len());
    let mut any_style = false;
    for (r, row) in items.iter().enumerate() {
        let Some(obj) = row.as_obj() else {
            return Err(unlike(row, "object"));
        };
        let mut style = FigStyle::default();
        for (key, value) in obj {
            if &*key.name == "fig" {
                style = FigStyle::from_node(value);
                any_style = true;
                continue;
            }
            let c = *index.entry(&key.name).or_insert_with(|| {
                names.push(&key.name);
                cells.push(vec![Cell::Missing; r]);
                names.len() - 1
            });
            cells[c].push(cell(value));
        }
        styles.push(style);
        for col in &mut cells {
            if col.len() == r {
                col.push(Cell::Missing);
            }
        }
    }
    let mut t = build(&names, &cells, opt)?;
    t.rows = items.len();
    t.raw = Some(items.to_vec().into());
    t.styles = any_style.then(|| Arc::from(styles));
    Ok(t)
}

/// An array of arrays, the first the header unless `opt.header` is false.
fn arrays(items: &[Node], opt: &ReadOptions) -> Result<Table> {
    let mut body = items;
    let mut names: Vec<Cow<str>> = Vec::new();
    if opt.header {
        let head = items[0].as_arr().unwrap_or_default();
        for (i, h) in head.iter().enumerate() {
            let name = match &h.kind {
                Kind::Str(s) if s.is_empty() => Cow::Owned(format!("column {}", i + 1)),
                Kind::Str(s) => Cow::Borrowed(&**s),
                Kind::Num(v) => Cow::Owned(format!("{v}")),
                Kind::Null => Cow::Owned(format!("column {}", i + 1)),
                _ => {
                    return Err(FigError::malformed(
                        format!("expected a column's name, not {}", a(h)),
                        h.at,
                    ));
                }
            };
            if names.contains(&name) {
                return Err(FigError::malformed(
                    format!("two columns are named \"{name}\""),
                    h.at,
                ));
            }
            names.push(name);
        }
        body = &items[1..];
    } else {
        let widest = items
            .iter()
            .filter_map(|r| r.as_arr().map(<[Node]>::len))
            .max()
            .unwrap_or(0);
        names = (1..=widest)
            .map(|i| Cow::Owned(format!("column {i}")))
            .collect();
    }
    let mut cells: Vec<Vec<Cell>> = names.iter().map(|_| Vec::new()).collect();
    for row in body {
        let Some(vals) = row.as_arr() else {
            return Err(unlike(row, "array"));
        };
        if vals.len() > names.len() {
            return Err(FigError::malformed(
                format!(
                    "a row of {} values where the header has {}",
                    vals.len(),
                    names.len()
                ),
                vals[names.len()].at,
            ));
        }
        for (c, col) in cells.iter_mut().enumerate() {
            col.push(vals.get(c).map_or(Cell::Missing, cell));
        }
    }
    let names: Vec<&str> = names.iter().map(|n| &**n).collect();
    build(&names, &cells, opt)
}

/// An object of arrays, a column a key.
fn columns(fields: &[(json::Key, Node)], opt: &ReadOptions) -> Result<Table> {
    let mut names: Vec<&str> = Vec::new();
    let mut cells: Vec<Vec<Cell>> = Vec::new();
    let mut styles: Option<Vec<FigStyle>> = None;
    let mut first: Option<(&str, usize)> = None;
    for (key, value) in fields {
        let Some(vals) = value.as_arr() else {
            return Err(FigError::malformed(
                format!(
                    "expected an array of the values of \"{}\", not {}",
                    key.name,
                    a(value)
                ),
                value.at,
            ));
        };
        match first {
            None => first = Some((&key.name, vals.len())),
            Some((name, n)) if n != vals.len() => {
                return Err(FigError::malformed(
                    format!(
                        "\"{}\" has {} values where \"{name}\" has {n}",
                        key.name,
                        vals.len()
                    ),
                    value.at,
                ));
            }
            Some(_) => {}
        }
        if &*key.name == "fig" {
            styles = Some(vals.iter().map(FigStyle::from_node).collect());
            continue;
        }
        names.push(&key.name);
        cells.push(vals.iter().map(cell).collect());
    }
    let mut t = build(&names, &cells, opt)?;
    if names.is_empty()
        && let Some((_, n)) = first
    {
        t.rows = n;
    }
    t.styles = styles.map(Arc::from);
    Ok(t)
}

/// The columns, each inferred.
fn build(names: &[&str], cells: &[Vec<Cell>], opt: &ReadOptions) -> Result<Table> {
    let columns = names
        .iter()
        .zip(cells)
        .map(|(name, cells)| infer_cells(name, cells, opt))
        .collect();
    Table::new(columns)
}
