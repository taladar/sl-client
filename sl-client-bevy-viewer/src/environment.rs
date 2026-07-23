//! Environment (EEP) ingest — the Phase 22.1 slice.
//!
//! The viewer holds one [`EnvironmentState`] resource: the region's (or a
//! parcel's) Extended-Environment settings — its sky, water, and day cycle. It
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
    Command, DayCycleFrame, EnvironmentSettings, SlCommand, SlEvent, SlSessionEvent,
};

use crate::sky_presets::FixedSky;

/// Where the current [`EnvironmentState::settings`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvironmentSource {
    /// The built-in legacy WindLight default — no grid settings ingested yet.
    Default,
    /// The whole-region environment (a `parcel_id` of `-1`).
    Region,
    /// A specific parcel's environment override.
    Parcel,
}

/// How many times to (re)request the region environment before giving up and
/// rendering with the legacy WindLight defaults.
const MAX_ENV_ATTEMPTS: u32 = 12;

/// Seconds between environment-request retries while a request is outstanding.
const ENV_RETRY_INTERVAL: f32 = 3.0;

/// The viewer's current environment: the sky / water / day-cycle settings the
/// later rendering phases draw from, plus where they came from.
#[derive(Resource)]
pub(crate) struct EnvironmentState {
    /// The active environment settings — what the sky / water / shadow phases
    /// render. Begins at the legacy WindLight default, is replaced when the
    /// grid answers a [`Command::RequestEnvironment`], and is *pinned* to a
    /// single preset frame while a fixed sky is selected
    /// ([`set_fixed_sky`](Self::set_fixed_sky)).
    pub(crate) settings: EnvironmentSettings,
    /// The provenance of [`Self::settings`].
    pub(crate) source: EnvironmentSource,
    /// The last **shared** (grid) environment: what [`Self::settings`] shows
    /// when no fixed sky is selected, and what "Use Shared Environment"
    /// restores. Kept current by [`ingest_environment`] even while a fixed sky
    /// is pinned, so un-pinning never renders stale grid settings.
    shared: EnvironmentSettings,
    /// The provenance of [`Self::shared`].
    shared_source: EnvironmentSource,
    /// The fixed sky pinned by the World ▸ Environment menu, if any — the
    /// reference viewer's local fixed environment
    /// (`LLEnvironment::setEnvironment(ENV_LOCAL, …)`), which survives region
    /// changes until "Use Shared Environment".
    fixed_sky: Option<FixedSky>,
    /// Whether a region-environment request is still outstanding — the retry loop
    /// keeps re-requesting until the reply is ingested or [`MAX_ENV_ATTEMPTS`] is
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
            source: EnvironmentSource::Default,
            shared: EnvironmentSettings::legacy_windlight_default(),
            shared_source: EnvironmentSource::Default,
            fixed_sky: None,
            req_pending: false,
            req_attempts: 0,
            req_next_retry_at: 0.0,
        }
    }
}

impl EnvironmentState {
    /// The fixed sky currently pinned by the World ▸ Environment menu, if any
    /// (drives the menu's check marks).
    pub(crate) const fn fixed_sky(&self) -> Option<FixedSky> {
        self.fixed_sky
    }

    /// Pin the rendered environment to `fixed` — a single-frame day cycle
    /// holding that preset sky over the shared environment's water — or restore
    /// the shared (grid) environment with `None`. The reference's World ▸
    /// Environment ▸ Sunrise / Midday / Sunset / Midnight / "Use Shared
    /// Environment".
    pub(crate) fn set_fixed_sky(&mut self, fixed: Option<FixedSky>) {
        self.fixed_sky = fixed;
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

    /// Recompute the active [`Self::settings`] from the shared environment and
    /// the pinned fixed sky.
    fn apply(&mut self) {
        match self.fixed_sky {
            None => {
                self.settings = self.shared.clone();
                self.source = self.shared_source;
            }
            Some(fixed) => {
                // The shared environment with its sky schedule replaced by one
                // frame pinned at keyframe 0 on the surface track (the upper
                // altitude tracks empty out, so every altitude falls back to
                // it); the water keeps following the shared cycle.
                let mut pinned = self.shared.clone();
                let sky = fixed.settings();
                let name = fixed.frame_name().to_owned();
                pinned.day_cycle.sky_tracks = vec![vec![DayCycleFrame {
                    keyframe: 0.0,
                    name: name.clone(),
                }]];
                pinned.day_cycle.sky_frames = std::iter::once((name, sky)).collect();
                self.settings = pinned;
                self.source = self.shared_source;
            }
        }
    }
}

/// Request the region environment after each region handshake, retrying until the
/// grid's EEP reply is ingested (or [`MAX_ENV_ATTEMPTS`] is reached). A single
/// one-shot request is fragile: on a slower / remote grid the `ExtEnvironment`
/// capability may not be seeded yet when the handshake completes, so the runtime
/// silently drops the request and the sky / cloud / water stack is left on the
/// legacy WindLight defaults forever (observed on aditi). Retrying until
/// [`ingest_environment`] clears the pending flag closes that race — the same
/// cap-not-ready-yet class of bug the terrain fetch hit. Parcels can override the
/// region environment; the viewer asks for the whole-region settings here
/// (`parcel_id: None`).
pub(crate) fn request_environment(
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
/// replacing the legacy default (or a previous region/parcel environment) with
/// the grid's settings.
pub(crate) fn ingest_environment(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<EnvironmentState>,
) {
    for event in events.read() {
        if let SlSessionEvent::Environment(settings) = &event.0 {
            let source = if settings.parcel_id < 0 {
                EnvironmentSource::Region
            } else {
                EnvironmentSource::Parcel
            };
            let sky_count = settings.day_cycle.sky_frames.len();
            let water_count = settings.day_cycle.water_frames.len();
            info!(
                "environment ingested ({source:?}): day_length={}s, day_offset={}s, \
                 {sky_count} sky frame(s), {water_count} water frame(s), cycle {:?}",
                settings.day_length, settings.day_offset, settings.day_cycle.name,
            );
            state.ingest_shared((**settings).clone(), source);
            // The reply landed — stop the request/retry loop for this region.
            state.req_pending = false;
        }
    }
}
