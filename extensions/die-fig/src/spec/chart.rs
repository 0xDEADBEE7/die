//! The grammar: a chart's marks, encodings, layers and transforms read from a spec, top-level
//! encodings inherited by each layer, every error at its place. The keys are listed in the
//! [module's docs](super).

use super::{
    Channel, ChannelValue, Chart, Curve, Cx, DataFormat, DataRef, DataSpec, Encoding, Labels,
    Layer, LegendPos, Mark, MarkKind, ReadHints, ScaleKind, Stack, ViewSettings, arr, field,
    number, obj, opt_bool, opt_string, string, strings, whole, wrong,
};
use crate::data::transform::{AggOp, FilterOp, Transform};
use crate::data::{ColumnKind, ReadOptions, SeriesKey};
use crate::error::{FigError, Result};
use crate::json::{Kind, Node};
use crate::style::FigStyle;
use std::path::Path;
use std::time::Duration;

/// A chart's keys.
pub const TOP: &[&str] = &[
    "type",
    "title",
    "subtitle",
    "data",
    "transform",
    "x",
    "y",
    "y2",
    "color",
    "size",
    "label",
    "tooltip",
    "detail",
    "mark",
    "layers",
    "series",
    "view",
    "fig",
];
/// A layer's keys.
pub const LAYER: &[&str] = &[
    "mark",
    "data",
    "transform",
    "x",
    "y",
    "y2",
    "color",
    "size",
    "label",
    "tooltip",
    "detail",
    "fig",
];
/// A channel's keys.
pub const CHANNEL: &[&str] = &[
    "field", "value", "type", "unit", "title", "scale", "domain", "zero", "format",
];
/// A mark's keys.
pub const MARK: &[&str] = &["type", "curve", "points", "stack", "horizontal"];
/// A data object's keys.
pub const DATA: &[&str] = &[
    "file",
    "values",
    "run",
    "every",
    "stream",
    "format",
    "delimiter",
    "header",
    "thousands",
    "time_format",
];
/// The keys of `view`.
pub const VIEW: &[&str] = &[
    "grid",
    "axes",
    "legend",
    "labels",
    "render_options",
    "motion",
];
/// The keys of a `fig` block, and `menu` at a figure's top.
pub const FIG: &[&str] = &[
    "color",
    "size",
    "override-focus",
    "show-label",
    "hidden",
    "label",
    "dash",
    "menu",
];
/// The transforms, by the key that names each.
pub const TRANSFORMS: &[&str] = &[
    "fold",
    "filter",
    "bin",
    "aggregate",
    "cumulative",
    "window",
    "sort",
    "top",
];
/// The bins a histogram or a bin transform makes at most, unless it says.
pub const MAX_BINS: usize = 20;

/// What a chart's `"type"` says of its first mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shorthand {
    /// `chart`: the mark is the spec's.
    Chart,
    /// `line`, `area`, `bar`, `scatter` (points) or `heatmap` (rects).
    Mark(MarkKind),
    /// `histogram`: bars over bins of `x`.
    Histogram,
}

impl Shorthand {
    /// The shorthand a `"type"` names, if it's a chart's.
    pub fn from_name(name: &str) -> Option<Shorthand> {
        Some(match name {
            "chart" => Shorthand::Chart,
            "line" => Shorthand::Mark(MarkKind::Line),
            "area" => Shorthand::Mark(MarkKind::Area),
            "bar" => Shorthand::Mark(MarkKind::Bar),
            "scatter" => Shorthand::Mark(MarkKind::Point),
            "heatmap" => Shorthand::Mark(MarkKind::Rect),
            "histogram" => Shorthand::Histogram,
            _ => return None,
        })
    }

    fn mark(self) -> Option<MarkKind> {
        match self {
            Shorthand::Chart => None,
            Shorthand::Mark(k) => Some(k),
            Shorthand::Histogram => Some(MarkKind::Bar),
        }
    }
}

