//! The sans-I/O session state machine: login, circuit establishment,
//! keep-alive, and clean logout, driven entirely by passed-in time.

use crate::types::{
    AssetType, Camera, Diagnostic, Event, ImageCodec, InventoryFolder, InventoryItem, LoginAccount,
    LoginParams, Object, TerrainPatch, Throttle,
};
use sl_types::lsl::Rotation;
use sl_wire::ControlFlags;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// How often an `AgentUpdate` is sent to keep the agent active.
const AGENT_UPDATE_INTERVAL: Duration = Duration::from_millis(1000);
/// How long owed acknowledgements may wait before being flushed as a `PacketAck`.
const ACK_FLUSH_DELAY: Duration = Duration::from_millis(150);
/// How long without inbound traffic before the link is considered dead. Kept
/// well under OpenSim's 60-second `AckTimeout`.
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(45);
/// How long to wait for a `LogoutReply` before giving up on a clean logout.
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(5);
/// The retransmission timeout for an unacknowledged reliable packet.
const RESEND_TIMEOUT: Duration = Duration::from_millis(1500);
/// The maximum number of times a reliable packet is sent before giving up.
const MAX_RESEND_ATTEMPTS: u32 = 6;
/// The maximum number of inbound reliable sequence numbers remembered for
/// duplicate suppression.
const SEEN_CAPACITY: usize = 4096;
/// The maximum number of acknowledgements packed into a single `PacketAck`.
const MAX_ACKS_PER_PACKET: usize = 255;
/// How long to wait for a `TeleportFinish` before declaring the teleport failed.
const TELEPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// The default draw distance (metres) advertised in keep-alive `AgentUpdate`s,
/// large enough that the simulator enables the neighbouring regions.
const DEFAULT_DRAW_DISTANCE: f32 = 256.0;
/// The world-map layer flag the viewer sends on map name/item requests (the
/// terrain layer; `LAYER_FLAG` in the reference viewer).
const MAP_LAYER_FLAG: u32 = 2;
/// The identity (no-op) rotation: the default body/head facing.
pub(crate) const IDENTITY_ROTATION: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// The HTTP capability for fetching inventory folder contents (a POST of an LLSD
/// folder list). Used as the seed capability name, the request cap, and the
/// message tag a driver feeds back via [`Session::handle_caps_event`].
pub const CAP_FETCH_INVENTORY: &str = "FetchInventoryDescendents2";

/// The HTTP capability for fetching a group's full member roster (a POST of an
/// LLSD `{ group_id }` map — the modern Second Life path that replaces the UDP
/// `GroupMembersRequest`/`Reply`). The LLSD response is decoded by
/// [`Session::handle_caps_event`] into [`Event::GroupMembers`].
pub const CAP_GROUP_MEMBER_DATA: &str = "GroupMemberData";

/// The HTTP capability for fetching a texture by UUID (an HTTP `GET` of
/// `?texture_id=<uuid>`, returning a `.j2c` codestream). The modern Second Life
/// path that replaces the legacy UDP `RequestImage`/`ImageData` stream; the
/// driver fetches it and surfaces an [`Event::TextureReceived`].
pub const CAP_GET_TEXTURE: &str = "GetTexture";

/// The HTTP capability for fetching a mesh asset by UUID (an HTTP `GET` of
/// `?mesh_id=<uuid>`). Surfaces as an [`Event::AssetReceived`].
pub const CAP_GET_MESH: &str = "GetMesh";

/// The newer HTTP capability for fetching a mesh asset by UUID, preferred over
/// [`CAP_GET_MESH`] when offered.
pub const CAP_GET_MESH2: &str = "GetMesh2";

/// The HTTP capability for fetching a generic asset by UUID and class (an HTTP
/// `GET` of `?<class>_id=<uuid>`, e.g. `?sound_id=`/`?animatn_id=`). The modern
/// path that replaces the legacy UDP `TransferRequest` for many asset classes;
/// surfaces as an [`Event::AssetReceived`].
pub const CAP_GET_ASSET: &str = "GetAsset";

/// The HTTP capability for the modern Second Life **server-side appearance bake**
/// ("Sunshine" / central baking): a POST of an LLSD `{ "cof_version": <int> }`
/// map asking the grid's bake service to composite the agent's current outfit.
/// On a baking-capable region the client no longer computes or uploads baked
/// textures itself (the legacy `AgentSetAppearance` / `UploadBakedTexture`
/// path); it manages the Current Outfit Folder in inventory and triggers this
/// capability, after which the server broadcasts the resulting baked-texture ids
/// to every viewer via the UDP `AvatarAppearance` ([`Event::AvatarAppearance`]).
/// The POST's own LLSD reply (`{ success, error?, expected? }`) is surfaced as
/// [`Event::ServerAppearanceUpdate`]. Driven by the runtimes'
/// `RequestServerAppearanceUpdate` command (an HTTP POST, like the inventory
/// and group-roster capabilities), whose LLSD reply is decoded by
/// [`Session::handle_caps_event`].
pub const CAP_UPDATE_AVATAR_APPEARANCE: &str = "UpdateAvatarAppearance";

