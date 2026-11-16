//! OpenSim's own functions that shadow the physics tranche's
//! (`server-lsl-lib-physics-vehicles`, from `server-lsl-lib-ossl`): the
//! inertia functions, `osSetPrimFloatOnWater`, `osTeleportObject` and
//! `osVolumeDetect`.
//!
//! Each is `OSSL_Api.cs`'s, on the OpenSim flavours alone (Second Life does
//! not compile them). The region's `[OSSL]` settings have already admitted
//! the call (`vm::ossl::admit`) by the time one runs.

use sl_types::lsl::{Rotation, Vector};

use crate::library::{CallError, Key};
use crate::value::Element;
use crate::vm::ScriptCtx;
use crate::vm::physics::{Inertia, PhysicsCall};

/// The inertia rotation OpenSim normalises before it keeps it
/// (`Vector4.Normalize`); the zero four-vector stays zero.
fn normalised(rotation: &Rotation) -> Rotation {
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
        return rotation.clone();
    }
    Rotation {
        x: rotation.x / length,
        y: rotation.y / length,
        z: rotation.z / length,
        s: rotation.s / length,
    }
}

/// Hands the host the inertia data `inertia`.
fn set(ctx: &mut ScriptCtx<'_>, inertia: Option<Inertia>) {
    let caller = ctx.caller();
    ctx.host().physics(caller, PhysicsCall::Inertia(inertia));
}

/// `osClearInertia`: the object's inertia is worked out from its shape
/// again.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_clear_inertia(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    set(ctx, None);
    Ok(())
}

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// `osGetInertiaData`: `[mass, centre of mass, inertia over mass, the
/// inertia rotation's x, y and z over mass]`. A physical object reads what
/// was set, or what its shape gives — the box its root fills, as the fake
/// grid's physics sees every object —, one that is not physical its mass,
/// its centre of mass from its root in the root's frame, and zero for the
/// rest (`SceneObjectGroup.GetInertiaData`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_inertia_data(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    let reading = ctx.host().physics_reading(caller);
    let object = ctx.host().whereabouts(caller).object;
    let root = ctx.host().prim(caller, &object);
    let physical = root.as_ref().is_some_and(|root| root.physics);
    let (mass, centre, inertia, aux) = match (physical, reading.inertia) {
        (true, Some(set)) => (
            set.mass,
            set.centre,
            set.inertia,
            Vector {
                x: set.rotation.x,
                y: set.rotation.y,
                z: set.rotation.z,
            },
        ),
        (true, None) => {
            let scale = root.as_ref().map_or(ZERO, |root| root.scale.clone());
            let t = reading.mass / 12.0;
            (
                reading.mass,
                ZERO,
                Vector {
                    x: t * scale.z.mul_add(scale.z, scale.y * scale.y),
                    y: t * scale.z.mul_add(scale.z, scale.x * scale.x),
                    z: t * scale.y.mul_add(scale.y, scale.x * scale.x),
                },
                ZERO,
            )
        }
        (false, _) => {
            let centre = root.as_ref().map_or(ZERO, |root| {
                let offset = Vector {
                    x: reading.centre_of_mass.x - root.position.x,
                    y: reading.centre_of_mass.y - root.position.y,
                    z: reading.centre_of_mass.z - root.position.z,
                };
                crate::ops::rotate(&offset, &conjugate(&root.rotation))
            });
            (reading.mass, centre, ZERO, ZERO)
        }
    };
    let over = |v: Vector| {
        if mass > 0.0 {
            Vector {
                x: v.x / mass,
                y: v.y / mass,
                z: v.z / mass,
            }
        } else {
            v
        }
    };
    Ok(vec![
        Element::Float(mass),
        Element::Vector(centre),
        Element::Vector(over(inertia)),
        Element::Vector(over(aux)),
    ])
}

/// The conjugate of `rotation`.
const fn conjugate(rotation: &Rotation) -> Rotation {
    Rotation {
        x: -rotation.x,
        y: -rotation.y,
        z: -rotation.z,
        s: rotation.s,
    }
}

