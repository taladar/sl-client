//! The CEF (Chromium Embedded Framework) implementation of
//! [`MediaBackend`] / [`MediaSurface`].
//!
//! Process model: the embedder either ships the `sl-cef-helper` binary next
//! to its executable and passes its path as
//! [`BackendConfig::subprocess_path`], or calls
//! [`execute_child_process`] at the very top of `main` so CEF can re-execute
//! the embedder binary as its subprocesses.
//!
//! Threading: everything here is thread-affine. [`CefMediaBackend::initialize`],
//! [`MediaBackend::pump`], every [`MediaSurface`] call and
//! [`MediaBackend::shutdown`] must happen on the same thread
//! (`external_message_pump` mode — CEF's browser-process UI thread *is* the
//! pumping thread). All handler callbacks fire inside
//! [`MediaBackend::pump`], so the `Rc<RefCell<…>>` shared state is never
//! contended.

use std::cell::RefCell;
use std::os::raw::{c_int, c_ulong};
use std::path::Path;
use std::rc::{Rc as StdRc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cef::args::Args;
use cef::rc::Rc as _;
use cef::{
    App, AudioHandler, AudioParameters, Browser, BrowserHost, BrowserSettings, CefString, Client,
    CommandLine, Cookie, CursorInfo, CursorType, DevToolsMessageObserver, DisplayHandler,
    Errorcode, Frame, ImplApp, ImplAudioHandler, ImplBrowser, ImplBrowserHost, ImplClient,
    ImplCommandLine, ImplCookieManager, ImplDevToolsMessageObserver, ImplDisplayHandler, ImplFrame,
    ImplLifeSpanHandler, ImplLoadHandler, ImplRenderHandler, ImplRequest, ImplRequestHandler,
    KeyEvent, KeyEventType, LifeSpanHandler, LoadHandler, LogSeverity, MouseButtonType, MouseEvent,
    PaintElementType, PopupFeatures, Range, Rect, Registration, RenderHandler, Request,
    RequestContext, RequestContextSettings, RequestHandler, ScreenInfo, Settings, State,
    TransitionType, WindowInfo, WindowOpenDisposition, WrapApp, WrapAudioHandler, WrapClient,
    WrapDevToolsMessageObserver, WrapDisplayHandler, WrapLifeSpanHandler, WrapLoadHandler,
    WrapRenderHandler, WrapRequestHandler, browser_host_create_browser_sync,
    cookie_manager_get_global_manager, request_context_create_context, wrap_app,
    wrap_audio_handler, wrap_client, wrap_dev_tools_message_observer, wrap_display_handler,
    wrap_life_span_handler, wrap_load_handler, wrap_render_handler, wrap_request_handler,
};

use crate::{
    AudioSink, BackendConfig, CursorKind, FrameView, KeyInput, MediaBackend, MediaError,
    MediaSurface, Modifiers, MouseButton, PopupRequest, SharedCookie, SurfaceConfig, SurfaceStatus,
    SurfaceTrust, ValidatedMediaUrl,
};

/// Whether the global CEF runtime was initialised in this process. CEF can
/// only ever be initialised once per process (and cannot be re-initialised
/// after shutdown).
static CEF_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// The next surface's [`SurfaceShared::serial`].
static SURFACE_SERIAL: AtomicU64 = AtomicU64::new(0);

/// The newest paint of one surface, copied out of CEF's `on_paint` buffer.
#[derive(Default)]
struct FrameStore {
    /// Tightly packed BGRA rows, top-down.
    bgra: Vec<u8>,
    /// Frame width in pixels.
    width: u32,
    /// Frame height in pixels.
    height: u32,
    /// Bumped on every stored paint.
    generation: u64,
}

/// State shared between one surface's handle and its CEF handler callbacks.
struct SurfaceShared {
    /// The size `view_rect` reports to CEF (the wanted surface size).
    view_width: i32,
    /// See [`Self::view_width`].
    view_height: i32,
    /// The last pointer position sent, for synthesising leave events.
    last_mouse: (i32, i32),
    /// The newest frame.
    frame: FrameStore,
    /// The navigation/load status snapshot handed to the embedder.
    status: SurfaceStatus,
    /// The live browser, set by `on_after_created` / creation and cleared by
    /// `on_before_close`.
    browser: Option<Browser>,
    /// Whether the page the surface was created with has started loading.
    /// Chromium issues that first navigation after `create_browser_sync`
    /// returns, so a `load_url` sent before it starts is replaced by it.
    first_load_started: bool,
    /// A URL [`MediaSurface::navigate`] was asked for before the first load
    /// started, loaded as soon as it does.
    pending_url: Option<String>,
    /// The address the embedder last sent the surface to, until a page
    /// commits or the load stops: while it is set, a navigation the page on
    /// screen starts by itself is [superseded](superseded_navigation).
    requested_url: Option<String>,
    /// This surface's number in creation order, naming it in the debug log.
    serial: u64,
    /// The DevTools loader of the main frame's current document, from its
    /// `Page.frameNavigated`: its own lifecycle events say when it presents
    /// ([`SurfaceStatus::presented`]), and an older document's are ignored.
    document_loader: Option<String>,
    /// The page-event observer's registration, kept so it stays attached;
    /// dropped when the browser closes.
    devtools: Option<Registration>,
}

impl SurfaceShared {
    /// A fresh shared state for a surface of the given size.
    fn new(width: i32, height: i32, initial_url: &str) -> Self {
        Self {
            view_width: width.max(1),
            view_height: height.max(1),
            last_mouse: (0, 0),
            frame: FrameStore::default(),
            status: SurfaceStatus {
                url: initial_url.to_owned(),
                ..SurfaceStatus::default()
            },
            browser: None,
            first_load_started: false,
            pending_url: None,
            requested_url: None,
            serial: SURFACE_SERIAL.fetch_add(1, Ordering::Relaxed),
            document_loader: None,
            devtools: None,
        }
    }

    /// Marks the status snapshot changed.
    fn touch_status(&mut self) {
        self.status.generation = self.status.generation.wrapping_add(1);
    }
}

wrap_render_handler! {
    // Offscreen render handler: reports the wanted view size and copies
    // each BGRA paint into the shared [`FrameStore`].
    struct OsrRenderHandler {
        shared: StdRc<RefCell<SurfaceShared>>,
    }
    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let shared = self.shared.borrow();
                rect.x = 0;
                rect.y = 0;
                rect.width = shared.view_width.max(1);
                rect.height = shared.view_height.max(1);
            }
        }
        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> c_int {
            match screen_info {
                Some(info) => {
                    info.device_scale_factor = 1.0;
                    1
                }
                None => 0,
            }
        }
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: c_int,
            height: c_int,
        ) {
            if type_ != PaintElementType::VIEW || buffer.is_null() {
                return;
            }
            let (Ok(width_px), Ok(height_px)) = (u32::try_from(width), u32::try_from(height))
            else {
                return;
            };
            let Some(byte_len) = usize::try_from(width_px)
                .ok()
                .zip(usize::try_from(height_px).ok())
                .and_then(|(w, h)| w.checked_mul(h))
                .and_then(|pixels| pixels.checked_mul(4))
            else {
                return;
            };
            if byte_len == 0 {
                return;
            }
            // SAFETY: CEF's on_paint contract: `buffer` points to
            // `width * height * 4` bytes of BGRA pixel data valid for the
            // duration of this callback; width/height were checked positive
            // above and the length product checked for overflow.
            let source = unsafe { std::slice::from_raw_parts(buffer, byte_len) };
            let mut shared = self.shared.borrow_mut();
            let frame = &mut shared.frame;
            frame.bgra.clear();
            frame.bgra.extend_from_slice(source);
            frame.width = width_px;
            frame.height = height_px;
            frame.generation = frame.generation.wrapping_add(1);
        }
    }
}

