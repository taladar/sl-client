//! Reading a **legacy WindLight preset** off disk: the pre-EEP `.xml` sky and
//! water files a decade of Second Life photographers have folders of, converted
//! into the EEP [`SkySettings`] / [`WaterSettings`] this workspace carries
//! everywhere else.
//!
//! # What a legacy preset is
//!
//! One LLSD-XML map per preset, written by the pre-2019 viewer's WindLight
//! editor into `windlight/skies/<name>.xml` and `windlight/water/<name>.xml`,
//! where `<name>` is the preset's name **percent-escaped** so it can be a
//! filename (`%28SS%29%20Atmos%2000%3A00%202.xml` is `(SS) Atmos 00:00 2`).
//! There is no `type` tag inside the file, so which of the two a file is comes
//! from the folder it was found in — here, from the caller
//! ([`legacy_preset_from_bytes`]'s `kind`).
//!
//! # It is a *conversion*, not a decode
//!
//! A legacy sky holds a dozen of the roughly forty settings an EEP sky frame
//! has, and holds them differently: the scalars are the first element of a
//! four-real array (WindLight stored every slider as a colour), the sun and moon
//! are two Euler angles rather than quaternions, and the cloud scroll rate is
//! biased by ten. Everything the file does not carry comes from
//! [`SkySettings::legacy_windlight_default`] /
//! [`WaterSettings::legacy_default`] — the reference's own `defaults()`, which
//! is exactly what `translateLegacySettings` starts from
//! (`indra/llinventory/llsettingssky.cpp`).
//!
//! This is also where the `legacy_haze` block of a *modern* sky comes from: the
//! reference moves the seven haze values the old format carried into an inner
//! map so a converted sky can still be written back out in the old shape. This
//! workspace's decoder already reads that block either way (see
//! `sky_settings_from_llsd`), so the seven land in the same fields whichever
//! route a sky arrives by.
//!
//! # Deliberately lenient where the reference is not
//!
//! The reference reads a scalar out of `legacy[key][0]`, which yields `0.0` for
//! a preset that stored it as a bare real rather than an array — a shape some
//! third-party WindLight editors did write. [`legacy_scalar`] accepts both, so
//! such a file imports as its author meant it rather than as a zero. Nothing
//! else is loosened: a key that is absent is absent, and its default stands.
//!
//! # The legacy *day cycle* takes a folder, not a file
//!
//! `windlight/days/*.xml` is a different shape: an array of
//! `[keyframe, sky-preset-name]` pairs naming presets that live in a *sibling*
//! `skies/` directory, so one file's bytes are not enough to convert one.
//! [`legacy_preset_from_bytes`] therefore refuses [`SettingsKind::DayCycle`]
//! rather than half-answering, and [`legacy_day_cycle_from_bytes`] takes the
//! day file plus a callback that hands back a *named* sibling preset's bytes —
//! which keeps this module free of the filesystem while letting its caller
//! resolve a name to a file however it likes.
//!
//! Reference (Firestorm, read-only): `llsettingssky.cpp`
//! (`translateLegacySettings`, `translateLegacyHazeSettings`),
//! `llsettingswater.cpp` (`translateLegacySettings`), `llsettingsvo.cpp`
//! (`buildFromLegacyPreset`, `read_legacy_preset_data`), `llenvironment.cpp`
//! (`createSkyFromLegacyPreset`, `createDayCycleFromLegacyPreset`).

use std::collections::BTreeMap;

use sl_wire::{Llsd, parse_llsd_xml};

use crate::{
    Color, DayCycle, DayCycleFrame, EnvironmentAsset, SettingsKind, SkySettings, WaterSettings,
    azimuth_altitude_to_rotation,
};

use super::conversions::{
    cloud_pos_density_from_llsd, color_alpha_from_llsd, color_from_llsd, glow_from_llsd,
    optional_texture_member, scale_from_llsd, vec2_from_llsd,
};

/// Why a legacy WindLight preset could not be imported.
///
/// The reference reports all of these as one `WLImportFail` notification naming
/// the file; they are separated here because the three say different things to
/// whoever picked the file — it is not a preset at all, it is the *other* kind
/// of preset, or it is a day cycle this path cannot take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LegacyPresetError {
    /// The bytes did not parse as LLSD XML — not a WindLight preset at all
    /// (the reference's `read_legacy_preset_data` failing to parse).
    #[error("the file is not an LLSD XML document")]
    NotLlsd,
    /// The document parsed but is not a map, so it has no settings in it.
    #[error("the LLSD document is not a map")]
    NotAMap,
    /// The map holds none of the keys this kind of preset is made of — the
    /// reference's `converted_something == false`, which is how a *water*
    /// preset handed to the sky importer is caught (and the reverse).
    #[error("the document holds no legacy WindLight settings of this kind")]
    NothingConverted,
    /// A legacy *day cycle* was asked for, which one file cannot answer: its
    /// keyframes name sky presets that live in sibling files.
    #[error("legacy day cycles are not imported one file at a time")]
    DayCycleUnsupported,
}

/// Convert a legacy WindLight preset file into an [`EnvironmentAsset`], tagging
/// the frame with `name` (see [`legacy_preset_name`] for where that comes from).
///
/// `kind` says which of the two a legacy preset is, because the file does not:
/// the old format has no `type` tag, and the viewer knew a sky from water by the
/// directory it read. [`SettingsKind::DayCycle`] is refused — see the module
/// documentation.
///
/// # Errors
///
/// [`LegacyPresetError`] when the bytes are not an LLSD-XML map, hold none of
/// the keys the requested kind is made of, or ask for a day cycle.
pub fn legacy_preset_from_bytes(
    kind: SettingsKind,
    name: &str,
    bytes: &[u8],
) -> Result<EnvironmentAsset, LegacyPresetError> {
    let preset = preset_map_from_bytes(bytes)?;
    match kind {
        SettingsKind::Sky => {
            sky_settings_from_legacy_preset(name, &preset).map(EnvironmentAsset::Sky)
        }
        SettingsKind::Water => {
            water_settings_from_legacy_preset(name, &preset).map(EnvironmentAsset::Water)
        }
        SettingsKind::DayCycle => Err(LegacyPresetError::DayCycleUnsupported),
    }
}

