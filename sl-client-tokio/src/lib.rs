#![doc = include_str!("../README.md")]

use std::io::Error as IoError;
use std::time::{Duration, Instant};

use reqwest::Client as ReqwestClient;
use reqwest::Error as ReqwestError;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use std::collections::HashMap;

use sl_proto::{
    CAP_AGENT_EXPERIENCES, CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES,
    CAP_FETCH_INVENTORY, CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES, CAP_GET_ASSET,
    CAP_GET_CREATOR_EXPERIENCES, CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_MESH,
    CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA,
    CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT,
    CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES, CAP_RENDER_MATERIALS,
    CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, CAP_UPLOAD_BAKED_TEXTURE,
    CAP_VOICE_SIGNALING, Llsd, RECV_BUFFER_SIZE, REQUESTED_CAPABILITIES, Session,
    ais_category_children_fetch_url, ais_category_children_url, ais_category_url,
    ais_create_category_url, ais_item_url, build_ais_create_category_body, build_ais_move_body,
    build_ais_rename_category_body, build_ais_update_item_body,
    build_create_inventory_category_request, build_event_queue_request,
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

// Re-export the core types a consumer needs so they can depend on this crate
// alone.
pub use sl_proto::{
    ActiveGroup, AnyMessage, Asset, AssetType, AvatarClassified, AvatarGroupMembership,
    AvatarInterests, AvatarPick, AvatarProperties, Camera, ChatAudible, ChatMessage,
    ChatSourceType, ChatType, ClassifiedInfo, ClassifiedUpdate, ClickAction, Command, ControlFlags,
    CreateGroupParams, DeRezDestination, DisconnectReason, EconomyData, EstateAccessDelta,
    EstateAccessKind, EstateInfo, Event, ExperienceInfo, ExperiencePermission,
    ExperienceProperties, ExperienceUpdate, ExtendedMesh, FlexibleData, Friend, FriendRights,
    GltfMaterialOverride, GroupMember, GroupMembership, GroupNotice, GroupNoticeAttachment,
    GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, HomeLocation, IceCandidate, ImDialog,
    ImageCodec, InstantMessage, InterestsUpdate, InventoryFolder, InventoryItem, InventoryOffer,
    InventoryType, LandingType, LegacyMaterial, LightData, LightImage, LindenAmount,
    LoadUrlRequest, LoginAccount, LoginParams, LoginRequest, LoginResponse, MEDIA_PERM_ALL,
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
    SculptData, SoundFlags, SoundPreload, TerrainLayerType, TerrainPatch, Texture,
    TextureAnimation, TextureEntry, TextureFace, Throttle, TransferStatus, Transmit, Uuid, Vector,
    VoiceAccountInfo, VoiceProvisionRequest, Wearable, WearableType, avatar_texture,
    decode_particle_system, decode_texture_anim, decode_texture_entry, grid_to_handle,
    group_powers, handle_to_global, handle_to_grid, particle_pattern, pcode, sim_access,
    texture_anim_mode,
};

/// How long to sleep when the session has no scheduled timeout.
const IDLE_SLEEP: Duration = Duration::from_secs(3600);

/// An error from the tokio client.
#[derive(Debug, Error)]
pub enum Error {
    /// A UDP socket I/O error.
    #[error("socket I/O error: {0}")]
    Io(#[from] IoError),
    /// An HTTP error while performing the XML-RPC login.
    #[error("login HTTP error: {0}")]
    Http(#[from] ReqwestError),
    /// The login response could not be parsed.
    #[error("login parse error: {0}")]
    Login(#[from] sl_wire::LoginParseError),
    /// A protocol state-machine error.
    #[error("protocol error: {0}")]
    Proto(#[from] sl_proto::Error),
    /// The grid rejected the login.
    #[error("login rejected: {reason} ({message})")]
    LoginRejected {
        /// The machine-readable reason code.
        reason: String,
        /// The human-readable message.
        message: String,
    },
    /// The grid requires a multi-factor one-time code. Retry [`Client::connect`]
    /// with a [`LoginRequest`] prepared via `LoginRequest::with_mfa`.
    #[error("multi-factor authentication required: {}", .0.message)]
    MfaChallenge(MfaChallenge),
    /// The session unexpectedly had no login request to perform.
    #[error("the session produced no login request")]
    NoLoginRequest,
}

/// A tokio-driven Second Life / OpenSim client wrapping a sans-I/O [`Session`].
#[derive(Debug)]
pub struct Client {
    /// The sans-I/O session being driven.
    session: Session,
    /// The bound UDP socket.
    socket: UdpSocket,
    /// A reusable receive buffer.
    recv_buf: Vec<u8>,
}

impl Client {
    /// Logs in over XML-RPC, binds a UDP socket, and bootstraps the circuit.
    ///
    /// # Errors
    ///
    /// Returns an [`enum@Error`] if the login HTTP request, the response parse, the
    /// socket bind, or the circuit bootstrap fails.
    pub async fn connect(params: LoginParams) -> Result<Self, Error> {
        let mut session = Session::new(params);
        let request = session.login_http_request().ok_or(Error::NoLoginRequest)?;

        let http = ReqwestClient::new();
        let body = http
            .post(&request.url)
            .header("Content-Type", "text/xml")
            .header("User-Agent", &request.user_agent)
            .body(request.body)
            .send()
            .await?
            .text()
            .await?;
        let success = match parse_login_response(&body)? {
            LoginResponse::Success(success) => *success,
            LoginResponse::MfaChallenge(challenge) => return Err(Error::MfaChallenge(challenge)),
            LoginResponse::Failure(failure) => {
                return Err(Error::LoginRejected {
                    reason: failure.reason,
                    message: failure.message,
                });
            }
        };

        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        session.handle_login_response(LoginResponse::Success(Box::new(success)), Instant::now())?;

        Ok(Self {
            session,
            socket,
            recv_buf: vec![0u8; RECV_BUFFER_SIZE],
        })
    }

    /// The agent's own id, available once logged in. Useful for self-directed
    /// requests (e.g. reading one's own picks or classifieds) before
    /// [`Client::run`] consumes the client.
    #[must_use]
    pub fn agent_id(&self) -> Option<Uuid> {
        self.session.agent_id()
    }

    /// Runs the session until it is disconnected or logged out, forwarding
    /// events to `events` and applying commands from `commands`.
    ///
    /// # Errors
    ///
    /// Returns an [`enum@Error`] on an unrecoverable socket or protocol error.
    pub async fn run(
        mut self,
        events: mpsc::Sender<Event>,
        mut commands: mpsc::Receiver<Command>,
    ) -> Result<(), Error> {
        // The region's capability map is fetched once from the seed and cached
        // here: the event-queue long-poll runs off `EventQueueGet`, and inventory
        // fetches POST to `FetchInventoryDescendents2`. Both deliver their decoded
        // payloads back over `caps_rx` to `handle_caps_event`.
        let http = ReqwestClient::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let (caps_tx, mut caps_rx) = mpsc::channel::<(String, Llsd)>(64);
        let mut caps = fetch_capabilities(self.session.seed_capability(), &http).await;
        let mut caps_task = spawn_event_queue(&caps, &http, &caps_tx);

        loop {
            while let Some(transmit) = self.session.poll_transmit() {
                self.socket
                    .send_to(&transmit.payload, transmit.destination)
                    .await?;
            }

            while let Some(event) = self.session.poll_event() {
                let terminal = matches!(event, Event::Disconnected(_) | Event::LoggedOut);
                // A region change brings a new seed capability, so re-fetch the
                // capability map and restart the event-queue poller.
                let region_changed = matches!(event, Event::RegionChanged { .. });
                // POST a neighbour's seed capability so the simulator starts
                // streaming that region's scene to the child circuit (its
                // `SendInitialData` is gated on the seed having been requested).
                // Detached: the POST must not block the main loop.
                if let Event::NeighborSeed {
                    seed_capability, ..
                } = &event
                {
                    let seed = seed_capability.clone();
                    let http = http.clone();
                    tokio::spawn(async move {
                        let _ignored = fetch_capabilities(Some(&seed), &http).await;
                    });
                }
                events.send(event).await.ok();
                if region_changed {
                    abort_task(&mut caps_task);
                    caps = fetch_capabilities(self.session.seed_capability(), &http).await;
                    caps_task = spawn_event_queue(&caps, &http, &caps_tx);
                }
                if terminal {
                    abort_task(&mut caps_task);
                    return Ok(());
                }
            }
            if self.session.is_closed() {
                abort_task(&mut caps_task);
                return Ok(());
            }

            let sleep = make_sleep(self.session.poll_timeout());
            tokio::pin!(sleep);

            tokio::select! {
                result = self.socket.recv_from(&mut self.recv_buf) => {
                    let (len, from) = result?;
                    if let Some(datagram) = self.recv_buf.get(..len) {
                        self.session.handle_datagram(from, datagram, Instant::now())?;
                    }
                }
                caps_event = caps_rx.recv() => {
                    if let Some((message, body)) = caps_event {
                        self.session.handle_caps_event(&message, &body, Instant::now())?;
                    }
                }
                command = commands.recv() => {
                    match command {
                        Some(Command::Send { message, reliability }) => {
                            self.session.enqueue(*message, reliability, Instant::now())?;
                        }
                        Some(Command::Chat { message, chat_type, channel }) => {
                            self.session.say(&message, chat_type, channel, Instant::now())?;
                        }
                        Some(Command::Typing(typing)) => {
                            self.session.set_typing(typing, Instant::now())?;
                        }
                        Some(Command::InstantMessage { to_agent_id, message }) => {
                            self.session.send_instant_message(to_agent_id, &message, Instant::now())?;
                        }
                        Some(Command::ImTyping { to_agent_id, typing }) => {
                            self.session.send_im_typing(to_agent_id, typing, Instant::now())?;
                        }
                        Some(Command::SetControls(controls)) => {
                            self.session.set_controls(controls, Instant::now())?;
                        }
                        Some(Command::SetThrottle(throttle)) => {
                            self.session.set_throttle(throttle, Instant::now())?;
                        }
                        Some(Command::SetRotation { body, head }) => {
                            self.session.set_rotation(body, head, Instant::now())?;
                        }
                        Some(Command::SetCamera(camera)) => {
                            self.session.set_camera(camera, Instant::now())?;
                        }
                        Some(Command::Stand) => {
                            self.session.stand(Instant::now())?;
                        }
                        Some(Command::SitOnGround) => {
                            self.session.sit_on_ground(Instant::now())?;
                        }
                        Some(Command::Sit { target, offset }) => {
                            self.session.sit_on(target, offset, Instant::now())?;
                        }
                        Some(Command::Autopilot { global_x, global_y, z }) => {
                            self.session.autopilot_to(global_x, global_y, z, Instant::now())?;
                        }
                        Some(Command::RequestAvatarProperties(target)) => {
                            self.session.request_avatar_properties(target, Instant::now())?;
                        }
                        Some(Command::RequestAvatarPicks(target)) => {
                            self.session.request_avatar_picks(target, Instant::now())?;
                        }
                        Some(Command::RequestAvatarNotes(target)) => {
                            self.session.request_avatar_notes(target, Instant::now())?;
                        }
                        Some(Command::RequestAvatarClassifieds(target)) => {
                            self.session
                                .request_avatar_classifieds(target, Instant::now())?;
                        }
                        Some(Command::RequestPickInfo {
                            creator_id,
                            pick_id,
                        }) => {
                            self.session
                                .request_pick_info(creator_id, pick_id, Instant::now())?;
                        }
                        Some(Command::RequestClassifiedInfo(classified_id)) => {
                            self.session
                                .request_classified_info(classified_id, Instant::now())?;
                        }
                        Some(Command::UpdateProfile(update)) => {
                            self.session.update_profile(&update, Instant::now())?;
                        }
                        Some(Command::UpdateInterests(update)) => {
                            self.session.update_interests(&update, Instant::now())?;
                        }
                        Some(Command::UpdateAvatarNotes { target_id, notes }) => {
                            self.session
                                .update_avatar_notes(target_id, &notes, Instant::now())?;
                        }
                        Some(Command::UpdatePick(update)) => {
                            self.session.update_pick(&update, Instant::now())?;
                        }
                        Some(Command::DeletePick(pick_id)) => {
                            self.session.delete_pick(pick_id, Instant::now())?;
                        }
                        Some(Command::GodDeletePick { pick_id, query_id }) => {
                            self.session
                                .god_delete_pick(pick_id, query_id, Instant::now())?;
                        }
                        Some(Command::UpdateClassified(update)) => {
                            self.session.update_classified(&update, Instant::now())?;
                        }
                        Some(Command::DeleteClassified(classified_id)) => {
                            self.session
                                .delete_classified(classified_id, Instant::now())?;
                        }
                        Some(Command::GodDeleteClassified {
                            classified_id,
                            query_id,
                        }) => {
                            self.session.god_delete_classified(
                                classified_id,
                                query_id,
                                Instant::now(),
                            )?;
                        }
                        Some(Command::RequestFolderContents(folder_id)) => {
                            self.session.request_folder_contents(folder_id, Instant::now())?;
                        }
                        Some(Command::FetchInventoryFolders(folder_ids)) => {
                            if let (Some(url), Some(owner)) =
                                (caps.get(CAP_FETCH_INVENTORY).cloned(), self.session.agent_id())
                            {
                                tokio::spawn(fetch_inventory(
                                    url, owner, folder_ids, http.clone(), caps_tx.clone(),
                                ));
                            }
                        }
                        Some(Command::CreateInventoryFolder { folder_id, parent_id, folder_type, name }) => {
                            self.session.create_inventory_folder(folder_id, parent_id, folder_type, &name, Instant::now())?;
                        }
                        Some(Command::UpdateInventoryFolder { folder_id, parent_id, folder_type, name }) => {
                            self.session.update_inventory_folder(folder_id, parent_id, folder_type, &name, Instant::now())?;
                        }
                        Some(Command::MoveInventoryFolder { folder_id, parent_id }) => {
                            self.session.move_inventory_folder(folder_id, parent_id, Instant::now())?;
                        }
                        Some(Command::RemoveInventoryFolders(folder_ids)) => {
                            self.session.remove_inventory_folders(&folder_ids, Instant::now())?;
                        }
                        Some(Command::CreateInventoryItem(new)) => {
                            self.session.create_inventory_item(&new, Instant::now())?;
                        }
                        Some(Command::UpdateInventoryItem { item, transaction_id }) => {
                            self.session.update_inventory_item(&item, transaction_id, Instant::now())?;
                        }
                        Some(Command::MoveInventoryItem { item_id, folder_id, new_name }) => {
                            self.session.move_inventory_item(item_id, folder_id, &new_name, Instant::now())?;
                        }
                        Some(Command::CopyInventoryItem { old_agent_id, old_item_id, new_folder_id, new_name }) => {
                            self.session.copy_inventory_item(old_agent_id, old_item_id, new_folder_id, &new_name, Instant::now())?;
                        }
                        Some(Command::RemoveInventoryItems(item_ids)) => {
                            self.session.remove_inventory_items(&item_ids, Instant::now())?;
                        }
                        Some(Command::ChangeInventoryItemFlags { item_id, flags }) => {
                            self.session.change_inventory_item_flags(item_id, flags, Instant::now())?;
                        }
                        Some(Command::PurgeInventoryDescendents(folder_id)) => {
                            self.session.purge_inventory_descendents(folder_id, Instant::now())?;
                        }
                        Some(Command::RemoveInventoryObjects { folder_ids, item_ids }) => {
                            self.session.remove_inventory_objects(&folder_ids, &item_ids, Instant::now())?;
                        }
                        Some(Command::CreateInventoryCategory { parent_id, folder_type, name }) => {
                            if let Some(url) = caps.get(CAP_CREATE_INVENTORY_CATEGORY).cloned() {
                                let body = build_create_inventory_category_request(Uuid::new_v4(), parent_id, folder_type, &name);
                                tokio::spawn(post_voice_cap(url, body, CAP_CREATE_INVENTORY_CATEGORY, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3CreateFolder { parent_id, folder_type, name }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_create_category_url(parent_id, Uuid::new_v4()));
                                let body = build_ais_create_category_body(folder_type, &name);
                                tokio::spawn(post_voice_cap(url, body, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3RenameFolder { folder_id, name }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_url(folder_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_rename_category_body(&name), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3MoveFolder { folder_id, parent_id }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_url(folder_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_move_body(parent_id), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3RemoveFolder(folder_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_url(folder_id));
                                tokio::spawn(delete_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3PurgeFolder(folder_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_children_url(folder_id));
                                tokio::spawn(delete_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3FetchFolderChildren { folder_id, depth }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_children_fetch_url(folder_id, depth));
                                tokio::spawn(get_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3UpdateItem { item_id, name, description }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_update_item_body(&name, &description), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3MoveItem { item_id, parent_id }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_move_body(parent_id), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3RemoveItem(item_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(delete_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3FetchItem(item_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(get_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::FetchGroupMembers(group_id)) => {
                            if let Some(url) = caps.get(CAP_GROUP_MEMBER_DATA).cloned() {
                                tokio::spawn(fetch_group_members(url, group_id, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::OfferFriendship { to_agent_id, message }) => {
                            self.session.send_friendship_offer(to_agent_id, &message, Instant::now())?;
                        }
                        Some(Command::GrantUserRights { target, rights }) => {
                            self.session.grant_user_rights(target, rights, Instant::now())?;
                        }
                        Some(Command::TerminateFriendship(other)) => {
                            self.session.terminate_friendship(other, Instant::now())?;
                        }
                        Some(Command::AcceptFriendship { transaction_id, calling_card_folder }) => {
                            self.session.accept_friendship(transaction_id, calling_card_folder, Instant::now())?;
                        }
                        Some(Command::DeclineFriendship(transaction_id)) => {
                            self.session.decline_friendship(transaction_id, Instant::now())?;
                        }
                        Some(Command::ActivateGroup(group_id)) => {
                            self.session.activate_group(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupMembers(group_id)) => {
                            self.session.request_group_members(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupRoles(group_id)) => {
                            self.session.request_group_roles(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupRoleMembers(group_id)) => {
                            self.session.request_group_role_members(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupTitles(group_id)) => {
                            self.session.request_group_titles(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupProfile(group_id)) => {
                            self.session.request_group_profile(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupNotices(group_id)) => {
                            self.session.request_group_notices(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupNotice(notice_id)) => {
                            self.session.request_group_notice(notice_id, Instant::now())?;
                        }
                        Some(Command::CreateGroup(params)) => {
                            self.session.create_group(&params, Instant::now())?;
                        }
                        Some(Command::JoinGroup(group_id)) => {
                            self.session.join_group(group_id, Instant::now())?;
                        }
                        Some(Command::LeaveGroup(group_id)) => {
                            self.session.leave_group(group_id, Instant::now())?;
                        }
                        Some(Command::InviteToGroup { group_id, invitees }) => {
                            self.session.invite_to_group(group_id, &invitees, Instant::now())?;
                        }
                        Some(Command::SetGroupAcceptNotices { group_id, accept_notices, list_in_profile }) => {
                            self.session.set_group_accept_notices(group_id, accept_notices, list_in_profile, Instant::now())?;
                        }
                        Some(Command::SetGroupContribution { group_id, contribution }) => {
                            self.session.set_group_contribution(group_id, contribution, Instant::now())?;
                        }
                        Some(Command::StartGroupSession(group_id)) => {
                            self.session.start_group_session(group_id, Instant::now())?;
                        }
                        Some(Command::SendGroupMessage { group_id, message }) => {
                            self.session.send_group_message(group_id, &message, Instant::now())?;
                        }
                        Some(Command::LeaveGroupSession(group_id)) => {
                            self.session.leave_group_session(group_id, Instant::now())?;
                        }
                        Some(Command::UpdateGroupRoles { group_id, roles }) => {
                            self.session.update_group_roles(group_id, &roles, Instant::now())?;
                        }
                        Some(Command::ChangeGroupRoleMembers { group_id, changes }) => {
                            self.session.change_group_role_members(group_id, &changes, Instant::now())?;
                        }
                        Some(Command::EjectGroupMembers { group_id, member_ids }) => {
                            self.session.eject_group_members(group_id, &member_ids, Instant::now())?;
                        }
                        Some(Command::SendGroupNotice { group_id, subject, message, attachment }) => {
                            self.session.send_group_notice(group_id, &subject, &message, attachment, Instant::now())?;
                        }
                        Some(Command::ReplyScriptDialog { object_id, chat_channel, button_index, button_label }) => {
                            self.session.reply_script_dialog(object_id, chat_channel, button_index, &button_label, Instant::now())?;
                        }
                        Some(Command::AnswerScriptPermissions { task_id, item_id, permissions }) => {
                            self.session.answer_script_permissions(task_id, item_id, permissions, Instant::now())?;
                        }
                        Some(Command::RequestMuteList) => {
                            self.session.request_mute_list(Instant::now())?;
                        }
                        Some(Command::Mute { id, name, mute_type, flags }) => {
                            self.session.mute(id, &name, mute_type, flags, Instant::now())?;
                        }
                        Some(Command::Unmute { id, name }) => {
                            self.session.unmute(id, &name, Instant::now())?;
                        }
                        Some(Command::Teleport { region_handle, position, look_at }) => {
                            self.session.teleport_to(region_handle, position, look_at, Instant::now())?;
                        }
                        Some(Command::RequestRegionInfo) => {
                            self.session.request_region_info(Instant::now())?;
                        }
                        Some(Command::RequestMoneyBalance) => {
                            self.session.request_money_balance(Instant::now())?;
                        }
                        Some(Command::RequestEconomyData) => {
                            self.session.request_economy_data(Instant::now())?;
                        }
                        Some(Command::SendMoneyTransfer { dest, amount, kind, description }) => {
                            self.session.send_money_transfer(
                                dest, amount, kind, &description, Instant::now(),
                            )?;
                        }
                        Some(Command::RequestParcelProperties { west, south, east, north, sequence_id }) => {
                            self.session.request_parcel_properties(
                                west, south, east, north, sequence_id, Instant::now(),
                            )?;
                        }
                        Some(Command::SetDrawDistance(far)) => {
                            self.session.set_draw_distance(far);
                        }
                        Some(Command::RequestMapBlocks { min_x, max_x, min_y, max_y }) => {
                            self.session.request_map_blocks(min_x, max_x, min_y, max_y, Instant::now())?;
                        }
                        Some(Command::RequestMapByName { name }) => {
                            self.session.request_map_by_name(&name, Instant::now())?;
                        }
                        Some(Command::RequestMapItems { item_type, region_handle }) => {
                            self.session.request_map_items(item_type, region_handle, Instant::now())?;
                        }
                        Some(Command::RequestObjects { local_ids }) => {
                            self.session.request_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::RequestObjectProperties { local_ids }) => {
                            self.session.request_object_properties(&local_ids, Instant::now())?;
                        }
                        Some(Command::DeselectObjects { local_ids }) => {
                            self.session.deselect_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::TouchObject { local_id }) => {
                            self.session.touch_object(local_id, Instant::now())?;
                        }
                        Some(Command::GrabObject { local_id, grab_offset }) => {
                            self.session.grab_object(local_id, grab_offset, Instant::now())?;
                        }
                        Some(Command::GrabObjectUpdate { object_id, grab_offset_initial, grab_position, time_since_last }) => {
                            self.session.grab_object_update(object_id, grab_offset_initial, grab_position, time_since_last, Instant::now())?;
                        }
                        Some(Command::DegrabObject { local_id }) => {
                            self.session.degrab_object(local_id, Instant::now())?;
                        }
                        Some(Command::RezObject { shape, group_id }) => {
                            self.session.rez_object(&shape, group_id, Instant::now())?;
                        }
                        Some(Command::DuplicateObjects { local_ids, offset, group_id }) => {
                            self.session.duplicate_objects(&local_ids, offset, group_id, Instant::now())?;
                        }
                        Some(Command::DeleteObjects { local_ids }) => {
                            self.session.delete_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::DerezObjects { local_ids, destination, destination_id, transaction_id, group_id }) => {
                            self.session.derez_objects(&local_ids, destination, destination_id, transaction_id, group_id, Instant::now())?;
                        }
                        Some(Command::UpdateObject { local_id, transform }) => {
                            self.session.update_object(local_id, &transform, Instant::now())?;
                        }
                        Some(Command::SetObjectName { local_id, name }) => {
                            self.session.set_object_name(local_id, &name, Instant::now())?;
                        }
                        Some(Command::SetObjectDescription { local_id, description }) => {
                            self.session.set_object_description(local_id, &description, Instant::now())?;
                        }
                        Some(Command::SetObjectClickAction { local_id, action }) => {
                            self.session.set_object_click_action(local_id, action, Instant::now())?;
                        }
                        Some(Command::SetObjectMaterial { local_id, material }) => {
                            self.session.set_object_material(local_id, material, Instant::now())?;
                        }
                        Some(Command::SetObjectFlags { local_id, flags }) => {
                            self.session.set_object_flags(local_id, &flags, Instant::now())?;
                        }
                        Some(Command::SetObjectGroup { local_ids, group_id }) => {
                            self.session.set_object_group(&local_ids, group_id, Instant::now())?;
                        }
                        Some(Command::SetObjectPermissions { local_ids, field, set, mask }) => {
                            self.session.set_object_permissions(&local_ids, field, set, mask, Instant::now())?;
                        }
                        Some(Command::SetObjectForSale { local_id, sale_type, sale_price }) => {
                            self.session.set_object_for_sale(local_id, sale_type, sale_price, Instant::now())?;
                        }
                        Some(Command::SetObjectCategory { local_id, category }) => {
                            self.session.set_object_category(local_id, category, Instant::now())?;
                        }
                        Some(Command::SetObjectIncludeInSearch { local_id, include }) => {
                            self.session.set_object_include_in_search(local_id, include, Instant::now())?;
                        }
                        Some(Command::LinkObjects { local_ids }) => {
                            self.session.link_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::DelinkObjects { local_ids }) => {
                            self.session.delink_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::UpdateParcel(update)) => {
                            self.session.update_parcel(&update, Instant::now())?;
                        }
                        Some(Command::RequestParcelAccessList { local_id, scope }) => {
                            self.session.request_parcel_access_list(local_id, scope, Instant::now())?;
                        }
                        Some(Command::UpdateParcelAccessList { local_id, scope, entries }) => {
                            self.session.update_parcel_access_list(local_id, scope, &entries, Instant::now())?;
                        }
                        Some(Command::RequestParcelDwell { local_id }) => {
                            self.session.request_parcel_dwell(local_id, Instant::now())?;
                        }
                        Some(Command::BuyParcel { local_id, price, area, group_id, is_group_owned }) => {
                            self.session.buy_parcel(local_id, price, area, group_id, is_group_owned, Instant::now())?;
                        }
                        Some(Command::ReturnParcelObjects { local_id, return_type, owner_ids, task_ids }) => {
                            self.session.return_parcel_objects(local_id, return_type, &owner_ids, &task_ids, Instant::now())?;
                        }
                        Some(Command::SelectParcelObjects { local_id, return_type, object_ids }) => {
                            self.session.select_parcel_objects(local_id, return_type, &object_ids, Instant::now())?;
                        }
                        Some(Command::DeedParcelToGroup { local_id, group_id }) => {
                            self.session.deed_parcel_to_group(local_id, group_id, Instant::now())?;
                        }
                        Some(Command::ReclaimParcel { local_id }) => {
                            self.session.reclaim_parcel(local_id, Instant::now())?;
                        }
                        Some(Command::ReleaseParcel { local_id }) => {
                            self.session.release_parcel(local_id, Instant::now())?;
                        }
                        Some(Command::RequestEstateInfo) => {
                            self.session.request_estate_info(Instant::now())?;
                        }
                        Some(Command::UpdateEstateAccess { delta, target }) => {
                            self.session.update_estate_access(delta, target, Instant::now())?;
                        }
                        Some(Command::KickEstateUser { target }) => {
                            self.session.kick_estate_user(target, Instant::now())?;
                        }
                        Some(Command::TeleportHomeUser { target }) => {
                            self.session.teleport_home_user(target, Instant::now())?;
                        }
                        Some(Command::TeleportHomeAllUsers) => {
                            self.session.teleport_home_all_users(Instant::now())?;
                        }
                        Some(Command::RestartRegion { seconds }) => {
                            self.session.restart_region(seconds, Instant::now())?;
                        }
                        Some(Command::SendEstateMessage { message }) => {
                            self.session.send_estate_message(&message, Instant::now())?;
                        }
                        Some(Command::SetRegionInfo(update)) => {
                            self.session.set_region_info(&update, Instant::now())?;
                        }
                        Some(Command::GodKickUser { target, reason }) => {
                            self.session.god_kick_user(target, &reason, Instant::now())?;
                        }
                        Some(Command::SendGodlikeMessage { method, params }) => {
                            let refs: Vec<&str> = params.iter().map(String::as_str).collect();
                            self.session.send_godlike_message(&method, &refs, Instant::now())?;
                        }
                        Some(Command::RequestTexture { texture_id, discard_level, priority }) => {
                            self.session.request_texture(texture_id, discard_level, priority, Instant::now())?;
                        }
                        Some(Command::RequestAsset { asset_id, asset_type, priority }) => {
                            self.session.request_asset(asset_id, asset_type, priority, Instant::now())?;
                        }
                        Some(Command::FetchTexture { texture_id, discard_level }) => {
                            if let Some(url) = caps.get(CAP_GET_TEXTURE).cloned() {
                                tokio::spawn(fetch_texture_http(
                                    url, texture_id, discard_level, http.clone(), events.clone(),
                                ));
                            }
                        }
                        Some(Command::FetchMesh { mesh_id, byte_range }) => {
                            // GetMesh2 is preferred when offered; fall back to GetMesh.
                            if let Some(url) = caps.get(CAP_GET_MESH2).or_else(|| caps.get(CAP_GET_MESH)).cloned() {
                                tokio::spawn(fetch_mesh_http(url, mesh_id, byte_range, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::FetchAsset { asset_id, asset_type, byte_range }) => {
                            if let Some(url) = caps.get(CAP_GET_ASSET).cloned() {
                                tokio::spawn(fetch_asset_http(
                                    url, asset_id, asset_type, byte_range, http.clone(), events.clone(),
                                ));
                            }
                        }
                        Some(Command::RequestWearables) => {
                            self.session.request_wearables(Instant::now())?;
                        }
                        Some(Command::SetWearing(wearables)) => {
                            self.session.set_wearing(&wearables, Instant::now())?;
                        }
                        Some(Command::SetAppearance { serial, size, texture_entry, visual_params, wearable_cache }) => {
                            self.session.set_appearance(serial, size, &texture_entry, &visual_params, &wearable_cache, Instant::now())?;
                        }
                        Some(Command::RequestCachedTextures { serial, slots }) => {
                            self.session.request_cached_textures(serial, &slots, Instant::now())?;
                        }
                        Some(Command::RequestServerAppearanceUpdate { cof_version }) => {
                            if let Some(url) = caps.get(CAP_UPDATE_AVATAR_APPEARANCE).cloned() {
                                tokio::spawn(request_server_appearance_update(
                                    url, cof_version, http.clone(), caps_tx.clone(),
                                ));
                            }
                        }
                        Some(Command::SetAnimations(animations)) => {
                            self.session.set_animations(&animations, Instant::now())?;
                        }
                        Some(Command::PlayAnimation(anim_id)) => {
                            self.session.play_animation(anim_id, Instant::now())?;
                        }
                        Some(Command::StopAnimation(anim_id)) => {
                            self.session.stop_animation(anim_id, Instant::now())?;
                        }
                        Some(Command::UploadAssetUdp { asset_type, data, temp_file, store_local }) => {
                            self.session.upload_asset_udp(asset_type, data, temp_file, store_local, Instant::now())?;
                        }
                        Some(Command::UploadAsset {
                            folder_id, asset_type, inventory_type, name, description,
                            next_owner_mask, group_mask, everyone_mask, expected_upload_cost, data,
                        }) => {
                            match (asset_type.caps_asset_name(), inventory_type.caps_name()) {
                                (Some(asset_name), Some(inv_name)) => {
                                    if let Some(url) = caps.get(CAP_NEW_FILE_AGENT_INVENTORY).cloned() {
                                        let body = build_new_file_agent_inventory_request(
                                            folder_id, asset_name, inv_name, &name, &description,
                                            next_owner_mask, group_mask, everyone_mask, expected_upload_cost,
                                        );
                                        tokio::spawn(run_caps_upload(url, body, data, http.clone(), events.clone()));
                                    } else {
                                        events.send(Event::AssetUploadFailed {
                                            reason: "NewFileAgentInventory capability not available".to_owned(),
                                        }).await.ok();
                                    }
                                }
                                _ => {
                                    events.send(Event::AssetUploadFailed {
                                        reason: "asset/inventory type is not uploadable".to_owned(),
                                    }).await.ok();
                                }
                            }
                        }
                        Some(Command::UploadBakedTexture { data }) => {
                            if let Some(url) = caps.get(CAP_UPLOAD_BAKED_TEXTURE).cloned() {
                                let body = build_upload_baked_texture_request();
                                tokio::spawn(run_caps_upload(url, body, data, http.clone(), events.clone()));
                            } else {
                                events.send(Event::AssetUploadFailed {
                                    reason: "UploadBakedTexture capability not available".to_owned(),
                                }).await.ok();
                            }
                        }
                        Some(Command::UpdateInventoryAsset { item_id, asset_type, data }) => {
                            match asset_type.update_item_cap() {
                                Some(cap) => {
                                    if let Some(url) = caps.get(cap).cloned() {
                                        let body = build_update_item_asset_request(item_id);
                                        tokio::spawn(run_caps_upload(url, body, data, http.clone(), events.clone()));
                                    } else {
                                        events.send(Event::AssetUploadFailed {
                                            reason: format!("{cap} capability not available"),
                                        }).await.ok();
                                    }
                                }
                                None => {
                                    events.send(Event::AssetUploadFailed {
                                        reason: "asset type has no inventory-update capability".to_owned(),
                                    }).await.ok();
                                }
                            }
                        }
                        Some(Command::RequestObjectMedia { object_id }) => {
                            if let Some(url) = caps.get(CAP_OBJECT_MEDIA).cloned() {
                                tokio::spawn(fetch_object_media(url, object_id, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetObjectMedia { object_id, faces }) => {
                            if let Some(url) = caps.get(CAP_OBJECT_MEDIA).cloned() {
                                let body = build_object_media_update_request(object_id, &faces);
                                tokio::spawn(post_object_media(url, body, http.clone()));
                            }
                        }
                        Some(Command::NavigateObjectMedia { object_id, face, url: media_url }) => {
                            if let Some(url) = caps.get(CAP_OBJECT_MEDIA_NAVIGATE).cloned() {
                                let body = build_object_media_navigate_request(object_id, face, &media_url);
                                tokio::spawn(post_object_media(url, body, http.clone()));
                            }
                        }
                        Some(Command::RequestRenderMaterials { material_ids }) => {
                            if let Some(url) = caps.get(CAP_RENDER_MATERIALS).cloned() {
                                tokio::spawn(fetch_render_materials(url, material_ids, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::ModifyMaterialParams { updates }) => {
                            if let Some(url) = caps.get(CAP_MODIFY_MATERIAL_PARAMS).cloned() {
                                let body = build_modify_material_params_request(&updates);
                                tokio::spawn(post_modify_material_params(url, body, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestVoiceAccount { request }) => {
                            if let Some(url) = caps.get(CAP_PROVISION_VOICE_ACCOUNT).cloned() {
                                let body = build_provision_voice_account_request(&request);
                                tokio::spawn(post_voice_cap(url, body, CAP_PROVISION_VOICE_ACCOUNT, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestParcelVoiceInfo) => {
                            if let Some(url) = caps.get(CAP_PARCEL_VOICE_INFO).cloned() {
                                let body = build_parcel_voice_info_request();
                                tokio::spawn(post_voice_cap(url, body, CAP_PARCEL_VOICE_INFO, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SendVoiceSignaling { viewer_session, candidates, completed }) => {
                            if let Some(url) = caps.get(CAP_VOICE_SIGNALING).cloned() {
                                let body = build_voice_signaling_request(&viewer_session, &candidates, completed);
                                tokio::spawn(post_voice_signaling(url, body, http.clone()));
                            }
                        }
                        Some(Command::RequestExperienceInfo { experience_ids }) => {
                            if let Some(base) = caps.get(CAP_GET_EXPERIENCE_INFO).cloned() {
                                let url = format!("{base}{}", experience_info_query(&experience_ids));
                                tokio::spawn(get_caps_llsd(url, CAP_GET_EXPERIENCE_INFO, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::FindExperiences { query, page }) => {
                            if let Some(base) = caps.get(CAP_FIND_EXPERIENCE_BY_NAME).cloned() {
                                let url = format!("{base}{}", find_experience_query(&query, page));
                                tokio::spawn(get_caps_llsd(url, CAP_FIND_EXPERIENCE_BY_NAME, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestExperiencePermissions) => {
                            if let Some(url) = caps.get(CAP_GET_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_GET_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetExperiencePermission { experience_id, permission }) => {
                            if let Some(base) = caps.get(CAP_EXPERIENCE_PREFERENCES).cloned() {
                                if permission.is_forget() {
                                    let url = format!("{base}{}", forget_experience_query(experience_id));
                                    tokio::spawn(delete_caps_llsd(url, CAP_EXPERIENCE_PREFERENCES, http.clone(), caps_tx.clone()));
                                } else {
                                    let body = build_set_experience_permission_request(experience_id, permission);
                                    tokio::spawn(put_caps_llsd(base, body, CAP_EXPERIENCE_PREFERENCES, http.clone(), caps_tx.clone()));
                                }
                            }
                        }
                        Some(Command::RequestOwnedExperiences) => {
                            if let Some(url) = caps.get(CAP_AGENT_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_AGENT_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestAdminExperiences) => {
                            if let Some(url) = caps.get(CAP_GET_ADMIN_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_GET_ADMIN_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestCreatorExperiences) => {
                            if let Some(url) = caps.get(CAP_GET_CREATOR_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_GET_CREATOR_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestGroupExperiences { group_id }) => {
                            if let Some(base) = caps.get(CAP_GROUP_EXPERIENCES).cloned() {
                                let url = format!("{base}{}", group_experiences_query(group_id));
                                tokio::spawn(fetch_group_experiences(url, group_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::RequestExperienceAdmin { experience_id }) => {
                            if let Some(base) = caps.get(CAP_IS_EXPERIENCE_ADMIN).cloned() {
                                let url = format!("{base}{}", experience_id_query(experience_id));
                                tokio::spawn(fetch_experience_admin(url, experience_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::RequestExperienceContributor { experience_id }) => {
                            if let Some(base) = caps.get(CAP_IS_EXPERIENCE_CONTRIBUTOR).cloned() {
                                let url = format!("{base}{}", experience_id_query(experience_id));
                                tokio::spawn(fetch_experience_contributor(url, experience_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::UpdateExperience { update }) => {
                            if let Some(url) = caps.get(CAP_UPDATE_EXPERIENCE).cloned() {
                                let body = build_update_experience_request(&update);
                                tokio::spawn(post_voice_cap(url, body, CAP_UPDATE_EXPERIENCE, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestRegionExperiences) => {
                            if let Some(url) = caps.get(CAP_REGION_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_REGION_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetRegionExperiences { allowed, blocked, trusted }) => {
                            if let Some(url) = caps.get(CAP_REGION_EXPERIENCES).cloned() {
                                let body = build_region_experiences_request(&allowed, &blocked, &trusted);
                                tokio::spawn(post_voice_cap(url, body, CAP_REGION_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::OfferTeleport { targets, message }) => {
                            self.session.offer_teleport(&targets, &message, Instant::now())?;
                        }
                        Some(Command::AcceptTeleportLure { lure_id }) => {
                            self.session.accept_teleport_lure(lure_id, Instant::now())?;
                        }
                        Some(Command::DeclineTeleportLure { from_agent_id, lure_id }) => {
                            self.session.decline_teleport_lure(from_agent_id, lure_id, Instant::now())?;
                        }
                        Some(Command::RequestTeleport { to_agent_id, message }) => {
                            self.session.request_teleport(to_agent_id, &message, Instant::now())?;
                        }
                        Some(Command::GiveInventory { to_agent_id, item_id, asset_type, item_name, transaction_id }) => {
                            self.session.give_inventory(to_agent_id, item_id, asset_type, &item_name, transaction_id, Instant::now())?;
                        }
                        Some(Command::GiveInventoryFolder { to_agent_id, folder_id, folder_name, transaction_id }) => {
                            self.session.give_inventory_folder(to_agent_id, folder_id, &folder_name, transaction_id, Instant::now())?;
                        }
                        Some(Command::AcceptInventoryOffer { offer, folder_id }) => {
                            self.session.accept_inventory_offer(&offer, folder_id, Instant::now())?;
                        }
                        Some(Command::DeclineInventoryOffer { offer, trash_folder_id }) => {
                            self.session.decline_inventory_offer(&offer, trash_folder_id, Instant::now())?;
                        }
                        Some(Command::StartConference { session_id, invitees, message }) => {
                            self.session.start_conference(session_id, &invitees, &message, Instant::now())?;
                        }
                        Some(Command::SendConferenceMessage { session_id, message }) => {
                            self.session.send_conference_message(session_id, &message, Instant::now())?;
                        }
                        Some(Command::LeaveConference { session_id }) => {
                            self.session.leave_conference(session_id, Instant::now())?;
                        }
                        Some(Command::RetrieveInstantMessages) => {
                            self.session.retrieve_instant_messages(Instant::now())?;
                        }
                        Some(Command::RequestOfflineMessages) => {
                            if let Some(url) = caps.get(CAP_READ_OFFLINE_MSGS).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_READ_OFFLINE_MSGS, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Logout) | None => {
                            self.session.initiate_logout(Instant::now());
                        }
                    }
                }
                () = &mut sleep => {
                    self.session.handle_timeout(Instant::now());
                }
            }
        }
    }
}

/// Aborts a running task handle, if present.
fn abort_task(task: &mut Option<tokio::task::JoinHandle<()>>) {
    if let Some(handle) = task.take() {
        handle.abort();
    }
}

/// Fetches the region's capability map by POSTing the seed with the requested
/// capability names, returning the cap-name → URL map (empty on any failure or
/// if no seed is known yet).
async fn fetch_capabilities(seed: Option<&str>, http: &ReqwestClient) -> HashMap<String, String> {
    let Some(seed_url) = seed else {
        return HashMap::new();
    };
    let result = http
        .post(seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
        .await;
    let Ok(response) = result else {
        return HashMap::new();
    };
    let Ok(text) = response.text().await else {
        return HashMap::new();
    };
    parse_seed_response(&text).unwrap_or_default()
}

/// Spawns the event-queue long-poll task for the `EventQueueGet` capability in
/// `caps`, or `None` if the region did not provide one.
fn spawn_event_queue(
    caps: &HashMap<String, String>,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) -> Option<tokio::task::JoinHandle<()>> {
    let event_queue_url = caps.get("EventQueueGet")?.clone();
    Some(tokio::spawn(run_event_queue(
        event_queue_url,
        http.clone(),
        caps_tx.clone(),
    )))
}

/// POSTs a `FetchInventoryDescendents2` request for `folder_ids` and forwards the
/// LLSD response to `caps_tx` tagged [`CAP_FETCH_INVENTORY`], for the session to
/// decode into [`Event::InventoryDescendents`].
async fn fetch_inventory(
    cap_url: String,
    owner_id: Uuid,
    folder_ids: Vec<Uuid>,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_fetch_inventory_request(owner_id, &folder_ids);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_FETCH_INVENTORY.to_owned(), llsd))
            .await
            .ok();
    }
}

/// POSTs the `GroupMemberData` capability for `group_id`, forwarding the decoded
/// LLSD roster back over `caps_tx` to be surfaced as an [`Event::GroupMembers`].
async fn fetch_group_members(
    cap_url: String,
    group_id: Uuid,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_group_member_data_request(group_id);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_GROUP_MEMBER_DATA.to_owned(), llsd))
            .await
            .ok();
    }
}

/// POSTs an `ObjectMedia` GET for `object_id`, forwarding the decoded LLSD
/// response back over `caps_tx` to be surfaced as an [`Event::ObjectMedia`].
async fn fetch_object_media(
    cap_url: String,
    object_id: Uuid,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_object_media_get_request(object_id);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_OBJECT_MEDIA.to_owned(), llsd)).await.ok();
    }
}

/// POSTs an `ObjectMedia` UPDATE (or, with `navigate`, an `ObjectMediaNavigate`)
/// to set the per-face media of `object_id`. Both are fire-and-forget: the
/// simulator advances the object's media version rather than replying with
/// media, so there is no event to surface — a client re-fetches with
/// [`Command::RequestObjectMedia`] to observe the change.
async fn post_object_media(cap_url: String, body: String, http: ReqwestClient) {
    http.post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
        .ok();
}

/// POSTs a `RenderMaterials` request for `material_ids` (the zipped binary-LLSD
/// form), decoding the zipped reply into the legacy materials and surfacing them
/// as an [`Event::RenderMaterials`]. Best-effort: a transport or decode failure
/// yields an empty list.
async fn fetch_render_materials(
    cap_url: String,
    material_ids: Vec<Uuid>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let body = build_render_materials_request(&material_ids);
    let materials = match http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    {
        Ok(response) => match response.text().await {
            Ok(text) => parse_render_materials_response(&text),
            Err(_error) => Vec::new(),
        },
        Err(_error) => Vec::new(),
    };
    events.send(Event::RenderMaterials(materials)).await.ok();
}

/// POSTs a `ModifyMaterialParams` request, forwarding the `{ success, message }`
/// reply back over `caps_tx` to be surfaced as an [`Event::MaterialParamsResult`].
async fn post_modify_material_params(
    cap_url: String,
    body: String,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_MODIFY_MATERIAL_PARAMS.to_owned(), llsd))
            .await
            .ok();
    }
}

/// POSTs a voice-signalling capability (`ProvisionVoiceAccountRequest` or
/// `ParcelVoiceInfoRequest`) carrying the prepared `body`, forwarding the LLSD
/// reply back over `caps_tx` tagged with `cap` so the session decodes it into
/// the matching event ([`Event::VoiceAccountProvisioned`] /
/// [`Event::ParcelVoiceInfo`]). Only the grid signalling is handled here; the
/// audio session is out of scope.
async fn post_voice_cap(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
async fn post_voice_signaling(cap_url: String, body: String, http: ReqwestClient) {
    http.post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
        .ok();
}

/// GETs `url` and parses the LLSD-XML reply, returning `None` on any
/// transport/parse failure. Shared by the experience capability fetches.
async fn get_llsd(url: &str, http: &ReqwestClient) -> Option<Llsd> {
    let response = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
        .ok()?;
    let text = response.text().await.ok()?;
    parse_llsd_xml(&text).ok()
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) into the
/// matching experience event.
async fn get_caps_llsd(
    url: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// PUTs `body` to an experience capability URL (the `Allow`/`Block` preference
/// set) and forwards the LLSD reply to `caps_tx` tagged `cap`.
async fn put_caps_llsd(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .put(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// Sends an HTTP PATCH of `body` to an AIS3 inventory capability URL (a folder /
/// item update or move) and forwards the LLSD reply to `caps_tx` tagged `cap`.
async fn patch_caps_llsd(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .patch(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// Sends an HTTP DELETE to an experience capability URL (the `Forget`
/// preference) and forwards the LLSD reply to `caps_tx` tagged `cap`.
async fn delete_caps_llsd(
    cap_url: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .delete(&cap_url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// GETs the `GroupExperiences` capability and forwards an [`Event::GroupExperiences`]
/// over `events`, echoing the queried `group_id` (the cap reply does not carry it).
async fn fetch_group_experiences(
    url: String,
    group_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        events
            .send(Event::GroupExperiences {
                group_id,
                experience_ids: parse_experience_ids(&llsd),
            })
            .await
            .ok();
    }
}

/// GETs the `IsExperienceAdmin` capability and forwards an
/// [`Event::ExperienceAdminStatus`] over `events`, echoing the queried experience.
async fn fetch_experience_admin(
    url: String,
    experience_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        events
            .send(Event::ExperienceAdminStatus {
                experience_id,
                is_admin: parse_experience_status(&llsd),
            })
            .await
            .ok();
    }
}

/// GETs the `IsExperienceContributor` capability and forwards an
/// [`Event::ExperienceContributorStatus`] over `events`, echoing the queried
/// experience.
async fn fetch_experience_contributor(
    url: String,
    experience_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        events
            .send(Event::ExperienceContributorStatus {
                experience_id,
                is_contributor: parse_experience_status(&llsd),
            })
            .await
            .ok();
    }
}

/// POSTs the `UpdateAvatarAppearance` capability for `cof_version` (the modern
/// Second Life server-side bake), forwarding the LLSD reply back over `caps_tx`
/// to be surfaced as an [`Event::ServerAppearanceUpdate`]. The baked appearance
/// itself arrives separately as a UDP [`Event::AvatarAppearance`].
async fn request_server_appearance_update(
    cap_url: String,
    cof_version: i32,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_update_avatar_appearance_request(cof_version);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd))
            .await
            .ok();
    }
}

/// Runs the modern two-step CAPS asset upload: POST the LLSD `metadata` to the
/// capability `cap_url` to obtain an `uploader` URL, then POST the raw `data`
/// bytes there. Surfaces the outcome as [`Event::AssetUploaded`] on success or
/// [`Event::AssetUploadFailed`] on any failure. Shared by the
/// `NewFileAgentInventory`, `UploadBakedTexture`, and `Update*AgentInventory`
/// uploads, whose responses share the `{ state, uploader, new_asset,
/// new_inventory_item }` shape.
async fn run_caps_upload(
    cap_url: String,
    metadata: String,
    data: Vec<u8>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = caps_upload_event(&cap_url, metadata, data, &http).await;
    events.send(event).await.ok();
}

/// Performs both steps of a CAPS asset upload and returns the resulting event.
async fn caps_upload_event(
    cap_url: &str,
    metadata: String,
    data: Vec<u8>,
    http: &ReqwestClient,
) -> Event {
    // Step 1: POST the metadata, expecting an `uploader` URL back.
    let uploader = match caps_upload_step(
        http,
        cap_url,
        "application/llsd+xml",
        metadata.into_bytes(),
    )
    .await
    {
        Ok(response) => match response.uploader {
            Some(url) => url,
            None => {
                return Event::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return Event::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw asset bytes to the uploader URL.
    match caps_upload_step(http, &uploader, "application/octet-stream", data).await {
        Ok(response) => match response.new_asset {
            Some(new_asset) => Event::AssetUploaded {
                new_asset,
                new_inventory_item: response.new_inventory_item,
            },
            None => Event::AssetUploadFailed {
                reason: response.error.unwrap_or_else(|| {
                    format!("upload did not complete (state {})", response.state)
                }),
            },
        },
        Err(reason) => Event::AssetUploadFailed { reason },
    }
}

/// POSTs one step of a CAPS upload and parses the LLSD response, returning the
/// parsed [`AssetUploadResponse`] or a human-readable failure reason.
async fn caps_upload_step(
    http: &ReqwestClient,
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<sl_proto::AssetUploadResponse, String> {
    let response = http
        .post(url)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .await
        .map_err(|error| format!("upload request failed: {error}"))?;
    let text = response
        .text()
        .await
        .map_err(|error| format!("upload response read failed: {error}"))?;
    parse_asset_upload_response(&text)
        .map_err(|error| format!("upload response parse failed: {error}"))
}

/// GETs a texture from the `GetTexture` capability and surfaces it as an
/// [`Event::TextureReceived`], or an [`Event::TextureNotFound`] on a 404 /
/// network failure.
///
/// For a non-zero `discard_level` this fetches only the level-of-detail prefix
/// using real HTTP `Range` requests (so the rest of the codestream is never
/// transferred): a small first request reads the J2C [`j2c::Header`], from which
/// the prefix byte length is computed, then a second `Range` request fetches
/// exactly that prefix when the first did not already cover it. A server that
/// ignores `Range` (replying `200` with the whole image) still yields the right
/// prefix, just without the bandwidth saving.
async fn fetch_texture_http(
    cap_url: String,
    texture_id: Uuid,
    discard_level: u8,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match fetch_texture_bytes(&http, &url, discard_level).await {
        Some(data) => Event::TextureReceived(Box::new(Texture {
            id: texture_id,
            codec: ImageCodec::J2c,
            data,
        })),
        None => Event::TextureNotFound(texture_id),
    };
    events.send(event).await.ok();
}

/// Fetches the codestream bytes for a texture at `discard_level`, using HTTP
/// `Range` requests to transfer only the needed LOD prefix. Returns `None` on a
/// 404 / network failure.
async fn fetch_texture_bytes(
    http: &ReqwestClient,
    url: &str,
    discard_level: u8,
) -> Option<Vec<u8>> {
    // Full resolution: one plain GET of the entire codestream.
    if discard_level == 0 {
        return http_get_prefix(http, url, None).await;
    }
    // Probe the header with a small Range request, then size the LOD prefix.
    let probe = http_get_prefix(http, url, Some(j2c::FIRST_PACKET_SIZE)).await?;
    let Some(header) = j2c::parse_header(&probe) else {
        // Not a recognisable J2C codestream: return whatever the probe yielded.
        return Some(probe);
    };
    let target = header.discard_data_size(discard_level);
    if probe.len() >= target {
        // The probe already covers the prefix (a coarse LOD, or a server that
        // ignored Range and sent the whole image).
        return Some(probe.get(..target).unwrap_or(&probe).to_vec());
    }
    // Fetch exactly the prefix the LOD needs.
    let body = http_get_prefix(http, url, Some(target)).await?;
    let size = target.min(body.len());
    Some(body.get(..size).unwrap_or(&body).to_vec())
}

/// Performs an HTTP `GET` for `url`, optionally requesting only the first
/// `max_bytes` via a `Range: bytes=0-(max_bytes-1)` header. Returns the response
/// body on a success status (`200` or `206`), or `None` on any failure.
async fn http_get_prefix(
    http: &ReqwestClient,
    url: &str,
    max_bytes: Option<usize>,
) -> Option<Vec<u8>> {
    let mut request = http.get(url).header("Accept", "image/x-j2c");
    if let Some(max) = max_bytes {
        request = request.header("Range", format!("bytes=0-{}", max.saturating_sub(1)));
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|bytes| bytes.to_vec())
}

/// GETs a mesh asset from the `GetMesh2`/`GetMesh` capability and surfaces it as
/// an [`Event::AssetReceived`] (or [`Event::AssetTransferFailed`] on failure).
/// An inclusive `byte_range` issues an HTTP `Range` request for just that span.
async fn fetch_mesh_http(
    cap_url: String,
    mesh_id: Uuid,
    byte_range: Option<(u32, u32)>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}/?mesh_id={mesh_id}");
    let event = http_asset_event(&http, &url, mesh_id, AssetType::Mesh, byte_range).await;
    events.send(event).await.ok();
}

/// GETs a generic asset from the `GetAsset` capability (using the asset class's
/// query parameter) and surfaces it as an [`Event::AssetReceived`] (or
/// [`Event::AssetTransferFailed`] on failure / an unsupported class). An
/// inclusive `byte_range` issues an HTTP `Range` request for just that span.
async fn fetch_asset_http(
    cap_url: String,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = match asset_type.get_asset_query_key() {
        Some(key) => {
            let url = format!("{cap_url}/?{key}={asset_id}");
            http_asset_event(&http, &url, asset_id, asset_type, byte_range).await
        }
        None => Event::AssetTransferFailed {
            asset_id,
            asset_type,
            status: TransferStatus::Error,
        },
    };
    events.send(event).await.ok();
}

/// Performs an HTTP `GET` for an asset and builds the resulting event: an
/// [`Event::AssetReceived`] on success, or an [`Event::AssetTransferFailed`]
/// (with [`TransferStatus::UnknownSource`], the 404 equivalent) on any failure.
/// An inclusive `byte_range` adds a `Range: bytes=start-end` header.
async fn http_asset_event(
    http: &ReqwestClient,
    url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
) -> Event {
    let failed = Event::AssetTransferFailed {
        asset_id,
        asset_type,
        status: TransferStatus::UnknownSource,
    };
    let mut request = http.get(url);
    if let Some((start, end)) = byte_range {
        request = request.header("Range", format!("bytes={start}-{end}"));
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => match response.bytes().await {
            Ok(bytes) => Event::AssetReceived(Box::new(Asset {
                id: asset_id,
                asset_type,
                data: bytes.to_vec(),
            })),
            Err(_error) => failed,
        },
        _ => failed,
    }
}

/// Long-polls the `EventQueueGet` capability at `event_queue_url`, forwarding each
/// decoded event to `caps_tx` until a request fails fatally or the receiver is
/// dropped (e.g. on region change).
async fn run_event_queue(
    event_queue_url: String,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let mut ack: Option<i32> = None;
    loop {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_error) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        let Ok(text) = response.text().await else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).await.is_err() {
                return;
            }
        }
    }
}

/// Builds a sleep future firing at `deadline`, or far in the future when there
/// is no scheduled timeout.
fn make_sleep(deadline: Option<Instant>) -> tokio::time::Sleep {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)),
        None => tokio::time::sleep(IDLE_SLEEP),
    }
}
