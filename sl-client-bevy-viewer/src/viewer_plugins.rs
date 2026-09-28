//! The viewer's plugin groups: the one definition of which plugins make up the
//! interface, the input layer, the render stack, the world fold, the build
//! tools and the host shell. [`crate::assembly`] assembles all six into the
//! viewer; the headless harnesses that stand up a subset of it — the readback
//! rig, the fixture world — take the groups they need.
//!
//! A plugin appears in exactly one group. The groups are `Plugin`s rather than
//! `PluginGroup`s so each registration keeps its comment and its shape from the
//! viewer's original assembly; consumers add them with `add_plugins` either way.
//! What is **not** in a group — the protocol plugin, the session driver, the
//! settings store, the capture harness — is the builder's, because it needs the
//! login parameters or the run's options.
//!
//! Order matters in two places: [`ViewerUiPlugins`] comes first, because the
//! world group's pie menus need its scaffold; and [`ViewerRenderPlugins`]
//! registers `SlFaceMaterialPlugin`, whose `Assets<FaceMaterial>` the edit
//! plugins' `FromWorld` resources build against, so it is added before
//! [`ViewerEditPlugins`].
//!
//! # These are groups, not schedules
//!
//! What a group holds is `add_plugins` lines. The world's stores, messages,
//! systems and ordering edges live in the crates that own them — the object
//! layer's in `sl_viewer_world_objects::WorldObjectsPlugin`, the scene's in
//! `sl_viewer_world_scene::WorldScenePlugin`, the avatars' in
//! `sl_viewer_world_avatar::WorldAvatarPlugin` — so a crate can be dropped into
//! a test `App` on its own and an ordering claim is tested next to the code it
//! constrains (the roadmap task `viewer-audit-plugins-own-their-schedule`).
//! A registration that stays here is one no single crate owns: the pie menus
//! over the world, the pick resolver the binary chooses between, and the touch
//! pick whose run condition comes from the build tools, a crate above the world.

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use sl_client_bevy::{
    CloudMaterialPlugin, SkyMaterialPlugin, StarMaterialPlugin, SunDiscMaterialPlugin,
    TerrainMaterialPlugin, WaterMaterialPlugin,
};

use crate::about_floater::AboutFloaterPlugin;
use crate::about_land::AboutLandPlugin;
use crate::about_landmark::AboutLandmarkPlugin;
use crate::about_region::AboutRegionPlugin;
use crate::assembly::MediaRuntime;
use crate::asset_blacklist::AssetBlacklistPlugin;
use crate::attachment_menu::AttachmentMenuPlugin;
use crate::avatar_menu::AvatarMenuPlugin;
use crate::avatar_picker::AvatarPickerPlugin;
use crate::avatar_profile::AvatarProfilePlugin;
use crate::blocked::BlockedPlugin;
use crate::camera::CameraPlugin;
use crate::chat_input::ChatInputPlugin;
use crate::conversations::ConversationsPlugin;
use crate::derender::DerenderPlugin;
use crate::edit_selection::EditSelectionPlugin;
use crate::edit_tool::EditToolPlugin;
use crate::emoji_complete::ColonCompletePlugin;
use crate::emoji_picker::EmojiPickerPlugin;
use crate::experience_log::ExperienceLogPlugin;
use crate::experience_permission::ExperiencePermissionPlugin;
use crate::experience_picker::ExperiencePickerPlugin;
use crate::experience_profile::ExperienceProfilePlugin;
use crate::experiences_floater::ExperiencesPlugin;
use crate::exposure::SlExposurePlugin;
use crate::floater::FloaterPlugin;
use crate::floater_persist::FloaterPersistPlugin;
use crate::gizmos::EditGizmoPlugin;
use crate::glow::SlGlowPlugin;
use crate::group_notice::GroupNoticePlugin;
use crate::group_picker::GroupPickerPlugin;
use crate::group_profile::GroupProfilePlugin;
use crate::groups::GroupsPlugin;
use crate::hud_pick::pick_and_touch;
use crate::i18n::ViewerI18nPlugin;
use crate::input_action::InputActionPlugin;
use crate::input_context::InputContextPlugin;
use crate::inventory::InventoryPlugin;
use crate::inventory_actions::InventoryActionsPlugin;
use crate::inventory_drag::InventoryDragPlugin;
use crate::inventory_filters::InventoryFiltersPlugin;
use crate::inventory_gallery::InventoryGalleryPlugin;
use crate::inventory_properties::InventoryPropertiesPlugin;
use crate::land_menu::LandMenuPlugin;
use crate::load_url::LoadUrlPlugin;
use crate::local_chat_input::LocalChatInputPlugin;
use crate::nearby_chat_bar::NearbyChatBarPlugin;
use crate::notification_host::{NotificationHostPlugin, NotificationSourcesPlugin};
use crate::notification_persist::NotificationPersistPlugin;
use crate::object_menu::ObjectMenuPlugin;
use crate::offers_invites::OffersInvitesPlugin;
use crate::particle_render::{ParticleRenderPlugin, setup_particle_quad};
use crate::people::PeoplePlugin;
use crate::physics::PhysicsPlugin;
use crate::pie_menu::PieMenuPlugin;
use crate::probes::ReflectionProbePlugin;
use crate::resolution_divisor::ResolutionDivisorPlugin;
use crate::script_dialog::ScriptDialogPlugin;
use crate::script_permission::ScriptPermissionPlugin;
use crate::settings::SettingsPersistPlugin;
use crate::settings_binding::SettingsBindingPlugin;
use crate::settings_index::SettingsIndexPlugin;
use crate::sit_camera::SitCameraPlugin;
use crate::spacenav::{DeviceRead, SpacenavPlugin};
use crate::stand_stop_button::StandStopButtonPlugin;
use crate::tonemap::SlTonemapPlugin;
use crate::ui::ViewerUiPlugin;
use crate::ui_tab::TabWidgetPlugin;
use crate::ui_table::TableWidgetPlugin;
use crate::ui_text_input::TextInputPlugin;
use crate::underwater_fog::UnderwaterFogPlugin;
use crate::virtual_list::VirtualListPlugin;

/// Input focus and actions, the camera, avatar movement, the sit camera and the
/// SpaceNavigator: what turns keys, mouse and devices into world intent.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ViewerInputPlugins {
    /// Whether the 6-DOF device is actually read — see [`DeviceRead`].
    pub(crate) spacenav: DeviceRead,
}

