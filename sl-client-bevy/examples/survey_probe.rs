//! A headless Bevy app that logs in, then on region handshake exercises the
//! survey commands (draw distance, region info, parcel properties, map blocks)
//! and logs every event it receives, before logging out after a fixed window.
//!
//! This is a parity probe for the tokio survey path: the key thing it proves is
//! that `ParcelProperties` (delivered only over the CAPS event queue) arrives,
//! i.e. the background event-queue poller works under Bevy.
//!
//! Configure via the same environment variables as the other examples:
//!   `SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`, `SL_START`,
//!   `SL_CHANNEL`, `SL_VERSION`, `SL_COLLECT_SECS`.

use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use sl_client_bevy::{
    LoginParams, LoginRequest, SessionDisconnectReason, SlClientPlugin, SlCommand, SlEvent,
    SlSessionEvent,
};
use tracing::{info, warn};

/// Tracks the collection window and what we have seen.
#[derive(Resource)]
struct ProbeState {
    /// How long to collect events after the handshake.
    collect: Duration,
    /// When to request logout, set once the handshake completes.
    logout_at: Option<Instant>,
    /// Whether logout has been requested.
    requested: bool,
    /// Whether a `ParcelProperties` (CAPS-only) event has arrived.
    saw_parcel: bool,
    /// Whether a `RegionInfoHandshake`/`RegionLimits` event has arrived.
    saw_region_info: bool,
    /// How many `MapBlock` events have arrived.
    map_blocks: u32,
}

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last");
    let channel = env_or("SL_CHANNEL", "sl-client-bevy-probe");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let collect_secs: u64 = env_or("SL_COLLECT_SECS", "20").parse()?;

    let params = LoginParams {
        login_uri,
        request: LoginRequest::new(first, last, password, start, channel, version),
    };

    info!("starting Bevy survey probe");
    let _exit = App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))))
        .add_plugins(SlClientPlugin { params })
        .insert_resource(ProbeState {
            collect: Duration::from_secs(collect_secs),
            logout_at: None,
            requested: false,
            saw_parcel: false,
            saw_region_info: false,
            map_blocks: 0,
        })
        .add_systems(Update, (on_events, maybe_logout))
        .run();
    Ok(())
}

/// Logs every event and, on the region handshake, fires the survey commands.
fn on_events(
    mut events: EventReader<SlEvent>,
    mut state: ResMut<ProbeState>,
    mut commands: EventWriter<SlCommand>,
    mut exit: EventWriter<AppExit>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::CircuitEstablished { sim } => info!("circuit established to {sim}"),
            SlSessionEvent::RegionHandshakeComplete => {
                info!("region handshake complete; firing survey commands");
                state.logout_at = Instant::now().checked_add(state.collect);
                // Draw distance, then the three survey requests. The parcel
                // query covers the whole 256m region so at least one parcel
                // overlaps it.
                commands.write(SlCommand::SetDrawDistance(128.0));
                commands.write(SlCommand::RequestRegionInfo);
                commands.write(SlCommand::RequestParcelProperties {
                    west: 0.0,
                    south: 0.0,
                    east: 256.0,
                    north: 256.0,
                    sequence_id: 1,
                });
                commands.write(SlCommand::RequestMapBlocks {
                    min_x: 999,
                    max_x: 1001,
                    min_y: 999,
                    max_y: 1001,
                });
            }
            SlSessionEvent::RegionInfoHandshake(identity) => {
                info!("region info handshake: {identity:?}");
                state.saw_region_info = true;
            }
            SlSessionEvent::RegionLimits(limits) => {
                info!("region limits: {limits:?}");
                state.saw_region_info = true;
            }
            SlSessionEvent::ParcelProperties(parcel) => {
                info!("PARCEL PROPERTIES (via CAPS event queue): {parcel:?}");
                state.saw_parcel = true;
            }
            SlSessionEvent::ParcelOverlay(_) => info!("parcel overlay block received"),
            SlSessionEvent::NeighborDiscovered(neighbor) => {
                info!("neighbour discovered: {neighbor:?}");
            }
            SlSessionEvent::MapBlock(region) => {
                info!("map block: {region:?}");
                state.map_blocks = state.map_blocks.saturating_add(1);
            }
            SlSessionEvent::ChatReceived(_)
            | SlSessionEvent::ChatTyping { .. }
            | SlSessionEvent::InstantMessageReceived(_)
            | SlSessionEvent::ImTyping { .. }
            | SlSessionEvent::SitResult { .. }
            | SlSessionEvent::AvatarProperties(_)
            | SlSessionEvent::AvatarInterests(_)
            | SlSessionEvent::AvatarGroups { .. }
            | SlSessionEvent::AvatarPicks { .. }
            | SlSessionEvent::AvatarNotes { .. }
            | SlSessionEvent::InventorySkeleton(_)
            | SlSessionEvent::InventoryDescendents { .. }
            | SlSessionEvent::FriendList(_)
            | SlSessionEvent::FriendsOnline(_)
            | SlSessionEvent::FriendsOffline(_)
            | SlSessionEvent::FriendRightsChanged { .. }
            | SlSessionEvent::ActiveGroupChanged(_)
            | SlSessionEvent::GroupMemberships(_)
            | SlSessionEvent::GroupMembers { .. }
            | SlSessionEvent::GroupRoleData { .. }
            | SlSessionEvent::GroupRoleMembers { .. }
            | SlSessionEvent::GroupTitles { .. }
            | SlSessionEvent::GroupProfileReceived(_)
            | SlSessionEvent::GroupNotices { .. }
            | SlSessionEvent::GroupSessionMessage { .. }
            | SlSessionEvent::GroupSessionParticipant { .. }
            | SlSessionEvent::CreateGroupResult { .. }
            | SlSessionEvent::JoinGroupResult { .. }
            | SlSessionEvent::LeaveGroupResult { .. }
            | SlSessionEvent::DroppedFromGroup { .. }
            | SlSessionEvent::ScriptDialog(_)
            | SlSessionEvent::ScriptPermissionRequest(_)
            | SlSessionEvent::LoadUrl(_)
            | SlSessionEvent::ScriptTeleport(_)
            | SlSessionEvent::MuteList(_)
            | SlSessionEvent::MuteListUnchanged
            | SlSessionEvent::TeleportStarted
            | SlSessionEvent::TeleportProgress { .. }
            | SlSessionEvent::TeleportLocal => {}
            SlSessionEvent::TeleportFailed { reason } => warn!("teleport failed: {reason}"),
            SlSessionEvent::RegionChanged { region_handle, sim } => {
                info!("region changed: handle={region_handle} sim={sim}");
            }
            SlSessionEvent::LoggedOut => {
                info!(
                    "logged out cleanly (saw_parcel={}, saw_region_info={}, map_blocks={})",
                    state.saw_parcel, state.saw_region_info, state.map_blocks
                );
                exit.write(AppExit::Success);
            }
            SlSessionEvent::Disconnected(reason) => {
                match reason {
                    SessionDisconnectReason::Timeout => warn!("disconnected: inactivity timeout"),
                    other => warn!("disconnected: {other:?}"),
                }
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Requests a clean logout once the collection window has elapsed.
fn maybe_logout(mut state: ResMut<ProbeState>, mut commands: EventWriter<SlCommand>) {
    if state.requested {
        return;
    }
    if let Some(deadline) = state.logout_at
        && Instant::now() >= deadline
    {
        info!("collection window elapsed; requesting logout");
        commands.write(SlCommand::Logout);
        state.requested = true;
    }
}
