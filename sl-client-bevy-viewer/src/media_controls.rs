//! The **floating media controls bar** (`viewer-media-prim-browser`): the
//! reference viewer's `LLPanelPrimMediaControls` — a small button bar hovering
//! above the media face under the cursor (or holding focus), with back /
//! forward / home / stop-or-reload / URL / mute / zoom / open-external
//! controls, a load progress read-out and the secure-lock marker (the Vintage
//! skin's `panel_prim_media_controls.xml` web-mode set; the movie scrubber
//! belongs to the separate video-playback task).
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
//! the avatar. `Escape` (which also drops media focus) unzooms too.

use bevy::camera::primitives::Aabb;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui_widgets::{Activate, Button};
use sl_client_bevy::{Command, SlCommand};

use crate::camera::{CameraRig, FocusTarget, ViewerCamera};
use crate::media_engine::{MediaEngineSystems, MediaSurfaces};
use crate::media_prim::{
    MediaData, MediaFocus, MediaPrimState, MediaTarget, media_permission_allows,
};
use crate::objects::ObjectState;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::web_floater::{normalize_web_url, open_in_system_browser};

/// The [`UiAction`] element name of the bar.
pub(crate) const MEDIA_CONTROLS_ELEMENT: &str = "media-controls";

/// Seconds without pointer activity before the bar hides (the reference's
/// `MediaControlTimeout`).
const INACTIVITY_HIDE_SECONDS: f32 = 3.0;

/// The `FLAGS_OBJECT_YOU_OWNER` update-flags bit (the agent owns the object).
const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// `MediaEntry::controls` value for the reduced (mini) control set.
const CONTROLS_MINI: i32 = 1;

/// Bar text colour.
const BAR_LABEL: Color = Color::srgb(0.9, 0.9, 0.92);
/// Bar text colour for unavailable actions.
const BAR_LABEL_DIM: Color = Color::srgb(0.45, 0.45, 0.5);

/// The bar's entities.
#[derive(Resource)]
struct MediaControlsUi {
    /// The bar root (absolute-positioned, shown/hidden).
    root: Entity,
    /// The URL field (hidden for mini controls).
    url_field: Entity,
    /// The URL row wrapper (hidden with the field).
    url_row: Entity,
    /// Back button label.
    back_label: Entity,
    /// Forward button label.
    forward_label: Entity,
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
}

/// Which media face the bar currently controls, plus the zoom state.
#[derive(Resource, Debug, Default)]
pub(crate) struct MediaControlsState {
    /// The face the bar is shown for.
    target: Option<MediaTarget>,
    /// Seconds since the last pointer activity.
    idle: f32,
    /// The face the camera is currently zoomed onto, if any.
    zoomed: Option<MediaTarget>,
}

/// The floating media-controls plugin.
pub(crate) struct MediaControlsPlugin;

impl Plugin for MediaControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MediaControlsState>()
            .add_systems(
                Startup,
                spawn_media_controls.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    update_media_controls,
                    handle_media_control_actions,
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
    let back_label = spawn_bar_button(&mut commands, buttons, "◀", "back", 30);
    let forward_label = spawn_bar_button(&mut commands, buttons, "▶", "forward", 31);
    let _home = spawn_bar_button(&mut commands, buttons, "⌂", "home", 32);
    let reload_label = spawn_bar_button(&mut commands, buttons, "⟳", "reload-or-stop", 33);
    let mute_label = spawn_bar_button(&mut commands, buttons, "🔊", "mute-toggle", 34);
    let zoom_label = spawn_bar_button(&mut commands, buttons, "⊕", "zoom-toggle", 35);
    let _external = spawn_bar_button(&mut commands, buttons, "↗", "open-external", 36);
    let status_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(11.0),
            TextColor(BAR_LABEL_DIM),
            ChildOf(buttons),
        ))
        .id();

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
            Text::new("🔒"),
            UiFont::Sans.at(11.0),
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
        back_label,
        forward_label,
        reload_label,
        mute_label,
        zoom_label,
        status_text,
        lock,
    });
}

