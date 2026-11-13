//! The VM against the acceptance of `server-lsl-vm-execution`: a runaway
//! loop cannot stall the region, a sleep resumes on its tick, a stopped
//! script keeps its globals, and an error stops only its own script.

use std::sync::Arc;

use core::time::Duration;

use pretty_assertions::assert_eq;

use super::*;
use crate::bytecode::{Position, StateId};
use crate::compile;
use sl_lsl::ast::TypeName;
use sl_types::chat::ChatVolume;

use crate::library::{BuiltinId, Event, event};
use crate::value::Value;

mod avatars;
mod clocks;
mod comms;
mod contents;
mod experiences;
mod floods;
mod http;
mod identity;
mod inventory;
mod json;
mod land;
mod latency;
mod linking;
mod linkset;
mod math;
mod memory;
mod money;
mod ossl;
mod persistence;
mod placement;
mod precision;
mod prim_state;
mod sensing;
mod sitting;
mod strings;
mod targets;
mod throttles;
mod timers;

/// A tenth of a second: ten ticks to the second, so `llSleep(2.0)` is twenty.
const STEP: Duration = Duration::from_millis(100);

/// A camera rule's value as the runner notes it: as short as it reads.
fn camera_number(value: f32) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0")
        .map_or_else(|| text.clone(), str::to_owned)
}

/// An avatar of a test region and what scripts did to it.
#[derive(Debug, Clone)]
pub(super) struct TestAvatar {
    /// What a script reads of it, but for what it plays.
    pub(super) agent: crate::vm::avatars::Agent,
    /// What scripts started on it, by asset.
    pub(super) started: Vec<String>,
    /// Its animation overrides, by state: the name given and the asset.
    pub(super) overrides: std::collections::BTreeMap<String, (String, String)>,
    /// Where its camera is and how it is turned.
    pub(super) camera: (sl_types::lsl::Vector, sl_types::lsl::Rotation),
}

impl TestAvatar {
    /// A standing avatar named `First Last`.
    pub(super) fn standing(legacy_name: &str) -> Self {
        let username = legacy_name.replace(' ', ".").to_lowercase();
        Self {
            agent: crate::vm::avatars::Agent {
                legacy_name: legacy_name.to_owned(),
                display_name: legacy_name.to_owned(),
                username,
                size: sl_types::lsl::Vector {
                    x: 0.45,
                    y: 0.6,
                    z: 1.9,
                },
                info: 0,
                language: "en-us".to_owned(),
                state: "Standing".to_owned(),
                animations: Vec::new(),
            },
            started: Vec::new(),
            overrides: std::collections::BTreeMap::new(),
            camera: (
                sl_types::lsl::Vector {
                    x: 128.0,
                    y: 124.0,
                    z: 27.0,
                },
                // Looking north, as a camera behind an avatar facing north.
                sl_types::lsl::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: core::f32::consts::FRAC_1_SQRT_2,
                    s: core::f32::consts::FRAC_1_SQRT_2,
                },
            ),
        }
    }

    /// What plays on it: its state's animation (overridden or not) and what
    /// scripts started.
    fn playing(&self) -> Vec<String> {
        let state = self.overrides.get(&self.agent.state).map_or_else(
            || {
                sl_anim::registry::builtin_animation_by_name(crate::vm::avatars::default_animation(
                    &self.agent.state,
                ))
                .map(|builtin| builtin.id.to_string())
                .unwrap_or_default()
            },
            |(_, asset)| asset.clone(),
        );
        core::iter::once(state)
            .chain(self.started.iter().cloned())
            .collect()
    }
}

/// A request waiting on a script's answer in a test region: the sender (a
/// script and its request's key, or [`None`] from outside), the method, and
/// the tick it arrived.
pub(super) type Waiting = (Option<(CallerId, String)>, String, u64);

/// The HTTP of a test region: the URLs granted, and the requests and
/// answers waiting to be routed after the tick
/// ([`comms::Region::route_http`]).
#[derive(Debug, Default)]
pub(super) struct HttpDesk {
    /// The flavour URLs are spelled for.
    pub(super) flavour: EngineFlavour,
    /// The `host:port` a URL is reached at.
    pub(super) authority: String,
    /// The `host:port` a secure URL is reached at.
    pub(super) secure_authority: String,
    /// Whether no secure URL is granted (no HTTPS listener).
    pub(super) no_secure: bool,
    /// How many URLs the land allows.
    pub(super) allowance: i32,
    /// The address an arriving request comes from.
    pub(super) remote: String,
    /// The URLs granted, by key: the script and the URL.
    pub(super) urls: std::collections::BTreeMap<String, (CallerId, String)>,
    /// Requests sent and not yet routed.
    pub(super) sent: Vec<(CallerId, crate::vm::http::Outbound)>,
    /// Answers given and not yet routed: the answering script, the request.
    pub(super) answered: Vec<(CallerId, String, crate::vm::http::Answered)>,
    /// Requests that reached a script and are not answered, by their key:
    /// the sender (a script and its request, or [`None`] from outside), the
    /// method, and the tick they arrived.
    pub(super) waiting: std::collections::BTreeMap<String, Waiting>,
    /// Answers to requests from outside, in order: the request's path and
    /// what came back.
    pub(super) fetched: Vec<(String, crate::vm::http::Answered)>,
    /// How many requests have arrived, for their keys.
    pub(super) arrivals: u64,
    /// Each prim's throttle, by number.
    pub(super) throttles: std::collections::BTreeMap<u64, crate::vm::http::Throttle>,
}

/// A host that records what scripts print, with the tick they printed at.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag rigs one independent answer of the host"
)]
struct Recorder {
    /// The tick being run.
    now: u64,
    /// `(tick, caller, text)` per `print`.
    printed: Vec<(u64, CallerId, String)>,
    /// Stub notices.
    stubbed: Vec<(CallerId, BuiltinId)>,
    /// `(tick, caller)` per state left.
    left: Vec<(u64, CallerId)>,
    /// The scripts `llGetScriptState` can name, all in one prim.
    scripts: std::collections::BTreeMap<String, CallerId>,
    /// `(tick, caller, speech)` per line said.
    said: Vec<(u64, CallerId, Speech)>,
    /// The object each script is in, by key; `"object-a"` when not listed.
    objects: std::collections::BTreeMap<CallerId, String>,
    /// `(tick, caller, text)` per line told the owner alone.
    told: Vec<(u64, CallerId, String)>,
    /// The state of the xorshift the random draws come from.
    draws: u32,
    /// The last scene id minted, from OpenSim's first local id, 7200.
    scene_ids: i32,
    /// The linkset each script is in; linkset 0 when not listed.
    linksets: std::collections::BTreeMap<CallerId, u64>,
    /// Each linkset's data store.
    stores: std::collections::BTreeMap<u64, LinksetData>,
    /// `(tick, caller, target, channel, text)` per `llRegionSayTo`.
    said_to: Vec<(u64, CallerId, String, i32, String)>,
    /// Every `DEBUG_CHANNEL` line, said or told one avatar, in order.
    debug: Vec<String>,
    /// `(tick, caller, message)` per instant message.
    messages: Vec<(u64, CallerId, InstantMessage)>,
    /// `(tick, caller, dialog)` per dialog or text box put.
    dialogs: Vec<(u64, CallerId, Dialog)>,
    /// `(caller, offer)` per `llLoadURL`.
    offers: Vec<(CallerId, LoadUrl)>,
    /// `(caller, destination)` per `llMapDestination`.
    destinations: Vec<(CallerId, MapDestination)>,
    /// The avatar wearing each script's object; none when not listed.
    worn: std::collections::BTreeMap<CallerId, String>,
    /// The prim each script is in, by number; a prim of its own (its id)
    /// when not listed. Every prim is linked to nothing.
    prims: std::collections::BTreeMap<CallerId, u64>,
    /// Each prim's key, by number; one fixed key when not listed.
    prim_keys: std::collections::BTreeMap<u64, String>,
    /// The owner of every script's object.
    owner: Option<String>,
    /// The avatar sitting on each script's object; none when not listed.
    seated: std::collections::BTreeMap<CallerId, String>,
    /// The avatars not in the region; every other key is.
    away: std::collections::BTreeSet<String>,
    /// `(tick, caller, question)` per permission question put.
    questions: Vec<(u64, CallerId, Question)>,
    /// `(tick, caller, payment)` per `llGiveMoney` handed over.
    given: Vec<(u64, CallerId, Payment)>,
    /// `(tick, caller, payment)` per `llTransferLindenDollars` that moved.
    transferred: Vec<(u64, CallerId, Payment)>,
    /// The balance of every avatar with an account, by key; a transfer to
    /// any other key names nobody.
    accounts: std::collections::BTreeMap<String, i64>,
    /// Each prim's pay layout, by number.
    pay_prices: std::collections::BTreeMap<u64, PayPrice>,
    /// What each prim's scripts last set `llPassTouches` to, by number.
    passes: std::collections::BTreeMap<u64, i32>,
    /// What each prim's scripts last set `llPassCollisions` to, by number.
    collision_passes: std::collections::BTreeMap<u64, i32>,
    /// Each `llMoveToTarget` and `llStopMoveToTarget` handed on, in order.
    move_targets: Vec<(CallerId, Option<crate::vm::prim::MoveTarget>)>,
    /// Each `llSetBuoyancy` handed on, in order.
    buoyancies: Vec<(CallerId, f32)>,
    /// The experience each script was compiled into; none when not listed.
    experiences: std::collections::BTreeMap<CallerId, ScriptExperience>,
    /// The experiences the grid knows, by key; an id not listed is no
    /// experience's.
    catalogue: std::collections::BTreeMap<String, ExperienceDetails>,
    /// The ids the region has looked up: an id answers empty until it is
    /// here, and is here once asked.
    looked_up: std::collections::BTreeSet<String>,
    /// `(agent, experience)` an avatar allowed (`true`) or blocked.
    admissions: std::collections::BTreeMap<(String, String), bool>,
    /// `(tick, caller, question)` per experience question put.
    experience_questions: Vec<(u64, CallerId, ExperienceQuestion)>,
    /// Each experience's key-value store, by key.
    key_values: std::collections::BTreeMap<String, KeyValueStore>,
    /// Whether the region's key-value store is off.
    store_disabled: bool,
    /// `(caller, agent, link)` per avatar seated by `llSitOnLink`.
    sat: Vec<(CallerId, String, i32)>,
    /// What the owner is in the region, but for the owner's key, which is
    /// [`Host::owner`]'s.
    standing: Standing,
    /// The simulator and grid.
    simulator: Simulator,
    /// The region's map texture (`osGetMapTexture`), if one is rigged.
    map_texture: Option<String>,
    /// The HTTP the scripts did: URLs, requests and answers, which the
    /// region routes between them after each tick.
    http: HttpDesk,
    /// The avatars in the region, by key, and what the scripts did to them.
    avatars: std::collections::BTreeMap<String, TestAvatar>,
    /// What the scripts sent avatars' viewers, in the runner's words
    /// (`# script-control-change …`, `# follow-cam …`).
    avatar_notes: Vec<String>,
    /// The avatar whose controls each script took.
    controls_taken: std::collections::BTreeMap<CallerId, String>,
    /// Each scripted teleport: the avatar and where to.
    teleports: Vec<(String, crate::vm::avatars::Destination)>,
    /// The ground's height everywhere in the region.
    ground: f32,
    /// Each avatar's address, by key.
    addresses: std::collections::BTreeMap<String, String>,
    /// `(tick, caller)` per `llDie` the host agreed to.
    died: Vec<(u64, CallerId)>,
    /// `(caller, fault)` per run-time error handed over.
    faults: Vec<(CallerId, Fault)>,
    /// Each prim's seat settings, by number.
    seats: std::collections::BTreeMap<u64, crate::vm::seating::Seat>,
    /// The avatars sitting on every script's object, in the order they sat.
    sitters: Vec<String>,
    /// The avatar on each prim's sit target, by number.
    on_target: std::collections::BTreeMap<u64, String>,
    /// `(caller, agent)` per `llUnSit`.
    unsat: Vec<(CallerId, String)>,
    /// `(caller, sit, text)` per pie text set.
    pie_texts: Vec<(CallerId, bool, String)>,
    /// The point each script's object is worn on; not worn when not listed.
    points: std::collections::BTreeMap<CallerId, i32>,
    /// `(caller, point)` per `llAttachToAvatar` handed over.
    attached_to: Vec<(CallerId, i32)>,
    /// The callers whose objects were taken off.
    detached: Vec<CallerId>,
    /// Where each script's prim stands; at the origin when not listed.
    placements: std::collections::BTreeMap<CallerId, crate::vm::seating::Placement>,
    /// `(caller, local)` per `llSetPos` handed over; each also becomes the
    /// prim's local and region position.
    moves: Vec<(CallerId, sl_types::lsl::Vector)>,
    /// How many prims each object has, by a prim's key.
    prim_counts: std::collections::BTreeMap<String, i32>,
    /// The linkset every script is in, where a test states one in full;
    /// otherwise each script's prim is a lone one.
    linked: Option<Links>,
    /// The keys `llCreateLink` may link; every other is an invalid target.
    linkable: std::collections::BTreeSet<String>,
    /// `(caller, target, parent)` per `llCreateLink` handed over.
    created_links: Vec<(CallerId, String, bool)>,
    /// `(caller, prim)` per `llBreakLink` handed over.
    broken_links: Vec<(CallerId, String)>,
    /// The callers per `llBreakAllLinks` handed over.
    broken_all: Vec<CallerId>,
    /// `(caller, allow)` per `llAllowInventoryDrop` handed over.
    allowed_drops: Vec<(CallerId, bool)>,
    /// Each prim's state, by key; a prim not listed is a fresh box.
    prim_states: std::collections::BTreeMap<String, crate::vm::prim::Prim>,
    /// `(prim key, bits)` per `changed` a write raised, in order.
    changes: Vec<(String, i32)>,
    /// `(caller, prim key, cue)` per sound played or preloaded.
    sounds: Vec<(CallerId, String, crate::vm::prim::SoundCue)>,
    /// The asset key of each item of the scripts' prims, by name and type.
    inventory: std::collections::BTreeMap<(String, i32), String>,
    /// What `llGetObjectDetails` reads of each key.
    details: std::collections::BTreeMap<String, crate::vm::prim::ObjectDetails>,
    /// The land impact and the streaming, physics and server costs of every
    /// prim; Second Life's of a half-metre cube when not stated.
    detail_costs: Option<(f32, f32, f32, f32)>,
    /// Whether a move by `llSetPos` raises OpenSim's `changed(32768)`, as
    /// the fake grid's move path does on an OpenSim region.
    moves_heard: bool,
    /// Whether a prim's last owner reads as its owner (OpenSim) rather than
    /// `NULL_KEY` (Second Life, for a prim nobody else owned).
    last_owner_is_owner: bool,
    /// Each prim's contents, by number. A prim listed here names its
    /// scripts by them (`llGetScriptState`); one not listed holds nothing,
    /// and its scripts are named by [`Self::scripts`].
    contents: std::collections::BTreeMap<u64, Vec<crate::vm::Item>>,
    /// The inventory name of each script instance of a prim listed in
    /// [`Self::contents`].
    names: std::collections::BTreeMap<CallerId, String>,
    /// The text of each notecard, by its asset key.
    notecards: std::collections::BTreeMap<String, String>,
    /// The notecards the region has read, by asset key.
    notecards_read: std::collections::BTreeSet<String>,
    /// `(caller, name, changed)` per item removed.
    removed: Vec<(CallerId, String, Vec<i32>)>,
    /// `(caller, target, items)` per give into a prim.
    gifts: Vec<(CallerId, String, Vec<String>)>,
    /// `(caller, offer)` per offer put to an avatar.
    offers_made: Vec<(CallerId, crate::vm::Offer)>,
    /// Each prim's remote script access pin, by number.
    pins: std::collections::BTreeMap<u64, i32>,
    /// `(caller, load)` per script loaded into another prim, which the test's
    /// region starts.
    loads: Vec<(CallerId, crate::vm::RemoteLoad)>,
    /// `(caller, rez, key)` per object rezzed, which the test's region puts
    /// in place: the key is the new prim's, numbered from 100.
    rezzes: Vec<(CallerId, crate::vm::Rez, String)>,
    /// The number of the last prim a rez made.
    last_rezzed: u64,
    /// The prims rezzed and still there, by key, with the prim of the script
    /// that rezzed each.
    rezzed: std::collections::BTreeMap<String, u64>,
    /// The key of the object that rezzed each prim, by key.
    rezzers: std::collections::BTreeMap<String, String>,
    /// The parcel everywhere in the region; no parcel when unset.
    parcel: Option<crate::vm::land::Parcel>,
    /// The region.
    region: crate::vm::land::Region,
    /// What `llRequestSimulatorData` reads of the grid's regions, by name in
    /// lower case.
    sims: std::collections::BTreeMap<String, crate::vm::land::SimData>,
    /// The wind everywhere; still air when unset.
    wind: Option<sl_types::lsl::Vector>,
    /// The changes made to the land, in order.
    land_changes: Vec<crate::vm::land::LandChange>,
    /// The region's statistics.
    sim_stats: Vec<f32>,
    /// The parcel at the region's west edge, where it is not [`Self::parcel`].
    edge_parcel: Option<crate::vm::land::Parcel>,
    /// The ground's height at the region's west, east, south and north edges,
    /// where it is not [`Self::ground`].
    edges: Option<[f32; 4]>,
    /// Each prim's name as an error line and an offer give it, by number;
    /// `Faulty prim` when not listed.
    prim_names: std::collections::BTreeMap<u64, String>,
    /// The region and the position every prim's whereabouts give; the local
    /// grid's when unset.
    at: Option<(String, sl_types::lsl::Vector)>,
    /// Whether a script loaded over one of its name raises two
    /// `CHANGED_INVENTORY`, the remove and the add, as OpenSim's
    /// `RezScriptFromPrim` does; one when not.
    replace_raises_twice: bool,
    /// The region's calendar time at tick 0, in microseconds since the epoch.
    epoch_micros: i64,
    /// The offset from UTC of the simulator host's time zone, in seconds.
    local_offset: i32,
    /// Every object and avatar a sweep weighs.
    sensed: Vec<crate::vm::sensing::Sensed>,
    /// What every ray hits, nearest its start first.
    ray_hits: Vec<crate::vm::sensing::RayHit>,
}

