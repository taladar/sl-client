//! The `@setenv_*` / `@getenv_*` family — a worn object driving the wearer's
//! sky.
//!
//! Like the debug-setting window ([`crate::extension`]) these are **extension
//! commands**: `setenv_ambient` is in no behaviour dictionary, so it reaches the
//! viewer as a [`RlvBehaviour::Unknown`] keyword and is picked up afterwards by
//! a registered handler — `RlvEnvironment` in the reference
//! (`rlvenvironment.cpp`), this module here. The two families are siblings, not
//! layers: neither can shadow the other, because their prefixes differ, and
//! both run only after [`RlvState::apply`] has handed the command back as
//! [`RlvOutcome::NotAStateChange`].
//!
//! What a script may reach is roughly forty **subkeys**, each naming one value
//! of the sky the wearer is looking at: the atmosphere and haze scalars, the
//! cloud layers, the sun and moon (their textures, sizes, brightness and where
//! they sit in the sky), and — through four whole-environment subkeys — a
//! settings asset, a library preset, a day cycle, or a time of day within the
//! current cycle.
//!
//! Despite the family's name it reaches **no water setting at all**: every row
//! the reference registers is a sky row (`registerSkyFn`), and the four
//! whole-environment rows replace the environment rather than edit it. A script
//! that wants the water changed has to hand over a whole settings asset.
//!
//! ## The split with the consumer
//!
//! The same one the debug window draws. Which subkeys exist, what each one is
//! called, what its value is scaled by on the way out and back in, how a colour
//! or a pair is spelled, which of them a script may read and which it may
//! write, and every rule about the legacy per-component spellings — all of that
//! is here and unit-tested. The values themselves come from an
//! [`RlvEnvSource`] the consumer implements over whatever its sky actually is.
//!
//! The consumer therefore never sees a scale factor: it reads and writes its
//! sky in the sky's **own** units, exactly as `LLSettingsSky` stores them, and
//! this module converts. That is where the reference puts the conversion too —
//! in the RLV lambdas, not in the settings object.
//!
//! ## Sun and moon are asked for as directions
//!
//! Six subkeys (`sunazimuth`, `sunelevation`, `moonazimuth`, `moonelevation`,
//! and the two legacy WindLight spellings `eastangle` and `sunmoonposition`)
//! address the sun's or the moon's **rotation**, which the sky stores as a
//! quaternion. This crate has no quaternion type and wants none, so the source
//! hands over the unit **direction** the body sits in — the image of the local
//! `+X` axis under that rotation, which is the whole of what the reference's
//! `convert_azimuth_and_altitude_to_quat` encodes — and takes back an
//! `(azimuth, elevation)` pair to rebuild one from. The spherical maths in
//! between, special cases and all, is the reference's and lives here.
//!
//! ## The reference's own quirks, kept
//!
//! - **the legacy per-component spellings are a fallback, not an alias.** A
//!   subkey that does not resolve is retried with its last character stripped
//!   and read as a colour component (`r`/`x`, `g`/`y`, `b`/`d`, `i`), which
//!   means `@getenv_ambientr` reads the red of the ambient colour — and also
//!   that `@getenv_cloud` is *not* a command while `@getenv_cloudr` is, because
//!   `cloud` only ever existed as a legacy name;
//! - **the `i` component is not a fourth channel.** Reading it gives the
//!   brightest of the three, and writing it scales all three toward the value —
//!   the pre-EEP "intensity" slider, reproduced arithmetic and all;
//! - **`@getenv_daytime` cannot answer the question it was asked.** A time of
//!   day only means anything while a day cycle is running, and then the honest
//!   answer changes every frame; the reference answers `-1` when a cycle is
//!   running and `2` when a fixed sky is pinned — `2` being a value
//!   `@setenv_daytime` itself rejects, so a script can feed the answer straight
//!   back without moving anything;
//! - **an unparsable option is `FAILED_PARAM`, not `FAILED_OPTION`.** Both
//!   `handleSetFn` and `registerSetEnvFn` report a value they could not parse as
//!   the former, and only `@setenv_asset` and `@setenv_daytime` — which parse
//!   fine and then dislike what they got — report the latter.
//!
//! ## Deliberate divergences
//!
//! - **there is one local environment layer here, not two.** The reference
//!   writes into `ENV_LOCAL` normally and into `ENV_EDIT` while an object holds
//!   `@setenv` (`RlvEnvironment::getTargetEnvironment`), then *selects* whichever
//!   it wrote — so the two differ in bookkeeping and not in what the wearer
//!   sees. This crate names one target and lets the consumer keep one layer;
//! - **a read of a sky the viewer does not have yet is a failure, not an empty
//!   success.** The reference always has a sky to read. When the consumer has
//!   none — no region environment ingested yet — the script is still answered,
//!   because one that asked a question must not be left waiting, but the outcome
//!   says the viewer could not do it rather than pretending the sky is all
//!   zeroes.

use uuid::Uuid;

use crate::behaviour::RlvBehaviour;
use crate::command::{RlvCommand, RlvParam};
use crate::extension::parse_prefix;
use crate::query::{RlvReply, is_valid_reply_channel, truncate_chat};
use crate::state::{RlvOutcome, RlvState};

/// The head a read carries (`RLV_GETENV_PREFIX`, `rlvenvironment.cpp:39`),
/// without its trailing underscore — the half of the keyword before the first
/// `_`.
const GETENV_HEAD: &str = "getenv";

/// The head a write carries (`RLV_SETENV_PREFIX`), without its trailing
/// underscore.
const SETENV_HEAD: &str = "setenv";

/// The shortest subkey the reference will look up at all: it requires the
/// keyword to be longer than the prefix plus two, so `@setenv_ab` names nothing
/// while `@setenv_abc` at least reaches the table
/// (`RlvEnvironment::onHandleCommand`).
const MIN_SUBKEY_LEN: usize = 3;

/// `SLIDER_SCALE_BLUE_HORIZON_DENSITY` — the blue horizon and blue density
/// sliders run at half the sky's own values.
const SCALE_BLUE_HORIZON_DENSITY: f32 = 2.0;

/// `SLIDER_SCALE_DENSITY_MULTIPLIER` — the density multiplier is a thousandth
/// of what the slider shows.
const SCALE_DENSITY_MULTIPLIER: f32 = 0.001;

/// `SLIDER_SCALE_GLOW_R` — the divisor behind the sun glow *size*, which is
/// also inverted (a bigger glow is a smaller red channel).
const SCALE_GLOW_R: f32 = 20.0;

/// `SLIDER_SCALE_GLOW_B` — the divisor behind the sun glow *focus*. Negative,
/// so the slider and the stored channel run in opposite directions.
const SCALE_GLOW_B: f32 = -5.0;

/// `SLIDER_SCALE_SUN_AMBIENT` — the ambient and sunlight colours run at a third
/// of the sky's own values.
const SCALE_SUN_AMBIENT: f32 = 3.0;

/// A full turn, the domain the legacy WindLight angles are expressed as a
/// fraction of.
const TWO_PI: f32 = core::f32::consts::TAU;

/// A quarter turn — the elevation limit `@setenv_sunelevation` and
/// `@setenv_moonelevation` clamp to.
const QUARTER_TURN: f32 = core::f32::consts::FRAC_PI_2;

// --- What the consumer supplies --------------------------------------------

/// Which of the two sky bodies a rotation subkey addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RlvSkyBody {
    /// The sun (`SETTING_SUN_ROTATION`).
    Sun,
    /// The moon (`SETTING_MOON_ROTATION`).
    Moon,
}

/// One value of the sky, in the sky's own units.
///
/// The variant a field holds is fixed by [`RlvSkyField::kind`]; a source that
/// answers with another one is treated as having no value for that field, which
/// surfaces as a failed read rather than as a silently coerced number.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RlvSkyValue {
    /// A scalar field.
    Float(f32),
    /// An RGB field, in the reference's `LLColor3` order.
    Color([f32; 3]),
    /// A two-component field (the cloud scroll rate).
    Vec2([f32; 2]),
    /// A texture field. The null UUID means "the viewer's own default", which
    /// is what the reference reads back for a sky that names no texture.
    Texture(Uuid),
}

/// What kind of value a [`RlvSkyField`] holds.
///
/// Exhaustive for the same reason [`RlvSkyField`] is: a consumer answers a read
/// with the variant the kind names, so a kind added later must break it into
/// answering that one too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RlvSkyKind {
    /// A [`RlvSkyValue::Float`] field.
    Float,
    /// A [`RlvSkyValue::Color`] field.
    Color,
    /// A [`RlvSkyValue::Vec2`] field.
    Vec2,
    /// A [`RlvSkyValue::Texture`] field.
    Texture,
}

/// One field of the sky an RLV subkey reads or writes.
///
/// Exhaustive on purpose, like [`RlvDebugSetting`](crate::RlvDebugSetting) and
/// for the same reason: a consumer has to say what every field is, and a field
/// added later should break it into saying so rather than quietly answering
/// nothing. The sun and moon rotations are **not** here — they are asked for as
/// directions instead, see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RlvSkyField {
    /// `SETTING_AMBIENT`.
    Ambient,
    /// `SETTING_BLUE_DENSITY`.
    BlueDensity,
    /// `SETTING_BLUE_HORIZON`.
    BlueHorizon,
    /// `SETTING_CLOUD_COLOR`.
    CloudColor,
    /// `SETTING_CLOUD_POS_DENSITY1` — the first cloud layer's position and
    /// density, stored as a colour.
    CloudPosDensity1,
    /// `SETTING_CLOUD_POS_DENSITY2` — the detail layer, likewise.
    CloudPosDensity2,
    /// `SETTING_SUNLIGHT_COLOR`.
    SunlightColor,
    /// `SETTING_GLOW` — size in the red channel, focus in the blue.
    Glow,
    /// `SETTING_CLOUD_SCROLL_RATE`.
    CloudScrollRate,
    /// `SETTING_DENSITY_MULTIPLIER`.
    DensityMultiplier,
    /// `SETTING_DISTANCE_MULTIPLIER`.
    DistanceMultiplier,
    /// `SETTING_SKY_DROPLET_RADIUS`.
    DropletRadius,
    /// `SETTING_HAZE_DENSITY`.
    HazeDensity,
    /// `SETTING_HAZE_HORIZON`.
    HazeHorizon,
    /// `SETTING_SKY_ICE_LEVEL`.
    IceLevel,
    /// `SETTING_MAX_Y` — the sky dome's maximum altitude.
    MaxY,
    /// `SETTING_SKY_MOISTURE_LEVEL`.
    MoistureLevel,
    /// `SETTING_GAMMA`.
    Gamma,
    /// `SETTING_CLOUD_SHADOW` — cloud coverage.
    CloudShadow,
    /// `SETTING_CLOUD_SCALE`.
    CloudScale,
    /// `SETTING_CLOUD_VARIANCE`.
    CloudVariance,
    /// `SETTING_MOON_BRIGHTNESS`.
    MoonBrightness,
    /// `SETTING_MOON_SCALE`.
    MoonScale,
    /// `SETTING_SUN_SCALE`.
    SunScale,
    /// `SETTING_STAR_BRIGHTNESS`.
    StarBrightness,
    /// `SETTING_CLOUD_TEXTUREID`.
    CloudTexture,
    /// `SETTING_MOON_TEXTUREID`.
    MoonTexture,
    /// `SETTING_SUN_TEXTUREID`.
    SunTexture,
}

