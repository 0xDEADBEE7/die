//! The time scale: UTC seconds mapped proportionally, ticked in calendar units (seconds to years)
//! in the zone, through `jiff::Zoned` so a DST change falls where the clock says.
//!
//! The units are d3's: 1, 5, 15 and 30 seconds and minutes, 1, 3, 6 and 12 hours, 1 and 2 days,
//! a week, 1 and 3 months, then years at nice steps; the one nearest a `count`th of the domain
//! wins. Under a second the ticks are linear ones. Each tick is where the zone's clock reads a
//! round time: an hour's ticks skip the hour that a spring-forward change jumps over and take
//! both of the hours an autumn change repeats; a day, a week, a month and a year start at the
//! day's first instant (midnight, or the end of a gap that swallows it). Weeks start on Monday.

use jiff::civil::{Date, DateTime};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{Timestamp, ToSpan, Zoned};

use super::Tick;
use super::format::{format_time, format_time_point};
use super::linear::{self, Inc, ascending};

const MINUTE: f64 = 60.0;
const HOUR: f64 = 3_600.0;
const DAY: f64 = 86_400.0;
const WEEK: f64 = 7.0 * DAY;
const MONTH: f64 = 30.0 * DAY;
const YEAR: f64 = 365.0 * DAY;

/// A bound on steps walked for one call; a sane domain takes a few times `count`.
const WALK: usize = 100_000;

/// The longest step in years: jiff's calendar holds 19,999 of them (-9999 to 9999), so a longer
/// one would have a tick or none, and past 19,998 jiff refuses the span.
const MOST_YEARS: i64 = 10_000;

/// A float of UTC seconds as a timestamp; None when it isn't finite or falls outside the years
/// jiff knows (-9999 to 9999).
///
/// The fraction is rounded to the finest power of ten of nanoseconds the float can tell apart
/// there: about a microsecond in 2026, where `i / 5` for a tick at .6 s is 0.5999998 s past the
/// second, and should say ".600".
pub(crate) fn timestamp(v: f64) -> Option<Timestamp> {
    if !v.is_finite() {
        return None;
    }
    let mut s = v.floor();
    if !(-4.0e11..4.0e11).contains(&s) {
        return None;
    }
    let grain = grain_ns(v);
    let mut ns = ((v - s) * 1e9 / grain).round() * grain;
    if ns >= 1e9 {
        s += 1.0;
        ns = 0.0;
    }
    Timestamp::new(s as i64, ns as i32).ok()
}

/// The finest power of ten of nanoseconds that floats of UTC seconds near `v` tell apart: a
/// microsecond in 2026, a nanosecond near 1970.
fn grain_ns(v: f64) -> f64 {
    let ulp_ns = v.abs().max(1.0) * f64::EPSILON * 1e9;
    10f64.powf(ulp_ns.log10().ceil()).clamp(1.0, 1e9)
}

/// `v` in `zone`.
pub(crate) fn zoned(v: f64, zone: &TimeZone) -> Option<Zoned> {
    Some(timestamp(v)?.to_zoned(zone.clone()))
}

/// A timestamp as a float of UTC seconds.
pub(crate) fn seconds(t: Timestamp) -> f64 {
    t.as_second() as f64 + t.subsec_nanosecond() as f64 / 1e9
}

/// How fine a time is, or how fine a label must be: the order of the calendar's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    /// Past the second.
    SubSecond,
    /// On a second.
    Second,
    /// On a minute or an hour: the clock.
    Clock,
    /// On a day's start.
    Day,
    /// On a month's start.
    Month,
    /// On a year's start.
    Year,
}

/// The coarsest field `z` starts: a midnight on the 21st is a day, on the 1st of a month a month.
/// A day starts at its first instant: 01:00 where a spring change swallows midnight, and only
/// the first of two midnights an autumn change repeats.
pub(crate) fn level_of(z: &Zoned) -> Level {
    if z.subsec_nanosecond() != 0 {
        Level::SubSecond
    } else if z.second() != 0 {
        Level::Second
    } else if !starts_day(z) {
        Level::Clock
    } else if z.day() != 1 {
        Level::Day
    } else if z.month() != 1 {
        Level::Month
    } else {
        Level::Year
    }
}

/// Whether `z` is its day's first instant in its zone (midnight, when the zone can't say).
fn starts_day(z: &Zoned) -> bool {
    match z.start_of_day() {
        Ok(first) => first.timestamp() == z.timestamp(),
        Err(_) => z.hour() == 0 && z.minute() == 0,
    }
}

/// The finest field worth writing for times `step` seconds apart. A step that isn't a positive
/// number asks for every field. The bounds leave room for a 23-hour day and a 28-day month.
pub(crate) fn level_for_step(step: f64) -> Level {
    if !(step.is_finite() && step > 0.0) || step < 1.0 {
        Level::SubSecond
    } else if step < MINUTE {
        Level::Second
    } else if step < 23.0 * HOUR {
        Level::Clock
    } else if step < 27.5 * DAY {
        Level::Day
    } else if step < 360.0 * DAY {
        Level::Month
    } else {
        Level::Year
    }
}

