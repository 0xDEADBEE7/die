//! Scales from data to pixels, their ticks, and how numbers and times are written.
//!
//! One type holds the four kinds: linear, log, time and band. Ticks fall at d3's nice steps (1,
//! 2 or 5 times a power of ten), at powers of ten on a log scale, and on the local calendar on a
//! time scale (midnights, whole hours), so a day daylight saving makes 23 hours long still
//! starts at midnight. Numbers have `,` between thousands and a true minus sign; a missing
//! value is "—". Data is `f64`, places `f32` logical px, a time UTC seconds since the epoch.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};

/// A scale's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Linear,
    Log,
    Time,
    Band,
}

/// A mapping from data to logical px.
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    pub kind: Kind,
    /// The data's extent; a band scale's is (0, bands), band `i` centred on `i`.
    pub domain: (f64, f64),
    /// Where the domain's ends go.
    pub range: (f32, f32),
    /// A band scale's categories, in order.
    pub bands: Vec<String>,
}

/// A tick: its value, and its label.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub value: f64,
    pub label: String,
}

/// The share of a band left between bars.
const BAND_PAD: f32 = 0.2;
/// A day, in seconds; a calendar month and year, roughly, to choose a step by.
const DAY: f64 = 86_400.0;
const MONTH: f64 = 30.0 * DAY;
const YEAR: f64 = 365.0 * DAY;
/// The steps a time axis takes, in seconds.
const TIME_STEPS: [f64; 18] = [
    1.0,
    5.0,
    15.0,
    30.0,
    60.0,
    300.0,
    900.0,
    1800.0,
    3600.0,
    10_800.0,
    21_600.0,
    43_200.0,
    DAY,
    2.0 * DAY,
    7.0 * DAY,
    MONTH,
    3.0 * MONTH,
    YEAR,
];
const MINUS: char = '\u{2212}';
const MISSING: &str = "—";
const MONTHS: &str = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec";
const WEEKDAYS: &str = "Mon Tue Wed Thu Fri Sat Sun";

impl Scale {
    /// A proportional scale.
    pub fn linear(domain: (f64, f64), range: (f32, f32)) -> Scale {
        Scale::of(Kind::Linear, domain, range)
    }

    /// A logarithmic scale; a domain's end at or under zero moves just above it.
    pub fn log(domain: (f64, f64), range: (f32, f32)) -> Scale {
        let floor = |v: f64| if v > 0.0 { v } else { 1e-3 };
        Scale::of(Kind::Log, (floor(domain.0), floor(domain.1)), range)
    }

    /// A scale of times, ticked on the local calendar.
    pub fn time(domain: (f64, f64), range: (f32, f32)) -> Scale {
        Scale::of(Kind::Time, domain, range)
    }

    /// One band a category.
    pub fn band(bands: Vec<String>, range: (f32, f32)) -> Scale {
        let n = bands.len().max(1) as f64;
        let domain = (-0.5, n - 0.5);
        Scale {
            bands,
            ..Scale::of(Kind::Band, domain, range)
        }
    }

    fn of(kind: Kind, domain: (f64, f64), range: (f32, f32)) -> Scale {
        let bands = Vec::new();
        Scale {
            kind,
            domain,
            range,
            bands,
        }
    }

    /// The domain widened to round ends: a linear one to its outer ticks (and apart, if it was
    /// one value), a log one to powers of ten, a time one apart by a minute if it was one time.
    pub fn nice(mut self, count: usize) -> Scale {
        let (lo, hi) = self.domain;
        match self.kind {
            Kind::Linear => {
                let (lo, hi) = if lo == hi { apart(lo) } else { (lo, hi) };
                let s = step(lo, hi, count);
                self.domain = if s > 0.0 {
                    ((lo / s).floor() * s, (hi / s).ceil() * s)
                } else {
                    (lo, hi)
                };
            }
            Kind::Log => {
                let at = |e: f64| 10f64.powf(e);
                self.domain = (at(lo.log10().floor()), at(hi.log10().ceil()));
                if self.domain.0 == self.domain.1 {
                    self.domain.1 *= 10.0;
                }
            }
            Kind::Time if lo == hi => self.domain = (lo - 60.0, hi + 60.0),
            _ => {}
        }
        self
    }

