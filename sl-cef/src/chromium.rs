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
use std::sync::atomic::{AtomicBool, Ordering};

use cef::args::Args;
use cef::rc::Rc as _;
use cef::{
    App, Browser, BrowserHost, BrowserSettings, CefString, Client, CommandLine, CursorInfo,
    CursorType, DisplayHandler, Errorcode, Frame, ImplApp, ImplBrowser, ImplBrowserHost,
    ImplClient, ImplCommandLine, ImplDisplayHandler, ImplFrame, ImplLifeSpanHandler,
    ImplLoadHandler, ImplRenderHandler, KeyEvent, KeyEventType, LifeSpanHandler, LoadHandler,
    LogSeverity, MouseButtonType, MouseEvent, PaintElementType, PopupFeatures, Rect, RenderHandler,
    RequestContext, RequestContextSettings, ScreenInfo, Settings, WindowInfo,
    WindowOpenDisposition, WrapApp, WrapClient, WrapDisplayHandler, WrapLifeSpanHandler,
    WrapLoadHandler, WrapRenderHandler, browser_host_create_browser_sync,
    request_context_create_context, wrap_app, wrap_client, wrap_display_handler,
    wrap_life_span_handler, wrap_load_handler, wrap_render_handler,
};

use crate::{
    BackendConfig, CursorKind, FrameView, KeyInput, MediaBackend, MediaError, MediaSurface,
    Modifiers, MouseButton, SurfaceConfig, SurfaceStatus,
};

/// Whether the global CEF runtime was initialised in this process. CEF can
/// only ever be initialised once per process (and cannot be re-initialised
/// after shutdown).
static CEF_INITIALIZED: AtomicBool = AtomicBool::new(false);

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
            _browser: Option<&mut Browser>,
            is_loading: c_int,
            can_go_back: c_int,
            can_go_forward: c_int,
        ) {
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
            let mut shared = self.shared.borrow_mut();
            shared.browser = browser.map(|b| b.clone());
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            let mut shared = self.shared.borrow_mut();
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
            _target_frame_name: Option<&CefString>,
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
                shared.status.popup_request = Some(url.to_string());
                shared.touch_status();
            }
            1
        }
    }
}

wrap_client! {
    // The per-surface CEF client wiring the handlers above together.
    struct OsrClient {
        render: RenderHandler,
        load: LoadHandler,
        display: DisplayHandler,
        life_span: LifeSpanHandler,
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
    }
}

wrap_app! {
    // The browser-process application: injects command-line switches before
    // Chromium parses them.
    struct OsrApp {}
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

/// One offscreen CEF browser surface (see [`MediaSurface`]).
pub struct CefMediaSurface {
    /// State shared with the CEF handler callbacks.
    shared: StdRc<RefCell<SurfaceShared>>,
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
    fn navigate(&self, url: &str) {
        if let Some(frame) = self.browser().and_then(|browser| browser.main_frame()) {
            frame.load_url(Some(&CefString::from(url)));
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
        self.shared.borrow_mut().last_mouse = (x, y);
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

    fn set_max_fps(&self, fps: u8) {
        if let Some(host) = self.host() {
            host.set_windowless_frame_rate(i32::from(fps.clamp(1, 60)));
        }
    }

    fn set_muted(&self, muted: bool) {
        if let Some(host) = self.host() {
            host.set_audio_muted(c_int::from(muted));
        }
    }

    fn muted(&self) -> bool {
        self.host().is_some_and(|host| host.is_audio_muted() != 0)
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

    fn take_popup_request(&self) -> Option<String> {
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

        let mut app = OsrApp::new();
        let ok = cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        );
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
            &config.initial_url,
        )));

        let render = OsrRenderHandler::new(StdRc::clone(&shared));
        let load = OsrLoadHandler::new(StdRc::clone(&shared));
        let display = OsrDisplayHandler::new(StdRc::clone(&shared));
        let life_span = OsrLifeSpanHandler::new(StdRc::clone(&shared));
        let mut client = OsrClient::new(render, load, display, life_span);

        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            ..WindowInfo::default()
        };
        let browser_settings = BrowserSettings {
            windowless_frame_rate: i32::from(config.max_fps.clamp(1, 60)),
            background_color: 0xFFFF_FFFF,
            ..BrowserSettings::default()
        };
        let mut request_context: Option<RequestContext> = if config.isolated {
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
}

impl Drop for CefMediaBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}