/// A calendar unit a tick steps by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl Unit {
    /// The label a tick of this unit needs at least.
    fn level(self) -> Level {
        match self {
            Unit::Second => Level::Second,
            Unit::Minute | Unit::Hour => Level::Clock,
            Unit::Day | Unit::Week => Level::Day,
            Unit::Month => Level::Month,
            Unit::Year => Level::Year,
        }
    }
}

/// d3's calendar steps, with two hours between one and three (a six-hour range reads in
/// two-hour steps, as the design's m4 has it): the unit, how many of it, and about how long
/// that is.
const INTERVALS: [(Unit, i64, f64); 19] = [
    (Unit::Second, 1, 1.0),
    (Unit::Second, 5, 5.0),
    (Unit::Second, 15, 15.0),
    (Unit::Second, 30, 30.0),
    (Unit::Minute, 1, MINUTE),
    (Unit::Minute, 5, 5.0 * MINUTE),
    (Unit::Minute, 15, 15.0 * MINUTE),
    (Unit::Minute, 30, 30.0 * MINUTE),
    (Unit::Hour, 1, HOUR),
    (Unit::Hour, 2, 2.0 * HOUR),
    (Unit::Hour, 3, 3.0 * HOUR),
    (Unit::Hour, 6, 6.0 * HOUR),
    (Unit::Hour, 12, 12.0 * HOUR),
    (Unit::Day, 1, DAY),
    (Unit::Day, 2, 2.0 * DAY),
    (Unit::Week, 1, WEEK),
    (Unit::Month, 1, MONTH),
    (Unit::Month, 3, 3.0 * MONTH),
    (Unit::Year, 1, YEAR),
];

/// The calendar step for about `count` ticks over `lo ..= hi`: (unit, how many, about how many
/// seconds), or None when it's under a second.
fn interval(lo: f64, hi: f64, count: usize) -> Option<(Unit, i64, f64)> {
    let target = (hi - lo) / count.max(1) as f64;
    let i = INTERVALS
        .iter()
        .position(|&(_, _, d)| d > target)
        .unwrap_or(INTERVALS.len());
    if i == 0 {
        return None;
    }
    if i == INTERVALS.len() {
        let n = match linear::spec(lo / YEAR, hi / YEAR, count.max(1) as f64) {
            Some((_, _, Inc::Mul(m))) => (m as i64).clamp(1, MOST_YEARS),
            _ => 1,
        };
        return Some((Unit::Year, n, n as f64 * YEAR));
    }
    let (a, b) = (INTERVALS[i - 1], INTERVALS[i]);
    Some(if target / a.2 < b.2 / target { a } else { b })
}

/// The instants a civil time has in `zone`: none in a gap, two in a fold.
fn instants(zone: &TimeZone, dt: DateTime) -> Vec<f64> {
    let offsets = match zone.to_ambiguous_timestamp(dt).offset() {
        AmbiguousOffset::Unambiguous { offset } => vec![offset],
        AmbiguousOffset::Gap { .. } => Vec::new(),
        AmbiguousOffset::Fold { before, after } => vec![before, after],
    };
    offsets
        .into_iter()
        .filter_map(|o| o.to_timestamp(dt).ok())
        .map(seconds)
        .collect()
}

/// The first instant of `d` in `zone`: midnight, or the end of a gap that swallows it.
fn start_of(d: Date, zone: &TimeZone) -> Option<f64> {
    Some(seconds(d.to_zoned(zone.clone()).ok()?.timestamp()))
}

/// The civil time `t` reads in `zone`, rounded down to a multiple of `n` of a clock unit.
fn floor_clock(t: DateTime, unit: Unit, n: i64) -> Option<DateTime> {
    let n8 = n as i8;
    let (h, m, s) = match unit {
        Unit::Hour => (t.hour() - t.hour() % n8, 0, 0),
        Unit::Minute => (t.hour(), t.minute() - t.minute() % n8, 0),
        _ => (t.hour(), t.minute(), t.second() - t.second() % n8),
    };
    DateTime::new(t.year(), t.month(), t.day(), h, m, s, 0).ok()
}

/// The first day of a calendar step at or before `d`; for years in steps whose multiple falls
/// before the calendar's first year, the first multiple inside it.
fn floor_date(d: Date, unit: Unit, n: i64) -> Option<Date> {
    match unit {
        Unit::Week => d
            .checked_sub((d.weekday().to_monday_zero_offset() as i64).days())
            .ok(),
        Unit::Month => {
            let m = d.month() - (d.month() - 1) % n as i8;
            Date::new(d.year(), m, 1).ok()
        }
        Unit::Year => {
            let y = d.year() as i64;
            let first = y - y.rem_euclid(n);
            let year = |y: i64| i16::try_from(y).ok().and_then(|y| Date::new(y, 1, 1).ok());
            year(first).or_else(|| year(first + n))
        }
        _ => Some(d),
    }
}

