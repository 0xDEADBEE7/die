//! The sparkline figure: one series' trend drawn small, with its newest value beside it.
//!
//! `{"type": "sparkline", "data": …, "x": "time", "y": "rpm"}`, and optionally `label` (what
//! it's of: else the `y` field's name), `format` (how the value is written, as a channel's)
//! and `unit` (written small after the value). `x` is a time or a number the line runs along,
//! `y` the numbers it shows; rows that share an `x` are summed, as one bar a band is.

use super::chart;
use super::{Cx, DataSpec, field, obj, opt_string, string};
use crate::error::{FigError, Result};
use crate::json::Node;

/// A sparkline's keys.
pub const SPARKLINE: &[&str] = &["type", "label", "data", "x", "y", "format", "unit"];

/// A sparkline: one series along a time or a number, drawn small.
#[derive(Clone, Debug, PartialEq)]
pub struct SparklineSpec {
    /// What it's of: its `label`, else the `y` field's name.
    pub label: String,
    /// Its data.
    pub data: DataSpec,
    /// The field it runs along: a time or a number.
    pub x: String,
    /// The field it shows: numbers.
    pub y: String,
    /// How the newest value is written.
    pub format: Option<String>,
    /// What the values are counted in, written small after the value.
    pub unit: Option<String>,
}

/// A sparkline figure.
pub(crate) fn sparkline(node: &Node, cx: &mut Cx<'_>) -> Result<SparklineSpec> {
    let o = obj(node, "a sparkline")?;
    cx.check_top(o, SPARKLINE);
    let data = chart::data_of(node, cx)?;
    let need = |k: &str| -> Result<String> {
        match field(node, k) {
            Some(v) => {
                let s = string(v, k)?;
                if s.is_empty() {
                    return Err(FigError::at(format!("\"{k}\" must name a field"), v.at));
                }
                Ok(s)
            }
            None => Err(FigError::at(
                format!("a sparkline needs \"{k}\", a field's name"),
                node.at,
            )),
        }
    };
    let x = need("x")?;
    let y = need("y")?;
    Ok(SparklineSpec {
        label: opt_string(node, "label")?.unwrap_or_else(|| y.clone()),
        data,
        x,
        y,
        format: opt_string(node, "format")?,
        unit: opt_string(node, "unit")?,
    })
}
