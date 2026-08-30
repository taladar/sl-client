//! The viewer's plugin groups: the one definition of which plugins make up the
//! input layer, the render stack, the world fold and the build tools, shared by
//! [`crate::run`] and every headless harness that stands up a subset of the
//! viewer — the readback rig, the fixture world, the full-stack harness against
//! the fake grid.
//!
//! A plugin appears in exactly one group. The groups are `Plugin`s rather than
//! `PluginGroup`s so each registration keeps its comment and its shape from the
//! viewer's original assembly; consumers add them with `add_plugins` either way.
//! What is **not** in a group — the UI scaffold and its panels, the session
//! driver, audio, media, persistence, diagnostics — stays in `run_session`,
//! because it either needs the login parameters or has no business in a test.
//!
//! Order matters in one place: [`ViewerRenderPlugins`] registers
//! `SlFaceMaterialPlugin`, whose `Assets<FaceMaterial>` the edit plugins'
//! `FromWorld` resources build against, so it is added before
//! [`ViewerEditPlugins`].

// The crate-root module aliases the moved registrations name bare, as
// `run_session` does at the crate root.
use crate::{
    avatars, body_physics, environment_assets, geometry_cache, ground, hand_pose, locomotion_ik,
    look_at, material_cache, movement, mutes, name_tag_content, reach, world_api,
};

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use sl_client_bevy::{
    CloudMaterialPlugin, SkyMaterialPlugin, StarMaterialPlugin, SunDiscMaterialPlugin,
    TerrainMaterialPlugin, WaterMaterialPlugin,
};

use crate::animations::AnimationPlayback;
use crate::animesh::ControlAvatarState;
use crate::appearance::{ServerBakeState, drive_server_bake};
use crate::asset_budget::{MeshUploadBudget, reset_mesh_upload_budget};
use crate::attachment_menu::AttachmentMenuPlugin;
use crate::avatar_menu::AvatarMenuPlugin;
use crate::avatars::RefetchAvatarTextures;
use crate::avatars::{
    AppearanceApplyBudget, AvatarBakeMaterials, AvatarRuntimeMorphs, OwnLocalBake, VolumeMorphGain,
    apply_avatar_names, fit_avatar_tag_heights, recenter_avatars, setup_avatar_body,
    toggle_volume_morphs, update_avatar_objects, update_coarse_avatars,
};
use crate::bake_inputs::{
    OwnBakeInputs, WearableAssetFetched, WearableAssetManager, assemble_own_bake,
    drive_wearable_requests, poll_wearable_assets, update_asset_caps,
};
use crate::bake_publish::OwnBakePublish;
use crate::bump::{BumpManager, apply_bump_normals, register_bump_faces};
use crate::camera::CameraPlugin;
use crate::edit_selection::EditSelectionPlugin;
use crate::edit_tool::EditToolPlugin;
use crate::environment::{EnvironmentState, ingest_environment, request_environment};
use crate::exposure::SlExposurePlugin;
use crate::flexi::simulate_flexi;
use crate::gizmos::EditGizmoPlugin;
use crate::glow::SlGlowPlugin;
use crate::hud_pick::pick_and_touch;
use crate::input_action::InputActionPlugin;
use crate::input_context::{InputContextPlugin, world_has_keyboard};
use crate::land_menu::LandMenuPlugin;
use crate::legacy_materials::{
    LegacyMaterialManager, apply_legacy_materials, apply_legacy_normal_maps,
    apply_legacy_specular_maps, drive_legacy_material_requests, receive_legacy_materials,
    register_legacy_materials,
};
use crate::materials::{
    MaterialManager, apply_blinn_phong_hide, apply_material_overrides, apply_pbr_textures,
    poll_materials, register_changed_render_materials, register_pbr_materials,
    revert_removed_render_materials, update_material_caps,
};
use crate::meshes::{MeshDecoded, MeshManager, poll_meshes, update_mesh_caps};
use crate::object_menu::ObjectMenuPlugin;
use crate::objects::{
    PendingDecodedMeshes, PendingDecodedSculpts, PendingObjectEvents, PrimLodTargets,
    TreeLodTargets, apply_object_meshes, apply_object_sculpts, apply_prim_lod, apply_tree_lod,
    recenter_objects, update_objects,
};
use crate::particle_render::{ParticleRenderPlugin, setup_particle_quad};
use crate::physics::PhysicsPlugin;
use crate::pie_menu::PieMenuPlugin;
use crate::probes::ReflectionProbePlugin;
use crate::render_priority::drive_render_priority;
use crate::rigged_attachments::{
    RiggedBindSkipLog, adopt_pending_attachments, apply_rigged_attachments,
};
use crate::sit_camera::SitCameraPlugin;
use crate::spacenav::SpacenavPlugin;
use crate::terrain::{
    PendingPatchRebuilds, TerrainTextures, drain_patch_rebuilds, recenter_terrain, update_terrain,
};
use crate::texture_anim::{drive_texture_animations, restore_stopped_animations};
use crate::textures::{
    DeferredFaceTextures, PrimTextures, TextureApplyBudget, TextureDecoded, TextureManager,
    apply_prim_textures, drain_deferred_face_textures, drain_lod_reuploads, poll_textures,
    reset_texture_apply_budget, serve_texture_boosts, sync_texture_blacklist, update_texture_caps,
};
use crate::tonemap::SlTonemapPlugin;
use crate::typing::TypingState;
use crate::underwater_fog::UnderwaterFogPlugin;
use crate::world_api::AvatarControls;
use crate::world_api::AvatarState;
use crate::world_api::BoostTexture;
use crate::world_api::DecodedTextures;
use crate::world_api::HudState;
use crate::world_api::ObjectState;
use crate::world_api::TerrainState;
use crate::world_api::world_scoped::{WorldResetSystems, WorldScopedAppExt as _};