/// Convert a legacy WindLight **sky** preset map into an EEP sky frame named
/// `name` (the reference's `LLSettingsSky::translateLegacySettings` over
/// `LLSettingsVOSky::buildFromLegacyPreset`).
///
/// Boxed because that is how [`EnvironmentAsset::Sky`] carries one, and a sky is
/// large enough that handing it back unboxed only to box it costs a copy.
///
/// # Errors
///
/// [`LegacyPresetError::NothingConverted`] when the map holds none of the keys a
/// WindLight sky is made of — which is what a water preset picked in the sky
/// editor looks like from here.
pub fn sky_settings_from_legacy_preset(
    name: &str,
    preset: &Llsd,
) -> Result<Box<SkySettings>, LegacyPresetError> {
    let mut sky = Box::new(SkySettings::legacy_windlight_default(name));
    let mut converted = false;
    // Every arm below is "if the preset carries this, take it" — the reference's
    // own shape, key for key, so a key it ignores is one this ignores too.
    let mut took = |present: bool| converted |= present;

    // The seven legacy-haze values (`translateLegacyHazeSettings`). They are the
    // *same* seven the modern `legacy_haze` sub-map holds, which is no accident:
    // that sub-map exists so a converted sky can be written back out in this
    // format.
    took(legacy_color(preset, "ambient", &mut sky.ambient));
    took(legacy_color(preset, "blue_density", &mut sky.blue_density));
    took(legacy_color(preset, "blue_horizon", &mut sky.blue_horizon));
    took(legacy_real(
        preset,
        "density_multiplier",
        &mut sky.density_multiplier,
    ));
    took(legacy_real(
        preset,
        "distance_multiplier",
        &mut sky.distance_multiplier,
    ));
    took(legacy_real(preset, "haze_density", &mut sky.haze_density));
    took(legacy_real(preset, "haze_horizon", &mut sky.haze_horizon));

    // The clouds.
    took(legacy_color(preset, "cloud_color", &mut sky.cloud_color));
    if let Some(value) = preset.get("cloud_pos_density1") {
        sky.cloud_pos_density1 = cloud_pos_density_from_llsd(Some(value));
        took(true);
    }
    if let Some(value) = preset.get("cloud_pos_density2") {
        sky.cloud_pos_density2 = cloud_pos_density_from_llsd(Some(value));
        took(true);
    }
    took(legacy_real(preset, "cloud_scale", &mut sky.cloud_scale));
    took(legacy_real(preset, "cloud_shadow", &mut sky.cloud_shadow));
    if let Some(value) = preset.get("cloud_scroll_rate") {
        sky.cloud_scroll_rate = legacy_cloud_scroll_rate(preset, value);
        took(true);
    }

    // The rest of the scalars and colours.
    took(legacy_real(preset, "gamma", &mut sky.gamma));
    if let Some(value) = preset.get("glow") {
        sky.glow = glow_from_llsd(Some(value));
        took(true);
    }
    took(legacy_real(preset, "max_y", &mut sky.max_y));
    // Star brightness is the one rescaled setting: WindLight's `0.0..=2.0`
    // slider became EEP's `0.0..=500.0`, and the reference multiplies by 250 on
    // the way in.
    if let Some(brightness) = legacy_scalar(preset.get("star_brightness")) {
        sky.star_brightness = brightness * 250.0;
        took(true);
    }
    if let Some(value) = preset.get("sunlight_color") {
        sky.sunlight_color = color_alpha_from_llsd(Some(value));
        took(true);
    }
    // The four planetary radii are EEP-era keys a WindLight preset normally has
    // none of; the reference reads them anyway, because its own converted
    // presets round-trip through this format.
    took(legacy_real(preset, "planet_radius", &mut sky.planet_radius));
    took(legacy_real(
        preset,
        "sky_bottom_radius",
        &mut sky.sky_bottom_radius,
    ));
    took(legacy_real(
        preset,
        "sky_top_radius",
        &mut sky.sky_top_radius,
    ));
    took(legacy_real(
        preset,
        "sun_arc_radians",
        &mut sky.sun_arc_radians,
    ));

    // The sun and the moon: two Euler angles in the old format, a pair of
    // quaternions in the new one. WindLight's east angle runs *clockwise*, hence
    // the negation, and its moon was always diametrically opposite its sun —
    // which is why only the sun has stored angles at all.
    if let (Some(east_angle), Some(sun_angle)) = (
        legacy_scalar(preset.get("east_angle")),
        legacy_scalar(preset.get("sun_angle")),
    ) {
        let azimuth = -east_angle;
        sky.sun_rotation = azimuth_altitude_to_rotation(azimuth, sun_angle);
        sky.moon_rotation =
            azimuth_altitude_to_rotation(azimuth + std::f32::consts::PI, -sun_angle);
        took(true);
    }

    if converted {
        Ok(sky)
    } else {
        Err(LegacyPresetError::NothingConverted)
    }
}

/// Convert a legacy WindLight **water** preset map into an EEP water frame named
/// `name` (the reference's `LLSettingsWater::translateLegacySettings`).
///
/// # Errors
///
/// [`LegacyPresetError::NothingConverted`] when the map holds none of the keys a
/// WindLight water preset is made of.
pub fn water_settings_from_legacy_preset(
    name: &str,
    preset: &Llsd,
) -> Result<WaterSettings, LegacyPresetError> {
    let mut water = WaterSettings::legacy_default(name);
    let mut converted = false;
    let mut took = |present: bool| converted |= present;

    // Water kept its own key spellings — camel-case, and `norm`/`wave` rather
    // than the EEP `normal_`/`wave1_direction` — so unlike the sky none of these
    // names carry over.
    took(legacy_real(
        preset,
        "blurMultiplier",
        &mut water.blur_multiplier,
    ));
    took(legacy_color(
        preset,
        "waterFogColor",
        &mut water.water_fog_color,
    ));
    took(legacy_real(
        preset,
        "waterFogDensity",
        &mut water.water_fog_density,
    ));
    took(legacy_real(
        preset,
        "underWaterFogMod",
        &mut water.underwater_fog_mod,
    ));
    took(legacy_real(
        preset,
        "fresnelOffset",
        &mut water.fresnel_offset,
    ));
    took(legacy_real(
        preset,
        "fresnelScale",
        &mut water.fresnel_scale,
    ));
    if preset.get("normalMap").is_some() {
        water.normal_map = optional_texture_member(preset, "normalMap");
        took(true);
    }
    if let Some(value) = preset.get("normScale") {
        water.normal_scale = scale_from_llsd(Some(value));
        took(true);
    }
    took(legacy_real(preset, "scaleAbove", &mut water.scale_above));
    took(legacy_real(preset, "scaleBelow", &mut water.scale_below));
    if let Some(value) = preset.get("wave1Dir") {
        water.wave1_direction = vec2_from_llsd(Some(value));
        took(true);
    }
    if let Some(value) = preset.get("wave2Dir") {
        water.wave2_direction = vec2_from_llsd(Some(value));
        took(true);
    }

    if converted {
        Ok(water)
    } else {
        Err(LegacyPresetError::NothingConverted)
    }
}

