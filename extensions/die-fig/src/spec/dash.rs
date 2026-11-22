//! Dashboards, stats and meters: rows of panels over named data and a shared time range, and the
//! tiles that show one number.
//!
//! - a stat: `label`, `data`, `value` (`"count"` or `{"sum": "rpm"}`, as an aggregate
//!   transform's, or `{"last": "rpm"}`, the newest by `trend`'s field), `delta` (`"previous"`
//!   or a number), `trend` (the field a sparkline runs along: a time or a number), `format`,
//!   `unit` (`"ms"`, `"%"`) and `good` (`up`, the default, `down` or `neither`). Without a
//!   label it is the value's field, or "count";
//! - a meter: `label`, `data`, `value`, `max` (above 0), `format`, `unit` and `thresholds`, a
//!   list of `{"at": 0.8, "status": "warning"}`, `at` a fraction of `max` from 0 to 1 and
//!   `status` one of `good`, `warning`, `serious` and `critical`;
//! - a dashboard: `title`, `data` (names, each a data spec, in the spec's order), `time`
//!   (`field`, and `range` as `"6h"`) and `rows`, each a list of panels: a figure (any but a
//!   dashboard) with a `span` (columns, 1 by default), or `{"file": "x.fig.json", "span": …}`,
//!   relative to the spec's directory. A panel's `"data": "req"` names the dashboard's data;
//!   a panel without `data` takes it when there is only one.

use super::chart::{self, parse_duration};
use super::{
    Aggregate, Cx, DashboardSpec, Delta, Figure, GoodWhen, MeterSpec, Panel, PanelSpec, StatSpec,
    Threshold, TimeSpec, arr, field, figure, number, obj, opt_string, string, whole, wrong,
};
use crate::error::{FigError, Result};
use crate::json::{Kind, Node};
use crate::style::Status;

/// A stat's keys.
pub const STAT: &[&str] = &[
    "type", "label", "data", "value", "delta", "trend", "format", "unit", "good",
];
/// A meter's keys.
pub const METER: &[&str] = &[
    "type",
    "label",
    "data",
    "value",
    "max",
    "format",
    "unit",
    "thresholds",
];
/// A dashboard's keys.
pub const DASHBOARD: &[&str] = &["type", "title", "data", "time", "rows"];

/// A stat tile.
pub(crate) fn stat(node: &Node, cx: &mut Cx<'_>) -> Result<StatSpec> {
    let o = obj(node, "a stat")?;
    cx.check_top(o, STAT);
    let data = chart::data_of(node, cx)?;
    let value = value_of(node)?;
    let delta = match field(node, "delta") {
        None => None,
        Some(d) => Some(match &d.kind {
            Kind::Str(s) if &**s == "previous" => Delta::Previous,
            Kind::Num(v) => Delta::Value(*v),
            _ => return Err(wrong(d, "delta", "\"previous\" or a number")),
        }),
    };
    let good = match field(node, "good") {
        None => GoodWhen::Up,
        Some(g) => match g.as_str() {
            Some("up") => GoodWhen::Up,
            Some("down") => GoodWhen::Down,
            Some("neither") => GoodWhen::Neither,
            _ => {
                return Err(FigError::at(
                    "\"good\" must be \"up\", \"down\" or \"neither\"",
                    g.at,
                ));
            }
        },
    };
    Ok(StatSpec {
        label: label_of(node, &value)?,
        data,
        value,
        delta,
        trend: opt_string(node, "trend")?,
        format: opt_string(node, "format")?,
        unit: opt_string(node, "unit")?,
        good,
    })
}

/// A meter tile.
pub(crate) fn meter(node: &Node, cx: &mut Cx<'_>) -> Result<MeterSpec> {
    let o = obj(node, "a meter")?;
    cx.check_top(o, METER);
    let data = chart::data_of(node, cx)?;
    let value = value_of(node)?;
    let max = match field(node, "max") {
        None => return Err(FigError::at("a meter needs a \"max\"", node.at)),
        Some(m) => match number(m, "max")? {
            v if v > 0.0 && v.is_finite() => v,
            _ => return Err(FigError::at("\"max\" must be a number above 0", m.at)),
        },
    };
    let thresholds = match field(node, "thresholds") {
        None => Vec::new(),
        Some(t) => thresholds(t, cx)?,
    };
    Ok(MeterSpec {
        label: label_of(node, &value)?,
        data,
        value,
        max,
        format: opt_string(node, "format")?,
        unit: opt_string(node, "unit")?,
        thresholds,
    })
}

