//! The avatar layer's schedule: [`WorldAvatarPlugin`].
//!
//! Who is in the region and what their bodies are made of: the avatar fold, the
//! names and tags drawn over them, the client-side bake inputs, and the worn
//! attachments bound to their skeletons. Registered here, in the crate that owns
//! them, rather than in the binary that composes it (the roadmap task
//! `viewer-audit-plugins-own-their-schedule`).
//!
//! # Ordering across the crate boundary
//!
//! Three edges reach outside this crate, and each is stated the way the tier
//! allows:
//!
//! - **downwards, by name**: the attachment binds run after the object layer's
//!   `update_objects` / `apply_object_meshes`, which this crate may name
//!   because `sl-viewer-world-objects` sits below it;
//! - **upwards, by set**: the name-tag composer runs after
//!   [`GroupsSystems::Ingested`](sl_viewer_social::groups::GroupsSystems), so the
//!   active group's title is this frame's — the People surface that fills the
//!   model sits far above the world and must not be named here;
//! - **sideways, by phase**: the composer also waits on
//!   [`WorldPhase::AvatarSkeletonsDriven`], the vocabulary
//!   `sl-viewer-world-api` exists to provide.
//!
//! # What is **not** here
//!
//! The render-side avatar work — the GPU pose pipeline, the complexity limit and
//! its floater, the derender blacklist, the replay rig — is its own plugin, so a
//! CPU-only harness takes the fold without any of it.

use bevy::prelude::*;
use sl_viewer_kit::appearance::{ServerBakeState, drive_server_bake};
use sl_viewer_world_api::world_scoped::{WorldResetSystems, WorldScopedAppExt as _};
use sl_viewer_world_api::{AvatarState, WorldPhase, world_has_keyboard};
use sl_viewer_world_objects::objects::{apply_object_meshes, update_objects};

use crate::animations::AnimationPlayback;
use crate::animesh::ControlAvatarState;
use crate::avatars::{
    AppearanceApplyBudget, AvatarBakeMaterials, AvatarRuntimeMorphs, OwnLocalBake,
    RefetchAvatarTextures, VolumeMorphGain, apply_avatar_names, fit_avatar_tag_heights,
    recenter_avatars, setup_avatar_body, toggle_volume_morphs, update_avatar_objects,
    update_coarse_avatars,
};
use crate::bake_inputs::{
    OwnBakeInputs, WearableAssetFetched, WearableAssetManager, assemble_own_bake,
    drive_wearable_requests, poll_wearable_assets, publish_rlv_avatar_sex, update_asset_caps,
};
use crate::bake_publish::OwnBakePublish;
use crate::rigged_attachments::{
    AttachmentAdoptSkipLog, RiggedBindSkipLog, adopt_pending_attachments, apply_rigged_attachments,
    route_in_world_rigged_meshes,
};
use crate::typing::TypingState;

/// The avatar layer's fold: who is here, what they are called, what their
/// bodies are baked from, and what they are wearing.
///
/// Add it to any app that wants avatars in its world. It needs no login
/// parameters, so a headless fixture world takes exactly this.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorldAvatarPlugin;

impl Plugin for WorldAvatarPlugin {
    fn build(&self, app: &mut App) {
        register_state(app);
        schedule_frame(app);
        // The avatar layer's own stacks: the appearance / bake pipeline, and the
        // two `PostUpdate` pose passes that write animated joint globals after
        // transform propagation.
        app.add_plugins((
            crate::avatars::AvatarAppearancePlugin,
            crate::animations::AvatarAnimationPlugin,
            crate::animations::AvatarPosePlugin,
            crate::animesh::AnimeshPosePlugin,
        ));
    }
}

