//! The **virtual trackball** (`viewer-ui-virtual-trackball`): aim the sun or the
//! moon by pointing at a hemisphere — the reference's `LLVirtualTrackball`.
//!
//! # What it is, and what it is not
//!
//! The state behind it is two angles, and the two angles already have two
//! sliders. So this widget adds no reachable value: it is a second, *spatial*
//! way to drive fields the environment editors drive today, which is why the
//! reference keeps the azimuth / elevation spinners beside every trackball and
//! writes each from the other. Picking a sun position off a hemisphere is a
//! different act from typing two numbers, and for photography it is the better
//! one.
//!
//! # The projection
//!
//! The control is a disc: the sky's hemisphere seen from **directly above**, so
//! a direction's horizontal part is its position on the disc and its height is
//! how far in from the rim it sits. North is up and east is to the right, as on
//! a map — so the disc does **not** mirror in a right-to-left layout, and it is
//! laid out by absolute inset from a compass angle rather than through taffy's
//! `direction`, exactly as the pie menu is (see
//! [`apply_ui_direction`](sl_viewer_ui_core::ui::apply_ui_direction)).
//!
//! With the angle convention `sl_proto::rotation_to_azimuth_altitude` returns —
//! azimuth measured from due **east** toward north, altitude above the horizon —
//! the projection is [`aim_to_disc`], one cosine each way, and its inverse is
//! [`disc_to_aim`].
//!
//! The disc can only say how far from the zenith a direction is, never which
//! side of the horizon it is on: a sun 30° up and a sun 30° down land on the
//! same point. So the marker carries the answer instead — filled above the
//! horizon, hollow below it — and a drag **keeps the hemisphere it started in**,
//! as the reference's does. Crossing the horizon is the keyboard's job (or the
//! elevation slider's).
//!
//! # Divergences from the reference, and why
//!
//! - **No four rotate buttons round the rim.** The reference wraps the disc in
//!   top / bottom / left / right buttons that roll the direction by 3° about a
//!   world axis, and maps the arrow keys onto them — inverted, so `KEY_DOWN`
//!   calls `onRotateTopClick`. Here the arrow keys step the two angles directly:
//!   left / right the compass angle, up / down the height. That is the same 3°
//!   increment, it is what the two sliders beside the widget would do, and it
//!   can cross the horizon (a world-axis roll can too, but only by carrying the
//!   azimuth 180° round with it).
//! - **Ctrl-drag does not switch to a rolling mode.** The reference's
//!   `DRAG_SCROLL` accumulates rotations about world axes from the pointer
//!   *delta* rather than aiming at its position. Its one advantage over aiming
//!   is that it can leave the hemisphere — which the keyboard here does without
//!   a hidden modifier.
//!
//! Reference (Firestorm, read-only): `llvirtualtrackball.cpp` / `.h`,
//! `widgets/virtual_trackball.xml`, and its two uses in
//! `floater_adjust_environment.xml` (`sun_rotation`, `moon_rotation`).

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::ValueChange;

use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui_font::UiFont;

/// The control's outer square, logical px. The aiming circle is inscribed in it
/// less a marker's width, so a marker centred on the rim still lies inside the
/// control's own box.
///
/// Private, and the whole control's geometry with it: a caller places the
/// control, and the disc inside it is the widget's business, exactly as a
/// slider's track height is `ui_color_picker`'s.
const TRACKBALL_SIZE: f32 = 100.0;

/// The marker's side, logical px.
const MARKER_SIZE: f32 = 11.0;

/// The aiming circle's radius, logical px — half the control less half a marker
/// at each end, so a marker centred on the rim still lies inside the control's
/// own box.
///
/// The slider thumb's argument, on a circle: a marker that reached the outer
/// edge would hang half its width outside its parent, which is both drawn over
/// whatever is beside the control and reported by the layout sweep's
/// containment check. The drawn disc is inset to the same circle, so where the
/// pointer aims, where the disc is drawn and where the marker travels are one
/// circle rather than three.
const RADIUS: f32 = (TRACKBALL_SIZE - MARKER_SIZE) / 2.0;

/// A compass letter's box, logical px.
const LABEL_BOX: f32 = 12.0;

/// A compass letter's font size, logical px.
const LABEL_FONT: f32 = 10.0;

/// How far inside the rim a compass letter sits, logical px.
const RIM_PAD: f32 = 1.0;

/// How many degrees an arrow key steps — the reference's
/// `increment_angle_btn`.
const NUDGE_DEGREES: f32 = 3.0;

/// The disc's fill above the horizon.
const DISC_FILL: Color = Color::srgba(0.10, 0.12, 0.17, 1.0);

/// The disc's fill below it — the reference draws the sphere at half intensity
/// when the body it holds is on the far side.
const DISC_FILL_BELOW: Color = Color::srgba(0.07, 0.08, 0.11, 1.0);

/// The disc's rim.
const DISC_BORDER: Color = Color::srgba(0.40, 0.44, 0.54, 1.0);

/// A disabled control's rim.
const DISABLED_BORDER: Color = Color::srgba(0.26, 0.28, 0.33, 1.0);

/// The compass letters' colour.
const LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// The sun marker's colour.
const SUN_COLOR: Color = Color::srgb(1.0, 0.85, 0.35);

/// The moon marker's colour.
const MOON_COLOR: Color = Color::srgb(0.82, 0.86, 0.95);

/// A marker's outline above the horizon — dark, so a pale marker reads against
/// a pale disc.
const MARKER_OUTLINE: Color = Color::srgba(0.06, 0.07, 0.10, 1.0);

/// A disabled marker's colour, whichever body it is.
const DISABLED_MARKER: Color = Color::srgb(0.42, 0.44, 0.48);

// ---------------------------------------------------------------------------
// The value.
// ---------------------------------------------------------------------------

/// Which celestial body a trackball aims — all the marker's look depends on.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackballBody {
    /// The sun: a warm filled marker.
    Sun,
    /// The moon: a pale one.
    Moon,
}

impl TrackballBody {
    /// The marker colour this body is drawn in.
    const fn color(self) -> Color {
        match self {
            Self::Sun => SUN_COLOR,
            Self::Moon => MOON_COLOR,
        }
    }

