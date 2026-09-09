//! The **Personal Lighting** floater (`viewer-environment-personal-lighting`):
//! change the sky and the water you are standing under, locally.
//!
//! Every control here writes the viewer's **local** environment layer. The
//! region's own settings are not touched, nothing is published to the grid, and
//! nobody else sees the change — it is the reference viewer's `ENV_LOCAL`, the
//! layer a photographer, a machinimist, or anyone who simply wants the sun
//! somewhere else works in.
//!
//! # What opening it does
//!
//! Opening the floater **captures** the environment currently on screen into the
//! local layer and starts editing that copy — the reference's
//! `captureCurrentEnvironment`. Until a control is touched nothing looks
//! different, but the sky in force is now the user's rather than the region's,
//! which is what makes the first slider drag a local edit instead of a request
//! to change the region.
//!
//! **Reset** (with the reference's confirmation) drops the whole local layer,
//! which puts the shared (region) environment back.
//!
//! # Why edits are instant and the menu's are not
//!
//! Picking a preset off the World ▸ Environment menu is an *arrival* at an
//! environment, and may cross-fade
//! (`EnvironmentManualTransitionTime`). Dragging a slider is not an arrival: a
//! fade restarted on every pixel of a drag would run the preview a beat behind
//! the hand moving it. So the editor writes through
//! [`EnvironmentState::set_local_instant`], which is the reference's
//! `TRANSITION_INSTANT` for exactly the same reason.
//!
//! # Deliberate divergences from the reference
//!
//! - **There is a water section.** The reference's Personal Lighting floater
//!   offers exactly one water control — the normal map — and sends anyone who
//!   wants the fog colour or the wave directions to the full settings editor.
//!   The knobs are the same ones, and they are as local and as reversible as the
//!   sky's, so they are here.
//! - **The sun and moon get azimuth / elevation sliders and no trackball.** The
//!   reference offers both, reading one out of the other. The two sliders are
//!   the whole of the state; a trackball is a second way to drive them and can
//!   be added over the same fields whenever there is a trackball widget.
//! - **There are no sun / moon beacon checkboxes.** They belong to the beacons
//!   feature ([[viewer-beacons-control]]), not to this window's state, and the
//!   reference only puts them here for convenience.
//!
//! Reference (Firestorm, read-only): `llfloaterenvironmentadjust.cpp`,
//! `floater_adjust_environment.xml`, `panel_settings_water.xml`.

use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderValue, ValueChange};
use sl_client_bevy::{EnvironmentAsset, SkySettings, WaterSettings};
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_pickers::ui_texture_picker::TextureSwatchValue;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater,
};
use sl_viewer_ui_widgets::ui_color_picker::{ColorPicked, ColorSwatchValue};
use sl_viewer_world_api::TexturePicked;
use sl_viewer_world_scene::environment::EnvironmentState;
use sl_viewer_world_scene::sky::day_position;

use crate::knobs::{ColorKnob, SkyKnob, TextureKnob, WaterKnob};
use crate::rows::{spawn_action_button, spawn_color_row, spawn_slider, spawn_texture_row};
use crate::style::{DIM_LABEL_COLOR, HEADING_SIZE};

/// The element-id prefix every control in this window is named by — the window
/// draws knobs it shares with the settings editors, so the *window* supplies
/// the part of the name that says which one a control belongs to.
const ELEMENT: &str = "personal-lighting";

/// The floater's registry id — what the World menu toggles.
pub const PERSONAL_LIGHTING_FLOATER_ID: &str = "personal-lighting";

/// The notification the Reset button raises before it drops the layer.
const RESET_CONFIRM: &str = "PersonalSettingsConfirmReset";

// ---------------------------------------------------------------------------
// Components and state.
// ---------------------------------------------------------------------------

/// A sky slider of *this* window: the knob it drives. The readout and the
/// thumb are [`SliderRow`](crate::rows::SliderRow)'s business, shared with
/// every other environment window.
#[derive(Component, Debug, Clone, Copy)]
struct SkySliderRow(SkyKnob);

/// A water slider of this window.
#[derive(Component, Debug, Clone, Copy)]
struct WaterSliderRow(WaterKnob);

/// A colour swatch's knob.
#[derive(Component, Debug, Clone, Copy)]
struct ColorSwatchKnob(ColorKnob);

