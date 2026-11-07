//! Calendar time for the clocks a script reads, on the `time` crate: UTC
//! dates and timestamps, and US Pacific time, which Second Life's
//! `llGetWallclock`, OpenSim's `osGetPSTWallclock` and both grids' login
//! (`sl-fake-grid`'s `daylight_savings`) answer in.

use core::fmt::Write as _;

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime};

/// Seconds in a day.
pub(crate) const DAY_SECONDS: i64 = 86_400;

/// The calendar instant `micros` microseconds after the epoch, or the epoch
/// for one `time` cannot represent.
fn instant(micros: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros).saturating_mul(1000))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// The UTC date of `micros`, `YYYY-MM-DD`.
pub(crate) fn date(micros: i64) -> String {
    let at = instant(micros);
    format!(
        "{:04}-{:02}-{:02}",
        at.year(),
        u8::from(at.month()),
        at.day()
    )
}

/// The `nth` (1-based) Sunday of `month` in `year`.
fn nth_sunday(year: i32, month: Month, nth: u8) -> Option<Date> {
    let first = Date::from_calendar_date(year, month, 1).ok()?;
    let offset = first.weekday().number_days_from_sunday();
    let first_sunday = if offset == 0 {
        1
    } else {
        8_u8.checked_sub(offset)?
    };
    let day = first_sunday.checked_add(nth.checked_sub(1)?.checked_mul(7)?)?;
    Date::from_calendar_date(year, month, day).ok()
}

/// Whether US Pacific daylight-saving time is in effect at UNIX time `unix`:
/// from 02:00 local on the second Sunday of March (10:00 UTC) to 02:00 local
/// on the first Sunday of November (09:00 UTC).
#[must_use]
pub fn pacific_daylight_saving(unix: i64) -> bool {
    let Ok(at) = OffsetDateTime::from_unix_timestamp(unix) else {
        return false;
    };
    let year = at.year();
    let start = nth_sunday(year, Month::March, 2).map(|day| day.with_hms(10, 0, 0));
    let end = nth_sunday(year, Month::November, 1).map(|day| day.with_hms(9, 0, 0));
    match (start, end) {
        (Some(Ok(start)), Some(Ok(end))) => {
            let at = PrimitiveDateTime::new(at.date(), at.time());
            start <= at && at < end
        }
        _unrepresentable => false,
    }
}

/// The offset of US Pacific time from UTC at UNIX time `unix`, in seconds:
/// -7 h in daylight time, -8 h otherwise.
pub(crate) fn pacific_offset(unix: i64) -> i64 {
    if pacific_daylight_saving(unix) {
        -7 * 3600
    } else {
        -8 * 3600
    }
}

/// `micros` since the epoch as `YYYY-MM-DDThh:mm:ss`, then `digits` digits of
/// the second's fraction (none for 0), then `Z`. Seven digits are .NET's
/// ticks of 100 ns; a microsecond clock fills the seventh with 0.
pub(crate) fn timestamp(micros: i64, digits: u32) -> String {
    let at = instant(micros);
    let mut text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    );
    if digits > 0 {
        let fraction = i64::from(at.microsecond());
        let scaled = if digits > 6 {
            fraction.saturating_mul(10_i64.saturating_pow(digits.saturating_sub(6)))
        } else {
            fraction
                .checked_div(10_i64.saturating_pow(6_u32.saturating_sub(digits)))
                .unwrap_or(0)
        };
        let width = usize::try_from(digits).unwrap_or(0);
        let _infallible = write!(text, ".{scaled:0width$}");
    }
    text.push('Z');
    text
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{date, pacific_daylight_saving, pacific_offset, timestamp};

    /// The probe's reading: 2026-10-28 is in daylight time, 2026-11-07 (after
    /// the first Sunday of November) in standard time, and the change in March
    /// comes at 10:00 UTC.
    #[test]
    fn pacific_time_follows_the_us_rule() {
        assert_eq!(pacific_offset(1_793_179_656), -7 * 3600);
        assert_eq!(pacific_offset(1_794_009_600), -8 * 3600);
        assert!(!pacific_daylight_saving(1_772_963_999));
        assert!(pacific_daylight_saving(1_772_964_000));
    }

    /// Six digits as Second Life writes them, seven as .NET does.
    #[test]
    fn a_timestamp_has_the_grids_digits() {
        let micros = 1_793_179_656_407_385;
        assert_eq!(timestamp(micros, 6), "2026-10-28T09:27:36.407385Z");
        assert_eq!(timestamp(micros, 7), "2026-10-28T09:27:36.4073850Z");
        assert_eq!(timestamp(micros, 0), "2026-10-28T09:27:36Z");
        assert_eq!(date(micros), "2026-10-28");
    }
}