/// A chart from a spec's object.
pub(crate) fn parse(node: &Node, sh: Shorthand, cx: &mut Cx<'_>) -> Result<Chart> {
    let o = obj(node, "a chart")?;
    let mut known = TOP.to_vec();
    if sh == Shorthand::Histogram {
        known.extend(["step", "bins"]);
    }
    cx.check_top(o, &known);
    let title = opt_string(node, "title")?;
    let subtitle = opt_string(node, "subtitle")?;
    let data = data_of(node, cx)?;
    let transform = transforms(node.get("transform"), cx)?;
    let top = encoding(node, cx)?.over(&Encoding::default());
    let style = top_style(node, cx);
    let view = view_settings(node, ViewSettings::default(), cx)?;
    let mut layers = Vec::new();
    match (field(node, "mark"), sh.mark()) {
        (Some(m), short) => layers.push(Layer {
            mark: mark(m, short, cx)?,
            enc: top.clone(),
            data: None,
            transform: Vec::new(),
            fig: style,
        }),
        (None, Some(k)) => layers.push(Layer {
            mark: Mark::new(k),
            enc: top.clone(),
            data: None,
            transform: Vec::new(),
            fig: style,
        }),
        (None, None) => {
            if let Some(f) = node.get("fig")
                && style != FigStyle::default()
            {
                cx.warnings.push(FigError::at(
                    "a fig block at the top styles the top's mark, and there is none",
                    f.at,
                ));
            }
        }
    }
    if sh == Shorthand::Histogram {
        let step = field(node, "step")
            .map(|s| positive(s, "step"))
            .transpose()?;
        let bins = field(node, "bins")
            .map(|b| whole(b, "bins", 1))
            .transpose()?
            .map_or(MAX_BINS, |b| b as usize);
        let l = &mut layers[0];
        let Some(x) = l.enc.x.clone().filter(|x| x.field.is_some()) else {
            return Err(FigError::at(
                "a histogram needs \"x\": the field it counts",
                node.at,
            ));
        };
        let color = l.enc.color.as_ref().and_then(|c| c.field.clone());
        let (ts, x, y) = binned(x, l.enc.y.clone(), color.as_deref(), step, bins, &[]);
        l.transform = ts;
        l.enc.x = Some(x);
        l.enc.y = Some(y);
    }
    if let Some(ls) = field(node, "layers") {
        for l in arr(ls, "layers")? {
            layers.push(layer(l, &top, cx)?);
        }
    }
    if layers.is_empty() {
        return Err(FigError::at(
            "a chart needs a \"mark\" or \"layers\"",
            node.at,
        ));
    }
    let series_fig = series_styles(node.get("series"), cx)?;
    Ok(Chart {
        title,
        subtitle,
        data,
        transform,
        layers,
        series_fig,
        view,
    })
}

/// A histogram's transforms and channels: `x` binned (at `step`, or into at most `bins`), the
/// rows in each bin counted, or their `y` summed, by bin and by `color`'s field. The x channel
/// becomes the bins' starts on a linear scale, titled as `x` was; `y` the count or the sum. New
/// columns' names avoid `taken`.
pub(crate) fn binned(
    x: Channel,
    y: Option<Channel>,
    color: Option<&str>,
    step: Option<f64>,
    bins: usize,
    taken: &[&str],
) -> (Vec<Transform>, Channel, Channel) {
    let f = x.field.clone().unwrap_or_default();
    let start = unique(&format!("{f}_start"), taken);
    let end = unique(&format!("{f}_end"), taken);
    let summed = y.as_ref().and_then(|y| y.field.clone());
    let (op, out) = match &summed {
        Some(s) => (AggOp::Sum, s.clone()),
        None => (AggOp::Count, unique("count", taken)),
    };
    let mut by = vec![start.clone(), end.clone()];
    by.extend(color.map(String::from));
    let ts = vec![
        Transform::Bin {
            field: f.clone(),
            step,
            max_bins: bins,
            start: start.clone(),
            end,
        },
        Transform::Aggregate {
            op,
            field: summed,
            by,
            out: out.clone(),
        },
    ];
    let x = Channel {
        field: Some(start),
        title: x.title.clone().or(Some(f)),
        scale: Some(ScaleKind::Linear),
        ..x
    };
    let y = Channel {
        field: Some(out),
        value: None,
        ..y.unwrap_or_default()
    };
    (ts, x, y)
}

/// `base`, or `base_2`, `base_3` … whichever isn't `taken`.
pub(crate) fn unique(base: &str, taken: &[&str]) -> String {
    if !taken.contains(&base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}_{i}"))
        .find(|n| !taken.contains(&n.as_str()))
        .unwrap_or_default()
}

/// The top's `fig` block, which styles the top's mark; `menu` is the view's.
fn top_style(node: &Node, cx: &mut Cx<'_>) -> FigStyle {
    match field(node, "fig") {
        Some(f) => fig_block(f, cx),
        None => FigStyle::default(),
    }
}

/// A `fig` block, tolerant as fig's are, with a warning for a misspelt key.
pub(crate) fn fig_block(n: &Node, cx: &mut Cx<'_>) -> FigStyle {
    if let Some(o) = n.as_obj() {
        cx.check(o, FIG);
    }
    FigStyle::from_node(n)
}

