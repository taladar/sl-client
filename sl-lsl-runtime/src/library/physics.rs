//! Library tranche: forces, impulses, hovers, look-ats, physics material and
//! mass, vehicles and keyframed motion (`server-lsl-lib-physics-vehicles`).
//! The status flags, targets and `llMoveToTarget` are
//! [`super::prim_state`] and [`super::targets`]; the pathfinding characters
//! [`super::characters`].
//!
//! Each function applies its grid's rules — what it refuses and in which
//! words, what it clamps, which frame a local vector is kept in — and hands
//! the host's physics a [`PhysicsCall`]. What the grids measured is the book
//! chapter `gridspec/lsl.md`, § Status flags, forces, targets and vehicles.

use sl_types::lsl::{Rotation, Vector};

use crate::library::prim_state::own;
use crate::library::{CallError, Exception, Key};
use crate::value::{Element, NULL_KEY, ZERO_VECTOR};
use crate::vm::ScriptCtx;
use crate::vm::physics::{
    CollisionFilter, Hover, Keyframe, KeyframeMode, KeyframedCall, KeyframedState, Keyframes,
    LookAt, MASS_UNIT_KG, PhysicsCall, PhysicsMaterial, VehicleCall,
};
use crate::vm::prim::Prim;

/// `vector` turned from the prim's frame into the region's, as OpenSim keeps
/// a local force, torque or impulse (`force *= llGetRot()`).
fn into_region(prim: Option<&Prim>, vector: Vector) -> Vector {
    prim.map_or_else(
        || vector.clone(),
        |prim| crate::ops::rotate(&vector, &prim.rotation),
    )
}

/// A steady force or torque as the grid keeps it: Second Life in the frame
/// the script named, OpenSim turned into the region's at once
/// (`physics/forces`: a local `<1,0,0>` on a prim a quarter round Z reads
/// `<1,0,0>` and `<0,1,0>`).
fn kept(ctx: &mut ScriptCtx<'_>, vector: Vector, local: i32) -> (Vector, bool) {
    if local == 0 {
        return (vector, false);
    }
    if ctx.flavour().is_opensim() {
        let prim = own(ctx);
        (into_region(prim.as_ref(), vector), false)
    } else {
        (vector, true)
    }
}

/// `llSetForce`: a steady force on the object, read back by `llGetForce` as
/// it was set, however large and whether or not the object is physical
/// (`physics/forces`); it moves the object only while physical.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_force(ctx: &mut ScriptCtx<'_>, force: Vector, local: i32) -> Result<(), CallError> {
    let (force, local) = kept(ctx, force, local);
    let caller = ctx.caller();
    ctx.host()
        .physics(caller, PhysicsCall::Force { force, local });
    Ok(())
}

/// `llGetForce`: the steady force, as it was set.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_force(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().physics_reading(caller).force)
}

/// `llSetTorque`: a steady torque, kept as `llSetForce` keeps a force.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_torque(ctx: &mut ScriptCtx<'_>, torque: Vector, local: i32) -> Result<(), CallError> {
    let (torque, local) = kept(ctx, torque, local);
    let caller = ctx.caller();
    ctx.host()
        .physics(caller, PhysicsCall::Torque { torque, local });
    Ok(())
}

/// `llGetTorque`: the steady torque, as it was set.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_torque(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().physics_reading(caller).torque)
}

/// `llSetForceAndTorque`: `llSetForce` and `llSetTorque` in one call.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_force_and_torque(
    ctx: &mut ScriptCtx<'_>,
    force: Vector,
    torque: Vector,
    local: i32,
) -> Result<(), CallError> {
    ll_set_force(ctx, force, local)?;
    ll_set_torque(ctx, torque, local)
}

/// OpenSim's cap on an impulse's length (`llApplyImpulse`).
const OPENSIM_IMPULSE_CAP: f32 = 20_000.0;

/// Whether the caller's object is physical.
fn physical(ctx: &mut ScriptCtx<'_>) -> bool {
    own(ctx).is_some_and(|prim| prim.physics)
}

/// `llApplyImpulse`: a kick to the object's velocity, the impulse over its
/// mass. On a prim that is not physical Second Life does nothing; OpenSim
/// keeps it and applies it when the prim turns physical, with every other
/// impulse and push it was given meanwhile (`physics/forces`: a prim turned
/// physical and weightless after an impulse of 1 and a push of 5 shot up at
/// about (1 + 5) / 1.25 m/s). OpenSim caps the impulse at 20 000.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_apply_impulse(
    ctx: &mut ScriptCtx<'_>,
    impulse: Vector,
    local: i32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    if !opensim_flavour && !physical(ctx) {
        return Ok(());
    }
    let impulse = if opensim_flavour {
        capped(impulse, OPENSIM_IMPULSE_CAP)
    } else {
        impulse
    };
    let caller = ctx.caller();
    ctx.host().physics(
        caller,
        PhysicsCall::Impulse {
            impulse,
            local: local != 0,
        },
    );
    Ok(())
}

