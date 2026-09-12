//! Environment (EEP) ingest — the Phase 22.1 slice.
//!
//! The viewer holds one [`EnvironmentState`] resource: the region's
//! Extended-Environment settings — its sky, water, and day cycle. It
//! starts at the built-in **legacy WindLight default**
//! ([`EnvironmentSettings::legacy_windlight_default`]), the same fallback the
//! reference viewer uses on a region that advertises no `ExtEnvironment`
//! capability, so the later sky / water / shadow phases always have settings to
//! render.
//!
//! On each region handshake the viewer requests the environment
//! ([`Command::RequestEnvironment`]); the grid's reply arrives as
//! [`SlSessionEvent::Environment`], which [`ingest_environment`] folds into the
//! resource. Parsing lives in `sl-proto` (Bevy-free); this module only requests,
//! stores, and logs — the sky / atmosphere rendering (P22.2), water (P23), and
//! shadows (P24) consume the stored settings.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use sl_client_bevy::{
    AssetKey, Command, DayCycle, DayCycleFrame, EnvironmentAsset, EnvironmentPushAction,
    EnvironmentSettings, ExperienceEnvironmentPush, ExperienceKey, Llsd, SettingsKind, SkySettings,
    SlCommand, SlEvent, SlSessionEvent, Uuid, WaterSettings, sky_with_blended_values,
    water_with_blended_values,
};
use sl_settings::SettingValue;
use sl_viewer_settings::ViewerSettings;

use sl_viewer_world_api::rlv::{RlvEnvironmentRequest, RlvEnvironmentSlot};

use crate::environment_assets::EnvironmentAssetManager;
use crate::sky_presets::FixedSky;

/// A World ▸ Environment menu selection: a time of day
/// ([`FixedSky`]) within one of three groups.
/// `None` on [`EnvironmentState`] means the region's shared environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FixedEnvironment {
    /// The region / parcel's *own* EEP day cycle, frozen at this time (fixed sun,
    /// the region's palette) — [`FixedSky::day_position`].
    DayCycle(FixedSky),
    /// A ported legacy Linden `A-*` WindLight preset — [`FixedSky::settings`].
    Legacy(FixedSky),
    /// A fetched reference `KNOWN_SKY_*` modern EEP library sky
    /// ([`FixedSky::modern_asset`]), resolved via [`EnvironmentAssetManager`] so it
    /// renders byte-identical input to Firestorm's matching preset.
    Modern(FixedSky),
}

impl FixedEnvironment {
    /// The time of day this selection pins, whichever group.
    pub(crate) const fn time(self) -> FixedSky {
        match self {
            Self::DayCycle(time) | Self::Legacy(time) | Self::Modern(time) => time,
        }
    }
}

/// One track of the local environment layer: the settings in force, and the
/// inventory settings **asset** they came from where there was one.
///
/// The asset id is not decoration. A surface that lists the settings assets in
/// inventory has to show *which* of them is in force — that is the whole of
/// `FloaterQuickPrefs::setSelectedEnvironment`, which reads
/// `sky->getAssetId()` and selects the matching row — and a sky is not
/// identifiable by its contents: two assets can hold the same frame, and a
/// script's edited sky holds no asset at all. `None` is that last case, and it
/// is what the reference's null asset id means too.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LocalTrack<T> {
    /// The settings asset this track was loaded from, or `None` for settings
    /// nothing in inventory names — a `@setenv_*` edit, or a sampled frame.
    pub asset: Option<Uuid>,
    /// The settings themselves.
    pub settings: T,
}

/// The **local** environment layer: the reference's `ENV_LOCAL`, which holds a
/// day cycle, a fixed sky and a fixed water *at the same time* and independently
/// (`LLEnvironment::DayInstance` — `setSky` and `setWater` each replace one and
/// leave the rest).
///
/// Three tracks rather than one asset because that is what the surfaces above it
/// are: the quick-preferences panel has one combo per track, and picking a water
/// preset there must not drop the sky the user picked a moment earlier. The one
/// place the tracks are *not* independent is a day cycle: installing one clears
/// the fixed sky and water, because `DayInstance::setDay` resets both and lets
/// the cycle animate them — which is why the reference's sky and water combos
/// fall back to showing "Day-cycle based" after a day is picked.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocalEnvironment {
    /// A whole day cycle, replacing the shared one.
    day: Option<LocalTrack<Box<DayCycle>>>,
    /// A fixed sky, pinned over whichever cycle is in force.
    sky: Option<LocalTrack<Box<SkySettings>>>,
    /// A fixed water frame, pinned over whichever cycle is in force.
    water: Option<LocalTrack<WaterSettings>>,
}

impl LocalEnvironment {
    /// Whether the layer holds nothing at all — the region's environment is
    /// what renders.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.day.is_none() && self.sky.is_none() && self.water.is_none()
    }

    /// The fixed sky in force, if any.
    #[must_use]
    pub const fn sky(&self) -> Option<&LocalTrack<Box<SkySettings>>> {
        self.sky.as_ref()
    }

    /// The fixed water in force, if any.
    #[must_use]
    pub const fn water(&self) -> Option<&LocalTrack<WaterSettings>> {
        self.water.as_ref()
    }

    /// The day cycle in force, if any.
    #[must_use]
    pub const fn day(&self) -> Option<&LocalTrack<Box<DayCycle>>> {
        self.day.as_ref()
    }

    /// Install one settings asset in the track it *is*, following
    /// `LLEnvironment::setEnvironment(ENV_LOCAL, settings)`: a sky or a water
    /// frame replaces only its own track, a day cycle replaces the cycle and
    /// clears both fixed frames.
    fn install(&mut self, asset: EnvironmentAsset, source: Option<Uuid>) {
        match asset {
            EnvironmentAsset::Sky(sky) => {
                self.sky = Some(LocalTrack {
                    asset: source,
                    settings: sky,
                });
            }
            EnvironmentAsset::Water(water) => {
                self.water = Some(LocalTrack {
                    asset: source,
                    settings: water,
                });
            }
            EnvironmentAsset::DayCycle(day) => {
                self.day = Some(LocalTrack {
                    asset: source,
                    settings: day,
                });
                self.sky = None;
                self.water = None;
            }
        }
    }
}

/// The **pushed** environment layer: the settings *experiences* have injected
/// over the region's (`llSetEnvironment`) — the reference's `ENV_PUSH`, held by
/// its `DayInjection` (`indra/newview/llenvironment.cpp`).
///
/// It sits above the region's and the parcel's settings and **below** the local
/// layer, which is the reference's own order (`ENV_EDIT`, `ENV_LOCAL`,
/// `ENV_PUSH`, `ENV_PARCEL`, `ENV_REGION`, `ENV_DEFAULT`): an experience
/// overrides the land, and the user overrides the experience.
///
/// Every injected value is filed under the experience that pushed it, because
/// that is the granularity a release has:
/// [`EnvironmentPushAction::Clear`] names one experience (or, with a nil id,
/// all of them) and must leave any other experience's injections standing.
/// Taking the last one away restores the region's environment with **no
/// refetch** — the settings underneath were never overwritten, only covered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PushedEnvironment {
    /// A whole day cycle an experience installed, and which one did — a
    /// [`Full`](EnvironmentPushAction::Full) push whose asset turned out to be a
    /// day cycle.
    day: Option<(ExperienceKey, Box<DayCycle>)>,
    /// A fixed sky frame an experience installed.
    sky: Option<(ExperienceKey, Box<SkySettings>)>,
    /// A fixed water frame an experience installed.
    water: Option<(ExperienceKey, WaterSettings)>,
    /// Per-key sky overrides from
    /// [`Partial`](EnvironmentPushAction::Partial) pushes, each tagged with the
    /// experience that last wrote it — the reference's `mOverrideValues` /
    /// `mOverrideExps`, which are per **key** and not per push, so two
    /// experiences can each own part of the sky.
    sky_values: BTreeMap<String, PushedValue>,
    /// The water counterpart of [`sky_values`](Self::sky_values).
    water_values: BTreeMap<String, PushedValue>,
}

/// One key an experience has pushed, and how far along its own blend it is.
#[derive(Debug, Clone, PartialEq)]
struct PushedValue {
    /// The experience that last wrote this key — the reference's
    /// `mOverrideExps` entry.
    experience: ExperienceKey,
    /// What it wrote.
    value: Llsd,
    /// The blend still running for this key, if any.
    blend: Option<ValueBlend>,
}

/// A single key's blend — the reference's `LLSettingsInjected::Injection`,
/// which is scheduled per key rather than per push and interpolates only that
/// key each tick (`applyInjections`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ValueBlend {
    /// Seconds still to run.
    remaining: f32,
    /// Seconds the blend runs for in total; always `> 0.0`.
    duration: f32,
    /// Whether this is the release fading back **out** to the value underneath
    /// (`mBlendIn == false`), rather than the push fading in. A key on its way
    /// out is nobody's any more: it does not keep its experience alive.
    fading_out: bool,
}

impl ValueBlend {
    /// How far toward the pushed value this key currently sits, `0.0..=1.0` —
    /// the reference's `mix`, inverted for a fade-out so the key starts at the
    /// pushed value and walks back to the one underneath.
    fn mix(&self) -> f32 {
        let elapsed = (1.0 - (self.remaining / self.duration)).clamp(0.0, 1.0);
        if self.fading_out {
            1.0 - elapsed
        } else {
            elapsed
        }
    }
}

impl PushedEnvironment {
    /// Whether no experience is holding any part of the environment — the
    /// reference's `!hasInjections()`, which is what makes it drop the whole
    /// `ENV_PUSH` instance.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.day.is_none()
            && self.sky.is_none()
            && self.water.is_none()
            && self.sky_values.is_empty()
            && self.water_values.is_empty()
    }

    /// Every experience currently holding some part of the environment — the
    /// reference's `mActiveExperiences`, and what a surface telling the user
    /// *who* changed their sky reads.
    ///
    /// A key still fading **out** does not count: the reference erases the
    /// experience from `mActiveExperiences` and `mOverrideExps` the moment the
    /// release is asked for, and only the fade itself outlives that. An
    /// experience whose sky is on its way off screen must not be re-asked about
    /// on the next parcel line, nor named as one holding the sky.
    #[must_use]
    pub fn experiences(&self) -> BTreeSet<ExperienceKey> {
        let tracks = [
            self.day.as_ref().map(|(experience, _)| *experience),
            self.sky.as_ref().map(|(experience, _)| *experience),
            self.water.as_ref().map(|(experience, _)| *experience),
        ];
        tracks
            .into_iter()
            .flatten()
            .chain(
                self.sky_values
                    .values()
                    .chain(self.water_values.values())
                    .filter(|pushed| !pushed.is_fading_out())
                    .map(|pushed| pushed.experience),
            )
            .collect()
    }

    /// Whether any key is still mid-blend — what
    /// [`advance_environment_transition`] ticks each frame, and the only reason
    /// the composed environment has to be rebuilt while nothing else changed.
    #[must_use]
    pub fn is_blending(&self) -> bool {
        self.sky_values
            .values()
            .chain(self.water_values.values())
            .any(|pushed| pushed.blend.is_some())
    }

    /// Advance every running per-key blend by `delta` seconds, completing the
    /// ones that are done: a fade-in becomes a plain override, a fade-out takes
    /// its key away. Returns whether anything moved, so the caller only
    /// recomposes when it did.
    fn advance_blends(&mut self, delta: f32) -> bool {
        let mut moved = false;
        for values in [&mut self.sky_values, &mut self.water_values] {
            for pushed in values.values_mut() {
                if let Some(blend) = &mut pushed.blend {
                    blend.remaining -= delta;
                    moved = true;
                    if blend.remaining <= 0.0 && !blend.fading_out {
                        pushed.blend = None;
                    }
                }
            }
            values.retain(|_, pushed| !pushed.is_finished_fading_out());
        }
        moved
    }

    /// Install a whole settings asset for `experience` — the reference's
    /// `setInjectedDay` / `setInjectedSky` / `setInjectedWater`.
    ///
    /// A day cycle clears the fixed sky and water it animates, exactly as a day
    /// cycle does in the local layer; a sky or a water frame replaces only its
    /// own track.
    fn install(&mut self, experience: ExperienceKey, asset: EnvironmentAsset) {
        match asset {
            EnvironmentAsset::Sky(sky) => self.sky = Some((experience, sky)),
            EnvironmentAsset::Water(water) => self.water = Some((experience, water)),
            EnvironmentAsset::DayCycle(day) => {
                self.day = Some((experience, day));
                self.sky = None;
                self.water = None;
            }
        }
    }

    /// Overlay the sky and/or water keys of a
    /// [`Partial`](EnvironmentPushAction::Partial) push — the reference's
    /// `injectSkySettings` / `injectWaterSettings`, one `injectSetting` per key.
    ///
    /// A push carrying neither is the reference's own special case: with both
    /// maps undefined `setExperienceEnvironment` *clears* this experience
    /// instead, so a script that pushes nothing releases what it held.
    ///
    /// `transition` is the push's own transition time: over
    /// [`INSTANT_TRANSITION`] each key is given its own blend rather than being
    /// written straight through, which is the reference's `injectSetting`
    /// threshold and the reason a partial push does **not** cross-fade the whole
    /// environment.
    fn inject(
        &mut self,
        experience: ExperienceKey,
        sky: Option<&Llsd>,
        water: Option<&Llsd>,
        transition: f32,
    ) {
        if sky.is_none() && water.is_none() {
            self.clear(Some(experience), transition);
            return;
        }
        overlay_pushed_values(&mut self.sky_values, experience, sky, transition);
        overlay_pushed_values(&mut self.water_values, experience, water, transition);
    }

    /// Drop everything `experience` is holding, or — for `None`, the nil
    /// experience id the reference reads as "all" — the whole layer.
    ///
    /// Per-key overrides fade back **out** over `transition` rather than
    /// vanishing (the reference's `removeInjection`, which turns each override
    /// it takes away into a blend-out injection); whole tracks are dropped at
    /// once, because the cross-fade that covers those is the whole-environment
    /// one [`EnvironmentState::apply_environment_push`] starts.
    fn clear(&mut self, experience: Option<ExperienceKey>, transition: f32) {
        if let Some(experience) = experience {
            if held_by(self.day.as_ref(), experience) {
                self.day = None;
            }
            if held_by(self.sky.as_ref(), experience) {
                self.sky = None;
            }
            if held_by(self.water.as_ref(), experience) {
                self.water = None;
            }
        } else {
            self.day = None;
            self.sky = None;
            self.water = None;
        }
        for values in [&mut self.sky_values, &mut self.water_values] {
            release_pushed_values(values, experience, transition);
        }
    }

    /// Whether `experience` — or, for `None`, anybody — holds one of the three
    /// whole-asset tracks. What decides whether releasing it is a
    /// whole-environment cross-fade or a set of per-key fades.
    fn holds_a_track(&self, experience: Option<ExperienceKey>) -> bool {
        match experience {
            Some(experience) => {
                held_by(self.day.as_ref(), experience)
                    || held_by(self.sky.as_ref(), experience)
                    || held_by(self.water.as_ref(), experience)
            }
            None => self.day.is_some() || self.sky.is_some() || self.water.is_some(),
        }
    }

    /// Apply the per-key injections to every frame of the cycle being composed.
    ///
    /// Per **frame** rather than to the sampled result, which reaches the same
    /// pixels by a route that keeps the day animating: the reference overrides
    /// the keys of a sky it recomputes each tick, and overriding both ends of an
    /// interpolation gives the same value at every point between them. Pinning
    /// the sampled frame instead would freeze the sky the moment an experience
    /// nudged one cloud setting.
    ///
    /// A key mid-blend is applied only `mix` of the way from the frame's own
    /// value, which is what makes a partial push fade **that key** instead of
    /// cross-fading the whole sky.
    fn apply_values(&self, settings: &mut EnvironmentSettings) {
        if !self.sky_values.is_empty() {
            let values = llsd_map_of(&self.sky_values);
            let mixes = mixes_of(&self.sky_values);
            for frame in settings.day_cycle.sky_frames.values_mut() {
                *frame = sky_with_blended_values(frame, &values, &mixes);
            }
        }
        if !self.water_values.is_empty() {
            let values = llsd_map_of(&self.water_values);
            let mixes = mixes_of(&self.water_values);
            for frame in settings.day_cycle.water_frames.values_mut() {
                *frame = water_with_blended_values(frame, &values, &mixes);
            }
        }
    }
}

impl PushedValue {
    /// Whether this key is on its way back out to the value underneath.
    fn is_fading_out(&self) -> bool {
        self.blend.is_some_and(|blend| blend.fading_out)
    }

    /// Whether this key has finished fading out and can be forgotten.
    fn is_finished_fading_out(&self) -> bool {
        self.blend
            .is_some_and(|blend| blend.fading_out && blend.remaining <= 0.0)
    }
}

/// Whether `track` is held by `experience` — one line, but written once because
/// the three whole-asset tracks hold three different settings types and a
/// closure would be monomorphised to whichever it was first called with.
fn held_by<T>(track: Option<&(ExperienceKey, T)>, experience: ExperienceKey) -> bool {
    track.is_some_and(|(owner, _)| *owner == experience)
}

