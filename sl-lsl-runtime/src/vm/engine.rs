//! The scheduler: every script instance of a region, run a bounded number of
//! instructions per tick, round-robin.

use std::collections::BTreeMap;
use std::sync::Arc;

use core::ops::Bound;
use core::time::Duration;

use crate::bytecode::Program;
use crate::library::{Event, event};
use crate::value::Value;
use crate::vm::context::Tick;
use crate::vm::detected::Detected;
use crate::vm::host::{CallerId, Host};
use crate::vm::instance::{Arrival, Coalescing, Instance, Outcome, PostError, Posted, Slice};

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
/// On aditi (2026-09-28) two busy scripts in one prim each kept the rate one
/// runs at alone, so a single script is held by its own share, not by the
/// region's; how many shares a region has before its scripts slow down was
/// not measured, and sixteen is a decision, not a copy (roadmap
/// `server-lsl-call-cost-sizes` is to measure it).
pub const REGION_SCRIPT_SHARES: u64 = 16;

/// A region's scheduling parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// The length of one tick.
    pub step: Duration,
    /// Instructions one script may run per tick.
    pub script_budget: u32,
    /// Instructions the region's scripts may run per tick together.
    pub region_budget: u32,
    /// Which queued touch, collision or `changed` a new one merges into.
    pub coalescing: Coalescing,
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
            u32::try_from(per_script.saturating_mul(u128::from(REGION_SCRIPT_SHARES)))
                .unwrap_or(u32::MAX);
        Self {
            step,
            script_budget,
            region_budget,
            coalescing: Coalescing::Reference,
        }
    }
}

/// What one tick did.
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// lasts.
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
        }
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
        self.instances.insert(id, Instance::new(program))
    }

    /// Remove an instance, returning it.
    pub fn remove(&mut self, id: CallerId) -> Option<Instance> {
        self.instances.remove(&id)
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

    /// An event posted now: in the current tick, merging by the region's
    /// policy.
    const fn arrival(&self) -> Arrival {
        Arrival {
            tick: self.now,
            coalescing: self.config.coalescing,
        }
    }

    /// Advance one tick and run the region's scripts in it.
    pub fn tick(&mut self, host: &mut dyn Host) -> TickReport {
        self.now = self.now.after(1);
        let now = self.now;
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
        // Timers first, so a timer due this tick runs in it.
        let arrival = self.arrival();
        for instance in self.instances.values_mut() {
            if instance.timer_due(now) {
                let _posted = instance.post(arrival, lifecycle("timer"), Vec::new());
            }
        }
        let mut region_left = self.config.region_budget;
        for id in order {
            let Some(instance) = self.instances.get_mut(&id) else {
                continue;
            };
            if !instance.is_runnable(now) {
                continue;
            }
            report.runnable = report.runnable.saturating_add(1);
            if region_left == 0 {
                continue;
            }
            report.served = report.served.saturating_add(1);
            self.last_served = Some(id);
            let mut script_left = self.config.script_budget;
            while script_left > 0 && region_left > 0 && instance.is_runnable(now) {
                let slice = instance.run_slice(
                    id,
                    now,
                    self.config.step,
                    script_left.min(region_left),
                    host,
                );
                script_left = script_left.saturating_sub(slice.used);
                region_left = region_left.saturating_sub(slice.used);
                report.used = report.used.saturating_add(u64::from(slice.used));
                // Every body runs at least one instruction, so a slice that
                // charged nothing had nothing to run.
                let stop = slice.used == 0 || matches!(slice.outcome, Outcome::Idle);
                report.slices.push((id, slice));
                if stop {
                    break;
                }
            }
        }
        report
    }
}

/// One of the events the engine raises itself, from the library table.
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
