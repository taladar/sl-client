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
//!
//! **Simulate + render (P30.2).** [`drive_particles`] runs a CPU particle
//! simulation for every [`ObjectParticleSystem`] source each frame and renders
//! its live particles as camera-facing textured billboards. It is a port of the
//! reference viewer's split of the work:
//!
//! - [`Emitter::emit`] ports `LLViewerPartSourceScript::update` — the burst timing
//!   accumulator (emit `burst_part_count` particles every `burst_rate` seconds),
//!   the source's angular-velocity rotation accumulation, its `max_age` death, and
//!   the four emission patterns (`DROP` / `EXPLODE` / `ANGLE` / `ANGLE_CONE`).
//! - [`Particle::integrate`] ports `LLViewerPartGroup::updateParticles` — the
//!   velocity/acceleration Verlet step, `TARGET_POS` / `TARGET_LINEAR` attraction,
//!   `BOUNCE`, `FOLLOW_SRC` drift, the per-particle colour / scale / glow
//!   interpolation, and the age-out kill.
//! - [`build_cloud_mesh`] ports `LLVOPartGroup::getGeometry` — each particle is a
//!   quad built to face the camera (with the `FOLLOW_VELOCITY` re-orientation),
//!   baked into one dynamic mesh per source, tinted by the per-vertex particle
//!   colour and textured via the shared texture pipeline (or a procedural soft
//!   sprite when the source names no texture, mirroring `sDefaultParticleImagep`).
//!
//! The simulation runs in **Bevy world space** (Y-up): a source's position and
//! orientation come from its object entity's `GlobalTransform`, so the emitter
//! tracks the prim as it moves. Emission directions are built in Second Life space
//! (where the wire angles / axes are defined) and carried into Bevy by the single
//! [basis change](crate::coords). Each source's live particles are baked into one
//! [`Mesh`] hung off a dedicated **world-space cloud entity** (not a child of the
//! source, mirroring `LLVOPartGroup` being its own spatial object), so the
//! billboards are placed by their absolute baked vertex positions.
//!
//! Deliberate simplifications from the reference (documented so they are not
//! mistaken for bugs): region **wind** is not ingested, so the `WIND` flag is a
//! no-op (particles keep their own velocity); the camera-distance / particle-size
//! **emission rate throttling** (an LOD optimisation) is not ported, only the hard
//! global particle cap; and the `RIBBON` / `BEAM` connected-strip particle kinds
//! render as ordinary billboards. A `TARGET_*` source whose target object is not
//! resolved falls back to its own position (the reference's own fallback).
//!
//! **HUD particles (P35.4).** A particle source on a **HUD attachment** (P35.1)
//! draws snow / rain / sparkles / a damage flash across the *screen*, not into the
//! world. The reference viewer models it as a second particle partition end to end
//! (`LL_PART_HUD` → `LLVOHUDPartGroup`, partition `PARTITION_HUD_PARTICLE`, render
//! type `RENDER_TYPE_HUD_PARTICLES`, billboarded against the HUD view point rather
//! than the eye, drawn unlit, and gated behind `RenderHUDParticles`).
//!
//! Here the HUD subtree already lives in a fixed patch of Bevy world space at the
//! origin (the [`HudScreen`](crate::hud::HudScreen) carries the basis change and
//! never moves), rendered *only* by the orthographic
//! [`HudCamera`] on [`HUD_RENDER_LAYER`]. So a HUD source's
//! `GlobalTransform` is already in that space and its particles integrate in world
//! space like any other source — the whole HUD/world split reduces to two render
//! decisions: put the cloud entity on [`HUD_RENDER_LAYER`] (so the HUD camera draws
//! it and the fly camera does not — the exact bug P35.1 fixed for HUD *geometry*,
//! one pipeline over), billboard its quads at the fixed HUD camera instead of the
//! fly camera, and draw it unlit (no light is on the HUD layer, so a lit HUD
//! material renders black). Mirroring the reference's `RenderHUDParticles` flag,
//! `SL_VIEWER_DISABLE_HUD_PARTICLES` suppresses HUD emitters entirely (defaulted
//! **on** for us — we have no settings UI and the point is to see it work).

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{DecodedTexture, Object, ParticleSystem, particle_pattern, to_bevy_image};

use crate::camera::ViewerCamera;
use crate::coords::sl_to_bevy_rotation;
use crate::hud::{HUD_RENDER_LAYER, HudCamera, on_hud_layer};
use crate::render_priority::AVATAR_BOOST_PRIORITY;
use crate::textures::TextureManager;

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

// ---------------------------------------------------------------------------
// P30.2 — CPU particle simulation + camera-facing billboard render.
// ---------------------------------------------------------------------------

/// The global cap on live particles across every source, mirroring the reference
/// viewer's `LLViewerPartSim::sMaxParticleCount` (4096). Emission stops once the
/// cap is reached and resumes as particles age out, so a scene full of emitters
/// cannot swamp the simulation.
const MAX_PARTICLES: usize = 4096;

/// The `LLPartData` per-particle flag masks (`indra/llmessage/llpartdata.h`),
/// mirrored here so the simulation reads the same bits the wire carries on
/// [`ParticleSystem::part_flags`].
mod part_flags {
    /// Interpolate colour from start to end over the particle's life
    /// (`LL_PART_INTERP_COLOR_MASK`).
    pub(super) const INTERP_COLOR: u32 = 0x01;
    /// Interpolate scale from start to end over the particle's life
    /// (`LL_PART_INTERP_SCALE_MASK`).
    pub(super) const INTERP_SCALE: u32 = 0x02;
    /// Bounce off the source's Z plane (`LL_PART_BOUNCE_MASK`).
    pub(super) const BOUNCE: u32 = 0x04;
    /// Follow the source position with no rotation (`LL_PART_FOLLOW_SRC_MASK`).
    pub(super) const FOLLOW_SRC: u32 = 0x10;
    /// Orient the billboard along the particle velocity
    /// (`LL_PART_FOLLOW_VELOCITY_MASK`).
    pub(super) const FOLLOW_VELOCITY: u32 = 0x20;
    /// Interpolate the velocity toward the source's target
    /// (`LL_PART_TARGET_POS_MASK`).
    pub(super) const TARGET_POS: u32 = 0x40;
    /// Linearly interpolate the position from source to target
    /// (`LL_PART_TARGET_LINEAR_MASK`).
    pub(super) const TARGET_LINEAR: u32 = 0x80;
    /// The particle is emissive (fullbright) rather than lit
    /// (`LL_PART_EMISSIVE_MASK`).
    pub(super) const EMISSIVE: u32 = 0x100;
}

/// The source-flag bit marking a system as using the "new" (correct) cone angle
/// convention (`LLPartSysData::LL_PART_USE_NEW_ANGLE`); when clear, the legacy
/// emission applies an extra outer-angle rotation.
const LL_PART_USE_NEW_ANGLE: u32 = 0x02;

/// The `LL_PART_BF_ONE` blend factor (`LLPartData` blend-func enum): a
/// destination factor of `ONE` is additive blending (the glow / fire look), any
/// other is ordinary source-over alpha blending.
const LL_PART_BF_ONE: u8 = 0;

