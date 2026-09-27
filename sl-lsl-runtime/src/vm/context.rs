//! What a library function sees of the script calling it.

use core::time::Duration;

use crate::num::ticks_for;
use crate::vm::detected::Detected;
use crate::vm::host::{CallerId, Host};

/// A heartbeat tick of the region: the unit every script-visible time is
/// counted in. A script's time is never read from a clock — sleeps, and later
/// timers and `llGetTime`, are tick numbers (the determinism rule of
/// `roadmap/context/lsl.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Tick(pub u64);

impl Tick {
    /// The tick `ticks` after this one.
    #[must_use]
    pub const fn after(self, ticks: u64) -> Self {
        Self(self.0.saturating_add(ticks))
    }
}

/// A repeating timer (`llSetTimerEvent`), counted in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timer {
    /// Ticks between two `timer` events; at least one.
    pub(crate) interval: u64,
    /// The tick the next `timer` event is due at.
    pub(crate) next: Tick,
}

/// The parts of an instance a library function may read or write: the
/// current event's detected block, the timer and the start parameter. The
/// VM owns everything else.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ScriptData {
    /// The detected block `llDetected*` reads.
    pub(crate) detected: Vec<Detected>,
    /// The repeating timer, if one is set.
    pub(crate) timer: Option<Timer>,
    /// `llGetStartParameter`: what the object was rezzed with.
    pub(crate) start_parameter: i32,
}

/// A control-flow effect a library function asks the VM for. The VM acts on
/// it once the function has returned, never from inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Request {
    /// Suspend the script for this many seconds (`llSleep`).
    Sleep(f32),
    /// Restart the script from scratch (`llResetScript`).
    Reset,
}

/// The calling script instance, as a library function sees it: which script
/// is calling, the current tick, the host, the script's own detected block,
/// timer and start parameter, and a way to ask the VM for a control-flow
/// effect (sleep, reset).
///
/// It deliberately does not reach the operand stack, the program counter or
/// the globals: a library function computes a value from its arguments and
/// the world, and anything that changes how the script continues is a
/// request the VM carries out after the call.
pub struct ScriptCtx<'host> {
    /// Who is calling.
    caller: CallerId,
    /// The tick the call happens in.
    now: Tick,
    /// The length of a tick, which turns seconds into ticks.
    step: Duration,
    /// The world.
    host: &'host mut dyn Host,
    /// The instance's library-visible state.
    data: &'host mut ScriptData,
    /// What the function asked the VM to do after it returns; the last
    /// request wins, except that a reset is never replaced by a sleep.
    request: Option<Request>,
}

impl<'host> ScriptCtx<'host> {
    /// A context for one call.
    pub(crate) fn new(
        caller: CallerId,
        now: Tick,
        step: Duration,
        host: &'host mut dyn Host,
        data: &'host mut ScriptData,
    ) -> Self {
        Self {
            caller,
            now,
            step,
            host,
            data,
            request: None,
        }
    }

    /// The calling instance.
    #[must_use]
    pub const fn caller(&self) -> CallerId {
        self.caller
    }

    /// The tick the call happens in.
    #[must_use]
    pub const fn now(&self) -> Tick {
        self.now
    }

    /// The world, for the functions that touch it.
    pub fn host(&mut self) -> &mut dyn Host {
        self.host
    }

    /// The detection at `index` of the event being handled, if there is one.
    #[must_use]
    pub fn detected(&self, index: i32) -> Option<&Detected> {
        self.data.detected.get(usize::try_from(index).ok()?)
    }

    /// Start, restart or (with a duration that is not positive) stop the
    /// script's repeating timer: the first `timer` event is due `seconds`
    /// from now, rounded up to a whole tick and at least one, and one every
    /// `seconds` after.
    pub fn set_timer(&mut self, seconds: f32) {
        let interval = ticks_for(f64::from(seconds), self.step);
        self.data.timer = (interval > 0).then(|| Timer {
            interval,
            next: self.now.after(interval),
        });
    }

    /// `llGetStartParameter`.
    #[must_use]
    pub const fn start_parameter(&self) -> i32 {
        self.data.start_parameter
    }

    /// Suspend the script for `seconds` once the function returns. A
    /// duration that is not positive (or is NaN) does not suspend.
    pub fn sleep(&mut self, seconds: f32) {
        if self.request != Some(Request::Reset) {
            self.request = Some(Request::Sleep(seconds));
        }
    }

    /// Restart the script once the function returns: globals back to their
    /// initialisers, the stack and the event queue dropped, and `default`'s
    /// `state_entry` next.
    pub const fn reset(&mut self) {
        self.request = Some(Request::Reset);
    }

    /// The effect requested, taken once by the VM.
    pub(crate) const fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
}

impl core::fmt::Debug for ScriptCtx<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ScriptCtx")
            .field("caller", &self.caller)
            .field("now", &self.now)
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}