/// A chart's or a tile's data: its `data`, or in a dashboard's panel the dashboard's only
/// data.
pub(crate) fn data_of(node: &Node, cx: &mut Cx<'_>) -> Result<DataSpec> {
    if let Some(d) = field(node, "data") {
        return data_spec(d, cx);
    }
    match cx.names.as_deref() {
        Some([one]) => Ok(DataSpec {
            source: DataRef::Named(one.clone()),
            format: None,
            read: ReadHints::default(),
        }),
        Some(names) if !names.is_empty() => Err(FigError::at(
            format!(
                "a panel needs \"data\": one of {}",
                names
                    .iter()
                    .map(|n| format!("\"{n}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            node.at,
        )),
        _ => Err(FigError::at("a figure needs \"data\"", node.at)),
    }
}

/// One layer under `layers`.
fn layer(node: &Node, top: &Encoding, cx: &mut Cx<'_>) -> Result<Layer> {
    let o = obj(node, "a layer")?;
    cx.check(o, LAYER);
    let Some(m) = field(node, "mark") else {
        return Err(FigError::at("a layer needs a \"mark\"", node.at));
    };
    let mark = mark(m, None, cx)?;
    let enc = encoding(node, cx)?.over(top);
    let data = field(node, "data").map(|d| data_spec(d, cx)).transpose()?;
    let transform = transforms(node.get("transform"), cx)?;
    let fig = field(node, "fig")
        .map(|f| fig_block(f, cx))
        .unwrap_or_default();
    Ok(Layer {
        mark,
        enc,
        data,
        transform,
        fig,
    })
}

/// A layer's encodings as written: each channel absent (None), dropped (`null`, Some(None)) or
/// given.
#[derive(Default)]
struct Draft {
    channels: [Option<Option<Channel>>; 6],
    tooltip: Option<Vec<String>>,
    detail: Option<Vec<String>>,
}

/// The channels, in [`Draft::channels`]' order.
const CHANNELS: [&str; 6] = ["x", "y", "y2", "color", "size", "label"];

impl Draft {
    /// The encodings, with `top`'s for each channel not given.
    fn over(self, top: &Encoding) -> Encoding {
        let [x, y, y2, color, size, label] = self.channels;
        let pick = |c: Option<Option<Channel>>, t: &Option<Channel>| match c {
            None => t.clone(),
            Some(c) => c,
        };
        Encoding {
            x: pick(x, &top.x),
            y: pick(y, &top.y),
            y2: pick(y2, &top.y2),
            color: pick(color, &top.color),
            size: pick(size, &top.size),
            label: pick(label, &top.label),
            tooltip: self.tooltip.unwrap_or_else(|| top.tooltip.clone()),
            detail: self.detail.unwrap_or_else(|| top.detail.clone()),
        }
    }
}

fn encoding(node: &Node, cx: &mut Cx<'_>) -> Result<Draft> {
    let mut d = Draft::default();
    for (slot, key) in d.channels.iter_mut().zip(CHANNELS) {
        *slot = match node.get(key) {
            None => None,
            Some(v) if v.is_null() => Some(None),
            Some(v) => Some(Some(channel(v, key, cx)?)),
        };
    }
    let list = |key: &str| -> Result<Option<Vec<String>>> {
        match node.get(key) {
            None => Ok(None),
            Some(v) if v.is_null() => Ok(Some(Vec::new())),
            Some(v) => strings(v, key).map(Some),
        }
    };
    d.tooltip = list("tooltip")?;
    d.detail = list("detail")?;
    Ok(d)
}

/// One channel: a field's name, a constant number, or an object.
pub(crate) fn channel(n: &Node, key: &str, cx: &mut Cx<'_>) -> Result<Channel> {
    let mut c = Channel {
        at: Some(n.at),
        ..Channel::default()
    };
    match &n.kind {
        Kind::Str(s) => c.field = Some(s.to_string()),
        Kind::Num(v) => c.value = Some(ChannelValue::Num(*v)),
        Kind::Obj(o) => {
            cx.check(o, CHANNEL);
            c.field = opt_string(n, "field")?;
            c.value = field(n, "value")
                .map(|v| match &v.kind {
                    Kind::Num(x) => Ok(ChannelValue::Num(*x)),
                    Kind::Str(s) => Ok(ChannelValue::Str(s.to_string())),
                    _ => Err(wrong(v, "value", "a number or a string")),
                })
                .transpose()?;
            match (&c.field, &c.value) {
                (Some(_), Some(_)) => {
                    return Err(FigError::at(
                        format!("\"{key}\" has a \"field\" and a \"value\"; give one"),
                        n.at,
                    ));
                }
                (None, None) => {
                    return Err(FigError::at(
                        format!("\"{key}\" needs a \"field\" or a \"value\""),
                        n.at,
                    ));
                }
                _ => {}
            }
            c.kind = field(n, "type").map(column_kind).transpose()?;
            c.unit = match field(n, "unit") {
                None => None,
                Some(u) => match u.as_str() {
                    Some(s @ ("s" | "ms")) => Some(s.to_string()),
                    _ => return Err(FigError::at("\"unit\" must be \"s\" or \"ms\"", u.at)),
                },
            };
            c.title = opt_string(n, "title")?;
            c.scale = field(n, "scale").map(scale_kind).transpose()?;
            c.domain = field(n, "domain").map(|d| domain(d, cx.opt)).transpose()?;
            c.zero = opt_bool(n, "zero")?;
            c.format = opt_string(n, "format")?;
        }
        _ => {
            return Err(wrong(n, key, "a field's name, a number or an object"));
        }
    }
    Ok(c)
}

/// A column kind's name in a spec.
pub fn kind_name(k: ColumnKind) -> &'static str {
    match k {
        ColumnKind::Number => "number",
        ColumnKind::Time => "time",
        ColumnKind::Category => "category",
        ColumnKind::Text => "text",
    }
}

fn column_kind(n: &Node) -> Result<ColumnKind> {
    let kinds = [
        ColumnKind::Number,
        ColumnKind::Time,
        ColumnKind::Category,
        ColumnKind::Text,
    ];
    n.as_str()
        .and_then(|s| kinds.into_iter().find(|k| kind_name(*k) == s))
        .ok_or_else(|| {
            FigError::at(
                "\"type\" must be \"number\", \"time\", \"category\" or \"text\"",
                n.at,
            )
        })
}

/// A scale kind's name in a spec.
pub fn scale_name(s: ScaleKind) -> &'static str {
    match s {
        ScaleKind::Auto => "auto",
        ScaleKind::Linear => "linear",
        ScaleKind::Log => "log",
        ScaleKind::Time => "time",
        ScaleKind::Band => "band",
    }
}

fn scale_kind(n: &Node) -> Result<ScaleKind> {
    let all = [
        ScaleKind::Auto,
        ScaleKind::Linear,
        ScaleKind::Log,
        ScaleKind::Time,
        ScaleKind::Band,
    ];
    n.as_str()
        .and_then(|s| all.into_iter().find(|k| scale_name(*k) == s))
        .ok_or_else(|| {
            FigError::at(
                "\"scale\" must be \"auto\", \"linear\", \"log\", \"time\" or \"band\"",
                n.at,
            )
        })
}

/// `[lo, hi]`: two numbers, or two times read in the data's zone; not equal.
fn domain(n: &Node, opt: &ReadOptions) -> Result<(f64, f64)> {
    let bad = || FigError::at("\"domain\" must be two numbers or two times", n.at);
    let ends = n.as_arr().filter(|a| a.len() == 2).ok_or_else(bad)?;
    let end = |e: &Node| match &e.kind {
        Kind::Num(v) => Some(*v),
        Kind::Str(s) => crate::data::time::parse_time(s, opt),
        _ => None,
    };
    let (Some(lo), Some(hi)) = (end(&ends[0]), end(&ends[1])) else {
        return Err(bad());
    };
    if lo == hi {
        return Err(FigError::at("\"domain\" needs two different ends", n.at));
    }
    Ok((lo, hi))
}

/// A mark: a kind's name, or an object; `short` is the kind a shorthand type gave.
fn mark(n: &Node, short: Option<MarkKind>, cx: &mut Cx<'_>) -> Result<Mark> {
    let named = |t: &Node| -> Result<MarkKind> {
        let s = string(t, "mark")?;
        MarkKind::ALL
            .into_iter()
            .find(|k| k.name() == s)
            .ok_or_else(|| {
                FigError::at(
                    format!(
                        "unknown mark \"{s}\": expected line, area, bar, point, rect, rule, band or text"
                    ),
                    t.at,
                )
            })
    };
    let (kind, at) = match &n.kind {
        Kind::Str(_) => (named(n)?, n.at),
        Kind::Obj(o) => {
            cx.check(o, MARK);
            match (field(n, "type"), short) {
                (Some(t), _) => (named(t)?, t.at),
                (None, Some(k)) => (k, n.at),
                (None, None) => return Err(FigError::at("a mark needs a \"type\"", n.at)),
            }
        }
        _ => return Err(wrong(n, "mark", "a mark's name or an object")),
    };
    if let Some(k) = short
        && k != kind
    {
        return Err(FigError::at(
            format!(
                "the mark is \"{}\" but the chart's type draws \"{}\"",
                kind.name(),
                k.name()
            ),
            at,
        ));
    }
    let mut m = Mark::new(kind);
    if n.as_obj().is_some() {
        if let Some(c) = field(n, "curve") {
            m.curve = match c.as_str() {
                Some("linear") => Curve::Linear,
                Some("step") => Curve::Step,
                _ => {
                    return Err(FigError::at(
                        "\"curve\" must be \"linear\" or \"step\"",
                        c.at,
                    ));
                }
            };
        }
        if let Some(p) = opt_bool(n, "points")? {
            m.points = p;
        }
        if let Some(s) = field(n, "stack") {
            m.stack = match &s.kind {
                Kind::Bool(true) => Stack::Zero,
                Kind::Bool(false) => Stack::None,
                Kind::Str(v) if &**v == "none" => Stack::None,
                Kind::Str(v) if &**v == "zero" => Stack::Zero,
                Kind::Str(v) if &**v == "normalize" => Stack::Normalize,
                _ => {
                    return Err(FigError::at(
                        "\"stack\" must be \"none\", \"zero\", \"normalize\", true or false",
                        s.at,
                    ));
                }
            };
        }
        if let Some(h) = opt_bool(n, "horizontal")? {
            m.horizontal = h;
        }
    }
    Ok(m)
}

/// A data spec: a file's name, or an object of one source and how to read it.
pub(crate) fn data_spec(n: &Node, cx: &mut Cx<'_>) -> Result<DataSpec> {
    let mut spec = DataSpec {
        source: DataRef::Source,
        format: None,
        read: ReadHints::default(),
    };
    match &n.kind {
        Kind::Str(s) => {
            spec.source = path_ref(s, n, cx, true)?;
            Ok(spec)
        }
        Kind::Obj(o) => {
            cx.check(o, DATA);
            let sources: Vec<&str> = ["file", "values", "run", "stream"]
                .into_iter()
                .filter(|k| field(n, k).is_some())
                .collect();
            match sources.as_slice() {
                [] => {
                    return Err(FigError::at(
                        "\"data\" needs one of \"file\", \"values\", \"run\" or \"stream\"",
                        n.at,
                    ));
                }
                [_] => {}
                [a, b, ..] => {
                    let at = o
                        .iter()
                        .find(|(k, _)| &*k.name == *b)
                        .map_or(n.at, |(k, _)| k.at);
                    return Err(FigError::at(
                        format!("\"data\" has \"{a}\" and \"{b}\"; give one"),
                        at,
                    ));
                }
            }
            spec.format = field(n, "format").map(data_format).transpose()?;
            spec.read = hints(n)?;
            if let Some((k, _)) = o.iter().find(|(k, v)| &*k.name == "every" && !v.is_null())
                && sources[0] != "run"
            {
                return Err(FigError::at("\"every\" goes with \"run\"", k.at));
            }
            let src = field(n, sources[0]).unwrap_or(n);
            spec.source = match sources[0] {
                "file" => path_ref(&string(src, "file")?, src, cx, false)?,
                "values" => DataRef::Inline(std::sync::Arc::new(values(src, &spec, cx.opt)?)),
                "run" => DataRef::Run {
                    cmd: command(src, "run")?,
                    every: field(n, "every").map(every).transpose()?,
                },
                _ => DataRef::Stream {
                    cmd: command(src, "stream")?,
                },
            };
            Ok(spec)
        }
        _ => Err(wrong(n, "data", "a file's name or an object")),
    }
}

/// A file's name as a source: `-` the buffer itself, a dashboard's data name if `named` may be
/// one, else the file relative to the spec's directory.
fn path_ref(s: &str, n: &Node, cx: &Cx<'_>, named: bool) -> Result<DataRef> {
    if s.is_empty() {
        return Err(FigError::at("\"data\" must name a file", n.at));
    }
    if s == "-" {
        return Ok(DataRef::Source);
    }
    if named
        && cx
            .names
            .as_ref()
            .is_some_and(|ns| ns.iter().any(|k| k == s))
    {
        return Ok(DataRef::Named(s.to_string()));
    }
    Ok(DataRef::File(resolve(cx.dir, s)))
}

/// A path relative to the spec's directory, or absolute.
pub(crate) fn resolve(dir: &Path, s: &str) -> std::path::PathBuf {
    dir.join(s)
}

fn command(n: &Node, key: &str) -> Result<String> {
    let c = string(n, key)?;
    if c.trim().is_empty() {
        return Err(FigError::at(format!("\"{key}\" needs a command"), n.at));
    }
    Ok(c)
}

/// A data format's name in a spec.
pub fn format_name(f: DataFormat) -> &'static str {
    match f {
        DataFormat::Csv => "csv",
        DataFormat::Tsv => "tsv",
        DataFormat::Json => "json",
        DataFormat::Jsonl => "jsonl",
    }
}