/// A meter's thresholds, lowest first.
fn thresholds(t: &Node, cx: &mut Cx<'_>) -> Result<Vec<Threshold>> {
    let mut out = Vec::new();
    for th in arr(t, "thresholds")? {
        let o = obj(th, "a threshold")?;
        cx.check(o, &["at", "status"]);
        let Some(at) = field(th, "at") else {
            return Err(FigError::at(
                "a threshold needs an \"at\", a fraction of the max",
                th.at,
            ));
        };
        let v = number(at, "at")?;
        if !(0.0..=1.0).contains(&v) {
            return Err(FigError::at(
                "\"at\" is a fraction of the max, from 0 to 1",
                at.at,
            ));
        }
        let Some(st) = field(th, "status") else {
            return Err(FigError::at("a threshold needs a \"status\"", th.at));
        };
        let status = match st.as_str() {
            Some("good") => Status::Good,
            Some("warning") => Status::Warning,
            Some("serious") => Status::Serious,
            Some("critical") => Status::Critical,
            _ => {
                return Err(FigError::at(
                    "\"status\" must be \"good\", \"warning\", \"serious\" or \"critical\"",
                    st.at,
                ));
            }
        };
        out.push(Threshold { at: v, status });
    }
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    Ok(out)
}

/// A tile's `value`.
fn value_of(node: &Node) -> Result<Aggregate> {
    let Some(v) = field(node, "value") else {
        return Err(FigError::at(
            "a tile needs a \"value\", as {\"sum\": \"rpm\"}",
            node.at,
        ));
    };
    let (op, f) = chart::aggregate(v, "value")?;
    Ok(Aggregate { op, field: f })
}

/// A tile's `label`, else its value's field, else "count".
fn label_of(node: &Node, value: &Aggregate) -> Result<String> {
    Ok(opt_string(node, "label")?
        .unwrap_or_else(|| value.field.clone().unwrap_or_else(|| "count".into())))
}

/// A dashboard.
pub(crate) fn dashboard(node: &Node, cx: &mut Cx<'_>) -> Result<DashboardSpec> {
    let o = obj(node, "a dashboard")?;
    cx.check_top(o, DASHBOARD);
    let mut data = Vec::new();
    if let Some(d) = field(node, "data") {
        for (k, v) in obj(d, "a dashboard's \"data\"")? {
            if k.name.is_empty() {
                return Err(FigError::at("a data name can't be empty", k.at));
            }
            data.push((k.name.to_string(), chart::data_spec(v, cx)?));
        }
    }
    let time = match field(node, "time") {
        None => None,
        Some(t) => {
            let to = obj(t, "\"time\"")?;
            cx.check(to, &["field", "range"]);
            let Some(f) = field(t, "field") else {
                return Err(FigError::at("\"time\" needs a \"field\"", t.at));
            };
            let range = match field(t, "range") {
                None => None,
                Some(r) => {
                    let s = string(r, "range")?;
                    if parse_duration(&s).is_none_or(|d| d.is_zero()) {
                        return Err(FigError::at(
                            "\"range\" must be a time like \"6h\" or \"7d\"",
                            r.at,
                        ));
                    }
                    Some(s)
                }
            };
            Some(TimeSpec {
                field: string(f, "field")?,
                range,
            })
        }
    };
    let Some(rows_node) = field(node, "rows") else {
        return Err(FigError::at(
            "a dashboard needs \"rows\" of panels",
            node.at,
        ));
    };
    let names: Vec<String> = data.iter().map(|(k, _)| k.clone()).collect();
    let mut rows = Vec::new();
    for r in arr(rows_node, "rows")? {
        let ps = arr(r, "a row")?;
        if ps.is_empty() {
            return Err(FigError::at("a row needs a panel", r.at));
        }
        let mut row = Vec::with_capacity(ps.len());
        for p in ps {
            row.push(panel(p, &names, cx)?);
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(FigError::at(
            "a dashboard needs \"rows\" of panels",
            rows_node.at,
        ));
    }
    Ok(DashboardSpec {
        title: opt_string(node, "title")?,
        data,
        time,
        rows,
    })
}

/// One panel: a figure, or another spec's file.
fn panel(p: &Node, names: &[String], cx: &mut Cx<'_>) -> Result<PanelSpec> {
    let po = obj(p, "a panel")?;
    let span = match field(p, "span") {
        None => 1,
        Some(s) => whole(s, "span", 1)?.min(u64::from(u16::MAX)) as u16,
    };
    if field(p, "type").is_none()
        && let Some(f) = field(p, "file")
    {
        cx.check(po, &["file", "span"]);
        let s = string(f, "file")?;
        if s.is_empty() {
            return Err(FigError::at("\"file\" must name a file", f.at));
        }
        return Ok(PanelSpec {
            panel: Panel::File(chart::resolve(cx.dir, &s)),
            span,
        });
    }
    let (names_before, panel_before) = (cx.names.replace(names.to_vec()), cx.panel);
    cx.panel = true;
    let f = figure(p, cx);
    cx.names = names_before;
    cx.panel = panel_before;
    let f = f?;
    if matches!(f, Figure::Dashboard(_)) {
        return Err(FigError::at("a panel can't be a dashboard", p.at));
    }
    Ok(PanelSpec {
        panel: Panel::Figure(Box::new(f)),
        span,
    })
}
