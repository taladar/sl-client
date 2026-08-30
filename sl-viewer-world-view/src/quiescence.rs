//! **Scene quiescence**: whether everything the scene asked for has arrived.
//!
//! A frame captured while a mesh is still at a coarse LOD, a texture still
//! undecoded, a bake not yet assembled or an avatar not yet spawned differs from
//! the same frame a second later for reasons that have nothing to do with the
//! code under test. Every asset store already knows its own in-flight count;
//! [`SceneQuiescence`] sums them, plus the deferred queues the stores cannot
//! see, so a capture — the screenshot mode's, the full-stack harness's — can
//! wait for *quiet* rather than for a delay long enough and hope.
//!
//! Quiet also requires the region to be up: an app that has not logged in has
//! nothing in flight, and that is not the quiet anyone means.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use sl_client_bevy::{GateStats, SlCurrentRegion, StoreStats};
use sl_viewer_platform::environment_assets::EnvironmentAssetManager;
use sl_viewer_world_avatar::animations::AnimationManager;
use sl_viewer_world_avatar::bake_inputs::WearableAssetManager;
use sl_viewer_world_objects::textures::TextureManager;
use sl_viewer_world_scene::terrain::PendingPatchRebuilds;

use crate::meshes::MeshManager;
use crate::objects::{PendingDecodedMeshes, PendingDecodedSculpts, PendingObjectEvents};

/// The scene's outstanding work, read from every store and queue that carries
/// any. Every store is optional, so an app that runs a subset of the viewer
/// (a harness) counts the stores it lacks as quiet.
#[derive(SystemParam)]
pub struct SceneQuiescence<'w, 's> {
    /// The current region, once the handshake has completed.
    region: Query<'w, 's, (), With<SlCurrentRegion>>,
    /// Textures: fetch, decode, retry.
    textures: Option<Res<'w, TextureManager>>,
    /// Meshes: fetch, decode, retry.
    meshes: Option<Res<'w, MeshManager>>,
    /// Wearable assets for the own bake.
    wearables: Option<Res<'w, WearableAssetManager>>,
    /// Animation assets.
    animations: Option<Res<'w, AnimationManager>>,
    /// Environment (EEP settings) assets.
    environment_assets: Option<Res<'w, EnvironmentAssetManager>>,
    /// Terrain patches awaiting a rebuild.
    patches: Option<Res<'w, PendingPatchRebuilds>>,
    /// Decoded meshes not yet built into geometry.
    pending_meshes: Option<Res<'w, PendingDecodedMeshes>>,
    /// Decoded sculpt maps not yet built into geometry.
    pending_sculpts: Option<Res<'w, PendingDecodedSculpts>>,
    /// Object events not yet folded into the scene.
    pending_objects: Option<Res<'w, PendingObjectEvents>>,
}

impl core::fmt::Debug for SceneQuiescence<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SceneQuiescence")
            .field("region_is_up", &self.region_is_up())
            .field("outstanding", &self.outstanding())
            .finish()
    }
}

impl SceneQuiescence<'_, '_> {
    /// Whether a region handshake has completed: the precondition for quiet
    /// meaning anything.
    #[must_use]
    pub fn region_is_up(&self) -> bool {
        !self.region.is_empty()
    }

    /// Everything still in flight or queued, summed across the stores.
    #[must_use]
    pub fn outstanding(&self) -> usize {
        let stores = [
            self.textures.as_deref().map(|store| {
                store_outstanding(&store.stats(), &store.gate_stats(), store.deferred_count())
            }),
            self.meshes.as_deref().map(|store| {
                store_outstanding(&store.stats(), &store.gate_stats(), store.deferred_count())
            }),
            self.wearables.as_deref().map(|store| {
                store_outstanding(&store.stats(), &store.gate_stats(), store.deferred_count())
            }),
            self.animations.as_deref().map(|store| {
                store_outstanding(&store.stats(), &store.gate_stats(), store.deferred_count())
            }),
            self.environment_assets.as_deref().map(|store| {
                store_outstanding(&store.stats(), &store.gate_stats(), store.deferred_count())
            }),
        ];
        let queues = [
            self.patches.as_deref().map(PendingPatchRebuilds::len),
            self.pending_meshes
                .as_deref()
                .map(PendingDecodedMeshes::len),
            self.pending_sculpts
                .as_deref()
                .map(PendingDecodedSculpts::len),
            self.pending_objects
                .as_deref()
                .map(PendingObjectEvents::len),
        ];
        stores
            .into_iter()
            .chain(queues)
            .flatten()
            .fold(0_usize, usize::saturating_add)
    }

    /// Whether the region is up and nothing is outstanding.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.region_is_up() && self.outstanding() == 0
    }
}

/// One store's outstanding work: every entry not yet ready or failed, every
/// gate slot in use or waiting, and the fetches parked outside its accounting.
fn store_outstanding(stats: &StoreStats, gate: &GateStats, deferred: usize) -> usize {
    [
        stats.queued,
        stats.reading_disk,
        stats.downloading,
        stats.decoding,
        gate.in_flight,
        gate.waiting,
        deferred,
    ]
    .into_iter()
    .fold(0_usize, usize::saturating_add)
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce as _;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::SlCurrentRegion;

    use super::SceneQuiescence;

    type TestError = Box<dyn core::error::Error>;

    /// An empty world is not quiet: nothing is in flight, but nothing is up.
    #[test]
    fn an_app_without_a_region_is_never_quiet() -> Result<(), TestError> {
        let mut world = World::new();
        let quiet = world
            .run_system_once(|quiescence: SceneQuiescence| {
                (
                    quiescence.region_is_up(),
                    quiescence.outstanding(),
                    quiescence.is_quiet(),
                )
            })
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(quiet, (false, 0, false));
        Ok(())
    }

    /// With a region and no stores at all, the scene is quiet — a harness that
    /// runs no asset store has nothing to wait for.
    #[test]
    fn a_region_with_nothing_in_flight_is_quiet() -> Result<(), TestError> {
        let mut world = World::new();
        world.spawn(SlCurrentRegion);
        let quiet = world
            .run_system_once(|quiescence: SceneQuiescence| quiescence.is_quiet())
            .map_err(|error| format!("{error:?}"))?;
        assert!(quiet);
        Ok(())
    }

    /// Every in-flight bucket counts as outstanding work, and the finished
    /// ones do not.
    #[test]
    fn only_unfinished_work_counts_as_outstanding() {
        use sl_client_bevy::{GateStats, StoreStats};

        let mut stats = StoreStats {
            ready: 100,
            failed: 3,
            cancelled: 2,
            in_memory: 100,
            ..StoreStats::default()
        };
        let mut gate = GateStats {
            capacity: 8,
            ..GateStats::default()
        };
        assert_eq!(super::store_outstanding(&stats, &gate, 0), 0);
        stats.queued = 2;
        stats.downloading = 1;
        stats.decoding = 1;
        gate.in_flight = 1;
        gate.waiting = 4;
        assert_eq!(super::store_outstanding(&stats, &gate, 3), 12);
    }
}
