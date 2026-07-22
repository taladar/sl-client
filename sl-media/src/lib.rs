//! Engine-agnostic media-surface traits and types for Second Life / OpenSim
//! viewers.
//!
//! A viewer renders two very different kinds of media onto the same in-world
//! surfaces (prim faces, UI panels): **web pages** (an offscreen browser —
//! the `sl-cef` crate) and **video / audio streams** (a media player — the
//! `sl-gst` crate). This crate is the boundary both engines implement, so the
//! viewer's surface-driving, input-routing and texture-mirroring code never
//! touches an engine type:
//!
//! - [`MediaBackend`] — an engine: initialise once, create surfaces, pump
//!   once per frame, shut down.
//! - [`MediaSurface`] — one offscreen surface: navigation, resize, portable
//!   mouse/keyboard input in, BGRA frames and status snapshots out — plus
//!   the time-based playback controls ([`MediaSurface::play`] /
//!   [`pause`](MediaSurface::pause) / [`seek`](MediaSurface::seek)) a video
//!   surface honours and a browser surface ignores.
//! - [`classify_url`] — the URL → [`MediaKind`] dispatch that decides which
//!   engine a media URL goes to (the reference viewer's `mime_types.xml`
//!   dispatch, by URL scheme / extension).
//!
//! Design notes (see `roadmap/in-progress/viewer-media-prim-browser.md` and
//! `roadmap/in-progress/viewer-video-playback.md` in the workspace): input
//! crosses the boundary as portable *Windows virtual-key code + text* (never
//! native key blobs), frames cross as CPU BGRA buffers (zero-copy is deferred
//! headroom), and everything is **single-threaded** from the caller's view:
//! backends are pumped on the caller's thread and none of the trait objects
//! are required to be `Send`.

use std::path::PathBuf;

/// Errors surfaced by a media backend.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// The engine's global runtime failed to initialise (or was initialised
    /// twice).
    #[error("media engine initialisation failed: {0}")]
    Init(String),
    /// Creating a media surface failed.
    #[error("media surface creation failed: {0}")]
    SurfaceCreation(String),
}

/// Configuration for initialising a [`MediaBackend`].
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Root directory for the engine's caches and logs. Must be absolute and
    /// writable; created if missing.
    pub cache_dir: PathBuf,
    /// Path to the subprocess helper executable (`sl-cef-helper`). When
    /// `None`, an engine needing subprocesses re-executes the current binary,
    /// which requires the embedder to detect a child invocation at the very
    /// top of `main`. Engines without subprocesses ignore this.
    pub subprocess_path: Option<PathBuf>,
    /// BCP-47 locale for the engine UI and `Accept-Language`, e.g. `en-US`.
    pub locale: Option<String>,
    /// Extra product token appended to the user agent, e.g.
    /// `SLClientViewer/0.1`.
    pub user_agent_product: Option<String>,
}

/// Configuration for creating one [`MediaSurface`].
#[derive(Debug, Clone)]
pub struct SurfaceConfig {
    /// Initial surface width in pixels (clamped to at least 1).
    pub width: u32,
    /// Initial surface height in pixels (clamped to at least 1).
    pub height: u32,
    /// The URL to load on creation.
    pub initial_url: String,
    /// Whether the surface gets its own isolated in-memory request context
    /// (cookies, storage). In-world media surfaces must be isolated; trusted
    /// UI browser panels may share the global context. Engines without a
    /// request-context notion (the video player) ignore this.
    pub isolated: bool,
    /// Maximum paint rate in frames per second (1–60).
    pub max_fps: u8,
    /// Whether audio starts muted.
    pub muted: bool,
    /// Whether time-based playback restarts from the beginning when it ends
    /// (a `MediaEntry`'s `auto_loop`). Browser surfaces ignore this.
    pub loop_media: bool,
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 768,
            initial_url: String::from("about:blank"),
            isolated: true,
            max_fps: 30,
            muted: false,
            loop_media: false,
        }
    }
}

/// A mouse button, viewer-side vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// The primary button.
    Left,
    /// The middle button / wheel click.
    Middle,
    /// The secondary button.
    Right,
}

/// Modifier state accompanying an input event.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a modifier snapshot genuinely is a set of independent key states"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// A Shift key is held.
    pub shift: bool,
    /// A Control key is held.
    pub control: bool,
    /// An Alt key is held.
    pub alt: bool,
    /// The primary mouse button is held (during drags).
    pub left_button: bool,
}

