//! Bevy visual viewer for Second Life / OpenSim.
//!
//! See the crate `README.md` and the `viewer` topic under `roadmap/` for the
//! staged plan. [`run`] logs in via the shared `credentials.toml` mechanism
//! (`sl-repl::auth`) and opens a window that renders a region: terrain, prims,
//! meshes, sculpts, avatars and chat.
//!
//! # Why this is a library
//!
//! The crate builds **two** binaries over one module tree:
//!
//! - `sl-client-bevy-viewer` (`src/main.rs`) — the viewer proper, a thin shell
//!   over [`run`].
//! - `sl-client-bevy-viewer-gallery` (`src/bin/`) — the UI gallery, a thin shell
//!   over [`gallery::run`]: the same widgets and panels with **no login and no
//!   world** (`viewer-ui-test-harness`).
//! - `sl-client-bevy-viewer-scenes` (`src/bin/`) — the render gallery, a thin
//!   shell over [`render_gallery::run`]: the same geometry, converters and
//!   materials with **no login and no world** (`viewer-render-test-harness`).
//!
//! Both need the UI modules (`ui`, `ui_font`, `ui_text`, [`gallery`]), and
//! two binaries cannot share a `pub(crate)` module tree — only a library can
//! give them one. Hence a library with two thin shells rather than two binaries
//! that each re-`#[path]`-include the same files, which would compile them twice
//! and leave every item either binary happens not to use tripping `dead_code`.
//!
//! Only the handful of items a shell actually calls ([`run`], [`Error`],
//! [`gallery::run`]) are `pub`; the module tree stays `pub(crate)` exactly as it
//! was.

mod about_floater;
mod about_land;
mod about_landmark;
mod about_region;
mod animations;
mod animesh;
mod appearance;
mod asset_budget;
mod asset_retry;
mod attachment_menu;
mod audio;
mod avatar_assets;
mod avatar_dump;
mod avatar_menu;
mod avatar_picker;
mod avatar_profile;
mod avatar_replay;
mod avatars;
mod bake_inputs;
mod bake_publish;
mod beacons;
mod body_physics;
mod bottom_toolbar;
mod browser_widget;
mod build_info;
mod bump;
mod camera;
mod chat;
mod chat_input;
mod clipboard;
mod conversations;
mod coords;
mod debug_settings;
mod diagnostics;
mod double_click_teleport;
mod edit_contents;
mod edit_create;
mod edit_link;
mod edit_material;
mod edit_material_asset;
mod edit_math;
mod edit_notecard;
mod edit_params;
mod edit_script;
mod edit_selection;
mod edit_texture;
mod edit_texture_align;
mod edit_tool;
mod edit_undo;
mod edit_wearable;
mod emoji_complete;
mod emoji_picker;
mod environment;
mod environment_assets;
mod experience_permission;
mod experiences_floater;
mod exposure;
mod face_material;
mod flexi;
mod floater;
mod floater_persist;
pub mod gallery;
mod geometry_cache;
mod gizmos;
mod glow;
mod gpu_avatar_spike;
mod gpu_avatars;
mod gpu_pick;
mod ground;
mod group_notice;
mod group_profile;
mod groups;
mod hand_pose;
mod hover_text;
mod hover_tooltip;
mod hud;
mod hud_pick;
mod i18n;
mod ik;
mod input_action;
mod input_context;
mod inspector_popup;
mod inventory;
mod inventory_actions;
mod inventory_drag;
mod inventory_filters;
mod inventory_gallery;
mod inventory_properties;
mod land_menu;
mod legacy_materials;
mod lights;
mod linkified_text;
mod load_url;
mod local_chat_input;
mod locomotion;
mod locomotion_ik;
mod look_at;
mod material_cache;
mod material_preview;
mod materials;
mod media_audio;
mod media_controls;
mod media_diagnostics;
mod media_engine;
mod media_keys;
mod media_prim;
mod menu;
mod menu_bar;
mod menu_search;
mod meshes;
mod minimap;
mod minimap_math;
mod movement;
mod mutes;
mod name_tag_billboard;
mod name_tag_content;
mod nearby_chat_bar;
mod notecard_render;
mod notification_host;
mod notification_persist;
mod notifications;
mod object_cost;
mod object_menu;
mod objects;
mod offers_invites;
mod parcel_audio;
mod parcel_borders;
mod parcel_names;
mod particle_render;
mod particles;
mod paths;
mod people;
mod physics;
mod pie_menu;
mod preferences;
mod preferences_alerts;
mod preferences_audio;
mod preferences_camera_move;
mod preferences_chat;
mod preferences_colors_skins;
mod preferences_general;
mod preferences_graphics;
mod preferences_network_cache;
mod probe_layers;
mod probes;
mod procedural;
mod quick_preferences;
mod radar;
mod radar_model;
mod reach;
pub mod render_gallery;
mod render_priority;
#[cfg(test)]
mod render_readback;
mod render_scene;
#[cfg(test)]
mod render_test;
mod replay_bundle;
mod scene_reset;
mod screenshot;
mod script_dialog;
mod script_permission;
mod search;
mod session;
mod settings;
mod settings_binding;
mod shadow_visibility;
mod sit_camera;
mod sit_offset;
mod skin;
mod skin_colors;
mod sky;
mod sky_presets;
mod slurl_dispatch;
mod snapshot_floater;
mod sound_cache;
mod spacenav;
mod stand_stop_button;
mod status_bar;
mod teleport_progress;
mod terrain;
mod texture_anim;
mod textures;
mod tonemap;
// Per-kind entity-population diagnostics streamed to Tracy; only compiled with
// the Tracy client present (it exists solely to feed the profiler).
#[cfg(feature = "profile-tracy")]
mod entity_diagnostics;
// Live circuit-count diagnostic streamed to Tracy; only compiled with the Tracy
// client present (it exists solely to feed the profiler).
#[cfg(feature = "profile-tracy")]
mod net_diagnostics;
// Tracy plot streaming + physics secondary frame mark; only compiled when the
// Tracy client (and its `tracing-tracy` bridge) is present.
#[cfg(feature = "profile-tracy")]
mod tracy_plots;
mod transparency;
mod typing;
mod ui;
mod ui_color_picker;
mod ui_combo;
mod ui_element;
mod ui_font;
mod ui_name_link;
mod ui_perf;
mod ui_pseudoloc;
mod ui_radio;
mod ui_search;
mod ui_sounds;
mod ui_tab;
mod ui_table;
#[cfg(test)]
mod ui_test;
mod ui_text;
mod ui_text_input;
mod ui_texture_picker;
mod underwater_fog;
mod url_linkify;
mod virtual_list;
mod volume_panel;
mod water;
mod water_exclusion;
mod web_auth;
mod web_floater;
mod world_map;
mod world_map_math;
mod world_map_tiles;
mod world_sounds;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::light::DirectionalLightShadowMap;
use bevy::light::cluster::{ClusterConfig, ClusterFarZMode, ClusterZConfig};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::window::{CursorGrabMode, CursorOptions};
use clap::Parser as _;
use sl_client_bevy::{
    AccountDirsConfig, AnimationKey, ChatLogConfig, ClientDirectories, CloudMaterialPlugin,
    InventoryCacheConfig, LoggedChatType, LoginFailure, LoginParams, LoginRequest, MfaChallenge,
    SkyMaterialPlugin, SlClientPlugin, SlLoginRejected, SlMfaChallenge, StarMaterialPlugin,
    StartLocation, SunDiscMaterialPlugin, TerrainMaterialPlugin, Uuid, WaterMaterialPlugin,
};
use sl_repl::{Avatar, Credentials};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::about_floater::AboutFloaterPlugin;
use crate::about_land::AboutLandPlugin;
use crate::about_landmark::AboutLandmarkPlugin;
use crate::about_region::AboutRegionPlugin;
use crate::animations::{
    AnimationManager, AnimationPlayback, drive_avatar_skeletons, ingest_avatar_animations,
    poll_animations, pose_avatar_skeletons, update_animation_caps,
};
use crate::animesh::{
    ControlAvatarState, drive_control_avatars, ingest_object_animations, publish_control_avatars,
};
use crate::appearance::{ServerBakeState, drive_server_bake};
use crate::asset_budget::{MeshUploadBudget, reset_mesh_upload_budget};
use crate::attachment_menu::AttachmentMenuPlugin;
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatar_menu::AvatarMenuPlugin;
use crate::avatar_picker::AvatarPickerPlugin;
use crate::avatar_profile::AvatarProfilePlugin;
use crate::avatars::RefetchAvatarTextures;
use crate::avatars::{
    AppearanceApplyBudget, AvatarBakeMaterials, AvatarRuntimeMorphs, AvatarState, OwnLocalBake,
    VolumeMorphGain, apply_avatar_appearance, apply_avatar_bake_textures, apply_avatar_names,
    apply_avatar_part_visibility, apply_avatar_runtime_morphs, apply_bom_face_materials,
    apply_own_local_bake, apply_own_shape_from_wearables, assign_avatar_bake_materials,
    fit_avatar_tag_heights, focus_camera_on_volume_shape, handle_refetch_avatar_textures,
    ingest_avatar_bakes, log_avatar_interest_census, recenter_avatars, setup_avatar_body,
    toggle_volume_morphs, update_avatar_objects, update_coarse_avatars,
};
use crate::bake_inputs::{
    OwnBakeInputs, WearableAssetFetched, WearableAssetManager, assemble_own_bake,
    drive_wearable_requests, poll_wearable_assets, update_asset_caps,
};
use crate::bake_publish::{OwnBakePublish, drive_bake_publish};
use crate::bump::{BumpManager, apply_bump_normals, register_bump_faces};
use crate::camera::{
    CameraMode, CameraPlugin, CameraRig, CameraSpin, CameraStart, SpinAxis, ViewerCamera,
    dump_camera_pose, position_camera,
};
use crate::chat::{
    ChatOverlay, position_chat_overlay, restyle_chat_overlay, setup_chat_overlay,
    tick_chat_overlay, update_chat_overlay,
};
use crate::chat_input::ChatInputPlugin;
use crate::conversations::ConversationsPlugin;
use crate::diagnostics::{
    PipelineOverlayVisible, pipeline_overlay_active, setup_pipeline_overlay,
    toggle_pipeline_overlay, update_pipeline_overlay,
};
use crate::edit_selection::EditSelectionPlugin;
use crate::edit_tool::EditToolPlugin;
use crate::emoji_complete::ColonCompletePlugin;
use crate::emoji_picker::EmojiPickerPlugin;
use crate::environment::{EnvironmentState, ingest_environment, request_environment};
use crate::experience_permission::ExperiencePermissionPlugin;
use crate::experiences_floater::ExperiencesPlugin;
use crate::exposure::{SlExposure, SlExposurePlugin};
use crate::flexi::simulate_flexi;
use crate::floater::FloaterPlugin;
use crate::floater_persist::FloaterPersistPlugin;
use crate::gizmos::EditGizmoPlugin;
use crate::glow::{SlGlow, SlGlowPlugin};
use crate::group_notice::GroupNoticePlugin;
use crate::group_profile::GroupProfilePlugin;
use crate::groups::GroupsPlugin;
use crate::hud::{HudState, apply_hud_fullbright, fit_hud_points, setup_hud_screen};
use crate::hud_pick::pick_and_touch;
use crate::i18n::ViewerI18nPlugin;
use crate::input_action::InputActionPlugin;
use crate::input_context::{CursorGrabAllowed, InputContextPlugin, world_has_keyboard};
use crate::inventory::InventoryPlugin;
use crate::inventory_actions::InventoryActionsPlugin;
use crate::inventory_drag::InventoryDragPlugin;
use crate::inventory_filters::InventoryFiltersPlugin;
use crate::inventory_gallery::InventoryGalleryPlugin;
use crate::inventory_properties::InventoryPropertiesPlugin;
use crate::land_menu::LandMenuPlugin;
use crate::legacy_materials::{
    LegacyMaterialManager, apply_legacy_materials, apply_legacy_normal_maps,
    apply_legacy_specular_maps, drive_legacy_material_requests, receive_legacy_materials,
    register_legacy_materials,
};
use crate::lights::{LocalLights, drive_local_lights};
use crate::load_url::LoadUrlPlugin;
use crate::local_chat_input::LocalChatInputPlugin;
use crate::locomotion::drive_own_locomotion;
use crate::materials::{
    MaterialManager, apply_blinn_phong_hide, apply_material_overrides, apply_pbr_textures,
    poll_materials, register_changed_render_materials, register_pbr_materials,
    revert_removed_render_materials, update_material_caps,
};
use crate::meshes::{MeshDecoded, MeshManager, poll_meshes, update_mesh_caps};
use crate::movement::{AvatarControls, drive_avatar_controls};
use crate::nearby_chat_bar::NearbyChatBarPlugin;
use crate::notification_host::{
    NotificationHostPlugin, ingest_alert_messages, spawn_notification_demo,
};
use crate::notification_persist::NotificationPersistPlugin;
use crate::object_menu::ObjectMenuPlugin;
use crate::objects::{
    ObjectState, PendingDecodedMeshes, PendingDecodedSculpts, PendingObjectEvents, PrimLodTargets,
    RiggedBindSkipLog, TreeLodTargets, adopt_pending_attachments, apply_object_meshes,
    apply_object_sculpts, apply_prim_lod, apply_rigged_attachments, apply_tree_lod,
    log_suspicious_objects, pick_object, prune_control_avatars, recenter_objects,
    spawn_animesh_control_avatars, update_objects,
};
use crate::offers_invites::OffersInvitesPlugin;
use crate::particle_render::{ParticleRenderPlugin, setup_particle_quad};
use crate::particles::{ParticleSim, drive_particles, focus_camera_on_particles, setup_particles};
use crate::people::PeoplePlugin;
use crate::physics::PhysicsPlugin;
use crate::pie_menu::PieMenuPlugin;
use crate::probes::ReflectionProbePlugin;
use crate::render_priority::drive_render_priority;
use crate::screenshot::{ScreenshotSchedule, capture_screenshots, poll_screenshot_saves};
use crate::script_dialog::ScriptDialogPlugin;
use crate::script_permission::ScriptPermissionPlugin;
use crate::session::{
    PlayOnLogin, ViewerSession, apply_draw_distance, drive_session, enforce_quit_deadline,
    handle_quit_input, handle_quit_requests, repeat_debug_animation, report_agent_viewport,
    report_camera_interest, save_settings_on_logout,
};
use crate::settings::{AccountContext, ViewerSettings, load_account_settings};
use crate::settings_binding::SettingsBindingPlugin;
use crate::sit_camera::SitCameraPlugin;
use crate::sky::{
    apply_cloud_textures, apply_disc_textures, apply_sky_textures, apply_star_textures,
    center_sky_on_camera, drive_clouds, drive_sky, drive_stars, drive_sun_moon_discs, setup_clouds,
    setup_sky, setup_stars, setup_sun_moon_discs,
};
use crate::spacenav::SpacenavPlugin;
use crate::stand_stop_button::StandStopButtonPlugin;
use crate::terrain::{
    PendingPatchRebuilds, TerrainState, drain_patch_rebuilds, recenter_terrain, update_terrain,
};
use crate::texture_anim::{drive_texture_animations, restore_stopped_animations};
use crate::textures::{
    DeferredFaceTextures, PrimTextures, TextureApplyBudget, TextureDecoded, TextureManager,
    apply_prim_textures, drain_deferred_face_textures, drain_lod_reuploads, poll_textures,
    reset_texture_apply_budget, update_texture_caps,
};
use crate::tonemap::{SlTonemap, SlTonemapPlugin};
use crate::typing::{TypingState, drive_own_typing};
use crate::ui::{UiScaffoldSystems, ViewerUiPlugin};
use crate::ui_element::UiAction;
use crate::ui_tab::TabWidgetPlugin;
use crate::ui_table::TableWidgetPlugin;
use crate::ui_text::{
    TextDemoVisible, apply_text_demo_visibility, setup_text_demo, toggle_text_demo,
};
use crate::ui_text_input::{
    TextInputDemoVisible, TextInputPlugin, apply_text_input_demo_visibility, setup_text_input_demo,
    toggle_text_input_demo, update_demo_value_readouts,
};
use crate::underwater_fog::{UnderwaterFog, UnderwaterFogPlugin, update_underwater_fog};
use crate::virtual_list::VirtualListPlugin;
use crate::water::{WaterLevel, apply_water_textures, drive_water, setup_water, update_water};
use crate::water_exclusion::{
    bind_water_exclusion_mask, convert_water_exclusion_faces, setup_water_exclusion,
    sync_water_exclusion_camera,
};