/// `vector`, shortened to `cap` where it is longer.
fn capped(vector: Vector, cap: f32) -> Vector {
    let length = magnitude(&vector);
    if length <= cap {
        return vector;
    }
    let share = cap / length;
    Vector {
        x: vector.x * share,
        y: vector.y * share,
        z: vector.z * share,
    }
}

/// The length of `vector`.
fn magnitude(vector: &Vector) -> f32 {
    vector
        .z
        .mul_add(vector.z, vector.x.mul_add(vector.x, vector.y * vector.y))
        .sqrt()
}

/// `llApplyRotationalImpulse`: a kick to the object's spin, kept as
/// `llApplyImpulse` keeps an impulse.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_apply_rotational_impulse(
    ctx: &mut ScriptCtx<'_>,
    impulse: Vector,
    local: i32,
) -> Result<(), CallError> {
    if !ctx.flavour().is_opensim() && !physical(ctx) {
        return Ok(());
    }
    let caller = ctx.caller();
    ctx.host().physics(
        caller,
        PhysicsCall::RotationalImpulse {
            impulse,
            local: local != 0,
        },
    );
    Ok(())
}

/// `llSetVelocity`: the physical object's velocity, from the physics' next
/// step — `llGetVel` read in the same event still has the old one, and a
/// second call in the same event replaces the first unseen
/// (`physics/forces`, on every grid). Nothing on a prim that is not
/// physical.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_velocity(
    ctx: &mut ScriptCtx<'_>,
    velocity: Vector,
    local: i32,
) -> Result<(), CallError> {
    if !physical(ctx) {
        return Ok(());
    }
    let caller = ctx.caller();
    ctx.host().physics(
        caller,
        PhysicsCall::Velocity {
            velocity,
            local: local != 0,
        },
    );
    Ok(())
}

/// `llSetAngularVelocity`: the physical object's spin, as `llSetVelocity`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_angular_velocity(
    ctx: &mut ScriptCtx<'_>,
    velocity: Vector,
    local: i32,
) -> Result<(), CallError> {
    if !physical(ctx) {
        return Ok(());
    }
    let caller = ctx.caller();
    ctx.host().physics(
        caller,
        PhysicsCall::AngularVelocity {
            velocity,
            local: local != 0,
        },
    );
    Ok(())
}

/// `llGetAccel`: the object's acceleration, zero for one that is not
/// physical (`physics/forces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_accel(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().physics_reading(caller).acceleration)
}

/// `llGetEnergy`: the object's energy, 1.0. OpenSim's is always 1.0
/// ("TODO: figure out real energy value"); Second Life spends an object's
/// energy on its pushes and impulses and refills it over time, which the
/// fake grid does not model, so a full charge is what it reads.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_get_energy(_ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(1.0)
}

/// `llPushObject`: pushes the object or avatar keyed `target` with a linear
/// and an angular impulse. The host applies the land's push rule and the
/// fall-off with distance; a key that is none pushes nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_push_object(
    ctx: &mut ScriptCtx<'_>,
    target: Key,
    impulse: Vector,
    angular: Vector,
    local: i32,
) -> Result<(), CallError> {
    if !crate::library::dotnet::is_uuid(&target.0) {
        return Ok(());
    }
    let caller = ctx.caller();
    ctx.host().physics(
        caller,
        PhysicsCall::Push {
            target: target.0,
            impulse,
            angular,
            local: local != 0,
        },
    );
    Ok(())
}

/// `llSetHoverHeight`: a physical object is held `height` metres above the
/// ground, or above the water where it is higher and `water` is set, with
/// the time constant `tau`. A prim that is not physical does not move
/// (`physics/forces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_hover_height(
    ctx: &mut ScriptCtx<'_>,
    height: f32,
    water: i32,
    tau: f32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().physics(
        caller,
        PhysicsCall::Hover(Some(Hover {
            height,
            water: water != 0,
            tau,
            repel: false,
        })),
    );
    Ok(())
}

/// `llStopHover`: the object is no longer held at a height.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_stop_hover(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().physics(caller, PhysicsCall::Hover(None));
    Ok(())
}