/// The HTTP capability for the modern asset upload: storing a new asset **and**
/// creating an inventory item for it (`NewFileAgentInventory`). A two-step
/// uploader — the driver POSTs the LLSD metadata (folder, asset/inventory type,
/// name, permissions, expected cost) and receives an `uploader` URL, then POSTs
/// the raw asset bytes there and receives `{ new_asset, new_inventory_item }`.
/// Surfaced as [`Event::AssetUploaded`] (or [`Event::AssetUploadFailed`]).
pub const CAP_NEW_FILE_AGENT_INVENTORY: &str = "NewFileAgentInventory";

/// The HTTP capability for uploading a client-computed **baked avatar texture**
/// (`UploadBakedTexture`): the legacy (pre-server-side-bake) appearance path.
/// Same two-step uploader as [`CAP_NEW_FILE_AGENT_INVENTORY`] but the metadata
/// POST is an empty map and the result is a *temporary* asset with no inventory
/// item (`new_inventory_item` is nil → `None`).
pub const CAP_UPLOAD_BAKED_TEXTURE: &str = "UploadBakedTexture";

/// The HTTP capability for replacing the asset of an existing **gesture**
/// inventory item (`UpdateGestureAgentInventory`). Two-step uploader; the
/// metadata POST carries the `item_id`. See also
/// [`AssetType::update_item_cap`](crate::AssetType::update_item_cap) for the
/// notecard / script / settings equivalents.
pub const CAP_UPDATE_GESTURE_AGENT_INVENTORY: &str = "UpdateGestureAgentInventory";

/// The HTTP capability for replacing the asset of an existing **notecard**
/// inventory item (`UpdateNotecardAgentInventory`). Two-step uploader carrying
/// the `item_id`.
pub const CAP_UPDATE_NOTECARD_AGENT_INVENTORY: &str = "UpdateNotecardAgentInventory";

/// The HTTP capability for replacing the asset of an existing **LSL script**
/// inventory item (`UpdateScriptAgent`). Two-step uploader carrying the
/// `item_id`.
pub const CAP_UPDATE_SCRIPT_AGENT: &str = "UpdateScriptAgent";

/// The HTTP capability for replacing the asset of an existing **settings**
/// inventory item (`UpdateSettingsAgentInventory`). Two-step uploader carrying
/// the `item_id`.
pub const CAP_UPDATE_SETTINGS_AGENT_INVENTORY: &str = "UpdateSettingsAgentInventory";

/// The HTTP capability for the **media-on-a-prim** read/write surface
/// (`ObjectMedia`): a POST of a `{ verb, object_id, … }` map. A `GET` verb asks
/// for an object's current per-face media (the simulator replies with an
/// `object_media_data` array decoded into [`Event::ObjectMedia`]); an `UPDATE`
/// verb sets it. Driven by the runtimes' `RequestObjectMedia` /
/// `SetObjectMedia` commands; the GET reply is decoded by
/// [`Session::handle_caps_event`].
pub const CAP_OBJECT_MEDIA: &str = "ObjectMedia";

/// The HTTP capability for navigating the media on a single prim face to a new
/// URL (`ObjectMediaNavigate`): a POST of a `{ object_id, current_url,
/// texture_index }` map. Driven by the runtimes' `NavigateObjectMedia` command;
/// the simulator advances the object's media version (visible on a subsequent
/// [`CAP_OBJECT_MEDIA`] GET) rather than replying with media data.
pub const CAP_OBJECT_MEDIA_NAVIGATE: &str = "ObjectMediaNavigate";

/// The HTTP capability for the legacy (normal/specular) **materials** surface
/// (`RenderMaterials`): a POST of a `{ "Zipped": <binary> }` map whose binary is
/// the zlib-compressed binary-LLSD array of the material ids to fetch. The
/// simulator replies with the matching materials (decoded into
/// [`Event::RenderMaterials`]). This is the path stock OpenSim implements.
/// Driven by the runtimes' `RequestRenderMaterials` command.
pub const CAP_RENDER_MATERIALS: &str = "RenderMaterials";

/// The HTTP capability for setting **GLTF (PBR) materials** on object faces
/// (`ModifyMaterialParams`): a POST of an array of `{ object_id, side,
/// gltf_json?, asset_id? }` maps. Driven by the runtimes' `ModifyMaterialParams`
/// command; the `{ success, message }` reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::MaterialParamsResult`].
pub const CAP_MODIFY_MATERIAL_PARAMS: &str = "ModifyMaterialParams";

