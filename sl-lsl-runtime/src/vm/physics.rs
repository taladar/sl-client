//! What a script asks of its object's physics, and what it reads back: the
//! forces, impulses and velocities it sets, the hover and look-at it starts,
//! the vehicle it configures, the keyframed motion it plays, the character
//! it makes, and the material its mass follows.
//!
//! The library applies each grid's rules before a call reaches the host —
//! what it refuses, what it clamps, which frame a local vector is kept in
//! (`physics/forces`: Second Life reads a local force back as given, OpenSim
//! rotated into the region) — and the host's physics only runs what it is
//! handed ([`Host::physics`](crate::vm::Host::physics)). What the host knows
//! that the library needs comes back as a [`PhysicsReading`].

use sl_types::lsl::{Rotation, Vector};

/// A prim's physics material (`llSetPhysicsMaterial`,
/// `llGetPhysicsMaterial`): every grid's fresh prim reads `[1.0, 0.5, 0.6,
/// 1000.0]` (`physics/material`).
#[expect(
    clippy::module_name_repetitions,
    reason = "read at its use sites as `vm::physics::PhysicsMaterial`, beside `Prim` and the other host types"
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsMaterial {
    /// The share of gravity it feels, −1 to 28.
    pub gravity: f32,
    /// How much of its speed a landing gives back, 0 to 1.
    pub restitution: f32,
    /// Its friction, 0 to 255.
    pub friction: f32,
    /// Its density, in kilograms a cubic metre, 1 to 22 587.
    pub density: f32,
}

impl Default for PhysicsMaterial {
    /// A fresh prim's.
    fn default() -> Self {
        Self {
            gravity: 1.0,
            restitution: 0.5,
            friction: 0.6,
            density: 1000.0,
        }
    }
}

/// A hover (`llSetHoverHeight`, `llGroundRepel`): the object is held
/// `height` metres above the ground (or the water) while physical.
#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    /// How high above the surface, in metres.
    pub height: f32,
    /// Whether the water counts as the surface where it is higher.
    pub water: bool,
    /// The time constant, in seconds.
    pub tau: f32,
    /// `llGroundRepel`: pushes only up, never down to the height.
    pub repel: bool,
}

/// A look-at (`llLookAt`, `llRotLookAt`): a physical object is turned to
/// `rotation`, the angle left shrinking by `e` every `tau` seconds — the
/// library works the time constant out of the script's strength and damping
/// as its grid does (`physics/targets`).
#[derive(Debug, Clone, PartialEq)]
pub struct LookAt {
    /// The rotation to turn to.
    pub rotation: Rotation,
    /// The time constant, in seconds; positive.
    pub tau: f32,
}

/// One frame of a keyframed motion: a move and a turn relative to where the
/// last frame left the object, over `seconds`.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    /// The move, region axes; [`None`] for a rotation-only motion.
    pub translation: Option<Vector>,
    /// The turn; [`None`] for a translation-only motion.
    pub rotation: Option<Rotation>,
    /// How long it takes, in seconds: OpenSim plays a negative or a short
    /// time as given, which the host takes as one tick.
    pub seconds: f32,
}

/// How a keyframed motion plays (`KFM_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyframeMode {
    /// Once, forwards (`KFM_FORWARD`, and OpenSim's unknown mode).
    Forward,
    /// Over and over (`KFM_LOOP`).
    Loop,
    /// Forwards, backwards, forwards (`KFM_PING_PONG`).
    PingPong,
    /// Once, backwards (`KFM_REVERSE`).
    Reverse,
}

/// A keyframed motion as the library parsed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframes {
    /// Its frames, in order.
    pub frames: Vec<Keyframe>,
    /// How it plays.
    pub mode: KeyframeMode,
}

/// What `llSetKeyframedMotion` hands the host, the grid's refusals already
/// applied.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyframedCall {
    /// A new motion: whatever was playing stops and this starts. On a
    /// physical object Second Life keeps it without playing it
    /// ([`Self::Keep`]).
    Play(Keyframes),
    /// Second Life's motion given to a physical object: nothing moves, but
    /// it is the one `KFM_CMD_PLAY` plays later (`physics/keyframed`).
    Keep(Keyframes),
    /// `KFM_CMD_PLAY`: the motion goes on — Second Life's from the start
    /// when it was not playing, OpenSim's only one that was paused or
    /// stopped and not deleted.
    Resume,
    /// `KFM_CMD_PAUSE`: the motion holds where it is.
    Pause,
    /// `KFM_CMD_STOP`: the motion ends where it is, not back at its start.
    Stop,
    /// Second Life's empty call with nothing playing: a `moving_start` and a
    /// `moving_end`, and no motion (`physics/keyframed`).
    Blank,
}