/// `llGroundRepel`: a physical object is kept at least `height` metres above
/// the ground (or the water), pushed up and never down. OpenSim's is a
/// hover whose height it adds the ground under the prim to before its hover
/// adds the ground again (`LSL_Api.llGroundRepel` over `PIDHoverType`): it
/// holds the object at the height plus twice the ground, and pulls it down
/// to it as well.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ground_repel(
    ctx: &mut ScriptCtx<'_>,
    height: f32,
    water: i32,
    tau: f32,
) -> Result<(), CallError> {
    let water = water != 0;
    let hover = if ctx.flavour().is_opensim() {
        let caller = ctx.caller();
        let here = ctx.host().whereabouts(caller).position;
        let ground = ctx.host().ground(caller, here.x, here.y);
        let surface = if water {
            ground.max(ctx.host().region(caller).water)
        } else {
            ground
        };
        Hover {
            height: height + surface,
            water,
            tau,
            repel: false,
        }
    } else {
        Hover {
            height,
            water,
            tau,
            repel: true,
        }
    };
    let caller = ctx.caller();
    ctx.host().physics(caller, PhysicsCall::Hover(Some(hover)));
    Ok(())
}

/// The rotation that points a prim's up axis (+Z) at `target` from `from`,
/// with its forward axis (+X) below the horizon (`LSL_Api.llLookAt`).
fn looking_at(from: &Vector, target: &Vector) -> Rotation {
    let towards = Vector {
        x: target.x - from.x,
        y: target.y - from.y,
        z: target.z - from.z,
    };
    let dir = normalised(&towards);
    let left = normalised(&Vector {
        x: -dir.y,
        y: dir.x,
        z: 0.0,
    });
    let up = Vector {
        x: dir.y.mul_add(left.z, -(dir.z * left.y)),
        y: dir.z.mul_add(left.x, -(dir.x * left.z)),
        z: dir.x.mul_add(left.y, -(dir.y * left.x)),
    };
    let axes = axes_to_rotation(&dir, &left, &up);
    let tilt = Rotation {
        x: 0.0,
        y: core::f32::consts::FRAC_1_SQRT_2,
        z: 0.0,
        s: core::f32::consts::FRAC_1_SQRT_2,
    };
    crate::ops::compose(&tilt, &axes)
}

/// `vector` at unit length, or zero for the zero vector.
fn normalised(vector: &Vector) -> Vector {
    let length = magnitude(vector);
    if length == 0.0 {
        return ZERO_VECTOR;
    }
    Vector {
        x: vector.x / length,
        y: vector.y / length,
        z: vector.z / length,
    }
}

/// `llAxes2Rot(fwd, left, up)`.
fn axes_to_rotation(fwd: &Vector, left: &Vector, up: &Vector) -> Rotation {
    let trace = fwd.x + left.y + up.z;
    let rotation = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        Rotation {
            x: (left.z - up.y) / s,
            y: (up.x - fwd.z) / s,
            z: (fwd.y - left.x) / s,
            s: s / 4.0,
        }
    } else if fwd.x > left.y && fwd.x > up.z {
        let s = (1.0 + fwd.x - left.y - up.z).sqrt() * 2.0;
        Rotation {
            x: s / 4.0,
            y: (fwd.y + left.x) / s,
            z: (up.x + fwd.z) / s,
            s: (left.z - up.y) / s,
        }
    } else if left.y > up.z {
        let s = (1.0 + left.y - fwd.x - up.z).sqrt() * 2.0;
        Rotation {
            x: (fwd.y + left.x) / s,
            y: s / 4.0,
            z: (left.z + up.y) / s,
            s: (up.x - fwd.z) / s,
        }
    } else {
        let s = (1.0 + up.z - fwd.x - left.y).sqrt() * 2.0;
        Rotation {
            x: (up.x + fwd.z) / s,
            y: (left.z + up.y) / s,
            z: s / 4.0,
            s: (fwd.y - left.x) / s,
        }
    };
    // Linden's quaternions are conjugated against the matrix-to-quaternion
    // textbook form: a row-major matrix of the axes.
    Rotation {
        x: -rotation.x,
        y: -rotation.y,
        z: -rotation.z,
        s: rotation.s,
    }
}

/// OpenSim's look-at below this strength is stopped at its first update
/// (`SceneObjectPart.UpdateLookAt`).
const OPENSIM_LOOK_AT_FLOOR: f32 = 0.04;

/// The time constant a physical object turns to a look-at with, from the
/// script's `strength` and `damping`, or [`None`] for a look-at that turns
/// nothing. Second Life's angle shrinks by `e` every `damping` seconds;
/// OpenSim spins the object at the angle left times `π × min(1, 1 /
/// strength)` (`SceneObjectPart.UpdateLookAt`). `physics/targets`: a quarter
/// turn with a strength of 1 and a damping of 0.5 came within 0.1 rad in
/// 1.36 s on Second Life (`0.5 × ln 15.7` = 1.38) and 0.87 s on OpenSim
/// (`ln 15.7 / π` = 0.88).
fn look_tau(opensim_flavour: bool, strength: f32, damping: f32) -> Option<f32> {
    if opensim_flavour {
        if strength < OPENSIM_LOOK_AT_FLOOR {
            return None;
        }
        Some(1.0 / (core::f32::consts::PI * (1.0 / strength).min(1.0)))
    } else {
        (damping > 0.0 && strength > 0.0).then_some(damping)
    }
}

