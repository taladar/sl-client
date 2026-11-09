//! Touches: what a script is told of a mouse button held on its prim.
//!
//! A viewer says it with three messages — `ObjectGrab` at the press, a
//! stream of `ObjectGrabUpdate`s while the button is held and an
//! `ObjectDeGrab` at the release — and a script hears `touch_start`, `touch`
//! and `touch_end`. What lies between is the region's, and the two live
//! grids were measured to do it differently (`object-touch-grab`,
//! 2026-10-12, written up in `book/src/gridspec/building.md` § Touch and
//! grab); [`TouchPolicy`] states the difference and this module applies it.
//!
//! - **Second Life holds a touch open** from the press to the release and
//!   raises `touch` on the region's own clock while it is open, 22.5 times a
//!   second whatever the viewer sends meanwhile, and once for a click too
//!   short for the clock. A second press with a touch open is ignored, and an
//!   update with none open opens one.
//! - **OpenSim answers each message**: a `touch_start` for every press, a
//!   `touch` for every update and nothing between them.
//!
//! The same three messages also drag a prim nothing takes a touch in
//! (`crate::grabs`); this module only listens to them, so the update that
//! raises a `touch` here still reaches the drag there.
//!
//! # Which scripts are told
//!
//! A touch on the root, or on a prim with no root, goes to its own scripts. A
//! touch on a child goes to the child's scripts when one of them takes a
//! touch (has a touch handler in its current state), and to the root's as
//! well as the child's `llPassTouches` says; a child whose scripts take none
//! passes it to the root unless it said `PASS_NEVER`, which on Second Life
//! leaves nobody told. Every script told reads the link number of the prim
//! that was clicked (`llDetectedLinkNumber`), `0` for a prim alone.
//!
//! # What a script detects
//!
//! The toucher's key, name and type; where the avatar stands; whether its
//! active group is the object's; the surface block of the message the event
//! reads, as the viewer sent it — no grid computes a touch position or a
//! normal — and the grab offset as each grid states it ([`GrabReading`]).
//!
//! # The flags a viewer reads
//!
//! A prim one of whose running scripts takes a touch carries
//! `FLAGS_HANDLE_TOUCH`, which a viewer reads to show a touch cursor and the
//! drag of `crate::grabs` reads to leave the prim where it is.
//! `refresh_flags` keeps it true after every tick's reconcile, with the
//! child-prim rule of each grid ([`ChildFlags`]).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use sl_lsl_runtime::vm::{CallerId, Detected, Touch};
use sl_proto::{
    AgentKey, ObjectGrabMoved, ObjectGrabbed, ObjectReleased, RegionLocalObjectId, ServerEvent,
    SimSession, SurfaceInfo, prim_flags,
};
use sl_types::key::ObjectKey;
use sl_types::lsl::Vector;

use crate::world::{AvatarIdentity, RegionChange, SceneFixtures};

/// The three touch events, by name.
const TOUCH_EVENTS: [&str; 3] = ["touch_start", "touch", "touch_end"];

/// `PASS_ALWAYS`: a child's touch reaches the root as well.
const PASS_ALWAYS: i32 = 1;

/// `PASS_NEVER`: a child's touch never reaches the root.
const PASS_NEVER: i32 = 2;

/// `llDetectedType`'s `AGENT` bit.
const AGENT: i32 = 1;

/// `llDetectedType`'s `ACTIVE` bit.
const ACTIVE: i32 = 2;

/// How a grid turns a held button into touch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldTouch {
    /// Second Life: a press opens a touch and a release closes it, and while
    /// it is open the region raises `touch` once every `every` of its own
    /// clock, whatever the viewer sends; a click shorter than that still
    /// raises one. A second press with a touch open is ignored, and an update
    /// with none open opens one.
    OnRegionClock {
        /// The time between two `touch` events of a held button.
        every: Duration,
    },
    /// OpenSim: every press is a `touch_start`, every update a `touch` and
    /// every release a `touch_end`, and nothing comes between them.
    PerMessage,
}

/// What `llDetectedGrab` reads in each touch event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabReading {
    /// Second Life: zero in a `touch_start` and a `touch_end`; in a `touch`
    /// the latest update's grab position less the prim's position and less
    /// the update's initial grab offset.
    SecondLife,
    /// OpenSim: the press's grab offset in a `touch_start`, the update's grab
    /// position less the prim's position in a `touch`, zero in a `touch_end`.
    OpenSim,
}

