//! The reusable embedded-browser UI widget (`viewer-media-prim-browser`, UI
//! half): a `bevy_ui` node that renders one offscreen web-media surface
//! ([`crate::media_engine`]) and routes pointer + keyboard input to it — the
//! equivalent of the reference viewer's `LLMediaCtrl` (`web_browser` XUI
//! widget).
//!
//! Interaction model (the reference's):
//! - **Click to focus**: a press on the view focuses it (`bevy_input_focus`),
//!   which suppresses world keys via [`crate::input_context`], and forwards
//!   the click to the page.
//! - **Keys while focused** arrive as `FocusedInput<KeyboardInput>` on the
//!   node: navigation keys travel as portable VK codes
//!   ([`crate::media_keys`]), committed text as character input — never raw
//!   native key blobs.
//! - **`Escape`** releases focus (the scaffold's standard release), and
//!   **`Tab`** stays with the UI's focus navigation rather than the page — a
//!   deliberate divergence from the reference (`LLMediaCtrl` gives the plugin
//!   first refusal) so the widget can never become a focus trap.
//! - The pointer position reaches the page in surface pixels via
//!   [`RelativeCursorPosition`]; the surface itself is sized from the node's
//!   laid-out physical size, so page pixels are 1:1 with screen pixels.

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::input_context::InputContext;
use crate::media_engine::{MediaEngine, MediaEngineSystems, MediaSurfaceId, MediaSurfaces};
use crate::media_keys::{current_modifiers, is_printable_text, vk_for_key_code};
use sl_cef::{KeyInput, MouseButton as MediaMouseButton, SurfaceConfig};

/// Pixels of page scroll per scroll-wheel line (Chromium's usual notch).
const WHEEL_PIXELS_PER_LINE: f32 = 40.0;

/// A view smaller than this on either axis is treated as not laid out yet.
const MIN_VIEW_PIXELS: f32 = 8.0;

/// One embedded browser view. Spawn with [`spawn_browser_view`]; the systems
/// here create its engine surface once the node has a laid-out size.
#[derive(Component, Debug)]
pub(crate) struct BrowserView {
    /// The URL the surface loads on creation.
    pub(crate) initial_url: String,
    /// Whether the surface runs in an isolated request context (in-world /
    /// untrusted content) or the shared one (trusted UI panels, so logins
    /// persist).
    pub(crate) isolated: bool,
    /// The live engine surface, once created.
    pub(crate) surface: Option<MediaSurfaceId>,
}

/// What [`spawn_browser_view`] needs to know.
#[derive(Debug, Clone)]
pub(crate) struct BrowserViewSpec {
    /// The URL to load.
    pub(crate) initial_url: String,
    /// Isolated (untrusted) or shared (trusted UI) request context.
    pub(crate) isolated: bool,
    /// The tab order slot of the view.
    pub(crate) tab_index: i32,
    /// `None`: the view stretches to fill its parent (`flex_grow`). `Some`: a
    /// fixed height in logical pixels (width still fills).
    pub(crate) fixed_height: Option<f32>,
}

/// Entity → surface, mirrored into a `Send` resource so the despawn-cleanup
/// system can close surfaces without the (already despawned) component.
#[derive(Resource, Default)]
struct BrowserViewIndex(std::collections::HashMap<Entity, MediaSurfaceId>);

/// The browser view that held focus last frame, to notify the engine on
/// focus loss.
#[derive(Resource, Default)]
struct FocusedBrowserView(Option<Entity>);

/// The embedded-browser widget plugin.
pub(crate) struct BrowserWidgetPlugin;

impl Plugin for BrowserWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BrowserViewIndex>()
            .init_resource::<FocusedBrowserView>()
            .add_systems(
                Update,
                (
                    create_browser_surfaces,
                    resize_browser_surfaces,
                    sync_browser_focus,
                    close_removed_browser_views,
                )
                    .chain()
                    .after(MediaEngineSystems::Pump),
            );
    }
}