wrap_load_handler! {
    // Load handler: mirrors loading state and load errors into the shared
    // status snapshot.
    struct OsrLoadHandler {
        shared: StdRc<RefCell<SurfaceShared>>,
    }
    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            browser: Option<&mut Browser>,
            is_loading: c_int,
            can_go_back: c_int,
            can_go_forward: c_int,
        ) {
            let pending = {
                let mut shared = self.shared.borrow_mut();
                shared.status.loading = is_loading != 0;
                shared.status.can_go_back = can_go_back != 0;
                shared.status.can_go_forward = can_go_forward != 0;
                if is_loading != 0 {
                    shared.status.load_error = None;
                    shared.status.progress = 0.0;
                } else {
                    shared.status.progress = 1.0;
                }
                shared.touch_status();
                shared.first_load_started = true;
                if is_loading == 0 {
                    // Whatever became of the asked-for page, it is over.
                    shared.requested_url = None;
                }
                let pending = shared.pending_url.take();
                if pending.is_some() {
                    shared.requested_url.clone_from(&pending);
                }
                (shared.serial, pending)
            };
            let (surface, pending) = pending;
            tracing::debug!(
                surface,
                loading = is_loading != 0,
                pending = pending.as_deref(),
                "CEF surface loading state changed"
            );
            // Outside the borrow: `load_url` may call back into a handler.
            if let Some(url) = pending
                && let Some(frame) = browser.and_then(|browser| browser.main_frame())
            {
                frame.load_url(Some(&CefString::from(url.as_str())));
            }
        }
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            transition_type: TransitionType,
        ) {
            if let Some(frame) = frame.filter(|frame| frame.is_main() != 0) {
                let url = CefString::from(&frame.url()).to_string();
                let surface = {
                    let mut shared = self.shared.borrow_mut();
                    // Only the asked-for page (or a link the user followed)
                    // got this far: the page on screen is no longer the old one.
                    shared.requested_url = None;
                    shared.serial
                };
                tracing::debug!(surface, url, ?transition_type, "CEF surface committed a page");
            }
        }
        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let raw_code: cef::sys::cef_errorcode_t = error_code.into();
            if raw_code == cef::sys::cef_errorcode_t::ERR_ABORTED {
                tracing::debug!(
                    surface = self.shared.borrow().serial,
                    url = failed_url.map(CefString::to_string),
                    "CEF surface navigation aborted"
                );
                return;
            }
            if frame.map(|f| f.is_main() != 0) != Some(true) {
                return;
            }
            let text = error_text.map(CefString::to_string).unwrap_or_default();
            let url = failed_url.map(CefString::to_string).unwrap_or_default();
            let mut shared = self.shared.borrow_mut();
            shared.status.load_error = Some(format!("{text} ({url})"));
            shared.touch_status();
        }
    }
}

wrap_display_handler! {
    // Display handler: mirrors URL / title / progress / cursor changes into
    // the shared status snapshot.
    struct OsrDisplayHandler {
        shared: StdRc<RefCell<SurfaceShared>>,
    }
    impl DisplayHandler {
        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if frame.map(|f| f.is_main() != 0) != Some(true) {
                return;
            }
            if let Some(url) = url {
                let mut shared = self.shared.borrow_mut();
                shared.status.url = url.to_string();
                shared.touch_status();
            }
        }
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(title) = title {
                let mut shared = self.shared.borrow_mut();
                shared.status.title = title.to_string();
                shared.touch_status();
            }
        }
        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            let mut shared = self.shared.borrow_mut();
            shared.status.progress = progress;
            shared.touch_status();
        }
        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: c_ulong,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> c_int {
            let raw: cef::sys::cef_cursor_type_t = type_.into();
            let kind = match raw {
                cef::sys::cef_cursor_type_t::CT_POINTER => CursorKind::Pointer,
                cef::sys::cef_cursor_type_t::CT_HAND => CursorKind::Hand,
                cef::sys::cef_cursor_type_t::CT_IBEAM => CursorKind::IBeam,
                _ => CursorKind::Other,
            };
            let mut shared = self.shared.borrow_mut();
            if shared.status.cursor != kind {
                shared.status.cursor = kind;
                shared.touch_status();
            }
            1
        }
    }
}

wrap_life_span_handler! {
    // Life-span handler: tracks browser creation/closure and suppresses
    // popups (recording the requested URL for the embedder to route).
    struct OsrLifeSpanHandler {
        shared: StdRc<RefCell<SurfaceShared>>,
    }
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let host = browser.as_deref().and_then(ImplBrowser::host);
            let registration = host.as_ref().and_then(|host| {
                let mut observer = OsrPageEvents::new(StdRc::clone(&self.shared));
                let registration = host.add_dev_tools_message_observer(Some(&mut observer));
                for message in PAGE_EVENT_REQUESTS {
                    if host.send_dev_tools_message(Some(message.as_bytes())) == 0 {
                        tracing::warn!(message, "CEF refused a DevTools request");
                    }
                }
                registration
            });
            let mut shared = self.shared.borrow_mut();
            if registration.is_none() {
                tracing::warn!(
                    surface = shared.serial,
                    "CEF surface has no page events: it will read busy throughout"
                );
            }
            shared.devtools = registration;
            shared.browser = browser.map(|b| b.clone());
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            let mut shared = self.shared.borrow_mut();
            shared.devtools = None;
            shared.browser = None;
            shared.status.closed = true;
            shared.touch_status();
        }
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: c_int,
            target_url: Option<&CefString>,
            target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<cef::DictionaryValue>>,
            _no_javascript_access: Option<&mut c_int>,
        ) -> c_int {
            if let Some(url) = target_url {
                let mut shared = self.shared.borrow_mut();
                let target = target_frame_name.map(ToString::to_string).unwrap_or_default();
                tracing::debug!(
                    surface = shared.serial,
                    url = %url,
                    target,
                    "CEF surface asked for a popup"
                );
                shared.status.popup_request = Some(PopupRequest::new(url.to_string(), &target));
                shared.touch_status();
            }
            1
        }
    }
}

/// The DevTools requests that make a surface report its page's lifecycle:
/// the page domain's events, the frame tree (naming the document already
/// there, whose arrival the observer attached too late for), then the
/// lifecycle events, which start by replaying the steps that document has
/// already taken.
const PAGE_EVENT_REQUESTS: [&str; 3] = [
    r#"{"id":1,"method":"Page.enable"}"#,
    r#"{"id":2,"method":"Page.getFrameTree"}"#,
    r#"{"id":3,"method":"Page.setLifecycleEventsEnabled","params":{"enabled":true}}"#,
];

/// The id of the `Page.getFrameTree` request in [`PAGE_EVENT_REQUESTS`].
const FRAME_TREE_REQUEST: c_int = 2;

/// What one DevTools page event says about the main frame's document.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentEvent {
    /// The main frame's document when the observer attached, loaded by
    /// `loader`: the current one unless an arrival was already seen.
    Current {
        /// The document's DevTools loader id.
        loader: String,
    },
    /// A new document arrived in the main frame, loaded by `loader`.
    Arrived {
        /// The document's DevTools loader id.
        loader: String,
    },
    /// The document `loader` loaded has presented: its first contentful
    /// paint, or for a page with nothing to paint, its network falling idle.
    Presented {
        /// The document's DevTools loader id.
        loader: String,
    },
}

/// `Page.frameNavigated`'s parameters, as far as they are read.
#[derive(serde::Deserialize)]
struct FrameNavigated {
    /// The frame that navigated.
    frame: NavigatedFrame,
}

/// The frame of a [`FrameNavigated`].
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NavigatedFrame {
    /// The parent frame's id; none for the main frame.
    parent_id: Option<String>,
    /// The loader of the frame's new document.
    loader_id: String,
}

/// `Page.getFrameTree`'s result, as far as it is read.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameTreeResult {
    /// The tree, rooted at the main frame.
    frame_tree: FrameTree,
}

/// One node of a [`FrameTreeResult`].
#[derive(serde::Deserialize)]
struct FrameTree {
    /// The node's frame.
    frame: NavigatedFrame,
}

