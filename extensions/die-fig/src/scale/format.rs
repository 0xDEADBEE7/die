//! How values are written: an axis's numbers in one style, the card's in full, times at the
//! precision of their step, and a spec's explicit formats.
//!
//! Numbers use `,` between thousands, `.` before decimals and a true minus sign (U+2212, as
//! d3-format writes it), so a column of figures lines up. A missing value (NaN) is "—", an
//! infinite one "∞". Times are on a 24-hour clock with day-first dates in English ("10:25",
//! "21 Nov", "10:25 · Fri 21 Nov"), in the zone they're given.

use jiff::tz::TimeZone;
use jiff::{Zoned, fmt::strtime};

use super::linear::pow10;
use super::time::{Level, level_for_step, level_of, zoned};

/// The minus sign.
const MINUS: char = '\u{2212}';
/// What a missing value says.
const MISSING: &str = "—";
/// Where an axis turns compact, and the most decimals a compact label may have.
const COMPACT_FROM: f64 = 10_000.0;
const COMPACT_DECIMALS: usize = 2;
/// The card's precision: significant digits, and the most decimals a column is given.
const SIGNIFICANT: i32 = 6;
const MOST_DECIMALS: usize = 6;
/// The compact suffixes past a thousand, in thousands.
const SUFFIXES: [&str; 4] = ["K", "M", "B", "T"];
/// SI's prefixes from 10⁻²⁴ to 10²⁴, by thousands.
const SI: [&str; 17] = [
    "y", "z", "a", "f", "p", "n", "µ", "m", "", "k", "M", "G", "T", "P", "E", "Z", "Y",
];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// How an axis writes its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberStyle {
    /// With thousands separators: 1,284.
    Full,
    /// With a K, M, B or T suffix: 12.9K, 4.2M.
    Compact,
}

/// The one style an axis writes its numbers in: Compact when it reaches 10,000 (`reach` is
/// its largest magnitude) and its labels then need at most two decimals at `step` (so an axis
/// zoomed in to 12,340–12,350 stays in full), else Full. A `step` that isn't a positive number
/// skips the second test. For a single figure, `reach` is its own magnitude: 1,284 in full and
/// 12.9K compact.
pub fn axis_style(reach: f64, step: f64) -> NumberStyle {
    if reach.is_nan() || reach.abs() < COMPACT_FROM {
        return NumberStyle::Full;
    }
    let ok = match step_decimals(step / pow10(thousands(reach.abs()) * 3)) {
        Some(d) => d <= COMPACT_DECIMALS,
        None => true,
    };
    if ok {
        NumberStyle::Compact
    } else {
        NumberStyle::Full
    }
}

/// 1,284 / 12.9K / 4.2M; one style per axis: Compact when the axis reaches 10,000.
///
/// Decimals come from `step`, the distance between neighbours on the axis: a step of 0.25
/// writes two, of 2,500 compact one (2.5K). Full keeps them all, so a column's points line up
/// ("0.0", "0.5", "1.0"); Compact drops trailing zeros, as the design's ticks do ("2.5K", "5K").
/// Compact writes under a thousand as it is, then K, M, B and T. A `step` that isn't a positive
/// number gives a figure on its own: in full as [`format_value`], compact to three significant
/// digits (12.9K, 1.28M). From 10¹⁵, and under 10⁻⁶, either style is scientific (1.5e−7).
pub fn format_number(v: f64, step: f64, style: NumberStyle) -> String {
    if let Some(s) = special(v) {
        return s;
    }
    if far(v.abs()) {
        return scientific(v);
    }
    match style {
        NumberStyle::Full => match step_decimals(step) {
            Some(d) => fixed(v, d, true),
            None => shortest(v, true),
        },
        NumberStyle::Compact => compact(v, step),
    }
}

/// A value in full for the card: thousands separators, decimals from the data's precision.
///
/// The value's own precision, to six significant digits and never fewer than its whole part:
/// 3,412, 0.41, 1,234.57, 0.1 + 0.2 as 0.3. Past 10¹⁵ and under 10⁻⁶ it's scientific
/// (1.5e−7).
pub fn format_value(v: f64) -> String {
    shortest(v, true)
}

/// How many decimals a column needs so that each of its values shows its own precision as
/// [`format_value`] would, at most six: for a card whose rows should line up. NaN and infinite
/// values don't count.
pub fn data_decimals(values: &[f64]) -> usize {
    values
        .iter()
        .filter(|v| v.is_finite() && **v != 0.0)
        .map(|&v| {
            let a = v.abs();
            if far(a) {
                return 0;
            }
            let s = trim(fixed(v, own_decimals(a), false));
            s.split_once('.').map_or(0, |(_, f)| f.len())
        })
        .max()
        .unwrap_or(0)
        .min(MOST_DECIMALS)
}

