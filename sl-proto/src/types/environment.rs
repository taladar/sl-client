//! Extended Environment (EEP): a region's or parcel's sky, water, and day-cycle
//! settings, parsed from the `ExtEnvironment` capability.
//!
//! The environment is a **day cycle**: a set of *tracks* (one for water, up to
//! four for the sky at increasing altitudes) that schedule named *frames* over
//! the course of a day, plus the [`SkySettings`] / [`WaterSettings`] frame
//! definitions the tracks reference.
//!
//! The deep atmospheric-scattering profiles (`rayleigh_config`, `mie_config`,
//! `absorption_config`) are carried as [`DensityLayer`] lists rather than
//! interpreted: nothing here renders from them — this workspace's renderer
//! takes its atmosphere from the legacy haze block — but carrying them is not
//! optional in either direction. An editor that saves a sky asset back over the
//! item it came from must not silently drop them, and the reference viewer's
//! sky validator marks all three **required with no default**, so a sky frame
//! missing them fails validation, takes its whole day cycle down with it (`Must
//! have at least one water and one sky frame!`), and leaves the region with no
//! environment at all. Every other documented sky/water parameter is parsed.

use std::collections::BTreeMap;

use sl_types::key::TextureKey;
use sl_types::lsl::Rotation;
use uuid::Uuid;

// `Color`, `ColorAlpha`, `Glow`, and `CloudPosDensity` now live in
// `sl_types::environment`, and the 3-axis `Scale` factor in `sl_types::map`;
// they are re-exported here so the existing `sl_proto::…` paths are unchanged.
// The LLSD codec helpers below stay client-local.
pub use sl_types::environment::{CloudPosDensity, Color, ColorAlpha, Glow};
pub use sl_types::map::Scale;

/// A region's or parcel's environment, parsed from the `ExtEnvironment`
/// capability (the reply to
/// [`Command::RequestEnvironment`](crate::Command::RequestEnvironment), delivered
/// as [`Event::Environment`](crate::Event::Environment)).
///
/// (Not `Eq`: it ultimately holds `f32` settings.)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvironmentSettings {
    /// The parcel these settings apply to, or `-1` for the whole region.
    pub parcel_id: i32,
    /// The region the settings came from (nil if the grid omitted it).
    pub region_id: Uuid,
    /// The length of a full day, in seconds.
    pub day_length: i32,
    /// The day-cycle phase offset, in seconds.
    pub day_offset: i32,
    /// The raw environment behaviour flags (e.g. whether parcels may override the
    /// region environment).
    pub flags: u32,
    /// The environment settings version the grid reported.
    pub env_version: i32,
    /// The three altitude breakpoints, in metres, at which the sky switches from
    /// one [`DayCycle::sky_tracks`] entry to the next.
    pub track_altitudes: [f32; 3],
    /// The day cycle: its schedule of sky/water frames and the frames themselves.
    pub day_cycle: DayCycle,
}

/// A partial environment update: the body of an `ExtEnvironment` PUT (the
/// reference viewer's `coroUpdateEnvironment`), published via
/// [`Command::SetEnvironment`](crate::Command::SetEnvironment). Every field is
/// optional except `flags`; a field is only sent (and only applied) when
/// `Some`. A full day cycle is carried inline in [`Self::day_cycle`], or
/// referenced by settings-asset id in [`Self::day_asset`] (with an optional
/// display name in [`Self::day_name`]).
///
/// (Not `Eq`: holds `f32` settings.)
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvironmentUpdate {
    /// The new length of a full day, in seconds, if changed.
    pub day_length: Option<i32>,
    /// The new day-cycle phase offset, in seconds, if changed.
    pub day_offset: Option<i32>,
    /// The new sky-track altitude breakpoints, in metres, if changed
    /// (region-scope updates only).
    pub track_altitudes: Option<[f32; 3]>,
    /// The new day cycle, carried inline, if changed.
    pub day_cycle: Option<DayCycle>,
    /// The new day cycle, referenced as a settings asset, if changed.
    pub day_asset: Option<Uuid>,
    /// The display name accompanying [`Self::day_asset`].
    pub day_name: Option<String>,
    /// The raw environment behaviour flags to store.
    pub flags: u32,
}

/// A day cycle: the tracks scheduling named frames over a day, plus the frame
/// definitions the tracks reference by name.
///
/// The sky and water frames are split into two maps here, but on the wire they
/// share **one** `frames` map keyed by name (`LLSettingsDay`'s own layout, which
/// both the `ExtEnvironment` envelope and a day-cycle settings asset carry). So
/// a sky frame and a water frame with the same name collide: the encoder emits
/// one map entry and only the last one written survives the round trip. Name
/// them apart.
///
/// (Not `Eq`: the frames hold `f32` settings.)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DayCycle {
    /// The cycle's name.
    pub name: String,
    /// The water track (track 0): keyframes naming [`Self::water_frames`] entries.
    pub water_track: Vec<DayCycleFrame>,
    /// The sky tracks (tracks 1+), ground up. Index 0 is the surface track; later
    /// entries take effect above the matching
    /// [`EnvironmentSettings::track_altitudes`] breakpoint. Each keyframe names a
    /// [`Self::sky_frames`] entry.
    pub sky_tracks: Vec<Vec<DayCycleFrame>>,
    /// The named sky frames the sky tracks reference.
    pub sky_frames: BTreeMap<String, SkySettings>,
    /// The named water frames the water track references.
    pub water_frames: BTreeMap<String, WaterSettings>,
}

/// How many **sky** tracks a day cycle carries: the surface track plus the three
/// altitude tracks above it (the reference's `LLSettingsDay::TRACK_MAX` minus the
/// water track).
pub const SKY_TRACK_COUNT: usize = 4;

/// Two keyframes closer together than this are the *same* keyframe as far as a
/// day cycle is concerned: an insert refuses, and a nearby-lookup answers with
/// the existing one. The reference's `LLSettingsDay::DEFAULT_FRAME_SLOP_FACTOR`.
pub const KEYFRAME_SLOP: f32 = 0.02501;

/// Which track of a [`DayCycle`] a keyframe belongs to.
///
/// The reference numbers its five tracks `0..=4` with water at zero, while
/// [`DayCycle::sky_tracks`] is a list of the sky tracks alone — so a bare index
/// means two different tracks depending on which side of that boundary wrote it.
/// This is that boundary, named once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DayTrack {
    /// The single region-wide water track (the reference's track 0).
    Water,
    /// One of the [`SKY_TRACK_COUNT`] sky tracks, indexed from the ground up:
    /// `0` is the surface track (the reference's track 1), and the rest take
    /// effect above the matching
    /// [`EnvironmentSettings::track_altitudes`] breakpoint.
    Sky(usize),
}

impl DayTrack {
    /// The surface sky track — where a day cycle with no altitude bands lives.
    pub const GROUND: Self = Self::Sky(0);

    /// Every track, water first, in the reference's order.
    #[must_use]
    pub const fn all() -> [Self; SKY_TRACK_COUNT.saturating_add(1)] {
        [
            Self::Water,
            Self::Sky(0),
            Self::Sky(1),
            Self::Sky(2),
            Self::Sky(3),
        ]
    }

    /// The [`DayCycle::sky_tracks`] index this names, or `None` for the water
    /// track.
    #[must_use]
    pub const fn sky_index(self) -> Option<usize> {
        match self {
            Self::Water => None,
            Self::Sky(index) => Some(index),
        }
    }

    /// The reference's own track number (`0` water, `1..=4` sky), which is what
    /// its notifications and its saved settings talk in.
    #[must_use]
    pub const fn reference_index(self) -> usize {
        match self {
            Self::Water => 0,
            Self::Sky(index) => index.saturating_add(1),
        }
    }

    /// The track the reference's number `index` names, or `None` past the last
    /// sky track.
    #[must_use]
    pub const fn from_reference_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Water),
            _ if index <= SKY_TRACK_COUNT => Some(Self::Sky(index.saturating_sub(1))),
            _ => None,
        }
    }

    /// Whether this track may be emptied completely.
    ///
    /// The water track and the surface sky track are what a cycle *is* — the
    /// reference keeps their first keyframe and clears the rest — while an
    /// altitude track above them is allowed to hold nothing at all, which is how
    /// a cycle says "no separate sky up here".
    #[must_use]
    pub const fn may_be_empty(self) -> bool {
        match self {
            Self::Water | Self::Sky(0) => false,
            Self::Sky(_other) => true,
        }
    }
}

/// One keyframe within a day-cycle track: a named frame and the time of day it
/// is reached.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DayCycleFrame {
    /// The time of day this frame is reached, as a fraction of the day in
    /// `0.0..=1.0`.
    pub keyframe: f32,
    /// The name of the [`SkySettings`] / [`WaterSettings`] frame applied at this
    /// keyframe (a key into [`DayCycle::sky_frames`] / [`DayCycle::water_frames`]).
    pub name: String,
}

impl DayCycle {
    /// The keyframes on `track`, in keyframe order. An altitude track a cycle
    /// does not carry reads as empty rather than missing, which is what it
    /// means.
    #[must_use]
    pub fn track(&self, track: DayTrack) -> &[DayCycleFrame] {
        match track.sky_index() {
            None => &self.water_track,
            Some(index) => self.sky_tracks.get(index).map_or(&[], Vec::as_slice),
        }
    }

    /// The keyframes on `track`, mutably, materialising the sky tracks up to it
    /// so a cycle that carried only a ground track can grow an altitude one.
    /// `None` past the last sky track.
    fn track_mut(&mut self, track: DayTrack) -> Option<&mut Vec<DayCycleFrame>> {
        match track.sky_index() {
            None => Some(&mut self.water_track),
            Some(index) if index < SKY_TRACK_COUNT => {
                if self.sky_tracks.len() <= index {
                    self.sky_tracks
                        .resize_with(index.saturating_add(1), Vec::new);
                }
                self.sky_tracks.get_mut(index)
            }
            Some(_past_the_end) => None,
        }
    }

    /// The index into [`track`](Self::track) of the keyframe within `slop` of
    /// `position`, nearest first — the reference's `getSettingsNearKeyframe`.
    ///
    /// The day wraps, so a keyframe just before midnight is near a position just
    /// after it.
    #[must_use]
    pub fn keyframe_near(&self, track: DayTrack, position: f32, slop: f32) -> Option<usize> {
        self.track(track)
            .iter()
            .enumerate()
            .map(|(index, frame)| (index, wrapped_distance(frame.keyframe, position)))
            .filter(|(_index, distance)| *distance <= slop)
            .min_by(|(_a, left), (_b, right)| left.total_cmp(right))
            .map(|(index, _distance)| index)
    }

    /// A frame name no frame in this cycle already answers to, derived from
    /// `base` (`"Sunrise"`, `"Sunrise (2)"`, …).
    ///
    /// One namespace, not two: the sky frames and the water frames share a
    /// single `frames` map on the wire (see [`DayCycle`]), so a water frame named
    /// like a sky frame is a frame that does not survive being saved.
    #[must_use]
    pub fn unique_frame_name(&self, base: &str) -> String {
        let taken =
            |name: &str| self.sky_frames.contains_key(name) || self.water_frames.contains_key(name);
        let base = if base.is_empty() { "Frame" } else { base };
        if !taken(base) {
            return base.to_owned();
        }
        // Bounded by the number of frames plus one, so some candidate in the
        // sequence is always free.
        let ceiling = self
            .sky_frames
            .len()
            .saturating_add(self.water_frames.len())
            .saturating_add(2);
        for suffix in 2..=ceiling {
            let candidate = format!("{base} ({suffix})");
            if !taken(&candidate) {
                return candidate;
            }
        }
        base.to_owned()
    }

    /// Put `sky` on sky `track` at `position`, under a fresh name derived from
    /// its own. Returns the name it was filed under, or `None` when the track is
    /// the water track, does not exist, or already holds a keyframe within
    /// [`KEYFRAME_SLOP`].
    pub fn insert_sky_keyframe(
        &mut self,
        track: DayTrack,
        position: f32,
        sky: SkySettings,
    ) -> Option<String> {
        if track.sky_index().is_none()
            || self.keyframe_near(track, position, KEYFRAME_SLOP).is_some()
        {
            return None;
        }
        let name = self.unique_frame_name(&sky.name);
        let mut sky = sky;
        name.clone_into(&mut sky.name);
        drop(self.sky_frames.insert(name.clone(), sky));
        self.insert_at(track, position, &name)?;
        Some(name)
    }

    /// Put `water` on the water track at `position`, under a fresh name derived
    /// from its own. `None` for a sky track or a position already taken.
    pub fn insert_water_keyframe(
        &mut self,
        track: DayTrack,
        position: f32,
        water: WaterSettings,
    ) -> Option<String> {
        if track.sky_index().is_some()
            || self.keyframe_near(track, position, KEYFRAME_SLOP).is_some()
        {
            return None;
        }
        let name = self.unique_frame_name(&water.name);
        let mut water = water;
        name.clone_into(&mut water.name);
        drop(self.water_frames.insert(name.clone(), water));
        self.insert_at(track, position, &name)?;
        Some(name)
    }

    /// File a keyframe naming `frame` at `position`, keeping the track sorted.
    fn insert_at(&mut self, track: DayTrack, position: f32, frame: &str) -> Option<()> {
        let keyframes = self.track_mut(track)?;
        let at = keyframes
            .iter()
            .position(|existing| existing.keyframe > position)
            .unwrap_or(keyframes.len());
        keyframes.insert(
            at,
            DayCycleFrame {
                keyframe: position.clamp(0.0, 1.0),
                name: frame.to_owned(),
            },
        );
        Some(())
    }

    /// Move the `index`-th keyframe of `track` to `position`, keeping the track
    /// sorted. Refuses a move onto another keyframe (within [`KEYFRAME_SLOP`]),
    /// and answers with where the keyframe ended up.
    pub fn move_keyframe(&mut self, track: DayTrack, index: usize, position: f32) -> Option<usize> {
        let position = position.clamp(0.0, 1.0);
        let blocked = self
            .keyframe_near(track, position, KEYFRAME_SLOP)
            .is_some_and(|near| near != index);
        if blocked {
            return None;
        }
        let keyframes = self.track_mut(track)?;
        let mut moved = keyframes.get(index).cloned()?;
        moved.keyframe = position;
        drop(keyframes.remove(index));
        let at = keyframes
            .iter()
            .position(|existing| existing.keyframe > position)
            .unwrap_or(keyframes.len());
        keyframes.insert(at, moved);
        Some(at)
    }

