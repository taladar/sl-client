//! The Preferences **audio** tab (`viewer-preferences-audio-tab`).
//!
//! Surfaces the audio settings as controls bound to the typed settings store
//! through the preferences shell ([`crate::preferences`]): the master and
//! per-category bus volumes with their mutes, the behaviour toggles (mute on
//! focus loss, ear position, collision sounds), the streaming autoplay
//! toggles, and the output-device picker.
//!
//! Ownership: this module registers **nothing** — every row binds a setting
//! owned (registered and applied) by its feature module:
//! [`crate::volume_panel`] (the bus volumes / mutes and
//! [`crate::volume_panel::SETTING_MUTE_WHEN_MINIMIZED`]), [`crate::audio`]
//! (ear position, output device), [`crate::world_sounds`] (collision
//! sounds), [`crate::parcel_audio`] (music autoplay) and [`crate::media_prim`]
//! (media autoplay). All of those register in `Startup` systems or in
//! [`crate::settings::ViewerSettings`]'s `load` — always before the
//! floater's deferred first-open build, so binding the keys here is safe.
//!
//! The **output device** combo starts from the device names enumerated at
//! tab-build time ([`Mixer::output_devices`]) and **re-enumerates while the
//! preferences floater is open** (`refresh_output_device_options`, every
//! `DEVICE_POLL_SECONDS`) so a hot-plugged PipeWire / PulseAudio device
//! appears without a restart — a poll, because cpal has no device-change
//! notification to subscribe to. The options update in place through
//! [`SetComboOptions`] (deferred while the popover is open); the paired
//! [`ComboBindingValues`] moves in the same pass so option index ↔ setting
//! value never skews. The device *names* ride the Fluent key-fallback: the
//! combo translates its option labels, and a key no bundle defines renders
//! as itself, which is exactly right for a hardware name (the dev-only
//! pseudolocale garbles them, which is acceptable). Voice rows (devices,
//! push-to-talk, visualizers) arrive with the voice task, not here.
//!
//! Reference (Firestorm, read-only): `panel_preferences_sound.xml`,
//! `llfloaterpreference.cpp`.

use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderStep};
use sl_audio::{Bus, Mixer};
use sl_settings::SettingValue;

use crate::preferences::{
    PreferencesUi, spawn_pref_checkbox, spawn_pref_combo, spawn_pref_combo_with_anchor,
    spawn_pref_section, spawn_pref_slider,
};
use crate::settings_binding::{ComboBindingValues, SettingBinding};
use crate::ui::UiPanelShown;
use crate::ui_combo::SetComboOptions;
use crate::volume_panel::{bus_mute_setting, bus_volume_setting};

/// The stable id of this tab in `crate::preferences::PREF_TABS`.
pub(crate) const TAB_ID: &str = "audio";

/// The volume sliders' step (the volume panel's, one twentieth).
const VOLUME_STEP: f32 = 0.05;

/// How often the output-device list re-enumerates while the preferences
/// floater is open (see the module doc; enumeration opens the audio host, so
/// it is not a per-frame thing).
const DEVICE_POLL_SECONDS: f32 = 2.0;

/// Marks the output-device combo's anchor, so
/// `refresh_output_device_options` finds it.
#[derive(Component, Debug, Clone, Copy)]
struct OutputDeviceCombo;

/// The Fluent label key of a bus's volume-slider row.
const fn volume_row_key(bus: Bus) -> &'static str {
    match bus {
        Bus::Master => "preferences-row-volume-master",
        Bus::Sfx => "preferences-row-volume-sfx",
        Bus::Ambient => "preferences-row-volume-ambient",
        Bus::Ui => "preferences-row-volume-ui",
        Bus::Music => "preferences-row-volume-music",
        Bus::Media => "preferences-row-volume-media",
        Bus::Voice => "preferences-row-volume-voice",
    }
}

/// The Fluent label key of a bus's mute-checkbox row (distinct per bus, so
/// the shell's search finds the right one).
const fn mute_row_key(bus: Bus) -> &'static str {
    match bus {
        Bus::Master => "preferences-row-mute-master",
        Bus::Sfx => "preferences-row-mute-sfx",
        Bus::Ambient => "preferences-row-mute-ambient",
        Bus::Ui => "preferences-row-mute-ui",
        Bus::Music => "preferences-row-mute-music",
        Bus::Media => "preferences-row-mute-media",
        Bus::Voice => "preferences-row-mute-voice",
    }
}

/// The output-device combo's options: the system default first, then one
/// option per enumerated device, each writing its name as the setting value.
fn device_options(devices: Vec<String>) -> Vec<(String, SettingValue)> {
    let mut options = vec![(
        String::from("preferences-audio-device-default"),
        SettingValue::String(String::new()),
    )];
    for name in devices {
        let value = SettingValue::String(name.clone());
        options.push((name, value));
    }
    options
}