/// Input focus and actions, the camera, avatar movement, the sit camera and the
/// SpaceNavigator: what turns keys, mouse and devices into world intent.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ViewerInputPlugins;

impl Plugin for ViewerInputPlugins {
    fn build(&self, app: &mut App) {
        // Input focus / modal context (viewer-input-focus-contexts): derives who owns
        // the keyboard and the cursor from `bevy_input_focus`. Gates every world key
        // binding below via `world_has_keyboard`, so typing into a focused text field
        // no longer also walks the avatar.
        app.add_plugins(InputContextPlugin);
        // The input action map (viewer-input-action-map): named actions + per-mode
        // binding profiles that replace the hardcoded keys in `movement` / `camera`.
        // Camera + movement read `ButtonInput<Action>`, gated once here on focus.
        app.add_plugins(InputActionPlugin);
        // The camera system (viewer-camera-*): one `ViewerCamera` entity driven by a
        // `CameraMode` state machine (mouselook / third-person / flycam), replacing the
        // debug fly-camera. Every `WorldPhase::CameraPositioned` consumer reads its pose.
        app.add_plugins(CameraPlugin);
        // Walking / turning / flying the own avatar from the movement actions.
        app.add_plugins(crate::movement::AvatarMovementPlugin);
        // Scripted sit camera + forced mouselook a seat imposes on sit
        // (viewer-sit-target-and-stand-button): tracked here, applied by
        // `position_camera`.
        app.add_plugins(SitCameraPlugin);
        // SpaceNavigator / 6-DOF device input (viewer-input-spacenav-*): publishes the
        // device state (Linux, behind the `spacenav` feature) for the flycam to consume.
        app.add_plugins(SpacenavPlugin);
    }
}

/// The render stack: the face material and every custom material pipeline,
/// the sky, water and their post-processes, particles, local lights, the
/// billboards, reflection probes, GPU avatars and the render-layer propagation.
/// Everything here needs a render app; a CPU-only harness leaves the group out.
///
/// The viewer takes the [`RenderStack::Full`] stack. The readback rig takes
/// [`RenderStack::Bare`]: the material pipelines, the probes, the transparency
/// ordering and the waterline split — what a registered scene's pixels are made
/// of — and nothing that stages content of its own (the sky dome, the ocean, the
/// lights) or reads the environment and the settings store (the post-processes,
/// the overlays, the GPU avatars).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ViewerRenderPlugins {
    /// How much of the stack to add.
    pub(crate) stack: RenderStack,
}

/// Which of the render stack's parts [`ViewerRenderPlugins`] adds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RenderStack {
    /// Everything: the viewer.
    #[default]
    Full,
    /// The materials, probes, transparency ordering and waterline split only:
    /// the test rigs' subset.
    #[cfg(test)]
    Bare,
}

impl ViewerRenderPlugins {
    /// The subset a registered scene's pixels are made of, and nothing that
    /// would stage content beside the scene's own.
    #[cfg(test)]
    pub(crate) const fn bare() -> Self {
        Self {
            stack: RenderStack::Bare,
        }
    }
}