/// Spawn an embedded browser view under `parent`. The engine surface is
/// created lazily once layout gives the node a size; until then (or with the
/// engine disabled) the view is a dark placeholder.
pub(crate) fn spawn_browser_view(
    commands: &mut Commands,
    parent: Entity,
    spec: &BrowserViewSpec,
) -> Entity {
    let mut node = Node {
        width: Val::Percent(100.0),
        ..default()
    };
    match spec.fixed_height {
        Some(height) => node.height = Val::Px(height),
        None => {
            node.flex_grow = 1.0;
            node.min_height = Val::Px(64.0);
        }
    }
    commands
        .spawn((
            node,
            BrowserView {
                initial_url: spec.initial_url.clone(),
                isolated: spec.isolated,
                surface: None,
            },
            BackgroundColor(Color::srgb(0.08, 0.09, 0.11)),
            RelativeCursorPosition::default(),
            bevy::input_focus::tab_navigation::TabIndex(spec.tab_index),
            Pickable::default(),
            Name::new("browser-view"),
            ChildOf(parent),
        ))
        .observe(on_browser_press)
        .observe(on_browser_release)
        .observe(on_browser_move)
        .observe(on_browser_out)
        .observe(on_browser_scroll)
        .observe(on_browser_key)
        .id()
}

/// The pointer position of `relative` in surface pixels, given the surface
/// size. `RelativeCursorPosition::normalized` is `(-0.5, -0.5)` at the
/// top-left corner and `(0.5, 0.5)` at the bottom-right.
fn surface_pixel(relative: &RelativeCursorPosition, size: UVec2) -> Option<(i32, i32)> {
    let normalized = relative.normalized?;
    let width = u16::try_from(size.x).unwrap_or(u16::MAX);
    let height = u16::try_from(size.y).unwrap_or(u16::MAX);
    let x = ((normalized.x + 0.5) * f32::from(width)).round();
    let y = ((normalized.y + 0.5) * f32::from(height)).round();
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    // The i32 range is far wider than any surface; f32→i32 via the checked
    // helper below keeps the cast lints happy.
    Some((float_to_pixel(x), float_to_pixel(y)))
}

/// A finite `f32` pixel coordinate as `i32`, saturating.
pub(crate) fn float_to_pixel(value: f32) -> i32 {
    if value >= 2_147_483_000.0 {
        i32::MAX
    } else if value <= -2_147_483_000.0 {
        i32::MIN
    } else {
        // Truncation is fine for a rounded, range-checked pixel coordinate.
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "value is rounded and range-checked just above"
        )]
        {
            value as i32
        }
    }
}

/// The engine mouse button for a picking pointer button.
const fn media_button(button: PointerButton) -> MediaMouseButton {
    match button {
        PointerButton::Primary => MediaMouseButton::Left,
        PointerButton::Secondary => MediaMouseButton::Right,
        PointerButton::Middle => MediaMouseButton::Middle,
    }
}

/// Pointer press on a view: focus it (click-to-focus) and forward the press.
fn on_browser_press(
    event: On<Pointer<Press>>,
    views: Query<(&BrowserView, &RelativeCursorPosition)>,
    surfaces: NonSend<MediaSurfaces>,
    mut focus: ResMut<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let entity = event.entity;
    let Ok((view, relative)) = views.get(entity) else {
        return;
    };
    focus.set(entity, FocusCause::Pressed);
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    let Some((x, y)) = surface_pixel(relative, slot.size) else {
        return;
    };
    slot.surface.set_focus(true);
    slot.surface.mouse_button(
        x,
        y,
        media_button(event.button),
        true,
        event.count.clamp(1, 2),
        current_modifiers(&keyboard, &mouse),
    );
}

