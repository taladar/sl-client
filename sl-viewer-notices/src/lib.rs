//! The viewer's notice surfaces: what interrupts, and what it is written in.
//!
//! A script asks to take controls, an experience asks to be trusted, the
//! simulator sends a dialog with buttons. Each arrives unbidden, has to be
//! shown without stealing the world, and has to survive a relog if it was not
//! answered -- that is [`notification_host`] and [`notification_persist`].
//!
//! The text they are written in lives here too. A notice is mostly a sentence
//! with things in it you can click: a resident, a group, a URL, a SLURL. That
//! is [`linkified_text`] and [`ui_name_link`], which every other surface that
//! shows a clickable name or link also uses.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `linkified_text::LinkTextStyle` and `script_dialog::ScriptDialog`. \
              That only became a lint when these items turned `pub` for the crate \
              split; renaming them would churn every call site in the viewer to \
              satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::settings`.
pub(crate) use sl_viewer_kit::parcel_names;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_platform::system_browser;
pub(crate) use sl_viewer_platform::url_linkify;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world::avatars;
pub(crate) use sl_viewer_world_api as world_api;

pub mod experience_permission;
pub mod experiences_floater;
pub mod linkified_text;
pub mod notification_host;
pub mod notification_persist;
pub mod script_dialog;
pub mod script_permission;
pub mod ui_name_link;