/// `v` with exactly `decimals` decimals and thousands separators (at most 15).
pub fn format_fixed(v: f64, decimals: usize) -> String {
    special(v).unwrap_or_else(|| fixed(v, decimals.min(15), true))
}

/// A time at the precision of `step` seconds ("21 Nov", "10:25", ":15").
///
/// An axis label: as short as its neighbours allow, by the coarsest field the time starts and
/// no finer than `step` asks: ".250" past a second, ":15" on a second, "10:25" on a minute or
/// an hour, "21 Nov" on a day's start, "Nov" on a month's, "2026" on a year's. So 10:00 on an
/// axis of quarter hours says "10:00", and midnight "22 Nov". Dates are day first, as the card
/// writes them. A `step` that isn't a positive number lets the time choose alone.
pub fn format_time(v: f64, step: f64, zone: &jiff::tz::TimeZone) -> String {
    let Some(z) = zoned(v, zone) else {
        return special(v).unwrap_or_else(|| MISSING.to_string());
    };
    let level = level_of(&z).max(level_for_step(step));
    match level {
        Level::SubSecond => format!(".{}", fraction(&z, step)),
        Level::Second => format!(":{:02}", z.second()),
        Level::Clock => clock(&z),
        Level::Day => format!("{} {}", z.day(), month(&z)),
        Level::Month => month(&z).to_string(),
        Level::Year => z.year().to_string(),
    }
}

/// A time at exactly the precision of `step` seconds, every field down to it written: for the
/// chip on a time axis. "10:25:15.250", "10:25:15", "10:25", "21 Nov", "Nov 2026" or "2026".
/// A `step` that isn't a positive number takes the precision from the time itself.
pub fn format_time_point(v: f64, step: f64, zone: &TimeZone) -> String {
    let Some(z) = zoned(v, zone) else {
        return special(v).unwrap_or_else(|| MISSING.to_string());
    };
    match precision(&z, step) {
        Level::SubSecond => format!("{}:{:02}.{}", clock(&z), z.second(), fraction(&z, step)),
        Level::Second => format!("{}:{:02}", clock(&z), z.second()),
        Level::Clock => clock(&z),
        Level::Day => format!("{} {}", z.day(), month(&z)),
        Level::Month => format!("{} {}", month(&z), z.year()),
        Level::Year => z.year().to_string(),
    }
}

/// A time's full form for the card ("10:25 · Fri 21 Nov").
///
/// The clock at the precision of `step`, then the weekday and date: "10:25:15 · Fri 21 Nov".
/// A step of a day or more drops the clock and adds the year ("Fri 21 Nov 2026"), of a month
/// "Nov 2026", of a year "2026". A `step` that isn't a positive number takes the precision from
/// the time itself.
pub fn format_time_full(v: f64, step: f64, zone: &jiff::tz::TimeZone) -> String {
    let Some(z) = zoned(v, zone) else {
        return special(v).unwrap_or_else(|| MISSING.to_string());
    };
    let date = || format!("{} {} {}", weekday(&z), z.day(), month(&z));
    match precision(&z, step) {
        Level::SubSecond => {
            format!(
                "{}:{:02}.{} · {}",
                clock(&z),
                z.second(),
                fraction(&z, step),
                date()
            )
        }
        Level::Second => format!("{}:{:02} · {}", clock(&z), z.second(), date()),
        Level::Clock => format!("{} · {}", clock(&z), date()),
        Level::Day => format!("{} {}", date(), z.year()),
        Level::Month => format!("{} {}", month(&z), z.year()),
        Level::Year => z.year().to_string(),
    }
}

/// An explicit format from a spec: "%H:%M" (strtime) or ",.2f"-like numeric (",", ".Nf", "%",
/// "s" SI).
///
/// A format with a `%` anywhere but at its end is jiff's strtime, applied to the time in `zone`.
/// Otherwise it's d3's numeric mini-language, cut down to `[,][.N][~][type]`: `,` groups
/// thousands, `.N` gives N decimals (N significant digits for `s`), `~` drops trailing zeros,
/// and the type is none or `f` (fixed), `d` (a whole number), `%` (times 100, with a percent
/// sign) or `s` (an SI prefix: 1.5k, 250µ). With no `.N` the value keeps its own precision, as
/// [`format_value`] (three significant digits for `s`). A format that is neither is ignored and
/// the value written as [`format_value`] would.
pub fn format_with(v: f64, fmt: &str, zone: &jiff::tz::TimeZone) -> String {
    let timeish = fmt
        .char_indices()
        .any(|(i, c)| c == '%' && i + 1 < fmt.len());
    if timeish {
        let Some(z) = zoned(v, zone) else {
            return special(v).unwrap_or_else(|| MISSING.to_string());
        };
        return strtime::format(fmt, &z).unwrap_or_else(|_| format_time_full(v, f64::NAN, zone));
    }
    if let Some(s) = special(v) {
        return s;
    }
    match Numeric::parse(fmt) {
        Some(n) if !fmt.trim().is_empty() => n.apply(v),
        _ => format_value(v),
    }
}