/// A small deterministic pseudo-random generator (a xorshift64* variant) standing
/// in for the reference viewer's `ll_frand`. Seeded per source so different
/// emitters diverge and the simulation is reproducible in tests (the workspace
/// bans wall-clock / OS randomness in library code paths that must stay testable).
#[derive(Debug, Clone)]
struct Rng {
    /// The generator state; kept non-zero (a zero state is a xorshift fixed point).
    state: u64,
}

impl Rng {
    /// Seed the generator, forcing a non-zero state.
    const fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    /// The next raw 64-bit output.
    const fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A uniform `f32` in `[0, 1)` — the reference viewer's `ll_frand()`. Built by
    /// stuffing 23 random bits into an `f32` mantissa (yielding `[1, 2)`) and
    /// subtracting one, so no lossy integer→float cast is needed.
    fn frand(&mut self) -> f32 {
        let mantissa = u32::try_from(self.next_u64() >> 41).unwrap_or(0);
        f32::from_bits(0x3f80_0000 | mantissa) - 1.0
    }

    /// A uniform `f32` in `[0, max)` — the reference viewer's `ll_frand(max)`.
    fn frand_max(&mut self, max: f32) -> f32 {
        self.frand() * max
    }

    /// A random unit vector uniformly distributed over the sphere, matching the
    /// reference `EXPLODE` pattern's rejection-sampled direction (reject points
    /// outside the unit ball or too near the origin, then normalise).
    fn unit_vector(&mut self) -> Vec3 {
        // Bounded so a pathological RNG cannot loop forever; the reference's own
        // acceptance region covers most of the cube, so a hit is near-certain.
        for _ in 0..32 {
            let v = Vec3::new(
                self.frand_max(2.0) - 1.0,
                self.frand_max(2.0) - 1.0,
                self.frand_max(2.0) - 1.0,
            );
            let mag2 = v.length_squared();
            if (0.01..=1.0).contains(&mag2) {
                return v.normalize();
            }
        }
        Vec3::Z
    }
}

/// One live particle, its state in **Bevy world space** (Y-up). Ported from the
/// reference viewer's `LLViewerPart`.
#[derive(Debug, Clone)]
struct Particle {
    /// The world position.
    pos: Vec3,
    /// The world velocity, metres per second.
    velocity: Vec3,
    /// The constant world acceleration applied each step (`mPartAccel`).
    accel: Vec3,
    /// The seconds this particle has been alive.
    age: f32,
    /// The particle's maximum lifetime, seconds (`mMaxAge`).
    max_age: f32,
    /// The per-particle flags (`part_flags::*`).
    flags: u32,
    /// The start / end RGBA colours (wire bytes) interpolated across the life.
    start_color: [u8; 4],
    /// The end RGBA colour (wire bytes).
    end_color: [u8; 4],
    /// The start / end `(x, y)` billboard size, metres.
    start_scale: [f32; 2],
    /// The end `(x, y)` billboard size, metres.
    end_scale: [f32; 2],
    /// The current interpolated colour, normalised to `0.0..=1.0` for the vertex
    /// colour.
    color: [f32; 4],
    /// The current interpolated `(x, y)` billboard size, metres.
    scale: [f32; 2],
    /// The `FOLLOW_SRC` position offset from the source, retained between steps so
    /// the particle drifts *with* the moving source (`mPosOffset`).
    pos_offset: Vec3,
}

impl Particle {
    /// Advance this particle by `dt` seconds against its source and target world
    /// positions, returning whether it is still alive (young enough). A port of
    /// `LLViewerPartGroup::updateParticles`.
    fn integrate(&mut self, dt: f32, src: Vec3, target: Vec3) -> bool {
        let cur_time = self.age + dt;
        let frac = if self.max_age > 0.0 {
            (cur_time / self.max_age).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // Drift with the source before the physics step.
        if self.flags & part_flags::FOLLOW_SRC != 0 {
            self.pos = v_add(src, self.pos_offset);
        }

        // WIND is a no-op here (region wind is not ingested — see the module docs).

        // Steer the velocity toward the target (a damped approach).
        if self.flags & part_flags::TARGET_POS != 0 {
            let remaining = (self.max_age - self.age).max(1.0e-4);
            let step = (dt / remaining).clamp(0.0, 0.1) * 5.0;
            let delta = v_scale(v_sub(target, self.pos), 1.0 / remaining);
            self.velocity = v_add(v_scale(self.velocity, 1.0 - step), v_scale(delta, step));
        }

        if self.flags & part_flags::TARGET_LINEAR != 0 {
            // Ride a straight line from source to target over the particle's life.
            let delta = v_sub(target, src);
            self.pos = v_add(src, v_scale(delta, frac));
            self.velocity = delta;
        } else {
            // Ordinary velocity/acceleration integration (`x += v·dt + ½·a·dt²`).
            self.pos = v_add(
                v_add(self.pos, v_scale(self.velocity, dt)),
                v_scale(self.accel, 0.5 * dt * dt),
            );
            self.velocity = v_add(self.velocity, v_scale(self.accel, dt));
        }

        // Bounce off the source's up-plane (Bevy Y is Second Life Z / up).
        if self.flags & part_flags::BOUNCE != 0 {
            let dz = self.pos.y - src.y;
            if dz < 0.0 {
                self.pos.y += -2.0 * dz;
                self.velocity.y *= -0.75;
            }
        }

        // Re-derive the source offset after moving, so `FOLLOW_SRC` tracks a moving
        // source next step.
        if self.flags & part_flags::FOLLOW_SRC != 0 {
            self.pos_offset = v_sub(self.pos, src);
        }

        // Colour: interpolate start→end when flagged, else hold the start colour.
        self.color = if self.flags & part_flags::INTERP_COLOR != 0 {
            lerp_color(self.start_color, self.end_color, frac)
        } else {
            color_to_f32(self.start_color)
        };

        // Scale: interpolate start→end when flagged, else hold the start scale.
        self.scale = if self.flags & part_flags::INTERP_SCALE != 0 {
            [
                lerp(self.start_scale[0], self.end_scale[0], frac),
                lerp(self.start_scale[1], self.end_scale[1], frac),
            ]
        } else {
            self.start_scale
        };

        self.age = cur_time;
        // The reference kills a particle once its age passes its max age.
        self.age <= self.max_age
    }
}

/// The per-source emitter runtime state, ported from the reference viewer's
/// `LLViewerPartSourceScript`: the burst-timing accumulators, the accumulated
/// angular-velocity rotation, and the source's own age (for a `max_age` death).
#[derive(Debug, Clone)]
struct Emitter {
    /// The source's total elapsed time, seconds (`mLastUpdateTime`).
    last_update_time: f32,
    /// The time of the last emitted burst, seconds (`mLastPartTime`).
    last_part_time: f32,
    /// The accumulated rotation from the source's angular velocity, in Second Life
    /// space (`mRotation`), applied to `ANGLE` / `ANGLE_CONE` emission directions.
    rotation: Quat,
    /// Whether the source has outlived its `max_age` and stopped emitting.
    dead: bool,
    /// The per-source pseudo-random generator.
    rng: Rng,
}

impl Emitter {
    /// A fresh emitter seeded from the source entity's bits.
    const fn new(seed: u64) -> Self {
        Self {
            last_update_time: 0.0,
            last_part_time: 0.0,
            rotation: Quat::IDENTITY,
            dead: false,
            rng: Rng::new(seed),
        }
    }