/// Parse a legacy preset file's bytes into the LLSD **map** a sky or water
/// preset is (the reference's `read_legacy_preset_data`, minus the reading).
///
/// # Errors
///
/// [`LegacyPresetError::NotLlsd`] when the bytes are not an LLSD-XML document,
/// and [`LegacyPresetError::NotAMap`] when they are one but not a map.
fn preset_map_from_bytes(bytes: &[u8]) -> Result<Llsd, LegacyPresetError> {
    let text = std::str::from_utf8(bytes).map_err(|_err| LegacyPresetError::NotLlsd)?;
    let preset = parse_llsd_xml(text).map_err(|_err| LegacyPresetError::NotLlsd)?;
    if preset.as_map().is_none() {
        return Err(LegacyPresetError::NotAMap);
    }
    Ok(preset)
}

/// Why a legacy WindLight **day cycle** could not be imported.
///
/// Separate from [`LegacyPresetError`] because a day cycle fails in ways one
/// preset cannot: it is a keyframe list rather than a settings map, and every
/// sky it names has to be found and converted *as well*, so the interesting
/// half of a failure is **which** sibling preset went wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LegacyDayCycleError {
    /// The bytes did not parse as LLSD XML at all.
    #[error("the file is not an LLSD XML document")]
    NotLlsd,
    /// The document parsed but is not the array of `[keyframe, sky-name]` pairs
    /// a legacy day cycle is — a sky or water preset picked in the day
    /// importer looks like this.
    #[error("the LLSD document is not a legacy day cycle")]
    NotADayCycle,
    /// The array held no keyframes, so there is no day in it.
    #[error("the day cycle names no keyframes")]
    NoKeyframes,
    /// A keyframe names a sky preset the caller could not find a file for.
    #[error("the sky preset \"{0}\" is not in the skies folder")]
    SkyNotFound(String),
    /// A named sky preset was found but would not convert.
    #[error("the sky preset \"{name}\" would not convert: {error}")]
    SkyNotConverted {
        /// The preset the day cycle's keyframe names.
        name: String,
        /// Why converting it failed.
        error: LegacyPresetError,
    },
    /// The accompanying water preset was found but would not convert.
    ///
    /// A water preset that is *missing* is not an error — see
    /// [`legacy_day_cycle_from_bytes`].
    #[error("the water preset \"{name}\" would not convert: {error}")]
    WaterNotConverted {
        /// The water preset's name, always [`LEGACY_DAY_WATER_PRESET`].
        name: String,
        /// Why converting it failed.
        error: LegacyPresetError,
    },
}

/// The water preset a converted legacy day cycle takes its single water
/// keyframe from — the reference hard-codes `"Default"` out of the `water/`
/// folder, because the old day format stored no water schedule of its own.
pub const LEGACY_DAY_WATER_PRESET: &str = "Default";

/// The prefix a converted day cycle's **sky** frames are keyed under, so a sky
/// and a water preset of the same name do not collide in the one `frames` map
/// the wire format has (the reference's `"sky:" + name`).
pub const LEGACY_DAY_SKY_FRAME_PREFIX: &str = "sky:";

/// The prefix a converted day cycle's **water** frame is keyed under — the
/// counterpart of [`LEGACY_DAY_SKY_FRAME_PREFIX`].
pub const LEGACY_DAY_WATER_FRAME_PREFIX: &str = "water:";

/// Convert a legacy WindLight **day cycle** file into an [`EnvironmentAsset`]
/// named `name`, pulling each sky preset its keyframes name — and the
/// accompanying water preset — through `read_preset`.
///
/// # The file is a schedule, not a settings map
///
/// A legacy day cycle is an LLSD array of `[keyframe, sky-preset-name]` pairs,
/// with the keyframe already a `0.0..=1.0` fraction of the day and the name
/// spelled **unescaped** (the percent-escaping belongs to the *filename* the
/// preset is stored under, not to the reference inside a day file). So the
/// caller is asked for presets by display name and resolves the file itself.
///
/// # What `read_preset` is asked for
///
/// - [`SettingsKind::Sky`] and a preset name, once per *distinct* name on the
///   track. A name it cannot answer fails the whole import
///   ([`LegacyDayCycleError::SkyNotFound`]) — a day cycle missing a third of
///   its skies is not a day cycle.
/// - [`SettingsKind::Water`] and [`LEGACY_DAY_WATER_PRESET`], once. `None`
///   here is **not** an error: the reference falls back to the `water/` folder
///   the viewer itself ships when the user's collection has none, and this
///   workspace ships no preset folder, so the fallback is
///   [`WaterSettings::legacy_default`] — which *is* the reference's own
///   `LLSettingsWater::defaults()`. A water preset that is present but broken
///   still fails, because that is a file the user meant to use.
///
/// # Divergences from the reference, and why
///
/// - The reference derives the sibling `skies/` and `water/` folders from the
///   day file's own directory rather than from its parent
///   (`LLSettingsVODay::buildFromLegacyPreset` calls `getDirName` once on the
///   full path), so on a real `windlight/days/…` layout it looks for
///   `days/skies` and falls back to the folder the viewer ships. Where a
///   preset comes from is this function's caller's business, which is the point
///   of taking a callback.
/// - The reference unescapes each frame name a second time on the way in
///   (`buildFromLegacyPresetFile` → `LLURI::unescape`). Day files store the
///   display name, so that is a no-op except on a name that happens to contain
///   something shaped like an escape; the name is taken verbatim here.
///
/// # Errors
///
/// [`LegacyDayCycleError`] when the bytes are not a legacy day cycle, name no
/// keyframes, or name a sky preset that cannot be found or converted.
pub fn legacy_day_cycle_from_bytes<R>(
    name: &str,
    bytes: &[u8],
    mut read_preset: R,
) -> Result<EnvironmentAsset, LegacyDayCycleError>
where
    R: FnMut(SettingsKind, &str) -> Option<Vec<u8>>,
{
    let keyframes = legacy_day_keyframes(bytes)?;
    let mut sky_frames: BTreeMap<String, SkySettings> = BTreeMap::new();
    let mut sky_track: Vec<DayCycleFrame> = Vec::with_capacity(keyframes.len());
    for (at, preset) in keyframes {
        let key = format!("{LEGACY_DAY_SKY_FRAME_PREFIX}{preset}");
        // A preset named twice is one frame referenced twice — the reference
        // collects the names into a `std::set` for exactly this reason.
        if !sky_frames.contains_key(&key) {
            let bytes = read_preset(SettingsKind::Sky, &preset)
                .ok_or_else(|| LegacyDayCycleError::SkyNotFound(preset.clone()))?;
            // Named for the key rather than for the preset, unlike the
            // reference. A frame's own `name` is not what a day-cycle asset is
            // keyed by — `day_cycle_to_llsd` writes the map key — so a cycle
            // whose frames are named anything else is one that reads back
            // differently from how it was built.
            let sky = convert_sibling(&key, &bytes, sky_settings_from_legacy_preset).map_err(
                |error| LegacyDayCycleError::SkyNotConverted {
                    name: preset.clone(),
                    error,
                },
            )?;
            drop(sky_frames.insert(key.clone(), *sky));
        }
        sky_track.push(DayCycleFrame {
            keyframe: at,
            name: key,
        });
    }

    let water_key = format!("{LEGACY_DAY_WATER_FRAME_PREFIX}{LEGACY_DAY_WATER_PRESET}");
    let water = match read_preset(SettingsKind::Water, LEGACY_DAY_WATER_PRESET) {
        Some(bytes) => convert_sibling(&water_key, &bytes, water_settings_from_legacy_preset)
            .map_err(|error| LegacyDayCycleError::WaterNotConverted {
                name: LEGACY_DAY_WATER_PRESET.to_owned(),
                error,
            })?,
        None => WaterSettings::legacy_default(&water_key),
    };

    Ok(EnvironmentAsset::DayCycle(Box::new(DayCycle {
        name: name.to_owned(),
        water_track: vec![DayCycleFrame {
            keyframe: 0.0,
            name: water_key.clone(),
        }],
        sky_tracks: vec![sky_track],
        sky_frames,
        water_frames: core::iter::once((water_key, water)).collect(),
    })))
}

