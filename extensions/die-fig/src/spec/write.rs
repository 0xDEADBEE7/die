//! A figure back to JSON text, for `:fig-spec`: pretty, keys in a fixed order, so a guessed
//! figure becomes a spec to edit and reads back as the same figure.
//!
//! [`parse`](super::parse) of the text gives the figure again, but for the places in the text
//! ([`Figure::without_places`] compares them), and writing that gives the same text. What it
//! writes:
//!
//! - a chart that is exactly fig's line or histogram (as [`super::fig`] makes them) as fig's
//!   own shape, so fig reads it too, and series of one name stay apart;
//! - any other chart as `"type": "chart"`: one layer at the top with its `mark`; several
//!   under `layers`, the encodings they all share at the top;
//! - a value only where it isn't the default: a mark's name alone when the rest is its
//!   default, a channel's field alone when it has nothing else, a view's settings that differ;
//! - a node or an edge as its object in the spec when that object still says what the node or
//!   edge is, else its fields and then the object's other keys;
//! - inline data as `values`: its rows' objects if it came from JSON objects; else its columns,
//!   a key a column of its cells (and `fig`, the rows' blocks, if it has them), or, when a
//!   column is named `fig`, a header row and a row a list; numbers as numbers, times in RFC 3339
//!   to the fewest decimals that read back as the same seconds, missing cells `null`;
//! - a file by its path, as it was resolved against the spec's (absolute) directory; a command
//!   with its `every`; the buffer as `"-"`.
//!
//! Lists of numbers and strings fill their lines rather than taking a line each.

use super::chart::{MAX_BINS, agg_name, format_duration, format_name, kind_name, scale_name};
use super::graph::{edge_extras, layout_name, node_extras};
use super::{
    Aggregate, Channel, ChannelValue, Chart, Curve, DashboardSpec, DataRef, DataSpec, Delta,
    Encoding, Figure, GoodWhen, GraphEdge, GraphNode, GraphSpec, Labels, Layer, Layout, LegendPos,
    Mark, MarkKind, MeterSpec, Panel, PanelSpec, ReadHints, ScaleKind, Stack, StatSpec,
    ViewSettings, field, id_of, opt_string,
};
use crate::data::transform::{AggOp, FilterOp, Transform};
use crate::data::{Column, SeriesKey, Table, Values};
use crate::error::Pos;
use crate::json::{Kind, Node};
use crate::style::FigStyle;

/// A figure as a spec's text.
pub fn write(figure: &Figure) -> String {
    let mut out = String::new();
    pretty(&figure_j(figure, &[]), 0, 0, &mut out);
    out.push('\n');
    out
}

/// The widest a line gets before an array or an object breaks over lines.
const WIDTH: usize = 100;

/// A JSON value being written: keys in the order they're pushed.
#[derive(Clone, Debug, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
    /// An object whose keys come from the data, not from here.
    Map(Vec<(String, J)>),
}

impl J {
    fn s(s: &str) -> J {
        J::Str(s.to_string())
    }

    fn strs(v: &[String]) -> J {
        J::Arr(v.iter().map(|s| J::s(s)).collect())
    }
}