/// The HTTP capability for replacing the asset of an existing **material**
/// inventory item (`UpdateMaterialAgentInventory`). Two-step uploader carrying
/// the `item_id` (see [`AssetType::update_item_cap`](crate::AssetType::update_item_cap)).
pub const CAP_UPDATE_MATERIAL_AGENT_INVENTORY: &str = "UpdateMaterialAgentInventory";

/// The HTTP capability for obtaining voice-chat account credentials
/// (`ProvisionVoiceAccountRequest`): a POST that returns either the legacy Vivox
/// SIP account (`{ username, password, voice_sip_uri_hostname,
/// voice_account_server_name }`) or, for the modern WebRTC path, a JSEP answer
/// SDP plus a viewer session. Driven by the runtimes' `RequestVoiceAccount`
/// command; the reply is decoded by [`Session::handle_caps_event`] into
/// [`Event::VoiceAccountProvisioned`]. Only the *signalling* is handled — the
/// audio transport (Vivox SIP/RTP or a WebRTC peer connection) is out of scope.
pub const CAP_PROVISION_VOICE_ACCOUNT: &str = "ProvisionVoiceAccountRequest";

/// The HTTP capability for a parcel's voice channel (`ParcelVoiceInfoRequest`):
/// a POST (empty body) that returns `{ parcel_local_id, region_name,
/// voice_credentials: { channel_uri } }`. Driven by the runtimes'
/// `RequestParcelVoiceInfo` command; the reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::ParcelVoiceInfo`].
pub const CAP_PARCEL_VOICE_INFO: &str = "ParcelVoiceInfoRequest";

/// The HTTP capability for WebRTC ICE-candidate trickling
/// (`VoiceSignalingRequest`): a fire-and-forget POST of the gathered candidates
/// (or an end-of-gathering marker) keyed by the viewer session from the
/// provision reply. Driven by the runtimes' `SendVoiceSignaling` command; the
/// simulator returns only an HTTP status, so there is no surfaced event. WebRTC
/// only (Vivox does not use it).
pub const CAP_VOICE_SIGNALING: &str = "VoiceSignalingRequest";

/// The HTTP capability for batch experience-metadata lookup (`GetExperienceInfo`):
/// a GET of `…/id/?page_size=N&public_id=<id>&…` returning `{ experience_keys,
/// error_ids }`. Driven by the runtimes' `RequestExperienceInfo` command; the
/// reply is decoded by [`Session::handle_caps_event`] into [`Event::ExperienceInfo`].
/// Experiences are a Second Life feature (stock OpenSim ships no module).
pub const CAP_GET_EXPERIENCE_INFO: &str = "GetExperienceInfo";

/// The HTTP capability for searching experiences by name (`FindExperienceByName`):
/// a GET of `…?page=N&page_size=M&query=<text>` returning `{ experience_keys }`.
/// Driven by `FindExperiences`; decoded into [`Event::ExperienceSearchResults`].
pub const CAP_FIND_EXPERIENCE_BY_NAME: &str = "FindExperienceByName";

/// The HTTP capability for the agent's admitted/blocked experiences
/// (`GetExperiences`): a GET returning `{ experiences, blocked }`. Driven by
/// `RequestExperiencePermissions`; decoded into [`Event::ExperiencePermissions`].
pub const CAP_GET_EXPERIENCES: &str = "GetExperiences";

/// The HTTP capability for the experiences the agent owns (`AgentExperiences`): a
/// GET returning `{ experience_ids }`. Driven by `RequestOwnedExperiences`;
/// decoded into [`Event::OwnedExperiences`].
pub const CAP_AGENT_EXPERIENCES: &str = "AgentExperiences";

/// The HTTP capability for the experiences the agent administers
/// (`GetAdminExperiences`): a GET returning `{ experience_ids }`. Driven by
/// `RequestAdminExperiences`; decoded into [`Event::AdminExperiences`].
pub const CAP_GET_ADMIN_EXPERIENCES: &str = "GetAdminExperiences";

/// The HTTP capability for the experiences the agent created
/// (`GetCreatorExperiences`): a GET returning `{ experience_ids }`. Driven by
/// `RequestCreatorExperiences`; decoded into [`Event::CreatorExperiences`].
pub const CAP_GET_CREATOR_EXPERIENCES: &str = "GetCreatorExperiences";

/// The HTTP capability for the experiences a group owns (`GroupExperiences`): a
/// GET of `…?<group_id>` returning `{ experience_ids }`. Driven by
/// `RequestGroupExperiences`; the runtime tags the reply with the queried group
/// to build [`Event::GroupExperiences`].
pub const CAP_GROUP_EXPERIENCES: &str = "GroupExperiences";