/// Record each key of `values` as owned by `experience`, replacing whatever
/// experience owned it before — the reference's `injectSetting`, which schedules
/// a per-key blend above its `0.1` second threshold and writes straight through
/// below it. A `values` that is not a map contributes nothing.
fn overlay_pushed_values(
    target: &mut BTreeMap<String, PushedValue>,
    experience: ExperienceKey,
    values: Option<&Llsd>,
    transition: f32,
) {
    let Some(Llsd::Map(entries)) = values else {
        return;
    };
    let blend = (transition > INSTANT_TRANSITION).then_some(ValueBlend {
        remaining: transition,
        duration: transition,
        fading_out: false,
    });
    for (key, value) in entries {
        drop(target.insert(
            key.clone(),
            PushedValue {
                experience,
                value: value.clone(),
                blend,
            },
        ));
    }
}

/// Start every key `experience` (or, for `None`, anybody) owns fading back out
/// over `transition`, or drop them outright at an instant one — the reference's
/// `removeInjection`.
///
/// A key already fading out is left alone: its fade is the release, and
/// restarting it would make the second of two clears take longer than the first.
fn release_pushed_values(
    target: &mut BTreeMap<String, PushedValue>,
    experience: Option<ExperienceKey>,
    transition: f32,
) {
    let owned = |pushed: &PushedValue| experience.is_none_or(|id| pushed.experience == id);
    if transition <= INSTANT_TRANSITION {
        target.retain(|_, pushed| !owned(pushed));
        return;
    }
    for pushed in target.values_mut() {
        if !owned(pushed) || pushed.is_fading_out() {
            continue;
        }
        // The value it fades out *from* is the one on screen now, which for a
        // key still fading in is only part of the way to what was pushed.
        pushed.blend = Some(ValueBlend {
            remaining: transition,
            duration: transition,
            fading_out: true,
        });
    }
}

/// The owned per-key overrides as the plain LLSD map the settings overlay takes.
fn llsd_map_of(values: &BTreeMap<String, PushedValue>) -> Llsd {
    Llsd::Map(
        values
            .iter()
            .map(|(key, pushed)| (key.clone(), pushed.value.clone()))
            .collect(),
    )
}

/// The per-key mixes of whichever keys are mid-blend — the overlay assigns every
/// other key whole.
fn mixes_of(values: &BTreeMap<String, PushedValue>) -> BTreeMap<String, f32> {
    values
        .iter()
        .filter_map(|(key, pushed)| Some((key.clone(), pushed.blend?.mix())))
        .collect()
}

/// Where the current [`EnvironmentState::settings`] came from — and, as
/// [`EnvironmentSource::of_reply`], the scope an incoming reply describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvironmentSource {
    /// The built-in legacy WindLight default — no grid settings ingested yet.
    Default,
    /// The whole-region environment (a `parcel_id` of `-1`).
    Region,
    /// A specific parcel's environment override. Never the source of
    /// [`EnvironmentState::shared`]: a parcel override lives in its own layer,
    /// above (not instead of) the region's — see
    /// [`EnvironmentState::ingest_parcel`].
    Parcel,
}

impl EnvironmentSource {
    /// The scope a reply's [`EnvironmentSettings::parcel_id`] names: the
    /// whole region for the `-1` sentinel
    /// (`LLEnvironment::INVALID_PARCEL_ID`), a specific parcel's override for
    /// any real parcel id — including `0`, which is a parcel like any other.
    const fn of_reply(parcel_id: i32) -> Self {
        if parcel_id < 0 {
            Self::Region
        } else {
            Self::Parcel
        }
    }
}

/// A transition time at or below which a change is an instant cut rather than a
/// fade — the reference's own threshold in `LLSettingsInjected::injectSetting`,
/// which writes a value straight through under `0.1` seconds instead of
/// scheduling a blend.
const INSTANT_TRANSITION: f32 = 0.1;

/// How long the release of an experience the agent has walked away from
/// cross-fades for, in seconds — the reference's `TRANSITION_FAST`, which is
/// what `DayInjection::testExperiencesOnParcelCoro` passes to `clearInjections`
/// for an experience the new parcel does not admit.
const FAST_TRANSITION: f32 = 1.0;

/// The reference's `TRANSITION_DEFAULT`, in seconds — what it cross-fades over
/// when it drops every injected environment at a region change
/// (`LLEnvironment::onRegionChange`).
const DEFAULT_TRANSITION: f32 = 5.0;

/// How many times to (re)request the region environment before giving up and
/// rendering with the legacy WindLight defaults.
const MAX_ENV_ATTEMPTS: u32 = 12;

/// Seconds between environment-request retries while a request is outstanding.
const ENV_RETRY_INTERVAL: f32 = 3.0;

/// The settings section the environment's own knobs live under.
pub const ENVIRONMENT_SECTION: &[&str] = &["environment"];

/// How long a *manual* environment change cross-fades for, in seconds — the
/// reference's `FSEnvironmentManualTransitionTime`. `0.0` (the default) is an
/// instant cut, which is what the viewer did before there was a knob.
pub const SETTING_TRANSITION_TIME: &str = "EnvironmentManualTransitionTime";

/// Whether the personal (local) environment is restored at the next login —
/// the reference's `EnvironmentPersistAcrossLogin`.
pub const SETTING_PERSIST_ACROSS_LOGIN: &str = "EnvironmentPersistAcrossLogin";

/// Whether picking the environment preset that is *already* pinned reverts to
/// the shared environment — the reference's `FSRepeatedEnvTogglesShared`, which
/// makes each World ▸ Environment entry (and its shortcut) a toggle rather than
/// a one-way pin.
pub const SETTING_REPEATED_TOGGLES_SHARED: &str = "EnvironmentRepeatedTogglesShared";

/// Where the saved personal environment is kept: one account-scoped setting
/// holding [`SavedEnvironment`] as JSON.
///
/// Hidden from the raw debug-settings editor because it is a serialised blob,
/// not a knob — the same treatment window geometry and table sort orders get.
const SETTING_SAVED_ENVIRONMENT: &str = "SavedPersonalEnvironment";

/// Declare the environment's own settings: the two lifecycle toggles, the
/// manual transition time, and the hidden slot the personal environment is
/// saved in.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        ENVIRONMENT_SECTION,
        SETTING_TRANSITION_TIME,
        SettingValue::F32(0.0),
        "Seconds to blend between sky/water settings when the environment is changed by hand. \
         0 is immediate.",
    );
    settings.register_in(
        ENVIRONMENT_SECTION,
        SETTING_PERSIST_ACROSS_LOGIN,
        SettingValue::Bool(true),
        "Restore the personal (local) environment at the next login.",
    );
    settings.register_in(
        ENVIRONMENT_SECTION,
        SETTING_REPEATED_TOGGLES_SHARED,
        SettingValue::Bool(false),
        "Picking the environment preset that is already pinned reverts to the shared \
         (region) environment.",
    );
    settings.register_hidden_in(
        ENVIRONMENT_SECTION,
        SETTING_SAVED_ENVIRONMENT,
        SettingValue::String(String::new()),
        "The personal environment saved for this account, as JSON. Written by the viewer.",
    );
}

/// Whether the environment preset that is already pinned reverts to the shared
/// environment when picked again ([`SETTING_REPEATED_TOGGLES_SHARED`]).
#[must_use]
pub fn repeated_toggles_shared(settings: Option<&ViewerSettings>) -> bool {
    settings.is_some_and(|settings| {
        settings
            .store()
            .get_bool(SETTING_REPEATED_TOGGLES_SHARED)
            .unwrap_or(false)
    })
}

/// The personal environment as it is written to the account: the menu's pin and
/// the three local tracks, which together are everything "Use Shared
/// Environment" would throw away.
///
/// The *shared* environment is deliberately not part of it — that is the grid's
/// to send, and a region may well have changed its own since the last session.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct SavedEnvironment {
    /// The World ▸ Environment pin, if one was in force.
    fixed: Option<FixedEnvironment>,
    /// The local layer's three tracks.
    local: LocalEnvironment,
}

/// A parcel-environment request in flight: which parcel it asks about, and the
/// retry clock that keeps asking until the grid answers.
///
/// Separate from the region's request because both can be outstanding at once —
/// walking across a parcel line during a region handshake does exactly that —
/// and a reply to one must not be taken for a reply to the other.
#[derive(Debug, Clone, Copy)]
struct ParcelRequest {
    /// The parcel being asked about, as the grid numbers them in this region.
    parcel_id: i32,
    /// How many attempts have gone out for this parcel.
    attempts: u32,
    /// The earliest time (`Time::elapsed_secs`) the next retry may fire.
    next_retry_at: f32,
}

/// A cross-fade from the environment that was rendering to the one just
/// selected — the reference's `LLSettingsBlender`, driven by
/// [`SETTING_TRANSITION_TIME`].
///
/// Only *manual* changes start one. A grid reply is not a transition: the
/// region's own environment arriving (or changing under the agent) is not
/// something the user did, and the reference blends those on its own schedule.
#[derive(Debug, Clone)]
struct EnvironmentTransition {
    /// The environment that was on screen when the change was made.
    from: Box<EnvironmentSettings>,
    /// Seconds elapsed since the change.
    elapsed: f32,
    /// Seconds the fade runs for; always `> 0.0` (a zero-length fade is no
    /// transition at all and is never recorded).
    duration: f32,
}

impl EnvironmentTransition {
    /// How far along the fade is, `0.0..=1.0`.
    fn fraction(&self) -> f32 {
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }
}

/// What a pinned day position selected against the environment in force.
///
/// A capture harness asked to photograph the scene at a particular time of day
/// has to be able to say whether it got one: a position accepted against a cycle
/// that cannot be sampled changes nothing, and the frames are then of whatever
/// sky the region already had, under a filename that says otherwise. This is the
/// answer, recorded whenever the environment is composed, so the run's status
/// file can carry it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DayPositionPin {
    /// No position is pinned; the sky follows the region clock.
    NotPinned,
    /// The pinned position samples the region's own day cycle, which schedules
    /// more than one sky. This is the only outcome under which two viewers
    /// pointed at one region are looking at the same sky.
    RegionDayCycle {
        /// The pinned position, `0.0..=1.0`.
        position: f32,
    },
    /// The region's cycle schedules one sky, so the four legacy WindLight
    /// presets were installed over it and the position samples *those*. The sun
    /// moves, but the sky is no longer the sky the region sent.
    SubstitutedPresets {
        /// The pinned position, `0.0..=1.0`.
        position: f32,
    },
    /// A fixed sky (World ▸ Environment) is selected, which names one frame
    /// outright; the pinned position is not consulted at all.
    OverriddenByFixedSky {
        /// The pinned position, `0.0..=1.0`.
        position: f32,
    },
}

impl DayPositionPin {
    /// The position that was asked for, if one was.
    #[must_use]
    pub const fn position(self) -> Option<f32> {
        match self {
            Self::NotPinned => None,
            Self::RegionDayCycle { position }
            | Self::SubstitutedPresets { position }
            | Self::OverriddenByFixedSky { position } => Some(position),
        }
    }

    /// Whether the sky on screen is the one the *region* serves at the pinned
    /// position — false as soon as anything stood in for it, which is what makes
    /// a cross-check capture comparable or not.
    #[must_use]
    pub const fn is_the_regions_own_sky(self) -> bool {
        matches!(self, Self::NotPinned | Self::RegionDayCycle { .. })
    }

    /// One line for a run's status file, or `None` when nothing was pinned.
    #[must_use]
    pub fn describe(self) -> Option<String> {
        match self {
            Self::NotPinned => None,
            Self::RegionDayCycle { position } => {
                Some(format!("sampled the region's day cycle at {position}"))
            }
            Self::SubstitutedPresets { position } => Some(format!(
                "the region's day cycle schedules one sky, so the legacy WindLight presets were \
                 installed over it and {position} samples those — not the region's sky"
            )),
            Self::OverriddenByFixedSky { position } => Some(format!(
                "a fixed sky is selected, which names one frame outright, so the pinned {position} \
                 selected nothing"
            )),
        }
    }
}

/// The viewer's current environment: the sky / water / day-cycle settings the
/// later rendering phases draw from, plus where they came from.
#[derive(Debug, Resource)]
pub struct EnvironmentState {
    /// The active environment settings — what the sky / water / shadow phases
    /// render. Begins at the legacy WindLight default, is replaced when the
    /// grid answers a [`Command::RequestEnvironment`], and is *pinned* to a
    /// single preset frame while a fixed environment is selected
    /// ([`set_fixed`](Self::set_fixed)).
    pub settings: EnvironmentSettings,
    /// A day position (`0.0..=1.0`) pinned instead of the clock — the
    /// `SL_VIEWER_SKY_DAY_POSITION` override, carried here because every sky,
    /// water, terrain and fog driver asks `crate::sky::day_position` of this
    /// state and none of them should have to know where the pin came from.
    pub pinned_day_position: Option<f32>,
    /// What [`Self::pinned_day_position`] selected the last time the environment
    /// was composed — read by the capture harness, which has to report a pin it
    /// could not honour rather than hand back frames that quietly ignore it.
    day_position_pin: DayPositionPin,
    /// The provenance of [`Self::settings`].
    pub(crate) source: EnvironmentSource,
    /// The last **shared** (grid) environment: what [`Self::settings`] shows
    /// when no fixed sky is selected, and what "Use Shared Environment"
    /// restores. Kept current by [`ingest_environment`] even while a fixed sky
    /// is pinned, so un-pinning never renders stale grid settings.
    shared: EnvironmentSettings,
    /// The provenance of [`Self::shared`].
    shared_source: EnvironmentSource,
    /// The environment pinned by the World ▸ Environment menu, if any — the
    /// reference viewer's local fixed environment
    /// (`LLEnvironment::setEnvironment(ENV_LOCAL, …)`), which survives region
    /// changes until "Use Shared Environment". One of three groups (Day Cycle,
    /// Legacy, Modern) at a time of day.
    fixed: Option<FixedEnvironment>,
    /// The local environment layer: the settings assets a script (`@setenv_*`)
    /// or a panel (the quick-preferences preset combos) has installed, one per
    /// track.
    ///
    /// Its **sky** track is the same `ENV_LOCAL` sky [`Self::fixed`] is, so
    /// those two are mutually exclusive and the last writer wins:
    /// [`set_fixed`](Self::set_fixed) drops a local sky, and installing one
    /// drops the menu's pin. The menu's pin is kept as its own field only
    /// because the menu's check marks ask which *preset* is pinned, which an
    /// arbitrary sky asset is not an answer to. The water and day tracks are
    /// nobody else's, so a pinned preset leaves them alone.
    local: LocalEnvironment,
    /// The **pushed** layer: what experiences have injected over the region's
    /// settings, above the parcel's and below the local one. See
    /// [`PushedEnvironment`].
    push: PushedEnvironment,
    /// The decoded sky for a pinned **Modern** selection, once its `KNOWN_SKY_*`
    /// asset resolves (see [`resolve_modern_environment`]), keyed by the time so a
    /// stale one is ignored after the selection changes. Until it resolves, a
    /// Modern selection renders the region's cycle at that time as a placeholder.
    modern_sky: Option<(FixedSky, SkySettings)>,
    /// The **parcel** the agent stands on has its own environment — the
    /// reference's `ENV_PARCEL`, which sits above the region's and below the
    /// local one (`LLEnvironment::recordEnvironment`, the `mParcelId !=
    /// INVALID_PARCEL_ID` branch). `None` when the parcel has no override, which
    /// is the common case and what most of a region is.
    ///
    /// Only the day cycle and its length / offset come from here. Track
    /// altitudes stay the region's: the reference assigns `mTrackAltitudes` in
    /// the *region* branch only, so a parcel cannot move the sky-track bands.
    parcel: Option<Box<EnvironmentSettings>>,
    /// The parcel the agent is standing on, as the grid numbers them within this
    /// region, or `None` before one is known. What an arriving parcel reply is
    /// matched against — a reply for a parcel the agent has since walked off is
    /// not this parcel's environment.
    parcel_id: Option<i32>,
    /// The environment version last seen for [`Self::parcel_id`] — the parcel's
    /// `parcel_environment_version`. A change to it means somebody edited that
    /// parcel's environment while the agent stood on it, which the reference
    /// re-requests on.
    parcel_env_version: i32,
    /// The parcel-environment request outstanding, if any.
    parcel_req: Option<ParcelRequest>,
    /// The parcel the agent is **standing** on, whatever its environment
    /// version says — the reference's `LLViewerParcelMgr::getAgentParcel()
    /// ->getLocalID()`.
    ///
    /// Not [`Self::parcel_id`], which is the parcel whose *environment* is being
    /// tracked and is `None` wherever a region has per-parcel overrides switched
    /// off. Where an experience may keep its sky is a question about land, not
    /// about environment versions, so it has to be answered on a region that
    /// serves no parcel environments at all.
    standing_parcel: Option<i32>,
    /// A parcel the agent has just stepped onto that the injecting experiences
    /// still have to be tested against, if the query has not gone out yet — the
    /// arming half of [`query_parcel_experiences`].
    experience_recheck: Option<i32>,
    /// Seconds a *manual* environment change cross-fades over, mirrored from
    /// [`SETTING_TRANSITION_TIME`] by [`sync_environment_settings`]. Zero — the
    /// declared default — is an instant cut.
    ///
    /// Held here rather than read at the point of use because the transition has
    /// to start in [`set_fixed`](Self::set_fixed) and friends, which a floater or
    /// a menu calls with nothing but `&mut EnvironmentState` in hand.
    pub manual_transition_seconds: f32,
    /// The cross-fade in flight, if any.
    transition: Option<EnvironmentTransition>,
    /// The **edit** layer: what an open settings editor is previewing, over
    /// every other layer — the reference's `ENV_EDIT`
    /// (`LLFloaterFixedEnvironment::onOpen` installs the settings it is editing
    /// there and `onClose` clears them again).
    ///
    /// Its own layer rather than a write into [`Self::local`] because an editor
    /// is a *preview*: whatever personal environment the user had before they
    /// opened one has to still be there when they close it, unsaved and
    /// untouched. Snapshotting the local layer on open and putting it back on
    /// close reaches the same place only while nothing else writes that layer
    /// in between — a script's `@setenv_*`, or the Personal Lighting window
    /// left open beside the editor, both do.
    ///
    /// Never persisted: [`saved_environment`](Self::saved_environment) ignores
    /// it, because a frame somebody was looking at in an editor is not a
    /// personal environment they chose.
    edit: LocalEnvironment,
    /// Whether the account's saved personal environment has been restored yet
    /// (once per session, after the account settings load — see
    /// [`restore_saved_environment`]).
    restored: bool,
    /// Whether a region-environment request is still outstanding — the retry loop
    /// keeps re-requesting until the reply is ingested or `MAX_ENV_ATTEMPTS` is
    /// reached.
    req_pending: bool,
    /// How many `RequestEnvironment` attempts have been made in the current cycle.
    req_attempts: u32,
    /// The earliest time (`Time::elapsed_secs`) the next retry may fire.
    req_next_retry_at: f32,
}