impl RlvSkyField {
    /// The kind of value this field holds — which [`RlvSkyValue`] variant a
    /// source must answer with.
    #[must_use]
    pub const fn kind(self) -> RlvSkyKind {
        match self {
            Self::Ambient
            | Self::BlueDensity
            | Self::BlueHorizon
            | Self::CloudColor
            | Self::CloudPosDensity1
            | Self::CloudPosDensity2
            | Self::SunlightColor
            | Self::Glow => RlvSkyKind::Color,
            Self::CloudScrollRate => RlvSkyKind::Vec2,
            Self::CloudTexture | Self::MoonTexture | Self::SunTexture => RlvSkyKind::Texture,
            Self::DensityMultiplier
            | Self::DistanceMultiplier
            | Self::DropletRadius
            | Self::HazeDensity
            | Self::HazeHorizon
            | Self::IceLevel
            | Self::MaxY
            | Self::MoistureLevel
            | Self::Gamma
            | Self::CloudShadow
            | Self::CloudScale
            | Self::CloudVariance
            | Self::MoonBrightness
            | Self::MoonScale
            | Self::SunScale
            | Self::StarBrightness => RlvSkyKind::Float,
        }
    }
}

/// A whole-environment change: the four subkeys that replace the environment
/// rather than edit one of its values.
///
/// Exhaustive on purpose, like [`RlvSkyField`]: a consumer has to say what it
/// does with each of these, and one added later should break it into saying so
/// rather than being quietly dropped.
#[derive(Debug, Clone, PartialEq)]
pub enum RlvEnvRequest {
    /// `@setenv_asset:<uuid>=force` — apply the settings asset with this id.
    Asset(Uuid),
    /// `@setenv_preset:<name-or-uuid>=force` — apply a **sky** from the
    /// library's `Environments` folder, or an asset id spelled out. Deprecated
    /// in the reference, kept because collars still send it.
    Preset(String),
    /// `@setenv_daycycle:<name-or-uuid>=force` — the same for a day cycle.
    DayCycle(String),
    /// `@setenv_daytime:<0..=1>=force` — pin a fixed sky sampled from the
    /// nearest day cycle at this position.
    DayTime(f32),
    /// `@setenv_daytime:-1=force` — drop the local environment and go back to
    /// the shared one.
    Clear,
}

/// What the consumer supplies: the sky behind the subkey table.
///
/// Everything the language decides — which subkeys exist, their scaling, their
/// spelling, who may read or write them — is this crate's. An implementation
/// only has to read and write its own sky.
pub trait RlvEnvSource {
    /// The current value of `field`, or `None` when there is no sky to read.
    ///
    /// The variant must match [`RlvSkyField::kind`]; one that does not is taken
    /// as no value at all.
    fn sky_value(&self, field: RlvSkyField) -> Option<RlvSkyValue>;

    /// Write `value` into `field` of the local sky, answering whether it could
    /// be written.
    ///
    /// The reference clones the sky it is about to edit into the local
    /// environment layer first (`RlvEnvironment::getTargetSky(true)`), so a
    /// write never edits the region's own settings; a consumer is expected to
    /// do the same. `false` means there was no sky to clone.
    fn set_sky_value(&mut self, field: RlvSkyField, value: RlvSkyValue) -> bool;

    /// The unit direction `body` sits in — the image of the local `+X` axis
    /// under its rotation — or `None` when there is no sky to read.
    fn sky_direction(&self, body: RlvSkyBody) -> Option<[f32; 3]>;

    /// Point `body` at the given azimuth and elevation, in radians, answering
    /// whether it could be written.
    ///
    /// The rotation to build from them is the reference's
    /// `convert_azimuth_and_altitude_to_quat`: the one taking `+X` to
    /// `(cos a cos e, sin a cos e, sin e)`.
    fn set_sky_angles(&mut self, body: RlvSkyBody, azimuth: f32, elevation: f32) -> bool;

    /// Apply a whole-environment change, answering whether it could be applied.
    ///
    /// `false` is what the reference reports for a preset name that resolves to
    /// nothing (`RLV_RET_FAILED_OPTION`).
    fn apply_environment(&mut self, request: &RlvEnvRequest) -> bool;

    /// Whether a **fixed** sky is in force rather than a running day cycle —
    /// the one fact `@getenv_daytime` reports.
    fn has_fixed_sky(&self) -> bool;
}

// --- The subkey table ------------------------------------------------------

/// What a subkey's value is spelled as, both on the way in and on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvEnvKind {
    /// A single number (`std::stof` in, `%f` out).
    Float,
    /// Three numbers separated by `/` (`LLColor3`).
    Color,
    /// Two numbers separated by `/` (`LLVector2`).
    Vec2,
    /// A UUID in full hyphenated form.
    Uuid,
    /// Free text — a preset or day-cycle name, which may also be an asset id.
    Text,
}

/// One subkey of the `@setenv_*` / `@getenv_*` family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RlvEnvSetting {
    /// `ambient` — the ambient light colour, at a third of the sky's value.
    Ambient,
    /// `bluedensity` — at half the sky's value.
    BlueDensity,
    /// `bluehorizon` — at half the sky's value.
    BlueHorizon,
    /// `densitymultiplier` — at a thousand times the sky's value.
    DensityMultiplier,
    /// `distancemultiplier`.
    DistanceMultiplier,
    /// `dropletradius`.
    DropletRadius,
    /// `hazedensity`.
    HazeDensity,
    /// `hazehorizon`.
    HazeHorizon,
    /// `icelevel`.
    IceLevel,
    /// `maxaltitude` — the sky dome's `max_y`.
    MaxAltitude,
    /// `moisturelevel`.
    MoistureLevel,
    /// `scenegamma` — the sky's gamma.
    SceneGamma,
    /// `cloudcolor`.
    CloudColor,
    /// `cloudcoverage` — the sky's cloud *shadow*.
    CloudCoverage,
    /// `clouddensity`, spelled `cloud` in the legacy per-component form.
    CloudDensity,
    /// `clouddetail`.
    CloudDetail,
    /// `cloudscale`.
    CloudScale,
    /// `cloudscroll`.
    CloudScroll,
    /// `cloudtexture`.
    CloudTexture,
    /// `cloudvariance`.
    CloudVariance,
    /// `moonbrightness`.
    MoonBrightness,
    /// `moonscale`.
    MoonScale,
    /// `moontexture`.
    MoonTexture,
    /// `sunglowsize` — the glow's red channel, inverted and scaled.
    SunGlowSize,
    /// `sunglowfocus` — the glow's blue channel, scaled by a negative.
    SunGlowFocus,
    /// `sunlightcolor`, spelled `sunmooncolor` in the legacy per-component
    /// form.
    SunlightColor,
    /// `sunscale`.
    SunScale,
    /// `suntexture`.
    SunTexture,
    /// `starbrightness`.
    StarBrightness,
    /// `sunazimuth` — where the sun sits around the horizon, in radians.
    SunAzimuth,
    /// `sunelevation` — how high the sun sits, in radians, clamped to a quarter
    /// turn either way on a write.
    SunElevation,
    /// `moonazimuth`.
    MoonAzimuth,
    /// `moonelevation`.
    MoonElevation,
    /// `eastangle` — the legacy WindLight azimuth: the opposite direction of
    /// travel, as a fraction of a full turn. Writing it moves the moon too.
    EastAngle,
    /// `sunmoonposition` — the legacy WindLight elevation, as a fraction of a
    /// full turn. Writing it moves the moon too.
    SunMoonPosition,
    /// `asset` — apply a settings asset by id.
    Asset,
    /// `preset` — apply a library sky by name or id (deprecated).
    Preset,
    /// `daycycle` — apply a library day cycle by name or id (deprecated).
    DayCycleName,
    /// `daytime` — pin a fixed sky from the running cycle, or drop the local
    /// environment with `-1`.
    DayTime,
}

/// One row of the subkey table.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RlvEnvSettingDef {
    /// Which subkey this row is.
    pub setting: RlvEnvSetting,
    /// The subkey a script writes after `@getenv_` / `@setenv_`. Already
    /// lower-cased, as the whole command line is.
    pub name: &'static str,
    /// The name this row also answers to when a legacy per-component suffix is
    /// stripped off, or `None` when it has no legacy form.
    ///
    /// Usually the same as [`name`](Self::name); `clouddensity` and
    /// `sunlightcolor` are the two that differ, and their legacy names —
    /// `cloud` and `sunmooncolor` — are **only** reachable with a component
    /// suffix, never on their own.
    pub legacy_name: Option<&'static str>,
    /// How its value is spelled.
    pub kind: RlvEnvKind,
    /// Whether `@getenv_<name>=<channel>` may read it.
    pub readable: bool,
    /// Whether `@setenv_<name>:<value>=force` may write it.
    pub writable: bool,
}

/// The subkey table, in the reference's own registration order
/// (`RlvEnvironment::RlvEnvironment`).
///
/// This is the definition: nothing else lists the subkeys, and the four
/// reference lookup maps (get, set, and a legacy pair) are the four flag
/// combinations of a row.
pub const RLV_ENV_SETTINGS: &[RlvEnvSettingDef] = &[
    write_only(RlvEnvSetting::Asset, "asset", RlvEnvKind::Uuid),
    write_only(RlvEnvSetting::Preset, "preset", RlvEnvKind::Text),
    write_only(RlvEnvSetting::DayCycleName, "daycycle", RlvEnvKind::Text),
    legacy_row(RlvEnvSetting::Ambient, "ambient", "ambient"),
    legacy_row(RlvEnvSetting::BlueDensity, "bluedensity", "bluedensity"),
    legacy_row(RlvEnvSetting::BlueHorizon, "bluehorizon", "bluehorizon"),
    float_row(RlvEnvSetting::DensityMultiplier, "densitymultiplier"),
    float_row(RlvEnvSetting::DistanceMultiplier, "distancemultiplier"),
    float_row(RlvEnvSetting::DropletRadius, "dropletradius"),
    float_row(RlvEnvSetting::HazeDensity, "hazedensity"),
    float_row(RlvEnvSetting::HazeHorizon, "hazehorizon"),
    float_row(RlvEnvSetting::IceLevel, "icelevel"),
    float_row(RlvEnvSetting::MaxAltitude, "maxaltitude"),
    float_row(RlvEnvSetting::MoistureLevel, "moisturelevel"),
    float_row(RlvEnvSetting::SceneGamma, "scenegamma"),
    legacy_row(RlvEnvSetting::CloudColor, "cloudcolor", "cloudcolor"),
    float_row(RlvEnvSetting::CloudCoverage, "cloudcoverage"),
    legacy_row(RlvEnvSetting::CloudDensity, "clouddensity", "cloud"),
    legacy_row(RlvEnvSetting::CloudDetail, "clouddetail", "clouddetail"),
    float_row(RlvEnvSetting::CloudScale, "cloudscale"),
    RlvEnvSettingDef {
        setting: RlvEnvSetting::CloudScroll,
        name: "cloudscroll",
        legacy_name: Some("cloudscroll"),
        kind: RlvEnvKind::Vec2,
        readable: true,
        writable: true,
    },
    uuid_row(RlvEnvSetting::CloudTexture, "cloudtexture"),
    float_row(RlvEnvSetting::CloudVariance, "cloudvariance"),
    float_row(RlvEnvSetting::MoonBrightness, "moonbrightness"),
    float_row(RlvEnvSetting::MoonScale, "moonscale"),
    uuid_row(RlvEnvSetting::MoonTexture, "moontexture"),
    float_row(RlvEnvSetting::SunGlowSize, "sunglowsize"),
    float_row(RlvEnvSetting::SunGlowFocus, "sunglowfocus"),
    legacy_row(
        RlvEnvSetting::SunlightColor,
        "sunlightcolor",
        "sunmooncolor",
    ),
    float_row(RlvEnvSetting::SunScale, "sunscale"),
    uuid_row(RlvEnvSetting::SunTexture, "suntexture"),
    float_row(RlvEnvSetting::StarBrightness, "starbrightness"),
    float_row(RlvEnvSetting::SunAzimuth, "sunazimuth"),
    float_row(RlvEnvSetting::SunElevation, "sunelevation"),
    float_row(RlvEnvSetting::MoonAzimuth, "moonazimuth"),
    float_row(RlvEnvSetting::MoonElevation, "moonelevation"),
    float_row(RlvEnvSetting::EastAngle, "eastangle"),
    float_row(RlvEnvSetting::SunMoonPosition, "sunmoonposition"),
    float_row(RlvEnvSetting::DayTime, "daytime"),
];

