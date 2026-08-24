//! The viewer's audio surfaces.
//!
//! Three things that all answer to sound and share nothing else with the rest
//! of the viewer: the in-world sound effects an object or an avatar triggers,
//! the parcel music and nearby-media bar at the bottom edge, and the volume
//! panel that governs both. The decode and mixing live below in `sl-audio`;
//! what is here is the viewer's side — what to play, when, and how loud.
//!
//! Nothing in this crate is reached by the world layer or by another feature,
//! so it compiles beside them rather than in front of them.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `parcel_audio::ParcelAudioUi` and `volume_panel::VolumePanel`. \
              That only became a lint when these items turned `pub` for the crate \
              split; renaming them would churn every call site in the viewer to \
              satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::settings` and `crate::ui`.
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::raycast_index;
pub(crate) use sl_viewer_media::media_audio;
pub(crate) use sl_viewer_media::media_diagnostics;
pub(crate) use sl_viewer_platform::sound_cache;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
pub(crate) use sl_viewer_world::avatars;
pub(crate) use sl_viewer_world::objects;
pub(crate) use sl_viewer_world_api as world_api;

pub mod audio;
pub mod parcel_audio;
pub mod volume_panel;
pub mod world_sounds;