/// An object under construction.
#[derive(Default)]
struct O(Vec<(&'static str, J)>);

impl O {
    fn put(&mut self, k: &'static str, v: J) {
        self.0.push((k, v));
    }

    fn opt(&mut self, k: &'static str, v: &Option<String>) {
        if let Some(s) = v {
            self.put(k, J::s(s));
        }
    }

    fn done(self) -> J {
        J::Obj(self.0)
    }
}

fn figure_j(f: &Figure, names: &[String]) -> J {
    match f {
        Figure::Chart(c) => chart_j(c, names),
        Figure::Graph(g) => graph_j(g),
        Figure::Stat(s) => stat_j(s, names),
        Figure::Meter(m) => meter_j(m, names),
        Figure::Dashboard(d) => dashboard_j(d),
    }
}

fn chart_j(c: &Chart, names: &[String]) -> J {
    if let Some(j) = fig_line(c).or_else(|| fig_histogram(c)) {
        return j;
    }
    let mut o = O::default();
    o.put("type", J::s("chart"));
    o.opt("title", &c.title);
    o.opt("subtitle", &c.subtitle);
    o.put("data", data_j(&c.data, names));
    if !c.transform.is_empty() {
        o.put(
            "transform",
            J::Arr(c.transform.iter().map(transform_j).collect()),
        );
    }
    match c.layers.as_slice() {
        [l] if l.data.is_none() && l.transform.is_empty() => {
            enc_into(&mut o, &l.enc, &Encoding::default());
            o.put("mark", mark_j(&l.mark));
            if l.fig != FigStyle::default() {
                o.put("fig", style_j(&l.fig));
            }
        }
        layers => {
            let common = common(layers);
            enc_into(&mut o, &common, &Encoding::default());
            o.put(
                "layers",
                J::Arr(layers.iter().map(|l| layer_j(l, &common, names)).collect()),
            );
        }
    }
    if !c.series_fig.is_empty() {
        o.put("series", series_j(&c.series_fig));
    }
    if c.view != ViewSettings::default() {
        o.put("view", view_j(&c.view, &ViewSettings::default()));
    }
    o.done()
}

/// The encodings every layer has the same of.
fn common(layers: &[Layer]) -> Encoding {
    // Channels are the same whatever their places in the text.
    let same = |get: fn(&Encoding) -> &Option<Channel>| -> Option<Channel> {
        let first = unplaced(get(&layers.first()?.enc))?;
        layers
            .iter()
            .all(|l| unplaced(get(&l.enc)).as_ref() == Some(&first))
            .then(|| get(&layers[0].enc).clone())
            .flatten()
    };
    let same_list = |get: fn(&Encoding) -> &Vec<String>| -> Vec<String> {
        match layers.first() {
            Some(l) if layers.iter().all(|m| get(&m.enc) == get(&l.enc)) => get(&l.enc).clone(),
            _ => Vec::new(),
        }
    };
    Encoding {
        x: same(|e| &e.x),
        y: same(|e| &e.y),
        y2: same(|e| &e.y2),
        color: same(|e| &e.color),
        size: same(|e| &e.size),
        label: same(|e| &e.label),
        tooltip: same_list(|e| &e.tooltip),
        detail: same_list(|e| &e.detail),
    }
}

/// Each channel of `enc` that `top` doesn't give: a channel is at the top only when every
/// layer has it.
fn enc_into(o: &mut O, enc: &Encoding, top: &Encoding) {
    let chans: [(&'static str, &Option<Channel>, &Option<Channel>); 6] = [
        ("x", &enc.x, &top.x),
        ("y", &enc.y, &top.y),
        ("y2", &enc.y2, &top.y2),
        ("color", &enc.color, &top.color),
        ("size", &enc.size, &top.size),
        ("label", &enc.label, &top.label),
    ];
    for (k, c, t) in chans {
        if let Some(c) = c
            && t.is_none()
        {
            o.put(k, channel_j(c));
        }
    }
    if !enc.tooltip.is_empty() && top.tooltip.is_empty() {
        o.put("tooltip", J::strs(&enc.tooltip));
    }
    if !enc.detail.is_empty() && top.detail.is_empty() {
        o.put("detail", J::strs(&enc.detail));
    }
}

fn layer_j(l: &Layer, top: &Encoding, names: &[String]) -> J {
    let mut o = O::default();
    o.put("mark", mark_j(&l.mark));
    if let Some(d) = &l.data {
        o.put("data", data_j(d, names));
    }
    if !l.transform.is_empty() {
        o.put(
            "transform",
            J::Arr(l.transform.iter().map(transform_j).collect()),
        );
    }
    enc_into(&mut o, &l.enc, top);
    if l.fig != FigStyle::default() {
        o.put("fig", style_j(&l.fig));
    }
    o.done()
}

fn channel_j(c: &Channel) -> J {
    let bare = c.kind.is_none()
        && c.unit.is_none()
        && c.title.is_none()
        && c.scale.is_none()
        && c.domain.is_none()
        && c.zero.is_none()
        && c.format.is_none()
        && c.value.is_none();
    if bare && let Some(f) = &c.field {
        return J::s(f);
    }
    let mut o = O::default();
    o.opt("field", &c.field);
    match &c.value {
        Some(ChannelValue::Num(v)) => o.put("value", J::Num(*v)),
        Some(ChannelValue::Str(s)) => o.put("value", J::s(s)),
        None => {}
    }
    if let Some(k) = c.kind {
        o.put("type", J::s(kind_name(k)));
    }
    o.opt("unit", &c.unit);
    o.opt("title", &c.title);
    if let Some(s) = c.scale {
        o.put("scale", J::s(scale_name(s)));
    }
    if let Some((lo, hi)) = c.domain {
        o.put("domain", J::Arr(vec![J::Num(lo), J::Num(hi)]));
    }
    if let Some(z) = c.zero {
        o.put("zero", J::Bool(z));
    }
    o.opt("format", &c.format);
    o.done()
}

fn mark_j(m: &Mark) -> J {
    if *m == Mark::new(m.kind) {
        return J::s(m.kind.name());
    }
    let mut o = O::default();
    o.put("type", J::s(m.kind.name()));
    if m.curve == Curve::Step {
        o.put("curve", J::s("step"));
    }
    if m.points {
        o.put("points", J::Bool(true));
    }
    if m.stack != MarkKind::default_stack(m.kind) {
        let s = match m.stack {
            Stack::None => "none",
            Stack::Zero => "zero",
            Stack::Normalize => "normalize",
        };
        o.put("stack", J::s(s));
    }
    if m.horizontal {
        o.put("horizontal", J::Bool(true));
    }
    o.done()
}

fn style_j(st: &FigStyle) -> J {
    let mut o = O::default();
    if let Some(c) = st.color {
        o.put("color", J::Str(c.name()));
    }
    if let Some(s) = st.size {
        o.put("size", J::Num(f64::from(s)));
    }
    if st.override_focus {
        o.put("override-focus", J::Bool(true));
    }
    if let Some(b) = st.show_label {
        o.put("show-label", J::Bool(b));
    }
    if st.hidden {
        o.put("hidden", J::Bool(true));
    }
    o.opt("label", &st.label);
    if st.dash {
        o.put("dash", J::Bool(true));
    }
    o.done()
}

fn view_j(v: &ViewSettings, base: &ViewSettings) -> J {
    let mut o = O::default();
    if v.grid != base.grid {
        o.put("grid", J::Bool(v.grid));
    }
    if v.axes != base.axes {
        o.put("axes", J::Bool(v.axes));
    }
    if v.legend != base.legend {
        let l = match v.legend {
            LegendPos::Top => "top",
            LegendPos::Right => "right",
            LegendPos::None => "none",
        };
        o.put("legend", J::s(l));
    }
    if v.labels != base.labels {
        let l = match v.labels {
            Labels::Auto => "auto",
            Labels::Ends => "ends",
            Labels::None => "none",
        };
        o.put("labels", J::s(l));
    }
    if v.render_options != base.render_options {
        o.put("render_options", J::Bool(v.render_options));
    }
    if v.motion != base.motion {
        o.put("motion", J::Bool(v.motion));
    }
    o.done()
}

/// `series`: blocks by name, a list where a name has several series.
fn series_j(styles: &[(SeriesKey, FigStyle)]) -> J {
    let mut by: Vec<(String, Vec<J>)> = Vec::new();
    for (k, st) in styles {
        let i = match by.iter().position(|(n, _)| **n == *k.name) {
            Some(i) => i,
            None => {
                by.push((k.name.to_string(), Vec::new()));
                by.len() - 1
            }
        };
        let list = &mut by[i].1;
        let nth = k.nth as usize;
        if list.len() <= nth {
            list.resize(nth + 1, J::Null);
        }
        list[nth] = style_j(st);
    }
    J::Map(
        by.into_iter()
            .map(|(n, mut l)| {
                let v = if l.len() == 1 { l.remove(0) } else { J::Arr(l) };
                (n, v)
            })
            .collect(),
    )
}

fn data_j(d: &DataSpec, names: &[String]) -> J {
    let plain = d.format.is_none() && d.read == ReadHints::default();
    let mut o = O::default();
    match &d.source {
        DataRef::File(p) => {
            let s = p.to_string_lossy().into_owned();
            if plain && s != "-" && !names.contains(&s) {
                return J::Str(s);
            }
            o.put("file", J::Str(s));
        }
        DataRef::Source if plain => return J::s("-"),
        DataRef::Source => o.put("file", J::s("-")),
        DataRef::Named(k) => return J::s(k),
        DataRef::Inline(t) => o.put("values", rows_j(t)),
        DataRef::Run { cmd, every } => {
            o.put("run", J::s(cmd));
            if let Some(e) = every {
                o.put("every", J::Str(format_duration(*e)));
            }
        }
        DataRef::Stream { cmd } => o.put("stream", J::s(cmd)),
    }
    if let Some(f) = d.format {
        o.put("format", J::s(format_name(f)));
    }
    let h = &d.read;
    if let Some(c) = h.delimiter {
        o.put("delimiter", J::Str(char::from(c).to_string()));
    }
    if let Some(b) = h.header {
        o.put("header", J::Bool(b));
    }
    if let Some(c) = h.thousands {
        o.put("thousands", J::Str(c.to_string()));
    }
    o.opt("time_format", &h.time_format);
    o.done()
}

/// A table's rows: the objects they came from; else its columns, a key a column of its cells
/// and `fig` the rows' blocks, which the reader reads back with no objects; or, with a column
/// named `fig`, which as a key would be the blocks, a header row and a row a list (a table with
/// that column came from text, and has no blocks).
fn rows_j(t: &Table) -> J {
    if let Some(raw) = &t.raw {
        return J::Arr(raw.iter().map(node_j).collect());
    }
    if t.column("fig").is_some() {
        let head = J::Arr(t.columns.iter().map(|c| J::s(&c.name)).collect());
        let rows = (0..t.rows).map(|r| J::Arr(t.columns.iter().map(|c| cell_j(c, r)).collect()));
        return J::Arr(std::iter::once(head).chain(rows).collect());
    }
    let mut m: Vec<(String, J)> = t
        .columns
        .iter()
        .map(|c| {
            let cells = (0..t.rows).map(|r| cell_j(c, r)).collect();
            (c.name.to_string(), J::Arr(cells))
        })
        .collect();
    if let Some(st) = &t.styles {
        m.push(("fig".into(), J::Arr(st.iter().map(style_j).collect())));
    }
    J::Map(m)
}

/// One cell: a number, a time's text, a string, or `null` for a missing one.
fn cell_j(c: &Column, r: usize) -> J {
    match &c.values {
        Values::Number(_) => finite(c.f64_at(r)),
        Values::Time(_) => time_j(c.f64_at(r)),
        _ => c.str_at(r).map_or(J::Null, J::s),
    }
}

fn finite(v: f64) -> J {
    if v.is_finite() { J::Num(v) } else { J::Null }
}

/// A time as RFC 3339 in UTC, in whole seconds, milliseconds, microseconds or nanoseconds,
/// the first that reads back as the same seconds (as the time reader makes them), else to the
/// nearest nanosecond; `null` for a time outside the years a timestamp has, which as a number
/// would make its column no longer one of times.
fn time_j(v: f64) -> J {
    // Beyond jiff's years (±9999), and far enough inside i128's nanoseconds.
    if !v.is_finite() || v.abs() >= 1e12 {
        return J::Null;
    }
    let secs = v.floor();
    // Exact where `v` is at least 1 from zero (it and its floor are within a factor of two) or
    // in [0, 1); in (-1, 0) only digits far finer than a nanosecond are lost.
    let frac = v - secs;
    let at = |digits: i32| {
        let n = (frac * 10f64.powi(digits)).round() as i128;
        let nanos = secs as i128 * 1_000_000_000 + n * 10i128.pow((9 - digits) as u32);
        jiff::Timestamp::from_nanosecond(nanos).ok()
    };
    let exact = [0, 3, 6, 9]
        .into_iter()
        .filter_map(at)
        .find(|ts| ts.as_duration().as_secs_f64() == v);
    match exact.or_else(|| at(9)) {
        Some(ts) => J::Str(ts.to_string()),
        None => J::Null,
    }
}

fn node_j(n: &Node) -> J {
    match &n.kind {
        Kind::Null => J::Null,
        Kind::Bool(b) => J::Bool(*b),
        Kind::Num(v) => J::Num(*v),
        Kind::Str(s) => J::s(s),
        Kind::Arr(a) => J::Arr(a.iter().map(node_j).collect()),
        Kind::Obj(o) => J::Map(
            o.iter()
                .map(|(k, v)| (k.name.to_string(), node_j(v)))
                .collect(),
        ),
    }
}

fn transform_j(t: &Transform) -> J {
    let mut o = O::default();
    let by = |o: &mut O, by: &[String]| {
        if !by.is_empty() {
            o.put("by", J::strs(by));
        }
    };
    match t {
        Transform::Fold { fields, key, value } => {
            o.put("fold", J::strs(fields));
            if key != "key" || value != "value" {
                o.put("as", J::Arr(vec![J::s(key), J::s(value)]));
            }
        }
        Transform::Filter { field, op } => {
            o.put("filter", J::s(field));
            match op {
                FilterOp::In(l) => o.put("in", J::strs(l)),
                FilterOp::NotIn(l) => o.put("not_in", J::strs(l)),
                FilterOp::Range(lo, hi) => {
                    let end = |e: &Option<f64>| e.map_or(J::Null, J::Num);
                    o.put("range", J::Arr(vec![end(lo), end(hi)]));
                }
            }
        }
        Transform::Bin {
            field,
            step,
            max_bins,
            start,
            end,
        } => {
            o.put("bin", J::s(field));
            if let Some(s) = step {
                o.put("step", J::Num(*s));
            }
            if *max_bins != MAX_BINS {
                o.put("max_bins", J::Num(*max_bins as f64));
            }
            if *start != format!("{field}_start") || *end != format!("{field}_end") {
                o.put("as", J::Arr(vec![J::s(start), J::s(end)]));
            }
        }
        Transform::Aggregate {
            op,
            field,
            by: b,
            out,
        } => {
            o.put("aggregate", agg_j(*op, field));
            by(&mut o, b);
            if *out != *field.as_deref().unwrap_or("count") {
                o.put("as", J::s(out));
            }
        }
        Transform::Cumulative { field, by: b, out } => {
            o.put("cumulative", J::s(field));
            by(&mut o, b);
            if *out != format!("{field}_cumulative") {
                o.put("as", J::s(out));
            }
        }
        Transform::Window {
            field,
            size,
            by: b,
            out,
        } => {
            o.put("window", J::s(field));
            o.put("size", J::Num(*size as f64));
            by(&mut o, b);
            if *out != format!("{field}_mean") {
                o.put("as", J::s(out));
            }
        }
        Transform::Sort { field, descending } => {
            o.put("sort", J::s(field));
            if *descending {
                o.put("descending", J::Bool(true));
            }
        }
        Transform::Top { n, by, field } => {
            o.put("top", J::Num(*n as f64));
            o.put("by", J::s(by));
            o.put("field", J::s(field));
        }
    }
    o.done()
}

fn agg_j(op: AggOp, field: &Option<String>) -> J {
    match (op, field) {
        (AggOp::Count, None) => J::s("count"),
        (op, Some(f)) => J::Map(vec![(agg_name(op).into(), J::s(f))]),
        (op, None) => J::Map(vec![(agg_name(op).into(), J::Null)]),
    }
}

/// A channel with its place forgotten, for comparing with one made here.
fn unplaced(c: &Option<Channel>) -> Option<Channel> {
    c.clone().map(|c| Channel { at: None, ..c })
}

fn on(field: &str, title: &Option<String>, scale: Option<ScaleKind>) -> Option<Channel> {
    Some(Channel {
        field: Some(field.into()),
        title: title.clone(),
        scale,
        ..Channel::default()
    })
}

/// The inline table of a chart drawn by one layer of `kind` with no transforms, if it is one.
fn one_inline_layer(c: &Chart, kind: MarkKind) -> Option<(&Table, &Layer)> {
    let [l] = c.layers.as_slice() else {
        return None;
    };
    let DataRef::Inline(t) = &c.data.source else {
        return None;
    };
    let plain = c.data.format.is_none()
        && c.data.read == ReadHints::default()
        && c.transform.is_empty()
        && l.data.is_none()
        && l.transform.is_empty()
        && l.mark == Mark::new(kind)
        && l.enc.y2.is_none()
        && l.enc.size.is_none()
        && l.enc.label.is_none()
        && l.enc.tooltip.is_empty()
        && l.enc.detail.is_empty()
        && t.raw.is_none()
        && t.styles.is_none();
    plain.then_some((&**t, l))
}

/// A category column's codes and entries, if column `i` of `t` is one named `name`.
fn category<'t>(t: &'t Table, i: usize, name: &str) -> Option<(&'t [u32], &'t [Box<str>])> {
    let c = t.columns.get(i).filter(|c| &*c.name == name)?;
    match &c.values {
        Values::Category { codes, dict } => Some((codes, dict)),
        _ => None,
    }
}

fn numbers<'t>(t: &'t Table, i: usize, name: &str) -> Option<&'t [f64]> {
    let c = t.columns.get(i).filter(|c| &*c.name == name)?;
    match &c.values {
        Values::Number(v) => Some(v),
        _ => None,
    }
}

