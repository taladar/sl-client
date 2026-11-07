//! Library tranche: the clocks (`server-lsl-lib-time-timers`) — calendar
//! time, the region's frame rate and dilation, and OpenSim's apparent time.
//!
//! Calendar time is the one thing a running script reads that a real grid
//! takes from the wall clock, and a reproducible one may not: here it is the
//! region's calendar time at its first tick ([`Host::epoch_micros`]) advanced
//! by the ticks since, so a script's dates follow from the seed and the ticks
//! alone (`book/src/simulator/determinism.md`). The shapes are each grid's,
//! measured by `time/clocks` (`book/src/gridspec/lsl.md` § The clocks).
//!
//! [`Host::epoch_micros`]: crate::vm::Host::epoch_micros

use crate::library::CallError;
use crate::library::calendar::{DAY_SECONDS, date, pacific_offset, timestamp};
use crate::num::to_f32;
use crate::vm::ScriptCtx;

/// Four hours, in seconds: the period `llGetTimeOfDay` counts.
const TIME_OF_DAY_PERIOD: f64 = 4.0 * 3600.0;

/// The region's calendar time, in microseconds since the epoch.
fn micros(ctx: &mut ScriptCtx<'_>) -> i64 {
    ctx.unix_micros()
}

/// The second of the day of `micros` shifted by `offset` seconds, with its
/// fraction.
fn second_of_day(micros: i64, offset: i64) -> f64 {
    let shifted = micros.saturating_add(offset.saturating_mul(1_000_000));
    let in_day = shifted.rem_euclid(DAY_SECONDS.saturating_mul(1_000_000));
    crate::num::wide_to_double(i128::from(in_day)) / 1_000_000.0
}

/// The offset of the region's host's zone from UTC at `micros`, in seconds
/// (OpenSim reads `DateTime.Now`).
fn host_offset(ctx: &mut ScriptCtx<'_>, micros: i64) -> i64 {
    i64::from(ctx.host().local_offset(micros.div_euclid(1_000_000)))
}

/// `llGetUnixTime`: the region's calendar time in whole seconds since the
/// epoch.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_unix_time(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    let seconds = micros(ctx).div_euclid(1_000_000);
    Ok(i32::try_from(seconds).unwrap_or(i32::MAX))
}

/// `llGetTimestamp`: the calendar time in UTC, `YYYY-MM-DDThh:mm:ss.ffffffZ`
/// — six fraction digits on Second Life, .NET's seven on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_timestamp(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let digits = if ctx.flavour().is_opensim() { 7 } else { 6 };
    Ok(timestamp(micros(ctx), digits))
}

/// `llGetDate`: the UTC date, `YYYY-MM-DD`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_date(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(date(micros(ctx)))
}

/// `llGetGMTclock`: the second of the UTC day — a whole number on Second
/// Life, with its fraction on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_gmt_clock(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let second = second_of_day(micros(ctx), 0);
    Ok(to_f32(if ctx.flavour().is_opensim() {
        second
    } else {
        second.floor()
    }))
}

/// `llGetWallclock`: the second of the day in US Pacific time, a whole
/// number, on Second Life; in the simulator host's own zone, with its
/// fraction, on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_wallclock(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let now = micros(ctx);
    if ctx.flavour().is_opensim() {
        let offset = host_offset(ctx, now);
        return Ok(to_f32(second_of_day(now, offset)));
    }
    let offset = pacific_offset(now.div_euclid(1_000_000));
    Ok(to_f32(second_of_day(now, offset).floor()))
}

/// `llGetTimeOfDay`: on Second Life the seconds into the parcel's day cycle
/// (with its default day, UTC's second modulo four hours); on OpenSim the
/// host's local second of the day modulo four hours.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_time_of_day(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    if !ctx.flavour().is_opensim() {
        // The parcel's day cycle: with its default day of four hours from an
        // offset of sixteen, the UTC second modulo four hours.
        return Ok(to_f32(crate::library::land::time_of_day(ctx, false)));
    }
    let now = micros(ctx);
    let offset = host_offset(ctx, now);
    let millis = (second_of_day(now, offset) * 1000.0).floor() / 1000.0;
    Ok(to_f32(millis % TIME_OF_DAY_PERIOD))
}