    /// Take the `index`-th keyframe off `track`, and the frame it named with it
    /// if nothing else still names it.
    ///
    /// Refuses to empty a track that [may not be empty](DayTrack::may_be_empty),
    /// as the reference refuses: a cycle with no water at all, or no sky at
    /// ground level, is not a cycle anything can render.
    pub fn remove_keyframe(&mut self, track: DayTrack, index: usize) -> bool {
        if !track.may_be_empty() && self.track(track).len() <= 1 {
            return false;
        }
        let Some(keyframes) = self.track_mut(track) else {
            return false;
        };
        if index >= keyframes.len() {
            return false;
        }
        let removed = keyframes.remove(index);
        self.forget_unreferenced(&removed.name);
        true
    }

    /// Empty `track` as far as it is allowed to be emptied: an altitude track
    /// entirely, the water and ground tracks down to their first keyframe (the
    /// reference's `onClearTrack`).
    pub fn clear_track(&mut self, track: DayTrack) {
        let keep = usize::from(!track.may_be_empty());
        let Some(keyframes) = self.track_mut(track) else {
            return;
        };
        let dropped: Vec<String> = keyframes
            .split_off(keep.min(keyframes.len()))
            .into_iter()
            .map(|frame| frame.name)
            .collect();
        for name in dropped {
            self.forget_unreferenced(&name);
        }
    }

    /// Replace `into`'s keyframes with **copies** of `source`'s `from` track,
    /// under fresh names — the reference's `cloneTrack`, which clones each frame
    /// rather than referencing it so editing one track cannot change another.
    ///
    /// `source` may be this cycle (copying one of its own tracks) or another one
    /// loaded from inventory. Sky and water do not mix: a water track copied
    /// into a sky track would name frames of the wrong kind, and the reference
    /// refuses it with `TrackLoadMismatch`.
    pub fn clone_track(&mut self, source: &Self, from: DayTrack, into: DayTrack) -> bool {
        if from.sky_index().is_none() != into.sky_index().is_none() {
            return false;
        }
        if self.track_mut(into).is_none() {
            return false;
        }
        let copied: Vec<DayCycleFrame> = source.track(from).to_vec();
        // Clear first: the source can be empty, and a partial overwrite would
        // leave the destination holding a mixture of both tracks.
        self.clear_whole_track(into);
        for frame in copied {
            match from.sky_index() {
                Some(_sky) => {
                    let Some(sky) = source.sky_frames.get(&frame.name).cloned() else {
                        continue;
                    };
                    drop(self.insert_sky_keyframe(into, frame.keyframe, sky));
                }
                None => {
                    let Some(water) = source.water_frames.get(&frame.name).cloned() else {
                        continue;
                    };
                    drop(self.insert_water_keyframe(into, frame.keyframe, water));
                }
            }
        }
        true
    }

    /// Give the `index`-th keyframe of `track` a frame of its own, when the one
    /// it names is shared with another keyframe.
    ///
    /// A frame is referenced *by name*, and one asset may legally name the same
    /// frame from two keyframes — so an editor writing a knob into "the selected
    /// keyframe's frame" would silently change the other one too. Splitting is
    /// what makes a keyframe editable in isolation. Answers with the name the
    /// keyframe holds afterwards, whether or not it had to change.
    pub fn split_shared_frame(&mut self, track: DayTrack, index: usize) -> Option<String> {
        let name = self.track(track).get(index)?.name.clone();
        if self.references(&name) <= 1 {
            return Some(name);
        }
        let fresh = self.unique_frame_name(&name);
        match track.sky_index() {
            Some(_sky) => {
                let mut sky = self.sky_frames.get(&name)?.clone();
                fresh.clone_into(&mut sky.name);
                drop(self.sky_frames.insert(fresh.clone(), sky));
            }
            None => {
                let mut water = self.water_frames.get(&name)?.clone();
                fresh.clone_into(&mut water.name);
                drop(self.water_frames.insert(fresh.clone(), water));
            }
        }
        let keyframes = self.track_mut(track)?;
        fresh.clone_into(&mut keyframes.get_mut(index)?.name);
        Some(fresh)
    }

    /// The **blended** sky on sky track `index` at day `position`, the
    /// day-cycle interpolation [`EnvironmentSettings::blended_sky_settings`]
    /// renders through once an altitude has chosen the track.
    #[must_use]
    pub fn blended_sky(&self, index: usize, position: f32) -> Option<SkySettings> {
        let blended = self.sky_tracks.get(index).and_then(|track| {
            let (lower, upper, factor) = bounding_keyframes(track, position)?;
            let lower_sky = self.sky_frames.get(&lower.name)?;
            // If the upper frame is missing, hold the lower one rather than
            // falling through to an unrelated frame.
            match self.sky_frames.get(&upper.name) {
                Some(upper_sky) => Some(lower_sky.blend(upper_sky, factor)),
                None => Some(lower_sky.clone()),
            }
        });
        blended.or_else(|| self.sky_frames.values().next().cloned())
    }

    /// The **blended** water at day `position` — the water counterpart of
    /// [`blended_sky`](Self::blended_sky), on the one region-wide track.
    #[must_use]
    pub fn blended_water(&self, position: f32) -> Option<WaterSettings> {
        let blended =
            bounding_keyframes(&self.water_track, position).and_then(|(lower, upper, factor)| {
                let lower_water = self.water_frames.get(&lower.name)?;
                match self.water_frames.get(&upper.name) {
                    Some(upper_water) => Some(lower_water.blend(upper_water, factor)),
                    None => Some(lower_water.clone()),
                }
            });
        blended.or_else(|| self.water_frames.values().next().cloned())
    }

    /// Empty a track completely, whatever it is — the destination half of a
    /// clone, which is about to be refilled.
    fn clear_whole_track(&mut self, track: DayTrack) {
        let Some(keyframes) = self.track_mut(track) else {
            return;
        };
        let dropped: Vec<String> = std::mem::take(keyframes)
            .into_iter()
            .map(|frame| frame.name)
            .collect();
        for name in dropped {
            self.forget_unreferenced(&name);
        }
    }

    /// How many keyframes, across every track, name `frame`.
    fn references(&self, frame: &str) -> usize {
        DayTrack::all()
            .into_iter()
            .map(|track| {
                self.track(track)
                    .iter()
                    .filter(|keyframe| keyframe.name == frame)
                    .count()
            })
            .sum()
    }

    /// Drop `frame`'s definition when no keyframe names it any more, so a saved
    /// asset does not grow a frame for every edit ever made.
    fn forget_unreferenced(&mut self, frame: &str) {
        if self.references(frame) > 0 {
            return;
        }
        drop(self.sky_frames.remove(frame));
        drop(self.water_frames.remove(frame));
    }
}

/// How far apart two normalised times of day are, the shorter way round the
/// clock — so `0.99` and `0.01` are `0.02` apart rather than `0.98`.
fn wrapped_distance(a: f32, b: f32) -> f32 {
    let raw = (a - b).abs();
    raw.min(1.0 - raw)
}

/// One layer of an atmospheric **density profile**: how much of a scattering
/// species is present at a given altitude, in the shape the reference's
/// `rayleigh_config`, `mie_config` and `absorption_config` arrays carry.
///
/// A layer's density is `exp_term * exp(exp_scale * h) + linear_term * h +
/// constant_term`, and [`width`](Self::width) is how far up it applies (zero
/// meaning "the rest of the atmosphere"). Ozone is the reason a profile is a
/// *list*: its absorption ramps up and then down again, which one layer cannot
/// say.
///
/// (Not `Eq`: holds `f32` fields.)
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DensityLayer {
    /// How far up this layer applies, in metres; `0.0` means the whole of the
    /// remaining atmosphere.
    pub width: f32,
    /// The coefficient of the exponential term.
    pub exp_term: f32,
    /// The scale inside the exponential, per metre (negative: density falls
    /// with altitude).
    pub exp_scale: f32,
    /// The coefficient of the linear term, per metre.
    pub linear_term: f32,
    /// The constant term.
    pub constant_term: f32,
    /// The Mie phase function's anisotropy, which only a `mie_config` layer
    /// carries — the reference omits the key entirely when it is zero, and its
    /// presence is what distinguishes a Mie layer on the wire.
    pub anisotropy: Option<f32>,
}

impl DensityLayer {
    /// The reference's default `rayleigh_config`
    /// (`LLSettingsSky::rayleighConfigDefault`): one layer, falling off with an
    /// 8 km scale height.
    #[must_use]
    pub fn rayleigh_default() -> Vec<Self> {
        vec![Self {
            width: 0.0,
            exp_term: 1.0,
            exp_scale: -1.0 / 8000.0,
            linear_term: 0.0,
            constant_term: 0.0,
            anisotropy: None,
        }]
    }

    /// The reference's default `mie_config`
    /// (`LLSettingsSky::mieConfigDefault`): one layer with a 1.2 km scale
    /// height and a forward-scattering anisotropy of `0.8`.
    #[must_use]
    pub fn mie_default() -> Vec<Self> {
        vec![Self {
            width: 0.0,
            exp_term: 1.0,
            exp_scale: -1.0 / 1200.0,
            linear_term: 0.0,
            constant_term: 0.0,
            anisotropy: Some(0.8),
        }]
    }

    /// The reference's default `absorption_config`
    /// (`LLSettingsSky::absorptionConfigDefault`): the ozone layer's two linear
    /// ramps, up to 25 km and then above it.
    #[must_use]
    pub fn absorption_default() -> Vec<Self> {
        vec![
            Self {
                width: 25000.0,
                exp_term: 0.0,
                exp_scale: 0.0,
                linear_term: -1.0 / 25000.0,
                constant_term: -2.0 / 3.0,
                anisotropy: None,
            },
            Self {
                width: 0.0,
                exp_term: 0.0,
                exp_scale: 0.0,
                linear_term: -1.0 / 15000.0,
                constant_term: 8.0 / 3.0,
                anisotropy: None,
            },
        ]
    }
}

/// A single sky frame (`LLSettingsSky`): the atmosphere, sun, moon, and cloud
/// state at one keyframe. The legacy haze colours/scalars (`ambient`,
/// `blue_horizon`, `blue_density`, `haze_*`, the multipliers) are read from the
/// frame's `legacy_haze` block.
///
/// (Not `Eq`: holds `f32` fields.)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkySettings {
    /// The frame's name.
    pub name: String,
    /// The sun's orientation.
    pub sun_rotation: Rotation,
    /// The moon's orientation.
    pub moon_rotation: Rotation,
    /// The sunlight colour, RGBA.
    pub sunlight_color: ColorAlpha,
    /// The ambient light colour, RGB (from `legacy_haze`).
    pub ambient: Color,
    /// The horizon blue colour, RGB (from `legacy_haze`).
    pub blue_horizon: Color,
    /// The blue-density colour, RGB (from `legacy_haze`).
    pub blue_density: Color,
    /// The haze horizon factor (from `legacy_haze`).
    pub haze_horizon: f32,
    /// The haze density (from `legacy_haze`).
    pub haze_density: f32,
    /// The atmospheric density multiplier (from `legacy_haze`).
    pub density_multiplier: f32,
    /// The atmospheric distance multiplier (from `legacy_haze`).
    pub distance_multiplier: f32,
    /// The maximum sky dome altitude.
    pub max_y: f32,
    /// The gamma applied to the sky.
    pub gamma: f32,
    /// The reflection-probe ambiance (`reflection_probe_ambiance`): an EEP-only
    /// setting (absent on a legacy / classic-mode sky, decoded as `0.0`) that,
    /// when non-zero, puts the sky into the reference's "fake HDR" path — the WL
    /// sky, clouds, and sun / moon discs are scaled by `sqrt(gamma) * 2` before
    /// tone-mapping (see [`SkySettings::sky_hdr_scale`]).
    pub reflection_probe_ambiance: f32,
    /// The cloud colour, RGB.
    pub cloud_color: Color,
    /// The cloud layer 1 position (X, Y) and density (Z).
    pub cloud_pos_density1: CloudPosDensity,
    /// The cloud layer 2 detail position (X, Y) and density (Z).
    pub cloud_pos_density2: CloudPosDensity,
    /// The cloud scale.
    pub cloud_scale: f32,
    /// The cloud scroll rate (X, Y).
    pub cloud_scroll_rate: [f32; 2],
    /// The cloud shadow / coverage.
    pub cloud_shadow: f32,
    /// The cloud variance.
    pub cloud_variance: f32,
    /// The sun/moon glow (size, unused, focus).
    pub glow: Glow,
    /// The starfield brightness.
    pub star_brightness: f32,
    /// The sun size scale.
    pub sun_scale: f32,
    /// The moon size scale.
    pub moon_scale: f32,
    /// The moon brightness multiplier.
    pub moon_brightness: f32,
    /// The sun's angular diameter, in radians.
    pub sun_arc_radians: f32,
    /// The atmospheric droplet radius.
    pub droplet_radius: f32,
    /// The ice level.
    pub ice_level: f32,
    /// The atmospheric moisture level.
    pub moisture_level: f32,
    /// The atmosphere's outer radius.
    pub sky_top_radius: f32,
    /// The atmosphere's inner radius.
    pub sky_bottom_radius: f32,
    /// The planet radius.
    pub planet_radius: f32,
    /// The sun disc texture (`None` for the viewer default).
    pub sun_texture: Option<TextureKey>,
    /// The moon disc texture (`None` for the viewer default).
    pub moon_texture: Option<TextureKey>,
    /// The cloud texture (`None` for the viewer default).
    pub cloud_texture: Option<TextureKey>,
    /// The bloom texture (`None` for the viewer default).
    pub bloom_texture: Option<TextureKey>,
    /// The halo texture (`None` for the viewer default).
    pub halo_texture: Option<TextureKey>,
    /// The rainbow texture (`None` for the viewer default).
    pub rainbow_texture: Option<TextureKey>,
    /// The sky dome's offset (`dome_offset`), carried and never read.
    ///
    /// The reference stopped reading it — `getSkyDomeOffset` is commented out
    /// and the dome is a constant now — but it is still in
    /// `LLSettingsSky::defaults()`, so every sky it saves carries one. `None`
    /// when the asset holds none, so a frame that never had one does not gain
    /// one by passing through here.
    pub dome_offset: Option<f32>,
    /// The sky dome's radius (`dome_radius`), carried and never read, for the
    /// same reason as [`dome_offset`](Self::dome_offset).
    pub dome_radius: Option<f32>,
    /// The Rayleigh (air molecule) scattering density profile
    /// (`rayleigh_config`), carried verbatim — see [`DensityLayer`].
    pub rayleigh_config: Vec<DensityLayer>,
    /// The Mie (aerosol) scattering density profile (`mie_config`), the one
    /// whose layers carry an [`anisotropy`](DensityLayer::anisotropy).
    pub mie_config: Vec<DensityLayer>,
    /// The absorption (ozone) density profile (`absorption_config`), two
    /// ramping layers in the reference's own default.
    pub absorption_config: Vec<DensityLayer>,
}

