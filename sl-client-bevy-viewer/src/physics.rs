//! The client-side physics foundation (P31.1): an [`avian3d`] physics world
//! bridged into the viewer's Bevy Y-up scene.
//!
//! This module stands up a shared physics substrate that later phases build on
//! — Phase 32 (flexi prims) and Phase 34 (avatar cloth / body physics) drive
//! their client-side simulations through it rather than hand-rolling a solver.
//! P31.1 is foundation only: the physics world runs (empty until P31.2 gives
//! server-flagged prims rigid bodies), configured with three things:
//!
//! - **Second Life gravity.** Second Life's world gravity is `-9.8` m/s² along
//!   its Z (up) axis (Firestorm `llmath.h` `GRAVITY = -9.8`, matched by
//!   OpenSim's `world_gravityz = -9.8`). The single Second Life → Bevy basis
//!   change (see [`crate::coords`]) carries that Z-up vector into Bevy's Y-up
//!   frame, so avian's [`Gravity`] resource points along `-Y`.
//! - **A fixed timestep.** avian runs its schedule in `FixedPostUpdate`, driven
//!   by Bevy's `Time<Fixed>` clock; pinning that clock to the Second Life
//!   simulator's target physics rate ([`SL_PHYSICS_HZ`], 45 Hz) makes the
//!   client's physics advance at the sim's cadence.
//! - **Time dilation.** A laden region does not keep up with 45 Hz; it reports
//!   the fraction of real time its physics frame is achieving in the
//!   `RegionData.TimeDilation` field of every object-update message (surfaced as
//!   [`SlSessionEvent::TimeDilation`]). avian's *relative speed* is exactly a
//!   time-dilation control, so [`drive_physics_time_dilation`] scales the
//!   physics clock by the agent region's dilation each frame — client-side
//!   dynamics then slow down in lock-step with the dilated sim instead of
//!   drifting ahead of it.
//!
//! No `sl-client-tokio` counterpart is needed: like the render materials and the
//! other viewer-only simulations (sky, water, particles), the physics world is a
//! viewer rendering concern, not a protocol capability, so the runtime-parity
//! rule does not apply.

use std::collections::HashMap;

use avian3d::prelude::{Gravity, Physics, PhysicsPlugins, PhysicsTime as _};
use bevy::prelude::*;
use sl_client_bevy::{RegionHandle, SlEvent, SlIdentity, SlSessionEvent, Vector};

use crate::coords::sl_to_bevy_vec;

/// Second Life's world gravity along its Z (up) axis, in metres per second
/// squared (Firestorm `llmath.h` `GRAVITY = -9.8`; OpenSim `world_gravityz =
/// -9.8`).
const SL_GRAVITY_Z: f32 = -9.8;

/// The Second Life simulator's target physics frame rate, in hertz. The sim
/// runs its physics at 45 frames per second when fully keeping up; the client's
/// physics world uses the same fixed step so its client-side dynamics advance at
/// the sim's cadence. The actual achieved rate is scaled down by the region's
/// time dilation (see [`drive_physics_time_dilation`]).
const SL_PHYSICS_HZ: f64 = 45.0;

/// The world gravity avian uses, as a Bevy Y-up vector: Second Life's `-9.8`
/// m/s² Z-up gravity carried through the single Second Life → Bevy basis change,
/// landing along Bevy `-Y`.
#[must_use]
fn sl_gravity() -> Vec3 {
    sl_to_bevy_vec(&Vector {
        x: 0.0,
        y: 0.0,
        z: SL_GRAVITY_Z,
    })
}

