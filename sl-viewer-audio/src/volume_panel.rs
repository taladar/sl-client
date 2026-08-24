//! The volume panel (`viewer-volume-panel`): the speaker / volume control in the
//! bottom bar's reserved trailing slot, and the pulldown of per-category sliders
//! behind it.
//!
//! Placement and shape mirror the reference viewer's **Vintage** skin: an inline
//! master-volume slider with a mute toggle in the bar, plus a small **▲** button
//! that opens a pulldown of the remaining categories — sound effects, ambient,
//! UI, music, media and voice — each with its own slider and mute (the canonical
//! `panel_volume_pulldown` set).
//!
//! Every control binds to a persisted setting through the two-way
//! [settings-binding layer](crate::settings_binding); a bridge system applies
//! those settings live to the shared [`sl_audio`] mixer's buses. This is the
//! user-facing face of the buses [`crate::audio`] opens: **mute retains the
//! slider level and never stops a source**, because mute is a separate boolean
//! setting from the volume, so the mixer drives the bus gain to zero while the
//! remembered level (the slider) is untouched.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{
    Activate, Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue,
};
use bevy::window::PrimaryWindow;

use sl_audio::{AudioMixer as _, Bus, BusLevel, Mixer};
use sl_settings::SettingValue;

use crate::i18n::Translated;
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_slider};
use crate::ui::BottomArea;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, column, row};
use crate::ui_font::UiFont;

/// The persisted-settings section the bus levels live under (`[audio.bus]`),
/// kept distinct from the parcel-stream player's own `[audio]` keys.
const BUS_SECTION: &[&str] = &["audio", "bus"];

/// The persisted-settings section [`SETTING_MUTE_WHEN_MINIMIZED`] lives under
/// (`[audio]` — a behaviour toggle, not a bus level).
const AUDIO_SECTION: &[&str] = &["audio"];

/// The reference `MuteWhenMinimized` setting name (the name is kept so a
/// reference value ports across): silence the master bus while the window is
/// unfocused. Deliberate deviation: the reference mutes only while
/// *minimised*; this viewer mutes on focus loss, because Wayland gives an
/// application no reliable minimised signal — focus is the portable superset.
/// Applied by `apply_volume_settings_to_mixer` as a mixer-side overlay: the
/// stored `master_mute` setting is never written, so the bar's mute glyph
/// does not flip, and refocusing restores the exact level (mute retains the
/// bus gain).
pub const SETTING_MUTE_WHEN_MINIMIZED: &str = "MuteWhenMinimized";

/// Slider track width in logical pixels.
const TRACK_WIDTH: f32 = 90.0;
/// Slider thumb width in logical pixels.
const THUMB_WIDTH: f32 = 10.0;
/// Slider track / thumb height in logical pixels.
const TRACK_HEIGHT: f32 = 12.0;
/// The pulldown / bar background.
const PANEL_BACKGROUND: Color = Color::srgba(0.08, 0.09, 0.12, 0.96);
/// The bar background.
const BAR_BACKGROUND: Color = Color::srgba(0.08, 0.09, 0.12, 0.92);
/// Label text colour.
const LABEL_COLOR: Color = Color::srgb(0.9, 0.9, 0.92);
/// Dimmed (muted) label / glyph colour.
const LABEL_DIM: Color = Color::srgb(0.62, 0.65, 0.72);
/// Button border.
const BUTTON_BORDER: Color = Color::srgb(0.3, 0.3, 0.35);
/// Button fill.
const BUTTON_FILL: Color = Color::srgb(0.16, 0.17, 0.2);
/// Slider track fill.
const TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);
/// Slider thumb fill.
const THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);
/// Font size for the cluster's glyphs and labels.
const FONT_SIZE: f32 = 13.0;

/// The default linear gain a fresh install starts a bus at, matching the
/// reference viewer's `AudioLevel*` settings-defaults (master full, effects and
/// UI half, streams quieter, voice a little louder). Mute defaults off, also
/// like the reference (`MuteAudio = 0`).
const fn default_gain(bus: Bus) -> f32 {
    match bus {
        Bus::Master => 1.0,
        Bus::Sfx | Bus::Ambient | Bus::Ui => 0.5,
        Bus::Music | Bus::Media => 0.3,
        Bus::Voice => 0.7,
    }
}