/// The local OpenSim grid login URI used when none is otherwise resolved.
const DEFAULT_LOGIN_URI: &str = "http://127.0.0.1:9000/";

/// An error from the viewer binary.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// A credentials-file or MFA-acquisition error.
    #[error("authentication error: {0}")]
    Auth(
        #[source]
        #[from]
        sl_repl::AuthError,
    ),
    /// A grid nickname could not be mapped to a login URI.
    #[error("unknown grid `{0}`; pass --login-uri explicitly")]
    UnknownGrid(String),
    /// The resolved login URI was not a valid URL.
    #[error("invalid login URI: {0}")]
    LoginUri(
        #[source]
        #[from]
        url::ParseError,
    ),
    /// The grid issued an MFA challenge but the avatar has no `mfa_command`.
    #[error("the grid requires multi-factor authentication but no mfa_command is configured")]
    MfaRequired,
    /// A `--replay` bundle could not be loaded (missing directory, no manifests,
    /// or an unreadable / unsupported manifest).
    #[error("replay bundle error: {0}")]
    Replay(String),
}

/// The command-line options for the viewer.
#[derive(clap::Parser, Debug)]
#[clap(
    name = "sl-client-bevy-viewer",
    about = clap::crate_description!(),
    author = clap::crate_authors!(),
    version = clap::crate_version!(),
    disable_version_flag = true,
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI switches are independent flags by nature; clap derives the parser from them"
)]
struct Options {
    /// The TOML credentials file.
    #[clap(
        long,
        default_value = "credentials.toml",
        env = "SL_VIEWER_CREDENTIALS"
    )]
    credentials: PathBuf,
    /// Which avatar in the credentials file to log in as (defaults to the file's
    /// `default_avatar`, or its sole avatar).
    #[clap(long)]
    avatar: Option<String>,
    /// A grid nickname (`agni` / `aditi` / `localhost`) to log in to.
    #[clap(long)]
    grid: Option<String>,
    /// An explicit XML-RPC login URI, overriding `--grid` and the avatar's own.
    #[clap(long)]
    login_uri: Option<String>,
    /// The login start location (`last`, `home`, or `uri:Region&x&y&z`),
    /// overriding the persisted preference (the General tab's default) for
    /// this run.
    #[clap(long)]
    start: Option<StartLocation>,
    /// The viewer channel reported to the grid.
    #[clap(long, default_value = build_info::VIEWER_NAME)]
    channel: String,
    /// The viewer version reported to the grid. Defaults to the crate version
    /// extended with the build-time `git describe` metadata (e.g.
    /// `0.1.0+ed81459`), so grid-side logs identify the exact build.
    #[clap(long, default_value_t = build_info::full_version())]
    version: String,
    /// Directory holding the standard Linden `character/` assets
    /// (`avatar_skeleton.xml`, `avatar_lad.xml`, the base-body `.llm` meshes) —
    /// point this at an installed Firestorm / Second Life viewer to render real
    /// system-avatar bodies. Without it, avatars stay placeholder spheres.
    #[clap(long, env = "SL_VIEWER_ASSETS")]
    viewer_assets: Option<PathBuf>,
    /// A debug affordance: play this animation (a built-in or uploaded `.anim`
    /// UUID) on the agent's **own** avatar once it lands, so the skeleton-animation
    /// driver can be exercised with a single login. Needs `--viewer-assets` (a
    /// sphere has no skeleton to pose). Repeat the flag (or pass a comma-separated
    /// list) to layer several at once and exercise the P18.4 priority blending.
    #[clap(long, env = "SL_VIEWER_PLAY_ANIMATION", value_delimiter = ',')]
    play_animation: Vec<Uuid>,
    /// Keep re-issuing `--play-animation` on a short cadence so it is still
    /// playing after the avatar has finished loading (a one-shot play can expire
    /// before the body is fully baked / on screen). Handy for capture runs.
    #[clap(long)]
    repeat_animation: bool,
    /// A debug affordance: when set, save a numbered PNG sequence of the window
    /// to this directory (after a startup delay, then quit) instead of running
    /// interactively — for inspecting an animated avatar offline. Leaves the
    /// cursor un-grabbed so it does not hijack the desktop it runs on.
    #[clap(long, env = "SL_VIEWER_SCREENSHOT_DIR")]
    screenshot_dir: Option<PathBuf>,
    /// A debug affordance: place the fly-camera at an absolute Second Life
    /// region-local position `x,y,z` (Z-up metres, e.g. `240,128,25` near an
    /// east edge) instead of snapping it to the agent on login. Lets an
    /// unattended screenshot capture frame a fixed viewpoint — such as a region
    /// edge, to inspect the water surface / underwater fog (R21). Pairs with
    /// `--camera-look-at` and `--camera-spin`.
    #[clap(long, value_parser = parse_sl_vec3, allow_hyphen_values = true)]
    camera_position: Option<Vec3>,
    /// Aim the fixed camera (`--camera-position`) at this Second Life
    /// region-local point `x,y,z` (Z-up metres). Ignored without
    /// `--camera-position`; without it the camera keeps its default forward aim.
    #[clap(long, value_parser = parse_sl_vec3, allow_hyphen_values = true)]
    camera_look_at: Option<Vec3>,
    /// A debug affordance: auto-rotate the camera at this many degrees per second
    /// about the axis chosen by `--camera-spin-axis` — a slow survey pan for a
    /// screenshot sequence. Works with the login-snapped camera too.
    #[clap(long, allow_hyphen_values = true)]
    camera_spin: Option<f32>,
    /// Which camera axis `--camera-spin` rotates about (default `yaw`, a
    /// left/right pan).
    #[clap(long, value_enum, default_value_t = SpinAxis::Yaw)]
    camera_spin_axis: SpinAxis,
    /// The UI skin to wear — a directory under `assets/skins/` (`graphite`,
    /// `azure`). Skins are colour / texture / font tokens only, never layout.
    /// Overrides the persisted preferences choice (the colors & skins tab) for
    /// this run, without rewriting it.
    #[clap(long)]
    skin: Option<String>,
    /// A theme overlay for the skin — a file under
    /// `assets/skins/<skin>/themes/` (e.g. `dark`), which redefines a subset of
    /// the skin's tokens. Omit for the skin's own base. Overrides the persisted
    /// preferences choice for this run, without rewriting it.
    #[clap(long)]
    theme: Option<String>,
    /// Watch the skin `.css` files and re-apply them live as they are edited —
    /// the skin-authoring loop. Off by default (a tiny background cost); turn it
    /// on while designing a skin or theme.
    #[clap(long)]
    watch_skins: bool,
    /// Disable the embedded web-media engine (CEF): no media-on-a-prim, no
    /// in-viewer browser floater, no profile Web-tab page rendering. The
    /// escape hatch when the CEF runtime misbehaves on a system.
    #[clap(long)]
    disable_web_media: bool,
    /// Disable the video/audio playback engine (GStreamer): no direct-URL
    /// video on media-on-a-prim faces and no parcel radio streams. The
    /// escape hatch when the system's GStreamer misbehaves.
    #[clap(long)]
    disable_video_media: bool,
    /// Do not log the grid account into the Second Life websites at login
    /// (`viewer-web-openid-auth`): the in-viewer browser, profile Web tab and
    /// Search Web tab then browse anonymously instead of already signed in.
    /// Has no effect off Second Life (OpenSim sends no OpenID token).
    #[clap(long)]
    no_web_auth: bool,
    /// Do not auto-fetch a joined group / conference session's server-side
    /// chat backlog (`chat-group-history-server-side`): the Conversations
    /// floater then shows no muted-green server-history band, only local
    /// recall and live lines. Has no effect off Second Life (OpenSim has no
    /// `ChatSessionRequest` capability, so nothing is fetched there either
    /// way).
    #[clap(long)]
    no_group_chat_history: bool,
    /// Render a captured avatar-state bundle **offline** (no login, no grid):
    /// `--replay <dir>` where `<dir>` is a bundle written by the capture
    /// (**Ctrl+Alt+D** with `SL_VIEWER_DUMP_DIR` set). The viewer rebuilds the
    /// avatar(s) from the bundle and draws them with the live render pipeline, so
    /// a render-only bug can be reproduced — and a fix tested — after the avatar
    /// has logged out. Needs `--viewer-assets` (a body needs the system skeleton).
    #[clap(long, value_name = "DIR")]
    replay: Option<PathBuf>,
    /// In `--replay`, add an orbiting local light around the avatar — a slow
    /// specular-highlight sweep for testing material shading. Off by default.
    #[clap(long)]
    replay_orbit_light: bool,
    /// In `--replay`, add a local reflection probe around the avatar, so
    /// image-based-lighting materials have a probe to sample. Off by default.
    #[clap(long)]
    replay_reflection_probe: bool,
}

/// Parse a `--camera-position` / `--camera-look-at` argument: three
/// comma-separated Second Life region-local coordinates (`x,y,z`, Z-up metres)
/// into a Bevy Y-up [`Vec3`], applying the same `(x, y, z) -> (x, z, -y)` axis
/// map as [`crate::coords::sl_to_bevy_vec`] so the operator can think in Second
/// Life region coordinates.
fn parse_sl_vec3(value: &str) -> Result<Vec3, String> {
    let parts: Vec<&str> = value.split(',').collect();
    let [x, y, z] = parts.as_slice() else {
        return Err(format!(
            "expected three comma-separated numbers `x,y,z`, got {value:?}"
        ));
    };
    let x = x.trim().parse::<f32>().map_err(|error| error.to_string())?;
    let y = y.trim().parse::<f32>().map_err(|error| error.to_string())?;
    let z = z.trim().parse::<f32>().map_err(|error| error.to_string())?;
    // Second Life Z-up region-local -> Bevy Y-up: (x, y, z) -> (x, z, -y).
    Ok(Vec3::new(x, z, -y))
}

/// Map a grid nickname to its XML-RPC login URI, or `None` if unknown.
fn grid_login_uri(grid: &str) -> Option<&'static str> {
    match grid.to_ascii_lowercase().as_str() {
        "agni" | "secondlife" | "sl" => Some("https://login.agni.lindenlab.com/cgi-bin/login.cgi"),
        "aditi" | "beta" => Some("https://login.aditi.lindenlab.com/cgi-bin/login.cgi"),
        "localhost" | "local" | "opensim" => Some(DEFAULT_LOGIN_URI),
        _other => None,
    }
}

