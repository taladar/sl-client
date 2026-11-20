//! Enough physics for the physics-shaped events: a physical object falls,
//! lands and rests, a scripted prim it lands on hears `collision_start`,
//! `collision` and `collision_end`, a prim under `llVolumeDetect` hears what
//! passes through it, the ground raises `land_collision_*`, and
//! `llMoveToTarget` draws an object to its target.
//!
//! The solver is kinematic and purpose-built rather than a rigid-body engine
//! (`book/src/simulator/physics.md`): every prim is the box its scale and
//! rotation bound, a physical object is the union of its prims' boxes, and a
//! tick moves each physical object, in local-id order, axis by axis against
//! the solid prims and the ground, stopping it at the first surface in its
//! way. There is no contact resolution, no stacking, no friction and no
//! rotation; there is a bounce where the imitated grid bounces. What a tick
//! does is a function of the region alone, so two runs of one seed agree.
//!
//! # What the grids showed (`events/collision`, `physics/targets`)
//!
//! A prim dropped two metres onto a scripted one:
//!
//! - **Second Life** raised one `collision_start`, about eleven `collision`
//!   and one `collision_end` on each side, the end with the faller still lying
//!   there: Havok puts a body at rest to sleep, and a sleeping body touches
//!   nothing ([`Rest::SleepsAfter`]). A script asleep while the whole
//!   collision happened heard none of it ([`BusyCollisions::Lost`]).
//! - **OpenSim** (ubODE) raised each twice — the prim bounced, wood on wood
//!   being 0.5 × 0.5 restitution above ubODE's 1.5 m/s bounce velocity, and
//!   the second landing at 1.35 m/s too slow to bounce again
//!   ([`Bounce::Materials`]) — and the faller never heard its second contact
//!   end: a body that rests keeps touching ([`Rest::KeepsTouching`]), and a
//!   body made non-physical drops its own contacts unheard while what it
//!   touched hears them end. A busy script heard all of it once it woke
//!   ([`BusyCollisions::Queued`]).
//! - On both, a prim under `llVolumeDetect` heard a start and an end and no
//!   `collision` as the faller fell through it, and the faller heard nothing
//!   of it; the faller's `llDetectedType` was 10 (`ACTIVE | SCRIPTED`).
//!
//! An avatar standing still on a scripted prim (`events/collision-standing`)
//! never stops touching it: one `collision_start`, then `collision` for as
//! long as it stands there, Second Life 6–8 a second and OpenSim 17–19
//! ([`PhysicsPolicy::agent_collision_interval`]); two avatars stepping on at
//! once were one `collision_start` detecting both on every grid.
//!
//! `llMoveToTarget` with a tau of 0.3 reached a target a metre up within a
//! tenth of a metre in 0.69 s on Second Life and 0.71 s on OpenSim: the
//! distance left shrinks by `e` every tau, against gravity.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use sl_lsl_runtime::Value;
use sl_lsl_runtime::vm::physics::{
    Character, CollisionFilter, Goal, Hover, Inertia, Keyframes, LookAt, MASS_UNIT_KG,
    PhysicsMaterial,
};
use sl_lsl_runtime::vm::prim::MoveTarget;
use sl_lsl_runtime::vm::{CallerId, Detected};
use sl_proto::{Object, RegionLocalObjectId, prim_flags};
use sl_types::key::{AgentKey, ObjectKey};
use sl_types::lsl::{Rotation, Vector};

use crate::keyframes::Player;
use crate::vehicles::Vehicle;
use crate::world::SceneFixtures;

/// The pull of gravity, in metres a second squared.
pub(crate) const GRAVITY: f32 = 9.806_65;

/// How fast a drag pulls a physical prim to the grab, and a spin turns it:
/// the time constant, seconds.
const GRAB_TAU: f32 = 0.25;

/// How long a drag's pull or a spin's turn holds after its last update.
pub(crate) const GRAB_HOLD: Duration = Duration::from_millis(500);

/// How close two surfaces stand to count as touching, in metres.
const CONTACT: f32 = 0.01;

/// The speed under which a physical object counts as at rest, in metres a
/// second.
const REST_SPEED: f32 = 0.01;

/// How far a physical object has to move in a tick to count as moving, in
/// metres.
const MOVING: f32 = 0.000_1;

/// Half the box an avatar collides as, the one the controller walks it as
/// ([`crate::movement::AVATAR_HALF_WIDTH_M`]).
const AVATAR_HALF: Vector = Vector {
    x: crate::movement::AVATAR_HALF_WIDTH_M,
    y: crate::movement::AVATAR_HALF_WIDTH_M,
    z: crate::world::AVATAR_CENTRE_ABOVE_GROUND_M,
};

/// How far round an avatar the controller is handed the solid prims, metres.
const AVATAR_REACH: f32 = 16.0;

/// How far a region reaches east and north, in metres.
const REGION_EDGE: f32 = 256.0;

/// `llDetectedType`'s `ACTIVE` bit: a physical object.
const ACTIVE: i32 = 2;

/// `llDetectedType`'s `PASSIVE` bit: an object that is not physical.
const PASSIVE: i32 = 4;

/// `llDetectedType`'s `SCRIPTED` bit.
const SCRIPTED: i32 = 8;

/// `PASS_NEVER`.
const PASS_NEVER: i32 = 2;

/// `PASS_ALWAYS`.
const PASS_ALWAYS: i32 = 1;

/// Whether a physical object bounces off what it lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bounce {
    /// It stops where it lands (Second Life's Havok, `events/collision`).
    Never,
    /// It bounces back at the product of the two materials' restitutions,
    /// the ground's being `ground` times the object's, when it lands faster
    /// than `above` metres a second (OpenSim's ubODE: `ODEScene`'s material
    /// table, `TerrainBounce` and `bounce_vel`).
    Materials {
        /// The ground's share of the object's restitution.
        ground: f32,
        /// The slowest landing that bounces, in metres a second.
        above: f32,
    },
}

/// What a physical object at rest does with what it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rest {
    /// At rest this long it sleeps, and a sleeping object touches nothing: its
    /// collisions end (Second Life).
    SleepsAfter(Duration),
    /// It keeps touching what it rests on (OpenSim).
    KeepsTouching,
}

/// What becomes of a collision event for a script busy in a handler — asleep
/// in one, typically — when the collision begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyCollisions {
    /// The script hears nothing of that collision, its start, its
    /// `collision` events or its end (Second Life).
    Lost,
    /// Its events wait in the script's queue like any other (OpenSim).
    Queued,
}

/// Where `land_collision_*` says the object touched the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandPoint {
    /// The ground under the object's root (Second Life: "the position of the
    /// collision with the ground").
    Ground,
    /// The object's root itself (OpenSim's `CreateDetObjectForGround`).
    Root,
}

/// How the imitated grid's physics shows in what scripts hear.
#[expect(
    clippy::module_name_repetitions,
    reason = "read at its use sites as `ImitatedGrid::physics_policy() -> PhysicsPolicy`, \
              beside the other per-flavour policies"
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one measured difference between the grids' physics"
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsPolicy {
    /// Whether a landing object bounces.
    pub bounce: Bounce,
    /// What an object at rest does with its contacts.
    pub rest: Rest,
    /// What a busy script hears of a collision.
    pub busy: BusyCollisions,
    /// What `land_collision_*` is handed.
    pub land_point: LandPoint,
    /// Whether a physical object raises `moving_start` and `moving_end` as
    /// it starts and stops. OpenSim raises them for keyframed motion alone
    /// (`KeyframeMotion` is the only caller of `TriggerMovingStartEvent`,
    /// and `physics/keyframed`'s prim, made physical mid-motion, fell without
    /// a second `moving_start`); Second Life's, unmeasured, is taken as its
    /// documentation states it.
    pub moving_events: bool,
    /// Whether an impulse or a push given to a prim that is not physical is
    /// kept and applied when it turns physical (OpenSim, `physics/forces`:
    /// (1 + 5) / 1.25 m/s up the moment it did) or lost (Second Life).
    pub impulses_wait: bool,
    /// Whether a force set while the object is not physical still pushes it
    /// once it turns physical (Second Life's, unmeasured) or is kept for
    /// `llGetForce` alone (OpenSim, `physics/forces`: 0.25 N on a weightless
    /// 1.25 kg prim left it still).
    pub force_survives_physics: bool,
    /// How the object's mass is reckoned.
    pub mass: MassRule,
    /// Whether `VEHICLE_TYPE_NONE` stops a moving vehicle at once (Second
    /// Life, `physics/vehicles`) or lets it coast (OpenSim: 6 m/s half a
    /// second later).
    pub vehicle_none_stops: bool,
    /// How long after a change of its root's physics shape a keyframed
    /// motion waits to start ([`None`]: it starts at once). Second Life's
    /// first motion after the shape was made convex started late
    /// (`physics/keyframed`: 0.43–0.46 m of a 0.5 m frame done 1.3 s after
    /// the call — 0.38 to 0.44 s late); the runtime changes the shape after
    /// `llSetPrimitiveParams`'s forced delay, at the call's moment, so the
    /// wait is counted from there.
    pub keyframed_wait_after_shape: Option<Duration>,
    /// Whether a drag pulls a physical prim towards the grab (Second Life:
    /// pulled along) or leaves it (OpenSim) — `object-touch-grab`, leg 10.
    pub grab_pulls_physical: bool,
    /// Whether `KFM_CMD_PLAY` with no motion on the object plays the last
    /// one given it (Second Life, `physics/keyframed`: the one given while
    /// it was physical) or does nothing (OpenSim, whose motion is gone).
    pub play_resumes_last: bool,
    /// The least time between two `collision` events a prim raises for an
    /// avatar that goes on touching it. An avatar never rests the way a
    /// physical object does, so its contact keeps raising them for as long as
    /// it stands there, on both grids, but neither raises one every frame
    /// (`events/collision-standing`, an avatar standing still on a prim for
    /// eight seconds): Second Life 6–8 a second (59 in 8 s), OpenSim 17–19
    /// under both engines.
    pub agent_collision_interval: Duration,
}

