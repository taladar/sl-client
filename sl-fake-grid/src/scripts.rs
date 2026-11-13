//! Scripts running in a region: the LSL engine (`sl_lsl_runtime::vm`) joined
//! to the region's world.
//!
//! The runtime knows nothing of prims — it talks to a
//! [`Host`] — and the rest of the grid knows nothing
//! of scripts. This module is the join: a [`RegionScripts`] beside each
//! region's world holding the engine and one instance per script, and a
//! `RegionHost` that answers every world-touching library call from the
//! region.
//!
//! # Life cycle: the world says which scripts exist
//!
//! An instance exists for every script item (an `LSLText` asset, not marked
//! Luau) in a prim's contents whose source compiles. Nothing creates one by
//! hand: each tick, before the scripts run, the region looks at the prims
//! whose contents were written since the last tick (the store's change ticks,
//! [`ObjectStore::take_changed_contents`]) and brings the instances in line —
//! a new item gets one, a removed item loses its own, an item whose asset
//! changed (a save) gets a fresh one of the new program, and an item that no
//! longer compiles has none. A prim that left the region takes its scripts
//! with it. So a fixture's script, a `RezScript`, a drop, a save and a delete
//! all reach the engine the same way, and no path that writes a prim's
//! contents can forget to tell it.
//!
//! A new instance runs if its item is in the contents' `running` set, which
//! is what the viewer's Running checkbox, a `RezScript` and an upload write.
//! After that the instance's own run flag is the truth: `SetScriptRunning`
//! sets both, and `GetScriptRunning` reads the instance (as the imitated
//! engine reads it: a script Second Life holds after a fault reads stopped).
//!
//! # A tick
//!
//! `run_tick` is the heartbeat's script slice. Under the region lock it
//! reconciles the instances, runs one [`Engine::tick`] with a `RegionHost`
//! over the world, and applies what the scripts did to the world — a line
//! said, an object that called `llDie`. What has to reach a viewer goes out
//! after the lock is released: the changes on the region's change stream (as
//! the region's own, [`REGION_SOURCE`]), and dialogs, permission questions,
//! offered pages, map destinations, instant messages and directed lines to
//! the sessions they are for. That is the crate's lock order — a session's
//! lock is never taken under the region's.
//!
//! # Taken, rezzed and restarted
//!
//! A take files each prim's contents and its scripts' saved state with the
//! taken linkset (`RegionScripts::save_prim`; a take of a copy saves them
//! and leaves them running). A rez from inventory puts the contents back under
//! the new prims' keys and the scripts arrive with them
//! (`RegionScripts::arrive`): the next reconcile brings each back from what
//! it saved, the time away counted as its engine counts it, with
//! `on_rez(0)` — or starts it afresh with `on_rez` where it saved nothing or
//! no longer fits its program, which is logged. A region restart
//! ([`FakeGrid::restart_region`](crate::FakeGrid::restart_region)) saves and
//! brings back every script in place, with `changed(CHANGED_REGION_START)`.
//!
//! # Experiences
//!
//! A script of an experience stands where its prim is as the region's
//! [`ExperienceLand`](crate::experiences::ExperienceLand) judges it. A
//! question put under the experience is remembered until it is answered: a
//! *Yes* records the experience allowed for the agent, which is what lets the
//! experience's every later script be granted unasked; a *No* records
//! nothing; and a question unanswered for five minutes of region ticks is
//! answered `XP_ERROR_REQUEST_PERM_TIMEOUT`.
//!
//! # Sitting and wearing
//!
//! Who sits on which prim and who wears what are the region's own state
//! (`sl_fake_grid::seating`); the host reads them for `llAvatarOnSitTarget`,
//! `llGetNumberOfPrims`, `llGetLinkKey`, `llGetAttached` and the placement
//! functions, and an `llUnSit`, an `llSitOnLink`, an `llAttachToAvatar` and an
//! `llDetachFromAvatar` are applied after the tick, the sitter's or the
//! wearer's own session told through the region's changes.
//!
//! # Touches
//!
//! A held mouse button reaches the scripts through [`crate::touches`]: each
//! tick, before the scripts run, a touch held open on Second Life is due its
//! `touch`, and after it every scripted prim's touch flags are brought in line
//! with what its scripts handle; `llPassTouches` is the prim's setting there.
//!
//! # Money
//!
//! `llGiveMoney` and `llTransferLindenDollars` pay out of the grid's ledger
//! ([`crate::money`]), the owner and the payee told by balance replies of type
//! `5009` delivered after the tick; a payment to a prim raises its scripts'
//! `money` there too. Where the grid keeps no balances (OpenSim's stock money
//! module) nothing moves.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_lsl_runtime::Dialect;
use sl_lsl_runtime::bytecode::Program;
use sl_lsl_runtime::library::BuiltinId;
use sl_lsl_runtime::vm::land::{Environment, LandChange, Parcel, ParcelMedia, Region, SimData};
use sl_lsl_runtime::vm::{
    Admission, Allowed, CallerId, Dialog, Engine, EngineConfig, EngineFlavour, ExperienceDetails,
    ExperienceError, ExperienceQuestion, Fault, Heard, Host, InstantMessage, InvalidLinkTarget,
    KeyValueStore, LinkedAvatar, LinkedPrim, Links, LinksetData, LinksetId, LoadUrl,
    MapDestination, MoneyRefusal, Overheard, PayPrice, Payment, Question, SavedScript,
    ScriptExperience, ScriptUsage, Simulator, SitRefusal, Speech, Standing, Tick, TickReport,
    Whereabouts,
};
use sl_proto::{
    AssetKey, AssetSource as _, AssetType, ChatSource, ChatType, ExperiencePermission, ImDialog,
    InventoryType, LoadUrlRequest, Object, RegionLocalObjectId, ScriptDialog, ScriptLanguage,
    ScriptPermissionRequest, ScriptTeleportRequest, ServerEvent, SimSession, TaskInventoryItem,
    prim_flags,
};
use sl_types::chat::{ChatChannel, ChatVolume};
use sl_types::key::{AgentKey, ExperienceKey, InventoryKey, ObjectKey, OwnerKey, TextureKey};
use sl_types::lsl::{Rotation, ScriptPermissions, Vector};
use sl_types::map::{Direction, RegionCoordinates};

use crate::chat::{DEBUG_CHANNEL, FULLY_AUDIBLE, Line};
use crate::runtime::GridCore;
use crate::scene_store::ObjectStore;
use crate::world::{AvatarIdentity, RegionChange, RegionUpdate, SceneFixtures};

/// The [`RegionUpdate::source`] of a change the region made itself rather
/// than a session — a script's line, an object a script removed. Session
/// numbers start at one, so no session's watcher takes it for its own.
pub const REGION_SOURCE: u64 = 0;

/// A run-time error a script in one of the grid's regions hit, as the grid
/// records it for whoever subscribes ([`FakeGrid::script_faults`]): where it
/// was, the error, and the line it happened on — which no grid tells a
/// resident, and which a test harness reports a broken script by.
///
/// [`FakeGrid::script_faults`]: crate::FakeGrid::script_faults
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFault {
    /// The region's name.
    pub region: String,
    /// The name of the prim the script is in.
    pub object: String,
    /// The key of the prim the script is in.
    pub prim: Option<ObjectKey>,
    /// The script's inventory name.
    pub script: String,
    /// The error, in the words the runtime gives it (`Math Error`).
    pub error: String,
    /// The 1-based source line and column of the instruction that failed,
    /// when the script's source map has one.
    pub position: Option<(u32, u32)>,
    /// The event whose handler was running, or [`None`] for the global
    /// initialisers.
    pub event: Option<&'static str>,
}

impl std::fmt::Display for ScriptFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "script {:?} in {:?} stopped", self.script, self.object)?;
        if let Some((line, column)) = self.position {
            write!(f, " at line {line}, column {column}")?;
        }
        if let Some(event) = self.event {
            write!(f, " in {event}")?;
        }
        write!(f, " with {}", self.error)
    }
}

/// A call a script made to a library function this grid only stubs, which
/// answered its default and did nothing else — told to whoever subscribes
/// ([`FakeGrid::stubbed_calls`]), so nobody mistakes the default for the
/// function's real answer.
///
/// [`FakeGrid::stubbed_calls`]: crate::FakeGrid::stubbed_calls
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubbedCall {
    /// The region's name.
    pub region: String,
    /// The name of the prim the script is in.
    pub object: String,
    /// The script's inventory name.
    pub script: String,
    /// The function (`llSetVehicleType`).
    pub function: &'static str,
}

mod avatars;
pub(crate) mod http;
mod land;
mod sensing;
mod task_inventory;

/// The address [`Host::agent_ip`] gives for an agent in the region: every
/// circuit of the fake grid is a loopback one.
const LOOPBACK_ADDRESS: &str = "127.0.0.1";

/// The physics engine an OpenSim-flavoured region reports
/// (`osGetPhysicsEngineType`): the local grid's.
const PHYSICS_ENGINE_TYPE: &str = "ubODE";

/// The physics engine's name and version an OpenSim-flavoured region reports
/// (`osGetPhysicsEngineName`): the local grid's.
const PHYSICS_ENGINE_NAME: &str = "ubODE 1.0-0.13.2";

/// The length of a day of a region's environment, in seconds, as both grids
/// default it where the region keeps no environment of its own.
const DEFAULT_DAY_LENGTH: i32 = 14_400;

/// The offset of a region's day cycle, in seconds, as OpenSim's default
/// environment sets it (`ViewerEnvironment`'s `DayOffset`).
const DEFAULT_DAY_OFFSET: i32 = 57_600;

/// The edge of a region, in metres.
const REGION_EDGE_M: u32 = 256;

/// How many ticks a program an upload compiled waits for the saved script's
/// instance to take it. The upload's answer and the item's new asset are two
/// steps of the session, a tick may fall between them, and a save whose item
/// never takes the program (an upload to an item the prim does not hold)
/// must not keep it for good.
const PROGRAM_HANDOFF_TICKS: u32 = 100;

/// How long an experience's permission question waits for its answer before
/// the script is told `XP_ERROR_REQUEST_PERM_TIMEOUT` (the runtime's
/// [`Host::ask_experience`] contract: five minutes).
const EXPERIENCE_QUESTION_TIMEOUT: Duration = Duration::from_secs(300);

/// One script instance's place in the region: the prim and the item it is,
/// the asset its program was compiled from, and the item's name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    /// The prim the script is in.
    prim: ObjectKey,
    /// The script's inventory item.
    item: InventoryKey,
    /// The asset its program was compiled from: a save writes a new one.
    asset: AssetKey,
    /// The item's name, which `llGetScriptState` and an error line name.
    name: String,
}

/// The scripts of one region: the engine running them, and which prim and
/// item each instance is.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `SceneFixtures::script_instances`, \
              `sl_fake_grid::RegionScripts` -- where a bare `Scripts` would not say whose"
)]
#[derive(Debug, Clone)]
pub struct RegionScripts {
    /// The engine, which holds the instances.
    engine: Engine,
    /// Each instance's prim and item, by the id the engine knows it by.
    slots: BTreeMap<CallerId, Slot>,
    /// The instance of each `(prim, item)`.
    by_item: BTreeMap<(ObjectKey, InventoryKey), CallerId>,
    /// The last id handed to an instance; ids are never reused, so an
    /// instance started later is served later in a tick.
    last_id: u64,
    /// Programs compiled for the region's scripts, by the asset the source
    /// was stored as, with how many ticks each has waited unused: an
    /// upload's, until the saved script's instance takes it (a save is
    /// compiled once), and every running script's, which copies of one
    /// script share.
    programs: BTreeMap<AssetKey, (Arc<Program>, u32)>,
    /// Each linkset's data store, by the key of its root
    /// (`llLinksetDataWrite`).
    linkset_data: BTreeMap<ObjectKey, LinksetData>,
    /// The pay layout each prim's scripts set (`llSetPayPrice`).
    pay_prices: BTreeMap<ObjectKey, PayPrice>,
    /// The experience ids the region has looked up
    /// (`llGetExperienceDetails` answers an id it has not seen with nothing,
    /// once).
    looked_up: BTreeSet<String>,
    /// The state of the seeded draw [`Host::random_bits`] answers from.
    draws: u64,
    /// How many scripts were runnable, and how many of those ran, in the
    /// last tick: the scripts-run percentage.
    last_load: (u32, u32),
    /// The scripts of an object just rezzed, by prim and item, waiting for
    /// the next reconcile to start them: what they saved at their take, if
    /// anything, and what the rez says.
    arriving: BTreeMap<(ObjectKey, InventoryKey), Arriving>,
    /// The experience questions put and not yet answered, by the script and
    /// the avatar asked: the experience, and the tick the question went out.
    experience_questions: BTreeMap<(CallerId, AgentKey), (ExperienceKey, Tick)>,
    /// The scripts that have teleported an avatar since it last granted them
    /// a permission, by the script and the avatar: Second Life alerts the
    /// avatar of the first such teleport only ([`RegionHost::teleport_alert`]).
    teleport_alerted: BTreeSet<(CallerId, AgentKey)>,
    /// The object a script rezzed, by its root, with the root of the object
    /// whose script rezzed it (`OBJECT_REZZER_KEY`, `llDerezObject`).
    pub(crate) rezzers: BTreeMap<ObjectKey, ObjectKey>,
    /// Rezzers to tell of the object they rezzed (`object_rez`), with the
    /// ticks each has waited: told after the tick in which the object's
    /// scripts ran their `on_rez` — on Second Life after the tick that rezzed
    /// it — before what they said is heard.
    pub(crate) rezzes_waiting: Vec<(CallerId, ObjectKey, u8)>,
    /// Scripts a load put in a prim (`llRemoteLoadScriptPin`), to start, with
    /// whether they run and the ticks each has waited: started once the tick
    /// after the one they landed in is over, so the prim's scripts hear of
    /// the new item first and read it stopped.
    pub(crate) starting: Vec<(ObjectKey, InventoryKey, bool, u8)>,
    /// Each prim's remote script access pin, by key
    /// (`llSetRemoteScriptAccessPin`).
    pub(crate) pins: BTreeMap<ObjectKey, i32>,
    /// The notecards the region has read, by asset: what Second Life's
    /// `llGetNotecardLineSync` reads.
    pub(crate) notecards_read: BTreeSet<AssetKey>,
    /// The region's calendar time at its tick 0, in microseconds since the
    /// epoch, read off the region's wall clock at its first tick: what a
    /// script's dates count on from by the ticks.
    epoch_micros: Option<i64>,
    /// The scripts' HTTP in the region ([`http::RegionHttp`]).
    http: http::RegionHttp,
    /// What scripts hold of the region's avatars ([`avatars::RegionAvatars`]).
    avatars: avatars::RegionAvatars,
    /// While the engine is lent to a tick, the calls into it that a change
    /// made under the tick asked for, in order, to be made once it is back;
    /// [`None`] while it is in place.
    deferred: Option<Vec<EngineCall>>,
}

/// A call into the region's engine that a change made while the engine was
/// lent to a tick asked for: made once the tick is over, in order with the
/// tick's other effects.
#[derive(Debug, Clone)]
enum EngineCall {
    /// `changed(bits)` in the script.
    Changed {
        /// The script.
        id: CallerId,
        /// The bits.
        bits: i32,
    },
    /// `bits` of what `granter` granted the script lapse.
    Revoke {
        /// The script.
        id: CallerId,
        /// The granting agent's key.
        granter: String,
        /// The bits.
        bits: i32,
    },
}

/// A script arriving in the region with the object it is in.
#[derive(Debug, Clone)]
struct Arriving {
    /// What it saved when its object was taken, or [`None`] to start it
    /// afresh.
    saved: Option<SavedScript>,
    /// How long its object was away.
    away: Duration,
    /// How its object came: rezzed, its script loaded, or worn by an avatar
    /// that brought it.
    how: ArrivedBy,
}

/// How an arriving script's object came into the region.
#[derive(Debug, Clone)]
enum ArrivedBy {
    /// Rezzed out of an inventory, with this start parameter and start
    /// string: `on_rez`.
    Rez(i32, String),
    /// Its script loaded into its prim (`llRemoteLoadScriptPin`): started
    /// afresh with this start parameter and no `on_rez`.
    Load(i32),
    /// Worn by an avatar that walked or teleported in: `changed` with these
    /// bits ([`Engine::travel_saved`]), or nothing for 0 — a move put back.
    Travel(i32),
}

impl Default for RegionScripts {
    /// No scripts, Second Life's engine, the default heartbeat step.
    fn default() -> Self {
        Self::new(crate::heartbeat::DEFAULT_STEP, EngineFlavour::SecondLife, 1)
    }
}

impl RegionScripts {
    /// What `agent` plays in `posture`: a script's override of the
    /// posture's state (`llSetAnimationOverride`), or the built-in.
    #[must_use]
    pub(crate) fn posture_animation(
        &self,
        agent: AgentKey,
        posture: crate::movement::Posture,
    ) -> uuid::Uuid {
        self.avatars.posture_animation(agent, posture)
    }

    /// No scripts yet, an engine of `flavour` ticking every `step`, and the
    /// random draws seeded with `seed`.
    #[must_use]
    pub(crate) fn new(step: Duration, flavour: EngineFlavour, seed: u64) -> Self {
        Self {
            engine: Engine::new(EngineConfig {
                // OpenSim merges no event into another, on both engines
                // (`events/merging`); Second Life's rule is the reference.
                coalescing: if flavour.is_opensim() {
                    sl_lsl_runtime::vm::Coalescing::Never
                } else {
                    sl_lsl_runtime::vm::Coalescing::Reference
                },
                ..EngineConfig::for_step(step, flavour)
            }),
            slots: BTreeMap::new(),
            by_item: BTreeMap::new(),
            last_id: 0,
            programs: BTreeMap::new(),
            linkset_data: BTreeMap::new(),
            pay_prices: BTreeMap::new(),
            looked_up: BTreeSet::new(),
            // Zero is xorshift's fixed point.
            draws: seed.max(1),
            last_load: (0, 0),
            arriving: BTreeMap::new(),
            experience_questions: BTreeMap::new(),
            teleport_alerted: BTreeSet::new(),
            rezzers: BTreeMap::new(),
            rezzes_waiting: Vec::new(),
            starting: Vec::new(),
            pins: BTreeMap::new(),
            notecards_read: BTreeSet::new(),
            epoch_micros: None,
            http: http::RegionHttp::default(),
            avatars: avatars::RegionAvatars::default(),
            deferred: None,
        }
    }

