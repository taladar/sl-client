//! What a library function sees of the script calling it.

use std::collections::BTreeMap;

use core::time::Duration;

use crate::num::ticks_for;
use crate::vm::detected::Detected;
use crate::vm::flavour::EngineFlavour;
use crate::vm::host::{CallerId, Host};
use crate::vm::instance::Instance;
use crate::vm::linkset_data::{Answer, LinksetChange, LinksetData};
use crate::vm::memory::{Memory, Stack};

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
    /// What library calls said refusing their arguments, worded for the
    /// flavour, waiting for the engine to say them on `DEBUG_CHANNEL`.
    pub(crate) refusals: Vec<String>,
    /// What library calls changed in the linkset's data store, in order,
    /// waiting for the engine to raise `linkset_data` for each in every
    /// script of the linkset.
    pub(crate) linkset: Vec<LinksetChange>,
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

/// The region's other scripts, as a library function may see them: their run
/// flags, and nothing else (`llGetScriptState`) — and which grid's engine the
/// region imitates.
///
/// The engine runs one instance at a time with the rest lent here, so a
/// script reads the very flag the engine schedules by — the one a run-time
/// error clears and the contents' Running checkbox shows — never a copy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Peers<'run> {
    /// The instances of the engine, less the one running; [`None`] outside
    /// an engine.
    instances: Option<&'run BTreeMap<CallerId, Instance>>,
    /// The flavour the region imitates.
    flavour: EngineFlavour,
}

impl<'run> Peers<'run> {
    /// No other scripts: an instance run on its own, outside an engine, as
    /// on Second Life.
    pub(crate) const NONE: Self = Self {
        instances: None,
        flavour: EngineFlavour::SecondLife,
    };

    /// The instances of an engine imitating `flavour`, less the one running.
    pub(crate) const fn of(
        instances: &'run BTreeMap<CallerId, Instance>,
        flavour: EngineFlavour,
    ) -> Self {
        Self {
            instances: Some(instances),
            flavour,
        }
    }

    /// The flavour the region imitates.
    pub(crate) const fn flavour(self) -> EngineFlavour {
        self.flavour
    }

    /// Whether the instance `id` names reads as running in this flavour;
    /// [`None`] when there is no such instance.
    fn is_running(self, id: CallerId) -> Option<bool> {
        self.instances?
            .get(&id)
            .map(|instance| instance.reads_running(self.flavour))
    }
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
    /// The region's other scripts.
    peers: Peers<'host>,
    /// What the function asked the VM to do after it returns; the last
    /// request wins, except that a reset is never replaced by a sleep.
    request: Option<Request>,
    /// The script's memory accounting.
    memory: &'host mut Memory,
    /// What the script's call stack holds as the call is made.
    stack: Stack,
}

impl<'host> ScriptCtx<'host> {
    /// A context for one call.
    pub(crate) fn new(
        caller: CallerId,
        now: Tick,
        step: Duration,
        host: &'host mut dyn Host,
        data: &'host mut ScriptData,
        peers: Peers<'host>,
        (memory, stack): (&'host mut Memory, Stack),
    ) -> Self {
        Self {
            caller,
            now,
            step,
            host,
            data,
            peers,
            request: None,
            memory,
            stack,
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

    /// `llGetScriptState`: whether the script called `name` in the caller's
    /// prim reads as running in the region's flavour, or [`None`] when the
    /// prim has no script by that name. The caller itself is running, since
    /// it is running this; a script with no instance in the region is not.
    pub fn script_running(&mut self, name: &str) -> Option<bool> {
        match self.host.script_named(self.caller, name)? {
            id if id == self.caller => Some(true),
            id => Some(self.peers.is_running(id).unwrap_or(false)),
        }
    }

    /// The grid's engine the region imitates, for a function whose answer or
    /// words differ between them.
    #[must_use]
    pub const fn flavour(&self) -> EngineFlavour {
        self.peers.flavour()
    }

    /// `llGetUsedMemory`: what the script uses as its grid counts it.
    #[must_use]
    pub const fn used_memory(&self) -> u32 {
        self.memory.used(self.stack)
    }

    /// `llGetFreeMemory`: Mono's limit less the most the script has used
    /// since it was set; YEngine's heap less what it uses now.
    #[must_use]
    pub const fn free_memory(&self) -> u32 {
        self.memory.free(self.stack)
    }

    /// `llGetMemoryLimit`.
    #[must_use]
    pub const fn memory_limit(&self) -> u32 {
        self.memory.limit()
    }

    /// `llSetMemoryLimit`: whether the grid took `limit`.
    pub fn set_memory_limit(&mut self, limit: i32) -> bool {
        self.memory.set_limit(limit, self.stack)
    }

    /// `llScriptProfiler`: start or stop recording the most memory used.
    pub fn profile(&mut self, flags: i32) {
        self.memory.profile(flags, self.stack);
    }

    /// `llGetSPMaxMemory`: the most memory used while profiling.
    #[must_use]
    pub fn profiled_memory(&self) -> u32 {
        self.memory.profiled()
    }

    /// A random number in `0..bound` from the region's seeded source; `0` for
    /// a bound of zero. The draw is reduced by Lemire's multiply-and-shift,
    /// whose bias over 32 bits is below anything a script can count.
    pub fn random_below(&mut self, bound: u32) -> u32 {
        let draw = u64::from(self.host.random_bits(self.caller));
        u32::try_from(draw.saturating_mul(u64::from(bound)) >> 32).unwrap_or(0)
    }

    /// 32 random bits from the region's seeded source (`llFrand`).
    pub fn random_bits(&mut self) -> u32 {
        self.host.random_bits(self.caller)
    }

    /// The caller's linkset data store, to read.
    pub fn linkset_data(&mut self) -> &LinksetData {
        self.host.linkset_data(self.caller)
    }

    /// Change the caller's linkset data store with `change` and answer what
    /// it answered; the change it made, if any, raises `linkset_data` in
    /// every script of the linkset once the slice is over — the caller's own
    /// too, unless the slice ends in a reset, which empties its queue.
    pub fn change_linkset_data<T>(
        &mut self,
        change: impl FnOnce(&mut LinksetData) -> Answer<T>,
    ) -> T {
        let answer = change(self.host.linkset_data(self.caller));
        if let Some(made) = answer.change {
            self.data.linkset.push(made);
        }
        answer.value
    }

    /// Refuse the call's arguments with `text`, worded for the
    /// [`Self::flavour`]: the engine says it on `DEBUG_CHANNEL` once the
    /// slice is over, through the same flood limit as a run-time error. The
    /// call itself carries on and returns its answer.
    pub fn refuse(&mut self, text: String) {
        self.data.refusals.push(text);
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