/// A chart that is fig's line as [`super::fig`] makes it, as fig's line.
fn fig_line(c: &Chart) -> Option<J> {
    let (t, l) = one_inline_layer(c, MarkKind::Line)?;
    if t.columns.len() != 3 {
        return None;
    }
    let (codes, dict) = category(t, 0, "series")?;
    let (xs, ys) = (numbers(t, 1, "x")?, numbers(t, 2, "y")?);
    if xs.len() != codes.len() || ys.len() != codes.len() {
        return None;
    }
    let x_label = &unplaced(&l.enc.x)?.title;
    let y_label = &unplaced(&l.enc.y)?.title;
    if unplaced(&l.enc.x) != on("x", x_label, None)
        || unplaced(&l.enc.y) != on("y", y_label, None)
        || unplaced(&l.enc.color) != on("series", &None, None)
        || dict.is_empty()
    {
        return None;
    }
    // Each series a run of rows in order, each in x order, every number finite.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (r, &code) in codes.iter().enumerate() {
        let code = code as usize;
        if code + 1 == runs.len() {
            runs[code].1 = r + 1;
        } else if code == runs.len() {
            runs.push((r, r + 1));
        } else {
            return None;
        }
    }
    if runs.len() != dict.len() {
        return None;
    }
    for &(a, b) in &runs {
        if xs[a..b].windows(2).any(|w| w[1] < w[0]) {
            return None;
        }
    }
    if xs.iter().chain(ys).any(|v| !v.is_finite()) {
        return None;
    }
    let styles = styles_of(c, dict)?;
    let series: Vec<J> = runs
        .iter()
        .zip(dict.iter())
        .enumerate()
        .map(|(i, (&(a, b), name))| {
            let mut o = O::default();
            o.put("label", J::s(name));
            if let Some(st) = &styles[i] {
                o.put("fig", style_j(st));
            }
            let points = (a..b)
                .map(|r| J::Obj(vec![("x", J::Num(xs[r])), ("y", J::Num(ys[r]))]))
                .collect();
            o.put("points", J::Arr(points));
            o.done()
        })
        .collect();
    let mut o = O::default();
    o.put("type", J::s("line"));
    o.opt("title", &c.title);
    o.opt("subtitle", &c.subtitle);
    o.opt("x_label", x_label);
    o.opt("y_label", y_label);
    o.put("series", J::Arr(series));
    fig_tail(&mut o, l, &c.view);
    Some(o.done())
}