/// Read a `Page.getFrameTree` result as the main frame's
/// [`DocumentEvent::Current`].
fn frame_tree_document(result: &[u8]) -> Option<DocumentEvent> {
    let tree: FrameTreeResult = serde_json::from_slice(result).ok()?;
    Some(DocumentEvent::Current {
        loader: tree.frame_tree.frame.loader_id,
    })
}

/// `Page.lifecycleEvent`'s parameters, as far as they are read.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleEvent {
    /// The loader of the document the event is about.
    loader_id: String,
    /// The lifecycle step (`init`, `load`, `firstContentfulPaint`, ...).
    name: String,
}

/// Read a DevTools event as a [`DocumentEvent`], or `None` for one that says
/// nothing about a document's arrival or presentation.
///
/// Chromium keeps showing the old page and drops input to the new one until
/// the new document's first contentful paint (paint holding), so that paint is
/// when the page can take a click. A page that never paints anything
/// contentful is let go once its network falls idle.
fn document_event(method: &str, params: &[u8]) -> Option<DocumentEvent> {
    match method {
        "Page.frameNavigated" => {
            let navigated: FrameNavigated = serde_json::from_slice(params).ok()?;
            navigated
                .frame
                .parent_id
                .is_none()
                .then_some(DocumentEvent::Arrived {
                    loader: navigated.frame.loader_id,
                })
        }
        "Page.lifecycleEvent" => {
            let event: LifecycleEvent = serde_json::from_slice(params).ok()?;
            matches!(event.name.as_str(), "firstContentfulPaint" | "networkIdle").then_some(
                DocumentEvent::Presented {
                    loader: event.loader_id,
                },
            )
        }
        _ => None,
    }
}

/// Apply `event` to a surface's current document `loader` and its
/// `presented` flag; whether `presented` changed. A presentation counts only
/// for the current document.
fn apply_document_event(
    loader: &mut Option<String>,
    presented: &mut bool,
    event: DocumentEvent,
) -> bool {
    match event {
        DocumentEvent::Current { loader: current } => {
            if loader.is_none() {
                *loader = Some(current);
            }
            false
        }
        DocumentEvent::Arrived { loader: arrived } => {
            *loader = Some(arrived);
            core::mem::replace(presented, false)
        }
        DocumentEvent::Presented { loader: shown } => {
            let current = loader.as_deref() == Some(shown.as_str());
            let changed = current && !*presented;
            if changed {
                *presented = true;
            }
            changed
        }
    }
}

wrap_dev_tools_message_observer! {
    // Page-event observer: follows the main frame's document from its arrival
    // to its first contentful paint, for `SurfaceStatus::presented`.
    struct OsrPageEvents {
        shared: StdRc<RefCell<SurfaceShared>>,
    }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: c_int,
            success: c_int,
            result: Option<&[u8]>,
        ) {
            if success == 0 {
                tracing::warn!(
                    surface = self.shared.borrow().serial,
                    message_id,
                    result = %String::from_utf8_lossy(result.unwrap_or_default()),
                    "CEF surface's DevTools request failed"
                );
                return;
            }
            if message_id != FRAME_TREE_REQUEST {
                return;
            }
            if let Some(event) = frame_tree_document(result.unwrap_or_default()) {
                self.apply(event);
            }
        }
        fn on_dev_tools_event(
            &self,
            _browser: Option<&mut Browser>,
            method: Option<&CefString>,
            params: Option<&[u8]>,
        ) {
            let Some(method) = method.map(CefString::to_string) else {
                return;
            };
            if let Some(event) = document_event(&method, params.unwrap_or_default()) {
                self.apply(event);
            }
        }
    }
}

impl OsrPageEvents {
    /// Apply one [`DocumentEvent`] to the surface's status.
    fn apply(&self, event: DocumentEvent) {
        let mut shared = self.shared.borrow_mut();
        let shared = &mut *shared;
        let arrived = matches!(event, DocumentEvent::Arrived { .. });
        if apply_document_event(
            &mut shared.document_loader,
            &mut shared.status.presented,
            event,
        ) || arrived
        {
            shared.touch_status();
            tracing::debug!(
                surface = shared.serial,
                presented = shared.status.presented,
                "CEF surface document"
            );
        }
    }
}

/// Audio state shared between one surface's CEF audio handler and its
/// [`CefMediaSurface`] handle. **Only this `Arc<Mutex<…>>` is touched from CEF's
/// audio thread** — never the `!Send` [`SurfaceShared`] — because
/// `on_audio_stream_packet` fires off the pump thread, exactly as the roadmap's
/// watch-out records.
#[derive(Default)]
struct CefAudioState {
    /// The mixer input to push into, attached by the viewer after creation.
    sink: Option<Arc<dyn AudioSink>>,
    /// The current stream's channel count (from `on_audio_stream_started`),
    /// needed to read each planar packet's per-channel pointers.
    channels: usize,
    /// Whether this surface is muted at the mixer input.
    muted: bool,
}

wrap_audio_handler! {
    // Per-surface audio handler. Installing it makes CEF deliver the page's PCM
    // here — planar f32, on CEF's audio thread — instead of opening its own
    // output stream, so page audio joins the shared mixer (positional for a prim
    // surface, 2-D for a UI panel). This is the escape from Firestorm's Dullahan
    // limitation the `viewer-cef-audio-mixer-handoff` task targets.
    struct OsrAudioHandler {
        audio: Arc<Mutex<CefAudioState>>,
    }
    impl AudioHandler {
        fn on_audio_stream_started(
            &self,
            _browser: Option<&mut Browser>,
            params: Option<&AudioParameters>,
            channels: c_int,
        ) {
            let rate = params
                .and_then(|params| u32::try_from(params.sample_rate).ok())
                .unwrap_or(48_000);
            let channel_count = usize::try_from(channels).unwrap_or(2).max(1);
            let sink = match self.audio.lock() {
                Ok(mut guard) => {
                    guard.channels = channel_count;
                    guard.sink.clone()
                }
                Err(_poisoned) => None,
            };
            if let Some(sink) = sink {
                sink.configure(rate, u16::try_from(channel_count).unwrap_or(2));
            }
        }
        fn on_audio_stream_packet(
            &self,
            _browser: Option<&mut Browser>,
            data: *mut *const f32,
            frames: c_int,
            _pts: i64,
        ) {
            if data.is_null() {
                return;
            }
            let Ok(frames) = usize::try_from(frames) else {
                return;
            };
            let (channels, sink) = match self.audio.lock() {
                Ok(guard) => (guard.channels, guard.sink.clone()),
                Err(_poisoned) => return,
            };
            let Some(sink) = sink else {
                return;
            };
            if channels == 0 {
                return;
            }
            // SAFETY: CEF's `on_audio_stream_packet` contract — `data` points to
            // `channels` channel pointers, each addressing `frames` f32 samples
            // valid for the duration of this call. `channels` came from the
            // paired `on_audio_stream_started`; `frames` is the argument.
            let plane_ptrs = unsafe { std::slice::from_raw_parts(data, channels) };
            let mut planes: Vec<&[f32]> = Vec::with_capacity(channels);
            for &plane in plane_ptrs {
                if plane.is_null() {
                    return;
                }
                // SAFETY: as above — each channel pointer addresses `frames` f32.
                planes.push(unsafe { std::slice::from_raw_parts(plane, frames) });
            }
            sink.push_planar(&planes);
        }
        fn on_audio_stream_stopped(&self, _browser: Option<&mut Browser>) {
            let sink = self.audio.lock().ok().and_then(|guard| guard.sink.clone());
            if let Some(sink) = sink {
                sink.stopped();
            }
        }
        fn on_audio_stream_error(
            &self,
            _browser: Option<&mut Browser>,
            message: Option<&CefString>,
        ) {
            if let Some(message) = message {
                tracing::debug!("CEF audio stream error: {}", message.to_string());
            }
            let sink = self.audio.lock().ok().and_then(|guard| guard.sink.clone());
            if let Some(sink) = sink {
                sink.stopped();
            }
        }
    }
}

