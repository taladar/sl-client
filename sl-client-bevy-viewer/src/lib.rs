//! Bevy visual viewer for Second Life / OpenSim.
//!
//! See the crate `README.md` and the `viewer` topic under `roadmap/` for the
//! staged plan. [`run`] logs in via the shared `credentials.toml` mechanism
//! (`sl-repl::auth`) and opens a window that renders a region: terrain, prims,
//! meshes, sculpts, avatars and chat.
//!
//! # Why this is a library
//!
//! The crate builds **three** binaries over one module tree:
//!
//! - `sl-client-bevy-viewer` (`src/main.rs`) — the viewer proper, a thin shell
//!   over [`run`].
//! - `sl-client-bevy-viewer-gallery` (`src/bin/`) — the UI gallery
//!   (`viewer-ui-test-harness`), and
//! - `sl-client-bevy-viewer-scenes` (`src/bin/`) — the render gallery
//!   (`viewer-render-test-harness`).
//!
//! The galleries themselves are `sl-viewer-gallery`; the two shells here exist
//! because what a gallery shows is composition. [`ui_elements::ELEMENTS`] and
//! [`floaters::FLOATERS`] name the feature modules, [`asset_root`] resolves the
//! `assets/` tree against **this** crate's compile-time directory, and
//! [`init_tracing`] installs the subscriber the `profile-*` features configure.
//! A shell gathers those four and hands them over.
//!
//! What still needs a library is `main.rs` and those shells sharing one
//! `pub(crate)` module tree — two binaries cannot, and re-`#[path]`-including
//! the same files would compile them twice and leave every item either binary
//! happens not to use tripping `dead_code`.
//!
//! Only what a shell actually calls ([`run`], [`Error`], [`init_tracing`],
//! [`asset_root`], [`floaters`], [`ui_elements`]) is `pub`, and
//! [`assembly`], the one assembly of the viewer App that the binary, the
//! full-stack harness and the end-to-end tier share; the rest of the module
//! tree stays `pub(crate)` exactly as it was.

mod about_floater;
pub(crate) use sl_viewer_people::add_friend;
pub(crate) use sl_viewer_places::about_land;
pub(crate) use sl_viewer_places::about_landmark;
pub(crate) use sl_viewer_places::about_region;
pub(crate) use sl_viewer_places::telehub;
pub(crate) use sl_viewer_places::top_objects;
pub(crate) use sl_viewer_world_avatar::animations;
pub mod assembly;
/// Every module that declares settings, in registration order.
///
/// This list lives here rather than in `settings` because a store that
/// named its own users would have to depend on all of them — the reason the
/// settings module could not be a crate of its own before. The binary is the
/// composition root and already depends on everything, so it is the honest
/// place for it.
///
/// `settings_golden` pins the surface this produces; adding a registrar
/// without updating that golden file fails the test, and dropping one
/// silently would otherwise revert a user's saved value to its default.
pub(crate) const REGISTRARS: &[fn(&mut crate::settings::ViewerSettings)] = &[
    crate::spacenav::register_settings,
    crate::minimap::register_settings,
    crate::double_click_teleport::register_settings,
    crate::parcel_borders::register_settings,
    crate::edit_land::register_settings,
    crate::world_map::register_settings,
    crate::search::register_settings,
    crate::tonemap::register_settings,
    crate::resolution_divisor::register_settings,
    crate::glow::register_settings,
    crate::exposure::register_settings,
    crate::snapshot_floater::register_settings,
    crate::panorama::register_settings,
    crate::i18n::register_settings,
    crate::avatars::register_settings,
    crate::hover_text::register_settings,
    crate::hover_tooltip::register_settings,
    crate::preferences_camera_move::register_settings,
    crate::preferences_chat::register_settings,
    crate::preferences_colors_skins::register_settings,
    crate::preferences_general::register_settings,
    crate::preferences_graphics::register_settings,
    crate::preferences_network_cache::register_settings,
    crate::presence::register_settings,
    crate::auto_reject::register_settings,
    crate::skin_colors::register_settings,
    crate::session::register_settings,
    crate::render_priority::register_settings,
    crate::particles::register_settings,
    crate::ui_sounds::register_settings,
    crate::audio::register_settings,
    crate::debug_settings::register_settings,
    crate::notification_host::register_settings,
    crate::rlv::register_settings,
    crate::environment::register_settings,
    crate::experience_log::register_settings,
    crate::experiences_floater::register_settings,
];

