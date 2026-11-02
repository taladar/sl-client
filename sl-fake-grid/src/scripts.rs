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
//! # Not yet wired
//!
//! - Sitting is not world state: `seated_on` is always `false` and
//!   `llSitOnLink` finds no sit target.
//! - The grid keeps no money balances: `llGiveMoney` moves nothing,
//!   `llTransferLindenDollars` refuses only a payee with no account, and a
//!   payment to an object raises no `money` event.
//! - An experience script's standing is not judged against the catalogue,
//!   the region and the parcel: it may run wherever it is.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_lsl_runtime::Dialect;
use sl_lsl_runtime::bytecode::Program;
use sl_lsl_runtime::library::BuiltinId;
use sl_lsl_runtime::vm::{
    Admission, Allowed, CallerId, Dialog, Engine, EngineConfig, EngineFlavour, ExperienceDetails,
    ExperienceError, ExperienceQuestion, Heard, Host, InstantMessage, KeyValueStore, LinkedPrim,
    Links, LinksetData, LinksetId, LoadUrl, MapDestination, MoneyRefusal, PayPrice, Payment,
    Question, SavedScript, ScriptExperience, ScriptUsage, Simulator, SitRefusal, Speech, Standing,
    TickReport, Whereabouts,
};
use sl_proto::{
    AssetKey, AssetSource as _, AssetType, ChatSource, ChatType, ImDialog, InventoryType,
    LoadUrlRequest, Object, RegionLocalObjectId, ScriptDialog, ScriptLanguage,
    ScriptPermissionRequest, ScriptTeleportRequest, ServerEvent, SimSession, TaskInventoryItem,
};
use sl_types::chat::{ChatChannel, ChatVolume};
use sl_types::key::{AgentKey, ExperienceKey, InventoryKey, ObjectKey, OwnerKey, TextureKey};
use sl_types::lsl::{ScriptPermissions, Vector};
use sl_types::map::{Direction, RegionCoordinates};

use crate::chat::{DEBUG_CHANNEL, FULLY_AUDIBLE, Line};
use crate::runtime::GridCore;
use crate::scene_store::ObjectStore;
use crate::world::{AvatarIdentity, RegionChange, RegionUpdate, SceneFixtures};

/// The [`RegionUpdate::source`] of a change the region made itself rather
/// than a session — a script's line, an object a script removed. Session
/// numbers start at one, so no session's watcher takes it for its own.
pub const REGION_SOURCE: u64 = 0;

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
/// default it.
const DEFAULT_DAY_LENGTH: f32 = 14_400.0;

/// The edge of a region, in metres.
const REGION_EDGE_M: u32 = 256;

/// How many ticks a program an upload compiled waits for the saved script's
/// instance to take it. The upload's answer and the item's new asset are two
/// steps of the session, a tick may fall between them, and a save whose item
/// never takes the program (an upload to an item the prim does not hold)
/// must not keep it for good.
const PROGRAM_HANDOFF_TICKS: u32 = 100;

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
}

/// A script arriving in the region with the object it is in.
#[derive(Debug, Clone)]
struct Arriving {
    /// What it saved when its object was taken, or [`None`] to start it
    /// afresh.
    saved: Option<SavedScript>,
    /// How long its object was away.
    away: Duration,
    /// The start parameter its object was rezzed with.
    start_parameter: i32,
}

impl Default for RegionScripts {
    /// No scripts, Second Life's engine, the default heartbeat step.
    fn default() -> Self {
        Self::new(crate::heartbeat::DEFAULT_STEP, EngineFlavour::SecondLife, 1)
    }
}

