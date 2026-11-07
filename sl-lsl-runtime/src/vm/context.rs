//! What a library function sees of the script calling it.

use std::collections::BTreeMap;

use core::time::Duration;

use crate::value::Element;
use crate::vm::comms::{LinkMessage, Listens};
use crate::vm::contents::Control;
use crate::vm::detected::Detected;
use crate::vm::economy::TransactionResult;
use crate::vm::experiences::{ExperienceError, ExperienceQuestion};
use crate::vm::flavour::EngineFlavour;
use crate::vm::host::{CallerId, Host};
use crate::vm::instance::Instance;
use crate::vm::linkset_data::{Answer, LinksetChange, LinksetData};
use crate::vm::ossl::{Settings, Standing, stock_ossl};
use crate::vm::permissions::{Permissions, Question};
use crate::vm::report::Warning;

/// An event a library call raised in the scripts of its linkset or in its
/// own, which the engine posts once the call's slice is over — in the order
/// of the calls, since on Second Life each is queued at its call (a link
/// message sent before a write is ahead of the write's `linkset_data` in
/// every queue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raised {
    /// `linkset_data`, in every script of the linkset.
    Linkset(LinksetChange),
    /// `link_message`, in every script the message reaches.
    Linked(LinkMessage),
    /// `run_time_permissions(mask)` in the calling script: a grant made
    /// without a question, or OpenSim's release and refusal.
    Permissions(i32),
    /// `transaction_result` in the calling script.
    Transaction(TransactionResult),
    /// `dataserver(id, data)` in the calling script: a key-value call's
    /// answer.
    Dataserver {
        /// The handle the call returned.
        id: String,
        /// The answer.
        data: String,
    },
    /// `experience_permissions(agent)` in the calling script.
    ExperienceGranted(String),
    /// `dataserver(from, message)` in the scripts of another prim:
    /// `osMessageObject`.
    ObjectMessage {
        /// The scripts of the prim messaged.
        to: Vec<CallerId>,
        /// The key of the sending prim.
        from: String,
        /// The message.
        message: String,
    },
    /// `experience_permissions_denied(agent, reason)` in the calling script.
    ExperienceDenied {
        /// The avatar asked.
        agent: String,
        /// Why, an `XP_ERROR_*` code.
        reason: ExperienceError,
    },
    /// `http_request(id, method, body)` in the calling script: the answer to
    /// `llRequestURL`, `URL_REQUEST_GRANTED` with the URL or
    /// `URL_REQUEST_DENIED`.
    UrlAnswer {
        /// The key the request returned.
        id: String,
        /// `URL_REQUEST_GRANTED` or `URL_REQUEST_DENIED`.
        method: &'static str,
        /// The URL, or nothing.
        body: String,
    },
    /// `http_response(id, status, metadata, body)` in the calling script at
    /// once: Second Life's problem report for a request `llHTTPRequest`
    /// refused under `HTTP_EXTENDED_ERROR`.
    HttpResponse {
        /// The request's key.
        id: String,
        /// The status.
        status: i32,
        /// The body.
        body: String,
    },
    /// `llSetScriptState` or `llResetOtherScript` of another script of the
    /// caller's prim.
    Control {
        /// The script.
        target: CallerId,
        /// What is done to it.
        control: Control,
    },
}
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

/// A repeating timer (`llSetTimerEvent`), in region time: the time since the
/// region's tick 0, which is the tick times the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timer {
    /// The time between two `timer` events, at least the flavour's floor.
    pub(crate) interval: Duration,
    /// The region time the next `timer` event is due at; it falls due at the
    /// first tick at or after it.
    pub(crate) next: Duration,
    /// Whether an event has fallen due since the timer was set: XEngine keeps
    /// the phase of its first.
    pub(crate) fired: bool,
}

