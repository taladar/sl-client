//! The viewer's chat surfaces.
//!
//! The nearby-chat overlay that fades in over the world ([`chat`]), the
//! always-visible bar at the bottom edge ([`nearby_chat_bar`]) and what it
//! submits ([`local_chat_input`]), plus the reusable chat input the
//! conversation window also uses ([`chat_input`]) and the emoji picker that
//! hangs off it ([`emoji_picker`]).
//!
//! The conversation window itself is not here -- it is a people surface, and
//! it reads this crate for its input row rather than the other way round.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `chat_input::ChatInputSpec` and `emoji_picker::EmojiPickerUi`. \
              That only became a lint when these items turned `pub` for the crate \
              split; renaming them would churn every call site in the viewer to \
              satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::settings`.
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::emoji_complete;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_view::input_context;

pub mod chat;
pub mod chat_input;
pub mod emoji_picker;
pub mod local_chat_input;
pub mod nearby_chat_bar;