/// `llGetRegionTimeDilation`: 1.0. Second Life's measures its physics frame,
/// and the fake grid's tick is stepped rather than timed, so it never runs
/// slow (the book's scheduling chapter); both grids read 1.0 on a quiet
/// region.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_get_region_time_dilation(_ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(1.0)
}

/// `llGetRegionFPS`: the region's frames a second — its heartbeat's, one
/// over the tick (45 on a region ticking as Second Life's, 55 as OpenSim's).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_fps(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let step = ctx.step().as_secs_f64();
    Ok(if step > 0.0 { to_f32(1.0 / step) } else { 0.0 })
}

/// The fraction of its day the region's environment is at
/// (`GetRegionDayFractionTime`): the calendar time plus the day's offset,
/// modulo the day's length.
fn day_fraction(ctx: &mut ScriptCtx<'_>) -> f64 {
    let caller = ctx.caller();
    let length = f64::from(ctx.host().day_length(caller));
    let offset = f64::from(ctx.host().day_offset(caller));
    if length <= 0.0 {
        return 0.0;
    }
    let seconds = crate::num::wide_to_double(i128::from(micros(ctx).div_euclid(1_000_000)));
    (((seconds + offset) % length) / length).clamp(0.0, 1.0)
}

/// OpenSim's `timeToString`: `frac` hours as `H:MM:SS AM` or, 24-hour,
/// `HH:MM:SS`.
fn apparent_string(hours: f32, format24: bool) -> String {
    let h = hours.trunc();
    let minutes = (hours - h) * 60.0;
    let m = minutes.trunc();
    let s = ((minutes - m) * 60.0).trunc();
    let (h, m, s) = (
        crate::num::float_to_int(h),
        crate::num::float_to_int(m),
        crate::num::float_to_int(s),
    );
    if format24 {
        return format!("{h:02}:{m:02}:{s:02}");
    }
    let (twelve, half) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h.saturating_sub(12), "PM"),
    };
    format!("{twelve}:{m:02}:{s:02} {half}")
}

/// `osGetApparentTime`: the second of the region's day-cycle day,
/// `86400 × fraction`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_apparent_time(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(86_400.0 * to_f32(day_fraction(ctx)))
}

/// `osGetCurrentSunHour`: the hour of the region's day-cycle day,
/// `24 × fraction`, 0 roughly at sunrise.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_current_sun_hour(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(24.0 * to_f32(day_fraction(ctx)))
}

/// `osGetApparentRegionTime`: the same as [`os_get_apparent_time`] — both read
/// the region's environment.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_apparent_region_time(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    os_get_apparent_time(ctx)
}

/// `osGetApparentTimeString`: the apparent time as a clock, 24-hour when
/// `format24` is not 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_apparent_time_string(
    ctx: &mut ScriptCtx<'_>,
    format24: i32,
) -> Result<String, CallError> {
    Ok(apparent_string(
        24.0 * to_f32(day_fraction(ctx)),
        format24 != 0,
    ))
}

/// `osGetApparentRegionTimeString`: the same as
/// [`os_get_apparent_time_string`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_apparent_region_time_string(
    ctx: &mut ScriptCtx<'_>,
    format24: i32,
) -> Result<String, CallError> {
    os_get_apparent_time_string(ctx, format24)
}

/// `osGetPSTWallclock`: the second of the day in US Pacific time, with its
/// fraction.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_pst_wallclock(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let now = micros(ctx);
    Ok(to_f32(second_of_day(
        now,
        pacific_offset(now.div_euclid(1_000_000)),
    )))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{apparent_string, second_of_day};

    /// The probe's reading: 09:27:36 UTC is second 34 056.
    #[test]
    fn the_second_of_the_day() {
        assert!((second_of_day(1_793_179_656_407_385, 0) - 34_056.407_385).abs() < 1e-6);
        assert!((second_of_day(1_793_179_656_000_000, -7 * 3600) - 8856.0).abs() < 1e-6);
    }

    /// OpenSim's clock strings.
    #[test]
    fn an_apparent_time_reads_as_opensim_writes_it() {
        assert_eq!(apparent_string(13.5, true), "13:30:00");
        assert_eq!(apparent_string(13.5, false), "1:30:00 PM");
        assert_eq!(apparent_string(0.25, false), "12:15:00 AM");
    }
}
