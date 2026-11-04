//! The in-viewer **web browser floater** (`viewer-media-prim-browser`, UI
//! half): the reference viewer's `floater_web_content` — a navigation
//! toolbar (back / forward / stop-or-reload / address bar / secure-lock /
//! open-external), the embedded browser view ([`crate::browser_widget`]),
//! and a status row (status text + load progress).
//!
//! Opened from **Content ▸ Web Browser** (the viewer binary's `menu_bar`) or by
//! writing an [`OpenWebBrowser`] message (other floaters route links here). Runs in
//! the **shared** (trusted-UI) request context so web logins persist across
//! pages, unlike in-world media surfaces which are isolated.
//!
//! # One window per page, as the reference keeps them
//!
//! `LLFloaterWebContent` is a keyed instance, not a singleton and not a tab
//! strip: `LLWeb::loadURLInternal` gives every page a fresh id, so a menu
//! entry, a chat link and a script's `llLoadURL` each open a window of their
//! own, and `matchesKey` reuses a window only when a page *names* one (a
//! `target="name"` link). `WebContentWindowLimit` (5) caps how many are open,
//! closing the least recently opened. This module does the same: an
//! [`OpenWebBrowser`] carries a [`WebTarget`], a window is keyed by it
//! ([`web_window_key`]), a page's popup opens through the same message with
//! the target the page named, and [`SETTING_WEB_WINDOW_LIMIT`] is the cap.
//! Every window has its own embedded view, so its own history, address and
//! title; closing one ends its view and leaves the others alone.

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui_widgets::Activate;
use bevy_flair::style::components::ClassList;

use crate::browser_widget::{
    BrowserView, BrowserViewSpec, SurfaceTrust, ValidatedMediaUrl, spawn_browser_view,
};
use crate::media_engine::{MediaEngineSystems, MediaSurfaces};
use sl_cef::SurfaceStatus;
use sl_settings::SettingValue;
use sl_viewer_intents::{OpenWebBrowser, WebTarget};
use sl_viewer_platform::system_browser::{ExternalUrl, normalize_web_url, open_in_system_browser};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::glyph;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin::{DISABLED_TEXT_CLASS, role_class, set_state_class_on, text_role};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{UiPanelShown, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_core::ui_text::set_editor_text;
use sl_viewer_ui_widgets::floater::{
    FloaterCaps, FloaterCommand, FloaterHandle, FloaterHost, FloaterKey, FloaterOp, FloaterSpec,
    FloaterSystems, KeyedFloaterOpen, KeyedFloaters,
};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The web-browser floater's [`sl_viewer_ui_widgets::floater::Floater::id`] —
/// the **kind** every browser window shares; which window is which comes from
/// its [`FloaterKey`] ([`web_window_key`]).
pub const WEB_FLOATER_ID: &str = "web-browser";

/// The settings section the browser's settings persist under.
const WEB_SETTINGS_SECTION: &[&str] = &["media"];

/// How many web browser windows may be open at once; opening one more closes
/// the least recently opened. 0 is no limit. The reference's
/// `WebContentWindowLimit`, under its name and with its default.
pub const SETTING_WEB_WINDOW_LIMIT: &str = "WebContentWindowLimit";

/// [`SETTING_WEB_WINDOW_LIMIT`]'s default — the reference's.
const DEFAULT_WEB_WINDOW_LIMIT: u32 = 5;

/// The page a fresh floater opens on.
const DEFAULT_HOME_URL: &str = "https://secondlife.com/";

/// The toolbar / status font size.
const WEB_FONT_SIZE: f32 = 13.0;

/// The status line — the muted role.
const STATUS_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// One browser window's own parts, on its floater root: what the toolbar,
/// the address bar and the status sync reach *their* window's view through.
#[derive(Component, Debug, Clone, Copy)]
pub struct WebWindow {
    /// The title-bar text (bound to the page title).
    title_text: Entity,
    /// The content the status is drawn into.
    parts: WebContentParts,
    /// When the window was opened, in [`WebWindowSeq`] order — the limit
    /// closes the lowest.
    opened: u64,
}

impl WebWindow {
    /// The window's embedded browser view.
    #[must_use]
    pub const fn view(&self) -> Entity {
        self.parts.view
    }
}

/// The next window's sequence number: both the order windows were opened in
/// (for the limit) and what makes a [`WebTarget::New`] key unique.
#[derive(Debug, Default, Resource)]
struct WebWindowSeq(u64);

/// The web floater's content entities: what [`spawn_web_content`] returns
/// and [`draw_web_status`] draws a status snapshot into. Shared by the live
/// floater and its gallery specimen.
#[derive(Debug, Clone, Copy)]
struct WebContentParts {
    /// The embedded browser view.
    view: Entity,
    /// The address field.
    address: Entity,
    /// The back button's label (dimmed when history is empty).
    back_label: Entity,
    /// The forward button's label.
    forward_label: Entity,
    /// The stop-or-reload button's label (⟳ while idle, ✕ while loading).
    reload_label: Entity,
    /// The secure-lock glyph (shown for `https://`).
    lock: Entity,
    /// The status-row text.
    status_text: Entity,
}

/// The web browser floater plugin.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebFloaterPlugin;

impl Plugin for WebFloaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenWebBrowser>()
            .add_message::<FloaterCommand>()
            .init_resource::<WebWindowSeq>()
            .add_systems(
                Update,
                (open_web_browser, sync_web_floater)
                    .chain()
                    .after(MediaEngineSystems::Pump)
                    // A link clicked inside another window both raises that
                    // window and opens this one; opening after the command
                    // pass keeps the new window on top (`FloaterSystems`).
                    .after(FloaterSystems::Commands),
            );
    }
}

