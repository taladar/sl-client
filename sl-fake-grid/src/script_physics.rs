//! What a script's physics call does to the region: the host's half of
//! `sl_lsl_runtime::vm::physics`. The library has already applied the
//! imitated grid's refusals, clamps and frames; this keeps what was asked on
//! the object for the next tick of [`crate::physics`], starts and stops its
//! keyframed motion and its character, and pushes what a push reaches.

use sl_lsl_runtime::vm::physics::{
    CharacterCall, KeyframedCall, KeyframedState, Keyframes, PhysicsCall, PhysicsReading,
    VehicleCall,
};
use sl_proto::{RegionLocalObjectId, prim_flags};
use sl_types::key::ObjectKey;
use sl_types::lsl::{Rotation, Vector};

use crate::keyframes::{Player, Run};
use crate::physics::Walker;
use crate::vehicles::Vehicle;
use crate::world::SceneFixtures;

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// A push's fall-off begins this far from the pusher, metres
/// (`LSL_Api.llPushObject`'s `PUSH_ATTENUATION_DISTANCE`).
const PUSH_ATTENUATION_DISTANCE: f32 = 17.0;

/// And halves each this many metres more (`PUSH_ATTENUATION_SCALE`).
const PUSH_ATTENUATION_SCALE: f32 = 5.0;

/// The object a script's call acts on: its root and the prim the script is
/// in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Caller {
    /// The linkset's root.
    pub(crate) root: RegionLocalObjectId,
    /// The prim the script is in.
    pub(crate) prim: RegionLocalObjectId,
}