impl Plugin for ViewerRenderPlugins {
    fn build(&self, app: &mut App) {
        let full = self.stack == RenderStack::Full;
        if full {
            // Amortise the sun's shadow-caster visibility cull over several frames
            // (viewer-perf-pbr-shadow-cluster-rez): replace Bevy's per-frame
            // check_dir_light_mesh_visibility with a round-robin one.
            app.add_plugins(crate::shadow_visibility::ShadowVisibilityPlugin);
            // P24.1: a larger sun/moon shadow map than the 2048 default, so the four
            // region-scale cascades (see `sky::shadow_cascades`) keep enough texels per
            // world unit to shadow an avatar crisply across a whole region.
            app.insert_resource(DirectionalLightShadowMap { size: 4096 });
        }
        // The custom material every prim/mesh/rigged/avatar/media face renders
        // through (per-map UV transforms + legacy Blinn-Phong specular; inert where
        // unused). Registered once here — and *before* the editor plugins below,
        // whose `FromWorld` resources (the selection highlight / face-cursor overlay
        // materials) build against `Assets<FaceMaterial>` at plugin-build time.
        app.add_plugins(crate::face_material::SlFaceMaterialPlugin);
        app.add_plugins(TerrainMaterialPlugin);
        if full {
            // In-world parcel borders / property lines (viewer-parcel-borders-render):
            // colour-coded vertical bands draped along parcel boundaries, driven by the
            // `parcel_borders` module's system below.
            app.add_plugins(crate::parcel_borders::ParcelBordersPlugin);
            // The in-world tracking beacon (viewer-beacons-beam-render): the vertical
            // beam + label + off-screen arrow drawn at the tracked position from the
            // shared `MapTracking` resource.
            app.add_plugins(crate::beacons::BeaconPlugin);
            // The world-space avatar name-tag billboards (viewer-name-tags-billboard-
            // render): the embedded billboard shader + material pipeline; the tag
            // systems themselves register with the avatar systems below.
            app.add_plugins(crate::name_tag_billboard::NameTagBillboardPlugin);
            // Object floating text (`llSetText`) reuses the name-tag billboard renderer
            // with its own fade registry + lifetime map (viewer-hover-text).
            app.add_plugins(crate::hover_text::HoverTextPlugin);
        }
        // The atmospheric sky dome material (P22.2), driven from the region's EEP
        // environment by `SkyPlugin`.
        app.add_plugins(SkyMaterialPlugin);
        // The sun / moon disc billboard material (P22.3), driven alongside the sky.
        app.add_plugins(SunDiscMaterialPlugin);
        // The scrolling cloud-layer material (P22.4), driven alongside the sky.
        app.add_plugins(CloudMaterialPlugin);
        // The night-time star-field material (P22.5), driven alongside the sky.
        app.add_plugins(StarMaterialPlugin);
        // The water-surface material (P23.1), driven from the region's EEP water
        // settings by `WaterPlugin`.
        app.add_plugins(WaterMaterialPlugin);
        if full {
            // The scene layer's own stacks, each scheduling itself against the world
            // phases rather than being wired system-by-system here: the sky dome with
            // its discs, clouds and stars; the endless ocean; the water-exclusion mask;
            // the CPU particle simulation; and the local-light budget.
            app.add_plugins((
                crate::sky::SkyPlugin,
                crate::water::WaterPlugin,
                crate::water_exclusion::WaterExclusionPlugin,
                crate::particles::ParticlesPlugin,
                crate::lights::LocalLightsPlugin,
            ));
            // GPU particles (viewer-perf-gpu-particles): the instanced quad renderer,
            // and the upload of the one shared unit-quad mesh every cloud instances.
            app.add_plugins(ParticleRenderPlugin);
            app.add_systems(Startup, setup_particle_quad);
        }
        // Water-relative transparency ordering (viewer-particle-water-ordering): a
        // render-world re-sort of the transparent phase so translucent content (a
        // fountain's spray, translucent prims) orders correctly against the
        // depth-writing water surface — below-water draws through it, above-water over
        // it — rather than being painted out by the camera-following plane.
        app.add_plugins(crate::transparency::TransparencyOrderPlugin);
        // Splits a translucent face that crosses the waterline into its two halves, so
        // each is ordered against the sea on its own side (the reference's `waterSign`).
        app.add_plugins(crate::water_clip::WaterClipPlugin);
        if full {
            // The underwater-fog post-process (P23.1): a fullscreen depth-based pass that
            // fogs everything below the water surface (reference `getWaterFogView`).
            app.add_plugins(UnderwaterFogPlugin);
            // The reference viewer's dynamic exposure (`generateExposure` / `exposureF`):
            // a fullscreen pass that reduces the composited scene's average luminance to a
            // 1×1 exposure map the tone mapper multiplies in, and the `sky_hdr_scale`
            // counterweight that keeps an EEP sky from washing out. Runs after the fog /
            // glow, before the tone mapper.
            app.add_plugins(SlExposurePlugin);
            // The reference viewer's tone mapper (P33.3): the one transfer from the linear
            // HDR scene to displayable colour, over the whole composited frame (reference
            // `postDeferredTonemap` — ACES / Khronos Neutral, blended by `RenderTonemapMix`).
            // Runs after the fog, which the reference likewise applies in linear space.
            app.add_plugins(SlTonemapPlugin);
            // The reference viewer's glow (`generateGlow` / `combineGlow`): the faithful
            // alpha-mask separable-Gaussian glow, replacing Bevy `Bloom`. Runs after the
            // tone mapper, as the reference does. Disabled by default until the materials
            // write the glow mask into their alpha (see `glow.rs`); the Bevy `Bloom` above
            // stays active meanwhile.
            app.add_plugins(SlGlowPlugin);
        }
        if full {
            // The GPU-avatar keystone spike (context/gpu-avatars.md §2.4 / §9.1 risk 1):
            // flag-gated by SL_VIEWER_GPU_AVATAR_SPIKE (`identity` | `marker`), read once
            // here. Unset (the default), this is a no-op plugin and the viewer is
            // byte-for-byte the normal path. Set, a compute pass overwrites one skinned
            // mesh's palette range inside Bevy's SkinUniforms buffer every frame — the
            // de-risking experiment for writing GPU-posed palettes into Bevy's own skin
            // path. Not a feature; delete or graft into Phase 1.
            app.add_plugins(crate::gpu_avatar_spike::GpuAvatarSpikePlugin::from_env());
            // The GPU-avatar pose pipeline (context/gpu-avatars.md §1/§2, Phases
            // 1a+1b): a compute pipeline re-runs the SL skeletal recurrence on the
            // GPU and writes the skin palettes into Bevy's SkinUniforms buffer. The
            // in-place path is the DEFAULT on a capable device (compute + storage
            // buffers, checked once at startup with an automatic legacy-CPU
            // fallback); SL_VIEWER_GPU_AVATARS overrides: `cpu`/`off` forces the
            // legacy CPU pose path, `ghost` the Phase 1a side-by-side comparison
            // harness (CPU in place + GPU-FK ghost 2 m aside). Env read once here.
            app.add_plugins(crate::gpu_avatars::GpuAvatarsPlugin::from_env());
        }
        // The reflection-probe pipeline (P33): captures a scene environment cubemap and
        // binds it as image-based lighting — a default (global) probe on the main view,
        // the scene-render half Bevy's env-map filter / consumer expect but never
        // produce.
        app.add_plugins(ReflectionProbePlugin);
        // The HUD layer (P35.1): the HUD screen puts its whole subtree — the routed
        // attachments and their faces — on `HUD_RENDER_LAYER` by propagating a single
        // `RenderLayers` down the hierarchy, so the world camera (default layer) never
        // draws a HUD. Propagation runs before Bevy decides what each camera sees, so a
        // just-routed attachment is layered in the very frame it is parented.
        app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate));
        app.configure_sets(
            PostUpdate,
            PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility),
        );
    }
}

