//! The avatar layer's asset stores publish their own pipeline figures.
//!
//! The other half of `sl_viewer_world_objects::asset_stats`: the same
//! demand-driven publish, for the two stores the avatar layer owns rather than
//! the object layer —
//! [`AnimationManager`] and [`WearableAssetManager`]. Split from it because the
//! avatar modules became their own crate, and a publisher that named both sets
//! would have been the object layer reaching up into the avatar layer for
//! nothing but a statistics read.
//!
//! Both halves publish into the one [`PipelineStats`] resource that
//! `sl-viewer-world-api` owns, so the `F3` overlay still reads a single
//! resource and neither half knows the other exists.

use bevy::prelude::*;

use crate::animations::AnimationManager;
use crate::bake_inputs::WearableAssetManager;
use crate::world_api::{ANIMATION_LABEL, PipelineStats, StorePipelineStats, WEARABLE_LABEL};

/// Publishing the avatar layer's asset-store figures for whoever displays them.
#[derive(Debug, Default)]
pub struct AvatarAssetStatsPlugin;

impl Plugin for AvatarAssetStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PipelineStats>().add_systems(
            Update,
            publish_avatar_asset_store_stats.run_if(PipelineStats::pipeline_stats_wanted),
        );
    }
}

/// Snapshot the avatar layer's asset stores into
/// [`PipelineStats`](crate::world_api::PipelineStats).
///
/// Only runs while something is displaying the figures.
fn publish_avatar_asset_store_stats(
    animations: Res<AnimationManager>,
    wearables: Res<WearableAssetManager>,
    mut published: ResMut<PipelineStats>,
) {
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
}

#[cfg(test)]
mod tests {
    use super::{AvatarAssetStatsPlugin, publish_avatar_asset_store_stats};
    use crate::animations::AnimationManager;
    use crate::bake_inputs::WearableAssetManager;
    use crate::world_api::{ANIMATION_LABEL, PipelineStats, WEARABLE_LABEL};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// Nothing is published while nothing is looking, and both stores appear
    /// the frame the reader states its demand — the avatar half honours the
    /// same demand gate as the object half.
    #[test]
    fn avatar_stores_publish_only_while_something_is_looking() {
        let mut app = App::new();
        app.add_plugins(AvatarAssetStatsPlugin)
            .insert_resource(AnimationManager::new(None))
            .init_resource::<WearableAssetManager>();

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
        for label in [ANIMATION_LABEL, WEARABLE_LABEL] {
            assert!(
                published.get(label).is_some(),
                "{label} publishes its figures once the reader asks"
            );
        }
    }

    /// The publisher is a plain system over the two stores, so it can be run
    /// against a bare world without the plugin's run condition.
    #[test]
    fn publisher_covers_every_avatar_store() {
        let mut app = App::new();
        app.init_resource::<PipelineStats>()
            .insert_resource(AnimationManager::new(None))
            .init_resource::<WearableAssetManager>()
            .add_systems(Update, publish_avatar_asset_store_stats);
        app.update();
        assert_eq!(
            app.world().resource::<PipelineStats>().iter().count(),
            2,
            "every asset store the avatar layer owns is published"
        );
    }
}