/// The region time at the start of tick `now`: the ticks times the step.
pub(crate) fn region_time(now: Tick, step: Duration) -> Duration {
    let nanos = u128::from(now.0).saturating_mul(step.as_nanos());
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// The shortest interval `flavour` runs a timer at: two of Second Life's
/// frames (0.044 s, `time/timer-floor`), OpenSim's `MinTimerInterval` (0.1 s
/// for YEngine, 0.5 s for XEngine at their defaults).
fn timer_floor(flavour: EngineFlavour) -> f64 {
    match flavour {
        EngineFlavour::SecondLife => 2.0 / 45.0,
        EngineFlavour::YEngine => 0.1,
        EngineFlavour::XEngine => 0.5,
    }
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
    /// The timer was just stopped on a flavour whose stop also takes a
    /// `timer` event still queued (Second Life and YEngine,
    /// `events/timer-reset`); the instance drops it after the call.
    pub(crate) drop_queued_timer: bool,
    /// `llGetStartParameter`: what the object was rezzed with.
    pub(crate) start_parameter: i32,
    /// `llGetStartString`: what the object was rezzed with
    /// (`REZ_PARAM_STRING`).
    pub(crate) start_string: String,
    /// One-shot sweeps asked for (`llSensor`) and not yet swept: at most one
    /// on Second Life, where each replaces the last; every one on OpenSim.
    pub(crate) sweeps: Vec<crate::vm::sensing::Query>,
    /// The repeating sweep (`llSensorRepeat`), which a state change ends.
    pub(crate) repeat: Option<crate::vm::sensing::Repeat>,
    /// The repeating sweep was just removed on a flavour whose remove also
    /// takes a sweep's event still queued (Second Life); the instance drops
    /// it after the call.
    pub(crate) drop_queued_sensor: bool,
    /// What library calls said refusing their arguments, worded for the
    /// flavour, waiting for the engine to say them on `DEBUG_CHANNEL`.
    pub(crate) refusals: Vec<Warning>,
    /// What library calls raised in other scripts — a change of the
    /// linkset's data store, a link message — in the order of the calls,
    /// waiting for the engine to post it once the slice is over.
    pub(crate) raised: Vec<Raised>,
    /// The script's listens (`llListen`), which a state change and a reset
    /// drop.
    pub(crate) listens: Listens,
    /// The script's permissions, which a state change keeps and a reset
    /// clears.
    pub(crate) permissions: Permissions,
    /// The script's time (`llGetTime`): the ticks begun since a reset or an
    /// `llResetTime`, counted whether the script runs or not. On aditi script
    /// time moves only in whole simulator frames, so a tick count is the
    /// reference's shape, not an approximation of it.
    pub(crate) script_ticks: u64,
    /// The script's HTTP state: its URLs, the requests it is waiting on and
    /// the ones that arrived at its URLs, which a reset drops.
    pub(crate) http: HttpState,
    /// The avatar controls the script took (`llTakeControls`), which a reset
    /// drops and a state change keeps.
    pub(crate) controls: Option<TakenControls>,
}

/// An avatar's controls a script took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TakenControls {
    /// The avatar.
    pub(crate) agent: String,
    /// What the last `ScriptControlChange` stated.
    pub(crate) held: crate::vm::avatars::Held,
    /// The events' state.
    pub(crate) events: crate::vm::avatars::Controls,
}

/// What a script holds of HTTP (`server-lsl-lib-http-url`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HttpState {
    /// The URLs granted to it and not released.
    pub(crate) urls: Vec<String>,
    /// Each request it sent and has had no answer to, with the longest body
    /// it takes.
    pub(crate) pending: BTreeMap<String, usize>,
    /// Each request that arrived at one of its URLs and is not answered.
    pub(crate) inbound: BTreeMap<String, crate::vm::http::Inbound>,
    /// The `Content-Type` the answer to an arrived request is served with,
    /// where `llSetContentType` or `osSetContentType` set one.
    pub(crate) content_types: BTreeMap<String, String>,
}

/// A control-flow effect a library function asks the VM for. The VM acts on
/// it once the function has returned, never from inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Request {
    /// Suspend the script for this many seconds (`llSleep`).
    Sleep(f32),
    /// Restart the script from scratch (`llResetScript`).
    Reset,
    /// End the script for good: its object is leaving the region (`llDie`).
    Die,
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
    /// The region's `[OSSL]` configuration; [`None`] for the stock one.
    ossl: Option<&'run Settings>,
}

