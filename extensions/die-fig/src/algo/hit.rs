//! The hit index: what the pointer is nearest. Nearest x by binary search over each visible
//! series' raw samples; points and nodes in a grid hash with cells of `HIT_DENSE`; edges by
//! segment distance.

use crate::data::SeriesKey;
use std::sync::Arc;

/// What's where, for the pointer: series by x, points and segments by screen position.
#[derive(Default)]
pub struct HitIndex {
    _series: Vec<SeriesKey>,
}

impl HitIndex {
    /// An empty index.
    pub fn new() -> HitIndex {
        HitIndex::default()
    }

    /// A series' samples, sorted by x.
    pub fn add_series(&mut self, _key: SeriesKey, _xs: Arc<[f64]>, _ys: Arc<[f64]>) {
        todo!("P1-algo: HitIndex::add_series")
    }

    /// A point at a screen position (logical px).
    pub fn add_point(&mut self, _key: SeriesKey, _row: u32, _x: f32, _y: f32) {
        todo!("P1-algo: HitIndex::add_point")
    }

    /// A segment between screen positions: an edge.
    pub fn add_segment(&mut self, _id: u32, _a: (f32, f32), _b: (f32, f32)) {
        todo!("P1-algo: HitIndex::add_segment")
    }

    /// The row of a series' sample nearest x, in data.
    pub fn nearest_x(&self, _key: &SeriesKey, _x: f64) -> Option<u32> {
        todo!("P1-algo: HitIndex::nearest_x")
    }

    /// The point nearest a screen position, within `reach`.
    pub fn nearest_point(&self, _x: f32, _y: f32, _reach: f32) -> Option<(SeriesKey, u32)> {
        todo!("P1-algo: HitIndex::nearest_point")
    }

    /// The segment nearest a screen position, within `reach`.
    pub fn nearest_segment(&self, _x: f32, _y: f32, _reach: f32) -> Option<u32> {
        todo!("P1-algo: HitIndex::nearest_segment")
    }
}