fn data_format(n: &Node) -> Result<DataFormat> {
    let all = [
        DataFormat::Csv,
        DataFormat::Tsv,
        DataFormat::Json,
        DataFormat::Jsonl,
    ];
    n.as_str()
        .and_then(|s| all.into_iter().find(|f| format_name(*f) == s))
        .ok_or_else(|| {
            FigError::at(
                "\"format\" must be \"csv\", \"tsv\", \"json\" or \"jsonl\"",
                n.at,
            )
        })
}

fn hints(n: &Node) -> Result<ReadHints> {
    let one = |key: &str| -> Result<Option<char>> {
        let Some(v) = field(n, key) else {
            return Ok(None);
        };
        let s = string(v, key)?;
        let mut cs = s.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) => Ok(Some(c)),
            _ => Err(FigError::at(
                format!("\"{key}\" must be one character"),
                v.at,
            )),
        }
    };
    let delimiter = match one("delimiter")? {
        None => None,
        Some(c) if c.is_ascii() => Some(c as u8),
        Some(_) => {
            let at = field(n, "delimiter").map_or(n.at, |v| v.at);
            return Err(FigError::at("\"delimiter\" must be an ASCII character", at));
        }
    };
    Ok(ReadHints {
        delimiter,
        header: opt_bool(n, "header")?,
        thousands: one("thousands")?,
        time_format: opt_string(n, "time_format")?,
    })
}