    /// Raises `changed(bits)` in the script, or, while the engine is lent to
    /// a tick, notes it to be raised once the tick is over.
    fn raise_changed_or_defer(&mut self, id: CallerId, bits: i32) {
        if let Some(deferred) = &mut self.deferred {
            deferred.push(EngineCall::Changed { id, bits });
        } else if let Err(error) = self.engine.changed(id, bits) {
            tracing::warn!(?id, %error, "raising changed failed");
        }
    }

    /// Lets `bits` of what `granter` granted the script lapse, or, while the
    /// engine is lent to a tick, notes it to be done once the tick is over.
    fn revoke_or_defer(&mut self, id: CallerId, granter: &str, bits: i32) {
        if let Some(deferred) = &mut self.deferred {
            deferred.push(EngineCall::Revoke {
                id,
                granter: granter.to_owned(),
                bits,
            });
        } else {
            self.engine.revoke(id, granter, bits);
        }
    }

    /// Makes a call a change under a tick deferred, the engine back in
    /// place.
    fn make(&mut self, call: EngineCall) {
        match call {
            EngineCall::Changed { id, bits } => self.raise_changed_or_defer(id, bits),
            EngineCall::Revoke { id, granter, bits } => self.revoke_or_defer(id, &granter, bits),
        }
    }

    /// The region's calendar time at the start of tick `tick`, in
    /// microseconds since the UNIX epoch: what a script's clocks read then.
    /// Before the region's first tick fixes where its ticks started, that is
    /// `wall` now less the ticks run, as the first tick will fix it.
    pub(crate) fn calendar_micros(&self, tick: Tick, wall: &crate::time::WallClock) -> i64 {
        let step = self.engine.config().step;
        self.epoch_micros
            .unwrap_or_else(|| {
                i64::try_from(wall.unix_micros())
                    .unwrap_or(i64::MAX)
                    .saturating_sub(ticks_as_micros(self.engine.now(), step))
            })
            .saturating_add(ticks_as_micros(tick, step))
    }

    /// The engine running the region's scripts.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The engine, to post an event to.
    pub(crate) const fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// The instances of the scripts in `prim`, in the order they started.
    #[must_use]
    pub fn scripts_in(&self, prim: ObjectKey) -> Vec<CallerId> {
        self.slots
            .iter()
            .filter(|(_id, slot)| slot.prim == prim)
            .map(|(id, _slot)| *id)
            .collect()
    }

    /// The programs the scripts of `prim` run, by the asset each was
    /// compiled from, in the order the scripts started: what a save changes.
    #[must_use]
    pub fn assets_in(&self, prim: ObjectKey) -> Vec<AssetKey> {
        self.slots
            .values()
            .filter(|slot| slot.prim == prim)
            .map(|slot| slot.asset)
            .collect()
    }

    /// The prims the region runs a script in.
    #[must_use]
    pub fn prims(&self) -> BTreeSet<ObjectKey> {
        self.slots.values().map(|slot| slot.prim).collect()
    }

    /// Runs the region's OSSL, LightShare and MOD calls under `settings`
    /// ([`Engine::set_ossl`]).
    pub(crate) fn set_ossl(&mut self, settings: sl_lsl_runtime::vm::ossl::Settings) {
        self.engine.set_ossl(settings);
    }

    /// Which grid's engine the region imitates.
    #[must_use]
    pub const fn flavour(&self) -> EngineFlavour {
        self.engine.config().flavour
    }

    /// How many script instances the region has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the region has no script instance.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The last tick the region's scripts had their turn in: an event raised
    /// now has been run once this has moved on.
    #[must_use]
    pub(crate) const fn last_turn(&self) -> Tick {
        self.engine.now()
    }

    /// The instance of the script `item` in the prim `prim`, if it has one.
    #[must_use]
    pub fn instance_of(&self, prim: ObjectKey, item: InventoryKey) -> Option<CallerId> {
        self.by_item.get(&(prim, item)).copied()
    }

    /// Whether the script `item` in `prim` reads as running, as the imitated
    /// engine reads it; [`None`] when it has no instance.
    #[must_use]
    pub fn reads_running(&self, prim: ObjectKey, item: InventoryKey) -> Option<bool> {
        let id = self.instance_of(prim, item)?;
        self.engine
            .instance(id)
            .map(|instance| instance.reads_running(self.flavour()))
    }

    /// Hand the engine the program an upload compiled from the source stored
    /// as `asset`, so the instance of the saved script is not compiled again.
    pub(crate) fn offer_program(&mut self, asset: AssetKey, program: Program) {
        let _replaced = self.programs.insert(asset, (Arc::new(program), 0));
    }

    /// Set the run flag of the script `item` in `prim`; whether it has an
    /// instance.
    pub(crate) fn set_running(
        &mut self,
        prim: ObjectKey,
        item: InventoryKey,
        running: bool,
    ) -> bool {
        let Some(id) = self.instance_of(prim, item) else {
            return false;
        };
        let flavour = self.flavour();
        self.engine
            .instance_mut(id)
            .map(|instance| instance.set_running_as(running, flavour))
            .is_some()
    }

    /// Reset the script `item` in `prim` from outside (`ScriptReset`); whether
    /// it has an instance.
    pub(crate) fn reset(&mut self, prim: ObjectKey, item: InventoryKey) -> bool {
        let Some(id) = self.instance_of(prim, item) else {
            return false;
        };
        self.engine
            .instance_mut(id)
            .map(|instance| instance.reset())
            .is_some()
    }

    /// The scripts of `prim` saved, by item, for a take to file with the
    /// object: taken out of the region when `leaving`, left running when the
    /// take is of a copy.
    pub(crate) fn save_prim(
        &mut self,
        prim: ObjectKey,
        leaving: bool,
    ) -> BTreeMap<InventoryKey, SavedScript> {
        let held: Vec<(CallerId, InventoryKey)> = self
            .slots
            .iter()
            .filter(|(_id, slot)| slot.prim == prim)
            .map(|(id, slot)| (*id, slot.item))
            .collect();
        let mut saved = BTreeMap::new();
        for (id, item) in held {
            let state = if leaving {
                let state = self.engine.take(id);
                self.remove(id);
                state
            } else {
                self.engine.save(id)
            };
            if let Some(state) = state {
                let _previous = saved.insert(item, state);
            }
        }
        saved
    }

    /// The script `item` arrives in `prim` with a rez, `away` after its
    /// object was taken: the next reconcile brings it back from `saved`, or
    /// starts it afresh, gives it the start string and raises
    /// `on_rez(start_parameter)` in it.
    pub(crate) fn arrive(
        &mut self,
        prim: ObjectKey,
        item: InventoryKey,
        saved: Option<SavedScript>,
        away: Duration,
        (start_parameter, start_string): (i32, &str),
    ) {
        let _previous = self.arriving.insert(
            (prim, item),
            Arriving {
                saved,
                away,
                how: ArrivedBy::Rez(start_parameter, start_string.to_owned()),
            },
        );
    }

    /// The script `item` arrives in `prim`, a worn object its wearer brought
    /// into the region `away` after it left the last one: the next reconcile
    /// brings it back from `saved` and raises `changed(change)` in it as its
    /// engine does ([`Engine::travel_saved`]).
    pub(crate) fn travel(
        &mut self,
        prim: ObjectKey,
        item: InventoryKey,
        saved: Option<SavedScript>,
        away: Duration,
        change: i32,
    ) {
        let _previous = self.arriving.insert(
            (prim, item),
            Arriving {
                saved,
                away,
                how: ArrivedBy::Travel(change),
            },
        );
    }

    /// The script `item` was loaded into `prim` (`llRemoteLoadScriptPin`):
    /// the next reconcile starts it afresh with `start_parameter`, stopped,
    /// and once the tick after is over it runs if `running`.
    pub(crate) fn load_script(
        &mut self,
        prim: ObjectKey,
        item: InventoryKey,
        start_parameter: i32,
        running: bool,
    ) {
        let _previous = self.arriving.insert(
            (prim, item),
            Arriving {
                saved: None,
                away: Duration::ZERO,
                how: ArrivedBy::Load(start_parameter),
            },
        );
        self.starting
            .retain(|(at, script, _, _)| (*at, *script) != (prim, item));
        self.starting.push((prim, item, running, 0));
    }

    /// The region goes down and comes back `down` later: every script is
    /// saved and brought back as its engine does it — its globals, state,
    /// timer, listens and queue kept, and `changed(CHANGED_REGION_START)`
    /// raised ([`Engine::shut_down`], [`Engine::start_up`]).
    pub(crate) fn restart(&mut self, down: Duration) {
        let programs: BTreeMap<CallerId, Arc<Program>> = self
            .slots
            .keys()
            .filter_map(|id| {
                self.engine
                    .instance(*id)
                    .map(|instance| (*id, Arc::clone(instance.program())))
            })
            .collect();
        let saved = self.engine.shut_down();
        let scripts: Vec<(CallerId, Arc<Program>, SavedScript)> = saved
            .into_iter()
            .filter_map(|(id, state)| {
                programs
                    .get(&id)
                    .map(|program| (id, Arc::clone(program), state))
            })
            .collect();
        if let Err((id, error)) = self.engine.start_up(scripts, down) {
            // Every script starts afresh rather than the region coming back
            // with none; said, because it loses what the scripts held.
            tracing::error!(
                ?id,
                %error,
                "a script did not fit its saved state; the region's scripts start afresh"
            );
            for (id, program) in programs {
                let _replaced = self.engine.add(id, program);
            }
        }
    }

    /// The pay layout the scripts of `prim` set, if any did.
    #[must_use]
    pub fn pay_price(&self, prim: ObjectKey) -> Option<PayPrice> {
        self.pay_prices.get(&prim).copied()
    }

    /// The region's active-script count as `SimStats` states it, and the
    /// percentage of the runnable scripts the last tick ran.
    #[must_use]
    pub fn load(&self) -> (f32, f32) {
        let (runnable, served) = self.last_load;
        let percent = if runnable == 0 {
            100.0
        } else {
            f64::from(served) * 100.0 / f64::from(runnable)
        };
        (narrow(self.engine.active_scripts()), narrow(percent))
    }

    /// What the scripts of the linkset whose root is `root` cost the region,
    /// summed over its prims: their time and memory, as a top-scripts report
    /// states them. [`None`] for a linkset with no script.
    #[must_use]
    pub fn usage_of_linkset(
        &self,
        objects: &ObjectStore,
        root: RegionLocalObjectId,
    ) -> Option<crate::world::ObjectCost> {
        let mut found = false;
        let mut time = Duration::ZERO;
        let mut memory = 0_u64;
        for (id, slot) in &self.slots {
            let Some(prim) = objects.get_by_key(slot.prim) else {
                continue;
            };
            let prim_root = if prim.parent_id.0 == 0 {
                prim.local_id
            } else {
                prim.parent_id
            };
            if prim_root != root {
                continue;
            }
            let Some(ScriptUsage {
                memory: bytes,
                time: spent,
                ..
            }) = self.engine.usage(*id)
            else {
                continue;
            };
            found = true;
            time = time.saturating_add(spent);
            memory = memory.saturating_add(u64::from(bytes));
        }
        found.then(|| {
            crate::world::ObjectCost::scripted(time).with_resources(
                narrow(f64::from(u32::try_from(memory).unwrap_or(u32::MAX))),
                0,
            )
        })
    }

    /// Offer `line`, said in the region, to every script in earshot of it —
    /// all but those of the prim that said it — as a `listen` for each of
    /// its listens that admits it. Where a script is is where its prim's
    /// linkset root stands.
    pub(crate) fn hear(&mut self, line: &Line, objects: &ObjectStore) {
        let heard = heard_of(line);
        for id in self.in_earshot(line, objects) {
            self.engine.overhear(id, &heard);
        }
    }

    /// The scripts `line` reaches: those whose prims stand within its range
    /// of it, or anywhere for a region-say, but for the speaking prim's own.
    fn in_earshot(&self, line: &Line, objects: &ObjectStore) -> Vec<CallerId> {
        self.slots
            .iter()
            .filter(|(_id, slot)| {
                crate::world::listening_position(objects, slot.prim)
                    .is_some_and(|position| line.reaches_listen_at(slot.prim, &position))
            })
            .map(|(id, _slot)| *id)
            .collect()
    }

    /// Tells every script whose experience question has waited
    /// [`EXPERIENCE_QUESTION_TIMEOUT`] by `now` (ticks of `step`) that it
    /// timed out.
    fn time_out_experience_questions(&mut self, now: Tick, step: Duration) {
        let limit = u64::try_from(
            EXPERIENCE_QUESTION_TIMEOUT
                .as_micros()
                .div_ceil(step.as_micros().max(1)),
        )
        .unwrap_or(u64::MAX);
        let expired: Vec<(CallerId, AgentKey)> = self
            .experience_questions
            .iter()
            .filter(|(_asked, (_experience, at))| now.0.saturating_sub(at.0) >= limit)
            .map(|(asked, _question)| *asked)
            .collect();
        for (id, agent) in expired {
            let _asked = self.experience_questions.remove(&(id, agent));
            let agent = agent.uuid().to_string();
            if let Err(error) =
                self.engine
                    .answer_experience(id, &agent, Err(ExperienceError::RequestTimedOut))
            {
                tracing::warn!(?id, %error, "an experience question's timeout could not be posted");
            }
        }
    }

    /// Take the instance `id` out of the region, with the questions it put.
    fn remove(&mut self, id: CallerId) {
        self.experience_questions
            .retain(|(asked, _agent), _question| *asked != id);
        self.teleport_alerted
            .retain(|(teleporter, _agent)| *teleporter != id);
        let _instance = self.engine.remove(id);
        if let Some(slot) = self.slots.remove(&id) {
            let _gone = self.by_item.remove(&(slot.prim, slot.item));
        }
    }

    /// Start an instance of `program` for `slot`, running as `running` says,
    /// replacing the one it had.
    fn start(&mut self, slot: Slot, program: Arc<Program>, running: bool) {
        let id = match self.by_item.get(&(slot.prim, slot.item)) {
            Some(id) => *id,
            None => {
                self.last_id = self.last_id.saturating_add(1);
                CallerId(self.last_id)
            }
        };
        let arriving = self.arriving.remove(&(slot.prim, slot.item));
        let restored = arriving.as_ref().and_then(|arriving| {
            let saved = arriving.saved.as_ref()?;
            let restored = match &arriving.how {
                ArrivedBy::Rez(start_parameter, _start_string) => self
                    .engine
                    .rez_saved(
                        id,
                        Arc::clone(&program),
                        saved,
                        arriving.away,
                        *start_parameter,
                    )
                    .map(|_posted| ()),
                // A loaded script never saved anything; it starts afresh.
                ArrivedBy::Load(_start_parameter) => return None,
                ArrivedBy::Travel(change) => self
                    .engine
                    .travel_saved(id, Arc::clone(&program), saved, arriving.away, *change)
                    .map(|_posted| ()),
            };
            match restored {
                Ok(()) => Some(()),
                Err(error) => {
                    tracing::warn!(
                        prim = %slot.prim,
                        item = %slot.item,
                        %error,
                        "a rezzed script did not fit what it saved at its take; it starts afresh"
                    );
                    None
                }
            }
        });
        if restored.is_none() {
            let _replaced = self.engine.add(id, program);
            if let Some(instance) = self.engine.instance_mut(id) {
                instance.set_running(running);
                if let Some(Arriving {
                    how: ArrivedBy::Load(start_parameter),
                    ..
                }) = &arriving
                {
                    instance.set_start_parameter(*start_parameter);
                }
            }
            if let Some(Arriving {
                how: ArrivedBy::Rez(start_parameter, _start_string),
                ..
            }) = &arriving
                && let Err(error) = self.engine.rez(id, *start_parameter)
            {
                tracing::warn!(?id, %error, "on_rez could not be raised");
            }
        }
        if let Some(Arriving {
            how: ArrivedBy::Rez(_start_parameter, start_string),
            ..
        }) = arriving
            && let Some(instance) = self.engine.instance_mut(id)
        {
            instance.set_start_string(start_string);
        }
        let _was = self.by_item.insert((slot.prim, slot.item), id);
        let _old = self.slots.insert(id, slot);
    }

    /// A new id out of the region's seeded draw: the item a script's wear
    /// files its object as, minted where no session's minter reaches.
    pub(crate) fn mint_id(&mut self) -> uuid::Uuid {
        let high = (u64::from(self.draw()) << 32) | u64::from(self.draw());
        let low = (u64::from(self.draw()) << 32) | u64::from(self.draw());
        uuid::Uuid::from_u128((u128::from(high) << 64) | u128::from(low))
    }

    /// The next 32 bits of the region's seeded draw (xorshift64).
    fn draw(&mut self) -> u32 {
        let mut x = self.draws;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.draws = x;
        u32::try_from(x >> 32).unwrap_or(u32::MAX)
    }
}

/// `value` as the `f32` a statistic or a report carries.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a statistic is an f32 on the wire, std has no lossless conversion to one, and the \
              precision lost is far below what it reports"
)]
const fn narrow(value: f64) -> f32 {
    value as f32
}

