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
//! `llMoveToTarget` with a tau of 0.3 reached a target a metre up within a
//! tenth of a metre in 0.69 s on Second Life and 0.71 s on OpenSim: the
//! distance left shrinks by `e` every tau, against gravity.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use sl_lsl_runtime::Value;
use sl_lsl_runtime::vm::prim::MoveTarget;
use sl_lsl_runtime::vm::{CallerId, Detected};
use sl_proto::{Object, RegionLocalObjectId, prim_flags};
use sl_types::key::{AgentKey, ObjectKey};
use sl_types::lsl::{Rotation, Vector};

use crate::world::SceneFixtures;

/// The pull of gravity, in metres a second squared.
const GRAVITY: f32 = 9.806_65;

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
    };
}

impl Default for PhysicsPolicy {
    /// Second Life's.
    fn default() -> Self {
        Self::SECOND_LIFE
    }
}

/// What a script set on an object that its physics reads.
#[derive(Debug, Clone, PartialEq)]
struct Settings {
    /// `llSetBuoyancy`: the share of gravity it does not feel.
    buoyancy: f32,
    /// `llMoveToTarget`: where it is drawn while physical.
    target: Option<MoveTarget>,
}

impl Default for Settings {
    /// Gravity in full, no target.
    fn default() -> Self {
        Self {
            buoyancy: 0.0,
            target: None,
        }
    }
}

/// A physical object's motion.
#[derive(Debug, Clone, PartialEq)]
struct Body {
    /// Its velocity, in metres a second.
    velocity: Vector,
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

/// One tick of the region's physics, `step` long: every physical object
/// moves, and every prim's scripts hear what it began, went on and stopped
/// touching. `ground` is the height of the ground at a point. Runs inside the
/// tick's update burst ([`crate::updates`]), so what moved goes out with it.
pub(crate) fn step(
    world: &mut SceneFixtures,
    avatars: &[Avatar],
    step: Duration,
    ground: &dyn Fn(f32, f32) -> f32,
) {
    let roots = physical_roots(world);
    if roots.is_empty() && world.physics.contacts.is_empty() && !anybody_listens(world) {
        world.physics.bodies.clear();
        return;
    }
    let dt = step.as_secs_f32();
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
        let moved = advance(world, (*root, *key), &prims, &solids, (dt, ground, policy));
        if let Some(by) = moved {
            for solid in &mut solids {
                if prims.contains(&solid.local_id) {
                    solid.aabb = solid.aabb.shifted(&by);
                }
            }
        }
    }
    let now = touching(world, &keys, &solids, avatars, ground);
    raise(world, now, &was_bodies);
}