/// The read options with a spec's hints over them.
pub fn with_hints(opt: &ReadOptions, h: &ReadHints) -> ReadOptions {
    let mut o = opt.clone();
    if h.delimiter.is_some() {
        o.delimiter = h.delimiter;
    }
    if let Some(b) = h.header {
        o.header = b;
    }
    if h.thousands.is_some() {
        o.thousands = h.thousands;
    }
    if h.time_format.is_some() {
        o.time_format.clone_from(&h.time_format);
    }
    o
}

/// `values`: JSON rows, or CSV, TSV or JSONL text.
fn values(n: &Node, spec: &DataSpec, opt: &ReadOptions) -> Result<crate::data::Table> {
    let o = with_hints(opt, &spec.read);
    match (&n.kind, spec.format) {
        (Kind::Str(s), Some(DataFormat::Jsonl)) => crate::data::rows::read_jsonl(s, &o),
        (Kind::Str(s), f) => {
            let mut o = o;
            if f == Some(DataFormat::Tsv) && o.delimiter.is_none() {
                o.delimiter = Some(b'\t');
            }
            crate::data::csv::read_csv(s, &o)
        }
        (Kind::Arr(_) | Kind::Obj(_), _) => crate::data::rows::read_json(n, &o),
        _ => Err(wrong(n, "values", "rows, or CSV text")),
    }
}

