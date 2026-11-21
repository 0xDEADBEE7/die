//! Scales: data to pixels and back, their ticks, and how values are written.
//!
//! - [`linear`], [`log`], [`time`] and [`band`]: each kind's mapping and ticks;
//! - [`format`]: numbers, values and times as an axis and the card write them.
//!
//! Data is `f64` and places are `f32` logical px. A time is UTC seconds since the Unix epoch;
//! its ticks and labels fall on the calendar of the scale's zone, through `jiff::Zoned`, so a day
//! that daylight saving makes 23 or 25 hours long still starts at midnight.

pub mod band;
pub mod format;
pub mod linear;
pub mod log;
pub mod time;

pub use format::{
    NumberStyle, axis_scientific, axis_style, data_decimals, format_fixed, format_number,
    format_tick, format_time, format_time_full, format_time_point, format_value, format_with,
};

/// A mapping from a domain of data to a range of logical px.
#[derive(Clone, Debug, PartialEq)]
pub enum Scale {
    /// Proportional.
    Linear {
        /// The data's extent.
        domain: (f64, f64),
        /// Where its ends go.
        range: (f32, f32),
    },
    /// Logarithmic in a base.
    Log {
        /// The data's extent, above zero.
        domain: (f64, f64),
        /// Where its ends go.
        range: (f32, f32),
        /// The base, for ticks.
        base: f64,
    },
    /// Times in UTC seconds, ticked on the calendar in a zone.
    Time {
        /// The data's extent.
        domain: (f64, f64),
        /// Where its ends go.
        range: (f32, f32),
        /// The zone its ticks fall in.
        zone: jiff::tz::TimeZone,
    },
    /// One band a category, with paddings between and outside them (0 to 1 of a step).
    Band {
        /// The categories, in order.
        domain: Vec<Box<str>>,
        /// Where the first band starts and the last ends.
        range: (f32, f32),
        /// The padding between bands.
        inner: f32,
        /// The padding outside the first and last.
        outer: f32,
    },
}

impl Scale {
    /// Where a value goes. For a band scale, use [`Scale::band_of`]: there `v` is a place in
    /// bands, band `i` running from `i` to `i + 1` with half the inner padding on each side, so
    /// `i + 0.5` is its centre. On a log scale a value at or past zero has no place: NaN. A
    /// domain of one value maps everything to the middle of the range.
    pub fn map(&self, v: f64) -> f32 {
        match self {
            Scale::Linear { domain, range } | Scale::Time { domain, range, .. } => {
                linear::map(*domain, *range, v)
            }
            Scale::Log { domain, range, .. } => log::map(*domain, *range, v),
            Scale::Band {
                domain,
                range,
                inner,
                outer,
            } => band::Geometry::new(domain.len(), *range, *inner, *outer).map(v),
        }
    }

    /// The value at a place: the inverse of [`Scale::map`]. A range of one place gives the
    /// domain's start.
    pub fn invert(&self, px: f32) -> f64 {
        match self {
            Scale::Linear { domain, range } | Scale::Time { domain, range, .. } => {
                linear::invert(*domain, *range, px)
            }
            Scale::Log { domain, range, .. } => log::invert(*domain, *range, px),
            Scale::Band {
                domain,
                range,
                inner,
                outer,
            } => band::Geometry::new(domain.len(), *range, *inner, *outer).invert(px),
        }
    }

    /// A band's start and width, for a band scale: the start is the band's lower edge in px
    /// (the right one on a reversed range), the width never negative. None past the last band,
    /// and on other scales.
    pub fn band_of(&self, i: usize) -> Option<(f32, f32)> {
        match self {
            Scale::Band {
                domain,
                range,
                inner,
                outer,
            } => band::Geometry::new(domain.len(), *range, *inner, *outer).band(i),
            _ => None,
        }
    }

    /// The band at a place, for a band scale: its band and half the inner padding on each side,
    /// so a place between two bars finds the nearer. None in the outer padding, outside the
    /// range, and on other scales.
    pub fn band_at(&self, px: f32) -> Option<usize> {
        match self {
            Scale::Band {
                domain,
                range,
                inner,
                outer,
            } => band::Geometry::new(domain.len(), *range, *inner, *outer).at(px),
            _ => None,
        }
    }