    /// Emit any particles due this frame, appending them to `particles` and
    /// bumping the shared live count `total` (never past `cap`). A port of
    /// `LLViewerPartSourceScript::update`'s burst loop.
    ///
    /// `src` is the source's world position and `q_sl` its Second Life-space world
    /// rotation (both from its object entity's `GlobalTransform`).
    #[expect(
        clippy::too_many_arguments,
        reason = "one burst-loop step needs the source pose, output buffer, and the shared cap"
    )]
    fn emit(
        &mut self,
        system: &ParticleSystem,
        dt: f32,
        src: Vec3,
        q_sl: Quat,
        particles: &mut Vec<Particle>,
        total: &mut usize,
        cap: usize,
    ) {
        if self.dead {
            return;
        }
        let old_update_time = self.last_update_time;
        self.last_update_time += dt;
        let mut dt_update = self.last_update_time - self.last_part_time;

        // A source with a finite max age dies (and stops emitting) once it outlives
        // it, mirroring `setDead()`.
        if system.max_age > 0.0 && (system.start_age + self.last_update_time) > system.max_age {
            self.dead = true;
            return;
        }

        let mut first_run = old_update_time <= 0.0;
        // Clamp a long stall so a huge frame gap cannot spawn an unbounded backlog.
        let max_time = (10.0 * system.burst_rate).max(1.0);
        dt_update = dt_update.min(max_time);

        while dt_update > system.burst_rate || first_run {
            first_run = false;

            // Accumulate the source rotation by its angular velocity (Second Life
            // space), or reset it when the source is not spinning.
            let av = vector_to_vec3(&system.angular_velocity);
            let av_mag = av.length();
            if av_mag != 0.0 {
                let dquat = Quat::from_axis_angle(av.normalize(), dt * av_mag);
                self.rotation = self.rotation.mul_quat(dquat);
            } else {
                self.rotation = Quat::IDENTITY;
            }

            if *total >= cap {
                self.last_part_time = self.last_update_time;
                break;
            }

            for _ in 0..system.burst_part_count {
                if *total >= cap {
                    break;
                }
                particles.push(self.spawn_particle(system, src, q_sl));
                *total = total.saturating_add(1);
            }

            self.last_part_time = self.last_update_time;
            dt_update -= system.burst_rate;
        }
    }

    /// Build one freshly emitted particle for the source's pattern, positioned and
    /// launched per `LLViewerPartSourceScript::update`'s pattern branches.
    fn spawn_particle(&mut self, system: &ParticleSystem, src: Vec3, q_sl: Quat) -> Particle {
        let accel = sl_to_bevy_rotation().mul_vec3(vector_to_vec3(&system.acceleration));
        let (pos, velocity) = self.launch(system, src, q_sl);
        Particle {
            pos,
            velocity,
            accel,
            age: 0.0,
            max_age: system.part_max_age,
            flags: system.part_flags,
            start_color: system.part_start_color,
            end_color: system.part_end_color,
            start_scale: system.part_start_scale,
            end_scale: system.part_end_scale,
            color: color_to_f32(system.part_start_color),
            scale: system.part_start_scale,
            pos_offset: Vec3::ZERO,
        }
    }

    /// Compute a new particle's world position and velocity for the source's
    /// emission pattern.
    fn launch(&mut self, system: &ParticleSystem, src: Vec3, q_sl: Quat) -> (Vec3, Vec3) {
        use particle_pattern::{ANGLE, ANGLE_CONE, ANGLE_CONE_EMPTY, DROP, EXPLODE};
        if system.pattern & DROP != 0 {
            // Dropped straight from the source with no initial velocity.
            (src, Vec3::ZERO)
        } else if system.pattern & EXPLODE != 0 {
            // Blown outward in a random direction (frame-independent, so built
            // directly in Bevy space).
            let dir = self.rng.unit_vector();
            let speed = self.burst_speed(system);
            (
                v_add(src, v_scale(dir, system.burst_radius)),
                v_scale(dir, speed),
            )
        } else if system.pattern & (ANGLE | ANGLE_CONE | ANGLE_CONE_EMPTY) != 0 {
            // Emitted within a cone about the source's up axis. The direction is
            // built in Second Life space (where the wire angles are defined), then
            // carried into Bevy by the basis change.
            let dir_bevy = self.cone_direction(system, q_sl);
            let speed = self.burst_speed(system);
            (
                v_add(src, v_scale(dir_bevy, system.burst_radius)),
                v_scale(dir_bevy, speed),
            )
        } else {
            // Unknown pattern: behave like `DROP` (the reference's fall-through).
            (src, Vec3::ZERO)
        }
    }

    /// A random launch speed in `[burst_speed_min, burst_speed_max]`.
    fn burst_speed(&mut self, system: &ParticleSystem) -> f32 {
        system.burst_speed_min
            + self
                .rng
                .frand_max(system.burst_speed_max - system.burst_speed_min)
    }

    /// The unit launch direction (in Bevy space) for the `ANGLE` / `ANGLE_CONE`
    /// patterns — a port of the reference's `part_dir_vector` construction, built
    /// in Second Life space then mapped to Bevy.
    fn cone_direction(&mut self, system: &ParticleSystem, q_sl: Quat) -> Vec3 {
        use particle_pattern::{ANGLE_CONE, ANGLE_CONE_EMPTY};
        // Start pointing up the source's Second Life Z axis.
        let mut dir = Vec3::Z;
        // A random angle between the inner and outer cone half-angles, on a random
        // side of the axis.
        let mut angle =
            system.inner_angle + self.rng.frand_max(system.outer_angle - system.inner_angle);
        if self.rng.frand() < 0.5 {
            angle = -angle;
        }
        dir = Quat::from_axis_angle(Vec3::X, angle).mul_vec3(dir);
        // A full cone spins the tilted direction around the axis; a plain `ANGLE`
        // stays on the X-tilt plane.
        if system.pattern & (ANGLE_CONE | ANGLE_CONE_EMPTY) != 0 {
            let spin = self.rng.frand_max(4.0 * core::f32::consts::PI);
            dir = Quat::from_axis_angle(Vec3::Z, spin).mul_vec3(dir);
        }
        // The legacy (non-"new"-angle) convention applies an extra outer-angle tilt.
        if system.flags & LL_PART_USE_NEW_ANGLE == 0 {
            dir = Quat::from_axis_angle(Vec3::X, system.outer_angle).mul_vec3(dir);
        }
        // Apply the source's own orientation, then its accumulated spin (both in
        // Second Life space), then carry the whole direction into Bevy.
        dir = q_sl.mul_vec3(dir);
        dir = self.rotation.mul_vec3(dir);
        sl_to_bevy_rotation().mul_vec3(dir)
    }
}

