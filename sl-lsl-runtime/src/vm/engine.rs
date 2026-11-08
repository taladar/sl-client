//! The scheduler: every script instance of a region, run a bounded number of
//! instructions per tick, round-robin.

use std::collections::BTreeMap;
use std::sync::Arc;

use sl_types::chat::ChatVolume;

use core::ops::Bound;
use core::time::Duration;

use crate::bytecode::Program;
use crate::library::{Event, event};
use crate::num::ticks_for;
use crate::value::Value;
use crate::vm::comms::{Heard, Overheard};
use crate::vm::contents::Control;
use crate::vm::context::{Peers, Raised, Tick};
use crate::vm::detected::Detected;
use crate::vm::experiences::ExperienceError;
use crate::vm::fault::RuntimeError;
use crate::vm::flavour::EngineFlavour;
use crate::vm::host::{CallerId, Host};
use crate::vm::instance::{
    Arrival, Coalescing, Instance, LOAD_WINDOW_SECONDS, Outcome, PostError, Posted, Slice, restore,
    save,
};
use crate::vm::ossl::{Settings, stock_ossl};
use crate::vm::persistence::{
    CHANGED_REGION_START, OPENSIM_CHANGED_POSITION, RestoreError, SavedScript,
};
use crate::vm::report::{
    Admission, Audience, DEBUG_CHANNEL, FLOOD_GAP_SECONDS, FLOOD_NOTICE, FLOOD_SECONDS,
    FLOOD_WINDOW_SECONDS, FloodGate, FloodTicks, Speech, fault_lines, fault_reach, owner_lines,
    refusal_reach,
};

/// A `DEBUG_CHANNEL` line a slice leaves to say, before the flood gate.
enum Line {
    /// Said in the open: the text, how far avatars hear it and how far
    /// scripts do.
    Said(String, ChatVolume, Option<ChatVolume>),
    /// Told the script's owner alone.
    ToOwner(String),
}

/// How many instructions one script may run per second of region time, as
/// charged — a library call and a loop's back-edge cost more than one
/// ([`BUILTIN_CALL_COST`](crate::vm::BUILTIN_CALL_COST),
/// [`BACKWARD_JUMP_COST`](crate::vm::BACKWARD_JUMP_COST)).
///
/// It is the per-script share of a tick ([`EngineConfig::script_budget`]),
/// stated per second so it holds whatever the heartbeat's step is. Counted in
/// instructions, never in elapsed time — a wall-clock slice would make how far
/// a script gets, and so the order of what scripts do, depend on the machine.
///
/// Measured on aditi (Mono, 2026-09-28, one busy script in a quiet region):
/// an empty `for` loop of a million iterations, 520 charged instructions
/// each, runs at 500 000 iterations a second (four runs, 499 806 to
/// 500 141). 520 × 500 000 is 260 million. A short loop is no measure:
/// `llGetTime` only moves in whole frames, so 50 000 iterations read as four
/// frames or five, and the first run after a save is slower still while Mono
/// compiles the script.
pub const SCRIPT_INSTRUCTIONS_PER_SECOND: u64 = 260_000_000;

/// How many scripts' full shares the region runs per tick before the rest
/// wait for the next — the region-wide budget, as a multiple of the
/// per-script one.
///
/// Measured on aditi (Mauve, 2026-10-29, probe `vm/region-budget`): 1, 16,
/// 64 and 128 busy scripts, each an empty loop of its own, every one of them
/// keeps the rate one runs at alone (469 680, 476 508, 478 536 and 477 162
/// iterations a second); 256 run at half that (241 793), the region's total
/// staying at 61.9 million — 130 one-script shares. An earlier run the same
/// evening read 134 (64.7 million against 481 455). The region's script time
/// rose with the scripts to 23.9 ms, a whole frame at 45 frames a second, and
/// `llGetTime` kept pace with real time throughout: a script sees its rate
/// fall, not its clock slow. Mauve runs some 2 000 mostly idle scripts of
/// its own, so a quiet region may have a little more.
pub const REGION_SCRIPT_SHARES: u64 = 130;

/// A region's scheduling parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    /// The length of one tick.
    pub step: Duration,
    /// Instructions one script may run per tick.
    pub script_budget: u32,
    /// Instructions the region's scripts may run per tick together.
    pub region_budget: u64,
    /// Which queued touch, collision or `changed` a new one merges into.
    pub coalescing: Coalescing,
    /// Which grid's engine the region imitates where they differ: the words
    /// and reach of a run-time error, what becomes of the faulted script,
    /// the flood limit.
    pub flavour: EngineFlavour,
    /// OpenSim's `ScriptDelayFactor` (`[XEngine]` / `[YEngine]`, 1.0 by
    /// default), which scales every forced delay of a library call — not
    /// `llSleep`, nor the second an error costs. Second Life has none, and
    /// its flavour ignores it.
    pub script_delay_factor: f64,
}

impl EngineConfig {
    /// The budgets for a tick of length `step`, from
    /// [`SCRIPT_INSTRUCTIONS_PER_SECOND`] and [`REGION_SCRIPT_SHARES`]. A
    /// step so short that a script's share would round to nothing still
    /// gets one instruction.
    #[must_use]
    pub fn for_step(step: Duration) -> Self {
        let per_script = u128::from(SCRIPT_INSTRUCTIONS_PER_SECOND)
            .saturating_mul(step.as_micros())
            .checked_div(1_000_000)
            .unwrap_or(0)
            .max(1);
        let script_budget = u32::try_from(per_script).unwrap_or(u32::MAX);
        let region_budget =
            u64::try_from(per_script.saturating_mul(u128::from(REGION_SCRIPT_SHARES)))
                .unwrap_or(u64::MAX);
        Self {
            step,
            script_budget,
            region_budget,
            coalescing: Coalescing::Reference,
            flavour: EngineFlavour::SecondLife,
            script_delay_factor: 1.0,
        }
    }
}

/// What one tick did.
#[derive(Debug, Clone, PartialEq)]
pub struct TickReport {
    /// The tick that ran.
    pub tick: Tick,
    /// Every slice run, in the order they ran.
    pub slices: Vec<(CallerId, Slice)>,
    /// Instructions charged, over all scripts.
    pub used: u64,
    /// How many scripts were runnable when the tick started.
    pub runnable: u32,
    /// How many of those got to run before the region's budget ran out —
    /// with [`Self::runnable`], the reference's "scripts run" percentage.
    pub served: u32,
}

