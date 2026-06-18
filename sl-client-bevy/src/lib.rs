#![doc = include_str!("../README.md")]

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

use bevy::prelude::*;
use reqwest::blocking::Client as ReqwestBlockingClient;

use std::collections::HashMap;

use sl_proto::{
    AssetUploadResponse, CAP_AGENT_EXPERIENCES, CAP_CREATE_INVENTORY_CATEGORY,
    CAP_EXPERIENCE_PREFERENCES, CAP_FETCH_INVENTORY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_ASSET, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_EXPERIENCE_INFO,
    CAP_GET_EXPERIENCES, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES,
    CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN,
    CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO,
    CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES,
    CAP_RENDER_MATERIALS, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE,
    CAP_UPLOAD_BAKED_TEXTURE, CAP_VOICE_SIGNALING, Event as SessionEvent, Llsd, LoginResponse,
    RECV_BUFFER_SIZE, REQUESTED_CAPABILITIES, Session, ais_category_children_fetch_url,
    ais_category_children_url, ais_category_url, ais_create_category_url, ais_item_url,
    build_ais_create_category_body, build_ais_move_body, build_ais_rename_category_body,
    build_ais_update_item_body, build_create_inventory_category_request, build_event_queue_request,
    build_fetch_inventory_request, build_group_member_data_request,
    build_modify_material_params_request, build_new_file_agent_inventory_request,
    build_object_media_get_request, build_object_media_navigate_request,
    build_object_media_update_request, build_parcel_voice_info_request,
    build_provision_voice_account_request, build_region_experiences_request,
    build_render_materials_request, build_seed_request, build_set_experience_permission_request,
    build_update_avatar_appearance_request, build_update_experience_request,
    build_update_item_asset_request, build_upload_baked_texture_request,
    build_voice_signaling_request, experience_id_query, experience_info_query,
    find_experience_query, forget_experience_query, group_experiences_query, j2c,
    parse_asset_upload_response, parse_event_queue_response, parse_experience_ids,
    parse_experience_status, parse_llsd_xml, parse_login_response, parse_render_materials_response,
    parse_seed_response,
};

// Re-export the core types a consumer needs to configure the plugin, drive the
// survey commands, and read events. `Event` is aliased to avoid clashing with
// Bevy's `Event` derive.
pub use sl_proto::{
    ActiveGroup, AnyMessage, AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarPick,
    AvatarProperties, Camera, ChatAudible, ChatMessage, ChatSourceType, ChatType, ClassifiedInfo,
    ClassifiedUpdate, ClickAction, Command, ControlFlags, CreateGroupParams, DeRezDestination,
    DisconnectReason, EconomyData, EstateAccessDelta, EstateAccessKind, EstateInfo, ExperienceInfo,
    ExperiencePermission, ExperienceProperties, ExperienceUpdate, ExtendedMesh, FlexibleData,
    Friend, FriendRights, GltfMaterialOverride, GroupMember, GroupMembership, GroupNotice,
    GroupNoticeAttachment, GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit,
    GroupRoleMember, GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, HomeLocation,
    IceCandidate, ImDialog, InstantMessage, InterestsUpdate, InventoryFolder, InventoryItem,
    InventoryOffer, InventoryType, LandingType, LegacyMaterial, LightData, LightImage,
    LindenAmount, LoadUrlRequest, LoginAccount, LoginParams, LoginRequest, MEDIA_PERM_ALL,
    MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MapItem, MapItemType,
    MapRegionInfo, Material, MaterialOverrideUpdate, Maturity, MediaEntry, MfaChallenge,
    MoneyBalance, MoneyTransaction, MoneyTransactionType, MuteEntry, MuteFlags, MuteType,
    NeighborInfo, NewInventoryItem, Object, ObjectExtraParams, ObjectFlagSettings,
    ObjectMediaResponse, ObjectMotion, ObjectProperties, ObjectTransform, ParcelAccessEntry,
    ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelFlags, ParcelInfo,
    ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelOverlayInfo, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate, ParcelVoiceInfo, ParticleSystem, PermissionField,
    PickInfo, PickUpdate, PlayingAnimation, PrimShape, PrimShapeParams, ProductType, ProfileUpdate,
    ReflectionProbe, RegionChatSettings, RegionCombatSettings, RegionFlags, RegionIdentity,
    RegionInfoUpdate, RegionLimits, Reliability, RenderMaterialEntry, RenderMaterialRef, Rotation,
    SaleType, ScriptDialog, ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest,
    SculptData, SoundFlags, SoundPreload, TerrainLayerType, TerrainPatch, TextureAnimation,
    TextureEntry, TextureFace, Throttle, Transmit, Uuid, Vector, VoiceAccountInfo,
    VoiceProvisionRequest, Wearable, WearableType, avatar_texture, decode_particle_system,
    decode_texture_anim, decode_texture_entry, grid_to_handle, group_powers, handle_to_global,
    handle_to_grid, particle_pattern, pcode, sim_access, texture_anim_mode,
};
#[doc(no_inline)]
pub use sl_proto::{Asset, AssetType, ImageCodec, Texture, TransferStatus};
pub use sl_proto::{DisconnectReason as SessionDisconnectReason, Event as SlSessionEvent};

/// How long to wait for a single CAPS event-queue long-poll before retrying.
const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

/// The Bevy plugin that drives a sans-I/O [`Session`] from ECS systems.
#[derive(Debug, Clone)]
pub struct SlClientPlugin {
    /// The login parameters used to start the session.
    pub params: LoginParams,
}

impl Plugin for SlClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SlEvent>()
            .add_event::<SlMfaChallenge>()
            .add_event::<SlCommand>()
            .insert_resource(SlConfig {
                params: self.params.clone(),
            })
            .add_systems(Startup, start_login)
            .add_systems(Update, drive);
    }
}

/// A high-level session event, emitted as a Bevy event.
#[derive(Event, Debug, Clone)]
pub struct SlEvent(pub SessionEvent);

/// Emitted when the grid requires a multi-factor one-time code. To answer it,
/// re-add the plugin with login parameters prepared via
/// `LoginRequest::with_mfa`.
#[derive(Event, Debug, Clone)]
pub struct SlMfaChallenge(pub MfaChallenge);

/// A command to a running session, sent as a Bevy event. Wraps the shared
/// [`Command`] vocabulary (defined in `sl-proto`) so it can be read as a Bevy
/// event; match on its `.0` to dispatch.
#[derive(Event, Debug)]
pub struct SlCommand(pub Command);

/// The plugin configuration resource.
#[derive(Resource, Debug)]
struct SlConfig {
    /// The login parameters.
    params: LoginParams,
}

/// The driver's runtime state resource.
#[derive(Resource)]
struct SlState {
    /// The current phase of the driver.
    inner: SlInner,
}

/// The driver phases.
enum SlInner {
    /// Awaiting the result of the (threaded, blocking) XML-RPC login.
    LoggingIn {
        /// The session whose circuit will be bootstrapped on success.
        session: Box<Session>,
        /// Receives the login response body (or an error string).
        rx: Receiver<Result<String, String>>,
    },
    /// The circuit is up; pumping the socket each frame.
    Running {
        /// The driven session.
        session: Box<Session>,
        /// The non-blocking UDP socket.
        socket: UdpSocket,
        /// A reusable receive buffer.
        recv_buf: Vec<u8>,
        /// The CAPS subsystem for the current region, if a seed capability is
        /// known. Restarted on each region change.
        caps: Option<Caps>,
    },
    /// The session is finished.
    Done,
}