/// The render + simulation state for one particle source, keyed by its object
/// entity in [`ParticleSim`].
struct Cloud {
    /// The source emitter state.
    emitter: Emitter,
    /// The source's live particles.
    particles: Vec<Particle>,
    /// The world-space entity carrying this source's baked billboard mesh (not a
    /// child of the object entity — its geometry is in absolute world coordinates).
    entity: Entity,
    /// The dynamic billboard mesh, rebuilt each frame from `particles`.
    mesh: Handle<Mesh>,
    /// The cloud's material (blend mode + texture), rebuilt only when the source
    /// system changes.
    material: Handle<StandardMaterial>,
    /// The system this cloud's emitter and material were last configured for; a
    /// change resets the emitter and re-derives the material.
    system: ParticleSystem,
    /// Whether the cloud's diffuse texture has been resolved onto its material yet.
    texture_applied: bool,
    /// Whether the cloud entity is currently visible (has live geometry). Tracked so
    /// the `Visibility` component is only rewritten on a change, and so an empty
    /// (zero-particle) cloud's mesh is left untouched rather than re-inserted empty
    /// every frame — which trips bevy's mesh allocator (R26).
    visible: bool,
    /// Whether this source is a **HUD attachment** (P35.4): its cloud entity sits on
    /// [`HUD_RENDER_LAYER`] (drawn by the HUD camera, not the fly camera), billboards
    /// against the fixed HUD camera, and renders unlit. Tracked so a source whose HUD
    /// classification arrives a frame late (the layer propagates in `PostUpdate`) is
    /// re-layered when it flips.
    is_hud: bool,
}

/// The scene-wide particle simulation state (P30.2): one [`Cloud`] per live
/// particle source. Rebuilt incrementally by [`drive_particles`] each frame.
#[derive(Resource, Default)]
pub(crate) struct ParticleSim {
    /// Live clouds keyed by their source object entity.
    clouds: HashMap<Entity, Cloud>,
}

impl ParticleSim {
    /// The world-space centroid of the cloud holding the most live particles, with
    /// that count — the debug focus target for [`focus_camera_on_particles`]. `None`
    /// when no cloud has any live particles yet.
    fn busiest_centroid(&self) -> Option<(Vec3, usize)> {
        let cloud = self
            .clouds
            .values()
            .filter(|cloud| !cloud.particles.is_empty())
            .max_by_key(|cloud| cloud.particles.len())?;
        let count = cloud.particles.len();
        let mut sum = Vec3::ZERO;
        for part in &cloud.particles {
            sum = v_add(sum, part.pos);
        }
        // `count` is non-zero (the filter kept only non-empty clouds).
        let inv = 1.0 / f32::from(u16::try_from(count).unwrap_or(u16::MAX));
        Some((v_scale(sum, inv), count))
    }
}

/// A debug affordance (env `SL_VIEWER_PARTICLE_FOCUS`): once any source has live
/// particles, put the camera in **flycam** looking at the busiest particle cloud
/// from a few metres back, so an unattended screenshot run frames a real emitter
/// without hand-aiming. Runs after [`drive_particles`] (so this frame's particles
/// are in) and after [`position_camera`](crate::camera::position_camera) (so it
/// overrides the follow pose). Flycam is the only mode whose pose a system may
/// write directly (the others recompute it), so it switches there.
pub(crate) fn focus_camera_on_particles(
    sim: Res<ParticleSim>,
    mut mode: ResMut<crate::camera::CameraMode>,
    mut camera: Query<(&mut Transform, &mut crate::camera::CameraRig), With<ViewerCamera>>,
    mut enabled: Local<Option<bool>>,
) {
    let on = *enabled.get_or_insert_with(|| std::env::var_os("SL_VIEWER_PARTICLE_FOCUS").is_some());
    if !on {
        return;
    }
    let Some((centroid, _count)) = sim.busiest_centroid() else {
        return;
    };
    let Ok((mut transform, mut rig)) = camera.single_mut() else {
        return;
    };
    // Stand back along +X/+Y from the cloud and look at it, in flycam so the pose
    // sticks (and the rig aim is seeded so the flycam driver reproduces it).
    *mode = crate::camera::CameraMode::Flycam;
    let eye = v_add(centroid, Vec3::new(6.0, 3.0, 6.0));
    let look = Vec3::new(centroid.x - eye.x, centroid.y - eye.y, centroid.z - eye.z);
    rig.aim_along(look);
    *transform = Transform::from_translation(eye).looking_at(centroid, Vec3::Y);
}

/// A procedural soft round particle sprite, uploaded once at startup and used
/// whenever a source names no texture — the counterpart of the reference viewer's
/// bundled `sDefaultParticleImagep` ("pixiesmall.j2c"), which is likewise a soft
/// white blob.
#[derive(Resource)]
pub(crate) struct DefaultParticleImage(Handle<Image>);

/// Startup: build and upload the procedural default particle sprite.
pub(crate) fn setup_particles(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let handle = images.add(default_particle_image());
    commands.insert_resource(DefaultParticleImage(handle));
}

/// Build the procedural default particle sprite: a small white square whose alpha
/// falls off smoothly to zero at the edges (a round soft blob).
///
/// Sampled with a **repeating** address mode, like every other texture this
/// viewer builds. It makes no visual difference here — a billboard quad's UVs
/// span exactly `[0, 1]`, so nothing ever samples off the end — but Bevy's
/// default is clamp-to-edge, and a texture path that leaves the default in place
/// is the R22h shape waiting for a caller whose UVs *do* leave the unit square.
/// `crate::render_test`'s sampler check holds every path to this, and it found
/// this one.
fn default_particle_image() -> Image {
    const SIZE: u32 = 32;
    let mut data: Vec<u8> = Vec::new();
    let half = f32::from(u16::try_from(SIZE).unwrap_or(0)) * 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            // The pixel centre offset from the sprite centre, normalised to the
            // half-width so the falloff reaches zero at the edge.
            let fx = (f32::from(u16::try_from(x).unwrap_or(0)) + 0.5 - half) / half;
            let fy = (f32::from(u16::try_from(y).unwrap_or(0)) + 0.5 - half) / half;
            let r = (fx * fx + fy * fy).sqrt();
            // A smooth radial falloff (1 at centre, 0 at radius 1), squared for a
            // softer edge.
            let a = (1.0 - r).clamp(0.0, 1.0);
            let alpha = a * a;
            data.push(255);
            data.push(255);
            data.push(255);
            data.push(float_to_u8(alpha * 255.0));
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        // sRGB so the white blob reads the same brightness as fetched sprites.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // See the doc comment: repeat, like every other texture path here.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The alpha mode a particle system's blend function implies: an additive
/// (destination `ONE`) blend is the glow / fire look; anything else is ordinary
/// source-over alpha. A whole system shares one blend function (it lives on the
/// system's particle *template*), so one material per cloud suffices.
const fn alpha_mode_for(system: &ParticleSystem) -> AlphaMode {
    if system.part_blend_func_dest == LL_PART_BF_ONE {
        AlphaMode::Add
    } else {
        AlphaMode::Blend
    }
}

/// Whether a cloud renders unlit (fullbright): an `EMISSIVE` system, one that
/// blends additively (the glow / fire look reads wrong when lit), or a **HUD**
/// cloud (P35.4 — no light is on the HUD layer, so a lit HUD material renders
/// black, and the reference forces `LLFace::FULLBRIGHT` on every HUD face). A whole
/// system shares its particle-template flags, so the choice is system-wide.
const fn is_unlit(system: &ParticleSystem, is_hud: bool) -> bool {
    is_hud
        || system.part_flags & part_flags::EMISSIVE != 0
        || system.part_blend_func_dest == LL_PART_BF_ONE
}

/// Build the material for a cloud from its system's blend function, with the
/// default sprite as its initial texture. Emissive / additive (and HUD) particles
/// are drawn unlit (fullbright, as the reference draws most particles); the material
/// is double-sided (the billboards face the camera either way).
fn particle_material(
    system: &ParticleSystem,
    is_hud: bool,
    default_image: Handle<Image>,
) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(default_image),
        alpha_mode: alpha_mode_for(system),
        unlit: is_unlit(system, is_hud),
        cull_mode: None,
        // Do not occlude other transparent geometry with particle depth.
        depth_bias: 0.0,
        ..default()
    }
}