/// `every`: a duration more than nothing, as a string or a number of seconds.
fn every(n: &Node) -> Result<Duration> {
    let d = match &n.kind {
        Kind::Num(v) if *v > 0.0 && v.is_finite() => {
            Some(Duration::from_millis((v * 1000.0).round() as u64))
        }
        Kind::Str(s) => parse_duration(s),
        _ => None,
    };
    match d {
        Some(d) if !d.is_zero() => Ok(d),
        _ => Err(FigError::at(
            "\"every\" must be a time like \"30s\", \"5m\" or \"1h\"",
            n.at,
        )),
    }
}

/// A duration like `30s`, `1h30m`, `500ms` or `1.5d`, to the millisecond: numbers each with a
/// unit of `ms`, `s`, `m`, `h` or `d`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let mut rest = s.trim();
    if rest.is_empty() {
        return None;
    }
    let mut ms = 0f64;
    while !rest.is_empty() {
        let n = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let v: f64 = rest[..n].parse().ok()?;
        rest = &rest[n..];
        let u = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let unit = match &rest[..u] {
            "ms" => 1.0,
            "s" => 1e3,
            "m" => 6e4,
            "h" => 3.6e6,
            "d" => 8.64e7,
            _ => return None,
        };
        ms += v * unit;
        rest = &rest[u..];
    }
    (ms.is_finite() && ms < 1e15).then(|| Duration::from_millis(ms.round() as u64))
}

/// A duration as [`parse_duration`] reads it: in the largest unit it's a whole number of.
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    for (unit, size) in [
        ("d", 86_400_000),
        ("h", 3_600_000),
        ("m", 60_000),
        ("s", 1000),
    ] {
        if ms > 0 && ms.is_multiple_of(size) {
            return format!("{}{unit}", ms / size);
        }
    }
    format!("{ms}ms")
}

/// `view`, over fig's `fig.menu`, over `base`.
pub(crate) fn view_settings(
    node: &Node,
    base: ViewSettings,
    cx: &mut Cx<'_>,
) -> Result<ViewSettings> {
    let mut v = base;
    if let Some(menu) = field(node, "fig").and_then(|f| field(f, "menu")) {
        let o = obj(menu, "\"menu\"")?;
        cx.check(o, &["plane", "render_options"]);
        if let Some(plane) = field(menu, "plane") {
            let p = obj(plane, "\"plane\"")?;
            cx.check(p, &["axis_lines", "grid_lines"]);
            if let Some(b) = opt_bool(plane, "axis_lines")? {
                v.axes = b;
            }
            if let Some(b) = opt_bool(plane, "grid_lines")? {
                v.grid = b;
            }
        }
        if let Some(b) = opt_bool(menu, "render_options")? {
            v.render_options = b;
        }
    }
    let Some(view) = field(node, "view") else {
        return Ok(v);
    };
    let o = obj(view, "\"view\"")?;
    cx.check(o, VIEW);
    if let Some(b) = opt_bool(view, "grid")? {
        v.grid = b;
    }
    if let Some(b) = opt_bool(view, "axes")? {
        v.axes = b;
    }
    if let Some(l) = field(view, "legend") {
        v.legend = match l.as_str() {
            Some("top") => LegendPos::Top,
            Some("right") => LegendPos::Right,
            Some("none") => LegendPos::None,
            _ => {
                return Err(FigError::at(
                    "\"legend\" must be \"top\", \"right\" or \"none\"",
                    l.at,
                ));
            }
        };
    }
    if let Some(l) = field(view, "labels") {
        v.labels = match l.as_str() {
            Some("auto") => Labels::Auto,
            Some("ends") => Labels::Ends,
            Some("none") => Labels::None,
            _ => {
                return Err(FigError::at(
                    "\"labels\" must be \"auto\", \"ends\" or \"none\"",
                    l.at,
                ));
            }
        };
    }
    if let Some(b) = opt_bool(view, "render_options")? {
        v.render_options = b;
    }
    if let Some(b) = opt_bool(view, "motion")? {
        v.motion = b;
    }
    Ok(v)
}