/// Moves the physical object whose root is `root` (keyed `key`), made of
/// `prims`, one tick of `dt` against `solids` and the ground, raising its
/// `moving_start` or `moving_end`; returns how far it moved, [`None`] for
/// not at all.
fn advance(
    world: &mut SceneFixtures,
    (root, key): (RegionLocalObjectId, ObjectKey),
    prims: &BTreeSet<RegionLocalObjectId>,
    solids: &[Solid],
    (dt, ground, policy): (f32, &dyn Fn(f32, f32) -> f32, PhysicsPolicy),
) -> Option<Vector> {
    let position = world.objects.get(root)?.motion.position.clone();
    let material = world.objects.get(root).map_or(3, |object| object.material);
    let settings = world
        .physics
        .settings
        .get(&key)
        .cloned()
        .unwrap_or_default();
    let mut body = world.physics.bodies.get(&key).cloned().unwrap_or_default();
    let mut bounds = solids
        .iter()
        .filter(|solid| prims.contains(&solid.local_id))
        .map(|solid| solid.aabb.clone())
        .reduce(|a, b| a.union(&b))?;
    let obstacles: Vec<&Solid> = solids
        .iter()
        .filter(|solid| solid.kind == Kind::Solid && solid.root != root)
        .collect();
    let target = settings.target.as_ref().filter(|target| target.tau > 0.0);
    let mut velocity = if let Some(target) = target {
        let factor = 1.0 - (-dt / target.tau).exp();
        scaled(&minus(&target.target, &position), factor / dt)
    } else {
        let fall = GRAVITY * (1.0 - settings.buoyancy) * dt;
        Vector {
            z: body.velocity.z - fall,
            ..body.velocity.clone()
        }
    };
    if body.asleep {
        let wakes = target.is_some_and(|_target| length(&velocity) * dt > MOVING)
            || (target.is_none()
                && settings.buoyancy < 1.0
                && !supported(&bounds, &obstacles, ground));
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
        let (allowed, hit) = reach(&bounds, axis, wanted, &obstacles, ground);
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
    let moving = length(&moved) > MOVING;
    if length(&velocity) < REST_SPEED && !moving {
        body.rest = body.rest.saturating_add(Duration::from_secs_f32(dt));
    } else {
        body.rest = Duration::ZERO;
    }
    if let Rest::SleepsAfter(after) = policy.rest
        && body.rest >= after
    {
        body.asleep = true;
        velocity = ZERO;
    }
    if let Some(mut object) = world.object_mut(root) {
        if moving {
            object.motion.position = placed;
        }
        object.motion.velocity = velocity.clone();
    }
    body.velocity = velocity;
    let started = moving && !body.moving;
    let stopped = !moving && body.moving;
    body.moving = moving;
    let _previous = world.physics.bodies.insert(key, body);
    if started {
        raise_plain(world, root, "moving_start");
    } else if stopped {
        raise_plain(world, root, "moving_end");
    }
    moving.then_some(moved)
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
/// physical and no longer is (`was_bodies`) ends unheard.
fn raise(
    world: &mut SceneFixtures,
    now: BTreeMap<Contact, (Detected, bool)>,
    was_bodies: &BTreeSet<ObjectKey>,
) {
    let before = core::mem::take(&mut world.physics.contacts);
    let busy_loses = world.physics.policy.busy == BusyCollisions::Lost;
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
        tell(world, contact, event, touching);
    }
    let mut kept = BTreeMap::new();
    for (contact, (detected, quiet)) in now {
        let land = contact.with == Collider::Land;
        let touching = match before.get(&contact) {
            Some(was) => {
                let touching = Touching {
                    detected,
                    quiet,
                    missed: was.missed.clone(),
                };
                if !quiet {
                    let event = if land { "land_collision" } else { "collision" };
                    tell(world, &contact, event, &touching);
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
                };
                let event = if land {
                    "land_collision_start"
                } else {
                    "collision_start"
                };
                tell(world, &contact, event, &touching);
                touching
            }
        };
        let _was = kept.insert(contact, touching);
    }
    world.physics.contacts = kept;
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

/// Raises `event` about `touching` in the scripts `contact`'s prim routes it
/// to, but those that missed the contact.
fn tell(world: &mut SceneFixtures, contact: &Contact, event: &str, touching: &Touching) {
    let Some(event) = sl_lsl_runtime::library::event(event) else {
        tracing::error!(event, "a collision event the runtime does not know");
        return;
    };
    let told: Vec<CallerId> = routed(world, contact.prim, event.name)
        .into_iter()
        .filter(|id| !touching.missed.contains(id))
        .collect();
    for id in told {
        let engine = world.script_instances.engine_mut();
        let posted = if contact.with == Collider::Land {
            engine.post(
                id,
                event,
                vec![Value::Vector(touching.detected.position.clone())],
            )
        } else {
            engine.post_detected(id, event, vec![touching.detected.clone()])
        };
        if let Err(error) = posted {
            tracing::warn!(?id, %error, "a collision could not be posted");
        }
    }
}

/// Raises the argument-less `event` in every script of the linkset whose root
/// is `root`.
fn raise_plain(world: &mut SceneFixtures, root: RegionLocalObjectId, event: &str) {
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