/// The scripts among `item`s a region runs: LSL source items with an asset,
/// not marked Luau (Second Life compiles a Luau script, and this grid runs no
/// Luau).
fn runs(item: &TaskInventoryItem) -> Option<AssetKey> {
    if item.inv_type != InventoryType::Script || item.asset_type != AssetType::ScriptText {
        return None;
    }
    if ScriptLanguage::from_item_flags(item.flags) == Some(ScriptLanguage::Luau) {
        return None;
    }
    item.asset_id
}

/// Bring the region's script instances in line with its world: a script
/// item in a prim whose contents were written since the last call gets an
/// instance of what its asset compiles to, a removed or recompiled one loses
/// the one it had, and a prim that left the region takes its scripts.
pub(crate) fn reconcile(world: &mut SceneFixtures, assets: &crate::assets::GridAssets) {
    let dialect = world.scripts.compiler.dialect();
    let scripts = &mut world.script_instances;
    let gone: Vec<CallerId> = scripts
        .slots
        .iter()
        .filter(|(_id, slot)| world.objects.get_by_key(slot.prim).is_none())
        .map(|(id, _slot)| *id)
        .collect();
    for id in gone {
        scripts.remove(id);
    }
    let gone_roots: Vec<ObjectKey> = scripts
        .linkset_data
        .keys()
        .chain(scripts.pay_prices.keys())
        .filter(|key| world.objects.get_by_key(**key).is_none())
        .copied()
        .collect();
    for key in gone_roots {
        let _data = scripts.linkset_data.remove(&key);
        let _layout = scripts.pay_prices.remove(&key);
    }
    for local_id in world.objects.take_changed_contents() {
        world.objects.settle_inventory_flag(local_id);
        let Some(prim) = world.objects.get(local_id).map(|object| object.full_id) else {
            continue;
        };
        let (wanted, running): (
            Vec<(InventoryKey, AssetKey, String)>,
            BTreeSet<InventoryKey>,
        ) = world
            .objects
            .contents(local_id)
            .map(|contents| {
                (
                    contents
                        .items
                        .iter()
                        .filter_map(|item| {
                            runs(item).map(|asset| (item.item_id, asset, item.name.clone()))
                        })
                        .collect(),
                    contents.running.clone(),
                )
            })
            .unwrap_or_default();
        let stale: Vec<CallerId> = scripts
            .slots
            .iter()
            .filter(|(_id, slot)| {
                slot.prim == prim
                    && !wanted
                        .iter()
                        .any(|(item, _asset, _name)| *item == slot.item)
            })
            .map(|(id, _slot)| *id)
            .collect();
        for id in stale {
            scripts.remove(id);
        }
        for (item, asset, name) in wanted {
            let held = scripts.by_item.get(&(prim, item)).copied();
            if let Some(id) = held
                && let Some(slot) = scripts.slots.get_mut(&id)
                && slot.asset == asset
            {
                // Kept, and renamed where the item was.
                slot.name = name;
                continue;
            }
            // New, or saved: a saved script's new instance keeps its id, and
            // with it its place in the order the engine serves scripts in.
            let Some(program) = program_of(&mut scripts.programs, assets, asset, dialect) else {
                tracing::debug!(%prim, %item, "a script does not compile, so it has no instance");
                if let Some(id) = held {
                    scripts.remove(id);
                }
                continue;
            };
            scripts.start(
                Slot {
                    prim,
                    item,
                    asset,
                    name,
                },
                program,
                running.contains(&item),
            );
        }
    }
    // An arrival no script took (one that does not compile) is not kept.
    scripts.arriving.clear();
    // A program no instance runs waits a while for one, and then goes.
    let used: BTreeSet<AssetKey> = scripts.slots.values().map(|slot| slot.asset).collect();
    scripts.programs.retain(|asset, (_program, waited)| {
        if used.contains(asset) {
            *waited = 0;
        } else {
            *waited = waited.saturating_add(1);
        }
        *waited <= PROGRAM_HANDOFF_TICKS
    });
}

/// The program the source stored as `asset` compiles to: the one an upload
/// handed over, or a compile of the stored source. [`None`] when there is no
/// such asset or it does not compile.
fn program_of(
    programs: &mut BTreeMap<AssetKey, (Arc<Program>, u32)>,
    assets: &crate::assets::GridAssets,
    asset: AssetKey,
    dialect: Dialect,
) -> Option<Arc<Program>> {
    if let Some((program, _waited)) = programs.get(&asset) {
        return Some(Arc::clone(program));
    }
    let source = String::from_utf8_lossy(assets.read().get(asset)?).into_owned();
    let program = Arc::new(sl_lsl_runtime::compile_for(&source, dialect).ok()?);
    let _held = programs.insert(asset, (Arc::clone(&program), 0));
    Some(program)
}

/// An avatar in the region when a tick starts: what a script may put a
/// question, a dialog or a page to.
#[derive(Debug, Clone)]
struct Present {
    /// The agent.
    agent: AgentKey,
    /// The experiences it has allowed.
    allowed: Vec<ExperienceKey>,
    /// The experiences it has blocked.
    blocked: Vec<ExperienceKey>,
    /// Whether it has god powers on (OpenSim's `IsGod`: a level of
    /// [`GOD_LEVEL_FLOOR`](crate::god_powers::GOD_LEVEL_FLOOR) or more
    /// granted).
    active_god: bool,
    /// Where it stands in the region.
    position: Vector,
    /// The controls its viewer holds down, as `CONTROL_*` bits.
    controls: i32,
    /// Its movement state as `llGetAnimation` names it.
    state: &'static str,
    /// The `AGENT_*` bits its posture sets.
    posture_info: i32,
    /// The animations playing on it, by asset key.
    animations: Vec<String>,
    /// Its viewer's camera, once it has said where it is.
    camera: Option<sl_proto::Camera>,
}

/// What a tick's scripts did that the region applies under its lock, once
/// the engine is back in place.
#[derive(Debug, Clone)]
enum Effect {
    /// `changed(bits)` a setter raised in the scripts of a prim: raised once
    /// the engine is back in place.
    Changed {
        /// The prim.
        prim: RegionLocalObjectId,
        /// The bits.
        bits: i32,
    },
    /// A line said, as the avatars and the agents' listens hear it; the
    /// scripts in earshot were handed to the engine in the call
    /// ([`Host::say`]).
    Say(Line),
    /// The linkset whose root is this prim leaves the region (`llDie`).
    Die(RegionLocalObjectId),
    /// The prim lets anybody drop an item in, or stops letting them
    /// (`llAllowInventoryDrop`).
    AllowDrop {
        /// The prim.
        prim: RegionLocalObjectId,
        /// Whether anybody may.
        allow: bool,
    },
    /// The prim was moved (`llSetPos`), already in the store: its scripts
    /// are told as a viewer's move tells them, once the engine is back.
    Moved {
        /// The prim.
        prim: RegionLocalObjectId,
        /// Where it now stands, from its parent.
        to: Vector,
    },
    /// The avatar is stood up (`llUnSit`).
    Unsit(AgentKey),
    /// The avatar is seated on the prim (`llSitOnLink`).
    Sit {
        /// The avatar.
        agent: AgentKey,
        /// The prim.
        prim: RegionLocalObjectId,
    },
    /// The root is put on its owner (`llAttachToAvatar`, or as a temporary
    /// attachment `llAttachToAvatarTemp`).
    Attach {
        /// The root.
        root: RegionLocalObjectId,
        /// The owner, and where it stands.
        wearer: (AgentKey, Vector),
        /// The point; 0 for the one it was last worn on.
        point: u8,
        /// Whether it is a temporary attachment, filed as no item.
        temporary: bool,
    },
    /// The worn root is taken off into its wearer's inventory
    /// (`llDetachFromAvatar`).
    Detach {
        /// The root.
        root: RegionLocalObjectId,
        /// Its wearer.
        agent: AgentKey,
    },
    /// A script wrote into the prim's contents: the region's sessions are
    /// told as its grid tells them of a script's write.
    Told {
        /// The prim.
        prim: RegionLocalObjectId,
        /// Whether a script landed or left, which OpenSim sends the prim again
        /// for.
        script: bool,
    },
    /// The prims a script rezzed, root first, already in the region: the
    /// region's sessions are sent them.
    Rezzed(Vec<Object>),
    /// An experience's permission question was put to an avatar: its
    /// answer, or its timeout, is waited for.
    Asked {
        /// The script that asked.
        id: CallerId,
        /// The avatar asked.
        agent: AgentKey,
        /// The experience.
        experience: ExperienceKey,
    },
    /// A call into the engine a change made under the tick asked for
    /// ([`RegionScripts::deferred`]).
    Engine(EngineCall),
    /// What the region's sessions are to be told of a change made under the
    /// tick, already in the store.
    Tell(Vec<RegionChange>),
}

/// What a tick's scripts send to a viewer, delivered once the region's lock
/// is released.
#[derive(Debug, Clone)]
enum Delivery {
    /// A line said to one avatar (`llRegionSayTo` on channel 0).
    Chat {
        /// The avatar.
        agent: AgentKey,
        /// The line.
        line: Line,
    },
    /// A dialog or text box.
    Dialog {
        /// The avatar.
        agent: AgentKey,
        /// The dialog.
        dialog: Box<ScriptDialog>,
    },
    /// A permission question, of the script's own or its experience's.
    Question {
        /// The avatar.
        agent: AgentKey,
        /// The question.
        question: Box<ScriptPermissionRequest>,
    },
    /// A page offered.
    LoadUrl {
        /// The avatar.
        agent: AgentKey,
        /// The offer.
        offer: Box<LoadUrlRequest>,
    },
    /// A world map opened at a place.
    MapDestination {
        /// The avatar.
        agent: AgentKey,
        /// The destination.
        destination: Box<ScriptTeleportRequest>,
    },
    /// An instant message from the object, wherever its avatar is.
    Message {
        /// The avatar.
        agent: AgentKey,
        /// The message.
        message: Box<sl_proto::InstantMessage>,
    },
    /// A sound a prim played once, triggered or had preloaded.
    Sound {
        /// The avatar who hears it.
        agent: AgentKey,
        /// What it hears.
        sound: Box<HeardSound>,
    },
    /// A balance reply for a payment a script made, wherever its avatar is.
    Balance {
        /// The avatar.
        agent: AgentKey,
        /// The reply.
        balance: Box<sl_proto::MoneyBalance>,
    },
    /// The parcel the avatar stands on, changed by a script
    /// (`llSetParcelMusicURL`).
    Parcel {
        /// The avatar.
        agent: AgentKey,
        /// The parcel as it now is.
        parcel: Box<sl_proto::ParcelInfo>,
    },
    /// An animation started or stopped on the avatar
    /// (`llStartAnimation`, `llStopAnimation`).
    Animate {
        /// The avatar.
        agent: AgentKey,
        /// The animation's asset.
        animation: uuid::Uuid,
        /// Whether it starts.
        start: bool,
        /// The prim of the script that asked, whose the animation is.
        object: ObjectKey,
    },
    /// The avatar's animation overrides changed (`llSetAnimationOverride`,
    /// `llResetAnimationOverride`): what its posture plays may have.
    Overridden {
        /// The avatar.
        agent: AgentKey,
    },
    /// The avatar's controls taken or released (`llTakeControls`,
    /// `llReleaseControls`).
    Controls {
        /// The avatar.
        agent: AgentKey,
        /// The `ScriptControlChange` blocks, in order.
        changes: Vec<sl_lsl_runtime::vm::avatars::ControlChange>,
    },
    /// The avatar's follow camera set, or cleared with no rules
    /// (`llSetCameraParams`, `llClearCameraParams`).
    FollowCam {
        /// The avatar.
        agent: AgentKey,
        /// The prim whose script set it.
        object: ObjectKey,
        /// The properties, or [`None`] to clear them.
        rules: Option<Vec<sl_lsl_runtime::vm::avatars::CameraRule>>,
    },
    /// The avatar sent somewhere by a script (`llTeleportAgent`,
    /// `llTeleportAgentGlobalCoords`, `llTeleportAgentHome`): handed to the
    /// grid's teleport machinery after the tick ([`crate::teleport::scripted`]).
    Teleport {
        /// The avatar.
        agent: AgentKey,
        /// Where it is sent.
        destination: Box<sl_lsl_runtime::vm::avatars::Destination>,
        /// The alert it is sent once it has moved, on a grid that sends one.
        alert: Option<String>,
    },
    /// The media of the parcel the avatar stands on, changed or commanded by
    /// a script (`llParcelMediaCommandList`).
    Media {
        /// The avatar.
        agent: AgentKey,
        /// The media as it now is, when it changed.
        update: Option<Box<sl_proto::ParcelMediaUpdateInfo>>,
        /// The command's flag bits, the command and the time it seeks to.
        command: Option<(u32, sl_proto::ParcelMediaCommand, f32)>,
    },
}

/// A sound an avatar is sent (`llPlaySound`, `llTriggerSound`,
/// `llPreloadSound`).
#[derive(Debug, Clone)]
struct HeardSound {
    /// The prim it comes from.
    object: ObjectKey,
    /// The prim's linkset root, when that is another prim.
    parent: Option<ObjectKey>,
    /// The prim's owner.
    owner: OwnerKey,
    /// The sound's asset.
    asset: AssetKey,
    /// Where the prim is in the region.
    position: Vector,
    /// How it is played.
    cue: sl_lsl_runtime::vm::prim::SoundCue,
}

/// The region as a script sees it: the [`Host`] the engine runs a tick
/// against.
struct RegionHost<'tick> {
    /// The grid.
    core: &'tick GridCore,
    /// The region's index in the grid.
    index: usize,
    /// The region's world, its engine lent to the tick.
    world: &'tick mut SceneFixtures,
    /// The grid's experience key-value stores, held for the tick.
    stores: parking_lot::MutexGuard<'tick, BTreeMap<String, KeyValueStore>>,
    /// The avatars in the region when the tick started.
    present: &'tick [Present],
    /// The agents with only a child circuit into the region when the tick
    /// started: they stand in a neighbour, and see this region from there.
    children: &'tick [Present],
    /// Each script's run flag and memory as its engine reads them, when the
    /// tick started: the engine is lent to the tick, and an object's details
    /// sum its scripts'.
    scripts_read: BTreeMap<CallerId, (bool, u32)>,
    /// What to apply to the world after the tick.
    effects: Vec<Effect>,
    /// What to send to viewers after the tick.
    deliveries: Vec<Delivery>,
}

/// The prim a script is in, as the host names it.
struct Origin {
    /// The prim's key.
    prim: ObjectKey,
    /// The prim's region-local id.
    local_id: RegionLocalObjectId,
    /// Its linkset's root.
    root: RegionLocalObjectId,
    /// The prim's record.
    object: Object,
    /// The prim's name.
    name: String,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// Makes a change to the region as the script's call makes it, so a read
    /// later in the tick sees it, as on both live grids (`prim/link`): the
    /// calls into the engine it asks for, and what the region's sessions are
    /// told of it, wait for the tick to be over, in order with its other
    /// effects.
    fn change_now(&mut self, change: impl FnOnce(&mut SceneFixtures) -> Vec<RegionChange>) {
        let told = change(self.world);
        let calls = self
            .world
            .script_instances
            .deferred
            .as_mut()
            .map(core::mem::take)
            .unwrap_or_default();
        self.effects.extend(calls.into_iter().map(Effect::Engine));
        if !told.is_empty() {
            self.effects.push(Effect::Tell(told));
        }
    }

    /// The prim's owner pays `payment` out of the grid's ledger
    /// ([`crate::money::pay_from_object`]): both sides told when it moved, the
    /// owner told of a refusal. Where the grid keeps no balances nothing moves
    /// and only a payee with no account is refused, as OpenSim's stock money
    /// module does.
    fn pay_out(&mut self, caller: CallerId, payment: &Payment) -> Result<(), MoneyRefusal> {
        let payee = uuid::Uuid::parse_str(&payment.to).ok();
        let Some(origin) = self.origin(caller) else {
            return Err(MoneyRefusal::NoSuchEntity);
        };
        let owner = AgentKey::from(origin.object.owner_id);
        let paid = crate::money::pay_from_object(
            self.core,
            owner,
            payee.unwrap_or_else(uuid::Uuid::nil),
            payment.amount,
            &origin.name,
        );
        match paid {
            crate::money::ObjectPayment::Moved(replies) => {
                let (payee_reply, owner_reply) = *replies;
                for reply in [payee_reply, owner_reply] {
                    self.deliveries.push(Delivery::Balance {
                        agent: reply.agent_id,
                        balance: Box::new(reply),
                    });
                }
                Ok(())
            }
            crate::money::ObjectPayment::Refused(reply, refusal) => {
                self.deliveries.push(Delivery::Balance {
                    agent: owner,
                    balance: reply,
                });
                Err(refusal)
            }
            crate::money::ObjectPayment::Unkept => {
                let known = payee
                    .and_then(|key| self.core.account_by_agent(AgentKey::from(key)))
                    .is_some();
                if known {
                    Ok(())
                } else {
                    Err(MoneyRefusal::NoSuchEntity)
                }
            }
        }
    }

    /// The slot of `caller`.
    fn slot(&self, caller: CallerId) -> Option<&Slot> {
        self.world.script_instances.slots.get(&caller)
    }

    /// The prim `caller` is in.
    fn origin(&self, caller: CallerId) -> Option<Origin> {
        let prim = self.slot(caller)?.prim;
        let local_id = self.world.local_id_of(prim)?;
        let object = self.world.objects.get(local_id)?.clone();
        let name = self
            .world
            .properties_of(local_id)
            .map(|properties| properties.name)
            .unwrap_or_default();
        Some(Origin {
            prim,
            local_id,
            root: self.world.root_of(local_id),
            object,
            name,
        })
    }