wrap_request_handler! {
    // Request handler: keeps an address the embedder asked for from being
    // overtaken by a navigation the page still on screen starts by itself.
    struct OsrRequestHandler {
        shared: StdRc<RefCell<SurfaceShared>>,
    }
    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            user_gesture: c_int,
            is_redirect: c_int,
        ) -> c_int {
            if frame.map(|frame| frame.is_main() != 0) != Some(true) {
                return 0;
            }
            let Some(request) = request else {
                return 0;
            };
            let url = CefString::from(&request.url()).to_string();
            let mut shared = self.shared.borrow_mut();
            let surface = shared.serial;
            let Some(requested) = shared.requested_url.as_deref() else {
                return 0;
            };
            if superseded_navigation(requested, &url, user_gesture != 0, is_redirect != 0) {
                tracing::debug!(
                    surface,
                    url,
                    requested,
                    "CEF surface cancelled a navigation the asked-for address supersedes"
                );
                return 1;
            }
            if user_gesture != 0 {
                // A link the user followed is newer than the asked-for address.
                shared.requested_url = None;
            }
            0
        }
    }
}

/// Whether a main-frame navigation to `url` is one the address the embedder
/// last asked for, `requested`, supersedes and is to be cancelled.
///
/// Between the embedder's `load_url` and the commit of that page, the page
/// still on screen keeps running, and a client redirect of its own — a script
/// or a meta refresh, as `https://secondlife.com/` does on its way to its
/// OpenID check — would otherwise replace the asked-for page as soon as it
/// started, leaving the old site where the user asked for another.
///
/// Kept: the asked-for page itself, a server redirect (one navigation's next
/// hop — the asked-for page's own, since its `load_url` cancelled every
/// older one), and anything the user's gesture started.
fn superseded_navigation(
    requested: &str,
    url: &str,
    user_gesture: bool,
    is_redirect: bool,
) -> bool {
    if user_gesture || is_redirect {
        return false;
    }
    !same_address(requested, url)
}

/// Whether two addresses name the same page, compared as Chromium and the
/// media allowlist both normalise them (`https://example.com` is
/// `https://example.com/`); one that does not parse is compared as written.
fn same_address(a: &str, b: &str) -> bool {
    match (ValidatedMediaUrl::parse(a), ValidatedMediaUrl::parse(b)) {
        (Ok(a), Ok(b)) => a.as_str() == b.as_str(),
        _ => a == b,
    }
}

wrap_client! {
    // The per-surface CEF client wiring the handlers above together.
    struct OsrClient {
        render: RenderHandler,
        load: LoadHandler,
        display: DisplayHandler,
        life_span: LifeSpanHandler,
        audio: AudioHandler,
        request: RequestHandler,
    }
    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life_span.clone())
        }
        fn audio_handler(&self) -> Option<AudioHandler> {
            Some(self.audio.clone())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request.clone())
        }
    }
}

wrap_app! {
    // The browser-process application: injects command-line switches before
    // Chromium parses them.
    struct OsrApp {
        // Whether to run on Chromium's `headless` Ozone platform (no display).
        headless: bool,
    }
    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let is_browser_process = process_type.map(CefString::to_string).unwrap_or_default().is_empty();
            if !is_browser_process {
                return;
            }
            if let Some(command_line) = command_line {
                // In-world media must be able to start playback without a
                // user gesture (the `auto_play` media flag).
                command_line.append_switch_with_value(
                    Some(&CefString::from("autoplay-policy")),
                    Some(&CefString::from("no-user-gesture-required")),
                );
                // Keep Chromium off the desktop keyring. Its OSCrypt asks the
                // Secret Service for a "Chromium Safe Storage" key to encrypt
                // the cookie database at rest, which puts a keyring unlock
                // prompt in front of the viewer on every start (the login page
                // and any UI browser bring CEF up early). Nothing of ours is
                // behind that prompt, and cancelling it is harmless — OSCrypt
                // then falls back to a fixed key and cookies still persist — so
                // it is a credential prompt that means nothing. As of Chromium
                // 139 `--password-store=basic` is the only switch that stops
                // it: the Secret Service key provider's auto-detection sends
                // every non-KDE desktop to the Secret Service, so no
                // environment avoids it. (The patched Firestorm harness needs
                // a `cef_initialize` interposer for the same switch, because
                // dullahan hands CEF an empty command line; we own the `App`,
                // so this is the whole fix here.) A no-op off Linux.
                command_line.append_switch_with_value(
                    Some(&CefString::from("password-store")),
                    Some(&CefString::from("basic")),
                );
                // A headless embedder has no display, and Chromium's default
                // Ozone platform (Wayland / X11) exits the whole engine when it
                // cannot connect to one. Off-screen rendering needs none, so run
                // on the `headless` platform, which Chromium passes on to its
                // subprocesses itself. That platform has no EGL display either,
                // so the GPU process fails to start and is respawned again and
                // again; the surfaces here are painted into CPU buffers
                // (`on_paint`) anyway, so raster and composite in software.
                if self.headless {
                    command_line.append_switch_with_value(
                        Some(&CefString::from("ozone-platform")),
                        Some(&CefString::from("headless")),
                    );
                    command_line.append_switch(Some(&CefString::from("disable-gpu")));
                    command_line
                        .append_switch(Some(&CefString::from("disable-gpu-compositing")));
                }
            }
        }
    }
}

/// Runs the CEF subprocess entry point for the current process and returns
/// its exit code.
///
/// Returns a negative value when the current process is *not* a CEF
/// subprocess (i.e. it is the browser/main process and should continue
/// normally). The `sl-cef-helper` binary is a thin shell over this; an
/// embedder that does not ship the helper must call this at the very top of
/// `main` and `std::process::exit` with the returned code when it is
/// non-negative.
#[must_use]
pub fn execute_child_process() -> i32 {
    let _hash = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    cef::execute_process(Some(args.as_main_args()), None, std::ptr::null_mut())
}

/// Converts a [`Modifiers`] set to CEF's event-flags bit set.
fn modifier_flags(modifiers: Modifiers) -> u32 {
    let mut flags = 0;
    if modifiers.shift {
        flags |= cef::sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0;
    }
    if modifiers.control {
        flags |= cef::sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0;
    }
    if modifiers.alt {
        flags |= cef::sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0;
    }
    if modifiers.left_button {
        flags |= cef::sys::cef_event_flags_t::EVENTFLAG_LEFT_MOUSE_BUTTON.0;
    }
    flags
}

/// Converts a filesystem path to the UTF-16 string CEF settings expect.
fn path_string(path: &Path) -> CefString {
    CefString::from(path.to_string_lossy().as_ref())
}

/// The per-surface capability set for `trust`, painting at `max_fps`.
///
/// A trusted panel (the search tab, a profile page, the web floater) and an
/// in-world prim face are the same kind of offscreen browser, so they used to
/// be created with the same settings — which meant whatever Chromium's
/// defaults happen to be, for a page whose URL any object owner writes.
/// [`crate::ValidatedMediaUrl`] already decides *what* an in-world page may
/// name; this decides what it may *do* once loaded.
///
/// The model is the reference viewer's `media_plugin_cef`, which likewise
/// leaves JavaScript, images, remote fonts and WebGL on for media (in-world
/// pages routinely need all four) and spends its hardening on the capabilities
/// that reach *out of* the page:
///
/// - **`javascript_close_windows`** — a page may not close the surface the
///   viewer owns. The embedder decides when a media face or panel goes away.
/// - **`javascript_access_clipboard`** / **`javascript_dom_paste`** — the
///   user's clipboard is viewer state; a parcel's page must not read what is
///   in it or push into it. This is the sharpest of the set: a clipboard read
///   leaks whatever the user last copied — a password, a chat line — to the
///   object owner's server on the next fetch.
/// - **`local_storage`** / **`databases_deprecated`** — an in-world page gets
///   a fresh in-memory request context per surface anyway (see
///   [`SurfaceTrust::is_isolated`]), so storage buys it nothing but a place to
///   accumulate state the user cannot see or clear.
///
/// Everything a trusted panel needs — a persisted login in `local_storage`, a
/// copy button in the search results — stays on for [`SurfaceTrust::Viewer`],
/// and those surfaces are stated explicitly rather than left at
/// `State::DEFAULT` so the contrast is in the source rather than in Chromium's
/// build flags.
fn browser_settings(trust: SurfaceTrust, max_fps: u8) -> BrowserSettings {
    let trusted_only = if trust.is_isolated() {
        State::DISABLED
    } else {
        State::ENABLED
    };
    BrowserSettings {
        windowless_frame_rate: i32::from(max_fps.clamp(1, 60)),
        background_color: 0xFFFF_FFFF,
        // On for both: an in-world page that cannot script, load images or
        // use WebGL is not media any more, and the reference viewer keeps all
        // of these for media too.
        javascript: State::ENABLED,
        image_loading: State::ENABLED,
        remote_fonts: State::ENABLED,
        webgl: State::ENABLED,
        // Trusted panels only.
        javascript_close_windows: trusted_only,
        javascript_access_clipboard: trusted_only,
        javascript_dom_paste: trusted_only,
        local_storage: trusted_only,
        databases_deprecated: trusted_only,
        ..BrowserSettings::default()
    }
}