// The leaf toolkit (geometry math, render leaves, small models) is its own
// crate; each module is aliased under its old name so every
// `crate::<module>::…` path in the viewer still resolves.
pub(crate) use sl_viewer_world_avatar::asset_blacklist;
pub mod asset_root;
pub(crate) use sl_viewer_world_avatar::avatar_asset_stats;
pub(crate) use sl_viewer_world_objects::asset_stats;
// The platform layer (directory layout, on-disk caches, clipboard, URL
// linkification) is its own crate; each module is aliased under its old
// name so every `crate::<module>::…` path in the viewer still resolves.
pub(crate) use sl_viewer_audio::audio;
pub(crate) use sl_viewer_kit::avatar_assets;
pub(crate) use sl_viewer_people::auto_reject;
pub(crate) use sl_viewer_people::avatar_profile;
pub(crate) use sl_viewer_people::blocked;
pub(crate) use sl_viewer_pickers::avatar_picker;
pub(crate) use sl_viewer_ui_context_menus::attachment_menu;
pub(crate) use sl_viewer_ui_context_menus::avatar_menu;
pub(crate) use sl_viewer_world_avatar::avatar_complexity;
pub(crate) use sl_viewer_world_avatar::avatar_dump;
pub(crate) use sl_viewer_world_avatar::avatar_render_floater;
pub(crate) use sl_viewer_world_avatar::avatar_render_settings;
pub(crate) use sl_viewer_world_avatar::avatar_replay;
pub(crate) use sl_viewer_world_avatar::avatars;
pub(crate) use sl_viewer_world_scene::beacons;
pub(crate) use sl_viewer_world_scene::debug_beacons;
mod bottom_toolbar;
// Media (the CEF / GStreamer backends, the browser widget) is its own crate;
// each module is aliased under its old name so every `crate::<module>::…`
// path in the viewer still resolves.
pub(crate) use sl_viewer_media::browser_widget;
#[cfg(test)]
mod automation_locator;
#[cfg(test)]
mod automation_model;
#[cfg(test)]
mod build_floater_test;
mod build_info;
pub(crate) use sl_viewer_chat::chat;
pub(crate) use sl_viewer_chat::chat_input;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_people::contact_sets;
pub(crate) use sl_viewer_people::contact_sets_panel;
pub(crate) use sl_viewer_people::conversations;
pub(crate) use sl_viewer_platform::clipboard;
pub(crate) use sl_viewer_world_view::camera;
mod crowd_debug_button;
pub(crate) use sl_viewer_preferences::debug_settings;
pub(crate) use sl_viewer_world_avatar::derender;
pub(crate) use sl_viewer_world_scene::diagnostics;
mod double_click_teleport;
pub(crate) use sl_viewer_asset_editors::asset_editor;
pub(crate) use sl_viewer_asset_editors::edit_notecard;
pub(crate) use sl_viewer_asset_editors::edit_script;
pub(crate) use sl_viewer_asset_editors::edit_wearable;
pub(crate) use sl_viewer_edit::edit_contents;
pub(crate) use sl_viewer_edit::edit_create;
pub(crate) use sl_viewer_edit::edit_land;
pub(crate) use sl_viewer_edit::edit_link;
pub(crate) use sl_viewer_edit::edit_material;
pub(crate) use sl_viewer_edit::edit_material_asset;
pub(crate) use sl_viewer_edit::edit_media;
pub(crate) use sl_viewer_edit::edit_params;
pub(crate) use sl_viewer_edit::edit_selection;
pub(crate) use sl_viewer_edit::edit_texture;
pub(crate) use sl_viewer_edit::edit_tool;
pub(crate) use sl_viewer_edit::edit_undo;
pub(crate) use sl_viewer_intents as intents;
pub(crate) use sl_viewer_social as social;
/// The shared world state every feature surface reads: the selection, the
/// edit modes, the mute and buddy lists, group memberships, presence and map
/// tracking. Aliased so the call sites read as a module of this crate.
pub(crate) use sl_viewer_world_api as world_api;
// The widgets (floaters, menus, inputs, tabs, tables) are their own crate;
// each module is aliased under its old name so every `crate::<module>::…`
// path in the viewer still resolves.
pub(crate) use sl_viewer_chat::emoji_complete;
pub(crate) use sl_viewer_chat::emoji_picker;
pub(crate) use sl_viewer_kit::face_material;
pub(crate) use sl_viewer_notices::experience_log;
pub(crate) use sl_viewer_notices::experience_permission;
pub(crate) use sl_viewer_notices::experience_picker;
pub(crate) use sl_viewer_notices::experience_profile;
pub(crate) use sl_viewer_notices::experiences_floater;
pub(crate) use sl_viewer_platform::file_dialog;
pub(crate) use sl_viewer_ui_widgets::floater;
#[cfg(test)]
mod floater_chrome;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub mod floaters;
pub(crate) use sl_viewer_edit::gizmos;
pub(crate) use sl_viewer_people::group_notice;
pub(crate) use sl_viewer_people::group_profile;
pub(crate) use sl_viewer_people::groups;
pub(crate) use sl_viewer_pickers::group_picker;
pub(crate) use sl_viewer_world_avatar::gpu_avatar_spike;
pub(crate) use sl_viewer_world_avatar::gpu_avatars;
pub(crate) use sl_viewer_world_objects::hover_text;
pub(crate) use sl_viewer_world_scene::environment;
pub(crate) use sl_viewer_world_scene::exposure;
pub(crate) use sl_viewer_world_scene::glow;
pub(crate) use sl_viewer_world_view::gpu_pick;
pub(crate) use sl_viewer_world_view::hover_tooltip;
pub(crate) use sl_viewer_world_view::hud;
pub(crate) use sl_viewer_world_view::hud_pick;
// The UI vocabulary (scaffold, fonts, skin, Fluent) is its own crate; each
// module is aliased under its old name so every `crate::<module>::…` path in
// the viewer still resolves.
pub(crate) use sl_viewer_chat::local_chat_input;
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_inventory::inventory_actions;
pub(crate) use sl_viewer_inventory::inventory_drag;
pub(crate) use sl_viewer_inventory::inventory_filters;
pub(crate) use sl_viewer_inventory::inventory_gallery;
pub(crate) use sl_viewer_inventory::inventory_properties;
pub(crate) use sl_viewer_inventory::settings_index;
pub(crate) use sl_viewer_media::media_diagnostics;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_notices::inspector_popup;
pub(crate) use sl_viewer_notices::linkified_text;
pub(crate) use sl_viewer_notices::load_url;
pub(crate) use sl_viewer_ui_context_menus::land_menu;
pub(crate) use sl_viewer_ui_core::i18n;
#[cfg(test)]
mod i18n_keys;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_world_objects::material_preview;
pub(crate) use sl_viewer_world_scene::lights;
pub(crate) use sl_viewer_world_view::input_action;
pub(crate) use sl_viewer_world_view::input_context;
pub(crate) use sl_viewer_world_view::media_controls;
pub(crate) use sl_viewer_world_view::media_prim;
pub(crate) use sl_viewer_world_view::nearby_media;
mod menu_bar;
mod menu_search;
pub(crate) use sl_viewer_asset_editors::notecard_render;
pub(crate) use sl_viewer_chat::nearby_chat_bar;
pub(crate) use sl_viewer_map::minimap;
pub(crate) use sl_viewer_notices::notification_host;
pub(crate) use sl_viewer_notices::notification_persist;
pub(crate) use sl_viewer_people::mutes;
pub(crate) use sl_viewer_world_objects::name_tag_billboard;
// The object layer schedules itself now (`WorldObjectsPlugin`), so the binary's
// own code no longer names these two modules — only the harnesses do, for the
// object fixtures they build worlds out of. Aliased under `cfg(test)` so a
// viewer build does not carry an import nothing reads.
#[cfg(test)]
pub(crate) use sl_viewer_world_objects::meshes;
#[cfg(test)]
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_view::movement;
// The notification catalogue is its own crate (~22k lines of declarative data
// with no dependency on anything else here), aliased under its old module name
// so every `crate::notifications::…` path in the viewer still resolves.
pub(crate) use sl_viewer_audio::parcel_audio;
pub(crate) use sl_viewer_environment::bulk_import;
pub(crate) use sl_viewer_environment::day_cycle_editor;
pub(crate) use sl_viewer_environment::my_environments;
pub(crate) use sl_viewer_environment::personal_lighting;
pub(crate) use sl_viewer_environment::settings_editor;
pub(crate) use sl_viewer_environment::settings_picker;
pub(crate) use sl_viewer_kit::parcel_names;
pub(crate) use sl_viewer_kit::particle_render;
pub(crate) use sl_viewer_kit::raycast_index;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_people::offers_invites;
pub(crate) use sl_viewer_people::people;
pub(crate) use sl_viewer_people::presence;
pub(crate) use sl_viewer_people::radar;
pub(crate) use sl_viewer_platform::local_time;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_preferences::phototools;
pub(crate) use sl_viewer_preferences::preferences;
pub(crate) use sl_viewer_preferences::preferences_alerts;
pub(crate) use sl_viewer_preferences::preferences_audio;
pub(crate) use sl_viewer_preferences::preferences_camera_move;
pub(crate) use sl_viewer_preferences::preferences_chat;
pub(crate) use sl_viewer_preferences::preferences_colors_skins;
pub(crate) use sl_viewer_preferences::preferences_general;
pub(crate) use sl_viewer_preferences::preferences_graphics;
pub(crate) use sl_viewer_preferences::preferences_network_cache;
pub(crate) use sl_viewer_preferences::quick_preferences;
pub(crate) use sl_viewer_preferences::quick_prefs_environment;
pub(crate) use sl_viewer_rlv::rlv_behaviours;
pub(crate) use sl_viewer_rlv::rlv_console;
pub(crate) use sl_viewer_rlv::rlv_locks;
pub(crate) use sl_viewer_rlv::rlv_strings;
pub(crate) use sl_viewer_ui_context_menus::object_menu;
pub(crate) use sl_viewer_ui_pie_menu::pie_menu;
pub(crate) use sl_viewer_world_api::rlv;
pub(crate) use sl_viewer_world_objects::render_priority;
pub(crate) use sl_viewer_world_scene::parcel_borders;
pub(crate) use sl_viewer_world_scene::parcel_owners;
pub(crate) use sl_viewer_world_scene::particles;
pub(crate) use sl_viewer_world_scene::probes;
pub(crate) use sl_viewer_world_view::panorama;
pub(crate) use sl_viewer_world_view::physics;
#[cfg(test)]
mod full_stack_test;
#[cfg(test)]
mod pixel_oracle;
#[cfg(test)]
mod render_matrix;
#[cfg(test)]
mod render_readback;
mod viewer_plugins;
#[cfg(test)]
mod world_test;
pub(crate) use sl_viewer_world_scene::render_overrides;
pub(crate) use sl_viewer_world_scene::resolution_divisor;
// Only the render-harness tiers (`render_matrix`, `render_readback`,
// `render_test`) build scenes; the viewer proper builds the world.
#[cfg(test)]
pub(crate) use sl_viewer_render_fixtures as render_scene;
#[cfg(test)]
mod render_test;
pub(crate) use sl_viewer_notices::script_dialog;
pub(crate) use sl_viewer_notices::script_permission;
pub(crate) use sl_viewer_search::search;
pub(crate) use sl_viewer_world_avatar::replay_bundle;
pub(crate) use sl_viewer_world_view::scene_dump;
pub(crate) use sl_viewer_world_view::screenshot;
pub(crate) use sl_viewer_world_view::session;
pub(crate) use sl_viewer_world_view::watch_window;
// The settings store is its own crate now that it no longer names the
// features that register with it — that list is `REGISTRARS` above.
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
#[cfg(test)]
mod settings_golden;
mod shadow_visibility;
pub(crate) use sl_viewer_kit::sky_presets;
pub(crate) use sl_viewer_kit::slt;
pub(crate) use sl_viewer_ui_core::skin;
mod skin_agreement;
pub(crate) use sl_viewer_places::slurl_dispatch;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_world_scene::sky;
pub(crate) use sl_viewer_world_view::sit_camera;
mod snapshot_floater;
pub(crate) use sl_viewer_platform::sound_cache;
pub(crate) use sl_viewer_spacenav as spacenav;
mod stand_stop_button;
mod status_bar;
pub(crate) use sl_viewer_places::teleport_progress;
pub(crate) use sl_viewer_world_scene::tonemap;
// Per-kind entity-population diagnostics streamed to Tracy; only compiled with
// the Tracy client present (it exists solely to feed the profiler).
#[cfg(feature = "profile-tracy")]
pub(crate) use sl_viewer_world_scene::entity_diagnostics;
// Live circuit-count diagnostic streamed to Tracy; only compiled with the Tracy
// client present (it exists solely to feed the profiler).
#[cfg(feature = "profile-tracy")]
mod net_diagnostics;
// Tracy plot streaming + physics secondary frame mark; only compiled when the
// Tracy client (and its `tracing-tracy` bridge) is present.
#[cfg(feature = "profile-tracy")]
mod tracy_plots;
pub(crate) use sl_viewer_ui_core::skin_palette;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_widgets::ui_checkbox;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_world_scene::transparency;
pub(crate) use sl_viewer_world_scene::water_clip;
#[cfg(test)]
mod ui_contract;
pub mod ui_elements;
pub(crate) use sl_viewer_notices::ui_name_link;
pub(crate) use sl_viewer_platform::ui_perf;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_spawn;
pub(crate) use sl_viewer_ui_sounds::ui_sounds;
pub(crate) use sl_viewer_ui_widgets::ui_radio;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_trackball;
#[cfg(test)]
mod ui_test;
pub(crate) use sl_viewer_audio::volume_panel;
pub(crate) use sl_viewer_audio::world_sounds;
pub(crate) use sl_viewer_map::world_map;
pub(crate) use sl_viewer_media::web_auth;
pub(crate) use sl_viewer_media::web_floater;
pub(crate) use sl_viewer_pickers::ui_texture_picker;
pub(crate) use sl_viewer_ui_core::ui_text;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_scene::underwater_fog;
pub(crate) use sl_viewer_world_scene::viewer_camera;
pub(crate) use sl_viewer_world_scene::water;
pub(crate) use sl_viewer_world_scene::water_exclusion;
pub(crate) use sl_viewer_world_scene::water_scene_depth;

