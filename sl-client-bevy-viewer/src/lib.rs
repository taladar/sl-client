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

mod animations;
mod animesh;
mod appearance;
mod avatar_assets;
mod avatars;
mod bake_inputs;
mod bake_publish;
mod body_physics;
mod bottom_toolbar;
mod bump;
mod camera;
mod chat;
mod chat_input;
mod coords;
mod diagnostics;
mod emoji_complete;
mod emoji_picker;
mod environment;
mod flexi;
mod floater;
mod floater_persist;
mod flycam_ui;
pub mod gallery;
mod ground;
mod hand_pose;
mod hud;
mod hud_pick;
mod i18n;
mod ik;
mod input_action;
mod input_context;
mod inventory;
mod legacy_materials;
mod lights;
mod local_chat_input;
mod locomotion;
mod locomotion_ik;
mod look_at;
mod materials;
mod menu;
mod menu_bar;
mod menu_search;
mod meshes;
mod movement;
mod objects;
mod particles;
mod paths;
mod physics;
mod pie_menu;
mod probes;
mod procedural;
mod reach;
pub mod render_gallery;
mod render_priority;
#[cfg(test)]
mod render_readback;
mod render_scene;
#[cfg(test)]
mod render_test;
mod screenshot;
mod session;
mod settings;
mod settings_binding;
mod skin;
mod sky;
mod spacenav;
mod status_bar;
mod terrain;
mod texture_anim;
mod textures;
mod tonemap;
mod typing;
mod ui;
mod ui_element;
mod ui_font;
mod ui_pseudoloc;
mod ui_search;
mod ui_tab;
#[cfg(test)]
mod ui_test;
mod ui_text;
mod ui_text_input;
mod underwater_fog;
mod virtual_list;
mod water;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::light::DirectionalLightShadowMap;
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