/// What a child prim's `llPassTouches` setting means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassRule {
    /// Second Life: `PASS_ALWAYS` passes a touch the child takes on to the
    /// root, and `PASS_NEVER` keeps from the root even one the child does not
    /// take; anything else passes only what the child does not take.
    SecondLife,
    /// OpenSim: any value but `0` passes (`PASS_NEVER` included), and a touch
    /// the child does not take always reaches the root.
    AnyButZero,
}

/// Which flags a child prim carries for a root that takes touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildFlags {
    /// Second Life: `FLAGS_SCRIPTED` and `FLAGS_HANDLE_TOUCH` both, and the
    /// child is not sent again for it — a viewer that does not ask keeps the
    /// flags it had.
    TouchQuietly,
    /// OpenSim: `FLAGS_SCRIPTED` alone, for any script in the root, and the
    /// child is sent again in full.
    ScriptedResent,
}

/// How a grid routes and reports a touch (`book/src/gridspec/building.md`
/// § Touch and grab).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPolicy {
    /// How a held button becomes events.
    pub held: HeldTouch,
    /// What `llDetectedGrab` reads.
    pub grab: GrabReading,
    /// What `llPassTouches` means.
    pub pass: PassRule,
    /// `llDetectedType` of an avatar standing still: `AGENT | ACTIVE` on
    /// Second Life, `AGENT` on OpenSim.
    pub agent_type: i32,
    /// What a child of a touch-taking root carries.
    pub child_flags: ChildFlags,
}

impl TouchPolicy {
    /// Second Life's, as measured on aditi: 22.5 `touch` events a second.
    pub const SECOND_LIFE: Self = Self {
        held: HeldTouch::OnRegionClock {
            every: Duration::from_nanos(44_444_444),
        },
        grab: GrabReading::SecondLife,
        pass: PassRule::SecondLife,
        agent_type: AGENT | ACTIVE,
        child_flags: ChildFlags::TouchQuietly,
    };

    /// OpenSim's, as measured on the local grid (YEngine).
    pub const OPENSIM: Self = Self {
        held: HeldTouch::PerMessage,
        grab: GrabReading::OpenSim,
        pass: PassRule::AnyButZero,
        agent_type: AGENT,
        child_flags: ChildFlags::ScriptedResent,
    };
}

impl Default for TouchPolicy {
    /// Second Life's.
    fn default() -> Self {
        Self::SECOND_LIFE
    }
}

/// A touch held open on Second Life: who holds it and what the next `touch`
/// reads.
#[derive(Debug, Clone, PartialEq)]
struct Held {
    /// The toucher as a script detects it, with the latest message's surface
    /// and grab offset.
    detected: Detected,
    /// The region time gathered towards the next `touch`.
    credit: Duration,
    /// Whether a `touch` has been raised for it yet.
    touched: bool,
}

/// The region's touches: those held open, and what each prim's scripts set
/// `llPassTouches` to.
#[derive(Debug, Clone, Default)]
pub struct Touches {
    /// How the region's grid touches.
    pub(crate) policy: TouchPolicy,
    /// The touches held open, by toucher and prim (Second Life's).
    held: BTreeMap<(AgentKey, RegionLocalObjectId), Held>,
    /// What each prim's scripts last set `llPassTouches` to; a prim not here
    /// is at `PASS_IF_NOT_HANDLED`.
    passes: BTreeMap<ObjectKey, i32>,
    /// The prims whose touch flags the region last set beyond what their
    /// own contents say — inherited by a child, or `FLAGS_HANDLE_TOUCH` —
    /// so a later refresh knows to look at them again.
    flagged: BTreeSet<ObjectKey>,
    /// The programs each linkset root ran at the last refresh: OpenSim sends
    /// a root's children again whenever they change.
    root_scripts: BTreeMap<ObjectKey, Vec<sl_proto::AssetKey>>,
}

