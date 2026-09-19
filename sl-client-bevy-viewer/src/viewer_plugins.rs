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
use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use sl_client_bevy::{
    CloudMaterialPlugin, SkyMaterialPlugin, StarMaterialPlugin, SunDiscMaterialPlugin,
    TerrainMaterialPlugin, WaterMaterialPlugin,
};

use crate::attachment_menu::AttachmentMenuPlugin;
use crate::avatar_menu::AvatarMenuPlugin;
use crate::camera::CameraPlugin;
use crate::edit_selection::EditSelectionPlugin;
use crate::edit_tool::EditToolPlugin;
use crate::exposure::SlExposurePlugin;
use crate::gizmos::EditGizmoPlugin;
use crate::glow::SlGlowPlugin;
use crate::hud_pick::pick_and_touch;
use crate::input_action::InputActionPlugin;
use crate::input_context::InputContextPlugin;
use crate::land_menu::LandMenuPlugin;
use crate::object_menu::ObjectMenuPlugin;
use crate::particle_render::{ParticleRenderPlugin, setup_particle_quad};
use crate::physics::PhysicsPlugin;
use crate::pie_menu::PieMenuPlugin;
use crate::probes::ReflectionProbePlugin;
use crate::sit_camera::SitCameraPlugin;
use crate::spacenav::{DeviceRead, SpacenavPlugin};
use crate::tonemap::SlTonemapPlugin;
use crate::underwater_fog::UnderwaterFogPlugin;

/// Input focus and actions, the camera, avatar movement, the sit camera and the
/// SpaceNavigator: what turns keys, mouse and devices into world intent.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ViewerInputPlugins {
    /// Whether the 6-DOF device is actually read — see [`DeviceRead`].
    pub(crate) spacenav: DeviceRead,
}

impl ViewerInputPlugins {
    /// The input fold with the 6-DOF **device read** left out — the headless
    /// fixture world's configuration. Every other input seam is driven through
    /// a window message the harness writes; the SpaceNavigator alone is read
    /// straight off the machine, so a fixture world keeps only the ECS half.
    #[cfg(test)]
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