/// A chart that is fig's histogram as [`super::fig`] makes it, as fig's histogram.
fn fig_histogram(c: &Chart) -> Option<J> {
    let (t, l) = one_inline_layer(c, MarkKind::Bar)?;
    if t.columns.len() != 3 {
        return None;
    }
    let (bcodes, buckets) = category(t, 0, "bucket")?;
    let (scodes, names) = category(t, 1, "series")?;
    let values = numbers(t, 2, "value")?;
    let x_label = &unplaced(&l.enc.x)?.title;
    let y_label = &unplaced(&l.enc.y)?.title;
    let (nb, ns) = (buckets.len(), names.len());
    if unplaced(&l.enc.x) != on("bucket", x_label, Some(ScaleKind::Band))
        || unplaced(&l.enc.y) != on("value", y_label, None)
        || unplaced(&l.enc.color) != on("series", &None, None)
        || nb == 0
        || ns == 0
        || t.rows != nb * ns
        || [bcodes.len(), scodes.len(), values.len()] != [t.rows; 3]
        || buckets.iter().any(|b| b.is_empty())
        || names
            .iter()
            .enumerate()
            .any(|(i, n)| n.is_empty() || names[..i].contains(n))
        || (0..t.rows).any(|r| bcodes[r] as usize != r / ns || scodes[r] as usize != r % ns)
        || values.iter().any(|v| !(v.is_finite() && *v >= 0.0))
        || c.series_fig.iter().any(|(k, _)| k.nth != 0)
    {
        return None;
    }
    let styles = styles_of(c, names)?;
    let series = names
        .iter()
        .zip(&styles)
        .map(|(n, st)| {
            let mut o = O::default();
            o.put("label", J::s(n));
            if let Some(st) = st {
                o.put("fig", style_j(st));
            }
            o.done()
        })
        .collect();
    let rows = buckets
        .iter()
        .enumerate()
        .map(|(b, label)| {
            let vs: Vec<(String, J)> = (0..ns)
                .filter(|&s| values[b * ns + s] != 0.0)
                .map(|s| (names[s].to_string(), J::Num(values[b * ns + s])))
                .collect();
            let mut o = O::default();
            o.put("label", J::s(label));
            if !vs.is_empty() {
                o.put("values", J::Map(vs));
            }
            o.done()
        })
        .collect();
    let mut o = O::default();
    o.put("type", J::s("histogram"));
    o.opt("title", &c.title);
    o.opt("subtitle", &c.subtitle);
    o.opt("x_label", x_label);
    o.opt("y_label", y_label);
    o.put("series", J::Arr(series));
    o.put("buckets", J::Arr(rows));
    fig_tail(&mut o, l, &c.view);
    Some(o.done())
}