/// A texture swatch's knob.
#[derive(Component, Debug, Clone, Copy)]
struct TextureSwatchKnob(TextureKnob);

/// The Reset button.
#[derive(Component, Debug, Clone, Copy)]
struct ResetButton;

/// The floater's chrome handles.
#[derive(Resource, Debug)]
struct PersonalLightingUi {
    /// The floater root, which carries the `UiPanelShown` the window is closed
    /// by and re-opened from.
    panel: Entity,
}

/// The environment the floater is editing.
///
/// Two buffers rather than reading the scene back each frame: the sky in force
/// is the *pinned* copy of what these hold, so re-reading it would round-trip
/// every edit through the day-cycle machinery and hand back a frame that has
/// already been blended and named. Editing the buffers and pinning the result is
/// the reference's `mLiveSky` / `mLiveWater`, for the same reason.
#[derive(Resource, Debug, Default)]
struct PersonalLightingEdit {
    /// The sky being edited, or `None` while the floater has never been opened.
    sky: Option<Box<SkySettings>>,
    /// The water being edited.
    water: Option<WaterSettings>,
    /// A control changed a buffer: push both to the environment.
    dirty: bool,
    /// The buffers were replaced (the floater opened): re-seed every widget from
    /// them.
    reseed: bool,
}

impl PersonalLightingEdit {
    /// The two buffers, mutably, or `None` before the first capture.
    fn pair(&mut self) -> Option<(&mut SkySettings, &mut WaterSettings)> {
        match (self.sky.as_deref_mut(), self.water.as_mut()) {
            (Some(sky), Some(water)) => Some((sky, water)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the Personal Lighting floater into a host.
#[derive(Debug, Clone, Copy, Default)]
pub struct PersonalLightingPlugin;

impl Plugin for PersonalLightingPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::rows::RowsPlugin>() {
            app.add_plugins(crate::rows::RowsPlugin);
        }
        app.init_resource::<PersonalLightingEdit>()
            .add_systems(
                Startup,
                spawn_personal_lighting.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                // Ordered: a capture on open must reach the widgets (reseed)
                // and the environment (push) in the same frame it happened, or
                // the window's first frame shows the previous sky's values.
                (
                    capture_on_open,
                    apply_color_picks,
                    apply_texture_picks,
                    reseed_personal_widgets,
                    push_personal_edits,
                    handle_reset_confirmation,
                )
                    .chain(),
            );
    }
}

/// The floater's [`FloaterSpec`] — shared with the `FLOATERS` registry, so the
/// swept window is the one the viewer spawns.
#[must_use]
pub fn personal_lighting_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PERSONAL_LIGHTING_FLOATER_ID,
        title: "Personal Lighting".to_owned(),
        position: Vec2::new(120.0, 120.0),
        // Four columns side by side, as the reference lays it out — a tall
        // single column would need a scroll view for a window whose whole point
        // is that every knob is under the hand at once. The water column is the
        // long one at fourteen knobs, and sets the height.
        default_size: Some(Vec2::new(660.0, 540.0)),
        min_size: Some(Vec2::new(320.0, 240.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the (hidden) floater chrome, its content deferred to first
/// open.
fn spawn_personal_lighting(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, personal_lighting_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("personal-lighting-title"));
    let builder = commands.register_system(build_personal_lighting_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
    commands.insert_resource(PersonalLightingUi { panel: handle.root });
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// The sky sliders of the second column — the atmosphere and cloud knobs.
const ATMOSPHERE_KNOBS: &[SkyKnob] = &[
    SkyKnob::HazeHorizon,
    SkyKnob::HazeDensity,
    SkyKnob::CloudCoverage,
    SkyKnob::CloudScale,
    SkyKnob::ProbeAmbiance,
    SkyKnob::Gamma,
];

/// The third column: where the sun and the moon are, and how they glow.
const SUN_MOON_KNOBS: &[SkyKnob] = &[
    SkyKnob::SunAzimuth,
    SkyKnob::SunElevation,
    SkyKnob::SunScale,
    SkyKnob::GlowFocus,
    SkyKnob::GlowSize,
    SkyKnob::StarBrightness,
    SkyKnob::MoonAzimuth,
    SkyKnob::MoonElevation,
];

/// The fourth column: the water.
const WATER_KNOBS: &[WaterKnob] = &[
    WaterKnob::FogDensity,
    WaterKnob::UnderwaterModifier,
    WaterKnob::FresnelScale,
    WaterKnob::FresnelOffset,
    WaterKnob::NormalScaleX,
    WaterKnob::NormalScaleY,
    WaterKnob::NormalScaleZ,
    WaterKnob::ScaleAbove,
    WaterKnob::ScaleBelow,
    WaterKnob::BlurMultiplier,
    WaterKnob::LargeWaveX,
    WaterKnob::LargeWaveY,
    WaterKnob::SmallWaveX,
    WaterKnob::SmallWaveY,
];

/// The colour swatches of the first column.
const COLOR_KNOBS: &[ColorKnob] = &[
    ColorKnob::Ambient,
    ColorKnob::BlueHorizon,
    ColorKnob::BlueDensity,
    ColorKnob::SunColor,
    ColorKnob::CloudColor,
    ColorKnob::WaterFogColor,
];

/// The texture swatches of the first column.
const TEXTURE_KNOBS: &[TextureKnob] = &[TextureKnob::CloudImage, TextureKnob::WaterNormalMap];

/// First-open content build: four columns — colours and images, the atmosphere,
/// the sun and moon, and the water.
fn build_personal_lighting_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(6.0)),
                ..row(Val::Px(10.0))
            },
            Name::new("personal-lighting:content"),
            ChildOf(handle.content),
        ))
        .id();
    let mut tab = 0_i32;