impl Touches {
    /// No touches, under `policy`.
    #[must_use]
    pub(crate) fn new(policy: TouchPolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    /// The prim `prim`'s `llPassTouches` setting is now `pass`.
    pub(crate) fn set_pass(&mut self, prim: ObjectKey, pass: i32) {
        let _previous = self.passes.insert(prim, pass);
    }

    /// How many touches are held open.
    #[must_use]
    pub fn held_open(&self) -> usize {
        self.held.len()
    }
}

/// Who touched, as the session that sent a touch message knows it.
pub(crate) struct Toucher<'a> {
    /// The avatar.
    pub(crate) identity: &'a AvatarIdentity,
    /// Its active group, if any.
    pub(crate) group: Option<sl_types::key::GroupKey>,
}

/// Raises what one drained [`ServerEvent`] — a press, an update or a release
/// of a button on a prim — tells the prim's scripts, as the region's grid
/// does. Returns whether the event was a press or a release, which nothing
/// else answers; an update is left for the drag (`crate::grabs`) as well.
pub(crate) fn observe(
    world: &mut SceneFixtures,
    toucher: &Toucher<'_>,
    sim: &SimSession,
    event: &ServerEvent,
) -> bool {
    match event {
        ServerEvent::ObjectGrabbed(press) => {
            press_on(world, toucher, sim, press);
            true
        }
        ServerEvent::ObjectGrabMoved(update) => {
            update_on(world, toucher, sim, update);
            false
        }
        ServerEvent::ObjectReleased(release) => {
            release_on(world, toucher, sim, release);
            true
        }
        _ => false,
    }
}

/// A press on a prim: `touch_start`, unless Second Life has a touch open.
fn press_on(
    world: &mut SceneFixtures,
    toucher: &Toucher<'_>,
    sim: &SimSession,
    press: &ObjectGrabbed,
) {
    let local_id = press.local_id;
    let Some(base) = detect(world, toucher, sim, local_id) else {
        return;
    };
    let policy = world.touches.policy;
    let grab = match policy.grab {
        GrabReading::SecondLife => sl_lsl_runtime::ZERO_VECTOR,
        GrabReading::OpenSim => press.grab_offset.clone(),
    };
    let detected = with_surface(base, press.surfaces.first(), grab);
    if let HeldTouch::OnRegionClock { .. } = policy.held {
        let key = (toucher.identity.agent_id, local_id);
        if world.touches.held.contains_key(&key) {
            return;
        }
        let _opened = world.touches.held.insert(
            key,
            Held {
                detected: Detected {
                    grab: sl_lsl_runtime::ZERO_VECTOR,
                    ..detected.clone()
                },
                credit: Duration::ZERO,
                touched: false,
            },
        );
    }
    raise(world, local_id, "touch_start", detected);
}

/// An update of a held button: on OpenSim a `touch`; on Second Life what the
/// next `touch` reads, opening a touch where none is open.
fn update_on(
    world: &mut SceneFixtures,
    toucher: &Toucher<'_>,
    sim: &SimSession,
    update: &ObjectGrabMoved,
) {
    let Some(local_id) = world.local_id_of(update.object_id) else {
        return;
    };
    let Some(base) = detect(world, toucher, sim, local_id) else {
        return;
    };
    let policy = world.touches.policy;
    let prim_at = prim_position(world, local_id);
    let from_prim = minus(&update.grab_position, &prim_at);
    match policy.held {
        HeldTouch::PerMessage => {
            let detected = with_surface(base, update.surfaces.first(), from_prim);
            raise(world, local_id, "touch", detected);
        }
        HeldTouch::OnRegionClock { .. } => {
            let grab = match policy.grab {
                GrabReading::SecondLife => minus(&from_prim, &update.grab_offset_initial),
                GrabReading::OpenSim => from_prim,
            };
            let latest = with_surface(base, update.surfaces.first(), grab);
            let key = (toucher.identity.agent_id, local_id);
            if let Some(held) = world.touches.held.get_mut(&key) {
                held.detected = latest;
                return;
            }
            let _opened = world.touches.held.insert(
                key,
                Held {
                    detected: latest.clone(),
                    credit: Duration::ZERO,
                    touched: false,
                },
            );
            let start = Detected {
                grab: sl_lsl_runtime::ZERO_VECTOR,
                ..latest
            };
            raise(world, local_id, "touch_start", start);
        }
    }
}

