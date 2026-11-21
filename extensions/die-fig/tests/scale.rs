//! The scales: linear, log, time and band mapping and inverting, nice domains, ticks at nice
//! numbers, at powers and on the calendar (across daylight saving changes in Europe/London and
//! Australia/Sydney), and every way a value is written.
//!
//! Times are built from text here, in UTC or a named zone from the system's zoneinfo, never from
//! the clock.

use die_test_cap as _;

use die_fig::scale::{
    self, NumberStyle, Scale, Tick, axis_style, data_decimals, format_fixed, format_number,
    format_time, format_time_full, format_time_point, format_value, format_with,
};
use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{self, TimeZone};

const NAN: f64 = f64::NAN;
const DAY: f64 = 86_400.0;
const HOUR: f64 = 3_600.0;

fn utc() -> TimeZone {
    TimeZone::fixed(tz::offset(0))
}

fn named(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap_or_else(|e| panic!("the system's zoneinfo has {name}: {e}"))
}

/// An instant written with its offset, as UTC seconds.
fn at(s: &str) -> f64 {
    let t: Timestamp = s.parse().unwrap();
    t.as_second() as f64 + t.subsec_nanosecond() as f64 / 1e9
}

/// A civil time in a zone (the earlier of two in a fold), as UTC seconds.
fn local(zone: &TimeZone, s: &str) -> f64 {
    let dt: DateTime = s.parse().unwrap();
    dt.to_zoned(zone.clone()).unwrap().timestamp().as_second() as f64
}

/// What a zone's clock reads at `v`, to the minute.
fn reads(zone: &TimeZone, v: f64) -> String {
    let z = Timestamp::from_second(v as i64)
        .unwrap()
        .to_zoned(zone.clone());
    z.strftime("%Y-%m-%d %H:%M").to_string()
}

fn labels(ts: &[Tick]) -> Vec<&str> {
    ts.iter().map(|t| t.label.as_str()).collect()
}

fn values(ts: &[Tick]) -> Vec<f64> {
    ts.iter().map(|t| t.value).collect()
}

fn majors(ts: &[Tick]) -> Vec<bool> {
    ts.iter().map(|t| t.major).collect()
}

fn gaps(ts: &[Tick]) -> Vec<f64> {
    ts.windows(2).map(|w| w[1].value - w[0].value).collect()
}

fn linear(lo: f64, hi: f64) -> Scale {
    Scale::Linear {
        domain: (lo, hi),
        range: (0.0, 500.0),
    }
}

fn log(lo: f64, hi: f64) -> Scale {
    Scale::Log {
        domain: (lo, hi),
        range: (0.0, 300.0),
        base: 10.0,
    }
}

fn time(lo: f64, hi: f64, zone: &TimeZone) -> Scale {
    Scale::Time {
        domain: (lo, hi),
        range: (0.0, 800.0),
        zone: zone.clone(),
    }
}

