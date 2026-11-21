//! A min/max pyramid over x-sorted samples, and the M4 envelope a long series draws from: per
//! pixel column the first, least, greatest and last sample, exact at the pixel.
//!
//! A line through each column's four samples, in index order, lights the same pixels as the line
//! through every sample (M4), so a million samples cost what the columns cost. The pyramid finds
//! a column's least and greatest in `O(log n)`: each level holds, per bucket of `2^k` samples,
//! the indices of its least and greatest, and a column's range is a few aligned buckets plus
//! fewer than [`LEAF`] raw samples at each end. Columns' ranges come from binary searches over x.
//!
//! Samples are `f64`, x ascending. A NaN y is a missing sample, and a gap: it never shows in an
//! envelope, and [`Pyramid::gap`] says whether one lies between two samples, so the line breaks
//! there. Ties go to the earlier sample, so the result is the same however it's found.

/// A column with no sample in it.
pub const EMPTY: [u32; 4] = [u32::MAX; 4];

/// The smallest bucket the pyramid keeps; below it, columns read the raw samples.
pub const LEAF: usize = 16;

const NONE: u32 = u32::MAX;

/// Per level, the min and max of each bucket of 2^k samples, over x-sorted data.
pub struct Pyramid {
    len: usize,
    /// `levels[j][b]`: the indices of the least and greatest sample in bucket `b` of
    /// `LEAF << j` samples, [`NONE`] when every one is missing.
    levels: Vec<Vec<(u32, u32)>>,
    missing: Missing,
}

impl Pyramid {
    /// The pyramid over samples sorted by x (ascending; a NaN x, if any, only at the end, where
    /// the envelope never reaches). `xs` and `ys` pair up to the shorter of the two. It's
    /// `O(n)` to build and holds about `n / LEAF` index pairs.
    pub fn build(xs: &[f64], ys: &[f64]) -> Pyramid {
        let len = xs.len().min(ys.len()).min(NONE as usize);
        let ys = &ys[..len];
        let missing = Missing::of(ys);
        let mut levels = Vec::new();
        let base: Vec<(u32, u32)> = ys
            .chunks(LEAF)
            .enumerate()
            .map(|(b, chunk)| {
                let mut m = MinMax::default();
                for (i, y) in chunk.iter().enumerate() {
                    if !y.is_nan() {
                        m.take(ys, (b * LEAF + i) as u32);
                    }
                }
                (m.min, m.max)
            })
            .collect();
        let mut level = base;
        while level.len() > 1 {
            let up = level
                .chunks(2)
                .map(|pair| {
                    let mut m = MinMax::default();
                    for &(lo, hi) in pair {
                        m.join(ys, lo, hi);
                    }
                    (m.min, m.max)
                })
                .collect();
            levels.push(std::mem::replace(&mut level, up));
        }
        levels.push(level);
        Pyramid {
            len,
            levels,
            missing,
        }
    }

    /// How many samples it was built over.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether it was built over no samples.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// For each of `columns` equal pixel columns over x in [x0, x1]: the first, min, max and
    /// last sample (as indices into the raw data), exact at the pixel (M4). NaN gaps stay gaps.
    ///
    /// `xs` and `ys` are the samples it was built over. A sample's column is [`column()`]'s. A
    /// column with no sample, or only missing ones, is [`EMPTY`]. Min and max are the first of
    /// equals. The line runs on from one column's last sample to the next drawn sample unless
    /// [`Pyramid::gap`] says a missing one lies between them; inside a column likewise, between
    /// its four in index order. With `x1 <= x0` or a bound that isn't finite, every column is
    /// empty.
    pub fn envelope(
        &self,
        xs: &[f64],
        ys: &[f64],
        x0: f64,
        x1: f64,
        columns: usize,
    ) -> Vec<[u32; 4]> {
        let mut out = vec![EMPTY; columns];
        let len = self.len.min(xs.len()).min(ys.len());
        if columns == 0 || !Domain::valid(x0, x1) {
            return out;
        }
        let (xs, ys) = (&xs[..len], &ys[..len]);
        let dom = Domain { x0, x1, columns };
        let lo = xs.partition_point(|&x| x < x0);
        let hi = lo + xs[lo..].partition_point(|&x| x <= x1);
        let mut s = lo;
        for (c, slot) in out.iter_mut().enumerate() {
            if s >= hi {
                break;
            }
            let e = if c + 1 == columns {
                hi
            } else {
                s + xs[s..hi].partition_point(|&x| dom.index(x) <= c)
            };
            if let Some(m4) = self.m4(ys, s, e) {
                *slot = m4;
            }
            s = e;
        }
        out
    }

    /// Whether a missing sample (a NaN y) lies strictly between samples `a` and `b`, in either
    /// order: where a line through them breaks.
    pub fn gap(&self, a: u32, b: u32) -> bool {
        let (a, b) = (a.min(b) as usize, a.max(b) as usize);
        self.missing.between(a, b)
    }