/// The world fold: the state every `SlEvent` consumer writes, the systems
/// that turn the session's stream into terrain, objects, avatars and their
/// names, the HUD screen, picking, physics and the world pie menus. Needs the
/// UI scaffold (for the menus) and `ViewerSettings`, `AnimationManager` and
/// `CameraStart`, which the viewer inserts from its login parameters.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ViewerWorldPlugins;

impl Plugin for ViewerWorldPlugins {
    fn build(&self, app: &mut App) {
        // The screen-space HUD screen with its viewport-anchored attachment points.
        app.add_plugins(crate::hud::HudScreenPlugin);
        // The radial (pie) menu widget (viewer-ui-radial-menu): the mechanism only —
        // which entries a given pie holds is per-domain and belongs with the domain.
        app.add_plugins(PieMenuPlugin);
        // The avatar context / pie menu (viewer-avatar-context-menu): the self / other
        // entry trees and their dispatch, opened by right-clicking an avatar's name
        // tag or body.
        app.add_plugins(AvatarMenuPlugin);
        // The in-world object context / pie menu (viewer-object-context-menu): the
        // reference object entry tree and its dispatch, opened by right-clicking an
        // in-world object (the shared resolver lives with the avatar menu).
        app.add_plugins(ObjectMenuPlugin);
        // The worn-attachment context / pie menus (viewer-attachment-context-menu,
        // viewer-hud-context-menu): the self / other entry trees and their dispatch,
        // opened by right-clicking a worn attachment — in world or on a HUD point.
        app.add_plugins(AttachmentMenuPlugin);
        // The land / terrain context / pie menu (viewer-land-context-menu): the
        // reference land entry set and its dispatch, opened by right-clicking bare
        // terrain (the shared resolver lives with the avatar menu).
        app.add_plugins(LandMenuPlugin);
        // The object layer's own stacks: the avatar appearance / bake pipeline, and
        // the two `PostUpdate` pose passes that write animated joint globals after
        // transform propagation.
        app.add_plugins((
            crate::avatars::AvatarAppearancePlugin,
            crate::animations::AvatarAnimationPlugin,
            crate::animations::AvatarPosePlugin,
            crate::animesh::AnimeshPosePlugin,
            crate::objects::ObjectDiagnosticsPlugin,
        ));
        // Shared object land-impact model (GetObjectCost), read by the hover tooltip
        // and the build floater.
        app.add_plugins(crate::object_cost::ObjectCostPlugin);
        // GPU ID-buffer picking (Phase 3): the cursor pick is a render, not a
        // ray cast — pixel-perfect against exactly what is drawn, GPU-posed
        // avatars included.
        app.add_plugins(crate::gpu_pick::GpuPickPlugin);
        // The client-side physics foundation (P31.1): server-authoritative prim /
        // avatar dead-reckoning and collision-geometry building (no physics engine —
        // the viewer simulates nothing). Feeds the custom raycast index below.
        app.add_plugins(PhysicsPlugin);
        // The custom off-thread static raycast index (viewer-perf-custom-static-raycast-index):
        // a parry BVH over the prim colliders, maintained on a background task and
        // queried lock-free for camera collision — the replacement for avian's
        // per-fixed-step `SpatialQuery` maintenance.
        app.add_plugins(crate::raycast_index::RaycastIndexPlugin);
        // A distant teleport replaced the world: every store that declared itself
        // `WorldScoped` empties itself in `WorldResetSystems::Purge`, and that has to
        // happen before the re-centring pass. Each purge drops its subsystem's origin
        // anchor, so re-centring afterwards simply anchors on the destination instead
        // of shifting the (already purged) scene by a delta from the region we left.
        // A crossing or a neighbour teleport keeps the world and never purges at all.
        app.configure_sets(
            Update,
            WorldResetSystems::Purge
                .before(recenter_terrain)
                .before(recenter_objects)
                .before(recenter_avatars),
        );
        app.init_resource::<EnvironmentState>();
        app.init_resource::<VolumeMorphGain>();
        app.init_world_scoped::<TerrainState>();
        app.init_world_scoped::<TerrainTextures>();
        app.init_resource::<PendingPatchRebuilds>();
        app.init_resource::<MeshUploadBudget>();
        app.init_resource::<crate::terrain::CurrentTerrainLighting>();
        app.init_world_scoped::<ObjectState>();
        app.init_world_scoped::<PendingObjectEvents>();
        app.init_world_scoped::<RiggedBindSkipLog>();
        app.init_resource::<PendingDecodedMeshes>();
        app.init_resource::<PendingDecodedSculpts>();
        app.init_resource::<HudState>();
        app.init_resource::<PrimLodTargets>();
        app.init_resource::<TreeLodTargets>();
        app.init_resource::<geometry_cache::GeometryCache>();
        app.init_resource::<material_cache::MaterialCache>();
        app.init_world_scoped::<AvatarState>();
        app.init_resource::<avatars::AvatarPlaceholderAssets>();
        app.init_resource::<AppearanceApplyBudget>();
        app.init_resource::<world_api::MuteModel>();
        app.add_message::<world_api::RequestBlock>();
        app.init_resource::<name_tag_content::NameTagStatuses>();
        app.init_resource::<AvatarRuntimeMorphs>();
        app.init_resource::<look_at::LookAtTargets>();
        app.init_resource::<look_at::LookAtMotion>();
        app.init_resource::<reach::PointAtTargets>();
        app.init_resource::<reach::PointAtSelection>();
        app.init_resource::<reach::ReachMotion>();
        app.init_resource::<body_physics::BodyPhysicsMotion>();
        app.init_resource::<hand_pose::HandPoseMotion>();
        app.init_resource::<locomotion_ik::LocomotionAdjust>();
        app.init_resource::<ground::AvatarGround>();
        app.init_resource::<AvatarControls>();
        app.init_resource::<movement::MovementTuning>();
        app.init_resource::<TypingState>();
        app.init_resource::<ControlAvatarState>();
        app.init_resource::<TextureManager>();
        app.init_resource::<DecodedTextures>();
        app.init_resource::<PrimTextures>();
        app.init_resource::<TextureApplyBudget>();
        app.init_resource::<DeferredFaceTextures>();
        app.insert_resource(MaterialManager::new());
        app.init_resource::<LegacyMaterialManager>();
        app.init_resource::<BumpManager>();
        app.init_resource::<AvatarBakeMaterials>();
        app.init_resource::<OwnLocalBake>();
        app.init_resource::<ServerBakeState>();
        app.init_resource::<MeshManager>();
        app.init_resource::<OwnBakeInputs>();
        app.init_resource::<OwnBakePublish>();
        app.init_resource::<WearableAssetManager>();
        app.init_resource::<AnimationPlayback>();
        app.init_resource::<environment_assets::EnvironmentAssetManager>();
        app.add_message::<TextureDecoded>();
        app.add_message::<BoostTexture>();
        app.add_message::<MeshDecoded>();
        app.add_message::<WearableAssetFetched>();
        app.add_message::<RefetchAvatarTextures>();
        app.add_message::<crate::world_api::LocalChatNotice>();
        app.add_systems(Startup, setup_avatar_body);
        app.add_systems(PreUpdate, material_cache::detach_shared_face_materials);
        app.add_systems(
            PreUpdate,
            (reset_texture_apply_budget, reset_mesh_upload_budget),
        );
        // The world fold: environment, the asset stores, terrain and objects,
        // avatars and their names, attachments — every `SlEvent` consumer that
        // turns the session's stream into scene state.
        app.add_systems(
            Update,
            (
                // Request the region environment (EEP) on handshake, then fold the
                // grid's reply into `EnvironmentState` (P22.1); the sky / water /
                // shadow phases render from it. Nested into one tuple to stay within
                // Bevy's per-tuple system limit.
                (
                    request_environment,
                    ingest_environment,
                    // Fetch + swap in a pinned Modern (`KNOWN_SKY_*`) sky once its
                    // asset decodes; after `ingest_environment` so the shared
                    // environment (the Modern placeholder) is current.
                    crate::environment::resolve_modern_environment,
                ),
                // Trigger our own avatar's server-side bake so P14 has bakes to fetch.
                drive_server_bake,
                // Keep the texture store's `GetTexture` cap current, then poll
                // finished fetches before the consumers that apply them. The
                // blacklist mirror rides along, so a blacklisted texture asset
                // (viewer-derender-blacklist) is refused before any fetch.
                // Nested into one tuple to stay within Bevy's per-tuple system
                // limit. `serve_texture_boosts` drains the fetch requests raised
                // by the crates that only show textures and cannot reach the
                // manager directly.
                (
                    update_texture_caps,
                    sync_texture_blacklist,
                    poll_textures,
                    serve_texture_boosts,
                ),
                // The same for the mesh store's `GetMesh2` / `GetMesh` cap, plus the
                // client-side bake inputs (P15.2): keep the wearable-asset store's
                // `ViewerAsset` cap current, request our own outfit and fetch its
                // wearable assets, then assemble each bake region's layer list.
                // Nested into one tuple to stay within Bevy's per-tuple system limit.
                (
                    update_mesh_caps,
                    poll_meshes,
                    update_asset_caps,
                    drive_wearable_requests,
                    poll_wearable_assets,
                    assemble_own_bake,
                ),
                // Scene re-base on a region change, then fold terrain + object
                // events. (The purge half of a *distant* teleport is each store's
                // own `WorldScoped` impl, ordered ahead of this by the
                // `WorldResetSystems::Purge` set above.) Nested into one tuple to
                // stay within Bevy's per-tuple system limit.
                (
                    // Recenter (origin follows the root region) before folding
                    // terrain events, so patches are placed on the current origin;
                    // then drain a few of the queued seam / whole-region patch
                    // rebuilds (`PendingPatchRebuilds`).
                    //
                    // Terrain **wins** the shared per-frame `MeshUploadBudget`:
                    // ordered before the object mesh/sculpt spenders (`update_objects`
                    // inline warm-cache builds, `apply_object_meshes` and its chained
                    // `apply_object_sculpts` / `apply_rigged_attachments`) so a region
                    // hand-off builds the ground first — a missing ground plane is far
                    // more visible than a few deferred prims, and terrain is a small,
                    // bursty set (a region's 16×16 patches) that at most defers objects
                    // for a few frames per region connect.
                    (recenter_terrain, update_terrain, drain_patch_rebuilds)
                        .chain()
                        .before(update_objects)
                        .before(apply_object_meshes),
                    // Re-base world-root objects onto the new origin (a crossing or
                    // a teleport to an already-connected region) before folding
                    // object events, so a static object stays put and a new object
                    // is placed against the current origin. Chained after the
                    // terrain recenter so it re-bases to the same authoritative root.
                    (recenter_objects, update_objects)
                        .chain()
                        .in_set(world_api::WorldPhase::ObjectsUpdated),
                ),
                // Build the geometry of any mesh object whose asset just decoded, and
                // of any sculpted prim whose sculpt map just decoded — both spend from
                // the shared `MeshUploadBudget` (refilled in `PreUpdate`) so a decode
                // burst's builds spread across frames; `apply_rigged_attachments`
                // spends from the same pool via its `.after(apply_object_meshes)` edge.
                (apply_object_meshes, apply_object_sculpts).chain(),
                // Apply decoded diffuse textures to parked faces, then the PBR (GLTF)
                // render-material pipeline (P27.1): keep the material store's
                // `ViewerAsset` cap current, register each newly-spawned face's
                // material, fold finished material fetches into the face materials, and
                // drop each decoded texture map into its slot. Nested into one tuple to
                // stay within Bevy's per-tuple system limit; runs after the
                // face-spawning systems so a face's PBR material is seen.
                (
                    // Amortise face-material re-preps across frames: refill the
                    // per-frame budget, drape freshly decoded textures (deferring the
                    // overflow past a decode burst), patch faces parked on an
                    // already-decoded texture (a build-tool live-preview pre-fetch, then
                    // a commit re-tessellation) that the decode-event-driven
                    // `apply_prim_textures` alone would strand, then drain the deferred
                    // backlog (face drapes, then the lower-priority LOD re-uploads) with
                    // whatever budget is left. Chained so each drain sees the budget the
                    // earlier steps spent (see `TextureApplyBudget`).
                    (
                        apply_prim_textures,
                        crate::textures::patch_parked_decoded_textures,
                        drain_deferred_face_textures,
                        drain_lod_reuploads,
                    )
                        .chain(),
                    update_material_caps,
                    register_pbr_materials,
                    // A render material assigned to an existing prim (build tool /
                    // in-world retexture) refreshes its holder without re-tessellating
                    // its faces, so register the change here — `register_pbr_materials`
                    // only sees freshly-spawned faces.
                    register_changed_render_materials,
                    // Phase 3: a render material cleared in-world removes the holder,
                    // so revert each of its faces to Blinn-Phong / diffuse (and bring
                    // back their legacy specular / normal, no longer superseded).
                    revert_removed_render_materials,
                    poll_materials,
                    apply_material_overrides,
                    crate::materials::drive_local_overrides,
                    apply_pbr_textures,
                    // FIRE-35138: while the build tool's Texture tab is on the
                    // Blinn-Phong mode, render each selected linkset's PBR faces as
                    // Blinn-Phong so they can be judged as edited; restore PBR on
                    // deselect / PBR tab / leaving build mode.
                    apply_blinn_phong_hide,
                    // The legacy (normal/specular) render-material pipeline (P27.3):
                    // register each face carrying a `TextureEntry` material id, batch
                    // the `RenderMaterials` cap requests, fold in the replies, and
                    // apply the materials + their normal maps to the faces.
                    register_legacy_materials,
                    drive_legacy_material_requests,
                    receive_legacy_materials,
                    apply_legacy_materials,
                    apply_legacy_normal_maps,
                    apply_legacy_specular_maps,
                    // The legacy per-face bump / shiny / glow / fullbright flags
                    // (P27.4): register each newly-spawned bumped face and, once its
                    // diffuse texture decodes, generate and assign its normal map
                    // (fullbright / glow / shiny are folded in at material-build time
                    // by `face_material`). Runs after the legacy material path so a
                    // face's real `LLMaterial` normal map takes precedence over bump.
                    register_bump_faces,
                    apply_bump_normals,
                ),
                // Avatar placeholder spheres: full-object avatars first, then the
                // coarse-only ones (which dedupe against the full-object set); then
                // fold resolved names in and float each name tag over its sphere.
                (
                    (
                        // Re-base avatars onto the new origin before folding avatar
                        // updates, so a stationary neighbour avatar stays put and a
                        // freshly-streamed one is placed against the current origin.
                        recenter_avatars,
                        update_avatar_objects,
                        update_coarse_avatars,
                        // One batched legacy + display-name request per frame,
                        // however many avatars just appeared.
                        avatars::flush_name_requests,
                    )
                        .chain()
                        .in_set(world_api::WorldPhase::AvatarsUpdated),
                    // The mute list (name-tag colouring + the block-list UI):
                    // request once at session-up, ingest the Xfer'd list, turn
                    // each guarded block request into an entry, and mirror
                    // locally-issued mutes. `apply_block_requests` runs before
                    // `note_local_mutes` so a just-guarded block is mirrored in
                    // the same frame it is sent.
                    (
                        mutes::request_mute_list,
                        mutes::ingest_mute_list,
                        mutes::apply_block_requests,
                        mutes::note_local_mutes,
                    )
                        .chain(),
                    // Nearby-chat typing signals for the tag's Typing line,
                    // then the content composer that assembles every tag's
                    // lines from names / title / statuses / colours /
                    // own-avatar distance (change-guarded; the PostUpdate
                    // renderer chain reacts to `Changed<TagContent>`).
                    (
                        name_tag_content::ingest_tag_statuses,
                        name_tag_content::compose_name_tags
                            .after(update_avatar_objects)
                            .after(update_coarse_avatars)
                            .after(apply_avatar_names)
                            .after(world_api::WorldPhase::AvatarSkeletonsDriven)
                            .after(crate::groups::ingest_group_events),
                    )
                        .chain(),
                    // Float each avatar's name tag above its skeleton's head
                    // top, after the bodies (and their skeleton instances)
                    // exist.
                    fit_avatar_tag_heights.after(update_avatar_objects),
                ),
                // Parent each worn attachment to its avatar's skeleton joint (P16.1),
                // after the avatars (and their skeleton instances) have been spawned.
                // Parent each rigid attachment to its avatar's skeleton joint (P16), and
                // bind each worn rigged mesh to its wearer's skeleton instance as a
                // `SkinnedMesh` (P17.2). Both run after the avatars (and their skeletons)
                // are spawned; the rigged bind also waits on the mesh decode
                // (`apply_object_meshes` set its pending skinned build). Nested into one
                // tuple to stay within Bevy's per-tuple system limit.
                (
                    adopt_pending_attachments
                        .after(update_avatar_objects)
                        .after(update_objects),
                    apply_rigged_attachments
                        .after(apply_object_meshes)
                        .after(update_avatar_objects),
                ),
                apply_avatar_names,
            ),
        );
        // The crosshair pick tool (press `P`) to identify the object under the
        // centre of the screen. Separate calls to stay clear of Bevy's per-tuple
        // system limit. (The SL_VIEWER_LOG_OBJECTS diagnostic is registered
        // conditionally with the other env-gated debug systems below.)
        app.add_systems(
            Update,
            (
                // HUD picking & clicking (P35.3): a left click touches the HUD (or,
                // failing that, world) object under the pointer through an orthographic
                // HUD-camera pick, HUD before world. The cursor is free to click with
                // in every camera mode except mouselook (which grabs it), so no
                // free-cursor toggle is needed any more — the reference's model, where
                // third-person clicks the world directly. While the build tool is
                // active the left click belongs to selection (viewer-object-
                // selection-core), so the touch pick stands down.
                pick_and_touch.run_if(crate::edit_tool::edit_tool_inactive),
                // The world half of the touch resolves on the GPU pick's
                // readback, 1–2 frames after the press.
                crate::hud_pick::resolve_touch_pick.run_if(crate::edit_tool::edit_tool_inactive),
                // On-screen render priority (P20.2): re-rank the queued texture / mesh
                // fetches by the pixel area each object covers, so what the camera
                // looks at loads first. Throttled internally. It also picks each plain
                // prim's tessellation level of detail (P21.3); `apply_prim_lod` then
                // re-tessellates any prim whose level changed, so it runs after.
                drive_render_priority,
                // Nested into one tuple to stay within Bevy's per-tuple system
                // limit: the LOD appliers rebuild geometry after the driver has
                // picked the levels, and the geometry-cache prune periodically
                // drops cache entries whose shared meshes all died (every face
                // entity despawned) — the cache holds only weak asset ids, so
                // that is bookkeeping, not asset freeing.
                (
                    // Budget the LOD re-tessellations across frames: `apply_prim_lod`
                    // and (P26.2) `apply_tree_lod` — which regenerates any tree whose
                    // branching / billboard tier the driver changed — each spend from
                    // the shared `MeshUploadBudget` (refilled in `PreUpdate`), so a
                    // tick's whole batch spreads over frames instead of a single
                    // command-flush spike. Chained so tree sees the budget prim spent;
                    // all after the driver has picked levels.
                    (apply_prim_lod, apply_tree_lod)
                        .chain()
                        .after(drive_render_priority),
                    geometry_cache::prune_geometry_cache.run_if(
                        bevy::time::common_conditions::on_timer(geometry_cache::PRUNE_INTERVAL),
                    ),
                    material_cache::prune_material_cache.run_if(
                        bevy::time::common_conditions::on_timer(material_cache::PRUNE_INTERVAL),
                    ),
                ),
                // Flexi prims (P32.2): step each flexible prim's CPU chain simulation
                // and rewrite its deformed geometry in place, after `update_objects` so
                // this frame's spawns / rebuilds have seeded their chain state.
                simulate_flexi.after(update_objects),
                // Debug (`V`): toggle the shape's collision-volume displacement live, so
                // the effect can be A/B'd on one avatar in one session (P34.3).
                toggle_volume_morphs.run_if(world_has_keyboard),
                // Animated textures (P28.2): advance every prim's `llSetTextureAnim`
                // and fold the current frame's UV / flipbook placement into its faces,
                // then reset a face to its static placement when the animation stops.
                drive_texture_animations,
                restore_stopped_animations,
            ),
        );
        app.add_systems(
            Update,
            crate::terrain::drive_terrain_lighting.after(world_api::WorldPhase::CameraPositioned),
        );
        // The EEP settings-asset fetch cap for the World ▸ Environment Modern
        // presets.
        app.add_systems(
            Update,
            (
                environment_assets::update_environment_asset_caps,
                environment_assets::poll_environment_assets,
            ),
        );
    }
}