/// Each series' block in `dict`'s order, if every block in `series_fig` is of a series there.
fn styles_of(c: &Chart, dict: &[Box<str>]) -> Option<Vec<Option<FigStyle>>> {
    let mut out: Vec<Option<FigStyle>> = vec![None; dict.len()];
    for (k, st) in &c.series_fig {
        let i = dict
            .iter()
            .enumerate()
            .filter(|(_, d)| ***d == *k.name)
            .nth(k.nth as usize)?
            .0;
        if out[i].is_some() || *st == FigStyle::default() {
            return None;
        }
        out[i] = Some(st.clone());
    }
    // The order of series_fig must be the series' order, as fig's reader makes it.
    let order: Vec<&FigStyle> = out.iter().flatten().collect();
    let given: Vec<&FigStyle> = c.series_fig.iter().map(|(_, s)| s).collect();
    (order == given).then_some(out)
}

/// A fig shape's `fig` (the layer's block) and `view`.
fn fig_tail(o: &mut O, l: &Layer, view: &ViewSettings) {
    if l.fig != FigStyle::default() {
        o.put("fig", style_j(&l.fig));
    }
    if *view != ViewSettings::default() {
        o.put("view", view_j(view, &ViewSettings::default()));
    }
}

fn graph_j(g: &GraphSpec) -> J {
    // The graph's "directed" is whichever default leaves more edges' objects as they are.
    let fits = |d: bool| g.edges.iter().filter(|e| edge_fits(e, d)).count();
    let directed = !g.edges.is_empty() && fits(true) > fits(false);
    let mut o = O::default();
    o.put("type", J::s("graph"));
    o.opt("title", &g.title);
    o.opt("subtitle", &g.subtitle);
    match &g.layout {
        Layout::Auto => {}
        Layout::Tree { root: Some(r) } => o.put(
            "layout",
            J::Obj(vec![("type", J::s("tree")), ("root", J::s(r))]),
        ),
        l => o.put("layout", J::s(layout_name(l))),
    }
    if directed {
        o.put("directed", J::Bool(true));
    }
    o.put("nodes", J::Arr(g.nodes.iter().map(node_of).collect()));
    if !g.edges.is_empty() {
        o.put(
            "edges",
            J::Arr(g.edges.iter().map(|e| edge_of(e, directed)).collect()),
        );
    }
    if g.view != ViewSettings::graph() {
        o.put("view", view_j(&g.view, &ViewSettings::graph()));
    }
    o.done()
}