/// A row a script may write but not read (`registerSetEnvFn` with no matching
/// `registerGetEnvFn`).
const fn write_only(
    setting: RlvEnvSetting,
    name: &'static str,
    kind: RlvEnvKind,
) -> RlvEnvSettingDef {
    RlvEnvSettingDef {
        setting,
        name,
        legacy_name: None,
        kind,
        readable: false,
        writable: true,
    }
}

/// A readable and writable scalar row.
const fn float_row(setting: RlvEnvSetting, name: &'static str) -> RlvEnvSettingDef {
    RlvEnvSettingDef {
        setting,
        name,
        legacy_name: None,
        kind: RlvEnvKind::Float,
        readable: true,
        writable: true,
    }
}

/// A readable and writable texture row.
const fn uuid_row(setting: RlvEnvSetting, name: &'static str) -> RlvEnvSettingDef {
    RlvEnvSettingDef {
        setting,
        name,
        legacy_name: None,
        kind: RlvEnvKind::Uuid,
        readable: true,
        writable: true,
    }
}

/// A readable and writable colour row that also answers to a legacy
/// per-component spelling.
const fn legacy_row(
    setting: RlvEnvSetting,
    name: &'static str,
    legacy_name: &'static str,
) -> RlvEnvSettingDef {
    RlvEnvSettingDef {
        setting,
        name,
        legacy_name: Some(legacy_name),
        kind: RlvEnvKind::Color,
        readable: true,
        writable: true,
    }
}

/// A component of a colour named by the legacy per-component spellings
/// (`rlvGetColorComponentFromCharacter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RlvColorComponent {
    /// `r` or `x` — the red channel, or the first of a pair.
    Red,
    /// `g` or `y` — the green channel, or the second of a pair.
    Green,
    /// `b` or `d` — the blue channel. Never valid on a pair.
    Blue,
    /// `i` — not a channel at all but the pre-EEP *intensity*: the brightest of
    /// the three on the way out, and a proportional rescale of all three on the
    /// way in. Never valid on a pair.
    Intensity,
}

impl RlvColorComponent {
    /// The component a trailing character names, or `None` when it names none.
    #[must_use]
    pub const fn from_char(ch: char) -> Option<Self> {
        match ch {
            'r' | 'x' => Some(Self::Red),
            'g' | 'y' => Some(Self::Green),
            'b' | 'd' => Some(Self::Blue),
            'i' => Some(Self::Intensity),
            _ => None,
        }
    }
}

/// Whether the **user** may change their own environment right now
/// (`RlvActions::canChangeEnvironment`, `rlvactions.cpp:396`).
///
/// `@setenv=n` hands one object the sky. While it holds it the environment menu
/// and the environment editors are the user's half of that bargain and have to
/// stop working, or the two would fight over one layer — the reference closes
/// the environment floaters outright and refuses the World ▸ Environment menu
/// events (`RlvBehaviourToggleHandler<RLV_BHVR_SETENV>`, `llviewermenu.cpp`).
///
/// The *object's* own `@setenv_*` commands are gated separately, inside
/// [`RlvState::run_environment`], because there the question is not whether
/// anyone holds the restriction but whether the object asking is the one that
/// does. This is the same split the debug window draws between
/// [`is_debug_setting_locked`](crate::is_debug_setting_locked) and its own
/// write gate.
#[must_use]
pub fn can_change_environment(state: &RlvState) -> bool {
    !state.has_behaviour(RlvBehaviour::Setenv)
}

/// Which of the two lookup directions a subkey is being resolved for — the
/// reference keeps a separate map per direction, so a write-only row is simply
/// absent when a read looks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// `@getenv_*`.
    Read,
    /// `@setenv_*`.
    Write,
}

impl Direction {
    /// Whether `row` is in this direction's map.
    const fn admits(self, row: &RlvEnvSettingDef) -> bool {
        match self {
            Self::Read => row.readable,
            Self::Write => row.writable,
        }
    }
}

/// Look one subkey up: exactly first, then — as the reference does — with its
/// last character stripped and read as a colour component.
///
/// `None` means the subkey names nothing in this direction, which is not a
/// failure but a command this handler does not own.
#[must_use]
fn lookup(
    subkey: &str,
    direction: Direction,
) -> Option<(&'static RlvEnvSettingDef, Option<RlvColorComponent>)> {
    if let Some(row) = RLV_ENV_SETTINGS
        .iter()
        .find(|row| row.name == subkey && direction.admits(row))
    {
        return Some((row, None));
    }
    let component = RlvColorComponent::from_char(subkey.chars().next_back()?)?;
    let base = subkey.get(..subkey.len().checked_sub(1)?)?;
    let row = RLV_ENV_SETTINGS
        .iter()
        .find(|row| row.legacy_name == Some(base) && direction.admits(row))?;
    Some((row, Some(component)))
}

// --- The decoded command ---------------------------------------------------

/// One `@getenv_*` / `@setenv_*` command, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvEnvCommand {
    /// `@getenv_<subkey>=<channel>` — read one value of the sky.
    GetEnv {
        /// The subkey as the script spelled it, which is not necessarily one
        /// the table knows.
        subkey: String,
        /// The channel the answer is shouted on.
        channel: i32,
    },
    /// `@setenv_<subkey>:<value>=force` — write one value of the sky, or
    /// replace the environment.
    SetEnv {
        /// The subkey as the script spelled it.
        subkey: String,
        /// The value text, still unparsed — what it parses *as* depends on the
        /// row it lands on.
        value: String,
    },
}

impl RlvEnvCommand {
    /// Decode `command` as an environment command, or `None` when it is not
    /// one.
    ///
    /// The conditions are the reference's, in its order: only a keyword the
    /// dictionary did not claim ([`RlvBehaviour::Unknown`]) can be one; the half
    /// before the first `_` must be exactly `getenv` or `setenv`; the half after
    /// it must be at least three characters; and the param kind decides which of
    /// the two it is, so `@getenv_ambient=force` is not a malformed write but no
    /// command at all.
    ///
    /// ```
    /// # use sl_rlv::{RlvCommand, RlvEnvCommand};
    /// let cmd = RlvCommand::parse_field("getenv_ambient=2222")?;
    /// assert_eq!(
    ///     RlvEnvCommand::classify(&cmd),
    ///     Some(RlvEnvCommand::GetEnv { subkey: "ambient".to_owned(), channel: 2222 })
    /// );
    /// # Ok::<(), sl_rlv::RlvParseError>(())
    /// ```
    #[must_use]
    pub fn classify(command: &RlvCommand) -> Option<Self> {
        if command.behaviour != RlvBehaviour::Unknown {
            return None;
        }
        let (head, subkey) = command.keyword.split_once('_')?;
        if subkey.len() < MIN_SUBKEY_LEN {
            return None;
        }
        let option = command.option.as_deref().unwrap_or("");
        match (head, &command.param) {
            (GETENV_HEAD, &RlvParam::Reply { channel }) => Some(Self::GetEnv {
                subkey: subkey.to_owned(),
                channel,
            }),
            (SETENV_HEAD, &RlvParam::Force) => Some(Self::SetEnv {
                subkey: subkey.to_owned(),
                value: option.to_owned(),
            }),
            _ => None,
        }
    }
}

/// What running an environment command produced.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RlvEnvResult {
    /// How it went, in the same vocabulary every other command reports in.
    pub outcome: RlvOutcome,
    /// The line to shout back, for a `@getenv_*`. `None` for a write, and for a
    /// read whose channel is not one a reply may go on — which, unlike the
    /// debug-setting family, this one reports as a **failure**, because the
    /// reference builds its outcome from whether `sendChatReply` went out.
    pub reply: Option<RlvReply>,
}

impl RlvEnvResult {
    /// A result that only reports an outcome.
    const fn outcome(outcome: RlvOutcome) -> Self {
        Self {
            outcome,
            reply: None,
        }
    }
}

// --- The wire value --------------------------------------------------------

/// One value as a *script* sees it: scaled, and in the spelling the reference
/// answers with.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RlvEnvValue {
    /// A number.
    Float(f32),
    /// Three numbers.
    Color([f32; 3]),
    /// Two numbers.
    Vec2([f32; 2]),
    /// A UUID.
    Uuid(Uuid),
}

impl RlvEnvValue {
    /// The text a script is answered with — `std::to_string` for a number,
    /// `llformat("%f/…")` for the rest, `LLUUID::asString` for an id.
    #[must_use]
    pub fn to_text(self) -> String {
        match self {
            Self::Float(value) => format!("{value:.6}"),
            Self::Color([red, green, blue]) => format!("{red:.6}/{green:.6}/{blue:.6}"),
            Self::Vec2([x, y]) => format!("{x:.6}/{y:.6}"),
            Self::Uuid(id) => id.to_string(),
        }
    }

    /// This value's `component`, for a legacy per-component read.
    ///
    /// `Intensity` is the brightest of a colour's three, and is not a component
    /// of a pair at all — `None` there, which the reference reports as a failure
    /// rather than as an empty answer.
    #[must_use]
    const fn component(self, component: RlvColorComponent) -> Option<f32> {
        match self {
            Self::Color([red, green, blue]) => Some(match component {
                RlvColorComponent::Red => red,
                RlvColorComponent::Green => green,
                RlvColorComponent::Blue => blue,
                // `const fn` cannot call `f32::max`, and the reference's
                // `llmax` is this comparison anyway.
                RlvColorComponent::Intensity => {
                    let highest = if red > green { red } else { green };
                    if highest > blue { highest } else { blue }
                }
            }),
            Self::Vec2([x, y]) => match component {
                RlvColorComponent::Red => Some(x),
                RlvColorComponent::Green => Some(y),
                RlvColorComponent::Blue | RlvColorComponent::Intensity => None,
            },
            Self::Float(_) | Self::Uuid(_) => None,
        }
    }

    /// This value with `component` replaced by `value`, for a legacy
    /// per-component write.
    ///
    /// The `Intensity` rule is the reference's, arithmetic and all
    /// (`handleLegacySetFn<LLColor3>`): a write of zero — or a write onto a
    /// colour that is already black — flattens all three to the value, and
    /// anything else rescales the three proportionally so their brightest
    /// becomes the value.
    #[must_use]
    fn with_component(self, component: RlvColorComponent, value: f32) -> Option<Self> {
        match self {
            Self::Color([red, green, blue]) => Some(Self::Color(match component {
                RlvColorComponent::Red => [value, green, blue],
                RlvColorComponent::Green => [red, value, blue],
                RlvColorComponent::Blue => [red, green, value],
                RlvColorComponent::Intensity => {
                    let brightest = red.max(green).max(blue);
                    if is_zero(value) || is_zero(brightest) {
                        [value, value, value]
                    } else {
                        let scale = 1.0 + (value - brightest) / brightest;
                        [red * scale, green * scale, blue * scale]
                    }
                }
            })),
            Self::Vec2([x, y]) => match component {
                RlvColorComponent::Red => Some(Self::Vec2([value, y])),
                RlvColorComponent::Green => Some(Self::Vec2([x, value])),
                RlvColorComponent::Blue | RlvColorComponent::Intensity => None,
            },
            Self::Float(_) | Self::Uuid(_) => None,
        }
    }
}

