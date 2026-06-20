//! The driver-facing `Session` API: login, UDP/CAPS dispatch, and command methods.

use super::conversions::{
    OutgoingIm, ZERO_VECTOR, active_group, ais_inventory_update_from_llsd, avatar_animations,
    avatar_appearance, avatar_group, avatar_interests, avatar_names, avatar_properties,
    bulk_update_folder, bulk_update_inventory_from_llsd, bulk_update_item, chat_message,
    chatterbox_invitation_from_llsd, classified_info, created_category_from_llsd,
    crossed_region_from_caps_llsd, economy_data, enable_simulator_from_caps_llsd,
    environment_from_llsd, establish_agent_communication_from_llsd, estate_access_from_params,
    estate_info_from_params, friend, group_member, group_members_from_caps_llsd, group_membership,
    group_memberships_from_caps_llsd, group_names, group_notice, group_profile, group_role,
    group_title, index_into, instant_message, inventory_descendents_from_llsd, inventory_folder,
    inventory_item, inventory_item_from_create, inventory_offer_bucket, map_item, map_region_info,
    money_balance, neighbor_info, object_from_full_update, object_properties,
    offline_messages_from_llsd, pack_uuids, parcel_info, parcel_info_from_llsd,
    parse_lure_region_handle, parse_mute_list, parse_uuid_string, pick_info, region_identity,
    region_limits, script_dialog, script_permission_request, server_appearance_update_from_llsd,
    skeleton_folder, teleport_finish_from_llsd, trimmed_string,
};
use super::{
    AGENT_UPDATE_INTERVAL, AssetTransfer, AssetUpload, CAP_AGENT_EXPERIENCES,
    CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES, CAP_EXT_ENVIRONMENT,
    CAP_FETCH_INVENTORY, CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES,
    CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES, CAP_GET_EXPERIENCE_INFO,
    CAP_GET_EXPERIENCES, CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_LIBRARY_API_V3,
    CAP_MODIFY_MATERIAL_PARAMS, CAP_OBJECT_MEDIA, CAP_PARCEL_VOICE_INFO,
    CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES,
    CAP_REMOTE_PARCEL_REQUEST, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, Circuit,
    DEFAULT_DRAW_DISTANCE, HandoverPending, IDENTITY_ROTATION, LOGOUT_TIMEOUT, MAX_INLINE_ASSET,
    SIT_TIMEOUT, Session, SessionState, TELEPORT_FLAGS_VIA_LURE, TELEPORT_TIMEOUT, TextureDownload,
    deadline, merge_deadline,
};
use crate::error::Error;
use crate::terrain;
use crate::types::{
    AlertInfo, Asset, AssetType, AttachmentPoint, AvatarClassified, AvatarPick, AvatarPickerResult,
    Camera, ChatType, ClassifiedUpdate, ClickAction, CoarseLocation, CreateGroupParams,
    DeRezDestination, Diagnostic, DirClassifiedResult, DirEventResult, DirFindFlags,
    DirGroupResult, DirLandResult, DirPeopleResult, DirPlaceResult, DisconnectReason,
    EstateAccessDelta, EstateCovenant, Event, EventInfo, FriendRights, GroupNoticeAttachment,
    GroupRoleEdit, GroupRoleMember, GroupRoleMemberChange, ImDialog, ImageCodec, InterestsUpdate,
    InventoryFolder, InventoryItem, InventoryOffer, LandSearchType, LoadUrlRequest, LoginAccount,
    LoginHttpRequest, LoginParams, MapItemType, Material, Maturity, MoneyTransactionType,
    MuteFlags, MuteType, NeighborInfo, NewInventoryItem, NotecardRez, Object, ObjectBuyItem,
    ObjectFlagSettings, ObjectPropertiesFamily, ObjectTransform, ParcelAccessEntry,
    ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelDetails, ParcelMediaCommand,
    ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo, ParcelReturnType, ParcelUpdate,
    PermissionField, PickUpdate, PlacesResult, PrimShape, ProfileUpdate, RegionInfoUpdate,
    Reliability, RestoreItem, RezAttachment, SaleType, ScriptPermissions, ScriptTeleportRequest,
    SoundFlags, SoundPreload, TelehubInfo, TeleportFlags, TerrainLayerType, TerrainPatch, Texture,
    Throttle, TransferStatus, Transmit, ViewerEffect, ViewerEffectData, ViewerEffectType, Wearable,
    WearableType, global_to_handle, handle_to_grid,
};
use sl_types::lsl::{Rotation, Vector};
use sl_types::money::LindenAmount;
use sl_wire::{
    AnyMessage, ControlFlags, GLTF_MATERIAL_OVERRIDE_METHOD, Llsd, MessageId, ObjectMediaResponse,
    PacketFlags, ParcelVoiceInfo, Reader, VoiceAccountInfo, build_group_notice_bucket,
    build_login_request, message_name, parse_datagram, parse_display_names, parse_experience_ids,
    parse_experience_infos, parse_experience_permissions, parse_gltf_material_override,
    parse_region_experiences, parse_remote_parcel_reply, zero_decode,
};
use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
use uuid::Uuid;