/// Whether a node's object still says what the node is.
fn node_fits(n: &GraphNode) -> bool {
    let raw = &n.raw;
    raw.as_obj().is_some()
        && field(raw, "id").and_then(|i| id_of(i, "id").ok()).as_ref() == Some(&n.id)
        && opt_string(raw, "label").ok() == Some(n.label.clone())
        && node_extras(raw).ok() == Some((n.group.clone(), n.size))
        && fig_of(raw) == n.fig
}

fn edge_fits(e: &GraphEdge, directed: bool) -> bool {
    let raw = &e.raw;
    let end = |a: &str, b: &str| -> Option<String> {
        match (field(raw, a), field(raw, b)) {
            (Some(v), None) | (None, Some(v)) => id_of(v, a).ok(),
            _ => None,
        }
    };
    raw.as_obj().is_some()
        && end("from", "source").as_ref() == Some(&e.from)
        && end("to", "target").as_ref() == Some(&e.to)
        && edge_extras(raw, directed).ok() == Some((e.label.clone(), e.weight, e.directed))
        && fig_of(raw) == e.fig
}

fn fig_of(raw: &Node) -> FigStyle {
    field(raw, "fig")
        .map(FigStyle::from_node)
        .unwrap_or_default()
}

/// The object's keys that aren't among `known`.
fn others(raw: &Node, known: &[&str]) -> Vec<(String, J)> {
    raw.as_obj()
        .unwrap_or_default()
        .iter()
        .filter(|(k, _)| !known.contains(&&*k.name))
        .map(|(k, v)| (k.name.to_string(), node_j(v)))
        .collect()
}