/// Parse a script's value text as `kind`, the reference's
/// `RlvCommandOptionHelper::parseOption`.
///
/// A number is `std::stof`, which takes the longest prefix it can use; a colour
/// or a pair is `sscanf("%f/%f[/%f]")`, which needs every field but ignores
/// whatever follows the last one; a UUID is `LLUUID::set`, which is strict.
#[must_use]
fn parse_value(kind: RlvEnvKind, text: &str) -> Option<RlvEnvValue> {
    match kind {
        RlvEnvKind::Float => parse_prefix::<f32>(text).map(RlvEnvValue::Float),
        RlvEnvKind::Color => {
            let mut parts = text.splitn(3, '/');
            let red = parse_prefix::<f32>(parts.next()?)?;
            let green = parse_prefix::<f32>(parts.next()?)?;
            let blue = parse_prefix::<f32>(parts.next()?)?;
            Some(RlvEnvValue::Color([red, green, blue]))
        }
        RlvEnvKind::Vec2 => {
            let mut parts = text.splitn(2, '/');
            let x = parse_prefix::<f32>(parts.next()?)?;
            let y = parse_prefix::<f32>(parts.next()?)?;
            Some(RlvEnvValue::Vec2([x, y]))
        }
        RlvEnvKind::Uuid => Uuid::try_parse(text).ok().map(RlvEnvValue::Uuid),
        // Free text is not a value in this sense; the two rows that carry it are
        // handled before this is reached.
        RlvEnvKind::Text => None,
    }
}

// --- Spherical geometry ----------------------------------------------------

/// Whether `value` is exactly zero, either sign — the reference's `is_zero`
/// (`llmath.h:109`), which is a bit test rather than a tolerance and is what its
/// azimuth / elevation special cases branch on.
#[must_use]
#[expect(
    clippy::verbose_bit_mask,
    reason = "the mask is the reference's own line (`(*(U32*)(&x) & 0x7fffffff) == 0`), and \
              reads as what it is — the sign bit dropped — where the suggested \
              `trailing_zeros() >= 31` reads as nothing at all"
)]
const fn is_zero(value: f32) -> bool {
    value.to_bits() & 0x7fff_ffff == 0
}

/// Whether `value` is exactly `-1`, which is what `@setenv_daytime` compares
/// against to mean "drop the local environment".
#[must_use]
const fn is_minus_one(value: f32) -> bool {
    value.to_bits() == (-1.0_f32).to_bits()
}

/// The azimuth of a direction, in `[0, 2π)`
/// (`rlvGetAzimuthFromDirectionVector`).
///
/// Straight up (or straight down) carries no azimuth at all, so the reference
/// answers a quarter turn there rather than whatever `atan2` makes of it; a
/// direction with no northing answers zero for the same reason.
#[must_use]
fn azimuth_of(direction: [f32; 3]) -> f32 {
    let [x, y, _] = direction;
    if is_zero(y) {
        return 0.0;
    }
    if is_zero(x) {
        return QUARTER_TURN;
    }
    let azimuth = y.atan2(x);
    if azimuth >= 0.0 {
        azimuth
    } else {
        azimuth + TWO_PI
    }
}

/// The elevation of a direction, in `[0, 2π)`
/// (`rlvGetElevationFromDirectionVector`).
///
/// The two axis-aligned cases use `atan2` in the plane the direction lies in,
/// which gives elevation the full turn of range the pre-EEP values had; the
/// general case is the plain `asin` of the up component.
#[must_use]
fn elevation_of(direction: [f32; 3]) -> f32 {
    let [x, y, z] = direction;
    if is_zero(z) {
        return 0.0;
    }
    let elevation = if is_zero(x) && !is_zero(y) {
        z.atan2(y)
    } else if !is_zero(x) && is_zero(y) {
        z.atan2(x)
    } else {
        z.clamp(-1.0, 1.0).asin()
    };
    if elevation >= 0.0 {
        elevation
    } else {
        elevation + TWO_PI
    }
}

/// Bring an angle into `[0, 2π]` by whole turns (`normalize_angle_domain`) —
/// what the legacy east angle is reported as a fraction of.
#[must_use]
fn normalize_angle(angle: f32) -> f32 {
    let mut angle = angle;
    while angle < 0.0 {
        angle += TWO_PI;
    }
    while angle > TWO_PI {
        angle -= TWO_PI;
    }
    angle
}

// --- Running a command -----------------------------------------------------

/// Run one environment command — the body of [`RlvState::run_environment`],
/// which is where it is documented.
pub(crate) fn run(
    state: &RlvState,
    issuer: Uuid,
    command: &RlvCommand,
    source: &mut impl RlvEnvSource,
) -> Option<RlvEnvResult> {
    match RlvEnvCommand::classify(command)? {
        RlvEnvCommand::GetEnv {
            ref subkey,
            channel,
        } => {
            let (row, component) = lookup(subkey, Direction::Read)?;
            Some(read(row.setting, component, channel, source))
        }
        RlvEnvCommand::SetEnv {
            ref subkey,
            ref value,
        } => {
            // The gate comes before the lookup, so an object that may not touch
            // the environment is refused even for a subkey that names nothing.
            // `@setenv=n` gives one object the environment, and that object is
            // not refused by its own restriction.
            if state.has_behaviour_except(RlvBehaviour::Setenv, "", issuer) {
                return Some(RlvEnvResult::outcome(RlvOutcome::FailedLock));
            }
            let (row, component) = lookup(subkey, Direction::Write)?;
            Some(RlvEnvResult::outcome(write(row, component, value, source)))
        }
    }
}

/// Answer a `@getenv_<subkey>=<channel>`.
fn read(
    setting: RlvEnvSetting,
    component: Option<RlvColorComponent>,
    channel: i32,
    source: &impl RlvEnvSource,
) -> RlvEnvResult {
    // `daytime` is the one row with no value behind it, only a verdict.
    let message = if setting == RlvEnvSetting::DayTime {
        // A day cycle is running, so there is no fixed time to report: `-1`. A
        // pinned sky has one, but the reference will not say what it is — it
        // answers `2`, which `@setenv_daytime` rejects, so a script can hand the
        // answer straight back without moving the sky.
        let answer = if source.has_fixed_sky() {
            2.0_f32
        } else {
            -1.0
        };
        RlvEnvValue::Float(answer).to_text()
    } else {
        let Some(value) = value_of(setting, source) else {
            // No sky to read. The script is still answered — one that asked a
            // question must not be left waiting — but with nothing, and the
            // outcome says the viewer could not do it rather than that the sky
            // is all zeroes.
            return RlvEnvResult {
                outcome: RlvOutcome::Failed,
                reply: reply_on(channel, String::new()),
            };
        };
        match component {
            None => value.to_text(),
            // A component this value does not have — the blue or the intensity
            // of a pair — is the reference's empty reply, which it reports as a
            // failure and does not send.
            Some(component) => match value.component(component) {
                None => return RlvEnvResult::outcome(RlvOutcome::FailedParam),
                Some(single) => RlvEnvValue::Float(single).to_text(),
            },
        }
    };
    match reply_on(channel, message) {
        // `RlvUtil::sendChatReply` refusing the channel is a failure here, not
        // the silent drop the debug-setting family settles for.
        None => RlvEnvResult::outcome(RlvOutcome::FailedParam),
        Some(reply) => RlvEnvResult {
            outcome: RlvOutcome::Success,
            reply: Some(reply),
        },
    }
}

/// The reply to shout `message` on `channel`, or `None` when the channel is not
/// one a reply may go on.
#[must_use]
fn reply_on(channel: i32, message: String) -> Option<RlvReply> {
    is_valid_reply_channel(channel, false).then(|| RlvReply {
        channel,
        message: truncate_chat(&message).to_owned(),
    })
}

/// Apply a `@setenv_<subkey>:<value>=force`.
fn write(
    row: &RlvEnvSettingDef,
    component: Option<RlvColorComponent>,
    text: &str,
    source: &mut impl RlvEnvSource,
) -> RlvOutcome {
    // The four whole-environment rows replace the environment rather than edit a
    // value of it, and none of them has a legacy per-component form.
    if let Some(request) = whole_environment(row.setting, text) {
        return match request {
            Err(outcome) => outcome,
            Ok(request) => {
                if source.apply_environment(&request) {
                    RlvOutcome::Success
                } else {
                    RlvOutcome::FailedOption
                }
            }
        };
    }
    // A legacy per-component write always sends a single number, whatever the
    // row holds — it is `parseOption<float>` in the reference regardless.
    let kind = if component.is_some() {
        RlvEnvKind::Float
    } else {
        row.kind
    };
    let Some(parsed) = parse_value(kind, text) else {
        return RlvOutcome::FailedParam;
    };
    let value = match (component, parsed) {
        (None, parsed) => parsed,
        (Some(component), RlvEnvValue::Float(single)) => {
            // Applied on top of the value that is there now, in the *scaled*
            // space the script sees — which is where the reference applies it
            // too, because its legacy lambdas are the ordinary get and set.
            let Some(current) = value_of(row.setting, source) else {
                return RlvOutcome::Failed;
            };
            let Some(merged) = current.with_component(component, single) else {
                return RlvOutcome::FailedParam;
            };
            merged
        }
        (Some(_), _) => return RlvOutcome::FailedParam,
    };
    if set_value(row.setting, value, source) {
        RlvOutcome::Success
    } else {
        RlvOutcome::Failed
    }
}

/// Decode a whole-environment row's option, or `None` when `setting` is not one
/// of the four.
///
/// The inner `Err` is the outcome for an option that parsed and was still
/// unusable — the reference's `RLV_RET_FAILED_OPTION`, as against the
/// `RLV_RET_FAILED_PARAM` it reports for one it could not parse at all.
#[must_use]
fn whole_environment(
    setting: RlvEnvSetting,
    text: &str,
) -> Option<Result<RlvEnvRequest, RlvOutcome>> {
    match setting {
        RlvEnvSetting::Asset => Some(match Uuid::try_parse(text) {
            Err(_) => Err(RlvOutcome::FailedParam),
            // A well-formed null id names no asset, which the reference calls a
            // bad option rather than a bad parse.
            Ok(id) if id.is_nil() => Err(RlvOutcome::FailedOption),
            Ok(id) => Ok(RlvEnvRequest::Asset(id)),
        }),
        // Free text always parses, so these two can only fail on the lookup the
        // consumer does.
        RlvEnvSetting::Preset => Some(Ok(RlvEnvRequest::Preset(text.to_owned()))),
        RlvEnvSetting::DayCycleName => Some(Ok(RlvEnvRequest::DayCycle(text.to_owned()))),
        RlvEnvSetting::DayTime => Some(match parse_prefix::<f32>(text) {
            None => Err(RlvOutcome::FailedParam),
            Some(position) if (0.0..=1.0).contains(&position) => {
                Ok(RlvEnvRequest::DayTime(position))
            }
            Some(position) if is_minus_one(position) => Ok(RlvEnvRequest::Clear),
            Some(_) => Err(RlvOutcome::FailedOption),
        }),
        _ => None,
    }
}

