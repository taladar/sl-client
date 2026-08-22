//! Media in the viewer: the engine that decodes it, the surfaces it draws to,
//! and the browser widget that hosts a page.
//!
//! Two backends sit behind one boundary — CEF for web pages, GStreamer for
//! streams — and nothing above this crate needs to know which is answering. The
//! parcel audio player and the media-on-a-prim faces are consumers, not part of
//! this crate: what lives here is the machinery, not the placement.
//!
//! - [`media_engine`] — backend selection and the frame pump.
//! - [`media_audio`] — the audio side of a media stream.
//! - [`media_keys`] — keyboard routing into a focused media surface.
//! - [`media_diagnostics`] — what the F3 overlay reports about media.
//! - [`browser_widget`] — a page as a UI element.
//! - [`web_auth`] — the browser-hosted login flow.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `media_engine::MediaEngine`. That only became a lint when these \
              items turned `pub` for the crate split; renaming them would churn \
              every call site in the viewer to satisfy a style rule this codebase \
              does not follow"
)]

pub mod browser_widget;
pub mod media_audio;
pub mod media_diagnostics;
pub mod media_engine;
pub mod media_keys;
pub mod web_auth;