/// Resolve the login URI from (in priority order) the explicit `--login-uri`,
/// `--grid`, the avatar's own `login_uri` / `grid`, and finally the local
/// default.
///
/// # Errors
///
/// Returns [`Error::UnknownGrid`] if a grid nickname has no known login URI.
fn resolve_login_uri(options: &Options, avatar: &Avatar) -> Result<String, Error> {
    if let Some(uri) = &options.login_uri {
        return Ok(uri.clone());
    }
    if let Some(grid) = &options.grid {
        return grid_login_uri(grid)
            .map(str::to_owned)
            .ok_or_else(|| Error::UnknownGrid(grid.clone()));
    }
    if let Some(uri) = avatar.login_uri() {
        return Ok(uri.to_owned());
    }
    if let Some(grid) = avatar.grid() {
        return grid_login_uri(grid)
            .map(str::to_owned)
            .ok_or_else(|| Error::UnknownGrid(grid.to_owned()));
    }
    Ok(DEFAULT_LOGIN_URI.to_owned())
}

/// The recoverable outcome of one windowed session: an MFA challenge to answer
/// or a retryable login rejection, either of which stops the app.
#[derive(Resource, Default)]
struct LoginOutcome {
    /// The MFA challenge the session stopped on, if any.
    challenge: Option<MfaChallenge>,
    /// The retryable "already logged in" rejection, if any.
    rejected: Option<LoginFailure>,
}

