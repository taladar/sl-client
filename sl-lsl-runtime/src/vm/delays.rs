//! How long a library call holds the script, per flavour: the forced delay
//! each grid puts on each call, OpenSim's `ScriptDelayFactor` that scales it,
//! and OpenSim's channel-0 chat throttle (book chapter `gridspec/lsl.md`,
//! § Forced delays and throttles).
//!
//! The reference's delay is the `sleep` of Linden Lab's keyword file
//! ([`Builtin::sleep`]); [`forced`] is that value where the grid agrees and
//! the grid's own where it does not. OpenSim's own functions have no delay
//! there; the few that sleep say so themselves. Second Life was measured
//! (`throttle/delays`); OpenSim's column is its `LSL_Api` / `OSSL_Api`
//! source, the `ScriptSleep` each function ends in, confirmed where the probe
//! called it. A call whose delay depends on how it went says so itself
//! ([`crate::vm::ScriptCtx::delay`]), and an OpenSim error costs a second on
//! top ([`crate::vm::ScriptCtx::error_pause`]).

use std::time::Duration;

use crate::library::Builtin;
use crate::vm::EngineFlavour;

/// The forced delay of `descriptor` under `flavour`, in seconds, before
/// OpenSim's `ScriptDelayFactor` ([`scaled`]).
#[must_use]
pub(crate) fn forced(flavour: EngineFlavour, descriptor: &Builtin) -> f64 {
    let own = if flavour.is_opensim() {
        opensim(descriptor.name)
    } else {
        second_life(descriptor.name)
    };
    own.unwrap_or_else(|| f64::from(descriptor.sleep))
}

/// Second Life's delays that are not the keyword file's (`throttle/delays`).
/// `llGiveInventory`'s 2 s to an avatar depends on the destination and is the
/// call's own.
fn second_life(name: &str) -> Option<f64> {
    match name {
        // The keyword file says 1.0; the call returns at once.
        "llTextBox" => Some(0.0),
        _ => None,
    }
}

/// OpenSim's delays that are not the keyword file's: the `ScriptSleep` each
/// function of `LSL_Api` / `OSSL_Api` takes when it runs to the end (its
/// `m_sleepMsOn*` defaults), and none where it takes none.
fn opensim(name: &str) -> Option<f64> {
    Some(match name {
        // No `ScriptSleep` at all.
        "llSetRemoteScriptAccessPin" | "llGetPrimitiveParams" | "llGetSimulatorHostname" => 0.0,
        // `m_sleepMsOnRequestAgentData`, which these share; an avatar in the
        // region is answered at once (the call's own).
        "llRequestUsername" | "llRequestUserKey" => 0.1,
        "llModPow" | "llEjectFromLand" | "llCreateLink" | "llGetLinkMedia" | "llSetLinkMedia"
        | "llClearLinkMedia" => 1.0,
        "llGiveInventory" => 3.0,
        "llLoadURL" => 10.0,
        "llSetPrimURL" => 2.0,
        _ => return None,
    })
}

/// `seconds` of forced delay as the grid takes it: on OpenSim scaled by the
/// region's `ScriptDelayFactor` and cut to whole milliseconds, and nothing at
/// all under ten (`LSL_Api.ScriptSleep`); on Second Life as it is.
#[must_use]
pub(crate) fn scaled(flavour: EngineFlavour, seconds: f64, factor: f64) -> f64 {
    if !flavour.is_opensim() {
        return seconds.max(0.0);
    }
    let millis = (seconds * 1000.0 * factor).trunc();
    if millis.is_nan() || millis < 10.0 {
        0.0
    } else {
        millis / 1000.0
    }
}

/// Lines on channel 0 OpenSim lets a script say within a second before the
/// next sleeps (`m_SayShoutCount >= 11`).
const CHANNEL_ZERO_LINES: u32 = 11;

/// The window OpenSim counts channel-0 lines in.
const CHANNEL_ZERO_WINDOW: Duration = Duration::from_secs(1);