/// Build the audio tab's content into its panel (the
/// `crate::preferences::PREF_TABS` `build` hook).
pub(crate) fn build_audio_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-volumes");
    for bus in Bus::ALL {
        spawn_pref_slider(
            commands,
            panel,
            volume_row_key(bus),
            SettingBinding::global(bus_volume_setting(bus)),
            SliderRange::new(0.0, 1.0),
            SliderStep(VOLUME_STEP),
        );
        spawn_pref_checkbox(
            commands,
            panel,
            mute_row_key(bus),
            SettingBinding::global(bus_mute_setting(bus)),
        );
    }

    spawn_pref_section(commands, panel, "preferences-section-audio-behaviour");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-mute-when-unfocused",
        SettingBinding::global(crate::volume_panel::SETTING_MUTE_WHEN_MINIMIZED),
    );
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-ear-mode",
        SettingBinding::global(crate::audio::SETTING_EAR_LOCATION),
        &[
            ("preferences-ear-camera", SettingValue::U32(0)),
            ("preferences-ear-avatar", SettingValue::U32(1)),
        ],
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-collision-sounds",
        SettingBinding::global(crate::world_sounds::SETTING_COLLISION_SOUNDS),
    );

    spawn_pref_section(commands, panel, "preferences-section-streaming");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-music-autoplay",
        SettingBinding::global(crate::parcel_audio::MUSIC_ENABLED_SETTING),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-media-autoplay",
        SettingBinding::global(crate::media_prim::MEDIA_AUTO_PLAY_SETTING),
    );

    spawn_pref_section(commands, panel, "preferences-section-audio-device");
    let options = device_options(Mixer::output_devices());
    let option_refs: Vec<(&str, SettingValue)> = options
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let (_row, anchor) = spawn_pref_combo_with_anchor(
        commands,
        panel,
        "preferences-row-output-device",
        SettingBinding::global(crate::audio::SETTING_OUTPUT_DEVICE),
        &option_refs,
    );
    commands.entity(anchor).insert(OutputDeviceCombo);
}

/// Re-enumerate the output devices while the preferences floater is open and
/// push any change into the device combo: the option labels through
/// [`SetComboOptions`] (an equal list is a no-op there), the paired
/// [`ComboBindingValues`] in the same pass. Closed, the poll timer resets so
/// the next open re-enumerates immediately.
fn refresh_output_device_options(
    time: Res<Time>,
    mut next_poll: Local<Option<f32>>,
    ui: Option<Res<PreferencesUi>>,
    panels: Query<&UiPanelShown>,
    mut combos: Query<(Entity, &mut ComboBindingValues), With<OutputDeviceCombo>>,
    mut writer: MessageWriter<SetComboOptions>,
) {
    let open = ui.is_some_and(|ui| panels.get(ui.root).is_ok_and(|shown| shown.0));
    if !open {
        *next_poll = None;
        return;
    }
    let now = time.elapsed_secs();
    if next_poll.is_some_and(|due| now < due) {
        return;
    }
    *next_poll = Some(now + DEVICE_POLL_SECONDS);
    let options = device_options(Mixer::output_devices());
    for (combo, mut values) in &mut combos {
        let new_values: Vec<SettingValue> =
            options.iter().map(|(_, value)| value.clone()).collect();
        // Guarded write, so an unchanged list does not dirty the component
        // every poll.
        if values.0 != new_values {
            values.0 = new_values;
        }
        writer.write(SetComboOptions {
            combo,
            labels: options.iter().map(|(label, _)| label.clone()).collect(),
        });
    }
}

/// The audio tab's runtime side (the tab *content* is built by the shell
/// through `crate::preferences::PREF_TABS`): the live output-device
/// re-enumeration.
#[derive(Debug)]
pub struct PreferencesAudioPlugin;

impl Plugin for PreferencesAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, refresh_output_device_options);
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_audio::Bus;
    use sl_settings::SettingValue;

    use super::{OutputDeviceCombo, device_options, mute_row_key, volume_row_key};
    use crate::preferences::PreferencesUi;
    use crate::settings_binding::ComboBindingValues;
    use crate::ui::UiPanelShown;
    use crate::ui_combo::SetComboOptions;

    /// Every bus gets its own volume and mute row label — 14 distinct keys,
    /// so the shell's per-label search stays meaningful.
    #[test]
    fn bus_row_keys_are_distinct_and_total() {
        let mut keys: Vec<&str> = Bus::ALL
            .into_iter()
            .flat_map(|bus| [volume_row_key(bus), mute_row_key(bus)])
            .collect();
        assert_eq!(keys.len(), 14);
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 14, "row label keys must be distinct");
    }

    #[test]
    fn device_options_lead_with_system_default() {
        let options = device_options(vec!["Speakers".to_owned(), "Headset".to_owned()]);
        let expected = vec![
            (
                "preferences-audio-device-default".to_owned(),
                SettingValue::String(String::new()),
            ),
            (
                "Speakers".to_owned(),
                SettingValue::String("Speakers".to_owned()),
            ),
            (
                "Headset".to_owned(),
                SettingValue::String("Headset".to_owned()),
            ),
        ];
        assert_eq!(options, expected);
    }

    /// The device poll runs only while the preferences floater is open, and
    /// its first pass writes the binding values with the system default
    /// leading (whatever real devices the host enumerates follow).
    #[test]
    fn device_refresh_gated_on_the_open_floater() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<SetComboOptions>()
            .add_systems(Update, super::refresh_output_device_options);
        let root = app.world_mut().spawn(UiPanelShown(false)).id();
        let strip = app.world_mut().spawn_empty().id();
        let field = app.world_mut().spawn_empty().id();
        app.world_mut().insert_resource(PreferencesUi {
            root,
            tab_strip: strip,
            search_field: field,
        });
        let combo = app
            .world_mut()
            .spawn((OutputDeviceCombo, ComboBindingValues(Vec::new())))
            .id();
        app.update();
        let closed_len = app
            .world()
            .entity(combo)
            .get::<ComboBindingValues>()
            .map_or(usize::MAX, |values| values.0.len());
        assert_eq!(closed_len, 0, "a closed floater polls nothing");
        if let Some(mut shown) = app.world_mut().entity_mut(root).get_mut::<UiPanelShown>() {
            shown.0 = true;
        }
        app.update();
        let first = app
            .world()
            .entity(combo)
            .get::<ComboBindingValues>()
            .and_then(|values| values.0.first().cloned());
        assert_eq!(
            first,
            Some(SettingValue::String(String::new())),
            "open: the system default leads the refreshed values"
        );
    }
}
