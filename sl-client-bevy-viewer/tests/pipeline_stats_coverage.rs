//! The two asset-stats publishers, together, cover every store the `F3`
//! pipeline overlay expects.
//!
//! Publishing is split across two crates — `sl_viewer_world_objects::asset_stats`
//! covers texture / mesh / material, `sl_viewer_world_avatar::avatar_asset_stats`
//! covers animation / wearable — because the avatar layer became its own crate
//! and a single publisher would have meant the object layer reaching up into it
//! for nothing but a statistics read.
//!
//! Each half already tests that it publishes its own stores. Neither can test
//! that the *pair* is complete, which is the thing the split can break quietly:
//! a plugin dropped from the composition root, or a store added to one half and
//! not to the other, shows up only as two missing lines on a debug panel nobody
//! reads on the frame it regresses. This test lives here because this is the
//! lowest crate that sees both halves.

#[cfg(test)]
mod test {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use sl_viewer_world_api::{PIPELINE_LABELS, PipelineStats};
    use sl_viewer_world_avatar::animations::AnimationManager;
    use sl_viewer_world_avatar::avatar_asset_stats::AvatarAssetStatsPlugin;
    use sl_viewer_world_avatar::bake_inputs::WearableAssetManager;
    use sl_viewer_world_objects::asset_stats::AssetStatsPlugin;
    use sl_viewer_world_objects::materials::MaterialManager;
    use sl_viewer_world_objects::meshes::MeshManager;
    use sl_viewer_world_objects::textures::TextureManager;

    /// An app carrying both publishers and the five stores they read.
    fn publishing_app() -> App {
        let mut app = App::new();
        app.add_plugins((AssetStatsPlugin, AvatarAssetStatsPlugin))
            .init_resource::<TextureManager>()
            .init_resource::<MeshManager>()
            .init_resource::<MaterialManager>()
            .insert_resource(AnimationManager::new())
            .init_resource::<WearableAssetManager>();
        app
    }

    /// Every label the overlay reads is published by one half or the other, and
    /// nothing publishes a label the overlay does not know about.
    #[test]
    fn both_halves_together_cover_every_label() {
        let mut app = publishing_app();
        app.world_mut()
            .resource_mut::<PipelineStats>()
            .set_wanted(true);
        app.update();

        let published = app.world().resource::<PipelineStats>();
        for label in PIPELINE_LABELS {
            assert!(
                published.get(label).is_some(),
                "no publisher covers {label} — a store's plugin is missing from \
                 the composition root, or the store was added to neither half"
            );
        }
        assert_eq!(
            published.iter().count(),
            PIPELINE_LABELS.len(),
            "a half published a label PIPELINE_LABELS does not list, so the \
             overlay will never show it"
        );
    }

    /// The demand gate survives the split: with two plugins registering the same
    /// resource, a hidden overlay must still cost nothing.
    #[test]
    fn neither_half_publishes_while_nothing_is_looking() {
        let mut app = publishing_app();
        app.update();
        assert_eq!(
            app.world().resource::<PipelineStats>().iter().count(),
            0,
            "a hidden overlay costs the stores nothing, from either half"
        );
    }
}