/// The setting name for a bus's remembered linear volume (`0.0..=1.0`). Public so
/// the parcel-stream bar can bind its inline volume slider to the **music** bus
/// (the stream's single volume, not one in series with the bus).
#[must_use]
pub fn bus_volume_setting(bus: Bus) -> String {
    volume_key(bus)
}

/// The setting name for a bus's mute flag. Public for the parcel-stream bar's
/// inline mute (see [`bus_volume_setting`]).
#[must_use]
pub fn bus_mute_setting(bus: Bus) -> String {
    mute_key(bus)
}

/// The setting name for a bus's remembered linear volume (`0.0..=1.0`).
fn volume_key(bus: Bus) -> String {
    format!("{}_volume", bus.key())
}

/// The setting name for a bus's mute flag.
fn mute_key(bus: Bus) -> String {
    format!("{}_mute", bus.key())
}

/// The setting name of the master-volume control, for the other surfaces that
/// bind the same value (Quick Preferences, the Preferences audio tab).
#[must_use]
pub fn master_volume_setting() -> String {
    volume_key(Bus::Master)
}

/// The Fluent label key for a bus's row.
const fn label_key(bus: Bus) -> &'static str {
    match bus {
        Bus::Master => "volume-panel-master",
        Bus::Sfx => "volume-panel-sfx",
        Bus::Ambient => "volume-panel-ambient",
        Bus::Ui => "volume-panel-ui",
        Bus::Music => "volume-panel-music",
        Bus::Media => "volume-panel-media",
        Bus::Voice => "volume-panel-voice",
    }
}

/// Marker for the pulldown panel root (the thing whose [`UiPanelShown`] toggles).
#[derive(Component)]
struct VolumePanelRoot;

/// The glyph text of a mute control, kept in sync with the bus's mute state.
#[derive(Component, Clone, Copy)]
struct VolumeMuteGlyph(Bus);

/// The ▲ button that opens / closes the pulldown.
#[derive(Component)]
struct VolumePanelToggleButton;

/// A slider thumb positioned from its parent slider's value each frame.
#[derive(Component)]
struct VolumeThumb;

/// Fired when a mute button is pressed.
#[derive(Message)]
struct ToggleMute(Bus);

/// Fired when the ▲ toggle button is pressed.
#[derive(Message)]
struct ToggleVolumePanel;

/// The volume-panel plugin: register the settings, spawn the cluster, and keep
/// the mixer, thumbs and glyphs in sync.
#[derive(Debug)]
pub struct VolumePanelPlugin;

impl Plugin for VolumePanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ToggleMute>()
            .add_message::<ToggleVolumePanel>()
            .add_systems(Startup, register_volume_settings)
            // Order the cluster after the parcel-audio bar so `upper_trailing`
            // lays them out (leading→trailing) as parcel-audio · volume. The
            // quick-prefs button places itself after this one: it sits
            // trailing-most, and stating it from there keeps the audio cluster
            // from naming a preferences module it otherwise never touches.
            .add_systems(
                Update,
                spawn_volume_controls.after(crate::parcel_audio::spawn_parcel_audio_bar),
            )
            .add_systems(
                Update,
                (
                    apply_mute_toggles,
                    apply_panel_toggle,
                    sync_mute_glyphs,
                    drive_volume_thumbs,
                    apply_volume_settings_to_mixer,
                )
                    .chain(),
            );
    }
}

/// Startup: declare a persisted volume + mute setting for every bus, plus
/// the mute-on-focus-loss toggle.
fn register_volume_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_settings(&mut settings);
}