fn node_of(n: &GraphNode) -> J {
    if node_fits(n) {
        return node_j(&n.raw);
    }
    let mut m: Vec<(String, J)> = vec![("id".into(), J::s(&n.id))];
    if let Some(l) = &n.label {
        m.push(("label".into(), J::s(l)));
    }
    if let Some(g) = &n.group {
        m.push(("group".into(), J::s(g)));
    }
    if let Some(s) = n.size {
        m.push(("size".into(), J::Num(s)));
    }
    if n.fig != FigStyle::default() {
        m.push(("fig".into(), style_j(&n.fig)));
    }
    // Every other key, `metadata` among them, as the object had it.
    m.extend(others(&n.raw, &["id", "label", "group", "size", "fig"]));
    J::Map(m)
}

fn edge_of(e: &GraphEdge, directed: bool) -> J {
    if edge_fits(e, directed) {
        return node_j(&e.raw);
    }
    let mut m: Vec<(String, J)> = vec![("from".into(), J::s(&e.from)), ("to".into(), J::s(&e.to))];
    if let Some(l) = &e.label {
        m.push(("label".into(), J::s(l)));
    }
    if let Some(w) = e.weight {
        m.push(("weight".into(), J::Num(w)));
    }
    if e.directed != directed {
        m.push(("directed".into(), J::Bool(e.directed)));
    }
    if e.fig != FigStyle::default() {
        m.push(("fig".into(), style_j(&e.fig)));
    }
    // Every other key but the ends' aliases, which `from` and `to` are now.
    m.extend(others(&e.raw, super::fig::EDGE));
    J::Map(m)
}

fn value_j(a: &Aggregate) -> J {
    agg_j(a.op, &a.field)
}

fn stat_j(s: &StatSpec, names: &[String]) -> J {
    let mut o = O::default();
    o.put("type", J::s("stat"));
    o.put("label", J::s(&s.label));
    o.put("data", data_j(&s.data, names));
    o.put("value", value_j(&s.value));
    match &s.delta {
        Some(Delta::Previous) => o.put("delta", J::s("previous")),
        Some(Delta::Value(v)) => o.put("delta", J::Num(*v)),
        None => {}
    }
    o.opt("trend", &s.trend);
    o.opt("format", &s.format);
    match s.good {
        GoodWhen::Up => {}
        GoodWhen::Down => o.put("good", J::s("down")),
        GoodWhen::Neither => o.put("good", J::s("neither")),
    }
    o.done()
}

fn meter_j(m: &MeterSpec, names: &[String]) -> J {
    let mut o = O::default();
    o.put("type", J::s("meter"));
    o.put("label", J::s(&m.label));
    o.put("data", data_j(&m.data, names));
    o.put("value", value_j(&m.value));
    o.put("max", J::Num(m.max));
    o.opt("format", &m.format);
    o.done()
}

