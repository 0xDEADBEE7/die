//! A gantt's days: which count, where a duration of working days ends, and how its times and
//! durations are written.
//!
//! In a plan of whole days a duration counts days on the calendar of the zone, through
//! `jiff::civil::Date`, so a day the clocks change is still one day. [`Days`] is the spec's
//! [`Calendar`] resolved in a zone: with `weekdays`, Saturdays and Sundays don't count, nor does
//! any day it lists as off. A day is named by the date it is, and its first instant
//! ([`Days::start`]) is where a bar that starts on it starts.

use crate::scale::format::{clock, month, weekday};
use crate::scale::time::{start_of, zoned};
use crate::spec::Calendar;
use jiff::ToSpan;
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;

/// A day in seconds, where the clocks don't change.
pub const DAY: f64 = 86_400.0;

/// The most days a count walks before it gives up on finding one that works (a calendar that
/// lists every day as off): some 55 years.
const WALK: usize = 20_000;

/// A plan's calendar in its zone.
#[derive(Clone, Debug)]
pub struct Days {
    /// Whether Saturdays and Sundays don't count.
    pub weekdays: bool,
    /// The days off besides, each a first and a last day, both taken in.
    pub off: Vec<(Date, Date)>,
    /// The zone the days are in.
    pub zone: TimeZone,
}

impl Days {
    /// The spec's calendar in `zone`.
    pub fn of(cal: &Calendar, zone: &TimeZone) -> Days {
        let day = |t: f64| zoned(t, zone).map(|z| z.date());
        let off = (cal.off.iter())
            .filter_map(|(a, b)| {
                let first = day(a.secs)?;
                let last = b.as_ref().map_or(Some(first), |b| day(b.secs))?;
                Some((first.min(last), first.max(last)))
            })
            .collect();
        Days {
            weekdays: cal.weekdays,
            off,
            zone: zone.clone(),
        }
    }

    /// Whether every day counts: no weekends out, no days off.
    pub fn every_day(&self) -> bool {
        !self.weekdays && self.off.is_empty()
    }

