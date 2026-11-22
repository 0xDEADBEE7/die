//! The tiles that show one number: stat, meter and sparkline (phase 4).
//!
//! - [`TileModel::build`]: a stat, meter or sparkline resolved against its table over a time
//!   range (the dashboard's visible time, else every row): the value, its change against the
//!   period before or a fixed number, the trend's points, a meter's status;
//! - [`base`] and [`overlay`]: the tile drawn in a rectangle, as the design's m4 has its tiles,
//!   and what the pointer shows over it (a card, and a mini crosshair on the trend);
//! - [`hit`]: what the pointer is over; [`detail`]: the tile's tree for the detail panel.
//!
//! # The numbers
//!
//! A stat's `value` aggregates its field over the rows whose time is inside the range: count,
//! sum, mean, min, max, p50, p95, or `last`, the newest row's by the trend's field. The time is
//! the `trend` field's, else the table's first time column; with neither, the range is not
//! applied. `delta: "previous"` is the same aggregate over the equal-length range just before
//! (with no range given, the data's own span is the range, so the period before has no rows and
//! the tile says so); `delta: 12.5` compares with that number. The change is written
//! relatively (`4.2 %`) for a value with no unit, in points for a unit of `%` (`0.08 pt`), and
//! in the unit for another (`12 ms`), and it is good, bad or neither by `good`.
//!
//! The value is abbreviated as the dataviz rules ask: three significant figures and compact from
//! 10,000 (`12.9K`, `1.28M`), whole numbers from 100 (`3,412`, `182`), one decimal from 10
//! (`12.3`), two from 0.1 (`0.41`, `4.27`), else two significant figures (`0.0041`), trailing
//! zeros dropped; a spec's `format` overrides. The card and the detail panel give it
//! in full.
//!
//! A stat's trend is twelve points: the aggregate over twelve equal parts of the range, or one
//! point a distinct time when the rows have no more than twelve. A sparkline's are its rows,
//! those sharing an `x` summed. A meter fills to `value / max`, its fill in the status of the
//! highest threshold passed.

pub mod draw;

pub use draw::{TileHover, base, hit, overlay, overlay_in};

use crate::data::transform::{AggOp, quantile};
use crate::data::{Column, Table, Values};
use crate::detail::{DetailRow, DetailTree};
use crate::error::{ErrorKind, FigError, Result};
use crate::scale::{
    NumberStyle, format_fixed, format_number, format_time_full, format_time_point, format_value,
    format_with,
};
use crate::scene::{Measure, TextSpec, Weight};
use crate::spec::{Delta, Figure, GoodWhen, MeterSpec, SparklineSpec, StatSpec, Threshold};
use crate::style::Status;
use jiff::tz::TimeZone;
use std::sync::OnceLock;

/// A stat tile's natural height: a tile row's in the design's m4.
pub const STAT_HEIGHT: f32 = 100.0;
/// A meter's natural height.
pub const METER_HEIGHT: f32 = 64.0;
/// A sparkline's natural height, as a stat's.
pub const SPARKLINE_HEIGHT: f32 = 100.0;
/// The most points a stat's trend has.
pub const TREND_POINTS: usize = 12;

/// Which tile a model is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileKind {
    /// One number, its change and its trend.
    Stat,
    /// A value against a maximum.
    Meter,
    /// One series' trend, small, with its newest value.
    Sparkline,
}

/// One point of a trend: where it is along the trend's field, the span of rows it sums up (a
/// twelfth of the range, or its own place), and its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrendPoint {
    /// Where it's drawn along the trend's field: its part's middle, or its own place.
    pub x: f64,
    /// The rows it's made of lie in this span along the field (both ends its place when it's
    /// one place).
    pub span: (f64, f64),
    /// Its value.
    pub y: f64,
}

/// A stat's change: what it's against, by how much, and whether that's good.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// The value it's against: the previous period's, or the spec's number. NaN when the
    /// previous period has no rows.
    pub against: f64,
    /// What it's against, as the delta line says it: "previous 6h", "12.5".
    pub period: String,
    /// The value less what it's against.
    pub diff: f64,
    /// The change in percent of what it's against, when that isn't zero.
    pub rel: Option<f64>,
    /// Good (true), bad (false), or neither (None: `good: "neither"`, or no change).
    pub good: Option<bool>,
}