/// `osSetInertia`: the object's mass, centre of mass, principal inertia
/// (given over the mass) and its rotation. A negative mass or inertia sets
/// nothing. The rotation's z is its y, as OpenSim reads it
/// (`new Vector4(x, y, y, s)`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_inertia(
    ctx: &mut ScriptCtx<'_>,
    mass: f32,
    centre: Vector,
    scaled: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    if mass < 0.0 || scaled.x < 0.0 || scaled.y < 0.0 || scaled.z < 0.0 {
        return Ok(());
    }
    let rotation = normalised(&Rotation {
        x: rotation.x,
        y: rotation.y,
        z: rotation.y,
        s: rotation.s,
    });
    set(
        ctx,
        Some(Inertia {
            mass,
            centre,
            inertia: Vector {
                x: mass * scaled.x,
                y: mass * scaled.y,
                z: mass * scaled.z,
            },
            rotation,
        }),
    );
    Ok(())
}

/// `osSetInertiaAsBox`: the inertia of a solid box of `size` and `mass`.
/// A negative mass sets nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_inertia_as_box(
    ctx: &mut ScriptCtx<'_>,
    mass: f32,
    size: Vector,
    centre: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    if mass < 0.0 {
        return Ok(());
    }
    let t = mass / 12.0;
    set(
        ctx,
        Some(Inertia {
            mass,
            centre,
            inertia: Vector {
                x: t * size.z.mul_add(size.z, size.y * size.y),
                y: t * size.z.mul_add(size.z, size.x * size.x),
                z: t * size.y.mul_add(size.y, size.x * size.x),
            },
            rotation: normalised(&rotation),
        }),
    );
    Ok(())
}

/// `osSetInertiaAsSphere`: the inertia of a solid sphere of `radius` and
/// `mass`, its rotation the identity. A negative mass sets nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_inertia_as_sphere(
    ctx: &mut ScriptCtx<'_>,
    mass: f32,
    radius: f32,
    centre: Vector,
) -> Result<(), CallError> {
    if mass < 0.0 {
        return Ok(());
    }
    let t = 0.4 * mass * radius * radius;
    set(
        ctx,
        Some(Inertia {
            mass,
            centre,
            inertia: Vector { x: t, y: t, z: t },
            rotation: crate::value::ZERO_ROTATION,
        }),
    );
    Ok(())
}

/// `osSetInertiaAsCylinder`: the inertia of a solid cylinder of `radius`,
/// `length` along its z and `mass`. A negative mass sets nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_inertia_as_cylinder(
    ctx: &mut ScriptCtx<'_>,
    mass: f32,
    radius: f32,
    length: f32,
    centre: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    if mass < 0.0 {
        return Ok(());
    }
    let r = radius * radius;
    let across = length.mul_add(length, 3.0 * r) * 8.333_333e-2 * mass;
    set(
        ctx,
        Some(Inertia {
            mass,
            centre,
            inertia: Vector {
                x: across,
                y: across,
                z: 0.5 * mass * r,
            },
            rotation: normalised(&rotation),
        }),
    );
    Ok(())
}

/// `osSetPrimFloatOnWater`: a physical object floats on the water, for an
/// argument of exactly 1, or no longer floats (`SceneObjectPart
/// .SetFloatOnWater`: `floatYN == 1`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_prim_float_on_water(ctx: &mut ScriptCtx<'_>, float: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host()
        .physics(caller, PhysicsCall::FloatOnWater(float == 1));
    Ok(())
}

/// `osTeleportObject`: moves the object keyed `object` within the region,
/// the host answering as OpenSim does (1, −1, −2); a key that is none is
/// shouted at (`osTeleportObject() invalid object Key`) and answered −1.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_object(
    ctx: &mut ScriptCtx<'_>,
    object: Key,
    position: Vector,
    rotation: Rotation,
    flags: i32,
) -> Result<i32, CallError> {
    if !crate::library::dotnet::is_uuid(&object.0) {
        ctx.shout_error("osTeleportObject() invalid object Key".to_owned());
        return Ok(-1);
    }
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .teleport_object(caller, &object.0, position, rotation, flags))
}

/// `osVolumeDetect`: `llVolumeDetect` for a root prim; in a child, the
/// child's own volume detect, unless the root already detects for the whole
/// linkset (`SceneObjectPart.ScriptSetVolumeDetect`). Nothing in a worn
/// object.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_volume_detect(ctx: &mut ScriptCtx<'_>, detect: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) != 0 {
        return Ok(());
    }
    let whereabouts = ctx.host().whereabouts(caller);
    if whereabouts.prim != whereabouts.object
        && ctx
            .host()
            .prim(caller, &whereabouts.object)
            .is_some_and(|root| root.volume_detect)
    {
        return Ok(());
    }
    crate::library::prim_state::ll_volume_detect(ctx, detect)
}