/// How an object's mass follows its prims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassRule {
    /// Each prim's density times its box's volume, the scale taken from the
    /// prim's physics shape, which is rebuilt at once after a change of
    /// scale unless one was rebuilt within `rebuild` before, and then
    /// `rebuild` after that one (Second Life, `physics/material`: a cube
    /// scaled to 1 m read 10 at once, then 10 still at 2 m, at 4 × 0.5 × 0.5
    /// m and in the two reads after it was scaled back, the third 1.25 —
    /// each read a `llSetPrimitiveParams` later, which at the measured
    /// forced delays puts them 0.47, 0.91 and 1.36 s after the first change:
    /// a window between 0.91 and 1.36 s, of which the fake grid takes 1 s).
    FollowsDensity {
        /// The least time between two rebuilds.
        rebuild: Duration,
    },
    /// What the prims weighed when the region first saw them, density and
    /// scale changes notwithstanding (OpenSim under ubODE, `physics/material`).
    AsRezzed,
}

impl PhysicsPolicy {
    /// Second Life's, measured on aditi (`events/collision`, 2026-10-25). An
    /// object sleeps a quarter of a second after it comes to rest: the eleven
    /// `collision` events aditi raised between a landing's start and its end,
    /// at its 45 frames a second.
    pub const SECOND_LIFE: Self = Self {
        bounce: Bounce::Never,
        rest: Rest::SleepsAfter(Duration::from_millis(250)),
        busy: BusyCollisions::Lost,
        land_point: LandPoint::Ground,
        moving_events: true,
        impulses_wait: false,
        force_survives_physics: true,
        mass: MassRule::FollowsDensity {
            rebuild: Duration::from_secs(1),
        },
        vehicle_none_stops: true,
        keyframed_wait_after_shape: Some(Duration::from_millis(400)),
        play_resumes_last: true,
        grab_pulls_physical: true,
        // 59 `collision` events in 8 s: 7.4 a second, every sixth frame of
        // its 45.
        agent_collision_interval: Duration::from_millis(133),
    };

    /// OpenSim's under ubODE, measured on the local grid under both engines
    /// (`events/collision`, 2026-10-25) and read from `ODEScene` and
    /// `SceneObjectPart.PhysicsCollision`.
    pub const OPENSIM: Self = Self {
        bounce: Bounce::Materials {
            ground: 0.001,
            above: 1.5,
        },
        rest: Rest::KeepsTouching,
        busy: BusyCollisions::Queued,
        land_point: LandPoint::Root,
        moving_events: false,
        impulses_wait: true,
        force_survives_physics: false,
        mass: MassRule::AsRezzed,
        vehicle_none_stops: false,
        keyframed_wait_after_shape: None,
        play_resumes_last: false,
        grab_pulls_physical: false,
        // `SceneObjectPart` subscribes a scripted prim's physics actor to its
        // collisions at 50 ms ("20 reports per second"), and ubODE reports
        // on the first 20 ms step past that: every 60 ms, measured 17–19 a
        // second.
        agent_collision_interval: Duration::from_millis(50),
    };
}

impl Default for PhysicsPolicy {
    /// Second Life's.
    fn default() -> Self {
        Self::SECOND_LIFE
    }
}

/// What scripts set on an object that its physics reads.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one script setting of its own"
)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settings {
    /// `llSetBuoyancy`: the share of gravity it does not feel.
    pub(crate) buoyancy: f32,
    /// `llMoveToTarget`: where it is drawn while physical.
    pub(crate) target: Option<MoveTarget>,
    /// `llSetForce`: the steady force, as set.
    pub(crate) force: Vector,
    /// Whether the force is in the object's frame.
    pub(crate) force_local: bool,
    /// Whether the force pushes the object ([`PhysicsPolicy::force_survives_physics`]).
    pub(crate) force_applies: bool,
    /// `llSetTorque`: the steady torque, as set.
    pub(crate) torque: Vector,
    /// Whether the torque is in the object's frame.
    pub(crate) torque_local: bool,
    /// `llSetHoverHeight` or `llGroundRepel`.
    pub(crate) hover: Option<Hover>,
    /// `llLookAt` or `llRotLookAt` on a physical object.
    pub(crate) look_at: Option<LookAt>,
    /// The vehicle, once a vehicle function was called.
    pub(crate) vehicle: Option<Vehicle>,
    /// Impulses waiting for the next step (or, on OpenSim, for physics),
    /// region axes.
    pub(crate) impulse: Vector,
    /// Rotational impulses waiting likewise.
    pub(crate) angular_impulse: Vector,
    /// `llSetVelocity` for the next step, region axes.
    pub(crate) velocity: Option<Vector>,
    /// `llSetAngularVelocity` for the next step, region axes.
    pub(crate) omega: Option<Vector>,
    /// `osSetInertia*`.
    pub(crate) inertia: Option<Inertia>,
    /// `osSetPrimFloatOnWater`: the water holds it up.
    pub(crate) float_on_water: bool,
    /// A drag's pull: where to and until when, physics time.
    pub(crate) grab: Option<(Vector, Duration)>,
    /// A spin's turn: to what and until when, physics time.
    pub(crate) spin: Option<(Rotation, Duration)>,
}

impl Default for Settings {
    /// Gravity in full, nothing else.
    fn default() -> Self {
        Self {
            buoyancy: 0.0,
            target: None,
            force: ZERO,
            force_local: false,
            force_applies: false,
            torque: ZERO,
            torque_local: false,
            hover: None,
            look_at: None,
            vehicle: None,
            impulse: ZERO,
            angular_impulse: ZERO,
            velocity: None,
            omega: None,
            inertia: None,
            float_on_water: false,
            grab: None,
            spin: None,
        }
    }
}

/// A physical object's motion.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Body {
    /// Its velocity, in metres a second.
    velocity: Vector,
    /// Its spin, radians a second about each region axis.
    pub(crate) omega: Vector,
    /// Its acceleration over the last step.
    pub(crate) acceleration: Vector,
    /// How long it has been at rest.
    rest: Duration,
    /// Whether it sleeps ([`Rest::SleepsAfter`]).
    asleep: bool,
    /// Whether it moved last tick, for `moving_start` and `moving_end`.
    moving: bool,
}

impl Default for Body {
    /// At rest, awake, not moving.
    fn default() -> Self {
        Self {
            velocity: ZERO,
            omega: ZERO,
            acceleration: ZERO,
            rest: Duration::ZERO,
            asleep: false,
            moving: false,
        }
    }
}

/// What a prim touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Collider {
    /// Another object's prim.
    Prim(RegionLocalObjectId),
    /// An avatar.
    Agent(AgentKey),
    /// The ground.
    Land,
}

/// One prim touching one thing: what its scripts hear about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Contact {
    /// The prim whose scripts hear of it.
    prim: RegionLocalObjectId,
    /// What it touches.
    with: Collider,
}

/// A contact as it stands.
#[derive(Debug, Clone, PartialEq)]
struct Touching {
    /// What the prim's scripts detect of what it touches; for the ground,
    /// [`Detected::blank`] with the point it touched as the position.
    detected: Detected,
    /// Whether the prim is under `llVolumeDetect`: a start and an end, and no
    /// `collision` between.
    quiet: bool,
    /// The scripts that were busy when it began, and hear nothing of it
    /// ([`BusyCollisions::Lost`]).
    missed: BTreeSet<CallerId>,
    /// When, on the physics clock, the prim's scripts last heard of it — its
    /// start or its last `collision` — which paces an avatar's
    /// ([`PhysicsPolicy::agent_collision_interval`]).
    told: Duration,
}

/// The region's physics: what scripts set on its objects, how each physical
/// object moves, and what each prim touches.
#[derive(Debug, Clone, Default)]
pub struct Physics {
    /// How the imitated grid behaves.
    pub(crate) policy: PhysicsPolicy,
    /// What scripts set on each object, by its root's key.
    settings: BTreeMap<ObjectKey, Settings>,
    /// Each physical object's motion, by its root's key.
    bodies: BTreeMap<ObjectKey, Body>,
    /// What each prim's scripts last set `llPassCollisions` to.
    passes: BTreeMap<ObjectKey, i32>,
    /// What each prim touched at the end of the last tick.
    contacts: BTreeMap<Contact, Touching>,
    /// Each prim's `llCollisionFilter`.
    filters: BTreeMap<ObjectKey, CollisionFilter>,
    /// Each object's keyframed motion, by its root's key.
    pub(crate) players: BTreeMap<ObjectKey, Player>,
    /// Second Life's last motion given to each object, which
    /// `KFM_CMD_PLAY` plays when nothing is.
    pub(crate) last_motion: BTreeMap<ObjectKey, Keyframes>,
    /// When each root's physics shape last changed, physics time.
    pub(crate) shape_changed: BTreeMap<ObjectKey, Duration>,
    /// Each prim's physics shape as it was last rebuilt
    /// ([`MassRule::FollowsDensity`]).
    rebuilt: BTreeMap<ObjectKey, Rebuilt>,
    /// What each object weighed when first seen ([`MassRule::AsRezzed`]).
    rezzed_mass: BTreeMap<ObjectKey, f32>,
    /// Each pathfinding character, by its root's key.
    pub(crate) characters: BTreeMap<ObjectKey, Walker>,
    /// When an object last moved another by `osTeleportObject`, physics
    /// time, by the moved object and the mover.
    pub(crate) teleports: BTreeMap<(ObjectKey, ObjectKey), Duration>,
    /// How much physics time has passed.
    pub(crate) clock: Duration,
    /// The events a script's physics call raised in the linkset of a root,
    /// raised once the engine the tick lends out is back.
    pub(crate) raised: Vec<(RegionLocalObjectId, &'static str)>,
}

/// A prim's physics shape as last rebuilt, and the scale waiting for the
/// next rebuild.
#[derive(Debug, Clone, PartialEq)]
struct Rebuilt {
    /// The scale it was built at.
    scale: Vector,
    /// When, physics time.
    at: Duration,
    /// The scale the prim has had since, waiting for a rebuild.
    pending: Option<Vector>,
}

/// A pathfinding character: its settings and where it goes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Walker {
    /// Its settings.
    pub(crate) settings: Character,
    /// Where it goes, [`None`] for standing.
    pub(crate) goal: Option<Goal>,
    /// The point of a patrol or a wander it goes to now.
    pub(crate) leg: usize,
    /// Whether this leg's `PU_SLOWDOWN_DISTANCE_REACHED` was raised.
    pub(crate) slowing: bool,
    /// A jump's upward speed for the next step.
    pub(crate) jump: Option<f32>,
}