impl Change {
    /// Whether there's nothing to compare with: the previous period had no rows.
    pub fn missing(&self) -> bool {
        self.against.is_nan()
    }

    /// Which way it went: 1 up, -1 down, 0 no change (or nothing to compare).
    pub fn direction(&self) -> i8 {
        if self.diff.is_nan() || self.diff == 0.0 {
            0
        } else if self.diff > 0.0 {
            1
        } else {
            -1
        }
    }
}

/// A stat, meter or sparkline resolved against its table over a range: what [`base`] draws.
#[derive(Clone, Debug, PartialEq)]
pub struct TileModel {
    /// Which tile.
    pub kind: TileKind,
    /// What it's of.
    pub label: String,
    /// The value: the aggregate, a sparkline's newest. NaN with no rows.
    pub value: f64,
    /// What it's counted in.
    pub unit: Option<String>,
    /// The spec's format for the value.
    pub format: Option<String>,
    /// How the value is made.
    pub op: AggOp,
    /// The field it's made from; None for a count of rows.
    pub field: Option<String>,
    /// How many rows it's over.
    pub rows: usize,
    /// The span of time (or of the trend's numbers) the value is over: the range given, else
    /// the data's own; None with no time to say.
    pub range: Option<(f64, f64)>,
    /// Whether `range` was given (the dashboard's visible time) rather than the data's own.
    pub given: bool,
    /// Whether the trend's field (the range's) is a time.
    pub time: bool,
    /// The field the trend runs along, or the time column the range was applied to.
    pub along: Option<String>,
    /// The change shown under the value: a stat's with a `delta`.
    pub change: Option<Change>,
    /// The trend's points, in order along their field.
    pub trend: Vec<TrendPoint>,
    /// Where the trend's ends are drawn along its field: its first and last possible points.
    pub trend_domain: (f64, f64),
    /// A meter's maximum (NaN for the others).
    pub max: f64,
    /// A meter's thresholds, lowest first.
    pub thresholds: Vec<Threshold>,
    /// The zone its times are written in.
    pub zone: TimeZone,
}

/// The system's zone, looked up once: the zone [`TileModel::build`] writes times in (ruling 2's
/// fallback; [`TileModel::build_in`] takes one).
fn system_zone() -> TimeZone {
    static ZONE: OnceLock<TimeZone> = OnceLock::new();
    ZONE.get_or_init(TimeZone::system).clone()
}

impl TileModel {
    /// A stat, meter or sparkline over `table`, its value over the rows inside `range` (the
    /// dashboard's visible time) when given and every row otherwise; times in the system's
    /// zone. A figure that isn't a tile, a field the table hasn't, or one of text where numbers
    /// should be, is a Mismatch error.
    pub fn build(fig: &Figure, table: &Table, range: Option<(f64, f64)>) -> Result<TileModel> {
        TileModel::build_in(fig, table, range, &system_zone())
    }

    /// [`TileModel::build`] with its times in `zone`.
    pub fn build_in(
        fig: &Figure,
        table: &Table,
        range: Option<(f64, f64)>,
        zone: &TimeZone,
    ) -> Result<TileModel> {
        let range = range.filter(|(a, b)| a.is_finite() && b.is_finite() && b >= a);
        match fig {
            Figure::Stat(s) => stat(s, table, range, zone),
            Figure::Meter(m) => meter(m, table, range, zone),
            Figure::Sparkline(s) => sparkline(s, table, range, zone),
            Figure::Chart(_) => Err(not_a_tile("a chart")),
            Figure::Graph(_) => Err(not_a_tile("a graph")),
            Figure::Dashboard(_) => Err(not_a_tile("a dashboard")),
        }
    }

    /// The least room it reads in, in px at scale 1: the label, and the value at 18 px, which
    /// never ellipsizes.
    pub fn min_size(&self, m: &mut dyn Measure) -> (f32, f32) {
        let value = TextSpec::new(draw::VALUE_MIN).weight(Weight::Semibold);
        let label = TextSpec::new(draw::LABEL);
        let vw = m.width(&self.text(), value);
        let lw = m.width(&self.label, label).min(96.0);
        let w = (2.0 * draw::PAD + vw.max(lw)).ceil().max(64.0);
        let h = match self.kind {
            TileKind::Meter => 2.0 * 12.0 + m.cap(label) + 12.0 + draw::TRACK_H,
            _ => {
                2.0 * 12.0
                    + m.cap(label)
                    + draw::LABEL_TO_VALUE * draw::VALUE_MIN / draw::VALUE
                    + 2.0
            }
        };
        (w, h.ceil())
    }