/// A decoded EEP settings asset (`AT_SETTINGS`) — a sky frame, a water frame, or
/// a whole day cycle.
///
/// A settings asset tags its own kind, so decoding one yields whichever of the
/// three it is. The single-frame kinds are what the World ▸ Environment presets
/// (the reference viewer's `KNOWN_SKY_*` library skies) are; the day-cycle kind
/// is what an *inventory* environment item usually holds, and what the
/// environment editor saves.
///
/// (Not `Eq`: the settings hold `f32` fields.)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EnvironmentAsset {
    /// A sky settings asset (`type` == `"sky"`). Boxed: [`SkySettings`] is much
    /// larger than [`WaterSettings`], so an unboxed variant makes every
    /// `EnvironmentAsset` the size of a sky (`clippy::large_enum_variant`).
    Sky(Box<SkySettings>),
    /// A water settings asset (`type` == `"water"`).
    Water(WaterSettings),
    /// A day-cycle settings asset (`type` == `"daycycle"`): the named sky and
    /// water frames and the tracks that sequence them. Boxed for the same
    /// reason as [`Sky`](Self::Sky) — a cycle carries whole frames.
    DayCycle(Box<DayCycle>),
}

impl EnvironmentAsset {
    /// Which of the three kinds this decoded asset is — the same distinction
    /// [`SettingsKind`] carries, reached by having parsed the body rather than
    /// by reading an inventory item's flags.
    #[must_use]
    pub const fn kind(&self) -> SettingsKind {
        match *self {
            Self::Sky(_) => SettingsKind::Sky,
            Self::Water(_) => SettingsKind::Water,
            Self::DayCycle(_) => SettingsKind::DayCycle,
        }
    }
}

/// Which kind of settings asset an inventory item holds, carried in the low byte
/// of the item's `flags` (`II_FLAGS_SUBTYPE_MASK`) exactly as a wearable's slot
/// and a script's language are (`LLSettingsType::type_e`).
///
/// This is the *only* way to tell one settings item from another **without
/// fetching it**: an [`EnvironmentAsset`] tags its own kind in its body, but a
/// list of every settings item in inventory cannot afford to download them all
/// to find out what they are. The reference reads the same byte for the same
/// reason (`LLSettingsType::fromInventoryFlags`).
///
/// The reference casts the byte straight to its enum, so an unrecognised one
/// becomes a value no arm of its `switch` matches and the item is logged and
/// dropped; here that is [`None`], which reaches the same outcome by a route
/// that cannot be mistaken for a valid kind.
/// Deliberately **not** `#[non_exhaustive]`, unlike its neighbours: these three
/// are the whole of `LLSettingsType::type_e`, its two sentinels being the
/// [`None`] this type's constructors return rather than kinds. Every consumer
/// files an asset under exactly one of them, and a fourth kind would be a
/// protocol change that ought to break each of those matches rather than fall
/// into a wildcard that quietly drops it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SettingsKind {
    /// A single sky frame (`ST_SKY = 0`).
    Sky,
    /// A single water frame (`ST_WATER = 1`).
    Water,
    /// A whole day cycle (`ST_DAYCYCLE = 2`).
    DayCycle,
}

impl SettingsKind {
    /// The item-`flags` low-byte mask carrying the settings subtype
    /// (`II_FLAGS_SUBTYPE_MASK`) — the same byte
    /// [`ScriptLanguage`](crate::ScriptLanguage) and a wearable's slot use.
    pub const SUBTYPE_MASK: u32 = 0x0000_00ff;

    /// The `LLSettingsType::type_e` byte for this kind.
    #[must_use]
    pub const fn subtype(self) -> u8 {
        match self {
            Self::Sky => 0,
            Self::Water => 1,
            Self::DayCycle => 2,
        }
    }

    /// Classifies an `LLSettingsType::type_e` byte, or `None` for one that names
    /// no kind (the reference's `ST_INVALID` / `ST_NONE` included).
    #[must_use]
    pub const fn from_subtype(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Sky),
            1 => Some(Self::Water),
            2 => Some(Self::DayCycle),
            _ => None,
        }
    }

    /// The kind recorded in an inventory item's `flags`, reading the subtype low
    /// byte ([`SUBTYPE_MASK`](Self::SUBTYPE_MASK)); `None` for an unknown one.
    ///
    /// The caller must already know the item *is* a settings item — every
    /// inventory item has flags, and a wearable's slot byte would be read as a
    /// kind just as happily.
    #[must_use]
    pub fn from_item_flags(flags: u32) -> Option<Self> {
        let byte = u8::try_from(flags & Self::SUBTYPE_MASK).ok()?;
        Self::from_subtype(byte)
    }

    /// The reference's own short name for the kind
    /// (`LLSettingsType::getDefaultName` keys: `"sky"`, `"water"`, `"day"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sky => "sky",
            Self::Water => "water",
            Self::DayCycle => "day",
        }
    }
}

/// A single water frame (`LLSettingsWater`): the surface and underwater state at
/// one keyframe.
///
/// (Not `Eq`: holds `f32` fields.)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WaterSettings {
    /// The frame's name.
    pub name: String,
    /// The reflection blur multiplier.
    pub blur_multiplier: f32,
    /// The Fresnel offset.
    pub fresnel_offset: f32,
    /// The Fresnel scale.
    pub fresnel_scale: f32,
    /// The normal-map (wavelet) scale (X, Y, Z).
    pub normal_scale: Scale,
    /// The normal/wave texture (`None` for the viewer default).
    pub normal_map: Option<TextureKey>,
    /// The refraction scale above the surface.
    pub scale_above: f32,
    /// The refraction scale below the surface.
    pub scale_below: f32,
    /// The transparent-water texture (`None` for the viewer default).
    pub transparent_texture: Option<TextureKey>,
    /// The underwater fog modifier.
    pub underwater_fog_mod: f32,
    /// The water fog colour, RGB.
    pub water_fog_color: Color,
    /// The water fog density exponent.
    pub water_fog_density: f32,
    /// The wave 1 direction (X, Y).
    pub wave1_direction: [f32; 2],
    /// The wave 2 direction (X, Y).
    pub wave2_direction: [f32; 2],
}

/// The name of the sky frame [`EnvironmentSettings::legacy_windlight_default`]
/// defines — and the name a fake grid's stock region environment reuses, so the
/// two agree about what "the default sky" is called.
pub const DEFAULT_SKY_FRAME: &str = "Default";

/// The name of the water frame beside [`DEFAULT_SKY_FRAME`]. Distinct from it
/// because the two kinds share one name namespace on the wire (see
/// [`DayCycle`]).
pub const DEFAULT_WATER_FRAME: &str = "Default Water";

/// The built-in sun-disc texture (`DEFAULT_SUN_ID`, `llsettingssky.cpp`) — what
/// a renderer samples when [`SkySettings::sun_texture`] is `None`.
pub const DEFAULT_SUN_TEXTURE: Uuid = Uuid::from_u128(0x32bf_bcea_24b1_fb9d_1ef9_48a2_8a63_730f);

/// The built-in moon-disc texture (`DEFAULT_MOON_ID`, `llsettingssky.cpp`) —
/// what a renderer samples when [`SkySettings::moon_texture`] is `None`.
pub const DEFAULT_MOON_TEXTURE: Uuid = Uuid::from_u128(0xd07f_6eed_b96a_47cd_b51d_400a_d4a1_c428);

/// The built-in cloud-noise texture (`DEFAULT_CLOUD_ID`, `llsettingssky.cpp`) —
/// what a renderer samples when [`SkySettings::cloud_texture`] is `None`.
pub const DEFAULT_CLOUD_TEXTURE: Uuid = Uuid::from_u128(0x1dc1_368f_e8fe_f02d_a08d_9d9f_11c1_af6b);

/// The built-in rainbow texture (`IMG_RAINBOW`, `llsettingssky.cpp`) — what a
/// renderer samples for the rainbow overlay when [`SkySettings::rainbow_texture`]
/// is `None`. Sampled by *row*: the vertical axis sweeps the band, the
/// horizontal axis selects the droplet-radius variant.
pub const DEFAULT_RAINBOW_TEXTURE: Uuid =
    Uuid::from_u128(0x11b4_c57c_56b3_04ed_1f82_2004_3638_82e4);

/// The built-in 22° ice-halo texture (`IMG_HALO`, `llsettingssky.cpp`) — what a
/// renderer samples for the halo overlay when [`SkySettings::halo_texture`] is
/// `None`. Sampled at column zero, so only its vertical profile matters.
pub const DEFAULT_HALO_TEXTURE: Uuid = Uuid::from_u128(0x1214_9143_f599_91a7_77ac_b52a_3c0f_59cd);

/// The built-in bloom / star texture (`IMG_BLOOM1`, `llsettingssky.cpp`) — what
/// the star field samples when [`SkySettings::bloom_texture`] is `None`. Drawn
/// additively, so its dark texels contribute nothing.
pub const DEFAULT_BLOOM_TEXTURE: Uuid = Uuid::from_u128(0x3c59_f7fe_9dc8_47f9_8aaf_a9dd_1fbc_3bef);

/// The built-in wave normal map (`DEFAULT_WATER_NORMAL`, `indra_constants.cpp`)
/// — what the water surface samples when [`WaterSettings::normal_map`] is
/// `None`.
pub const DEFAULT_WATER_NORMAL_TEXTURE: Uuid =
    Uuid::from_u128(0x822d_ed49_9a6c_f61c_cb89_6df5_4f42_cdf4);

/// Every built-in environment texture a renderer falls back to, in one list, so
/// a grid fixture can answer the whole set and a test can prove none was
/// forgotten.
///
/// The reference viewer ships **none** of these — Firestorm marks the sky ones
/// `// dataserver` and its `static_assets` folders hold only animations,
/// wearables and gestures — so on a real grid they are ordinary library assets
/// fetched over `GetTexture` like any other. A fake grid that serves no
/// substitute leaves a viewer retrying seven fetches on every arrival.
pub const BUILTIN_ENVIRONMENT_TEXTURES: [Uuid; 7] = [
    DEFAULT_SUN_TEXTURE,
    DEFAULT_MOON_TEXTURE,
    DEFAULT_CLOUD_TEXTURE,
    DEFAULT_RAINBOW_TEXTURE,
    DEFAULT_HALO_TEXTURE,
    DEFAULT_BLOOM_TEXTURE,
    DEFAULT_WATER_NORMAL_TEXTURE,
];

/// The water plane's own two textures, which the renderer picks between by
/// whether the water is transparent — Firestorm's
/// `DEFAULT_TRANSPARENT_WATER_TEXTURE` and `DEFAULT_OPAQUE_WATER_TEXTURE`
/// (`indra/llinventory/llsettingswater.cpp`).
///
/// Separate from [`DEFAULT_WATER_NORMAL_TEXTURE`], which is the ripple normal
/// map: these are the surface itself. Like the rest, the viewer ships neither
/// and fetches both from the grid on arrival.
pub const BUILTIN_WATER_PLANE_TEXTURES: [Uuid; 2] = [
    Uuid::from_u128(0x2bfd_3884_7e27_69b9_ba3a_3e67_3f68_0004),
    Uuid::from_u128(0x43c3_2285_d658_1793_c123_bf86_315d_e055),
];

/// The fifteen **standard bump maps** the reference viewer loads from
/// `app_settings/std_bump.ini` at startup: woodgrain, bark, bricks, checker,
/// concrete, crustytile, cutstone, discs, gravel, petridish, siding, stonetile,
/// stucco, suction, weave — in that order, which is the order `std_bump.ini`
/// lists them and therefore the order the bumpiness enum indexes them.
///
/// A face with a legacy `bump` value above the procedural range names one of
/// these, and the viewer fetches every one of them over `GetTexture` **whether
/// or not any face uses it** — it builds the standard-bumpmap table on startup.
/// So a grid that serves none of them leaves fifteen fetches retrying on every
/// single arrival, which is enough on its own to keep a scene from ever falling
/// quiet.
pub const BUILTIN_BUMPMAP_TEXTURES: [Uuid; 15] = [
    Uuid::from_u128(0x058c_75c0_a0d5_f2f8_43f3_e969_9a89_c2fc), // woodgrain
    Uuid::from_u128(0x6c9f_a78a_1c69_2168_325b_3e03_ffa3_48ce), // bark
    Uuid::from_u128(0xb8ee_d5f0_64b7_6e12_b67f_43fa_8e77_3440), // bricks
    Uuid::from_u128(0x9dea_b416_9c63_78d6_d558_9a15_6f12_044c), // checker
    Uuid::from_u128(0xdb9d_39ec_a896_c287_1ced_6456_6217_021e), // concrete
    Uuid::from_u128(0xf2d7_b6f6_4200_1e9a_fd5b_9645_9e95_0f94), // crustytile
    Uuid::from_u128(0xd925_8671_868f_7511_c321_7bae_f9e9_48a4), // cutstone
    Uuid::from_u128(0xd21e_44ca_ff1c_a96e_b2ef_c075_3426_b7d9), // discs
    Uuid::from_u128(0x4726_f13e_bd07_f2fb_feb0_bfa2_ac58_ab61), // gravel
    Uuid::from_u128(0xe569_711a_27c2_aad4_9246_0c91_0239_a179), // petridish
    Uuid::from_u128(0x073c_9723_540c_5449_cdd4_0e87_fdc1_59e3), // siding
    Uuid::from_u128(0xae87_4d1a_93ef_54fb_5fd3_eb0c_b156_afc0), // stonetile
    Uuid::from_u128(0x92e6_6e00_f56f_598a_7997_048a_a64c_de18), // stucco
    Uuid::from_u128(0x83b7_7fc6_10b4_63ec_4de7_f406_29f2_38c5), // suction
    Uuid::from_u128(0x7351_98cf_6ea0_2550_e222_21d3_c6a3_41ae), // weave
];