/// Declare this module's persisted settings (split from the Startup system so
/// tests can register on a bare store).
fn register_settings(settings: &mut ViewerSettings) {
    for bus in Bus::ALL {
        settings.register_in(
            BUS_SECTION,
            &volume_key(bus),
            SettingValue::F32(default_gain(bus)),
            "Linear volume (0-1) for this audio bus",
        );
        settings.register_in(
            BUS_SECTION,
            &mute_key(bus),
            SettingValue::Bool(false),
            "Whether this audio bus is muted (mute retains the volume level)",
        );
    }
    settings.register_in(
        AUDIO_SECTION,
        SETTING_MUTE_WHEN_MINIMIZED,
        SettingValue::Bool(false),
        "Mute audio while the viewer window is minimised / unfocused",
    );
}

/// Spawn the volume cluster into the bottom area's trailing slot, once (the
/// [`Local`] latch waits for the bottom toolbar's host to exist).
pub fn spawn_volume_controls(
    mut commands: Commands,
    area: Option<Res<BottomArea>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Some(area) = area else {
        return;
    };

    let wrapper = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexEnd,
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("volume-cluster"),
            ChildOf(area.upper_trailing),
        ))
        .id();

    // The pulldown, above the bar, hidden until the ▲ toggle opens it.
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Percent(100.0),
                right: Val::Px(0.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(4.0))
            },
            BackgroundColor(PANEL_BACKGROUND),
            UiPanelShown(false),
            VolumePanelRoot,
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("volume-pulldown"),
            ChildOf(wrapper),
        ))
        .id();
    // The pulldown title.
    commands.spawn((
        Text::default(),
        Translated::new("volume-panel-title"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(panel),
    ));
    for (index, bus) in Bus::CATEGORIES.into_iter().enumerate() {
        spawn_volume_row(&mut commands, panel, bus, tab(200, index.saturating_mul(2)));
    }

    // The inline bar: master mute, master slider, and the ▲ pulldown toggle.
    let cluster = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..row(Val::Px(6.0))
            },
            BackgroundColor(BAR_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("volume-bar"),
            ChildOf(wrapper),
        ))
        .id();
    spawn_mute_button(&mut commands, cluster, Bus::Master, 190);
    spawn_bound_slider(&mut commands, cluster, Bus::Master, 191);
    spawn_toggle_button(&mut commands, cluster, 192);

    *spawned = true;
}

/// Compute a tab index from a base and a per-row offset without a panicking add.
fn tab(base: i32, index: usize) -> i32 {
    base.saturating_add(i32::try_from(index).unwrap_or(0))
}

/// Spawn one pulldown row: label, slider and mute button for `bus`.
fn spawn_volume_row(commands: &mut Commands, parent: Entity, bus: Bus, tab_base: i32) {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("volume-row:{}", bus.key())),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Node {
            width: Val::Px(64.0),
            ..default()
        },
        Text::default(),
        Translated::new(label_key(bus)),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    spawn_bound_slider(commands, row_entity, bus, tab_base);
    spawn_mute_button(commands, row_entity, bus, tab_base.saturating_add(1));
}

/// Spawn a settings-bound slider (track + thumb) for `bus` on `parent`.
fn spawn_bound_slider(commands: &mut Commands, parent: Entity, bus: Bus, tab_index: i32) {
    let slider = commands
        .spawn((
            bound_slider(
                SettingBinding::global(volume_key(bus)),
                SliderRange::new(0.0, 1.0),
                SliderStep(0.05),
            ),
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(tab_index),
            Pickable::default(),
            Name::new(format!("volume-slider:{}", bus.key())),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        SliderThumb,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(THUMB_WIDTH),
            height: Val::Px(TRACK_HEIGHT),
            ..default()
        },
        LogicalInset(LogicalRect {
            inline_start: Val::Px(0.0),
            ..LogicalRect::ZERO
        }),
        BackgroundColor(THUMB_FILL),
        VolumeThumb,
        Pickable::IGNORE,
        ChildOf(slider),
    ));
}