use std::num::NonZero;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use clap::Parser as _;
use sl_client_bevy::{LoginParams, LoginRequest, StartLocation, Uuid};
use sl_repl::{Avatar, Credentials};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::assembly::{
    CaptureStartup, MediaRuntime, ViewerAppBuilder, ViewerAppOptions, WindowMode,
};
use crate::camera::{CameraSpin, CameraStart, SpinAxis};

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
    /// The Bevy app asked to exit with a failing status — a plugin that could
    /// not build, a renderer thread that panicked, a system that requested a
    /// failing exit. The code the app chose is carried so a log names it, but
    /// the process exits `1` either way: this is a `main` returning `Err`, not
    /// a `std::process::exit`, which would skip the tracing guards' flush and
    /// lose the very log that explains the failure.
    #[error("the viewer exited with a failing status ({0})")]
    AppFailed(NonZero<u8>),
    /// The `--screenshot-dir` could not be created, so the run has nowhere to
    /// put the frames it was started to take.
    #[error("could not create the screenshot directory")]
    ScreenshotDir(#[source] std::io::Error),
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
    /// (`avatar_skeleton.xml`, `avatar_lad.xml`, the base-body `.llm` meshes).
    /// Defaults to the vendored `viewer-assets/character/` beside the
    /// workspace when present; point this at an installed Firestorm / Second
    /// Life viewer to use different assets. Without any, avatars stay
    /// placeholder spheres.
    #[clap(long, env = "SL_VIEWER_ASSETS")]
    viewer_assets: Option<PathBuf>,
    /// Directory of viewer-shipped assets named `<uuid>.<class>` — the
    /// built-in animations, library body parts, clothing and gestures a grid
    /// would otherwise have to serve. Every asset fetch consults these before
    /// its cache and before the network, so the viewer keeps working against a
    /// grid whose library is incomplete. Repeat the flag (or pass a
    /// comma-separated list) to layer several; a later directory wins.
    /// Defaults to the vendored `viewer-assets/static_assets/` and
    /// `viewer-assets/fs_static_assets/` beside the workspace when present;
    /// point this at an installed Firestorm's `app_settings/` copies instead if
    /// you want its versions.
    #[clap(long, env = "SL_VIEWER_STATIC_ASSETS", value_delimiter = ',')]
    static_assets: Vec<PathBuf>,
    /// Ship no static assets: every asset comes from the grid, as it would on a
    /// viewer without a library. The way to tell a grid-side asset problem from
    /// a vendored-copy one.
    #[clap(long, conflicts_with = "static_assets")]
    no_static_assets: bool,
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
    /// The pixel grid every `--screenshot-dir` frame is rendered at,
    /// `WIDTHxHEIGHT` (default `1920x1080`). A run never captures the window:
    /// the frame is rendered into an off-screen target of exactly this size,
    /// because a window's size is a request no window manager promises to honour
    /// or to keep constant — and two frames of different sizes cannot be diffed.
    /// The environment variable is the one the Firestorm capture harness reads,
    /// so one env block sizes both viewers. Under `--headless` it is also the
    /// size of the off-screen window, which then *is* the captured frame.
    #[clap(long, env = "SL_VIEWER_CAPTURE_SIZE", value_parser = crate::screenshot::parse_capture_size)]
    capture_size: Option<crate::screenshot::CaptureSize>,
    /// Put the viewer's UI in the captured frames. Off by default, so a
    /// cross-viewer render comparison sees the world rather than two viewers'
    /// unrelated interfaces. Independent of `--capture-hud`: asking for the HUD
    /// alone hides the UI for the run (the two share one camera). The
    /// environment variable takes the Firestorm harness's falsey set — unset,
    /// empty, `0`, `false`, `no` and `off` are off, anything else on — so one
    /// env block means the same thing to both viewers.
    #[clap(
        long,
        env = "SL_VIEWER_CAPTURE_UI",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    capture_ui: bool,
    /// Put the HUD-attachment layer in the captured frames — the way to compare
    /// HUD rendering between the two viewers. Off by default. Independent of
    /// `--capture-ui`.
    #[clap(
        long,
        env = "SL_VIEWER_CAPTURE_HUD",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    capture_hud: bool,
    /// Put the edit-tool gizmo overlay (the move / rotate / scale handles and
    /// selection outlines) in the captured frames. Off by default; only visible
    /// at all when something is selected.
    #[clap(
        long,
        env = "SL_VIEWER_CAPTURE_GIZMOS",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    capture_gizmos: bool,
    /// Let a capture run make sound. Off by default: a run logs into a scene
    /// whose sound sources loop for as long as it lasts, and plays them through
    /// whatever speakers the machine has while nobody is listening. The master
    /// bus is silenced in the mixer only, so no stored volume or mute setting
    /// changes. Only means anything with `--screenshot-dir`.
    #[clap(
        long,
        env = "SL_VIEWER_CAPTURE_AUDIO",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    capture_audio: bool,
    /// Write the structured scene dump — what the viewer was showing when it
    /// took its frames — to this path instead of `<screenshot-dir>/scene.json`.
    /// The document is the one the patched Firestorm writes
    /// (`schema_version` 1), so the two can be diffed field by field to say
    /// *why* two frames differ: a prim in the wrong place, a texture that
    /// resolved to a different asset, a mesh stuck at a coarser LOD, a material
    /// that never arrived. Needs `--screenshot-dir`: the dump describes the
    /// scene a capture's last frame was taken from.
    #[clap(long, env = "SL_VIEWER_SCENE_DUMP", value_name = "PATH")]
    scene_dump: Option<PathBuf>,
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
    /// screenshot sequence. Only the flycam spins, so this needs a fixed
    /// `--camera-position`; the default third-person camera, which follows the
    /// avatar, ignores it.
    #[clap(long, allow_hyphen_values = true)]
    camera_spin: Option<f32>,
    /// Which camera axis `--camera-spin` rotates about (default `yaw`, a
    /// left/right pan).
    #[clap(long, value_enum, default_value_t = SpinAxis::Yaw)]
    camera_spin_axis: SpinAxis,
    /// The camera's vertical field of view in **degrees**, overriding the
    /// persisted `CameraAngle` preference for this run without rewriting it.
    ///
    /// Both viewers default to the reference's 60°, so a cross-check does not
    /// need this — but a comparison whose framing depends on two viewers'
    /// defaults agreeing is a comparison with an unstated premise, and this is
    /// how a run states it. The environment variable is the one the Firestorm
    /// harness reads, so one env block aims both lenses.
    #[clap(long, env = "SL_VIEWER_CAPTURE_FOV", value_name = "DEGREES")]
    camera_fov: Option<f32>,
    /// The scale the interface is drawn at, overriding the persisted `UiScale`
    /// preference for this run without rewriting it.
    ///
    /// A capture draws its interface into an off-screen image whose own scale
    /// factor is 1, so this is the scale a UI frame holds; the window's output
    /// scale plays no part in it. The environment variable is the one the
    /// Firestorm harness reads (as its `UIScaleFactor`), so one env block draws
    /// both interfaces at one scale. Refused outside the preference's own
    /// range, 0.75 to 2.
    #[clap(
        long,
        env = "SL_VIEWER_CAPTURE_UI_SCALE",
        value_name = "FACTOR",
        value_parser = parse_ui_scale
    )]
    capture_ui_scale: Option<f32>,
    /// The UI skin to wear — a directory under `assets/skins/` (`graphite`,
    /// `azure`, `vintage`). Skins change colour, texture, font and a widget's
    /// shape, never layout.
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
    /// Run with no OS window and no display: the viewer renders the world and
    /// its interface into an off-screen window of `--capture-size` (default
    /// 1920x1080) at a UI scale factor of 1, at a fixed 60 frames a second.
    /// It reads no mouse, keyboard, gamepad or 3D mouse, never touches the
    /// desktop's clipboard and opens no audio device: only the automation
    /// tier's synthetic input moves it. With `--screenshot-dir`, the frames are
    /// that window.
    #[clap(long)]
    headless: bool,
    /// With `--headless`, also open a window showing the run, for a person to
    /// follow. It takes no input — moving the mouse or typing over it changes
    /// nothing — and closing it ends the run with a graceful logout.
    #[clap(long, requires = "headless")]
    watch: bool,
}