/// Startup system: spawn the one [`ViewerCamera`]. The scene's directional light
/// (the sun / moon) is spawned by [`crate::sky::setup_sky`], which also drives it
/// from the region's environment.
///
/// The camera starts in third-person, which follows the avatar as soon as it
/// arrives ([`position_camera`]), so no login camera-snap is needed. A fixed
/// `--camera-position` instead starts it in **flycam** at that absolute pose (and
/// aims it), which is what the unattended screenshot harness frames from; the
/// `SL_VIEWER_CAMERA_*` envs seed the third-person orbit so the harness can also
/// frame the avatar from a chosen angle.
fn setup_scene(
    mut commands: Commands,
    camera_start: Res<CameraStart>,
    mut mode: ResMut<CameraMode>,
) {
    let mut rig = CameraRig::default();
    // Seed the third-person orbit from the debug framing envs (a no-op when unset):
    // orbit → azimuth, elevation → elevation, distance → distance.
    rig.seed_orbit_from_env();
    let camera_transform = if let Some(position) = camera_start.position {
        // A fixed pose is a flycam pose: place and aim it, and leave it alone.
        let mut transform = Transform::from_translation(position);
        if let Some(look) = camera_start.look {
            rig.aim_along(look);
            // `drive_flycam` owns the flycam transform and only integrates input
            // deltas onto it — it never reads the rig's yaw/pitch. So the initial
            // facing has to be baked into the transform rotation here, or the camera
            // keeps its identity (SL-north) orientation and `--camera-look-at` is
            // silently ignored. Reconstruct the rotation from the rig exactly as
            // mouselook does (`aim_quat` → forward along `look`), so the transform
            // and rig agree from the first frame.
            transform.rotation = rig.aim_quat();
        }
        *mode = CameraMode::Flycam;
        transform
    } else {
        // A provisional pose near a region centre; `position_camera` moves it to
        // frame the avatar the moment one arrives.
        Transform::from_translation(Vec3::new(128.0, 30.0, -128.0))
    };
    commands.spawn((
        // The underwater-fog post-process (P23.1) samples the scene depth, so make
        // the main-pass depth texture readable (`TEXTURE_BINDING`). MSAA is pinned
        // to 4× (the default) so that depth texture is multisampled to match the
        // fog pass's `texture_depth_2d_multisampled` binding.
        Camera3d {
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        // A close near plane (2 cm) so the camera can push right up to fine detail
        // — an avatar's face — without the surface clipping away, and a far plane
        // well beyond a region's diagonal so distant objects do not vanish.
        Projection::Perspective(PerspectiveProjection {
            near: 0.02,
            far: 4096.0,
            ..default()
        }),
        camera_transform,
        ViewerCamera,
        rig,
        // A clustered-forward Z config tuned for a viewer that pushes the camera
        // right up to avatars wearing small local lights (facelights). Bevy's
        // default `ClusterZConfig` keeps a **special first Z-slice** spanning
        // `[near_plane, first_slice_depth=5 m]`, and its default
        // `MaxClusterableObjectRange` far mode derives the grid's far plane from the
        // visible lights' own reach. Together those drop a worn light out of a
        // mid-distance band: the light and the surface it lights sit inside that 5 m
        // special slice, whose light handling fails, so a facelight only reaches the
        // face when the camera is inside the light sphere (a separate special case)
        // and goes dark across the rest of the near field. Shrinking the special
        // slice to `0.5 m` puts the whole avatar-viewing range into ordinary
        // well-conditioned logarithmic slices (which light correctly), and pinning
        // the far plane to a constant stops a lone small light from collapsing the
        // grid's depth range. The XY/Z counts stay at Bevy's defaults.
        ClusterConfig::FixedZ {
            total: 4096,
            z_slices: 24,
            z_config: ClusterZConfig {
                first_slice_depth: 0.5,
                far_z_mode: ClusterFarZMode::Constant(512.0),
            },
            dynamic_resizing: true,
        },
        Msaa::Sample4,
        // P33.3: render the scene into a floating-point target and tonemap it once,
        // at the end, with the reference viewer's own tone mapper (`tonemap`).
        //
        // Without `Hdr` the view target is 8-bit, which Bevy takes as the cue to
        // tonemap `StandardMaterial` inside the mesh shader — leaving the viewer's
        // custom sky / terrain / water materials (which never call Bevy's tonemapper)
        // merely *clipped* at 1.0 instead, two different transfers in one frame. The
        // reflection probes capture the scene linear and un-tonemapped, so that split
        // also made a probe's cubemap disagree with what the eye saw of the very same
        // surroundings — the miscalibration P33.3 exists to fix. One HDR target plus
        // one tone mapper at the end puts every material in the one linear space the
        // probes capture.
        Hdr,
        // Bevy's tonemapping is switched off: `SlTonemap` (the pass and its settings,
        // mirroring the reference's `RenderTonemapType` / `RenderTonemapMix` /
        // `RenderExposure`) is this viewer's tone mapper, and two would double up.
        Tonemapping::None,
        SlTonemap::default(),
        // The reference's dynamic exposure inputs (the `exp_min`/`exp_max` range is
        // filled per frame from the active sky by `refresh_exposure`). Only on the
        // main camera — the reflection-probe capture cameras stay linear.
        SlExposure::default(),
        // The reference's glow pass inputs (disabled by default; see `glow.rs`).
        // Only on the main camera.
        SlGlow::default(),
        // Bevy's *photometric* exposure: what turns the sun's illuminance (lux) and a
        // prim light's lumens into the linear radiance the frame is composed in. It is
        // a distinct thing from the reference's `RenderExposure` (a plain scale on the
        // finished linear frame, carried by `SlTonemap`), and it is spelled out rather
        // than left implicit because the reflection probes read it: their intensity is
        // derived from it (`probes::probe_intensity`), so a probe reproduces the
        // radiance it captured instead of re-scaling it.
        Exposure::default(),
        // The Second Life / Firestorm glow pass (`RenderGlow*`) is [`SlGlow`] above
        // (the faithful alpha-mask separable-Gaussian glow, `glow.rs`), which runs
        // after the tone mapper as the reference does — it replaced the Bevy
        // screen-space `Bloom` this camera used to carry.
        //
        // The `UnderwaterFog` component both carries the per-frame fog parameters
        // and selects this camera for the fog pass.
        UnderwaterFog::default(),
    ));
}

/// Capture a login-stopping outcome (MFA challenge or retryable rejection) into
/// the [`LoginOutcome`] resource and exit the app so the caller can restart the
/// login with the answer folded in.
fn capture_login_outcome(
    mut mfa: MessageReader<SlMfaChallenge>,
    mut rejected: MessageReader<SlLoginRejected>,
    mut outcome: ResMut<LoginOutcome>,
    mut exit: MessageWriter<AppExit>,
) {
    for challenge in mfa.read() {
        outcome.challenge = Some(challenge.0.clone());
        exit.write(AppExit::Success);
    }
    for rejection in rejected.read() {
        outcome.rejected = Some(rejection.0.clone());
        exit.write(AppExit::Success);
    }
}

/// Load the system-avatar `character/` assets from `dir`, logging (and swallowing)
/// a failure so a bad `--viewer-assets` path leaves avatars as placeholder
/// spheres rather than aborting the session.
fn load_avatar_library(dir: Option<&Path>) -> Option<AvatarAssetLibrary> {
    let dir = dir?;
    match AvatarAssetLibrary::load(dir) {
        Ok(library) => Some(library),
        Err(error) => {
            warn!(
                "failed to load avatar assets from {}: {error}; avatars stay spheres",
                dir.display()
            );
            None
        }
    }
}

/// The camera's start-up configuration for a viewer session — the fixed pose
/// (if any) and the optional auto-spin — bundled so [`run_session`] stays within
/// the argument-count lint.
struct CameraStartup {
    /// The fixed start pose, or the login-snapped default.
    start: CameraStart,
    /// The optional auto-spin survey pan.
    spin: CameraSpin,
}

/// The skin configuration for a viewer session: which skin / theme to wear and
/// whether to hot-watch the `.css` files. Bundled alongside [`CameraStartup`] to
/// keep [`run_session`] within the argument-count lint.
struct SkinRuntime {
    /// The initial skin + theme selection.
    selection: crate::skin::SkinSelection,
    /// Whether to watch the skin `.css` files for live edits (`--watch-skins`).
    watch: bool,
}

/// Which media engines a viewer session may start: the web (CEF) and video
/// (GStreamer) switches from `--disable-web-media` / `--disable-video-media`.
/// Bundled alongside [`CameraStartup`] to keep [`run_session`] within the
/// argument-count lint.
struct MediaRuntime {
    /// Whether the web (CEF) engine may initialise.
    web: bool,
    /// Whether the video (GStreamer) engine may initialise.
    video: bool,
    /// Whether to auto-login the grid account into the Second Life websites at
    /// login (`viewer-web-openid-auth`); cleared by `--no-web-auth`.
    web_auth: bool,
}

/// Run one windowed session to completion, returning any recoverable login
/// outcome (an MFA challenge or a retryable rejection) it stopped on.
#[expect(
    clippy::too_many_arguments,
    reason = "the viewer's startup knobs, already bundled where they group naturally \
              (camera, skin, media)"
)]
fn run_session(
    params: &LoginParams,
    viewer_assets: Option<&Path>,
    play_animation: &[Uuid],
    repeat_animation: bool,
    screenshot_dir: Option<&Path>,
    camera: CameraStartup,
    skin: SkinRuntime,
    media: MediaRuntime,
    fetch_server_chat_history: bool,
    replay: Option<crate::avatar_replay::ReplayConfig>,
) -> LoginOutcome {
    // Offline (avatar-state replay) mode: the plugin registers its event/resource
    // substrate but never logs in; the session is fed synthetic events from the
    // bundle instead (see `crate::avatar_replay`).
    let offline = replay.is_some();
    let CameraStartup {
        start: camera_start,
        spin: camera_spin,
    } = camera;
    let SkinRuntime {
        selection: skin,
        watch: watch_skins,
    } = skin;
    // Start the cursor free (visible, un-grabbed): the viewer opens in
    // third-person, whose pointer is free to click the world / UI.
    // `crate::input_context::drive_cursor_grab` captures it only when the camera
    // enters mouselook. (In screenshot mode it stays free regardless, so an
    // unattended capture run never hijacks the desktop's pointer.)
    let cursor_options = CursorOptions {
        grab_mode: CursorGrabMode::None,
        visible: true,
        ..default()
    };
    // Per-avatar on-disk directories, keyed by grid + avatar name (with UUID
    // rename discovery). Each kind lands under the XDG root that fits it: chat
    // transcripts under state, the inventory cache under cache, account settings
    // under config — a separate `accounts/<grid>/<name>/` tree under each.
    // Derived from the login parameters (grid from the login URI, name from the
    // request) and resolved to the avatar's directory at login, once the UUID is
    // known — by the plugin (`account_dirs`, for chat / inventory) and the
    // settings account-scope loader (`AccountContext` + `load_account_settings`).
    let grid = sl_account_dirs::grid_dir_name(&params.login_uri);
    let avatar =
        sl_account_dirs::avatar_dir_name(&params.request.first_name, &params.request.last_name);
    let account_dirs = Some(AccountDirsConfig {
        grid: grid.clone(),
        avatar: avatar.clone(),
        chat_log_base: crate::paths::state_accounts_base(),
        inventory_cache_base: crate::paths::cache_accounts_base(),
    });
    let config_accounts_base = crate::paths::config_accounts_base();

    // Resolve the system time zone now, while the process is still single-threaded:
    // it reads the `TZ` environment variable, and reading the environment is only
    // sound before Bevy's task pools spawn (below, with `DefaultPlugins`). The
    // snapshot floater reuses this cached zone to stamp filenames in local time.
    let local_time_zone = crate::snapshot_floater::LocalTimeZone::capture();

    let mut app = App::new();
    app.insert_resource(local_time_zone);
    // The About floater's login-derived facts (grid, login URI, reported
    // channel/version) — captured here where they are all still at hand.
    app.insert_resource(crate::about_floater::AboutSessionInfo {
        grid: grid.clone(),
        login_uri: params.login_uri.to_string(),
        channel: params.request.channel.clone(),
        version: params.request.version.clone(),
    });
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "sl-client-bevy-viewer".to_owned(),
                    // Wayland app-id (also X11 WM_CLASS) so compositors can
                    // match window rules / icons to this application.
                    name: Some("sl-client-bevy-viewer".to_owned()),
                    ..default()
                }),
                primary_cursor_options: Some(cursor_options),
                // Don't let Bevy's default close-to-exit despawn the window on a
                // close request (X button, or a Wayland compositor close): our
                // `handle_quit_requests` owns it and logs out gracefully first.
                close_when_requested: false,
                ..default()
            })
            // Watch the asset directory so an edited skin `.css` re-applies live
            // (`--watch-skins`, the skin-authoring loop). Off unless asked, since
            // watching carries a small background cost.
            .set(AssetPlugin {
                watch_for_changes_override: watch_skins.then_some(true),
                ..default()
            })
            // The binary installs its own `tracing` subscriber (so the
            // pre-window login logs go somewhere); drop Bevy's `LogPlugin` to
            // avoid the "global subscriber already set" clash.
            .disable::<LogPlugin>(),
    )
    .insert_resource(skin)
    .add_plugins(SlClientPlugin {
        params: params.clone(),
        diagnostics: true,
        // Log every text-chat type to the per-avatar chat directory — the
        // pre-login default; once the account settings load,
        // `preferences_chat` pushes the avatar's stored logging preferences
        // over this via `Command::SetChatLogConfig`.
        chat_log_config: ChatLogConfig {
            enabled: BTreeSet::from([
                LoggedChatType::Nearby,
                LoggedChatType::InstantMessage,
                LoggedChatType::Group,
                LoggedChatType::Conference,
            ]),
            ..ChatLogConfig::default()
        },
        directories: ClientDirectories::default(),
        account_dirs,
        // Cache the inventory tree per avatar (agent tree + Library).
        inventory_cache_config: InventoryCacheConfig {
            enabled: true,
            cache_library: true,
        },
        background_inventory_fetch: false,
        fetch_server_chat_history,
        offline,
    })
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
    // Shared OS-clipboard handle for the "Copy SLURL" affordances.
    .add_plugins(crate::clipboard::ClipboardPlugin)
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
    // Input focus / modal context (viewer-input-focus-contexts): derives who owns
    // the keyboard and the cursor from `bevy_input_focus`. Gates every world key
    // binding below via `world_has_keyboard`, so typing into a focused text field
    // no longer also walks the avatar.
    .add_plugins(InputContextPlugin)
    // The input action map (viewer-input-action-map): named actions + per-mode
    // binding profiles that replace the hardcoded keys in `movement` / `camera`.
    // Camera + movement read `ButtonInput<Action>`, gated once here on focus.
    .add_plugins(InputActionPlugin)
    // The camera system (viewer-camera-*): one `ViewerCamera` entity driven by a
    // `CameraMode` state machine (mouselook / third-person / flycam), replacing the
    // debug fly-camera. Every `.after(position_camera)` consumer reads its pose.
    .add_plugins(CameraPlugin)
    // Scripted sit camera + forced mouselook a seat imposes on sit
    // (viewer-sit-target-and-stand-button): tracked here, applied by
    // `position_camera`.
    .add_plugins(SitCameraPlugin)
    // SpaceNavigator / 6-DOF device input (viewer-input-spacenav-*): publishes the
    // device state (Linux, behind the `spacenav` feature) for the flycam to consume.
    .add_plugins(SpacenavPlugin)
    // The Stand Up / Stop flycam state button in the bottom toolbar's reserved
    // slot (viewer-sit-target-and-stand-button): Stand while seated, Stop flycam
    // while in flycam.
    .add_plugins(StandStopButtonPlugin)
    .add_plugins(crate::teleport_progress::TeleportProgressPlugin)
    .add_plugins(crate::double_click_teleport::DoubleClickTeleportPlugin)
    // The radial (pie) menu widget (viewer-ui-radial-menu): the mechanism only —
    // which entries a given pie holds is per-domain and belongs with the domain.
    .add_plugins(PieMenuPlugin)
    // The avatar context / pie menu (viewer-avatar-context-menu): the self / other
    // entry trees and their dispatch, opened by right-clicking an avatar's name
    // tag or body.
    .add_plugins(AvatarMenuPlugin)
    // The in-world object context / pie menu (viewer-object-context-menu): the
    // reference object entry tree and its dispatch, opened by right-clicking an
    // in-world object (the shared resolver lives with the avatar menu).
    .add_plugins(ObjectMenuPlugin)
    // The worn-attachment context / pie menus (viewer-attachment-context-menu,
    // viewer-hud-context-menu): the self / other entry trees and their dispatch,
    // opened by right-clicking a worn attachment — in world or on a HUD point.
    .add_plugins(AttachmentMenuPlugin)
    // The land / terrain context / pie menu (viewer-land-context-menu): the
    // reference land entry set and its dispatch, opened by right-clicking bare
    // terrain (the shared resolver lives with the avatar menu).
    .add_plugins(LandMenuPlugin)
    // The custom material every prim/mesh/rigged/avatar/media face renders
    // through (per-map UV transforms + legacy Blinn-Phong specular; inert where
    // unused). Registered once here — and *before* the editor plugins below,
    // whose `FromWorld` resources (the selection highlight / face-cursor overlay
    // materials) build against `Assets<FaceMaterial>` at plugin-build time.
    .add_plugins(crate::audio::AudioPlugin)
    // The shared sound-asset fetch/decode/cache (viewer-in-world-sounds,
    // viewer-ui-sound-effects) and the in-world spatial-sound producer that
    // feeds the mixer's Sfx bus (llTriggerSound one-shots + attached sounds).
    .add_plugins(crate::sound_cache::SoundCachePlugin)
    .add_plugins(crate::world_sounds::WorldSoundsPlugin)
    // The viewer's own 2-D UI feedback sounds on the mixer's UI bus
    // (viewer-ui-sound-effects): the typing chirp, money up/down, teleport,
    // snapshot shutter — raised as PlayUiSound messages by their surfaces.
    .add_plugins(crate::ui_sounds::UiSoundsPlugin)
    // Amortise the sun's shadow-caster visibility cull over several frames
    // (viewer-perf-pbr-shadow-cluster-rez): replace Bevy's per-frame
    // check_dir_light_mesh_visibility with a round-robin one.
    .add_plugins(crate::shadow_visibility::ShadowVisibilityPlugin)
    .add_plugins(crate::face_material::SlFaceMaterialPlugin)
    // The build tool (viewer-object-edit-floater-shell): the Build Tools
    // floater, the edit-mode switch, and the numeric transform fields.
    .add_plugins(EditToolPlugin)
    // The parameter tabs (viewer-prim-parameter-editing): the Object-tab
    // name / description / flag / shape editors and the Features-tab
    // material / flexi / light editors.
    .add_plugins(crate::edit_params::EditParamsPlugin)
    // The Texture tab (viewer-prim-texture-editing) + Select Face tool
    // (viewer-edit-face-selection): per-face colour / transparency / glow /
    // bump / shiny / mapping and texture repeats / offset / rotation.
    .add_plugins(crate::edit_texture::EditTexturePlugin)
    // The Blinn-Phong normal / specular maps + PBR (GLTF) material channels of
    // the Texture tab (viewer-face-materials-pbr).
    .add_plugins(crate::edit_material::EditMaterialPlugin)
    // The Content tab + standalone Object Contents floater
    // (viewer-prim-inventory-editing): the prim task-inventory list, its
    // per-object cache, and the add / remove / rename / copy-out actions.
    .add_plugins(crate::edit_contents::EditContentsPlugin)
    // The notecard viewer & editor floater (viewer-notecard-editor): open a
    // notecard from inventory, read it, edit its text when the item is
    // modifiable, and save it back to agent inventory. Embedded items are
    // listed (inline clickable rendering waits on the rich-text widget).
    .add_plugins(crate::edit_notecard::EditNotecardPlugin)
    .add_plugins(crate::notecard_render::NotecardRenderPlugin)
    .add_plugins(crate::edit_wearable::EditWearablePlugin)
    .add_plugins(crate::edit_material_asset::EditMaterialAssetPlugin)
    // The LSL script editor floater (viewer-lsl-editor-save-compile): open a
    // script from agent or task inventory, read it, edit its source when
    // modifiable, and save it back — which the simulator compiles, its result
    // surfaced as a status line and a diagnostics list (syntax highlighting
    // waits on the rich-text widget).
    .add_plugins(crate::edit_script::EditScriptPlugin)
    // Offscreen material-on-a-sphere previews for the PBR render-material swatch
    // and the material picker's preview pane (viewer-material-swatch-sphere-preview).
    .add_plugins(crate::material_preview::MaterialPreviewPlugin)
    // The Create tool (viewer-prim-creation): the create panel's base-type
    // picker and the click-to-rez placer for prims / trees / grass.
    .add_plugins(crate::edit_create::EditCreatePlugin)
    // The object selection core (viewer-object-selection-core): click /
    // rubber-band selection, the selection set + highlight, and the
    // ObjectSelect / ObjectDeselect / ObjectProperties wire sync.
    .add_plugins(EditSelectionPlugin)
    // The transform gizmos (viewer-transform-gizmos): move / rotate / stretch
    // manipulators over the selection, sending MultipleObjectUpdate edits.
    .add_plugins(EditGizmoPlugin)
    // Prim linking / unlinking (viewer-prim-linking): Ctrl+L / Ctrl+Shift+L
    // and the Build menu, sending ObjectLink / ObjectDelink with the
    // last-selected object as the linkset root.
    .add_plugins(crate::edit_link::EditLinkPlugin)
    // Object-edit undo / redo (viewer-build-undo-redo): Ctrl+Z / Ctrl+Y and
    // the Build menu, sending the server-side Undo / Redo for the selection.
    .add_plugins(crate::edit_undo::EditUndoPlugin)
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
    .add_plugins(AboutLandmarkPlugin)
    .add_plugins(AvatarPickerPlugin)
    // The avatar profile floater (viewer-social-profiles): 2nd Life / Web /
    // Picks / Classifieds / 1st Life / Notes, opened from the avatar pie's
    // Profile slice and the People list, editable for one's own profile.
    .add_plugins(AvatarProfilePlugin)
    // The web-media engine (viewer-media-prim-browser): offscreen Chromium
    // (sl-cef) pumped on the main thread, one surface per embedded page. The
    // consumers below (browser widget / floater, media-on-a-prim, controls
    // bar) all no-op when it is disabled or failed to start.
    .add_plugins(crate::media_engine::MediaEnginePlugin {
        enabled: media.web,
        video_enabled: media.video,
    })
    // The Second Life website auto-login (viewer-web-openid-auth): at login,
    // POST the login response's OpenID token off-thread and inject the reply's
    // session cookie into the shared browser context, so the web surfaces
    // below open already signed in. No-op off Second Life or with
    // `--no-web-auth`.
    .add_plugins(crate::web_auth::WebAuthPlugin {
        enabled: media.web && media.web_auth,
    })
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
    // Recovers the real DNS / TCP / TLS / HTTP reason a media stream failed,
    // which GStreamer's souphttpsrc hides — shared by the parcel-audio and
    // media-on-a-prim consumers below.
    .add_plugins(crate::media_diagnostics::MediaDiagnosticsPlugin)
    // Media-on-a-prim (LLViewerMedia / LLViewerMediaFocus): ObjectMedia data
    // driving per-face surfaces, world input routing and the focus model.
    .add_plugins(crate::media_prim::MediaPrimPlugin)
    // The floating media controls bar above the media face under the cursor
    // (LLPanelPrimMediaControls).
    .add_plugins(crate::media_controls::MediaControlsPlugin)
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
    // the shared substrate the specific dialogs sit in. The live source
    // (`ingest_alert_messages`) and the `SL_VIEWER_NOTIFICATION_DEMO` trigger are
    // added below as viewer-only systems, since the plugin itself must host
    // without the session `SlEvent` stream (so the login-free gallery can use it).
    .add_plugins(NotificationHostPlugin)
    // The persistent-notification store (viewer-notification-persistence): saves
    // the open (unacknowledged) sticky notifications to a per-account file and
    // re-displays them on next login (the reference LLPersistentNotificationStorage).
    // After the host, whose PersistNotification / NotificationResponse it records.
    .add_plugins(NotificationPersistPlugin)
    // Surface the simulator's `AlertMessage` / `AgentAlertMessage` (a stream
    // nothing consumed before) as notifications. The `SL_VIEWER_NOTIFICATION_DEMO`
    // sample spread is registered conditionally with the other env-gated debug
    // systems below.
    .add_systems(Update, ingest_alert_messages)
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
    // The Experiences floater (viewer-experience-permission-dialog): the manage
    // surface listing the agent's allowed / blocked experiences with a per-row
    // Forget (Command::SetExperiencePermission Forget); opened from the Avatar
    // menu. After FloaterPlugin, whose spawn_floater it builds on.
    .add_plugins(ExperiencesPlugin)
    // The offers & invites toast host (viewer-dialog-offers-invites): pops an
    // accept / decline card when the grid throws an inventory offer, a teleport
    // lure, a friendship offer or a group-membership invitation over IM, wiring
    // each to its protocol reply (AcceptInventoryOffer / AcceptTeleportLure /
    // AcceptFriendship / AcceptGroupInvitation and the matching declines). After
    // NotificationHostPlugin, whose shared channel it adopts its card into, and
    // InventoryPlugin, whose folders the accept replies file into.
    .add_plugins(OffersInvitesPlugin)
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
    // Per-user floater geometry (viewer-ui-floater-persist-geometry): remember
    // each floater's position, size, minimized / docked state and open / closed
    // state across sessions, in the per-avatar account settings.
    .add_plugins(FloaterPersistPlugin)
    .add_plugins(ParticleRenderPlugin)
    .add_plugins(TerrainMaterialPlugin)
    // In-world parcel borders / property lines (viewer-parcel-borders-render):
    // colour-coded vertical bands draped along parcel boundaries, driven by the
    // `parcel_borders` module's system below.
    .add_plugins(crate::parcel_borders::ParcelBordersPlugin)
    // The in-world tracking beacon (viewer-beacons-beam-render): the vertical
    // beam + label + off-screen arrow drawn at the tracked position from the
    // shared `MapTracking` resource.
    .add_plugins(crate::beacons::BeaconPlugin)
    // The world-space avatar name-tag billboards (viewer-name-tags-billboard-
    // render): the embedded billboard shader + material pipeline; the tag
    // systems themselves register with the avatar systems below.
    .add_plugins(crate::name_tag_billboard::NameTagBillboardPlugin)
    // Object floating text (`llSetText`) reuses the name-tag billboard renderer
    // with its own fade registry + lifetime map (viewer-hover-text).
    .add_plugins(crate::hover_text::HoverTextPlugin)
    // In-world hover tooltips over objects / avatars / land (viewer-hover-tooltips).
    .add_plugins(crate::hover_tooltip::HoverTooltipPlugin)
    // Shared object land-impact model (GetObjectCost), read by the hover tooltip
    // and the build floater.
    .add_plugins(crate::object_cost::ObjectCostPlugin)
    // The atmospheric sky dome material (P22.2), driven from the region's EEP
    // environment by the `sky` module's systems below.
    .add_plugins(SkyMaterialPlugin)
    // The sun / moon disc billboard material (P22.3), driven alongside the sky.
    .add_plugins(SunDiscMaterialPlugin)
    // The scrolling cloud-layer material (P22.4), driven alongside the sky.
    .add_plugins(CloudMaterialPlugin)
    // The night-time star-field material (P22.5), driven alongside the sky.
    .add_plugins(StarMaterialPlugin)
    // The water-surface material (P23.1), driven from the region's EEP water
    // settings by the `water` module's systems below.
    .add_plugins(WaterMaterialPlugin)
    // Water-relative transparency ordering (viewer-particle-water-ordering): a
    // render-world re-sort of the transparent phase so translucent content (a
    // fountain's spray, translucent prims) orders correctly against the
    // depth-writing water surface — below-water draws through it, above-water over
    // it — rather than being painted out by the camera-following plane.
    .add_plugins(crate::transparency::TransparencyOrderPlugin)
    // The underwater-fog post-process (P23.1): a fullscreen depth-based pass that
    // fogs everything below the water surface (reference `getWaterFogView`).
    .add_plugins(UnderwaterFogPlugin)
    // The reference viewer's dynamic exposure (`generateExposure` / `exposureF`):
    // a fullscreen pass that reduces the composited scene's average luminance to a
    // 1×1 exposure map the tone mapper multiplies in, and the `sky_hdr_scale`
    // counterweight that keeps an EEP sky from washing out. Runs after the fog /
    // glow, before the tone mapper.
    .add_plugins(SlExposurePlugin)
    // The reference viewer's tone mapper (P33.3): the one transfer from the linear
    // HDR scene to displayable colour, over the whole composited frame (reference
    // `postDeferredTonemap` — ACES / Khronos Neutral, blended by `RenderTonemapMix`).
    // Runs after the fog, which the reference likewise applies in linear space.
    .add_plugins(SlTonemapPlugin)
    // The reference viewer's glow (`generateGlow` / `combineGlow`): the faithful
    // alpha-mask separable-Gaussian glow, replacing Bevy `Bloom`. Runs after the
    // tone mapper, as the reference does. Disabled by default until the materials
    // write the glow mask into their alpha (see `glow.rs`); the Bevy `Bloom` above
    // stays active meanwhile.
    .add_plugins(SlGlowPlugin)
    // The GPU-avatar keystone spike (context/gpu-avatars.md §2.4 / §9.1 risk 1):
    // flag-gated by SL_VIEWER_GPU_AVATAR_SPIKE (`identity` | `marker`), read once
    // here. Unset (the default), this is a no-op plugin and the viewer is
    // byte-for-byte the normal path. Set, a compute pass overwrites one skinned
    // mesh's palette range inside Bevy's SkinUniforms buffer every frame — the
    // de-risking experiment for writing GPU-posed palettes into Bevy's own skin
    // path. Not a feature; delete or graft into Phase 1.
    .add_plugins(crate::gpu_avatar_spike::GpuAvatarSpikePlugin::from_env())
    // The GPU-avatar pose pipeline (context/gpu-avatars.md §1/§2, Phases
    // 1a+1b): a compute pipeline re-runs the SL skeletal recurrence on the
    // GPU and writes the skin palettes into Bevy's SkinUniforms buffer. The
    // in-place path is the DEFAULT on a capable device (compute + storage
    // buffers, checked once at startup with an automatic legacy-CPU
    // fallback); SL_VIEWER_GPU_AVATARS overrides: `cpu`/`off` forces the
    // legacy CPU pose path, `ghost` the Phase 1a side-by-side comparison
    // harness (CPU in place + GPU-FK ghost 2 m aside). Env read once here.
    .add_plugins(crate::gpu_avatars::GpuAvatarsPlugin::from_env())
    // GPU ID-buffer picking (Phase 3): the cursor pick is a render, not a
    // ray cast — pixel-perfect against exactly what is drawn, GPU-posed
    // avatars included.
    .add_plugins(crate::gpu_pick::GpuPickPlugin)
    // The client-side physics foundation (P31.1): an avian3d physics world with
    // Second Life gravity, a fixed timestep at the sim's target rate, and
    // region-time-dilation scaling — reused by Phase 32 (flexi) and Phase 34
    // (avatar physics).
    .add_plugins(PhysicsPlugin)
    // The reflection-probe pipeline (P33): captures a scene environment cubemap and
    // binds it as image-based lighting — a default (global) probe on the main view,
    // the scene-render half Bevy's env-map filter / consumer expect but never
    // produce.
    .add_plugins(ReflectionProbePlugin)
    // The HUD layer (P35.1): the HUD screen puts its whole subtree — the routed
    // attachments and their faces — on `HUD_RENDER_LAYER` by propagating a single
    // `RenderLayers` down the hierarchy, so the world camera (default layer) never
    // draws a HUD. Propagation runs before Bevy decides what each camera sees, so a
    // just-routed attachment is layered in the very frame it is parented.
    .add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
    .configure_sets(
        PostUpdate,
        PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility),
    )
    // Gate bevy_ui's unconditional full-tree stack rebuild and layout walk
    // behind "did any of that system's inputs actually change (visibly)"
    // (viewer-perf-ui-layout-per-frame-relayout); each gated system is its
    // set's sole member, so this needs no fork. The conditions and their
    // rationale live in `crate::ui_perf`.
    .configure_sets(
        PostUpdate,
        bevy::ui::UiSystems::Stack.run_if(ui_perf::ui_stack_dirty),
    )
    .configure_sets(
        PostUpdate,
        bevy::ui::UiSystems::Layout.run_if(ui_perf::ui_layout_dirty),
    )
    // `SL_VIEWER_LOG_UI_DIRTY=1` names what tripped the layout gate per frame.
    .add_plugins(ui_perf::UiPerfDiagnosticsPlugin)
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
    app
        // P24.1: a larger sun/moon shadow map than the 2048 default, so the four
        // region-scale cascades (see `sky::shadow_cascades`) keep enough texels per
        // world unit to shadow an avatar crisply across a whole region.
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .init_resource::<ViewerSession>()
        // The per-avatar account identity (grid + name + accounts root), used by
        // `load_account_settings` to locate the account-scope settings once the
        // agent UUID is known at login.
        .insert_resource(AccountContext {
            accounts_base: config_accounts_base,
            grid,
            avatar,
        })
        // The viewer settings store (viewer-ui-settings-store), the reference's
        // `gSavedSettings`: registers each feature's settings and loads any persisted
        // global overrides (e.g. SpaceNavigator sensitivities). The per-avatar account
        // scope loads at login via `load_account_settings`.
        .init_resource::<ViewerSettings>()
        // The debug camera override (`--camera-position` / `--camera-look-at` /
        // `--camera-spin`): `setup_scene` reads the start pose, `drive_flycam` reads
        // the spin, and third-person auto-follows when no pose is fixed. The world
        // context may grab the cursor (only in mouselook) unless this is an unattended
        // screenshot run, whose whole point is to leave the desktop's pointer alone.
        .insert_resource(CursorGrabAllowed(screenshot_dir.is_none()))
        .insert_resource(camera_start)
        .insert_resource(camera_spin)
        .init_resource::<LoginOutcome>()
        .init_resource::<EnvironmentState>()
        // The live A/B state of the shape's collision-volume displacement (P34.3), seeded
        // from `SL_VIEWER_VOLUME_MORPH_GAIN` and toggled by the `V` key.
        .init_resource::<VolumeMorphGain>()
        .init_resource::<TerrainState>()
        .init_resource::<PendingPatchRebuilds>()
        // One shared per-frame mesh-upload lane spent by object spawn / geometry /
        // LOD / terrain apply (replaces their old independent budgets).
        .init_resource::<MeshUploadBudget>()
        .init_resource::<crate::terrain::CurrentTerrainLighting>()
        .init_resource::<ObjectState>()
        .init_resource::<PendingObjectEvents>()
        .init_resource::<RiggedBindSkipLog>()
        .init_resource::<PendingDecodedMeshes>()
        .init_resource::<PendingDecodedSculpts>()
        // The screen-space HUD hierarchy (P35.1), spawned by `setup_hud_screen`.
        .init_resource::<HudState>()
        // The water-render bookkeeping (P23.1) is created by `setup_water` at
        // startup, so no `init_resource` is needed here; the surface level the
        // underwater-fog pass reads is a small resource published by `drive_water`.
        .init_resource::<WaterLevel>()
        .init_resource::<PrimLodTargets>()
        .init_resource::<TreeLodTargets>()
        // The cross-instance geometry cache: shared mesh handles for identical
        // prim / sculpt / mesh geometry (`viewer-perf-prim-tessellation-cache`).
        .init_resource::<geometry_cache::GeometryCache>()
        // The cross-instance material cache: shared face-material handles for
        // identical face content, so matched copies batch into instanced draws
        // (`viewer-perf-material-intern`).
        .init_resource::<material_cache::MaterialCache>()
        .init_resource::<LocalLights>()
        .init_resource::<ParticleSim>()
        .init_resource::<AvatarState>()
        .init_resource::<AppearanceApplyBudget>()
        .init_resource::<mutes::MuteModel>()
        .init_resource::<name_tag_content::NameTagStatuses>()
        .init_resource::<AvatarRuntimeMorphs>()
        .init_resource::<look_at::LookAtTargets>()
        .init_resource::<look_at::LookAtMotion>()
        .init_resource::<reach::PointAtTargets>()
        .init_resource::<reach::PointAtSelection>()
        .init_resource::<reach::ReachMotion>()
        .init_resource::<body_physics::BodyPhysicsMotion>()
        .init_resource::<hand_pose::HandPoseMotion>()
        .init_resource::<locomotion_ik::LocomotionAdjust>()
        .init_resource::<ground::AvatarGround>()
        .init_resource::<AvatarControls>()
        .init_resource::<movement::MovementTuning>()
        .init_resource::<TypingState>()
        .init_resource::<ControlAvatarState>()
        .init_resource::<ChatOverlay>()
        .init_resource::<TextureManager>()
        .init_resource::<PrimTextures>()
        .init_resource::<TextureApplyBudget>()
        .init_resource::<DeferredFaceTextures>()
        .insert_resource(MaterialManager::new())
        .init_resource::<LegacyMaterialManager>()
        .init_resource::<BumpManager>()
        .init_resource::<AvatarBakeMaterials>()
        .init_resource::<OwnLocalBake>()
        .init_resource::<ServerBakeState>()
        .init_resource::<MeshManager>()
        .init_resource::<OwnBakeInputs>()
        .init_resource::<OwnBakePublish>()
        .init_resource::<WearableAssetManager>()
        .insert_resource(AnimationManager::new(viewer_assets.map(Path::to_path_buf)))
        .init_resource::<AnimationPlayback>()
        .init_resource::<environment_assets::EnvironmentAssetManager>()
        .insert_resource(PipelineOverlayVisible::from_env())
        // The UI text & font foundation demo (viewer-ui-text-foundation): a
        // toggleable `EditableText` panel, seeded shown/hidden from
        // `SL_VIEWER_TEXT_DEMO` so the screenshot harness can capture it.
        .insert_resource(TextDemoVisible::from_env())
        // The reusable text-input widget demo (viewer-ui-text-input-widget): a
        // toggleable panel of single- / multi-line and numeric fields, seeded
        // shown/hidden from `SL_VIEWER_TEXT_INPUT_DEMO` for the screenshot harness.
        .insert_resource(TextInputDemoVisible::from_env())
        .insert_resource(PlayOnLogin {
            animations: play_animation
                .iter()
                .copied()
                .map(AnimationKey::from)
                .collect(),
            repeat: repeat_animation,
        })
        .add_message::<TextureDecoded>()
        .add_message::<MeshDecoded>()
        .add_message::<WearableAssetFetched>()
        .add_message::<RefetchAvatarTextures>()
        .add_message::<crate::chat::LocalChatNotice>()
        // Menu ▸ Quit writes this; `handle_quit_requests` turns it (and a window
        // close) into a graceful logout.
        .add_message::<crate::session::QuitRequested>()
        // The pie-menu widget's `commit_pie_selection` runs every frame and writes a
        // `UiAction`, so the message must be registered here too — it was previously
        // only registered in the gallery / test apps, where the pie menu had been
        // exercised, so the live viewer panicked on the unregistered writer.
        .add_message::<UiAction>()
        .add_systems(
            Startup,
            (
                setup_scene,
                setup_sky,
                setup_sun_moon_discs,
                setup_clouds,
                setup_stars,
                setup_water,
                // The water-exclusion mask camera + render target
                // (`viewer-water-exclusion`); its mask is bound into the water
                // material by `bind_water_exclusion_mask` once both exist.
                setup_water_exclusion,
                // The chat overlay now parents itself under the scaffold's
                // `UiRoot` (so the snapshot include-UI-off hide covers it), and so
                // must see the root.
                setup_chat_overlay.after(UiScaffoldSystems::SpawnRoot),
                setup_pipeline_overlay,
                // The UI text & font foundation demo panel (viewer-ui-text-foundation),
                // which parents itself to the scaffold's `UiRoot` and so must see it.
                setup_text_demo.after(UiScaffoldSystems::SpawnRoot),
                // The reusable text-input widget demo panel (viewer-ui-text-input-widget),
                // likewise parented to the scaffold's `UiRoot`.
                setup_text_input_demo.after(UiScaffoldSystems::SpawnRoot),
                setup_avatar_body,
                // P35.1: the screen-space HUD screen + its attachment-point nodes, which
                // a worn HUD is routed onto instead of a body joint.
                setup_hud_screen,
                // P30.2: upload the procedural default particle sprite.
                setup_particles,
                // GPU particles (viewer-perf-gpu-particles): upload the one shared
                // unit-quad mesh every cloud instances.
                setup_particle_quad,
            ),
        )
        // The material cache's copy-on-write detach net: give any interned
        // (shared-material) face a private material before this frame's
        // `Update` mutators — texture animation, PBR registration, HUD
        // fullbright, the edit floaters' live previews — can write into the
        // shared asset. Scheduled in `PreUpdate` so the swap's commands are
        // applied at the schedule boundary, ahead of every mutator.
        .add_systems(PreUpdate, material_cache::detach_shared_face_materials)
        // Refill the shared per-frame asset-upload budgets in `PreUpdate`, ahead of
        // every `Update` apply system that spends from them — the image lane
        // (`TextureApplyBudget`, drawn by the texture / PBR-map / bump / legacy / bake
        // systems) and the mesh lane (`MeshUploadBudget`, drawn by object spawn /
        // geometry / LOD / terrain). Resetting here rather than inside the scattered
        // Update tuples guarantees the refill precedes all consumers regardless of
        // their relative order.
        .add_systems(
            PreUpdate,
            (reset_texture_apply_budget, reset_mesh_upload_budget),
        )
        .add_systems(
            Update,
            (
                capture_login_outcome,
                drive_session,
                // Announce the (user-tunable) draw distance on handshake and
                // whenever the quick-preferences slider moves it.
                apply_draw_distance,
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
                // finished fetches before the consumers that apply them.
                update_texture_caps,
                poll_textures,
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
                // Scene re-base / purge on a region change, then fold terrain +
                // object events. Nested into one tuple to stay within Bevy's
                // per-tuple system limit.
                (
                    // A distant teleport purged the session's world; despawn the
                    // stale scene mirror (objects / avatars / terrain) before the
                    // recenter systems, so each re-anchors on the destination
                    // without a spurious shift. A crossing / neighbour teleport
                    // keeps the world (no-op).
                    scene_reset::reset_scene_on_world_reset
                        .before(recenter_terrain)
                        .before(recenter_objects)
                        .before(recenter_avatars),
                    // Recenter (origin follows the root region) before folding
                    // terrain events, so patches are placed on the current origin;
                    // then drain a few of the queued seam / whole-region patch
                    // rebuilds (`PendingPatchRebuilds`).
                    (recenter_terrain, update_terrain, drain_patch_rebuilds).chain(),
                    // Re-base world-root objects onto the new origin (a crossing or
                    // a teleport to an already-connected region) before folding
                    // object events, so a static object stays put and a new object
                    // is placed against the current origin. Chained after the
                    // terrain recenter so it re-bases to the same authoritative root.
                    (recenter_objects, update_objects).chain(),
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
                        .chain(),
                    // The mute list (name-tag colouring + future block-list
                    // UI): request once at session-up, ingest the Xfer'd
                    // list, and mirror locally-issued mutes.
                    (
                        mutes::request_mute_list,
                        mutes::ingest_mute_list,
                        mutes::note_local_mutes,
                    ),
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
                            .after(crate::animations::drive_avatar_skeletons)
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
                // Object floating text (`llSetText`, viewer-hover-text): reap
                // billboards whose object cleared its text or despawned, then
                // (re)compose the rest from the mirrored `ObjectFloatingText`.
                // The PostUpdate world-text chain lays out the changed content.
                (
                    hover_text::despawn_removed_hover_text.after(update_objects),
                    hover_text::sync_object_hover_text.after(update_objects),
                )
                    .chain(),
                apply_avatar_names,
                // Re-shape each rigged body from its avatar's visual params — morph
                // targets (P13.3) and skeletal proportions (P13.4) — show/hide whole
                // base regions from the worn skirt / mesh-body items (P13.5), then
                // fetch each avatar's server-published baked textures (P14.1) and
                // drape them over the matching body regions (P14.2), filling each
                // region material once its bake decodes. When the grid publishes no
                // server bake for our own avatar (OpenSim), drape the locally
                // composited client-side bake (P15.3) over the regions it did not bake,
                // after the server-bake assignment so a real bake still wins. Nested
                // into one tuple to stay within Bevy's per-tuple system limit.
                (
                    apply_avatar_appearance,
                    // Drive the per-frame runtime morph params (eye blink, body
                    // physics) into each part's `MeshMorphWeights` (P31.12a), after
                    // the appearance rebuild has (re)seeded those components.
                    apply_avatar_runtime_morphs.after(apply_avatar_appearance),
                    // Render our own avatar from its worn shape, not the server's echo
                    // of our own last publish (R12); after `apply_avatar_appearance`
                    // so it overrides a just-stored server appearance.
                    apply_own_shape_from_wearables.after(apply_avatar_appearance),
                    apply_avatar_part_visibility,
                    ingest_avatar_bakes,
                    // The avatar pies' manual Tex Refresh: re-issue an agent's bake
                    // fetches, before assignment so a refreshed bake is picked up
                    // the same frame it re-decodes.
                    handle_refetch_avatar_textures,
                    assign_avatar_bake_materials,
                    apply_avatar_bake_textures,
                    apply_own_local_bake.after(assign_avatar_bake_materials),
                    // Point each worn bake-on-mesh (BoM) rigged face at its wearer's
                    // baked region material (P17.3), after both bake-assignment paths
                    // have settled the region materials this frame.
                    apply_bom_face_materials
                        .after(assign_avatar_bake_materials)
                        .after(apply_own_local_bake),
                    // Publish our own client-side bake to the grid (P15.4): encode +
                    // upload each composited region over `UploadBakedTexture`, then
                    // advertise them in an `AgentSetAppearance` (OpenSim-only path).
                    drive_bake_publish,
                ),
                // Append newly received local chat to the on-screen overlay, age each
                // line so it fades and despawns once chat goes quiet
                // (viewer-chat-overlay-fade), and keep the overlay pinned just above the
                // bottom area (toolbar + nearby-chat bar) so they never overlap as the
                // bar grows / shrinks / toggles.
                (
                    update_chat_overlay,
                    tick_chat_overlay,
                    restyle_chat_overlay,
                    position_chat_overlay,
                ),
                // Quit handling: request a clean logout on the quit key, then force the
                // exit once the grace period lapses. Nested into one tuple to stay
                // within Bevy's per-tuple system limit. Only the key half is gated on
                // the input context — `Q` is a character a text field wants, and
                // `Escape` there means "give the keyboard back" (see
                // `input_context`) — while the deadline must still fire once a quit is
                // under way, whatever has focus.
                (
                    handle_quit_input.run_if(world_has_keyboard),
                    // Menu ▸ Quit and the window close button / compositor close
                    // both route through a graceful logout here.
                    handle_quit_requests,
                    enforce_quit_deadline,
                    // Load the per-avatar account settings once the agent UUID is
                    // known at login (once; a no-op every frame thereafter).
                    load_account_settings,
                    // Persist the settings store when a logout is requested.
                    save_settings_on_logout,
                ),
                // Walk / turn / fly the own avatar from the movement actions
                // (viewer-input-action-map): the simulator moves the avatar and the
                // P31.4 dead-reckoner smooths the returned motion. The camera itself is
                // driven by `CameraPlugin`. Actions are already gated on focus by the
                // action map, so no `run_if` is needed here.
                drive_avatar_controls,
            ),
        )
        // The crosshair pick tool (press `P`) to identify the object under the
        // centre of the screen. Separate calls to stay clear of Bevy's per-tuple
        // system limit. (The SL_VIEWER_LOG_OBJECTS diagnostic is registered
        // conditionally with the other env-gated debug systems below.)
        .add_systems(
            Update,
            (
                pick_object.run_if(world_has_keyboard),
                // The screen-space HUD (P35.2): keep each HUD point anchored to its
                // corner of the viewport as the window's aspect changes, and render every
                // HUD face fullbright (the reference forces `LLFace::FULLBRIGHT` on a HUD
                // attachment; here a lit one would also render black, since the world's
                // sun is not on the HUD layer).
                (fit_hud_points, apply_hud_fullbright),
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
                // Key-toggled texture/mesh pipeline-status panel (P19.3): flip its
                // resource on the toggle key, then drive the panel's visibility and
                // (while shown) its text from the live store snapshots.
                toggle_pipeline_overlay,
                update_pipeline_overlay
                    .run_if(pipeline_overlay_active)
                    .after(toggle_pipeline_overlay),
                // UI text & font foundation (viewer-ui-text-foundation): toggle /
                // apply the demo panel's visibility (the F4 key). Nested into one
                // tuple to stay within Bevy's per-tuple system limit.
                (
                    toggle_text_demo,
                    apply_text_demo_visibility
                        .run_if(resource_changed::<TextDemoVisible>)
                        .after(toggle_text_demo),
                ),
                // Reusable text-input widget (viewer-ui-text-input-widget): toggle /
                // apply the demo panel's visibility (the F8 key), and keep the numeric
                // rows' live parsed-value read-outs current.
                (
                    toggle_text_input_demo,
                    apply_text_input_demo_visibility
                        .run_if(resource_changed::<TextInputDemoVisible>)
                        .after(toggle_text_input_demo),
                    update_demo_value_readouts.run_if(crate::ui_text_input::text_input_demo_active),
                ),
                // Local lights (P25.2): render the nearest / brightest light-flagged
                // prims as Bevy point / spot lights, after the fly-camera so the
                // distance-based budget selection uses the current viewpoint.
                drive_local_lights.after(position_camera),
                // Particles (P30.2): advance each source's CPU particle simulation and
                // rebuild its camera-facing billboard mesh, after the fly-camera so the
                // billboards face the current viewpoint.
                drive_particles.after(position_camera),
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
        )
        // Atmospheric sky (P22.2): keep the sky dome centred on the camera, then fold
        // the region environment + camera altitude into the sky material, the sun /
        // moon directional light, and the ambient light, and swap each decoded sky
        // overlay texture into the material. Run after the fly-camera so the dome
        // tracks the current viewpoint.
        .add_systems(
            Update,
            (
                center_sky_on_camera.after(position_camera),
                drive_sky.after(position_camera),
                apply_sky_textures,
                // Terrain lighting (viewer-clouds-sun-occlusion): drive each region's
                // ground with the sky frame's atmospheric sun / ambient colours, like
                // the reference legacy terrain, after the camera so it reads the
                // current altitude's sky frame.
                crate::terrain::drive_terrain_lighting.after(position_camera),
                // Sun / moon discs (P22.3): aim and colour the billboards from the same
                // active sky frame (after the fly-camera, so they track the viewpoint),
                // then swap each decoded disc texture into its material.
                drive_sun_moon_discs.after(position_camera),
                apply_disc_textures,
                // Cloud layer (P22.4): fold the same active sky frame into the cloud
                // material, accumulate the scroll, and swap in the decoded cloud noise.
                drive_clouds.after(position_camera),
                apply_cloud_textures,
                // Star field (P22.5): centre / rotate the field on the camera, fade it
                // in with the active sky frame's `star_brightness`, and swap in the
                // decoded bloom texture.
                drive_stars.after(position_camera),
                apply_star_textures,
                // Water surface (P23.1): learn each region's water height, then centre
                // the endless ocean on the camera and place a per-region plane where a
                // neighbour's sea level differs, fold the EEP water settings into the
                // shared material (after the fly-camera, so the ocean tracks the
                // viewpoint), and swap in the decoded wave normal map.
                update_water,
                drive_water.after(position_camera),
                apply_water_textures,
                // Water-exclusion surfaces (`viewer-water-exclusion`): route faces
                // textured with the invisiprim-successor sentinel onto the mask
                // layer, slave the mask camera to the main view (after the
                // fly-camera so the mask lines up with what the water samples it
                // against), and bind the finished mask into the water material.
                convert_water_exclusion_faces,
                sync_water_exclusion_camera.after(position_camera),
                bind_water_exclusion_mask,
                // Underwater fog (P23.1): refresh the camera's fog parameters (water
                // level, EEP fog colour/density, reconstruction matrix) each frame,
                // after the fly-camera so the matrix matches the current viewpoint.
                update_underwater_fog
                    .after(position_camera)
                    .after(drive_water),
            ),
        )
        // Animations: keep the animation store's `ViewerAsset` cap current, request a
        // motion for every animation each nearby avatar is playing, and fold finished
        // resolves into the shared motion cache (P18.2); then drive each rigged
        // avatar's skeleton from its playing motions, overlaying the sampled keyframe
        // poses onto the appearance rest pose (P18.3, so after `apply_avatar_appearance`).
        .add_systems(
            Update,
            (
                update_animation_caps,
                // Refresh the EEP settings-asset fetch cap and drain finished
                // fetch+decode tasks for the World ▸ Environment Modern presets.
                environment_assets::update_environment_asset_caps,
                environment_assets::poll_environment_assets,
                ingest_avatar_animations,
                poll_animations,
                // Client-side locomotion / state animations for the own avatar (P31.6):
                // derive its movement state from the P31.4 velocity + P31.5 controls and
                // play the matching built-in animation when the simulator is silent about
                // it. After the controls (so it reads the freshly advertised intent) and
                // before the skeleton driver (so its client-driven set is reconciled into
                // the same frame's pose).
                drive_own_locomotion
                    .after(drive_avatar_controls)
                    .before(drive_avatar_skeletons)
                    .run_if(world_has_keyboard),
                // Typing state animation for the own avatar (P31.9): reconcile the typing
                // state the nearby-chat bar drives, play `ANIM_AGENT_TYPE` locally, and
                // broadcast a `StartTyping` / `StopTyping` `ChatFromViewer`. Not gated on
                // `world_has_keyboard` — typing happens while the *chat field* holds the
                // keyboard (the TextEntry context), so that gate would suppress it. Like
                // locomotion it must reconcile its client-driven set before the skeleton
                // driver folds it into the frame's pose.
                drive_own_typing.before(drive_avatar_skeletons),
                drive_avatar_skeletons.after(apply_avatar_appearance),
                // Hand-pose morph (P31.13): cross-fade each avatar's hands into the pose
                // its highest-priority playing animation asks for. After the skeleton
                // driver (whose playing set it reads) and before the runtime-morph fold,
                // so the cross-faded weights reach the GPU in the same frame.
                hand_pose::drive_hand_poses
                    .after(drive_avatar_skeletons)
                    .before(apply_avatar_runtime_morphs),
                report_camera_interest,
                report_agent_viewport,
                // Head & eye look-at tracking (P31.12): derive the own avatar's look-at
                // target from the fly-camera, and ingest nearby avatars' `ViewerEffect`
                // look-at gaze hints. The pose pass (PostUpdate) reads both.
                look_at::update_own_look_at_target,
                look_at::receive_look_at_effects,
                // Activity-driven reach & aim (P31.15): the own avatar's object selection
                // (the E key) and the point-at effect it publishes, other avatars' point-at
                // effects, and the G key that plays an aim animation through the simulator
                // so the targeting motion engages the way a scripted weapon would drive it.
                // The pose pass (PostUpdate) reads the resulting targets.
                (
                    reach::select_object_under_crosshair.run_if(world_has_keyboard),
                    reach::drive_own_point_at.after(reach::select_object_under_crosshair),
                    reach::receive_point_at_effects,
                    reach::drive_aim_animation.run_if(world_has_keyboard),
                ),
                // Avatar ground probe (P31.14): resolve what is under each avatar's root
                // and ankles — the terrain land height combined with the simulator's
                // collision (foot) plane, as the reference viewer's `getGround` does — for
                // the foot IK and the landing recovery. It reads the ankle joint globals
                // the pose pass wrote *last* frame.
                ground::probe_avatar_ground,
                // Animesh (P29): request each animated object's animation motions, drive
                // its control-avatar skeleton from them (after its rigged meshes bind in
                // `apply_rigged_attachments`), and drop control avatars whose object is
                // gone (after `update_objects` has processed removals).
                ingest_object_animations,
                drive_control_avatars.after(apply_rigged_attachments),
                // Spawn a control avatar as soon as an animesh has an animation playing
                // (after `drive_control_avatars` folds the `ObjectAnimation` into the
                // playback clock), so a late mesh bind does not lose an early animation.
                spawn_animesh_control_avatars.after(drive_control_avatars),
                prune_control_avatars.after(update_objects),
            ),
        )
        // Write the posed avatars' (and animesh control avatars') animated joint world
        // matrices straight into their `GlobalTransform`s (P18.3 / P29.2), after
        // transform propagation has produced the rest globals this frame — so the
        // animated pose is what skinning / render extraction reads, without the
        // limb-shear a rotation overlaid on the baked-scale local transform would cause.
        .add_systems(
            PostUpdate,
            (
                pose_avatar_skeletons.after(TransformSystems::Propagate),
                // Publish each animesh control avatar's pose slot to the GPU
                // feed (its object world matrix + empty corrections) after
                // propagation, so the GPU samples/blends/FK-poses it in place
                // (§5) — no per-object joint entities remain.
                publish_control_avatars.after(TransformSystems::Propagate),
                // (Worn rigid attachments no longer need a hand re-propagation:
                // their attachment-point node is an avatar-root child whose local
                // `Transform` the pose driver's socket writer sets each frame, so
                // ordinary change-gated propagation seats the worn subtree — the
                // former `pose_attachment_nodes` pass, Phase 4 §5.4.)
                // Object floating text placement (viewer-hover-text): read the
                // object's freshly-propagated world pose and lift the text by
                // 0.6 × the prim's Z scale in world up (the billboard's own
                // transform then propagates next frame — a 1-frame trail on a
                // moving object, imperceptible for the stationary vendors /
                // signs floating text lives on, and never an origin flash).
                hover_text::follow_hover_text.after(TransformSystems::Propagate),
            ),
        )
        // The world-space name-tag billboard chain
        // (viewer-name-tags-billboard-render): materialise changed tag content
        // as text spans, lay the spans out through the shared text pipeline,
        // rebuild changed tag meshes, then place each tag over its avatar
        // anchor (smoothed follow + distance cutoff + preference gates). All
        // before transform propagation so page children inherit this frame's
        // matrix; the layout step runs after the global span-change detector
        // and after camera updates (its scale-factor source).
        .add_systems(
            PostUpdate,
            (
                name_tag_billboard::apply_name_tag_settings,
                hover_text::apply_hover_text_settings,
                name_tag_billboard::sync_tag_spans,
                name_tag_billboard::layout_tag_text
                    .after(bevy::text::detect_text_needs_rerender)
                    .after(bevy::camera::CameraUpdateSystems),
                name_tag_billboard::build_tag_meshes,
                name_tag_billboard::follow_tag_anchors,
                name_tag_billboard::solve_tag_overlap,
                name_tag_billboard::sync_tag_pages,
            )
                .chain()
                .before(TransformSystems::Propagate),
        );
    // Load the client-side avatar assets (if a directory was given) so rigged
    // bodies replace the placeholder spheres; absent them the viewer keeps spheres.
    if let Some(library) = load_avatar_library(viewer_assets) {
        app.insert_resource(library);
    }
    // Env-gated debug / demo systems, registered only when their switch is set
    // (the `capture_screenshots` pattern) — a normal session pays no scheduler
    // dispatch for them at all. Each predicate mirrors the system's own
    // internal env check.
    if std::env::var_os("SL_VIEWER_LOG_OBJECTS").is_some() {
        app.add_systems(Update, log_suspicious_objects);
    }
    if std::env::var("SL_VIEWER_LOG_AVATAR_INTEREST").as_deref() == Ok("1") {
        // R22b diagnostic census of unresolved coarse "blue sphere" avatars.
        app.add_systems(Update, log_avatar_interest_census);
    }
    if std::env::var_os("SL_VIEWER_CAMERA_DUMP").is_some() {
        // Log the camera pose as a ready-to-paste
        // `--camera-position`/`--camera-look-at` for repeatable framing.
        app.add_systems(Update, dump_camera_pose.after(position_camera));
    }
    if std::env::var_os("SL_VIEWER_PARTICLE_FOCUS").is_some() {
        // Aim the camera at the busiest particle cloud so an unattended
        // screenshot frames a real emitter.
        app.add_systems(
            Update,
            focus_camera_on_particles
                .after(drive_particles)
                .after(position_camera),
        );
    }
    if std::env::var_os("SL_VIEWER_VOLUME_FOCUS").is_some() {
        // Aim the camera at the avatar whose shape displaces its collision
        // volumes the most (P34.3).
        app.add_systems(Update, focus_camera_on_volume_shape.after(position_camera));
    }
    if std::env::var_os(crate::notification_host::DEMO_ENV).is_some() {
        // Raise a sample notification spread on startup so the live stacking /
        // fade / modal behaviour can be watched without a server alert.
        app.add_systems(Update, spawn_notification_demo);
    }
    if repeat_animation && !play_animation.is_empty() {
        // Keep re-issuing the `--play-animation` motions (`--repeat-animation`)
        // so a one-shot animation still plays once the avatar has loaded.
        app.add_systems(Update, repeat_debug_animation);
    }
    // Avatar-state capture (viewer-avatar-state-dump-replay): only when
    // `SL_VIEWER_DUMP_DIR` is set — retain the raw avatar/appearance/animation
    // events each frame, and write a bundle per avatar on Ctrl+Alt+D. Off (zero
    // cost) in a normal session.
    if std::env::var_os("SL_VIEWER_DUMP_DIR").is_some() {
        app.init_resource::<crate::avatar_dump::ReplayCaptureStore>()
            .add_systems(
                Update,
                (
                    crate::avatar_dump::capture_replay_inputs,
                    crate::avatar_dump::dump_avatars_on_key,
                ),
            );
    }
    // Avatar-state replay (viewer-avatar-state-dump-replay): inject the bundle's
    // captured events once and drive the optional test rig (orbit light /
    // reflection probe). Only present in `--replay` mode.
    if let Some(config) = replay {
        app.insert_resource(config).add_systems(
            Update,
            (
                crate::avatar_replay::inject_replay_bundle,
                crate::avatar_replay::drive_replay_orbit_light,
                crate::avatar_replay::follow_replay_probe,
            ),
        );
    }
    // In screenshot mode, capture a numbered PNG sequence of the window after a
    // startup delay, then quit (the R11 offline-inspection harness).
    if let Some(dir) = screenshot_dir {
        if let Err(error) = fs_err::create_dir_all(dir) {
            warn!("failed to create screenshot dir {}: {error}", dir.display());
        }
        app.insert_resource(ScreenshotSchedule::new(dir.to_path_buf()))
            .add_systems(Update, (capture_screenshots, poll_screenshot_saves));
    }
    let _exit = app.run();
    app.world_mut()
        .remove_resource::<LoginOutcome>()
        .unwrap_or_default()
}

/// Run the viewer end-to-end, restarting the windowed app once per MFA
/// challenge with the acquired token folded in.
///
/// # Errors
///
/// Returns an [`enum@Error`] if credentials cannot be loaded, the login URI
/// cannot be resolved, or an MFA challenge cannot be answered.
fn run_viewer(options: &Options) -> Result<(), Error> {
    let credentials = Credentials::load(&options.credentials)?;
    let avatar = credentials.select(options.avatar.as_deref())?;
    let login_uri = resolve_login_uri(options, avatar)?;

    // The persisted start-location preference (the preferences General tab) is
    // read from a throwaway store load: the Bevy app — and with it the
    // `ViewerSettings` resource — does not exist yet at login-request time.
    let (start, stored_skin, stored_theme) = {
        let settings = crate::settings::ViewerSettings::load();
        // The network & cache tab's restart-scoped knobs (cache root and
        // size ceilings, chat-log root, HTTP proxy, a pending clear-cache
        // request) are consumed from this same pre-app load, before any
        // store or HTTP client exists.
        crate::preferences_network_cache::apply_startup_settings(&settings);
        let stored = settings
            .store()
            .get_str(crate::preferences_general::SETTING_LOGIN_START_LOCATION)
            .ok()
            .map(str::to_owned);
        let start = crate::preferences_general::resolve_start_location(
            options.start.clone(),
            stored.as_deref(),
        );
        // The persisted skin choice (the colors & skins tab) seeds the initial
        // dress; the CLI / env values override it inside `resolve`.
        let (stored_skin, stored_theme) =
            crate::preferences_colors_skins::stored_skin_choice(&settings);
        (start, stored_skin, stored_theme)
    };
    let mut request = LoginRequest::new(
        avatar.first().to_owned(),
        avatar.last().to_owned(),
        avatar.password().expose().to_owned(),
        start,
        options.channel.clone(),
        options.version.clone(),
    );
    loop {
        info!(
            "logging in as {} {} to {login_uri}",
            avatar.first(),
            avatar.last()
        );
        let params = LoginParams {
            login_uri: login_uri.parse()?,
            request: request.clone(),
        };
        let camera_start = CameraStart {
            position: options.camera_position,
            // Aim the fixed camera at the look-at point (the direction from the
            // camera to the target); ignored without a fixed position.
            look: match (options.camera_position, options.camera_look_at) {
                (Some(position), Some(target)) => Some(Vec3::new(
                    target.x - position.x,
                    target.y - position.y,
                    target.z - position.z,
                )),
                _other => None,
            },
        };
        let camera_spin = CameraSpin {
            rate: options.camera_spin.unwrap_or(0.0).to_radians(),
            axis: options.camera_spin_axis,
        };
        let outcome = run_session(
            &params,
            options.viewer_assets.as_deref(),
            &options.play_animation,
            options.repeat_animation,
            options.screenshot_dir.as_deref(),
            CameraStartup {
                start: camera_start,
                spin: camera_spin,
            },
            SkinRuntime {
                selection: crate::skin::SkinSelection::resolve(
                    options.skin.clone(),
                    options.theme.clone(),
                    stored_skin.clone(),
                    stored_theme.clone(),
                ),
                watch: options.watch_skins,
            },
            MediaRuntime {
                web: !options.disable_web_media,
                video: !options.disable_video_media,
                web_auth: !options.no_web_auth,
            },
            !options.no_group_chat_history,
            None,
        );
        if let Some(challenge) = outcome.challenge {
            info!(
                "multi-factor authentication required: {}",
                challenge.message
            );
            let token = avatar.acquire_mfa()?.ok_or(Error::MfaRequired)?;
            request = request.with_mfa(token.expose(), challenge.mfa_hash);
            continue;
        }
        if let Some(rejection) = outcome.rejected {
            // The viewer has no interactive prompt, so a retryable rejection is
            // reported and the run ends rather than looping (a rapid re-login
            // may be flagged by the grid). Logged at `error!` so a launch that
            // fails login — e.g. an OpenSim stale-presence block on a too-quick
            // re-login — is unmistakable in the log even at `RUST_LOG=error`,
            // rather than looking like a silent early exit.
            error!(
                "login rejected: {} ({}); the viewer will exit without connecting",
                rejection.reason, rejection.message
            );
        }
        break;
    }
    info!("session ended");
    Ok(())
}

/// Render a captured avatar-state bundle offline (`--replay <dir>`): point the
/// asset stores at the bundle's drop-in `cache/`, load its manifests, and run one
/// windowed session with login disabled and the replay injector wired in. No
/// credentials, no grid, no login retry loop.
///
/// # Errors
///
/// Returns [`Error::Replay`] if the bundle is missing, empty, or unreadable.
fn run_replay(options: &Options, bundle_dir: &Path) -> Result<(), Error> {
    // Serve every asset request from the bundle's drop-in cache for the rest of
    // the process (must be set before the asset stores are built below).
    crate::paths::set_replay_cache_root(bundle_dir.join(crate::replay_bundle::CACHE_SUBDIR));
    info!(
        "replay: assets served from {:?}",
        crate::paths::asset_cache_dir("texturecache")
    );
    let manifests = crate::replay_bundle::load_bundle(bundle_dir).map_err(Error::Replay)?;
    if manifests.is_empty() {
        return Err(Error::Replay(format!(
            "no avatar manifests (*.json) in {}",
            bundle_dir.display()
        )));
    }
    info!(
        "replaying {} avatar(s) from {}",
        manifests.len(),
        bundle_dir.display()
    );
    let config = crate::avatar_replay::ReplayConfig::new(
        manifests,
        options.replay_orbit_light,
        options.replay_reflection_probe,
    );

    // Frame the camera on the primary avatar unless the operator fixed a pose.
    let camera_start = if options.camera_position.is_some() {
        CameraStart {
            position: options.camera_position,
            look: match (options.camera_position, options.camera_look_at) {
                (Some(position), Some(target)) => Some(Vec3::new(
                    target.x - position.x,
                    target.y - position.y,
                    target.z - position.z,
                )),
                _other => None,
            },
        }
    } else {
        replay_camera_start(&config)
    };
    let camera_spin = CameraSpin {
        rate: options.camera_spin.unwrap_or(0.0).to_radians(),
        axis: options.camera_spin_axis,
    };

    // A placeholder login (never used offline), only to satisfy the plugin's
    // required `LoginParams`.
    let params = LoginParams {
        login_uri: DEFAULT_LOGIN_URI.parse()?,
        request: LoginRequest::new(
            "Replay".to_owned(),
            "Avatar".to_owned(),
            String::new(),
            StartLocation::Last,
            options.channel.clone(),
            options.version.clone(),
        ),
    };
    let _outcome = run_session(
        &params,
        options.viewer_assets.as_deref(),
        &options.play_animation,
        options.repeat_animation,
        options.screenshot_dir.as_deref(),
        CameraStartup {
            start: camera_start,
            spin: camera_spin,
        },
        SkinRuntime {
            selection: {
                // The persisted skin choice dresses the replay UI too; the
                // throwaway pre-app load is the `run_viewer` idiom.
                let settings = crate::settings::ViewerSettings::load();
                let (stored_skin, stored_theme) =
                    crate::preferences_colors_skins::stored_skin_choice(&settings);
                crate::skin::SkinSelection::resolve(
                    options.skin.clone(),
                    options.theme.clone(),
                    stored_skin,
                    stored_theme,
                )
            },
            watch: options.watch_skins,
        },
        // No network surfaces offline: keep the media engines and web auth off.
        MediaRuntime {
            web: false,
            video: false,
            web_auth: false,
        },
        // Offline there is no session thread, so the flag is inert; false keeps
        // the no-network intent explicit.
        false,
        Some(config),
    );
    info!("replay ended");
    Ok(())
}

/// The flycam start pose framing the primary replay avatar in a three-quarter
/// view — placed in front of and above the avatar's chest, looking back at it —
/// or the default (login-snapped) start when no avatar object was captured.
fn replay_camera_start(config: &crate::avatar_replay::ReplayConfig) -> CameraStart {
    let Some(avatar) = config.primary_position() else {
        return CameraStart::default();
    };
    // Aim at the chest (~1 m above the object root, which sits at the feet).
    let target = Vec3::new(avatar.x, avatar.y + 1.0, avatar.z);
    // A three-quarter viewpoint a couple of metres out.
    let position = Vec3::new(target.x + 1.8, target.y + 0.4, target.z + 2.2);
    CameraStart {
        position: Some(position),
        look: Some(Vec3::new(
            target.x - position.x,
            target.y - position.y,
            target.z - position.z,
        )),
    }
}

/// Guards that keep profiling tracing layers alive for the process lifetime.
///
/// The Chrome/Perfetto tracer (`profile-chrome`) buffers events on a worker
/// thread and only finalises a complete trace file when its flush guard drops,
/// so [`init_tracing`]'s caller must hold the returned value until the app exits
/// (`let _guards = init_tracing();`). With no profiling feature enabled this is a
/// zero-sized do-nothing token, but callers should hold it uniformly.
#[must_use = "hold the returned guard until the app exits so profiling output is flushed"]
pub struct TracingGuards {
    /// Flush guard for the `tracing-chrome` trace file; dropping it finalises and
    /// closes the JSON trace. Never read — only its `Drop` matters.
    #[cfg(feature = "profile-chrome")]
    _chrome: tracing_chrome::FlushGuard,
}

impl std::fmt::Debug for TracingGuards {
    /// Hand-written because `tracing_chrome::FlushGuard` is not `Debug`; the guard
    /// carries no inspectable state worth printing anyway.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TracingGuards").finish_non_exhaustive()
    }
}