/// Pointer release on a view: forward the button up.
fn on_browser_release(
    event: On<Pointer<Release>>,
    views: Query<(&BrowserView, &RelativeCursorPosition)>,
    surfaces: NonSend<MediaSurfaces>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let Ok((view, relative)) = views.get(event.entity) else {
        return;
    };
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    let Some((x, y)) = surface_pixel(relative, slot.size) else {
        return;
    };
    slot.surface.mouse_button(
        x,
        y,
        media_button(event.button),
        false,
        1,
        current_modifiers(&keyboard, &mouse),
    );
}

/// Pointer motion over a view: forward the hover position.
fn on_browser_move(
    event: On<Pointer<bevy::picking::events::Move>>,
    views: Query<(&BrowserView, &RelativeCursorPosition)>,
    surfaces: NonSend<MediaSurfaces>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let Ok((view, relative)) = views.get(event.entity) else {
        return;
    };
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    let Some((x, y)) = surface_pixel(relative, slot.size) else {
        return;
    };
    slot.surface
        .mouse_move(x, y, current_modifiers(&keyboard, &mouse));
}

/// Pointer leaving a view: tell the page.
fn on_browser_out(
    event: On<Pointer<bevy::picking::events::Out>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
) {
    let Ok(view) = views.get(event.entity) else {
        return;
    };
    if let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) {
        slot.surface.mouse_leave();
    }
}

/// Scroll over a view: forward as pixel deltas and keep it from also
/// scrolling an enclosing UI scroll container.
fn on_browser_scroll(
    mut event: On<Pointer<Scroll>>,
    views: Query<(&BrowserView, &RelativeCursorPosition)>,
    surfaces: NonSend<MediaSurfaces>,
) {
    let Ok((view, relative)) = views.get(event.entity) else {
        return;
    };
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    let Some((x, y)) = surface_pixel(relative, slot.size) else {
        return;
    };
    slot.surface.mouse_wheel(
        x,
        y,
        float_to_pixel(event.x * WHEEL_PIXELS_PER_LINE),
        float_to_pixel(event.y * WHEEL_PIXELS_PER_LINE),
    );
    event.propagate(false);
}

/// Keys while a view holds focus: navigation keys as portable VK events,
/// committed text as character input. `Tab` is left to the scaffold's focus
/// navigation and `Escape` to its focus release (see the module docs).
fn on_browser_key(
    event: On<FocusedInput<KeyboardInput>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let Ok(view) = views.get(event.focused_entity) else {
        return;
    };
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    let input = &event.input;
    if matches!(input.key_code, KeyCode::Tab | KeyCode::Escape) {
        return;
    }
    let modifiers = current_modifiers(&keyboard, &mouse);
    let down = input.state.is_pressed();
    if let Some(vk) = vk_for_key_code(input.key_code) {
        slot.surface.key(KeyInput {
            down,
            vk,
            modifiers,
        });
    }
    if down
        && let Some(text) = &input.text
        && is_printable_text(text)
    {
        slot.surface.insert_text(text);
    }
}

/// Create the engine surface for every view whose node has a laid-out size,
/// and bind its image to the node.
fn create_browser_surfaces(
    mut views: Query<(Entity, &mut BrowserView, &ComputedNode)>,
    mut engine: NonSendMut<MediaEngine>,
    mut surfaces: NonSendMut<MediaSurfaces>,
    mut images: ResMut<Assets<Image>>,
    mut index: ResMut<BrowserViewIndex>,
    mut commands: Commands,
) {
    for (entity, mut view, computed) in &mut views {
        if view.surface.is_some() {
            continue;
        }
        let size = computed.size();
        if size.x < MIN_VIEW_PIXELS || size.y < MIN_VIEW_PIXELS {
            continue;
        }
        let config = SurfaceConfig {
            width: pixel_dimension(size.x),
            height: pixel_dimension(size.y),
            initial_url: view.initial_url.clone(),
            isolated: view.isolated,
            max_fps: 30,
            muted: false,
            loop_media: false,
        };
        let Some(id) = surfaces.create(&mut engine, &mut images, &config) else {
            // No engine: leave the placeholder and stop retrying.
            view.surface = Some(MediaSurfaceId::PLACEHOLDER);
            continue;
        };
        view.surface = Some(id);
        index.0.insert(entity, id);
        if let Some(slot) = surfaces.get(id) {
            commands
                .entity(entity)
                .insert(ImageNode::new(slot.image.clone()));
        }
    }
}