/// One offscreen CEF browser surface (see [`MediaSurface`]).
pub struct CefMediaSurface {
    /// State shared with the CEF handler callbacks.
    shared: StdRc<RefCell<SurfaceShared>>,
    /// Audio state shared with the CEF audio handler (a separate `Send + Sync`
    /// cell, because audio callbacks fire off the pump thread).
    audio: Arc<Mutex<CefAudioState>>,
}

impl std::fmt::Debug for CefMediaSurface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shared = self.shared.borrow();
        formatter
            .debug_struct("CefMediaSurface")
            .field("view_width", &shared.view_width)
            .field("view_height", &shared.view_height)
            .field("url", &shared.status.url)
            .field("closed", &shared.status.closed)
            .finish()
    }
}

/// CEF's "no range" (`CefRange::InvalidRange`): an IME composition or commit
/// that replaces nothing but the composition itself.
const NO_REPLACEMENT: Range = Range {
    from: u32::MAX,
    to: u32::MAX,
};

/// The UTF-16 offset of UTF-8 byte offset `byte` in `text` — winit reports an
/// IME cursor in bytes, CEF takes it in UTF-16 code units. A byte offset past
/// the end, or inside a character, counts as the end.
fn utf16_offset(text: &str, byte: usize) -> u32 {
    let prefix = text.get(..byte).unwrap_or(text);
    u32::try_from(prefix.encode_utf16().count()).unwrap_or(u32::MAX)
}

impl CefMediaSurface {
    /// The browser host, if the browser is still alive. Clones the handle out
    /// of the shared cell so no borrow is held across the CEF call.
    fn host(&self) -> Option<BrowserHost> {
        let browser = self.shared.borrow().browser.clone();
        browser.and_then(|browser| browser.host())
    }

    /// The browser, if still alive (borrow-free clone, see [`Self::host`]).
    fn browser(&self) -> Option<Browser> {
        self.shared.borrow().browser.clone()
    }
}

impl MediaSurface for CefMediaSurface {
    fn navigate(&self, url: &ValidatedMediaUrl) {
        {
            let mut shared = self.shared.borrow_mut();
            if !shared.first_load_started {
                tracing::debug!(
                    surface = shared.serial,
                    url = url.as_str(),
                    "CEF surface navigation held until the first load starts"
                );
                shared.pending_url = Some(url.as_str().to_owned());
                return;
            }
        }
        let surface = {
            let mut shared = self.shared.borrow_mut();
            shared.requested_url = Some(url.as_str().to_owned());
            shared.serial
        };
        tracing::debug!(surface, url = url.as_str(), "CEF surface navigating");
        if let Some(frame) = self.browser().and_then(|browser| browser.main_frame()) {
            frame.load_url(Some(&CefString::from(url.as_str())));
        }
    }

    fn reload(&self) {
        if let Some(browser) = self.browser() {
            browser.reload_ignore_cache();
        }
    }

    fn stop(&self) {
        if let Some(browser) = self.browser() {
            browser.stop_load();
        }
    }

    fn go_back(&self) {
        if let Some(browser) = self.browser() {
            browser.go_back();
        }
    }

    fn go_forward(&self) {
        if let Some(browser) = self.browser() {
            browser.go_forward();
        }
    }

    fn resize(&self, width: u32, height: u32) {
        let width = i32::try_from(width.clamp(1, 8192)).unwrap_or(1);
        let height = i32::try_from(height.clamp(1, 8192)).unwrap_or(1);
        {
            let mut shared = self.shared.borrow_mut();
            if shared.view_width == width && shared.view_height == height {
                return;
            }
            shared.view_width = width;
            shared.view_height = height;
        }
        // view_rect may fire synchronously inside was_resized, so the borrow
        // above must already be released here.
        if let Some(host) = self.host() {
            host.was_resized();
        }
    }

    fn set_focus(&self, focused: bool) {
        if let Some(host) = self.host() {
            host.set_focus(c_int::from(focused));
        }
    }

    fn mouse_move(&self, x: i32, y: i32, modifiers: Modifiers) {
        self.shared.borrow_mut().last_mouse = (x, y);
        if let Some(host) = self.host() {
            let event = MouseEvent {
                x,
                y,
                modifiers: modifier_flags(modifiers),
            };
            host.send_mouse_move_event(Some(&event), 0);
        }
    }

    fn mouse_leave(&self) {
        let (x, y) = self.shared.borrow().last_mouse;
        if let Some(host) = self.host() {
            let event = MouseEvent { x, y, modifiers: 0 };
            host.send_mouse_move_event(Some(&event), 1);
        }
    }

    fn mouse_button(
        &self,
        x: i32,
        y: i32,
        button: MouseButton,
        down: bool,
        click_count: u8,
        modifiers: Modifiers,
    ) {
        let surface = {
            let mut shared = self.shared.borrow_mut();
            shared.last_mouse = (x, y);
            shared.serial
        };
        tracing::debug!(surface, x, y, down, "CEF surface mouse button");
        if let Some(host) = self.host() {
            let event = MouseEvent {
                x,
                y,
                modifiers: modifier_flags(modifiers),
            };
            let button = match button {
                MouseButton::Left => MouseButtonType::LEFT,
                MouseButton::Middle => MouseButtonType::MIDDLE,
                MouseButton::Right => MouseButtonType::RIGHT,
            };
            let mouse_up = c_int::from(!down);
            let click_count = i32::from(click_count.max(1));
            host.send_mouse_click_event(Some(&event), button, mouse_up, click_count);
        }
    }

    fn mouse_wheel(&self, x: i32, y: i32, delta_x: i32, delta_y: i32) {
        if let Some(host) = self.host() {
            let event = MouseEvent { x, y, modifiers: 0 };
            host.send_mouse_wheel_event(Some(&event), delta_x, delta_y);
        }
    }

    fn key(&self, input: KeyInput) {
        if let Some(host) = self.host() {
            let event = KeyEvent {
                type_: if input.down {
                    KeyEventType::RAWKEYDOWN
                } else {
                    KeyEventType::KEYUP
                },
                modifiers: modifier_flags(input.modifiers),
                windows_key_code: input.vk,
                ..KeyEvent::default()
            };
            host.send_key_event(Some(&event));
        }
    }

    fn insert_text(&self, text: &str) {
        if let Some(host) = self.host() {
            for unit in text.encode_utf16() {
                let event = KeyEvent {
                    type_: KeyEventType::CHAR,
                    windows_key_code: i32::from(unit),
                    character: unit,
                    unmodified_character: unit,
                    ..KeyEvent::default()
                };
                host.send_key_event(Some(&event));
            }
        }
    }

