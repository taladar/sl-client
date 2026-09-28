//! **The one assembly of the viewer App.** [`ViewerAppBuilder`] turns a
//! [`ViewerAppOptions`] into the Bevy [`App`] the user runs: the protocol
//! plugin, the six plugin groups of `viewer_plugins`, the settings
//! store, the session driver, the camera, and the run-scoped overrides the
//! command line sets.
//!
//! # Why one function
//!
//! The binary, the full-stack harness and the end-to-end tier each need the
//! whole viewer. Before this module the binary assembled it in a private
//! function and the full-stack harness re-assembled a subset by hand, stubbing
//! the resources and messages of the groups it left out — so that tier never
//! ran the app the user runs, and every new plugin was one more stub to
//! remember. Now each of them asks for the viewer through
//! [`ViewerAppBuilder::build`], and what differs between them is an option with
//! a name:
//!
//! - [`WindowMode`]: an OS window and its event loop, or neither — a harness
//!   that steps `update` itself and renders into images.
//! - [`Storage`]: the user's settings, chat logs and caches, or nothing on
//!   disk at all.
//! - `audio_device`: whether the machine's speakers are opened.
//! - `render_overrides`: the render debug knobs read from the environment, or
//!   stated — a test must not change its answer because a developer exported
//!   `SL_VIEWER_DISABLE_GLOW` in their shell.
//!
//! Some things `run()` does are **process-wide** and stay out of the builder:
//! the tracing subscriber, the static-asset library, the termination-signal
//! handler and the replay cache root. Making those per App is its own task
//! (`viewer-automation-per-app-state`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::app::ScheduleRunnerPlugin;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::window::{CursorGrabMode, CursorOptions, ExitCondition};
use bevy::winit::WinitPlugin;
use sl_client_bevy::{
    AccountDirsConfig, AnimationKey, ChatLogConfig, ClientDirectories, InventoryCacheConfig,
    LoggedChatType, LoginFailure, LoginParams, MfaChallenge, SlClientPlugin, SlLoginRejected,
    SlMfaChallenge, Uuid,
};
use tracing::warn;

use crate::Error;
use crate::animations::AnimationManager;
use crate::avatar_assets::AvatarAssetLibrary;
use crate::camera::{CameraSpin, CameraStart};
use crate::input_context::CursorGrabAllowed;
use crate::render_overrides::RenderOverrides;
use crate::session::PlayOnLogin;
use crate::settings::{AccountContext, SettingsAgent, ViewerSettings, load_account_settings};
use crate::viewer_camera::viewer_camera_bundle;
use crate::viewer_plugins::{
    ViewerEditPlugins, ViewerInputPlugins, ViewerRenderPlugins, ViewerShellPlugins,
    ViewerUiPlugins, ViewerWorldPlugins,
};
use crate::world_api::{CameraMode, CameraRig};

/// Whether the viewer has an OS window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowMode {
    /// A winit window with its event loop and pipelined rendering: the viewer
    /// the user runs. [`ViewerApp::run`] returns when the window closes.
    #[default]
    Windowed,
    /// No window and no event loop: the caller steps [`App::update`] itself, so
    /// one update is one frame and the render runs inline on the caller's
    /// thread. Nothing reads the machine's input devices, the cursor is never
    /// grabbed, and only cameras aimed at an image render anything — a camera
    /// aimed at the (absent) window draws nothing. The full-stack harness is
    /// this mode; rendering and picking the interface with no window is
    /// `viewer-automation-windowless-mode`'s to add.
    Windowless,
}

/// Where the viewer keeps what it stores between sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Storage {
    /// The user's directories: the global and per-avatar settings files, the
    /// per-avatar chat logs and the inventory cache, under the XDG roots.
    #[default]
    UserDirectories,
    /// Nothing: every setting starts at its declared default and is never
    /// written, no chat is logged and no inventory is cached. A test viewer's
    /// storage — it must neither read the developer's preferences nor leave
    /// its own behind. (The asset caches are process-wide and not covered.)
    Ephemeral,
}