    /// A tile row's height: m4's 100 px for a stat and a sparkline, 64 for a meter.
    pub fn natural_height(&self) -> f32 {
        match self.kind {
            TileKind::Stat => STAT_HEIGHT,
            TileKind::Meter => METER_HEIGHT,
            TileKind::Sparkline => SPARKLINE_HEIGHT,
        }
    }

    /// The value as the tile writes it: abbreviated, or by the spec's format; "–" with no
    /// rows.
    pub fn text(&self) -> String {
        if self.value.is_nan() {
            return "\u{2013}".into();
        }
        self.short(self.value)
    }

    /// The value in full, for the card: "1,284,512".
    pub fn exact(&self) -> String {
        self.full(self.value)
    }

    /// The value with its unit, in full: "182.4 ms", "0.41 %".
    pub fn exact_with_unit(&self) -> String {
        with_unit(&self.exact(), self.unit.as_deref())
    }

    /// `v` abbreviated as the tile's value is.
    pub fn short(&self, v: f64) -> String {
        match &self.format {
            Some(f) => format_with(v, f, &self.zone),
            None => short_number(v),
        }
    }

    /// `v` in full as the card writes the value.
    pub fn full(&self, v: f64) -> String {
        match &self.format {
            Some(f) => format_with(v, f, &self.zone),
            None => format_value(v),
        }
    }

    /// What the value aggregates, as the card says it: "sum of rpm", "count of rows",
    /// "newest rpm".
    pub fn aggregate_text(&self) -> String {
        let f = self.field.as_deref();
        match (self.kind, self.op, f) {
            (TileKind::Sparkline, _, Some(f)) => format!("newest {f}"),
            (_, AggOp::Count, None) => "count of rows".into(),
            (_, AggOp::Count, Some(f)) => format!("count of {f}"),
            (_, AggOp::Last, Some(f)) => format!("newest {f}"),
            (_, op, Some(f)) => format!("{} of {f}", op_word(op)),
            (_, op, None) => op_word(op).to_string(),
        }
    }

    /// The range the value is over, written: "06:00 – 12:00 · Fri 21 Nov", "all rows".
    pub fn range_text(&self) -> String {
        match self.range {
            Some((a, b)) => self.span_text(a, b),
            None => "all rows".into(),
        }
    }

    /// A span along the trend's field, written: two times, or two numbers.
    pub fn span_text(&self, a: f64, b: f64) -> String {
        if !self.time {
            let (x, y) = (format_value(a), format_value(b));
            let name = self.along.as_deref().unwrap_or("x");
            return if a == b {
                format!("{name} {x}")
            } else {
                format!("{name} {x} \u{2013} {y}")
            };
        }
        let step = if (b - a).abs() >= 3.0 * 86_400.0 {
            86_400.0
        } else {
            60.0
        };
        if a == b {
            return format_time_full(a, step, &self.zone);
        }
        let (fa, fb) = (
            format_time_full(a, step, &self.zone),
            format_time_full(b, step, &self.zone),
        );
        let day = |s: &str| s.split(" \u{b7} ").nth(1).map(str::to_string);
        if step < 86_400.0 && day(&fa).is_some() && day(&fa) == day(&fb) {
            format!("{} \u{2013} {}", format_time_point(a, step, &self.zone), fb)
        } else {
            format!("{fa} \u{2013} {fb}")
        }
    }

    /// A trend point's place, written for its card: its span, or its own place.
    pub fn point_text(&self, i: usize) -> String {
        match self.trend.get(i) {
            Some(p) => self.span_text(p.span.0, p.span.1),
            None => String::new(),
        }
    }

    /// The trend point nearest `x` along the trend's field (a time from the dashboard's linked
    /// crosshair), if there's one within half a part's width of it (any for a sparkline's, as
    /// a chart's crosshair snaps).
    pub fn trend_at(&self, x: f64) -> Option<usize> {
        if self.trend.is_empty() || x.is_nan() {
            return None;
        }
        let i = (0..self.trend.len()).min_by(|&a, &b| {
            (self.trend[a].x - x)
                .abs()
                .total_cmp(&(self.trend[b].x - x).abs())
        })?;
        let p = self.trend[i];
        let half = match self.trend.len() {
            0 | 1 => f64::INFINITY,
            n => (self.trend_domain.1 - self.trend_domain.0).abs() / (n - 1) as f64,
        };
        let inside = x >= p.span.0 && x <= p.span.1;
        let near = (p.x - x).abs() <= half.max(f64::MIN_POSITIVE);
        (inside || near || self.kind == TileKind::Sparkline).then_some(i)
    }