    /// The domain's ends; a band scale's is (0, the number of bands).
    pub fn domain(&self) -> (f64, f64) {
        match self {
            Scale::Linear { domain, .. }
            | Scale::Log { domain, .. }
            | Scale::Time { domain, .. } => *domain,
            Scale::Band { domain, .. } => (0.0, domain.len() as f64),
        }
    }

    /// The range's ends.
    pub fn range(&self) -> (f32, f32) {
        match self {
            Scale::Linear { range, .. }
            | Scale::Log { range, .. }
            | Scale::Time { range, .. }
            | Scale::Band { range, .. } => *range,
        }
    }

    /// The same scale over another domain. A band scale's domain is its categories, so it comes
    /// back unchanged: make a band scale over the categories in view instead.
    pub fn with_domain(&self, d: (f64, f64)) -> Scale {
        let mut s = self.clone();
        match &mut s {
            Scale::Linear { domain, .. }
            | Scale::Log { domain, .. }
            | Scale::Time { domain, .. } => *domain = d,
            Scale::Band { .. } => {}
        }
        s
    }

    /// The same scale onto another range.
    pub fn with_range(&self, r: (f32, f32)) -> Scale {
        let mut s = self.clone();
        match &mut s {
            Scale::Linear { range, .. }
            | Scale::Log { range, .. }
            | Scale::Time { range, .. }
            | Scale::Band { range, .. } => *range = r,
        }
        s
    }

    /// The same scale with its domain widened to where about `count` ticks fall at its ends: a
    /// linear one by [`nice`], a log one to powers of its base, a time one to its ticks' calendar
    /// unit in its zone. A band scale comes back unchanged.
    pub fn nice(&self, count: usize) -> Scale {
        match self {
            Scale::Linear { domain, .. } => {
                let (lo, hi, _) = nice(domain.0, domain.1, count);
                self.with_domain((lo, hi))
            }
            Scale::Log { domain, base, .. } => self.with_domain(log::nice(*domain, *base)),
            Scale::Time { domain, zone, .. } => self.with_domain(time::nice(*domain, count, zone)),
            Scale::Band { .. } => self.clone(),
        }
    }
}

/// A tick: its value, its label, and whether it's a major one.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// Where it is, in data.
    pub value: f64,
    /// What it says.
    pub label: String,
    /// A major tick: a day's start on an axis of hours.
    pub major: bool,
}

/// About `count` ticks over the scale's domain (nice numbers; calendar units for time), in
/// ascending order of value, none for a `count` of 0.
///
/// - Linear: steps of 1, 2 or 5 times a power of ten, labelled in one [`NumberStyle`] for the
///   axis ([`axis_style`]: compact when it reaches 10,000), and all in scientific notation or
///   none ([`axis_scientific`]). None are major. Where the floats can't count in the step
///   (1e16 to 1e16 + 2 in tenths) there are fewer.
/// - Log: powers of the base, with 2 and 5 between them in base 10 when there's room, or every
///   second or third power when there isn't; linear ticks inside a single decade. The powers
///   are major; scientific all or none, as linear ones.
/// - Time: seconds, minutes, hours, days, Monday weeks, months, quarters or years in the zone,
///   each label as short as its place allows (":15", "10:25", "21 Nov", "Nov", "2026"); a tick
///   that starts a larger unit than the step's (a midnight among hours) is major and says so.
///   Under a second, linear ones, no finer than the floats there tell apart.
/// - Band: each category at its band's centre (`i + 0.5`), every nth when there are more than
///   `count`.
pub fn ticks(scale: &Scale, count: usize) -> Vec<Tick> {
    if count == 0 {
        return Vec::new();
    }
    match scale {
        Scale::Linear { domain, .. } => linear::ticks(*domain, count),
        Scale::Log { domain, base, .. } => log::ticks(*domain, *base, count),
        Scale::Time { domain, zone, .. } => time::ticks(*domain, count, zone),
        Scale::Band { domain, .. } => band::ticks(domain, count),
    }
}

/// A domain widened to nice numbers, and its step: the ends are multiples of a step of 1, 2 or 5
/// times a power of ten that gives about `count` intervals (`nice(0, 1100, 5)` is
/// `(0, 1200, 200)`). A domain of one value is widened by a tenth of it each way (by 1 at zero)
/// first; a reversed one stays reversed; a domain with no finite ends is `(0, 1)`.
pub fn nice(lo: f64, hi: f64, count: usize) -> (f64, f64, f64) {
    linear::nice(lo, hi, count)
}