/// The owner of every object a [`Recorder`] has, unless it says otherwise.
const OWNER: &str = "0a0a0a0a-0000-4000-8000-000000000001";

impl Host for Recorder {
    fn print(&mut self, caller: CallerId, text: &str) {
        self.printed.push((self.now, caller, text.to_owned()));
    }

    fn stubbed(&mut self, caller: CallerId, id: BuiltinId) {
        self.stubbed.push((caller, id));
    }

    fn left_state(&mut self, caller: CallerId) {
        self.left.push((self.now, caller));
    }

    fn die(&mut self, caller: CallerId) -> bool {
        if self.worn.contains_key(&caller) {
            return false;
        }
        self.died.push((self.now, caller));
        true
    }

    fn script_named(&self, caller: CallerId, name: &str) -> Option<CallerId> {
        let prim = self.prim_of(caller);
        let Some(items) = self.contents.get(&prim) else {
            return self.scripts.get(name).copied();
        };
        let _item = items
            .iter()
            .find(|item| item.name == name && item.kind == crate::vm::contents::INVENTORY_SCRIPT)?;
        // A script with no instance reads as stopped: it gets an id no
        // instance has.
        Some(
            self.names
                .iter()
                .find(|(id, script)| script.as_str() == name && self.prim_of(**id) == prim)
                .map_or(CallerId(u64::MAX), |(id, _)| *id),
        )
    }

    fn say(&mut self, caller: CallerId, speech: &Speech) -> Option<Overheard> {
        if speech.channel == DEBUG_CHANNEL {
            self.debug.push(speech.text.clone());
        }
        self.said.push((self.now, caller, speech.clone()));
        None
    }

    fn owner_say(&mut self, caller: CallerId, text: &str) {
        self.told.push((self.now, caller, text.to_owned()));
    }

    fn faulted(&mut self, caller: CallerId, fault: &Fault) {
        self.faults.push((caller, fault.clone()));
    }

    fn say_to(
        &mut self,
        caller: CallerId,
        target: &str,
        channel: i32,
        text: &str,
    ) -> Option<Overheard> {
        if channel == DEBUG_CHANNEL {
            self.debug.push(text.to_owned());
        }
        self.said_to.push((
            self.now,
            caller,
            target.to_owned(),
            channel,
            text.to_owned(),
        ));
        None
    }

    fn instant_message(&mut self, caller: CallerId, message: &InstantMessage) {
        self.messages.push((self.now, caller, message.clone()));
    }

    fn dialog(&mut self, caller: CallerId, dialog: &Dialog) {
        self.dialogs.push((self.now, caller, dialog.clone()));
    }

    fn load_url(&mut self, caller: CallerId, offer: &LoadUrl) {
        self.offers.push((caller, offer.clone()));
    }

    fn map_destination(&mut self, caller: CallerId, destination: &MapDestination) {
        self.destinations.push((caller, destination.clone()));
    }

    fn worn_by(&self, caller: CallerId) -> Option<String> {
        self.worn.get(&caller).cloned()
    }

    fn links(&self, caller: CallerId) -> Links {
        let prim = self.prim_of(caller);
        let mut scripts: Vec<CallerId> = self
            .prims
            .iter()
            .filter(|(_, in_prim)| **in_prim == prim)
            .map(|(id, _)| *id)
            .collect();
        if !scripts.contains(&caller) {
            scripts.push(caller);
        }
        scripts.sort();
        if let Some(linked) = &self.linked {
            return linked.clone();
        }
        // A lone prim with an avatar on it is the root of a linkset.
        let link = i32::from(!self.sitters.is_empty());
        Links {
            this: link,
            prims: vec![LinkedPrim {
                link,
                key: self.prim_key(prim),
                name: self
                    .prim_states
                    .get(&self.prim_key(prim))
                    .map_or_else(|| "Object".to_owned(), |state| state.name.clone()),
                scripts,
            }],
            sitters: self
                .sitters
                .iter()
                .map(|key| LinkedAvatar {
                    key: key.clone(),
                    name: format!("Sitter {}", key.get(..8).unwrap_or(key)),
                })
                .collect(),
        }
    }

    fn create_link(
        &mut self,
        caller: CallerId,
        target: &str,
        parent: bool,
    ) -> Result<(), InvalidLinkTarget> {
        if !self.linkable.contains(target) {
            return Err(InvalidLinkTarget);
        }
        self.created_links.push((caller, target.to_owned(), parent));
        Ok(())
    }

    fn break_link(&mut self, caller: CallerId, prim: &str) {
        self.broken_links.push((caller, prim.to_owned()));
    }

    fn break_all_links(&mut self, caller: CallerId) {
        self.broken_all.push(caller);
    }

    fn allow_inventory_drop(&mut self, caller: CallerId, allow: bool) {
        self.allowed_drops.push((caller, allow));
    }

    fn whereabouts(&self, caller: CallerId) -> Whereabouts {
        Whereabouts {
            object: self
                .objects
                .get(&caller)
                .cloned()
                .unwrap_or_else(|| "object-a".to_owned()),
            prim: self.prim_key(self.prim_of(caller)),
            name: self
                .prim_names
                .get(&self.prim_of(caller))
                .cloned()
                .unwrap_or_else(|| "Faulty prim".to_owned()),
            script: self.names.get(&caller).cloned().unwrap_or_else(|| {
                self.scripts
                    .iter()
                    .find(|(_, id)| **id == caller)
                    .map_or_else(|| format!("Script {}", caller.0), |(name, _)| name.clone())
            }),
            item: "d9cbb4b7-206e-45c2-8c9a-7429643962ac".to_owned(),
            region: self
                .at
                .as_ref()
                .map_or_else(|| "Default Region".to_owned(), |(region, _)| region.clone()),
            position: self.at.as_ref().map_or(
                sl_types::lsl::Vector {
                    x: 120.0,
                    y: 128.0,
                    z: 27.5,
                },
                |(_, position)| position.clone(),
            ),
        }
    }

    fn random_bits(&mut self, _caller: CallerId) -> u32 {
        // xorshift32 from a fixed seed: a region's draws are a function of
        // its seed.
        let mut state = if self.draws == 0 {
            0x9E37_79B9
        } else {
            self.draws
        };
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        self.draws = state;
        state
    }

    fn mint_scene_id(&mut self) -> i32 {
        self.scene_ids = self.scene_ids.max(7200).saturating_add(1);
        self.scene_ids
    }

    fn linkset(&self, caller: CallerId) -> LinksetId {
        LinksetId(self.linksets.get(&caller).copied().unwrap_or(0))
    }

    fn linkset_data(&mut self, caller: CallerId) -> &mut LinksetData {
        let linkset = self.linkset(caller).0;
        self.stores.entry(linkset).or_default()
    }

    fn owner(&self, _caller: CallerId) -> String {
        self.owner.clone().unwrap_or_else(|| OWNER.to_owned())
    }

    fn seated_on(&self, caller: CallerId, agent: &str) -> bool {
        self.seated
            .get(&caller)
            .is_some_and(|sitter| sitter.eq_ignore_ascii_case(agent))
            || self
                .sitters
                .iter()
                .any(|sitter| sitter.eq_ignore_ascii_case(agent))
    }

    fn ask_permissions(&mut self, caller: CallerId, question: &Question) -> bool {
        if self.away.contains(&question.agent) {
            return false;
        }
        self.questions.push((self.now, caller, question.clone()));
        true
    }

    fn give_money(&mut self, caller: CallerId, payment: &Payment) {
        self.given.push((self.now, caller, payment.clone()));
    }

    fn transfer_money(&mut self, caller: CallerId, payment: &Payment) -> Result<(), MoneyRefusal> {
        if !self.accounts.contains_key(&payment.to) {
            return Err(MoneyRefusal::NoSuchEntity);
        }
        let owner = self.owner(caller);
        let held = self.accounts.get(&owner).copied().unwrap_or(0);
        let amount = i64::from(payment.amount);
        if held < amount {
            return Err(MoneyRefusal::InsufficientFunds);
        }
        if let Some(balance) = self.accounts.get_mut(&owner) {
            *balance = balance.saturating_sub(amount);
        }
        if let Some(balance) = self.accounts.get_mut(&payment.to) {
            *balance = balance.saturating_add(amount);
        }
        self.transferred.push((self.now, caller, payment.clone()));
        Ok(())
    }

    fn set_pay_price(&mut self, caller: CallerId, layout: PayPrice) {
        let prim = self.prim_of(caller);
        let _old = self.pay_prices.insert(prim, layout);
    }

    fn pass_touches(&mut self, caller: CallerId, pass: i32) {
        let prim = self.prim_of(caller);
        let _old = self.passes.insert(prim, pass);
    }

    fn pass_collisions(&mut self, caller: CallerId, pass: i32) {
        let prim = self.prim_of(caller);
        let _old = self.collision_passes.insert(prim, pass);
    }

    fn move_to_target(&mut self, caller: CallerId, target: Option<crate::vm::prim::MoveTarget>) {
        self.move_targets.push((caller, target));
    }

    fn set_buoyancy(&mut self, caller: CallerId, buoyancy: f32) {
        self.buoyancies.push((caller, buoyancy));
    }

    fn experience(&self, caller: CallerId) -> Option<ScriptExperience> {
        self.experiences.get(&caller).cloned()
    }

    fn experience_details(&mut self, _caller: CallerId, id: &str) -> Option<ExperienceDetails> {
        if self.looked_up.insert(id.to_owned()) && !self.catalogue.contains_key(id) {
            return None;
        }
        Some(
            self.catalogue
                .get(id)
                .cloned()
                .unwrap_or_else(|| ExperienceDetails::invalid(id)),
        )
    }

    fn admission(&self, agent: &str, experience: &str) -> Admission {
        self.admission_of(agent, experience)
    }

    fn ask_experience(&mut self, caller: CallerId, question: &ExperienceQuestion) -> bool {
        if self.away.contains(&question.agent) {
            return false;
        }
        self.experience_questions
            .push((self.now, caller, question.clone()));
        true
    }

    fn key_value_store(&mut self, experience: &str) -> Result<&mut KeyValueStore, ExperienceError> {
        if self.store_disabled {
            return Err(ExperienceError::StoreDisabled);
        }
        Ok(self.key_values.entry(experience.to_owned()).or_default())
    }

    fn sit_on_link(&mut self, caller: CallerId, agent: &str, link: i32) -> Result<(), SitRefusal> {
        if self.away.contains(agent) {
            return Err(SitRefusal::InvalidAgent);
        }
        self.sat.push((caller, agent.to_owned(), link));
        Ok(())
    }

    fn standing(&self, caller: CallerId) -> Standing {
        Standing {
            owner: self.owner(caller),
            ..self.standing.clone()
        }
    }

    fn simulator(&self) -> Simulator {
        self.simulator.clone()
    }

    fn region_size(&self, _caller: CallerId) -> (u32, u32) {
        (256, 256)
    }

    fn terrain_height(&self, _caller: CallerId, _x: u32, _y: u32) -> f32 {
        self.ground
    }

    fn day_length(&self, _caller: CallerId) -> f32 {
        14_400.0
    }

    fn prim_scripts(&self, prim: &str) -> Option<Vec<CallerId>> {
        let (number, _) = self
            .prim_keys
            .iter()
            .find(|(_, key)| key.eq_ignore_ascii_case(prim))?;
        Some(
            self.prims
                .iter()
                .filter(|(_, in_prim)| *in_prim == number)
                .map(|(id, _)| *id)
                .collect(),
        )
    }