/// The build tools: the Build Tools floater and its tabs, the editors they
/// open, selection, gizmos, linking and undo. After [`ViewerRenderPlugins`].
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ViewerEditPlugins;

impl Plugin for ViewerEditPlugins {
    fn build(&self, app: &mut App) {
        // The build tool (viewer-object-edit-floater-shell): the Build Tools
        // floater, the edit-mode switch, and the numeric transform fields.
        app.add_plugins(EditToolPlugin);
        // The parameter tabs (viewer-prim-parameter-editing): the Object-tab
        // name / description / flag / shape editors and the Features-tab
        // material / flexi / light editors.
        app.add_plugins(crate::edit_params::EditParamsPlugin);
        // The Texture tab (viewer-prim-texture-editing) + Select Face tool
        // (viewer-edit-face-selection): per-face colour / transparency / glow /
        // bump / shiny / mapping and texture repeats / offset / rotation.
        app.add_plugins(crate::edit_texture::EditTexturePlugin);
        // The Blinn-Phong normal / specular maps + PBR (GLTF) material channels of
        // the Texture tab (viewer-face-materials-pbr).
        app.add_plugins(crate::edit_material::EditMaterialPlugin);
        // The Content tab + standalone Object Contents floater
        // (viewer-prim-inventory-editing): the prim task-inventory list, its
        // per-object cache, and the add / remove / rename / copy-out actions.
        app.add_plugins(crate::edit_contents::EditContentsPlugin);
        // The notecard viewer & editor floater (viewer-notecard-editor): open a
        // notecard from inventory, read it, edit its text when the item is
        // modifiable, and save it back to agent inventory. Embedded items are
        // listed (inline clickable rendering waits on the rich-text widget).
        app.add_plugins(crate::edit_notecard::EditNotecardPlugin);
        app.add_plugins(crate::notecard_render::NotecardRenderPlugin);
        app.add_plugins(crate::edit_wearable::EditWearablePlugin);
        app.add_plugins(crate::edit_material_asset::EditMaterialAssetPlugin);
        // The LSL script editor floater (viewer-lsl-editor-save-compile): open a
        // script from agent or task inventory, read it, edit its source when
        // modifiable, and save it back — which the simulator compiles, its result
        // surfaced as a status line and a diagnostics list (syntax highlighting
        // waits on the rich-text widget).
        app.add_plugins(crate::edit_script::EditScriptPlugin);
        // Offscreen material-on-a-sphere previews for the PBR render-material swatch
        // and the material picker's preview pane (viewer-material-swatch-sphere-preview).
        app.add_plugins(crate::material_preview::MaterialPreviewPlugin);
        // The Create tool (viewer-prim-creation): the create panel's base-type
        // picker and the click-to-rez placer for prims / trees / grass.
        app.add_plugins(crate::edit_create::EditCreatePlugin);
        // The object selection core (viewer-object-selection-core): click /
        // rubber-band selection, the selection set + highlight, and the
        // ObjectSelect / ObjectDeselect / ObjectProperties wire sync.
        app.add_plugins(EditSelectionPlugin);
        // The transform gizmos (viewer-transform-gizmos): move / rotate / stretch
        // manipulators over the selection, sending MultipleObjectUpdate edits.
        app.add_plugins(EditGizmoPlugin);
        // Prim linking / unlinking (viewer-prim-linking): Ctrl+L / Ctrl+Shift+L
        // and the Build menu, sending ObjectLink / ObjectDelink with the
        // last-selected object as the linkset root.
        app.add_plugins(crate::edit_link::EditLinkPlugin);
        // Object-edit undo / redo (viewer-build-undo-redo): Ctrl+Z / Ctrl+Y and
        // the Build menu, sending the server-side Undo / Redo for the selection.
        app.add_plugins(crate::edit_undo::EditUndoPlugin);
    }
}