/// Parse a `--capture-ui-scale` argument: a factor within the `UiScale`
/// preference's own range. Refused rather than clamped outside it, because a
/// clamped pin would draw one viewer's interface at a scale the run did not
/// ask for while the other drew the one it did.
fn parse_ui_scale(value: &str) -> Result<f32, String> {
    use crate::preferences_general::{UI_SCALE_MAX, UI_SCALE_MIN};
    let factor: f32 = value
        .trim()
        .parse()
        .map_err(|error| format!("expected a number, got {value:?}: {error}"))?;
    if !(UI_SCALE_MIN..=UI_SCALE_MAX).contains(&factor) {
        return Err(format!(
            "a UI scale of {factor} is outside {UI_SCALE_MIN} to {UI_SCALE_MAX}"
        ));
    }
    Ok(factor)
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

/// The vendored static-asset directories (`viewer-assets/static_assets/` and
/// `viewer-assets/fs_static_assets/` at the workspace root, see the
/// `viewer-assets` README for provenance), when this build still sits beside
/// its sources — the default for `--static-assets` /
/// `SL_VIEWER_STATIC_ASSETS`.
///
/// Firestorm's own two directories, in its order: the Linden library first, the
/// Firestorm additions second, so an id in both resolves to Firestorm's.
fn default_static_assets() -> Vec<PathBuf> {
    let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        return Vec::new();
    };
    [
        "viewer-assets/static_assets",
        "viewer-assets/fs_static_assets",
    ]
    .into_iter()
    .map(|relative| root.join(relative))
    .filter(|dir| dir.is_dir())
    .collect()
}