/// The stores and messages the avatar layer owns.
fn register_state(app: &mut App) {
    // Keyed by the departed region's scoped ids, so a **distant** teleport
    // throws them away.
    app.init_world_scoped::<AvatarState>();
    app.init_world_scoped::<RiggedBindSkipLog>();
    app.init_world_scoped::<AttachmentAdoptSkipLog>();
    app.init_resource::<crate::avatars::AvatarPlaceholderAssets>();
    app.init_resource::<AppearanceApplyBudget>();
    app.init_resource::<AvatarRuntimeMorphs>();
    app.init_resource::<AvatarBakeMaterials>();
    app.init_resource::<OwnLocalBake>();
    app.init_resource::<ServerBakeState>();
    // The live A/B state of the shape's collision-volume displacement
    // (P34.3), seeded from `SL_VIEWER_VOLUME_MORPH_GAIN` and toggled by `V`.
    app.init_resource::<VolumeMorphGain>();
    app.init_resource::<crate::name_tag_content::NameTagStatuses>();
    app.init_resource::<crate::look_at::LookAtTargets>();
    app.init_resource::<crate::look_at::LookAtMotion>();
    app.init_resource::<crate::reach::PointAtTargets>();
    app.init_resource::<crate::reach::PointAtSelection>();
    app.init_resource::<crate::reach::ReachMotion>();
    app.init_resource::<crate::body_physics::BodyPhysicsMotion>();
    app.init_resource::<crate::hand_pose::HandPoseMotion>();
    app.init_resource::<crate::locomotion_ik::LocomotionAdjust>();
    app.init_resource::<crate::ground::AvatarGround>();
    // The own avatar's advertised movement intent: written by the view
    // layer's movement driver, read here by the client-side locomotion
    // animations and the motion stops. `init_resource` is idempotent, so
    // declaring the read costs nothing where the writer is present and
    // keeps a viewless fixture world from failing param validation.
    app.init_resource::<sl_viewer_world_api::AvatarControls>();
    app.init_resource::<TypingState>();
    app.init_resource::<ControlAvatarState>();
    app.init_resource::<OwnBakeInputs>();
    app.init_resource::<OwnBakePublish>();
    app.init_resource::<WearableAssetManager>();
    app.init_resource::<AnimationPlayback>();
    // Written by `publish_rlv_avatar_sex` below and read by the RLV engine,
    // which takes it as an `Option<Res<_>>` precisely so it can run in an
    // app that has no avatars.
    app.init_resource::<sl_viewer_world_api::rlv::RlvExtFacts>();
    app.add_message::<WearableAssetFetched>();
    app.add_message::<RefetchAvatarTextures>();
}

/// The avatar layer's frame.
fn schedule_frame(app: &mut App) {
    // A distant teleport replaced the world: `AvatarState` empties itself in
    // `WorldResetSystems::Purge`, which has to happen before the re-centring
    // pass — the purge drops the layer's origin anchor, so re-centring
    // afterwards anchors on the destination instead of shifting the (already
    // purged) scene by a delta from the region we left.
    app.configure_sets(Update, WorldResetSystems::Purge.before(recenter_avatars));
    app.add_systems(Startup, setup_avatar_body);
    app.add_systems(
        Update,
        (
            // Trigger our own avatar's server-side bake so the bake fetch has
            // bakes to fetch.
            drive_server_bake,
            bake_input_pipeline(),
            avatar_fold_pipeline(),
            name_tag_pipeline(),
            // Float each avatar's name tag above its skeleton's head top,
            // after the bodies (and their skeleton instances) exist.
            fit_avatar_tag_heights.after(update_avatar_objects),
            apply_avatar_names,
            attachment_bind_pipeline(),
            // Debug (`V`): toggle the shape's collision-volume displacement
            // live, so the effect can be A/B'd on one avatar in one session
            // (P34.3).
            toggle_volume_morphs.run_if(world_has_keyboard),
        ),
    );
}

// ---------------------------------------------------------------------------
// The avatar layer's pipelines
// ---------------------------------------------------------------------------
//
// Functions rather than inline tuples so the order can be *tested*:
// `ScheduleConfigs` is a value, so a test can put one into a bare `Schedule`,
// initialize it, and read back the order the executor will actually use. A
// comment claiming an order cannot be tested; this can. See the roadmap task
// `viewer-audit-system-ordering-claims`.

use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;

/// The client-side bake inputs (P15.2): keep the wearable-asset store's
/// `ViewerAsset` cap current, request our own outfit, fetch its wearable assets,
/// and assemble each bake region's layer list from them.
///
/// A genuine chain: a cap refresh un-parks the requests the poll finishes,
/// `assemble_own_bake` builds its layer lists out of exactly the wearable assets
/// `poll_wearable_assets` has folded in, and `publish_rlv_avatar_sex` reads the
/// worn Shape that landed with them — the worn Shape being what says whether the
/// avatar is male, one of the two facts RLV's debug-setting allowlist cannot
/// read out of a settings store.
fn bake_input_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        update_asset_caps,
        drive_wearable_requests,
        poll_wearable_assets,
        assemble_own_bake,
        publish_rlv_avatar_sex,
    )
        .chain()
}

/// The avatar fold: re-base onto the current origin, fold this frame's avatar
/// updates — full objects first, then the coarse-only ones, which dedupe against
/// the full-object set — and finally ask for whatever names just appeared.
///
/// Chained because each stage reads what the previous one wrote: re-basing
/// before the fold keeps a stationary neighbour avatar put and places a
/// freshly-streamed one against the current origin, the coarse pass needs the
/// full-object set to dedupe against, and `flush_name_requests` batches exactly
/// one legacy + display-name request per frame however many avatars just
/// appeared.
fn avatar_fold_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        recenter_avatars,
        update_avatar_objects,
        update_coarse_avatars,
        crate::avatars::flush_name_requests,
    )
        .chain()
        .in_set(WorldPhase::AvatarsUpdated)
}

