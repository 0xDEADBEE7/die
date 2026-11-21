//! The graph spec beyond fig's: groups, sizes, weights, directed edges and a layout, with ids
//! checked unique and edges' ends checked known ([`super::fig`] reads fig's part and checks).
//!
//! - a node's `group` (a string or a number) colours it, and its `size` (above 0) scales it;
//! - an edge's `label`, `weight` (a number) and `directed` (true or false; the graph's
//!   `"directed"` gives every edge's default, false as fig's are);
//! - `layout`: `"force"` (fig's stress layout), `"tree"`, `"layered"`, `"auto"` (the default:
//!   the graph picks, see [`crate::graph::GraphModel::build`]), or `{"type": "tree", "root":
//!   id}`.
//!
//! [`from_table`] makes a tree of a table with an `id` and a `parent` column: an org chart from
//! a CSV.

use super::{
    GraphEdge, GraphNode, GraphSpec, Layout, ViewSettings, boolean, field, id_of, number, obj,
    opt_string, string,
};
use crate::data::{Column, Table, Values};
use crate::error::{FigError, Pos, Result};
use crate::json::{Key, Kind, Node};
use crate::style::FigStyle;
use std::collections::BTreeSet;

/// A node's group and size.
pub(crate) fn node_extras(n: &Node) -> Result<(Option<String>, Option<f64>)> {
    let group = field(n, "group").map(|g| id_of(g, "group")).transpose()?;
    let size = match field(n, "size") {
        None => None,
        Some(s) => match number(s, "size")? {
            v if v > 0.0 && v.is_finite() => Some(v),
            _ => return Err(FigError::at("\"size\" must be a number above 0", s.at)),
        },
    };
    Ok((group, size))
}

/// An edge's label, weight and whether it's directed (`directed` unless it says).
pub(crate) fn edge_extras(e: &Node, directed: bool) -> Result<(Option<String>, Option<f64>, bool)> {
    let label = opt_string(e, "label")?;
    let weight = match field(e, "weight") {
        None => None,
        Some(w) => match number(w, "weight")? {
            v if v.is_finite() => Some(v),
            _ => return Err(FigError::at("\"weight\" must be a number", w.at)),
        },
    };
    let directed = match field(e, "directed") {
        Some(d) => boolean(d, "directed")?,
        None => directed,
    };
    Ok((label, weight, directed))
}

/// A layout's name in a spec.
pub fn layout_name(l: &Layout) -> &'static str {
    match l {
        Layout::Auto => "auto",
        Layout::Force => "force",
        Layout::Tree { .. } => "tree",
        Layout::Layered => "layered",
    }
}

/// `layout`: a name, or `{"type": …, "root": …}` for a tree from a known node.
pub(crate) fn layout(n: Option<&Node>, ids: &BTreeSet<String>) -> Result<Layout> {
    let Some(n) = n else {
        return Ok(Layout::Auto);
    };
    let (name, at, root) = match n.as_obj() {
        Some(_) => {
            let Some(t) = field(n, "type") else {
                return Err(FigError::at("a layout needs a \"type\"", n.at));
            };
            obj(n, "\"layout\"")?;
            let root = field(n, "root").map(|r| (id_of(r, "root"), r.at));
            (string(t, "type")?, t.at, root)
        }
        None => (string(n, "layout")?, n.at, None),
    };
    let root = match root {
        None => None,
        Some((r, at)) => {
            let r = r?;
            if !ids.contains(&r) {
                return Err(FigError::at(format!("unknown layout root: {r}"), at));
            }
            Some(r)
        }
    };
    match name.as_str() {
        "force" | "layered" | "auto" if root.is_some() => {
            Err(FigError::at("only a \"tree\" layout has a \"root\"", at))
        }
        "auto" => Ok(Layout::Auto),
        "force" => Ok(Layout::Force),
        "tree" => Ok(Layout::Tree { root }),
        "layered" => Ok(Layout::Layered),
        _ => Err(FigError::at(
            "\"layout\" must be \"force\", \"tree\", \"layered\" or \"auto\"",
            at,
        )),
    }
}

/// The names an id column goes by, in any case.
const ID_NAMES: &[&str] = &["id"];
/// The names a parent column goes by.
const PARENT_NAMES: &[&str] = &["parent", "parent_id", "parentid", "parent id", "parent-id"];
/// The names a label column goes by, the first found winning.
const LABEL_NAMES: &[&str] = &["label", "name", "title"];

/// The column named one of `names` (in any case, trimmed), the first of `names` found.
fn named<'t>(table: &'t Table, names: &[&str]) -> Option<&'t Column> {
    names.iter().find_map(|want| {
        table
            .columns
            .iter()
            .find(|c| c.name.trim().eq_ignore_ascii_case(want))
    })
}

