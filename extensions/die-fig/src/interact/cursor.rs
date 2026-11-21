//! The keyboard's cursor on the data, and its motions: by step, series, peak and neighbour.
//! None while a figure opens; the first `h l j k`, click or search hit sets it (ruling 6).

use crate::data::SeriesKey;

/// Where the cursor is on the data.
#[derive(Clone, Debug, PartialEq)]
pub enum Cursor {
    /// A sample of a series.
    Point {
        /// The series.
        series: SeriesKey,
        /// Its row.
        row: u32,
    },
    /// A band, and a segment of it if stacked.
    Band {
        /// The band, from 0.
        band: u32,
        /// The series of the segment.
        series: Option<SeriesKey>,
    },
    /// A heatmap's cell.
    Cell {
        /// The row.
        row: u32,
        /// The column.
        col: u32,
    },
    /// A graph's node, by its id.
    Node(Box<str>),
}