/// A [`tracing_tracy`] field formatter kept as a *distinct type* from the
/// terminal fmt layer's [`DefaultFields`].
///
/// The fmt layer and Tracy both cache a span's formatted fields in the span's
/// extension map, keyed by the field-formatter type: `FormattedFields<N>`. With
/// [`tracing_tracy::TracyLayer::default`] that type is `FormattedFields<DefaultFields>`
/// — exactly the type the fmt layer already stores. The fmt layer's
/// `on_new_span` runs first (it is the inner layer) and inserts an
/// ANSI-*coloured* copy (it colours terminal output). Tracy's `on_new_span`
/// then finds the extension already present and reuses it verbatim, so the raw
/// ANSI escapes end up in Tracy zone names, which Tracy renders literally.
///
/// Wrapping [`DefaultFields`] in a newtype gives Tracy its own extension type
/// (`FormattedFields<TracyFieldFormatter>`), so it formats its own copy — with
/// ANSI disabled, since [`FormattedFields::new`] defaults `was_ansi` to `false`
/// — while the terminal fmt layer keeps its colours.
#[cfg(feature = "profile-tracy")]
#[derive(Default)]
struct TracyFieldFormatter(tracing_subscriber::fmt::format::DefaultFields);

#[cfg(feature = "profile-tracy")]
impl<'writer> tracing_subscriber::fmt::FormatFields<'writer> for TracyFieldFormatter {
    /// Delegate to the wrapped [`DefaultFields`]; the newtype exists only to be a
    /// distinct type in the span extension map, not to change the formatting.
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        writer: tracing_subscriber::fmt::format::Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        tracing_subscriber::fmt::FormatFields::format_fields(&self.0, writer, fields)
    }
}