    /// The key of the root of the object `caller` is in.
    fn object_of(&self, caller: CallerId) -> Option<ObjectKey> {
        let origin = self.origin(caller)?;
        self.world.objects.get(origin.root).map(|root| root.full_id)
    }

    /// Where `caller`'s prim hears and speaks from.
    fn position(&self, caller: CallerId) -> Vector {
        self.slot(caller)
            .and_then(|slot| crate::world::listening_position(&self.world.objects, slot.prim))
            .unwrap_or(Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })
    }

    /// The avatar `agent` (a key as a script names it), when it is in the
    /// region.
    fn present(&self, agent: &str) -> Option<&Present> {
        let key = uuid::Uuid::parse_str(agent).ok()?;
        self.present
            .iter()
            .find(|present| present.agent.uuid() == key)
    }

    /// The legacy name of the account `owner`, or its key when the grid has
    /// no such account (a group, an object's owner nobody registered).
    fn name_of(&self, owner: uuid::Uuid) -> (String, String) {
        self.core
            .account_by_agent(AgentKey::from(owner))
            .map_or_else(
                || (owner.to_string(), String::new()),
                |account| {
                    (
                        account.config.first_name.clone(),
                        account.config.last_name.clone(),
                    )
                },
            )
    }

    /// The alert Second Life sends an avatar a script of `caller`'s teleported:
    /// `You have been teleported by the object '…' on the parcel '…' owned by
    /// …`, naming the object, the parcel it stands on and its owner by user
    /// name (`experience/powers`). Sent for the first teleport since the
    /// avatar last granted the script a permission, and not again: aditi sent
    /// it on the teleport after each grant and not on the teleport back that
    /// followed. OpenSim sends none.
    fn teleport_alert(&mut self, caller: CallerId, agent: AgentKey) -> Option<String> {
        if self.core.teleport_policy.scripted_alert
            != crate::imitates::ScriptedTeleportAlert::NamesTheObject
        {
            return None;
        }
        let origin = self.origin(caller)?;
        if !self
            .world
            .script_instances
            .teleport_alerted
            .insert((caller, agent))
        {
            return None;
        }
        let here = Host::placement(self, caller).position;
        let parcel = self
            .parcel_record(here.x, here.y)
            .map(|parcel| parcel.name)
            .unwrap_or_default();
        let (first, last) = self.name_of(origin.object.owner_id);
        let owner = if last.is_empty() || last == "Resident" {
            first
        } else {
            format!("{first} {last}")
        };
        Some(format!(
            "You have been teleported by the object '{}' on the parcel '{parcel}' owned by \
             {owner}.",
            origin.name
        ))
    }

    /// The linkset `caller` is in, root first.
    fn linkset_prims(&self, caller: CallerId) -> Vec<RegionLocalObjectId> {
        let Some(origin) = self.origin(caller) else {
            return Vec::new();
        };
        std::iter::once(origin.root)
            .chain(crate::link_sets::children_in_order(self.world, origin.root))
            .collect()
    }

    /// The scripts in the prim keyed `prim`, in the order of their ids.
    fn scripts_in(&self, prim: ObjectKey) -> Vec<CallerId> {
        self.world.script_instances.scripts_in(prim)
    }

    /// Put the permission question `mask` — an experience's, where
    /// `experience` names one — to `agent`; `false` when it is not in the
    /// region.
    fn put_question(
        &mut self,
        caller: CallerId,
        agent: &str,
        mask: i32,
        experience: Option<ExperienceKey>,
    ) -> bool {
        let Some(present) = self.present(agent) else {
            return false;
        };
        let agent = present.agent;
        let (Some(origin), Some(item)) =
            (self.origin(caller), self.slot(caller).map(|slot| slot.item))
        else {
            return false;
        };
        if let Some(experience) = experience {
            self.effects.push(Effect::Asked {
                id: caller,
                agent,
                experience,
            });
        }
        let (first, last) = self.name_of(origin.object.owner_id);
        let object_owner = if last.is_empty() {
            first
        } else {
            format!("{first} {last}")
        };
        self.deliveries.push(Delivery::Question {
            agent,
            question: Box::new(ScriptPermissionRequest {
                task_id: origin.prim,
                item_id: item,
                object_name: origin.name,
                object_owner,
                experience_id: experience,
                permissions: ScriptPermissions(mask),
            }),
        });
        true
    }

    /// The avatar wearing the object `caller` is in: the owner of a root
    /// hanging off something that is not a prim of the region (an avatar).
    fn wearer(&self, caller: CallerId) -> Option<AgentKey> {
        let origin = self.origin(caller)?;
        if let Some(attached) = self.world.objects.attached_to(origin.root) {
            return Some(attached.agent);
        }
        // A scenario's worn object, put on its avatar by the record alone.
        let root = self.world.objects.get(origin.root)?;
        (root.parent_id.0 != 0 && !self.world.objects.contains(root.parent_id))
            .then(|| AgentKey::from(root.owner_id))
    }

    /// The prim numbered `link` in `caller`'s linkset, as [`Host::links`]
    /// numbers it.
    fn prim_numbered(&self, caller: CallerId, link: i32) -> Option<RegionLocalObjectId> {
        let prims = self.linkset_prims(caller);
        let root = *prims.first()?;
        let alone = prims.len() == 1 && crate::seating::sitters_of(self.world, root).is_empty();
        if alone {
            return (link == 0).then_some(root);
        }
        usize::try_from(link)
            .ok()?
            .checked_sub(1)
            .and_then(|index| prims.get(index))
            .copied()
    }

    /// Whether the region imitates OpenSim.
    const fn opensim(&self) -> bool {
        self.world.script_instances.flavour().is_opensim()
    }

    /// The region-local id of the prim keyed `prim`.
    fn prim_keyed(&self, prim: &str) -> Option<RegionLocalObjectId> {
        let key = ObjectKey::from(uuid::Uuid::parse_str(prim).ok()?);
        self.world
            .objects
            .get_by_key(key)
            .map(|object| object.local_id)
    }

    /// What `llGetObjectDetails` reads of the prim `local_id`.
    fn prim_details(
        &self,
        local_id: RegionLocalObjectId,
    ) -> Option<sl_lsl_runtime::vm::prim::ObjectDetails> {
        let prim = crate::prim_state::read(self.world, local_id, self.opensim())?;
        let object = self.world.objects.get(local_id)?;
        let properties = self.world.properties_of(local_id)?;
        let root = self.world.root_of(local_id);
        let prims: Vec<RegionLocalObjectId> = std::iter::once(root)
            .chain(crate::link_sets::children_in_order(self.world, root))
            .collect();
        let scripts: Vec<CallerId> = prims
            .iter()
            .filter_map(|prim| self.world.objects.get(*prim))
            .flat_map(|prim| self.world.script_instances.scripts_in(prim.full_id))
            .collect();
        let read: Vec<(bool, u32)> = scripts
            .iter()
            .filter_map(|id| self.scripts_read.get(id).copied())
            .collect();
        let running = read.iter().filter(|(running, _memory)| *running).count();
        let memory: u64 = read
            .iter()
            .map(|(_running, memory)| u64::from(*memory))
            .sum();
        let inventory: usize = prims
            .iter()
            .filter_map(|prim| self.world.objects.contents(*prim))
            .map(|contents| contents.items.len())
            .sum();
        let count = |value: usize| i32::try_from(value).unwrap_or(i32::MAX);
        let owner = match properties.owner {
            OwnerKey::Agent(agent) => agent.uuid(),
            OwnerKey::Group(group) => group.uuid(),
        };
        let masks = &properties.permissions;
        let mask = |value: sl_wire::Permissions| value.bits().cast_signed();
        let attached = self
            .world
            .objects
            .attached_to(root)
            .map_or(0, |attached| i32::from(attached.point));
        Some(sl_lsl_runtime::vm::prim::ObjectDetails {
            avatar: false,
            name: prim.name.clone(),
            description: prim.description.clone(),
            position: prim.position.clone(),
            rotation: prim.rotation.clone(),
            velocity: object.motion.velocity.clone(),
            owner: owner.to_string(),
            group: properties.group.map_or_else(
                || uuid::Uuid::nil().to_string(),
                |group| group.uuid().to_string(),
            ),
            creator: prim.creator.clone(),
            last_owner: properties.last_owner_id.to_string(),
            root: self
                .world
                .objects
                .get(root)
                .map_or_else(|| prim.key.clone(), |root| root.full_id.uuid().to_string()),
            attached_point: attached,
            scripts: (count(scripts.len()), count(running)),
            script_memory: i32::try_from(memory).unwrap_or(i32::MAX),
            prim_count: count(prims.len()),
            inventory_count: count(inventory),
            click_action: prim.click_action,
            omega: Vector {
                x: prim.omega.axis.x * prim.omega.spin,
                y: prim.omega.axis.y * prim.omega.spin,
                z: prim.omega.axis.z * prim.omega.spin,
            },
            scale: prim.scale.clone(),
            material: prim.material,
            flags: (prim.physics, prim.phantom, prim.temp_on_rez),
            text: prim.text.clone(),
            // The region has no cost model yet: every prim costs what each
            // grid said the probe's cube cost (`prim/identity`).
            costs: if self.opensim() {
                (1.0, 1.0, 0.1, 0.0)
            } else {
                (1.0, 0.05, 1.0, 0.5)
            },
            selected: (0, count(crate::seating::sitters_of(self.world, root).len())),
            // The object's root's creation, which a take keeps and a rez
            // from an inventory brings back (`prim/box-beat`: three rezzes of
            // one item, 25 s after it was made, all read the moment it was
            // made).
            rezzed: self
                .world
                .properties_of(root)
                .and_then(|root| i64::try_from(root.creation_date).ok()),
            // The object a script rezzed names it; any other, its owner, who
            // rezzed it (`prim/identity`: aditi and both OpenSim engines read
            // the owner of a cube the avatar rezzed).
            rezzer: self.world.objects.get(root).map_or_else(
                || uuid::Uuid::nil().to_string(),
                |prim| {
                    self.world
                        .script_instances
                        .rezzers
                        .get(&prim.full_id)
                        .map_or_else(|| prim.owner_id.to_string(), ToString::to_string)
                },
            ),
            masks: [
                mask(masks.base),
                mask(masks.owner),
                mask(masks.group),
                mask(masks.everyone),
                mask(masks.next_owner),
            ],
            hover_height: 0.0,
            body_shape: -1.0,
            animated: 0,
        })
    }

    /// What `llGetObjectDetails` reads of an avatar in the region.
    fn avatar_details(&self, present: &Present) -> sl_lsl_runtime::vm::prim::ObjectDetails {
        let agent = present.agent.uuid();
        let (first, last) = self.name_of(agent);
        let worn = self.world.objects.worn_by(present.agent);
        let key = agent.to_string();
        sl_lsl_runtime::vm::prim::ObjectDetails {
            avatar: true,
            name: format!("{first} {last}"),
            description: String::new(),
            position: present.position.clone(),
            rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            velocity: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            owner: key.clone(),
            group: uuid::Uuid::nil().to_string(),
            creator: uuid::Uuid::nil().to_string(),
            last_owner: uuid::Uuid::nil().to_string(),
            root: key,
            attached_point: 0,
            scripts: (0, 0),
            script_memory: 0,
            prim_count: i32::try_from(worn.len()).unwrap_or(i32::MAX),
            inventory_count: 0,
            click_action: 0,
            omega: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            scale: Vector {
                x: 0.45,
                y: 0.6,
                z: 1.9,
            },
            material: 0,
            flags: (false, false, false),
            text: sl_lsl_runtime::vm::prim::HoverText {
                text: String::new(),
                color: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                alpha: 0.0,
            },
            costs: (0.0, 0.0, 0.0, 0.0),
            selected: (0, 0),
            rezzed: None,
            rezzer: uuid::Uuid::nil().to_string(),
            masks: [0; 5],
            hover_height: 0.0,
            body_shape: 0.0,
            animated: 0,
        }
    }

    /// The avatar a script names by `agent`, as the grid keys it.
    fn agent_named(agent: &str) -> Option<AgentKey> {
        uuid::Uuid::parse_str(agent).ok().map(AgentKey::from)
    }
}

/// The chat type a line said at `volume` has.
const fn chat_type_of(volume: ChatVolume) -> ChatType {
    match volume {
        ChatVolume::Whisper => ChatType::Whisper,
        ChatVolume::Say => ChatType::Normal,
        ChatVolume::Shout => ChatType::Shout,
        ChatVolume::RegionSay => ChatType::Region,
    }
}