/// Turns the caller's object to `rotation` with a look-at's `strength` and
/// `damping`: a physical one over time ([`look_tau`]), one that is not (or
/// is worn) at once. `snap_unless_physical` says whether the rotation is set
/// at once on a prim that is not physical whatever the strength and damping
/// (`llRotLookAt`) or only when both are positive (`llLookAt`); a physical
/// object with a strength of 0 is turned at once on OpenSim.
fn look(
    ctx: &mut ScriptCtx<'_>,
    rotation: Rotation,
    strength: f32,
    damping: f32,
    snap_unless_physical: bool,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let caller = ctx.caller();
    let worn = ctx.host().attached(caller) != 0;
    if !physical(ctx) || worn {
        if snap_unless_physical || (strength > 0.0 && damping > 0.0) {
            crate::library::prim_state::ll_set_local_rot(ctx, rotation)?;
        }
        return Ok(());
    }
    if opensim_flavour && strength == 0.0 {
        return crate::library::prim_state::ll_set_local_rot(ctx, rotation);
    }
    if let Some(tau) = look_tau(opensim_flavour, strength, damping) {
        ctx.host()
            .physics(caller, PhysicsCall::LookAt(Some(LookAt { rotation, tau })));
    }
    Ok(())
}

/// `llLookAt`: turns the object's up axis towards `target`, its forward axis
/// below the horizon — a prim that is not physical at once, on both grids
/// (`physics/targets`), when strength and damping are positive; a physical
/// one over time.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_look_at(
    ctx: &mut ScriptCtx<'_>,
    target: Vector,
    strength: f32,
    damping: f32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let from = ctx.host().whereabouts(caller).position;
    let rotation = looking_at(&from, &target);
    look(ctx, rotation, strength, damping, false)
}

/// `llRotLookAt`: turns the object to `rotation` — a prim that is not
/// physical at once (`physics/targets`), a physical one over time.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot_look_at(
    ctx: &mut ScriptCtx<'_>,
    rotation: Rotation,
    strength: f32,
    damping: f32,
) -> Result<(), CallError> {
    look(ctx, rotation, strength, damping, true)
}

/// `llStopLookAt`: the object is no longer turned towards anything.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_stop_look_at(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().physics(caller, PhysicsCall::LookAt(None));
    Ok(())
}

/// `llSetPhysicsMaterial`'s `DENSITY` bit.
const DENSITY: i32 = 1;
/// `llSetPhysicsMaterial`'s `FRICTION` bit.
const FRICTION: i32 = 2;
/// `llSetPhysicsMaterial`'s `RESTITUTION` bit.
const RESTITUTION: i32 = 4;
/// `llSetPhysicsMaterial`'s `GRAVITY_MULTIPLIER` bit.
const GRAVITY_MULTIPLIER: i32 = 8;

/// Second Life's complaint at a term out of range (`physics/material`).
const MATERIAL_REFUSED: &str = "ERROR: Failed to set material properties.";

/// `llSetPhysicsMaterial`: the terms whose bits are in `bits`. A term
/// outside its range — gravity −1 to 28, restitution 0 to 1, friction 0 to
/// 255, density 1 to 22 587 — refuses the whole call, nothing changing: on
/// Second Life with `ERROR: Failed to set material properties.` on the debug
/// channel, on OpenSim silently (`physics/material`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_physics_material(
    ctx: &mut ScriptCtx<'_>,
    bits: i32,
    gravity: f32,
    restitution: f32,
    friction: f32,
    density: f32,
) -> Result<(), CallError> {
    let terms = [
        (GRAVITY_MULTIPLIER, gravity, -1.0, 28.0),
        (RESTITUTION, restitution, 0.0, 1.0),
        (FRICTION, friction, 0.0, 255.0),
        (DENSITY, density, 1.0, 22_587.0),
    ];
    let refused = terms
        .iter()
        .any(|(bit, value, low, high)| bits & bit != 0 && !(*low..=*high).contains(value));
    if refused {
        if !ctx.flavour().is_opensim() {
            ctx.refuse(MATERIAL_REFUSED.to_owned());
        }
        return Ok(());
    }
    crate::library::prim_state::change_own(ctx, |_ctx, prim, _changes| {
        let material = &mut prim.physics_material;
        if bits & GRAVITY_MULTIPLIER != 0 {
            material.gravity = gravity;
        }
        if bits & RESTITUTION != 0 {
            material.restitution = restitution;
        }
        if bits & FRICTION != 0 {
            material.friction = friction;
        }
        if bits & DENSITY != 0 {
            material.density = density;
        }
    });
    Ok(())
}