/// Tracy configuration that swaps the default field formatter for
/// [`TracyFieldFormatter`] so Tracy zone names carry no ANSI escapes.
#[cfg(feature = "profile-tracy")]
#[derive(Default)]
struct TracyConfig(TracyFieldFormatter);

#[cfg(feature = "profile-tracy")]
impl tracing_tracy::Config for TracyConfig {
    type Formatter = TracyFieldFormatter;

    /// The field formatter Tracy uses for zone names — our ANSI-free newtype.
    fn formatter(&self) -> &Self::Formatter {
        &self.0
    }
}

/// Install the `tracing` subscriber both binaries share.
///
/// The viewer disables Bevy's own `LogPlugin` (see `run_session`) because the
/// login happens before the window exists and its logs must go somewhere, so the
/// subscriber is ours to install — once, from the binary, before any Bevy plugin
/// could claim the global slot. Bevy's own profilers attach their tracing layers
/// *through* `LogPlugin`, so with it disabled they never install; the `profile-*`
/// features re-wire the Tracy and Chrome/Perfetto layers here instead (see
/// `viewer-profiling-logplugin-tracing`).
///
/// Hold the returned [`TracingGuards`] until the app exits so the Chrome tracer's
/// trace file is flushed.
pub fn init_tracing() -> TracingGuards {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_ignored| EnvFilter::new("info"));

    // When Tracy is active, `bevy_render` emits a `tracy.frame_mark` INFO event
    // every frame purely as a Tracy frame boundary; keep it out of the
    // human-readable log so it does not spam the terminal (mirrors `LogPlugin`).
    #[cfg(feature = "profile-tracy")]
    let fmt_layer = {
        use tracing_subscriber::Layer as _;
        tracing_subscriber::fmt::layer().with_filter(tracing_subscriber::filter::FilterFn::new(
            |meta| meta.fields().field("tracy.frame_mark").is_none(),
        ))
    };
    #[cfg(not(feature = "profile-tracy"))]
    let fmt_layer = tracing_subscriber::fmt::layer();

    #[cfg(feature = "profile-chrome")]
    let (chrome_layer, chrome_guard) = {
        use tracing_subscriber::fmt::{FormattedFields, format::DefaultFields};
        // `TRACE_CHROME` overrides the output path, matching Bevy's `LogPlugin`.
        let mut builder = tracing_chrome::ChromeLayerBuilder::new();
        if let Ok(path) = std::env::var("TRACE_CHROME") {
            builder = builder.file(path);
        }
        // Name spans by their formatted fields (e.g. the system name), so the
        // trace shows "system: name=..." instead of a wall of bare "system".
        builder
            .name_fn(Box::new(|event_or_span| match event_or_span {
                tracing_chrome::EventOrSpan::Event(event) => event.metadata().name().into(),
                tracing_chrome::EventOrSpan::Span(span) => span
                    .extensions()
                    .get::<FormattedFields<DefaultFields>>()
                    .map_or_else(
                        || span.metadata().name().into(),
                        |fields| format!("{}: {}", span.metadata().name(), fields.fields.as_str()),
                    ),
            }))
            .build()
    };

    // The `EnvFilter` sits below the output layers so it gates all of them (the
    // fmt log, Tracy and Chrome), exactly as `LogPlugin` orders them.
    let subscriber = tracing_subscriber::registry().with(filter).with(fmt_layer);

    #[cfg(feature = "profile-tracy")]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::new(TracyConfig::default()));

    #[cfg(feature = "profile-chrome")]
    let subscriber = subscriber.with(chrome_layer);

    subscriber.init();

    // On-demand mode (the `tracing-tracy/ondemand` feature) means the client
    // records nothing until a profiler connects and discards on disconnect, so
    // memory does *not* grow while untethered — unlike Tracy's default, which
    // buffers every event until a client attaches.
    #[cfg(feature = "profile-tracy")]
    info!(
        "Tracy profiling is active (on-demand): data is collected only while a profiler is connected"
    );

    TracingGuards {
        #[cfg(feature = "profile-chrome")]
        _chrome: chrome_guard,
    }
}

/// The viewer entry point: parse options, initialise logging, and run the viewer.
///
/// The `sl-client-bevy-viewer` binary is a thin shell over this, so that the
/// whole viewer — the UI scaffold especially — lives in a library the gallery
/// binary ([`gallery`]) can build against too.
///
/// # Errors
///
/// Returns [`Error`] if the credentials, grid or login URI cannot be resolved.
pub fn run() -> Result<(), Error> {
    // Held for the whole process so the Chrome profiler (if enabled) flushes.
    let _tracing_guards = init_tracing();
    let options = Options::parse();
    // `--replay <dir>` renders a captured bundle offline; otherwise a normal login.
    if let Some(bundle_dir) = options.replay.clone() {
        return run_replay(&options, &bundle_dir);
    }
    run_viewer(&options)
}