    /// Where a value goes: a band scale's value is its band's number, and goes to its centre.
    /// On a log scale a value at or under zero has no place: NaN.
    pub fn map(&self, v: f64) -> f32 {
        let (d0, d1, v) = match self.kind {
            Kind::Log if v <= 0.0 => return f32::NAN,
            Kind::Log => (self.domain.0.log10(), self.domain.1.log10(), v.log10()),
            _ => (self.domain.0, self.domain.1, v),
        };
        let (r0, r1) = self.range;
        if d0 == d1 {
            return (r0 + r1) / 2.0;
        }
        r0 + ((v - d0) / (d1 - d0)) as f32 * (r1 - r0)
    }

    /// The value at a place: the inverse of [`Scale::map`].
    pub fn invert(&self, px: f32) -> f64 {
        let (r0, r1) = self.range;
        let t = if r0 == r1 {
            0.0
        } else {
            f64::from((px - r0) / (r1 - r0))
        };
        let (d0, d1) = self.domain;
        match self.kind {
            Kind::Log => 10f64.powf(d0.log10() + t * (d1.log10() - d0.log10())),
            _ => d0 + t * (d1 - d0),
        }
    }

    /// The band at a place, on a band scale.
    pub fn band_at(&self, px: f32) -> Option<usize> {
        let i = self.invert(px).round();
        (self.kind == Kind::Band && i >= 0.0 && (i as usize) < self.bands.len())
            .then_some(i as usize)
    }

    /// How wide a bar in a band is: a band less its padding; 0 on other scales.
    pub fn band_width(&self) -> f32 {
        let n = self.bands.len().max(1) as f32;
        match self.kind {
            Kind::Band => (self.range.1 - self.range.0).abs() / n * (1.0 - BAND_PAD),
            _ => 0.0,
        }
    }

    /// About `count` ticks inside the domain, labelled as the axis writes them; a band scale's
    /// are its bands.
    pub fn ticks(&self, count: usize) -> Vec<Tick> {
        let (lo, hi) = (
            self.domain.0.min(self.domain.1),
            self.domain.0.max(self.domain.1),
        );
        let count = count.max(1);
        match self.kind {
            Kind::Linear => linear_ticks(lo, hi, count),
            Kind::Log => log_ticks(lo, hi, count),
            Kind::Time => time_ticks(lo, hi, count),
            Kind::Band => {
                let tick = |(i, b): (usize, &String)| Tick {
                    value: i as f64,
                    label: b.clone(),
                };
                self.bands.iter().enumerate().map(tick).collect()
            }
        }
    }
}

/// A single value widened to a range: by a tenth of itself each way, or by one around zero.
fn apart(v: f64) -> (f64, f64) {
    let d = if v == 0.0 { 1.0 } else { v.abs() / 10.0 };
    (v - d, v + d)
}

/// d3's tick step for about `count` ticks over `lo..hi`: 1, 2, 5 or 10 times a power of ten,
/// rounded at the geometric midpoints (√2, √10, √50). 0 when there's no step.
fn step(lo: f64, hi: f64, count: usize) -> f64 {
    let raw = (hi - lo) / count as f64;
    if !(raw.is_finite() && raw > 0.0) {
        return 0.0;
    }
    let power = pow10(raw.log10().floor() as i32);
    let e = raw / power;
    let factor = if e >= 50f64.sqrt() {
        10.0
    } else if e >= 10f64.sqrt() {
        5.0
    } else if e >= 2f64.sqrt() {
        2.0
    } else {
        1.0
    };
    factor * power
}