/// What a viewer session is fed at startup, beside the login parameters:
/// where its art comes from, which animations it plays on the own avatar,
/// whether it backfills group chat history from the server, and whether the
/// whole run is an offline replay rather than a login.
#[derive(Debug, Default)]
pub struct SessionContent {
    /// The Linden `character/` directory the avatar bodies are built from, or
    /// `None` to leave every avatar a placeholder sphere.
    pub viewer_assets: Option<PathBuf>,
    /// The animations to start on the own avatar once logged in
    /// (`--play-animation`).
    pub play_animation: Vec<Uuid>,
    /// Whether those animations are re-issued on a short cadence rather than
    /// played once (`--repeat-animation`).
    pub repeat_animation: bool,
    /// Whether to ask the server for group chat history at login; cleared by
    /// `--no-group-chat-history`.
    pub fetch_server_chat_history: bool,
    /// The avatar-state replay bundle (`--replay`), or `None` for a live login.
    /// Its presence is what puts the session in offline mode.
    pub replay: Option<crate::avatar_replay::ReplayConfig>,
}

/// The unattended capture harness's configuration: where the PNG sequence
/// goes, and what its frames hold. Capture settings without a directory are
/// meaningless, which reads better as one value than as several fields that
/// have to agree.
#[derive(Debug, Clone)]
pub struct CaptureStartup {
    /// The screenshot directory (`--screenshot-dir`), or `None` for an ordinary
    /// session.
    pub dir: Option<PathBuf>,
    /// The pixel grid the frames are rendered at and which layers of the
    /// composited frame they hold (`--capture-size`, `--capture-ui`,
    /// `--capture-hud`, `--capture-gizmos`).
    pub content: crate::screenshot::CaptureContent,
    /// Where the structured scene dump goes (`--scene-dump`), or `None` for
    /// `<screenshot-dir>/scene.json`.
    pub scene_dump: Option<PathBuf>,
    /// Whether the run may make sound (`--capture-audio`).
    pub audio: bool,
    /// The interface scale pinned for this run (`--capture-ui-scale`), or
    /// `None` to use the preference. Honoured with or without a screenshot
    /// directory, as the pinned lens is.
    pub ui_scale: Option<f32>,
}

impl Default for CaptureStartup {
    fn default() -> Self {
        Self {
            dir: None,
            content: crate::screenshot::CaptureContent::WORLD_ONLY,
            scene_dump: None,
            audio: false,
            ui_scale: None,
        }
    }
}

/// The camera's start-up configuration: the fixed pose (if any), the optional
/// auto-spin and a pinned lens.
#[derive(Debug, Default)]
pub struct CameraStartup {
    /// The fixed start pose, or the login-snapped default.
    pub start: CameraStart,
    /// The optional auto-spin survey pan.
    pub spin: CameraSpin,
    /// A vertical field of view in radians overriding the persisted
    /// `CameraAngle` for this run (`--camera-fov`, in degrees on the command
    /// line), or `None` to use the preference.
    pub field_of_view: Option<f32>,
}

/// The skin configuration: which skin / theme to wear and whether to
/// hot-watch the `.css` files.
#[derive(Debug, Default)]
pub struct SkinRuntime {
    /// The initial skin + theme selection.
    pub selection: crate::skin::SkinSelection,
    /// Whether to watch the skin `.css` files for live edits (`--watch-skins`).
    pub watch: bool,
}

/// Which media engines a viewer session may start: the web (CEF) and video
/// (GStreamer) switches from `--disable-web-media` / `--disable-video-media`.
#[derive(Debug, Clone, Copy)]
pub struct MediaRuntime {
    /// Whether the web (CEF) engine may initialise.
    pub web: bool,
    /// Whether the video (GStreamer) engine may initialise.
    pub video: bool,
    /// Whether to auto-login the grid account into the Second Life websites at
    /// login (`viewer-web-openid-auth`); cleared by `--no-web-auth`.
    pub web_auth: bool,
}

impl MediaRuntime {
    /// Every engine off, and no website login: an offline run, or a test
    /// binary, which has no `sl-cef-helper` beside it to start Chromium with.
    pub const OFF: Self = Self {
        web: false,
        video: false,
        web_auth: false,
    };
}