/// `series`: each series' `fig` block by its name, a list for several of one name.
fn series_styles(n: Option<&Node>, cx: &mut Cx<'_>) -> Result<Vec<(SeriesKey, FigStyle)>> {
    let Some(n) = n.filter(|n| !n.is_null()) else {
        return Ok(Vec::new());
    };
    if n.as_arr().is_some() {
        return Err(FigError::at(
            "fig's line \"series\" need \"type\": \"line\"; a chart's are an object of fig blocks by name",
            n.at,
        ));
    }
    let o = obj(n, "\"series\"")?;
    let mut out = Vec::new();
    for (k, v) in o {
        let blocks: Vec<&Node> = match &v.kind {
            Kind::Obj(_) => vec![v],
            Kind::Arr(a) => a.iter().collect(),
            _ => return Err(wrong(v, &k.name, "a fig block or a list of them")),
        };
        for (nth, b) in blocks.into_iter().enumerate() {
            if b.is_null() {
                continue;
            }
            if b.as_obj().is_none() {
                return Err(wrong(b, &k.name, "a fig block"));
            }
            let st = fig_block(b, cx);
            if st != FigStyle::default() {
                out.push((SeriesKey::new(&k.name, nth as u32), st));
            }
        }
    }
    Ok(out)
}

/// `transform`: a list of transforms, or nothing.
pub(crate) fn transforms(n: Option<&Node>, cx: &mut Cx<'_>) -> Result<Vec<Transform>> {
    let Some(n) = n.filter(|n| !n.is_null()) else {
        return Ok(Vec::new());
    };
    arr(n, "transform")?
        .iter()
        .map(|t| transform(t, cx))
        .collect()
}