/// A parsed numeric format.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Numeric {
    group: bool,
    precision: Option<usize>,
    trim: bool,
    kind: Kind,
}

/// A numeric format's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fixed,
    Whole,
    Percent,
    Si,
}

impl Numeric {
    /// `[,][.N][~][type]`, or None.
    fn parse(fmt: &str) -> Option<Numeric> {
        let mut s = fmt.trim();
        let group = s.starts_with(',');
        if group {
            s = &s[1..];
        }
        let mut precision = None;
        if let Some(rest) = s.strip_prefix('.') {
            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            precision = Some(rest[..digits].parse::<usize>().ok()?.min(20));
            s = &rest[digits..];
        }
        let trim = s.starts_with('~');
        if trim {
            s = &s[1..];
        }
        let kind = match s {
            "" | "f" => Kind::Fixed,
            "d" => Kind::Whole,
            "%" => Kind::Percent,
            "s" => Kind::Si,
            _ => return None,
        };
        Some(Numeric {
            group,
            precision,
            trim,
            kind,
        })
    }

    /// `v` (finite) in this format.
    fn apply(self, v: f64) -> String {
        let fixed_or_own = |w: f64| match self.precision {
            // d3's bound: past 10²¹ even a fixed format is scientific.
            Some(_) if w.abs() >= 1e21 => scientific(w),
            Some(p) => {
                let s = fixed(w, p.min(15), self.group);
                if self.trim { trim(s) } else { s }
            }
            None => shortest(w, self.group),
        };
        match self.kind {
            Kind::Fixed => fixed_or_own(v),
            Kind::Whole => fixed(v.round(), 0, self.group),
            Kind::Percent => fixed_or_own(v * 100.0) + "%",
            Kind::Si => si(v, self.precision, self.trim || self.precision.is_none()),
        }
    }
}

/// NaN, infinities: what they say. None for a finite value.
fn special(v: f64) -> Option<String> {
    if v.is_nan() {
        Some(MISSING.to_string())
    } else if v.is_infinite() {
        Some(if v > 0.0 {
            "∞".to_string()
        } else {
            format!("{MINUS}∞")
        })
    } else {
        None
    }
}