impl<'run> Peers<'run> {
    /// No other scripts: an instance run on its own, outside an engine, as
    /// on Second Life.
    pub(crate) const NONE: Self = Self {
        instances: None,
        flavour: EngineFlavour::SecondLife,
        ossl: None,
    };

    /// The instances of an engine imitating `flavour` under the `[OSSL]`
    /// configuration `ossl`, less the one running.
    pub(crate) const fn of(
        instances: &'run BTreeMap<CallerId, Instance>,
        flavour: EngineFlavour,
        ossl: &'run Settings,
    ) -> Self {
        Self {
            instances: Some(instances),
            flavour,
            ossl: Some(ossl),
        }
    }

    /// The flavour the region imitates.
    pub(crate) const fn flavour(self) -> EngineFlavour {
        self.flavour
    }

    /// The region's `[OSSL]` configuration.
    pub(crate) fn ossl(self) -> &'run Settings {
        self.ossl.unwrap_or_else(|| stock_ossl())
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
    /// The argument a function changed in place and what it holds now, as
    /// OpenSim's library changes the list object it is handed
    /// ([`crate::vm::objects`]).
    written_back: Option<(usize, Vec<Element>)>,
    /// The argument whose own list the function returned, as OpenSim's
    /// library hands back the very object it was given in some cases.
    returned: Option<usize>,
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
            written_back: None,
            returned: None,
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

    /// Start, restart or stop the script's repeating timer: the first
    /// `timer` event is due `seconds` from now and one every `seconds` after,
    /// never more often than the flavour's floor (`time/timer-floor`). Zero
    /// stops it on every grid; a negative interval stops it on Second Life
    /// and runs it at the floor on OpenSim.
    pub fn set_timer(&mut self, seconds: f32) {
        let flavour = self.flavour();
        let floor = timer_floor(flavour);
        let seconds = f64::from(seconds);
        let interval = if seconds.is_nan() || seconds == 0.0 {
            None
        } else if seconds < 0.0 {
            flavour.is_opensim().then_some(floor)
        } else {
            Some(seconds.max(floor))
        };
        let now = region_time(self.now, self.step);
        self.data.timer = interval.map(|seconds| {
            // Whole microseconds first, as a sleep's: 0.3 as an `f32` is a
            // hair above 0.3 s and must not be read as a tick more.
            let interval =
                Duration::try_from_secs_f64((seconds * 1e6).round() / 1e6).unwrap_or(Duration::MAX);
            Timer {
                interval,
                next: now.saturating_add(interval),
                fired: false,
            }
        });
        // A stop takes the `timer` already queued with it on Second Life and
        // on YEngine (OpenSim's `CancelScriptEvent`), not on XEngine, whose
        // cancel leaves its queue alone; setting another interval leaves it
        // on all three (`events/timer-reset`, 2026-11-05).
        if interval.is_none() && flavour != EngineFlavour::XEngine {
            self.data.drop_queued_timer = true;
        }
    }

    /// `llSensor`: sweep once with `query`, at the next tick. Second Life
    /// keeps one asked-for sweep, the last, and ends a repeating one; OpenSim
    /// sweeps for each and leaves the repeat running (`events/merging`,
    /// `detection/sensor`).
    pub fn sense_once(&mut self, query: crate::vm::sensing::Query) {
        if self.flavour().is_opensim() {
            self.data.sweeps.push(query);
        } else {
            self.data.sweeps = vec![query];
            self.data.repeat = None;
        }
    }