/// Whether a keyframed motion plays on the object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyframedState {
    /// It has none, or none that is going on.
    #[default]
    Idle,
    /// One plays.
    Playing,
    /// One is paused.
    Paused,
}

/// A vehicle setting (`llSetVehicle*`, `llRemoveVehicleFlags`), the
/// parameter codes and flag bits as the script gave them: the host's
/// vehicle model clamps what each grid clamps.
#[derive(Debug, Clone, PartialEq)]
pub enum VehicleCall {
    /// `llSetVehicleType`: the type's defaults, or no vehicle for
    /// `VEHICLE_TYPE_NONE` and a type no grid has.
    Type(i32),
    /// `llSetVehicleFloatParam`: a float parameter.
    Float(i32, f32),
    /// `llSetVehicleVectorParam`: a vector parameter.
    Vector(i32, Vector),
    /// `llSetVehicleRotationParam`: a rotation parameter.
    Rotation(i32, Rotation),
    /// `llSetVehicleFlags`.
    SetFlags(i32),
    /// `llRemoveVehicleFlags`.
    RemoveFlags(i32),
}

/// A pathfinding character's settings (`llCreateCharacter`,
/// `llUpdateCharacter`).
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    /// `CHARACTER_DESIRED_SPEED`, metres a second.
    pub speed: f32,
    /// `CHARACTER_RADIUS`, metres.
    pub radius: f32,
    /// `CHARACTER_LENGTH`, metres.
    pub length: f32,
    /// `CHARACTER_TYPE`.
    pub kind: i32,
    /// `CHARACTER_ORIENTATION`.
    pub orientation: i32,
}

impl Default for Character {
    /// Second Life's defaults for a character the script named nothing of.
    fn default() -> Self {
        Self {
            speed: 3.0,
            radius: 0.5,
            length: 1.0,
            kind: 0,
            orientation: 1,
        }
    }
}

/// Where a character goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Goal {
    /// `llNavigateTo`: to a point, then stops.
    Point(Vector),
    /// `llPatrolPoints`: round the points, over and over.
    Patrol(Vec<Vector>),
    /// `llWanderWithin`: from point to point within `range` of `origin`.
    Wander {
        /// The middle of the area.
        origin: Vector,
        /// How far it may go from it on each axis.
        range: Vector,
    },
    /// `llPursue`: after the object or avatar keyed so, `offset` from it.
    Pursue {
        /// What it goes after.
        target: String,
        /// Where, from the target.
        offset: Vector,
    },
    /// `llEvade` and `llFleeFrom`: away from a point, `distance` metres.
    Flee {
        /// What it runs from.
        from: Vector,
        /// How far.
        distance: f32,
    },
}

/// What a pathfinding function hands the host.
#[derive(Debug, Clone, PartialEq)]
pub enum CharacterCall {
    /// `llCreateCharacter` and `llUpdateCharacter`: the object is (still) a
    /// character with these settings.
    Make(Character),
    /// `llDeleteCharacter`: the object is no character any more.
    Delete,
    /// A goal to go after.
    Go(Goal),
    /// `CHARACTER_CMD_STOP` and `CHARACTER_CMD_SMOOTH_STOP`: it stops.
    Stop,
    /// `CHARACTER_CMD_JUMP`: it jumps this high.
    Jump(f32),
}

/// What `llCollisionFilter` lets through: collision events only of (or,
/// with `accept` false, of anything but) the objects and avatars with that
/// name, that key, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionFilter {
    /// The name, empty for any.
    pub name: String,
    /// The key, empty (or `NULL_KEY`) for any.
    pub key: String,
    /// Whether what matches is let through (`TRUE`) or kept out.
    pub accept: bool,
}

/// OpenSim's inertia data (`osSetInertia*`, `osGetInertiaData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Inertia {
    /// The total mass, kilograms.
    pub mass: f32,
    /// The centre of mass, from the root in its frame.
    pub centre: Vector,
    /// The principal inertia, unscaled.
    pub inertia: Vector,
    /// The principal axes' rotation, as OpenSim keeps it (a four-vector).
    pub rotation: Rotation,
}