impl Physics {
    /// A region's physics under `policy`, nothing moving yet.
    #[must_use]
    pub fn new(policy: PhysicsPolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    /// `llPassCollisions` in the prim `prim`.
    pub(crate) fn set_pass(&mut self, prim: ObjectKey, pass: i32) {
        let _previous = self.passes.insert(prim, pass);
    }

    /// `llMoveToTarget` or, for [`None`], `llStopMoveToTarget` on the object
    /// whose root is `object`.
    pub(crate) fn set_target(&mut self, object: ObjectKey, target: Option<MoveTarget>) {
        self.settings.entry(object).or_default().target = target;
    }

    /// `llSetBuoyancy` on the object whose root is `object`.
    pub(crate) fn set_buoyancy(&mut self, object: ObjectKey, buoyancy: f32) {
        self.settings.entry(object).or_default().buoyancy = buoyancy;
    }

    /// What scripts set on the object whose root is `object`, to change.
    pub(crate) fn settings_mut(&mut self, object: ObjectKey) -> &mut Settings {
        self.settings.entry(object).or_default()
    }

    /// What scripts set on the object whose root is `object`.
    pub(crate) fn settings_of(&self, object: ObjectKey) -> Option<&Settings> {
        self.settings.get(&object)
    }

    /// The motion of the physical object whose root is `object`.
    pub(crate) fn body_of(&self, object: ObjectKey) -> Option<&Body> {
        self.bodies.get(&object)
    }

    /// `llCollisionFilter` in the prim `prim`.
    pub(crate) fn set_filter(&mut self, prim: ObjectKey, filter: Option<CollisionFilter>) {
        match filter {
            Some(filter) => {
                let _previous = self.filters.insert(prim, filter);
            }
            None => {
                let _previous = self.filters.remove(&prim);
            }
        }
    }

    /// The prim `prim`'s scale changed to `scale` ([`MassRule::FollowsDensity`]):
    /// its physics shape is rebuilt now, or once the last rebuild is
    /// `rebuild` old.
    pub(crate) fn rescaled(&mut self, prim: ObjectKey, scale: &Vector) {
        let MassRule::FollowsDensity { rebuild } = self.policy.mass else {
            return;
        };
        let now = self.clock;
        match self.rebuilt.get_mut(&prim) {
            Some(built) if now.saturating_sub(built.at) < rebuild => {
                built.pending = Some(scale.clone());
            }
            _ => {
                let _previous = self.rebuilt.insert(
                    prim,
                    Rebuilt {
                        scale: scale.clone(),
                        at: now,
                        pending: None,
                    },
                );
            }
        }
    }

    /// The scale the prim `prim`'s mass is reckoned from, its scale now
    /// `scale`.
    fn mass_scale(&self, prim: ObjectKey, scale: &Vector) -> Vector {
        let MassRule::FollowsDensity { rebuild } = self.policy.mass else {
            return scale.clone();
        };
        match self.rebuilt.get(&prim) {
            Some(built) => match &built.pending {
                Some(pending) if self.clock.saturating_sub(built.at) >= rebuild => pending.clone(),
                _ => built.scale.clone(),
            },
            None => scale.clone(),
        }
    }

    /// Folds each pending rebuild whose time has come into its prim's shape.
    fn rebuild_due(&mut self) {
        let MassRule::FollowsDensity { rebuild } = self.policy.mass else {
            return;
        };
        let now = self.clock;
        for built in self.rebuilt.values_mut() {
            if now.saturating_sub(built.at) >= rebuild
                && let Some(pending) = built.pending.take()
            {
                built.scale = pending;
                built.at = now;
            }
        }
    }
}

/// The mass of the object whose root is `root`, in Linden's units, as the
/// imitated grid reckons it ([`MassRule`]); OpenSim's inertia data, once set,
/// gives it instead.
pub(crate) fn mass_of(world: &SceneFixtures, root: RegionLocalObjectId) -> f32 {
    let Some(key) = world.objects.get(root).map(|object| object.full_id) else {
        return 0.0;
    };
    let physics = &world.physics;
    if let Some(inertia) = physics
        .settings
        .get(&key)
        .and_then(|settings| settings.inertia.as_ref())
        && physics.policy.mass == MassRule::AsRezzed
    {
        return inertia.mass;
    }
    if physics.policy.mass == MassRule::AsRezzed
        && let Some(mass) = physics.rezzed_mass.get(&key)
    {
        return *mass;
    }
    density_mass(world, root)
}

/// The mass the prims of the linkset `root` have now: each one's density
/// times the volume of its box, its scale the one its physics shape was
/// last built at.
fn density_mass(world: &SceneFixtures, root: RegionLocalObjectId) -> f32 {
    std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .filter_map(|local_id| world.objects.get(local_id))
        .map(|prim| {
            let density = world.objects.scripted(prim.local_id).map_or_else(
                || PhysicsMaterial::default().density,
                |scripted| scripted.prim.physics_material.density,
            );
            let scale = world.physics.mass_scale(prim.full_id, &prim.scale);
            density * scale.x * scale.y * scale.z / MASS_UNIT_KG
        })
        .sum()
}

/// The material of the prim `local_id`.
fn material_of(world: &SceneFixtures, local_id: RegionLocalObjectId) -> PhysicsMaterial {
    world
        .objects
        .scripted(local_id)
        .map_or_else(PhysicsMaterial::default, |scripted| {
            scripted.prim.physics_material
        })
}

/// The `STATUS_ROTATE_*` flags of the prim `local_id`: which axes it may
/// turn about while physical.
fn rotate_flags(world: &SceneFixtures, local_id: RegionLocalObjectId) -> (bool, bool, bool) {
    let status = world
        .objects
        .scripted(local_id)
        .map_or(sl_lsl_runtime::vm::prim::STATUS_ROTATE_XYZ, |scripted| {
            scripted.prim.status
        });
    (status & 2 != 0, status & 4 != 0, status & 8 != 0)
}

/// An avatar standing in the region, as physics sees it.
#[derive(Debug, Clone)]
pub(crate) struct Avatar {
    /// The agent.
    pub(crate) agent: AgentKey,
    /// Its full name.
    pub(crate) name: String,
    /// Where it stands: the centre of its body.
    pub(crate) position: Vector,
}

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// An axis of the region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// East.
    X,
    /// North.
    Y,
    /// Up.
    Z,
}

/// The three axes, in the order an object moves along them.
const AXES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

/// `v` along `axis`.
const fn along(v: &Vector, axis: Axis) -> f32 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

/// `v` with its `axis` component set to `value`.
const fn with(v: &Vector, axis: Axis, value: f32) -> Vector {
    match axis {
        Axis::X => Vector {
            x: value,
            y: v.y,
            z: v.z,
        },
        Axis::Y => Vector {
            x: v.x,
            y: value,
            z: v.z,
        },
        Axis::Z => Vector {
            x: v.x,
            y: v.y,
            z: value,
        },
    }
}