/// The CAPS subsystem for one region: a background thread fetches the capability
/// map (reported over `map_rx`) then long-polls `EventQueueGet`, forwarding each
/// decoded event over `events_rx`. One-shot CAPS fetches (inventory) run on their
/// own threads and report back over the same `events_tx`. Dropping it signals the
/// poller thread to stop after its in-flight request returns.
struct Caps {
    /// Receives decoded event-queue events and CAPS responses (e.g. inventory).
    events_rx: Receiver<(String, Llsd)>,
    /// A sender clone for spawning one-shot CAPS fetches.
    events_tx: Sender<(String, Llsd)>,
    /// Receives fully-formed session events from one-shot binary asset fetches
    /// (the HTTP texture/mesh/asset caps, which return raw bytes rather than
    /// LLSD), to be surfaced directly as [`SlEvent`]s.
    asset_rx: Receiver<SessionEvent>,
    /// A sender clone for spawning one-shot binary asset fetches.
    asset_tx: Sender<SessionEvent>,
    /// Receives the region's capability map once the poller has fetched it.
    map_rx: Receiver<HashMap<String, String>>,
    /// The cached capability map (cap name → URL), empty until discovered.
    map: HashMap<String, String>,
    /// Set on drop to ask the poller thread to stop at its next loop iteration.
    stop: Arc<AtomicBool>,
}

impl Drop for Caps {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Startup system: builds the session and spawns the blocking login thread.
fn start_login(mut commands: Commands, config: Res<SlConfig>) {
    let session = Session::new(config.params.clone());
    let inner = match session.login_http_request() {
        Some(request) => {
            let (tx, rx) = unbounded();
            std::thread::spawn(move || {
                tx.send(perform_login(
                    &request.url,
                    &request.user_agent,
                    request.body,
                ))
                .ok();
            });
            SlInner::LoggingIn {
                session: Box::new(session),
                rx,
            }
        }
        None => SlInner::Done,
    };
    commands.insert_resource(SlState { inner });
}

/// Performs the blocking XML-RPC login POST, returning the response body.
fn perform_login(url: &str, user_agent: &str, body: String) -> Result<String, String> {
    ReqwestBlockingClient::new()
        .post(url)
        .header("Content-Type", "text/xml")
        .header("User-Agent", user_agent)
        .body(body)
        .send()
        .and_then(reqwest::blocking::Response::text)
        .map_err(|error| error.to_string())
}

/// Update system: advances the session each frame.
fn drive(
    mut state: ResMut<SlState>,
    mut events: EventWriter<SlEvent>,
    mut mfa: EventWriter<SlMfaChallenge>,
    mut commands: EventReader<SlCommand>,
) {
    let now = Instant::now();
    let inner = std::mem::replace(&mut state.inner, SlInner::Done);
    state.inner = match inner {
        SlInner::LoggingIn { session, rx } => {
            advance_login(session, rx, now, &mut events, &mut mfa)
        }
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
        } => advance_running(
            session,
            socket,
            recv_buf,
            caps,
            now,
            &mut events,
            &mut commands,
        ),
        SlInner::Done => SlInner::Done,
    };
}

/// Handles the logging-in phase, transitioning to `Running` once the login
/// response arrives.
fn advance_login(
    mut session: Box<Session>,
    rx: Receiver<Result<String, String>>,
    now: Instant,
    events: &mut EventWriter<SlEvent>,
    mfa: &mut EventWriter<SlMfaChallenge>,
) -> SlInner {
    match rx.try_recv() {
        Ok(Ok(body)) => match parse_login_response(&body) {
            Ok(LoginResponse::Success(success)) => {
                if session
                    .handle_login_response(LoginResponse::Success(success), now)
                    .is_err()
                {
                    emit_disconnect(events, DisconnectReason::ProtocolError);
                    return SlInner::Done;
                }
                match bind_socket() {
                    Ok(socket) => {
                        let caps = start_caps(&session);
                        SlInner::Running {
                            session,
                            socket,
                            recv_buf: vec![0u8; RECV_BUFFER_SIZE],
                            caps,
                        }
                    }
                    Err(()) => {
                        emit_disconnect(events, DisconnectReason::ProtocolError);
                        SlInner::Done
                    }
                }
            }
            Ok(LoginResponse::MfaChallenge(challenge)) => {
                mfa.write(SlMfaChallenge(challenge));
                SlInner::Done
            }
            Ok(LoginResponse::Failure(failure)) => {
                emit_disconnect(
                    events,
                    DisconnectReason::LoginFailed {
                        reason: failure.reason,
                        message: failure.message,
                    },
                );
                SlInner::Done
            }
            Err(_parse) => {
                emit_disconnect(events, DisconnectReason::ProtocolError);
                SlInner::Done
            }
        },
        Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
            emit_disconnect(events, DisconnectReason::ProtocolError);
            SlInner::Done
        }
        Err(TryRecvError::Empty) => SlInner::LoggingIn { session, rx },
    }
}

/// Binds a non-blocking UDP socket on an ephemeral port.
fn bind_socket() -> Result<UdpSocket, ()> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_ignored| ())?;
    socket.set_nonblocking(true).map_err(|_ignored| ())?;
    Ok(socket)
}

