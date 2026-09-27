//! What a library function sees of the script calling it.

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
/// is calling, the current tick, the host, and a way to ask the VM for a
/// control-flow effect (sleep, reset).
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
    /// The world.
    host: &'host mut dyn Host,
    /// What the function asked the VM to do after it returns; the last
    /// request wins, except that a reset is never replaced by a sleep.
    request: Option<Request>,
}

impl<'host> ScriptCtx<'host> {
    /// A context for one call.
    pub(crate) fn new(caller: CallerId, now: Tick, host: &'host mut dyn Host) -> Self {
        Self {
            caller,
            now,
            host,
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