/// `a + b`.
fn plus(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// `a - b`.
fn minus(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `v × k`.
fn scaled(v: &Vector, k: f32) -> Vector {
    Vector {
        x: v.x * k,
        y: v.y * k,
        z: v.z * k,
    }
}

/// The length of `v`.
fn length(v: &Vector) -> f32 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

/// An axis-aligned box, region coordinates.
#[derive(Debug, Clone, PartialEq)]
struct Aabb {
    /// Its south-west-lower corner.
    min: Vector,
    /// Its north-east-upper corner.
    max: Vector,
}

impl Aabb {
    /// The box `half` either side of `centre`.
    fn around(centre: &Vector, half: &Vector) -> Self {
        Self {
            min: minus(centre, half),
            max: plus(centre, half),
        }
    }

    /// The box moved by `by`.
    fn shifted(&self, by: &Vector) -> Self {
        Self {
            min: plus(&self.min, by),
            max: plus(&self.max, by),
        }
    }

    /// The smallest box holding both.
    const fn union(&self, other: &Self) -> Self {
        Self {
            min: Vector {
                x: self.min.x.min(other.min.x),
                y: self.min.y.min(other.min.y),
                z: self.min.z.min(other.min.z),
            },
            max: Vector {
                x: self.max.x.max(other.max.x),
                y: self.max.y.max(other.max.y),
                z: self.max.z.max(other.max.z),
            },
        }
    }

    /// Whether the two overlap along `axis` by more than `slack` (a negative
    /// slack admits a gap that wide).
    fn overlap_on(&self, other: &Self, axis: Axis, slack: f32) -> bool {
        along(&self.min, axis) < along(&other.max, axis) - slack
            && along(&other.min, axis) < along(&self.max, axis) - slack
    }

    /// Whether the two stand within [`CONTACT`] of each other on every axis.
    fn touches(&self, other: &Self) -> bool {
        AXES.iter()
            .all(|axis| self.overlap_on(other, *axis, -CONTACT))
    }

    /// Whether the two overlap on every axis.
    fn overlaps(&self, other: &Self) -> bool {
        AXES.iter().all(|axis| self.overlap_on(other, *axis, 0.0))
    }

    /// Its centre.
    const fn centre(&self) -> Vector {
        Vector {
            x: f32::midpoint(self.min.x, self.max.x),
            y: f32::midpoint(self.min.y, self.max.y),
            z: f32::midpoint(self.min.z, self.max.z),
        }
    }
}

/// What a prim is to a moving object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// It stops it.
    Solid,
    /// It lets it through and hears it pass (`llVolumeDetect`).
    VolumeDetect,
}

/// A prim that takes part: solid or volume-detecting, not phantom, not worn.
#[derive(Debug, Clone)]
struct Solid {
    /// The prim.
    local_id: RegionLocalObjectId,
    /// Its linkset's root.
    root: RegionLocalObjectId,
    /// The box it fills.
    aabb: Aabb,
    /// What it is.
    kind: Kind,
}

/// The box the prim `object` fills in the region.
fn box_of(world: &SceneFixtures, object: &Object) -> Aabb {
    let (centre, rotation) = crate::seating::frame_of(world, object);
    let half = scaled(&object.scale, 0.5);
    let reach = |axis: Vector| {
        let turned = crate::object_edits::rotate(&rotation, &axis);
        Vector {
            x: turned.x.abs(),
            y: turned.y.abs(),
            z: turned.z.abs(),
        }
    };
    let ex = reach(Vector {
        x: half.x,
        y: 0.0,
        z: 0.0,
    });
    let ey = reach(Vector {
        x: 0.0,
        y: half.y,
        z: 0.0,
    });
    let ez = reach(Vector {
        x: 0.0,
        y: 0.0,
        z: half.z,
    });
    Aabb::around(&centre, &plus(&plus(&ex, &ey), &ez))
}

/// Whether the prim `local_id` is under `llVolumeDetect`.
fn volume_detects(world: &SceneFixtures, local_id: RegionLocalObjectId) -> bool {
    world
        .objects
        .scripted(local_id)
        .is_some_and(|scripted| scripted.prim.volume_detect)
}

/// Every prim of the region that takes part, in the store's order.
fn solids(world: &SceneFixtures) -> Vec<Solid> {
    world
        .objects
        .iter()
        .filter(|object| object.pcode == sl_proto::pcode::PRIMITIVE)
        .filter_map(|object| {
            let root = world.root_of(object.local_id);
            if world.objects.attached_to(root).is_some() {
                return None;
            }
            let kind = if volume_detects(world, object.local_id) {
                Kind::VolumeDetect
            } else if object.update_flags & prim_flags::PHANTOM != 0 {
                return None;
            } else {
                Kind::Solid
            };
            Some(Solid {
                local_id: object.local_id,
                root,
                aabb: box_of(world, object),
                kind,
            })
        })
        .collect()
}

/// The solid prims within [`AVATAR_REACH`] of an avatar centred at `near`,
/// as the boxes the avatar controller stands on and is stopped by
/// ([`crate::movement`]): neither phantom nor volume-detecting, not worn.
pub(crate) fn avatar_blocks(world: &SceneFixtures, near: &Vector) -> Vec<crate::movement::Block> {
    let around = Aabb::around(
        near,
        &Vector {
            x: AVATAR_REACH,
            y: AVATAR_REACH,
            z: AVATAR_REACH,
        },
    );
    solids(world)
        .into_iter()
        .filter(|solid| solid.kind == Kind::Solid && solid.aabb.overlaps(&around))
        .map(|solid| crate::movement::Block {
            min: solid.aabb.min,
            max: solid.aabb.max,
        })
        .collect()
}

/// The roots of the region's physical objects, in local-id order, worn ones
/// left out.
fn physical_roots(world: &SceneFixtures) -> Vec<RegionLocalObjectId> {
    let mut roots: Vec<RegionLocalObjectId> = world
        .objects
        .iter()
        .filter(|object| object.parent_id.0 == 0)
        .filter(|object| object.pcode == sl_proto::pcode::PRIMITIVE)
        .filter(|object| object.update_flags & prim_flags::USE_PHYSICS != 0)
        .filter(|object| world.objects.attached_to(object.local_id).is_none())
        .map(|object| object.local_id)
        .collect();
    roots.sort_unstable();
    roots
}

/// The collision events, a script handling any of which makes its prim's
/// contacts worth working out.
const COLLISION_EVENTS: [&str; 6] = [
    "collision_start",
    "collision",
    "collision_end",
    "land_collision_start",
    "land_collision",
    "land_collision_end",
];

/// Whether any running script of the region handles a collision event.
fn anybody_listens(world: &SceneFixtures) -> bool {
    let scripts = &world.script_instances;
    scripts.prims().into_iter().any(|prim| {
        scripts.scripts_in(prim).into_iter().any(|id| {
            scripts.engine().instance(id).is_some_and(|instance| {
                COLLISION_EVENTS.iter().any(|event| instance.handles(event))
            })
        })
    })
}

/// What a tick of physics reads of its region.
pub(crate) struct Region<'a> {
    /// The avatars standing in it.
    pub(crate) avatars: &'a [Avatar],
    /// The height of the ground at a point.
    pub(crate) ground: &'a dyn Fn(f32, f32) -> f32,
    /// The height of the water.
    pub(crate) water: f32,
}

/// One tick of the region's physics, `step` long: every keyframed motion
/// plays its share, every physical object moves, and every prim's scripts
/// hear what it began, went on and stopped touching. Runs inside the tick's
/// update burst ([`crate::updates`]), so what moved goes out with it.
pub(crate) fn step(world: &mut SceneFixtures, region: &Region<'_>, step: Duration) {
    let dt = step.as_secs_f32();
    world.physics.clock = world.physics.clock.saturating_add(step);
    world.physics.rebuild_due();
    remember_rezzed_masses(world);
    play_keyframes(world, dt);
    let roots = physical_roots(world);
    if roots.is_empty() && world.physics.contacts.is_empty() && !anybody_listens(world) {
        world.physics.bodies.clear();
        return;
    }
    let policy = world.physics.policy;
    let keys: BTreeMap<RegionLocalObjectId, ObjectKey> = roots
        .iter()
        .filter_map(|root| Some((*root, world.objects.get(*root)?.full_id)))
        .collect();
    // What stopped being physical took its motion with it.
    let was_bodies: BTreeSet<ObjectKey> = world.physics.bodies.keys().copied().collect();
    world
        .physics
        .bodies
        .retain(|key, _body| keys.values().any(|known| known == key));
    let mut solids = solids(world);
    for (root, key) in &keys {
        let prims: BTreeSet<RegionLocalObjectId> = std::iter::once(*root)
            .chain(crate::link_sets::children_in_order(world, *root))
            .collect();
        let moved = advance(world, (*root, *key), &prims, &solids, (dt, region, policy));
        if moved.is_some() {
            for solid in &mut solids {
                if prims.contains(&solid.local_id)
                    && let Some(object) = world.objects.get(solid.local_id)
                {
                    solid.aabb = box_of(world, &object.clone());
                }
            }
        }
    }
    let now = touching(world, &keys, &solids, region.avatars, region.ground);
    raise(world, now, &was_bodies);
}

/// Notes what each object of the region weighs the first time the physics
/// sees it ([`MassRule::AsRezzed`]), and forgets the objects gone.
fn remember_rezzed_masses(world: &mut SceneFixtures) {
    if world.physics.policy.mass != MassRule::AsRezzed {
        return;
    }
    let roots: Vec<(RegionLocalObjectId, ObjectKey)> = world
        .objects
        .iter()
        .filter(|object| object.parent_id.0 == 0 && object.pcode == sl_proto::pcode::PRIMITIVE)
        .map(|object| (object.local_id, object.full_id))
        .collect();
    let fresh: Vec<(ObjectKey, f32)> = roots
        .iter()
        .filter(|(_root, key)| !world.physics.rezzed_mass.contains_key(key))
        .map(|(root, key)| (*key, density_mass(world, *root)))
        .collect();
    world.physics.rezzed_mass.extend(fresh);
    let present: BTreeSet<ObjectKey> = roots.iter().map(|(_root, key)| *key).collect();
    world
        .physics
        .rezzed_mass
        .retain(|key, _mass| present.contains(key));
}

/// One tick of every keyframed motion: each object playing one moves and
/// turns by its share, raising `moving_end` when it ends. A motion whose
/// object turned physical is gone, as OpenSim deletes it then (it raised a
/// `moving_end`, `physics/keyframed`); Second Life refuses the physics.
fn play_keyframes(world: &mut SceneFixtures, dt: f32) {
    let keys: Vec<ObjectKey> = world.physics.players.keys().copied().collect();
    for key in keys {
        let Some(root) = world.local_id_of(key) else {
            let _gone = world.physics.players.remove(&key);
            continue;
        };
        let physical = world
            .objects
            .get(root)
            .is_some_and(|object| object.update_flags & prim_flags::USE_PHYSICS != 0);
        if physical {
            if let Some(player) = world.physics.players.remove(&key)
                && player.run == crate::keyframes::Run::Playing
            {
                raise_plain(world, root, "moving_end");
            }
            continue;
        }
        let Some(moved) = world
            .physics
            .players
            .get_mut(&key)
            .and_then(|player| player.tick(dt))
        else {
            continue;
        };
        if let Some(mut object) = world.object_mut(root) {
            object.motion.position = plus(&object.motion.position, &moved.by);
            object.motion.rotation = normalised(&crate::object_edits::multiply(
                &moved.turn,
                &object.motion.rotation,
            ));
            if moved.ended {
                object.motion.velocity = ZERO;
                object.motion.angular_velocity = ZERO;
            } else {
                object.motion.velocity = scaled(&moved.by, 1.0 / dt);
                object.motion.angular_velocity = scaled(&rotation_vector(&moved.turn), 1.0 / dt);
            }
        }
        if moved.ended {
            raise_plain(world, root, "moving_end");
        }
    }
}