impl Host for RegionHost<'_> {
    fn print(&mut self, caller: CallerId, text: &str) {
        let script = self.slot(caller).map(|slot| slot.name.clone());
        tracing::info!(?caller, ?script, text, "a script printed");
    }

    fn stubbed(&mut self, caller: CallerId, id: BuiltinId) {
        tracing::debug!(
            ?caller,
            ?id,
            "a stubbed library function answered its default"
        );
        let at = self.whereabouts(caller);
        self.core.tell_stubbed_call(&StubbedCall {
            region: at.region,
            object: at.name,
            script: at.script,
            function: id.descriptor().name,
        });
    }

    fn die(&mut self, caller: CallerId) -> bool {
        if self.wearer(caller).is_some() {
            return false;
        }
        let Some(origin) = self.origin(caller) else {
            return false;
        };
        self.effects.push(Effect::Die(origin.root));
        true
    }

    fn left_state(&mut self, _caller: CallerId) {
        // A script's taken controls are what a state change releases, and the
        // grid hands out none yet (`server-lsl-lib-avatar-control`).
    }

    fn script_named(&self, caller: CallerId, name: &str) -> Option<CallerId> {
        let prim = self.slot(caller)?.prim;
        self.world
            .script_instances
            .slots
            .iter()
            .find(|(_id, slot)| slot.prim == prim && slot.name == name)
            .map(|(id, _slot)| *id)
    }

    fn say(&mut self, caller: CallerId, speech: &Speech) -> Option<Overheard> {
        let origin = self.origin(caller)?;
        let channel = ChatChannel(speech.channel);
        let avatars = Line {
            source: ChatSource::Object(origin.prim),
            from_name: origin.name,
            owner_id: origin.object.owner_id,
            chat_type: if channel == DEBUG_CHANNEL {
                ChatType::DebugChannel
            } else {
                chat_type_of(speech.to_avatars)
            },
            channel,
            position: self.position(caller),
            message: speech.text.clone(),
        };
        let overheard = speech.to_scripts.and_then(|volume| {
            let line = Line {
                chat_type: chat_type_of(volume),
                ..avatars.clone()
            };
            let by = self
                .world
                .script_instances
                .in_earshot(&line, &self.world.objects);
            (!by.is_empty()).then(|| Overheard {
                heard: heard_of(&line),
                by,
            })
        });
        self.effects.push(Effect::Say(avatars));
        overheard
    }

    fn say_to(
        &mut self,
        caller: CallerId,
        target: &str,
        channel: i32,
        text: &str,
    ) -> Option<Overheard> {
        let origin = self.origin(caller)?;
        let key = uuid::Uuid::parse_str(target).ok()?;
        let heard = Heard {
            channel,
            name: origin.name.clone(),
            speaker: origin.prim.uuid().to_string(),
            message: text.to_owned(),
        };
        // A prim's scripts hear it on any channel.
        if self
            .world
            .objects
            .get_by_key(ObjectKey::from(key))
            .is_some()
        {
            let by = self.scripts_in(ObjectKey::from(key));
            return Some(Overheard { heard, by });
        }
        self.present(target)?;
        let agent = AgentKey::from(key);
        if channel == 0 {
            let line = Line {
                source: ChatSource::Object(origin.prim),
                from_name: origin.name,
                owner_id: origin.object.owner_id,
                chat_type: ChatType::Direct,
                channel: ChatChannel(channel),
                position: self.position(caller),
                message: text.to_owned(),
            };
            self.deliveries.push(Delivery::Chat { agent, line });
            return None;
        }
        // On `DEBUG_CHANNEL` the avatar's viewer is sent it as a script error,
        // as Second Life tells an owner a rule list's complaints
        // (`errors/owner-lines`) and OpenSim an OSSL denial marked for the
        // owner.
        if ChatChannel(channel) == DEBUG_CHANNEL {
            let line = Line {
                source: ChatSource::Object(origin.prim),
                from_name: origin.name.clone(),
                owner_id: origin.object.owner_id,
                chat_type: ChatType::DebugChannel,
                channel: ChatChannel(channel),
                position: self.position(caller),
                message: text.to_owned(),
            };
            self.deliveries.push(Delivery::Chat { agent, line });
        }
        // On another channel, the scripts of what the avatar wears.
        let to: Vec<CallerId> = self
            .world
            .script_instances
            .slots
            .keys()
            .copied()
            .filter(|id| self.wearer(*id) == Some(agent))
            .collect();
        Some(Overheard { heard, by: to })
    }

    fn instant_message(&mut self, caller: CallerId, message: &InstantMessage) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let Ok(agent) = uuid::Uuid::parse_str(&message.agent) else {
            return;
        };
        let position = self.position(caller);
        let region_id = self.core.region(self.index).map(|region| region.region_id);
        self.deliveries.push(Delivery::Message {
            agent: AgentKey::from(agent),
            message: Box::new(sl_proto::InstantMessage {
                from_agent_id: AgentKey::from(origin.object.owner_id),
                from_agent_name: origin.name,
                to_agent_id: AgentKey::from(agent),
                dialog: ImDialog::FromTask,
                from_group: false,
                region_id,
                position: RegionCoordinates::new(position.x, position.y, position.z),
                offline: false,
                timestamp: None,
                id: origin.prim.uuid(),
                parent_estate_id: 1,
                message: message.text.clone(),
                binary_bucket: Vec::new(),
            }),
        });
    }

    fn dialog(&mut self, caller: CallerId, dialog: &Dialog) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let Some(present) = self.present(&dialog.agent) else {
            return;
        };
        let agent = present.agent;
        let (owner_first_name, owner_last_name) = self.name_of(origin.object.owner_id);
        self.deliveries.push(Delivery::Dialog {
            agent,
            dialog: Box::new(ScriptDialog {
                object_id: origin.prim,
                object_name: origin.name,
                owner_first_name,
                owner_last_name,
                owner_id: Some(origin.object.owner_id),
                message: dialog.message.clone(),
                chat_channel: ChatChannel(dialog.channel),
                image_id: TextureKey::from(uuid::Uuid::nil()),
                buttons: dialog.buttons.clone(),
            }),
        });
    }

    fn load_url(&mut self, caller: CallerId, offer: &LoadUrl) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let Some(present) = self.present(&offer.agent) else {
            return;
        };
        let agent = present.agent;
        let url = match offer.url.parse::<url::Url>() {
            Ok(url) => url,
            Err(error) => {
                tracing::warn!(url = offer.url, %error, "a script offered a page that is no URL");
                return;
            }
        };
        self.deliveries.push(Delivery::LoadUrl {
            agent,
            offer: Box::new(LoadUrlRequest {
                object_name: origin.name,
                object_id: origin.prim,
                owner: OwnerKey::Agent(AgentKey::from(origin.object.owner_id)),
                message: offer.message.clone(),
                url,
            }),
        });
    }

    fn map_destination(&mut self, caller: CallerId, destination: &MapDestination) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let Some(present) = self.present(&destination.agent) else {
            return;
        };
        let agent = present.agent;
        let region_name =
            sl_wire::region_name_from_wire("script map destination", &destination.region)
                .ok()
                .flatten();
        let (at, look) = (&destination.position, &destination.look_at);
        self.deliveries.push(Delivery::MapDestination {
            agent,
            destination: Box::new(ScriptTeleportRequest {
                object_name: origin.name,
                region_name,
                position: RegionCoordinates::new(at.x, at.y, at.z),
                look_at: Direction::new(look.x, look.y, look.z),
                flags: 0,
            }),
        });
    }

    fn worn_by(&self, caller: CallerId) -> Option<String> {
        self.wearer(caller).map(|agent| agent.uuid().to_string())
    }

    fn links(&self, caller: CallerId) -> Links {
        let Some(origin) = self.origin(caller) else {
            return Links {
                this: 0,
                prims: Vec::new(),
                sitters: Vec::new(),
            };
        };
        let prims = self.linkset_prims(caller);
        let sitters: Vec<LinkedAvatar> = crate::seating::sitters_of(self.world, origin.root)
            .into_iter()
            .map(|agent| {
                let (first, last) = self.name_of(agent.uuid());
                LinkedAvatar {
                    key: agent.uuid().to_string(),
                    name: format!("{first} {last}").trim_end().to_owned(),
                }
            })
            .collect();
        let alone = prims.len() <= 1 && sitters.is_empty();
        let mut this = 0;
        let linked = prims
            .iter()
            .zip(1_i32..)
            .filter_map(|(local_id, number)| {
                let link = if alone { 0 } else { number };
                if *local_id == origin.local_id {
                    this = link;
                }
                let key = self.world.objects.get(*local_id)?.full_id;
                let name = self
                    .world
                    .properties_of(*local_id)
                    .map(|properties| properties.name)
                    .unwrap_or_default();
                Some(LinkedPrim {
                    link,
                    key: key.uuid().to_string(),
                    name,
                    scripts: self.scripts_in(key),
                })
            })
            .collect();
        Links {
            this,
            prims: linked,
            sitters,
        }
    }

    fn create_link(
        &mut self,
        caller: CallerId,
        target: &str,
        parent: bool,
    ) -> Result<(), InvalidLinkTarget> {
        let origin = self.origin(caller).ok_or(InvalidLinkTarget)?;
        let key = uuid::Uuid::parse_str(target).map_err(|_error| InvalidLinkTarget)?;
        let other = self
            .world
            .objects
            .get_by_key(ObjectKey::from(key))
            .map(|prim| prim.local_id)
            .ok_or(InvalidLinkTarget)?;
        let other_root = self.world.root_of(other);
        let owner =
            |root: RegionLocalObjectId| self.world.objects.get(root).map(|prim| prim.owner_id);
        // Neither grid links an object somebody wears, or another owner's.
        let worn = |root: RegionLocalObjectId| self.world.objects.attached_to(root).is_some();
        if worn(other_root) || worn(origin.root) || owner(other_root) != owner(origin.root) {
            return Err(InvalidLinkTarget);
        }
        let (first, second) = if parent {
            (origin.local_id, other)
        } else {
            (other, origin.local_id)
        };
        self.change_now(|world| {
            crate::link_sets::script_link(world, origin.local_id, first, second)
        });
        Ok(())
    }

    fn break_link(&mut self, caller: CallerId, prim: &str) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        if self.world.objects.attached_to(origin.root).is_some() {
            return;
        }
        let Some(local_id) = uuid::Uuid::parse_str(prim)
            .ok()
            .and_then(|key| self.world.local_id_of(ObjectKey::from(key)))
        else {
            return;
        };
        self.change_now(|world| crate::link_sets::script_break_link(world, local_id));
    }

    fn break_all_links(&mut self, caller: CallerId) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        if self.world.objects.attached_to(origin.root).is_none() {
            self.change_now(|world| crate::link_sets::script_break_all_links(world, origin.root));
        }
    }

    fn allow_inventory_drop(&mut self, caller: CallerId, allow: bool) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        self.effects.push(Effect::AllowDrop {
            prim: origin.local_id,
            allow,
        });
    }

    fn owner_say(&mut self, caller: CallerId, text: &str) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let avatars = Line {
            source: ChatSource::Object(origin.prim),
            from_name: origin.name,
            owner_id: origin.object.owner_id,
            chat_type: ChatType::Owner,
            channel: crate::chat::PUBLIC_CHANNEL,
            position: self.position(caller),
            message: text.to_owned(),
        };
        self.effects.push(Effect::Say(avatars));
    }

    fn whereabouts(&self, caller: CallerId) -> Whereabouts {
        let region = self
            .core
            .region(self.index)
            .map(|region| region.config.name.clone())
            .unwrap_or_default();
        let position = self.position(caller);
        let (script, item) = self
            .slot(caller)
            .map(|slot| (slot.name.clone(), slot.item.uuid().to_string()))
            .unwrap_or_default();
        let Some(origin) = self.origin(caller) else {
            return Whereabouts {
                object: String::new(),
                prim: String::new(),
                name: String::new(),
                script,
                item,
                region,
                position,
            };
        };
        let object = self
            .world
            .objects
            .get(origin.root)
            .map_or(origin.prim, |root| root.full_id);
        Whereabouts {
            object: object.uuid().to_string(),
            prim: origin.prim.uuid().to_string(),
            name: origin.name,
            script,
            item,
            region,
            position,
        }
    }

    fn faulted(&mut self, caller: CallerId, fault: &Fault) {
        let at = self.whereabouts(caller);
        let recorded = ScriptFault {
            region: at.region,
            object: at.name,
            prim: uuid::Uuid::parse_str(&at.prim).ok().map(ObjectKey::from),
            script: at.script,
            error: fault.error.to_string(),
            position: fault
                .position
                .map(|position| (position.line, position.column)),
            event: fault.event,
        };
        tracing::debug!(%recorded, "a script faulted");
        self.core.tell_script_fault(&recorded);
    }

    fn random_bits(&mut self, _caller: CallerId) -> u32 {
        self.world.script_instances.draw()
    }

    fn mint_scene_id(&mut self) -> i32 {
        i32::try_from(self.world.mint_local_id().0).unwrap_or(i32::MAX)
    }

    fn linkset(&self, caller: CallerId) -> LinksetId {
        LinksetId(
            self.origin(caller)
                .map_or(0, |origin| u64::from(origin.root.0)),
        )
    }

    fn linkset_data(&mut self, caller: CallerId) -> &mut LinksetData {
        let root = self
            .origin(caller)
            .and_then(|origin| self.world.objects.get(origin.root))
            .map_or_else(|| ObjectKey::from(uuid::Uuid::nil()), |root| root.full_id);
        self.world
            .script_instances
            .linkset_data
            .entry(root)
            .or_default()
    }

    fn owner(&self, caller: CallerId) -> String {
        self.origin(caller)
            .map_or_else(uuid::Uuid::nil, |origin| origin.object.owner_id)
            .to_string()
    }

    fn seated_on(&self, caller: CallerId, agent: &str) -> bool {
        let (Some(origin), Some(agent)) = (self.origin(caller), Self::agent_named(agent)) else {
            return false;
        };
        self.world
            .objects
            .seat_of(agent)
            .is_some_and(|(seat, _sitter)| self.world.root_of(seat.local_id) == origin.root)
    }

    fn ask_permissions(&mut self, caller: CallerId, question: &Question) -> bool {
        self.put_question(caller, &question.agent, question.mask, None)
    }

    fn give_money(&mut self, caller: CallerId, payment: &Payment) {
        // `llGiveMoney` says nothing of a refusal; the owner's failed balance
        // reply is all there is of one.
        let _paid = self.pay_out(caller, payment);
    }

    fn transfer_money(&mut self, caller: CallerId, payment: &Payment) -> Result<(), MoneyRefusal> {
        self.pay_out(caller, payment)
    }

    fn set_pay_price(&mut self, caller: CallerId, layout: PayPrice) {
        if let Some(origin) = self.origin(caller) {
            let _old = self
                .world
                .script_instances
                .pay_prices
                .insert(origin.prim, layout);
        }
    }

    fn pass_touches(&mut self, caller: CallerId, pass: i32) {
        if let Some(origin) = self.origin(caller) {
            self.world.touches.set_pass(origin.prim, pass);
        }
    }

    fn pass_collisions(&mut self, caller: CallerId, pass: i32) {
        if let Some(origin) = self.origin(caller) {
            self.world.physics.set_pass(origin.prim, pass);
        }
    }

    fn move_to_target(
        &mut self,
        caller: CallerId,
        target: Option<sl_lsl_runtime::vm::prim::MoveTarget>,
    ) {
        if let Some(object) = self.object_of(caller) {
            self.world.physics.set_target(object, target);
        }
    }

    fn set_buoyancy(&mut self, caller: CallerId, buoyancy: f32) {
        if let Some(object) = self.object_of(caller) {
            self.world.physics.set_buoyancy(object, buoyancy);
        }
    }

    fn experience(&self, caller: CallerId) -> Option<ScriptExperience> {
        if self.world.script_instances.flavour().is_opensim() {
            return None;
        }
        let slot = self.slot(caller)?;
        let experience = self
            .world
            .script_experience(slot.prim, slot.item)
            .flatten()?;
        let origin = self.origin(caller)?;
        let record =
            crate::experiences::record_for(experience, AgentKey::from(origin.object.owner_id));
        let position = self.position(caller);
        let maturity = self
            .core
            .region(self.index)
            .map_or(sl_proto::Maturity::Pg, |region| region.config.maturity);
        let standing = self.world.experience_land.standing(
            experience,
            record.as_ref(),
            self.world.parcel_at_position(&position),
            maturity,
        );
        Some(ScriptExperience {
            key: experience.uuid().to_string(),
            standing,
        })
    }

    fn experience_details(&mut self, _caller: CallerId, id: &str) -> Option<ExperienceDetails> {
        let found = uuid::Uuid::parse_str(id).ok().and_then(|key| {
            crate::experiences::catalogue()
                .into_iter()
                .find(|record| record.public_id.uuid() == key)
        });
        if self.world.script_instances.looked_up.insert(id.to_owned()) && found.is_none() {
            return None;
        }
        Some(found.map_or_else(
            || ExperienceDetails::invalid(id),
            |record| {
                ExperienceDetails {
                    name: record.name,
                    owner: record
                        .owner
                        .map_or_else(uuid::Uuid::nil, |owner| owner.uuid())
                        .to_string(),
                    id: id.to_owned(),
                    state: ExperienceError::None,
                    group: record
                        .group
                        .map_or_else(uuid::Uuid::nil, |group| group.uuid())
                        .to_string(),
                }
            },
        ))
    }

    fn admission(&self, agent: &str, experience: &str) -> Admission {
        let (Some(present), Ok(experience)) =
            (self.present(agent), uuid::Uuid::parse_str(experience))
        else {
            return Admission::Undecided;
        };
        if present.blocked.iter().any(|key| key.uuid() == experience) {
            Admission::Blocked
        } else if present.allowed.iter().any(|key| key.uuid() == experience) {
            Admission::Allowed
        } else {
            Admission::Undecided
        }
    }

    fn ask_experience(&mut self, caller: CallerId, question: &ExperienceQuestion) -> bool {
        let experience = uuid::Uuid::parse_str(&question.experience)
            .ok()
            .map(ExperienceKey::from);
        self.put_question(caller, &question.agent, question.mask, experience)
    }

    fn key_value_store(&mut self, experience: &str) -> Result<&mut KeyValueStore, ExperienceError> {
        Ok(self.stores.entry(experience.to_owned()).or_default())
    }

    fn sit_on_link(&mut self, caller: CallerId, agent: &str, link: i32) -> Result<(), SitRefusal> {
        let Some(present) = self.present(agent) else {
            return Err(SitRefusal::InvalidAgent);
        };
        let agent = present.agent;
        if self.wearer(caller).is_some() {
            return Err(SitRefusal::InvalidObject);
        }
        let origin = self.origin(caller).ok_or(SitRefusal::InvalidObject)?;
        // The parcel the object stands on has to let the avatar in.
        let (centre, _turn) = crate::seating::frame_of(self.world, &origin.object);
        let banned = self
            .world
            .parcel_at(centre.x, centre.y)
            .is_some_and(|parcel| {
                self.world
                    .access_list(parcel.local_id, sl_proto::ParcelAccessScope::Ban)
                    .iter()
                    .any(|entry| entry.id == agent.uuid())
            });
        if banned {
            return Err(SitRefusal::NoAccess);
        }
        // The named prim's target, or the next free one down the linkset.
        let prims = self.linkset_prims(caller);
        let first = self
            .prim_numbered(caller, link)
            .ok_or(SitRefusal::NoSitTarget)?;
        let free = prims
            .iter()
            .skip_while(|prim| **prim != first)
            .find(|prim| {
                self.world.objects.get(**prim).is_some_and(|object| {
                    self.world
                        .objects
                        .seat_settings(object.local_id)
                        .is_some_and(|seat| seat.target.is_some())
                        && crate::seating::place(self.world, object).on_target
                })
            })
            .copied()
            .ok_or(SitRefusal::NoSitTarget)?;
        self.effects.push(Effect::Sit { agent, prim: free });
        Ok(())
    }

    fn seat(
        &mut self,
        caller: CallerId,
        link: i32,
    ) -> Option<&mut sl_lsl_runtime::vm::seating::Seat> {
        let prim = self.prim_numbered(caller, link)?;
        self.world.objects.seat_settings_mut(prim)
    }

    fn avatar_on_sit_target(&self, caller: CallerId, link: i32) -> Option<String> {
        let prim = self.prim_numbered(caller, link)?;
        self.world
            .objects
            .sitters(prim)
            .iter()
            .find(|sitter| sitter.on_target)
            .map(|sitter| sitter.agent.uuid().to_string())
    }

    fn unsit(&mut self, caller: CallerId, agent: &str) {
        let (Some(origin), Some(agent)) = (self.origin(caller), Self::agent_named(agent)) else {
            return;
        };
        let Some((seat, _sitter)) = self.world.objects.seat_of(agent) else {
            return;
        };
        let on_this = self.world.root_of(seat.local_id) == origin.root;
        // Sitting on something else, the avatar is stood up by an object whose
        // owner owns the land it is over, or is a god (`LSL_Api.llUnSit`).
        let over_owners_land = || {
            let at = crate::seating::seated_position(self.world, agent);
            let owner = AgentKey::from(origin.object.owner_id);
            at.and_then(|at| self.world.parcel_at(at.x, at.y))
                .is_some_and(|parcel| parcel.owner == OwnerKey::Agent(owner))
                || self.standing(caller).is(Allowed::God)
        };
        if on_this || over_owners_land() {
            self.effects.push(Effect::Unsit(agent));
        }
    }

    fn set_pie_text(&mut self, caller: CallerId, sit: bool, text: &str) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let Some(mut properties) = self.world.properties_of(origin.local_id) else {
            return;
        };
        if sit {
            text.clone_into(&mut properties.sit_name);
        } else {
            text.clone_into(&mut properties.touch_name);
        }
        if let Some(mut prim) = self.world.object_mut(origin.local_id) {
            prim.properties = Some(properties);
        }
    }

    fn attached(&self, caller: CallerId) -> i32 {
        self.origin(caller)
            .and_then(|origin| self.world.objects.attached_to(origin.root))
            .map_or(0, |attached| i32::from(attached.point))
    }

    fn attach(&mut self, caller: CallerId, point: i32) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let owner = origin.object.owner_id.to_string();
        let Some(present) = self.present(&owner) else {
            return;
        };
        let wearer = (present.agent, present.position.clone());
        self.effects.push(Effect::Attach {
            root: origin.root,
            wearer,
            point: u8::try_from(point).unwrap_or(0),
            temporary: false,
        });
    }

    fn attach_temp(&mut self, caller: CallerId, point: i32) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let owner = origin.object.owner_id.to_string();
        let Some(present) = self.present(&owner) else {
            return;
        };
        let wearer = (present.agent, present.position.clone());
        self.effects.push(Effect::Attach {
            root: origin.root,
            wearer,
            point: u8::try_from(point).unwrap_or(0),
            temporary: true,
        });
    }

    fn detach(&mut self, caller: CallerId) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        if let Some(attached) = self.world.objects.attached_to(origin.root) {
            let agent = attached.agent;
            self.effects.push(Effect::Detach {
                root: origin.root,
                agent,
            });
        }
    }

    fn placement(&self, caller: CallerId) -> sl_lsl_runtime::vm::seating::Placement {
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let Some(origin) = self.origin(caller) else {
            return sl_lsl_runtime::vm::seating::Placement {
                position: zero.clone(),
                local: zero.clone(),
                root: zero,
            };
        };
        let (position, _turn) = crate::seating::frame_of(self.world, &origin.object);
        let root = self.world.objects.get(origin.root).map_or_else(
            || position.clone(),
            |root| crate::seating::frame_of(self.world, root).0,
        );
        // A root's own record is its region position, but a worn root's is its
        // offset on the point; a child's is its offset from the root.
        let local = if origin.local_id == origin.root
            && self.world.objects.attached_to(origin.root).is_none()
        {
            position.clone()
        } else {
            origin.object.motion.position.clone()
        };
        sl_lsl_runtime::vm::seating::Placement {
            position,
            local,
            root,
        }
    }

    fn set_position(&mut self, caller: CallerId, local: Vector) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let mut to = local;
        // An unworn root's position is a place in the region: it stays out
        // of the ground, as on both grids, and in the region. A move past the
        // edge would cross to a neighbour on Second Life; the fake grid does
        // not carry an object over a border yet
        // (`server-world-changed-object-crossing`), so the prim stays where
        // it was, as OpenSim's refusal of a position outside its region
        // leaves it.
        if origin.local_id == origin.root && self.world.objects.attached_to(origin.root).is_none() {
            let edge = u16::try_from(REGION_EDGE_M).map_or(f32::MAX, f32::from);
            if !(0.0..edge).contains(&to.x) || !(0.0..edge).contains(&to.y) {
                return;
            }
            let ground = self
                .core
                .region(self.index)
                .map_or(0.0, |region| region.config.terrain.height_at(to.x, to.y));
            to.z = to.z.max(ground);
        }
        // OpenSim's part raises its position change whether a child goes
        // anywhere or not; a root that stays put raises nothing anywhere.
        let child = origin.local_id != origin.root;
        let raises_anyway = child && self.opensim();
        let Some(mut prim) = self.world.object_mut(origin.local_id) else {
            return;
        };
        if prim.motion.position == to && !raises_anyway {
            return;
        }
        prim.motion.position = to.clone();
        drop(prim);
        self.effects.push(Effect::Moved {
            prim: origin.local_id,
            to,
        });
    }

    fn prim(&self, _caller: CallerId, prim: &str) -> Option<sl_lsl_runtime::vm::prim::Prim> {
        let local_id = self.prim_keyed(prim)?;
        crate::prim_state::read(self.world, local_id, self.opensim())
    }

    fn rezzed_in(&self, prim: &str) -> Option<Tick> {
        self.world
            .objects
            .rezzed_at(self.prim_keyed(prim)?)
            .map(Tick)
    }

    fn set_prim(&mut self, _caller: CallerId, state: &sl_lsl_runtime::vm::prim::Prim) {
        let Some(local_id) = self.prim_keyed(&state.key) else {
            return;
        };
        let opensim = self.opensim();
        let region = self.core.region(self.index);
        let ground =
            |x: f32, y: f32| region.map_or(0.0, |region| region.config.terrain.height_at(x, y));
        let edge = u16::try_from(REGION_EDGE_M).map_or(f32::MAX, f32::from);
        let _written = crate::prim_state::write(self.world, local_id, state, opensim, ground, edge);
    }

    fn set_region_position(&mut self, caller: CallerId, position: Vector) -> Option<Vector> {
        let origin = self.origin(caller)?;
        if self.world.objects.attached_to(origin.root).is_some() {
            return None;
        }
        let edge = u16::try_from(REGION_EDGE_M).map_or(f32::MAX, f32::from);
        if !(0.0..edge).contains(&position.x) || !(0.0..edge).contains(&position.y) {
            return None;
        }
        let ground = self.core.region(self.index).map_or(0.0, |region| {
            region.config.terrain.height_at(position.x, position.y)
        });
        let mut to = position;
        to.z = to.z.max(ground);
        let mut root = self.world.object_mut(origin.root)?;
        root.motion.position = to.clone();
        Some(to)
    }

    fn linkset_of(&self, key: &str) -> Vec<String> {
        let Some(local_id) = self.prim_keyed(key) else {
            return Vec::new();
        };
        let root = self.world.root_of(local_id);
        std::iter::once(root)
            .chain(crate::link_sets::children_in_order(self.world, root))
            .filter_map(|prim| self.world.objects.get(prim))
            .map(|prim| prim.full_id.uuid().to_string())
            .collect()
    }

    fn object_details(
        &self,
        _caller: CallerId,
        key: &str,
    ) -> Option<sl_lsl_runtime::vm::prim::ObjectDetails> {
        if let Some(local_id) = self.prim_keyed(key) {
            return self.prim_details(local_id);
        }
        if let Some(present) = self.present(key) {
            return Some(self.avatar_details(present));
        }
        // OpenSim keeps a child agent's `ScenePresence` in the region and
        // answers `llGetObjectDetails` from it (`comms/owner-away`: the owner
        // in a neighbour still has its `OBJECT_POS`); Second Life answers
        // only for an avatar in the region.
        if !self.world.script_instances.flavour().is_opensim() {
            return None;
        }
        let agent = uuid::Uuid::parse_str(key).ok()?;
        let child = self
            .children
            .iter()
            .find(|child| child.agent.uuid() == agent)?;
        Some(self.avatar_details(child))
    }

    fn inventory_key(&self, caller: CallerId, name: &str, asset_type: i32) -> Option<String> {
        let origin = self.origin(caller)?;
        self.world
            .objects
            .contents(origin.local_id)?
            .items
            .iter()
            .find(|item| item.name == name && item.asset_type.to_code() == asset_type)
            .and_then(|item| item.asset_id)
            .map(|asset| asset.uuid().to_string())
    }

    fn sound(&mut self, _caller: CallerId, prim: &str, cue: &sl_lsl_runtime::vm::prim::SoundCue) {
        use sl_lsl_runtime::vm::prim::SoundCue;
        let Some(local_id) = self.prim_keyed(prim) else {
            return;
        };
        let Some(object) = self.world.objects.get(local_id).cloned() else {
            return;
        };
        let asset = match cue {
            SoundCue::Play { sound, .. }
            | SoundCue::Trigger { sound, .. }
            | SoundCue::Preload { sound } => sound,
        };
        let Ok(asset) = uuid::Uuid::parse_str(asset) else {
            return;
        };
        let root = self.world.root_of(local_id);
        let parent = (root != local_id)
            .then(|| self.world.objects.get(root).map(|root| root.full_id))
            .flatten();
        let (position, _turn) = crate::seating::frame_of(self.world, &object);
        let bounds = match cue {
            SoundCue::Trigger { bounds, .. } => bounds.clone(),
            _ => None,
        };
        for present in self.present {
            if let Some((low, high)) = &bounds {
                let at = &present.position;
                let inside = (low.x..=high.x).contains(&at.x)
                    && (low.y..=high.y).contains(&at.y)
                    && (low.z..=high.z).contains(&at.z);
                if !inside {
                    continue;
                }
            }
            self.deliveries.push(Delivery::Sound {
                agent: present.agent,
                sound: Box::new(HeardSound {
                    object: object.full_id,
                    parent,
                    owner: OwnerKey::Agent(AgentKey::from(object.owner_id)),
                    asset: AssetKey::from(asset),
                    position: position.clone(),
                    cue: cue.clone(),
                }),
            });
        }
    }

    fn prim_count(&self, object: &str) -> Option<i32> {
        let key = ObjectKey::from(uuid::Uuid::parse_str(object).ok()?);
        let prim = self.world.objects.get_by_key(key)?;
        let root = self.world.root_of(prim.local_id);
        let children = crate::link_sets::children_in_order(self.world, root).len();
        i32::try_from(children.saturating_add(1)).ok()
    }

    fn standing(&self, caller: CallerId) -> Standing {
        let Some(origin) = self.origin(caller) else {
            return Standing::default();
        };
        let owner = origin.object.owner_id;
        let item = self.slot(caller).and_then(|slot| {
            self.world
                .objects
                .contents(origin.local_id)?
                .items
                .iter()
                .find(|held| held.item_id == slot.item)
                .cloned()
        });
        let mut roles = Vec::new();
        let estate_owner = self.core.estate_owner.uuid();
        if owner == estate_owner {
            roles.push(Allowed::EstateOwner);
        }
        if self.world.estate.managers.contains(&owner) && owner != estate_owner {
            roles.push(Allowed::EstateManager);
        }
        let position = self.position(caller);
        if self
            .world
            .parcel_at(position.x, position.y)
            .is_some_and(|parcel| parcel.owner == OwnerKey::Agent(AgentKey::from(owner)))
        {
            roles.push(Allowed::ParcelOwner);
        }
        // OpenSim's `PermissionsModule`: a grid god is an account of a god's
        // level; any god (`IsAdministrator`) is one, or the estate owner
        // (`region_owner_is_god`, as `GodMode::at_login` grants it); an active
        // god has god powers on in the region.
        let grid_god = self.core.accounts.iter().any(|account| {
            account.agent_id.uuid() == owner
                && account.config.god_level >= crate::god_powers::GOD_LEVEL_FLOOR
        });
        if grid_god {
            roles.push(Allowed::GridGod);
        }
        if grid_god || owner == estate_owner {
            roles.push(Allowed::God);
        }
        if self
            .present
            .iter()
            .any(|present| present.agent.uuid() == owner && present.active_god)
        {
            roles.push(Allowed::ActiveGod);
        }
        Standing {
            owner: owner.to_string(),
            creator: item
                .as_ref()
                .map_or_else(uuid::Uuid::nil, |item| item.creator_id.uuid())
                .to_string(),
            modifiable: item.is_some_and(|item| {
                item.permissions
                    .next_owner
                    .contains(sl_wire::Permissions::MODIFY)
            }),
            roles,
        }
    }

    fn simulator(&self) -> Simulator {
        let version = self
            .core
            .region(self.index)
            .and_then(|region| region.config.simulator_version.clone())
            .unwrap_or_else(|| crate::runtime::SERVER_CHANNEL_VERSION.to_owned());
        Simulator {
            version,
            grid_name: self.core.identity.name.clone(),
            grid_nick: self.core.identity.nick.clone(),
            physics_engine_type: PHYSICS_ENGINE_TYPE.to_owned(),
            physics_engine_name: PHYSICS_ENGINE_NAME.to_owned(),
        }
    }

    fn region_size(&self, _caller: CallerId) -> (u32, u32) {
        (REGION_EDGE_M, REGION_EDGE_M)
    }

    fn terrain_height(&self, _caller: CallerId, x: u32, y: u32) -> f32 {
        let metres = |at: u32| u16::try_from(at).map_or(0.0, f32::from);
        self.core.region(self.index).map_or(0.0, |region| {
            region.config.terrain.height_at(metres(x), metres(y))
        })
    }

    fn day_length(&self, _caller: CallerId) -> f32 {
        self.day().0
    }

    fn prim_scripts(&self, prim: &str) -> Option<Vec<CallerId>> {
        let key = ObjectKey::from(uuid::Uuid::parse_str(prim).ok()?);
        self.world.objects.get_by_key(key)?;
        Some(self.scripts_in(key))
    }

    fn agent_ip(&self, agent: &str) -> String {
        self.present(agent)
            .map_or_else(String::new, |_present| LOOPBACK_ADDRESS.to_owned())
    }

    fn contents(&self, caller: CallerId) -> Vec<sl_lsl_runtime::vm::Item> {
        self.contents_of(caller)
    }

    fn remove_item(&mut self, caller: CallerId, name: &str, changed: &[i32]) -> bool {
        self.remove_from(caller, name, changed)
    }

    fn give_to_prim(&mut self, caller: CallerId, target: &str, items: &[String]) -> bool {
        self.give_into(caller, target, items)
    }

    fn offer_items(&mut self, caller: CallerId, offer: &sl_lsl_runtime::vm::Offer) -> bool {
        self.offer_to(caller, offer)
    }

    fn set_remote_pin(&mut self, caller: CallerId, pin: i32) {
        self.set_pin(caller, pin);
    }

    fn remote_pin(&self, prim: &str) -> Option<(i32, String)> {
        self.pin_of(prim)
    }

    fn load_script(&mut self, caller: CallerId, load: &sl_lsl_runtime::vm::RemoteLoad) {
        self.load_into(caller, load);
    }

    fn rez(
        &mut self,
        caller: CallerId,
        rez: &sl_lsl_runtime::vm::Rez,
    ) -> Result<String, sl_lsl_runtime::vm::RezRefusal> {
        self.rez_from(caller, rez)
    }

    fn derez(&mut self, caller: CallerId, object: &str) -> bool {
        self.derez_of(caller, object)
    }

    fn notecard_text(&mut self, asset: &str) -> Option<String> {
        self.read_notecard(asset)
    }

    fn notecard_read(&self, asset: &str) -> Option<String> {
        self.notecard_if_read(asset)
    }

    fn parcel_at(&self, _caller: CallerId, x: f32, y: f32) -> Option<Parcel> {
        self.parcel_record(x, y)
    }

    fn region(&self, _caller: CallerId) -> Region {
        self.region_record()
    }

    fn sim_data(&self, _caller: CallerId, name: &str) -> Option<SimData> {
        self.sim_record(name)
    }

    fn ground(&self, _caller: CallerId, x: f32, y: f32) -> f32 {
        self.ground_at(x, y)
    }

    fn wind(&self, _caller: CallerId, _position: &Vector) -> Vector {
        self.wind_at()
    }

    fn environment(&self, _caller: CallerId, _position: Option<&Vector>, unix: i64) -> Environment {
        self.environment_at(unix)
    }

    fn change_land(&mut self, _caller: CallerId, position: &Vector, change: &LandChange) -> bool {
        self.land_change(position, change)
    }

    fn parcel_media(&self, _caller: CallerId, position: &Vector) -> ParcelMedia {
        self.media_of(position)
    }

    fn sim_stats(&self, _caller: CallerId) -> Vec<f32> {
        self.stats()
    }

    fn environment_rule(
        &self,
        _caller: CallerId,
        _position: &Vector,
        rule: i32,
    ) -> Option<Vec<sl_lsl_runtime::Element>> {
        self.environment_values(rule)
    }

    fn map_texture(&self, _caller: CallerId, region: &str) -> Option<String> {
        self.map_texture_of(region)
    }

    fn send_http(&mut self, caller: CallerId, request: &sl_lsl_runtime::vm::http::Outbound) {
        self.send_http_of(caller, request);
    }

    fn request_url(&mut self, caller: CallerId, key: &str, secure: bool) -> Option<String> {
        self.request_url_of(caller, key, secure)
    }

    fn release_url(&mut self, caller: CallerId, url: &str) {
        self.release_url_of(caller, url);
    }

    fn free_urls(&self, _caller: CallerId) -> i32 {
        self.free_urls_of()
    }

    fn answer_http(
        &mut self,
        _caller: CallerId,
        request: &str,
        answered: sl_lsl_runtime::vm::http::Answered,
    ) {
        self.answer_http_of(request, answered);
    }

    fn http_throttle(&mut self, caller: CallerId) -> &mut sl_lsl_runtime::vm::http::Throttle {
        self.throttle_of(caller)
    }

    fn url_authority(&self, _caller: CallerId) -> String {
        http::authority(self.core)
    }

    fn agent(&self, _caller: CallerId, agent: &str) -> Option<sl_lsl_runtime::vm::avatars::Agent> {
        self.agent_of(agent)
    }

    fn agent_record(
        &self,
        _caller: CallerId,
        agent: &str,
    ) -> Option<sl_lsl_runtime::vm::avatars::AgentRecord> {
        self.agent_record_of(agent)
    }

    fn agent_named(&self, _caller: CallerId, name: &str) -> Option<(String, bool)> {
        self.agent_called(name)
    }

    fn animate(&mut self, caller: CallerId, agent: &str, animation: &str, start: bool) {
        let Some(object) = self.slot(caller).map(|slot| slot.prim) else {
            return;
        };
        self.animate_agent(agent, animation, start, object);
    }

    fn set_animation_override(
        &mut self,
        _caller: CallerId,
        agent: &str,
        state: &str,
        animation: Option<(&str, &str)>,
    ) {
        self.override_animation(agent, state, animation);
    }

    fn animation_override(&self, _caller: CallerId, agent: &str, state: &str) -> Option<String> {
        self.override_of(agent, state)
    }

    fn object_animate(&mut self, caller: CallerId, name: &str, _animation: &str, _start: bool) {
        // The fake grid plays no animesh from a script yet: say so, rather
        // than pretend it played.
        self.refuse_unmodelled(caller, &format!("llStartObjectAnimation(\"{name}\")"));
    }

    fn object_animations(&self, _caller: CallerId) -> Vec<String> {
        Vec::new()
    }

    fn control_changes(
        &mut self,
        caller: CallerId,
        agent: &str,
        changes: &[sl_lsl_runtime::vm::avatars::ControlChange],
        taken: Option<i32>,
    ) {
        self.change_controls(caller, agent, changes, taken);
    }

    fn set_camera(
        &mut self,
        caller: CallerId,
        agent: &str,
        rules: &[sl_lsl_runtime::vm::avatars::CameraRule],
    ) {
        self.follow_camera(caller, agent, Some(rules));
    }

    fn clear_camera(&mut self, caller: CallerId, agent: &str) {
        self.follow_camera(caller, agent, None);
    }

    fn camera(&self, _caller: CallerId, agent: &str) -> Option<(Vector, sl_types::lsl::Rotation)> {
        self.camera_of_agent(agent)
    }

    fn teleport_agent(
        &mut self,
        caller: CallerId,
        agent: &str,
        destination: &sl_lsl_runtime::vm::avatars::Destination,
    ) {
        // The grid's teleport machinery runs outside the tick, and waits on
        // the client: the teleport is handed to it once the tick is over.
        let Some(present) = self.present(agent) else {
            return;
        };
        let agent = present.agent;
        let alert = match destination {
            sl_lsl_runtime::vm::avatars::Destination::Home => None,
            _sent => self.teleport_alert(caller, agent),
        };
        self.deliveries.push(Delivery::Teleport {
            agent,
            destination: Box::new(destination.clone()),
            alert,
        });
    }

    fn attached_list(&self, _caller: CallerId, agent: &str) -> Option<Vec<String>> {
        self.attached_of(agent)
    }

    fn make_notecard(
        &mut self,
        caller: CallerId,
        name: &str,
        description: &str,
        text: &str,
        changed: &[i32],
    ) {
        self.make_notecard_in(caller, (name, description, text), changed);
    }

    fn epoch_micros(&self) -> i64 {
        self.world.script_instances.epoch_micros.unwrap_or(0)
    }

    fn local_offset(&self, _unix: i64) -> i32 {
        // The fake grid's simulator host keeps UTC.
        0
    }

    fn day_offset(&self, _caller: CallerId) -> f32 {
        self.day().1
    }

    fn sense(&self, _caller: CallerId) -> Vec<sl_lsl_runtime::vm::sensing::Sensed> {
        self.sensed()
    }

    fn cast_ray(
        &self,
        caller: CallerId,
        start: &Vector,
        end: &Vector,
    ) -> Vec<sl_lsl_runtime::vm::sensing::RayHit> {
        self.ray_hits(caller, start, end)
    }
}