impl Default for MediaRuntime {
    /// Everything on: the interactive viewer.
    fn default() -> Self {
        Self {
            web: true,
            video: true,
            web_auth: true,
        }
    }
}

/// Everything a viewer App is built from. [`ViewerAppOptions::new`] is the
/// interactive viewer; a harness changes the fields it has a reason to.
#[derive(Debug)]
pub struct ViewerAppOptions {
    /// Who logs in, where, and as which viewer. A replay run carries a
    /// placeholder the offline plugin never uses.
    pub params: LoginParams,
    /// What the session is fed beside the login.
    pub content: SessionContent,
    /// The unattended capture harness, if this is a capture run.
    pub capture: CaptureStartup,
    /// Where the camera starts, and a pinned lens.
    pub camera: CameraStartup,
    /// The skin the interface wears.
    pub skin: SkinRuntime,
    /// Which media engines may start.
    pub media: MediaRuntime,
    /// Whether there is an OS window.
    pub window: WindowMode,
    /// Where settings, chat logs and the inventory cache live, if anywhere.
    pub storage: Storage,
    /// Whether the audio device is opened.
    pub audio_device: bool,
    /// The render debug knobs, or `None` to read them from the environment
    /// (`SL_VIEWER_DISABLE_GLOW`, `SL_VIEWER_SKY_DAY_POSITION`, …) — which is
    /// how the interactive viewer and a capture run take them.
    pub render_overrides: Option<RenderOverrides>,
}

impl ViewerAppOptions {
    /// The interactive viewer logging in with `params`: a window, the user's
    /// directories, sound, every media engine, the vendored avatar bodies and
    /// the render knobs from the environment.
    #[must_use]
    pub fn new(params: LoginParams) -> Self {
        Self {
            params,
            content: SessionContent {
                viewer_assets: default_viewer_assets(),
                fetch_server_chat_history: true,
                ..SessionContent::default()
            },
            capture: CaptureStartup::default(),
            camera: CameraStartup::default(),
            skin: SkinRuntime::default(),
            media: MediaRuntime::default(),
            window: WindowMode::Windowed,
            storage: Storage::UserDirectories,
            audio_device: true,
            render_overrides: None,
        }
    }
}

/// Builds the viewer App from a [`ViewerAppOptions`] — see the [module
/// documentation](self).
#[derive(Debug)]
pub struct ViewerAppBuilder {
    /// What to build.
    options: ViewerAppOptions,
}

/// The recoverable outcome of one session: an MFA challenge to answer or a
/// retryable login rejection, either of which stops the app.
#[derive(Resource, Debug, Default)]
pub struct LoginOutcome {
    /// The MFA challenge the session stopped on, if any.
    pub challenge: Option<MfaChallenge>,
    /// The retryable "already logged in" rejection, if any.
    pub rejected: Option<LoginFailure>,
}

/// A built viewer: [`run`](Self::run) it, or step it through
/// [`app_mut`](Self::app_mut).
#[derive(Debug)]
pub struct ViewerApp {
    /// The assembled App.
    app: App,
}

impl ViewerAppBuilder {
    /// A builder for the viewer `options` describe.
    #[must_use]
    pub const fn from_options(options: ViewerAppOptions) -> Self {
        Self { options }
    }