/// A cell as an id or a label: a number as JSON writes it (`7`, not `7.0`), a string as it is;
/// None when it's missing or empty.
fn cell_text(c: &Column, row: usize) -> Option<String> {
    match &c.values {
        Values::Number(_) | Values::Time(_) => {
            let v = c.f64_at(row);
            v.is_finite().then(|| number_text(v))
        }
        _ => c
            .str_at(row)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
    }
}

fn number_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// A row's place: its line in a CSV with a header (row 0 on line 2), column 1. The byte is
/// unknown (0); a row whose fields hold line breaks puts the rows after it off by those.
fn row_at(row: usize) -> Pos {
    Pos {
        line: row as u32 + 2,
        col: 1,
        byte: 0,
    }
}

/// A row as a JSON object of its cells, for the detail panel: the source object where the rows
/// came from JSON, else each column's cell (a number, a string, or null where it's missing).
fn row_node(table: &Table, row: usize, at: Pos) -> Node {
    if let Some(raw) = table.raw.as_ref().and_then(|r| r.get(row)) {
        return raw.clone();
    }
    let cells = table
        .columns
        .iter()
        .map(|c| {
            let kind = match &c.values {
                Values::Number(_) | Values::Time(_) => {
                    let v = c.f64_at(row);
                    if v.is_finite() {
                        Kind::Num(v)
                    } else {
                        Kind::Null
                    }
                }
                _ => match c.str_at(row) {
                    Some(s) if !s.is_empty() => Kind::Str(s.into()),
                    _ => Kind::Null,
                },
            };
            (
                Key {
                    name: c.name.clone(),
                    at,
                },
                Node { kind, at },
            )
        })
        .collect();
    Node {
        kind: Kind::Obj(cells),
        at,
    }
}

/// A tree from a table with an `id` column and a `parent` column (`parent_id` too, in any
/// case): a node a row, in the rows' order, and an edge from each row's parent to it. A
/// `label`, `name` or `title` column labels the nodes, a `group` column colours them, a
/// positive `size` column sizes them, and each row's `fig` block (JSON rows) is its node's. A
/// row with no parent is a root. None when the table hasn't both columns.
///
/// Errors, each at its row's place (its line in a CSV with a header): a row with no id, `node
/// ids must be unique: …`, `unknown parent: …` (a mismatch).
pub fn from_table(table: &Table) -> Option<Result<GraphSpec>> {
    let id_col = named(table, ID_NAMES)?;
    let parent_col = named(table, PARENT_NAMES)?;
    Some(tree_of(table, id_col, parent_col))
}

fn tree_of(table: &Table, id_col: &Column, parent_col: &Column) -> Result<GraphSpec> {
    if table.rows == 0 {
        return Err(FigError::new("nothing to draw: the data has no rows")
            .with_kind(crate::error::ErrorKind::Empty));
    }
    let label_col = named(table, LABEL_NAMES);
    let group_col = named(table, &["group"]);
    let size_col = named(table, &["size"]).filter(|c| matches!(c.values, Values::Number(_)));
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut nodes = Vec::with_capacity(table.rows);
    for row in 0..table.rows {
        let at = row_at(row);
        let Some(id) = cell_text(id_col, row) else {
            return Err(FigError::at(
                format!("row {} has no \"{}\"", row + 1, id_col.name),
                at,
            ));
        };
        if !seen.insert(id.clone()) {
            return Err(FigError::at(
                format!("node ids must be unique: {id} is on two rows"),
                at,
            ));
        }
        let size = size_col
            .map(|c| c.f64_at(row))
            .filter(|v| v.is_finite() && *v > 0.0);
        let fig = table
            .styles
            .as_ref()
            .and_then(|s| s.get(row).cloned())
            .unwrap_or_default();
        nodes.push(GraphNode {
            label: label_col.and_then(|c| cell_text(c, row)),
            group: group_col.and_then(|c| cell_text(c, row)),
            size,
            fig,
            raw: row_node(table, row, at),
            at,
            id,
        });
    }
    let mut edges = Vec::new();
    for (row, node) in nodes.iter().enumerate() {
        let Some(parent) = cell_text(parent_col, row) else {
            continue;
        };
        let at = row_at(row);
        if !seen.contains(&parent) {
            return Err(FigError::at(format!("unknown parent: {parent}"), at));
        }
        let end = |s: &str| Node {
            kind: Kind::Str(s.into()),
            at,
        };
        let key = |s: &str| Key { name: s.into(), at };
        edges.push(GraphEdge {
            raw: Node {
                kind: Kind::Obj(vec![
                    (key("from"), end(&parent)),
                    (key("to"), end(&node.id)),
                ]),
                at,
            },
            from: parent,
            to: node.id.clone(),
            label: None,
            weight: None,
            directed: false,
            fig: FigStyle::default(),
            at,
        });
    }
    Ok(GraphSpec {
        title: None,
        subtitle: None,
        nodes,
        edges,
        layout: Layout::Tree { root: None },
        view: ViewSettings::graph(),
    })
}