    fn agent_ip(&self, agent: &str) -> String {
        self.addresses.get(agent).cloned().unwrap_or_default()
    }

    fn seat(&mut self, caller: CallerId, link: i32) -> Option<&mut crate::vm::seating::Seat> {
        let this = self.links(caller).this;
        if link != this {
            return None;
        }
        let prim = self.prim_of(caller);
        Some(self.seats.entry(prim).or_default())
    }

    fn avatar_on_sit_target(&self, caller: CallerId, link: i32) -> Option<String> {
        if link != self.links(caller).this {
            return None;
        }
        self.on_target.get(&self.prim_of(caller)).cloned()
    }

    fn unsit(&mut self, caller: CallerId, agent: &str) {
        self.unsat.push((caller, agent.to_owned()));
    }

    fn set_pie_text(&mut self, caller: CallerId, sit: bool, text: &str) {
        self.pie_texts.push((caller, sit, text.to_owned()));
    }

    fn attached(&self, caller: CallerId) -> i32 {
        self.points.get(&caller).copied().unwrap_or(0)
    }

    fn attach(&mut self, caller: CallerId, point: i32) {
        self.attached_to.push((caller, point));
    }

    fn detach(&mut self, caller: CallerId) {
        self.detached.push(caller);
    }

    fn placement(&self, caller: CallerId) -> crate::vm::seating::Placement {
        self.placements
            .get(&caller)
            .cloned()
            .unwrap_or(crate::vm::seating::Placement {
                position: crate::value::ZERO_VECTOR,
                local: crate::value::ZERO_VECTOR,
                root: crate::value::ZERO_VECTOR,
            })
    }

    fn set_position(&mut self, caller: CallerId, local: sl_types::lsl::Vector) {
        let mut local = local;
        if self.linked.is_none() {
            local.z = local.z.max(self.ground);
        }
        let mut placement = self.placement(caller);
        if self.moves_heard && placement.local != local {
            self.changes
                .push((self.prim_key(self.prim_of(caller)), 0x8000));
        }
        placement.local = local.clone();
        placement.position = local.clone();
        let _was = self.placements.insert(caller, placement);
        self.moves.push((caller, local));
    }

    fn prim_count(&self, object: &str) -> Option<i32> {
        self.prim_counts.get(object).copied()
    }

    fn rezzed_in(&self, _prim: &str) -> Option<crate::vm::Tick> {
        // Every prim is there when the region starts.
        Some(crate::vm::Tick(0))
    }

    fn prim(&self, caller: CallerId, prim: &str) -> Option<crate::vm::prim::Prim> {
        let mut state = self
            .prim_states
            .get(prim)
            .cloned()
            .unwrap_or_else(|| crate::vm::prim::Prim::new_box(prim, OWNER));
        if prim == self.prim_key(self.prim_of(caller)) {
            let placement = self.placement(caller);
            state.position = placement.position;
            state.local_position = placement.local;
        }
        Some(state)
    }

    fn set_prim(&mut self, caller: CallerId, state: &crate::vm::prim::Prim) {
        let mut state = state.clone();
        if state.key == self.prim_key(self.prim_of(caller)) {
            let mut placement = self.placement(caller);
            if placement.local != state.local_position {
                let mut to = state.local_position.clone();
                if self.linked.is_none() {
                    to.z = to.z.max(self.ground);
                    placement.position = to.clone();
                }
                placement.local = to.clone();
                state.local_position = to.clone();
                state.position = placement.position.clone();
                let _was = self.placements.insert(caller, placement);
            }
        }
        if self.linked.is_none() {
            state.rotation = state.local_rotation.clone();
            state.root_rotation = state.local_rotation.clone();
        }
        let _was = self.prim_states.insert(state.key.clone(), state);
    }

    fn set_region_position(
        &mut self,
        caller: CallerId,
        position: sl_types::lsl::Vector,
    ) -> Option<sl_types::lsl::Vector> {
        if !(0.0..256.0).contains(&position.x) || !(0.0..256.0).contains(&position.y) {
            return None;
        }
        let mut to = position;
        to.z = to.z.max(self.ground);
        let mut placement = self.placement(caller);
        placement.local = to.clone();
        placement.position = to.clone();
        let _was = self.placements.insert(caller, placement);
        Some(to)
    }

    fn linkset_of(&self, key: &str) -> Vec<String> {
        let own: Vec<String> = self
            .linked
            .as_ref()
            .map(|links| links.prims.iter().map(|prim| prim.key.clone()).collect())
            .unwrap_or_default();
        if own.iter().any(|prim| prim == key) {
            return own;
        }
        if self.prim_keys.values().any(|prim| prim == key) || self.prim_states.contains_key(key) {
            return vec![key.to_owned()];
        }
        Vec::new()
    }

    fn object_details(
        &self,
        caller: CallerId,
        key: &str,
    ) -> Option<crate::vm::prim::ObjectDetails> {
        // A key in any case, as the grid parses it.
        let key = &key.to_ascii_lowercase();
        if let Some(details) = self.details.get(key) {
            return Some(details.clone());
        }
        let prim = self.prim(caller, key).filter(|_| {
            self.prim_states.contains_key(key)
                || self.prim_keys.values().any(|prim| prim == key)
                || *key == self.prim_key(self.prim_of(caller))
        })?;
        Some(crate::vm::prim::ObjectDetails {
            avatar: false,
            name: prim.name.clone(),
            description: prim.description.clone(),
            position: prim.position.clone(),
            rotation: prim.rotation.clone(),
            velocity: crate::value::ZERO_VECTOR,
            owner: OWNER.to_owned(),
            group: crate::value::NULL_KEY.to_owned(),
            creator: prim.creator.clone(),
            last_owner: if self.last_owner_is_owner {
                OWNER.to_owned()
            } else {
                crate::value::NULL_KEY.to_owned()
            },
            root: prim.key.clone(),
            attached_point: 0,
            scripts: (1, 1),
            script_memory: 0x0001_0000,
            prim_count: 1,
            inventory_count: 1,
            click_action: prim.click_action,
            omega: crate::value::ZERO_VECTOR,
            scale: prim.scale.clone(),
            material: prim.material,
            flags: (prim.physics, prim.phantom, prim.temp_on_rez),
            text: prim.text.clone(),
            costs: self.detail_costs.unwrap_or((1.0, 0.05, 1.0, 0.5)),
            selected: (0, 0),
            rezzed: None,
            rezzer: self
                .rezzers
                .get(key)
                .cloned()
                .unwrap_or_else(|| OWNER.to_owned()),
            masks: [i32::MAX, i32::MAX, 0, 0, 532_480],
            hover_height: 0.0,
            body_shape: -1.0,
            animated: 0,
        })
    }

    fn inventory_key(&self, _caller: CallerId, name: &str, asset_type: i32) -> Option<String> {
        self.inventory.get(&(name.to_owned(), asset_type)).cloned()
    }

    fn sound(&mut self, caller: CallerId, prim: &str, cue: &crate::vm::prim::SoundCue) {
        self.sounds.push((caller, prim.to_owned(), cue.clone()));
    }

    fn contents(&self, caller: CallerId) -> Vec<crate::vm::Item> {
        self.contents
            .get(&self.prim_of(caller))
            .cloned()
            .unwrap_or_default()
    }

    fn remove_item(&mut self, caller: CallerId, name: &str, changed: &[i32]) -> bool {
        let prim = self.prim_of(caller);
        let Some(items) = self.contents.get_mut(&prim) else {
            return false;
        };
        let before = items.len();
        items.retain(|item| item.name != name);
        if items.len() == before {
            return false;
        }
        let key = self.prim_key(prim);
        for bits in changed {
            self.changes.push((key.clone(), *bits));
        }
        self.removed
            .push((caller, name.to_owned(), changed.to_vec()));
        true
    }

    fn give_to_prim(&mut self, caller: CallerId, target: &str, items: &[String]) -> bool {
        let Some(to) = self.prim_numbered(target) else {
            return false;
        };
        let from = self.contents(caller);
        for name in items {
            let Some(mut item) = from.iter().find(|item| &item.name == name).cloned() else {
                continue;
            };
            let held = self.contents.entry(to).or_default();
            item.name = free_name(held, &item.name);
            held.push(item);
            self.changes.push((target.to_owned(), 1));
        }
        self.gifts.push((caller, target.to_owned(), items.to_vec()));
        true
    }

    fn offer_items(&mut self, caller: CallerId, offer: &crate::vm::Offer) -> bool {
        if offer.agent == crate::value::NULL_KEY || self.away.contains(&offer.agent) {
            return false;
        }
        self.offers_made.push((caller, offer.clone()));
        true
    }

    fn set_remote_pin(&mut self, caller: CallerId, pin: i32) {
        let _was = self.pins.insert(self.prim_of(caller), pin);
    }

    fn remote_pin(&self, prim: &str) -> Option<(i32, String)> {
        let number = self.prim_numbered(prim)?;
        Some((
            self.pins.get(&number).copied().unwrap_or(0),
            self.owner.clone().unwrap_or_else(|| OWNER.to_owned()),
        ))
    }

    fn load_script(&mut self, caller: CallerId, load: &crate::vm::RemoteLoad) {
        let Some(to) = self.prim_numbered(&load.target) else {
            return;
        };
        let Some(item) = self
            .contents(caller)
            .into_iter()
            .find(|item| item.name == load.script)
        else {
            return;
        };
        let held = self.contents.entry(to).or_default();
        let before = held.len();
        held.retain(|other| other.name != load.script);
        let replaced = held.len() < before;
        held.push(item);
        if replaced && self.replace_raises_twice {
            self.changes.push((load.target.clone(), 1));
        }
        self.changes.push((load.target.clone(), 1));
        self.loads.push((caller, load.clone()));
    }

    fn rez(
        &mut self,
        caller: CallerId,
        rez: &crate::vm::Rez,
    ) -> Result<String, crate::vm::RezRefusal> {
        self.last_rezzed = self.last_rezzed.max(99).saturating_add(1);
        let number = self.last_rezzed;
        let key = format!("00000000-0000-4000-8000-{number:012}");
        let _key = self.prim_keys.insert(number, key.clone());
        let _rezzer = self.rezzed.insert(key.clone(), self.prim_of(caller));
        let rezzer = self.prim_key(self.prim_of(caller));
        let _by = self.rezzers.insert(key.clone(), rezzer);
        self.rezzes.push((caller, rez.clone(), key.clone()));
        Ok(key)
    }

    fn derez(&mut self, caller: CallerId, object: &str) -> bool {
        let rezzer = self.prim_of(caller);
        if self.rezzed.get(object) != Some(&rezzer) {
            return false;
        }
        let _gone = self.rezzed.remove(object);
        true
    }

    fn notecard_text(&mut self, asset: &str) -> Option<String> {
        let text = self.notecards.get(asset).cloned()?;
        let _read = self.notecards_read.insert(asset.to_owned());
        Some(text)
    }

    fn notecard_read(&self, asset: &str) -> Option<String> {
        self.notecards_read
            .contains(asset)
            .then(|| self.notecards.get(asset).cloned())
            .flatten()
    }

    fn make_notecard(
        &mut self,
        caller: CallerId,
        name: &str,
        description: &str,
        text: &str,
        changed: &[i32],
    ) {
        let prim = self.prim_of(caller);
        let asset = format!("0e0e0e0e-0000-4000-8000-{:012}", self.notecards.len());
        let _was = self.notecards.insert(asset.clone(), text.to_owned());
        let owner = self.owner.clone().unwrap_or_else(|| OWNER.to_owned());
        let held = self.contents.entry(prim).or_default();
        let item = crate::vm::Item {
            name: free_name(held, name),
            kind: crate::vm::contents::INVENTORY_NOTECARD,
            item: asset.replace("0e0e0e0e", "0f0f0f0f"),
            asset,
            creator: owner,
            last_owner: crate::value::NULL_KEY.to_owned(),
            masks: crate::vm::Masks::full(),
            description: description.to_owned(),
            acquired: 0,
        };
        held.push(item);
        let key = self.prim_key(prim);
        for bits in changed {
            self.changes.push((key.clone(), *bits));
        }
    }

    fn epoch_micros(&self) -> i64 {
        self.epoch_micros
    }

    fn local_offset(&self, _unix: i64) -> i32 {
        self.local_offset
    }

    fn day_offset(&self, _caller: CallerId) -> f32 {
        57_600.0
    }

    fn sense(&self, _caller: CallerId) -> Vec<crate::vm::sensing::Sensed> {
        self.sensed.clone()
    }

    fn parcel_at(&self, _caller: CallerId, x: f32, y: f32) -> Option<crate::vm::land::Parcel> {
        let (width, depth) = (256.0, 256.0);
        if x < 0.0 || y < 0.0 || x >= width || y >= depth {
            return None;
        }
        if x < 1.0
            && let Some(edge) = &self.edge_parcel
        {
            return Some(edge.clone());
        }
        self.parcel.clone()
    }

    fn region(&self, _caller: CallerId) -> crate::vm::land::Region {
        self.region.clone()
    }

    fn sim_data(&self, _caller: CallerId, name: &str) -> Option<crate::vm::land::SimData> {
        self.sims.get(&name.to_lowercase()).cloned()
    }

    fn ground(&self, _caller: CallerId, x: f32, y: f32) -> f32 {
        match self.edges {
            Some([west, _, _, _]) if x < 0.5 => west,
            Some([_, east, _, _]) if x > 255.5 => east,
            Some([_, _, south, _]) if y < 0.5 => south,
            Some([_, _, _, north]) if y > 255.5 => north,
            _ => self.ground,
        }
    }

    fn wind(&self, _caller: CallerId, _position: &sl_types::lsl::Vector) -> sl_types::lsl::Vector {
        self.wind.clone().unwrap_or(crate::value::ZERO_VECTOR)
    }

    fn environment(
        &self,
        _caller: CallerId,
        _position: Option<&sl_types::lsl::Vector>,
        unix: i64,
    ) -> crate::vm::land::Environment {
        // The default day: the sun turned about y by the day's fraction.
        let into = unix.saturating_add(57_600).rem_euclid(14_400);
        let fraction = f32::from(u16::try_from(into).unwrap_or(0)) / 14_400.0;
        let angle = fraction * core::f32::consts::TAU;
        let (sin, cos) = (angle / 2.0).sin_cos();
        crate::vm::land::Environment {
            day_length: 14_400.0,
            day_offset: 57_600.0,
            sun: sl_types::lsl::Rotation {
                x: 0.0,
                y: -sin,
                z: 0.0,
                s: cos,
            },
            moon: sl_types::lsl::Rotation {
                x: 0.0,
                y: cos,
                z: 0.0,
                s: sin,
            },
        }
    }