    /// A meter's fill: the value's share of its max, 0 to 1 (0 with no value).
    pub fn fraction(&self) -> f64 {
        if self.value.is_nan() || !(self.max > 0.0) {
            return 0.0;
        }
        (self.value / self.max).clamp(0.0, 1.0)
    }

    /// A meter's status: the highest threshold its value has reached.
    pub fn status(&self) -> Option<Status> {
        let f = if self.value.is_nan() || !(self.max > 0.0) {
            return None;
        } else {
            self.value / self.max
        };
        self.thresholds
            .iter()
            .rev()
            .find(|t| f >= t.at)
            .map(|t| t.status)
    }

    /// The change's amount as the delta line writes it: "4.2 %", "0.08 pt", "12 ms"; and the
    /// whole line after the arrow: "4.2 % vs previous 6h". None without a change, or with
    /// nothing to compare.
    pub fn change_text(&self) -> Option<(String, String)> {
        let c = self.change.as_ref().filter(|c| !c.missing())?;
        let amount = change_amount(c, self.unit.as_deref());
        let line = format!("{amount} vs {}", c.period);
        Some((amount, line))
    }
}

/// "a chart isn't a tile".
fn not_a_tile(what: &str) -> FigError {
    FigError::new(format!(
        "{what} isn't a tile: a tile is a stat, a meter or a sparkline"
    ))
    .with_kind(ErrorKind::Mismatch)
}

/// An aggregate's word in "sum of rpm".
fn op_word(op: AggOp) -> &'static str {
    match op {
        AggOp::Count => "count",
        AggOp::Sum => "sum",
        AggOp::Mean => "mean",
        AggOp::Min => "least",
        AggOp::Max => "most",
        AggOp::P50 => "median",
        AggOp::P95 => "p95",
        AggOp::Last => "newest",
    }
}

/// A value and its unit: "0.41 %" ("%" after a space, as the axes write it), "182 ms".
pub fn with_unit(v: &str, unit: Option<&str>) -> String {
    match unit.filter(|u| !u.is_empty()) {
        Some(u) => format!("{v} {u}"),
        None => v.to_string(),
    }
}

/// A number abbreviated for a tile: compact to three significant figures from 10,000 (12.9K,
/// 1.28M), whole from 100 (3,412, 182), one decimal from 10 (12.3), two from 0.1 (0.41, 4.27),
/// else two significant figures (0.0041); trailing zeros dropped.
pub fn short_number(v: f64) -> String {
    if !v.is_finite() {
        return format_value(v);
    }
    let a = v.abs();
    if a >= 10_000.0 {
        return format_number(v, f64::NAN, NumberStyle::Compact);
    }
    if a >= 100.0 {
        return format_fixed(v.round(), 0);
    }
    if a == 0.0 {
        return "0".into();
    }
    let decimals = if a >= 10.0 {
        1
    } else if a >= 0.1 {
        2
    } else {
        (1 - a.log10().floor() as i32).clamp(2, 8) as usize
    };
    trim(&format_fixed(v, decimals))
}

/// Trailing zeros after a decimal point dropped, and the point with them.
fn trim(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A change's amount: in percent of what it's against when there's no unit (one decimal under
/// 10 %), in points for a unit of `%`, else in the unit.
fn change_amount(c: &Change, unit: Option<&str>) -> String {
    let unit = unit.filter(|u| !u.is_empty());
    match (unit, c.rel) {
        (Some("%"), _) => format!("{} pt", short_number(c.diff.abs())),
        (Some(u), _) => format!("{} {u}", short_number(c.diff.abs())),
        (None, Some(r)) => {
            let r = r.abs();
            let s = if r < 0.05 {
                "< 0.1".to_string()
            } else if r < 10.0 {
                trim(&format_fixed(r, 1))
            } else {
                format_fixed(r.round(), 0)
            };
            format!("{s} %")
        }
        (None, None) => short_number(c.diff.abs()),
    }
}

/// A duration in seconds as a range's length: "6h", "15m", "7d", "1h 30m", "45s".
pub fn duration_text(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return String::new();
    }
    let s = secs.round() as u64;
    let (d, h, m, sec) = (s / 86_400, s % 86_400 / 3_600, s % 3_600 / 60, s % 60);
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(format!("{d}d"));
    }
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if sec > 0 || parts.is_empty() {
        parts.push(format!("{sec}s"));
    }
    parts.truncate(2);
    parts.join(" ")
}

