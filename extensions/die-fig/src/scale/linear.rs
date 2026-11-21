//! The linear scale: proportional mapping, and ticks at nice steps of 1, 2 or 5 times a power of
//! ten.
//!
//! The steps are d3's: for about `count` ticks the raw step is rounded to 1, 2, 5 or 10 times a
//! power of ten at the geometric midpoints (√2, √10, √50). A step under 1 is kept as its inverse,
//! so a tick is `i / 10` and never `i × 0.1`: 0.3 comes out as 0.3, not 0.30000000000000004.

use super::Tick;
use super::format::{axis_scientific, axis_style, format_number, format_tick};

/// A tick step, kept exact: a multiple when it's 1 or more, a divisor when it's less.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Inc {
    /// Ticks at `i × step`.
    Mul(f64),
    /// Ticks at `i / inverse`.
    Div(f64),
}

impl Inc {
    /// The `i`th multiple of the step.
    pub(crate) fn at(self, i: f64) -> f64 {
        match self {
            Inc::Mul(s) => i * s,
            Inc::Div(d) => i / d,
        }
    }

    /// The step itself.
    pub(crate) fn step(self) -> f64 {
        match self {
            Inc::Mul(s) => s,
            Inc::Div(d) => 1.0 / d,
        }
    }

    /// `v` rounded down to a multiple of the step.
    fn floor(self, v: f64) -> f64 {
        match self {
            Inc::Mul(s) => (v / s).floor() * s,
            Inc::Div(d) => (v * d).floor() / d,
        }
    }

    /// `v` rounded up to a multiple of the step.
    fn ceil(self, v: f64) -> f64 {
        match self {
            Inc::Mul(s) => (v / s).ceil() * s,
            Inc::Div(d) => (v * d).ceil() / d,
        }
    }
}

/// √50, √10 and √2: where a raw step rounds up to the next of 10, 5 and 2.
const E10: f64 = 7.071_067_811_865_476;
const E5: f64 = 3.162_277_660_168_379_5;
const E2: f64 = std::f64::consts::SQRT_2;

/// More ticks than this from one call means the step underflowed; none is safer.
const MOST: f64 = 1.0e6;

/// 2⁵³: from here a float can't count by one (`i + 1.0 == i`), so a tick's index is no longer
/// exact and a step finer than the floats around it would run its ticks together.
const EXACT: f64 = 9_007_199_254_740_992.0;

/// Ten to the `k`, exact while ten to the `|k|` is (to 22).
pub(crate) fn pow10(k: i32) -> f64 {
    if (0..=22).contains(&k) {
        (0..k).fold(1.0, |p, _| p * 10.0)
    } else if (-22..0).contains(&k) {
        1.0 / pow10(-k)
    } else {
        10f64.powi(k)
    }
}

/// The first and last tick's index and the step, for about `count` ticks over `lo ..= hi`
/// (`lo < hi`, both finite). None when the step can't be written as a float. Where about
/// `count` would step finer than the floats there can count (2⁵³ steps from zero: 1e16 to
/// 1e16 + 2 in tenths), it's half as many, until they can or there's one.
pub(crate) fn spec(lo: f64, hi: f64, count: f64) -> Option<(f64, f64, Inc)> {
    spec_in(lo, hi, count, true)
}

/// [`spec`]; `fewer` allows halving `count` (not after doubling it, so the two never take turns).
fn spec_in(lo: f64, hi: f64, count: f64, fewer: bool) -> Option<(f64, f64, Inc)> {
    let raw = (hi - lo) / count;
    if !(raw.is_finite() && raw > 0.0) {
        return None;
    }
    let power = raw.log10().floor();
    let error = raw / 10f64.powf(power);
    let factor = if error >= E10 {
        10.0
    } else if error >= E5 {
        5.0
    } else if error >= E2 {
        2.0
    } else {
        1.0
    };
    let p = power as i32;
    let (i1, i2, inc) = if p < 0 {
        let d = pow10(-p) / factor;
        if !d.is_finite() {
            return None;
        }
        let (mut a, mut b) = ((lo * d).round(), (hi * d).round());
        if a / d < lo {
            a += 1.0;
        }
        if b / d > hi {
            b -= 1.0;
        }
        (a, b, Inc::Div(d))
    } else {
        let m = pow10(p) * factor;
        if !(m.is_finite() && m > 0.0) {
            return None;
        }
        let (mut a, mut b) = ((lo / m).round(), (hi / m).round());
        if a * m < lo {
            a += 1.0;
        }
        if b * m > hi {
            b -= 1.0;
        }
        (a, b, Inc::Mul(m))
    };
    if i2 < i1 && (0.5..2.0).contains(&count) {
        return spec_in(lo, hi, count * 2.0, false);
    }
    if !(i1.is_finite() && i2.is_finite()) || i2 - i1 > MOST {
        return None;
    }
    if i1.abs().max(i2.abs()) >= EXACT {
        return if fewer && count >= 2.0 {
            spec_in(lo, hi, (count / 2.0).floor(), true)
        } else {
            None
        };
    }
    Some((i1, i2, inc))
}