/// Register the browser's settings — the window limit.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        WEB_SETTINGS_SECTION,
        SETTING_WEB_WINDOW_LIMIT,
        SettingValue::U32(DEFAULT_WEB_WINDOW_LIMIT),
        "setting-desc-WebContentWindowLimit",
    );
}

/// The window limit as configured, or `None` for no limit (a 0).
fn window_limit(settings: Option<&ViewerSettings>) -> Option<usize> {
    let limit = settings
        .and_then(|settings| settings.store().get_u32(SETTING_WEB_WINDOW_LIMIT).ok())
        .unwrap_or(DEFAULT_WEB_WINDOW_LIMIT);
    usize::try_from(limit).ok().filter(|limit| *limit > 0)
}

/// The web floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn web_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: WEB_FLOATER_ID,
        title: String::from("Web Browser"),
        position: Vec2::new(160.0, 90.0),
        default_size: Some(Vec2::new(760.0, 520.0)),
        min_size: Some(Vec2::new(420.0, 300.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: true,
        },
    }
}

/// The key a page opening in `target` finds its window under: a named target
/// always the same window, a new one a key no other window has (`seq` is the
/// sequence number the window would be opened under).
///
/// The two are prefixed apart so a page cannot name a target that collides
/// with a fresh window's key.
#[must_use]
pub fn web_window_key(target: &WebTarget, seq: u64) -> FloaterKey {
    match target {
        WebTarget::New => FloaterKey::subject(&format!("new-{seq}")),
        WebTarget::Named(name) => FloaterKey::subject(&format!("target-{name}")),
    }
}

/// A window opened this frame, whose content is built once every request has
/// been read — so a second request for the same named target in the same
/// frame changes the page it opens on instead of opening a second window.
#[derive(Debug)]
struct FreshWindow {
    /// The key it was opened under.
    key: FloaterKey,
    /// Its chrome.
    handle: FloaterHandle,
    /// The page it opens on.
    url: ValidatedMediaUrl,
    /// Its place in the open order.
    opened: u64,
}

