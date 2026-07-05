//! Minimum-viable Bevy visual viewer for Second Life / OpenSim.
//!
//! See the crate `README.md` and `VIEWER_ROADMAP.md` for the staged plan. This
//! binary logs in via the shared `credentials.toml` mechanism (`sl-repl::auth`)
//! and opens a window that renders a region. This is the Phase 1 slice — the
//! viewer shell: window, login, a debug fly-camera, and a clean quit key —
//! with terrain, prims, meshes, sculpts, avatars, and chat landing in later
//! phases.

mod appearance;
mod avatar_assets;
mod avatars;
mod camera;
mod chat;
mod coords;
mod meshes;
mod objects;
mod session;
mod terrain;
mod textures;

use std::path::{Path, PathBuf};

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use clap::Parser as _;
use sl_client_bevy::{
    ChatLogConfig, ClientDirectories, InventoryCacheConfig, LoginFailure, LoginParams,
    LoginRequest, MfaChallenge, SlClientPlugin, SlLoginRejected, SlMfaChallenge, StartLocation,
    TerrainMaterialPlugin,
};
use sl_repl::{Avatar, Credentials};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::appearance::{ServerBakeState, drive_server_bake};
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::{
    AvatarBakeMaterials, AvatarState, apply_avatar_appearance, apply_avatar_bake_textures,
    apply_avatar_names, apply_avatar_part_visibility, assign_avatar_bake_materials,
    ingest_avatar_bakes, position_name_tags, setup_avatar_body, update_avatar_objects,
    update_coarse_avatars,
};
use crate::camera::{FlyCamera, fly_camera};
use crate::chat::{ChatOverlay, setup_chat_overlay, update_chat_overlay};
use crate::meshes::{MeshDecoded, MeshManager, poll_meshes, update_mesh_caps};
use crate::objects::{ObjectState, apply_object_meshes, apply_object_sculpts, update_objects};
use crate::session::{ViewerSession, drive_session, enforce_quit_deadline, handle_quit_input};
use crate::terrain::{TerrainState, recenter_terrain, update_terrain};
use crate::textures::{
    PrimTextures, TextureDecoded, TextureManager, apply_prim_textures, poll_textures,
    update_texture_caps,
};

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

/// Startup system: spawn the fly-camera and a directional light.
fn setup_scene(mut commands: Commands) {
    // A provisional camera pose near a region centre (256 m region, ~30 m up);
    // `drive_session` snaps it to the agent's real login position once the
    // agent's avatar object arrives.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(128.0, 30.0, -128.0),
        FlyCamera::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
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
fn run_session(params: &LoginParams, viewer_assets: Option<&Path>) -> LoginOutcome {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "sl-client-bevy-viewer".to_owned(),
                    ..default()
                }),
                // Capture and hide the cursor so raw mouse motion drives
                // mouse-look.
                primary_cursor_options: Some(CursorOptions {
                    grab_mode: CursorGrabMode::Locked,
                    visible: false,
                    ..default()
                }),
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
    .init_resource::<ViewerSession>()
    .init_resource::<LoginOutcome>()
    .init_resource::<TerrainState>()
    .init_resource::<ObjectState>()
    .init_resource::<AvatarState>()
    .init_resource::<ChatOverlay>()
    .init_resource::<TextureManager>()
    .init_resource::<PrimTextures>()
    .init_resource::<AvatarBakeMaterials>()
    .init_resource::<ServerBakeState>()
    .init_resource::<MeshManager>()
    .add_message::<TextureDecoded>()
    .add_message::<MeshDecoded>()
    .add_systems(
        Startup,
        (setup_scene, setup_chat_overlay, setup_avatar_body),
    )
    .add_systems(
        Update,
        (
            capture_login_outcome,
            drive_session,
            // Trigger our own avatar's server-side bake so P14 has bakes to fetch.
            drive_server_bake,
            // Keep the texture store's `GetTexture` cap current, then poll
            // finished fetches before the consumers that apply them.
            update_texture_caps,
            poll_textures,
            // The same for the mesh store's `GetMesh2` / `GetMesh` cap.
            update_mesh_caps,
            poll_meshes,
            // Recenter (origin follows the root region) before folding terrain
            // events, so patches are placed on the current origin.
            (recenter_terrain, update_terrain).chain(),
            update_objects,
            // Build the geometry of any mesh object whose asset just decoded, and
            // of any sculpted prim whose sculpt map just decoded.
            apply_object_meshes,
            apply_object_sculpts,
            apply_prim_textures,
            // Avatar placeholder spheres: full-object avatars first, then the
            // coarse-only ones (which dedupe against the full-object set); then
            // fold resolved names in and float each name tag over its sphere.
            (update_avatar_objects, update_coarse_avatars).chain(),
            apply_avatar_names,
            // Re-shape each rigged body from its avatar's visual params — morph
            // targets (P13.3) and skeletal proportions (P13.4) — show/hide whole
            // base regions from the worn skirt / mesh-body items (P13.5), then
            // fetch each avatar's server-published baked textures (P14.1) and
            // drape them over the matching body regions (P14.2), filling each
            // region material once its bake decodes. Nested into one tuple to stay
            // within Bevy's per-tuple system limit.
            (
                apply_avatar_appearance,
                apply_avatar_part_visibility,
                ingest_avatar_bakes,
                assign_avatar_bake_materials,
                apply_avatar_bake_textures,
            ),
            position_name_tags,
            // Append newly received local chat to the on-screen overlay.
            update_chat_overlay,
            handle_quit_input,
            enforce_quit_deadline,
            fly_camera,
        ),
    );
    // Load the client-side avatar assets (if a directory was given) so rigged
    // bodies replace the placeholder spheres; absent them the viewer keeps spheres.
    if let Some(library) = load_avatar_library(viewer_assets) {
        app.insert_resource(library);
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
        let outcome = run_session(&params, options.viewer_assets.as_deref());
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