    fn change_land(
        &mut self,
        _caller: CallerId,
        _position: &sl_types::lsl::Vector,
        change: &crate::vm::land::LandChange,
    ) -> bool {
        if let (crate::vm::land::LandChange::MusicUrl(url), Some(parcel)) =
            (change, self.parcel.as_mut())
        {
            parcel.music_url.clone_from(url);
        }
        self.land_changes.push(change.clone());
        true
    }

    fn parcel_media(
        &self,
        _caller: CallerId,
        _position: &sl_types::lsl::Vector,
    ) -> crate::vm::land::ParcelMedia {
        crate::vm::land::ParcelMedia::default()
    }

    fn sim_stats(&self, _caller: CallerId) -> Vec<f32> {
        self.sim_stats.clone()
    }

    fn environment_rule(
        &self,
        _caller: CallerId,
        _position: &sl_types::lsl::Vector,
        rule: i32,
    ) -> Option<Vec<crate::value::Element>> {
        (rule == 0).then(|| {
            vec![crate::value::Element::Vector(sl_types::lsl::Vector {
                x: 0.35,
                y: 0.35,
                z: 0.35,
            })]
        })
    }

    fn map_texture(&self, _caller: CallerId, region: &str) -> Option<String> {
        region
            .is_empty()
            .then(|| self.map_texture.clone())
            .flatten()
    }

    fn send_http(&mut self, caller: CallerId, request: &crate::vm::http::Outbound) {
        self.http.sent.push((caller, request.clone()));
    }

    fn request_url(&mut self, caller: CallerId, key: &str, secure: bool) -> Option<String> {
        if (secure && self.http.no_secure) || self.free_urls(caller) <= 0 {
            return None;
        }
        let authority = if secure {
            self.http.secure_authority.clone()
        } else {
            self.http.authority.clone()
        };
        let url = crate::vm::http::granted_url(self.http.flavour, &authority, key, secure);
        let _previous = self.http.urls.insert(key.to_owned(), (caller, url.clone()));
        Some(url)
    }

    fn release_url(&mut self, caller: CallerId, url: &str) {
        self.http
            .urls
            .retain(|_, (holder, held)| !(*holder == caller && held == url));
    }

    fn free_urls(&self, _caller: CallerId) -> i32 {
        self.http
            .allowance
            .saturating_sub(i32::try_from(self.http.urls.len()).unwrap_or(i32::MAX))
    }

    fn answer_http(
        &mut self,
        caller: CallerId,
        request: &str,
        answered: crate::vm::http::Answered,
    ) {
        self.http
            .answered
            .push((caller, request.to_owned(), answered));
    }

    fn http_throttle(&mut self, caller: CallerId) -> &mut crate::vm::http::Throttle {
        let prim = self.prim_of(caller);
        self.http.throttles.entry(prim).or_default()
    }

    fn url_authority(&self, _caller: CallerId) -> String {
        self.http.authority.clone()
    }

    fn agent(&self, _caller: CallerId, agent: &str) -> Option<crate::vm::avatars::Agent> {
        self.avatars
            .get(agent)
            .map(|avatar| crate::vm::avatars::Agent {
                animations: avatar.playing(),
                ..avatar.agent.clone()
            })
    }

    fn agent_record(
        &self,
        _caller: CallerId,
        agent: &str,
    ) -> Option<crate::vm::avatars::AgentRecord> {
        self.avatars
            .get(agent)
            .map(|avatar| crate::vm::avatars::AgentRecord {
                online: true,
                legacy_name: avatar.agent.legacy_name.clone(),
                display_name: avatar.agent.display_name.clone(),
                username: avatar.agent.username.clone(),
                born: "2026-01-01".to_owned(),
                payment: 0,
            })
    }

    fn agent_named(&self, _caller: CallerId, name: &str) -> Option<(String, bool)> {
        self.avatars
            .iter()
            .find(|(_, avatar)| {
                avatar.agent.legacy_name.eq_ignore_ascii_case(name)
                    || avatar.agent.username.eq_ignore_ascii_case(name)
            })
            .map(|(key, _)| (key.clone(), true))
    }

    fn animate(&mut self, _caller: CallerId, agent: &str, animation: &str, start: bool) {
        if let Some(avatar) = self.avatars.get_mut(agent) {
            avatar.started.retain(|held| held != animation);
            if start {
                avatar.started.push(animation.to_owned());
            }
        }
    }

    fn set_animation_override(
        &mut self,
        _caller: CallerId,
        agent: &str,
        state: &str,
        animation: Option<(&str, &str)>,
    ) {
        if let Some(avatar) = self.avatars.get_mut(agent) {
            match animation {
                Some((name, asset)) => {
                    let _previous = avatar
                        .overrides
                        .insert(state.to_owned(), (name.to_owned(), asset.to_owned()));
                }
                None => {
                    let _gone = avatar.overrides.remove(state);
                }
            }
        }
    }

    fn animation_override(&self, _caller: CallerId, agent: &str, state: &str) -> Option<String> {
        self.avatars
            .get(agent)?
            .overrides
            .get(state)
            .map(|(name, _)| name.clone())
    }

    fn object_animate(&mut self, _caller: CallerId, _name: &str, _animation: &str, _start: bool) {}

    fn object_animations(&self, _caller: CallerId) -> Vec<String> {
        Vec::new()
    }

    fn control_changes(
        &mut self,
        caller: CallerId,
        agent: &str,
        changes: &[crate::vm::avatars::ControlChange],
        taken: Option<i32>,
    ) {
        for change in changes {
            self.avatar_notes.push(format!(
                "# script-control-change action={} controls={} pass_to_agent={}",
                if change.take { "take" } else { "release" },
                change.controls,
                u8::from(change.pass_to_agent)
            ));
        }
        match taken {
            Some(_) => {
                let _previous = self.controls_taken.insert(caller, agent.to_owned());
            }
            None => {
                let _gone = self.controls_taken.remove(&caller);
            }
        }
    }

    fn set_camera(
        &mut self,
        _caller: CallerId,
        _agent: &str,
        rules: &[crate::vm::avatars::CameraRule],
    ) {
        let rules: Vec<String> = rules
            .iter()
            .map(|rule| format!("{}={}", rule.property, camera_number(rule.value)))
            .collect();
        self.avatar_notes
            .push(format!("# follow-cam object=probe {}", rules.join(" ")));
    }

    fn clear_camera(&mut self, _caller: CallerId, _agent: &str) {
        self.avatar_notes
            .push("# follow-cam-clear object=probe".to_owned());
    }

    fn camera(
        &self,
        _caller: CallerId,
        agent: &str,
    ) -> Option<(sl_types::lsl::Vector, sl_types::lsl::Rotation)> {
        self.avatars.get(agent).map(|avatar| avatar.camera.clone())
    }

    fn teleport_agent(
        &mut self,
        _caller: CallerId,
        agent: &str,
        destination: &crate::vm::avatars::Destination,
    ) {
        self.teleports.push((agent.to_owned(), destination.clone()));
    }

    fn attached_list(&self, _caller: CallerId, agent: &str) -> Option<Vec<String>> {
        self.avatars.get(agent).map(|_| Vec::new())
    }

    fn cast_ray(
        &self,
        _caller: CallerId,
        _start: &sl_types::lsl::Vector,
        _end: &sl_types::lsl::Vector,
    ) -> Vec<crate::vm::sensing::RayHit> {
        self.ray_hits.clone()
    }
}

/// `name`, or the first `name N` no item of `held` has, as both grids name
/// a second copy.
fn free_name(held: &[crate::vm::Item], name: &str) -> String {
    let taken = |candidate: &str| held.iter().any(|item| item.name == candidate);
    if !taken(name) {
        return name.to_owned();
    }
    (1_u32..)
        .map(|number| format!("{name} {number}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

impl Recorder {
    /// What `agent` answered `experience`.
    fn admission_of(&self, agent: &str, experience: &str) -> Admission {
        match self
            .admissions
            .get(&(agent.to_owned(), experience.to_owned()))
        {
            Some(true) => Admission::Allowed,
            Some(false) => Admission::Blocked,
            None => Admission::Undecided,
        }
    }

    /// The number of the prim keyed `key`.
    fn prim_numbered(&self, key: &str) -> Option<u64> {
        self.prim_keys
            .iter()
            .find(|(_, prim)| prim.as_str() == key)
            .map(|(number, _)| *number)
    }

    /// The prim `caller` is in.
    fn prim_of(&self, caller: CallerId) -> u64 {
        self.prims.get(&caller).copied().unwrap_or(caller.0)
    }

    /// The key of the prim numbered `prim`.
    fn prim_key(&self, prim: u64) -> String {
        self.prim_keys
            .get(&prim)
            .cloned()
            .unwrap_or_else(|| "fab01e52-3517-4545-be1b-4cbf6d8ace3b".to_owned())
    }

    /// What `caller` printed, with the tick of each.
    fn from(&self, caller: CallerId) -> Vec<(u64, &str)> {
        self.printed
            .iter()
            .filter(|(_, who, _)| *who == caller)
            .map(|(tick, _, text)| (*tick, text.as_str()))
            .collect()
    }

    /// The text of every line `caller`'s object said, in order.
    fn heard(&self, caller: CallerId) -> Vec<&str> {
        self.said
            .iter()
            .filter(|(_, who, _)| *who == caller)
            .map(|(_, _, speech)| speech.text.as_str())
            .collect()
    }

    /// What `caller` printed, text only.
    fn texts(&self, caller: CallerId) -> Vec<&str> {
        self.from(caller)
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }
}

/// A compiled script, shared.
fn program(source: &str) -> Result<Arc<crate::bytecode::Program>, String> {
    compile(source).map(Arc::new).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// A compiled script, shared, as the grid `flavour` imitates compiles it.
fn program_for(
    source: &str,
    flavour: EngineFlavour,
) -> Result<Arc<crate::bytecode::Program>, String> {
    let dialect = if flavour.is_opensim() {
        crate::Dialect::OpenSim
    } else {
        crate::Dialect::SecondLife
    };
    crate::compile_for(source, dialect)
        .map(Arc::new)
        .map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
}

/// A region with the default budgets for [`STEP`].
fn engine() -> Engine {
    Engine::new(EngineConfig::for_step(STEP, EngineFlavour::SecondLife))
}

/// Run `ticks` ticks, returning each report.
fn run(engine: &mut Engine, host: &mut Recorder, ticks: u64) -> Vec<TickReport> {
    core::iter::repeat_with(|| {
        host.now = engine.now().0.saturating_add(1);
        engine.tick(host)
    })
    .take(usize::try_from(ticks).unwrap_or(usize::MAX))
    .collect()
}

/// The event called `name`.
fn named(name: &str) -> Result<&'static Event, String> {
    event(name).ok_or_else(|| format!("no event {name}"))
}

/// The outcomes `caller`'s slices in `report` ended with.
fn outcomes(report: &TickReport, caller: CallerId) -> Vec<Outcome> {
    report
        .slices
        .iter()
        .filter(|(who, _)| *who == caller)
        .map(|(_, slice)| slice.outcome.clone())
        .collect()
}

const A: CallerId = CallerId(1);
const B: CallerId = CallerId(2);
/// A third script.
const C: CallerId = CallerId(3);
/// A fourth script.
const D: CallerId = CallerId(4);
/// A fifth script.
const E: CallerId = CallerId(5);

#[test]
fn a_script_initialises_its_globals_then_enters_default() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer n = 4;
            string s = "x";
            default { state_entry() { print(s + (string)(n * 2)); } }"#,
        )?,
    );
    let reports = run(&mut engine, &mut host, 2);
    assert_eq!(host.from(A), vec![(1, "x8")]);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![Outcome::Finished, Outcome::Finished])
    );
    // An idle script is not runnable and costs nothing.
    assert_eq!(reports.get(1).map(|report| report.runnable), Some(0));
    Ok(())
}

#[test]
fn a_runaway_loop_uses_its_budget_and_the_tick_still_completes() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("integer n; default { state_entry() { while (TRUE) { ++n; } } }")?,
    );
    let _none = engine.add(
        B,
        program(
            "default { state_entry() {
                integer i;
                for (i = 0; i < 3; ++i) { print((string)i); llSleep(0.1); }
            } }",
        )?,
    );
    let budget = engine.config().script_budget;
    let reports = run(&mut engine, &mut host, 4);
    // The neighbour runs one iteration per tick, the loop notwithstanding.
    assert_eq!(host.from(B), vec![(1, "0"), (2, "1"), (3, "2")]);
    let mut spent = 0_u32;
    let mut shares = 0_u32;
    for report in &reports {
        assert_eq!(
            outcomes(report, A).last(),
            Some(&Outcome::Yielded),
            "tick {:?}",
            report.tick
        );
        let used: u32 = report
            .slices
            .iter()
            .filter(|(who, _)| *who == A)
            .map(|(_, slice)| slice.used)
            .sum();
        spent = spent.saturating_add(used);
        shares = shares.saturating_add(budget);
        // Spent to the last instruction of every share so far, and past it
        // by at most the one back-edge the next tick pays back.
        assert!(
            (shares..=shares.saturating_add(cost::speed(EngineFlavour::SecondLife).backward_jump))
                .contains(&spent),
            "tick {:?} used {spent} of {shares} in all",
            report.tick
        );
    }
    // And it really is running: the counter moves every tick.
    let counter = |engine: &Engine| match engine.instance(A).and_then(|a| a.global("n")) {
        Some(Value::Integer(n)) => *n,
        _ => -1,
    };
    let before = counter(&engine);
    let _reports = run(&mut engine, &mut host, 1);
    assert!(counter(&engine) > before, "the loop made no progress");
    Ok(())
}

#[test]
fn a_sleep_resumes_on_its_tick_and_not_before() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default { state_entry() {
                print("before"); llSleep(2.0); print("after"); llSleep(0.25); print("end");
            } }"#,
        )?,
    );
    let reports = run(&mut engine, &mut host, 30);
    // Two seconds are twenty ticks; a quarter second rounds up to three.
    assert_eq!(
        host.from(A),
        vec![(1, "before"), (21, "after"), (24, "end")]
    );
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![Outcome::Finished, Outcome::Sleeping(Tick(21))])
    );
    // Asleep is not runnable.
    assert!(
        reports
            .iter()
            .filter(|report| (2..21).contains(&report.tick.0))
            .all(|report| report.runnable == 0)
    );
    Ok(())
}

#[test]
fn a_stopped_script_keeps_its_globals_and_resumes_where_it_was() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer n;
            default {
                state_entry() { while (TRUE) { ++n; print((string)n); llSleep(0.1); } }
                touch_start(integer count) { print(\"touched\"); }
            }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 3);
    assert_eq!(host.texts(A), vec!["1", "2", "3"]);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(false);
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Stopped))
    );
    let reports = run(&mut engine, &mut host, 5);
    assert!(reports.iter().all(|report| report.runnable == 0));
    assert_eq!(host.texts(A), vec!["1", "2", "3"]);
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(
        instance.is_busy(),
        "stopping dropped the handler in progress"
    );
    assert_eq!(instance.global("n"), Some(&Value::Integer(3)));
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 2);
    // It carries on counting from where it was, not from a reset.
    assert_eq!(host.texts(A), vec!["1", "2", "3", "4", "5"]);
    // The touch posted while stopped never arrives.
    assert_eq!(engine.instance(A).map(Instance::queued), Some(0));
    Ok(())
}

