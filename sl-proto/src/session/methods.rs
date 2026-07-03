//! The driver-facing `Session` API: login, UDP/CAPS dispatch, and command methods.

use super::conversions::{
    OutgoingIm, ZERO_VECTOR, active_group, agent_drop_group_from_llsd,
    agent_list_voice_updates_from_llsd, agent_state_update_from_llsd,
    ais_inventory_update_from_llsd, avatar_animations, avatar_appearance, avatar_group,
    avatar_interests, avatar_names, avatar_properties, bulk_update_folder,
    bulk_update_inventory_from_llsd, bulk_update_item, chat_message, chat_session_roster_from_llsd,
    chatterbox_invitation_from_llsd, classified_info, created_category_from_llsd,
    crossed_region_from_caps_llsd, display_name_update_from_llsd, economy_data,
    enable_simulator_from_caps_llsd, environment_from_llsd,
    establish_agent_communication_from_llsd, estate_access_from_params, estate_info_from_params,
    friend, grid_coordinates_from_handle, group_account_details, group_account_summary,
    group_account_transactions, group_active_proposal_item, group_member,
    group_members_from_caps_llsd, group_membership, group_memberships_from_caps_llsd, group_names,
    group_notice, group_profile, group_role, group_title, group_vote_history_item, index_into,
    instant_message, inventory_descendents_from_llsd, inventory_folder, inventory_item,
    inventory_item_from_create, inventory_offer_bucket, invite_channel_from_llsd, map_item,
    map_layer, map_region_info, money_balance, nav_mesh_status_from_llsd, neighbor_info,
    object_from_full_update, object_properties, offline_messages_from_llsd,
    open_region_info_from_llsd, pack_uuids, parcel_info, parcel_info_from_llsd,
    parse_lure_region_handle, parse_mute_list, parse_task_inventory, parse_uuid_string, pick_info,
    region_identity, region_limits, required_voice_version_from_llsd, script_dialog,
    script_permission_request, script_running_from_caps_llsd, server_appearance_update_from_llsd,
    set_display_name_reply_from_llsd, sim_console_response_from_llsd, skeleton_folder,
    teleport_finish_from_llsd, trimmed_string, voice_channel_info_from_llsd,
    windlight_refresh_from_llsd,
};
use super::{
    AGENT_UPDATE_INTERVAL, CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES, CAP_ATTACHMENT_RESOURCES,
    CAP_CHAT_SESSION_REQUEST, CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES,
    CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES,
    CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_OBJECT_COST, CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_LAND_RESOURCES, CAP_LIBRARY_API_V3,
    CAP_MODIFY_MATERIAL_PARAMS, CAP_OBJECT_MEDIA, CAP_PARCEL_VOICE_INFO,
    CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES,
    CAP_REMOTE_PARCEL_REQUEST, CAP_RESOURCE_COST_SELECTED, CAP_SIMULATOR_FEATURES,
    CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, ChatLifecycleView, ChatSession,
    ChatSessionInfo, ChatSessionKind, ChatSessionLifecycle, Circuit, DEFAULT_DRAW_DISTANCE,
    FolderState, FriendPresence, GrantStatus, HolderKind, IDENTITY_ROTATION, Inventory,
    InventoryOwner, LAND_RESOURCE_DETAIL_TAG, LAND_RESOURCE_SUMMARY_TAG, LOGOUT_TIMEOUT,
    MessageCursor, PING_INTERVAL, PendingInvite, SIT_TIMEOUT, ScriptGrant, ScriptHolder, Session,
    SessionMessage, SessionState, SitState, TELEPORT_TIMEOUT, TYPING_TIMEOUT, TakenControls,
    TeleportPhase, TextureDownload, VoiceChannelInfo, XferDownload, XferPurpose, deadline,
    merge_deadline,
};
use crate::GroupRoleKey;
use crate::asset_keys::{AnimationKey, AssetKey};
use crate::bookkeeping_ids::{
    GroupRequestId, ImSessionId, InventoryCallbackId, InvoiceId, LureId, PingId, QueryId,
    TransactionId, XferId,
};
use crate::error::Error;
use crate::scoped_id::{CircuitId, ScopedObjectId, ScopedParcelId};
use crate::terrain;
use crate::types::EventId;
use crate::types::{
    AlertInfo, AssetType, AttachmentMode, AttachmentPoint, AvatarClassified, AvatarPick,
    AvatarPickerResult, Camera, ChatType, Child, ClassifiedCategory, ClassifiedUpdate, ClickAction,
    CoarseLocation, CreateGroupParams, DeRezDestination, DetachOrder, Diagnostic,
    DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult, DirLandResult,
    DirPeopleResult, DirPlaceResult, DirectoryVisibility, DisconnectReason, EjectAction,
    EstateAccessDelta, EstateCovenant, Event, EventInfo, FeatureDisabled, FolderInfo, FolderType,
    FollowCamProperty, FollowCamPropertyValue, FreezeAction, Friend, FriendRights, GenericMessage,
    GenericStreamingMessage, GestureActivation, GodRegionUpdate, GroupNoticeAttachment,
    GroupNoticeKey, GroupRoleEdit, GroupRoleMember, GroupRoleMemberChange, ImDialog, ImageCodec,
    InterestsUpdate, InventoryCursor, InventoryFolder, InventoryItem, InventoryItemMove,
    InventoryOffer, ItemInfo, Kick, LandEdit, LandSearchType, LandStatItem, LandStatReportType,
    LoadUrlRequest, LoginAccount, LoginHttpRequest, LoginParams, MapItemType, Material, Maturity,
    MeanCollision, MeanCollisionType, MoneyTransactionType, MovementMode, MuteFlags, MuteType,
    NeighborInfo, NewInventoryItem, NewInventoryLink, NotecardRez, Object, ObjectBuyItem,
    ObjectExtraParams, ObjectFlagSettings, ObjectPlayingAnimation, ObjectPropertiesFamily,
    ObjectTransform, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory,
    ParcelDetails, ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo,
    ParcelReturnType, ParcelUpdate, PermissionField, PickKey, PickUpdate, PlacesResult, Postcard,
    PrimShape, PrimShapeParams, ProfileUpdate, ProposalVoteId, RegionInfoUpdate, RegionStats,
    Reliability, RestoreItem, RezAttachment, RezObjectParams, RezScriptParams, SaleType,
    ScriptControl, ScriptControlAction, ScriptControlsInfo, ScriptGrantInfo, ScriptLanguage,
    ScriptPermissionState, ScriptPermissionStatus, ScriptPermissions, ScriptTeleportRequest,
    ServerError, SimStatId, SimWideDeleteFlags, SimulatorTime, SoundFlags, SoundPreload,
    StartLocationSlot, TaskInventoryKey, TaskInventoryReply, TelehubInfo, TeleportFlags,
    TerrainLayerType, TerrainPatch, Texture, TextureEntry, Throttle, Transmit,
    UpdateGroupInfoParams, UserInfo, ViewerEffect, ViewerEffectData, ViewerEffectType, Wearable,
    WearableType,
};
use sl_types::chat::ChatChannel;
use sl_types::key::{
    AgentKey, ClassifiedKey, ExperienceKey, FriendKey, GroupKey, InventoryFolderKey, InventoryKey,
    ObjectKey, OwnerKey, ParcelKey, TextureKey,
};
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::{Distance, GridCoordinates, RegionCoordinates};
use sl_types::money::LindenAmount;
use sl_wire::{
    AbuseReport, AnyMessage, CircuitCode, ControlFlags, GLTF_MATERIAL_OVERRIDE_METHOD, Llsd,
    MessageId, ObjectMediaResponse, PacketFlags, ParcelVoiceInfo, Permissions, Permissions5,
    Reader, RegionHandle, RegionLocalObjectId, RegionLocalParcelId, SequenceNumber,
    VoiceAccountInfo, WireError, build_group_notice_bucket, build_login_request, message_name,
    parse_agent_preferences, parse_attachment_resources, parse_datagram, parse_display_names,
    parse_experience_ids, parse_experience_infos, parse_experience_permissions,
    parse_get_object_cost, parse_get_object_physics_data, parse_gltf_material_override,
    parse_land_resource_detail, parse_land_resource_summary, parse_land_resources_reply,
    parse_object_physics_properties, parse_region_experiences, parse_remote_parcel_reply,
    parse_resource_cost_selected, parse_simulator_features, zero_decode,
};
use sl_wire::{Direction, GlobalCoordinates};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
use uuid::Uuid;

/// The maximum number of ids packed into a single `UUIDNameRequest` /
/// `UUIDGroupNameRequest`. Each id is 16 bytes; 80 keeps the datagram (plus its
/// header and block count) comfortably within a typical UDP MTU.
const UUID_NAMES_PER_REQUEST: usize = 80;

/// Splits a slice of [`ScopedObjectId`]s into the single [`CircuitId`] they all
/// belong to and the bare region-local ids, ready for a batch request (which
/// targets one simulator). Returns `Ok(None)` for an empty slice (a no-op), or
/// [`Error::MixedCircuits`] if the ids are not all scoped to the same circuit.
fn split_scoped_object_ids(
    scoped: &[ScopedObjectId],
) -> Result<Option<(CircuitId, Vec<RegionLocalObjectId>)>, Error> {
    let Some(first) = scoped.first() else {
        return Ok(None);
    };
    let circuit = first.circuit;
    let mut ids = Vec::with_capacity(scoped.len());
    for entry in scoped {
        if entry.circuit != circuit {
            return Err(Error::MixedCircuits);
        }
        ids.push(entry.id);
    }
    Ok(Some((circuit, ids)))
}

/// Yields each set bit of `controls` as its own single-bit mask (e.g. `0b1010`
/// yields `0b10` then `0b1000`), low bit first. Used to fold a control bitfield
/// into the per-bit taken-controls counts without raw indexing (clippy-clean),
/// replacing the viewer's `for i in 0..TOTAL_CONTROLS { if controls & (1<<i) }`.
fn iter_bits(controls: ControlFlags) -> impl Iterator<Item = u32> {
    let mut remaining = controls.bits();
    core::iter::from_fn(move || {
        if remaining == 0 {
            return None;
        }
        let bit = remaining & remaining.wrapping_neg();
        remaining &= !bit;
        Some(bit)
    })
}

/// Maps an IM session id + `from_group` flag (the two fields of a
/// [`ConferenceInvited`](Event::ConferenceInvited) the driver is answering) to the
/// typed [`ChatSessionKind`] key: a group IM session reinterprets the id as a
/// group id, an ad-hoc conference keeps it as the conference id. Shared by the
/// accept / decline invite methods.
fn invite_session_kind(session_id: ImSessionId, from_group: bool) -> ChatSessionKind {
    if from_group {
        ChatSessionKind::Group {
            group_id: GroupKey::from(session_id.get()),
        }
    } else {
        ChatSessionKind::Conference { id: session_id }
    }
}

impl Session {
    /// Creates a new session for the given login parameters.
    #[must_use]
    pub const fn new(login: LoginParams) -> Self {
        Self {
            login,
            state: SessionState::New,
            circuit: None,
            children: BTreeMap::new(),
            child_seeds: BTreeMap::new(),
            next_circuit_id: 1,
            draw_distance: DEFAULT_DRAW_DISTANCE,
            controls: ControlFlags::empty(),
            throttle: None,
            body_rotation: IDENTITY_ROTATION,
            head_rotation: IDENTITY_ROTATION,
            camera: Camera::region_center(),
            sit: SitState::NotSitting,
            teleport: TeleportPhase::Idle,
            script_grants: BTreeMap::new(),
            taken_controls: TakenControls {
                consumed: BTreeMap::new(),
                passed_on: BTreeMap::new(),
            },
            friends: BTreeMap::new(),
            online: BTreeSet::new(),
            chat_sessions: BTreeMap::new(),
            seed_capability: None,
            login_account: None,
            xfer_downloads: BTreeMap::new(),
            next_xfer_id: XferId(1),
            pending_task_inventory: BTreeSet::new(),
            pending_task_inventory_unresolved: VecDeque::new(),
            texture_downloads: BTreeMap::new(),
            objects: BTreeMap::new(),
            terrain: BTreeMap::new(),
            regions: BTreeMap::new(),
            time_dilation: BTreeMap::new(),
            own_avatar: BTreeMap::new(),
            inventory: Inventory::new(),
            background_inventory_fetch: false,
            events: VecDeque::new(),
            diagnostics_enabled: false,
            diagnostics: VecDeque::new(),
        }
    }

    /// Enables or disables protocol-diagnostic collection.
    ///
    /// Off by default. While enabled, the session records a [`Diagnostic`] (and
    /// emits `tracing` records) at the points where it would otherwise silently
    /// drop inbound data: undecodable datagrams, unhandled messages, and unknown
    /// or malformed CAPS events. Disabling clears any already-queued
    /// diagnostics. Drain them with [`Session::poll_diagnostic`].
    pub fn set_diagnostics(&mut self, enabled: bool) {
        self.diagnostics_enabled = enabled;
        if !enabled {
            self.diagnostics.clear();
        }
    }

    /// Whether protocol-diagnostic collection is currently enabled.
    #[must_use]
    pub const fn diagnostics_enabled(&self) -> bool {
        self.diagnostics_enabled
    }

    /// The next pending [`Diagnostic`], if any. Always `None` unless diagnostics
    /// were enabled with [`Session::set_diagnostics`].
    pub fn poll_diagnostic(&mut self) -> Option<Diagnostic> {
        self.diagnostics.pop_front()
    }

    /// Queues `diagnostic` for the driver when diagnostics are enabled; a no-op
    /// otherwise (so the silent-drop sites stay free on the normal path).
    fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics_enabled {
            self.diagnostics.push_back(diagnostic);
        }
    }

    /// Records that a recognised CAPS event named `message` arrived but its LLSD
    /// body failed to parse into the expected shape, for a legacy decoder that
    /// reports no specific cause. Logs a warning and queues a
    /// [`Diagnostic::CapsDecodeFailed`] with no `reason`.
    fn caps_decode_failed(&mut self, message: &str) {
        tracing::warn!(event = message, "CAPS event body failed to parse");
        self.push_diagnostic(Diagnostic::CapsDecodeFailed {
            message: message.to_owned(),
            reason: None,
        });
    }

    /// Like [`caps_decode_failed`](Self::caps_decode_failed) but records the
    /// specific [`WireError`] that rejected the body (which field was missing or
    /// malformed), logging it loudly and carrying it in the diagnostic's
    /// `reason` so a drop can be debugged.
    fn caps_decode_error(&mut self, message: &str, error: &WireError) {
        tracing::warn!(
            event = message,
            error = %error,
            "CAPS event body failed to parse"
        );
        self.push_diagnostic(Diagnostic::CapsDecodeFailed {
            message: message.to_owned(),
            reason: Some(error.to_string()),
        });
    }

    /// Sets the draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    /// A larger value makes the simulator enable more neighbouring regions
    /// (surfaced as [`Event::NeighborDiscovered`]). Takes effect on the next
    /// keep-alive, including for the current circuit.
    pub fn set_draw_distance(&mut self, draw_distance: Distance) {
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.draw_distance = draw_distance.clone();
        }
        self.draw_distance = draw_distance;
    }

    /// The current region's capability-seed URL, once login (or a teleport) has
    /// provided one. The driver POSTs this to obtain the capability map and the
    /// `EventQueueGet` URL. It changes on each region change.
    #[must_use]
    pub const fn seed_capability(&self) -> Option<&url::Url> {
        self.seed_capability.as_ref()
    }

    /// Feeds a parsed CAPS response into the session, surfacing any recognised
    /// payload. Handles `ParcelProperties` and `TeleportFinish` (delivered over
    /// the event queue, not UDP) and [`CAP_FETCH_INVENTORY`] (the LLSD response to
    /// a `FetchInventoryDescendents2` POST the driver performed on the client's
    /// behalf), surfaced as [`Event::InventoryDescendents`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if a teleport-handover bootstrap packet fails to
    /// encode.
    pub fn handle_caps_event(
        &mut self,
        message: &str,
        body: &Llsd,
        now: Instant,
    ) -> Result<(), Error> {
        tracing::trace!(event = message, "inbound CAPS event");
        match message {
            "ParcelProperties" => {
                if let Some(parcel) = parcel_info_from_llsd(body) {
                    self.events
                        .push_back(Event::ParcelProperties(Box::new(parcel)));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            CAP_EXT_ENVIRONMENT => {
                if let Some(environment) = environment_from_llsd(body) {
                    self.events
                        .push_back(Event::Environment(Box::new(environment)));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // A task script's run state, answered over the event queue when the
            // region has one (OpenSim's default, and modern SL) in place of the
            // UDP `ScriptRunningReply`, in response to a `GetScriptRunning`.
            "ScriptRunningReply" => {
                if let Some((object_id, item_id, running)) = script_running_from_caps_llsd(body) {
                    self.events.push_back(Event::ScriptRunning {
                        object_id,
                        item_id,
                        running,
                    });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            "TeleportFinish" => {
                if let Some(finish) = teleport_finish_from_llsd(body) {
                    let region_handle = match self.teleport {
                        TeleportPhase::Requested { target } => target,
                        TeleportPhase::Idle | TeleportPhase::Handover { .. } => RegionHandle(0),
                    };
                    if matches!(self.state, SessionState::Teleporting) {
                        self.events.push_back(Event::TeleportFinished {
                            region_handle,
                            sim: finish.dest,
                            maturity: Maturity::from_sim_access(finish.sim_access),
                            flags: TeleportFlags(finish.teleport_flags),
                        });
                    }
                    self.begin_handover(finish.dest, region_handle, Some(finish.seed), now)?;
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // A neighbouring region is announced over the CAPS event queue (the
            // modern path; OpenSim does not use the UDP `EnableSimulator`). Open a
            // child-agent circuit so it holds the agent's presence before a
            // crossing.
            "EnableSimulator" => {
                if let Some((handle, sim)) = enable_simulator_from_caps_llsd(body) {
                    let handle = RegionHandle(handle);
                    self.open_child_circuit(sim, now)?;
                    if let Some(circuit_id) = self.circuit_id_for(sim) {
                        self.regions.insert(circuit_id, handle);
                    }
                    self.events
                        .push_back(Event::NeighborDiscovered(NeighborInfo {
                            region_handle: handle,
                            sim,
                            grid_coordinates: grid_coordinates_from_handle(handle),
                        }));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // A neighbouring region's child-agent seed capability, sent after we
            // open the child circuit; cache it for when the child is promoted to
            // root on a border crossing.
            "EstablishAgentCommunication" => {
                if let Some((sim, seed)) = establish_agent_communication_from_llsd(body) {
                    self.child_seeds.insert(sim, seed.clone());
                    // Surface the seed so the driver POSTs it: OpenSim only streams
                    // a region's scene to the (child) agent once its capabilities
                    // have been requested (`SentSeeds`), so this unlocks neighbour
                    // object streaming on the child circuit.
                    self.events.push_back(Event::NeighborSeed {
                        sim,
                        seed_capability: seed,
                    });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The agent has physically crossed a region border; OpenSim signals
            // the handover over the CAPS event queue (not the UDP `CrossedRegion`).
            // Promote the pre-opened child circuit for the destination to root.
            "CrossedRegion" if matches!(self.state, SessionState::Active) => {
                if let Some((handle, dest, seed)) = crossed_region_from_caps_llsd(body) {
                    self.promote_child_to_root(dest, RegionHandle(handle), Some(seed), now)?;
                } else {
                    self.caps_decode_failed(message);
                }
            }
            CAP_FETCH_INVENTORY | CAP_FETCH_LIBRARY => {
                for event in inventory_descendents_from_llsd(body) {
                    if let Event::InventoryDescendents {
                        folder_id,
                        version,
                        folders,
                        items,
                        ..
                    } = &event
                    {
                        // Route the reply into the tree its target folder belongs
                        // to — agent or Library — so a `FetchLibDescendents2`
                        // response folds under the Library owner.
                        let owner = self.inventory_reply_owner(*folder_id);
                        self.cache_inventory(folders, items, owner);
                        self.inventory
                            .mark_folder_loaded(*folder_id, *version, owner);
                    }
                    self.events.push_back(event);
                }
            }
            // A `BulkUpdateInventory` the simulator delivers over the CAPS event
            // queue (the modern path OpenSim prefers for copies/gives over the
            // UDP packet). Merge it into the cache like the UDP form.
            "BulkUpdateInventory" => {
                if let Some((transaction_id, folders, items)) =
                    bulk_update_inventory_from_llsd(body)
                {
                    self.cache_inventory(&folders, &items, InventoryOwner::Agent);
                    self.events.push_back(Event::InventoryBulkUpdate {
                        transaction_id,
                        folders,
                        items,
                        item_callbacks: Vec::new(),
                    });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The reply to an AIS3 (`InventoryAPIv3`/`LibraryAPIv3`) REST
            // operation — folders/items it created, updated, or fetched, embedded
            // under `_embedded` (and/or at the top level). Merge into the cache.
            CAP_INVENTORY_API_V3 | CAP_LIBRARY_API_V3 => {
                let owner = if message == CAP_LIBRARY_API_V3 {
                    InventoryOwner::Library
                } else {
                    InventoryOwner::Agent
                };
                let (folders, items) = ais_inventory_update_from_llsd(body);
                if !folders.is_empty() || !items.is_empty() {
                    self.cache_inventory(&folders, &items, owner);
                    self.events.push_back(Event::InventoryBulkUpdate {
                        transaction_id: Uuid::nil(),
                        folders,
                        items,
                        item_callbacks: Vec::new(),
                    });
                }
            }
            // The synchronous reply to a `CreateInventoryCategory` POST:
            // `{ folder_id, name, parent_id, type }` for the new folder.
            CAP_CREATE_INVENTORY_CATEGORY => {
                if let Some(folder) = created_category_from_llsd(body) {
                    self.cache_inventory_folder(folder.clone());
                    self.events.push_back(Event::InventoryBulkUpdate {
                        transaction_id: Uuid::nil(),
                        folders: vec![folder],
                        items: Vec::new(),
                        item_callbacks: Vec::new(),
                    });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The modern (CAPS event-queue) delivery of group memberships; the
            // UDP `AgentGroupDataUpdate` is deprecated on Second Life.
            "AgentGroupDataUpdate" => {
                if let Some(event) = group_memberships_from_caps_llsd(body) {
                    self.events.push_back(event);
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The response to a `GroupMemberData` capability POST (the modern
            // group roster fetch).
            CAP_GROUP_MEMBER_DATA => {
                if let Some(event) = group_members_from_caps_llsd(body) {
                    self.events.push_back(event);
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The reply to an `UpdateAvatarAppearance` POST (server-side baking).
            // The baked result itself arrives separately as a UDP
            // `AvatarAppearance`; this only reports whether the bake request was
            // accepted (and, on a version mismatch, the COF version the server
            // expected, so the client can re-request).
            CAP_UPDATE_AVATAR_APPEARANCE => {
                self.events
                    .push_back(server_appearance_update_from_llsd(body));
            }
            // The reply to an `ObjectMedia` GET: an object's current per-face
            // media (`UPDATE` and the navigate cap have no media-bearing reply —
            // they advance the object's media version instead).
            CAP_OBJECT_MEDIA => match ObjectMediaResponse::from_llsd(body) {
                Ok(response) => self.events.push_back(Event::ObjectMedia {
                    object_id: response.object_id,
                    version: response.version,
                    faces: response.faces,
                }),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `ModifyMaterialParams` POST (setting a GLTF material
            // on object faces): a `{ success, message }` status map.
            CAP_MODIFY_MATERIAL_PARAMS => {
                match (
                    body.field_bool("success", "success"),
                    body.field_str("message", "message"),
                ) {
                    (Ok(success), Ok(field_message)) => {
                        self.events.push_back(Event::MaterialParamsResult {
                            success: success.unwrap_or(false),
                            message: field_message.unwrap_or_default().to_owned(),
                        });
                    }
                    _ => self.caps_decode_failed(message),
                }
            }
            // The reply to a `ProvisionVoiceAccountRequest` POST: either Vivox
            // SIP credentials or a WebRTC JSEP answer. Only the signalling is
            // surfaced; opening the audio session is the caller's concern.
            CAP_PROVISION_VOICE_ACCOUNT => match VoiceAccountInfo::from_llsd(body) {
                Ok(info) => self.events.push_back(Event::VoiceAccountProvisioned(info)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `ParcelVoiceInfoRequest` POST: the parcel's voice
            // channel URI (absent when the parcel has no voice).
            CAP_PARCEL_VOICE_INFO => match ParcelVoiceInfo::from_llsd(body) {
                Ok(Some(info)) => self.events.push_back(Event::ParcelVoiceInfo(info)),
                Ok(None) => self.caps_decode_failed(message),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetDisplayNames` GET: the requested agents' display
            // names (with unresolved ids folded in as `missing` placeholders).
            CAP_GET_DISPLAY_NAMES => match parse_display_names(body) {
                Ok(names) => self.events.push_back(Event::DisplayNames(names)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `RemoteParcelRequest` POST: the grid-wide parcel id
            // covering the requested region location (feeds a `ParcelInfoRequest`).
            CAP_REMOTE_PARCEL_REQUEST => match parse_remote_parcel_reply(body) {
                Ok(Some(parcel_id)) => self.events.push_back(Event::RemoteParcelId(parcel_id)),
                Ok(None) => self.caps_decode_failed(message),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `SimulatorFeatures` GET: the region's feature flags
            // and limits (with the OpenSim-only grid extras folded in when present).
            CAP_SIMULATOR_FEATURES => match parse_simulator_features(body) {
                Ok(features) => self
                    .events
                    .push_back(Event::SimulatorFeatures(Box::new(features))),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to an `AgentPreferences` POST: the agent's full stored
            // preferences after the (possibly empty) update.
            CAP_AGENT_PREFERENCES => match parse_agent_preferences(body) {
                Ok(preferences) => self
                    .events
                    .push_back(Event::AgentPreferences(Box::new(preferences))),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetObjectCost` POST: the per-object land-impact and
            // physics costs, keyed by object id.
            CAP_GET_OBJECT_COST => match parse_get_object_cost(body) {
                Ok(costs) => self.events.push_back(Event::ObjectCosts(costs)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `ResourceCostSelected` POST: the summed selection
            // costs.
            CAP_RESOURCE_COST_SELECTED => match parse_resource_cost_selected(body) {
                Ok(cost) => self.events.push_back(Event::SelectedResourceCost(cost)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetObjectPhysicsData` POST: the per-object physics
            // material parameters, keyed by object id.
            CAP_GET_OBJECT_PHYSICS_DATA => match parse_get_object_physics_data(body) {
                Ok(data) => self.events.push_back(Event::ObjectPhysicsData(data)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // An `ObjectPhysicsProperties` event-queue push: updated physics
            // material parameters for a batch of objects, keyed by region-local id.
            "ObjectPhysicsProperties" => match parse_object_physics_properties(body) {
                Ok(raw) => {
                    // An event-queue push for the current region: scope each id to
                    // the root circuit.
                    let circuit = self.root_circuit_id().unwrap_or_default();
                    let entries = raw
                        .into_iter()
                        .map(|(id, data)| (ScopedObjectId::new(circuit, id), data))
                        .collect();
                    self.events
                        .push_back(Event::ObjectPhysicsProperties(entries));
                }
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to an `AttachmentResources` GET: the agent's scripted
            // attachments grouped by attachment point, with a resource summary.
            CAP_ATTACHMENT_RESOURCES => match parse_attachment_resources(body) {
                Ok(report) => self
                    .events
                    .push_back(Event::AttachmentResources(Box::new(report))),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `LandResources` POST: the follow-up cap URLs the
            // runtimes then GET (surfacing the summary/detail reports below).
            CAP_LAND_RESOURCES => match parse_land_resources_reply(body) {
                Ok(urls) => self.events.push_back(Event::LandResourcesUrls(urls)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // A `LandResources` `ScriptResourceSummary` follow-up GET: the parcel's
            // resource totals (forwarded by the runtimes under this tag).
            LAND_RESOURCE_SUMMARY_TAG => match parse_land_resource_summary(body) {
                Ok(summary) => self.events.push_back(Event::LandResourceSummary(summary)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // A `LandResources` `ScriptResourceDetails` follow-up GET: the parcel's
            // per-object resource breakdown.
            LAND_RESOURCE_DETAIL_TAG => match parse_land_resource_detail(body) {
                Ok(detail) => self.events.push_back(Event::LandResourceDetail(detail)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetExperienceInfo` GET: the requested experiences'
            // metadata (with unresolved ids folded in as `missing` placeholders).
            CAP_GET_EXPERIENCE_INFO => match parse_experience_infos(body) {
                Ok(infos) => self.events.push_back(Event::ExperienceInfo(infos)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `FindExperienceByName` GET: one page of search hits.
            CAP_FIND_EXPERIENCE_BY_NAME => match parse_experience_infos(body) {
                Ok(infos) => self.events.push_back(Event::ExperienceSearchResults(infos)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetExperiences` GET or an `ExperiencePreferences`
            // PUT/DELETE: the agent's allowed/blocked experiences.
            CAP_GET_EXPERIENCES | CAP_EXPERIENCE_PREFERENCES => {
                match parse_experience_permissions(body) {
                    Ok((allowed, blocked)) => self
                        .events
                        .push_back(Event::ExperiencePermissions { allowed, blocked }),
                    Err(error) => self.caps_decode_error(message, &error),
                }
            }
            // The reply to an `AgentExperiences` GET: experiences the agent owns.
            CAP_AGENT_EXPERIENCES => match parse_experience_ids(body) {
                Ok(ids) => self.events.push_back(Event::OwnedExperiences(ids)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetAdminExperiences` GET: experiences the agent
            // administers.
            CAP_GET_ADMIN_EXPERIENCES => match parse_experience_ids(body) {
                Ok(ids) => self.events.push_back(Event::AdminExperiences(ids)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `GetCreatorExperiences` GET: experiences the agent
            // created.
            CAP_GET_CREATOR_EXPERIENCES => match parse_experience_ids(body) {
                Ok(ids) => self.events.push_back(Event::CreatorExperiences(ids)),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to an `UpdateExperience` POST: the experience's metadata
            // after the edit.
            CAP_UPDATE_EXPERIENCE => match parse_experience_infos(body) {
                Ok(infos) => self.events.push_back(Event::ExperienceUpdated(
                    infos.into_iter().next().unwrap_or_default(),
                )),
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `RegionExperiences` GET or POST: the region's
            // allow/block/trust lists.
            CAP_REGION_EXPERIENCES => match parse_region_experiences(body) {
                Ok((allowed, blocked, trusted)) => {
                    self.events.push_back(Event::RegionExperiences {
                        allowed,
                        blocked,
                        trusted,
                    });
                }
                Err(error) => self.caps_decode_error(message, &error),
            },
            // The reply to a `ReadOfflineMsgs` GET (the modern Second Life
            // offline-IM history, #28): an array of stored instant messages, each
            // surfaced as an offline [`Event::InstantMessageReceived`] (the legacy
            // UDP `RetrieveInstantMessages` path re-delivers them as UDP IMs
            // instead).
            CAP_READ_OFFLINE_MSGS => {
                for im in offline_messages_from_llsd(body) {
                    // A replayed offline IM drains into the 1:1 session keyed by
                    // its sender, logged with the original wire timestamp (only
                    // conversation `Message` dialogs are logged — a stored group
                    // notice is surfaced as an event but not logged to a session).
                    if im.dialog == ImDialog::Message {
                        let peer = im.from_agent_id;
                        self.log_inbound_message(
                            ChatSessionKind::Direct { peer },
                            SessionMessage {
                                sender: peer,
                                dialog: ImDialog::Message,
                                text: im.message.clone(),
                                timestamp: im.timestamp,
                            },
                            now,
                        );
                    }
                    self.events
                        .push_back(Event::InstantMessageReceived(Box::new(im)));
                }
            }
            // A conference / group IM-session invitation delivered over the CAPS
            // event queue (the modern path, #28). Join by sending into the session
            // with [`Session::send_conference_message`].
            "ChatterBoxInvitation" => {
                if let Some(event) = chatterbox_invitation_from_llsd(body) {
                    // Record the invitation as a pending `Invited` chat-session
                    // entry (the registry is the pending-invitation read model)
                    // before surfacing the event unchanged for the driver to act
                    // on. The session is keyed by the group id for a group IM or
                    // the conference id otherwise; the channel(s) are classified
                    // from the body's `instantmessage` / `voice` sub-maps.
                    if let Event::ConferenceInvited {
                        session_id,
                        from_agent_id,
                        from_group,
                        session_name,
                        ..
                    } = &event
                    {
                        let kind = if *from_group {
                            ChatSessionKind::Group {
                                group_id: GroupKey::from(*session_id),
                            }
                        } else {
                            ChatSessionKind::Conference {
                                id: ImSessionId::from(*session_id),
                            }
                        };
                        self.mark_chat_session_invited(
                            kind,
                            PendingInvite {
                                inviter: *from_agent_id,
                                session_name: session_name.clone(),
                                channel: invite_channel_from_llsd(body),
                            },
                            now,
                        );
                        // A voice invitation carries a `voice` body (B8): record
                        // that the session offers voice and decode whatever channel
                        // coordinates the invite supplied. `has_voice` is set from
                        // the body's presence even when the coordinates are empty
                        // (the full coordinates arrive in the accept reply).
                        if let Some(voice) = body.get("voice")
                            && let Some(chat_session) = self.chat_session_get_mut(kind)
                        {
                            chat_session.voice.has_voice = true;
                            chat_session.voice.channel = Some(voice_channel_info_from_llsd(voice));
                        }
                    }
                    self.events.push_back(event);
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The reply to a `ChatSessionRequest` accept/decline POST (#28). The
            // accept reply carries the session's current agent roster, which the
            // runtime tags with the session id + `from_group`; fold it into that
            // session's participants (the modern equivalent of replaying the
            // `SessionAdd` stream). A voice accept reply also carries a
            // `voice_channel_info` block — record the channel coordinates and that
            // the session offers voice (B8). The decline reply and OpenSim's
            // stubbed `<llsd>true</llsd>` carry neither, so this is then a no-op.
            CAP_CHAT_SESSION_REQUEST => {
                let roster = chat_session_roster_from_llsd(body);
                let voice = body.get("voice_channel_info");
                if !roster.is_empty() || voice.is_some() {
                    let session_uuid = body
                        .get("session-id")
                        .and_then(Llsd::as_uuid)
                        .unwrap_or_default();
                    let from_group = body
                        .get("from_group")
                        .and_then(Llsd::as_bool)
                        .unwrap_or(false);
                    let kind = if from_group {
                        ChatSessionKind::Group {
                            group_id: GroupKey::from(session_uuid),
                        }
                    } else {
                        ChatSessionKind::Conference {
                            id: ImSessionId::from(session_uuid),
                        }
                    };
                    let session = self.chat_session_mut(kind, now);
                    for agent in roster {
                        session.participants.insert(agent);
                    }
                    if let Some(voice) = voice {
                        session.voice.has_voice = true;
                        session.voice.channel = Some(voice_channel_info_from_llsd(voice));
                    }
                }
            }
            // A `ChatterBoxSessionAgentListUpdates` push (#28): the modern voice
            // participant list for a session we are in. Fold the per-agent voice
            // flag into that session's `voice.members` (B8) — adding the
            // voice-connected, dropping those who left or are text-only. The
            // talk-activity / "is now speaking" flag is out of scope. The session
            // is resolved from the wire id against the existing registry (the
            // event carries no group/conference discriminator); an update for an
            // unknown session is ignored.
            //
            // This is an informational roster push, **not** a join: it folds via
            // the non-promoting [`Self::chat_session_get_mut`] so a still-pending
            // `Invited` session keeps its lifecycle (OpenSim sends such an update
            // alongside the `ChatterBoxInvitation` itself, before we have accepted
            // it). Joining happens on an explicit accept, our own send, or an
            // inbound session message — never on a bare roster update. Activity is
            // still stamped so the update orders the session list.
            "ChatterBoxSessionAgentListUpdates" => {
                if let Some((session_uuid, updates)) = agent_list_voice_updates_from_llsd(body)
                    && let Some(kind) = self.chat_session_kind_for_session_id(session_uuid)
                    && let Some(session) = self.chat_session_get_mut(kind)
                {
                    session.last_activity = now;
                    for (agent, in_voice) in updates {
                        if in_voice {
                            session.voice.has_voice = true;
                            session.voice.members.insert(agent);
                        } else {
                            session.voice.members.remove(&agent);
                        }
                    }
                }
            }
            // A pathfinding agent-state push: whether the agent may currently
            // rebake this region's navmesh (`{ "can_modify_navmesh": bool }`).
            // SL-only.
            "AgentStateUpdate" => {
                if let Some(can_modify_navmesh) = agent_state_update_from_llsd(body) {
                    self.events
                        .push_back(Event::AgentStateUpdate { can_modify_navmesh });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // A pathfinding navmesh-status push: the region's navmesh build
            // state and version. SL-only.
            "NavMeshStatusUpdate" => {
                if let Some(status) = nav_mesh_status_from_llsd(body) {
                    self.events.push_back(Event::NavMeshStatus(status));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The simulator dropped this agent from a group (ejected, group
            // dissolved, …); the client should forget its cached membership.
            "AgentDropGroup" => {
                if let Some(group) = agent_drop_group_from_llsd(body) {
                    self.events
                        .push_back(Event::AgentDroppedFromGroup { group });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // A cached display name changed (for this agent or another). SL-only.
            "DisplayNameUpdate" => {
                if let Some(update) = display_name_update_from_llsd(body) {
                    self.events
                        .push_back(Event::DisplayNameUpdate(Box::new(update)));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The result of this agent's own set-display-name request. SL-only.
            "SetDisplayNameReply" => {
                self.events.push_back(Event::SetDisplayNameReply(Box::new(
                    set_display_name_reply_from_llsd(body),
                )));
            }
            // The simulator asks the client to re-fetch the region's environment
            // (e.g. after an estate-manager windlight change).
            "WindLightRefresh" => {
                self.events.push_back(Event::WindLightRefresh {
                    interpolate: windlight_refresh_from_llsd(body),
                });
            }
            // The text output of a region debug-console command.
            "SimConsoleResponse" => {
                self.events.push_back(Event::SimConsoleResponse {
                    output: sim_console_response_from_llsd(body),
                });
            }
            // The voice protocol version this region requires. SL-only.
            "RequiredVoiceVersion" => {
                self.events.push_back(Event::RequiredVoiceVersion(
                    required_voice_version_from_llsd(body),
                ));
            }
            // OpenSim's extended per-region settings/limits. OpenSim-only.
            "OpenRegionInfo" => {
                self.events
                    .push_back(Event::OpenRegionInfo(Box::new(open_region_info_from_llsd(
                        body,
                    ))));
            }
            _ => {
                tracing::trace!(event = message, "unhandled CAPS event");
                self.push_diagnostic(Diagnostic::UnknownCapsEvent {
                    message: message.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Hands the circuit over to a teleport destination `dest`: retargets the
    /// circuit, sends `UseCircuitCode` + `CompleteAgentMovement` (creating the
    /// child presence then promoting it to root, as a viewer does on
    /// `TeleportFinish`), records the seed capability, and awaits the
    /// destination's handshake / `AgentMovementComplete`. No-op unless a teleport
    /// is in flight.
    fn begin_handover(
        &mut self,
        dest: SocketAddr,
        region_handle: RegionHandle,
        seed_capability: Option<url::Url>,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Teleporting) {
            return Ok(());
        }
        // Retarget synchronously: it resets the circuit's sequence/ack/seen/timer
        // state to the new simulator, after which the source check accepts only
        // the destination. A retarget is a fresh connection to a different region
        // (the destination had no pre-opened child), so mint a new circuit id —
        // the destination's region-local ids are a new space, and any scoped id
        // captured at the source must now fail to resolve.
        let circuit_id = self.mint_circuit_id();
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.id = circuit_id;
            circuit.retarget(dest, now);
            circuit.send_use_circuit_code(now)?;
            circuit.send_complete_agent_movement(now)?;
        }
        // Any child circuits were neighbours of the source region; drop them.
        self.children.clear();
        self.child_seeds.clear();
        // The retargeted root and the dropped neighbours leave their cached
        // objects and terrain stale (new local-id spaces and a new region at the
        // destination); start fresh.
        self.objects.clear();
        self.terrain.clear();
        self.regions.clear();
        self.time_dilation.clear();
        if seed_capability.is_some() {
            self.seed_capability = seed_capability;
        }
        // A teleport unseats the agent: any prior object-sit no longer holds at
        // the destination, so drop it rather than report a stale seat. (A plain
        // region *crossing* — `promote_child_to_root` — keeps the seat, since a
        // vehicle the agent sits on carries it across the border.)
        self.sit = SitState::NotSitting;
        // A real teleport leaves in-world objects behind in the old simulator;
        // drop their permission grants (attachments cross with the avatar).
        self.drop_inworld_grants();
        // Deliberately NOT reset here (nor at any region boundary): `chat_sessions`
        // / `friends` / `online` / `inventory` are grid-level state routed by the
        // grid's IM / group / presence / inventory services, not the region
        // simulator, so they survive every teleport and crossing (the inverse of
        // the region-local `sit` / script grants above). They are seeded empty
        // only in `Session::new` and die solely when the `Session` is dropped —
        // see CHAT_ROADMAP B10/A9 and INVENTORY_ROADMAP A10/B3.
        self.teleport = TeleportPhase::Handover { region_handle };
        self.state = SessionState::AwaitingHandshake;
        Ok(())
    }

    /// Completes the initial login handshake or a teleport handover: arms the
    /// keep-alive `AgentUpdate`, transitions to `Active`, and emits
    /// `RegionHandshakeComplete` (login) or `RegionChanged` (handover). Idempotent
    /// — only acts while still `AwaitingHandshake`, so it may be driven by
    /// whichever of `RegionHandshake` / `AgentMovementComplete` arrives first.
    fn complete_arrival(&mut self, now: Instant) {
        if !matches!(self.state, SessionState::AwaitingHandshake) {
            return;
        }
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
            // Arm the keep-alive ping on the root circuit, matching the reference
            // viewer's periodic circuit ping; the first one goes out one interval
            // from arrival.
            circuit.timers.ping = Some(deadline(now, PING_INTERVAL));
            // Re-advertise the bandwidth throttle on the new root circuit: each
            // region starts with the simulator's conservative defaults until the
            // client tells it otherwise. Best-effort — a wire-encode failure here
            // must not abort arrival.
            if let Some(throttle) = self.throttle {
                let _ignored = circuit.send_agent_throttle(&throttle, now);
            }
        }
        self.state = SessionState::Active;
        match core::mem::replace(&mut self.teleport, TeleportPhase::Idle) {
            TeleportPhase::Handover { region_handle } => {
                if let Some((sim, circuit)) = self.circuit.as_ref().map(|c| (c.sim_addr, c.id)) {
                    self.events.push_back(Event::RegionChanged {
                        region_handle,
                        sim,
                        circuit,
                    });
                }
            }
            TeleportPhase::Idle | TeleportPhase::Requested { .. } => {
                self.events.push_back(Event::RegionHandshakeComplete);
            }
        }
    }

    /// Opens a child-agent circuit to a neighbouring simulator `sim`: a fresh
    /// circuit reusing the agent identity and circuit code, with `UseCircuitCode`
    /// sent but **not** `CompleteAgentMovement` (so it stays a child agent). A
    /// no-op if `sim` is already the root or an existing child, or if there is no
    /// root circuit yet to copy the identity from.
    fn open_child_circuit(&mut self, sim: SocketAddr, now: Instant) -> Result<(), Error> {
        if self.circuit.as_ref().map(|c| c.sim_addr) == Some(sim)
            || self.children.contains_key(&sim)
        {
            return Ok(());
        }
        let Some(root) = self.circuit.as_ref() else {
            return Ok(());
        };
        let (agent_id, session_id, code) = (root.agent_id, root.session_id, root.code);
        let circuit_id = self.mint_circuit_id();
        let mut child = Circuit::new(
            circuit_id,
            sim,
            agent_id,
            session_id,
            code,
            self.draw_distance.clone(),
            now,
        );
        child.send_use_circuit_code(now)?;
        // Advertise the throttle on the child too, so the neighbour opens up its
        // object stream to this child agent (it otherwise uses conservative
        // defaults). Best-effort — a wire-encode failure must not abort.
        if let Some(throttle) = self.throttle {
            let _ignored = child.send_agent_throttle(&throttle, now);
        }
        // Drive the child agent with periodic `AgentUpdate`s (camera/interest) so
        // the neighbour streams its scene objects to this child circuit, the same
        // way the root circuit is kept advertised. Send one immediately and arm
        // the cadence.
        let controls = self.controls.bits();
        let body = self.body_rotation.clone();
        let head = self.head_rotation.clone();
        let _ignored = child.send_agent_update(controls, body, head, &self.camera, now);
        child.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
        // Arm the keep-alive ping on the child circuit too, so the link to the
        // neighbour is measured the same way as the root (the first ping goes out
        // one interval from now).
        child.timers.ping = Some(deadline(now, PING_INTERVAL));
        self.children.insert(sim, child);
        Ok(())
    }

    /// Promotes a child-agent circuit at `dest` to the root after the avatar
    /// crosses a region border (`CrossedRegion`): completes the agent movement so
    /// the neighbour makes us a root agent, swaps it in as the active circuit
    /// (demoting the old root to a child), drops the now-stale neighbour
    /// circuits, records the new seed, and awaits arrival (so `complete_arrival`
    /// emits `RegionChanged`). Falls back to a fresh circuit if no child was
    /// pre-opened.
    fn promote_child_to_root(
        &mut self,
        dest: SocketAddr,
        region_handle: RegionHandle,
        seed: Option<url::Url>,
        now: Instant,
    ) -> Result<(), Error> {
        let Some(root) = self.circuit.as_ref() else {
            return Ok(());
        };
        let (agent_id, session_id, code) = (root.agent_id, root.session_id, root.code);
        // Prefer the seed from `CrossedRegion`; fall back to the one cached from
        // the child's `EstablishAgentCommunication`.
        let seed = seed.or_else(|| self.child_seeds.get(&dest).cloned());
        // A pre-opened child keeps its own circuit id (same connection instance);
        // only the fresh fallback circuit needs a newly minted one.
        let fallback_id = self.mint_circuit_id();
        let mut new_root = self.children.remove(&dest).unwrap_or_else(|| {
            Circuit::new(
                fallback_id,
                dest,
                agent_id,
                session_id,
                code,
                self.draw_distance.clone(),
                now,
            )
        });
        self.child_seeds.remove(&dest);
        new_root.send_complete_agent_movement(now)?;
        // The old root becomes a child agent of the new region. The *other*
        // children stay open: a neighbour of the old region is often also a
        // neighbour of the new one (regions can border on every side), so
        // tearing them down would be wrong. The simulator retires the ones that
        // no longer apply via `DisableSimulator`; any that go silent expire on
        // inactivity, and the new region announces any genuinely new neighbours
        // via `EnableSimulator`.
        let old_root = self.circuit.replace(new_root);
        if let Some(old) = old_root {
            self.children.insert(old.sim_addr, old);
        }
        if seed.is_some() {
            self.seed_capability = seed;
        }
        self.teleport = TeleportPhase::Handover { region_handle };
        self.state = SessionState::AwaitingHandshake;
        Ok(())
    }

    /// The XML-RPC login request the driver must perform, or `None` once login
    /// has already been answered.
    #[must_use]
    pub fn login_http_request(&self) -> Option<LoginHttpRequest> {
        if matches!(self.state, SessionState::New) {
            Some(LoginHttpRequest {
                url: self.login.login_uri.clone(),
                body: build_login_request(&self.login.request),
                user_agent: self.login.request.user_agent(),
            })
        } else {
            None
        }
    }

    /// Feeds back the parsed login response, bootstrapping the circuit on
    /// success or closing the session on failure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SessionClosed`] if the session has already reached its
    /// terminal closed/disconnected state, or [`Error::AlreadyLoggedIn`] if it
    /// is already logged in — login is valid only once, from the freshly
    /// constructed state, so a relogin must use a fresh [`Session`]. Returns
    /// [`Error::Wire`] if a bootstrap packet fails to encode.
    pub fn handle_login_response(
        &mut self,
        response: sl_wire::LoginResponse,
        now: Instant,
    ) -> Result<(), Error> {
        // Login is valid exactly once, from the freshly-constructed `New` state.
        // Any other state means a relogin, which must build a fresh `Session`:
        // a terminal closed session is never revived, and a live one would have
        // its circuit torn down and half-rebuilt, stranding stale per-session
        // state (e.g. `script_grants` / `taken_controls`, which carry no `close`
        // hook precisely because a session is never reused). Reject either way.
        match self.state {
            SessionState::New => {}
            SessionState::Closed => return Err(Error::SessionClosed),
            SessionState::AwaitingHandshake
            | SessionState::Active
            | SessionState::Teleporting
            | SessionState::LoggingOut => return Err(Error::AlreadyLoggedIn),
        }
        match response {
            sl_wire::LoginResponse::Failure(failure) => {
                self.close(DisconnectReason::LoginFailed {
                    reason: failure.reason,
                    message: failure.message,
                });
            }
            // A driver performs the interactive MFA retry during its login
            // phase; if a challenge reaches the session it is treated as a
            // login failure.
            sl_wire::LoginResponse::MfaChallenge(challenge) => {
                self.close(DisconnectReason::LoginFailed {
                    reason: "mfa_challenge".to_owned(),
                    message: challenge.message,
                });
            }
            sl_wire::LoginResponse::Success(success) => {
                let sim_addr = SocketAddr::new(IpAddr::V4(success.sim_ip), success.sim_port);
                let circuit_id = self.mint_circuit_id();
                let mut circuit = Circuit::new(
                    circuit_id,
                    sim_addr,
                    success.agent_id,
                    success.session_id,
                    success.circuit_code,
                    self.draw_distance.clone(),
                    now,
                );
                circuit.send_use_circuit_code(now)?;
                circuit.send_complete_agent_movement(now)?;
                self.circuit = Some(circuit);
                // A fresh session: discard any objects and terrain from a
                // previous login.
                self.objects.clear();
                self.terrain.clear();
                self.regions.clear();
                self.time_dilation.clear();
                // Seed the root region's handle from the login response's global
                // `region_x` / `region_y` so it is known before any object update
                // arrives — in particular for the `RegionHandshake`, which does
                // not itself carry the handle.
                if let (Some(region_x), Some(region_y)) = (success.region_x, success.region_y) {
                    self.regions
                        .insert(circuit_id, RegionHandle::from_global(region_x, region_y));
                }
                self.seed_capability = Some(success.seed_capability.clone());
                self.inventory.set_agent_root(success.inventory_root);
                self.inventory.set_library_root(success.library_root);
                self.inventory
                    .set_library_owner(success.library_owner.map(OwnerKey::Agent));
                self.state = SessionState::AwaitingHandshake;
                self.events.push_back(Event::CircuitEstablished {
                    sim: sim_addr,
                    circuit: circuit_id,
                });
                let account = LoginAccount {
                    home: success.home,
                    look_at: success.look_at,
                    agent_access: Maturity::from_login_access(success.agent_access.as_deref()),
                    agent_access_max: Maturity::from_login_access(
                        success.agent_access_max.as_deref(),
                    ),
                    max_agent_groups: success.max_agent_groups,
                    library_root: success.library_root,
                    library_owner: success.library_owner,
                };
                self.login_account = Some(account.clone());
                self.events.push_back(Event::Account(Box::new(account)));
                if !success.library_skeleton.is_empty() {
                    let library: Vec<InventoryFolder> = success
                        .library_skeleton
                        .iter()
                        .map(skeleton_folder)
                        .collect();
                    // Seed the held model with the Library skeleton under the
                    // `Library` owner: each folder lands `Unknown` carrying its
                    // authoritative skeleton version, queryable and cacheable apart
                    // from the agent tree.
                    for folder in &library {
                        self.inventory
                            .cache_folder(folder.clone(), InventoryOwner::Library);
                    }
                    self.events.push_back(Event::LibraryInventory(library));
                }
                if !success.inventory_skeleton.is_empty() {
                    let folders: Vec<InventoryFolder> = success
                        .inventory_skeleton
                        .iter()
                        .map(skeleton_folder)
                        .collect();
                    // Seed the held inventory model with the skeleton: each
                    // folder lands `Unknown` (contents unfetched) carrying its
                    // authoritative skeleton version, linked into the index.
                    for folder in &folders {
                        self.inventory
                            .cache_folder(folder.clone(), InventoryOwner::Agent);
                    }
                    self.events.push_back(Event::InventorySkeleton(folders));
                }
                if !success.buddy_list.is_empty() {
                    let friends: Vec<Friend> = success.buddy_list.iter().map(friend).collect();
                    // Seed the buddy-list cache from the same `friend()`-mapped
                    // data; `online` stays empty (the buddy list carries rights,
                    // not presence — that arrives later as notifications).
                    self.friends = friends.iter().map(|f| (f.id, *f)).collect();
                    self.events.push_back(Event::FriendList(friends));
                }
            }
        }
        Ok(())
    }

    /// Processes an inbound datagram received from `from` at `now`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if the datagram cannot be parsed or a reply fails
    /// to encode.
    pub fn handle_datagram(
        &mut self,
        from: SocketAddr,
        datagram: &[u8],
        now: Instant,
    ) -> Result<(), Error> {
        if matches!(self.state, SessionState::Closed | SessionState::New) {
            return Ok(());
        }
        // Accept traffic from the root circuit or any open child circuit; ignore
        // anything else.
        let is_root = self.circuit.as_ref().map(|c| c.sim_addr) == Some(from);
        if !is_root && !self.children.contains_key(&from) {
            return Ok(());
        }

        let parsed = parse_datagram(datagram)?;

        let process = {
            let circuit = if is_root {
                self.circuit.as_mut()
            } else {
                self.children.get_mut(&from)
            };
            let Some(circuit) = circuit else {
                return Ok(());
            };
            circuit.note_received(now);
            circuit.record_acks(&parsed.acks);
            if parsed.flags.contains(PacketFlags::RELIABLE) {
                circuit.queue_ack(parsed.sequence, now);
                circuit.mark_seen(parsed.sequence)
            } else {
                true
            }
        };
        if !process {
            return Ok(());
        }

        let decoded;
        let body = if parsed.flags.contains(PacketFlags::ZEROCODED) {
            decoded = zero_decode(parsed.body)?;
            decoded.as_slice()
        } else {
            parsed.body
        };

        let mut reader = Reader::new(body);
        let id = MessageId::decode(&mut reader)?;
        // Unrecognized messages (and bodies that fail to decode) are dropped
        // rather than failing the datagram, but surfaced as a diagnostic.
        let message = match AnyMessage::decode(id, &mut reader) {
            Ok(message) => message,
            Err(error) => {
                let name = message_name(id);
                tracing::warn!(?id, name, %error, "dropping undecodable inbound message");
                self.push_diagnostic(Diagnostic::DecodeFailed {
                    id,
                    name,
                    error,
                    // Capture the (post zero-decode) body only when diagnostics
                    // are on, so the normal path pays nothing.
                    raw: if self.diagnostics_enabled {
                        body.to_vec()
                    } else {
                        Vec::new()
                    },
                    failed_offset: reader.position(),
                });
                return Ok(());
            }
        };
        tracing::trace!(?id, name = message.name(), %from, "inbound message");
        if is_root {
            self.dispatch(from, &message, now)
        } else {
            self.dispatch_child(from, &message, now)
        }
    }

    /// Handles a message that arrived on a child-agent circuit. Children carry
    /// limited traffic; we keep the circuit healthy (ping replies, region
    /// handshake acknowledgement) and otherwise ignore it — the crossing into a
    /// child region is driven by `CrossedRegion` on the root circuit.
    fn dispatch_child(
        &mut self,
        from: SocketAddr,
        message: &AnyMessage,
        now: Instant,
    ) -> Result<(), Error> {
        // A child agent still receives the neighbour region's object stream;
        // cache it so a roaming/proximity bot sees adjacent regions too.
        if self.try_dispatch_object(from, message, now)? {
            return Ok(());
        }
        match message {
            AnyMessage::StartPingCheck(ping) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    circuit.send_complete_ping_check(PingId(ping.ping_id.ping_id), now)?;
                }
            }
            AnyMessage::CompletePingCheck(reply) => {
                // The neighbour's answer to our keep-alive `StartPingCheck` on
                // this child circuit: surface the round-trip time as a
                // child-circuit `Event::Ping`.
                if let Some(rtt) = self.children.get_mut(&from).and_then(|circuit| {
                    circuit.record_ping_reply(PingId(reply.ping_id.ping_id), now)
                }) {
                    self.events.push_back(Event::Ping {
                        sim: from,
                        child: true,
                        rtt,
                    });
                }
            }
            AnyMessage::RegionHandshake(handshake) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    circuit.send_region_handshake_reply(now)?;
                }
                // Surface the neighbour region's identity (terrain textures +
                // elevation bands, flags, maturity, …) just like the root
                // handshake, keyed by this child circuit's region handle, so a
                // viewer can shade neighbour terrain with its own textures.
                let region_handle = self
                    .circuit_id_for(from)
                    .and_then(|circuit_id| self.regions.get(&circuit_id).copied())
                    .unwrap_or(RegionHandle(0));
                self.events
                    .push_back(Event::RegionInfoHandshake(Box::new(region_identity(
                        handshake,
                        region_handle,
                    )?)));
            }
            AnyMessage::PacketAck(ack) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    for packet in &ack.packets {
                        circuit.record_acks(&[SequenceNumber(packet.id)]);
                    }
                }
            }
            AnyMessage::DisableSimulator(_) => {
                // The simulator is retiring this child circuit. Resolve its
                // circuit id before removing it so the per-circuit caches can be
                // dropped.
                let circuit_id = self.circuit_id_for(from);
                self.children.remove(&from);
                self.child_seeds.remove(&from);
                if let Some(circuit_id) = circuit_id {
                    self.forget_sim_objects(circuit_id);
                }
            }
            _ => {
                self.push_diagnostic(Diagnostic::UnhandledMessage {
                    id: message.id(),
                    name: message.name(),
                    child: true,
                });
            }
        }
        Ok(())
    }

    /// Handles the object/scene-graph messages (full / compressed / cached /
    /// terse updates, `KillObject`, `ObjectProperties`) that arrive on the root
    /// *and* child circuits, keyed by the source simulator `from`. Returns `true`
    /// if `message` was an object message (and thus fully handled here).
    fn try_dispatch_object(
        &mut self,
        from: SocketAddr,
        message: &AnyMessage,
        now: Instant,
    ) -> Result<bool, Error> {
        match message {
            AnyMessage::ObjectUpdate(update) => {
                let region_handle = RegionHandle(update.region_data.region_handle);
                self.note_time_dilation(from, region_handle, update.region_data.time_dilation);
                for block in &update.object_data {
                    self.upsert_object(from, object_from_full_update(block, region_handle)?);
                }
            }
            AnyMessage::ObjectUpdateCompressed(update) => {
                let region_handle = RegionHandle(update.region_data.region_handle);
                self.note_time_dilation(from, region_handle, update.region_data.time_dilation);
                for block in &update.object_data {
                    if let Some(object) = crate::object_update::compressed_object(
                        &block.data,
                        region_handle,
                        block.update_flags,
                    ) {
                        self.upsert_object(from, object);
                    }
                }
            }
            AnyMessage::ObjectUpdateCached(update) => {
                self.note_time_dilation(
                    from,
                    RegionHandle(update.region_data.region_handle),
                    update.region_data.time_dilation,
                );
                // We keep no persistent object cache across sessions, so any entry
                // not already held with a matching CRC is a miss; fetch the full
                // update for the misses (a full `ObjectUpdate` follows).
                let cached = self
                    .circuit_id_for(from)
                    .and_then(|circuit_id| self.objects.get(&circuit_id));
                let misses: Vec<RegionLocalObjectId> = update
                    .object_data
                    .iter()
                    .filter(|block| {
                        cached
                            .and_then(|sim| sim.get(&RegionLocalObjectId(block.id)))
                            .is_none_or(|object| object.crc != block.crc)
                    })
                    .map(|block| RegionLocalObjectId(block.id))
                    .collect();
                self.request_object_ids(from, &misses, now);
            }
            AnyMessage::ImprovedTerseObjectUpdate(update) => {
                self.note_time_dilation(
                    from,
                    RegionHandle(update.region_data.region_handle),
                    update.region_data.time_dilation,
                );
                // Terse updates carry only motion. Apply to known objects; for
                // unknown ones (which lack identity here), fetch the full update.
                let mut misses = Vec::new();
                for block in &update.object_data {
                    let Some(terse) = crate::object_update::terse_update(&block.data) else {
                        continue;
                    };
                    let local_id = terse.local_id;
                    let texture_entry =
                        crate::object_update::terse_texture_entry(&block.texture_entry);
                    if !self.apply_terse_update(from, terse, texture_entry) {
                        misses.push(local_id);
                    }
                }
                self.request_object_ids(from, &misses, now);
            }
            AnyMessage::KillObject(kill) => {
                let Some(circuit_id) = self.circuit_id_for(from) else {
                    return Ok(true);
                };
                for block in &kill.object_data {
                    let removed = self
                        .objects
                        .get_mut(&circuit_id)
                        .and_then(|sim| sim.remove(&RegionLocalObjectId(block.id)));
                    let region_handle = removed
                        .as_ref()
                        .map_or(RegionHandle(0), |object| object.region_handle);
                    // The object (or detached attachment, whose detach echoes a
                    // `KillObject`) is gone; drop any permission grants on it.
                    if let Some(full_id) = removed.as_ref().map(|object| object.full_id) {
                        self.script_grants
                            .retain(|holder, _| holder.task_id != full_id);
                    }
                    self.events.push_back(Event::ObjectRemoved {
                        region_handle,
                        local_id: ScopedObjectId::new(circuit_id, RegionLocalObjectId(block.id)),
                    });
                }
            }
            AnyMessage::ObjectProperties(props) => {
                let circuit_id = self.circuit_id_for(from);
                for block in &props.object_data {
                    let properties = object_properties(block)?;
                    if let Some(object) = circuit_id
                        .and_then(|circuit_id| self.objects.get_mut(&circuit_id))
                        .and_then(|sim| {
                            sim.values_mut()
                                .find(|object| object.full_id == properties.object_id)
                        })
                    {
                        object.properties = Some(properties.clone());
                    }
                    self.events
                        .push_back(Event::ObjectProperties(Box::new(properties)));
                }
            }
            AnyMessage::LayerData(layer) => {
                self.dispatch_terrain(from, &layer.layer_data.data);
            }
            // A GLTF (PBR) material override for an object in this sim, pushed as
            // a `GenericStreamingMessage`. Only the override method is ours;
            // other streaming methods are ignored (but still consumed here).
            AnyMessage::GenericStreamingMessage(message)
                if message.method_data.method == GLTF_MATERIAL_OVERRIDE_METHOD =>
            {
                if let Some(decoded) = parse_gltf_material_override(&message.data_block.data) {
                    let circuit_id = self.circuit_id_for(from).unwrap_or_default();
                    let region_handle = self
                        .regions
                        .get(&circuit_id)
                        .copied()
                        .unwrap_or(RegionHandle(0));
                    self.events.push_back(Event::GltfMaterialOverride {
                        region_handle,
                        local_id: ScopedObjectId::new(circuit_id, decoded.local_id),
                        faces: decoded.faces,
                        overrides: decoded.overrides,
                    });
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Decodes a `LayerData` payload received from simulator `from`, caching each
    /// patch (keyed by layer and grid position) and emitting an
    /// [`Event::TerrainPatch`]. Best-effort: a malformed group header is ignored.
    fn dispatch_terrain(&mut self, from: SocketAddr, data: &[u8]) {
        let Some((layer, patches)) = terrain::decode_layer(data) else {
            return;
        };
        let Some(circuit_id) = self.circuit_id_for(from) else {
            return;
        };
        let region_handle = self
            .regions
            .get(&circuit_id)
            .copied()
            .unwrap_or(RegionHandle(0));
        let cache = self.terrain.entry(circuit_id).or_default();
        let mut emit = Vec::with_capacity(patches.len());
        for decoded in patches {
            let patch = terrain::into_terrain_patch(decoded, layer, region_handle);
            cache.insert((layer.code(), patch.patch_x, patch.patch_y), patch.clone());
            emit.push(patch);
        }
        for patch in emit {
            self.events.push_back(Event::TerrainPatch(Box::new(patch)));
        }
    }

    /// Records the `RegionData.TimeDilation` carried by an object-update message
    /// from simulator `from`, emitting [`Event::TimeDilation`] when the raw value
    /// differs from the last one seen for that sim (so a steady region does not
    /// re-emit on every update). `raw` is the 16-bit wire value; the event carries
    /// the `0.0`..=`1.0` fraction.
    fn note_time_dilation(&mut self, from: SocketAddr, region_handle: RegionHandle, raw: u16) {
        let Some(circuit_id) = self.circuit_id_for(from) else {
            return;
        };
        if self.time_dilation.insert(circuit_id, raw) == Some(raw) {
            return;
        }
        self.events.push_back(Event::TimeDilation {
            region_handle,
            dilation: f32::from(raw) / f32::from(u16::MAX),
        });
    }

    /// Inserts or refreshes a scene object in the cache for simulator `from`,
    /// emitting [`Event::ObjectAdded`] for a newly seen local id or
    /// [`Event::ObjectUpdated`] for one already cached. Any previously merged
    /// [`properties`](Object::properties) are preserved across a refresh that
    /// does not carry its own.
    fn upsert_object(&mut self, from: SocketAddr, mut object: Object) {
        let Some(circuit_id) = self.circuit_id_for(from) else {
            return;
        };
        // Stamp the object with the circuit it was learned on so a caller can
        // build a [`ScopedObjectId`] from it (via [`Object::scoped_id`]).
        object.circuit = circuit_id;
        // Remember this circuit's region handle so terrain patches (whose
        // `LayerData` message carries no handle) can be labelled with it.
        if object.region_handle != RegionHandle(0) {
            self.regions.insert(circuit_id, object.region_handle);
        }
        // Record our own avatar's region-local id the first time its object is
        // seen on this circuit, so attachments (objects parented to it) can later
        // be recognised when classifying script-permission holders.
        if object.pcode == crate::types::pcode::AVATAR
            && self
                .agent_id()
                .is_some_and(|agent| agent.uuid() == object.full_id.uuid())
        {
            self.note_own_avatar(circuit_id, object.local_id);
        }
        let sim = self.objects.entry(circuit_id).or_default();
        match sim.get(&object.local_id) {
            Some(existing) => {
                if object.properties.is_none() {
                    object.properties.clone_from(&existing.properties);
                }
                sim.insert(object.local_id, object.clone());
                self.events
                    .push_back(Event::ObjectUpdated(Box::new(object)));
            }
            None => {
                sim.insert(object.local_id, object.clone());
                self.events.push_back(Event::ObjectAdded(Box::new(object)));
            }
        }
    }

    /// Records the agent's own avatar region-local id for circuit `circuit_id`
    /// the first time it is observed (set-once): a region-local id is stable for
    /// the life of a circuit, so a later observation never overwrites it. Fed by
    /// the object-update path ([`Session::upsert_object`]) and the
    /// `AgentMovementComplete` backstop.
    fn note_own_avatar(&mut self, circuit_id: CircuitId, local_id: RegionLocalObjectId) {
        self.own_avatar.entry(circuit_id).or_insert(local_id);
    }

    /// Scans circuit `circuit_id`'s object cache for the agent's own avatar
    /// object — an avatar (`pcode::AVATAR`) whose `full_id` is the agent's id —
    /// returning its region-local id if present. Used by the
    /// `AgentMovementComplete` backstop, which carries no region-local id of its
    /// own, to learn the own-avatar id from an avatar object that was cached
    /// before the slot could be filled.
    fn cached_own_avatar_local_id(&self, circuit_id: CircuitId) -> Option<RegionLocalObjectId> {
        let agent = self.agent_id()?;
        self.objects.get(&circuit_id)?.values().find_map(|object| {
            (object.pcode == crate::types::pcode::AVATAR && object.full_id.uuid() == agent.uuid())
                .then_some(object.local_id)
        })
    }

    /// Finds a cached object by its persistent global id ([`ObjectKey`]),
    /// scanning every circuit's cache (there is no by-`full_id` index; only
    /// nearby objects are cached, so the scan is small). Used by
    /// [`Session::holder_kind`] to classify a script-permission holder.
    fn object_by_full_id(&self, full_id: ObjectKey) -> Option<&Object> {
        self.objects
            .values()
            .flat_map(BTreeMap::values)
            .find(|object| object.full_id == full_id)
    }

    /// Classifies the holder of a script-permission grant from the object cache:
    /// whether the holding object `task_id` is one of *this* agent's attachments
    /// or an in-world object, plus the circuit it was found on (for reset
    /// scoping).
    ///
    /// A holder is an [`HolderKind::Attachment`] iff its cached object is an
    /// attachment ([`Object::attachment_point`] is set) *and* it is parented, on
    /// the same circuit, to our own avatar (the B1.5 cached own-avatar
    /// region-local id). Anything else — an in-world prim, another avatar's
    /// attachment, or a holder not in the cache — is [`HolderKind::InWorld`], the
    /// conservative default (it is then cleared on the next teleport rather than
    /// kept forever).
    fn holder_kind(&self, task_id: ObjectKey) -> (HolderKind, Option<CircuitId>) {
        let Some(object) = self.object_by_full_id(task_id) else {
            return (HolderKind::InWorld, None);
        };
        let circuit = object.circuit;
        let parented_to_us = object.parent_id != RegionLocalObjectId(0)
            && self.own_avatar.get(&circuit) == Some(&object.parent_id);
        let kind = if object.attachment_point().is_some() && parented_to_us {
            HolderKind::Attachment
        } else {
            HolderKind::InWorld
        };
        (kind, Some(circuit))
    }

    /// Drops every in-world script-permission grant, keeping the attachment
    /// grants. Called at the two real-teleport unseat sites (a left-behind
    /// in-world object is in the old simulator and unreachable; an attachment
    /// crosses the border with the avatar). Mirrors the [`SitState`] reset
    /// precedent — only the real-teleport sites call this, not a sit/stand.
    fn drop_inworld_grants(&mut self) {
        self.script_grants
            .retain(|_, grant| matches!(grant.kind, HolderKind::Attachment));
    }

    /// Folds one `ScriptControlChange` block into the session-global
    /// taken-controls tracker: a [`ScriptControlAction::Take`] increments each
    /// named control bit's count, a [`ScriptControlAction::Release`] saturating-
    /// decrements it (the key removed at zero). The `pass_to_agent` flag selects
    /// the map, mirroring the viewer's two counts. A release for an untracked bit
    /// is a no-op (never goes negative). Touches no grant — "permission granted"
    /// (the registry) and "controls currently taken" stay separate concerns.
    fn note_taken_controls(
        &mut self,
        action: ScriptControlAction,
        controls: ControlFlags,
        pass_to_agent: bool,
    ) {
        let counts = if pass_to_agent {
            &mut self.taken_controls.passed_on
        } else {
            &mut self.taken_controls.consumed
        };
        for bit in iter_bits(controls) {
            match action {
                ScriptControlAction::Take => {
                    let count = counts.entry(bit).or_insert(0);
                    *count = count.saturating_add(1);
                }
                ScriptControlAction::Release => {
                    if let Some(count) = counts.get_mut(&bit) {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            counts.remove(&bit);
                        }
                    }
                }
            }
        }
    }

    /// Applies a terse update to an object already cached for simulator `from`,
    /// emitting [`Event::ObjectUpdated`]. Carries the object's new motion and
    /// state, plus the trailing raw `TextureEntry` blob when the simulator flagged
    /// the update with a texture/colour change (`texture_entry`, otherwise `None`,
    /// which leaves the cached entry untouched). Returns `false` if the object is
    /// not cached (the caller should fetch its full update).
    fn apply_terse_update(
        &mut self,
        from: SocketAddr,
        update: crate::object_update::TerseUpdate,
        texture_entry: Option<Vec<u8>>,
    ) -> bool {
        let Some(circuit_id) = self.circuit_id_for(from) else {
            return false;
        };
        let Some(object) = self
            .objects
            .get_mut(&circuit_id)
            .and_then(|sim| sim.get_mut(&update.local_id))
        else {
            return false;
        };
        object.state = update.state;
        object.motion = update.motion;
        if let Some(texture_entry) = texture_entry {
            object.texture_entry = texture_entry;
        }
        let snapshot = object.clone();
        self.events
            .push_back(Event::ObjectUpdated(Box::new(snapshot)));
        true
    }

    /// Sends a `RequestMultipleObjects` (full cache-miss) for the given local ids
    /// on the circuit at `from` (root or child). Best-effort: a missing circuit or
    /// encode failure is ignored (these are speculative fetches driven by the
    /// simulator's stream).
    fn request_object_ids(
        &mut self,
        from: SocketAddr,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) {
        if local_ids.is_empty() {
            return;
        }
        if let Some(circuit) = self.circuit_mut(from) {
            let _ignored = circuit.send_request_multiple_objects(local_ids, now);
        }
    }

    /// Returns a mutable reference to the circuit at `addr`, whether it is the
    /// root or a child circuit.
    fn circuit_mut(&mut self, addr: SocketAddr) -> Option<&mut Circuit> {
        if self.circuit.as_ref().map(|c| c.sim_addr) == Some(addr) {
            self.circuit.as_mut()
        } else {
            self.children.get_mut(&addr)
        }
    }

    /// Mints a fresh [`CircuitId`] for a newly established circuit instance,
    /// advancing the monotonic counter (skipping the zero sentinel on the
    /// astronomically unlikely wrap).
    fn mint_circuit_id(&mut self) -> CircuitId {
        let id = self.next_circuit_id;
        self.next_circuit_id = self.next_circuit_id.checked_add(1).unwrap_or(1);
        CircuitId(id)
    }

    /// The [`CircuitId`] of the circuit at `addr` (root or child), if one is
    /// live. Used to key the per-circuit caches from an inbound message's
    /// source address.
    fn circuit_id_for(&self, addr: SocketAddr) -> Option<CircuitId> {
        if let Some(root) = self.circuit.as_ref()
            && root.sim_addr == addr
        {
            return Some(root.id);
        }
        self.children.get(&addr).map(|child| child.id)
    }

    /// A mutable reference to the live circuit (root or child) with the given
    /// [`CircuitId`], or `None` if no such circuit is established (a stale id
    /// from a torn-down circuit).
    fn circuit_by_id_mut(&mut self, id: CircuitId) -> Option<&mut Circuit> {
        if let Some(root) = self.circuit.as_mut()
            && root.id == id
        {
            return Some(root);
        }
        self.children.values_mut().find(|child| child.id == id)
    }

    /// Resolves the circuit a [`ScopedObjectId`] / [`ScopedParcelId`] is scoped
    /// to, for a send. Returns [`Error::NoCircuit`] when no circuit is
    /// established at all (not logged in — so the existing `# Errors` docs hold),
    /// or [`Error::UnknownCircuit`] when a circuit exists but none matches the
    /// scoped id — i.e. the id is stale (captured on a circuit since torn down by
    /// a teleport, region crossing, relogin, or `DisableSimulator`).
    fn circuit_for_scope(&mut self, circuit: CircuitId) -> Result<&mut Circuit, Error> {
        if self.circuit.is_none() {
            return Err(Error::NoCircuit);
        }
        self.circuit_by_id_mut(circuit).ok_or(Error::UnknownCircuit)
    }

    /// The [`CircuitId`] of the current root circuit, if login has established
    /// one. A driver pairs it with a region-local id to build a
    /// [`ScopedObjectId`] / [`ScopedParcelId`] for the region the agent is in.
    #[must_use]
    pub fn root_circuit_id(&self) -> Option<CircuitId> {
        self.circuit.as_ref().map(|circuit| circuit.id)
    }

    /// The [`RegionHandle`] of the current root region, if login has established
    /// a root circuit. Seeded from the login response's global `region_x` /
    /// `region_y` at login (so it is known before any object update arrives) and
    /// kept current as the root circuit changes region; a driver pairs it with a
    /// region-local position to issue an intra-region
    /// [`Command::Teleport`](crate::Command::Teleport). `None` before login.
    #[must_use]
    pub fn region_handle(&self) -> Option<RegionHandle> {
        self.root_circuit_id()
            .and_then(|circuit| self.regions.get(&circuit).copied())
    }

    /// The agent's **own** avatar object on the current root circuit, as a
    /// [`ScopedObjectId`], once that avatar's object has been observed (its
    /// `ObjectUpdate` cached, or read back at `AgentMovementComplete`); `None`
    /// before then or when not logged in.
    ///
    /// The region-local id is learned per circuit and is stable for the life of
    /// that circuit (a fresh one is assigned in each region). The session uses it
    /// internally to recognise the agent's own attachments — objects parented to
    /// this avatar — when classifying script-permission holders; a driver may
    /// also pair it with the region's circuit to act on its own avatar object.
    #[must_use]
    pub fn own_avatar_id(&self) -> Option<ScopedObjectId> {
        let circuit = self.circuit.as_ref()?;
        self.own_avatar
            .get(&circuit.id)
            .map(|&local_id| ScopedObjectId::new(circuit.id, local_id))
    }

    /// Drops every cached object for the circuit instance `circuit_id` (it has
    /// gone away), emitting an [`Event::ObjectRemoved`] for each so consumers
    /// can prune.
    fn forget_sim_objects(&mut self, circuit_id: CircuitId) {
        // The terrain, region-handle, time-dilation, and own-avatar caches for
        // this circuit go stale too.
        self.terrain.remove(&circuit_id);
        self.regions.remove(&circuit_id);
        self.time_dilation.remove(&circuit_id);
        self.own_avatar.remove(&circuit_id);
        // Drop any permission grants scoped to this retiring (child/neighbour)
        // circuit; the root is never retired this way, so attachment grants
        // (root-scoped) are never dropped here.
        self.script_grants
            .retain(|_, grant| grant.circuit != Some(circuit_id));
        let Some(sim) = self.objects.remove(&circuit_id) else {
            return;
        };
        for object in sim.into_values() {
            self.events.push_back(Event::ObjectRemoved {
                region_handle: object.region_handle,
                local_id: ScopedObjectId::new(circuit_id, object.local_id),
            });
        }
    }

    /// Acts on a decoded inbound message received on the root circuit `from`.
    fn dispatch(
        &mut self,
        from: SocketAddr,
        message: &AnyMessage,
        now: Instant,
    ) -> Result<(), Error> {
        // Object/scene-graph updates arrive on the root *and* child circuits;
        // handle them uniformly, keyed by the source sim.
        if self.try_dispatch_object(from, message, now)? {
            return Ok(());
        }
        match message {
            AnyMessage::RegionHandshake(handshake) => {
                if matches!(self.state, SessionState::AwaitingHandshake) {
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_region_handshake_reply(now)?;
                    }
                    let region_handle = self
                        .circuit_id_for(from)
                        .and_then(|circuit_id| self.regions.get(&circuit_id).copied())
                        .unwrap_or(RegionHandle(0));
                    self.events
                        .push_back(Event::RegionInfoHandshake(Box::new(region_identity(
                            handshake,
                            region_handle,
                        )?)));
                    self.complete_arrival(now);
                }
            }
            AnyMessage::AgentMovementComplete(_) => {
                // After a teleport handover the destination promotes us to root
                // and confirms with AgentMovementComplete; it may not re-send a
                // RegionHandshake, so complete the arrival here too (idempotent).
                self.complete_arrival(now);
                // Backstop the own-avatar id for attachment detection: the
                // message carries no region-local id, so read it from our own
                // avatar object if it was already cached on this circuit.
                if let Some(circuit_id) = self.circuit_id_for(from)
                    && let Some(local_id) = self.cached_own_avatar_local_id(circuit_id)
                {
                    self.note_own_avatar(circuit_id, local_id);
                }
            }
            AnyMessage::RegionInfo(info) => {
                self.events
                    .push_back(Event::RegionLimits(region_limits(info)?));
            }
            AnyMessage::UUIDNameReply(reply) => {
                self.events
                    .push_back(Event::AvatarNames(avatar_names(reply)));
            }
            AnyMessage::UUIDGroupNameReply(reply) => {
                self.events
                    .push_back(Event::GroupNames(group_names(reply)));
            }
            AnyMessage::MoneyBalanceReply(reply) => {
                self.events
                    .push_back(Event::MoneyBalance(money_balance(reply)?));
            }
            AnyMessage::EconomyData(data) => {
                self.events
                    .push_back(Event::EconomyData(Box::new(economy_data(data)?)));
            }
            AnyMessage::ParcelProperties(props) => {
                self.events
                    .push_back(Event::ParcelProperties(Box::new(parcel_info(props)?)));
            }
            AnyMessage::ParcelOverlay(overlay) => {
                self.events
                    .push_back(Event::ParcelOverlay(ParcelOverlayInfo {
                        sequence_id: overlay.parcel_data.sequence_id,
                        data: overlay.parcel_data.data.clone(),
                    }));
            }
            // A scripted parcel-media control (`llParcelMediaCommandList`): the
            // simulator tells viewers to play/pause/stop/loop the parcel's
            // streaming media, or carries a new URL/texture/time/size. Each set
            // bit in `flags` marks a field of this message as meaningful.
            AnyMessage::ParcelMediaCommandMessage(command) => {
                let block = &command.command_block;
                self.events.push_back(Event::ParcelMediaCommand {
                    flags: block.flags,
                    command: ParcelMediaCommand::from_u32(block.command),
                    time: block.time,
                });
            }
            // The parcel's media settings changed (`ParcelMediaUpdate`): the new
            // media URL / texture id / type / dimensions for the streaming media
            // surface. The extended block carries the MIME type and size.
            AnyMessage::ParcelMediaUpdate(update) => {
                let data = &update.data_block;
                let extended = &update.data_block_extended;
                self.events
                    .push_back(Event::ParcelMediaUpdate(ParcelMediaUpdateInfo {
                        media_url: sl_wire::optional_url_from_wire(
                            "MediaURL",
                            &trimmed_string(&data.media_url),
                        )?,
                        media_id: crate::types::optional_key_from_wire(data.media_id),
                        media_auto_scale: data.media_auto_scale != 0,
                        media_type: trimmed_string(&extended.media_type),
                        media_desc: trimmed_string(&extended.media_desc),
                        media_width: crate::types::optional_i32_from_wire(extended.media_width),
                        media_height: crate::types::optional_i32_from_wire(extended.media_height),
                        media_loop: extended.media_loop != 0,
                    }));
            }
            AnyMessage::ParcelDwellReply(reply) => {
                let circuit = self.circuit_id_for(from).unwrap_or_default();
                self.events.push_back(Event::ParcelDwell {
                    local_id: ScopedParcelId::new(
                        circuit,
                        RegionLocalParcelId(reply.data.local_id),
                    ),
                    parcel_id: ParcelKey::from(reply.data.parcel_id),
                    dwell: reply.data.dwell,
                });
            }
            AnyMessage::ParcelAccessListReply(reply) => {
                let circuit = self.circuit_id_for(from).unwrap_or_default();
                self.events.push_back(Event::ParcelAccessList {
                    local_id: ScopedParcelId::new(
                        circuit,
                        RegionLocalParcelId(reply.data.local_id),
                    ),
                    scope: ParcelAccessScope::from_u32(reply.data.flags),
                    // A simulator represents an *empty* list as a single
                    // nil-agent placeholder block (never as zero blocks), so a
                    // nil id is the empty-list sentinel rather than a real member
                    // — the reference viewer skips it, and so do we.
                    entries: reply
                        .list
                        .iter()
                        .filter(|entry| !entry.id.is_nil())
                        .map(|entry| ParcelAccessEntry {
                            id: entry.id,
                            time: entry.time,
                            flags: ParcelAccessFlags(entry.flags),
                        })
                        .collect(),
                });
            }
            AnyMessage::ParcelObjectOwnersReply(reply) => {
                self.events.push_back(Event::ParcelObjectOwners {
                    owners: reply
                        .data
                        .iter()
                        .map(|owner| ParcelObjectOwner {
                            owner: crate::types::owner_key_from_wire(
                                owner.owner_id,
                                owner.is_group_owned,
                            ),
                            count: owner.count,
                            online_status: owner.online_status,
                        })
                        .collect(),
                });
            }
            AnyMessage::LandStatReply(reply) => {
                self.events.push_back(Event::LandStatReply {
                    report_type: LandStatReportType::from_u32(reply.request_data.report_type),
                    request_flags: reply.request_data.request_flags,
                    total_object_count: reply.request_data.total_object_count,
                    items: reply
                        .report_data
                        .iter()
                        .map(|item| LandStatItem {
                            task_local_id: RegionLocalObjectId(item.task_local_id),
                            task_id: ObjectKey::from(item.task_id),
                            location: RegionCoordinates::new(item.location_x, item.location_y, item.location_z),
                            score: item.score,
                            task_name: trimmed_string(&item.task_name),
                            owner_name: trimmed_string(&item.owner_name),
                        })
                        .collect(),
                });
            }
            AnyMessage::ParcelInfoReply(reply) => {
                let data = &reply.data;
                self.events.push_back(Event::ParcelDetails(ParcelDetails {
                    parcel_id: ParcelKey::from(data.parcel_id),
                    owner_id: data.owner_id,
                    name: trimmed_string(&data.name),
                    description: trimmed_string(&data.desc),
                    actual_area: crate::types::land_area_from_wire("ActualArea", data.actual_area)?,
                    billable_area: crate::types::land_area_from_wire(
                        "BillableArea",
                        data.billable_area,
                    )?,
                    flags: data.flags,
                    global_position: GlobalCoordinates::new(
                        f64::from(data.global_x),
                        f64::from(data.global_y),
                        f64::from(data.global_z),
                    ),
                    sim_name: sl_wire::region_name_from_wire(
                        "SimName",
                        &trimmed_string(&data.sim_name),
                    )?,
                    snapshot_id: crate::types::optional_key_from_wire(data.snapshot_id),
                    dwell: data.dwell,
                    // The packed parcel flags byte carries PARCEL_FOR_SALE (0x04).
                    sale_price: crate::types::linden_price_from_wire(
                        data.flags & 0x04 != 0,
                        "SalePrice",
                        data.sale_price,
                    )?,
                    auction_id: data.auction_id,
                }));
            }
            AnyMessage::EstateOwnerMessage(message) => {
                match trimmed_string(&message.method_data.method).as_str() {
                    "estateupdateinfo" => {
                        if let Some(info) = estate_info_from_params(&message.param_list)? {
                            self.events.push_back(Event::EstateInfo(Box::new(info)));
                        }
                    }
                    "setaccess" => {
                        if let Some(event) = estate_access_from_params(&message.param_list) {
                            self.events.push_back(event);
                        }
                    }
                    _ => {}
                }
            }
            AnyMessage::EstateCovenantReply(reply) => {
                let data = &reply.data;
                self.events.push_back(Event::EstateCovenant(EstateCovenant {
                    covenant_id: (!data.covenant_id.is_nil()).then_some(data.covenant_id),
                    covenant_timestamp: data.covenant_timestamp,
                    estate_name: trimmed_string(&data.estate_name),
                    estate_owner_id: data.estate_owner_id,
                }));
            }
            AnyMessage::TelehubInfo(info) => {
                let block = &info.telehub_block;
                self.events.push_back(Event::TelehubInfo(TelehubInfo {
                    object_id: crate::types::optional_key_from_wire(block.object_id),
                    object_name: trimmed_string(&block.object_name),
                    position: block.telehub_pos.clone(),
                    rotation: block.telehub_rot.clone(),
                    spawn_points: info
                        .spawn_point_block
                        .iter()
                        .map(|spawn| spawn.spawn_point_pos.clone())
                        .collect(),
                }));
            }
            AnyMessage::ChatFromSimulator(chat) => {
                let data = &chat.chat_data;
                match ChatType::from_u8(data.chat_type) {
                    // A typing animation trigger carries no text; surface it as a
                    // distinct typing signal rather than an empty chat line.
                    chat_type @ (ChatType::StartTyping | ChatType::StopTyping) => {
                        self.events.push_back(Event::ChatTyping {
                            from_name: trimmed_string(&data.from_name),
                            source_id: data.source_id,
                            typing: matches!(chat_type, ChatType::StartTyping),
                        });
                    }
                    _ => self
                        .events
                        .push_back(Event::ChatReceived(Box::new(chat_message(data)))),
                }
            }
            AnyMessage::ImprovedInstantMessage(im) => {
                let block = &im.message_block;
                match ImDialog::from_u8(block.dialog) {
                    // Typing notifications carry no real text; surface them as a
                    // distinct signal rather than an empty instant message.
                    dialog @ (ImDialog::TypingStart | ImDialog::TypingStop) => {
                        let from_agent_id = AgentKey::from(im.agent_data.agent_id);
                        let typing = matches!(dialog, ImDialog::TypingStart);
                        // The wire carries the session id but no `from_group`, so
                        // resolve by the registry: a tracked group / conference
                        // keyed by `block.id`, otherwise a 1:1 keyed by the peer
                        // (`from_agent_id` always identifies the typer — the 1:1
                        // `id` field is not reliably the XOR id across senders).
                        let group_kind = ChatSessionKind::Group {
                            group_id: GroupKey::from(block.id),
                        };
                        let conference_kind = ChatSessionKind::Conference {
                            id: ImSessionId::from(block.id),
                        };
                        let kind = if self.chat_session(group_kind).is_some() {
                            group_kind
                        } else if self.chat_session(conference_kind).is_some() {
                            conference_kind
                        } else {
                            ChatSessionKind::Direct {
                                peer: from_agent_id,
                            }
                        };
                        // Typing never opens a session (a non-creating lookup):
                        // if it is not already open, the event still fires but
                        // nothing is stored.
                        if let Some(chat_session) = self.chat_session_get_mut(kind) {
                            if typing {
                                chat_session.typing.insert(from_agent_id, now);
                            } else {
                                chat_session.typing.remove(&from_agent_id);
                            }
                        }
                        self.events.push_back(Event::ImTyping {
                            from_agent_id,
                            from_agent_name: trimmed_string(&block.from_agent_name),
                            session_id: block.id,
                            typing,
                        });
                    }
                    // Group IM session traffic (the session id is the group id).
                    // Inbound traffic means we are effectively a participant, so
                    // open/track the session (the registry mirrors live sessions).
                    ImDialog::SessionSend if block.from_group => {
                        let group_id = GroupKey::from(block.id);
                        let from_agent_id = AgentKey::from(im.agent_data.agent_id);
                        let message = trimmed_string(&block.message);
                        self.log_inbound_message(
                            ChatSessionKind::Group { group_id },
                            SessionMessage {
                                sender: from_agent_id,
                                dialog: ImDialog::SessionSend,
                                text: message.clone(),
                                timestamp: crate::types::optional_u32_from_wire(block.timestamp),
                            },
                            now,
                        );
                        self.events.push_back(Event::GroupSessionMessage {
                            group_id,
                            from_agent_id,
                            from_name: trimmed_string(&block.from_agent_name),
                            message,
                        });
                    }
                    dialog @ (ImDialog::SessionAdd | ImDialog::SessionLeave)
                        if block.from_group =>
                    {
                        let group_id = GroupKey::from(block.id);
                        let agent_id = AgentKey::from(im.agent_data.agent_id);
                        let joined = matches!(dialog, ImDialog::SessionAdd);
                        // Participant traffic also opens the session (it is
                        // "joined" traffic — the A4 rule), then folds the roster.
                        let chat_session =
                            self.chat_session_mut(ChatSessionKind::Group { group_id }, now);
                        if joined {
                            chat_session.participants.insert(agent_id);
                        } else {
                            chat_session.participants.remove(&agent_id);
                        }
                        self.events.push_back(Event::GroupSessionParticipant {
                            group_id,
                            agent_id,
                            joined,
                        });
                    }
                    // Ad-hoc conference session traffic mirrors the group-session
                    // arms above but with `from_group` clear (#28); the session id
                    // is the conference id, not a group id.
                    ImDialog::SessionSend => {
                        let id = ImSessionId::from(block.id);
                        let from_agent_id = AgentKey::from(im.agent_data.agent_id);
                        let message = trimmed_string(&block.message);
                        self.log_inbound_message(
                            ChatSessionKind::Conference { id },
                            SessionMessage {
                                sender: from_agent_id,
                                dialog: ImDialog::SessionSend,
                                text: message.clone(),
                                timestamp: crate::types::optional_u32_from_wire(block.timestamp),
                            },
                            now,
                        );
                        self.events.push_back(Event::ConferenceSessionMessage {
                            session_id: block.id,
                            from_agent_id,
                            from_name: trimmed_string(&block.from_agent_name),
                            message,
                        });
                    }
                    dialog @ (ImDialog::SessionAdd | ImDialog::SessionLeave) => {
                        let id = ImSessionId::from(block.id);
                        let agent_id = AgentKey::from(im.agent_data.agent_id);
                        let joined = matches!(dialog, ImDialog::SessionAdd);
                        // Conference participant traffic mirrors the group arm:
                        // open the session, then fold the roster.
                        let chat_session =
                            self.chat_session_mut(ChatSessionKind::Conference { id }, now);
                        if joined {
                            chat_session.participants.insert(agent_id);
                        } else {
                            chat_session.participants.remove(&agent_id);
                        }
                        self.events.push_back(Event::ConferenceSessionParticipant {
                            session_id: block.id,
                            agent_id,
                            joined,
                        });
                    }
                    // They accepted *our* friendship offer: the `from_agent_id`
                    // is the new friend. Add them to the buddy cache with the
                    // default `CAN_SEE_ONLINE` both ways (OpenSim/SL write that
                    // for both directions and push no `ChangeUserRights`), then
                    // surface the IM unchanged. (We accepting *their* offer takes
                    // the `accept_friendship` path instead — no IM is sent to the
                    // accepter.)
                    ImDialog::FriendshipAccepted => {
                        let friend_id = FriendKey::from(im.agent_data.agent_id);
                        self.add_friend(friend_id);
                        self.events
                            .push_back(Event::InstantMessageReceived(Box::new(instant_message(
                                &im.agent_data,
                                block,
                            ))));
                    }
                    // An ordinary 1:1 instant message opens/tracks the direct
                    // session keyed by the peer (the sender), then surfaces the IM
                    // unchanged. Only `Message` does this — the other non-session
                    // dialogs (offers, lures, system notices) carry no session.
                    ImDialog::Message => {
                        let peer = AgentKey::from(im.agent_data.agent_id);
                        let received = instant_message(&im.agent_data, block);
                        self.log_inbound_message(
                            ChatSessionKind::Direct { peer },
                            SessionMessage {
                                sender: peer,
                                dialog: ImDialog::Message,
                                text: received.message.clone(),
                                timestamp: received.timestamp,
                            },
                            now,
                        );
                        self.events
                            .push_back(Event::InstantMessageReceived(Box::new(received)));
                    }
                    _ => self
                        .events
                        .push_back(Event::InstantMessageReceived(Box::new(instant_message(
                            &im.agent_data,
                            block,
                        )))),
                }
            }
            AnyMessage::AvatarSitResponse(response) => {
                // Only act on a response to our own AgentRequestSit; complete the
                // sit with an AgentSit and surface the result.
                if matches!(self.sit, SitState::AwaitingResponse) {
                    let sit_object = ObjectKey::from(response.sit_object.id);
                    self.sit = SitState::Seated { on: sit_object };
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.sit = None;
                        circuit.send_agent_sit(now)?;
                    }
                    let transform = &response.sit_transform;
                    self.events.push_back(Event::SitResult {
                        sit_object,
                        autopilot: transform.auto_pilot,
                        sit_position: transform.sit_position.clone(),
                        sit_rotation: transform.sit_rotation.clone(),
                        camera_eye_offset: transform.camera_eye_offset.clone(),
                        camera_at_offset: transform.camera_at_offset.clone(),
                        force_mouselook: transform.force_mouselook,
                    });
                }
            }
            AnyMessage::AvatarPropertiesReply(reply) => {
                self.events
                    .push_back(Event::AvatarProperties(Box::new(avatar_properties(
                        reply.agent_data.avatar_id,
                        &reply.properties_data,
                    ))));
            }
            AnyMessage::AvatarInterestsReply(reply) => {
                self.events
                    .push_back(Event::AvatarInterests(Box::new(avatar_interests(
                        reply.agent_data.avatar_id,
                        &reply.properties_data,
                    ))));
            }
            AnyMessage::AvatarGroupsReply(reply) => {
                self.events.push_back(Event::AvatarGroups {
                    avatar_id: AgentKey::from(reply.agent_data.avatar_id),
                    groups: reply.group_data.iter().map(avatar_group).collect(),
                    list_in_profile: reply.new_group_data.list_in_profile,
                });
            }
            AnyMessage::AvatarPicksReply(reply) => {
                self.events.push_back(Event::AvatarPicks {
                    target_id: reply.agent_data.target_id,
                    picks: reply
                        .data
                        .iter()
                        .map(|pick| AvatarPick {
                            pick_id: PickKey::from(pick.pick_id),
                            name: trimmed_string(&pick.pick_name),
                        })
                        .collect(),
                });
            }
            AnyMessage::AvatarNotesReply(reply) => {
                self.events.push_back(Event::AvatarNotes {
                    target_id: reply.data.target_id,
                    notes: trimmed_string(&reply.data.notes),
                });
            }
            AnyMessage::AvatarClassifiedReply(reply) => {
                self.events.push_back(Event::AvatarClassifieds {
                    target_id: reply.agent_data.target_id,
                    classifieds: reply
                        .data
                        .iter()
                        .map(|classified| AvatarClassified {
                            classified_id: ClassifiedKey::from(classified.classified_id),
                            name: trimmed_string(&classified.name),
                        })
                        .collect(),
                });
            }
            AnyMessage::PickInfoReply(reply) => {
                self.events
                    .push_back(Event::PickInfo(Box::new(pick_info(&reply.data)?)));
            }
            AnyMessage::ClassifiedInfoReply(reply) => {
                self.events
                    .push_back(Event::ClassifiedInfo(Box::new(classified_info(&reply.data)?)));
            }
            AnyMessage::InventoryDescendents(reply) => {
                // OpenSim emits a single nil-id placeholder `FolderData` block for
                // an empty folder (an LLUDP "stuffing" quirk a real viewer
                // ignores); a nil folder/item id is never a real descendent, so
                // drop it here — otherwise the background crawl would mark the
                // phantom folder `Fetching` forever and an explicit crawl would
                // hang fetching it. Mirrors `bulk_update_inventory_from_llsd`.
                let folders: Vec<InventoryFolder> = reply
                    .folder_data
                    .iter()
                    .map(inventory_folder)
                    .filter(|folder| !folder.folder_id.uuid().is_nil())
                    .collect();
                let items: Vec<InventoryItem> = reply
                    .item_data
                    .iter()
                    .map(inventory_item)
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .filter(|item| !item.item_id.uuid().is_nil())
                    .collect();
                let folder_id = InventoryFolderKey::from(reply.agent_data.folder_id);
                // Route into the tree the target folder belongs to (agent or
                // Library), so a UDP Library fetch stays in the Library tree.
                let owner = self.inventory_reply_owner(folder_id);
                self.cache_inventory(&folders, &items, owner);
                self.inventory
                    .mark_folder_loaded(folder_id, reply.agent_data.version, owner);
                self.events.push_back(Event::InventoryDescendents {
                    folder_id,
                    version: reply.agent_data.version,
                    descendents: reply.agent_data.descendents,
                    folders,
                    items,
                });
            }
            // A single item the simulator created or whose asset it replaced
            // (the reply to `CreateInventoryItem`, or an accepted inventory
            // offer). Merge it into the cache.
            AnyMessage::UpdateCreateInventoryItem(reply) => {
                // The `InventoryData` block is repeatable: the simulator can batch
                // several created/updated items into one message, so surface every
                // entry (and cache each), not just the first.
                for data in &reply.inventory_data {
                    let item = inventory_item_from_create(data)?;
                    self.cache_inventory_item(item.clone());
                    let callback_id = crate::types::optional_u32_from_wire(data.callback_id)
                        .map(InventoryCallbackId);
                    self.events.push_back(Event::InventoryItemCreated {
                        sim_approved: reply.agent_data.sim_approved,
                        transaction_id: reply.agent_data.transaction_id,
                        callback_id,
                        item,
                    });
                }
            }
            // A batch update the simulator pushed (after a copy, give, or
            // server-side change). Merge folders and items into the cache.
            AnyMessage::BulkUpdateInventory(update) => {
                let folders: Vec<InventoryFolder> =
                    update.folder_data.iter().map(bulk_update_folder).collect();
                let items: Vec<InventoryItem> = update
                    .item_data
                    .iter()
                    .map(bulk_update_item)
                    .collect::<Result<_, _>>()?;
                // Carry each item's async `CallbackID` (when non-zero) so a client
                // that issued a create/copy can correlate the returned callback id
                // to the resulting item even though the reply arrived here rather
                // than as an `UpdateCreateInventoryItem`.
                let item_callbacks: Vec<(InventoryKey, InventoryCallbackId)> = update
                    .item_data
                    .iter()
                    .filter(|data| data.callback_id != 0)
                    .map(|data| {
                        (
                            InventoryKey::from(data.item_id),
                            InventoryCallbackId(data.callback_id),
                        )
                    })
                    .collect();
                self.cache_inventory(&folders, &items, InventoryOwner::Agent);
                self.events.push_back(Event::InventoryBulkUpdate {
                    transaction_id: update.agent_data.transaction_id,
                    folders,
                    items,
                    item_callbacks,
                });
            }
            AnyMessage::EnableSimulator(sim) => {
                let info = neighbor_info(&sim.simulator_info);
                // Pre-open a child-agent circuit to the neighbour so it holds the
                // agent's presence before the avatar crosses the border.
                self.open_child_circuit(info.sim, now)?;
                if let Some(circuit_id) = self.circuit_id_for(info.sim) {
                    self.regions.insert(circuit_id, info.region_handle);
                }
                self.events.push_back(Event::NeighborDiscovered(info));
            }
            AnyMessage::MapBlockReply(reply) => {
                for (index, data) in reply.data.iter().enumerate() {
                    if let Some(region) = map_region_info(data, reply.size.get(index))? {
                        self.events.push_back(Event::MapBlock(Box::new(region)));
                    }
                }
            }
            AnyMessage::MapItemReply(reply) => {
                self.events.push_back(Event::MapItems {
                    item_type: MapItemType::from_u32(reply.request_data.item_type),
                    items: reply.data.iter().map(map_item).collect(),
                });
            }
            AnyMessage::MapLayerReply(reply) => {
                self.events.push_back(Event::MapLayers {
                    layers: reply.layer_data.iter().map(map_layer).collect(),
                });
            }
            AnyMessage::TeleportStart(_) => {
                self.events.push_back(Event::TeleportStarted);
            }
            AnyMessage::TeleportProgress(progress) => {
                self.events.push_back(Event::TeleportProgress {
                    message: String::from_utf8_lossy(&progress.info.message).into_owned(),
                    teleport_flags: progress.info.teleport_flags,
                });
            }
            AnyMessage::TeleportLocal(_) => {
                // An intra-region teleport: no new circuit, just resume activity.
                if matches!(self.state, SessionState::Teleporting) {
                    self.state = SessionState::Active;
                    // A teleport (even a local one) unseats the agent and leaves
                    // in-world objects behind; drop their permission grants.
                    self.sit = SitState::NotSitting;
                    self.drop_inworld_grants();
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.teleport = None;
                    }
                    self.events.push_back(Event::TeleportLocal);
                }
            }
            AnyMessage::TeleportFailed(failed) => {
                if matches!(self.state, SessionState::Teleporting) {
                    self.state = SessionState::Active;
                    self.teleport = TeleportPhase::Idle;
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.teleport = None;
                    }
                }
                self.events.push_back(Event::TeleportFailed {
                    reason: String::from_utf8_lossy(&failed.info.reason).into_owned(),
                    alert_info: failed.alert_info.first().map(|block| AlertInfo {
                        message: String::from_utf8_lossy(&block.message).into_owned(),
                        extra_params: String::from_utf8_lossy(&block.extra_params).into_owned(),
                    }),
                });
            }
            AnyMessage::TeleportFinish(finish) => {
                // The UDP TeleportFinish path (grids without an event queue).
                // OpenSim normally delivers TeleportFinish over the CAPS event
                // queue instead; see `handle_caps_event`.
                if matches!(self.state, SessionState::Teleporting) {
                    let info = &finish.info;
                    // IPPORT is big-endian on the wire; the generated decoder
                    // reads it little-endian, so swap back to host order.
                    let port = info.sim_port.swap_bytes();
                    let dest = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(info.sim_ip)), port);
                    // An empty inline seed is the "absent" sentinel and falls back
                    // to the seed cached from the child's
                    // `EstablishAgentCommunication`; a non-empty but unparsable one
                    // is a hard error (drops the datagram) rather than masked.
                    let seed = sl_wire::optional_url_from_wire(
                        "seed-capability",
                        String::from_utf8_lossy(&info.seed_capability).as_ref(),
                    )?;
                    self.events.push_back(Event::TeleportFinished {
                        region_handle: RegionHandle(info.region_handle),
                        sim: dest,
                        maturity: Maturity::from_sim_access(info.sim_access),
                        flags: TeleportFlags(info.teleport_flags),
                    });
                    self.begin_handover(dest, RegionHandle(info.region_handle), seed, now)?;
                }
            }
            AnyMessage::CrossedRegion(crossed) => {
                // The avatar walked across a region border; the source region
                // hands us the destination's details. Promote the pre-opened
                // child circuit there to root.
                if matches!(self.state, SessionState::Active) {
                    let region = &crossed.region_data;
                    // IPPORT is big-endian on the wire; the generated decoder
                    // reads it little-endian, so swap back to host order.
                    let port = region.sim_port.swap_bytes();
                    let dest = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(region.sim_ip)), port);
                    // As in `TeleportFinish`: an empty inline seed falls back to
                    // the cached child seed during promotion; a non-empty but
                    // unparsable one is a hard error.
                    let seed = sl_wire::optional_url_from_wire(
                        "seed-capability",
                        String::from_utf8_lossy(&region.seed_capability).as_ref(),
                    )?;
                    self.promote_child_to_root(dest, RegionHandle(region.region_handle), seed, now)?;
                }
            }
            AnyMessage::StartPingCheck(ping) => {
                if let Some(circuit) = self.circuit.as_mut() {
                    circuit.send_complete_ping_check(PingId(ping.ping_id.ping_id), now)?;
                }
            }
            AnyMessage::CompletePingCheck(reply) => {
                // The simulator's answer to our keep-alive `StartPingCheck` on the
                // root circuit: surface the round-trip time when it matches the
                // ping in flight.
                if let Some((sim, rtt)) = self.circuit.as_mut().and_then(|circuit| {
                    circuit
                        .record_ping_reply(PingId(reply.ping_id.ping_id), now)
                        .map(|rtt| (circuit.sim_addr, rtt))
                }) {
                    self.events.push_back(Event::Ping {
                        sim,
                        child: false,
                        rtt,
                    });
                }
            }
            AnyMessage::PacketAck(ack) => {
                if let Some(circuit) = self.circuit.as_mut() {
                    for packet in &ack.packets {
                        circuit.record_acks(&[SequenceNumber(packet.id)]);
                    }
                }
            }
            AnyMessage::MuteListUpdate(update) => {
                // The mute list changed; download the named file over Xfer.
                let filename = trimmed_string(&update.mute_data.filename);
                if filename.is_empty() {
                    self.events.push_back(Event::MuteList(Vec::new()));
                } else {
                    self.start_xfer_download(XferPurpose::MuteList, &filename, now)?;
                }
            }
            AnyMessage::UseCachedMuteList(_) => {
                self.events.push_back(Event::MuteListUnchanged);
            }
            AnyMessage::SendXferPacket(packet) => {
                let xfer_id = XferId(packet.xfer_id.id);
                let packet_num = packet.xfer_id.packet;
                // The high bit marks the final packet; the low 31 bits are the
                // sequence number (the first packet is sequence 0).
                let is_last = packet_num & 0x8000_0000 != 0;
                let sequence = packet_num & 0x7fff_ffff;
                if self.xfer_downloads.contains_key(&xfer_id) {
                    // The first packet carries a 4-byte little-endian length
                    // prefix before the file data; later packets are raw.
                    let chunk: &[u8] = if sequence == 0 {
                        packet.data_packet.data.get(4..).unwrap_or(&[])
                    } else {
                        &packet.data_packet.data
                    };
                    if let Some(download) = self.xfer_downloads.get_mut(&xfer_id) {
                        download.buffer.extend_from_slice(chunk);
                    }
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_confirm_xfer_packet(xfer_id, packet_num, now)?;
                    }
                    if is_last && let Some(download) = self.xfer_downloads.remove(&xfer_id) {
                        self.finish_xfer_download(xfer_id, download)?;
                    }
                }
            }
            AnyMessage::ImageData(image) => {
                // The first packet of a UDP texture download: the codec/size/
                // packet-count header plus packet 0's data.
                let id = image.image_id.id;
                let completed = if let Some(download) = self.texture_downloads.get_mut(&id) {
                    download.codec = ImageCodec::from_code(image.image_id.codec);
                    download.packets = image.image_id.packets;
                    download.chunks.insert(0, image.image_data.data.clone());
                    download.is_complete()
                } else {
                    false
                };
                if completed && let Some(download) = self.texture_downloads.remove(&id) {
                    let texture = Texture {
                        id: TextureKey::from(id),
                        codec: download.codec,
                        data: download.assemble(),
                    };
                    self.events
                        .push_back(Event::TextureReceived(Box::new(texture)));
                }
            }
            AnyMessage::ImagePacket(image) => {
                // A follow-on packet of a UDP texture download (packets 1..).
                let id = image.image_id.id;
                let packet_index = image.image_id.packet;
                let completed = if let Some(download) = self.texture_downloads.get_mut(&id) {
                    download
                        .chunks
                        .insert(packet_index, image.image_data.data.clone());
                    download.is_complete()
                } else {
                    false
                };
                if completed && let Some(download) = self.texture_downloads.remove(&id) {
                    let texture = Texture {
                        id: TextureKey::from(id),
                        codec: download.codec,
                        data: download.assemble(),
                    };
                    self.events
                        .push_back(Event::TextureReceived(Box::new(texture)));
                }
            }
            AnyMessage::ImageNotInDatabase(missing) => {
                let id = missing.image_id.id;
                self.texture_downloads.remove(&id);
                self.events
                    .push_back(Event::TextureNotFound(TextureKey::from(id)));
            }
            // Another avatar's appearance (baked textures + visual params),
            // pushed when it comes into range or restyles. Decoded for both the
            // modern server-side bake (the texture entry names the server's bakes)
            // and the legacy client-side bake.
            AnyMessage::AvatarAppearance(appearance) => {
                self.events
                    .push_back(Event::AvatarAppearance(Box::new(avatar_appearance(
                        appearance,
                    ))));
            }
            // The agent's own current wearables, pushed at login and after every
            // wearable change (or in reply to `AgentWearablesRequest`).
            AnyMessage::AgentWearablesUpdate(update) => {
                self.events.push_back(Event::AgentWearables {
                    serial: update.agent_data.serial_num,
                    wearables: update
                        .wearable_data
                        .iter()
                        .map(|block| Wearable {
                            item_id: InventoryKey::from(block.item_id),
                            asset_id: crate::types::optional_uuid_from_wire(block.asset_id),
                            wearable_type: WearableType::from_code(block.wearable_type),
                        })
                        .collect(),
                });
            }
            // Another avatar's currently-playing animations, pushed whenever its
            // animation set changes. The list is the complete current set, not a
            // delta — a stopped animation simply drops out of a later update.
            AnyMessage::AvatarAnimation(animation) => {
                self.events.push_back(Event::AvatarAnimation {
                    avatar_id: AgentKey::from(animation.sender.id),
                    animations: avatar_animations(animation),
                    physical_events: animation
                        .physical_avatar_event_list
                        .iter()
                        .map(|block| block.type_data.clone())
                        .collect(),
                });
            }
            // The full authoritative set of animations now signalled on an
            // animated-mesh (animesh) object; the object analogue of
            // `AvatarAnimation`.
            AnyMessage::ObjectAnimation(animation) => {
                self.events.push_back(Event::ObjectAnimation {
                    object_id: ObjectKey::from(animation.sender.id),
                    animations: animation
                        .animation_list
                        .iter()
                        .map(|block| ObjectPlayingAnimation {
                            anim_id: AnimationKey::from(block.anim_id),
                            sequence_id: block.anim_sequence_id,
                        })
                        .collect(),
                });
            }
            // The simulator could not find one of the agent's temporary baked
            // textures and is asking the viewer to rebake and re-upload it.
            AnyMessage::RebakeAvatarTextures(rebake) => {
                self.events.push_back(Event::RebakeAvatarTextures {
                    texture_id: TextureKey::from(rebake.texture_data.texture_id),
                });
            }
            // The simulator informs this agent that a friendship has ended; the
            // agent's own id in `AgentData` is redundant, only the former
            // friend (`ExBlock.OtherID`) matters.
            AnyMessage::TerminateFriendship(terminate) => {
                let other = FriendKey::from(terminate.ex_block.other_id);
                // A former friend must never linger as online or in the roster.
                self.friends.remove(&other);
                self.online.remove(&other);
                self.events.push_back(Event::FriendshipTerminated { other });
            }
            // Another agent offered this agent their calling card (a reference
            // card to that avatar, not a friendship request). `AgentData.AgentID`
            // is the offering agent; `AgentBlock.DestID` is this agent and is
            // dropped.
            AnyMessage::OfferCallingCard(offer) => {
                self.events.push_back(Event::CallingCardOffered {
                    offering_agent: AgentKey::from(offer.agent_data.agent_id),
                    transaction: TransactionId::from(offer.agent_block.transaction_id),
                });
            }
            // A calling card this agent offered was accepted. `AgentData.AgentID`
            // is the accepting agent; the `FolderData` destination folder is the
            // accepter's own inventory and is dropped.
            AnyMessage::AcceptCallingCard(accept) => {
                self.events.push_back(Event::CallingCardAccepted {
                    agent: AgentKey::from(accept.agent_data.agent_id),
                    transaction: TransactionId::from(accept.transaction_block.transaction_id),
                });
            }
            // A calling card this agent offered was declined.
            AnyMessage::DeclineCallingCard(decline) => {
                self.events.push_back(Event::CallingCardDeclined {
                    agent: AgentKey::from(decline.agent_data.agent_id),
                    transaction: TransactionId::from(decline.transaction_block.transaction_id),
                });
            }
            // The simulator removed inventory items server-side (e.g. a delete
            // made from another session). The echoed `AgentData.AgentID` is
            // dropped; a client mirroring inventory drops these items.
            AnyMessage::RemoveInventoryItem(remove) => {
                self.events.push_back(Event::InventoryItemsRemoved {
                    items: remove
                        .inventory_data
                        .iter()
                        .map(|block| InventoryKey::from(block.item_id))
                        .collect(),
                });
            }
            // The simulator removed inventory folders server-side.
            AnyMessage::RemoveInventoryFolder(remove) => {
                self.events.push_back(Event::InventoryFoldersRemoved {
                    folders: remove
                        .folder_data
                        .iter()
                        .map(|block| InventoryFolderKey::from(block.folder_id))
                        .collect(),
                });
            }
            // The simulator removed a mixed set of folders and items server-side
            // in one message.
            AnyMessage::RemoveInventoryObjects(remove) => {
                self.events.push_back(Event::InventoryObjectsRemoved {
                    folders: remove
                        .folder_data
                        .iter()
                        .map(|block| InventoryFolderKey::from(block.folder_id))
                        .collect(),
                    items: remove
                        .item_data
                        .iter()
                        .map(|block| InventoryKey::from(block.item_id))
                        .collect(),
                });
            }
            // The simulator re-parented (and optionally renamed) inventory items
            // server-side. An empty `NewName` means the move does not rename.
            AnyMessage::MoveInventoryItem(move_item) => {
                self.events.push_back(Event::InventoryItemsMoved {
                    stamp: move_item.agent_data.stamp,
                    moves: move_item
                        .inventory_data
                        .iter()
                        .map(|block| {
                            let new_name = trimmed_string(&block.new_name);
                            InventoryItemMove {
                                item: InventoryKey::from(block.item_id),
                                folder: InventoryFolderKey::from(block.folder_id),
                                new_name: if new_name.is_empty() {
                                    None
                                } else {
                                    Some(new_name)
                                },
                            }
                        })
                        .collect(),
                });
            }
            // The contents serial and Xfer filename of an in-world object's task
            // inventory, in reply to a `RequestTaskInventory`.
            AnyMessage::ReplyTaskInventory(reply) => {
                let task = ObjectKey::from(reply.inventory_data.task_id);
                let serial = reply.inventory_data.serial;
                let filename = trimmed_string(&reply.inventory_data.filename);
                self.events
                    .push_back(Event::TaskInventoryReply(TaskInventoryReply {
                        task,
                        serial,
                        filename: filename.clone(),
                    }));
                // If `fetch_task_inventory` asked for this object's parsed
                // contents, follow the reply to its `Xfer` file (or emit an empty
                // listing directly when the task inventory is empty).
                let claimed = self.pending_task_inventory.remove(&task)
                    || self.pending_task_inventory_unresolved.pop_front().is_some();
                if claimed {
                    if filename.is_empty() {
                        self.events.push_back(Event::TaskInventoryContents {
                            task,
                            serial,
                            items: Vec::new(),
                        });
                    } else {
                        self.start_xfer_download(
                            XferPurpose::TaskInventory { task, serial },
                            &filename,
                            now,
                        )?;
                    }
                }
            }
            // The agent's own account contact preferences, in reply to a
            // `UserInfoRequest`. The echoed `AgentData.AgentID` is dropped.
            AnyMessage::UserInfoReply(reply) => {
                self.events.push_back(Event::UserInfo(UserInfo {
                    im_via_email: reply.user_data.im_via_e_mail,
                    directory_visibility: DirectoryVisibility::from_wire(&trimmed_string(
                        &reply.user_data.directory_visibility,
                    )),
                    email: trimmed_string(&reply.user_data.e_mail),
                }));
            }
            // A delayed derez succeeded with no inventory created on the viewer
            // (e.g. a save into task inventory); correlate via the transaction id.
            AnyMessage::DeRezAck(ack) => {
                self.events.push_back(Event::DeRezAck {
                    transaction: TransactionId::from(ack.transaction_data.transaction_id),
                    success: ack.transaction_data.success,
                });
            }
            // The simulator forced this agent's object selection. The region-local
            // ids are scoped to the originating circuit.
            AnyMessage::ForceObjectSelect(force) => {
                if let Some(circuit_id) = self.circuit_id_for(from) {
                    self.events.push_back(Event::ForceObjectSelect {
                        reset_list: force.header.reset_list,
                        objects: force
                            .data
                            .iter()
                            .map(|block| {
                                ScopedObjectId::new(circuit_id, RegionLocalObjectId(block.local_id))
                            })
                            .collect(),
                    });
                }
            }
            // The simulator granted (or revoked, with level 0) this agent's
            // god-like powers. The wire `Token` is checked on the sim and ignored
            // by the viewer, so it is dropped.
            AnyMessage::GrantGodlikePowers(grant) => {
                self.events.push_back(Event::GodlikePowersGranted {
                    god_level: grant.grant_data.god_level,
                });
            }
            // A one-shot spatial sound played at a fixed region-local position
            // (a scripted `llTriggerSound`, a collision sound, …). May originate
            // in a neighbouring region, so it carries its own region handle. The
            // wire `ParentID` is nil when the triggering object is itself the
            // root, which we surface as `None`.
            AnyMessage::SoundTrigger(trigger) => {
                let block = &trigger.sound_data;
                self.events.push_back(Event::SoundTrigger {
                    sound_id: block.sound_id,
                    owner_id: block.owner_id,
                    object_id: ObjectKey::from(block.object_id),
                    parent_id: (!block.parent_id.is_nil())
                        .then_some(ObjectKey::from(block.parent_id)),
                    region_handle: RegionHandle(block.handle),
                    position: block.position.clone(),
                    gain: block.gain,
                });
            }
            // A looping or one-shot sound bound to an in-world object (a scripted
            // `llPlaySound`/`llLoopSound`); the `STOP` flag stops it instead.
            AnyMessage::AttachedSound(sound) => {
                let block = &sound.data_block;
                self.events.push_back(Event::AttachedSound {
                    sound_id: block.sound_id,
                    object_id: ObjectKey::from(block.object_id),
                    owner_id: block.owner_id,
                    gain: block.gain,
                    flags: SoundFlags(block.flags),
                });
            }
            // A volume change for a sound already attached to an object.
            AnyMessage::AttachedSoundGainChange(change) => {
                let block = &change.data_block;
                self.events.push_back(Event::AttachedSoundGainChange {
                    object_id: ObjectKey::from(block.object_id),
                    gain: block.gain,
                });
            }
            // A hint to pre-fetch sound assets the simulator is about to play.
            AnyMessage::PreloadSound(preload) => {
                self.events.push_back(Event::PreloadSound {
                    sounds: preload
                        .data_block
                        .iter()
                        .map(|block| SoundPreload {
                            sound_id: block.sound_id,
                            object_id: ObjectKey::from(block.object_id),
                            owner_id: block.owner_id,
                        })
                        .collect(),
                });
            }
            // The reply to a baked-texture cache query (`AgentCachedTexture`).
            AnyMessage::AgentCachedTextureResponse(response) => {
                self.events.push_back(Event::CachedTextureResponse {
                    serial: response.agent_data.serial_num,
                    textures: response
                        .wearable_data
                        .iter()
                        .map(|block| (block.texture_index, block.texture_id))
                        .collect(),
                });
            }
            // Coarse (minimap) positions of nearby avatars. The location and
            // agent-data blocks are parallel arrays; `you`/`prey` index into them
            // (a negative index means "none").
            AnyMessage::CoarseLocationUpdate(update) => {
                let locations = update
                    .agent_data
                    .iter()
                    .zip(update.location.iter())
                    .map(|(agent, location)| CoarseLocation {
                        agent_id: AgentKey::from(agent.agent_id),
                        x: location.x,
                        y: location.y,
                        z: u16::from(location.z).saturating_mul(4),
                    })
                    .collect();
                self.events.push_back(Event::CoarseLocationUpdate {
                    locations,
                    you: index_into(update.index.you),
                    prey: index_into(update.index.prey),
                });
            }
            // Periodic region performance telemetry (~1 Hz). `RegionX`/`RegionY`
            // carry the region's map-tile indices (grid coordinates); the
            // 64-bit extended flags fall back to the zero-extended 32-bit flags
            // when the sim sends no `RegionInfo` block.
            AnyMessage::SimStats(stats) => {
                let region_flags_extended = stats
                    .region_info
                    .first()
                    .map_or_else(|| u64::from(stats.region.region_flags), |info| {
                        info.region_flags_extended
                    });
                self.events.push_back(Event::SimStats(Box::new(RegionStats {
                    grid_coordinates: GridCoordinates::new(
                        stats.region.region_x,
                        stats.region.region_y,
                    ),
                    region_flags: stats.region.region_flags,
                    object_capacity: stats.region.object_capacity,
                    region_flags_extended,
                    stats: stats
                        .stat
                        .iter()
                        .map(|block| (SimStatId::from_id(block.stat_id), block.stat_value))
                        .collect(),
                })));
            }
            // The simulator's world clock and sun state, so the viewer can
            // resynchronise its day cycle.
            AnyMessage::SimulatorViewerTimeMessage(time) => {
                self.events
                    .push_back(Event::SimulatorTime(Box::new(SimulatorTime {
                        usec_since_start: time.time_info.usec_since_start,
                        sec_per_day: time.time_info.sec_per_day,
                        sec_per_year: time.time_info.sec_per_year,
                        sun_direction: time.time_info.sun_direction.clone(),
                        sun_phase: time.time_info.sun_phase,
                        sun_ang_velocity: time.time_info.sun_ang_velocity.clone(),
                    })));
            }
            // Transient HUD effects from other avatars (look-at / point-at gaze,
            // beams, …). Each effect's `TypeData` is decoded into a typed
            // `ViewerEffectData` (unknown layouts stay raw).
            AnyMessage::ViewerEffect(effect) => {
                let effects = effect
                    .effect
                    .iter()
                    .map(|block| {
                        let effect_type = ViewerEffectType::from_code(block.r#type);
                        ViewerEffect {
                            id: block.id,
                            agent_id: AgentKey::from(block.agent_id),
                            effect_type,
                            duration: block.duration,
                            color: block.color,
                            data: ViewerEffectData::from_wire(effect_type, &block.type_data),
                        }
                    })
                    .collect();
                self.events.push_back(Event::ViewerEffect(effects));
            }
            // The reply to a `FindAgent` lookup: the located global positions.
            AnyMessage::FindAgent(find) => {
                self.events.push_back(Event::FindAgentReply {
                    hunter: find.agent_block.hunter,
                    prey: find.agent_block.prey,
                    locations: find
                        .location_block
                        .iter()
                        .map(|block| (block.global_x, block.global_y))
                        .collect(),
                });
            }
            // The people results of a `DirFindQuery` (people search).
            AnyMessage::DirPeopleReply(reply) => {
                self.events.push_back(Event::DirPeopleReply {
                    query_id: reply.query_data.query_id,
                    results: reply
                        .query_replies
                        .iter()
                        .map(|block| DirPeopleResult {
                            agent_id: AgentKey::from(block.agent_id),
                            first_name: trimmed_string(&block.first_name),
                            last_name: trimmed_string(&block.last_name),
                            group: trimmed_string(&block.group),
                            online: block.online,
                            reputation: block.reputation,
                        })
                        .collect(),
                });
            }
            // The group results of a `DirFindQuery` (group search).
            AnyMessage::DirGroupsReply(reply) => {
                self.events.push_back(Event::DirGroupsReply {
                    query_id: reply.query_data.query_id,
                    results: reply
                        .query_replies
                        .iter()
                        .map(|block| DirGroupResult {
                            group_id: GroupKey::from(block.group_id),
                            group_name: trimmed_string(&block.group_name),
                            members: block.members,
                            search_order: block.search_order,
                        })
                        .collect(),
                });
            }
            // The event results of a `DirFindQuery` (event search).
            AnyMessage::DirEventsReply(reply) => {
                self.events.push_back(Event::DirEventsReply {
                    query_id: reply.query_data.query_id,
                    results: reply
                        .query_replies
                        .iter()
                        .map(|block| DirEventResult {
                            owner_id: block.owner_id,
                            name: trimmed_string(&block.name),
                            event_id: EventId::new(block.event_id),
                            date: trimmed_string(&block.date),
                            unix_time: block.unix_time,
                            event_flags: block.event_flags,
                        })
                        .collect(),
                    status: reply
                        .status_data
                        .first()
                        .map_or(0, |status| status.status),
                });
            }
            // The results of a `DirClassifiedQuery`.
            AnyMessage::DirClassifiedReply(reply) => {
                self.events.push_back(Event::DirClassifiedReply {
                    query_id: reply.query_data.query_id,
                    results: reply
                        .query_replies
                        .iter()
                        .map(|block| {
                            Ok(DirClassifiedResult {
                                classified_id: ClassifiedKey::from(block.classified_id),
                                name: trimmed_string(&block.name),
                                classified_flags: block.classified_flags,
                                creation_date: block.creation_date,
                                expiration_date: block.expiration_date,
                                price_for_listing: crate::types::linden_from_wire(
                                    "PriceForListing",
                                    block.price_for_listing,
                                )?,
                            })
                        })
                        .collect::<Result<_, sl_wire::WireError>>()?,
                    status: reply
                        .status_data
                        .first()
                        .map_or(0, |status| status.status),
                });
            }
            // The results of a `DirPlacesQuery`.
            AnyMessage::DirPlacesReply(reply) => {
                self.events.push_back(Event::DirPlacesReply {
                    query_id: reply
                        .query_data
                        .first()
                        .map_or_else(Uuid::nil, |query| query.query_id),
                    results: reply
                        .query_replies
                        .iter()
                        .map(|block| DirPlaceResult {
                            parcel_id: ParcelKey::from(block.parcel_id),
                            name: trimmed_string(&block.name),
                            for_sale: block.for_sale,
                            auction: block.auction,
                            dwell: block.dwell,
                        })
                        .collect(),
                    status: reply
                        .status_data
                        .first()
                        .map_or(0, |status| status.status),
                });
            }
            // The results of a `DirLandQuery`.
            AnyMessage::DirLandReply(reply) => {
                self.events.push_back(Event::DirLandReply {
                    query_id: reply.query_data.query_id,
                    results: reply
                        .query_replies
                        .iter()
                        .map(|block| {
                            Ok(DirLandResult {
                                parcel_id: ParcelKey::from(block.parcel_id),
                                name: trimmed_string(&block.name),
                                auction: block.auction,
                                for_sale: block.for_sale,
                                sale_price: crate::types::linden_price_from_wire(
                                    block.for_sale,
                                    "SalePrice",
                                    block.sale_price,
                                )?,
                                actual_area: crate::types::land_area_from_wire(
                                    "ActualArea",
                                    block.actual_area,
                                )?,
                            })
                        })
                        .collect::<Result<_, sl_wire::WireError>>()?,
                });
            }
            // The results of an `AvatarPickerRequest` (name autocomplete).
            AnyMessage::AvatarPickerReply(reply) => {
                self.events.push_back(Event::AvatarPickerReply {
                    query_id: reply.agent_data.query_id,
                    results: reply
                        .data
                        .iter()
                        .map(|block| AvatarPickerResult {
                            avatar_id: AgentKey::from(block.avatar_id),
                            first_name: trimmed_string(&block.first_name),
                            last_name: trimmed_string(&block.last_name),
                        })
                        .collect(),
                });
            }
            // The results of a `PlacesQuery` (land holdings).
            AnyMessage::PlacesReply(reply) => {
                self.events.push_back(Event::PlacesReply {
                    query_id: reply.agent_data.query_id,
                    transaction_id: reply.transaction_data.transaction_id,
                    results: reply
                        .query_data
                        .iter()
                        .map(|block| {
                            Ok(PlacesResult {
                                owner_id: block.owner_id,
                                name: trimmed_string(&block.name),
                                description: trimmed_string(&block.desc),
                                actual_area: crate::types::land_area_from_wire(
                                    "ActualArea",
                                    block.actual_area,
                                )?,
                                billable_area: crate::types::land_area_from_wire(
                                    "BillableArea",
                                    block.billable_area,
                                )?,
                                flags: block.flags,
                                global_position: GlobalCoordinates::new(
                                    f64::from(block.global_x),
                                    f64::from(block.global_y),
                                    f64::from(block.global_z),
                                ),
                                sim_name: sl_wire::region_name_from_wire(
                                    "SimName",
                                    &trimmed_string(&block.sim_name),
                                )?,
                                snapshot_id: crate::types::optional_key_from_wire(block.snapshot_id),
                                dwell: block.dwell,
                                price: crate::types::linden_from_wire("Price", block.price)?,
                            })
                        })
                        .collect::<Result<_, sl_wire::WireError>>()?,
                });
            }
            // The full detail of an event, in reply to an `EventInfoRequest`.
            AnyMessage::EventInfoReply(reply) => {
                let data = &reply.event_data;
                let [global_x, global_y, global_z] = data.global_pos;
                self.events.push_back(Event::EventInfoReply {
                    info: EventInfo {
                        event_id: EventId::new(data.event_id),
                        creator: AgentKey::from(parse_uuid_string("Creator", &data.creator)?),
                        name: trimmed_string(&data.name),
                        category: trimmed_string(&data.category),
                        description: trimmed_string(&data.desc),
                        date: trimmed_string(&data.date),
                        date_utc: data.date_utc,
                        duration: data.duration,
                        cover: data.cover,
                        amount: crate::types::linden_cover_from_wire(data.cover, data.amount),
                        sim_name: sl_wire::region_name_from_wire(
                            "SimName",
                            &trimmed_string(&data.sim_name),
                        )?,
                        global_position: GlobalCoordinates::new(global_x, global_y, global_z),
                        flags: data.event_flags,
                    },
                });
            }
            // An object's pay-button layout, in reply to a `RequestPayPrice`.
            AnyMessage::PayPriceReply(reply) => {
                self.events.push_back(Event::PayPriceReply {
                    object_id: ObjectKey::from(reply.object_data.object_id),
                    default_pay_price: reply.object_data.default_pay_price,
                    pay_buttons: reply
                        .button_data
                        .iter()
                        .map(|button| button.pay_button)
                        .collect(),
                });
            }
            // An object's condensed broadcast properties, in reply to a
            // `RequestObjectPropertiesFamily`.
            AnyMessage::ObjectPropertiesFamily(reply) => {
                let data = &reply.object_data;
                self.events.push_back(Event::ObjectPropertiesFamily {
                    properties: ObjectPropertiesFamily {
                        request_flags: data.request_flags,
                        object_id: ObjectKey::from(data.object_id),
                        owner: crate::types::object_owner_from_wire(data.owner_id, data.group_id),
                        group: crate::types::group_from_wire(data.group_id),
                        permissions: Permissions5 {
                            base: Permissions::from_bits(data.base_mask),
                            owner: Permissions::from_bits(data.owner_mask),
                            group: Permissions::from_bits(data.group_mask),
                            everyone: Permissions::from_bits(data.everyone_mask),
                            next_owner: Permissions::from_bits(data.next_owner_mask),
                        },
                        ownership_cost: crate::types::linden_from_wire(
                            "OwnershipCost",
                            data.ownership_cost,
                        )?,
                        sale_type: data.sale_type,
                        sale_price: crate::types::linden_price_from_wire(
                            data.sale_type != 0,
                            "SalePrice",
                            data.sale_price,
                        )?,
                        category: data.category,
                        last_owner_id: data.last_owner_id,
                        name: trimmed_string(&data.name),
                        description: trimmed_string(&data.description),
                    },
                });
            }
            AnyMessage::ScriptRunningReply(reply) => {
                let script = &reply.script;
                self.events.push_back(Event::ScriptRunning {
                    object_id: ObjectKey::from(script.object_id),
                    item_id: InventoryKey::from(script.item_id),
                    running: script.running,
                });
            }
            AnyMessage::GenericMessage(generic)
                // The sim NUL-terminates the method name on the wire.
                if trimmed_string(&generic.method_data.method) == "emptymutelist" =>
            {
                self.events.push_back(Event::MuteList(Vec::new()));
            }
            // A generic method-name + parameter envelope used for a grab-bag of
            // loosely-coupled features keyed by `Method` (the feature-specific
            // ones, like `emptymutelist` above, are matched first); the parameter
            // blobs are surfaced verbatim for the consumer to parse.
            AnyMessage::GenericMessage(generic) => {
                self.events.push_back(Event::GenericMessage(GenericMessage {
                    method: trimmed_string(&generic.method_data.method),
                    invoice: InvoiceId::from(generic.method_data.invoice),
                    params: generic
                        .param_list
                        .iter()
                        .map(|block| block.parameter.clone())
                        .collect(),
                }));
            }
            // The same envelope as `GenericMessage`, but with a larger per-param
            // size limit (real grids carry it over HTTP rather than UDP).
            AnyMessage::LargeGenericMessage(generic) => {
                self.events
                    .push_back(Event::LargeGenericMessage(GenericMessage {
                        method: trimmed_string(&generic.method_data.method),
                        invoice: InvoiceId::from(generic.method_data.invoice),
                        params: generic
                            .param_list
                            .iter()
                            .map(|block| block.parameter.clone())
                            .collect(),
                    }));
            }
            // An optimised streaming envelope: a numeric method id plus a single
            // opaque data blob (e.g. a GLTF material override), surfaced verbatim.
            AnyMessage::GenericStreamingMessage(streaming) => {
                self.events
                    .push_back(Event::GenericStreamingMessage(GenericStreamingMessage {
                        method: streaming.method_data.method,
                        data: streaming.data_block.data.clone(),
                    }));
            }
            // A generic UDP error report: an HTTP-like code plus an originating
            // system path and human-readable message, surfaced verbatim. The
            // `id` correlation field is deliberately polymorphic on the wire, so
            // it stays a raw `Uuid`; the binary `data` blob is kept verbatim.
            AnyMessage::Error(error) => {
                self.events.push_back(Event::ServerError(Box::new(ServerError {
                    agent: AgentKey::from(error.agent_data.agent_id),
                    code: error.data.code,
                    token: trimmed_string(&error.data.token),
                    id: error.data.id,
                    system: trimmed_string(&error.data.system),
                    message: trimmed_string(&error.data.message),
                    data: error.data.data.clone(),
                })));
            }
            // A notice that a feature the agent asked for is unavailable.
            AnyMessage::FeatureDisabled(disabled) => {
                self.events.push_back(Event::FeatureDisabled(FeatureDisabled {
                    message: trimmed_string(&disabled.failure_info.error_message),
                    agent: AgentKey::from(disabled.failure_info.agent_id),
                    transaction: TransactionId::from(disabled.failure_info.transaction_id),
                }));
            }
            // A server-initiated forced logout: surface the kick details, then
            // drive the session to its terminal `Disconnected` state (the
            // routing target address / echoed session id carry nothing useful).
            AnyMessage::KickUser(kick) => {
                let reason = trimmed_string(&kick.user_info.reason);
                self.events.push_back(Event::Kicked(Kick {
                    agent: AgentKey::from(kick.user_info.agent_id),
                    reason: reason.clone(),
                }));
                self.close(DisconnectReason::Kicked { message: reason });
            }
            AnyMessage::ScriptDialog(dialog) => {
                self.events
                    .push_back(Event::ScriptDialog(Box::new(script_dialog(dialog))));
            }
            AnyMessage::ScriptQuestion(question) => {
                self.events
                    .push_back(Event::ScriptPermissionRequest(Box::new(
                        script_permission_request(question),
                    )));
            }
            AnyMessage::ScriptControlChange(change) => {
                // Fold every block into the session-global taken-controls tracker
                // before surfacing the event (the driver still routes the actual
                // inputs from the unchanged event). A take/release pair can arrive
                // in one message; they are applied in order.
                for block in &change.data {
                    self.note_taken_controls(
                        ScriptControlAction::from_take_controls(block.take_controls),
                        ControlFlags::from_bits(block.controls),
                        block.pass_to_agent,
                    );
                }
                let controls = change
                    .data
                    .iter()
                    .map(|block| ScriptControl {
                        action: ScriptControlAction::from_take_controls(block.take_controls),
                        controls: ControlFlags::from_bits(block.controls),
                        pass_to_agent: block.pass_to_agent,
                    })
                    .collect();
                self.events.push_back(Event::ScriptControlChange(controls));
            }
            AnyMessage::SetFollowCamProperties(properties) => {
                let object_id = ObjectKey::from(properties.object_data.object_id);
                let properties = properties
                    .camera_property
                    .iter()
                    .map(|block| FollowCamPropertyValue {
                        property: FollowCamProperty::from_i32(block.r#type),
                        value: block.value,
                    })
                    .collect();
                self.events.push_back(Event::SetFollowCamProperties {
                    object_id,
                    properties,
                });
            }
            AnyMessage::ClearFollowCamProperties(clear) => {
                self.events.push_back(Event::ClearFollowCamProperties {
                    object_id: ObjectKey::from(clear.object_data.object_id),
                });
            }
            AnyMessage::AlertMessage(alert) => {
                self.events.push_back(Event::AlertMessage {
                    message: trimmed_string(&alert.alert_data.message),
                    alert_info: alert
                        .alert_info
                        .iter()
                        .map(|block| AlertInfo {
                            message: trimmed_string(&block.message),
                            extra_params: trimmed_string(&block.extra_params),
                        })
                        .collect(),
                    agents: alert
                        .agent_info
                        .iter()
                        .map(|block| block.agent_id)
                        .collect(),
                });
            }
            AnyMessage::AgentAlertMessage(alert) => {
                self.events.push_back(Event::AgentAlertMessage {
                    agent_id: AgentKey::from(alert.agent_data.agent_id),
                    modal: alert.alert_data.modal,
                    message: trimmed_string(&alert.alert_data.message),
                });
            }
            AnyMessage::MeanCollisionAlert(alert) => {
                let collisions = alert
                    .mean_collision
                    .iter()
                    .map(|block| MeanCollision {
                        victim: block.victim,
                        perp: block.perp,
                        time: block.time,
                        magnitude: block.mag,
                        collision_type: MeanCollisionType::from_u8(block.r#type),
                    })
                    .collect();
                self.events.push_back(Event::MeanCollisionAlert(collisions));
            }
            AnyMessage::HealthMessage(health) => {
                self.events.push_back(Event::HealthMessage {
                    health: health.health_data.health,
                });
            }
            AnyMessage::CameraConstraint(constraint) => {
                self.events.push_back(Event::CameraConstraint {
                    plane: constraint.camera_collide_plane.plane,
                });
            }
            AnyMessage::ViewerFrozenMessage(frozen) => {
                self.events.push_back(Event::ViewerFrozen {
                    frozen: frozen.frozen_data.data,
                });
            }
            AnyMessage::LoadURL(load) => {
                let data = &load.data;
                self.events
                    .push_back(Event::LoadUrl(Box::new(LoadUrlRequest {
                        object_name: trimmed_string(&data.object_name),
                        object_id: ObjectKey::from(data.object_id),
                        owner: crate::types::owner_key_from_wire(
                            data.owner_id,
                            data.owner_is_group,
                        ),
                        message: trimmed_string(&data.message),
                        url: sl_wire::url_from_wire("URL", &trimmed_string(&data.url))?,
                    })));
            }
            AnyMessage::ScriptTeleportRequest(request) => {
                let data = &request.data;
                self.events
                    .push_back(Event::ScriptTeleport(Box::new(ScriptTeleportRequest {
                        object_name: trimmed_string(&data.object_name),
                        region_name: sl_wire::region_name_from_wire(
                            "SimName",
                            &trimmed_string(&data.sim_name),
                        )?,
                        position: RegionCoordinates::new(
                            data.sim_position.x,
                            data.sim_position.y,
                            data.sim_position.z,
                        ),
                        look_at: Direction::new(data.look_at.x, data.look_at.y, data.look_at.z),
                        flags: request.options.first().map_or(0, |option| option.flags),
                    })));
            }
            AnyMessage::AgentDataUpdate(update) => {
                self.events
                    .push_back(Event::ActiveGroupChanged(Box::new(active_group(
                        &update.agent_data,
                    ))));
            }
            AnyMessage::AgentGroupDataUpdate(update) => {
                self.events.push_back(Event::GroupMemberships(
                    update
                        .group_data
                        .iter()
                        .map(group_membership)
                        .collect::<Result<_, _>>()?,
                ));
            }
            AnyMessage::GroupMembersReply(reply) => {
                self.events.push_back(Event::GroupMembers {
                    group_id: GroupKey::from(reply.group_data.group_id),
                    request_id: reply.group_data.request_id,
                    member_count: reply.group_data.member_count,
                    members: reply
                        .member_data
                        .iter()
                        .map(group_member)
                        .collect::<Result<_, _>>()?,
                });
            }
            AnyMessage::GroupRoleDataReply(reply) => {
                self.events.push_back(Event::GroupRoleData {
                    group_id: GroupKey::from(reply.group_data.group_id),
                    request_id: reply.group_data.request_id,
                    role_count: reply.group_data.role_count,
                    roles: reply.role_data.iter().map(group_role).collect(),
                });
            }
            AnyMessage::GroupRoleMembersReply(reply) => {
                self.events.push_back(Event::GroupRoleMembers {
                    group_id: GroupKey::from(reply.agent_data.group_id),
                    request_id: reply.agent_data.request_id,
                    total_pairs: reply.agent_data.total_pairs,
                    pairs: reply
                        .member_data
                        .iter()
                        .map(|pair| GroupRoleMember {
                            role_id: crate::types::optional_key_from_wire(pair.role_id),
                            member_id: AgentKey::from(pair.member_id),
                        })
                        .collect(),
                });
            }
            AnyMessage::GroupTitlesReply(reply) => {
                self.events.push_back(Event::GroupTitles {
                    group_id: GroupKey::from(reply.agent_data.group_id),
                    request_id: reply.agent_data.request_id,
                    titles: reply.group_data.iter().map(group_title).collect(),
                });
            }
            AnyMessage::GroupProfileReply(reply) => {
                self.events
                    .push_back(Event::GroupProfileReceived(Box::new(group_profile(
                        &reply.group_data,
                    )?)));
            }
            AnyMessage::GroupNoticesListReply(reply) => {
                self.events.push_back(Event::GroupNotices {
                    group_id: GroupKey::from(reply.agent_data.group_id),
                    notices: reply.data.iter().map(group_notice).collect(),
                });
            }
            AnyMessage::GroupAccountSummaryReply(reply) => {
                self.events
                    .push_back(Event::GroupAccountSummary(group_account_summary(reply)?));
            }
            AnyMessage::GroupAccountDetailsReply(reply) => {
                self.events
                    .push_back(Event::GroupAccountDetails(group_account_details(reply)));
            }
            AnyMessage::GroupAccountTransactionsReply(reply) => {
                self.events
                    .push_back(Event::GroupAccountTransactions(group_account_transactions(
                        reply,
                    )));
            }
            AnyMessage::GroupActiveProposalItemReply(reply) => {
                self.events.push_back(Event::GroupActiveProposals {
                    group_id: GroupKey::from(reply.agent_data.group_id),
                    transaction_id: reply.transaction_data.transaction_id,
                    total_num_items: reply.transaction_data.total_num_items,
                    proposals: reply
                        .proposal_data
                        .iter()
                        .map(group_active_proposal_item)
                        .collect(),
                });
            }
            AnyMessage::GroupVoteHistoryItemReply(reply) => {
                self.events.push_back(Event::GroupVoteHistory {
                    group_id: GroupKey::from(reply.agent_data.group_id),
                    transaction_id: reply.transaction_data.transaction_id,
                    total_num_items: reply.transaction_data.total_num_items,
                    item: group_vote_history_item(reply),
                });
            }
            AnyMessage::CreateGroupReply(reply) => {
                self.events.push_back(Event::CreateGroupResult {
                    group_id: GroupKey::from(reply.reply_data.group_id),
                    success: reply.reply_data.success,
                    message: trimmed_string(&reply.reply_data.message),
                });
            }
            AnyMessage::JoinGroupReply(reply) => {
                self.events.push_back(Event::JoinGroupResult {
                    group_id: GroupKey::from(reply.group_data.group_id),
                    success: reply.group_data.success,
                });
            }
            AnyMessage::LeaveGroupReply(reply) => {
                self.events.push_back(Event::LeaveGroupResult {
                    group_id: GroupKey::from(reply.group_data.group_id),
                    success: reply.group_data.success,
                });
            }
            AnyMessage::AgentDropGroup(drop) => {
                self.events.push_back(Event::DroppedFromGroup {
                    group_id: GroupKey::from(drop.agent_data.group_id),
                });
            }
            AnyMessage::EjectGroupMemberReply(reply) => {
                self.events.push_back(Event::EjectGroupMemberResult {
                    group_id: GroupKey::from(reply.group_data.group_id),
                    success: reply.eject_data.success,
                });
            }
            AnyMessage::OnlineNotification(notification) => {
                let ids = notification
                    .agent_block
                    .iter()
                    .map(|block| FriendKey::from(block.agent_id))
                    .collect::<Vec<_>>();
                // Record presence: this is one of the only two handlers that
                // mutate `online` (the other is the offline removal below, plus
                // a terminated friendship). No IM / chat path ever touches it.
                self.online.extend(ids.iter().copied());
                if !ids.is_empty() {
                    self.events.push_back(Event::FriendsOnline(ids));
                }
            }
            AnyMessage::OfflineNotification(notification) => {
                let ids = notification
                    .agent_block
                    .iter()
                    .map(|block| FriendKey::from(block.agent_id))
                    .collect::<Vec<_>>();
                for id in &ids {
                    self.online.remove(id);
                    // Presence-driven auto-reset (A7): an offlined friend can no
                    // longer be typing, and a friend who crashed without a
                    // `SessionLeave` would otherwise linger in every group /
                    // conference roster. Drop them from `typing` and
                    // `participants` in each session — the fast path for friends
                    // that grant see-online, layering with the sim's
                    // `SessionLeave` (which covers non-friends too) and the 9 s
                    // typing expiry. No session is removed and no per-session
                    // "offline" marker is stored: a 1:1 persists to logout (its
                    // peer-offline state is `!is_online(peer)`), so `FriendsOnline`
                    // needs no chat action. The `FriendKey` and the roster's
                    // `AgentKey` share the same `Key` identity.
                    let agent = AgentKey(id.0);
                    for chat_session in self.chat_sessions.values_mut() {
                        chat_session.typing.remove(&agent);
                        chat_session.participants.remove(&agent);
                        // An offlined friend can no longer be voice-connected
                        // either (B8): drop them from the voice roster on the same
                        // fan-out, idempotent with the agent-list voice updates.
                        chat_session.voice.members.remove(&agent);
                    }
                }
                if !ids.is_empty() {
                    self.events.push_back(Event::FriendsOffline(ids));
                }
            }
            AnyMessage::ChangeUserRights(change) => {
                // The AgentData id distinguishes the direction: when it is our
                // own id, each rights block echoes a change *we* made to a
                // friend (`agent_related` is the friend); otherwise the friend
                // (`AgentData.AgentID`) changed the rights they grant us, and
                // `agent_related` is our own id.
                let own = self
                    .circuit
                    .as_ref()
                    .map_or_else(Uuid::nil, |circuit| circuit.agent_id.uuid());
                for block in &change.rights {
                    let granted_to_us = change.agent_data.agent_id != own;
                    let friend_id = FriendKey::from(if granted_to_us {
                        change.agent_data.agent_id
                    } else {
                        block.agent_related
                    });
                    let rights = FriendRights(block.related_rights);
                    // Update the cached friend's rights by direction:
                    // `granted_to_us` ⇒ the rights the friend grants us
                    // (`rights_received`); otherwise the echo of our own grant
                    // (`rights_granted`). An unknown friend (a rights change
                    // racing ahead of the add signal) is ignored rather than
                    // synthesising a half-known entry.
                    if let Some(cached) = self.friends.get_mut(&friend_id) {
                        if granted_to_us {
                            cached.rights_received = rights;
                        } else {
                            cached.rights_granted = rights;
                        }
                    }
                    self.events.push_back(Event::FriendRightsChanged {
                        friend_id,
                        rights,
                        granted_to_us,
                    });
                }
            }
            AnyMessage::LogoutReply(_) => {
                self.state = SessionState::Closed;
                self.events.push_back(Event::LoggedOut);
            }
            _ => {
                self.push_diagnostic(Diagnostic::UnhandledMessage {
                    id: message.id(),
                    name: message.name(),
                    child: false,
                });
            }
        }
        Ok(())
    }

    /// Advances all timers to `now`, sending keep-alives, retransmissions, and
    /// acknowledgements and detecting timeouts.
    pub fn handle_timeout(&mut self, now: Instant) {
        if self.run_timeout(now).is_err() {
            self.close(DisconnectReason::ProtocolError);
        }
    }

    /// The fallible body of [`Self::handle_timeout`].
    fn run_timeout(&mut self, now: Instant) -> Result<(), Error> {
        if matches!(self.state, SessionState::Closed) {
            return Ok(());
        }

        // Prune stale "X is typing…" entries: a lost `TypingStop` would otherwise
        // strand the indicator. The accessor stays `now`-free by pruning here on
        // the timed loop (an explicit `TypingStop` still clears immediately).
        for chat_session in self.chat_sessions.values_mut() {
            chat_session
                .typing
                .retain(|_, last_seen| now.saturating_duration_since(*last_seen) < TYPING_TIMEOUT);
        }

        if self
            .circuit
            .as_ref()
            .is_some_and(|c| now >= c.timers.inactivity)
        {
            self.close(DisconnectReason::Timeout);
            return Ok(());
        }

        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.logout)
            .is_some_and(|d| now >= d)
        {
            tracing::warn!("logout timed out waiting for LogoutReply");
            self.push_diagnostic(Diagnostic::ExpectedReplyMissing {
                request: "Logout".to_owned(),
                sequence: None,
            });
            self.state = SessionState::Closed;
            self.events.push_back(Event::LoggedOut);
            return Ok(());
        }

        if matches!(self.state, SessionState::Teleporting)
            && self
                .circuit
                .as_ref()
                .and_then(|c| c.timers.teleport)
                .is_some_and(|d| now >= d)
        {
            self.state = SessionState::Active;
            self.teleport = TeleportPhase::Idle;
            if let Some(circuit) = self.circuit.as_mut() {
                circuit.timers.teleport = None;
            }
            self.events.push_back(Event::TeleportFailed {
                reason: "teleport timed out".to_owned(),
                alert_info: None,
            });
            return Ok(());
        }

        let exhausted = self
            .circuit
            .as_mut()
            .map_or_else(Vec::new, |c| c.process_resends(now));
        if !exhausted.is_empty() {
            for (sequence, name) in exhausted {
                tracing::warn!(
                    sequence = sequence.get(),
                    message = name.unwrap_or("?"),
                    "reliable packet exhausted its retransmission budget"
                );
                self.push_diagnostic(Diagnostic::ExpectedReplyMissing {
                    request: name.map_or_else(|| "reliable packet".to_owned(), str::to_owned),
                    sequence: Some(sequence),
                });
            }
            self.close(DisconnectReason::HandshakeFailed);
            return Ok(());
        }

        // A sit request whose `AvatarSitResponse` never arrived: surface the
        // missing reply (the session keeps running — sit is best-effort).
        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.sit)
            .is_some_and(|d| now >= d)
        {
            self.sit = SitState::NotSitting;
            if let Some(circuit) = self.circuit.as_mut() {
                circuit.timers.sit = None;
            }
            tracing::warn!("sit timed out waiting for AvatarSitResponse");
            self.push_diagnostic(Diagnostic::ExpectedReplyMissing {
                request: "Sit".to_owned(),
                sequence: None,
            });
        }

        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.ack_flush)
            .is_some_and(|d| now >= d)
            && let Some(circuit) = self.circuit.as_mut()
        {
            circuit.flush_acks(now)?;
        }

        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.agent_update)
            .is_some_and(|d| now >= d)
        {
            let controls = self.controls.bits();
            let body = self.body_rotation.clone();
            let head = self.head_rotation.clone();
            let camera = self.camera.clone();
            if let Some(circuit) = self.circuit.as_mut() {
                circuit.send_agent_update(controls, body, head, &camera, now)?;
                circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
            }
        }

        // Send the periodic keep-alive ping on the root circuit; the matching
        // `CompletePingCheck` surfaces as an `Event::Ping` with the round-trip
        // time. Best-effort — a wire-encode failure must not abort the timer loop.
        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.ping)
            .is_some_and(|d| now >= d)
            && let Some(circuit) = self.circuit.as_mut()
        {
            let _ignored = circuit.send_start_ping_check(now);
            circuit.timers.ping = Some(deadline(now, PING_INTERVAL));
        }

        // Keep child circuits healthy: flush owed acks, retransmit, advertise the
        // agent (camera/interest) so the neighbour streams its objects, and drop
        // any that have gone silent (a dead child never fails the session).
        let controls = self.controls.bits();
        let body = self.body_rotation.clone();
        let head = self.head_rotation.clone();
        let camera = self.camera.clone();
        let mut dead = Vec::new();
        let mut child_exhausted = Vec::new();
        for (addr, child) in &mut self.children {
            if now >= child.timers.inactivity {
                dead.push(*addr);
                continue;
            }
            // A child circuit never fails the session, but a reliable packet
            // exhausting its budget there is still worth surfacing.
            child_exhausted.extend(child.process_resends(now));
            if child.timers.ack_flush.is_some_and(|d| now >= d) {
                child.flush_acks(now)?;
            }
            if child.timers.agent_update.is_some_and(|d| now >= d) {
                child.send_agent_update(controls, body.clone(), head.clone(), &camera, now)?;
                child.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
            }
            // Ping the neighbour too, the same cadence as the root; its
            // `CompletePingCheck` surfaces a child-circuit `Event::Ping`. The
            // reply is recorded when it arrives (in `dispatch_child`), so nothing
            // is emitted here. Best-effort — a wire-encode failure is ignored.
            if child.timers.ping.is_some_and(|d| now >= d) {
                let _ignored = child.send_start_ping_check(now);
                child.timers.ping = Some(deadline(now, PING_INTERVAL));
            }
        }
        for (sequence, name) in child_exhausted {
            tracing::warn!(
                sequence = sequence.get(),
                message = name.unwrap_or("?"),
                "reliable packet on a child circuit exhausted its retransmission budget"
            );
            self.push_diagnostic(Diagnostic::ExpectedReplyMissing {
                request: name.map_or_else(|| "reliable packet".to_owned(), str::to_owned),
                sequence: Some(sequence),
            });
        }
        for addr in dead {
            let circuit_id = self.circuit_id_for(addr);
            self.children.remove(&addr);
            self.child_seeds.remove(&addr);
            if let Some(circuit_id) = circuit_id {
                self.forget_sim_objects(circuit_id);
            }
        }

        Ok(())
    }

    /// Enqueues an application message for delivery.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn enqueue(
        &mut self,
        message: AnyMessage,
        reliability: Reliability,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send(&message, reliability, now)?;
        Ok(())
    }

    /// Sends local chat via `ChatFromViewer`. `chat_type` selects the range
    /// (whisper / normal / shout); `channel` is `0` for ordinary local chat or a
    /// non-zero channel for scripted listeners. Incoming chat is surfaced as
    /// [`Event::ChatReceived`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn say(
        &mut self,
        message: &str,
        chat_type: ChatType,
        channel: ChatChannel,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_chat_from_viewer(message, chat_type, channel, now)?;
        Ok(())
    }

    /// Broadcasts a local-chat typing indicator via `ChatFromViewer`: a
    /// `StartTyping` message when `typing`, otherwise `StopTyping` (both with no
    /// text). Nearby viewers show or clear the typing animation; the counterpart
    /// is surfaced to other clients as [`Event::ChatTyping`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_typing(&mut self, typing: bool, now: Instant) -> Result<(), Error> {
        let chat_type = if typing {
            ChatType::StartTyping
        } else {
            ChatType::StopTyping
        };
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_chat_from_viewer("", chat_type, ChatChannel(0), now)?;
        Ok(())
    }

    /// The agent's legacy name (`"First Last"`), used as the `FromAgentName` of
    /// outgoing instant messages.
    fn agent_name(&self) -> String {
        format!(
            "{} {}",
            self.login.request.first_name, self.login.request.last_name
        )
    }

    /// Sends a direct (1:1) instant message to `to_agent_id` via
    /// `ImprovedInstantMessage`. Incoming IMs are surfaced as
    /// [`Event::InstantMessageReceived`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_instant_message(
        &mut self,
        to_agent_id: AgentKey,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let own_agent = self.agent_id();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_instant_message_raw(
            to_agent_id,
            ImDialog::Message,
            message,
            &from_name,
            now,
        )?;
        // Sending a 1:1 IM opens/tracks the direct session keyed by the peer and
        // logs our own message (timestamp `None` — the sans-IO layer has no clock).
        if let Some(sender) = own_agent {
            self.log_outbound_message(
                ChatSessionKind::Direct { peer: to_agent_id },
                SessionMessage {
                    sender,
                    dialog: ImDialog::Message,
                    text: message.to_owned(),
                    timestamp: None,
                },
                now,
            );
        }
        Ok(())
    }

    /// Sends an instant-message typing indicator to `to_agent_id`: an
    /// `IM_TYPING_START` message when `typing`, otherwise `IM_TYPING_STOP`. The
    /// counterpart is surfaced to other clients as [`Event::ImTyping`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_im_typing(
        &mut self,
        to_agent_id: AgentKey,
        typing: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let dialog = if typing {
            ImDialog::TypingStart
        } else {
            ImDialog::TypingStop
        };
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // The viewer sends the literal text "typing" with a typing IM.
        circuit.send_instant_message_raw(to_agent_id, dialog, "typing", &from_name, now)?;
        Ok(())
    }

    /// Offers friendship to `to_agent_id` via an `ImprovedInstantMessage` with
    /// the `IM_FRIENDSHIP_OFFERED` dialog. The recipient sees it as an
    /// [`Event::InstantMessageReceived`] with [`ImDialog::FriendshipOffered`] and
    /// replies with [`Session::accept_friendship`] or
    /// [`Session::decline_friendship`], echoing the offer's
    /// [`InstantMessage::id`](crate::InstantMessage::id) as the transaction id.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_friendship_offer(
        &mut self,
        to_agent_id: AgentKey,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_instant_message_raw(
            to_agent_id,
            ImDialog::FriendshipOffered,
            message,
            &from_name,
            now,
        )?;
        Ok(())
    }

    /// Sends an `AgentUpdate` immediately with the current control state, plus the
    /// transient `extra` control bits (e.g. a one-shot `STAND_UP`). The extra bits
    /// are not persisted, so the next keep-alive clears them.
    fn send_agent_update_now(&mut self, extra: ControlFlags, now: Instant) -> Result<(), Error> {
        let controls = self.controls.union(extra).bits();
        let body = self.body_rotation.clone();
        let head = self.head_rotation.clone();
        let camera = self.camera.clone();
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.send_agent_update(controls, body, head, &camera, now)?;
        } else {
            return Err(Error::NoCircuit);
        }
        Ok(())
    }

    /// Sets the agent control flags advertised in `AgentUpdate`s and sends one
    /// immediately. The simulator moves the agent accordingly (e.g.
    /// [`ControlFlags::AT_POS`] walks forward in the body-rotation direction,
    /// `| `[`ControlFlags::FLY`] flies); pass [`ControlFlags::empty`] to stop.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_controls(&mut self, controls: ControlFlags, now: Instant) -> Result<(), Error> {
        self.controls = controls;
        self.send_agent_update_now(ControlFlags::empty(), now)
    }

    /// Sets the bandwidth throttle (`AgentThrottle`) advertised to the simulator
    /// and sends it immediately on the root circuit. The [`Throttle`] is
    /// remembered and re-sent automatically on every region change (each new
    /// root region starts with the simulator's conservative defaults, which
    /// starve the bulk object / terrain / texture streams). Use a
    /// [`Throttle::preset_1000`]-style preset or a custom split as a starting
    /// point.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_throttle(&mut self, throttle: Throttle, now: Instant) -> Result<(), Error> {
        self.throttle = Some(throttle);
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_throttle(&throttle, now)?;
        // Also advertise it on the child circuits so neighbour regions open up
        // their object streams. Best-effort per child.
        for child in self.children.values_mut() {
            let _ignored = child.send_agent_throttle(&throttle, now);
        }
        Ok(())
    }

    /// Sets the agent's body and head rotation (facing) advertised in
    /// `AgentUpdate`s and sends one immediately. This steers the direction the
    /// agent walks/flies under [`ControlFlags::AT_POS`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_rotation(
        &mut self,
        body_rotation: Rotation,
        head_rotation: Rotation,
        now: Instant,
    ) -> Result<(), Error> {
        self.body_rotation = body_rotation;
        self.head_rotation = head_rotation;
        self.send_agent_update_now(ControlFlags::empty(), now)
    }

    /// Sets the agent's camera viewpoint (position and look axes) advertised in
    /// `AgentUpdate`s and sends one immediately on the root circuit and every
    /// child circuit. The simulator uses it to build the agent's *interest list*
    /// — which objects, avatars and regions it streams — so this steers what the
    /// agent receives toward where it looks rather than the region origin. The
    /// viewpoint is remembered and re-sent on every keep-alive (root and
    /// children) and survives region changes, like the movement controls. Use
    /// [`Camera::looking_at`] to aim at a target, or build a [`Camera`]
    /// directly. The draw distance is set separately with
    /// [`Session::set_draw_distance`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_camera(&mut self, camera: Camera, now: Instant) -> Result<(), Error> {
        self.camera = camera;
        let controls = self.controls.bits();
        let body = self.body_rotation.clone();
        let head = self.head_rotation.clone();
        let camera = self.camera.clone();
        // Advertise on the child circuits too, so neighbour regions update the
        // interest list for this agent. Best-effort per child — a wire-encode
        // failure must not abort the root send.
        for child in self.children.values_mut() {
            let _ignored =
                child.send_agent_update(controls, body.clone(), head.clone(), &camera, now);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_update(controls, body, head, &camera, now)?;
        Ok(())
    }

    /// The agent's current camera viewpoint advertised in `AgentUpdate`s
    /// (defaults to [`Camera::region_center`] until [`Session::set_camera`] is
    /// called).
    #[must_use]
    pub const fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Stands the agent up (from sitting), sending one `AgentUpdate` with the
    /// transient `STAND_UP` control bit. Does not change the persistent controls.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn stand(&mut self, now: Instant) -> Result<(), Error> {
        self.sit = SitState::NotSitting;
        self.send_agent_update_now(ControlFlags::STAND_UP, now)
    }

    /// The object the agent is currently seated on, or [`None`] if it is not
    /// seated on an object (standing, ground-sitting, or a sit request still
    /// awaiting its `AvatarSitResponse`). Set once a [`sit_on`](Self::sit_on)
    /// completes and cleared by [`stand`](Self::stand).
    #[must_use]
    pub const fn seat(&self) -> Option<ObjectKey> {
        match self.sit {
            SitState::Seated { on } => Some(on),
            SitState::NotSitting | SitState::AwaitingResponse => None,
        }
    }

    /// Sits the agent on the ground where it stands, sending one `AgentUpdate`
    /// with the transient `SIT_ON_GROUND` control bit.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn sit_on_ground(&mut self, now: Instant) -> Result<(), Error> {
        self.send_agent_update_now(ControlFlags::SIT_ON_GROUND, now)
    }

    /// Requests to sit on the object `target` at the given region-local `offset`
    /// via `AgentRequestSit`. The simulator replies with an `AvatarSitResponse`,
    /// which the session completes with an `AgentSit` and surfaces as
    /// [`Event::SitResult`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn sit_on(&mut self, target: ObjectKey, offset: Vector, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_request_sit(target.uuid(), offset, now)?;
        circuit.timers.sit = Some(deadline(now, SIT_TIMEOUT));
        self.sit = SitState::AwaitingResponse;
        Ok(())
    }

    /// Walks the agent to the global coordinates `(global_x, global_y, z)` using
    /// the simulator's server-side autopilot (a `GenericMessage` with method
    /// `autopilot`). The X/Y are global metres (region south-west corner plus the
    /// region-local offset — see [`handle_to_global`](crate::handle_to_global));
    /// Z is the region-local height. Movement happens without the client needing
    /// any scene knowledge.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn autopilot_to(
        &mut self,
        global_x: f64,
        global_y: f64,
        z: f64,
        now: Instant,
    ) -> Result<(), Error> {
        let params = [global_x.to_string(), global_y.to_string(), z.to_string()];
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("autopilot", &params, now)?;
        Ok(())
    }

    /// Requests the profile of the avatar `target` via `AvatarPropertiesRequest`.
    /// The simulator replies with [`Event::AvatarProperties`], and usually also
    /// [`Event::AvatarInterests`] and [`Event::AvatarGroups`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_properties(
        &mut self,
        target: AgentKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_properties_request(target.uuid(), now)?;
        Ok(())
    }

    /// Requests the picks of the avatar `target` (a `GenericMessage`
    /// `avatarpicksrequest`). The reply arrives as [`Event::AvatarPicks`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_picks(&mut self, target: AgentKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarpicksrequest", &[target.uuid().to_string()], now)?;
        Ok(())
    }

    /// Requests the agent's private notes about the avatar `target` (a
    /// `GenericMessage` `avatarnotesrequest`). The reply arrives as
    /// [`Event::AvatarNotes`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_notes(&mut self, target: AgentKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarnotesrequest", &[target.uuid().to_string()], now)?;
        Ok(())
    }

    /// Requests the classified ads of the avatar `target` (a `GenericMessage`
    /// `avatarclassifiedsrequest`). The reply arrives as
    /// [`Event::AvatarClassifieds`]; fetch a classified's full details with
    /// [`Session::request_classified_info`]. (#29)
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_classifieds(
        &mut self,
        target: AgentKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message(
            "avatarclassifiedsrequest",
            &[target.uuid().to_string()],
            now,
        )?;
        Ok(())
    }

    /// Requests the full details of one pick (a `GenericMessage`
    /// `pickinforequest`). `creator_id` is the avatar that owns the pick (the
    /// `target_id` from [`Event::AvatarPicks`]) and `pick_id` the pick's id. The
    /// reply arrives as [`Event::PickInfo`]. (#29)
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_pick_info(
        &mut self,
        creator_id: AgentKey,
        pick_id: PickKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message(
            "pickinforequest",
            &[creator_id.to_string(), pick_id.to_string()],
            now,
        )?;
        Ok(())
    }

    /// Requests the full details of one classified ad (`ClassifiedInfoRequest`).
    /// The reply arrives as [`Event::ClassifiedInfo`]. (#29)
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_classified_info(
        &mut self,
        classified_id: ClassifiedKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_classified_info_request(classified_id, now)?;
        Ok(())
    }

    /// Replaces the agent's own profile via `AvatarPropertiesUpdate` (#29). This
    /// overwrites every field, so read the current values with
    /// [`Session::request_avatar_properties`] first and build the
    /// [`ProfileUpdate`] from there.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_profile(&mut self, update: &ProfileUpdate, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_properties_update(update, now)?;
        Ok(())
    }

    /// Replaces the agent's own interests via `AvatarInterestsUpdate` (#29).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_interests(
        &mut self,
        update: &InterestsUpdate,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_interests_update(update, now)?;
        Ok(())
    }

    /// Sets the agent's private notes about the avatar `target` via
    /// `AvatarNotesUpdate` (#29). Read the current notes with
    /// [`Session::request_avatar_notes`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_avatar_notes(
        &mut self,
        target: AgentKey,
        notes: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_notes_update(target.uuid(), notes, now)?;
        Ok(())
    }

    /// Creates or edits one of the agent's picks via `PickInfoUpdate` (#29).
    /// Supply a fresh [`PickUpdate::pick_id`] to create a pick, or an existing
    /// one to edit it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_pick(&mut self, update: &PickUpdate, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_pick_info_update(update, now)?;
        Ok(())
    }

    /// Deletes one of the agent's picks via `PickDelete` (#29).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn delete_pick(&mut self, pick_id: PickKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_pick_delete(pick_id, now)?;
        Ok(())
    }

    /// Deletes any agent's pick via `PickGodDelete` (god-only). `query_id` lets
    /// the dataserver resend the affected agent's pick list. (#29)
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn god_delete_pick(
        &mut self,
        pick_id: PickKey,
        query_id: QueryId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_pick_god_delete(pick_id, query_id.get(), now)?;
        Ok(())
    }

    /// Creates or edits one of the agent's classifieds via
    /// `ClassifiedInfoUpdate` (#29). Supply a fresh
    /// [`ClassifiedUpdate::classified_id`] to create a classified, or an
    /// existing one to edit it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_classified(
        &mut self,
        update: &ClassifiedUpdate,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_classified_info_update(update, now)?;
        Ok(())
    }

    /// Deletes one of the agent's classifieds via `ClassifiedDelete` (#29).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn delete_classified(
        &mut self,
        classified_id: ClassifiedKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_classified_delete(classified_id, now)?;
        Ok(())
    }

    /// Deletes any agent's classified via `ClassifiedGodDelete` (god-only).
    /// `query_id` lets the dataserver resend the affected agent's classified
    /// list. (#29)
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn god_delete_classified(
        &mut self,
        classified_id: ClassifiedKey,
        query_id: QueryId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_classified_god_delete(classified_id, query_id.get(), now)?;
        Ok(())
    }

    /// Sets the friendship rights this agent grants the friend `target` via
    /// `GrantUserRights`. `rights` is a [`FriendRights`] bitfield (combine the
    /// `FriendRights::CAN_*` flags). The simulator echoes the change back as an
    /// [`Event::FriendRightsChanged`] with `granted_to_us = false`.
    ///
    /// The agent's friend list (with the current rights) arrives at login as
    /// [`Event::FriendList`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn grant_user_rights(
        &mut self,
        target: FriendKey,
        rights: FriendRights,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_grant_user_rights(target, rights.0, now)?;
        Ok(())
    }

    /// Ends the friendship with `other` via `TerminateFriendship`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn terminate_friendship(&mut self, other: FriendKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_terminate_friendship(other, now)?;
        Ok(())
    }

    /// Accepts a friendship offer via `AcceptFriendship`. The `transaction_id`
    /// is the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming
    /// [`ImDialog::FriendshipOffered`] IM; `friend_id` is the offering agent (the
    /// `from_agent_id` of that same offer IM the driver is answering);
    /// `calling_card_folder` is the inventory folder to place the new friend's
    /// calling card in (use the Calling Cards system folder, or the inventory
    /// root).
    ///
    /// The accepter receives no `FriendshipAccepted` IM (the simulator sends it
    /// only to the original offerer), so the new friend is added to the buddy
    /// cache here, with the default `CAN_SEE_ONLINE` both ways, from the
    /// caller-supplied `friend_id` rather than a presence signal.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn accept_friendship(
        &mut self,
        transaction_id: TransactionId,
        friend_id: FriendKey,
        calling_card_folder: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_accept_friendship(transaction_id.get(), calling_card_folder.uuid(), now)?;
        self.add_friend(friend_id);
        Ok(())
    }

    /// Inserts a freshly-formed friendship into the buddy cache with the default
    /// `CAN_SEE_ONLINE` grant in both directions (the rights a new friendship is
    /// born with on OpenSim and SL; any later `ChangeUserRights` corrects a
    /// divergence). Used by both live-add paths — we accepting their offer
    /// ([`Session::accept_friendship`]) and them accepting ours (the inbound
    /// `FriendshipAccepted` IM). Does not touch `online`: a new friendship is
    /// not a presence signal.
    fn add_friend(&mut self, friend_id: FriendKey) {
        let default_rights = FriendRights(FriendRights::CAN_SEE_ONLINE);
        self.friends.insert(
            friend_id,
            Friend {
                id: friend_id,
                rights_granted: default_rights,
                rights_received: default_rights,
            },
        );
    }

    /// The buddy-list cache: every current friend, with the friendship rights in
    /// both directions. Live for the whole session (seeded from login, kept up
    /// to date by the friendship add/remove/rights signals). Iteration is
    /// deterministic (ordered by friend id).
    pub fn friends(&self) -> impl Iterator<Item = Friend> + '_ {
        self.friends.values().copied()
    }

    /// Looks up a single friend in the buddy cache, or `None` if `id` is not a
    /// current friend.
    #[must_use]
    pub fn friend(&self, id: FriendKey) -> Option<Friend> {
        self.friends.get(&id).copied()
    }

    /// Whether `friend` is currently **known-online** via an authoritative
    /// presence notification. Absence is *not* provable offline — a friend who
    /// does not grant us `CAN_SEE_ONLINE` never generates a notification, so
    /// read `false` as "offline or not visible to us", never "definitely
    /// offline".
    #[must_use]
    pub fn is_online(&self, friend: FriendKey) -> bool {
        self.online.contains(&friend)
    }

    /// The friends currently known to be online (see [`Session::is_online`] for
    /// the visibility caveat). Iteration is deterministic (ordered by id).
    pub fn online_friends(&self) -> impl Iterator<Item = FriendKey> + '_ {
        self.online.iter().copied()
    }

    /// Get-or-create the chat session for `kind`, stamping its `last_activity` to
    /// `now`, and return it for the caller to mutate. The single lazy-open
    /// primitive: every site that observes traffic for a session (an outbound
    /// send, an inbound session message / participant change, a 1:1 IM) routes
    /// through here, so the open semantics live in one place. A freshly-created
    /// session starts empty (`ChatSession::new`).
    fn chat_session_mut(&mut self, kind: ChatSessionKind, now: Instant) -> &mut ChatSession {
        let session = self
            .chat_sessions
            .entry(kind)
            .or_insert_with(|| ChatSession::new(now));
        session.last_activity = now;
        // Every site routing through here observes real session traffic (an
        // outbound send, an inbound message / participant change, an accept's
        // roster), which is the "joined" signal: promote a pending `Invited`
        // entry to `Joined` (a no-op for an already-joined session). Typing,
        // which must not open or join a session, uses the non-creating
        // `chat_session_get_mut` instead and so never reaches this promotion.
        session.lifecycle = ChatSessionLifecycle::Joined;
        session
    }

    /// Get-or-create the chat session for `kind` as a pending **invitation**,
    /// stamping its `last_activity` to `now` and recording `invite`. Unlike
    /// [`Self::chat_session_mut`] this does **not** promote to `Joined`: it is the
    /// sole path that sets [`ChatSessionLifecycle::Invited`]. An entry that is
    /// already `Joined` (we have since seen traffic / accepted) is left joined —
    /// an invitation never demotes a live session; only a fresh or still-invited
    /// entry takes the invite payload.
    fn mark_chat_session_invited(
        &mut self,
        kind: ChatSessionKind,
        invite: PendingInvite,
        now: Instant,
    ) {
        let fresh = !self.chat_sessions.contains_key(&kind);
        let session = self
            .chat_sessions
            .entry(kind)
            .or_insert_with(|| ChatSession::new(now));
        session.last_activity = now;
        if fresh || matches!(session.lifecycle, ChatSessionLifecycle::Invited(_)) {
            session.lifecycle = ChatSessionLifecycle::Invited(invite);
        }
    }

    /// Logs an inbound conversation message into `kind`'s history, opening the
    /// session if needed, and bumps its unread count unless the message is our
    /// own echo. Shared by the 1:1, group, and conference inbound arms and the
    /// offline-IM drain.
    fn log_inbound_message(
        &mut self,
        kind: ChatSessionKind,
        message: SessionMessage,
        now: Instant,
    ) {
        let own_agent = self.agent_id();
        self.chat_session_mut(kind, now)
            .log_inbound(message, own_agent);
    }

    /// Logs one of our own outbound messages into `kind`'s history, opening the
    /// session if needed, and clears its unread count. Shared by the 1:1, group,
    /// and conference send paths.
    fn log_outbound_message(
        &mut self,
        kind: ChatSessionKind,
        message: SessionMessage,
        now: Instant,
    ) {
        self.chat_session_mut(kind, now).log_outbound(message);
    }

    /// Mutable access to an *already-open* chat session, **without** creating one
    /// — the non-opening counterpart to [`Self::chat_session_mut`]. Used by the
    /// typing fold, which must never conjure a session: a stray "typing…" then
    /// cancelled would otherwise pollute the registry with an empty session.
    /// Returns `None` if no session is open for `kind`.
    fn chat_session_get_mut(&mut self, kind: ChatSessionKind) -> Option<&mut ChatSession> {
        self.chat_sessions.get_mut(&kind)
    }

    /// Read-only access to an already-open chat session, or `None` if none is
    /// open for `kind`. Backs the roster / typing accessors and the typing fold's
    /// session resolution.
    fn chat_session(&self, kind: ChatSessionKind) -> Option<&ChatSession> {
        self.chat_sessions.get(&kind)
    }

    /// Resolves a wire IM-session uuid to the [`ChatSessionKind`] of an *already-
    /// open* session, by matching each registered session's canonical wire id
    /// against `session_id`. Used by CAPS pushes (the voice agent-list updates)
    /// that carry only the bare session uuid with no group/conference
    /// discriminator. Returns `None` when no open session matches (the update is
    /// for a session we are not tracking).
    fn chat_session_kind_for_session_id(&self, session_id: Uuid) -> Option<ChatSessionKind> {
        let own_agent = self.agent_id()?;
        self.chat_sessions
            .keys()
            .copied()
            .find(|kind| kind.canonical_session_id(own_agent) == session_id)
    }

    /// The currently-open chat sessions (1:1 direct, group, ad-hoc conference),
    /// ordered **newest-first** by last activity (the most recently active
    /// session leads), with the typed [`ChatSessionKind`] breaking ties for a
    /// deterministic order. A session opens lazily on the first inbound *or*
    /// outbound traffic and is removed on an explicit `SessionLeave` (1:1 has no
    /// leave, so it persists to logout); the store is grid-level and survives
    /// teleport.
    pub fn chat_sessions(&self) -> impl Iterator<Item = ChatSessionKind> {
        let mut entries: Vec<(ChatSessionKind, Instant)> = self
            .chat_sessions
            .iter()
            .map(|(&kind, session)| (kind, session.last_activity))
            .collect();
        entries.sort_by(|(left_kind, left_at), (right_kind, right_at)| {
            right_at
                .cmp(left_at)
                .then_with(|| left_kind.cmp(right_kind))
        });
        entries.into_iter().map(|(kind, _)| kind)
    }

    /// The participants of `session`: the simulator-reported roster for a group
    /// or conference (which includes self once we have joined), or the implicit
    /// `{ peer }` synthesised from a `Direct` key (a 1:1's roster is never
    /// materialised — its members are `{ self, peer }`). Returns an empty
    /// iterator for a group / conference that is not open.
    pub fn participants(&self, session: ChatSessionKind) -> impl Iterator<Item = AgentKey> {
        let participants: Vec<AgentKey> = match session {
            ChatSessionKind::Direct { peer } => vec![peer],
            group_or_conference => self
                .chat_session(group_or_conference)
                .map(|chat_session| chat_session.participants.iter().copied().collect())
                .unwrap_or_default(),
        };
        participants.into_iter()
    }

    /// The avatars currently typing in `session` (remote typers only — our own
    /// outbound typing is never mirrored). Stale entries are pruned on the timed
    /// loop, so this read is `now`-free; an unopened session yields nothing.
    pub fn typing(&self, session: ChatSessionKind) -> impl Iterator<Item = AgentKey> {
        let typing: Vec<AgentKey> = self
            .chat_session(session)
            .map(|chat_session| chat_session.typing.keys().copied().collect())
            .unwrap_or_default();
        typing.into_iter()
    }

    /// The logged conversation history of `session`, oldest-first (insertion
    /// order is the sequence), bounded to the most recent `HISTORY_CAP` messages.
    /// Yields nothing for a session that is not open.
    pub fn history(&self, session: ChatSessionKind) -> impl Iterator<Item = &SessionMessage> {
        self.chat_session(session)
            .into_iter()
            .flat_map(|chat_session| chat_session.history.iter())
    }

    /// The number of unread inbound messages in `session` (zero if the session is
    /// not open). Reset by our own outbound send and by [`Self::mark_session_read`].
    #[must_use]
    pub fn unread(&self, session: ChatSessionKind) -> u32 {
        self.chat_session(session)
            .map_or(0, |chat_session| chat_session.unread)
    }

    /// The total unread message count across every open chat session — the badge
    /// number a viewer shows on its conversations button. Saturates rather than
    /// overflowing.
    #[must_use]
    pub fn total_unread(&self) -> u32 {
        self.chat_sessions
            .values()
            .map(|chat_session| chat_session.unread)
            .fold(0, u32::saturating_add)
    }

    /// Marks `session` as read, resetting its unread count to zero. A no-op if no
    /// session is open for `session` (nothing has been received to read). Backs
    /// [`Command::MarkSessionRead`](crate::Command::MarkSessionRead).
    pub fn mark_session_read(&mut self, session: ChatSessionKind) {
        if let Some(chat_session) = self.chat_session_get_mut(session) {
            chat_session.unread = 0;
        }
    }

    /// The lifecycle of `session` — whether it is a still-pending invitation
    /// ([`ChatSessionLifecycle::Invited`], carrying the
    /// [`PendingInvite`](crate::PendingInvite)) or one we have joined
    /// ([`ChatSessionLifecycle::Joined`]) — or `None` if no session is open for
    /// `session`. The pending invitations are exactly the open sessions whose
    /// lifecycle is `Invited`.
    #[must_use]
    pub fn chat_session_lifecycle(
        &self,
        session: ChatSessionKind,
    ) -> Option<&ChatSessionLifecycle> {
        self.chat_session(session)
            .map(|chat_session| &chat_session.lifecycle)
    }

    /// Builds the light [`ChatSessionInfo`] snapshot of one session, composing the
    /// lower-level lifecycle / participant / typing / unread accessors. History
    /// and the activity stamp are deliberately left out (see [`ChatSessionInfo`]).
    fn chat_session_info(&self, kind: ChatSessionKind) -> ChatSessionInfo {
        let lifecycle = self
            .chat_session_lifecycle(kind)
            .map_or(ChatLifecycleView::Joined, ChatLifecycleView::from_lifecycle);
        ChatSessionInfo {
            kind,
            lifecycle,
            participants: self.participants(kind).collect(),
            typing: self.typing(kind).collect(),
            unread: self.unread(kind),
            has_voice: self.session_has_voice(kind),
            voice_joined: self.session_voice_joined(kind),
            voice_members: self.session_voice_members(kind).collect(),
        }
    }

    /// A light snapshot of every open chat session, ordered **newest-first** by
    /// last activity (the same order as [`Self::chat_sessions`]) and carrying no
    /// history — the read-out behind
    /// [`Command::QueryChatSessions`](crate::Command::QueryChatSessions) /
    /// [`Event::ChatSessions`](crate::Event::ChatSessions). A bevy system reads
    /// this directly off the borrowed `Session`; the tokio / REPL runtimes pull it
    /// over the query/reply bridge. The per-session history is fetched separately
    /// and one bounded page at a time via [`Self::history_page`].
    pub fn chat_sessions_info(&self) -> impl Iterator<Item = ChatSessionInfo> + '_ {
        self.chat_sessions()
            .map(|kind| self.chat_session_info(kind))
    }

    /// A snapshot of the buddy cache paired with each friend's live online flag —
    /// the read-out behind [`Command::QueryFriends`](crate::Command::QueryFriends)
    /// / [`Event::FriendsSnapshot`](crate::Event::FriendsSnapshot). Iteration is
    /// deterministic (ordered by friend id). `online` carries the
    /// [`Self::is_online`] visibility caveat.
    pub fn friends_presence(&self) -> impl Iterator<Item = FriendPresence> + '_ {
        self.friends().map(|friend| FriendPresence {
            online: self.is_online(friend.id),
            friend,
        })
    }

    /// The number of messages currently retained in `session`'s **in-memory**
    /// history ring (`0` for an unknown session). The runtime's file-backed paging
    /// uses it as the boundary between the in-memory tail and the on-disk archive:
    /// a [`MessageCursor`] whose count reaches this length has exhausted memory and
    /// continues from the transcript.
    #[must_use]
    pub fn history_len(&self, session: ChatSessionKind) -> usize {
        self.chat_session(session)
            .map_or(0, |chat| chat.history.len())
    }

    /// One bounded, **newest-first** page of `session`'s in-memory conversation
    /// history, plus a `prev` cursor for the next (older) page — the read-out
    /// behind [`Command::QueryChatHistoryPage`](crate::Command::QueryChatHistoryPage)
    /// / [`Event::ChatHistoryPage`](crate::Event::ChatHistoryPage).
    ///
    /// `before` is `None` for the newest page, or a `prev` cursor returned by an
    /// earlier call to continue older. At most `limit` messages are yielded,
    /// newest first; the returned cursor is `Some` while older in-memory messages
    /// remain and `None` once the window reaches the oldest retained message (the
    /// runtime's on-disk chat log continues older pages from there). The iterator
    /// borrows the session, so a bevy reader pages with zero copies; the channel
    /// runtimes clone the bounded window into an `Arc<[_]>`.
    pub fn history_page(
        &self,
        session: ChatSessionKind,
        before: Option<MessageCursor>,
        limit: usize,
    ) -> (
        impl Iterator<Item = &SessionMessage> + '_,
        Option<MessageCursor>,
    ) {
        let consumed = before.map_or(0, MessageCursor::consumed);
        let history = self.chat_session(session).map(|chat| &chat.history);
        let len = history.map_or(0, VecDeque::len);
        let take = len.saturating_sub(consumed).min(limit);
        let next = consumed.saturating_add(take);
        let prev = (next < len).then(|| MessageCursor::new(next));
        let page = history
            .into_iter()
            .flat_map(|entries| entries.iter().rev())
            .skip(consumed)
            .take(take);
        (page, prev)
    }

    /// Accepts a pending chat-session invitation, promoting its registry entry to
    /// [`Joined`](ChatSessionLifecycle::Joined) (get-or-creating it as joined if
    /// it is somehow absent). This is the pure-state half of the accept; the
    /// runtime additionally POSTs `{ "method": "accept invitation", "session-id" }`
    /// to the `ChatSessionRequest` capability when present (whose reply roster
    /// seeds the participants), and on a grid without the cap relies on the
    /// optimistic local join (the simulator added us when it routed the invite).
    /// `from_group` selects whether `session_id` keys a [`Group`](ChatSessionKind::Group)
    /// or a [`Conference`](ChatSessionKind::Conference) session, mirroring the
    /// [`Event::ConferenceInvited`](crate::Event::ConferenceInvited) being answered.
    pub fn accept_chat_invite(&mut self, session_id: ImSessionId, from_group: bool, now: Instant) {
        let kind = invite_session_kind(session_id, from_group);
        // `chat_session_mut` stamps activity and promotes `Invited` → `Joined`.
        let _session = self.chat_session_mut(kind, now);
    }

    /// Declines a pending chat-session invitation, **removing** its registry
    /// entry (the registry tracks only live sessions). This is the pure-state
    /// half of the decline; the runtime additionally POSTs `{ "method": "decline
    /// invitation", "session-id" }` to the `ChatSessionRequest` capability when
    /// present, or sends a UDP `SessionLeave` as the fallback on a grid without
    /// the cap. `from_group` selects [`Group`](ChatSessionKind::Group) vs
    /// [`Conference`](ChatSessionKind::Conference), as for [`Self::accept_chat_invite`].
    pub fn decline_chat_invite(
        &mut self,
        session_id: ImSessionId,
        from_group: bool,
        _now: Instant,
    ) {
        let kind = invite_session_kind(session_id, from_group);
        let _removed = self.chat_sessions.remove(&kind);
    }

    /// Records that we have joined `session`'s voice channel — the pure-state half
    /// of [`Command::JoinSessionVoice`](crate::Command::JoinSessionVoice), set
    /// **optimistically** at the signalling level (there is no audio ack, exactly
    /// as the text [`Joined`](ChatSessionLifecycle::Joined) lifecycle is
    /// optimistic). Get-or-creates the session (joining voice implies an active
    /// conversation — a 1:1 P2P call may have no text traffic yet) and sets both
    /// `voice.joined` and `voice.has_voice`. The runtime additionally provisions a
    /// voice account and signals into the channel via `ChatSessionRequest`.
    pub fn join_session_voice(&mut self, session: ChatSessionKind, now: Instant) {
        let chat_session = self.chat_session_mut(session, now);
        chat_session.voice.joined = true;
        chat_session.voice.has_voice = true;
    }

    /// Records that we have left `session`'s voice channel — the pure-state half of
    /// [`Command::LeaveSessionVoice`](crate::Command::LeaveSessionVoice). Clears
    /// `voice.joined` (optimistically), leaving the rest of the session — its text
    /// channel, history, and roster — intact: leaving voice is not leaving the
    /// conversation. A no-op if no session is open for `session`. The runtime
    /// additionally signals the voice decline / logout on the wire.
    pub fn leave_session_voice(&mut self, session: ChatSessionKind) {
        if let Some(chat_session) = self.chat_session_get_mut(session) {
            chat_session.voice.joined = false;
        }
    }

    /// Whether `session` offers a voice channel (signalling only) — `false` for a
    /// session that is not open or has seen no voice invite / accept / join.
    #[must_use]
    pub fn session_has_voice(&self, session: ChatSessionKind) -> bool {
        self.chat_session(session)
            .is_some_and(|chat_session| chat_session.voice.has_voice)
    }

    /// The voice-channel coordinates of `session` (the room uri / credentials /
    /// backend / handle), or `None` when the session is not open or carries no
    /// known channel coordinates yet.
    #[must_use]
    pub fn session_voice_channel(&self, session: ChatSessionKind) -> Option<&VoiceChannelInfo> {
        self.chat_session(session)
            .and_then(|chat_session| chat_session.voice.channel.as_ref())
    }

    /// Whether *we* have joined `session`'s voice channel (optimistic, at the
    /// signalling level) — `false` for a session that is not open.
    #[must_use]
    pub fn session_voice_joined(&self, session: ChatSessionKind) -> bool {
        self.chat_session(session)
            .is_some_and(|chat_session| chat_session.voice.joined)
    }

    /// The avatars currently voice-connected in `session` (never the speaking
    /// state). For a `Direct` 1:1 the implicit voice pair is `{ self, peer }` once
    /// we have joined; for a group / conference it is the subset the agent-list
    /// voice updates report. Empty for a session that is not open or has no voice
    /// members.
    pub fn session_voice_members(
        &self,
        session: ChatSessionKind,
    ) -> impl Iterator<Item = AgentKey> {
        let members: Vec<AgentKey> = match session {
            // A 1:1 P2P voice call's members are implicitly { self, peer } once we
            // have joined; the agent-list updates never materialise a roster for a
            // direct session.
            ChatSessionKind::Direct { peer } => self
                .chat_session(session)
                .filter(|chat_session| chat_session.voice.joined)
                .map(|_| {
                    let mut pair = vec![peer];
                    if let Some(own) = self.agent_id() {
                        pair.push(own);
                    }
                    pair
                })
                .unwrap_or_default(),
            group_or_conference => self
                .chat_session(group_or_conference)
                .map(|chat_session| chat_session.voice.members.iter().copied().collect())
                .unwrap_or_default(),
        };
        members.into_iter()
    }

    /// Declines a friendship offer via `DeclineFriendship`. The `transaction_id`
    /// is the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming
    /// [`ImDialog::FriendshipOffered`] IM.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn decline_friendship(
        &mut self,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_decline_friendship(transaction_id.get(), now)?;
        Ok(())
    }

    /// Offers this agent's calling card to `to_agent_id` via `OfferCallingCard`
    /// — a reference card to this avatar, filed in the recipient's Calling Cards
    /// folder (this is *not* a friendship request; use
    /// [`Session::send_friendship_offer`] for that). The recipient sees it as an
    /// [`Event::CallingCardOffered`] and replies with
    /// [`Session::accept_calling_card`] or [`Session::decline_calling_card`],
    /// echoing `transaction_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn offer_calling_card(
        &mut self,
        to_agent_id: AgentKey,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_offer_calling_card(to_agent_id, transaction_id.get(), now)?;
        Ok(())
    }

    /// Accepts a calling-card offer via `AcceptCallingCard`. The `transaction_id`
    /// is the one echoed by the incoming [`Event::CallingCardOffered`];
    /// `calling_card_folder` is the inventory folder to file the new card in (use
    /// the Calling Cards system folder, or the inventory root). The offering
    /// agent sees an [`Event::CallingCardAccepted`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn accept_calling_card(
        &mut self,
        transaction_id: TransactionId,
        calling_card_folder: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_accept_calling_card(transaction_id.get(), calling_card_folder.uuid(), now)?;
        Ok(())
    }

    /// Declines a calling-card offer via `DeclineCallingCard`. The
    /// `transaction_id` is the one echoed by the incoming
    /// [`Event::CallingCardOffered`]. The offering agent sees an
    /// [`Event::CallingCardDeclined`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn decline_calling_card(
        &mut self,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_decline_calling_card(transaction_id.get(), now)?;
        Ok(())
    }

    /// Sets the agent's active group (`ActivateGroup`): `Some(group)` makes that
    /// group active, `None` clears the active group (sent as the nil group id on
    /// the wire). The simulator confirms with an [`Event::ActiveGroupChanged`]
    /// whose [`active_group_id`](crate::ActiveGroup::active_group_id) mirrors the
    /// `Option`. The agent's memberships arrive at login as
    /// [`Event::GroupMemberships`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn activate_group(
        &mut self,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // The nil group id is the wire convention for "no active group".
        let group_id = group_id.unwrap_or_else(|| GroupKey::from(Uuid::nil()));
        circuit.send_activate_group(group_id, now)?;
        Ok(())
    }

    /// Requests a group's member roster (`GroupMembersRequest`). The reply
    /// arrives as [`Event::GroupMembers`] (the simulator may split large rosters
    /// across several events).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_members(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_members_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's roles (`GroupRoleDataRequest`). The reply arrives as
    /// [`Event::GroupRoleData`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_roles(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_role_data_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's role↔member pairings (`GroupRoleMembersRequest`). The
    /// reply arrives as [`Event::GroupRoleMembers`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_role_members(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_role_members_request(group_id, now)?;
        Ok(())
    }

    /// Requests the agent's selectable titles in a group (`GroupTitlesRequest`).
    /// The reply arrives as [`Event::GroupTitles`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_titles(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_titles_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's profile (`GroupProfileRequest`). The reply arrives as
    /// [`Event::GroupProfileReceived`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_profile(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_profile_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's notice list (`GroupNoticesListRequest`). The reply
    /// arrives as [`Event::GroupNotices`] (headers only; fetch a notice's body
    /// with [`Session::request_group_notice`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_notices(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_notices_list_request(group_id, now)?;
        Ok(())
    }

    /// Requests a single group notice's full body and attachment
    /// (`GroupNoticeRequest`); the notice is delivered as an
    /// [`Event::InstantMessageReceived`] with the `GroupNotice` dialog.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_notice(
        &mut self,
        notice_id: GroupNoticeKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_notice_request(notice_id, now)?;
        Ok(())
    }

    /// Creates a new group (`CreateGroupRequest`). The result arrives as
    /// [`Event::CreateGroupResult`] (with the new group id on success). Note the
    /// grid may charge an L$ creation fee.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn create_group(&mut self, params: &CreateGroupParams, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_group_request(params, now)?;
        Ok(())
    }

    /// Edits an existing group's profile (`UpdateGroupInfo`): charter, insignia,
    /// search visibility, membership fee, enrollment, and publish flags. The
    /// agent must hold the group's change-identity power. A group cannot be
    /// renamed, so [`UpdateGroupInfoParams`] carries no name.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_group_info(
        &mut self,
        params: &UpdateGroupInfoParams,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_group_info(params, now)?;
        Ok(())
    }

    /// Sets the agent's active title within a group (`GroupTitleUpdate`): the
    /// title shown above the avatar's name is the one carried by `title_role_id`
    /// (a group role the agent belongs to; query the choices with
    /// [`Session::request_group_titles`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_group_title(
        &mut self,
        group_id: GroupKey,
        title_role_id: GroupRoleKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_title_update(group_id, title_role_id, now)?;
        Ok(())
    }

    /// Joins an open-enrollment group (`JoinGroupRequest`). The result arrives as
    /// [`Event::JoinGroupResult`]. Closed groups require an invitation instead
    /// (see [`Session::invite_to_group`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn join_group(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_join_group_request(group_id, now)?;
        Ok(())
    }

    /// Leaves a group (`LeaveGroupRequest`). The result arrives as
    /// [`Event::LeaveGroupResult`], followed by an [`Event::DroppedFromGroup`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn leave_group(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_leave_group_request(group_id, now)?;
        Ok(())
    }

    /// Invites agents to a group (`InviteGroupRequest`). Each invitee is an
    /// `(invitee_id, role_id)` pair; use [`Uuid::nil`] for the role to assign the
    /// default "Everyone" role. Invitees receive a group-invitation IM.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn invite_to_group(
        &mut self,
        group_id: GroupKey,
        invitees: &[(AgentKey, GroupRoleKey)],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_invite_group_request(group_id, invitees, now)?;
        Ok(())
    }

    /// Sets whether the agent accepts notices from a group and lists it in their
    /// profile (`SetGroupAcceptNotices`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_group_accept_notices(
        &mut self,
        group_id: GroupKey,
        accept_notices: bool,
        list_in_profile: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_set_group_accept_notices(group_id, accept_notices, list_in_profile, now)?;
        Ok(())
    }

    /// Sets the agent's L$ contribution to a group (`SetGroupContribution`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_group_contribution(
        &mut self,
        group_id: GroupKey,
        contribution: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_set_group_contribution(group_id, contribution, now)?;
        Ok(())
    }

    /// Starts (joins) a group's IM session (`ImprovedInstantMessage`,
    /// `IM_SESSION_GROUP_START`), so the agent receives the group's chat. Group
    /// messages arrive as [`Event::GroupSessionMessage`]. Sending a message with
    /// [`Session::send_group_message`] also joins the session implicitly.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn start_group_session(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(
            group_id,
            ImDialog::SessionGroupStart,
            "",
            &from_name,
            now,
        )?;
        // Starting a group session opens/tracks it (keyed by the group id).
        self.chat_session_mut(ChatSessionKind::Group { group_id }, now);
        Ok(())
    }

    /// Sends a message to a group's IM session (`ImprovedInstantMessage`,
    /// `IM_SESSION_SEND`, session id = group id). Other members receive it as
    /// [`Event::GroupSessionMessage`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_group_message(
        &mut self,
        group_id: GroupKey,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let own_agent = self.agent_id();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(group_id, ImDialog::SessionSend, message, &from_name, now)?;
        // Sending into a group session opens/tracks it (keyed by the group id) and
        // logs our own message.
        if let Some(sender) = own_agent {
            self.log_outbound_message(
                ChatSessionKind::Group { group_id },
                SessionMessage {
                    sender,
                    dialog: ImDialog::SessionSend,
                    text: message.to_owned(),
                    timestamp: None,
                },
                now,
            );
        }
        Ok(())
    }

    /// Leaves a group's IM session (`ImprovedInstantMessage`,
    /// `IM_SESSION_LEAVE`), so the agent stops receiving the group's chat without
    /// leaving the group itself.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn leave_group_session(&mut self, group_id: GroupKey, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(group_id, ImDialog::SessionLeave, "", &from_name, now)?;
        // Leaving removes the entry — the registry tracks only live sessions.
        self.chat_sessions
            .remove(&ChatSessionKind::Group { group_id });
        Ok(())
    }

    // -- Group management edits (#31) --------------------------------------

    /// Creates, updates, or deletes group roles (`GroupRoleUpdate`), one
    /// [`GroupRoleEdit`] per role. Each edit's [`update_type`] selects whether
    /// the role is created, has its data/powers updated, or is deleted; the
    /// `powers` bitfield is built from the [`group_powers`](crate::group_powers)
    /// constants. The agent needs the matching role-management powers (e.g.
    /// [`group_powers::ROLE_CREATE`](crate::group_powers::ROLE_CREATE)). There is
    /// no direct reply; re-request the roles with
    /// [`Session::request_group_roles`] to observe the change.
    ///
    /// [`update_type`]: GroupRoleEdit::update_type
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_group_roles(
        &mut self,
        group_id: GroupKey,
        roles: &[GroupRoleEdit],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_role_update(group_id, roles, now)?;
        Ok(())
    }

    /// Adds members to or removes members from group roles (`GroupRoleChanges`),
    /// one [`GroupRoleMemberChange`] per assignment. The agent needs the
    /// matching powers (e.g.
    /// [`group_powers::ROLE_ASSIGN_MEMBER`](crate::group_powers::ROLE_ASSIGN_MEMBER)).
    /// There is no direct reply; re-request the role members with
    /// [`Session::request_group_role_members`] to observe the change.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn change_group_role_members(
        &mut self,
        group_id: GroupKey,
        changes: &[GroupRoleMemberChange],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_role_changes(group_id, changes, now)?;
        Ok(())
    }

    /// Ejects each agent in `member_ids` from `group_id`
    /// (`EjectGroupMemberRequest`). The agent needs
    /// [`group_powers::MEMBER_EJECT`](crate::group_powers::MEMBER_EJECT). The
    /// result arrives as [`Event::EjectGroupMemberResult`]. An agent cannot eject
    /// itself (use [`Session::leave_group`] instead).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn eject_group_members(
        &mut self,
        group_id: GroupKey,
        member_ids: &[AgentKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_eject_group_members(group_id, member_ids, now)?;
        Ok(())
    }

    /// Marks each gesture in `gestures` active for this session
    /// (`ActivateGestures`), so the simulator preloads them and they fire on
    /// their trigger words/keys. The gesture assets are uploaded separately;
    /// this only toggles which are live. There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn activate_gestures(
        &mut self,
        gestures: &[GestureActivation],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_activate_gestures(gestures, now)?;
        Ok(())
    }

    /// Marks each gesture named in `item_ids` inactive for this session
    /// (`DeactivateGestures`). There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn deactivate_gestures(
        &mut self,
        item_ids: &[InventoryKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let wire: Vec<Uuid> = item_ids.iter().map(InventoryKey::uuid).collect();
        circuit.send_deactivate_gestures(&wire, now)?;
        Ok(())
    }

    /// Chooses whether the avatar runs or walks for ground movement
    /// (`SetAlwaysRun`). There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_always_run(&mut self, mode: MovementMode, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_set_always_run(mode, now)?;
        Ok(())
    }

    /// Tells the simulator the viewer has stalled and is not reading the network
    /// (`AgentPause`), so it stops streaming updates until [`Session::resume_agent`].
    /// There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn pause_agent(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_pause(now)?;
        Ok(())
    }

    /// Tells the simulator the viewer has resumed reading the network
    /// (`AgentResume`) after a [`Session::pause_agent`]. There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn resume_agent(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_resume(now)?;
        Ok(())
    }

    /// Updates the agent's vertical field of view (`AgentFOV`), in radians; the
    /// simulator uses it for interest-list culling. There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_agent_fov(&mut self, vertical_angle: f32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_fov(vertical_angle, now)?;
        Ok(())
    }

    /// Updates the agent's viewport size in pixels (`AgentHeightWidth`), sent
    /// when the viewer window is created or resized. There is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_agent_size(&mut self, height: u16, width: u16, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_height_width(height, width, now)?;
        Ok(())
    }

    /// Forcibly releases any agent movement controls a script has taken
    /// (`ForceScriptControlRelease`), reversing a
    /// [`Event::ScriptControlChange`](crate::Event::ScriptControlChange). There
    /// is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn release_script_controls(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_force_script_control_release(now)?;
        // Clear the taken-controls mirror to empty on send, not on the echo:
        // OpenSim's echo is `Controls = 0xFFFFFFFF, PassToAgent = false`, which
        // would decrement only `consumed` and leak the `passed_on` counts. The
        // later echo's clamped decrement from an already-empty map is a harmless
        // no-op. The `TAKE_CONTROLS` *grant* persists (a script may re-take), so
        // `script_grants` is untouched.
        self.taken_controls.consumed.clear();
        self.taken_controls.passed_on.clear();
        Ok(())
    }

    /// Requests a group's financial summary (`GroupAccountSummaryRequest`) for
    /// the accounting interval selected by `interval_days`/`current_interval` (0 =
    /// current, 1 = previous). The reply arrives as
    /// [`Event::GroupAccountSummary`]. The `request_id` is echoed back for
    /// correlation. The agent needs group accountability powers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_account_summary(
        &mut self,
        group_id: GroupKey,
        request_id: GroupRequestId,
        interval_days: i32,
        current_interval: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_account_summary_request(
            group_id,
            request_id.get(),
            interval_days,
            current_interval,
            now,
        )?;
        Ok(())
    }

    /// Requests a group's itemised accounting detail
    /// (`GroupAccountDetailsRequest`). The reply arrives as
    /// [`Event::GroupAccountDetails`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_account_details(
        &mut self,
        group_id: GroupKey,
        request_id: GroupRequestId,
        interval_days: i32,
        current_interval: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_account_details_request(
            group_id,
            request_id.get(),
            interval_days,
            current_interval,
            now,
        )?;
        Ok(())
    }

    /// Requests a group's transaction log (`GroupAccountTransactionsRequest`). The
    /// reply arrives as [`Event::GroupAccountTransactions`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_account_transactions(
        &mut self,
        group_id: GroupKey,
        request_id: GroupRequestId,
        interval_days: i32,
        current_interval: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_account_transactions_request(
            group_id,
            request_id.get(),
            interval_days,
            current_interval,
            now,
        )?;
        Ok(())
    }

    /// Requests a group's active proposals (`GroupActiveProposalsRequest`). The
    /// reply arrives as [`Event::GroupActiveProposals`]. The `transaction_id` is
    /// echoed back for correlation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_active_proposals(
        &mut self,
        group_id: GroupKey,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_active_proposals_request(group_id, transaction_id.get(), now)?;
        Ok(())
    }

    /// Requests a group's vote history (`GroupVoteHistoryRequest`). Each finished
    /// proposal arrives as [`Event::GroupVoteHistory`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_vote_history(
        &mut self,
        group_id: GroupKey,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_vote_history_request(group_id, transaction_id.get(), now)?;
        Ok(())
    }

    /// Starts a new group proposal/vote (`StartGroupProposal`): `quorum` votes are
    /// required for the result to count, `majority` (0.0–1.0) to pass, voting open
    /// for `duration` seconds. It then appears in the group's active proposals.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn start_group_proposal(
        &mut self,
        group_id: GroupKey,
        quorum: i32,
        majority: f32,
        duration: i32,
        proposal_text: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_start_group_proposal(
            group_id,
            quorum,
            majority,
            duration,
            proposal_text,
            now,
        )?;
        Ok(())
    }

    /// Casts a vote on an active group proposal (`GroupProposalBallot`):
    /// `vote_cast` is the choice (e.g. `"yes"`/`"no"`/`"abstain"`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn cast_group_proposal_ballot(
        &mut self,
        proposal_id: ProposalVoteId,
        group_id: GroupKey,
        vote_cast: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_proposal_ballot(proposal_id, group_id, vote_cast, now)?;
        Ok(())
    }

    /// Posts a group notice (`ImprovedInstantMessage`, `IM_GROUP_NOTICE`). The
    /// `subject` and `message` are joined with a `|` on the wire, as the viewer
    /// sends. An optional [`GroupNoticeAttachment`] attaches an inventory item
    /// (which must be copy+transfer); it is packed into the binary bucket as the
    /// viewer's `<? LLSD/XML ?>` `{ item_id, owner_id }` stream, with the empty
    /// bucket sent when there is no attachment. The agent needs
    /// [`group_powers::NOTICES_SEND`](crate::group_powers::NOTICES_SEND). The
    /// grid relays the notice to every member who accepts notices.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_group_notice(
        &mut self,
        group_id: GroupKey,
        subject: &str,
        message: &str,
        attachment: Option<GroupNoticeAttachment>,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        // The viewer joins subject and message with a single '|'.
        let subject_and_message = format!("{subject}|{message}");
        // An attachment is the LLSD bucket; otherwise the one-byte empty bucket.
        let binary_bucket = attachment.map_or_else(
            || vec![0_u8],
            |attachment| build_group_notice_bucket(attachment.item_id, attachment.owner_id),
        );
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: group_id.uuid(),
                from_group: false,
                dialog: ImDialog::GroupNotice,
                id: Uuid::nil(),
                message: &subject_and_message,
                from_name: &from_name,
                binary_bucket,
            },
            now,
        )?;
        Ok(())
    }

    // -- Complete the IM surface (#28) -------------------------------------

    /// Offers a teleport ("lure") to each agent in `targets` via `StartLure`.
    /// Each recipient receives an [`Event::InstantMessageReceived`] with
    /// [`ImDialog::LureUser`]; its [`InstantMessage::id`](crate::InstantMessage::id) is the lure id they
    /// pass to [`Session::accept_teleport_lure`] to teleport to this agent (or
    /// to [`Session::decline_teleport_lure`] to refuse).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn offer_teleport(
        &mut self,
        targets: &[AgentKey],
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let targets: Vec<Uuid> = targets.iter().map(AgentKey::uuid).collect();
        circuit.send_start_lure(&targets, message, now)?;
        Ok(())
    }

    /// Accepts a teleport lure via `TeleportLureRequest`, teleporting this agent
    /// to the location the lure describes. `lure_id` is the
    /// [`InstantMessage::id`](crate::InstantMessage::id) of the received [`ImDialog::LureUser`] IM. This
    /// drives the same teleport handover as
    /// [`Session::teleport_to`](crate::Session::teleport_to): on success the
    /// session re-establishes at the destination and emits
    /// [`Event::RegionChanged`]; on failure it emits [`Event::TeleportFailed`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotActive`] if the session is not active,
    /// [`Error::NoCircuit`] if no circuit is established, or [`Error::Wire`] if
    /// the request fails to encode.
    pub fn accept_teleport_lure(&mut self, lure_id: LureId, now: Instant) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Active) {
            return Err(Error::NotActive);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_teleport_lure_request(
            lure_id.get(),
            TeleportFlags(TeleportFlags::VIA_LURE),
            now,
        )?;
        circuit.timers.teleport = Some(deadline(now, TELEPORT_TIMEOUT));
        // Best-effort destination hint; a cross-region lure's TeleportFinish
        // carries the authoritative handle, so a non-fake-parcel id is harmless.
        self.teleport = TeleportPhase::Requested {
            target: parse_lure_region_handle(lure_id.get()),
        };
        self.state = SessionState::Teleporting;
        Ok(())
    }

    /// Declines a teleport lure via an `IM_LURE_DECLINED` instant message to the
    /// offerer. `from_agent_id` is the offer IM's [`InstantMessage::from_agent_id`](crate::InstantMessage::from_agent_id)
    /// and `lure_id` its [`InstantMessage::id`](crate::InstantMessage::id).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn decline_teleport_lure(
        &mut self,
        from_agent_id: AgentKey,
        lure_id: LureId,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: from_agent_id.uuid(),
                from_group: false,
                dialog: ImDialog::LureDeclined,
                id: lure_id.get(),
                message: "",
                from_name: &from_name,
                binary_bucket: Vec::new(),
            },
            now,
        )?;
        Ok(())
    }

    /// Requests a teleport from `to_agent_id` (asks them to offer this agent a
    /// teleport) via an `IM_TELEPORT_REQUEST` instant message.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn request_teleport(
        &mut self,
        to_agent_id: AgentKey,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: to_agent_id.uuid(),
                from_group: false,
                dialog: ImDialog::TeleportRequest,
                id: Uuid::nil(),
                message,
                from_name: &from_name,
                binary_bucket: Vec::new(),
            },
            now,
        )?;
        Ok(())
    }

    /// Offers an inventory item to `to_agent_id` over IM (`IM_INVENTORY_OFFERED`).
    /// `transaction_id` is a fresh, caller-chosen id the recipient echoes back
    /// when accepting or declining. The recipient sees an
    /// [`Event::InstantMessageReceived`] with [`ImDialog::InventoryOffered`];
    /// decode the offer with [`InstantMessage::inventory_offer`](crate::InstantMessage::inventory_offer).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn give_inventory(
        &mut self,
        to_agent_id: AgentKey,
        item_id: InventoryKey,
        asset_type: AssetType,
        item_name: &str,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let bucket = inventory_offer_bucket(asset_type, item_id.uuid())?;
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: to_agent_id.uuid(),
                from_group: false,
                dialog: ImDialog::InventoryOffered,
                id: transaction_id.get(),
                message: item_name,
                from_name: &from_name,
                binary_bucket: bucket,
            },
            now,
        )?;
        Ok(())
    }

    /// Offers an inventory folder to `to_agent_id` over IM (`IM_INVENTORY_OFFERED`,
    /// the binary bucket led by [`AssetType::Folder`]). `transaction_id` is a
    /// fresh, caller-chosen id echoed back on accept/decline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn give_inventory_folder(
        &mut self,
        to_agent_id: AgentKey,
        folder_id: InventoryFolderKey,
        folder_name: &str,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let bucket = inventory_offer_bucket(AssetType::Folder, folder_id.uuid())?;
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: to_agent_id.uuid(),
                from_group: false,
                dialog: ImDialog::InventoryOffered,
                id: transaction_id.get(),
                message: folder_name,
                from_name: &from_name,
                binary_bucket: bucket,
            },
            now,
        )?;
        Ok(())
    }

    /// Accepts an inventory offer received over IM (`IM_INVENTORY_ACCEPTED`),
    /// filing the offered item/folder into `folder_id`. The `offer` is the
    /// decoded [`InventoryOffer`] from the incoming
    /// [`InstantMessage::inventory_offer`](crate::InstantMessage::inventory_offer).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn accept_inventory_offer(
        &mut self,
        offer: &InventoryOffer,
        folder_id: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: offer.from_agent_id.uuid(),
                from_group: false,
                dialog: ImDialog::InventoryAccepted,
                id: offer.transaction_id,
                message: "",
                from_name: &from_name,
                binary_bucket: folder_id.uuid().as_bytes().to_vec(),
            },
            now,
        )?;
        Ok(())
    }

    /// Declines an inventory offer received over IM (`IM_INVENTORY_DECLINED`);
    /// the simulator routes the offered item/folder into `trash_folder_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn decline_inventory_offer(
        &mut self,
        offer: &InventoryOffer,
        trash_folder_id: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: offer.from_agent_id.uuid(),
                from_group: false,
                dialog: ImDialog::InventoryDeclined,
                id: offer.transaction_id,
                message: "",
                from_name: &from_name,
                binary_bucket: trash_folder_id.uuid().as_bytes().to_vec(),
            },
            now,
        )?;
        Ok(())
    }

    /// Starts (or adds invitees to) an ad-hoc conference IM session
    /// (`IM_SESSION_CONFERENCE_START`). `session_id` is a fresh, caller-chosen id
    /// naming the session; `invitees` are packed into the binary bucket as raw
    /// 16-byte ids. Call again with the same `session_id` and further invitees to
    /// invite more people. Conference messages arrive as
    /// [`Event::ConferenceSessionMessage`], joins/leaves as
    /// [`Event::ConferenceSessionParticipant`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn start_conference(
        &mut self,
        session_id: ImSessionId,
        invitees: &[AgentKey],
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let invitee_ids: Vec<Uuid> = invitees.iter().map(AgentKey::uuid).collect();
        let bucket = pack_uuids(&invitee_ids);
        let to_agent_id = invitee_ids
            .first()
            .copied()
            .unwrap_or_else(|| session_id.get());
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id,
                from_group: false,
                dialog: ImDialog::SessionConferenceStart,
                id: session_id.get(),
                message,
                from_name: &from_name,
                binary_bucket: bucket,
            },
            now,
        )?;
        // Starting a conference opens/tracks it (keyed by the minted session id).
        self.chat_session_mut(ChatSessionKind::Conference { id: session_id }, now);
        Ok(())
    }

    /// Sends a message into an ad-hoc conference / multi-party IM session
    /// (`IM_SESSION_SEND`, session id = `session_id`). Other participants receive
    /// it as [`Event::ConferenceSessionMessage`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_conference_message(
        &mut self,
        session_id: ImSessionId,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let own_agent = self.agent_id();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: session_id.get(),
                from_group: false,
                dialog: ImDialog::SessionSend,
                id: session_id.get(),
                message,
                from_name: &from_name,
                binary_bucket: Vec::new(),
            },
            now,
        )?;
        // Sending into a conference opens/tracks it (keyed by the session id) and
        // logs our own message.
        if let Some(sender) = own_agent {
            self.log_outbound_message(
                ChatSessionKind::Conference { id: session_id },
                SessionMessage {
                    sender,
                    dialog: ImDialog::SessionSend,
                    text: message.to_owned(),
                    timestamp: None,
                },
                now,
            );
        }
        Ok(())
    }

    /// Leaves an ad-hoc conference / multi-party IM session (`IM_SESSION_LEAVE`),
    /// so the agent stops receiving its chat.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn leave_conference(&mut self, session_id: ImSessionId, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: session_id.get(),
                from_group: false,
                dialog: ImDialog::SessionLeave,
                id: session_id.get(),
                message: "",
                from_name: &from_name,
                binary_bucket: Vec::new(),
            },
            now,
        )?;
        // Leaving removes the entry — the registry tracks only live sessions.
        self.chat_sessions
            .remove(&ChatSessionKind::Conference { id: session_id });
        Ok(())
    }

    /// Requests the agent's stored offline instant messages over the legacy UDP
    /// trigger (`RetrieveInstantMessages`). The simulator re-delivers each as an
    /// ordinary [`Event::InstantMessageReceived`] with [`InstantMessage::offline`](crate::InstantMessage::offline)
    /// set. The modern Second Life path is the `ReadOfflineMsgs` capability
    /// (driven from the runtimes), decoded into the same events.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn retrieve_instant_messages(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_retrieve_instant_messages(now)?;
        Ok(())
    }

    /// Replies to a scripted-object dialog (`ScriptDialogReply`): the chosen
    /// `button_index`/`button_label` (from the [`Event::ScriptDialog`]'s
    /// [`ScriptDialog::buttons`](crate::ScriptDialog::buttons)) is sent back to `object_id` on the dialog's
    /// hidden `chat_channel`. For an `llTextBox`, pass the typed text as
    /// `button_label` with `button_index` `0`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the reply fails to encode.
    pub fn reply_script_dialog(
        &mut self,
        object_id: ObjectKey,
        chat_channel: ChatChannel,
        button_index: i32,
        button_label: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_script_dialog_reply(
            object_id,
            chat_channel,
            button_index,
            button_label,
            now,
        )?;
        Ok(())
    }

    /// Answers a scripted-object permission request (`ScriptAnswerYes`) from the
    /// [`Event::ScriptPermissionRequest`]: grants the `permissions` bitfield (a
    /// subset of those requested) to the script `item_id` in object `task_id`.
    /// Pass [`ScriptPermissions::default`] (an empty set) to deny everything.
    ///
    /// `experience_id` is the experience the answered request was made under
    /// (from the [`ScriptPermissionRequest`](crate::ScriptPermissionRequest), or
    /// `None` outside an experience):
    /// the session keeps no outstanding-request state, so the driver passes it
    /// back from the request it is answering, to record on the grant.
    ///
    /// The answer is recorded into the session's permission mirror after the wire
    /// send (the mirror follows the wire): a non-empty `permissions` records a
    /// grant, and an empty set records an explicit *deny* (distinct from a
    /// never-asked holder — see [`Session::script_permission_status`]); either
    /// replaces any prior answer for the holder. The simulator stays
    /// authoritative — the mirror is an API convenience read via
    /// [`Session::granted_permissions`] / [`Session::script_permission_status`] /
    /// [`Session::script_grants`], never a security boundary.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the reply fails to encode.
    pub fn answer_script_permissions(
        &mut self,
        task_id: ObjectKey,
        item_id: InventoryKey,
        permissions: ScriptPermissions,
        experience_id: Option<ExperienceKey>,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_script_answer_yes(task_id, item_id.uuid(), permissions.0, now)?;
        // Record the answer into the mirror after the send (it follows the
        // wire). An empty answer is an explicit *deny* (recorded as such, distinct
        // from never-asked); a non-empty answer is a grant. Either way the entry
        // replaces any prior one for the holder and carries the holder kind /
        // circuit so the region-leave resets treat a denial like a grant.
        let holder = ScriptHolder { task_id, item_id };
        let status = if permissions.0 == 0 {
            GrantStatus::Denied
        } else {
            GrantStatus::Granted(permissions)
        };
        let (kind, circuit) = self.holder_kind(task_id);
        self.script_grants.insert(
            holder,
            ScriptGrant {
                status,
                kind,
                circuit,
                experience_id,
            },
        );
        Ok(())
    }

    /// The script permissions currently granted to the script `item_id` in
    /// object `task_id`, as recorded in the permission mirror. Returns an empty
    /// [`ScriptPermissions`] when there is no grant (a denied or never-answered
    /// request both read as empty — the mirror records only live grants).
    ///
    /// The simulator stays authoritative; this is an API-convenience mirror of
    /// what the agent granted, not a security boundary.
    #[must_use]
    pub fn granted_permissions(
        &self,
        task_id: ObjectKey,
        item_id: InventoryKey,
    ) -> ScriptPermissions {
        self.script_grants
            .get(&ScriptHolder { task_id, item_id })
            .map_or(ScriptPermissions(0), |grant| match grant.status {
                GrantStatus::Granted(permissions) => permissions,
                GrantStatus::Denied => ScriptPermissions(0),
            })
    }

    /// The tri-state status of the script `item_id` in object `task_id`:
    /// [`ScriptPermissionStatus::NeverAsked`] when the mirror holds no entry,
    /// [`ScriptPermissionStatus::Denied`] when the agent answered with no
    /// permissions, or [`ScriptPermissionStatus::Granted`] with the granted
    /// subset. Distinguishes a never-asked script from an explicitly denied one
    /// (which [`Session::granted_permissions`] cannot, both reading empty).
    ///
    /// The simulator stays authoritative; this is an API-convenience mirror of
    /// the agent's recorded answer, not a security boundary.
    #[must_use]
    pub fn script_permission_status(
        &self,
        task_id: ObjectKey,
        item_id: InventoryKey,
    ) -> ScriptPermissionStatus {
        self.script_grants
            .get(&ScriptHolder { task_id, item_id })
            .map_or(ScriptPermissionStatus::NeverAsked, |grant| {
                match grant.status {
                    GrantStatus::Denied => ScriptPermissionStatus::Denied,
                    GrantStatus::Granted(permissions) => {
                        ScriptPermissionStatus::Granted(permissions)
                    }
                }
            })
    }

    /// Every recorded answer in the mirror, as read-only [`ScriptGrantInfo`]
    /// views (deterministic order). Includes explicit denials (`denied` set,
    /// `granted` empty); a never-asked script is absent. Empty when the mirror
    /// holds no entry.
    ///
    /// The simulator stays authoritative; this mirrors what the agent answered,
    /// never a security boundary.
    pub fn script_grants(&self) -> impl Iterator<Item = ScriptGrantInfo> + '_ {
        self.script_grants
            .iter()
            .map(|(holder, grant)| ScriptGrantInfo {
                task_id: holder.task_id,
                item_id: holder.item_id,
                granted: match grant.status {
                    GrantStatus::Granted(permissions) => permissions,
                    GrantStatus::Denied => ScriptPermissions(0),
                },
                denied: matches!(grant.status, GrantStatus::Denied),
                is_attachment: matches!(grant.kind, HolderKind::Attachment),
                experience_id: grant.experience_id,
            })
    }

    /// Returns which movement controls scripts are currently holding, split by
    /// `PassToAgent` (see [`ScriptControlsInfo`]).
    ///
    /// The session tracks this from the inbound `ScriptControlChange` (a
    /// `llTakeControls` adds, a `llReleaseControls` removes) and clears it on
    /// [`Session::release_script_controls`]. It is **not** reset on a region
    /// change — controls are agent-global and the viewer keeps them across a
    /// teleport. The simulator stays authoritative; this is an API-convenience
    /// mirror.
    #[must_use]
    pub fn script_controls(&self) -> ScriptControlsInfo {
        let union = |counts: &BTreeMap<u32, u32>| {
            counts.keys().fold(ControlFlags::empty(), |acc, &bit| {
                acc | ControlFlags::from_bits(bit)
            })
        };
        ScriptControlsInfo {
            taken: union(&self.taken_controls.consumed),
            passed_to_agent: union(&self.taken_controls.passed_on),
        }
    }

    /// A complete snapshot of the script-permission mirror: every recorded grant
    /// or denial ([`Session::script_grants`]) bundled with the currently-held
    /// movement controls ([`Session::script_controls`]).
    ///
    /// This is the read-out behind
    /// [`Command::QueryScriptPermissions`](crate::Command::QueryScriptPermissions):
    /// a runtime builds this in reply to that command and surfaces it as
    /// [`Event::ScriptPermissionState`]. The simulator stays authoritative; this
    /// is an API-convenience mirror, not a security boundary.
    #[must_use]
    pub fn script_permission_state(&self) -> ScriptPermissionState {
        ScriptPermissionState {
            grants: self.script_grants().collect(),
            controls: self.script_controls(),
        }
    }

    /// Allocates the next monotonic [`XferId`] (never zero), for a new inbound
    /// `Xfer` download or the caller-initiated [`request_xfer`](Self::request_xfer)
    /// path.
    fn alloc_xfer_id(&mut self) -> XferId {
        let id = self.next_xfer_id;
        self.next_xfer_id = XferId(self.next_xfer_id.get().checked_add(1).unwrap_or(1));
        id
    }

    /// Registers a new inbound `Xfer` download for `filename` with the given
    /// routing `purpose` and queues the `RequestXfer` that starts it. Returns the
    /// allocated [`XferId`] correlating the transfer.
    fn start_xfer_download(
        &mut self,
        purpose: XferPurpose,
        filename: &str,
        now: Instant,
    ) -> Result<XferId, Error> {
        let xfer_id = self.alloc_xfer_id();
        self.xfer_downloads.insert(
            xfer_id,
            XferDownload {
                purpose,
                buffer: Vec::new(),
            },
        );
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.send_request_xfer(xfer_id, filename, now)?;
        }
        Ok(xfer_id)
    }

    /// Routes a completed inbound `Xfer` download's assembled bytes to the typed
    /// event named by its [`XferPurpose`].
    fn finish_xfer_download(
        &mut self,
        xfer_id: XferId,
        download: XferDownload,
    ) -> Result<(), Error> {
        match download.purpose {
            XferPurpose::MuteList => {
                self.events
                    .push_back(Event::MuteList(parse_mute_list(&download.buffer)?));
            }
            XferPurpose::TaskInventory { task, serial } => {
                self.events.push_back(Event::TaskInventoryContents {
                    task,
                    serial,
                    items: parse_task_inventory(&download.buffer)?,
                });
            }
            XferPurpose::Generic => {
                self.events.push_back(Event::XferDownloaded {
                    xfer_id,
                    data: download.buffer,
                });
            }
        }
        Ok(())
    }

    /// Downloads an arbitrary named file over the legacy `Xfer` path
    /// (`RequestXfer`), surfacing the assembled bytes as
    /// [`Event::XferDownloaded`] tagged with the returned [`XferId`]. This is the
    /// generic building block the mute-list and task-inventory consumers
    /// specialize; use it directly when a message hands you a raw `Xfer`
    /// `filename` (for example a [`TaskInventoryReply`](crate::TaskInventoryReply)
    /// `filename` you want the bytes of without the parsed listing).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_xfer(&mut self, filename: &str, now: Instant) -> Result<XferId, Error> {
        if self.circuit.is_none() {
            return Err(Error::NoCircuit);
        }
        self.start_xfer_download(XferPurpose::Generic, filename, now)
    }

    /// Requests the agent's mute (block) list (`MuteListRequest` with a zero
    /// CRC, forcing a fresh download). The simulator replies with the list (the
    /// file is downloaded over the `Xfer` path and surfaced as
    /// [`Event::MuteList`]), or with [`Event::MuteListUnchanged`] /
    /// [`Event::MuteList`]`([])` for an unchanged or empty list.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_mute_list(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_mute_list_request(0, now)?;
        Ok(())
    }

    /// Mutes (blocks) an entity (`UpdateMuteListEntry`). `mute_type` selects what
    /// is muted (use [`MuteType::Agent`] for an avatar); `name` is its display
    /// name (required, especially for [`MuteType::ByName`] where `id` is nil);
    /// `flags` are the per-aspect *exceptions* (use [`MuteFlags::default`] to mute
    /// everything). Re-request the list to see the change.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn mute(
        &mut self,
        id: Uuid,
        name: &str,
        mute_type: MuteType,
        flags: MuteFlags,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_mute_list_entry(id, name, mute_type.to_i32(), flags.0, now)?;
        Ok(())
    }

    /// Removes a mute (`RemoveMuteListEntry`). `id` and `name` must match the
    /// existing entry (from [`Event::MuteList`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn unmute(&mut self, id: Uuid, name: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_mute_list_entry(id, name, now)?;
        Ok(())
    }

    /// Requests a texture by asset id over the legacy UDP image path
    /// (`RequestImage`). The simulator streams it back as an `ImageData` header
    /// packet plus `ImagePacket` follow-ups, which are reassembled and surfaced
    /// as [`Event::TextureReceived`] (or [`Event::TextureNotFound`] if the asset
    /// does not exist). `discard_level` selects the level of detail (0 = full
    /// resolution; higher values request a coarser, smaller image); `priority`
    /// orders concurrent fetches (a larger value is fetched sooner). The modern
    /// alternative is the HTTP `GetTexture` capability (a runtime `FetchTexture`
    /// command), which is preferred on the Second Life grid.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_texture(
        &mut self,
        texture_id: TextureKey,
        discard_level: i8,
        priority: f32,
        now: Instant,
    ) -> Result<(), Error> {
        // A fresh download buffer; a repeat request just restarts it.
        self.texture_downloads.insert(
            texture_id.uuid(),
            TextureDownload {
                codec: ImageCodec::J2c,
                packets: 0,
                chunks: BTreeMap::new(),
            },
        );
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // `type` 0 is the normal image channel; start at packet 0.
        circuit.send_request_image(texture_id, discard_level, priority, 0, 0, now)?;
        Ok(())
    }

    /// Asks the simulator to (re-)send the agent's own current wearables via
    /// `AgentWearablesRequest`. The reply arrives as [`Event::AgentWearables`].
    /// The simulator also pushes one unsolicited at login and after every
    /// wearable change, so a passive client need not call this.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_wearables(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_wearables_request(now)?;
        Ok(())
    }

    /// Sets the agent's outfit via `AgentIsNowWearing`: the complete set of
    /// wearables the agent should now be wearing (only the
    /// [`item_id`](Wearable::item_id) and
    /// [`wearable_type`](Wearable::wearable_type) are sent). Each wearable item
    /// must already be in the agent's inventory (see
    /// [`Session::request_folder_contents`]). The simulator acknowledges by
    /// pushing a fresh [`Event::AgentWearables`].
    ///
    /// Note this changes which wearables are *worn*; the avatar's rendered
    /// appearance is only refreshed once the baked textures are recomputed and
    /// advertised with [`Session::set_appearance`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_wearing(&mut self, wearables: &[Wearable], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_is_now_wearing(wearables, now)?;
        Ok(())
    }

    /// Attaches the in-world object `local_id` to the avatar via `ObjectAttach`,
    /// worn at `attachment_point` and rotated by `rotation`. `mode` chooses
    /// whether the object is added to the point alongside anything already there
    /// or replaces it. To wear an item straight from inventory instead, use
    /// [`Session::rez_attachment`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn attach_object(
        &mut self,
        local_id: ScopedObjectId,
        attachment_point: AttachmentPoint,
        mode: AttachmentMode,
        rotation: &Rotation,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_attach(local_id, attachment_point, mode, rotation, now)?;
        Ok(())
    }

    /// Detaches the attachments `local_ids` back to inventory via `ObjectDetach`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn detach_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_detach(&local_ids, now)?;
        Ok(())
    }

    /// Drops the attachments `local_ids` from the avatar onto the ground via
    /// `ObjectDrop` (they become ordinary in-world objects).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn drop_attachments(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_drop(&local_ids, now)?;
        Ok(())
    }

    /// Removes (takes off) the worn item `item_id` via `RemoveAttachment`. Unlike
    /// [`Session::detach_objects`] this names the inventory item, not the rezzed
    /// object's region-local id.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn remove_attachment(
        &mut self,
        attachment_point: AttachmentPoint,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_attachment(attachment_point, item_id.uuid(), now)?;
        Ok(())
    }

    /// Wears the inventory item described by `rez` as an attachment via
    /// `RezSingleAttachmentFromInv`. To attach an object already rezzed in-world,
    /// use [`Session::attach_object`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_attachment(&mut self, rez: &RezAttachment, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_rez_single_attachment(rez, now)?;
        Ok(())
    }

    /// Wears several inventory items as attachments in one compound message via
    /// `RezMultipleAttachmentsFromInv`. `compound_id` is a fresh caller-chosen id
    /// correlating the message's parts; `detach` says whether to detach
    /// everything currently worn first.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_attachments(
        &mut self,
        compound_id: TransactionId,
        detach: DetachOrder,
        attachments: &[RezAttachment],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_rez_multiple_attachments(compound_id.get(), detach, attachments, now)?;
        Ok(())
    }

    /// Sends one or more viewer effects via `ViewerEffect` (look-at / point-at
    /// gaze hints, the editing/touch beam, and other transient HUD effects other
    /// viewers render). The effects are batched into a single message.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn send_viewer_effect(
        &mut self,
        effects: &[ViewerEffect],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_viewer_effect(effects, now)?;
        Ok(())
    }

    /// Asks the simulator to track `prey_id`'s position via `TrackAgent`; the
    /// tracked agent's coarse location is then streamed back in
    /// `CoarseLocationUpdate` (surfaced as
    /// [`Event::CoarseLocationUpdate`](crate::Event::CoarseLocationUpdate)).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn track_agent(&mut self, prey_id: AgentKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_track_agent(prey_id, now)?;
        Ok(())
    }

    /// Asks the simulator for `prey`'s global position via `FindAgent` (an
    /// estate/god lookup, sent on behalf of `hunter` — usually the agent's own
    /// id). The simulator answers with a `FindAgent` carrying the found
    /// positions, surfaced as
    /// [`Event::FindAgentReply`](crate::Event::FindAgentReply).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn find_agent(
        &mut self,
        hunter: AgentKey,
        prey: AgentKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_find_agent(hunter.uuid(), prey.uuid(), now)?;
        Ok(())
    }

    /// Runs a directory people / groups / events search via `DirFindQuery`.
    /// `flags` selects what is searched ([`DirFindFlags::PEOPLE`] /
    /// [`DirFindFlags::GROUPS`] / [`DirFindFlags::EVENTS`]) and how the results
    /// are sorted/filtered; `query_start` pages the results. The reply arrives as
    /// [`Event::DirPeopleReply`] / [`Event::DirGroupsReply`] /
    /// [`Event::DirEventsReply`], correlated by `query_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn dir_find_query(
        &mut self,
        query_id: QueryId,
        query_text: &str,
        flags: DirFindFlags,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_find_query(query_id.get(), query_text, flags, query_start, now)?;
        Ok(())
    }

    /// Searches the places directory via `DirPlacesQuery`. The reply arrives as
    /// [`Event::DirPlacesReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire query block")]
    pub fn dir_places_query(
        &mut self,
        query_id: QueryId,
        query_text: &str,
        flags: DirFindFlags,
        category: ParcelCategory,
        sim_name: &str,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_places_query(
            query_id.get(),
            query_text,
            flags,
            category,
            sim_name,
            query_start,
            now,
        )?;
        Ok(())
    }

    /// Searches the land-for-sale directory via `DirLandQuery`. The reply arrives
    /// as [`Event::DirLandReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire query block")]
    pub fn dir_land_query(
        &mut self,
        query_id: QueryId,
        flags: DirFindFlags,
        search_type: LandSearchType,
        price: i32,
        area: i32,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_land_query(
            query_id.get(),
            flags,
            search_type,
            price,
            area,
            query_start,
            now,
        )?;
        Ok(())
    }

    /// Searches the classifieds directory via `DirClassifiedQuery`. The reply
    /// arrives as [`Event::DirClassifiedReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn dir_classified_query(
        &mut self,
        query_id: QueryId,
        query_text: &str,
        flags: DirFindFlags,
        category: ClassifiedCategory,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_classified_query(
            query_id.get(),
            query_text,
            flags,
            category,
            query_start,
            now,
        )?;
        Ok(())
    }

    /// Autocompletes avatar names via `AvatarPickerRequest`. The reply arrives as
    /// [`Event::AvatarPickerReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn avatar_picker_request(
        &mut self,
        query_id: QueryId,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_picker_request(query_id.get(), name, now)?;
        Ok(())
    }

    /// Looks up an agent's or group's land holdings via `PlacesQuery` (the
    /// land-holdings panels, distinct from the directory search). The reply
    /// arrives as [`Event::PlacesReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire query block")]
    pub fn places_query(
        &mut self,
        query_id: QueryId,
        transaction_id: TransactionId,
        query_text: &str,
        flags: DirFindFlags,
        category: ParcelCategory,
        sim_name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_places_query(
            query_id.get(),
            transaction_id.get(),
            query_text,
            flags,
            category,
            sim_name,
            now,
        )?;
        Ok(())
    }

    /// Requests the full detail of an in-world event via `EventInfoRequest`,
    /// using the `event_id` from an events [`Event::DirEventsReply`] (or the
    /// events directory). The reply arrives as [`Event::EventInfoReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn event_info_request(&mut self, event_id: EventId, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_event_info_request(event_id, now)?;
        Ok(())
    }

    /// Subscribes to a reminder for an in-world event via
    /// `EventNotificationAddRequest`. There is no direct reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn event_notification_add_request(
        &mut self,
        event_id: EventId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_event_notification_add_request(event_id, now)?;
        Ok(())
    }

    /// Cancels a previously-added event reminder via
    /// `EventNotificationRemoveRequest`. There is no direct reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn event_notification_remove_request(
        &mut self,
        event_id: EventId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_event_notification_remove_request(event_id, now)?;
        Ok(())
    }

    /// Buys one or more in-world objects offered for sale via `ObjectBuy`. The
    /// sale type and price in each [`ObjectBuyItem`] must match what the object
    /// advertises (see [`Session::request_object_properties_family`]); a derezed
    /// purchase is placed in `category_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn buy_object(
        &mut self,
        group_id: GroupKey,
        category_id: Uuid,
        objects: &[ObjectBuyItem],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_buy(group_id, category_id, objects, now)?;
        Ok(())
    }

    /// Buys a single item out of an object's contents via `BuyObjectInventory`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn buy_object_inventory(
        &mut self,
        object_id: ObjectKey,
        item_id: InventoryKey,
        folder_id: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_buy_object_inventory(object_id, item_id.uuid(), folder_id.uuid(), now)?;
        Ok(())
    }

    /// Requests an object's pay-button layout via `RequestPayPrice`. The reply
    /// arrives as [`Event::PayPriceReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_pay_price(&mut self, object_id: ObjectKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_request_pay_price(object_id, now)?;
        Ok(())
    }

    /// Requests an object's condensed broadcast properties via
    /// `RequestObjectPropertiesFamily`. The reply arrives as
    /// [`Event::ObjectPropertiesFamily`]. Unlike
    /// [`Session::request_object_properties`] this needs no prior selection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_object_properties_family(
        &mut self,
        request_flags: u32,
        object_id: ObjectKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_request_object_properties_family(request_flags, object_id, now)?;
        Ok(())
    }

    /// Begins an interactive spin (rotate) of an object via `ObjectSpinStart`;
    /// pairs with [`Session::spin_object_update`] / [`Session::spin_object_stop`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn spin_object_start(&mut self, object_id: ObjectKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_spin_start(object_id, now)?;
        Ok(())
    }

    /// Updates an in-progress object spin with the latest rotation via
    /// `ObjectSpinUpdate`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn spin_object_update(
        &mut self,
        object_id: ObjectKey,
        rotation: Rotation,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_spin_update(object_id, rotation, now)?;
        Ok(())
    }

    /// Ends an interactive object spin via `ObjectSpinStop`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn spin_object_stop(&mut self, object_id: ObjectKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_spin_stop(object_id, now)?;
        Ok(())
    }

    /// Duplicates objects, dropping the copies against the surface a ray hits,
    /// via `ObjectDuplicateOnRay`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    #[expect(
        clippy::too_many_arguments,
        clippy::fn_params_excessive_bools,
        reason = "mirrors the ObjectDuplicateOnRay wire block one-to-one"
    )]
    pub fn duplicate_objects_on_ray(
        &mut self,
        local_ids: &[ScopedObjectId],
        group_id: Option<GroupKey>,
        ray_start: Vector,
        ray_end: Vector,
        bypass_raycast: bool,
        ray_end_is_intersection: bool,
        copy_centers: bool,
        copy_rotates: bool,
        ray_target_id: Option<ObjectKey>,
        duplicate_flags: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_duplicate_on_ray(
            &local_ids,
            group_id,
            ray_start,
            ray_end,
            bypass_raycast,
            ray_end_is_intersection,
            copy_centers,
            copy_rotates,
            ray_target_id,
            duplicate_flags,
            now,
        )?;
        Ok(())
    }

    /// Restores an inventory item to the world at its last in-world position via
    /// `RezRestoreToWorld` (a `UDPDeprecated` message a viewer may still send).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_restore_to_world(&mut self, item: &RestoreItem, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_rez_restore_to_world(item, now)?;
        Ok(())
    }

    /// Rezzes an object embedded in a notecard asset via `RezObjectFromNotecard`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_object_from_notecard(
        &mut self,
        rez: &NotecardRez,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_rez_object_from_notecard(rez, now)?;
        Ok(())
    }

    /// Rezzes the inventory item `params.item` into the world as a new object via
    /// `RezObject`. The ray fields of `params` place the object exactly as
    /// [`Session::rez_object`] (the new-prim `ObjectAdd` path) does; the new
    /// object arrives as an [`Event::ObjectAdded`]. To rez objects embedded in a
    /// notecard instead, use [`Session::rez_object_from_notecard`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_object_from_inventory(
        &mut self,
        params: &RezObjectParams,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_rez_object(params, now)?;
        Ok(())
    }

    /// Drops the script inventory item `params.item` into the task inventory of
    /// the in-world object `target` via `RezScript`, optionally rezzed already
    /// running ([`RezScriptParams::enabled`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_script(
        &mut self,
        target: ScopedObjectId,
        params: &RezScriptParams,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(target.circuit)?;
        circuit.send_rez_script(target.id, params, now)?;
        Ok(())
    }

    /// Revokes the named LSL script `permissions` previously granted to the
    /// object `object_id` via `RevokePermissions` (the inverse of
    /// [`Session::answer_script_permissions`]). Passing
    /// [`ScriptPermissions::default`] (an empty set) revokes nothing; a full set
    /// revokes every previously granted permission.
    ///
    /// The full requested bitfield goes on the wire, but the mirror only follows
    /// what the simulator actually honours for this message: the animation bits
    /// (`TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS`). Those bits are cleared from
    /// every grant on `object_id` (object-scoped, so possibly several scripts);
    /// a grant left empty is removed. The other bits (e.g. `TELEPORT`) the
    /// simulator keeps enforcing, so the conservative mirror leaves them; control
    /// grants are released via [`Session::release_script_controls`], not here.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn revoke_script_permissions(
        &mut self,
        object_id: ObjectKey,
        permissions: ScriptPermissions,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_revoke_permissions(object_id, permissions, now)?;
        // Mirror only the bits the simulator honours for `RevokePermissions`.
        let honoured = permissions.0
            & (ScriptPermissions::TRIGGER_ANIMATION | ScriptPermissions::OVERRIDE_ANIMATIONS);
        if honoured != 0 {
            self.script_grants.retain(|holder, grant| {
                // Only a grant on this object loses the honoured bits; denials and
                // grants on other objects are untouched. A grant emptied by the
                // revoke is dropped (a denial is always kept).
                if let GrantStatus::Granted(ref mut granted) = grant.status {
                    if holder.task_id == object_id {
                        granted.0 &= !honoured;
                    }
                    return granted.0 != 0;
                }
                true
            });
        }
        Ok(())
    }

    /// Detaches the worn attachment whose inventory item is `item_id` back into
    /// inventory via `DetachAttachmentIntoInv`. Unlike [`Session::detach_objects`]
    /// this names the inventory item, not the rezzed object's region-local id, so
    /// it works without first having seen the object rez.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn detach_attachment_into_inventory(
        &mut self,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_detach_attachment_into_inv(item_id, now)?;
        Ok(())
    }

    /// Requests the task (object) inventory listing of the in-world object
    /// `target` via `RequestTaskInventory`. The simulator answers with a
    /// `ReplyTaskInventory`, surfaced as
    /// [`Event::TaskInventoryReply`](crate::Event::TaskInventoryReply); the full
    /// contents are then downloaded over the Xfer path named by its `filename`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `target`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_task_inventory(
        &mut self,
        target: ScopedObjectId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(target.circuit)?;
        circuit.send_request_task_inventory(target.id, now)?;
        Ok(())
    }

    /// Requests and *reads* the task (object) inventory of `target`: sends the
    /// same `RequestTaskInventory` as
    /// [`request_task_inventory`](Self::request_task_inventory), but then follows
    /// the `ReplyTaskInventory` to its `Xfer` file, downloads it, and parses the
    /// listing — surfacing the parsed items as
    /// [`Event::TaskInventoryContents`] (an empty list when the inventory is
    /// empty). The lower-level [`Event::TaskInventoryReply`] is still emitted
    /// first, so a caller that only wants the serial can ignore the contents.
    ///
    /// The reply is correlated to `target` by the object's full id, resolved
    /// from the object cache at request time; if the object is not yet cached
    /// the fetch falls back to matching the next otherwise-unclaimed reply, which
    /// cannot disambiguate concurrent uncached fetches.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `target`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn fetch_task_inventory(
        &mut self,
        target: ScopedObjectId,
        now: Instant,
    ) -> Result<(), Error> {
        match self.resolve_object_key(target) {
            Some(task) => {
                self.pending_task_inventory.insert(task);
            }
            None => self.pending_task_inventory_unresolved.push_back(()),
        }
        let circuit = self.circuit_for_scope(target.circuit)?;
        circuit.send_request_task_inventory(target.id, now)?;
        Ok(())
    }

    /// Resolves a [`ScopedObjectId`] to the cached object's full [`ObjectKey`],
    /// or `None` when that object is not (yet) in the scene-graph cache.
    fn resolve_object_key(&self, target: ScopedObjectId) -> Option<ObjectKey> {
        self.objects
            .get(&target.circuit)?
            .get(&target.id)
            .map(|object| object.full_id)
    }

    /// Writes the inventory item `item` into the task inventory of the in-world
    /// object `target` via `UpdateTaskInventory`, adding a new item or replacing
    /// the existing one the simulator matches by `key` (item id vs. asset id).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `target`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_task_inventory(
        &mut self,
        target: ScopedObjectId,
        key: TaskInventoryKey,
        item: &RestoreItem,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(target.circuit)?;
        circuit.send_update_task_inventory(target.id, key, item, now)?;
        Ok(())
    }

    /// Moves the task inventory item `item_id` out of the in-world object
    /// `target` into the agent inventory folder `folder_id` via
    /// `MoveTaskInventory`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `target`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn move_task_inventory(
        &mut self,
        target: ScopedObjectId,
        folder_id: InventoryFolderKey,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(target.circuit)?;
        circuit.send_move_task_inventory(target.id, folder_id, item_id, now)?;
        Ok(())
    }

    /// Removes the task inventory item `item_id` from the in-world object
    /// `target` via `RemoveTaskInventory`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `target`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn remove_task_inventory(
        &mut self,
        target: ScopedObjectId,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(target.circuit)?;
        circuit.send_remove_task_inventory(target.id, item_id, now)?;
        Ok(())
    }

    /// Asks whether the script `item_id` inside the task `object_id` is running
    /// via `GetScriptRunning`. The reply arrives as [`Event::ScriptRunning`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_script_running(
        &mut self,
        object_id: ObjectKey,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_get_script_running(object_id, item_id.uuid(), now)?;
        Ok(())
    }

    /// Starts or stops the script `item_id` inside the task `object_id` via
    /// `SetScriptRunning` (`running` selects run vs. stop).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_script_running(
        &mut self,
        object_id: ObjectKey,
        item_id: InventoryKey,
        running: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_set_script_running(object_id, item_id.uuid(), running, now)?;
        Ok(())
    }

    /// Resets the script `item_id` inside the task `object_id` to its initial
    /// state via `ScriptReset`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn reset_script(
        &mut self,
        object_id: ObjectKey,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_script_reset(object_id, item_id.uuid(), now)?;
        Ok(())
    }

    /// Advertises the agent's own appearance to the simulator (and, through it,
    /// to other viewers) via `AgentSetAppearance`: its bounding-box `size`
    /// (metres), the packed `texture_entry` blob carrying the baked-texture ids,
    /// the `visual_params` bytes (one quantized byte per parameter, in the
    /// reference viewer's order), and the per-baked-slot `wearable_cache` hashes
    /// (`(cache id, texture slot index)`; see the [`avatar_texture`](crate::avatar_texture) constants).
    /// `serial` must strictly increase across calls (0 resets the simulator's
    /// counter).
    ///
    /// Computing the baked textures and visual parameters is the avatar-baking
    /// step (it normally requires uploading the bakes — the upload pipeline is a
    /// separate feature); this method is the wire surface that publishes an
    /// already-computed appearance.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_appearance(
        &mut self,
        serial: u32,
        size: Vector,
        texture_entry: &[u8],
        visual_params: &[u8],
        wearable_cache: &[(Uuid, u8)],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_set_appearance(
            serial,
            size,
            texture_entry,
            visual_params,
            wearable_cache,
            now,
        )?;
        Ok(())
    }

    /// Starts and/or stops several of the agent's own animations at once via
    /// `AgentAnimation`. Each `(anim_id, start)` pair starts the animation when
    /// `start` is `true` and stops it when `false`. `anim_id` is a built-in
    /// animation UUID or an uploaded animation asset id. Other viewers observe
    /// the result as an [`Event::AvatarAnimation`] for this agent.
    ///
    /// For a single animation prefer the
    /// [`play_animation`](Session::play_animation) /
    /// [`stop_animation`](Session::stop_animation) convenience wrappers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_animations(
        &mut self,
        animations: &[(AnimationKey, bool)],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let wire: Vec<(Uuid, bool)> = animations
            .iter()
            .map(|(anim_id, start)| (anim_id.uuid(), *start))
            .collect();
        circuit.send_agent_animation(&wire, now)?;
        Ok(())
    }

    /// Starts one of the agent's own animations via `AgentAnimation`. `anim_id`
    /// is a built-in animation UUID or an uploaded animation asset id. Convenience
    /// for [`set_animations`](Session::set_animations) with a single starting
    /// animation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn play_animation(&mut self, anim_id: AnimationKey, now: Instant) -> Result<(), Error> {
        self.set_animations(&[(anim_id, true)], now)
    }

    /// Stops one of the agent's own animations via `AgentAnimation`. Convenience
    /// for [`set_animations`](Session::set_animations) with a single stopping
    /// animation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn stop_animation(&mut self, anim_id: AnimationKey, now: Instant) -> Result<(), Error> {
        self.set_animations(&[(anim_id, false)], now)
    }

    /// Queries the simulator's baked-texture cache via `AgentCachedTexture`: for
    /// each queried slot (`(cache id, texture slot index)`; see the
    /// [`avatar_texture`](crate::avatar_texture) constants) the simulator reports whether it already has
    /// a matching bake, in an [`Event::CachedTextureResponse`]. A viewer uses
    /// this before baking to skip re-uploading textures the grid already has.
    /// `serial` is echoed back in the reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_cached_textures(
        &mut self,
        serial: i32,
        slots: &[(Uuid, u8)],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_cached_texture(serial, slots, now)?;
        Ok(())
    }

    /// The agent's own id, once login has established the circuit. Useful as the
    /// `owner_id` for inventory fetches and for recognising the client's own
    /// messages.
    #[must_use]
    pub fn agent_id(&self) -> Option<AgentKey> {
        self.circuit.as_ref().map(|circuit| circuit.agent_id)
    }

    /// The agent's legacy name (`"First Last"`) from the login request — the same
    /// `FromAgentName` carried by outgoing instant messages. Available before the
    /// circuit is up (it comes from the login parameters), so a runtime can derive
    /// its per-account chat-log directory and label its own outbound lines without
    /// waiting on a server round-trip.
    #[must_use]
    pub fn agent_legacy_name(&self) -> String {
        self.agent_name()
    }

    /// The session id, once login has established the circuit. The companion to
    /// [`Session::agent_id`] for a driver that wants to symbolize the session in
    /// its logs.
    #[must_use]
    pub const fn session_id(&self) -> Option<Uuid> {
        match self.circuit.as_ref() {
            Some(circuit) => Some(circuit.session_id),
            None => None,
        }
    }

    /// The circuit code, once login has established the circuit. The companion to
    /// [`Session::agent_id`] for a driver that wants to symbolize the circuit in
    /// its logs.
    #[must_use]
    pub const fn circuit_code(&self) -> Option<CircuitCode> {
        match self.circuit.as_ref() {
            Some(circuit) => Some(circuit.code),
            None => None,
        }
    }

    /// The agent's inventory root ("My Inventory") folder id, from the login
    /// response, or `None` if the grid did not provide it. Use it as the starting
    /// point for [`Session::request_folder_contents`].
    #[must_use]
    pub const fn inventory_root(&self) -> Option<InventoryFolderKey> {
        self.inventory.agent_root()
    }

    /// The shared Library root folder id, from the login response, or `None` if
    /// the grid did not provide it. The read-only Library tree hangs off this
    /// root (held under [`InventoryOwner::Library`]); see also
    /// [`Session::login_account`].
    #[must_use]
    pub const fn library_root(&self) -> Option<InventoryFolderKey> {
        self.inventory.library_root()
    }

    /// The shared Library owner id, from the login response, or `None` if the grid
    /// did not provide it. Library folder fetches are addressed to this owner (over
    /// [`CAP_FETCH_LIBRARY`](crate::CAP_FETCH_LIBRARY) on Second Life, or the UDP
    /// `FetchInventoryDescendents` path on OpenSim) rather than the agent id; the
    /// same id is also reachable via [`Session::login_account`].
    #[must_use]
    pub const fn library_owner(&self) -> Option<OwnerKey> {
        self.inventory.library_owner()
    }

    /// Account-level facts from the login response (home, start look-at, maturity
    /// ratings, group limit, and the shared Library roots), or `None` before
    /// login. The same data is also emitted once as [`Event::Account`].
    #[must_use]
    pub const fn login_account(&self) -> Option<&LoginAccount> {
        self.login_account.as_ref()
    }

    /// Requests the contents (sub-folders and items) of the inventory folder
    /// `folder_id` via `FetchInventoryDescendents`. The reply arrives as
    /// [`Event::InventoryDescendents`]. The folder structure as a whole is also
    /// available upfront from [`Event::InventorySkeleton`] (login).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_folder_contents(
        &mut self,
        folder_id: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        // A Library folder is fetched with the Library owner id; the agent's own
        // folders with the agent id (the circuit default).
        let owner_id = match self.inventory.folder_owner(folder_id) {
            Some(InventoryOwner::Library) => {
                self.inventory.library_owner().map(|owner| owner.uuid())
            }
            _ => None,
        };
        {
            let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
            let owner_id = owner_id.unwrap_or_else(|| circuit.agent_id.uuid());
            circuit.send_fetch_inventory_descendents(folder_id.uuid(), owner_id, now)?;
        }
        // Track the in-flight request in the model so the background scheduler
        // does not re-pick this folder and the completion query reflects it.
        self.inventory.mark_folder_fetching(folder_id);
        Ok(())
    }

    /// The next batch of folders the background crawler should fetch — a
    /// breadth-first sweep over [`Unknown`](FolderState::Unknown) folders bounded
    /// by `max_in_flight` (minus those already in flight), flipping each returned
    /// folder to [`Fetching`](FolderState::Fetching). Returns an **empty** batch
    /// when the background inventory crawl is disabled (the default — see
    /// [`Session::set_background_inventory_fetch`]), so a consumer that never reads
    /// inventory pays nothing.
    ///
    /// The runtime shell POSTs a `FetchInventoryDescendents2` for each returned
    /// folder ([`INVENTORY_FETCH_MAX_IN_FLIGHT`](crate::INVENTORY_FETCH_MAX_IN_FLIGHT)
    /// is the conventional bound); the reply folds in and flips the folder
    /// [`Loaded`](FolderState::Loaded), seeding its children `Unknown` for the next
    /// sweep. The explicit pulls ([`Session::request_folder_contents`],
    /// [`Command::FetchInventoryFolders`](crate::Command::FetchInventoryFolders))
    /// work regardless of the flag.
    pub fn next_inventory_fetch_batch(&mut self, max_in_flight: usize) -> Vec<InventoryFolderKey> {
        if !self.background_inventory_fetch {
            return Vec::new();
        }
        self.inventory.next_fetch_batch(max_in_flight)
    }

    /// Enables or disables the automatic background inventory crawl (default
    /// **disabled**). While disabled, [`Session::next_inventory_fetch_batch`]
    /// returns empty and nothing auto-enqueues, so a consumer that ignores
    /// inventory issues no folder fetches. The explicit pull paths
    /// ([`Session::request_folder_contents`],
    /// [`Command::FetchInventoryFolders`](crate::Command::FetchInventoryFolders))
    /// stay available either way.
    pub const fn set_background_inventory_fetch(&mut self, enabled: bool) {
        self.background_inventory_fetch = enabled;
    }

    /// Whether the automatic background inventory crawl is enabled (see
    /// [`Session::set_background_inventory_fetch`]).
    #[must_use]
    pub const fn background_inventory_fetch(&self) -> bool {
        self.background_inventory_fetch
    }

    /// Whether the `owner` inventory tree is fully fetched — no folder under it is
    /// [`Unknown`](FolderState::Unknown) or [`Fetching`](FolderState::Fetching).
    /// The background-crawl completion signal (vacuously true before any folder of
    /// that owner is known).
    #[must_use]
    pub fn inventory_fully_loaded(&self, owner: InventoryOwner) -> bool {
        self.inventory.fully_loaded(owner)
    }

    // ---- Inventory cache (#30) ---------------------------------------------

    /// A cached inventory folder by id, if known (from the login skeleton, a
    /// folder-contents fetch, a simulator push, or the agent's own mutations).
    #[must_use]
    pub fn inventory_folder(&self, folder_id: InventoryFolderKey) -> Option<&InventoryFolder> {
        self.inventory.folder(folder_id)
    }

    /// A cached inventory item by id, if known (from a folder-contents fetch, a
    /// simulator push, or the agent's own mutations).
    #[must_use]
    pub fn inventory_item(&self, item_id: InventoryKey) -> Option<&InventoryItem> {
        self.inventory.item(item_id)
    }

    /// All cached inventory folders whose metadata is present, in key order. A
    /// folder that is known to exist but whose metadata has not yet arrived is
    /// not yielded; query its [`Session::folder_fetch_state`] to observe it.
    pub fn inventory_folders(&self) -> impl Iterator<Item = &InventoryFolder> {
        self.inventory.folders_iter()
    }

    /// Owning [`FolderInfo`] snapshots of every cached folder in the agent's own
    /// tree — each with its resolved [`FolderType`] and fetch [`FolderState`] — in
    /// key order. The login skeleton seeds the whole agent folder structure into
    /// the held model at login, so this is populated (types and all) before any
    /// contents fetch. The `Arc`-friendly, channel-crossing counterpart of
    /// [`Session::inventory_folders`] behind the
    /// [`Command`](crate::Command)/[`Event`](crate::Event) pull-bridge (the reply
    /// to [`Command::QueryInventoryFolders`](crate::Command::QueryInventoryFolders));
    /// the read-only Library tree is excluded (query its root via
    /// [`Session::library_root`]).
    #[must_use]
    pub fn inventory_folder_infos(&self) -> Vec<FolderInfo> {
        self.inventory
            .folders_iter()
            .filter(|folder| {
                self.inventory.folder_owner(folder.folder_id) != Some(InventoryOwner::Library)
            })
            .map(|folder| {
                let state = self
                    .inventory
                    .folder_state(folder.folder_id)
                    .unwrap_or(FolderState::Unknown);
                FolderInfo::from_folder(folder, state)
            })
            .collect()
    }

    /// All cached inventory items, in key order.
    pub fn inventory_items(&self) -> impl Iterator<Item = &InventoryItem> {
        self.inventory.items_iter()
    }

    /// The cached immediate children of `folder_id` as a borrowed [`Child`]
    /// iterator — its sub-folders first (in key order), then the items directly
    /// inside it — resolved O(children) through the parent→children index. This is
    /// the zero-copy tree-walk surface (bevy reads it directly via `&Session`);
    /// the owning, `Arc`-friendly, paginated counterpart is
    /// [`Session::inventory_folder_page`]. Only as complete as the cache (fetch
    /// the folder with [`Session::request_folder_contents`], or the modern AIS3
    /// CAPS path, to populate it).
    pub fn inventory_children(
        &self,
        folder_id: InventoryFolderKey,
    ) -> impl Iterator<Item = Child<'_>> {
        self.inventory.children_iter(folder_id)
    }

    /// One page of `folder_id`'s children as owning snapshots: a window over the
    /// **combined** child sequence — its sub-folders first, then its items, in
    /// parent→children-index order — so a single page can span the folder/item
    /// boundary of one mixed folder. Returns the [`FolderInfo`] and [`ItemInfo`]
    /// snapshots in that window plus the cursor for the next page (`None` when the
    /// folder is exhausted). Pass `None` for `before` to start at the beginning,
    /// then feed each returned cursor back as the next call's `before`.
    ///
    /// The owning, `Arc`-friendly counterpart of the borrowed
    /// [`Session::inventory_children`] walk — the snapshot read surface behind the
    /// [`Command`](crate::Command)/[`Event`](crate::Event) pull-bridge for the
    /// channel-based runtimes (mirrors [`Session::history_page`]).
    #[must_use]
    pub fn inventory_folder_page(
        &self,
        folder_id: InventoryFolderKey,
        before: Option<InventoryCursor>,
        limit: usize,
    ) -> (Vec<FolderInfo>, Vec<ItemInfo>, Option<InventoryCursor>) {
        let (folders, items) = self.inventory.children(folder_id);
        let total = folders.len().saturating_add(items.len());
        let start = before.map_or(0, InventoryCursor::consumed);
        let mut folder_infos = Vec::new();
        let mut item_infos = Vec::new();
        let combined = folders
            .iter()
            .copied()
            .map(Child::Folder)
            .chain(items.iter().copied().map(Child::Item));
        for child in combined.skip(start).take(limit) {
            match child {
                Child::Folder(folder) => {
                    let state = self
                        .inventory
                        .folder_state(folder.folder_id)
                        .unwrap_or(FolderState::Unknown);
                    folder_infos.push(FolderInfo::from_folder(folder, state));
                }
                Child::Item(item) => item_infos.push(ItemInfo::from_item(item)),
            }
        }
        let consumed = folder_infos.len().saturating_add(item_infos.len());
        let next_pos = start.saturating_add(consumed);
        let next = (next_pos < total).then(|| InventoryCursor::new(next_pos));
        (folder_infos, item_infos, next)
    }

    /// The contents [`FolderState`] of `folder_id` (`Unknown` / `Fetching` /
    /// `Loaded { version }`), or `None` if the folder is not in the model. A
    /// skeleton folder is `Unknown` until its contents are fetched in their own
    /// right; a descendents reply for the folder flips it to `Loaded`.
    #[must_use]
    pub fn folder_fetch_state(&self, folder_id: InventoryFolderKey) -> Option<FolderState> {
        self.inventory.folder_state(folder_id)
    }

    /// Marks `folder_id` as having a contents fetch in flight, flipping it to
    /// [`Fetching`](FolderState::Fetching) so the background crawl scheduler does
    /// not also pick it and [`Session::folder_fetch_state`] reflects the pending
    /// request. The UDP [`Session::request_folder_contents`] does this
    /// internally; a runtime that instead issues the fetch over the modern CAPS
    /// path (`FetchInventoryDescendents2` / `FetchLibDescendents2`, which Second
    /// Life requires) calls this so the in-flight bookkeeping matches either way.
    pub fn mark_folder_fetching(&mut self, folder_id: InventoryFolderKey) {
        self.inventory.mark_folder_fetching(folder_id);
    }

    /// Which tree — the agent's own inventory ([`InventoryOwner::Agent`]) or the
    /// read-only shared Library ([`InventoryOwner::Library`]) — the known folder
    /// `folder_id` belongs to, or `None` if it is not in the model.
    #[must_use]
    pub fn inventory_owner(&self, folder_id: InventoryFolderKey) -> Option<InventoryOwner> {
        self.inventory.folder_owner(folder_id)
    }

    // ---- Inventory disk cache (sans-I/O core) -------------------------------

    /// Serialises the cacheable snapshot of one tree (`owner`) to the un-gzipped
    /// disk-cache bytes: a 4-byte big-endian version header
    /// ([`INVENTORY_CACHE_VERSION`](crate::INVENTORY_CACHE_VERSION), matching
    /// Firestorm) followed by a binary-LLSD `{ categories, items }` map. Only
    /// fully-fetched ([`Loaded`](FolderState::Loaded)) folders and the items in
    /// them are written. The runtime shell gzips the result and writes it to
    /// `<agent-uuid>.inv.llsd.gz` (or `.lib.inv.llsd.gz` for
    /// [`InventoryOwner::Library`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if an item fails to serialise.
    pub fn inventory_cache_bytes(&self, owner: InventoryOwner) -> Result<Vec<u8>, Error> {
        Ok(super::inventory_cache::inventory_to_cache_bytes(
            &self.inventory,
            owner,
        )?)
    }

    /// Loads a disk cache (the un-gzipped bytes the runtime shell read back) into
    /// the held model under `owner`, **before** the login skeleton arrives: every
    /// cached folder lands [`Loaded`](FolderState::Loaded) at its stored version,
    /// to be confirmed or invalidated by the later
    /// [`Session::merge_inventory_skeleton`]. Returns `true` if a version-valid
    /// cache was loaded, `false` if the bytes were cold (wrong/short version
    /// header) and nothing was loaded — in which case the skeleton merge will
    /// refetch the whole tree.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if a version-valid payload is not decodable binary
    /// LLSD.
    pub fn load_inventory_cache(
        &mut self,
        owner: InventoryOwner,
        bytes: &[u8],
    ) -> Result<bool, Error> {
        match super::inventory_cache::inventory_from_cache_bytes(bytes)
            .map_err(sl_wire::WireError::from)?
        {
            Some(cached) => {
                super::inventory_cache::load_cached_into(&mut self.inventory, &cached, owner);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Reconciles the held `owner` tree against the authoritative login skeleton,
    /// returning the folders that still need a contents fetch (the initial
    /// background-fetch queue). Run once per owner after
    /// [`Session::load_inventory_cache`]: a cached folder whose version matches the
    /// skeleton keeps its loaded contents; a mismatch, a skeleton-only folder, or
    /// a server-deleted folder is invalidated (its stale cached contents dropped),
    /// mirroring Firestorm's `loadSkeleton`.
    pub fn merge_inventory_skeleton(
        &mut self,
        owner: InventoryOwner,
        skeleton: &[InventoryFolder],
    ) -> Vec<InventoryFolderKey> {
        self.inventory.merge_skeleton(skeleton, owner)
    }

    /// Whether the held inventory model has cacheable changes since the last
    /// [`Session::clear_inventory_dirty`] — set by every fold/mutation that can
    /// alter the cacheable snapshot. The runtime cache shell's optional
    /// dirty/idle save checks this to skip a no-op rewrite (the cache is
    /// otherwise written only at logout, mirroring Firestorm's shutdown-only
    /// save).
    #[must_use]
    pub const fn inventory_dirty(&self) -> bool {
        self.inventory.is_dirty()
    }

    /// Clears the inventory dirty flag, called by the runtime cache shell once it
    /// has persisted the cache (or right after the post-login load+merge, to reset
    /// the baseline so the first idle tick does not re-save an unchanged model).
    pub const fn clear_inventory_dirty(&mut self) {
        self.inventory.clear_dirty();
    }

    /// Inserts/updates a folder in the held model (agent tree), maintaining the
    /// index and fetch state. A version of `0` (as carried by a descendents
    /// reply's sub-folders, which omit it) does not clobber a known version from
    /// the login skeleton.
    fn cache_inventory_folder(&mut self, folder: InventoryFolder) {
        self.inventory.cache_folder(folder, InventoryOwner::Agent);
    }

    /// Merges a batch of folders and items into the held model under `owner` (from
    /// a descendents fetch or a simulator push). The agent's own mutations and
    /// bulk updates fold under [`InventoryOwner::Agent`]; a descendents reply folds
    /// under the tree its target folder belongs to (so a Library fetch stays in the
    /// Library tree).
    fn cache_inventory(
        &mut self,
        folders: &[InventoryFolder],
        items: &[InventoryItem],
        owner: InventoryOwner,
    ) {
        for folder in folders {
            self.inventory.cache_folder(folder.clone(), owner);
        }
        for item in items {
            self.inventory.cache_item(item.clone(), owner);
        }
    }

    /// The tree a descendents reply for `folder_id` belongs to — the owner already
    /// recorded for that folder (seeded from the agent or Library skeleton), or
    /// [`InventoryOwner::Agent`] if the folder is somehow unknown.
    fn inventory_reply_owner(&self, folder_id: InventoryFolderKey) -> InventoryOwner {
        self.inventory
            .folder_owner(folder_id)
            .unwrap_or(InventoryOwner::Agent)
    }

    /// Inserts/updates an item in the held model (agent tree), maintaining the
    /// index.
    fn cache_inventory_item(&mut self, item: InventoryItem) {
        self.inventory.cache_item(item, InventoryOwner::Agent);
    }

    /// Allocates the next async inventory `CallbackID` (never zero).
    fn next_inventory_callback(&mut self) -> InventoryCallbackId {
        self.inventory.next_callback()
    }

    // ---- Inventory mutation over UDP (#30) ---------------------------------

    /// Creates a new inventory folder `folder_id` (a fresh, caller-chosen id)
    /// named `name` of [`FolderType`] under `parent_id`, via
    /// `CreateInventoryFolder`, returning the new folder's key for symmetry with
    /// the read accessors. The simulator sends no reply, so the folder is added to
    /// the local cache optimistically. On Second Life the modern AIS3 CAPS path
    /// (or the `CreateInventoryCategory` cap) returns a confirmed result instead.
    ///
    /// `sl-proto` is sans-IO and mints no UUIDs: the caller allocates the fresh v4
    /// `folder_id` (the protocol lets the client choose *folder* ids; the
    /// simulator allocates *item* ids and echoes a callback id). The id is
    /// validated rather than generated.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInventoryOperation`] if `folder_id` is nil or
    /// already present in the held model (which would clobber an existing folder),
    /// [`Error::NoCircuit`] if no circuit is established, or [`Error::Wire`] on an
    /// encode failure.
    pub fn create_inventory_folder(
        &mut self,
        folder_id: InventoryFolderKey,
        parent_id: InventoryFolderKey,
        folder_type: FolderType,
        name: &str,
        now: Instant,
    ) -> Result<InventoryFolderKey, Error> {
        if folder_id.uuid().is_nil() {
            return Err(Error::InvalidInventoryOperation(
                "a new inventory folder id must not be nil",
            ));
        }
        if self.inventory.folder_state(folder_id).is_some() {
            return Err(Error::InvalidInventoryOperation(
                "an inventory folder with this id already exists",
            ));
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_inventory_folder(
            folder_id.uuid(),
            parent_id.uuid(),
            folder_type.to_code(),
            name,
            now,
        )?;
        self.cache_inventory_folder(InventoryFolder {
            folder_id,
            parent_id: crate::types::optional_key_from_wire(parent_id.uuid()),
            name: name.to_owned(),
            folder_type: folder_type.to_code(),
            version: 1,
        });
        Ok(folder_id)
    }

    /// Renames / re-types / re-parents the existing folder `folder_id` via
    /// `UpdateInventoryFolder` (an all-fields overwrite). The cache is updated
    /// optimistically. To change a single attribute without risking a clobber of
    /// the others, prefer the focused [`Session::rename_inventory_folder`] /
    /// [`Session::retype_inventory_folder`] helpers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn update_inventory_folder(
        &mut self,
        folder_id: InventoryFolderKey,
        parent_id: InventoryFolderKey,
        folder_type: FolderType,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_inventory_folder(
            folder_id.uuid(),
            parent_id.uuid(),
            folder_type.to_code(),
            name,
            now,
        )?;
        self.cache_inventory_folder(InventoryFolder {
            folder_id,
            parent_id: crate::types::optional_key_from_wire(parent_id.uuid()),
            name: name.to_owned(),
            folder_type: folder_type.to_code(),
            version: self
                .inventory
                .folder(folder_id)
                .map_or(1, |folder| folder.version),
        });
        Ok(())
    }

    /// Renames the folder `folder_id` to `name` without touching its parent or
    /// type — a clobber-free wrapper over the all-fields
    /// [`Session::update_inventory_folder`] that reads the untouched fields from
    /// the cached folder, so a rename cannot accidentally re-parent or re-type it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInventoryOperation`] if the folder's metadata is
    /// not in the held model (nothing to read the other fields from),
    /// [`Error::NoCircuit`] if no circuit is established, or [`Error::Wire`] on an
    /// encode failure.
    pub fn rename_inventory_folder(
        &mut self,
        folder_id: InventoryFolderKey,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let parent = self.cached_folder_parent(folder_id)?;
        let folder_type = self.cached_folder_type(folder_id)?;
        self.update_inventory_folder(folder_id, parent, folder_type, name, now)
    }

    /// Re-types the folder `folder_id` to `folder_type` without touching its name
    /// or parent — the clobber-free type-only companion of
    /// [`Session::rename_inventory_folder`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInventoryOperation`] if the folder's metadata is
    /// not in the held model, [`Error::NoCircuit`] if no circuit is established,
    /// or [`Error::Wire`] on an encode failure.
    pub fn retype_inventory_folder(
        &mut self,
        folder_id: InventoryFolderKey,
        folder_type: FolderType,
        now: Instant,
    ) -> Result<(), Error> {
        let parent = self.cached_folder_parent(folder_id)?;
        let name = self
            .inventory
            .folder(folder_id)
            .map(|folder| folder.name.clone())
            .ok_or(Error::InvalidInventoryOperation(
                "the folder to re-type is not in the inventory model",
            ))?;
        self.update_inventory_folder(folder_id, parent, folder_type, &name, now)
    }

    /// The cached parent key of `folder_id` (the inventory root's nil key for a
    /// top-level folder), or an error if the folder is not in the model.
    fn cached_folder_parent(
        &self,
        folder_id: InventoryFolderKey,
    ) -> Result<InventoryFolderKey, Error> {
        let folder = self
            .inventory
            .folder(folder_id)
            .ok_or(Error::InvalidInventoryOperation(
                "the folder is not in the inventory model",
            ))?;
        Ok(folder
            .parent_id
            .unwrap_or_else(|| InventoryFolderKey::from(Uuid::nil())))
    }

    /// The cached [`FolderType`] of `folder_id`, or an error if the folder is not
    /// in the model.
    fn cached_folder_type(&self, folder_id: InventoryFolderKey) -> Result<FolderType, Error> {
        let folder = self
            .inventory
            .folder(folder_id)
            .ok_or(Error::InvalidInventoryOperation(
                "the folder is not in the inventory model",
            ))?;
        Ok(FolderType::from_code(folder.folder_type))
    }

    /// Re-parents the folder `folder_id` under `parent_id` via
    /// `MoveInventoryFolder` (without re-timestamping its children). Use
    /// [`Session::move_inventory_folders`] to move several at once.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn move_inventory_folder(
        &mut self,
        folder_id: InventoryFolderKey,
        parent_id: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        self.move_inventory_folders(&[(folder_id, parent_id)], false, now)
    }

    /// Re-parents several folders in one `MoveInventoryFolder` (each a
    /// `(folder, new_parent)` pair). `stamp` asks the simulator to re-timestamp
    /// the moved children. The cache is updated optimistically.
    ///
    /// Each move is checked O(1) against the held parent→children index *before*
    /// anything is sent: the target parent must be in the model, and the move
    /// must not make a folder its own ancestor. If **any** move fails the check,
    /// none is sent and the model is left unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInventoryOperation`] if a target parent is not in
    /// the held model, or a move would form a cycle (a folder into itself or one
    /// of its descendants); [`Error::NoCircuit`] if no circuit is established; or
    /// [`Error::Wire`] on an encode failure.
    pub fn move_inventory_folders(
        &mut self,
        moves: &[(InventoryFolderKey, InventoryFolderKey)],
        stamp: bool,
        now: Instant,
    ) -> Result<(), Error> {
        for &(folder_id, parent_id) in moves {
            if !self.inventory.contains_folder(parent_id) {
                return Err(Error::InvalidInventoryOperation(
                    "the move target parent is not in the inventory model",
                ));
            }
            if self.inventory.is_self_or_descendant(folder_id, parent_id) {
                return Err(Error::InvalidInventoryOperation(
                    "moving a folder into itself or a descendant would form a cycle",
                ));
            }
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let wire: Vec<(Uuid, Uuid)> = moves
            .iter()
            .map(|(folder_id, parent_id)| (folder_id.uuid(), parent_id.uuid()))
            .collect();
        circuit.send_move_inventory_folders(&wire, stamp, now)?;
        for &(folder_id, parent_id) in moves {
            self.inventory.reparent_folder(folder_id, parent_id);
        }
        Ok(())
    }

    /// Deletes folders (moved to the trash server-side) via
    /// `RemoveInventoryFolder`, dropping them and their cached descendents.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn remove_inventory_folders(
        &mut self,
        folder_ids: &[InventoryFolderKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let wire: Vec<Uuid> = folder_ids.iter().map(InventoryFolderKey::uuid).collect();
        circuit.send_remove_inventory_folders(&wire, now)?;
        for folder_id in folder_ids {
            self.inventory.remove_folder(*folder_id);
        }
        Ok(())
    }

    /// Creates a new inventory item via `CreateInventoryItem`, returning the
    /// async callback id the simulator echoes in its `UpdateCreateInventoryItem`
    /// reply ([`Event::InventoryItemCreated`]). The simulator allocates the
    /// item's id.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn create_inventory_item(
        &mut self,
        new: &NewInventoryItem,
        now: Instant,
    ) -> Result<InventoryCallbackId, Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_inventory_item(new, callback_id, now)?;
        Ok(callback_id)
    }

    /// Creates a new **script** inventory item via `CreateInventoryItem`,
    /// returning the async callback id echoed in the simulator's
    /// `UpdateCreateInventoryItem` reply ([`Event::InventoryItemCreated`]).
    ///
    /// The item is created empty of a client-supplied asset (nil transaction id),
    /// so **the simulator fills it with its default script body** — a compilable
    /// starter, never an empty (non-compiling) script — selecting the LSL or Lua
    /// default from `language` (carried as the item's subtype, exactly as the
    /// viewer's "New Script" does). To then replace the body with custom source
    /// and compile it, follow up with
    /// [`Command::UploadScript`](crate::Command::UploadScript) once the item id
    /// arrives. A Lua item only takes on a Lua default on a SLua-capable grid;
    /// elsewhere the simulator falls back to the LSL default.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn create_script(
        &mut self,
        folder_id: InventoryFolderKey,
        name: &str,
        description: &str,
        next_owner_mask: u32,
        language: ScriptLanguage,
        now: Instant,
    ) -> Result<InventoryCallbackId, Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_script_item(
            folder_id,
            name,
            description,
            next_owner_mask,
            language.subtype(),
            callback_id,
            now,
        )?;
        Ok(callback_id)
    }

    /// Creates an inventory **link** to an existing item or folder via
    /// `LinkInventoryItem`, returning the async callback id the simulator echoes
    /// in its `UpdateCreateInventoryItem` reply ([`Event::InventoryItemCreated`]).
    /// The simulator allocates the link item's id. A link is a lightweight
    /// pointer; removing it leaves the linked target intact.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn link_inventory_item(
        &mut self,
        new: &NewInventoryLink,
        now: Instant,
    ) -> Result<InventoryCallbackId, Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_link_inventory_item(new, callback_id, now)?;
        Ok(callback_id)
    }

    /// Rewrites an item's metadata / permissions via `UpdateInventoryItem`. A
    /// non-nil `transaction_id` binds a freshly uploaded asset to the item. The
    /// cache is updated optimistically.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn update_inventory_item(
        &mut self,
        item: &InventoryItem,
        transaction_id: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_inventory_item(item, transaction_id.get(), callback_id, now)?;
        self.cache_inventory_item(item.clone());
        Ok(())
    }

    /// Renames the item `item_id` to `name` without touching any other field — a
    /// clobber-free wrapper over the all-fields [`Session::update_inventory_item`]
    /// that reads the rest of the item from the cache, so a rename cannot
    /// accidentally re-parent, re-asset, or reset its permissions. No asset
    /// transaction is bound (the transaction id is nil).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInventoryOperation`] if the item is not in the held
    /// model, [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn rename_inventory_item(
        &mut self,
        item_id: InventoryKey,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let mut item = self.cached_item(item_id)?;
        name.clone_into(&mut item.name);
        self.update_inventory_item(&item, TransactionId::from(Uuid::nil()), now)
    }

    /// Replaces the permission masks of item `item_id` without touching any other
    /// field — the clobber-free permissions-only companion of
    /// [`Session::rename_inventory_item`] (reads the rest from the cache). No
    /// asset transaction is bound.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInventoryOperation`] if the item is not in the held
    /// model, [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn set_inventory_item_permissions(
        &mut self,
        item_id: InventoryKey,
        permissions: Permissions5,
        now: Instant,
    ) -> Result<(), Error> {
        let mut item = self.cached_item(item_id)?;
        item.permissions = permissions;
        self.update_inventory_item(&item, TransactionId::from(Uuid::nil()), now)
    }

    /// A clone of the cached [`InventoryItem`] `item_id`, or an error if it is not
    /// in the held model — the read step shared by the clobber-free item helpers.
    fn cached_item(&self, item_id: InventoryKey) -> Result<InventoryItem, Error> {
        self.inventory
            .item(item_id)
            .cloned()
            .ok_or(Error::InvalidInventoryOperation(
                "the item is not in the inventory model",
            ))
    }

    /// Moves the item `item_id` into folder `folder_id`, optionally renaming it
    /// (an empty `new_name` keeps the current name), via `MoveInventoryItem`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn move_inventory_item(
        &mut self,
        item_id: InventoryKey,
        folder_id: InventoryFolderKey,
        new_name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        self.move_inventory_items(&[(item_id, folder_id, new_name.to_owned())], false, now)
    }

    /// Moves several items in one `MoveInventoryItem` (each `(item, folder,
    /// new_name)`; an empty `new_name` keeps the name). `stamp` asks the
    /// simulator to re-timestamp. The cache is updated optimistically.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn move_inventory_items(
        &mut self,
        moves: &[(InventoryKey, InventoryFolderKey, String)],
        stamp: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let wire: Vec<(Uuid, Uuid, String)> = moves
            .iter()
            .map(|(item_id, folder_id, new_name)| {
                (item_id.uuid(), folder_id.uuid(), new_name.clone())
            })
            .collect();
        circuit.send_move_inventory_items(&wire, stamp, now)?;
        for (item_id, folder_id, new_name) in moves {
            self.inventory.move_item(*item_id, *folder_id, new_name);
        }
        Ok(())
    }

    /// Copies the item `old_item_id` (owned by `old_agent_id`) into
    /// `new_folder_id` under `new_name`, via `CopyInventoryItem`. The simulator
    /// answers with a `BulkUpdateInventory` for the new item
    /// ([`Event::InventoryBulkUpdate`]); the returned callback id correlates it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn copy_inventory_item(
        &mut self,
        old_agent_id: AgentKey,
        old_item_id: InventoryKey,
        new_folder_id: InventoryFolderKey,
        new_name: &str,
        now: Instant,
    ) -> Result<InventoryCallbackId, Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_copy_inventory_item(
            old_agent_id,
            old_item_id.uuid(),
            new_folder_id.uuid(),
            new_name,
            callback_id,
            now,
        )?;
        Ok(callback_id)
    }

    /// Deletes items via `RemoveInventoryItem`, dropping them from the cache.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn remove_inventory_items(
        &mut self,
        item_ids: &[InventoryKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let wire: Vec<Uuid> = item_ids.iter().map(InventoryKey::uuid).collect();
        circuit.send_remove_inventory_items(&wire, now)?;
        for item_id in item_ids {
            self.inventory.remove_item(*item_id);
        }
        Ok(())
    }

    /// Rewrites the flags of item `item_id` via `ChangeInventoryItemFlags`. The
    /// cache is updated optimistically.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn change_inventory_item_flags(
        &mut self,
        item_id: InventoryKey,
        flags: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_change_inventory_item_flags(item_id.uuid(), flags, now)?;
        self.inventory.set_item_flags(item_id, flags);
        Ok(())
    }

    /// Empties a folder's contents (e.g. the Trash) via
    /// `PurgeInventoryDescendents`, dropping its cached descendents.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn purge_inventory_descendents(
        &mut self,
        folder_id: InventoryFolderKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_purge_inventory_descendents(folder_id.uuid(), now)?;
        self.inventory.purge_descendents(folder_id);
        Ok(())
    }

    /// Deletes a mixed set of folders and items in one `RemoveInventoryObjects`,
    /// dropping them (and the folders' descendents) from the cache.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn remove_inventory_objects(
        &mut self,
        folder_ids: &[InventoryFolderKey],
        item_ids: &[InventoryKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let folder_wire: Vec<Uuid> = folder_ids.iter().map(InventoryFolderKey::uuid).collect();
        let item_wire: Vec<Uuid> = item_ids.iter().map(InventoryKey::uuid).collect();
        circuit.send_remove_inventory_objects(&folder_wire, &item_wire, now)?;
        for folder_id in folder_ids {
            self.inventory.remove_folder(*folder_id);
        }
        for item_id in item_ids {
            self.inventory.remove_item(*item_id);
        }
        Ok(())
    }

    /// Requests the region's info (agent and object limits) via
    /// `RequestRegionInfo`. The reply arrives as an [`Event::RegionLimits`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_region_info(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_request_region_info(now)?;
        Ok(())
    }

    /// Resolves agent ids to their legacy names via `UUIDNameRequest`. Replies
    /// arrive as [`Event::AvatarNames`]; a single request may be answered by
    /// several replies, and each reply may batch several ids. The session does
    /// not resolve or cache names itself — this is the primitive a caller uses to
    /// turn the UUIDs that pervade the protocol (object owners, estate managers,
    /// inventory creators, …) into legacy names on demand.
    ///
    /// `ids` is split into MTU-sized batches automatically; an empty slice sends
    /// nothing. Duplicate ids are sent as-is (the simulator de-duplicates).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if a request fails to encode.
    pub fn request_avatar_names(&mut self, ids: &[AgentKey], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let ids: Vec<Uuid> = ids.iter().map(AgentKey::uuid).collect();
        for batch in ids.chunks(UUID_NAMES_PER_REQUEST) {
            circuit.send_uuid_name_request(batch, now)?;
        }
        Ok(())
    }

    /// Resolves group ids to their names via `UUIDGroupNameRequest`. Replies
    /// arrive as [`Event::GroupNames`]. See [`Self::request_avatar_names`] for the
    /// batching and caching semantics.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if a request fails to encode.
    pub fn request_group_names(&mut self, ids: &[GroupKey], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let ids: Vec<Uuid> = ids.iter().map(GroupKey::uuid).collect();
        for batch in ids.chunks(UUID_NAMES_PER_REQUEST) {
            circuit.send_uuid_group_name_request(batch, now)?;
        }
        Ok(())
    }

    /// Requests the agent's current L$ balance via `MoneyBalanceRequest`. The
    /// reply arrives as an [`Event::MoneyBalance`]. The simulator also pushes a
    /// `MoneyBalanceReply` unsolicited whenever a transaction changes the balance.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_money_balance(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_money_balance_request(now)?;
        Ok(())
    }

    /// Requests the grid's economy data (upload/claim/group prices and region
    /// object capacity) via `EconomyDataRequest`. The reply arrives as an
    /// [`Event::EconomyData`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_economy_data(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_economy_data_request(now)?;
        Ok(())
    }

    /// Pays `amount` L$ to another avatar or object via `MoneyTransferRequest`.
    /// `kind` selects the transaction type (e.g. [`MoneyTransactionType::Gift`]
    /// for a direct avatar payment, [`MoneyTransactionType::PayObject`] for a
    /// scripted object); `description` annotates the transaction. The grid pushes
    /// a fresh [`Event::MoneyBalance`] once the transfer settles. The amount is
    /// clamped to the `i32` wire range.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn send_money_transfer(
        &mut self,
        dest: Uuid,
        amount: LindenAmount,
        kind: MoneyTransactionType,
        description: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let amount = i32::try_from(amount.0).unwrap_or(i32::MAX);
        circuit.send_money_transfer(dest, amount, kind.to_i32(), description, now)?;
        Ok(())
    }

    /// Requests `ParcelProperties` for the parcel overlapping the given metre
    /// rectangle (region-local coordinates). `sequence_id` is echoed back in the
    /// reply ([`Event::ParcelProperties`]) so callers can match outstanding
    /// queries.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_properties(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        sequence_id: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_properties_request(west, south, east, north, sequence_id, now)?;
        Ok(())
    }

    /// Requests `ParcelProperties` for the parcel identified by its region-local
    /// id via `ParcelPropertiesRequestByID` (rather than by a metre rectangle as
    /// [`request_parcel_properties`](Self::request_parcel_properties) does).
    /// `sequence_id` is echoed back in the reply ([`Event::ParcelProperties`]) so
    /// callers can match outstanding queries.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `local_id`'s circuit has gone away,
    /// or [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_properties_by_id(
        &mut self,
        local_id: ScopedParcelId,
        sequence_id: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        circuit.send_parcel_properties_request_by_id(local_id.id, sequence_id, now)?;
        Ok(())
    }

    /// Sets the parcel `local_id`'s auto-return time for other people's objects
    /// via `ParcelSetOtherCleanTime`. `clean_time` is rounded down to whole
    /// minutes; [`Duration::ZERO`](std::time::Duration) disables auto-return.
    /// Requires parcel ownership / land edit rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `local_id`'s circuit has gone away,
    /// or [`Error::Wire`] if the request fails to encode.
    pub fn set_parcel_other_clean_time(
        &mut self,
        local_id: ScopedParcelId,
        clean_time: std::time::Duration,
        now: Instant,
    ) -> Result<(), Error> {
        let minutes = i32::try_from(clean_time.as_secs() / 60).unwrap_or(i32::MAX);
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        circuit.send_parcel_set_other_clean_time(local_id.id, minutes, now)?;
        Ok(())
    }

    /// Terraforms a piece of land via `ModifyLand`, applying a single brush
    /// stroke described by `edit` to the agent's current region. Requires land
    /// edit rights on the affected parcel(s).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn modify_land(&mut self, edit: &LandEdit, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_modify_land(edit, now)?;
        Ok(())
    }

    /// Undoes the agent's last terraform edit in the current region via
    /// `UndoLand`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn undo_land(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_undo_land(now)?;
        Ok(())
    }

    /// Edits a parcel's settings via `ParcelPropertiesUpdate`. Build the
    /// [`ParcelUpdate`] from [`ParcelUpdate::default`], setting `local_id` (from
    /// [`Event::ParcelProperties`]) and the fields to change. Requires the agent
    /// to own the parcel or hold estate/god powers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_parcel(&mut self, update: &ParcelUpdate, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_properties_update(update, now)?;
        Ok(())
    }

    /// Requests a parcel's allow or ban list via `ParcelAccessListRequest`. The
    /// reply arrives as [`Event::ParcelAccessList`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_access_list(
        &mut self,
        local_id: ScopedParcelId,
        scope: ParcelAccessScope,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_access_list_request(local_id, scope.to_u32(), now)?;
        Ok(())
    }

    /// Replaces a parcel's allow or ban list via `ParcelAccessListUpdate`. An
    /// empty `entries` clears the list. Requires parcel ownership / land edit
    /// rights.
    ///
    /// `transaction_id` groups the packets of one logical update; it must be
    /// unique per update (the runtime mints a fresh one), because the reference
    /// simulator only clears the existing entries before applying the new ones
    /// when the id differs from the previous update's — reusing a stale or nil id
    /// *appends* to the list instead of replacing it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_parcel_access_list(
        &mut self,
        local_id: ScopedParcelId,
        scope: ParcelAccessScope,
        entries: &[ParcelAccessEntry],
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_access_list_update(
            local_id,
            scope.to_u32(),
            entries,
            transaction_id,
            now,
        )?;
        Ok(())
    }

    /// Requests a parcel's dwell (traffic) value via `ParcelDwellRequest`. The
    /// reply arrives as [`Event::ParcelDwell`]. Dwell is public — no ownership
    /// required.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_dwell(
        &mut self,
        local_id: ScopedParcelId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_dwell_request(local_id, now)?;
        Ok(())
    }

    /// Buys a parcel via `ParcelBuy` for `price` L$ covering `area` m². Pass a
    /// `group_id` with `is_group_owned` to buy on a group's behalf (nil/false for
    /// a personal purchase).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn buy_parcel(
        &mut self,
        local_id: ScopedParcelId,
        price: i32,
        area: i32,
        group_id: Option<GroupKey>,
        is_group_owned: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_buy(local_id, price, area, group_id, is_group_owned, now)?;
        Ok(())
    }

    /// Returns objects on a parcel via `ParcelReturnObjects`. `return_type`
    /// selects which objects (owner/group/other/for-sale — combine
    /// [`ParcelReturnType`] constants); `owner_ids`/`task_ids` optionally scope it
    /// (use [`ParcelReturnType::LIST`] with `task_ids` to return specific
    /// objects). Requires parcel ownership / land rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn return_parcel_objects(
        &mut self,
        local_id: ScopedParcelId,
        return_type: ParcelReturnType,
        owner_ids: &[OwnerKey],
        task_ids: &[ObjectKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        let owner_ids: Vec<Uuid> = owner_ids.iter().map(OwnerKey::uuid).collect();
        circuit.send_parcel_return_objects(local_id, return_type.0, &owner_ids, task_ids, now)?;
        Ok(())
    }

    /// Selects (highlights) objects on a parcel via `ParcelSelectObjects`.
    /// `return_type` selects which objects; pass [`ParcelReturnType::LIST`] with
    /// `object_ids` to select specific objects.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn select_parcel_objects(
        &mut self,
        local_id: ScopedParcelId,
        return_type: ParcelReturnType,
        object_ids: &[ObjectKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_select_objects(local_id, return_type.0, object_ids, now)?;
        Ok(())
    }

    /// Deeds a parcel to a group via `ParcelDeedToGroup`. Requires parcel
    /// ownership and membership of `group_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn deed_parcel_to_group(
        &mut self,
        local_id: ScopedParcelId,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_deed_to_group(local_id, group_id, now)?;
        Ok(())
    }

    /// Reclaims a parcel to the estate via `ParcelReclaim` (estate-manager
    /// operation).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn reclaim_parcel(&mut self, local_id: ScopedParcelId, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_reclaim(local_id, now)?;
        Ok(())
    }

    /// Releases (abandons) a parcel back to the estate via `ParcelRelease`.
    /// Requires parcel ownership.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn release_parcel(&mut self, local_id: ScopedParcelId, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_release(local_id, now)?;
        Ok(())
    }

    /// Joins all owned, leased parcels within the metre rectangle into one parcel
    /// via `ParcelJoin`. Requires land rights over every parcel in the rectangle.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn join_parcels(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_join(west, south, east, north, now)?;
        Ok(())
    }

    /// Subdivides a parcel via `ParcelDivide`: the metre rectangle (a subsection
    /// of exactly one parcel) becomes a new parcel. Requires land rights over the
    /// parcel.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn divide_parcel(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_divide(west, south, east, north, now)?;
        Ok(())
    }

    /// Requests the per-owner object tallies for a parcel via
    /// `ParcelObjectOwnersRequest`. The reply arrives as
    /// [`Event::ParcelObjectOwners`]. Requires parcel ownership / land rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_object_owners(
        &mut self,
        local_id: ScopedParcelId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_object_owners_request(local_id, now)?;
        Ok(())
    }

    /// Buys a temporary access pass to a parcel via `ParcelBuyPass` at its
    /// configured pass price.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn buy_parcel_pass(&mut self, local_id: ScopedParcelId, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_parcel_buy_pass(local_id, now)?;
        Ok(())
    }

    /// Disables (stops) scripted objects on a parcel via `ParcelDisableObjects`.
    /// `return_type` selects which objects; pass [`ParcelReturnType::LIST`] with
    /// `task_ids` to disable specific objects. Requires parcel ownership / land
    /// rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn disable_parcel_objects(
        &mut self,
        local_id: ScopedParcelId,
        return_type: ParcelReturnType,
        owner_ids: &[OwnerKey],
        task_ids: &[ObjectKey],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        let owner_ids: Vec<Uuid> = owner_ids.iter().map(OwnerKey::uuid).collect();
        circuit.send_parcel_disable_objects(local_id, return_type.0, &owner_ids, task_ids, now)?;
        Ok(())
    }

    /// Requests a parcel's basic listing by its grid-wide parcel id via
    /// `ParcelInfoRequest`. The reply arrives as [`Event::ParcelDetails`]. Resolve
    /// the parcel id from a region location first with the runtimes'
    /// `RequestRemoteParcelId` command (the `RemoteParcelRequest` capability).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_info(&mut self, parcel_id: ParcelKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_info_request(parcel_id, now)?;
        Ok(())
    }

    /// Requests the region's (or a parcel's) **top scripts / top colliders**
    /// report via a UDP `LandStatRequest`. The reply arrives as
    /// [`Event::LandStatReply`]. `report_type` selects the report;
    /// `parcel_local_id` scopes it to a parcel (`0` for the whole region);
    /// `filter` narrows it to objects/owners whose name contains the string (empty
    /// for none); `request_flags` is passed through verbatim. Requires
    /// estate-manager rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_land_stat(
        &mut self,
        report_type: LandStatReportType,
        request_flags: u32,
        filter: &str,
        parcel_local_id: ScopedParcelId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(parcel_local_id.circuit)?;
        let parcel_local_id = parcel_local_id.id;
        circuit.send_land_stat_request(
            report_type.to_u32(),
            request_flags,
            filter,
            parcel_local_id,
            now,
        )?;
        Ok(())
    }

    /// Requests the current region's estate configuration and access lists via
    /// `EstateOwnerMessage`/`getinfo`. The reply arrives as an
    /// [`Event::EstateInfo`] plus one or more [`Event::EstateAccessList`].
    /// Requires the agent to be the estate owner or a manager.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_estate_info(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("getinfo", &[], now)?;
        Ok(())
    }

    /// Adds or removes an agent/group from one of the estate's access lists
    /// (allowed agents/groups, bans, managers) via `estateaccessdelta`. The
    /// updated list arrives as [`Event::EstateAccessList`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_estate_access(
        &mut self,
        delta: EstateAccessDelta,
        target: OwnerKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let params = [
            circuit.agent_id.to_string(),
            delta.to_u32().to_string(),
            target.uuid().to_string(),
        ];
        circuit.send_estate_owner_message("estateaccessdelta", &params, now)?;
        Ok(())
    }

    /// Kicks (ejects) an agent from the region via `EstateOwnerMessage`/
    /// `kickestate`. The agent is sent home.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn kick_estate_user(&mut self, target: AgentKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("kickestate", &[target.uuid().to_string()], now)?;
        Ok(())
    }

    /// Teleports an agent home via `EstateOwnerMessage`/`teleporthomeuser`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn teleport_home_user(&mut self, target: AgentKey, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let params = [circuit.agent_id.to_string(), target.uuid().to_string()];
        circuit.send_estate_owner_message("teleporthomeuser", &params, now)?;
        Ok(())
    }

    /// Teleports every agent in the region home via `EstateOwnerMessage`/
    /// `teleporthomeallusers`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn teleport_home_all_users(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("teleporthomeallusers", &[], now)?;
        Ok(())
    }

    /// Schedules a region restart in `seconds` via `EstateOwnerMessage`/
    /// `restart`. Pass `-1` to push a pending restart out by an hour (the
    /// reference viewer's "cancel restart").
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn restart_region(&mut self, seconds: i32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("restart", &[seconds.to_string()], now)?;
        Ok(())
    }

    /// Sends an estate-wide notice (blue-box message) to everyone in the estate
    /// via `EstateOwnerMessage`/`simulatormessage`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn send_estate_message(&mut self, message: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let sender = circuit.agent_id.to_string();
        // ParamList: grid_x, grid_y (unused, "-1"), sender id, sender name, body.
        let params = [
            "-1".to_owned(),
            "-1".to_owned(),
            sender.clone(),
            sender,
            message.to_owned(),
        ];
        circuit.send_estate_owner_message("simulatormessage", &params, now)?;
        Ok(())
    }

    /// Updates the region's settings (maturity, agent limit, object bonus, the
    /// terraform/fly/damage/land-resell/push/parcel-change toggles) via
    /// `EstateOwnerMessage`/`setregioninfo`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_region_info(
        &mut self,
        update: &RegionInfoUpdate,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let yn = |flag: bool| if flag { "Y" } else { "N" }.to_owned();
        let params = [
            yn(update.block_terraform),
            yn(update.block_fly),
            yn(update.allow_damage),
            yn(update.allow_land_resell),
            format!("{:.6}", f64::from(update.agent_limit)),
            format!("{:.6}", update.object_bonus),
            update.maturity.to_sim_access().to_string(),
            yn(update.restrict_pushobject),
            yn(update.allow_parcel_changes),
        ];
        circuit.send_estate_owner_message("setregioninfo", &params, now)?;
        Ok(())
    }

    /// Requests the estate's covenant summary via `EstateCovenantRequest`. The
    /// reply arrives as [`Event::EstateCovenant`]; fetch the covenant notecard
    /// asset separately with its `covenant_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_estate_covenant(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_covenant_request(now)?;
        Ok(())
    }

    /// Requests the region's telehub configuration via `EstateOwnerMessage`/
    /// `telehub` (`info ui`). The reply arrives as [`Event::TelehubInfo`].
    /// Requires the agent to be the estate owner or a god.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_telehub_info(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("telehub", &["info ui".to_owned()], now)?;
        Ok(())
    }

    /// Connects the given in-region object as the region's telehub via
    /// `EstateOwnerMessage`/`telehub` (`connect`). The updated configuration
    /// arrives as [`Event::TelehubInfo`]. Requires estate-owner or god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn connect_telehub(
        &mut self,
        object_local_id: ScopedObjectId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(object_local_id.circuit)?;
        let object_local_id = object_local_id.id;
        let params = ["connect".to_owned(), object_local_id.to_string()];
        circuit.send_estate_owner_message("telehub", &params, now)?;
        Ok(())
    }

    /// Removes the region's telehub via `EstateOwnerMessage`/`telehub`
    /// (`delete`). The updated configuration arrives as [`Event::TelehubInfo`].
    /// Requires estate-owner or god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn disconnect_telehub(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("telehub", &["delete".to_owned()], now)?;
        Ok(())
    }

    /// Adds a telehub spawn point at the given in-region object's position via
    /// `EstateOwnerMessage`/`telehub` (`spawnpoint add`). The updated
    /// configuration arrives as [`Event::TelehubInfo`]. Requires estate-owner or
    /// god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn add_telehub_spawn_point(
        &mut self,
        object_local_id: ScopedObjectId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(object_local_id.circuit)?;
        let object_local_id = object_local_id.id;
        let params = ["spawnpoint add".to_owned(), object_local_id.to_string()];
        circuit.send_estate_owner_message("telehub", &params, now)?;
        Ok(())
    }

    /// Removes a telehub spawn point by index via `EstateOwnerMessage`/`telehub`
    /// (`spawnpoint remove`). The updated configuration arrives as
    /// [`Event::TelehubInfo`]. Requires estate-owner or god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn remove_telehub_spawn_point(
        &mut self,
        spawn_index: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let params = ["spawnpoint remove".to_owned(), spawn_index.to_string()];
        circuit.send_estate_owner_message("telehub", &params, now)?;
        Ok(())
    }

    /// Ejects an agent from the region with god powers via `GodKickUser`. Unlike
    /// [`Session::kick_estate_user`] this needs grid-god rights, not just estate
    /// ownership.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn god_kick_user(
        &mut self,
        target: AgentKey,
        reason: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_god_kick_user(target.uuid(), reason, now)?;
        Ok(())
    }

    /// Sends a `GodlikeMessage` with the given method and string parameters — the
    /// generic god-level admin command channel. Needs grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn send_godlike_message(
        &mut self,
        method: &str,
        params: &[&str],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let params: Vec<String> = params.iter().map(|param| (*param).to_owned()).collect();
        circuit.send_godlike_message(method, &params, now)?;
        Ok(())
    }

    /// Requests world-map blocks for the inclusive grid-coordinate rectangle
    /// `[min_x, max_x] x [min_y, max_y]` (region indices). Each region in range
    /// arrives as an [`Event::MapBlock`], giving its name, coordinates, and
    /// maturity. Coordinates are clamped to the protocol's 16-bit range.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_blocks(
        &mut self,
        min_x: u32,
        max_x: u32,
        min_y: u32,
        max_y: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let clamp = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        circuit.send_map_block_request(
            clamp(min_x),
            clamp(max_x),
            clamp(min_y),
            clamp(max_y),
            now,
        )?;
        Ok(())
    }

    /// Searches the world map for regions whose name matches `name` via
    /// `MapNameRequest`. Each match arrives as an [`Event::MapBlock`] (the same
    /// reply as [`Session::request_map_blocks`]). Useful for resolving a region
    /// name to its handle/coordinates without knowing where it sits on the grid.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_by_name(&mut self, name: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_map_name_request(name, now)?;
        Ok(())
    }

    /// Requests world-map overlay items of the given [`MapItemType`] (avatar
    /// locations, telehubs, land for sale, events) via `MapItemRequest`.
    /// `region_handle` of 0 targets the current region; any other handle targets
    /// that region. The reply arrives as an [`Event::MapItems`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_items(
        &mut self,
        item_type: MapItemType,
        region_handle: RegionHandle,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_map_item_request(item_type.to_u32(), region_handle.0, now)?;
        Ok(())
    }

    /// Requests the world-map image-tile layers via `MapLayerRequest`. The reply
    /// arrives as an [`Event::MapLayers`], giving the textures and the grid
    /// rectangles they cover (complementing the per-region detail from
    /// [`Session::request_map_blocks`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_layer(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_map_layer_request(now)?;
        Ok(())
    }

    /// Files an abuse / bug report over the legacy `UserReport` UDP message.
    /// Fire-and-forget; there is no reply. On Second Life prefer the
    /// `SendUserReport` capability ([`Command::SendAbuseReportViaCaps`](crate::Command::SendAbuseReportViaCaps),
    /// driven by the runtimes); the UDP path is the fallback (and the only path
    /// OpenSim implements).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the report fails to encode.
    pub fn send_abuse_report(&mut self, report: &AbuseReport, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_user_report(report, now)?;
        Ok(())
    }

    /// Emails a snapshot postcard over the `SendPostcard` UDP message (the
    /// referenced snapshot asset must already be uploaded). Fire-and-forget;
    /// there is no reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the postcard fails to encode.
    pub fn send_postcard(&mut self, postcard: &Postcard, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_postcard(postcard, now)?;
        Ok(())
    }

    /// All cached scene objects across the current region *and* every
    /// neighbouring region a child circuit is streaming (from `ObjectUpdate` /
    /// `ObjectUpdateCompressed`, kept current by motion updates). Each
    /// [`Object`] carries its [`region_handle`](Object::region_handle); a sim's
    /// objects are dropped when its circuit goes away.
    pub fn objects(&self) -> impl Iterator<Item = &Object> {
        self.objects.values().flat_map(BTreeMap::values)
    }

    /// All cached scene objects in the region identified by `region_handle`.
    pub fn objects_in_region(&self, region_handle: RegionHandle) -> impl Iterator<Item = &Object> {
        self.objects()
            .filter(move |object| object.region_handle == region_handle)
    }

    /// Looks up a cached scene object by its [`ScopedObjectId`] — the
    /// region-local id paired with the circuit it belongs to. Resolves against
    /// that exact circuit instance (the current region *or* any neighbour a
    /// child circuit is streaming), so an id captured on a now-torn-down circuit
    /// returns `None` rather than aliasing whatever shares its numeric id on the
    /// current circuit. Build one from an [`Object`] via
    /// [`Object::scoped_id`](crate::Object::scoped_id), or from
    /// [`Session::root_circuit_id`] plus a raw id.
    #[must_use]
    pub fn object(&self, id: ScopedObjectId) -> Option<&Object> {
        self.objects.get(&id.circuit)?.get(&id.id)
    }

    /// All cached terrain patches across the current region *and* every
    /// neighbouring region a child circuit is streaming (decoded from
    /// `LayerData`). Includes every layer (LAND/WATER/WIND/CLOUD); filter on
    /// [`TerrainPatch::layer`] for a specific one. A sim's patches are dropped
    /// when its circuit goes away.
    pub fn terrain_patches(&self) -> impl Iterator<Item = &TerrainPatch> {
        self.terrain.values().flat_map(BTreeMap::values)
    }

    /// All cached terrain patches in the region identified by `region_handle`.
    pub fn terrain_patches_in_region(
        &self,
        region_handle: RegionHandle,
    ) -> impl Iterator<Item = &TerrainPatch> {
        self.terrain_patches()
            .filter(move |patch| patch.region_handle == region_handle)
    }

    /// The ground height (metres) at region-local cell (`x`, `y`) in the region
    /// the agent is currently in (the root circuit), from the cached LAND
    /// terrain, or `None` if that patch has not been received. `x`/`y` are
    /// integer metres within the region (`0..region_size`). Standard regions use
    /// 16-metre LAND patches; for variable ("extended") regions use
    /// [`Session::terrain_patches`] and each patch's own [`TerrainPatch::size`].
    #[must_use]
    pub fn terrain_height(&self, x: u32, y: u32) -> Option<f32> {
        let root = self.circuit.as_ref().map(|circuit| circuit.id)?;
        let cache = self.terrain.get(&root)?;
        // LAND patches on a standard region are 16×16; locate the patch by its
        // grid position then the cell within it (16 is a non-zero literal).
        let patch = cache.get(&(TerrainLayerType::Land.code(), x / 16, y / 16))?;
        patch.value(x % 16, y % 16)
    }

    /// Requests the full `ObjectUpdate` for the given region-local ids via
    /// `RequestMultipleObjects` (a "full" cache miss). Useful to (re)fetch
    /// objects seen only as cached/terse stubs, or to repopulate after a gap.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_request_multiple_objects(&local_ids, now)?;
        Ok(())
    }

    /// Requests an object's extended properties by selecting it (`ObjectSelect`).
    /// The simulator replies with `ObjectProperties`, surfaced as
    /// [`Event::ObjectProperties`] (and merged into the cached [`Object`]). Pair
    /// with [`Session::deselect_objects`] to release the selection afterwards.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_object_properties(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_select(&local_ids, now)?;
        Ok(())
    }

    /// Deselects objects previously selected with
    /// [`Session::request_object_properties`] (`ObjectDeselect`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn deselect_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_deselect(&local_ids, now)?;
        Ok(())
    }

    // Object interaction & editing (#17) -----------------------------------
    //
    // An object is named by its [`ScopedObjectId`] (from [`Session::objects`] /
    // an object event), which carries the [`CircuitId`] of the circuit it was
    // learned on. These ops are sent on *that* circuit — the root region or a
    // neighbour's child circuit — so they work on objects across a region
    // border just as in the region the avatar stands in (this is what the
    // viewer does: it grabs/edits on the object's own region). Operations that
    // create rather than name an existing object (e.g. [`Session::rez_object`])
    // act on the current (root) region instead. Each sends a single reliable
    // message. Edit and rez operations require the appropriate object/parcel
    // permissions on the grid; the simulator silently ignores a request the
    // agent is not allowed to make.

    /// Touches (left-clicks) the object `local_id`: sends an `ObjectGrab` and an
    /// immediate `ObjectDeGrab` with no drag in between, which is what triggers
    /// a script's `touch_start`/`touch_end` (and a `CLICK_ACTION_*` such as buy
    /// or pay). For a press-drag-release interaction use [`Session::grab_object`],
    /// [`Session::grab_object_update`], and [`Session::degrab_object`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if a request fails to encode.
    pub fn touch_object(&mut self, local_id: ScopedObjectId, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_grab(local_id, ZERO_VECTOR, now)?;
        circuit.send_object_degrab(local_id, now)?;
        Ok(())
    }

    /// Begins grabbing the object `local_id` (an `ObjectGrab`) with the given
    /// grab offset from the object's centre. Follow with
    /// [`Session::grab_object_update`] to drag and [`Session::degrab_object`] to
    /// release.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn grab_object(
        &mut self,
        local_id: ScopedObjectId,
        grab_offset: Vector,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_grab(local_id, grab_offset, now)?;
        Ok(())
    }

    /// Updates an in-progress grab (an `ObjectGrabUpdate`) as the avatar drags
    /// the object identified by its persistent `object_id` (not its local id) to
    /// `grab_position`. `time_since_last` is milliseconds since the previous
    /// update.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn grab_object_update(
        &mut self,
        object_id: ObjectKey,
        grab_offset_initial: Vector,
        grab_position: Vector,
        time_since_last: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_grab_update(
            object_id,
            grab_offset_initial,
            grab_position,
            time_since_last,
            now,
        )?;
        Ok(())
    }

    /// Releases a grab on the object `local_id` (an `ObjectDeGrab`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn degrab_object(&mut self, local_id: ScopedObjectId, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_degrab(local_id, now)?;
        Ok(())
    }

    /// Rezzes (creates) a new primitive described by `shape` (an `ObjectAdd`);
    /// `group_id` is the group the new object is set to (`None` for none). The
    /// new object arrives as an [`Event::ObjectAdded`]. Build `shape`
    /// from [`PrimShape::cube`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_object(
        &mut self,
        shape: &PrimShape,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_add(shape, group_id, now)?;
        Ok(())
    }

    /// Duplicates the objects `local_ids` (an `ObjectDuplicate`), offsetting the
    /// copies by `offset` metres; `group_id` is the group the copies are set to.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn duplicate_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        offset: Vector,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_duplicate(&local_ids, offset, group_id, now)?;
        Ok(())
    }

    /// Force-deletes the objects `local_ids` (an `ObjectDelete`). This is the
    /// reference viewer's *force-delete* path (its only use of `ObjectDelete`)
    /// and generally needs estate/god powers; many simulators — including stock
    /// OpenSim, which has no `ObjectDelete` handler — ignore it. For an ordinary,
    /// portable delete-to-trash use [`Session::derez_objects`] with
    /// [`DeRezDestination::Trash`] and the agent's trash folder id (from the
    /// login inventory skeleton). Removed objects arrive as
    /// [`Event::ObjectRemoved`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn delete_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_delete(&local_ids, now)?;
        Ok(())
    }

    /// Derezzes the objects `local_ids` (a `DeRezObject`) to `destination` (take
    /// to inventory, return, trash, …). The `destination` carries its own target
    /// folder, item, or task id where one applies; `transaction_id` is a
    /// caller-chosen id correlating any resulting inventory update; `group_id` is
    /// the active group (use [`Uuid::nil`] for none).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn derez_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        destination: DeRezDestination,
        transaction_id: TransactionId,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_derez_object(&local_ids, destination, transaction_id.get(), group_id, now)?;
        Ok(())
    }

    /// Moves, rotates, and/or scales the object `local_id` (a
    /// `MultipleObjectUpdate`) according to `transform`. Only the components set
    /// in `transform` are changed. The resulting motion arrives as an
    /// [`Event::ObjectUpdated`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_object(
        &mut self,
        local_id: ScopedObjectId,
        transform: &ObjectTransform,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_multiple_object_update(local_id, transform, now)?;
        Ok(())
    }

    /// Moves the object `local_id` to the region-local `position`. A convenience
    /// wrapper over [`Session::update_object`]; `group` moves the whole linkset.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_position(
        &mut self,
        local_id: ScopedObjectId,
        position: Vector,
        group: bool,
        now: Instant,
    ) -> Result<(), Error> {
        self.update_object(
            local_id,
            &ObjectTransform {
                position: Some(position),
                group,
                ..ObjectTransform::default()
            },
            now,
        )
    }

    /// Rotates the object `local_id` to `rotation`. A convenience wrapper over
    /// [`Session::update_object`]; `group` rotates the whole linkset.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_rotation(
        &mut self,
        local_id: ScopedObjectId,
        rotation: Rotation,
        group: bool,
        now: Instant,
    ) -> Result<(), Error> {
        self.update_object(
            local_id,
            &ObjectTransform {
                rotation: Some(rotation),
                group,
                ..ObjectTransform::default()
            },
            now,
        )
    }

    /// Resizes the object `local_id` to `scale` metres. A convenience wrapper
    /// over [`Session::update_object`]; `group` scales the whole linkset and
    /// `uniform` scales proportionally about the centre.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_scale(
        &mut self,
        local_id: ScopedObjectId,
        scale: Vector,
        group: bool,
        uniform: bool,
        now: Instant,
    ) -> Result<(), Error> {
        self.update_object(
            local_id,
            &ObjectTransform {
                scale: Some(scale),
                group,
                uniform,
                ..ObjectTransform::default()
            },
            now,
        )
    }

    /// Renames the object `local_id` (an `ObjectName`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_name(
        &mut self,
        local_id: ScopedObjectId,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_name(local_id, name, now)?;
        Ok(())
    }

    /// Re-describes the object `local_id` (an `ObjectDescription`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_description(
        &mut self,
        local_id: ScopedObjectId,
        description: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_description(local_id, description, now)?;
        Ok(())
    }

    /// Sets the left-click behaviour of the object `local_id` (an
    /// `ObjectClickAction`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_click_action(
        &mut self,
        local_id: ScopedObjectId,
        action: ClickAction,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_click_action(local_id, action, now)?;
        Ok(())
    }

    /// Sets the physical material of the object `local_id` (an `ObjectMaterial`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_material(
        &mut self,
        local_id: ScopedObjectId,
        material: Material,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_material(local_id, material, now)?;
        Ok(())
    }

    /// Sets the physics/temporary/phantom flags of the object `local_id` (an
    /// `ObjectFlagUpdate`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_flags(
        &mut self,
        local_id: ScopedObjectId,
        flags: &ObjectFlagSettings,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_flag_update(local_id, flags, now)?;
        Ok(())
    }

    /// Sets the path/profile geometry of the object `local_id` (an `ObjectShape`).
    /// The `shape` fields are the quantized wire values (see [`PrimShapeParams`]);
    /// read an object's current geometry from
    /// [`Object::shape`](crate::Object::shape).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_shape(
        &mut self,
        local_id: ScopedObjectId,
        shape: &PrimShapeParams,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_shape(local_id, shape, now)?;
        Ok(())
    }

    /// Sets the per-face textures of the object `local_id` (an `ObjectImage`).
    /// `texture_entry` is the new [`TextureEntry`] — build one with a single
    /// [`TextureFace`](crate::TextureFace) to retexture every face uniformly, or
    /// one face per prim face to set them individually. `media_url` is the legacy
    /// parcel-media URL ([`None`] for none).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_image(
        &mut self,
        local_id: ScopedObjectId,
        media_url: Option<&str>,
        texture_entry: &TextureEntry,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_image(local_id, media_url.unwrap_or(""), texture_entry, now)?;
        Ok(())
    }

    /// Sets the complete extra-parameter state of the object `local_id` (an
    /// `ObjectExtraParams`): flexi/light/sculpt/mesh/light-image/render-material/
    /// reflection-probe. Every known subtype is sent, in-use when `params` carries
    /// it — so a subtype left [`None`] (or, for render materials, empty) in
    /// `params` is *cleared* on the object. Passing
    /// [`ObjectExtraParams::default`](crate::ObjectExtraParams) clears them all.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_extra_params(
        &mut self,
        local_id: ScopedObjectId,
        params: &ObjectExtraParams,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_extra_params(local_id, params, now)?;
        Ok(())
    }

    /// Sets the group the objects `local_ids` are set to (an `ObjectGroup`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_group(
        &mut self,
        local_ids: &[ScopedObjectId],
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_group(&local_ids, group_id, now)?;
        Ok(())
    }

    /// Sets or clears the `mask` permission bits of the `field` mask on the
    /// objects `local_ids` (an `ObjectPermissions`). The `mask` is a typed
    /// [`Permissions`] set (e.g. [`Permissions::COPY`]` | `[`Permissions::MODIFY`]);
    /// `set` adds those bits when true and removes them when false.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_permissions(
        &mut self,
        local_ids: &[ScopedObjectId],
        field: PermissionField,
        set: bool,
        mask: Permissions,
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_permissions(&local_ids, field, set, mask, now)?;
        Ok(())
    }

    /// Sets the sale type and price of the object `local_id` (an
    /// `ObjectSaleInfo`). A price of 0 with [`SaleType::NotForSale`] takes it off
    /// sale.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_for_sale(
        &mut self,
        local_id: ScopedObjectId,
        sale_type: SaleType,
        sale_price: Option<LindenAmount>,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_sale_info(local_id, sale_type, sale_price, now)?;
        Ok(())
    }

    /// Sets the search/category code of the object `local_id` (an
    /// `ObjectCategory`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_category(
        &mut self,
        local_id: ScopedObjectId,
        category: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_category(local_id, category, now)?;
        Ok(())
    }

    /// Toggles whether the object `local_id` is listed in search (an
    /// `ObjectIncludeInSearch`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_include_in_search(
        &mut self,
        local_id: ScopedObjectId,
        include: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(local_id.circuit)?;
        let local_id = local_id.id;
        circuit.send_object_include_in_search(local_id, include, now)?;
        Ok(())
    }

    /// Links the objects `local_ids` into one linkset (an `ObjectLink`). The
    /// first id becomes the root prim; the rest become its children.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn link_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_link(&local_ids, now)?;
        Ok(())
    }

    /// Unlinks the objects `local_ids` from their linksets (an `ObjectDelink`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn delink_objects(
        &mut self,
        local_ids: &[ScopedObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        let Some((scope, local_ids)) = split_scoped_object_ids(local_ids)? else {
            return Ok(());
        };
        let circuit = self.circuit_for_scope(scope)?;
        circuit.send_object_delink(&local_ids, now)?;
        Ok(())
    }

    /// Requests an in-world teleport to `position` (region-local) in the region
    /// identified by `region_handle`, looking towards `look_at`. On success the
    /// session re-establishes its circuit at the destination simulator and emits
    /// [`Event::RegionChanged`]; on failure it emits [`Event::TeleportFailed`]
    /// and stays connected to the current region.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotActive`] if the session is not in the active state,
    /// [`Error::NoCircuit`] if no circuit is established, or [`Error::Wire`] if
    /// the request fails to encode.
    pub fn teleport_to(
        &mut self,
        region_handle: RegionHandle,
        position: RegionCoordinates,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Active) {
            return Err(Error::NotActive);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // The wire `TeleportLocationRequest` carries a plain vector; unwrap the
        // typed region-local coordinates at the codec boundary.
        let position = Vector {
            x: position.x(),
            y: position.y(),
            z: position.z(),
        };
        circuit.send_teleport_location_request(region_handle.0, position, look_at, now)?;
        circuit.timers.teleport = Some(deadline(now, TELEPORT_TIMEOUT));
        self.teleport = TeleportPhase::Requested {
            target: region_handle,
        };
        self.state = SessionState::Teleporting;
        Ok(())
    }

    /// Requests a teleport to a landmark (`TeleportLandmarkRequest`). `landmark`
    /// is the landmark inventory item's *asset* id, or `None` to teleport to the
    /// agent's home location (the wire `LandmarkID` is then nil). On success the
    /// session re-establishes its circuit at the destination simulator and emits
    /// [`Event::RegionChanged`]; on failure it emits [`Event::TeleportFailed`].
    /// The destination region handle is unknown until the `TeleportFinish`
    /// arrives, so the in-flight teleport phase carries no target hint.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotActive`] if the session is not in the active state,
    /// [`Error::NoCircuit`] if no circuit is established, or [`Error::Wire`] if
    /// the request fails to encode.
    pub fn teleport_via_landmark(
        &mut self,
        landmark: Option<AssetKey>,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Active) {
            return Err(Error::NotActive);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_teleport_landmark_request(landmark, now)?;
        circuit.timers.teleport = Some(deadline(now, TELEPORT_TIMEOUT));
        // A landmark teleport's destination is resolved sim-side; the
        // authoritative handle arrives with the TeleportFinish.
        self.teleport = TeleportPhase::Requested {
            target: RegionHandle(0),
        };
        self.state = SessionState::Teleporting;
        Ok(())
    }

    /// Cancels an in-progress teleport (`TeleportCancel`). If the session is
    /// currently teleporting it returns to the active state and disarms the
    /// teleport timeout; if no teleport is in flight the message is still sent
    /// but has no local effect.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn cancel_teleport(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_teleport_cancel(now)?;
        if matches!(self.state, SessionState::Teleporting) {
            circuit.timers.teleport = None;
            self.teleport = TeleportPhase::Idle;
            self.state = SessionState::Active;
        }
        Ok(())
    }

    /// Records a start location (`SetStartLocationRequest`): stores `position`
    /// and `look_at` (region-local) as the named [`StartLocationSlot`]. The
    /// everyday use is [`StartLocationSlot::Home`] ("set home to here"). The
    /// simulator fills in the region name, so none is taken here.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_start_location(
        &mut self,
        slot: StartLocationSlot,
        position: RegionCoordinates,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // The wire blocks carry plain vectors; unwrap the typed region-local
        // coordinates at the codec boundary.
        let position = Vector {
            x: position.x(),
            y: position.y(),
            z: position.z(),
        };
        circuit.send_set_start_location_request(slot, position, look_at, now)?;
        Ok(())
    }

    /// Polls for a fresh `AgentDataUpdate` (`AgentDataUpdateRequest`) without
    /// changing any agent data. The reply arrives as the already-handled
    /// active-group update.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_agent_data_update(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_data_update_request(now)?;
        Ok(())
    }

    /// Quits the session leaving the agent's in-world objects behind
    /// (`AgentQuitCopy`) — the "crash quit" the reference viewer sends so a
    /// subsequent login can recover rezzed objects. Sends the message only; the
    /// caller still drives the local shutdown (e.g. [`Session::initiate_logout`]
    /// is the clean alternative).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn quit_copy(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_quit_copy(now)?;
        Ok(())
    }

    /// Toggles simulator-side velocity interpolation of object motion
    /// (`VelocityInterpolateOn` / `VelocityInterpolateOff`): when enabled the
    /// simulator smooths object positions between updates from their velocities.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_velocity_interpolation(&mut self, enabled: bool, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        if enabled {
            circuit.send_velocity_interpolate_on(now)?;
        } else {
            circuit.send_velocity_interpolate_off(now)?;
        }
        Ok(())
    }

    /// Requests the agent's own account contact preferences
    /// (`UserInfoRequest`). The reply arrives as [`Event::UserInfo`] carrying the
    /// IM-via-email flag, directory visibility, and the email address on file.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_user_info(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_user_info_request(now)?;
        Ok(())
    }

    /// Updates the agent's account contact preferences (`UpdateUserInfo`):
    /// whether offline instant messages are forwarded to email (`im_via_email`)
    /// and the directory/search visibility (`directory_visibility`). Mirrors the
    /// writable fields of [`Event::UserInfo`]; the email address itself is not
    /// settable over UDP (the wire message carries no email field), so it is
    /// left unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_user_info(
        &mut self,
        im_via_email: bool,
        directory_visibility: DirectoryVisibility,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_user_info(im_via_email, directory_visibility.to_wire(), now)?;
        Ok(())
    }

    /// Triggers a one-shot spatial sound (`SoundTrigger`): plays `sound` at
    /// `position` (region-local to `region_handle`) with linear `gain`
    /// (`0.0`..=`1.0`). This is the viewer→sim counterpart of the inbound
    /// [`Event::SoundTrigger`]; the simulator fills in the owner/object ids, so
    /// only the asset, gain, and location are supplied. Sent unreliably, as the
    /// reference viewer does.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn trigger_sound(
        &mut self,
        sound: AssetKey,
        gain: f32,
        region_handle: RegionHandle,
        position: RegionCoordinates,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // The wire block carries a plain vector; unwrap the typed region-local
        // coordinates at the codec boundary.
        let position = Vector {
            x: position.x(),
            y: position.y(),
            z: position.z(),
        };
        circuit.send_sound_trigger(sound, gain, region_handle.0, position, now)?;
        Ok(())
    }

    /// Requests that the simulator grant (`godlike = true`) or drop (`false`)
    /// god powers for this agent via `RequestGodlikePowers`. The grant arrives
    /// as [`Event::GodlikePowersGranted`]. The agent must actually hold god
    /// rights on the grid for the request to succeed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_godlike_powers(&mut self, godlike: bool, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_request_godlike_powers(godlike, now)?;
        Ok(())
    }

    /// Ejects `target` from the agent's land via `EjectUser`, optionally also
    /// banning them from the parcel (`action`). The agent must own or manage
    /// the land the target is standing on.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn eject_user(
        &mut self,
        target: AgentKey,
        action: EjectAction,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_eject_user(target.uuid(), action.to_wire(), now)?;
        Ok(())
    }

    /// Freezes or unfreezes `target` on the agent's land via `FreezeUser`
    /// (`action`). A frozen avatar cannot move or act until unfrozen (or until
    /// the freeze times out). The agent must own or manage the land.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn freeze_user(
        &mut self,
        target: AgentKey,
        action: FreezeAction,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_freeze_user(target.uuid(), action.to_wire(), now)?;
        Ok(())
    }

    /// Deletes (or returns) the objects `owner` has across the whole region via
    /// `SimWideDeletes`, filtered by `flags`. Needs estate-manager or god
    /// rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn sim_wide_deletes(
        &mut self,
        owner: AgentKey,
        flags: SimWideDeleteFlags,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_sim_wide_deletes(owner.uuid(), flags.to_wire(), now)?;
        Ok(())
    }

    /// Pushes god-tools region parameters via `GodUpdateRegionInfo` (`update`):
    /// the region name, estate ids, region flags, billing factor, land price,
    /// and teleport-redirect grid. The simulator overwrites these wholesale.
    /// Needs grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn god_update_region_info(
        &mut self,
        update: &GodRegionUpdate,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_god_update_region_info(update, now)?;
        Ok(())
    }

    /// Force-reassigns the ownership of the parcel `parcel` to `owner` via
    /// `ParcelGodForceOwner`. Needs grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `parcel`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn parcel_god_force_owner(
        &mut self,
        parcel: ScopedParcelId,
        owner: OwnerKey,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(parcel.circuit)?;
        circuit.send_parcel_god_force_owner(parcel.id, owner.uuid(), now)?;
        Ok(())
    }

    /// Marks the parcel `parcel` (and the content on it) as owned by the
    /// governor/maintenance account via `ParcelGodMarkAsContent`. Needs
    /// grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `parcel`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn parcel_god_mark_as_content(
        &mut self,
        parcel: ScopedParcelId,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(parcel.circuit)?;
        circuit.send_parcel_god_mark_as_content(parcel.id, now)?;
        Ok(())
    }

    /// Deletes the events-directory listing `event` and re-runs the search via
    /// `EventGodDelete`. The `query_id` / `query_text` / `flags` / `query_start`
    /// arguments carry the current events search so the simulator can return the
    /// refreshed result page (correlated by `query_id`, exactly as
    /// [`dir_find_query`](Self::dir_find_query) does). Needs grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn event_god_delete(
        &mut self,
        event: EventId,
        query_id: QueryId,
        query_text: &str,
        flags: DirFindFlags,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_event_god_delete(
            event.get(),
            query_id.get(),
            query_text,
            flags,
            query_start,
            now,
        )?;
        Ok(())
    }

    /// Saves the region (world) state to `filename` via `StateSave`. An empty
    /// `filename` lets the simulator pick the autosave name, exactly as the
    /// reference viewer does. Needs grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn state_save(&mut self, filename: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_state_save(filename, now)?;
        Ok(())
    }

    /// Starts a land auction on the parcel `parcel` via `ViewerStartAuction`,
    /// optionally advertised by the `snapshot` texture (`None` for none). Needs
    /// grid-god rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownCircuit`] if `parcel`'s circuit has gone away, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn viewer_start_auction(
        &mut self,
        parcel: ScopedParcelId,
        snapshot: Option<TextureKey>,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit_for_scope(parcel.circuit)?;
        circuit.send_viewer_start_auction(parcel.id, snapshot, now)?;
        Ok(())
    }

    /// Begins a clean logout: queues a `LogoutRequest` and arms the logout
    /// timeout. Does nothing if the session is already closing or closed.
    pub fn initiate_logout(&mut self, now: Instant) {
        if matches!(self.state, SessionState::Closed | SessionState::LoggingOut) {
            return;
        }
        match self.circuit.as_mut() {
            Some(circuit) => {
                if circuit.send_logout_request(now).is_err() {
                    self.close(DisconnectReason::ProtocolError);
                    return;
                }
                circuit.timers.logout = Some(deadline(now, LOGOUT_TIMEOUT));
                self.state = SessionState::LoggingOut;
            }
            None => self.close(DisconnectReason::ProtocolError),
        }
    }

    /// The next datagram to transmit, if any: the root circuit's queue first,
    /// then each child circuit's, so the driver can multiplex all circuits onto
    /// one socket using [`Transmit::destination`].
    pub fn poll_transmit(&mut self) -> Option<Transmit> {
        if let Some(circuit) = self.circuit.as_mut()
            && let Some(payload) = circuit.out.pop_front()
        {
            return Some(Transmit {
                destination: circuit.sim_addr,
                payload,
            });
        }
        for circuit in self.children.values_mut() {
            if let Some(payload) = circuit.out.pop_front() {
                return Some(Transmit {
                    destination: circuit.sim_addr,
                    payload,
                });
            }
        }
        None
    }

    /// The earliest instant at which [`Self::handle_timeout`] should next run.
    #[must_use]
    pub fn poll_timeout(&self) -> Option<Instant> {
        if matches!(self.state, SessionState::Closed) {
            return None;
        }
        let circuit = self.circuit.as_ref()?;
        let mut earliest = Some(circuit.timers.inactivity);
        merge_deadline(&mut earliest, circuit.timers.ack_flush);
        merge_deadline(&mut earliest, circuit.timers.agent_update);
        merge_deadline(&mut earliest, circuit.timers.logout);
        merge_deadline(&mut earliest, circuit.timers.teleport);
        merge_deadline(&mut earliest, circuit.timers.sit);
        merge_deadline(&mut earliest, circuit.next_resend_deadline());
        for child in self.children.values() {
            merge_deadline(&mut earliest, Some(child.timers.inactivity));
            merge_deadline(&mut earliest, child.timers.ack_flush);
            merge_deadline(&mut earliest, child.next_resend_deadline());
        }
        earliest
    }

    /// The next high-level event, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Returns `true` once the session has reached its terminal state.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self.state, SessionState::Closed)
    }

    /// Transitions to the closed state, emitting a disconnect event once.
    fn close(&mut self, reason: DisconnectReason) {
        if !matches!(self.state, SessionState::Closed) {
            self.state = SessionState::Closed;
            self.events.push_back(Event::Disconnected(reason));
        }
    }
}