/// Parse and convert one sibling preset's bytes with `convert`, which is either
/// of the two per-kind converters above.
///
/// Both take an already-parsed map, so this is the `read_legacy_preset_data`
/// half they share, named once rather than written out twice.
fn convert_sibling<T>(
    name: &str,
    bytes: &[u8],
    convert: impl Fn(&str, &Llsd) -> Result<T, LegacyPresetError>,
) -> Result<T, LegacyPresetError> {
    let preset = preset_map_from_bytes(bytes)?;
    convert(name, &preset)
}

/// The `(keyframe, sky-preset-name)` pairs a legacy day cycle file spells, in
/// keyframe order.
///
/// The keyframe is clamped into `0.0..=1.0`, which is where the rest of this
/// workspace's day-cycle code expects a keyframe to be. The order the file
/// happens to be written in is not trusted: every other track in this workspace
/// is kept sorted (see `DayCycle::insert_at`), and a legacy file is just a list.
///
/// # Errors
///
/// [`LegacyDayCycleError`] when the document is not an array of
/// `[real, string]` pairs, or is an empty one.
fn legacy_day_keyframes(bytes: &[u8]) -> Result<Vec<(f32, String)>, LegacyDayCycleError> {
    let text = std::str::from_utf8(bytes).map_err(|_err| LegacyDayCycleError::NotLlsd)?;
    let document = parse_llsd_xml(text).map_err(|_err| LegacyDayCycleError::NotLlsd)?;
    let entries = document
        .as_array()
        .ok_or(LegacyDayCycleError::NotADayCycle)?;
    let mut keyframes: Vec<(f32, String)> = Vec::with_capacity(entries.len());
    for entry in entries {
        // A malformed entry fails the file rather than being skipped: the
        // reference reads the two slots unconditionally and gets `0.0` and an
        // empty name out of a wrong shape, which then fails to load a sky
        // called "". Saying so up front is the same answer, earlier.
        let pair = entry.as_array().ok_or(LegacyDayCycleError::NotADayCycle)?;
        let at = pair
            .first()
            .and_then(Llsd::as_f32)
            .ok_or(LegacyDayCycleError::NotADayCycle)?;
        let preset = pair
            .get(1)
            .and_then(Llsd::as_str)
            .ok_or(LegacyDayCycleError::NotADayCycle)?;
        keyframes.push((at.clamp(0.0, 1.0), preset.to_owned()));
    }
    if keyframes.is_empty() {
        return Err(LegacyDayCycleError::NoKeyframes);
    }
    keyframes.sort_by(|(left, _lname), (right, _rname)| left.total_cmp(right));
    Ok(keyframes)
}