    /// The first, least, greatest and last sample in `s..e`, if any isn't missing.
    fn m4(&self, ys: &[f64], s: usize, e: usize) -> Option<[u32; 4]> {
        let first = self.missing.next(s, e)?;
        let last = self.missing.prev(s, e)?;
        let m = self.range(ys, first, last + 1);
        Some([first as u32, m.min, m.max, last as u32])
    }

    /// The least and greatest sample in `s..e`: aligned buckets where they fit, raw samples at
    /// the ends.
    fn range(&self, ys: &[f64], s: usize, e: usize) -> MinMax {
        let mut m = MinMax::default();
        let mut i = s;
        while i < e {
            if !i.is_multiple_of(LEAF) || i + LEAF > e {
                if !ys[i].is_nan() {
                    m.take(ys, i as u32);
                }
                i += 1;
                continue;
            }
            let (mut j, mut b) = (0, i / LEAF);
            while j + 1 < self.levels.len() && b.is_multiple_of(2) && i + (LEAF << (j + 1)) <= e {
                j += 1;
                b /= 2;
            }
            let (lo, hi) = self.levels[j][b];
            m.join(ys, lo, hi);
            i += LEAF << j;
        }
        m
    }
}

/// Which of `columns` equal columns over x in [x0, x1] a sample at `x` falls in: column `c`
/// holds `x0 + c·w <= x < x0 + (c+1)·w` for `w = (x1 - x0) / columns`, and the last holds `x1`
/// too. None outside, for NaN, and when `x1 <= x0` or a bound isn't finite. The envelope uses
/// it, so a brute-force scan with it is the envelope's definition.
pub fn column(x: f64, x0: f64, x1: f64, columns: usize) -> Option<usize> {
    if columns == 0 || !Domain::valid(x0, x1) || !(x0 <= x && x <= x1) {
        return None;
    }
    Some(Domain { x0, x1, columns }.index(x))
}

struct Domain {
    x0: f64,
    x1: f64,
    columns: usize,
}

impl Domain {
    fn valid(x0: f64, x1: f64) -> bool {
        x0.is_finite() && x1.is_finite() && x1 > x0 && (x1 - x0).is_finite()
    }

    /// Monotone in x over [x0, x1], as every step rounds monotonically: binary searches with it
    /// split the samples exactly where a scan would.
    fn index(&self, x: f64) -> usize {
        let t = (x - self.x0) / (self.x1 - self.x0) * self.columns as f64;
        (t.floor() as usize).min(self.columns - 1)
    }
}

/// The indices of the least and greatest sample seen, the first of equals.
struct MinMax {
    min: u32,
    max: u32,
}

impl Default for MinMax {
    fn default() -> MinMax {
        MinMax {
            min: NONE,
            max: NONE,
        }
    }
}

impl MinMax {
    /// One sample, later than every one seen.
    fn take(&mut self, ys: &[f64], i: u32) {
        self.join(ys, i, i);
    }

    /// A bucket's least and greatest, later than every sample seen.
    fn join(&mut self, ys: &[f64], lo: u32, hi: u32) {
        if lo != NONE && (self.min == NONE || ys[lo as usize] < ys[self.min as usize]) {
            self.min = lo;
        }
        if hi != NONE && (self.max == NONE || ys[hi as usize] > ys[self.max as usize]) {
            self.max = hi;
        }
    }
}

/// The runs of missing samples, ascending, each maximal: `O(log runs)` to ask about.
#[derive(Clone, Debug, Default)]
struct Missing {
    /// `[start, end)` of each run.
    runs: Vec<(usize, usize)>,
}

impl Missing {
    /// The runs of NaN in `ys`.
    fn of(ys: &[f64]) -> Missing {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for (i, y) in ys.iter().enumerate() {
            if y.is_nan() {
                match runs.last_mut() {
                    Some(r) if r.1 == i => r.1 = i + 1,
                    _ => runs.push((i, i + 1)),
                }
            }
        }
        Missing { runs }
    }

    /// The run at or after `i`: the first that ends past it.
    fn run_from(&self, i: usize) -> Option<(usize, usize)> {
        let k = self.runs.partition_point(|r| r.1 <= i);
        self.runs.get(k).copied()
    }

    /// The first sample in `i..end` that isn't missing.
    fn next(&self, i: usize, end: usize) -> Option<usize> {
        let j = match self.run_from(i) {
            Some((s, e)) if s <= i => e,
            _ => i,
        };
        (j < end).then_some(j)
    }

    /// The last sample in `start..end` that isn't missing.
    fn prev(&self, start: usize, end: usize) -> Option<usize> {
        if end <= start {
            return None;
        }
        let i = end - 1;
        let j = match self.run_from(i) {
            Some((s, _)) if s <= i => s.checked_sub(1)?,
            _ => i,
        };
        (j >= start).then_some(j)
    }

    /// Whether a missing sample lies strictly between `a` and `b` (`a <= b`).
    fn between(&self, a: usize, b: usize) -> bool {
        if b <= a + 1 {
            return false;
        }
        self.run_from(a + 1).is_some_and(|(s, _)| s < b)
    }
}