/// The name tags' content: fold the nearby-chat typing signals in for the tag's
/// Typing line, then compose every tag's lines from names / title / statuses /
/// colours / own-avatar distance.
///
/// The composer is change-guarded and the `PostUpdate` renderer chain reacts to
/// `Changed<TagContent>`, so everything it reads has to be this frame's: the
/// avatars themselves, the names that resolved, the skeletons that were driven,
/// and — through the shared
/// [`GroupsSystems::Ingested`](sl_viewer_social::groups::GroupsSystems) set — the
/// group memberships whose titles the tags show.
fn name_tag_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        crate::name_tag_content::ingest_tag_statuses,
        crate::name_tag_content::compose_name_tags
            .after(update_avatar_objects)
            .after(update_coarse_avatars)
            .after(apply_avatar_names)
            .after(WorldPhase::AvatarSkeletonsDriven)
            .after(sl_viewer_social::groups::GroupsSystems::Ingested),
    )
        .chain()
}

/// Bind each worn attachment to its wearer: parent the rigid ones to their
/// avatar's skeleton joint (P16), and bind each worn rigged mesh to its wearer's
/// skeleton instance as a `SkinnedMesh` (P17.2).
///
/// Both run after the avatars (and their skeletons) are spawned; the rigged bind
/// also waits on the mesh decode, which is what set its pending skinned build.
/// `route_in_world_rigged_meshes` runs before the bind, so a rigged mesh
/// standing in the world is handed to the static mesh path rather than traced as
/// a wearer that never resolves.
fn attachment_bind_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        adopt_pending_attachments
            .after(update_avatar_objects)
            .after(update_objects),
        route_in_world_rigged_meshes
            .after(apply_object_meshes)
            .after(update_objects)
            .before(apply_rigged_attachments),
        apply_rigged_attachments
            .after(apply_object_meshes)
            .after(update_avatar_objects),
    )
        .into_configs()
}

#[cfg(test)]
mod tests {
    use super::{attachment_bind_pipeline, avatar_fold_pipeline, bake_input_pipeline};

    use sl_viewer_world_api::schedule_order::ScheduleOrder;
    use sl_viewer_world_api::stage;
    use sl_viewer_world_objects::objects::{apply_object_meshes, update_objects};

    use crate::avatars::{
        apply_avatar_names, recenter_avatars, update_avatar_objects, update_coarse_avatars,
    };
    use crate::bake_inputs::{
        assemble_own_bake, drive_wearable_requests, poll_wearable_assets, publish_rlv_avatar_sex,
        update_asset_caps,
    };
    use crate::rigged_attachments::{apply_rigged_attachments, route_in_world_rigged_meshes};

    /// The avatar layer's fold, reduced to what an ordering assertion needs.
    ///
    /// The object-layer systems the attachment binds order themselves against
    /// are registered too: an edge naming a system that is not in the schedule
    /// constrains nothing.
    fn fold() -> ScheduleOrder {
        ScheduleOrder::of((
            bake_input_pipeline(),
            avatar_fold_pipeline(),
            attachment_bind_pipeline(),
            apply_avatar_names,
            (update_objects, apply_object_meshes),
        ))
    }

    /// The bake inputs are one chain: the cap refresh un-parks the requests the
    /// poll finishes, the assembly reads exactly what the poll folded in, and the
    /// RLV sex fact is published from the Shape that landed with them.
    #[test]
    fn bake_inputs_run_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(update_asset_caps),
            stage!(drive_wearable_requests),
            stage!(poll_wearable_assets),
            stage!(assemble_own_bake),
            stage!(publish_rlv_avatar_sex),
        ]);
    }

    /// The avatar fold re-bases before it folds, and the coarse pass sees the
    /// full-object set it dedupes against.
    #[test]
    fn avatar_fold_runs_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(recenter_avatars),
            stage!(update_avatar_objects),
            stage!(update_coarse_avatars),
            stage!(crate::avatars::flush_name_requests),
        ]);
    }

    /// An attachment is routed before it is bound, and neither happens before the
    /// object that carries it and the avatar that wears it exist.
    #[test]
    fn attachments_bind_after_their_inputs() {
        let fold = fold();
        fold.assert_pipeline(&[
            stage!(route_in_world_rigged_meshes),
            stage!(apply_rigged_attachments),
        ]);
        fold.assert_pipeline(&[
            stage!(apply_object_meshes),
            stage!(apply_rigged_attachments),
        ]);
        fold.assert_pipeline(&[
            stage!(update_avatar_objects),
            stage!(apply_rigged_attachments),
        ]);
    }
}