/// The viewer's own two utility textures that nonetheless live on the grid:
/// `IMG_SMOKE` (the particle default) and `IMG_FACE_SELECT` (the build-tool
/// face highlight), both from `indra/llcommon/indra_constants.cpp`.
///
/// Both are marked `// VIEWER` or used by viewer UI, but neither ships with the
/// client — they are fetched like any other library texture, and so a grid has
/// to answer them.
pub const BUILTIN_VIEWER_TEXTURES: [Uuid; 2] = [
    Uuid::from_u128(0xb4ba_225c_373f_446d_9f7e_6cb7_b5cf_9b3d), // IMG_SMOKE
    Uuid::from_u128(0xa85a_c674_cb75_4af6_9499_df7c_5aaf_7a28), // IMG_FACE_SELECT
];

impl EnvironmentSettings {
    /// The built-in **legacy WindLight default** environment: the sky and water
    /// the reference viewer falls back to when a region advertises no Extended
    /// Environment (EEP) capability. Mirrors Firestorm's `LLSettingsSky::defaults`
    /// / `LLSettingsWater::defaults` (`indra/llinventory/llsettings{sky,water}.cpp`)
    /// — one midday sky frame and one water frame on a trivial single-keyframe day
    /// cycle. Used as the viewer's starting environment until a real
    /// [`Event::Environment`](crate::Event::Environment) arrives.
    ///
    /// The two frames are named apart rather than both "Default": sky and water
    /// frames share one name namespace on the wire (see [`DayCycle`]), so a
    /// same-named pair survives only as long as nothing serializes it.
    #[must_use]
    pub fn legacy_windlight_default() -> Self {
        let sky = SkySettings::legacy_windlight_default(DEFAULT_SKY_FRAME);
        let water = WaterSettings::legacy_default(DEFAULT_WATER_FRAME);
        let mut sky_frames = BTreeMap::new();
        drop(sky_frames.insert(sky.name.clone(), sky));
        let mut water_frames = BTreeMap::new();
        drop(water_frames.insert(water.name.clone(), water));
        let frame = |name: &str| DayCycleFrame {
            keyframe: 0.0,
            name: name.to_owned(),
        };
        Self {
            parcel_id: -1,
            region_id: Uuid::nil(),
            // The reference default day length is four hours.
            day_length: 4 * 60 * 60,
            day_offset: 0,
            flags: 0,
            env_version: -1,
            track_altitudes: [1000.0, 2000.0, 3000.0],
            day_cycle: DayCycle {
                name: "Default".to_owned(),
                water_track: vec![frame(DEFAULT_WATER_FRAME)],
                sky_tracks: vec![vec![frame(DEFAULT_SKY_FRAME)]],
                sky_frames,
                water_frames,
            },
        }
    }

    /// The 0-based index into [`DayCycle::sky_tracks`] whose altitude band
    /// contains `altitude` (metres above the region), mirroring the reference
    /// `LLEnvironment::calculateSkyTrackForAltitude`
    /// (`indra/newview/llenvironment.cpp`).
    ///
    /// The reference clamps a camera altitude against the four breakpoints
    /// `[0, a1, a2, a3]` and returns a *track number* `1..=4`, where sky track 1
    /// is the surface track. Here the surface track is [`DayCycle::sky_tracks`]
    /// index 0, so the mapping is: `altitude <= a1` → 0, `<= a2` → 1, `<= a3` → 2,
    /// otherwise 3. The result is clamped to the number of tracks the day cycle
    /// actually carries, so a cycle with a single ground track always selects it.
    #[must_use]
    pub fn sky_track_for_altitude(&self, altitude: f32) -> usize {
        let [a1, a2, a3] = self.track_altitudes;
        let raw = if altitude <= a1 {
            0
        } else if altitude <= a2 {
            1
        } else if altitude <= a3 {
            2
        } else {
            3
        };
        let last = self.day_cycle.sky_tracks.len().saturating_sub(1);
        raw.min(last)
    }

    /// The active [`SkySettings`] for a camera at `altitude` and a day-cycle
    /// `position` (the normalised time of day, `0.0..=1.0`): the keyframe in force
    /// at `position` on the altitude-selected
    /// [`sky_track`](Self::sky_track_for_altitude), resolved through
    /// [`DayCycle::sky_frames`].
    ///
    /// This selects the *active* keyframe (the reference
    /// `LLEnvironment::convert_time_to_position` → `get_wrapping_atbefore`)
    /// without blending toward the next one. The smooth day-cycle interpolation
    /// is [`blended_sky_settings`](Self::blended_sky_settings); this unblended
    /// selection is kept for the callers (and tests) that want a borrowed frame.
    /// Falls back to any defined sky frame if the selected track is empty or
    /// names a missing frame, and to `None` only if the cycle defines no sky
    /// frame at all.
    #[must_use]
    pub fn active_sky_settings(&self, altitude: f32, position: f32) -> Option<&SkySettings> {
        let cycle = &self.day_cycle;
        let track_frame = cycle
            .sky_tracks
            .get(self.sky_track_for_altitude(altitude))
            .and_then(|track| active_keyframe(track, position))
            .and_then(|frame| cycle.sky_frames.get(&frame.name));
        track_frame.or_else(|| cycle.sky_frames.values().next())
    }

    /// The **blended** [`SkySettings`] for a camera at `altitude` and a day-cycle
    /// `position` (the normalised time of day, `0.0..=1.0`): the smooth
    /// interpolation between the two keyframes bounding `position` on the
    /// altitude-selected [`sky_track`](Self::sky_track_for_altitude), the
    /// reference `LLEnvironment` day-cycle blender
    /// (`LLSettingsBlender` → `LLSettingsBase::blend`).
    ///
    /// Where [`active_sky_settings`](Self::active_sky_settings) snaps to the
    /// keyframe in force, this finds the bounding pair `(lower, upper)` around
    /// `position` (wrapping across the day boundary) and blends the lower toward
    /// the upper by the fraction of the way `position` has travelled between
    /// their keyframe times (see [`SkySettings::blend`]). A single-keyframe track
    /// (or the built-in default cycle) yields that one frame unchanged.
    ///
    /// Returns an *owned* frame (the blend synthesises new values), unlike the
    /// borrowing `active_sky_settings`. Falls back to any defined sky frame if
    /// the selected track is empty or names missing frames, and to `None` only if
    /// the cycle defines no sky frame at all.
    #[must_use]
    pub fn blended_sky_settings(&self, altitude: f32, position: f32) -> Option<SkySettings> {
        self.day_cycle
            .blended_sky(self.sky_track_for_altitude(altitude), position)
    }

    /// The active [`WaterSettings`] for a day-cycle `position` (the normalised
    /// time of day, `0.0..=1.0`): the keyframe in force at `position` on the
    /// single region-wide [`water_track`](DayCycle::water_track), resolved through
    /// [`DayCycle::water_frames`].
    ///
    /// Water has no altitude tracks (unlike the sky) — one region-wide track — so
    /// this takes only a day-cycle `position`. Like
    /// [`active_sky_settings`](Self::active_sky_settings) it selects the *active*
    /// keyframe without blending toward the next one; the smooth interpolation is
    /// [`blended_water_settings`](Self::blended_water_settings). Falls back to any
    /// defined water frame if the track is empty or names a missing frame, and to
    /// `None` only if the cycle defines no water frame at all.
    #[must_use]
    pub fn active_water_settings(&self, position: f32) -> Option<&WaterSettings> {
        let cycle = &self.day_cycle;
        let track_frame = active_keyframe(&cycle.water_track, position)
            .and_then(|frame| cycle.water_frames.get(&frame.name));
        track_frame.or_else(|| cycle.water_frames.values().next())
    }

    /// The **blended** [`WaterSettings`] for a day-cycle `position` (the
    /// normalised time of day, `0.0..=1.0`): the smooth interpolation between the
    /// two keyframes bounding `position` on the region-wide
    /// [`water_track`](DayCycle::water_track), the water counterpart of
    /// [`blended_sky_settings`](Self::blended_sky_settings) (the reference
    /// `LLSettingsWater::blend`).
    ///
    /// Returns an *owned* frame (the blend synthesises new values). Falls back to
    /// any defined water frame if the track is empty or names missing frames, and
    /// to `None` only if the cycle defines no water frame at all.
    #[must_use]
    pub fn blended_water_settings(&self, position: f32) -> Option<WaterSettings> {
        self.day_cycle.blended_water(position)
    }
}

/// The day-cycle keyframe in force at normalised time `position` (`0.0..=1.0`) on
/// `track`: the frame with the greatest keyframe time `<= position`, wrapping to
/// the last keyframe of the cycle when `position` precedes the first (the
/// reference `get_wrapping_atbefore`). `None` only for an empty track.
fn active_keyframe(track: &[DayCycleFrame], position: f32) -> Option<&DayCycleFrame> {
    let at_before = track
        .iter()
        .filter(|frame| frame.keyframe <= position)
        .max_by(|a, b| a.keyframe.total_cmp(&b.keyframe));
    // Before the first keyframe the cycle wraps: the latest keyframe (end of the
    // previous day) is still in force.
    at_before.or_else(|| {
        track
            .iter()
            .max_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
    })
}