impl Default for EnvironmentState {
    fn default() -> Self {
        Self {
            settings: EnvironmentSettings::legacy_windlight_default(),
            pinned_day_position: None,
            day_position_pin: DayPositionPin::NotPinned,
            source: EnvironmentSource::Default,
            shared: EnvironmentSettings::legacy_windlight_default(),
            shared_source: EnvironmentSource::Default,
            fixed: None,
            local: LocalEnvironment::default(),
            push: PushedEnvironment::default(),
            modern_sky: None,
            parcel: None,
            parcel_id: None,
            parcel_env_version: -1,
            parcel_req: None,
            standing_parcel: None,
            experience_recheck: None,
            manual_transition_seconds: 0.0,
            transition: None,
            edit: LocalEnvironment::default(),
            restored: false,
            req_pending: false,
            req_attempts: 0,
            req_next_retry_at: 0.0,
        }
    }
}

impl EnvironmentState {
    /// The default state with the day position the overrides pin, if any.
    #[must_use]
    pub fn from_overrides(overrides: &crate::render_overrides::RenderOverrides) -> Self {
        let mut state = Self {
            pinned_day_position: overrides.day_position,
            ..Self::default()
        };
        // Composed once here rather than waiting for the first environment to
        // arrive: a run that never reaches a region must still report the pin it
        // was given, and the built-in default it starts from is a single-frame
        // cycle like any other.
        state.apply();
        state
    }

    /// The environment currently pinned by the World ▸ Environment menu, if any
    /// (drives the menu's check marks).
    #[must_use]
    pub const fn fixed(&self) -> Option<FixedEnvironment> {
        self.fixed
    }

    /// Pin the rendered environment to `fixed` — a single-frame day cycle holding
    /// the selected sky over the shared environment's water — or restore the
    /// shared (grid) environment with `None`. The reference's World ▸ Environment
    /// local fixed sky (`setEnvironment(ENV_LOCAL, …)`).
    pub fn set_fixed(&mut self, fixed: Option<FixedEnvironment>) {
        self.begin_transition();
        self.fixed = fixed;
        // "Use Shared Environment" (`None`) is the reference's
        // `setSharedEnvironment`: the whole local layer goes, tracks a script
        // installed included. Pinning a preset takes only the *sky* track back —
        // it is the one thing a pin and a local sky both are.
        match fixed {
            None => self.local = LocalEnvironment::default(),
            Some(_) => self.local.sky = None,
        }
        // A selection change invalidates any resolved Modern sky (a fresh Modern
        // selection re-resolves; a non-Modern selection drops it).
        if !matches!(fixed, Some(FixedEnvironment::Modern(_))) {
            self.modern_sky = None;
        }
        self.apply();
    }

    /// Tell the environment which parcel the agent is standing on, and what that
    /// parcel's environment version is — the reference's
    /// `LLEnvironment::onParcelChange` plus the `environment_changed` re-request
    /// `LLViewerParcelMgr` makes when a parcel's own version moves under a
    /// standing agent.
    ///
    /// Stepping over a parcel line **drops the old parcel's override at once**
    /// rather than keeping it until the new parcel answers. The alternative is a
    /// sky belonging to the parcel behind you for as long as the round trip
    /// takes, which is the one wrong answer available here: the region's
    /// environment is always a defensible thing to draw, and another parcel's
    /// never is.
    pub fn set_agent_parcel(&mut self, parcel_id: Option<i32>, env_version: i32) {
        if self.parcel_id == parcel_id && self.parcel_env_version == env_version {
            return;
        }
        let moved = self.parcel_id != parcel_id;
        self.parcel_id = parcel_id;
        self.parcel_env_version = env_version;
        if moved {
            self.parcel = None;
        }
        self.parcel_req = parcel_id.map(|parcel_id| ParcelRequest {
            parcel_id,
            attempts: 0,
            next_retry_at: 0.0,
        });
        if moved {
            self.apply();
        }
    }

    /// Mirror the parcel the agent is **standing** on, and arm the experience
    /// re-check when that is a different parcel from the last one.
    ///
    /// An experience is admitted per land, so stepping over a parcel line is the
    /// moment its injected sky may stop being allowed. The reference hooks
    /// exactly this signal from inside the injection itself
    /// (`DayInjection::onParcelChange` → `testExperiencesOnParcel`), which is why
    /// nothing is asked while no experience is injecting: there is no injection
    /// to hold the hook.
    pub fn set_standing_parcel(&mut self, parcel_id: Option<i32>) {
        if self.standing_parcel == parcel_id {
            return;
        }
        self.standing_parcel = parcel_id;
        // A parcel the agent has *left* (no parcel known any more) is nothing to
        // ask a question about; the next one the agent steps onto asks it.
        if let Some(parcel_id) = parcel_id
            && !self.push.is_empty()
        {
            self.experience_recheck = Some(parcel_id);
        }
    }

    /// The parcel the agent is standing on, as the region numbers them — what an
    /// arriving [`Event::ParcelExperiences`](sl_client_bevy::SlSessionEvent::ParcelExperiences)
    /// is matched against.
    #[must_use]
    pub const fn standing_parcel(&self) -> Option<i32> {
        self.standing_parcel
    }

    /// Release everything `experience` has injected, over the reference's
    /// `TRANSITION_FAST` — what an experience the parcel does not admit gets.
    ///
    /// A no-op for an experience holding nothing, so an `ExperienceQuery` reply
    /// listing several refusals starts one cross-fade per experience that
    /// actually had a sky here, and none at all for the common answer where every
    /// injecting experience is still allowed.
    pub fn release_pushed_experience(&mut self, experience: ExperienceKey) {
        if !self.push.experiences().contains(&experience) {
            return;
        }
        if self.push.holds_a_track(Some(experience)) {
            self.begin_transition_over(FAST_TRANSITION);
        }
        self.push.clear(Some(experience), FAST_TRANSITION);
        self.apply();
    }

    /// The parcel environment in force, if the parcel the agent stands on has
    /// one — what a surface asking "is this parcel's sky its own" reads.
    #[must_use]
    pub fn parcel_environment(&self) -> Option<&EnvironmentSettings> {
        self.parcel.as_deref()
    }

    /// The local environment layer — what a script and the preset combos have
    /// installed, per track.
    #[must_use]
    pub const fn local(&self) -> &LocalEnvironment {
        &self.local
    }

    /// Install one settings asset in the local layer's matching track — the RLV
    /// `@setenv_*` family's write target and the quick-preferences preset
    /// combos'. `source` is the inventory asset it came from, where a surface
    /// knows one, so a list of presets can show which row is in force.
    ///
    /// A **sky** takes the local sky track back from the menu's pin, for the
    /// same reason [`set_fixed`](Self::set_fixed) takes it back from a script:
    /// the reference has one `ENV_LOCAL` sky, not two. A day cycle does the
    /// same, since it animates the sky it replaces. A water frame is nobody
    /// else's track and leaves the pin standing.
    pub fn set_local(&mut self, local: EnvironmentAsset, source: Option<Uuid>) {
        self.begin_transition();
        self.install_local(local, source);
    }

    /// [`set_local`](Self::set_local) with **no** cross-fade, whatever the manual
    /// transition time says — the reference's `TRANSITION_INSTANT`.
    ///
    /// This is what a live editor writes through. A fade is for *arriving* at an
    /// environment somebody chose; a dragged slider is not an arrival, and
    /// starting a fresh fade on every pixel of a drag would smear the preview a
    /// frame or two behind the hand moving it, which is precisely the feedback
    /// the editor exists to give.
    pub fn set_local_instant(&mut self, local: EnvironmentAsset, source: Option<Uuid>) {
        self.transition = None;
        self.install_local(local, source);
    }

    /// The shared body of the two: install the asset in its track and recompose.
    fn install_local(&mut self, local: EnvironmentAsset, source: Option<Uuid>) {
        if !matches!(local, EnvironmentAsset::Water(_)) {
            self.fixed = None;
            self.modern_sky = None;
        }
        self.local.install(local, source);
        self.apply();
    }

    /// Install one settings asset in the **edit** layer — the frame an open
    /// settings editor is previewing, over every other layer (the reference's
    /// `ENV_EDIT`).
    ///
    /// Never fades. An editor writes this on every drag of every slider, and a
    /// fade restarted per pixel would run the preview a beat behind the hand
    /// moving it — the same reason [`set_local_instant`](Self::set_local_instant)
    /// exists.
    pub fn set_edit(&mut self, edit: EnvironmentAsset) {
        self.transition = None;
        self.edit.install(edit, None);
        self.apply();
    }

    /// Take one track out of the edit layer — an editor closing, putting back
    /// whatever the layers underneath were holding all along.
    ///
    /// Per track, not the whole layer: the sky editor and the water editor are
    /// separate windows and either can be open without the other.
    pub fn clear_edit(&mut self, kind: SettingsKind) {
        match kind {
            SettingsKind::Sky => self.edit.sky = None,
            SettingsKind::Water => self.edit.water = None,
            SettingsKind::DayCycle => self.edit.day = None,
        }
        self.apply();
    }

    /// The edit layer — what an open editor is previewing, if anything.
    #[must_use]
    pub const fn editing(&self) -> &LocalEnvironment {
        &self.edit
    }

    /// The pushed layer — what experiences have injected, if anything.
    #[must_use]
    pub const fn pushed(&self) -> &PushedEnvironment {
        &self.push
    }

    /// Carry out one `PushExpEnvironment` whose settings are already in hand:
    /// [`Clear`](EnvironmentPushAction::Clear) releases the experience,
    /// [`Partial`](EnvironmentPushAction::Partial) overlays its keys, and
    /// [`Full`](EnvironmentPushAction::Full) installs `asset` — which the caller
    /// has fetched by the id the push named, and passes as `None` while it is
    /// still in flight or turned out not to be settings at all.
    ///
    /// The fade is the **push's** transition time, not the viewer's manual one:
    /// an experience states how long its change should take, and that number is
    /// as much a part of the push as the sky is.
    ///
    /// *Which* fade depends on what the push does, exactly as it does in the
    /// reference. A whole settings asset going in or a whole track coming out is
    /// a whole-environment cross-fade (`animateSkyChange` /
    /// `animateWaterChange`); a partial push and its release are **per key**
    /// (`injectSetting` / `removeInjection`), leaving every key the push does not
    /// name at the value it already had for the whole of the transition.
    pub fn apply_environment_push(
        &mut self,
        push: &ExperienceEnvironmentPush,
        asset: Option<EnvironmentAsset>,
    ) {
        match &push.action {
            EnvironmentPushAction::Clear => {
                // A nil experience id is the reference's "every experience".
                let experience =
                    (!push.experience_id.uuid().is_nil()).then_some(push.experience_id);
                if self.push.holds_a_track(experience) {
                    self.begin_transition_over(push.transition_time);
                }
                self.push.clear(experience, push.transition_time);
            }
            EnvironmentPushAction::Partial { sky, water } => {
                self.push.inject(
                    push.experience_id,
                    sky.as_ref(),
                    water.as_ref(),
                    push.transition_time,
                );
            }
            EnvironmentPushAction::Full { asset_id } => {
                let Some(asset) = asset else {
                    debug!(
                        "experience {} pushed settings asset {asset_id}, which is not in hand yet",
                        push.experience_id
                    );
                    return;
                };
                self.begin_transition_over(push.transition_time);
                self.push.install(push.experience_id, asset);
            }
        }
        self.apply();
    }

    /// Drop everything the pushed layer holds — every experience at once, as a
    /// `ClearEnvironment` with a nil experience id does — cross-faded over
    /// `transition_seconds`.
    ///
    /// A **region change** is the other caller: the reference drops every
    /// injected environment at one, over its `TRANSITION_DEFAULT`
    /// (`LLEnvironment::onRegionChange`, whose "for now environmental
    /// experiences do not survive region crossings" is unconditional — the
    /// capability test beside it is commented out). Without that a script that
    /// pushed a sky and then lost the agent to a teleport keeps it forever:
    /// nothing in the destination knows who is holding the sky, and the
    /// per-parcel re-check ([`set_standing_parcel`](Self::set_standing_parcel))
    /// only fires where the destination serves an `ExperienceQuery`.
    pub fn clear_pushed(&mut self, transition_seconds: f32) {
        if self.push.is_empty() {
            return;
        }
        self.begin_transition_over(transition_seconds);
        // The whole layer goes at once here, fade and all: a region crossing
        // replaces the environment underneath as well, so there is nothing for a
        // per-key fade-out to walk back to.
        self.push = PushedEnvironment::default();
        self.apply();
    }

    /// Empty the local layer, falling back to whatever the menu has pinned
    /// (nothing, usually) and then to the shared environment.
    pub fn clear_local(&mut self) {
        self.begin_transition();
        self.local = LocalEnvironment::default();
        self.apply();
    }

    /// The sky to render at `altitude` and day `position` — [`Self::settings`]
    /// sampled, cross-faded with whatever was on screen while a manual change is
    /// still fading in ([`SETTING_TRANSITION_TIME`]).
    ///
    /// Every renderer asks this rather than sampling `settings` itself, because
    /// the fade is exactly the difference between "the settings in force" and
    /// "what the frame should draw", and only one of those is a field.
    #[must_use]
    pub fn sky_at(&self, altitude: f32, position: f32) -> Option<SkySettings> {
        let target = self.settings.blended_sky_settings(altitude, position);
        let Some(transition) = &self.transition else {
            return target;
        };
        match (
            transition.from.blended_sky_settings(altitude, position),
            target,
        ) {
            (Some(from), Some(target)) => Some(from.blend(&target, transition.fraction())),
            // Nothing to fade from (or to): the destination is the answer.
            (_, target) => target,
        }
    }

    /// The water to render at day `position`, cross-faded like [`Self::sky_at`].
    #[must_use]
    pub fn water_at(&self, position: f32) -> Option<WaterSettings> {
        let target = self.settings.blended_water_settings(position);
        let Some(transition) = &self.transition else {
            return target;
        };
        match (transition.from.blended_water_settings(position), target) {
            (Some(from), Some(target)) => Some(from.blend(&target, transition.fraction())),
            (_, target) => target,
        }
    }

    /// Whether a manual cross-fade is still running — what
    /// [`advance_environment_transition`] ticks, and what a test asserts on.
    #[must_use]
    pub const fn is_transitioning(&self) -> bool {
        self.transition.is_some()
    }

    /// Start a cross-fade from what is on screen now, to be completed by
    /// whichever change is about to be applied. A no-op while the transition
    /// time is zero, which is the declared default.
    fn begin_transition(&mut self) {
        if self.manual_transition_seconds <= 0.0 {
            self.transition = None;
            return;
        }
        let from = Box::new(self.displayed_environment());
        self.transition = Some(EnvironmentTransition {
            from,
            elapsed: 0.0,
            duration: self.manual_transition_seconds,
        });
    }

    /// Start a cross-fade of `seconds`, whatever the manual transition time
    /// says — what a push that states its own transition time gets.
    ///
    /// Anything at or under [`INSTANT_TRANSITION`] is no fade at all, matching
    /// the reference's own threshold (`injectSetting` writes the value straight
    /// through rather than blending it).
    fn begin_transition_over(&mut self, seconds: f32) {
        if seconds <= INSTANT_TRANSITION {
            self.transition = None;
            return;
        }
        let from = Box::new(self.displayed_environment());
        self.transition = Some(EnvironmentTransition {
            from,
            elapsed: 0.0,
            duration: seconds,
        });
    }

    /// Advance a running fade by `delta` seconds, ending it once it is done.
    fn advance_transition(&mut self, delta: f32) {
        let Some(transition) = &mut self.transition else {
            return;
        };
        transition.elapsed += delta;
        if transition.elapsed >= transition.duration {
            self.transition = None;
        }
    }