/// The HTTP capability for the agent's per-experience preferences
/// (`ExperiencePreferences`): an `Allow`/`Block` PUT of `{ "<id>": { permission }
/// }`, or a `Forget` DELETE of `…?<id>`; both reply `{ experiences, blocked }`.
/// Driven by `SetExperiencePermission`; decoded into [`Event::ExperiencePermissions`].
pub const CAP_EXPERIENCE_PREFERENCES: &str = "ExperiencePreferences";

/// The HTTP capability testing whether the agent is an admin of an experience
/// (`IsExperienceAdmin`): a GET of `…?experience_id=<id>` returning `{ status }`.
/// Driven by `RequestExperienceAdmin`; the runtime tags the reply with the queried
/// experience to build [`Event::ExperienceAdminStatus`].
pub const CAP_IS_EXPERIENCE_ADMIN: &str = "IsExperienceAdmin";

/// The HTTP capability testing whether the agent contributes to an experience
/// (`IsExperienceContributor`): a GET of `…?experience_id=<id>` returning `{
/// status }`. Driven by `RequestExperienceContributor`; the runtime tags the
/// reply with the queried experience to build [`Event::ExperienceContributorStatus`].
pub const CAP_IS_EXPERIENCE_CONTRIBUTOR: &str = "IsExperienceContributor";

/// The HTTP capability for editing an experience's metadata (`UpdateExperience`):
/// a POST of the editable fields returning the updated experience. Driven by
/// `UpdateExperience`; decoded into [`Event::ExperienceUpdated`].
pub const CAP_UPDATE_EXPERIENCE: &str = "UpdateExperience";

/// The HTTP capability for the region's experience allow/block/trust lists
/// (`RegionExperiences`): a GET to read, or a POST of `{ allowed, blocked,
/// trusted }` to update (estate-gated); both reply with the three lists. Driven by
/// `RequestRegionExperiences` / `SetRegionExperiences`; decoded into
/// [`Event::RegionExperiences`].
pub const CAP_REGION_EXPERIENCES: &str = "RegionExperiences";

/// Completing the IM surface (#28): the modern Second Life capability that
/// returns the agent's stored offline instant messages as an LLSD array (the
/// legacy path is the UDP `RetrieveInstantMessages` trigger). GET; decoded into
/// one [`Event::InstantMessageReceived`] per stored message.
pub const CAP_READ_OFFLINE_MSGS: &str = "ReadOfflineMsgs";

/// Inventory mutation (#30): the modern Second Life **AIS3** REST inventory
/// capability (`InventoryAPIv3`). Folder/item create/update/move/remove are HTTP
/// verbs against path suffixes under this base URL (see `sl_wire::inventory`).
/// Served only by Second Life; stock OpenSim ships no AIS3 cap. Replies are
/// decoded by [`Session::handle_caps_event`] into [`Event::InventoryBulkUpdate`].
pub const CAP_INVENTORY_API_V3: &str = "InventoryAPIv3";

/// Inventory mutation (#30): the AIS3 capability for the *library* inventory
/// (read-only, same REST shape as [`CAP_INVENTORY_API_V3`]). Second-Life only.
pub const CAP_LIBRARY_API_V3: &str = "LibraryAPIv3";

/// Inventory mutation (#30): the `CreateInventoryCategory` capability — a folder
/// create that returns a synchronous `{ folder_id, name, parent_id, type }`
/// reply (unlike the no-reply UDP `CreateInventoryFolder`). Served by **both**
/// OpenSim and Second Life. Decoded into [`Event::InventoryBulkUpdate`].
pub const CAP_CREATE_INVENTORY_CATEGORY: &str = "CreateInventoryCategory";

/// The viewer's `TELEPORT_FLAGS_VIA_LURE` (`1 << 2`), sent in a
/// `TeleportLureRequest` when accepting a teleport lure (#28).
const TELEPORT_FLAGS_VIA_LURE: u32 = 4;