/// `llGetPhysicsMaterial`: `[gravity multiplier, restitution, friction,
/// density]`, a fresh prim's `[1.0, 0.5, 0.6, 1000.0]` (`physics/material`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_physics_material(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    let material = own(ctx).map_or_else(PhysicsMaterial::default, |prim| prim.physics_material);
    Ok(vec![
        Element::Float(material.gravity),
        Element::Float(material.restitution),
        Element::Float(material.friction),
        Element::Float(material.density),
    ])
}

/// `llGetMass`: the mass of the whole object, in Linden's units (kilograms
/// over 100). A fresh half-metre cube's is 1.25 on every grid; Second Life's
/// follows the density and (late) the scale, OpenSim's stays at the rezzed
/// prim's — the host's physics keeps each (`physics/material`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_mass(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().physics_reading(caller).mass)
}

/// `llGetMassMKS`: `llGetMass` in kilograms.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_mass_mks(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(ll_get_mass(ctx)? * MASS_UNIT_KG)
}

/// `llGetObjectMass`: the mass of the object (its whole linkset) or avatar
/// keyed `id`, in Linden's units; 0 for a key that is none, or of nothing in
/// the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_object_mass(ctx: &mut ScriptCtx<'_>, id: Key) -> Result<f32, CallError> {
    if id.0 == NULL_KEY || !crate::library::dotnet::is_uuid(&id.0) {
        return Ok(0.0);
    }
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .object_details(caller, &id.0)
        .map_or(0.0, |details| details.mass))
}

/// `llGetCenterOfMass`: the object's centre of mass, region coordinates — a
/// single prim's own position (`physics/material`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_center_of_mass(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().physics_reading(caller).centre_of_mass)
}

/// `llCollisionFilter`: from now on the prim's collision events are only of
/// what has the name `name` and the key `id` (an empty name or `NULL_KEY`
/// matching anything), or with `accept` `FALSE` only of what does not. An
/// empty name and `NULL_KEY` accepted is no filter at all.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_collision_filter(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    id: Key,
    accept: i32,
) -> Result<(), CallError> {
    let key = if crate::library::dotnet::is_uuid(&id.0) && id.0 != NULL_KEY {
        id.0
    } else {
        String::new()
    };
    let accept = accept != 0;
    let filter = (!(name.is_empty() && key.is_empty() && accept)).then_some(CollisionFilter {
        name,
        key,
        accept,
    });
    let caller = ctx.caller();
    ctx.host()
        .physics(caller, PhysicsCall::CollisionFilter(filter));
    Ok(())
}

/// `llCollisionSprite`: nothing, on every grid — Linden's viewers stopped
/// drawing collision sprites, and OpenSim's is an empty body for that reason.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_collision_sprite(
    _ctx: &mut ScriptCtx<'_>,
    _impact_sprite: String,
) -> Result<(), CallError> {
    Ok(())
}

/// The vehicle parameters `llSetVehicleFloatParam` takes: the vector ones
/// (set on all three axes) and the float ones.
fn float_param(param: i32) -> bool {
    (16..=20).contains(&param) || (24..=40).contains(&param)
}

/// The vehicle parameters `llSetVehicleVectorParam` takes.
fn vector_param(param: i32) -> bool {
    (16..=20).contains(&param)
}

/// `VEHICLE_REFERENCE_FRAME`, the one rotation parameter.
const VEHICLE_REFERENCE_FRAME: i32 = 44;

/// `llSetVehicleType`: the vehicle type's defaults, or no vehicle for
/// `VEHICLE_TYPE_NONE` and a number no type has. No status flag moves
/// (`physics/vehicles`, on every grid).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_vehicle_type(ctx: &mut ScriptCtx<'_>, kind: i32) -> Result<(), CallError> {
    vehicle(ctx, VehicleCall::Type(kind));
    Ok(())
}

/// Hands the host's vehicle model `call`.
fn vehicle(ctx: &mut ScriptCtx<'_>, call: VehicleCall) {
    let caller = ctx.caller();
    ctx.host().physics(caller, PhysicsCall::Vehicle(call));
}

/// `llSetVehicleFloatParam`: a float parameter (or a vector one, on all
/// three axes). Second Life refuses a code no float parameter has with
/// `Unknown vehicle float parameter <code>`; OpenSim is silent. Neither
/// complains of a value out of range (`physics/vehicles`), which the
/// vehicle model clamps.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_vehicle_float_param(
    ctx: &mut ScriptCtx<'_>,
    param: i32,
    value: f32,
) -> Result<(), CallError> {
    if !float_param(param) {
        refuse_param(ctx, "float", param);
        return Ok(());
    }
    vehicle(ctx, VehicleCall::Float(param, value));
    Ok(())
}

/// Second Life's refusal of a vehicle parameter code the setter of `kind`
/// does not take; OpenSim's silence.
fn refuse_param(ctx: &mut ScriptCtx<'_>, kind: &str, param: i32) {
    if !ctx.flavour().is_opensim() {
        ctx.refuse(format!("Unknown vehicle {kind} parameter {param}"));
    }
}