    let swatches = spawn_column(&mut commands, content, "personal-lighting-colours");
    for knob in COLOR_KNOBS {
        spawn_color_swatch_row(&mut commands, swatches, *knob, &mut tab);
    }
    for knob in TEXTURE_KNOBS {
        spawn_texture_swatch_row(&mut commands, swatches, *knob, &mut tab);
    }
    spawn_reset_button(&mut commands, swatches, &mut tab);

    let atmosphere = spawn_column(&mut commands, content, "personal-lighting-atmosphere");
    for knob in ATMOSPHERE_KNOBS {
        spawn_sky_slider(&mut commands, atmosphere, *knob, &mut tab);
    }

    let bodies = spawn_column(&mut commands, content, "personal-lighting-sun-moon");
    for knob in SUN_MOON_KNOBS {
        spawn_sky_slider(&mut commands, bodies, *knob, &mut tab);
    }

    let water = spawn_column(&mut commands, content, "personal-lighting-water");
    for knob in WATER_KNOBS {
        spawn_water_slider(&mut commands, water, *knob, &mut tab);
    }
}

/// One titled column of the floater; returns the node rows are parented into.
fn spawn_column(commands: &mut Commands, parent: Entity, heading_key: &'static str) -> Entity {
    let column_entity = commands
        .spawn((
            Node {
                min_width: Val::Px(0.0),
                ..column(Val::Px(3.0))
            },
            Name::new(format!("{heading_key}:column")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(HEADING_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Translated::new(heading_key),
        ChildOf(column_entity),
    ));
    column_entity
}

/// One sky-knob slider row.
fn spawn_sky_slider(commands: &mut Commands, parent: Entity, knob: SkyKnob, tab: &mut i32) {
    let track = spawn_slider(
        commands,
        parent,
        ELEMENT,
        knob.slug(),
        knob.range(),
        knob.decimals(),
        tab,
    );
    commands
        .entity(track)
        .insert(SkySliderRow(knob))
        .observe(on_sky_slider_change);
}

/// One water-knob slider row.
fn spawn_water_slider(commands: &mut Commands, parent: Entity, knob: WaterKnob, tab: &mut i32) {
    let track = spawn_slider(
        commands,
        parent,
        ELEMENT,
        knob.slug(),
        knob.range(),
        knob.decimals(),
        tab,
    );
    commands
        .entity(track)
        .insert(WaterSliderRow(knob))
        .observe(on_water_slider_change);
}

/// One colour-swatch row.
fn spawn_color_swatch_row(commands: &mut Commands, parent: Entity, knob: ColorKnob, tab: &mut i32) {
    let swatch = spawn_color_row(commands, parent, ELEMENT, knob, tab);
    commands.entity(swatch).insert(ColorSwatchKnob(knob));
}

/// One texture-swatch row.
fn spawn_texture_swatch_row(
    commands: &mut Commands,
    parent: Entity,
    knob: TextureKnob,
    tab: &mut i32,
) {
    let swatch = spawn_texture_row(commands, parent, ELEMENT, knob, tab);
    commands.entity(swatch).insert(TextureSwatchKnob(knob));
}

/// The Reset button, which drops the whole local layer after a confirmation.
fn spawn_reset_button(commands: &mut Commands, parent: Entity, tab: &mut i32) {
    let button = spawn_action_button(
        commands,
        parent,
        ELEMENT,
        "reset",
        "personal-lighting-reset".to_owned(),
        tab,
    );
    commands
        .entity(button)
        .insert((
            ResetButton,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                margin: UiRect::top(Val::Px(6.0)),
                align_self: AlignSelf::FlexStart,
                ..Default::default()
            },
        ))
        .observe(on_reset_pressed);
}

// ---------------------------------------------------------------------------
// Capture, edit, apply.
// ---------------------------------------------------------------------------

/// Capture the environment on screen into the edit buffers when the floater
/// opens — the reference's `captureCurrentEnvironment`, which is what makes the
/// first slider drag a *local* edit rather than a change to the region.
fn capture_on_open(
    panels: Query<(Entity, &UiPanelShown), Changed<UiPanelShown>>,
    ui: Option<Res<PersonalLightingUi>>,
    environment: Option<Res<EnvironmentState>>,
    mut edit: ResMut<PersonalLightingEdit>,
) {
    let (Some(ui), Some(environment)) = (ui, environment) else {
        return;
    };
    let opened = panels
        .iter()
        .any(|(entity, shown)| entity == ui.panel && shown.0);
    if !opened {
        return;
    }
    let position = day_position(&environment);
    let (Some(sky), Some(water)) = (
        environment.sky_at(0.0, position),
        environment.water_at(position),
    ) else {
        // An environment with no sky or no water frame at all: there is nothing
        // to clone, and inventing one would be a sky nobody asked for.
        warn!("personal lighting: no environment to capture; the window opens empty");
        return;
    };
    edit.sky = Some(Box::new(sky));
    edit.water = Some(water);
    edit.reseed = true;
    // Install the capture at once, so the layer being edited is the local one
    // from the moment the window is up.
    edit.dirty = true;
}

/// A sky slider moved: clamp it, write the knob, and mark the buffers dirty.
fn on_sky_slider_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(&SkySliderRow, &SliderRange)>,
    mut edit: ResMut<PersonalLightingEdit>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(sky) = edit.sky.as_deref_mut() {
        row_info.0.write(sky, clamped);
        edit.dirty = true;
    }
}

