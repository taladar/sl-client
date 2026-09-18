//! The scene layer's schedule: [`WorldScenePlugin`].
//!
//! The ground the objects stand on and the region environment they are lit by:
//! the terrain fold, the EEP environment fold and the settings-asset fetch that
//! resolves what either of them names. Registered here, in the crate that owns
//! them, rather than in the binary that composes it (the roadmap task
//! `viewer-audit-plugins-own-their-schedule`).
//!
//! # What is **not** here
//!
//! Everything in this crate that needs a render app — the sky dome and its
//! discs, clouds and stars, the ocean and its exclusion mask and scene-depth
//! copy, the particle simulation, the local-light budget, the parcel-border
//! bands, the beacons, and the four post-processes (underwater fog, exposure,
//! tone map, glow) — is its own plugin already, and each schedules itself
//! against [`WorldPhase::CameraPositioned`]. They stay separate because a
//! CPU-only harness takes this fold without any of them.

use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;
use sl_viewer_world_api::TerrainState;
use sl_viewer_world_api::world_scoped::{WorldResetSystems, WorldScopedAppExt as _};
use sl_viewer_world_objects::objects::{apply_object_meshes, update_objects};

use crate::environment::{EnvironmentState, ingest_environment, request_environment};
use crate::terrain::{
    PendingPatchRebuilds, TerrainTextures, drain_patch_rebuilds, recenter_terrain, update_terrain,
};

/// The scene layer's fold: terrain, the region environment, and the settings
/// assets either of them resolves.
///
/// Add it to any app that wants a region's ground and sky state, whether or not
/// that app draws them.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorldScenePlugin;

impl Plugin for WorldScenePlugin {
    fn build(&self, app: &mut App) {
        // The terrain stores are keyed by region handle, so a **distant**
        // teleport throws them away; the environment is a single current value
        // and simply gets overwritten by the destination's.
        app.init_world_scoped::<TerrainState>();
        app.init_world_scoped::<TerrainTextures>();
        app.init_resource::<PendingPatchRebuilds>();
        // The viewer seeds this from its `--sky-day-position` style render
        // overrides before the plugin is added, so this is the harness's
        // default rather than the viewer's.
        app.init_resource::<EnvironmentState>();
        app.init_resource::<crate::environment::LocalEnvironmentPick>();
        // What the RLV `@setenv_*` family has queued, read by
        // `apply_rlv_environment` below. The RLV engine owns it and fills it,
        // but it sits far above the scene and a world without it still runs
        // this fold — so the reader declares the read. `init_resource` is
        // idempotent, so the engine still owns it where both are present.
        app.init_resource::<sl_viewer_world_api::rlv::RlvEnvironmentSlot>();
        app.init_resource::<sl_viewer_platform::environment_assets::EnvironmentAssetManager>();
        // A distant teleport replaced the world: each terrain store empties
        // itself in `WorldResetSystems::Purge`, and that has to happen before
        // the re-centring pass — the purge drops the layer's origin anchor, so
        // re-centring afterwards anchors on the destination instead of shifting
        // the (already purged) scene by a delta from the region we left.
        app.configure_sets(Update, WorldResetSystems::Purge.before(recenter_terrain));
        app.add_systems(
            Update,
            (
                environment_fold_pipeline(),
                environment_asset_pipeline(),
                terrain_fold_pipeline(),
            ),
        );
    }
}

/// The terrain fold: re-base the ground onto the current origin, fold this
/// frame's patch events in, then drain a few of the queued seam / whole-region
/// patch rebuilds ([`PendingPatchRebuilds`]).
///
/// Chained because each stage reads what the one before it wrote: recentring
/// (the origin follows the root region) has to precede folding terrain events,
/// so patches are placed on the current origin.
///
/// Terrain **wins** the shared per-frame `MeshUploadBudget`: ordered before the
/// object layer's mesh / sculpt spenders (`update_objects`' inline warm-cache
/// builds, `apply_object_meshes` and its chained `apply_object_sculpts` /
/// `apply_rigged_attachments`) so a region hand-off builds the ground first — a
/// missing ground plane is far more visible than a few deferred prims, and
/// terrain is a small, bursty set (a region's 16×16 patches) that at most defers
/// objects for a few frames per region connect.
fn terrain_fold_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (recenter_terrain, update_terrain, drain_patch_rebuilds)
        .chain()
        .before(update_objects)
        .before(apply_object_meshes)
}