/// The capability names the client requests from the region seed. A driver POSTs
/// these to the seed URL to obtain the capability map, then uses `EventQueueGet`
/// for the event-queue long-poll, [`CAP_FETCH_INVENTORY`] for inventory fetches,
/// [`CAP_GROUP_MEMBER_DATA`] for group rosters, the asset/texture/mesh caps
/// ([`CAP_GET_TEXTURE`], [`CAP_GET_MESH`], [`CAP_GET_MESH2`], [`CAP_GET_ASSET`])
/// for the HTTP asset-fetch pipeline, and the upload caps
/// ([`CAP_NEW_FILE_AGENT_INVENTORY`], [`CAP_UPLOAD_BAKED_TEXTURE`], and the
/// `Update*AgentInventory` family) for the HTTP asset-upload pipeline.
pub const REQUESTED_CAPABILITIES: &[&str] = &[
    "EventQueueGet",
    CAP_FETCH_INVENTORY,
    CAP_GROUP_MEMBER_DATA,
    CAP_GET_TEXTURE,
    CAP_GET_MESH,
    CAP_GET_MESH2,
    CAP_GET_ASSET,
    CAP_UPDATE_AVATAR_APPEARANCE,
    CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE,
    CAP_RENDER_MATERIALS,
    CAP_MODIFY_MATERIAL_PARAMS,
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_PROVISION_VOICE_ACCOUNT,
    CAP_PARCEL_VOICE_INFO,
    CAP_VOICE_SIGNALING,
    CAP_GET_EXPERIENCE_INFO,
    CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_EXPERIENCES,
    CAP_AGENT_EXPERIENCES,
    CAP_GET_ADMIN_EXPERIENCES,
    CAP_GET_CREATOR_EXPERIENCES,
    CAP_GROUP_EXPERIENCES,
    CAP_EXPERIENCE_PREFERENCES,
    CAP_IS_EXPERIENCE_ADMIN,
    CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_UPDATE_EXPERIENCE,
    CAP_REGION_EXPERIENCES,
    CAP_READ_OFFLINE_MSGS,
    CAP_INVENTORY_API_V3,
    CAP_LIBRARY_API_V3,
    CAP_CREATE_INVENTORY_CATEGORY,
];

/// The maximum UDP datagram size an I/O driver should be prepared to receive.
///
/// Sized at the theoretical IPv4/UDP payload maximum (64 KiB) so a driver's
/// receive buffer never truncates an inbound datagram.
pub const RECV_BUFFER_SIZE: usize = 0x1_0000;

/// Computes `now + duration`, saturating at `now` on (impossible) overflow.
fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

/// Updates `earliest` to the minimum of itself and `candidate`.
fn merge_deadline(earliest: &mut Option<Instant>, candidate: Option<Instant>) {
    if let Some(candidate) = candidate {
        *earliest = Some(match *earliest {
            Some(current) => current.min(candidate),
            None => candidate,
        });
    }
}

/// A reliable packet awaiting acknowledgement.
#[derive(Debug, Clone)]
struct UnackedPacket {
    /// The fully encoded datagram, ready to resend.
    datagram: Vec<u8>,
    /// When the packet was last sent.
    sent_at: Instant,
    /// How many times the packet has been sent so far.
    attempts: u32,
}

/// A bounded set of recently seen inbound reliable sequence numbers, used to
/// suppress duplicate processing of retransmitted reliable packets.
#[derive(Debug, Default)]
struct SeenWindow {
    /// Membership set for O(1) lookup.
    set: HashSet<u32>,
    /// Insertion order, for evicting the oldest entries.
    order: VecDeque<u32>,
}

impl SeenWindow {
    /// Records `sequence`; returns `true` if it was not seen before.
    fn insert(&mut self, sequence: u32) -> bool {
        if !self.set.insert(sequence) {
            return false;
        }
        self.order.push_back(sequence);
        if self.order.len() > SEEN_CAPACITY
            && let Some(evicted) = self.order.pop_front()
        {
            self.set.remove(&evicted);
        }
        true
    }
}

/// The per-connection timers, expressed as absolute deadlines.
#[derive(Debug)]
struct Timers {
    /// When the link is declared dead for lack of inbound traffic.
    inactivity: Instant,
    /// When to flush owed acknowledgements, if any are pending.
    ack_flush: Option<Instant>,
    /// When to send the next `AgentUpdate`, once the session is active.
    agent_update: Option<Instant>,
    /// When to give up waiting for a `LogoutReply`, once logging out.
    logout: Option<Instant>,
    /// When to give up waiting for a `TeleportFinish`, once teleporting.
    teleport: Option<Instant>,
}

/// An in-flight legacy UDP texture download (`RequestImage` →
/// `ImageData`/`ImagePacket`). The first packet (`ImageData`) carries the codec,
/// total size and packet count plus packet 0's data; subsequent `ImagePacket`s
/// carry packets `1..`. Packets are buffered by index so an out-of-order arrival
/// still reassembles correctly.
#[derive(Debug)]
struct TextureDownload {
    /// The codec reported by the `ImageData` header.
    codec: ImageCodec,
    /// The total number of packets, from the `ImageData` header.
    packets: u16,
    /// The received packet payloads, keyed by packet index (0 = `ImageData`).
    chunks: BTreeMap<u16, Vec<u8>>,
}

impl TextureDownload {
    /// Whether every packet `0..packets` has been received.
    fn is_complete(&self) -> bool {
        usize::from(self.packets) == self.chunks.len()
    }

    /// Concatenates the buffered packets in index order into the full encoded
    /// image bytes.
    fn assemble(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for chunk in self.chunks.values() {
            data.extend_from_slice(chunk);
        }
        data
    }
}