    /// Whether a day is a Saturday or a Sunday.
    pub fn weekend(d: Date) -> bool {
        matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday)
    }

    /// Whether the calendar lists a day as off.
    pub fn listed(&self, d: Date) -> bool {
        self.off.iter().any(|&(a, b)| a <= d && d <= b)
    }

    /// Whether a day counts towards a duration.
    pub fn works(&self, d: Date) -> bool {
        !(self.weekdays && Days::weekend(d)) && !self.listed(d)
    }

    /// Whether a day is shaded: a Saturday, a Sunday, or a day off.
    pub fn shaded(&self, d: Date) -> bool {
        Days::weekend(d) || self.listed(d)
    }

    /// The day an instant is in.
    pub fn date(&self, t: f64) -> Option<Date> {
        zoned(t, &self.zone).map(|z| z.date())
    }

    /// A day's first instant, in UTC seconds.
    pub fn start(&self, d: Date) -> f64 {
        start_of(d, &self.zone).unwrap_or(f64::NAN)
    }

    /// Whether an instant is its day's first.
    pub fn at_midnight(&self, t: f64) -> bool {
        self.date(t).is_some_and(|d| self.start(d) == t)
    }

    /// The first day that counts at or after `d`.
    pub fn next_working(&self, d: Date) -> Date {
        let mut at = d;
        for _ in 0..WALK {
            if self.works(at) {
                return at;
            }
            match at.tomorrow() {
                Ok(n) => at = n,
                Err(_) => break,
            }
        }
        d
    }

    /// The last day that counts at or before `d`.
    pub fn prev_working(&self, d: Date) -> Date {
        let mut at = d;
        for _ in 0..WALK {
            if self.works(at) {
                return at;
            }
            match at.yesterday() {
                Ok(n) => at = n,
                Err(_) => break,
            }
        }
        d
    }

    /// The day after the `n`th day that counts from `start` on: where a task of `n` days that
    /// starts on `start` stops. A start on a day off stands, and the count begins at the next
    /// day that counts. No days is `start` itself.
    pub fn add(&self, start: Date, n: i64) -> Date {
        if n <= 0 {
            return start;
        }
        if self.every_day() {
            return start.checked_add(n.days()).unwrap_or(start);
        }
        let (mut at, mut left) = (start, n);
        for _ in 0..WALK {
            if self.works(at) {
                left -= 1;
            }
            let Ok(next) = at.tomorrow() else { break };
            at = next;
            if left == 0 {
                return at;
            }
        }
        start.checked_add(n.days()).unwrap_or(start)
    }

    /// The `n`th day that counts back from `last`, `last` taken in: where a task of `n` days
    /// whose last day is `last` starts. Never a day off.
    pub fn back(&self, last: Date, n: i64) -> Date {
        if n <= 0 {
            return last;
        }
        if self.every_day() {
            return last.checked_sub((n - 1).days()).unwrap_or(last);
        }
        let (mut at, mut left) = (self.prev_working(last), n);
        for _ in 0..WALK {
            if self.works(at) {
                left -= 1;
                if left == 0 {
                    return at;
                }
            }
            let Ok(prev) = at.yesterday() else { break };
            at = prev;
        }
        last.checked_sub((n - 1).days()).unwrap_or(last)
    }

    /// How many days count from `a` up to `b` (not taken in). Every day, where none counts at
    /// all (a span of days off): a bar that long is that many days.
    pub fn count(&self, a: Date, b: Date) -> i64 {
        if b <= a {
            return 0;
        }
        let all = i64::from((b - a).get_days());
        if self.every_day() {
            return all;
        }
        let mut n = 0;
        let mut at = a;
        for _ in 0..WALK {
            if at >= b {
                break;
            }
            if self.works(at) {
                n += 1;
            }
            let Ok(next) = at.tomorrow() else { break };
            at = next;
        }
        if n == 0 { all } else { n }
    }

    /// The days from the one `lo` is in to the one `hi` is in, each with its first instant and
    /// the next day's: what the plot shades. None past `most` days.
    pub fn each(&self, lo: f64, hi: f64, most: usize) -> Option<Vec<(Date, f64, f64)>> {
        let (first, last) = (self.date(lo)?, self.date(hi)?);
        let n = i64::from((last - first).get_days());
        if n < 0 || n as usize > most {
            return None;
        }
        let mut out = Vec::with_capacity(n as usize + 1);
        let mut at = first;
        loop {
            let next = at.tomorrow().ok()?;
            out.push((at, self.start(at), self.start(next)));
            if at >= last {
                break;
            }
            at = next;
        }
        Some(out)
    }
}

/// A duration a task gives: `"3d"`, `"2w"`, `"36h"`, `"90m"`, `"1d12h"`, each a number and a
/// unit of `w`, `d`, `h`, `m` or `s`; a bare number is days. In seconds; None for anything
/// else, and for less than nothing.
pub fn parse_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(days) = s.parse::<f64>() {
        return (days.is_finite() && days >= 0.0).then_some(days * DAY);
    }
    let mut rest = s;
    let mut secs = 0.0;
    while !rest.is_empty() {
        let n = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let v: f64 = rest[..n].parse().ok()?;
        rest = rest[n..].trim_start();
        let u = rest
            .find(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
            .unwrap_or(rest.len());
        let unit = match &rest[..u] {
            "w" => 7.0 * DAY,
            "d" => DAY,
            "h" => 3600.0,
            "m" => 60.0,
            "s" => 1.0,
            _ => return None,
        };
        secs += v * unit;
        rest = rest[u..].trim_start();
    }
    (secs.is_finite() && secs < 1e12).then_some(secs)
}