    /// Assemble the App. Nothing runs yet: no frame has been stepped and no
    /// login has started.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ScreenshotDir`] if the capture directory cannot be
    /// created — the run was started to take frames, and one that cannot write
    /// them has already failed.
    pub fn build(self) -> Result<ViewerApp, Error> {
        let ViewerAppOptions {
            params,
            content,
            capture,
            camera,
            skin,
            media,
            window,
            storage,
            audio_device,
            render_overrides,
        } = self.options;
        let SessionContent {
            viewer_assets,
            play_animation,
            repeat_animation,
            fetch_server_chat_history,
            replay,
        } = content;
        // Offline (avatar-state replay) mode: the plugin registers its event/resource
        // substrate but never logs in; the session is fed synthetic events from the
        // bundle instead (see `crate::avatar_replay`).
        let offline = replay.is_some();
        let CameraStartup {
            start: camera_start,
            spin: camera_spin,
            field_of_view: camera_fov,
        } = camera;
        let SkinRuntime {
            selection: skin,
            watch: watch_skins,
        } = skin;
        let windowed = window == WindowMode::Windowed;
        let user_directories = storage == Storage::UserDirectories;
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
        let account_dirs = user_directories.then(|| AccountDirsConfig {
            grid: grid.clone(),
            avatar: avatar.clone(),
            chat_log_base: crate::paths::state_accounts_base(),
            inventory_cache_base: crate::paths::cache_accounts_base(),
        });
        let config_accounts_base = user_directories
            .then(crate::paths::config_accounts_base)
            .flatten();

        // Resolve the system time zone now, while the process is still single-threaded:
        // it reads the `TZ` environment variable, and reading the environment is only
        // sound before Bevy's task pools spawn (below, with `DefaultPlugins`). The
        // snapshot floater reuses this cached zone to stamp filenames in local time.
        let local_time_zone = crate::local_time::LocalTimeZone::capture();

        let mut app = App::new();
        app.insert_resource(local_time_zone);
        // The render debug knobs (`SL_VIEWER_DISABLE_GLOW`, `SL_VIEWER_SKY_DAY_POSITION`,
        // …), read from the environment exactly once and only here — while the process
        // is still single-threaded — unless the caller stated them, and the environment
        // state their day-position pin seeds. Every consumer reads the resource.
        let render_overrides = render_overrides.unwrap_or_else(RenderOverrides::from_env);
        app.insert_resource(crate::environment::EnvironmentState::from_overrides(
            &render_overrides,
        ));
        app.insert_resource(render_overrides);
        // The About floater's login-derived facts (grid, login URI, reported
        // channel/version) — captured here where they are all still at hand.
        app.insert_resource(crate::about_floater::AboutSessionInfo {
            grid: grid.clone(),
            login_uri: params.login_uri.to_string(),
            channel: params.request.channel.clone(),
            version: params.request.version.clone(),
        });
        let default_plugins = DefaultPlugins
            // Resolve the viewer's own `assets/` (icons, locales, skins) rather
            // than inheriting Bevy's executable-relative default, which a binary
            // run out of `target/` finds nothing under — see `asset_root`.
            //
            // Watch the asset directory so an edited skin `.css` re-applies live
            // (`--watch-skins`, the skin-authoring loop). Off unless asked, since
            // watching carries a small background cost.
            .set(crate::asset_root::asset_plugin(watch_skins.then_some(true)))
            // The binary installs its own `tracing` subscriber (so the
            // pre-window login logs go somewhere), and a harness captures the
            // logs itself; drop Bevy's `LogPlugin` to avoid the "global
            // subscriber already set" clash.
            .disable::<LogPlugin>();
        match window {
            WindowMode::Windowed => {
                // Start the cursor free (visible, un-grabbed): the viewer opens in
                // third-person, whose pointer is free to click the world / UI.
                // `crate::input_context::drive_cursor_grab` captures it only when the
                // camera enters mouselook. (In screenshot mode it stays free
                // regardless, so an unattended capture run never hijacks the
                // desktop's pointer.)
                let cursor_options = CursorOptions {
                    grab_mode: CursorGrabMode::None,
                    visible: true,
                    ..default()
                };
                app.add_plugins(default_plugins.set(WindowPlugin {
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
                }));
            }
            WindowMode::Windowless => {
                app.add_plugins(
                    default_plugins
                        .set(WindowPlugin {
                            // No window at all, and the app must not exit for the
                            // lack of one.
                            primary_window: None,
                            exit_condition: ExitCondition::DontExit,
                            ..default()
                        })
                        // No event loop: the caller drives `update` itself, so the
                        // frames are counted rather than raced.
                        .disable::<WinitPlugin>()
                        // No render thread either: with the render app run inline,
                        // one `update` is exactly one rendered frame and everything
                        // the render world logs lands on the caller's thread.
                        .disable::<PipelinedRenderingPlugin>(),
                )
                .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO));
            }
        }
        app.insert_resource(skin).add_plugins(SlClientPlugin {
            params: params.clone(),
            diagnostics: true,
            // Log every text-chat type to the per-avatar chat directory — the
            // pre-login default; once the account settings load,
            // `preferences_chat` pushes the avatar's stored logging preferences
            // over this via `Command::SetChatLogConfig`. With no directories
            // there is nowhere to log to.
            chat_log_config: if user_directories {
                ChatLogConfig {
                    enabled: BTreeSet::from([
                        LoggedChatType::Nearby,
                        LoggedChatType::InstantMessage,
                        LoggedChatType::Group,
                        LoggedChatType::Conference,
                    ]),
                    ..ChatLogConfig::default()
                }
            } else {
                ChatLogConfig::default()
            },
            directories: ClientDirectories::default(),
            account_dirs,
            // Cache the inventory tree per avatar (agent tree + Library).
            inventory_cache_config: InventoryCacheConfig {
                enabled: user_directories,
                cache_library: user_directories,
            },
            // **On**, which the default is not. The flag defaults off so a *library*
            // consumer that ignores inventory pays nothing for it; a viewer is the
            // other case entirely, and leaving it off meant caching a tree we never
            // fetched — the persistence paid for, the completeness not.
            //
            // Everything that asks a question of the whole inventory is wrong
            // without it, and wrong *silently*, because a folder nobody has expanded
            // simply contributes nothing: inventory search matches only what has
            // been browsed to, and the settings surfaces (the quick-preferences
            // combos, the My Environments library, the settings picker) listed only
            // the folders the user happened to have opened — which is how a freshly
            // created sky came to be invisible until its folder was clicked.
            //
            // The cost is bounded and paid once per account: the crawl is
            // breadth-first with a bounded number of folder-contents requests in
            // flight, and the on-disk cache above reconciles against the login
            // skeleton so version-matching folders skip the refetch on every later
            // login.
            background_inventory_fetch: true,
            fetch_server_chat_history,
            offline,
        });
        // The six plugin groups (`crate::viewer_plugins`): the interface first
        // (the world group's pie menus need its scaffold), then input, the render
        // stack (whose `SlFaceMaterialPlugin` the editor plugins' `FromWorld`
        // resources build against), the world fold, the build tools, and what
        // the viewer needs of the machine.
        app.add_plugins(ViewerUiPlugins)
            .add_plugins(if windowed {
                ViewerInputPlugins::default()
            } else {
                ViewerInputPlugins::without_devices()
            })
            .add_plugins(ViewerRenderPlugins::default())
            .add_plugins(ViewerWorldPlugins::default())
            .add_plugins(ViewerEditPlugins)
            .add_plugins(ViewerShellPlugins {
                audio_device,
                media,
            });
        app
            // The per-avatar account identity (grid + name + accounts root), used by
            // `load_account_settings` to locate the account-scope settings once the
            // agent UUID is known at login. No accounts root, no account scope.
            .insert_resource(AccountContext {
                accounts_base: config_accounts_base,
                grid,
                avatar,
            })
            // The viewer settings store (viewer-ui-settings-store), the reference's
            // `gSavedSettings`: registers each feature's settings and loads any persisted
            // global overrides (e.g. SpaceNavigator sensitivities). `REGISTRARS` is the
            // binary's to hold — a store that named its own users would depend on all of
            // them — so the store is inserted here and only its *persistence* is a plugin.
            // With no storage the store has no file behind it: every setting reads its
            // declared default and nothing is ever written.
            .insert_resource(if user_directories {
                ViewerSettings::load_with(crate::paths::global_settings_file(), crate::REGISTRARS)
            } else {
                ViewerSettings::declared_for_test(crate::REGISTRARS)
            })
            // Hand the settings store the one runtime fact its account-scope loader
            // needs. It reads this mirror rather than `SlIdentity` so that the
            // protocol stack does not sit underneath every crate that reads a
            // setting.
            .add_systems(Update, mirror_agent_id.before(load_account_settings))
            // The debug camera override (`--camera-position` / `--camera-look-at` /
            // `--camera-spin`): `setup_scene` reads the start pose, `drive_flycam` reads
            // the spin, and third-person auto-follows when no pose is fixed. The world
            // context may grab the cursor (only in mouselook) unless this is an
            // unattended screenshot run or has no window, whose whole point is to
            // leave the desktop's pointer alone.
            .insert_resource(CursorGrabAllowed(windowed && capture.dir.is_none()))
            .insert_resource(camera_start)
            .insert_resource(camera_spin)
            .init_resource::<LoginOutcome>()
            .insert_resource(AnimationManager::new())
            // The session driver and its shutdown: the `SlEvent` fold, the draw
            // distance and interest-camera reports, the graceful logout every quit
            // path routes through, and the synchronous exit save. `--repeat-animation`
            // is the one part that is a run's choice rather than the session's.
            .add_plugins(crate::session::SessionDriverPlugin { repeat_animation })
            // The debug animations to play on the own avatar once it lands
            // (`--play-animation`), over the plugin's "play nothing" default.
            .insert_resource(PlayOnLogin {
                animations: play_animation
                    .iter()
                    .copied()
                    .map(AnimationKey::from)
                    .collect(),
                repeat: repeat_animation,
            })
            .add_systems(Startup, setup_scene)
            .add_systems(Update, capture_login_outcome);
        // (Worn rigid attachments no longer need a hand re-propagation: their
        // attachment-point node is an avatar-root child whose local `Transform` the
        // pose driver's socket writer sets each frame, so ordinary change-gated
        // propagation seats the worn subtree — the former `pose_attachment_nodes`
        // pass, Phase 4 §5.4.)
        // Load the client-side avatar assets (if a directory was given) so rigged
        // bodies replace the placeholder spheres; absent them the viewer keeps spheres.
        if let Some(library) = load_avatar_library(viewer_assets.as_deref()) {
            app.insert_resource(library);
        }
        if repeat_animation && play_animation.is_empty() {
            // There is nothing to repeat, and a silent no-op looks exactly like a
            // run that worked — the same reasoning as the `--capture-*` warnings.
            // (The repeat *system* is `SessionDriverPlugin`'s to add or not.)
            warn!("--repeat-animation has no effect without --play-animation");
        }
        // Avatar-state replay (viewer-avatar-state-dump-replay): inject the bundle's
        // captured events once and drive the optional test rig (orbit light /
        // reflection probe). Only present in `--replay` mode.
        if let Some(config) = replay {
            app.insert_resource(config)
                .add_plugins(crate::avatar_replay::AvatarReplayPlugin);
        }
        // In screenshot mode, capture a numbered PNG sequence after a startup delay,
        // then quit (the R11 offline-inspection harness) — from the window, or from
        // an off-screen target of the pinned `--capture-size` when one was asked for.
        if let Some(dir) = capture.dir.as_deref() {
            // Abort here rather than warning and running on. A run given
            // `--screenshot-dir` exists to write frames into it; carrying on gives
            // a `ScreenshotPlugin` that fails on every single capture, which buries
            // the one error that explains why under a run's worth of noise.
            fs_err::create_dir_all(dir).map_err(Error::ScreenshotDir)?;
            if !capture.audio {
                app.insert_resource(crate::volume_panel::SilenceAudioForRun);
            }
            app.add_plugins(crate::screenshot::ScreenshotPlugin {
                dir: dir.to_path_buf(),
                content: capture.content,
                // A replay run has no grid, so it must not wait for a region to come
                // up before it photographs the avatar it rebuilt offline.
                grid_expected: !offline,
            });
            // The structured description of the scene those frames were taken from,
            // in the same document the patched Firestorm writes — so a frame pair
            // that differs can be asked *why* it differs. `--scene-dump` moves it;
            // by default it lands beside the frames it describes.
            app.add_plugins(crate::scene_dump::SceneDumpPlugin {
                path: capture
                    .scene_dump
                    .clone()
                    .unwrap_or_else(|| dir.join("scene.json")),
                identity: crate::scene_dump::DumpIdentity {
                    channel: params.request.channel.clone(),
                    version: params.request.version.clone(),
                    grid: sl_account_dirs::grid_dir_name(&params.login_uri),
                },
            });
        } else if capture.scene_dump.is_some() {
            // A dump describes the scene a capture's last frame was taken from, so
            // without a capture there is no moment to describe. Say so rather than
            // writing nothing and leaving the operator to wonder.
            warn!("--scene-dump has no effect without --screenshot-dir");
        } else if capture.content != crate::screenshot::CaptureContent::WORLD_ONLY {
            // Capture knobs with nothing to capture is a mistyped command line, and a
            // silent no-op would look exactly like a run that worked.
            warn!("the --capture-* options have no effect without --screenshot-dir");
        }
        // A lens pinned for this run (`--camera-fov`), which beats the persisted
        // `CameraAngle` without rewriting it.
        if let Some(radians) = camera_fov {
            app.insert_resource(crate::preferences_camera_move::CameraFovOverride { radians });
        }
        // Likewise an interface scale pinned for this run (`--capture-ui-scale`).
        if let Some(factor) = capture.ui_scale {
            app.insert_resource(crate::preferences_general::UiScaleOverride { factor });
        }
        Ok(ViewerApp { app })
    }
}