/// `v × k`.
fn scaled(v: &Vector, k: f32) -> Vector {
    Vector {
        x: v.x * k,
        y: v.y * k,
        z: v.z * k,
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

/// The key of the prim `local_id`.
fn key_of(world: &SceneFixtures, local_id: RegionLocalObjectId) -> Option<ObjectKey> {
    world.objects.get(local_id).map(|object| object.full_id)
}

/// Whether the linkset whose root is `root` is physical.
fn physical(world: &SceneFixtures, root: RegionLocalObjectId) -> bool {
    world
        .objects
        .get(root)
        .is_some_and(|object| object.update_flags & prim_flags::USE_PHYSICS != 0)
}

/// `v`, given in the frame of the object whose root is `root` when `local`,
/// on the region's axes.
fn in_region(world: &SceneFixtures, root: RegionLocalObjectId, v: &Vector, local: bool) -> Vector {
    if !local {
        return v.clone();
    }
    world.objects.get(root).map_or_else(
        || v.clone(),
        |object| crate::object_edits::rotate(&object.motion.rotation, v),
    )
}

/// The physics step, seconds.
const fn tick_seconds(world: &SceneFixtures) -> f32 {
    world.script_instances.engine().config().step.as_secs_f32()
}

/// Applies `call`, which a script in `caller` made, to the region. A push is
/// [`push_object`]'s, which reaches beyond the caller.
pub(crate) fn apply(world: &mut SceneFixtures, caller: Caller, call: PhysicsCall) {
    let Some(key) = key_of(world, caller.root) else {
        return;
    };
    let tick = tick_seconds(world);
    let is_physical = physical(world, caller.root);
    match call {
        PhysicsCall::Force { force, local } => {
            let survives = world.physics.policy.force_survives_physics;
            let settings = world.physics.settings_mut(key);
            settings.force = force;
            settings.force_local = local;
            settings.force_applies = is_physical || survives;
        }
        PhysicsCall::Torque { torque, local } => {
            let settings = world.physics.settings_mut(key);
            settings.torque = torque;
            settings.torque_local = local;
        }
        PhysicsCall::Impulse { impulse, local } => {
            let impulse = in_region(world, caller.root, &impulse, local);
            if is_physical || world.physics.policy.impulses_wait {
                let settings = world.physics.settings_mut(key);
                settings.impulse = plus(&settings.impulse, &impulse);
            }
        }
        PhysicsCall::RotationalImpulse { impulse, local } => {
            let impulse = in_region(world, caller.root, &impulse, local);
            if is_physical || world.physics.policy.impulses_wait {
                let settings = world.physics.settings_mut(key);
                settings.angular_impulse = plus(&settings.angular_impulse, &impulse);
            }
        }
        PhysicsCall::Velocity { velocity, local } => {
            let velocity = in_region(world, caller.root, &velocity, local);
            world.physics.settings_mut(key).velocity = Some(velocity);
        }
        PhysicsCall::AngularVelocity { velocity, local } => {
            let velocity = in_region(world, caller.root, &velocity, local);
            world.physics.settings_mut(key).omega = Some(velocity);
        }
        PhysicsCall::Push { .. } => {
            tracing::error!("a push reached the caller's own physics; the host pushes");
        }
        PhysicsCall::Hover(hover) => world.physics.settings_mut(key).hover = hover,
        PhysicsCall::LookAt(look_at) => {
            let settings = world.physics.settings_mut(key);
            let stopped = look_at.is_none() && settings.look_at.is_some();
            settings.look_at = look_at;
            // OpenSim's stop zeroes the spin a look-at gave.
            if stopped {
                settings.omega = Some(ZERO);
            }
        }
        PhysicsCall::Vehicle(vehicle) => apply_vehicle(world, key, &vehicle, tick),
        PhysicsCall::Keyframed(keyframed) => apply_keyframed(world, caller.root, key, keyframed),
        PhysicsCall::Character(character) => apply_character(world, caller.root, key, character),
        PhysicsCall::CollisionFilter(filter) => {
            if let Some(prim) = key_of(world, caller.prim) {
                world.physics.set_filter(prim, filter);
            }
        }
        PhysicsCall::Inertia(inertia) => world.physics.settings_mut(key).inertia = inertia,
        PhysicsCall::FloatOnWater(floats) => {
            world.physics.settings_mut(key).float_on_water = floats;
        }
    }
}

/// A vehicle setting on the object keyed `key`. Second Life's
/// `VEHICLE_TYPE_NONE` stops a moving vehicle at once
/// ([`crate::physics::PhysicsPolicy::vehicle_none_stops`]).
fn apply_vehicle(world: &mut SceneFixtures, key: ObjectKey, call: &VehicleCall, tick: f32) {
    let stops = world.physics.policy.vehicle_none_stops;
    let settings = world.physics.settings_mut(key);
    let was_active = settings.vehicle.as_ref().is_some_and(Vehicle::active);
    let vehicle = settings.vehicle.get_or_insert_with(|| Vehicle::none(tick));
    match call {
        VehicleCall::Type(kind) => {
            vehicle.set_type(*kind, tick);
            if stops && was_active && !vehicle.active() {
                settings.velocity = Some(ZERO);
                settings.omega = Some(ZERO);
            }
        }
        VehicleCall::Float(code, value) => vehicle.set_float(*code, *value, tick),
        VehicleCall::Vector(code, value) => vehicle.set_vector(*code, value, tick),
        VehicleCall::Rotation(code, value) => vehicle.set_rotation(*code, value),
        VehicleCall::SetFlags(flags) => vehicle.set_flags(*flags, false),
        VehicleCall::RemoveFlags(flags) => vehicle.set_flags(*flags, true),
    }
}

/// Starts `motion` on the object whose root is `root`, keyed `key`: what
/// played is replaced (OpenSim deletes it, with no `moving_end`), and the
/// new one raises `moving_start`. It waits as long as the imitated grid
/// makes a motion wait after a change of the physics shape.
fn start_motion(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
    key: ObjectKey,
    motion: Keyframes,
) {
    let wait = world
        .physics
        .policy
        .keyframed_wait_after_shape
        .zip(world.physics.shape_changed.get(&key).copied())
        .map_or(0.0, |(after, changed)| {
            changed
                .saturating_add(after)
                .saturating_sub(world.physics.clock)
                .as_secs_f32()
        });
    let empty = motion.frames.is_empty();
    let mut player = Player::new(motion, wait);
    if empty {
        // A motion of no frames is kept, and plays nothing
        // (`KeyframeMotion.Start` of no keyframes).
        player.run = Run::Stopped;
    }
    let _replaced = world.physics.players.insert(key, player);
    if !empty {
        world.physics.raised.push((root, "moving_start"));
    }
}

/// Stills the object whose root is `root`: a stopped or paused motion
/// leaves it where it is, at rest.
fn still(world: &mut SceneFixtures, root: RegionLocalObjectId) {
    if let Some(mut object) = world.object_mut(root) {
        object.motion.velocity = ZERO;
        object.motion.angular_velocity = ZERO;
    }
}

/// A keyframed call on the object whose root is `root`, keyed `key`.
fn apply_keyframed(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
    key: ObjectKey,
    call: KeyframedCall,
) {
    let run = world.physics.players.get(&key).map(|player| player.run);
    match call {
        KeyframedCall::Play(motion) => {
            if world.physics.policy.play_resumes_last {
                let _previous = world.physics.last_motion.insert(key, motion.clone());
            }
            start_motion(world, root, key, motion);
        }
        KeyframedCall::Keep(motion) => {
            let _previous = world.physics.last_motion.insert(key, motion);
        }
        KeyframedCall::Resume => match run {
            Some(Run::Paused) => {
                if let Some(player) = world.physics.players.get_mut(&key) {
                    player.run = Run::Playing;
                }
                world.physics.raised.push((root, "moving_start"));
            }
            Some(Run::Stopped) => {
                let restarted = world
                    .physics
                    .players
                    .get_mut(&key)
                    .filter(|player| !player.is_empty())
                    .map(Player::restart)
                    .is_some();
                if restarted {
                    world.physics.raised.push((root, "moving_start"));
                }
            }
            Some(Run::Playing) => {}
            None => {
                if let Some(motion) = world.physics.last_motion.get(&key).cloned() {
                    start_motion(world, root, key, motion);
                }
            }
        },
        KeyframedCall::Pause => {
            if run == Some(Run::Playing) {
                if let Some(player) = world.physics.players.get_mut(&key) {
                    player.run = Run::Paused;
                }
                still(world, root);
                world.physics.raised.push((root, "moving_end"));
            }
        }
        KeyframedCall::Stop => {
            if let Some(player) = world.physics.players.get_mut(&key) {
                player.run = Run::Stopped;
            }
            if run.is_some() {
                still(world, root);
            }
            if run == Some(Run::Playing) {
                world.physics.raised.push((root, "moving_end"));
            }
        }
        KeyframedCall::Blank => {
            world.physics.raised.push((root, "moving_start"));
            world.physics.raised.push((root, "moving_end"));
        }
    }
}

/// Sets or clears the physics flag of the object whose root is `root`.
fn set_physical(world: &mut SceneFixtures, root: RegionLocalObjectId, on: bool) {
    if let Some(mut object) = world.object_mut(root) {
        if on {
            object.update_flags |= prim_flags::USE_PHYSICS;
        } else {
            object.update_flags &= !prim_flags::USE_PHYSICS;
        }
    }
}

/// A pathfinding call on the object whose root is `root`, keyed `key`: a
/// character is physical while it is one (`physics/character`).
fn apply_character(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
    key: ObjectKey,
    call: CharacterCall,
) {
    match call {
        CharacterCall::Make(settings) => {
            match world.physics.characters.get_mut(&key) {
                Some(walker) => walker.settings = settings,
                None => {
                    let _previous = world.physics.characters.insert(
                        key,
                        Walker {
                            settings,
                            goal: None,
                            leg: 0,
                            slowing: false,
                            jump: None,
                        },
                    );
                }
            }
            set_physical(world, root, true);
        }
        CharacterCall::Delete => {
            if world.physics.characters.remove(&key).is_some() {
                set_physical(world, root, false);
            }
        }
        CharacterCall::Go(goal) => {
            if let Some(walker) = world.physics.characters.get_mut(&key) {
                walker.goal = Some(goal);
                walker.leg = 0;
                walker.slowing = false;
            }
        }
        CharacterCall::Stop => {
            if let Some(walker) = world.physics.characters.get_mut(&key) {
                walker.goal = None;
            }
        }
        CharacterCall::Jump(height) => {
            if let Some(walker) = world.physics.characters.get_mut(&key) {
                walker.jump = Some((2.0 * crate::physics::GRAVITY * height.max(0.0)).sqrt());
            }
        }
    }
}

/// What a push does once the host has found what it reaches.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Pushed {
    /// Nothing: the key is nothing's, the land refuses, or the grid drops a
    /// push of an object that is not physical.
    Nothing,
    /// An object's velocity and spin get the kicks (kept in its settings).
    Object,
    /// An avatar is pushed: its velocity changes by this much.
    Avatar(sl_types::key::AgentKey, Vector),
}

/// The mass of an avatar, in Linden's units, for a push: OpenSim's ubODE
/// character's density (60) times the box it collides as. Unmeasured on
/// either grid.
pub(crate) const AVATAR_MASS: f32 =
    60.0 * 0.45 * 0.6 * 1.9 / sl_lsl_runtime::vm::physics::MASS_UNIT_KG;

/// `llPushObject` from `caller` at the object or avatar keyed `target`: the
/// impulse falls off past 17 m (`LSL_Api.llPushObject`); an object is pushed
/// whatever the land says, an avatar only where the parcel allows pushing or
/// the pusher's owner owns it or is the avatar (`may_push_avatar`, worked out
/// by the host from the land under the avatar).
pub(crate) fn push_object(
    world: &mut SceneFixtures,
    caller: Caller,
    (target, avatar): (&str, Option<(sl_types::key::AgentKey, Vector, bool)>),
    (impulse, angular, local): (Vector, Vector, bool),
) -> Pushed {
    let Some(from) = world
        .objects
        .get(caller.root)
        .map(|object| crate::seating::frame_of(world, object).0)
    else {
        return Pushed::Nothing;
    };
    let attenuation = |at: &Vector| {
        let (dx, dy, dz) = (at.x - from.x, at.y - from.y, at.z - from.z);
        let distance = dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt();
        if distance > PUSH_ATTENUATION_DISTANCE {
            1.0 / (1.0 + (distance - PUSH_ATTENUATION_DISTANCE) / PUSH_ATTENUATION_SCALE)
        } else {
            1.0
        }
    };
    if let Some((agent, position, allowed)) = avatar {
        if !allowed {
            return Pushed::Nothing;
        }
        let kick = scaled(&impulse, attenuation(&position) / AVATAR_MASS);
        return Pushed::Avatar(agent, kick);
    }
    let Some(prim) = uuid::Uuid::parse_str(target)
        .ok()
        .and_then(|id| world.local_id_of(ObjectKey::from(id)))
    else {
        return Pushed::Nothing;
    };
    if world.objects.attached_to(world.root_of(prim)).is_some() {
        return Pushed::Nothing;
    }
    let root = world.root_of(prim);
    let Some(key) = key_of(world, root) else {
        return Pushed::Nothing;
    };
    if !physical(world, root) && !world.physics.policy.impulses_wait {
        return Pushed::Nothing;
    }
    let at = world
        .objects
        .get(prim)
        .map_or(ZERO, |object| crate::seating::frame_of(world, object).0);
    let share = attenuation(&at);
    let linear = scaled(&in_region(world, root, &impulse, local), share);
    let angular = in_region(world, root, &angular, local);
    let settings = world.physics.settings_mut(key);
    settings.impulse = plus(&settings.impulse, &linear);
    settings.angular_impulse = plus(&settings.angular_impulse, &angular);
    Pushed::Object
}

/// What the physics says of the object whose root is `root`, its centre of
/// mass the mass-weighted middle of its prims.
pub(crate) fn reading(world: &SceneFixtures, root: RegionLocalObjectId) -> PhysicsReading {
    let Some(object) = world.objects.get(root) else {
        return PhysicsReading::at_rest(ZERO);
    };
    let key = object.full_id;
    let position = crate::seating::frame_of(world, object).0;
    let settings = world.physics.settings_of(key);
    let body = world.physics.body_of(key);
    let is_physical = object.update_flags & prim_flags::USE_PHYSICS != 0;
    let keyframed = match world.physics.players.get(&key).map(|player| player.run) {
        Some(Run::Playing) => KeyframedState::Playing,
        Some(Run::Paused) => KeyframedState::Paused,
        Some(Run::Stopped) | None => KeyframedState::Idle,
    };
    PhysicsReading {
        force: settings.map_or(ZERO, |settings| settings.force.clone()),
        torque: settings.map_or(ZERO, |settings| settings.torque.clone()),
        omega: is_physical.then(|| body.map_or(ZERO, |body| body.omega.clone())),
        acceleration: if is_physical {
            body.map_or(ZERO, |body| body.acceleration.clone())
        } else {
            ZERO
        },
        mass: crate::physics::mass_of(world, root),
        centre_of_mass: centre_of_mass(world, root).unwrap_or(position),
        keyframed,
        character: world.physics.characters.contains_key(&key),
        inertia: settings.and_then(|settings| settings.inertia.clone()),
    }
}

/// The mass-weighted middle of the prims of the linkset whose root is
/// `root`, each prim's mass its share of the box volumes; [`None`] for a
/// linkset of no volume.
fn centre_of_mass(world: &SceneFixtures, root: RegionLocalObjectId) -> Option<Vector> {
    let mut total = 0.0;
    let mut sum = ZERO;
    for prim in std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .filter_map(|local_id| world.objects.get(local_id))
    {
        let volume = prim.scale.x * prim.scale.y * prim.scale.z;
        let at = crate::seating::frame_of(world, prim).0;
        sum = plus(&sum, &scaled(&at, volume));
        total += volume;
    }
    (total > 0.0).then(|| scaled(&sum, 1.0 / total))
}

/// `OSTPOBJ_STOPATTARGET`.
const STOP_AT_TARGET: i32 = 0x1;
/// `OSTPOBJ_SETROT`.
const SET_ROTATION: i32 = 0x4;

/// How soon one object may move another again by `osTeleportObject`.
const TELEPORT_COOLDOWN: core::time::Duration = core::time::Duration::from_secs(1);

/// `osTeleportObject` of the object `target`'s linkset by the object keyed
/// `mover` (`SceneObjectGroup.TeleportObject`): within the region — a
/// destination outside it is a crossing, which the fake grid's objects do
/// not make, and is answered −1 —, kept above the ground by half its
/// height, its velocity kept unless `OSTPOBJ_STOPATTARGET` and turned with
/// it. `allowed` is the land check the host made for a stranger's object.
pub(crate) fn teleport_object(
    world: &mut SceneFixtures,
    (target, mover): (RegionLocalObjectId, ObjectKey),
    (position, rotation, flags): (Vector, Rotation, i32),
    ground: impl Fn(f32, f32) -> f32,
) -> i32 {
    let root = world.root_of(target);
    let Some(key) = key_of(world, root) else {
        return -1;
    };
    if world.objects.attached_to(root).is_some() || world.physics.players.contains_key(&key) {
        return -1;
    }
    if !(0.0..256.0).contains(&position.x) || !(0.0..256.0).contains(&position.y) {
        return -1;
    }
    let now = world.physics.clock;
    if world
        .physics
        .teleports
        .get(&(key, mover))
        .is_some_and(|last| now.saturating_sub(*last) < TELEPORT_COOLDOWN)
    {
        return -2;
    }
    let _previous = world.physics.teleports.insert((key, mover), now);
    let stop = flags & STOP_AT_TARGET != 0;
    let set_rotation = flags & SET_ROTATION != 0;
    let Some(object) = world.objects.get(root).cloned() else {
        return -1;
    };
    let length = rotation
        .s
        .mul_add(
            rotation.s,
            rotation.z.mul_add(
                rotation.z,
                rotation.x.mul_add(rotation.x, rotation.y * rotation.y),
            ),
        )
        .sqrt();
    let rotation = if length > 0.0 {
        Rotation {
            x: rotation.x / length,
            y: rotation.y / length,
            z: rotation.z / length,
            s: rotation.s / length,
        }
    } else {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }
    };
    let current = object.motion.rotation.clone();
    // The turn to make: the new rotation's difference from the current one
    // for `OSTPOBJ_SETROT`, the rotation itself otherwise.
    let turn = if set_rotation {
        crate::object_edits::multiply(
            &rotation,
            &Rotation {
                x: -current.x,
                y: -current.y,
                z: -current.z,
                s: current.s,
            },
        )
    } else {
        rotation
    };
    let turns = set_rotation || turn.s.abs() < 0.999_99;
    let mut velocity = if stop {
        ZERO
    } else {
        object.motion.velocity.clone()
    };
    let mut omega = if stop {
        ZERO
    } else {
        object.motion.angular_velocity.clone()
    };
    let mut orientation = current;
    if turns {
        velocity = crate::object_edits::rotate(&turn, &velocity);
        omega = crate::object_edits::rotate(&turn, &omega);
        orientation = crate::object_edits::multiply(&turn, &orientation);
    }
    let reach = crate::object_edits::rotate(&orientation, &object.scale)
        .z
        .abs();
    let mut to = position;
    to.z = to.z.max(0.5f32.mul_add(reach, ground(to.x, to.y)) + 0.01);
    if let Some(mut stored) = world.object_mut(root) {
        stored.motion.position = to;
        stored.motion.rotation = orientation;
        stored.motion.velocity = velocity.clone();
        stored.motion.angular_velocity = omega.clone();
    }
    if world.physics.body_of(key).is_some() {
        let settings = world.physics.settings_mut(key);
        settings.velocity = Some(velocity);
        settings.omega = Some(omega);
    }
    1
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_lsl_runtime::vm::physics::{Keyframe, KeyframeMode};
    use sl_types::key::AgentKey;

    use super::*;
    use crate::physics::{Physics, PhysicsPolicy};

    /// The moved box's region-local id.
    const BOX: RegionLocalObjectId = RegionLocalObjectId(7);

    /// A region holding one half-metre box at `<100, 100, 30>`, its physics
    /// imitating `policy`.
    fn region(policy: PhysicsPolicy) -> SceneFixtures {
        let mut world = SceneFixtures::default();
        world.physics = Physics::new(policy);
        world.objects.push(crate::world::box_prim(
            BOX,
            ObjectKey::from(uuid::Uuid::from_u128(0x70)),
            AgentKey::from(uuid::Uuid::from_u128(0x1)),
            Vector {
                x: 100.0,
                y: 100.0,
                z: 30.0,
            },
            Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
        ));
        world
    }

    /// The box's position.
    fn at(world: &SceneFixtures) -> Option<Vector> {
        world
            .objects
            .get(BOX)
            .map(|object| object.motion.position.clone())
    }

    #[test]
    fn a_teleported_object_lands_above_the_ground_and_cools_down() {
        let mut world = region(PhysicsPolicy::OPENSIM);
        let mover = ObjectKey::from(uuid::Uuid::from_u128(0x90));
        let identity = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
        let flat = |_x: f32, _y: f32| 20.0;
        let to = Vector {
            x: 50.0,
            y: 60.0,
            z: 10.0,
        };
        assert_eq!(
            teleport_object(
                &mut world,
                (BOX, mover),
                (to.clone(), identity.clone(), 0),
                flat
            ),
            1
        );
        // Half its height and a centimetre above the ground.
        assert_eq!(
            at(&world),
            Some(Vector {
                x: 50.0,
                y: 60.0,
                z: 20.26,
            })
        );
        // The same mover again within the second.
        assert_eq!(
            teleport_object(
                &mut world,
                (BOX, mover),
                (to.clone(), identity.clone(), 0),
                flat
            ),
            -2
        );
        world.physics.clock = Duration::from_secs(2);
        // Off the region: a crossing, which the fake grid does not make.
        let off = Vector { x: 300.0, ..to };
        assert_eq!(
            teleport_object(&mut world, (BOX, mover), (off, identity, 0), flat),
            -1
        );
    }

    #[test]
    fn a_motion_holds_a_teleport_and_an_empty_one_plays_nothing() {
        let mut world = region(PhysicsPolicy::OPENSIM);
        let key = ObjectKey::from(uuid::Uuid::from_u128(0x70));
        let caller = Caller {
            root: BOX,
            prim: BOX,
        };
        apply(
            &mut world,
            caller,
            PhysicsCall::Keyframed(KeyframedCall::Play(Keyframes {
                frames: Vec::new(),
                mode: KeyframeMode::Forward,
            })),
        );
        assert!(world.physics.raised.is_empty());
        assert_eq!(reading(&world, BOX).keyframed, KeyframedState::Idle);
        // A motion, even one that never played, holds the object.
        let identity = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
        let mover = ObjectKey::from(uuid::Uuid::from_u128(0x90));
        assert_eq!(
            teleport_object(&mut world, (BOX, mover), (ZERO, identity, 0), |_x, _y| 0.0),
            -1
        );
        apply(
            &mut world,
            caller,
            PhysicsCall::Keyframed(KeyframedCall::Play(Keyframes {
                frames: vec![Keyframe {
                    translation: Some(Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    }),
                    rotation: None,
                    seconds: 1.0,
                }],
                mode: KeyframeMode::Forward,
            })),
        );
        assert_eq!(world.physics.raised, vec![(BOX, "moving_start")]);
        assert_eq!(reading(&world, BOX).keyframed, KeyframedState::Playing);
        assert!(world.physics.players.contains_key(&key));
    }

    #[test]
    fn the_mass_on_second_life_follows_a_scale_late_and_opensims_not_at_all() {
        let mut second_life = region(PhysicsPolicy::SECOND_LIFE);
        let prim = ObjectKey::from(uuid::Uuid::from_u128(0x70));
        let mass = |world: &SceneFixtures| crate::physics::mass_of(world, BOX);
        assert_eq!(mass(&second_life).to_bits(), 1.25_f32.to_bits());
        let scale = |world: &mut SceneFixtures, side: f32| {
            let to = Vector {
                x: side,
                y: side,
                z: side,
            };
            if let Some(mut object) = world.object_mut(BOX) {
                object.scale = to.clone();
            }
            world.physics.rescaled(prim, &to);
        };
        scale(&mut second_life, 1.0);
        assert_eq!(mass(&second_life).to_bits(), 10.0_f32.to_bits());
        scale(&mut second_life, 2.0);
        assert_eq!(mass(&second_life).to_bits(), 10.0_f32.to_bits());
        second_life.physics.clock = Duration::from_millis(1200);
        assert_eq!(mass(&second_life).to_bits(), 80.0_f32.to_bits());

        let mut opensim = region(PhysicsPolicy::OPENSIM);
        crate::physics::step(
            &mut opensim,
            &crate::physics::Region {
                avatars: &[],
                ground: &|_x, _y| 0.0,
                water: 20.0,
            },
            Duration::from_millis(100),
        );
        scale(&mut opensim, 2.0);
        assert_eq!(mass(&opensim).to_bits(), 1.25_f32.to_bits());
    }
}
