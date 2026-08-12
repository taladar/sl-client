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
//! The **output device** combo is built from the device names enumerated at
//! tab-build time ([`Mixer::output_devices`]); a device hot-plugged later
//! shows up after a viewer restart (the row label says so — the
//! [`crate::preferences_graphics`] mirror-resolution idiom). The device
//! *names* ride the Fluent key-fallback: the combo translates its option
//! labels, and a key no bundle defines renders as itself, which is exactly
//! right for a hardware name (the dev-only pseudolocale garbles them, which
//! is acceptable). Voice rows (devices, push-to-talk, visualizers) arrive
//! with the voice task, not here.
//!
//! Reference (Firestorm, read-only): `panel_preferences_sound.xml`,
//! `llfloaterpreference.cpp`.

use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderStep};
use sl_audio::{Bus, Mixer};
use sl_settings::SettingValue;

use crate::preferences::{
    spawn_pref_checkbox, spawn_pref_combo, spawn_pref_section, spawn_pref_slider,
};
use crate::settings_binding::SettingBinding;
use crate::volume_panel::{bus_mute_setting, bus_volume_setting};

/// The stable id of this tab in [`crate::preferences::PREF_TABS`].
pub(crate) const TAB_ID: &str = "audio";

/// The volume sliders' step (the volume panel's, one twentieth).
const VOLUME_STEP: f32 = 0.05;

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
/// [`crate::preferences::PREF_TABS`] `build` hook).
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
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-output-device",
        SettingBinding::global(crate::audio::SETTING_OUTPUT_DEVICE),
        &option_refs,
    );
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_audio::Bus;
    use sl_settings::SettingValue;

    use super::{device_options, mute_row_key, volume_row_key};

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
}
