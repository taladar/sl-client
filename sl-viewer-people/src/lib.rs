//! The viewer's people surfaces: who else is here, and what you can do about
//! it.
//!
//! [`conversations`] is the window the rest hang inside -- the tab strip whose
//! People pane [`people`] fills with the friends list, and into whose slots
//! [`groups`] and [`blocked`] spawn their own panes. Around them:
//! [`radar`] for who is nearby, [`avatar_profile`] and [`group_profile`] for
//! who they are, [`contact_sets`] and [`contact_sets_panel`] for the user's
//! own grouping of them, [`mutes`] and [`auto_reject`] for who is refused,
//! [`offers_invites`] and [`group_notice`] for what they send, and
//! [`presence`] for how the agent appears back.
//!
//! They move together because they are literally one floater: the panes are
//! children of the conversation window's pane slots, so none of them can be
//! built without it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `contact_sets::ContactSets` and `group_notice::GroupNoticeToast`. \
              That only became a lint when these items turned `pub` for the crate \
              split; renaming them would churn every call site in the viewer to \
              satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::world_api`.
pub(crate) use sl_viewer_chat::chat_input;
pub(crate) use sl_viewer_chat::local_chat_input;
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_inventory::inventory_actions;
pub(crate) use sl_viewer_inventory::inventory_drag;
pub(crate) use sl_viewer_inventory::inventory_properties;
pub(crate) use sl_viewer_kit::minimap_math;
pub(crate) use sl_viewer_kit::radar_model;
pub(crate) use sl_viewer_kit::slt;
pub(crate) use sl_viewer_map::minimap;
pub(crate) use sl_viewer_media::browser_widget;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_notices::linkified_text;
pub(crate) use sl_viewer_notices::notification_host;
pub(crate) use sl_viewer_notices::notification_persist;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_platform::clipboard;
pub(crate) use sl_viewer_platform::system_browser;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::skin;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_sounds;
pub(crate) use sl_viewer_ui_core::ui_text;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::animations;
pub(crate) use sl_viewer_world_objects::avatar_complexity;
pub(crate) use sl_viewer_world_objects::avatar_render_settings;
pub(crate) use sl_viewer_world_objects::derender;
pub(crate) use sl_viewer_world_objects::name_tag_content;
pub(crate) use sl_viewer_world_objects::textures;
pub(crate) use sl_viewer_world_view::session;

pub mod auto_reject;
pub mod avatar_profile;
pub mod blocked;
pub mod contact_sets;
pub mod contact_sets_panel;
pub mod conversations;
pub mod group_notice;
pub mod group_profile;
pub mod groups;
pub mod mutes;
pub mod offers_invites;
pub mod people;
pub mod presence;
pub mod radar;