/// An in-flight generic asset transfer (`TransferRequest` →
/// `TransferInfo`/`TransferPacket`). The `TransferInfo` reply gives the total
/// size; each `TransferPacket` carries an in-order chunk and a status (the last
/// one is `LLTS_DONE`).
#[derive(Debug)]
struct AssetTransfer {
    /// The requested asset id (for the surfaced event).
    asset_id: Uuid,
    /// The requested asset class (for the surfaced event).
    asset_type: AssetType,
    /// The received packet payloads, keyed by packet index, reassembled in
    /// order once the transfer completes.
    chunks: BTreeMap<i32, Vec<u8>>,
}

impl AssetTransfer {
    /// Concatenates the buffered packets in index order into the full asset
    /// bytes.
    fn assemble(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for chunk in self.chunks.values() {
            data.extend_from_slice(chunk);
        }
        data
    }
}

/// The maximum asset payload (bytes) inlined directly in an `AssetUploadRequest`.
/// Larger assets are streamed over the `Xfer` path: the request is sent with an
/// empty `AssetData`, the simulator replies with a `RequestXfer`, and the client
/// streams the bytes in [`XFER_CHUNK`]-sized `SendXferPacket`s. Kept well under
/// the UDP MTU so the whole request fits in one datagram.
const MAX_INLINE_ASSET: usize = 1200;

/// The asset-data payload (bytes) carried in each upload `SendXferPacket`. The
/// first packet additionally carries a 4-byte little-endian length prefix, which
/// the simulator strips. Sized to stay within the UDP MTU.
const XFER_CHUNK: usize = 1000;

/// An in-flight legacy UDP asset upload (`AssetUploadRequest` →, for a large
/// asset, `RequestXfer` → `SendXferPacket`/`ConfirmXferPacket` → ...). Keyed by
/// the predicted asset id (`combine(transaction_id, secure_session_id)`), which
/// the simulator echoes as the `RequestXfer`'s `VFileID`. For an inlined asset
/// the bytes travel in the request itself and no `Xfer` follows; this record is
/// kept only so [`Event::AssetUploadComplete`] can name the asset class.
#[derive(Debug)]
struct AssetUpload {
    /// The full asset bytes to stream (empty once inlined in the request — the
    /// terminating `AssetUploadComplete` carries the asset class and id).
    data: Vec<u8>,
    /// The number of `SendXferPacket`s already sent (the next packet's sequence).
    sent: u32,
}

impl AssetUpload {
    /// The total number of `Xfer` packets needed to send [`data`](Self::data),
    /// at least one (an empty trailing packet is never sent — the data is
    /// chunked, and a final partial or full chunk carries the last-packet flag).
    fn packet_count(&self) -> u32 {
        let chunks = self.data.len().div_ceil(XFER_CHUNK).max(1);
        u32::try_from(chunks).unwrap_or(u32::MAX)
    }

    /// Builds the `Data` field for packet `sequence`: the chunk of [`data`](Self::data)
    /// at that index, with packet 0 prefixed by the 4-byte little-endian total
    /// asset length the simulator expects.
    fn packet_data(&self, sequence: u32) -> Vec<u8> {
        let start = usize::try_from(sequence)
            .unwrap_or(usize::MAX)
            .saturating_mul(XFER_CHUNK);
        let end = start.saturating_add(XFER_CHUNK).min(self.data.len());
        let chunk = self.data.get(start..end).unwrap_or_default();
        let mut out = Vec::with_capacity(chunk.len().saturating_add(4));
        if sequence == 0 {
            // The first packet carries the total asset length as a 4-byte
            // little-endian prefix (the simulator strips it). Packed by hand: the
            // `to_le_bytes` helper is denied by the `little_endian_bytes` lint.
            let len = u32::try_from(self.data.len()).unwrap_or(u32::MAX);
            out.push(u8::try_from(len & 0xff).unwrap_or(0));
            out.push(u8::try_from((len >> 8) & 0xff).unwrap_or(0));
            out.push(u8::try_from((len >> 16) & 0xff).unwrap_or(0));
            out.push(u8::try_from((len >> 24) & 0xff).unwrap_or(0));
        }
        out.extend_from_slice(chunk);
        out
    }
}

/// The UDP circuit to a single simulator.
#[derive(Debug)]
struct Circuit {
    /// The simulator's UDP address.
    sim_addr: SocketAddr,
    /// The agent/avatar id.
    agent_id: Uuid,
    /// The session id.
    session_id: Uuid,
    /// The circuit code.
    code: u32,
    /// The next outgoing sequence number.
    next_sequence: u32,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<u32>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<u32, UnackedPacket>,
    /// Recently seen inbound reliable sequence numbers.
    seen: SeenWindow,
    /// Datagrams ready to be transmitted.
    out: VecDeque<Vec<u8>>,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: f32,
    /// The connection timers.
    timers: Timers,
}

