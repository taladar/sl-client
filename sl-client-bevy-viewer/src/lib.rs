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
pub(crate) use sl_viewer_places::about_land;
pub(crate) use sl_viewer_places::about_landmark;
pub(crate) use sl_viewer_places::about_region;
pub(crate) use sl_viewer_world_avatar::animations;
pub(crate) use sl_viewer_world_avatar::animesh;
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
    crate::world_map::register_settings,
    crate::search::register_settings,
    crate::tonemap::register_settings,
    crate::glow::register_settings,
    crate::exposure::register_settings,
    crate::snapshot_floater::register_settings,
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
];

// The leaf toolkit (geometry math, render leaves, small models) is its own
// crate; each module is aliased under its old name so every
// `crate::<module>::…` path in the viewer still resolves.
pub(crate) use sl_viewer_kit::appearance;
mod asset_blacklist;
pub(crate) use sl_viewer_world_avatar::avatar_asset_stats;
pub(crate) use sl_viewer_world_objects::asset_budget;
pub(crate) use sl_viewer_world_objects::asset_stats;
// The platform layer (directory layout, on-disk caches, clipboard, URL
// linkification) is its own crate; each module is aliased under its old
// name so every `crate::<module>::…` path in the viewer still resolves.
mod attachment_menu;
pub(crate) use sl_viewer_audio::audio;
pub(crate) use sl_viewer_kit::avatar_assets;
pub(crate) use sl_viewer_people::auto_reject;
pub(crate) use sl_viewer_world_avatar::avatar_complexity;
pub(crate) use sl_viewer_world_avatar::avatar_dump;
mod avatar_menu;
pub(crate) use sl_viewer_people::avatar_profile;
pub(crate) use sl_viewer_pickers::avatar_picker;
mod avatar_render_floater;
pub(crate) use sl_viewer_people::blocked;
pub(crate) use sl_viewer_world_avatar::avatar_render_settings;
pub(crate) use sl_viewer_world_avatar::avatar_replay;
pub(crate) use sl_viewer_world_avatar::avatars;
pub(crate) use sl_viewer_world_avatar::bake_inputs;
pub(crate) use sl_viewer_world_avatar::bake_publish;
pub(crate) use sl_viewer_world_avatar::body_physics;
pub(crate) use sl_viewer_world_scene::beacons;
mod bottom_toolbar;
// Media (the CEF / GStreamer backends, the browser widget) is its own crate;
// each module is aliased under its old name so every `crate::<module>::…`
// path in the viewer still resolves.
pub(crate) use sl_viewer_media::browser_widget;
mod build_info;
pub(crate) use sl_viewer_chat::chat;
pub(crate) use sl_viewer_chat::chat_input;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_people::contact_sets;
pub(crate) use sl_viewer_people::contact_sets_panel;
pub(crate) use sl_viewer_people::conversations;
pub(crate) use sl_viewer_platform::clipboard;
pub(crate) use sl_viewer_world_objects::bump;
pub(crate) use sl_viewer_world_view::camera;
mod crowd_debug_button;
pub(crate) use sl_viewer_preferences::debug_settings;
pub(crate) use sl_viewer_world_avatar::derender;
pub(crate) use sl_viewer_world_scene::diagnostics;
mod double_click_teleport;
pub(crate) use sl_viewer_asset_editors::edit_notecard;
pub(crate) use sl_viewer_asset_editors::edit_script;
pub(crate) use sl_viewer_asset_editors::edit_wearable;
pub(crate) use sl_viewer_edit::edit_contents;
pub(crate) use sl_viewer_edit::edit_create;
pub(crate) use sl_viewer_edit::edit_link;
pub(crate) use sl_viewer_edit::edit_material;
pub(crate) use sl_viewer_edit::edit_material_asset;
pub(crate) use sl_viewer_edit::edit_params;
pub(crate) use sl_viewer_edit::edit_selection;
pub(crate) use sl_viewer_edit::edit_texture;
pub(crate) use sl_viewer_edit::edit_tool;
pub(crate) use sl_viewer_edit::edit_undo;
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
pub(crate) use sl_viewer_kit::flexi;
pub(crate) use sl_viewer_notices::experience_permission;
pub(crate) use sl_viewer_notices::experiences_floater;
pub(crate) use sl_viewer_platform::environment_assets;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_world_scene::environment;
pub(crate) use sl_viewer_world_scene::exposure;
pub mod gallery;
pub(crate) use sl_viewer_edit::gizmos;
pub(crate) use sl_viewer_kit::geometry_cache;
pub(crate) use sl_viewer_people::group_notice;
pub(crate) use sl_viewer_people::group_profile;
pub(crate) use sl_viewer_people::groups;
pub(crate) use sl_viewer_world_avatar::gpu_avatar_spike;
pub(crate) use sl_viewer_world_avatar::gpu_avatars;
pub(crate) use sl_viewer_world_avatar::ground;
pub(crate) use sl_viewer_world_avatar::hand_pose;
pub(crate) use sl_viewer_world_objects::hover_text;
pub(crate) use sl_viewer_world_scene::glow;
pub(crate) use sl_viewer_world_view::gpu_pick;
mod hover_tooltip;
pub(crate) use sl_viewer_world_view::hud;
pub(crate) use sl_viewer_world_view::hud_pick;
// The UI vocabulary (scaffold, fonts, skin, Fluent) is its own crate; each
// module is aliased under its old name so every `crate::<module>::…` path in
// the viewer still resolves.
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_world_view::input_action;
pub(crate) use sl_viewer_world_view::input_context;
mod inspector_popup;
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_inventory::inventory_actions;
pub(crate) use sl_viewer_inventory::inventory_drag;
pub(crate) use sl_viewer_inventory::inventory_filters;
pub(crate) use sl_viewer_inventory::inventory_gallery;
pub(crate) use sl_viewer_inventory::inventory_properties;
mod land_menu;
pub(crate) use sl_viewer_notices::linkified_text;
pub(crate) use sl_viewer_world_objects::legacy_materials;
pub(crate) use sl_viewer_world_scene::lights;
mod load_url;
pub(crate) use sl_viewer_chat::local_chat_input;
pub(crate) use sl_viewer_world_avatar::locomotion_ik;
pub(crate) use sl_viewer_world_avatar::look_at;
pub(crate) use sl_viewer_world_objects::material_cache;
pub(crate) use sl_viewer_world_objects::material_preview;
pub(crate) use sl_viewer_world_objects::materials;
mod media_controls;
pub(crate) use sl_viewer_media::media_diagnostics;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_world_view::media_prim;
mod menu_bar;
mod menu_search;
pub(crate) use sl_viewer_asset_editors::notecard_render;
pub(crate) use sl_viewer_chat::nearby_chat_bar;
pub(crate) use sl_viewer_map::minimap;
pub(crate) use sl_viewer_notices::notification_host;
pub(crate) use sl_viewer_notices::notification_persist;
pub(crate) use sl_viewer_people::mutes;
pub(crate) use sl_viewer_world_avatar::name_tag_content;
pub(crate) use sl_viewer_world_objects::meshes;
pub(crate) use sl_viewer_world_objects::name_tag_billboard;
pub(crate) use sl_viewer_world_view::movement;
// The notification catalogue is its own crate (~22k lines of declarative data
// with no dependency on anything else here), aliased under its old module name
// so every `crate::notifications::…` path in the viewer still resolves.
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_world_objects::object_cost;
mod object_menu;
pub(crate) use sl_viewer_audio::parcel_audio;
pub(crate) use sl_viewer_kit::parcel_names;
pub(crate) use sl_viewer_kit::particle_render;
pub(crate) use sl_viewer_kit::raycast_index;
pub(crate) use sl_viewer_people::offers_invites;
pub(crate) use sl_viewer_people::people;
pub(crate) use sl_viewer_people::presence;
pub(crate) use sl_viewer_people::radar;
pub(crate) use sl_viewer_platform::paths;
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
pub(crate) use sl_viewer_ui_pie_menu::pie_menu;
pub(crate) use sl_viewer_world_avatar::reach;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_scene::parcel_borders;
pub(crate) use sl_viewer_world_scene::particles;
pub(crate) use sl_viewer_world_scene::probes;
pub(crate) use sl_viewer_world_view::physics;
pub mod render_gallery;
pub(crate) use sl_viewer_world_objects::render_priority;
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
pub(crate) use sl_viewer_world_scene::render_scene;
#[cfg(test)]
mod render_test;
pub(crate) use sl_viewer_notices::script_dialog;
pub(crate) use sl_viewer_notices::script_permission;
pub(crate) use sl_viewer_search::search;
pub(crate) use sl_viewer_world_avatar::replay_bundle;
pub(crate) use sl_viewer_world_avatar::rigged_attachments;
pub(crate) use sl_viewer_world_view::screenshot;
pub(crate) use sl_viewer_world_view::session;
// The settings store is its own crate now that it no longer names the
// features that register with it — that list is `REGISTRARS` above.
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
#[cfg(test)]
mod settings_golden;
pub(crate) use sl_viewer_kit::shadow_visibility;
pub(crate) use sl_viewer_kit::sit_offset;
pub(crate) use sl_viewer_kit::sky_presets;
pub(crate) use sl_viewer_kit::slt;
pub(crate) use sl_viewer_ui_core::skin;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_world_scene::sky;
pub(crate) use sl_viewer_world_view::sit_camera;
mod slurl_dispatch;
mod snapshot_floater;
pub(crate) use sl_viewer_platform::sound_cache;
pub(crate) use sl_viewer_platform::system_browser;
pub(crate) use sl_viewer_spacenav as spacenav;
mod stand_stop_button;
mod status_bar;
mod teleport_progress;
pub(crate) use sl_viewer_world_objects::texture_anim;
pub(crate) use sl_viewer_world_objects::textures;
pub(crate) use sl_viewer_world_scene::terrain;
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
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_world_avatar::typing;
pub(crate) use sl_viewer_world_scene::transparency;
pub(crate) use sl_viewer_world_scene::water_clip;
mod ui_elements;
pub(crate) use sl_viewer_notices::ui_name_link;
pub(crate) use sl_viewer_platform::ui_perf;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_sounds;
pub(crate) use sl_viewer_ui_widgets::ui_radio;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
#[cfg(test)]
mod ui_test;
pub(crate) use sl_viewer_audio::volume_panel;
pub(crate) use sl_viewer_media::web_auth;
pub(crate) use sl_viewer_pickers::ui_texture_picker;
pub(crate) use sl_viewer_platform::url_linkify;
pub(crate) use sl_viewer_ui_core::ui_text;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_scene::underwater_fog;
pub(crate) use sl_viewer_world_scene::viewer_camera;
pub(crate) use sl_viewer_world_scene::water;
pub(crate) use sl_viewer_world_scene::water_exclusion;
mod web_floater;
pub(crate) use sl_viewer_audio::world_sounds;
pub(crate) use sl_viewer_map::world_map;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use clap::Parser as _;
use sl_client_bevy::{
    AccountDirsConfig, AnimationKey, ChatLogConfig, ClientDirectories, InventoryCacheConfig,
    LoggedChatType, LoginFailure, LoginParams, LoginRequest, MfaChallenge, SlClientPlugin,
    SlLoginRejected, SlMfaChallenge, StartLocation, Uuid,
};
use sl_repl::{Avatar, Credentials};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::about_floater::AboutFloaterPlugin;
use crate::about_land::AboutLandPlugin;
use crate::about_landmark::AboutLandmarkPlugin;
use crate::about_region::AboutRegionPlugin;
use crate::animations::AnimationManager;
use crate::asset_blacklist::AssetBlacklistPlugin;
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatar_picker::AvatarPickerPlugin;
use crate::avatar_profile::AvatarProfilePlugin;
use crate::blocked::BlockedPlugin;
use crate::camera::{CameraSpin, CameraStart, SpinAxis};
use crate::chat::{
    ChatOverlay, position_chat_overlay, restyle_chat_overlay, setup_chat_overlay,
    tick_chat_overlay, update_chat_overlay,
};
use crate::chat_input::ChatInputPlugin;
use crate::conversations::ConversationsPlugin;
use crate::derender::DerenderPlugin;
use crate::emoji_complete::ColonCompletePlugin;
use crate::emoji_picker::EmojiPickerPlugin;
use crate::experience_permission::ExperiencePermissionPlugin;
use crate::experiences_floater::ExperiencesPlugin;
use crate::floater::FloaterPlugin;
use crate::floater_persist::FloaterPersistPlugin;
use crate::group_notice::GroupNoticePlugin;
use crate::group_profile::GroupProfilePlugin;
use crate::groups::GroupsPlugin;
use crate::i18n::ViewerI18nPlugin;
use crate::input_context::{CursorGrabAllowed, world_has_keyboard};
use crate::inventory::InventoryPlugin;
use crate::inventory_actions::InventoryActionsPlugin;
use crate::inventory_drag::InventoryDragPlugin;
use crate::inventory_filters::InventoryFiltersPlugin;
use crate::inventory_gallery::InventoryGalleryPlugin;
use crate::inventory_properties::InventoryPropertiesPlugin;
use crate::load_url::LoadUrlPlugin;
use crate::local_chat_input::LocalChatInputPlugin;
use crate::nearby_chat_bar::NearbyChatBarPlugin;
use crate::notification_host::{
    NotificationHostPlugin, announce_command_failures, apply_diagnostics_setting,
    ingest_alert_messages, ingest_protocol_diagnostics, spawn_notification_demo,
};
use crate::notification_persist::NotificationPersistPlugin;
use crate::offers_invites::OffersInvitesPlugin;
use crate::people::PeoplePlugin;
use crate::script_dialog::ScriptDialogPlugin;
use crate::script_permission::ScriptPermissionPlugin;
use crate::session::{
    PlayOnLogin, ViewerSession, apply_draw_distance, drive_session, enforce_quit_deadline,
    handle_quit_input, handle_quit_requests, repeat_debug_animation, report_agent_viewport,
    report_camera_interest, save_settings_on_logout,
};
use crate::settings::{AccountContext, ViewerSettings, load_account_settings};
use crate::settings_binding::SettingsBindingPlugin;
use crate::stand_stop_button::StandStopButtonPlugin;
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
use crate::viewer_camera::viewer_camera_bundle;
use crate::viewer_plugins::{
    ViewerEditPlugins, ViewerInputPlugins, ViewerRenderPlugins, ViewerWorldPlugins,
};
use crate::virtual_list::VirtualListPlugin;
use crate::world_api::{CameraMode, CameraRig};

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
    /// (`avatar_skeleton.xml`, `avatar_lad.xml`, the base-body `.llm` meshes).
    /// Defaults to the vendored `viewer-assets/character/` beside the
    /// workspace when present; point this at an installed Firestorm / Second
    /// Life viewer to use different assets. Without any, avatars stay
    /// placeholder spheres.
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
/// (the sun / moon) is spawned by `sky::setup_sky`, which also drives it
/// from the region's environment.
///
/// The camera starts in third-person, which follows the avatar as soon as it
/// arrives (`camera::position_camera`), so no login camera-snap is needed. A fixed
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