/// Convert a Second Life [`Vector`](sl_client_bevy::Vector) into a Bevy [`Vec3`]
/// with its components carried across verbatim (no basis change) — for a vector
/// that is *rotated* into Bevy space separately, such as an emission direction
/// built in Second Life space.
const fn vector_to_vec3(vector: &sl_client_bevy::Vector) -> Vec3 {
    Vec3::new(vector.x, vector.y, vector.z)
}

/// Component-wise `a + b`. The workspace `arithmetic_side_effects` lint fires on
/// the glam `+` operator (not on the plain `f32` arithmetic inside), so the
/// particle math routes vector sums through these helpers.
fn v_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Component-wise `a - b` (see [`v_add`] for why this is a helper).
fn v_sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise `v * s` (see [`v_add`] for why this is a helper).
fn v_scale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// Linear interpolation between two `f32`s.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Convert a wire RGBA colour (bytes) to a normalised `0.0..=1.0` `[f32; 4]`.
fn color_to_f32(color: [u8; 4]) -> [f32; 4] {
    [
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        f32::from(color[3]) / 255.0,
    ]
}

/// Interpolate two wire RGBA colours by `t`, returning the normalised result — the
/// reference's full-RGBA particle colour lerp.
fn lerp_color(start: [u8; 4], end: [u8; 4], t: f32) -> [f32; 4] {
    let s = color_to_f32(start);
    let e = color_to_f32(end);
    [
        lerp(s[0], e[0], t),
        lerp(s[1], e[1], t),
        lerp(s[2], e[2], t),
        lerp(s[3], e[3], t),
    ]
}

/// Truncate a non-negative `f32` into a `u8`, saturating at the range ends — for
/// packing an alpha value into the procedural sprite.
///
/// `pub(crate)` because `crate::render_scene` packs its procedurally computed
/// sculpt map with it: the conversion is the same one, and a second copy would
/// be a second place to get the clamp wrong.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value pre-clamped to 0..=255; truncate-toward-zero for a colour byte"
)]
pub(crate) const fn float_to_u8(value: f32) -> u8 {
    value.clamp(0.0, 255.0) as u8
}

/// Build the dynamic billboard mesh for a cloud's live particles, each a
/// camera-facing quad — a port of `LLVOPartGroup::getGeometry`. Positions are in
/// absolute Bevy world space (the cloud entity has an identity transform).
fn build_cloud_mesh(particles: &[Particle], camera_pos: Vec3) -> Mesh {
    let count = particles.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(count.saturating_mul(4));
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(count.saturating_mul(4));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(count.saturating_mul(4));
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(count.saturating_mul(4));
    let mut indices: Vec<u32> = Vec::with_capacity(count.saturating_mul(6));
    let mut base: u32 = 0;

    for part in particles {
        let (v0, v1, v2, v3, normal) = billboard_quad(part, camera_pos);
        positions.push(v0.to_array());
        positions.push(v1.to_array());
        positions.push(v2.to_array());
        positions.push(v3.to_array());
        for _ in 0..4 {
            normals.push(normal.to_array());
            colors.push(part.color);
        }
        // The texcoords the reference assigns to the four billboard corners.
        uvs.push([0.0, 1.0]);
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 1.0]);
        uvs.push([1.0, 0.0]);
        // Two triangles over the quad (winding is irrelevant — the material is
        // double-sided).
        for offset in [0, 1, 2, 2, 1, 3] {
            indices.push(base.saturating_add(offset));
        }
        base = base.saturating_add(4);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The four world-space corners and the normal of one particle's camera-facing
/// billboard quad, a port of `LLVOPartGroup::getGeometry` (the non-ribbon path),
/// including the `FOLLOW_VELOCITY` re-orientation.
fn billboard_quad(part: &Particle, camera_pos: Vec3) -> (Vec3, Vec3, Vec3, Vec3, Vec3) {
    let at = v_sub(part.pos, camera_pos);
    // The camera-facing basis: right ⊥ (at, up), then up ⊥ (right, at).
    let up_world = Vec3::Y;
    let mut right = normalize_or(at.cross(up_world), Vec3::X);
    let mut up = normalize_or(right.cross(at), Vec3::Y);

    // Orient the billboard along the velocity when flagged (streak the sprite in
    // its direction of travel).
    if part.flags & part_flags::FOLLOW_VELOCITY != 0 && part.velocity != Vec3::ZERO {
        let nv = part.velocity.normalize_or_zero();
        let f0 = nv.dot(right);
        let f1 = nv.dot(up);
        let len = (f0 * f0 + f1 * f1).sqrt();
        if len > 1.0e-4 {
            let f0n = f0 / len;
            let f1n = f1 / len;
            let new_up = v_add(v_scale(right, f0n), v_scale(up, f1n));
            let new_right = v_sub(v_scale(right, f1n), v_scale(up, f0n));
            up = normalize_or(new_up, up);
            right = normalize_or(new_right, right);
        }
    }

    let half_right = v_scale(right, 0.5 * part.scale[0]);
    let half_up = v_scale(up, 0.5 * part.scale[1]);
    let pos_plus_up = v_add(part.pos, half_up);
    let pos_minus_up = v_sub(part.pos, half_up);
    let normal = normalize_or(v_scale(at, -1.0), Vec3::Z);
    (
        v_sub(pos_plus_up, half_right),
        v_sub(pos_minus_up, half_right),
        v_add(pos_plus_up, half_right),
        v_add(pos_minus_up, half_right),
        normal,
    )
}

/// Normalise `v`, or return `fallback` if it is too short to normalise stably.
fn normalize_or(v: Vec3, fallback: Vec3) -> Vec3 {
    if v.length_squared() > 1.0e-12 {
        v.normalize()
    } else {
        fallback
    }
}