fn dashboard_j(d: &DashboardSpec) -> J {
    let names: Vec<String> = d.data.iter().map(|(k, _)| k.clone()).collect();
    let mut o = O::default();
    o.put("type", J::s("dashboard"));
    o.opt("title", &d.title);
    if !d.data.is_empty() {
        o.put(
            "data",
            J::Map(
                d.data
                    .iter()
                    .map(|(k, v)| (k.clone(), data_j(v, &[])))
                    .collect(),
            ),
        );
    }
    if let Some(t) = &d.time {
        let mut to = O::default();
        to.put("field", J::s(&t.field));
        to.opt("range", &t.range);
        o.put("time", to.done());
    }
    let rows = d
        .rows
        .iter()
        .map(|r| J::Arr(r.iter().map(|p| panel_j(p, &names)).collect()))
        .collect();
    o.put("rows", J::Arr(rows));
    o.done()
}

fn panel_j(p: &PanelSpec, names: &[String]) -> J {
    let mut j = match &p.panel {
        Panel::Figure(f) => figure_j(f, names),
        Panel::File(path) => J::Obj(vec![("file", J::Str(path.to_string_lossy().into_owned()))]),
    };
    if p.span != 1 {
        let span = J::Num(f64::from(p.span));
        match &mut j {
            J::Obj(o) => o.push(("span", span)),
            J::Map(m) => m.push(("span".into(), span)),
            _ => {}
        }
    }
    j
}

/// A scalar as JSON, through the reader's own writer.
fn scalar(j: &J) -> Option<String> {
    let kind = match j {
        J::Null => Kind::Null,
        J::Bool(b) => Kind::Bool(*b),
        J::Num(v) => Kind::Num(*v),
        J::Str(s) => Kind::Str(s.as_str().into()),
        _ => return None,
    };
    Some(
        Node {
            kind,
            at: Pos::default(),
        }
        .to_text(),
    )
}

fn key_text(k: &str) -> String {
    scalar(&J::s(k)).unwrap_or_default()
}

fn entries(j: &J) -> Option<Vec<(String, &J)>> {
    match j {
        J::Obj(o) => Some(o.iter().map(|(k, v)| (key_text(k), v)).collect()),
        J::Map(m) => Some(m.iter().map(|(k, v)| (key_text(k), v)).collect()),
        _ => None,
    }
}

/// On one line: `{ "a": 1, "b": [1, 2] }`.
fn compact(j: &J) -> String {
    if let Some(s) = scalar(j) {
        return s;
    }
    if let J::Arr(a) = j {
        let inner: Vec<String> = a.iter().map(compact).collect();
        return format!("[{}]", inner.join(", "));
    }
    let es = entries(j).unwrap_or_default();
    if es.is_empty() {
        return "{}".into();
    }
    let inner: Vec<String> = es
        .iter()
        .map(|(k, v)| format!("{k}: {}", compact(v)))
        .collect();
    format!("{{ {} }}", inner.join(", "))
}

/// `j` at column `col`, its inner lines indented from `indent`: on one line if it fits; a list
/// of scalars as many to a line as fit, any other list or object a value a line.
fn pretty(j: &J, indent: usize, col: usize, out: &mut String) {
    let flat = compact(j);
    if scalar(j).is_some() || col + flat.chars().count() <= WIDTH {
        out.push_str(&flat);
        return;
    }
    let pad = " ".repeat(indent + 2);
    if let J::Arr(a) = j
        && let Some(items) = a.iter().map(scalar).collect::<Option<Vec<String>>>()
    {
        out.push_str("[\n");
        let mut line = String::new();
        for (i, mut item) in items.into_iter().enumerate() {
            if i + 1 < a.len() {
                item.push(',');
            }
            let wide = pad.len() + line.chars().count() + 1 + item.chars().count();
            if !line.is_empty() && wide > WIDTH {
                out.push_str(&pad);
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&item);
        }
        out.push_str(&pad);
        out.push_str(&line);
        out.push('\n');
    } else if let J::Arr(a) = j {
        out.push_str("[\n");
        for (i, v) in a.iter().enumerate() {
            out.push_str(&pad);
            pretty(v, indent + 2, indent + 2, out);
            out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
        }
    } else {
        out.push_str("{\n");
        let es = entries(j).unwrap_or_default();
        for (i, (k, v)) in es.iter().enumerate() {
            out.push_str(&pad);
            out.push_str(k);
            out.push_str(": ");
            pretty(v, indent + 2, indent + 4 + k.chars().count(), out);
            out.push_str(if i + 1 < es.len() { ",\n" } else { "\n" });
        }
    }
    out.push_str(&" ".repeat(indent));
    out.push(if matches!(j, J::Arr(_)) { ']' } else { '}' });
}
