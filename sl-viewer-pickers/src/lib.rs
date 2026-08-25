//! The viewer's chooser floaters.
//!
//! Two things the rest of the viewer asks for rather than implements: pick a
//! resident ([`avatar_picker`]) and pick a texture or a render material
//! ([`ui_texture_picker`]), the latter also owning the reusable swatch a
//! caller parks in its own panel to show and re-open the current choice.
//!
//! The build tool, the wearable and material editors, the region and land
//! floaters and the profile surfaces all summon one of these; none of them
//! needs to know how a chooser is built.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `avatar_picker::AvatarPickerUi` and \
              `ui_texture_picker::TexturePickerUi`. That only became a lint when \
              these items turned `pub` for the crate split; renaming them would \
              churn every call site in the viewer to satisfy a style rule this \
              codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::inventory`.
pub(crate) use sl_viewer_inventory::inventory;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::floater_persist;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::material_preview;

pub mod avatar_picker;
pub mod ui_texture_picker;