/// The avatars standing in the region at `index`, in session order, and
/// beside them the agents that see it down a child circuit from a neighbour.
async fn presence(core: &GridCore, index: usize) -> (Vec<Present>, Vec<Present>) {
    let mut sessions: Vec<_> = core
        .sessions
        .lock()
        .await
        .iter()
        .map(|(seq, shared)| (*seq, shared.clone()))
        .collect();
    sessions.sort_unstable_by_key(|(seq, _shared)| *seq);
    let mut present = Vec::new();
    let mut children = Vec::new();
    for (_seq, shared) in sessions {
        if shared.is_closed() {
            continue;
        }
        let here = shared
            .with_state(|state| {
                (state.region == index).then(|| {
                    let (allowed, blocked) = state.sim.experiences().agent_permissions();
                    let present = Present {
                        agent: state.avatar.agent_id,
                        allowed,
                        blocked,
                        active_god: state.god_mode.told >= crate::god_powers::GOD_LEVEL_FLOOR,
                        position: crate::chat::agent_position(&state.sim),
                        controls: i32::from_ne_bytes(state.motion.controls().bits().to_ne_bytes()),
                        state: state.motion.posture().state_name(),
                        posture_info: state.motion.posture().agent_info(),
                        animations: state
                            .animations
                            .playing(state.avatar.agent_id)
                            .iter()
                            .map(|playing| playing.anim_id.to_string())
                            .collect(),
                        camera: state.motion.camera().cloned(),
                    };
                    (state.sim.is_root_agent(), present)
                })
            })
            .await;
        match here {
            Some((true, here)) => present.push(here),
            Some((false, here)) => children.push(here),
            None => {}
        }
    }
    (present, children)
}

