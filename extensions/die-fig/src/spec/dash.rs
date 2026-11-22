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
//! - a dashboard (the `.fig` layout): `title`; `data`, names each a data spec in the spec's
//!   order (embedded `{"values": …}`, a path, `{"run": …}`, `{"stream": …}`, or another
//!   dashboard's as `"x.fig#name"`); `time` (`field`, and `range` as `"6h"`); `tooltip`
//!   (`"single"`, the default, or `"shared"`: every linked panel shows the hovered moment's
//!   card); and `rows`, top to bottom. A row is a list of panels, or `{"height": 2, "panels":
//!   […]}` for a share of the height (1 by default). A panel is any figure but a dashboard,
//!   `{"file": "x.fig"}` (that whole figure, its paths from its own directory; a dashboard
//!   there is drawn as a nested group), or `{"column": [panels…]}` (panels stacked down the
//!   column), each with a `span`, its share of the row's width (1 by default). A panel's
//!   `"data": "req"` names the dashboard's data, and `"data": "x.fig#name"` borrows another
//!   dashboard's; a panel without `data` takes the dashboard's when there is only one.

use super::chart::{self, parse_duration};
use super::{
    Aggregate, Cx, DashboardSpec, Delta, Figure, GoodWhen, MeterSpec, Panel, PanelSpec, Row,
    StatSpec, Threshold, TimeSpec, Tooltip, arr, field, figure, number, obj, opt_string, string,
    whole, wrong,
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
pub const DASHBOARD: &[&str] = &["type", "title", "data", "time", "tooltip", "rows"];
/// A row's keys, written as an object.
pub const ROW: &[&str] = &["height", "panels"];

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
    let tooltip = match field(node, "tooltip") {
        None => Tooltip::Single,
        Some(t) => match t.as_str() {
            Some("single") => Tooltip::Single,
            Some("shared") => Tooltip::Shared,
            _ => {
                return Err(FigError::at(
                    "\"tooltip\" must be \"single\" or \"shared\"",
                    t.at,
                ));
            }
        },
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
        rows.push(row(r, &names, cx)?);
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
        tooltip,
        rows,
    })
}

/// One row: a list of panels, or an object of `height` and `panels`.
fn row(r: &Node, names: &[String], cx: &mut Cx<'_>) -> Result<Row> {
    let (ps, height) = match &r.kind {
        Kind::Arr(ps) => (ps.as_slice(), None),
        Kind::Obj(o) => {
            cx.check(o, ROW);
            let Some(ps) = field(r, "panels") else {
                return Err(FigError::at("a row needs \"panels\"", r.at));
            };
            let height = match field(r, "height") {
                None => None,
                Some(h) => match h.as_f64() {
                    Some(v) if v > 0.0 && v.is_finite() => Some(v),
                    _ => {
                        return Err(FigError::at("\"height\" must be a number above 0", h.at));
                    }
                },
            };
            (arr(ps, "panels")?, height)
        }
        _ => {
            return Err(FigError::at(
                format!(
                    "a row must be a list of panels or {{\"height\": …, \"panels\": […]}}, not {}",
                    super::a(r)
                ),
                r.at,
            ));
        }
    };
    if ps.is_empty() {
        return Err(FigError::at("a row needs a panel", r.at));
    }
    let mut panels = Vec::with_capacity(ps.len());
    for p in ps {
        panels.push(panel(p, names, cx)?);
    }
    Ok(Row {
        panels,
        height,
        at: r.at,
    })
}

/// One panel: a figure, another spec's file, or a column of panels.
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
            at: f.at,
        });
    }
    if field(p, "type").is_none()
        && let Some(c) = field(p, "column")
    {
        cx.check(po, &["column", "span"]);
        let cs = arr(c, "column")?;
        if cs.is_empty() {
            return Err(FigError::at("a column needs a panel", c.at));
        }
        let mut inner = Vec::with_capacity(cs.len());
        for q in cs {
            inner.push(panel(q, names, cx)?);
        }
        return Ok(PanelSpec {
            panel: Panel::Column(inner),
            span,
            at: p.at,
        });
    }
    let (names_before, panel_before) = (cx.names.replace(names.to_vec()), cx.panel);
    cx.panel = true;
    let f = figure(p, cx);
    cx.names = names_before;
    cx.panel = panel_before;
    let f = f?;
    if matches!(f, Figure::Dashboard(_)) {
        return Err(FigError::at(
            "a panel can't be a dashboard; put it in a file and use {\"file\": …}",
            p.at,
        ));
    }
    Ok(PanelSpec {
        panel: Panel::Figure(Box::new(f)),
        span,
        at: p.at,
    })
}

/// A data reference to another dashboard's data, `x.fig#name`: the file and the name, split at
/// the last `#`, when the file is a `.fig` or `.fig.json` and the name isn't empty.
pub fn borrowed(s: &str) -> Option<(&str, &str)> {
    let (file, name) = s.rsplit_once('#')?;
    let lower = file.to_ascii_lowercase();
    let fig = lower.ends_with(".fig") || lower.ends_with(".fig.json");
    (fig && !name.is_empty() && file.len() > ".fig".len()).then_some((file, name))
}
