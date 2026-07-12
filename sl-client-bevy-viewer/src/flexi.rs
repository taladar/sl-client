//! Flexible prims (Phase 32): fold a prim's `LLFlexibleObjectData` extra-param
//! block into the scene mirror.
//!
//! **Ingest (P32.1).** A "flexi" prim carries a flexible-object extra-param
//! ([`FlexibleData`]) marking its path as a soft chain that bends under a
//! simulated spring / gravity / wind force (set by the build tool's *Features →
//! Flexible Path* or by `llSetPrimitiveParams`). sl-proto already decodes that
//! block into a [`FlexibleData`] on `Object::extra.flexible` (a port of
//! `LLFlexibleObjectData::unpack` — the four packed tension / drag / gravity /
//! wind bytes, the two simulate-LOD "softness" bits stashed in their high bits,
//! and the trailing user-force vector); [`flexi_from_object`] lifts a present
//! block onto an [`ObjectFlexi`] component that [`apply_object`] attaches to (or
//! clears from) each object entity as its updates arrive — ready for the P32.2
//! CPU chain simulation that will deform / re-tessellate the flexi geometry.
//!
//! The reference viewer's `LLVOVolume::isFlexible` treats a prim as flexi
//! **exactly when** it carries this block (`getFlexibleObjectData()` non-null) —
//! there is no null / sentinel form the way particles carry a zero-CRC "stop"
//! system, so the ingest is a straight `Option` lift: present → attach, absent →
//! remove. A prim toggled flexi on or off in-world flips the extra-param block
//! present / absent, so refreshing the component on every update (the way
//! [`apply_light`](crate::lights) / [`apply_particles`](crate::particles) do)
//! tracks that toggle.
//!
//! Flexi is mutually exclusive with server physics — the reference forces a flexi
//! prim `FLAGS_PHANTOM` on and `FLAGS_USE_PHYSICS` off (`setIsFlexible`), so a
//! flexi prim never also carries the P31.2 physics-body marker. The deformation
//! is purely client-side (the simulator sends no per-frame flexi state), which is
//! why the whole feature is a viewer-side simulation built on the block ingested
//! here.
//!
//! The component rides the **object entity** (which carries the prim's world
//! position / rotation), so P32.2's chain simulation will ride the prim's
//! transform the way the reference viewer's `LLVolumeImplFlexible` anchors its
//! chain at the prim's root.
//!
//! [`apply_object`]: crate::objects
//!
//! Reference (read-only): Firestorm `LLVOVolume::isFlexible` / `setIsFlexible`
//! (`indra/newview/llvovolume.cpp`), `LLVolumeImplFlexible`
//! (`indra/newview/llflexibleobject.cpp`), and `LLFlexibleObjectData`
//! (`indra/llprimitive/llprimitive.{h,cpp}`).

use bevy::prelude::*;
use sl_client_bevy::{FlexibleData, Object};

/// A component marking an object entity as a **flexible ("flexi") prim**, carrying
/// the decoded `LLFlexibleObjectData` parameters in Second Life semantics — ready
/// for P32.2 to drive a CPU chain simulation and deform the prim's path.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// [`apply_object`](crate::objects) as its updates arrive. See
/// [`flexi_from_object`] for the present-vs-absent lift.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct ObjectFlexi {
    /// The decoded flexible-object parameters: the simulate-LOD softness (0–3),
    /// path tension (stiffness), air friction (damping), gravity on the tip, wind
    /// sensitivity, and the constant user force pushing the path.
    pub(crate) data: FlexibleData,
}

/// Lift an object's flexible-object block onto an [`ObjectFlexi`], or `None` when
/// the object is not (or is no longer) a flexi prim.
///
/// Mirrors the reference viewer's `LLVOVolume::isFlexible`: a prim is flexi
/// exactly when it carries a flexible-object extra-param block
/// (`getFlexibleObjectData()`), so this is a straight `Option` lift with no
/// sentinel to reject (unlike the particle system's zero-CRC "null" form).
pub(crate) fn flexi_from_object(object: &Object) -> Option<ObjectFlexi> {
    object
        .extra
        .flexible
        .clone()
        .map(|data| ObjectFlexi { data })
}

/// Reconcile an object entity's [`ObjectFlexi`] component (P32.1) with its current
/// flexible-object block: insert / refresh it when the prim is flexi, remove it
/// when the prim was made rigid in-world (the block dropped) or never was flexi.
/// Called on both the spawn and update paths so a prim toggled flexi on or off
/// between updates is tracked, the way [`apply_light`](crate::lights) /
/// [`apply_particles`](crate::particles) are.
pub(crate) fn apply_flexi(entity: Entity, flexi: Option<ObjectFlexi>, commands: &mut Commands) {
    match flexi {
        Some(flexi) => {
            let data = &flexi.data;
            debug!(
                "object flexi prim: softness={} tension={:.2} air_friction={:.2} \
                 gravity={:.2} wind={:.2} user_force=({:.2},{:.2},{:.2})",
                data.softness,
                data.tension,
                data.air_friction,
                data.gravity,
                data.wind_sensitivity,
                data.user_force.x,
                data.user_force.y,
                data.user_force.z,
            );
            commands.entity(entity).insert(flexi);
        }
        None => {
            commands.entity(entity).remove::<ObjectFlexi>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectFlexi, flexi_from_object};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{FlexibleData, Object, Vector};

    /// A minimal plain prim object with no extra params — the fixture the flexi
    /// tests decorate.
    fn bare_object() -> Object {
        use sl_client_bevy::{
            CircuitId, ObjectMotion, RegionHandle, RegionLocalObjectId, Rotation, Uuid,
        };
        // A fresh zero vector per use (`Vector` derives neither `Copy` nor
        // `Default`).
        const fn zero() -> Vector {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode: 9,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: ObjectMotion {
                position: zero(),
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::from_u128(0),
            sound: Uuid::from_u128(0),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// A representative flexi block (mid-range softness / tension / gravity, a
    /// small steady user force).
    fn flexi_data() -> FlexibleData {
        FlexibleData {
            softness: 2,
            tension: 1.0,
            air_friction: 2.0,
            gravity: 0.3,
            wind_sensitivity: 0.0,
            user_force: Vector {
                x: 0.0,
                y: 0.0,
                z: -0.5,
            },
        }
    }

    /// An object with no flexible-object block is not a flexi prim.
    #[test]
    fn no_flexible_block_is_none() {
        assert_eq!(flexi_from_object(&bare_object()), None);
    }

    /// A prim carrying a flexible-object block lifts into a component holding it.
    #[test]
    fn flexi_block_becomes_a_component() {
        let mut object = bare_object();
        let data = flexi_data();
        object.extra.flexible = Some(data.clone());
        assert_eq!(flexi_from_object(&object), Some(ObjectFlexi { data }));
    }
}
