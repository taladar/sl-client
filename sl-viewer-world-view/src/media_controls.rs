//! The **floating media controls bar** (`viewer-media-prim-browser` /
//! `viewer-video-playback`): the reference viewer's
//! `LLPanelPrimMediaControls` — a small button bar hovering above the media
//! face under the cursor (or holding focus). For a **web** surface it shows
//! the browser set — back / forward / home / stop-or-reload / URL / mute /
//! zoom / open-external, a load progress read-out and the secure-lock marker
//! (the Vintage skin's `panel_prim_media_controls.xml` web-mode set). For a
//! **video** surface (a GStreamer playback face) it shows the movie set
//! instead: play-pause / restart, a seek scrubber with a position ∕ duration
//! read-out (hidden for unseekable live streams), mute, zoom and
//! open-external — with the stream's "now playing" title (or its loud
//! missing-decoder error) in the status slot.
//!
//! Placement mirrors the reference's `updateShape`: the face's bounding box
//! corners are projected to the viewport, and the bar sits centred above the
//! box's top edge, clamped on-screen. The bar hides after ~3 s without
//! pointer activity (the reference fades; this bar hides), reappearing on the
//! next hover. Which controls show follows the entry: `controls == MINI`
//! drops the URL field, and a viewer without control permission
//! (`perms_control`, [`crate::media_prim::media_permission_allows`]) gets no
//! bar at all.
//!
//! **Zoom** parks the third-person camera squarely in front of the face
//! (focus-on-point plus a normal-scaled offset — `LLViewerMediaFocus::
//! setCameraZoom`'s geometry, simplified); **unzoom** returns the focus to
//! the avatar. `Escape` (which also drops media focus) unzooms too. The Nearby
//! Media window zooms the same way, by [`MediaZoomRequest`] — onto a face it
//! lists, which need not be under the cursor or even playing.

use bevy::camera::primitives::Aabb;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui_widgets::{
    Activate, Slider, SliderDragState, SliderRange, SliderStep, SliderValue, ValueChange,
};
use bevy::window::PrimaryWindow;
use bevy_flair::style::components::ClassList;
use sl_cef::{PlaybackState, ValidatedMediaUrl};
use sl_client_bevy::{Command, SlCommand};
use sl_viewer_ui_core::glyph;
use sl_viewer_ui_core::skin::{
    DISABLED_TEXT_CLASS, role_class, set_state_class, set_state_class_on, text_role,
};

use crate::camera::FocusTarget;
use crate::media_prim::{MediaData, MediaPrimState, media_permission_allows};
use sl_viewer_media::media_diagnostics::MediaDiagnostics;
use sl_viewer_media::media_engine::{MediaEngineKind, MediaEngineSystems, MediaSurfaces};
use sl_viewer_platform::system_browser::{ExternalUrl, normalize_web_url, open_in_system_browser};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::UiAction;
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_widgets::ui_slider::{SliderStyle, SliderWidgetPlugin, spawn_slider};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_world_api::MediaFocus;
use sl_viewer_world_api::MediaTarget;
use sl_viewer_world_api::ObjectState;
use sl_viewer_world_api::{CameraRig, ViewerCamera};

/// The [`UiAction`] element name of the bar.
pub const MEDIA_CONTROLS_ELEMENT: &str = "media-controls";

/// Seconds without pointer activity before the bar hides (the reference's
/// `MediaControlTimeout`).
const INACTIVITY_HIDE_SECONDS: f32 = 3.0;

/// The `FLAGS_OBJECT_YOU_OWNER` update-flags bit (the agent owns the object).
const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// `MediaEntry::controls` value for the reduced (mini) control set.
const CONTROLS_MINI: i32 = 1;

/// The bar's label — the primary role.
const BAR_LABEL: Color = SkinPalette::FALLBACK.text_primary;
/// The label of a control the bar cannot offer: the disabled role, which is
/// what this tone was — the two `BAR_LABEL_DIM`s meant different things.
const BAR_LABEL_DIM: Color = SkinPalette::FALLBACK.text_disabled;

/// How the seek scrubber is drawn.
const SCRUBBER: SliderStyle = SliderStyle {
    track_width: 220.0,
    track_height: 12.0,
    border: 1.0,
    border_color: Color::srgb(0.3, 0.3, 0.35),
    track_fill: Color::srgb(0.16, 0.19, 0.25),
    thumb_width: 10.0,
    thumb_fill: Color::srgb(0.62, 0.72, 0.86),
};

/// The bar's entities.
#[derive(Resource)]
struct MediaControlsUi {
    /// The bar root (absolute-positioned, shown/hidden).
    root: Entity,
    /// The URL field (hidden for mini controls and video surfaces).
    url_field: Entity,
    /// The URL row wrapper (hidden with the field).
    url_row: Entity,
    /// Play-pause button (video surfaces only) and its glyph label.
    play: (Entity, Entity),
    /// Back button (web surfaces only) and its label.
    back: (Entity, Entity),
    /// Forward button (web surfaces only) and its label.
    forward: (Entity, Entity),
    /// Home button (web surfaces only).
    home: Entity,
    /// Stop-or-reload label.
    reload_label: Entity,
    /// Mute toggle label.
    mute_label: Entity,
    /// Zoom toggle label.
    zoom_label: Entity,
    /// The progress / status text.
    status_text: Entity,
    /// The secure-lock glyph.
    lock: Entity,
    /// The seek-scrubber row (video surfaces with a seekable stream).
    scrub_row: Entity,
    /// The seek slider.
    scrub_slider: Entity,
    /// The position ∕ duration read-out.
    time_text: Entity,
}