    fn ime_set_composition(&self, text: &str, cursor: Option<(usize, usize)>) {
        if text.is_empty() {
            self.ime_cancel();
            return;
        }
        if let Some(host) = self.host() {
            let end = utf16_offset(text, text.len());
            let selection = cursor.map_or(Range { from: end, to: end }, |(anchor, focus)| {
                let anchor = utf16_offset(text, anchor);
                let focus = utf16_offset(text, focus);
                Range {
                    from: anchor.min(focus),
                    to: anchor.max(focus),
                }
            });
            // No underline list: CEF then draws its own single underline under
            // the whole composition, which is what winit's one cursor range can
            // describe anyway.
            host.ime_set_composition(
                Some(&CefString::from(text)),
                None,
                Some(&NO_REPLACEMENT),
                Some(&selection),
            );
        }
    }

    fn ime_commit(&self, text: &str) {
        if let Some(host) = self.host() {
            host.ime_commit_text(Some(&CefString::from(text)), Some(&NO_REPLACEMENT), 0);
        }
    }

    fn ime_cancel(&self) {
        if let Some(host) = self.host() {
            host.ime_cancel_composition();
        }
    }

    fn set_max_fps(&self, fps: u8) {
        if let Some(host) = self.host() {
            host.set_windowless_frame_rate(i32::from(fps.clamp(1, 60)));
        }
    }

    fn set_muted(&self, muted: bool) {
        // With a mixer sink attached the mute lives at the mixer input (the page
        // keeps delivering PCM, muted downstream, so the media bus and future
        // per-surface volume govern it); otherwise fall back to CEF's own device
        // mute.
        let routed = match self.audio.lock() {
            Ok(mut guard) => {
                guard.muted = muted;
                guard.sink.clone()
            }
            Err(_poisoned) => None,
        };
        match routed {
            Some(sink) => sink.set_muted(muted),
            None => {
                if let Some(host) = self.host() {
                    host.set_audio_muted(c_int::from(muted));
                }
            }
        }
    }

    fn muted(&self) -> bool {
        // Prefer the mixer-input mute once a sink is attached; otherwise report
        // CEF's device mute.
        match self.audio.lock() {
            Ok(guard) if guard.sink.is_some() => guard.muted,
            _other => self.host().is_some_and(|host| host.is_audio_muted() != 0),
        }
    }

    fn set_audio_sink(&self, sink: Arc<dyn AudioSink>) {
        let muted = match self.audio.lock() {
            Ok(mut guard) => {
                guard.sink = Some(Arc::clone(&sink));
                guard.muted
            }
            Err(_poisoned) => false,
        };
        // Carry any pre-existing mute across the hand-off, and drop CEF's own
        // device mute now that the mixer input owns muting (the handler keeps
        // delivering PCM).
        sink.set_muted(muted);
        if let Some(host) = self.host() {
            host.set_audio_muted(c_int::from(false));
        }
    }

    fn with_new_frame(
        &self,
        seen_generation: &mut u64,
        consumer: &mut dyn FnMut(FrameView<'_>),
    ) -> bool {
        let shared = self.shared.borrow();
        let frame = &shared.frame;
        if frame.generation == *seen_generation || frame.width == 0 || frame.height == 0 {
            return false;
        }
        *seen_generation = frame.generation;
        consumer(FrameView {
            bgra: &frame.bgra,
            width: frame.width,
            height: frame.height,
        });
        true
    }

    fn status(&self) -> SurfaceStatus {
        self.shared.borrow().status.clone()
    }

    fn take_popup_request(&self) -> Option<PopupRequest> {
        self.shared.borrow_mut().status.popup_request.take()
    }

    fn request_close(&self) {
        if let Some(host) = self.host() {
            host.close_browser(1);
        } else {
            let mut shared = self.shared.borrow_mut();
            shared.status.closed = true;
            shared.touch_status();
        }
    }
}

/// The CEF implementation of [`MediaBackend`]. At most one per process; CEF
/// cannot be re-initialised after [`MediaBackend::shutdown`].
pub struct CefMediaBackend {
    /// Keeps the process argv copies alive that were handed to CEF.
    _args: Args,
    /// Weak handles to all surfaces created, for shutdown.
    surfaces: Vec<Weak<RefCell<SurfaceShared>>>,
    /// Whether [`MediaBackend::shutdown`] already ran.
    shut_down: bool,
}

impl std::fmt::Debug for CefMediaBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CefMediaBackend")
            .field("surfaces", &self.surfaces.len())
            .field("shut_down", &self.shut_down)
            .finish()
    }
}

/// How the embedding process handles the signals that ask it to quit, saved
/// before CEF starts and put back after it.
///
/// Chromium's browser start-up installs a graceful-shutdown handler of its own
/// for `SIGTERM` and `SIGINT` — `CefSettings::disable_signal_handlers` does not
/// stop it — which replaced the viewer's: a viewer with web media on exited at
/// once on a `SIGTERM` instead of logging out, and left its session on the grid
/// (found by the end-to-end tier's process backend, 2026-10-01). How the
/// process quits is the embedder's to decide, so whatever it had installed is
/// restored, and the embedder shuts CEF down itself on the way out.
struct EmbedderSignals {
    /// Each signal and the action it had, as `sigaction` reported it.
    saved: Vec<(libc::c_int, libc::sigaction)>,
}

impl EmbedderSignals {
    /// The signals whose handling stays the embedder's.
    const SIGNALS: [libc::c_int; 2] = [libc::SIGTERM, libc::SIGINT];

    /// Read the current action of each of [`Self::SIGNALS`]; one that cannot
    /// be read is left out, and so left as CEF sets it.
    fn save() -> Self {
        let saved = Self::SIGNALS
            .into_iter()
            .filter_map(|signal| {
                // SAFETY: `sigaction` is plain old data, for which all-zero
                // bytes are a valid (empty) value; it is only a buffer here.
                let mut action: libc::sigaction = unsafe { core::mem::zeroed() };
                // SAFETY: a null new action makes `sigaction` only report the
                // current one, into `action`, a valid writable `sigaction`.
                let read = unsafe { libc::sigaction(signal, core::ptr::null(), &raw mut action) };
                (read == 0).then_some((signal, action))
            })
            .collect();
        Self { saved }
    }