/// The tick values over `lo ..= hi` (ascending, finite, `lo < hi`) and their step.
pub(crate) fn values(lo: f64, hi: f64, count: usize) -> Option<(Vec<f64>, f64)> {
    let (i1, i2, inc) = spec(lo, hi, count.max(1) as f64)?;
    // A whole-number count: [`spec`] keeps the indices exact and at most `MOST` apart.
    let n = if i2 < i1 { 0 } else { (i2 - i1) as u64 + 1 };
    // `+ 0.0` turns a -0 into 0.
    let mut out: Vec<f64> = (0..n).map(|k| inc.at(i1 + k as f64) + 0.0).collect();
    // Near the floats' limits two neighbours can round to one value.
    out.dedup();
    Some((out, inc.step()))
}

/// A domain's ends in ascending order, or None when either isn't finite.
pub(crate) fn ascending(d: (f64, f64)) -> Option<(f64, f64)> {
    let (a, b) = d;
    if !(a.is_finite() && b.is_finite()) {
        return None;
    }
    Some(if a <= b { (a, b) } else { (b, a) })
}

/// The linear scale's ticks (see [`super::ticks`]).
pub(crate) fn ticks(domain: (f64, f64), count: usize) -> Vec<Tick> {
    let Some((lo, hi)) = ascending(domain) else {
        return Vec::new();
    };
    if lo == hi {
        let style = axis_style(lo.abs(), f64::NAN);
        return vec![Tick {
            value: lo + 0.0,
            label: format_number(lo, f64::NAN, style),
            major: false,
        }];
    }
    let Some((vals, step)) = values(lo, hi, count) else {
        return Vec::new();
    };
    labelled(&vals, step)
}

/// Ticks at `vals`, labelled in one style for the axis at `step`: all scientific or none.
pub(crate) fn labelled(vals: &[f64], step: f64) -> Vec<Tick> {
    let reach = vals.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let style = axis_style(reach, step);
    let sci = axis_scientific(vals);
    vals.iter()
        .map(|&v| Tick {
            value: v,
            label: format_tick(v, step, style, sci),
            major: false,
        })
        .collect()
}

/// A place for `v`: proportional between the domain's ends and the range's.
pub(crate) fn map(domain: (f64, f64), range: (f32, f32), v: f64) -> f32 {
    let (d0, d1) = domain;
    let (r0, r1) = (range.0 as f64, range.1 as f64);
    if d1 == d0 {
        return ((r0 + r1) / 2.0) as f32;
    }
    (r0 + (v - d0) / (d1 - d0) * (r1 - r0)) as f32
}

/// The value at `px`: the inverse of [`map`].
pub(crate) fn invert(domain: (f64, f64), range: (f32, f32), px: f32) -> f64 {
    let (d0, d1) = domain;
    let (r0, r1) = (range.0 as f64, range.1 as f64);
    if r1 == r0 {
        return d0;
    }
    d0 + (px as f64 - r0) / (r1 - r0) * (d1 - d0)
}

/// [`super::nice`].
pub(crate) fn nice(lo: f64, hi: f64, count: usize) -> (f64, f64, f64) {
    let count = count.max(1) as f64;
    let Some((mut a, mut b)) = ascending((lo, hi)) else {
        return nice(0.0, 1.0, count as usize);
    };
    if a == b {
        let pad = if a == 0.0 { 1.0 } else { a.abs() / 10.0 };
        a -= pad;
        b += pad;
    }
    let mut last: Option<Inc> = None;
    // d3's loop: a widened domain can ask for a larger step, which widens it again. It settles
    // in two or three rounds; ten is a bound, not a budget.
    for _ in 0..10 {
        let Some((_, _, inc)) = spec(a, b, count) else {
            break;
        };
        if last == Some(inc) {
            break;
        }
        a = inc.floor(a) + 0.0;
        b = inc.ceil(b) + 0.0;
        last = Some(inc);
    }
    let step = last.map_or(f64::NAN, Inc::step);
    if lo > hi { (b, a, step) } else { (a, b, step) }
}
