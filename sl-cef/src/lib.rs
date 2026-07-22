//! Offscreen Chromium (CEF) web-media engine for Second Life / OpenSim
//! viewers.
//!
//! This crate wraps the [`cef`] crate (prebuilt CEF binaries) behind the
//! engine-agnostic surface API of [`sl_media`] so a viewer can render web
//! content both onto in-world prim faces (media-on-a-prim) and into UI
//! widgets (embedded browser panels), without the UI layer ever touching CEF
//! types:
//!
//! - [`MediaBackend`] — the engine: initialise once, create surfaces, pump
//!   its message loop once per frame, shut down.
//! - [`MediaSurface`] — one offscreen browser: navigation, resize, portable
//!   mouse/keyboard input in, BGRA frames and status snapshots out.
//!
//! The traits and the plain data types crossing the boundary live in the
//! shared [`sl_media`] crate (re-exported here, so this crate's consumers
//! need no separate import) — `sl-gst` implements the same boundary for
//! direct video / audio playback. The concrete CEF implementation lives in
//! [`chromium`]. Everything is **single-threaded**: CEF is pumped on the
//! caller's thread (`external_message_pump`), all callbacks fire inside
//! [`MediaBackend::pump`], and none of the types are `Send`.
//!
//! Design notes (see `roadmap/in-progress/viewer-media-prim-browser.md` in
//! the workspace): input crosses the boundary as portable *Windows virtual-key
//! code + text* (never native key blobs), frames cross as CPU BGRA buffers
//! (zero-copy is deferred headroom), and each surface can run in an isolated
//! request context so hostile in-world pages cannot read another surface's
//! cookies.
//!
//! The playback half of the boundary ([`MediaSurface::play`] /
//! [`pause`](MediaSurface::pause) / [`seek`](MediaSurface::seek) /
//! [`set_volume`](MediaSurface::set_volume), [`SurfaceStatus::playback`]) is
//! left at its default no-op implementations here: a browser surface has no
//! media clock — in-page audio/video control stays with the page itself, and
//! the one host-side control CEF exposes is [`MediaSurface::set_muted`].

pub use sl_media::*;

pub mod chromium;
pub mod vk;