    /// The slug this body's control is named by.
    const fn slug(self) -> &'static str {
        match self {
            Self::Sun => "sun",
            Self::Moon => "moon",
        }
    }
}

/// Where a trackball is aimed: spherical angles in **degrees**, the units the
/// azimuth and elevation sliders beside it show.
///
/// The consumer owns this component as much as the widget does — writing it
/// re-places the marker, exactly as writing a
/// [`ColorSwatchValue`](crate::ui_color_picker::ColorSwatchValue) repaints a
/// swatch — and the widget writes it back as the user drags.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TrackballAim {
    /// The compass angle, `0.0..360.0`, measured from due **east** toward
    /// north. That is the sky asset's own convention (`+x` east, `+y` north),
    /// not the surveyor's — so `0.0` puts the marker on the disc's **E** and
    /// `90.0` on its **N**.
    pub azimuth: f32,
    /// The height above the horizon, `-90.0..=90.0`. Negative is below it.
    pub elevation: f32,
}

impl TrackballAim {
    /// Straight up: the aim a control with nothing to show yet opens on.
    pub const ZENITH: Self = Self {
        azimuth: 0.0,
        elevation: 90.0,
    };

    /// This aim with the azimuth wrapped into `0.0..360.0` and the elevation
    /// clamped to the horizon-to-pole range, which is the only form the widget
    /// ever emits.
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            azimuth: if self.azimuth.is_finite() {
                self.azimuth.rem_euclid(360.0)
            } else {
                0.0
            },
            elevation: self.elevation.clamp(-90.0, 90.0),
        }
    }

    /// Whether this aim is above the horizon. Exactly on it counts as above, as
    /// the reference's `draw_point.mV[VZ] >= 0.f` does.
    #[must_use]
    pub fn above_horizon(self) -> bool {
        self.elevation >= 0.0
    }

    /// The pair as a [`Vec2`] — azimuth in `x`, elevation in `y` — which is what
    /// a [`ValueChange`] carries.
    const fn as_vec2(self) -> Vec2 {
        Vec2::new(self.azimuth, self.elevation)
    }
}

/// Where the marker for `aim` sits on the **unit disc**: the centre is the
/// zenith, the rim is the horizon, `x` runs east and `y` north.
///
/// The hemisphere seen from above, orthographically: the horizontal part of a
/// unit direction is `cos(elevation)` long and points along the azimuth, so the
/// distance from the centre *is* the cosine of the height. A direction below the
/// horizon projects onto the same point as its mirror above it — see the module
/// documentation for what carries the difference.
#[must_use]
pub fn aim_to_disc(aim: TrackballAim) -> Vec2 {
    let azimuth = aim.azimuth.to_radians();
    let elevation = aim.elevation.to_radians();
    let ground = elevation.cos().clamp(0.0, 1.0);
    Vec2::new(azimuth.cos() * ground, azimuth.sin() * ground)
}

/// The aim a point on the unit disc means, given the aim it is replacing.
///
/// `None` outside the disc: the reference refuses to drag past the rim rather
/// than clamping to it (`handleHover`'s `pointInTouchCircle` guard), and holding
/// the last aim is a better answer than pinning the body to the horizon every
/// time a hand overshoots.
///
/// `current` decides two things the disc cannot say: which side of the horizon
/// the result is on (a drag keeps the hemisphere it started in), and — at the
/// exact centre, where every azimuth means the same direction — which azimuth to
/// keep, so passing the pointer over the zenith does not spin the compass.
#[must_use]
pub fn disc_to_aim(point: Vec2, current: TrackballAim) -> Option<TrackballAim> {
    let distance = point.length();
    if distance > 1.0 {
        return None;
    }
    let magnitude = distance.clamp(0.0, 1.0).acos().to_degrees();
    let elevation = if current.above_horizon() {
        magnitude
    } else {
        -magnitude
    };
    let azimuth = if distance <= f32::EPSILON {
        current.azimuth
    } else {
        point.y.atan2(point.x).to_degrees()
    };
    Some(TrackballAim { azimuth, elevation }.normalized())
}

// ---------------------------------------------------------------------------
// The widget.
// ---------------------------------------------------------------------------

/// The drawn disc under a trackball's marker.
#[derive(Component, Debug, Clone, Copy)]
struct TrackballDisc;

/// The marker showing where a trackball is aimed.
#[derive(Component, Debug, Clone, Copy)]
struct TrackballMarker;

/// One of a trackball's four compass letters.
#[derive(Component, Debug, Clone, Copy)]
struct TrackballLabel;

/// Whether a trackball is being dragged, so a release commits once.
#[derive(Component, Debug, Clone, Copy, Default)]
struct TrackballDrag {
    /// Set between a press inside the circle and the release that ends it.
    dragging: bool,
}

