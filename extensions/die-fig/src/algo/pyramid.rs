//! A min/max pyramid over x-sorted samples, and the M4 envelope a long series draws from: per
//! pixel column the first, least, greatest and last sample, exact at the pixel.

/// Per level, the min and max of each bucket of 2^k samples, over x-sorted data.
pub struct Pyramid {
    _levels: Vec<Vec<(u32, u32)>>,
}

impl Pyramid {
    /// The pyramid over samples sorted by x.
    pub fn build(_xs: &[f64], _ys: &[f64]) -> Pyramid {
        todo!("P1-algo: Pyramid::build")
    }

    /// For each of `columns` equal pixel columns over x in [x0, x1]: the first, min, max and
    /// last sample (as indices into the raw data), exact at the pixel (M4). NaN gaps stay gaps.
    pub fn envelope(
        &self,
        _xs: &[f64],
        _ys: &[f64],
        _x0: f64,
        _x1: f64,
        _columns: usize,
    ) -> Vec<[u32; 4]> {
        todo!("P1-algo: Pyramid::envelope")
    }
}