    /// Put back each saved action.
    fn restore(&self) {
        for (signal, action) in &self.saved {
            // SAFETY: `action` is exactly what `sigaction` reported for this
            // signal before CEF started — a handler the process installed and
            // still has, or a default — so installing it again is installing a
            // valid action; a null old-action pointer asks for nothing back.
            let restored = unsafe { libc::sigaction(*signal, action, core::ptr::null_mut()) };
            if restored != 0 {
                tracing::warn!(
                    "could not give signal {signal} its handler back after CEF started: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

/// The runtime settings [`CefMediaBackend::initialize`] starts CEF with.
fn cef_settings(config: &BackendConfig) -> Settings {
    let mut settings = Settings {
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        no_sandbox: 1,
        cache_path: path_string(&config.cache_dir),
        root_cache_path: path_string(&config.cache_dir),
        log_file: path_string(&config.cache_dir.join("cef.log")),
        log_severity: LogSeverity::from(cef::sys::cef_log_severity_t::LOGSEVERITY_WARNING),
        ..Settings::default()
    };
    if let Some(subprocess) = &config.subprocess_path {
        settings.browser_subprocess_path = path_string(subprocess);
    }
    if let Some(locale) = &config.locale {
        settings.locale = CefString::from(locale.as_str());
        settings.accept_language_list = CefString::from(locale.as_str());
    }
    if let Some(product) = &config.user_agent_product {
        settings.user_agent_product = CefString::from(product.as_str());
    }
    settings
}

impl CefMediaBackend {
    /// Initialises the global CEF runtime and returns the backend.
    ///
    /// Must be called on the thread that will [`MediaBackend::pump`] — for a
    /// windowed application that is the main/UI thread.
    ///
    /// # Errors
    /// Returns [`MediaError::Init`] if CEF was already initialised in this
    /// process, the cache directory cannot be created, or CEF itself refuses
    /// to initialise.
    pub fn initialize(config: &BackendConfig) -> Result<Self, MediaError> {
        if CEF_INITIALIZED.swap(true, Ordering::SeqCst) {
            return Err(MediaError::Init(String::from(
                "CEF was already initialised in this process",
            )));
        }
        fs_err::create_dir_all(&config.cache_dir)
            .map_err(|error| MediaError::Init(format!("creating the cache dir: {error}")))?;

        let _hash = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        let args = Args::new();

        let settings = cef_settings(config);
        let mut app = OsrApp::new(config.headless);
        let embedder_signals = EmbedderSignals::save();
        let ok = cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        );
        embedder_signals.restore();
        if ok != 1 {
            return Err(MediaError::Init(format!(
                "cef::initialize returned {ok} (exit code {})",
                cef::get_exit_code()
            )));
        }
        tracing::info!(cache_dir = %config.cache_dir.display(), "CEF runtime initialised");
        Ok(Self {
            _args: args,
            surfaces: Vec::new(),
            shut_down: false,
        })
    }

    /// The shared states of all still-live surfaces.
    fn live_shared(&self) -> Vec<StdRc<RefCell<SurfaceShared>>> {
        self.surfaces
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|shared| !shared.borrow().status.closed)
            .collect()
    }
}

impl MediaBackend for CefMediaBackend {
    fn create_surface(
        &mut self,
        config: &SurfaceConfig,
    ) -> Result<Box<dyn MediaSurface>, MediaError> {
        if self.shut_down {
            return Err(MediaError::SurfaceCreation(String::from(
                "the backend is already shut down",
            )));
        }
        let width = i32::try_from(config.width.clamp(1, 8192)).unwrap_or(1);
        let height = i32::try_from(config.height.clamp(1, 8192)).unwrap_or(1);
        let shared = StdRc::new(RefCell::new(SurfaceShared::new(
            width,
            height,
            config.initial_url.as_str(),
        )));
        tracing::debug!(
            surface = shared.borrow().serial,
            url = config.initial_url.as_str(),
            "CEF surface created"
        );

        let audio = Arc::new(Mutex::new(CefAudioState::default()));
        let render = OsrRenderHandler::new(StdRc::clone(&shared));
        let load = OsrLoadHandler::new(StdRc::clone(&shared));
        let display = OsrDisplayHandler::new(StdRc::clone(&shared));
        let life_span = OsrLifeSpanHandler::new(StdRc::clone(&shared));
        let audio_handler = OsrAudioHandler::new(Arc::clone(&audio));
        let request = OsrRequestHandler::new(StdRc::clone(&shared));
        let mut client = OsrClient::new(render, load, display, life_span, audio_handler, request);

        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            ..WindowInfo::default()
        };
        let browser_settings = browser_settings(config.trust, config.max_fps);
        let mut request_context: Option<RequestContext> = if config.trust.is_isolated() {
            request_context_create_context(Some(&RequestContextSettings::default()), None)
        } else {
            None
        };

        let url = CefString::from(config.initial_url.as_str());
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url),
            Some(&browser_settings),
            None,
            request_context.as_mut(),
        )
        .ok_or_else(|| {
            MediaError::SurfaceCreation(String::from("browser_host_create_browser_sync failed"))
        })?;

        shared.borrow_mut().browser = Some(browser);
        let surface = CefMediaSurface {
            shared: StdRc::clone(&shared),
            audio,
        };
        if config.muted {
            surface.set_muted(true);
        }
        self.surfaces.push(StdRc::downgrade(&shared));
        Ok(Box::new(surface))
    }

    fn pump(&mut self) {
        cef::do_message_loop_work();
        self.surfaces.retain(|weak| weak.upgrade().is_some());
    }

    fn live_surfaces(&self) -> usize {
        self.live_shared().len()
    }

    fn shutdown(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        for shared in self.live_shared() {
            let browser = shared.borrow().browser.clone();
            if let Some(host) = browser.and_then(|browser| browser.host()) {
                host.close_browser(1);
            }
        }
        // Pump until every browser reported on_before_close (bounded).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while self.live_surfaces() > 0 && std::time::Instant::now() < deadline {
            cef::do_message_loop_work();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if self.live_surfaces() > 0 {
            tracing::warn!(
                live = self.live_surfaces(),
                "CEF browsers still alive at shutdown; skipping cef::shutdown"
            );
            return;
        }
        // Release the last strong browser handles before tearing CEF down.
        for weak in &self.surfaces {
            if let Some(shared) = weak.upgrade() {
                shared.borrow_mut().browser = None;
            }
        }
        cef::do_message_loop_work();
        cef::shutdown();
        tracing::info!("CEF runtime shut down");
    }

    fn set_shared_cookie(&mut self, cookie: &SharedCookie) -> Result<(), MediaError> {
        if self.shut_down {
            return Err(MediaError::Cookie(String::from("the backend is shut down")));
        }
        // The global cookie manager backs the *shared* (global) request
        // context — the one non-isolated surfaces are created against
        // (`create_surface` passes `None` for those). Isolated in-world
        // surfaces have their own contexts and are never reached here.
        let manager = cookie_manager_get_global_manager(None)
            .ok_or_else(|| MediaError::Cookie(String::from("global cookie manager unavailable")))?;
        let cef_cookie = Cookie {
            name: CefString::from(cookie.name.as_str()),
            value: CefString::from(cookie.value.as_str()),
            domain: CefString::from(cookie.domain.as_str()),
            path: CefString::from(cookie.path.as_str()),
            secure: c_int::from(cookie.secure),
            httponly: c_int::from(cookie.http_only),
            // A session cookie: no expiry, cleared when the store is cleared.
            has_expires: 0,
            ..Cookie::default()
        };
        let url = CefString::from(cookie.url.as_str());
        // The set is queued onto CEF's IO thread; passing no callback is fine
        // since a following navigation reads the store after it is applied.
        if manager.set_cookie(Some(&url), Some(&cef_cookie), None) == 1 {
            Ok(())
        } else {
            Err(MediaError::Cookie(format!(
                "set_cookie rejected {} for {}",
                cookie.name, cookie.url
            )))
        }
    }

    fn clear_shared_cookies(&mut self) -> Result<(), MediaError> {
        if self.shut_down {
            return Ok(());
        }
        let manager = cookie_manager_get_global_manager(None)
            .ok_or_else(|| MediaError::Cookie(String::from("global cookie manager unavailable")))?;
        // `delete_cookies(None, None, ..)` deletes every cookie in the store;
        // `flush_store` persists the deletion before the process may exit.
        let _deleted = manager.delete_cookies(None, None, None);
        let _flushed = manager.flush_store(None);
        Ok(())
    }
}