/// Handles the running phase: receive UDP and CAPS events, apply commands, time
/// out, transmit, and surface events.
fn advance_running(
    mut session: Box<Session>,
    socket: UdpSocket,
    mut recv_buf: Vec<u8>,
    mut caps: Option<Caps>,
    now: Instant,
    events: &mut EventWriter<SlEvent>,
    commands: &mut EventReader<SlCommand>,
) -> SlInner {
    // Drain all available inbound datagrams.
    loop {
        match socket.recv_from(&mut recv_buf) {
            Ok((len, from)) => {
                if let Some(datagram) = recv_buf.get(..len) {
                    session.handle_datagram(from, datagram, now).ok();
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(_other) => break,
        }
    }

    // Cache the capability map once the poller discovers it, then drain any CAPS
    // payloads (event-queue events plus inventory responses).
    if let Some(caps) = caps.as_mut() {
        while let Ok(map) = caps.map_rx.try_recv() {
            caps.map = map;
        }
        while let Ok((message, body)) = caps.events_rx.try_recv() {
            session.handle_caps_event(&message, &body, now).ok();
        }
        // Binary asset fetches return fully-formed session events; surface them.
        while let Ok(event) = caps.asset_rx.try_recv() {
            events.write(SlEvent(event));
        }
    }

    // Apply queued commands.
    for command in commands.read() {
        match &command.0 {
            Command::Send {
                message,
                reliability,
            } => {
                session.enqueue((**message).clone(), *reliability, now).ok();
            }
            Command::Chat {
                message,
                chat_type,
                channel,
            } => {
                session.say(message, *chat_type, *channel, now).ok();
            }
            Command::Typing(typing) => {
                session.set_typing(*typing, now).ok();
            }
            Command::InstantMessage {
                to_agent_id,
                message,
            } => {
                session
                    .send_instant_message(*to_agent_id, message, now)
                    .ok();
            }
            Command::ImTyping {
                to_agent_id,
                typing,
            } => {
                session.send_im_typing(*to_agent_id, *typing, now).ok();
            }
            Command::SetControls(controls) => {
                session.set_controls(*controls, now).ok();
            }
            Command::SetThrottle(throttle) => {
                session.set_throttle(*throttle, now).ok();
            }
            Command::SetRotation { body, head } => {
                session.set_rotation(body.clone(), head.clone(), now).ok();
            }
            Command::SetCamera(camera) => {
                session.set_camera(camera.clone(), now).ok();
            }
            Command::Stand => {
                session.stand(now).ok();
            }
            Command::SitOnGround => {
                session.sit_on_ground(now).ok();
            }
            Command::Sit { target, offset } => {
                session.sit_on(*target, offset.clone(), now).ok();
            }
            Command::Autopilot {
                global_x,
                global_y,
                z,
            } => {
                session.autopilot_to(*global_x, *global_y, *z, now).ok();
            }
            Command::RequestAvatarProperties(target) => {
                session.request_avatar_properties(*target, now).ok();
            }
            Command::RequestAvatarPicks(target) => {
                session.request_avatar_picks(*target, now).ok();
            }
            Command::RequestAvatarNotes(target) => {
                session.request_avatar_notes(*target, now).ok();
            }
            Command::RequestAvatarClassifieds(target) => {
                session.request_avatar_classifieds(*target, now).ok();
            }
            Command::RequestPickInfo {
                creator_id,
                pick_id,
            } => {
                session.request_pick_info(*creator_id, *pick_id, now).ok();
            }
            Command::RequestClassifiedInfo(classified_id) => {
                session.request_classified_info(*classified_id, now).ok();
            }
            Command::UpdateProfile(update) => {
                session.update_profile(update, now).ok();
            }
            Command::UpdateInterests(update) => {
                session.update_interests(update, now).ok();
            }
            Command::UpdateAvatarNotes { target_id, notes } => {
                session.update_avatar_notes(*target_id, notes, now).ok();
            }
            Command::UpdatePick(update) => {
                session.update_pick(update, now).ok();
            }
            Command::DeletePick(pick_id) => {
                session.delete_pick(*pick_id, now).ok();
            }
            Command::GodDeletePick { pick_id, query_id } => {
                session.god_delete_pick(*pick_id, *query_id, now).ok();
            }
            Command::UpdateClassified(update) => {
                session.update_classified(update, now).ok();
            }
            Command::DeleteClassified(classified_id) => {
                session.delete_classified(*classified_id, now).ok();
            }
            Command::GodDeleteClassified {
                classified_id,
                query_id,
            } => {
                session
                    .god_delete_classified(*classified_id, *query_id, now)
                    .ok();
            }
            Command::RequestFolderContents(folder_id) => {
                session.request_folder_contents(*folder_id, now).ok();
            }
            Command::FetchInventoryFolders(folder_ids) => {
                if let Some(caps) = caps.as_ref()
                    && let (Some(url), Some(owner)) = (
                        caps.map.get(CAP_FETCH_INVENTORY).cloned(),
                        session.agent_id(),
                    )
                {
                    let events_tx = caps.events_tx.clone();
                    let folders = folder_ids.clone();
                    std::thread::spawn(move || {
                        run_inventory_fetch(&url, owner, &folders, &events_tx);
                    });
                }
            }
            Command::CreateInventoryFolder {
                folder_id,
                parent_id,
                folder_type,
                name,
            } => {
                session
                    .create_inventory_folder(*folder_id, *parent_id, *folder_type, name, now)
                    .ok();
            }
            Command::UpdateInventoryFolder {
                folder_id,
                parent_id,
                folder_type,
                name,
            } => {
                session
                    .update_inventory_folder(*folder_id, *parent_id, *folder_type, name, now)
                    .ok();
            }
            Command::MoveInventoryFolder {
                folder_id,
                parent_id,
            } => {
                session
                    .move_inventory_folder(*folder_id, *parent_id, now)
                    .ok();
            }
            Command::RemoveInventoryFolders(folder_ids) => {
                session.remove_inventory_folders(folder_ids, now).ok();
            }
            Command::CreateInventoryItem(new) => {
                session.create_inventory_item(new, now).ok();
            }
            Command::UpdateInventoryItem {
                item,
                transaction_id,
            } => {
                session
                    .update_inventory_item(item, *transaction_id, now)
                    .ok();
            }
            Command::MoveInventoryItem {
                item_id,
                folder_id,
                new_name,
            } => {
                session
                    .move_inventory_item(*item_id, *folder_id, new_name, now)
                    .ok();
            }
            Command::CopyInventoryItem {
                old_agent_id,
                old_item_id,
                new_folder_id,
                new_name,
            } => {
                session
                    .copy_inventory_item(*old_agent_id, *old_item_id, *new_folder_id, new_name, now)
                    .ok();
            }
            Command::RemoveInventoryItems(item_ids) => {
                session.remove_inventory_items(item_ids, now).ok();
            }
            Command::ChangeInventoryItemFlags { item_id, flags } => {
                session
                    .change_inventory_item_flags(*item_id, *flags, now)
                    .ok();
            }
            Command::PurgeInventoryDescendents(folder_id) => {
                session.purge_inventory_descendents(*folder_id, now).ok();
            }
            Command::RemoveInventoryObjects {
                folder_ids,
                item_ids,
            } => {
                session
                    .remove_inventory_objects(folder_ids, item_ids, now)
                    .ok();
            }
            Command::CreateInventoryCategory {
                parent_id,
                folder_type,
                name,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CREATE_INVENTORY_CATEGORY).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let body = build_create_inventory_category_request(
                        Uuid::new_v4(),
                        *parent_id,
                        *folder_type,
                        name,
                    );
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_CREATE_INVENTORY_CATEGORY, &events_tx);
                    });
                }
            }
            Command::Ais3CreateFolder {
                parent_id,
                folder_type,
                name,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!(
                        "{base}{}",
                        ais_create_category_url(*parent_id, Uuid::new_v4())
                    );
                    let body = build_ais_create_category_body(*folder_type, name);
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3RenameFolder { folder_id, name } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_url(*folder_id));
                    let body = build_ais_rename_category_body(name);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3MoveFolder {
                folder_id,
                parent_id,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_url(*folder_id));
                    let body = build_ais_move_body(*parent_id);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3RemoveFolder(folder_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_url(*folder_id));
                    std::thread::spawn(move || {
                        run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3PurgeFolder(folder_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_children_url(*folder_id));
                    std::thread::spawn(move || {
                        run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3FetchFolderChildren { folder_id, depth } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!(
                        "{base}{}",
                        ais_category_children_fetch_url(*folder_id, *depth)
                    );
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3UpdateItem {
                item_id,
                name,
                description,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    let body = build_ais_update_item_body(name, description);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3MoveItem { item_id, parent_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    let body = build_ais_move_body(*parent_id);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3RemoveItem(item_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    std::thread::spawn(move || {
                        run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3FetchItem(item_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::FetchGroupMembers(group_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GROUP_MEMBER_DATA).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let group = *group_id;
                    std::thread::spawn(move || {
                        run_group_members_fetch(&url, group, &events_tx);
                    });
                }
            }
            Command::OfferFriendship {
                to_agent_id,
                message,
            } => {
                session
                    .send_friendship_offer(*to_agent_id, message, now)
                    .ok();
            }
            Command::GrantUserRights { target, rights } => {
                session.grant_user_rights(*target, *rights, now).ok();
            }
            Command::TerminateFriendship(other) => {
                session.terminate_friendship(*other, now).ok();
            }
            Command::AcceptFriendship {
                transaction_id,
                calling_card_folder,
            } => {
                session
                    .accept_friendship(*transaction_id, *calling_card_folder, now)
                    .ok();
            }
            Command::DeclineFriendship(transaction_id) => {
                session.decline_friendship(*transaction_id, now).ok();
            }
            Command::ActivateGroup(group_id) => {
                session.activate_group(*group_id, now).ok();
            }
            Command::RequestGroupMembers(group_id) => {
                session.request_group_members(*group_id, now).ok();
            }
            Command::RequestGroupRoles(group_id) => {
                session.request_group_roles(*group_id, now).ok();
            }
            Command::RequestGroupRoleMembers(group_id) => {
                session.request_group_role_members(*group_id, now).ok();
            }
            Command::RequestGroupTitles(group_id) => {
                session.request_group_titles(*group_id, now).ok();
            }
            Command::RequestGroupProfile(group_id) => {
                session.request_group_profile(*group_id, now).ok();
            }
            Command::RequestGroupNotices(group_id) => {
                session.request_group_notices(*group_id, now).ok();
            }
            Command::RequestGroupNotice(notice_id) => {
                session.request_group_notice(*notice_id, now).ok();
            }
            Command::CreateGroup(params) => {
                session.create_group(params, now).ok();
            }
            Command::JoinGroup(group_id) => {
                session.join_group(*group_id, now).ok();
            }
            Command::LeaveGroup(group_id) => {
                session.leave_group(*group_id, now).ok();
            }
            Command::InviteToGroup { group_id, invitees } => {
                session.invite_to_group(*group_id, invitees, now).ok();
            }
            Command::SetGroupAcceptNotices {
                group_id,
                accept_notices,
                list_in_profile,
            } => {
                session
                    .set_group_accept_notices(*group_id, *accept_notices, *list_in_profile, now)
                    .ok();
            }
            Command::SetGroupContribution {
                group_id,
                contribution,
            } => {
                session
                    .set_group_contribution(*group_id, *contribution, now)
                    .ok();
            }
            Command::StartGroupSession(group_id) => {
                session.start_group_session(*group_id, now).ok();
            }
            Command::SendGroupMessage { group_id, message } => {
                session.send_group_message(*group_id, message, now).ok();
            }
            Command::LeaveGroupSession(group_id) => {
                session.leave_group_session(*group_id, now).ok();
            }
            Command::UpdateGroupRoles { group_id, roles } => {
                session.update_group_roles(*group_id, roles, now).ok();
            }
            Command::ChangeGroupRoleMembers { group_id, changes } => {
                session
                    .change_group_role_members(*group_id, changes, now)
                    .ok();
            }
            Command::EjectGroupMembers {
                group_id,
                member_ids,
            } => {
                session.eject_group_members(*group_id, member_ids, now).ok();
            }
            Command::SendGroupNotice {
                group_id,
                subject,
                message,
                attachment,
            } => {
                session
                    .send_group_notice(*group_id, subject, message, *attachment, now)
                    .ok();
            }
            Command::ReplyScriptDialog {
                object_id,
                chat_channel,
                button_index,
                button_label,
            } => {
                session
                    .reply_script_dialog(
                        *object_id,
                        *chat_channel,
                        *button_index,
                        button_label,
                        now,
                    )
                    .ok();
            }
            Command::AnswerScriptPermissions {
                task_id,
                item_id,
                permissions,
            } => {
                session
                    .answer_script_permissions(*task_id, *item_id, *permissions, now)
                    .ok();
            }
            Command::RequestMuteList => {
                session.request_mute_list(now).ok();
            }
            Command::Mute {
                id,
                name,
                mute_type,
                flags,
            } => {
                session.mute(*id, name, *mute_type, *flags, now).ok();
            }
            Command::Unmute { id, name } => {
                session.unmute(*id, name, now).ok();
            }
            Command::Teleport {
                region_handle,
                position,
                look_at,
            } => {
                session
                    .teleport_to(*region_handle, position.clone(), look_at.clone(), now)
                    .ok();
            }
            Command::RequestRegionInfo => {
                session.request_region_info(now).ok();
            }
            Command::RequestMoneyBalance => {
                session.request_money_balance(now).ok();
            }
            Command::RequestEconomyData => {
                session.request_economy_data(now).ok();
            }
            Command::SendMoneyTransfer {
                dest,
                amount,
                kind,
                description,
            } => {
                session
                    .send_money_transfer(*dest, amount.clone(), *kind, description, now)
                    .ok();
            }
            Command::RequestParcelProperties {
                west,
                south,
                east,
                north,
                sequence_id,
            } => {
                session
                    .request_parcel_properties(*west, *south, *east, *north, *sequence_id, now)
                    .ok();
            }
            Command::SetDrawDistance(far) => session.set_draw_distance(*far),
            Command::RequestMapBlocks {
                min_x,
                max_x,
                min_y,
                max_y,
            } => {
                session
                    .request_map_blocks(*min_x, *max_x, *min_y, *max_y, now)
                    .ok();
            }
            Command::RequestMapByName { name } => {
                session.request_map_by_name(name, now).ok();
            }
            Command::RequestMapItems {
                item_type,
                region_handle,
            } => {
                session
                    .request_map_items(*item_type, *region_handle, now)
                    .ok();
            }
            Command::RequestObjects { local_ids } => {
                session.request_objects(local_ids, now).ok();
            }
            Command::RequestObjectProperties { local_ids } => {
                session.request_object_properties(local_ids, now).ok();
            }
            Command::DeselectObjects { local_ids } => {
                session.deselect_objects(local_ids, now).ok();
            }
            Command::TouchObject { local_id } => {
                session.touch_object(*local_id, now).ok();
            }
            Command::GrabObject {
                local_id,
                grab_offset,
            } => {
                session
                    .grab_object(*local_id, grab_offset.clone(), now)
                    .ok();
            }
            Command::GrabObjectUpdate {
                object_id,
                grab_offset_initial,
                grab_position,
                time_since_last,
            } => {
                session
                    .grab_object_update(
                        *object_id,
                        grab_offset_initial.clone(),
                        grab_position.clone(),
                        *time_since_last,
                        now,
                    )
                    .ok();
            }
            Command::DegrabObject { local_id } => {
                session.degrab_object(*local_id, now).ok();
            }
            Command::RezObject { shape, group_id } => {
                session.rez_object(shape, *group_id, now).ok();
            }
            Command::DuplicateObjects {
                local_ids,
                offset,
                group_id,
            } => {
                session
                    .duplicate_objects(local_ids, offset.clone(), *group_id, now)
                    .ok();
            }
            Command::DeleteObjects { local_ids } => {
                session.delete_objects(local_ids, now).ok();
            }
            Command::DerezObjects {
                local_ids,
                destination,
                destination_id,
                transaction_id,
                group_id,
            } => {
                session
                    .derez_objects(
                        local_ids,
                        *destination,
                        *destination_id,
                        *transaction_id,
                        *group_id,
                        now,
                    )
                    .ok();
            }
            Command::UpdateObject {
                local_id,
                transform,
            } => {
                session.update_object(*local_id, transform, now).ok();
            }
            Command::SetObjectName { local_id, name } => {
                session.set_object_name(*local_id, name, now).ok();
            }
            Command::SetObjectDescription {
                local_id,
                description,
            } => {
                session
                    .set_object_description(*local_id, description, now)
                    .ok();
            }
            Command::SetObjectClickAction { local_id, action } => {
                session
                    .set_object_click_action(*local_id, *action, now)
                    .ok();
            }
            Command::SetObjectMaterial { local_id, material } => {
                session.set_object_material(*local_id, *material, now).ok();
            }
            Command::SetObjectFlags { local_id, flags } => {
                session.set_object_flags(*local_id, flags, now).ok();
            }
            Command::SetObjectGroup {
                local_ids,
                group_id,
            } => {
                session.set_object_group(local_ids, *group_id, now).ok();
            }
            Command::SetObjectPermissions {
                local_ids,
                field,
                set,
                mask,
            } => {
                session
                    .set_object_permissions(local_ids, *field, *set, *mask, now)
                    .ok();
            }
            Command::SetObjectForSale {
                local_id,
                sale_type,
                sale_price,
            } => {
                session
                    .set_object_for_sale(*local_id, *sale_type, *sale_price, now)
                    .ok();
            }
            Command::SetObjectCategory { local_id, category } => {
                session.set_object_category(*local_id, *category, now).ok();
            }
            Command::SetObjectIncludeInSearch { local_id, include } => {
                session
                    .set_object_include_in_search(*local_id, *include, now)
                    .ok();
            }
            Command::LinkObjects { local_ids } => {
                session.link_objects(local_ids, now).ok();
            }
            Command::DelinkObjects { local_ids } => {
                session.delink_objects(local_ids, now).ok();
            }
            Command::UpdateParcel(update) => {
                session.update_parcel(update, now).ok();
            }
            Command::RequestParcelAccessList { local_id, scope } => {
                session
                    .request_parcel_access_list(*local_id, *scope, now)
                    .ok();
            }
            Command::UpdateParcelAccessList {
                local_id,
                scope,
                entries,
            } => {
                session
                    .update_parcel_access_list(*local_id, *scope, entries, now)
                    .ok();
            }
            Command::RequestParcelDwell { local_id } => {
                session.request_parcel_dwell(*local_id, now).ok();
            }
            Command::BuyParcel {
                local_id,
                price,
                area,
                group_id,
                is_group_owned,
            } => {
                session
                    .buy_parcel(*local_id, *price, *area, *group_id, *is_group_owned, now)
                    .ok();
            }
            Command::ReturnParcelObjects {
                local_id,
                return_type,
                owner_ids,
                task_ids,
            } => {
                session
                    .return_parcel_objects(*local_id, *return_type, owner_ids, task_ids, now)
                    .ok();
            }
            Command::SelectParcelObjects {
                local_id,
                return_type,
                object_ids,
            } => {
                session
                    .select_parcel_objects(*local_id, *return_type, object_ids, now)
                    .ok();
            }
            Command::DeedParcelToGroup { local_id, group_id } => {
                session.deed_parcel_to_group(*local_id, *group_id, now).ok();
            }
            Command::ReclaimParcel { local_id } => {
                session.reclaim_parcel(*local_id, now).ok();
            }
            Command::ReleaseParcel { local_id } => {
                session.release_parcel(*local_id, now).ok();
            }
            Command::RequestEstateInfo => {
                session.request_estate_info(now).ok();
            }
            Command::UpdateEstateAccess { delta, target } => {
                session.update_estate_access(*delta, *target, now).ok();
            }
            Command::KickEstateUser { target } => {
                session.kick_estate_user(*target, now).ok();
            }
            Command::TeleportHomeUser { target } => {
                session.teleport_home_user(*target, now).ok();
            }
            Command::TeleportHomeAllUsers => {
                session.teleport_home_all_users(now).ok();
            }
            Command::RestartRegion { seconds } => {
                session.restart_region(*seconds, now).ok();
            }
            Command::SendEstateMessage { message } => {
                session.send_estate_message(message, now).ok();
            }
            Command::SetRegionInfo(update) => {
                session.set_region_info(update, now).ok();
            }
            Command::GodKickUser { target, reason } => {
                session.god_kick_user(*target, reason, now).ok();
            }
            Command::SendGodlikeMessage { method, params } => {
                let refs: Vec<&str> = params.iter().map(String::as_str).collect();
                session.send_godlike_message(method, &refs, now).ok();
            }
            Command::RequestTexture {
                texture_id,
                discard_level,
                priority,
            } => {
                session
                    .request_texture(*texture_id, *discard_level, *priority, now)
                    .ok();
            }
            Command::RequestAsset {
                asset_id,
                asset_type,
                priority,
            } => {
                session
                    .request_asset(*asset_id, *asset_type, *priority, now)
                    .ok();
            }
            Command::FetchTexture {
                texture_id,
                discard_level,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_TEXTURE).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, discard) = (*texture_id, *discard_level);
                    std::thread::spawn(move || {
                        run_texture_fetch(&url, id, discard, &asset_tx);
                    });
                }
            }
            Command::FetchMesh {
                mesh_id,
                byte_range,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps
                        .map
                        .get(CAP_GET_MESH2)
                        .or_else(|| caps.map.get(CAP_GET_MESH))
                        .cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, range) = (*mesh_id, *byte_range);
                    std::thread::spawn(move || {
                        run_asset_fetch(
                            &url,
                            &format!("?mesh_id={id}"),
                            id,
                            AssetType::Mesh,
                            range,
                            &asset_tx,
                        );
                    });
                }
            }
            Command::FetchAsset {
                asset_id,
                asset_type,
                byte_range,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_ASSET).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, asset_type, range) = (*asset_id, *asset_type, *byte_range);
                    std::thread::spawn(move || {
                        run_generic_asset_fetch(&url, id, asset_type, range, &asset_tx);
                    });
                }
            }
            Command::RequestWearables => {
                session.request_wearables(now).ok();
            }
            Command::SetWearing(wearables) => {
                session.set_wearing(wearables, now).ok();
            }
            Command::SetAppearance {
                serial,
                size,
                texture_entry,
                visual_params,
                wearable_cache,
            } => {
                session
                    .set_appearance(
                        *serial,
                        size.clone(),
                        texture_entry,
                        visual_params,
                        wearable_cache,
                        now,
                    )
                    .ok();
            }
            Command::RequestCachedTextures { serial, slots } => {
                session.request_cached_textures(*serial, slots, now).ok();
            }
            Command::RequestServerAppearanceUpdate { cof_version } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_UPDATE_AVATAR_APPEARANCE).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let version = *cof_version;
                    std::thread::spawn(move || {
                        run_server_appearance_update(&url, version, &events_tx);
                    });
                }
            }
            Command::SetAnimations(animations) => {
                session.set_animations(animations, now).ok();
            }
            Command::PlayAnimation(anim_id) => {
                session.play_animation(*anim_id, now).ok();
            }
            Command::StopAnimation(anim_id) => {
                session.stop_animation(*anim_id, now).ok();
            }
            Command::UploadAssetUdp {
                asset_type,
                data,
                temp_file,
                store_local,
            } => {
                session
                    .upload_asset_udp(*asset_type, data.clone(), *temp_file, *store_local, now)
                    .ok();
            }
            Command::UploadAsset {
                folder_id,
                asset_type,
                inventory_type,
                name,
                description,
                next_owner_mask,
                group_mask,
                everyone_mask,
                expected_upload_cost,
                data,
            } => {
                spawn_new_file_upload(
                    caps.as_ref(),
                    *folder_id,
                    *asset_type,
                    *inventory_type,
                    name,
                    description,
                    *next_owner_mask,
                    *group_mask,
                    *everyone_mask,
                    *expected_upload_cost,
                    data.clone(),
                );
            }
            Command::UploadBakedTexture { data } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_UPLOAD_BAKED_TEXTURE).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let body = build_upload_baked_texture_request();
                    let data = data.clone();
                    std::thread::spawn(move || {
                        let event = run_caps_upload(&url, body, data);
                        asset_tx.send(event).ok();
                    });
                } else {
                    emit_upload_unavailable(caps.as_ref(), "UploadBakedTexture");
                }
            }
            Command::UpdateInventoryAsset {
                item_id,
                asset_type,
                data,
            } => match asset_type.update_item_cap() {
                Some(cap) => {
                    if let Some(caps) = caps.as_ref()
                        && let Some(url) = caps.map.get(cap).cloned()
                    {
                        let asset_tx = caps.asset_tx.clone();
                        let body = build_update_item_asset_request(*item_id);
                        let data = data.clone();
                        std::thread::spawn(move || {
                            let event = run_caps_upload(&url, body, data);
                            asset_tx.send(event).ok();
                        });
                    } else {
                        emit_upload_unavailable(caps.as_ref(), cap);
                    }
                }
                None => emit_upload_failure(
                    caps.as_ref(),
                    "asset type has no inventory-update capability".to_owned(),
                ),
            },
            Command::RequestObjectMedia { object_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let object = *object_id;
                    std::thread::spawn(move || {
                        run_object_media_fetch(&url, object, &events_tx);
                    });
                }
            }
            Command::SetObjectMedia { object_id, faces } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA).cloned()
                {
                    let body = build_object_media_update_request(*object_id, faces);
                    std::thread::spawn(move || {
                        run_object_media_post(&url, body);
                    });
                }
            }
            Command::NavigateObjectMedia {
                object_id,
                face,
                url: media_url,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA_NAVIGATE).cloned()
                {
                    let body = build_object_media_navigate_request(*object_id, *face, media_url);
                    std::thread::spawn(move || {
                        run_object_media_post(&url, body);
                    });
                }
            }
            Command::RequestRenderMaterials { material_ids } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_RENDER_MATERIALS).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let ids = material_ids.clone();
                    std::thread::spawn(move || {
                        run_render_materials_fetch(&url, ids, &asset_tx);
                    });
                }
            }
            Command::ModifyMaterialParams { updates } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_MODIFY_MATERIAL_PARAMS).cloned()
                {
                    let body = build_modify_material_params_request(updates);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_modify_material_params(&url, body, &events_tx);
                    });
                }
            }
            Command::RequestVoiceAccount { request } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_PROVISION_VOICE_ACCOUNT).cloned()
                {
                    let body = build_provision_voice_account_request(request);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_PROVISION_VOICE_ACCOUNT, &events_tx);
                    });
                }
            }
            Command::RequestParcelVoiceInfo => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_PARCEL_VOICE_INFO).cloned()
                {
                    let body = build_parcel_voice_info_request();
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_PARCEL_VOICE_INFO, &events_tx);
                    });
                }
            }
            Command::SendVoiceSignaling {
                viewer_session,
                candidates,
                completed,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_VOICE_SIGNALING).cloned()
                {
                    let body =
                        build_voice_signaling_request(viewer_session, candidates, *completed);
                    std::thread::spawn(move || {
                        run_voice_signaling(&url, body);
                    });
                }
            }
            Command::RequestExperienceInfo { experience_ids } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_GET_EXPERIENCE_INFO).cloned()
                {
                    let url = format!("{base}{}", experience_info_query(experience_ids));
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_EXPERIENCE_INFO, &events_tx);
                    });
                }
            }
            Command::FindExperiences { query, page } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_FIND_EXPERIENCE_BY_NAME).cloned()
                {
                    let url = format!("{base}{}", find_experience_query(query, *page));
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_FIND_EXPERIENCE_BY_NAME, &events_tx);
                    });
                }
            }
            Command::RequestExperiencePermissions => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::SetExperiencePermission {
                experience_id,
                permission,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_EXPERIENCE_PREFERENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    if permission.is_forget() {
                        let url = format!("{base}{}", forget_experience_query(*experience_id));
                        std::thread::spawn(move || {
                            run_delete_caps_llsd(&url, CAP_EXPERIENCE_PREFERENCES, &events_tx);
                        });
                    } else {
                        let body =
                            build_set_experience_permission_request(*experience_id, *permission);
                        std::thread::spawn(move || {
                            run_put_caps_llsd(&base, body, CAP_EXPERIENCE_PREFERENCES, &events_tx);
                        });
                    }
                }
            }
            Command::RequestOwnedExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_AGENT_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_AGENT_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::RequestAdminExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_ADMIN_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_ADMIN_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::RequestCreatorExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_CREATOR_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_CREATOR_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::RequestGroupExperiences { group_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_GROUP_EXPERIENCES).cloned()
                {
                    let url = format!("{base}{}", group_experiences_query(*group_id));
                    let group_id = *group_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_group_experiences(&url, group_id, &asset_tx);
                    });
                }
            }
            Command::RequestExperienceAdmin { experience_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_IS_EXPERIENCE_ADMIN).cloned()
                {
                    let url = format!("{base}{}", experience_id_query(*experience_id));
                    let experience_id = *experience_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_experience_status(&url, experience_id, true, &asset_tx);
                    });
                }
            }
            Command::RequestExperienceContributor { experience_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_IS_EXPERIENCE_CONTRIBUTOR).cloned()
                {
                    let url = format!("{base}{}", experience_id_query(*experience_id));
                    let experience_id = *experience_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_experience_status(&url, experience_id, false, &asset_tx);
                    });
                }
            }
            Command::UpdateExperience { update } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_UPDATE_EXPERIENCE).cloned()
                {
                    let body = build_update_experience_request(update);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_UPDATE_EXPERIENCE, &events_tx);
                    });
                }
            }
            Command::RequestRegionExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_REGION_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_REGION_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::SetRegionExperiences {
                allowed,
                blocked,
                trusted,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_REGION_EXPERIENCES).cloned()
                {
                    let body = build_region_experiences_request(allowed, blocked, trusted);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_REGION_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::OfferTeleport { targets, message } => {
                session.offer_teleport(targets, message, now).ok();
            }
            Command::AcceptTeleportLure { lure_id } => {
                session.accept_teleport_lure(*lure_id, now).ok();
            }
            Command::DeclineTeleportLure {
                from_agent_id,
                lure_id,
            } => {
                session
                    .decline_teleport_lure(*from_agent_id, *lure_id, now)
                    .ok();
            }
            Command::RequestTeleport {
                to_agent_id,
                message,
            } => {
                session.request_teleport(*to_agent_id, message, now).ok();
            }
            Command::GiveInventory {
                to_agent_id,
                item_id,
                asset_type,
                item_name,
                transaction_id,
            } => {
                session
                    .give_inventory(
                        *to_agent_id,
                        *item_id,
                        *asset_type,
                        item_name,
                        *transaction_id,
                        now,
                    )
                    .ok();
            }
            Command::GiveInventoryFolder {
                to_agent_id,
                folder_id,
                folder_name,
                transaction_id,
            } => {
                session
                    .give_inventory_folder(
                        *to_agent_id,
                        *folder_id,
                        folder_name,
                        *transaction_id,
                        now,
                    )
                    .ok();
            }
            Command::AcceptInventoryOffer { offer, folder_id } => {
                session.accept_inventory_offer(offer, *folder_id, now).ok();
            }
            Command::DeclineInventoryOffer {
                offer,
                trash_folder_id,
            } => {
                session
                    .decline_inventory_offer(offer, *trash_folder_id, now)
                    .ok();
            }
            Command::StartConference {
                session_id,
                invitees,
                message,
            } => {
                session
                    .start_conference(*session_id, invitees, message, now)
                    .ok();
            }
            Command::SendConferenceMessage {
                session_id,
                message,
            } => {
                session
                    .send_conference_message(*session_id, message, now)
                    .ok();
            }
            Command::LeaveConference { session_id } => {
                session.leave_conference(*session_id, now).ok();
            }
            Command::RetrieveInstantMessages => {
                session.retrieve_instant_messages(now).ok();
            }
            Command::RequestOfflineMessages => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_READ_OFFLINE_MSGS).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_READ_OFFLINE_MSGS, &events_tx);
                    });
                }
            }
            Command::Logout => session.initiate_logout(now),
        }
    }

    // Fire timers that are due.
    if session
        .poll_timeout()
        .is_some_and(|deadline| now >= deadline)
    {
        session.handle_timeout(now);
    }

    // Flush outgoing datagrams.
    while let Some(transmit) = session.poll_transmit() {
        socket.send_to(&transmit.payload, transmit.destination).ok();
    }

    // Surface events. A region change brings a new seed capability, so restart
    // the event-queue poller against the new region (dropping the old poller
    // signals its thread to stop).
    let mut done = false;
    let mut region_changed = false;
    while let Some(event) = session.poll_event() {
        match &event {
            SessionEvent::Disconnected(_) | SessionEvent::LoggedOut => done = true,
            SessionEvent::RegionChanged { .. } => region_changed = true,
            // POST a neighbour's seed capability so the simulator streams that
            // region's scene to the child circuit (its `SendInitialData` is gated
            // on the seed having been requested). One-shot, off the ECS thread.
            SessionEvent::NeighborSeed {
                seed_capability, ..
            } => post_neighbour_seed(seed_capability.clone()),
            _ => {}
        }
        events.write(SlEvent(event));
    }
    if region_changed {
        caps = start_caps(&session);
    }

    if done || session.is_closed() {
        SlInner::Done
    } else {
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
        }
    }
}