/// `v` with `d` decimals, thousands grouped if asked, and a minus only if a digit isn't zero.
fn fixed(v: f64, d: usize, group: bool) -> String {
    let body = format!("{:.*}", d, v.abs());
    let (whole, frac) = match body.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (body.as_str(), None),
    };
    let mut out = String::with_capacity(body.len() + body.len() / 3 + 3);
    if v < 0.0 && body.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
        out.push(MINUS);
    }
    if group {
        let n = whole.len();
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (n - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
    } else {
        out.push_str(whole);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// Trailing zeros after a decimal point dropped, and the point if nothing follows it.
fn trim(s: String) -> String {
    if !s.contains('.') {
        return s;
    }
    let t = s.trim_end_matches('0').trim_end_matches('.');
    t.to_string()
}

/// The decimals that write every multiple of `step` exactly: 0.25 two, 2,500 none. A step that
/// no decimal writes exactly (a third) gets two significant digits. None when `step` isn't a
/// positive number.
fn step_decimals(step: f64) -> Option<usize> {
    if !(step.is_finite() && step > 0.0) {
        return None;
    }
    for d in 0..=15 {
        let scaled = step * pow10(d);
        if (scaled - scaled.round()).abs() <= scaled * 1e-9 {
            return Some(d as usize);
        }
    }
    Some((1 - step.log10().floor() as i32).clamp(0, 15) as usize)
}

/// Decimals for `sig` significant digits of `a` (above zero).
fn sig_decimals(a: f64, sig: i32) -> usize {
    (sig - 1 - a.log10().floor() as i32).clamp(0, 15) as usize
}

/// The card's decimals for `a` (above zero): six significant digits, never fewer than none.
fn own_decimals(a: f64) -> usize {
    let whole = a.log10().floor() as i32 + 1;
    (SIGNIFICANT - whole).clamp(0, 12) as usize
}

/// [`format_value`], grouped or not.
fn shortest(v: f64, group: bool) -> String {
    if let Some(s) = special(v) {
        return s;
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let a = v.abs();
    if far(a) {
        return scientific(v);
    }
    trim(fixed(v, own_decimals(a), group))
}

/// Whether a magnitude is past what plain figures write well: from 10¹⁵, or under 10⁻⁶ but not
/// zero.
fn far(a: f64) -> bool {
    a != 0.0 && !(1e-6..1e15).contains(&a)
}

/// `v` as 1.5e−7: six significant digits, trailing zeros dropped.
fn scientific(v: f64) -> String {
    let body = format!("{:.5e}", v.abs());
    let (m, e) = body.split_once('e').unwrap_or((body.as_str(), "0"));
    let m = trim(m.to_string());
    let e = e.replace('-', &MINUS.to_string());
    let sign = if v < 0.0 {
        MINUS.to_string()
    } else {
        String::new()
    };
    format!("{sign}{m}e{e}")
}

/// Which thousand `a` is in: 0 under a thousand, 1 for K, up to 4 for T.
fn thousands(a: f64) -> i32 {
    (1..=4).rev().find(|&k| a >= pow10(k * 3)).unwrap_or(0)
}

/// `a` rounded to `d` decimals.
fn rounded(a: f64, d: usize) -> f64 {
    let p = pow10(d as i32);
    (a * p).round() / p
}

/// [`NumberStyle::Compact`].
fn compact(v: f64, step: f64) -> String {
    let mut k = thousands(v.abs());
    loop {
        let unit = pow10(k * 3);
        let scaled = v / unit;
        let d = step_decimals(step / unit).unwrap_or_else(|| {
            if scaled == 0.0 {
                0
            } else {
                sig_decimals(scaled.abs(), 3)
            }
        });
        // 999.95 to three digits is 1000: that's 1K, as 999,950 is 1M.
        if k < 4 && rounded(scaled.abs(), d) >= 1000.0 {
            k += 1;
            continue;
        }
        let s = trim(fixed(scaled, d, k > 0));
        return if k == 0 {
            s
        } else {
            s + SUFFIXES[(k - 1) as usize]
        };
    }
}

/// [`Kind::Si`]: `p` significant digits (3 when not given) and SI's prefix.
fn si(v: f64, p: Option<usize>, trimmed: bool) -> String {
    let p = p.unwrap_or(3).clamp(1, 17) as i32;
    let finish = |s: String| if trimmed { trim(s) } else { s };
    if v == 0.0 {
        return finish(fixed(0.0, (p - 1) as usize, false));
    }
    let a = v.abs();
    if !(1e-24..1e27).contains(&a) {
        return scientific(v);
    }
    let mut e = ((a.log10() / 3.0).floor() as i32).clamp(-8, 8);
    let mut bumped = false;
    loop {
        let scaled = a / pow10(e * 3);
        // Rounded up into the next prefix, it's 1 of it: 999.9 to three digits is 1.00k.
        let lead = if bumped {
            0
        } else {
            scaled.log10().floor() as i32
        };
        let d = (p - 1 - lead).max(0) as usize;
        if e < 8 && rounded(scaled, d) >= 1000.0 {
            e += 1;
            bumped = true;
            continue;
        }
        let signed = if v < 0.0 { -scaled } else { scaled };
        return finish(fixed(signed, d, false)) + SI[(e + 8) as usize];
    }
}

/// How finely to write `z` for `step`: the step's precision, or the time's own without one.
fn precision(z: &Zoned, step: f64) -> Level {
    if step.is_finite() && step > 0.0 {
        level_for_step(step)
    } else {
        level_of(z)
    }
}

/// "10:25".
fn clock(z: &Zoned) -> String {
    format!("{:02}:{:02}", z.hour(), z.minute())
}

/// "Nov".
fn month(z: &Zoned) -> &'static str {
    MONTHS[(z.month() - 1).clamp(0, 11) as usize]
}

/// "Fri".
fn weekday(z: &Zoned) -> &'static str {
    WEEKDAYS[z.weekday().to_monday_zero_offset().clamp(0, 6) as usize]
}

/// The fraction of a second, in milli-, micro- or nanoseconds as `step` (or the time) needs.
fn fraction(z: &Zoned, step: f64) -> String {
    let ns = z.subsec_nanosecond().unsigned_abs();
    let digits = if step.is_finite() && step > 0.0 && step < 1.0 {
        if step >= 1e-3 {
            3
        } else if step >= 1e-6 {
            6
        } else {
            9
        }
    } else if ns.is_multiple_of(1_000_000) {
        3
    } else if ns.is_multiple_of(1_000) {
        6
    } else {
        9
    };
    let shown = ns / 10u32.pow(9 - digits);
    format!("{shown:0width$}", width = digits as usize)
}
