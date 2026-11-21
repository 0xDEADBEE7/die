//! The log scale: mapping by the logarithm, ticks at powers of the base and their multiples.
//!
//! A domain below zero is the mirror of one above it (d3's `-log(-x)`); a domain that touches
//! or crosses zero has no logarithm, so it maps everything to NaN and has no ticks.

use super::Tick;
use super::format::{axis_style, format_number};
use super::linear::{self, ascending, pow10};

/// Slack for a power that a logarithm misses by an ulp: log₂(8) can be 2.9999999999999996.
const SLACK: f64 = 1e-9;

/// The sign the domain's values share: 1 above zero, -1 below, None when it touches or crosses
/// zero or isn't finite.
fn sign(domain: (f64, f64)) -> Option<f64> {
    let (a, b) = domain;
    if !(a.is_finite() && b.is_finite()) {
        None
    } else if a > 0.0 && b > 0.0 {
        Some(1.0)
    } else if a < 0.0 && b < 0.0 {
        Some(-1.0)
    } else {
        None
    }
}

/// The transformed value: `ln v` above zero, `-ln(-v)` below, NaN on the wrong side.
fn fwd(s: f64, v: f64) -> f64 {
    if v * s > 0.0 {
        s * (s * v).ln()
    } else {
        f64::NAN
    }
}

/// The inverse of [`fwd`].
fn back(s: f64, t: f64) -> f64 {
    s * (s * t).exp()
}

/// The log scale's place for `v`.
pub(crate) fn map(domain: (f64, f64), range: (f32, f32), v: f64) -> f32 {
    let Some(s) = sign(domain) else {
        return f32::NAN;
    };
    let t = fwd(s, v);
    if t.is_nan() {
        return f32::NAN;
    }
    linear::map((fwd(s, domain.0), fwd(s, domain.1)), range, t)
}

/// The log scale's value at `px`.
pub(crate) fn invert(domain: (f64, f64), range: (f32, f32), px: f32) -> f64 {
    let Some(s) = sign(domain) else {
        return f64::NAN;
    };
    back(
        s,
        linear::invert((fwd(s, domain.0), fwd(s, domain.1)), range, px),
    )
}

/// A usable base: above 1 and finite, else 10.
fn base_of(base: f64) -> f64 {
    if base.is_finite() && base > 1.0 {
        base
    } else {
        10.0
    }
}

/// The logarithm of `v` in `base`, exact at powers of ten for base 10.
fn log_in(base: f64, v: f64) -> f64 {
    if base == 10.0 {
        v.log10()
    } else {
        v.ln() / base.ln()
    }
}

/// `m` times the base to the `k`, exact in base 10 while the power is.
fn value(base: f64, m: f64, k: i32) -> f64 {
    if base == 10.0 {
        if k >= 0 { m * pow10(k) } else { m / pow10(-k) }
    } else {
        m * base.powi(k)
    }
}

/// Ticks over `a ..= b` (both above zero, `a < b`): (value, the power it sits on, whether it's
/// a power), or None when too few fall there to be worth it.
fn powers(a: f64, b: f64, base: f64, count: usize) -> Option<Vec<(f64, f64, bool)>> {
    let (la, lb) = (log_in(base, a), log_in(base, b));
    let decades = lb - la;
    let count_f = count as f64;
    let mantissas: &[f64] = if base == 10.0 && decades * 3.0 <= count_f {
        &[1.0, 2.0, 5.0]
    } else {
        &[1.0]
    };
    let every = if decades <= count_f {
        1
    } else {
        (decades / count_f).ceil() as i32
    };
    let (k0, k1) = ((la - SLACK).floor(), (lb + SLACK).floor());
    if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > 4096.0 {
        return None;
    }
    let (lo, hi) = (a * (1.0 - SLACK), b * (1.0 + SLACK));
    let mut out = Vec::new();
    for k in k0 as i32..=k1 as i32 {
        if k.rem_euclid(every) != 0 {
            continue;
        }
        let power = value(base, 1.0, k);
        for &m in mantissas {
            let v = value(base, m, k);
            if (lo..=hi).contains(&v) {
                out.push((v, power, m == 1.0));
            }
        }
    }
    // d3's rule: fewer than half the ticks asked for, and linear ones say more.
    (out.len() * 2 >= count && out.len() >= 2).then_some(out)
}

/// The log scale's ticks (see [`super::ticks`]).
pub(crate) fn ticks(domain: (f64, f64), base: f64, count: usize) -> Vec<Tick> {
    let Some(s) = sign(domain) else {
        return Vec::new();
    };
    let Some((lo, hi)) = ascending(domain) else {
        return Vec::new();
    };
    // Work on magnitudes; a domain below zero is mirrored back at the end.
    let (a, b) = if s > 0.0 { (lo, hi) } else { (-hi, -lo) };
    if a == b {
        return linear::ticks(domain, count);
    }
    let base = base_of(base);
    let Some(mut ts) = powers(a, b, base, count) else {
        let Some((mut vals, step)) = linear::values(a, b, count) else {
            return Vec::new();
        };
        if s < 0.0 {
            vals = vals.iter().rev().map(|v| -v).collect();
        }
        return linear::labelled(&vals, step);
    };
    if s < 0.0 {
        ts = ts
            .iter()
            .rev()
            .map(|&(v, p, major)| (-v, p, major))
            .collect();
    }
    let reach = ts.iter().fold(0.0f64, |m, t| m.max(t.0.abs()));
    let style = axis_style(reach, f64::NAN);
    ts.into_iter()
        .map(|(v, p, major)| Tick {
            value: v,
            label: format_number(v, p, style),
            major,
        })
        .collect()
}

/// A log domain widened to powers of its base; one that touches zero comes back as it was.
pub(crate) fn nice(domain: (f64, f64), base: f64) -> (f64, f64) {
    let Some(s) = sign(domain) else {
        return domain;
    };
    let base = base_of(base);
    let widen = |v: f64, up: bool| {
        let l = log_in(base, s * v);
        let k = if up {
            (l - SLACK).ceil()
        } else {
            (l + SLACK).floor()
        };
        s * value(base, 1.0, k as i32)
    };
    let (d0, d1) = domain;
    // The end nearer zero rounds towards it, the other away, whichever way round the domain is.
    let low_first = (s * d0) <= (s * d1);
    if low_first {
        (widen(d0, false), widen(d1, true))
    } else {
        (widen(d0, true), widen(d1, false))
    }
}