    /// What is being drawn right now: [`Self::settings`] as it stands, or — when
    /// a fade is still running — a single-frame environment holding the frames
    /// that fade has *reached*.
    ///
    /// Changing the environment twice in quick succession is the case this
    /// exists for: taking `settings` as the new starting point would snap the
    /// sky to the first change's destination before fading away from it, which
    /// is a visible jump in the one place a fade was asked for. Pinning the
    /// reached frames costs the altitude tracks for the length of the second
    /// fade, which is the cheaper of the two artefacts by a wide margin.
    fn displayed_environment(&self) -> EnvironmentSettings {
        let mut displayed = self.settings.clone();
        if self.transition.is_none() {
            return displayed;
        }
        let position = crate::sky::day_position(self);
        if let Some(sky) = self.sky_at(0.0, position) {
            let name = sky.name.clone();
            pin_sky_into(&mut displayed, sky, name);
        }
        if let Some(water) = self.water_at(position) {
            let name = water.name.clone();
            pin_water_into(&mut displayed, water, name);
        }
        displayed
    }

    /// Whether a **fixed sky** is in force locally rather than a running cycle —
    /// what `@getenv_daytime` reports, and the question the reference asks as
    /// `getEnvironmentFixedSky(ENV_LOCAL)`.
    ///
    /// A local *day cycle* is not one, and neither is a local water frame: both
    /// leave the sky animating, which is exactly the case the reference answers
    /// `-1` for.
    #[must_use]
    pub const fn has_local_fixed_sky(&self) -> bool {
        self.fixed.is_some() || self.local.sky.is_some()
    }

    /// The shared day cycle sampled at `position` (`0.0..=1.0`) — the sky
    /// `@setenv_daytime` pins.
    ///
    /// The reference builds it from the nearest running cycle (local, then
    /// pushed, parcel, region); the local layer here is always a single frame,
    /// so the shared cycle is the nearest one that *has* a position to sample.
    /// A shared environment with no sky at all falls back to the legacy midday
    /// preset, as the menu's own Day Cycle group does.
    #[must_use]
    pub fn shared_sky_at(&self, position: f32) -> SkySettings {
        self.shared
            .blended_sky_settings(0.0, position)
            .unwrap_or_else(|| FixedSky::Midday.settings())
    }

    /// The sky being rendered right now at ground level — what a `@getenv_*`
    /// read answers from.
    #[must_use]
    pub fn rendered_sky(&self) -> Option<SkySettings> {
        self.settings
            .blended_sky_settings(0.0, crate::sky::day_position(self))
    }

    /// Record the decoded sky for a resolved **Modern** selection and re-apply,
    /// swapping the region-cycle placeholder for the real `KNOWN_SKY_*` sky.
    /// Called by [`resolve_modern_environment`] once the asset arrives.
    pub(crate) fn set_modern_sky(&mut self, time: FixedSky, sky: SkySettings) {
        self.modern_sky = Some((time, sky));
        self.apply();
    }

    /// Fold a freshly-ingested shared environment in: it becomes the rendered
    /// settings unless a fixed sky is pinned (in which case it is remembered
    /// for the next "Use Shared Environment").
    fn ingest_shared(&mut self, settings: EnvironmentSettings, source: EnvironmentSource) {
        self.shared = settings;
        self.shared_source = source;
        self.apply();
    }

    /// Fold one grid environment reply in, and report the scope it described.
    ///
    /// Only a **region**-scoped reply is the shared environment: it becomes
    /// [`Self::shared`] — what renders, and what "Use Shared Environment"
    /// restores — and satisfies the outstanding [`Command::RequestEnvironment`],
    /// ending the retry loop.
    ///
    /// A **parcel**-scoped reply is an override of the region's settings for one
    /// parcel, not the region's settings, and goes to its own layer
    /// ([`Self::ingest_parcel`]) — the reference's `ENV_PARCEL`, which sits
    /// *above* `ENV_REGION` and never touches it
    /// (`LLEnvironment::recordEnvironment`). Treating one as the shared
    /// environment would make a parcel override what "Use Shared Environment"
    /// restores, and would cancel the region retry loop before the region's own
    /// settings ever arrived.
    fn ingest_reply(&mut self, settings: EnvironmentSettings) -> EnvironmentSource {
        let source = EnvironmentSource::of_reply(settings.parcel_id);
        match source {
            EnvironmentSource::Region | EnvironmentSource::Default => {
                self.ingest_shared(settings, EnvironmentSource::Region);
                // The region's reply landed — stop the request/retry loop.
                self.req_pending = false;
            }
            EnvironmentSource::Parcel => self.ingest_parcel(settings),
        }
        source
    }

    /// Fold a **parcel**-scoped reply into the parcel layer.
    ///
    /// Two ways a reply is not an environment. It may be for a parcel the agent
    /// has since walked off — the reference drops those too
    /// (`parcel->getLocalID() != parcel_id`), and taking one would paint a
    /// neighbour's sky over this parcel. Or it may be the grid's way of saying
    /// *this parcel has no override*: the reference tests `!mDayCycle` and then
    /// an empty water or ground-level track, and both arrive here as a day cycle
    /// with empty tracks, because an absent `day_cycle` decodes to an empty one.
    /// OpenSim sends exactly that (`ViewerEnvironment.DefaultToOSD` — an
    /// `is_default` map with no `day_cycle`) for every parcel that has not set
    /// its own.
    fn ingest_parcel(&mut self, settings: EnvironmentSettings) {
        if self.parcel_id != Some(settings.parcel_id) {
            debug!(
                "environment reply for parcel {} ignored: the agent is on {:?}",
                settings.parcel_id, self.parcel_id
            );
            return;
        }
        // Answered, whichever way it went.
        self.parcel_req = None;
        let usable = !settings.day_cycle.water_track.is_empty()
            && settings
                .day_cycle
                .sky_tracks
                .first()
                .is_some_and(|ground| !ground.is_empty());
        self.parcel = usable.then(|| Box::new(settings));
        self.apply();
    }

    /// Recompute the active [`Self::settings`] from the shared environment and
    /// the pinned fixed sky.
    fn apply(&mut self) {
        // Bottom-up, exactly as the layers stack: the shared environment, then
        // a local day cycle replacing the whole schedule, then whichever of the
        // menu's pin and a local sky holds the sky track (they are the same
        // track and cannot both), then a local water frame.
        self.settings = self.shared.clone();
        self.source = self.shared_source;
        // The parcel's own environment, over the region's and under everything
        // the user or a script has said. It supplies the day cycle and its
        // length / offset and nothing else — the reference's
        // `setEnvironment(ENV_PARCEL, dayCycle, dayLength, dayOffset, version)`
        // — so the region keeps its track altitudes, which is what stops a
        // parcel from moving the sky-track bands out from under an agent
        // climbing through them.
        if let Some(parcel) = &self.parcel {
            self.settings.day_cycle = parcel.day_cycle.clone();
            self.settings.day_length = parcel.day_length;
            self.settings.day_offset = parcel.day_offset;
            self.settings.parcel_id = parcel.parcel_id;
            self.settings.env_version = parcel.env_version;
            self.source = EnvironmentSource::Parcel;
        }
        // The **pushed** layer, over the land and under the user: what an
        // experience inside this parcel has injected. Its whole-asset tracks
        // stack the way the local layer's do (a cycle, then a fixed sky, then a
        // fixed water frame); its per-key injections are folded into every frame
        // of whichever cycle came out of that, so the day keeps animating.
        if let Some((_, day)) = &self.push.day {
            self.settings.day_cycle = (**day).clone();
        }
        if let Some((_, sky)) = &self.push.sky {
            let name = sky.name.clone();
            let settings = (**sky).clone();
            self.pin_sky(settings, name);
        }
        if let Some((_, water)) = &self.push.water {
            let name = water.name.clone();
            pin_water_into(&mut self.settings, water.clone(), name);
        }
        self.push.apply_values(&mut self.settings);
        if let Some(day) = &self.local.day {
            self.settings.day_cycle = (*day.settings).clone();
        }
        match self.fixed {
            None => {
                if let Some(sky) = &self.local.sky {
                    let name = sky.settings.name.clone();
                    let settings = (*sky.settings).clone();
                    self.pin_sky(settings, name);
                }
            }
            Some(selection) => {
                let time = selection.time();
                // Each group supplies the fixed sky; only the source differs.
                let sky = match selection {
                    FixedEnvironment::Legacy(_) => time.settings(),
                    // The region's own cycle frozen at this time — and the
                    // placeholder a Modern selection shows until its asset loads.
                    FixedEnvironment::DayCycle(_) => self.day_cycle_frame(time),
                    FixedEnvironment::Modern(_) => match &self.modern_sky {
                        Some((resolved, sky)) if *resolved == time => sky.clone(),
                        _ => self.day_cycle_frame(time),
                    },
                };
                self.pin_sky(sky, time.frame_name().to_owned());
            }
        }
        if let Some(water) = &self.local.water {
            let name = water.settings.name.clone();
            let settings = water.settings.clone();
            pin_water_into(&mut self.settings, settings, name);
        }
        // The **edit** layer, over everything else: the frame an open settings
        // editor is previewing. Its sky wins over the menu's pin as well as over
        // a local sky, which is why it is applied after both rather than folded
        // into either.
        if let Some(day) = &self.edit.day {
            self.settings.day_cycle = (*day.settings).clone();
        }
        if let Some(sky) = &self.edit.sky {
            let name = sky.settings.name.clone();
            let settings = (*sky.settings).clone();
            self.pin_sky(settings, name);
        }
        if let Some(water) = &self.edit.water {
            let name = water.settings.name.clone();
            let settings = water.settings.clone();
            pin_water_into(&mut self.settings, settings, name);
        }

        self.day_position_pin = self.resolve_day_position_pin();
    }

    /// Settle what a pinned day position (`SL_VIEWER_SKY_DAY_POSITION`, used by
    /// the screenshot harness and headless checks) can actually select against
    /// the environment just composed, substituting the four-preset cycle when it
    /// would otherwise select nothing.
    ///
    /// A pinned position only means something if the cycle in force schedules
    /// more than one sky: a single-frame environment — which is what the local
    /// OpenSim grid and this workspace's own fake grid serve by default — renders
    /// the same noon sky at every position. So when there is nothing to
    /// interpolate the legacy presets go in instead, and the run gets a sunrise
    /// it can look at.
    ///
    /// **Only then.** Substituting over a region that *does* serve a real cycle
    /// would render a sky the region never sent, which is exactly the kind of
    /// silent divergence a cross-check run exists to find; the reference viewer
    /// has no such affordance and would draw the region's own frames. Whichever
    /// way it goes, the answer is recorded rather than assumed, because a
    /// substituted sky is a fact about the capture and belongs in its status
    /// file.
    ///
    /// A pinned **fixed** environment (the World ▸ Environment menu) already
    /// selects a specific frame and takes precedence, so nothing is substituted
    /// under one. Gated on the *resolved* override rather than on the variable
    /// merely being present, so the cycle is installed exactly when the pin will
    /// drive it (`crate::sky::day_position`); a malformed value falls back to the
    /// clock, which the region's own environment already follows.
    fn resolve_day_position_pin(&mut self) -> DayPositionPin {
        let Some(position) = self.pinned_day_position else {
            return DayPositionPin::NotPinned;
        };
        if self.fixed.is_some() {
            return DayPositionPin::OverriddenByFixedSky { position };
        }
        if self.settings.day_position_moves_the_sky(0.0) {
            return DayPositionPin::RegionDayCycle { position };
        }
        crate::sky_presets::install_preset_day_cycle(&mut self.settings);
        DayPositionPin::SubstitutedPresets { position }
    }

    /// What the pinned day position selected the last time the environment was
    /// composed: whether the sky on screen is the one the region serves at that
    /// position, and what stood in for it when it is not.
    #[must_use]
    pub const fn day_position_pin(&self) -> DayPositionPin {
        self.day_position_pin
    }

    /// Replace the sky schedule of the environment being composed with a single
    /// `sky` frame pinned at keyframe 0 on the surface track (the upper altitude
    /// tracks empty out, so every altitude falls back to it); the water keeps
    /// following whichever cycle is in force. Shared by all three
    /// fixed-environment groups and by a local sky asset.
    fn pin_sky(&mut self, sky: SkySettings, name: String) {
        pin_sky_into(&mut self.settings, sky, name);
    }

    /// The day cycle **in force** sampled (frozen) at `time`'s canonical
    /// position — the Day Cycle group's sky, and the placeholder a Modern
    /// selection shows until its asset loads. Falls back to the legacy preset
    /// when that cycle defines no sky.
    ///
    /// Read off the environment being composed rather than off the shared one,
    /// so a local day cycle is what "the day cycle, frozen" freezes: the two can
    /// hold at once (pinning a preset takes only the sky track), and freezing
    /// the region's cycle while a different one is rendering would show a sky
    /// from an environment nobody selected.
    fn day_cycle_frame(&self, time: FixedSky) -> SkySettings {
        self.settings
            .blended_sky_settings(0.0, time.day_position())
            .unwrap_or_else(|| time.settings())
    }

    /// The personal environment worth saving, or `None` when there is none —
    /// nothing pinned and an empty local layer, which is what "Use Shared
    /// Environment" leaves behind and must *not* come back at the next login.
    fn saved_environment(&self) -> Option<SavedEnvironment> {
        (self.fixed.is_some() || !self.local.is_empty()).then(|| SavedEnvironment {
            fixed: self.fixed,
            local: self.local.clone(),
        })
    }

    /// Install a personal environment read back from the account, without a
    /// cross-fade: there is nothing on screen yet to fade away from.
    fn restore_environment(&mut self, saved: SavedEnvironment) {
        self.fixed = saved.fixed;
        self.local = saved.local;
        // A pinned Modern sky has to be re-fetched; the placeholder stands
        // until `resolve_modern_environment` gets its asset.
        self.modern_sky = None;
        self.transition = None;
        self.apply();
    }
}

/// Replace `settings`' sky schedule with a single `sky` frame pinned at
/// keyframe 0 on the surface track — the upper altitude tracks empty out, so
/// every altitude falls back to it, and the water keeps following whichever
/// cycle is in force.
fn pin_sky_into(settings: &mut EnvironmentSettings, sky: SkySettings, name: String) {
    settings.day_cycle.sky_tracks = vec![vec![DayCycleFrame {
        keyframe: 0.0,
        name: name.clone(),
    }]];
    settings.day_cycle.sky_frames = std::iter::once((name, sky)).collect();
}

/// [`pin_sky_into`] for the water track.
fn pin_water_into(settings: &mut EnvironmentSettings, water: WaterSettings, name: String) {
    settings.day_cycle.water_track = vec![DayCycleFrame {
        keyframe: 0.0,
        name: name.clone(),
    }];
    settings.day_cycle.water_frames = std::iter::once((name, water)).collect();
}

/// Advance whichever fades are running — the whole-environment cross-fade and
/// each experience-pushed key's own blend — and end them when they are done.
///
/// Reads before it writes on purpose: [`EnvironmentState`] is a change-detected
/// resource, and a viewer that is not fading anything must not look to anything
/// downstream as though its environment changed every frame.
pub fn advance_environment_transition(time: Res<Time>, mut state: ResMut<EnvironmentState>) {
    let blending = state.push.is_blending();
    if state.transition.is_none() && !blending {
        return;
    }
    let delta = time.delta_secs();
    state.advance_transition(delta);
    if blending && state.push.advance_blends(delta) {
        // A per-key blend changes what the composed environment holds, so the
        // frames have to be rebuilt — unlike the whole-environment cross-fade,
        // which `sky_at` applies at sample time.
        state.apply();
    }
}

/// Mirror the environment's own settings into [`EnvironmentState`]: today the
/// manual transition time, which has to be in hand at the moment a menu or a
/// floater changes the environment.
pub fn sync_environment_settings(
    settings: Option<Res<ViewerSettings>>,
    mut state: ResMut<EnvironmentState>,
) {
    let seconds = settings
        .and_then(|settings| settings.store().get_f32(SETTING_TRANSITION_TIME).ok())
        // A negative time is not a fade run backwards; it is a typo in a hand-
        // edited settings file, and an instant cut is what it meant.
        .map_or(0.0, |seconds| seconds.max(0.0));
    // By bits: this is a write-on-change guard over a value copied verbatim from
    // the settings store, not a measurement, so "the same float" is exactly what
    // is being asked and a tolerance would be the wrong question.
    if state.manual_transition_seconds.to_bits() != seconds.to_bits() {
        state.manual_transition_seconds = seconds;
    }
}