/// The delay OpenSim puts on `llSay` / `llShout` past the throttle.
pub(crate) const CHANNEL_ZERO_DELAY: f32 = 2.0;

/// OpenSim's channel-0 chat throttle, one per script (`LSL_Api`'s
/// `m_SayShoutCount` and `m_lastSayShoutCheck`): a channel-0 `llSay` or
/// `llShout` more than a second after the window opened opens a new one and
/// counts nothing, any other adds one; once eleven are counted every `llSay`
/// and `llShout` — on any channel — sleeps two seconds until a channel-0 line
/// opens a new window. So of 25 lines in a row the 12th and the 24th sleep.
/// It survives a reset, as `LSL_Api` does; Second Life has no such throttle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SayThrottle {
    /// The lines counted in the window.
    count: u32,
    /// When the window opened: the script's start until a line opens one.
    opened: Option<Duration>,
}

impl SayThrottle {
    /// The window opens at `now` if none has: the script started then.
    pub(crate) const fn start(&mut self, now: Duration) {
        if self.opened.is_none() {
            self.opened = Some(now);
        }
    }

    /// A line on `channel` at `now`: whether the call sleeps
    /// [`CHANNEL_ZERO_DELAY`] before it is said.
    pub(crate) fn say(&mut self, channel: i32, now: Duration) -> bool {
        if channel == 0 {
            let opened = self.opened.unwrap_or(now);
            if now.saturating_sub(opened) > CHANNEL_ZERO_WINDOW {
                self.opened = Some(now);
                self.count = 0;
            } else {
                self.count = self.count.saturating_add(1);
                self.opened = Some(opened);
            }
        }
        self.count >= CHANNEL_ZERO_LINES
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn opensim_cuts_a_scaled_delay_to_milliseconds_and_drops_one_under_ten() {
        let os = EngineFlavour::YEngine;
        assert!((scaled(os, 0.2, 1.0) - 0.2).abs() < 1e-9);
        assert!((scaled(os, 0.2, 0.5) - 0.1).abs() < 1e-9);
        assert!((scaled(os, 0.025, 1.0) - 0.025).abs() < 1e-9);
        assert!(scaled(os, 0.025, 0.3).abs() < 1e-9, "7 ms is under ten");
        assert!(scaled(os, 1.0, 0.0).abs() < 1e-9);
        assert!((scaled(EngineFlavour::SecondLife, 0.2, 0.5) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn the_twelfth_and_the_twenty_fourth_channel_zero_line_sleep() {
        let mut throttle = SayThrottle::default();
        throttle.start(Duration::ZERO);
        let mut now = Duration::from_secs(5);
        let mut slept = Vec::new();
        for call in 1_u32..=25 {
            if throttle.say(0, now) {
                slept.push(call);
                now = now.saturating_add(Duration::from_secs(2));
            }
            now = now.saturating_add(Duration::from_millis(10));
        }
        assert_eq!(slept, vec![12, 24]);
    }

    #[test]
    fn past_the_count_every_line_sleeps_until_channel_zero_opens_a_window() {
        let mut throttle = SayThrottle::default();
        throttle.start(Duration::ZERO);
        let at = Duration::from_secs(5);
        for _ in 0..12 {
            let _sleeps = throttle.say(0, at);
        }
        assert!(throttle.say(7, at), "any channel sleeps past the count");
        assert!(throttle.say(7, at.saturating_add(Duration::from_secs(9))));
        assert!(!throttle.say(0, at.saturating_add(Duration::from_secs(9))));
    }

    #[test]
    fn a_line_within_a_second_of_the_start_is_counted() {
        let mut throttle = SayThrottle::default();
        throttle.start(Duration::ZERO);
        let slept = (1_u32..=12)
            .filter(|_| throttle.say(0, Duration::from_millis(500)))
            .count();
        assert_eq!(slept, 2, "the 11th and 12th: the start opened the window");
    }
}