/// A release: on Second Life the `touch` a click too short for the clock is
/// owed, then `touch_end` — which both grids raise for a release with no press
/// before it.
fn release_on(
    world: &mut SceneFixtures,
    toucher: &Toucher<'_>,
    sim: &SimSession,
    release: &ObjectReleased,
) {
    let local_id = release.local_id;
    let Some(base) = detect(world, toucher, sim, local_id) else {
        return;
    };
    if let Some(held) = world
        .touches
        .held
        .remove(&(toucher.identity.agent_id, local_id))
        && !held.touched
    {
        raise(world, local_id, "touch", held.detected);
    }
    let detected = with_surface(base, release.surfaces.first(), sl_lsl_runtime::ZERO_VECTOR);
    raise(world, local_id, "touch_end", detected);
}

/// One tick of `step` of the region's clock: a `touch` for every touch held
/// open that is due one. A touch whose prim left the region, or whose toucher
/// is not in it (`present`), is closed without one.
pub(crate) fn tick(world: &mut SceneFixtures, step: Duration, present: &[AgentKey]) {
    let HeldTouch::OnRegionClock { every } = world.touches.policy.held else {
        return;
    };
    let objects = &world.objects;
    world.touches.held.retain(|(agent, local_id), _held| {
        present.contains(agent) && objects.get(*local_id).is_some()
    });
    let mut due = Vec::new();
    for ((_agent, local_id), held) in &mut world.touches.held {
        held.credit = held.credit.saturating_add(step);
        if held.credit < every {
            continue;
        }
        // Raised more than once in a tick they would merge into one event.
        while held.credit >= every {
            held.credit = held.credit.saturating_sub(every);
        }
        held.touched = true;
        due.push((*local_id, held.detected.clone()));
    }
    for (local_id, detected) in due {
        raise(world, local_id, "touch", detected);
    }
}

/// The toucher as a script in the prim `local_id` detects it, with no surface
/// and no grab yet; [`None`] for a prim the region does not have.
fn detect(
    world: &SceneFixtures,
    toucher: &Toucher<'_>,
    sim: &SimSession,
    local_id: RegionLocalObjectId,
) -> Option<Detected> {
    world.objects.get(local_id)?;
    let root = world.root_of(local_id);
    // The two groups as they are: no group on either side is the same
    // group (`events/other-avatar` on aditi; OpenSim compares the ids, both
    // null).
    let same_group = toucher.group == world.record_of(root).and_then(|record| record.group);
    let agent = toucher.identity.agent_id.uuid().to_string();
    Some(Detected {
        key: agent.clone(),
        name: format!(
            "{} {}",
            toucher.identity.first_name, toucher.identity.last_name
        ),
        owner: agent,
        kind: world.touches.policy.agent_type,
        same_group,
        link_number: link_number(world, local_id),
        position: crate::chat::agent_position(sim),
        ..Detected::blank()
    })
}

/// `detected` with the surface block `surface` — or none — and the grab
/// offset `grab`.
fn with_surface(detected: Detected, surface: Option<&SurfaceInfo>, grab: Vector) -> Detected {
    let touch = surface.map_or(Touch::INVALID, |surface| Touch {
        face: surface.face_index,
        st: flat(surface.st),
        uv: flat(surface.uv),
        position: surface.position.clone(),
        normal: surface.normal.clone(),
        binormal: surface.binormal.clone(),
    });
    Detected {
        grab,
        touch,
        ..detected
    }
}

/// A texture or surface coordinate as the vector a script reads.
const fn flat([x, y]: [f32; 2]) -> Vector {
    Vector { x, y, z: 0.0 }
}

/// `left - right`.
fn minus(left: &Vector, right: &Vector) -> Vector {
    Vector {
        x: left.x - right.x,
        y: left.y - right.y,
        z: left.z - right.z,
    }
}

/// Where the prim `local_id` is in the region: a root's own position, a
/// child's offset turned and put by its root.
fn prim_position(world: &SceneFixtures, local_id: RegionLocalObjectId) -> Vector {
    let Some(prim) = world.objects.get(local_id) else {
        return sl_lsl_runtime::ZERO_VECTOR;
    };
    let root = world.root_of(local_id);
    if root == local_id {
        return prim.motion.position.clone();
    }
    let Some(root) = world.objects.get(root) else {
        return prim.motion.position.clone();
    };
    let turned = crate::object_edits::rotate(&root.motion.rotation, &prim.motion.position);
    Vector {
        x: root.motion.position.x + turned.x,
        y: root.motion.position.y + turned.y,
        z: root.motion.position.z + turned.z,
    }
}