/// Every script instance of a region, and the tick they run at.
///
/// Each [`Self::tick`] advances the tick by one and gives every runnable
/// instance up to [`EngineConfig::script_budget`] instructions, until the
/// region's [`EngineConfig::region_budget`] is spent. Instances are served in
/// the order of their [`CallerId`]s, starting just after the one the previous
/// tick served last, so when the region is over budget every script runs
/// slower rather than some not at all. An instance may run several bodies in
/// one tick — a handler, then the next queued event's — while its share
/// lasts. One operation that costs more than is left of the share (a join of
/// a long list) runs to the end, and what it overdrew comes out of the
/// instance's next shares, so a script never runs faster for doing its work
/// in large pieces.
#[derive(Debug, Clone)]
pub struct Engine {
    /// The scheduling parameters.
    config: EngineConfig,
    /// The current tick.
    now: Tick,
    /// The instances, in serving order.
    instances: BTreeMap<CallerId, Instance>,
    /// The instance the previous tick served last.
    last_served: Option<CallerId>,
    /// Second Life's per-object flood limit on error lines.
    floods: FloodGate,
    /// How many scripts have been running a tick, averaged over
    /// [`LOAD_WINDOW_SECONDS`]: Second Life's active-script count.
    active: f64,
    /// The region's `[OSSL]` configuration, which an OpenSim flavour's
    /// OSSL, LightShare and MOD calls pass; [`None`] for the stock one.
    ossl: Option<Arc<Settings>>,
    /// Lines heard on OpenSim that wait for its next async command pass, in
    /// the order they were said.
    overheard: Vec<(CallerId, Heard)>,
    /// The scale each prim rescaled since its last bounding-box refresh on
    /// Second Life had before, by key: what its box still measures until
    /// the refresh its [`BoxClock`] holds it to.
    stale_boxes: BTreeMap<String, sl_types::lsl::Vector>,
    /// When each prim's bounding box last caught up with its scale on Second
    /// Life, and when a rescale since will show, by key.
    box_clocks: BTreeMap<String, BoxClock>,
}

/// When one prim's bounding box catches up with its scale on Second Life: at
/// the next tick after a rescale, unless it caught up less than
/// [`SECOND_LIFE_BOX_REFRESH`] before, when it waits until that long after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoxClock {
    /// The tick the box last caught up in, if it ever had to.
    refreshed: Option<Tick>,
    /// The tick a rescale waiting in [`Engine::stale_boxes`] shows in.
    due: Tick,
}

/// How many ticks after its raise Second Life runs a link message: the next
/// frame, to the sender too (`events/latency`, aditi 2026-11-08: one frame
/// to itself, two for an echo's answer).
const SECOND_LIFE_LINK_TICKS: u64 = 1;

/// How many ticks after its call Second Life runs a setter's `changed` and
/// a sweep's answer (`events/latency`: two frames for both).
const SECOND_LIFE_LATE_TICKS: u64 = 2;

/// How long Second Life keeps a prim's bounding box behind a new scale after
/// the box last caught up: a rescale shows in `llGetBoundingBox` the frame
/// after it, unless the box caught up less than a second before, when it
/// shows a second after that (`prim/box-beat`, aditi 2026-11-08: twenty
/// rescales across the second, each seen a frame after its set or exactly a
/// second after the box's last catch-up, whichever came later; at 45 frames
/// a second the region's clock and its frames cannot be told apart).
const SECOND_LIFE_BOX_REFRESH: Duration = Duration::from_secs(1);

/// How often OpenSim's async command pass delivers the lines its scripts
/// heard (`AsyncCommandManager`'s `AsyncLLCommandLoopms`, 100 ms by default):
/// `WorldCommModule.DeliverMessage` only queues a line, and the pass hands it
/// to the listening scripts (`events/latency`: an echo's answer 0.1 s, the
/// first hop alone anything from nothing to 0.1 s).
const OPENSIM_ASYNC_PASS: Duration = Duration::from_millis(100);

/// One script's line in the region's top-scripts report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptUsage {
    /// The script.
    pub id: CallerId,
    /// What it adds to its object's script memory
    /// ([`Instance::script_memory`]).
    pub memory: u32,
    /// The script time it has been taking each second, averaged over the
    /// last [`LOAD_WINDOW_SECONDS`]: its load at
    /// [`SCRIPT_INSTRUCTIONS_PER_SECOND`].
    pub time: Duration,
}

impl Engine {
    /// An empty region at tick zero.
    #[must_use]
    pub const fn new(config: EngineConfig) -> Self {
        Self {
            config,
            now: Tick(0),
            instances: BTreeMap::new(),
            last_served: None,
            floods: FloodGate::new(),
            active: 0.0,
            ossl: None,
            overheard: Vec::new(),
            stale_boxes: BTreeMap::new(),
            box_clocks: BTreeMap::new(),
        }
    }

    /// The region's `[OSSL]` configuration: the stock
    /// `osslDefaultEnable.ini` until [`Self::set_ossl`].
    #[must_use]
    pub fn ossl(&self) -> &Settings {
        self.ossl.as_deref().unwrap_or_else(|| stock_ossl())
    }

    /// Run the region's OSSL, LightShare and MOD calls under `config`.
    pub fn set_ossl(&mut self, config: Settings) {
        self.ossl = Some(Arc::new(config));
    }

    /// The scheduling parameters.
    #[must_use]
    pub const fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// The current tick: the last one run, zero before the first.
    #[must_use]
    pub const fn now(&self) -> Tick {
        self.now
    }

    /// Start a new instance of `program` under `id`, replacing — and
    /// returning — any instance that had it. It runs its initialisers and
    /// `default`'s `state_entry` from the next tick.
    pub fn add(&mut self, id: CallerId, program: Arc<Program>) -> Option<Instance> {
        self.instances
            .insert(id, Instance::with_flavour(program, self.config.flavour))
    }