#[test]
fn a_run_time_error_stops_one_script_and_its_neighbours_run_on() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer zero;
default {
    state_entry() {
        print(\"start\");
        print((string)(1 / zero));
        print(\"unreached\");
    }
}",
        )?,
    );
    let _none = engine.add(
        B,
        program("default { state_entry() { while (TRUE) { print(\"b\"); llSleep(0.1); } } }")?,
    );
    let reports = run(&mut engine, &mut host, 3);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![
            Outcome::Finished,
            Outcome::Faulted(Fault {
                error: RuntimeError::MathError(crate::Division::Integer),
                position: Some(Position {
                    line: 5,
                    column: 24
                }),
                event: Some("state_entry"),
            }),
        ])
    );
    assert_eq!(host.texts(A), vec!["start"]);
    assert_eq!(host.texts(B), vec!["b", "b", "b"]);
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(!instance.is_running());
    assert_eq!(instance.state(), StateId::DEFAULT);
    Ok(())
}

/// `inventory/listing`'s `start-helper`: a script reset while stopped is
/// started after the starter's next line on every grid; only a script stopped
/// in the middle of a body wakes inside the call. On OpenSim the reset makes
/// it run `state_entry` again; on Second Life it carries on without one.
#[test]
fn a_script_reset_while_stopped_starts_after_the_starters_next_line() -> Result<(), String> {
    let starter = r#"integer step;
        default {
            state_entry() { llSetTimerEvent(0.5); }
            timer() {
                ++step;
                if (step == 1) llSetScriptState("Helper", FALSE);
                else if (step == 2) llResetOtherScript("Helper");
                else {
                    llSetTimerEvent(0.0);
                    llSetScriptState("Helper", TRUE);
                    print("started");
                }
            }
        }"#;
    let helper = r#"default { state_entry() { print("entry"); } }"#;
    for (flavour, heard) in [
        (EngineFlavour::SecondLife, vec!["entry", "started"]),
        (EngineFlavour::YEngine, vec!["entry", "started", "entry"]),
        (EngineFlavour::XEngine, vec!["entry", "started", "entry"]),
    ] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder {
            scripts: [("Starter", A), ("Helper", B)]
                .into_iter()
                .map(|(name, id)| (name.to_owned(), id))
                .collect(),
            ..Recorder::default()
        };
        let _none = engine.add(A, program_for(starter, flavour)?);
        let _none = engine.add(B, program_for(helper, flavour)?);
        let _reports = run(&mut engine, &mut host, 40);
        let printed: Vec<&str> = host
            .printed
            .iter()
            .map(|(_, _, text)| text.as_str())
            .collect();
        assert_eq!(printed, heard, "{flavour:?}");
    }
    Ok(())
}

#[test]
fn get_script_state_reads_the_flag_a_fault_clears() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder {
        scripts: [("Watcher", A), ("Divider", B)]
            .into_iter()
            .map(|(name, id)| (name.to_owned(), id))
            .collect(),
        ..Recorder::default()
    };
    let _none = engine.add(
        A,
        program(
            r#"report() {
                print((string)llGetScriptState("Watcher")
                    + (string)llGetScriptState("Divider")
                    + (string)llGetScriptState("Nobody"));
            }
            default {
                state_entry() { report(); llSetTimerEvent(0.5); }
                timer() { report(); llSetTimerEvent(0.0); }
            }"#,
        )?,
    );
    let _none = engine.add(
        B,
        program("integer zero; default { state_entry() { llSleep(1.5); zero = 1 / zero; } }")?,
    );
    // Each report asks after a missing script, which costs a second — and
    // LSL evaluates right to left, so the others are read after it.
    let _reports = run(&mut engine, &mut host, 30);
    assert_eq!(host.texts(A), vec!["110", "100"]);
    // Second Life: setting the run flag again does not release a script a
    // fault holds…
    engine
        .instance_mut(B)
        .ok_or("no instance")?
        .set_running(true);
    engine.instance_mut(A).ok_or("no instance")?.reset();
    let _reports = run(&mut engine, &mut host, 11);
    assert_eq!(host.texts(A), vec!["110", "100", "100"]);
    // …a reset does, and it reads as running again: one flag, not a copy.
    engine.instance_mut(B).ok_or("no instance")?.reset();
    engine.instance_mut(A).ok_or("no instance")?.reset();
    let _reports = run(&mut engine, &mut host, 11);
    assert_eq!(host.texts(A), vec!["110", "100", "100", "110"]);
    Ok(())
}

#[test]
fn a_body_mono_refuses_faults_before_its_first_line() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { state_entry() { integer i = 3; print(\"x\"); float g = i *= 1.5; } }")?,
    );
    let reports = run(&mut engine, &mut host, 1);
    assert!(host.texts(A).is_empty());
    assert!(matches!(
        reports.first().map(|report| outcomes(report, A)).as_deref(),
        Some([
            Outcome::Finished,
            Outcome::Faulted(Fault {
                error: RuntimeError::InvalidProgram,
                ..
            })
        ])
    ));
    Ok(())
}

#[test]
fn runaway_recursion_is_a_stack_heap_collision_not_a_host_crash() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("integer f(integer n) { return f(n + 1); } default { state_entry() { f(0); } }")?,
    );
    let reports = run(&mut engine, &mut host, 10);
    let faults: Vec<RuntimeError> = reports
        .iter()
        .flat_map(|report| outcomes(report, A))
        .filter_map(|outcome| match outcome {
            Outcome::Faulted(fault) => Some(fault.error),
            _ => None,
        })
        .collect();
    assert_eq!(faults, vec![RuntimeError::StackHeapCollision]);
    Ok(())
}

#[test]
fn a_reset_restarts_the_initialisers_and_drops_the_queue() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer g = 5;
            integer resets;
            default {
                state_entry() { print((string)g); g = 7; }
                touch_start(integer count) {
                    print(\"touch \" + (string)g);
                    llResetScript();
                    print(\"unreached\");
                }
                changed(integer change) { print(\"stale change\"); }
            }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    assert_eq!(engine.changed(A, 1), Ok(Some(Posted::Queued)));
    let reports = run(&mut engine, &mut host, 2);
    // The touch runs and resets; the change, queued behind it, is dropped
    // with the queue; the initialisers put `g` back to 5.
    assert_eq!(host.texts(A), vec!["5", "touch 7", "5"]);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![Outcome::Reset, Outcome::Finished, Outcome::Finished])
    );
    assert_eq!(engine.instance(A).map(Instance::queued), Some(0));
    Ok(())
}

/// `llDie` asks the host to remove the object and ends the script there:
/// nothing after the call runs, the queue goes, and later events are not
/// taken. An attachment stays, and its script carries on past the call.
#[test]
fn ll_die_ends_the_script_unless_the_object_is_worn() -> Result<(), String> {
    let source = "default {
        state_entry() { print(\"before\"); llDie(); print(\"after\"); }
        changed(integer change) { print(\"changed\"); }
    }";
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    let _also = engine.add(B, program(source)?);
    let _worn = host.worn.insert(B, OWNER.to_owned());
    let reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["before"]);
    assert_eq!(host.texts(B), vec!["before", "after"]);
    assert_eq!(host.died, vec![(1, A)]);
    assert!(
        reports
            .first()
            .is_some_and(|report| outcomes(report, A).contains(&Outcome::Died))
    );
    assert_eq!(engine.changed(A, 1), Ok(Some(Posted::Stopped)));
    let _later = run(&mut engine, &mut host, 2);
    assert_eq!(host.texts(A), vec!["before"]);
    Ok(())
}

#[test]
fn a_state_change_runs_exit_then_entry_and_discards_the_queue() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer f() { if (TRUE) state two; return 1; }
            default {
                state_entry() { print("entry"); }
                touch_start(integer count) {
                    integer x = f();
                    print("after " + (string)x);
                }
                state_exit() { print("exit"); }
            }
            state two {
                state_entry() { print("two"); state two; }
                touch_start(integer count) { print("touch two"); }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    for _ in 0..2 {
        let _posted = engine.post_detected(A, touch, vec![toucher("a")]);
    }
    let reports = run(&mut engine, &mut host, 2);
    // The function's `state` yields 0 and the handler carries on; the
    // transition waits for its end, then discards the second touch. `state
    // two` inside `two` is no transition. All three as aditi does them
    // (2026-09-28): a `state default;` in `default` ran neither `state_exit`
    // nor `state_entry`, and a function's `state other;` returned 0 to a
    // caller that carried on before `default`'s `state_exit` ran.
    assert_eq!(host.texts(A), vec!["entry", "after 0", "exit", "two"]);
    let two = StateId(1);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![
            Outcome::Finished,
            Outcome::StateChanged(two),
            Outcome::Finished,
        ])
    );
    assert_eq!(engine.instance(A).map(Instance::state), Some(two));
    assert_eq!(engine.instance(A).map(Instance::queued), Some(0));
    Ok(())
}

#[test]
fn events_reach_only_a_handler_of_the_current_state() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default { touch_start(integer count) { print("touched " + (string)count); } }"#,
        )?,
    );
    let touch = named("touch_start")?;
    let timer = named("timer")?;
    let link = named("link_message")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a"), toucher("b"), toucher("c")]),
        Ok(Some(Posted::Queued))
    );
    assert_eq!(engine.post(A, timer, vec![]), Ok(Some(Posted::NoHandler)));
    assert_eq!(engine.post(B, timer, vec![]), Ok(None));
    assert_eq!(
        engine.post(A, link, vec![]),
        Err(PostError::Arity {
            event: "link_message",
            expected: 4,
            found: 0,
        })
    );
    assert_eq!(
        engine.post(
            A,
            link,
            vec![
                Value::Float(1.0),
                Value::Integer(0),
                Value::String(String::new()),
                Value::Key(String::new()),
            ]
        ),
        Err(PostError::Argument {
            event: "link_message",
            index: 0,
            expected: TypeName::Integer,
            found: TypeName::Float,
        })
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["touched 3"]);
    Ok(())
}

#[test]
fn an_over_budget_region_serves_its_scripts_in_turn() -> Result<(), String> {
    let config = EngineConfig::for_step(STEP, EngineFlavour::SecondLife);
    // Room for one script's share per tick, and three busy scripts.
    let mut engine = Engine::new(EngineConfig {
        region_budget: u64::from(config.script_budget),
        ..config
    });
    let mut host = Recorder::default();
    let busy = program("default { state_entry() { while (TRUE) { } } }")?;
    let c = CallerId(3);
    for id in [A, B, c] {
        let _none = engine.add(id, Arc::clone(&busy));
    }
    let reports = run(&mut engine, &mut host, 6);
    let served: Vec<CallerId> = reports
        .iter()
        .filter_map(|report| report.slices.first().map(|(id, _)| *id))
        .collect();
    assert_eq!(served, vec![A, B, c, A, B, c]);
    for report in &reports {
        assert_eq!((report.runnable, report.served), (3, 1));
        // The region's budget, give or take the one back-edge a script
        // overran its share by last time it ran and pays back now.
        let budget = u64::from(config.script_budget);
        let slack = u64::from(cost::speed(EngineFlavour::SecondLife).backward_jump);
        assert!(
            (budget.saturating_sub(slack)..=budget.saturating_add(slack)).contains(&report.used),
            "used {} of {budget}",
            report.used
        );
    }
    Ok(())
}

#[test]
fn every_value_type_survives_a_round_trip_through_the_machine() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"vector v = <1, 2, 3>;
            rotation r;
            list l = [1, 2.5, "s"];
            default { state_entry() {
                v.y = 5;
                r.s = 0.5;
                l += v;
                key k = "k";
                print((string)v + " " + (string)r.s + " " + (string)llGetListLength(l)
                    + " " + (string)l + " " + (string)k + " " + (string)(v.x + v.z));
            } }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(
        host.texts(A),
        vec![
            "<1.00000, 5.00000, 3.00000> 0.500000 4 12.500000s<1.000000, 5.000000, 3.000000> k 4.000000"
        ]
    );
    assert!(host.stubbed.is_empty());
    Ok(())
}

#[test]
fn a_call_to_a_function_nobody_wrote_stops_the_script() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program("default { state_entry() { llStopHover(); } }")?);
    let reports = run(&mut engine, &mut host, 1);
    assert!(matches!(
        reports.first().map(|report| outcomes(report, A)).as_deref(),
        Some([
            Outcome::Finished,
            Outcome::Faulted(Fault {
                error: RuntimeError::Unimplemented(BuiltinId::LlStopHover),
                ..
            })
        ])
    ));
    Ok(())
}

#[test]
fn a_sleep_runs_out_while_stopped_and_a_restart_resumes_at_once() -> Result<(), String> {
    // aditi (2026-09-28): a script stopped one second into `llSleep(5.0)`
    // and restarted at nine woke within the restart's frame.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(r#"default { state_entry() { print("sleep"); llSleep(5.0); print("woke"); } }"#)?,
    );
    let _reports = run(&mut engine, &mut host, 10);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(false);
    let _reports = run(&mut engine, &mut host, 80);
    assert_eq!(host.texts(A), vec!["sleep"]);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.from(A), vec![(1, "sleep"), (91, "woke")]);
    Ok(())
}

#[test]
fn a_restart_before_the_sleep_ends_still_waits_it_out() -> Result<(), String> {
    // aditi (2026-09-28): `llSleep(5.0)` begun at 06.73 s, the script stopped
    // at 07.76 and restarted at 09.82, woke at 11.78 — five seconds after it
    // began. The sleep counts on while the script is stopped.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(r#"default { state_entry() { print("sleep"); llSleep(5.0); print("woke"); } }"#)?,
    );
    let _reports = run(&mut engine, &mut host, 10);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(false);
    let _reports = run(&mut engine, &mut host, 20);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 30);
    assert_eq!(host.from(A), vec![(1, "sleep"), (51, "woke")]);
    Ok(())
}

/// A touch by `name`, keyed after it.
fn toucher(name: &str) -> Detected {
    Detected {
        key: format!("{name:0>8}-0000-0000-0000-000000000000"),
        name: name.to_owned(),
        kind: 1,
        touch: Touch {
            face: 2,
            ..Touch::INVALID
        },
        ..Detected::blank()
    }
}