/// The lifecycle state of a [`Session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Constructed; the login request is available but not yet answered.
    New,
    /// Login succeeded; bootstrap packets sent, awaiting the region handshake.
    AwaitingHandshake,
    /// The region handshake completed; keep-alives are flowing.
    Active,
    /// A `TeleportLocationRequest` was sent; awaiting the `TeleportFinish`.
    Teleporting,
    /// A `LogoutRequest` was sent; awaiting the `LogoutReply`.
    LoggingOut,
    /// The session is finished.
    Closed,
}

/// Bookkeeping for an in-progress teleport handover, so the next
/// `RegionHandshake` is reported as a [`Event::RegionChanged`].
#[derive(Debug)]
struct HandoverPending {
    /// The destination region handle reported by `TeleportFinish`.
    region_handle: u64,
}

/// A single agent session: login bookkeeping plus one simulator circuit.
///
/// This is a pure state machine. Feed it bytes and the current [`Instant`] via
/// the `handle_*` methods; drain datagrams, timeouts, and events via the
/// `poll_*` methods. It performs no I/O and never reads a clock.
#[derive(Debug)]
pub struct Session {
    /// The login parameters.
    login: LoginParams,
    /// The current lifecycle state.
    state: SessionState,
    /// The active (root) circuit, once login has succeeded.
    circuit: Option<Circuit>,
    /// Child-agent circuits to neighbouring regions, keyed by simulator address.
    /// Opened from `EnableSimulator` so a neighbour already holds the agent's
    /// presence when the avatar crosses the border (promoted to root on
    /// `CrossedRegion`).
    children: BTreeMap<SocketAddr, Circuit>,
    /// The capability-seed URL for each child region (from the CAPS
    /// `EstablishAgentCommunication` event), keyed by simulator address; used as
    /// the new seed when a child is promoted to root.
    child_seeds: BTreeMap<SocketAddr, String>,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: f32,
    /// The agent control flags advertised in keep-alive `AgentUpdate`s; the
    /// simulator moves the agent accordingly.
    controls: ControlFlags,
    /// The desired bandwidth throttle (`AgentThrottle`), once the application
    /// has set one. Persisted so it can be re-sent on every region change (a new
    /// root circuit starts with the simulator's defaults until re-told).
    throttle: Option<Throttle>,
    /// The agent's body rotation (facing) sent in `AgentUpdate`s.
    body_rotation: Rotation,
    /// The agent's head rotation sent in `AgentUpdate`s.
    head_rotation: Rotation,
    /// The agent's camera viewpoint advertised in keep-alive `AgentUpdate`s on
    /// the root *and* every child circuit. Drives the simulator's interest list,
    /// so it follows where the agent looks rather than the region origin.
    /// Defaults to [`Camera::region_center`] until a client calls
    /// [`Session::set_camera`].
    camera: Camera,
    /// Set between an `AgentRequestSit` and the `AvatarSitResponse` that follows,
    /// so the response is completed with an `AgentSit`.
    sit_requested: bool,
    /// In-progress teleport handover bookkeeping, if any.
    handover: Option<HandoverPending>,
    /// The destination region handle of an in-flight teleport (between sending
    /// `TeleportLocationRequest` and receiving `TeleportFinish`/failure).
    teleport_target: Option<u64>,
    /// The current region's capability-seed URL (from login or a teleport), for
    /// the driver to fetch the CAPS map and event queue.
    seed_capability: Option<String>,
    /// The agent's inventory root ("My Inventory") folder id, from the login
    /// response.
    inventory_root: Option<Uuid>,
    /// Account-level facts from the login response (home, maturity, group limit,
    /// Library roots), or `None` before login.
    login_account: Option<LoginAccount>,
    /// In-flight mute-list file downloads (`Xfer` id → accumulated file bytes),
    /// started when a `MuteListUpdate` arrives.
    mute_xfers: BTreeMap<u64, Vec<u8>>,
    /// A monotonic counter for generating `Xfer` ids (never zero).
    next_xfer_id: u64,
    /// In-flight legacy UDP texture downloads, keyed by the texture's asset id
    /// (echoed in every `ImageData`/`ImagePacket`). Started by
    /// [`Session::request_texture`].
    texture_downloads: BTreeMap<Uuid, TextureDownload>,
    /// In-flight generic asset transfers, keyed by the client-generated
    /// transfer id (echoed in every `TransferInfo`/`TransferPacket`). Started by
    /// [`Session::request_asset`].
    asset_transfers: BTreeMap<Uuid, AssetTransfer>,
    /// A monotonic counter for generating asset transfer ids (each packed into a
    /// fresh `TransferID` UUID; never zero).
    next_transfer_id: u128,
    /// The agent's secure session id, from the login response. Combined with an
    /// upload's transaction id to predict the stored asset's UUID
    /// ([`combine_uuids`](sl_wire::combine_uuids)), so an upload's
    /// simulator-initiated `RequestXfer` (whose `VFileID` is that asset id) can be
    /// matched to its pending upload.
    secure_session_id: Uuid,
    /// In-flight legacy UDP asset uploads, keyed by the predicted asset id
    /// (`combine(transaction_id, secure_session_id)`). Started by
    /// [`Session::upload_asset_udp`]; removed on `AssetUploadComplete`.
    asset_uploads: BTreeMap<Uuid, AssetUpload>,
    /// Maps an active upload `Xfer` id (chosen by the simulator in its
    /// `RequestXfer`) to the predicted asset id keying [`asset_uploads`](Self::asset_uploads),
    /// so an inbound `ConfirmXferPacket` can find the upload to advance.
    upload_xfers: BTreeMap<u64, Uuid>,
    /// A monotonic counter for generating upload transaction ids (each packed
    /// into a fresh transaction UUID; never zero).
    next_upload_id: u128,
    /// The scene-graph object cache, keyed by the simulator the objects belong
    /// to (the root region *and* every child/neighbour circuit), then by
    /// region-local id. Region-local ids are only unique within a simulator, so
    /// the cache is partitioned per sim. A sim's objects are dropped when its
    /// circuit goes away (`DisableSimulator`, teleport handover, relogin).
    objects: BTreeMap<SocketAddr, BTreeMap<u32, Object>>,
    /// The decoded terrain cache, keyed by the simulator the patches belong to
    /// (the root region *and* every neighbour streamed over a child circuit),
    /// then by `(layer code, patch x, patch y)` so each layer's patches are kept
    /// side by side. Dropped with the rest of a sim's state when its circuit
    /// goes away. See [`Session::terrain_patches`] and [`Session::terrain_height`].
    terrain: BTreeMap<SocketAddr, BTreeMap<(u8, u32, u32), TerrainPatch>>,
    /// The region handle most recently learned for each simulator (from object
    /// updates, which carry it, and from `EnableSimulator`). Used to label
    /// terrain patches, which the `LayerData` message does not itself tag with a
    /// region handle.
    regions: BTreeMap<SocketAddr, u64>,
    /// The most recent raw `RegionData.TimeDilation` (a `u16`) seen for each
    /// simulator, used to de-duplicate [`Event::TimeDilation`] so it is emitted
    /// only when the region's frame time-dilation actually changes (every
    /// object-update message carries the field). See [`Session::note_time_dilation`].
    time_dilation: BTreeMap<SocketAddr, u16>,
    /// The live inventory-folder cache, keyed by folder id. Seeded from the
    /// login skeleton ([`Event::InventorySkeleton`]), grown by folder-contents
    /// fetches ([`Event::InventoryDescendents`], both UDP and CAPS) and the
    /// simulator's [`Event::InventoryBulkUpdate`] pushes, and kept current by the
    /// agent's own mutations. See [`Session::inventory_folder`].
    inventory_folders: BTreeMap<Uuid, InventoryFolder>,
    /// The live inventory-item cache, keyed by item id. Populated by
    /// folder-contents fetches and the simulator's
    /// [`Event::InventoryItemCreated`] / [`Event::InventoryBulkUpdate`] pushes,
    /// and kept current by the agent's own mutations. See
    /// [`Session::inventory_item`].
    inventory_items: BTreeMap<Uuid, InventoryItem>,
    /// A monotonic counter for the async `CallbackID` of inventory create/update
    /// requests, echoed back in the simulator's reply so a client can correlate.
    next_inventory_callback: u32,
    /// Pending high-level events for the driver.
    events: VecDeque<Event>,
    /// Whether protocol diagnostics are collected. Off by default so the
    /// silent-drop sites cost nothing (no raw-byte capture, no queueing) on the
    /// normal path. Toggled by [`Session::set_diagnostics`].
    diagnostics_enabled: bool,
    /// Pending [`Diagnostic`]s for the driver, populated only while
    /// `diagnostics_enabled`. Drained by [`Session::poll_diagnostic`].
    diagnostics: VecDeque<Diagnostic>,
}

mod circuit;
mod conversions;
mod methods;

pub(crate) use conversions::{ZERO_VECTOR, instant_message};
pub use conversions::{
    ais_inventory_update_to_llsd, build_map_block_reply, build_map_item_reply,
    bulk_update_inventory_to_llsd, chatterbox_invitation_to_llsd, created_category_to_llsd,
    crossed_region_to_caps_llsd, enable_simulator_to_caps_llsd,
    establish_agent_communication_to_llsd, group_members_to_caps_llsd,
    group_memberships_to_caps_llsd, inventory_descendents_to_llsd, offline_messages_to_llsd,
    parcel_info_to_llsd, server_appearance_update_to_llsd, teleport_finish_to_llsd,
};
