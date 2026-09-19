//! The viewer's **context menus**: what right-clicking a thing offers to do
//! with it.
//!
//! Four entry trees — [`avatar_menu`], [`object_menu`], [`attachment_menu`] and
//! [`land_menu`] — over the radial widget in `sl-viewer-ui-pie-menu`, which
//! holds the compass-point layout and nothing about what the slices say.
//!
//! They are one crate because they are one menu. Right-clicking an avatar
//! offers its attachments, right-clicking an attachment offers the avatar
//! wearing it, both offer the land underfoot, and a sub-menu of one is a
//! sub-menu of another; the four files name each other on nearly every screen.
//! Splitting them by the kind of thing clicked would cut across that rather
//! than along it.
//!
//! What they are *not* is composition. Assembling the viewer is the binary's
//! job; deciding that "Sit Here" belongs south-west of a prim, and that it is
//! greyed out when the prim's flags forbid it, is this crate's.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one menu and is named for it, so its types read \
              as `avatar_menu::OpenAvatarMenu` and `land_menu::LandMenuPlugin`. \
              That only became a lint when these items turned `pub` for the \
              crate split; renaming them would churn every call site in the \
              viewer to satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::objects`.
pub(crate) use sl_viewer_edit::edit_contents;
pub(crate) use sl_viewer_edit::edit_tool;
pub(crate) use sl_viewer_intents as intents;
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::sit_offset;
pub(crate) use sl_viewer_people::contact_sets_panel;
pub(crate) use sl_viewer_places::about_land;
pub(crate) use sl_viewer_social as social;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_pie_menu::pie_menu;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_avatar::avatar_complexity;
pub(crate) use sl_viewer_world_avatar::avatar_render_settings;
pub(crate) use sl_viewer_world_avatar::avatars;
pub(crate) use sl_viewer_world_avatar::derender;
pub(crate) use sl_viewer_world_objects::name_tag_billboard;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_view::gpu_pick;
pub(crate) use sl_viewer_world_view::hud_pick;
pub(crate) use sl_viewer_world_view::input_action;

pub mod attachment_menu;
pub mod avatar_menu;
pub mod land_menu;
pub(crate) mod menu_params;
pub mod object_menu;