/// The two keyframes bounding normalised time `position` (`0.0..=1.0`) on
/// `track`, plus the blend factor `0.0..=1.0` measuring how far `position` has
/// travelled from the lower keyframe toward the upper one (the reference
/// `LLSettingsDay` bounding-keyframe lookup that feeds `LLSettingsBlender`).
///
/// The *lower* keyframe is the one in force at `position` ([`active_keyframe`]);
/// the *upper* is the next keyframe after it. The day cycle wraps, so when
/// `position` sits after the last keyframe the upper wraps to the first
/// (its keyframe time treated as `+ 1.0`), and when `position` precedes the
/// first keyframe the lower wraps to the last (treated as `- 1.0`). A
/// single-keyframe track returns that frame as both bounds with factor `0.0`.
/// `None` only for an empty track.
fn bounding_keyframes(
    track: &[DayCycleFrame],
    position: f32,
) -> Option<(&DayCycleFrame, &DayCycleFrame, f32)> {
    let lower = active_keyframe(track, position)?;
    // A single keyframe is in force all day: it is both bounds, blended with
    // itself (factor is immaterial, so report the natural `0.0`).
    if let [only] = track {
        return Some((only, only, 0.0));
    }
    // The upper bound is the earliest keyframe strictly after `position`; if none
    // exists the cycle wraps to the earliest keyframe of the day.
    let upper = track
        .iter()
        .filter(|frame| frame.keyframe > position)
        .min_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
        .or_else(|| {
            track
                .iter()
                .min_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
        })?;
    // Unwrap the two keyframe times onto a monotonic line around `position` so the
    // span is positive even across the day boundary.
    let lower_time = if lower.keyframe <= position {
        lower.keyframe
    } else {
        lower.keyframe - 1.0
    };
    let upper_time = if upper.keyframe > position {
        upper.keyframe
    } else {
        upper.keyframe + 1.0
    };
    let span = upper_time - lower_time;
    let factor = if span > f32::EPSILON {
        ((position - lower_time) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some((lower, upper, factor))
}

/// Linear interpolation between `a` and `b` by `factor` (`0.0` → `a`, `1.0` →
/// `b`), the scalar primitive every [`SkySettings::blend`] channel builds on.
fn lerp_f32(a: f32, b: f32, factor: f32) -> f32 {
    a + (b - a) * factor
}

/// Per-channel lerp of two [`Color`]s.
fn lerp_color(a: Color, b: Color, factor: f32) -> Color {
    Color::new(
        lerp_f32(a.red(), b.red(), factor),
        lerp_f32(a.green(), b.green(), factor),
        lerp_f32(a.blue(), b.blue(), factor),
    )
}

/// Per-channel lerp of two [`ColorAlpha`]s.
fn lerp_color_alpha(a: ColorAlpha, b: ColorAlpha, factor: f32) -> ColorAlpha {
    ColorAlpha::new(
        lerp_f32(a.red(), b.red(), factor),
        lerp_f32(a.green(), b.green(), factor),
        lerp_f32(a.blue(), b.blue(), factor),
        lerp_f32(a.alpha(), b.alpha(), factor),
    )
}

/// Per-component lerp of two [`Glow`]s (the reserved middle component is
/// interpolated too, so a round trip stays well-defined).
fn lerp_glow(a: Glow, b: Glow, factor: f32) -> Glow {
    Glow::new(
        lerp_f32(a.size(), b.size(), factor),
        lerp_f32(a.reserved(), b.reserved(), factor),
        lerp_f32(a.focus(), b.focus(), factor),
    )
}

/// Per-component lerp of two [`CloudPosDensity`]s.
fn lerp_cloud_pos_density(a: CloudPosDensity, b: CloudPosDensity, factor: f32) -> CloudPosDensity {
    CloudPosDensity::new(
        lerp_f32(a.position_x(), b.position_x(), factor),
        lerp_f32(a.position_y(), b.position_y(), factor),
        lerp_f32(a.density(), b.density(), factor),
    )
}

/// Per-component lerp of two 2-vectors (e.g. `cloud_scroll_rate`).
fn lerp_array2(a: [f32; 2], b: [f32; 2], factor: f32) -> [f32; 2] {
    let [ax, ay] = a;
    let [bx, by] = b;
    [lerp_f32(ax, bx, factor), lerp_f32(ay, by, factor)]
}

/// Layer-wise lerp of two density profiles.
///
/// Two profiles of the **same shape** interpolate term by term, which is what
/// the reference's own map interpolation does when it walks two settings LLSDs
/// of matching structure. Two of *different* shapes have no term-wise
/// correspondence at all — a two-layer ozone ramp against a one-layer one — so
/// they switch at the halfway point rather than producing a stack that is
/// neither. Two absent profiles stay absent.
fn lerp_density_profile(a: &[DensityLayer], b: &[DensityLayer], factor: f32) -> Vec<DensityLayer> {
    if a.len() != b.len() {
        return if factor > 0.5 { b.to_vec() } else { a.to_vec() };
    }
    a.iter()
        .zip(b)
        .map(|(lower, upper)| DensityLayer {
            width: lerp_f32(lower.width, upper.width, factor),
            exp_term: lerp_f32(lower.exp_term, upper.exp_term, factor),
            exp_scale: lerp_f32(lower.exp_scale, upper.exp_scale, factor),
            linear_term: lerp_f32(lower.linear_term, upper.linear_term, factor),
            constant_term: lerp_f32(lower.constant_term, upper.constant_term, factor),
            // An anisotropy only one side carries is not a number to blend
            // toward from nothing: take whichever side is in force.
            anisotropy: match (lower.anisotropy, upper.anisotropy) {
                (Some(lower), Some(upper)) => Some(lerp_f32(lower, upper, factor)),
                (lower, upper) => {
                    if factor > 0.5 {
                        upper
                    } else {
                        lower
                    }
                }
            },
        })
        .collect()
}

/// Per-axis lerp of two [`Scale`]s (e.g. the water `normal_scale`).
fn lerp_scale(a: Scale, b: Scale, factor: f32) -> Scale {
    Scale::new(
        lerp_f32(a.x(), b.x(), factor),
        lerp_f32(a.y(), b.y(), factor),
        lerp_f32(a.z(), b.z(), factor),
    )
}

/// Normalise a quaternion, falling back to identity for a degenerate (zero)
/// input so a blend never produces a non-rotation.
fn normalize_rotation(r: Rotation) -> Rotation {
    let length = (r.x * r.x + r.y * r.y + r.z * r.z + r.s * r.s).sqrt();
    if length > f32::EPSILON {
        let inv = 1.0 / length;
        Rotation {
            x: r.x * inv,
            y: r.y * inv,
            z: r.z * inv,
            s: r.s * inv,
        }
    } else {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }
    }
}

/// Spherical linear interpolation between two rotations (the reference slerps the
/// sun/moon `sun_rotation` / `moon_rotation` keys rather than lerping their
/// components — `LLSettingsBase::getSlerps`). Takes the shortest arc (negating
/// the far quaternion when the dot product is negative) and degrades to a
/// normalised lerp for nearly-parallel inputs to stay numerically stable.
fn slerp_rotation(a: &Rotation, b: &Rotation, factor: f32) -> Rotation {
    // Nearly parallel: the arc is tiny, so a normalised lerp is both stable and
    // visually identical to a slerp.
    const SLERP_LERP_THRESHOLD: f32 = 0.9995;
    let raw_dot = a.x * b.x + a.y * b.y + a.z * b.z + a.s * b.s;
    // Take the shortest arc: a quaternion and its negation are the same rotation.
    let (bx, by, bz, bs, dot) = if raw_dot < 0.0 {
        (-b.x, -b.y, -b.z, -b.s, -raw_dot)
    } else {
        (b.x, b.y, b.z, b.s, raw_dot)
    };
    if dot > SLERP_LERP_THRESHOLD {
        return normalize_rotation(Rotation {
            x: lerp_f32(a.x, bx, factor),
            y: lerp_f32(a.y, by, factor),
            z: lerp_f32(a.z, bz, factor),
            s: lerp_f32(a.s, bs, factor),
        });
    }
    let theta_0 = dot.clamp(-1.0, 1.0).acos();
    let sin_theta_0 = theta_0.sin();
    let theta = theta_0 * factor;
    let scale_from = (theta_0 - theta).sin() / sin_theta_0;
    let scale_to = theta.sin() / sin_theta_0;
    Rotation {
        x: a.x * scale_from + bx * scale_to,
        y: a.y * scale_from + by * scale_to,
        z: a.z * scale_from + bz * scale_to,
        s: a.s * scale_from + bs * scale_to,
    }
}

/// Selects `lower` below the halfway point and `upper` at or beyond it — the
/// reference `LLSettingsBase::interpolateSDValue` fallback for the non-numeric
/// settings (textures, names): a discrete `mix > 0.5 ? other : this`.
fn pick_at_half<T: Clone>(lower: &T, upper: &T, factor: f32) -> T {
    if factor > 0.5 {
        upper.clone()
    } else {
        lower.clone()
    }
}

/// Reproduces the reference `convert_azimuth_and_altitude_to_quat`
/// (`indra/llinventory/llsettingssky.cpp`): the rotation taking the local `+X`
/// axis to the sky direction at the given spherical angles, in radians.
///
/// Public because it is the only way to *place* a sky's sun or moon, and
/// [`SkySettings::sun_rotation`] is otherwise a quaternion with no way to build a
/// meaningful one. The reference's own named presets (Sunrise / Midday / Sunset /
/// Midnight) are grid assets rather than code, so anything offline that wants a
/// sky at a given time of day — an environment editor, a render scene — has to
/// put the sun somewhere itself, and this is where the reference's convention for
/// that lives.
#[must_use]
pub fn azimuth_altitude_to_rotation(azimuth: f32, altitude: f32) -> Rotation {
    // The unit direction the angles point at (SL's `+x` right, `+y` at, `+z` up).
    let dir_x = azimuth.cos() * altitude.cos();
    let dir_y = azimuth.sin() * altitude.cos();
    let dir_z = altitude.sin();
    // `axis = x_axis × dir`; `dir` is a unit vector, so `x_axis · dir` is `dir_x`.
    let axis_x = 0.0_f32;
    let axis_y = -dir_z;
    let axis_z = dir_y;
    let axis_len = axis_y.hypot(axis_z);
    let angle = dir_x.clamp(-1.0, 1.0).acos();
    // `dir` parallel to `+x`: no rotation.
    if axis_len <= f32::EPSILON {
        return Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
    }
    let half = angle * 0.5;
    let sin_half = half.sin();
    let scale = sin_half / axis_len;
    Rotation {
        x: axis_x * scale,
        y: axis_y * scale,
        z: axis_z * scale,
        s: half.cos(),
    }
}

/// The inverse of [`azimuth_altitude_to_rotation`]: where a sky's sun or moon
/// *is*, as spherical angles in radians — the reference's
/// `LLVirtualTrackball::getAzimuthAndElevation`.
///
/// Public for the same reason the forward conversion is: an environment editor
/// has to show the sun's azimuth and elevation on two sliders, and
/// [`SkySettings::sun_rotation`] is a quaternion with no other way to ask.
/// Azimuth comes back normalised to `0.0..TAU` so a slider over `0°..360°` has a
/// value for every rotation; altitude is `-FRAC_PI_2..=FRAC_PI_2`.
#[must_use]
pub fn rotation_to_azimuth_altitude(rotation: &Rotation) -> (f32, f32) {
    // The rotation applied to the local `+X` axis — the first column of the
    // rotation matrix, which is the direction the forward conversion encoded.
    let (x, y, z, s) = (rotation.x, rotation.y, rotation.z, rotation.s);
    let dir_x = 1.0 - 2.0 * z.mul_add(z, y * y);
    let dir_y = 2.0 * s.mul_add(z, x * y);
    let dir_z = 2.0 * s.mul_add(-y, x * z);
    let altitude = dir_z.clamp(-1.0, 1.0).asin();
    let azimuth = dir_y.atan2(dir_x);
    (
        if azimuth < 0.0 {
            azimuth + std::f32::consts::TAU
        } else {
            azimuth
        },
        altitude,
    )
}

impl SkySettings {
    /// Blend this sky frame toward `other` by `factor` (`0.0` → `self`, `1.0` →
    /// `other`), the reference day-cycle frame interpolation
    /// (`LLSettingsBase::blend` over the sky settings map).
    ///
    /// Numeric channels (the haze scalars, colours, cloud/glow parameters,
    /// radii, …) are linearly interpolated; the sun and moon rotations are
    /// **slerped** (the reference marks them as slerp keys); and the discrete,
    /// non-blendable settings — the frame name and the sun / moon / cloud / bloom
    /// / halo / rainbow textures — snap to whichever frame is nearer
    /// (`factor > 0.5` picks `other`), matching the reference's
    /// `mix > 0.5 ? other : this` for its non-numeric settings.
    #[must_use]
    pub fn blend(&self, other: &Self, factor: f32) -> Self {
        Self {
            name: pick_at_half(&self.name, &other.name, factor),
            sun_rotation: slerp_rotation(&self.sun_rotation, &other.sun_rotation, factor),
            moon_rotation: slerp_rotation(&self.moon_rotation, &other.moon_rotation, factor),
            sunlight_color: lerp_color_alpha(self.sunlight_color, other.sunlight_color, factor),
            ambient: lerp_color(self.ambient, other.ambient, factor),
            blue_horizon: lerp_color(self.blue_horizon, other.blue_horizon, factor),
            blue_density: lerp_color(self.blue_density, other.blue_density, factor),
            haze_horizon: lerp_f32(self.haze_horizon, other.haze_horizon, factor),
            haze_density: lerp_f32(self.haze_density, other.haze_density, factor),
            density_multiplier: lerp_f32(self.density_multiplier, other.density_multiplier, factor),
            distance_multiplier: lerp_f32(
                self.distance_multiplier,
                other.distance_multiplier,
                factor,
            ),
            max_y: lerp_f32(self.max_y, other.max_y, factor),
            gamma: lerp_f32(self.gamma, other.gamma, factor),
            reflection_probe_ambiance: lerp_f32(
                self.reflection_probe_ambiance,
                other.reflection_probe_ambiance,
                factor,
            ),
            cloud_color: lerp_color(self.cloud_color, other.cloud_color, factor),
            cloud_pos_density1: lerp_cloud_pos_density(
                self.cloud_pos_density1,
                other.cloud_pos_density1,
                factor,
            ),
            cloud_pos_density2: lerp_cloud_pos_density(
                self.cloud_pos_density2,
                other.cloud_pos_density2,
                factor,
            ),
            cloud_scale: lerp_f32(self.cloud_scale, other.cloud_scale, factor),
            cloud_scroll_rate: lerp_array2(self.cloud_scroll_rate, other.cloud_scroll_rate, factor),
            cloud_shadow: lerp_f32(self.cloud_shadow, other.cloud_shadow, factor),
            cloud_variance: lerp_f32(self.cloud_variance, other.cloud_variance, factor),
            glow: lerp_glow(self.glow, other.glow, factor),
            star_brightness: lerp_f32(self.star_brightness, other.star_brightness, factor),
            sun_scale: lerp_f32(self.sun_scale, other.sun_scale, factor),
            moon_scale: lerp_f32(self.moon_scale, other.moon_scale, factor),
            moon_brightness: lerp_f32(self.moon_brightness, other.moon_brightness, factor),
            sun_arc_radians: lerp_f32(self.sun_arc_radians, other.sun_arc_radians, factor),
            droplet_radius: lerp_f32(self.droplet_radius, other.droplet_radius, factor),
            ice_level: lerp_f32(self.ice_level, other.ice_level, factor),
            moisture_level: lerp_f32(self.moisture_level, other.moisture_level, factor),
            sky_top_radius: lerp_f32(self.sky_top_radius, other.sky_top_radius, factor),
            sky_bottom_radius: lerp_f32(self.sky_bottom_radius, other.sky_bottom_radius, factor),
            planet_radius: lerp_f32(self.planet_radius, other.planet_radius, factor),
            sun_texture: pick_at_half(&self.sun_texture, &other.sun_texture, factor),
            moon_texture: pick_at_half(&self.moon_texture, &other.moon_texture, factor),
            cloud_texture: pick_at_half(&self.cloud_texture, &other.cloud_texture, factor),
            bloom_texture: pick_at_half(&self.bloom_texture, &other.bloom_texture, factor),
            halo_texture: pick_at_half(&self.halo_texture, &other.halo_texture, factor),
            rainbow_texture: pick_at_half(&self.rainbow_texture, &other.rainbow_texture, factor),
            dome_offset: pick_at_half(&self.dome_offset, &other.dome_offset, factor),
            dome_radius: pick_at_half(&self.dome_radius, &other.dome_radius, factor),
            // The reference blends a density profile the way it blends any
            // other setting — but only where the two frames' layer *lists* line
            // up, which is what `lerp_density_profile` insists on before it
            // interpolates. Two profiles of different shapes snap like a
            // texture id rather than producing a layer list neither frame has.
            rayleigh_config: lerp_density_profile(
                &self.rayleigh_config,
                &other.rayleigh_config,
                factor,
            ),
            mie_config: lerp_density_profile(&self.mie_config, &other.mie_config, factor),
            absorption_config: lerp_density_profile(
                &self.absorption_config,
                &other.absorption_config,
                factor,
            ),
        }
    }

    /// The sky's "fake HDR" scale (`SKY_HDR_SCALE`), applied to the WL sky,
    /// clouds, and sun / moon discs after linearisation and before tone-mapping,
    /// the reference `LLSettingsVOSky::applySpecial`
    /// (`indra/newview/llsettingsvo.cpp`).
    ///
    /// With the shipped defaults (`RenderSkyAutoAdjustLegacy = false`) an EEP sky
    /// whose `reflection_probe_ambiance` is non-zero is scaled by
    /// `sqrt(gamma) * 2` — the "modifier so `1.0` maps to the most desirable
    /// default and the maximum does not go off the rails" from the reference —
    /// which pushes bright pixels (notably the sun disc) above `1.0` so they blow
    /// out under tone-mapping. A legacy / classic-mode sky (`ambiance == 0`) keeps
    /// `1.0`.
    ///
    /// The reference's third branch — auto-adjusting a *legacy* sky to
    /// `RenderSkyAutoAdjustHDRScale` — only fires when the
    /// `RenderSkyAutoAdjustLegacy` debug setting is enabled (it ships disabled, so
    /// classic mode is the default), which the viewer does not model; that path
    /// therefore also returns `1.0` here.
    #[must_use]
    pub fn sky_hdr_scale(&self) -> f32 {
        if self.reflection_probe_ambiance == 0.0 {
            1.0
        } else {
            self.gamma.max(0.0).sqrt() * 2.0
        }
    }

    /// The reference viewer's built-in default sky (`LLSettingsSky::defaults`,
    /// `indra/llinventory/llsettingssky.cpp`), including the legacy-haze fallbacks
    /// (`LLColor3`/`F32` defaults from `LLSettingsSky::loadValuesFromLLSD`) and
    /// the three atmospheric-scattering profiles, which the reference requires
    /// of every sky frame. Every documented scalar/colour is set to its
    /// reference default.
    #[must_use]
    pub fn legacy_windlight_default(name: &str) -> Self {
        // Sun and moon tracks at the default day's start (track position 0): the
        // reference offsets the two so they do not sit at opposite poles.
        let eighty_deg = 80.0_f32.to_radians();
        let eighth_pi = std::f32::consts::FRAC_PI_8;
        let sun_rotation = azimuth_altitude_to_rotation(0.0, eighty_deg);
        let moon_rotation = azimuth_altitude_to_rotation(eighth_pi, eighty_deg + eighth_pi);
        Self {
            name: name.to_owned(),
            sun_rotation,
            moon_rotation,
            sunlight_color: ColorAlpha::new(0.7342, 0.7815, 0.8999, 0.0),
            // Legacy-haze defaults.
            ambient: Color::new(0.25, 0.25, 0.25),
            blue_horizon: Color::new(0.4954, 0.4954, 0.6399),
            blue_density: Color::new(0.2447, 0.4487, 0.7599),
            haze_horizon: 0.19,
            haze_density: 0.7,
            density_multiplier: 0.0001,
            distance_multiplier: 0.8,
            max_y: 1605.0,
            gamma: 1.0,
            // A legacy / classic-mode sky has no `reflection_probe_ambiance`
            // setting, so it decodes as (and defaults to) `0.0` — the shipped
            // `sky_hdr_scale = 1.0` path.
            reflection_probe_ambiance: 0.0,
            cloud_color: Color::new(0.4099, 0.4099, 0.4099),
            cloud_pos_density1: CloudPosDensity::new(1.0, 0.526, 1.0),
            cloud_pos_density2: CloudPosDensity::new(1.0, 0.526, 1.0),
            cloud_scale: 0.4199,
            cloud_scroll_rate: [0.2, 0.01],
            cloud_shadow: 0.2699,
            cloud_variance: 0.0,
            glow: Glow::new(5.0, 0.001, -0.4799),
            star_brightness: 250.0,
            sun_scale: 1.0,
            moon_scale: 1.0,
            moon_brightness: 0.5,
            sun_arc_radians: 0.00045,
            droplet_radius: 800.0,
            ice_level: 0.0,
            moisture_level: 0.0,
            sky_top_radius: 6420.0,
            sky_bottom_radius: 6360.0,
            planet_radius: 6360.0,
            // `None` selects the viewer's built-in sun/moon/cloud/etc. textures.
            sun_texture: None,
            moon_texture: None,
            cloud_texture: None,
            bloom_texture: None,
            halo_texture: None,
            rainbow_texture: None,
            // Absent, not the reference's 0.96 / 15000: this default stands in
            // for a sky *document*, and the two are carried rather than read,
            // so inventing them would put keys in a frame that never had any.
            dome_offset: None,
            dome_radius: None,
            // The reference's own defaults, unlike the dome pair above: all
            // three profiles are *required with no default* by its sky
            // validator, so a frame that leaves them empty is one it throws
            // away — see the module documentation.
            rayleigh_config: DensityLayer::rayleigh_default(),
            mie_config: DensityLayer::mie_default(),
            absorption_config: DensityLayer::absorption_default(),
        }
    }
}

impl WaterSettings {
    /// Blend this water frame toward `other` by `factor` (`0.0` → `self`, `1.0` →
    /// `other`), the reference day-cycle frame interpolation
    /// (`LLSettingsWater::blend`).
    ///
    /// Numeric channels (the fresnel scalars, blur / fog / refraction scalars, the
    /// fog colour, the normal (wavelet) scale, and the two wave directions) are
    /// linearly interpolated; the discrete, non-blendable settings — the frame
    /// name and the normal / transparent textures — snap to whichever frame is
    /// nearer (`factor > 0.5` picks `other`), matching the reference's
    /// `mix > 0.5 ? other : this` for its non-numeric settings.
    #[must_use]
    pub fn blend(&self, other: &Self, factor: f32) -> Self {
        Self {
            name: pick_at_half(&self.name, &other.name, factor),
            blur_multiplier: lerp_f32(self.blur_multiplier, other.blur_multiplier, factor),
            fresnel_offset: lerp_f32(self.fresnel_offset, other.fresnel_offset, factor),
            fresnel_scale: lerp_f32(self.fresnel_scale, other.fresnel_scale, factor),
            normal_scale: lerp_scale(self.normal_scale, other.normal_scale, factor),
            normal_map: pick_at_half(&self.normal_map, &other.normal_map, factor),
            scale_above: lerp_f32(self.scale_above, other.scale_above, factor),
            scale_below: lerp_f32(self.scale_below, other.scale_below, factor),
            transparent_texture: pick_at_half(
                &self.transparent_texture,
                &other.transparent_texture,
                factor,
            ),
            underwater_fog_mod: lerp_f32(self.underwater_fog_mod, other.underwater_fog_mod, factor),
            water_fog_color: lerp_color(self.water_fog_color, other.water_fog_color, factor),
            water_fog_density: lerp_f32(self.water_fog_density, other.water_fog_density, factor),
            wave1_direction: lerp_array2(self.wave1_direction, other.wave1_direction, factor),
            wave2_direction: lerp_array2(self.wave2_direction, other.wave2_direction, factor),
        }
    }

    /// The reference viewer's built-in default water (`LLSettingsWater::defaults`,
    /// `indra/llinventory/llsettingswater.cpp`).
    #[must_use]
    pub fn legacy_default(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            blur_multiplier: 0.04,
            fresnel_offset: 0.5,
            fresnel_scale: 0.3999,
            normal_scale: Scale::new(2.0, 2.0, 2.0),
            normal_map: None,
            scale_above: 0.0299,
            scale_below: 0.2,
            transparent_texture: None,
            underwater_fog_mod: 0.25,
            water_fog_color: Color::new(0.0156, 0.149, 0.2509),
            water_fog_density: 2.0,
            wave1_direction: [1.04999, -0.42],
            wave2_direction: [1.10999, -1.16],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CloudPosDensity, Color, ColorAlpha, EnvironmentSettings, Glow, Scale, SkySettings,
        azimuth_altitude_to_rotation, rotation_to_azimuth_altitude,
    };
    use pretty_assertions::assert_eq;

    /// Placing the sun at an angle and asking where it is gives the angle back.
    ///
    /// The two are used together by any environment editor: the sliders read
    /// through the inverse and write through the forward conversion, so a sky
    /// merely *opened* in one must not drift.
    #[test]
    fn sun_angles_round_trip_through_the_rotation() {
        // Azimuths on both sides of the `atan2` branch cut, and altitudes at and
        // near the poles the forward conversion special-cases.
        for azimuth_deg in [0.0_f32, 45.0, 179.0, 181.0, 270.0, 359.0] {
            for altitude_deg in [-89.0_f32, -45.0, -0.5, 0.0, 0.5, 45.0, 89.0] {
                let (azimuth, altitude) = (azimuth_deg.to_radians(), altitude_deg.to_radians());
                let (back_azimuth, back_altitude) =
                    rotation_to_azimuth_altitude(&azimuth_altitude_to_rotation(azimuth, altitude));
                assert!(
                    (back_azimuth.to_degrees() - azimuth_deg).abs() < 0.01,
                    "azimuth {azimuth_deg} came back as {}",
                    back_azimuth.to_degrees()
                );
                assert!(
                    (back_altitude.to_degrees() - altitude_deg).abs() < 0.01,
                    "altitude {altitude_deg} came back as {}",
                    back_altitude.to_degrees()
                );
            }
        }
    }

    /// Azimuth comes back in `0..360°`, never negative — a slider over that
    /// range has to have a value for a sun in the western half of the sky.
    #[test]
    fn a_western_azimuth_is_not_reported_negative() {
        let rotation = azimuth_altitude_to_rotation(300.0_f32.to_radians(), 0.0);
        let (azimuth, _altitude) = rotation_to_azimuth_altitude(&rotation);
        assert!(
            (0.0..std::f32::consts::TAU).contains(&azimuth),
            "azimuth {azimuth} is outside 0..TAU"
        );
    }

    #[test]
    fn color_channels_round_trip() {
        let color = Color::new(0.25, 0.5, 0.75);
        // Compare bit patterns: `float_cmp` forbids an exact `==` on the floats.
        assert_eq!(color.red().to_bits(), 0.25_f32.to_bits());
        assert_eq!(color.green().to_bits(), 0.5_f32.to_bits());
        assert_eq!(color.blue().to_bits(), 0.75_f32.to_bits());
    }

    #[test]
    fn color_alpha_channels_round_trip() {
        let color = ColorAlpha::new(0.25, 0.5, 0.75, 0.875);
        assert_eq!(color.red().to_bits(), 0.25_f32.to_bits());
        assert_eq!(color.green().to_bits(), 0.5_f32.to_bits());
        assert_eq!(color.blue().to_bits(), 0.75_f32.to_bits());
        assert_eq!(color.alpha().to_bits(), 0.875_f32.to_bits());
    }

    #[test]
    fn scale_axes_round_trip() {
        let scale = Scale::new(2.0, 3.0, 4.0);
        assert_eq!(scale.x().to_bits(), 2.0_f32.to_bits());
        assert_eq!(scale.y().to_bits(), 3.0_f32.to_bits());
        assert_eq!(scale.z().to_bits(), 4.0_f32.to_bits());
    }

    #[test]
    fn glow_preserves_the_reserved_middle_component() {
        // The middle component is unused but must round-trip verbatim.
        let glow = Glow::new(5.0, -1.5, -2.5);
        assert_eq!(glow.size().to_bits(), 5.0_f32.to_bits());
        assert_eq!(glow.reserved().to_bits(), (-1.5_f32).to_bits());
        assert_eq!(glow.focus().to_bits(), (-2.5_f32).to_bits());
    }

    #[test]
    fn cloud_pos_density_names_its_components() {
        let value = CloudPosDensity::new(1.0, 0.5, 0.25);
        assert_eq!(value.position_x().to_bits(), 1.0_f32.to_bits());
        assert_eq!(value.position_y().to_bits(), 0.5_f32.to_bits());
        assert_eq!(value.density().to_bits(), 0.25_f32.to_bits());
    }

    #[test]
    fn sky_hdr_scale_is_one_for_a_legacy_sky() {
        // A legacy / classic-mode sky has no `reflection_probe_ambiance`, so the
        // shipped `SKY_HDR_SCALE = 1.0` path (the 2026-08-03 sky-colour fix's
        // assumption).
        let sky = SkySettings::legacy_windlight_default("Default");
        assert_eq!(sky.reflection_probe_ambiance.to_bits(), 0.0_f32.to_bits());
        assert_eq!(sky.sky_hdr_scale().to_bits(), 1.0_f32.to_bits());
    }

    #[test]
    fn sky_hdr_scale_follows_gamma_for_an_eep_probe_ambiance_sky() {
        // An EEP sky with a non-zero `reflection_probe_ambiance` is scaled by
        // `sqrt(gamma) * 2` (the reference `LLSettingsVOSky::applySpecial`), which
        // pushes bright pixels — the sun disc — above 1.0 so they blow out.
        let mut sky = SkySettings::legacy_windlight_default("Default");
        sky.reflection_probe_ambiance = 0.5;
        sky.gamma = 1.0;
        // sqrt(1.0) * 2 == 2.0
        assert_eq!(sky.sky_hdr_scale().to_bits(), 2.0_f32.to_bits());
        sky.gamma = 4.0;
        // sqrt(4.0) * 2 == 4.0
        assert_eq!(sky.sky_hdr_scale().to_bits(), 4.0_f32.to_bits());
    }

    #[test]
    fn legacy_windlight_default_has_one_referenced_sky_and_water_frame() {
        let env = EnvironmentSettings::legacy_windlight_default();
        let cycle = &env.day_cycle;
        // The single sky/water track keyframe must resolve to a defined frame.
        let sky_ok = cycle
            .sky_tracks
            .first()
            .and_then(|track| track.first())
            .is_some_and(|frame| cycle.sky_frames.contains_key(&frame.name));
        let water_ok = cycle
            .water_track
            .first()
            .is_some_and(|frame| cycle.water_frames.contains_key(&frame.name));
        assert!(sky_ok);
        assert!(water_ok);
    }

    #[test]
    fn sky_track_selection_maps_altitude_bands_to_the_reference_track_numbers() {
        let mut env = EnvironmentSettings::legacy_windlight_default();
        env.track_altitudes = [1000.0, 2000.0, 3000.0];
        // Four tracks so every band is distinct (the default cycle has one).
        let frame = |name: &str| super::DayCycleFrame {
            keyframe: 0.0,
            name: name.to_owned(),
        };
        env.day_cycle.sky_tracks = vec![
            vec![frame("ground")],
            vec![frame("mid")],
            vec![frame("high")],
            vec![frame("space")],
        ];
        assert_eq!(env.sky_track_for_altitude(0.0), 0);
        assert_eq!(env.sky_track_for_altitude(1000.0), 0);
        assert_eq!(env.sky_track_for_altitude(1000.1), 1);
        assert_eq!(env.sky_track_for_altitude(2000.0), 1);
        assert_eq!(env.sky_track_for_altitude(2500.0), 2);
        assert_eq!(env.sky_track_for_altitude(3000.0), 2);
        assert_eq!(env.sky_track_for_altitude(9000.0), 3);
    }

    #[test]
    fn sky_track_selection_clamps_to_available_tracks() {
        // The default cycle carries only the surface track, so every altitude
        // must resolve to it rather than an out-of-range index.
        let env = EnvironmentSettings::legacy_windlight_default();
        assert_eq!(env.day_cycle.sky_tracks.len(), 1);
        assert_eq!(env.sky_track_for_altitude(0.0), 0);
        assert_eq!(env.sky_track_for_altitude(50_000.0), 0);
        // And the surface frame resolves to a defined sky frame at any day time.
        assert!(env.active_sky_settings(50_000.0, 0.0).is_some());
        assert!(env.active_sky_settings(50_000.0, 0.5).is_some());
    }

    #[test]
    fn active_keyframe_picks_the_frame_in_force_and_wraps_before_the_first() {
        use super::{DayCycle, active_keyframe};
        let track = vec![
            super::DayCycleFrame {
                keyframe: 0.25,
                name: "morning".to_owned(),
            },
            super::DayCycleFrame {
                keyframe: 0.75,
                name: "evening".to_owned(),
            },
        ];
        // At/after a keyframe, that frame is in force.
        assert_eq!(
            active_keyframe(&track, 0.25).map(|f| f.name.as_str()),
            Some("morning")
        );
        assert_eq!(
            active_keyframe(&track, 0.5).map(|f| f.name.as_str()),
            Some("morning")
        );
        assert_eq!(
            active_keyframe(&track, 0.9).map(|f| f.name.as_str()),
            Some("evening")
        );
        // Before the first keyframe the cycle wraps to the last frame.
        assert_eq!(
            active_keyframe(&track, 0.1).map(|f| f.name.as_str()),
            Some("evening")
        );
        // An empty track has no active frame.
        let empty = DayCycle {
            name: String::new(),
            water_track: Vec::new(),
            sky_tracks: Vec::new(),
            sky_frames: std::collections::BTreeMap::new(),
            water_frames: std::collections::BTreeMap::new(),
        };
        assert!(active_keyframe(&empty.water_track, 0.5).is_none());
    }

    #[test]
    fn default_sky_sun_rotation_is_a_unit_quaternion() {
        let sky = SkySettings::legacy_windlight_default("Default");
        let rotation = sky.sun_rotation;
        // A rotation must be a unit quaternion; the fallback sun should point up
        // and away from straight ahead (a non-identity daytime track).
        let length_squared = rotation.x * rotation.x
            + rotation.y * rotation.y
            + rotation.z * rotation.z
            + rotation.s * rotation.s;
        assert!((length_squared - 1.0).abs() < 1.0e-4);
        assert!(rotation.s.to_bits() != 1.0_f32.to_bits());
    }

    #[test]
    fn bounding_keyframes_brackets_the_position_and_wraps_across_the_day() {
        use super::{DayCycleFrame, bounding_keyframes};
        let track = vec![
            DayCycleFrame {
                keyframe: 0.25,
                name: "morning".to_owned(),
            },
            DayCycleFrame {
                keyframe: 0.75,
                name: "evening".to_owned(),
            },
        ];
        // The bounds' names plus the blend factor, for a lint-clean assertion
        // (no `expect`, and the factor is compared approximately).
        let bracket = |position: f32| {
            bounding_keyframes(&track, position)
                .map(|(lower, upper, factor)| (lower.name.clone(), upper.name.clone(), factor))
        };
        // Mid-morning: bracketed by morning→evening, half-way between them.
        assert!(bracket(0.5).is_some_and(|(lower, upper, factor)| {
            lower == "morning" && upper == "evening" && (factor - 0.5).abs() < 1.0e-6
        }));
        // After the last keyframe the upper wraps to the first (next day):
        // span 0.75→1.25, position 0.9 → (0.9 - 0.75) / 0.5 = 0.3.
        assert!(bracket(0.9).is_some_and(|(lower, upper, factor)| {
            lower == "evening" && upper == "morning" && (factor - 0.3).abs() < 1.0e-6
        }));
        // Before the first keyframe the lower wraps to the last (previous day):
        // span -0.25→0.25, position 0.1 → (0.1 + 0.25) / 0.5 = 0.7.
        assert!(bracket(0.1).is_some_and(|(lower, upper, factor)| {
            lower == "evening" && upper == "morning" && (factor - 0.7).abs() < 1.0e-6
        }));
    }

    #[test]
    fn bounding_keyframes_of_a_single_frame_track_returns_it_as_both_bounds() {
        use super::{DayCycleFrame, bounding_keyframes};
        let track = vec![DayCycleFrame {
            keyframe: 0.4,
            name: "only".to_owned(),
        }];
        assert!(
            bounding_keyframes(&track, 0.9).is_some_and(|(lower, upper, factor)| {
                lower.name == "only"
                    && upper.name == "only"
                    && factor.to_bits() == 0.0_f32.to_bits()
            })
        );
        // An empty track has no bounds.
        assert!(bounding_keyframes(&[], 0.5).is_none());
    }

    #[test]
    fn sky_blend_interpolates_scalars_and_snaps_at_the_endpoints() {
        let mut a = SkySettings::legacy_windlight_default("A");
        let mut b = SkySettings::legacy_windlight_default("B");
        a.cloud_shadow = 0.2;
        b.cloud_shadow = 0.8;
        a.gamma = 1.0;
        b.gamma = 2.0;
        // Endpoints reproduce the source frames exactly.
        let at_zero = a.blend(&b, 0.0);
        assert_eq!(at_zero.cloud_shadow.to_bits(), 0.2_f32.to_bits());
        assert_eq!(at_zero.gamma.to_bits(), 1.0_f32.to_bits());
        let at_one = a.blend(&b, 1.0);
        assert!((at_one.cloud_shadow - 0.8).abs() < 1.0e-6);
        assert!((at_one.gamma - 2.0).abs() < 1.0e-6);
        // Midpoint is the arithmetic mean of each scalar.
        let mid = a.blend(&b, 0.5);
        assert!((mid.cloud_shadow - 0.5).abs() < 1.0e-6);
        assert!((mid.gamma - 1.5).abs() < 1.0e-6);
    }

    #[test]
    fn sky_blend_slerps_rotations_to_a_unit_quaternion() {
        let a = SkySettings::legacy_windlight_default("A");
        let b = SkySettings::legacy_windlight_default("B");
        // Give the two frames genuinely different sun orientations to slerp.
        let mut b = b;
        b.sun_rotation = a.moon_rotation.clone();
        let mid = a.blend(&b, 0.5);
        let r = &mid.sun_rotation;
        let length_squared = r.x * r.x + r.y * r.y + r.z * r.z + r.s * r.s;
        assert!((length_squared - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn sky_blend_snaps_textures_and_name_at_the_halfway_point() {
        use sl_types::key::TextureKey;
        use uuid::Uuid;
        let mut a = SkySettings::legacy_windlight_default("A");
        let mut b = SkySettings::legacy_windlight_default("B");
        a.sun_texture = Some(TextureKey::from(Uuid::from_u128(1)));
        b.sun_texture = Some(TextureKey::from(Uuid::from_u128(2)));
        // Below halfway the lower frame's discrete settings win; at/above, the upper.
        assert_eq!(a.blend(&b, 0.25).sun_texture, a.sun_texture);
        assert_eq!(a.blend(&b, 0.25).name, "A");
        assert_eq!(a.blend(&b, 0.75).sun_texture, b.sun_texture);
        assert_eq!(a.blend(&b, 0.75).name, "B");
    }

    #[test]
    fn blended_sky_settings_interpolates_between_the_bounding_keyframes() {
        use super::{DayCycle, DayCycleFrame};
        use std::collections::BTreeMap;
        let mut dawn = SkySettings::legacy_windlight_default("dawn");
        let mut dusk = SkySettings::legacy_windlight_default("dusk");
        dawn.cloud_shadow = 0.0;
        dusk.cloud_shadow = 1.0;
        let mut sky_frames = BTreeMap::new();
        drop(sky_frames.insert("dawn".to_owned(), dawn));
        drop(sky_frames.insert("dusk".to_owned(), dusk));
        let track = vec![
            DayCycleFrame {
                keyframe: 0.0,
                name: "dawn".to_owned(),
            },
            DayCycleFrame {
                keyframe: 0.5,
                name: "dusk".to_owned(),
            },
        ];
        let mut env = EnvironmentSettings::legacy_windlight_default();
        env.day_cycle = DayCycle {
            name: "test".to_owned(),
            water_track: Vec::new(),
            sky_tracks: vec![track],
            sky_frames,
            water_frames: BTreeMap::new(),
        };
        // A quarter of the way from dawn (0.0) to dusk (0.5): factor 0.5.
        assert!(
            env.blended_sky_settings(0.0, 0.25)
                .is_some_and(|quarter| (quarter.cloud_shadow - 0.5).abs() < 1.0e-6)
        );
        // Exactly on the dawn keyframe: the dawn frame, unblended.
        assert!(
            env.blended_sky_settings(0.0, 0.0)
                .is_some_and(|at_dawn| at_dawn.cloud_shadow.abs() < 1.0e-6)
        );
    }

    #[test]
    fn blended_sky_settings_of_the_default_cycle_returns_its_single_frame() {
        // The built-in default cycle has one keyframe, so every day position and
        // altitude blends the same frame with itself — its values unchanged.
        let env = EnvironmentSettings::legacy_windlight_default();
        let reference = SkySettings::legacy_windlight_default("Default");
        assert!(env.blended_sky_settings(0.0, 0.5).is_some_and(|noon| {
            noon.cloud_shadow.to_bits() == reference.cloud_shadow.to_bits()
                && noon.gamma.to_bits() == reference.gamma.to_bits()
                && noon.name == reference.name
        }));
    }

    #[test]
    fn water_blend_interpolates_scalars_and_snaps_at_the_endpoints() {
        use super::WaterSettings;
        let mut calm = WaterSettings::legacy_default("calm");
        let mut choppy = WaterSettings::legacy_default("choppy");
        calm.fresnel_scale = 0.2;
        choppy.fresnel_scale = 0.8;
        // Halfway: the scalar is the mean.
        let mid = calm.blend(&choppy, 0.5);
        assert!((mid.fresnel_scale - 0.5).abs() < 1.0e-6);
        // Endpoints reproduce each frame's scalar exactly.
        assert_eq!(
            calm.blend(&choppy, 0.0).fresnel_scale.to_bits(),
            0.2_f32.to_bits()
        );
        assert_eq!(
            calm.blend(&choppy, 1.0).fresnel_scale.to_bits(),
            0.8_f32.to_bits()
        );
    }

    #[test]
    fn water_blend_lerps_scale_and_wave_vectors() {
        use super::{Scale, WaterSettings};
        let mut calm = WaterSettings::legacy_default("calm");
        let mut choppy = WaterSettings::legacy_default("choppy");
        calm.normal_scale = Scale::new(1.0, 1.0, 1.0);
        choppy.normal_scale = Scale::new(3.0, 3.0, 3.0);
        calm.wave1_direction = [0.0, 0.0];
        choppy.wave1_direction = [1.0, -1.0];
        let mid = calm.blend(&choppy, 0.5);
        assert!((mid.normal_scale.x() - 2.0).abs() < 1.0e-6);
        assert!((mid.wave1_direction[0] - 0.5).abs() < 1.0e-6);
        assert!((mid.wave1_direction[1] + 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn water_blend_snaps_textures_and_name_at_the_halfway_point() {
        use super::Uuid;
        use super::WaterSettings;
        use sl_types::key::TextureKey;
        let mut calm = WaterSettings::legacy_default("calm");
        let mut choppy = WaterSettings::legacy_default("choppy");
        calm.normal_map = Some(TextureKey::from(Uuid::from_u128(1)));
        choppy.normal_map = Some(TextureKey::from(Uuid::from_u128(2)));
        // Below halfway holds `self`; above halfway snaps to `other`.
        assert_eq!(calm.blend(&choppy, 0.4).name, "calm");
        assert_eq!(calm.blend(&choppy, 0.4).normal_map, calm.normal_map);
        assert_eq!(calm.blend(&choppy, 0.6).name, "choppy");
        assert_eq!(calm.blend(&choppy, 0.6).normal_map, choppy.normal_map);
    }

    #[test]
    fn blended_water_settings_interpolates_between_the_bounding_keyframes() {
        use super::{DayCycle, DayCycleFrame, WaterSettings};
        use std::collections::BTreeMap;
        let mut calm = WaterSettings::legacy_default("calm");
        let mut choppy = WaterSettings::legacy_default("choppy");
        calm.fresnel_scale = 0.0;
        choppy.fresnel_scale = 1.0;
        let mut water_frames = BTreeMap::new();
        drop(water_frames.insert("calm".to_owned(), calm));
        drop(water_frames.insert("choppy".to_owned(), choppy));
        let track = vec![
            DayCycleFrame {
                keyframe: 0.0,
                name: "calm".to_owned(),
            },
            DayCycleFrame {
                keyframe: 0.5,
                name: "choppy".to_owned(),
            },
        ];
        let mut env = EnvironmentSettings::legacy_windlight_default();
        env.day_cycle = DayCycle {
            name: "test".to_owned(),
            water_track: track,
            sky_tracks: vec![vec![DayCycleFrame {
                keyframe: 0.0,
                name: "Default".to_owned(),
            }]],
            sky_frames: env.day_cycle.sky_frames.clone(),
            water_frames,
        };
        // A quarter of the way from calm (0.0) to choppy (0.5): factor 0.5.
        assert!(
            env.blended_water_settings(0.25)
                .is_some_and(|quarter| (quarter.fresnel_scale - 0.5).abs() < 1.0e-6)
        );
        // The active (unblended) selection at that position is still the calm frame.
        assert!(
            env.active_water_settings(0.25)
                .is_some_and(|active| active.fresnel_scale.abs() < 1.0e-6)
        );
    }

    #[test]
    fn blended_water_settings_of_the_default_cycle_returns_its_single_frame() {
        use super::WaterSettings;
        // The built-in default cycle has one water keyframe, so every day position
        // blends the same frame with itself — its values unchanged.
        let env = EnvironmentSettings::legacy_windlight_default();
        let reference = WaterSettings::legacy_default(super::DEFAULT_WATER_FRAME);
        assert!(env.blended_water_settings(0.5).is_some_and(|noon| {
            noon.fresnel_scale.to_bits() == reference.fresnel_scale.to_bits()
                && noon.water_fog_density.to_bits() == reference.water_fog_density.to_bits()
                && noon.name == reference.name
        }));
    }

    #[test]
    fn settings_kind_reads_the_reference_subtype_bytes_both_ways() {
        use super::SettingsKind;
        // `LLSettingsType::type_e`: ST_SKY = 0, ST_WATER = 1, ST_DAYCYCLE = 2.
        for (kind, byte) in [
            (SettingsKind::Sky, 0_u8),
            (SettingsKind::Water, 1),
            (SettingsKind::DayCycle, 2),
        ] {
            assert_eq!(kind.subtype(), byte);
            assert_eq!(SettingsKind::from_subtype(byte), Some(kind));
        }
        // ST_INVALID (255) and everything else names no kind.
        assert_eq!(SettingsKind::from_subtype(3), None);
        assert_eq!(SettingsKind::from_subtype(255), None);
    }

    #[test]
    fn settings_kind_masks_the_low_flag_byte_and_ignores_the_rest() {
        use super::SettingsKind;
        // Only `II_FLAGS_SUBTYPE_MASK` carries the kind: the high bits are other
        // item flags (`II_FLAGS_OBJECT_SLAM_PERM`, the shared-reference bit, …)
        // and must not change the answer.
        assert_eq!(SettingsKind::from_item_flags(0), Some(SettingsKind::Sky));
        assert_eq!(
            SettingsKind::from_item_flags(0xdead_ff00 | 2),
            Some(SettingsKind::DayCycle)
        );
        assert_eq!(
            SettingsKind::from_item_flags(0x4000_0001),
            Some(SettingsKind::Water)
        );
        // A byte no kind claims is refused rather than cast into one, which is
        // where the reference's unchecked cast lands its `default:` arm.
        assert_eq!(SettingsKind::from_item_flags(0xff), None);
    }

    #[test]
    fn a_decoded_asset_reports_the_kind_its_flags_would_have_carried() {
        use super::{EnvironmentAsset, SettingsKind, SkySettings, WaterSettings};
        let sky = EnvironmentAsset::Sky(Box::new(SkySettings::legacy_windlight_default(
            super::DEFAULT_SKY_FRAME,
        )));
        let water =
            EnvironmentAsset::Water(WaterSettings::legacy_default(super::DEFAULT_WATER_FRAME));
        let day = EnvironmentAsset::DayCycle(Box::new(
            EnvironmentSettings::legacy_windlight_default().day_cycle,
        ));
        assert_eq!(sky.kind(), SettingsKind::Sky);
        assert_eq!(water.kind(), SettingsKind::Water);
        assert_eq!(day.kind(), SettingsKind::DayCycle);
    }

    // -----------------------------------------------------------------------
    // Editing a day cycle (`LLSettingsDay`'s own track operations).
    // -----------------------------------------------------------------------

    /// A cycle with one sky keyframe on the ground track and one water
    /// keyframe — the shape the built-in default has, and what every editing
    /// test below starts from.
    fn one_frame_cycle() -> super::DayCycle {
        EnvironmentSettings::legacy_windlight_default().day_cycle
    }

    /// The reference numbers its tracks `0..=4` with water first; the sky
    /// tracks here are a list of their own. A round trip through both
    /// numberings is what keeps a keyframe on the track it was put on.
    #[test]
    fn a_track_survives_the_reference_numbering_both_ways() {
        use super::{DayTrack, SKY_TRACK_COUNT};
        for track in DayTrack::all() {
            assert_eq!(
                DayTrack::from_reference_index(track.reference_index()),
                Some(track)
            );
        }
        assert_eq!(DayTrack::from_reference_index(0), Some(DayTrack::Water));
        assert_eq!(DayTrack::from_reference_index(1), Some(DayTrack::GROUND));
        // Past the last sky track there is no track, rather than a fifth one.
        assert_eq!(
            DayTrack::from_reference_index(SKY_TRACK_COUNT.saturating_add(1)),
            None
        );
        assert_eq!(DayTrack::Water.sky_index(), None);
        assert_eq!(DayTrack::Sky(2).sky_index(), Some(2));
    }

    /// **An altitude track a cycle does not carry reads as empty**, and writing
    /// to it grows the list. A day cycle off a grid usually has one sky track;
    /// the editor lets somebody put a sky at 3000 m on it, and that has to
    /// materialise rather than be dropped.
    #[test]
    fn writing_to_an_absent_altitude_track_materialises_it() {
        use super::{DayTrack, SkySettings};
        let mut cycle = one_frame_cycle();
        assert_eq!(cycle.sky_tracks.len(), 1);
        assert!(cycle.track(DayTrack::Sky(3)).is_empty());
        let filed = cycle.insert_sky_keyframe(
            DayTrack::Sky(3),
            0.5,
            SkySettings::legacy_windlight_default("High"),
        );
        assert_eq!(filed.as_deref(), Some("High"));
        assert_eq!(cycle.sky_tracks.len(), 4);
        assert_eq!(cycle.track(DayTrack::Sky(3)).len(), 1);
        // The tracks in between exist and are empty, which is what a cycle with
        // no separate sky in those bands means.
        assert!(cycle.track(DayTrack::Sky(1)).is_empty());
    }

    /// **Two keyframes cannot sit on top of each other.** The reference refuses
    /// an insert within its slop factor because its own timeline widget cannot
    /// tell two such handles apart — and a pair of keyframes a fraction of a
    /// percent apart is a discontinuity nobody meant to author.
    #[test]
    fn a_keyframe_cannot_be_added_onto_another() {
        use super::{DayTrack, KEYFRAME_SLOP, SkySettings};
        let mut cycle = one_frame_cycle();
        let sky = || SkySettings::legacy_windlight_default("Noon");
        assert!(
            cycle
                .insert_sky_keyframe(DayTrack::GROUND, 0.5, sky())
                .is_some()
        );
        // Inside the slop of the one just added: refused.
        assert!(
            cycle
                .insert_sky_keyframe(DayTrack::GROUND, 0.5 + KEYFRAME_SLOP / 2.0, sky())
                .is_none()
        );
        // Just outside it: allowed, and named apart from its neighbour.
        let far = cycle.insert_sky_keyframe(DayTrack::GROUND, 0.5 + KEYFRAME_SLOP * 2.0, sky());
        assert_eq!(far.as_deref(), Some("Noon (2)"));
        assert_eq!(cycle.track(DayTrack::GROUND).len(), 3);
    }

    /// The slop lookup goes the short way round midnight, so the last keyframe
    /// of the day is near the first position of the next one.
    #[test]
    fn a_keyframe_before_midnight_is_near_a_position_after_it() {
        use super::{DayTrack, KEYFRAME_SLOP, SkySettings};
        let mut cycle = one_frame_cycle();
        drop(cycle.insert_sky_keyframe(
            DayTrack::GROUND,
            0.99,
            SkySettings::legacy_windlight_default("Midnight"),
        ));
        assert!(
            cycle
                .keyframe_near(DayTrack::GROUND, 0.005, KEYFRAME_SLOP)
                .is_some()
        );
    }

    /// A keyframe stays sorted when it is dragged past its neighbours, and it
    /// refuses to land on one.
    #[test]
    fn moving_a_keyframe_keeps_the_track_ordered_and_refuses_a_collision() {
        use super::{DayTrack, SkySettings};
        let mut cycle = one_frame_cycle();
        for (position, name) in [(0.25, "Morning"), (0.75, "Evening")] {
            drop(cycle.insert_sky_keyframe(
                DayTrack::GROUND,
                position,
                SkySettings::legacy_windlight_default(name),
            ));
        }
        // The ground track is now [0.0 default, 0.25 Morning, 0.75 Evening].
        let moved = cycle.move_keyframe(DayTrack::GROUND, 1, 0.9);
        assert_eq!(moved, Some(2), "dragging past a neighbour re-sorts");
        let names: Vec<&str> = cycle
            .track(DayTrack::GROUND)
            .iter()
            .map(|frame| frame.name.as_str())
            .collect();
        assert_eq!(names, [super::DEFAULT_SKY_FRAME, "Evening", "Morning"]);
        // Onto its new neighbour: refused, and nothing moves.
        assert_eq!(cycle.move_keyframe(DayTrack::GROUND, 2, 0.75), None);
        assert_eq!(
            cycle.track(DayTrack::GROUND).get(2).map(|f| f.keyframe),
            Some(0.9)
        );
    }

    /// **Removing the last keyframe of a track that must have one is refused**,
    /// and removing any other takes its frame definition with it — the asset
    /// would otherwise grow a frame for every edit ever made and never shrink.
    #[test]
    fn removing_a_keyframe_drops_its_frame_but_never_empties_the_ground() {
        use super::{DayTrack, SkySettings};
        let mut cycle = one_frame_cycle();
        drop(cycle.insert_sky_keyframe(
            DayTrack::GROUND,
            0.5,
            SkySettings::legacy_windlight_default("Noon"),
        ));
        assert!(cycle.sky_frames.contains_key("Noon"));
        assert!(cycle.remove_keyframe(DayTrack::GROUND, 1));
        assert!(
            !cycle.sky_frames.contains_key("Noon"),
            "an orphan frame goes"
        );
        // One left on the ground track, and it stays.
        assert!(!cycle.remove_keyframe(DayTrack::GROUND, 0));
        assert_eq!(cycle.track(DayTrack::GROUND).len(), 1);
        // The water track is the same: never empty.
        assert!(!cycle.remove_keyframe(DayTrack::Water, 0));
        // An altitude track may be emptied completely.
        drop(cycle.insert_sky_keyframe(
            DayTrack::Sky(2),
            0.5,
            SkySettings::legacy_windlight_default("High"),
        ));
        assert!(cycle.remove_keyframe(DayTrack::Sky(2), 0));
        assert!(cycle.track(DayTrack::Sky(2)).is_empty());
    }

    /// Clearing follows the same rule: an altitude track goes entirely, the
    /// water and ground tracks down to their first keyframe.
    #[test]
    fn clearing_a_track_keeps_what_a_cycle_cannot_do_without() {
        use super::{DayTrack, SkySettings};
        let mut cycle = one_frame_cycle();
        for (track, position, name) in [
            (DayTrack::GROUND, 0.3, "Morning"),
            (DayTrack::GROUND, 0.6, "Evening"),
            (DayTrack::Sky(1), 0.2, "High A"),
            (DayTrack::Sky(1), 0.8, "High B"),
        ] {
            drop(cycle.insert_sky_keyframe(
                track,
                position,
                SkySettings::legacy_windlight_default(name),
            ));
        }
        cycle.clear_track(DayTrack::Sky(1));
        assert!(cycle.track(DayTrack::Sky(1)).is_empty());
        assert!(!cycle.sky_frames.contains_key("High A"));

        cycle.clear_track(DayTrack::GROUND);
        assert_eq!(cycle.track(DayTrack::GROUND).len(), 1);
        assert!(!cycle.sky_frames.contains_key("Evening"));
        // And the one frame the ground track kept is still defined.
        let kept = cycle
            .track(DayTrack::GROUND)
            .first()
            .map(|frame| frame.name.clone())
            .unwrap_or_default();
        assert!(cycle.sky_frames.contains_key(&kept));
    }

    /// **A cloned track is copies, not references.** The reference clones each
    /// frame (`buildDerivedClone`) for exactly this reason: editing the sky at
    /// 2000 m must not reach down and change the one at ground level.
    #[test]
    fn cloning_a_track_copies_its_frames_rather_than_sharing_them() {
        use super::{DayTrack, SkySettings};
        let mut cycle = one_frame_cycle();
        drop(cycle.insert_sky_keyframe(
            DayTrack::GROUND,
            0.5,
            SkySettings::legacy_windlight_default("Noon"),
        ));
        let source = cycle.clone();
        assert!(cycle.clone_track(&source, DayTrack::GROUND, DayTrack::Sky(1)));
        assert_eq!(cycle.track(DayTrack::Sky(1)).len(), 2);
        // Every keyframe of the copy names a frame of its own.
        let ground: Vec<&str> = cycle
            .track(DayTrack::GROUND)
            .iter()
            .map(|frame| frame.name.as_str())
            .collect();
        for frame in cycle.track(DayTrack::Sky(1)) {
            assert!(!ground.contains(&frame.name.as_str()), "{}", frame.name);
        }
        // Editing the copy leaves the original alone.
        let copied = cycle
            .track(DayTrack::Sky(1))
            .first()
            .map(|frame| frame.name.clone())
            .unwrap_or_default();
        if let Some(frame) = cycle.sky_frames.get_mut(&copied) {
            frame.haze_density = 9.0;
        }
        let original = cycle
            .track(DayTrack::GROUND)
            .first()
            .map(|frame| frame.name.clone())
            .unwrap_or_default();
        assert!(
            cycle
                .sky_frames
                .get(&original)
                .is_some_and(|frame| frame.haze_density < 9.0)
        );
        // Water and sky do not mix.
        assert!(!cycle.clone_track(&source, DayTrack::Water, DayTrack::GROUND));
        assert!(!cycle.clone_track(&source, DayTrack::GROUND, DayTrack::Water));
    }

    /// **A frame two keyframes share is split before either is edited.** One
    /// asset may legally name the same frame twice, and an editor that wrote
    /// into it would change a keyframe the user never selected.
    #[test]
    fn editing_a_shared_frame_gives_the_keyframe_one_of_its_own() {
        use super::{DayCycleFrame, DayTrack};
        let mut cycle = one_frame_cycle();
        let shared = cycle
            .track(DayTrack::GROUND)
            .first()
            .map(|frame| frame.name.clone())
            .unwrap_or_default();
        // A second keyframe naming the same frame — what a hand-written asset
        // (or another viewer) may well contain.
        if let Some(track) = cycle.sky_tracks.first_mut() {
            track.push(DayCycleFrame {
                keyframe: 0.5,
                name: shared.clone(),
            });
        }
        let split = cycle.split_shared_frame(DayTrack::GROUND, 1);
        assert!(split.is_some());
        pretty_assertions::assert_ne!(split.as_deref(), Some(shared.as_str()));
        assert_eq!(cycle.sky_frames.len(), 2);
        // Splitting again is a no-op: it is nobody else's frame now.
        let again = cycle.split_shared_frame(DayTrack::GROUND, 1);
        assert_eq!(again, split);
        assert_eq!(cycle.sky_frames.len(), 2);
    }

    /// The per-track blend the editor previews through is the one the renderer
    /// uses once an altitude has chosen the track — same function, and a track
    /// index rather than a height is the whole difference.
    #[test]
    fn the_editor_s_blend_and_the_renderer_s_are_the_same_lookup() {
        use super::DayTrack;
        let mut settings = EnvironmentSettings::legacy_windlight_default();
        settings.track_altitudes = [1000.0, 2000.0, 3000.0];
        let cycle = &mut settings.day_cycle;
        drop(cycle.insert_sky_keyframe(
            DayTrack::GROUND,
            0.5,
            SkySettings {
                haze_density: 3.0,
                ..SkySettings::legacy_windlight_default("Noon")
            },
        ));
        let by_track = settings.day_cycle.blended_sky(0, 0.5);
        let by_altitude = settings.blended_sky_settings(10.0, 0.5);
        assert_eq!(
            by_track.map(|sky| sky.haze_density),
            by_altitude.map(|sky| sky.haze_density)
        );
        assert_eq!(
            settings
                .day_cycle
                .blended_water(0.25)
                .map(|water| water.name),
            settings
                .blended_water_settings(0.25)
                .map(|water| water.name)
        );
    }
}
