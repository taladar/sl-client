//! Offscreen Chromium (CEF) web-media engine for Second Life / OpenSim
//! viewers.
//!
//! This crate wraps the [`cef`] crate (prebuilt CEF binaries) behind a small,
//! engine-agnostic surface API so a viewer can render web content both onto
//! in-world prim faces (media-on-a-prim) and into UI widgets (embedded
//! browser panels), without the UI layer ever touching CEF types:
//!
//! - [`MediaBackend`] — the engine: initialise once, create surfaces, pump
//!   its message loop once per frame, shut down.
//! - [`MediaSurface`] — one offscreen browser: navigation, resize, portable
//!   mouse/keyboard input in, BGRA frames and status snapshots out.
//!
//! The concrete CEF implementation lives in [`chromium`]; the traits and the
//! plain data types crossing the boundary live here. Everything is
//! **single-threaded**: CEF is pumped on the caller's thread
//! (`external_message_pump`), all callbacks fire inside
//! [`MediaBackend::pump`], and none of the types are `Send`.
//!
//! Design notes (see `roadmap/in-progress/viewer-media-prim-browser.md` in
//! the workspace): input crosses the boundary as portable *Windows virtual-key
//! code + text* (never native key blobs), frames cross as CPU BGRA buffers
//! (zero-copy is deferred headroom), and each surface can run in an isolated
//! request context so hostile in-world pages cannot read another surface's
//! cookies.

pub mod chromium;
pub mod vk;

use std::path::PathBuf;

/// Errors surfaced by the media backend.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// The global CEF runtime failed to initialise (or was initialised twice).
    #[error("CEF initialisation failed: {0}")]
    Init(String),
    /// Creating a browser surface failed.
    #[error("CEF surface creation failed: {0}")]
    SurfaceCreation(String),
}

/// Configuration for initialising a [`MediaBackend`].
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Root directory for the engine's caches and logs. Must be absolute and
    /// writable; created if missing.
    pub cache_dir: PathBuf,
    /// Path to the subprocess helper executable (`sl-cef-helper`). When
    /// `None`, CEF re-executes the current binary as its subprocesses, which
    /// requires the embedder to call [`chromium::execute_child_process`]
    /// at the very top of `main`.
    pub subprocess_path: Option<PathBuf>,
    /// BCP-47 locale for the browser UI and `Accept-Language`, e.g. `en-US`.
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
    /// UI browser panels may share the global context.
    pub isolated: bool,
    /// Maximum paint rate in frames per second (1–60).
    pub max_fps: u8,
    /// Whether audio starts muted.
    pub muted: bool,
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

/// A portable keyboard event: a Windows virtual-key code (see [`vk`]) plus
/// modifier state. Text input travels separately via
/// [`MediaSurface::insert_text`].
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

/// A snapshot of a surface's navigation and load state. Cheap to clone; the
/// `generation` counter advances whenever any field changes so callers can
/// skip unchanged snapshots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceStatus {
    /// Monotonic change counter for this snapshot.
    pub generation: u64,
    /// The current page URL.
    pub url: String,
    /// The current page title.
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
    /// Whether the browser has been fully closed (its host process side is
    /// gone and the surface can be dropped).
    pub closed: bool,
}

/// One offscreen browser surface.
///
/// All methods are cheap fire-and-forget calls into the engine; effects
/// (frames, status changes) materialise during subsequent
/// [`MediaBackend::pump`] calls.
pub trait MediaSurface {
    /// Navigates the surface to `url`.
    fn navigate(&self, url: &str);
    /// Reloads the current page (bypassing the cache).
    fn reload(&self);
    /// Stops an in-progress load.
    fn stop(&self);
    /// Navigates one step back in history.
    fn go_back(&self);
    /// Navigates one step forward in history.
    fn go_forward(&self);
    /// Resizes the surface to `width` × `height` pixels (each clamped to at
    /// least 1).
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
    /// Asks the browser to close. The surface reports
    /// [`SurfaceStatus::closed`] once tear-down completed (requires further
    /// [`MediaBackend::pump`] calls).
    fn request_close(&self);
}

/// The web-media engine: owns the global runtime, creates surfaces, and
/// pumps the engine's message loop.
pub trait MediaBackend {
    /// Creates a new offscreen surface.
    ///
    /// # Errors
    /// Returns [`MediaError::SurfaceCreation`] if the engine could not create
    /// the browser.
    fn create_surface(
        &mut self,
        config: &SurfaceConfig,
    ) -> Result<Box<dyn MediaSurface>, MediaError>;
    /// Performs one iteration of engine work. Call once per frame on the
    /// thread that initialised the backend; all surface callbacks (paints,
    /// status updates) fire inside this call.
    fn pump(&mut self);
    /// The number of live (not yet fully closed) browser surfaces.
    fn live_surfaces(&self) -> usize;
    /// Shuts the engine down: closes remaining surfaces, pumps until they
    /// are gone (bounded), and tears down the global runtime. Idempotent.
    fn shutdown(&mut self);
}