/// The region-environment (EEP) fold: mirror the settings, restore what this
/// account saved, request the region's and the agent's parcel's environment,
/// fold the replies in, layer an experience's `llSetEnvironment` push over
/// them, resolve the pinned / picked / RLV-queued settings assets, and finally
/// advance the cross-fade and persist what it settled on.
///
/// Chained because every one of those steps reads what the previous one wrote
/// into [`EnvironmentState`]: the parcel track has to precede the request that
/// names the parcel, the experience push has to land on top of the region reply
/// rather than under it, and the RLV apply and the persist both have to see the
/// environment this frame settled on. Unordered, an experience push could sit
/// *under* the region reply that arrived in the same frame, and the saved
/// personal environment could be a frame stale.
fn environment_fold_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        // Mirror the manual transition time into the state, and bring back the
        // personal environment this account saved, before anything can change
        // either.
        crate::environment::sync_environment_settings,
        crate::environment::restore_saved_environment,
        request_environment,
        // The parcel the agent stands on, and its own environment (the
        // reference's ENV_PARCEL). Before the ingest, so a reply that lands this
        // frame is matched against the parcel the agent is on now.
        crate::environment::track_agent_parcel,
        crate::environment::request_parcel_environment,
        ingest_environment,
        // An experience's `llSetEnvironment` push, into the layer above the
        // region's. After the ingest, so a region reply landing this frame is
        // underneath the push rather than over it.
        crate::environment::ingest_experience_environment_push,
        // An experience is admitted per land: ask the region which of the
        // injecting ones the parcel just stepped onto still allows, and release
        // the ones it does not. After the push ingest, so a push landing this
        // frame is in the set the query names.
        crate::environment::query_parcel_experiences,
        crate::environment::ingest_parcel_experiences,
        // Fetch + swap in a pinned Modern (`KNOWN_SKY_*`) sky once its asset
        // decodes; after `ingest_environment` so the shared environment (the
        // Modern placeholder) is current.
        crate::environment::resolve_modern_environment,
        // Install the settings asset a panel (quick preferences' sky / water /
        // day-cycle combos) has picked, once its asset decodes.
        crate::environment::resolve_local_environment_pick,
        // Install whatever the RLV `@setenv_*` family has queued and republish
        // the rendered sky for the next `@getenv_*` read. Last of the four so
        // the sky it publishes is the one this frame settled on.
        crate::environment::apply_rlv_environment,
        // Advance whichever manual cross-fade is running, and save the personal
        // environment whenever it settles on something new. Last, so both see
        // the frame's final environment.
        crate::environment::advance_environment_transition,
        crate::environment::persist_saved_environment,
    )
        .chain()
}

/// The EEP settings-asset fetch for the World ▸ Environment Modern presets:
/// keep the `ViewerAsset` capability URL current, then poll the fetches it
/// un-parked. Chained for the same reason as the stores in the object layer: a
/// cap refresh is what un-parks the requests the poll then finishes.
fn environment_asset_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        sl_viewer_platform::environment_assets::update_environment_asset_caps,
        sl_viewer_platform::environment_assets::poll_environment_assets,
    )
        .chain()
}

#[cfg(test)]
mod tests {
    use super::{environment_asset_pipeline, environment_fold_pipeline, terrain_fold_pipeline};

    use sl_viewer_world_api::schedule_order::ScheduleOrder;
    use sl_viewer_world_api::stage;
    use sl_viewer_world_objects::objects::{apply_object_meshes, update_objects};

    use crate::environment::{ingest_environment, request_environment};
    use crate::terrain::{drain_patch_rebuilds, recenter_terrain, update_terrain};

    /// The scene layer's fold, reduced to what an ordering assertion needs.
    ///
    /// The object-layer systems the terrain fold orders itself ahead of are
    /// registered too: an edge naming a system that is not in the schedule
    /// constrains nothing.
    fn fold() -> ScheduleOrder {
        ScheduleOrder::of((
            environment_fold_pipeline(),
            environment_asset_pipeline(),
            terrain_fold_pipeline(),
            (update_objects, apply_object_meshes),
        ))
    }

    /// The environment fold settles in one frame: the parcel is tracked before
    /// the request that names it, an experience's push lands on top of the
    /// region reply rather than under it, and the cross-fade and the persist
    /// both see what the frame settled on.
    #[test]
    fn environment_fold_runs_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(crate::environment::sync_environment_settings),
            stage!(crate::environment::restore_saved_environment),
            stage!(request_environment),
            stage!(crate::environment::track_agent_parcel),
            stage!(crate::environment::request_parcel_environment),
            stage!(ingest_environment),
            stage!(crate::environment::ingest_experience_environment_push),
            stage!(crate::environment::query_parcel_experiences),
            stage!(crate::environment::ingest_parcel_experiences),
            stage!(crate::environment::resolve_modern_environment),
            stage!(crate::environment::resolve_local_environment_pick),
            stage!(crate::environment::apply_rlv_environment),
            stage!(crate::environment::advance_environment_transition),
            stage!(crate::environment::persist_saved_environment),
        ]);
    }

    /// The settings-asset store polls against the cap it just refreshed.
    #[test]
    fn environment_assets_run_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(sl_viewer_platform::environment_assets::update_environment_asset_caps),
            stage!(sl_viewer_platform::environment_assets::poll_environment_assets),
        ]);
    }

    /// Terrain re-bases before it folds, drains its rebuild queue after, and
    /// takes the frame's mesh-upload budget ahead of the object layer.
    #[test]
    fn terrain_folds_before_the_objects_spend_the_budget() {
        let fold = fold();
        fold.assert_pipeline(&[
            stage!(recenter_terrain),
            stage!(update_terrain),
            stage!(drain_patch_rebuilds),
        ]);
        fold.assert_pipeline(&[stage!(drain_patch_rebuilds), stage!(update_objects)]);
        fold.assert_pipeline(&[stage!(drain_patch_rebuilds), stage!(apply_object_meshes)]);
    }
}
