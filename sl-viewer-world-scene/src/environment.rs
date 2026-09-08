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

use bevy::prelude::*;
use sl_client_bevy::{
    AssetKey, Command, DayCycle, DayCycleFrame, EnvironmentAsset, EnvironmentSettings, SkySettings,
    SlCommand, SlEvent, SlSessionEvent, Uuid, WaterSettings,
};

use sl_viewer_world_api::rlv::{RlvEnvironmentRequest, RlvEnvironmentSlot};

use crate::environment_assets::EnvironmentAssetManager;
use crate::sky_presets::FixedSky;

/// A World ▸ Environment menu selection: a time of day
/// ([`FixedSky`]) within one of three groups.
/// `None` on [`EnvironmentState`] means the region's shared environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default, PartialEq)]
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

/// Where the current [`EnvironmentState::settings`] came from — and, as
/// [`EnvironmentSource::of_reply`], the scope an incoming reply describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvironmentSource {
    /// The built-in legacy WindLight default — no grid settings ingested yet.
    Default,
    /// The whole-region environment (a `parcel_id` of `-1`).
    Region,
    /// A specific parcel's environment override. Never the source of
    /// [`EnvironmentState::shared`]: the reference viewer keeps a parcel
    /// override in its own `ENV_PARCEL` layer, above (not instead of) the
    /// region's `ENV_REGION` — see [`EnvironmentState::ingest_reply`].
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

/// How many times to (re)request the region environment before giving up and
/// rendering with the legacy WindLight defaults.
const MAX_ENV_ATTEMPTS: u32 = 12;

