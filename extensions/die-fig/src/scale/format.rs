//! How values are written: an axis's numbers in one style, the card's in full, times at the
//! precision of their step, and a spec's explicit formats.

/// How an axis writes its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberStyle {
    /// With thousands separators: 1,284.
    Full,
    /// With an SI suffix: 12.9K, 4.2M.
    Compact,
}

/// 1,284 / 12.9K / 4.2M; one style per axis: Compact when the axis reaches 10,000.
pub fn format_number(_v: f64, _step: f64, _style: NumberStyle) -> String {
    todo!("P1-scale: format_number")
}

/// A value in full for the card: thousands separators, decimals from the data's precision.
pub fn format_value(_v: f64) -> String {
    todo!("P1-scale: format_value")
}

/// A time at the precision of `step` seconds ("Nov 21", "10:25", ":15").
pub fn format_time(_v: f64, _step: f64, _zone: &jiff::tz::TimeZone) -> String {
    todo!("P1-scale: format_time")
}

/// A time's full form for the card ("10:25 · Fri 21 Nov").
pub fn format_time_full(_v: f64, _step: f64, _zone: &jiff::tz::TimeZone) -> String {
    todo!("P1-scale: format_time_full")
}

/// An explicit format from a spec: "%H:%M" (strtime) or ",.2f"-like numeric (",", ".Nf", "%",
/// "s" SI).
pub fn format_with(_v: f64, _fmt: &str, _zone: &jiff::tz::TimeZone) -> String {
    todo!("P1-scale: format_with")
}