/// The scaled value behind one subkey — what a script reads, and the value a
/// legacy per-component write is applied on top of.
#[must_use]
fn value_of(setting: RlvEnvSetting, source: &impl RlvEnvSource) -> Option<RlvEnvValue> {
    match setting {
        RlvEnvSetting::Ambient => scaled_color(source, RlvSkyField::Ambient, SCALE_SUN_AMBIENT),
        RlvEnvSetting::SunlightColor => {
            scaled_color(source, RlvSkyField::SunlightColor, SCALE_SUN_AMBIENT)
        }
        RlvEnvSetting::BlueDensity => {
            scaled_color(source, RlvSkyField::BlueDensity, SCALE_BLUE_HORIZON_DENSITY)
        }
        RlvEnvSetting::BlueHorizon => {
            scaled_color(source, RlvSkyField::BlueHorizon, SCALE_BLUE_HORIZON_DENSITY)
        }
        RlvEnvSetting::CloudColor => scaled_color(source, RlvSkyField::CloudColor, 1.0),
        RlvEnvSetting::CloudDensity => scaled_color(source, RlvSkyField::CloudPosDensity1, 1.0),
        RlvEnvSetting::CloudDetail => scaled_color(source, RlvSkyField::CloudPosDensity2, 1.0),
        RlvEnvSetting::CloudScroll => match source.sky_value(RlvSkyField::CloudScrollRate)? {
            RlvSkyValue::Vec2(pair) => Some(RlvEnvValue::Vec2(pair)),
            _ => None,
        },
        RlvEnvSetting::DensityMultiplier => Some(RlvEnvValue::Float(
            float_field(source, RlvSkyField::DensityMultiplier)? / SCALE_DENSITY_MULTIPLIER,
        )),
        RlvEnvSetting::DistanceMultiplier => plain_float(source, RlvSkyField::DistanceMultiplier),
        RlvEnvSetting::DropletRadius => plain_float(source, RlvSkyField::DropletRadius),
        RlvEnvSetting::HazeDensity => plain_float(source, RlvSkyField::HazeDensity),
        RlvEnvSetting::HazeHorizon => plain_float(source, RlvSkyField::HazeHorizon),
        RlvEnvSetting::IceLevel => plain_float(source, RlvSkyField::IceLevel),
        RlvEnvSetting::MaxAltitude => plain_float(source, RlvSkyField::MaxY),
        RlvEnvSetting::MoistureLevel => plain_float(source, RlvSkyField::MoistureLevel),
        RlvEnvSetting::SceneGamma => plain_float(source, RlvSkyField::Gamma),
        RlvEnvSetting::CloudCoverage => plain_float(source, RlvSkyField::CloudShadow),
        RlvEnvSetting::CloudScale => plain_float(source, RlvSkyField::CloudScale),
        RlvEnvSetting::CloudVariance => plain_float(source, RlvSkyField::CloudVariance),
        RlvEnvSetting::MoonBrightness => plain_float(source, RlvSkyField::MoonBrightness),
        RlvEnvSetting::MoonScale => plain_float(source, RlvSkyField::MoonScale),
        RlvEnvSetting::SunScale => plain_float(source, RlvSkyField::SunScale),
        RlvEnvSetting::StarBrightness => plain_float(source, RlvSkyField::StarBrightness),
        RlvEnvSetting::CloudTexture => texture(source, RlvSkyField::CloudTexture),
        RlvEnvSetting::MoonTexture => texture(source, RlvSkyField::MoonTexture),
        RlvEnvSetting::SunTexture => texture(source, RlvSkyField::SunTexture),
        // The glow slider runs backwards from the channel behind it: a bigger
        // glow is a smaller red.
        RlvEnvSetting::SunGlowSize => {
            let [red, _, _] = color_field(source, RlvSkyField::Glow)?;
            Some(RlvEnvValue::Float(2.0 - red / SCALE_GLOW_R))
        }
        RlvEnvSetting::SunGlowFocus => {
            let [_, _, blue] = color_field(source, RlvSkyField::Glow)?;
            Some(RlvEnvValue::Float(blue / SCALE_GLOW_B))
        }
        RlvEnvSetting::SunAzimuth => Some(RlvEnvValue::Float(azimuth_of(
            source.sky_direction(RlvSkyBody::Sun)?,
        ))),
        RlvEnvSetting::SunElevation => Some(RlvEnvValue::Float(elevation_of(
            source.sky_direction(RlvSkyBody::Sun)?,
        ))),
        RlvEnvSetting::MoonAzimuth => Some(RlvEnvValue::Float(azimuth_of(
            source.sky_direction(RlvSkyBody::Moon)?,
        ))),
        RlvEnvSetting::MoonElevation => Some(RlvEnvValue::Float(elevation_of(
            source.sky_direction(RlvSkyBody::Moon)?,
        ))),
        // Legacy WindLight turned the other way round the compass, and reported
        // both angles as a fraction of a full turn.
        RlvEnvSetting::EastAngle => {
            let azimuth = azimuth_of(source.sky_direction(RlvSkyBody::Sun)?);
            Some(RlvEnvValue::Float(normalize_angle(-azimuth) / TWO_PI))
        }
        RlvEnvSetting::SunMoonPosition => {
            let elevation = elevation_of(source.sky_direction(RlvSkyBody::Sun)?);
            Some(RlvEnvValue::Float(elevation / TWO_PI))
        }
        // Not values: `daytime` is answered by `read`, and the other three are
        // write-only.
        RlvEnvSetting::DayTime
        | RlvEnvSetting::Asset
        | RlvEnvSetting::Preset
        | RlvEnvSetting::DayCycleName => None,
    }
}

/// A scalar sky field, or `None` when the source has none or answers with the
/// wrong kind.
#[must_use]
fn float_field(source: &impl RlvEnvSource, field: RlvSkyField) -> Option<f32> {
    match source.sky_value(field)? {
        RlvSkyValue::Float(value) => Some(value),
        _ => None,
    }
}

/// A colour sky field, or `None` when the source has none or answers with the
/// wrong kind.
#[must_use]
fn color_field(source: &impl RlvEnvSource, field: RlvSkyField) -> Option<[f32; 3]> {
    match source.sky_value(field)? {
        RlvSkyValue::Color(color) => Some(color),
        _ => None,
    }
}

/// A scalar sky field a script sees unscaled.
#[must_use]
fn plain_float(source: &impl RlvEnvSource, field: RlvSkyField) -> Option<RlvEnvValue> {
    Some(RlvEnvValue::Float(float_field(source, field)?))
}

/// A colour sky field divided by its slider scale.
#[must_use]
fn scaled_color(source: &impl RlvEnvSource, field: RlvSkyField, scale: f32) -> Option<RlvEnvValue> {
    let [red, green, blue] = color_field(source, field)?;
    Some(RlvEnvValue::Color([
        red / scale,
        green / scale,
        blue / scale,
    ]))
}

/// A texture sky field.
#[must_use]
fn texture(source: &impl RlvEnvSource, field: RlvSkyField) -> Option<RlvEnvValue> {
    match source.sky_value(field)? {
        RlvSkyValue::Texture(id) => Some(RlvEnvValue::Uuid(id)),
        _ => None,
    }
}

/// Write one subkey's scaled value back into the sky, answering whether it
/// could be written.
fn set_value(setting: RlvEnvSetting, value: RlvEnvValue, source: &mut impl RlvEnvSource) -> bool {
    match setting {
        RlvEnvSetting::Ambient => {
            set_scaled_color(source, RlvSkyField::Ambient, value, SCALE_SUN_AMBIENT)
        }
        RlvEnvSetting::SunlightColor => {
            set_scaled_color(source, RlvSkyField::SunlightColor, value, SCALE_SUN_AMBIENT)
        }
        RlvEnvSetting::BlueDensity => set_scaled_color(
            source,
            RlvSkyField::BlueDensity,
            value,
            SCALE_BLUE_HORIZON_DENSITY,
        ),
        RlvEnvSetting::BlueHorizon => set_scaled_color(
            source,
            RlvSkyField::BlueHorizon,
            value,
            SCALE_BLUE_HORIZON_DENSITY,
        ),
        RlvEnvSetting::CloudColor => set_scaled_color(source, RlvSkyField::CloudColor, value, 1.0),
        RlvEnvSetting::CloudDensity => {
            set_scaled_color(source, RlvSkyField::CloudPosDensity1, value, 1.0)
        }
        RlvEnvSetting::CloudDetail => {
            set_scaled_color(source, RlvSkyField::CloudPosDensity2, value, 1.0)
        }
        RlvEnvSetting::CloudScroll => match value {
            RlvEnvValue::Vec2(pair) => {
                source.set_sky_value(RlvSkyField::CloudScrollRate, RlvSkyValue::Vec2(pair))
            }
            _ => false,
        },
        RlvEnvSetting::DensityMultiplier => match value {
            RlvEnvValue::Float(number) => source.set_sky_value(
                RlvSkyField::DensityMultiplier,
                RlvSkyValue::Float(number * SCALE_DENSITY_MULTIPLIER),
            ),
            _ => false,
        },
        RlvEnvSetting::DistanceMultiplier => {
            set_plain_float(source, RlvSkyField::DistanceMultiplier, value)
        }
        RlvEnvSetting::DropletRadius => set_plain_float(source, RlvSkyField::DropletRadius, value),
        RlvEnvSetting::HazeDensity => set_plain_float(source, RlvSkyField::HazeDensity, value),
        RlvEnvSetting::HazeHorizon => set_plain_float(source, RlvSkyField::HazeHorizon, value),
        RlvEnvSetting::IceLevel => set_plain_float(source, RlvSkyField::IceLevel, value),
        RlvEnvSetting::MaxAltitude => set_plain_float(source, RlvSkyField::MaxY, value),
        RlvEnvSetting::MoistureLevel => set_plain_float(source, RlvSkyField::MoistureLevel, value),
        RlvEnvSetting::SceneGamma => set_plain_float(source, RlvSkyField::Gamma, value),
        RlvEnvSetting::CloudCoverage => set_plain_float(source, RlvSkyField::CloudShadow, value),
        RlvEnvSetting::CloudScale => set_plain_float(source, RlvSkyField::CloudScale, value),
        RlvEnvSetting::CloudVariance => set_plain_float(source, RlvSkyField::CloudVariance, value),
        RlvEnvSetting::MoonBrightness => {
            set_plain_float(source, RlvSkyField::MoonBrightness, value)
        }
        RlvEnvSetting::MoonScale => set_plain_float(source, RlvSkyField::MoonScale, value),
        RlvEnvSetting::SunScale => set_plain_float(source, RlvSkyField::SunScale, value),
        RlvEnvSetting::StarBrightness => {
            set_plain_float(source, RlvSkyField::StarBrightness, value)
        }
        RlvEnvSetting::CloudTexture => set_texture(source, RlvSkyField::CloudTexture, value),
        RlvEnvSetting::MoonTexture => set_texture(source, RlvSkyField::MoonTexture, value),
        RlvEnvSetting::SunTexture => set_texture(source, RlvSkyField::SunTexture, value),
        // Each glow slider owns one channel and leaves the other alone; the
        // reference zeroes the green, which nothing reads.
        RlvEnvSetting::SunGlowSize => match (value, color_field(source, RlvSkyField::Glow)) {
            (RlvEnvValue::Float(size), Some([_, _, blue])) => source.set_sky_value(
                RlvSkyField::Glow,
                RlvSkyValue::Color([(2.0 - size) * SCALE_GLOW_R, 0.0, blue]),
            ),
            _ => false,
        },
        RlvEnvSetting::SunGlowFocus => match (value, color_field(source, RlvSkyField::Glow)) {
            (RlvEnvValue::Float(focus), Some([red, _, _])) => source.set_sky_value(
                RlvSkyField::Glow,
                RlvSkyValue::Color([red, 0.0, focus * SCALE_GLOW_B]),
            ),
            _ => false,
        },
        RlvEnvSetting::SunAzimuth => set_angle(source, RlvSkyBody::Sun, value, Angle::Azimuth),
        RlvEnvSetting::SunElevation => set_angle(source, RlvSkyBody::Sun, value, Angle::Elevation),
        RlvEnvSetting::MoonAzimuth => set_angle(source, RlvSkyBody::Moon, value, Angle::Azimuth),
        RlvEnvSetting::MoonElevation => {
            set_angle(source, RlvSkyBody::Moon, value, Angle::Elevation)
        }
        // The two legacy spellings move the moon with the sun, keeping it
        // diametrically opposite as pre-EEP WindLight did.
        RlvEnvSetting::EastAngle => match value {
            RlvEnvValue::Float(east) => match source.sky_direction(RlvSkyBody::Sun) {
                None => false,
                Some(direction) => {
                    set_sun_and_moon(source, -east * TWO_PI, elevation_of(direction))
                }
            },
            _ => false,
        },
        RlvEnvSetting::SunMoonPosition => match value {
            RlvEnvValue::Float(position) => match source.sky_direction(RlvSkyBody::Sun) {
                None => false,
                Some(direction) => {
                    set_sun_and_moon(source, azimuth_of(direction), position * TWO_PI)
                }
            },
            _ => false,
        },
        // Handled before this is reached.
        RlvEnvSetting::DayTime
        | RlvEnvSetting::Asset
        | RlvEnvSetting::Preset
        | RlvEnvSetting::DayCycleName => false,
    }
}