/// Ten to the `k`, exact as the float nearest it.
fn pow10(k: i32) -> f64 {
    if k < 0 {
        1.0 / 10f64.powi(-k)
    } else {
        10f64.powi(k)
    }
}

/// The values at multiples of `s` from `lo` to `hi`; a step under one kept as its inverse, so
/// a tick is 3 / 10 and never 3 × 0.1.
fn multiples(lo: f64, hi: f64, s: f64) -> Vec<f64> {
    let slack = 1e-9;
    let (a, b) = (
        (lo / s - slack).ceil() as i64,
        (hi / s + slack).floor() as i64,
    );
    let inverse = (1.0 / s).round();
    let at = |i: i64| {
        if s < 1.0 {
            i as f64 / inverse
        } else {
            i as f64 * s
        }
    };
    (a..=b.min(a + 1000)).map(at).collect()
}

/// A linear axis's ticks: decimals from the step, compact (12.5K) once it reaches 10,000.
fn linear_ticks(lo: f64, hi: f64, count: usize) -> Vec<Tick> {
    let s = step(lo, hi, count);
    if s == 0.0 {
        let label = full(lo);
        return vec![Tick { value: lo, label }];
    }
    let reach = lo.abs().max(hi.abs());
    let label = |v: f64| {
        if reach >= 10_000.0 {
            compact(v, s)
        } else {
            fixed(v, decimals(s), true)
        }
    };
    let tick = |v: f64| Tick {
        value: v,
        label: label(v),
    };
    multiples(lo, hi, s).into_iter().map(tick).collect()
}

/// A log axis's ticks: powers of ten (every other one, or fewer, past twice `count`), with
/// their 2s and 5s over two decades or less; linear ticks inside one decade.
fn log_ticks(lo: f64, hi: f64, count: usize) -> Vec<Tick> {
    let (a, b) = (lo.log10().floor() as i32, hi.log10().ceil() as i32);
    let decades = (b - a).max(1) as usize;
    let every = decades.div_ceil(count * 2).max(1);
    let steps: &[f64] = if decades <= 2 {
        &[1.0, 2.0, 5.0]
    } else {
        &[1.0]
    };
    let mut ticks = Vec::new();
    for e in (a..=b).filter(|e| ((e - a) as usize).is_multiple_of(every)) {
        for m in steps {
            let v = m * pow10(e);
            if v >= lo * (1.0 - 1e-9) && v <= hi * (1.0 + 1e-9) {
                ticks.push(Tick {
                    value: v,
                    label: short(v),
                });
            }
        }
    }
    if ticks.len() < 2 {
        return linear_ticks(lo, hi, count);
    }
    ticks
}

/// A time axis's ticks on the local calendar: the first step of [`TIME_STEPS`] that gives no
/// more than `count`, each tick where the local clock or calendar starts it.
fn time_ticks(lo: f64, hi: f64, count: usize) -> Vec<Tick> {
    let span = hi - lo;
    let step = TIME_STEPS
        .into_iter()
        .find(|s| span / s <= count as f64)
        .unwrap_or(YEAR);
    let zone = TimeZone::system();
    let Some(z) = zoned(lo, &zone) else {
        return Vec::new();
    };
    let day = z.date();
    let mut at = if step < DAY {
        let since = i64::from(z.hour()) * 3600 + i64::from(z.minute()) * 60 + i64::from(z.second());
        let start = since / step as i64 * step as i64;
        day.to_datetime(Time::midnight())
            .checked_add(start.seconds())
    } else if step == 7.0 * DAY {
        let back = i64::from(day.weekday().to_monday_zero_offset());
        day.checked_sub(back.days())
            .map(|d| d.to_datetime(Time::midnight()))
    } else if step >= YEAR {
        Date::new(day.year(), 1, 1).map(|d| d.to_datetime(Time::midnight()))
    } else if step >= MONTH {
        let first = day.month() - (day.month() - 1) % (step / MONTH) as i8;
        Date::new(day.year(), first, 1).map(|d| d.to_datetime(Time::midnight()))
    } else {
        Ok(day.to_datetime(Time::midnight()))
    };
    let mut ticks = Vec::new();
    while let Ok(dt) = at {
        let Some(v) = civil_seconds(dt, &zone) else {
            break;
        };
        if v > hi || ticks.len() > 500 {
            break;
        }
        if v >= lo {
            ticks.push(Tick {
                value: v,
                label: time_label(v, step),
            });
        }
        at = advance(dt, step);
    }
    ticks
}