/// Seconds between environment-request retries while a request is outstanding.
const ENV_RETRY_INTERVAL: f32 = 3.0;

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
    /// The decoded sky for a pinned **Modern** selection, once its `KNOWN_SKY_*`
    /// asset resolves (see [`resolve_modern_environment`]), keyed by the time so a
    /// stale one is ignored after the selection changes. Until it resolves, a
    /// Modern selection renders the region's cycle at that time as a placeholder.
    modern_sky: Option<(FixedSky, SkySettings)>,
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
            source: EnvironmentSource::Default,
            shared: EnvironmentSettings::legacy_windlight_default(),
            shared_source: EnvironmentSource::Default,
            fixed: None,
            local: LocalEnvironment::default(),
            modern_sky: None,
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
        Self {
            pinned_day_position: overrides.day_position,
            ..Self::default()
        }
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
        if !matches!(local, EnvironmentAsset::Water(_)) {
            self.fixed = None;
            self.modern_sky = None;
        }
        self.local.install(local, source);
        self.apply();
    }

    /// Empty the local layer, falling back to whatever the menu has pinned
    /// (nothing, usually) and then to the shared environment.
    pub fn clear_local(&mut self) {
        self.local = LocalEnvironment::default();
        self.apply();
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
    /// parcel, not the region's settings; the reference viewer records it in a
    /// separate `ENV_PARCEL` layer that sits *above* `ENV_REGION`
    /// (`LLEnvironment::recordEnvironment`, `llenvironment.cpp:1874`) and never
    /// touches the region layer with it. Treating one as the shared environment
    /// would make a parcel override what "Use Shared Environment" restores, and
    /// would cancel the region retry loop before the region's own settings ever
    /// arrived. The viewer therefore leaves the shared environment (and the
    /// request cycle) alone here; rendering the parcel layer belongs to the
    /// environment-override work (`viewer-environment-personal-lighting`), which
    /// is also what will first ask for a parcel-scoped environment.
    fn ingest_reply(&mut self, settings: EnvironmentSettings) -> EnvironmentSource {
        let source = EnvironmentSource::of_reply(settings.parcel_id);
        if matches!(source, EnvironmentSource::Region) {
            self.ingest_shared(settings, source);
            // The region's reply landed — stop the request/retry loop.
            self.req_pending = false;
        }
        source
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
            self.settings.day_cycle.water_track = vec![DayCycleFrame {
                keyframe: 0.0,
                name: name.clone(),
            }];
            self.settings.day_cycle.water_frames =
                std::iter::once((name, water.settings.clone())).collect();
        }

        // Debug affordance: when a pinned day position (`SL_VIEWER_SKY_DAY_POSITION`)
        // (used by the screenshot harness and headless checks), install a full
        // day cycle synthesised from the four legacy presets so the pinned
        // position actually moves the sun — the local OpenSim grid ships a
        // single-frame environment, which leaves the position nothing to
        // interpolate (every value renders the same noon sky). A pinned fixed
        // environment (the World ▸ Environment menu) already selects a specific
        // frame and takes precedence, so the override only applies when none is
        // pinned.
        // Gated on the *resolved* override rather than on the variable merely being
        // present, so the synthesised cycle is installed exactly when the pin will
        // actually drive it (`crate::sky::day_position`); a malformed value falls
        // back to the clock, which the region's own environment already follows.
        if self.fixed.is_none() && self.pinned_day_position.is_some() {
            crate::sky_presets::install_preset_day_cycle(&mut self.settings);
        }
    }

    /// Replace the sky schedule of the environment being composed with a single
    /// `sky` frame pinned at keyframe 0 on the surface track (the upper altitude
    /// tracks empty out, so every altitude falls back to it); the water keeps
    /// following whichever cycle is in force. Shared by all three
    /// fixed-environment groups and by a local sky asset.
    fn pin_sky(&mut self, sky: SkySettings, name: String) {
        self.settings.day_cycle.sky_tracks = vec![vec![DayCycleFrame {
            keyframe: 0.0,
            name: name.clone(),
        }]];
        self.settings.day_cycle.sky_frames = std::iter::once((name, sky)).collect();
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

/// Request the region environment after each region handshake, retrying until the
/// grid's EEP reply is ingested (or `MAX_ENV_ATTEMPTS` is reached). A single
/// one-shot request is fragile: on a slower / remote grid the `ExtEnvironment`
/// capability may not be seeded yet when the handshake completes, so the runtime
/// silently drops the request and the sky / cloud / water stack is left on the
/// legacy WindLight defaults forever (observed on aditi). Retrying until
/// [`ingest_environment`] clears the pending flag closes that race — the same
/// cap-not-ready-yet class of bug the terrain fetch hit. Parcels can override the
/// region environment; the viewer asks for the whole-region settings here
/// (`parcel_id: None`).
pub fn request_environment(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut commands: MessageWriter<SlCommand>,
    mut state: ResMut<EnvironmentState>,
) {
    // A handshake (initial login or a border crossing) starts a fresh request
    // cycle for the new region's environment.
    for event in events.read() {
        if matches!(event.0, SlSessionEvent::RegionHandshakeComplete) {
            info!("region handshake complete; requesting environment (EEP) settings");
            state.req_pending = true;
            state.req_attempts = 0;
            state.req_next_retry_at = 0.0;
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

/// Fold an incoming [`SlSessionEvent::Environment`] into [`EnvironmentState`],
/// replacing the legacy default (or the previously ingested region environment)
/// with the grid's settings. A parcel-scoped reply is logged and dropped rather
/// than mistaken for the region's — see `EnvironmentState::ingest_reply`.
pub fn ingest_environment(mut events: MessageReader<SlEvent>, mut state: ResMut<EnvironmentState>) {
    for event in events.read() {
        if let SlSessionEvent::Environment(settings) = &event.0 {
            let sky_count = settings.day_cycle.sky_frames.len();
            let water_count = settings.day_cycle.water_frames.len();
            match state.ingest_reply((**settings).clone()) {
                // Kept out of the shared environment on purpose — see
                // `EnvironmentState::ingest_reply`.
                EnvironmentSource::Parcel => info!(
                    "environment reply for parcel {} ignored: a parcel override is not the \
                     region's shared environment ({sky_count} sky frame(s), \
                     {water_count} water frame(s), cycle {:?})",
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

    use super::{
        EnvironmentAsset, EnvironmentSettings, EnvironmentSource, EnvironmentState,
        FixedEnvironment, SkySettings, Uuid,
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

    /// The other half of the bug: "Use Shared Environment" restores the region's
    /// settings, never a parcel's override.
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
}
