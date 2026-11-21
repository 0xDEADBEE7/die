//! The graph spec beyond fig's: groups, sizes, weights, directed edges and a layout, with ids
//! checked unique and edges' ends checked known ([`super::fig`] reads fig's part and checks).
//!
//! - a node's `group` (a string or a number) colours it, and its `size` (above 0) scales it;
//! - an edge's `label`, `weight` (a number) and `directed` (true or false; the graph's
//!   `"directed"` gives every edge's default, false as fig's are);
//! - `layout`: `"force"` (fig's stress layout, the default), `"tree"`, `"layered"`, or
//!   `{"type": "tree", "root": id}`.

use super::{Layout, boolean, field, id_of, number, obj, opt_string, string};
use crate::error::{FigError, Result};
use crate::json::Node;
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
        Layout::Force => "force",
        Layout::Tree { .. } => "tree",
        Layout::Layered => "layered",
    }
}

/// `layout`: a name, or `{"type": …, "root": …}` for a tree from a known node.
pub(crate) fn layout(n: Option<&Node>, ids: &BTreeSet<String>) -> Result<Layout> {
    let Some(n) = n else {
        return Ok(Layout::Force);
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
        "force" | "layered" if root.is_some() => {
            Err(FigError::at("only a \"tree\" layout has a \"root\"", at))
        }
        "force" => Ok(Layout::Force),
        "tree" => Ok(Layout::Tree { root }),
        "layered" => Ok(Layout::Layered),
        _ => Err(FigError::at(
            "\"layout\" must be \"force\", \"tree\" or \"layered\"",
            at,
        )),
    }
}