/// Which half of a body's placement one of the four `@setenv_*azimuth` /
/// `@setenv_*elevation` subkeys replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Angle {
    /// Around the horizon; the elevation is kept.
    Azimuth,
    /// Above the horizon, clamped to a quarter turn either way; the azimuth is
    /// kept.
    Elevation,
}

/// Replace one half of `body`'s placement, keeping the other.
fn set_angle(
    source: &mut impl RlvEnvSource,
    body: RlvSkyBody,
    value: RlvEnvValue,
    half: Angle,
) -> bool {
    let RlvEnvValue::Float(angle) = value else {
        return false;
    };
    let Some(direction) = source.sky_direction(body) else {
        return false;
    };
    let (azimuth, elevation) = match half {
        Angle::Azimuth => (angle, elevation_of(direction)),
        Angle::Elevation => (
            azimuth_of(direction),
            angle.clamp(-QUARTER_TURN, QUARTER_TURN),
        ),
    };
    source.set_sky_angles(body, azimuth, elevation)
}

/// Place the sun, and the moon diametrically opposite it — what the two legacy
/// WindLight spellings do.
fn set_sun_and_moon(source: &mut impl RlvEnvSource, azimuth: f32, elevation: f32) -> bool {
    let sun = source.set_sky_angles(RlvSkyBody::Sun, azimuth, elevation);
    let moon = source.set_sky_angles(
        RlvSkyBody::Moon,
        azimuth + core::f32::consts::PI,
        -elevation,
    );
    sun && moon
}

/// Write a scalar field a script sees unscaled.
fn set_plain_float(source: &mut impl RlvEnvSource, field: RlvSkyField, value: RlvEnvValue) -> bool {
    match value {
        RlvEnvValue::Float(number) => source.set_sky_value(field, RlvSkyValue::Float(number)),
        _ => false,
    }
}

/// Write a colour field, multiplying the script's value by its slider scale.
fn set_scaled_color(
    source: &mut impl RlvEnvSource,
    field: RlvSkyField,
    value: RlvEnvValue,
    scale: f32,
) -> bool {
    match value {
        RlvEnvValue::Color([red, green, blue]) => source.set_sky_value(
            field,
            RlvSkyValue::Color([red * scale, green * scale, blue * scale]),
        ),
        _ => false,
    }
}

