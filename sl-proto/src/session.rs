//! The sans-I/O session state machine: login, circuit establishment,
//! keep-alive, and clean logout, driven entirely by passed-in time.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use sl_types::lsl::{Rotation, Vector};
use sl_types::money::LindenAmount;
use sl_wire::messages::{
    AcceptFriendship, AcceptFriendshipAgentDataBlock, AcceptFriendshipFolderDataBlock,
    AcceptFriendshipTransactionBlockBlock, AgentRequestSit, AgentRequestSitAgentDataBlock,
    AgentRequestSitTargetObjectBlock, AgentSit, AgentSitAgentDataBlock, AgentThrottle,
    AgentThrottleAgentDataBlock, AgentThrottleThrottleBlock, AgentUpdate,
    AgentUpdateAgentDataBlock, AvatarGroupsReplyGroupDataBlock,
    AvatarInterestsReplyPropertiesDataBlock, AvatarPropertiesReplyPropertiesDataBlock,
    AvatarPropertiesRequest, AvatarPropertiesRequestAgentDataBlock, ChatFromSimulatorChatDataBlock,
    ChatFromViewer, ChatFromViewerAgentDataBlock, ChatFromViewerChatDataBlock,
    CompleteAgentMovement, CompleteAgentMovementAgentDataBlock, CompletePingCheck,
    CompletePingCheckPingIDBlock, DeclineFriendship, DeclineFriendshipAgentDataBlock,
    DeclineFriendshipTransactionBlockBlock, EnableSimulatorSimulatorInfoBlock,
    FetchInventoryDescendents, FetchInventoryDescendentsAgentDataBlock,
    FetchInventoryDescendentsInventoryDataBlock, GenericMessage, GenericMessageAgentDataBlock,
    GenericMessageMethodDataBlock, GenericMessageParamListBlock, GrantUserRights,
    GrantUserRightsAgentDataBlock, GrantUserRightsRightsBlock, ImprovedInstantMessage,
    ImprovedInstantMessageAgentDataBlock, ImprovedInstantMessageEstateBlockBlock,
    ImprovedInstantMessageMessageBlockBlock, InventoryDescendentsFolderDataBlock,
    InventoryDescendentsItemDataBlock, LogoutRequest, LogoutRequestAgentDataBlock,
    MapBlockReplyDataBlock, MapBlockReplySizeBlock, MapBlockRequest, MapBlockRequestAgentDataBlock,
    MapBlockRequestPositionDataBlock, MapItemRequest, MapItemRequestAgentDataBlock,
    MapItemRequestRequestDataBlock, MapNameRequest, MapNameRequestAgentDataBlock,
    MapNameRequestNameDataBlock, PacketAck, PacketAckPacketsBlock, ParcelPropertiesParcelDataBlock,
    ParcelPropertiesRequest, ParcelPropertiesRequestAgentDataBlock,
    ParcelPropertiesRequestParcelDataBlock, RegionHandshakeRegionInfo3Block,
    RegionHandshakeRegionInfoBlock, RegionHandshakeReply, RegionHandshakeReplyAgentDataBlock,
    RegionHandshakeReplyRegionInfoBlock, RegionInfoRegionInfo2Block, RegionInfoRegionInfoBlock,
    RequestRegionInfo, RequestRegionInfoAgentDataBlock, TeleportLocationRequest,
    TeleportLocationRequestAgentDataBlock, TeleportLocationRequestInfoBlock, TerminateFriendship,
    TerminateFriendshipAgentDataBlock, TerminateFriendshipExBlockBlock, UseCircuitCode,
    UseCircuitCodeCircuitCodeBlock,
};
// Script dialogs & permissions (#8): the outgoing reply messages.
use sl_wire::messages::{
    ScriptAnswerYes, ScriptAnswerYesAgentDataBlock, ScriptAnswerYesDataBlock, ScriptDialogReply,
    ScriptDialogReplyAgentDataBlock, ScriptDialogReplyDataBlock,
};
// Mute list (#9): the outgoing mute-edit messages and the Xfer download messages.
use sl_wire::messages::{
    ConfirmXferPacket, ConfirmXferPacketXferIDBlock, MuteListRequest,
    MuteListRequestAgentDataBlock, MuteListRequestMuteDataBlock, RemoveMuteListEntry,
    RemoveMuteListEntryAgentDataBlock, RemoveMuteListEntryMuteDataBlock, RequestXfer,
    RequestXferXferIDBlock, UpdateMuteListEntry, UpdateMuteListEntryAgentDataBlock,
    UpdateMuteListEntryMuteDataBlock,
};
// Money / economy (#11): the outgoing balance/economy/transfer requests.
use sl_wire::messages::{
    EconomyDataRequest, MoneyBalanceRequest, MoneyBalanceRequestAgentDataBlock,
    MoneyBalanceRequestMoneyDataBlock, MoneyTransferRequest, MoneyTransferRequestAgentDataBlock,
    MoneyTransferRequestMoneyDataBlock,
};
// Parcel management (#13): the outgoing land-edit / access / buy / return messages.
use sl_wire::messages::{
    ParcelAccessListRequest, ParcelAccessListRequestAgentDataBlock,
    ParcelAccessListRequestDataBlock, ParcelAccessListUpdate, ParcelAccessListUpdateAgentDataBlock,
    ParcelAccessListUpdateDataBlock, ParcelAccessListUpdateListBlock, ParcelBuy,
    ParcelBuyAgentDataBlock, ParcelBuyDataBlock, ParcelBuyParcelDataBlock, ParcelDeedToGroup,
    ParcelDeedToGroupAgentDataBlock, ParcelDeedToGroupDataBlock, ParcelDwellRequest,
    ParcelDwellRequestAgentDataBlock, ParcelDwellRequestDataBlock, ParcelPropertiesUpdate,
    ParcelPropertiesUpdateAgentDataBlock, ParcelPropertiesUpdateParcelDataBlock, ParcelReclaim,
    ParcelReclaimAgentDataBlock, ParcelReclaimDataBlock, ParcelRelease,
    ParcelReleaseAgentDataBlock, ParcelReleaseDataBlock, ParcelReturnObjects,
    ParcelReturnObjectsAgentDataBlock, ParcelReturnObjectsOwnerIDsBlock,
    ParcelReturnObjectsParcelDataBlock, ParcelReturnObjectsTaskIDsBlock, ParcelSelectObjects,
    ParcelSelectObjectsAgentDataBlock, ParcelSelectObjectsParcelDataBlock,
    ParcelSelectObjectsReturnIDsBlock,
};
// Object / scene graph (#16): incoming update blocks consumed by the decoders,
// and the outgoing select / cache-miss request messages.
use sl_wire::messages::{
    ObjectDeselect, ObjectDeselectAgentDataBlock, ObjectDeselectObjectDataBlock,
    ObjectPropertiesObjectDataBlock, ObjectSelect, ObjectSelectAgentDataBlock,
    ObjectSelectObjectDataBlock, ObjectUpdateObjectDataBlock, RequestMultipleObjects,
    RequestMultipleObjectsAgentDataBlock, RequestMultipleObjectsObjectDataBlock,
};
// Object interaction & editing (#17): the outgoing touch / rez / edit messages.
use sl_wire::messages::{
    DeRezObject, DeRezObjectAgentBlockBlock, DeRezObjectAgentDataBlock, DeRezObjectObjectDataBlock,
    MultipleObjectUpdate, MultipleObjectUpdateAgentDataBlock, MultipleObjectUpdateObjectDataBlock,
    ObjectAdd, ObjectAddAgentDataBlock, ObjectAddObjectDataBlock, ObjectCategory,
    ObjectCategoryAgentDataBlock, ObjectCategoryObjectDataBlock, ObjectClickAction,
    ObjectClickActionAgentDataBlock, ObjectClickActionObjectDataBlock, ObjectDeGrab,
    ObjectDeGrabAgentDataBlock, ObjectDeGrabObjectDataBlock, ObjectDelete,
    ObjectDeleteAgentDataBlock, ObjectDeleteObjectDataBlock, ObjectDelink,
    ObjectDelinkAgentDataBlock, ObjectDelinkObjectDataBlock, ObjectDescription,
    ObjectDescriptionAgentDataBlock, ObjectDescriptionObjectDataBlock, ObjectDuplicate,
    ObjectDuplicateAgentDataBlock, ObjectDuplicateObjectDataBlock, ObjectDuplicateSharedDataBlock,
    ObjectFlagUpdate, ObjectFlagUpdateAgentDataBlock, ObjectGrab, ObjectGrabAgentDataBlock,
    ObjectGrabObjectDataBlock, ObjectGrabUpdate, ObjectGrabUpdateAgentDataBlock,
    ObjectGrabUpdateObjectDataBlock, ObjectGroup, ObjectGroupAgentDataBlock,
    ObjectGroupObjectDataBlock, ObjectIncludeInSearch, ObjectIncludeInSearchAgentDataBlock,
    ObjectIncludeInSearchObjectDataBlock, ObjectLink, ObjectLinkAgentDataBlock,
    ObjectLinkObjectDataBlock, ObjectMaterial, ObjectMaterialAgentDataBlock,
    ObjectMaterialObjectDataBlock, ObjectName, ObjectNameAgentDataBlock, ObjectNameObjectDataBlock,
    ObjectPermissions, ObjectPermissionsAgentDataBlock, ObjectPermissionsHeaderDataBlock,
    ObjectPermissionsObjectDataBlock, ObjectSaleInfo, ObjectSaleInfoAgentDataBlock,
    ObjectSaleInfoObjectDataBlock,
};
// Asset & texture pipeline (#19): the outgoing image/transfer requests.
use sl_wire::messages::{
    RequestImage, RequestImageAgentDataBlock, RequestImageRequestImageBlock, TransferRequest,
    TransferRequestTransferInfoBlock,
};
// Asset upload (#23): the legacy UDP asset-upload request and the Xfer-send
// (upload-direction) packet.
use sl_wire::messages::{
    AssetUploadRequest, AssetUploadRequestAssetBlockBlock, SendXferPacket,
    SendXferPacketDataPacketBlock, SendXferPacketXferIDBlock,
};
// Avatar appearance & wearables (#20): the outgoing appearance/wearable messages.
use sl_wire::messages::{
    AgentCachedTexture, AgentCachedTextureAgentDataBlock, AgentCachedTextureWearableDataBlock,
    AgentIsNowWearing, AgentIsNowWearingAgentDataBlock, AgentIsNowWearingWearableDataBlock,
    AgentSetAppearance, AgentSetAppearanceAgentDataBlock, AgentSetAppearanceObjectDataBlock,
    AgentSetAppearanceVisualParamBlock, AgentSetAppearanceWearableDataBlock, AgentWearablesRequest,
    AgentWearablesRequestAgentDataBlock,
};
// Animations (#21): the outgoing trigger message.
use sl_wire::messages::{
    AgentAnimation, AgentAnimationAgentDataBlock, AgentAnimationAnimationListBlock,
    AgentAnimationPhysicalAvatarEventListBlock,
};
// Estate / region management (#14): the outgoing estate-owner / god messages.
use sl_wire::messages::{
    EstateOwnerMessage, EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
    EstateOwnerMessageParamListBlock, GodKickUser, GodKickUserUserInfoBlock, GodlikeMessage,
    GodlikeMessageAgentDataBlock, GodlikeMessageMethodDataBlock, GodlikeMessageParamListBlock,
};
// Group support (#7): incoming reply blocks consumed by the converter helpers.
use sl_wire::messages::{
    AgentDataUpdateAgentDataBlock, AgentGroupDataUpdateGroupDataBlock,
    GroupMembersReplyMemberDataBlock, GroupNoticesListReplyDataBlock,
    GroupProfileReplyGroupDataBlock, GroupRoleDataReplyRoleDataBlock,
    GroupTitlesReplyGroupDataBlock,
};
// Group support (#7): the outgoing group messages and their blocks.
use sl_wire::messages::{
    ActivateGroup, ActivateGroupAgentDataBlock, CreateGroupRequest,
    CreateGroupRequestAgentDataBlock, CreateGroupRequestGroupDataBlock, GroupMembersRequest,
    GroupMembersRequestAgentDataBlock, GroupMembersRequestGroupDataBlock, GroupNoticeRequest,
    GroupNoticeRequestAgentDataBlock, GroupNoticeRequestDataBlock, GroupNoticesListRequest,
    GroupNoticesListRequestAgentDataBlock, GroupNoticesListRequestDataBlock, GroupProfileRequest,
    GroupProfileRequestAgentDataBlock, GroupProfileRequestGroupDataBlock, GroupRoleDataRequest,
    GroupRoleDataRequestAgentDataBlock, GroupRoleDataRequestGroupDataBlock,
    GroupRoleMembersRequest, GroupRoleMembersRequestAgentDataBlock,
    GroupRoleMembersRequestGroupDataBlock, GroupTitlesRequest, GroupTitlesRequestAgentDataBlock,
    InviteGroupRequest, InviteGroupRequestAgentDataBlock, InviteGroupRequestGroupDataBlock,
    InviteGroupRequestInviteDataBlock, JoinGroupRequest, JoinGroupRequestAgentDataBlock,
    JoinGroupRequestGroupDataBlock, LeaveGroupRequest, LeaveGroupRequestAgentDataBlock,
    LeaveGroupRequestGroupDataBlock, SetGroupAcceptNotices, SetGroupAcceptNoticesAgentDataBlock,
    SetGroupAcceptNoticesDataBlock, SetGroupAcceptNoticesNewDataBlock, SetGroupContribution,
    SetGroupContributionAgentDataBlock, SetGroupContributionDataBlock,
};
use sl_wire::{
    AnyMessage, ControlFlags, Llsd, MessageId, PacketFlags, Reader, SkeletonFolder, WireError,
    Writer, build_login_request, encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::error::Error;
use crate::terrain;
use crate::types::{
    ActiveGroup, Asset, AssetType, AvatarGroupMembership, AvatarInterests, AvatarPick,
    AvatarProperties, ChatAudible, ChatMessage, ChatSourceType, ChatType, ClickAction,
    CreateGroupParams, DeRezDestination, DisconnectReason, EconomyData, EstateAccessDelta,
    EstateAccessKind, EstateInfo, Event, Friend, FriendRights, GroupMember, GroupMembership,
    GroupNotice, GroupProfile, GroupRole, GroupRoleMember, GroupTitle, ImDialog, ImageCodec,
    InstantMessage, InventoryFolder, InventoryItem, LoadUrlRequest, LoginHttpRequest, LoginParams,
    MapItem, MapItemType, MapRegionInfo, Material, Maturity, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MuteEntry, MuteFlags, MuteType, NeighborInfo, Object, ObjectFlagSettings,
    ObjectMotion, ObjectProperties, ObjectTransform, ParcelAccessEntry, ParcelAccessScope,
    ParcelInfo, ParcelOverlayInfo, ParcelReturnType, ParcelUpdate, PermissionField,
    PlayingAnimation, PrimShape, ProductType, RegionIdentity, RegionInfoUpdate, RegionLimits,
    Reliability, SaleType, ScriptDialog, ScriptPermissionRequest, ScriptPermissions,
    ScriptTeleportRequest, SoundFlags, SoundPreload, TerrainLayerType, TerrainPatch, Texture,
    Throttle, TransferStatus, Transmit, Wearable, WearableType, avatar_texture, grid_to_handle,
    handle_to_grid,
};
use crate::{appearance, types::AvatarAppearance, types::AvatarAttachment};

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
const IDENTITY_ROTATION: Rotation = Rotation {
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
];

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

impl Circuit {
    /// Creates a circuit and arms the inactivity timer.
    fn new(
        sim_addr: SocketAddr,
        agent_id: Uuid,
        session_id: Uuid,
        circuit_code: u32,
        draw_distance: f32,
        now: Instant,
    ) -> Self {
        Self {
            sim_addr,
            agent_id,
            session_id,
            code: circuit_code,
            next_sequence: 1,
            pending_acks: Vec::new(),
            unacked: BTreeMap::new(),
            seen: SeenWindow::default(),
            out: VecDeque::new(),
            draw_distance,
            timers: Timers {
                inactivity: deadline(now, INACTIVITY_TIMEOUT),
                ack_flush: None,
                agent_update: None,
                logout: None,
                teleport: None,
            },
        }
    }

    /// Re-points the circuit at a new simulator after a teleport, resetting the
    /// per-circuit sequence/ack/seen/timer state while keeping the agent
    /// identity and circuit code (both reused across regions).
    fn retarget(&mut self, sim_addr: SocketAddr, now: Instant) {
        self.sim_addr = sim_addr;
        self.next_sequence = 1;
        self.pending_acks.clear();
        self.unacked.clear();
        self.seen = SeenWindow::default();
        self.out.clear();
        self.timers = Timers {
            inactivity: deadline(now, INACTIVITY_TIMEOUT),
            ack_flush: None,
            agent_update: None,
            logout: None,
            teleport: None,
        };
    }

    /// Allocates the next outgoing sequence number.
    const fn next_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        sequence
    }

    /// Encodes and queues a message, tracking it for resend when reliable.
    fn send(
        &mut self,
        message: &AnyMessage,
        reliability: Reliability,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut writer = Writer::new();
        message.id().encode(&mut writer);
        message.encode_body(&mut writer)?;
        let body = writer.into_bytes();

        let sequence = self.next_sequence();
        let flags = match reliability {
            Reliability::Reliable => PacketFlags::RELIABLE,
            Reliability::Unreliable => PacketFlags::EMPTY,
        };
        let datagram = encode_datagram(flags, sequence, &body);

        if matches!(reliability, Reliability::Reliable) {
            self.unacked.insert(
                sequence,
                UnackedPacket {
                    datagram: datagram.clone(),
                    sent_at: now,
                    attempts: 1,
                },
            );
        }
        self.out.push_back(datagram);
        Ok(())
    }

    /// Queues `UseCircuitCode` reliably.
    fn send_use_circuit_code(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: self.code,
                session_id: self.session_id,
                id: self.agent_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentThrottle` reliably, telling the simulator how to allocate
    /// its UDP send bandwidth across the seven traffic categories. The seven
    /// per-category rates are packed as little-endian `f32` bits-per-second
    /// values (the `Throttle` wire encoding); `GenCounter` is left at zero, as
    /// the reference viewer does (the simulator does not order by it).
    fn send_agent_throttle(&mut self, throttle: &Throttle, now: Instant) -> Result<(), WireError> {
        let mut writer = Writer::new();
        for rate in throttle.bits_per_second() {
            writer.put_f32(rate);
        }
        let message = AnyMessage::AgentThrottle(AgentThrottle {
            agent_data: AgentThrottleAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                circuit_code: self.code,
            },
            throttle: AgentThrottleThrottleBlock {
                gen_counter: 0,
                throttles: writer.into_bytes(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues `CompleteAgentMovement` reliably.
    fn send_complete_agent_movement(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::CompleteAgentMovement(CompleteAgentMovement {
            agent_data: CompleteAgentMovementAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                circuit_code: self.code,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues `RegionHandshakeReply` reliably.
    fn send_region_handshake_reply(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::RegionHandshakeReply(RegionHandshakeReply {
            agent_data: RegionHandshakeReplyAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            region_info: RegionHandshakeReplyRegionInfoBlock { flags: 0 },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CompletePingCheck` reply unreliably.
    fn send_complete_ping_check(&mut self, ping_id: u8, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::CompletePingCheck(CompletePingCheck {
            ping_id: CompletePingCheckPingIDBlock { ping_id },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues a `ChatFromViewer` reliably, sending local chat. The wire string
    /// carries a trailing NUL, as a real viewer sends.
    fn send_chat_from_viewer(
        &mut self,
        message: &str,
        chat_type: ChatType,
        channel: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut bytes = message.as_bytes().to_vec();
        bytes.push(0);
        let message = AnyMessage::ChatFromViewer(ChatFromViewer {
            agent_data: ChatFromViewerAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            chat_data: ChatFromViewerChatDataBlock {
                message: bytes,
                r#type: chat_type.to_u8(),
                channel,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ImprovedInstantMessage` reliably to a single agent. The IM
    /// session id is the canonical `agent_id XOR to_agent_id` the viewer uses for
    /// 1:1 sessions; `from_group` is false and the binary bucket is empty (the
    /// shape of an ordinary direct IM or a typing notification). The wire strings
    /// carry trailing NULs, as a real viewer sends.
    fn send_instant_message_raw(
        &mut self,
        to_agent_id: Uuid,
        dialog: ImDialog,
        message: &str,
        from_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut name_bytes = from_name.as_bytes().to_vec();
        name_bytes.push(0);
        let mut message_bytes = message.as_bytes().to_vec();
        message_bytes.push(0);
        let message = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id,
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0, // IM_ONLINE
                dialog: dialog.to_u8(),
                id: compute_im_session_id(self.agent_id, to_agent_id),
                timestamp: 0,
                from_agent_name: name_bytes,
                message: message_bytes,
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentUpdate` unreliably carrying the given control flags and
    /// body/head rotation.
    ///
    /// The camera is placed at the region centre with an orthonormal basis and
    /// the configured draw distance, so the simulator builds an interest list
    /// and enables the neighbouring regions (which arrive as `EnableSimulator`).
    /// The simulator moves the agent according to `control_flags` in the
    /// direction of `body_rotation`.
    fn send_agent_update(
        &mut self,
        control_flags: u32,
        body_rotation: Rotation,
        head_rotation: Rotation,
        now: Instant,
    ) -> Result<(), WireError> {
        let camera_center = Vector {
            x: 128.0,
            y: 128.0,
            z: 30.0,
        };
        let camera_at_axis = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let camera_left_axis = Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        let camera_up_axis = Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let message = AnyMessage::AgentUpdate(AgentUpdate {
            agent_data: AgentUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                body_rotation,
                head_rotation,
                state: 0,
                camera_center,
                camera_at_axis,
                camera_left_axis,
                camera_up_axis,
                far: self.draw_distance,
                control_flags,
                flags: 0,
            },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues an `AgentRequestSit` reliably (ask to sit on `target` at `offset`).
    fn send_agent_request_sit(
        &mut self,
        target: Uuid,
        offset: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentRequestSit(AgentRequestSit {
            agent_data: AgentRequestSitAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            target_object: AgentRequestSitTargetObjectBlock {
                target_id: target,
                offset,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentSit` reliably (complete a sit after `AvatarSitResponse`).
    fn send_agent_sit(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentSit(AgentSit {
            agent_data: AgentSitAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GenericMessage` reliably with the given method and string
    /// parameters (used for the server-side `autopilot` walk-to command).
    fn send_generic_message(
        &mut self,
        method: &str,
        params: &[String],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GenericMessage(GenericMessage {
            agent_data: GenericMessageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                transaction_id: Uuid::nil(),
            },
            method_data: GenericMessageMethodDataBlock {
                method: method.as_bytes().to_vec(),
                invoice: Uuid::nil(),
            },
            param_list: params
                .iter()
                .map(|param| GenericMessageParamListBlock {
                    parameter: param.as_bytes().to_vec(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AvatarPropertiesRequest` reliably for the avatar `target`.
    fn send_avatar_properties_request(
        &mut self,
        target: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AvatarPropertiesRequest(AvatarPropertiesRequest {
            agent_data: AvatarPropertiesRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                avatar_id: target,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GrantUserRights` reliably, setting the rights this agent grants
    /// the friend `target` to `rights`.
    fn send_grant_user_rights(
        &mut self,
        target: Uuid,
        rights: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GrantUserRights(GrantUserRights {
            agent_data: GrantUserRightsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            rights: vec![GrantUserRightsRightsBlock {
                agent_related: target,
                related_rights: rights,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TerminateFriendship` reliably, ending the friendship with
    /// `other`.
    fn send_terminate_friendship(&mut self, other: Uuid, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::TerminateFriendship(TerminateFriendship {
            agent_data: TerminateFriendshipAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            ex_block: TerminateFriendshipExBlockBlock { other_id: other },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AcceptFriendship` reliably for the friendship-offer
    /// `transaction_id`, placing the new calling card in `folder`.
    fn send_accept_friendship(
        &mut self,
        transaction_id: Uuid,
        folder: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AcceptFriendship(AcceptFriendship {
            agent_data: AcceptFriendshipAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            transaction_block: AcceptFriendshipTransactionBlockBlock { transaction_id },
            folder_data: vec![AcceptFriendshipFolderDataBlock { folder_id: folder }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeclineFriendship` reliably for the friendship-offer
    /// `transaction_id`.
    fn send_decline_friendship(
        &mut self,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeclineFriendship(DeclineFriendship {
            agent_data: DeclineFriendshipAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            transaction_block: DeclineFriendshipTransactionBlockBlock { transaction_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ActivateGroup` reliably, making `group_id` the active group
    /// (nil clears the active group).
    fn send_activate_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ActivateGroup(ActivateGroup {
            agent_data: ActivateGroupAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupMembersRequest` reliably for `group_id`.
    fn send_group_members_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupMembersRequest(GroupMembersRequest {
            agent_data: GroupMembersRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupMembersRequestGroupDataBlock {
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleDataRequest` reliably for `group_id`.
    fn send_group_role_data_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleDataRequest(GroupRoleDataRequest {
            agent_data: GroupRoleDataRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupRoleDataRequestGroupDataBlock {
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleMembersRequest` reliably for `group_id`.
    fn send_group_role_members_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleMembersRequest(GroupRoleMembersRequest {
            agent_data: GroupRoleMembersRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupRoleMembersRequestGroupDataBlock {
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupTitlesRequest` reliably for `group_id`.
    fn send_group_titles_request(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::GroupTitlesRequest(GroupTitlesRequest {
            agent_data: GroupTitlesRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupProfileRequest` reliably for `group_id`.
    fn send_group_profile_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupProfileRequest(GroupProfileRequest {
            agent_data: GroupProfileRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupProfileRequestGroupDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupNoticesListRequest` reliably for `group_id`.
    fn send_group_notices_list_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupNoticesListRequest(GroupNoticesListRequest {
            agent_data: GroupNoticesListRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: GroupNoticesListRequestDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupNoticeRequest` reliably for the notice `notice_id`.
    fn send_group_notice_request(
        &mut self,
        notice_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupNoticeRequest(GroupNoticeRequest {
            agent_data: GroupNoticeRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: GroupNoticeRequestDataBlock {
                group_notice_id: notice_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CreateGroupRequest` reliably.
    fn send_create_group_request(
        &mut self,
        params: &CreateGroupParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CreateGroupRequest(CreateGroupRequest {
            agent_data: CreateGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: CreateGroupRequestGroupDataBlock {
                name: with_nul(&params.name),
                charter: with_nul(&params.charter),
                show_in_list: params.show_in_list,
                insignia_id: params.insignia_id,
                membership_fee: params.membership_fee,
                open_enrollment: params.open_enrollment,
                allow_publish: params.allow_publish,
                mature_publish: params.mature_publish,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `JoinGroupRequest` reliably for `group_id`.
    fn send_join_group_request(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::JoinGroupRequest(JoinGroupRequest {
            agent_data: JoinGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: JoinGroupRequestGroupDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LeaveGroupRequest` reliably for `group_id`.
    fn send_leave_group_request(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::LeaveGroupRequest(LeaveGroupRequest {
            agent_data: LeaveGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: LeaveGroupRequestGroupDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `InviteGroupRequest` reliably inviting `invitees` (each an
    /// `(invitee_id, role_id)` pair, nil `role_id` for the default Everyone role)
    /// to `group_id`.
    fn send_invite_group_request(
        &mut self,
        group_id: Uuid,
        invitees: &[(Uuid, Uuid)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::InviteGroupRequest(InviteGroupRequest {
            agent_data: InviteGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: InviteGroupRequestGroupDataBlock { group_id },
            invite_data: invitees
                .iter()
                .map(|(invitee_id, role_id)| InviteGroupRequestInviteDataBlock {
                    invitee_id: *invitee_id,
                    role_id: *role_id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetGroupAcceptNotices` reliably for `group_id`.
    fn send_set_group_accept_notices(
        &mut self,
        group_id: Uuid,
        accept_notices: bool,
        list_in_profile: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetGroupAcceptNotices(SetGroupAcceptNotices {
            agent_data: SetGroupAcceptNoticesAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: SetGroupAcceptNoticesDataBlock {
                group_id,
                accept_notices,
            },
            new_data: SetGroupAcceptNoticesNewDataBlock { list_in_profile },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetGroupContribution` reliably for `group_id`.
    fn send_set_group_contribution(
        &mut self,
        group_id: Uuid,
        contribution: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetGroupContribution(SetGroupContribution {
            agent_data: SetGroupContributionAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: SetGroupContributionDataBlock {
                group_id,
                contribution,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a group IM (`ImprovedInstantMessage`) reliably: the session id and
    /// recipient are both `group_id`, as group chat requires. `dialog` selects
    /// start/send/leave.
    fn send_group_session_im(
        &mut self,
        group_id: Uuid,
        dialog: ImDialog,
        message: &str,
        from_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let im = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: group_id,
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0,
                dialog: dialog.to_u8(),
                id: group_id,
                timestamp: 0,
                from_agent_name: with_nul(from_name),
                message: with_nul(message),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.send(&im, Reliability::Reliable, now)
    }

    /// Queues a `ScriptDialogReply` reliably (the chosen `llDialog` button).
    fn send_script_dialog_reply(
        &mut self,
        object_id: Uuid,
        chat_channel: i32,
        button_index: i32,
        button_label: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptDialogReply(ScriptDialogReply {
            agent_data: ScriptDialogReplyAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ScriptDialogReplyDataBlock {
                object_id,
                chat_channel,
                button_index,
                button_label: with_nul(button_label),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ScriptAnswerYes` reliably granting `permissions` to the script
    /// `item_id` in object `task_id` (pass `0` to deny everything).
    fn send_script_answer_yes(
        &mut self,
        task_id: Uuid,
        item_id: Uuid,
        permissions: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptAnswerYes(ScriptAnswerYes {
            agent_data: ScriptAnswerYesAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ScriptAnswerYesDataBlock {
                task_id,
                item_id,
                questions: permissions,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MuteListRequest` reliably. `mute_crc` is the CRC of the cached
    /// mute list (`0` forces a fresh download).
    fn send_mute_list_request(&mut self, mute_crc: u32, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::MuteListRequest(MuteListRequest {
            agent_data: MuteListRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            mute_data: MuteListRequestMuteDataBlock { mute_crc },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateMuteListEntry` reliably (add or update a mute).
    fn send_update_mute_list_entry(
        &mut self,
        mute_id: Uuid,
        mute_name: &str,
        mute_type: i32,
        mute_flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UpdateMuteListEntry(UpdateMuteListEntry {
            agent_data: UpdateMuteListEntryAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            mute_data: UpdateMuteListEntryMuteDataBlock {
                mute_id,
                mute_name: with_nul(mute_name),
                mute_type,
                mute_flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveMuteListEntry` reliably (remove a mute).
    fn send_remove_mute_list_entry(
        &mut self,
        mute_id: Uuid,
        mute_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveMuteListEntry(RemoveMuteListEntry {
            agent_data: RemoveMuteListEntryAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            mute_data: RemoveMuteListEntryMuteDataBlock {
                mute_id,
                mute_name: with_nul(mute_name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestXfer` reliably to download the file `filename` under the
    /// transfer id `xfer_id`.
    fn send_request_xfer(
        &mut self,
        xfer_id: u64,
        filename: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestXfer(RequestXfer {
            xfer_id: RequestXferXferIDBlock {
                id: xfer_id,
                filename: with_nul(filename),
                file_path: 0,
                delete_on_completion: true,
                use_big_packets: false,
                v_file_id: Uuid::nil(),
                v_file_type: 0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ConfirmXferPacket` reliably acknowledging `packet` of `xfer_id`.
    fn send_confirm_xfer_packet(
        &mut self,
        xfer_id: u64,
        packet: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ConfirmXferPacket(ConfirmXferPacket {
            xfer_id: ConfirmXferPacketXferIDBlock {
                id: xfer_id,
                packet,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AssetUploadRequest` reliably: a legacy UDP upload of `data` as
    /// asset class `asset_type`, identified by `transaction_id`. `data` is the
    /// inline payload (empty to force the `Xfer` path); `temp_file`/`store_local`
    /// mark a temporary / sim-local-only asset.
    fn send_asset_upload_request(
        &mut self,
        transaction_id: Uuid,
        asset_type: i8,
        temp_file: bool,
        store_local: bool,
        data: Vec<u8>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AssetUploadRequest(AssetUploadRequest {
            asset_block: AssetUploadRequestAssetBlockBlock {
                transaction_id,
                r#type: asset_type,
                tempfile: temp_file,
                store_local,
                asset_data: data,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SendXferPacket` reliably: the chunk `data` for sequence
    /// `packet` of upload `xfer_id`. `packet` already carries the `0x80000000`
    /// last-packet flag for the final chunk.
    fn send_send_xfer_packet(
        &mut self,
        xfer_id: u64,
        packet: u32,
        data: Vec<u8>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SendXferPacket(SendXferPacket {
            xfer_id: SendXferPacketXferIDBlock {
                id: xfer_id,
                packet,
            },
            data_packet: SendXferPacketDataPacketBlock { data },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestImage` reliably to download the texture `image_id` over
    /// the legacy UDP image path, starting at `packet` (0 for a fresh download)
    /// and at the given `discard_level` (0 = full resolution) and download
    /// `priority`. `image_type` is the request channel (0 = normal).
    fn send_request_image(
        &mut self,
        image_id: Uuid,
        discard_level: i8,
        priority: f32,
        packet: u32,
        image_type: u8,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestImage(RequestImage {
            agent_data: RequestImageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            request_image: vec![RequestImageRequestImageBlock {
                image: image_id,
                discard_level,
                download_priority: priority,
                packet,
                r#type: image_type,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TransferRequest` reliably to download a generic asset over the
    /// transfer path: channel `LLTCT_ASSET` (2), source `LLTST_ASSET` (2), and a
    /// `Params` block of the asset id (16 bytes) followed by its `LLAssetType`
    /// code (a little-endian `i32`), matching the viewer's `LLTransferSourceAsset`.
    fn send_transfer_request(
        &mut self,
        transfer_id: Uuid,
        asset_id: Uuid,
        asset_type: AssetType,
        priority: f32,
        now: Instant,
    ) -> Result<(), WireError> {
        // LLTCT_ASSET / LLTST_ASSET.
        const CHANNEL_ASSET: i32 = 2;
        const SOURCE_ASSET: i32 = 2;
        // The viewer's `LLTransferSourceAsset` params: the asset UUID followed
        // by its `LLAssetType` code as a little-endian `i32`.
        let mut writer = Writer::new();
        writer.put_uuid(asset_id);
        writer.put_i32(asset_type.to_code());
        let message = AnyMessage::TransferRequest(TransferRequest {
            transfer_info: TransferRequestTransferInfoBlock {
                transfer_id,
                channel_type: CHANNEL_ASSET,
                source_type: SOURCE_ASSET,
                priority,
                params: writer.into_bytes(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentWearablesRequest` reliably, asking the simulator to
    /// (re-)send the agent's current wearables as an `AgentWearablesUpdate`.
    fn send_agent_wearables_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentWearablesRequest(AgentWearablesRequest {
            agent_data: AgentWearablesRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentIsNowWearing` reliably, telling the simulator the agent's
    /// new outfit (one `(item id, wearable slot)` per worn wearable).
    fn send_agent_is_now_wearing(
        &mut self,
        wearables: &[Wearable],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentIsNowWearing(AgentIsNowWearing {
            agent_data: AgentIsNowWearingAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            wearable_data: wearables
                .iter()
                .map(|wearable| AgentIsNowWearingWearableDataBlock {
                    item_id: wearable.item_id,
                    wearable_type: wearable.wearable_type.to_code(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentSetAppearance` reliably, advertising the agent's own
    /// appearance: its bounding-box `size`, the baked-texture `texture_entry`
    /// blob, the `visual_params` bytes, and the per-baked-slot `wearable_cache`
    /// hashes (`(cache id, texture slot index)`). `serial` must increase on each
    /// change (0 resets).
    fn send_agent_set_appearance(
        &mut self,
        serial: u32,
        size: Vector,
        texture_entry: &[u8],
        visual_params: &[u8],
        wearable_cache: &[(Uuid, u8)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentSetAppearance(AgentSetAppearance {
            agent_data: AgentSetAppearanceAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                serial_num: serial,
                size,
            },
            wearable_data: wearable_cache
                .iter()
                .map(
                    |&(cache_id, texture_index)| AgentSetAppearanceWearableDataBlock {
                        cache_id,
                        texture_index,
                    },
                )
                .collect(),
            object_data: AgentSetAppearanceObjectDataBlock {
                texture_entry: texture_entry.to_vec(),
            },
            visual_param: visual_params
                .iter()
                .map(|&param_value| AgentSetAppearanceVisualParamBlock { param_value })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentAnimation` reliably, starting or stopping the agent's own
    /// animations. Each `(anim_id, start)` pair starts (`true`) or stops
    /// (`false`) one animation. Mirrors the reference viewer, which always
    /// appends a single empty `PhysicalAvatarEventList` block.
    fn send_agent_animation(
        &mut self,
        animations: &[(Uuid, bool)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentAnimation(AgentAnimation {
            agent_data: AgentAnimationAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            animation_list: animations
                .iter()
                .map(|&(anim_id, start_anim)| AgentAnimationAnimationListBlock {
                    anim_id,
                    start_anim,
                })
                .collect(),
            physical_avatar_event_list: vec![AgentAnimationPhysicalAvatarEventListBlock {
                type_data: Vec::new(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentCachedTexture` reliably, asking the simulator which of the
    /// queried baked-texture slots it already has cached (`(cache id, texture
    /// slot index)` per slot). The reply is an `AgentCachedTextureResponse`.
    fn send_agent_cached_texture(
        &mut self,
        serial: i32,
        slots: &[(Uuid, u8)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentCachedTexture(AgentCachedTexture {
            agent_data: AgentCachedTextureAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                serial_num: serial,
            },
            wearable_data: slots
                .iter()
                .map(|&(id, texture_index)| AgentCachedTextureWearableDataBlock {
                    id,
                    texture_index,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `FetchInventoryDescendents` reliably for the folder `folder_id`
    /// (sorted by name), requesting its sub-folders and items.
    fn send_fetch_inventory_descendents(
        &mut self,
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::FetchInventoryDescendents(FetchInventoryDescendents {
            agent_data: FetchInventoryDescendentsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            inventory_data: FetchInventoryDescendentsInventoryDataBlock {
                folder_id,
                // Own inventory: the owner is the agent itself.
                owner_id: self.agent_id,
                sort_order: 0, // 0 = by name
                fetch_folders: true,
                fetch_items: true,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TeleportLocationRequest` reliably.
    fn send_teleport_location_request(
        &mut self,
        region_handle: u64,
        position: Vector,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::TeleportLocationRequest(TeleportLocationRequest {
            agent_data: TeleportLocationRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            info: TeleportLocationRequestInfoBlock {
                region_handle,
                position,
                look_at,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LogoutRequest` reliably.
    fn send_logout_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::LogoutRequest(LogoutRequest {
            agent_data: LogoutRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestRegionInfo` reliably.
    fn send_request_region_info(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::RequestRegionInfo(RequestRegionInfo {
            agent_data: RequestRegionInfoAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoneyBalanceRequest` reliably. The transaction id is nil: a
    /// plain balance poll does not need to correlate a specific transaction.
    fn send_money_balance_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::MoneyBalanceRequest(MoneyBalanceRequest {
            agent_data: MoneyBalanceRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            money_data: MoneyBalanceRequestMoneyDataBlock {
                transaction_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EconomyDataRequest` reliably (an empty message).
    fn send_economy_data_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::EconomyDataRequest(EconomyDataRequest {});
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoneyTransferRequest` reliably: pay `amount` L$ to `dest` with
    /// the given transaction type and description. The source is this agent.
    fn send_money_transfer(
        &mut self,
        dest: Uuid,
        amount: i32,
        transaction_type: i32,
        description: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MoneyTransferRequest(MoneyTransferRequest {
            agent_data: MoneyTransferRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            money_data: MoneyTransferRequestMoneyDataBlock {
                source_id: self.agent_id,
                dest_id: dest,
                // Flags and the aggregate-permission hints are unused for a plain
                // avatar/object payment; the simulator ignores them.
                flags: 0,
                amount,
                aggregate_perm_next_owner: 0,
                aggregate_perm_inventory: 0,
                transaction_type,
                description: with_nul(description),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelPropertiesRequest` reliably for the given metre rectangle.
    fn send_parcel_properties_request(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        sequence_id: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelPropertiesRequest(ParcelPropertiesRequest {
            agent_data: ParcelPropertiesRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelPropertiesRequestParcelDataBlock {
                sequence_id,
                west,
                south,
                east,
                north,
                snap_selection: false,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelPropertiesUpdate` reliably (edit a parcel's settings).
    fn send_parcel_properties_update(
        &mut self,
        update: &ParcelUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelPropertiesUpdate(ParcelPropertiesUpdate {
            agent_data: ParcelPropertiesUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelPropertiesUpdateParcelDataBlock {
                local_id: update.local_id,
                // The message-level flag the reference viewer sends (0x01).
                flags: 0x1,
                parcel_flags: update.parcel_flags.bits(),
                sale_price: update.sale_price,
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                music_url: with_nul(&update.music_url),
                media_url: with_nul(&update.media_url),
                media_id: update.media_id,
                media_auto_scale: u8::from(update.media_auto_scale),
                group_id: update.group_id,
                pass_price: update.pass_price,
                pass_hours: update.pass_hours,
                category: update.category.to_u8(),
                auth_buyer_id: update.auth_buyer_id,
                snapshot_id: update.snapshot_id,
                user_location: update.user_location.clone(),
                user_look_at: update.user_look_at.clone(),
                landing_type: update.landing_type,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelAccessListRequest` reliably (fetch the allow or ban list
    /// selected by `flags`). The reply is a `ParcelAccessListReply`.
    fn send_parcel_access_list_request(
        &mut self,
        local_id: i32,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelAccessListRequest(ParcelAccessListRequest {
            agent_data: ParcelAccessListRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelAccessListRequestDataBlock {
                sequence_id: 0,
                flags,
                local_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelAccessListUpdate` reliably (replace the allow or ban list
    /// selected by `flags`). An empty list clears it (sent as one empty entry, as
    /// the reference viewer does).
    fn send_parcel_access_list_update(
        &mut self,
        local_id: i32,
        flags: u32,
        entries: &[ParcelAccessEntry],
        now: Instant,
    ) -> Result<(), WireError> {
        let list = if entries.is_empty() {
            vec![ParcelAccessListUpdateListBlock {
                id: Uuid::nil(),
                time: 0,
                flags: 0,
            }]
        } else {
            entries
                .iter()
                .map(|entry| ParcelAccessListUpdateListBlock {
                    id: entry.id,
                    time: entry.time,
                    flags,
                })
                .collect()
        };
        let message = AnyMessage::ParcelAccessListUpdate(ParcelAccessListUpdate {
            agent_data: ParcelAccessListUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelAccessListUpdateDataBlock {
                flags,
                local_id,
                transaction_id: Uuid::nil(),
                sequence_id: 1,
                sections: 1,
            },
            list,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDwellRequest` reliably. The reply is a `ParcelDwellReply`.
    fn send_parcel_dwell_request(&mut self, local_id: i32, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDwellRequest(ParcelDwellRequest {
            agent_data: ParcelDwellRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            // The simulator fills in parcel_id from local_id.
            data: ParcelDwellRequestDataBlock {
                local_id,
                parcel_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelBuy` reliably (purchase the parcel).
    fn send_parcel_buy(
        &mut self,
        local_id: i32,
        price: i32,
        area: i32,
        group_id: Uuid,
        is_group_owned: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelBuy(ParcelBuy {
            agent_data: ParcelBuyAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelBuyDataBlock {
                group_id,
                is_group_owned,
                remove_contribution: false,
                local_id,
                r#final: true,
            },
            parcel_data: ParcelBuyParcelDataBlock { price, area },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelReturnObjects` reliably (return objects on the parcel
    /// matching `return_type`, optionally scoped to the given owner/task ids).
    fn send_parcel_return_objects(
        &mut self,
        local_id: i32,
        return_type: u32,
        owner_ids: &[Uuid],
        task_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelReturnObjects(ParcelReturnObjects {
            agent_data: ParcelReturnObjectsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelReturnObjectsParcelDataBlock {
                local_id,
                return_type,
            },
            task_i_ds: task_ids
                .iter()
                .map(|id| ParcelReturnObjectsTaskIDsBlock { task_id: *id })
                .collect(),
            owner_i_ds: owner_ids
                .iter()
                .map(|id| ParcelReturnObjectsOwnerIDsBlock { owner_id: *id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelSelectObjects` reliably (select the parcel objects matching
    /// `return_type`, or the explicit `object_ids` when using the list type).
    fn send_parcel_select_objects(
        &mut self,
        local_id: i32,
        return_type: u32,
        object_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelSelectObjects(ParcelSelectObjects {
            agent_data: ParcelSelectObjectsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelSelectObjectsParcelDataBlock {
                local_id,
                return_type,
            },
            return_i_ds: object_ids
                .iter()
                .map(|id| ParcelSelectObjectsReturnIDsBlock { return_id: *id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDeedToGroup` reliably (deed the parcel to `group_id`).
    fn send_parcel_deed_to_group(
        &mut self,
        local_id: i32,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDeedToGroup(ParcelDeedToGroup {
            agent_data: ParcelDeedToGroupAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelDeedToGroupDataBlock { group_id, local_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelReclaim` reliably (reclaim the parcel to the estate).
    fn send_parcel_reclaim(&mut self, local_id: i32, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ParcelReclaim(ParcelReclaim {
            agent_data: ParcelReclaimAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelReclaimDataBlock { local_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelRelease` reliably (abandon the parcel back to the estate).
    fn send_parcel_release(&mut self, local_id: i32, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ParcelRelease(ParcelRelease {
            agent_data: ParcelReleaseAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelReleaseDataBlock { local_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EstateOwnerMessage` reliably with the given method and string
    /// parameters. An empty parameter list is sent as one empty block (matching
    /// the reference viewer). The invoice is nil — the simulator echoes it back.
    fn send_estate_owner_message(
        &mut self,
        method: &str,
        params: &[String],
        now: Instant,
    ) -> Result<(), WireError> {
        let param_list = if params.is_empty() {
            vec![EstateOwnerMessageParamListBlock {
                parameter: Vec::new(),
            }]
        } else {
            params
                .iter()
                .map(|param| EstateOwnerMessageParamListBlock {
                    parameter: with_nul(param),
                })
                .collect()
        };
        let message = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
            agent_data: EstateOwnerMessageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                transaction_id: Uuid::nil(),
            },
            method_data: EstateOwnerMessageMethodDataBlock {
                method: with_nul(method),
                invoice: Uuid::nil(),
            },
            param_list,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GodlikeMessage` reliably (a god-level estate/admin command).
    fn send_godlike_message(
        &mut self,
        method: &str,
        params: &[String],
        now: Instant,
    ) -> Result<(), WireError> {
        let param_list = if params.is_empty() {
            vec![GodlikeMessageParamListBlock {
                parameter: Vec::new(),
            }]
        } else {
            params
                .iter()
                .map(|param| GodlikeMessageParamListBlock {
                    parameter: with_nul(param),
                })
                .collect()
        };
        let message = AnyMessage::GodlikeMessage(GodlikeMessage {
            agent_data: GodlikeMessageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                transaction_id: Uuid::nil(),
            },
            method_data: GodlikeMessageMethodDataBlock {
                method: with_nul(method),
                invoice: Uuid::nil(),
            },
            param_list,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GodKickUser` reliably (god-level eject of `target`).
    fn send_god_kick_user(
        &mut self,
        target: Uuid,
        reason: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GodKickUser(GodKickUser {
            user_info: GodKickUserUserInfoBlock {
                god_id: self.agent_id,
                god_session_id: self.session_id,
                agent_id: target,
                kick_flags: 0,
                reason: with_nul(reason),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapBlockRequest` reliably for a grid-coordinate rectangle.
    fn send_map_block_request(
        &mut self,
        min_x: u16,
        max_x: u16,
        min_y: u16,
        max_y: u16,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MapBlockRequest(MapBlockRequest {
            agent_data: MapBlockRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                // Flags 0 selects the terrain map layer; estate/godlike unused.
                flags: 0,
                estate_id: 0,
                godlike: false,
            },
            position_data: MapBlockRequestPositionDataBlock {
                min_x,
                max_x,
                min_y,
                max_y,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapNameRequest` reliably (search regions by name). The reply is
    /// a `MapBlockReply`, the same as [`Circuit::send_map_block_request`].
    fn send_map_name_request(&mut self, name: &str, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::MapNameRequest(MapNameRequest {
            agent_data: MapNameRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                // The viewer's map-layer flag (2); estate/godlike filled by the sim.
                flags: MAP_LAYER_FLAG,
                estate_id: 0,
                godlike: false,
            },
            name_data: MapNameRequestNameDataBlock {
                name: with_nul(name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapItemRequest` reliably for the given item type. `region_handle`
    /// of 0 targets the current region; otherwise it targets that region.
    fn send_map_item_request(
        &mut self,
        item_type: u32,
        region_handle: u64,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MapItemRequest(MapItemRequest {
            agent_data: MapItemRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                flags: MAP_LAYER_FLAG,
                estate_id: 0,
                godlike: false,
            },
            request_data: MapItemRequestRequestDataBlock {
                item_type,
                region_handle,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestMultipleObjects` reliably, asking the simulator to (re)send
    /// the full `ObjectUpdate` for each local id (cache-miss type "full" = 0).
    fn send_request_multiple_objects(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestMultipleObjects(RequestMultipleObjects {
            agent_data: RequestMultipleObjectsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| RequestMultipleObjectsObjectDataBlock {
                    cache_miss_type: 0,
                    id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSelect` reliably for the given local ids. Selecting an
    /// object makes the simulator send its `ObjectProperties`.
    fn send_object_select(&mut self, local_ids: &[u32], now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSelect(ObjectSelect {
            agent_data: ObjectSelectAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectSelectObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDeselect` reliably for the given local ids.
    fn send_object_deselect(&mut self, local_ids: &[u32], now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDeselect(ObjectDeselect {
            agent_data: ObjectDeselectAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDeselectObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    // Object interaction & editing (#17) -----------------------------------

    /// Queues an `ObjectGrab` reliably (the start of a touch/click) for `local_id`
    /// with `grab_offset` and an empty surface-info list.
    fn send_object_grab(
        &mut self,
        local_id: u32,
        grab_offset: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGrab(ObjectGrab {
            agent_data: ObjectGrabAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: ObjectGrabObjectDataBlock {
                local_id,
                grab_offset,
            },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectGrabUpdate` reliably (a drag while grabbing) for the
    /// object `object_id`.
    fn send_object_grab_update(
        &mut self,
        object_id: Uuid,
        grab_offset_initial: Vector,
        grab_position: Vector,
        time_since_last: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGrabUpdate(ObjectGrabUpdate {
            agent_data: ObjectGrabUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: ObjectGrabUpdateObjectDataBlock {
                object_id,
                grab_offset_initial,
                grab_position,
                time_since_last,
            },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDeGrab` reliably (the end of a touch/click) for `local_id`.
    fn send_object_degrab(&mut self, local_id: u32, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDeGrab(ObjectDeGrab {
            agent_data: ObjectDeGrabAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: ObjectDeGrabObjectDataBlock { local_id },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectAdd` reliably to rez a new primitive from `shape`.
    fn send_object_add(
        &mut self,
        shape: &PrimShape,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectAdd(ObjectAdd {
            agent_data: ObjectAddAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            object_data: ObjectAddObjectDataBlock {
                p_code: shape.pcode,
                material: shape.material.to_code(),
                add_flags: shape.add_flags,
                path_curve: shape.path_curve,
                profile_curve: shape.profile_curve,
                path_begin: shape.path_begin,
                path_end: shape.path_end,
                path_scale_x: shape.path_scale_x,
                path_scale_y: shape.path_scale_y,
                path_shear_x: shape.path_shear_x,
                path_shear_y: shape.path_shear_y,
                path_twist: shape.path_twist,
                path_twist_begin: shape.path_twist_begin,
                path_radius_offset: shape.path_radius_offset,
                path_taper_x: shape.path_taper_x,
                path_taper_y: shape.path_taper_y,
                path_revolutions: shape.path_revolutions,
                path_skew: shape.path_skew,
                profile_begin: shape.profile_begin,
                profile_end: shape.profile_end,
                profile_hollow: shape.profile_hollow,
                // Rez exactly at `position`: skip the raycast and treat the ray
                // endpoint as the placement point (the viewer's headless rez path).
                bypass_raycast: 1,
                ray_start: shape.position.clone(),
                ray_end: shape.position.clone(),
                ray_target_id: Uuid::nil(),
                ray_end_is_intersection: 0,
                scale: shape.scale.clone(),
                rotation: shape.rotation.clone(),
                state: shape.state,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDuplicate` reliably (copy `local_ids` by `offset`).
    fn send_object_duplicate(
        &mut self,
        local_ids: &[u32],
        offset: Vector,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDuplicate(ObjectDuplicate {
            agent_data: ObjectDuplicateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            shared_data: ObjectDuplicateSharedDataBlock {
                offset,
                duplicate_flags: 0,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDuplicateObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDelete` reliably for `local_ids` (non-god, non-forced).
    fn send_object_delete(&mut self, local_ids: &[u32], now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDelete(ObjectDelete {
            agent_data: ObjectDeleteAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                force: false,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDeleteObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeRezObject` reliably (take/return/trash `local_ids`).
    fn send_derez_object(
        &mut self,
        local_ids: &[u32],
        destination: DeRezDestination,
        destination_id: Uuid,
        transaction_id: Uuid,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeRezObject(DeRezObject {
            agent_data: DeRezObjectAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            agent_block: DeRezObjectAgentBlockBlock {
                group_id,
                destination: destination.to_code(),
                destination_id,
                transaction_id,
                // The whole selection fits in one packet.
                packet_count: 1,
                packet_number: 0,
            },
            object_data: local_ids
                .iter()
                .map(|id| DeRezObjectObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectName` reliably (rename `local_id`).
    fn send_object_name(
        &mut self,
        local_id: u32,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectName(ObjectName {
            agent_data: ObjectNameAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectNameObjectDataBlock {
                local_id,
                name: name.as_bytes().to_vec(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDescription` reliably (re-describe `local_id`).
    fn send_object_description(
        &mut self,
        local_id: u32,
        description: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDescription(ObjectDescription {
            agent_data: ObjectDescriptionAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectDescriptionObjectDataBlock {
                local_id,
                description: description.as_bytes().to_vec(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectClickAction` reliably (set the left-click behaviour).
    fn send_object_click_action(
        &mut self,
        local_id: u32,
        action: ClickAction,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectClickAction(ObjectClickAction {
            agent_data: ObjectClickActionAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectClickActionObjectDataBlock {
                object_local_id: local_id,
                click_action: action.to_code(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectMaterial` reliably (set the physical material).
    fn send_object_material(
        &mut self,
        local_id: u32,
        material: Material,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectMaterial(ObjectMaterial {
            agent_data: ObjectMaterialAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectMaterialObjectDataBlock {
                object_local_id: local_id,
                material: material.to_code(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectFlagUpdate` reliably (set physics/temporary/phantom).
    fn send_object_flag_update(
        &mut self,
        local_id: u32,
        flags: &ObjectFlagSettings,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectFlagUpdate(ObjectFlagUpdate {
            agent_data: ObjectFlagUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                object_local_id: local_id,
                use_physics: flags.use_physics,
                is_temporary: flags.is_temporary,
                is_phantom: flags.is_phantom,
                casts_shadows: flags.casts_shadows,
            },
            // No extra-physics (shape-type/density/…) overrides.
            extra_physics: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectGroup` reliably (set the group `local_ids` are set to).
    fn send_object_group(
        &mut self,
        local_ids: &[u32],
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGroup(ObjectGroup {
            agent_data: ObjectGroupAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectGroupObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectPermissions` reliably (set/clear `mask` bits of `field`).
    fn send_object_permissions(
        &mut self,
        local_ids: &[u32],
        field: PermissionField,
        set: bool,
        mask: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectPermissions(ObjectPermissions {
            agent_data: ObjectPermissionsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            header_data: ObjectPermissionsHeaderDataBlock { r#override: false },
            object_data: local_ids
                .iter()
                .map(|id| ObjectPermissionsObjectDataBlock {
                    object_local_id: *id,
                    field: field.to_code(),
                    set: u8::from(set),
                    mask,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSaleInfo` reliably (set the sale type and price).
    fn send_object_sale_info(
        &mut self,
        local_id: u32,
        sale_type: SaleType,
        sale_price: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSaleInfo(ObjectSaleInfo {
            agent_data: ObjectSaleInfoAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectSaleInfoObjectDataBlock {
                local_id,
                sale_type: sale_type.to_code(),
                sale_price,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectCategory` reliably (set the object's category code).
    fn send_object_category(
        &mut self,
        local_id: u32,
        category: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectCategory(ObjectCategory {
            agent_data: ObjectCategoryAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectCategoryObjectDataBlock { local_id, category }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectIncludeInSearch` reliably (toggle search visibility).
    fn send_object_include_in_search(
        &mut self,
        local_id: u32,
        include: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectIncludeInSearch(ObjectIncludeInSearch {
            agent_data: ObjectIncludeInSearchAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectIncludeInSearchObjectDataBlock {
                object_local_id: local_id,
                include_in_search: include,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectLink` reliably linking `local_ids` (the first id becomes
    /// the linkset root).
    fn send_object_link(&mut self, local_ids: &[u32], now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ObjectLink(ObjectLink {
            agent_data: ObjectLinkAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectLinkObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDelink` reliably unlinking `local_ids`.
    fn send_object_delink(&mut self, local_ids: &[u32], now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDelink(ObjectDelink {
            agent_data: ObjectDelinkAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDelinkObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MultipleObjectUpdate` reliably applying `transform` to `local_id`.
    /// The packed `Data` blob carries position/rotation/scale in that fixed
    /// order, matching the simulator's `MultipleObjectUpdate` parser.
    fn send_multiple_object_update(
        &mut self,
        local_id: u32,
        transform: &ObjectTransform,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut data = Writer::new();
        if let Some(position) = &transform.position {
            data.put_vector3(position);
        }
        if let Some(rotation) = &transform.rotation {
            let [x, y, z] = pack_quaternion_to_vec3(rotation);
            data.put_f32(x);
            data.put_f32(y);
            data.put_f32(z);
        }
        if let Some(scale) = &transform.scale {
            data.put_vector3(scale);
        }
        let message = AnyMessage::MultipleObjectUpdate(MultipleObjectUpdate {
            agent_data: MultipleObjectUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![MultipleObjectUpdateObjectDataBlock {
                object_local_id: local_id,
                r#type: transform.type_byte(),
                data: data.into_bytes(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Records that a datagram was received, resetting the inactivity timer.
    fn note_received(&mut self, now: Instant) {
        self.timers.inactivity = deadline(now, INACTIVITY_TIMEOUT);
    }

    /// Records that we owe an acknowledgement for `sequence`, arming the flush.
    fn queue_ack(&mut self, sequence: u32, now: Instant) {
        self.pending_acks.push(sequence);
        if self.timers.ack_flush.is_none() {
            self.timers.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    fn record_acks(&mut self, ids: &[u32]) {
        for id in ids {
            self.unacked.remove(id);
        }
    }

    /// Records an inbound reliable `sequence`; returns `true` if it is new.
    fn mark_seen(&mut self, sequence: u32) -> bool {
        self.seen.insert(sequence)
    }

    /// Flushes owed acknowledgements as one or more `PacketAck` messages.
    fn flush_acks(&mut self, now: Instant) -> Result<(), WireError> {
        self.timers.ack_flush = None;
        if self.pending_acks.is_empty() {
            return Ok(());
        }
        let acks = std::mem::take(&mut self.pending_acks);
        for chunk in acks.chunks(MAX_ACKS_PER_PACKET) {
            let packets = chunk
                .iter()
                .map(|id| PacketAckPacketsBlock { id: *id })
                .collect();
            let message = AnyMessage::PacketAck(PacketAck { packets });
            self.send(&message, Reliability::Unreliable, now)?;
        }
        Ok(())
    }

    /// Retransmits unacknowledged reliable packets whose timeout has elapsed.
    ///
    /// Returns `true` if any packet has exhausted its retransmission budget.
    fn process_resends(&mut self, now: Instant) -> bool {
        let mut exhausted = false;
        let mut to_send = Vec::new();
        for packet in self.unacked.values_mut() {
            if now < deadline(packet.sent_at, RESEND_TIMEOUT) {
                continue;
            }
            if packet.attempts >= MAX_RESEND_ATTEMPTS {
                exhausted = true;
                continue;
            }
            let mut datagram = packet.datagram.clone();
            if let Some(first) = datagram.first_mut() {
                *first |= PacketFlags::RESENT.bits();
            }
            packet.sent_at = now;
            packet.attempts = packet.attempts.saturating_add(1);
            to_send.push(datagram);
        }
        self.out.extend(to_send);
        exhausted
    }

    /// The earliest retransmission deadline across all unacked packets.
    fn next_resend_deadline(&self) -> Option<Instant> {
        self.unacked
            .values()
            .map(|packet| deadline(packet.sent_at, RESEND_TIMEOUT))
            .min()
    }
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
    /// Pending high-level events for the driver.
    events: VecDeque<Event>,
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
            draw_distance: DEFAULT_DRAW_DISTANCE,
            controls: ControlFlags::empty(),
            throttle: None,
            body_rotation: IDENTITY_ROTATION,
            head_rotation: IDENTITY_ROTATION,
            sit_requested: false,
            handover: None,
            teleport_target: None,
            seed_capability: None,
            inventory_root: None,
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
            events: VecDeque::new(),
        }
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
        match message {
            "ParcelProperties" => {
                if let Some(parcel) = parcel_info_from_llsd(body) {
                    self.events
                        .push_back(Event::ParcelProperties(Box::new(parcel)));
                }
            }
            "TeleportFinish" => {
                if let Some((dest, seed)) = teleport_finish_from_llsd(body) {
                    let region_handle = self.teleport_target.unwrap_or(0);
                    self.begin_handover(dest, region_handle, Some(seed), now)?;
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
                }
            }
            // The agent has physically crossed a region border; OpenSim signals
            // the handover over the CAPS event queue (not the UDP `CrossedRegion`).
            // Promote the pre-opened child circuit for the destination to root.
            "CrossedRegion" if matches!(self.state, SessionState::Active) => {
                if let Some((handle, dest, seed)) = crossed_region_from_caps_llsd(body) {
                    self.promote_child_to_root(dest, handle, Some(seed), now)?;
                }
            }
            CAP_FETCH_INVENTORY => {
                for event in inventory_descendents_from_llsd(body) {
                    self.events.push_back(event);
                }
            }
            // The modern (CAPS event-queue) delivery of group memberships; the
            // UDP `AgentGroupDataUpdate` is deprecated on Second Life.
            "AgentGroupDataUpdate" => {
                if let Some(event) = group_memberships_from_caps_llsd(body) {
                    self.events.push_back(event);
                }
            }
            // The response to a `GroupMemberData` capability POST (the modern
            // group roster fetch).
            CAP_GROUP_MEMBER_DATA => {
                if let Some(event) = group_members_from_caps_llsd(body) {
                    self.events.push_back(event);
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
            _ => {}
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
        let _ignored = child.send_agent_update(controls, body, head, now);
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
                self.seed_capability = Some(success.seed_capability.clone());
                self.inventory_root = success.inventory_root;
                self.secure_session_id = success.secure_session_id;
                self.state = SessionState::AwaitingHandshake;
                self.events
                    .push_back(Event::CircuitEstablished { sim: sim_addr });
                if !success.inventory_skeleton.is_empty() {
                    let folders = success
                        .inventory_skeleton
                        .iter()
                        .map(skeleton_folder)
                        .collect();
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
        // Unrecognized messages are ignored rather than failing the datagram.
        let Ok(message) = AnyMessage::decode(id, &mut reader) else {
            return Ok(());
        };
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
            _ => {}
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
                for block in &update.object_data {
                    self.upsert_object(from, object_from_full_update(block, region_handle));
                }
            }
            AnyMessage::ObjectUpdateCompressed(update) => {
                let region_handle = update.region_data.region_handle;
                for block in &update.object_data {
                    if let Some(object) =
                        compressed_object(&block.data, region_handle, block.update_flags)
                    {
                        self.upsert_object(from, object);
                    }
                }
            }
            AnyMessage::ObjectUpdateCached(update) => {
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
                // Terse updates carry only motion. Apply to known objects; for
                // unknown ones (which lack identity here), fetch the full update.
                let mut misses = Vec::new();
                for block in &update.object_data {
                    let Some(terse) = terse_update(&block.data) else {
                        continue;
                    };
                    let local_id = terse.local_id;
                    if !self.apply_terse_update(from, terse) {
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

    /// Applies a motion-only terse update to an object already cached for
    /// simulator `from`, emitting [`Event::ObjectUpdated`]. Returns `false` if the
    /// object is not cached (the caller should fetch its full update).
    fn apply_terse_update(&mut self, from: SocketAddr, update: TerseUpdate) -> bool {
        let Some(object) = self
            .objects
            .get_mut(&from)
            .and_then(|sim| sim.get_mut(&update.local_id))
        else {
            return false;
        };
        object.state = update.state;
        object.motion = update.motion;
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
        // The terrain and region-handle caches for this sim go stale too.
        self.terrain.remove(&addr);
        self.regions.remove(&addr);
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
                    self.events
                        .push_back(Event::RegionInfoHandshake(Box::new(region_identity(
                            &handshake.region_info,
                            &handshake.region_info3,
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
                self.events.push_back(Event::RegionLimits(region_limits(
                    &info.region_info,
                    &info.region_info2,
                )));
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
                    .push_back(Event::ParcelProperties(Box::new(parcel_info(
                        &props.parcel_data,
                    ))));
            }
            AnyMessage::ParcelOverlay(overlay) => {
                self.events
                    .push_back(Event::ParcelOverlay(ParcelOverlayInfo {
                        sequence_id: overlay.parcel_data.sequence_id,
                        data: overlay.parcel_data.data.clone(),
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
                        })
                        .collect(),
                });
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
                        circuit.send_agent_sit(now)?;
                    }
                    let transform = &response.sit_transform;
                    self.events.push_back(Event::SitResult {
                        sit_object: response.sit_object.id,
                        autopilot: transform.auto_pilot,
                        sit_position: (
                            transform.sit_position.x,
                            transform.sit_position.y,
                            transform.sit_position.z,
                        ),
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
            AnyMessage::InventoryDescendents(reply) => {
                self.events.push_back(Event::InventoryDescendents {
                    folder_id: reply.agent_data.folder_id,
                    version: reply.agent_data.version,
                    descendents: reply.agent_data.descendents,
                    folders: reply.folder_data.iter().map(inventory_folder).collect(),
                    items: reply.item_data.iter().map(inventory_item).collect(),
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
                if !matches!(status, TransferStatus::Ok | TransferStatus::Done)
                    && let Some(transfer) = self.asset_transfers.remove(&transfer_id)
                {
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
                    roles: reply.role_data.iter().map(group_role).collect(),
                });
            }
            AnyMessage::GroupRoleMembersReply(reply) => {
                self.events.push_back(Event::GroupRoleMembers {
                    group_id: reply.agent_data.group_id,
                    request_id: reply.agent_data.request_id,
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
            _ => {}
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
            });
            return Ok(());
        }

        let exhausted = self
            .circuit
            .as_mut()
            .is_some_and(|c| c.process_resends(now));
        if exhausted {
            self.close(DisconnectReason::HandshakeFailed);
            return Ok(());
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
            if let Some(circuit) = self.circuit.as_mut() {
                circuit.send_agent_update(controls, body, head, now)?;
                circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
            }
        }

        // Keep child circuits healthy: flush owed acks, retransmit, advertise the
        // agent (camera/interest) so the neighbour streams its objects, and drop
        // any that have gone silent (a dead child never fails the session).
        let controls = self.controls.bits();
        let body = self.body_rotation.clone();
        let head = self.head_rotation.clone();
        let mut dead = Vec::new();
        for (addr, child) in &mut self.children {
            if now >= child.timers.inactivity {
                dead.push(*addr);
                continue;
            }
            child.process_resends(now);
            if child.timers.ack_flush.is_some_and(|d| now >= d) {
                child.flush_acks(now)?;
            }
            if child.timers.agent_update.is_some_and(|d| now >= d) {
                child.send_agent_update(controls, body.clone(), head.clone(), now)?;
                child.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
            }
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
    /// [`InstantMessage::id`] as the transaction id.
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
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.send_agent_update(controls, body, head, now)?;
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
    /// is the [`InstantMessage::id`] of the incoming
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
    /// is the [`InstantMessage::id`] of the incoming
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

    /// Replies to a scripted-object dialog (`ScriptDialogReply`): the chosen
    /// `button_index`/`button_label` (from the [`Event::ScriptDialog`]'s
    /// [`ScriptDialog::buttons`]) is sent back to `object_id` on the dialog's
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

    /// Advertises the agent's own appearance to the simulator (and, through it,
    /// to other viewers) via `AgentSetAppearance`: its bounding-box `size`
    /// (metres), the packed `texture_entry` blob carrying the baked-texture ids,
    /// the `visual_params` bytes (one quantized byte per parameter, in the
    /// reference viewer's order), and the per-baked-slot `wearable_cache` hashes
    /// (`(cache id, texture slot index)`; see the [`avatar_texture`] constants).
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
    /// [`avatar_texture`] constants) the simulator reports whether it already has
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

    /// The agent's inventory root ("My Inventory") folder id, from the login
    /// response, or `None` if the grid did not provide it. Use it as the starting
    /// point for [`Session::request_folder_contents`].
    #[must_use]
    pub const fn inventory_root(&self) -> Option<Uuid> {
        self.inventory_root
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

/// Decodes name/SKU bytes to a `String`, dropping any trailing NUL padding the
/// simulator appends to fixed-width string fields.
fn trimmed_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_owned()
}

/// Encodes a string as NUL-terminated UTF-8 bytes, as the viewer sends variable
/// string fields on the wire.
fn with_nul(s: &str) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Parses a downloaded mute-list file into [`MuteEntry`] values. Each non-empty
/// line is `<type> <uuid> <name>|<flags>` (the viewer's on-disk format).
fn parse_mute_list(bytes: &[u8]) -> Vec<MuteEntry> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(parse_mute_line)
        .collect()
}

/// Parses one mute-list line, or `None` if it is blank/malformed.
fn parse_mute_line(line: &str) -> Option<MuteEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // The flags follow the last '|'; everything before is "<type> <uuid> <name>".
    let (head, flags) = line.rsplit_once('|').map_or((line, 0), |(head, tail)| {
        (head, tail.trim().parse().unwrap_or(0))
    });
    let mut parts = head.splitn(3, ' ');
    let mute_type = parts.next()?.trim().parse::<i32>().ok()?;
    let id = Uuid::parse_str(parts.next()?.trim()).unwrap_or_else(|_| Uuid::nil());
    let name = parts.next().unwrap_or("").trim().to_owned();
    Some(MuteEntry {
        id,
        name,
        mute_type: MuteType::from_i32(mute_type),
        flags: MuteFlags(flags),
    })
}

/// Builds a [`RegionIdentity`] from a `RegionHandshake`'s region-info blocks.
fn region_identity(
    info: &RegionHandshakeRegionInfoBlock,
    info3: &RegionHandshakeRegionInfo3Block,
) -> RegionIdentity {
    let product_sku = trimmed_string(&info3.product_sku);
    let product_name = trimmed_string(&info3.product_name);
    RegionIdentity {
        sim_name: trimmed_string(&info.sim_name),
        region_flags: info.region_flags,
        maturity: Maturity::from_sim_access(info.sim_access),
        product: ProductType::classify(&product_sku, &product_name),
        product_sku,
        product_name,
    }
}

/// Builds [`RegionLimits`] from a `RegionInfo` message's region-info blocks.
fn region_limits(
    info: &RegionInfoRegionInfoBlock,
    info2: &RegionInfoRegionInfo2Block,
) -> RegionLimits {
    // Prefer the 32-bit agent cap; fall back to the legacy 8-bit field when the
    // grid leaves the wider one at zero.
    let max_agents = if info2.max_agents32 == 0 {
        u32::from(info.max_agents)
    } else {
        info2.max_agents32
    };
    RegionLimits {
        sim_name: trimmed_string(&info.sim_name),
        max_agents,
        hard_max_agents: info2.hard_max_agents,
        hard_max_objects: info2.hard_max_objects,
        region_flags: info.region_flags,
        maturity: Maturity::from_sim_access(info.sim_access),
    }
}

/// Builds a [`MoneyBalance`] from a `MoneyBalanceReply`. The optional
/// `TransactionInfo` block is all-zero for a plain balance poll; it is surfaced
/// only when it describes a real transaction (non-zero type).
fn money_balance(reply: &sl_wire::messages::MoneyBalanceReply) -> MoneyBalance {
    let data = &reply.money_data;
    let info = &reply.transaction_info;
    let transaction = (info.transaction_type != 0).then(|| MoneyTransaction {
        transaction_type: info.transaction_type,
        source_id: info.source_id,
        source_is_group: info.is_source_group,
        dest_id: info.dest_id,
        dest_is_group: info.is_dest_group,
        amount: LindenAmount(u64::try_from(info.amount).unwrap_or(0)),
        item_description: trimmed_string(&info.item_description),
    });
    MoneyBalance {
        agent_id: data.agent_id,
        success: data.transaction_success,
        balance: LindenAmount(u64::try_from(data.money_balance).unwrap_or(0)),
        square_meters_credit: data.square_meters_credit,
        square_meters_committed: data.square_meters_committed,
        description: trimmed_string(&data.description),
        transaction,
    }
}

/// Builds an [`AvatarAppearance`] from an `AvatarAppearance` message: decodes the
/// per-avatar `TextureEntry` (the baked-texture ids) and collects the visual
/// params and optional appearance/hover/attachment blocks.
fn avatar_appearance(message: &sl_wire::messages::AvatarAppearance) -> AvatarAppearance {
    let texture_entry =
        appearance::decode_texture_entry(&message.object_data.texture_entry, avatar_texture::COUNT);
    let visual_params = message
        .visual_param
        .iter()
        .map(|block| block.param_value)
        .collect();
    let appearance_block = message.appearance_data.first();
    let attachments = message
        .attachment_block
        .iter()
        .map(|block| AvatarAttachment {
            id: block.id,
            attachment_point: block.attachment_point,
        })
        .collect();
    AvatarAppearance {
        avatar_id: message.sender.id,
        is_trial: message.sender.is_trial,
        texture_entry,
        visual_params,
        appearance_version: appearance_block.map(|block| block.appearance_version),
        cof_version: appearance_block.map(|block| block.cof_version),
        appearance_flags: appearance_block.map(|block| block.flags),
        hover_height: message
            .appearance_hover
            .first()
            .map(|block| block.hover_height.clone()),
        attachments,
    }
}

/// Builds the [`PlayingAnimation`] list from an `AvatarAnimation` message. The
/// `AnimationSourceList` is positionally correlated with the `AnimationList`
/// (entry `i`'s source is `AnimationSourceList[i]`, when present), matching the
/// reference viewer's `process_avatar_animation`.
fn avatar_animations(message: &sl_wire::messages::AvatarAnimation) -> Vec<PlayingAnimation> {
    message
        .animation_list
        .iter()
        .enumerate()
        .map(|(index, block)| PlayingAnimation {
            anim_id: block.anim_id,
            sequence_id: block.anim_sequence_id,
            source_id: message
                .animation_source_list
                .get(index)
                .map(|source| source.object_id),
        })
        .collect()
}

/// Builds an [`Event::ServerAppearanceUpdate`] from the LLSD reply to an
/// `UpdateAvatarAppearance` POST (`{ success, error?, expected? }`).
fn server_appearance_update_from_llsd(body: &Llsd) -> Event {
    Event::ServerAppearanceUpdate {
        success: body.get("success").and_then(Llsd::as_bool).unwrap_or(false),
        error: body
            .get("error")
            .and_then(Llsd::as_str)
            .map(ToOwned::to_owned),
        expected_cof_version: body.get("expected").and_then(Llsd::as_i32),
    }
}

/// Builds [`EconomyData`] from an `EconomyData` message's info block.
const fn economy_data(data: &sl_wire::messages::EconomyData) -> EconomyData {
    let info = &data.info;
    EconomyData {
        object_capacity: info.object_capacity,
        object_count: info.object_count,
        price_energy_unit: info.price_energy_unit,
        price_object_claim: info.price_object_claim,
        price_public_object_decay: info.price_public_object_decay,
        price_public_object_delete: info.price_public_object_delete,
        price_parcel_claim: info.price_parcel_claim,
        price_parcel_claim_factor: info.price_parcel_claim_factor,
        price_upload: info.price_upload,
        price_rent_light: info.price_rent_light,
        teleport_min_price: info.teleport_min_price,
        teleport_price_exponent: info.teleport_price_exponent,
        energy_efficiency: info.energy_efficiency,
        price_object_rent: info.price_object_rent,
        price_object_scale_factor: info.price_object_scale_factor,
        price_parcel_rent: info.price_parcel_rent,
        price_group_create: info.price_group_create,
    }
}

/// Builds a [`ParcelInfo`] from a `ParcelProperties` parcel-data block.
fn parcel_info(data: &ParcelPropertiesParcelDataBlock) -> ParcelInfo {
    ParcelInfo {
        sequence_id: data.sequence_id,
        local_id: data.local_id,
        aabb_min: (data.aabb_min.x, data.aabb_min.y, data.aabb_min.z),
        aabb_max: (data.aabb_max.x, data.aabb_max.y, data.aabb_max.z),
        area: data.area,
        bitmap: data.bitmap.clone(),
        max_prims: data.max_prims,
        sim_wide_max_prims: data.sim_wide_max_prims,
        sim_wide_total_prims: data.sim_wide_total_prims,
        owner_id: data.owner_id,
        raw_parcel_flags: data.parcel_flags,
        music_url: trimmed_string(&data.music_url),
        media_url: trimmed_string(&data.media_url),
        media_id: data.media_id,
        media_auto_scale: data.media_auto_scale != 0,
    }
}

/// Builds a [`ChatMessage`] from a `ChatFromSimulator` chat-data block. The
/// `FromName` and `Message` strings carry trailing NUL padding, which is removed.
fn chat_message(data: &ChatFromSimulatorChatDataBlock) -> ChatMessage {
    ChatMessage {
        from_name: trimmed_string(&data.from_name),
        source_id: data.source_id,
        owner_id: data.owner_id,
        source_type: ChatSourceType::from_u8(data.source_type),
        chat_type: ChatType::from_u8(data.chat_type),
        audible: ChatAudible::from_u8(data.audible),
        position: (data.position.x, data.position.y, data.position.z),
        message: trimmed_string(&data.message),
    }
}

/// Computes the canonical 1:1 IM session id the viewer uses: the byte-wise XOR
/// of the two agent ids, except an IM to oneself (where the XOR would be nil)
/// uses the agent id directly.
fn compute_im_session_id(agent_id: Uuid, other: Uuid) -> Uuid {
    if agent_id == other {
        return agent_id;
    }
    let mut out = [0u8; 16];
    for (slot, (a, b)) in out
        .iter_mut()
        .zip(agent_id.as_bytes().iter().zip(other.as_bytes()))
    {
        *slot = a ^ b;
    }
    Uuid::from_bytes(out)
}

/// Builds an [`InstantMessage`] from an `ImprovedInstantMessage`'s agent-data and
/// message blocks. The `FromAgentName` and `Message` strings carry trailing NUL
/// padding, which is removed.
fn instant_message(
    agent_data: &ImprovedInstantMessageAgentDataBlock,
    block: &ImprovedInstantMessageMessageBlockBlock,
) -> InstantMessage {
    InstantMessage {
        from_agent_id: agent_data.agent_id,
        from_agent_name: trimmed_string(&block.from_agent_name),
        to_agent_id: block.to_agent_id,
        dialog: ImDialog::from_u8(block.dialog),
        from_group: block.from_group,
        region_id: block.region_id,
        position: (block.position.x, block.position.y, block.position.z),
        offline: block.offline != 0,
        timestamp: block.timestamp,
        id: block.id,
        parent_estate_id: block.parent_estate_id,
        message: trimmed_string(&block.message),
        binary_bucket: block.binary_bucket.clone(),
    }
}

/// Builds [`AvatarProperties`] from an `AvatarPropertiesReply` properties block.
fn avatar_properties(
    avatar_id: Uuid,
    data: &AvatarPropertiesReplyPropertiesDataBlock,
) -> AvatarProperties {
    AvatarProperties {
        avatar_id,
        image_id: data.image_id,
        fl_image_id: data.fl_image_id,
        partner_id: data.partner_id,
        about_text: trimmed_string(&data.about_text),
        fl_about_text: trimmed_string(&data.fl_about_text),
        born_on: trimmed_string(&data.born_on),
        profile_url: trimmed_string(&data.profile_url),
        charter_member: trimmed_string(&data.charter_member),
        flags: data.flags,
    }
}

/// Builds [`AvatarInterests`] from an `AvatarInterestsReply` properties block.
fn avatar_interests(
    avatar_id: Uuid,
    data: &AvatarInterestsReplyPropertiesDataBlock,
) -> AvatarInterests {
    AvatarInterests {
        avatar_id,
        want_to_mask: data.want_to_mask,
        want_to_text: trimmed_string(&data.want_to_text),
        skills_mask: data.skills_mask,
        skills_text: trimmed_string(&data.skills_text),
        languages_text: trimmed_string(&data.languages_text),
    }
}

/// Builds an [`AvatarGroupMembership`] from an `AvatarGroupsReply` group entry.
fn avatar_group(data: &AvatarGroupsReplyGroupDataBlock) -> AvatarGroupMembership {
    AvatarGroupMembership {
        group_id: data.group_id,
        group_name: trimmed_string(&data.group_name),
        group_title: trimmed_string(&data.group_title),
        group_powers: data.group_powers,
        accept_notices: data.accept_notices,
        group_insignia_id: data.group_insignia_id,
    }
}

/// Converts a login [`SkeletonFolder`] into an [`InventoryFolder`].
fn skeleton_folder(folder: &SkeletonFolder) -> InventoryFolder {
    InventoryFolder {
        folder_id: folder.folder_id,
        parent_id: folder.parent_id,
        name: folder.name.clone(),
        folder_type: folder.type_default,
        version: folder.version,
    }
}

/// Builds a [`Friend`] from a login `buddy-list` entry.
const fn friend(entry: &sl_wire::BuddyListEntry) -> Friend {
    Friend {
        id: entry.buddy_id,
        rights_granted: FriendRights(entry.rights_granted),
        rights_received: FriendRights(entry.rights_has),
    }
}

/// Builds [`ActiveGroup`] from an `AgentDataUpdate` block.
fn active_group(data: &AgentDataUpdateAgentDataBlock) -> ActiveGroup {
    ActiveGroup {
        agent_id: data.agent_id,
        first_name: trimmed_string(&data.first_name),
        last_name: trimmed_string(&data.last_name),
        group_title: trimmed_string(&data.group_title),
        active_group_id: data.active_group_id,
        group_powers: data.group_powers,
        group_name: trimmed_string(&data.group_name),
    }
}

/// Builds [`GroupMembership`] from an `AgentGroupDataUpdate` entry.
fn group_membership(data: &AgentGroupDataUpdateGroupDataBlock) -> GroupMembership {
    GroupMembership {
        group_id: data.group_id,
        group_powers: data.group_powers,
        accept_notices: data.accept_notices,
        group_insignia_id: data.group_insignia_id,
        contribution: data.contribution,
        group_name: trimmed_string(&data.group_name),
    }
}

/// Builds [`GroupMember`] from a `GroupMembersReply` entry.
fn group_member(data: &GroupMembersReplyMemberDataBlock) -> GroupMember {
    GroupMember {
        agent_id: data.agent_id,
        contribution: data.contribution,
        online_status: trimmed_string(&data.online_status),
        agent_powers: data.agent_powers,
        title: trimmed_string(&data.title),
        is_owner: data.is_owner,
    }
}

/// Builds [`GroupRole`] from a `GroupRoleDataReply` entry.
fn group_role(data: &GroupRoleDataReplyRoleDataBlock) -> GroupRole {
    GroupRole {
        role_id: data.role_id,
        name: trimmed_string(&data.name),
        title: trimmed_string(&data.title),
        description: trimmed_string(&data.description),
        powers: data.powers,
        members: data.members,
    }
}

/// Builds [`GroupTitle`] from a `GroupTitlesReply` entry.
fn group_title(data: &GroupTitlesReplyGroupDataBlock) -> GroupTitle {
    GroupTitle {
        title: trimmed_string(&data.title),
        role_id: data.role_id,
        selected: data.selected,
    }
}

/// Builds [`GroupProfile`] from a `GroupProfileReply` block.
fn group_profile(data: &GroupProfileReplyGroupDataBlock) -> GroupProfile {
    GroupProfile {
        group_id: data.group_id,
        name: trimmed_string(&data.name),
        charter: trimmed_string(&data.charter),
        show_in_list: data.show_in_list,
        member_title: trimmed_string(&data.member_title),
        powers: data.powers_mask,
        insignia_id: data.insignia_id,
        founder_id: data.founder_id,
        membership_fee: data.membership_fee,
        open_enrollment: data.open_enrollment,
        money: data.money,
        member_count: data.group_membership_count,
        role_count: data.group_roles_count,
        allow_publish: data.allow_publish,
        mature_publish: data.mature_publish,
        owner_role: data.owner_role,
    }
}

/// Builds [`GroupNotice`] from a `GroupNoticesListReply` entry.
fn group_notice(data: &GroupNoticesListReplyDataBlock) -> GroupNotice {
    GroupNotice {
        notice_id: data.notice_id,
        timestamp: data.timestamp,
        from_name: trimmed_string(&data.from_name),
        subject: trimmed_string(&data.subject),
        has_attachment: data.has_attachment,
        asset_type: data.asset_type,
    }
}

/// Builds a [`ScriptDialog`] value from a `ScriptDialog` message.
fn script_dialog(message: &sl_wire::messages::ScriptDialog) -> ScriptDialog {
    let data = &message.data;
    ScriptDialog {
        object_id: data.object_id,
        object_name: trimmed_string(&data.object_name),
        owner_first_name: trimmed_string(&data.first_name),
        owner_last_name: trimmed_string(&data.last_name),
        owner_id: message
            .owner_data
            .first()
            .map_or_else(Uuid::nil, |owner| owner.owner_id),
        message: trimmed_string(&data.message),
        chat_channel: data.chat_channel,
        image_id: data.image_id,
        buttons: message
            .buttons
            .iter()
            .map(|button| trimmed_string(&button.button_label))
            .collect(),
    }
}

/// Builds a [`ScriptPermissionRequest`] value from a `ScriptQuestion` message.
fn script_permission_request(
    message: &sl_wire::messages::ScriptQuestion,
) -> ScriptPermissionRequest {
    let data = &message.data;
    ScriptPermissionRequest {
        task_id: data.task_id,
        item_id: data.item_id,
        object_name: trimmed_string(&data.object_name),
        object_owner: trimmed_string(&data.object_owner),
        experience_id: message.experience.experience_id,
        permissions: ScriptPermissions(data.questions),
    }
}

/// Builds an [`InventoryFolder`] from an `InventoryDescendents` folder entry.
/// Such entries carry no per-folder version, so it is reported as `0`.
fn inventory_folder(data: &InventoryDescendentsFolderDataBlock) -> InventoryFolder {
    InventoryFolder {
        folder_id: data.folder_id,
        parent_id: data.parent_id,
        name: trimmed_string(&data.name),
        folder_type: data.r#type,
        version: 0,
    }
}

/// Builds an [`InventoryItem`] from an `InventoryDescendents` item entry.
fn inventory_item(data: &InventoryDescendentsItemDataBlock) -> InventoryItem {
    InventoryItem {
        item_id: data.item_id,
        folder_id: data.folder_id,
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.description),
        asset_id: data.asset_id,
        item_type: data.r#type,
        inv_type: data.inv_type,
        flags: data.flags,
        sale_type: data.sale_type,
        sale_price: data.sale_price,
        creation_date: data.creation_date,
        owner_id: data.owner_id,
        creator_id: data.creator_id,
        group_id: data.group_id,
        group_owned: data.group_owned,
        base_mask: data.base_mask,
        owner_mask: data.owner_mask,
        group_mask: data.group_mask,
        everyone_mask: data.everyone_mask,
        next_owner_mask: data.next_owner_mask,
    }
}

/// Builds a [`NeighborInfo`] from an `EnableSimulator` simulator-info block.
fn neighbor_info(info: &EnableSimulatorSimulatorInfoBlock) -> NeighborInfo {
    // IPPORT is big-endian (network order) on the wire, but the generated field
    // decoder reads it as a little-endian U16, so swap the bytes back to host
    // order here. (IPADDR is raw octets in order and needs no swap.)
    let port = info.port.swap_bytes();
    let sim = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(info.ip)), port);
    let (grid_x, grid_y) = handle_to_grid(info.handle);
    NeighborInfo {
        region_handle: info.handle,
        sim,
        grid_x,
        grid_y,
    }
}

/// Builds a [`MapRegionInfo`] from a `MapBlockReply` data block (with its
/// optional size block), or `None` for a sentinel/empty entry.
fn map_region_info(
    data: &MapBlockReplyDataBlock,
    size: Option<&MapBlockReplySizeBlock>,
) -> Option<MapRegionInfo> {
    // The map sends a sentinel block (0,0 / empty name) for "not found".
    if data.x == 0 && data.y == 0 {
        return None;
    }
    let name = trimmed_string(&data.name);
    if name.is_empty() {
        return None;
    }
    let grid_x = u32::from(data.x);
    let grid_y = u32::from(data.y);
    Some(MapRegionInfo {
        name,
        grid_x,
        grid_y,
        region_handle: grid_to_handle(grid_x, grid_y),
        maturity: Maturity::from_sim_access(data.access),
        region_flags: data.region_flags,
        size_x: size
            .map(|block| u32::from(block.size_x))
            .filter(|&value| value != 0)
            .unwrap_or(256),
        size_y: size
            .map(|block| u32::from(block.size_y))
            .filter(|&value| value != 0)
            .unwrap_or(256),
        agents: data.agents,
        map_image_id: data.map_image_id,
    })
}

/// Builds a [`MapItem`] from a `MapItemReply` data block. Coordinates are global
/// metres; `extra`/`extra2` are type-specific (see [`MapItem`]).
fn map_item(data: &sl_wire::messages::MapItemReplyDataBlock) -> MapItem {
    MapItem {
        global_x: data.x,
        global_y: data.y,
        id: data.id,
        extra: data.extra,
        extra2: data.extra2,
        name: trimmed_string(&data.name),
    }
}

/// Builds [`EstateInfo`] from an `estateupdateinfo` `EstateOwnerMessage`'s param
/// list (10 string parameters: name, owner, id, flags, sun, parent, covenant id,
/// covenant timestamp, "1", abuse email).
fn estate_info_from_params(params: &[EstateOwnerMessageParamListBlock]) -> Option<EstateInfo> {
    if params.len() < 8 {
        return None;
    }
    let text = |index: usize| {
        params
            .get(index)
            .map(|block| trimmed_string(&block.parameter))
            .unwrap_or_default()
    };
    Some(EstateInfo {
        estate_name: text(0),
        estate_owner: Uuid::parse_str(&text(1)).unwrap_or_else(|_| Uuid::nil()),
        estate_id: text(2).parse().unwrap_or(0),
        estate_flags: text(3).parse().unwrap_or(0),
        sun_position: text(4).parse().unwrap_or(0),
        parent_estate: text(5).parse().unwrap_or(0),
        covenant_id: Uuid::parse_str(&text(6)).unwrap_or_else(|_| Uuid::nil()),
        covenant_timestamp: text(7).parse().unwrap_or(0),
        abuse_email: text(9),
    })
}

/// Builds an [`Event::EstateAccessList`] from a `setaccess` `EstateOwnerMessage`.
/// `param[0]` is the estate id, `param[1]` the single-category code bit,
/// `param[2..=5]` per-category counts, and `param[6..]` the member ids — each a
/// raw 16-byte UUID (not a string).
fn estate_access_from_params(params: &[EstateOwnerMessageParamListBlock]) -> Option<Event> {
    if params.len() < 6 {
        return None;
    }
    let text = |index: usize| {
        params
            .get(index)
            .map(|block| trimmed_string(&block.parameter))
            .unwrap_or_default()
    };
    let estate_id = text(0).parse().unwrap_or(0);
    let code: u32 = text(1).parse().unwrap_or(0);
    let kind = if code & 1 != 0 {
        EstateAccessKind::AllowedAgents
    } else if code & 2 != 0 {
        EstateAccessKind::AllowedGroups
    } else if code & 4 != 0 {
        EstateAccessKind::BannedAgents
    } else if code & 8 != 0 {
        EstateAccessKind::Managers
    } else {
        return None;
    };
    let members = params
        .iter()
        .skip(6)
        .filter_map(|block| {
            let bytes = block.parameter.get(..16)?;
            Uuid::from_slice(bytes).ok()
        })
        .collect();
    Some(Event::EstateAccessList {
        estate_id,
        kind,
        members,
    })
}

/// Extracts the destination UDP address and seed capability from a CAPS
/// `TeleportFinish` event body: `{ "Info": [ { "SimIP": <binary 4 bytes>,
/// "SimPort": <integer>, "SeedCapability": <string>, … } ] }`. The CAPS `SimPort`
/// is a plain host-order integer port (unlike the byte-swapped generated-UDP field).
fn teleport_finish_from_llsd(body: &Llsd) -> Option<(SocketAddr, String)> {
    let info = body.get("Info").and_then(|info| info.index(0))?;
    let octets: [u8; 4] = info
        .get("SimIP")
        .and_then(Llsd::as_binary)?
        .try_into()
        .ok()?;
    let port = u16::try_from(info.get("SimPort").and_then(Llsd::as_i32)?).ok()?;
    let seed = info
        .get("SeedCapability")
        .and_then(Llsd::as_str)
        .unwrap_or("")
        .to_owned();
    Some((
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        seed,
    ))
}

/// Extracts a neighbour's region handle and simulator address from a CAPS
/// `EnableSimulator` event body: `{ "SimulatorInfo": [{ "Handle": <u64 binary>,
/// "IP": <4 bytes>, "Port": <integer> }] }`. Unlike the UDP message the port is
/// a plain integer (no byte swap).
fn enable_simulator_from_caps_llsd(body: &Llsd) -> Option<(u64, SocketAddr)> {
    let info = body.get("SimulatorInfo").and_then(|s| s.index(0))?;
    let handle = info.get("Handle").map(llsd_u64)?;
    let octets: [u8; 4] = info.get("IP").and_then(Llsd::as_binary)?.try_into().ok()?;
    let port = u16::try_from(info.get("Port").and_then(Llsd::as_i32)?).ok()?;
    Some((
        handle,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
    ))
}

/// Extracts the destination region handle, simulator address and seed capability
/// from a CAPS `CrossedRegion` event body: the `RegionData` array carries
/// `RegionHandle` (u64), `SimIP` (4 bytes), `SimPort` (plain integer, no swap)
/// and `SeedCapability` (url).
fn crossed_region_from_caps_llsd(body: &Llsd) -> Option<(u64, SocketAddr, String)> {
    let region = body.get("RegionData").and_then(|r| r.index(0))?;
    let handle = region.get("RegionHandle").map(llsd_u64)?;
    let octets: [u8; 4] = region
        .get("SimIP")
        .and_then(Llsd::as_binary)?
        .try_into()
        .ok()?;
    let port = u16::try_from(region.get("SimPort").and_then(Llsd::as_i32)?).ok()?;
    let seed = region
        .get("SeedCapability")
        .and_then(Llsd::as_str)
        .unwrap_or("")
        .to_owned();
    Some((
        handle,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        seed,
    ))
}

/// Extracts the child region's simulator address and seed capability from a CAPS
/// `EstablishAgentCommunication` event body: `{ "sim-ip-and-port": "ip:port",
/// "seed-capability": url }`.
fn establish_agent_communication_from_llsd(body: &Llsd) -> Option<(SocketAddr, String)> {
    let sim = body.get("sim-ip-and-port").and_then(Llsd::as_str)?;
    let sim: SocketAddr = sim.parse().ok()?;
    let seed = body
        .get("seed-capability")
        .and_then(Llsd::as_str)
        .unwrap_or("")
        .to_owned();
    Some((sim, seed))
}

/// Builds a [`ParcelInfo`] from a CAPS `ParcelProperties` event body.
fn parcel_info_from_llsd(body: &Llsd) -> Option<ParcelInfo> {
    let data = body
        .get("ParcelData")
        .and_then(|parcel_data| parcel_data.index(0))?;
    Some(ParcelInfo {
        sequence_id: data.get("SequenceID").and_then(Llsd::as_i32).unwrap_or(0),
        local_id: data.get("LocalID").and_then(Llsd::as_i32).unwrap_or(0),
        aabb_min: vec3_from_llsd(data.get("AABBMin")),
        aabb_max: vec3_from_llsd(data.get("AABBMax")),
        area: data.get("Area").and_then(Llsd::as_i32).unwrap_or(0),
        bitmap: data
            .get("Bitmap")
            .and_then(Llsd::as_binary)
            .map(<[u8]>::to_vec)
            .unwrap_or_default(),
        max_prims: data.get("MaxPrims").and_then(Llsd::as_i32).unwrap_or(0),
        sim_wide_max_prims: data
            .get("SimWideMaxPrims")
            .and_then(Llsd::as_i32)
            .unwrap_or(0),
        sim_wide_total_prims: data
            .get("SimWideTotalPrims")
            .and_then(Llsd::as_i32)
            .unwrap_or(0),
        owner_id: data
            .get("OwnerID")
            .and_then(Llsd::as_uuid)
            .unwrap_or_else(Uuid::nil),
        // OpenSim encodes the `uint` ParcelFlags as a 4-byte binary LLSD element,
        // so read it tolerantly (binary / integer / string).
        raw_parcel_flags: data.get("ParcelFlags").map_or(0, llsd_u32),
        music_url: data
            .get("MusicURL")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        media_url: data
            .get("MediaURL")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        media_id: data
            .get("MediaID")
            .and_then(Llsd::as_uuid)
            .unwrap_or_else(Uuid::nil),
        // OpenSim encodes MediaAutoScale as an LLSD boolean; `as_bool` also
        // tolerates the integer form.
        media_auto_scale: data
            .get("MediaAutoScale")
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
    })
}

/// Reads a three-component vector (`[x, y, z]` reals) from an LLSD array.
fn vec3_from_llsd(value: Option<&Llsd>) -> (f32, f32, f32) {
    let component = |index: usize| {
        value
            .and_then(|vector| vector.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    (component(0), component(1), component(2))
}

/// Reads a UUID from an LLSD map member, defaulting to nil.
fn uuid_member(map: &Llsd, key: &str) -> Uuid {
    map.get(key)
        .and_then(Llsd::as_uuid)
        .unwrap_or_else(Uuid::nil)
}

/// Reads an `i32` from an LLSD map member, defaulting to `0`.
fn i32_member(map: &Llsd, key: &str) -> i32 {
    map.get(key).and_then(Llsd::as_i32).unwrap_or(0)
}

/// Reads a string from an LLSD map member, defaulting to empty.
fn string_member(map: &Llsd, key: &str) -> String {
    map.get(key).and_then(Llsd::as_str).unwrap_or("").to_owned()
}

/// Decodes a `u64` from an LLSD value as the viewer's `ll_U64_from_sd` does:
/// from an 8-byte big-endian binary, a hex/decimal string, or an integer.
/// Reads a `u32` from an LLSD value that may be a 4-byte big-endian binary
/// element (how OpenSim encodes `uint` fields such as `ParcelFlags`), an
/// integer, or a decimal/hex string.
fn llsd_u32(value: &Llsd) -> u32 {
    match value {
        Llsd::Binary(bytes) if bytes.len() >= 4 => bytes
            .iter()
            .take(4)
            .fold(0u32, |acc, &byte| (acc << 8) | u32::from(byte)),
        Llsd::String(s) => {
            let trimmed = s.trim().trim_start_matches("0x");
            u32::from_str_radix(trimmed, 16)
                .ok()
                .or_else(|| s.trim().parse().ok())
                .unwrap_or(0)
        }
        Llsd::Integer(i) => u32::try_from(*i).unwrap_or(0),
        _ => 0,
    }
}

/// Reads a `u64` from an LLSD value that may be an 8-byte big-endian binary
/// element (how OpenSim encodes `U64` region handles), an integer, or a
/// decimal/hex string.
fn llsd_u64(value: &Llsd) -> u64 {
    match value {
        Llsd::Binary(bytes) if bytes.len() >= 8 => bytes
            .iter()
            .take(8)
            .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte)),
        Llsd::String(s) => {
            let trimmed = s.trim().trim_start_matches("0x");
            u64::from_str_radix(trimmed, 16)
                .ok()
                .or_else(|| s.trim().parse().ok())
                .unwrap_or(0)
        }
        Llsd::Integer(i) => u64::try_from(*i).unwrap_or(0),
        _ => 0,
    }
}

/// Decodes the CAPS event-queue `AgentGroupDataUpdate` event (the modern
/// delivery of the agent's group memberships) into [`Event::GroupMemberships`].
/// The LLSD mirrors the UDP message: a `GroupData` array of per-group maps.
fn group_memberships_from_caps_llsd(body: &Llsd) -> Option<Event> {
    // The sim sometimes double-wraps the payload in a nested `body`.
    let body = body.get("body").unwrap_or(body);
    let groups = body.get("GroupData").and_then(Llsd::as_array)?;
    let memberships = groups
        .iter()
        .filter_map(|group| {
            let group_id = group.get("GroupID").and_then(Llsd::as_uuid)?;
            Some(GroupMembership {
                group_id,
                group_powers: group.get("GroupPowers").map_or(0, llsd_u64),
                accept_notices: group
                    .get("AcceptNotices")
                    .and_then(Llsd::as_bool)
                    .unwrap_or(false),
                group_insignia_id: group
                    .get("GroupInsigniaID")
                    .and_then(Llsd::as_uuid)
                    .unwrap_or_else(Uuid::nil),
                contribution: group
                    .get("Contribution")
                    .and_then(Llsd::as_i32)
                    .unwrap_or(0),
                group_name: group
                    .get("GroupName")
                    .and_then(Llsd::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect();
    Some(Event::GroupMemberships(memberships))
}

/// Decodes a `GroupMemberData` capability response into [`Event::GroupMembers`].
/// The LLSD is `{ group_id, members: { <id>: {...} }, titles: [...],
/// defaults: { default_powers } }`; per-member fields fall back to the defaults.
fn group_members_from_caps_llsd(body: &Llsd) -> Option<Event> {
    let group_id = body.get("group_id").and_then(Llsd::as_uuid)?;
    let Llsd::Map(members) = body.get("members")? else {
        return None;
    };
    let titles = body.get("titles").and_then(Llsd::as_array);
    let default_title = titles
        .and_then(|t| t.first())
        .and_then(Llsd::as_str)
        .unwrap_or_default();
    let default_powers = body
        .get("defaults")
        .and_then(|d| d.get("default_powers"))
        .map_or(0, llsd_u64);

    let mut roster: Vec<GroupMember> = members
        .iter()
        .filter_map(|(member_id, info)| {
            let agent_id = Uuid::parse_str(member_id).ok()?;
            let title = info
                .get("title")
                .and_then(Llsd::as_i32)
                .and_then(|index| titles?.get(usize::try_from(index).ok()?))
                .and_then(Llsd::as_str)
                .unwrap_or(default_title)
                .to_owned();
            Some(GroupMember {
                agent_id,
                contribution: info
                    .get("donated_square_meters")
                    .and_then(Llsd::as_i32)
                    .unwrap_or(0),
                online_status: info
                    .get("last_login")
                    .and_then(Llsd::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                agent_powers: info.get("powers").map_or(default_powers, llsd_u64),
                title,
                is_owner: info.get("owner").is_some(),
            })
        })
        .collect();
    // The members map is unordered; sort by id for deterministic output.
    roster.sort_by_key(|member| member.agent_id);
    let member_count = i32::try_from(roster.len()).unwrap_or(i32::MAX);
    Some(Event::GroupMembers {
        group_id,
        request_id: Uuid::nil(),
        member_count,
        members: roster,
    })
}

/// Parses a `FetchInventoryDescendents2` CAPS response body into one
/// [`Event::InventoryDescendents`] per returned folder. The HTTP response shape
/// differs from the UDP `InventoryDescendents`, but yields the same value types.
fn inventory_descendents_from_llsd(body: &Llsd) -> Vec<Event> {
    let Some(folders) = body.get("folders").and_then(Llsd::as_array) else {
        return Vec::new();
    };
    folders
        .iter()
        .map(|folder| {
            let categories = folder
                .get("categories")
                .and_then(Llsd::as_array)
                .unwrap_or(&[]);
            let items = folder.get("items").and_then(Llsd::as_array).unwrap_or(&[]);
            Event::InventoryDescendents {
                folder_id: uuid_member(folder, "folder_id"),
                version: i32_member(folder, "version"),
                descendents: i32_member(folder, "descendents"),
                folders: categories.iter().map(inventory_folder_from_llsd).collect(),
                items: items.iter().map(inventory_item_from_llsd).collect(),
            }
        })
        .collect()
}

/// Builds an [`InventoryFolder`] from a CAPS `categories` entry.
fn inventory_folder_from_llsd(category: &Llsd) -> InventoryFolder {
    InventoryFolder {
        folder_id: uuid_member(category, "category_id"),
        parent_id: uuid_member(category, "parent_id"),
        name: string_member(category, "name"),
        folder_type: i8::try_from(i32_member(category, "type_default")).unwrap_or(-1),
        version: i32_member(category, "version"),
    }
}

/// Builds an [`InventoryItem`] from a CAPS `items` entry (with nested
/// `permissions` and `sale_info` maps).
fn inventory_item_from_llsd(item: &Llsd) -> InventoryItem {
    let permissions = item.get("permissions");
    let sale_info = item.get("sale_info");
    let perm = |key: &str| {
        permissions
            .map_or(0, |p| i32_member(p, key))
            .cast_unsigned()
    };
    let perm_uuid = |key: &str| permissions.map_or_else(Uuid::nil, |p| uuid_member(p, key));
    InventoryItem {
        item_id: uuid_member(item, "item_id"),
        folder_id: uuid_member(item, "parent_id"),
        name: string_member(item, "name"),
        description: string_member(item, "desc"),
        asset_id: uuid_member(item, "asset_id"),
        item_type: i8::try_from(i32_member(item, "type")).unwrap_or(-1),
        inv_type: i8::try_from(i32_member(item, "inv_type")).unwrap_or(-1),
        flags: i32_member(item, "flags").cast_unsigned(),
        sale_type: sale_info.map_or(0, |s| u8::try_from(i32_member(s, "sale_type")).unwrap_or(0)),
        sale_price: sale_info.map_or(0, |s| i32_member(s, "sale_price")),
        creation_date: i32_member(item, "created_at"),
        owner_id: perm_uuid("owner_id"),
        creator_id: perm_uuid("creator_id"),
        group_id: perm_uuid("group_id"),
        group_owned: permissions
            .and_then(|p| p.get("is_owner_group"))
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        base_mask: perm("base_mask"),
        owner_mask: perm("owner_mask"),
        group_mask: perm("group_mask"),
        everyone_mask: perm("everyone_mask"),
        next_owner_mask: perm("next_owner_mask"),
    }
}

// ---------------------------------------------------------------------------
// Object / scene graph (#16): decoders for the packed `ObjectData`/`Data` blobs.
// ---------------------------------------------------------------------------

/// The `CompressedFlags` bitfield carried in an `ObjectUpdateCompressed` blob,
/// gating which optional fields follow (mirrors LL's `CompressedFlags`).
const COMPRESSED_SCRATCHPAD: u32 = 0x01;
/// The object carries a tree species byte.
const COMPRESSED_TREE: u32 = 0x02;
/// The object has floating text (`llSetText`).
const COMPRESSED_HAS_TEXT: u32 = 0x04;
/// The object is linked to a parent (a `ParentID` follows).
const COMPRESSED_HAS_PARENT: u32 = 0x20;
/// The object has a non-zero angular velocity (a vector follows).
const COMPRESSED_HAS_ANGULAR_VELOCITY: u32 = 0x80;
/// The object has a media URL.
const COMPRESSED_MEDIA_URL: u32 = 0x200;

/// A zero [`Vector`], used as the fall-back for absent/short motion fields.
const ZERO_VECTOR: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// Dequantizes a 16-bit fixed-point value spanning `[lower, upper]` back to an
/// `f32`, matching LL's `U16_to_F32` (including its snap-to-zero of values
/// within one quantum of zero).
fn u16_to_f32(value: u16, lower: f32, upper: f32) -> f32 {
    let range = upper - lower;
    let result = f32::from(value) / f32::from(u16::MAX) * range + lower;
    let max_error = range / f32::from(u16::MAX);
    if result.abs() < max_error {
        0.0
    } else {
        result
    }
}

/// Reads three consecutive 16-bit-quantized floats (each spanning
/// `[-range, range]`) as a [`Vector`].
fn read_quantized_vector(reader: &mut Reader<'_>, range: f32) -> Result<Vector, WireError> {
    let x = u16_to_f32(reader.u16()?, -range, range);
    let y = u16_to_f32(reader.u16()?, -range, range);
    let z = u16_to_f32(reader.u16()?, -range, range);
    Ok(Vector { x, y, z })
}

/// Packs a unit quaternion into the three-float form a `MultipleObjectUpdate`
/// `Data` blob carries (LL's `LLQuaternion::packToVector3`): normalize, then if
/// the real component is negative negate the vector part so the receiver can
/// reconstruct `w = sqrt(1 - x² - y² - z²) >= 0`.
fn pack_quaternion_to_vec3(rotation: &Rotation) -> [f32; 3] {
    let Rotation { x, y, z, s } = *rotation;
    let magnitude = s.mul_add(s, z.mul_add(z, x.mul_add(x, y * y))).sqrt();
    let (mut x, mut y, mut z) = if magnitude > f32::EPSILON {
        (x / magnitude, y / magnitude, z / magnitude)
    } else {
        (x, y, z)
    };
    if s < 0.0 {
        x = -x;
        y = -y;
        z = -z;
    }
    [x, y, z]
}

/// A zero/identity [`ObjectMotion`], used when a motion blob is malformed.
const fn zero_motion() -> ObjectMotion {
    ObjectMotion {
        position: ZERO_VECTOR,
        velocity: ZERO_VECTOR,
        acceleration: ZERO_VECTOR,
        rotation: IDENTITY_ROTATION,
        angular_velocity: ZERO_VECTOR,
    }
}

/// Decodes the full-precision `ObjectData` blob of an `ObjectUpdate` into an
/// [`ObjectMotion`]. Avatar variants (length 76/140) carry a 16-byte collision
/// plane prefix, which is skipped. Returns a zero motion on a short/garbled
/// blob rather than erroring (best-effort, no panic).
fn full_object_motion(blob: &[u8]) -> ObjectMotion {
    full_object_motion_inner(blob).unwrap_or_else(|_ignored| zero_motion())
}

/// The fallible inner of [`full_object_motion`].
fn full_object_motion_inner(blob: &[u8]) -> Result<ObjectMotion, WireError> {
    let mut reader = Reader::new(blob);
    if matches!(blob.len(), 76 | 140) {
        // Avatar collision plane (LLVector4) prefix — read and discard.
        let _plane = reader.vector4()?;
    }
    let position = reader.vector3()?;
    let velocity = reader.vector3()?;
    let acceleration = reader.vector3()?;
    // Rotation is a packed quaternion (three floats, w reconstructed).
    let rotation = reader.quaternion()?;
    let angular_velocity = reader.vector3()?;
    Ok(ObjectMotion {
        position,
        velocity,
        acceleration,
        rotation,
        angular_velocity,
    })
}

/// A decoded `ImprovedTerseObjectUpdate` entry: the object's local id, its state
/// byte, and its new motion.
struct TerseUpdate {
    /// The object's region-local id.
    local_id: u32,
    /// The object/attachment state byte.
    state: u8,
    /// The object's new kinematic state (position full precision; velocity,
    /// acceleration, rotation, and angular velocity 16-bit quantized).
    motion: ObjectMotion,
}

/// Decodes the `Data` blob of an `ImprovedTerseObjectUpdate` entry. Returns
/// `None` on a short/garbled blob.
fn terse_update(blob: &[u8]) -> Option<TerseUpdate> {
    let mut reader = Reader::new(blob);
    let local_id = reader.u32().ok()?;
    let state = reader.u8().ok()?;
    let has_collision_plane = reader.u8().ok()? != 0;
    if has_collision_plane {
        // Avatar collision plane (LLVector4) — read and discard.
        let _plane = reader.vector4().ok()?;
    }
    let position = reader.vector3().ok()?;
    let velocity = read_quantized_vector(&mut reader, 128.0).ok()?;
    let acceleration = read_quantized_vector(&mut reader, 64.0).ok()?;
    // Rotation: four explicit 16-bit components (x, y, z, w) — not packed.
    let rot_x = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rot_y = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rot_z = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rot_s = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rotation = Rotation {
        x: rot_x,
        y: rot_y,
        z: rot_z,
        s: rot_s,
    };
    let angular_velocity = read_quantized_vector(&mut reader, 64.0).ok()?;
    Some(TerseUpdate {
        local_id,
        state,
        motion: ObjectMotion {
            position,
            velocity,
            acceleration,
            rotation,
            angular_velocity,
        },
    })
}

/// Reads a NUL-terminated UTF-8 string from `reader` (consuming the terminator).
fn read_nul_string(reader: &mut Reader<'_>) -> Option<String> {
    let mut bytes = Vec::new();
    loop {
        let byte = reader.u8().ok()?;
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Builds an [`Object`] from a full `ObjectUpdate` object-data block.
fn object_from_full_update(block: &ObjectUpdateObjectDataBlock, region_handle: u64) -> Object {
    Object {
        region_handle,
        local_id: block.id,
        full_id: block.full_id,
        parent_id: block.parent_id,
        pcode: block.p_code,
        state: block.state,
        crc: block.crc,
        material: block.material,
        click_action: block.click_action,
        update_flags: block.update_flags,
        scale: block.scale.clone(),
        motion: full_object_motion(&block.object_data),
        owner_id: block.owner_id,
        sound: block.sound,
        gain: block.gain,
        sound_flags: block.flags,
        sound_radius: block.radius,
        text: trimmed_string(&block.text),
        text_color: block.text_color,
        name_value: trimmed_string(&block.name_value),
        media_url: trimmed_string(&block.media_url),
        texture_entry: block.texture_entry.clone(),
        extra_params: block.extra_params.clone(),
        properties: None,
    }
}

/// Decodes the packed `Data` blob of an `ObjectUpdateCompressed` entry into an
/// [`Object`]. The reliable fixed prefix (identity, scale, position, rotation,
/// flags, owner, optional angular velocity / parent / tree, floating text, and
/// media URL) is decoded; the trailing variable-size fields (particle systems,
/// extra params, sound, name-values, shape, texture entry) are not, as walking
/// past the length-prefix-less legacy particle block is not possible from the
/// stream alone. Returns `None` on a short/garbled blob.
fn compressed_object(blob: &[u8], region_handle: u64, update_flags: u32) -> Option<Object> {
    let mut reader = Reader::new(blob);
    let full_id = reader.uuid().ok()?;
    let local_id = reader.u32().ok()?;
    let pcode = reader.u8().ok()?;
    let state = reader.u8().ok()?;
    let crc = reader.u32().ok()?;
    let material = reader.u8().ok()?;
    let click_action = reader.u8().ok()?;
    let scale = reader.vector3().ok()?;
    let position = reader.vector3().ok()?;
    // Rotation is a packed quaternion (three floats, w reconstructed).
    let rotation = reader.quaternion().ok()?;
    let cflags = reader.u32().ok()?;
    let owner_id = reader.uuid().ok()?;
    let angular_velocity = if cflags & COMPRESSED_HAS_ANGULAR_VELOCITY != 0 {
        reader.vector3().ok()?
    } else {
        ZERO_VECTOR
    };
    let parent_id = if cflags & COMPRESSED_HAS_PARENT != 0 {
        reader.u32().ok()?
    } else {
        0
    };
    if cflags & COMPRESSED_TREE != 0 {
        let _tree_species = reader.u8().ok()?;
    } else if cflags & COMPRESSED_SCRATCHPAD != 0 {
        let size = reader.u32().ok()?;
        let _scratch = reader.take(usize::try_from(size).ok()?).ok()?;
    }
    let text = if cflags & COMPRESSED_HAS_TEXT != 0 {
        let text = read_nul_string(&mut reader)?;
        let _color = reader.take_array::<4>().ok()?;
        text
    } else {
        String::new()
    };
    let media_url = if cflags & COMPRESSED_MEDIA_URL != 0 {
        read_nul_string(&mut reader)?
    } else {
        String::new()
    };
    Some(Object {
        region_handle,
        local_id,
        full_id,
        parent_id,
        pcode,
        state,
        crc,
        material,
        click_action,
        update_flags,
        scale,
        motion: ObjectMotion {
            position,
            velocity: ZERO_VECTOR,
            acceleration: ZERO_VECTOR,
            rotation,
            angular_velocity,
        },
        owner_id,
        sound: Uuid::nil(),
        gain: 0.0,
        sound_flags: 0,
        sound_radius: 0.0,
        text,
        text_color: [0; 4],
        name_value: String::new(),
        media_url,
        texture_entry: Vec::new(),
        extra_params: Vec::new(),
        properties: None,
    })
}

/// Builds an [`ObjectProperties`] from an `ObjectProperties` object-data block.
fn object_properties(block: &ObjectPropertiesObjectDataBlock) -> ObjectProperties {
    ObjectProperties {
        object_id: block.object_id,
        creator_id: block.creator_id,
        owner_id: block.owner_id,
        group_id: block.group_id,
        last_owner_id: block.last_owner_id,
        creation_date: block.creation_date,
        base_mask: block.base_mask,
        owner_mask: block.owner_mask,
        group_mask: block.group_mask,
        everyone_mask: block.everyone_mask,
        next_owner_mask: block.next_owner_mask,
        ownership_cost: block.ownership_cost,
        sale_type: block.sale_type,
        sale_price: block.sale_price,
        category: block.category,
        name: trimmed_string(&block.name),
        description: trimmed_string(&block.description),
        touch_name: trimmed_string(&block.touch_name),
        sit_name: trimmed_string(&block.sit_name),
    }
}