impl ViewerInputPlugins {
    /// The input fold with the 6-DOF **device read** left out — the headless
    /// fixture world's configuration, and a windowless viewer's. Every other
    /// input seam is driven through a window message the harness writes; the
    /// SpaceNavigator alone is read straight off the machine, so a viewer that
    /// must not answer the machine's input keeps only the ECS half.
    pub(crate) const fn without_devices() -> Self {
        Self {
            spacenav: DeviceRead::None,
        }
    }
}

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
        app.add_plugins(SpacenavPlugin {
            read: self.spacenav,
        });
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
            // The in-world Land Owners tint (viewer-parcel-owners-terrain-overlay):
            // the ground itself shaded by parcel-ownership class, off the same
            // decoded overlay grid the property lines use.
            app.add_plugins(crate::parcel_owners::ParcelOwnerOverlayPlugin);
            // The in-world tracking beacon (viewer-beacons-beam-render): the vertical
            // beam + label + off-screen arrow drawn at the tracked position from the
            // shared `MapTracking` resource.
            app.add_plugins(crate::beacons::BeaconPlugin);
            // The in-world debug-beacon markers (viewer-region-telehub): the cross
            // markers an open floater asks for through the shared `DebugBeacons`
            // resource — the telehub and its selected spawn point today.
            app.add_plugins(crate::debug_beacons::DebugBeaconPlugin);
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
            // the scene-depth copy the water's refraction rejects against; the CPU
            // particle simulation; and the local-light budget.
            app.add_plugins((
                crate::sky::SkyPlugin,
                crate::water::WaterPlugin,
                crate::water_exclusion::WaterExclusionPlugin,
                crate::water_scene_depth::WaterSceneDepthPlugin,
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
            // The reference viewer's `RenderResolutionDivisor`: renders the 3D
            // world into an image 1/n the size of the window and stretches it
            // back over the view, leaving the interface at full resolution.
            // Inert at the default divisor of 1, which is why it can sit here
            // unconditionally.
            app.add_plugins(ResolutionDivisorPlugin);
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
            // GPU and writes the skin palettes into Bevy's SkinUniforms buffer. It
            // is the only path: Phase 4 removed the CPU joint entities, so a
            // device without compute + storage buffers renders avatars at their
            // bind pose rather than falling back. There is no path-selecting env
            // knob any more (the old SL_VIEWER_GPU_AVATARS `cpu`/`off`/`ghost`
            // went with the scaffolding); `SL_VIEWER_GPU_AVATARS_READBACK=1`, the
            // one knob left, only turns on the palette readback + verdict log and
            // is read once here.
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
pub(crate) struct ViewerWorldPlugins {
    /// Which pick resolver answers the cursor-pick queue.
    pub(crate) pick: PickStack,
}

/// Which resolver [`ViewerWorldPlugins`] installs for the cursor-pick queue —
/// exactly one, because whichever runs first drains it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PickStack {
    /// The GPU ID-buffer rasteriser: the viewer.
    #[default]
    Gpu,
    /// The `MeshRayCast` double: the headless fixture world.
    #[cfg(test)]
    Cpu,
}

impl ViewerWorldPlugins {
    /// The world fold with the CPU pick resolver — the headless fixture
    /// world's configuration.
    #[cfg(test)]
    pub(crate) const fn cpu_pick() -> Self {
        Self {
            pick: PickStack::Cpu,
        }
    }
}

impl Plugin for ViewerWorldPlugins {
    fn build(&self, app: &mut App) {
        // The three world layers, each owning its own stores, messages, systems
        // and ordering edges. What is left in this group is what no single layer
        // owns: the HUD screen, the pie menus over the world, the pick
        // resolver, the physics foundation and the raycast index it feeds.
        app.add_plugins((
            sl_viewer_world_objects::WorldObjectsPlugin,
            sl_viewer_world_scene::WorldScenePlugin,
            sl_viewer_world_avatar::WorldAvatarPlugin,
        ));
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
        // The mute list, which is not world state but is what answers the Block
        // slice of the three pies above: every Block affordance writes a
        // `RequestBlock` and this is the one guarded place it becomes a
        // `Command::Mute`. A world without it has pies whose Block slice does
        // nothing, so the group that hosts the pies brings it.
        app.add_plugins(crate::mutes::MutesPlugin);
        // GPU ID-buffer picking (Phase 3): the cursor pick is a render, not a
        // ray cast — pixel-perfect against exactly what is drawn, GPU-posed
        // avatars included. The headless fixture world swaps in the CPU
        // resolver — same registry, same channel, a `MeshRayCast` instead of
        // the rasteriser ([[viewer-cpu-pick-resolver]]).
        match self.pick {
            PickStack::Gpu => app.add_plugins(crate::gpu_pick::GpuPickPlugin),
            #[cfg(test)]
            PickStack::Cpu => app.add_plugins(crate::gpu_pick::CpuPickResolverPlugin),
        };
        // The client-side physics foundation (P31.1): server-authoritative prim /
        // avatar dead-reckoning and collision-geometry building (no physics engine —
        // the viewer simulates nothing). Feeds the custom raycast index below.
        app.add_plugins(PhysicsPlugin);
        // The custom off-thread static raycast index (viewer-perf-custom-static-raycast-index):
        // a parry BVH over the prim colliders, maintained on a background task and
        // queried lock-free for camera collision — the replacement for avian's
        // per-fixed-step `SpatialQuery` maintenance.
        app.add_plugins(crate::raycast_index::RaycastIndexPlugin);
        // The skin-attribute / `SkinnedMesh` agreement, checked in the main
        // world on whatever just changed — after every system above that spawns
        // or re-meshes a skinned entity, and so before the extract that would
        // hand a mismatch to wgpu. It stays in the binary rather than moving
        // into the avatar layer because it is a check over everything *any*
        // crate spawned, and its static twin (`crate::render_test`'s
        // `unskinned_violations`) lives here too. See `crate::skin_agreement`
        // for why the two halves can disagree at all and why one bad entity
        // takes a whole batched draw down with it.
        app.add_systems(PostUpdate, crate::skin_agreement::assert_skin_agreement);
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
                pick_and_touch,
                // The world half of the touch resolves on the GPU pick's
                // readback, 1–2 frames after the press.
                crate::hud_pick::resolve_touch_pick,
            )
                // The gate is the build tool's, which lives in a crate above this
                // one; the binary is where the two meet.
                .run_if(crate::edit_tool::edit_tool_inactive),
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
        // The scaffold the asset editors share: the chrome their windows are
        // made of, and the guard that asks before a close throws unsaved work
        // away (viewer-audit-asset-editor-scaffold).
        app.add_plugins(crate::asset_editor::AssetEditorScaffoldPlugin);
        // The rich-text field (viewer-notecard-inline-items): the text widget
        // whose flow carries objects — the notecard body's embedded items — and
        // whose ranges can be styled. Registered before its consumers so a
        // field spawned at start-up already has its systems.
        app.add_plugins(sl_viewer_ui_widgets::ui_rich_text::RichTextPlugin);
        // The notecard viewer & editor floater (viewer-notecard-editor): open a
        // notecard from inventory, read it, edit its text when the item is
        // modifiable, and save it back to agent inventory. Its embedded items
        // are drawn inline in the body, whether it is being read or written.
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
        // The Land tool (viewer-terrain-edit-brushes / viewer-parcel-join-split):
        // the ground drag-select, the six terraform brushes, and the parcel
        // subdivide / join the selected rectangle feeds.
        app.add_plugins(crate::edit_land::EditLandPlugin);
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

/// The interface and every feature surface: the UI scaffold, skin and i18n,
/// the widgets, the floater manager and every window it hosts, the menus, the
/// toolbars, the notification host and its toasts, the chat, social, inventory,
/// places, environment, RLV, preferences and media surfaces, and the features
/// whose state those windows show. Everything here is logic and layout; what
/// touches the host machine is [`ViewerShellPlugins`].
///
/// Added **before** the other groups: [`ViewerWorldPlugins`]' pie menus need
/// the scaffold, and a few plugins here add a widget plugin its later users
/// only add when it is missing (the trackball, the combo, the text input).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ViewerUiPlugins;

impl Plugin for ViewerUiPlugins {
    fn build(&self, app: &mut App) {
        app
            // The viewer UI scaffold (viewer-ui-widget-scaffold): the `bevy_ui` +
            // `bevy_ui_widgets` + `bevy_input_focus` bring-up, the one `UiRoot` every
            // panel parents itself to, tab navigation, the bundled font stack, and the
            // direction-neutral / content-driven layout conventions the whole UI cluster
            // inherits.
            .add_plugins(ViewerUiPlugin)
            // The UI skin / design-token system (viewer-ui-skin-tokens): stands up the
            // `bevy_flair` CSS engine, registers the logical box / corner properties (so
            // skins author `margin-inline-start`, never physical `left`), and dresses the
            // `UiRoot` in the selected skin's hot-reloadable `.css` tokens. After
            // `ViewerUiPlugin` so the `UiRoot` it styles already exists.
            .add_plugins(crate::skin::ViewerSkinPlugin)
            .add_plugins(crate::skin_colors::SkinColorsPlugin)
            // The i18n foundation (viewer-i18n-fluent-scaffold): Project Fluent `.ftl`
            // bundles behind Bevy assets with runtime locale switching, the `Translator`
            // string-lookup API (typed named arguments → per-locale plural / gender), and
            // the `UiLocale` resource carrying the locale's LTR/RTL direction and
            // typographic conventions (the tab widget's truncation ellipsis). Ahead of
            // every UI-bearing panel so panels are authored translatable from day one.
            .add_plugins(ViewerI18nPlugin)
            // The reusable tab widget's runtime half (viewer-ui-tab-widget): reflects a
            // resizable strip's persisted / dragged width onto its node.
            .add_plugins(TabWidgetPlugin)
            // The reusable table widget's runtime half (viewer-ui-table-widget): column
            // width sync + resize, locale-ellipsis reveal, sort-arrow drive, and the
            // per-table sort / column-width settings seed + persist.
            .add_plugins(TableWidgetPlugin)
            // The reusable clickable name-link widget (viewer-clickable-name-widgets):
            // resolves an avatar / group / owner name against the caches, keeps the
            // label + link tint in step, and opens the right profile on click.
            .add_plugins(crate::ui_name_link::NameLinkPlugin)
            // The shared URL-linkification widget (viewer-url-linkification): renders text
            // with clickable http(s) / SLURL / secondlife:///app links, resolves agent /
            // group / parcel names in place, shows the target URL on hover, and opens web
            // links. The parcel-name cache feeds the parcel-link labels.
            .add_plugins(crate::parcel_names::ParcelNamesPlugin)
            .add_plugins(crate::linkified_text::LinkifiedTextPlugin)
            // Routes a clicked / command-line SLURL to its handler (profile, IM,
            // teleport, world map): viewer-slurl-parse-dispatch.
            .add_plugins(crate::slurl_dispatch::SlurlDispatchPlugin)
            // The self-dismissing avatar / object inspector mini-popups opened from a
            // clicked `.../inspect` / objectim link: viewer-inspector-popups.
            .add_plugins(crate::inspector_popup::InspectorPopupPlugin)
            // The reusable radio-widget's runtime half (viewer-ui-radio-widget): keeps
            // each option's `Checked` marker and indicator glyph reconciled to the
            // group's selection, so a click and an external write (the Build Tools
            // floater's tool sync) drive the same visual path.
            .add_plugins(crate::ui_radio::RadioWidgetPlugin)
            // The sun / moon trackball's drawing half (viewer-ui-virtual-trackball):
            // places each marker from the aim its window wrote and paints the
            // below-horizon state. The environment editors' `RowsPlugin` adds it too,
            // guarded, so a host that takes only those windows still draws them.
            .add_plugins(crate::ui_trackball::TrackballPlugin)
            // The reusable combo / dropdown widget (viewer-ui-combo-widget): the closed
            // value reconcile, the ComboChanged message, and the outside-press dismiss.
            .add_plugins(crate::ui_combo::ComboWidgetPlugin)
            // The reusable colour-picker floater + swatch (viewer-ui-color-picker): the
            // OpenColorPicker / ColorPicked messages, the RGB-slider floater, and the
            // swatch fill reconcile.
            .add_plugins(crate::ui_color_picker::ColorPickerPlugin)
            // The reusable texture-picker floater + swatch (viewer-ui-texture-picker):
            // the OpenTexturePicker / TexturePicked messages, the inventory thumbnail
            // grid floater, and the swatch thumbnail reconcile.
            .add_plugins(crate::ui_texture_picker::TexturePickerPlugin)
            // The reusable text-input widget's runtime half (viewer-ui-text-input-widget):
            // the whole-string numeric validator that reverts a field to its last valid
            // value when an edit makes it structurally invalid (a second '.', a misplaced
            // '-') — the part `EditableTextFilter`'s per-character check cannot express.
            .add_plugins(TextInputPlugin)
            // The reusable search-field widget's runtime half (viewer-ui-search-field):
            // the clear-button / placeholder visibility and clear-on-Escape, shared by the
            // menu-bar and inventory search boxes.
            .add_plugins(crate::ui_search::SearchFieldPlugin)
            // The two-way widget↔settings binding (viewer-ui-settings-binding): the
            // `control_name=` idiom — a checkbox / slider names the setting it edits and
            // the store and widget are kept in sync both ways. Also owns the `F7` demo.
            .add_plugins(SettingsBindingPlugin)
            // The Stand Up / Stop flycam state button in the bottom toolbar's reserved
            // slot (viewer-sit-target-and-stand-button): Stand while seated, Stop flycam
            // while in flycam.
            .add_plugins(StandStopButtonPlugin)
            // The Spawn crowd debug button (SL_VIEWER_CROWD): only present while a
            // synthetic crowd is armed, hands the user the manual capture trigger.
            .add_plugins(crate::crowd_debug_button::CrowdDebugButtonPlugin)
            .add_plugins(crate::teleport_progress::TeleportProgressPlugin)
            .add_plugins(crate::double_click_teleport::DoubleClickTeleportPlugin)
            // The line-based menu widget (viewer-ui-context-menu) + reusable menu bar
            // (viewer-ui-menu-bar): drop-down / context menus and the strip of buttons
            // that open them, built on `bevy_ui_widgets`' headless menu machinery. The
            // mechanism only — which entries a menu holds is per-domain (the live top
            // bar is `crate::menu_bar`, gear menus belong to their window).
            .add_plugins(crate::menu::MenuWidgetPlugin)
            // The virtualized (windowed-recycling) list widget (viewer-ui-virtualized-list):
            // a bounded row pool that recycles as the viewport scrolls, so a long panel
            // (inventory, radar, chat at scale) costs the viewport, not the item count.
            .add_plugins(VirtualListPlugin)
            // The floater window manager (viewer-ui-floater-basic / -resize-dock): the
            // draggable, raise-on-click, closable title-bar window — plus resize, minimize
            // and dock / tear-off — every panel hangs off. Spawns a trailing-edge dock host.
            // The inventory window (below) is its first live consumer.
            .add_plugins(FloaterPlugin)
            // The inventory window (viewer-inventory-folder-tree / -outfit-tab /
            // -search-filter): the folder tree, the Everything / Recent / Worn tabs and the
            // search bar, on the high-level inventory bridge, toggled with `Ctrl+I`. Hosted
            // in a floater, so it drags / resizes / minimizes / docks.
            .add_plugins(InventoryPlugin)
            .add_plugins(InventoryActionsPlugin)
            .add_plugins(InventoryDragPlugin)
            .add_plugins(InventoryFiltersPlugin)
            .add_plugins(InventoryGalleryPlugin)
            .add_plugins(InventoryPropertiesPlugin)
            // The settings-asset index (viewer-environment-settings-index): every sky /
            // water / day-cycle item the mirror holds, grouped by kind and addressable
            // by name — what `@setenv_preset:<name>` resolves against, and what the
            // environment pickers list. Needs InventoryPlugin's model, so it follows it.
            .add_plugins(SettingsIndexPlugin)
            .add_plugins(AboutLandmarkPlugin)
            .add_plugins(AvatarPickerPlugin)
            // The group picker (viewer-region-estate-group-picker): the chooser behind
            // every set-group control — About Land's group, the build tool's, and the
            // estate's allowed-groups Add. Lists the agent's memberships (GroupsModel,
            // whose GroupsPlugin follows) and, where the caller can use one, searches
            // the directory for a group the agent is not in.
            .add_plugins(GroupPickerPlugin)
            // The avatar profile floater (viewer-social-profiles): 2nd Life / Web /
            // Picks / Classifieds / 1st Life / Notes, opened from the avatar pie's
            // Profile slice and the People list, editable for one's own profile.
            .add_plugins(AvatarProfilePlugin)
            // The embedded-browser UI widget (LLMediaCtrl): surface-backed image
            // nodes with click-to-focus pointer / keyboard routing.
            .add_plugins(crate::browser_widget::BrowserWidgetPlugin)
            // The in-viewer web browser floater (floater_web_content): navigation
            // toolbar + browser view + status row, opened from Content ▸ Web Browser.
            .add_plugins(crate::web_floater::WebFloaterPlugin)
            // The minimap ("net map") floater: terrain / object / parcel layers,
            // avatar dots, frustum wedge, double-click teleport and context menu.
            .add_plugins(crate::minimap::MinimapPlugin)
            // The world-map floater: grid-wide tile imagery (shared sl-map-apis
            // fetch / cache), per-region info + item markers, region-name search.
            .add_plugins(crate::world_map::WorldMapPlugin)
            // The Search floater: the protocol-backed legacy directory search
            // (people / groups / events / places / land / classifieds).
            .add_plugins(crate::search::SearchFloaterPlugin)
            // Media-on-a-prim (LLViewerMedia / LLViewerMediaFocus): ObjectMedia data
            // driving per-face surfaces, world input routing and the focus model.
            .add_plugins(crate::media_prim::MediaPrimPlugin)
            // The floating media controls bar above the media face under the cursor
            // (LLPanelPrimMediaControls).
            .add_plugins(crate::media_controls::MediaControlsPlugin)
            // The Nearby Media window (LLPanelNearByMedia): the parcel stream and every
            // media face around the agent, each with its own controls.
            .add_plugins(crate::nearby_media::NearbyMediaPlugin)
            // Parcel streaming audio (viewer-streaming-audio): the GStreamer radio
            // stream following the agent's parcel, with its bottom-bar controls.
            .add_plugins(crate::parcel_audio::ParcelAudioPlugin)
            .add_plugins(crate::volume_panel::VolumePanelPlugin)
            // The emoji-picker floater (viewer-emoji-picker-floater): a grouped,
            // searchable grid of emoji in a floater, toggled with `Ctrl+E`; clicking a
            // glyph inserts it into the text field the picker last saw focused. On the
            // emoji dataset (`sl-emoji`), the search-field / tab / virtualized-list
            // widgets and the floater manager. After the floater plugin (its host) and
            // the inventory plugin (a search-field consumer it shares systems with).
            .add_plugins(EmojiPickerPlugin)
            // The inline `:`-emoji completer (viewer-emoji-colon-autocomplete): a popup of
            // matching short-codes on a field's trailing `:token`. Defines the
            // `ColonCompleteSet` the chat input's Enter-to-send orders after.
            .add_plugins(ColonCompletePlugin)
            // The reusable chat-input widget (viewer-ui-text-input-emoji): a single-line
            // field with an emoji button (opens the picker for it) and the `:`-completer,
            // emitting a submit event. The base every chat surface is built on.
            .add_plugins(ChatInputPlugin)
            // The reusable local-chat-input widget (viewer-chat-channel-and-commands): the
            // chat input plus a whisper/say/shout select box, `/N` channel routing,
            // Shift/Ctrl+Enter volume overrides and the `/command` registry. Emits a
            // structured submission; the live nearby-chat bar and conversations floater
            // (each a follow-up) are its consumers.
            .add_plugins(LocalChatInputPlugin)
            // The live top menu bar (viewer-ui-menu-bar): the strip of pull-down menu
            // names at the top of the screen, on `crate::menu`'s widget. After the
            // inventory plugin so the Avatar ▸ Inventory entry can toggle its window.
            .add_plugins(crate::menu_bar::TopMenuBarPlugin)
            // Menu search (viewer-ui-menu-search): a text field in the bar (after the last
            // menu) whose term drives `crate::menu`'s `MenuFilter`, so opening a menu shows
            // only the matching entries. After the top-menu plugin, which spawns the field.
            .add_plugins(crate::menu_search::MenuSearchPlugin)
            // The status area (viewer-ui-status-bar): the parcel permission icons,
            // region / parcel / position, L$ balance, SLT time and FPS read-outs that
            // share the top row, hugging its trailing edge next to the menu bar.
            .add_plugins(crate::status_bar::StatusBarPlugin)
            // The toast / notification host (viewer-ui-notification-host): the screen
            // channel that stacks, times out, fades and dismisses transient
            // notifications from the declarative catalogue, plus the modal-alert scrim —
            // the shared substrate the specific dialogs sit in.
            .add_plugins(NotificationHostPlugin)
            // The live sources that raise into it — simulator alerts, failed commands,
            // protocol diagnostics and the demo spread. A separate plugin because all
            // of them read the session, which the host deliberately does not (so the
            // login-free gallery can still host toast specimens).
            .add_plugins(NotificationSourcesPlugin)
            // The bottom toolbar (viewer-ui-bottom-toolbar): the persistent strip of
            // toggle buttons that open the main floaters (Inventory wired today, the rest
            // disabled placeholders until their tasks land), and the bottom-area layout
            // host the nearby-chat / audio / voice / quick-preferences controls hang off.
            // After the inventory plugin so its Inventory toggle can reach the window.
            .add_plugins(crate::bottom_toolbar::BottomToolbarPlugin)
            // The live nearby-chat bar (viewer-chat-input-bar): the local-chat-input
            // widget placed in the bottom-area upper stack (above the button bar), sending
            // its LocalChatSubmit as Command::Chat, driving the typing animation, and
            // focused by Enter. The bottom toolbar's leading chat button toggles it. After
            // the toolbar (whose BottomArea it fills) and the local-chat-input plugin.
            .add_plugins(NearbyChatBarPlugin)
            // The Conversations floater (viewer-social-im-conversations): one window with
            // vertical tabs for nearby chat, 1:1 IMs, group chats and conferences, each a
            // transcript pane plus its chat input. After the chat-input / local-chat-input
            // plugins whose widgets it hosts, and the floater manager.
            .add_plugins(ConversationsPlugin)
            // The People / Contacts surface (viewer-social-people-panel): the Friends
            // list hosted as a pinned tab inside the Conversations floater. After
            // ConversationsPlugin, whose strip / panel area it adds its tab and pane into.
            .add_plugins(PeoplePlugin)
            .add_plugins(crate::radar::RadarPlugin)
            // The Groups list (viewer-social-groups): the member's own groups, built into
            // the Groups sub-tab of the People pane. After PeoplePlugin, whose Groups
            // content slot it fills.
            .add_plugins(GroupsPlugin)
            // The Blocked Residents & Objects list (viewer-block-list): the mute list
            // built into the Blocked sub-tab of the People pane, plus the by-name block
            // floater. After PeoplePlugin, whose Blocked content slot it fills.
            .add_plugins(BlockedPlugin)
            // Contact sets (viewer-contact-sets): the client-side named, coloured groups
            // of residents, their per-account store, and the Contact Sets sub-tab of the
            // People pane (plus the add-to-set and set-settings floaters). After
            // PeoplePlugin, whose Contact Sets content slot the panel fills.
            .add_plugins(crate::contact_sets::ContactSetsPlugin)
            .add_plugins(crate::contact_sets_panel::ContactSetsPanelPlugin)
            // Avatar complexity limiting (viewer-avatar-complexity-limit): score what each
            // nearby avatar costs to draw and, past the budget, draw them as a flat
            // jellydoll instead of their attachments. Its systems bracket the scene mirror
            // and the avatar bake / visibility passes through explicit edges.
            .add_plugins(crate::avatar_complexity::AvatarComplexityPlugin)
            // The standing per-avatar render exceptions
            // (viewer-avatar-render-settings-manager): the persisted "always draw this
            // person in full" / "never draw them in full" decisions the complexity
            // limit obeys above its own rules, and the floater that manages them.
            // Before AvatarComplexityPlugin's mirror by explicit edge.
            .add_plugins(crate::avatar_render_settings::AvatarRenderSettingsPlugin)
            .add_plugins(crate::avatar_render_floater::AvatarRenderFloaterPlugin)
            // Derender + asset blacklist (viewer-derender-blacklist): the client-side
            // suppression of an object / avatar the user does not want to see, its
            // per-avatar persisted blacklist, and the scene purge. Its systems bracket
            // the scene mirror (before the ingest, after the fold) via explicit edges.
            .add_plugins(DerenderPlugin)
            // The Asset Blacklist floater (viewer-derender-blacklist): the list of what
            // this avatar has derendered, with Re-render / Clear temporary. After
            // DerenderPlugin, whose list it presents.
            .add_plugins(AssetBlacklistPlugin)
            // The RLVa control surface (viewer-rlva-floaters-toggles): the console,
            // the restrictions / locks / strings windows, and the RLVa menu's toggles.
            // They read the one RlvSession the world-API tier holds, so they can go
            // anywhere after it is initialised.
            // The environment editors (viewer-environment-personal-lighting): the
            // Personal Lighting window and the local sky / water override it writes.
            // After the environment state exists, which the scene tier initialises.
            .add_plugins(sl_viewer_environment::EnvironmentUiPlugins)
            .add_plugins(sl_viewer_rlv::RlvUiPlugins)
            // The RLV command intake (viewer-rlv-command-intake): the owner-say gate a
            // worn collar speaks through, the one seam every `@get*` / `@notify` answer
            // is shouted back by, and the pass that lifts a vanished object's
            // restrictions. Separate from the windows above because it is the wiring
            // that makes the engine reachable at all rather than a surface that draws
            // it; after them, since it fills the console they show.
            .add_plugins(sl_viewer_rlv::intake::RlvIntakePlugin)
            .add_plugins(GroupProfilePlugin)
            // The group-notice toast host (viewer-group-notice-display): pops a card —
            // group image, subject, body and any attached item — when a group posts a
            // notice, mirroring the reference LLToastGroupNotifyPanel. After
            // GroupProfilePlugin (whose RequestedGroupNotices it reads to suppress a
            // toast for a notice the Notices tab pulled up itself) and GroupsPlugin
            // (whose membership insignia it shows).
            .add_plugins(GroupNoticePlugin)
            // The script-dialog toast host (viewer-dialog-lldialog): pops a card — object
            // / owner title, message, and a button grid or a text field — when a scripted
            // object calls llDialog / llTextBox, wiring the reply on the hidden chat
            // channel (Command::ReplyScriptDialog). After NotificationHostPlugin, whose
            // shared channel it adopts its card into.
            .add_plugins(ScriptDialogPlugin)
            // The script web-page request toast host (viewer-dialog-script-load-url):
            // pops a card — heading, object / owner title, message and the target URL —
            // when a scripted object calls llLoadURL (the LoadURL message), with Load
            // (open the URL in the embedded browser), Block (mute) and Ignore actions.
            // After NotificationHostPlugin (whose shared channel it adopts its card into)
            // and WebFloaterPlugin (whose OpenWebBrowser message Load writes).
            .add_plugins(LoadUrlPlugin)
            // The script permission-request toast host (viewer-permission-request-dialog):
            // pops a card — object / owner, the requested permission bits, Yes / No /
            // Block (or the money-access caution card with Allow access / Deny) — when a
            // scripted object calls llRequestPermissions (the ScriptQuestion message),
            // wiring the grant / deny reply (Command::AnswerScriptPermissions). After
            // NotificationHostPlugin, whose shared channel it adopts its card into.
            .add_plugins(ScriptPermissionPlugin)
            // The experience-acceptance toast host (viewer-experience-permission-dialog):
            // pops the reference ScriptQuestionExperience card — object / owner, the
            // experience name / scope, the requested permission bits, Yes / No / Block
            // Experience / Block Object — when a scripted object requests to run under an
            // experience (a ScriptQuestion carrying an Experience id), admitting or
            // blocking the experience (Command::SetExperiencePermission) alongside the
            // grant / deny reply. After ScriptPermissionPlugin (which skips the experience
            // requests this host owns) and NotificationHostPlugin (whose shared channel it
            // adopts its card into).
            .add_plugins(ExperiencePermissionPlugin)
            // The Experiences floater (viewer-experiences-floater): the manage surface's
            // seven tabs -- search, the agent's allowed / blocked / admin / contributor
            // / owned lists, and the event log -- over the experience caps. After
            // FloaterPlugin, whose spawn_floater it builds on.
            .add_plugins(ExperiencesPlugin)
            // One experience's own page (viewer-experiences-floater): a keyed window per
            // experience, carrying the metadata, the allow / forget / block actions and
            // -- for an administrator -- the editable fields. After ExperiencesPlugin,
            // whose lists and search results open it.
            .add_plugins(ExperienceProfilePlugin)
            // The reusable "Choose Experience" picker (viewer-region-experiences-panel):
            // the window every estate experience list's Add opens, answering an
            // OpenExperiencePicker with an ExperiencePicked. After ExperiencesPlugin,
            // whose persisted search-rating setting it shares, and ExperienceProfilePlugin,
            // whose window its View Profile button opens.
            .add_plugins(ExperiencePickerPlugin)
            // The experience event log (viewer-experience-event-stream): the per-account
            // record of what the experiences the agent joined actually did to them, the
            // only signal in the protocol that reports an experience attachment, and the
            // producer of the ExperienceEvent / ExperienceEventAttachment toasts. Before
            // ExperiencesPlugin would read it is unnecessary -- the floater's Events
            // section reads the resource, which exists from plugin build.
            .add_plugins(ExperienceLogPlugin)
            // The offers & invites toast host (viewer-dialog-offers-invites): pops an
            // accept / decline card when the grid throws an inventory offer, a teleport
            // lure, a friendship offer or a group-membership invitation over IM, wiring
            // each to its protocol reply (AcceptInventoryOffer / AcceptTeleportLure /
            // AcceptFriendship / AcceptGroupInvitation and the matching declines). After
            // NotificationHostPlugin, whose shared channel it adopts its card into, and
            // InventoryPlugin, whose folders the accept replies file into.
            .add_plugins(OffersInvitesPlugin)
            // The friendship-offer path (viewer-add-friend-offers-silently): the one
            // prompted way an Add Friend affordance — the avatar pie, the radar, the
            // minimap, the profile, the inspector, search, a secondlife:///…/requestfriend
            // link — reaches the wire, asking for the offer's message the way the
            // reference's AddFriendWithMessage dialog does, refusing self-friendship and
            // confirming what was sent. After NotificationHostPlugin, whose dialog it
            // raises and whose answer it reads.
            .add_plugins(crate::add_friend::AddFriendPlugin)
            // The presence modes (viewer-do-not-disturb-away): Away / auto-AFK, Do Not
            // Disturb and the two autorespond modes, their signalled-animation wire
            // writes, and the canned IM replies they send. After the conversations
            // plugin, whose ingest the auto-reply orders itself ahead of.
            .add_plugins(crate::presence::PresencePlugin)
            // The About Land floater (viewer-parcel-options-general): the parcel's
            // General / Covenant / Objects tabs. Subject-bound, persistence-exempt;
            // opened from the top-bar location read-out and the land pie.
            .add_plugins(AboutLandPlugin)
            // The Region / Estate floater (viewer-region-options-debug / -general /
            // -terrain / -estate): the region-and-estate info surface. Bound to the
            // current region, persistence-exempt; opened from the World menu.
            .add_plugins(AboutFloaterPlugin)
            .add_plugins(AboutRegionPlugin)
            // The snapshot floater (viewer-snapshot-floater): a framed live world preview
            // (a second off-screen camera into an image) with resolution / format
            // selection and a save-to-disk destination that echoes the path to chat.
            // Opened from the bottom toolbar's Snapshot button.
            .add_plugins(crate::snapshot_floater::SnapshotFloaterPlugin)
            // The 360-degree snapshot floater (viewer-360-snapshot): a capture renderer
            // of its own -- six cube-map faces shot from the camera's eye point with the
            // viewer camera itself, reprojected into an equirectangular panorama and
            // written with the GPano metadata that makes it open as a sphere. Opened
            // from World > Photo and Video.
            .add_plugins(crate::panorama::PanoramaPlugin)
            // The Preferences floater shell (viewer-preferences-floater): the tabbed
            // settings window over the typed store — snapshot on open, revert on
            // Cancel / close, persist on OK, with the cross-tab search filter. The
            // per-tab tasks plug their panels into its registry. After FloaterPlugin,
            // whose spawn_floater and deferred-content build it rides.
            .add_plugins(crate::preferences::PreferencesPlugin)
            // The raw debug-settings editor (viewer-preferences-debug-settings-editor):
            // a separate floater over *every* registered setting — searchable list,
            // per-kind detail editor, per-scope override editing. Live edits, no
            // OK / Cancel snapshot. After FloaterPlugin, whose spawn_floater and
            // deferred-content build it rides.
            .add_plugins(crate::debug_settings::DebugSettingsPlugin)
            // The Quick Preferences panel (viewer-quick-preferences): the small
            // bottom-right floater of the settings reached-for hourly (draw distance,
            // particle cap, environment preset + time of day), a curated view over the
            // typed store. Opened from a gear button in the bottom toolbar's trailing
            // area. After FloaterPlugin (its spawn_floater / deferred-content build) and
            // the bottom toolbar (its BottomArea host).
            .add_plugins(crate::quick_preferences::QuickPreferencesPlugin)
            .add_plugins(crate::quick_prefs_environment::QuickPrefsEnvironmentPlugin)
            // Phototools (viewer-phototools): the photographer's window — the
            // environment on one tab and the render knobs that change the *look* on the
            // others, a second curated view over the same store the graphics tab binds.
            // Opened from World ▸ Photo and Video ▸ Phototools (Alt+P).
            .add_plugins(crate::phototools::PhototoolsPlugin)
            // The alerts tab's popup list (viewer-preferences-alerts-tab): the model
            // refresh, row pool and binding behind the panel build_alerts_tab plugs
            // into the shell's registry.
            .add_plugins(crate::preferences_alerts::PreferencesAlertsPlugin)
            // The general tab's appliers (viewer-preferences-general-tab): the live
            // UI-scale write and the maturity-preference server conversation behind
            // the panel build_general_tab plugs into the shell's registry.
            .add_plugins(crate::preferences_general::PreferencesGeneralPlugin)
            .add_plugins(crate::preferences_graphics::PreferencesGraphicsPlugin)
            // The audio tab's live output-device re-enumeration
            // (viewer-preferences-audio-tab); the tab content itself plugs into the
            // shell's registry.
            .add_plugins(crate::preferences_audio::PreferencesAudioPlugin)
            // The chat / IM + privacy tab's runtime side
            // (viewer-preferences-chat-privacy-tab): the login-time chat-log
            // configuration push, the `UserInfo` request / seed pair, and the per-OK
            // apply hook; the tab content itself plugs into the shell's registry.
            .add_plugins(crate::preferences_chat::PreferencesChatPlugin)
            // The camera & movement tab's runtime side
            // (viewer-preferences-camera-move-tab): the per-frame CameraTuning /
            // MovementTuning refreshes and the field-of-view / mouselook-avatar
            // appliers; the tab content itself plugs into the shell's registry.
            .add_plugins(crate::preferences_camera_move::PreferencesCameraMovePlugin)
            .add_plugins(crate::preferences_colors_skins::PreferencesColorsSkinsPlugin)
            .add_plugins(crate::preferences_network_cache::PreferencesNetworkCachePlugin)
            // In-world hover tooltips over objects / avatars / land (viewer-hover-tooltips).
            .add_plugins(crate::hover_tooltip::HoverTooltipPlugin)
            // The `F3` pipeline-status overlay and the asset-store statistics it and the
            // Tracy plots read.
            .add_plugins((
                crate::diagnostics::PipelineOverlayPlugin,
                crate::asset_stats::AssetStatsPlugin,
                crate::avatar_asset_stats::AvatarAssetStatsPlugin,
            ))
            // Gate bevy_ui's unconditional full-tree stack rebuild and layout walk
            // behind "did any of that system's inputs actually change (visibly)"
            // (viewer-perf-ui-layout-per-frame-relayout), and bring the env-gated
            // skip-rate meter that says whether the gate is behaving with it.
            // `SL_VIEWER_LOG_UI_DIRTY=1` names what tripped it per frame.
            .add_plugins(crate::ui_perf::UiLayoutGatePlugin)
            // The on-screen nearby-chat overlay, and the two demo panels the
            // screenshot harness captures (`SL_VIEWER_TEXT_DEMO`, F4;
            // `SL_VIEWER_TEXT_INPUT_DEMO`, F8).
            .add_plugins((
                crate::chat::ChatOverlayPlugin,
                crate::ui_text::TextDemoPlugin,
                crate::ui_text_input::TextInputDemoPlugin,
            ));
    }
}

/// What the viewer needs of the machine it runs on: the OS clipboard and file
/// dialog, the audio device and the sound producers that feed it, the web and
/// video engines and the website login, the files notification and floater
/// state persist to, the settings store's write-back, the diagnostics
/// instruments and the avatar-state capture. After [`ViewerUiPlugins`], whose
/// surfaces are the consumers of all of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewerShellPlugins {
    /// Whether the audio device is opened. Off, the mixer does not exist and
    /// every producer — each guards on it — plays nothing: a test process must
    /// not reach for the machine's speakers.
    pub(crate) audio_device: bool,
    /// Which media engines may start, and whether the website login runs.
    pub(crate) media: MediaRuntime,
    /// Whether the viewer has no display (any mode but a real window), so the
    /// media engines must not look for one.
    pub(crate) headless: bool,
}

impl Plugin for ViewerShellPlugins {
    fn build(&self, app: &mut App) {
        // The host's file-open dialog (the XDG FileChooser portal on Linux), for
        // every "… from disk": the settings editors' Import today, the uploaders
        // when they land.
        app.add_plugins(crate::file_dialog::FileDialogPlugin);
        if self.audio_device {
            app.add_plugins(crate::audio::AudioPlugin);
        }
        app
            // The shared sound-asset fetch/decode/cache (viewer-in-world-sounds,
            // viewer-ui-sound-effects) and the in-world spatial-sound producer that
            // feeds the mixer's Sfx bus (llTriggerSound one-shots + attached sounds).
            .add_plugins(crate::sound_cache::SoundCachePlugin)
            .add_plugins(crate::world_sounds::WorldSoundsPlugin)
            // The viewer's own 2-D UI feedback sounds on the mixer's UI bus
            // (viewer-ui-sound-effects): the typing chirp, money up/down, teleport,
            // snapshot shutter — raised as PlayUiSound messages by their surfaces.
            .add_plugins(crate::ui_sounds::UiSoundsPlugin)
            // The web-media engine (viewer-media-prim-browser): offscreen Chromium
            // (sl-cef) pumped on the main thread, one surface per embedded page. The
            // consumers (browser widget / floater, media-on-a-prim, controls bar)
            // all no-op when it is disabled or failed to start.
            .add_plugins(crate::media_engine::MediaEnginePlugin {
                enabled: self.media.web,
                video_enabled: self.media.video,
                headless: self.headless,
            })
            // The Second Life website auto-login (viewer-web-openid-auth): at login,
            // POST the login response's OpenID token off-thread and inject the reply's
            // session cookie into the shared browser context, so the web surfaces
            // open already signed in. No-op off Second Life or with `--no-web-auth`.
            .add_plugins(crate::web_auth::WebAuthPlugin {
                enabled: self.media.web && self.media.web_auth,
            })
            // Recovers the real DNS / TCP / TLS / HTTP reason a media stream failed,
            // which GStreamer's souphttpsrc hides — shared by the parcel-audio and
            // media-on-a-prim consumers.
            .add_plugins(crate::media_diagnostics::MediaDiagnosticsPlugin)
            // The persistent-notification store (viewer-notification-persistence): saves
            // the open (unacknowledged) sticky notifications to a per-account file and
            // re-displays them on next login (the reference LLPersistentNotificationStorage).
            // After the host, whose PersistNotification / NotificationResponse it records.
            .add_plugins(NotificationPersistPlugin)
            // Per-user floater geometry (viewer-ui-floater-persist-geometry): remember
            // each floater's position, size, minimized / docked state and open / closed
            // state across sessions, in the per-avatar account settings.
            .add_plugins(FloaterPersistPlugin)
            // The settings store's write-back and its account-scope loader. The store
            // itself is the builder's to insert: it decides which files, if any, the
            // settings are read from and written to.
            .add_plugins(SettingsPersistPlugin)
            // Avatar-state capture (viewer-avatar-state-dump-replay), which adds
            // nothing at all unless `SL_VIEWER_DUMP_DIR` is set.
            .add_plugins(crate::avatar_dump::AvatarDumpPlugin)
            // Frame-time / FPS instruments — the smoothed FPS the status area
            // (`crate::status_bar`) shows and the frame budget the fetch/decode pipeline
            // work is watched against.
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            // Live entity count — cheap, and (via `tracy_plots`) plotted over time so a
            // Tracy capture shows how per-frame system cost tracks the rezzing entity
            // population instead of leaving it to be guessed from batch-span counts.
            .add_plugins(EntityCountDiagnosticsPlugin::default());
        // Extra diagnostic *sources* that are only worth their cost while a profiler
        // is attached (nothing consumes them outside the Tracy plots yet — move them
        // out of this gate once the statistics floater reads them), so they compile
        // in only with the Tracy client:
        //   * process/system CPU + memory — carries real sampling overhead;
        //   * the live region-circuit count (`crate::net_diagnostics`);
        //   * the per-kind entity population, main and render world
        //     (`crate::entity_diagnostics`).
        // Render-pass GPU/CPU timings + draw-call / pipeline stats need no add here:
        // `RenderPlugin` (via `DefaultPlugins`) already installs
        // `RenderDiagnosticsPlugin`, so those rows are always in the store and stream
        // through `tracy_plots` whenever a profiler is attached.
        #[cfg(feature = "profile-tracy")]
        app.add_plugins((
            bevy::diagnostic::SystemInformationDiagnosticsPlugin,
            crate::net_diagnostics::NetDiagnosticsPlugin,
            crate::entity_diagnostics::EntityDiagnosticsPlugin,
        ));
        // Stream those diagnostics (and any others registered) to Tracy as plots,
        // and mark the fixed-timestep physics loop as a Tracy secondary frame, so
        // the profiler shows graphed telemetry and a physics-cadence timeline on top
        // of the `tracing` zones. Only present with the Tracy client compiled in.
        #[cfg(feature = "profile-tracy")]
        app.add_plugins(crate::tracy_plots::TracyProfilingPlugin);
    }
}
