//! The band scale: one band a category, with inner and outer paddings, for bars and heatmaps.
//!
//! The geometry is d3's `scaleBand` centred in its range: a step for each band, of which the
//! inner padding is the gap between bands, and the outer padding (in steps) outside the first
//! and last. Places in bands run from 0 at the first band's slot to n at the last's end, each
//! slot a band and half the inner gap on each side, so `i + 0.5` is band `i`'s centre.

use super::Tick;

/// Where a band scale's bands fall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    /// How many bands.
    n: usize,
    /// Where place 0 in bands is: the first slot's start.
    origin: f64,
    /// One band and one inner gap, signed with the range.
    step: f64,
    /// Where the first band starts.
    start: f64,
    /// A band's width, signed with the range.
    width: f64,
}

impl Geometry {
    /// The geometry of `n` bands over `range`, paddings clamped to 0 to 1 (inner) and 0 up
    /// (outer).
    pub(crate) fn new(n: usize, range: (f32, f32), inner: f32, outer: f32) -> Geometry {
        let inner = if inner.is_finite() {
            inner.clamp(0.0, 1.0) as f64
        } else {
            0.0
        };
        let outer = if outer.is_finite() {
            outer.max(0.0) as f64
        } else {
            0.0
        };
        let (r0, r1) = (range.0 as f64, range.1 as f64);
        let nf = n as f64;
        let step = (r1 - r0) / (nf - inner + 2.0 * outer).max(1.0);
        let start = r0 + (r1 - r0 - step * (nf - inner)) / 2.0;
        Geometry {
            n,
            origin: start - step * inner / 2.0,
            step,
            start,
            width: step * (1.0 - inner),
        }
    }

    /// The place of `v`, in bands.
    pub(crate) fn map(&self, v: f64) -> f32 {
        (self.origin + v * self.step) as f32
    }

    /// The place in bands at `px`.
    pub(crate) fn invert(&self, px: f32) -> f64 {
        if self.step == 0.0 {
            return 0.0;
        }
        (px as f64 - self.origin) / self.step
    }

    /// Band `i`'s lower edge and width.
    pub(crate) fn band(&self, i: usize) -> Option<(f32, f32)> {
        if i >= self.n {
            return None;
        }
        let a = self.start + i as f64 * self.step;
        let b = a + self.width;
        Some((a.min(b) as f32, (b - a).abs() as f32))
    }

    /// The band whose slot holds `px`.
    pub(crate) fn at(&self, px: f32) -> Option<usize> {
        let i = self.invert(px).floor();
        (i >= 0.0 && i < self.n as f64).then_some(i as usize)
    }
}

/// The band scale's ticks: each category at its centre, every nth when there are more than
/// `count`.
pub(crate) fn ticks(domain: &[Box<str>], count: usize) -> Vec<Tick> {
    let every = domain.len().div_ceil(count.max(1)).max(1);
    domain
        .iter()
        .enumerate()
        .step_by(every)
        .map(|(i, name)| Tick {
            value: i as f64 + 0.5,
            label: name.to_string(),
            major: false,
        })
        .collect()
}