// ------------------------------------------------------------------ reading the table

/// A column by name, or a Mismatch error naming the table's columns.
fn column<'a>(table: &'a Table, name: &str, what: &str) -> Result<&'a Column> {
    table.column(name).ok_or_else(|| {
        let has: Vec<&str> = table.columns.iter().map(|c| &*c.name).collect();
        let has = if has.is_empty() {
            "it has no columns".to_string()
        } else {
            format!("it has {}", has.join(", "))
        };
        FigError::new(format!("{what} \"{name}\" isn't in the data: {has}"))
            .with_kind(ErrorKind::Mismatch)
    })
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

/// A column of numbers, or a Mismatch error.
fn numbers<'a>(table: &'a Table, name: &str, what: &str) -> Result<&'a Column> {
    let c = column(table, name, what)?;
    match c.values {
        Values::Number(_) => Ok(c),
        _ => Err(FigError::new(format!(
            "{what} \"{name}\" must hold numbers, and it holds {}",
            holds(c)
        ))
        .with_kind(ErrorKind::Mismatch)),
    }
}

/// A column a trend runs along: times or numbers, or a Mismatch error.
fn ordered<'a>(table: &'a Table, name: &str, what: &str) -> Result<&'a Column> {
    let c = column(table, name, what)?;
    match c.values {
        Values::Number(_) | Values::Time(_) => Ok(c),
        _ => Err(FigError::new(format!(
            "{what} \"{name}\" must hold times or numbers, and it holds {}",
            holds(c)
        ))
        .with_kind(ErrorKind::Mismatch)),
    }
}

/// The column the range applies to: `along` (a trend's field), else the first time column.
fn time_column<'a>(table: &'a Table, along: Option<&'a Column>) -> Option<&'a Column> {
    along.or_else(|| {
        table
            .columns
            .iter()
            .find(|c| matches!(c.values, Values::Time(_)))
    })
}

/// The rows whose value along `t` is within `[lo, hi]` (both ends in), in table order; every
/// row with no range or no column to apply it to.
fn rows_in(table: &Table, t: Option<&Column>, range: Option<(f64, f64)>) -> Vec<usize> {
    match (t, range) {
        (Some(c), Some((lo, hi))) => (0..table.rows)
            .filter(|&r| {
                let v = c.f64_at(r);
                v >= lo && v <= hi
            })
            .collect(),
        _ => (0..table.rows).collect(),
    }
}

/// The rows whose value along `t` is within `[lo, hi)`.
fn rows_before(table: &Table, t: &Column, lo: f64, hi: f64) -> Vec<usize> {
    (0..table.rows)
        .filter(|&r| {
            let v = t.f64_at(r);
            v >= lo && v < hi
        })
        .collect()
}

/// The least and the most finite value of a column over some rows.
fn extent(c: &Column, rows: &[usize]) -> Option<(f64, f64)> {
    rows.iter()
        .map(|&r| c.f64_at(r))
        .filter(|v| v.is_finite())
        .fold(None, |acc, v| match acc {
            None => Some((v, v)),
            Some((a, b)) => Some((a.min(v), b.max(v))),
        })
}

/// One value of many rows: `op` over `field`'s values in them (a count of rows with no field);
/// `last` the newest by `along` (else the last in table order). NaN when there's none.
fn aggregate(op: AggOp, field: Option<&Column>, along: Option<&Column>, rows: &[usize]) -> f64 {
    let Some(f) = field else {
        return rows.len() as f64;
    };
    let present: Vec<usize> = rows
        .iter()
        .copied()
        .filter(|&r| !f.f64_at(r).is_nan())
        .collect();
    if op == AggOp::Count {
        return present.len() as f64;
    }
    if present.is_empty() {
        return f64::NAN;
    }
    if op == AggOp::Last {
        let newest = match along {
            Some(a) => present
                .iter()
                .copied()
                .filter(|&r| !a.f64_at(r).is_nan())
                .max_by(|&x, &y| a.f64_at(x).total_cmp(&a.f64_at(y)).then(x.cmp(&y))),
            None => present.last().copied(),
        };
        return newest.map_or(f64::NAN, |r| f.f64_at(r));
    }
    let mut v: Vec<f64> = present.iter().map(|&r| f.f64_at(r)).collect();
    match op {
        AggOp::Sum => v.iter().sum(),
        AggOp::Mean => v.iter().sum::<f64>() / v.len() as f64,
        AggOp::Min => v.iter().copied().fold(f64::INFINITY, f64::min),
        AggOp::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        AggOp::P50 => quantile(&mut v, 0.5),
        AggOp::P95 => quantile(&mut v, 0.95),
        AggOp::Count | AggOp::Last => unreachable!("handled above"),
    }
}