/// A portable keyboard event: a Windows virtual-key code plus modifier state
/// (the browser engine's `vk` module maps Bevy key codes onto these). Text
/// input travels separately via [`MediaSurface::insert_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    /// `true` for key-down, `false` for key-up.
    pub down: bool,
    /// The Windows virtual-key code (used on every platform — CEF's portable
    /// `windows_key_code` convention).
    pub vk: i32,
    /// Modifier state at the time of the event.
    pub modifiers: Modifiers,
}

/// The pointer cursor a page requests while hovering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorKind {
    /// The default arrow pointer.
    #[default]
    Pointer,
    /// A hand (link) cursor.
    Hand,
    /// An I-beam (text) cursor.
    IBeam,
    /// Any other cursor (resize handles, crosshairs, …).
    Other,
}

/// A read-only view of the newest BGRA frame, valid only inside the
/// [`MediaSurface::with_new_frame`] callback.
#[derive(Debug)]
pub struct FrameView<'buffer> {
    /// Tightly packed BGRA pixel rows, top-down; `width * height * 4` bytes.
    pub bgra: &'buffer [u8],
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// Where a time-based playback surface currently is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    /// The pipeline is still starting up (prerolling / connecting).
    #[default]
    Loading,
    /// Playback is stalled while the network buffer refills.
    Buffering,
    /// The media is playing.
    Playing,
    /// The media is paused (user pause, or stopped-at-position).
    Paused,
    /// Playback reached the end of the stream (and the surface does not
    /// loop).
    Ended,
    /// Playback failed; [`SurfaceStatus::load_error`] carries the reason.
    Error,
}

/// A snapshot of a time-based (video / audio) surface's playback position —
/// [`None`] on surfaces without a media clock (a web page).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlaybackStatus {
    /// The playback lifecycle state.
    pub state: PlaybackState,
    /// The current position in seconds from the start of the media.
    pub position_seconds: f64,
    /// The media duration in seconds; [`None`] while unknown, and for
    /// unbounded live streams.
    pub duration_seconds: Option<f64>,
    /// Whether the media supports seeking (a file does; a live radio stream
    /// does not).
    pub seekable: bool,
    /// The fill level of the network buffer in percent while
    /// [`Buffering`](PlaybackState::Buffering), if known.
    pub buffering_percent: Option<u8>,
}

/// A snapshot of a surface's navigation and load state. Cheap to clone; the
/// `generation` counter advances whenever any field changes so callers can
/// skip unchanged snapshots.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a status snapshot mirrors several independent engine flags (loading, history \
              availability, closed); folding them into state enums would misrepresent that \
              they vary independently"
)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceStatus {
    /// Monotonic change counter for this snapshot.
    pub generation: u64,
    /// The current page / media URL.
    pub url: String,
    /// The current page title — for streams, the "now playing" title from the
    /// stream's metadata (ICY / media tags), when the stream sends one.
    pub title: String,
    /// Whether a load is in progress.
    pub loading: bool,
    /// Whether history back-navigation is possible.
    pub can_go_back: bool,
    /// Whether history forward-navigation is possible.
    pub can_go_forward: bool,
    /// Load progress in `[0, 1]`.
    pub progress: f64,
    /// The last load error, if the most recent navigation failed.
    pub load_error: Option<String>,
    /// The cursor the page currently requests.
    pub cursor: CursorKind,
    /// A URL the page asked to open in a new window (popups are suppressed;
    /// the viewer decides whether to open it elsewhere). Cleared on read via
    /// [`MediaSurface::take_popup_request`].
    pub popup_request: Option<String>,
    /// Whether the surface has been fully closed (its engine side is gone and
    /// the surface can be dropped).
    pub closed: bool,
    /// Time-based playback state ([`None`] on web surfaces).
    pub playback: Option<PlaybackStatus>,
}