/// Which media face the bar currently controls, plus the zoom state.
#[derive(Resource, Debug, Default)]
pub struct MediaControlsState {
    /// The face the bar is shown for.
    target: Option<MediaTarget>,
    /// Seconds since the last pointer activity.
    idle: f32,
    /// The face the camera is currently zoomed onto, if any.
    zoomed: Option<MediaTarget>,
}

impl MediaControlsState {
    /// The face the camera is zoomed onto, if any — the Nearby Media window
    /// reads it to offer *unzoom* rather than *zoom* for that row.
    #[must_use]
    pub const fn zoomed(&self) -> Option<MediaTarget> {
        self.zoomed
    }
}

/// Zoom the camera onto a media face, or back out — the Nearby Media window's
/// zoom / unzoom and its row double-click (the reference's
/// `LLViewerMediaFocus::focusZoomOnMedia` / `unZoom`).
///
/// A zoom also gives the face media focus, as the reference's does, so the zoom
/// holds until focus moves on (`Escape`, a click elsewhere) exactly as a zoom
/// from the floating bar does.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaZoomRequest {
    /// The face to zoom onto; `None` zooms back out to the avatar.
    pub target: Option<MediaTarget>,
}

/// The floating media-controls plugin.
#[derive(Debug, Clone, Copy, Default)]
pub struct MediaControlsPlugin;

impl Plugin for MediaControlsPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SliderWidgetPlugin>() {
            app.add_plugins(SliderWidgetPlugin);
        }
        app.init_resource::<MediaControlsState>()
            .add_message::<MediaZoomRequest>()
            .add_systems(
                Startup,
                spawn_media_controls.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    update_media_controls,
                    drive_scrub_visual,
                    handle_media_control_actions,
                    handle_media_zoom_requests,
                    unzoom_on_focus_loss,
                )
                    .chain()
                    .after(MediaEngineSystems::Pump)
                    .after(crate::media_prim::MediaPrimSystems::Drive),
            );
    }
}

/// Startup: build the (hidden) bar under the UI root.
fn spawn_media_controls(mut commands: Commands, root: Res<UiRoot>) {
    let bar = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // Physical (not logical) placement on purpose: the bar is
                // anchored to a screen-space projection of a world face, which
                // does not mirror in RTL layouts.
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                padding: UiRect::all(Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..column(Val::Px(3.0))
            },
            BorderColor::all(Color::srgb(0.35, 0.35, 0.4)),
            BackgroundColor(Color::srgba(0.1, 0.1, 0.12, 0.92)),
            GlobalZIndex(40),
            UiPanelShown(false),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("media-controls-bar"),
            ChildOf(root.0),
        ))
        .id();

    let buttons = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(3.0))
            },
            ChildOf(bar),
        ))
        .id();
    let play = spawn_bar_button(&mut commands, buttons, glyph::PLAY_PAUSE, "play-pause", 29);
    let back = spawn_bar_button(&mut commands, buttons, glyph::BACK, "back", 30);
    let forward = spawn_bar_button(&mut commands, buttons, glyph::FORWARD, "forward", 31);
    let (home, _home_label) = spawn_bar_button(&mut commands, buttons, glyph::HOME, "home", 32);
    let (_reload, reload_label) =
        spawn_bar_button(&mut commands, buttons, glyph::RELOAD, "reload-or-stop", 33);
    let (_mute, mute_label) =
        spawn_bar_button(&mut commands, buttons, glyph::SPEAKER, "mute-toggle", 34);
    let (_zoom, zoom_label) =
        spawn_bar_button(&mut commands, buttons, glyph::ZOOM, "zoom-toggle", 35);
    let _external = spawn_bar_button(&mut commands, buttons, glyph::EXTERNAL, "open-external", 36);
    let status_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(11.0),
            text_role(BAR_LABEL_DIM),
            ChildOf(buttons),
        ))
        .id();

    let (scrub_row, scrub_slider, time_text) = spawn_scrub_row(&mut commands, bar);

    let url_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(3.0))
            },
            ChildOf(bar),
        ))
        .id();
    let lock = commands
        .spawn((
            glyph::glyph_host(
                glyph::SECURE,
                UiFont::Sans.at(11.0),
                role_class(BAR_LABEL_DIM),
            ),
            TextColor(BAR_LABEL_DIM),
            Visibility::Hidden,
            ChildOf(url_row),
        ))
        .id();
    let url_field = spawn_text_input(
        &mut commands,
        url_row,
        &TextInputSpec {
            initial: String::new(),
            font_size: 11.0,
            width_glyphs: 36.0,
            tab_index: 37,
            max_characters: Some(1023),
            ..TextInputSpec::new("media-url", TextInputKind::Line)
        },
    );
    commands.entity(url_field).observe(on_media_url_key);

    commands.insert_resource(MediaControlsUi {
        root: bar,
        url_field,
        url_row,
        play,
        back,
        forward,
        home,
        reload_label,
        mute_label,
        zoom_label,
        status_text,
        lock,
        scrub_row,
        scrub_slider,
        time_text,
    });
}

/// The video seek row: a scrubber slider whose drags seek the surface, plus
/// the position ∕ duration read-out. Returns `(row, slider, time_text)`.
fn spawn_scrub_row(commands: &mut Commands, bar: Entity) -> (Entity, Entity, Entity) {
    let scrub_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                // Hidden until a seekable video surface is under the bar.
                display: Display::None,
                ..row(Val::Px(6.0))
            },
            ChildOf(bar),
        ))
        .id();
    let slider = spawn_slider(
        commands,
        scrub_row,
        SCRUBBER,
        38,
        0.0,
        (
            Slider::default(),
            SliderValue(0.0),
            SliderRange::new(0.0, 1.0),
            SliderStep(1.0),
            Name::new("media-controls-scrubber"),
        ),
    );
    commands.entity(slider).observe(on_scrub_change);
    let time_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(11.0),
            text_role(BAR_LABEL),
            ChildOf(scrub_row),
        ))
        .id();
    (scrub_row, slider, time_text)
}