/// The link number of the prim `local_id`: `0` for a prim alone — no other
/// prim and nobody sitting — else its place in link order, the root first.
fn link_number(world: &SceneFixtures, local_id: RegionLocalObjectId) -> i32 {
    let root = world.root_of(local_id);
    let prims: Vec<RegionLocalObjectId> = std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .collect();
    if prims.len() <= 1 && crate::seating::sitters_of(world, root).is_empty() {
        return 0;
    }
    prims
        .iter()
        .zip(1_i32..)
        .find_map(|(prim, number)| (*prim == local_id).then_some(number))
        .unwrap_or(0)
}

/// The scripts of the prim `prim`.
fn scripts_of(world: &SceneFixtures, prim: ObjectKey) -> Vec<CallerId> {
    world.script_instances.scripts_in(prim)
}

/// Whether one of the running scripts of `prim` takes a touch: has a handler
/// for any of the three touch events in its current state.
pub(crate) fn takes_touches(world: &SceneFixtures, prim: ObjectKey) -> bool {
    scripts_of(world, prim).into_iter().any(|id| {
        world
            .script_instances
            .engine()
            .instance(id)
            .is_some_and(|instance| {
                instance.is_running()
                    && !instance.is_held()
                    && TOUCH_EVENTS.iter().any(|event| instance.handles(event))
            })
    })
}

/// The scripts a touch on the prim `local_id` is told to, in link order.
fn told(world: &SceneFixtures, local_id: RegionLocalObjectId) -> Vec<CallerId> {
    let Some(prim) = world.objects.get(local_id).map(|object| object.full_id) else {
        return Vec::new();
    };
    let root = world.root_of(local_id);
    if root == local_id {
        return scripts_of(world, prim);
    }
    let Some(root_key) = world.objects.get(root).map(|object| object.full_id) else {
        return scripts_of(world, prim);
    };
    let pass = world.touches.passes.get(&prim).copied().unwrap_or_default();
    let (child, to_root) = if takes_touches(world, prim) {
        let passes = match world.touches.policy.pass {
            PassRule::SecondLife => pass == PASS_ALWAYS,
            PassRule::AnyButZero => pass != 0,
        };
        (true, passes)
    } else {
        let passes = match world.touches.policy.pass {
            PassRule::SecondLife => pass != PASS_NEVER,
            PassRule::AnyButZero => true,
        };
        (false, passes)
    };
    let mut scripts = Vec::new();
    if to_root {
        scripts.extend(scripts_of(world, root_key));
    }
    if child {
        scripts.extend(scripts_of(world, prim));
    }
    scripts
}

/// Raises `event` with `detected` in every script a touch on `local_id` is
/// told to.
fn raise(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    event: &str,
    detected: Detected,
) {
    let Some(event) = sl_lsl_runtime::library::event(event) else {
        tracing::error!(event, "a touch event the runtime does not know");
        return;
    };
    let told = told(world, local_id);
    tracing::trace!(%local_id, event = event.name, ?told, "a touch");
    for id in told {
        match world
            .script_instances
            .engine_mut()
            .post_detected(id, event, vec![detected.clone()])
        {
            Ok(posted) => tracing::trace!(?id, ?posted, "a touch posted"),
            Err(error) => tracing::warn!(?id, %error, "a touch could not be posted"),
        }
    }
}