/// Starts the CAPS subsystem for the session's current seed capability: a
/// background thread that fetches the capability map (reported over `map_rx`)
/// then long-polls `EventQueueGet`. Returns `None` if no seed is known yet.
fn start_caps(session: &Session) -> Option<Caps> {
    let seed = session.seed_capability()?.to_owned();
    let (events_tx, events_rx) = unbounded();
    let (asset_tx, asset_rx) = unbounded();
    let (map_tx, map_rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_events = events_tx.clone();
    std::thread::spawn(move || run_caps(seed, &thread_events, &map_tx, &thread_stop));
    Some(Caps {
        events_rx,
        events_tx,
        asset_rx,
        asset_tx,
        map_rx,
        map: HashMap::new(),
        stop,
    })
}

/// POSTs a neighbour region's seed capability (in a detached thread, result
/// ignored) so the simulator marks the agent's capabilities as sent and begins
/// streaming that region's scene to the child circuit.
fn post_neighbour_seed(seed_url: String) {
    std::thread::spawn(move || {
        let Ok(http) = ReqwestBlockingClient::builder()
            .timeout(EVENT_QUEUE_TIMEOUT)
            .build()
        else {
            return;
        };
        let _ignored = http
            .post(&seed_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_seed_request(REQUESTED_CAPABILITIES))
            .send();
    });
}

/// Fetches the capability map from `seed_url` (reporting it over `map_tx`), then
/// long-polls the `EventQueueGet` capability, forwarding each decoded event to
/// `caps_tx` until `stop` is set, a receiver is dropped (e.g. on region change),
/// or a request fails fatally.
fn run_caps(
    seed_url: String,
    caps_tx: &Sender<(String, Llsd)>,
    map_tx: &Sender<HashMap<String, String>>,
    stop: &AtomicBool,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .post(&seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    let Ok(capabilities) = parse_seed_response(&text) else {
        return;
    };
    map_tx.send(capabilities.clone()).ok();
    let Some(event_queue_url) = capabilities.get("EventQueueGet").cloned() else {
        return;
    };

    let mut ack: Option<i32> = None;
    while !stop.load(Ordering::Relaxed) {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
        {
            Ok(response) => response,
            Err(_error) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let Ok(text) = response.text() else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).is_err() {
                return;
            }
        }
    }
}

/// POSTs a `FetchInventoryDescendents2` request for `folder_ids` and forwards the
/// LLSD response to `caps_tx` tagged [`CAP_FETCH_INVENTORY`], for the session to
/// decode into [`SlSessionEvent::InventoryDescendents`].
fn run_inventory_fetch(
    cap_url: &str,
    owner_id: Uuid,
    folder_ids: &[Uuid],
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_fetch_inventory_request(owner_id, folder_ids);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_FETCH_INVENTORY.to_owned(), llsd)).ok();
    }
}

