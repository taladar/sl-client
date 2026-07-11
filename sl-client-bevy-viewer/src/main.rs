//! Minimum-viable Bevy visual viewer for Second Life / OpenSim.
//!
//! See the crate `README.md` and `VIEWER_ROADMAP.md` for the staged plan. This
//! binary logs in via the shared `credentials.toml` mechanism (`sl-repl::auth`)
//! and opens a window that renders a region. This is the Phase 1 slice — the
//! viewer shell: window, login, a debug fly-camera, and a clean quit key —
//! with terrain, prims, meshes, sculpts, avatars, and chat landing in later
//! phases.

mod animations;
mod animesh;
mod appearance;
mod avatar_assets;
mod avatars;
mod bake_inputs;
mod bake_publish;
mod bump;
mod camera;
mod chat;
mod coords;
mod diagnostics;
mod environment;
mod legacy_materials;
mod lights;
mod materials;
mod meshes;
mod objects;
mod particles;
mod render_priority;
mod screenshot;
mod session;
mod sky;
mod terrain;
mod texture_anim;
mod textures;
mod underwater_fog;
mod water;

use std::path::{Path, PathBuf};

use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::light::DirectionalLightShadowMap;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::window::{CursorGrabMode, CursorOptions};
use clap::Parser as _;
use sl_client_bevy::{
    AnimationKey, ChatLogConfig, ClientDirectories, CloudMaterialPlugin, InventoryCacheConfig,
    LoginFailure, LoginParams, LoginRequest, MfaChallenge, SkyMaterialPlugin, SlClientPlugin,
    SlLoginRejected, SlMfaChallenge, StarMaterialPlugin, StartLocation, SunDiscMaterialPlugin,
    TerrainMaterialPlugin, Uuid, WaterMaterialPlugin,
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
    AvatarBakeMaterials, AvatarState, OwnLocalBake, annotate_avatar_distances,
    apply_avatar_appearance, apply_avatar_bake_textures, apply_avatar_names,
    apply_avatar_part_visibility, apply_bom_face_materials, apply_own_local_bake,
    apply_own_shape_from_wearables, assign_avatar_bake_materials, ingest_avatar_bakes,
    log_avatar_interest_census, position_name_tags, setup_avatar_body, update_avatar_objects,
    update_coarse_avatars,
};
use crate::bake_inputs::{
    OwnBakeInputs, WearableAssetFetched, WearableAssetManager, assemble_own_bake,
    drive_wearable_requests, poll_wearable_assets, update_asset_caps,
};
use crate::bake_publish::{OwnBakePublish, drive_bake_publish};
use crate::bump::{BumpManager, apply_bump_normals, register_bump_faces};
use crate::camera::{CameraSpin, CameraStart, FlyCamera, SpinAxis, fly_camera};
use crate::chat::{ChatOverlay, setup_chat_overlay, update_chat_overlay};
use crate::diagnostics::{
    PipelineOverlayVisible, setup_diagnostics_overlay, setup_pipeline_overlay,
    toggle_pipeline_overlay, update_diagnostics_overlay, update_pipeline_overlay,
};
use crate::environment::{EnvironmentState, ingest_environment, request_environment};
use crate::legacy_materials::{
    LegacyMaterialManager, apply_legacy_materials, apply_legacy_normal_maps,
    drive_legacy_material_requests, receive_legacy_materials, register_legacy_materials,
};
use crate::lights::{LocalLights, drive_local_lights};
use crate::materials::{
    MaterialManager, apply_material_overrides, apply_pbr_textures, poll_materials,
    register_pbr_materials, update_material_caps,
};
use crate::meshes::{MeshDecoded, MeshManager, poll_meshes, update_mesh_caps};
use crate::objects::{
    ObjectState, PrimLodTargets, TreeLodTargets, adopt_pending_attachments, apply_object_meshes,
    apply_object_sculpts, apply_prim_lod, apply_rigged_attachments, apply_tree_lod,
    log_suspicious_objects, pick_object, prune_control_avatars, spawn_animesh_control_avatars,
    update_objects,
};
use crate::render_priority::drive_render_priority;
use crate::screenshot::{ScreenshotSchedule, capture_screenshots};
use crate::session::{
    PlayOnLogin, ViewerSession, drive_session, enforce_quit_deadline, handle_quit_input,
    repeat_debug_animation, report_agent_viewport, report_camera_interest,
};
use crate::sky::{
    apply_cloud_textures, apply_disc_textures, apply_sky_textures, apply_star_textures,
    center_sky_on_camera, drive_clouds, drive_sky, drive_stars, drive_sun_moon_discs, setup_clouds,
    setup_sky, setup_stars, setup_sun_moon_discs,
};
use crate::terrain::{TerrainState, recenter_terrain, update_terrain};
use crate::texture_anim::{drive_texture_animations, restore_stopped_animations};
use crate::textures::{
    PrimTextures, TextureDecoded, TextureManager, apply_prim_textures, poll_textures,
    update_texture_caps,
};
use crate::underwater_fog::{UnderwaterFog, UnderwaterFogPlugin, update_underwater_fog};
use crate::water::{WaterLevel, apply_water_textures, drive_water, setup_water, update_water};

