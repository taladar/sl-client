//! Particle systems (Phase 30): fold a prim's `LLPartSysData` particle-system
//! block into the scene mirror.
//!
//! **Ingest (P30.1).** Each in-world prim may carry a particle-system block
//! (`PSBlock` / `LLPartSysData`, set by `llParticleSystem`) marking it as a
//! particle *source* — an emitter of textured billboard particles. sl-proto
//! already decodes that block into a [`ParticleSystem`] on `Object::particles`
//! (both the legacy 86-byte and the modern glow/blend-extended wire forms);
//! [`particles_from_object`] lifts a *live* system onto an
//! [`ObjectParticleSystem`] component that [`apply_object`] attaches to (or
//! clears from) each object entity as its updates arrive — ready for the P30.2
//! CPU simulation + camera-facing billboard render.
//!
//! A system whose CRC is zero is the reference viewer's "null" particle system
//! (`LLPartSysData::isNullPS`) — the sentinel `llParticleSystem([])` sends to
//! *stop* emitting — so it clears the component rather than attaching a dead
//! emitter, mirroring `LLViewerPartSourceScript::unpackPSS` returning `NULL`
//! (which makes `unpackParticleSource` mark the old source dead).
//!
//! The component is attached to the **object entity** (which carries the prim's
//! world position / rotation), so P30.2's emitter rides the prim's transform the
//! way the reference viewer's `LLViewerPartSourceScript` tracks its source
//! object.
//!
//! [`apply_object`]: crate::objects
//!
//! Reference (read-only): Firestorm `LLViewerObject::unpackParticleSource` /
//! `LLViewerPartSourceScript::unpackPSS` (`indra/newview/llviewerpartsource.cpp`),
//! `LLPartSysData` (`indra/llmessage/llpartdata.cpp`).

use bevy::prelude::*;
use sl_client_bevy::{Object, ParticleSystem};

/// A component marking an object entity as a **particle source**, carrying the
/// decoded `LLPartSysData` particle-system parameters in Second Life semantics —
/// ready for P30.2 to drive a CPU particle simulation and render its particles as
/// camera-facing billboards.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// [`apply_object`](crate::objects) as its updates arrive. Only a *live* system
/// (non-zero CRC) is carried; see [`particles_from_object`].
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct ObjectParticleSystem {
    /// The decoded particle system: the source parameters (pattern, burst / age
    /// timing, emission angles / radius / speed, angular velocity, acceleration,
    /// texture, target) plus the template particle parameters it emits
    /// (per-particle age, start / end colour and scale, glow, blend).
    pub(crate) system: ParticleSystem,
}

/// Lift a live particle system off an object into an [`ObjectParticleSystem`], or
/// `None` when the object is not (or is no longer) a particle source.
///
/// Returns `None` in the two cases the reference viewer treats as "no source":
/// the object carries no particle-system block at all (`Object::particles` is
/// `None` — sl-proto already yields `None` for an empty `PSBlock`, matching
/// `isNullPS`'s zero-size check), or it carries a **null** system whose CRC is
/// zero (`LLPartSysData::isNullPS` — the `llParticleSystem([])` stop sentinel).
pub(crate) fn particles_from_object(object: &Object) -> Option<ObjectParticleSystem> {
    let system = object.particles.clone()?;
    // A zero-CRC system is the reference viewer's "null" particle system: the
    // sentinel a script sends to stop emitting. `isNullPS` rejects it, so it is
    // not a live source.
    if system.crc == 0 {
        return None;
    }
    Some(ObjectParticleSystem { system })
}

/// Reconcile an object entity's [`ObjectParticleSystem`] component (P30.1) with
/// its current particle-system block: insert / refresh it when the object is a
/// live particle source, remove it when the source was cleared in-world (a null
/// system) or the object stopped carrying one. Called on both the spawn and
/// update paths so a source toggled on or off between updates is tracked, the way
/// [`apply_light`](crate::objects) is for lights.
pub(crate) fn apply_particles(
    entity: Entity,
    particles: Option<ObjectParticleSystem>,
    commands: &mut Commands,
) {
    match particles {
        Some(particles) => {
            let system = &particles.system;
            debug!(
                "object particle source: pattern={:#04x} flags={:#x} burst_rate={:.2}s \
                 burst_count={} part_max_age={:.2}s texture={:?} target={:?}",
                system.pattern,
                system.flags,
                system.burst_rate,
                system.burst_part_count,
                system.part_max_age,
                system.texture_id,
                system.target_id,
            );
            commands.entity(entity).insert(particles);
        }
        None => {
            commands.entity(entity).remove::<ObjectParticleSystem>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectParticleSystem, particles_from_object};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Object, ParticleSystem, Vector};

    /// A minimal plain prim object with no particle system — the fixture the
    /// particle tests decorate.
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

    /// A minimal live particle system (non-zero CRC) with defaulted particle
    /// template fields.
    fn live_system() -> ParticleSystem {
        use sl_client_bevy::particle_pattern;
        ParticleSystem {
            crc: 0xABCD,
            flags: 0,
            pattern: particle_pattern::EXPLODE,
            max_age: 0.0,
            start_age: 0.0,
            inner_angle: 0.0,
            outer_angle: 0.0,
            burst_rate: 0.1,
            burst_radius: 1.0,
            burst_speed_min: 1.0,
            burst_speed_max: 1.0,
            burst_part_count: 4,
            angular_velocity: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            acceleration: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            texture_id: None,
            target_id: None,
            part_flags: 0,
            part_max_age: 2.0,
            part_start_color: [255; 4],
            part_end_color: [255; 4],
            part_start_scale: [1.0, 1.0],
            part_end_scale: [1.0, 1.0],
            part_start_glow: 0.0,
            part_end_glow: 0.0,
            part_blend_func_source: 7,
            part_blend_func_dest: 9,
        }
    }

    /// An object with no particle system is not a source.
    #[test]
    fn no_particle_system_is_none() {
        assert_eq!(particles_from_object(&bare_object()), None);
    }

    /// A live (non-zero CRC) particle system decodes into a component carrying it.
    #[test]
    fn live_system_becomes_a_component() {
        let mut object = bare_object();
        let system = live_system();
        object.particles = Some(system.clone());
        assert_eq!(
            particles_from_object(&object),
            Some(ObjectParticleSystem { system })
        );
    }

    /// A null system (CRC zero — the `llParticleSystem([])` stop sentinel) is not
    /// a live source, so it clears rather than attaches a dead emitter.
    #[test]
    fn null_system_is_none() {
        let mut object = bare_object();
        let mut system = live_system();
        system.crc = 0;
        object.particles = Some(system);
        assert_eq!(particles_from_object(&object), None);
    }
}