/// A positive `f32` pixel dimension as `u32` for the engine.
fn pixel_dimension(value: f32) -> u32 {
    if value <= 1.0 {
        1
    } else if value >= 8192.0 {
        8192
    } else {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value is range-checked to [1, 8192] just above"
        )]
        {
            value as u32
        }
    }
}

/// Keep each surface sized to its node's laid-out physical size.
fn resize_browser_surfaces(
    views: Query<(&BrowserView, &ComputedNode), Changed<ComputedNode>>,
    surfaces: NonSend<MediaSurfaces>,
) {
    for (view, computed) in &views {
        let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
            continue;
        };
        let size = computed.size();
        if size.x < MIN_VIEW_PIXELS || size.y < MIN_VIEW_PIXELS {
            // Collapsed (hidden) views idle at 1 fps rather than resize to
            // nothing.
            slot.surface.set_max_fps(1);
            continue;
        }
        slot.surface.set_max_fps(30);
        slot.surface
            .resize(pixel_dimension(size.x), pixel_dimension(size.y));
    }
}

/// Mirror `bevy_input_focus` onto the engine: the view under focus gets
/// engine focus, the one that lost it gives it up.
fn sync_browser_focus(
    focus: Res<InputFocus>,
    mut last: ResMut<FocusedBrowserView>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
    _context: Res<InputContext>,
) {
    let current = focus.get().filter(|entity| views.contains(*entity));
    if current == last.0 {
        return;
    }
    if let Some(previous) = last.0
        && let Ok(view) = views.get(previous)
        && let Some(slot) = view.surface.and_then(|id| surfaces.get(id))
    {
        slot.surface.set_focus(false);
    }
    if let Some(next) = current
        && let Ok(view) = views.get(next)
        && let Some(slot) = view.surface.and_then(|id| surfaces.get(id))
    {
        slot.surface.set_focus(true);
    }
    last.0 = current;
}

/// Close the engine surface of every despawned view.
fn close_removed_browser_views(
    mut removed: RemovedComponents<BrowserView>,
    mut index: ResMut<BrowserViewIndex>,
    mut surfaces: NonSendMut<MediaSurfaces>,
) {
    for entity in removed.read() {
        if let Some(id) = index.0.remove(&entity) {
            surfaces.close(id);
        }
    }
}

/// The offline gallery / test specimen: a bordered, fixed-size browser view
/// on a data URL (no network), or the dark placeholder when the media engine
/// is not running.
pub(crate) fn spawn_browser_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let frame = commands
        .spawn((
            Node {
                width: Val::Px(480.0),
                height: Val::Px(320.0),
                border: UiRect::all(Val::Px(1.0)),
                ..crate::ui::column(Val::Px(0.0))
            },
            BorderColor::all(Color::srgb(0.4, 0.4, 0.45)),
            Name::new("browser-view-specimen"),
            ChildOf(parent),
        ))
        .id();
    let sample = cx.text("sl-client embedded browser");
    let url = format!(
        "data:text/html,<body style='background:%23202430;color:%23e8e8f0;\
         font-family:sans-serif'><h2>{}</h2><p>offline specimen page</p></body>",
        sample.replace(' ', "%20")
    );
    spawn_browser_view(
        commands,
        frame,
        &BrowserViewSpec {
            initial_url: url,
            isolated: true,
            tab_index: 1,
            fixed_height: None,
        },
    );
    frame
}