/// Observer: the scrubber was dragged — seek the bar's surface to the new
/// position (the slider range is the media duration in seconds).
fn on_scrub_change(
    change: On<ValueChange<f32>>,
    bar_state: Res<MediaControlsState>,
    prim_state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
    mut commands: Commands,
) {
    // Reflect the drag on the widget (the headless slider does not move its
    // own value).
    commands
        .entity(change.source)
        .insert(SliderValue(change.value));
    let Some(target) = bar_state.target else {
        return;
    };
    let Some(active) = prim_state.active.get(&target) else {
        return;
    };
    let Some(slot) = surfaces.get(active.surface) else {
        return;
    };
    slot.surface.seek(f64::from(change.value));
}

/// Keep the scrubber's thumb at the slider's value, and the slider tracking
/// the playback position while it is not being dragged.
fn drive_scrub_visual(
    ui: Option<Res<MediaControlsUi>>,
    bar_state: Res<MediaControlsState>,
    prim_state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
    sliders: Query<(&SliderValue, &SliderRange, &SliderDragState), With<Slider>>,
    mut commands: Commands,
) {
    let Some(ui) = ui else { return };
    let Ok((value, range, drag)) = sliders.get(ui.scrub_slider) else {
        return;
    };
    // Track the live position (and duration → range) unless the user is
    // mid-drag. `SliderValue` / `SliderRange` are immutable components, so
    // updates go through insertion.
    if !drag.dragging
        && let Some(target) = bar_state.target
        && let Some(active) = prim_state.active.get(&target)
        && let Some(slot) = surfaces.get(active.surface)
        && let Some(playback) = &slot.status.playback
    {
        if let Some(duration) = playback.duration_seconds {
            let end = as_slider_seconds(duration).max(1.0);
            // Bit-exact change detection, not a numeric comparison.
            if range.end().to_bits() != end.to_bits() {
                commands
                    .entity(ui.scrub_slider)
                    .insert(SliderRange::new(0.0, end));
            }
        }
        let position = as_slider_seconds(playback.position_seconds);
        if value.0.to_bits() != position.to_bits() {
            commands
                .entity(ui.scrub_slider)
                .insert(SliderValue(position));
        }
    }
}

/// A media position in seconds as the scrubber's `f32` value.
#[expect(
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    reason = "media positions are far below f32's precision loss threshold for whole seconds"
)]
fn as_slider_seconds(seconds: f64) -> f32 {
    seconds.clamp(0.0, f64::from(f32::MAX)) as f32
}

/// A playback time as `m:ss` (or `h:mm:ss` from an hour up), for the
/// scrubber's read-out.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions,
    reason = "the value is clamped non-negative and rounded before the cast, and a media \
              position always fits u64"
)]
fn format_media_time(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// The glyph buttons' font size, in logical pixels.
const GLYPH_FONT_SIZE: f32 = 12.0;

/// One glyph button on the bar; returns `(button, label)`.
fn spawn_bar_button(
    commands: &mut Commands,
    parent: Entity,
    slot: &'static str,
    action: &'static str,
    tab_index: i32,
) -> (Entity, Entity) {
    let spawned = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::glyph(slot, format!("media-controls-{action}-name")),
            format!("media-controls-button:{action}"),
        )
        .kind(ButtonKind::Headless)
        .tab_index(tab_index)
        .compact()
        .colors(Color::srgb(0.16, 0.17, 0.2), Color::srgb(0.3, 0.3, 0.35))
        .label_color(SkinPalette::FALLBACK.text_primary)
        .font_size(GLYPH_FONT_SIZE),
    );
    commands.entity(spawned.button).observe(
        move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
            actions.write(UiAction {
                element: MEDIA_CONTROLS_ELEMENT,
                action,
            });
        },
    );
    (spawned.button, spawned.label)
}

/// The bar's chrome queries, bundled to stay within Bevy's system-parameter
/// arity.
#[derive(bevy::ecs::system::SystemParam)]
struct BarChrome<'w, 's> {
    /// The bar root's (and URL row's) layout node.
    nodes: Query<'w, 's, &'static mut Node>,
    /// The bar root's laid-out size, for placement.
    computed: Query<'w, 's, &'static ComputedNode>,
    /// The bar's show/hide switch.
    shown_panels: Query<'w, 's, &'static mut UiPanelShown>,
    /// Text labels (reload glyph, mute glyph, zoom glyph, progress).
    texts: Query<'w, 's, &'static mut Text>,
    /// The class lists an enable-gated label's state is written through.
    classes: Query<'w, 's, &'static mut ClassList>,
    /// The URL field.
    editors: Query<'w, 's, &'static mut EditableText>,
    /// The secure-lock glyph's visibility.
    visibilities: Query<'w, 's, &'static mut Visibility>,
    /// The font context for programmatic text replacement.
    font_cx: ResMut<'w, FontCx>,
    /// The layout context for programmatic text replacement.
    layout_cx: ResMut<'w, LayoutCx>,
}