/// Spawn a trackball under `parent`, named `{element}-{body}:trackball`.
///
/// The returned entity carries [`TrackballAim`] and emits
/// [`ValueChange<Vec2>`] — azimuth in `x`, elevation in `y`, both degrees —
/// which a consumer observes exactly as it observes a slider's.
pub fn spawn_trackball(
    commands: &mut Commands,
    parent: Entity,
    element: &str,
    body: TrackballBody,
    tab_index: i32,
    initial: TrackballAim,
) -> Entity {
    let aim = initial.normalized();
    let trackball = commands
        .spawn((
            Node {
                width: Val::Px(TRACKBALL_SIZE),
                height: Val::Px(TRACKBALL_SIZE),
                // Nothing inside is in the flow: the disc, the four letters and
                // the marker are each placed by absolute inset from the centre,
                // which is what keeps a compass out of the layout direction's
                // reach.
                ..Default::default()
            },
            body,
            aim,
            TrackballDrag::default(),
            TabIndex(tab_index),
            Pickable::default(),
            Name::new(format!("{element}-{}:trackball", body.slug())),
            ChildOf(parent),
        ))
        .observe(on_trackball_press)
        .observe(on_trackball_drag)
        .observe(on_trackball_drag_end)
        .observe(on_trackball_release)
        .observe(on_trackball_cancel)
        .observe(on_trackball_key)
        .id();

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(MARKER_SIZE / 2.0),
            top: Val::Px(MARKER_SIZE / 2.0),
            width: Val::Px(RADIUS * 2.0),
            height: Val::Px(RADIUS * 2.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Percent(50.0)),
            ..Default::default()
        },
        BorderColor::all(DISC_BORDER),
        BackgroundColor(DISC_FILL),
        TrackballDisc,
        Pickable::IGNORE,
        Name::new(format!("{element}-{}:trackball-disc", body.slug())),
        ChildOf(trackball),
    ));

    for (key, offset) in [
        ("trackball-north", Vec2::new(0.0, 1.0)),
        ("trackball-east", Vec2::new(1.0, 0.0)),
        ("trackball-south", Vec2::new(0.0, -1.0)),
        ("trackball-west", Vec2::new(-1.0, 0.0)),
    ] {
        spawn_compass_label(commands, trackball, key, offset);
    }

    let at = marker_inset(aim);
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            width: Val::Px(MARKER_SIZE),
            height: Val::Px(MARKER_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Percent(50.0)),
            ..Default::default()
        },
        BorderColor::all(MARKER_OUTLINE),
        BackgroundColor(body.color()),
        TrackballMarker,
        Pickable::IGNORE,
        Name::new(format!("{element}-{}:trackball-marker", body.slug())),
        ChildOf(trackball),
    ));

    trackball
}

/// One compass letter, at the rim in the given unit-disc direction.
fn spawn_compass_label(
    commands: &mut Commands,
    trackball: Entity,
    key: &'static str,
    direction: Vec2,
) {
    // The letter's *box* is placed, not its centre, so it is inset by its own
    // size the same way the marker is — and by `RIM_PAD` further, to sit just
    // inside the rim rather than straddling it.
    let travel = RADIUS - LABEL_BOX / 2.0 - RIM_PAD;
    let centre = TRACKBALL_SIZE / 2.0;
    commands.spawn((
        Text::new(String::new()),
        Translated::new(key),
        TextLayout {
            justify: Justify::Center,
            linebreak: LineBreak::NoWrap,
        },
        UiFont::Sans.at(LABEL_FONT),
        TextColor(LABEL_COLOR),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(centre + direction.x * travel - LABEL_BOX / 2.0),
            top: Val::Px(centre - direction.y * travel - LABEL_BOX / 2.0),
            width: Val::Px(LABEL_BOX),
            ..Default::default()
        },
        TrackballLabel,
        Pickable::IGNORE,
        ChildOf(trackball),
    ));
}

/// Where a marker's **box** goes for an aim: its top-left corner in the
/// control's own logical pixels.
fn marker_inset(aim: TrackballAim) -> Vec2 {
    let disc = aim_to_disc(aim);
    let centre = TRACKBALL_SIZE / 2.0;
    Vec2::new(
        centre + disc.x * RADIUS - MARKER_SIZE / 2.0,
        // The disc's `y` runs north, which is *up* the screen.
        centre - disc.y * RADIUS - MARKER_SIZE / 2.0,
    )
}

/// The unit-disc point a pointer is over, or `None` if the control has no box
/// yet.
///
/// The aiming circle is [`RADIUS`] rather than half the control, so the
/// normalized position — which spans the whole control — is scaled back up to
/// the circle it is actually aiming at.
fn pointer_on_disc(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    target: &ComputedUiRenderTargetInfo,
    ui_scale: &UiScale,
    position: Vec2,
) -> Option<Vec2> {
    // Component-wise `f32` throughout: the workspace's arithmetic lint fires on
    // `glam`'s overloaded operators, and every scalar here is finite by
    // construction.
    let physical = target.scale_factor() / ui_scale.0;
    let normalized = node.normalize_point(
        *transform,
        Vec2::new(position.x * physical, position.y * physical),
    );
    // The normalized position spans the whole control; the circle it is aiming
    // at is `RADIUS`, so it is scaled back up to that. The `y` is flipped
    // because the UI's runs down the screen and the disc's runs north.
    let gain = TRACKBALL_SIZE / RADIUS;
    normalized.map(|point| Vec2::new(point.x * gain, -point.y * gain))
}

/// What every pointer handler needs to turn a screen position into an aim.
type TrackballQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static TrackballAim,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static ComputedUiRenderTargetInfo,
    ),
>;

/// Aim a trackball at a pointer position and tell the world, if the position is
/// on the disc at all.
fn aim_at_pointer(
    entity: Entity,
    position: Vec2,
    is_final: bool,
    balls: &TrackballQuery,
    ui_scale: &UiScale,
    commands: &mut Commands,
) -> Option<TrackballAim> {
    let Ok((aim, node, transform, target)) = balls.get(entity) else {
        return None;
    };
    let point = pointer_on_disc(node, transform, target, ui_scale, position)?;
    let aimed = disc_to_aim(point, *aim)?;
    commands.entity(entity).insert(aimed);
    commands.trigger(ValueChange {
        source: entity,
        value: aimed.as_vec2(),
        is_final,
    });
    Some(aimed)
}

/// A press inside the circle starts a drag and aims at once — the reference's
/// click-to-set. A press in the control's corners, outside the circle, does
/// nothing at all, as `pointInTouchCircle` refuses to capture there.
fn on_trackball_press(
    mut press: On<Pointer<Press>>,
    balls: TrackballQuery,
    mut drags: Query<&mut TrackballDrag>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut focus: ResMut<InputFocus>,
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let entity = press.entity;
    if !drags.contains(entity) {
        return;
    }
    press.propagate(false);
    if disabled.contains(entity) {
        return;
    }
    let position = press.pointer_location.position;
    if aim_at_pointer(entity, position, false, &balls, &ui_scale, &mut commands).is_none() {
        return;
    }
    if let Ok(mut drag) = drags.get_mut(entity) {
        drag.dragging = true;
    }
    focus.set(entity, FocusCause::Pressed);
}