impl ViewerApp {
    /// Run the App to completion, returning any recoverable login outcome (an
    /// MFA challenge or a retryable rejection) it stopped on.
    ///
    /// # Errors
    ///
    /// Returns [`Error::AppFailed`] if the Bevy app exited with a failing
    /// status, which is **not** a recoverable outcome: the caller must not retry
    /// it the way it retries an MFA challenge.
    pub fn run(mut self) -> Result<LoginOutcome, Error> {
        let exit = self.app.run();
        // Taken before the exit is judged, so the outcome is out of the world
        // either way — but reported only on a clean exit. An app that failed has
        // not "stopped on an MFA challenge"; it stopped on the failure, and
        // handing the caller a challenge to retry would send it round the login
        // loop again on the strength of a run that never got that far.
        let outcome = self
            .app
            .world_mut()
            .remove_resource::<LoginOutcome>()
            .unwrap_or_default();
        match exit {
            AppExit::Success => Ok(outcome),
            AppExit::Error(code) => Err(Error::AppFailed(code)),
        }
    }

    /// The App, for a caller that steps it frame by frame or adds to it.
    pub const fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// The App itself, for a caller that owns it from here on.
    pub fn into_app(self) -> App {
        self.app
    }
}

/// The vendored character directory (`viewer-assets/character/` at the
/// workspace root, see its README for provenance), when this build still sits
/// beside its sources — the default for `--viewer-assets` /
/// `SL_VIEWER_ASSETS`, so avatars get the real Linden bodies out of the box
/// while an explicit flag or environment variable still overrides.
#[must_use]
pub fn default_viewer_assets() -> Option<PathBuf> {
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("viewer-assets/character");
    vendored.is_dir().then_some(vendored)
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

/// Startup system: spawn the one [`ViewerCamera`](crate::world_api::ViewerCamera).
/// The scene's directional light (the sun / moon) is spawned by
/// `sky::setup_sky`, which also drives it from the region's environment.
///
/// The camera starts in third-person, which follows the avatar as soon as it
/// arrives (`camera::position_camera`), so no login camera-snap is needed. A fixed
/// `--camera-position` instead starts it in **flycam** at that absolute pose (and
/// aims it), which is what the unattended screenshot harness frames from; the
/// `SL_VIEWER_CAMERA_*` envs seed the third-person orbit so the harness can also
/// frame the avatar from a chosen angle.
pub(crate) fn setup_scene(
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
    commands.spawn((viewer_camera_bundle(camera_transform), rig));
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

/// Mirror the logged-in agent's UUID from the runtime's `SlIdentity` into
/// [`SettingsAgent`], which is all `sl-viewer-settings`' account-scope loader
/// needs to know about login.
///
/// The settings store is a floor nearly every crate in the viewer stands on, so
/// it names no runtime: reading `SlIdentity` there put `sl-proto`, `sl-wire`,
/// `sl-asset`, `reqwest` and `tokio` underneath all of them for one `Uuid`. The
/// composition root is the one place that legitimately knows both sides, so the
/// mirroring lives here.
fn mirror_agent_id(identity: Res<sl_client_bevy::SlIdentity>, mut agent: ResMut<SettingsAgent>) {
    let current = identity.agent_id.map(|id| id.uuid());
    if agent.0 != current {
        agent.0 = current;
    }
}
