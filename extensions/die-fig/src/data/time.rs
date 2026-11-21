//! Times parsed in an injected zone (ruling 2), as `f64` seconds since the Unix epoch, UTC.
//!
//! A cell is tried as a `jiff::Timestamp` (it has an offset or `Z`), then, when it carries a
//! `[zone]` annotation and no offset, as a `jiff::Zoned` in that zone, then as a
//! `jiff::civil::DateTime` in [`ReadOptions::zone`], then as a `jiff::civil::Date` (midnight in
//! the zone) only when it has no time part. A civil time that a change of clocks skips or repeats
//! takes jiff's compatible reading: in a gap the later instant, in a fold the earlier.
//!
//! Slashed and day-first dates need an explicit [`ReadOptions::time_format`] (strtime), which
//! is tried before the ISO forms; day and month are never guessed. A plain number is never a
//! time here: epoch numbers stay numbers unless a spec says `"type": "time"` with a `"unit"`,
//! which [`as_time`] applies.

use super::infer::{is_missing, parse_number};
use super::{Column, ReadOptions, Values};
use crate::error::{FigError, Result};
use jiff::civil::{Date, DateTime};
use jiff::fmt::strtime;
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

/// A cell's time in UTC seconds since the epoch, if it is one.
pub fn parse_time(s: &str, opt: &ReadOptions) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(fmt) = opt.time_format.as_deref()
        && let Some(t) = with_format(s, fmt, &opt.zone)
    {
        return Some(t);
    }
    iso(s, &opt.zone)
}

/// A cell read with a strtime format: an epoch (`%s`), an offset or an IANA zone in the text
/// wins over `zone`; a civil date and time is in `zone`, and a date alone is its midnight there.
pub fn with_format(s: &str, fmt: &str, zone: &TimeZone) -> Option<f64> {
    let tm = strtime::parse(fmt, s.trim()).ok()?;
    if tm.timestamp().is_some() || tm.offset().is_some() {
        return tm.to_timestamp().ok().map(seconds);
    }
    if tm.iana_time_zone().is_some() {
        return tm.to_zoned().ok().map(|z| seconds(z.timestamp()));
    }
    let dt = tm.to_datetime().ok()?;
    zone.to_timestamp(dt).ok().map(seconds)
}

/// The ISO forms, in ruling 2's order.
fn iso(s: &str, zone: &TimeZone) -> Option<f64> {
    // "20261121" would read as a basic-format date: a number is never a time without a unit.
    if parse_number(s, None).is_some() {
        return None;
    }
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Some(seconds(ts));
    }
    if s.contains('[')
        && let Ok(z) = s.parse::<Zoned>()
    {
        return Some(seconds(z.timestamp()));
    }
    if let Ok(dt) = s.parse::<DateTime>() {
        return zone.to_timestamp(dt).ok().map(seconds);
    }
    if !s.contains([':', 'T', 't', ' ']) {
        let d = s.parse::<Date>().ok()?;
        return zone
            .to_timestamp(d.to_datetime(jiff::civil::Time::midnight()))
            .ok()
            .map(seconds);
    }
    None
}

/// An instant as `f64` seconds since the epoch.
fn seconds(ts: Timestamp) -> f64 {
    ts.as_duration().as_secs_f64()
}

/// An epoch number in `unit` (`"s"` or `"ms"`) as seconds; None for another unit.
pub fn epoch(v: f64, unit: &str) -> Option<f64> {
    match unit {
        "s" => Some(v),
        "ms" => Some(v / 1000.0),
        _ => None,
    }
}

/// A column as times, for a channel whose spec says `"type": "time"`: a time column as it is;
/// numbers as epoch seconds or milliseconds, only with a `unit`; strings parsed by
/// [`parse_time`], every one that isn't missing.
pub fn as_time(col: &Column, unit: Option<&str>, opt: &ReadOptions) -> Result<Column> {
    let name = &*col.name;
    match (&col.values, unit) {
        (Values::Time(_), _) => Ok(col.clone()),
        (Values::Number(v), Some(u)) => {
            if epoch(0.0, u).is_none() {
                return Err(FigError::new(format!(
                    "unit \"{u}\": an epoch time's unit is \"s\" or \"ms\""
                )));
            }
            let out = v.iter().map(|&x| epoch(x, u).unwrap_or(f64::NAN));
            Ok(Column::time(name, out.collect()))
        }
        (Values::Number(_), None) => Err(FigError::new(format!(
            "\"{name}\" holds numbers: give \"unit\": \"s\" or \"ms\" to read them as epoch times"
        ))),
        (Values::Category { .. } | Values::Text(_), _) => {
            let mut out = Vec::with_capacity(col.len());
            for row in 0..col.len() {
                let t = match col.str_at(row) {
                    None => f64::NAN,
                    Some(s) if is_missing(s) => f64::NAN,
                    Some(s) => parse_time(s, opt).ok_or_else(|| {
                        FigError::new(format!("\"{s}\" in \"{name}\" is not a time"))
                    })?,
                };
                out.push(t);
            }
            Ok(Column::time(name, out))
        }
    }
}
