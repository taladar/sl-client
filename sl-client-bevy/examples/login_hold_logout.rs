//! A headless Bevy app that logs in to a Second Life / OpenSim grid, holds the
//! session alive past the server inactivity timeout, then logs out cleanly.
//!
//! Configure via the same environment variables as the tokio example:
//!   `SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`, `SL_START`,
//!   `SL_CHANNEL`, `SL_VERSION`, `SL_HOLD_SECS`.

use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use sl_client_bevy::{
    LoginParams, LoginRequest, SessionDisconnectReason, SlClientPlugin, SlCommand, SlEvent,
    SlSessionEvent,
};
use tracing::{info, warn};

/// Tracks when to request logout after the handshake completes.
#[derive(Resource)]
struct HoldState {
    /// How long to stay connected after the handshake.
    hold: Duration,
    /// When to request logout, set once the handshake completes.
    logout_at: Option<Instant>,
    /// Whether the logout has already been requested.
    requested: bool,
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
    let channel = env_or("SL_CHANNEL", "sl-client-bevy-example");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "90").parse()?;

    let params = LoginParams {
        login_uri,
        request: LoginRequest::new(first, last, password, start, channel, version),
    };

    info!("starting Bevy session");
    let _exit = App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))))
        .add_plugins(SlClientPlugin { params })
        .insert_resource(HoldState {
            hold: Duration::from_secs(hold_secs),
            logout_at: None,
            requested: false,
        })
        .add_systems(Update, (on_events, maybe_logout))
        .run();
    Ok(())
}

/// Logs session events and schedules logout once the region handshake lands.
fn on_events(
    mut events: EventReader<SlEvent>,
    mut hold: ResMut<HoldState>,
    mut exit: EventWriter<AppExit>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::CircuitEstablished { sim } => info!("circuit established to {sim}"),
            SlSessionEvent::RegionHandshakeComplete => {
                info!("region handshake complete; holding for {:?}", hold.hold);
                hold.logout_at = Instant::now().checked_add(hold.hold);
            }
            SlSessionEvent::LoggedOut => {
                info!("logged out cleanly");
                exit.write(AppExit::Success);
            }
            SlSessionEvent::Disconnected(reason) => {
                match reason {
                    SessionDisconnectReason::Timeout => warn!("disconnected: inactivity timeout"),
                    other => warn!("disconnected: {other:?}"),
                }
                exit.write(AppExit::Success);
            }
            SlSessionEvent::ChatReceived(chat) => {
                info!(
                    "chat from {} ({:?}): {}",
                    chat.from_name, chat.chat_type, chat.message
                );
            }
            SlSessionEvent::ChatTyping {
                from_name, typing, ..
            } => {
                info!(
                    "{from_name} {} typing",
                    if *typing { "started" } else { "stopped" }
                );
            }
            SlSessionEvent::InstantMessageReceived(im) => {
                info!(
                    "IM from {} ({:?}): {}",
                    im.from_agent_name, im.dialog, im.message
                );
            }
            SlSessionEvent::ImTyping {
                from_agent_name,
                typing,
                ..
            } => {
                info!(
                    "{from_agent_name} {} typing (IM)",
                    if *typing { "started" } else { "stopped" }
                );
            }
            SlSessionEvent::SitResult {
                sit_object,
                sit_position,
                ..
            } => {
                info!("sat on {sit_object} at {sit_position:?}");
            }
            SlSessionEvent::AvatarProperties(props) => {
                info!(
                    "avatar {} born {}: {}",
                    props.avatar_id, props.born_on, props.about_text
                );
            }
            SlSessionEvent::AvatarPicks { picks, .. } => info!("avatar has {} picks", picks.len()),
            SlSessionEvent::InventorySkeleton(folders) => {
                info!("inventory skeleton: {} folders", folders.len());
            }
            SlSessionEvent::InventoryDescendents { folders, items, .. } => {
                info!(
                    "folder contents: {} sub-folders, {} items",
                    folders.len(),
                    items.len()
                );
            }
            SlSessionEvent::FriendList(friends) => {
                info!("friend list: {} friend(s)", friends.len());
            }
            SlSessionEvent::FriendsOnline(ids) => info!("{} friend(s) came online", ids.len()),
            SlSessionEvent::FriendsOffline(ids) => info!("{} friend(s) went offline", ids.len()),
            SlSessionEvent::FriendRightsChanged {
                friend_id,
                granted_to_us,
                ..
            } => info!(
                "friend rights changed for {friend_id} ({})",
                if *granted_to_us {
                    "they->us"
                } else {
                    "us->them"
                }
            ),
            SlSessionEvent::ActiveGroupChanged(active) => info!(
                "active group: {} (title {:?})",
                active.group_name, active.group_title
            ),
            SlSessionEvent::GroupMemberships(groups) => {
                info!("member of {} group(s)", groups.len());
            }
            SlSessionEvent::GroupSessionMessage {
                from_name, message, ..
            } => info!("group chat from {from_name}: {message}"),
            SlSessionEvent::ScriptDialog(dialog) => info!(
                "script dialog from {:?}: {:?} [{}]",
                dialog.object_name,
                dialog.message,
                dialog.buttons.join(", ")
            ),
            SlSessionEvent::ScriptPermissionRequest(request) => info!(
                "permission request from {:?} (0x{:x})",
                request.object_name, request.permissions.0
            ),
            SlSessionEvent::LoadUrl(load) => {
                info!("load-url from {:?}: {}", load.object_name, load.url);
            }
            SlSessionEvent::ScriptTeleport(request) => {
                info!("script teleport to {:?}", request.region_name);
            }
            // This demo ignores the remaining profile/region/parcel/teleport/group events.
            SlSessionEvent::GroupMembers { .. }
            | SlSessionEvent::GroupRoleData { .. }
            | SlSessionEvent::GroupRoleMembers { .. }
            | SlSessionEvent::GroupTitles { .. }
            | SlSessionEvent::GroupProfileReceived(_)
            | SlSessionEvent::GroupNotices { .. }
            | SlSessionEvent::GroupSessionParticipant { .. }
            | SlSessionEvent::CreateGroupResult { .. }
            | SlSessionEvent::JoinGroupResult { .. }
            | SlSessionEvent::LeaveGroupResult { .. }
            | SlSessionEvent::DroppedFromGroup { .. }
            | SlSessionEvent::AvatarInterests(_)
            | SlSessionEvent::AvatarGroups { .. }
            | SlSessionEvent::AvatarNotes { .. }
            | SlSessionEvent::RegionInfoHandshake(_)
            | SlSessionEvent::RegionLimits(_)
            | SlSessionEvent::ParcelProperties(_)
            | SlSessionEvent::ParcelOverlay(_)
            | SlSessionEvent::NeighborDiscovered(_)
            | SlSessionEvent::MapBlock(_)
            | SlSessionEvent::TeleportStarted
            | SlSessionEvent::TeleportProgress { .. }
            | SlSessionEvent::TeleportLocal
            | SlSessionEvent::TeleportFailed { .. }
            | SlSessionEvent::RegionChanged { .. } => {}
        }
    }
}

/// Requests a clean logout once the hold period has elapsed.
fn maybe_logout(mut hold: ResMut<HoldState>, mut commands: EventWriter<SlCommand>) {
    if hold.requested {
        return;
    }
    if let Some(deadline) = hold.logout_at
        && Instant::now() >= deadline
    {
        info!("hold elapsed; requesting logout");
        commands.write(SlCommand::Logout);
        hold.requested = true;
    }
}
