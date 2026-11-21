//! fig's largest-triangle-three-buckets downsampling, verbatim (briefs/fig-port.md §3).
//!
//! It keeps the first and last samples and, from each of `threshold - 2` buckets between them,
//! the sample making the largest triangle with the one kept before it and the next bucket's mean.
//! It knows nothing of gaps (a NaN loses every comparison) and depends on the samples' order. SVG
//! export and the cell fallback thin lines with it; the pixel path draws from
//! [`super::Pyramid`]'s envelope instead.

/// The indices of the samples kept, in order: exactly `threshold` of them when there are more
/// samples than that, or every sample when there aren't or `threshold` is under 3. `xs` and `ys`
/// pair up to the shorter of the two.
pub fn lttb(xs: &[f64], ys: &[f64], threshold: usize) -> Vec<usize> {
    let len = xs.len().min(ys.len());
    if threshold >= len || threshold < 3 {
        return (0..len).collect();
    }
    let mut kept = Vec::with_capacity(threshold);
    kept.push(0);
    let bw = (len - 2) as f64 / (threshold - 2) as f64;
    let mut sel = 0;
    for b in 0..threshold - 2 {
        // The next bucket's mean.
        let a0 = ((b + 1) as f64 * bw).floor() as usize + 1;
        let a1 = (((b + 2) as f64 * bw).floor() as usize + 1).min(len);
        let next = a0.min(len - 1)..a1.max(a0 + 1).min(len);
        let count = next.len() as f64;
        let (ax, ay) = next.fold((0.0, 0.0), |s, i| (s.0 + xs[i], s.1 + ys[i]));
        let (ax, ay) = (ax / count, ay / count);
        // This bucket: the sample with the largest triangle; the first of equals.
        let r0 = (b as f64 * bw).floor() as usize + 1;
        let r1 = (((b + 1) as f64 * bw).floor() as usize + 1)
            .min(len - 1)
            .max(r0 + 1);
        let (sx, sy) = (xs[sel], ys[sel]);
        let mut most = -1.0;
        let mut pick = r0;
        for i in r0..r1 {
            let area = ((sx - ax) * (ys[i] - sy) - (sx - xs[i]) * (ay - sy)).abs();
            if area > most {
                most = area;
                pick = i;
            }
        }
        kept.push(pick);
        sel = pick;
    }
    kept.push(len - 1);
    kept
}