/// The vendored character directory (`viewer-assets/character/` at the
/// workspace root, see its README for provenance), when this build still sits
/// beside its sources — the default for `--viewer-assets` /
/// `SL_VIEWER_ASSETS`, so avatars get the real Linden bodies out of the box
/// while an explicit flag or environment variable still overrides.
fn default_viewer_assets() -> Option<PathBuf> {
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("viewer-assets/character");
    vendored.is_dir().then_some(vendored)
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
    // The render debug knobs (`SL_VIEWER_DISABLE_GLOW`, `SL_VIEWER_SKY_DAY_POSITION`,
    // …), read from the environment exactly once and only here — while the process
    // is still single-threaded — and the environment state their day-position pin
    // seeds. Every consumer reads the resource; a headless rig inserts its own.
    let render_overrides = crate::render_overrides::RenderOverrides::from_env();
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
    // The four plugin groups the headless harnesses share with the viewer
    // (`crate::viewer_plugins`): input, then the render stack (whose
    // `SlFaceMaterialPlugin` the editor plugins' `FromWorld` resources build
    // against), the world fold, and the build tools.
    .add_plugins(ViewerInputPlugins::default())
    .add_plugins(ViewerRenderPlugins::default())
    .add_plugins(ViewerWorldPlugins::default())
    .add_plugins(ViewerEditPlugins)
    // The Stand Up / Stop flycam state button in the bottom toolbar's reserved
    // slot (viewer-sit-target-and-stand-button): Stand while seated, Stop flycam
    // while in flycam.
    .add_plugins(StandStopButtonPlugin)
    // The Spawn crowd debug button (SL_VIEWER_CROWD): only present while a
    // synthetic crowd is armed, hands the user the manual capture trigger.
    .add_plugins(crate::crowd_debug_button::CrowdDebugButtonPlugin)
    .add_plugins(crate::teleport_progress::TeleportProgressPlugin)
    .add_plugins(crate::double_click_teleport::DoubleClickTeleportPlugin)
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
    // Surface a queued command whose send failed (no circuit, a stale scoped id,
    // an encode error), so an action that never reached the simulator says so
    // instead of looking as if it worked.
    .add_systems(Update, announce_command_failures)
    // Drain the protocol diagnostics the session collects — decode failures,
    // unhandled messages, unknown capability events, missing replies — into the
    // log, and push the developer switch that turns their collection on or off.
    .add_systems(
        Update,
        (ingest_protocol_diagnostics, apply_diagnostics_setting),
    )
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
        .insert_resource(ViewerSettings::load_with(REGISTRARS))
        // The debug camera override (`--camera-position` / `--camera-look-at` /
        // `--camera-spin`): `setup_scene` reads the start pose, `drive_flycam` reads
        // the spin, and third-person auto-follows when no pose is fixed. The world
        // context may grab the cursor (only in mouselook) unless this is an unattended
        // screenshot run, whose whole point is to leave the desktop's pointer alone.
        .insert_resource(CursorGrabAllowed(screenshot_dir.is_none()))
        .insert_resource(camera_start)
        .insert_resource(camera_spin)
        .init_resource::<LoginOutcome>()
        // The live A/B state of the shape's collision-volume displacement (P34.3), seeded
        // from `SL_VIEWER_VOLUME_MORPH_GAIN` and toggled by the `V` key.
        // One shared per-frame mesh-upload lane spent by object spawn / geometry /
        // LOD / terrain apply (replaces their old independent budgets).
        // The deferred geometry builds of the objects `ObjectState` tracks, kept
        // beside it rather than inside a tracked object: an in-flight asset fetch
        // or a retained LOD rebuild is machinery, not world state.
        // The screen-space HUD hierarchy (P35.1), spawned by `setup_hud_screen`.
        // The water-render bookkeeping (P23.1) is created by `setup_water` at
        // startup, so no `init_resource` is needed here; the surface level the
        // underwater-fog pass reads is a small resource published by `drive_water`.
        // The cross-instance geometry cache: shared mesh handles for identical
        // prim / sculpt / mesh geometry (`viewer-perf-prim-tessellation-cache`).
        // The cross-instance material cache: shared face-material handles for
        // identical face content, so matched copies batch into instanced draws
        // (`viewer-perf-material-intern`).
        .init_resource::<ChatOverlay>()
        .insert_resource(AnimationManager::new(viewer_assets.map(Path::to_path_buf)))
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
                // The chat overlay now parents itself under the scaffold's
                // `UiRoot` (so the snapshot include-UI-off hide covers it), and so
                // must see the root.
                setup_chat_overlay.after(UiScaffoldSystems::SpawnRoot),
                // The UI text & font foundation demo panel (viewer-ui-text-foundation),
                // which parents itself to the scaffold's `UiRoot` and so must see it.
                setup_text_demo.after(UiScaffoldSystems::SpawnRoot),
                // The reusable text-input widget demo panel (viewer-ui-text-input-widget),
                // likewise parented to the scaffold's `UiRoot`.
                setup_text_input_demo.after(UiScaffoldSystems::SpawnRoot),
            ),
        )
        // The material cache's copy-on-write detach net: give any interned
        // (shared-material) face a private material before this frame's
        // `Update` mutators — texture animation, PBR registration, HUD
        // fullbright, the edit floaters' live previews — can write into the
        // shared asset. Scheduled in `PreUpdate` so the swap's commands are
        // applied at the schedule boundary, ahead of every mutator.
        // Refill the shared per-frame asset-upload budgets in `PreUpdate`, ahead of
        // every `Update` apply system that spends from them — the image lane
        // (`TextureApplyBudget`, drawn by the texture / PBR-map / bump / legacy / bake
        // systems) and the mesh lane (`MeshUploadBudget`, drawn by object spawn /
        // geometry / LOD / terrain). Resetting here rather than inside the scattered
        // Update tuples guarantees the refill precedes all consumers regardless of
        // their relative order.
        .add_systems(
            Update,
            (
                capture_login_outcome,
                drive_session,
                // Announce the (user-tunable) draw distance on handshake and
                // whenever the quick-preferences slider moves it.
                apply_draw_distance,
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
            ),
        )
        // UI text & font foundation and the text-input widget demo panels.
        .add_systems(
            Update,
            (
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
            ),
        )
        // Terrain lighting (viewer-clouds-sun-occlusion): drive each region's ground
        // with the sky frame's atmospheric sun / ambient colours, like the reference
        // legacy terrain, after the camera so it reads the current altitude's sky
        // frame. The sky, water, water-exclusion and underwater-fog stacks schedule
        // themselves — see `SkyPlugin` and its siblings.
        // The EEP settings-asset fetch cap for the World ▸ Environment Modern
        // presets, and the session's camera-interest / viewport reports. The avatar
        // animation pipeline that used to share this call schedules itself — see
        // `AvatarAnimationPlugin`.
        .add_systems(
            Update,
            (
                // The interest camera is the viewpoint the simulator builds the
                // agent's object stream around, so it must be *this* frame's pose:
                // after the camera, or every report describes where the camera was a
                // frame ago (a whole report interval at its ~45 Hz cadence).
                report_camera_interest.after(world_api::WorldPhase::CameraPositioned),
                report_agent_viewport,
            ),
        );
    // (Worn rigid attachments no longer need a hand re-propagation: their
    // attachment-point node is an avatar-root child whose local `Transform` the
    // pose driver's socket writer sets each frame, so ordinary change-gated
    // propagation seats the worn subtree — the former `pose_attachment_nodes`
    // pass, Phase 4 §5.4.)
    // Load the client-side avatar assets (if a directory was given) so rigged
    // bodies replace the placeholder spheres; absent them the viewer keeps spheres.
    if let Some(library) = load_avatar_library(viewer_assets) {
        app.insert_resource(library);
    }
    // Env-gated debug / demo systems, registered only when their switch is set
    // (the `capture_screenshots` pattern) — a normal session pays no scheduler
    // dispatch for them at all. Each predicate mirrors the system's own
    // internal env check.
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
        app.insert_resource(config)
            .add_plugins(crate::avatar_replay::AvatarReplayPlugin);
    }
    // In screenshot mode, capture a numbered PNG sequence of the window after a
    // startup delay, then quit (the R11 offline-inspection harness).
    if let Some(dir) = screenshot_dir {
        if let Err(error) = fs_err::create_dir_all(dir) {
            warn!("failed to create screenshot dir {}: {error}", dir.display());
        }
        app.add_plugins(crate::screenshot::ScreenshotPlugin {
            dir: dir.to_path_buf(),
        });
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
        let settings = crate::settings::ViewerSettings::load_with(crate::REGISTRARS);
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
                let settings = crate::settings::ViewerSettings::load_with(crate::REGISTRARS);
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
    let mut options = Options::parse();
    // An explicit `--viewer-assets` / `SL_VIEWER_ASSETS` wins; otherwise the
    // vendored character directory serves the real Linden bodies by default.
    options.viewer_assets = options.viewer_assets.take().or_else(default_viewer_assets);
    // `--replay <dir>` renders a captured bundle offline; otherwise a normal login.
    if let Some(bundle_dir) = options.replay.clone() {
        return run_replay(&options, &bundle_dir);
    }
    run_viewer(&options)
}