/// So many of a thing: "1 day", "3 days".
fn count_of(n: i64, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// A count of days as the card writes it, a week being `per_week` of them (five where
/// weekends don't count): "5 days", "2 weeks 3 days", "1 week".
pub fn days_text(n: i64, per_week: i64) -> String {
    let (weeks, days) = (n / per_week, n % per_week);
    match (weeks, days) {
        (0, d) => count_of(d, "day"),
        (w, 0) => count_of(w, "week"),
        (w, d) => format!("{} {}", count_of(w, "week"), count_of(d, "day")),
    }
}

/// A span of seconds as the card writes it in a plan with times of day: "45 min", "36 h",
/// "1 h 30 min", and past two days "3 days 4 h".
pub fn span_text(secs: f64) -> String {
    let mins = (secs / 60.0).round() as i64;
    if mins < 60 {
        return format!("{mins} min");
    }
    let (h, m) = (mins / 60, mins % 60);
    if h < 48 {
        return match m {
            0 => format!("{h} h"),
            m => format!("{h} h {m} min"),
        };
    }
    match (h / 24, h % 24) {
        (d, 0) => count_of(d, "day"),
        (d, h) => format!("{} {h} h", count_of(d, "day")),
    }
}

/// A span of seconds as a task's `duration` reads it back: "5d", "36h", "1h30m", "45m".
pub fn span_spec(secs: f64) -> String {
    let mins = (secs / 60.0).round() as i64;
    let (d, h, m) = (mins / 1440, (mins / 60) % 24, mins % 60);
    match (d, h, m) {
        (d, 0, 0) => format!("{d}d"),
        (0, h, 0) => format!("{h}h"),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h{m}m"),
        (d, h, 0) => format!("{}h", d * 24 + h),
        (d, h, m) => format!("{}h{m}m", d * 24 + h),
    }
}

/// A day as the card writes it: "Mon 2 Nov", with its year when asked.
pub fn day_text(t: f64, year: bool, zone: &TimeZone) -> String {
    let Some(z) = zoned(t, zone) else {
        return "—".into();
    };
    let date = format!("{} {} {}", weekday(&z), z.day(), month(&z));
    if year {
        format!("{date} {}", z.year())
    } else {
        date
    }
}

/// A moment as the card writes it in a plan with times of day: "Mon 2 Nov 14:30".
pub fn moment_text(t: f64, year: bool, zone: &TimeZone) -> String {
    let Some(z) = zoned(t, zone) else {
        return "—".into();
    };
    format!("{} {}", day_text(t, year, zone), clock(&z))
}

/// A day in a few letters, as a milestone's date is written beside it: "15 Dec".
pub fn short_day(t: f64, zone: &TimeZone) -> String {
    match zoned(t, zone) {
        Some(z) => format!("{} {}", z.day(), month(&z)),
        None => "—".into(),
    }
}

/// The clock at a moment: "14:30".
pub fn clock_text(t: f64, zone: &TimeZone) -> String {
    zoned(t, zone).map_or_else(|| "—".into(), |z| clock(&z))
}

/// A day as a spec writes it: "2026-11-02".
pub fn iso_day(t: f64, zone: &TimeZone) -> String {
    match zoned(t, zone) {
        Some(z) => format!("{:04}-{:02}-{:02}", z.year(), z.month(), z.day()),
        None => String::new(),
    }
}

/// A moment as a spec writes it, in the zone: "2026-11-02T14:30", with seconds when it has
/// them.
pub fn iso_moment(t: f64, zone: &TimeZone) -> String {
    match zoned(t, zone) {
        Some(z) if z.second() != 0 => {
            format!("{}T{}:{:02}", iso_day(t, zone), clock(&z), z.second())
        }
        Some(z) => format!("{}T{}", iso_day(t, zone), clock(&z)),
        None => String::new(),
    }
}

/// The year an instant is in.
pub fn year_of(t: f64, zone: &TimeZone) -> Option<i16> {
    zoned(t, zone).map(|z| z.year())
}
