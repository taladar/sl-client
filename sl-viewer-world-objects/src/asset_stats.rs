//! The object layer's asset stores publish their own pipeline figures.
//!
//! The pipeline-status overlay (`F3`, `sl_viewer_world_scene::diagnostics`) shows
//! a line per asset store: per-stage entry counts, in-memory footprint, and the
//! admission gate's in-flight / capacity / waiting figures. It used to read them
//! by taking one `Res<…Manager>` per store — which meant the scene layer named
//! four of this crate's asset stores (`AnimationManager`, `MaterialManager`,
//! `MeshManager`, `WearableAssetManager`) for no reason but to call three
//! accessors on each, and named [`TextureManager`] there as well as in the places
//! that genuinely fetch through it.
//!
//! This publishes them instead:
//! [`PipelineStats`], a keyed resource defined in
//! the layer *below* both, carries `label → figures`, and the overlay reads that
//! one resource. The dependency runs the way the data does.
//!
//! It is demand-driven: the overlay states whether it is looking
//! (`PipelineStats::set_wanted`), and this system's run condition reads that, so a
//! hidden overlay costs one boolean check per frame rather than five stats
//! snapshots.

use bevy::prelude::*;

use crate::animations::AnimationManager;
use crate::bake_inputs::WearableAssetManager;
use crate::materials::MaterialManager;
use crate::meshes::MeshManager;
use crate::textures::TextureManager;
use crate::world_api::{
    ANIMATION_LABEL, MATERIAL_LABEL, MESH_LABEL, PipelineStats, StorePipelineStats, TEXTURE_LABEL,
    WEARABLE_LABEL,
};

/// Publishing the object layer's asset-store figures for whoever displays them.
#[derive(Debug, Default)]
pub struct AssetStatsPlugin;

impl Plugin for AssetStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PipelineStats>().add_systems(
            Update,
            publish_asset_store_stats.run_if(PipelineStats::pipeline_stats_wanted),
        );
    }
}

/// Snapshot each of this crate's asset stores into
/// [`PipelineStats`](crate::world_api::PipelineStats).
///
/// Only runs while something is displaying the figures.
fn publish_asset_store_stats(
    textures: Res<TextureManager>,
    meshes: Res<MeshManager>,
    animations: Res<AnimationManager>,
    wearables: Res<WearableAssetManager>,
    materials: Res<MaterialManager>,
    mut published: ResMut<PipelineStats>,
) {
    published.publish(
        TEXTURE_LABEL,
        StorePipelineStats {
            stats: textures.stats(),
            gate: textures.gate_stats(),
            deferred: textures.deferred_count(),
        },
    );
    published.publish(
        MESH_LABEL,
        StorePipelineStats {
            stats: meshes.stats(),
            gate: meshes.gate_stats(),
            deferred: meshes.deferred_count(),
        },
    );
    published.publish(
        ANIMATION_LABEL,
        StorePipelineStats {
            stats: animations.stats(),
            gate: animations.gate_stats(),
            deferred: animations.deferred_count(),
        },
    );
    published.publish(
        WEARABLE_LABEL,
        StorePipelineStats {
            stats: wearables.stats(),
            gate: wearables.gate_stats(),
            deferred: wearables.deferred_count(),
        },
    );
    published.publish(
        MATERIAL_LABEL,
        StorePipelineStats {
            stats: materials.stats(),
            gate: materials.gate_stats(),
            deferred: materials.deferred_count(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{AssetStatsPlugin, publish_asset_store_stats};
    use crate::animations::AnimationManager;
    use crate::bake_inputs::WearableAssetManager;
    use crate::materials::MaterialManager;
    use crate::meshes::MeshManager;
    use crate::textures::TextureManager;
    use crate::world_api::{
        ANIMATION_LABEL, MATERIAL_LABEL, MESH_LABEL, PipelineStats, TEXTURE_LABEL, WEARABLE_LABEL,
    };
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// Nothing is published while nothing is looking, and every store appears the
    /// frame the reader states its demand — the whole point of the inversion is
    /// that the display costs the stores nothing until it is up.
    #[test]
    fn stores_publish_only_while_something_is_looking() {
        let mut app = App::new();
        app.add_plugins(AssetStatsPlugin)
            .init_resource::<TextureManager>()
            .init_resource::<MeshManager>()
            .insert_resource(AnimationManager::new(None))
            .init_resource::<WearableAssetManager>()
            .init_resource::<MaterialManager>();

        app.update();
        assert_eq!(
            app.world().resource::<PipelineStats>().iter().count(),
            0,
            "a hidden overlay costs the stores nothing"
        );

        app.world_mut()
            .resource_mut::<PipelineStats>()
            .set_wanted(true);
        app.update();
        let published = app.world().resource::<PipelineStats>();
        for label in [
            TEXTURE_LABEL,
            MESH_LABEL,
            ANIMATION_LABEL,
            WEARABLE_LABEL,
            MATERIAL_LABEL,
        ] {
            assert!(
                published.get(label).is_some(),
                "{label} publishes its figures once the reader asks"
            );
        }
    }

    /// The publisher is a plain system over the five stores, so it can be run
    /// against a bare world without the plugin's run condition.
    #[test]
    fn publisher_covers_every_store_in_this_crate() {
        let mut app = App::new();
        app.init_resource::<PipelineStats>()
            .init_resource::<TextureManager>()
            .init_resource::<MeshManager>()
            .insert_resource(AnimationManager::new(None))
            .init_resource::<WearableAssetManager>()
            .init_resource::<MaterialManager>()
            .add_systems(Update, publish_asset_store_stats);
        app.update();
        assert_eq!(
            app.world().resource::<PipelineStats>().iter().count(),
            5,
            "every asset store this crate owns is published"
        );
    }
}