    /// `llSensorRepeat`: sweep with `query` every `seconds`, the first one
    /// interval in — on OpenSim a tenth of a second later — never more often
    /// than every 0.05 s on Second Life and 0.1 s on OpenSim. An interval of
    /// 0 sweeps once at once on Second Life and not at all on OpenSim.
    pub fn sense_repeat(&mut self, query: crate::vm::sensing::Query, seconds: f32) {
        let opensim = self.flavour().is_opensim();
        let seconds = f64::from(seconds);
        if seconds <= 0.0 || seconds.is_nan() {
            self.data.repeat = None;
            if !opensim {
                self.data.sweeps = vec![query];
            }
            return;
        }
        let floor = if opensim { 0.1 } else { 0.05 };
        let interval = Duration::try_from_secs_f64((seconds.max(floor) * 1e6).round() / 1e6)
            .unwrap_or(Duration::MAX);
        let first = if opensim {
            interval.saturating_add(Duration::from_millis(100))
        } else {
            interval
        };
        self.data.repeat = Some(crate::vm::sensing::Repeat {
            query,
            interval,
            next: region_time(self.now, self.step).saturating_add(first),
        });
    }

    /// `llSensorRemove`: end the repeating sweep. Second Life drops a sweep's
    /// event still queued; OpenSim delivers it.
    pub fn sense_remove(&mut self) {
        self.data.repeat = None;
        if !self.flavour().is_opensim() {
            self.data.drop_queued_sensor = true;
        }
    }

    /// The region's calendar time now, in microseconds since the UNIX epoch:
    /// its time at tick 0 and the ticks since.
    pub fn unix_micros(&mut self) -> i64 {
        let since = region_time(self.now, self.step).as_micros();
        self.host
            .epoch_micros()
            .saturating_add(i64::try_from(since).unwrap_or(i64::MAX))
    }

    /// The length of a tick: one over the region's frame rate.
    #[must_use]
    pub const fn step(&self) -> Duration {
        self.step
    }

    /// `llGetTime`: the script's time, in seconds.
    #[must_use]
    pub fn script_time(&self) -> f32 {
        let ticks = u32::try_from(self.data.script_ticks).unwrap_or(u32::MAX);
        let seconds = self.step.saturating_mul(ticks).as_secs_f64();
        // OpenSim rounds it to the millisecond (`Math.Round(…, 3)`).
        crate::num::to_f32(if self.flavour().is_opensim() {
            (seconds * 1000.0).round() / 1000.0
        } else {
            seconds
        })
    }

