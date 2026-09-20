//! The viewer's preferences.
//!
//! The tabbed floater over every declared setting ([`preferences`] and its
//! per-tab modules), the quick-preferences popover at the bottom edge
//! ([`quick_preferences`]), the photographer's curated view over the render
//! knobs and the environment ([`phototools`]), and the raw debug-settings
//! editor ([`debug_settings`]) for the ones no tab shows.
//!
//! This crate sits at the top of the feature tier, which is the point: a tab
//! draws a control for a setting whose behaviour lives elsewhere, so the
//! dependency runs from the tab to the behaviour and never the other way.
//!
//! It does *not* follow that a tab needs a dependency on every crate it draws a
//! control for. A binding needs only the setting's **name**, and a name two
//! layers agree on belongs beneath both -- in `sl-viewer-world-api` for the
//! world settings, and in [`sl_viewer_settings::keys`] for the ones whose
//! behaviour lives in the feature tier alongside this crate. Where a tab needs
//! actual behaviour (the audio buses, the sky presets) the dependency is real
//! and it is here.

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
// The Personal Lighting window, which the Phototools environment tab is a door
// to. Only its floater id is named — but a floater id is the window's own,
// there is no layer below both that holds one, and this crate is where a
// surface that draws another's controls belongs.
pub(crate) use sl_viewer_environment::personal_lighting;
// The inventory mirror's settings-asset index: the sky / water / day-cycle
// lists the quick-preferences environment combos are a view of. A dependency on
// the inventory crate rather than a projection resource below it because the
// list *is* an inventory query — every settings item, grouped, name-ordered —
// and there is nowhere lower to hold one that would not be the same query
// written twice.
pub(crate) use sl_viewer_inventory::settings_index;
pub(crate) use sl_viewer_kit::minimap_math;
pub(crate) use sl_viewer_kit::sky_presets;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_platform::clipboard;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_settings as settings;
// The keys of settings whose behaviour lives elsewhere, re-aliased under their
// owning module's name so a tab still addresses `crate::radar::SETTING_*`. Each
// group is everything this crate ever named from that module, so the module's
// crate need not be a dependency here — see `sl_viewer_settings::keys`.
//
// `sl-viewer-people` and `sl-viewer-map` are not dependencies at all for this
// reason, and neither are `sl-viewer-world-objects` and
// `sl-viewer-world-avatar` (outside the tests, which call the owners'
// registrars — see the dev-dependencies). `sl-viewer-world-scene` remains one:
// `sky`, `environment`, `render_overrides` and `viewer_camera` are behaviour,
// not names. Its six key-only modules are here all the same, because where a
// name lives is a question about the name, not about what else the crate
// happens to owe that dependency.
pub(crate) use sl_viewer_settings::keys::{
    auto_reject, avatar_complexity, derender, exposure, glow, group_notice, hover_text, minimap,
    name_tag_billboard, name_tag_content, offers_invites, parcel_borders, particles, people,
    presence, probes, radar, render_priority, tonemap, world_map,
};
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::skin;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_spawn;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_slider;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_scene::environment;
pub(crate) use sl_viewer_world_scene::render_overrides;
pub(crate) use sl_viewer_world_scene::sky;
pub(crate) use sl_viewer_world_view::camera;
pub(crate) use sl_viewer_world_view::media_prim;
pub(crate) use sl_viewer_world_view::movement;
pub(crate) use sl_viewer_world_view::session;

pub mod debug_settings;
pub mod phototools;
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
pub mod quick_prefs_environment;