/// A civil time one step on.
fn advance(dt: DateTime, step: f64) -> Result<DateTime, jiff::Error> {
    if step >= YEAR {
        dt.checked_add(1.year())
    } else if step >= MONTH {
        dt.checked_add(((step / MONTH) as i64).months())
    } else if step >= DAY {
        dt.checked_add(((step / DAY) as i64).days())
    } else {
        dt.checked_add((step as i64).seconds())
    }
}

/// A civil time in `zone`, as UTC seconds.
fn civil_seconds(dt: DateTime, zone: &TimeZone) -> Option<f64> {
    zone.to_timestamp(dt)
        .ok()
        .map(|t| t.as_millisecond() as f64 / 1000.0)
}

/// A time in `zone`.
fn zoned(v: f64, zone: &TimeZone) -> Option<Zoned> {
    if !v.is_finite() {
        return None;
    }
    let ts = Timestamp::from_millisecond((v * 1000.0).round() as i64).ok()?;
    Some(ts.to_zoned(zone.clone()))
}

/// A time's axis label at a step of `step` seconds, in the local zone: "2026" by years, "Nov"
/// by months, "21 Nov" by days or at midnight, "10:25" by minutes and hours, "10:25:15" finer.
pub fn time_label(v: f64, step: f64) -> String {
    let Some(z) = zoned(v, &TimeZone::system()) else {
        return MISSING.to_string();
    };
    let midnight = (z.hour(), z.minute(), z.second()) == (0, 0, 0);
    if step >= YEAR {
        z.year().to_string()
    } else if step >= MONTH {
        month(&z).to_string()
    } else if step >= DAY || midnight {
        format!("{} {}", z.day(), month(&z))
    } else if step < 60.0 {
        format!("{:02}:{:02}:{:02}", z.hour(), z.minute(), z.second())
    } else {
        format!("{:02}:{:02}", z.hour(), z.minute())
    }
}

/// A time in full for the card, in the local zone: "10:25 · Fri 21 Nov", or at midnight (a
/// date) "Fri 21 Nov".
pub fn time_full(v: f64) -> String {
    let Some(z) = zoned(v, &TimeZone::system()) else {
        return MISSING.to_string();
    };
    let weekday = z.weekday().to_monday_zero_offset() as usize;
    let weekday = WEEKDAYS.split(' ').nth(weekday).unwrap_or("");
    let day = format!("{weekday} {} {}", z.day(), month(&z));
    match (z.hour(), z.minute()) {
        (0, 0) => day,
        (h, m) => format!("{h:02}:{m:02} · {day}"),
    }
}

/// "Nov".
fn month(z: &Zoned) -> &'static str {
    MONTHS
        .split(' ')
        .nth((z.month() - 1) as usize)
        .unwrap_or("")
}

/// A value in full, for the card: six significant digits, never fewer than its whole part,
/// thousands grouped, trailing zeros dropped: 3,412, 0.41, 1,234.57.
pub fn full(v: f64) -> String {
    if !v.is_finite() || v == 0.0 {
        return special(v);
    }
    let whole = v.abs().log10().floor() as i32 + 1;
    trim(fixed(v, (6 - whole).clamp(0, 12) as usize, true))
}

