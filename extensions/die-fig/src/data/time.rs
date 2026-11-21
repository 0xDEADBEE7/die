//! Times parsed in an injected zone (ruling 2), as `f64` seconds since the Unix epoch, UTC.
//!
//! A cell is tried as a `jiff::Timestamp` (it has an offset or `Z`), then as a
//! `jiff::civil::DateTime` in [`ReadOptions::zone`], then as a `jiff::civil::Date` only when it
//! has no time part. Slashed and day-first dates need an explicit
//! [`ReadOptions::time_format`] (strtime). Epoch numbers stay numbers unless a spec says
//! otherwise.

use super::ReadOptions;

/// A cell's time in UTC seconds since the epoch, if it is one.
pub fn parse_time(_s: &str, _opt: &ReadOptions) -> Option<f64> {
    todo!("P1-data: parse_time")
}