/// A drag re-aims at wherever the pointer now is.
fn on_trackball_drag(
    mut drag: On<Pointer<Drag>>,
    balls: TrackballQuery,
    drags: Query<&TrackballDrag>,
    disabled: Query<(), With<InteractionDisabled>>,
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let entity = drag.entity;
    if !drags.get(entity).is_ok_and(|state| state.dragging) || disabled.contains(entity) {
        return;
    }
    drag.propagate(false);
    let position = drag.pointer_location.position;
    aim_at_pointer(entity, position, false, &balls, &ui_scale, &mut commands);
}

/// The end of a drag: one last aim, marked final.
fn on_trackball_drag_end(
    mut drag_end: On<Pointer<DragEnd>>,
    balls: TrackballQuery,
    mut drags: Query<&mut TrackballDrag>,
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    if drag_end.button != PointerButton::Primary {
        return;
    }
    let entity = drag_end.entity;
    let Ok(mut drag) = drags.get_mut(entity) else {
        return;
    };
    if !drag.dragging {
        return;
    }
    drag_end.propagate(false);
    drag.dragging = false;
    let position = drag_end.pointer_location.position;
    commit(entity, position, &balls, &ui_scale, &mut commands);
}

/// A release that ends a press which never became a drag — a plain click, which
/// still commits the aim it set.
fn on_trackball_release(
    release: On<Pointer<Release>>,
    balls: TrackballQuery,
    mut drags: Query<&mut TrackballDrag>,
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    if release.button != PointerButton::Primary {
        return;
    }
    let entity = release.entity;
    let Ok(mut drag) = drags.get_mut(entity) else {
        return;
    };
    if !drag.dragging {
        return;
    }
    drag.dragging = false;
    commit(
        entity,
        release.pointer_location.position,
        &balls,
        &ui_scale,
        &mut commands,
    );
}

/// A cancelled pointer drops the drag without committing anything.
fn on_trackball_cancel(cancel: On<Pointer<Cancel>>, mut drags: Query<&mut TrackballDrag>) {
    if let Ok(mut drag) = drags.get_mut(cancel.entity) {
        drag.dragging = false;
    }
}

/// Emit the final value for a gesture that has just ended: the aim the pointer
/// is over, or — if it left the disc — the one the control is already holding,
/// so a gesture always ends with a committed value.
fn commit(
    entity: Entity,
    position: Vec2,
    balls: &TrackballQuery,
    ui_scale: &UiScale,
    commands: &mut Commands,
) {
    if aim_at_pointer(entity, position, true, balls, ui_scale, commands).is_some() {
        return;
    }
    if let Ok((aim, _node, _transform, _target)) = balls.get(entity) {
        commands.trigger(ValueChange {
            source: entity,
            value: aim.as_vec2(),
            is_final: true,
        });
    }
}

/// Arrow keys while the control holds focus: left / right step the compass
/// angle, up / down the height, by [`NUDGE_DEGREES`] — and up / down are the
/// only way from this widget to cross the horizon.
fn on_trackball_key(
    event: On<FocusedInput<KeyboardInput>>,
    balls: Query<&TrackballAim>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    let entity = event.focused_entity;
    let Ok(aim) = balls.get(entity) else {
        return;
    };
    if disabled.contains(entity) || !event.input.state.is_pressed() {
        return;
    }
    let (azimuth, elevation) = match event.input.key_code {
        KeyCode::ArrowLeft => (-NUDGE_DEGREES, 0.0),
        KeyCode::ArrowRight => (NUDGE_DEGREES, 0.0),
        KeyCode::ArrowUp => (0.0, NUDGE_DEGREES),
        KeyCode::ArrowDown => (0.0, -NUDGE_DEGREES),
        _other => return,
    };
    let nudged = TrackballAim {
        azimuth: aim.azimuth + azimuth,
        elevation: aim.elevation + elevation,
    }
    .normalized();
    commands.entity(entity).insert(nudged);
    commands.trigger(ValueChange {
        source: entity,
        value: nudged.as_vec2(),
        is_final: true,
    });
}

// ---------------------------------------------------------------------------
// Drawing.
// ---------------------------------------------------------------------------