/// One offscreen media surface.
///
/// All methods are cheap fire-and-forget calls into the engine; effects
/// (frames, status changes) materialise during subsequent
/// [`MediaBackend::pump`] calls.
///
/// The input methods and history navigation are meaningful on browser
/// surfaces and no-ops on playback surfaces; the playback methods
/// ([`play`](Self::play) / [`pause`](Self::pause) / [`seek`](Self::seek) /
/// [`set_volume`](Self::set_volume)) are the reverse — the default
/// implementations do nothing so each engine only implements its half.
pub trait MediaSurface {
    /// Navigates the surface to `url`.
    fn navigate(&self, url: &str);
    /// Reloads the current page (bypassing the cache). On a playback surface:
    /// restarts the media from the beginning.
    fn reload(&self);
    /// Stops an in-progress load. On a playback surface: stops playback
    /// (pausing at the current position).
    fn stop(&self);
    /// Navigates one step back in history.
    fn go_back(&self);
    /// Navigates one step forward in history.
    fn go_forward(&self);
    /// Resizes the surface to `width` × `height` pixels (each clamped to at
    /// least 1). Playback surfaces keep the media's own frame size and may
    /// ignore this.
    fn resize(&self, width: u32, height: u32);
    /// Grants or removes keyboard focus.
    fn set_focus(&self, focused: bool);
    /// Moves the pointer to surface-local pixel coordinates.
    fn mouse_move(&self, x: i32, y: i32, modifiers: Modifiers);
    /// Tells the page the pointer left the surface.
    fn mouse_leave(&self);
    /// Presses or releases a mouse button at surface-local coordinates.
    /// `click_count` is 1 for single clicks, 2 for double clicks.
    fn mouse_button(
        &self,
        x: i32,
        y: i32,
        button: MouseButton,
        down: bool,
        click_count: u8,
        modifiers: Modifiers,
    );
    /// Scrolls at surface-local coordinates by pixel deltas.
    fn mouse_wheel(&self, x: i32, y: i32, delta_x: i32, delta_y: i32);
    /// Sends a portable key event (see [`KeyInput`]).
    fn key(&self, input: KeyInput);
    /// Inserts committed text (from the platform's text/IME pipeline) as
    /// character input.
    fn insert_text(&self, text: &str);
    /// Sets the maximum paint rate in frames per second (1–60) — the
    /// interest-throttle knob.
    fn set_max_fps(&self, fps: u8);
    /// Mutes or unmutes the surface's audio.
    fn set_muted(&self, muted: bool);
    /// Whether the surface's audio is muted.
    fn muted(&self) -> bool;
    /// Starts / resumes time-based playback (no-op on web surfaces).
    fn play(&self) {}
    /// Pauses time-based playback at the current position (no-op on web
    /// surfaces).
    fn pause(&self) {}
    /// Seeks time-based playback to `seconds` from the start (no-op on web
    /// surfaces and unseekable streams).
    fn seek(&self, seconds: f64) {
        let _unused = seconds;
    }
    /// Sets the playback volume in `[0, 1]` (no-op on web surfaces, whose
    /// audio control is [`set_muted`](Self::set_muted) only for now).
    fn set_volume(&self, volume: f64) {
        let _unused = volume;
    }
    /// Invokes `consumer` with the newest frame if its generation is newer
    /// than `*seen_generation`, then updates `*seen_generation`. Returns
    /// `true` if the consumer ran.
    fn with_new_frame(
        &self,
        seen_generation: &mut u64,
        consumer: &mut dyn FnMut(FrameView<'_>),
    ) -> bool;
    /// Returns the current status snapshot (see [`SurfaceStatus`]).
    fn status(&self) -> SurfaceStatus;
    /// Takes (and clears) a pending popup-open request, if any.
    fn take_popup_request(&self) -> Option<String>;
    /// Asks the engine to close the surface. The surface reports
    /// [`SurfaceStatus::closed`] once tear-down completed (which may require
    /// further [`MediaBackend::pump`] calls).
    fn request_close(&self);
}

/// A media engine: owns its global runtime, creates surfaces, and pumps the
/// engine's per-frame work.
pub trait MediaBackend {
    /// Creates a new offscreen surface.
    ///
    /// # Errors
    /// Returns [`MediaError::SurfaceCreation`] if the engine could not create
    /// the surface.
    fn create_surface(
        &mut self,
        config: &SurfaceConfig,
    ) -> Result<Box<dyn MediaSurface>, MediaError>;
    /// Performs one iteration of engine work. Call once per frame on the
    /// thread that initialised the backend; all surface callbacks (paints,
    /// status updates) fire inside this call.
    fn pump(&mut self);
    /// The number of live (not yet fully closed) surfaces.
    fn live_surfaces(&self) -> usize;
    /// Shuts the engine down: closes remaining surfaces, pumps until they
    /// are gone (bounded), and tears down the global runtime. Idempotent.
    fn shutdown(&mut self);
}

/// Which engine a media URL belongs to — the reference viewer's
/// `mime_types.xml` dispatch ("web" widgets vs the "movie" media plugin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// A web page: render through the browser engine.
    Web,
    /// A video (or video-capable container / streaming manifest): render
    /// through the playback engine.
    Video,
    /// An audio stream / file: play through the playback engine (the surface
    /// shows no frames).
    Audio,
}

