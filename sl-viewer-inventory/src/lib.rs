//! The viewer's inventory.
//!
//! The folder and item model fed from the wire ([`inventory`]), the tree and
//! gallery views over it ([`inventory_gallery`]), the filters that narrow them
//! ([`inventory_filters`]), an item's properties and permissions
//! ([`inventory_properties`]), the per-item actions ([`inventory_actions`]),
//! and dragging an item out of the panel onto an avatar, an object's contents
//! or the ground ([`inventory_drag`]).
//!
//! Requests that carry an item -- open this landmark, wear this, embed this in
//! a notecard -- are declared here rather than in the surfaces that answer
//! them, because the payload is this crate's `ItemInfo` and asking should not
//! mean depending on the answer.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `inventory::InventoryModel` and `inventory_drag::DragState`. That \
              only became a lint when these items turned `pub` for the crate split; \
              renaming them would churn every call site in the viewer to satisfy a \
              style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::settings`.
pub(crate) use sl_viewer_kit::avatar_assets;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world::avatars;
pub(crate) use sl_viewer_world::gpu_pick;
pub(crate) use sl_viewer_world::hud_pick;
pub(crate) use sl_viewer_world::input_context;
pub(crate) use sl_viewer_world::objects;
pub(crate) use sl_viewer_world::textures;
pub(crate) use sl_viewer_world_api as world_api;

pub mod inventory;
pub mod inventory_actions;
pub mod inventory_drag;
pub mod inventory_filters;
pub mod inventory_gallery;
pub mod inventory_properties;