/// Place every trackball's marker and paint the disc, the marker and the
/// compass letters for the hemisphere it is in and whether it is enabled.
///
/// One query per component type rather than one per role: two
/// `Query<&mut BackgroundColor>` in a single system are a conflicting access
/// and would take the app down on its first frame. Every write is guarded by a
/// compare, so a control nobody is touching re-marks nothing and never
/// re-enters layout.
fn sync_trackballs(
    balls: Query<(
        &TrackballAim,
        &TrackballBody,
        &Children,
        Has<InteractionDisabled>,
    )>,
    parts: Query<(
        Has<TrackballDisc>,
        Has<TrackballMarker>,
        Has<TrackballLabel>,
    )>,
    mut nodes: Query<&mut Node>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    mut visibilities: Query<&mut Visibility>,
) {
    for (aim, body, children, disabled) in &balls {
        let above = aim.above_horizon();
        let at = marker_inset(*aim);
        let marker_color = if disabled {
            DISABLED_MARKER
        } else {
            body.color()
        };
        for child in children.iter() {
            let Ok((is_disc, is_marker, is_label)) = parts.get(child) else {
                continue;
            };
            if is_disc {
                paint(
                    child,
                    if above { DISC_FILL } else { DISC_FILL_BELOW },
                    if disabled {
                        DISABLED_BORDER
                    } else {
                        DISC_BORDER
                    },
                    &mut backgrounds,
                    &mut borders,
                );
            }
            if is_marker {
                place_marker(child, at, &mut nodes);
                // Below the horizon the marker is hollow — the reference's
                // "back" thumb image, and the only thing on the disc that says
                // which side of the horizon it is showing.
                let (fill, outline) = if above {
                    (marker_color, MARKER_OUTLINE)
                } else {
                    (Color::NONE, marker_color)
                };
                paint(child, fill, outline, &mut backgrounds, &mut borders);
            }
            if is_label && let Ok(mut visibility) = visibilities.get_mut(child) {
                // The reference hides the direction labels on a disabled
                // control rather than dimming them.
                let wanted = if disabled {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
                if *visibility != wanted {
                    *visibility = wanted;
                }
            }
        }
    }
}

/// Write a node's fill and outline, each only if it would change.
fn paint(
    entity: Entity,
    fill: Color,
    outline: Color,
    backgrounds: &mut Query<&mut BackgroundColor>,
    borders: &mut Query<&mut BorderColor>,
) {
    if let Ok(mut background) = backgrounds.get_mut(entity)
        && background.0 != fill
    {
        background.0 = fill;
    }
    let wanted = BorderColor::all(outline);
    if let Ok(mut border) = borders.get_mut(entity)
        && *border != wanted
    {
        *border = wanted;
    }
}

/// Move a marker's box, only if it would change.
fn place_marker(entity: Entity, at: Vec2, nodes: &mut Query<&mut Node>) {
    let (left, top) = (Val::Px(at.x), Val::Px(at.y));
    if let Ok(mut node) = nodes.get_mut(entity) {
        if node.left != left {
            node.left = left;
        }
        if node.top != top {
            node.top = top;
        }
    }
}

/// The aim the gallery specimen's **sun** opens on — above the horizon, off
/// both the pole and a cardinal point, so nothing about it is a special case.
///
/// Public because the contract table pins what a gesture does to it, and a
/// second copy of these two numbers is a table that silently stops testing what
/// it says it does.
pub const SPECIMEN_SUN_AIM: TrackballAim = TrackballAim {
    azimuth: 55.0,
    elevation: 35.0,
};

/// The specimen's **moon**, below the horizon — the hollow marker.
pub const SPECIMEN_MOON_AIM: TrackballAim = TrackballAim {
    azimuth: 235.0,
    elevation: -20.0,
};

/// The element prefix the specimen's two controls are named by. Private: the
/// contract table addresses them by their whole node names, which is what a
/// failing sweep prints.
const SPECIMEN_ELEMENT: &str = "gallery";

/// Gallery element: a sun and a moon trackball side by side, one above the
/// horizon and one below it, so both marker states are on the page at once.
///
/// The two [`spawn_trackball`] calls are the ones the environment editors make;
/// nothing here is a gallery-only construction (`crate::ui_element`'s one
/// rule).
pub fn spawn_trackball_pair(
    commands: &mut Commands,
    parent: Entity,
    _cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let row = commands
        .spawn((
            Node {
                column_gap: Val::Px(12.0),
                ..sl_viewer_ui_core::ui::row(Val::Px(12.0))
            },
            Name::new("trackball-pair"),
            ChildOf(parent),
        ))
        .id();
    spawn_trackball(
        commands,
        row,
        SPECIMEN_ELEMENT,
        TrackballBody::Sun,
        1,
        SPECIMEN_SUN_AIM,
    );
    spawn_trackball(
        commands,
        row,
        SPECIMEN_ELEMENT,
        TrackballBody::Moon,
        2,
        SPECIMEN_MOON_AIM,
    );
    row
}

/// The plugin a host adds to draw trackballs. The pointer and keyboard halves
/// are observers attached by [`spawn_trackball`], so a control still aims
/// without it — but its marker never moves.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrackballPlugin;

impl Plugin for TrackballPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_trackballs);
    }
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::InputFocus;
    use bevy::prelude::*;
    use bevy::ui_widgets::ValueChange;
    use pretty_assertions::assert_eq;

    use super::{
        MARKER_SIZE, RADIUS, TRACKBALL_SIZE, TrackballAim, TrackballBody, aim_to_disc, disc_to_aim,
        marker_inset, spawn_trackball,
    };

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// How close two angles have to be to count as the same one, in degrees.
    /// The round trip runs through two transcendental functions in `f32`.
    const ANGLE_EPSILON: f32 = 0.01;

    /// Assert two angles agree to [`ANGLE_EPSILON`].
    #[track_caller]
    fn assert_angle(got: f32, wanted: f32, what: &str) {
        assert!(
            (got - wanted).abs() < ANGLE_EPSILON,
            "{what}: {got} is not {wanted}"
        );
    }

    /// **The projection and its inverse agree**, over both hemispheres, the
    /// poles and the horizon.
    ///
    /// The elevation is held to a looser tolerance than [`ANGLE_EPSILON`]
    /// because the projection genuinely loses it near the horizon: the distance
    /// from the centre is `cos(elevation)`, whose derivative vanishes there, so
    /// the `acos` that reads it back amplifies the last bit of an `f32` into
    /// roughly `sqrt(2 * f32::EPSILON)` radians — about a fiftieth of a degree,
    /// at the *worst* point on the disc. That is a property of drawing a
    /// hemisphere flat, not of this code, it is two orders of magnitude below a
    /// pixel of marker travel, and the widget never round-trips through the disc
    /// anyway — the aim it holds is the value, and the disc is only how it is
    /// drawn and pointed at.
    #[test]
    fn an_aim_survives_the_disc_and_back() -> Result<(), TestError> {
        /// The worst the `acos` at the horizon can cost, in degrees.
        const HORIZON_EPSILON: f32 = 0.05;

        for azimuth in [0.0_f32, 1.0, 89.9, 90.0, 179.0, 180.0, 271.0, 359.5] {
            for elevation in [-90.0_f32, -45.0, -0.5, 0.0, 0.5, 30.0, 89.0, 90.0] {
                let aim = TrackballAim { azimuth, elevation };
                let back = disc_to_aim(aim_to_disc(aim), aim)
                    .ok_or_else(|| format!("{aim:?} projected off the disc"))?;
                assert!(
                    (back.elevation - elevation).abs() < HORIZON_EPSILON,
                    "elevation: {} is not {elevation}",
                    back.elevation
                );
                // At a pole every azimuth is the same direction, and the
                // inverse says so by keeping the one it was given.
                if elevation.abs() < 90.0 {
                    assert_angle(back.azimuth, azimuth, "azimuth");
                }
            }
        }
        Ok(())
    }

    /// **The compass is the one on the disc**: due east is to the right, north
    /// is up, and the zenith is dead centre.
    #[test]
    fn the_disc_is_a_map_with_north_up() {
        let at = |azimuth: f32, elevation: f32| aim_to_disc(TrackballAim { azimuth, elevation });
        assert!(at(0.0, 0.0).abs_diff_eq(Vec2::new(1.0, 0.0), 1e-5), "east");
        assert!(
            at(90.0, 0.0).abs_diff_eq(Vec2::new(0.0, 1.0), 1e-5),
            "north"
        );
        assert!(
            at(180.0, 0.0).abs_diff_eq(Vec2::new(-1.0, 0.0), 1e-5),
            "west"
        );
        assert!(
            at(270.0, 0.0).abs_diff_eq(Vec2::new(0.0, -1.0), 1e-5),
            "south"
        );
        assert!(
            at(123.0, 90.0).abs_diff_eq(Vec2::ZERO, 1e-5),
            "the zenith is the centre whatever the azimuth"
        );
    }

    /// **A sun below the horizon draws where its mirror above it would.** The
    /// projection cannot separate them — which is why the marker's fill carries
    /// the answer.
    #[test]
    fn the_two_hemispheres_project_onto_the_same_disc() {
        let above = aim_to_disc(TrackballAim {
            azimuth: 40.0,
            elevation: 25.0,
        });
        let below = aim_to_disc(TrackballAim {
            azimuth: 40.0,
            elevation: -25.0,
        });
        assert!(above.abs_diff_eq(below, 1e-6), "{above} vs {below}");
    }

    /// **A drag keeps the hemisphere it started in** — the reference's rule, and
    /// the reason a night sky does not jump to noon when the sun is nudged.
    #[test]
    fn aiming_keeps_the_hemisphere_it_started_in() -> Result<(), TestError> {
        let point = Vec2::new(0.5, 0.0);
        let from_above = disc_to_aim(
            point,
            TrackballAim {
                azimuth: 0.0,
                elevation: 10.0,
            },
        )
        .ok_or("the point is on the disc")?;
        let from_below = disc_to_aim(
            point,
            TrackballAim {
                azimuth: 0.0,
                elevation: -10.0,
            },
        )
        .ok_or("the point is on the disc")?;
        assert!(from_above.elevation > 0.0, "{from_above:?}");
        assert!(from_below.elevation < 0.0, "{from_below:?}");
        assert_angle(
            from_above.elevation,
            -from_below.elevation,
            "the same height, opposite sides",
        );
        // Exactly on the horizon counts as above, as the reference's `>= 0`
        // does, so a rim drag from there stays up.
        let from_horizon = disc_to_aim(point, TrackballAim::ZENITH.normalized())
            .ok_or("the point is on the disc")?;
        assert!(from_horizon.elevation > 0.0, "{from_horizon:?}");
        Ok(())
    }

    /// **Outside the rim is not an aim.** The control holds what it had rather
    /// than pinning the body to the horizon.
    #[test]
    fn a_point_past_the_rim_is_refused() {
        assert_eq!(
            disc_to_aim(Vec2::new(1.001, 0.0), TrackballAim::ZENITH),
            None
        );
        assert!(
            disc_to_aim(Vec2::new(1.0, 0.0), TrackballAim::ZENITH).is_some(),
            "the rim itself is on the disc"
        );
    }

    /// **The centre keeps the azimuth it was handed.** Every azimuth means the
    /// same direction there, so inventing one would spin the compass under a
    /// hand that passed over the zenith.
    #[test]
    fn the_centre_keeps_the_azimuth() -> Result<(), TestError> {
        let aimed = disc_to_aim(
            Vec2::ZERO,
            TrackballAim {
                azimuth: 137.0,
                elevation: 20.0,
            },
        )
        .ok_or("the centre is on the disc")?;
        assert_angle(aimed.azimuth, 137.0, "azimuth");
        assert_angle(aimed.elevation, 90.0, "the centre is the zenith");
        Ok(())
    }

    /// **A marker never leaves the control's box.** The whole reason the aiming
    /// circle is a marker's width short of the control: a box that escaped its
    /// parent is drawn over whatever is beside it, and the layout sweep's
    /// containment check says so.
    #[test]
    fn a_marker_at_the_horizon_stays_inside_the_control() {
        for azimuth in [0.0_f32, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
            let at = marker_inset(TrackballAim {
                azimuth,
                elevation: 0.0,
            });
            assert!(
                at.x >= -0.001 && at.y >= -0.001,
                "{azimuth}° puts the marker at {at}, off the leading edge"
            );
            let far = TRACKBALL_SIZE - MARKER_SIZE + 0.001;
            assert!(
                at.x <= far && at.y <= far,
                "{azimuth}° puts the marker at {at}, past the trailing edge"
            );
        }
    }

    /// **The zenith is the middle of the control.**
    #[test]
    fn the_zenith_centres_the_marker() {
        let at = marker_inset(TrackballAim::ZENITH);
        let centre = (TRACKBALL_SIZE - MARKER_SIZE) / 2.0;
        assert!(
            (at.x - centre).abs() < 1e-4 && (at.y - centre).abs() < 1e-4,
            "{at} is not the centre {centre}"
        );
    }

    /// **North is up on the screen**: the marker for due north sits above the
    /// centre, not below it.
    #[test]
    fn north_puts_the_marker_above_the_centre() {
        let north = marker_inset(TrackballAim {
            azimuth: 90.0,
            elevation: 0.0,
        });
        let centre = marker_inset(TrackballAim::ZENITH);
        assert!(
            north.y < centre.y,
            "north at {north} is not above the centre {centre}"
        );
        assert!(
            (north.y - (centre.y - RADIUS)).abs() < 1e-4,
            "north is a full radius above the centre"
        );
    }

    /// **An azimuth wraps and an elevation clamps.**
    #[test]
    fn a_normalized_aim_is_on_the_sphere() {
        let wrapped = TrackballAim {
            azimuth: -30.0,
            elevation: 140.0,
        }
        .normalized();
        assert_angle(wrapped.azimuth, 330.0, "a negative azimuth wraps");
        assert_angle(wrapped.elevation, 90.0, "an elevation past the pole clamps");
        let far = TrackballAim {
            azimuth: 725.0,
            elevation: -300.0,
        }
        .normalized();
        assert_angle(far.azimuth, 5.0, "two turns and five degrees");
        assert_angle(far.elevation, -90.0, "clamped at the nadir");
    }

    /// What a trackball has emitted, collected by a global observer — a
    /// `ValueChange` is an entity event rather than a message, so the testkit's
    /// message recorder cannot see it.
    #[derive(Resource, Debug, Default)]
    struct Emitted(Vec<ValueChange<Vec2>>);

    /// A headless app with one trackball in it, laid out.
    fn trackball_app(body: TrackballBody, initial: TrackballAim) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(super::TrackballPlugin)
            .init_resource::<UiScale>()
            .init_resource::<InputFocus>()
            .init_resource::<Emitted>();
        app.add_observer(
            |change: On<ValueChange<Vec2>>, mut emitted: ResMut<Emitted>| {
                emitted.0.push(ValueChange {
                    source: change.source,
                    value: change.value,
                    is_final: change.is_final,
                });
            },
        );
        let root = app.world_mut().spawn(Node::default()).id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let trackball = {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_trackball(&mut commands, root, "test", body, 0, initial)
        };
        queue.apply(app.world_mut());
        app.update();
        (app, trackball)
    }

    /// Where the marker's box currently is, in the control's logical pixels.
    fn marker_at(app: &App, trackball: Entity) -> Option<Vec2> {
        let children: Vec<Entity> = app.world().get::<Children>(trackball)?.iter().collect();
        for child in children {
            if app.world().get::<super::TrackballMarker>(child).is_none() {
                continue;
            }
            let node = app.world().get::<Node>(child)?;
            if let (Val::Px(left), Val::Px(top)) = (node.left, node.top) {
                return Some(Vec2::new(left, top));
            }
        }
        None
    }

    /// **A consumer writing the aim moves the marker.** The component is the
    /// value: the widget draws whatever is in it, whoever wrote it.
    #[test]
    fn writing_the_aim_moves_the_marker() -> Result<(), TestError> {
        let (mut app, trackball) = trackball_app(TrackballBody::Sun, TrackballAim::ZENITH);
        let centre = marker_at(&app, trackball).ok_or("no marker")?;
        app.world_mut().entity_mut(trackball).insert(TrackballAim {
            azimuth: 90.0,
            elevation: 0.0,
        });
        app.update();
        let north = marker_at(&app, trackball).ok_or("no marker")?;
        assert!(north.y < centre.y, "{north} is not north of {centre}");
        Ok(())
    }

    /// **The trackball, driven** (`viewer-ui-widget-interaction-suite`): a press
    /// and a drag on the real geometry, through the real pointer.
    ///
    /// Every test above hands the mapping a point it has already worked out,
    /// which makes them tests of the projection and not of where a point comes
    /// from. The pointer path is the half that depends on the control's *layout*
    /// — a node that laid out at the wrong size, or a transform inverted the
    /// wrong way, aims somewhere else for a gesture that still looks right — and
    /// only a real drag on a laid-out control can see that.
    mod scenarios {
        use bevy::input::keyboard::Key;
        use bevy::prelude::*;
        use bevy::ui_widgets::ValueChange;
        use pretty_assertions::assert_eq;

        use super::{TestError, assert_angle};
        use crate::ui_test::interact::{self, InteractionTest, centre_of};
        use crate::ui_test::settle;
        use crate::ui_trackball::{
            NUDGE_DEGREES as NUDGE, RADIUS, TrackballAim, TrackballBody, TrackballPlugin,
            spawn_trackball,
        };
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};

        /// The control's node name.
        const TRACKBALL: &str = "test-sun:trackball";

        /// What the control has emitted.
        #[derive(Resource, Debug, Default)]
        struct Emitted(Vec<ValueChange<Vec2>>);

        /// One trackball under the real pointer stack, aimed at the zenith.
        fn trackball_app() -> App {
            let mut app = InteractionTest::new().build();
            app.add_plugins(TrackballPlugin);
            app.init_resource::<Emitted>();
            app.add_observer(
                |change: On<ValueChange<Vec2>>, mut emitted: ResMut<Emitted>| {
                    emitted.0.push(ValueChange {
                        source: change.source,
                        value: change.value,
                        is_final: change.is_final,
                    });
                },
            );
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_trackball(
                        &mut commands,
                        root.0,
                        "test",
                        TrackballBody::Sun,
                        1,
                        TrackballAim::ZENITH,
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            settle(&mut app);
            app
        }

        /// The aim the control is holding.
        fn aim(app: &mut App) -> Option<TrackballAim> {
            let entity = crate::ui_test::find_by_name(app, TRACKBALL)?;
            app.world().get::<TrackballAim>(entity).copied()
        }

        /// **A drag aims where the hand is.** Half a radius due east of the
        /// centre is azimuth 0° at 60° up — `acos(0.5)` — and the gesture ends
        /// committed.
        #[test]
        fn a_drag_aims_at_the_pointer() -> Result<(), TestError> {
            let mut app = trackball_app();
            let centre = centre_of(&mut app, TRACKBALL).ok_or("the control never laid out")?;
            assert_angle(
                aim(&mut app).ok_or("no aim")?.elevation,
                90.0,
                "it starts at the zenith",
            );

            interact::drag(
                &mut app,
                centre,
                Vec2::new(centre.x + RADIUS / 2.0, centre.y),
                4,
                MouseButton::Left,
            );
            settle(&mut app);

            let aimed = aim(&mut app).ok_or("no aim")?;
            assert_angle(aimed.azimuth, 0.0, "half a radius east is due east");
            assert_angle(aimed.elevation, 60.0, "and half way out is 60 degrees up");

            let emitted = &app.world().resource::<Emitted>().0;
            let last = emitted.last().ok_or("the drag emitted nothing")?;
            assert!(last.is_final, "the gesture ended committed: {last:?}");
            assert!(
                emitted.iter().any(|change| !change.is_final),
                "and it previewed on the way: {emitted:?}"
            );
            Ok(())
        }

        /// **North is up under the pointer too.** Dragging *up* the screen from
        /// the centre aims north, which is the claim the whole compass rests on
        /// and the one an inverted transform would break.
        #[test]
        fn dragging_up_the_screen_aims_north() -> Result<(), TestError> {
            let mut app = trackball_app();
            let centre = centre_of(&mut app, TRACKBALL).ok_or("the control never laid out")?;
            interact::drag(
                &mut app,
                centre,
                Vec2::new(centre.x, centre.y - RADIUS / 2.0),
                4,
                MouseButton::Left,
            );
            settle(&mut app);
            assert_angle(
                aim(&mut app).ok_or("no aim")?.azimuth,
                90.0,
                "up the screen is north",
            );
            Ok(())
        }

        /// **A drag that leaves the disc holds the last aim** rather than
        /// pinning the body to the horizon — the reference's
        /// `pointInTouchCircle` guard — and still ends with a committed value.
        #[test]
        fn a_drag_off_the_disc_holds_its_last_aim() -> Result<(), TestError> {
            let mut app = trackball_app();
            let centre = centre_of(&mut app, TRACKBALL).ok_or("the control never laid out")?;
            interact::drag(
                &mut app,
                centre,
                Vec2::new(centre.x + RADIUS / 2.0, centre.y),
                4,
                MouseButton::Left,
            );
            settle(&mut app);
            let held = aim(&mut app).ok_or("no aim")?;

            // Well past the rim, in one hop, so nothing in between lands on the
            // disc and re-aims it.
            interact::drag(
                &mut app,
                Vec2::new(centre.x + RADIUS / 2.0, centre.y),
                Vec2::new(centre.x + RADIUS * 4.0, centre.y),
                1,
                MouseButton::Left,
            );
            settle(&mut app);

            let after = aim(&mut app).ok_or("no aim")?;
            assert_angle(after.azimuth, held.azimuth, "the azimuth is where it was");
            assert_angle(after.elevation, held.elevation, "and so is the elevation");
            let last = app
                .world()
                .resource::<Emitted>()
                .0
                .last()
                .copied()
                .ok_or("nothing was emitted")?;
            assert!(last.is_final, "the gesture still ended committed");
            Ok(())
        }

        /// **A press in the control's corner does nothing.** The square is the
        /// widget's box; the circle is what it aims by, and the reference does
        /// not capture outside it.
        #[test]
        fn a_press_outside_the_circle_is_inert() -> Result<(), TestError> {
            let mut app = trackball_app();
            let centre = centre_of(&mut app, TRACKBALL).ok_or("the control never laid out")?;
            let before = aim(&mut app).ok_or("no aim")?;
            // The corner of the square, which is a radius times root two out.
            let corner = Vec2::new(centre.x + RADIUS * 0.95, centre.y + RADIUS * 0.95);
            interact::click(&mut app, corner, MouseButton::Left);
            settle(&mut app);
            assert_eq!(aim(&mut app), Some(before), "the corner aimed the control");
            assert!(
                app.world().resource::<Emitted>().0.is_empty(),
                "and it emitted nothing"
            );
            Ok(())
        }

        /// **An arrow key steps the angle it names and commits.** Left and
        /// right walk the compass, up and down the height — and down is how a
        /// body goes under the horizon from this widget, which the disc alone
        /// cannot do.
        #[test]
        fn the_arrow_keys_step_the_two_angles() -> Result<(), TestError> {
            let mut app = trackball_app();
            let trackball =
                crate::ui_test::find_by_name(&mut app, TRACKBALL).ok_or("no control")?;
            // Off the pole, where an azimuth means something, and just above the
            // horizon, so three steps down cross it.
            app.world_mut().entity_mut(trackball).insert(TrackballAim {
                azimuth: 10.0,
                elevation: 1.0,
            });
            interact::focus(&mut app, trackball);

            for (key_code, logical, azimuth, elevation) in [
                (KeyCode::ArrowRight, Key::ArrowRight, 10.0 + NUDGE, 1.0),
                (KeyCode::ArrowLeft, Key::ArrowLeft, 10.0, 1.0),
                (KeyCode::ArrowDown, Key::ArrowDown, 10.0, 1.0 - NUDGE),
                (KeyCode::ArrowUp, Key::ArrowUp, 10.0, 1.0),
            ] {
                interact::tap(&mut app, key_code, logical);
                settle(&mut app);
                let aimed = aim(&mut app).ok_or("the control lost its aim")?;
                assert_angle(aimed.azimuth, azimuth, "azimuth");
                assert_angle(aimed.elevation, elevation, "elevation");
            }
            let emitted = &app.world().resource::<Emitted>().0;
            assert_eq!(emitted.len(), 4, "one commit per key, and only on the down");
            assert!(
                emitted.iter().all(|change| change.is_final),
                "a key press is a finished gesture: {emitted:?}"
            );

            interact::tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
            settle(&mut app);
            let aimed = aim(&mut app).ok_or("the control lost its aim")?;
            assert!(aimed.elevation < 0.0, "{aimed:?} did not cross the horizon");
            Ok(())
        }

        /// **A disabled control ignores the pointer and the keyboard**, and says
        /// so: its compass letters go away, as the reference's do.
        #[test]
        fn a_disabled_trackball_is_inert() -> Result<(), TestError> {
            let mut app = trackball_app();
            let trackball =
                crate::ui_test::find_by_name(&mut app, TRACKBALL).ok_or("no control")?;
            app.world_mut()
                .entity_mut(trackball)
                .insert(bevy::ui::InteractionDisabled);
            settle(&mut app);
            let before = aim(&mut app).ok_or("no aim")?;

            let centre = centre_of(&mut app, TRACKBALL).ok_or("the control never laid out")?;
            interact::drag(
                &mut app,
                centre,
                Vec2::new(centre.x + RADIUS / 2.0, centre.y),
                4,
                MouseButton::Left,
            );
            settle(&mut app);
            interact::focus(&mut app, trackball);
            interact::tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
            settle(&mut app);

            assert_eq!(aim(&mut app), Some(before), "a disabled control was aimed");
            assert!(
                app.world().resource::<Emitted>().0.is_empty(),
                "and it emitted nothing"
            );
            let hidden = app
                .world_mut()
                .query::<(&super::super::TrackballLabel, &Visibility)>()
                .iter(app.world())
                .all(|(_label, visibility)| *visibility == Visibility::Hidden);
            assert!(hidden, "a disabled control still shows its compass letters");
            Ok(())
        }
    }
}