/// One glyph button on the bar; returns the label entity.
fn spawn_bar_button(
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
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.3, 0.3, 0.35)),
            BackgroundColor(Color::srgb(0.16, 0.17, 0.2)),
            Pickable::default(),
            Name::new(format!("media-controls-button:{action}")),
            ChildOf(parent),
        ))
        .observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: MEDIA_CONTROLS_ELEMENT,
                    action,
                });
            },
        )
        .id();
    commands
        .spawn((
            Text::new(glyph),
            UiFont::Sans.at(12.0),
            TextColor(BAR_LABEL),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id()
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
    /// Enable-gated label colours.
    colors: Query<'w, 's, &'static mut TextColor>,
    /// The URL field.
    editors: Query<'w, 's, &'static mut EditableText>,
    /// The secure-lock glyph's visibility.
    visibilities: Query<'w, 's, &'static mut Visibility>,
    /// The font context for programmatic text replacement.
    font_cx: ResMut<'w, FontCx>,
    /// The layout context for programmatic text replacement.
    layout_cx: ResMut<'w, LayoutCx>,
}

/// Show / place / sync the bar every frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the media \
              state feeding the bar, the projection inputs (camera, face transforms), and the \
              bundled chrome queries"
)]
fn update_media_controls(
    ui: Option<Res<MediaControlsUi>>,
    mut bar_state: ResMut<MediaControlsState>,
    focus: Res<MediaFocus>,
    data: Res<MediaData>,
    prim_state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
    objects: Res<ObjectState>,
    time: Res<Time>,
    mut cursor_moves: MessageReader<bevy::window::CursorMoved>,
    mouse: Res<ButtonInput<MouseButton>>,
    input_focus: Res<InputFocus>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    face_geometry: Query<(&Aabb, &GlobalTransform)>,
    windows: Query<&Window>,
    mut chrome: BarChrome,
) {
    let Some(ui) = ui else {
        return;
    };
    // Pointer activity feeds the inactivity timer.
    let moved = cursor_moves.read().next().is_some();
    if moved || mouse.get_just_pressed().next().is_some() {
        bar_state.idle = 0.0;
    } else {
        bar_state.idle += time.delta_secs();
    }

    // Which face the bar serves: focus first, then hover; keep the current
    // target while the cursor is over the bar itself (hover = None then).
    let target = focus
        .focused
        .or(focus.hover)
        .or(if bar_state.idle < INACTIVITY_HIDE_SECONDS {
            bar_state.target
        } else {
            None
        });

    let mut show = false;
    'decide: {
        let Some(target) = target else {
            break 'decide;
        };
        let Some(entry) = data.entry(target) else {
            break 'decide;
        };
        let Some(active) = prim_state.active.get(&target) else {
            break 'decide;
        };
        let Some(slot) = surfaces.get(active.surface) else {
            break 'decide;
        };
        let is_owner = objects
            .update_flags_by_key(target.object)
            .is_some_and(|flags| flags & FLAGS_OBJECT_YOU_OWNER != 0);
        if !media_permission_allows(entry.perms_control, is_owner) {
            break 'decide;
        }
        if bar_state.idle >= INACTIVITY_HIDE_SECONDS {
            break 'decide;
        }
        bar_state.target = Some(target);
        show = true;

        // ---- Placement: project the face's box, sit above its top edge.
        if let Ok((camera, camera_transform)) = cameras.single()
            && let Ok((aabb, face_transform)) = face_geometry.get(active.face_entity)
            && let Ok(window) = windows.single()
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
                let center_x = (min.x + max.x) * 0.5;
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
        if let Ok(mut node) = chrome.nodes.get_mut(ui.url_row) {
            let want = if mini { Display::None } else { Display::Flex };
            if node.display != want {
                node.display = want;
            }
        }
        set_color(&mut chrome.colors, ui.back_label, status.can_go_back);
        set_color(&mut chrome.colors, ui.forward_label, status.can_go_forward);
        if let Ok(mut reload) = chrome.texts.get_mut(ui.reload_label) {
            let want = if status.loading { "✕" } else { "⟳" };
            if reload.0 != want {
                want.clone_into(&mut reload.0);
            }
        }
        if let Ok(mut mute) = chrome.texts.get_mut(ui.mute_label) {
            let want = if slot.surface.muted() { "🔇" } else { "🔊" };
            if mute.0 != want {
                want.clone_into(&mut mute.0);
            }
        }
        if let Ok(mut zoom) = chrome.texts.get_mut(ui.zoom_label) {
            let want = if bar_state.zoomed == Some(target) {
                "⊖"
            } else {
                "⊕"
            };
            if zoom.0 != want {
                want.clone_into(&mut zoom.0);
            }
        }
        if let Ok(mut lock) = chrome.visibilities.get_mut(ui.lock) {
            let want = if status.url.starts_with("https://") {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *lock != want {
                *lock = want;
            }
        }
        if input_focus.get() != Some(ui.url_field)
            && let Ok(mut editor) = chrome.editors.get_mut(ui.url_field)
            && editor.value().to_string() != status.url
        {
            crate::web_floater::set_editor_text(
                &mut editor,
                &status.url,
                &mut chrome.font_cx,
                &mut chrome.layout_cx,
            );
        }
        if let Ok(mut text) = chrome.texts.get_mut(ui.status_text) {
            let want = if status.loading {
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
        bar_state.target = None;
    }
    if let Ok(mut shown) = chrome.shown_panels.get_mut(ui.root)
        && shown.0 != show
    {
        shown.0 = show;
    }
}

/// Recolour an enable-gated button label.
fn set_color(colors: &mut Query<&mut TextColor>, label: Entity, enabled: bool) {
    if let Ok(mut color) = colors.get_mut(label) {
        let want = if enabled { BAR_LABEL } else { BAR_LABEL_DIM };
        if color.0 != want {
            color.0 = want;
        }
    }
}

/// `Enter` in the bar's URL field: white-list-check the typed URL, navigate
/// the surface, and broadcast the navigation to the region (the reference's
/// shared MoaP navigation via `ObjectMediaNavigate`).
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected resources / queries: the key event, \
              the field, the bar / media state, the surface table and the command channel"
)]
fn on_media_url_key(
    event: On<FocusedInput<KeyboardInput>>,
    editors: Query<&EditableText>,
    ui: Option<Res<MediaControlsUi>>,
    bar_state: Res<MediaControlsState>,
    data: Res<MediaData>,
    prim_state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
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
    let Some(entry) = data.entry(target) else {
        return;
    };
    let Ok(parsed) = url::Url::parse(&url) else {
        return;
    };
    if !entry.check_candidate_url(&parsed) {
        warn!("media white-list rejects {url}");
        return;
    }
    if let Some(active) = prim_state.active.get(&target)
        && let Some(slot) = surfaces.get(active.surface)
    {
        slot.surface.navigate(&url);
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
#[expect(
    clippy::too_many_arguments,
    reason = "threaded resources: the action stream, the bar / media state, the surface \
              table, and the camera pieces the zoom drives"
)]
fn handle_media_control_actions(
    mut actions: MessageReader<UiAction>,
    mut bar_state: ResMut<MediaControlsState>,
    focus: Res<MediaFocus>,
    data: Res<MediaData>,
    prim_state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
    face_geometry: Query<(&Aabb, &GlobalTransform)>,
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
        let Some(active) = prim_state.active.get(&target) else {
            continue;
        };
        let Some(slot) = surfaces.get(active.surface) else {
            continue;
        };
        match action.action {
            "back" => slot.surface.go_back(),
            "forward" => slot.surface.go_forward(),
            "home" => {
                if let Some(home) = data.entry(target).and_then(|entry| entry.home_url.as_ref()) {
                    slot.surface.navigate(home.as_str());
                }
            }
            "reload-or-stop" => {
                if slot.status.loading {
                    slot.surface.stop();
                } else {
                    slot.surface.reload();
                }
            }
            "mute-toggle" => slot.surface.set_muted(!slot.surface.muted()),
            "open-external" => open_in_system_browser(&slot.status.url),
            "zoom-toggle" => {
                if bar_state.zoomed == Some(target) {
                    *camera_focus = FocusTarget::Avatar;
                    bar_state.zoomed = None;
                } else if let Ok((aabb, transform)) = face_geometry.get(active.face_entity)
                    && let Ok((projection, camera_transform, mut rig)) = cameras.single_mut()
                {
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
                    // Distance so the face's largest extent fills the view at a
                    // slight padding (the reference's ZOOM_MEDIUM, padding 1.1).
                    let distance = (extent * 1.1) / (fov * 0.5).tan();
                    let towards_camera = Vec3::new(
                        camera_transform.translation().x - center.x,
                        camera_transform.translation().y - center.y,
                        camera_transform.translation().z - center.z,
                    );
                    let normal = focus
                        .hover_normal
                        .filter(|normal| normal.dot(towards_camera) > 0.0)
                        .unwrap_or(towards_camera)
                        .normalize_or_zero();
                    if normal != Vec3::ZERO {
                        rig.set_point_offset(Vec3::new(
                            normal.x * distance,
                            normal.y * distance,
                            normal.z * distance,
                        ));
                        *camera_focus = FocusTarget::Point(center);
                        bar_state.zoomed = Some(target);
                    }
                }
            }
            _ => {}
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
    let face_gone = !prim_state.active.contains_key(&zoomed);
    let focus_left = focus.focused != Some(zoomed) && focus.hover != Some(zoomed);
    if face_gone || (focus_left && focus.focused.is_none() && bar_state.target.is_none()) {
        *camera_focus = FocusTarget::Avatar;
        bar_state.zoomed = None;
    }
}