use crate::animations::{
    AnimationManager, AnimationPlayback, drive_avatar_skeletons, ingest_avatar_animations,
    poll_animations, pose_avatar_skeletons, update_animation_caps,
};
use crate::animesh::{
    ControlAvatarState, drive_control_avatars, ingest_object_animations, pose_control_avatars,
};
use crate::appearance::{ServerBakeState, drive_server_bake};
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::{
    AvatarBakeMaterials, AvatarRuntimeMorphs, AvatarState, OwnLocalBake, VolumeMorphGain,
    annotate_avatar_distances, apply_avatar_appearance, apply_avatar_bake_textures,
    apply_avatar_names, apply_avatar_part_visibility, apply_avatar_runtime_morphs,
    apply_bom_face_materials, apply_own_local_bake, apply_own_shape_from_wearables,
    assign_avatar_bake_materials, focus_camera_on_volume_shape, ingest_avatar_bakes,
    log_avatar_interest_census, position_name_tags, setup_avatar_body, toggle_volume_morphs,
    update_avatar_objects, update_coarse_avatars,
};
use crate::bake_inputs::{
    OwnBakeInputs, WearableAssetFetched, WearableAssetManager, assemble_own_bake,
    drive_wearable_requests, poll_wearable_assets, update_asset_caps,
};
use crate::bake_publish::{OwnBakePublish, drive_bake_publish};
use crate::bump::{BumpManager, apply_bump_normals, register_bump_faces};
use crate::camera::{
    CameraMode, CameraPlugin, CameraRig, CameraSpin, CameraStart, SpinAxis, ViewerCamera,
    position_camera,
};
use crate::chat::{ChatOverlay, setup_chat_overlay, update_chat_overlay};
use crate::chat_input::ChatInputPlugin;
use crate::diagnostics::{
    PipelineOverlayVisible, setup_pipeline_overlay, toggle_pipeline_overlay,
    update_pipeline_overlay,
};
use crate::emoji_complete::ColonCompletePlugin;
use crate::emoji_picker::EmojiPickerPlugin;
use crate::environment::{EnvironmentState, ingest_environment, request_environment};
use crate::flexi::simulate_flexi;
use crate::floater::FloaterPlugin;
use crate::floater_persist::FloaterPersistPlugin;
use crate::flycam_ui::FlycamButtonPlugin;
use crate::hud::{HudState, apply_hud_fullbright, fit_hud_points, setup_hud_screen};
use crate::hud_pick::pick_and_touch;
use crate::i18n::ViewerI18nPlugin;
use crate::input_action::InputActionPlugin;
use crate::input_context::{CursorGrabAllowed, InputContextPlugin, world_has_keyboard};
use crate::inventory::InventoryPlugin;
use crate::legacy_materials::{
    LegacyMaterialManager, apply_legacy_materials, apply_legacy_normal_maps,
    drive_legacy_material_requests, receive_legacy_materials, register_legacy_materials,
};
use crate::lights::{LocalLights, drive_local_lights};
use crate::local_chat_input::LocalChatInputPlugin;
use crate::locomotion::drive_own_locomotion;
use crate::materials::{
    MaterialManager, apply_material_overrides, apply_pbr_textures, poll_materials,
    register_pbr_materials, update_material_caps,
};
use crate::meshes::{MeshDecoded, MeshManager, poll_meshes, update_mesh_caps};
use crate::movement::{AvatarControls, drive_avatar_controls};
use crate::objects::{
    ObjectState, PrimLodTargets, TreeLodTargets, adopt_pending_attachments, apply_object_meshes,
    apply_object_sculpts, apply_prim_lod, apply_rigged_attachments, apply_tree_lod,
    log_suspicious_objects, pick_object, prune_control_avatars, spawn_animesh_control_avatars,
    update_objects,
};
use crate::particles::{ParticleSim, drive_particles, focus_camera_on_particles, setup_particles};
use crate::physics::PhysicsPlugin;
use crate::pie_menu::PieMenuPlugin;
use crate::probes::ReflectionProbePlugin;
use crate::render_priority::drive_render_priority;
use crate::screenshot::{ScreenshotSchedule, capture_screenshots};
use crate::session::{
    PlayOnLogin, ViewerSession, drive_session, enforce_quit_deadline, handle_quit_input,
    repeat_debug_animation, report_agent_viewport, report_camera_interest, save_settings_on_logout,
};
use crate::settings::{AccountContext, ViewerSettings, load_account_settings};
use crate::settings_binding::SettingsBindingPlugin;
use crate::sky::{
    apply_cloud_textures, apply_disc_textures, apply_sky_textures, apply_star_textures,
    center_sky_on_camera, drive_clouds, drive_sky, drive_stars, drive_sun_moon_discs, setup_clouds,
    setup_sky, setup_stars, setup_sun_moon_discs,
};
use crate::spacenav::SpacenavPlugin;
use crate::terrain::{TerrainState, recenter_terrain, update_terrain};
use crate::texture_anim::{drive_texture_animations, restore_stopped_animations};
use crate::textures::{
    PrimTextures, TextureDecoded, TextureManager, apply_prim_textures, poll_textures,
    update_texture_caps,
};
use crate::tonemap::{SlTonemap, SlTonemapPlugin};
use crate::typing::{TypingState, drive_own_typing};
use crate::ui::{UiScaffoldSystems, ViewerUiPlugin};
use crate::ui_element::UiAction;
use crate::ui_tab::TabWidgetPlugin;
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
    /// The login start location (`last`, `home`, or `uri:Region&x&y&z`).
    #[clap(long, default_value = "last")]
    start: StartLocation,
    /// The viewer channel reported to the grid.
    #[clap(long, default_value = "sl-client-bevy-viewer")]
    channel: String,
    /// The viewer version reported to the grid.
    #[clap(long, default_value = clap::crate_version!())]
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
    #[clap(long)]
    skin: Option<String>,
    /// A theme overlay for the skin — a file under
    /// `assets/skins/<skin>/themes/` (e.g. `dark`), which redefines a subset of
    /// the skin's tokens. Omit for the skin's own base.
    #[clap(long)]
    theme: Option<String>,
    /// Watch the skin `.css` files and re-apply them live as they are edited —
    /// the skin-authoring loop. Off by default (a tiny background cost); turn it
    /// on while designing a skin or theme.
    #[clap(long)]
    watch_skins: bool,
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
    let translation = if let Some(position) = camera_start.position {
        // A fixed pose is a flycam pose: place and aim it, and leave it alone.
        if let Some(look) = camera_start.look {
            rig.aim_along(look);
        }
        *mode = CameraMode::Flycam;
        position
    } else {
        // A provisional pose near a region centre; `position_camera` moves it to
        // frame the avatar the moment one arrives.
        Vec3::new(128.0, 30.0, -128.0)
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
        Transform::from_translation(translation),
        ViewerCamera,
        rig,
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
        // Bevy's *photometric* exposure: what turns the sun's illuminance (lux) and a
        // prim light's lumens into the linear radiance the frame is composed in. It is
        // a distinct thing from the reference's `RenderExposure` (a plain scale on the
        // finished linear frame, carried by `SlTonemap`), and it is spelled out rather
        // than left implicit because the reflection probes read it: their intensity is
        // derived from it (`probes::probe_intensity`), so a probe reproduces the
        // radiance it captured instead of re-scaling it.
        Exposure::default(),
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

/// Run one windowed session to completion, returning any recoverable login
/// outcome (an MFA challenge or a retryable rejection) it stopped on.
fn run_session(
    params: &LoginParams,
    viewer_assets: Option<&Path>,
    play_animation: &[Uuid],
    repeat_animation: bool,
    screenshot_dir: Option<&Path>,
    camera: CameraStartup,
    skin: SkinRuntime,
) -> LoginOutcome {
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

    let mut app = App::new();
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
        // Log every text-chat type to the per-avatar chat directory.
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
    // SpaceNavigator / 6-DOF device input (viewer-input-spacenav-*): publishes the
    // device state (Linux, behind the `spacenav` feature) for the flycam to consume.
    .add_plugins(SpacenavPlugin)
    // The on-screen "Stop flycam" button (shown only in flycam mode).
    .add_plugins(FlycamButtonPlugin)
    // The radial (pie) menu widget (viewer-ui-radial-menu): the mechanism only —
    // which entries a given pie holds is per-domain and belongs with the domain
    // (viewer-object-context-menu), so nothing here opens one yet. The widget is
    // reachable in the gallery's `radial-menu-target` card meanwhile.
    .add_plugins(PieMenuPlugin)
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
    // The bottom toolbar (viewer-ui-bottom-toolbar): the persistent strip of
    // toggle buttons that open the main floaters (Inventory wired today, the rest
    // disabled placeholders until their tasks land), and the bottom-area layout
    // host the nearby-chat / audio / voice / quick-preferences controls hang off.
    // After the inventory plugin so its Inventory toggle can reach the window.
    .add_plugins(crate::bottom_toolbar::BottomToolbarPlugin)
    // Per-user floater geometry (viewer-ui-floater-persist-geometry): remember
    // each floater's position, size, minimized / docked state and open / closed
    // state across sessions, in the per-avatar account settings.
    .add_plugins(FloaterPersistPlugin)
    .add_plugins(TerrainMaterialPlugin)
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
    // The underwater-fog post-process (P23.1): a fullscreen depth-based pass that
    // fogs everything below the water surface (reference `getWaterFogView`).
    .add_plugins(UnderwaterFogPlugin)
    // The reference viewer's tone mapper (P33.3): the one transfer from the linear
    // HDR scene to displayable colour, over the whole composited frame (reference
    // `postDeferredTonemap` — ACES / Khronos Neutral, blended by `RenderTonemapMix`).
    // Runs after the fog, which the reference likewise applies in linear space.
    .add_plugins(SlTonemapPlugin)
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
    // Frame-time / FPS instruments — the smoothed FPS the status area
    // (`crate::status_bar`) shows and the frame budget the fetch/decode pipeline
    // work is watched against.
    .add_plugins(FrameTimeDiagnosticsPlugin::default())
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
    .init_resource::<ObjectState>()
    // The screen-space HUD hierarchy (P35.1), spawned by `setup_hud_screen`.
    .init_resource::<HudState>()
    // The water-render bookkeeping (P23.1) is created by `setup_water` at
    // startup, so no `init_resource` is needed here; the surface level the
    // underwater-fog pass reads is a small resource published by `drive_water`.
    .init_resource::<WaterLevel>()
    .init_resource::<PrimLodTargets>()
    .init_resource::<TreeLodTargets>()
    .init_resource::<LocalLights>()
    .init_resource::<ParticleSim>()
    .init_resource::<AvatarState>()
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
    .init_resource::<TypingState>()
    .init_resource::<ControlAvatarState>()
    .init_resource::<ChatOverlay>()
    .init_resource::<TextureManager>()
    .init_resource::<PrimTextures>()
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
            setup_chat_overlay,
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
        ),
    )
    .add_systems(
        Update,
        (
            capture_login_outcome,
            drive_session,
            // Request the region environment (EEP) on handshake, then fold the
            // grid's reply into `EnvironmentState` (P22.1); the sky / water /
            // shadow phases render from it. Nested into one tuple to stay within
            // Bevy's per-tuple system limit.
            (request_environment, ingest_environment),
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
            // Recenter (origin follows the root region) before folding terrain
            // events, so patches are placed on the current origin.
            (recenter_terrain, update_terrain).chain(),
            update_objects,
            // Build the geometry of any mesh object whose asset just decoded, and
            // of any sculpted prim whose sculpt map just decoded.
            apply_object_meshes,
            apply_object_sculpts,
            // Apply decoded diffuse textures to parked faces, then the PBR (GLTF)
            // render-material pipeline (P27.1): keep the material store's
            // `ViewerAsset` cap current, register each newly-spawned face's
            // material, fold finished material fetches into the face materials, and
            // drop each decoded texture map into its slot. Nested into one tuple to
            // stay within Bevy's per-tuple system limit; runs after the
            // face-spawning systems so a face's PBR material is seen.
            (
                apply_prim_textures,
                update_material_caps,
                register_pbr_materials,
                poll_materials,
                apply_material_overrides,
                apply_pbr_textures,
                // The legacy (normal/specular) render-material pipeline (P27.3):
                // register each face carrying a `TextureEntry` material id, batch
                // the `RenderMaterials` cap requests, fold in the replies, and
                // apply the materials + their normal maps to the faces.
                register_legacy_materials,
                drive_legacy_material_requests,
                receive_legacy_materials,
                apply_legacy_materials,
                apply_legacy_normal_maps,
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
                (update_avatar_objects, update_coarse_avatars).chain(),
                // R22b diagnostic census of unresolved coarse "blue sphere" avatars,
                // plus per-tag distance annotation (both gated by
                // `SL_VIEWER_LOG_AVATAR_INTEREST`; a no-op otherwise).
                log_avatar_interest_census,
                annotate_avatar_distances,
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
            position_name_tags,
            // Append newly received local chat to the on-screen overlay.
            update_chat_overlay,
            // Quit handling: request a clean logout on the quit key, then force the
            // exit once the grace period lapses. Nested into one tuple to stay
            // within Bevy's per-tuple system limit. Only the key half is gated on
            // the input context — `Q` is a character a text field wants, and
            // `Escape` there means "give the keyboard back" (see
            // `input_context`) — while the deadline must still fire once a quit is
            // under way, whatever has focus.
            (
                handle_quit_input.run_if(world_has_keyboard),
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
    // Opt-in diagnostic (SL_VIEWER_LOG_OBJECTS): flag region-sized / sky objects
    // so a live session can tell an unculled large object from a wrongly decoded one.
    // Plus the crosshair pick tool (press `P`) to identify the object under the
    // centre of the screen. Separate calls to stay clear of Bevy's per-tuple
    // system limit.
    .add_systems(
        Update,
        (
            log_suspicious_objects,
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
            // third-person clicks the world directly.
            pick_and_touch,
            // On-screen render priority (P20.2): re-rank the queued texture / mesh
            // fetches by the pixel area each object covers, so what the camera
            // looks at loads first. Throttled internally. It also picks each plain
            // prim's tessellation level of detail (P21.3); `apply_prim_lod` then
            // re-tessellates any prim whose level changed, so it runs after.
            drive_render_priority,
            apply_prim_lod.after(drive_render_priority),
            // Tree level of detail (P26.2): regenerate any tree whose branching /
            // billboard tier the driver changed, after it has picked the tiers.
            apply_tree_lod.after(drive_render_priority),
            // Key-toggled texture/mesh pipeline-status panel (P19.3): flip its
            // resource on the toggle key, then drive the panel's visibility and
            // (while shown) its text from the live store snapshots.
            toggle_pipeline_overlay,
            update_pipeline_overlay.after(toggle_pipeline_overlay),
            // UI text & font foundation (viewer-ui-text-foundation): toggle /
            // apply the demo panel's visibility (the F4 key). Nested into one
            // tuple to stay within Bevy's per-tuple system limit.
            (
                toggle_text_demo,
                apply_text_demo_visibility.after(toggle_text_demo),
            ),
            // Reusable text-input widget (viewer-ui-text-input-widget): toggle /
            // apply the demo panel's visibility (the F8 key), and keep the numeric
            // rows' live parsed-value read-outs current.
            (
                toggle_text_input_demo,
                apply_text_input_demo_visibility.after(toggle_text_input_demo),
                update_demo_value_readouts,
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
            // Debug (env `SL_VIEWER_PARTICLE_FOCUS`): aim the camera at the busiest
            // particle cloud so an unattended screenshot frames a real emitter.
            focus_camera_on_particles
                .after(drive_particles)
                .after(position_camera),
            // Debug (env `SL_VIEWER_VOLUME_FOCUS`): aim the camera at the avatar whose
            // shape displaces its collision volumes the most (P34.3), the only subject
            // on which the effect is visible at all.
            focus_camera_on_volume_shape.after(position_camera),
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
            // Typing state animation for the own avatar (P31.9): toggle the typing
            // state (the T key stands in for a chat-entry box), play `ANIM_AGENT_TYPE`
            // locally, and broadcast a `StartTyping` / `StopTyping` `ChatFromViewer`.
            // Like locomotion it must reconcile its client-driven set before the
            // skeleton driver folds it into the frame's pose.
            drive_own_typing
                .before(drive_avatar_skeletons)
                .run_if(world_has_keyboard),
            drive_avatar_skeletons.after(apply_avatar_appearance),
            // Hand-pose morph (P31.13): cross-fade each avatar's hands into the pose
            // its highest-priority playing animation asks for. After the skeleton
            // driver (whose playing set it reads) and before the runtime-morph fold,
            // so the cross-faded weights reach the GPU in the same frame.
            hand_pose::drive_hand_poses
                .after(drive_avatar_skeletons)
                .before(apply_avatar_runtime_morphs),
            repeat_debug_animation,
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
            // Avatar ground probe (P31.14): raycast what is under each avatar's root
            // and ankles, for the foot IK and the landing recovery. It reads the joint
            // globals the pose pass wrote *last* frame — it cannot run inside that pass,
            // which writes the very `GlobalTransform`s `MeshRayCast` reads.
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
            pose_control_avatars.after(TransformSystems::Propagate),
        ),
    );
    // Load the client-side avatar assets (if a directory was given) so rigged
    // bodies replace the placeholder spheres; absent them the viewer keeps spheres.
    if let Some(library) = load_avatar_library(viewer_assets) {
        app.insert_resource(library);
    }
    // In screenshot mode, capture a numbered PNG sequence of the window after a
    // startup delay, then quit (the R11 offline-inspection harness).
    if let Some(dir) = screenshot_dir {
        if let Err(error) = fs_err::create_dir_all(dir) {
            warn!("failed to create screenshot dir {}: {error}", dir.display());
        }
        app.insert_resource(ScreenshotSchedule::new(dir.to_path_buf()))
            .add_systems(Update, capture_screenshots);
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

    let mut request = LoginRequest::new(
        avatar.first().to_owned(),
        avatar.last().to_owned(),
        avatar.password().expose().to_owned(),
        options.start.clone(),
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
                ),
                watch: options.watch_skins,
            },
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
            // may be flagged by the grid).
            warn!(
                "login rejected: {} ({}); not retrying",
                rejection.reason, rejection.message
            );
        }
        break;
    }
    info!("session ended");
    Ok(())
}

/// Install the `tracing` subscriber both binaries share.
///
/// The viewer disables Bevy's own `LogPlugin` (see `run_session`) because the
/// login happens before the window exists and its logs must go somewhere, so the
/// subscriber is ours to install — once, from the binary, before any Bevy plugin
/// could claim the global slot.
pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_ignored| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
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
    init_tracing();
    let options = Options::parse();
    run_viewer(&options)
}