/// The preset name a legacy file's stem stands for: its **percent-unescaped**
/// form, so `%28SS%29%20Atmos%2000%3A00%202` is `(SS) Atmos 00:00 2`.
///
/// The old viewer wrote a preset's name straight into a filename and escaped
/// whatever a filesystem would object to, which is why a WindLight folder is
/// full of `%20`; the reference undoes it with `LLURI::unescape` before handing
/// the name to the converter. Decoding is done on **bytes** and the result read
/// back as UTF-8, because a non-ASCII preset name is a multi-byte sequence
/// escaped one byte at a time (`%C3%A9` is one `é`, not two characters). A `%`
/// that is not followed by two hex digits is left alone, as a name that
/// legitimately contains one must be.
#[must_use]
pub fn legacy_preset_name(file_stem: &str) -> String {
    let bytes = file_stem.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while let Some(&byte) = bytes.get(index) {
        let pair = (byte == b'%')
            .then(|| {
                let high = bytes.get(index.saturating_add(1))?;
                let low = bytes.get(index.saturating_add(2))?;
                hex_pair(*high, *low)
            })
            .flatten();
        match pair {
            Some(value) => {
                decoded.push(value);
                index = index.saturating_add(3);
            }
            None => {
                decoded.push(byte);
                index = index.saturating_add(1);
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// The byte two ASCII hex digits spell, or `None` if either is not one.
fn hex_pair(high: u8, low: u8) -> Option<u8> {
    let digit = |byte: u8| char::from(byte).to_digit(16);
    let high = digit(high)?;
    let low = digit(low)?;
    u8::try_from(high.saturating_mul(16).saturating_add(low)).ok()
}

/// Read a legacy scalar setting, accepting both shapes WindLight presets are
/// written in: the four-real array the stock editor wrote (of which only the
/// first element is the value), and the bare real some third-party editors
/// wrote. The reference only reads the first shape for the sky's scalars and
/// only the second for water's, which is a distinction the files themselves do
/// not honour.
fn legacy_scalar(value: Option<&Llsd>) -> Option<f32> {
    let value = value?;
    match value.as_array() {
        Some(array) => array.first().and_then(Llsd::as_f32),
        None => value.as_f32(),
    }
}

/// Take a legacy scalar into `slot` if the preset carries `key`, reporting
/// whether it did — one arm of the reference's long `if (legacy.has(…))` chain.
fn legacy_real(preset: &Llsd, key: &str, slot: &mut f32) -> bool {
    match legacy_scalar(preset.get(key)) {
        Some(value) => {
            *slot = value;
            true
        }
        None => false,
    }
}

/// Take a legacy RGB colour into `slot` if the preset carries `key`, reporting
/// whether it did. WindLight wrote four components where EEP keeps three; the
/// fourth was the editor's own slider bookkeeping and the reference drops it
/// (`LLColor3(legacy[key])`).
fn legacy_color(preset: &Llsd, key: &str, slot: &mut Color) -> bool {
    match preset.get(key) {
        Some(value) => {
            *slot = color_from_llsd(Some(value));
            true
        }
        None => false,
    }
}

/// The cloud scroll rate a legacy preset spells, un-biased and gated.
///
/// WindLight stored the two rates offset by ten so the editor's slider could run
/// from zero, and kept a separate `enable_cloud_scroll` pair of booleans that
/// zeroed an axis without losing its stored rate. EEP has neither, so the bias
/// comes off and a disabled axis becomes a literal zero — which is the
/// reference's own conversion, and means an imported preset with scrolling
/// switched off can no longer be switched back on to the rate it remembered.
fn legacy_cloud_scroll_rate(preset: &Llsd, value: &Llsd) -> [f32; 2] {
    let [x, y] = vec2_from_llsd(Some(value));
    let enabled = preset.get("enable_cloud_scroll");
    let axis_enabled = |index: usize| {
        enabled
            .and_then(|flags| flags.index(index))
            .and_then(Llsd::as_bool)
            .unwrap_or(true)
    };
    [
        if axis_enabled(0) { x - 10.0 } else { 0.0 },
        if axis_enabled(1) { y - 10.0 } else { 0.0 },
    ]
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::{
        LEGACY_DAY_WATER_PRESET, LegacyDayCycleError, LegacyPresetError,
        legacy_day_cycle_from_bytes, legacy_preset_from_bytes, legacy_preset_name,
    };
    use crate::{
        DayCycle, DayCycleFrame, DayTrack, EnvironmentAsset, SettingsKind, SkySettings,
        WaterSettings,
    };

    /// The stock `Default.xml` sky the pre-EEP viewer shipped, verbatim — the
    /// shape every WindLight sky preset in the wild is written in (scalars as
    /// four-real arrays, the sun as two Euler angles, the scroll rate biased by
    /// ten).
    const DEFAULT_SKY: &str = r"<llsd>
    <map>
    <key>ambient</key>
        <array>
            <real>1.0499999523162842</real>
            <real>1.0499999523162842</real>
            <real>1.0499999523162842</real>
            <real>0.34999999403953552</real>
        </array>
    <key>blue_density</key>
        <array>
            <real>0.24475815892219543</real>
            <real>0.44872328639030457</real>
            <real>0.75999999046325684</real>
            <real>0.37999999523162842</real>
        </array>
    <key>blue_horizon</key>
        <array>
            <real>0.49548381567001343</real>
            <real>0.49548381567001343</real>
            <real>0.63999998569488525</real>
            <real>0.31999999284744263</real>
        </array>
    <key>cloud_color</key>
        <array>
            <real>0.40999999642372131</real>
            <real>0.40999999642372131</real>
            <real>0.40999999642372131</real>
            <real>0.40999999642372131</real>
        </array>
    <key>cloud_pos_density1</key>
        <array>
            <real>1.6884100437164307</real>
            <real>0.52609699964523315</real>
            <real>1</real>
            <real>1</real>
        </array>
    <key>cloud_pos_density2</key>
        <array>
            <real>1.6884100437164307</real>
            <real>0.52609699964523315</real>
            <real>0.125</real>
            <real>1</real>
        </array>
    <key>cloud_scale</key>
        <array>
            <real>0.41999998688697815</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>cloud_scroll_rate</key>
        <array>
            <real>10.199999809265137</real>
            <real>10.01099967956543</real>
        </array>
    <key>cloud_shadow</key>
        <array>
            <real>0.26999998092651367</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>density_multiplier</key>
        <array>
            <real>0.00017999998817685992</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>distance_multiplier</key>
        <array>
            <real>0.80000001192092896</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>east_angle</key>
        <real>0</real>
    <key>enable_cloud_scroll</key>
        <array>
            <boolean>1</boolean>
            <boolean>1</boolean>
        </array>
    <key>gamma</key>
        <array>
            <real>1</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>glow</key>
        <array>
            <real>5</real>
            <real>0.0010000000474974513</real>
            <real>-0.47999998927116394</real>
            <real>1</real>
        </array>
    <key>haze_density</key>
        <array>
            <real>0.69999998807907104</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>haze_horizon</key>
        <array>
            <real>0.18999999761581421</real>
            <real>0.19915600121021271</real>
            <real>0.19915600121021271</real>
            <real>1</real>
        </array>
    <key>lightnorm</key>
        <array>
            <real>0</real>
            <real>0.91269159317016602</real>
            <real>-0.40864911675453186</real>
            <real>0</real>
        </array>
    <key>max_y</key>
        <array>
            <real>1605</real>
            <real>0</real>
            <real>0</real>
            <real>1</real>
        </array>
    <key>preset_num</key>
        <integer>22</integer>
    <key>star_brightness</key>
        <real>0</real>
    <key>sun_angle</key>
        <real>1.9917697906494141</real>
    <key>sunlight_color</key>
        <array>
            <real>0.7342105507850647</real>
            <real>0.78157895803451538</real>
            <real>0.89999997615814209</real>
            <real>0.29999998211860657</real>
        </array>
    </map>
</llsd>
";

    /// The stock `Default.xml` water preset the pre-EEP viewer shipped,
    /// verbatim: camel-case keys, bare reals rather than arrays, and a normal
    /// map named outright.
    const DEFAULT_WATER: &str = r"<llsd>
    <map>
    <key>blurMultiplier</key>
        <real>0.040000002831220627</real>
    <key>fresnelOffset</key>
        <real>0.5</real>
    <key>fresnelScale</key>
        <real>0.39999997615814209</real>
    <key>normScale</key>
        <array>
            <integer>2</integer>
            <integer>2</integer>
            <integer>2</integer>
        </array>
    <key>normalMap</key>
        <uuid>822ded49-9a6c-f61c-cb89-6df54f42cdf4</uuid>
    <key>scaleAbove</key>
        <real>0.029999999329447746</real>
    <key>scaleBelow</key>
        <real>0.20000000298023224</real>
    <key>underWaterFogMod</key>
        <real>0.25</real>
    <key>waterFogColor</key>
        <array>
            <real>0.015686275437474251</real>
            <real>0.14901961386203766</real>
            <real>0.25098040699958801</real>
            <real>1</real>
        </array>
    <key>waterFogDensity</key>
        <real>16</real>
    <key>wave1Dir</key>
        <array>
            <real>1.0499997138977051</real>
            <real>-0.42000007629394531</real>
        </array>
    <key>wave2Dir</key>
        <array>
            <real>1.1099996566772461</real>
            <real>-1.1600000858306885</real>
        </array>
    </map>
</llsd>
";

    /// A boxed error, so a test can `?` rather than reach for the `panic!` the
    /// workspace's lints (rightly) forbid.
    type TestError = Box<dyn core::error::Error>;

    /// Whether two floats agree closely enough that a failure would be about the
    /// **conversion** rather than about the last bit of a decimal literal. A
    /// legacy preset is decimal text all the way down, so pinning an exact
    /// `f32` would be pinning the text-to-float round trip, not this module.
    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 1.0e-5
    }

    /// Import the fixture sky, or fail the test saying what came back instead.
    fn imported_sky(name: &str, xml: &str) -> Result<Box<SkySettings>, TestError> {
        match legacy_preset_from_bytes(SettingsKind::Sky, name, xml.as_bytes()) {
            Ok(EnvironmentAsset::Sky(sky)) => Ok(sky),
            other => Err(format!("expected a sky, got {other:?}").into()),
        }
    }

    /// Import the fixture water preset, or fail the test saying what came back
    /// instead.
    fn imported_water(name: &str, xml: &str) -> Result<WaterSettings, TestError> {
        match legacy_preset_from_bytes(SettingsKind::Water, name, xml.as_bytes()) {
            Ok(EnvironmentAsset::Water(water)) => Ok(water),
            other => Err(format!("expected water, got {other:?}").into()),
        }
    }

    /// Every value the stock WindLight sky carries reaches the EEP frame, in the
    /// EEP units: the scalars out of their arrays, the colours down to three
    /// channels, the scroll rate un-biased, and star brightness rescaled.
    #[test]
    fn a_stock_sky_preset_converts_field_by_field() -> Result<(), TestError> {
        let sky = imported_sky("Default", DEFAULT_SKY)?;
        assert_eq!(sky.name, "Default", "the caller's name tags the frame");
        // The haze block.
        for (what, actual, expected) in [
            ("ambient r", sky.ambient.red(), 1.05),
            ("blue density b", sky.blue_density.blue(), 0.76),
            ("blue horizon g", sky.blue_horizon.green(), 0.495_484),
            ("density multiplier", sky.density_multiplier, 0.000_18),
            ("distance multiplier", sky.distance_multiplier, 0.8),
            ("haze density", sky.haze_density, 0.7),
            ("haze horizon", sky.haze_horizon, 0.19),
            // The clouds. The scroll rate is the biased pair minus ten.
            ("cloud colour r", sky.cloud_color.red(), 0.41),
            ("cloud density 1", sky.cloud_pos_density1.density(), 1.0),
            ("cloud density 2", sky.cloud_pos_density2.density(), 0.125),
            ("cloud scale", sky.cloud_scale, 0.42),
            ("cloud shadow", sky.cloud_shadow, 0.27),
            ("scroll x, un-biased", sky.cloud_scroll_rate[0], 0.2),
            ("scroll y, un-biased", sky.cloud_scroll_rate[1], 0.011),
            // The rest.
            ("gamma", sky.gamma, 1.0),
            ("glow size", sky.glow.size(), 5.0),
            ("glow focus", sky.glow.focus(), -0.48),
            ("max y", sky.max_y, 1605.0),
            // The stock preset's own value is zero, and zero times 250 is
            // still zero — the rescaling itself is pinned separately, below.
            ("star brightness", sky.star_brightness, 0.0),
            ("sunlight alpha", sky.sunlight_color.alpha(), 0.3),
        ] {
            assert!(
                close(actual, expected),
                "{what}: {actual} is not {expected}"
            );
        }
        Ok(())
    }

    /// A setting the old format never had keeps the reference's default rather
    /// than a zero — the whole reason the conversion starts from `defaults()`.
    #[test]
    fn an_absent_setting_keeps_the_reference_default() -> Result<(), TestError> {
        let sky = imported_sky("Default", DEFAULT_SKY)?;
        let default = SkySettings::legacy_windlight_default("Default");
        assert!(
            close(sky.moisture_level, default.moisture_level),
            "an EEP-only scalar the preset has no key for"
        );
        assert_eq!(
            sky.rayleigh_config, default.rayleigh_config,
            "the density profiles a WindLight preset never carried"
        );
        assert!(
            close(sky.cloud_variance, default.cloud_variance),
            "cloud variance arrived with EEP"
        );
        assert!(
            close(sky.reflection_probe_ambiance, 0.0),
            "a legacy sky is a classic-mode sky"
        );
        Ok(())
    }

    /// The two Euler angles become the sun's rotation, and the moon lands
    /// opposite it — the old format stored no moon of its own.
    #[test]
    fn the_sun_angles_place_the_sun_and_the_moon_opposite_it() -> Result<(), TestError> {
        let sky = imported_sky("Default", DEFAULT_SKY)?;
        let (sun_azimuth, sun_altitude) = crate::rotation_to_azimuth_altitude(&sky.sun_rotation);
        let (moon_azimuth, moon_altitude) = crate::rotation_to_azimuth_altitude(&sky.moon_rotation);
        // `east_angle` is 0 and `sun_angle` 1.99177 radians — past the zenith,
        // so the round trip comes back as the same direction spelled from the
        // other side: azimuth pi, altitude pi - 1.99177.
        let flipped_altitude = std::f32::consts::PI - 1.991_769_8;
        assert!(
            (sun_altitude - flipped_altitude).abs() < 1.0e-4,
            "the sun keeps its altitude: {sun_altitude}"
        );
        assert!(
            (moon_altitude + flipped_altitude).abs() < 1.0e-4,
            "the moon is as far below as the sun is above: {moon_altitude}"
        );
        let azimuth_gap = (sun_azimuth - moon_azimuth).abs();
        assert!(
            (azimuth_gap - std::f32::consts::PI).abs() < 1.0e-4,
            "the moon is half a turn away: {sun_azimuth} vs {moon_azimuth}"
        );
        Ok(())
    }

    /// An axis whose scroll was switched off converts to a literal zero rather
    /// than to its remembered rate, as the reference does.
    #[test]
    fn a_disabled_scroll_axis_converts_to_zero() -> Result<(), TestError> {
        let disabled = DEFAULT_SKY.replace(
            "<boolean>1</boolean>\n            <boolean>1</boolean>",
            "<boolean>0</boolean>\n            <boolean>1</boolean>",
        );
        assert_ne!(disabled, DEFAULT_SKY, "the fixture edit has to bite");
        let sky = imported_sky("Default", &disabled)?;
        let [scroll_x, scroll_y] = sky.cloud_scroll_rate;
        assert!(
            close(scroll_x, 0.0),
            "the disabled axis is zero, not 0.2: {scroll_x}"
        );
        assert!(
            close(scroll_y, 0.011),
            "the other axis is untouched: {scroll_y}"
        );
        Ok(())
    }

    /// The stock water preset converts, including the normal map it names and
    /// the fog density the two formats disagree about the default of.
    #[test]
    fn a_stock_water_preset_converts_field_by_field() -> Result<(), TestError> {
        let water = imported_water("Default", DEFAULT_WATER)?;
        assert_eq!(water.name, "Default", "the caller's name tags the frame");
        assert_eq!(
            water.normal_map.map(|key| key.uuid().to_string()),
            Some("822ded49-9a6c-f61c-cb89-6df54f42cdf4".to_owned()),
            "the normal map the preset names"
        );
        for (what, actual, expected) in [
            ("blur multiplier", water.blur_multiplier, 0.04),
            ("fresnel offset", water.fresnel_offset, 0.5),
            ("fresnel scale", water.fresnel_scale, 0.4),
            ("normal scale from integers", water.normal_scale.x(), 2.0),
            ("scale above", water.scale_above, 0.03),
            ("scale below", water.scale_below, 0.2),
            ("underwater fog mod", water.underwater_fog_mod, 0.25),
            ("fog colour b", water.water_fog_color.blue(), 0.250_98),
            // Taken verbatim: the legacy default is 16 where the EEP default is
            // 2, and the reference copies the number rather than rescaling it.
            ("fog density", water.water_fog_density, 16.0),
            ("wave 1 x", water.wave1_direction[0], 1.05),
            ("wave 1 y", water.wave1_direction[1], -0.42),
        ] {
            assert!(
                close(actual, expected),
                "{what}: {actual} is not {expected}"
            );
        }
        Ok(())
    }

    /// A water preset picked in the sky editor is caught by holding none of the
    /// sky's keys, rather than importing as a sky of pure defaults.
    #[test]
    fn the_wrong_kind_of_preset_is_refused() {
        assert_eq!(
            legacy_preset_from_bytes(SettingsKind::Sky, "Default", DEFAULT_WATER.as_bytes()),
            Err(LegacyPresetError::NothingConverted),
            "water read as a sky"
        );
        assert_eq!(
            legacy_preset_from_bytes(SettingsKind::Water, "Default", DEFAULT_SKY.as_bytes()),
            Err(LegacyPresetError::NothingConverted),
            "a sky read as water"
        );
    }

    /// Everything that is not a preset is refused with a reason, and a day cycle
    /// is refused even though its file parses.
    #[test]
    fn a_non_preset_is_refused_with_a_reason() {
        assert_eq!(
            legacy_preset_from_bytes(SettingsKind::Sky, "x", b"not xml at all"),
            Err(LegacyPresetError::NotLlsd),
            "not XML"
        );
        assert_eq!(
            legacy_preset_from_bytes(SettingsKind::Sky, "x", b"<llsd><array /></llsd>"),
            Err(LegacyPresetError::NotAMap),
            "XML, but not a settings map"
        );
        assert_eq!(
            legacy_preset_from_bytes(SettingsKind::DayCycle, "x", DEFAULT_SKY.as_bytes()),
            Err(LegacyPresetError::DayCycleUnsupported),
            "a day cycle needs its sibling files"
        );
    }

    /// A scalar written as a bare real rather than the stock four-real array is
    /// read as the value it plainly is.
    #[test]
    fn a_scalar_written_bare_is_still_read() -> Result<(), TestError> {
        let bare = DEFAULT_SKY.replace(
            "<key>cloud_scale</key>\n        <array>\n            \
             <real>0.41999998688697815</real>\n            <real>0</real>\n            \
             <real>0</real>\n            <real>1</real>\n        </array>",
            "<key>cloud_scale</key>\n        <real>0.41999998688697815</real>",
        );
        assert_ne!(bare, DEFAULT_SKY, "the fixture edit has to bite");
        let cloud_scale = imported_sky("Default", &bare)?.cloud_scale;
        assert!(
            close(cloud_scale, 0.42),
            "a bare real is the value, not a zero: {cloud_scale}"
        );
        Ok(())
    }

    /// Star brightness is the one **rescaled** setting: WindLight's `0..=2`
    /// slider became EEP's `0..=500`, so the reference multiplies by 250 coming
    /// in. The stock preset's own value is zero, which any scaling leaves at
    /// zero, so it takes a preset with a non-zero one to see it.
    #[test]
    fn star_brightness_is_rescaled_by_250() -> Result<(), TestError> {
        let bright = DEFAULT_SKY.replace(
            "<key>star_brightness</key>\n        <real>0</real>",
            "<key>star_brightness</key>\n        <real>1.5</real>",
        );
        assert_ne!(bright, DEFAULT_SKY, "the fixture edit has to bite");
        let brightness = imported_sky("Default", &bright)?.star_brightness;
        assert!(
            close(brightness, 375.0),
            "1.5 on the old slider is 375 on the new one: {brightness}"
        );
        Ok(())
    }

    /// A preset's name comes back out of its escaped filename, multi-byte
    /// characters included, and a lone `%` survives.
    #[test]
    fn an_escaped_file_stem_unescapes_to_the_preset_name() {
        assert_eq!(
            legacy_preset_name("%28SS%29%20Atmos%2000%3A00%202"),
            "(SS) Atmos 00:00 2",
            "the stock escaping"
        );
        assert_eq!(
            legacy_preset_name("Cr%C3%A9puscule"),
            "Crépuscule",
            "a multi-byte character is escaped one byte at a time"
        );
        assert_eq!(
            legacy_preset_name("100%"),
            "100%",
            "a trailing percent is not an escape"
        );
        assert_eq!(
            legacy_preset_name("50%zz off"),
            "50%zz off",
            "a percent with no hex pair after it stands"
        );
    }

    // -----------------------------------------------------------------------
    // The day cycle.
    // -----------------------------------------------------------------------

    /// A stock legacy day cycle, in the shape the shipped `days/Default.xml` is
    /// written in: an array of `[keyframe, sky-preset-name]` pairs, with the
    /// names spelled **unescaped** (the escaping belongs to the filename the
    /// sky is stored under, not to the reference from inside a day file).
    ///
    /// Deliberately written out of keyframe order, and naming one preset twice,
    /// so the sorting and the frame sharing are exercised by the fixture rather
    /// than by a special case.
    const DEFAULT_DAY: &str = r"<llsd>
    <array>
        <array>
            <real>0.5</real>
            <string>(SS) Noon</string>
        </array>
        <array>
            <real>0</real>
            <string>A-12AM</string>
        </array>
        <array>
            <real>0.75</real>
            <string>A-12AM</string>
        </array>
    </array>
</llsd>
";

    /// Convert the fixture day cycle, resolving sibling presets out of `files`
    /// (a `(kind, name) -> xml` lookup), or fail the test saying what came back
    /// instead.
    fn imported_day(
        name: &str,
        xml: &str,
        files: &[(SettingsKind, &str, &str)],
    ) -> Result<Box<DayCycle>, TestError> {
        let converted = legacy_day_cycle_from_bytes(name, xml.as_bytes(), |kind, wanted| {
            files
                .iter()
                .find(|(file_kind, file_name, _xml)| *file_kind == kind && *file_name == wanted)
                .map(|(_kind, _name, xml)| xml.as_bytes().to_vec())
        });
        match converted {
            Ok(EnvironmentAsset::DayCycle(cycle)) => Ok(cycle),
            other => Err(format!("expected a day cycle, got {other:?}").into()),
        }
    }

    /// The sibling files the fixture day cycle needs, all present.
    fn stock_siblings() -> Vec<(SettingsKind, &'static str, &'static str)> {
        vec![
            (SettingsKind::Sky, "(SS) Noon", DEFAULT_SKY),
            (SettingsKind::Sky, "A-12AM", DEFAULT_SKY),
            (SettingsKind::Water, LEGACY_DAY_WATER_PRESET, DEFAULT_WATER),
        ]
    }

    /// A whole legacy day cycle converts: its keyframes land on the surface sky
    /// track in keyframe order, each naming a `sky:`-prefixed frame that is
    /// really there, a preset named twice is stored once and shared, and the
    /// sibling water preset becomes the single water keyframe.
    #[test]
    fn a_legacy_day_cycle_converts_into_tracks_and_frames() -> Result<(), TestError> {
        let cycle = imported_day("Stock day", DEFAULT_DAY, &stock_siblings())?;
        assert_eq!(cycle.name, "Stock day", "the caller's name tags the cycle");
        let track = cycle.track(DayTrack::GROUND);
        assert_eq!(
            track
                .iter()
                .map(|frame| (frame.keyframe, frame.name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0.0, "sky:A-12AM"),
                (0.5, "sky:(SS) Noon"),
                (0.75, "sky:A-12AM"),
            ],
            "sorted by keyframe, each naming its prefixed frame"
        );
        assert_eq!(
            cycle.sky_frames.keys().collect::<Vec<_>>(),
            vec!["sky:(SS) Noon", "sky:A-12AM"],
            "a preset named twice is one frame, referenced twice"
        );
        assert_eq!(
            cycle.water_track,
            vec![DayCycleFrame {
                keyframe: 0.0,
                name: "water:Default".to_owned(),
            }],
            "the old format has no water schedule: one frame, all day"
        );
        let water = cycle
            .water_frames
            .get("water:Default")
            .ok_or("the water track's frame is there")?;
        assert!(
            close(water.water_fog_density, 16.0),
            "and it is the *converted* preset, not the default: {}",
            water.water_fog_density
        );
        Ok(())
    }

    /// Each frame is named for the key it is filed under, so a converted cycle
    /// reads back from its own asset bytes exactly as it was built — the map
    /// key is what `day_cycle_to_llsd` writes and what the decoder names a
    /// frame by.
    #[test]
    fn every_frame_is_named_for_its_key() -> Result<(), TestError> {
        let cycle = imported_day("Stock day", DEFAULT_DAY, &stock_siblings())?;
        for (key, sky) in &cycle.sky_frames {
            assert_eq!(&sky.name, key, "sky frame named for its key");
        }
        for (key, water) in &cycle.water_frames {
            assert_eq!(&water.name, key, "water frame named for its key");
        }
        Ok(())
    }

    /// A missing *water* preset is not a failure — the reference falls back to
    /// the `water/` folder the viewer ships, and this workspace ships none, so
    /// the fallback is the reference's own `defaults()`.
    #[test]
    fn a_missing_water_preset_falls_back_to_the_default() -> Result<(), TestError> {
        let siblings: Vec<_> = stock_siblings()
            .into_iter()
            .filter(|(kind, _name, _xml)| *kind != SettingsKind::Water)
            .collect();
        let cycle = imported_day("No water", DEFAULT_DAY, &siblings)?;
        let water = cycle
            .water_frames
            .get("water:Default")
            .ok_or("a water frame is there regardless")?;
        assert!(
            close(water.water_fog_density, 2.0),
            "the EEP default fog density, not the WindLight preset's: {}",
            water.water_fog_density
        );
        Ok(())
    }

    /// A missing *sky* preset fails the whole cycle, naming the one it could not
    /// find: a day cycle two thirds of whose skies are the default is not the
    /// day cycle its author wrote.
    #[test]
    fn a_missing_sky_preset_fails_the_cycle() {
        let siblings = [
            (SettingsKind::Sky, "A-12AM", DEFAULT_SKY),
            (SettingsKind::Water, LEGACY_DAY_WATER_PRESET, DEFAULT_WATER),
        ];
        let converted =
            legacy_day_cycle_from_bytes("Stock day", DEFAULT_DAY.as_bytes(), |kind, wanted| {
                siblings
                    .iter()
                    .find(|(file_kind, name, _xml)| *file_kind == kind && *name == wanted)
                    .map(|(_kind, _name, xml)| xml.as_bytes().to_vec())
            });
        assert_eq!(
            converted.err(),
            Some(LegacyDayCycleError::SkyNotFound("(SS) Noon".to_owned())),
            "and it says which preset was missing"
        );
    }

    /// A sky preset that is *there* but is not a sky fails the cycle with the
    /// underlying conversion's own reason, naming the preset.
    #[test]
    fn a_sibling_that_will_not_convert_fails_the_cycle() {
        let siblings = [
            // Water filed among the skies: it holds none of a sky's keys.
            (SettingsKind::Sky, "(SS) Noon", DEFAULT_WATER),
            (SettingsKind::Sky, "A-12AM", DEFAULT_SKY),
            (SettingsKind::Water, LEGACY_DAY_WATER_PRESET, DEFAULT_WATER),
        ];
        let converted =
            legacy_day_cycle_from_bytes("Stock day", DEFAULT_DAY.as_bytes(), |kind, wanted| {
                siblings
                    .iter()
                    .find(|(file_kind, name, _xml)| *file_kind == kind && *name == wanted)
                    .map(|(_kind, _name, xml)| xml.as_bytes().to_vec())
            });
        assert_eq!(
            converted.err(),
            Some(LegacyDayCycleError::SkyNotConverted {
                name: "(SS) Noon".to_owned(),
                error: LegacyPresetError::NothingConverted,
            }),
            "the sibling's own reason, attributed to the sibling"
        );
    }

    /// Everything that is not a legacy day cycle is refused with a reason — a
    /// sky preset picked in the day importer included, since a settings map is
    /// not a keyframe array.
    #[test]
    fn a_non_day_cycle_is_refused_with_a_reason() {
        let none = |_kind: SettingsKind, _name: &str| None;
        assert_eq!(
            legacy_day_cycle_from_bytes("x", b"not xml at all", none).err(),
            Some(LegacyDayCycleError::NotLlsd),
            "not XML"
        );
        assert_eq!(
            legacy_day_cycle_from_bytes("x", DEFAULT_SKY.as_bytes(), none).err(),
            Some(LegacyDayCycleError::NotADayCycle),
            "a sky preset is a map, not a keyframe array"
        );
        assert_eq!(
            legacy_day_cycle_from_bytes("x", b"<llsd><array /></llsd>", none).err(),
            Some(LegacyDayCycleError::NoKeyframes),
            "an array with no keyframes in it is no day"
        );
        assert_eq!(
            legacy_day_cycle_from_bytes(
                "x",
                b"<llsd><array><string>A-12AM</string></array></llsd>",
                none
            )
            .err(),
            Some(LegacyDayCycleError::NotADayCycle),
            "a keyframe that is not a [real, string] pair"
        );
    }
}