fn band(names: &[&str], range: (f32, f32), inner: f32, outer: f32) -> Scale {
    Scale::Band {
        domain: names.iter().map(|&n| n.into()).collect(),
        range,
        inner,
        outer,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

// ---------------------------------------------------------------------------------- nice

#[test]
fn nice_widens_a_domain_to_steps_of_one_two_or_five() {
    assert_eq!(scale::nice(0.0, 1100.0, 5), (0.0, 1200.0, 200.0));
    assert_eq!(scale::nice(0.13, 0.87, 5), (0.0, 1.0, 0.2));
    assert_eq!(scale::nice(-3.2, 7.9, 10), (-4.0, 8.0, 1.0));
    assert_eq!(scale::nice(3.0, 870.0, 4), (0.0, 1000.0, 200.0));
    // Reversed stays reversed.
    assert_eq!(scale::nice(1100.0, 0.0, 5), (1200.0, 0.0, 200.0));
}

#[test]
fn nice_gives_a_single_value_room_and_nonsense_a_unit() {
    assert_eq!(scale::nice(5.0, 5.0, 5), (4.4, 5.6, 0.2));
    assert_eq!(scale::nice(0.0, 0.0, 5), (-1.0, 1.0, 0.5));
    assert_eq!(scale::nice(NAN, 1.0, 5), (0.0, 1.0, 0.2));
    assert_eq!(scale::nice(0.0, f64::INFINITY, 5), (0.0, 1.0, 0.2));
    // A count of nothing is a count of one.
    assert_eq!(scale::nice(0.0, 7.0, 0), (0.0, 10.0, 10.0));
}

// ---------------------------------------------------------------------------------- map

#[test]
fn a_linear_scale_maps_and_inverts_proportionally() {
    let s = Scale::Linear {
        domain: (0.0, 10.0),
        range: (0.0, 200.0),
    };
    assert_eq!(s.map(5.0), 100.0);
    assert_eq!(s.map(-1.0), -20.0);
    assert_eq!(s.invert(50.0), 2.5);
    assert_eq!(s.domain(), (0.0, 10.0));
    assert_eq!(s.range(), (0.0, 200.0));
    // A y axis runs up the screen.
    let y = s.with_range((200.0, 0.0));
    assert_eq!(y.map(2.0), 160.0);
    assert_eq!(y.invert(160.0), 2.0);
    assert_eq!(s.with_domain((0.0, 20.0)).map(5.0), 50.0);
    assert_eq!(s.with_range((0.0, 400.0)).map(5.0), 200.0);
    // One value maps to the middle; one place inverts to the domain's start.
    let flat = Scale::Linear {
        domain: (3.0, 3.0),
        range: (0.0, 200.0),
    };
    assert_eq!(flat.map(3.0), 100.0);
    assert_eq!(s.with_range((7.0, 7.0)).invert(7.0), 0.0);
}

#[test]
fn a_time_scale_maps_its_seconds_like_a_linear_one() {
    let t0 = at("2025-11-21T06:00:00Z");
    let s = time(t0, t0 + 6.0 * HOUR, &utc());
    assert_eq!(s.map(t0 + 3.0 * HOUR), 400.0);
    assert!(close(s.invert(200.0), t0 + 1.5 * HOUR));
    assert_eq!(
        s.with_domain((t0, t0 + 3.0 * HOUR)).map(t0 + 3.0 * HOUR),
        800.0
    );
}

#[test]
fn a_log_scale_maps_by_the_logarithm() {
    let s = log(1.0, 1000.0);
    assert!((s.map(10.0) - 100.0).abs() < 1e-3);
    assert!((s.map(100.0) - 200.0).abs() < 1e-3);
    assert!(close(s.invert(150.0), 10f64.sqrt() * 10.0));
    assert!(s.map(0.0).is_nan());
    assert!(s.map(-5.0).is_nan());
    // Below zero it's the mirror image.
    let neg = log(-1000.0, -1.0);
    assert!((neg.map(-10.0) - 200.0).abs() < 1e-3);
    assert!(close(neg.invert(200.0), -10.0));
    // A domain that touches zero has no logarithm.
    assert!(log(0.0, 100.0).map(10.0).is_nan());
    assert!(log(0.0, 100.0).invert(10.0).is_nan());
    assert!(scale::ticks(&log(0.0, 100.0), 5).is_empty());
}

#[test]
fn a_band_scale_places_bands_with_their_paddings() {
    let s = band(&["a", "b", "c", "d"], (0.0, 100.0), 0.2, 0.1);
    // A step of 100 / (4 - 0.2 + 2 × 0.1) = 25, bands 20 wide, starting 2.5 in.
    assert_eq!(s.band_of(0), Some((2.5, 20.0)));
    assert_eq!(s.band_of(3), Some((77.5, 20.0)));
    assert_eq!(s.band_of(4), None);
    assert_eq!(s.domain(), (0.0, 4.0));
    // Places in bands: i + 0.5 is band i's centre.
    assert_eq!(s.map(0.5), 12.5);
    assert_eq!(s.map(2.5), 62.5);
    assert_eq!(s.invert(62.5), 2.5);
    // Between two bars the nearer one; past the ends none.
    assert_eq!(s.band_at(1.0), Some(0));
    assert_eq!(s.band_at(24.0), Some(0));
    assert_eq!(s.band_at(26.0), Some(1));
    assert_eq!(s.band_at(99.0), Some(3));
    assert_eq!(s.band_at(-1.0), None);
    assert_eq!(s.band_at(101.0), None);
    // Bands don't zoom by domain, and other scales have none.
    assert_eq!(s.with_domain((1.0, 2.0)), s);
    assert_eq!(linear(0.0, 1.0).band_of(0), None);
    assert_eq!(linear(0.0, 1.0).band_at(1.0), None);
}

#[test]
fn a_band_scale_on_a_reversed_range_counts_from_its_start() {
    let s = band(&["a", "b", "c", "d"], (100.0, 0.0), 0.2, 0.1);
    assert_eq!(s.band_of(0), Some((77.5, 20.0)));
    assert_eq!(s.band_of(3), Some((2.5, 20.0)));
    assert_eq!(s.band_at(90.0), Some(0));
    assert_eq!(s.band_at(10.0), Some(3));
    // One band with room for two sits in the middle.
    let one = band(&["only"], (0.0, 100.0), 0.5, 0.0);
    assert_eq!(one.band_of(0), Some((25.0, 50.0)));
    assert_eq!(band(&[], (0.0, 100.0), 0.1, 0.1).band_of(0), None);
}

#[test]
fn band_ticks_name_each_category_at_its_centre_or_every_nth() {
    let s = band(&["a", "b", "c", "d", "e"], (0.0, 100.0), 0.1, 0.0);
    let all = scale::ticks(&s, 10);
    assert_eq!(labels(&all), ["a", "b", "c", "d", "e"]);
    assert_eq!(values(&all), [0.5, 1.5, 2.5, 3.5, 4.5]);
    assert_eq!(labels(&scale::ticks(&s, 2)), ["a", "d"]);
    assert_eq!(labels(&scale::ticks(&s, 3)), ["a", "c", "e"]);
    assert!(scale::ticks(&s, 0).is_empty());
}

// ---------------------------------------------------------------------------------- linear ticks

#[test]
fn linear_ticks_are_exact_decimals_in_one_style() {
    let t = scale::ticks(&linear(0.0, 1.0), 5);
    assert_eq!(values(&t), [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    assert_eq!(labels(&t), ["0.0", "0.2", "0.4", "0.6", "0.8", "1.0"]);
    let t = scale::ticks(&linear(0.0, 0.3), 3);
    assert_eq!(values(&t), [0.0, 0.1, 0.2, 0.3]);
    let t = scale::ticks(&linear(0.0, 1000.0), 5);
    assert_eq!(labels(&t), ["0", "200", "400", "600", "800", "1,000"]);
    assert!(majors(&t).iter().all(|m| !m));
    let t = scale::ticks(&linear(-2.5, 2.5), 5);
    assert_eq!(labels(&t), ["−2", "−1", "0", "1", "2"]);
    // Only the ticks inside the domain; a reversed domain ticks the same.
    assert_eq!(
        values(&scale::ticks(&linear(0.13, 0.87), 5)),
        [0.2, 0.4, 0.6, 0.8]
    );
    assert_eq!(
        scale::ticks(&linear(1.0, 0.0), 5),
        scale::ticks(&linear(0.0, 1.0), 5)
    );
}

#[test]
fn an_axis_that_reaches_ten_thousand_is_compact_throughout() {
    let t = scale::ticks(&linear(0.0, 25_000.0), 5);
    assert_eq!(labels(&t), ["0", "5K", "10K", "15K", "20K", "25K"]);
    let t = scale::ticks(&linear(0.0, 9_000.0), 5);
    assert_eq!(labels(&t), ["0", "2,000", "4,000", "6,000", "8,000"]);
    let t = scale::ticks(&linear(9_000.0, 11_000.0), 4);
    assert_eq!(labels(&t), ["9K", "9.5K", "10K", "10.5K", "11K"]);
    let t = scale::ticks(&linear(0.0, 3_000_000.0), 6);
    assert_eq!(labels(&t), ["0", "500K", "1M", "1.5M", "2M", "2.5M", "3M"]);
    // Zoomed in to a few units of a large number, compact would need decimals: in full.
    let t = scale::ticks(&linear(12_340.0, 12_350.0), 5);
    assert_eq!(
        labels(&t),
        ["12,340", "12,342", "12,344", "12,346", "12,348", "12,350"]
    );
}

#[test]
fn degenerate_linear_domains_tick_once_or_not_at_all() {
    let t = scale::ticks(&linear(42.0, 42.0), 5);
    assert_eq!(labels(&t), ["42"]);
    assert!(scale::ticks(&linear(NAN, 1.0), 5).is_empty());
    assert!(scale::ticks(&linear(0.0, 1.0), 0).is_empty());
}

// ---------------------------------------------------------------------------------- log ticks

#[test]
fn log_ticks_are_powers_with_two_and_five_between_when_there_is_room() {
    let t = scale::ticks(&log(1.0, 1000.0), 5);
    assert_eq!(values(&t), [1.0, 10.0, 100.0, 1000.0]);
    assert_eq!(labels(&t), ["1", "10", "100", "1,000"]);
    assert!(majors(&t).iter().all(|&m| m));
    let t = scale::ticks(&log(1.0, 100.0), 10);
    assert_eq!(values(&t), [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);
    assert_eq!(majors(&t), [true, false, false, true, false, false, true]);
    let t = scale::ticks(&log(0.01, 1.0), 10);
    assert_eq!(
        labels(&t),
        ["0.01", "0.02", "0.05", "0.1", "0.2", "0.5", "1"]
    );
    assert_eq!(values(&t), [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0]);
}

#[test]
fn log_ticks_skip_powers_when_there_are_many_and_go_linear_inside_a_decade() {
    let t = scale::ticks(&log(1.0, 1e6), 3);
    assert_eq!(values(&t), [1.0, 100.0, 1e4, 1e6]);
    assert_eq!(labels(&t), ["1", "100", "10K", "1M"]);
    let t = scale::ticks(&log(20.0, 45.0), 5);
    assert_eq!(values(&t), [20.0, 25.0, 30.0, 35.0, 40.0, 45.0]);
    assert!(majors(&t).iter().all(|m| !m));
    let t = scale::ticks(&log(-1000.0, -1.0), 5);
    assert_eq!(labels(&t), ["−1,000", "−100", "−10", "−1"]);
    // Base 2: its powers.
    let two = Scale::Log {
        domain: (1.0, 64.0),
        range: (0.0, 100.0),
        base: 2.0,
    };
    assert_eq!(
        labels(&scale::ticks(&two, 10)),
        ["1", "2", "4", "8", "16", "32", "64"]
    );
}

#[test]
fn nice_scales_widen_to_their_own_units() {
    assert_eq!(linear(0.13, 0.87).nice(5).domain(), (0.0, 1.0));
    assert_eq!(log(3.0, 870.0).nice(5).domain(), (1.0, 1000.0));
    assert_eq!(log(-870.0, -3.0).nice(5).domain(), (-1000.0, -1.0));
    let t0 = at("2025-11-21T10:07:00Z");
    let s = time(t0, at("2025-11-21T13:52:00Z"), &utc()).nice(4);
    assert_eq!(
        s.domain(),
        (at("2025-11-21T10:00:00Z"), at("2025-11-21T14:00:00Z"))
    );
    let b = band(&["a"], (0.0, 1.0), 0.0, 0.0);
    assert_eq!(b.nice(5), b);
}

// ---------------------------------------------------------------------------------- time ticks

#[test]
fn time_ticks_fall_on_the_clock() {
    let z = utc();
    let t = scale::ticks(
        &time(at("2025-11-21T06:00:00Z"), at("2025-11-21T12:00:00Z"), &z),
        6,
    );
    assert_eq!(
        labels(&t),
        [
            "06:00", "07:00", "08:00", "09:00", "10:00", "11:00", "12:00"
        ]
    );
    assert!(majors(&t).iter().all(|m| !m));
    let t = scale::ticks(
        &time(at("2025-11-21T10:00:00Z"), at("2025-11-21T11:00:00Z"), &z),
        4,
    );
    assert_eq!(labels(&t), ["10:00", "10:15", "10:30", "10:45", "11:00"]);
    // Off the round times, the ticks start at the next one.
    let t = scale::ticks(
        &time(at("2025-11-21T10:07:00Z"), at("2025-11-21T11:52:00Z"), &z),
        4,
    );
    assert_eq!(labels(&t), ["10:30", "11:00", "11:30"]);
}

#[test]
fn seconds_and_their_fractions_label_only_what_changes() {
    let z = utc();
    let t = scale::ticks(
        &time(at("2025-11-21T10:25:00Z"), at("2025-11-21T10:26:00Z"), &z),
        4,
    );
    assert_eq!(labels(&t), ["10:25", ":15", ":30", ":45", "10:26"]);
    assert_eq!(majors(&t), [true, false, false, false, true]);
    let t = scale::ticks(
        &time(at("2025-11-21T10:25:15Z"), at("2025-11-21T10:25:16Z"), &z),
        4,
    );
    assert_eq!(labels(&t), [":15", ".200", ".400", ".600", ".800", ":16"]);
    assert_eq!(majors(&t), [true, false, false, false, false, true]);
}

#[test]
fn a_midnight_among_hours_is_major_and_names_its_day() {
    let z = utc();
    let t = scale::ticks(
        &time(at("2025-11-21T18:00:00Z"), at("2025-11-22T06:00:00Z"), &z),
        6,
    );
    assert_eq!(labels(&t), ["18:00", "21:00", "22 Nov", "03:00", "06:00"]);
    assert_eq!(majors(&t), [false, false, true, false, false]);
}

#[test]
fn days_weeks_months_and_years_tick_on_the_calendar() {
    let z = utc();
    let t = scale::ticks(
        &time(at("2025-11-17T00:00:00Z"), at("2025-11-24T00:00:00Z"), &z),
        7,
    );
    assert_eq!(
        labels(&t),
        [
            "17 Nov", "18 Nov", "19 Nov", "20 Nov", "21 Nov", "22 Nov", "23 Nov", "24 Nov"
        ]
    );
    let t = scale::ticks(
        &time(at("2025-11-28T00:00:00Z"), at("2025-12-04T00:00:00Z"), &z),
        6,
    );
    assert_eq!(
        labels(&t),
        [
            "28 Nov", "29 Nov", "30 Nov", "Dec", "2 Dec", "3 Dec", "4 Dec"
        ]
    );
    assert_eq!(majors(&t), [false, false, false, true, false, false, false]);
    // Weeks start on Monday.
    let t = scale::ticks(
        &time(at("2025-11-01T00:00:00Z"), at("2025-12-31T00:00:00Z"), &z),
        8,
    );
    assert_eq!(
        labels(&t),
        [
            "3 Nov", "10 Nov", "17 Nov", "24 Nov", "Dec", "8 Dec", "15 Dec", "22 Dec", "29 Dec"
        ]
    );
    let t = scale::ticks(
        &time(at("2026-01-01T00:00:00Z"), at("2026-12-31T00:00:00Z"), &z),
        12,
    );
    assert_eq!(
        labels(&t),
        [
            "2026", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
        ]
    );
    assert_eq!(majors(&t)[..2], [true, false]);
    let t = scale::ticks(
        &time(at("2025-01-01T00:00:00Z"), at("2026-12-31T00:00:00Z"), &z),
        8,
    );
    assert_eq!(
        labels(&t),
        ["2025", "Apr", "Jul", "Oct", "2026", "Apr", "Jul", "Oct"]
    );
    let t = scale::ticks(
        &time(at("2000-01-01T00:00:00Z"), at("2030-01-01T00:00:00Z"), &z),
        6,
    );
    assert_eq!(
        labels(&t),
        ["2000", "2005", "2010", "2015", "2020", "2025", "2030"]
    );
    assert!(majors(&t).iter().all(|m| !m));
}

#[test]
fn days_in_twos_restart_on_the_first() {
    let z = utc();
    let t = scale::ticks(
        &time(at("2025-11-25T00:00:00Z"), at("2025-12-08T00:00:00Z"), &z),
        6,
    );
    assert_eq!(
        labels(&t),
        [
            "25 Nov", "27 Nov", "29 Nov", "Dec", "3 Dec", "5 Dec", "7 Dec"
        ]
    );
}

#[test]
fn time_ticks_follow_their_zone() {
    // 06:00 to 12:00 UTC is 11:30 to 17:30 in India: the ticks are on India's hours.
    let india = TimeZone::fixed(tz::Offset::from_seconds(5 * 3600 + 1800).unwrap());
    let t = scale::ticks(
        &time(
            at("2025-11-21T06:00:00Z"),
            at("2025-11-21T12:00:00Z"),
            &india,
        ),
        6,
    );
    assert_eq!(
        labels(&t),
        ["12:00", "13:00", "14:00", "15:00", "16:00", "17:00"]
    );
    assert_eq!(t[0].value, at("2025-11-21T06:30:00Z"));
}

#[test]
fn london_spring_forward_skips_the_hour_the_clock_jumps() {
    let z = named("Europe/London");
    let lo = local(&z, "2026-03-28T22:00");
    let hi = local(&z, "2026-03-29T05:00");
    assert_eq!(hi - lo, 6.0 * HOUR, "the night is an hour short");
    let t = scale::ticks(&time(lo, hi, &z), 7);
    assert_eq!(
        labels(&t),
        [
            "22:00", "23:00", "29 Mar", "02:00", "03:00", "04:00", "05:00"
        ]
    );
    assert_eq!(majors(&t), [false, false, true, false, false, false, false]);
    // Each a real hour apart: 00:00 GMT to 02:00 BST is one.
    assert!(gaps(&t).iter().all(|&g| g == HOUR), "{:?}", gaps(&t));
    // Three-hourly on the day itself: 00:00 to 03:00 is two hours.
    let lo = local(&z, "2026-03-29T00:00");
    let hi = local(&z, "2026-03-29T12:00");
    let t = scale::ticks(&time(lo, hi, &z), 4);
    assert_eq!(labels(&t), ["29 Mar", "03:00", "06:00", "09:00", "12:00"]);
    assert_eq!(gaps(&t), [2.0 * HOUR, 3.0 * HOUR, 3.0 * HOUR, 3.0 * HOUR]);
}

#[test]
fn london_fall_back_shows_the_repeated_hour_twice() {
    let z = named("Europe/London");
    let lo = at("2026-10-24T21:00:00Z"); // 22:00 BST
    let hi = at("2026-10-25T04:00:00Z"); // 04:00 GMT
    let t = scale::ticks(&time(lo, hi, &z), 7);
    assert_eq!(
        labels(&t),
        [
            "22:00", "23:00", "25 Oct", "01:00", "01:00", "02:00", "03:00", "04:00"
        ]
    );
    assert!(gaps(&t).iter().all(|&g| g == HOUR), "{:?}", gaps(&t));
    assert_eq!(t[3].value, at("2026-10-25T00:00:00Z"), "01:00 BST");
    assert_eq!(t[4].value, at("2026-10-25T01:00:00Z"), "01:00 GMT");
}

#[test]
fn london_days_start_at_midnight_whatever_their_length() {
    let z = named("Europe/London");
    let t = scale::ticks(
        &time(
            local(&z, "2026-03-27T00:00"),
            local(&z, "2026-04-01T00:00"),
            &z,
        ),
        5,
    );
    assert_eq!(
        labels(&t),
        ["27 Mar", "28 Mar", "29 Mar", "30 Mar", "31 Mar", "Apr"]
    );
    assert!(t.iter().all(|t| reads(&z, t.value).ends_with("00:00")));
    // The 29th has 23 hours.
    assert_eq!(
        gaps(&t),
        [
            24.0 * HOUR,
            24.0 * HOUR,
            23.0 * HOUR,
            24.0 * HOUR,
            24.0 * HOUR
        ]
    );
    let t = scale::ticks(
        &time(
            local(&z, "2026-10-23T00:00"),
            local(&z, "2026-10-28T00:00"),
            &z,
        ),
        5,
    );
    assert_eq!(
        labels(&t),
        ["23 Oct", "24 Oct", "25 Oct", "26 Oct", "27 Oct", "28 Oct"]
    );
    // The 25th has 25.
    assert_eq!(
        gaps(&t),
        [
            24.0 * HOUR,
            24.0 * HOUR,
            25.0 * HOUR,
            24.0 * HOUR,
            24.0 * HOUR
        ]
    );
    assert_eq!(reads(&z, t[3].value), "2026-10-26 00:00");
    // Months start at local midnight too: 1 April is in BST, an hour before UTC's.
    let t = scale::ticks(
        &time(
            local(&z, "2026-01-01T00:00"),
            local(&z, "2026-12-31T00:00"),
            &z,
        ),
        12,
    );
    assert_eq!(t[3].label, "Apr");
    assert_eq!(t[3].value, at("2026-03-31T23:00:00Z"));
}

#[test]
fn sydney_spring_forward_and_fall_back() {
    let z = named("Australia/Sydney");
    // 4 October 2026: 02:00 AEST becomes 03:00 AEDT.
    let lo = local(&z, "2026-10-04T00:00");
    let hi = local(&z, "2026-10-04T05:00");
    assert_eq!(hi - lo, 4.0 * HOUR);
    let t = scale::ticks(&time(lo, hi, &z), 4);
    assert_eq!(labels(&t), ["4 Oct", "01:00", "03:00", "04:00", "05:00"]);
    assert!(gaps(&t).iter().all(|&g| g == HOUR), "{:?}", gaps(&t));
    // 5 April 2026: 03:00 AEDT becomes 02:00 AEST, so 02:00 comes twice.
    let lo = local(&z, "2026-04-05T00:00");
    let hi = at("2026-04-04T18:00:00Z"); // 04:00 AEST
    assert_eq!(hi - lo, 5.0 * HOUR);
    let t = scale::ticks(&time(lo, hi, &z), 5);
    assert_eq!(
        labels(&t),
        ["5 Apr", "01:00", "02:00", "02:00", "03:00", "04:00"]
    );
    assert!(gaps(&t).iter().all(|&g| g == HOUR), "{:?}", gaps(&t));
}

#[test]
fn sydney_days_are_23_and_25_hours_at_the_changes() {
    let z = named("Australia/Sydney");
    let t = scale::ticks(
        &time(
            local(&z, "2026-10-02T00:00"),
            local(&z, "2026-10-06T00:00"),
            &z,
        ),
        4,
    );
    assert_eq!(labels(&t), ["2 Oct", "3 Oct", "4 Oct", "5 Oct", "6 Oct"]);
    assert_eq!(
        gaps(&t),
        [24.0 * HOUR, 24.0 * HOUR, 23.0 * HOUR, 24.0 * HOUR]
    );
    let t = scale::ticks(
        &time(
            local(&z, "2026-04-03T00:00"),
            local(&z, "2026-04-07T00:00"),
            &z,
        ),
        4,
    );
    assert_eq!(labels(&t), ["3 Apr", "4 Apr", "5 Apr", "6 Apr", "7 Apr"]);
    assert_eq!(
        gaps(&t),
        [24.0 * HOUR, 24.0 * HOUR, 25.0 * HOUR, 24.0 * HOUR]
    );
    assert!(t.iter().all(|t| reads(&z, t.value).ends_with("00:00")));
    // Three-hourly over the spring day: 00:00 to 03:00 is two hours, then three.
    let t = scale::ticks(
        &time(
            local(&z, "2026-10-04T00:00"),
            local(&z, "2026-10-04T12:00"),
            &z,
        ),
        4,
    );
    assert_eq!(labels(&t), ["4 Oct", "03:00", "06:00", "09:00", "12:00"]);
    assert_eq!(gaps(&t)[0], 2.0 * HOUR);
}

#[test]
fn degenerate_time_domains_tick_once_or_not_at_all() {
    let t0 = at("2025-11-21T10:25:00Z");
    assert_eq!(labels(&scale::ticks(&time(t0, t0, &utc()), 5)), ["10:25"]);
    assert!(scale::ticks(&time(NAN, t0, &utc()), 5).is_empty());
    let late = scale::ticks(&time(t0 + 6.0 * HOUR, t0, &utc()), 6);
    assert_eq!(late, scale::ticks(&time(t0, t0 + 6.0 * HOUR, &utc()), 6));
}

// ---------------------------------------------------------------------------------- numbers

#[test]
fn format_number_writes_an_axis_in_full_or_compact() {
    assert_eq!(format_number(1284.0, 1.0, NumberStyle::Full), "1,284");
    assert_eq!(format_number(-1284.0, 1.0, NumberStyle::Full), "−1,284");
    assert_eq!(format_number(0.5, 0.25, NumberStyle::Full), "0.50");
    assert_eq!(format_number(-0.0001, 0.01, NumberStyle::Full), "0.00");
    assert_eq!(format_number(1234.5678, NAN, NumberStyle::Full), "1,234.57");
    assert_eq!(
        format_number(12_900.0, 100.0, NumberStyle::Compact),
        "12.9K"
    );
    assert_eq!(
        format_number(4_200_000.0, 100_000.0, NumberStyle::Compact),
        "4.2M"
    );
    assert_eq!(format_number(2.5e9, 5e8, NumberStyle::Compact), "2.5B");
    assert_eq!(format_number(3e12, 1e12, NumberStyle::Compact), "3T");
    assert_eq!(
        format_number(-15_000.0, 5_000.0, NumberStyle::Compact),
        "−15K"
    );
    assert_eq!(format_number(500.0, 500.0, NumberStyle::Compact), "500");
    // A figure on its own: three significant digits, and 999,950 rounds up to the next suffix.
    assert_eq!(format_number(12_900.0, NAN, NumberStyle::Compact), "12.9K");
    assert_eq!(
        format_number(1_284_000.0, NAN, NumberStyle::Compact),
        "1.28M"
    );
    assert_eq!(format_number(999_950.0, NAN, NumberStyle::Compact), "1M");
    assert_eq!(format_number(182.0, NAN, NumberStyle::Compact), "182");
    assert_eq!(format_number(999.9999, NAN, NumberStyle::Compact), "1K");
    // Past what figures write well, either style is scientific.
    assert_eq!(format_number(1e300, NAN, NumberStyle::Compact), "1e300");
    assert_eq!(format_number(2e-10, 1e-10, NumberStyle::Full), "2e−10");
    assert_eq!(format_number(NAN, 1.0, NumberStyle::Full), "—");
    assert_eq!(
        format_number(f64::NEG_INFINITY, 1.0, NumberStyle::Compact),
        "−∞"
    );
}

#[test]
fn axis_style_is_compact_from_ten_thousand() {
    assert_eq!(axis_style(1284.0, NAN), NumberStyle::Full);
    assert_eq!(axis_style(9_999.0, 1_000.0), NumberStyle::Full);
    assert_eq!(axis_style(10_000.0, 2_000.0), NumberStyle::Compact);
    assert_eq!(axis_style(12_900.0, NAN), NumberStyle::Compact);
    assert_eq!(axis_style(1_500_000.0, 250_000.0), NumberStyle::Compact);
    // Unless compact labels would need three decimals.
    assert_eq!(axis_style(12_350.0, 2.0), NumberStyle::Full);
    assert_eq!(axis_style(NAN, 1.0), NumberStyle::Full);
}

#[test]
fn format_value_keeps_the_datas_precision() {
    assert_eq!(format_value(3412.0), "3,412");
    assert_eq!(format_value(0.41), "0.41");
    assert_eq!(format_value(0.1 + 0.2), "0.3");
    assert_eq!(format_value(1234.5678), "1,234.57");
    assert_eq!(format_value(-2.5), "−2.5");
    assert_eq!(format_value(1_284_000.0), "1,284,000");
    assert_eq!(format_value(0.000_123_456), "0.000123456");
    assert_eq!(format_value(1.5e-7), "1.5e−7");
    assert_eq!(format_value(2.5e18), "2.5e18");
    assert_eq!(format_value(0.0), "0");
    assert_eq!(format_value(-0.0), "0");
    assert_eq!(format_value(NAN), "—");
    assert_eq!(format_value(f64::INFINITY), "∞");
}

#[test]
fn a_column_gets_the_decimals_its_values_need() {
    assert_eq!(data_decimals(&[1.0, 2.5, 3.25]), 2);
    assert_eq!(data_decimals(&[1040.0, 980.0]), 0);
    assert_eq!(data_decimals(&[NAN, 0.1 + 0.2]), 1);
    assert_eq!(data_decimals(&[0.000_001]), 6);
    assert_eq!(data_decimals(&[]), 0);
    assert_eq!(format_fixed(1040.5, 2), "1,040.50");
    assert_eq!(format_fixed(-3.0, 0), "−3");
    assert_eq!(format_fixed(NAN, 2), "—");
}

// ---------------------------------------------------------------------------------- times

#[test]
fn format_time_writes_as_little_as_the_step_allows() {
    let z = utc();
    let v = at("2025-11-21T10:25:00Z");
    assert_eq!(format_time(v, 300.0, &z), "10:25");
    assert_eq!(format_time(v, 15.0, &z), "10:25");
    assert_eq!(format_time(v + 15.0, 15.0, &z), ":15");
    assert_eq!(format_time(v + 15.25, 0.05, &z), ".250");
    assert_eq!(format_time(v, DAY, &z), "21 Nov");
    assert_eq!(format_time(at("2025-11-21T00:00:00Z"), HOUR, &z), "21 Nov");
    assert_eq!(format_time(v, 30.0 * DAY, &z), "Nov");
    assert_eq!(format_time(at("2026-01-01T00:00:00Z"), DAY, &z), "2026");
    assert_eq!(format_time(v, 365.0 * DAY, &z), "2025");
    // With no step the time chooses.
    assert_eq!(format_time(v, NAN, &z), "10:25");
    assert_eq!(format_time(NAN, 60.0, &z), "—");
}

#[test]
fn format_time_full_is_the_cards_header() {
    let z = utc();
    let v = at("2025-11-21T10:25:00Z");
    assert_eq!(format_time_full(v, 300.0, &z), "10:25 · Fri 21 Nov");
    assert_eq!(format_time_full(v + 15.0, 1.0, &z), "10:25:15 · Fri 21 Nov");
    assert_eq!(
        format_time_full(v + 0.25, 0.001, &z),
        "10:25:00.250 · Fri 21 Nov"
    );
    assert_eq!(format_time_full(v, DAY, &z), "Fri 21 Nov 2025");
    assert_eq!(format_time_full(v, 7.0 * DAY, &z), "Fri 21 Nov 2025");
    assert_eq!(format_time_full(v, 30.0 * DAY, &z), "Nov 2025");
    assert_eq!(format_time_full(v, 365.0 * DAY, &z), "2025");
    assert_eq!(format_time_full(v, NAN, &z), "10:25 · Fri 21 Nov");
    assert_eq!(
        format_time_full(at("2025-11-21T00:00:00Z"), NAN, &z),
        "Fri 21 Nov 2025"
    );
    // The zone decides the clock and even the day.
    assert_eq!(
        format_time_full(v, 60.0, &named("Europe/London")),
        "10:25 · Fri 21 Nov"
    );
    assert_eq!(
        format_time_full(v, 60.0, &named("Australia/Sydney")),
        "21:25 · Fri 21 Nov"
    );
    let late = at("2025-11-21T20:00:00Z");
    assert_eq!(
        format_time_full(late, 60.0, &named("Australia/Sydney")),
        "07:00 · Sat 22 Nov"
    );
}

#[test]
fn format_time_point_writes_every_field_to_the_step() {
    let z = utc();
    let v = at("2025-11-21T10:25:00Z");
    assert_eq!(format_time_point(v, 300.0, &z), "10:25");
    assert_eq!(format_time_point(v + 15.0, 1.0, &z), "10:25:15");
    assert_eq!(format_time_point(v + 15.25, 0.01, &z), "10:25:15.250");
    assert_eq!(
        format_time_point(at("2025-11-21T00:00:00Z"), 300.0, &z),
        "00:00"
    );
    assert_eq!(format_time_point(v, DAY, &z), "21 Nov");
    assert_eq!(format_time_point(v, 30.0 * DAY, &z), "Nov 2025");
    assert_eq!(format_time_point(v, 365.0 * DAY, &z), "2025");
}

// ---------------------------------------------------------------------------------- formats

#[test]
fn format_with_reads_strtime_for_times() {
    let z = utc();
    let v = at("2025-11-21T10:25:00Z");
    assert_eq!(format_with(v, "%H:%M", &z), "10:25");
    assert_eq!(format_with(v, "%a %d %b %Y", &z), "Fri 21 Nov 2025");
    assert_eq!(
        format_with(v, "%Y-%m-%dT%H:%M", &named("Australia/Sydney")),
        "2025-11-21T21:25"
    );
    assert_eq!(format_with(NAN, "%H:%M", &z), "—");
}

#[test]
fn format_with_reads_d3s_numbers() {
    let z = utc();
    assert_eq!(format_with(1234.5678, ",.2f", &z), "1,234.57");
    assert_eq!(format_with(1234.5678, ".2f", &z), "1234.57");
    assert_eq!(format_with(1234.5678, ",", &z), "1,234.57");
    assert_eq!(format_with(2.5, ".2f", &z), "2.50");
    assert_eq!(format_with(2.5, ".2~f", &z), "2.5");
    assert_eq!(format_with(-2.5, ".1f", &z), "−2.5");
    assert_eq!(format_with(3.7, "d", &z), "4");
    assert_eq!(format_with(1_234_567.0, ",d", &z), "1,234,567");
    assert_eq!(format_with(0.41, ".1%", &z), "41.0%");
    assert_eq!(format_with(0.41, "%", &z), "41%");
    assert_eq!(format_with(123.4, ",.0%", &z), "12,340%");
    assert_eq!(format_with(1500.0, "s", &z), "1.5k");
    assert_eq!(format_with(1500.0, ".3s", &z), "1.50k");
    assert_eq!(format_with(0.000_25, "s", &z), "250µ");
    assert_eq!(format_with(-2.5e6, ".2s", &z), "−2.5M");
    assert_eq!(format_with(999_950.0, "s", &z), "1M");
    assert_eq!(format_with(999.9999, ".3s", &z), "1.00k");
    assert_eq!(format_with(42.0, "s", &z), "42");
    // What isn't a format is ignored.
    assert_eq!(format_with(42.0, "bogus", &z), "42");
    assert_eq!(format_with(1234.5, "", &z), "1,234.5");
    assert_eq!(format_with(NAN, ",.2f", &z), "—");
}