/// The media state the controls bar shows, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): what is focused or hovered,
/// the per-object media data behind it, which faces hold a live surface, and
/// those surfaces themselves.
#[derive(bevy::ecs::system::SystemParam)]
struct MediaBarState<'w> {
    /// What the bar is showing: the focused face, else the hovered one.
    focus: Res<'w, MediaFocus>,
    /// The per-object media data (URL, permissions, controls).
    data: Res<'w, MediaData>,
    /// Which faces hold a live surface.
    prim_state: Res<'w, MediaPrimState>,
    /// The live surfaces, for the title / progress / secure state.
    surfaces: NonSend<'w, MediaSurfaces>,
}

/// The bar's own mutable state, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the target and idle timer it
/// shows itself by, and the media diagnostics it reports a stalled load through.
#[derive(Debug, bevy::ecs::system::SystemParam)]
struct BarRuntime<'w> {
    /// The bar's target, zoom and inactivity timer.
    state: ResMut<'w, MediaControlsState>,
    /// The per-URL load diagnostics shown in the status line.
    diagnostics: ResMut<'w, MediaDiagnostics>,
}

/// What places the bar over its face on screen, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the object model, the world
/// camera the face is projected through, the face's bounds and pose, and the
/// window the projection lands in.
#[derive(Debug, bevy::ecs::system::SystemParam)]
struct BarProjection<'w, 's> {
    /// The object model, for the owner check.
    objects: Res<'w, ObjectState>,
    /// The world camera the face is projected through.
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// The face's bounds and world pose.
    face_geometry: Query<'w, 's, (&'static Aabb, &'static GlobalTransform)>,
    /// The window the projection lands in.
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

/// The pointer activity the bar's auto-hide is driven by, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(Debug, bevy::ecs::system::SystemParam)]
struct BarPointer<'w, 's> {
    /// Cursor motion, which resets the idle timer.
    cursor_moves: MessageReader<'w, 's, bevy::window::CursorMoved>,
    /// Mouse presses, likewise.
    mouse: Res<'w, ButtonInput<MouseButton>>,
    /// The focused widget, so the URL field keeps the keyboard.
    input_focus: Res<'w, InputFocus>,
}

/// Show / place / sync the bar every frame.
fn update_media_controls(
    ui: Option<Res<MediaControlsUi>>,
    mut bar: BarRuntime,
    media: MediaBarState,
    time: Res<Time>,
    mut pointer: BarPointer,
    projection: BarProjection,
    mut chrome: BarChrome,
) {
    let Some(ui) = ui else {
        return;
    };
    // Pointer activity feeds the inactivity timer.
    let moved = pointer.cursor_moves.read().next().is_some();
    if moved || pointer.mouse.get_just_pressed().next().is_some() {
        bar.state.idle = 0.0;
    } else {
        bar.state.idle += time.delta_secs();
    }

    // Which face the bar serves: focus first, then hover; keep the current
    // target while the cursor is over the bar itself (hover = None then).
    let target =
        media
            .focus
            .focused
            .or(media.focus.hover)
            .or(if bar.state.idle < INACTIVITY_HIDE_SECONDS {
                bar.state.target
            } else {
                None
            });

    let mut show = false;
    'decide: {
        let Some(target) = target else {
            break 'decide;
        };
        let Some(entry) = media.data.entry(target) else {
            break 'decide;
        };
        let Some(active) = media.prim_state.active.get(&target) else {
            break 'decide;
        };
        let Some(slot) = media.surfaces.get(active.surface) else {
            break 'decide;
        };
        let is_owner = projection
            .objects
            .update_flags_by_key(target.object)
            .is_some_and(|flags| flags & FLAGS_OBJECT_YOU_OWNER != 0);
        if !media_permission_allows(entry.perms_control, is_owner) {
            break 'decide;
        }
        if bar.state.idle >= INACTIVITY_HIDE_SECONDS {
            break 'decide;
        }
        bar.state.target = Some(target);
        show = true;

        // ---- Placement: project the face's box, sit above its top edge.
        if let Ok((camera, camera_transform)) = projection.cameras.single()
            && let Ok((aabb, face_transform)) = projection.face_geometry.get(active.face_entity)
            && let Ok(window) = projection.windows.single()
        {
            let mut min = Vec2::new(f32::MAX, f32::MAX);
            let mut max = Vec2::new(f32::MIN, f32::MIN);
            let mut any = false;
            for index in 0..8_u8 {
                let corner = Vec3::new(
                    if index & 1 == 0 {
                        aabb.center.x - aabb.half_extents.x
                    } else {
                        aabb.center.x + aabb.half_extents.x
                    },
                    if index & 2 == 0 {
                        aabb.center.y - aabb.half_extents.y
                    } else {
                        aabb.center.y + aabb.half_extents.y
                    },
                    if index & 4 == 0 {
                        aabb.center.z - aabb.half_extents.z
                    } else {
                        aabb.center.z + aabb.half_extents.z
                    },
                );
                let world = face_transform.transform_point(corner);
                if let Ok(view) = camera.world_to_viewport(camera_transform, world) {
                    min = min.min(view);
                    max = max.max(view);
                    any = true;
                }
            }
            if any {
                let bar_size = chrome
                    .computed
                    .get(ui.root)
                    .map_or(Vec2::new(300.0, 50.0), |node| node.size());
                let center_x = f32::midpoint(min.x, max.x);
                let x = (center_x - bar_size.x * 0.5)
                    .clamp(4.0, (window.width() - bar_size.x - 4.0).max(4.0));
                let y = (min.y - bar_size.y - 6.0)
                    .clamp(4.0, (window.height() - bar_size.y - 4.0).max(4.0));
                if let Ok(mut node) = chrome.nodes.get_mut(ui.root) {
                    node.left = Val::Px(x);
                    node.top = Val::Px(y);
                }
            }
        }

        // ---- Chrome sync.
        let status = &slot.status;
        let mini = entry.controls == CONTROLS_MINI;
        let video = slot.kind == MediaEngineKind::Video;
        let playback = status.playback.as_ref();
        // Which rows / buttons this surface kind shows.
        set_display(&mut chrome.nodes, ui.url_row, !video && !mini);
        let seekable = playback
            .is_some_and(|playback| playback.seekable && playback.duration_seconds.is_some());
        set_display(&mut chrome.nodes, ui.scrub_row, video && seekable && !mini);
        set_display(&mut chrome.nodes, ui.play.0, video);
        set_display(&mut chrome.nodes, ui.back.0, !video);
        set_display(&mut chrome.nodes, ui.forward.0, !video);
        set_display(&mut chrome.nodes, ui.home, !video);
        if video {
            let playing = matches!(
                playback.map(|playback| playback.state),
                Some(PlaybackState::Playing | PlaybackState::Buffering)
            );
            set_state_class_on(&mut chrome.classes, ui.play.1, glyph::PLAYING, playing);
            if let Ok(mut time) = chrome.texts.get_mut(ui.time_text)
                && let Some(playback) = playback
            {
                let want = playback.duration_seconds.map_or_else(
                    || format_media_time(playback.position_seconds),
                    |duration| {
                        format!(
                            "{} / {}",
                            format_media_time(playback.position_seconds),
                            format_media_time(duration)
                        )
                    },
                );
                if time.0 != want {
                    time.0 = want;
                }
            }
        } else {
            set_label_enabled(&mut chrome.classes, ui.back.1, status.can_go_back);
            set_label_enabled(&mut chrome.classes, ui.forward.1, status.can_go_forward);
        }
        // The four toggling marks say only which state is true; which glyph
        // each state wears is the skin's.
        set_state_class_on(
            &mut chrome.classes,
            ui.reload_label,
            glyph::LOADING,
            !video && status.loading,
        );
        set_state_class_on(
            &mut chrome.classes,
            ui.mute_label,
            glyph::MUTED,
            slot.surface.muted(),
        );
        set_state_class_on(
            &mut chrome.classes,
            ui.zoom_label,
            glyph::ZOOMED,
            bar.state.zoomed == Some(target),
        );
        if let Ok(mut lock) = chrome.visibilities.get_mut(ui.lock) {
            let want = if !video && status.url.starts_with("https://") {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *lock != want {
                *lock = want;
            }
        }
        if !video
            && pointer.input_focus.get() != Some(ui.url_field)
            && let Ok(mut editor) = chrome.editors.get_mut(ui.url_field)
            && editor.value().to_string() != status.url
        {
            sl_viewer_ui_core::ui_text::set_editor_text(
                &mut editor,
                &status.url,
                &mut chrome.font_cx,
                &mut chrome.layout_cx,
            );
        }
        if let Ok(mut text) = chrome.texts.get_mut(ui.status_text) {
            // Video surfaces put their "now playing" title (or their loud
            // decoder-gap error) here; web surfaces their load progress. A
            // generic HTTP-source error is refined with the precise reason a
            // background probe recovers (GStreamer hides DNS / TCP / TLS / HTTP
            // causes — see [`sl_viewer_media::media_diagnostics`]).
            let want = if video {
                let load_error = status.load_error.clone().map(|generic| {
                    if status.network_diagnosable {
                        bar.diagnostics.request(&status.url);
                        bar.diagnostics
                            .reason(&status.url)
                            .map_or(generic, String::from)
                    } else {
                        generic
                    }
                });
                load_error.unwrap_or_else(|| {
                    if playback.is_some_and(|playback| playback.state == PlaybackState::Buffering) {
                        playback
                            .and_then(|playback| playback.buffering_percent)
                            .map_or_else(String::new, |percent| format!("{percent}%"))
                    } else {
                        status.title.clone()
                    }
                })
            } else if status.loading {
                format!("{:.0}%", status.progress * 100.0)
            } else {
                String::new()
            };
            if text.0 != want {
                text.0 = want;
            }
        }
    }

    if !show {
        bar.state.target = None;
    }
    if let Ok(mut shown) = chrome.shown_panels.get_mut(ui.root)
        && shown.0 != show
    {
        shown.0 = show;
    }
}

/// Mark an enable-gated button label, so the skin greys it.
fn set_label_enabled(classes: &mut Query<&mut ClassList>, label: Entity, enabled: bool) {
    if let Ok(mut list) = classes.get_mut(label) {
        set_state_class(&mut list, DISABLED_TEXT_CLASS, !enabled);
    }
}

/// Show or hide a node via its `display` property.
fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    if let Ok(mut node) = nodes.get_mut(entity) {
        let want = if shown { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

/// `Enter` in the bar's URL field: white-list-check the typed URL, navigate
/// the surface, and broadcast the navigation to the region (the reference's
/// shared MoaP navigation via `ObjectMediaNavigate`).
fn on_media_url_key(
    event: On<FocusedInput<KeyboardInput>>,
    editors: Query<&EditableText>,
    ui: Option<Res<MediaControlsUi>>,
    bar_state: Res<MediaControlsState>,
    media: MediaBarState,
    mut commands: MessageWriter<SlCommand>,
) {
    if !event.input.state.is_pressed() || event.input.key_code != KeyCode::Enter {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let Some(target) = bar_state.target else {
        return;
    };
    let Ok(editor) = editors.get(ui.url_field) else {
        return;
    };
    let Some(url) = normalize_web_url(&editor.value().to_string()) else {
        return;
    };
    let Some(entry) = media.data.entry(target) else {
        return;
    };
    let Ok(parsed) = url::Url::parse(&url) else {
        return;
    };
    if !entry.check_candidate_url(&parsed) {
        warn!("media white-list rejects {url}");
        return;
    }
    // The typed URL goes to the prim's surface *and* to the grid, where every
    // other agent's viewer will pick it up: it passes the same scheme
    // allowlist as a URL that arrives from the grid.
    let Ok(validated) = ValidatedMediaUrl::from_url(&parsed)
        .inspect_err(|error| warn!("media URL not navigated: {error}"))
    else {
        return;
    };
    if let Some(active) = media.prim_state.active.get(&target)
        && let Some(slot) = media.surfaces.get(active.surface)
    {
        slot.surface.navigate(&validated);
    }
    if let Ok(face) = u8::try_from(target.face.get()) {
        commands.write(SlCommand(Command::NavigateObjectMedia {
            object_id: target.object,
            face,
            url: url.clone(),
        }));
        commands.write(SlCommand(Command::RequestObjectMedia {
            object_id: target.object,
        }));
    }
}

/// Route the bar's button [`UiAction`]s.
fn handle_media_control_actions(
    mut actions: MessageReader<UiAction>,
    mut bar_state: ResMut<MediaControlsState>,
    media: MediaBarState,
    lookups: ZoomLookups,
    mut cameras: Query<(&Projection, &GlobalTransform, &mut CameraRig), With<ViewerCamera>>,
    mut camera_focus: ResMut<FocusTarget>,
) {
    for action in actions.read() {
        if action.element != MEDIA_CONTROLS_ELEMENT {
            continue;
        }
        let Some(target) = bar_state.target else {
            continue;
        };
        let Some(active) = media.prim_state.active.get(&target) else {
            continue;
        };
        let Some(slot) = media.surfaces.get(active.surface) else {
            continue;
        };
        match action.action {
            "back" => slot.surface.go_back(),
            "forward" => slot.surface.go_forward(),
            "play-pause" => {
                let playing = slot.status.playback.as_ref().is_some_and(|playback| {
                    matches!(
                        playback.state,
                        PlaybackState::Playing | PlaybackState::Buffering | PlaybackState::Loading
                    )
                });
                if playing {
                    slot.surface.pause();
                } else {
                    slot.surface.play();
                }
            }
            "home" => {
                if let Some(home) = media
                    .data
                    .entry(target)
                    .and_then(|entry| entry.home_url.as_ref())
                    .and_then(|home| {
                        ValidatedMediaUrl::from_url(home)
                            .inspect_err(|error| warn!("media home URL not opened: {error}"))
                            .ok()
                    })
                {
                    slot.surface.navigate(&home);
                }
            }
            "reload-or-stop" => {
                if slot.kind == MediaEngineKind::Web && slot.status.loading {
                    slot.surface.stop();
                } else {
                    slot.surface.reload();
                }
            }
            "mute-toggle" => slot.surface.set_muted(!slot.surface.muted()),
            // The page's *current* URL, which the page chose by navigating:
            // remote data, filtered at the sink like any other.
            "open-external" => {
                if let Ok(url) = ExternalUrl::parse(&slot.status.url).inspect_err(|error| {
                    warn!("media page not opened in the system browser: {error}");
                }) {
                    open_in_system_browser(&url);
                }
            }
            "zoom-toggle" => {
                if bar_state.zoomed == Some(target) {
                    *camera_focus = FocusTarget::Avatar;
                    bar_state.zoomed = None;
                } else if let Ok(face) = lookups.face_geometry.get(active.face_entity)
                    && let Ok(camera) = cameras.single_mut()
                    && zoom_camera_onto(
                        face,
                        camera,
                        (
                            ZoomView::HoverNormal(media.focus.hover_normal),
                            lookups.media_down(active.face_entity),
                        ),
                        &mut camera_focus,
                    )
                {
                    bar_state.zoomed = Some(target);
                }
            }
            _ => {}
        }
    }
}

/// Which way a zoom looks at the face.
#[derive(Debug, Clone, Copy)]
enum ZoomView {
    /// Along the normal at the last hover hit — when it faces the camera; a
    /// hit on the far side of a double-sided face would put the camera behind
    /// it, so otherwise straight from where the camera is.
    HoverNormal(Option<Vec3>),
    /// Along the face's own normal, whatever side the camera is on — the
    /// reference's `getApproximateFaceNormal`, for a zoom that has no hover hit
    /// (the Nearby Media window's). Looking from the camera instead gave a
    /// tilted face, a ramp, a flat side-on view.
    FaceNormal(Vec3),
}

/// Park the camera squarely in front of a face so its largest extent fills the
/// view — `LLViewerMediaFocus::setCameraZoom`'s geometry, simplified, looking
/// as `view` says. With the page's `down` direction (world space, see
/// [`ZoomLookups::media_down`]) the camera also rolls so the page's bottom is
/// the screen's; the reference keeps world up, which turns a page on a tilted
/// face sideways. Returns whether it moved (a degenerate direction leaves it
/// where it was).
fn zoom_camera_onto(
    (aabb, transform): (&Aabb, &GlobalTransform),
    (projection, camera_transform, mut rig): (&Projection, &GlobalTransform, Mut<CameraRig>),
    (view, down): (ZoomView, Option<Vec3>),
    camera_focus: &mut FocusTarget,
) -> bool {
    let center = transform.transform_point(Vec3::from(aabb.center));
    let world_half = Vec3::from(aabb.half_extents);
    let scale = transform.scale();
    let extent = (world_half.x * scale.x.abs())
        .max(world_half.y * scale.y.abs())
        .max(world_half.z * scale.z.abs())
        .max(0.1);
    let fov = match projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => core::f32::consts::FRAC_PI_4,
    };
    // Distance so the face's largest extent fills the view at a slight padding
    // (the reference's ZOOM_MEDIUM, padding 1.1).
    let distance = (extent * 1.1) / (fov * 0.5).tan();
    let towards_camera = Vec3::new(
        camera_transform.translation().x - center.x,
        camera_transform.translation().y - center.y,
        camera_transform.translation().z - center.z,
    );
    let normal = match view {
        ZoomView::HoverNormal(normal) => normal
            .filter(|normal| normal.dot(towards_camera) > 0.0)
            .unwrap_or(towards_camera),
        ZoomView::FaceNormal(normal) => normal,
    }
    .normalize_or_zero();
    if normal == Vec3::ZERO {
        return false;
    }
    let offset = Vec3::new(
        normal.x * distance,
        normal.y * distance,
        normal.z * distance,
    );
    // The page's up, flattened into the plane the camera sees it in; a page
    // seen edge-on along its own up has none, and keeps world up.
    let up = down.and_then(|down| {
        let up = Vec3::new(-down.x, -down.y, -down.z);
        let along = up.dot(normal);
        let flat = Vec3::new(
            up.x - normal.x * along,
            up.y - normal.y * along,
            up.z - normal.z * along,
        );
        flat.try_normalize()
    });
    match up {
        Some(up) => rig.set_point_view(offset, up),
        None => rig.set_point_offset(offset),
    }
    *camera_focus = FocusTarget::Point(center);
    true
}

/// Which way is down on a page drawn over a mesh, in the mesh's own space: the
/// sum over its triangles of `∂P/∂v` — the direction the sampled `v` grows,
/// `v` being each vertex's UV after `placement` — each weighted by its UV area.
/// Zero when no triangle has a UV extent.
fn page_down(
    positions: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[usize],
    placement: bevy::math::Affine2,
) -> Vec3 {
    let corner = |index: usize| -> Option<(Vec3, Vec2)> {
        let position = Vec3::from_array(*positions.get(index)?);
        let uv = placement.transform_point2(Vec2::from_array(*uvs.get(index)?));
        Some((position, uv))
    };
    let mut down = Vec3::ZERO;
    for &[a, b, c] in indices.as_chunks::<3>().0 {
        let (Some((p0, t0)), Some((p1, t1)), Some((p2, t2))) = (corner(a), corner(b), corner(c))
        else {
            continue;
        };
        let e1 = sl_viewer_world_api::vsub(p1, p0);
        let e2 = sl_viewer_world_api::vsub(p2, p0);
        let (du1, dv1) = (t1.x - t0.x, t1.y - t0.y);
        let (du2, dv2) = (t2.x - t0.x, t2.y - t0.y);
        let det = du1.mul_add(dv2, -(du2 * dv1));
        if det.abs() <= f32::EPSILON {
            continue;
        }
        // ∂P/∂v = (du1·e2 − du2·e1) / det; times |det| it is this.
        let gradient = sl_viewer_world_api::vscale(
            sl_viewer_world_api::vsub(
                sl_viewer_world_api::vscale(e2, du1),
                sl_viewer_world_api::vscale(e1, du2),
            ),
            det.signum(),
        );
        down = Vec3::new(
            down.x + gradient.x,
            down.y + gradient.y,
            down.z + gradient.z,
        );
    }
    down
}

/// The lookups a [`MediaZoomRequest`] resolves its face through, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct ZoomLookups<'w, 's> {
    /// The live surfaces — a playing face's entity is the one it wears.
    prim_state: Res<'w, MediaPrimState>,
    /// The object model, for a face that is not playing.
    objects: Res<'w, ObjectState>,
    /// The prim faces, to find that one's entity.
    faces: Query<
        'w,
        's,
        (
            &'static sl_viewer_world_objects::objects::PrimFaceEntity,
            &'static sl_viewer_world_objects::objects::FaceTextureDebug,
        ),
    >,
    /// The face's bounds and placement.
    face_geometry: Query<'w, 's, (&'static Aabb, &'static GlobalTransform)>,
    /// The face's mesh, for its normal.
    face_meshes: Query<'w, 's, &'static Mesh3d>,
    /// The meshes (prim faces keep theirs on the CPU).
    meshes: Res<'w, Assets<Mesh>>,
}

impl ZoomLookups<'_, '_> {
    /// Which way is **down** on the page `entity` shows, in world space: the
    /// direction the sampled `v` grows along the face (`ATTRIBUTE_UV_0` is
    /// top-down, row 0 of the media image at `v = 0`), after the face's own
    /// texture placement (repeats / offset / rotation — the media material's
    /// `uv_transform`). Summed over the face's triangles, each `∂P/∂v` weighted
    /// by its UV area. `None` without a mesh, UVs or a direction.
    fn media_down(&self, entity: Entity) -> Option<Vec3> {
        let mesh = self.meshes.get(&self.face_meshes.get(entity).ok()?.0)?;
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return None;
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            return None;
        };
        let placement = self
            .faces
            .get(entity)
            .map(
                |(_face, sl_viewer_world_objects::objects::FaceTextureDebug(tf))| {
                    sl_client_bevy::texture_face_uv_transform(tf)
                },
            )
            .unwrap_or_default();
        let indices: Vec<usize> = mesh.indices()?.iter().collect();
        let down = page_down(positions, uvs, &indices, placement);
        let (_aabb, transform) = self.face_geometry.get(entity).ok()?;
        // A surface direction goes out by the transform's linear part itself
        // (only a normal needs the inverse-transpose).
        let world = Vec3::from(
            transform
                .affine()
                .matrix3
                .mul_vec3a(bevy::math::Vec3A::from(down)),
        );
        world.try_normalize()
    }

    /// `entity`'s face normal in world space: its mesh's vertex normals
    /// averaged and carried out by the inverse-transpose of its transform (a
    /// prim's scale is non-uniform). `None` without a mesh, normals, or a
    /// direction (a face whose normals cancel out).
    fn face_normal(&self, entity: Entity) -> Option<Vec3> {
        let mesh = self.meshes.get(&self.face_meshes.get(entity).ok()?.0)?;
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            return None;
        };
        let sum = normals.iter().fold(Vec3::ZERO, |sum, [x, y, z]| {
            Vec3::new(sum.x + x, sum.y + y, sum.z + z)
        });
        let (_aabb, transform) = self.face_geometry.get(entity).ok()?;
        let linear = transform.affine().matrix3;
        let world = linear
            .inverse()
            .transpose()
            .mul_vec3a(bevy::math::Vec3A::from(sum));
        let world = Vec3::from(world).normalize_or_zero();
        (world != Vec3::ZERO && world.is_finite()).then_some(world)
    }
}

