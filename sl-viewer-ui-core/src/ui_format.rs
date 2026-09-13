//! How a value reads in a cell: the renderings that are about presentation
//! rather than about any one panel.
//!
//! Everything here is pure — a value in, a string out — so it is unit-tested
//! without a world and shared by whichever panels show the same kind of number.

use std::time::Duration;

/// Seconds in a minute.
const SECS_PER_MINUTE: u64 = 60;

/// Minutes in an hour.
const MINUTES_PER_HOUR: u64 = 60;

/// Hours in a day.
const HOURS_PER_DAY: u64 = 24;

/// Microseconds in a millisecond.
const MICROS_PER_MILLI: u32 = 1_000;

/// Render a duration in mixed units, most significant first, with **every**
/// non-zero unit — `2d 3h 5m 12s`, `1h 2m 5s 400ms`, `4s 210ms`, `123ms`,
/// `420µs`.
///
/// This is `humantime`'s own rendering: a unit that carries nothing is left out
/// entirely, and the ones that carry something are all shown, so the value reads
/// exactly rather than to a rounded "about an hour". A zero duration reads
/// `0µs`, because "the simulator reported zero" and "the simulator reported
/// nothing" are different answers and a list shows both.
///
/// Microseconds are the floor: nothing this renders comes from a finer clock,
/// and a nanosecond tail would be noise in a cell.
#[must_use]
pub fn format_duration_units(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let units = [
        (
            total_secs / (HOURS_PER_DAY * MINUTES_PER_HOUR * SECS_PER_MINUTE),
            "d",
        ),
        (
            total_secs / (MINUTES_PER_HOUR * SECS_PER_MINUTE) % HOURS_PER_DAY,
            "h",
        ),
        (total_secs / SECS_PER_MINUTE % MINUTES_PER_HOUR, "m"),
        (total_secs % SECS_PER_MINUTE, "s"),
        (u64::from(duration.subsec_millis()), "ms"),
        (u64::from(duration.subsec_micros() % MICROS_PER_MILLI), "µs"),
    ];
    let rendered = units
        .iter()
        .filter(|(value, _unit)| *value > 0)
        .map(|(value, unit)| format!("{value}{unit}"))
        .collect::<Vec<String>>()
        .join(" ");
    if rendered.is_empty() {
        return "0µs".to_owned();
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::format_duration_units;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// Every non-zero unit, most significant first — the value exactly, not
    /// rounded to the two largest units.
    #[test]
    fn a_duration_reads_in_every_unit_it_uses() {
        assert_eq!(
            format_duration_units(Duration::from_millis(3_725_400)),
            "1h 2m 5s 400ms"
        );
        assert_eq!(format_duration_units(Duration::from_secs(3_600)), "1h");
        assert_eq!(format_duration_units(Duration::from_secs(245)), "4m 5s");
        assert_eq!(
            format_duration_units(Duration::from_millis(4_210)),
            "4s 210ms"
        );
        assert_eq!(format_duration_units(Duration::from_millis(123)), "123ms");
        assert_eq!(format_duration_units(Duration::from_micros(420)), "420µs");
    }

    /// A unit that carries nothing is left out, not written as a zero — so an
    /// hour and a half is `1h 30m`, and an hour and half a second is
    /// `1h 500ms`.
    #[test]
    fn an_empty_unit_is_left_out() {
        assert_eq!(format_duration_units(Duration::from_secs(5_400)), "1h 30m");
        assert_eq!(
            format_duration_units(Duration::from_millis(3_600_500)),
            "1h 500ms"
        );
    }

    /// A day is a unit too: a script time of sixteen minutes is one thing to
    /// read, and one of two days is another.
    #[test]
    fn a_long_duration_reads_in_days() {
        assert_eq!(
            format_duration_units(Duration::from_secs(2 * 86_400 + 3 * 3_600)),
            "2d 3h"
        );
    }

    /// Zero is a reported value, not an absent one.
    #[test]
    fn zero_reads_as_zero() {
        assert_eq!(format_duration_units(Duration::ZERO), "0µs");
    }
}