#[test]
fn the_queue_holds_sixty_four_events_and_drops_the_rest() -> Result<(), String> {
    let mut engine = engine();
    let _none = engine.add(
        A,
        program(
            "default { link_message(integer sender, integer number, string text, key id) { } }",
        )?,
    );
    let link = named("link_message")?;
    let args = || {
        vec![
            Value::Integer(1),
            Value::Integer(0),
            Value::String(String::new()),
            Value::Key(String::new()),
        ]
    };
    let posted: Vec<Posted> =
        core::iter::repeat_with(|| engine.post(A, link, args()).ok().flatten())
            .take(70)
            .collect::<Option<_>>()
            .ok_or("a post failed")?;
    assert_eq!(
        posted.iter().filter(|p| **p == Posted::Queued).count(),
        MAX_QUEUED
    );
    assert_eq!(posted.last(), Some(&Posted::QueueFull));
    assert_eq!(engine.instance(A).map(Instance::queued), Some(MAX_QUEUED));
    Ok(())
}

#[test]
fn a_timer_repeats_on_its_ticks_and_never_stacks() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer n;
            default {
                state_entry() { llSetTimerEvent(0.3); }
                timer() {
                    ++n;
                    print("timer " + (string)n);
                    if (n == 2) llSleep(1.0);
                    if (n == 3) llSetTimerEvent(0.0);
                }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 40);
    // Due at 4 and 7; the second handler sleeps ten ticks, through three
    // more due ticks (10, 13, 16). On Second Life a timer does not fall due
    // while its own handler runs (`events/timer-reset`): the one overdue is
    // raised once, in the tick after the handler returned at 17, and stops
    // the timer.
    assert_eq!(
        host.from(A),
        vec![(4, "timer 1"), (7, "timer 2"), (18, "timer 3")]
    );
    assert_eq!(engine.instance(A).map(Instance::has_timer), Some(false));
    Ok(())
}

#[test]
fn a_state_change_releases_the_hosts_hold_and_discards_the_queue() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default {
                state_entry() { llSetTimerEvent(0.5); }
                touch_start(integer n) { state two; }
                changed(integer change) { print("stale changed"); }
            }
            state two {
                timer() { print("timer in two"); llSetTimerEvent(0.0); }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    // A `changed` waits behind a touch on every queue rule.
    assert_eq!(engine.changed(A, 2), Ok(Some(Posted::Queued)));
    let _reports = run(&mut engine, &mut host, 10);
    // The `changed` queued behind the touch is gone with the state; the
    // host was told once; the timer set in `default` carries on in `two`.
    assert_eq!(host.from(A), vec![(6, "timer in two")]);
    assert_eq!(host.left, vec![(2, A)]);
    Ok(())
}

#[test]
fn detected_functions_read_the_block_of_the_event_being_handled_and_only_it() -> Result<(), String>
{
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"string who(integer i) {
                return llDetectedName(i) + "/" + (string)llDetectedType(i) + "/"
                    + (string)llDetectedTouchFace(i) + "/" + (string)llDetectedKey(i);
            }
            default {
                touch_start(integer n) {
                    print((string)n + " " + who(0) + " " + who(1) + " " + who(2));
                    llSetTimerEvent(0.1);
                }
                timer() { llSetTimerEvent(0.0); print("timer " + who(0)); }
            }"#,
        )?,
    );
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    // A second toucher in the same tick joins the queued event.
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("b"), toucher("a")]),
        Ok(Some(Posted::Merged))
    );
    let _reports = run(&mut engine, &mut host, 3);
    let a = "0000000a-0000-0000-0000-000000000000";
    let b = "0000000b-0000-0000-0000-000000000000";
    // Past the block, and in an event without one, everything reads zero —
    // the name as the `NULL_KEY` string, the face as 0 (aditi, 2026-09-28).
    let null = crate::value::NULL_KEY;
    assert_eq!(
        host.texts(A),
        vec![
            format!("2 a/1/2/{a} b/1/2/{b} {null}/0/0/{null}").as_str(),
            format!("timer {null}/0/0/{null}").as_str(),
        ]
    );
    Ok(())
}

#[test]
fn touches_queued_in_different_ticks_merge_on_second_life_only() -> Result<(), String> {
    // `events/merging` (2026-10-25): three touches a second apart, made while
    // the script slept, were one `touch_start` on Second Life and three on
    // OpenSim.
    for (coalescing, merged, seen) in [
        (Coalescing::Reference, Posted::Merged, vec!["1", "2"]),
        (Coalescing::Never, Posted::Queued, vec!["1", "1", "1"]),
    ] {
        let mut engine = Engine::new(EngineConfig {
            coalescing,
            ..EngineConfig::for_step(STEP, EngineFlavour::SecondLife)
        });
        let mut host = Recorder::default();
        let _none = engine.add(
            A,
            program("default { touch_start(integer n) { print((string)n); llSleep(0.5); } }")?,
        );
        let touch = named("touch_start")?;
        assert_eq!(
            engine.post_detected(A, touch, vec![toucher("a")]),
            Ok(Some(Posted::Queued))
        );
        let _reports = run(&mut engine, &mut host, 1);
        assert_eq!(
            engine.post_detected(A, touch, vec![toucher("b")]),
            Ok(Some(Posted::Queued))
        );
        let _reports = run(&mut engine, &mut host, 1);
        // A tick later, while the first of them still waits.
        assert_eq!(
            engine.post_detected(A, touch, vec![toucher("c")]),
            Ok(Some(merged))
        );
        let _reports = run(&mut engine, &mut host, 20);
        assert_eq!(host.texts(A), seen, "{coalescing:?}");
    }
    let mut engine = engine();
    let _none = engine.add(A, program("default { touch_start(integer n) { } }")?);
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post(A, touch, vec![Value::Integer(1)]),
        Err(PostError::Detection("touch_start"))
    );
    assert_eq!(
        engine.post_detected(A, touch, Vec::new()),
        Err(PostError::DetectedCount {
            event: "touch_start",
            found: 0,
        })
    );
    assert_eq!(
        engine.post(A, named("state_entry")?, Vec::new()),
        Err(PostError::Transition("state_entry"))
    );
    Ok(())
}

#[test]
fn changed_carries_the_bits_of_whatever_raised_it() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { changed(integer change) { print((string)change); } }")?,
    );
    let bit = |name: &str| match crate::library::constant(name).map(|c| c.value.to_value()) {
        Some(Value::Integer(bit)) => Ok(bit),
        _ => Err(format!("no constant {name}")),
    };
    let raised = [
        "CHANGED_INVENTORY",
        "CHANGED_COLOR",
        "CHANGED_SCALE",
        "CHANGED_LINK",
        "CHANGED_OWNER",
        "CHANGED_REGION_START",
    ];
    for (index, name) in raised.into_iter().enumerate() {
        let expected = if index == 0 {
            Posted::Queued
        } else {
            Posted::Merged
        };
        assert_eq!(engine.changed(A, bit(name)?), Ok(Some(expected)));
    }
    let _reports = run(&mut engine, &mut host, 1);
    // One tick's changes are one event with every bit.
    assert_eq!(host.texts(A), vec!["1195"]);
    // Changes a tick apart are events of their own.
    for name in ["CHANGED_TEXTURE", "CHANGED_SHAPE"] {
        assert_eq!(engine.changed(A, bit(name)?), Ok(Some(Posted::Queued)));
        let _reports = run(&mut engine, &mut host, 1);
    }
    assert_eq!(host.texts(A), vec!["1195", "16", "4"]);
    Ok(())
}

#[test]
fn on_rez_brings_the_start_parameter_and_a_reset_keeps_it() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer resets;
            default {
                state_entry() { print("start " + (string)llGetStartParameter()); }
                on_rez(integer param) {
                    print("rez " + (string)param + " " + (string)llGetStartParameter());
                    llResetScript();
                }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(engine.rez(A, 42), Ok(Some(Posted::Queued)));
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["start 0", "rez 42 42", "start 42"]);
    Ok(())
}

#[test]
fn the_engines_own_events_are_in_the_table() {
    for name in ["timer", "changed", "on_rez"] {
        assert!(event(name).is_some(), "{name}");
    }
}

/// A sample argument of type `ty` and how `(string)` prints it.
fn sample(ty: TypeName) -> Value {
    match ty {
        TypeName::Integer => Value::Integer(7),
        TypeName::Float => Value::Float(1.5),
        TypeName::String => Value::String("s".to_owned()),
        TypeName::Key => Value::Key(crate::value::NULL_KEY.to_owned()),
        TypeName::Vector => Value::Vector(sl_types::lsl::Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        }),
        TypeName::Rotation => Value::Rotation(crate::value::ZERO_ROTATION),
        TypeName::List => Value::List(vec![crate::value::Element::Integer(4)]),
    }
}

#[test]
fn every_event_reaches_a_handler_with_its_parameters() -> Result<(), String> {
    let mut checked = 0_usize;
    for event in &crate::library::EVENTS {
        if matches!(event.name, "state_entry" | "state_exit") {
            continue;
        }
        let params: Vec<String> = event
            .args
            .iter()
            .enumerate()
            .map(|(i, arg)| format!("{} a{i}", arg.ty.keyword()))
            .collect();
        let printed: Vec<String> = (0..event.args.len())
            .map(|i| format!("(string)a{i}"))
            .collect();
        let body = if printed.is_empty() {
            format!("print({:?});", event.name)
        } else {
            format!("print({});", printed.join(" + \"|\" + "))
        };
        let source = format!(
            "default {{ {}({}) {{ {body} }} }}",
            event.name,
            params.join(", ")
        );
        let mut engine = engine();
        let mut host = Recorder::default();
        let _none = engine.add(
            A,
            program(&source).map_err(|e| format!("{}: {e}", event.name))?,
        );
        let expected = if is_detection_event(event.name) {
            let _posted = engine
                .post_detected(A, event, vec![Detected::blank()])
                .map_err(|e| e.to_string())?;
            "1".to_owned()
        } else {
            let args: Vec<Value> = event.args.iter().map(|arg| sample(arg.ty)).collect();
            let expected = if args.is_empty() {
                event.name.to_owned()
            } else {
                args.iter()
                    .map(|value| match crate::cast(value.clone(), TypeName::String) {
                        Ok(Value::String(text)) => text,
                        _ => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            };
            assert_eq!(
                engine.post(A, event, args),
                Ok(Some(Posted::Queued)),
                "{}",
                event.name
            );
            expected
        };
        let _reports = run(&mut engine, &mut host, 1);
        assert_eq!(host.texts(A), vec![expected.as_str()], "{}", event.name);
        checked = checked.saturating_add(1);
    }
    assert_eq!(checked, crate::library::EVENTS.len().saturating_sub(2));
    Ok(())
}

/// Three changes — two in one tick, one in the next — and two touches in
/// one tick, under `coalescing`; what the script saw.
fn arrivals(coalescing: Coalescing) -> Result<Vec<String>, String> {
    let mut engine = Engine::new(EngineConfig {
        coalescing,
        ..EngineConfig::for_step(STEP, EngineFlavour::SecondLife)
    });
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default {
                state_entry() { llSleep(0.3); }
                changed(integer change) { print("changed " + (string)change); }
                touch_start(integer n) { print("touch " + (string)n); }
            }"#,
        )?,
    );
    // The script sleeps through ticks 1-3, so everything below queues.
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    let posts = [
        engine.changed(A, 2),
        engine.changed(A, 8),
        engine.post_detected(A, touch, vec![toucher("a")]),
        engine.post_detected(A, touch, vec![toucher("b")]),
    ];
    let _reports = run(&mut engine, &mut host, 1);
    let late = engine.changed(A, 16);
    let _reports = run(&mut engine, &mut host, 5);
    let mut seen: Vec<String> = posts
        .iter()
        .chain([&late])
        .map(|posted| format!("{posted:?}"))
        .collect();
    seen.extend(host.texts(A).into_iter().map(str::to_owned));
    Ok(seen)
}

#[test]
fn a_test_can_force_every_arrival_shape_a_script_may_meet() -> Result<(), String> {
    let queued = "Ok(Some(Queued))";
    let merged = "Ok(Some(Merged))";
    // The reference: one tick's raises merge, and the touch waits ahead of
    // the `changed` raised before it — so the late change finds the queued
    // `changed` behind it, not next in line, and joins it.
    assert_eq!(
        arrivals(Coalescing::Reference)?,
        vec![
            queued,
            merged,
            queued,
            merged,
            merged,
            "touch 2",
            "changed 26"
        ]
    );
    // The most split-up arrival.
    assert_eq!(
        arrivals(Coalescing::Never)?,
        vec![
            queued,
            queued,
            queued,
            queued,
            queued,
            "changed 2",
            "changed 8",
            "touch 1",
            "touch 1",
            "changed 16"
        ]
    );
    // The most merged: the late change joins the one still queued.
    assert_eq!(
        arrivals(Coalescing::WhileQueued)?,
        vec![
            queued,
            merged,
            queued,
            merged,
            merged,
            "changed 26",
            "touch 2"
        ]
    );
    Ok(())
}

/// What a sleeping script ran, in order, of a touch, a `changed` and a link
/// message raised in that order while it slept, under `coalescing`.
fn queue_order(coalescing: Coalescing) -> Result<Vec<String>, String> {
    let mut engine = Engine::new(EngineConfig {
        coalescing,
        ..EngineConfig::for_step(STEP, EngineFlavour::SecondLife)
    });
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default {
                state_entry() { llSleep(1.0); }
                touch_start(integer n) { print("touch_start"); }
                touch_end(integer n) { print("touch_end"); }
                changed(integer change) { print("changed"); }
                link_message(integer sender, integer num, string msg, key id) { print(msg); }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = engine.post_detected(A, named("touch_start")?, vec![toucher("a")]);
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = engine.changed(A, 2);
    let _posted = link(&mut engine, A, 0, "link 1")?;
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = engine.post_detected(A, named("touch_end")?, vec![toucher("a")]);
    let _posted = link(&mut engine, A, 0, "link 2")?;
    let _reports = run(&mut engine, &mut host, 20);
    Ok(host.texts(A).into_iter().map(str::to_owned).collect())
}

#[test]
fn second_life_runs_touches_after_the_rest_and_changed_last() -> Result<(), String> {
    // `events/order` (2026-10-25): on Second Life a touch and a `changed`
    // raised while the script slept come after the link messages, chat,
    // timers and sensor answers of the same sleep, the `changed` last of all;
    // OpenSim runs every event in the order it was raised.
    assert_eq!(
        queue_order(Coalescing::Reference)?,
        vec!["link 1", "link 2", "touch_start", "touch_end", "changed"]
    );
    assert_eq!(
        queue_order(Coalescing::Never)?,
        vec!["touch_start", "changed", "link 1", "touch_end", "link 2"]
    );
    Ok(())
}

#[test]
fn a_changed_waits_behind_its_own_handlers_events_alone() -> Result<(), String> {
    // `events/order` round 1 (aditi, 2026-10-25): a link message and a
    // colour change raised while the handler slept, then the fence it sends
    // itself; the fence's handler sends a second fence. The `changed` waits
    // behind the link message and the first fence, raised with it, but not
    // behind the second, raised while a later handler ran.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "default {
                state_entry() { llSleep(1.0); }
                link_message(integer sender, integer num, string text, key id) {
                    print(\"link \" + (string)num);
                    if (num == 900) llSleep(1.0);
                }
                changed(integer change) { print(\"changed\"); }
            }",
        )?,
    );
    // Ten ticks to the second: all three raised in the first handler's
    // sleep, the second fence in the first fence's.
    let _reports = run(&mut engine, &mut host, 2);
    let _posted = link(&mut engine, A, 1, "")?;
    let _posted = engine.changed(A, 2);
    let _posted = link(&mut engine, A, 900, "")?;
    let _reports = run(&mut engine, &mut host, 14);
    let _posted = link(&mut engine, A, 901, "")?;
    let _reports = run(&mut engine, &mut host, 30);
    assert_eq!(
        host.texts(A),
        vec!["link 1", "link 900", "changed", "link 901"]
    );
    Ok(())
}

#[test]
fn pass_touches_is_handed_to_the_host_as_the_script_gave_it() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { state_entry() { llPassTouches(PASS_NEVER); llPassTouches(7); } }")?,
    );
    let _reports = run(&mut engine, &mut host, 2);
    let prim = host.prim_of(A);
    assert_eq!(host.passes.get(&prim), Some(&7));
    Ok(())
}