/// Carry out the Nearby Media window's zoom / unzoom.
fn handle_media_zoom_requests(
    mut requests: MessageReader<MediaZoomRequest>,
    lookups: ZoomLookups,
    mut bar_state: ResMut<MediaControlsState>,
    mut focus: ResMut<MediaFocus>,
    mut cameras: Query<(&Projection, &GlobalTransform, &mut CameraRig), With<ViewerCamera>>,
    mut camera_focus: ResMut<FocusTarget>,
) {
    for request in requests.read() {
        let Some(target) = request.target else {
            if bar_state.zoomed.take().is_some() {
                *camera_focus = FocusTarget::Avatar;
            }
            continue;
        };
        let active = lookups.prim_state.active.get(&target);
        let face_entity = active.map(|active| active.face_entity).or_else(|| {
            crate::media_prim::resolve_face_entity(&lookups.objects, target, &lookups.faces)
        });
        let Some(entity) = face_entity else {
            continue;
        };
        let Ok(face) = lookups.face_geometry.get(entity) else {
            continue;
        };
        let Ok(camera) = cameras.single_mut() else {
            continue;
        };
        // No hover hit to take a normal from: the face's own, as the
        // reference's `focusZoomOnMedia` does.
        let view = lookups
            .face_normal(entity)
            .map_or(ZoomView::HoverNormal(None), ZoomView::FaceNormal);
        if zoom_camera_onto(
            face,
            camera,
            (view, lookups.media_down(entity)),
            &mut camera_focus,
        ) {
            bar_state.zoomed = Some(target);
            focus.focused = Some(target);
            focus.focused_takes_keyboard =
                active.is_some_and(|active| active.kind == MediaEngineKind::Web);
        }
    }
}