/// A value abbreviated for a tile or a log axis: compact to three significant figures from
/// 10,000 (12.9K, 1.28M), whole from 100 (3,412), one decimal from 10 (12.3), two from 0.1
/// (0.41), else two significant figures (0.0041); trailing zeros dropped.
pub fn short(v: f64) -> String {
    let a = v.abs();
    if !v.is_finite() || v == 0.0 {
        return special(v);
    }
    let d = match a {
        a if a >= 10_000.0 => return compact(v, f64::NAN),
        a if a >= 100.0 => 0,
        a if a >= 10.0 => 1,
        a if a >= 0.1 => 2,
        a => (1 - a.log10().floor() as i32).clamp(0, 12) as usize,
    };
    trim(fixed(v, d, true))
}

/// A value in a spec's format, d3's mini-language cut to `[,][.N][~][f|d|%]`: `,` groups
/// thousands, `.N` gives N decimals, `~` drops trailing zeros, `d` rounds to a whole number and
/// `%` writes a hundred times it with a percent sign. Any other format writes it in [`full`].
pub fn with_format(v: f64, format: &str) -> String {
    if !v.is_finite() {
        return special(v);
    }
    let mut s = format.trim();
    let group = s.starts_with(',');
    s = s.trim_start_matches(',');
    let mut places = None;
    if let Some(rest) = s.strip_prefix('.') {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        places = rest[..digits].parse::<usize>().ok().map(|p| p.min(12));
        s = &rest[digits..];
    }
    let tidy = s.starts_with('~');
    s = s.trim_start_matches('~');
    let (v, suffix) = match s {
        "" | "f" => (v, ""),
        "d" => return fixed(v.round(), 0, group),
        "%" => (v * 100.0, "%"),
        _ => return full(v),
    };
    let body = match places {
        Some(p) => fixed(v, p, group),
        None => full(v),
    };
    let body = if tidy { trim(body) } else { body };
    body + suffix
}

/// A value and its unit: "182 ms", "99.95 s".
pub fn with_unit(value: &str, unit: &str) -> String {
    if unit.is_empty() {
        value.to_string()
    } else {
        format!("{value} {unit}")
    }
}

/// The decimals that write every multiple of a 1, 2 or 5 step: 0.25 none of those, 0.5 one.
fn decimals(step: f64) -> usize {
    if step.is_finite() && step > 0.0 {
        (-step.log10().floor()).clamp(0.0, 12.0) as usize
    } else {
        2
    }
}

/// With a K, M, B or T suffix: decimals from `step` at the value's thousand, else three
/// significant figures; trailing zeros dropped.
fn compact(v: f64, step: f64) -> String {
    let k = (1..=4)
        .rev()
        .find(|&k| v.abs() >= pow10(k * 3))
        .unwrap_or(0);
    let unit = pow10(k * 3);
    let scaled = v / unit;
    let d = if step.is_finite() && step > 0.0 {
        decimals(step / unit)
    } else if scaled == 0.0 {
        0
    } else {
        (2 - scaled.abs().log10().floor() as i32).clamp(0, 12) as usize
    };
    let suffix = ["", "K", "M", "B", "T"][k as usize];
    trim(fixed(scaled, d, true)) + suffix
}

/// What a missing, infinite or zero value says.
fn special(v: f64) -> String {
    match v {
        v if v.is_nan() => MISSING.to_string(),
        v if v == f64::INFINITY => "∞".to_string(),
        v if v == f64::NEG_INFINITY => format!("{MINUS}∞"),
        _ => "0".to_string(),
    }
}

