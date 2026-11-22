//! Follow mode (contract-p4 §4, mouse-ux §3.2) as a pure function of the view's x window, the
//! data's newest x and whether it followed before: on a time chart (alone, or the shared time
//! of a dashboard's linked panels) whose data grows, the view follows while its right edge is
//! at the newest sample, and new rows slide the window keeping its width.
//!
//! - **New data** ([`FollowChange::Data`]) slides a following window so its right edge is the
//!   newest sample, its width kept; it never moves a window that isn't following. A view that
//!   fits the data (None) fits it still: it grows with the data, its right edge always the end.
//! - **The hand** ([`FollowChange::Moved`]: a pan, a zoom, a brush, a key) leaves it following
//!   while its right edge is at the newest ([`at_end`]) and not otherwise: away from the end it
//!   stops; panned (or zoomed out) back to the end it follows again.
//! - **`G`**, and a double-click's reset ([`FollowChange::Resume`]), put the right edge on the
//!   newest and follow.
//!
//! New data never moves the cursor off its sample either: [`keep_cursor`] finds the sample a
//! cursor was on in the data as it is now, where rows dropped from the front have moved it.

use crate::interact::Cursor;
use crate::plot::ChartModel;

/// What changed, for follow mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowChange {
    /// New data came: a run's output, a stream's fold, a file appended to.
    Data,
    /// The hand moved the view: a pan, a zoom, a brush, a key.
    Moved,
    /// `G`, or a double-click's reset: to the newest, following.
    Resume,
}

/// A view's x window and whether it follows, after a change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    /// The x window: None fits the data.
    pub x: Option<(f64, f64)>,
    /// Whether it follows the newest sample (the status says `following`).
    pub on: bool,
}

/// Follow mode after `change`: the view's x window `x` (None fits the data), the data's newest
/// x as it is now, whether it followed before (`on`), and how much of x a pixel is at the
/// view's width (`px`, the slack "at the end" allows).
pub fn follow(
    x: Option<(f64, f64)>,
    newest: Option<f64>,
    on: bool,
    px: f64,
    change: FollowChange,
) -> Follow {
    match change {
        FollowChange::Data => match (x, newest) {
            (Some((a, b)), Some(n)) if on && n > b => Follow {
                x: Some((n - (b - a), n)),
                on,
            },
            _ => Follow { x, on },
        },
        FollowChange::Moved => match (x, newest) {
            (Some(w), Some(n)) => Follow {
                x,
                on: at_end(w, n, px),
            },
            // A view that fits the data ends at its end.
            (None, Some(_)) => Follow { x, on: true },
            (_, None) => Follow { x, on },
        },
        FollowChange::Resume => match (x, newest) {
            (Some((a, b)), Some(n)) => Follow {
                x: Some((n - (b - a), n)),
                on: true,
            },
            _ => Follow { x, on: true },
        },
    }
}

/// Whether the window `x` is at the newest sample `newest`: it's in view, and the right edge is
/// no more than `px` before it (past it counts too: there's nothing newer to show).
pub fn at_end(x: (f64, f64), newest: f64, px: f64) -> bool {
    let (a, b) = (x.0.min(x.1), x.0.max(x.1));
    let px = if px.is_finite() { px.abs() } else { 0.0 };
    newest >= a && b >= newest - px
}

/// The cursor on the same sample in `new` as it was on in `old`: a point by its series and its
/// x (rows dropped from the front move its row), a band or a cell by its categories. None when
/// that sample is gone.
pub fn keep_cursor(old: &ChartModel, new: &ChartModel, cursor: &Cursor) -> Option<Cursor> {
    match cursor {
        Cursor::Point { series, row } => {
            let was = old.series.iter().find(|s| &s.key == series)?;
            let now = new.series.iter().find(|s| &s.key == series)?;
            let i = *row as usize;
            let (x, y) = (*was.xs.get(i)?, was.ys.get(i).copied().unwrap_or(f64::NAN));
            // On a band axis x is a band's index: the band by its name.
            let x = match old.x.bands.is_empty() {
                true => x,
                false => {
                    let name = old.x.bands.get(x as usize)?;
                    new.x.bands.iter().position(|b| b == name)? as f64
                }
            };
            let from = now.xs.partition_point(|v| *v < x);
            let same = (from..now.xs.len()).take_while(|&j| now.xs[j] == x);
            let same: Vec<usize> = same.collect();
            // Of the samples at that x, the one of the same value, else the first.
            let j = (same.iter())
                .find(|&&j| {
                    now.ys
                        .get(j)
                        .is_some_and(|v| v == &y || v.is_nan() && y.is_nan())
                })
                .or(same.first())?;
            Some(Cursor::Point {
                series: series.clone(),
                row: u32::try_from(*j).ok()?,
            })
        }
        Cursor::Band { band, series } => {
            let name = old.x.bands.get(*band as usize)?;
            let at = new.x.bands.iter().position(|b| b == name)?;
            let series = match series {
                Some(k) => Some(new.series.iter().find(|s| &s.key == k)?.key.clone()),
                None => None,
            };
            Some(Cursor::Band {
                band: u32::try_from(at).ok()?,
                series,
            })
        }
        Cursor::Cell { row, col } => {
            let r = old.y.bands.get(*row as usize)?;
            let c = old.x.bands.get(*col as usize)?;
            let row = new.y.bands.iter().position(|b| b == r)?;
            let col = new.x.bands.iter().position(|b| b == c)?;
            Some(Cursor::Cell {
                row: u32::try_from(row).ok()?,
                col: u32::try_from(col).ok()?,
            })
        }
        Cursor::Node(id) => Some(Cursor::Node(id.clone())),
    }
}