/// Installs the process-wide static-asset library every later
/// [`AssetStore`](sl_asset::AssetStore) consults, and logs what it holds.
///
/// Called once, before any store is built, because a store snapshots the
/// library at construction — the reference viewer seeds its cache at the same
/// point in start-up, and for the same reason.
fn install_static_assets(options: &Options) {
    if options.no_static_assets {
        tracing::info!("--no-static-assets: every asset comes from the grid");
        return;
    }
    let library = sl_asset::StaticAssetLibrary::load(&options.static_assets);
    let count = library.len();
    if sl_asset::static_assets::install(library) {
        tracing::info!(
            "shipping {count} static asset(s) from {:?}",
            options.static_assets
        );
    }
}

/// What this run's captured frames hold, from the `--capture-*` options: the
/// pixel grid, and each layer of the composited frame independently.
fn capture_content(options: &Options) -> crate::screenshot::CaptureContent {
    crate::screenshot::CaptureContent {
        size: options
            .capture_size
            .unwrap_or(crate::screenshot::CaptureSize::DEFAULT),
        ui: options.capture_ui,
        hud: options.capture_hud,
        gizmos: options.capture_gizmos,
    }
}

/// The builder options every command-line run shares: the interactive
/// viewer's, with what the command line says about the capture, the camera's
/// spin and lens, the avatar art and animations, and the skin watch.
fn cli_app_options(options: &Options, params: LoginParams) -> ViewerAppOptions {
    let mut app_options = ViewerAppOptions::new(params);
    app_options
        .content
        .viewer_assets
        .clone_from(&options.viewer_assets);
    app_options
        .content
        .play_animation
        .clone_from(&options.play_animation);
    app_options.content.repeat_animation = options.repeat_animation;
    app_options.capture = CaptureStartup {
        dir: options.screenshot_dir.clone(),
        content: capture_content(options),
        scene_dump: options.scene_dump.clone(),
        audio: options.capture_audio,
        ui_scale: options.capture_ui_scale,
    };
    app_options.camera.spin = CameraSpin {
        rate: options.camera_spin.unwrap_or(0.0).to_radians(),
        axis: options.camera_spin_axis,
    };
    app_options.camera.field_of_view = options.camera_fov.map(f32::to_radians);
    app_options.skin.watch = options.watch_skins;
    if options.headless {
        // The off-screen window is the capture size, so a headless capture's
        // frames are the window itself.
        let size = app_options.capture.content.size;
        app_options.window = WindowMode::Headless {
            size: UVec2::new(size.width, size.height),
            watch: options.watch,
        };
        // An unattended run plays nothing through the machine's speakers,
        // watched or not — the same as the test harness.
        app_options.audio_device = false;
    }
    app_options
}