/// Clamp a raw region time dilation into avian's *relative speed* contract.
///
/// `set_relative_speed` panics on a negative or non-finite ratio; the wire value
/// is already `0.0..=1.0`, but guard a non-finite value (falling back to a
/// healthy `1.0`) and clamp into range so a malformed update can never poison the
/// physics clock.
#[must_use]
const fn clamp_dilation(dilation: f32) -> f32 {
    if dilation.is_finite() {
        dilation.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// The most recent `RegionData.TimeDilation` seen per region, folded from the
/// session event stream by [`ingest_time_dilation`] and read (for the agent's
/// current region) by [`drive_physics_time_dilation`].
#[derive(Resource, Default)]
pub(crate) struct RegionTimeDilation {
    /// The latest dilation (`0.0..=1.0`) for each region, keyed by handle.
    per_region: HashMap<RegionHandle, f32>,
}

/// The viewer's physics plugin: adds the avian physics world, sets Second Life
/// gravity + the fixed timestep, and wires the time-dilation drive.
pub(crate) struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            // Second Life gravity, bridged into Bevy's Y-up frame.
            .insert_resource(Gravity(sl_gravity()))
            // Pin the fixed clock that drives avian's `FixedPostUpdate` schedule
            // to the Second Life simulator's target physics rate.
            .insert_resource(Time::<Fixed>::from_hz(SL_PHYSICS_HZ))
            .init_resource::<RegionTimeDilation>()
            .add_systems(
                Update,
                (ingest_time_dilation, drive_physics_time_dilation).chain(),
            );
    }
}

/// Fold each region's most recent `TimeDilation` into [`RegionTimeDilation`].
pub(crate) fn ingest_time_dilation(
    mut events: MessageReader<SlEvent>,
    mut dilations: ResMut<RegionTimeDilation>,
) {
    for event in events.read() {
        if let SlSessionEvent::TimeDilation {
            region_handle,
            dilation,
        } = &event.0
        {
            dilations.per_region.insert(*region_handle, *dilation);
        }
    }
}

/// Scale the physics clock by the agent's current-region time dilation each
/// frame, so client-side physics runs at the same effective rate as the (laden)
/// simulator. Defaults to full speed (`1.0`) while the region is unknown or has
/// not yet reported a dilation.
pub(crate) fn drive_physics_time_dilation(
    identity: Res<SlIdentity>,
    dilations: Res<RegionTimeDilation>,
    mut time: ResMut<Time<Physics>>,
) {
    let dilation = identity
        .region_handle
        .and_then(|handle| dilations.per_region.get(&handle).copied())
        .unwrap_or(1.0);
    let ratio = clamp_dilation(dilation);
    // Only touch the clock when the ratio actually changes, so a steady region
    // does no per-frame resource mutation.
    if (time.relative_speed() - ratio).abs() > f32::EPSILON {
        time.set_relative_speed(ratio);
    }
}

#[cfg(test)]
mod tests {
    use super::{SL_GRAVITY_Z, clamp_dilation, sl_gravity};
    use bevy::math::Vec3;

    /// Assert two `f32` are equal within a tight tolerance (the workspace lints
    /// forbid a strict `float_cmp`, and the clamp results are exact anyway).
    fn approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "{actual} should equal {expected}"
        );
    }

    /// Second Life's Z-up gravity maps to a Bevy `-Y` vector of the same
    /// magnitude — the physics world falls "down" in the rendered scene.
    #[test]
    fn gravity_maps_z_up_to_bevy_down() {
        // Straight down in Bevy's Y-up frame at 9.8 m/s².
        assert!(sl_gravity().abs_diff_eq(Vec3::new(0.0, SL_GRAVITY_Z, 0.0), 1.0e-6));
        assert!(sl_gravity().abs_diff_eq(Vec3::new(0.0, -9.8, 0.0), 1.0e-6));
    }

    /// A healthy region (dilation `1.0`) runs physics at full speed; a laden one
    /// scales it down; the endpoints of the wire domain pass through unchanged.
    #[test]
    fn dilation_clamps_into_the_relative_speed_domain() {
        approx(clamp_dilation(1.0), 1.0);
        approx(clamp_dilation(0.5), 0.5);
        approx(clamp_dilation(0.0), 0.0);
    }

    /// An out-of-range or non-finite dilation can never poison the physics clock
    /// (avian's `set_relative_speed` would panic on a negative / non-finite
    /// ratio): it is clamped into range, and a `NaN` falls back to full speed.
    #[test]
    fn dilation_guards_against_bad_values() {
        approx(clamp_dilation(-0.5), 0.0);
        approx(clamp_dilation(2.0), 1.0);
        approx(clamp_dilation(f32::NAN), 1.0);
        approx(clamp_dilation(f32::INFINITY), 1.0);
    }
}