impl Drop for CefMediaBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use cef::State;
    use pretty_assertions::assert_eq;

    use super::{
        DocumentEvent, apply_document_event, browser_settings, document_event, frame_tree_document,
        superseded_navigation, utf16_offset,
    };
    use crate::SurfaceTrust;

    /// The main frame's navigation is a document's arrival, a subframe's is
    /// not; its first contentful paint and its network falling idle are its
    /// presentation, and the other lifecycle steps and events are nothing.
    #[test]
    fn page_events_are_read_as_the_main_frames_document() {
        let arrived =
            br#"{"frame":{"id":"F","loaderId":"L1","url":"http://a/"},"type":"Navigation"}"#;
        assert_eq!(
            document_event("Page.frameNavigated", arrived),
            Some(DocumentEvent::Arrived {
                loader: "L1".to_owned()
            })
        );
        let subframe = br#"{"frame":{"id":"S","parentId":"F","loaderId":"L2","url":"http://b/"}}"#;
        assert_eq!(document_event("Page.frameNavigated", subframe), None);
        for name in ["firstContentfulPaint", "networkIdle"] {
            let params =
                format!(r#"{{"frameId":"F","loaderId":"L1","name":"{name}","timestamp":1.5}}"#);
            assert_eq!(
                document_event("Page.lifecycleEvent", params.as_bytes()),
                Some(DocumentEvent::Presented {
                    loader: "L1".to_owned()
                }),
                "{name}"
            );
        }
        for name in ["init", "DOMContentLoaded", "load", "firstPaint"] {
            let params =
                format!(r#"{{"frameId":"F","loaderId":"L1","name":"{name}","timestamp":1.5}}"#);
            assert_eq!(
                document_event("Page.lifecycleEvent", params.as_bytes()),
                None,
                "{name}"
            );
        }
        assert_eq!(
            document_event("Page.loadEventFired", br#"{"timestamp":1.5}"#),
            None
        );
        assert_eq!(document_event("Page.frameNavigated", b"not json"), None);
    }

    /// A document is presented only by its own presentation: a new arrival
    /// clears it, and the previous document's late paint does not set it.
    #[test]
    fn only_the_current_document_presents() {
        let arrived = |loader: &str| DocumentEvent::Arrived {
            loader: loader.to_owned(),
        };
        let shown = |loader: &str| DocumentEvent::Presented {
            loader: loader.to_owned(),
        };
        let mut loader = None;
        let mut presented = false;
        assert!(!apply_document_event(
            &mut loader,
            &mut presented,
            shown("L1")
        ));
        assert!(!presented);
        assert!(!apply_document_event(
            &mut loader,
            &mut presented,
            arrived("L1")
        ));
        assert!(apply_document_event(
            &mut loader,
            &mut presented,
            shown("L1")
        ));
        assert!(presented);
        assert!(!apply_document_event(
            &mut loader,
            &mut presented,
            shown("L1")
        ));
        assert!(apply_document_event(
            &mut loader,
            &mut presented,
            arrived("L2")
        ));
        assert!(!presented);
        assert!(!apply_document_event(
            &mut loader,
            &mut presented,
            shown("L1")
        ));
        assert!(!presented);
        assert!(apply_document_event(
            &mut loader,
            &mut presented,
            shown("L2")
        ));
        assert!(presented);
    }

    /// The frame tree names the document already there when the observer
    /// attached, whose replayed paint then presents it; it does not undo an
    /// arrival seen before its answer came.
    #[test]
    fn the_document_already_there_is_followed_from_the_frame_tree() {
        let tree = br#"{"frameTree":{"frame":{"id":"F","loaderId":"L0","url":"http://a/"},"childFrames":[{"frame":{"id":"S","parentId":"F","loaderId":"LS","url":"http://b/"}}]}}"#;
        let current = frame_tree_document(tree);
        assert_eq!(
            current,
            Some(DocumentEvent::Current {
                loader: "L0".to_owned()
            })
        );
        assert_eq!(frame_tree_document(b"{}"), None);
        let shown = |loader: &str| DocumentEvent::Presented {
            loader: loader.to_owned(),
        };
        let mut loader = None;
        let mut presented = false;
        let current = DocumentEvent::Current {
            loader: "L0".to_owned(),
        };
        assert!(!apply_document_event(
            &mut loader,
            &mut presented,
            current.clone()
        ));
        assert!(apply_document_event(
            &mut loader,
            &mut presented,
            shown("L0")
        ));
        let mut loader = Some("L1".to_owned());
        let mut presented = false;
        assert!(!apply_document_event(&mut loader, &mut presented, current));
        assert_eq!(loader.as_deref(), Some("L1"));
        assert!(!apply_document_event(
            &mut loader,
            &mut presented,
            shown("L0")
        ));
    }

    /// The address typed into a window wins over the old page's own client
    /// redirect (the home page's OpenID hop), while the asked-for page, a
    /// server redirect and a link the user clicks all go ahead.
    #[test]
    fn an_asked_for_address_supersedes_the_old_pages_own_navigations() {
        let typed = "http://127.0.0.1:4000/browser";
        let hop = "https://secondlife.com/?openid_identifier=anonymous";
        assert!(superseded_navigation(typed, hop, false, false));
        assert!(!superseded_navigation(typed, typed, false, false));
        assert!(!superseded_navigation(typed, hop, false, true));
        assert!(!superseded_navigation(typed, hop, true, false));
    }

    /// The asked-for page is recognised however Chromium spells it back: a
    /// bare origin gains its root path, a host its lower case.
    #[test]
    fn the_asked_for_address_is_matched_normalised() {
        assert!(!superseded_navigation(
            "https://Example.com",
            "https://example.com/",
            false,
            false
        ));
        assert!(superseded_navigation(
            "https://example.com/a",
            "https://example.com/b",
            false,
            false
        ));
    }

    /// winit's byte offsets become CEF's UTF-16 ones: a CJK character is three
    /// bytes and one unit, an astral emoji four bytes and two units, and an
    /// offset past the end (or mid-character) is the end.
    #[test]
    fn ime_offsets_are_converted_to_utf16() {
        let text = "日本😀x";
        assert_eq!(utf16_offset(text, 0), 0);
        assert_eq!(utf16_offset(text, 3), 1);
        assert_eq!(utf16_offset(text, 6), 2);
        assert_eq!(utf16_offset(text, 10), 4);
        assert_eq!(utf16_offset(text, 11), 5);
        assert_eq!(utf16_offset(text, 99), 5);
        assert_eq!(utf16_offset(text, 1), 5);
    }

    /// An in-world page reaches nothing outside itself: it cannot close the
    /// surface, touch the clipboard, or keep storage between navigations.
    #[test]
    fn in_world_surfaces_lose_the_reaching_out_capabilities() {
        let settings = browser_settings(SurfaceTrust::InWorld, 15);
        assert_eq!(settings.javascript_close_windows, State::DISABLED);
        assert_eq!(settings.javascript_access_clipboard, State::DISABLED);
        assert_eq!(settings.javascript_dom_paste, State::DISABLED);
        assert_eq!(settings.local_storage, State::DISABLED);
        assert_eq!(settings.databases_deprecated, State::DISABLED);
    }

    /// …but it is still a usable media page: script, images, fonts and WebGL
    /// stay on, as they do in the reference viewer's media plugin.
    #[test]
    fn in_world_surfaces_keep_what_media_needs() {
        let settings = browser_settings(SurfaceTrust::InWorld, 15);
        assert_eq!(settings.javascript, State::ENABLED);
        assert_eq!(settings.image_loading, State::ENABLED);
        assert_eq!(settings.remote_fonts, State::ENABLED);
        assert_eq!(settings.webgl, State::ENABLED);
    }

    /// A trusted panel keeps every capability — a persisted login needs
    /// `local_storage`, a copy button needs the clipboard.
    #[test]
    fn viewer_surfaces_keep_every_capability() {
        let settings = browser_settings(SurfaceTrust::Viewer, 30);
        assert_eq!(settings.javascript_close_windows, State::ENABLED);
        assert_eq!(settings.javascript_access_clipboard, State::ENABLED);
        assert_eq!(settings.javascript_dom_paste, State::ENABLED);
        assert_eq!(settings.local_storage, State::ENABLED);
        assert_eq!(settings.databases_deprecated, State::ENABLED);
    }

    /// Trust decides capabilities and nothing else: the paint rate and the
    /// opaque white background are the same either way, and the rate is
    /// clamped into CEF's 1–60 range.
    #[test]
    fn trust_does_not_disturb_the_paint_settings() {
        for trust in [SurfaceTrust::Viewer, SurfaceTrust::InWorld] {
            let settings = browser_settings(trust, 15);
            assert_eq!(settings.windowless_frame_rate, 15);
            assert_eq!(settings.background_color, 0xFFFF_FFFF);
            assert_eq!(browser_settings(trust, 0).windowless_frame_rate, 1);
            assert_eq!(browser_settings(trust, 240).windowless_frame_rate, 60);
        }
    }

    /// The trust value the config carries is the isolation decision too — an
    /// in-world surface gets its own request context, a viewer panel shares
    /// the one the grid's web-session cookie lives in.
    #[test]
    fn only_in_world_surfaces_are_isolated() {
        assert!(SurfaceTrust::InWorld.is_isolated());
        assert!(!SurfaceTrust::Viewer.is_isolated());
    }
}