/// Restore the account's saved personal environment, once, after the account
/// settings have loaded ([`SETTING_PERSIST_ACROSS_LOGIN`]).
///
/// The restore is marked done even when the setting is off or the slot is
/// empty: the question "has this session restored yet" is about the session, not
/// about whether anything was found, and re-asking it every frame would let a
/// personal environment the user has since cleared come back.
pub fn restore_saved_environment(
    settings: Option<Res<ViewerSettings>>,
    mut state: ResMut<EnvironmentState>,
) {
    if state.restored {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    state.restored = true;
    if !settings
        .store()
        .get_bool(SETTING_PERSIST_ACROSS_LOGIN)
        .unwrap_or(false)
    {
        return;
    }
    let saved = settings
        .store()
        .get_str(SETTING_SAVED_ENVIRONMENT)
        .unwrap_or_default();
    if saved.is_empty() {
        return;
    }
    match serde_json::from_str::<SavedEnvironment>(saved) {
        Ok(saved) => {
            info!("restoring the personal environment saved for this account");
            state.restore_environment(saved);
        }
        // A blob this viewer cannot read is a settings file from another
        // version, not a reason to fail a login: the region's environment is a
        // perfectly good fallback and the next save overwrites it.
        Err(error) => warn!("saved personal environment could not be read: {error}"),
    }
}

/// Write the personal environment to the account whenever it changes, so the
/// next login can restore it ([`SETTING_PERSIST_ACROSS_LOGIN`]).
///
/// Gated on the resource's own change detection, so a still environment costs
/// nothing; `Local` then remembers the last blob written, so a change that does
/// not alter the *saved* part (a fade advancing, say) writes nothing either.
pub fn persist_saved_environment(
    state: Res<EnvironmentState>,
    settings: Option<ResMut<ViewerSettings>>,
    mut written: Local<Option<String>>,
) {
    if !state.is_changed() {
        return;
    }
    let Some(mut settings) = settings else {
        return;
    };
    // Before the account scope is loaded there is nowhere to write, and the
    // restore has not run yet — saving now would persist the pre-restore state
    // over the very environment about to come back.
    if !settings.account_loaded() || !state.restored {
        return;
    }
    if !settings
        .store()
        .get_bool(SETTING_PERSIST_ACROSS_LOGIN)
        .unwrap_or(false)
    {
        return;
    }
    let encoded = match state
        .saved_environment()
        .as_ref()
        .map(serde_json::to_string)
    {
        Some(Ok(encoded)) => encoded,
        Some(Err(error)) => {
            warn!("personal environment could not be saved: {error}");
            return;
        }
        // Nothing personal in force: clear the slot rather than leave the last
        // one behind, or "Use Shared Environment" would not survive a relog.
        None => String::new(),
    };
    if written.as_ref() == Some(&encoded) {
        return;
    }
    *written = Some(encoded.clone());
    settings.set_account(SETTING_SAVED_ENVIRONMENT, SettingValue::String(encoded));
}

/// Resolve a pinned **Modern** environment selection: request its `KNOWN_SKY_*`
/// library sky asset and, once decoded, swap the decoded sky into the rendered
/// environment (replacing the region-cycle placeholder). A no-op unless a Modern
/// sky is pinned and not yet resolved for the pinned time.
pub fn resolve_modern_environment(
    mut state: ResMut<EnvironmentState>,
    mut assets: ResMut<EnvironmentAssetManager>,
) {
    let Some(FixedEnvironment::Modern(time)) = state.fixed else {
        return;
    };
    if state.modern_sky.as_ref().map(|(resolved, _)| *resolved) == Some(time) {
        return;
    }
    let key = AssetKey::from(time.modern_asset());
    assets.request(key);
    if let Some(asset) = assets.get(key)
        && let EnvironmentAsset::Sky(sky) = asset.as_ref()
    {
        let sky = sky.as_ref().clone();
        state.set_modern_sky(time, sky);
    }
}

/// A settings asset a **panel** has asked to install in the local environment
/// layer, waiting for its asset to decode.
///
/// The asset is not in hand when the user picks it: a combo row carries an
/// inventory item's asset id, and the settings behind it may never have been
/// fetched. So a pick is a *request* — recorded here, satisfied by
/// [`resolve_local_environment_pick`] whenever the fetch lands — rather than a
/// call the panel could make directly. Only the latest pick is kept: a user
/// clicking through a list of skies wants the last one, not all of them applied
/// in fetch order.
///
/// The panel writes it, so the panel needs no dependency on the asset store, and
/// two panels asking for the same thing is one fetch.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocalEnvironmentPick {
    /// The settings asset waiting to be installed, if any.
    wanted: Option<AssetKey>,
}

impl LocalEnvironmentPick {
    /// Ask for `asset` to be installed in the local layer once it decodes,
    /// replacing any pick still waiting.
    pub const fn request(&mut self, asset: AssetKey) {
        self.wanted = Some(asset);
    }

    /// The pick still waiting to be installed, if any — a panel's cue that a
    /// row it has selected is not in force *yet*.
    #[must_use]
    pub const fn pending(&self) -> Option<AssetKey> {
        self.wanted
    }
}

/// Fetch the settings asset a panel has picked ([`LocalEnvironmentPick`]) and,
/// once it decodes, install it in the local environment layer's matching track.
///
/// The same shape as [`resolve_modern_environment`] and the `@setenv_asset` half
/// of [`apply_rlv_environment`]: request every frame the pick is outstanding
/// (the store de-duplicates, and a deferred request retries once the cap is
/// known), and clear the pick only when the decoded asset is actually in hand.
pub fn resolve_local_environment_pick(
    mut pick: ResMut<LocalEnvironmentPick>,
    mut state: ResMut<EnvironmentState>,
    mut assets: ResMut<EnvironmentAssetManager>,
) {
    let Some(key) = pick.wanted else {
        return;
    };
    assets.request(key);
    if let Some(asset) = assets.get(key) {
        let asset = asset.as_ref().clone();
        pick.wanted = None;
        state.set_local(asset, Some(key.uuid()));
    } else if assets.is_unavailable(key) {
        // Give up rather than wait forever. The panel holds its selection while
        // a pick is outstanding, so a settings asset that cannot be fetched or
        // decoded would otherwise freeze the combos on a row that is not what is
        // rendering — a wrong answer held indefinitely, where dropping the pick
        // lets the next sync say what is actually in force.
        warn!(
            "settings asset {} could not be fetched or decoded; the environment is unchanged",
            key.uuid()
        );
        pick.wanted = None;
    }
}

/// Request the region environment after each region handshake **and after every
/// `RegionInfo`**, retrying until the grid's EEP reply is ingested (or
/// `MAX_ENV_ATTEMPTS` is reached).
///
/// A single one-shot request is fragile: on a slower / remote grid the
/// `ExtEnvironment` capability may not be seeded yet when the handshake
/// completes, so the runtime silently drops the request and the sky / cloud /
/// water stack is left on the legacy WindLight defaults forever (observed on
/// aditi). Retrying until [`ingest_environment`] clears the pending flag closes
/// that race — the same cap-not-ready-yet class of bug the terrain fetch hit.
/// Parcels can override the region environment; the viewer asks for the
/// whole-region settings here (`parcel_id: None`).
///
/// The handshake alone was not enough, and that was a bug rather than a
/// simplification. A handshake happens at login and at a border crossing, so an
/// estate that changed its sky under an avatar **already standing in the
/// region** never reached that viewer: it kept drawing whatever it fetched on
/// arrival for the rest of the session. The reference viewer re-reads on the
/// message a simulator sends when those settings are saved — `RegionInfo`,
/// routed through `LLViewerRegion::processRegionInfo` into
/// `LLRegionInfoModel`'s update signal, which `LLEnvironment` has hooked to
/// `requestRegion()`. It is **unconditional** there: the signal fires at the end
/// of every `LLRegionInfoModel::update` without comparing a single field
/// against what it held, so no "did the environment part change" test is owed
/// here either — and there could not be one, since `RegionInfo` carries no
/// environment fields at all. It is a hint that the region's settings were
/// written, not a copy of them.
///
/// This asks for the whole-region settings (`parcel_id: None`); the parcel the
/// agent is standing on is asked about separately, by
/// [`request_parcel_environment`].
pub fn request_environment(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut commands: MessageWriter<SlCommand>,
    mut state: ResMut<EnvironmentState>,
) {
    // A handshake (initial login or a border crossing) starts a fresh request
    // cycle for the new region's environment, and so does a `RegionInfo` —
    // which is how a change made while the avatar stands here arrives.
    for event in events.read() {
        let reason = match event.0 {
            SlSessionEvent::RegionHandshakeComplete => "region handshake complete",
            SlSessionEvent::RegionLimits(_) => "region info received",
            _ => continue,
        };
        info!("{reason}; requesting environment (EEP) settings");
        state.req_pending = true;
        state.req_attempts = 0;
        state.req_next_retry_at = 0.0;
        if matches!(event.0, SlSessionEvent::RegionHandshakeComplete) {
            // An experience's injected sky does not cross a region line: the
            // reference drops the whole `ENV_PUSH` layer here, and there is no
            // way to keep it honest across the crossing — the destination
            // region has its own idea of which experiences it admits, and the
            // script that pushed the sky is back where the agent left it.
            state.clear_pushed(DEFAULT_TRANSITION);
            // Parcel ids are region-local, so the one being stood on means
            // nothing here any more — and neither does its environment. The
            // next `SlAgentParcel` mirror re-asks for the new region's. Only a
            // handshake means a new region; a `RegionInfo` is this one saying
            // its own settings were saved.
            state.parcel = None;
            state.parcel_id = None;
            state.parcel_env_version = -1;
            state.parcel_req = None;
            state.standing_parcel = None;
            state.experience_recheck = None;
            state.apply();
        }
    }

    if !state.req_pending {
        return;
    }
    let now = time.elapsed_secs();
    if now < state.req_next_retry_at {
        return;
    }
    if state.req_attempts >= MAX_ENV_ATTEMPTS {
        warn!(
            "environment (EEP) not received after {MAX_ENV_ATTEMPTS} attempts; \
             rendering with the legacy WindLight defaults"
        );
        state.req_pending = false;
        return;
    }
    state.req_attempts = state.req_attempts.saturating_add(1);
    state.req_next_retry_at = now + ENV_RETRY_INTERVAL;
    debug!(
        "requesting environment (EEP) settings (attempt {}/{MAX_ENV_ATTEMPTS})",
        state.req_attempts
    );
    commands.write(SlCommand(Command::RequestEnvironment { parcel_id: None }));
}

/// Mirror the parcel the agent is standing on into [`EnvironmentState`], so a
/// step across a parcel line asks the grid for that parcel's environment.
///
/// The version is carried along with the id because a parcel's environment can
/// change under a standing agent: the reference re-requests when
/// `ParcelProperties` reports a different `parcel_environment_version`, and this
/// is the same signal. A region that has switched per-parcel overrides *off*
/// reports `-1` and no parcel is asked about at all, which saves a round trip
/// per parcel line on most of OpenSim.
pub fn track_agent_parcel(
    agent: Option<Res<sl_client_bevy::SlAgentParcel>>,
    mut state: ResMut<EnvironmentState>,
) {
    let Some(agent) = agent else {
        // No session mirror (a headless world fold without the session plugin):
        // there is no parcel to be standing on, and the region's environment is
        // the whole of the answer.
        return;
    };
    // Which parcel the agent is *standing* on is a separate question from which
    // parcel's environment is being tracked: a region with per-parcel overrides
    // switched off has no parcel environment to ask for, but an experience is
    // still admitted (or not) per parcel there.
    let standing = agent.current.as_ref().map(|parcel| parcel.local_id.get());
    if state.standing_parcel() != standing {
        state.set_standing_parcel(standing);
    }
    let parcel = agent.current.as_ref().filter(|parcel| {
        parcel.region_allow_environment_override && parcel.parcel_environment_version >= 0
    });
    let version = parcel.map_or(-1, |parcel| parcel.parcel_environment_version);
    let parcel_id = parcel.map(|parcel| parcel.local_id.get());
    // Read before writing: this runs every frame and the parcel under an agent
    // almost never changes, so touching the resource unconditionally would mark
    // the environment changed on every one of them — and everything downstream
    // that guards on `is_changed` (the personal-environment save, for one) would
    // do its work forever.
    if state.parcel_id == parcel_id && state.parcel_env_version == version {
        return;
    }
    state.set_agent_parcel(parcel_id, version);
}

/// Request the environment of the parcel the agent is standing on, retrying on
/// the same clock the region's request uses.
///
/// Its own system rather than an arm of [`request_environment`] because the two
/// answer different questions and can be outstanding at once: the region's is
/// started by a handshake and satisfied by a region-scoped reply, this one is
/// started by a step across a parcel line and satisfied by a reply for *that*
/// parcel.
pub fn request_parcel_environment(
    time: Res<Time>,
    mut commands: MessageWriter<SlCommand>,
    mut state: ResMut<EnvironmentState>,
) {
    let Some(request) = state.parcel_req else {
        return;
    };
    let now = time.elapsed_secs();
    if now < request.next_retry_at {
        return;
    }
    if request.attempts >= MAX_ENV_ATTEMPTS {
        warn!(
            "parcel {} environment not received after {MAX_ENV_ATTEMPTS} attempts;              rendering the region's",
            request.parcel_id
        );
        state.parcel_req = None;
        return;
    }
    state.parcel_req = Some(ParcelRequest {
        attempts: request.attempts.saturating_add(1),
        next_retry_at: now + ENV_RETRY_INTERVAL,
        ..request
    });
    debug!(
        "requesting environment (EEP) settings for parcel {} (attempt {}/{MAX_ENV_ATTEMPTS})",
        request.parcel_id,
        request.attempts.saturating_add(1),
    );
    commands.write(SlCommand(Command::RequestEnvironment {
        parcel_id: Some(request.parcel_id),
    }));
}

/// Ask the region which of the currently injecting experiences the parcel the
/// agent has just stepped onto admits (`ExperienceQuery`).
///
/// The reference asks this from inside the injection
/// (`DayInjection::testExperiencesOnParcel`), which is why the question is only
/// asked while something is injecting — and why nothing is retried: an
/// unanswered query means the sky stays as it is until the next parcel line,
/// which is what a region that serves no `ExperienceQuery` at all (every
/// OpenSim) gets on every crossing.
pub fn query_parcel_experiences(
    mut commands: MessageWriter<SlCommand>,
    mut state: ResMut<EnvironmentState>,
) {
    let Some(parcel_id) = state.experience_recheck else {
        return;
    };
    let experiences: Vec<ExperienceKey> = state.pushed().experiences().into_iter().collect();
    state.experience_recheck = None;
    if experiences.is_empty() {
        return;
    }
    debug!(
        "agent stepped onto parcel {parcel_id}; asking which of {} injecting experience(s) it admits",
        experiences.len()
    );
    commands.write(SlCommand(Command::QueryParcelExperiences {
        parcel_id,
        experiences,
    }));
}

/// Release the injected environment of every experience the parcel the agent
/// stands on does **not** admit — the answer half of
/// [`query_parcel_experiences`].
///
/// An answer about a parcel the agent has since walked off is discarded rather
/// than acted on: the reference re-reads the agent's parcel when its coroutine
/// resumes and returns if it has changed, because clearing an experience the
/// *previous* parcel refused would take away a sky the current one allows.
pub fn ingest_parcel_experiences(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<EnvironmentState>,
) {
    for event in events.read() {
        let SlSessionEvent::ParcelExperiences {
            parcel_id,
            experiences,
        } = &event.0
        else {
            continue;
        };
        if state.standing_parcel() != Some(*parcel_id) {
            debug!(
                "experience query answered for parcel {parcel_id}, which the agent has already \
                 left (now on {:?}); ignoring it",
                state.standing_parcel()
            );
            continue;
        }
        for (experience, admitted) in experiences {
            if *admitted {
                continue;
            }
            info!(
                "parcel {parcel_id} does not admit experience {experience}; releasing the \
                 environment it pushed"
            );
            state.release_pushed_experience(*experience);
        }
    }
}

/// Fold an incoming [`SlSessionEvent::Environment`] into [`EnvironmentState`],
/// replacing the legacy default (or the previously ingested region environment)
/// with the grid's settings. A parcel-scoped reply goes to the parcel layer
/// instead of the region's — see `EnvironmentState::ingest_reply`.
pub fn ingest_environment(mut events: MessageReader<SlEvent>, mut state: ResMut<EnvironmentState>) {
    for event in events.read() {
        if let SlSessionEvent::Environment(settings) = &event.0 {
            let sky_count = settings.day_cycle.sky_frames.len();
            let water_count = settings.day_cycle.water_frames.len();
            match state.ingest_reply((**settings).clone()) {
                // Its own layer, above the region's — see
                // `EnvironmentState::ingest_parcel`.
                EnvironmentSource::Parcel => info!(
                    "environment ingested (parcel {}): {sky_count} sky frame(s), \
                     {water_count} water frame(s), cycle {:?}",
                    settings.parcel_id, settings.day_cycle.name,
                ),
                EnvironmentSource::Region | EnvironmentSource::Default => info!(
                    "environment ingested (region): day_length={}s, day_offset={}s, \
                     {sky_count} sky frame(s), {water_count} water frame(s), cycle {:?}",
                    settings.day_length, settings.day_offset, settings.day_cycle.name,
                ),
            }
        }
    }
}

/// Fold each incoming [`SlSessionEvent::ExperienceEnvironmentPush`] into the
/// pushed layer of [`EnvironmentState`] — an experience's `llSetEnvironment`.
///
/// `Clear` and `Partial` land the frame they arrive; a `Full` push names a
/// settings **asset**, so it is held here until
/// [`EnvironmentAssetManager`] resolves it (and dropped, with a warning, once
/// the manager says it never will). Held as a queue rather than one slot
/// because two experiences may each be waiting on an asset, and a second one
/// must not cancel the first.
pub fn ingest_experience_environment_push(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<EnvironmentState>,
    mut assets: ResMut<EnvironmentAssetManager>,
    mut pending: Local<Vec<(ExperienceEnvironmentPush, AssetKey)>>,
) {
    for event in events.read() {
        let SlSessionEvent::ExperienceEnvironmentPush(push) = &event.0 else {
            continue;
        };
        info!(
            "experience {} pushed {} at this viewer from {:?} on {:?} (transition {}s)",
            push.experience_id,
            push.action.name(),
            push.object_name,
            push.parcel_name,
            push.transition_time,
        );
        // A release cancels whatever that experience was still waiting on: the
        // asset it asked for is no longer wanted, and installing it after the
        // clear would put back exactly what the clear took away.
        if matches!(push.action, EnvironmentPushAction::Clear) {
            let released = push.experience_id;
            let all = released.uuid().is_nil();
            pending.retain(|(waiting, _)| !all && waiting.experience_id != released);
        }
        if let EnvironmentPushAction::Full { asset_id } = push.action {
            pending.push(((**push).clone(), AssetKey::from(asset_id)));
            continue;
        }
        state.apply_environment_push(push, None);
    }

    // Whatever is waiting on an asset: request it (idempotent), then install it
    // the frame it decodes.
    pending.retain(|(push, key)| {
        assets.request(*key);
        if let Some(asset) = assets.get(*key) {
            let asset = asset.as_ref().clone();
            state.apply_environment_push(push, Some(asset));
            return false;
        }
        if assets.is_unavailable(*key) {
            warn!(
                "experience {} pushed settings asset {}, which cannot be fetched or decoded; \
                 the sky is unchanged",
                push.experience_id,
                key.uuid()
            );
            return false;
        }
        true
    });
}