/// Video container / manifest file extensions dispatched to the playback
/// engine (Firestorm's `mime_types.xml` `video/*` rows, plus the adaptive
/// manifests).
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "qt", "webm", "mkv", "avi", "ogv", "mpg", "mpeg", "m2v", "ts", "m2ts",
    "3gp", "3g2", "wmv", "flv", "m3u8", "mpd",
];

/// Audio file / playlist extensions dispatched to the playback engine
/// (Firestorm's `mime_types.xml` `audio/*` rows).
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "aac", "m4a", "oga", "ogg", "opus", "wav", "flac", "wma", "aif", "aiff", "pls", "m3u",
];

/// URL schemes that are always streaming media, whatever the path looks like.
const STREAM_SCHEMES: &[&str] = &["rtsp", "rtsps", "rtmp", "rtmps", "mms"];

/// Which engine `url` should be handed to.
///
/// The reference viewer resolves the *served* MIME type with an HTTP probe
/// and dispatches through `mime_types.xml`; this classifier is the
/// deliberate simplification of that — dispatch by URL scheme and path
/// extension, with everything unrecognised going to the browser (which is
/// also the reference's default for unknown types). A media file served
/// under an extension-less URL therefore lands in the browser; revisit with
/// a content-type probe if that turns out to matter in practice.
#[must_use]
pub fn classify_url(url: &url::Url) -> MediaKind {
    let scheme = url.scheme().to_ascii_lowercase();
    if STREAM_SCHEMES.contains(&scheme.as_str()) {
        return MediaKind::Video;
    }
    let path = url.path();
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_stem, extension)| extension.to_ascii_lowercase());
    match extension {
        Some(extension) if VIDEO_EXTENSIONS.contains(&extension.as_str()) => MediaKind::Video,
        Some(extension) if AUDIO_EXTENSIONS.contains(&extension.as_str()) => MediaKind::Audio,
        _other => MediaKind::Web,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{MediaKind, classify_url};

    /// The [`MediaKind`] for a literal URL.
    fn kind(text: &str) -> Result<MediaKind, String> {
        let url = url::Url::parse(text).map_err(|error| format!("bad test url {text}: {error}"))?;
        Ok(classify_url(&url))
    }

    #[test]
    fn video_extensions_dispatch_to_video() -> Result<(), String> {
        assert_eq!(kind("https://a.example/clip.mp4")?, MediaKind::Video);
        assert_eq!(kind("http://a.example/Movie.WebM")?, MediaKind::Video);
        assert_eq!(
            kind("https://cdn.example/live/master.m3u8?token=x")?,
            MediaKind::Video
        );
        assert_eq!(kind("https://a.example/v.ogv")?, MediaKind::Video);
        Ok(())
    }

    #[test]
    fn audio_extensions_dispatch_to_audio() -> Result<(), String> {
        assert_eq!(kind("https://a.example/song.mp3")?, MediaKind::Audio);
        assert_eq!(kind("http://radio.example/stream.pls")?, MediaKind::Audio);
        assert_eq!(kind("https://a.example/track.flac")?, MediaKind::Audio);
        Ok(())
    }

    #[test]
    fn stream_schemes_dispatch_to_video() -> Result<(), String> {
        assert_eq!(kind("rtsp://cam.example/feed")?, MediaKind::Video);
        assert_eq!(kind("rtmp://live.example/app/key")?, MediaKind::Video);
        Ok(())
    }

    #[test]
    fn everything_else_is_web() -> Result<(), String> {
        assert_eq!(kind("https://example.com/")?, MediaKind::Web);
        assert_eq!(kind("https://example.com/page.html")?, MediaKind::Web);
        // Extension-less media URLs (a Shoutcast mount, say) land in the
        // browser — the documented simplification.
        assert_eq!(kind("http://radio.example:8000/stream")?, MediaKind::Web);
        // A dot in a directory, not the file name, is not an extension.
        assert_eq!(kind("https://a.example/v1.2/index")?, MediaKind::Web);
        Ok(())
    }
}