/// The fixed camera pose `--camera-position` / `--camera-look-at` ask for,
/// aimed at the look-at point (the direction from the camera to the target), or
/// `None` without a fixed position — a look-at alone is ignored.
fn fixed_camera_start(options: &Options) -> Option<CameraStart> {
    let position = options.camera_position?;
    Some(CameraStart {
        position: Some(position),
        look: options.camera_look_at.map(|target| {
            Vec3::new(
                target.x - position.x,
                target.y - position.y,
                target.z - position.z,
            )
        }),
    })
}

/// Run the viewer end-to-end, restarting the windowed app once per MFA
/// challenge with the acquired token folded in.
///
/// # Errors
///
/// Returns an [`enum@Error`] if credentials cannot be loaded, the login URI
/// cannot be resolved, an MFA challenge cannot be answered, or a session fails
/// ([`Error::AppFailed`] / [`Error::ScreenshotDir`]) — a failing session ends
/// the retry loop rather than being retried.
fn run_viewer(options: &Options) -> Result<(), Error> {
    // Before anything else, so a signal that arrives during login is still a
    // graceful logout: the flag is only read once the app is running, but the
    // handler must already be installed when the signal lands. A failure to
    // install one is logged and carried on from — the viewer still runs, it just
    // dies abruptly when signalled, as it always did.
    if let Err(error) = crate::session::install_termination_handler() {
        warn!("could not install the SIGTERM/SIGINT handler: {error}");
    }
    let credentials = Credentials::load(&options.credentials)?;
    let avatar = credentials.select(options.avatar.as_deref())?;
    let login_uri = resolve_login_uri(options, avatar)?;

    // The persisted start-location preference (the preferences General tab) is
    // read from a throwaway store load: the Bevy app — and with it the
    // `ViewerSettings` resource — does not exist yet at login-request time.
    let (start, stored_skin, stored_theme) = {
        let settings = crate::settings::ViewerSettings::load_with(
            crate::paths::global_settings_file(),
            crate::REGISTRARS,
        );
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
        let mut app_options = cli_app_options(options, params);
        app_options.content.fetch_server_chat_history = !options.no_group_chat_history;
        app_options.camera.start = fixed_camera_start(options).unwrap_or_default();
        app_options.skin.selection = crate::skin::SkinSelection::resolve(
            options.skin.clone(),
            options.theme.clone(),
            stored_skin.clone(),
            stored_theme.clone(),
        );
        app_options.media = MediaRuntime {
            web: !options.disable_web_media,
            video: !options.disable_video_media,
            web_auth: !options.no_web_auth,
        };
        let outcome = ViewerAppBuilder::from_options(app_options).build()?.run()?;
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
/// Returns [`Error::Replay`] if the bundle is missing, empty, or unreadable,
/// or [`Error::AppFailed`] / [`Error::ScreenshotDir`] if the session itself
/// fails — which is the whole point of a replay run, since it is normally
/// driven unattended by a harness reading the exit status.
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
    let camera_start = fixed_camera_start(options).unwrap_or_else(|| replay_camera_start(&config));

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
    let mut app_options = cli_app_options(options, params);
    // Offline there is no session thread, so the flag is inert; false keeps the
    // no-network intent explicit.
    app_options.content.fetch_server_chat_history = false;
    app_options.content.replay = Some(config);
    app_options.camera.start = camera_start;
    app_options.skin.selection = {
        // The persisted skin choice dresses the replay UI too; the throwaway
        // pre-app load is the `run_viewer` idiom.
        let settings = crate::settings::ViewerSettings::load_with(
            crate::paths::global_settings_file(),
            crate::REGISTRARS,
        );
        let (stored_skin, stored_theme) =
            crate::preferences_colors_skins::stored_skin_choice(&settings);
        crate::skin::SkinSelection::resolve(
            options.skin.clone(),
            options.theme.clone(),
            stored_skin,
            stored_theme,
        )
    };
    // No network surfaces offline: keep the media engines and web auth off.
    app_options.media = MediaRuntime::OFF;
    let _outcome = ViewerAppBuilder::from_options(app_options).build()?.run()?;
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
/// The viewer disables Bevy's own `LogPlugin` (see [`assembly`]) because the
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
/// shells can build against too.
///
/// # Errors
///
/// Returns [`enum@Error`] if the credentials, grid or login URI cannot be
/// resolved, or if the run itself fails — the process then exits non-zero, so
/// a script or harness that checks the status sees the failure.
pub fn run() -> Result<(), Error> {
    // Held for the whole process so the Chrome profiler (if enabled) flushes.
    let _tracing_guards = init_tracing();
    let mut options = Options::parse();
    // An explicit `--viewer-assets` / `SL_VIEWER_ASSETS` wins; otherwise the
    // vendored character directory serves the real Linden bodies by default.
    options.viewer_assets = options
        .viewer_assets
        .take()
        .or_else(crate::assembly::default_viewer_assets);
    // Likewise for the shipped assets, and installed here — before anything
    // builds an asset store, which snapshots the library as it is.
    if options.static_assets.is_empty() {
        options.static_assets = default_static_assets();
    }
    install_static_assets(&options);
    // `--replay <dir>` renders a captured bundle offline; otherwise a normal login.
    if let Some(bundle_dir) = options.replay.clone() {
        return run_replay(&options, &bundle_dir);
    }
    run_viewer(&options)
}

#[cfg(test)]
mod ui_scale_option_tests {
    use super::parse_ui_scale;

    /// Whether `value` parses to `want`.
    fn parses_to(value: &str, want: f32) -> bool {
        parse_ui_scale(value).is_ok_and(|factor| (factor - want).abs() < f32::EPSILON)
    }

    /// A pin inside the preference's range is taken as written, the ends
    /// included.
    #[test]
    fn a_ui_scale_in_range_is_taken() {
        assert!(parses_to("1", 1.0));
        assert!(parses_to("0.75", 0.75));
        assert!(parses_to(" 2 ", 2.0));
    }

    /// Outside the range, or not a number, is refused rather than clamped: a
    /// clamped pin draws one viewer at a scale the run did not ask for.
    #[test]
    fn a_ui_scale_out_of_range_or_malformed_is_refused() {
        let refused = |value: &str, because: &str| {
            parse_ui_scale(value).is_err_and(|error| error.contains(because))
        };
        assert!(refused("0.5", "outside"));
        assert!(refused("2.5", "outside"));
        assert!(refused("NaN", "outside"));
        assert!(refused("big", "expected a number"));
    }
}