/// `v` with `d` decimals, thousands grouped if asked, and a minus sign only if a digit isn't
/// zero.
fn fixed(v: f64, d: usize, group: bool) -> String {
    let body = format!("{:.*}", d, v.abs());
    let (whole, frac) = body.split_once('.').unwrap_or((&body, ""));
    let mut out = String::new();
    if v < 0.0 && body.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
        out.push(MINUS);
    }
    for (i, c) in whole.chars().enumerate() {
        if group && i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

/// Trailing zeros after a decimal point dropped, and the point with nothing after it.
fn trim(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(s: &Scale, count: usize) -> Vec<String> {
        s.ticks(count).into_iter().map(|t| t.label).collect()
    }

    #[test]
    fn linear_ticks_are_nice_and_compact_from_ten_thousand() {
        let s = Scale::linear((0.0, 1.0), (0.0, 100.0));
        assert_eq!(labels(&s, 5), ["0.0", "0.2", "0.4", "0.6", "0.8", "1.0"]);
        let s = Scale::linear((-3.0, 47.0), (0.0, 100.0)).nice(5);
        assert_eq!(s.domain, (-10.0, 50.0));
        let s = Scale::linear((0.0, 25_000.0), (0.0, 100.0));
        assert_eq!(labels(&s, 5), ["0", "5K", "10K", "15K", "20K", "25K"]);
        assert_eq!(s.map(12_500.0), 50.0);
        assert_eq!(s.invert(50.0), 12_500.0);
    }

    #[test]
    fn log_and_band_scales_place_and_tick() {
        let s = Scale::log((1.0, 1e6), (0.0, 60.0));
        assert_eq!(labels(&s, 2), ["1", "100", "10K", "1M"]);
        assert_eq!(labels(&s, 3).len(), 7);
        assert_eq!(s.map(1000.0), 30.0);
        assert!(s.map(0.0).is_nan());
        let s = Scale::band(vec!["a".into(), "b".into()], (0.0, 100.0));
        assert_eq!((s.map(0.0), s.map(1.0), s.band_width()), (25.0, 75.0, 40.0));
        assert_eq!(
            (s.band_at(10.0), s.band_at(60.0), s.band_at(101.0)),
            (Some(0), Some(1), None)
        );
    }

    #[test]
    fn time_ticks_fall_on_the_local_calendar() {
        let zone = TimeZone::system();
        let at = |s: &str| civil_seconds(s.parse().unwrap(), &zone).unwrap();
        let s = Scale::time(
            (at("2026-11-15T10:20"), at("2026-11-22T09:00")),
            (0.0, 700.0),
        );
        assert_eq!(
            labels(&s, 8),
            [
                "16 Nov", "17 Nov", "18 Nov", "19 Nov", "20 Nov", "21 Nov", "22 Nov"
            ]
        );
        let s = Scale::time(
            (at("2026-11-21T22:10"), at("2026-11-22T04:00")),
            (0.0, 700.0),
        );
        assert_eq!(
            labels(&s, 6),
            ["23:00", "22 Nov", "01:00", "02:00", "03:00", "04:00"]
        );
        assert_eq!(time_full(at("2026-11-21T10:25")), "10:25 · Sat 21 Nov");
        assert_eq!(time_full(at("2026-11-21T00:00")), "Sat 21 Nov");
    }

    #[test]
    fn numbers_are_written_as_asked() {
        assert_eq!(with_format(1234.567, ".2f"), "1234.57");
        assert_eq!(with_format(1234.567, ",.1f"), "1,234.6");
        assert_eq!(with_format(0.125, ".1%"), "12.5%");
        assert_eq!(with_format(-2.5, ".2f"), "\u{2212}2.50");
        assert_eq!(
            (full(1234.5678), full(0.1 + 0.2), full(f64::NAN)),
            ("1,234.57".into(), "0.3".into(), "—".into())
        );
        assert_eq!(
            [
                short(12_900.0),
                short(1_284_000.0),
                short(182.4),
                short(0.0041)
            ],
            ["12.9K", "1.28M", "182", "0.0041"]
        );
        assert_eq!(with_unit(&with_format(99.95, ".2f"), "s"), "99.95 s");
    }
}
