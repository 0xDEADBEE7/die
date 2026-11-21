//! Scales: data to pixels and back, their ticks, and how values are written.
//!
//! - [`linear`], [`log`], [`time`] and [`band`]: each kind's mapping and ticks;
//! - [`format`]: numbers, values and times as an axis and the card write them.

pub mod band;
pub mod format;
pub mod linear;
pub mod log;
pub mod time;

pub use format::{
    NumberStyle, format_number, format_time, format_time_full, format_value, format_with,
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
    /// Where a value goes. For a band scale, use [`Scale::band_of`].
    pub fn map(&self, _v: f64) -> f32 {
        todo!("P1-scale: Scale::map")
    }

    /// The value at a place.
    pub fn invert(&self, _px: f32) -> f64 {
        todo!("P1-scale: Scale::invert")
    }

    /// A band's start and width, for a band scale.
    pub fn band_of(&self, _i: usize) -> Option<(f32, f32)> {
        todo!("P1-scale: Scale::band_of")
    }

    /// The band at a place, for a band scale.
    pub fn band_at(&self, _px: f32) -> Option<usize> {
        todo!("P1-scale: Scale::band_at")
    }

    /// The domain's ends; a band scale's is (0, the number of bands).
    pub fn domain(&self) -> (f64, f64) {
        todo!("P1-scale: Scale::domain")
    }

    /// The same scale over another domain.
    pub fn with_domain(&self, _d: (f64, f64)) -> Scale {
        todo!("P1-scale: Scale::with_domain")
    }

    /// The same scale onto another range.
    pub fn with_range(&self, _r: (f32, f32)) -> Scale {
        todo!("P1-scale: Scale::with_range")
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

/// About `count` ticks over the scale's domain (nice numbers; calendar units for time).
pub fn ticks(_scale: &Scale, _count: usize) -> Vec<Tick> {
    todo!("P1-scale: ticks")
}

/// A domain widened to nice numbers, and its step.
pub fn nice(_lo: f64, _hi: f64, _count: usize) -> (f64, f64, f64) {
    todo!("P1-scale: nice")
}