/// `llSetVehicleVectorParam`: a vector parameter. Second Life refuses any
/// other code, a float parameter's among them, with `Unknown vehicle vector
/// parameter <code>` (`physics/vehicles`: 24, `VEHICLE_HOVER_HEIGHT`);
/// OpenSim is silent.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_vehicle_vector_param(
    ctx: &mut ScriptCtx<'_>,
    param: i32,
    value: Vector,
) -> Result<(), CallError> {
    if !vector_param(param) {
        refuse_param(ctx, "vector", param);
        return Ok(());
    }
    vehicle(ctx, VehicleCall::Vector(param, value));
    Ok(())
}

/// `llSetVehicleRotationParam`: `VEHICLE_REFERENCE_FRAME`. Second Life
/// refuses any other code with `Unknown vehicle rotation parameter <code>`;
/// OpenSim is silent.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_vehicle_rotation_param(
    ctx: &mut ScriptCtx<'_>,
    param: i32,
    value: Rotation,
) -> Result<(), CallError> {
    if param != VEHICLE_REFERENCE_FRAME {
        refuse_param(ctx, "rotation", param);
        return Ok(());
    }
    vehicle(ctx, VehicleCall::Rotation(param, value));
    Ok(())
}

/// `llSetVehicleFlags`: sets the flag bits, unknown ones silently
/// (`physics/vehicles`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_vehicle_flags(ctx: &mut ScriptCtx<'_>, flags: i32) -> Result<(), CallError> {
    vehicle(ctx, VehicleCall::SetFlags(flags));
    Ok(())
}

/// `llRemoveVehicleFlags`: clears the flag bits.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_remove_vehicle_flags(ctx: &mut ScriptCtx<'_>, flags: i32) -> Result<(), CallError> {
    vehicle(ctx, VehicleCall::RemoveFlags(flags));
    Ok(())
}

/// `KFM_COMMAND`.
const KFM_COMMAND: i32 = 0;
/// `KFM_MODE`.
const KFM_MODE: i32 = 1;
/// `KFM_DATA`.
const KFM_DATA: i32 = 2;
/// `KFM_CMD_PLAY`.
const KFM_CMD_PLAY: i32 = 0;
/// `KFM_CMD_STOP`.
const KFM_CMD_STOP: i32 = 1;
/// `KFM_CMD_PAUSE`.
const KFM_CMD_PAUSE: i32 = 2;
/// `KFM_ROTATION`.
const KFM_ROTATION: i32 = 1;
/// `KFM_TRANSLATION`.
const KFM_TRANSLATION: i32 = 2;
/// `PRIM_PHYSICS_SHAPE_PRIM`, a fresh prim's physics shape.
const PHYSICS_SHAPE_PRIM: i32 = 0;

/// The shortest frame Second Life plays, in seconds (`physics/keyframed`:
/// 0.05 s and −1 s refused, `Delta times must be >0.1s`).
const SECOND_LIFE_SHORTEST_FRAME: f32 = 0.1;

/// The keyframed mode `KFM_MODE`'s value names, [`None`] for a number no
/// mode has.
const fn mode_named(value: i32) -> Option<KeyframeMode> {
    match value {
        0 => Some(KeyframeMode::Forward),
        1 => Some(KeyframeMode::Loop),
        2 => Some(KeyframeMode::PingPong),
        3 => Some(KeyframeMode::Reverse),
        _ => None,
    }
}

/// `llSetKeyframedMotion`: plays `frames` — each a move, a turn and a time,
/// relative to the last — on the object, as `options` say; with no frames,
/// the `KFM_COMMAND` in `options` plays, pauses or stops what was given.
/// A worn object plays nothing, and on OpenSim neither does a physical one.
///
/// Second Life refuses, each with its line on the debug channel
/// (`physics/keyframed`): a prim whose physics shape is the default (prim),
/// a frame under 0.1 s, a list whose frames do not read — a translation
/// that is no vector, a time missing or no number —, an unknown mode or
/// option; it moves a physical prim not at all and keeps the motion for a
/// `KFM_CMD_PLAY`. OpenSim plays all of those but the malformed lists — an
/// incomplete last frame is dropped, an element of the wrong type throws —
/// as given: a short or negative time at once, an unknown mode forward, an
/// unknown option ignored.
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim's list reader throws: an
/// `InvalidCastException` for a translation or rotation of another type, or
/// a time that is a vector or a rotation, or an option that is no integer.
pub fn ll_set_keyframed_motion(
    ctx: &mut ScriptCtx<'_>,
    frames: Vec<Element>,
    options: Vec<Element>,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) != 0 {
        return Ok(());
    }
    let call = if ctx.flavour().is_opensim() {
        opensim_keyframed(ctx, &frames, &options)?
    } else {
        second_life_keyframed(ctx, &frames, &options)
    };
    if let Some(call) = call {
        ctx.host().physics(caller, PhysicsCall::Keyframed(call));
    }
    Ok(())
}