/// Write a texture field.
fn set_texture(source: &mut impl RlvEnvSource, field: RlvSkyField, value: RlvEnvValue) -> bool {
    match value {
        RlvEnvValue::Uuid(id) => source.set_sky_value(field, RlvSkyValue::Texture(id)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RLV_ENV_SETTINGS, RlvEnvCommand, RlvEnvRequest, RlvEnvSetting, RlvEnvSource, RlvSkyBody,
        RlvSkyField, RlvSkyKind, RlvSkyValue,
    };
    use crate::behaviour::RlvBehaviour;
    use crate::command::RlvCommand;
    use crate::state::{RlvOutcome, RlvState};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of the disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// A sky whose every field is a value a test can recognise on sight, plus a
    /// record of what was written to it.
    struct TestSky {
        /// The stored fields.
        fields: BTreeMap<RlvSkyField, RlvSkyValue>,
        /// Where the sun and the moon point.
        directions: BTreeMap<RlvSkyBody, [f32; 3]>,
        /// The angles the last `set_sky_angles` was given, per body.
        angles: BTreeMap<RlvSkyBody, (f32, f32)>,
        /// Every whole-environment change asked for, in order.
        requests: Vec<RlvEnvRequest>,
        /// What `has_fixed_sky` answers.
        fixed: bool,
        /// Whether there is a sky at all.
        empty: bool,
    }

    impl Default for TestSky {
        fn default() -> Self {
            let mut fields = BTreeMap::new();
            // The three scaled colours carry values that divide cleanly, so an
            // assertion reads as the arithmetic it is checking.
            let _previous =
                fields.insert(RlvSkyField::Ambient, RlvSkyValue::Color([0.75, 1.5, 3.0]));
            let _previous = fields.insert(
                RlvSkyField::SunlightColor,
                RlvSkyValue::Color([3.0, 1.5, 0.75]),
            );
            let _previous = fields.insert(
                RlvSkyField::BlueDensity,
                RlvSkyValue::Color([0.5, 1.0, 1.5]),
            );
            let _previous = fields.insert(
                RlvSkyField::BlueHorizon,
                RlvSkyValue::Color([1.5, 1.0, 0.5]),
            );
            let _previous = fields.insert(
                RlvSkyField::CloudColor,
                RlvSkyValue::Color([0.25, 0.5, 0.75]),
            );
            let _previous = fields.insert(
                RlvSkyField::CloudPosDensity1,
                RlvSkyValue::Color([1.0, 0.5, 0.25]),
            );
            let _previous = fields.insert(
                RlvSkyField::CloudPosDensity2,
                RlvSkyValue::Color([0.125, 0.25, 0.5]),
            );
            // Glow: red 10 reads back as a size of 1.5, blue -2.5 as a focus of
            // 0.5.
            let _previous = fields.insert(RlvSkyField::Glow, RlvSkyValue::Color([10.0, 0.0, -2.5]));
            let _previous =
                fields.insert(RlvSkyField::CloudScrollRate, RlvSkyValue::Vec2([0.2, 0.01]));
            let _previous =
                fields.insert(RlvSkyField::DensityMultiplier, RlvSkyValue::Float(0.0018));
            for (field, value) in [
                (RlvSkyField::DistanceMultiplier, 0.8),
                (RlvSkyField::DropletRadius, 800.0),
                (RlvSkyField::HazeDensity, 0.7),
                (RlvSkyField::HazeHorizon, 0.19),
                (RlvSkyField::IceLevel, 0.0),
                (RlvSkyField::MaxY, 1605.0),
                (RlvSkyField::MoistureLevel, 0.0),
                (RlvSkyField::Gamma, 1.0),
                (RlvSkyField::CloudShadow, 0.27),
                (RlvSkyField::CloudScale, 0.42),
                (RlvSkyField::CloudVariance, 0.0),
                (RlvSkyField::MoonBrightness, 0.5),
                (RlvSkyField::MoonScale, 1.0),
                (RlvSkyField::SunScale, 1.0),
                (RlvSkyField::StarBrightness, 0.0),
            ] {
                let _previous = fields.insert(field, RlvSkyValue::Float(value));
            }
            for field in [
                RlvSkyField::CloudTexture,
                RlvSkyField::MoonTexture,
                RlvSkyField::SunTexture,
            ] {
                let _previous = fields.insert(field, RlvSkyValue::Texture(Uuid::nil()));
            }
            let mut directions = BTreeMap::new();
            // Due east on the horizon, and the moon opposite it.
            let _previous = directions.insert(RlvSkyBody::Sun, [1.0, 0.0, 0.0]);
            let _previous = directions.insert(RlvSkyBody::Moon, [-1.0, 0.0, 0.0]);
            Self {
                fields,
                directions,
                angles: BTreeMap::new(),
                requests: Vec::new(),
                fixed: false,
                empty: false,
            }
        }
    }

    impl RlvEnvSource for TestSky {
        fn sky_value(&self, field: RlvSkyField) -> Option<RlvSkyValue> {
            if self.empty {
                return None;
            }
            self.fields.get(&field).copied()
        }

        fn set_sky_value(&mut self, field: RlvSkyField, value: RlvSkyValue) -> bool {
            if self.empty {
                return false;
            }
            let _previous = self.fields.insert(field, value);
            true
        }

        fn sky_direction(&self, body: RlvSkyBody) -> Option<[f32; 3]> {
            if self.empty {
                return None;
            }
            self.directions.get(&body).copied()
        }

        fn set_sky_angles(&mut self, body: RlvSkyBody, azimuth: f32, elevation: f32) -> bool {
            if self.empty {
                return false;
            }
            let _previous = self.angles.insert(body, (azimuth, elevation));
            // The direction the angles name, so a read after a write agrees with
            // it — the reference's `convert_azimuth_and_altitude_to_quat`.
            let _previous_direction = self.directions.insert(
                body,
                [
                    azimuth.cos() * elevation.cos(),
                    azimuth.sin() * elevation.cos(),
                    elevation.sin(),
                ],
            );
            true
        }

        fn apply_environment(&mut self, request: &RlvEnvRequest) -> bool {
            if self.empty {
                return false;
            }
            self.requests.push(request.clone());
            true
        }

        fn has_fixed_sky(&self) -> bool {
            self.fixed
        }
    }

    /// The object every test issues its commands from.
    const fn collar() -> Uuid {
        Uuid::from_u128(0x0C01_1A12)
    }

    /// Another object, for the `@setenv` ownership gate.
    const fn other() -> Uuid {
        Uuid::from_u128(0x0777_0777)
    }

    /// Run one command field through the environment dispatch.
    fn run(
        state: &RlvState,
        issuer: Uuid,
        field: &str,
        sky: &mut TestSky,
    ) -> Result<Option<super::RlvEnvResult>, TestError> {
        let command = RlvCommand::parse_field(field)?;
        Ok(state.run_environment(issuer, &command, sky))
    }

    /// The message a read answered with, or an error naming what went wrong
    /// instead.
    fn answer(field: &str, sky: &mut TestSky) -> Result<String, TestError> {
        let state = RlvState::new();
        let result = run(&state, collar(), field, sky)?.ok_or("not an environment command")?;
        assert_eq!(result.outcome, RlvOutcome::Success, "{field}");
        Ok(result.reply.ok_or("no reply")?.message)
    }

    /// The outcome of a write.
    fn write(field: &str, sky: &mut TestSky) -> Result<RlvOutcome, TestError> {
        let state = RlvState::new();
        Ok(run(&state, collar(), field, sky)?
            .ok_or("not an environment command")?
            .outcome)
    }

    /// Nothing in the table shares a name with anything else, and the two rows
    /// with a legacy spelling of their own are the two the reference renamed.
    #[test]
    fn the_table_is_a_lookup() {
        for (index, row) in RLV_ENV_SETTINGS.iter().enumerate() {
            assert!(
                RLV_ENV_SETTINGS
                    .iter()
                    .enumerate()
                    .all(|(other, candidate)| other == index || candidate.name != row.name),
                "{} is in the table twice",
                row.name
            );
            assert!(
                row.readable || row.writable,
                "{} can be neither read nor written",
                row.name
            );
        }
        let renamed: Vec<_> = RLV_ENV_SETTINGS
            .iter()
            .filter(|row| row.legacy_name.is_some_and(|legacy| legacy != row.name))
            .map(|row| (row.name, row.legacy_name))
            .collect();
        assert_eq!(
            renamed,
            vec![
                ("clouddensity", Some("cloud")),
                ("sunlightcolor", Some("sunmooncolor")),
            ]
        );
    }

    /// The keyword really does reach this handler: the dictionary claims
    /// `@setenv` but not `@setenv_ambient`, so the latter is an unknown keyword
    /// and not a local modifier of the former.
    #[test]
    fn a_subkey_is_not_a_modifier_of_setenv() -> Result<(), TestError> {
        assert_eq!(
            RlvCommand::parse_field("setenv=n")?.behaviour,
            RlvBehaviour::Setenv
        );
        assert_eq!(
            RlvCommand::parse_field("setenv_ambient=force")?.behaviour,
            RlvBehaviour::Unknown
        );
        Ok(())
    }

    /// The dispatch conditions, all four of them.
    #[test]
    fn classify_matches_the_reference_conditions() -> Result<(), TestError> {
        assert_eq!(
            RlvEnvCommand::classify(&RlvCommand::parse_field("setenv_ambient:1/2/3=force")?),
            Some(RlvEnvCommand::SetEnv {
                subkey: "ambient".to_owned(),
                value: "1/2/3".to_owned(),
            })
        );
        // The param kind is part of the match, not a later check.
        assert_eq!(
            RlvEnvCommand::classify(&RlvCommand::parse_field("getenv_ambient=force")?),
            None
        );
        assert_eq!(
            RlvEnvCommand::classify(&RlvCommand::parse_field("setenv_ambient=2222")?),
            None
        );
        // Too short to be looked up at all.
        assert_eq!(
            RlvEnvCommand::classify(&RlvCommand::parse_field("getenv_ab=2222")?),
            None
        );
        // A neighbouring family with the same shape.
        assert_eq!(
            RlvEnvCommand::classify(&RlvCommand::parse_field("getdebug_avatarsex=2222")?),
            None
        );
        Ok(())
    }

    /// Every field a subkey names is answered in the variant its kind declares,
    /// so a source cannot be right about one and wrong about the other.
    #[test]
    fn a_field_is_read_in_the_kind_it_declares() {
        let sky = TestSky::default();
        for (field, kind) in [
            (RlvSkyField::Ambient, RlvSkyKind::Color),
            (RlvSkyField::CloudScrollRate, RlvSkyKind::Vec2),
            (RlvSkyField::Gamma, RlvSkyKind::Float),
            (RlvSkyField::SunTexture, RlvSkyKind::Texture),
        ] {
            assert_eq!(field.kind(), kind);
            let value = sky.sky_value(field);
            assert!(
                matches!(
                    (value, kind),
                    (Some(RlvSkyValue::Color(_)), RlvSkyKind::Color)
                        | (Some(RlvSkyValue::Vec2(_)), RlvSkyKind::Vec2)
                        | (Some(RlvSkyValue::Float(_)), RlvSkyKind::Float)
                        | (Some(RlvSkyValue::Texture(_)), RlvSkyKind::Texture)
                ),
                "{field:?} answered the wrong kind"
            );
        }
    }

    /// The slider scales, each one on the row that carries it.
    #[test]
    fn a_read_is_scaled_the_way_the_reference_scales_it() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        // Ambient and sunlight run at a third.
        assert_eq!(
            answer("getenv_ambient=2222", &mut sky)?,
            "0.250000/0.500000/1.000000"
        );
        assert_eq!(
            answer("getenv_sunlightcolor=2222", &mut sky)?,
            "1.000000/0.500000/0.250000"
        );
        // Blue density and blue horizon at a half.
        assert_eq!(
            answer("getenv_bluedensity=2222", &mut sky)?,
            "0.250000/0.500000/0.750000"
        );
        assert_eq!(
            answer("getenv_bluehorizon=2222", &mut sky)?,
            "0.750000/0.500000/0.250000"
        );
        // The density multiplier is shown a thousand times bigger.
        assert_eq!(
            answer("getenv_densitymultiplier=2222", &mut sky)?,
            "1.800000"
        );
        // The glow's two halves, one of them inverted.
        assert_eq!(answer("getenv_sunglowsize=2222", &mut sky)?, "1.500000");
        assert_eq!(answer("getenv_sunglowfocus=2222", &mut sky)?, "0.500000");
        // An unscaled row, for contrast.
        assert_eq!(answer("getenv_cloudcoverage=2222", &mut sky)?, "0.270000");
        // A pair and a texture.
        assert_eq!(
            answer("getenv_cloudscroll=2222", &mut sky)?,
            "0.200000/0.010000"
        );
        assert_eq!(
            answer("getenv_suntexture=2222", &mut sky)?,
            "00000000-0000-0000-0000-000000000000"
        );
        Ok(())
    }

    /// A write goes back through the same scale it was read out of.
    #[test]
    fn a_write_is_unscaled_again() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        assert_eq!(
            write("setenv_ambient:0.25/0.5/1=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.sky_value(RlvSkyField::Ambient),
            Some(RlvSkyValue::Color([0.75, 1.5, 3.0]))
        );
        assert_eq!(
            write("setenv_densitymultiplier:2=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.sky_value(RlvSkyField::DensityMultiplier),
            Some(RlvSkyValue::Float(0.002))
        );
        // Each glow slider owns one channel and zeroes the unused middle.
        assert_eq!(
            write("setenv_sunglowsize:1=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.sky_value(RlvSkyField::Glow),
            Some(RlvSkyValue::Color([20.0, 0.0, -2.5]))
        );
        assert_eq!(
            write("setenv_sunglowfocus:1=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.sky_value(RlvSkyField::Glow),
            Some(RlvSkyValue::Color([20.0, 0.0, -5.0]))
        );
        Ok(())
    }

    /// A texture written as the null id is still a write — the sky names no
    /// texture, which is the viewer's own default.
    #[test]
    fn a_texture_is_written_as_an_id() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
        assert_eq!(
            write(&format!("setenv_cloudtexture:{id}=force"), &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.sky_value(RlvSkyField::CloudTexture),
            Some(RlvSkyValue::Texture(id))
        );
        assert_eq!(
            answer("getenv_cloudtexture=2222", &mut sky)?,
            id.to_string()
        );
        // Not a UUID at all: the reference's strict `LLUUID::set`.
        assert_eq!(
            write("setenv_cloudtexture:not-an-id=force", &mut sky)?,
            RlvOutcome::FailedParam
        );
        Ok(())
    }

    /// The legacy per-component spellings, including the two names that only
    /// exist in that form.
    #[test]
    fn a_component_suffix_reaches_the_legacy_names() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        // `r` and `x` are the same component, and the value is the *scaled* one.
        assert_eq!(answer("getenv_ambientr=2222", &mut sky)?, "0.250000");
        assert_eq!(answer("getenv_ambientx=2222", &mut sky)?, "0.250000");
        assert_eq!(answer("getenv_ambientg=2222", &mut sky)?, "0.500000");
        assert_eq!(answer("getenv_ambientb=2222", &mut sky)?, "1.000000");
        // `i` is the brightest of the three, not a fourth channel.
        assert_eq!(answer("getenv_ambienti=2222", &mut sky)?, "1.000000");
        // `cloud` and `sunmooncolor` exist only with a suffix.
        assert_eq!(answer("getenv_cloudr=2222", &mut sky)?, "1.000000");
        assert_eq!(answer("getenv_sunmooncolorr=2222", &mut sky)?, "1.000000");
        let state = RlvState::new();
        assert_eq!(run(&state, collar(), "getenv_cloud=2222", &mut sky)?, None);
        assert_eq!(
            run(&state, collar(), "getenv_sunmooncolor=2222", &mut sky)?,
            None
        );
        Ok(())
    }

    /// A legacy write sends one number and is applied on top of what is there,
    /// in the scaled space the script reads in.
    #[test]
    fn a_component_write_keeps_the_other_channels() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        assert_eq!(
            write("setenv_ambientg:1=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            answer("getenv_ambient=2222", &mut sky)?,
            "0.250000/1.000000/1.000000"
        );
        Ok(())
    }

    /// The pre-EEP intensity slider: a proportional rescale, and a flattening
    /// when either end of it is zero.
    #[test]
    fn the_intensity_component_rescales_the_whole_colour() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        // Brightest is 1.0 (the blue); asking for 2.0 doubles all three.
        assert_eq!(
            write("setenv_ambienti:2=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            answer("getenv_ambient=2222", &mut sky)?,
            "0.500000/1.000000/2.000000"
        );
        // Zero flattens rather than dividing by the brightest.
        assert_eq!(
            write("setenv_ambienti:0=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            answer("getenv_ambient=2222", &mut sky)?,
            "0.000000/0.000000/0.000000"
        );
        // And a black colour cannot be rescaled, so it is flattened too.
        assert_eq!(
            write("setenv_ambienti:1=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            answer("getenv_ambient=2222", &mut sky)?,
            "1.000000/1.000000/1.000000"
        );
        Ok(())
    }

    /// A pair has two components, not four: the blue and the intensity of a
    /// cloud scroll rate are refusals, both ways round.
    #[test]
    fn a_pair_has_no_blue_and_no_intensity() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        assert_eq!(answer("getenv_cloudscrollx=2222", &mut sky)?, "0.200000");
        assert_eq!(answer("getenv_cloudscrolly=2222", &mut sky)?, "0.010000");
        let state = RlvState::new();
        for field in ["getenv_cloudscrollb=2222", "getenv_cloudscrolli=2222"] {
            let result =
                run(&state, collar(), field, &mut sky)?.ok_or("not an environment command")?;
            assert_eq!(result.outcome, RlvOutcome::FailedParam, "{field}");
            assert_eq!(result.reply, None, "{field}");
        }
        assert_eq!(
            write("setenv_cloudscrollb:1=force", &mut sky)?,
            RlvOutcome::FailedParam
        );
        Ok(())
    }

    /// The sun's placement, read and written through both the modern spellings
    /// and the two legacy WindLight ones.
    #[test]
    fn the_sun_is_placed_by_azimuth_and_elevation() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        // Due east on the horizon.
        assert_eq!(answer("getenv_sunazimuth=2222", &mut sky)?, "0.000000");
        assert_eq!(answer("getenv_sunelevation=2222", &mut sky)?, "0.000000");
        // A quarter turn of azimuth, keeping the elevation.
        assert_eq!(
            write("setenv_sunazimuth:1.5707964=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(sky.angles.get(&RlvSkyBody::Sun).map(|it| it.1), Some(0.0));
        assert_eq!(answer("getenv_sunazimuth=2222", &mut sky)?, "1.570796");
        // The elevation is clamped to a quarter turn, so a full turn is a
        // quarter.
        assert_eq!(
            write("setenv_sunelevation:6.2831855=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.angles.get(&RlvSkyBody::Sun).map(|it| it.1),
            Some(core::f32::consts::FRAC_PI_2)
        );
        Ok(())
    }

    /// The legacy WindLight spellings turn the other way round the compass, are
    /// reported as a fraction of a full turn, and move the moon with the sun.
    #[test]
    fn the_legacy_angles_move_both_bodies() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        // The **negative** zero is the reference's, and is kept: the east angle
        // is the azimuth negated, and negating a zero azimuth leaves a signed
        // zero that `normalize_angle_domain` does not touch (it only adds a turn
        // to an angle *below* zero) and `%f` prints with its sign. A script
        // reading the number back gets zero either way.
        assert_eq!(answer("getenv_eastangle=2222", &mut sky)?, "-0.000000");
        assert_eq!(answer("getenv_sunmoonposition=2222", &mut sky)?, "0.000000");
        assert_eq!(
            write("setenv_sunmoonposition:0.125=force", &mut sky)?,
            RlvOutcome::Success
        );
        let sun = sky.angles.get(&RlvSkyBody::Sun).copied().ok_or("no sun")?;
        let moon = sky
            .angles
            .get(&RlvSkyBody::Moon)
            .copied()
            .ok_or("no moon")?;
        assert_eq!(format!("{:.6}", sun.1), "0.785398");
        // Diametrically opposite, as pre-EEP WindLight kept it.
        assert_eq!(format!("{:.6}", moon.1), "-0.785398");
        assert_eq!(
            format!("{:.6}", moon.0 - sun.0),
            format!("{:.6}", core::f32::consts::PI)
        );
        Ok(())
    }

    /// `@getenv_daytime` answers a verdict rather than a time, and
    /// `@setenv_daytime` takes a position, a `-1`, and nothing else.
    #[test]
    fn daytime_is_a_verdict_one_way_and_a_position_the_other() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        assert_eq!(answer("getenv_daytime=2222", &mut sky)?, "-1.000000");
        sky.fixed = true;
        // `2` is a value `@setenv_daytime` itself rejects, which is the point.
        assert_eq!(answer("getenv_daytime=2222", &mut sky)?, "2.000000");
        assert_eq!(
            write("setenv_daytime:0.25=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            write("setenv_daytime:-1=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            write("setenv_daytime:2=force", &mut sky)?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            write("setenv_daytime:noon=force", &mut sky)?,
            RlvOutcome::FailedParam
        );
        assert_eq!(
            sky.requests,
            vec![RlvEnvRequest::DayTime(0.25), RlvEnvRequest::Clear]
        );
        Ok(())
    }

    /// The three whole-environment rows a script installs an asset with, and
    /// the two ways one can be refused.
    #[test]
    fn an_asset_is_installed_whole() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let id = Uuid::from_u128(0xfeed_face_dead_beef_0123_4567_89ab_cdef);
        assert_eq!(
            write(&format!("setenv_asset:{id}=force"), &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            write("setenv_preset:sunrise=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            write("setenv_daycycle:default=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.requests,
            vec![
                RlvEnvRequest::Asset(id),
                RlvEnvRequest::Preset("sunrise".to_owned()),
                RlvEnvRequest::DayCycle("default".to_owned()),
            ]
        );
        // A well-formed null id names no asset — a bad option, not a bad parse.
        assert_eq!(
            write(
                "setenv_asset:00000000-0000-0000-0000-000000000000=force",
                &mut sky
            )?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            write("setenv_asset:sunrise=force", &mut sky)?,
            RlvOutcome::FailedParam
        );
        // A consumer that cannot resolve one reports a bad option.
        sky.empty = true;
        assert_eq!(
            write(&format!("setenv_asset:{id}=force"), &mut sky)?,
            RlvOutcome::FailedOption
        );
        Ok(())
    }

    /// The write-only rows are not readable, and a read of one is not a failure
    /// but a command this handler does not own.
    #[test]
    fn a_write_only_row_is_not_a_read() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let state = RlvState::new();
        for field in [
            "getenv_asset=2222",
            "getenv_preset=2222",
            "getenv_daycycle=2222",
        ] {
            assert_eq!(run(&state, collar(), field, &mut sky)?, None, "{field}");
        }
        Ok(())
    }

    /// `@setenv=n` gives one object the environment: another object's write is
    /// refused, the holder's own is not, and the gate is checked before the
    /// subkey is even looked up.
    #[test]
    fn setenv_gives_one_object_the_sky() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let mut state = RlvState::new();
        assert_eq!(
            state.apply(collar(), &RlvCommand::parse_field("setenv=n")?),
            RlvOutcome::Success
        );
        assert_eq!(
            run(&state, other(), "setenv_ambient:1/1/1=force", &mut sky)?
                .ok_or("not an environment command")?
                .outcome,
            RlvOutcome::FailedLock
        );
        // Even for a subkey that names nothing — the reference checks the gate
        // first and reports it handled.
        assert_eq!(
            run(&state, other(), "setenv_nonsense:1=force", &mut sky)?
                .ok_or("not an environment command")?
                .outcome,
            RlvOutcome::FailedLock
        );
        assert_eq!(
            run(&state, collar(), "setenv_ambient:1/1/1=force", &mut sky)?
                .ok_or("not an environment command")?
                .outcome,
            RlvOutcome::Success
        );
        // A *read* is never gated.
        assert_eq!(
            run(&state, other(), "getenv_ambient=2222", &mut sky)?
                .ok_or("not an environment command")?
                .outcome,
            RlvOutcome::Success
        );
        Ok(())
    }

    /// A channel no reply may go on is a failure here, not the silent drop the
    /// debug-setting family settles for.
    #[test]
    fn an_unusable_reply_channel_fails_the_read() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let state = RlvState::new();
        let result = run(&state, collar(), "getenv_ambient=0", &mut sky)?
            .ok_or("not an environment command")?;
        assert_eq!(result.outcome, RlvOutcome::FailedParam);
        assert_eq!(result.reply, None);
        Ok(())
    }

    /// A viewer with no sky yet still answers, because a script that asked a
    /// question must not be left waiting — but says it could not do it.
    #[test]
    fn a_read_without_a_sky_answers_nothing_and_says_so() -> Result<(), TestError> {
        let mut sky = TestSky {
            empty: true,
            ..TestSky::default()
        };
        let state = RlvState::new();
        let result = run(&state, collar(), "getenv_ambient=2222", &mut sky)?
            .ok_or("not an environment command")?;
        assert_eq!(result.outcome, RlvOutcome::Failed);
        assert_eq!(result.reply.ok_or("no reply")?.message, "");
        assert_eq!(
            write("setenv_ambient:1/1/1=force", &mut sky)?,
            RlvOutcome::Failed
        );
        Ok(())
    }

    /// A subkey the table does not know is not this handler's command at all,
    /// so the caller goes on to report it unknown.
    #[test]
    fn an_unknown_subkey_is_not_this_handler() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let state = RlvState::new();
        assert_eq!(
            run(&state, collar(), "getenv_nonsense=2222", &mut sky)?,
            None
        );
        assert_eq!(
            run(&state, collar(), "setenv_nonsense:1=force", &mut sky)?,
            None
        );
        Ok(())
    }

    /// A colour needs all three fields; anything past the last one is ignored,
    /// as `sscanf` ignores it.
    #[test]
    fn a_colour_needs_three_fields() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        assert_eq!(
            write("setenv_ambient:1/2=force", &mut sky)?,
            RlvOutcome::FailedParam
        );
        assert_eq!(
            write("setenv_ambient:1/2/3junk=force", &mut sky)?,
            RlvOutcome::Success
        );
        assert_eq!(
            sky.sky_value(RlvSkyField::Ambient),
            Some(RlvSkyValue::Color([3.0, 6.0, 9.0]))
        );
        Ok(())
    }

    /// Every subkey in the table is reachable through a command of its own
    /// direction, which is the only thing that makes the table a language.
    #[test]
    fn every_row_is_reachable() -> Result<(), TestError> {
        let mut sky = TestSky::default();
        let state = RlvState::new();
        for row in RLV_ENV_SETTINGS {
            if row.readable {
                assert!(
                    run(
                        &state,
                        collar(),
                        &format!("getenv_{}=2222", row.name),
                        &mut sky
                    )?
                    .is_some(),
                    "@getenv_{} is not reachable",
                    row.name
                );
            }
            if row.writable {
                assert!(
                    run(
                        &state,
                        collar(),
                        &format!("setenv_{}:0=force", row.name),
                        &mut sky
                    )?
                    .is_some(),
                    "@setenv_{} is not reachable",
                    row.name
                );
            }
        }
        // And the table is the size the reference registers.
        assert_eq!(RLV_ENV_SETTINGS.len(), 39);
        assert_eq!(
            RLV_ENV_SETTINGS
                .iter()
                .filter(|row| row.legacy_name.is_some())
                .count(),
            8
        );
        Ok(())
    }

    /// `RlvEnvSetting` is only ever reached through the table, so the two
    /// cannot drift: every variant the table names round-trips.
    #[test]
    fn every_variant_has_exactly_one_row() {
        /// Every variant of the subkey enum, kept here rather than in the crate
        /// because nothing but this check needs to enumerate them.
        const ALL: &[RlvEnvSetting] = &[
            RlvEnvSetting::Ambient,
            RlvEnvSetting::BlueDensity,
            RlvEnvSetting::BlueHorizon,
            RlvEnvSetting::DensityMultiplier,
            RlvEnvSetting::DistanceMultiplier,
            RlvEnvSetting::DropletRadius,
            RlvEnvSetting::HazeDensity,
            RlvEnvSetting::HazeHorizon,
            RlvEnvSetting::IceLevel,
            RlvEnvSetting::MaxAltitude,
            RlvEnvSetting::MoistureLevel,
            RlvEnvSetting::SceneGamma,
            RlvEnvSetting::CloudColor,
            RlvEnvSetting::CloudCoverage,
            RlvEnvSetting::CloudDensity,
            RlvEnvSetting::CloudDetail,
            RlvEnvSetting::CloudScale,
            RlvEnvSetting::CloudScroll,
            RlvEnvSetting::CloudTexture,
            RlvEnvSetting::CloudVariance,
            RlvEnvSetting::MoonBrightness,
            RlvEnvSetting::MoonScale,
            RlvEnvSetting::MoonTexture,
            RlvEnvSetting::SunGlowSize,
            RlvEnvSetting::SunGlowFocus,
            RlvEnvSetting::SunlightColor,
            RlvEnvSetting::SunScale,
            RlvEnvSetting::SunTexture,
            RlvEnvSetting::StarBrightness,
            RlvEnvSetting::SunAzimuth,
            RlvEnvSetting::SunElevation,
            RlvEnvSetting::MoonAzimuth,
            RlvEnvSetting::MoonElevation,
            RlvEnvSetting::EastAngle,
            RlvEnvSetting::SunMoonPosition,
            RlvEnvSetting::Asset,
            RlvEnvSetting::Preset,
            RlvEnvSetting::DayCycleName,
            RlvEnvSetting::DayTime,
        ];
        for setting in ALL {
            assert_eq!(
                RLV_ENV_SETTINGS
                    .iter()
                    .filter(|row| row.setting == *setting)
                    .count(),
                1,
                "{setting:?} does not have exactly one row"
            );
        }
        assert_eq!(ALL.len(), RLV_ENV_SETTINGS.len());
    }
}
