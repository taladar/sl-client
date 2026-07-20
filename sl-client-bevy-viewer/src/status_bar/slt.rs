//! Second Life Time (SLT) conversion: the current UTC instant → the US Pacific
//! wall-clock components the status area's clock shows.
//!
//! The reference viewer's status bar always shows SLT — US Pacific time —
//! regardless of the user's own zone. SLT observes US daylight saving: Pacific
//! Standard Time (`UTC-8`) in winter and Pacific Daylight Time (`UTC-7`) in
//! summer, switching on the second Sunday of March (02:00 local → 03:00) and the
//! first Sunday of November (02:00 local → 01:00). This module carries that
//! calendar math so the clock reads correctly year-round without pulling in a
//! time-zone database.
//!
//! The date arithmetic is Howard Hinnant's civil-from-days / days-from-civil
//! algorithm (`http://howardhinnant.github.io/date_algorithms.html`), adapted to
//! `i64`. Values here are tiny (days and seconds around the Unix epoch), so the
//! saturating helpers used to satisfy the `arithmetic_side_effects` lint never
//! actually clamp; division and remainder are by non-zero literals on
//! non-negative values, which the lint permits.

use sl_l10n::CivilDateTime;

/// Seconds in a day.
const SECS_PER_DAY: i64 = 86_400;

/// Saturating `i64` addition, keeping the calendar math clear of the
/// `arithmetic_side_effects` lint (every value here is far inside `i64`'s range).
const fn add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}

/// Saturating `i64` subtraction (see [`add`]).
const fn sub(a: i64, b: i64) -> i64 {
    a.saturating_sub(b)
}

/// Saturating `i64` multiplication (see [`add`]).
const fn mul(a: i64, b: i64) -> i64 {
    a.saturating_mul(b)
}

/// The current UTC instant as whole seconds since the Unix epoch, or `0` if the
/// system clock is set before the epoch (which the caller then renders as the
/// epoch — a harmless clock read, never a panic).
#[must_use]
pub(crate) fn now_unix() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_u64, |elapsed| elapsed.as_secs());
    i64::try_from(secs).unwrap_or(0)
}

/// The civil (year, month, day) for a count of days since the Unix epoch
/// (1970-01-01 = day 0), by Hinnant's `civil_from_days`.
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = add(days, 719_468);
    let era = (if z >= 0 { z } else { sub(z, 146_096) }) / 146_097;
    let doe = sub(z, mul(era, 146_097)); // [0, 146096]
    let yoe = sub(add(sub(doe, doe / 1_460), doe / 36_524), doe / 146_096) / 365; // [0, 399]
    let year = add(yoe, mul(era, 400));
    // `doy = doe - (365*yoe + yoe/4 - yoe/100)`
    let doy = sub(doe, sub(add(mul(365, yoe), yoe / 4), yoe / 100)); // [0, 365]
    let mp = add(mul(5, doy), 2) / 153; // [0, 11]
    let day = add(sub(doy, add(mul(153, mp), 2) / 5), 1); // [1, 31]
    let month = if mp < 10 { add(mp, 3) } else { sub(mp, 9) }; // [1, 12]
    let year = if month <= 2 { add(year, 1) } else { year };
    (year, month, day)
}

/// The count of days since the Unix epoch for a civil date, by Hinnant's
/// `days_from_civil`.
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { sub(year, 1) } else { year };
    let era = (if y >= 0 { y } else { sub(y, 399) }) / 400;
    let yoe = sub(y, mul(era, 400)); // [0, 399]
    let mm = if month > 2 {
        sub(month, 3)
    } else {
        add(month, 9)
    };
    let doy = sub(add(add(mul(153, mm), 2) / 5, day), 1); // [0, 365]
    let doe = add(sub(add(mul(yoe, 365), yoe / 4), yoe / 100), doy); // [0, 146096]
    sub(add(mul(era, 146_097), doe), 719_468)
}

/// The day of the week for a count of days since the Unix epoch, with Sunday
/// `= 0` … Saturday `= 6`. The epoch (day 0) is a Thursday (`= 4`).
const fn weekday_from_days(days: i64) -> i64 {
    add(days % 7, 4) % 7
}

/// The day-of-month of the `n`-th `weekday` (Sunday `= 0`) of `month` in `year`,
/// e.g. the 2nd Sunday of March.
const fn nth_weekday_of_month(year: i64, month: i64, weekday: i64, n: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    let first_dow = weekday_from_days(first);
    let offset = sub(add(weekday, 7), first_dow) % 7; // 0..6 days to the first such weekday
    add(add(1, offset), mul(sub(n, 1), 7))
}

/// The Unix second at which US daylight saving **starts** in `year` — the second
/// Sunday of March at 02:00 PST, i.e. 10:00 UTC.
const fn dst_start_unix(year: i64) -> i64 {
    let day = nth_weekday_of_month(year, 3, 0, 2);
    add(
        mul(days_from_civil(year, 3, day), SECS_PER_DAY),
        mul(10, 3_600),
    )
}