/// A water slider moved.
fn on_water_slider_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(&WaterSliderRow, &SliderRange)>,
    mut edit: ResMut<PersonalLightingEdit>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(water) = edit.water.as_mut() {
        row_info.0.write(water, clamped);
        edit.dirty = true;
    }
}

/// A colour came back from the picker: write it into the buffers.
///
/// Both the live-preview replies and the committed one are applied, so the
/// preview in the picker and the sky behind it move together — and Cancel's
/// revert reply puts the original back for free.
fn apply_color_picks(
    mut picks: MessageReader<ColorPicked>,
    mut swatches: Query<(&ColorSwatchKnob, &mut ColorSwatchValue)>,
    mut edit: ResMut<PersonalLightingEdit>,
) {
    for pick in picks.read() {
        let Ok((knob, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.color;
        let knob = knob.0;
        let Some((sky, water)) = edit.pair() else {
            continue;
        };
        knob.write(sky, water, pick.color);
        edit.dirty = true;
    }
}

/// A texture came back from the picker.
fn apply_texture_picks(
    mut picks: MessageReader<TexturePicked>,
    mut swatches: Query<(&TextureSwatchKnob, &mut TextureSwatchValue)>,
    mut edit: ResMut<PersonalLightingEdit>,
) {
    for pick in picks.read() {
        let Ok((knob, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.texture;
        let knob = knob.0;
        let Some((sky, water)) = edit.pair() else {
            continue;
        };
        knob.write(sky, water, pick.texture);
        edit.dirty = true;
    }
}

/// Seed every widget from the buffers after a capture, so the window opens
/// showing the sky it is about to edit rather than the last one it edited.
fn reseed_personal_widgets(
    mut commands: Commands,
    mut edit: ResMut<PersonalLightingEdit>,
    sky_sliders: Query<(Entity, &SkySliderRow, &SliderRange, &SliderValue)>,
    water_sliders: Query<(Entity, &WaterSliderRow, &SliderRange, &SliderValue)>,
    mut colors: Query<(&ColorSwatchKnob, &mut ColorSwatchValue)>,
    mut textures: Query<(&TextureSwatchKnob, &mut TextureSwatchValue)>,
) {
    if !edit.reseed {
        return;
    }
    // The content is built on the window's *first* open, by a deferred builder
    // whose commands land a frame later — so the capture that asked for this
    // reseed can arrive before there is a single widget to seed. Hold the
    // request rather than spend it on an empty query, or every slider would
    // spend that first open showing the bottom of its range instead of the sky
    // it is editing.
    if sky_sliders.is_empty() {
        return;
    }
    edit.reseed = false;
    let (Some(sky), Some(water)) = (edit.sky.as_deref(), edit.water.as_ref()) else {
        return;
    };
    // `SliderValue` is an immutable component, so a new value is *inserted*
    // rather than assigned — and only when it differs, since an insert marks the
    // component changed whether or not it holds anything new.
    // Compared by bits, not within a tolerance: this asks "would the insert
    // change anything", and an insert marks the component changed whether or not
    // it carries a new number. Two values that differ in their last bit are two
    // different slider positions to everything downstream.
    for (entity, row_info, range, value) in &sky_sliders {
        let wanted = range.clamp(row_info.0.read(sky));
        if value.0.to_bits() != wanted.to_bits() {
            commands.entity(entity).insert(SliderValue(wanted));
        }
    }
    for (entity, row_info, range, value) in &water_sliders {
        let wanted = range.clamp(row_info.0.read(water));
        if value.0.to_bits() != wanted.to_bits() {
            commands.entity(entity).insert(SliderValue(wanted));
        }
    }
    for (knob, mut value) in &mut colors {
        value.0 = knob.0.read(sky, water);
    }
    for (knob, mut value) in &mut textures {
        value.0 = knob.0.read(sky, water);
    }
}

/// Push the edit buffers into the local environment layer.
///
/// Both tracks every time: the sky and the water are separate tracks and this
/// window owns both, so writing only the one that changed would leave the other
/// following the region while the user is plainly editing it.
fn push_personal_edits(
    mut edit: ResMut<PersonalLightingEdit>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    if !edit.dirty {
        return;
    }
    let Some(mut environment) = environment else {
        return;
    };
    edit.dirty = false;
    if let Some(sky) = edit.sky.clone() {
        // No asset id: this sky is the user's own edit, and no inventory row
        // names it — a preset list must not claim one is in force.
        environment.set_local_instant(EnvironmentAsset::Sky(sky), None);
    }
    if let Some(water) = edit.water.clone() {
        environment.set_local_instant(EnvironmentAsset::Water(water), None);
    }
}

/// Reset pressed: ask first. Dropping a personal environment somebody has been
/// building is not undoable, which is why the reference asks too.
fn on_reset_pressed(
    press: On<Pointer<Press>>,
    buttons: Query<(), With<ResetButton>>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut show: MessageWriter<ShowNotification>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    if buttons.contains(press.entity) {
        show.write(ShowNotification::new(RESET_CONFIRM));
    }
}

/// The confirmation came back: on Yes, drop the whole local layer (which is what
/// puts the shared environment back), forget the buffers and close the window —
/// the reference closes it too, since what it was editing no longer exists.
fn handle_reset_confirmation(
    mut responses: MessageReader<NotificationResponse>,
    mut edit: ResMut<PersonalLightingEdit>,
    environment: Option<ResMut<EnvironmentState>>,
    ui: Option<Res<PersonalLightingUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let confirmed = responses
        .read()
        .any(|response| response.template == RESET_CONFIRM && response.button == Some("OK"));
    if !confirmed {
        return;
    }
    if let Some(mut environment) = environment {
        environment.set_fixed(None);
    }
    edit.sky = None;
    edit.water = None;
    edit.dirty = false;
    edit.reseed = false;
    if let Some(ui) = ui
        && let Ok(mut shown) = panels.get_mut(ui.panel)
    {
        shown.0 = false;
    }
}