/// Drive the particle simulation and render (P30.2): for every live particle
/// source, advance its emitter and particles this frame and rebuild its
/// camera-facing billboard mesh.
///
/// Sources gain a [`Cloud`] the first frame they appear and lose it (its cloud
/// entity despawned) when the [`ObjectParticleSystem`] component is removed — a
/// source toggled off in-world, or its object gone. The whole simulation is
/// bounded by [`MAX_PARTICLES`]; particles beyond the cap are simply not emitted.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's arguments are its resource/query dependencies"
)]
pub(crate) fn drive_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut sim: ResMut<ParticleSim>,
    sources: Query<(
        Entity,
        &ObjectParticleSystem,
        &GlobalTransform,
        Option<&RenderLayers>,
    )>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    hud_camera: Query<&GlobalTransform, (With<HudCamera>, Without<ViewerCamera>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut manager: ResMut<TextureManager>,
    default_image: Res<DefaultParticleImage>,
    // A throttle so the live-count diagnostic logs periodically, not every frame.
    mut log_timer: Local<f32>,
    // `SL_VIEWER_DISABLE_HUD_PARTICLES` (P35.4): the reference's `RenderHUDParticles`
    // flag, mirrored — read once and cached. Defaulted on (HUD particles emit).
    mut hud_disabled: Local<Option<bool>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let camera_pos = camera.translation();
    // The fixed HUD camera stands well back of the origin-anchored HUD screen and
    // looks along it; billboarding HUD particles at its position keeps them square to
    // the (orthographic) HUD view. Absent a HUD camera (a run without avatar assets
    // spawns no HUD screen), no source is a HUD source anyway, so the fallback is
    // never used.
    let hud_camera_pos = hud_camera
        .single()
        .map_or(camera_pos, GlobalTransform::translation);
    let hud_disabled = *hud_disabled
        .get_or_insert_with(|| std::env::var_os("SL_VIEWER_DISABLE_HUD_PARTICLES").is_some());
    let dt = time.delta_secs();

    // Snapshot each source's world pose, HUD classification, and system, releasing
    // the query borrow before the resource is mutated. The Second Life-space source
    // rotation is recovered from the Bevy world rotation by undoing the basis change.
    // A HUD source is suppressed entirely when HUD particles are disabled — it is
    // then dropped from the snapshot, so its cloud (if any) is retired below.
    let basis_inv = sl_to_bevy_rotation().inverse();
    let sources_data: Vec<(Entity, Vec3, Quat, ParticleSystem, bool)> = sources
        .iter()
        .filter_map(|(entity, ops, global, layers)| {
            let is_hud = on_hud_layer(layers);
            if is_hud && hud_disabled {
                return None;
            }
            Some((
                entity,
                global.translation(),
                basis_inv.mul_quat(global.rotation()),
                ops.system.clone(),
                is_hud,
            ))
        })
        .collect();
    let current: HashSet<Entity> = sources_data.iter().map(|(entity, ..)| *entity).collect();

    // Retire clouds whose source is gone (component removed or object despawned).
    sim.clouds.retain(|entity, cloud| {
        if current.contains(entity) {
            true
        } else {
            commands.entity(cloud.entity).try_despawn();
            false
        }
    });

    // A running live-particle count across all clouds, so the global cap is
    // respected as each source emits.
    let mut total: usize = 0;

    for (entity, src, q_sl, system, is_hud) in &sources_data {
        let is_hud = *is_hud;
        // Ensure a cloud exists for this source, spawning its render entity on first
        // sight. A HUD source's entity goes on the HUD render layer, so the HUD camera
        // draws it (and the fly camera does not); a world source's stays on the
        // default layer.
        let cloud = sim.clouds.entry(*entity).or_insert_with(|| {
            let mesh = meshes.add(Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            ));
            let material =
                materials.add(particle_material(system, is_hud, default_image.0.clone()));
            let mut cloud_commands = commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                // Starts hidden: the mesh is empty until the first particles are
                // built into it, and an empty mesh must not be rendered / uploaded
                // (R26). Flipped to visible once it has geometry.
                Visibility::Hidden,
                NotShadowCaster,
                // The billboard mesh is rebuilt in place every frame, so its
                // `Aabb` (computed once when `Mesh3d` was added, from the then
                // empty mesh) never covers the live particles — leaving it
                // frustum-culled from every viewpoint. Particles are their own
                // dynamic geometry, so opt out of frustum culling entirely (the
                // way `objects.rs` does for its rebuilt meshes).
                NoFrustumCulling,
                // Named so a diagnostic — or `crate::render_test`'s checks —
                // can say "the particle cloud" rather than an entity id the
                // reader has no way to resolve. The cloud is spawned here rather
                // than by whatever created the source, so this is the only place
                // that knows what it is.
                Name::new("particle-cloud"),
            ));
            if is_hud {
                cloud_commands.insert(RenderLayers::layer(HUD_RENDER_LAYER));
            }
            let cloud_entity = cloud_commands.id();
            Cloud {
                emitter: Emitter::new(entity.to_bits()),
                particles: Vec::new(),
                entity: cloud_entity,
                mesh,
                material,
                system: system.clone(),
                texture_applied: false,
                visible: false,
                is_hud,
            }
        });

        // The HUD classification can arrive a frame after the cloud is spawned (the
        // source's HUD layer propagates down in `PostUpdate`): move the cloud entity
        // onto (or off) the HUD layer and re-light its material when it flips.
        if cloud.is_hud != is_hud {
            cloud.is_hud = is_hud;
            if is_hud {
                commands
                    .entity(cloud.entity)
                    .insert(RenderLayers::layer(HUD_RENDER_LAYER));
            } else {
                commands.entity(cloud.entity).remove::<RenderLayers>();
            }
            if let Some(mut material) = materials.get_mut(&cloud.material) {
                material.unlit = is_unlit(system, is_hud);
            }
        }

        // A re-tuned source (a fresh `llParticleSystem`) restarts the emitter and
        // re-derives the material and texture.
        if cloud.system != *system {
            cloud.system = system.clone();
            cloud.emitter = Emitter::new(entity.to_bits());
            cloud.particles.clear();
            cloud.texture_applied = false;
            if let Some(mut material) = materials.get_mut(&cloud.material) {
                material.alpha_mode = alpha_mode_for(system);
                material.unlit = is_unlit(system, is_hud);
                material.base_color_texture = Some(default_image.0.clone());
            }
        }

        // Advance the existing particles (dropping the dead), then emit this
        // frame's bursts. A `TARGET_*` source has no resolved target object here,
        // so it falls back to its own position (the reference's own fallback).
        cloud
            .particles
            .retain_mut(|part| part.integrate(dt, *src, *src));
        total = total.saturating_add(cloud.particles.len());
        cloud.emitter.emit(
            system,
            dt,
            *src,
            *q_sl,
            &mut cloud.particles,
            &mut total,
            MAX_PARTICLES,
        );

        // Resolve the source's diffuse texture through the shared pipeline (or keep
        // the default sprite), applying it once it decodes.
        apply_cloud_texture(
            cloud,
            system,
            &mut manager,
            &mut images,
            &mut materials,
            &default_image,
        );

        // Rebuild the billboard mesh from the current particles — but only when the
        // cloud has any. Re-inserting an empty (zero-vertex) mesh makes bevy's mesh
        // allocator log a "use-after-free: unallocated key" error every frame (it
        // skips allocating a zero-size vertex buffer but still tries to copy into it,
        // R26), so an idle / zero-particle cloud instead leaves its last mesh
        // untouched and hides the entity. Visibility is only rewritten on a change.
        let want_visible = !cloud.particles.is_empty();
        if want_visible {
            // A HUD cloud faces the fixed HUD camera (so its quads stay square to the
            // orthographic HUD view); a world cloud faces the fly camera.
            let billboard_from = if is_hud { hud_camera_pos } else { camera_pos };
            let mesh = build_cloud_mesh(&cloud.particles, billboard_from);
            let _replaced = meshes.insert(&cloud.mesh, mesh);
        }
        if cloud.visible != want_visible {
            cloud.visible = want_visible;
            let visibility = if want_visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            commands.entity(cloud.entity).insert(visibility);
        }
    }

    // Periodic live-count diagnostic: how many sources and particles the sim holds.
    *log_timer += dt;
    if *log_timer >= 2.0 {
        *log_timer = 0.0;
        debug!(
            "particle sim: {} source cloud(s), {total} live particle(s)",
            sim.clouds.len(),
        );
    }
}