/// Open a page per [`OpenWebBrowser`] message (menu, links, popups): in the
/// window its target names when that window is up, else in a new window —
/// closing the least recently opened first when the limit is reached.
#[expect(
    clippy::too_many_arguments,
    reason = "the open path needs the keyed manager, the windows and views it may reuse, the \
              settings for the limit, the close it may issue and the commands that build a new \
              window; bundling them would only rename the list"
)]
fn open_web_browser(
    mut requests: MessageReader<OpenWebBrowser>,
    mut floaters: KeyedFloaters,
    mut seq: ResMut<WebWindowSeq>,
    windows: Query<(Entity, &WebWindow)>,
    mut views: Query<&mut BrowserView>,
    surfaces: Option<NonSend<MediaSurfaces>>,
    settings: Option<Res<ViewerSettings>>,
    mut closes: MessageWriter<FloaterCommand>,
    mut commands: Commands,
) {
    let mut fresh: Vec<FreshWindow> = Vec::new();
    let mut closing: Vec<Entity> = Vec::new();
    let limit = window_limit(settings.as_deref());
    for request in requests.read() {
        let url = match &request.url {
            Some(text) => match validated_web_url(text) {
                Ok(url) => Some(url),
                // Refused and logged: there is no page to open a window on.
                Err(_refused) => continue,
            },
            None => None,
        };
        let key = web_window_key(&request.target, seq.0);
        if let Some(window) = fresh.iter_mut().find(|window| window.key == key) {
            if let Some(url) = url {
                window.url = url;
            }
            continue;
        }
        if floaters.instance(WEB_FLOATER_ID, &key).is_none()
            && let Some(limit) = limit
        {
            make_room(limit, &windows, &fresh, &mut closing, &mut closes);
        }
        match floaters.open(web_floater_spec(), key.clone()) {
            KeyedFloaterOpen::Spawned(handle) => {
                fresh.push(FreshWindow {
                    key,
                    handle,
                    url: url.unwrap_or_else(home_url),
                    opened: seq.0,
                });
                seq.0 = seq.0.wrapping_add(1);
            }
            KeyedFloaterOpen::Existing(root) => {
                if let Some(url) = url
                    && let Ok((_root, window)) = windows.get(root)
                    && let Ok(mut view) = views.get_mut(window.parts.view)
                {
                    navigate(&mut view, surfaces.as_deref(), url);
                }
            }
        }
    }
    for window in fresh {
        build_web_window(&mut commands, window);
    }
}

/// Close the least recently opened windows until one more fits under
/// `limit`. Windows opened this frame count, and are the newest.
fn make_room(
    limit: usize,
    windows: &Query<(Entity, &WebWindow)>,
    fresh: &[FreshWindow],
    closing: &mut Vec<Entity>,
    closes: &mut MessageWriter<FloaterCommand>,
) {
    let mut open: Vec<(u64, Entity)> = windows
        .iter()
        .map(|(root, window)| (window.opened, root))
        .chain(
            fresh
                .iter()
                .map(|window| (window.opened, window.handle.root)),
        )
        .filter(|(_opened, root)| !closing.contains(root))
        .collect();
    open.sort_unstable();
    let excess = open.len().saturating_add(1).saturating_sub(limit);
    for (_opened, root) in open.into_iter().take(excess) {
        closes.write(FloaterCommand {
            floater: root,
            op: FloaterOp::CloseNow,
        });
        closing.push(root);
    }
}

/// Point `view` at `url`: navigate its surface when it has one, or change the
/// page it will open on when its surface is not created yet (a window opened
/// a frame ago, still waiting for its first layout).
fn navigate(view: &mut BrowserView, surfaces: Option<&MediaSurfaces>, url: ValidatedMediaUrl) {
    match view.surface {
        Some(id) => {
            if let Some(slot) = surfaces.and_then(|surfaces| surfaces.get(id)) {
                slot.surface.navigate(&url);
            }
        }
        None => view.initial_url = url,
    }
}

/// The home page a window opened without a URL shows.
fn home_url() -> ValidatedMediaUrl {
    validated_web_url(DEFAULT_HOME_URL).unwrap_or_else(|_refused| ValidatedMediaUrl::blank())
}

/// Build a just-opened window's content and give it its [`WebWindow`].
fn build_web_window(commands: &mut Commands, window: FreshWindow) {
    let FreshWindow {
        handle,
        url,
        opened,
        ..
    } = window;
    commands
        .entity(handle.title_text)
        .insert(Translated::new("web-floater-title"));
    let parts = spawn_web_content(commands, handle.content, WEB_FONT_SIZE, url);
    commands.entity(handle.root).insert(WebWindow {
        title_text: handle.title_text,
        parts,
        opened,
    });
}

// ---------------------------------------------------------------------------
// Gallery specimen
// ---------------------------------------------------------------------------