/// POSTs a `GroupMemberData` request for `group_id` and forwards the LLSD roster
/// response to `caps_tx` tagged [`CAP_GROUP_MEMBER_DATA`], for the session to
/// decode into [`SlSessionEvent::GroupMembers`].
fn run_group_members_fetch(cap_url: &str, group_id: Uuid, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_group_member_data_request(group_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_GROUP_MEMBER_DATA.to_owned(), llsd)).ok();
    }
}

/// POSTs an `UpdateAvatarAppearance` request for `cof_version` (the modern
/// Second Life server-side bake) and forwards the LLSD reply to `caps_tx` tagged
/// [`CAP_UPDATE_AVATAR_APPEARANCE`], for the session to surface as a
/// [`SlSessionEvent::ServerAppearanceUpdate`]. The baked appearance itself
/// arrives separately over UDP as a [`SlSessionEvent::AvatarAppearance`].
fn run_server_appearance_update(cap_url: &str, cof_version: i32, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_update_avatar_appearance_request(cof_version);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd))
            .ok();
    }
}

/// POSTs an `ObjectMedia` GET for `object_id` and forwards the decoded LLSD
/// response to `caps_tx` tagged [`CAP_OBJECT_MEDIA`], for the session to surface
/// as a [`SlSessionEvent::ObjectMedia`].
fn run_object_media_fetch(cap_url: &str, object_id: Uuid, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_object_media_get_request(object_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_OBJECT_MEDIA.to_owned(), llsd)).ok();
    }
}