impl RegionScripts {
    /// No scripts yet, an engine of `flavour` ticking every `step`, and the
    /// random draws seeded with `seed`.
    #[must_use]
    pub(crate) fn new(step: Duration, flavour: EngineFlavour, seed: u64) -> Self {
        Self {
            engine: Engine::new(EngineConfig {
                flavour,
                ..EngineConfig::for_step(step)
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
        }
    }

    /// The engine running the region's scripts.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
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
        self.engine
            .instance_mut(id)
            .map(|instance| instance.set_running(running))
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
    /// starts it afresh, and raises `on_rez(start_parameter)` in it.
    pub(crate) fn arrive(
        &mut self,
        prim: ObjectKey,
        item: InventoryKey,
        saved: Option<SavedScript>,
        away: Duration,
        start_parameter: i32,
    ) {
        let _previous = self.arriving.insert(
            (prim, item),
            Arriving {
                saved,
                away,
                start_parameter,
            },
        );
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
        let heard = Heard {
            channel: line.channel.0,
            name: line.from_name.clone(),
            speaker: line.source.source_id().to_string(),
            message: line.message.clone(),
        };
        let in_earshot: Vec<CallerId> = self
            .slots
            .iter()
            .filter(|(_id, slot)| {
                crate::world::listening_position(objects, slot.prim)
                    .is_some_and(|position| line.reaches_listen_at(slot.prim, &position))
            })
            .map(|(id, _slot)| *id)
            .collect();
        for id in in_earshot {
            self.offer(id, &heard);
        }
    }

    /// Offer `heard` to the instance `id`.
    fn offer(&mut self, id: CallerId, heard: &Heard) {
        if let Err(error) = self.engine.hear(id, heard) {
            tracing::warn!(?id, %error, "a line could not be offered to a script");
        }
    }

    /// Take the instance `id` out of the region.
    fn remove(&mut self, id: CallerId) {
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
            match self.engine.rez_saved(
                id,
                Arc::clone(&program),
                saved,
                arriving.away,
                arriving.start_parameter,
            ) {
                Ok(_posted) => Some(()),
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
            }
            if let Some(arriving) = arriving
                && let Err(error) = self.engine.rez(id, arriving.start_parameter)
            {
                tracing::warn!(?id, %error, "on_rez could not be raised");
            }
        }
        let _was = self.by_item.insert((slot.prim, slot.item), id);
        let _old = self.slots.insert(id, slot);
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
}

/// What a tick's scripts did that the region applies under its lock, once
/// the engine is back in place.
#[derive(Debug, Clone)]
enum Effect {
    /// A line said: what the avatars and the agents' listens hear, and what
    /// the scripts in earshot are offered, where any are.
    Say {
        /// The line as the avatars hear it.
        avatars: Line,
        /// The line as the scripts hear it, or [`None`] when no script does.
        scripts: Option<Line>,
    },
    /// A line offered to these scripts alone (`llRegionSayTo`).
    Hear {
        /// The scripts.
        to: Vec<CallerId>,
        /// The line.
        heard: Heard,
    },
    /// The linkset whose root is this prim leaves the region (`llDie`).
    Die(RegionLocalObjectId),
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

impl RegionHost<'_> {
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
        self.world
            .script_instances
            .slots
            .iter()
            .filter(|(_id, slot)| slot.prim == prim)
            .map(|(id, _slot)| *id)
            .collect()
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
        let root = self.world.objects.get(origin.root)?;
        (root.parent_id.0 != 0 && !self.world.objects.contains(root.parent_id))
            .then(|| AgentKey::from(root.owner_id))
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

    fn say(&mut self, caller: CallerId, speech: &Speech) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
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
        let scripts = speech.to_scripts.map(|volume| Line {
            chat_type: chat_type_of(volume),
            ..avatars.clone()
        });
        self.effects.push(Effect::Say { avatars, scripts });
    }

    fn say_to(&mut self, caller: CallerId, target: &str, channel: i32, text: &str) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let Ok(key) = uuid::Uuid::parse_str(target) else {
            return;
        };
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
            let to = self.scripts_in(ObjectKey::from(key));
            self.effects.push(Effect::Hear { to, heard });
            return;
        }
        if self.present(target).is_none() {
            return;
        }
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
            return;
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
        self.effects.push(Effect::Hear { to, heard });
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
            };
        };
        let prims = self.linkset_prims(caller);
        let alone = prims.len() <= 1;
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
                Some(LinkedPrim {
                    link,
                    scripts: self.scripts_in(key),
                })
            })
            .collect();
        Links {
            this,
            prims: linked,
        }
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
        self.effects.push(Effect::Say {
            avatars,
            scripts: None,
        });
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

    fn random_bits(&mut self, _caller: CallerId) -> u32 {
        self.world.script_instances.draw()
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

    fn seated_on(&self, _caller: CallerId, _agent: &str) -> bool {
        // Sitting is not yet world state (`server-world-sit-and-attach`).
        false
    }

    fn ask_permissions(&mut self, caller: CallerId, question: &Question) -> bool {
        self.put_question(caller, &question.agent, question.mask, None)
    }

    fn give_money(&mut self, caller: CallerId, payment: &Payment) {
        // The grid keeps no balances yet (`server-fake-grid-money-ledger`).
        tracing::info!(
            ?caller,
            to = payment.to,
            amount = payment.amount,
            "a script paid out money, which the fake grid does not move"
        );
    }

    fn transfer_money(&mut self, _caller: CallerId, payment: &Payment) -> Result<(), MoneyRefusal> {
        // Only a payee with no account is refused: the grid keeps no balances
        // to refuse an owner short of the amount (`server-fake-grid-money-ledger`).
        let known = uuid::Uuid::parse_str(&payment.to)
            .ok()
            .and_then(|key| self.core.account_by_agent(AgentKey::from(key)))
            .is_some();
        if known {
            Ok(())
        } else {
            Err(MoneyRefusal::NoSuchEntity)
        }
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

    fn experience(&self, caller: CallerId) -> Option<ScriptExperience> {
        if self.world.script_instances.flavour().is_opensim() {
            return None;
        }
        let slot = self.slot(caller)?;
        let experience = self
            .world
            .script_experience(slot.prim, slot.item)
            .flatten()?;
        Some(ScriptExperience {
            key: experience.uuid().to_string(),
            standing: Ok(()),
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

    fn sit_on_link(
        &mut self,
        _caller: CallerId,
        agent: &str,
        _link: i32,
    ) -> Result<(), SitRefusal> {
        // Sitting is not yet world state (`server-world-sit-and-attach`).
        if self.present(agent).is_none() {
            return Err(SitRefusal::InvalidAgent);
        }
        Err(SitRefusal::NoSitTarget)
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
        DEFAULT_DAY_LENGTH
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
}

/// The avatars standing in the region at `index`, in session order.
async fn presence(core: &GridCore, index: usize) -> Vec<Present> {
    let mut sessions: Vec<_> = core
        .sessions
        .lock()
        .await
        .iter()
        .map(|(seq, shared)| (*seq, shared.clone()))
        .collect();
    sessions.sort_unstable_by_key(|(seq, _shared)| *seq);
    let mut present = Vec::new();
    for (_seq, shared) in sessions {
        if shared.is_closed() {
            continue;
        }
        let here = shared
            .with_state(|state| {
                (state.region == index && state.sim.is_root_agent()).then(|| {
                    let (allowed, blocked) = state.sim.experiences().agent_permissions();
                    Present {
                        agent: state.avatar.agent_id,
                        allowed,
                        blocked,
                    }
                })
            })
            .await;
        present.extend(here);
    }
    present
}

/// One tick of the region's scripts: what it ran, or [`None`] for a region
/// the grid does not have or that has no script.
pub(crate) async fn run_tick(core: &GridCore, index: usize) -> Option<TickReport> {
    let region = core.region(index)?;
    let scripted = {
        let mut world = region.world.lock();
        reconcile(&mut world, &core.assets);
        !world.script_instances.is_empty()
    };
    if !scripted {
        return None;
    }
    let present = presence(core, index).await;
    let (report, changes, deliveries) = {
        let mut world = region.world.lock();
        tick_locked(core, index, &mut world, &present)
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

/// Runs one engine tick over `world` and applies to it what the scripts did,
/// returning the tick's report, the changes the region's sessions are to be
/// told and what is to be sent to viewers.
fn tick_locked(
    core: &GridCore,
    index: usize,
    world: &mut SceneFixtures,
    present: &[Present],
) -> (TickReport, Vec<RegionChange>, Vec<Delivery>) {
    let config = *world.script_instances.engine.config();
    let mut engine = core::mem::replace(&mut world.script_instances.engine, Engine::new(config));
    let mut host = RegionHost {
        core,
        index,
        world: &mut *world,
        stores: core.experience_stores.lock(),
        present,
        effects: Vec::new(),
        deliveries: Vec::new(),
    };
    let report = engine.tick(&mut host);
    let effects = core::mem::take(&mut host.effects);
    let deliveries = core::mem::take(&mut host.deliveries);
    drop(host);
    world.script_instances.engine = engine;
    world.script_instances.last_load = (report.runnable, report.served);
    let mut changes = Vec::new();
    for effect in effects {
        match effect {
            Effect::Say { avatars, scripts } => {
                if let Some(line) = scripts {
                    world.script_instances.hear(&line, &world.objects);
                }
                changes.push(world.say_to_avatars(avatars));
            }
            Effect::Hear { to, heard } => {
                for id in to {
                    world.script_instances.offer(id, &heard);
                }
            }
            Effect::Die(root) => changes.extend(remove_linkset(world, root)),
        }
    }
    (report, changes, deliveries)
}

/// Takes the linkset whose root is `root` out of the region with its
/// scripts, children first: a `KillObject` for each prim.
fn remove_linkset(world: &mut SceneFixtures, root: RegionLocalObjectId) -> Vec<RegionChange> {
    let mut prims = crate::link_sets::children_in_order(world, root);
    prims.push(root);
    let mut changes = Vec::with_capacity(prims.len());
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
async fn deliver(core: &GridCore, index: usize, deliveries: Vec<Delivery>) {
    for delivery in deliveries {
        let agent = match &delivery {
            Delivery::Chat { agent, .. }
            | Delivery::Dialog { agent, .. }
            | Delivery::Question { agent, .. }
            | Delivery::LoadUrl { agent, .. }
            | Delivery::MapDestination { agent, .. }
            | Delivery::Message { agent, .. } => *agent,
        };
        let Some(shared) = core.root_session_of(agent).await else {
            tracing::debug!(%agent, "a script spoke to an avatar no longer in world");
            continue;
        };
        let sent = shared
            .with_state(|state| {
                let now = shared.now();
                if state.region != index && !matches!(delivery, Delivery::Message { .. }) {
                    return Ok(());
                }
                send(&mut state.sim, &delivery, now)
            })
            .await;
        if let Err(error) = sent {
            tracing::warn!(%agent, %error, "sending what a script said failed");
        }
    }
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
    }
}

/// Answers one drained [`ServerEvent`] if it is a command to a script —
/// reset it, answer or revoke its permissions, reply to its dialog, ask its
/// pay layout — returning what the region's others are to be told. `None`
/// for anything else.
pub(crate) fn answer_script_command(
    world: &mut SceneFixtures,
    identity: &AvatarIdentity,
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
            let engine = &mut world.script_instances.engine;
            let answered = match engine.answer(id, &agent, permissions.0) {
                Ok(Some(_posted)) => Ok(true),
                Ok(None) => {
                    let answer = if permissions.0 == 0 {
                        Err(ExperienceError::NotPermitted)
                    } else {
                        Ok(())
                    };
                    engine
                        .answer_experience(id, &agent, answer)
                        .map(|posted| posted.is_some())
                }
                Err(error) => Err(error),
            };
            match answered {
                Ok(true) => {}
                Ok(false) => tracing::debug!(?id, "an answer to a question the script did not put"),
                Err(error) => {
                    tracing::warn!(?id, %error, "a permission answer could not be posted");
                }
            }
            Some(Vec::new())
        }
        ServerEvent::RevokeScriptPermissions {
            object_id,
            permissions,
        } => {
            let ids: Vec<CallerId> = world
                .script_instances
                .slots
                .iter()
                .filter(|(_id, slot)| slot.prim == *object_id)
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
        // prim no script set a layout for is not answered.
        ServerEvent::RequestPayPrice { object_id } => {
            let flavour = world.script_instances.flavour();
            if let Some(layout) = world
                .script_instances
                .pay_price(*object_id)
                .and_then(|layout| layout.reply(flavour))
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