/// Spawn a mute toggle button (a speaker glyph) for `bus` on `parent`.
fn spawn_mute_button(commands: &mut Commands, parent: Entity, bus: Bus, tab_index: i32) {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_FILL),
            Pickable::default(),
            Name::new(format!("volume-mute:{}", bus.key())),
            ChildOf(parent),
        ))
        .observe(
            move |_activate: On<Activate>, mut writer: MessageWriter<ToggleMute>| {
                writer.write(ToggleMute(bus));
            },
        )
        .id();
    commands.spawn((
        Text::new("🔊"),
        VolumeMuteGlyph(bus),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// Spawn the ▲ button that opens / closes the pulldown.
fn spawn_toggle_button(commands: &mut Commands, parent: Entity, tab_index: i32) {
    let button = commands
        .spawn((
            Button,
            VolumePanelToggleButton,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_FILL),
            Pickable::default(),
            Name::new("volume-panel-toggle"),
            ChildOf(parent),
        ))
        .observe(
            |_activate: On<Activate>, mut writer: MessageWriter<ToggleVolumePanel>| {
                writer.write(ToggleVolumePanel);
            },
        )
        .id();
    commands.spawn((
        Text::new("▲"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// Flip a bus's mute setting when its mute button is pressed.
fn apply_mute_toggles(
    mut events: MessageReader<ToggleMute>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let Some(mut settings) = settings else {
        events.clear();
        return;
    };
    for ToggleMute(bus) in events.read() {
        let key = mute_key(*bus);
        let now = settings.store().get_bool(&key).unwrap_or(false);
        settings.set(sl_settings::Scope::Global, &key, SettingValue::Bool(!now));
    }
}

/// Open / close the pulldown when the ▲ toggle is pressed.
fn apply_panel_toggle(
    mut events: MessageReader<ToggleVolumePanel>,
    mut panels: Query<&mut UiPanelShown, With<VolumePanelRoot>>,
) {
    let mut toggles = 0u32;
    for _ in events.read() {
        toggles = toggles.wrapping_add(1);
    }
    if toggles == 0 {
        return;
    }
    for mut shown in &mut panels {
        shown.0 = !shown.0;
    }
}

/// Keep each mute glyph (🔊 / 🔇) and its dimming in step with the bus's mute
/// setting.
fn sync_mute_glyphs(
    settings: Option<Res<ViewerSettings>>,
    mut glyphs: Query<(&VolumeMuteGlyph, &mut Text, &mut TextColor)>,
) {
    let Some(settings) = settings else {
        return;
    };
    for (glyph, mut text, mut color) in &mut glyphs {
        let muted = settings
            .store()
            .get_bool(&mute_key(glyph.0))
            .unwrap_or(false);
        let want = if muted { "🔇" } else { "🔊" };
        if text.0 != want {
            want.clone_into(&mut text.0);
        }
        let want_color = if muted { LABEL_DIM } else { LABEL_COLOR };
        if color.0 != want_color {
            color.0 = want_color;
        }
    }
}

/// Position each volume thumb from its parent slider's value.
fn drive_volume_thumbs(
    sliders: Query<(&SliderValue, &SliderRange, &Children), With<Slider>>,
    mut thumbs: Query<&mut LogicalInset, With<VolumeThumb>>,
) {
    for (value, range, children) in &sliders {
        let span = range.span();
        let fraction = if span > f32::EPSILON {
            ((value.0 - range.start()) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let offset = fraction * (TRACK_WIDTH - THUMB_WIDTH);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child)
                && inset.0.inline_start != Val::Px(offset)
            {
                inset.0.inline_start = Val::Px(offset);
            }
        }
    }
}

/// Whether the master bus should be silenced for focus reasons: the
/// mute-on-focus-loss setting is on and the window is unfocused.
const fn master_silenced(mute_when_minimized: bool, focused: bool) -> bool {
    mute_when_minimized && !focused
}

/// Bridge: push every bus's persisted volume + mute into the mixer each frame.
///
/// [`Mixer::set_bus_level`] diffs against its own copy, so an unchanged value is
/// a no-op — calling every frame is cheap and keeps the buses correct after any
/// settings change (panel, quick-prefs or preferences tab), a fresh login, or a
/// device hot-plug that rebuilt the graph.
///
/// [`SETTING_MUTE_WHEN_MINIMIZED`] overlays the **master** bus's mute here,
/// mixer-side only: the stored `master_mute` setting is never written (see
/// the constant's doc). A missing primary window reads as focused.
fn apply_volume_settings_to_mixer(
    settings: Option<Res<ViewerSettings>>,
    mixer: Option<NonSendMut<Mixer>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let (Some(settings), Some(mut mixer)) = (settings, mixer) else {
        return;
    };
    let mute_when_minimized = settings
        .store()
        .get_bool(SETTING_MUTE_WHEN_MINIMIZED)
        .unwrap_or(false);
    let focused = windows.iter().next().is_none_or(|window| window.focused);
    let silenced = master_silenced(mute_when_minimized, focused);
    for bus in Bus::ALL {
        let gain = settings
            .store()
            .get_f32(&volume_key(bus))
            .unwrap_or_else(|_| default_gain(bus));
        let muted = settings.store().get_bool(&mute_key(bus)).unwrap_or(false);
        let mut level = BusLevel::from_linear(gain);
        level.set_muted(muted || (matches!(bus, Bus::Master) && silenced));
        mixer.set_bus_level(bus, level);
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy::window::PrimaryWindow;
    use pretty_assertions::assert_eq;
    use sl_audio::{AudioMixer as _, Bus, BusLevel, Mixer, MixerConfig};
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        SETTING_MUTE_WHEN_MINIMIZED, apply_volume_settings_to_mixer, default_gain, master_silenced,
        mute_key, volume_key,
    };
    use crate::settings::ViewerSettings;

    /// A [`ViewerSettings`] with this module's settings registered.
    fn settings() -> ViewerSettings {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        super::register_settings(&mut settings);
        settings
    }

    #[test]
    fn registered_defaults_match_consts() {
        let settings = settings();
        let store = settings.store();
        for bus in Bus::ALL {
            assert_eq!(
                store.get_f32(&volume_key(bus)).ok(),
                Some(default_gain(bus)),
                "volume default of {bus:?}"
            );
            assert_eq!(
                store.get_bool(&mute_key(bus)).ok(),
                Some(false),
                "mute default of {bus:?}"
            );
        }
        assert_eq!(
            store.get_bool(SETTING_MUTE_WHEN_MINIMIZED).ok(),
            Some(false)
        );
    }

    #[test]
    fn master_silence_is_focus_and_setting() {
        assert!(!master_silenced(false, true));
        assert!(!master_silenced(false, false));
        assert!(!master_silenced(true, true));
        assert!(master_silenced(true, false));
        // The contract the feature rides: mute retains the bus gain. The
        // values are stored verbatim, so `Option` equality is exact.
        let mut level = BusLevel::from_linear(0.4);
        level.set_muted(true);
        assert_eq!(Some(level.gain()), Some(0.4));
        assert_eq!(Some(level.effective_gain()), Some(0.0));
    }

    /// An unfocused window with the setting on silences the master bus in the
    /// mixer without ever writing the stored `master_mute` setting (so the
    /// bar's mute glyph stays put, and refocusing restores the exact level).
    #[test]
    fn focus_mute_never_writes_the_store() {
        let Ok(mixer) = Mixer::new(&MixerConfig::default()) else {
            // No audio backend in this environment; the pure-fn test above
            // carries the behaviour.
            return;
        };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(settings())
            .insert_non_send(mixer)
            .add_systems(Update, apply_volume_settings_to_mixer);
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_MUTE_WHEN_MINIMIZED,
            SettingValue::Bool(true),
        );
        app.world_mut().spawn((
            Window {
                focused: false,
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        let master = app.world().non_send::<Mixer>().bus_level(Bus::Master);
        assert_eq!(
            Some(master.effective_gain()),
            Some(0.0),
            "master silenced in the mixer"
        );
        assert_eq!(
            Some(master.gain()),
            Some(default_gain(Bus::Master)),
            "level retained"
        );
        let settings = app.world().resource::<ViewerSettings>();
        assert_eq!(
            settings.store().get_bool(&mute_key(Bus::Master)).ok(),
            Some(false),
            "the stored master_mute setting is untouched"
        );
    }
}