/// The earliest the zone's clock reads from `t0` to `hi`: what it reads at `t0`, or less just
/// after an autumn change on the way, when it turns back.
fn least_civil(t0: Timestamp, hi: f64, zone: &TimeZone) -> DateTime {
    let mut least = zone.to_datetime(t0);
    for change in zone.following(t0) {
        if seconds(change.timestamp()) > hi {
            break;
        }
        least = least.min(zone.to_datetime(change.timestamp()));
    }
    least
}

/// The instants where the zone's calendar steps by `n` units, from the last at or before `lo`
/// to the first at or after `hi`, ascending and without repeats.
fn steps(lo: f64, hi: f64, unit: Unit, n: i64, zone: &TimeZone) -> Vec<f64> {
    let Some(t0) = timestamp(lo) else {
        return Vec::new();
    };
    let civil = zone.to_datetime(t0);
    let mut out = Vec::new();
    match unit {
        Unit::Second | Unit::Minute | Unit::Hour => {
            let span = match unit {
                Unit::Hour => n.hours(),
                Unit::Minute => n.minutes(),
                _ => n.seconds(),
            };
            // The walk is over the clock's readings, which an autumn change turns back: it
            // starts from the earliest the clock reads in the domain, so the repeated readings'
            // second instants are there even when `lo` is late in their first pass. One step
            // back, so a floor that lands in a gap still leaves one at or before `lo`.
            let least = least_civil(t0, hi, zone);
            let first = floor_clock(least, unit, n).and_then(|dt| dt.checked_sub(span).ok());
            let mut at = first;
            for _ in 0..WALK {
                let Some(dt) = at else { break };
                let here = instants(zone, dt);
                // Done when every instant of this reading is past `hi`: in a fold the second
                // is past it while the first, and the readings after it, are still due.
                let done = !here.is_empty() && here.iter().all(|&t| t >= hi);
                out.extend(here);
                if done {
                    break;
                }
                at = dt.checked_add(span).ok();
            }
        }
        Unit::Day | Unit::Week | Unit::Month | Unit::Year => {
            let (span, back) = match unit {
                Unit::Day => (1.days(), (n + 1).days()),
                Unit::Week => (1.weeks(), 1.weeks()),
                Unit::Month => (n.months(), n.months()),
                _ => (n.years(), n.years()),
            };
            // One step back, or the floor itself at the calendar's first day.
            let first = floor_date(civil.date(), unit, n).map(|d| d.checked_sub(back).unwrap_or(d));
            let mut at = first;
            for _ in 0..WALK {
                let Some(d) = at else { break };
                // Days in twos restart on each month's 1st (1, 3, 5, … 31, 1), as d3's do.
                let aligned = unit != Unit::Day || (d.day() as i64 - 1) % n == 0;
                if aligned && let Some(t) = start_of(d, zone) {
                    out.push(t);
                    if t >= hi {
                        break;
                    }
                }
                at = d.checked_add(span).ok();
            }
        }
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

/// The time scale's ticks (see [`super::ticks`]).
pub(crate) fn ticks(domain: (f64, f64), count: usize, zone: &TimeZone) -> Vec<Tick> {
    let Some((lo, hi)) = ascending(domain) else {
        return Vec::new();
    };
    if lo == hi {
        return vec![Tick {
            value: lo,
            label: format_time_point(lo, MINUTE, zone),
            major: false,
        }];
    }
    let major_past = |v: f64, level: Level| zoned(v, zone).is_some_and(|z| level_of(&z) > level);
    let Some((unit, n, about)) = interval(lo, hi, count) else {
        // No finer than the floats here tell apart (a microsecond in 2026), or the ticks would
        // run together and their labels repeat.
        let grain = grain_ns(lo.abs().max(hi.abs())) / 1e9;
        let count = count.min(((hi - lo) / grain).floor().max(1.0) as usize);
        let Some((vals, step)) = linear::values(lo, hi, count) else {
            return Vec::new();
        };
        return vals
            .into_iter()
            .map(|v| Tick {
                value: v,
                label: format_time(v, step, zone),
                major: major_past(v, Level::SubSecond),
            })
            .collect();
    };
    let slack = (hi - lo) * 1e-12;
    steps(lo, hi, unit, n, zone)
        .into_iter()
        .filter(|&t| t >= lo - slack && t <= hi + slack)
        .map(|v| Tick {
            value: v,
            label: format_time(v, about, zone),
            major: major_past(v, unit.level()),
        })
        .collect()
}

/// A time domain widened to its ticks' calendar unit in `zone` (see [`super::Scale::nice`]).
pub(crate) fn nice(domain: (f64, f64), count: usize, zone: &TimeZone) -> (f64, f64) {
    let Some((lo, hi)) = ascending(domain) else {
        return domain;
    };
    if lo == hi {
        return domain;
    }
    let (a, b) = match interval(lo, hi, count) {
        None => {
            let (a, b, _) = linear::nice(lo, hi, count);
            (a, b)
        }
        Some((unit, n, _)) => {
            let at = steps(lo, hi, unit, n, zone);
            let a = at.iter().rev().find(|&&t| t <= lo).copied().unwrap_or(lo);
            let b = at.iter().find(|&&t| t >= hi).copied().unwrap_or(hi);
            (a, b)
        }
    };
    if domain.0 > domain.1 { (b, a) } else { (a, b) }
}