/// What a physics function hands the host, the grid's rules applied.
#[expect(
    clippy::module_name_repetitions,
    reason = "read at its use sites as `vm::physics::PhysicsCall`, beside `Prim` and the other host types"
)]
#[derive(Debug, Clone, PartialEq)]
pub enum PhysicsCall {
    /// `llSetForce`: a steady force, newtons, in the object's frame for
    /// `local` (Second Life keeps a local force so), on the region's axes
    /// otherwise.
    Force {
        /// The force.
        force: Vector,
        /// Whether it is in the object's frame.
        local: bool,
    },
    /// `llSetTorque`: a steady torque, as [`Self::Force`].
    Torque {
        /// The torque.
        torque: Vector,
        /// Whether it is in the object's frame.
        local: bool,
    },
    /// `llApplyImpulse`: a kick to the velocity, newton-seconds.
    Impulse {
        /// The impulse.
        impulse: Vector,
        /// Whether it is in the object's frame.
        local: bool,
    },
    /// `llApplyRotationalImpulse`: a kick to the spin.
    RotationalImpulse {
        /// The impulse.
        impulse: Vector,
        /// Whether it is in the object's frame.
        local: bool,
    },
    /// `llSetVelocity`: the velocity, from the next step.
    Velocity {
        /// The velocity.
        velocity: Vector,
        /// Whether it is in the object's frame.
        local: bool,
    },
    /// `llSetAngularVelocity`: the spin, from the next step.
    AngularVelocity {
        /// The spin, radians a second about each axis.
        velocity: Vector,
        /// Whether it is in the object's frame.
        local: bool,
    },
    /// `llPushObject`: a push of the object or avatar keyed `target`.
    Push {
        /// What is pushed.
        target: String,
        /// The linear impulse.
        impulse: Vector,
        /// The angular impulse.
        angular: Vector,
        /// Whether both are in the pushed thing's frame.
        local: bool,
    },
    /// `llSetHoverHeight`, `llGroundRepel`, or for [`None`] `llStopHover`.
    Hover(Option<Hover>),
    /// `llLookAt`, `llRotLookAt` on a physical object, or for [`None`]
    /// `llStopLookAt`.
    LookAt(Option<LookAt>),
    /// A vehicle setting.
    Vehicle(VehicleCall),
    /// A keyframed motion.
    Keyframed(KeyframedCall),
    /// A pathfinding character.
    Character(CharacterCall),
    /// `llCollisionFilter`, or for [`None`] no filter.
    CollisionFilter(Option<CollisionFilter>),
    /// `osSetInertia*`, or for [`None`] `osClearInertia`.
    Inertia(Option<Inertia>),
    /// `osSetPrimFloatOnWater`: a physical object floats on the water.
    FloatOnWater(bool),
}

/// What the host's physics says of the object a script is in.
#[expect(
    clippy::module_name_repetitions,
    reason = "read at its use sites as `vm::physics::PhysicsReading`, beside `Prim` and the other host types"
)]
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsReading {
    /// The steady force, as it was set.
    pub force: Vector,
    /// The steady torque, as it was set.
    pub torque: Vector,
    /// The spin of a physical object; [`None`] for one that is not physical,
    /// whose `llGetOmega` is its `llTargetOmega`.
    pub omega: Option<Vector>,
    /// The acceleration, metres a second squared.
    pub acceleration: Vector,
    /// The mass, in Linden's units (kilograms over 100), of the whole
    /// linkset.
    pub mass: f32,
    /// The centre of mass, region coordinates.
    pub centre_of_mass: Vector,
    /// Whether a keyframed motion plays.
    pub keyframed: KeyframedState,
    /// Whether the object is a pathfinding character.
    pub character: bool,
    /// The inertia data set by `osSetInertia*`, [`None`] for none.
    pub inertia: Option<Inertia>,
}

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

impl PhysicsReading {
    /// An object at rest at `centre`, with no force, no motion and no mass:
    /// what a host without physics answers.
    #[must_use]
    pub const fn at_rest(centre: Vector) -> Self {
        Self {
            force: ZERO,
            torque: ZERO,
            omega: None,
            acceleration: ZERO,
            mass: 0.0,
            centre_of_mass: centre,
            keyframed: KeyframedState::Idle,
            character: false,
            inertia: None,
        }
    }
}

/// `llGetMassMKS` over `llGetMass`: Linden's mass unit is a hundred
/// kilograms (`physics/material`: 1.25 and 125 for a fresh half-metre cube).
pub const MASS_UNIT_KG: f32 = 100.0;