    /// `llResetTime`: the script's time starts again from zero.
    pub const fn reset_script_time(&mut self) {
        self.data.script_ticks = 0;
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
            id => Some(
                self.pending_run(id)
                    .unwrap_or_else(|| self.peers.is_running(id).unwrap_or(false)),
            ),
        }
    }

    /// The run flag an earlier call of this slice set for the script `id`
    /// and the engine has yet to apply, if one did: what the script reads
    /// from the moment of the call.
    fn pending_run(&self, id: CallerId) -> Option<bool> {
        self.data
            .raised
            .iter()
            .rev()
            .find_map(|raised| match raised {
                Raised::Control {
                    target,
                    control: Control::Run(running),
                } if *target == id => Some(*running),
                _ => None,
            })
    }

    /// The script called `name` in the caller's prim, or [`None`] when the
    /// prim has no script by that name.
    #[must_use]
    pub fn script_named(&self, name: &str) -> Option<CallerId> {
        self.host.script_named(self.caller, name)
    }

    /// Do `control` to the script `target` of the caller's prim once the
    /// slice is over, as the flavour's engine does it to another script; a
    /// run flag set reads back at once ([`Self::script_running`]).
    pub fn control_script(&mut self, target: CallerId, control: Control) {
        self.data.raised.push(Raised::Control { target, control });
    }

    /// `llGetStartString`.
    #[must_use]
    pub fn start_string(&self) -> &str {
        &self.data.start_string
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
            self.data.raised.push(Raised::Linkset(made));
        }
        answer.value
    }

    /// The script's listens, to open, close or turn on and off.
    pub const fn listens(&mut self) -> &mut Listens {
        &mut self.data.listens
    }

    /// Send a link message; the engine delivers it to every script `message`
    /// reaches once the slice is over — the sender too, unless the slice ends
    /// in a reset, which empties its queue.
    pub fn send_linked(&mut self, message: LinkMessage) {
        self.data.raised.push(Raised::Linked(message));
    }

    /// What the script holds of an avatar's permissions.
    #[must_use]
    pub const fn permissions(&self) -> &Permissions {
        &self.data.permissions
    }

    /// `agent` holds `mask` for the script at once, no question put — an
    /// implicit grant, or with no agent and nothing held, OpenSim's release.
    /// `run_time_permissions(mask)` follows once the slice is over.
    pub fn grant_permissions(&mut self, agent: Option<String>, mask: i32) {
        self.data.permissions.grant(agent, mask);
        self.raise_permissions(mask);
    }

    /// Raise `run_time_permissions(mask)` in the script once the slice is
    /// over, holding what it holds.
    pub fn raise_permissions(&mut self, mask: i32) {
        self.data.raised.push(Raised::Permissions(mask));
    }

    /// Put `question` to its avatar through the host; whether it was put —
    /// `false` when the avatar is not in the region. A question put waits for
    /// [`Engine::answer`](crate::vm::Engine::answer).
    pub fn ask_permissions(&mut self, question: Question) -> bool {
        if !self.host.ask_permissions(self.caller, &question) {
            return false;
        }
        let flavour = self.flavour();
        self.data.permissions.ask(question, flavour);
        true
    }

    /// Put the experience `question` to its avatar through the host; whether
    /// it was put — `false` when the avatar is not in the region. A question
    /// put waits for
    /// [`Engine::answer_experience`](crate::vm::Engine::answer_experience).
    pub fn ask_experience(&mut self, question: ExperienceQuestion) -> bool {
        if !self.host.ask_experience(self.caller, &question) {
            return false;
        }
        self.data.permissions.ask_experience(question);
        true
    }

    /// `agent` holds `mask` for the script at once, the experience it is in
    /// allowed before; `experience_permissions(agent)` follows once the slice
    /// is over.
    pub fn grant_experience(&mut self, agent: String, mask: i32) {
        self.data.permissions.grant(Some(agent.clone()), mask);
        self.data.raised.push(Raised::ExperienceGranted(agent));
    }

    /// Raise `experience_permissions_denied(agent, reason)` in the script
    /// once the slice is over, holding what it holds.
    pub fn deny_experience(&mut self, agent: String, reason: ExperienceError) {
        self.data
            .raised
            .push(Raised::ExperienceDenied { agent, reason });
    }

    /// The avatar controls the script took.
    pub(crate) const fn controls(&mut self) -> &mut Option<TakenControls> {
        &mut self.data.controls
    }

    /// Drop `bits` of what the script holds of its granter, as a lapse does
    /// (`llReleaseControls` drops `PERMISSION_TAKE_CONTROLS`).
    pub fn revoke_permissions(&mut self, bits: i32) {
        if let Some(granter) = self.data.permissions.granter().map(str::to_owned) {
            self.data.permissions.revoke(&granter, bits);
        }
    }

    /// The script's HTTP state.
    pub(crate) const fn http(&mut self) -> &mut HttpState {
        &mut self.data.http
    }

    /// The region time of the call: the tick times the step.
    #[must_use]
    pub fn region_time(&self) -> Duration {
        region_time(self.now, self.step)
    }

    /// Raise `http_request(id, method, body)` in the script once the slice is
    /// over: `llRequestURL`'s answer.
    pub fn raise_url_answer(&mut self, id: String, method: &'static str, body: String) {
        self.data
            .raised
            .push(Raised::UrlAnswer { id, method, body });
    }

    /// Raise `http_response(id, status, [], body)` in the script once the
    /// slice is over.
    pub fn raise_http_response(&mut self, id: String, status: i32, body: String) {
        self.data
            .raised
            .push(Raised::HttpResponse { id, status, body });
    }

    /// Raise `dataserver(id, data)` in the script once the slice is over.
    pub fn raise_dataserver(&mut self, id: String, data: String) {
        self.data.raised.push(Raised::Dataserver { id, data });
    }

    /// A fresh key for a request's handle, from the region's seeded source: a
    /// version 4 UUID, as each grid mints one.
    pub fn fresh_key(&mut self) -> String {
        let words = [
            self.random_bits(),
            self.random_bits(),
            self.random_bits(),
            self.random_bits(),
        ];
        let [a, b, c, d] = words;
        let version = (b & 0xffff_0fff) | 0x0000_4000;
        let variant = (c & 0x3fff_ffff) | 0x8000_0000;
        format!(
            "{a:08x}-{:04x}-{:04x}-{:04x}-{:04x}{d:08x}",
            version >> 16,
            version & 0xffff,
            variant >> 16,
            variant & 0xffff,
        )
    }

    /// Raise `dataserver(from, message)` in the scripts `to` once the slice
    /// is over (`osMessageObject`).
    pub fn message_scripts(&mut self, to: Vec<CallerId>, from: String, message: String) {
        self.data
            .raised
            .push(Raised::ObjectMessage { to, from, message });
    }

    /// Raise `transaction_result` in the script once the slice is over.
    pub fn raise_transaction(&mut self, result: TransactionResult) {
        self.data.raised.push(Raised::Transaction(result));
    }

    /// Refuse the call's arguments with `text`, worded for the
    /// [`Self::flavour`]: the engine says it on `DEBUG_CHANNEL` once the
    /// slice is over, through the same flood limit as a run-time error. The
    /// call itself carries on and returns its answer.
    pub fn refuse(&mut self, text: String) {
        self.data.refusals.push(Warning {
            text,
            shouted: false,
            to_owner: false,
        });
    }

    /// Refuse the call's arguments with `text` as [`Self::refuse`] does, but
    /// as a complaint Second Life tells the owner alone — a rule list's
    /// (`errors/owner-lines`). OpenSim says it as any other refusal.
    pub fn refuse_to_owner(&mut self, text: String) {
        self.data.refusals.push(Warning {
            text,
            shouted: false,
            to_owner: true,
        });
    }

    /// Shout `text` on `DEBUG_CHANNEL` once the slice is over, to avatars
    /// and scripts alike, as OpenSim's OSSL, LightShare and MOD warnings are
    /// (`OSSLShoutError`): the call carries on.
    pub fn shout_error(&mut self, text: String) {
        self.data.refusals.push(Warning {
            text,
            shouted: true,
            to_owner: false,
        });
    }

    /// The region's `[OSSL]` configuration.
    #[must_use]
    pub fn ossl(&self) -> &Settings {
        self.peers.ossl()
    }

    /// What the script's owner is in the region, as an `Allow_` line asks.
    #[must_use]
    pub fn standing(&self) -> Standing {
        self.host.standing(self.caller)
    }

    /// Suspend the script for `seconds` once the function returns. A
    /// duration that is not positive (or is NaN) does not suspend.
    pub const fn sleep(&mut self, seconds: f32) {
        if self.request.is_none() || matches!(self.request, Some(Request::Sleep(_))) {
            self.request = Some(Request::Sleep(seconds));
        }
    }

    /// Restart the script once the function returns: globals back to their
    /// initialisers, the stack and the event queue dropped, and `default`'s
    /// `state_entry` next.
    pub const fn reset(&mut self) {
        self.request = Some(Request::Reset);
    }

    /// Run nothing more once the function returns: the script's object is
    /// leaving the region (`llDie`), and the host removes it once control is
    /// back with the grid.
    pub const fn die(&mut self) {
        self.request = Some(Request::Die);
    }

    /// The effect requested, taken once by the VM.
    pub(crate) const fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    /// The list object handed as argument `argument` now holds `list`: the
    /// function changed it in place, and every variable holding it reads
    /// `list` after the call — even one that ends in an error.
    pub(crate) fn write_back(&mut self, argument: usize, list: Vec<Element>) {
        self.written_back = Some((argument, list));
    }

    /// The argument the function changed in place and what it holds now, if
    /// it changed one.
    pub(crate) const fn take_written_back(&mut self) -> Option<(usize, Vec<Element>)> {
        self.written_back.take()
    }

    /// The list the function returns is the very object handed as argument
    /// `argument`, not a copy of it.
    pub(crate) const fn return_argument(&mut self, argument: usize) {
        self.returned = Some(argument);
    }

    /// The argument whose own list the function returned, if it did.
    pub(crate) const fn take_returned(&mut self) -> Option<usize> {
        self.returned.take()
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
