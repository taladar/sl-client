//! The viewer's directory search.
//!
//! One floater over the four `Dir*` wire queries: People, Groups, Places, Land,
//! Events and Classifieds as result tables, plus a web tab that embeds the
//! grid's own search site. A result's actions -- teleport there, open that
//! profile -- are written as requests rather than performed here, so the
//! floater never has to know which surface answers them.

#![expect(
    clippy::module_name_repetitions,
    reason = "the module owns one concept and is named for it, so its types read \
              as `search::SearchUi` and `search::SearchTab`. That only became a \
              lint when these items turned `pub` for the crate split; renaming \
              them would churn every call site in the viewer to satisfy a style \
              rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so this
// module keeps addressing them as `crate::ui` and `crate::settings`.
pub(crate) use sl_viewer_map::world_map;
pub(crate) use sl_viewer_media::browser_widget;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_radio;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::textures;

pub mod search;