/// Carry out whatever the RLV `@setenv_*` family has queued in
/// [`RlvEnvironmentSlot`], and tell it what the sky looks like now.
///
/// The RLV engine cannot reach [`EnvironmentState`] — it lives three crates
/// away, and the enforcement surfaces that run commands must not depend on the
/// renderer — so the two meet in a world-API resource, exactly as `@setrot`
/// meets the movement driver. This is the scene's half: it **takes** the edited
/// sky and the queued request (so nothing is applied twice), installs the result
/// in the local environment slot, and republishes what is being rendered for the
/// next `@getenv_*` read.
///
/// The asset a `@setenv_asset` names is fetched, so it is remembered until the
/// settings store has decoded it, and any newer command replaces it — a script
/// that asks for one sky and then edits another is not still waiting for the
/// first when the second lands.
pub fn apply_rlv_environment(
    mut slot: ResMut<RlvEnvironmentSlot>,
    mut state: ResMut<EnvironmentState>,
    mut assets: ResMut<EnvironmentAssetManager>,
    mut pending: Local<Option<AssetKey>>,
) {
    if let Some(request) = slot.request.take() {
        match request {
            RlvEnvironmentRequest::Clear => {
                *pending = None;
                // Back to the shared environment, which is what dropping the
                // whole local layer means — the menu's pin goes with it.
                state.set_fixed(None);
            }
            RlvEnvironmentRequest::DayTime(position) => {
                *pending = None;
                let sky = state.shared_sky_at(position);
                // A sampled frame is nobody's asset: no inventory row names it,
                // and a preset list must not claim one is in force.
                state.set_local(EnvironmentAsset::Sky(Box::new(sky)), None);
            }
            RlvEnvironmentRequest::Asset(id) => *pending = Some(AssetKey::from(id)),
            // Never queued: the source resolves a preset or day-cycle *name* to
            // an asset id before it gets here, and refuses one it cannot.
            RlvEnvironmentRequest::Preset(_) | RlvEnvironmentRequest::DayCycle(_) => {}
        }
    }
    // A per-value edit comes second, because a whole-environment change drops
    // any edit that was waiting: an edit that survived one was made *after* it
    // and replaces the layer again, pending asset and all.
    if let Some(sky) = slot.edited.take() {
        *pending = None;
        state.set_local(EnvironmentAsset::Sky(Box::new(sky)), None);
    }
    if let Some(key) = *pending {
        assets.request(key);
        if let Some(asset) = assets.get(key) {
            let asset = asset.as_ref().clone();
            *pending = None;
            state.set_local(asset, Some(key.uuid()));
        }
    }

    // What a read answers from. Guarded rather than written blind: the slot is a
    // change-detected resource, and a sky that has not moved should not look to
    // anything downstream as though it had.
    let rendered = state.rendered_sky();
    if slot.rendered != rendered {
        slot.rendered = rendered;
    }
    let fixed = state.has_local_fixed_sky();
    if slot.fixed_sky != fixed {
        slot.fixed_sky = fixed;
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use bevy::prelude::*;
    use sl_client_bevy::{Command, SlCommand, SlEvent, SlSessionEvent};

    use super::{
        DayPositionPin, EnvironmentAsset, EnvironmentPushAction, EnvironmentSettings,
        EnvironmentSource, EnvironmentState, ExperienceEnvironmentPush, ExperienceKey,
        FixedEnvironment, Llsd, SavedEnvironment, SkySettings, Uuid,
    };
    use crate::sky_presets::FixedSky;
    use sl_client_bevy::WaterSettings;

    /// An environment reply for `parcel_id` (`-1` = the whole region), tagged by
    /// its day length so the folded settings are identifiable.
    fn reply(parcel_id: i32, day_length: i32) -> EnvironmentSettings {
        let mut settings = EnvironmentSettings::legacy_windlight_default();
        settings.parcel_id = parcel_id;
        settings.day_length = day_length;
        settings
    }

    /// A region cycle with two named sky keyframes, so a day position has
    /// something to choose between.
    fn two_frame_region_cycle() -> EnvironmentSettings {
        let mut settings = reply(-1, 1234);
        let dawn = SkySettings::legacy_windlight_default("region-dawn");
        let dusk = SkySettings::legacy_windlight_default("region-dusk");
        settings.day_cycle.sky_tracks = vec![vec![
            sl_client_bevy::DayCycleFrame {
                keyframe: 0.0,
                name: "region-dawn".to_owned(),
            },
            sl_client_bevy::DayCycleFrame {
                keyframe: 0.5,
                name: "region-dusk".to_owned(),
            },
        ]];
        settings.day_cycle.sky_frames = [
            ("region-dawn".to_owned(), dawn),
            ("region-dusk".to_owned(), dusk),
        ]
        .into_iter()
        .collect();
        settings
    }

    /// A pinned day position against a region that serves one sky is a request
    /// nothing can answer — so the legacy presets go in, and the run is told
    /// that the sky on screen is not the region's.
    #[test]
    fn a_pin_over_a_one_frame_cycle_substitutes_the_presets_and_says_so() {
        let mut state = EnvironmentState {
            pinned_day_position: Some(0.5),
            ..Default::default()
        };
        state.apply();
        assert_eq!(
            state.day_position_pin(),
            DayPositionPin::SubstitutedPresets { position: 0.5 }
        );
        assert!(!state.day_position_pin().is_the_regions_own_sky());
        assert!(
            state
                .day_position_pin()
                .describe()
                .is_some_and(|line| line.contains("not the region's sky"))
        );
        // And the substitute is a cycle a position can actually sample.
        assert!(state.settings.day_position_moves_the_sky(0.0));
    }

    /// A region that *does* serve a cycle keeps it. Substituting here would draw
    /// a sky the region never sent while reporting the pin as honoured, which is
    /// the one way a cross-check can lie about which renderer drew what.
    #[test]
    fn a_pin_over_a_real_cycle_leaves_the_regions_own_frames_alone() {
        let mut state = EnvironmentState {
            pinned_day_position: Some(0.25),
            ..Default::default()
        };
        assert_eq!(
            state.ingest_reply(two_frame_region_cycle()),
            EnvironmentSource::Region
        );
        assert_eq!(
            state.day_position_pin(),
            DayPositionPin::RegionDayCycle { position: 0.25 }
        );
        assert!(state.day_position_pin().is_the_regions_own_sky());
        let names: Vec<&str> = state
            .settings
            .day_cycle
            .sky_frames
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(names, ["region-dawn", "region-dusk"]);
    }

    /// A fixed sky names one frame outright, so the pin selects nothing — and a
    /// run that asked for both has asked for two different things.
    #[test]
    fn a_fixed_sky_beats_the_pin_and_the_run_is_told() {
        let mut state = EnvironmentState {
            pinned_day_position: Some(0.75),
            ..Default::default()
        };
        assert_eq!(
            state.ingest_reply(two_frame_region_cycle()),
            EnvironmentSource::Region
        );
        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));
        assert_eq!(
            state.day_position_pin(),
            DayPositionPin::OverriddenByFixedSky { position: 0.75 }
        );
        assert!(!state.day_position_pin().is_the_regions_own_sky());
    }

    /// `-1` is the whole region; every non-negative id — `0` included — is a
    /// parcel's own override.
    #[test]
    fn reply_scope_follows_the_parcel_id_sentinel() {
        assert_eq!(EnvironmentSource::of_reply(-1), EnvironmentSource::Region);
        assert_eq!(EnvironmentSource::of_reply(0), EnvironmentSource::Parcel);
        assert_eq!(EnvironmentSource::of_reply(37), EnvironmentSource::Parcel);
    }

    #[test]
    fn a_region_reply_becomes_the_shared_environment_and_ends_the_retry_loop() {
        let mut state = EnvironmentState {
            req_pending: true,
            ..Default::default()
        };

        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );

        assert_eq!(state.shared.day_length, 1234);
        assert_eq!(state.settings.day_length, 1234);
        assert_eq!(state.shared_source, EnvironmentSource::Region);
        assert_eq!(state.source, EnvironmentSource::Region);
        assert!(
            !state.req_pending,
            "the region's reply satisfies the request"
        );
    }

    /// A parcel reply never becomes [`EnvironmentState::shared`], whatever else
    /// it does — that field is the *grid region's* environment, and it is what
    /// the parcel layer and the local layer are stacked on top of.
    #[test]
    fn a_parcel_reply_does_not_replace_the_region_environment() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );

        assert_eq!(state.ingest_reply(reply(7, 999)), EnvironmentSource::Parcel);

        assert_eq!(state.shared.day_length, 1234, "the region's settings stand");
        assert_eq!(state.settings.day_length, 1234);
        assert_eq!(state.shared_source, EnvironmentSource::Region);
    }

    /// The bug this guards: a parcel reply used to clear `req_pending`, so the
    /// region environment was never re-requested and the sky stayed on the
    /// legacy WindLight defaults.
    #[test]
    fn a_parcel_reply_leaves_the_region_request_outstanding() {
        let mut state = EnvironmentState {
            req_pending: true,
            ..Default::default()
        };

        assert_eq!(state.ingest_reply(reply(3, 999)), EnvironmentSource::Parcel);

        assert!(state.req_pending, "the region is still unanswered");
        assert_eq!(state.shared_source, EnvironmentSource::Default);
        assert_eq!(state.source, EnvironmentSource::Default);
    }

    /// A parcel reply carrying a real environment: one sky frame on the ground
    /// track and one water frame, which is the shape a parcel that has set its
    /// own environment sends.
    fn parcel_reply(parcel_id: i32, day_length: i32, frame: &str) -> EnvironmentSettings {
        let mut settings = reply(parcel_id, day_length);
        settings.day_cycle.name = frame.to_owned();
        settings.day_cycle.sky_tracks = vec![vec![super::DayCycleFrame {
            keyframe: 0.0,
            name: frame.to_owned(),
        }]];
        settings.day_cycle.sky_frames = std::iter::once((
            frame.to_owned(),
            SkySettings::legacy_windlight_default(frame),
        ))
        .collect();
        settings.day_cycle.water_track = vec![super::DayCycleFrame {
            keyframe: 0.0,
            name: frame.to_owned(),
        }];
        settings.day_cycle.water_frames =
            std::iter::once((frame.to_owned(), WaterSettings::legacy_default(frame))).collect();
        settings
    }

    /// **A parcel's own environment is what renders while the agent stands on
    /// it** — the reference's `setEnvironment(ENV_PARCEL, …)`, above the region
    /// and below anything the user has said.
    #[test]
    fn a_parcel_override_renders_over_the_region() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );
        state.set_agent_parcel(Some(7), 3);

        assert_eq!(
            state.ingest_reply(parcel_reply(7, 900, "parcel-sky")),
            EnvironmentSource::Parcel
        );

        assert!(
            state
                .settings
                .day_cycle
                .sky_frames
                .contains_key("parcel-sky")
        );
        assert_eq!(
            state.settings.day_length, 900,
            "the parcel's own day length"
        );
        assert_eq!(state.source, EnvironmentSource::Parcel);
        // And the region underneath it is untouched, so "Use Shared
        // Environment" and a later region reply still mean the region.
        assert_eq!(state.shared.day_length, 1234);
    }

    /// **A parcel with no override is not an environment.** OpenSim answers
    /// every such parcel with an `is_default` map carrying no `day_cycle`, which
    /// decodes to empty tracks — the reference's `!mDayCycle` and
    /// `isTrackEmpty` arms, both of which clear the layer rather than render it.
    /// Rendering it would paint an empty sky over most of a region.
    #[test]
    fn a_parcel_without_an_override_keeps_the_region_sky() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );
        state.set_agent_parcel(Some(7), 3);

        // `reply` builds the legacy default, whose *tracks* are empty — exactly
        // what an absent `day_cycle` decodes to.
        let mut empty = reply(7, 900);
        empty.day_cycle.sky_tracks = Vec::new();
        empty.day_cycle.water_track = Vec::new();
        assert_eq!(state.ingest_reply(empty), EnvironmentSource::Parcel);

        assert!(state.parcel_environment().is_none());
        assert_eq!(state.settings.day_length, 1234, "the region's day length");
        assert_eq!(state.source, EnvironmentSource::Region);
    }

    /// **A reply for a parcel the agent has walked off is dropped**, as the
    /// reference drops one whose id is not the agent parcel's. Taking it would
    /// paint the neighbour's sky over the parcel actually being stood on.
    #[test]
    fn a_reply_for_another_parcel_is_dropped() {
        let mut state = EnvironmentState::default();
        state.set_agent_parcel(Some(7), 3);

        assert_eq!(
            state.ingest_reply(parcel_reply(8, 900, "next-door")),
            EnvironmentSource::Parcel
        );

        assert!(state.parcel_environment().is_none());
        assert!(
            !state
                .settings
                .day_cycle
                .sky_frames
                .contains_key("next-door"),
            "the neighbour's sky is not what renders"
        );
    }

    /// **Stepping over a parcel line drops the old override at once**, without
    /// waiting for the new parcel to answer. The region's environment is always
    /// a defensible thing to draw; the parcel behind you never is.
    #[test]
    fn walking_off_a_parcel_drops_its_environment() {
        let mut state = EnvironmentState::default();
        let _region = state.ingest_reply(reply(-1, 1234));
        state.set_agent_parcel(Some(7), 3);
        let _parcel = state.ingest_reply(parcel_reply(7, 900, "parcel-sky"));
        assert_eq!(state.settings.day_length, 900);

        state.set_agent_parcel(Some(8), 1);

        assert!(state.parcel_environment().is_none());
        assert_eq!(state.settings.day_length, 1234, "back to the region's");
    }

    /// **A parcel cannot move the sky-track altitudes.** The reference assigns
    /// `mTrackAltitudes` in the region branch only, so an agent climbing through
    /// the bands crosses them where the *region* put them however many parcels
    /// they fly over.
    #[test]
    fn a_parcel_does_not_move_the_track_altitudes() {
        let mut state = EnvironmentState::default();
        let mut region = reply(-1, 1234);
        region.track_altitudes = [1000.0, 2000.0, 3000.0];
        let _region: EnvironmentSource = state.ingest_reply(region);
        state.set_agent_parcel(Some(7), 3);

        let mut parcel = parcel_reply(7, 900, "parcel-sky");
        parcel.track_altitudes = [10.0, 20.0, 30.0];
        let _parcel: EnvironmentSource = state.ingest_reply(parcel);

        // By bits: these are the region's own numbers copied through, not a
        // computation, so anything but "the same floats" is a bug.
        assert_eq!(
            state.settings.track_altitudes.map(f32::to_bits),
            [1000.0_f32, 2000.0, 3000.0].map(f32::to_bits),
        );
    }

    /// **A personal environment still wins over the parcel's**, which is the
    /// whole point of the layer order: a parcel may repaint the sky, and the
    /// user may repaint it back for themselves.
    #[test]
    fn the_local_layer_still_wins_over_a_parcel() {
        let mut state = EnvironmentState::default();
        let _region = state.ingest_reply(reply(-1, 1234));
        state.set_agent_parcel(Some(7), 3);
        let _parcel = state.ingest_reply(parcel_reply(7, 900, "parcel-sky"));

        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);

        assert_eq!(
            state
                .settings
                .day_cycle
                .sky_frames
                .get("script")
                .map(|sky| sky.gamma),
            Some(3.5)
        );
        assert!(
            state.parcel_environment().is_some(),
            "the parcel's environment is still underneath, for when the local layer goes"
        );
        state.set_fixed(None);
        assert!(
            state
                .settings
                .day_cycle
                .sky_frames
                .contains_key("parcel-sky")
        );
    }

    /// The other half of the bug: "Use Shared Environment" restores the *shared*
    /// environment, and a parcel override is never part of that — it is its own
    /// layer. Here no parcel is being stood on, so the reply is not this agent's
    /// parcel's either, and the region's settings are what come back.
    #[test]
    fn unpinning_a_fixed_sky_restores_the_region_not_a_parcel() {
        let mut state = EnvironmentState::default();
        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );
        assert_eq!(state.ingest_reply(reply(7, 999)), EnvironmentSource::Parcel);

        state.set_fixed(None);

        assert_eq!(state.settings.day_length, 1234);
    }

    /// A `RegionInfo`'s decoded limits.
    ///
    /// Spelled out rather than defaulted because [`RegionLimits`] has no
    /// `Default` — and should not: every field of it is something a grid
    /// actually said. Nothing here reads the contents; what is under test is
    /// that the *arrival* of one re-reads the environment, which is also why
    /// the reference viewer compares no field either.
    fn region_info() -> sl_client_bevy::RegionLimits {
        sl_client_bevy::RegionLimits {
            sim_name: None,
            max_agents: 40,
            hard_max_agents: 0,
            hard_max_objects: 0,
            region_flags: 0,
            region_flags_extended: 0,
            maturity: sl_client_bevy::Maturity::Pg,
            estate_id: 1,
            parent_estate_id: 1,
            water_height: 20.0,
            billable_factor: 1.0,
            object_bonus_factor: 1.0,
            terrain_raise_limit: 4.0,
            terrain_lower_limit: -4.0,
            price_per_meter: sl_client_bevy::LindenAmount(1),
            redirect_grid_x: 0,
            redirect_grid_y: 0,
            use_estate_sun: true,
            sun_hour: 0.0,
            chat_settings: None,
            combat_settings: None,
        }
    }

    /// Every request the system wrote in one run.
    /// Drained rather than read through a cursor: Bevy keeps a message alive
    /// for two frames, so a fresh cursor per call would report the *previous*
    /// call's request again and every "and then nothing happened" assertion
    /// would be answered by the request that already had.
    fn requests(app: &mut App) -> Vec<i32> {
        app.update();
        app.world_mut()
            .resource_mut::<Messages<SlCommand>>()
            .drain()
            .filter_map(|command| match command.0 {
                Command::RequestEnvironment { parcel_id } => Some(parcel_id.unwrap_or(-1)),
                _ => None,
            })
            .collect()
    }

    /// An app running [`request_environment`] with nothing else in it.
    fn env_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin);
        app.add_message::<SlEvent>();
        app.add_message::<SlCommand>();
        app.init_resource::<EnvironmentState>();
        app.add_systems(Update, super::request_environment);
        app
    }

    /// **A `RegionInfo` re-reads the region's environment.**
    ///
    /// The estate saved its sky while the avatar was standing here, so no
    /// handshake is coming: `RegionInfo` is the only notice the viewer gets,
    /// and the reference viewer re-reads on it unconditionally. Before this the
    /// viewer kept drawing the sky it fetched on arrival for the rest of the
    /// session.
    #[test]
    fn a_region_info_re_reads_the_environment() {
        let mut app = env_app();
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::RegionLimits(region_info())));
        assert_eq!(
            requests(&mut app),
            vec![-1],
            "a RegionInfo must start a fresh whole-region environment request"
        );
    }

    /// The handshake still does what it always did — the `RegionInfo` trigger is
    /// an addition, not a replacement.
    #[test]
    fn a_handshake_still_re_reads_the_environment() {
        let mut app = env_app();
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::RegionHandshakeComplete));
        assert_eq!(requests(&mut app), vec![-1]);
    }

    /// An unrelated event starts nothing: the retry loop is armed by the two
    /// region notices and by nothing else, so a chatty session does not turn
    /// into a stream of environment fetches.
    #[test]
    fn an_unrelated_event_asks_for_nothing() {
        let mut app = env_app();
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::RegionHandshakeComplete));
        assert_eq!(requests(&mut app), vec![-1]);
        // The reply lands, ending the cycle; an unrelated event must not
        // restart it.
        app.world_mut()
            .resource_mut::<EnvironmentState>()
            .ingest_reply(reply(-1, 1234));
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::SimulatorVersion("test".to_owned())));
        assert_eq!(requests(&mut app), Vec::<i32>::new());
    }

    /// A sky nothing else could have produced, so a test can tell it apart from
    /// the region's own at a glance.
    fn script_sky() -> SkySettings {
        SkySettings {
            name: "script".to_owned(),
            gamma: 3.5,
            ..SkySettings::legacy_windlight_default("script")
        }
    }

    /// A script's sky is what renders while it holds the local layer, and the
    /// shared environment underneath it is untouched.
    #[test]
    fn a_script_sky_is_what_renders() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );

        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);

        assert_eq!(
            state
                .settings
                .day_cycle
                .sky_frames
                .get("script")
                .map(|sky| sky.gamma),
            Some(3.5)
        );
        // The water and the day length keep following the grid: a sky asset
        // replaces the sky schedule and nothing else.
        assert_eq!(state.settings.day_length, 1234);
        assert_eq!(state.shared.day_length, 1234);
        assert!(state.has_local_fixed_sky());
    }

    /// **An editor's preview renders over the personal environment, and gives
    /// it back.** The whole point of the edit layer being a layer: a settings
    /// editor is opened over whatever sky the user had built, and closing it —
    /// saved or not — has to leave that sky exactly as it was. A window that
    /// wrote the local layer instead would have to snapshot and restore it, and
    /// a script's `@setenv_*` landing in between would make the restore put
    /// back something the user never chose.
    #[test]
    fn an_edited_frame_renders_over_the_local_one_and_gives_it_back() {
        let mut state = EnvironmentState::default();
        let _source = state.ingest_reply(reply(-1, 1234));
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);

        let mut edited = SkySettings::legacy_windlight_default("edited");
        edited.gamma = 9.5;
        state.set_edit(EnvironmentAsset::Sky(Box::new(edited)));

        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.gamma),
            Some(9.5),
            "the frame being edited is the one that renders"
        );
        assert!(
            state.local().sky().is_some(),
            "the personal environment is still underneath, untouched"
        );

        state.clear_edit(sl_client_bevy::SettingsKind::Sky);
        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.gamma),
            Some(3.5),
            "closing the editor puts the personal sky back"
        );
    }

    /// The two editors own separate tracks, so closing one does not take the
    /// other's preview away — both windows can be open at once.
    #[test]
    fn closing_one_editor_leaves_the_other_s_preview() {
        let mut state = EnvironmentState::default();
        let _source = state.ingest_reply(reply(-1, 1234));

        let mut water = WaterSettings::legacy_default("edited-water");
        water.fresnel_scale = 0.125;
        state.set_edit(EnvironmentAsset::Water(water));
        let mut sky = SkySettings::legacy_windlight_default("edited-sky");
        sky.gamma = 9.5;
        state.set_edit(EnvironmentAsset::Sky(Box::new(sky)));

        state.clear_edit(sl_client_bevy::SettingsKind::Sky);

        assert_eq!(
            state.water_at(0.0).map(|water| water.fresnel_scale),
            Some(0.125),
            "the water editor's preview outlives the sky editor's window"
        );
    }

    /// **A preview is not a personal environment.** What an editor is showing
    /// must not be written to the account and come back at the next login: the
    /// user was looking at it, not living in it.
    #[test]
    fn an_edited_frame_is_not_saved_as_the_personal_environment() {
        let mut state = EnvironmentState::default();
        state.set_edit(EnvironmentAsset::Sky(Box::new(script_sky())));
        assert!(
            state.saved_environment().is_none(),
            "an open editor alone is nothing to persist"
        );
    }

    /// One local **sky**, not two: the menu takes that track back from a
    /// script, and a script takes it back from the menu.
    #[test]
    fn the_menu_and_a_script_share_one_local_sky() {
        let mut state = EnvironmentState::default();
        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);
        assert_eq!(
            state.fixed(),
            None,
            "a script takes the track from the menu"
        );

        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midday)));
        assert!(
            state.local().sky().is_none(),
            "the menu takes the track back from a script"
        );

        // And "Use Shared Environment" empties the whole layer, whichever put
        // what in it -- the reference's `setSharedEnvironment`.
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);
        state.set_local(
            EnvironmentAsset::Water(WaterSettings::legacy_default("script-water")),
            None,
        );
        state.set_fixed(None);
        assert!(state.local().is_empty());
        assert!(!state.has_local_fixed_sky());
    }

    /// **A pinned preset leaves the other two tracks standing.** Only the sky
    /// is a thing the menu and the local layer both claim; a water frame a
    /// script (or the preset combos) installed is not the menu's to drop, and
    /// the reference does not drop it either -- `setEnvironment(ENV_LOCAL,
    /// fixedEnvironment_t)` writes the one track it was given.
    #[test]
    fn pinning_a_preset_keeps_the_local_water_and_day() {
        let mut state = EnvironmentState::default();
        let mut cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        cycle.name = "script-cycle".to_owned();
        state.set_local(
            EnvironmentAsset::DayCycle(Box::new(cycle)),
            Some(Uuid::from_u128(0xD1)),
        );
        state.set_local(
            EnvironmentAsset::Water(WaterSettings::legacy_default("script-water")),
            Some(Uuid::from_u128(0xB1)),
        );

        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));

        assert_eq!(
            state.local().day().and_then(|track| track.asset),
            Some(Uuid::from_u128(0xD1)),
            "the day cycle stands"
        );
        assert_eq!(
            state.local().water().and_then(|track| track.asset),
            Some(Uuid::from_u128(0xB1)),
            "and so does the water"
        );
        assert_eq!(state.settings.day_cycle.name, "script-cycle");
        assert_eq!(
            state
                .settings
                .day_cycle
                .water_frames
                .keys()
                .collect::<Vec<_>>(),
            vec!["script-water"],
        );
    }

    /// **The three tracks are independent, and a day cycle is the exception.**
    ///
    /// Picking a water preset must not drop the sky picked a moment earlier --
    /// that is the whole reason the quick-preferences panel has three combos --
    /// while a day cycle clears both fixed frames, because
    /// `DayInstance::setDay` resets them and lets the cycle animate them.
    #[test]
    fn tracks_are_independent_except_a_day_cycle() {
        let mut state = EnvironmentState::default();
        state.set_local(
            EnvironmentAsset::Sky(Box::new(script_sky())),
            Some(Uuid::from_u128(0xA1)),
        );
        state.set_local(
            EnvironmentAsset::Water(WaterSettings::legacy_default("script-water")),
            Some(Uuid::from_u128(0xA2)),
        );
        assert_eq!(
            state.local().sky().and_then(|track| track.asset),
            Some(Uuid::from_u128(0xA1)),
            "a water pick leaves the sky alone"
        );
        // Both are rendering, at once.
        assert!(state.settings.day_cycle.sky_frames.contains_key("script"));
        assert!(
            state
                .settings
                .day_cycle
                .water_frames
                .contains_key("script-water")
        );

        let mut cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        cycle.name = "script-cycle".to_owned();
        state.set_local(EnvironmentAsset::DayCycle(Box::new(cycle)), None);
        assert!(state.local().sky().is_none(), "a day cycle clears the sky");
        assert!(state.local().water().is_none(), "and the water");
        assert!(state.local().day().is_some());
    }

    /// A water asset overrides the water track and leaves the sky alone; a day
    /// cycle replaces the whole schedule.
    #[test]
    fn a_local_asset_overrides_only_the_track_it_is() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );
        let sky_frames = state.settings.day_cycle.sky_frames.clone();

        let water = WaterSettings {
            name: "script-water".to_owned(),
            ..WaterSettings::legacy_default("script-water")
        };
        state.set_local(EnvironmentAsset::Water(water), None);
        assert_eq!(state.settings.day_cycle.sky_frames, sky_frames);
        assert_eq!(
            state
                .settings
                .day_cycle
                .water_frames
                .keys()
                .collect::<Vec<_>>(),
            vec!["script-water"]
        );

        let mut cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        cycle.name = "script-cycle".to_owned();
        state.set_local(EnvironmentAsset::DayCycle(Box::new(cycle)), None);
        assert_eq!(state.settings.day_cycle.name, "script-cycle");
        assert_eq!(
            state.settings.day_length, 1234,
            "the grid's day length is not part of the cycle asset"
        );
    }

    /// Only a local **sky** is a fixed sky. A day cycle or a water frame leaves
    /// the sky animating, which is the case `@getenv_daytime` answers `-1` for.
    #[test]
    fn only_a_local_sky_is_a_fixed_sky() {
        let mut state = EnvironmentState::default();
        assert!(!state.has_local_fixed_sky());

        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);
        assert!(state.has_local_fixed_sky());

        let mut cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        cycle.name = "script-cycle".to_owned();
        state.set_local(EnvironmentAsset::DayCycle(Box::new(cycle)), None);
        assert!(!state.has_local_fixed_sky());

        state.set_local(
            EnvironmentAsset::Water(WaterSettings::legacy_default("script-water")),
            None,
        );
        assert!(!state.has_local_fixed_sky());

        // The menu's own pin is a fixed sky whatever a script left behind.
        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));
        assert!(state.has_local_fixed_sky());
    }

    /// **A manual change with no transition time set is a cut, as it always
    /// was.** The declared default is zero, so the fade must cost nothing and
    /// change nothing until somebody asks for one.
    #[test]
    fn without_a_transition_time_a_change_is_instant() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );

        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));

        assert!(!state.is_transitioning());
        assert_eq!(
            state.sky_at(0.0, 0.5),
            state.settings.blended_sky_settings(0.0, 0.5),
            "with no fade, sampling the state is sampling the settings"
        );
    }

    /// **A fade is a fade: half way through, half of it has happened.** The
    /// midpoint is the assertion that matters — an unblended `sky_at` would
    /// answer the destination from the first frame, and a fade running the wrong
    /// way would answer the origin until the very end.
    #[test]
    fn a_manual_change_fades_from_what_was_rendering() {
        let mut state = EnvironmentState {
            manual_transition_seconds: 4.0,
            ..Default::default()
        };
        let before = state
            .settings
            .blended_sky_settings(0.0, 0.5)
            .map(|sky| sky.gamma);
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);
        assert!(state.is_transitioning());

        // At the start the old sky is what is drawn, gamma and all.
        let start = state.sky_at(0.0, 0.5).map(|sky| sky.gamma);
        assert_eq!(start, before);

        // Half way: between the two, and neither.
        let middle = {
            state.advance_transition(2.0);
            state.sky_at(0.0, 0.5).map(|sky| sky.gamma)
        };
        let blended = match (before, middle) {
            (Some(from), Some(middle)) => {
                (middle - from).abs() > f32::EPSILON && (middle - 3.5_f32).abs() > f32::EPSILON
            }
            // No sky on one side or the other is not a blend either, and is the
            // more interesting failure of the two.
            _ => false,
        };
        assert!(
            blended,
            "the half-way gamma {middle:?} is an endpoint, not a blend of {before:?} and 3.5"
        );

        // And past the end the fade is gone and the destination stands.
        state.advance_transition(2.0);
        assert!(!state.is_transitioning());
        assert_eq!(state.sky_at(0.0, 0.5).map(|sky| sky.gamma), Some(3.5));
    }

    /// **A live edit never starts a fade**, whatever the transition time says —
    /// the path the Personal Lighting sliders write through.
    #[test]
    fn a_live_edit_is_never_faded() {
        let mut state = EnvironmentState {
            manual_transition_seconds: 4.0,
            ..Default::default()
        };
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);
        assert!(state.is_transitioning(), "a plain set_local does fade");

        state.set_local_instant(EnvironmentAsset::Sky(Box::new(script_sky())), None);

        assert!(!state.is_transitioning());
        assert_eq!(state.sky_at(0.0, 0.5).map(|sky| sky.gamma), Some(3.5));
    }

    /// **A grid reply is not a manual change**, so the region's own environment
    /// arriving (or changing under the agent) does not fade.
    #[test]
    fn a_grid_reply_does_not_start_a_fade() {
        let mut state = EnvironmentState {
            manual_transition_seconds: 4.0,
            ..Default::default()
        };
        let _source = state.ingest_reply(reply(-1, 1234));
        assert!(!state.is_transitioning());
    }

    /// **Only a personal environment is worth saving.** After "Use Shared
    /// Environment" there is nothing to bring back, and a stale blob returning
    /// at the next login would be the setting doing the opposite of its name.
    #[test]
    fn nothing_personal_is_saved_as_nothing() {
        let mut state = EnvironmentState::default();
        assert!(state.saved_environment().is_none());

        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));
        assert!(state.saved_environment().is_some());

        state.set_fixed(None);
        assert!(state.saved_environment().is_none());
    }

    /// **The saved environment round-trips through its JSON**, pin and all —
    /// the whole point of writing it is that the next session renders the same
    /// sky.
    #[test]
    fn a_saved_environment_comes_back_as_it_went() {
        let mut state = EnvironmentState::default();
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);
        state.set_local(
            EnvironmentAsset::Water(WaterSettings::legacy_default("script-water")),
            Some(Uuid::from_u128(0xB1)),
        );
        let decoded = state
            .saved_environment()
            .and_then(|saved| serde_json::to_string(&saved).ok())
            .and_then(|encoded| serde_json::from_str::<SavedEnvironment>(&encoded).ok());
        assert!(
            decoded.is_some(),
            "an edited environment is a personal one, and has to survive its JSON"
        );

        let mut restored = EnvironmentState::default();
        if let Some(decoded) = decoded {
            restored.restore_environment(decoded);
        }

        assert_eq!(restored.local(), state.local());
        assert_eq!(
            restored
                .settings
                .day_cycle
                .sky_frames
                .get("script")
                .map(|sky| sky.gamma),
            Some(3.5)
        );
        assert_eq!(
            restored.local().water().and_then(|track| track.asset),
            Some(Uuid::from_u128(0xB1))
        );
    }

    /// A restored menu pin is the pin, not a sky asset — the check marks have to
    /// come back ticked on the entry the user chose.
    #[test]
    fn a_restored_pin_is_still_a_pin() {
        let mut state = EnvironmentState::default();
        state.set_fixed(Some(FixedEnvironment::Modern(FixedSky::Sunset)));
        let saved = state.saved_environment();
        assert!(saved.is_some(), "a pin is a personal environment");

        let mut restored = EnvironmentState::default();
        if let Some(saved) = saved {
            restored.restore_environment(saved);
        }

        assert_eq!(
            restored.fixed(),
            Some(FixedEnvironment::Modern(FixedSky::Sunset))
        );
    }

    /// `@setenv_daytime` samples the **shared** cycle, not the local layer —
    /// which is always a single frame and has no position to sample.
    #[test]
    fn a_daytime_sky_comes_from_the_shared_cycle() {
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );
        state.set_local(EnvironmentAsset::Sky(Box::new(script_sky())), None);

        let sampled = state.shared_sky_at(0.25);

        assert_ne!(
            sampled.name, "script",
            "the script's own sky is not what a position samples"
        );
    }

    /// An experience's push, as the wire carries one.
    fn push(
        experience: ExperienceKey,
        action: EnvironmentPushAction,
        transition_time: f32,
    ) -> ExperienceEnvironmentPush {
        ExperienceEnvironmentPush {
            experience_id: experience,
            action,
            transition_time,
            owner_id: Uuid::from_u128(0xAA),
            object_name: "Weather Machine".to_owned(),
            parcel_name: "The Back Forty".to_owned(),
        }
    }

    /// A one-key sky fragment, the shape a `PushPartialEnvironment` carries.
    fn sky_fragment(key: &str, value: f64) -> Llsd {
        Llsd::Map(std::collections::HashMap::from([(
            key.to_owned(),
            Llsd::Real(value),
        )]))
    }

    /// The whole point of the layer: taking the push away puts the region's own
    /// environment back **without asking the grid again**, because the region's
    /// settings were only ever covered.
    #[test]
    fn releasing_an_experience_restores_the_region_without_a_refetch() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut state = EnvironmentState::default();
        assert_eq!(
            state.ingest_reply(reply(-1, 1234)),
            EnvironmentSource::Region
        );
        let region_sky = state.rendered_sky();
        assert!(region_sky.is_some(), "the region has a sky to go back to");

        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Full {
                    asset_id: Uuid::from_u128(0x5117),
                },
                0.0,
            ),
            Some(EnvironmentAsset::Sky(Box::new(script_sky()))),
        );
        assert_eq!(
            state.rendered_sky().map(|sky| sky.name),
            Some("script".to_owned()),
            "the pushed sky is what renders"
        );
        assert!(!state.pushed().is_empty());

        state.apply_environment_push(&push(experience, EnvironmentPushAction::Clear, 0.0), None);

        assert!(state.pushed().is_empty(), "the release emptied the layer");
        assert_eq!(
            state.rendered_sky(),
            region_sky,
            "the region's own sky is back, and nothing was re-requested"
        );
        assert_eq!(state.settings.day_length, 1234);
    }

    /// The user outranks the experience: `ENV_LOCAL` sits above `ENV_PUSH`.
    #[test]
    fn a_local_pin_outranks_a_pushed_sky() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut state = EnvironmentState::default();
        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Full {
                    asset_id: Uuid::from_u128(0x5117),
                },
                0.0,
            ),
            Some(EnvironmentAsset::Sky(Box::new(script_sky()))),
        );
        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Midnight)));

        assert_eq!(
            state.rendered_sky().map(|sky| sky.name),
            Some(FixedSky::Midnight.frame_name().to_owned()),
            "the menu's pin renders over the experience's sky"
        );
        assert!(
            !state.pushed().is_empty(),
            "the experience still holds its layer — it is covered, not released"
        );
    }

    /// A partial push overlays only the keys it names, and leaves the day cycle
    /// animating rather than freezing it on one frame.
    #[test]
    fn a_partial_push_changes_one_key_and_keeps_the_cycle() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE2));
        let mut state = EnvironmentState::default();
        state.ingest_shared(two_frame_region_cycle(), EnvironmentSource::Region);
        let before_gamma = state.sky_at(0.0, 0.0).map(|sky| sky.gamma.to_bits());
        assert!(
            before_gamma.is_some(),
            "the region cycle has a ground sky to compare against"
        );

        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Partial {
                    sky: Some(sky_fragment("cloud_shadow", 0.75)),
                    water: None,
                },
                0.0,
            ),
            None,
        );

        let dawn = state.sky_at(0.0, 0.0);
        let dusk = state.sky_at(0.0, 0.5);
        assert_eq!(
            dawn.as_ref().map(|sky| sky.cloud_shadow.to_bits()),
            Some(0.75_f32.to_bits())
        );
        assert_eq!(
            dusk.as_ref().map(|sky| sky.cloud_shadow.to_bits()),
            Some(0.75_f32.to_bits()),
            "the override rides every frame of the cycle"
        );
        assert_ne!(
            dawn.as_ref().map(|sky| sky.name.clone()),
            dusk.as_ref().map(|sky| sky.name.clone()),
            "the cycle still schedules two skies — the push did not pin one"
        );
        assert_eq!(
            dawn.as_ref().map(|sky| sky.gamma.to_bits()),
            before_gamma,
            "a key the push did not name is untouched"
        );
    }

    /// Two experiences hold different parts of the sky, and releasing one leaves
    /// the other's standing — the reason every injection is filed by experience.
    #[test]
    fn releasing_one_experience_leaves_the_others_injection() {
        let first = ExperienceKey::from(Uuid::from_u128(0xE1));
        let second = ExperienceKey::from(Uuid::from_u128(0xE2));
        let mut state = EnvironmentState::default();
        state.apply_environment_push(
            &push(
                first,
                EnvironmentPushAction::Partial {
                    sky: Some(sky_fragment("cloud_shadow", 0.75)),
                    water: None,
                },
                0.0,
            ),
            None,
        );
        state.apply_environment_push(
            &push(
                second,
                EnvironmentPushAction::Partial {
                    sky: Some(sky_fragment("star_brightness", 3.0)),
                    water: None,
                },
                0.0,
            ),
            None,
        );
        assert_eq!(state.pushed().experiences().len(), 2);

        state.apply_environment_push(&push(first, EnvironmentPushAction::Clear, 0.0), None);

        let sky = state.rendered_sky();
        assert_eq!(
            sky.as_ref().map(|sky| sky.star_brightness.to_bits()),
            Some(3.0_f32.to_bits()),
            "the second experience still holds its key"
        );
        assert_ne!(
            sky.as_ref().map(|sky| sky.cloud_shadow.to_bits()),
            Some(0.75_f32.to_bits()),
            "the released experience's key is gone"
        );
        assert_eq!(
            state.pushed().experiences().into_iter().collect::<Vec<_>>(),
            vec![second]
        );
    }

    /// A `ClearEnvironment` with a nil experience id is the reference's "every
    /// experience", not "the experience whose id is nil".
    #[test]
    fn a_nil_experience_clear_releases_every_experience() {
        let first = ExperienceKey::from(Uuid::from_u128(0xE1));
        let second = ExperienceKey::from(Uuid::from_u128(0xE2));
        let mut state = EnvironmentState::default();
        for experience in [first, second] {
            state.apply_environment_push(
                &push(
                    experience,
                    EnvironmentPushAction::Partial {
                        sky: Some(sky_fragment("cloud_shadow", 0.5)),
                        water: None,
                    },
                    0.0,
                ),
                None,
            );
        }

        state.apply_environment_push(
            &push(
                ExperienceKey::from(Uuid::nil()),
                EnvironmentPushAction::Clear,
                0.0,
            ),
            None,
        );

        assert!(state.pushed().is_empty());
    }

    /// A push states how long its change takes; that number, not the viewer's
    /// manual transition time, is what fades. A whole settings asset going in is
    /// a whole-environment cross-fade, the reference's `animateSkyChange`.
    #[test]
    fn a_push_fades_over_its_own_transition_time() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut state = EnvironmentState::default();
        assert_eq!(state.manual_transition_seconds.to_bits(), 0.0_f32.to_bits());

        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Full {
                    asset_id: Uuid::from_u128(0x5117),
                },
                4.0,
            ),
            Some(EnvironmentAsset::Sky(Box::new(script_sky()))),
        );

        assert!(
            state.is_transitioning(),
            "a manual transition time of zero must not suppress the push's own fade"
        );
    }

    /// **A partial push fades the key it names, and only that key.**
    ///
    /// The reference schedules an `Injection` per key above its `0.1` second
    /// threshold and interpolates each one on its own each tick; it does *not*
    /// cross-fade the whole environment, which would drag every other key along
    /// with the one the script asked for.
    #[test]
    fn a_partial_push_blends_per_key_rather_than_cross_fading_the_sky() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut state = EnvironmentState::default();
        state.ingest_shared(two_frame_region_cycle(), EnvironmentSource::Region);
        let before = state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow);
        assert!(before.is_some(), "the region cycle has a sky to blend from");
        assert_ne!(before, Some(0.75), "…and it does not already say 0.75");

        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Partial {
                    sky: Some(sky_fragment("cloud_shadow", 0.75)),
                    water: None,
                },
                4.0,
            ),
            None,
        );

        assert!(
            !state.is_transitioning(),
            "a partial push must not cross-fade the whole environment"
        );
        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow),
            before,
            "at mix zero the key still reads the value underneath it"
        );

        // Halfway: the key is halfway, and nothing else moved.
        assert!(state.push.advance_blends(2.0));
        state.apply();
        let halfway = state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow);
        let expected = before.map(|before| before + (0.75 - before) * 0.5);
        assert!(
            halfway
                .zip(expected)
                .is_some_and(|(got, want)| (got - want).abs() < 1e-5),
            "the key blends toward the pushed value: {halfway:?} against {expected:?}"
        );

        // Done: the key holds what was pushed and the blend is over.
        assert!(state.push.advance_blends(2.5));
        state.apply();
        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow),
            Some(0.75)
        );
        assert!(!state.push.is_blending(), "the blend finished");
    }

    /// **A release fades the key back out rather than snapping it.**
    ///
    /// The reference turns each override it takes away into a blend-out
    /// injection (`removeInjection`), and drops the experience from its active
    /// set at once — the fade outlives the experience, not the other way round.
    #[test]
    fn a_released_key_fades_back_out_and_the_experience_is_gone_at_once() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut state = EnvironmentState::default();
        state.ingest_shared(two_frame_region_cycle(), EnvironmentSource::Region);
        let before = state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow);

        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Partial {
                    sky: Some(sky_fragment("cloud_shadow", 0.75)),
                    water: None,
                },
                0.0,
            ),
            None,
        );
        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow),
            Some(0.75),
            "an instant push writes straight through"
        );

        state.apply_environment_push(&push(experience, EnvironmentPushAction::Clear, 4.0), None);

        assert!(
            state.pushed().experiences().is_empty(),
            "the experience holds nothing the moment it releases"
        );
        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow),
            Some(0.75),
            "…but its value is still on screen while the fade runs"
        );

        assert!(state.push.advance_blends(4.5));
        state.apply();
        assert_eq!(
            state.sky_at(0.0, 0.0).map(|sky| sky.cloud_shadow),
            before,
            "the value underneath is back once the fade is over"
        );
        assert!(state.pushed().is_empty(), "and the layer is empty again");
    }

    /// The personal environment is the user's; an experience's push is not part
    /// of it and must not be written to the account.
    #[test]
    fn a_pushed_environment_is_never_saved() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut state = EnvironmentState::default();
        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Full {
                    asset_id: Uuid::from_u128(0x5117),
                },
                0.0,
            ),
            Some(EnvironmentAsset::Sky(Box::new(script_sky()))),
        );

        assert!(
            state.saved_environment().is_none(),
            "a sky an experience imposed is not a personal environment"
        );
    }

    /// An app running the two experience-recheck systems and nothing else.
    fn recheck_app() -> App {
        let mut app = App::new();
        app.add_message::<SlEvent>();
        app.add_message::<SlCommand>();
        app.init_resource::<EnvironmentState>();
        app.add_systems(
            Update,
            (
                super::query_parcel_experiences,
                super::ingest_parcel_experiences,
            )
                .chain(),
        );
        app
    }

    /// Every `ExperienceQuery` the systems asked for in one run, drained for the
    /// same reason [`requests`] drains.
    fn queries(app: &mut App) -> Vec<(i32, Vec<ExperienceKey>)> {
        app.update();
        app.world_mut()
            .resource_mut::<Messages<SlCommand>>()
            .drain()
            .filter_map(|command| match command.0 {
                Command::QueryParcelExperiences {
                    parcel_id,
                    experiences,
                } => Some((parcel_id, experiences)),
                _ => None,
            })
            .collect()
    }

    /// Install a pushed sky for `experience` in `state`, instantly.
    fn inject_sky(state: &mut EnvironmentState, experience: ExperienceKey) {
        state.apply_environment_push(
            &push(
                experience,
                EnvironmentPushAction::Full {
                    asset_id: Uuid::from_u128(0x5117),
                },
                0.0,
            ),
            Some(EnvironmentAsset::Sky(Box::new(script_sky()))),
        );
    }

    /// **Stepping over a parcel line asks whether the sky may come along.**
    ///
    /// An experience is admitted per land, so the parcel change is the moment
    /// its injection may stop being allowed — and nothing is asked while nothing
    /// is injecting, because the reference hangs the hook off the injection
    /// itself.
    #[test]
    fn a_parcel_change_asks_about_the_injecting_experiences() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut app = recheck_app();

        // Standing still on parcel 1 with nothing injected: no question.
        app.world_mut()
            .resource_mut::<EnvironmentState>()
            .set_standing_parcel(Some(1));
        assert_eq!(queries(&mut app), Vec::new());

        // A push arrives; still no question — the agent has not moved.
        inject_sky(
            &mut app.world_mut().resource_mut::<EnvironmentState>(),
            experience,
        );
        assert_eq!(queries(&mut app), Vec::new());

        // Now it walks onto parcel 2.
        app.world_mut()
            .resource_mut::<EnvironmentState>()
            .set_standing_parcel(Some(2));
        assert_eq!(queries(&mut app), vec![(2, vec![experience])]);
        // And the question is asked once, not every frame it stands there.
        assert_eq!(queries(&mut app), Vec::new());
    }

    /// **A parcel that refuses an experience takes its sky back.**
    ///
    /// The region's own environment returns with no refetch, exactly as an
    /// explicit release does — the settings underneath were only covered.
    #[test]
    fn a_refused_experience_loses_its_sky_and_the_region_returns() {
        let refused = ExperienceKey::from(Uuid::from_u128(0xE1));
        let allowed = ExperienceKey::from(Uuid::from_u128(0xE2));
        let mut app = recheck_app();
        let region_sky = {
            let mut state = app.world_mut().resource_mut::<EnvironmentState>();
            assert_eq!(
                state.ingest_reply(reply(-1, 1234)),
                EnvironmentSource::Region
            );
            let region_sky = state.rendered_sky();
            state.set_standing_parcel(Some(1));
            state.apply_environment_push(
                &push(
                    allowed,
                    EnvironmentPushAction::Partial {
                        sky: Some(sky_fragment("star_brightness", 3.0)),
                        water: None,
                    },
                    0.0,
                ),
                None,
            );
            inject_sky(&mut state, refused);
            state.set_standing_parcel(Some(2));
            region_sky
        };
        assert_eq!(
            queries(&mut app)
                .into_iter()
                .map(|(parcel, _)| parcel)
                .collect::<Vec<_>>(),
            vec![2]
        );

        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::ParcelExperiences {
                parcel_id: 2,
                experiences: vec![(refused, false), (allowed, true)],
            }));
        app.update();

        let state = app.world().resource::<EnvironmentState>();
        assert_eq!(
            state.pushed().experiences().into_iter().collect::<Vec<_>>(),
            vec![allowed],
            "only the experience the parcel refused was released"
        );
        assert_eq!(
            state.rendered_sky().map(|sky| sky.name),
            region_sky.map(|sky| sky.name),
            "the region's own sky is back under the surviving experience's key"
        );
    }

    /// **An answer about land the agent has already left changes nothing.**
    ///
    /// The reply names only experiences, so without matching it against the
    /// parcel the agent is on *now* a refusal from the parcel behind them would
    /// take away a sky the one they are standing on allows.
    #[test]
    fn an_answer_for_a_parcel_already_left_is_ignored() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut app = recheck_app();
        {
            let mut state = app.world_mut().resource_mut::<EnvironmentState>();
            state.set_standing_parcel(Some(1));
            inject_sky(&mut state, experience);
            state.set_standing_parcel(Some(2));
            // …and straight on again, before the answer for parcel 2 lands.
            state.set_standing_parcel(Some(3));
        }
        let _asked = queries(&mut app);

        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::ParcelExperiences {
                parcel_id: 2,
                experiences: vec![(experience, false)],
            }));
        app.update();

        assert!(
            !app.world()
                .resource::<EnvironmentState>()
                .pushed()
                .is_empty(),
            "a refusal from the parcel behind the agent must not clear the sky here"
        );
    }

    /// **An injected sky does not cross a region line.**
    ///
    /// The reference drops every injection at a region change unconditionally,
    /// and it has to: the script holding the sky is back where the agent left
    /// it, and the destination has its own idea of which experiences it admits.
    #[test]
    fn a_region_change_drops_every_injected_environment() {
        let experience = ExperienceKey::from(Uuid::from_u128(0xE1));
        let mut app = env_app();
        {
            let mut state = app.world_mut().resource_mut::<EnvironmentState>();
            assert_eq!(
                state.ingest_reply(reply(-1, 1234)),
                EnvironmentSource::Region
            );
            state.set_standing_parcel(Some(1));
            inject_sky(&mut state, experience);
            assert!(!state.pushed().is_empty());
        }

        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::RegionHandshakeComplete));
        app.update();

        let state = app.world().resource::<EnvironmentState>();
        assert!(
            state.pushed().is_empty(),
            "the experience's sky did not survive the crossing"
        );
        assert_eq!(
            state.standing_parcel(),
            None,
            "parcel ids are region-local; the one left behind names nothing here"
        );
    }
}