    /// What the script `id` costs the region: its script memory and the time
    /// it has been taking; [`None`] when there is no such instance.
    #[must_use]
    pub fn usage(&self, id: CallerId) -> Option<ScriptUsage> {
        let instance = self.instances.get(&id)?;
        let ticks_per_second =
            Duration::from_secs(1).as_secs_f64() / self.config.step.as_secs_f64();
        let rate = f64::from(u32::try_from(SCRIPT_INSTRUCTIONS_PER_SECOND).unwrap_or(u32::MAX));
        let seconds = instance.load() * ticks_per_second / rate;
        Some(ScriptUsage {
            id,
            memory: instance.script_memory(self.config.flavour),
            time: Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO),
        })
    }

    /// The region's top-scripts report: every script's [`ScriptUsage`], the
    /// one taking the most time first, a tie in the order of their ids.
    #[must_use]
    pub fn top_scripts(&self) -> Vec<ScriptUsage> {
        let mut usage: Vec<ScriptUsage> = self
            .instances
            .keys()
            .filter_map(|id| self.usage(*id))
            .collect();
        usage.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
        usage
    }

    /// The region's active-script count, as its `SimStats` `ActiveScripts`
    /// says it: on Second Life the scripts running, averaged over
    /// [`LOAD_WINDOW_SECONDS`] (a fractional figure); on OpenSim every script
    /// in the region, running or not (`memory/object`, 2026-10-26).
    #[must_use]
    pub fn active_scripts(&self) -> f64 {
        match self.config.flavour {
            EngineFlavour::SecondLife => self.active,
            EngineFlavour::YEngine | EngineFlavour::XEngine => {
                f64::from(u32::try_from(self.instances.len()).unwrap_or(u32::MAX))
            }
        }
    }

    /// Remove an instance, returning it.
    pub fn remove(&mut self, id: CallerId) -> Option<Instance> {
        self.forget_pending(id);
        self.instances.remove(&id)
    }

    /// Drop the lines on their way to the instance `id`, which is leaving.
    fn forget_pending(&mut self, id: CallerId) {
        self.overheard.retain(|(to, _heard)| *to != id);
    }

    /// The instance `id` names.
    #[must_use]
    pub fn instance(&self, id: CallerId) -> Option<&Instance> {
        self.instances.get(&id)
    }

    /// The instance `id` names, to change its run flag or reset it.
    pub fn instance_mut(&mut self, id: CallerId) -> Option<&mut Instance> {
        self.instances.get_mut(&id)
    }

    /// Offer the instance `id` an event; [`None`] when there is no such
    /// instance.
    ///
    /// # Errors
    ///
    /// [`PostError`] when `args` does not match the event's parameters.
    pub fn post(
        &mut self,
        id: CallerId,
        event: &'static Event,
        args: Vec<Value>,
    ) -> Result<Option<Posted>, PostError> {
        let arrival = self.arrival();
        self.instances
            .get_mut(&id)
            .map(|instance| instance.post(arrival, event, args))
            .transpose()
    }

    /// Offer the instance `id` a detection event — a touch, a collision, a
    /// sensor sweep — with who or what was detected; [`None`] when there is
    /// no such instance. Posted again in the same tick, a touch or collision
    /// merges into the one already queued.
    ///
    /// # Errors
    ///
    /// [`PostError`] when `event` is not a detection event or the block is
    /// empty or too long.
    pub fn post_detected(
        &mut self,
        id: CallerId,
        event: &'static Event,
        detected: Vec<Detected>,
    ) -> Result<Option<Posted>, PostError> {
        let arrival = self.arrival();
        self.instances
            .get_mut(&id)
            .map(|instance| instance.post_detected(arrival, event, detected))
            .transpose()
    }

    /// Offer the instance `id` a line `heard` in earshot of it: it raises
    /// `listen(channel, name, speaker, message)` once for each of its active
    /// listens that admits the line, and nothing when none does. [`None`]
    /// when there is no such instance; otherwise what became of each event
    /// raised, in the order of the listens.
    ///
    /// Who is in earshot is the host's to decide — the ranges, a region-say's
    /// whole region, a `llRegionSayTo`'s one target, and never the speaking
    /// prim's own scripts.
    ///
    /// # Errors
    ///
    /// None in practice: `listen` takes an integer, two strings and a key.
    pub fn hear(&mut self, id: CallerId, heard: &Heard) -> Result<Option<Vec<Posted>>, PostError> {
        let Some(admitting) = self
            .instances
            .get(&id)
            .map(|instance| instance.listens().admitting(heard))
        else {
            return Ok(None);
        };
        let args = vec![
            Value::Integer(heard.channel),
            Value::String(heard.name.clone()),
            Value::Key(heard.speaker.clone()),
            Value::String(heard.message.clone()),
        ];
        let mut posted = Vec::with_capacity(admitting);
        for _ in 0..admitting {
            if let Some(outcome) = self.post(id, lifecycle("listen"), args.clone())? {
                posted.push(outcome);
            }
        }
        Ok(Some(posted))
    }

    /// Offer the instance `id` a line `heard` when its flavour delivers chat
    /// to scripts: at once on Second Life ([`Self::hear`]), at the next of
    /// OpenSim's async command passes, every 100 ms of region time, there
    /// (`events/latency`).
    pub fn overhear(&mut self, id: CallerId, heard: &Heard) {
        if self.config.flavour.is_opensim() {
            self.overheard.push((id, heard.clone()));
        } else {
            // `listen` takes an integer, two strings and a key: it never
            // fails.
            let _posted = self.hear(id, heard);
        }
    }

    /// Offer every script `overheard` reaches its line, as
    /// [`Self::overhear`] does — but never the speaking script `speaker`,
    /// whose prim does not hear its own line.
    fn overhear_all(&mut self, speaker: CallerId, overheard: &Overheard) {
        for to in &overheard.by {
            if *to != speaker {
                self.overhear(*to, &overheard.heard);
            }
        }
    }

    /// Raise `changed(change)` in the instance `id` — the one function every
    /// raiser calls (an inventory change, a link, a colour, a scale, an owner,
    /// a region crossing, a teleport, …), with the `CHANGED_*` bits of what
    /// happened. Whether it merges into a `changed` already queued is the
    /// region's [`Coalescing`]; by Second Life's rule, changes of one tick
    /// always do, and later ones join the newest queued `changed` unless it
    /// is next in line.
    ///
    /// # Errors
    ///
    /// None in practice: `changed` takes one integer.
    pub fn changed(&mut self, id: CallerId, change: i32) -> Result<Option<Posted>, PostError> {
        self.post(id, lifecycle("changed"), vec![Value::Integer(change)])
    }

    /// The object the instance `id` is in was worn by `wearer`, or — for
    /// [`None`] — taken off: raise `attach(wearer)`, `attach(NULL_KEY)` for
    /// a drop.
    ///
    /// # Errors
    ///
    /// None in practice: `attach` takes one key.
    pub fn attach(
        &mut self,
        id: CallerId,
        wearer: Option<&str>,
    ) -> Result<Option<Posted>, PostError> {
        let key = wearer.unwrap_or(crate::value::NULL_KEY).to_owned();
        self.post(id, lifecycle("attach"), vec![Value::Key(key)])
    }

    /// The object the instance `id` is in was rezzed with `start_parameter`:
    /// set what `llGetStartParameter` answers, then raise
    /// `on_rez(start_parameter)`.
    ///
    /// # Errors
    ///
    /// None in practice: `on_rez` takes one integer.
    pub fn rez(&mut self, id: CallerId, start_parameter: i32) -> Result<Option<Posted>, PostError> {
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.set_start_parameter(start_parameter);
        }
        self.post(
            id,
            lifecycle("on_rez"),
            vec![Value::Integer(start_parameter)],
        )
    }

    /// The avatar `agent` answered the permission question the instance `id`
    /// put to it with `granted` — a viewer's `ScriptAnswerYes`, whose mask is
    /// held as it came, more than was asked included — and
    /// `run_time_permissions(granted)` is raised. [`None`] when there is no
    /// such instance or it is not waiting for `agent`'s answer: an answer
    /// nobody asked for is nothing.
    ///
    /// # Errors
    ///
    /// None in practice: `run_time_permissions` takes one integer.
    pub fn answer(
        &mut self,
        id: CallerId,
        agent: &str,
        granted: i32,
    ) -> Result<Option<Posted>, PostError> {
        let answered = self
            .instances
            .get_mut(&id)
            .is_some_and(|instance| instance.permissions_mut().answer(agent, granted));
        if !answered {
            return Ok(None);
        }
        self.post(
            id,
            lifecycle("run_time_permissions"),
            vec![Value::Integer(granted)],
        )
    }

    /// The avatar `agent` answered the experience question the instance `id`
    /// put to it: [`Ok`] raises `experience_permissions(agent)`, the grant
    /// holding [`EXPERIENCE_PERMISSIONS`] for `agent`; an error raises
    /// `experience_permissions_denied(agent, reason)` — the host's
    /// [`ExperienceError::NotPermitted`] for a refusal or a block,
    /// [`ExperienceError::RequestTimedOut`] for a question nobody answered.
    /// [`None`] when there is no such instance or it is not waiting for
    /// `agent`'s answer. The host keeps the answer for the experience; this
    /// is only the one script's share of it.
    ///
    /// [`EXPERIENCE_PERMISSIONS`]: crate::vm::EXPERIENCE_PERMISSIONS
    ///
    /// # Errors
    ///
    /// None in practice: both events take a key, and an integer.
    pub fn answer_experience(
        &mut self,
        id: CallerId,
        agent: &str,
        answer: Result<(), ExperienceError>,
    ) -> Result<Option<Posted>, PostError> {
        let answered = self.instances.get_mut(&id).is_some_and(|instance| {
            instance
                .permissions_mut()
                .answer_experience(agent, answer.is_ok())
        });
        if !answered {
            return Ok(None);
        }
        match answer {
            Ok(()) => self.post(
                id,
                lifecycle("experience_permissions"),
                vec![Value::Key(agent.to_owned())],
            ),
            Err(reason) => self.post(
                id,
                lifecycle("experience_permissions_denied"),
                vec![Value::Key(agent.to_owned()), Value::Integer(reason.code())],
            ),
        }
    }

    /// The avatar `agent`'s viewer revoked `bits` of what it granted the
    /// instance `id` (`RevokePermissions`; the host applies one naming the
    /// region to every script, one naming an object to that object's). The
    /// bits go, the granter stays unless every bit went; no event is raised,
    /// on any grid. A grant `agent` did not make is left alone. The host
    /// revokes the same way what a lapse drops: `PERMISSION_TAKE_CONTROLS`
    /// when the object is dropped, `PERMISSION_TAKE_CONTROLS` and
    /// `PERMISSION_CONTROL_CAMERA` when its sitter stands.
    pub fn revoke(&mut self, id: CallerId, agent: &str, bits: i32) {
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.permissions_mut().revoke(agent, bits);
        }
    }

    /// The object the instance `id`'s script rezzed (`llRezObject` and its
    /// family) is in the region and its scripts have run their `on_rez`:
    /// raise `object_rez(object)` in the rezzer. The host calls this a tick
    /// after it brought the child's scripts in, which is why the rezzer
    /// hears of the rez after the child ran (`inventory/rez`).
    ///
    /// # Errors
    ///
    /// None in practice: `object_rez` takes one key.
    pub fn object_rezzed(
        &mut self,
        id: CallerId,
        object: &str,
    ) -> Result<Option<Posted>, PostError> {
        self.post(
            id,
            lifecycle("object_rez"),
            vec![Value::Key(object.to_owned())],
        )
    }

    /// `payer` paid the prim the instance `id` is in `amount` L$, and the
    /// grid took it: raise `money(payer, amount)`. The host offers every
    /// script of the prim the payment; one whose state has no `money` handler
    /// hears nothing, and the payment stands all the same.
    ///
    /// # Errors
    ///
    /// None in practice: `money` takes a key and an integer.
    pub fn paid(
        &mut self,
        id: CallerId,
        payer: &str,
        amount: i32,
    ) -> Result<Option<Posted>, PostError> {
        self.post(
            id,
            lifecycle("money"),
            vec![Value::Key(payer.to_owned()), Value::Integer(amount)],
        )
    }

    /// The answer to the request `request` the instance `id` sent
    /// (`llHTTPRequest`) came back: `http_response(request, status, metadata,
    /// body)`, the body cut at the request's `HTTP_BODY_MAXLENGTH` as its
    /// flavour cuts it — on Second Life in every script of the prim, the
    /// others of which `prim` names; on OpenSim in the sender alone. [`None`]
    /// when there is no such instance, or it is not waiting on that request
    /// (a reset forgot it).
    ///
    /// # Errors
    ///
    /// None in practice: `http_response` takes a key, an integer, a list and
    /// a string.
    pub fn http_response(
        &mut self,
        id: CallerId,
        request: &str,
        status: i32,
        body: &str,
        prim: &[CallerId],
    ) -> Result<Option<Posted>, PostError> {
        let flavour = self.config.flavour;
        let Some(max) = self
            .instances
            .get_mut(&id)
            .and_then(|instance| instance.data_mut().http.pending.remove(request))
        else {
            return Ok(None);
        };
        let (body, metadata) = crate::vm::http::response_body(flavour, body, max);
        let args = vec![
            Value::Key(request.to_owned()),
            Value::Integer(status),
            Value::List(
                metadata
                    .into_iter()
                    .map(crate::value::Element::Integer)
                    .collect(),
            ),
            Value::String(body),
        ];
        if flavour == EngineFlavour::SecondLife {
            for other in prim.iter().filter(|other| **other != id) {
                let _posted = self.post(*other, lifecycle("http_response"), args.clone())?;
            }
        }
        self.post(id, lifecycle("http_response"), args)
    }

    /// A request arrived at one of the instance `id`'s URLs, the grid's URL
    /// surface having handed it on ([`crate::vm::http::arrive`]):
    /// `http_request(request, method, body)`, its headers kept for
    /// `llGetHTTPHeader` and `llHTTPResponse` until it is answered. [`None`]
    /// when there is no such instance.
    ///
    /// # Errors
    ///
    /// None in practice: `http_request` takes a key and two strings.
    pub fn http_request(
        &mut self,
        id: CallerId,
        request: &str,
        inbound: crate::vm::http::Inbound,
    ) -> Result<Option<Posted>, PostError> {
        let Some(instance) = self.instances.get_mut(&id) else {
            return Ok(None);
        };
        let args = vec![
            Value::Key(request.to_owned()),
            Value::String(inbound.method.clone()),
            Value::String(inbound.body.clone()),
        ];
        let _previous = instance
            .data_mut()
            .http
            .inbound
            .insert(request.to_owned(), inbound);
        self.post(id, lifecycle("http_request"), args)
    }

    /// The keys `held` down at this tick by the avatar whose controls the
    /// instance `id` took: `control(agent, level, edge)` when the hold
    /// changed, or as its flavour repeats a hold. [`None`] when there is no
    /// such instance, it took no controls of `agent`, or nothing is raised.
    ///
    /// # Errors
    ///
    /// None in practice: `control` takes a key and two integers.
    pub fn controls(
        &mut self,
        id: CallerId,
        agent: &str,
        held: i32,
    ) -> Result<Option<Posted>, PostError> {
        let flavour = self.config.flavour;
        let Some(raised) = self.instances.get_mut(&id).and_then(|instance| {
            let taken = instance.data_mut().controls.as_mut()?;
            if taken.agent != agent {
                return None;
            }
            taken.events.tick(flavour, held)
        }) else {
            return Ok(None);
        };
        let (level, edge) = raised;
        self.post(
            id,
            lifecycle("control"),
            vec![
                Value::Key(agent.to_owned()),
                Value::Integer(level),
                Value::Integer(edge),
            ],
        )
    }

    /// The avatar and controls the instance `id` holds taken: what a host
    /// routes the avatar's keys to, releasing them when it no longer does (a
    /// reset drops them).
    #[must_use]
    pub fn taken_controls(&self, id: CallerId) -> Option<(&str, i32)> {
        self.instances.get(&id).and_then(|instance| {
            instance
                .data()
                .controls
                .as_ref()
                .map(|taken| (taken.agent.as_str(), taken.events.taken))
        })
    }

    /// The URLs the instance `id` holds: what a host keeps granted to it,
    /// releasing the rest (a reset drops them all).
    #[must_use]
    pub fn urls(&self, id: CallerId) -> &[String] {
        self.instances
            .get(&id)
            .map_or(&[], |instance| instance.data().http.urls.as_slice())
    }

    /// The object the instance `id` is in was taken into inventory: the
    /// instance leaves the region, and is handed back saved for the grid to
    /// keep with the item ([`Self::rez_saved`] brings it back). [`None`] when
    /// there is no such instance.
    ///
    /// What is saved is all the script holds, a sleep it is in included —
    /// on Second Life the sleep counts on through the take and the rez, and
    /// the handler ends after the rez, its full time asleep. YEngine ends the
    /// sleep at the take: the handler goes on as soon as the object is
    /// rezzed (`persistence/rerez`, 2026-10-26). XEngine resets the script on
    /// the rez, so what it saves is never run.
    pub fn take(&mut self, id: CallerId) -> Option<SavedScript> {
        let saved = self.save(id)?;
        self.forget_pending(id);
        let _taken = self.instances.remove(&id);
        Some(saved)
    }

    /// The instance `id` saved as [`Self::take`] saves it, the instance left
    /// running where it is: what a take of a copy files, while the object it
    /// copied runs on. [`None`] when there is no such instance.
    #[must_use]
    pub fn save(&self, id: CallerId) -> Option<SavedScript> {
        let instance = self.instances.get(&id)?;
        let mut saved = save(instance, self.now, self.config.step);
        if self.config.flavour == EngineFlavour::YEngine {
            saved.wake();
        }
        Some(saved)
    }

    /// The object a script was taken in is rezzed again with
    /// `start_parameter`, `away` after the take: bring `saved` back under
    /// `id` as an instance of `program` (the inventory item's), replacing any
    /// instance that had the id, and raise `on_rez(start_parameter)`.
    ///
    /// What it finds is the grid's (`persistence/rerez`, 2026-10-26):
    ///
    /// - **Second Life** keeps everything — the globals and the state, the
    ///   permissions and their granter, the timer and the listens, the queued
    ///   events, a stopped script stopped — and the time away counts against
    ///   a sleep and the timer. A body in progress finishes first, then
    ///   `on_rez`, then the events queued before the take.
    /// - **YEngine** keeps the same, but the time away counts against
    ///   nothing (OpenSim saves a timer as the time left of it, and its sleep
    ///   ended at the take): the body in progress, the queued events, then
    ///   `on_rez`.
    /// - **XEngine** resets the script, a stopped one included, which runs
    ///   again: its initialisers, `default`'s `state_entry`, then `on_rez`.
    ///
    /// No grid raises a `changed` for it.
    ///
    /// # Errors
    ///
    /// [`RestoreError`] when `saved` does not fit `program` or was saved
    /// under another engine; nothing is added then.
    pub fn rez_saved(
        &mut self,
        id: CallerId,
        program: Arc<Program>,
        saved: &SavedScript,
        away: Duration,
        start_parameter: i32,
    ) -> Result<Posted, RestoreError> {
        let flavour = self.config.flavour;
        let mut saved = saved.clone();
        if flavour == EngineFlavour::SecondLife {
            saved.elapse(away);
        }
        let mut instance = restore(program, &saved, flavour, self.now, self.config.step)?;
        if flavour == EngineFlavour::XEngine {
            instance.set_running(true);
            instance.reset();
        }
        instance.set_start_parameter(start_parameter);
        let arrival = self.arrival();
        let args = vec![Value::Integer(start_parameter)];
        let posted = match flavour {
            EngineFlavour::SecondLife => instance.post_first(arrival, lifecycle("on_rez"), args),
            EngineFlavour::YEngine | EngineFlavour::XEngine => {
                instance.post(arrival, lifecycle("on_rez"), args)
            }
        };
        let _replaced = self.instances.insert(id, instance);
        Ok(posted?)
    }

    /// The region is going down: every instance leaves it, handed back saved
    /// under its id for the grid to keep with the region's objects
    /// ([`Self::start_up`] brings them back).
    ///
    /// YEngine ends a sleep at the shutdown, and the handler goes on once the
    /// region is back; XEngine loses the body in progress — a sleeping
    /// handler never finishes — though the events queued behind it run
    /// (`persistence/restart`, 2026-10-26). Second Life is not measured
    /// (`gridspec-aditi-test-land`): it keeps the sleep as a take does.
    pub fn shut_down(&mut self) -> Vec<(CallerId, SavedScript)> {
        let now = self.now;
        let step = self.config.step;
        let flavour = self.config.flavour;
        self.last_served = None;
        core::mem::take(&mut self.instances)
            .into_iter()
            .map(|(id, instance)| {
                let mut saved = save(&instance, now, step);
                match flavour {
                    EngineFlavour::SecondLife => {}
                    EngineFlavour::YEngine => saved.wake(),
                    EngineFlavour::XEngine => {
                        saved.abandon();
                        saved.separate_lists();
                    }
                }
                (id, saved)
            })
            .collect()
    }

    /// The region is up again, `down` after [`Self::shut_down`]: bring every
    /// saved script back under its id as an instance of its program, and
    /// raise in each what tells it the region started — on OpenSim
    /// `changed(OPENSIM_CHANGED_POSITION)` then
    /// `changed(CHANGED_REGION_START)`, behind the events it had queued; on
    /// Second Life `changed(CHANGED_REGION_START)` alone (not measured, and
    /// the constant is all the reference documents). No grid runs
    /// `state_entry` or `on_rez`.
    ///
    /// Both OpenSim engines keep the globals, the state, the permissions,
    /// the timer, the listens and the queue; the time down counts against
    /// nothing. XEngine runs a stopped script again. On Second Life the time
    /// down counts against a sleep and the timer, as the time away of a take
    /// does.
    ///
    /// # Errors
    ///
    /// The id and the [`RestoreError`] of the first script that does not fit
    /// its program; no script is added then.
    pub fn start_up(
        &mut self,
        scripts: Vec<(CallerId, Arc<Program>, SavedScript)>,
        down: Duration,
    ) -> Result<(), (CallerId, RestoreError)> {
        let flavour = self.config.flavour;
        let mut restored = Vec::with_capacity(scripts.len());
        for (id, program, mut saved) in scripts {
            if flavour == EngineFlavour::SecondLife {
                saved.elapse(down);
            }
            let mut instance = restore(program, &saved, flavour, self.now, self.config.step)
                .map_err(|error| (id, error))?;
            if flavour == EngineFlavour::XEngine {
                instance.set_running(true);
            }
            restored.push((id, instance));
        }
        let changes: &[i32] = match flavour {
            EngineFlavour::SecondLife => &[CHANGED_REGION_START],
            EngineFlavour::YEngine | EngineFlavour::XEngine => {
                &[OPENSIM_CHANGED_POSITION, CHANGED_REGION_START]
            }
        };
        let ids: Vec<CallerId> = restored.iter().map(|(id, _)| *id).collect();
        self.instances.extend(restored);
        for id in ids {
            for change in changes {
                let _posted = self
                    .changed(id, *change)
                    .map_err(|error| (id, RestoreError::from(error)))?;
            }
        }
        Ok(())
    }

    /// An event posted now: in the current tick, merging by the region's
    /// policy, its handler free to start at once.
    const fn arrival(&self) -> Arrival {
        Arrival {
            tick: self.now,
            coalescing: self.config.coalescing,
            ready: self.now,
        }
    }

    /// Say on `DEBUG_CHANNEL` what went wrong in `slice` of the instance
    /// `id`: the refusals of its library calls, then its fault, each line in
    /// the flavour's words and reach and, on Second Life, through the
    /// object's flood limit.
    fn report(&mut self, id: CallerId, slice: &Slice, host: &mut dyn Host) {
        let flavour = self.config.flavour;
        let fault = match &slice.outcome {
            Outcome::Faulted(fault) => Some(fault),
            Outcome::Idle
            | Outcome::Yielded
            | Outcome::Finished
            | Outcome::Sleeping(_)
            | Outcome::StateChanged(_)
            | Outcome::Reset
            | Outcome::Died => None,
        };
        if slice.refusals.is_empty() && fault.is_none() {
            return;
        }
        let at = host.whereabouts(id);
        for line in fault
            .map(|fault| owner_lines(flavour, fault, &at))
            .unwrap_or_default()
        {
            host.owner_say(id, &line);
        }
        let refused = slice.refusals.iter().map(|warning| {
            if warning.shouted {
                Line::Said(
                    warning.text.clone(),
                    ChatVolume::Shout,
                    Some(ChatVolume::Shout),
                )
            } else if warning.to_owner && !flavour.is_opensim() {
                Line::ToOwner(warning.text.clone())
            } else {
                Line::Said(
                    warning.text.clone(),
                    ChatVolume::Say,
                    Some(refusal_reach(flavour)),
                )
            }
        });
        // An OSSL error marked for the owner goes to the owner alone, on
        // `DEBUG_CHANNEL` (`PermissionErrorToOwner`).
        let to_owner = fault.is_some_and(
            |fault| matches!(&fault.error, RuntimeError::Thrown { thrown, .. } if thrown.to_owner),
        );
        let fault_lines = fault
            .map(|fault| fault_lines(flavour, fault, &at))
            .unwrap_or_default();
        let faulted: Vec<Line> = if to_owner {
            fault_lines.into_iter().map(Line::ToOwner).collect()
        } else {
            fault_lines
                .into_iter()
                .map(|text| Line::Said(text, ChatVolume::Say, fault_reach(flavour)))
                .collect()
        };
        let ticks = FloodTicks {
            window: ticks_for(FLOOD_WINDOW_SECONDS, self.config.step),
            flood: ticks_for(FLOOD_SECONDS, self.config.step),
            gap: ticks_for(FLOOD_GAP_SECONDS, self.config.step),
        };
        for line in refused.chain(faulted) {
            let audience = match &line {
                Line::Said(..) => Audience::Near,
                Line::ToOwner(_) => Audience::Owner,
            };
            let admission = match flavour {
                EngineFlavour::SecondLife => {
                    self.floods.admit(&at.object, audience, self.now, ticks)
                }
                EngineFlavour::YEngine | EngineFlavour::XEngine => Admission::Pass,
            };
            let heard = match (admission, line) {
                (Admission::Drop, _) => None,
                (Admission::Pass, Line::ToOwner(text)) => {
                    let owner = host.owner(id);
                    host.say_to(id, &owner, DEBUG_CHANNEL, &text)
                }
                (Admission::Pass, Line::Said(text, to_avatars, to_scripts)) => host.say(
                    id,
                    &Speech {
                        channel: DEBUG_CHANNEL,
                        text,
                        to_avatars,
                        to_scripts,
                    },
                ),
                (Admission::Notice, _) => host.say(
                    id,
                    &Speech {
                        channel: DEBUG_CHANNEL,
                        text: FLOOD_NOTICE.to_owned(),
                        to_avatars: ChatVolume::Say,
                        to_scripts: fault_reach(flavour),
                    },
                ),
            };
            if let Some(overheard) = heard {
                self.overhear_all(id, &overheard);
            }
        }
    }

    /// Post what `slice` of the instance `id` raised, in the order of its
    /// calls: `linkset_data` for each change of the linkset's store in every
    /// script of the linkset, `link_message(sender, num, text, id)` for
    /// each link message in every script it reaches
    /// ([`Links::reached`](crate::vm::Links::reached)), OpenSim's in-call
    /// `changed` in every script of the changed prim and its in-call sweep's
    /// `sensor` or `no_sensor`, and its own `run_time_permissions` and
    /// `transaction_result`. `instance` itself
    /// gets its share too, unless the slice ended in a reset, which emptied
    /// its queue: on Second Life an event is queued at the call, so a reset
    /// after it in the same event drops it (`data/linkset-data`'s
    /// `after-script-reset`).
    fn raise(&mut self, id: CallerId, instance: &mut Instance, slice: &Slice, host: &dyn Host) {
        if slice.raised.is_empty() {
            return;
        }
        let own = !matches!(slice.outcome, Outcome::Reset);
        let arrival = self.arrival();
        let flavour = self.config.flavour;
        let linkset = host.linkset(id);
        let links = host.links(id);
        for raised in &slice.raised {
            let (event, args, reached) = match raised {
                Raised::Control { target, control } => {
                    if *target == id {
                        apply_control(instance, *control, flavour);
                    } else if let Some(peer) = self.instances.get_mut(target) {
                        apply_control(peer, *control, flavour);
                    }
                    continue;
                }
                Raised::Sweep(query) => {
                    let sweeper = sweeper_of(id, host);
                    let candidates = host.sense(id);
                    let Some(found) =
                        crate::vm::sensing::sweep(query, &sweeper, &candidates, flavour)
                    else {
                        continue;
                    };
                    if own {
                        let _posted = if found.is_empty() {
                            instance.post(arrival, lifecycle("no_sensor"), Vec::new())
                        } else {
                            instance.post_detected(arrival, lifecycle("sensor"), found)
                        };
                    }
                    continue;
                }
                Raised::Heard(overheard) => {
                    self.overhear_all(id, overheard);
                    continue;
                }
                Raised::Rescaled { prim, before } => {
                    // The scale at the last refresh: a second rescale before
                    // the next keeps the first one's, and its time.
                    if !self.stale_boxes.contains_key(prim) {
                        let _kept = self.stale_boxes.insert(prim.clone(), before.clone());
                        let next = self.now.after(1);
                        let period = self.box_refresh_ticks();
                        // A box that never caught up counts from the rez.
                        let clock =
                            self.box_clocks
                                .entry(prim.clone())
                                .or_insert_with(|| BoxClock {
                                    refreshed: host.rezzed_in(prim),
                                    due: next,
                                });
                        clock.due = clock
                            .refreshed
                            .map_or(next, |refreshed| next.max(refreshed.after(period)));
                    }
                    continue;
                }
                Raised::Changed { prim, bits } => (
                    lifecycle("changed"),
                    vec![Value::Integer(*bits)],
                    host.prim_scripts(prim).unwrap_or_default(),
                ),
                Raised::Linkset(change) => (
                    lifecycle("linkset_data"),
                    vec![
                        Value::Integer(change.action.code()),
                        Value::String(change.name.clone()),
                        Value::String(change.value.clone()),
                    ],
                    core::iter::once(id)
                        .chain(
                            self.instances
                                .keys()
                                .copied()
                                .filter(|other| host.linkset(*other) == linkset),
                        )
                        .collect::<Vec<_>>(),
                ),
                Raised::Linked(message) => (
                    lifecycle("link_message"),
                    vec![
                        Value::Integer(links.this),
                        Value::Integer(message.num),
                        Value::String(message.text.clone()),
                        Value::Key(message.id.clone()),
                    ],
                    links.reached(message.target, flavour),
                ),
                Raised::Permissions(mask) => (
                    lifecycle("run_time_permissions"),
                    vec![Value::Integer(*mask)],
                    vec![id],
                ),
                Raised::UrlAnswer {
                    id: handle,
                    method,
                    body,
                } => (
                    lifecycle("http_request"),
                    vec![
                        Value::Key(handle.clone()),
                        Value::String((*method).to_owned()),
                        Value::String(body.clone()),
                    ],
                    vec![id],
                ),
                Raised::HttpResponse {
                    id: handle,
                    status,
                    body,
                } => (
                    lifecycle("http_response"),
                    vec![
                        Value::Key(handle.clone()),
                        Value::Integer(*status),
                        Value::List(Vec::new()),
                        Value::String(body.clone()),
                    ],
                    // Second Life raises it in every script of the prim.
                    core::iter::once(id)
                        .chain(
                            host.prim_scripts(&host.whereabouts(id).prim)
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|other| {
                                    *other != id && flavour == EngineFlavour::SecondLife
                                }),
                        )
                        .collect(),
                ),
                Raised::Dataserver { id: handle, data } => (
                    lifecycle("dataserver"),
                    vec![Value::Key(handle.clone()), Value::String(data.clone())],
                    vec![id],
                ),
                Raised::ObjectMessage { to, from, message } => (
                    lifecycle("dataserver"),
                    vec![Value::Key(from.clone()), Value::String(message.clone())],
                    to.clone(),
                ),
                Raised::ExperienceGranted(agent) => (
                    lifecycle("experience_permissions"),
                    vec![Value::Key(agent.clone())],
                    vec![id],
                ),
                Raised::ExperienceDenied { agent, reason } => (
                    lifecycle("experience_permissions_denied"),
                    vec![Value::Key(agent.clone()), Value::Integer(reason.code())],
                    vec![id],
                ),
                Raised::Transaction(result) => (
                    lifecycle("transaction_result"),
                    vec![
                        Value::Key(result.id.clone()),
                        Value::Integer(i32::from(result.success)),
                        Value::String(result.data.clone()),
                    ],
                    vec![id],
                ),
            };
            // Queued now everywhere; Second Life runs a link message at the
            // next tick and a setter's `changed` two on, OpenSim both at
            // once.
            let arrival = match (raised, flavour) {
                (Raised::Linked(_), EngineFlavour::SecondLife) => {
                    arrival.ready_after(SECOND_LIFE_LINK_TICKS)
                }
                (Raised::Changed { .. }, EngineFlavour::SecondLife) => {
                    arrival.ready_after(SECOND_LIFE_LATE_TICKS)
                }
                _ => arrival,
            };
            for to in reached {
                if to == id {
                    if own {
                        let _posted = instance.post(arrival, event, args.clone());
                    }
                } else if let Some(peer) = self.instances.get_mut(&to) {
                    let _posted = peer.post(arrival, event, args.clone());
                }
            }
        }
    }

    /// [`SECOND_LIFE_BOX_REFRESH`] in whole ticks.
    fn box_refresh_ticks(&self) -> u64 {
        let step = self.config.step.as_nanos().max(1);
        let ticks = SECOND_LIFE_BOX_REFRESH
            .as_nanos()
            .saturating_add(step / 2)
            .checked_div(step)
            .unwrap_or(0);
        u64::try_from(ticks).unwrap_or(u64::MAX)
    }

    /// Bring each bounding box whose refresh falls in tick `now` up to its
    /// prim's scale ([`BoxClock`]), and forget the clocks of the boxes that
    /// caught up long enough ago to hold nothing back.
    fn refresh_boxes(&mut self, now: Tick) {
        if self.box_clocks.is_empty() {
            return;
        }
        let period = self.box_refresh_ticks();
        let stale = &mut self.stale_boxes;
        self.box_clocks.retain(|prim, clock| {
            if stale.contains_key(prim) {
                if clock.due <= now {
                    let _caught_up = stale.remove(prim);
                    clock.refreshed = Some(now);
                }
                return true;
            }
            clock
                .refreshed
                .is_some_and(|refreshed| refreshed.after(period) > now)
        });
    }

    /// Hand the scripts the lines they heard since OpenSim's last async
    /// command pass, when one falls in tick `now`: the first tick at or after
    /// each [`OPENSIM_ASYNC_PASS`] of region time.
    fn pass_heard(&mut self, now: Tick) {
        if self.overheard.is_empty() {
            return;
        }
        let pass = OPENSIM_ASYNC_PASS.as_micros();
        let at = |tick: u64| {
            u128::from(tick)
                .saturating_mul(self.config.step.as_micros())
                .checked_div(pass)
                .unwrap_or(0)
        };
        if now.0 > 0 && at(now.0) == at(now.0.saturating_sub(1)) {
            return;
        }
        for (id, heard) in core::mem::take(&mut self.overheard) {
            let _posted = self.hear(id, &heard);
        }
    }

    /// Advance one tick and run the region's scripts in it.
    pub fn tick(&mut self, host: &mut dyn Host) -> TickReport {
        self.now = self.now.after(1);
        let now = self.now;
        self.refresh_boxes(now);
        let order: Vec<CallerId> = match self.last_served {
            Some(last) => self
                .instances
                .range((Bound::Excluded(last), Bound::Unbounded))
                .chain(self.instances.range(..=last))
                .map(|(id, _)| *id)
                .collect(),
            None => self.instances.keys().copied().collect(),
        };
        let mut report = TickReport {
            tick: now,
            slices: Vec::new(),
            used: 0,
            runnable: 0,
            served: 0,
        };
        // Timers first, so a timer due this tick runs in it; but XEngine's
        // come from OpenSim's async command pass, after what the scripts do
        // in the same moment — a call its forced delay held back runs before
        // a timer due as the delay ends (`prim/changed`).
        let flavour = self.config.flavour;
        let timers_last = flavour == EngineFlavour::XEngine;
        for instance in self.instances.values_mut() {
            instance.count_tick();
        }
        if !timers_last {
            self.post_due_timers(now);
            self.pass_heard(now);
        }
        self.post_due_sweeps(now, host);
        let mut region_left = self.config.region_budget;
        // What each script served this tick has left of its share.
        let mut left: BTreeMap<CallerId, u32> = BTreeMap::new();
        for id in &order {
            let Some(mut instance) = self.instances.remove(id) else {
                continue;
            };
            if !instance.is_runnable(now) {
                let _none = self.instances.insert(*id, instance);
                continue;
            }
            report.runnable = report.runnable.saturating_add(1);
            if region_left == 0 {
                let _none = self.instances.insert(*id, instance);
                continue;
            }
            report.served = report.served.saturating_add(1);
            self.last_served = Some(*id);
            let share = self.repaid_share(&mut instance, &mut region_left);
            let rest = self.run_share(
                *id,
                &mut instance,
                (share, &mut region_left),
                &mut report,
                host,
            );
            let _was = left.insert(*id, rest);
            let _none = self.instances.insert(*id, instance);
        }
        // OpenSim runs an event as soon as it is posted, on an engine
        // thread: a script that an event reached after its turn in the tick,
        // or one not yet served, runs it now on what is left of its share —
        // a link message's answer takes no tick there (`events/latency`).
        // Second Life serves each script once a frame.
        while flavour.is_opensim() && region_left > 0 {
            let mut ran = false;
            for id in &order {
                let Some(mut instance) = self.instances.remove(id) else {
                    continue;
                };
                if !instance.is_runnable(now) || left.get(id) == Some(&0) {
                    let _none = self.instances.insert(*id, instance);
                    continue;
                }
                let share = match left.get(id) {
                    Some(rest) => *rest,
                    None => self.repaid_share(&mut instance, &mut region_left),
                };
                let used_before = report.used;
                let rest = self.run_share(
                    *id,
                    &mut instance,
                    (share, &mut region_left),
                    &mut report,
                    host,
                );
                ran |= report.used > used_before;
                let _was = left.insert(*id, rest);
                let _none = self.instances.insert(*id, instance);
            }
            if !ran {
                break;
            }
        }
        if timers_last {
            self.post_due_timers(now);
            self.pass_heard(now);
        }
        self.settle();
        report
    }

    /// The instance's share of this tick: the script budget less what it
    /// overdrew last time, which is still running and comes out of the
    /// region's budget too.
    fn repaid_share(&self, instance: &mut Instance, region_left: &mut u64) -> u32 {
        let share = instance.repay(self.config.script_budget);
        *region_left =
            region_left.saturating_sub(u64::from(self.config.script_budget.saturating_sub(share)));
        share
    }

    /// Run the instance `id` for up to `share` instructions of what is left
    /// of the region's budget, slice after slice while it has something to
    /// run, posting what each slice raised; answers what is left of the
    /// share.
    fn run_share(
        &mut self,
        id: CallerId,
        instance: &mut Instance,
        (share, region_left): (u32, &mut u64),
        report: &mut TickReport,
        host: &mut dyn Host,
    ) -> u32 {
        let now = self.now;
        let mut script_left = share;
        while script_left > 0 && *region_left > 0 && instance.is_runnable(now) {
            let slice = instance.run_slice_among(
                id,
                now,
                self.config.step,
                u32::try_from(*region_left).map_or(script_left, |region| script_left.min(region)),
                host,
                Peers::of(
                    &self.instances,
                    self.config.flavour,
                    (
                        self.ossl.as_deref().unwrap_or_else(|| stock_ossl()),
                        self.config.script_delay_factor,
                    ),
                    &self.stale_boxes,
                ),
            );
            self.report(id, &slice, host);
            self.raise(id, instance, &slice, host);
            instance.charge(slice.used);
            instance.overdraw(slice.used.saturating_sub(script_left));
            script_left = script_left.saturating_sub(slice.used);
            *region_left = region_left.saturating_sub(u64::from(slice.used));
            report.used = report.used.saturating_add(u64::from(slice.used));
            // Every body runs at least one instruction, so a slice that
            // charged nothing had nothing to run.
            let stop = slice.used == 0 || matches!(slice.outcome, Outcome::Idle);
            report.slices.push((id, slice));
            if stop {
                break;
            }
        }
        script_left
    }

    /// Post a `timer` event to every script whose timer is due at `now`.
    fn post_due_timers(&mut self, now: Tick) {
        let arrival = self.arrival();
        let flavour = self.config.flavour;
        let step = self.config.step;
        for instance in self.instances.values_mut() {
            if instance.timer_due(now, step, flavour) {
                let _posted = instance.post(arrival, lifecycle("timer"), Vec::new());
            }
        }
    }

    /// Sweep for every script whose sensor is due at `now`, and post what each
    /// sweep found: `sensor` with the detected block, `no_sensor`, or — on
    /// Second Life, for a range of 0 or less — nothing.
    fn post_due_sweeps(&mut self, now: Tick, host: &dyn Host) {
        let step = self.config.step;
        let flavour = self.config.flavour;
        let due: Vec<(CallerId, Vec<crate::vm::sensing::Query>)> = self
            .instances
            .iter_mut()
            .map(|(id, instance)| (*id, instance.sweeps_due(now, step)))
            .filter(|(_, queries)| !queries.is_empty())
            .collect();
        for (id, queries) in due {
            let sweeper = sweeper_of(id, host);
            let candidates = host.sense(id);
            for query in queries {
                let Some(found) = crate::vm::sensing::sweep(&query, &sweeper, &candidates, flavour)
                else {
                    continue;
                };
                // Second Life runs the answer a frame after it sweeps, two
                // after the call (`events/latency`).
                let arrival = if flavour == EngineFlavour::SecondLife {
                    self.arrival()
                        .ready_after(SECOND_LIFE_LATE_TICKS.saturating_sub(1))
                } else {
                    self.arrival()
                };
                let Some(instance) = self.instances.get_mut(&id) else {
                    continue;
                };
                // A sweep's events take what the table says they take.
                let _posted = if found.is_empty() {
                    instance.post(arrival, lifecycle("no_sensor"), Vec::new())
                } else {
                    instance.post_detected(arrival, lifecycle("sensor"), found)
                };
            }
        }
    }

    /// Fold the tick into every script's load and the active-script count.
    fn settle(&mut self) {
        let weight = (self.config.step.as_secs_f64() / LOAD_WINDOW_SECONDS).min(1.0);
        let flavour = self.config.flavour;
        let mut running = 0_u32;
        for instance in self.instances.values_mut() {
            instance.settle(weight);
            if instance.reads_running(flavour) {
                running = running.saturating_add(1);
            }
        }
        self.active += (f64::from(running) - self.active) * weight;
    }
}