/// Resolve a cloud's diffuse texture: request the source's named texture through
/// the shared pipeline and drop it onto the material once it decodes, or keep the
/// procedural default sprite when the source names none.
fn apply_cloud_texture(
    cloud: &mut Cloud,
    system: &ParticleSystem,
    manager: &mut TextureManager,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    default_image: &DefaultParticleImage,
) {
    if cloud.texture_applied {
        return;
    }
    match system.texture_id {
        Some(texture_id) => {
            // Boosted like an avatar texture so a visible emitter's sprite loads
            // promptly rather than queued behind nearer prims.
            manager.request_boosted(texture_id, AVATAR_BOOST_PRIORITY);
            if let Some(decoded) = manager.decoded(texture_id) {
                let handle = images.add(build_particle_image(decoded));
                if let Some(mut material) = materials.get_mut(&cloud.material) {
                    material.base_color_texture = Some(handle);
                }
                cloud.texture_applied = true;
            }
        }
        None => {
            // No named texture: the default sprite (set at material creation) is
            // final.
            if let Some(mut material) = materials.get_mut(&cloud.material) {
                material.base_color_texture = Some(default_image.0.clone());
            }
            cloud.texture_applied = true;
        }
    }
}

/// Build the Bevy [`Image`] for a decoded particle sprite. The reference viewer
/// samples particle textures clamped (`TAM_CLAMP`), which is Bevy's default
/// address mode, so `to_bevy_image`'s sampler is used as-is.
fn build_particle_image(decoded: &DecodedTexture) -> Image {
    to_bevy_image(decoded)
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

    // -----------------------------------------------------------------------
    // P30.2 simulation tests.
    // -----------------------------------------------------------------------

    use super::{Emitter, Particle, Rng, alpha_mode_for, build_cloud_mesh, is_unlit, part_flags};
    use bevy::math::{Quat, Vec3};
    use bevy::mesh::{Mesh, PrimitiveTopology, VertexAttributeValues};
    use bevy::prelude::AlphaMode;

    /// The seed the emitter tests use, so the pseudo-random sequence is fixed.
    const TEST_SEED: u64 = 0x1234_5678;

    /// The RNG is deterministic (same seed → same stream) and stays in `[0, 1)`.
    #[test]
    fn rng_is_deterministic_and_bounded() {
        let mut a = Rng::new(TEST_SEED);
        let mut b = Rng::new(TEST_SEED);
        for _ in 0..1000 {
            let x = a.frand();
            assert!((0.0..1.0).contains(&x), "frand {x} out of range");
            assert_eq!(x.to_bits(), b.frand().to_bits());
        }
    }

    /// The RNG's unit vector is (near) unit length.
    #[test]
    fn rng_unit_vector_is_normalised() {
        let mut rng = Rng::new(TEST_SEED);
        for _ in 0..100 {
            let v = rng.unit_vector();
            assert!((v.length() - 1.0).abs() < 1.0e-4, "|{v:?}| != 1");
        }
    }

    /// A source emits its first burst immediately (the reference's `first_run`) and
    /// then one burst per `burst_rate` of elapsed time.
    #[test]
    fn emitter_bursts_on_schedule() {
        let system = live_system(); // EXPLODE, burst_rate 0.1, burst_part_count 4.
        let mut emitter = Emitter::new(TEST_SEED);
        let mut particles: Vec<Particle> = Vec::new();
        let mut total = 0_usize;
        // A tiny first step still fires the immediate first burst → 4 particles.
        emitter.emit(
            &system,
            0.01,
            Vec3::ZERO,
            Quat::IDENTITY,
            &mut particles,
            &mut total,
            4096,
        );
        assert_eq!(particles.len(), 4);
        assert_eq!(total, 4);
        // A step covering three more burst intervals fires three more bursts.
        emitter.emit(
            &system,
            0.3,
            Vec3::ZERO,
            Quat::IDENTITY,
            &mut particles,
            &mut total,
            4096,
        );
        assert_eq!(particles.len(), 16);
    }

    /// Emission stops at the global cap, never overshooting it.
    #[test]
    fn emitter_respects_the_cap() {
        let mut system = live_system();
        system.burst_part_count = 10;
        let mut emitter = Emitter::new(TEST_SEED);
        let mut particles: Vec<Particle> = Vec::new();
        let mut total = 4090_usize; // 6 slots left under a 4096 cap.
        emitter.emit(
            &system,
            0.01,
            Vec3::ZERO,
            Quat::IDENTITY,
            &mut particles,
            &mut total,
            4096,
        );
        assert_eq!(total, 4096);
        assert_eq!(particles.len(), 6);
    }

    /// A source with a finite `max_age` stops emitting once it outlives it.
    #[test]
    fn emitter_dies_at_max_age() {
        let mut system = live_system();
        system.max_age = 0.5;
        let mut emitter = Emitter::new(TEST_SEED);
        let mut particles: Vec<Particle> = Vec::new();
        let mut total = 0_usize;
        emitter.emit(
            &system,
            0.1,
            Vec3::ZERO,
            Quat::IDENTITY,
            &mut particles,
            &mut total,
            4096,
        );
        let after_first = particles.len();
        assert!(after_first > 0);
        // Step well past the source's max age: no further particles, ever.
        emitter.emit(
            &system,
            1.0,
            Vec3::ZERO,
            Quat::IDENTITY,
            &mut particles,
            &mut total,
            4096,
        );
        emitter.emit(
            &system,
            1.0,
            Vec3::ZERO,
            Quat::IDENTITY,
            &mut particles,
            &mut total,
            4096,
        );
        assert_eq!(particles.len(), after_first);
    }

    /// The `DROP` pattern launches at the source with no velocity.
    #[test]
    fn drop_launches_at_source_still() {
        let mut system = live_system();
        system.pattern = sl_client_bevy::particle_pattern::DROP;
        let mut emitter = Emitter::new(TEST_SEED);
        let src = Vec3::new(5.0, 6.0, 7.0);
        let (pos, vel) = emitter.launch(&system, src, Quat::IDENTITY);
        assert!(pos.abs_diff_eq(src, 1.0e-6));
        assert!(vel.abs_diff_eq(Vec3::ZERO, 1.0e-6));
    }

    /// The `EXPLODE` pattern launches within the burst radius at the burst speed.
    #[test]
    fn explode_launches_within_radius_at_speed() {
        let system = live_system(); // EXPLODE, radius 1, speed exactly 1.
        let mut emitter = Emitter::new(TEST_SEED);
        for _ in 0..50 {
            let (pos, vel) = emitter.launch(&system, Vec3::ZERO, Quat::IDENTITY);
            assert!((pos.length() - system.burst_radius).abs() < 1.0e-3);
            assert!((vel.length() - 1.0).abs() < 1.0e-3);
        }
    }

    /// A straight-up `ANGLE` cone (zero angles, "new angle") launches along Bevy
    /// `+Y` — Second Life `+Z` (up) carried through the basis change.
    #[test]
    fn angle_zero_launches_up() {
        let mut system = live_system();
        system.pattern = sl_client_bevy::particle_pattern::ANGLE;
        system.flags = super::LL_PART_USE_NEW_ANGLE;
        system.inner_angle = 0.0;
        system.outer_angle = 0.0;
        system.burst_speed_min = 2.0;
        system.burst_speed_max = 2.0;
        let mut emitter = Emitter::new(TEST_SEED);
        let (_pos, vel) = emitter.launch(&system, Vec3::ZERO, Quat::IDENTITY);
        assert!(
            vel.abs_diff_eq(Vec3::new(0.0, 2.0, 0.0), 1.0e-4),
            "vel {vel:?}"
        );
    }

    /// A default (flag-less) particle just drifts under its velocity and ages out.
    #[test]
    fn particle_moves_and_dies() {
        let mut part = Particle {
            pos: Vec3::ZERO,
            velocity: Vec3::new(1.0, 0.0, 0.0),
            accel: Vec3::ZERO,
            age: 0.0,
            max_age: 1.0,
            flags: 0,
            start_color: [255; 4],
            end_color: [255; 4],
            start_scale: [1.0, 1.0],
            end_scale: [1.0, 1.0],
            color: [1.0; 4],
            scale: [1.0, 1.0],
            pos_offset: Vec3::ZERO,
        };
        assert!(part.integrate(0.5, Vec3::ZERO, Vec3::ZERO));
        assert!(part.pos.abs_diff_eq(Vec3::new(0.5, 0.0, 0.0), 1.0e-6));
        // Past its max age it dies.
        assert!(!part.integrate(0.6, Vec3::ZERO, Vec3::ZERO));
    }

    /// A `BOUNCE` particle reflects off the source's up-plane.
    #[test]
    fn particle_bounces_off_source_plane() {
        let mut part = Particle {
            pos: Vec3::new(0.0, 1.0, 0.0),
            velocity: Vec3::new(0.0, -10.0, 0.0),
            accel: Vec3::ZERO,
            age: 0.0,
            max_age: 100.0,
            flags: part_flags::BOUNCE,
            start_color: [255; 4],
            end_color: [255; 4],
            start_scale: [1.0, 1.0],
            end_scale: [1.0, 1.0],
            color: [1.0; 4],
            scale: [1.0, 1.0],
            pos_offset: Vec3::ZERO,
        };
        // A big step drives it below the source plane (y=0); it should reflect up.
        assert!(part.integrate(1.0, Vec3::ZERO, Vec3::ZERO));
        assert!(
            part.pos.y >= 0.0,
            "did not bounce back above the plane: {:?}",
            part.pos
        );
        assert!(part.velocity.y > 0.0, "velocity not reflected upward");
    }

    /// Colour interpolates from start to end over the life when flagged.
    #[test]
    fn colour_interpolates_when_flagged() {
        let mut part = Particle {
            pos: Vec3::ZERO,
            velocity: Vec3::ZERO,
            accel: Vec3::ZERO,
            age: 0.0,
            max_age: 1.0,
            flags: part_flags::INTERP_COLOR,
            start_color: [0, 0, 0, 255],
            end_color: [255, 255, 255, 255],
            start_scale: [1.0, 1.0],
            end_scale: [1.0, 1.0],
            color: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0],
            pos_offset: Vec3::ZERO,
        };
        // Half a unit-life step → half-grey.
        assert!(part.integrate(0.5, Vec3::ZERO, Vec3::ZERO));
        for (channel, value) in part.color.iter().take(3).enumerate() {
            assert!((value - 0.5).abs() < 1.0e-3, "channel {channel} = {value}");
        }
    }

    /// The billboard mesh has four vertices and six indices per particle, all
    /// attributes present.
    #[test]
    fn mesh_has_a_quad_per_particle() {
        let part = Particle {
            pos: Vec3::new(0.0, 0.0, 10.0),
            velocity: Vec3::ZERO,
            accel: Vec3::ZERO,
            age: 0.0,
            max_age: 1.0,
            flags: 0,
            start_color: [255; 4],
            end_color: [255; 4],
            start_scale: [2.0, 2.0],
            end_scale: [2.0, 2.0],
            color: [1.0, 1.0, 1.0, 1.0],
            scale: [2.0, 2.0],
            pos_offset: Vec3::ZERO,
        };
        let particles = vec![part.clone(), part];
        let mesh = build_cloud_mesh(&particles, Vec3::ZERO);
        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::TriangleList);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("mesh has float3 positions")
        };
        assert_eq!(positions.len(), 8);
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(12));
        // The billboard faces the camera: with the camera at the origin looking down
        // +Z at a particle on +Z, the quad spans the X/Y plane (all four corners at
        // the particle's Z).
        for pos in positions {
            assert!(
                (pos[2] - 10.0).abs() < 1.0e-4,
                "corner not camera-facing: {pos:?}"
            );
        }
    }

    /// Additive-blend (destination `ONE`) systems are drawn unlit and additive; an
    /// ordinary alpha system blends and (absent the emissive flag) is lit.
    #[test]
    fn blend_mode_follows_the_blend_function() {
        let mut additive = live_system();
        additive.part_blend_func_dest = super::LL_PART_BF_ONE;
        assert_eq!(alpha_mode_for(&additive), AlphaMode::Add);
        assert!(is_unlit(&additive, false));

        let alpha = live_system(); // dest 9 (one-minus-src-alpha).
        assert_eq!(alpha_mode_for(&alpha), AlphaMode::Blend);
        assert!(!is_unlit(&alpha, false));
    }

    /// A HUD cloud is always drawn unlit — no light is on the HUD render layer, so a
    /// lit HUD material would render black — regardless of its blend function.
    #[test]
    fn hud_clouds_are_always_unlit() {
        let alpha = live_system(); // an ordinary alpha system: lit in the world…
        assert!(!is_unlit(&alpha, false));
        assert!(is_unlit(&alpha, true), "…but unlit on the HUD");
    }
}
