//! What the viewer knows about a place.
//!
//! [`about_land`] is the parcel floater -- ownership, access, media, sale and
//! the options a parcel owner sets. [`about_region`] is its region and estate
//! counterpart, down to the terrain textures and the estate manager list.
//! [`about_landmark`] is the small one: a landmark item's destination, and the
//! teleport to it.
//!
//! All three are read-mostly views over parcel and region state the world
//! layer already holds, plus the wire calls that change it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `about_land::AboutLandUi` and `about_region::AboutRegionUi`. That only became a lint when these items turned `pub` \
              for the crate split; renaming them would churn every call site in \
              the viewer to satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::inventory`.
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_inventory::inventory_properties;
pub(crate) use sl_viewer_notices::ui_name_link;
pub(crate) use sl_viewer_pickers::ui_texture_picker;
pub(crate) use sl_viewer_platform::clipboard;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::textures;
pub(crate) use sl_viewer_world_scene::environment;

pub mod about_land;
pub mod about_landmark;
pub mod about_region;