/// The trend's points over `rows`: one a distinct place along `along` when there are no more
/// than [`TREND_POINTS`], else the aggregate over that many equal parts of `span` (parts with
/// no rows left out). With the domain its ends are drawn at.
fn trend_points(
    op: AggOp,
    field: Option<&Column>,
    along: &Column,
    rows: &[usize],
    span: Option<(f64, f64)>,
) -> (Vec<TrendPoint>, (f64, f64)) {
    let mut places: Vec<f64> = rows
        .iter()
        .map(|&r| along.f64_at(r))
        .filter(|v| v.is_finite())
        .collect();
    places.sort_by(f64::total_cmp);
    places.dedup();
    let Some(&first) = places.first() else {
        return (Vec::new(), (0.0, 0.0));
    };
    let last = places[places.len() - 1];
    if places.len() <= TREND_POINTS {
        let mut out = Vec::with_capacity(places.len());
        for &x in &places {
            let at: Vec<usize> = rows
                .iter()
                .copied()
                .filter(|&r| along.f64_at(r) == x)
                .collect();
            let y = aggregate(op, field, Some(along), &at);
            if !y.is_nan() {
                out.push(TrendPoint { x, span: (x, x), y });
            }
        }
        return (out, (first, last));
    }
    let (lo, hi) = span.unwrap_or((first, last));
    let step = (hi - lo) / TREND_POINTS as f64;
    let mut parts: Vec<Vec<usize>> = vec![Vec::new(); TREND_POINTS];
    for &r in rows {
        let v = along.f64_at(r);
        if !v.is_finite() || step <= 0.0 {
            continue;
        }
        let k = (((v - lo) / step).floor() as i64).clamp(0, TREND_POINTS as i64 - 1) as usize;
        parts[k].push(r);
    }
    let mut out = Vec::new();
    for (k, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let y = aggregate(op, field, Some(along), part);
        if y.is_nan() {
            continue;
        }
        let a = lo + k as f64 * step;
        out.push(TrendPoint {
            x: a + step / 2.0,
            span: (a, a + step),
            y,
        });
    }
    (out, (lo + step / 2.0, hi - step / 2.0))
}

fn stat(
    s: &StatSpec,
    table: &Table,
    range: Option<(f64, f64)>,
    zone: &TimeZone,
) -> Result<TileModel> {
    let field = match &s.value.field {
        Some(f) => Some(numbers(table, f, "the value's field")?),
        None => None,
    };
    let along = match &s.trend {
        Some(t) => Some(ordered(table, t, "the trend's field")?),
        None => None,
    };
    let t = time_column(table, along);
    let rows = rows_in(table, t, range);
    let value = aggregate(s.value.op, field, t, &rows);
    let shown = match (range, t) {
        (Some(r), Some(_)) => Some(r),
        (_, Some(c)) => extent(c, &rows),
        _ => None,
    };
    let change = match &s.delta {
        None => None,
        Some(d) => {
            let (against, period) = match d {
                Delta::Value(v) => (*v, with_unit(&short_number(*v), s.unit.as_deref())),
                Delta::Previous => match (t, shown) {
                    (Some(c), Some((lo, hi))) => {
                        let len = hi - lo;
                        let before = rows_before(table, c, lo - len, lo);
                        let v = if before.is_empty() || len <= 0.0 {
                            f64::NAN
                        } else {
                            aggregate(s.value.op, field, Some(c), &before)
                        };
                        let what = if matches!(c.values, Values::Time(_)) {
                            duration_text(len)
                        } else {
                            format!("{} {}", short_number(len), c.name)
                        };
                        (v, format!("previous {what}").trim_end().to_string())
                    }
                    _ => (f64::NAN, "previous".into()),
                },
            };
            Some(compare(value, against, period, s.good))
        }
    };
    let (trend, trend_domain) = match along {
        Some(a) => trend_points(s.value.op, field, a, &rows, shown),
        None => (Vec::new(), (0.0, 0.0)),
    };
    Ok(TileModel {
        kind: TileKind::Stat,
        label: s.label.clone(),
        value,
        unit: s.unit.clone(),
        format: s.format.clone(),
        op: s.value.op,
        field: s.value.field.clone(),
        rows: rows.len(),
        range: shown,
        given: range.is_some() && t.is_some(),
        time: t.is_some_and(|c| matches!(c.values, Values::Time(_))),
        along: t.map(|c| c.name.to_string()),
        change,
        trend,
        trend_domain,
        max: f64::NAN,
        thresholds: Vec::new(),
        zone: zone.clone(),
    })
}