/// The Unix second at which US daylight saving **ends** in `year` — the first
/// Sunday of November at 02:00 PDT, i.e. 09:00 UTC.
const fn dst_end_unix(year: i64) -> i64 {
    let day = nth_weekday_of_month(year, 11, 0, 1);
    add(
        mul(days_from_civil(year, 11, day), SECS_PER_DAY),
        mul(9, 3_600),
    )
}

/// The SLT (US Pacific) UTC offset in seconds for a Unix instant: `-7h` (PDT)
/// inside the daylight-saving window, `-8h` (PST) outside it.
const fn slt_offset_seconds(unix: i64) -> i64 {
    let (year, _, _) = civil_from_days(unix / SECS_PER_DAY);
    if unix >= dst_start_unix(year) && unix < dst_end_unix(year) {
        mul(-7, 3_600)
    } else {
        mul(-8, 3_600)
    }
}

/// Convert a Unix instant to the SLT (US Pacific) wall-clock components the
/// status area's clock renders.
#[must_use]
pub(crate) fn current_slt(unix: i64) -> CivilDateTime {
    let local = add(unix, slt_offset_seconds(unix));
    let days = local / SECS_PER_DAY;
    let secs = local % SECS_PER_DAY;
    let (year, month, day) = civil_from_days(days);
    CivilDateTime {
        year: i32::try_from(year).unwrap_or(1970),
        month: u8::try_from(month).unwrap_or(1),
        day: u8::try_from(day).unwrap_or(1),
        hour: u8::try_from(secs / 3_600).unwrap_or(0),
        minute: u8::try_from(secs % 3_600 / 60).unwrap_or(0),
        second: u8::try_from(secs % 60).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, current_slt, days_from_civil, nth_weekday_of_month};
    use pretty_assertions::assert_eq;

    /// `civil_from_days` and `days_from_civil` round-trip across a range of
    /// dates, so the calendar math is self-consistent.
    #[test]
    fn civil_days_round_trip() {
        for days in [0_i64, 1, 365, 18_628, 20_000, 25_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    /// A few known epoch-day dates decode correctly.
    #[test]
    fn known_dates_decode() {
        // 1970-01-01 is day 0.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2021-01-01 is 18628 days after the epoch.
        assert_eq!(civil_from_days(18_628), (2021, 1, 1));
    }

    /// The nth-weekday helper finds the US daylight-saving transition days.
    #[test]
    fn dst_transition_days() {
        // 2021: DST starts Sun Mar 14, ends Sun Nov 7.
        assert_eq!(nth_weekday_of_month(2021, 3, 0, 2), 14);
        assert_eq!(nth_weekday_of_month(2021, 11, 0, 1), 7);
        // 2026: DST starts Sun Mar 8, ends Sun Nov 1.
        assert_eq!(nth_weekday_of_month(2026, 3, 0, 2), 8);
        assert_eq!(nth_weekday_of_month(2026, 11, 0, 1), 1);
    }

    /// A winter instant reads as PST (`UTC-8`).
    #[test]
    fn winter_is_pst() {
        // 2021-01-01 00:00:00 UTC → 2020-12-31 16:00:00 PST.
        let when = current_slt(1_609_459_200);
        assert_eq!(
            (when.year, when.month, when.day, when.hour, when.minute),
            (2020, 12, 31, 16, 0)
        );
    }

    /// A summer instant reads as PDT (`UTC-7`).
    #[test]
    fn summer_is_pdt() {
        // 2021-07-01 00:00:00 UTC → 2021-06-30 17:00:00 PDT.
        let when = current_slt(1_625_097_600);
        assert_eq!(
            (when.year, when.month, when.day, when.hour, when.minute),
            (2021, 6, 30, 17, 0)
        );
    }

    /// The spring-forward boundary: one minute before the transition is still
    /// PST; one minute after is PDT (the local clock jumps 01:59 → 03:00).
    #[test]
    fn spring_forward_boundary() {
        // 2021 DST start is Sun Mar 14 10:00:00 UTC (= 02:00 PST → 03:00 PDT).
        // 09:59 UTC → 01:59 PST.
        let before = current_slt(1_615_715_940);
        assert_eq!((before.hour, before.minute), (1, 59));
        // 10:00 UTC → 03:00 PDT.
        let after = current_slt(1_615_716_000);
        assert_eq!((after.hour, after.minute), (3, 0));
    }

    /// The fall-back boundary: one minute before the transition is PDT; one
    /// minute after is PST (the local clock falls 01:59 → 01:00).
    #[test]
    fn fall_back_boundary() {
        // 2021 DST end is Sun Nov 7 09:00:00 UTC (= 02:00 PDT → 01:00 PST).
        // 08:59 UTC → 01:59 PDT.
        let before = current_slt(1_636_275_540);
        assert_eq!((before.hour, before.minute), (1, 59));
        // 09:00 UTC → 01:00 PST.
        let after = current_slt(1_636_275_600);
        assert_eq!((after.hour, after.minute), (1, 0));
    }
}
