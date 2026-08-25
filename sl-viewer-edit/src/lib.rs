//! The viewer's build tools.
//!
//! What the reference calls the Build floater, split by tab: the selection
//! itself and the gizmos that move, rotate and stretch it ([`edit_selection`],
//! [`gizmos`], [`edit_tool`]); the object tab's position, rotation, size and
//! physics ([`edit_params`]); the texture and material tabs, both the
//! Blinn-Phong maps and the PBR ones ([`edit_texture`], [`edit_material`],
//! [`edit_texture_align`], [`edit_material_asset`]); an object's contents
//! ([`edit_contents`]); rez ([`edit_create`]); link and unlink
//! ([`edit_link`]); and the undo stack the whole floater writes through
//! ([`edit_undo`]).
//!
//! Everything here reads the world and asks the inventory and the pickers for
//! what it needs; nothing below it reads back.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `edit_tool::EditToolUi` and `edit_selection::SelectionRect`. That \
              only became a lint when these items turned `pub` for the crate split; \
              renaming them would churn every call site in the viewer to satisfy a \
              style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::objects` and `crate::ui`.
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_inventory::inventory_drag;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::edit_math;
pub(crate) use sl_viewer_kit::face_material;
pub(crate) use sl_viewer_pickers::ui_texture_picker;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_text;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_radio;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::legacy_materials;
pub(crate) use sl_viewer_world_objects::material_preview;
pub(crate) use sl_viewer_world_objects::materials;
pub(crate) use sl_viewer_world_objects::object_cost;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_objects::textures;
pub(crate) use sl_viewer_world_view::camera;

pub mod edit_contents;
pub mod edit_create;
pub mod edit_link;
pub mod edit_material;
pub mod edit_material_asset;
pub mod edit_params;
pub mod edit_selection;
pub mod edit_texture;
pub mod edit_texture_align;
pub mod edit_tool;
pub mod edit_undo;
pub mod gizmos;
