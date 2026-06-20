//! Logs in to a Second Life / OpenSim grid, holds the session alive past the
//! server inactivity timeout, then logs out cleanly.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-example`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `90`)

use std::time::Duration;

use sl_client_tokio::{
    Camera, Client, Command, DisconnectReason, Error, Event, LoginParams, LoginRequest, Throttle,
    Uuid, Vector, avatar_texture, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Prompts on the terminal for a multi-factor one-time code.
async fn prompt_mfa_code() -> Result<String, Box<dyn std::error::Error>> {
    info!("enter your multi-factor one-time code:");
    let line = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok::<String, std::io::Error>(input.trim().to_owned())
    })
    .await??;
    Ok(line)
}

/// Connects, performing the interactive MFA retry if the grid challenges.
async fn connect_with_mfa(
    login_uri: &str,
    mut request: LoginRequest,
) -> Result<Client, Box<dyn std::error::Error>> {
    loop {
        let params = LoginParams {
            login_uri: login_uri.to_owned(),
            request: request.clone(),
        };
        match Client::connect(params).await {
            Ok(client) => return Ok(client),
            Err(Error::MfaChallenge(challenge)) => {
                info!(
                    "multi-factor authentication required: {}",
                    challenge.message
                );
                let code = prompt_mfa_code().await?;
                request = request.with_mfa(code, challenge.mfa_hash);
            }
            Err(other) => return Err(other.into()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last");
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-example");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "90").parse()?;

    info!("logging in...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = connect_with_mfa(&login_uri, request).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(64);
    let (command_tx, command_rx) = mpsc::channel::<Command>(8);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::CircuitEstablished { sim } => info!("circuit established to {sim}"),
            Event::RegionHandshakeComplete => {
                info!("region handshake complete; holding for {hold_secs}s");
                // Advertise a bandwidth throttle so the simulator opens up the
                // bulk object/terrain/texture streams (re-sent on region change).
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                // Point the camera at a real viewpoint (looking from above the
                // region centre toward the north-east ground) so the simulator's
                // interest list follows where the agent looks rather than the
                // region origin. Re-sent on every keep-alive and region change.
                let camera = Camera::looking_at(
                    Vector {
                        x: 128.0,
                        y: 128.0,
                        z: 40.0,
                    },
                    Vector {
                        x: 160.0,
                        y: 160.0,
                        z: 20.0,
                    },
                );
                info!(
                    "setting camera: at={:?} left={:?} up={:?}",
                    camera.at_axis, camera.left_axis, camera.up_axis
                );
                command_tx.send(Command::SetCamera(camera)).await.ok();
                // Ask the simulator for the agent's current outfit; the reply
                // arrives as an `Event::AgentWearables`.
                command_tx.send(Command::RequestWearables).await.ok();
                // Play a built-in animation (ANIM_AGENT_CLAP,
                // 9b0c1c4e-8ac7-7969-1494-28c874c4f668); the simulator echoes
                // the agent's own animation set back as an
                // `Event::AvatarAnimation` for this avatar.
                command_tx
                    .send(Command::PlayAnimation(Uuid::from_u128(
                        0x9b0c_1c4e_8ac7_7969_1494_28c8_74c4_f668,
                    )))
                    .await
                    .ok();
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    info!("hold elapsed; requesting logout");
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::LoggedOut => {
                info!("logged out cleanly");
                break;
            }
            Event::Disconnected(reason) => {
                match reason {
                    DisconnectReason::Timeout => warn!("disconnected: inactivity timeout"),
                    other => warn!("disconnected: {other:?}"),
                }
                break;
            }
            Event::ChatReceived(chat) => {
                info!(
                    "chat from {} ({:?}): {}",
                    chat.from_name, chat.chat_type, chat.message
                );
            }
            Event::ChatTyping {
                from_name, typing, ..
            } => {
                info!(
                    "{from_name} {} typing",
                    if typing { "started" } else { "stopped" }
                );
            }
            Event::InstantMessageReceived(im) => {
                info!(
                    "IM from {} ({:?}): {}",
                    im.from_agent_name, im.dialog, im.message
                );
            }
            Event::ImTyping {
                from_agent_name,
                typing,
                ..
            } => {
                info!(
                    "{from_agent_name} {} typing (IM)",
                    if typing { "started" } else { "stopped" }
                );
            }
            Event::SitResult {
                sit_object,
                sit_position,
                sit_rotation,
                force_mouselook,
                ..
            } => {
                info!(
                    "sat on {sit_object} at {sit_position:?} facing {sit_rotation:?} \
                     (force_mouselook = {force_mouselook})"
                );
            }
            Event::AvatarProperties(props) => {
                info!(
                    "avatar {} born {}: {}",
                    props.avatar_id, props.born_on, props.about_text
                );
            }
            Event::AvatarPicks { picks, .. } => info!("avatar has {} picks", picks.len()),
            Event::AvatarClassifieds { classifieds, .. } => {
                info!("avatar has {} classifieds", classifieds.len());
            }
            Event::PickInfo(pick) => info!("pick details: {}", pick.name),
            Event::ClassifiedInfo(classified) => {
                info!("classified details: {}", classified.name);
            }
            Event::Account(account) => {
                info!(
                    "account: access {:?}/{:?}, max groups {:?}, home {:?}",
                    account.agent_access,
                    account.agent_access_max,
                    account.max_agent_groups,
                    account.home
                );
            }
            Event::InventorySkeleton(folders) => {
                info!("inventory skeleton: {} folders", folders.len());
            }
            Event::LibraryInventory(folders) => {
                info!("library skeleton: {} folders", folders.len());
            }
            Event::InventoryDescendents { folders, items, .. } => {
                info!(
                    "folder contents: {} sub-folders, {} items",
                    folders.len(),
                    items.len()
                );
            }
            Event::InventoryItemCreated { item, .. } => {
                info!("inventory item created: {}", item.name);
            }
            Event::InventoryBulkUpdate { folders, items, .. } => {
                info!(
                    "bulk inventory update: {} folder(s), {} item(s)",
                    folders.len(),
                    items.len()
                );
            }
            Event::FriendList(friends) => {
                info!("friend list: {} friend(s)", friends.len());
            }
            Event::FriendsOnline(ids) => info!("{} friend(s) came online", ids.len()),
            Event::FriendsOffline(ids) => info!("{} friend(s) went offline", ids.len()),
            Event::FriendRightsChanged {
                friend_id,
                granted_to_us,
                ..
            } => info!(
                "friend rights changed for {friend_id} ({})",
                if granted_to_us {
                    "they->us"
                } else {
                    "us->them"
                }
            ),
            Event::ActiveGroupChanged(active) => info!(
                "active group: {} (title {:?})",
                active.group_name, active.group_title
            ),
            Event::GroupMemberships(groups) => {
                info!("member of {} group(s)", groups.len());
            }
            Event::GroupSessionMessage {
                from_name, message, ..
            } => info!("group chat from {from_name}: {message}"),
            Event::ConferenceSessionMessage {
                from_name, message, ..
            } => info!("conference chat from {from_name}: {message}"),
            Event::ConferenceSessionParticipant {
                agent_id, joined, ..
            } => info!(
                "conference participant {agent_id} {}",
                if joined { "joined" } else { "left" }
            ),
            Event::ConferenceInvited {
                from_name,
                message,
                session_name,
                dialog,
                from_group,
                ..
            } => info!(
                "conference invitation to {session_name:?} ({dialog:?}, from_group={from_group}) from {from_name}: {message}"
            ),
            Event::ScriptDialog(dialog) => info!(
                "script dialog from {:?}: {:?} [{}]",
                dialog.object_name,
                dialog.message,
                dialog.buttons.join(", ")
            ),
            Event::ScriptPermissionRequest(request) => info!(
                "permission request from {:?} (0x{:x})",
                request.object_name, request.permissions.0
            ),
            Event::LoadUrl(load) => info!("load-url from {:?}: {}", load.object_name, load.url),
            Event::ScriptTeleport(request) => {
                info!("script teleport to {:?}", request.region_name);
            }
            Event::MuteList(entries) => info!("mute list: {} entr(ies)", entries.len()),
            Event::MuteListUnchanged => info!("mute list unchanged (cached)"),
            Event::ObjectAdded(object) => {
                let kind = if object.pcode == pcode::AVATAR {
                    "avatar"
                } else {
                    "object"
                };
                let text = if object.text.is_empty() {
                    String::new()
                } else {
                    format!(" text={:?}", object.text)
                };
                info!(
                    "{kind} {} (pcode {}, parent {}) in region {:#x} at {:?}{text}",
                    object.local_id,
                    object.pcode,
                    object.parent_id,
                    object.region_handle,
                    object.motion.position,
                );
            }
            Event::ObjectProperties(props) => {
                info!(
                    "object properties: {:?} — {:?}",
                    props.name, props.description
                );
            }
            Event::ObjectRemoved { local_id, .. } => info!("object {local_id} removed"),
            Event::AvatarAppearance(appearance) => {
                let baked: Vec<&str> = avatar_texture::BAKED
                    .iter()
                    .filter(|(index, _)| {
                        appearance
                            .texture_entry
                            .texture_id(*index)
                            .is_some_and(|id| !id.is_nil())
                    })
                    .map(|(_, name)| *name)
                    .collect();
                info!(
                    "avatar appearance: {} — {} visual params, baked slots {:?}",
                    appearance.avatar_id,
                    appearance.visual_params.len(),
                    baked,
                );
            }
            Event::AgentWearables { serial, wearables } => {
                info!(
                    "own wearables (serial {serial}): {} worn — {:?}",
                    wearables.len(),
                    wearables
                        .iter()
                        .map(|w| w.wearable_type)
                        .collect::<Vec<_>>(),
                );
            }
            Event::AvatarAnimation {
                avatar_id,
                animations,
                physical_events,
            } => {
                info!(
                    "avatar {avatar_id} playing {} animation(s): {:?} ({} physical event block(s))",
                    animations.len(),
                    animations.iter().map(|a| a.anim_id).collect::<Vec<_>>(),
                    physical_events.len(),
                );
            }
            Event::CoarseLocationUpdate { locations, .. } => {
                info!(
                    "coarse-location update: {} nearby avatar(s)",
                    locations.len()
                );
            }
            Event::ViewerEffect(effects) => {
                info!("{} viewer effect(s)", effects.len());
            }
            Event::FindAgentReply {
                prey, locations, ..
            } => {
                info!(
                    "find-agent reply for {prey}: {} location(s)",
                    locations.len()
                );
            }
            Event::SoundTrigger {
                sound_id,
                object_id,
                gain,
                position,
                ..
            } => {
                info!(
                    "sound trigger {sound_id} from object {object_id} at {position:?} gain {gain}"
                );
            }
            Event::AttachedSound {
                sound_id,
                object_id,
                gain,
                flags,
                ..
            } => {
                info!(
                    "attached sound {sound_id} on object {object_id} gain {gain} \
                     (loop={}, stop={})",
                    flags.is_loop(),
                    flags.is_stop(),
                );
            }
            Event::AttachedSoundGainChange { object_id, gain } => {
                info!("attached-sound gain change on object {object_id}: {gain}");
            }
            Event::PreloadSound { sounds } => {
                info!("preload {} sound(s): {:?}", sounds.len(), sounds);
            }
            Event::ParcelMediaCommand {
                command,
                flags,
                time,
            } => {
                info!("parcel media command {command:?} (flags {flags:#x}, time {time})");
            }
            Event::ParcelMediaUpdate(update) => {
                info!(
                    "parcel media update: url {:?} type {:?} {}x{} loop={}",
                    update.media_url,
                    update.media_type,
                    update.media_width,
                    update.media_height,
                    update.media_loop,
                );
            }
            Event::ObjectMedia {
                object_id,
                version,
                faces,
            } => {
                let with_media = faces.iter().filter(|face| face.is_some()).count();
                info!(
                    "object media for {object_id} (version {version}): {} face(s), {with_media} with media",
                    faces.len(),
                );
            }
            Event::GltfMaterialOverride {
                region_handle,
                local_id,
                faces,
                ..
            } => {
                info!(
                    "GLTF material override on object {local_id} in region {region_handle:#x}: {} face(s)",
                    faces.len(),
                );
            }
            Event::RenderMaterials(materials) => {
                info!(
                    "received {} legacy material(s) over RenderMaterials",
                    materials.len(),
                );
            }
            Event::MaterialParamsResult { success, message } => {
                info!("ModifyMaterialParams result: success={success} message={message:?}");
            }
            // This demo ignores motion-only churn and the remaining
            // profile/region/parcel/teleport/group/appearance events.
            Event::ObjectUpdated(_)
            | Event::ServerAppearanceUpdate { .. }
            | Event::CachedTextureResponse { .. }
            | Event::GroupMembers { .. }
            | Event::GroupRoleData { .. }
            | Event::GroupRoleMembers { .. }
            | Event::GroupTitles { .. }
            | Event::GroupProfileReceived(_)
            | Event::GroupNotices { .. }
            | Event::GroupSessionParticipant { .. }
            | Event::CreateGroupResult { .. }
            | Event::JoinGroupResult { .. }
            | Event::LeaveGroupResult { .. }
            | Event::DroppedFromGroup { .. }
            | Event::EjectGroupMemberResult { .. }
            | Event::AvatarInterests(_)
            | Event::AvatarGroups { .. }
            | Event::AvatarNotes { .. }
            | Event::RegionInfoHandshake(_)
            | Event::RegionLimits(_)
            | Event::AvatarNames(_)
            | Event::GroupNames(_)
            | Event::DisplayNames(_)
            | Event::DirPeopleReply { .. }
            | Event::DirGroupsReply { .. }
            | Event::DirEventsReply { .. }
            | Event::DirClassifiedReply { .. }
            | Event::DirPlacesReply { .. }
            | Event::DirLandReply { .. }
            | Event::AvatarPickerReply { .. }
            | Event::PlacesReply { .. }
            | Event::EventInfoReply { .. }
            | Event::ObjectPropertiesFamily { .. }
            | Event::PayPriceReply { .. }
            | Event::Environment(_)
            | Event::MoneyBalance(_)
            | Event::EconomyData(_)
            | Event::ParcelProperties(_)
            | Event::ParcelOverlay(_)
            | Event::ParcelDwell { .. }
            | Event::ParcelAccessList { .. }
            | Event::EstateInfo(_)
            | Event::EstateAccessList { .. }
            | Event::NeighborDiscovered(_)
            | Event::NeighborSeed { .. }
            | Event::MapBlock(_)
            | Event::MapItems { .. }
            | Event::TeleportStarted
            | Event::TeleportProgress { .. }
            | Event::TeleportFinished { .. }
            | Event::TeleportLocal
            | Event::TeleportFailed { .. }
            | Event::TimeDilation { .. }
            | Event::TerrainPatch(_)
            | Event::TextureReceived(_)
            | Event::TextureNotFound(_)
            | Event::AssetReceived(_)
            | Event::AssetTransferStarted { .. }
            | Event::AssetTransferFailed { .. }
            | Event::AssetUploadComplete { .. }
            | Event::AssetUploaded { .. }
            | Event::AssetUploadFailed { .. }
            | Event::VoiceAccountProvisioned(_)
            | Event::ParcelVoiceInfo(_)
            | Event::RegionChanged { .. }
            | Event::ExperienceInfo(_)
            | Event::ExperienceSearchResults(_)
            | Event::ExperiencePermissions { .. }
            | Event::OwnedExperiences(_)
            | Event::AdminExperiences(_)
            | Event::CreatorExperiences(_)
            | Event::GroupExperiences { .. }
            | Event::ExperienceAdminStatus { .. }
            | Event::ExperienceContributorStatus { .. }
            | Event::ExperienceUpdated(_)
            | Event::RegionExperiences { .. } => {}
        }
    }

    run.await??;
    Ok(())
}
