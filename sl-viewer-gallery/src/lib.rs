//! The viewer's two **offline galleries**: every UI surface, and every render
//! scene, with no login and no world.
//!
//! - [`gallery`] is the UI gallery (`viewer-ui-test-harness`): every element in
//!   the element registry and every window in the floater registry, live, in
//!   any script, direction, font size and UI scale — the surface a person looks
//!   at to judge appearance, beside the sweep that checks it unattended.
//! - [`render_gallery`] is its counterpart for geometry (
//!   `viewer-render-test-harness`): the same converters, materials and
//!   environment the viewer draws the world with, over a fixed set of scenes.
//!
//! # Why the registries are arguments
//!
//! Neither gallery holds a list of what to show. The element and floater
//! registries name three dozen feature modules **plus** the handful of surfaces
//! that genuinely live in the viewer binary, so they stay in the composition
//! root and arrive here as parameters — together with the resolved
//! [`AssetPlugin`](bevy::asset::AssetPlugin), which has to be built where the
//! `assets/` tree is, since its development fallback is a compile-time
//! `CARGO_MANIFEST_DIR`.
//!
//! That is the whole interface. It is what lets the galleries be a crate at all
//! rather than 2,000 lines of harness compiled into the viewer's own library,
//! and it means a gallery can no longer quietly reach into the binary's private
//! module tree.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one gallery and is named for it, so its types \
              read as `gallery::GalleryCell` and `render_gallery::GalleryArgs`. \
              That only became a lint when these items turned `pub` for the \
              crate split; renaming them would churn every call site to satisfy \
              a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::floater`.
pub(crate) use sl_viewer_asset_editors::asset_editor;
pub(crate) use sl_viewer_chat::chat_input;
pub(crate) use sl_viewer_chat::emoji_complete;
pub(crate) use sl_viewer_chat::emoji_picker;
pub(crate) use sl_viewer_chat::local_chat_input;
pub(crate) use sl_viewer_intents as intents;
pub(crate) use sl_viewer_kit::face_material;
pub(crate) use sl_viewer_media::browser_widget;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_notices::linkified_text;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::skin;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_pie_menu::pie_menu;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::menu;
pub(crate) use sl_viewer_ui_widgets::ui_color_picker;
pub(crate) use sl_viewer_ui_widgets::ui_radio;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_ui_widgets::ui_trackball;
pub(crate) use sl_viewer_world_scene::environment;
pub(crate) use sl_viewer_world_scene::probes;
pub(crate) use sl_viewer_world_scene::render_overrides;
pub(crate) use sl_viewer_world_scene::render_scene;
pub(crate) use sl_viewer_world_scene::viewer_camera;

pub mod gallery;
pub mod render_gallery;