/// What Second Life makes of a keyframed call, [`None`] for one it refused
/// (having said why).
fn second_life_keyframed(
    ctx: &mut ScriptCtx<'_>,
    frames: &[Element],
    options: &[Element],
) -> Option<KeyframedCall> {
    let caller = ctx.caller();
    let state = ctx.host().physics_reading(caller).keyframed;
    let mut mode = KeyframeMode::Forward;
    let mut data = KFM_TRANSLATION | KFM_ROTATION;
    let mut command = None;
    let mut pairs = options.chunks(2);
    for pair in pairs.by_ref() {
        let (Some(Element::Integer(option)), value) = (pair.first(), pair.get(1)) else {
            ctx.refuse("Unknown parameter type.".to_owned());
            return None;
        };
        let value = match value {
            Some(Element::Integer(value)) => *value,
            _ => {
                ctx.refuse("Unknown parameter type.".to_owned());
                return None;
            }
        };
        match *option {
            KFM_MODE => {
                let Some(named) = mode_named(value) else {
                    ctx.refuse("Unknown playback mode.".to_owned());
                    return None;
                };
                mode = named;
            }
            KFM_DATA if (1..=3).contains(&value) => data = value,
            KFM_COMMAND if (0..=2).contains(&value) => command = Some(value),
            _ => {
                ctx.refuse("Unknown parameter type.".to_owned());
                return None;
            }
        }
    }
    if frames.is_empty() {
        return Some(match command {
            Some(KFM_CMD_PLAY) => KeyframedCall::Resume,
            Some(KFM_CMD_PAUSE) => KeyframedCall::Pause,
            Some(KFM_CMD_STOP) => KeyframedCall::Stop,
            _ if state == KeyframedState::Idle => KeyframedCall::Blank,
            _ => KeyframedCall::Stop,
        });
    }
    let parsed = second_life_frames(frames, data);
    let frames = match parsed {
        Ok(frames) => frames,
        Err(complaint) => {
            ctx.refuse(complaint.to_owned());
            return None;
        }
    };
    if frames
        .iter()
        .any(|frame| frame.seconds < SECOND_LIFE_SHORTEST_FRAME)
    {
        ctx.refuse("Delta times must be >0.1s".to_owned());
        return None;
    }
    let object = ctx.host().whereabouts(caller).object;
    let shape = ctx
        .host()
        .prim(caller, &object)
        .map_or(PHYSICS_SHAPE_PRIM, |root| root.physics_shape);
    if shape == PHYSICS_SHAPE_PRIM {
        ctx.refuse(
            "Error, failed to play animation. Does your linkset exceed the complexity limit for \
             keyframing?"
                .to_owned(),
        );
        return None;
    }
    let motion = Keyframes { frames, mode };
    Some(if physical(ctx) {
        KeyframedCall::Keep(motion)
    } else {
        KeyframedCall::Play(motion)
    })
}

/// Second Life's reading of a keyframe list with the data `data`, or the
/// line it refuses it with.
fn second_life_frames(frames: &[Element], data: i32) -> Result<Vec<Keyframe>, &'static str> {
    let mut out = Vec::new();
    let mut elements = frames.iter();
    loop {
        let mut frame = Keyframe {
            translation: None,
            rotation: None,
            seconds: 0.0,
        };
        let Some(first) = elements.next() else {
            return Ok(out);
        };
        let mut next = Some(first);
        if data & KFM_TRANSLATION != 0 {
            match next {
                Some(Element::Vector(vector)) => frame.translation = Some(vector.clone()),
                _ => return Err("Error, expected translation vector in keyframe list."),
            }
            next = elements.next();
        }
        if data & KFM_ROTATION != 0 {
            match next {
                Some(Element::Rotation(rotation)) => {
                    frame.rotation = Some(rotation.clone());
                }
                None => return Err("Expected delta time in keyframe list."),
                _ => return Err("Error, expected rotation in keyframe list."),
            }
            next = elements.next();
        }
        frame.seconds = match next {
            Some(Element::Float(seconds)) => *seconds,
            Some(Element::Integer(seconds)) => crate::num::int_to_float(*seconds),
            _ => return Err("Expected delta time in keyframe list."),
        };
        out.push(frame);
    }
}