/// The axis of `turn` scaled by its angle, radians.
fn rotation_vector(turn: &Rotation) -> Vector {
    let turn = if turn.s < 0.0 {
        Rotation {
            x: -turn.x,
            y: -turn.y,
            z: -turn.z,
            s: -turn.s,
        }
    } else {
        turn.clone()
    };
    let angle = 2.0 * turn.s.clamp(-1.0, 1.0).acos();
    let sine = (angle / 2.0).sin();
    if sine.abs() < 1e-6 {
        return ZERO;
    }
    scaled(
        &Vector {
            x: turn.x,
            y: turn.y,
            z: turn.z,
        },
        angle / sine,
    )
}

/// `rotation` turned by the spin `omega`, region axes, for `dt` seconds.
fn spun(rotation: &Rotation, omega: &Vector, dt: f32) -> Rotation {
    let rate = length(omega);
    let angle = rate * dt;
    if angle < 1e-7 {
        return rotation.clone();
    }
    let axis = scaled(omega, 1.0 / rate);
    let (sine, cosine) = (angle / 2.0).sin_cos();
    let turn = Rotation {
        x: axis.x * sine,
        y: axis.y * sine,
        z: axis.z * sine,
        s: cosine,
    };
    normalised(&crate::object_edits::multiply(&turn, rotation))
}

/// `r` at unit length.
fn normalised(r: &Rotation) -> Rotation {
    let size =
        r.s.mul_add(r.s, r.z.mul_add(r.z, r.x.mul_add(r.x, r.y * r.y)))
            .sqrt();
    if size == 0.0 {
        return IDENTITY;
    }
    Rotation {
        x: r.x / size,
        y: r.y / size,
        z: r.z / size,
        s: r.s / size,
    }
}

/// The conjugate of `r`.
const fn conjugate(r: &Rotation) -> Rotation {
    Rotation {
        x: -r.x,
        y: -r.y,
        z: -r.z,
        s: r.s,
    }
}

/// The spin that turns `rotation` to `target`, the angle left shrinking by
/// `e` every `tau` seconds (a look-at).
fn towards(rotation: &Rotation, target: &Rotation, tau: f32) -> Vector {
    let delta = crate::object_edits::multiply(target, &conjugate(rotation));
    scaled(&rotation_vector(&delta), 1.0 / tau.max(f32::EPSILON))
}

/// `omega` with its spin about each axis of the object's frame that its
/// `STATUS_ROTATE_*` flag locks taken out.
fn locked(omega: &Vector, rotation: &Rotation, (x, y, z): (bool, bool, bool)) -> Vector {
    if x && y && z {
        return omega.clone();
    }
    let local = crate::object_edits::rotate(&conjugate(rotation), omega);
    let kept = Vector {
        x: if x { local.x } else { 0.0 },
        y: if y { local.y } else { 0.0 },
        z: if z { local.z } else { 0.0 },
    };
    crate::object_edits::rotate(rotation, &kept)
}

/// `PU_SLOWDOWN_DISTANCE_REACHED`.
const PU_SLOWDOWN_DISTANCE_REACHED: i32 = 0;

/// `PU_GOAL_REACHED`.
const PU_GOAL_REACHED: i32 = 1;

/// Where the character whose root is `root` (keyed `key`), standing at
/// `position`, goes now, [`None`] for nowhere: a goal's point, a patrol's or
/// a wander's next one, what it pursues, or a point away from what it flees.
fn walk_target(
    world: &SceneFixtures,
    key: ObjectKey,
    position: &Vector,
    avatars: &[Avatar],
) -> Option<Vector> {
    let walker = world.physics.characters.get(&key)?;
    match walker.goal.as_ref()? {
        Goal::Point(point) => Some(point.clone()),
        Goal::Patrol(points) => points
            .get(walker.leg.checked_rem(points.len()).unwrap_or(0))
            .cloned(),
        Goal::Wander { origin, range } => {
            // A point within the range, the same for the same leg of the
            // same object in every run.
            let seed = key.uuid().as_u128() ^ u128::try_from(walker.leg).unwrap_or(0);
            let share = |shift: u32| {
                let bits = u32::try_from((seed >> shift) & 0xffff).unwrap_or(0);
                f32::from(u16::try_from(bits).unwrap_or(0)) / f32::from(u16::MAX) * 2.0 - 1.0
            };
            Some(Vector {
                x: share(0).mul_add(range.x, origin.x),
                y: share(16).mul_add(range.y, origin.y),
                z: origin.z,
            })
        }
        Goal::Pursue { target, offset } => {
            let at = uuid::Uuid::parse_str(target).ok().and_then(|id| {
                world
                    .local_id_of(ObjectKey::from(id))
                    .and_then(|local_id| world.objects.get(local_id))
                    .map(|object| crate::seating::frame_of(world, object).0)
                    .or_else(|| {
                        avatars
                            .iter()
                            .find(|avatar| avatar.agent.uuid() == id)
                            .map(|avatar| avatar.position.clone())
                    })
            })?;
            Some(plus(&at, offset))
        }
        Goal::Flee { from, distance } => {
            let away = minus(position, from);
            let flat = Vector { z: 0.0, ..away };
            let gone = length(&flat);
            if gone >= *distance {
                return Some(position.clone());
            }
            let direction = if gone > 0.0 {
                scaled(&flat, 1.0 / gone)
            } else {
                Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                }
            };
            Some(plus(from, &scaled(&direction, *distance)))
        }
    }
}

/// Steers the character whose root is `root` (keyed `key`) at `position`:
/// its level velocity towards where it goes, at its speed, and the
/// `path_update` events of the leg — `PU_SLOWDOWN_DISTANCE_REACHED` within
/// twice its radius, `PU_GOAL_REACHED` within its radius, a patrol or a
/// wander going on to its next point.
fn steer(
    world: &mut SceneFixtures,
    (root, key): (RegionLocalObjectId, ObjectKey),
    position: &Vector,
    velocity: &mut Vector,
    avatars: &[Avatar],
) {
    let Some(walker) = world.physics.characters.get(&key).cloned() else {
        return;
    };
    if let Some(jump) = walker.jump
        && let Some(walker) = world.physics.characters.get_mut(&key)
    {
        velocity.z = jump;
        walker.jump = None;
    }
    let Some(target) = walk_target(world, key, position, avatars) else {
        velocity.x = 0.0;
        velocity.y = 0.0;
        return;
    };
    let left = Vector {
        z: 0.0,
        ..minus(&target, position)
    };
    let distance = length(&left);
    let radius = walker.settings.radius.max(0.1);
    let mut events = Vec::new();
    if distance <= radius.mul_add(2.0, 0.0) && !walker.slowing {
        events.push(PU_SLOWDOWN_DISTANCE_REACHED);
    }
    let reached = distance <= radius;
    if reached {
        velocity.x = 0.0;
        velocity.y = 0.0;
        let pursuing = matches!(walker.goal, Some(Goal::Pursue { .. }));
        if !pursuing {
            events.push(PU_GOAL_REACHED);
        }
    } else {
        let speed = walker.settings.speed;
        velocity.x = left.x / distance * speed;
        velocity.y = left.y / distance * speed;
    }
    if let Some(walker) = world.physics.characters.get_mut(&key) {
        walker.slowing |= events.contains(&PU_SLOWDOWN_DISTANCE_REACHED);
        if reached {
            match walker.goal {
                Some(Goal::Patrol(_) | Goal::Wander { .. }) => {
                    walker.leg = walker.leg.saturating_add(1);
                    walker.slowing = false;
                }
                Some(Goal::Pursue { .. }) => {}
                _ => walker.goal = None,
            }
        }
    }
    for event in events {
        raise_with(
            world,
            root,
            "path_update",
            vec![Value::Integer(event), Value::List(Vec::new())],
        );
    }
}