#[test]
fn pass_collisions_is_handed_to_the_host_as_the_script_gave_it() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { state_entry() { llPassCollisions(PASS_ALWAYS); } }")?,
    );
    let _reports = run(&mut engine, &mut host, 2);
    let prim = host.prim_of(A);
    assert_eq!(host.collision_passes.get(&prim), Some(&1));
    Ok(())
}

/// The movers of `physics/targets`: a tau of 0 moves nothing on either
/// grid, a target 70 m off moves nothing on Second Life and is gone to on
/// OpenSim, and a stop and a buoyancy are handed on as given.
#[test]
fn move_to_target_refuses_what_each_grid_refuses() -> Result<(), String> {
    let source = "default { state_entry() {
        llMoveToTarget(llGetPos() + <0.0, 0.0, 1.0>, 0.0);
        llMoveToTarget(llGetPos() + <0.0, 0.0, 70.0>, 0.3);
        llMoveToTarget(llGetPos() + <0.0, 0.0, 1.0>, 0.3);
        llStopMoveToTarget();
        llSetBuoyancy(1.0);
    } }";
    for (flavour, far_kept) in [
        (EngineFlavour::SecondLife, false),
        (EngineFlavour::YEngine, true),
    ] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program_for(source, flavour)?);
        let _reports = run(&mut engine, &mut host, 2);
        let rises: Vec<Option<f32>> = host
            .move_targets
            .iter()
            .map(|(_caller, target)| target.as_ref().map(|target| target.tau))
            .collect();
        let mut expected = Vec::new();
        if far_kept {
            expected.push(Some(0.3));
        }
        expected.extend([Some(0.3), None]);
        assert_eq!(rises, expected, "{flavour:?}");
        assert_eq!(host.buoyancies, vec![(A, 1.0)], "{flavour:?}");
    }
    Ok(())
}

#[test]
fn a_script_says_whether_its_state_handles_an_event() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "default { state_entry() { state other; } touch_start(integer n) { } }
             state other { touch_end(integer n) { } }",
        )?,
    );
    let handles = |engine: &Engine, event: &str| {
        engine
            .instance(A)
            .is_some_and(|instance| instance.handles(event))
    };
    assert!(handles(&engine, "touch_start"));
    assert!(!handles(&engine, "touch_end"));
    let _reports = run(&mut engine, &mut host, 2);
    assert!(!handles(&engine, "touch_start"));
    assert!(handles(&engine, "touch_end"));
    Ok(())
}

#[test]
fn a_later_change_joins_a_queued_one_unless_it_is_next_in_line() -> Result<(), String> {
    // aditi (2026-09-28), four touches alike: while the handler was busy,
    // scale, colour, (texture, which raised nothing) and scale again, a fifth
    // of a second apart, arrived as `8` and then `10`.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "default {
                state_entry() { llSleep(1.0); }
                changed(integer change) { print((string)change); }
            }",
        )?,
    );
    let mut posted = Vec::new();
    for bits in [8, 2, 8] {
        let _reports = run(&mut engine, &mut host, 2);
        posted.push(engine.changed(A, bits));
    }
    let _reports = run(&mut engine, &mut host, 10);
    assert_eq!(
        posted,
        vec![
            Ok(Some(Posted::Queued)),
            Ok(Some(Posted::Queued)),
            Ok(Some(Posted::Merged)),
        ]
    );
    assert_eq!(host.texts(A), vec!["8", "10"]);
    Ok(())
}

/// A region of [`STEP`] ticks imitating `flavour`.
fn engine_as(flavour: EngineFlavour) -> Engine {
    Engine::new(EngineConfig::for_step(STEP, flavour))
}

/// Post `link_message(0, num, msg, "")` to `id`.
fn link(engine: &mut Engine, id: CallerId, num: i32, msg: &str) -> Result<Option<Posted>, String> {
    engine
        .post(
            id,
            named("link_message")?,
            vec![
                Value::Integer(0),
                Value::Integer(num),
                Value::String(msg.to_owned()),
                Value::Key(String::new()),
            ],
        )
        .map_err(|error| error.to_string())
}

/// The `Div int` script of the probe `errors/runtime-errors`: link message 1
/// divides by its text, 100 asks for the global it set first.
const DIVIDER: &str = r#"integer g = 1;
default {
    link_message(integer sender, integer num, string msg, key id) {
        if (num == 100) print("g=" + (string)g);
        else { g = 42; print((string)(1 / (integer)msg)); }
    }
}"#;

/// A host whose one script `A` is called `Div int`.
fn divider_host() -> Recorder {
    Recorder {
        scripts: [("Div int".to_owned(), A)].into_iter().collect(),
        ..Recorder::default()
    }
}

#[test]
fn second_life_says_two_lines_stops_and_holds_the_script_until_a_reset() -> Result<(), String> {
    let mut engine = engine_as(EngineFlavour::SecondLife);
    let mut host = divider_host();
    let _none = engine.add(A, program(DIVIDER)?);
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(link(&mut engine, A, 1, "0")?, Some(Posted::Queued));
    let _reports = run(&mut engine, &mut host, 1);
    // aditi, 2026-10-23: the object's name and the script's, then the error.
    assert_eq!(
        host.heard(A),
        vec![
            "Faulty prim [script:Div int] Script run-time error",
            "Math Error"
        ]
    );
    // The host is handed the fault itself, with the line no grid tells.
    assert_eq!(
        host.faults
            .iter()
            .map(|(caller, fault)| (*caller, fault.to_string(), fault.position, fault.event))
            .collect::<Vec<_>>(),
        vec![(
            A,
            "Math Error".to_owned(),
            Some(Position {
                line: 5,
                column: 39
            }),
            Some("link_message")
        )]
    );
    assert!(host.said.iter().all(|(_, _, speech)| {
        speech.channel == DEBUG_CHANNEL
            && speech.to_avatars == ChatVolume::Say
            && speech.to_scripts == Some(ChatVolume::Say)
    }));
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(!instance.is_running());
    assert!(instance.is_held());
    assert!(!instance.reads_running(EngineFlavour::SecondLife));
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Stopped));
    // `llSetScriptState(TRUE)` alone: still stopped, still deaf.
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    assert!(
        !engine
            .instance(A)
            .ok_or("no instance")?
            .reads_running(EngineFlavour::SecondLife)
    );
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Stopped));
    // Reset after it: running again, from scratch.
    engine.instance_mut(A).ok_or("no instance")?.reset();
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Queued));
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["g=1"]);
    Ok(())
}

#[test]
fn second_life_releases_a_held_script_reset_first_then_started() -> Result<(), String> {
    let mut engine = engine_as(EngineFlavour::SecondLife);
    let mut host = divider_host();
    let _none = engine.add(A, program(DIVIDER)?);
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = link(&mut engine, A, 1, "0")?;
    let _reports = run(&mut engine, &mut host, 1);
    engine.instance_mut(A).ok_or("no instance")?.reset();
    // Reset but not started: the run flag the fault cleared stays clear.
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Stopped));
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Queued));
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["g=1"]);
    Ok(())
}

#[test]
fn yengine_says_one_line_to_avatars_and_holds_a_script_that_reads_running() -> Result<(), String> {
    let mut engine = engine_as(EngineFlavour::YEngine);
    let mut host = divider_host();
    let _none = engine.add(A, program(DIVIDER)?);
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = link(&mut engine, A, 1, "0")?;
    let _reports = run(&mut engine, &mut host, 1);
    // OpenSim YEngine, 2026-10-23, verbatim but for the prim's key.
    assert_eq!(
        host.heard(A),
        vec![
            "YEngine: Division by Zero (script: Div int event: link_message primID: \
             fab01e52-3517-4545-be1b-4cbf6d8ace3b at: <120,128,27>) Script must be Reset to \
             re-enable.\n"
        ]
    );
    assert!(
        host.said
            .iter()
            .all(|(_, _, speech)| speech.to_scripts.is_none())
    );
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(instance.is_running());
    assert!(instance.is_held());
    assert!(instance.reads_running(EngineFlavour::YEngine));
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Stopped));
    engine.instance_mut(A).ok_or("no instance")?.reset();
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = link(&mut engine, A, 100, "")?;
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["g=1"]);
    Ok(())
}

#[test]
fn xengine_says_one_line_and_the_script_runs_on_with_its_globals() -> Result<(), String> {
    let mut engine = engine_as(EngineFlavour::XEngine);
    let mut host = divider_host();
    let _none = engine.add(A, program(DIVIDER)?);
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = link(&mut engine, A, 1, "0")?;
    let _posted = link(&mut engine, A, 100, "")?;
    let _reports = run(&mut engine, &mut host, 1);
    // OpenSim XEngine, 2026-10-27: no space before the bracket, the
    // position as a vector.
    assert_eq!(
        host.heard(A),
        vec![
            "Integer division by Zero(script: Div int event: link_message \
             primID:fab01e52-3517-4545-be1b-4cbf6d8ace3b at <120, 128, 27.5>)"
        ]
    );
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(instance.is_running());
    assert!(!instance.is_held());
    // The event queued behind the fault is handled, in the state it
    // faulted in, with the global it set before.
    assert_eq!(host.texts(A), vec!["g=42"]);
    Ok(())
}

#[test]
fn opensim_names_the_kind_of_division() -> Result<(), String> {
    let source = r"default {
        link_message(integer sender, integer num, string msg, key id) {
            if (num == 1) print((string)(1.0 / (float)msg));
            else print((string)(<1.0, 2.0, 3.0> / (float)msg));
        }
    }";
    let cases = [
        (EngineFlavour::XEngine, 1, "Float division by zero(script:"),
        (EngineFlavour::XEngine, 2, "Vector division by zero(script:"),
        (
            EngineFlavour::YEngine,
            1,
            "YEngine: Division by Zero (script:",
        ),
        (
            EngineFlavour::YEngine,
            2,
            "YEngine: Vector division by zero (script:",
        ),
        (
            EngineFlavour::SecondLife,
            1,
            "Faulty prim [script:Script 1]",
        ),
        (
            EngineFlavour::SecondLife,
            2,
            "Faulty prim [script:Script 1]",
        ),
    ];
    for (flavour, num, start) in cases {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program(source)?);
        let _reports = run(&mut engine, &mut host, 1);
        let _posted = link(&mut engine, A, num, "0")?;
        let _reports = run(&mut engine, &mut host, 1);
        let heard = host.heard(A);
        assert!(
            heard.first().is_some_and(|line| line.starts_with(start)),
            "{flavour:?} case {num}: {heard:?}"
        );
    }
    Ok(())
}

#[test]
fn opensim_says_nothing_of_a_runaway_recursion() -> Result<(), String> {
    let source = "integer deep(integer depth) { return deep(depth + 1) + 1; }
        default { state_entry() { print((string)deep(0)); } }";
    for flavour in [EngineFlavour::YEngine, EngineFlavour::XEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program(source)?);
        let reports = run(&mut engine, &mut host, 4);
        assert!(reports.iter().any(|report| {
            outcomes(report, A).iter().any(|outcome| {
                matches!(
                    outcome,
                    Outcome::Faulted(Fault {
                        error: RuntimeError::StackHeapCollision,
                        ..
                    })
                )
            })
        }));
        assert!(host.heard(A).is_empty(), "{flavour:?}");
    }
    let mut engine = engine_as(EngineFlavour::SecondLife);
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    let _reports = run(&mut engine, &mut host, 4);
    assert_eq!(
        host.heard(A),
        vec![
            "Faulty prim [script:Script 1] Script run-time error",
            "Stack-Heap Collision"
        ]
    );
    Ok(())
}

#[test]
fn a_missing_script_is_refused_in_the_flavours_words_and_costs_a_second() -> Result<(), String> {
    let source = r#"default { state_entry() {
        print((string)llGetScriptState("Nope"));
        print("after");
    } }"#;
    let cases = [
        (
            EngineFlavour::SecondLife,
            "Could not find script 'Nope'.",
            ChatVolume::Say,
        ),
        (
            EngineFlavour::YEngine,
            "llGetScriptState: Can't find script 'Nope'",
            ChatVolume::Shout,
        ),
        (
            EngineFlavour::XEngine,
            "llGetScriptState: Can't find script 'Nope'",
            ChatVolume::Shout,
        ),
    ];
    for (flavour, text, reach) in cases {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program(source)?);
        let _reports = run(&mut engine, &mut host, 12);
        assert_eq!(host.heard(A), vec![text], "{flavour:?}");
        assert!(
            host.said
                .iter()
                .all(|(_, _, speech)| speech.to_scripts == Some(reach)),
            "{flavour:?}"
        );
        // The script goes on a second after the call, with its answer.
        assert_eq!(host.from(A), vec![(11, "0"), (11, "after")], "{flavour:?}");
        assert!(
            engine
                .instance(A)
                .ok_or("no instance")?
                .reads_running(flavour)
        );
    }
    Ok(())
}

/// Twelve refusals as fast as the script can, then one after 10 seconds of
/// quiet and one after 25 more: the probe `errors/flood`.
const FLOODER: &str = r#"integer i;
default { state_entry() {
    for (i = 1; i <= 12; ++i) llGetScriptState("Nope " + (string)i);
    llSleep(10.0);
    llGetScriptState("Later 1");
    llSleep(25.0);
    llGetScriptState("Later 2");
    print("done");
} }"#;