/// The change from `against` to `value`, good by `good`.
fn compare(value: f64, against: f64, period: String, good: GoodWhen) -> Change {
    let diff = value - against;
    let rel = (against != 0.0 && against.is_finite() && value.is_finite())
        .then(|| diff / against.abs() * 100.0);
    let dir = if diff.is_nan() || diff == 0.0 {
        0
    } else {
        diff.signum() as i8
    };
    let good = match (good, dir) {
        (_, 0) | (GoodWhen::Neither, _) => None,
        (GoodWhen::Up, d) => Some(d > 0),
        (GoodWhen::Down, d) => Some(d < 0),
    };
    Change {
        against,
        period,
        diff,
        rel,
        good,
    }
}

fn meter(
    s: &MeterSpec,
    table: &Table,
    range: Option<(f64, f64)>,
    zone: &TimeZone,
) -> Result<TileModel> {
    let field = match &s.value.field {
        Some(f) => Some(numbers(table, f, "the value's field")?),
        None => None,
    };
    let t = time_column(table, None);
    let rows = rows_in(table, t, range);
    let value = aggregate(s.value.op, field, t, &rows);
    let shown = match (range, t) {
        (Some(r), Some(_)) => Some(r),
        (_, Some(c)) => extent(c, &rows),
        _ => None,
    };
    Ok(TileModel {
        kind: TileKind::Meter,
        label: s.label.clone(),
        value,
        unit: s.unit.clone(),
        format: s.format.clone(),
        op: s.value.op,
        field: s.value.field.clone(),
        rows: rows.len(),
        range: shown,
        given: range.is_some() && t.is_some(),
        time: t.is_some(),
        along: t.map(|c| c.name.to_string()),
        change: None,
        trend: Vec::new(),
        trend_domain: (0.0, 0.0),
        max: s.max,
        thresholds: s.thresholds.clone(),
        zone: zone.clone(),
    })
}

fn sparkline(
    s: &SparklineSpec,
    table: &Table,
    range: Option<(f64, f64)>,
    zone: &TimeZone,
) -> Result<TileModel> {
    let x = ordered(table, &s.x, "the sparkline's x")?;
    let y = numbers(table, &s.y, "the sparkline's y")?;
    let time = matches!(x.values, Values::Time(_));
    let rows = rows_in(table, time.then_some(x), range);
    // Rows sharing an x summed, in x order.
    let mut pts: Vec<(f64, f64)> = rows
        .iter()
        .map(|&r| (x.f64_at(r), y.f64_at(r)))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut trend: Vec<TrendPoint> = Vec::with_capacity(pts.len());
    for (a, b) in pts {
        match trend.last_mut() {
            Some(p) if p.x == a => p.y += b,
            _ => trend.push(TrendPoint {
                x: a,
                span: (a, a),
                y: b,
            }),
        }
    }
    let value = trend.last().map_or(f64::NAN, |p| p.y);
    let domain = match (trend.first(), trend.last()) {
        (Some(a), Some(b)) => (a.x, b.x),
        _ => (0.0, 0.0),
    };
    let shown = match (range, time) {
        (Some(r), true) => Some(r),
        _ => (!trend.is_empty()).then_some(domain),
    };
    Ok(TileModel {
        kind: TileKind::Sparkline,
        label: s.label.clone(),
        value,
        unit: s.unit.clone(),
        format: s.format.clone(),
        op: AggOp::Last,
        field: Some(s.y.clone()),
        rows: rows.len(),
        range: shown,
        given: range.is_some() && time,
        time,
        along: Some(s.x.clone()),
        change: None,
        trend,
        trend_domain: domain,
        max: f64::NAN,
        thresholds: Vec::new(),
        zone: zone.clone(),
    })
}