fn transform(n: &Node, cx: &mut Cx<'_>) -> Result<Transform> {
    let o = obj(n, "a transform")?;
    let ops: Vec<&crate::json::Key> = o
        .iter()
        .map(|(k, _)| k)
        .filter(|k| TRANSFORMS.contains(&&*k.name))
        .collect();
    let op = match ops.as_slice() {
        [] => {
            return Err(FigError::at(
                "a transform needs one of \"fold\", \"filter\", \"bin\", \"aggregate\", \"cumulative\", \"window\", \"sort\" or \"top\"",
                n.at,
            ));
        }
        [one] => &*one.name,
        [first, second, ..] => {
            return Err(FigError::at(
                format!(
                    "a transform does one thing: \"{}\" and \"{}\" are both here",
                    first.name, second.name
                ),
                second.at,
            ));
        }
    };
    let v = n.get(op).unwrap_or(n);
    let by = |key: &str| -> Result<Vec<String>> {
        field(n, key).map_or(Ok(Vec::new()), |b| strings(b, key))
    };
    let pair = |key: &str| -> Result<Option<(String, String)>> {
        let Some(p) = field(n, key) else {
            return Ok(None);
        };
        match p.as_arr() {
            Some([a, b]) => Ok(Some((string(a, key)?, string(b, key)?))),
            _ => Err(FigError::at(format!("\"{key}\" must be two names"), p.at)),
        }
    };
    let out = |default: String| -> Result<String> { Ok(opt_string(n, "as")?.unwrap_or(default)) };
    Ok(match op {
        "fold" => {
            cx.check(o, &["fold", "as"]);
            let fields = strings(v, "fold")?;
            if fields.is_empty() {
                return Err(FigError::at("\"fold\" needs a field to fold", v.at));
            }
            let (key, value) = pair("as")?.unwrap_or(("key".into(), "value".into()));
            Transform::Fold { fields, key, value }
        }
        "filter" => {
            cx.check(o, &["filter", "in", "not_in", "range", "min", "max"]);
            let field_name = string(v, "filter")?;
            let list = |key: &str| -> Result<Option<Vec<String>>> {
                let Some(l) = field(n, key) else {
                    return Ok(None);
                };
                arr(l, key)?
                    .iter()
                    .map(|e| match &e.kind {
                        Kind::Str(s) => Ok(s.to_string()),
                        Kind::Num(_) => Ok(e.to_text()),
                        _ => Err(wrong(e, key, "a string or a number")),
                    })
                    .collect::<Result<Vec<_>>>()
                    .map(Some)
            };
            let end = |e: &Node, key: &str| -> Result<Option<f64>> {
                match &e.kind {
                    Kind::Null => Ok(None),
                    _ => number(e, key).map(Some),
                }
            };
            let op = if let Some(l) = list("in")? {
                FilterOp::In(l)
            } else if let Some(l) = list("not_in")? {
                FilterOp::NotIn(l)
            } else if let Some(r) = field(n, "range") {
                match r.as_arr() {
                    Some([lo, hi]) => FilterOp::Range(end(lo, "range")?, end(hi, "range")?),
                    _ => {
                        return Err(FigError::at(
                            "\"range\" must be [low, high], null for no end",
                            r.at,
                        ));
                    }
                }
            } else if field(n, "min").is_some() || field(n, "max").is_some() {
                FilterOp::Range(
                    field(n, "min").map(|m| number(m, "min")).transpose()?,
                    field(n, "max").map(|m| number(m, "max")).transpose()?,
                )
            } else {
                return Err(FigError::at(
                    "a filter needs \"in\", \"not_in\", \"range\", \"min\" or \"max\"",
                    n.at,
                ));
            };
            Transform::Filter {
                field: field_name,
                op,
            }
        }
        "bin" => {
            cx.check(o, &["bin", "step", "max_bins", "as"]);
            let f = string(v, "bin")?;
            let step = field(n, "step").map(|s| positive(s, "step")).transpose()?;
            let max_bins = field(n, "max_bins")
                .map(|m| whole(m, "max_bins", 1))
                .transpose()?
                .map_or(MAX_BINS, |m| m as usize);
            let (start, end) =
                pair("as")?.unwrap_or_else(|| (format!("{f}_start"), format!("{f}_end")));
            Transform::Bin {
                field: f,
                step,
                max_bins,
                start,
                end,
            }
        }
        "aggregate" => {
            cx.check(o, &["aggregate", "by", "as"]);
            let (op, f) = aggregate(v, "aggregate")?;
            let default = f.clone().unwrap_or_else(|| "count".into());
            Transform::Aggregate {
                op,
                field: f,
                by: by("by")?,
                out: out(default)?,
            }
        }
        "cumulative" => {
            cx.check(o, &["cumulative", "by", "as"]);
            let f = string(v, "cumulative")?;
            Transform::Cumulative {
                out: out(format!("{f}_cumulative"))?,
                field: f,
                by: by("by")?,
            }
        }
        "window" => {
            cx.check(o, &["window", "size", "by", "as"]);
            let f = string(v, "window")?;
            let Some(size) = field(n, "size") else {
                return Err(FigError::at("a window needs a \"size\"", n.at));
            };
            Transform::Window {
                out: out(format!("{f}_mean"))?,
                size: whole(size, "size", 1)? as usize,
                field: f,
                by: by("by")?,
            }
        }
        "sort" => {
            cx.check(o, &["sort", "descending"]);
            Transform::Sort {
                field: string(v, "sort")?,
                descending: opt_bool(n, "descending")?.unwrap_or(false),
            }
        }
        _ => {
            cx.check(o, &["top", "by", "field"]);
            let need = |key: &str| -> Result<String> {
                match field(n, key) {
                    Some(f) => string(f, key),
                    None => Err(FigError::at(format!("\"top\" needs \"{key}\""), n.at)),
                }
            };
            Transform::Top {
                n: whole(v, "top", 1)? as usize,
                by: need("by")?,
                field: need("field")?,
            }
        }
    })
}

/// An aggregate op's name in a spec.
pub fn agg_name(op: AggOp) -> &'static str {
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

/// An aggregate: `"count"`, or one op over a field, `{"sum": "rpm"}` (`{"count": "rpm"}`
/// counts the rows that have one).
pub(crate) fn aggregate(n: &Node, key: &str) -> Result<(AggOp, Option<String>)> {
    let ops = [
        AggOp::Count,
        AggOp::Sum,
        AggOp::Mean,
        AggOp::Min,
        AggOp::Max,
        AggOp::P50,
        AggOp::P95,
    ];
    let bad = || {
        FigError::at(
            format!(
                "\"{key}\" must be \"count\" or one of count, sum, mean, min, max, p50 and p95 over a field, as {{\"sum\": \"rpm\"}}"
            ),
            n.at,
        )
    };
    match &n.kind {
        Kind::Str(s) if &**s == "count" => Ok((AggOp::Count, None)),
        Kind::Obj(o) if o.len() == 1 => {
            let (k, v) = &o[0];
            let op = ops
                .into_iter()
                .find(|op| agg_name(*op) == &*k.name)
                .ok_or_else(bad)?;
            match (&v.kind, op) {
                (Kind::Null, AggOp::Count) => Ok((op, None)),
                (Kind::Str(f), _) => Ok((op, Some(f.to_string()))),
                _ => Err(wrong(v, &k.name, "a field's name")),
            }
        }
        _ => Err(bad()),
    }
}

/// A number above zero.
pub(crate) fn positive(n: &Node, key: &str) -> Result<f64> {
    match n.as_f64() {
        Some(v) if v > 0.0 && v.is_finite() => Ok(v),
        _ => Err(FigError::at(
            format!("\"{key}\" must be a number above 0"),
            n.at,
        )),
    }
}
