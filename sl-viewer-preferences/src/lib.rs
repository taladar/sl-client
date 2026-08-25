//! The viewer's preferences.
//!
//! The tabbed floater over every declared setting ([`preferences`] and its
//! per-tab modules), the quick-preferences popover at the bottom edge
//! ([`quick_preferences`]), and the raw debug-settings editor
//! ([`debug_settings`]) for the ones no tab shows.
//!
//! This crate sits at the top of the feature tier and depends on nearly all of
//! it, which is the point: a tab draws a control for a setting whose behaviour
//! lives elsewhere, so the dependency runs from the tab to the behaviour and
//! never the other way. The setting *keys* live with the behaviour -- many of
//! them in `sl-viewer-world-api` -- precisely so that stays true.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `preferences::PreferencesUi` and `debug_settings::DebugSettingsUi`. \
              That only became a lint when these items turned `pub` for the crate \
              split; renaming them would churn every call site in the viewer to \
              satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::settings`.
pub(crate) use sl_viewer_audio::audio;
pub(crate) use sl_viewer_audio::parcel_audio;
pub(crate) use sl_viewer_audio::volume_panel;
pub(crate) use sl_viewer_audio::world_sounds;
pub(crate) use sl_viewer_kit::minimap_math;
pub(crate) use sl_viewer_kit::sky_presets;
pub(crate) use sl_viewer_map::minimap;
pub(crate) use sl_viewer_map::world_map;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_people::auto_reject;
pub(crate) use sl_viewer_people::group_notice;
pub(crate) use sl_viewer_people::offers_invites;
pub(crate) use sl_viewer_people::people;
pub(crate) use sl_viewer_people::presence;
pub(crate) use sl_viewer_people::radar;
pub(crate) use sl_viewer_platform::clipboard;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::skin;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::avatar_complexity;
pub(crate) use sl_viewer_world_objects::avatars;
pub(crate) use sl_viewer_world_objects::derender;
pub(crate) use sl_viewer_world_objects::hover_text;
pub(crate) use sl_viewer_world_objects::name_tag_billboard;
pub(crate) use sl_viewer_world_objects::name_tag_content;
pub(crate) use sl_viewer_world_objects::render_priority;
pub(crate) use sl_viewer_world_scene::environment;
pub(crate) use sl_viewer_world_scene::exposure;
pub(crate) use sl_viewer_world_scene::glow;
pub(crate) use sl_viewer_world_scene::parcel_borders;
pub(crate) use sl_viewer_world_scene::particles;
pub(crate) use sl_viewer_world_scene::probes;
pub(crate) use sl_viewer_world_scene::sky;
pub(crate) use sl_viewer_world_scene::tonemap;
pub(crate) use sl_viewer_world_view::camera;
pub(crate) use sl_viewer_world_view::media_prim;
pub(crate) use sl_viewer_world_view::movement;
pub(crate) use sl_viewer_world_view::session;

pub mod debug_settings;
pub mod preferences;
pub mod preferences_alerts;
pub mod preferences_audio;
pub mod preferences_camera_move;
pub mod preferences_chat;
pub mod preferences_colors_skins;
pub mod preferences_general;
pub mod preferences_graphics;
pub mod preferences_network_cache;
pub mod quick_preferences;