// ------------------------------------------------------------------ the detail panel

/// A status's name.
pub fn status_name(s: Status) -> &'static str {
    match s {
        Status::Good => "good",
        Status::Warning => "warning",
        Status::Serious => "serious",
        Status::Critical => "critical",
    }
}

/// The tile as the detail panel shows it (for `⏎`): its value in full, what it aggregates and
/// over which rows; a stat's change and trend, a meter's max, status and thresholds, a
/// sparkline's newest, least, most and mean.
pub fn detail(model: &TileModel) -> DetailTree {
    let kind = match model.kind {
        TileKind::Stat => "stat",
        TileKind::Meter => "meter",
        TileKind::Sparkline => "sparkline",
    };
    let mut t = DetailTree::new(
        model.label.clone(),
        format!("{kind} \u{b7} {}", model.aggregate_text()),
    );
    let value = if model.value.is_nan() {
        DetailRow::field("value", "no data").quiet()
    } else {
        DetailRow::field("value", model.exact_with_unit()).copying(model.exact())
    };
    t.rows.push(value);
    if model.kind != TileKind::Sparkline {
        t.rows
            .push(DetailRow::field("aggregate", model.aggregate_text()));
    }
    let what = if model.given { "range" } else { "data" };
    t.rows.push(DetailRow::field(what, model.range_text()));
    t.rows
        .push(DetailRow::field("rows", format_fixed(model.rows as f64, 0)));
    if let Some(c) = &model.change {
        if c.missing() {
            t.rows
                .push(DetailRow::field(c.period.clone(), "no data").quiet());
        } else {
            t.rows.push(
                DetailRow::field(
                    c.period.clone(),
                    with_unit(&model.full(c.against), model.unit.as_deref()),
                )
                .copying(model.full(c.against)),
            );
            let arrow = match c.direction() {
                1 => "\u{25b2} ",
                -1 => "\u{25bc} ",
                _ => "",
            };
            let amount = model.change_text().map(|(a, _)| a).unwrap_or_default();
            let verdict = match c.good {
                Some(true) => " \u{b7} good",
                Some(false) => " \u{b7} bad",
                None => "",
            };
            t.rows.push(DetailRow::field(
                "change",
                format!("{arrow}{amount}{verdict}"),
            ));
        }
    }
    match model.kind {
        TileKind::Meter => {
            t.rows.push(DetailRow::field(
                "max",
                with_unit(&model.full(model.max), model.unit.as_deref()),
            ));
            t.rows.push(DetailRow::field(
                "of max",
                format!("{} %", short_number(model.fraction() * 100.0)),
            ));
            if let Some(s) = model.status() {
                t.rows.push(DetailRow::field("status", status_name(s)));
            }
            if !model.thresholds.is_empty() {
                t.rows.push(DetailRow::section("thresholds"));
                for th in &model.thresholds {
                    t.rows.push(DetailRow::field(
                        format!("{} %", short_number(th.at * 100.0)),
                        status_name(th.status),
                    ));
                }
            }
        }
        TileKind::Sparkline => {
            let ys: Vec<f64> = model.trend.iter().map(|p| p.y).collect();
            if !ys.is_empty() {
                let lo = ys.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let mean = ys.iter().sum::<f64>() / ys.len() as f64;
                let u = model.unit.as_deref();
                t.rows
                    .push(DetailRow::field("least", with_unit(&model.full(lo), u)));
                t.rows
                    .push(DetailRow::field("most", with_unit(&model.full(hi), u)));
                t.rows
                    .push(DetailRow::field("mean", with_unit(&short_number(mean), u)));
                t.rows
                    .push(DetailRow::field("points", format_fixed(ys.len() as f64, 0)));
            }
        }
        TileKind::Stat => {
            if !model.trend.is_empty() {
                t.rows.push(DetailRow::section("trend"));
                for (i, p) in model.trend.iter().enumerate() {
                    t.rows.push(
                        DetailRow::field(
                            model.point_text(i),
                            with_unit(&model.full(p.y), model.unit.as_deref()),
                        )
                        .copying(model.full(p.y)),
                    );
                }
            }
        }
    }
    t
}