/// POSTs a pre-built `ObjectMedia` UPDATE or `ObjectMediaNavigate` `body` to
/// `cap_url`. Fire-and-forget: the simulator advances the object's media version
/// rather than replying with media, so a client re-fetches with
/// [`Command::RequestObjectMedia`] to observe the change.
fn run_object_media_post(cap_url: &str, body: String) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    http.post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .ok();
}

/// POSTs a `RenderMaterials` request for `material_ids` (the zipped binary-LLSD
/// form) and forwards the decoded legacy materials to `asset_tx` as a
/// [`SlSessionEvent::RenderMaterials`]. Best-effort: a transport or decode
/// failure yields an empty list.
fn run_render_materials_fetch(
    cap_url: &str,
    material_ids: Vec<Uuid>,
    asset_tx: &Sender<SessionEvent>,
) {
    let materials = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()
        .and_then(|http| {
            let body = build_render_materials_request(&material_ids);
            http.post(cap_url)
                .header("Content-Type", "application/llsd+xml")
                .body(body)
                .send()
                .ok()
        })
        .and_then(|response| response.text().ok())
        .map(|text| parse_render_materials_response(&text))
        .unwrap_or_default();
    asset_tx.send(SessionEvent::RenderMaterials(materials)).ok();
}

/// POSTs a `ModifyMaterialParams` request and forwards the `{ success, message }`
/// reply to `caps_tx` tagged [`CAP_MODIFY_MATERIAL_PARAMS`], for the session to
/// surface as a [`SlSessionEvent::MaterialParamsResult`].
fn run_modify_material_params(cap_url: &str, body: String, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_MODIFY_MATERIAL_PARAMS.to_owned(), llsd))
            .ok();
    }
}