/// One tick of the region's scripts: what it ran, or [`None`] for a region
/// the grid does not have or that has no script.
pub(crate) async fn run_tick(core: &Arc<GridCore>, index: usize) -> Option<TickReport> {
    let region = core.region(index)?;
    let (scripted, unflagged) = {
        let mut world = region.world.lock();
        reconcile(&mut world, &core.assets);
        let scripted = !world.script_instances.is_empty();
        // The last script gone takes its prim's touch flags with it.
        let unflagged = if scripted {
            Vec::new()
        } else {
            crate::touches::refresh_flags(&mut world)
        };
        drop(world);
        (scripted, unflagged)
    };
    for change in unflagged {
        drop(region.changes.send(RegionUpdate {
            source: REGION_SOURCE,
            change,
        }));
    }
    if !scripted {
        // Physics runs whether or not a script is there to hear of it.
        let changes = {
            let mut world = region.world.lock();
            crate::updates::begin(&mut world);
            let step = world.script_instances.engine.config().step;
            crate::physics::step(&mut world, &[], step, &|x, y| {
                region.config.terrain.height_at(x, y)
            });
            let changes = crate::updates::finish(&mut world, Vec::new());
            crate::attachments::finish_leaving(&mut world, &core.assets, (core.clock)());
            drop(world);
            changes
        };
        for change in changes {
            drop(region.changes.send(RegionUpdate {
                source: REGION_SOURCE,
                change,
            }));
        }
        return None;
    }
    let (present, children) = presence(core, index).await;
    let (report, changes, deliveries) = {
        let mut world = region.world.lock();
        crate::updates::begin(&mut world);
        let (report, changes, deliveries) =
            tick_locked(core, index, &mut world, &present, &children);
        let changes = crate::updates::finish(&mut world, changes);
        // What came off before this tick has had its turn, and leaves.
        crate::attachments::finish_leaving(&mut world, &core.assets, (core.clock)());
        drop(world);
        (report, changes, deliveries)
    };
    for change in changes {
        // A region nobody is in has nobody to tell.
        drop(region.changes.send(RegionUpdate {
            source: REGION_SOURCE,
            change,
        }));
    }
    deliver(core, index, deliveries).await;
    Some(report)
}

/// What a script hears of `line`.
fn heard_of(line: &Line) -> Heard {
    Heard {
        channel: line.channel.0,
        name: line.from_name.clone(),
        speaker: line.source.source_id().to_string(),
        message: line.message.clone(),
    }
}

/// Runs one engine tick over `world` and applies to it what the scripts did,
/// returning the tick's report, the changes the region's sessions are to be
/// told and what is to be sent to viewers.
fn tick_locked(
    core: &GridCore,
    index: usize,
    world: &mut SceneFixtures,
    present: &[Present],
    children: &[Present],
) -> (TickReport, Vec<RegionChange>, Vec<Delivery>) {
    let config = *world.script_instances.engine.config();
    // A touch held open is due its `touch` before the scripts run.
    let agents: Vec<AgentKey> = present.iter().map(|present| present.agent).collect();
    crate::touches::tick(world, config.step, &agents);
    // What physics does in the tick, the scripts hear of in it.
    let avatars: Vec<crate::physics::Avatar> = present
        .iter()
        .map(|present| crate::physics::Avatar {
            agent: present.agent,
            name: core.account_by_agent(present.agent).map_or_else(
                || present.agent.uuid().to_string(),
                |account| format!("{} {}", account.config.first_name, account.config.last_name),
            ),
            position: present.position.clone(),
        })
        .collect();
    if let Some(region) = core.region(index) {
        crate::physics::step(world, &avatars, config.step, &|x, y| {
            region.config.terrain.height_at(x, y)
        });
    }
    // A prim that comes into the region from here to the next tick came in
    // in the tick about to run.
    world
        .objects
        .set_rez_tick(world.script_instances.engine.now().after(1).0);
    if world.script_instances.epoch_micros.is_none() {
        // Where the region's ticks started: its wall clock now, less the
        // ticks already run.
        let wall = i64::try_from(world.wall.unix_micros()).unwrap_or(i64::MAX);
        let ran = ticks_as_micros(world.script_instances.engine.now(), config.step);
        world.script_instances.epoch_micros = Some(wall.saturating_sub(ran));
    }
    http::hand_in(core, index, &mut world.script_instances);
    {
        let scripts = &mut world.script_instances;
        avatars::hand_in_controls(&mut scripts.engine, &mut scripts.avatars, present);
    }
    let scripts_read = {
        let scripts = &world.script_instances;
        let flavour = scripts.flavour();
        scripts
            .slots
            .keys()
            .filter_map(|id| {
                let instance = scripts.engine.instance(*id)?;
                Some((
                    *id,
                    (
                        instance.reads_running(flavour),
                        instance.script_memory(flavour),
                    ),
                ))
            })
            .collect()
    };
    let mut engine = core::mem::replace(&mut world.script_instances.engine, Engine::new(config));
    world.script_instances.deferred = Some(Vec::new());
    let mut host = RegionHost {
        core,
        index,
        world: &mut *world,
        stores: core.experience_stores.lock(),
        present,
        children,
        scripts_read,
        effects: Vec::new(),
        deliveries: Vec::new(),
    };
    let report = engine.tick(&mut host);
    let effects = core::mem::take(&mut host.effects);
    let deliveries = core::mem::take(&mut host.deliveries);
    drop(host);
    world.script_instances.engine = engine;
    // A call no effect carries was asked for outside a change the host made
    // as a script's call: it is made first.
    for call in world.script_instances.deferred.take().unwrap_or_default() {
        world.script_instances.make(call);
    }
    world.script_instances.last_load = (report.runnable, report.served);
    http::hand_out(core, index, &mut world.script_instances);
    tell_rezzers(world);
    let mut changes = Vec::new();
    for effect in effects {
        match effect {
            Effect::Changed { prim, bits } => raise_changed_in(world, &[prim], bits),
            Effect::Say(avatars) => changes.push(world.say_to_avatars(avatars)),
            Effect::Die(root) => changes.extend(remove_linkset(world, root)),
            Effect::Engine(call) => world.script_instances.make(call),
            Effect::Tell(told) => changes.extend(told),
            Effect::AllowDrop { prim, allow } => {
                if let Some(mut object) = world.object_mut(prim) {
                    let flags = object.update_flags;
                    if allow {
                        object.update_flags |= prim_flags::ALLOW_INVENTORY_DROP;
                    } else {
                        object.update_flags &= !prim_flags::ALLOW_INVENTORY_DROP;
                    }
                    if object.update_flags == flags {
                        continue;
                    }
                }
                if let Some(object) = world.object_by_local_id(prim) {
                    changes.push(RegionChange::Updated(Box::new(object)));
                }
            }
            Effect::Moved { prim, to } => {
                let transform = sl_proto::ObjectTransform {
                    position: Some(to),
                    ..sl_proto::ObjectTransform::default()
                };
                crate::changes::transformed(world, prim, &transform);
            }
            Effect::Unsit(agent) => {
                if let Some(stands_at) = crate::seating::stand(world, agent) {
                    changes.push(RegionChange::Unseated { agent, stands_at });
                }
            }
            Effect::Sit { agent, prim } => {
                let Some(seat) = world.objects.get(prim).cloned() else {
                    continue;
                };
                let Some(transform) = crate::seating::sit(world, agent, &seat) else {
                    continue;
                };
                if let Some((_seat, sitter)) = world.objects.seat_of(agent) {
                    changes.push(RegionChange::Seated {
                        agent,
                        offset: sitter.offset.clone(),
                        rotation: sitter.rotation.clone(),
                        seat: Box::new(seat),
                        transform: Box::new(transform),
                    });
                }
            }
            // A temporary attachment is worn as no item and shown at once;
            // any other is filed as a new item by its wearer's session first,
            // which then shows it ([`RegionChange::FileWorn`]).
            Effect::Attach {
                root,
                wearer: (agent, at),
                point,
                temporary,
            } => {
                let item = if temporary {
                    crate::scene_store::WornItem::Temporary
                } else {
                    crate::scene_store::WornItem::Item(InventoryKey::from(
                        world.script_instances.mint_id(),
                    ))
                };
                if let Some(objects) =
                    crate::seating::wear(world, (agent, &at), root, (point, None), item)
                {
                    changes.push(if temporary {
                        RegionChange::Worn { agent, objects }
                    } else {
                        RegionChange::FileWorn { agent, root }
                    });
                }
            }
            Effect::Detach { root, agent } => {
                changes.push(RegionChange::TakeOff { agent, root });
            }
            Effect::Told { prim, script } => {
                changes.extend(crate::task_contents::told_by_script(world, prim, script));
            }
            Effect::Rezzed(objects) => {
                changes.extend(
                    objects
                        .into_iter()
                        .map(|object| RegionChange::Rezzed(Box::new(object))),
                );
            }
            Effect::Asked {
                id,
                agent,
                experience,
            } => {
                let _earlier = world
                    .script_instances
                    .experience_questions
                    .insert((id, agent), (experience, report.tick));
            }
        }
    }
    world
        .script_instances
        .time_out_experience_questions(report.tick, config.step);
    start_loaded(world);
    // What the tick started, saved over or moved to another state may take
    // touches now, or no longer.
    changes.extend(crate::touches::refresh_flags(world));
    (report, changes, deliveries)
}

/// `ticks` ticks of `step`, in microseconds.
fn ticks_as_micros(ticks: Tick, step: Duration) -> i64 {
    i64::try_from(u128::from(ticks.0).saturating_mul(step.as_micros())).unwrap_or(i64::MAX)
}

/// Tells each rezzer whose object's scripts ran their `on_rez` in the tick
/// just run — on Second Life, whose object was rezzed in it — of the object
/// it rezzed (`object_rez`): before what the object's scripts said is heard,
/// so on both grids the rezzer hears of the rez first though the child ran
/// before it (`inventory/rez`).
fn tell_rezzers(world: &mut SceneFixtures) {
    let scripts = &mut world.script_instances;
    let mut waiting = core::mem::take(&mut scripts.rezzes_waiting);
    waiting.retain_mut(|(rezzer, object, waited)| {
        if *waited == 0 {
            *waited = 1;
            return true;
        }
        let key = object.uuid().to_string();
        if let Err(error) = scripts.engine.object_rezzed(*rezzer, &key) {
            tracing::warn!(?rezzer, %error, "object_rez could not be raised");
        }
        false
    });
    scripts.rezzes_waiting = waiting;
}

/// Starts each script a load put in a prim once the tick after the one it
/// landed in is over, if it is to run.
fn start_loaded(world: &mut SceneFixtures) {
    let mut starting = core::mem::take(&mut world.script_instances.starting);
    starting.retain_mut(|(prim, item, running, waited)| {
        if *waited == 0 {
            *waited = 1;
            return true;
        }
        if *running {
            let _started = world.script_instances.set_running(*prim, *item, true);
            if let Some(local_id) = world.local_id_of(*prim)
                && let Some(contents) = world.objects.contents_mut(local_id)
            {
                let _was = contents.running.insert(*item);
            }
        }
        false
    });
    world.script_instances.starting = starting;
}

/// The scripts in the linkset whose root is `root`, prim by prim in link
/// order.
pub(crate) fn linkset_scripts(world: &SceneFixtures, root: RegionLocalObjectId) -> Vec<CallerId> {
    let prims: Vec<ObjectKey> = std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .filter_map(|local_id| world.objects.get(local_id).map(|prim| prim.full_id))
        .collect();
    let mut scripts = Vec::new();
    for prim in prims {
        scripts.extend(
            world
                .script_instances
                .slots
                .iter()
                .filter(|(_id, slot)| slot.prim == prim)
                .map(|(id, _slot)| *id),
        );
    }
    scripts
}

/// Raises `changed(bits)` in every script of the linkset whose root is
/// `root` — what concerns the whole object, as an avatar sitting on it does.
pub(crate) fn raise_changed(world: &mut SceneFixtures, root: RegionLocalObjectId, bits: i32) {
    for id in linkset_scripts(world, root) {
        world.script_instances.raise_changed_or_defer(id, bits);
    }
}

/// Raises `changed(bits)` in the scripts of each of `prims`, prim by prim in
/// the order given — what concerns a prim and not its whole set, as a link
/// that moved it into one does on OpenSim.
pub(crate) fn raise_changed_in(
    world: &mut SceneFixtures,
    prims: &[RegionLocalObjectId],
    bits: i32,
) {
    for local_id in prims {
        let Some(key) = world.objects.get(*local_id).map(|prim| prim.full_id) else {
            continue;
        };
        for id in world.script_instances.scripts_in(key) {
            world.script_instances.raise_changed_or_defer(id, bits);
        }
    }
}

/// Drops `bits` of what `agent` granted every script of the linkset whose
/// root is `root`: a lapse, as a sitter's standing up drops
/// `PERMISSION_TAKE_CONTROLS` and `PERMISSION_CONTROL_CAMERA`.
pub(crate) fn lapse(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
    agent: AgentKey,
    bits: i32,
) {
    let granter = agent.uuid().to_string();
    for id in linkset_scripts(world, root) {
        world.script_instances.revoke_or_defer(id, &granter, bits);
    }
}

/// Tells every script of the linkset whose root is `root` that it was put on
/// by `wearer`, or — [`None`] — taken off: `attach(wearer)`, on OpenSim after
/// its own `changed(32768)` twice for a wear and once for a drop
/// (`events/attach`; Second Life raises no `changed` for either). A drop of an
/// object that `travelled` here on its wearer raises no `changed(32768)` on
/// OpenSim either (`events/changed-worn`).
pub(crate) fn raise_attach(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
    wearer: Option<AgentKey>,
    travelled: bool,
) {
    let moves = match (world.script_instances.flavour().is_opensim(), wearer) {
        (false, _) => 0,
        (true, Some(_)) => 2,
        (true, None) if travelled => 0,
        (true, None) => 1,
    };
    let wearer = wearer.map(|agent| agent.uuid().to_string());
    for id in linkset_scripts(world, root) {
        let engine = &mut world.script_instances.engine;
        for _ in 0..moves {
            if let Err(error) = engine.changed(id, sl_lsl_runtime::vm::OPENSIM_CHANGED_POSITION) {
                tracing::warn!(?id, %error, "raising OpenSim's position change failed");
            }
        }
        if let Err(error) = engine.attach(id, wearer.as_deref()) {
            tracing::warn!(?id, %error, "raising attach failed");
        }
    }
}