#[test]
fn second_life_drops_an_objects_error_lines_in_a_flood_until_it_quiets() -> Result<(), String> {
    let mut engine = engine_as(EngineFlavour::SecondLife);
    let mut host = Recorder {
        objects: [(B, "object-b".to_owned())].into_iter().collect(),
        ..Recorder::default()
    };
    let _none = engine.add(A, program(FLOODER)?);
    // A script in another object of the same owner's, mid-flood.
    let _none = engine.add(
        B,
        program(r#"default { state_entry() { llSleep(10.5); llGetScriptState("Other"); } }"#)?,
    );
    let _reports = run(&mut engine, &mut host, 520);
    assert_eq!(host.texts(A), vec!["done"]);
    let mut expected: Vec<String> = (1..=8)
        .map(|i| format!("Could not find script 'Nope {i}'."))
        .collect();
    expected.push(FLOOD_NOTICE.to_owned());
    expected.push("Could not find script 'Later 2'.".to_owned());
    assert_eq!(host.heard(A), expected);
    assert_eq!(host.heard(B), vec!["Could not find script 'Other'."]);
    Ok(())
}

#[test]
fn opensim_passes_every_error_line() -> Result<(), String> {
    let mut engine = engine_as(EngineFlavour::YEngine);
    let mut host = Recorder::default();
    let _none = engine.add(A, program(FLOODER)?);
    let _reports = run(&mut engine, &mut host, 520);
    assert_eq!(host.heard(A).len(), 14);
    Ok(())
}

#[test]
fn a_script_listening_on_debug_channel_hears_a_fault_and_the_region_ticks_on() -> Result<(), String>
{
    // The acceptance of `server-lsl-runtime-errors`, through a stand-in for
    // the region's chat fan-out: every line said that scripts hear is handed
    // to B's `listen`.
    let mut engine = engine_as(EngineFlavour::SecondLife);
    let mut host = divider_host();
    let _none = engine.add(A, program(DIVIDER)?);
    let _none = engine.add(
        B,
        program(
            r#"default { listen(integer channel, string name, key id, string message) {
                print((string)channel + " " + message);
            } }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = link(&mut engine, A, 1, "0")?;
    let _reports = run(&mut engine, &mut host, 1);
    let heard: Vec<Speech> = host
        .said
        .iter()
        .filter(|(_, _, speech)| speech.to_scripts.is_some())
        .map(|(_, _, speech)| speech.clone())
        .collect();
    for speech in heard {
        let _posted = engine
            .post(
                B,
                named("listen")?,
                vec![
                    Value::Integer(speech.channel),
                    Value::String("Faulty prim".to_owned()),
                    Value::Key("fab01e52-3517-4545-be1b-4cbf6d8ace3b".to_owned()),
                    Value::String(speech.text),
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(
        host.texts(B),
        vec![
            "2147483647 Faulty prim [script:Div int] Script run-time error",
            "2147483647 Math Error"
        ]
    );
    assert!(
        !engine
            .instance(A)
            .ok_or("no instance")?
            .reads_running(EngineFlavour::SecondLife)
    );
    Ok(())
}

/// A region imitating `flavour` whose budgets never run out, for counting
/// what a script charges.
fn unbounded(flavour: EngineFlavour) -> Engine {
    Engine::new(EngineConfig {
        script_budget: u32::MAX,
        region_budget: u64::MAX,
        ..EngineConfig::for_step(STEP, flavour)
    })
}

/// The script time `source` is charged under `flavour` to run its
/// `state_entry` to the end.
fn charged(source: &str, flavour: EngineFlavour) -> Result<u64, String> {
    let mut engine = unbounded(flavour);
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    let reports = run(&mut engine, &mut host, 2);
    Ok(reports.iter().map(|report| report.used).sum())
}

/// The per-call cost above an empty loop of the same shape, in nanoseconds,
/// of each `(function, size)` a `vm/call-cost-*` transcript holds: one call's
/// period less one empty iteration's at the same size.
fn measured_costs(transcript: &str) -> Vec<(String, u64, f64)> {
    let field = |line: &str, name: &str| -> Option<String> {
        line.split(' ')
            .find_map(|word| word.strip_prefix(name)?.strip_prefix('='))
            .map(ToOwned::to_owned)
    };
    let rates: Vec<(String, u64, f64)> = transcript
        .lines()
        .filter(|line| line.contains(" rec=rate "))
        .filter_map(|line| {
            let calls: f64 = field(line, "calls")?.parse().ok()?;
            let seconds: f64 = field(line, "seconds")?.parse().ok()?;
            Some((
                field(line, "fn")?,
                field(line, "size")?.parse().ok()?,
                seconds / calls * 1e9,
            ))
        })
        .collect();
    rates
        .iter()
        .filter(|(function, ..)| function != "none")
        .filter_map(|(function, size, period)| {
            let empty = rates
                .iter()
                .find(|(other, at, _)| other == "none" && at == size)?
                .2;
            Some((function.clone(), *size, period - empty))
        })
        .collect()
}

/// A script that runs `body` `count` times in a loop, with a global list
/// `L` of `size` integers, `P = [-1]`, `C` the `llList2CSV` of `L`, and a
/// global string `S` of `size` characters, all written out as initialisers
/// so filling them costs no joins.
fn call_cost_script(size: u64, count: u32, body: &str) -> String {
    let elements: Vec<String> = (0..size).map(|element| element.to_string()).collect();
    let text: String = "ab,"
        .chars()
        .cycle()
        .take(usize::try_from(size).unwrap_or(0))
        .collect();
    format!(
        "list L = [{list}]; list P = [-1]; list r; string C = \"{list}\"; string S = \"{text}\";
        string s; integer n; float f;
        default {{ state_entry() {{
            integer j;
            for (j = 0; j < {count}; ++j) {{ {body} }}
        }} }}",
        list = elements.join(", ")
    )
}

/// Each grid's `vm/call-cost-*` transcripts: lists, strings and the newer
/// functions (OpenSim compiles none of the last three it times, so its newer
/// transcript stops at the compile error and holds no rates).
const CALL_COST_TRANSCRIPTS: [(EngineFlavour, [&str; 3]); 3] = [
    (
        EngineFlavour::SecondLife,
        [
            include_str!("../../probes/results/aditi/vm/call-cost-lists.txt"),
            include_str!("../../probes/results/aditi/vm/call-cost-strings.txt"),
            include_str!("../../probes/results/aditi/vm/call-cost-newer.txt"),
        ],
    ),
    (
        EngineFlavour::YEngine,
        [
            include_str!("../../probes/results/opensim-yengine/vm/call-cost-lists.txt"),
            include_str!("../../probes/results/opensim-yengine/vm/call-cost-strings.txt"),
            include_str!("../../probes/results/opensim-yengine/vm/call-cost-newer.txt"),
        ],
    ),
    (
        EngineFlavour::XEngine,
        [
            include_str!("../../probes/results/opensim-xengine/vm/call-cost-lists.txt"),
            include_str!("../../probes/results/opensim-xengine/vm/call-cost-strings.txt"),
            include_str!("../../probes/results/opensim-xengine/vm/call-cost-newer.txt"),
        ],
    ),
];

/// What one grid's cells are held to.
struct CostRule {
    /// The grid.
    flavour: EngineFlavour,
    /// The cells left out, as function and size.
    outliers: &'static [(&'static str, u64)],
    /// The grid's noise floor, in nanoseconds.
    noise: f64,
    /// How many cells are checked.
    expected: usize,
}

#[test]
fn joins_and_library_calls_cost_what_they_cost_on_each_grid() -> Result<(), String> {
    const CALLS: u32 = 10;
    // The probes' own loop bodies.
    let bodies = [
        ("llGetListLength", "n = llGetListLength(L);"),
        ("llList2Integer", "n = llList2Integer(L, -1);"),
        ("llListSort", "r = llListSort(L, 1, TRUE);"),
        ("llList2CSV", "s = llList2CSV(L);"),
        ("llDumpList2String", "s = llDumpList2String(L, \",\");"),
        ("llListFindList", "n = llListFindList(L, P);"),
        ("llList2List", "r = llList2List(L, 0, -1);"),
        ("llDeleteSubList", "r = llDeleteSubList(L, 0, 0);"),
        ("llListReplaceList", "r = llListReplaceList(L, P, 0, 0);"),
        ("list-plus", "r = L + P;"),
        (
            "llListStatistics",
            "f = llListStatistics(LIST_STAT_SUM, L);",
        ),
        ("llCSV2List", "r = llCSV2List(C);"),
        ("llListRandomize", "r = llListRandomize(L, 1);"),
        ("llStringLength", "n = llStringLength(S);"),
        ("llGetSubString", "s = llGetSubString(S, -1, -1);"),
        ("llSubStringIndex", "n = llSubStringIndex(S, \"#\");"),
        (
            "llParseString2List",
            "r = llParseString2List(S, [\",\"], []);",
        ),
        ("llToUpper", "s = llToUpper(S);"),
        ("llStringToBase64", "s = llStringToBase64(S);"),
        ("llMD5String", "s = llMD5String(S, 0);"),
        ("llSHA1String", "s = llSHA1String(S);"),
        ("llEscapeURL", "s = llEscapeURL(S);"),
        ("string-plus", "s = S + \"x\";"),
        ("llInsertString", "s = llInsertString(S, 0, \"x\");"),
        ("llDeleteSubString", "s = llDeleteSubString(S, 0, 0);"),
        ("llStringTrim", "s = llStringTrim(S, STRING_TRIM);"),
        (
            "llReplaceSubString",
            "s = llReplaceSubString(S, \",\", \";\", 0);",
        ),
        ("llSHA256String", "s = llSHA256String(S);"),
        ("llComputeHash", "s = llComputeHash(S, \"sha256\");"),
        ("llOrd", "n = llOrd(S, -1);"),
        ("llHash", "n = llHash(S);"),
    ];
    // Each grid's cells `vm::cost` leaves out, and how near a charge must
    // come to the rest: within 5 %, or within the noise of the grid's empty
    // loop. aditi's outliers are each far from the other run of the same cell
    // (book: 4.2–7.9, 113–639, 63–661 µs); XEngine's empty loop is 2.3 µs and
    // its cells scatter by some 150 ns around it, YEngine's 35 ns by a few.
    let rules = [
        CostRule {
            flavour: EngineFlavour::SecondLife,
            outliers: &[
                ("llList2Integer", 1),
                ("llList2List", 10),
                ("llListReplaceList", 1),
            ],
            noise: 0.0,
            expected: 31 * 4 - 3,
        },
        CostRule {
            flavour: EngineFlavour::YEngine,
            outliers: &[],
            noise: 20.0,
            expected: 26 * 4,
        },
        CostRule {
            flavour: EngineFlavour::XEngine,
            outliers: &[],
            noise: 150.0,
            expected: 26 * 4,
        },
    ];
    for (flavour, transcripts) in CALL_COST_TRANSCRIPTS {
        let Some(CostRule {
            outliers,
            noise,
            expected,
            ..
        }) = rules.iter().find(|rule| rule.flavour == flavour)
        else {
            return Err(format!("no rule for {flavour:?}"));
        };
        let mut checked = 0_usize;
        for (function, size, measured) in transcripts.iter().flat_map(|t| measured_costs(t)) {
            if outliers.contains(&(function.as_str(), size)) {
                continue;
            }
            let Some((_, body)) = bodies.iter().find(|(name, _)| *name == function) else {
                continue;
            };
            let with = charged(&call_cost_script(size, CALLS, body), flavour)?;
            let without = charged(&call_cost_script(size, CALLS, ""), flavour)?;
            let per_call =
                f64::from(u32::try_from(with.saturating_sub(without)).unwrap_or(u32::MAX))
                    / f64::from(CALLS);
            assert!(
                (per_call - measured).abs() <= (measured.abs() * 0.05).max(*noise),
                "{flavour:?} {function} at {size}: charged {per_call:.0} ns, measured {measured:.0} ns"
            );
            checked = checked.saturating_add(1);
        }
        // Four sizes of each function timed, but the outliers.
        assert_eq!(checked, *expected, "{flavour:?}");
    }
    Ok(())
}

#[test]
fn a_join_dearer_than_a_share_sits_its_script_out_until_it_is_paid_for() -> Result<(), String> {
    let step = Duration::from_millis(1);
    let config = EngineConfig::for_step(step, EngineFlavour::SecondLife);
    let mut engine = Engine::new(config);
    let mut host = Recorder::default();
    let source = call_cost_script(500, 1, "r = L + P; print(\"joined\");");
    let _none = engine.add(A, program(&source)?);
    let join = cost::speed(EngineFlavour::SecondLife)
        .list_join()
        .charge(501);
    let share = config.script_budget;
    assert!(
        join > share.saturating_mul(4),
        "the join must outlast shares"
    );
    let _reports = run(&mut engine, &mut host, u64::from(join / share + 4));
    let printed = host.from(A);
    let [(tick, "joined")] = printed.as_slice() else {
        return Err(format!("printed {printed:?}"));
    };
    // The tick the join is paid off in, or the one after it when the loop's
    // own instructions tip it over.
    let paid = u64::from(join.div_ceil(share));
    assert!(
        (paid..=paid.saturating_add(1)).contains(tick),
        "joined at tick {tick}, paid off at {paid}"
    );
    Ok(())
}

#[test]
fn a_regions_budget_is_its_engines_shares_whatever_the_step() {
    for (flavour, shares) in [
        (EngineFlavour::SecondLife, 130),
        (EngineFlavour::YEngine, 2),
        (EngineFlavour::XEngine, 100),
    ] {
        for step in [
            Duration::from_millis(1),
            Duration::from_millis(22),
            Duration::from_secs(1),
        ] {
            let config = EngineConfig::for_step(step, flavour);
            assert_eq!(
                u128::from(config.script_budget),
                step.as_nanos(),
                "{flavour:?} step {step:?}"
            );
            assert_eq!(
                config.region_budget,
                u64::from(config.script_budget).saturating_mul(shares),
                "{flavour:?} step {step:?}"
            );
        }
    }
}

#[test]
fn script_time_counts_ticks_from_a_reset_and_runs_on_while_stopped() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r"default {
                touch_start(integer n) { print((string)llGetTime()); }
                touch_end(integer n) { print((string)llGetAndResetTime()); }
                touch(integer n) { llResetTime(); print((string)llGetTime()); }
            }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 5);
    let _posted = engine.post_detected(A, named("touch_start")?, vec![toucher("a")]);
    let _reports = run(&mut engine, &mut host, 1);
    if let Some(instance) = engine.instance_mut(A) {
        instance.set_running(false);
    }
    let _reports = run(&mut engine, &mut host, 4);
    if let Some(instance) = engine.instance_mut(A) {
        instance.set_running(true);
    }
    let _posted = engine.post_detected(A, named("touch_end")?, vec![toucher("a")]);
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = engine.post_detected(A, named("touch")?, vec![toucher("a")]);
    let _reports = run(&mut engine, &mut host, 1);
    // Six ticks of 0.1 s, then four more stopped and one to run in; reset,
    // and read in the tick it was reset in.
    assert_eq!(host.texts(A), vec!["0.600000", "1.100000", "0.000000"]);
    Ok(())
}