/// POSTs a voice-signalling capability (`ProvisionVoiceAccountRequest` or
/// `ParcelVoiceInfoRequest`) carrying the prepared `body` and forwards the LLSD
/// reply to `caps_tx` tagged with `cap`, for the session to surface as the
/// matching event ([`SlSessionEvent::VoiceAccountProvisioned`] /
/// [`SlSessionEvent::ParcelVoiceInfo`]). Only the grid signalling is handled;
/// the audio session is out of scope.
fn run_voice_cap(cap_url: &str, body: String, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
fn run_voice_signaling(cap_url: &str, body: String) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    http.post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .ok();
}

/// GETs `url` and parses the LLSD-XML reply, returning `None` on any
/// transport/parse failure. Shared by the experience capability fetches.
fn blocking_get_llsd(url: &str) -> Option<Llsd> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
        .ok()?;
    let text = response.text().ok()?;
    parse_llsd_xml(&text).ok()
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event).
fn run_get_caps_llsd(url: &str, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    if let Some(llsd) = blocking_get_llsd(url) {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// PUTs `body` to an experience capability URL (the `Allow`/`Block` preference
/// set) and forwards the LLSD reply to `caps_tx` tagged `cap`.
fn run_put_caps_llsd(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .put(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    if let Ok(text) = response.text()
        && let Ok(llsd) = parse_llsd_xml(&text)
    {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// Sends an HTTP PATCH of `body` to an AIS3 inventory capability URL (a folder /
/// item update or move) and forwards the LLSD reply to `caps_tx` tagged `cap`.
fn run_patch_caps_llsd(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .patch(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    if let Ok(text) = response.text()
        && let Ok(llsd) = parse_llsd_xml(&text)
    {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// Sends an HTTP DELETE to an experience capability URL (the `Forget`
/// preference) and forwards the LLSD reply to `caps_tx` tagged `cap`.
fn run_delete_caps_llsd(cap_url: &str, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .delete(cap_url)
        .header("Accept", "application/llsd+xml")
        .send()
    else {
        return;
    };
    if let Ok(text) = response.text()
        && let Ok(llsd) = parse_llsd_xml(&text)
    {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// GETs the `GroupExperiences` capability and forwards an
/// [`SlSessionEvent::GroupExperiences`] over `asset_tx`, echoing the queried
/// `group_id` (the cap reply does not carry it).
fn run_group_experiences(url: &str, group_id: Uuid, asset_tx: &Sender<SessionEvent>) {
    if let Some(llsd) = blocking_get_llsd(url) {
        asset_tx
            .send(SessionEvent::GroupExperiences {
                group_id,
                experience_ids: parse_experience_ids(&llsd),
            })
            .ok();
    }
}

/// GETs an `IsExperienceAdmin` (`admin` true) or `IsExperienceContributor`
/// (`admin` false) capability and forwards the corresponding status event over
/// `asset_tx`, echoing the queried `experience_id`.
fn run_experience_status(
    url: &str,
    experience_id: Uuid,
    admin: bool,
    asset_tx: &Sender<SessionEvent>,
) {
    let Some(llsd) = blocking_get_llsd(url) else {
        return;
    };
    let status = parse_experience_status(&llsd);
    let event = if admin {
        SessionEvent::ExperienceAdminStatus {
            experience_id,
            is_admin: status,
        }
    } else {
        SessionEvent::ExperienceContributorStatus {
            experience_id,
            is_contributor: status,
        }
    };
    asset_tx.send(event).ok();
}

/// Spawns the modern `NewFileAgentInventory` two-step CAPS upload on a background
/// thread, emitting [`SlSessionEvent::AssetUploaded`] /
/// [`SlSessionEvent::AssetUploadFailed`] over the asset channel. Emits a failure
/// immediately if the asset/inventory type is not uploadable or the capability
/// is unavailable.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the flat NewFileAgentInventory upload command fields"
)]
fn spawn_new_file_upload(
    caps: Option<&Caps>,
    folder_id: Uuid,
    asset_type: AssetType,
    inventory_type: InventoryType,
    name: &str,
    description: &str,
    next_owner_mask: u32,
    group_mask: u32,
    everyone_mask: u32,
    expected_upload_cost: i32,
    data: Vec<u8>,
) {
    let (Some(asset_name), Some(inv_name)) =
        (asset_type.caps_asset_name(), inventory_type.caps_name())
    else {
        emit_upload_failure(caps, "asset/inventory type is not uploadable".to_owned());
        return;
    };
    let Some(caps) = caps else {
        return;
    };
    let Some(url) = caps.map.get(CAP_NEW_FILE_AGENT_INVENTORY).cloned() else {
        let asset_tx = caps.asset_tx.clone();
        asset_tx
            .send(SessionEvent::AssetUploadFailed {
                reason: "NewFileAgentInventory capability not available".to_owned(),
            })
            .ok();
        return;
    };
    let body = build_new_file_agent_inventory_request(
        folder_id,
        asset_name,
        inv_name,
        name,
        description,
        next_owner_mask,
        group_mask,
        everyone_mask,
        expected_upload_cost,
    );
    let asset_tx = caps.asset_tx.clone();
    std::thread::spawn(move || {
        let event = run_caps_upload(&url, body, data);
        asset_tx.send(event).ok();
    });
}

/// Emits an [`SlSessionEvent::AssetUploadFailed`] over the asset channel naming a
/// capability that is not available on the current region.
fn emit_upload_unavailable(caps: Option<&Caps>, cap: &str) {
    emit_upload_failure(caps, format!("{cap} capability not available"));
}

/// Emits an [`SlSessionEvent::AssetUploadFailed`] over the asset channel with the
/// given reason (a no-op if no capabilities are established yet).
fn emit_upload_failure(caps: Option<&Caps>, reason: String) {
    if let Some(caps) = caps {
        caps.asset_tx
            .send(SessionEvent::AssetUploadFailed { reason })
            .ok();
    }
}

/// Runs both steps of a modern CAPS asset upload synchronously (on the calling
/// background thread): POST the LLSD `metadata` to `cap_url` for an `uploader`
/// URL, then POST the raw `data` bytes there. Returns
/// [`SlSessionEvent::AssetUploaded`] on success or
/// [`SlSessionEvent::AssetUploadFailed`] on any failure.
fn run_caps_upload(cap_url: &str, metadata: String, data: Vec<u8>) -> SessionEvent {
    // Step 1: POST the metadata, expecting an `uploader` URL back.
    let uploader = match caps_upload_step(cap_url, "application/llsd+xml", metadata.into_bytes()) {
        Ok(response) => match response.uploader {
            Some(url) => url,
            None => {
                return SessionEvent::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return SessionEvent::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw asset bytes to the uploader URL.
    match caps_upload_step(&uploader, "application/octet-stream", data) {
        Ok(response) => match response.new_asset {
            Some(new_asset) => SessionEvent::AssetUploaded {
                new_asset,
                new_inventory_item: response.new_inventory_item,
            },
            None => SessionEvent::AssetUploadFailed {
                reason: response.error.unwrap_or_else(|| {
                    format!("upload did not complete (state {})", response.state)
                }),
            },
        },
        Err(reason) => SessionEvent::AssetUploadFailed { reason },
    }
}

/// POSTs one step of a CAPS upload (blocking) and parses the LLSD response,
/// returning the parsed [`AssetUploadResponse`] or a failure reason.
fn caps_upload_step(
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<AssetUploadResponse, String> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .map_err(|error| format!("HTTP client build failed: {error}"))?;
    let response = http
        .post(url)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .map_err(|error| format!("upload request failed: {error}"))?;
    let text = response
        .text()
        .map_err(|error| format!("upload response read failed: {error}"))?;
    parse_asset_upload_response(&text)
        .map_err(|error| format!("upload response parse failed: {error}"))
}

/// Performs a blocking HTTP `GET`, returning the body bytes on a 2xx response,
/// or `None` on any network/HTTP failure. When `max_bytes` is `Some`, requests
/// only the first `max_bytes` via a `Range: bytes=0-(max_bytes-1)` header.
fn blocking_get_bytes(url: &str, max_bytes: Option<usize>) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let mut request = http.get(url);
    if let Some(max) = max_bytes {
        request = request.header("Range", format!("bytes=0-{}", max.saturating_sub(1)));
    }
    let response = request.send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

/// Performs a blocking HTTP `GET` for an inclusive `(start, end)` byte range via
/// a `Range: bytes=start-end` header, returning the body on a 2xx response.
fn blocking_get_range(url: &str, start: u32, end: u32) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http
        .get(url)
        .header("Range", format!("bytes={start}-{end}"))
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

/// GETs a texture from the `GetTexture` capability and forwards a
/// [`SlSessionEvent::TextureReceived`] or a [`SlSessionEvent::TextureNotFound`]
/// over `asset_tx`. For a non-zero `discard_level` only the level-of-detail
/// prefix is fetched, using HTTP `Range` requests (see [`fetch_texture_lod`]).
fn run_texture_fetch(
    cap_url: &str,
    texture_id: Uuid,
    discard_level: u8,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match fetch_texture_lod(&url, discard_level) {
        Some(data) => SessionEvent::TextureReceived(Box::new(Texture {
            id: texture_id,
            codec: ImageCodec::J2c,
            data,
        })),
        None => SessionEvent::TextureNotFound(texture_id),
    };
    asset_tx.send(event).ok();
}

/// Fetches the codestream bytes for a texture at `discard_level` using HTTP
/// `Range` requests to transfer only the needed LOD prefix: a small probe reads
/// the J2C [`j2c::Header`], from which the prefix length is computed, then a
/// second `Range` request fetches exactly that prefix when the probe did not
/// already cover it. Returns `None` on a 404 / network failure.
fn fetch_texture_lod(url: &str, discard_level: u8) -> Option<Vec<u8>> {
    if discard_level == 0 {
        return blocking_get_bytes(url, None);
    }
    let probe = blocking_get_bytes(url, Some(j2c::FIRST_PACKET_SIZE))?;
    let Some(header) = j2c::parse_header(&probe) else {
        return Some(probe);
    };
    let target = header.discard_data_size(discard_level);
    if probe.len() >= target {
        return Some(probe.get(..target).unwrap_or(&probe).to_vec());
    }
    let body = blocking_get_bytes(url, Some(target))?;
    let size = target.min(body.len());
    Some(body.get(..size).unwrap_or(&body).to_vec())
}

/// GETs an asset from `{cap_url}/{query}` and forwards a
/// [`SlSessionEvent::AssetReceived`] (or a [`SlSessionEvent::AssetTransferFailed`]
/// with the 404-equivalent [`TransferStatus::UnknownSource`]) over `asset_tx`.
/// An inclusive `byte_range` issues an HTTP `Range` request for just that span.
fn run_asset_fetch(
    cap_url: &str,
    query: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/{query}");
    let bytes = match byte_range {
        Some((start, end)) => blocking_get_range(&url, start, end),
        None => blocking_get_bytes(&url, None),
    };
    let event = match bytes {
        Some(data) => SessionEvent::AssetReceived(Box::new(Asset {
            id: asset_id,
            asset_type,
            data,
        })),
        None => SessionEvent::AssetTransferFailed {
            asset_id,
            asset_type,
            status: TransferStatus::UnknownSource,
        },
    };
    asset_tx.send(event).ok();
}

/// GETs a generic asset from the `GetAsset` capability using the asset class's
/// query key, forwarding the result over `asset_tx` (or an
/// [`SlSessionEvent::AssetTransferFailed`] for a class the cap cannot serve). An
/// inclusive `byte_range` issues an HTTP `Range` request for just that span.
fn run_generic_asset_fetch(
    cap_url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    asset_tx: &Sender<SessionEvent>,
) {
    match asset_type.get_asset_query_key() {
        Some(key) => {
            run_asset_fetch(
                cap_url,
                &format!("?{key}={asset_id}"),
                asset_id,
                asset_type,
                byte_range,
                asset_tx,
            );
        }
        None => {
            asset_tx
                .send(SessionEvent::AssetTransferFailed {
                    asset_id,
                    asset_type,
                    status: TransferStatus::Error,
                })
                .ok();
        }
    }
}

/// Emits a disconnect event.
fn emit_disconnect(events: &mut EventWriter<SlEvent>, reason: DisconnectReason) {
    events.write(SlEvent(SessionEvent::Disconnected(reason)));
}
