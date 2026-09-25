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
use sl_viewer_intents::OpenWebBrowser;
use sl_viewer_platform::system_browser::{ExternalUrl, normalize_web_url, open_in_system_browser};
use sl_viewer_ui_core::glyph;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin::{DISABLED_TEXT_CLASS, role_class, set_state_class_on, text_role};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::UiAction;
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_core::ui_text::set_editor_text;
use sl_viewer_ui_widgets::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The [`UiAction`] element name of the floater's toolbar.
pub const WEB_BROWSER_ELEMENT: &str = "web-browser";

/// The web-browser floater's stable [`sl_viewer_ui_widgets::floater::Floater::id`], the key
/// the openers (menu bar) look the panel up by.
pub const WEB_FLOATER_ID: &str = "web-browser";

/// The page a fresh floater opens on.
const DEFAULT_HOME_URL: &str = "https://secondlife.com/";

/// The toolbar / status font size.
const WEB_FONT_SIZE: f32 = 13.0;

/// The status line — the muted role.
const STATUS_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// The floater's entities.
#[derive(Debug, Resource)]
pub struct WebFloaterUi {
    /// The floater root (open/close via [`UiPanelShown`]).
    root: Entity,
    /// The title-bar text (bound to the page title).
    title_text: Entity,
    /// The content the status is drawn into.
    parts: WebContentParts,
}

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
            .add_systems(
                Startup,
                spawn_web_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (open_web_browser, handle_web_actions, sync_web_floater)
                    .chain()
                    .after(MediaEngineSystems::Pump),
            );
    }
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

/// Startup: build the floater — toolbar, browser view, status row.
fn spawn_web_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, web_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("web-floater-title"));
    let parts = spawn_web_content(
        &mut commands,
        handle.content,
        WEB_FONT_SIZE,
        validated_web_url(DEFAULT_HOME_URL).unwrap_or_else(|_refused| ValidatedMediaUrl::blank()),
    );
    commands.insert_resource(WebFloaterUi {
        root: handle.root,
        title_text: handle.title_text,
        parts,
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

/// One glyph toolbar button emitting a [`UiAction`]; returns the label entity
/// (recoloured for enablement), at `font_size`.
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
        ButtonSpec::bordered(UiLabel::Glyph(slot), format!("web-browser-button:{action}"))
            .kind(ButtonKind::Headless)
            .tab_index(tab_index)
            .padding(7.0, 3.0)
            .colors(Color::srgb(0.16, 0.17, 0.2), Color::srgb(0.35, 0.35, 0.4))
            .label_color(SkinPalette::FALLBACK.text_primary)
            .font_size(font_size),
    );
    commands.entity(spawned.button).observe(
        move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
            actions.write(UiAction {
                element: WEB_BROWSER_ELEMENT,
                action,
            });
        },
    );
    spawned.label
}

/// `Enter` in the address field navigates the view to the typed URL.
fn on_address_key(
    event: On<FocusedInput<KeyboardInput>>,
    editors: Query<&EditableText>,
    ui: Option<Res<WebFloaterUi>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
) {
    if !event.input.state.is_pressed() || event.input.key_code != KeyCode::Enter {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let Ok(editor) = editors.get(ui.parts.address) else {
        return;
    };
    let Some(url) = normalize_web_url(&editor.value().to_string()) else {
        return;
    };
    let Ok(url) = validated_web_url(&url) else {
        return;
    };
    if let Ok(view) = views.get(ui.parts.view)
        && let Some(slot) = view.surface.and_then(|id| surfaces.get(id))
    {
        slot.surface.navigate(&url);
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

/// Open the floater on an [`OpenWebBrowser`] message (menu, other floaters).
fn open_web_browser(
    mut requests: MessageReader<OpenWebBrowser>,
    ui: Option<Res<WebFloaterUi>>,
    mut panels: Query<&mut UiPanelShown>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
) {
    let Some(ui) = ui else {
        return;
    };
    for request in requests.read() {
        if let Ok(mut shown) = panels.get_mut(ui.root) {
            shown.0 = true;
        }
        if let Some(url) = &request.url
            && let Ok(url) = validated_web_url(url)
            && let Ok(view) = views.get(ui.parts.view)
            && let Some(slot) = view.surface.and_then(|id| surfaces.get(id))
        {
            slot.surface.navigate(&url);
        }
    }
}

/// Route the toolbar's [`UiAction`]s to the view's surface.
fn handle_web_actions(
    mut actions: MessageReader<UiAction>,
    ui: Option<Res<WebFloaterUi>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
) {
    let Some(ui) = ui else {
        return;
    };
    for action in actions.read() {
        if action.element != WEB_BROWSER_ELEMENT {
            continue;
        }
        let Ok(view) = views.get(ui.parts.view) else {
            continue;
        };
        let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
            continue;
        };
        match action.action {
            "back" => slot.surface.go_back(),
            "forward" => slot.surface.go_forward(),
            "reload-or-stop" => {
                if slot.status.loading {
                    slot.surface.stop();
                } else {
                    slot.surface.reload();
                }
            }
            // The page's *current* URL, which the page chose by navigating:
            // remote data, filtered at the sink like any other.
            "open-external" => {
                if let Ok(url) = ExternalUrl::parse(&slot.status.url).inspect_err(|error| {
                    warn!("web page not opened in the system browser: {error}");
                }) {
                    open_in_system_browser(&url);
                }
            }
            _ => {}
        }
    }
}

/// The floater's chrome, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam) — one query per piece
/// updated, plus the shown flag that says whether to update any of it and the
/// two text contexts a rewritten address bar is re-laid-out through.
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
    /// Whether the floater is shown at all — a hidden one is left alone.
    panels: Query<'w, 's, &'static UiPanelShown>,
    /// The font context a rewritten address bar is re-laid-out through.
    font_cx: ResMut<'w, FontCx>,
    /// Its layout context.
    layout_cx: ResMut<'w, LayoutCx>,
}

/// Mirror the view's status into the chrome: title, address (unless being
/// edited), back/forward enablement, stop-vs-reload glyph, the secure lock,
/// and the status row. Also routes a page's popup request into this same
/// view (popups are suppressed engine-side).
fn sync_web_floater(
    ui: Option<Res<WebFloaterUi>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
    focus: Res<InputFocus>,
    mut chrome: WebChrome,
) {
    let Some(ui) = ui else {
        return;
    };
    if !chrome.panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    let Ok(view) = views.get(ui.parts.view) else {
        return;
    };
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    // A popup request is the *page's* choice of URL, not the user's.
    if let Some(popup) = slot.surface.take_popup_request()
        && let Ok(popup) = validated_web_url(&popup)
    {
        slot.surface.navigate(&popup);
    }
    let status = &slot.status;

    if let Ok(mut title) = chrome.texts.get_mut(ui.title_text) {
        let want = if status.title.is_empty() {
            &status.url
        } else {
            &status.title
        };
        if title.0 != *want {
            title.0.clone_from(want);
        }
    }
    let editing_address = focus.get() == Some(ui.parts.address);
    draw_web_status(&ui.parts, status, editing_address, &mut chrome);
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
    use pretty_assertions::assert_eq;

    use super::normalize_web_url;

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
