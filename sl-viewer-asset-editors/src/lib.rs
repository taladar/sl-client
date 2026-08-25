//! Editors for the assets an inventory item points at.
//!
//! A notecard ([`edit_notecard`]) with its embedded items and the renderer
//! that lays one out for reading ([`notecard_render`]); a script
//! ([`edit_script`]), whether it lives in inventory or in an object's
//! contents, with its compile and run state; and a wearable
//! ([`edit_wearable`]), whose edits preview on the avatar before they are
//! saved.
//!
//! Each opens from a request the inventory writes, so none of them is named
//! by the surface that asks for it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `edit_notecard::NotecardEditor` and `edit_script::ScriptEditor`. That only became a lint when these items turned `pub` \
              for the crate split; renaming them would churn every call site in \
              the viewer to satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::inventory`.
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_inventory::inventory_actions;
pub(crate) use sl_viewer_inventory::inventory_properties;
pub(crate) use sl_viewer_kit::avatar_assets;
pub(crate) use sl_viewer_notices::linkified_text;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_pickers::ui_texture_picker;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_radio;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::avatars;
pub(crate) use sl_viewer_world_objects::bake_inputs;
pub(crate) use sl_viewer_world_objects::textures;

pub mod edit_notecard;
pub mod edit_script;
pub mod edit_wearable;
pub mod notecard_render;