/// Dropping media focus (`Escape`, or the face going away) unzooms, matching
/// the reference's `ESC` behaviour.
fn unzoom_on_focus_loss(
    focus: Res<MediaFocus>,
    prim_state: Res<MediaPrimState>,
    mut bar_state: ResMut<MediaControlsState>,
    mut camera_focus: ResMut<FocusTarget>,
) {
    let Some(zoomed) = bar_state.zoomed else {
        return;
    };
    // A face zoomed onto from the Nearby Media window need not be playing, so
    // "gone" is gone from the driver's list, not merely not live.
    let face_gone = !prim_state.active.contains_key(&zoomed)
        && !prim_state.nearby.iter().any(|face| face.target == zoomed);
    let focus_left = focus.focused != Some(zoomed) && focus.hover != Some(zoomed);
    if face_gone || (focus_left && focus.focused.is_none() && bar_state.target.is_none()) {
        *camera_focus = FocusTarget::Avatar;
        bar_state.zoomed = None;
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{Affine2, Vec2, Vec3};
    use pretty_assertions::assert_eq;

    use super::page_down;

    /// A unit quad in the XY plane whose UV `v` runs top-down along −Y: down
    /// on the page is −Y. The face's texture rotation turns it with the page.
    #[test]
    fn page_down_follows_the_uvs_and_the_texture_rotation() {
        let positions = [
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let indices = [0, 1, 2, 0, 2, 3];

        let down = page_down(&positions, &uvs, &indices, Affine2::IDENTITY)
            .try_normalize()
            .unwrap_or(Vec3::ZERO);
        assert!(down.abs_diff_eq(Vec3::NEG_Y, 1e-5), "{down}");

        // A quarter turn of the texture about its centre: the page's down now
        // runs along the quad's X axis.
        let quarter = Affine2::from_translation(Vec2::splat(0.5))
            * Affine2::from_angle(core::f32::consts::FRAC_PI_2)
            * Affine2::from_translation(Vec2::splat(-0.5));
        let turned = page_down(&positions, &uvs, &indices, quarter)
            .try_normalize()
            .unwrap_or(Vec3::ZERO);
        assert!(turned.y.abs() < 1e-5 && turned.x.abs() > 0.99, "{turned}");
    }

    /// Degenerate UVs give no direction rather than a garbage one.
    #[test]
    fn page_down_is_zero_without_uv_extent() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let uvs = [[0.5, 0.5]; 3];
        assert_eq!(
            page_down(&positions, &uvs, &[0, 1, 2], Affine2::IDENTITY),
            Vec3::ZERO
        );
    }
}