/// Brings every scripted prim's touch flags in line with its scripts — after
/// a reconcile, a save, a state change — and returns the prims a viewer has
/// to be sent again for it, each in full.
///
/// A prim carries `FLAGS_HANDLE_TOUCH` when one of its running scripts takes a
/// touch, and `FLAGS_SCRIPTED` when it holds a script; a child of a root that
/// takes touches carries what [`ChildFlags`] says. Only the prims the region
/// runs scripts in, the ones it flagged before, and their linksets are
/// looked at: a fixture flagged by hand keeps its flags.
pub(crate) fn refresh_flags(world: &mut SceneFixtures) -> Vec<RegionChange> {
    // A prim that left takes its setting with it.
    let objects = &world.objects;
    world
        .touches
        .passes
        .retain(|prim, _pass| objects.get_by_key(*prim).is_some());
    let mut roots: BTreeSet<RegionLocalObjectId> = BTreeSet::new();
    let watched: Vec<ObjectKey> = world
        .script_instances
        .prims()
        .into_iter()
        .chain(world.touches.flagged.iter().copied())
        .collect();
    for prim in watched {
        if let Some(local_id) = world.local_id_of(prim) {
            let _new = roots.insert(world.root_of(local_id));
        }
    }
    let child_flags = world.touches.policy.child_flags;
    let mut flagged = BTreeSet::new();
    let mut changes = Vec::new();
    for root in roots {
        let Some(root_key) = world.objects.get(root).map(|object| object.full_id) else {
            continue;
        };
        let root_takes = takes_touches(world, root_key);
        let root_scripted = holds_a_script(world, root);
        // OpenSim sends every child again as its root's scripts change
        // (`SceneObjectGroup.aggregateScriptEvents`), whether or not its
        // flags did.
        let assets = world.script_instances.assets_in(root_key);
        let root_changed = world.touches.root_scripts.get(&root_key) != Some(&assets);
        if assets.is_empty() {
            let _gone = world.touches.root_scripts.remove(&root_key);
        } else {
            let _previous = world.touches.root_scripts.insert(root_key, assets);
        }
        let prims: Vec<RegionLocalObjectId> = std::iter::once(root)
            .chain(crate::link_sets::children_in_order(world, root))
            .collect();
        for local_id in prims {
            let Some(key) = world.objects.get(local_id).map(|object| object.full_id) else {
                continue;
            };
            let child = local_id != root;
            let own_takes = takes_touches(world, key);
            let own_scripted = holds_a_script(world, local_id);
            let (inherited, resend_inherited) = match child_flags {
                ChildFlags::TouchQuietly if child && root_takes => {
                    (prim_flags::SCRIPTED | prim_flags::HANDLE_TOUCH, false)
                }
                ChildFlags::ScriptedResent if child && root_scripted => {
                    (prim_flags::SCRIPTED, true)
                }
                ChildFlags::TouchQuietly | ChildFlags::ScriptedResent => (0, false),
            };
            let own_contents = if own_scripted {
                prim_flags::SCRIPTED
            } else {
                0
            };
            let mut wanted = inherited | own_contents;
            if own_takes {
                wanted |= prim_flags::HANDLE_TOUCH;
            }
            // A flag the prim's own contents do not explain is the region's
            // to clear again later.
            if wanted & !own_contents != 0 {
                let _new = flagged.insert(key);
            }
            let Some(mut object) = world.object_mut(local_id) else {
                continue;
            };
            let mask = prim_flags::SCRIPTED | prim_flags::HANDLE_TOUCH;
            let had = object.update_flags & mask;
            let resent_with_root = child && root_changed && resend_inherited;
            if had == wanted && !resent_with_root {
                continue;
            }
            object.update_flags = (object.update_flags & !mask) | wanted;
            let own_changed =
                (had ^ wanted) & !inherited != 0 || (resend_inherited && had != wanted);
            let quiet = !own_changed
                && !resent_with_root
                && child
                && child_flags == ChildFlags::TouchQuietly;
            let sent = object.clone();
            drop(object);
            if quiet {
                // Not announced now, and not by the tick's update burst
                // either ([`crate::updates`]).
                world.objects.amend_shown(local_id, |shown| {
                    shown.update_flags = (shown.update_flags & !mask) | wanted;
                });
            } else {
                changes.push(RegionChange::Updated(Box::new(sent)));
            }
        }
    }
    world.touches.flagged = flagged;
    changes
}

/// Whether the prim `local_id` holds a script item.
fn holds_a_script(world: &SceneFixtures, local_id: RegionLocalObjectId) -> bool {
    world.objects.contents(local_id).is_some_and(|contents| {
        contents
            .items
            .iter()
            .any(|item| item.inv_type == sl_proto::InventoryType::Script)
    })
}
