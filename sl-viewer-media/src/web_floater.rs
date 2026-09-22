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
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use crate::browser_widget::{
    BrowserView, BrowserViewSpec, SurfaceTrust, ValidatedMediaUrl, spawn_browser_view,
};
use crate::media_engine::{MediaEngineSystems, MediaSurfaces};
use sl_viewer_intents::OpenWebBrowser;
use sl_viewer_platform::system_browser::{ExternalUrl, normalize_web_url, open_in_system_browser};
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin::{DISABLED_TEXT_CLASS, TEXT_CLASS, set_state_class_on, text_role};
use sl_viewer_ui_core::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::UiAction;
use sl_viewer_ui_core::ui_font::UiFont;
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

/// Status-row text colour.
const STATUS_COLOR: Color = Color::srgb(0.7, 0.72, 0.78);

/// The floater's entities.
#[derive(Debug, Resource)]
pub struct WebFloaterUi {
    /// The floater root (open/close via [`UiPanelShown`]).
    root: Entity,
    /// The title-bar text (bound to the page title).
    title_text: Entity,
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

    let content = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                ..column(Val::Px(4.0))
            },
            ChildOf(handle.content),
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
    let back_label = spawn_toolbar_button(&mut commands, toolbar, "◀", "back", 1);
    let forward_label = spawn_toolbar_button(&mut commands, toolbar, "▶", "forward", 2);
    let reload_label = spawn_toolbar_button(&mut commands, toolbar, "⟳", "reload-or-stop", 3);
    let lock = commands
        .spawn((
            Text::new("🔒"),
            UiFont::Sans.at(WEB_FONT_SIZE),
            text_role(STATUS_COLOR),
            Visibility::Hidden,
            ChildOf(toolbar),
        ))
        .id();
    let address = spawn_text_input(
        &mut commands,
        toolbar,
        &TextInputSpec {
            initial: String::new(),
            font_size: WEB_FONT_SIZE,
            width_glyphs: 40.0,
            tab_index: 4,
            max_characters: Some(1024),
            fill: true,
            ..TextInputSpec::new("web-address", TextInputKind::Line)
        },
    );
    commands.entity(address).observe(on_address_key);
    let _external = spawn_toolbar_button(&mut commands, toolbar, "↗", "open-external", 5);

    // The page itself.
    let view = spawn_browser_view(
        &mut commands,
        content,
        &BrowserViewSpec {
            initial_url: validated_web_url(DEFAULT_HOME_URL)
                .unwrap_or_else(|_refused| ValidatedMediaUrl::blank()),
            trust: SurfaceTrust::Viewer,
            tab_index: 6,
            fixed_height: None,
        },
    );

    // Status row.
    let status_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(WEB_FONT_SIZE),
            text_role(STATUS_COLOR),
            ChildOf(content),
        ))
        .id();

    commands.insert_resource(WebFloaterUi {
        root: handle.root,
        title_text: handle.title_text,
        view,
        address,
        back_label,
        forward_label,
        reload_label,
        lock,
        status_text,
    });
}

/// One glyph toolbar button emitting a [`UiAction`]; returns the label entity
/// (recoloured for enablement).
fn spawn_toolbar_button(
    commands: &mut Commands,
    parent: Entity,
    glyph: &str,
    action: &'static str,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(7.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.35, 0.35, 0.4)),
            BackgroundColor(Color::srgb(0.16, 0.17, 0.2)),
            Pickable::default(),
            Name::new(format!("web-browser-button:{action}")),
            ChildOf(parent),
        ))
        .observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: WEB_BROWSER_ELEMENT,
                    action,
                });
            },
        )
        .id();
    commands
        .spawn((
            Text::new(glyph),
            UiFont::Sans.at(WEB_FONT_SIZE),
            ClassList::new_with_classes([TEXT_CLASS]),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id()
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
    let Ok(editor) = editors.get(ui.address) else {
        return;
    };
    let Some(url) = normalize_web_url(&editor.value().to_string()) else {
        return;
    };
    let Ok(url) = validated_web_url(&url) else {
        return;
    };
    if let Ok(view) = views.get(ui.view)
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
            && let Ok(view) = views.get(ui.view)
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
        let Ok(view) = views.get(ui.view) else {
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
    chrome: WebChrome,
) {
    let WebChrome {
        mut texts,
        mut classes,
        mut editors,
        mut visibilities,
        panels,
        mut font_cx,
        mut layout_cx,
    } = chrome;
    let Some(ui) = ui else {
        return;
    };
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    let Ok(view) = views.get(ui.view) else {
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

    if let Ok(mut title) = texts.get_mut(ui.title_text) {
        let want = if status.title.is_empty() {
            &status.url
        } else {
            &status.title
        };
        if title.0 != *want {
            title.0.clone_from(want);
        }
    }
    // The address mirrors the page unless the user is editing it. Set through
    // the parley editor + a layout refresh (the `ui_text_input` revert idiom)
    // — a queued edit against a cleared buffer can apply at a stale selection
    // offset and panic on a char boundary.
    if focus.get() != Some(ui.address)
        && let Ok(mut editor) = editors.get_mut(ui.address)
        && editor.value().to_string() != status.url
    {
        set_editor_text(&mut editor, &status.url, &mut font_cx, &mut layout_cx);
    }
    set_state_class_on(
        &mut classes,
        ui.back_label,
        DISABLED_TEXT_CLASS,
        !status.can_go_back,
    );
    set_state_class_on(
        &mut classes,
        ui.forward_label,
        DISABLED_TEXT_CLASS,
        !status.can_go_forward,
    );
    if let Ok(mut reload) = texts.get_mut(ui.reload_label) {
        let want = if status.loading { "✕" } else { "⟳" };
        if reload.0 != want {
            want.clone_into(&mut reload.0);
        }
    }
    if let Ok(mut lock) = visibilities.get_mut(ui.lock) {
        let want = if status.url.starts_with("https://") {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *lock != want {
            *lock = want;
        }
    }
    if let Ok(mut text) = texts.get_mut(ui.status_text) {
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