/// What OpenSim makes of a keyframed call (`LSL_Api.llSetKeyframedMotion`),
/// [`None`] for one it ignores.
fn opensim_keyframed(
    ctx: &mut ScriptCtx<'_>,
    frames: &[Element],
    options: &[Element],
) -> Result<Option<KeyframedCall>, CallError> {
    if physical(ctx) {
        return Ok(None);
    }
    let integer = |element: &Element| {
        crate::library::money_permissions::opensim_integer_item(element).map_err(|exception| {
            CallError::Exception {
                id: crate::library::BuiltinId::LlSetKeyframedMotion,
                exception,
            }
        })
    };
    if frames.is_empty() {
        // The host ignores a command for an object that has no motion
        // (OpenSim's `KeyframeMotion` is null).
        if options.is_empty() {
            return Ok(Some(KeyframedCall::Stop));
        }
        let mut call = None;
        let mut index = 0_usize;
        while let Some(option) = options.get(index) {
            index = index.saturating_add(1);
            if integer(option)? != KFM_COMMAND {
                continue;
            }
            let Some(command) = options.get(index) else {
                break;
            };
            index = index.saturating_add(1);
            call = match integer(command)? {
                KFM_CMD_PLAY => Some(KeyframedCall::Resume),
                KFM_CMD_STOP => Some(KeyframedCall::Stop),
                KFM_CMD_PAUSE => Some(KeyframedCall::Pause),
                _ => call,
            };
        }
        return Ok(call);
    }
    let mut mode = KeyframeMode::Forward;
    let mut data = KFM_TRANSLATION | KFM_ROTATION;
    let mut index = 0_usize;
    while let Some(option) = options.get(index) {
        index = index.saturating_add(1);
        let option = integer(option)?;
        let remain = options.len().saturating_sub(index);
        match option {
            KFM_MODE if remain >= 1 => {
                let value = options.get(index).map(integer).transpose()?;
                index = index.saturating_add(1);
                if let Some(named) = value.and_then(mode_named) {
                    mode = named;
                }
            }
            KFM_DATA if remain >= 1 => {
                if let Some(value) = options.get(index).map(integer).transpose()? {
                    data = value;
                }
                index = index.saturating_add(1);
            }
            _ => {}
        }
    }
    let frames = opensim_frames(frames, data)?;
    Ok(Some(KeyframedCall::Play(Keyframes { frames, mode })))
}

/// OpenSim's reading of a keyframe list with the data `data`: frames of two
/// elements (three for translation and rotation), an incomplete last frame
/// dropped, each element read by `LSL_Types.list`'s typed getters, which
/// throw at an element of the wrong type.
fn opensim_frames(frames: &[Element], data: i32) -> Result<Vec<Keyframe>, CallError> {
    let throw = |exception| CallError::Exception {
        id: crate::library::BuiltinId::LlSetKeyframedMotion,
        exception,
    };
    let length = if data == KFM_TRANSLATION | KFM_ROTATION {
        3
    } else {
        2
    };
    let mut out = Vec::new();
    for frame in frames.chunks_exact(length) {
        let mut elements = frame.iter();
        let mut keyframe = Keyframe {
            translation: None,
            rotation: None,
            seconds: 0.0,
        };
        if data & KFM_TRANSLATION != 0 {
            match elements.next() {
                Some(Element::Vector(vector)) => keyframe.translation = Some(vector.clone()),
                other => {
                    return Err(throw(Exception::ItemNot {
                        expected: "Vector3",
                        given: dotnet_type(other),
                    }));
                }
            }
        }
        if data & KFM_ROTATION != 0 {
            match elements.next() {
                Some(Element::Rotation(rotation)) => {
                    keyframe.rotation = Some(normalised_rotation(rotation));
                }
                other => {
                    return Err(throw(Exception::ItemNot {
                        expected: "Quaternion",
                        given: dotnet_type(other),
                    }));
                }
            }
        }
        keyframe.seconds = match elements.next() {
            Some(Element::Float(seconds)) => *seconds,
            Some(Element::Integer(seconds)) => crate::num::int_to_float(*seconds),
            Some(Element::String(text) | Element::Key(text)) => {
                crate::library::dotnet::parse_double(text).map_or(0.0, crate::num::to_f32)
            }
            _ => return Err(throw(Exception::InvalidCast)),
        };
        out.push(keyframe);
    }
    Ok(out)
}

/// The .NET type OpenSim's list holds an element as, for an
/// `InvalidCastException`'s message.
const fn dotnet_type(element: Option<&Element>) -> &'static str {
    match element {
        Some(Element::Integer(_)) => "LSLInteger",
        Some(Element::Float(_)) => "LSLFloat",
        Some(Element::String(_) | Element::Key(_)) => "LSLString",
        Some(Element::Vector(_)) => "Vector3",
        Some(Element::Rotation(_)) => "Quaternion",
        None => "null",
    }
}

/// `rotation` at unit length, as OpenSim's `GetQuaternionItem` normalises
/// it; the identity for the zero rotation.
fn normalised_rotation(rotation: &Rotation) -> Rotation {
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
    if length == 0.0 {
        return crate::value::ZERO_ROTATION;
    }
    Rotation {
        x: rotation.x / length,
        y: rotation.y / length,
        z: rotation.z / length,
        s: rotation.s / length,
    }
}