/// Do `control` to `instance`, another script's `llSetScriptState` or
/// `llResetOtherScript`, as `flavour`'s engine does it
/// (`inventory/listing`): a run flag is set on every grid; a reset of a
/// stopped script does nothing on Second Life, where started again it carries
/// on without `state_entry`, and resets it on OpenSim, where started again it
/// runs `state_entry`; a reset of a running script drops what it had queued
/// on Second Life and XEngine, and on YEngine comes after it.
fn apply_control(instance: &mut Instance, control: Control, flavour: EngineFlavour) {
    match control {
        Control::Run(running) => instance.set_running_as(running, flavour),
        Control::Reset => match flavour {
            EngineFlavour::SecondLife => {
                if instance.is_running() || instance.is_held() {
                    instance.reset();
                }
            }
            EngineFlavour::YEngine => {
                if instance.is_running() {
                    instance.reset_when_idle();
                } else {
                    instance.reset();
                }
            }
            EngineFlavour::XEngine => instance.reset(),
        },
    }
}

/// Where the script `id`'s prim is, as a sweep measures from it.
fn sweeper_of(id: CallerId, host: &dyn Host) -> crate::vm::sensing::Sweeper {
    let at = host.whereabouts(id);
    let rotation = host
        .prim(id, &at.prim)
        .map_or(crate::value::ZERO_ROTATION, |prim| prim.rotation);
    let group = host.object_details(id, &at.object).map_or_else(
        || crate::value::NULL_KEY.to_owned(),
        |details| details.group,
    );
    crate::vm::sensing::Sweeper {
        position: host.placement(id).position,
        rotation,
        root: at.object,
        group,
    }
}

/// One of the events the engine raises itself (a timer, a change, a rez, a
/// change of the linkset's data store, a line heard, a link message, an
/// answer, a payment, a transfer's result), from the library table.
fn lifecycle(name: &'static str) -> &'static Event {
    /// A stand-in that can never match a handler, should the table ever lose
    /// the event — the build's own table test catches that first.
    static MISSING: Event = Event {
        name: "",
        args: &[],
        tooltip: "",
    };
    event(name).unwrap_or(&MISSING)
}