/// The local OpenSim grid login URI used when none is otherwise resolved.
const DEFAULT_LOGIN_URI: &str = "http://127.0.0.1:9000/";

/// An error from the viewer binary.
#[derive(thiserror::Error, Debug)]
enum Error {
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

/// Startup system: spawn the fly-camera. The scene's directional light (the
/// sun / moon) is spawned by [`crate::sky::setup_sky`], which also drives it from
/// the region's environment.
fn setup_scene(mut commands: Commands, camera_start: Res<CameraStart>) {
    // A provisional camera pose near a region centre (256 m region, ~30 m up);
    // `drive_session` snaps it to the agent's real login position once the
    // agent's avatar object arrives — unless `--camera-position` fixed an
    // absolute pose, in which case place it there and aim it (and `drive_session`
    // leaves it alone).
    let mut camera = FlyCamera::default();
    let translation = if let Some(position) = camera_start.position {
        if let Some(look) = camera_start.look {
            camera.aim_along(look);
        }
        position
    } else {
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
        camera,
        Msaa::Sample4,
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

/// Run one windowed session to completion, returning any recoverable login
/// outcome (an MFA challenge or a retryable rejection) it stopped on.
fn run_session(
    params: &LoginParams,
    viewer_assets: Option<&Path>,
    play_animation: &[Uuid],
    repeat_animation: bool,
    screenshot_dir: Option<&Path>,
    camera_start: CameraStart,
    camera_spin: CameraSpin,
) -> LoginOutcome {
    // In screenshot mode leave the cursor free (visible, un-grabbed) so an
    // unattended capture run does not hijack the desktop's pointer.
    let cursor_options = if screenshot_dir.is_some() {
        CursorOptions {
            grab_mode: CursorGrabMode::None,
            visible: true,
            ..default()
        }
    } else {
        // Capture and hide the cursor so raw mouse motion drives mouse-look.
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..default()
        }
    };
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "sl-client-bevy-viewer".to_owned(),
                    ..default()
                }),
                primary_cursor_options: Some(cursor_options),
                ..default()
            })
            // The binary installs its own `tracing` subscriber (so the
            // pre-window login logs go somewhere); drop Bevy's `LogPlugin` to
            // avoid the "global subscriber already set" clash.
            .disable::<LogPlugin>(),
    )
    .add_plugins(SlClientPlugin {
        params: params.clone(),
        diagnostics: true,
        chat_log_config: ChatLogConfig::default(),
        directories: ClientDirectories::default(),
        inventory_cache_config: InventoryCacheConfig::default(),
        background_inventory_fetch: false,
    })
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
    // Frame-time / FPS and entity-count instruments for the Phase 19 diagnostics
    // overlay (the rendering-fidelity phases lean hard on the fetch/decode
    // pipeline, so make the frame budget visible).
    .add_plugins((
        FrameTimeDiagnosticsPlugin::default(),
        EntityCountDiagnosticsPlugin::default(),
    ))
    // P24.1: a larger sun/moon shadow map than the 2048 default, so the four
    // region-scale cascades (see `sky::shadow_cascades`) keep enough texels per
    // world unit to shadow an avatar crisply across a whole region.
    .insert_resource(DirectionalLightShadowMap { size: 4096 })
    .init_resource::<ViewerSession>()
    // The debug camera override (`--camera-position` / `--camera-look-at` /
    // `--camera-spin`): `setup_scene` reads the start pose, `fly_camera` reads
    // the spin, and `drive_session` skips its login-snap when a pose is fixed.
    .insert_resource(camera_start)
    .insert_resource(camera_spin)
    .init_resource::<LoginOutcome>()
    .init_resource::<EnvironmentState>()
    .init_resource::<TerrainState>()
    .init_resource::<ObjectState>()
    // The water-render bookkeeping (P23.1) is created by `setup_water` at
    // startup, so no `init_resource` is needed here; the surface level the
    // underwater-fog pass reads is a small resource published by `drive_water`.
    .init_resource::<WaterLevel>()
    .init_resource::<PrimLodTargets>()
    .init_resource::<TreeLodTargets>()
    .init_resource::<LocalLights>()
    .init_resource::<AvatarState>()
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
            setup_diagnostics_overlay,
            setup_pipeline_overlay,
            setup_avatar_body,
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
            // within Bevy's per-tuple system limit.
            (handle_quit_input, enforce_quit_deadline),
            fly_camera,
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
            pick_object,
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
            update_diagnostics_overlay,
            // Key-toggled texture/mesh pipeline-status panel (P19.3): flip its
            // resource on the toggle key, then drive the panel's visibility and
            // (while shown) its text from the live store snapshots.
            toggle_pipeline_overlay,
            update_pipeline_overlay.after(toggle_pipeline_overlay),
            // Local lights (P25.2): render the nearest / brightest light-flagged
            // prims as Bevy point / spot lights, after the fly-camera so the
            // distance-based budget selection uses the current viewpoint.
            drive_local_lights.after(fly_camera),
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
            center_sky_on_camera.after(fly_camera),
            drive_sky.after(fly_camera),
            apply_sky_textures,
            // Sun / moon discs (P22.3): aim and colour the billboards from the same
            // active sky frame (after the fly-camera, so they track the viewpoint),
            // then swap each decoded disc texture into its material.
            drive_sun_moon_discs.after(fly_camera),
            apply_disc_textures,
            // Cloud layer (P22.4): fold the same active sky frame into the cloud
            // material, accumulate the scroll, and swap in the decoded cloud noise.
            drive_clouds.after(fly_camera),
            apply_cloud_textures,
            // Star field (P22.5): centre / rotate the field on the camera, fade it
            // in with the active sky frame's `star_brightness`, and swap in the
            // decoded bloom texture.
            drive_stars.after(fly_camera),
            apply_star_textures,
            // Water surface (P23.1): learn each region's water height, then centre
            // the endless ocean on the camera and place a per-region plane where a
            // neighbour's sea level differs, fold the EEP water settings into the
            // shared material (after the fly-camera, so the ocean tracks the
            // viewpoint), and swap in the decoded wave normal map.
            update_water,
            drive_water.after(fly_camera),
            apply_water_textures,
            // Underwater fog (P23.1): refresh the camera's fog parameters (water
            // level, EEP fog colour/density, reconstruction matrix) each frame,
            // after the fly-camera so the matrix matches the current viewpoint.
            update_underwater_fog.after(fly_camera).after(drive_water),
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
            drive_avatar_skeletons.after(apply_avatar_appearance),
            repeat_debug_animation,
            report_camera_interest,
            report_agent_viewport,
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
            camera_start,
            camera_spin,
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

/// Entry point: parse options, initialise logging, and run the viewer.
fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_ignored| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
    let options = Options::parse();
    run_viewer(&options)
}