/// Takes the linkset whose root is `root` out of the region with its
/// scripts, children first: a `KillObject` for each prim.
pub(crate) fn remove_linkset(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
) -> Vec<RegionChange> {
    let mut changes = crate::seating::stand_all_on(world, root);
    let mut prims = crate::link_sets::children_in_order(world, root);
    prims.push(root);
    for local_id in prims {
        let Some(object) = world.remove_object(local_id) else {
            continue;
        };
        let leaving: Vec<CallerId> = world
            .script_instances
            .slots
            .iter()
            .filter(|(_id, slot)| slot.prim == object.full_id)
            .map(|(id, _slot)| *id)
            .collect();
        for id in leaving {
            world.script_instances.remove(id);
        }
        changes.push(RegionChange::Killed(local_id));
    }
    changes
}

/// Sends what a tick's scripts said to viewers, each to the session it is
/// for: one in the region, but an instant message wherever its avatar is.
async fn deliver(core: &Arc<GridCore>, index: usize, deliveries: Vec<Delivery>) {
    for delivery in deliveries {
        let agent = match &delivery {
            Delivery::Chat { agent, .. }
            | Delivery::Dialog { agent, .. }
            | Delivery::Question { agent, .. }
            | Delivery::LoadUrl { agent, .. }
            | Delivery::MapDestination { agent, .. }
            | Delivery::Message { agent, .. }
            | Delivery::Sound { agent, .. }
            | Delivery::Balance { agent, .. }
            | Delivery::Parcel { agent, .. }
            | Delivery::Media { agent, .. }
            | Delivery::Animate { agent, .. }
            | Delivery::Overridden { agent }
            | Delivery::Controls { agent, .. }
            | Delivery::FollowCam { agent, .. }
            | Delivery::Teleport { agent, .. } => *agent,
        };
        if let Delivery::Teleport {
            destination, alert, ..
        } = delivery
        {
            crate::teleport::scripted(core, index, agent, *destination, alert).await;
            continue;
        }
        let Some(shared) = core.root_session_of(agent).await else {
            tracing::debug!(%agent, "a script spoke to an avatar no longer in world");
            continue;
        };
        let sent = shared
            .with_state(|state| {
                let now = shared.now();
                if state.region != index
                    && !matches!(
                        delivery,
                        Delivery::Message { .. } | Delivery::Balance { .. }
                    )
                {
                    return Ok(());
                }
                match &delivery {
                    Delivery::Animate {
                        animation,
                        start,
                        object,
                        ..
                    } => {
                        if state.animations.apply_scripted(*animation, *start, *object) {
                            state_animations(state, now)?;
                        }
                        Ok(())
                    }
                    Delivery::Overridden { .. } => {
                        crate::movement::state_posture(state, now);
                        Ok(())
                    }
                    _ => send(&mut state.sim, &delivery, now),
                }
            })
            .await;
        if let Err(error) = sent {
            tracing::warn!(%agent, %error, "sending what a script said failed");
        }
    }
}

/// States the agent's animation set to its viewer, as a change to it is, and
/// to the region's other viewers: a pose ball's pose is seen by everybody
/// standing round it, as the agent's own animations are.
fn state_animations(
    state: &mut crate::driver::SimState,
    now: Instant,
) -> Result<(), sl_proto::Error> {
    let agent = state.avatar.agent_id;
    let own = state.animations.own(state.animation_policy, agent);
    let statement = state.animations.observed(state.animation_policy, agent);
    state.tell_others(|resident| {
        resident.animations.clone_from(&statement);
        RegionChange::ResidentAnimated {
            agent,
            statement: Box::new(statement),
        }
    });
    own.send(agent, &mut state.sim, now)
}

/// Sends one `delivery` down `sim`.
fn send(sim: &mut SimSession, delivery: &Delivery, now: Instant) -> Result<(), sl_proto::Error> {
    match delivery {
        Delivery::Chat { line, .. } => sim.send_chat_from_simulator(
            &line.from_name,
            line.source,
            line.owner_id,
            line.chat_type,
            FULLY_AUDIBLE,
            line.position.clone(),
            &line.message,
            now,
        ),
        Delivery::Dialog { dialog, .. } => sim.send_script_dialog(dialog, now),
        Delivery::Question { question, .. } => sim.send_script_question(question, now),
        Delivery::LoadUrl { offer, .. } => sim.send_load_url(offer, now),
        Delivery::MapDestination { destination, .. } => {
            sim.send_script_teleport_request(destination, now)
        }
        Delivery::Message { message, .. } => sim.send_instant_message(message, now),
        Delivery::Sound { sound, .. } => send_sound(sim, sound, now),
        Delivery::Balance { balance, .. } => sim.send_money_balance_reply(balance, now),
        Delivery::Controls { changes, .. } => {
            let controls: Vec<sl_proto::ScriptControl> = changes
                .iter()
                .map(|change| sl_proto::ScriptControl {
                    action: if change.take {
                        sl_proto::ScriptControlAction::Take
                    } else {
                        sl_proto::ScriptControlAction::Release
                    },
                    controls: sl_proto::ControlFlags::from_bits(change.controls),
                    pass_to_agent: change.pass_to_agent,
                })
                .collect();
            sim.send_script_control_change(&controls, now)
        }
        Delivery::FollowCam { object, rules, .. } => match rules {
            Some(rules) => {
                let properties: Vec<sl_proto::FollowCamPropertyValue> = rules
                    .iter()
                    .map(|rule| sl_proto::FollowCamPropertyValue {
                        property: sl_proto::FollowCamProperty::from_i32(rule.property),
                        value: rule.value,
                    })
                    .collect();
                sim.send_set_follow_cam_properties(*object, &properties, now)
            }
            None => sim.send_clear_follow_cam_properties(*object, now),
        },
        // Sent with the session's animation set, above; a teleport is the
        // grid's, not the session's.
        Delivery::Animate { .. } | Delivery::Overridden { .. } | Delivery::Teleport { .. } => {
            Ok(())
        }
        Delivery::Parcel { parcel, .. } => {
            // Both grids push a changed parcel down the event queue.
            sim.enqueue_parcel_properties(parcel);
            Ok(())
        }
        Delivery::Media {
            update, command, ..
        } => {
            if let Some(update) = update {
                sim.send_parcel_media_update(update, now)?;
            }
            match command {
                Some((flags, command, time)) => {
                    sim.send_parcel_media_command(*flags, *command, *time, now)
                }
                None => Ok(()),
            }
        }
    }
}

/// Sends `sound` down `sim`: a sound played once is the prim's attached
/// sound without the loop flag, a triggered one a `SoundTrigger` at the
/// prim's position, a preload a `PreloadSound`.
fn send_sound(
    sim: &mut SimSession,
    sound: &HeardSound,
    now: Instant,
) -> Result<(), sl_proto::Error> {
    use sl_lsl_runtime::vm::prim::SoundCue;
    match &sound.cue {
        SoundCue::Play { volume, slave, .. } => {
            let flags = if *slave {
                sl_proto::SoundFlags(sl_proto::SoundFlags::SYNC_SLAVE)
            } else {
                sl_proto::SoundFlags(0)
            };
            sim.send_attached_sound(sound.object, sound.owner, sound.asset, *volume, flags, now)
        }
        SoundCue::Trigger { volume, .. } => sim.send_sound_trigger(
            sound.asset,
            sound.owner,
            sound.object,
            sound.parent,
            sound.position.clone(),
            *volume,
            now,
        ),
        SoundCue::Preload { .. } => sim.send_preload_sound(
            &[sl_proto::SoundPreload {
                object_id: sound.object,
                owner_id: sound.owner.uuid(),
                sound_id: sound.asset.uuid(),
            }],
            now,
        ),
    }
}

/// Answers one drained [`ServerEvent`] if it is a command to a script —
/// reset it, answer or revoke its permissions, reply to its dialog, ask its
/// pay layout — returning what the region's others are to be told. `None`
/// for anything else.
pub(crate) fn answer_script_command(
    world: &mut SceneFixtures,
    (identity, region_id): (&AvatarIdentity, uuid::Uuid),
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    let agent = identity.agent_id.uuid().to_string();
    match event {
        ServerEvent::ResetScript { object_id, item_id } => {
            let reset = world
                .script_instances
                .reset(*object_id, InventoryKey::from(*item_id));
            if !reset {
                tracing::debug!(%object_id, %item_id, "a reset named a script with no instance");
            }
            Some(Vec::new())
        }
        ServerEvent::ScriptPermissionAnswer {
            task_id,
            item_id,
            permissions,
        } => {
            let Some(id) = world.script_instances.instance_of(*task_id, *item_id) else {
                return Some(Vec::new());
            };
            let scripts = &mut world.script_instances;
            let experience = scripts
                .experience_questions
                .remove(&(id, identity.agent_id))
                .map(|(experience, _asked)| experience);
            let answered = if let Some(experience) = experience {
                // Accepting an experience's question is the agent allowing
                // the experience, which the simulator records: the viewer
                // sends nothing else (`script_question_cb`). A refusal is
                // recorded by nobody; a block comes over
                // `ExperiencePreferences` on its own.
                let answer = if permissions.0 == 0 {
                    Err(ExperienceError::NotPermitted)
                } else {
                    sim.experiences_mut()
                        .set_preference(experience, ExperiencePermission::Allow);
                    Ok(())
                };
                scripts
                    .engine
                    .answer_experience(id, &agent, answer)
                    .map(|posted| posted.is_some())
            } else {
                scripts
                    .engine
                    .answer(id, &agent, permissions.0)
                    .map(|posted| posted.is_some())
            };
            match answered {
                // A grant starts the script's teleports over: the next one is
                // alerted again on a grid that alerts it.
                Ok(true) => {
                    let _was = scripts.teleport_alerted.remove(&(id, identity.agent_id));
                }
                Ok(false) => tracing::debug!(?id, "an answer to a question the script did not put"),
                Err(error) => {
                    tracing::warn!(?id, %error, "a permission answer could not be posted");
                }
            }
            Some(Vec::new())
        }
        // A revoke names a prim, or the region for every prim in it (the
        // viewer's revoke of the animation permissions: `money/lapse`, on
        // aditi and YEngine). OpenSim's `HandleRevokePermissions` hears only
        // one that takes an animation permission.
        ServerEvent::RevokeScriptPermissions {
            object_id,
            permissions,
        } => {
            const ANIMATION_PERMISSIONS: i32 = 0x10 | 0x8000;
            if world.script_instances.flavour().is_opensim()
                && permissions.0 & ANIMATION_PERMISSIONS == 0
            {
                return Some(Vec::new());
            }
            let every_prim = object_id.uuid() == region_id;
            let ids: Vec<CallerId> = world
                .script_instances
                .slots
                .iter()
                .filter(|(_id, slot)| every_prim || slot.prim == *object_id)
                .map(|(id, _slot)| *id)
                .collect();
            for id in ids {
                world
                    .script_instances
                    .engine
                    .revoke(id, &agent, permissions.0);
            }
            Some(Vec::new())
        }
        // The reply is a line the avatar says on the dialog's channel, heard
        // across the whole region (OpenSim's `HandlerScriptDialogReply`:
        // "Behaviour in SL is that the response can be heard from any
        // distance").
        ServerEvent::ScriptDialogReply { answer, .. } => {
            let line = Line {
                source: ChatSource::Agent(identity.agent_id),
                from_name: format!("{} {}", identity.first_name, identity.last_name),
                owner_id: identity.agent_id.uuid(),
                chat_type: ChatType::Region,
                channel: answer.chat_channel,
                position: crate::chat::agent_position(sim),
                message: answer.button_label.clone(),
            };
            Some(vec![world.say(line)])
        }
        // What a script set with `llSetPayPrice`, as the grid answers it; a
        // prim no script set a layout for answers the default one.
        ServerEvent::RequestPayPrice { object_id } => {
            let flavour = world.script_instances.flavour();
            let present = world.local_id_of(*object_id).is_some();
            if let Some(layout) = world
                .script_instances
                .pay_price(*object_id)
                .unwrap_or_default()
                .reply(flavour)
                .filter(|_layout| present)
                && let Err(error) =
                    sim.send_pay_price_reply(*object_id, layout.price, &layout.buttons, now)
            {
                tracing::warn!(%object_id, %error, "answering a pay layout failed");
            }
            Some(Vec::new())
        }
        _other => None,
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::assets::GridAssets;
    use crate::world::{TaskInventory, box_prim};

    /// The prim the tests script.
    const PRIM: RegionLocalObjectId = RegionLocalObjectId(7);

    /// A script that compiles.
    const SOURCE: &str = "default { state_entry() { llSay(0, \"one\"); } }";

    /// Another that compiles.
    const SAVED: &str = "default { state_entry() { llSay(0, \"two\"); } }";

    /// One that does not.
    const BROKEN: &str = "default { state_entry() { integer x = ; } }";

    /// The prim's key.
    fn prim_key() -> ObjectKey {
        ObjectKey::from(uuid::Uuid::from_u128(0x7))
    }

    /// The script item's key.
    fn item_key() -> InventoryKey {
        InventoryKey::from(uuid::Uuid::from_u128(0x17))
    }

    /// An asset key from a small number.
    fn asset(id: u128) -> AssetKey {
        AssetKey::from(uuid::Uuid::from_u128(id))
    }

    /// The script item, its source stored as `body`.
    fn item(body: AssetKey) -> TaskInventoryItem {
        TaskInventoryItem {
            item_id: item_key(),
            parent_task: prim_key(),
            asset_id: Some(body),
            ..crate::scenario::stock_script_item()
        }
    }

    /// A region holding the prim, its contents holding the item running, and
    /// the store holding the three sources.
    fn region() -> Result<(SceneFixtures, GridAssets), &'static str> {
        let mut world = SceneFixtures::new();
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        world.objects.push(box_prim(
            PRIM,
            prim_key(),
            AgentKey::from(uuid::Uuid::from_u128(0x1)),
            zero.clone(),
            zero,
        ));
        let mut contents = TaskInventory::stated(1, vec![item(asset(0xA))]);
        let _running = contents.running.insert(item_key());
        world
            .add_task_inventory(PRIM, contents)
            .map_err(|_contents| "the prim is not in the region")?;
        let assets = GridAssets::default();
        for (id, source) in [(0xA, SOURCE), (0xB, SAVED), (0xC, BROKEN)] {
            let _previous = assets.write().insert(asset(id), source.as_bytes().to_vec());
        }
        assert!(assets.read().get(asset(0xA)).is_some());
        Ok((world, assets))
    }

    /// The asset the item's instance was compiled from, or `None` when it
    /// has no instance.
    fn instance_asset(world: &SceneFixtures) -> Option<AssetKey> {
        let id = world.script_instances.instance_of(prim_key(), item_key())?;
        world.script_instances.slots.get(&id).map(|slot| slot.asset)
    }

    /// The prim's script gets an instance, running as its contents say; a
    /// save gets it a fresh one of the new program under the same id; a save
    /// that does not compile leaves it none; a removed item takes its own
    /// with it.
    #[test]
    fn the_instances_follow_the_contents() -> Result<(), &'static str> {
        let (mut world, assets) = region()?;
        reconcile(&mut world, &assets);
        assert_eq!(instance_asset(&world), Some(asset(0xA)));
        assert_eq!(
            world.script_instances.reads_running(prim_key(), item_key()),
            Some(true)
        );
        let first = world.script_instances.instance_of(prim_key(), item_key());

        // Nothing written, nothing changed.
        reconcile(&mut world, &assets);
        assert_eq!(
            world.script_instances.instance_of(prim_key(), item_key()),
            first
        );

        world
            .objects
            .contents_mut(PRIM)
            .ok_or("no contents")?
            .write(item(asset(0xB)));
        reconcile(&mut world, &assets);
        assert_eq!(instance_asset(&world), Some(asset(0xB)));
        assert_eq!(
            world.script_instances.instance_of(prim_key(), item_key()),
            first,
            "a save keeps the script's place in the serving order"
        );

        world
            .objects
            .contents_mut(PRIM)
            .ok_or("no contents")?
            .write(item(asset(0xC)));
        reconcile(&mut world, &assets);
        assert_eq!(instance_asset(&world), None);

        world
            .objects
            .contents_mut(PRIM)
            .ok_or("no contents")?
            .write(item(asset(0xA)));
        reconcile(&mut world, &assets);
        assert_eq!(instance_asset(&world), Some(asset(0xA)));
        let _removed = world
            .objects
            .contents_mut(PRIM)
            .ok_or("no contents")?
            .remove(item_key());
        reconcile(&mut world, &assets);
        assert!(world.script_instances.is_empty());
        Ok(())
    }

    /// A prim that leaves the region takes its scripts with it, however it
    /// left.
    #[test]
    fn a_prim_takes_its_scripts_when_it_goes() -> Result<(), &'static str> {
        let (mut world, assets) = region()?;
        reconcile(&mut world, &assets);
        assert_eq!(world.script_instances.len(), 1);
        let _gone = world.remove_object(PRIM);
        reconcile(&mut world, &assets);
        assert!(world.script_instances.is_empty());
        Ok(())
    }

    /// A program an upload compiled is the one the saved script runs, not a
    /// second compile of its source — even when the source is not in the
    /// store at all.
    #[test]
    fn a_save_is_compiled_once() -> Result<(), &'static str> {
        let (mut world, assets) = region()?;
        reconcile(&mut world, &assets);
        let program = sl_lsl_runtime::compile_for(SAVED, Dialect::SecondLife)
            .map_err(|_errors| "the saved source does not compile")?;
        world.script_instances.offer_program(asset(0xD), program);
        world
            .objects
            .contents_mut(PRIM)
            .ok_or("no contents")?
            .write(item(asset(0xD)));
        reconcile(&mut world, &assets);
        assert_eq!(instance_asset(&world), Some(asset(0xD)));
        Ok(())
    }
}