/// Moves the physical object whose root is `root` (keyed `key`), made of
/// `prims`, one tick of `dt` against `solids` and the ground, raising its
/// `moving_start` or `moving_end`; returns how far it moved, [`None`] for
/// not at all.
///
/// Its velocity this tick is last tick's with the impulses since added, or
/// what `llSetVelocity` set, then the tick's accelerations — gravity (or a
/// vehicle's whole model), the steady force over the mass — and a hover's
/// pull, a character's steering, or `llMoveToTarget`'s draw in place of the
/// parts they hold. Its spin likewise, with a look-at's turn, the axes its
/// rotation flags lock taken out; it turns by its spin.
fn advance(
    world: &mut SceneFixtures,
    (root, key): (RegionLocalObjectId, ObjectKey),
    prims: &BTreeSet<RegionLocalObjectId>,
    solids: &[Solid],
    (dt, region, policy): (f32, &Region<'_>, PhysicsPolicy),
) -> Option<Vector> {
    let (position, rotation) = world.objects.get(root).map(|object| {
        (
            object.motion.position.clone(),
            object.motion.rotation.clone(),
        )
    })?;
    let material = world.objects.get(root).map_or(3, |object| object.material);
    let settings = world
        .physics
        .settings
        .get(&key)
        .cloned()
        .unwrap_or_default();
    if let Some(waiting) = world.physics.settings.get_mut(&key) {
        waiting.impulse = ZERO;
        waiting.angular_impulse = ZERO;
        waiting.velocity = None;
        waiting.omega = None;
    }
    let mut body = world.physics.bodies.get(&key).cloned().unwrap_or_default();
    let mass = mass_of(world, root).max(1e-6);
    let gravity_multiplier = material_of(world, root).gravity;
    let water = region.water;
    let floats = settings.float_on_water;
    let ground = |x: f32, y: f32| {
        let land = (region.ground)(x, y);
        if floats { land.max(water) } else { land }
    };
    let mut bounds = solids
        .iter()
        .filter(|solid| prims.contains(&solid.local_id))
        .map(|solid| solid.aabb.clone())
        .reduce(|a, b| a.union(&b))?;
    let obstacles: Vec<&Solid> = solids
        .iter()
        .filter(|solid| solid.kind == Kind::Solid && solid.root != root)
        .collect();

    let before = body.velocity.clone();
    // What `llSetVelocity` set, then the impulses given since on top of it.
    let base = settings
        .velocity
        .clone()
        .unwrap_or_else(|| body.velocity.clone());
    let mut velocity = plus(&base, &scaled(&settings.impulse, 1.0 / mass));
    let spin = settings.omega.clone().unwrap_or_else(|| body.omega.clone());
    let mut omega = plus(&spin, &scaled(&settings.angular_impulse, 1.0 / mass));
    let kicked = settings.impulse != ZERO
        || settings.angular_impulse != ZERO
        || settings.velocity.is_some()
        || settings.omega.is_some();

    let mut vehicle = settings.vehicle.clone().filter(Vehicle::active);
    let (mut accel, mut alpha) = if let Some(vehicle) = vehicle.as_mut() {
        let out = vehicle.step(
            &crate::vehicles::Input {
                rotation: &rotation,
                position: &position,
                velocity: &velocity,
                omega: &omega,
                ground: &ground,
                water,
                gravity_multiplier,
            },
            dt,
        );
        (out.linear, out.angular)
    } else {
        (
            Vector {
                x: 0.0,
                y: 0.0,
                z: -GRAVITY * gravity_multiplier * (1.0 - settings.buoyancy),
            },
            ZERO,
        )
    };
    if let Some(vehicle) = vehicle {
        world.physics.settings_mut(key).vehicle = Some(vehicle);
    }
    if settings.force_applies && settings.force != ZERO {
        let force = if settings.force_local {
            crate::object_edits::rotate(&rotation, &settings.force)
        } else {
            settings.force.clone()
        };
        accel = plus(&accel, &scaled(&force, 1.0 / mass));
    }
    if settings.torque != ZERO {
        let torque = if settings.torque_local {
            crate::object_edits::rotate(&rotation, &settings.torque)
        } else {
            settings.torque.clone()
        };
        alpha = plus(&alpha, &scaled(&torque, 1.0 / mass));
    }
    velocity = plus(&velocity, &scaled(&accel, dt));
    omega = plus(&omega, &scaled(&alpha, dt));
    let driven = settings.force_applies && settings.force != ZERO
        || settings.torque != ZERO
        || settings
            .grab
            .as_ref()
            .is_some_and(|(_to, until)| *until >= world.physics.clock)
        || settings
            .spin
            .as_ref()
            .is_some_and(|(_to, until)| *until >= world.physics.clock)
        || settings.vehicle.as_ref().is_some_and(Vehicle::active)
        || settings.hover.is_some()
        || settings.look_at.is_some()
        || world.physics.characters.contains_key(&key);

    if let Some(hover) = &settings.hover {
        let surface = if hover.water {
            ground(position.x, position.y).max(water)
        } else {
            ground(position.x, position.y)
        };
        let left = surface + hover.height - position.z;
        if !hover.repel || left > 0.0 {
            let factor = if hover.tau > 0.0 {
                1.0 - (-dt / hover.tau).exp()
            } else {
                1.0
            };
            velocity.z = left * factor / dt;
        }
    }
    steer(world, (root, key), &position, &mut velocity, region.avatars);
    let target = settings.target.as_ref().filter(|target| target.tau > 0.0);
    if let Some(target) = target {
        let factor = 1.0 - (-dt / target.tau).exp();
        velocity = scaled(&minus(&target.target, &position), factor / dt);
    }
    if let Some(look) = &settings.look_at {
        omega = towards(&rotation, &look.rotation, look.tau);
    }
    let now = world.physics.clock;
    let grab = settings.grab.as_ref().filter(|(_to, until)| *until >= now);
    if let Some((to, _until)) = grab {
        let factor = 1.0 - (-dt / GRAB_TAU).exp();
        velocity = scaled(&minus(to, &position), factor / dt);
    }
    let spin = settings.spin.as_ref().filter(|(_to, until)| *until >= now);
    if let Some((to, _until)) = spin {
        omega = towards(&rotation, to, GRAB_TAU);
    }
    // Resting on something, it is held back by friction: Coulomb's, the
    // material's coefficient times the gravity it feels (none for a
    // weightless one), against its sliding and its spin. A vehicle's model
    // has its own; a character, a target, a drag and a spin set their own
    // motion.
    let steered = target.is_some()
        || grab.is_some()
        || spin.is_some()
        || settings.look_at.is_some()
        || world.physics.characters.contains_key(&key)
        || settings.vehicle.as_ref().is_some_and(Vehicle::active);
    if !steered && supported(&bounds, &obstacles, &ground) {
        let weight = (GRAVITY * gravity_multiplier * (1.0 - settings.buoyancy)).max(0.0);
        let slowing = material_of(world, root).friction.min(1.0) * weight * dt;
        let level = Vector {
            z: 0.0,
            ..velocity.clone()
        };
        let speed = length(&level);
        let kept = if speed > slowing {
            (speed - slowing) / speed
        } else {
            0.0
        };
        velocity.x *= kept;
        velocity.y *= kept;
        let rate = length(&omega);
        let spun_down = if rate > slowing {
            (rate - slowing) / rate
        } else {
            0.0
        };
        omega = scaled(&omega, spun_down);
    }
    omega = locked(&omega, &rotation, rotate_flags(world, root));

    if body.asleep {
        let wakes = kicked
            || driven
            || target.is_some_and(|_target| length(&velocity) * dt > MOVING)
            || (target.is_none()
                && settings.buoyancy < 1.0
                && !supported(&bounds, &obstacles, &ground));
        if !wakes {
            return None;
        }
        body.asleep = false;
    }
    let mut moved = ZERO;
    for axis in AXES {
        let wanted = along(&velocity, axis) * dt;
        if wanted == 0.0 {
            continue;
        }
        let (allowed, hit) = reach(&bounds, axis, wanted, &obstacles, &ground);
        let by = with(&ZERO, axis, allowed);
        bounds = bounds.shifted(&by);
        moved = plus(&moved, &by);
        if let Some(other) = hit {
            let speed = along(&velocity, axis);
            let back = bounced(world, policy.bounce, material, other, speed.abs());
            velocity = with(&velocity, axis, -speed * back);
        }
    }
    // Kept in the region and out of the ground.
    let low = ground(bounds.centre().x, bounds.centre().y);
    if bounds.min.z < low {
        moved = plus(&moved, &with(&ZERO, Axis::Z, low - bounds.min.z));
        velocity = with(&velocity, Axis::Z, along(&velocity, Axis::Z).max(0.0));
    }
    let mut placed = plus(&position, &moved);
    placed.x = placed.x.clamp(0.0, REGION_EDGE - CONTACT);
    placed.y = placed.y.clamp(0.0, REGION_EDGE - CONTACT);
    let moved = minus(&placed, &position);
    let turned = spun(&rotation, &omega, dt);
    let turning = length(&omega) * dt > 1e-6;
    let moving = length(&moved) > MOVING;
    if length(&velocity) < REST_SPEED && length(&omega) < REST_SPEED && !moving {
        body.rest = body.rest.saturating_add(Duration::from_secs_f32(dt));
    } else {
        body.rest = Duration::ZERO;
    }
    if let Rest::SleepsAfter(after) = policy.rest
        && body.rest >= after
        && !driven
    {
        body.asleep = true;
        velocity = ZERO;
        omega = ZERO;
    }
    body.acceleration = scaled(&minus(&velocity, &before), 1.0 / dt);
    if let Some(mut object) = world.object_mut(root) {
        if moving {
            object.motion.position = placed;
        }
        if turning {
            object.motion.rotation = turned;
        }
        object.motion.velocity = velocity.clone();
        object.motion.angular_velocity = omega.clone();
    }
    body.velocity = velocity;
    body.omega = omega;
    let started = moving && !body.moving;
    let stopped = !moving && body.moving;
    body.moving = moving;
    let _previous = world.physics.bodies.insert(key, body);
    if policy.moving_events {
        if started {
            raise_plain(world, root, "moving_start");
        } else if stopped {
            raise_plain(world, root, "moving_end");
        }
    }
    (moving || turning).then_some(moved)
}

/// How far `bounds` may move along `axis`, of the `wanted` distance, before it
/// meets an obstacle or the ground, and the obstacle met — the ground's
/// [`Collider::Land`] — if it met one.
fn reach(
    bounds: &Aabb,
    axis: Axis,
    wanted: f32,
    obstacles: &[&Solid],
    ground: &dyn Fn(f32, f32) -> f32,
) -> (f32, Option<Collider>) {
    let mut allowed = wanted;
    let mut hit = None;
    let across = |solid: &Solid| {
        AXES.iter()
            .filter(|other| **other != axis)
            .all(|other| bounds.overlap_on(&solid.aabb, *other, CONTACT))
    };
    for solid in obstacles.iter().filter(|solid| across(solid)) {
        if wanted > 0.0 {
            let gap = along(&solid.aabb.min, axis) - along(&bounds.max, axis);
            if gap >= -CONTACT && gap < allowed {
                allowed = gap.max(0.0);
                hit = Some(Collider::Prim(solid.local_id));
            }
        } else {
            let gap = along(&solid.aabb.max, axis) - along(&bounds.min, axis);
            if gap <= CONTACT && gap > allowed {
                allowed = gap.min(0.0);
                hit = Some(Collider::Prim(solid.local_id));
            }
        }
    }
    if axis == Axis::Z && wanted < 0.0 {
        let centre = bounds.centre();
        let gap = ground(centre.x, centre.y) - bounds.min.z;
        if gap > allowed {
            allowed = gap.min(0.0);
            hit = Some(Collider::Land);
        }
    }
    (allowed, hit)
}

/// Whether something solid, or the ground, holds `bounds` up.
fn supported(bounds: &Aabb, obstacles: &[&Solid], ground: &dyn Fn(f32, f32) -> f32) -> bool {
    let (allowed, _hit) = reach(bounds, Axis::Z, -CONTACT, obstacles, ground);
    allowed > -CONTACT
}

/// The share of its speed an object of `material` keeps, back the way it
/// came, landing on `other` at `speed` under `bounce`.
fn bounced(
    world: &SceneFixtures,
    bounce: Bounce,
    material: u8,
    other: Collider,
    speed: f32,
) -> f32 {
    let Bounce::Materials { ground, above } = bounce else {
        return 0.0;
    };
    if speed <= above {
        return 0.0;
    }
    let own = restitution(material);
    match other {
        Collider::Prim(local_id) => {
            own * world
                .objects
                .get(local_id)
                .map_or(0.0, |object| restitution(object.material))
        }
        Collider::Land => own * ground,
        Collider::Agent(_agent) => 0.0,
    }
}

/// The restitution of a `PRIM_MATERIAL_*` (ubODE's `m_materialContactsData`).
const fn restitution(material: u8) -> f32 {
    match material {
        0 | 1 => 0.4,
        2 | 5 => 0.7,
        3 => 0.5,
        4 => 0.3,
        6 => 0.95,
        _ => 0.0,
    }
}

/// What every prim touches now: each physical object's prims what they rest
/// against and the ground under them, each volume-detecting prim what passes
/// through it, and each prim the avatars standing against or in it.
fn touching(
    world: &SceneFixtures,
    bodies: &BTreeMap<RegionLocalObjectId, ObjectKey>,
    solids: &[Solid],
    avatars: &[Avatar],
    ground: &dyn Fn(f32, f32) -> f32,
) -> BTreeMap<Contact, (Detected, bool)> {
    let mut now = BTreeMap::new();
    let asleep = |root: RegionLocalObjectId| {
        bodies
            .get(&root)
            .and_then(|key| world.physics.bodies.get(key))
            .is_some_and(|body| body.asleep)
    };
    for (root, _key) in bodies.iter().filter(|(root, _key)| !asleep(**root)) {
        let own: Vec<&Solid> = solids.iter().filter(|solid| solid.root == *root).collect();
        for prim in own.iter().filter(|prim| prim.kind == Kind::Solid) {
            for other in solids.iter().filter(|other| other.root != *root) {
                match other.kind {
                    Kind::Solid if prim.aabb.touches(&other.aabb) => {
                        let _was = now.insert(
                            Contact {
                                prim: prim.local_id,
                                with: Collider::Prim(other.local_id),
                            },
                            (detect_prim(world, prim.local_id, other.local_id), false),
                        );
                        let _was = now.insert(
                            Contact {
                                prim: other.local_id,
                                with: Collider::Prim(prim.local_id),
                            },
                            (detect_prim(world, other.local_id, prim.local_id), false),
                        );
                    }
                    Kind::VolumeDetect if prim.aabb.overlaps(&other.aabb) => {
                        let _was = now.insert(
                            Contact {
                                prim: other.local_id,
                                with: Collider::Prim(prim.local_id),
                            },
                            (detect_prim(world, other.local_id, prim.local_id), true),
                        );
                    }
                    Kind::Solid | Kind::VolumeDetect => {}
                }
            }
        }
        let Some(bounds) = own
            .iter()
            .map(|solid| solid.aabb.clone())
            .reduce(|a, b| a.union(&b))
        else {
            continue;
        };
        let centre = bounds.centre();
        let low = ground(centre.x, centre.y);
        if bounds.min.z <= low + CONTACT {
            let at = match world.physics.policy.land_point {
                LandPoint::Ground => Vector { z: low, ..centre },
                LandPoint::Root => world
                    .objects
                    .get(*root)
                    .map_or(centre, |object| object.motion.position.clone()),
            };
            let _was = now.insert(
                Contact {
                    prim: *root,
                    with: Collider::Land,
                },
                (
                    Detected {
                        position: at,
                        ..Detected::blank()
                    },
                    false,
                ),
            );
        }
    }
    for avatar in avatars {
        let reach = Aabb::around(&avatar.position, &AVATAR_HALF);
        for solid in solids {
            let quiet = solid.kind == Kind::VolumeDetect;
            let meets = if quiet {
                reach.overlaps(&solid.aabb)
            } else {
                reach.touches(&solid.aabb)
            };
            if meets {
                let _was = now.insert(
                    Contact {
                        prim: solid.local_id,
                        with: Collider::Agent(avatar.agent),
                    },
                    (detect_avatar(world, solid.local_id, avatar), quiet),
                );
            }
        }
    }
    now
}

/// What a script in the prim `prim` detects of the prim `other` it touches.
fn detect_prim(
    world: &SceneFixtures,
    prim: RegionLocalObjectId,
    other: RegionLocalObjectId,
) -> Detected {
    let Some(object) = world.objects.get(other) else {
        return Detected::blank();
    };
    let other_root = world.root_of(other);
    let physical = world
        .objects
        .get(other_root)
        .is_some_and(|root| root.update_flags & prim_flags::USE_PHYSICS != 0);
    let mut kind = if physical { ACTIVE } else { PASSIVE };
    if scripted(world, other_root) {
        kind |= SCRIPTED;
    }
    let properties = world.properties_of(other);
    let group = |root: RegionLocalObjectId| world.record_of(root).and_then(|record| record.group);
    let (position, rotation) = crate::seating::frame_of(world, object);
    let velocity = world
        .objects
        .get(other_root)
        .map_or(ZERO, |root| root.motion.velocity.clone());
    Detected {
        key: object.full_id.uuid().to_string(),
        name: properties
            .map(|properties| properties.name)
            .unwrap_or_default(),
        owner: object.owner_id.to_string(),
        kind,
        same_group: group(world.root_of(prim)) == group(other_root),
        link_number: crate::touches::link_number(world, prim),
        position,
        rotation,
        velocity,
        ..Detected::blank()
    }
}

/// What a script in the prim `prim` detects of `avatar` touching it.
fn detect_avatar(world: &SceneFixtures, prim: RegionLocalObjectId, avatar: &Avatar) -> Detected {
    let agent = avatar.agent.uuid().to_string();
    Detected {
        key: agent.clone(),
        name: avatar.name.clone(),
        owner: agent,
        kind: world.touches.policy.agent_type,
        same_group: false,
        link_number: crate::touches::link_number(world, prim),
        position: avatar.position.clone(),
        rotation: IDENTITY,
        ..Detected::blank()
    }
}

/// The identity rotation.
const IDENTITY: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// Whether a script has an instance in the linkset whose root is `root`.
fn scripted(world: &SceneFixtures, root: RegionLocalObjectId) -> bool {
    std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .filter_map(|local_id| world.objects.get(local_id))
        .any(|prim| !world.script_instances.scripts_in(prim.full_id).is_empty())
}

/// Raises in each prim's scripts what its contacts did since the last tick:
/// `collision_start` and `land_collision_start` for one begun,
/// `collision` and `land_collision` for one going on, `collision_end` and
/// `land_collision_end` for one over. A contact of an object that was
/// physical and no longer is (`was_bodies`) ends unheard, and an avatar's
/// that goes on is heard at most once every
/// [`agent_collision_interval`](PhysicsPolicy::agent_collision_interval).
///
/// What one prim began, went on or stopped touching in one tick is one event
/// of each kind, its detected each thing touched: both grids hand a prim
/// that two avatars step onto at once one `collision_start` with two detected
/// (`events/collision-standing`, before the probe filtered the other avatar
/// out: Second Life `other+owner`, OpenSim `owner+other` under both
/// engines), as OpenSim's `SceneObjectPart.SendCollisionEvent` gathers the
/// colliders of one physics report into one event.
fn raise(
    world: &mut SceneFixtures,
    now: BTreeMap<Contact, (Detected, bool)>,
    was_bodies: &BTreeSet<ObjectKey>,
) {
    let before = core::mem::take(&mut world.physics.contacts);
    let policy = world.physics.policy;
    let clock = world.physics.clock;
    let busy_loses = policy.busy == BusyCollisions::Lost;
    let mut heard: BTreeMap<(RegionLocalObjectId, &'static str), Vec<Touching>> = BTreeMap::new();
    let mut hear = |contact: &Contact, event: &'static str, touching: &Touching| {
        heard
            .entry((contact.prim, event))
            .or_default()
            .push(touching.clone());
    };
    for (contact, touching) in &before {
        if now.contains_key(contact) {
            continue;
        }
        let root = world.root_of(contact.prim);
        let unheard = world.objects.get(root).is_some_and(|object| {
            was_bodies.contains(&object.full_id)
                && object.update_flags & prim_flags::USE_PHYSICS == 0
        });
        if unheard {
            continue;
        }
        let event = if contact.with == Collider::Land {
            "land_collision_end"
        } else {
            "collision_end"
        };
        hear(contact, event, touching);
    }
    let mut kept = BTreeMap::new();
    for (contact, (detected, quiet)) in now {
        let land = contact.with == Collider::Land;
        let touching = match before.get(&contact) {
            Some(was) => {
                let paced = matches!(contact.with, Collider::Agent(_))
                    && clock.saturating_sub(was.told) < policy.agent_collision_interval;
                let mut touching = Touching {
                    detected,
                    quiet,
                    missed: was.missed.clone(),
                    told: was.told,
                };
                if !quiet && !paced {
                    touching.told = clock;
                    let event = if land { "land_collision" } else { "collision" };
                    hear(&contact, event, &touching);
                }
                touching
            }
            None => {
                let missed = if busy_loses {
                    busy_scripts(world, contact.prim)
                } else {
                    BTreeSet::new()
                };
                let touching = Touching {
                    detected,
                    quiet,
                    missed,
                    told: clock,
                };
                let event = if land {
                    "land_collision_start"
                } else {
                    "collision_start"
                };
                hear(&contact, event, &touching);
                touching
            }
        };
        let _was = kept.insert(contact, touching);
    }
    world.physics.contacts = kept;
    let mut heard: Vec<((RegionLocalObjectId, &'static str), Vec<Touching>)> =
        heard.into_iter().collect();
    heard.sort_by_key(|((prim, event), _touchings)| (*prim, told_order(event)));
    for ((prim, event), touchings) in heard {
        tell(world, prim, event, &touchings);
    }
}

/// Where in a tick's events for one prim `event` is told: OpenSim's
/// `SceneObjectPart.PhysicsCollision` sends `collision_start`, then
/// `collision_end`, then `collision`, then `land_collision_start`,
/// `land_collision` and `land_collision_end`.
fn told_order(event: &str) -> u8 {
    match event {
        "collision_start" => 0,
        "collision_end" => 1,
        "collision" => 2,
        "land_collision_start" => 3,
        "land_collision" => 4,
        "land_collision_end" => 5,
        _ => 6,
    }
}

/// The scripts of the prim `prim` and of its root that are busy in a handler.
fn busy_scripts(world: &SceneFixtures, prim: RegionLocalObjectId) -> BTreeSet<CallerId> {
    let root = world.root_of(prim);
    [prim, root]
        .into_iter()
        .filter_map(|local_id| world.objects.get(local_id).map(|object| object.full_id))
        .flat_map(|key| world.script_instances.scripts_in(key))
        .filter(|id| {
            world
                .script_instances
                .engine()
                .instance(*id)
                .is_some_and(sl_lsl_runtime::vm::Instance::is_busy)
        })
        .collect()
}

/// Raises `event` about what `touchings` touch in the scripts the prim
/// `prim` routes it to, as one event per script: a collision event detecting
/// each of them that its `llCollisionFilter` lets through and that the script
/// did not miss, or a land event for each (a prim touches the ground once).
fn tell(world: &mut SceneFixtures, prim: RegionLocalObjectId, event: &str, touchings: &[Touching]) {
    let Some(event) = sl_lsl_runtime::library::event(event) else {
        tracing::error!(event, "a collision event the runtime does not know");
        return;
    };
    let land = event.name.starts_with("land_");
    let filter = world
        .objects
        .get(prim)
        .and_then(|object| world.physics.filters.get(&object.full_id))
        .cloned();
    let passing: Vec<&Touching> = touchings
        .iter()
        .filter(|touching| {
            land || filter
                .as_ref()
                .is_none_or(|filter| lets_through(filter, &touching.detected))
        })
        .collect();
    for id in routed(world, prim, event.name) {
        let heard: Vec<&Touching> = passing
            .iter()
            .copied()
            .filter(|touching| !touching.missed.contains(&id))
            .collect();
        if heard.is_empty() {
            continue;
        }
        let engine = world.script_instances.engine_mut();
        let posted = if land {
            heard.iter().try_for_each(|touching| {
                engine
                    .post(
                        id,
                        event,
                        vec![Value::Vector(touching.detected.position.clone())],
                    )
                    .map(|_posted| ())
            })
        } else {
            engine
                .post_detected(
                    id,
                    event,
                    heard
                        .iter()
                        .map(|touching| touching.detected.clone())
                        .collect(),
                )
                .map(|_posted| ())
        };
        if let Err(error) = posted {
            tracing::warn!(?id, %error, "a collision could not be posted");
        }
    }
}

/// Whether `filter` (`llCollisionFilter`) lets a collision with what
/// `detected` describes through: an empty name or key matches anything, and
/// what matches both is let through when the filter accepts, kept out when
/// it does not.
fn lets_through(filter: &CollisionFilter, detected: &Detected) -> bool {
    let name = filter.name.is_empty() || filter.name == detected.name;
    let key = filter.key.is_empty() || filter.key.eq_ignore_ascii_case(&detected.key);
    filter.accept == (name && key)
}

/// Raises the argument-less `event` in every script of the linkset whose root
/// is `root`.
pub(crate) fn raise_plain(world: &mut SceneFixtures, root: RegionLocalObjectId, event: &str) {
    let Some(event) = sl_lsl_runtime::library::event(event) else {
        tracing::error!(event, "a movement event the runtime does not know");
        return;
    };
    for id in crate::scripts::linkset_scripts(world, root) {
        if let Err(error) = world
            .script_instances
            .engine_mut()
            .post(id, event, Vec::new())
        {
            tracing::warn!(?id, %error, "a movement event could not be posted");
        }
    }
}

/// Raises `event` with `args` in every script of the linkset whose root is
/// `root`.
fn raise_with(world: &mut SceneFixtures, root: RegionLocalObjectId, event: &str, args: Vec<Value>) {
    let Some(event) = sl_lsl_runtime::library::event(event) else {
        tracing::error!(event, "a physics event the runtime does not know");
        return;
    };
    for id in crate::scripts::linkset_scripts(world, root) {
        if let Err(error) = world
            .script_instances
            .engine_mut()
            .post(id, event, args.clone())
        {
            tracing::warn!(?id, %error, "a physics event could not be posted");
        }
    }
}

/// The scripts a collision `event` of the prim `local_id` is raised in: its
/// own when one of them handles it, its root's when none does or when it
/// passes them on, as the grid passes touches (`llPassCollisions`).
fn routed(world: &SceneFixtures, local_id: RegionLocalObjectId, event: &str) -> Vec<CallerId> {
    let Some(prim) = world.objects.get(local_id).map(|object| object.full_id) else {
        return Vec::new();
    };
    let handling = |key: ObjectKey| -> Vec<CallerId> {
        world
            .script_instances
            .scripts_in(key)
            .into_iter()
            .filter(|id| {
                world
                    .script_instances
                    .engine()
                    .instance(*id)
                    .is_some_and(|instance| instance.handles(event))
            })
            .collect()
    };
    let root = world.root_of(local_id);
    let own = handling(prim);
    if root == local_id {
        return own;
    }
    let Some(root_key) = world.objects.get(root).map(|object| object.full_id) else {
        return own;
    };
    let pass = world.physics.passes.get(&prim).copied().unwrap_or_default();
    let to_root = match (own.is_empty(), world.touches.policy.pass) {
        (false, crate::touches::PassRule::SecondLife) => pass == PASS_ALWAYS,
        (false, crate::touches::PassRule::AnyButZero) => pass != 0,
        (true, crate::touches::PassRule::SecondLife) => pass != PASS_NEVER,
        (true, crate::touches::PassRule::AnyButZero) => true,
    };
    let mut scripts = Vec::new();
    if to_root {
        scripts.extend(handling(root_key));
    }
    scripts.extend(own);
    scripts
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A one-metre box at `centre`.
    fn cube(centre: Vector) -> Aabb {
        Aabb::around(
            &centre,
            &Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
        )
    }

    /// A solid prim filling `aabb`.
    fn solid(local_id: u32, aabb: Aabb) -> Solid {
        Solid {
            local_id: RegionLocalObjectId(local_id),
            root: RegionLocalObjectId(local_id),
            aabb,
            kind: Kind::Solid,
        }
    }

    #[test]
    fn a_falling_box_stops_on_the_top_of_the_one_below() {
        let below = solid(
            2,
            cube(Vector {
                x: 10.0,
                y: 10.0,
                z: 20.0,
            }),
        );
        let falling = cube(Vector {
            x: 10.2,
            y: 10.0,
            z: 21.5,
        });
        let flat = |_x: f32, _y: f32| 0.0;
        let (allowed, hit) = reach(&falling, Axis::Z, -2.0, &[&below], &flat);
        assert_eq!(
            (allowed.to_bits(), hit),
            (
                (-0.5_f32).to_bits(),
                Some(Collider::Prim(RegionLocalObjectId(2)))
            )
        );
        // Beside it, the box falls to the ground.
        let beside = cube(Vector {
            x: 12.0,
            y: 10.0,
            z: 1.5,
        });
        let (allowed, hit) = reach(&beside, Axis::Z, -2.0, &[&below], &flat);
        assert_eq!(
            (allowed.to_bits(), hit),
            ((-1.0_f32).to_bits(), Some(Collider::Land))
        );
    }

    #[test]
    fn boxes_touch_within_a_centimetre_and_overlap_only_inside() {
        let a = cube(Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        let resting = cube(Vector {
            x: 0.0,
            y: 0.0,
            z: 1.005,
        });
        let apart = cube(Vector {
            x: 0.0,
            y: 0.0,
            z: 1.1,
        });
        assert!(a.touches(&resting));
        assert!(!a.overlaps(&resting));
        assert!(!a.touches(&apart));
    }

    #[test]
    fn opensim_bounces_wood_on_wood_once_from_two_metres() {
        // Wood on wood keeps a quarter of the speed above 1.5 m/s: from the
        // 5.4 m/s of a 1.5 m fall, 1.35 m/s, which lands again too slowly.
        let bounce = PhysicsPolicy::OPENSIM.bounce;
        let world = SceneFixtures::default();
        let first = bounced(&world, bounce, 3, Collider::Land, 5.4);
        assert_eq!(first.to_bits(), (0.5_f32 * 0.001).to_bits());
        assert_eq!(
            (restitution(3) * restitution(3)).to_bits(),
            0.25_f32.to_bits()
        );
        assert_eq!(
            bounced(
                &world,
                PhysicsPolicy::SECOND_LIFE.bounce,
                3,
                Collider::Land,
                5.4
            )
            .to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            bounced(&world, bounce, 3, Collider::Land, 1.35).to_bits(),
            0.0_f32.to_bits()
        );
    }
}