/// The web floater's gallery / `ui_test` specimen: the live content, built by
/// the same `spawn_web_content` at the cell's font size, with a sample page
/// status drawn into the chrome by the same `draw_web_status` the live sync
/// uses — a secure page mid-load, with history behind it and none ahead, so
/// the lock, the loading mark, a greyed Forward and the status row all show.
///
/// The view opens an offline `data:` page rather than the live home page, so
/// neither the gallery nor the sweep touches the network; without the media
/// engine it stays the dark placeholder.
pub fn spawn_web_floater_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let parts = spawn_web_content(
        commands,
        parent,
        cx.font_size,
        crate::browser_widget::specimen_page_url(&cx.text("Sample Page")),
    );
    let status = SurfaceStatus {
        url: String::from("https://example.com/sample/page.html"),
        title: cx.text("Sample Page"),
        loading: true,
        can_go_back: true,
        can_go_forward: false,
        progress: 0.42,
        ..SurfaceStatus::default()
    };
    commands.queue(move |world: &mut World| {
        if let Err(error) = world.run_system_cached_with(draw_web_status_system, (parts, status)) {
            warn!("web floater specimen: the sample status was not drawn: {error}");
        }
    });
    parent
}

/// [`draw_web_status`] as a one-shot system, for the specimen (which holds
/// only [`Commands`]). Nothing is being edited in a specimen.
fn draw_web_status_system(
    In((parts, status)): In<(WebContentParts, SurfaceStatus)>,
    mut chrome: WebChrome,
) {
    draw_web_status(&parts, &status, false, &mut chrome);
}

/// Build the web floater's content into `content` at `font_size`: the
/// toolbar, the browser view opening `initial_url`, and the status row.
/// Shared by the live floater and its specimen.
fn spawn_web_content(
    commands: &mut Commands,
    content: Entity,
    font_size: f32,
    initial_url: ValidatedMediaUrl,
) -> WebContentParts {
    let content = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                ..column(Val::Px(4.0))
            },
            ChildOf(content),
        ))
        .id();

    // Toolbar: ◀ ▶ ⟳ [lock][address............] ↗
    let toolbar = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(4.0))
            },
            ChildOf(content),
        ))
        .id();
    let back_label = spawn_toolbar_button(commands, toolbar, glyph::BACK, "back", 1, font_size);
    let forward_label =
        spawn_toolbar_button(commands, toolbar, glyph::FORWARD, "forward", 2, font_size);
    let reload_label = spawn_toolbar_button(
        commands,
        toolbar,
        glyph::RELOAD,
        "reload-or-stop",
        3,
        font_size,
    );
    let lock = commands
        .spawn((
            glyph::glyph_host(
                glyph::SECURE,
                UiFont::Sans.at(font_size),
                role_class(STATUS_COLOR),
            ),
            TextColor(STATUS_COLOR),
            Visibility::Hidden,
            ChildOf(toolbar),
        ))
        .id();
    let address = spawn_text_input(
        commands,
        toolbar,
        &TextInputSpec {
            initial: String::new(),
            font_size,
            width_glyphs: 40.0,
            tab_index: 4,
            max_characters: Some(1024),
            fill: true,
            name_key: Some("web-browser-address-name"),
            ..TextInputSpec::new("web-address", TextInputKind::Line)
        },
    );
    commands.entity(address).observe(on_address_key);
    let _external = spawn_toolbar_button(
        commands,
        toolbar,
        glyph::EXTERNAL,
        "open-external",
        5,
        font_size,
    );

    // The page itself.
    let view = spawn_browser_view(
        commands,
        content,
        &BrowserViewSpec {
            initial_url,
            trust: SurfaceTrust::Viewer,
            tab_index: 6,
            fixed_height: None,
        },
    );

    // Status row.
    let status_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            text_role(STATUS_COLOR),
            ChildOf(content),
        ))
        .id();

    WebContentParts {
        view,
        address,
        back_label,
        forward_label,
        reload_label,
        lock,
        status_text,
    }
}

/// One glyph toolbar button acting on the view of the window it is in;
/// returns the label entity (recoloured for enablement), at `font_size`.
fn spawn_toolbar_button(
    commands: &mut Commands,
    parent: Entity,
    slot: &'static str,
    action: &'static str,
    tab_index: i32,
    font_size: f32,
) -> Entity {
    let spawned = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::glyph(slot, toolbar_button_name_key(action)),
            format!("web-browser-button:{action}"),
        )
        .kind(ButtonKind::Headless)
        .tab_index(tab_index)
        .padding(7.0, 3.0)
        .font_size(font_size),
    );
    commands.entity(spawned.button).observe(
        move |activate: On<Activate>,
              host: FloaterHost,
              windows: Query<&WebWindow>,
              views: Query<&BrowserView>,
              surfaces: Option<NonSend<MediaSurfaces>>| {
            let slot = host
                .of(activate.entity)
                .and_then(|root| windows.get(root).ok())
                .and_then(|window| views.get(window.parts.view).ok())
                .and_then(|view| view.surface)
                .and_then(|id| surfaces.as_deref()?.get(id));
            if let Some(slot) = slot {
                toolbar_action(action, &*slot.surface, &slot.status);
            }
        },
    );
    spawned.label
}