/// The maximum number of ids packed into a single `UUIDNameRequest` /
/// `UUIDGroupNameRequest`. Each id is 16 bytes; 80 keeps the datagram (plus its
/// header and block count) comfortably within a typical UDP MTU.
const UUID_NAMES_PER_REQUEST: usize = 80;

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
            draw_distance: DEFAULT_DRAW_DISTANCE,
            controls: ControlFlags::empty(),
            throttle: None,
            body_rotation: IDENTITY_ROTATION,
            head_rotation: IDENTITY_ROTATION,
            camera: Camera::region_center(),
            sit_requested: false,
            handover: None,
            teleport_target: None,
            seed_capability: None,
            inventory_root: None,
            login_account: None,
            mute_xfers: BTreeMap::new(),
            next_xfer_id: 1,
            texture_downloads: BTreeMap::new(),
            asset_transfers: BTreeMap::new(),
            next_transfer_id: 1,
            secure_session_id: Uuid::nil(),
            asset_uploads: BTreeMap::new(),
            upload_xfers: BTreeMap::new(),
            next_upload_id: 1,
            objects: BTreeMap::new(),
            terrain: BTreeMap::new(),
            regions: BTreeMap::new(),
            time_dilation: BTreeMap::new(),
            inventory_folders: BTreeMap::new(),
            inventory_items: BTreeMap::new(),
            next_inventory_callback: 1,
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
    /// body failed to parse into the expected shape. Logs a warning and queues a
    /// [`Diagnostic::CapsDecodeFailed`].
    fn caps_decode_failed(&mut self, message: &str) {
        tracing::warn!(event = message, "CAPS event body failed to parse");
        self.push_diagnostic(Diagnostic::CapsDecodeFailed {
            message: message.to_owned(),
        });
    }

    /// Sets the draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    /// A larger value makes the simulator enable more neighbouring regions
    /// (surfaced as [`Event::NeighborDiscovered`]). Takes effect on the next
    /// keep-alive, including for the current circuit.
    pub const fn set_draw_distance(&mut self, draw_distance: f32) {
        self.draw_distance = draw_distance;
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.draw_distance = draw_distance;
        }
    }

    /// The current region's capability-seed URL, once login (or a teleport) has
    /// provided one. The driver POSTs this to obtain the capability map and the
    /// `EventQueueGet` URL. It changes on each region change.
    #[must_use]
    pub fn seed_capability(&self) -> Option<&str> {
        self.seed_capability.as_deref()
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
            "TeleportFinish" => {
                if let Some(finish) = teleport_finish_from_llsd(body) {
                    let region_handle = self.teleport_target.unwrap_or(0);
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
                    self.open_child_circuit(sim, now)?;
                    self.regions.insert(sim, handle);
                    let (grid_x, grid_y) = handle_to_grid(handle);
                    self.events
                        .push_back(Event::NeighborDiscovered(NeighborInfo {
                            region_handle: handle,
                            sim,
                            grid_x,
                            grid_y,
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
                    self.promote_child_to_root(dest, handle, Some(seed), now)?;
                } else {
                    self.caps_decode_failed(message);
                }
            }
            CAP_FETCH_INVENTORY => {
                for event in inventory_descendents_from_llsd(body) {
                    if let Event::InventoryDescendents { folders, items, .. } = &event {
                        self.cache_inventory(folders, items);
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
                    self.cache_inventory(&folders, &items);
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
                let (folders, items) = ais_inventory_update_from_llsd(body);
                if !folders.is_empty() || !items.is_empty() {
                    self.cache_inventory(&folders, &items);
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
            CAP_OBJECT_MEDIA => {
                if let Some(response) = ObjectMediaResponse::from_llsd(body) {
                    self.events.push_back(Event::ObjectMedia {
                        object_id: response.object_id,
                        version: response.version,
                        faces: response.faces,
                    });
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The reply to a `ModifyMaterialParams` POST (setting a GLTF material
            // on object faces): a `{ success, message }` status map.
            CAP_MODIFY_MATERIAL_PARAMS => {
                let success = body.get("success").and_then(Llsd::as_bool).unwrap_or(false);
                let message = body
                    .get("message")
                    .and_then(Llsd::as_str)
                    .unwrap_or_default()
                    .to_owned();
                self.events
                    .push_back(Event::MaterialParamsResult { success, message });
            }
            // The reply to a `ProvisionVoiceAccountRequest` POST: either Vivox
            // SIP credentials or a WebRTC JSEP answer. Only the signalling is
            // surfaced; opening the audio session is the caller's concern.
            CAP_PROVISION_VOICE_ACCOUNT => {
                self.events
                    .push_back(Event::VoiceAccountProvisioned(VoiceAccountInfo::from_llsd(
                        body,
                    )));
            }
            // The reply to a `ParcelVoiceInfoRequest` POST: the parcel's voice
            // channel URI (absent when the parcel has no voice).
            CAP_PARCEL_VOICE_INFO => {
                if let Some(info) = ParcelVoiceInfo::from_llsd(body) {
                    self.events.push_back(Event::ParcelVoiceInfo(info));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The reply to a `GetDisplayNames` GET: the requested agents' display
            // names (with unresolved ids folded in as `missing` placeholders).
            CAP_GET_DISPLAY_NAMES => {
                self.events
                    .push_back(Event::DisplayNames(parse_display_names(body)));
            }
            // The reply to a `RemoteParcelRequest` POST: the grid-wide parcel id
            // covering the requested region location (feeds a `ParcelInfoRequest`).
            CAP_REMOTE_PARCEL_REQUEST => {
                if let Some(parcel_id) = parse_remote_parcel_reply(body) {
                    self.events.push_back(Event::RemoteParcelId(parcel_id));
                } else {
                    self.caps_decode_failed(message);
                }
            }
            // The reply to a `GetExperienceInfo` GET: the requested experiences'
            // metadata (with unresolved ids folded in as `missing` placeholders).
            CAP_GET_EXPERIENCE_INFO => {
                self.events
                    .push_back(Event::ExperienceInfo(parse_experience_infos(body)));
            }
            // The reply to a `FindExperienceByName` GET: one page of search hits.
            CAP_FIND_EXPERIENCE_BY_NAME => {
                self.events
                    .push_back(Event::ExperienceSearchResults(parse_experience_infos(body)));
            }
            // The reply to a `GetExperiences` GET or an `ExperiencePreferences`
            // PUT/DELETE: the agent's allowed/blocked experiences.
            CAP_GET_EXPERIENCES | CAP_EXPERIENCE_PREFERENCES => {
                let (allowed, blocked) = parse_experience_permissions(body);
                self.events
                    .push_back(Event::ExperiencePermissions { allowed, blocked });
            }
            // The reply to an `AgentExperiences` GET: experiences the agent owns.
            CAP_AGENT_EXPERIENCES => {
                self.events
                    .push_back(Event::OwnedExperiences(parse_experience_ids(body)));
            }
            // The reply to a `GetAdminExperiences` GET: experiences the agent
            // administers.
            CAP_GET_ADMIN_EXPERIENCES => {
                self.events
                    .push_back(Event::AdminExperiences(parse_experience_ids(body)));
            }
            // The reply to a `GetCreatorExperiences` GET: experiences the agent
            // created.
            CAP_GET_CREATOR_EXPERIENCES => {
                self.events
                    .push_back(Event::CreatorExperiences(parse_experience_ids(body)));
            }
            // The reply to an `UpdateExperience` POST: the experience's metadata
            // after the edit.
            CAP_UPDATE_EXPERIENCE => {
                self.events.push_back(Event::ExperienceUpdated(
                    parse_experience_infos(body)
                        .into_iter()
                        .next()
                        .unwrap_or_default(),
                ));
            }
            // The reply to a `RegionExperiences` GET or POST: the region's
            // allow/block/trust lists.
            CAP_REGION_EXPERIENCES => {
                let (allowed, blocked, trusted) = parse_region_experiences(body);
                self.events.push_back(Event::RegionExperiences {
                    allowed,
                    blocked,
                    trusted,
                });
            }
            // The reply to a `ReadOfflineMsgs` GET (the modern Second Life
            // offline-IM history, #28): an array of stored instant messages, each
            // surfaced as an offline [`Event::InstantMessageReceived`] (the legacy
            // UDP `RetrieveInstantMessages` path re-delivers them as UDP IMs
            // instead).
            CAP_READ_OFFLINE_MSGS => {
                for im in offline_messages_from_llsd(body) {
                    self.events
                        .push_back(Event::InstantMessageReceived(Box::new(im)));
                }
            }
            // A conference / group IM-session invitation delivered over the CAPS
            // event queue (the modern path, #28). Join by sending into the session
            // with [`Session::send_conference_message`].
            "ChatterBoxInvitation" => {
                if let Some(event) = chatterbox_invitation_from_llsd(body) {
                    self.events.push_back(event);
                } else {
                    self.caps_decode_failed(message);
                }
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
        region_handle: u64,
        seed_capability: Option<String>,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Teleporting) {
            return Ok(());
        }
        // Retarget synchronously: it resets the circuit's sequence/ack/seen/timer
        // state to the new simulator, after which the source check accepts only
        // the destination.
        if let Some(circuit) = self.circuit.as_mut() {
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
        self.teleport_target = None;
        self.handover = Some(HandoverPending { region_handle });
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
            // Re-advertise the bandwidth throttle on the new root circuit: each
            // region starts with the simulator's conservative defaults until the
            // client tells it otherwise. Best-effort — a wire-encode failure here
            // must not abort arrival.
            if let Some(throttle) = self.throttle {
                let _ignored = circuit.send_agent_throttle(&throttle, now);
            }
        }
        self.state = SessionState::Active;
        match self.handover.take() {
            Some(handover) => {
                if let Some(sim) = self.circuit.as_ref().map(|c| c.sim_addr) {
                    self.events.push_back(Event::RegionChanged {
                        region_handle: handover.region_handle,
                        sim,
                    });
                }
            }
            None => self.events.push_back(Event::RegionHandshakeComplete),
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
        let mut child = Circuit::new(
            sim,
            root.agent_id,
            root.session_id,
            root.code,
            self.draw_distance,
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
        region_handle: u64,
        seed: Option<String>,
        now: Instant,
    ) -> Result<(), Error> {
        let Some(root) = self.circuit.as_ref() else {
            return Ok(());
        };
        let (agent_id, session_id, code) = (root.agent_id, root.session_id, root.code);
        // Prefer the seed from `CrossedRegion`; fall back to the one cached from
        // the child's `EstablishAgentCommunication`.
        let seed = seed
            .filter(|s| !s.is_empty())
            .or_else(|| self.child_seeds.get(&dest).cloned());
        let mut new_root = self.children.remove(&dest).unwrap_or_else(|| {
            Circuit::new(dest, agent_id, session_id, code, self.draw_distance, now)
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
        self.handover = Some(HandoverPending { region_handle });
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
    /// Returns [`Error::Wire`] if a bootstrap packet fails to encode.
    pub fn handle_login_response(
        &mut self,
        response: sl_wire::LoginResponse,
        now: Instant,
    ) -> Result<(), Error> {
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
                let mut circuit = Circuit::new(
                    sim_addr,
                    success.agent_id,
                    success.session_id,
                    success.circuit_code,
                    self.draw_distance,
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
                        .insert(sim_addr, global_to_handle(region_x, region_y));
                }
                self.seed_capability = Some(success.seed_capability.clone());
                self.inventory_root = success.inventory_root;
                self.secure_session_id = success.secure_session_id;
                self.state = SessionState::AwaitingHandshake;
                self.events
                    .push_back(Event::CircuitEstablished { sim: sim_addr });
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
                    self.events.push_back(Event::LibraryInventory(library));
                }
                if !success.inventory_skeleton.is_empty() {
                    let folders: Vec<InventoryFolder> = success
                        .inventory_skeleton
                        .iter()
                        .map(skeleton_folder)
                        .collect();
                    // Seed the live inventory cache with the skeleton.
                    for folder in &folders {
                        self.inventory_folders
                            .insert(folder.folder_id, folder.clone());
                    }
                    self.events.push_back(Event::InventorySkeleton(folders));
                }
                if !success.buddy_list.is_empty() {
                    let friends = success.buddy_list.iter().map(friend).collect();
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
        if self.try_dispatch_object(from, message, now) {
            return Ok(());
        }
        match message {
            AnyMessage::StartPingCheck(ping) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    circuit.send_complete_ping_check(ping.ping_id.ping_id, now)?;
                }
            }
            AnyMessage::RegionHandshake(_) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    circuit.send_region_handshake_reply(now)?;
                }
            }
            AnyMessage::PacketAck(ack) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    for packet in &ack.packets {
                        circuit.record_acks(&[packet.id]);
                    }
                }
            }
            AnyMessage::DisableSimulator(_) => {
                // The simulator is retiring this child circuit.
                self.children.remove(&from);
                self.child_seeds.remove(&from);
                self.forget_sim_objects(from);
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
    ) -> bool {
        match message {
            AnyMessage::ObjectUpdate(update) => {
                let region_handle = update.region_data.region_handle;
                self.note_time_dilation(from, region_handle, update.region_data.time_dilation);
                for block in &update.object_data {
                    self.upsert_object(from, object_from_full_update(block, region_handle));
                }
            }
            AnyMessage::ObjectUpdateCompressed(update) => {
                let region_handle = update.region_data.region_handle;
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
                    update.region_data.region_handle,
                    update.region_data.time_dilation,
                );
                // We keep no persistent object cache across sessions, so any entry
                // not already held with a matching CRC is a miss; fetch the full
                // update for the misses (a full `ObjectUpdate` follows).
                let cached = self.objects.get(&from);
                let misses: Vec<u32> = update
                    .object_data
                    .iter()
                    .filter(|block| {
                        cached
                            .and_then(|sim| sim.get(&block.id))
                            .is_none_or(|object| object.crc != block.crc)
                    })
                    .map(|block| block.id)
                    .collect();
                self.request_object_ids(from, &misses, now);
            }
            AnyMessage::ImprovedTerseObjectUpdate(update) => {
                self.note_time_dilation(
                    from,
                    update.region_data.region_handle,
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
                for block in &kill.object_data {
                    let region_handle = self
                        .objects
                        .get_mut(&from)
                        .and_then(|sim| sim.remove(&block.id))
                        .map_or(0, |object| object.region_handle);
                    self.events.push_back(Event::ObjectRemoved {
                        region_handle,
                        local_id: block.id,
                    });
                }
            }
            AnyMessage::ObjectProperties(props) => {
                for block in &props.object_data {
                    let properties = object_properties(block);
                    if let Some(object) = self.objects.get_mut(&from).and_then(|sim| {
                        sim.values_mut()
                            .find(|object| object.full_id == properties.object_id)
                    }) {
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
                    let region_handle = self.regions.get(&from).copied().unwrap_or(0);
                    self.events.push_back(Event::GltfMaterialOverride {
                        region_handle,
                        local_id: decoded.local_id,
                        faces: decoded.faces,
                        overrides: decoded.overrides,
                    });
                }
            }
            _ => return false,
        }
        true
    }

    /// Decodes a `LayerData` payload received from simulator `from`, caching each
    /// patch (keyed by layer and grid position) and emitting an
    /// [`Event::TerrainPatch`]. Best-effort: a malformed group header is ignored.
    fn dispatch_terrain(&mut self, from: SocketAddr, data: &[u8]) {
        let Some((layer, patches)) = terrain::decode_layer(data) else {
            return;
        };
        let region_handle = self.regions.get(&from).copied().unwrap_or(0);
        let cache = self.terrain.entry(from).or_default();
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
    fn note_time_dilation(&mut self, from: SocketAddr, region_handle: u64, raw: u16) {
        if self.time_dilation.insert(from, raw) == Some(raw) {
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
        // Remember this sim's region handle so terrain patches (whose `LayerData`
        // message carries no handle) can be labelled with it.
        if object.region_handle != 0 {
            self.regions.insert(from, object.region_handle);
        }
        let sim = self.objects.entry(from).or_default();
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
        let Some(object) = self
            .objects
            .get_mut(&from)
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
    fn request_object_ids(&mut self, from: SocketAddr, local_ids: &[u32], now: Instant) {
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

    /// Drops every cached object for simulator `addr` (its circuit has gone away),
    /// emitting an [`Event::ObjectRemoved`] for each so consumers can prune.
    fn forget_sim_objects(&mut self, addr: SocketAddr) {
        // The terrain, region-handle, and time-dilation caches for this sim go
        // stale too.
        self.terrain.remove(&addr);
        self.regions.remove(&addr);
        self.time_dilation.remove(&addr);
        let Some(sim) = self.objects.remove(&addr) else {
            return;
        };
        for object in sim.into_values() {
            self.events.push_back(Event::ObjectRemoved {
                region_handle: object.region_handle,
                local_id: object.local_id,
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
        if self.try_dispatch_object(from, message, now) {
            return Ok(());
        }
        match message {
            AnyMessage::RegionHandshake(handshake) => {
                if matches!(self.state, SessionState::AwaitingHandshake) {
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_region_handshake_reply(now)?;
                    }
                    let region_handle = self.regions.get(&from).copied().unwrap_or(0);
                    self.events
                        .push_back(Event::RegionInfoHandshake(Box::new(region_identity(
                            handshake,
                            region_handle,
                        ))));
                    self.complete_arrival(now);
                }
            }
            AnyMessage::AgentMovementComplete(_) => {
                // After a teleport handover the destination promotes us to root
                // and confirms with AgentMovementComplete; it may not re-send a
                // RegionHandshake, so complete the arrival here too (idempotent).
                self.complete_arrival(now);
            }
            AnyMessage::RegionInfo(info) => {
                self.events
                    .push_back(Event::RegionLimits(region_limits(info)));
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
                    .push_back(Event::MoneyBalance(money_balance(reply)));
            }
            AnyMessage::EconomyData(data) => {
                self.events
                    .push_back(Event::EconomyData(Box::new(economy_data(data))));
            }
            AnyMessage::ParcelProperties(props) => {
                self.events
                    .push_back(Event::ParcelProperties(Box::new(parcel_info(props))));
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
                        media_url: trimmed_string(&data.media_url),
                        media_id: data.media_id,
                        media_auto_scale: data.media_auto_scale != 0,
                        media_type: trimmed_string(&extended.media_type),
                        media_desc: trimmed_string(&extended.media_desc),
                        media_width: extended.media_width,
                        media_height: extended.media_height,
                        media_loop: extended.media_loop != 0,
                    }));
            }
            AnyMessage::ParcelDwellReply(reply) => {
                self.events.push_back(Event::ParcelDwell {
                    local_id: reply.data.local_id,
                    parcel_id: reply.data.parcel_id,
                    dwell: reply.data.dwell,
                });
            }
            AnyMessage::ParcelAccessListReply(reply) => {
                self.events.push_back(Event::ParcelAccessList {
                    local_id: reply.data.local_id,
                    scope: ParcelAccessScope::from_u32(reply.data.flags),
                    entries: reply
                        .list
                        .iter()
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
                            owner_id: owner.owner_id,
                            is_group_owned: owner.is_group_owned,
                            count: owner.count,
                            online_status: owner.online_status,
                        })
                        .collect(),
                });
            }
            AnyMessage::ParcelInfoReply(reply) => {
                let data = &reply.data;
                self.events.push_back(Event::ParcelDetails(ParcelDetails {
                    parcel_id: data.parcel_id,
                    owner_id: data.owner_id,
                    name: trimmed_string(&data.name),
                    description: trimmed_string(&data.desc),
                    actual_area: data.actual_area,
                    billable_area: data.billable_area,
                    flags: data.flags,
                    global_x: data.global_x,
                    global_y: data.global_y,
                    global_z: data.global_z,
                    sim_name: trimmed_string(&data.sim_name),
                    snapshot_id: data.snapshot_id,
                    dwell: data.dwell,
                    sale_price: data.sale_price,
                    auction_id: data.auction_id,
                }));
            }
            AnyMessage::EstateOwnerMessage(message) => {
                match trimmed_string(&message.method_data.method).as_str() {
                    "estateupdateinfo" => {
                        if let Some(info) = estate_info_from_params(&message.param_list) {
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
                    covenant_id: data.covenant_id,
                    covenant_timestamp: data.covenant_timestamp,
                    estate_name: trimmed_string(&data.estate_name),
                    estate_owner_id: data.estate_owner_id,
                }));
            }
            AnyMessage::TelehubInfo(info) => {
                let block = &info.telehub_block;
                self.events.push_back(Event::TelehubInfo(TelehubInfo {
                    object_id: block.object_id,
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
                        self.events.push_back(Event::ImTyping {
                            from_agent_id: im.agent_data.agent_id,
                            from_agent_name: trimmed_string(&block.from_agent_name),
                            session_id: block.id,
                            typing: matches!(dialog, ImDialog::TypingStart),
                        });
                    }
                    // Group IM session traffic (the session id is the group id).
                    ImDialog::SessionSend if block.from_group => {
                        self.events.push_back(Event::GroupSessionMessage {
                            group_id: block.id,
                            from_agent_id: im.agent_data.agent_id,
                            from_name: trimmed_string(&block.from_agent_name),
                            message: trimmed_string(&block.message),
                        });
                    }
                    dialog @ (ImDialog::SessionAdd | ImDialog::SessionLeave)
                        if block.from_group =>
                    {
                        self.events.push_back(Event::GroupSessionParticipant {
                            group_id: block.id,
                            agent_id: im.agent_data.agent_id,
                            joined: matches!(dialog, ImDialog::SessionAdd),
                        });
                    }
                    // Ad-hoc conference session traffic mirrors the group-session
                    // arms above but with `from_group` clear (#28); the session id
                    // is the conference id, not a group id.
                    ImDialog::SessionSend => {
                        self.events.push_back(Event::ConferenceSessionMessage {
                            session_id: block.id,
                            from_agent_id: im.agent_data.agent_id,
                            from_name: trimmed_string(&block.from_agent_name),
                            message: trimmed_string(&block.message),
                        });
                    }
                    dialog @ (ImDialog::SessionAdd | ImDialog::SessionLeave) => {
                        self.events.push_back(Event::ConferenceSessionParticipant {
                            session_id: block.id,
                            agent_id: im.agent_data.agent_id,
                            joined: matches!(dialog, ImDialog::SessionAdd),
                        });
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
                if self.sit_requested {
                    self.sit_requested = false;
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.sit = None;
                        circuit.send_agent_sit(now)?;
                    }
                    let transform = &response.sit_transform;
                    self.events.push_back(Event::SitResult {
                        sit_object: response.sit_object.id,
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
                    avatar_id: reply.agent_data.avatar_id,
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
                            pick_id: pick.pick_id,
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
                            classified_id: classified.classified_id,
                            name: trimmed_string(&classified.name),
                        })
                        .collect(),
                });
            }
            AnyMessage::PickInfoReply(reply) => {
                self.events
                    .push_back(Event::PickInfo(Box::new(pick_info(&reply.data))));
            }
            AnyMessage::ClassifiedInfoReply(reply) => {
                self.events
                    .push_back(Event::ClassifiedInfo(Box::new(classified_info(&reply.data))));
            }
            AnyMessage::InventoryDescendents(reply) => {
                let folders: Vec<InventoryFolder> =
                    reply.folder_data.iter().map(inventory_folder).collect();
                let items: Vec<InventoryItem> =
                    reply.item_data.iter().map(inventory_item).collect();
                self.cache_inventory(&folders, &items);
                self.events.push_back(Event::InventoryDescendents {
                    folder_id: reply.agent_data.folder_id,
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
                    let item = inventory_item_from_create(data);
                    self.cache_inventory_item(item.clone());
                    self.events.push_back(Event::InventoryItemCreated {
                        sim_approved: reply.agent_data.sim_approved,
                        transaction_id: reply.agent_data.transaction_id,
                        callback_id: data.callback_id,
                        item,
                    });
                }
            }
            // A batch update the simulator pushed (after a copy, give, or
            // server-side change). Merge folders and items into the cache.
            AnyMessage::BulkUpdateInventory(update) => {
                let folders: Vec<InventoryFolder> =
                    update.folder_data.iter().map(bulk_update_folder).collect();
                let items: Vec<InventoryItem> =
                    update.item_data.iter().map(bulk_update_item).collect();
                // Carry each item's async `CallbackID` (when non-zero) so a client
                // that issued a create/copy can correlate the returned callback id
                // to the resulting item even though the reply arrived here rather
                // than as an `UpdateCreateInventoryItem`.
                let item_callbacks: Vec<(Uuid, u32)> = update
                    .item_data
                    .iter()
                    .filter(|data| data.callback_id != 0)
                    .map(|data| (data.item_id, data.callback_id))
                    .collect();
                self.cache_inventory(&folders, &items);
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
                self.regions.insert(info.sim, info.region_handle);
                self.events.push_back(Event::NeighborDiscovered(info));
            }
            AnyMessage::MapBlockReply(reply) => {
                for (index, data) in reply.data.iter().enumerate() {
                    if let Some(region) = map_region_info(data, reply.size.get(index)) {
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
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.teleport = None;
                    }
                    self.events.push_back(Event::TeleportLocal);
                }
            }
            AnyMessage::TeleportFailed(failed) => {
                if matches!(self.state, SessionState::Teleporting) {
                    self.state = SessionState::Active;
                    self.teleport_target = None;
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
                    let seed = Some(String::from_utf8_lossy(&info.seed_capability).into_owned());
                    self.events.push_back(Event::TeleportFinished {
                        region_handle: info.region_handle,
                        sim: dest,
                        maturity: Maturity::from_sim_access(info.sim_access),
                        flags: TeleportFlags(info.teleport_flags),
                    });
                    self.begin_handover(dest, info.region_handle, seed, now)?;
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
                    let seed = Some(String::from_utf8_lossy(&region.seed_capability).into_owned());
                    self.promote_child_to_root(dest, region.region_handle, seed, now)?;
                }
            }
            AnyMessage::StartPingCheck(ping) => {
                if let Some(circuit) = self.circuit.as_mut() {
                    circuit.send_complete_ping_check(ping.ping_id.ping_id, now)?;
                }
            }
            AnyMessage::PacketAck(ack) => {
                if let Some(circuit) = self.circuit.as_mut() {
                    for packet in &ack.packets {
                        circuit.record_acks(&[packet.id]);
                    }
                }
            }
            AnyMessage::MuteListUpdate(update) => {
                // The mute list changed; download the named file over Xfer.
                let filename = trimmed_string(&update.mute_data.filename);
                if filename.is_empty() {
                    self.events.push_back(Event::MuteList(Vec::new()));
                } else {
                    let xfer_id = self.next_xfer_id;
                    self.next_xfer_id = self.next_xfer_id.checked_add(1).unwrap_or(1);
                    self.mute_xfers.insert(xfer_id, Vec::new());
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_request_xfer(xfer_id, &filename, now)?;
                    }
                }
            }
            AnyMessage::UseCachedMuteList(_) => {
                self.events.push_back(Event::MuteListUnchanged);
            }
            // The simulator requesting an asset upload over the `Xfer` path: it
            // answers a large (non-inlined) `AssetUploadRequest` with a
            // `RequestXfer` whose `VFileID` is the asset id we predicted. Begin
            // streaming `SendXferPacket`s; the simulator pulls each subsequent
            // packet by acking the previous one (`ConfirmXferPacket`).
            AnyMessage::RequestXfer(request) => {
                let xfer_id = request.xfer_id.id;
                let asset_id = request.xfer_id.v_file_id;
                if self.asset_uploads.contains_key(&asset_id) {
                    self.upload_xfers.insert(xfer_id, asset_id);
                    self.advance_upload(xfer_id, asset_id, now)?;
                }
            }
            // The simulator acknowledged one of our upload packets; send the
            // next chunk (the terminal `AssetUploadComplete` follows the last).
            AnyMessage::ConfirmXferPacket(ack) => {
                let xfer_id = ack.xfer_id.id;
                if let Some(&asset_id) = self.upload_xfers.get(&xfer_id) {
                    let more = self
                        .asset_uploads
                        .get(&asset_id)
                        .is_some_and(|upload| upload.sent < upload.packet_count());
                    if more {
                        self.advance_upload(xfer_id, asset_id, now)?;
                    }
                }
            }
            // A legacy UDP upload finished (inline or via `Xfer`): the simulator
            // reports the stored asset's id and whether it succeeded.
            AnyMessage::AssetUploadComplete(complete) => {
                let asset_id = complete.asset_block.uuid;
                let asset_type = AssetType::from_code(i32::from(complete.asset_block.r#type));
                let success = complete.asset_block.success;
                self.asset_uploads.remove(&asset_id);
                self.upload_xfers.retain(|_, id| *id != asset_id);
                self.events.push_back(Event::AssetUploadComplete {
                    asset_id,
                    asset_type,
                    success,
                });
            }
            AnyMessage::SendXferPacket(packet) => {
                let xfer_id = packet.xfer_id.id;
                let packet_num = packet.xfer_id.packet;
                // The high bit marks the final packet; the low 31 bits are the
                // sequence number (the first packet is sequence 0).
                let is_last = packet_num & 0x8000_0000 != 0;
                let sequence = packet_num & 0x7fff_ffff;
                if self.mute_xfers.contains_key(&xfer_id) {
                    // The first packet carries a 4-byte little-endian length
                    // prefix before the file data; later packets are raw.
                    let chunk: &[u8] = if sequence == 0 {
                        packet.data_packet.data.get(4..).unwrap_or(&[])
                    } else {
                        &packet.data_packet.data
                    };
                    if let Some(buffer) = self.mute_xfers.get_mut(&xfer_id) {
                        buffer.extend_from_slice(chunk);
                    }
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_confirm_xfer_packet(xfer_id, packet_num, now)?;
                    }
                    if is_last && let Some(buffer) = self.mute_xfers.remove(&xfer_id) {
                        self.events
                            .push_back(Event::MuteList(parse_mute_list(&buffer)));
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
                        id,
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
                        id,
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
                self.events.push_back(Event::TextureNotFound(id));
            }
            AnyMessage::TransferInfo(info) => {
                // The transfer's initial status/size. A non-success status here
                // (e.g. the asset is missing or denied) means no data follows.
                let transfer_id = info.transfer_info.transfer_id;
                let status = TransferStatus::from_code(info.transfer_info.status);
                if matches!(status, TransferStatus::Ok | TransferStatus::Done) {
                    // Success: the asset exists and its bytes follow as
                    // `TransferPacket`s. Surface the declared total size so a
                    // caller can show progress / preallocate before they arrive.
                    if let Some(transfer) = self.asset_transfers.get(&transfer_id) {
                        self.events.push_back(Event::AssetTransferStarted {
                            asset_id: transfer.asset_id,
                            asset_type: transfer.asset_type,
                            size: info.transfer_info.size,
                        });
                    }
                } else if let Some(transfer) = self.asset_transfers.remove(&transfer_id) {
                    self.events.push_back(Event::AssetTransferFailed {
                        asset_id: transfer.asset_id,
                        asset_type: transfer.asset_type,
                        status,
                    });
                }
            }
            AnyMessage::TransferPacket(packet) => {
                // A data chunk of a generic asset transfer; the final packet
                // carries `LLTS_DONE`.
                let transfer_id = packet.transfer_data.transfer_id;
                let status = TransferStatus::from_code(packet.transfer_data.status);
                let packet_index = packet.transfer_data.packet;
                let mut done = false;
                let mut failed = false;
                if let Some(transfer) = self.asset_transfers.get_mut(&transfer_id) {
                    transfer
                        .chunks
                        .insert(packet_index, packet.transfer_data.data.clone());
                    match status {
                        TransferStatus::Done => done = true,
                        TransferStatus::Ok => {}
                        _ => failed = true,
                    }
                }
                if done {
                    if let Some(transfer) = self.asset_transfers.remove(&transfer_id) {
                        let asset = Asset {
                            id: transfer.asset_id,
                            asset_type: transfer.asset_type,
                            data: transfer.assemble(),
                        };
                        self.events.push_back(Event::AssetReceived(Box::new(asset)));
                    }
                } else if failed && let Some(transfer) = self.asset_transfers.remove(&transfer_id) {
                    self.events.push_back(Event::AssetTransferFailed {
                        asset_id: transfer.asset_id,
                        asset_type: transfer.asset_type,
                        status,
                    });
                }
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
                            item_id: block.item_id,
                            asset_id: block.asset_id,
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
                    avatar_id: animation.sender.id,
                    animations: avatar_animations(animation),
                    physical_events: animation
                        .physical_avatar_event_list
                        .iter()
                        .map(|block| block.type_data.clone())
                        .collect(),
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
                    object_id: block.object_id,
                    parent_id: (!block.parent_id.is_nil()).then_some(block.parent_id),
                    region_handle: block.handle,
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
                    object_id: block.object_id,
                    owner_id: block.owner_id,
                    gain: block.gain,
                    flags: SoundFlags(block.flags),
                });
            }
            // A volume change for a sound already attached to an object.
            AnyMessage::AttachedSoundGainChange(change) => {
                let block = &change.data_block;
                self.events.push_back(Event::AttachedSoundGainChange {
                    object_id: block.object_id,
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
                            object_id: block.object_id,
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
                        agent_id: agent.agent_id,
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
                            agent_id: block.agent_id,
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
                            agent_id: block.agent_id,
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
                            group_id: block.group_id,
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
                            event_id: block.event_id,
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
                        .map(|block| DirClassifiedResult {
                            classified_id: block.classified_id,
                            name: trimmed_string(&block.name),
                            classified_flags: block.classified_flags,
                            creation_date: block.creation_date,
                            expiration_date: block.expiration_date,
                            price_for_listing: block.price_for_listing,
                        })
                        .collect(),
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
                            parcel_id: block.parcel_id,
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
                        .map(|block| DirLandResult {
                            parcel_id: block.parcel_id,
                            name: trimmed_string(&block.name),
                            auction: block.auction,
                            for_sale: block.for_sale,
                            sale_price: block.sale_price,
                            actual_area: block.actual_area,
                        })
                        .collect(),
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
                            avatar_id: block.avatar_id,
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
                        .map(|block| PlacesResult {
                            owner_id: block.owner_id,
                            name: trimmed_string(&block.name),
                            description: trimmed_string(&block.desc),
                            actual_area: block.actual_area,
                            billable_area: block.billable_area,
                            flags: block.flags,
                            global_position: (block.global_x, block.global_y, block.global_z),
                            sim_name: trimmed_string(&block.sim_name),
                            snapshot_id: block.snapshot_id,
                            dwell: block.dwell,
                            price: block.price,
                        })
                        .collect(),
                });
            }
            // The full detail of an event, in reply to an `EventInfoRequest`.
            AnyMessage::EventInfoReply(reply) => {
                let data = &reply.event_data;
                let [global_x, global_y, global_z] = data.global_pos;
                self.events.push_back(Event::EventInfoReply {
                    info: EventInfo {
                        event_id: data.event_id,
                        creator: parse_uuid_string(&data.creator),
                        name: trimmed_string(&data.name),
                        category: trimmed_string(&data.category),
                        description: trimmed_string(&data.desc),
                        date: trimmed_string(&data.date),
                        date_utc: data.date_utc,
                        duration: data.duration,
                        cover: data.cover,
                        amount: data.amount,
                        sim_name: trimmed_string(&data.sim_name),
                        global_position: (global_x, global_y, global_z),
                        flags: data.event_flags,
                    },
                });
            }
            // An object's pay-button layout, in reply to a `RequestPayPrice`.
            AnyMessage::PayPriceReply(reply) => {
                self.events.push_back(Event::PayPriceReply {
                    object_id: reply.object_data.object_id,
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
                        object_id: data.object_id,
                        owner_id: data.owner_id,
                        group_id: data.group_id,
                        base_mask: data.base_mask,
                        owner_mask: data.owner_mask,
                        group_mask: data.group_mask,
                        everyone_mask: data.everyone_mask,
                        next_owner_mask: data.next_owner_mask,
                        ownership_cost: data.ownership_cost,
                        sale_type: data.sale_type,
                        sale_price: data.sale_price,
                        category: data.category,
                        last_owner_id: data.last_owner_id,
                        name: trimmed_string(&data.name),
                        description: trimmed_string(&data.description),
                    },
                });
            }
            AnyMessage::GenericMessage(generic)
                // The sim NUL-terminates the method name on the wire.
                if trimmed_string(&generic.method_data.method) == "emptymutelist" =>
            {
                self.events.push_back(Event::MuteList(Vec::new()));
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
            AnyMessage::LoadURL(load) => {
                let data = &load.data;
                self.events
                    .push_back(Event::LoadUrl(Box::new(LoadUrlRequest {
                        object_name: trimmed_string(&data.object_name),
                        object_id: data.object_id,
                        owner_id: data.owner_id,
                        owner_is_group: data.owner_is_group,
                        message: trimmed_string(&data.message),
                        url: trimmed_string(&data.url),
                    })));
            }
            AnyMessage::ScriptTeleportRequest(request) => {
                let data = &request.data;
                self.events
                    .push_back(Event::ScriptTeleport(Box::new(ScriptTeleportRequest {
                        object_name: trimmed_string(&data.object_name),
                        region_name: trimmed_string(&data.sim_name),
                        position: (
                            data.sim_position.x,
                            data.sim_position.y,
                            data.sim_position.z,
                        ),
                        look_at: (data.look_at.x, data.look_at.y, data.look_at.z),
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
                    update.group_data.iter().map(group_membership).collect(),
                ));
            }
            AnyMessage::GroupMembersReply(reply) => {
                self.events.push_back(Event::GroupMembers {
                    group_id: reply.group_data.group_id,
                    request_id: reply.group_data.request_id,
                    member_count: reply.group_data.member_count,
                    members: reply.member_data.iter().map(group_member).collect(),
                });
            }
            AnyMessage::GroupRoleDataReply(reply) => {
                self.events.push_back(Event::GroupRoleData {
                    group_id: reply.group_data.group_id,
                    request_id: reply.group_data.request_id,
                    role_count: reply.group_data.role_count,
                    roles: reply.role_data.iter().map(group_role).collect(),
                });
            }
            AnyMessage::GroupRoleMembersReply(reply) => {
                self.events.push_back(Event::GroupRoleMembers {
                    group_id: reply.agent_data.group_id,
                    request_id: reply.agent_data.request_id,
                    total_pairs: reply.agent_data.total_pairs,
                    pairs: reply
                        .member_data
                        .iter()
                        .map(|pair| GroupRoleMember {
                            role_id: pair.role_id,
                            member_id: pair.member_id,
                        })
                        .collect(),
                });
            }
            AnyMessage::GroupTitlesReply(reply) => {
                self.events.push_back(Event::GroupTitles {
                    group_id: reply.agent_data.group_id,
                    request_id: reply.agent_data.request_id,
                    titles: reply.group_data.iter().map(group_title).collect(),
                });
            }
            AnyMessage::GroupProfileReply(reply) => {
                self.events
                    .push_back(Event::GroupProfileReceived(Box::new(group_profile(
                        &reply.group_data,
                    ))));
            }
            AnyMessage::GroupNoticesListReply(reply) => {
                self.events.push_back(Event::GroupNotices {
                    group_id: reply.agent_data.group_id,
                    notices: reply.data.iter().map(group_notice).collect(),
                });
            }
            AnyMessage::CreateGroupReply(reply) => {
                self.events.push_back(Event::CreateGroupResult {
                    group_id: reply.reply_data.group_id,
                    success: reply.reply_data.success,
                    message: trimmed_string(&reply.reply_data.message),
                });
            }
            AnyMessage::JoinGroupReply(reply) => {
                self.events.push_back(Event::JoinGroupResult {
                    group_id: reply.group_data.group_id,
                    success: reply.group_data.success,
                });
            }
            AnyMessage::LeaveGroupReply(reply) => {
                self.events.push_back(Event::LeaveGroupResult {
                    group_id: reply.group_data.group_id,
                    success: reply.group_data.success,
                });
            }
            AnyMessage::AgentDropGroup(drop) => {
                self.events.push_back(Event::DroppedFromGroup {
                    group_id: drop.agent_data.group_id,
                });
            }
            AnyMessage::EjectGroupMemberReply(reply) => {
                self.events.push_back(Event::EjectGroupMemberResult {
                    group_id: reply.group_data.group_id,
                    success: reply.eject_data.success,
                });
            }
            AnyMessage::OnlineNotification(notification) => {
                let ids = notification
                    .agent_block
                    .iter()
                    .map(|block| block.agent_id)
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    self.events.push_back(Event::FriendsOnline(ids));
                }
            }
            AnyMessage::OfflineNotification(notification) => {
                let ids = notification
                    .agent_block
                    .iter()
                    .map(|block| block.agent_id)
                    .collect::<Vec<_>>();
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
                    .map_or_else(Uuid::nil, |circuit| circuit.agent_id);
                for block in &change.rights {
                    let granted_to_us = change.agent_data.agent_id != own;
                    let friend_id = if granted_to_us {
                        change.agent_data.agent_id
                    } else {
                        block.agent_related
                    };
                    self.events.push_back(Event::FriendRightsChanged {
                        friend_id,
                        rights: FriendRights(block.related_rights),
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
            self.teleport_target = None;
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
                    sequence,
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
            self.sit_requested = false;
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
        }
        for (sequence, name) in child_exhausted {
            tracing::warn!(
                sequence,
                message = name.unwrap_or("?"),
                "reliable packet on a child circuit exhausted its retransmission budget"
            );
            self.push_diagnostic(Diagnostic::ExpectedReplyMissing {
                request: name.map_or_else(|| "reliable packet".to_owned(), str::to_owned),
                sequence: Some(sequence),
            });
        }
        for addr in dead {
            self.children.remove(&addr);
            self.child_seeds.remove(&addr);
            self.forget_sim_objects(addr);
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
        channel: i32,
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
        circuit.send_chat_from_viewer("", chat_type, 0, now)?;
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
        to_agent_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_instant_message_raw(
            to_agent_id,
            ImDialog::Message,
            message,
            &from_name,
            now,
        )?;
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
        to_agent_id: Uuid,
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
        to_agent_id: Uuid,
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
        self.send_agent_update_now(ControlFlags::STAND_UP, now)
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
    pub fn sit_on(&mut self, target: Uuid, offset: Vector, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_request_sit(target, offset, now)?;
        circuit.timers.sit = Some(deadline(now, SIT_TIMEOUT));
        self.sit_requested = true;
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
    pub fn request_avatar_properties(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_properties_request(target, now)?;
        Ok(())
    }

    /// Requests the picks of the avatar `target` (a `GenericMessage`
    /// `avatarpicksrequest`). The reply arrives as [`Event::AvatarPicks`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_picks(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarpicksrequest", &[target.to_string()], now)?;
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
    pub fn request_avatar_notes(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarnotesrequest", &[target.to_string()], now)?;
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
    pub fn request_avatar_classifieds(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarclassifiedsrequest", &[target.to_string()], now)?;
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
        creator_id: Uuid,
        pick_id: Uuid,
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
        classified_id: Uuid,
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
        target: Uuid,
        notes: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_notes_update(target, notes, now)?;
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
    pub fn delete_pick(&mut self, pick_id: Uuid, now: Instant) -> Result<(), Error> {
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
        pick_id: Uuid,
        query_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_pick_god_delete(pick_id, query_id, now)?;
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
    pub fn delete_classified(&mut self, classified_id: Uuid, now: Instant) -> Result<(), Error> {
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
        classified_id: Uuid,
        query_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_classified_god_delete(classified_id, query_id, now)?;
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
        target: Uuid,
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
    pub fn terminate_friendship(&mut self, other: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_terminate_friendship(other, now)?;
        Ok(())
    }

    /// Accepts a friendship offer via `AcceptFriendship`. The `transaction_id`
    /// is the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming
    /// [`ImDialog::FriendshipOffered`] IM; `calling_card_folder` is the
    /// inventory folder to place the new friend's calling card in (use the
    /// Calling Cards system folder, or the inventory root).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn accept_friendship(
        &mut self,
        transaction_id: Uuid,
        calling_card_folder: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_accept_friendship(transaction_id, calling_card_folder, now)?;
        Ok(())
    }

    /// Declines a friendship offer via `DeclineFriendship`. The `transaction_id`
    /// is the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming
    /// [`ImDialog::FriendshipOffered`] IM.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn decline_friendship(&mut self, transaction_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_decline_friendship(transaction_id, now)?;
        Ok(())
    }

    /// Makes `group_id` the agent's active group (`ActivateGroup`); pass
    /// [`Uuid::nil`] to clear it. The simulator confirms with an
    /// [`Event::ActiveGroupChanged`]. The agent's memberships arrive at login as
    /// [`Event::GroupMemberships`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn activate_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn request_group_members(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn request_group_roles(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
        group_id: Uuid,
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
    pub fn request_group_titles(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn request_group_profile(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn request_group_notices(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn request_group_notice(&mut self, notice_id: Uuid, now: Instant) -> Result<(), Error> {
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

    /// Joins an open-enrollment group (`JoinGroupRequest`). The result arrives as
    /// [`Event::JoinGroupResult`]. Closed groups require an invitation instead
    /// (see [`Session::invite_to_group`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn join_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn leave_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
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
        group_id: Uuid,
        invitees: &[(Uuid, Uuid)],
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
        group_id: Uuid,
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
        group_id: Uuid,
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
    pub fn start_group_session(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(
            group_id,
            ImDialog::SessionGroupStart,
            "",
            &from_name,
            now,
        )?;
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
        group_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(group_id, ImDialog::SessionSend, message, &from_name, now)?;
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
    pub fn leave_group_session(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(group_id, ImDialog::SessionLeave, "", &from_name, now)?;
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
        group_id: Uuid,
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
        group_id: Uuid,
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
        group_id: Uuid,
        member_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_eject_group_members(group_id, member_ids, now)?;
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
        group_id: Uuid,
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
                to_agent_id: group_id,
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
        targets: &[Uuid],
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_start_lure(targets, message, now)?;
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
    pub fn accept_teleport_lure(&mut self, lure_id: Uuid, now: Instant) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Active) {
            return Err(Error::NotActive);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_teleport_lure_request(lure_id, TELEPORT_FLAGS_VIA_LURE, now)?;
        circuit.timers.teleport = Some(deadline(now, TELEPORT_TIMEOUT));
        // Best-effort destination hint; a cross-region lure's TeleportFinish
        // carries the authoritative handle, so a non-fake-parcel id is harmless.
        self.teleport_target = Some(parse_lure_region_handle(lure_id));
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
        from_agent_id: Uuid,
        lure_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: from_agent_id,
                from_group: false,
                dialog: ImDialog::LureDeclined,
                id: lure_id,
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
        to_agent_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id,
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
        to_agent_id: Uuid,
        item_id: Uuid,
        asset_type: AssetType,
        item_name: &str,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let bucket = inventory_offer_bucket(asset_type, item_id);
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id,
                from_group: false,
                dialog: ImDialog::InventoryOffered,
                id: transaction_id,
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
        to_agent_id: Uuid,
        folder_id: Uuid,
        folder_name: &str,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let bucket = inventory_offer_bucket(AssetType::Folder, folder_id);
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id,
                from_group: false,
                dialog: ImDialog::InventoryOffered,
                id: transaction_id,
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
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: offer.from_agent_id,
                from_group: false,
                dialog: ImDialog::InventoryAccepted,
                id: offer.transaction_id,
                message: "",
                from_name: &from_name,
                binary_bucket: folder_id.as_bytes().to_vec(),
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
        trash_folder_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: offer.from_agent_id,
                from_group: false,
                dialog: ImDialog::InventoryDeclined,
                id: offer.transaction_id,
                message: "",
                from_name: &from_name,
                binary_bucket: trash_folder_id.as_bytes().to_vec(),
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
        session_id: Uuid,
        invitees: &[Uuid],
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let bucket = pack_uuids(invitees);
        let to_agent_id = invitees.first().copied().unwrap_or(session_id);
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id,
                from_group: false,
                dialog: ImDialog::SessionConferenceStart,
                id: session_id,
                message,
                from_name: &from_name,
                binary_bucket: bucket,
            },
            now,
        )?;
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
        session_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: session_id,
                from_group: false,
                dialog: ImDialog::SessionSend,
                id: session_id,
                message,
                from_name: &from_name,
                binary_bucket: Vec::new(),
            },
            now,
        )?;
        Ok(())
    }

    /// Leaves an ad-hoc conference / multi-party IM session (`IM_SESSION_LEAVE`),
    /// so the agent stops receiving its chat.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn leave_conference(&mut self, session_id: Uuid, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_im(
            &OutgoingIm {
                to_agent_id: session_id,
                from_group: false,
                dialog: ImDialog::SessionLeave,
                id: session_id,
                message: "",
                from_name: &from_name,
                binary_bucket: Vec::new(),
            },
            now,
        )?;
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
        object_id: Uuid,
        chat_channel: i32,
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
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the reply fails to encode.
    pub fn answer_script_permissions(
        &mut self,
        task_id: Uuid,
        item_id: Uuid,
        permissions: ScriptPermissions,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_script_answer_yes(task_id, item_id, permissions.0, now)?;
        Ok(())
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
        texture_id: Uuid,
        discard_level: i8,
        priority: f32,
        now: Instant,
    ) -> Result<(), Error> {
        // A fresh download buffer; a repeat request just restarts it.
        self.texture_downloads.insert(
            texture_id,
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

    /// Requests a generic asset (sound, animation, notecard, landmark, mesh, …)
    /// by asset id and class over the UDP transfer path (`TransferRequest`). The
    /// simulator replies with a `TransferInfo` (size/status) then `TransferPacket`
    /// chunks, reassembled and surfaced as [`Event::AssetReceived`] (or
    /// [`Event::AssetTransferFailed`] if the asset is missing or denied).
    /// `priority` orders concurrent transfers. The modern alternative is the HTTP
    /// `GetAsset`/`GetMesh` capability (a runtime `FetchAsset`/`FetchMesh`
    /// command).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_asset(
        &mut self,
        asset_id: Uuid,
        asset_type: AssetType,
        priority: f32,
        now: Instant,
    ) -> Result<(), Error> {
        let transfer_id = Uuid::from_u128(self.next_transfer_id);
        self.next_transfer_id = self.next_transfer_id.checked_add(1).unwrap_or(1);
        self.asset_transfers.insert(
            transfer_id,
            AssetTransfer {
                asset_id,
                asset_type,
                chunks: BTreeMap::new(),
            },
        );
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_transfer_request(transfer_id, asset_id, asset_type, priority, now)?;
        Ok(())
    }

    /// Uploads `data` as a new asset of class `asset_type` over the **legacy UDP
    /// path** (`AssetUploadRequest`), returning the asset's predicted UUID (the
    /// same id the simulator will report in the terminating
    /// [`Event::AssetUploadComplete`]).
    ///
    /// Small assets (≤ `MAX_INLINE_ASSET` bytes) are inlined in the request;
    /// larger ones are streamed over the `Xfer` path automatically (the simulator
    /// answers with a `RequestXfer` and the session streams `SendXferPacket`s,
    /// driven by the simulator's `ConfirmXferPacket`s). `temp_file` marks a
    /// temporary asset; `store_local` keeps it on the simulator only.
    ///
    /// This path stores **only the asset** — it does not create an inventory
    /// item (a viewer would follow up with a `CreateInventoryItem` referencing
    /// the same transaction id). For an upload that also creates an inventory
    /// item, use the modern CAPS path (the runtimes' `UploadAsset` command).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn upload_asset_udp(
        &mut self,
        asset_type: AssetType,
        data: Vec<u8>,
        temp_file: bool,
        store_local: bool,
        now: Instant,
    ) -> Result<Uuid, Error> {
        let transaction_id = Uuid::from_u128(self.next_upload_id);
        self.next_upload_id = self.next_upload_id.checked_add(1).unwrap_or(1);
        let asset_id = sl_wire::combine_uuids(transaction_id, self.secure_session_id);
        let inline = data.len() <= MAX_INLINE_ASSET;
        // The simulator treats an `AssetData` of more than 2 bytes as the inline
        // asset; an empty payload forces the `Xfer` path.
        let request_data = if inline { data.clone() } else { Vec::new() };
        self.asset_uploads.insert(
            asset_id,
            AssetUpload {
                // The inline path needs no buffered copy; only the `Xfer` path
                // streams from `data`.
                data: if inline { Vec::new() } else { data },
                sent: 0,
            },
        );
        // The `AssetUploadRequest` `Type` field is a signed byte; every real
        // `LLAssetType` code fits, but clamp defensively.
        let type_code = i8::try_from(asset_type.to_code()).unwrap_or(0);
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_asset_upload_request(
            transaction_id,
            type_code,
            temp_file,
            store_local,
            request_data,
            now,
        )?;
        Ok(asset_id)
    }

    /// Sends the next `SendXferPacket` of the upload keyed by `asset_id` over the
    /// root circuit, flagging the final packet, and advancing its sent counter.
    fn advance_upload(&mut self, xfer_id: u64, asset_id: Uuid, now: Instant) -> Result<(), Error> {
        let Some(upload) = self.asset_uploads.get_mut(&asset_id) else {
            return Ok(());
        };
        let sequence = upload.sent;
        let total = upload.packet_count();
        // The final packet's number carries the high-bit last-packet flag.
        let is_last = sequence.saturating_add(1) >= total;
        let packet = if is_last {
            sequence | 0x8000_0000
        } else {
            sequence
        };
        let data = upload.packet_data(sequence);
        upload.sent = sequence.saturating_add(1);
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.send_send_xfer_packet(xfer_id, packet, data, now)?;
        }
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
    /// worn at `attachment_point` and rotated by `rotation`. When `add` is `true`
    /// the object is added to the point alongside anything already there rather
    /// than replacing it. To wear an item straight from inventory instead, use
    /// [`Session::rez_attachment`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn attach_object(
        &mut self,
        local_id: u32,
        attachment_point: AttachmentPoint,
        add: bool,
        rotation: &Rotation,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_attach(local_id, attachment_point, add, rotation, now)?;
        Ok(())
    }

    /// Detaches the attachments `local_ids` back to inventory via `ObjectDetach`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn detach_objects(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_detach(local_ids, now)?;
        Ok(())
    }

    /// Drops the attachments `local_ids` from the avatar onto the ground via
    /// `ObjectDrop` (they become ordinary in-world objects).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn drop_attachments(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_drop(local_ids, now)?;
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
        item_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_attachment(attachment_point, item_id, now)?;
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
    /// correlating the message's parts; `first_detach_all` detaches everything
    /// currently worn first.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_attachments(
        &mut self,
        compound_id: Uuid,
        first_detach_all: bool,
        attachments: &[RezAttachment],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_rez_multiple_attachments(compound_id, first_detach_all, attachments, now)?;
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
    pub fn track_agent(&mut self, prey_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn find_agent(&mut self, hunter: Uuid, prey: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_find_agent(hunter, prey, now)?;
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
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_find_query(query_id, query_text, flags, query_start, now)?;
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
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        category: ParcelCategory,
        sim_name: &str,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_places_query(
            query_id,
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
        query_id: Uuid,
        flags: DirFindFlags,
        search_type: LandSearchType,
        price: i32,
        area: i32,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_land_query(query_id, flags, search_type, price, area, query_start, now)?;
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
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        category: u32,
        query_start: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_dir_classified_query(
            query_id,
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
        query_id: Uuid,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_picker_request(query_id, name, now)?;
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
        query_id: Uuid,
        transaction_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        category: ParcelCategory,
        sim_name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_places_query(
            query_id,
            transaction_id,
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
    pub fn event_info_request(&mut self, event_id: u32, now: Instant) -> Result<(), Error> {
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
        event_id: u32,
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
        event_id: u32,
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
        group_id: Uuid,
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
        object_id: Uuid,
        item_id: Uuid,
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_buy_object_inventory(object_id, item_id, folder_id, now)?;
        Ok(())
    }

    /// Requests an object's pay-button layout via `RequestPayPrice`. The reply
    /// arrives as [`Event::PayPriceReply`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_pay_price(&mut self, object_id: Uuid, now: Instant) -> Result<(), Error> {
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
        object_id: Uuid,
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
    pub fn spin_object_start(&mut self, object_id: Uuid, now: Instant) -> Result<(), Error> {
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
        object_id: Uuid,
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
    pub fn spin_object_stop(&mut self, object_id: Uuid, now: Instant) -> Result<(), Error> {
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
        local_ids: &[u32],
        group_id: Uuid,
        ray_start: Vector,
        ray_end: Vector,
        bypass_raycast: bool,
        ray_end_is_intersection: bool,
        copy_centers: bool,
        copy_rotates: bool,
        ray_target_id: Uuid,
        duplicate_flags: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_duplicate_on_ray(
            local_ids,
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
        animations: &[(Uuid, bool)],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_animation(animations, now)?;
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
    pub fn play_animation(&mut self, anim_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn stop_animation(&mut self, anim_id: Uuid, now: Instant) -> Result<(), Error> {
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
    pub fn agent_id(&self) -> Option<Uuid> {
        self.circuit.as_ref().map(|circuit| circuit.agent_id)
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
    pub const fn circuit_code(&self) -> Option<u32> {
        match self.circuit.as_ref() {
            Some(circuit) => Some(circuit.code),
            None => None,
        }
    }

    /// The agent's inventory root ("My Inventory") folder id, from the login
    /// response, or `None` if the grid did not provide it. Use it as the starting
    /// point for [`Session::request_folder_contents`].
    #[must_use]
    pub const fn inventory_root(&self) -> Option<Uuid> {
        self.inventory_root
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
    pub fn request_folder_contents(&mut self, folder_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_fetch_inventory_descendents(folder_id, now)?;
        Ok(())
    }

    // ---- Inventory cache (#30) ---------------------------------------------

    /// A cached inventory folder by id, if known (from the login skeleton, a
    /// folder-contents fetch, a simulator push, or the agent's own mutations).
    #[must_use]
    pub fn inventory_folder(&self, folder_id: Uuid) -> Option<&InventoryFolder> {
        self.inventory_folders.get(&folder_id)
    }

    /// A cached inventory item by id, if known (from a folder-contents fetch, a
    /// simulator push, or the agent's own mutations).
    #[must_use]
    pub fn inventory_item(&self, item_id: Uuid) -> Option<&InventoryItem> {
        self.inventory_items.get(&item_id)
    }

    /// All cached inventory folders, keyed by folder id.
    #[must_use]
    pub const fn inventory_folders(&self) -> &BTreeMap<Uuid, InventoryFolder> {
        &self.inventory_folders
    }

    /// All cached inventory items, keyed by item id.
    #[must_use]
    pub const fn inventory_items(&self) -> &BTreeMap<Uuid, InventoryItem> {
        &self.inventory_items
    }

    /// The cached immediate children of `folder_id`: its sub-folders and the
    /// items directly inside it. Only as complete as the cache (fetch the folder
    /// with [`Session::request_folder_contents`], or the modern AIS3 CAPS path,
    /// to populate it).
    #[must_use]
    pub fn inventory_children(
        &self,
        folder_id: Uuid,
    ) -> (Vec<&InventoryFolder>, Vec<&InventoryItem>) {
        let folders = self
            .inventory_folders
            .values()
            .filter(|folder| folder.parent_id == folder_id)
            .collect();
        let items = self
            .inventory_items
            .values()
            .filter(|item| item.folder_id == folder_id)
            .collect();
        (folders, items)
    }

    /// Inserts/updates a folder in the cache. A version of `0` (as carried by a
    /// descendents reply's sub-folders, which omit it) does not clobber a known
    /// version from the login skeleton.
    fn cache_inventory_folder(&mut self, mut folder: InventoryFolder) {
        if let (0, Some(existing)) = (
            folder.version,
            self.inventory_folders.get(&folder.folder_id),
        ) {
            folder.version = existing.version;
        }
        self.inventory_folders.insert(folder.folder_id, folder);
    }

    /// Merges a batch of folders and items into the live cache (from a
    /// descendents fetch or a simulator push).
    fn cache_inventory(&mut self, folders: &[InventoryFolder], items: &[InventoryItem]) {
        for folder in folders {
            self.cache_inventory_folder(folder.clone());
        }
        for item in items {
            self.cache_inventory_item(item.clone());
        }
    }

    /// Inserts/updates an item in the cache.
    fn cache_inventory_item(&mut self, item: InventoryItem) {
        self.inventory_items.insert(item.item_id, item);
    }

    /// Recursively drops a folder's cached descendents (sub-folders and items),
    /// used by [`Session::purge_inventory_descendents`].
    fn purge_cached_descendents(&mut self, folder_id: Uuid) {
        let subfolders: Vec<Uuid> = self
            .inventory_folders
            .values()
            .filter(|folder| folder.parent_id == folder_id)
            .map(|folder| folder.folder_id)
            .collect();
        self.inventory_items
            .retain(|_, item| item.folder_id != folder_id);
        for sub in subfolders {
            self.purge_cached_descendents(sub);
            self.inventory_folders.remove(&sub);
        }
    }

    /// Allocates the next async inventory `CallbackID` (never zero).
    fn next_inventory_callback(&mut self) -> u32 {
        let id = self.next_inventory_callback;
        self.next_inventory_callback = self.next_inventory_callback.wrapping_add(1).max(1);
        id
    }

    // ---- Inventory mutation over UDP (#30) ---------------------------------

    /// Creates a new inventory folder `folder_id` (a fresh, caller-chosen id)
    /// named `name` of `folder_type` (a `FolderType`, or `-1` for none) under
    /// `parent_id`, via `CreateInventoryFolder`. The simulator sends no reply, so
    /// the folder is added to the local cache optimistically. On Second Life the
    /// modern AIS3 CAPS path (or the `CreateInventoryCategory` cap) returns a
    /// confirmed result instead.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn create_inventory_folder(
        &mut self,
        folder_id: Uuid,
        parent_id: Uuid,
        folder_type: i8,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_inventory_folder(folder_id, parent_id, folder_type, name, now)?;
        self.cache_inventory_folder(InventoryFolder {
            folder_id,
            parent_id,
            name: name.to_owned(),
            folder_type,
            version: 1,
        });
        Ok(())
    }

    /// Renames / re-types / re-parents the existing folder `folder_id` via
    /// `UpdateInventoryFolder`. The cache is updated optimistically.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn update_inventory_folder(
        &mut self,
        folder_id: Uuid,
        parent_id: Uuid,
        folder_type: i8,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_inventory_folder(folder_id, parent_id, folder_type, name, now)?;
        self.cache_inventory_folder(InventoryFolder {
            folder_id,
            parent_id,
            name: name.to_owned(),
            folder_type,
            version: self
                .inventory_folders
                .get(&folder_id)
                .map_or(1, |folder| folder.version),
        });
        Ok(())
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
        folder_id: Uuid,
        parent_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        self.move_inventory_folders(&[(folder_id, parent_id)], false, now)
    }

    /// Re-parents several folders in one `MoveInventoryFolder` (each a
    /// `(folder, new_parent)` pair). `stamp` asks the simulator to re-timestamp
    /// the moved children. The cache is updated optimistically.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established, or
    /// [`Error::Wire`] on an encode failure.
    pub fn move_inventory_folders(
        &mut self,
        moves: &[(Uuid, Uuid)],
        stamp: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_move_inventory_folders(moves, stamp, now)?;
        for &(folder_id, parent_id) in moves {
            if let Some(folder) = self.inventory_folders.get_mut(&folder_id) {
                folder.parent_id = parent_id;
            }
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
        folder_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_inventory_folders(folder_ids, now)?;
        for &folder_id in folder_ids {
            self.purge_cached_descendents(folder_id);
            self.inventory_folders.remove(&folder_id);
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
    ) -> Result<u32, Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_inventory_item(new, callback_id, now)?;
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
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_inventory_item(item, transaction_id, callback_id, now)?;
        self.cache_inventory_item(item.clone());
        Ok(())
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
        item_id: Uuid,
        folder_id: Uuid,
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
        moves: &[(Uuid, Uuid, String)],
        stamp: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_move_inventory_items(moves, stamp, now)?;
        for (item_id, folder_id, new_name) in moves {
            if let Some(item) = self.inventory_items.get_mut(item_id) {
                item.folder_id = *folder_id;
                if !new_name.is_empty() {
                    item.name.clone_from(new_name);
                }
            }
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
        old_agent_id: Uuid,
        old_item_id: Uuid,
        new_folder_id: Uuid,
        new_name: &str,
        now: Instant,
    ) -> Result<u32, Error> {
        let callback_id = self.next_inventory_callback();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_copy_inventory_item(
            old_agent_id,
            old_item_id,
            new_folder_id,
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
    pub fn remove_inventory_items(&mut self, item_ids: &[Uuid], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_inventory_items(item_ids, now)?;
        for item_id in item_ids {
            self.inventory_items.remove(item_id);
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
        item_id: Uuid,
        flags: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_change_inventory_item_flags(item_id, flags, now)?;
        if let Some(item) = self.inventory_items.get_mut(&item_id) {
            item.flags = flags;
        }
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
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_purge_inventory_descendents(folder_id, now)?;
        self.purge_cached_descendents(folder_id);
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
        folder_ids: &[Uuid],
        item_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_inventory_objects(folder_ids, item_ids, now)?;
        for &folder_id in folder_ids {
            self.purge_cached_descendents(folder_id);
            self.inventory_folders.remove(&folder_id);
        }
        for item_id in item_ids {
            self.inventory_items.remove(item_id);
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
    pub fn request_avatar_names(&mut self, ids: &[Uuid], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn request_group_names(&mut self, ids: &[Uuid], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: i32,
        scope: ParcelAccessScope,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_access_list_request(local_id, scope.to_u32(), now)?;
        Ok(())
    }

    /// Replaces a parcel's allow or ban list via `ParcelAccessListUpdate`. An
    /// empty `entries` clears the list. Requires parcel ownership / land edit
    /// rights.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn update_parcel_access_list(
        &mut self,
        local_id: i32,
        scope: ParcelAccessScope,
        entries: &[ParcelAccessEntry],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_access_list_update(local_id, scope.to_u32(), entries, now)?;
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
    pub fn request_parcel_dwell(&mut self, local_id: i32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: i32,
        price: i32,
        area: i32,
        group_id: Uuid,
        is_group_owned: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: i32,
        return_type: ParcelReturnType,
        owner_ids: &[Uuid],
        task_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_return_objects(local_id, return_type.0, owner_ids, task_ids, now)?;
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
        local_id: i32,
        return_type: ParcelReturnType,
        object_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: i32,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn reclaim_parcel(&mut self, local_id: i32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn release_parcel(&mut self, local_id: i32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn buy_parcel_pass(&mut self, local_id: i32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: i32,
        return_type: ParcelReturnType,
        owner_ids: &[Uuid],
        task_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_disable_objects(local_id, return_type.0, owner_ids, task_ids, now)?;
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
    pub fn request_parcel_info(&mut self, parcel_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_info_request(parcel_id, now)?;
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
        target: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let params = [
            circuit.agent_id.to_string(),
            delta.to_u32().to_string(),
            target.to_string(),
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
    pub fn kick_estate_user(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_estate_owner_message("kickestate", &[target.to_string()], now)?;
        Ok(())
    }

    /// Teleports an agent home via `EstateOwnerMessage`/`teleporthomeuser`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn teleport_home_user(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let params = [circuit.agent_id.to_string(), target.to_string()];
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
    pub fn connect_telehub(&mut self, object_local_id: u32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        object_local_id: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn god_kick_user(&mut self, target: Uuid, reason: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_god_kick_user(target, reason, now)?;
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
        region_handle: u64,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_map_item_request(item_type.to_u32(), region_handle, now)?;
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
    pub fn objects_in_region(&self, region_handle: u64) -> impl Iterator<Item = &Object> {
        self.objects()
            .filter(move |object| object.region_handle == region_handle)
    }

    /// Looks up a cached scene object by its region-local id in the region the
    /// agent is currently in (the root circuit). Use [`Session::objects`] /
    /// [`Session::objects_in_region`] to reach neighbour-region objects, whose
    /// local ids share the same numeric space.
    #[must_use]
    pub fn object(&self, local_id: u32) -> Option<&Object> {
        let root = self.circuit.as_ref().map(|circuit| circuit.sim_addr)?;
        self.objects.get(&root)?.get(&local_id)
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
        region_handle: u64,
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
        let root = self.circuit.as_ref().map(|circuit| circuit.sim_addr)?;
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
    pub fn request_objects(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_request_multiple_objects(local_ids, now)?;
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
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_select(local_ids, now)?;
        Ok(())
    }

    /// Deselects objects previously selected with
    /// [`Session::request_object_properties`] (`ObjectDeselect`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn deselect_objects(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_deselect(local_ids, now)?;
        Ok(())
    }

    // Object interaction & editing (#17) -----------------------------------
    //
    // These act on the current (root) region; an object is named by its
    // region-local id (from [`Session::objects`] / an object event). Each sends
    // a single reliable message on the root circuit. Edit and rez operations
    // require the appropriate object/parcel permissions on the grid; the
    // simulator silently ignores a request the agent is not allowed to make.

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
    pub fn touch_object(&mut self, local_id: u32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        grab_offset: Vector,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        object_id: Uuid,
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
    pub fn degrab_object(&mut self, local_id: u32, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_degrab(local_id, now)?;
        Ok(())
    }

    /// Rezzes (creates) a new primitive described by `shape` (an `ObjectAdd`);
    /// `group_id` is the group the new object is set to (use [`Uuid::nil`] for
    /// none). The new object arrives as an [`Event::ObjectAdded`]. Build `shape`
    /// from [`PrimShape::cube`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn rez_object(
        &mut self,
        shape: &PrimShape,
        group_id: Uuid,
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
        local_ids: &[u32],
        offset: Vector,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_duplicate(local_ids, offset, group_id, now)?;
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
    pub fn delete_objects(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_delete(local_ids, now)?;
        Ok(())
    }

    /// Derezzes the objects `local_ids` (a `DeRezObject`) to `destination` (take
    /// to inventory, return, trash, …). `destination_id` is the target folder or
    /// task id (its meaning depends on `destination`); `transaction_id` is a
    /// caller-chosen id correlating any resulting inventory update; `group_id` is
    /// the active group (use [`Uuid::nil`] for none).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn derez_objects(
        &mut self,
        local_ids: &[u32],
        destination: DeRezDestination,
        destination_id: Uuid,
        transaction_id: Uuid,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_derez_object(
            local_ids,
            destination,
            destination_id,
            transaction_id,
            group_id,
            now,
        )?;
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
        local_id: u32,
        transform: &ObjectTransform,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
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
        local_id: u32,
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
        local_id: u32,
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
        local_id: u32,
        name: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        description: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        action: ClickAction,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        material: Material,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        flags: &ObjectFlagSettings,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_flag_update(local_id, flags, now)?;
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
        local_ids: &[u32],
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_group(local_ids, group_id, now)?;
        Ok(())
    }

    /// Sets or clears `mask` permission bits of the `field` mask on the objects
    /// `local_ids` (an `ObjectPermissions`). The `mask` bits are the LSL
    /// `PERM_*` permission flags (`PERM_COPY`, `PERM_MODIFY`, `PERM_TRANSFER`,
    /// …); `set` adds them when true and removes them when false.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_object_permissions(
        &mut self,
        local_ids: &[u32],
        field: PermissionField,
        set: bool,
        mask: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_permissions(local_ids, field, set, mask, now)?;
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
        local_id: u32,
        sale_type: SaleType,
        sale_price: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        category: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
        local_id: u32,
        include: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
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
    pub fn link_objects(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_link(local_ids, now)?;
        Ok(())
    }

    /// Unlinks the objects `local_ids` from their linksets (an `ObjectDelink`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn delink_objects(&mut self, local_ids: &[u32], now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_object_delink(local_ids, now)?;
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
        region_handle: u64,
        position: Vector,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Active) {
            return Err(Error::NotActive);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_teleport_location_request(region_handle, position, look_at, now)?;
        circuit.timers.teleport = Some(deadline(now, TELEPORT_TIMEOUT));
        self.teleport_target = Some(region_handle);
        self.state = SessionState::Teleporting;
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
