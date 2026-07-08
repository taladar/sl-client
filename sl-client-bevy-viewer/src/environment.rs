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
use sl_client_bevy::{Command, EnvironmentSettings, SlCommand, SlEvent, SlSessionEvent};

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

/// The viewer's current environment: the sky / water / day-cycle settings the
/// later rendering phases draw from, plus where they came from.
#[derive(Resource)]
pub(crate) struct EnvironmentState {
    /// The active environment settings. Begins at the legacy WindLight default and
    /// is replaced when the grid answers a [`Command::RequestEnvironment`].
    pub(crate) settings: EnvironmentSettings,
    /// The provenance of [`Self::settings`].
    pub(crate) source: EnvironmentSource,
}

impl Default for EnvironmentState {
    fn default() -> Self {
        Self {
            settings: EnvironmentSettings::legacy_windlight_default(),
            source: EnvironmentSource::Default,
        }
    }
}

/// Request the region environment once each region handshake completes, so the
/// grid's EEP settings replace the built-in default. Parcels can override the
/// region environment, but the viewer asks for the whole-region settings here
/// (`parcel_id: None`); a parcel-scoped request lands with the parcel work.
pub(crate) fn request_environment(
    mut events: MessageReader<SlEvent>,
    mut commands: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        if matches!(event.0, SlSessionEvent::RegionHandshakeComplete) {
            info!("region handshake complete; requesting environment (EEP) settings");
            commands.write(SlCommand(Command::RequestEnvironment { parcel_id: None }));
        }
    }
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
            state.settings = (**settings).clone();
            state.source = source;
        }
    }
}
