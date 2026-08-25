//! The viewer's two maps.
//!
//! [`minimap`] is the corner view that follows the avatar; [`world_map`] is the
//! full-region browser with search and teleport, and [`world_map_tiles`] fetches
//! and caches the imagery both draw. They share a tracking target, so a beacon
//! set in one is followed by the other.
//!
//! Both read the world -- terrain, avatars, the camera -- and neither is read
//! back by it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `world_map::WorldMapUi` and `minimap::MinimapUi`. That only became \
              a lint when these items turned `pub` for the crate split; renaming \
              them would churn every call site in the viewer to satisfy a style \
              rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::coords` and `crate::settings`.
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::minimap_math;
pub(crate) use sl_viewer_kit::world_map_math;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_text;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_scene::water;

pub mod minimap;
pub mod world_map;
pub mod world_map_tiles;