/// Carry out the toolbar button `action` on one window's surface.
fn toolbar_action(action: &str, surface: &dyn sl_cef::MediaSurface, status: &SurfaceStatus) {
    match action {
        "back" => surface.go_back(),
        "forward" => surface.go_forward(),
        "reload-or-stop" => {
            if status.loading {
                surface.stop();
            } else {
                surface.reload();
            }
        }
        // The page's *current* URL, which the page chose by navigating:
        // remote data, filtered at the sink like any other.
        "open-external" => {
            if let Ok(url) = ExternalUrl::parse(&status.url).inspect_err(|error| {
                warn!("web page not opened in the system browser: {error}");
            }) {
                open_in_system_browser(&url);
            }
        }
        _ => {}
    }
}

/// The Fluent key naming the toolbar button that reports `action`.
fn toolbar_button_name_key(action: &'static str) -> &'static str {
    match action {
        "back" => "web-browser-back-name",
        "forward" => "web-browser-forward-name",
        "reload-or-stop" => "web-browser-reload-or-stop-name",
        "open-external" => "web-browser-open-external-name",
        _ => "",
    }
}

/// `Enter` in the address field navigates its window's view to the typed URL.
fn on_address_key(
    event: On<FocusedInput<KeyboardInput>>,
    editors: Query<&EditableText>,
    host: FloaterHost,
    windows: Query<&WebWindow>,
    mut views: Query<&mut BrowserView>,
    surfaces: Option<NonSend<MediaSurfaces>>,
) {
    if !event.input.state.is_pressed() || event.input.key_code != KeyCode::Enter {
        return;
    }
    let field = event.focused_entity;
    let Ok(editor) = editors.get(field) else {
        return;
    };
    let Some(url) = normalize_web_url(&editor.value().to_string()) else {
        return;
    };
    let Ok(url) = validated_web_url(&url) else {
        return;
    };
    if let Some(window) = host.of(field).and_then(|root| windows.get(root).ok())
        && let Ok(mut view) = views.get_mut(window.parts.view)
    {
        navigate(&mut view, surfaces.as_deref(), url);
    }
}

/// Check a URL against the media scheme allowlist before the floater's surface
/// sees it, reporting a refusal.
///
/// The address bar and the [`OpenWebBrowser`] message both land here, and
/// neither is only fed by the user: a `secondlife:///` link in chat, a profile
/// field and a page's own popup request all open this floater, so the filter
/// applies to the whole floater rather than to its grid-sourced callers alone.
fn validated_web_url(text: &str) -> Result<ValidatedMediaUrl, sl_cef::MediaUrlError> {
    ValidatedMediaUrl::parse(text)
        .inspect_err(|error| warn!("web floater did not open a URL: {error}"))
}

/// A browser window's chrome, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam) — one query per piece
/// updated, plus the two text contexts a rewritten address bar is
/// re-laid-out through.
#[derive(bevy::ecs::system::SystemParam)]
struct WebChrome<'w, 's> {
    /// The status row and the button glyphs.
    texts: Query<'w, 's, &'static mut Text>,
    /// The class lists a greyed Back / Forward is written through.
    classes: Query<'w, 's, &'static mut ClassList>,
    /// The address bar, rewritten while it is not being edited.
    editors: Query<'w, 's, &'static mut EditableText>,
    /// The secure lock, shown only for a secure page.
    visibilities: Query<'w, 's, &'static mut Visibility>,
    /// The font context a rewritten address bar is re-laid-out through.
    font_cx: ResMut<'w, FontCx>,
    /// Its layout context.
    layout_cx: ResMut<'w, LayoutCx>,
}

/// Mirror each shown window's view status into its chrome: title, address
/// (unless being edited), back/forward enablement, stop-vs-reload glyph, the
/// secure lock, and the status row. Also turns a page's popup request into an
/// [`OpenWebBrowser`] — in the window the page named, or a new one, as the
/// reference's `LLMediaCtrl` does with a clicked `target=` link (popups are
/// suppressed engine-side).
fn sync_web_floater(
    windows: Query<(&WebWindow, &UiPanelShown)>,
    views: Query<&BrowserView>,
    surfaces: Option<NonSend<MediaSurfaces>>,
    focus: Res<InputFocus>,
    mut opens: MessageWriter<OpenWebBrowser>,
    mut chrome: WebChrome,
) {
    let Some(surfaces) = surfaces else {
        return;
    };
    for (window, shown) in &windows {
        if !shown.0 {
            continue;
        }
        let Some(slot) = views
            .get(window.parts.view)
            .ok()
            .and_then(|view| view.surface)
            .and_then(|id| surfaces.get(id))
        else {
            continue;
        };
        // The page's choice of URL, not the user's: validated when it opens.
        if let Some(popup) = slot.surface.take_popup_request() {
            opens.write(OpenWebBrowser {
                url: Some(popup.url),
                target: popup.target.map_or(WebTarget::New, WebTarget::Named),
            });
        }
        let status = &slot.status;
        if let Ok(mut title) = chrome.texts.get_mut(window.title_text) {
            let want = if status.title.is_empty() {
                &status.url
            } else {
                &status.title
            };
            if title.0 != *want {
                title.0.clone_from(want);
            }
        }
        let editing_address = focus.get() == Some(window.parts.address);
        draw_web_status(&window.parts, status, editing_address, &mut chrome);
    }
}

/// Draw one status snapshot into the content's chrome: the address (unless
/// `editing_address`), back/forward enablement, stop-vs-reload glyph, the
/// secure lock and the status row. The pure half of [`sync_web_floater`],
/// which the specimen calls with a sample status.
fn draw_web_status(
    parts: &WebContentParts,
    status: &SurfaceStatus,
    editing_address: bool,
    chrome: &mut WebChrome,
) {
    let WebChrome {
        texts,
        classes,
        editors,
        visibilities,
        font_cx,
        layout_cx,
        ..
    } = chrome;
    // The address mirrors the page unless the user is editing it. Set through
    // the parley editor + a layout refresh (the `ui_text_input` revert idiom)
    // — a queued edit against a cleared buffer can apply at a stale selection
    // offset and panic on a char boundary.
    if !editing_address
        && let Ok(mut editor) = editors.get_mut(parts.address)
        && editor.value().to_string() != status.url
    {
        set_editor_text(&mut editor, &status.url, font_cx, layout_cx);
    }
    set_state_class_on(
        classes,
        parts.back_label,
        DISABLED_TEXT_CLASS,
        !status.can_go_back,
    );
    set_state_class_on(
        classes,
        parts.forward_label,
        DISABLED_TEXT_CLASS,
        !status.can_go_forward,
    );
    // Reload or stop: the state is ours, the mark the skin's.
    set_state_class_on(classes, parts.reload_label, glyph::LOADING, status.loading);
    if let Ok(mut lock) = visibilities.get_mut(parts.lock) {
        let want = if status.url.starts_with("https://") {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *lock != want {
            *lock = want;
        }
    }
    if let Ok(mut text) = texts.get_mut(parts.status_text) {
        let want = if let Some(error) = &status.load_error {
            error.clone()
        } else if status.loading {
            format!("Loading… {:.0}%", status.progress * 100.0)
        } else {
            String::new()
        };
        if text.0 != want {
            text.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::InputFocus;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_viewer_intents::{OpenWebBrowser, WebTarget};
    use sl_viewer_ui_core::ui::UiRoot;
    use sl_viewer_ui_widgets::floater::{Floater, FloaterPlugin};

    use super::{WebFloaterPlugin, WebWindow, normalize_web_url};
    use crate::media_engine::MediaSurfaces;

    /// A headless app with the floater manager and the browser windows, and
    /// an empty surface table: no engine, so every view stays waiting for its
    /// surface — the keying, not the page, is under test.
    fn browser_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<UiScale>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::text::FontCx>()
            .init_resource::<bevy::text::LayoutCx>()
            .insert_non_send(MediaSurfaces::default())
            .add_plugins((FloaterPlugin, WebFloaterPlugin));
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        app.update();
        app
    }

    /// Ask for `url` in `target`, as a link or the menu does, and let the
    /// open (and any close it asks for) land.
    fn open(app: &mut App, url: Option<&str>, target: WebTarget) {
        app.world_mut().write_message(OpenWebBrowser {
            url: url.map(str::to_owned),
            target,
        });
        app.update();
        app.update();
    }

    /// Every browser window's key, in the order the windows were opened.
    fn windows(app: &mut App) -> Vec<String> {
        let mut query = app.world_mut().query::<(&Floater, &WebWindow)>();
        let mut found: Vec<(u64, String)> = query
            .iter(app.world())
            .map(|(floater, window)| {
                (
                    window.opened,
                    floater
                        .key()
                        .map(|key| key.as_str().to_owned())
                        .unwrap_or_default(),
                )
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_opened, key)| key).collect()
    }

    /// The page each window's view opens on, in open order.
    fn pages(app: &mut App) -> Vec<String> {
        let mut query = app.world_mut().query::<&WebWindow>();
        let mut views: Vec<(u64, Entity)> = query
            .iter(app.world())
            .map(|window| (window.opened, window.view()))
            .collect();
        views.sort();
        views
            .into_iter()
            .filter_map(|(_opened, view)| {
                app.world()
                    .get::<crate::browser_widget::BrowserView>(view)
                    .map(|view| view.initial_url.as_str().to_owned())
            })
            .collect()
    }

    /// Two pages from two openers (the menu, a chat link) are two windows —
    /// the second no longer replaces the first's page.
    #[test]
    fn two_pages_open_two_windows() {
        let mut app = browser_app();
        open(&mut app, None, WebTarget::New);
        open(&mut app, Some("https://example.com/a"), WebTarget::New);
        assert_eq!(windows(&mut app), ["new-0", "new-1"]);
        assert_eq!(
            pages(&mut app),
            ["https://secondlife.com/", "https://example.com/a"]
        );
    }

    /// A page that names its window lands in that window every time, and
    /// navigates it rather than opening another.
    #[test]
    fn a_named_target_reuses_its_window() {
        let mut app = browser_app();
        let help = || WebTarget::Named(String::from("help"));
        open(&mut app, Some("https://example.com/a"), help());
        open(&mut app, Some("https://example.com/b"), help());
        assert_eq!(windows(&mut app), ["target-help"]);
        assert_eq!(pages(&mut app), ["https://example.com/b"]);
    }

    /// Two requests for one named window in the same frame open it once, on
    /// the later page.
    #[test]
    fn one_frame_two_requests_for_one_name_open_one_window() {
        let mut app = browser_app();
        for page in ["https://example.com/a", "https://example.com/b"] {
            app.world_mut().write_message(OpenWebBrowser {
                url: Some(page.to_owned()),
                target: WebTarget::Named(String::from("help")),
            });
        }
        app.update();
        app.update();
        assert_eq!(windows(&mut app), ["target-help"]);
        assert_eq!(pages(&mut app), ["https://example.com/b"]);
    }

    /// Past the reference's limit of five, opening a window closes the least
    /// recently opened one.
    #[test]
    fn the_sixth_window_closes_the_oldest() {
        let mut app = browser_app();
        for page in 0..6 {
            open(
                &mut app,
                Some(&format!("https://example.com/{page}")),
                WebTarget::New,
            );
        }
        assert_eq!(
            windows(&mut app),
            ["new-1", "new-2", "new-3", "new-4", "new-5"]
        );
    }

    /// A URL outside the media scheme allowlist opens no window at all.
    #[test]
    fn a_refused_url_opens_no_window() {
        let mut app = browser_app();
        open(&mut app, Some("file:///etc/passwd"), WebTarget::New);
        assert_eq!(windows(&mut app), Vec::<String>::new());
    }

    #[test]
    fn bare_hosts_get_https() {
        assert_eq!(
            normalize_web_url("example.com"),
            Some(String::from("https://example.com/"))
        );
        assert_eq!(
            normalize_web_url("  example.com/path?q=1 "),
            Some(String::from("https://example.com/path?q=1"))
        );
    }

    #[test]
    fn explicit_schemes_are_kept() {
        assert_eq!(
            normalize_web_url("http://example.com"),
            Some(String::from("http://example.com/"))
        );
    }

    #[test]
    fn junk_is_rejected() {
        assert_eq!(normalize_web_url(""), None);
        assert_eq!(normalize_web_url("   "), None);
        assert_eq!(normalize_web_url("http://"), None);
    }
}
