//! The sans-I/O **simulator-side** session state machine — the inverse of the
//! client [`Session`](crate::Session).
//!
//! A [`SimSession`] models one simulator's view of a single agent's UDP circuit.
//! Where the client [`Session`] *sends* the circuit-bootstrap, keep-alive and
//! command messages and *decodes* what the simulator pushes, a [`SimSession`]
//! does the mirror image: it accepts the circuit (`UseCircuitCode` +
//! `CompleteAgentMovement`), keeps the link healthy (ping replies, owed
//! acknowledgements, reliable retransmission, inactivity timeout), decodes the
//! client-only messages into a [`ServerEvent`], and exposes a typed API to push
//! server messages (`RegionHandshake`, `ChatFromSimulator`, `ObjectUpdate`,
//! `LayerData`, …) and to enqueue CAPS [`EventQueueGet`](crate::build_event_queue_response)
//! events.
//!
//! It performs no I/O and never reads a clock: feed it inbound datagrams and the
//! current [`Instant`] through the `handle_*` methods, and drain datagrams,
//! timeouts and events through the `poll_*` methods. It reuses the symmetric
//! `sl-wire` framing/ack/zerocode machinery (`encode_datagram`/`parse_datagram`/
//! `PacketFlags`/`PacketAck`), so a [`SimSession`] and a client [`Session`] can
//! be driven against each other through the real wire path.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_types::chat::ChatChannel;
use sl_types::key::{
    AgentKey, ExperienceKey, FriendKey, GroupKey, GroupRoleKey, InventoryFolderKey,
    InventoryItemOrFolderKey, InventoryKey, ObjectKey, OwnerKey, ParcelKey, TextureKey,
};
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::{GridCoordinates, RegionCoordinates};
use sl_wire::messages::{
    AcceptCallingCard, AcceptCallingCardAgentDataBlock, AcceptCallingCardTransactionBlockBlock,
    DeclineCallingCard, DeclineCallingCardAgentDataBlock, DeclineCallingCardTransactionBlockBlock,
    OfferCallingCard, OfferCallingCardAgentBlockBlock, OfferCallingCardAgentDataBlock,
    TerminateFriendship, TerminateFriendshipAgentDataBlock, TerminateFriendshipExBlockBlock,
};
use sl_wire::messages::{
    AgentAlertMessage, AgentAlertMessageAgentDataBlock, AgentAlertMessageAlertDataBlock,
    AlertMessage, AlertMessageAgentInfoBlock, AlertMessageAlertDataBlock,
    AlertMessageAlertInfoBlock, CameraConstraint, CameraConstraintCameraCollidePlaneBlock,
    HealthMessage, HealthMessageHealthDataBlock, LandStatReply, LandStatReplyReportDataBlock,
    LandStatReplyRequestDataBlock, MeanCollisionAlert, MeanCollisionAlertMeanCollisionBlock,
    ViewerFrozenMessage, ViewerFrozenMessageFrozenDataBlock,
};
use sl_wire::messages::{
    AgentMovementComplete, AgentMovementCompleteAgentDataBlock, AgentMovementCompleteDataBlock,
    AgentMovementCompleteSimDataBlock, AvatarPickerReply, AvatarPickerReplyAgentDataBlock,
    AvatarPickerReplyDataBlock, ChatFromSimulator, ChatFromSimulatorChatDataBlock,
    CoarseLocationUpdate, CoarseLocationUpdateAgentDataBlock, CoarseLocationUpdateIndexBlock,
    CoarseLocationUpdateLocationBlock, CompletePingCheck, CompletePingCheckPingIDBlock,
    DirClassifiedReply, DirClassifiedReplyAgentDataBlock, DirClassifiedReplyQueryDataBlock,
    DirClassifiedReplyQueryRepliesBlock, DirClassifiedReplyStatusDataBlock, DirEventsReply,
    DirEventsReplyAgentDataBlock, DirEventsReplyQueryDataBlock, DirEventsReplyQueryRepliesBlock,
    DirEventsReplyStatusDataBlock, DirGroupsReply, DirGroupsReplyAgentDataBlock,
    DirGroupsReplyQueryDataBlock, DirGroupsReplyQueryRepliesBlock, DirLandReply,
    DirLandReplyAgentDataBlock, DirLandReplyQueryDataBlock, DirLandReplyQueryRepliesBlock,
    DirPeopleReply, DirPeopleReplyAgentDataBlock, DirPeopleReplyQueryDataBlock,
    DirPeopleReplyQueryRepliesBlock, DirPlacesReply, DirPlacesReplyAgentDataBlock,
    DirPlacesReplyQueryDataBlock, DirPlacesReplyQueryRepliesBlock, DirPlacesReplyStatusDataBlock,
    EstateCovenantReply, EstateCovenantReplyDataBlock, EstateOwnerMessageParamListBlock,
    EventInfoReply, EventInfoReplyAgentDataBlock, EventInfoReplyEventDataBlock, FindAgent,
    FindAgentAgentBlockBlock, FindAgentLocationBlockBlock, LogoutReply, LogoutReplyAgentDataBlock,
    PacketAck, PlacesReply, PlacesReplyAgentDataBlock, PlacesReplyQueryDataBlock,
    PlacesReplyTransactionDataBlock, StartPingCheck, StartPingCheckPingIDBlock, UUIDGroupNameReply,
    UUIDGroupNameReplyUUIDNameBlockBlock, UUIDNameReply, UUIDNameReplyUUIDNameBlockBlock,
    ViewerEffect as ViewerEffectMessage, ViewerEffectAgentDataBlock, ViewerEffectEffectBlock,
};
use sl_wire::messages::{
    ClearFollowCamProperties, ClearFollowCamPropertiesObjectDataBlock, ScriptControlChange,
    ScriptControlChangeDataBlock, SetFollowCamProperties,
    SetFollowCamPropertiesCameraPropertyBlock, SetFollowCamPropertiesObjectDataBlock,
};
use sl_wire::messages::{
    DeRezAck, DeRezAckTransactionDataBlock, ForceObjectSelect, ForceObjectSelectDataBlock,
    ForceObjectSelectHeaderBlock, GrantGodlikePowers, GrantGodlikePowersAgentDataBlock,
    GrantGodlikePowersGrantDataBlock, MoveInventoryItem, MoveInventoryItemAgentDataBlock,
    MoveInventoryItemInventoryDataBlock, RemoveInventoryFolder,
    RemoveInventoryFolderAgentDataBlock, RemoveInventoryFolderFolderDataBlock, RemoveInventoryItem,
    RemoveInventoryItemAgentDataBlock, RemoveInventoryItemInventoryDataBlock,
    RemoveInventoryObjects, RemoveInventoryObjectsAgentDataBlock,
    RemoveInventoryObjectsFolderDataBlock, RemoveInventoryObjectsItemDataBlock, ReplyTaskInventory,
    ReplyTaskInventoryInventoryDataBlock, UserInfoReply, UserInfoReplyAgentDataBlock,
    UserInfoReplyUserDataBlock,
};
use sl_wire::messages::{
    Error as ErrorWire, ErrorAgentDataBlock, ErrorDataBlock,
    FeatureDisabled as FeatureDisabledWire, FeatureDisabledFailureInfoBlock, KickUser,
    KickUserTargetBlockBlock, KickUserUserInfoBlock,
};
use sl_wire::messages::{
    GenericMessage as GenericMessageWire, GenericMessageAgentDataBlock,
    GenericMessageMethodDataBlock, GenericMessageParamListBlock,
    GenericStreamingMessage as GenericStreamingMessageWire, GenericStreamingMessageDataBlockBlock,
    GenericStreamingMessageMethodDataBlock, LargeGenericMessage as LargeGenericMessageWire,
    LargeGenericMessageAgentDataBlock, LargeGenericMessageMethodDataBlock,
    LargeGenericMessageParamListBlock, SimStats, SimStatsPidStatBlock, SimStatsRegionBlock,
    SimStatsRegionInfoBlock, SimStatsStatBlock, SimulatorViewerTimeMessage,
    SimulatorViewerTimeMessageTimeInfoBlock,
};
use sl_wire::messages::{
    GroupAccountDetailsReply, GroupAccountDetailsReplyAgentDataBlock,
    GroupAccountDetailsReplyHistoryDataBlock, GroupAccountDetailsReplyMoneyDataBlock,
    GroupAccountSummaryReply, GroupAccountSummaryReplyAgentDataBlock,
    GroupAccountSummaryReplyMoneyDataBlock, GroupAccountTransactionsReply,
    GroupAccountTransactionsReplyAgentDataBlock, GroupAccountTransactionsReplyHistoryDataBlock,
    GroupAccountTransactionsReplyMoneyDataBlock, GroupActiveProposalItemReply,
    GroupActiveProposalItemReplyAgentDataBlock, GroupActiveProposalItemReplyProposalDataBlock,
    GroupActiveProposalItemReplyTransactionDataBlock, GroupVoteHistoryItemReply,
    GroupVoteHistoryItemReplyAgentDataBlock, GroupVoteHistoryItemReplyHistoryItemDataBlock,
    GroupVoteHistoryItemReplyTransactionDataBlock, GroupVoteHistoryItemReplyVoteItemBlock,
};
use sl_wire::messages::{
    ObjectAnimation as ObjectAnimationWire, ObjectAnimationAnimationListBlock,
    ObjectAnimationSenderBlock, RebakeAvatarTextures as RebakeAvatarTexturesWire,
    RebakeAvatarTexturesTextureDataBlock,
};
use sl_wire::messages::{
    ObjectPropertiesFamily as ObjectPropertiesFamilyMessage,
    ObjectPropertiesFamilyObjectDataBlock as ObjectPropertiesFamilyObjectDataBlockMessage,
    ParcelInfoReply, ParcelInfoReplyAgentDataBlock, ParcelInfoReplyDataBlock,
    ParcelObjectOwnersReply, ParcelObjectOwnersReplyDataBlock, PayPriceReply,
    PayPriceReplyButtonDataBlock, PayPriceReplyObjectDataBlock, ScriptRunningReply,
    ScriptRunningReplyScriptBlock, TelehubInfo as TelehubInfoMessage,
    TelehubInfoSpawnPointBlockBlock, TelehubInfoTelehubBlockBlock,
};
use sl_wire::{
    AnyMessage, CircuitCode, ControlFlags, EventQueueEvent, ExperienceInfo, ExperiencePermission,
    ExperienceUpdate, GlobalCoordinates, Llsd, MessageId, PacketFlags, Permissions, Permissions5,
    Reader, RegionHandle, RegionLocalObjectId, RegionLocalParcelId, SequenceNumber, WireError,
    Writer, build_event_queue_response, encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::AssetKey;
use crate::appearance::{MAX_FACES, decode_texture_entry};
use crate::bookkeeping_ids::{
    ImSessionId, LureId, PingId, QueryId, TransactionId, TransferId, XferId,
};
use crate::error::Error;
use crate::extra_params::decode_extra_param_blocks;
use crate::session::{
    SERVER_HISTORY_CAP, ServerHistoryMessage, agent_drop_group_to_llsd,
    agent_list_voice_updates_to_llsd, agent_state_update_to_llsd, build_map_block_reply,
    build_map_item_reply, build_map_layer_reply, build_task_inventory,
    chatterbox_invitation_to_llsd, crossed_region_to_caps_llsd, display_name_update_to_llsd,
    enable_simulator_to_caps_llsd, establish_agent_communication_to_llsd, full_update_block,
    instant_message, nav_mesh_status_to_llsd, open_region_info_to_llsd, parcel_properties_to_llsd,
    parcel_properties_to_wire, region_handshake_message, required_voice_version_to_llsd,
    set_display_name_reply_to_llsd, shape_from_object_shape_block, sim_console_response_to_llsd,
    teleport_finish_to_llsd, unpack_uuids, windlight_refresh_to_llsd,
};
use crate::sim_experiences::SimExperiences;
use crate::sim_inventory::{SimInventoryError, SimInventoryTree};
use crate::sim_voice::{SimVoice, VoiceProvisionOutcome, VoiceProvisionRefusal};
use crate::types::directory::category_from_wire;
use crate::types::{
    AlertInfo, AssetType, AttachmentMode, AttachmentPoint, AvatarName, AvatarPickerResult, Camera,
    ChatSource, ChatType, ClassifiedCategory, CoarseLocation, DayCycle, DetachOrder,
    DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult, DirLandResult,
    DirPeopleResult, DirPlaceResult, DirectoryVisibility, DisplayNameUpdate, EjectAction,
    EnvironmentSettings, EnvironmentUpdate, EstateCovenant, EventInfo, FeatureDisabled,
    FollowCamPropertyValue, FreezeAction, FriendRights, GenericMessage, GenericStreamingMessage,
    GestureActivation, GodRegionUpdate, GroupAccountDetails, GroupAccountSummary,
    GroupAccountTransactions, GroupActiveProposalItem, GroupName, GroupVoteHistoryItem, ImDialog,
    InstantMessage, InventoryFolder, InventoryItem, InventoryItemMove, InventoryType, Kick,
    LandBrushAction, LandBrushSize, LandEdit, LandSearchType, LandStatItem, LandStatReportType,
    MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags, MeanCollision, MovementMode,
    NavMeshStatus, NewInventoryLink, NotecardRez, Object, ObjectBuyItem, ObjectExtraParams,
    ObjectPlayingAnimation, ObjectPropertiesFamily, OpenRegionInfo, ParcelCategory, ParcelDetails,
    ParcelInfo, ParcelObjectOwner, PlacesResult, Postcard, PrimShapeParams, ProposalVoteId,
    RegionIdentity, RegionStats, Reliability, RequiredVoiceVersion, RestoreItem, RezAttachment,
    RezObjectParams, RezScriptParams, SaleType, ScriptControl, ScriptPermissionRequest,
    ScriptPermissions, ServerError, SetDisplayNameReply, SimWideDeleteFlags, SimulatorTime,
    StartLocationSlot, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, TelehubInfo,
    TerraformArea, TextureEntry, Throttle, TransferStatus, Transmit, UpdateGroupInfoParams,
    UserInfo, ViewerEffect, ViewerEffectData, ViewerEffectType,
};
use crate::types::{Event, EventId};
use sl_wire::AbuseReport;
use sl_wire::combine_uuids;
use sl_wire::messages::{
    AbortXfer, AbortXferXferIDBlock, AssetUploadComplete, AssetUploadCompleteAssetBlockBlock,
    ConfirmXferPacket, ConfirmXferPacketXferIDBlock, InitiateDownload,
    InitiateDownloadAgentDataBlock, InitiateDownloadFileDataBlock, RequestXfer,
    RequestXferXferIDBlock, SendXferPacket, SendXferPacketDataPacketBlock,
    SendXferPacketXferIDBlock,
};
use sl_wire::messages::{
    AvatarSitResponse, AvatarSitResponseSitObjectBlock, AvatarSitResponseSitTransformBlock,
    ScriptQuestion, ScriptQuestionDataBlock, ScriptQuestionExperienceBlock,
};
use sl_wire::messages::{
    ChangeUserRights, ChangeUserRightsAgentDataBlock, ChangeUserRightsRightsBlock,
    ImprovedInstantMessage, ImprovedInstantMessageAgentDataBlock,
    ImprovedInstantMessageEstateBlockBlock, ImprovedInstantMessageMessageBlockBlock,
    OfflineNotification, OfflineNotificationAgentBlockBlock, OnlineNotification,
    OnlineNotificationAgentBlockBlock,
};
use sl_wire::messages::{
    DisableSimulator, TeleportFailed, TeleportFailedInfoBlock, TeleportLocal,
    TeleportLocalInfoBlock, TeleportProgress, TeleportProgressAgentDataBlock,
    TeleportProgressInfoBlock, TeleportStart, TeleportStartInfoBlock,
};
use sl_wire::messages::{
    KillObject, KillObjectObjectDataBlock, ObjectUpdate, ObjectUpdateCompressed,
    ObjectUpdateCompressedObjectDataBlock, ObjectUpdateCompressedRegionDataBlock,
    ObjectUpdateRegionDataBlock, ParcelOverlay, ParcelOverlayParcelDataBlock,
};
use sl_wire::messages::{
    TransferInfo, TransferInfoTransferInfoBlock, TransferPacket, TransferPacketTransferDataBlock,
};
use sl_wire::{AgentPreferences, DisplayName, ObjectPermMasks};
use sl_wire::{
    AttachmentResourcesReport, LslSyntax, ObjectCost, ObjectPhysicsData, ParcelScriptResources,
    RemoteParcelRequest, ResourceSummary, SelectedResourceCost, SimulatorFeatures,
};
use sl_wire::{
    FaceMaterialPut, LegacyMaterial, MaterialOverrideUpdate, MediaEntry,
    NewFileAgentInventoryRequest, RenderMaterialEntry, UpdateScriptAgentRequest,
    UpdateScriptTaskRequest,
};
use sl_wire::{IceCandidate, ParcelVoiceInfo, VoiceAccountInfo, VoiceProvisionRequest};
use sl_wire::{
    TRANSFER_CHANNEL_ASSET, TRANSFER_SOURCE_ASSET, TRANSFER_SOURCE_SIM_ESTATE,
    TRANSFER_SOURCE_SIM_INV_ITEM, TransferSourceParamsAsset, TransferSourceParamsEstate,
    TransferSourceParamsInvItem, XferPacketId, decode_xfer_chunk, next_xfer_chunk,
};

/// Decodes a [`RestoreItem`] from one of the field-identical inventory-item
/// blocks the rez messages carry (`RezRestoreToWorld`, `RezObject`, `RezScript`).
/// The blocks are distinct generated wire types but share the same field names,
/// so a macro reuses the decode without a 21-field helper or a trait over the
/// three blocks. Expands to a `RestoreItem`; the `?` on the sale-price decode
/// propagates a [`WireError`](sl_wire::WireError) to the enclosing method.
macro_rules! restore_item_from_inventory_block {
    ($block:expr) => {{
        let block = $block;
        RestoreItem {
            item_id: InventoryKey::from(block.item_id),
            folder_id: InventoryFolderKey::from(block.folder_id),
            creator_id: AgentKey::from(block.creator_id),
            owner: crate::types::inventory_owner_from_wire(
                block.owner_id,
                block.group_id,
                block.group_owned,
            ),
            group: crate::types::group_from_wire(block.group_id),
            permissions: Permissions5 {
                base: Permissions::from_bits(block.base_mask),
                owner: Permissions::from_bits(block.owner_mask),
                group: Permissions::from_bits(block.group_mask),
                everyone: Permissions::from_bits(block.everyone_mask),
                next_owner: Permissions::from_bits(block.next_owner_mask),
            },
            transaction_id: block.transaction_id,
            asset_type: block.r#type,
            inv_type: block.inv_type,
            flags: block.flags,
            sale_type: SaleType::from_code(block.sale_type),
            sale_price: crate::types::linden_price_from_wire(
                block.sale_type != 0,
                "SalePrice",
                block.sale_price,
            )?,
            name: trimmed_string(&block.name),
            description: trimmed_string(&block.description),
            creation_date: block.creation_date,
            crc: block.crc,
        }
    }};
}

/// How long to batch owed acknowledgements before flushing them as a `PacketAck`
/// (matches the client [`Session`](crate::Session)).
const ACK_FLUSH_DELAY: Duration = Duration::from_millis(150);

/// How long the circuit may go without any inbound traffic before it is declared
/// dead.
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(45);

/// How long to wait for an acknowledgement before retransmitting a reliable
/// packet.
const RESEND_TIMEOUT: Duration = Duration::from_millis(1500);

/// The cadence at which the simulator pings an active client with a
/// `StartPingCheck`.
const PING_INTERVAL: Duration = Duration::from_secs(5);

/// How many times a reliable packet is retransmitted before the link is given
/// up as dead.
const MAX_RESEND_ATTEMPTS: u32 = 6;

/// The bound on the recently-seen inbound reliable sequence window.
const SEEN_CAPACITY: usize = 4096;

/// The maximum number of names packed into a single `UUIDNameReply` /
/// `UUIDGroupNameReply`. Smaller than the request batch because each entry also
/// carries the (variable-length) name strings.
const UUID_NAMES_PER_REPLY: usize = 40;

/// The maximum number of acknowledgements packed into a single `PacketAck`.
const MAX_ACKS_PER_PACKET: usize = 255;

/// Computes `now + duration`, saturating at `now` on (impossible) overflow.
fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

/// Narrows a global-metre `f64` to the `f32` the `PlacesReply` `GlobalX/Y/Z`
/// fields carry. Global positions are in-range metre values, so the narrowing
/// is exact for the data the wire (an `F32`) round-trips.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "PlacesReply GlobalX/Y/Z are F32; a global metre value is within f32 range"
)]
const fn global_to_f32(meters: f64) -> f32 {
    meters as f32
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

/// A reliable packet awaiting acknowledgement, kept so it can be retransmitted.
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
    set: HashSet<SequenceNumber>,
    /// Insertion order, for evicting the oldest entries.
    order: VecDeque<SequenceNumber>,
}

impl SeenWindow {
    /// Records `sequence`; returns `true` if it was not seen before.
    fn insert(&mut self, sequence: SequenceNumber) -> bool {
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

/// The lifecycle state of a [`SimSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum SimState {
    /// Constructed; no circuit accepted yet (awaiting `UseCircuitCode`).
    AwaitingCircuit,
    /// The circuit is up: `UseCircuitCode` accepted, keep-alives flow. The agent
    /// completes its arrival once `CompleteAgentMovement` is answered.
    Active,
    /// The session is finished (the client logged out or the link timed out).
    Closed,
}

/// An outbound server-side `Xfer` file send in flight — the mirror of the
/// client's inbound [`Session::request_xfer`](crate::Session::request_xfer)
/// download. The registered file bytes are streamed one `SendXferPacket` at a
/// time, each released by the client's `ConfirmXferPacket` (the same
/// one-packet-in-flight pacing the client's own upload side uses).
#[derive(Debug)]
struct SimXferSend {
    /// The filename the file was registered (and requested) under.
    filename: String,
    /// The complete file bytes being streamed.
    data: Vec<u8>,
    /// How many bytes of [`data`](Self::data) have already been sent.
    sent: usize,
    /// The sequence number of the next `SendXferPacket` (the first is 0).
    next_sequence: u32,
    /// Whether the final packet (high-bit end-of-file marker) has been sent
    /// and is only awaiting its confirmation.
    last_sent: bool,
}

/// What an inbound server-side `Xfer` pull becomes once its final packet is
/// confirmed — the mirror of the client's download purpose tag.
#[derive(Debug)]
enum SimXferReceivePurpose {
    /// The byte stream of an oversized legacy asset upload
    /// ([`Session::save_inventory_asset`](crate::Session::save_inventory_asset))
    /// the simulator requested from the client by its predicted `VFileID`;
    /// completes with an `AssetUploadComplete` and
    /// [`ServerEvent::AssetUploaded`].
    AssetUpload {
        /// The predicted stored asset id,
        /// `combine(transaction_id, secure_session_id)`.
        asset_id: Uuid,
        /// The asset type declared by the `AssetUploadRequest`.
        asset_type: AssetType,
        /// The upload's transaction id, echoed on
        /// [`ServerEvent::AssetUploaded`].
        transaction_id: TransactionId,
    },
    /// A named file the simulator pulled from the client
    /// ([`SimSession::request_xfer_upload`]) — the terrain RAW upload;
    /// completes with [`ServerEvent::XferReceived`].
    NamedFile {
        /// The filename the pull named (the client's viewer-side name).
        filename: String,
    },
}

/// An inbound server-side `Xfer` pull in flight.
#[derive(Debug)]
struct SimXferReceive {
    /// What the assembled bytes become.
    purpose: SimXferReceivePurpose,
    /// The file bytes accumulated so far (the seq-0 length prefix stripped).
    buffer: Vec<u8>,
}

/// The maximum number of asset bytes carried in a single outbound
/// `TransferPacket`. The reference viewer accepts up to 2048
/// (`MAX_PACKET_DATA_SIZE`); like OpenSim's `SendAsset`, packets are kept
/// safely under a datagram's worth.
const TRANSFER_CHUNK_SIZE: usize = 1000;

/// The server-side mirror status of one client-[`Session`](crate::Session)
/// flow-level state machine, as pinned in [`SESSION_FLOW_COVERAGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FlowMirrorStatus {
    /// The [`SimSession`] implements the mirroring server-side machine,
    /// proven by `Session` ↔ `SimSession` loopback tests.
    Mirrored,
    /// No server-side machine yet — a follow-up `protocol-sim-*` task will
    /// mirror it.
    Pending,
    /// Deliberately **not** mirrored: both Second Life and OpenSim offer a
    /// modern (CAPS) alternative for this flow, so the legacy UDP leg is
    /// skipped per the legacy-skip rule (`roadmap/context/protocol.md`).
    Legacy,
}

/// **The `Session` ↔ [`SimSession`] flow-mirroring coverage table.** One row
/// per flow-level (multi-message) state machine the client
/// [`Session`](crate::Session) implements, with its server-side mirror
/// status. Pinned by the `flow_coverage_table_is_pinned` loopback test —
/// changing a row is a deliberate edit there.
///
/// Stateless request/reply surfaces (money, object selection, appearance,
/// group management edits, directory/map/profile queries) are *not* rows:
/// they carry no per-flow client state, so a canned `send_*` reply — most of
/// which [`SimSession`] already has — covers them without a state machine.
///
/// The `Legacy` rows are pinned so skipping them stays a deliberate,
/// documented decision: the UDP texture download is superseded by the
/// `GetTexture` capability on both grids, and the UDP inventory-folder fetch
/// by `FetchInventoryDescendents2`/AISv3 (whose *server* side belongs to the
/// `protocol-sim-caps-inventory` task).
pub const SESSION_FLOW_COVERAGE: &[(&str, FlowMirrorStatus)] = &[
    ("root circuit lifecycle", FlowMirrorStatus::Mirrored),
    ("child-agent circuits", FlowMirrorStatus::Mirrored),
    ("teleport / region handover", FlowMirrorStatus::Mirrored),
    ("object sit", FlowMirrorStatus::Mirrored),
    ("Xfer download", FlowMirrorStatus::Mirrored),
    ("Xfer upload", FlowMirrorStatus::Mirrored),
    ("terrain RAW download", FlowMirrorStatus::Mirrored),
    ("terrain RAW upload", FlowMirrorStatus::Mirrored),
    (
        "legacy transaction asset upload",
        FlowMirrorStatus::Mirrored,
    ),
    ("task-inventory fetch", FlowMirrorStatus::Mirrored),
    (
        "UDP asset Transfer (task item + estate covenant)",
        FlowMirrorStatus::Mirrored,
    ),
    ("UDP texture download", FlowMirrorStatus::Legacy),
    ("UDP inventory-folder fetch", FlowMirrorStatus::Legacy),
    (
        "chat-session lifecycle + server history",
        FlowMirrorStatus::Mirrored,
    ),
    ("friendship / presence", FlowMirrorStatus::Mirrored),
    (
        "script permission / control mirror",
        FlowMirrorStatus::Mirrored,
    ),
];

/// Whether the circuit hosts a **child** agent (scene streaming only) or the
/// **root** agent (the avatar is present in this region). A client opens a
/// child circuit with `UseCircuitCode` alone — a neighbour holding presence
/// ahead of a crossing, or a teleport destination before its confirmation —
/// and promotes it by sending `CompleteAgentMovement`, exactly as the region
/// servers distinguish child and root agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AgentPresence {
    /// `UseCircuitCode` accepted, no `CompleteAgentMovement` yet: a child
    /// agent (the region streams its scene, but the avatar is elsewhere).
    Child,
    /// `CompleteAgentMovement` answered: the root agent — the avatar is in
    /// this region.
    Root,
}

/// The decoded source of a client `TransferRequest`, surfaced on
/// [`ServerEvent::TransferRequested`]. Only the two source types that remain
/// UDP-only on both grids are decoded; a plain asset-by-id request
/// (superseded by the `ViewerAsset` HTTP capability) is auto-refused and not
/// surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferRequestSource {
    /// A task-inventory item's asset (source `SimInvItem`) — a script or
    /// notecard body in a prim's contents.
    TaskInventoryItem(TransferSourceParamsInvItem),
    /// An estate asset (source `SimEstate`) — the covenant notecard.
    Estate(TransferSourceParamsEstate),
}

/// The size of one `ParcelOverlay` chunk: a simulator splits the region's
/// per-4 m-cell ownership map into 1024-byte pieces (four for a 256 m region).
pub const PARCEL_OVERLAY_CHUNK_BYTES: usize = 1024;

/// The decoded camera/control state carried by a client `AgentUpdate`, surfaced
/// as [`ServerEvent::AgentUpdate`]. The simulator uses this to move the agent
/// and to drive its interest list, mirroring what the client
/// [`Session`](crate::Session) sends.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentUpdateInfo {
    /// The agent's body (facing) rotation.
    pub body_rotation: Rotation,
    /// The agent's head rotation.
    pub head_rotation: Rotation,
    /// The control flags pressed (walk/fly/turn/…); the simulator moves the
    /// agent accordingly.
    pub controls: ControlFlags,
    /// The agent's camera viewpoint, driving the region's interest list.
    pub camera: Camera,
    /// The draw distance (metres) the client advertises.
    pub far: f32,
    /// The agent state byte (e.g. editing/typing flags).
    pub state: u8,
    /// The `AgentUpdate` flags byte.
    pub flags: u8,
}

/// The seat placement an `AvatarSitResponse` carries — the server-authored
/// half of the sit handshake, mirroring the fields the client surfaces on
/// [`Event::SitResult`](crate::Event::SitResult).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SitTransform {
    /// Whether the viewer should autopilot (walk) to the seat first (the
    /// target is out of immediate sit range).
    pub autopilot: bool,
    /// The seat position relative to the object, in metres.
    pub sit_position: Vector,
    /// The seated orientation relative to the object — which way the avatar
    /// faces once seated.
    pub sit_rotation: Rotation,
    /// The scripted-sit camera eye position relative to the seat
    /// (`llSetCameraEyeOffset`); the zero vector when the seat's script sets
    /// no custom camera.
    pub camera_eye_offset: Vector,
    /// The scripted-sit camera focus point relative to the seat
    /// (`llSetCameraAtOffset`); the zero vector when the seat's script sets
    /// no custom camera.
    pub camera_at_offset: Vector,
    /// Whether sitting forces the avatar into mouselook (set by vehicles and
    /// weapon huds).
    pub force_mouselook: bool,
}

/// One friend-rights change entry, shared by the client's `GrantUserRights`
/// decode ([`ServerEvent::UserRightsGranted`]) and the server's
/// [`SimSession::send_change_user_rights`] push (a `RightsBlock` on either
/// wire message).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UserRightsEntry {
    /// The related agent (`AgentRelated` on the wire): the friend whose
    /// rights are being set, or — on a [`SimSession::send_change_user_rights`]
    /// push where the *friend* changed what they grant — the receiving agent.
    pub agent: FriendKey,
    /// The complete rights bitfield now in force (`RelatedRights`), not a
    /// delta.
    pub rights: FriendRights,
}

/// What a simulator-side chat session is — a group's IM channel or an ad-hoc
/// conference. The server twin of the client's `ChatSessionKind` (a 1:1
/// exchange is not a server-side session: it is plain IM relay).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SimChatSessionKind {
    /// A group's IM session; the wire session id is the group id.
    Group {
        /// The group whose channel this is.
        group_id: GroupKey,
    },
    /// An ad-hoc conference of individual agents, keyed by a minted session
    /// id.
    Conference,
}

/// One live chat session on the simulator side — the mirror of the client's
/// session registry entry, plus the **server history** backlog the
/// `ChatSessionRequest` capability's `fetch history` method serves (the cap
/// dispatch belongs to the CAPS surface; the state lives here).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SimChatSession {
    /// Whether this is a group channel or an ad-hoc conference.
    pub kind: SimChatSessionKind,
    /// The current participant roster.
    pub participants: BTreeSet<AgentKey>,
    /// The recent-message backlog (newest last, capped like the client's
    /// mirror): every message relayed through this session, whether sent by
    /// this session's own client ([`ServerEvent::SessionMessageSent`]) or
    /// pushed to it ([`SimSession::send_session_message`]).
    pub history: Vec<ServerHistoryMessage>,
}

impl SimChatSession {
    /// Appends one message to the history backlog, dropping the oldest
    /// entries beyond the cap.
    fn log(&mut self, message: ServerHistoryMessage) {
        self.history.push(message);
        if self.history.len() > SERVER_HISTORY_CAP {
            let excess = self.history.len().saturating_sub(SERVER_HISTORY_CAP);
            self.history.drain(..excess);
        }
    }
}

/// The server-side sit state machine — the mirror of the client's private
/// `SitState` (`AwaitingResponse` on the client corresponds to nothing here:
/// the request is surfaced as [`ServerEvent::SitRequested`] and the machine
/// only advances once the driver answers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum SimSitState {
    /// The agent is not seated and no sit offer is outstanding.
    NotSitting,
    /// [`SimSession::send_avatar_sit_response`] was sent; awaiting the
    /// client's completing `AgentSit`.
    ResponseSent {
        /// The object offered as a seat.
        on: ObjectKey,
    },
    /// The client completed the handshake with `AgentSit`; the agent is
    /// seated.
    Seated {
        /// The object sat upon.
        on: ObjectKey,
    },
}

/// A server-side event decoded from a client-only message, the inverse of the
/// client's [`Command`](crate::Command)/[`Event`](crate::Event) split: it is
/// what the simulator observes a client doing.
///
/// Circuit-lifecycle messages are both acted on (the simulator answers them) and
/// surfaced here. Messages with a meaningful payload (`ChatFromViewer`,
/// `ImprovedInstantMessage`, `AgentUpdate`, `AgentThrottle`) are decoded into
/// typed variants. Every other decoded client message is surfaced verbatim as
/// [`ServerEvent::ClientMessage`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ServerEvent {
    /// The client opened the circuit with `UseCircuitCode`. The simulator now
    /// knows the agent/session ids and circuit code for this link.
    CircuitOpened {
        /// The agent (avatar) id.
        agent_id: AgentKey,
        /// The session id.
        session_id: Uuid,
        /// The circuit code.
        circuit_code: CircuitCode,
    },
    /// The client sent `CompleteAgentMovement`; the simulator has replied with an
    /// `AgentMovementComplete` and the agent is now present in the region.
    AgentArrived,
    /// The client acknowledged the region handshake with `RegionHandshakeReply`.
    RegionHandshakeReplied,
    /// The client pinged the link with `StartPingCheck`; the simulator has
    /// replied with a `CompletePingCheck`.
    PingRequested {
        /// The ping id echoed back to the client.
        ping_id: PingId,
    },
    /// The client set its bandwidth throttle (`AgentThrottle`).
    Throttle(Throttle),
    /// The client sent an `AgentUpdate` (movement controls + camera).
    AgentUpdate(Box<AgentUpdateInfo>),
    /// The client said something on local chat (`ChatFromViewer`).
    Chat {
        /// The chat text (NUL terminator stripped).
        message: String,
        /// The chat channel (0 = public local chat).
        channel: ChatChannel,
        /// The chat type (whisper/normal/shout/typing/…).
        chat_type: ChatType,
    },
    /// The client sent an instant message (`ImprovedInstantMessage`). The
    /// group/conference **session dialogs** do not take this path — they
    /// decode into the typed session events below, exactly as the client
    /// routes them away from its own generic IM event.
    InstantMessage(Box<InstantMessage>),
    /// The client asked to start (join) a group's IM session
    /// (`ImprovedInstantMessage`, `SessionGroupStart`) — the inverse of the
    /// client's
    /// [`Session::start_group_session`](crate::Session::start_group_session).
    /// The registry entry is created with the sender in its roster
    /// ([`SimSession::chat_session`]).
    GroupSessionStartRequested {
        /// The group whose session the client joins (the wire session id).
        group_id: GroupKey,
    },
    /// The client started an ad-hoc conference (`ImprovedInstantMessage`,
    /// `SessionConferenceStart`) — the inverse of the client's
    /// [`Session::start_conference`](crate::Session::start_conference). The
    /// registry entry is created with the sender and the invitees in its
    /// roster; the driver materialises the session on each invitee's
    /// [`SimSession`] ([`SimSession::open_chat_session`]) and delivers the
    /// invitation over its event queue
    /// ([`SimSession::enqueue_chatterbox_invitation`]).
    ConferenceStartRequested {
        /// The conference's minted session id.
        session_id: ImSessionId,
        /// The invited agents, unpacked from the binary bucket.
        invitees: Vec<AgentKey>,
        /// The accompanying invitation message text.
        message: String,
    },
    /// The client sent a message into a group/conference session
    /// (`ImprovedInstantMessage`, `SessionSend`) — the inverse of the
    /// client's [`Session::send_group_message`](crate::Session::send_group_message)
    /// / [`Session::send_conference_message`](crate::Session::send_conference_message).
    /// When the session is known the message is appended to its server
    /// history; an unknown session still surfaces (the simulator is
    /// authoritative for membership — the driver polices). The driver relays
    /// to the other participants' sessions with
    /// [`SimSession::send_session_message`].
    SessionMessageSent {
        /// The session the message was sent into.
        session_id: ImSessionId,
        /// The message text.
        message: String,
    },
    /// The client left a group/conference session
    /// (`ImprovedInstantMessage`, `SessionLeave`) — the inverse of the
    /// client's [`Session::leave_group_session`](crate::Session::leave_group_session)
    /// / [`Session::leave_conference`](crate::Session::leave_conference). The
    /// sender is dropped from the roster (an emptied session is removed); the
    /// driver notifies the remaining participants with
    /// [`SimSession::send_session_participant`].
    SessionLeaveRequested {
        /// The session being left.
        session_id: ImSessionId,
    },
    /// The client accepted a friendship offer (`AcceptFriendship`) — the
    /// inverse of the client's
    /// [`Session::accept_friendship`](crate::Session::accept_friendship). The
    /// `transaction` echoes the offer IM's id. The driver relays the outcome
    /// to the offerer's [`SimSession`] as an
    /// [`ImDialog::FriendshipAccepted`](crate::ImDialog::FriendshipAccepted)
    /// IM ([`SimSession::send_instant_message`]) — the grid-level buddy store
    /// itself stays the driver's job.
    FriendshipAccepted {
        /// The offer transaction being accepted (the offer IM's id).
        transaction: TransactionId,
        /// The accepter's inventory folder(s) for the new friend's calling
        /// card (the accepter's own inventory — a relaying driver normally
        /// drops this).
        calling_card_folders: Vec<InventoryFolderKey>,
    },
    /// The client declined a friendship offer (`DeclineFriendship`) — the
    /// inverse of the client's
    /// [`Session::decline_friendship`](crate::Session::decline_friendship).
    /// The driver relays the outcome to the offerer's [`SimSession`] as an
    /// [`ImDialog::FriendshipDeclined`](crate::ImDialog::FriendshipDeclined)
    /// IM.
    FriendshipDeclined {
        /// The offer transaction being declined (the offer IM's id).
        transaction: TransactionId,
    },
    /// The client asked to end a friendship (`TerminateFriendship`) — the
    /// inverse of the client's
    /// [`Session::terminate_friendship`](crate::Session::terminate_friendship).
    /// The driver confirms with
    /// [`SimSession::send_terminate_friendship`] on this session and relays
    /// the removal to the former friend's [`SimSession`].
    FriendshipTerminationRequested {
        /// The former friend being removed.
        other: FriendKey,
    },
    /// The client set the rights it grants some friends (`GrantUserRights`) —
    /// the inverse of the client's
    /// [`Session::grant_user_rights`](crate::Session::grant_user_rights). The
    /// driver echoes each entry back with
    /// [`SimSession::send_change_user_rights`] (changer = this agent) and
    /// pushes the change to each affected friend's [`SimSession`].
    UserRightsGranted {
        /// The rights entries, one per friend whose grant changed.
        rights: Vec<UserRightsEntry>,
    },
    /// The client asked the simulator to resolve agent ids to legacy names
    /// (`UUIDNameRequest`). The server answers with
    /// [`SimSession::send_avatar_names`].
    AvatarNamesRequested(Vec<Uuid>),
    /// The client asked the simulator to resolve group ids to names
    /// (`UUIDGroupNameRequest`). The server answers with
    /// [`SimSession::send_group_names`].
    GroupNamesRequested(Vec<Uuid>),
    /// The client attached an in-world object to its avatar (`ObjectAttach`).
    AttachObject {
        /// The attached object's region-local id.
        local_id: RegionLocalObjectId,
        /// The point the object is attached to.
        attachment_point: AttachmentPoint,
        /// Whether the object was added to the point rather than replacing what
        /// was there.
        mode: AttachmentMode,
        /// The rotation the object is worn at.
        rotation: Rotation,
    },
    /// The client detached attachments back to inventory (`ObjectDetach`).
    DetachObjects(Vec<RegionLocalObjectId>),
    /// The client dropped attachments onto the ground (`ObjectDrop`).
    DropAttachments(Vec<RegionLocalObjectId>),
    /// The client took off a worn item by inventory id (`RemoveAttachment`).
    RemoveAttachment {
        /// The point the item was worn on.
        attachment_point: AttachmentPoint,
        /// The worn item's inventory item id.
        item_id: Uuid,
    },
    /// The client wore an inventory item as an attachment
    /// (`RezSingleAttachmentFromInv`).
    RezAttachment(Box<RezAttachment>),
    /// The client wore several inventory items as attachments in one compound
    /// message (`RezMultipleAttachmentsFromInv`).
    RezAttachments {
        /// The compound message's correlation id.
        compound_id: Uuid,
        /// Whether everything worn was detached first.
        detach: DetachOrder,
        /// The items the client wore.
        attachments: Vec<RezAttachment>,
    },
    /// The client emitted one or more viewer effects (`ViewerEffect`): look-at /
    /// point-at gaze hints, the editing/touch beam, and other transient HUD
    /// effects. A simulator would relay these to other nearby viewers.
    ViewerEffect(Vec<ViewerEffect>),
    /// The client marked one or more gestures active (`ActivateGestures`); the
    /// simulator records which gesture assets are live for the session so it can
    /// preload them.
    ActivateGestures {
        /// The gestures to activate (each pairs an inventory item id with its
        /// gesture asset id).
        gestures: Vec<GestureActivation>,
    },
    /// The client marked one or more gestures inactive (`DeactivateGestures`),
    /// naming them by inventory item id.
    DeactivateGestures {
        /// The inventory item ids of the gestures to deactivate.
        item_ids: Vec<Uuid>,
    },
    /// The client chose whether the avatar runs or walks (`SetAlwaysRun`).
    SetAlwaysRun {
        /// Whether the avatar always runs or walks.
        mode: MovementMode,
    },
    /// The client reported it has stalled and is not reading the network
    /// (`AgentPause`); the simulator should stop streaming updates until a
    /// matching [`ServerEvent::AgentResume`]. `serial_num` is a monotonic counter
    /// shared with resume — ignore non-increasing values.
    AgentPause {
        /// The pause/resume serial number; ignore if not greater than the last.
        serial_num: u32,
    },
    /// The client reported it has resumed reading the network (`AgentResume`)
    /// after an [`ServerEvent::AgentPause`]. `serial_num` is the same monotonic
    /// counter shared with pause.
    AgentResume {
        /// The pause/resume serial number; ignore if not greater than the last.
        serial_num: u32,
    },
    /// The client updated its vertical field of view (`AgentFOV`), in radians;
    /// the simulator uses it for interest-list culling.
    AgentFov {
        /// The vertical field of view, in radians.
        vertical_angle: f32,
    },
    /// The client updated its viewport size in pixels (`AgentHeightWidth`), sent
    /// when the viewer window is created or resized.
    AgentHeightWidth {
        /// The viewport height in pixels.
        height: u16,
        /// The viewport width in pixels.
        width: u16,
    },
    /// The client forcibly released any agent movement controls a script had
    /// taken (`ForceScriptControlRelease`); the simulator should drop all
    /// script-held controls for this agent.
    ForceScriptControlRelease,
    /// The client answered a [`SimSession::send_script_question`]
    /// (`ScriptAnswerYes`), granting `permissions` to the script `item_id` in
    /// object `task_id` — the inverse of the client's
    /// [`Session::answer_script_permissions`](crate::Session::answer_script_permissions).
    /// An empty set is an explicit deny. The answer is recorded in the grant
    /// mirror ([`SimSession::script_grant`]) whether or not a question was
    /// outstanding — the simulator stays authoritative for enforcement; the
    /// mirror only records what the agent answered.
    ScriptPermissionAnswer {
        /// The task (object) id holding the script.
        task_id: ObjectKey,
        /// The script item id within the object.
        item_id: InventoryKey,
        /// The granted permission subset (empty = explicit deny).
        permissions: ScriptPermissions,
    },
    /// The client asked to track an agent's position (`TrackAgent`); the
    /// simulator would stream the tracked agent's coarse location back via
    /// [`SimSession::send_coarse_location_update`].
    TrackAgent {
        /// The agent to track.
        prey_id: AgentKey,
    },
    /// The client asked for an agent's global position (`FindAgent`); the
    /// simulator answers with [`SimSession::send_find_agent_reply`].
    FindAgent {
        /// The requesting agent (the "hunter").
        hunter: Uuid,
        /// The agent to locate (the "prey").
        prey: Uuid,
    },
    /// The client ran a directory people / groups / events search
    /// (`DirFindQuery`); the simulator answers with the matching `send_dir_*`
    /// reply, correlated by `query_id`.
    DirFindQuery {
        /// The client-chosen id to echo back in the reply.
        query_id: Uuid,
        /// The search text.
        query_text: String,
        /// What to search and how to sort/filter.
        flags: DirFindFlags,
        /// The 0-based index of the first result the client wants.
        query_start: i32,
    },
    /// The client searched the places directory (`DirPlacesQuery`); the simulator
    /// answers with [`SimSession::send_dir_places_reply`].
    DirPlacesQuery {
        /// The client-chosen id to echo back in the reply.
        query_id: Uuid,
        /// The search text.
        query_text: String,
        /// Result inclusion/sort flags.
        flags: DirFindFlags,
        /// The parcel category to filter by.
        category: ParcelCategory,
        /// An optional region-name filter (empty for any region).
        sim_name: String,
        /// The 0-based index of the first result the client wants.
        query_start: i32,
    },
    /// The client searched the land-for-sale directory (`DirLandQuery`); the
    /// simulator answers with [`SimSession::send_dir_land_reply`].
    DirLandQuery {
        /// The client-chosen id to echo back in the reply.
        query_id: Uuid,
        /// Result inclusion/sort and limit flags.
        flags: DirFindFlags,
        /// Which sale types to include.
        search_type: LandSearchType,
        /// The price limit.
        price: i32,
        /// The area limit.
        area: i32,
        /// The 0-based index of the first result the client wants.
        query_start: i32,
    },
    /// The client searched the classifieds directory (`DirClassifiedQuery`); the
    /// simulator answers with [`SimSession::send_dir_classified_reply`].
    DirClassifiedQuery {
        /// The client-chosen id to echo back in the reply.
        query_id: Uuid,
        /// The search text.
        query_text: String,
        /// Result inclusion/sort flags.
        flags: DirFindFlags,
        /// The classified category to filter by
        /// ([`ClassifiedCategory::AnyCategory`] for any).
        category: ClassifiedCategory,
        /// The 0-based index of the first result the client wants.
        query_start: i32,
    },
    /// The client requested avatar-name autocomplete (`AvatarPickerRequest`); the
    /// simulator answers with [`SimSession::send_avatar_picker_reply`].
    AvatarPickerRequest {
        /// The client-chosen id to echo back in the reply.
        query_id: Uuid,
        /// The (partial) name to match.
        name: String,
    },
    /// The client looked up land holdings (`PlacesQuery`); the simulator answers
    /// with [`SimSession::send_places_reply`].
    PlacesQuery {
        /// The client-chosen id to echo back in the reply.
        query_id: Uuid,
        /// A correlation id to echo back in the reply.
        transaction_id: Uuid,
        /// The search text.
        query_text: String,
        /// Result flags.
        flags: DirFindFlags,
        /// The parcel category to filter by.
        category: ParcelCategory,
        /// An optional region-name filter (empty for any region).
        sim_name: String,
    },
    /// The client requested an in-world event's full detail (`EventInfoRequest`);
    /// the simulator answers with [`SimSession::send_event_info_reply`].
    EventInfoRequest {
        /// The event to look up.
        event_id: EventId,
    },
    /// The client subscribed to a reminder for an in-world event
    /// (`EventNotificationAddRequest`). There is no direct reply.
    EventNotificationAddRequest {
        /// The event to be reminded about.
        event_id: EventId,
    },
    /// The client cancelled an event reminder (`EventNotificationRemoveRequest`).
    /// There is no direct reply.
    EventNotificationRemoveRequest {
        /// The event whose reminder to cancel.
        event_id: EventId,
    },
    /// The client wants to buy in-world objects (`ObjectBuy`).
    BuyObject {
        /// The active group ([`Uuid::nil`] for none).
        group_id: GroupKey,
        /// The inventory folder a derezed purchase is placed in.
        category_id: Uuid,
        /// The objects to buy (each with its advertised sale type and price).
        objects: Vec<ObjectBuyItem>,
    },
    /// The client wants to buy an item out of an object's contents
    /// (`BuyObjectInventory`).
    BuyObjectInventory {
        /// The object whose contents holds the item.
        object_id: ObjectKey,
        /// The inventory item to buy.
        item_id: Uuid,
        /// The folder the bought item is placed in.
        folder_id: Uuid,
    },
    /// The client asked for an object's pay-button layout (`RequestPayPrice`);
    /// the simulator answers with [`SimSession::send_pay_price_reply`].
    RequestPayPrice {
        /// The object queried.
        object_id: ObjectKey,
    },
    /// The client asked for an object's condensed broadcast properties
    /// (`RequestObjectPropertiesFamily`); the simulator answers with
    /// [`SimSession::send_object_properties_family`].
    RequestObjectPropertiesFamily {
        /// The request flags, echoed back in the reply.
        request_flags: u32,
        /// The object queried.
        object_id: ObjectKey,
    },
    /// The client began an interactive object spin (`ObjectSpinStart`).
    SpinObjectStart {
        /// The object being spun.
        object_id: ObjectKey,
    },
    /// The client updated an in-progress object spin (`ObjectSpinUpdate`).
    SpinObjectUpdate {
        /// The object being spun.
        object_id: ObjectKey,
        /// The new rotation.
        rotation: Rotation,
    },
    /// The client ended an interactive object spin (`ObjectSpinStop`).
    SpinObjectStop {
        /// The object being spun.
        object_id: ObjectKey,
    },
    /// The client wants to duplicate objects onto a raycast surface
    /// (`ObjectDuplicateOnRay`).
    DuplicateObjectsOnRay {
        /// The region-local ids to duplicate.
        local_ids: Vec<RegionLocalObjectId>,
        /// The active group the copies are set to (`None` for none).
        group_id: Option<GroupKey>,
        /// The ray's start point (region-local).
        ray_start: Vector,
        /// The ray's end point (region-local).
        ray_end: Vector,
        /// When set, the simulator trusts `ray_end` rather than raycasting.
        bypass_raycast: bool,
        /// Whether `ray_end` is the actual intersection point.
        ray_end_is_intersection: bool,
        /// Whether to copy each object's centre offset.
        copy_centers: bool,
        /// Whether to copy each object's rotation.
        copy_rotates: bool,
        /// The object the ray is cast against (`None` for the terrain).
        ray_target_id: Option<ObjectKey>,
        /// The duplicate flags (see `object_flags.h`).
        duplicate_flags: u32,
    },
    /// The client wants to restore an inventory item to the world
    /// (`RezRestoreToWorld`).
    RezRestoreToWorld {
        /// The full inventory item to restore.
        item: RestoreItem,
    },
    /// The client wants to rez an object embedded in a notecard
    /// (`RezObjectFromNotecard`).
    RezObjectFromNotecard {
        /// The rez parameters (ray placement, permissions, notecard, items).
        rez: NotecardRez,
    },
    /// The client wants to join all its leased parcels within a metre rectangle
    /// into one parcel (`ParcelJoin`).
    JoinParcels {
        /// The western edge of the rectangle (metres, region-local).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
    },
    /// The client wants to subdivide a parcel along a metre rectangle
    /// (`ParcelDivide`).
    DivideParcel {
        /// The western edge of the rectangle (metres, region-local).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
    },
    /// The client asked for a parcel's per-owner object tallies
    /// (`ParcelObjectOwnersRequest`); the simulator answers with
    /// [`SimSession::send_parcel_object_owners_reply`].
    RequestParcelObjectOwners {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
    },
    /// The client wants to buy a temporary access pass to a parcel
    /// (`ParcelBuyPass`).
    BuyParcelPass {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
    },
    /// The client wants to disable scripted objects on a parcel
    /// (`ParcelDisableObjects`).
    DisableParcelObjects {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// Which objects to disable (combined `ParcelReturnType` constants).
        return_type: u32,
        /// The owner-id scope (empty for none).
        owner_ids: Vec<Uuid>,
        /// The explicit object/task-id scope (empty for none).
        task_ids: Vec<ObjectKey>,
    },
    /// The client asked for a parcel's basic listing by grid-wide parcel id
    /// (`ParcelInfoRequest`); the simulator answers with
    /// [`SimSession::send_parcel_info_reply`].
    RequestParcelInfo {
        /// The parcel's grid-wide id.
        parcel_id: ParcelKey,
    },
    /// The client asked whether a task's script is running (`GetScriptRunning`);
    /// the simulator answers with [`SimSession::send_script_running_reply`].
    RequestScriptRunning {
        /// The object (task) holding the script.
        object_id: ObjectKey,
        /// The script inventory item inside that task.
        item_id: Uuid,
    },
    /// The client asked to start or stop a task's script (`SetScriptRunning`).
    SetScriptRunning {
        /// The object (task) holding the script.
        object_id: ObjectKey,
        /// The script inventory item inside that task.
        item_id: Uuid,
        /// `true` to run the script, `false` to stop it.
        running: bool,
    },
    /// The client asked to reset a task's script (`ScriptReset`).
    ResetScript {
        /// The object (task) holding the script.
        object_id: ObjectKey,
        /// The script inventory item inside that task.
        item_id: Uuid,
    },
    /// The client requested a group's financial summary
    /// (`GroupAccountSummaryRequest`); the simulator answers with
    /// [`SimSession::send_group_account_summary_reply`].
    RequestGroupAccountSummary {
        /// The group to summarise.
        group_id: GroupKey,
        /// The client-chosen request id to echo back.
        request_id: Uuid,
        /// The accounting interval length in days.
        interval_days: i32,
        /// Which interval (0 = current, 1 = previous).
        current_interval: i32,
    },
    /// The client requested a group's itemised accounting detail
    /// (`GroupAccountDetailsRequest`); the simulator answers with
    /// [`SimSession::send_group_account_details_reply`].
    RequestGroupAccountDetails {
        /// The group to detail.
        group_id: GroupKey,
        /// The client-chosen request id to echo back.
        request_id: Uuid,
        /// The accounting interval length in days.
        interval_days: i32,
        /// Which interval (0 = current, 1 = previous).
        current_interval: i32,
    },
    /// The client requested a group's transaction log
    /// (`GroupAccountTransactionsRequest`); the simulator answers with
    /// [`SimSession::send_group_account_transactions_reply`].
    RequestGroupAccountTransactions {
        /// The group whose log to return.
        group_id: GroupKey,
        /// The client-chosen request id to echo back.
        request_id: Uuid,
        /// The accounting interval length in days.
        interval_days: i32,
        /// Which interval (0 = current, 1 = previous).
        current_interval: i32,
    },
    /// The client requested a group's active proposals
    /// (`GroupActiveProposalsRequest`); the simulator answers with
    /// [`SimSession::send_group_active_proposals_reply`].
    RequestGroupActiveProposals {
        /// The group to query.
        group_id: GroupKey,
        /// The client-chosen transaction id to echo back.
        transaction_id: Uuid,
    },
    /// The client requested a group's vote history (`GroupVoteHistoryRequest`);
    /// the simulator answers with
    /// [`SimSession::send_group_vote_history_reply`].
    RequestGroupVoteHistory {
        /// The group to query.
        group_id: GroupKey,
        /// The client-chosen transaction id to echo back.
        transaction_id: Uuid,
    },
    /// The client started a new group proposal (`StartGroupProposal`).
    StartGroupProposal {
        /// The group to start the proposal in.
        group_id: GroupKey,
        /// The minimum number of votes required for the result to count.
        quorum: i32,
        /// The fraction of votes needed to pass (0.0–1.0).
        majority: f32,
        /// The voting window length in seconds.
        duration: i32,
        /// The proposal text.
        proposal_text: String,
    },
    /// The client cast a vote on an active group proposal
    /// (`GroupProposalBallot`).
    GroupProposalBallot {
        /// The proposal's id.
        proposal_id: ProposalVoteId,
        /// The group the proposal belongs to.
        group_id: GroupKey,
        /// The vote cast (e.g. `"yes"`/`"no"`).
        vote_cast: String,
    },
    /// The client asked for the estate covenant (`EstateCovenantRequest`); the
    /// simulator answers with [`SimSession::send_estate_covenant_reply`].
    RequestEstateCovenant,
    /// The client requested the region's telehub configuration
    /// (`EstateOwnerMessage`/`telehub` `info ui`); the simulator answers with
    /// [`SimSession::send_telehub_info`].
    RequestTelehubInfo,
    /// The client asked to make an object the region's telehub
    /// (`EstateOwnerMessage`/`telehub` `connect`).
    ConnectTelehub {
        /// The local id of the object to make the telehub.
        object_local_id: RegionLocalObjectId,
    },
    /// The client asked to remove the region's telehub (`EstateOwnerMessage`/
    /// `telehub` `delete`).
    DisconnectTelehub,
    /// The client asked to add a telehub spawn point at an object's position
    /// (`EstateOwnerMessage`/`telehub` `spawnpoint add`).
    AddTelehubSpawnPoint {
        /// The local id of the object marking the spawn point.
        object_local_id: RegionLocalObjectId,
    },
    /// The client asked to remove a telehub spawn point by index
    /// (`EstateOwnerMessage`/`telehub` `spawnpoint remove`).
    RemoveTelehubSpawnPoint {
        /// The zero-based index of the spawn point to remove.
        spawn_index: u32,
    },
    /// The client asked to download the region's terrain heightmap as an LL
    /// RAW file (`EstateOwnerMessage`/`terrain` `download filename`). The
    /// driver serialises the heightmap and offers it with
    /// [`SimSession::send_initiate_download`], echoing this viewer filename.
    TerrainDownloadRequested {
        /// The filename the viewer wants the download tagged with.
        viewer_filename: String,
    },
    /// The client asked to upload a terrain heightmap RAW file
    /// (`EstateOwnerMessage`/`terrain` `upload filename`). The driver pulls it
    /// with [`SimSession::request_xfer_upload`] naming this filename; the
    /// bytes arrive as [`ServerEvent::XferReceived`].
    TerrainUploadRequested {
        /// The viewer-side filename the client will stream on request.
        viewer_filename: String,
    },
    /// The client asked to bake the current terrain as the region's revert
    /// baseline (`EstateOwnerMessage`/`terrain` `bake`).
    TerrainBakeRequested,
    /// An `EstateOwnerMessage` whose method has no typed event of its own
    /// (the telehub and terrain methods do) — surfaced raw so a driver can
    /// act on any estate command. The invoice is the client's correlation id
    /// for a reply.
    EstateOwnerRequest {
        /// The estate method name (e.g. `setregioninfo`, `kickestate`).
        method: String,
        /// The client's invoice id, echoed on a reply.
        invoice: Uuid,
        /// The method's string parameters, in order.
        params: Vec<String>,
    },
    /// The client filed an abuse / bug report over the legacy `UserReport` UDP
    /// message (the modern path is the `SendUserReport` capability). The
    /// simulator routes it to the grid's abuse desk; fire-and-forget.
    AbuseReportReceived(Box<AbuseReport>),
    /// The client filed an abuse report bearing a snapshot over the
    /// `SendUserReportWithScreenshot` two-step uploader: the first step
    /// carried the report, the second the raw screenshot bytes (JPEG-2000,
    /// kept verbatim). The simulator routes both to the grid's abuse desk;
    /// fire-and-forget.
    AbuseReportWithScreenshotReceived {
        /// The parsed abuse report from the first upload step.
        report: Box<AbuseReport>,
        /// The raw screenshot bytes from the second upload step.
        screenshot: Vec<u8>,
    },
    /// A two-stage CAPS asset upload completed: the raw bytes arrived at the
    /// uploader URL and the simulator minted the stored asset id (plus, for the
    /// inventory-creating caps, an inventory item id). Covers
    /// `NewFileAgentInventory`, `UploadBakedTexture`, and every
    /// `Update{Gesture,Notecard,Script,Settings,Material}{Agent,Task}Inventory`
    /// cap — the metadata says which. Fire-and-forget for the driver to persist.
    CapsAssetUploaded {
        /// The parsed step-1 metadata identifying what was uploaded.
        metadata: Box<CapsUploadMetadata>,
        /// The stored asset id the simulator minted and returned to the client.
        new_asset: AssetKey,
        /// The created/updated inventory item id (`None` for a temporary
        /// `UploadBakedTexture` bake).
        new_inventory_item: Option<InventoryKey>,
        /// The complete uploaded asset bytes from the second step.
        data: Vec<u8>,
    },
    /// The client asked the grid to server-side bake its appearance
    /// (`UpdateAvatarAppearance`) at the given Current Outfit Folder version.
    /// The baked-texture ids arrive separately over UDP `AvatarAppearance`; the
    /// capability itself answers the client with the accept reply
    /// (`{ success: true }`).
    ServerAppearanceRequested {
        /// The Current Outfit Folder version the client asked the grid to bake.
        cof_version: i32,
    },
    /// The client copied an item embedded in a notecard into inventory
    /// (`CopyInventoryFromNotecard`). Fire-and-forget: the copied item is
    /// delivered over the normal inventory-update stream, so there is no reply.
    CopyInventoryFromNotecardRequested {
        /// The notecard holding the embedded item.
        notecard_id: InventoryKey,
        /// The in-world object holding the notecard, or `None` for an
        /// agent-inventory notecard.
        object_id: Option<ObjectKey>,
        /// The embedded item copied.
        item_id: InventoryKey,
        /// The destination folder, or `None` to let the simulator pick the
        /// system folder for the item's type.
        folder_id: Option<InventoryFolderKey>,
    },
    /// The client set legacy (normal/specular) materials on object faces via
    /// the `RenderMaterials` PUT. A world mutation — the driver applies it and
    /// echoes the assigned material ids on the faces' texture entries.
    RenderMaterialsSet {
        /// One entry per affected face (a cleared face has `material: None`).
        updates: Vec<FaceMaterialPut>,
    },
    /// The client set GLTF (PBR) material params on object faces
    /// (`ModifyMaterialParams`). A world mutation for the driver to apply.
    MaterialParamsModified {
        /// One entry per affected face.
        updates: Vec<MaterialOverrideUpdate>,
    },
    /// The client set the per-face media on an object (`ObjectMedia` UPDATE
    /// verb). The simulator has recorded it and advanced the media version.
    ObjectMediaSet {
        /// The object whose media was set.
        object_id: ObjectKey,
        /// The new per-face media (one slot per prim face; `None` for a face
        /// without media).
        faces: Vec<Option<MediaEntry>>,
    },
    /// The client navigated the media on one object face
    /// (`ObjectMediaNavigate`). The simulator has advanced the object's media
    /// version rather than replying with media data.
    ObjectMediaNavigated {
        /// The object whose media face was navigated.
        object_id: ObjectKey,
        /// The prim face navigated.
        face: u8,
        /// The URL the face was navigated to.
        url: String,
    },
    /// The client published environment settings (`ExtEnvironment` PUT). The
    /// update is already applied to the serving store
    /// ([`SimSession::set_environment`]); fire-and-forget for a driver
    /// persisting environments or notifying other clients (e.g. via
    /// [`SimSession::enqueue_windlight_refresh`]).
    EnvironmentUpdated {
        /// The updated parcel's region-local id, or `-1` for the region.
        parcel_id: i32,
        /// The single sky track the client scoped the update to, if any (the
        /// serving store applies the update wholesale either way).
        track_no: Option<i32>,
        /// The parsed update the store merged.
        update: Box<EnvironmentUpdate>,
    },
    /// The client created an inventory folder over the `InventoryAPIv3`
    /// create verb or the `CreateInventoryCategory` capability. The folder is
    /// already applied to the session's serving tree
    /// ([`SimSession::agent_inventory`]); fire-and-forget for a driver
    /// persisting inventory.
    InventoryCategoryCreated {
        /// The created folder as stored (version 1, parent set).
        folder: Box<InventoryFolder>,
    },
    /// The client created inventory links (`InventoryAPIv3` create with a
    /// `links` payload — the Current Outfit Folder wear path). Applied to the
    /// serving tree; fire-and-forget.
    InventoryLinksCreated {
        /// The created link items as stored.
        links: Vec<InventoryItem>,
    },
    /// The client renamed an inventory folder (`InventoryAPIv3`
    /// `PATCH /category/<id>` with `{ name }`). Applied to the serving tree.
    InventoryCategoryRenamed {
        /// The renamed folder.
        folder_id: InventoryFolderKey,
        /// Its new name.
        name: String,
    },
    /// The client moved an inventory folder (`InventoryAPIv3`
    /// `PATCH /category/<id>` with `{ parent_id }`). Applied to the serving
    /// tree.
    InventoryCategoryMoved {
        /// The moved folder.
        folder_id: InventoryFolderKey,
        /// Its new parent.
        parent_id: InventoryFolderKey,
    },
    /// The client updated an inventory item's name/description
    /// (`InventoryAPIv3` `PATCH /item/<id>`). Applied to the serving tree.
    InventoryItemUpdated {
        /// The updated item.
        item_id: InventoryKey,
        /// Its new name.
        name: String,
        /// Its new description.
        description: String,
    },
    /// The client moved an inventory item (`InventoryAPIv3` `PATCH
    /// /item/<id>` with `{ parent_id }`). Applied to the serving tree.
    InventoryItemMoved {
        /// The moved item.
        item_id: InventoryKey,
        /// The folder it now sits in.
        folder_id: InventoryFolderKey,
    },
    /// The client deleted an inventory folder (`InventoryAPIv3`
    /// `DELETE /category/<id>`). The whole subtree is already removed from
    /// the serving tree.
    InventoryCategoryRemoved {
        /// The deleted folder.
        folder_id: InventoryFolderKey,
        /// Every folder removed (the subtree, `folder_id` included).
        removed_folders: Vec<InventoryFolderKey>,
        /// Every item that was inside the subtree.
        removed_items: Vec<InventoryKey>,
    },
    /// The client emptied an inventory folder (`InventoryAPIv3`
    /// `DELETE /category/<id>/children`). The children are removed from the
    /// serving tree; the folder itself remains.
    InventoryCategoryPurged {
        /// The emptied folder (still present).
        folder_id: InventoryFolderKey,
        /// The removed child-folder subtrees.
        removed_folders: Vec<InventoryFolderKey>,
        /// Every removed item (direct and in removed subtrees).
        removed_items: Vec<InventoryKey>,
    },
    /// The client deleted an inventory item (`InventoryAPIv3`
    /// `DELETE /item/<id>`). Already removed from the serving tree.
    InventoryItemRemoved {
        /// The deleted item.
        item_id: InventoryKey,
    },
    /// The client set (or forgot) a per-experience preference
    /// (`ExperiencePreferences` PUT / DELETE). Already applied to the
    /// serving store ([`SimSession::experiences`]); fire-and-forget for a
    /// driver persisting agent preferences.
    ExperiencePermissionSet {
        /// The experience the preference addresses.
        experience_id: ExperienceKey,
        /// `Allow` / `Block` from the PUT body; `Forget` for the DELETE
        /// form.
        permission: ExperiencePermission,
    },
    /// The client edited an experience's metadata (`UpdateExperience`
    /// POST). Already applied to the serving store's record; fire-and-forget
    /// for a driver persisting experience profiles.
    ExperienceUpdated {
        /// The parsed edit the store applied (editable fields only — owner,
        /// quota and expiration are server-controlled and untouched).
        update: Box<ExperienceUpdate>,
    },
    /// The client replaced the region's experience lists
    /// (`RegionExperiences` POST). Already applied wholesale to the serving
    /// store; fire-and-forget for a driver persisting region settings.
    RegionExperiencesSet {
        /// The region's new allowed list.
        allowed: Vec<ExperienceKey>,
        /// The region's new blocked list.
        blocked: Vec<ExperienceKey>,
        /// The region's new trusted list.
        trusted: Vec<ExperienceKey>,
    },
    /// The client POSTed a `ProvisionVoiceAccountRequest` — a WebRTC offer
    /// (spatial or chat-session channel), a WebRTC logout, or a Vivox
    /// account request. Already answered from the voice stub
    /// ([`SimSession::voice`]); the `outcome` says how (an opened
    /// connection carries its minted `viewer_session`). Informational for a
    /// driver — a world-authority grid would hand the connection to its
    /// media server here.
    VoiceProvisionRequested {
        /// The decoded request, verbatim (the offer SDP included).
        request: Box<VoiceProvisionRequest>,
        /// What the stub did with it.
        outcome: VoiceProvisionOutcome,
    },
    /// The client trickled ICE candidates (or end-of-gathering) over
    /// `VoiceSignalingRequest`. Already recorded on the connection when
    /// `known`; a trickle for a `viewer_session` the stub never provisioned
    /// answers `404` and is surfaced with `known: false`.
    VoiceSignalingReceived {
        /// The connection the trickle belongs to.
        viewer_session: String,
        /// The candidates in this batch (empty for the end-of-gathering form).
        candidates: Vec<IceCandidate>,
        /// Whether this batch signalled end-of-gathering.
        completed: bool,
        /// Whether the `viewer_session` is a live connection.
        known: bool,
    },
    /// The client asked for its parcel's voice channel
    /// (`ParcelVoiceInfoRequest`). Already answered from the stub's parcel
    /// table for the agent's recorded parcel; informational.
    ParcelVoiceInfoRequested {
        /// The parcel the reply described (`-1` when unknown).
        parcel_local_id: RegionLocalParcelId,
        /// Whether the reply carried a channel (`false` = "no voice here").
        enabled: bool,
    },
    /// The client emailed a snapshot postcard (`SendPostcard`). The simulator
    /// renders and sends the email; fire-and-forget.
    PostcardReceived(Box<Postcard>),
    /// The client requested world-map blocks for a grid-coordinate rectangle
    /// (`MapBlockRequest`); the simulator answers with one or more
    /// [`SimSession::send_map_block_reply`] for the regions in range.
    MapBlockRequested {
        /// Minimum grid x in region-widths (inclusive).
        min_x: u16,
        /// Maximum grid x in region-widths (inclusive).
        max_x: u16,
        /// Minimum grid y in region-widths (inclusive).
        min_y: u16,
        /// Maximum grid y in region-widths (inclusive).
        max_y: u16,
        /// The request's map-layer flags.
        flags: MapRequestFlags,
    },
    /// The client searched the world map for regions by name
    /// (`MapNameRequest`); the simulator answers with the matching regions via
    /// [`SimSession::send_map_block_reply`].
    MapNameRequested {
        /// The region name (or prefix) to search for.
        name: String,
        /// The request's map-layer flags.
        flags: MapRequestFlags,
    },
    /// The client requested world-map overlay items of a given type
    /// (`MapItemRequest`); the simulator answers with
    /// [`SimSession::send_map_item_reply`].
    MapItemRequested {
        /// The kind of item requested (avatars, telehubs, land for sale, …).
        item_type: MapItemType,
        /// The target region handle (0 = the client's current region).
        region_handle: RegionHandle,
        /// The request's map-layer flags.
        flags: MapRequestFlags,
    },
    /// The client requested the world-map image-tile layers
    /// (`MapLayerRequest`); the simulator answers with
    /// [`SimSession::send_map_layer_reply`].
    MapLayerRequested {
        /// The request's map-layer flags.
        flags: MapRequestFlags,
    },
    /// The client requested a clean logout (`LogoutRequest`); the simulator has
    /// replied with a `LogoutReply` and closed the session.
    LoggedOut,
    /// The client offered its calling card to another agent
    /// (`OfferCallingCard`) — a reference card to this client's avatar, to be
    /// filed in the recipient's Calling Cards folder. This is *not* a friendship
    /// request. The simulator delivers it to `dest` (e.g. via
    /// [`SimSession::send_offer_calling_card`]), which replies with an accept or
    /// decline echoing `transaction`. The inverse of the client's
    /// [`Session::offer_calling_card`](crate::Session::offer_calling_card).
    CallingCardOffered {
        /// The agent the client is offering its calling card to.
        dest: AgentKey,
        /// Correlation id for the offer; the recipient echoes it when accepting
        /// or declining so the simulator can match the reply.
        transaction: TransactionId,
    },
    /// The client accepted a calling-card offer (`AcceptCallingCard`), filing the
    /// new card in `folder`. `transaction` echoes the original offer. The inverse
    /// of the client's
    /// [`Session::accept_calling_card`](crate::Session::accept_calling_card).
    CallingCardAccepted {
        /// Correlation id echoed from the original calling-card offer.
        transaction: TransactionId,
        /// The client's inventory folder the new calling card is filed in.
        folder: InventoryFolderKey,
    },
    /// The client declined a calling-card offer (`DeclineCallingCard`).
    /// `transaction` echoes the original offer. The inverse of the client's
    /// [`Session::decline_calling_card`](crate::Session::decline_calling_card).
    CallingCardDeclined {
        /// Correlation id echoed from the original calling-card offer.
        transaction: TransactionId,
    },
    /// The client set an object's path/profile geometry (`ObjectShape`). The
    /// inverse of the client's
    /// [`Session::set_object_shape`](crate::Session::set_object_shape). One event
    /// is emitted per object block in the message.
    ObjectShapeSet {
        /// The region-local id of the object being reshaped.
        local_id: RegionLocalObjectId,
        /// The new quantized path/profile geometry.
        shape: PrimShapeParams,
    },
    /// The client set an object's per-face textures / texture entry
    /// (`ObjectImage`). The inverse of the client's
    /// [`Session::set_object_image`](crate::Session::set_object_image). One event
    /// is emitted per object block in the message.
    ObjectImageSet {
        /// The region-local id of the object being retextured.
        local_id: RegionLocalObjectId,
        /// The legacy parcel-media URL, if any (an empty wire field is [`None`]).
        media_url: Option<String>,
        /// The new per-face texture entry.
        texture_entry: TextureEntry,
    },
    /// The client set an object's complete extra-parameter state
    /// (`ObjectExtraParams`): flexi/light/sculpt/mesh/light-image/render-material/
    /// reflection-probe. The inverse of the client's
    /// [`Session::set_object_extra_params`](crate::Session::set_object_extra_params).
    /// The message carries one block per subtype for an object — they are folded
    /// back into one [`ObjectExtraParams`], so a subtype sent not-in-use is
    /// absent (cleared) here. One event is emitted per distinct object.
    ObjectExtraParamsSet {
        /// The region-local id of the object whose parameters were set.
        local_id: RegionLocalObjectId,
        /// The object's complete extra-parameter state.
        params: ObjectExtraParams,
    },
    /// The client rezzed an inventory item into the world as a new object
    /// (`RezObject`). The inverse of the client's
    /// [`Session::rez_object_from_inventory`](crate::Session::rez_object_from_inventory)
    /// (distinct from [`RezObjectFromNotecard`](Self::RezObjectFromNotecard),
    /// which rezzes objects embedded in a notecard).
    RezObjectFromInventory {
        /// The ray placement, applied permission masks and the source inventory
        /// item being rezzed.
        params: RezObjectParams,
    },
    /// The client dropped a script inventory item into an in-world object's task
    /// inventory (`RezScript`). The inverse of the client's
    /// [`Session::rez_script`](crate::Session::rez_script).
    RezScript {
        /// The region-local id of the object whose task inventory receives the
        /// script.
        local_id: RegionLocalObjectId,
        /// The running flag, active group and the script inventory item.
        params: RezScriptParams,
    },
    /// The client revoked LSL script permissions previously granted to an object
    /// (`RevokePermissions`). The inverse of the client's
    /// [`Session::revoke_script_permissions`](crate::Session::revoke_script_permissions).
    RevokeScriptPermissions {
        /// The object whose granted permissions are revoked.
        object_id: ObjectKey,
        /// The permissions being revoked (an empty set revokes nothing).
        permissions: ScriptPermissions,
    },
    /// The client detached a worn attachment back into inventory, named by its
    /// inventory item id (`DetachAttachmentIntoInv`). The inverse of the client's
    /// [`Session::detach_attachment_into_inventory`](crate::Session::detach_attachment_into_inventory).
    DetachAttachmentIntoInventory {
        /// The inventory item id of the worn attachment being detached.
        item_id: InventoryKey,
    },
    /// The client asked for the task (object) inventory listing of an in-world
    /// object (`RequestTaskInventory`). The inverse of the client's
    /// [`Session::request_task_inventory`](crate::Session::request_task_inventory);
    /// a simulator answers with a `ReplyTaskInventory`.
    RequestTaskInventory {
        /// The region-local id of the object whose task inventory is requested.
        local_id: RegionLocalObjectId,
    },
    /// The client wrote an inventory item into an in-world object's task
    /// inventory (`UpdateTaskInventory`). The inverse of the client's
    /// [`Session::update_task_inventory`](crate::Session::update_task_inventory).
    UpdateTaskInventory {
        /// The region-local id of the object whose task inventory is written.
        local_id: RegionLocalObjectId,
        /// Whether the simulator matches the existing item by item id or asset id.
        key: TaskInventoryKey,
        /// The full inventory item being written.
        item: RestoreItem,
    },
    /// The client moved a task inventory item out of an in-world object into an
    /// agent inventory folder (`MoveTaskInventory`). The inverse of the client's
    /// [`Session::move_task_inventory`](crate::Session::move_task_inventory).
    MoveTaskInventory {
        /// The region-local id of the object the item is moved out of.
        local_id: RegionLocalObjectId,
        /// The agent inventory folder the item is moved into.
        folder_id: InventoryFolderKey,
        /// The inventory item id being moved.
        item_id: InventoryKey,
    },
    /// The client removed a task inventory item from an in-world object
    /// (`RemoveTaskInventory`). The inverse of the client's
    /// [`Session::remove_task_inventory`](crate::Session::remove_task_inventory).
    RemoveTaskInventory {
        /// The region-local id of the object the item is removed from.
        local_id: RegionLocalObjectId,
        /// The inventory item id being removed.
        item_id: InventoryKey,
    },
    /// The client asked to download a file over the legacy `Xfer` path
    /// (`RequestXfer`). When the named file was registered
    /// ([`SimSession::register_xfer_file`]) the simulator began streaming it;
    /// otherwise it refused with an `AbortXfer`. The inverse of the client's
    /// [`Session::request_xfer`](crate::Session::request_xfer).
    XferRequested {
        /// The client-chosen transfer id.
        xfer_id: XferId,
        /// The requested filename.
        filename: String,
        /// Whether a registered file matched and streaming began.
        served: bool,
    },
    /// The client confirmed the final packet of a served `Xfer` file send —
    /// the download completed. The inverse of the client's
    /// [`Event::XferDownloaded`](crate::Event::XferDownloaded).
    XferServed {
        /// The transfer id.
        xfer_id: XferId,
        /// The filename the file was registered under.
        filename: String,
        /// The number of file bytes streamed.
        byte_count: usize,
    },
    /// A named file the simulator pulled from the client with
    /// [`SimSession::request_xfer_upload`] arrived in full — the server side
    /// of the client's [`Event::XferUploaded`](crate::Event::XferUploaded)
    /// (the terrain RAW upload).
    XferReceived {
        /// The transfer id the pull was issued under.
        xfer_id: XferId,
        /// The filename the pull named.
        filename: String,
        /// The assembled file bytes (length prefix stripped).
        data: Vec<u8>,
    },
    /// The client aborted an in-flight `Xfer` transfer (`AbortXfer`), in
    /// either direction. The inverse of the client's
    /// [`Event::XferAborted`](crate::Event::XferAborted).
    XferAborted {
        /// The transfer id.
        xfer_id: XferId,
        /// The abort result code.
        result: i32,
    },
    /// The client started a legacy transaction asset upload
    /// (`AssetUploadRequest`) — the in-place wearable-save path. Small assets
    /// arrive inline and complete immediately; an oversized one is pulled from
    /// the client over `Xfer` first. Completion (either way) surfaces
    /// separately as [`ServerEvent::AssetUploaded`]. The inverse of the
    /// client's
    /// [`Session::save_inventory_asset`](crate::Session::save_inventory_asset).
    AssetUploadRequested {
        /// The upload's transaction id (the stored asset id is
        /// `combine(transaction_id, secure_session_id)`).
        transaction_id: TransactionId,
        /// The declared asset type.
        asset_type: AssetType,
        /// Whether the asset bytes were carried inline (small upload) rather
        /// than pulled over `Xfer`.
        inline: bool,
        /// Whether the asset is a temporary upload.
        tempfile: bool,
        /// Whether the asset should only be stored sim-locally.
        store_local: bool,
    },
    /// A legacy transaction asset upload finished: the asset bytes are fully
    /// received (inline, or over the `Xfer` pull) and the simulator has
    /// replied with an `AssetUploadComplete`. The inverse of the client's
    /// [`Event::InventoryAssetSaved`](crate::Event::InventoryAssetSaved).
    AssetUploaded {
        /// The stored asset id, `combine(transaction_id, secure_session_id)`.
        asset_id: AssetKey,
        /// The asset type.
        asset_type: AssetType,
        /// The upload's transaction id.
        transaction_id: TransactionId,
        /// The complete asset bytes.
        data: Vec<u8>,
    },
    /// The client asked to download an asset over the legacy UDP Transfer
    /// path (`TransferRequest`) from a source that is still UDP-only on both
    /// grids (task-inventory item asset, estate covenant). The driver answers
    /// with [`SimSession::send_transfer_asset`] or
    /// [`SimSession::send_transfer_fail`]. The inverse of the client's
    /// [`Session::fetch_task_item_asset`](crate::Session::fetch_task_item_asset)
    /// / [`Session::fetch_estate_covenant_asset`](crate::Session::fetch_estate_covenant_asset).
    TransferRequested {
        /// The client-minted transfer id, to pass back to the answer.
        transfer_id: TransferId,
        /// The transfer priority the client asked for.
        priority: f32,
        /// The decoded request source.
        source: TransferRequestSource,
    },
    /// A client sent a `TransferRequest` for the plain asset-by-id source
    /// (`LLTST_ASSET`), the path superseded by the `ViewerAsset` capability on
    /// both grids. It was refused with an unknown-source `TransferInfo` per the
    /// legacy-skip rule; the decoded params (`None` if malformed) say what the
    /// client wanted so a driver can log it.
    LegacyAssetTransferRefused {
        /// The client's transfer id.
        transfer_id: TransferId,
        /// The requested asset id and type, if the params blob decoded.
        params: Option<TransferSourceParamsAsset>,
    },
    /// The client cancelled an in-flight asset Transfer (`TransferAbort`).
    /// The inverse of the client's
    /// [`Session::abort_transfer`](crate::Session::abort_transfer).
    TransferAborted {
        /// The transfer id that was cancelled.
        transfer_id: TransferId,
    },
    /// The client applied a terraform brush stroke (`ModifyLand`). The inverse
    /// of the client's
    /// [`Session::modify_land`](crate::Session::modify_land).
    ModifyLand {
        /// The decoded terraform edit (action, brush, strength, area, parcel).
        edit: LandEdit,
    },
    /// The client undid its last terraform edit (`UndoLand`). The inverse of the
    /// client's [`Session::undo_land`](crate::Session::undo_land).
    UndoLand,
    /// The client requested a parcel's properties by its region-local id
    /// (`ParcelPropertiesRequestByID`). The inverse of the client's
    /// [`Session::request_parcel_properties_by_id`](crate::Session::request_parcel_properties_by_id);
    /// a simulator answers with a `ParcelProperties`.
    RequestParcelPropertiesById {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// The query sequence id, echoed back in the reply.
        sequence_id: i32,
    },
    /// The client requested the properties of the parcel(s) under a metre
    /// rectangle (`ParcelPropertiesRequest`). The inverse of the client's
    /// [`Session::request_parcel_properties`](crate::Session::request_parcel_properties);
    /// a simulator answers with a `ParcelProperties` per covered parcel (see
    /// [`SimSession::send_parcel_properties`]).
    RequestParcelProperties {
        /// The rectangle's west edge, in metres.
        west: f32,
        /// The rectangle's south edge, in metres.
        south: f32,
        /// The rectangle's east edge, in metres.
        east: f32,
        /// The rectangle's north edge, in metres.
        north: f32,
        /// The query sequence id, echoed back in the reply (the viewer uses
        /// the negative "agent parcel" / "hover parcel" sentinels here).
        sequence_id: i32,
        /// Whether the viewer asked for the reply to snap to parcel bounds.
        snap_selection: bool,
    },
    /// The client asked the simulator to (re)send full updates for objects it
    /// is missing (`RequestMultipleObjects`). The inverse of the client's
    /// [`Session::request_objects`](crate::Session::request_objects); a
    /// simulator answers with `ObjectUpdate`s (see
    /// [`SimSession::send_object_update`]).
    RequestObjects {
        /// The requested objects' region-local ids, with the cache-miss kind
        /// the client reported for each (`0` = full miss, `1` = CRC mismatch).
        objects: Vec<(RegionLocalObjectId, u8)>,
    },
    /// The client set a parcel's auto-return time for other people's objects
    /// (`ParcelSetOtherCleanTime`). The inverse of the client's
    /// [`Session::set_parcel_other_clean_time`](crate::Session::set_parcel_other_clean_time).
    SetParcelOtherCleanTime {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// The auto-return time, in whole minutes on the wire. [`Duration::ZERO`]
        /// disables auto-return.
        ///
        /// [`Duration::ZERO`]: std::time::Duration::ZERO
        clean_time: std::time::Duration,
    },
    /// The client created an inventory link (`LinkInventoryItem`). The inverse of
    /// the client's
    /// [`Session::link_inventory_item`](crate::Session::link_inventory_item); a
    /// simulator allocates the link item's id and answers with a
    /// `BulkUpdateInventory` echoing `callback_id`.
    LinkInventoryItem {
        /// The new link's folder, target, name/description, and asset/inv type
        /// codes.
        link: NewInventoryLink,
        /// The client's async callback id, echoed back in the reply so the client
        /// can correlate it.
        callback_id: u32,
    },
    /// The client edited a group's profile (`UpdateGroupInfo`): charter, insignia,
    /// search visibility, membership fee, enrollment, and publish flags. The
    /// inverse of the client's
    /// [`Session::update_group_info`](crate::Session::update_group_info).
    UpdateGroupInfo {
        /// The decoded group-profile edit (a group cannot be renamed, so this
        /// carries no name).
        params: UpdateGroupInfoParams,
    },
    /// The client set its active title within a group (`GroupTitleUpdate`). The
    /// inverse of the client's
    /// [`Session::update_group_title`](crate::Session::update_group_title).
    UpdateGroupTitle {
        /// The group whose title is being changed.
        group_id: GroupKey,
        /// The group role carrying the desired title.
        title_role_id: GroupRoleKey,
    },
    /// The client requested a teleport to a landmark (`TeleportLandmarkRequest`).
    /// The inverse of the client's
    /// [`Session::teleport_via_landmark`](crate::Session::teleport_via_landmark);
    /// the simulator resolves the destination and answers with a
    /// `TeleportFinish`.
    TeleportViaLandmark {
        /// The landmark inventory item's *asset* id, or `None` (a nil wire
        /// `LandmarkID`) to teleport to the agent's home location.
        landmark: Option<AssetKey>,
    },
    /// The client requested a teleport to an explicit region handle and
    /// position (`TeleportLocationRequest`). The inverse of the client's
    /// [`Session::teleport_to`](crate::Session::teleport_to). The driver
    /// answers with the [`SimSession::send_teleport_start`] /
    /// [`send_teleport_progress`](SimSession::send_teleport_progress) /
    /// [`send_teleport_local`](SimSession::send_teleport_local) /
    /// [`send_teleport_failed`](SimSession::send_teleport_failed) mechanics —
    /// or, for an inter-region teleport, the CAPS event-queue trio
    /// [`enqueue_enable_simulator`](SimSession::enqueue_enable_simulator) /
    /// [`enqueue_establish_agent_communication`](SimSession::enqueue_establish_agent_communication)
    /// / [`enqueue_teleport_finish`](SimSession::enqueue_teleport_finish).
    TeleportRequested {
        /// The destination region handle.
        region_handle: RegionHandle,
        /// The destination position within the region.
        position: RegionCoordinates,
        /// The direction the avatar should face on arrival.
        look_at: Vector,
    },
    /// The client accepted a teleport lure and asks to be teleported to the
    /// lure's destination (`TeleportLureRequest`). The inverse of the client's
    /// [`Session::accept_teleport_lure`](crate::Session::accept_teleport_lure).
    TeleportViaLure {
        /// The lure offer id the client is accepting (the offering IM's id).
        lure_id: LureId,
        /// The teleport flags the client echoed (`via lure`, godlike variants).
        teleport_flags: u32,
    },
    /// The client cancelled an in-progress teleport (`TeleportCancel`). The
    /// inverse of the client's
    /// [`Session::cancel_teleport`](crate::Session::cancel_teleport).
    CancelTeleport,
    /// The client asked to sit on an object (`AgentRequestSit`). The inverse
    /// of the client's [`Session::sit_on`](crate::Session::sit_on). The
    /// driver answers with [`SimSession::send_avatar_sit_response`] (the
    /// client then completes the handshake with `AgentSit`).
    SitRequested {
        /// The object the client wants to sit on.
        target: ObjectKey,
        /// The clicked sit offset relative to the object, in metres.
        offset: Vector,
    },
    /// The client completed the sit handshake (`AgentSit`). `on` is the seat
    /// from the outstanding [`SimSession::send_avatar_sit_response`] (the
    /// agent is now seated — [`SimSession::seated_on`] reports it); `None`
    /// for an unsolicited `AgentSit`, which leaves the sit state untouched
    /// (mirroring the client ignoring an unsolicited `AvatarSitResponse`).
    SitConfirmed {
        /// The object sat upon, when a sit response was outstanding.
        on: Option<ObjectKey>,
    },
    /// The client stood up: an `AgentUpdate` carried the transient
    /// `STAND_UP` control flag while the agent was seated (or awaiting the
    /// completing `AgentSit`). The inverse of the client's
    /// [`Session::stand`](crate::Session::stand). The sit state resets to
    /// not-sitting.
    StoodUp,
    /// The client recorded a start location (`SetStartLocationRequest`): stores
    /// the region-local `position` and `look_at` as the named [`StartLocationSlot`]
    /// (the everyday case being [`StartLocationSlot::Home`], "set home to here").
    /// The inverse of the client's
    /// [`Session::set_start_location`](crate::Session::set_start_location). The
    /// wire `SimName` is empty — the simulator is expected to fill in the current
    /// region's name.
    SetStartLocation {
        /// Which start-location slot to record.
        slot: StartLocationSlot,
        /// The region-local position to record.
        position: RegionCoordinates,
        /// The region-local look-at direction to record.
        look_at: Vector,
    },
    /// The client polled for a fresh `AgentDataUpdate` without changing any agent
    /// data (`AgentDataUpdateRequest`). The inverse of the client's
    /// [`Session::request_agent_data_update`](crate::Session::request_agent_data_update);
    /// the simulator answers with an `AgentDataUpdate`.
    RequestAgentDataUpdate,
    /// The client quit leaving its in-world objects behind (`AgentQuitCopy`) — the
    /// "crash quit" the reference viewer sends so a subsequent login can recover
    /// rezzed objects. The inverse of the client's
    /// [`Session::quit_copy`](crate::Session::quit_copy).
    QuitCopy {
        /// The circuit code carried in the `FuseBlock`, echoing the client's own
        /// circuit code.
        viewer_circuit_code: CircuitCode,
    },
    /// The client toggled simulator-side velocity interpolation of object motion
    /// (`VelocityInterpolateOn` / `VelocityInterpolateOff`). The inverse of the
    /// client's
    /// [`Session::set_velocity_interpolation`](crate::Session::set_velocity_interpolation).
    SetVelocityInterpolation {
        /// `true` for `VelocityInterpolateOn`, `false` for
        /// `VelocityInterpolateOff`.
        enabled: bool,
    },
    /// The client requested its own account contact preferences
    /// (`UserInfoRequest`). The inverse of the client's
    /// [`Session::request_user_info`](crate::Session::request_user_info); the
    /// simulator answers with a `UserInfoReply`.
    RequestUserInfo,
    /// The client updated its account contact preferences (`UpdateUserInfo`):
    /// whether offline instant messages are forwarded to email and the
    /// directory/search visibility. The inverse of the client's
    /// [`Session::update_user_info`](crate::Session::update_user_info). The email
    /// address itself is not settable over this message (the wire block carries no
    /// email field), so it is absent here.
    UpdateUserInfo {
        /// Whether offline instant messages are forwarded to the agent's email.
        im_via_email: bool,
        /// The agent's directory/search visibility setting.
        directory_visibility: DirectoryVisibility,
    },
    /// The client triggered a one-shot spatial sound (`SoundTrigger`): play
    /// `sound` at the region-local `position` (within `region_handle`) with linear
    /// `gain`. The inverse of the client's
    /// [`Session::trigger_sound`](crate::Session::trigger_sound). The wire
    /// owner/object/parent ids are nil for a viewer-originated trigger — the
    /// simulator fills them in — so they are not surfaced here.
    TriggerSound {
        /// The sound asset to play.
        sound: AssetKey,
        /// The linear gain (`0.0`..=`1.0`).
        gain: f32,
        /// The region the sound plays in.
        region_handle: RegionHandle,
        /// The region-local position to play the sound at.
        position: RegionCoordinates,
    },
    /// The client asked the simulator to grant or drop god powers for it
    /// (`RequestGodlikePowers`). The inverse of the client's
    /// [`Session::request_godlike_powers`](crate::Session::request_godlike_powers).
    /// The agent must actually hold god rights on the grid for the request to
    /// succeed; the grant is delivered to the viewer as a `GrantGodlikePowers`.
    /// The wire `Token` is nil for a viewer-originated request, so it is not
    /// surfaced here.
    RequestGodlikePowers {
        /// Whether the client is asking to acquire (`true`) or drop (`false`) god
        /// powers.
        godlike: bool,
    },
    /// The client ejected an avatar from its land (`EjectUser`), optionally also
    /// banning them. The inverse of the client's
    /// [`Session::eject_user`](crate::Session::eject_user).
    EjectUser {
        /// The avatar being ejected.
        target: AgentKey,
        /// Whether to eject only or eject and ban.
        action: EjectAction,
    },
    /// The client froze or unfroze an avatar on its land (`FreezeUser`). The
    /// inverse of the client's
    /// [`Session::freeze_user`](crate::Session::freeze_user).
    FreezeUser {
        /// The avatar being frozen or unfrozen.
        target: AgentKey,
        /// Whether to freeze or unfreeze the avatar.
        action: FreezeAction,
    },
    /// The client requested a region-wide delete (or return) of an owner's
    /// objects (`SimWideDeletes`). The inverse of the client's
    /// [`Session::sim_wide_deletes`](crate::Session::sim_wide_deletes). Needs
    /// estate-manager or god rights.
    SimWideDeletes {
        /// The owner whose objects are deleted or returned.
        owner: AgentKey,
        /// Which of the owner's objects the operation targets.
        flags: SimWideDeleteFlags,
    },
    /// The client pushed god-tools region parameters (`GodUpdateRegionInfo`). The
    /// inverse of the client's
    /// [`Session::god_update_region_info`](crate::Session::god_update_region_info).
    /// The simulator overwrites the region's parameters wholesale from `update`.
    /// Needs grid-god rights.
    GodUpdateRegionInfo {
        /// The region parameters to apply.
        update: GodRegionUpdate,
    },
    /// The client force-reassigned a parcel's ownership (`ParcelGodForceOwner`).
    /// The inverse of the client's
    /// [`Session::parcel_god_force_owner`](crate::Session::parcel_god_force_owner).
    /// Needs grid-god rights.
    ParcelGodForceOwner {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// The avatar to make the new owner of the parcel.
        owner: OwnerKey,
    },
    /// The client marked a parcel (and its content) as owned by the
    /// governor/maintenance account (`ParcelGodMarkAsContent`). The inverse of
    /// the client's
    /// [`Session::parcel_god_mark_as_content`](crate::Session::parcel_god_mark_as_content).
    /// Needs grid-god rights.
    ParcelGodMarkAsContent {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
    },
    /// The client deleted an events-directory listing and asked the simulator to
    /// re-run the search (`EventGodDelete`). The inverse of the client's
    /// [`Session::event_god_delete`](crate::Session::event_god_delete); the
    /// simulator answers with a refreshed `DirEventsReply` correlated by
    /// `query_id`. Needs grid-god rights.
    EventGodDelete {
        /// The events-directory listing to delete.
        event: EventId,
        /// The client-chosen id to echo back in the refreshed reply.
        query_id: QueryId,
        /// The events search text to re-run.
        query_text: String,
        /// What to search and how to sort/filter.
        flags: DirFindFlags,
        /// The 0-based index of the first result the client wants.
        query_start: i32,
    },
    /// The client asked the simulator to save the region (world) state
    /// (`StateSave`). The inverse of the client's
    /// [`Session::state_save`](crate::Session::state_save). Needs grid-god rights.
    StateSave {
        /// The target filename, or [`None`] to let the simulator pick the
        /// autosave name (an empty filename on the wire).
        filename: Option<String>,
    },
    /// The client started a land auction on a parcel (`ViewerStartAuction`). The
    /// inverse of the client's
    /// [`Session::viewer_start_auction`](crate::Session::viewer_start_auction).
    /// Needs grid-god rights.
    ViewerStartAuction {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// The snapshot texture advertising the auction, or [`None`] for none (a
        /// nil id on the wire).
        snapshot: Option<TextureKey>,
    },
    /// Any other decoded client message, surfaced verbatim. This is how the
    /// remaining client-only messages reach the simulator: fully decoded but
    /// without a dedicated typed variant.
    ClientMessage(Box<AnyMessage>),
    /// The link was lost without a clean logout (the inactivity timeout elapsed
    /// or a reliable packet exhausted its retransmission budget).
    Disconnected,
}

/// A simulator-side session: one client's UDP circuit, modelled as a pure state
/// machine.
///
/// See the module documentation for the I/O contract. Construct it with
/// [`SimSession::new`], feed inbound datagrams via [`SimSession::handle_datagram`]
/// and timeouts via [`SimSession::handle_timeout`], push server messages with
/// [`SimSession::push`] (or the typed helpers), enqueue CAPS events with
/// [`SimSession::enqueue_caps_event`], and drain output with
/// [`SimSession::poll_transmit`], [`SimSession::poll_event`] and
/// [`SimSession::poll_timeout`].
#[derive(Debug)]
pub struct SimSession {
    /// The current lifecycle state.
    state: SimState,
    /// The region handle this simulator serves (echoed in `AgentMovementComplete`).
    region_handle: RegionHandle,
    /// The channel/version string reported in `AgentMovementComplete`.
    channel_version: Vec<u8>,
    /// The client's UDP address, learned from the first inbound datagram.
    client_addr: Option<SocketAddr>,
    /// The agent id, from `UseCircuitCode`.
    agent_id: Option<AgentKey>,
    /// The session id, from `UseCircuitCode`.
    session_id: Option<Uuid>,
    /// The circuit code, from `UseCircuitCode`.
    circuit_code: Option<CircuitCode>,
    /// The next outgoing sequence number.
    next_sequence: SequenceNumber,
    /// The next `StartPingCheck` ping id.
    next_ping_id: PingId,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<SequenceNumber>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<SequenceNumber, UnackedPacket>,
    /// Recently seen inbound reliable sequence numbers.
    seen: SeenWindow,
    /// Datagrams ready to be transmitted to the client.
    out: VecDeque<Vec<u8>>,
    /// When the link is declared dead for lack of inbound traffic.
    inactivity: Instant,
    /// When to flush owed acknowledgements, if any are pending.
    ack_flush: Option<Instant>,
    /// When to send the next periodic `StartPingCheck`, once active.
    ping: Option<Instant>,
    /// The CAPS `EventQueueGet` events enqueued for the client, awaiting a
    /// long-poll.
    caps_events: Vec<EventQueueEvent>,
    /// The id of the next `EventQueueGet` batch (echoed as the client's next
    /// `ack`).
    event_queue_id: i32,
    /// Files registered for the client to download over the legacy `Xfer`
    /// path, keyed by filename. An entry is consumed by the `RequestXfer`
    /// that starts its download (requests ask for delete-on-completion), so
    /// re-serving a name needs a fresh
    /// [`SimSession::register_xfer_file`].
    xfer_files: BTreeMap<String, Vec<u8>>,
    /// Outbound `Xfer` file sends in flight, keyed by the client-chosen id.
    xfer_sends: BTreeMap<XferId, SimXferSend>,
    /// Inbound `Xfer` asset pulls in flight (oversized legacy uploads),
    /// keyed by the simulator-assigned id.
    xfer_receives: BTreeMap<XferId, SimXferReceive>,
    /// The next simulator-assigned `Xfer` id for an asset pull (never zero).
    next_xfer_id: XferId,
    /// The account's secure session id — the extra entropy the legacy asset
    /// upload path combines with a transaction id to derive the stored asset
    /// id. `None` until [`SimSession::set_secure_session_id`]; an upload
    /// arriving while unset is refused with a failed `AssetUploadComplete`.
    secure_session_id: Option<Uuid>,
    /// Asset Transfer requests awaiting the driver's answer, keyed by the
    /// client-minted [`TransferId`] and holding the raw request params to echo
    /// back in the `TransferInfo` (as the reference serving side does).
    transfer_serves: BTreeMap<TransferId, Vec<u8>>,
    /// Whether this circuit hosts a child or the root agent: `Child` from
    /// `UseCircuitCode`, promoted to `Root` by `CompleteAgentMovement`.
    agent_presence: AgentPresence,
    /// The agent's sit state (the server-side mirror of the client's sit
    /// machine).
    sit: SimSitState,
    /// Outstanding `ScriptQuestion`s awaiting the client's `ScriptAnswerYes`,
    /// keyed by (task, item) and holding the asked permission set
    /// ([`SimSession::script_question`]).
    script_questions: BTreeMap<(ObjectKey, InventoryKey), ScriptPermissions>,
    /// Recorded script-permission answers — the server twin of the client's
    /// grant registry ([`SimSession::script_grant`]). An empty
    /// [`ScriptPermissions`] is an explicit deny, distinct from an absent
    /// (never-answered) holder.
    script_grants: BTreeMap<(ObjectKey, InventoryKey), ScriptPermissions>,
    /// The live group/conference chat sessions, keyed by the wire session id
    /// (a group session's id **is** the group id) —
    /// [`SimSession::chat_session`].
    chat_sessions: BTreeMap<ImSessionId, SimChatSession>,
    /// Instant messages stored while the agent was offline, awaiting the
    /// deliver-once `ReadOfflineMsgs` fetch ([`SimSession::take_offline_messages`]).
    offline_messages: Vec<InstantMessage>,
    /// The people-service display-name store the `GetDisplayNames` capability
    /// serves from, keyed by agent ([`SimSession::set_display_name`]).
    display_names: BTreeMap<AgentKey, DisplayName>,
    /// The agent's server-stored preferences, served and updated by the
    /// `AgentPreferences` capability ([`SimSession::agent_preferences`]).
    agent_preferences: AgentPreferences,
    /// An abuse report parked by the first `SendUserReportWithScreenshot` step
    /// until the second step delivers the screenshot bytes.
    pending_report_screenshot: Option<Box<AbuseReport>>,
    /// Two-stage CAPS uploads parked between step 1 (the metadata POST, which
    /// mints the uploader URL) and step 2 (the raw-bytes POST that completes
    /// them), keyed by capability name — one in-flight upload per cap, the same
    /// shape as [`pending_report_screenshot`](Self::pending_report_screenshot)
    /// generalised across the whole `Update*`/`NewFile*` family.
    pending_caps_uploads: BTreeMap<&'static str, CapsUploadMetadata>,
    /// A monotonic source for the ids the two-stage uploader mints on
    /// completion (`new_asset` / `new_inventory_item`) and the `ObjectMedia`
    /// version serials — a sim-server simplification. A real grid mints random
    /// asset ids; the client stores whatever id it is handed, so the value's
    /// structure is immaterial, and a deterministic counter keeps `SimSession`
    /// pure (no clock, no RNG).
    next_sim_serial: u128,
    /// The GLTF/legacy materials the `RenderMaterials` query serves, keyed by
    /// material id ([`SimSession::set_region_material`]). Driver-populated: the
    /// authoritative prim/material database is out of scope, so this is a small
    /// serving store like [`display_names`](Self::display_names).
    region_materials: BTreeMap<Uuid, LegacyMaterial>,
    /// The per-object media the `ObjectMedia` GET serves, keyed by object
    /// ([`SimSession::set_object_media`]). Driver-populated.
    object_media: BTreeMap<ObjectKey, ObjectMediaState>,
    /// The agent's inventory tree the inventory capabilities
    /// (`FetchInventoryDescendents2`, `FetchInventory2`, `InventoryAPIv3`,
    /// `CreateInventoryCategory`) serve from. Driver-populated
    /// ([`SimSession::agent_inventory_mut`]) like
    /// [`display_names`](Self::display_names), but the AIS3 mutations apply
    /// to it — fixture state, not world authority — so follow-up fetches
    /// observe them.
    agent_inventory: SimInventoryTree,
    /// The read-only shared-Library tree the `FetchLibDescendents2` /
    /// `FetchLib2` / `LibraryAPIv3` capabilities serve from.
    /// Driver-populated ([`SimSession::library_inventory_mut`]); the
    /// mutation caps never touch it (`LibraryAPIv3` is GET-only).
    library_inventory: SimInventoryTree,
    /// The feature document the `SimulatorFeatures` capability serves.
    /// Driver-populated ([`SimSession::set_simulator_features`]); its
    /// `lsl_syntax_id` is owned by [`SimSession::set_lsl_syntax`] so the
    /// advertised id always matches the served `LSLSyntax` document.
    simulator_features: SimulatorFeatures,
    /// The LSL syntax document the `LSLSyntax` capability serves.
    /// Driver-populated ([`SimSession::set_lsl_syntax`]).
    lsl_syntax: LslSyntax,
    /// The environment settings the `ExtEnvironment` capability serves and
    /// updates, keyed by parcel id (`-1` = the region entry, seeded at
    /// construction and never removed; a parcel without its own entry falls
    /// back to the region's — SL parcels inherit the region environment).
    environments: BTreeMap<i32, EnvironmentSettings>,
    /// The per-object costs the `GetObjectCost` capability serves, keyed by
    /// object ([`SimSession::set_object_cost`]). Driver-populated.
    object_costs: BTreeMap<ObjectKey, ObjectCost>,
    /// The per-object physics data the `GetObjectPhysicsData` capability
    /// serves, keyed by object ([`SimSession::set_object_physics`]).
    /// Driver-populated.
    object_physics: BTreeMap<ObjectKey, ObjectPhysicsData>,
    /// The per-object selection costs the `ResourceCostSelected` capability
    /// sums over, keyed by object ([`SimSession::set_selection_cost`]).
    /// Driver-populated; the request's roots/prims distinction validates the
    /// body but does not change the arithmetic — the driver stores whichever
    /// contributions it wants summed.
    selection_costs: BTreeMap<ObjectKey, SelectedResourceCost>,
    /// This region's id, matched against `RemoteParcelRequest` lookups (and
    /// useful to seed [`EnvironmentSettings::region_id`]). Nil until the
    /// driver sets it ([`SimSession::set_region_id`]).
    region_id: Uuid,
    /// The parcel-cover rectangles the `RemoteParcelRequest` capability
    /// resolves locations against ([`SimSession::add_parcel`]).
    /// Driver-populated, first containing rectangle wins.
    parcels: Vec<SimParcel>,
    /// The agent's scripted-attachment report the `AttachmentResources`
    /// capability serves ([`SimSession::set_attachment_resources`]).
    /// Driver-populated.
    attachment_resources: AttachmentResourcesReport,
    /// The parcel script-resource summary the `LandResources` follow-up
    /// summary GET serves ([`SimSession::set_land_resource_summary`]).
    /// Driver-populated; the POST's parcel id is validated but the stored
    /// report is served as-is — its scope is the driver's choice.
    land_resource_summary: ResourceSummary,
    /// The per-parcel script-resource details the `LandResources` follow-up
    /// details GET serves ([`SimSession::set_land_resource_details`]).
    /// Driver-populated.
    land_resource_details: Vec<ParcelScriptResources>,
    /// The experience fixture set the twelve experience capabilities serve
    /// from. Driver-populated ([`SimSession::experiences_mut`]) like
    /// [`display_names`](Self::display_names), but the three mutating caps
    /// (`ExperiencePreferences`, `UpdateExperience`, the
    /// `RegionExperiences` POST) apply to it — fixture state, not world
    /// authority — so follow-up reads observe them.
    experiences: SimExperiences,
    /// The voice signalling stub the three voice capabilities serve from
    /// ([`SimSession::voice_mut`] to enable a backend and seed parcel
    /// channels; the live WebRTC connections are its mutable state).
    voice: SimVoice,
    /// Pending events for the driver.
    events: VecDeque<ServerEvent>,
}

/// One parcel-cover rectangle the `RemoteParcelRequest` lookup resolves
/// against: `[west, east) × [south, north)`, in region-local metres.
#[derive(Debug, Clone, PartialEq)]
pub struct SimParcel {
    /// The parcel's id, answered on a hit.
    pub parcel_id: ParcelKey,
    /// The rectangle's west edge (inclusive), in metres.
    pub west: f32,
    /// The rectangle's south edge (inclusive), in metres.
    pub south: f32,
    /// The rectangle's east edge (exclusive), in metres.
    pub east: f32,
    /// The rectangle's north edge (exclusive), in metres.
    pub north: f32,
}

/// The parsed step-1 metadata of a two-stage CAPS upload, parked in
/// [`SimSession`] until the raw-bytes step completes it. One variant per upload
/// family; carries exactly what the completion event needs to describe the
/// stored asset.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum CapsUploadMetadata {
    /// `NewFileAgentInventory` — store a new asset and create an inventory item.
    NewFileInventory(NewFileAgentInventoryRequest),
    /// `UploadBakedTexture` — a temporary avatar bake (no inventory item).
    BakedTexture,
    /// An `Update*AgentInventory` asset replacement (gesture / notecard /
    /// settings / material), carrying the cap name and the item being updated.
    UpdateAgentItem {
        /// The capability name (which asset kind is being replaced).
        cap: String,
        /// The agent-inventory item whose asset is replaced.
        item_id: InventoryKey,
    },
    /// An `Update*TaskInventory` asset replacement (notecard task), carrying the
    /// holding object and the item within it.
    UpdateTaskItem {
        /// The capability name.
        cap: String,
        /// The in-world object holding the task inventory.
        task_id: ObjectKey,
        /// The task-inventory item whose asset is replaced.
        item_id: InventoryKey,
    },
    /// `UpdateScriptAgent` — replace and compile an agent-inventory script.
    UpdateScriptAgent(UpdateScriptAgentRequest),
    /// `UpdateScriptTask` — replace and compile a task-inventory script.
    UpdateScriptTask(UpdateScriptTaskRequest),
}

impl CapsUploadMetadata {
    /// Whether this upload compiles a script — its completion reply carries the
    /// `{ compiled, errors }` result the client folds into
    /// [`Event::ScriptUploaded`](crate::Event::ScriptUploaded).
    #[must_use]
    pub(crate) const fn is_script(&self) -> bool {
        matches!(self, Self::UpdateScriptAgent(_) | Self::UpdateScriptTask(_))
    }

    /// Whether this upload creates or updates an inventory item (so the
    /// completion carries a `new_inventory_item`). `UploadBakedTexture` is the
    /// sole exception — a temporary bake produces no inventory item.
    const fn creates_inventory_item(&self) -> bool {
        !matches!(self, Self::BakedTexture)
    }
}

/// The per-object media state the `ObjectMedia` GET serves — the media version
/// string and the per-face media entries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObjectMediaState {
    /// The media version string (`x-mv:<serial>/<uuid>`), advanced on every
    /// media change.
    pub version: String,
    /// Per-face media, one slot per prim face in order; `None` for a face
    /// without media.
    pub faces: Vec<Option<MediaEntry>>,
}

/// The environment a fresh session's region entry starts from: SL's stock
/// four-hour day (`day_length` 14400 s, `day_offset` 57600 s), version 1, the
/// default sky-track altitude breakpoints, and an empty day cycle (both codec
/// directions tolerate empty tracks/frames) named "Default Daycycle".
fn default_region_environment() -> EnvironmentSettings {
    EnvironmentSettings {
        parcel_id: -1,
        region_id: Uuid::nil(),
        day_length: 14400,
        day_offset: 57600,
        flags: 0,
        env_version: 1,
        track_altitudes: [1000.0, 2000.0, 3000.0],
        day_cycle: DayCycle {
            name: "Default Daycycle".to_owned(),
            water_track: Vec::new(),
            sky_tracks: Vec::new(),
            sky_frames: BTreeMap::new(),
            water_frames: BTreeMap::new(),
        },
    }
}

/// The `AgentPreferences` set a fresh session starts from — OpenSim's stored
/// defaults (`IAgentPreferencesService.cs`): hover height `0.0`, zero default
/// permission masks, access ceiling `"M"`, language `"en-us"` marked public,
/// and god level `0`. Every field is `Some` so the capability always echoes a
/// full set.
fn default_agent_preferences() -> AgentPreferences {
    AgentPreferences {
        hover_height: Some(0.0),
        default_object_perm_masks: Some(ObjectPermMasks {
            group: 0,
            everyone: 0,
            next_owner: 0,
        }),
        max_access_pref: Some("M".to_owned()),
        language: Some("en-us".to_owned()),
        language_is_public: Some(true),
        god_level: Some(0),
    }
}

impl SimSession {
    /// Creates a simulator session serving `region_handle`, armed with the
    /// inactivity timer at `now`. The session awaits the circuit until the
    /// client sends `UseCircuitCode`.
    #[must_use]
    pub fn new(region_handle: RegionHandle, now: Instant) -> Self {
        Self {
            state: SimState::AwaitingCircuit,
            region_handle,
            channel_version: b"sl-proto SimSession".to_vec(),
            client_addr: None,
            agent_id: None,
            session_id: None,
            circuit_code: None,
            next_sequence: SequenceNumber::FIRST,
            next_ping_id: PingId(1),
            pending_acks: Vec::new(),
            unacked: BTreeMap::new(),
            seen: SeenWindow::default(),
            out: VecDeque::new(),
            inactivity: deadline(now, INACTIVITY_TIMEOUT),
            ack_flush: None,
            ping: None,
            caps_events: Vec::new(),
            event_queue_id: 1,
            xfer_files: BTreeMap::new(),
            xfer_sends: BTreeMap::new(),
            xfer_receives: BTreeMap::new(),
            next_xfer_id: XferId(1),
            secure_session_id: None,
            transfer_serves: BTreeMap::new(),
            agent_presence: AgentPresence::Child,
            sit: SimSitState::NotSitting,
            script_questions: BTreeMap::new(),
            script_grants: BTreeMap::new(),
            chat_sessions: BTreeMap::new(),
            offline_messages: Vec::new(),
            display_names: BTreeMap::new(),
            agent_preferences: default_agent_preferences(),
            pending_report_screenshot: None,
            pending_caps_uploads: BTreeMap::new(),
            next_sim_serial: 0,
            region_materials: BTreeMap::new(),
            object_media: BTreeMap::new(),
            agent_inventory: SimInventoryTree::default(),
            library_inventory: SimInventoryTree::default(),
            simulator_features: SimulatorFeatures::default(),
            lsl_syntax: LslSyntax::default(),
            environments: BTreeMap::from([(-1, default_region_environment())]),
            object_costs: BTreeMap::new(),
            object_physics: BTreeMap::new(),
            selection_costs: BTreeMap::new(),
            region_id: Uuid::nil(),
            parcels: Vec::new(),
            attachment_resources: AttachmentResourcesReport::default(),
            land_resource_summary: ResourceSummary::default(),
            land_resource_details: Vec::new(),
            experiences: SimExperiences::default(),
            voice: SimVoice::default(),
            events: VecDeque::new(),
        }
    }

    /// Whether this circuit currently hosts a child agent or the root agent.
    /// Meaningful once the circuit is open ([`Self::client_addr`] is set);
    /// before that it reports `Child`.
    #[must_use]
    pub const fn agent_presence(&self) -> AgentPresence {
        self.agent_presence
    }

    /// Whether the avatar is present in this region (the circuit was promoted
    /// to root by a `CompleteAgentMovement`).
    #[must_use]
    pub const fn is_root_agent(&self) -> bool {
        matches!(self.agent_presence, AgentPresence::Root)
    }

    /// The object the agent is seated on, once the sit handshake completed
    /// with the client's `AgentSit` (the mirror of the client's
    /// [`Session::seat`](crate::Session::seat)). `None` while not sitting or
    /// while an [`SimSession::send_avatar_sit_response`] is still awaiting
    /// its `AgentSit`.
    #[must_use]
    pub const fn seated_on(&self) -> Option<ObjectKey> {
        match self.sit {
            SimSitState::Seated { on } => Some(on),
            SimSitState::NotSitting | SimSitState::ResponseSent { .. } => None,
        }
    }

    /// The permission set a [`SimSession::send_script_question`] asked for the
    /// script `item_id` in object `task_id`, while its answer is still
    /// outstanding (`None` once answered or never asked).
    #[must_use]
    pub fn script_question(
        &self,
        task_id: ObjectKey,
        item_id: InventoryKey,
    ) -> Option<ScriptPermissions> {
        self.script_questions.get(&(task_id, item_id)).copied()
    }

    /// The recorded answer to a `ScriptQuestion` for the script `item_id` in
    /// object `task_id`: `None` when never answered, `Some` of an empty set
    /// for an explicit deny — the server twin of the client's tri-state
    /// [`Session::script_permission_status`](crate::Session::script_permission_status).
    #[must_use]
    pub fn script_grant(
        &self,
        task_id: ObjectKey,
        item_id: InventoryKey,
    ) -> Option<ScriptPermissions> {
        self.script_grants.get(&(task_id, item_id)).copied()
    }

    /// The live chat session keyed by `session_id` (a group session's id is
    /// the group id), or `None` if this session does not know it.
    #[must_use]
    pub fn chat_session(&self, session_id: ImSessionId) -> Option<&SimChatSession> {
        self.chat_sessions.get(&session_id)
    }

    /// Stores an instant message that arrived while the agent was offline, to
    /// be served (once) by the `ReadOfflineMsgs` capability — the driver API
    /// feeding [`SimSession::take_offline_messages`].
    pub fn store_offline_message(&mut self, message: InstantMessage) {
        self.offline_messages.push(message);
    }

    /// Drains the stored offline messages, oldest-first — OpenSim's
    /// delete-on-fetch deliver-once semantics: the `ReadOfflineMsgs` handler
    /// serves the batch and a repeated fetch yields an empty list.
    pub fn take_offline_messages(&mut self) -> Vec<InstantMessage> {
        std::mem::take(&mut self.offline_messages)
    }

    /// Registers (or replaces) an avatar's display-name record in the store
    /// the `GetDisplayNames` capability serves from, keyed by the record's
    /// [`id`](DisplayName::id) — the driver API for the people service.
    pub fn set_display_name(&mut self, name: DisplayName) {
        self.display_names.insert(name.id, name);
    }

    /// The stored display-name record for `agent`, if the people-service store
    /// knows one ([`SimSession::set_display_name`]).
    #[must_use]
    pub fn display_name(&self, agent: AgentKey) -> Option<&DisplayName> {
        self.display_names.get(&agent)
    }

    /// The agent's server-stored preferences — the full set the
    /// `AgentPreferences` capability echoes on every request. Starts at
    /// OpenSim's defaults (`IAgentPreferencesService.cs`): hover height `0.0`,
    /// zero permission masks, access ceiling `"M"`, language `"en-us"` and
    /// public, god level `0`.
    #[must_use]
    pub const fn agent_preferences(&self) -> &AgentPreferences {
        &self.agent_preferences
    }

    /// Merges an `AgentPreferences` capability update into the stored set:
    /// each `Some` field of `update` overwrites the stored value. `god_level`
    /// is ignored — it is reply-only (the grid reports the agent's
    /// administrative level; clients cannot set it).
    pub(crate) fn merge_agent_preferences(&mut self, update: &AgentPreferences) {
        if let Some(hover_height) = update.hover_height {
            self.agent_preferences.hover_height = Some(hover_height);
        }
        if let Some(masks) = update.default_object_perm_masks {
            self.agent_preferences.default_object_perm_masks = Some(masks);
        }
        if let Some(max_access_pref) = &update.max_access_pref {
            self.agent_preferences.max_access_pref = Some(max_access_pref.clone());
        }
        if let Some(language) = &update.language {
            self.agent_preferences.language = Some(language.clone());
        }
        if let Some(language_is_public) = update.language_is_public {
            self.agent_preferences.language_is_public = Some(language_is_public);
        }
    }

    /// Accepts a chat-session invitation on behalf of this circuit's agent
    /// (the `ChatSessionRequest` `"accept invitation"` server side): adds the
    /// agent (when the circuit knows one) to the session's roster and returns
    /// the roster snapshot for the accept reply, or `None` for a session this
    /// simulator does not know. Relaying the join to the *other* participants'
    /// sessions stays the driver's job
    /// ([`SimSession::send_session_participant`] /
    /// [`SimSession::enqueue_chatterbox_agent_list_updates`]).
    pub(crate) fn chat_session_accept(&mut self, session_id: ImSessionId) -> Option<Vec<AgentKey>> {
        let chat_session = self.chat_sessions.get_mut(&session_id)?;
        if let Some(agent) = self.agent_id {
            chat_session.participants.insert(agent);
        }
        Some(chat_session.participants.iter().copied().collect())
    }

    /// Declines a chat-session invitation on behalf of this circuit's agent
    /// (the `ChatSessionRequest` `"decline invitation"` server side): removes
    /// the agent from the session's roster, dropping the session when the
    /// roster empties (the same rule as
    /// [`SimSession::send_session_participant`]). A decline for an unknown
    /// session is a no-op.
    pub(crate) fn chat_session_decline(&mut self, session_id: ImSessionId) {
        let Some(agent) = self.agent_id else {
            return;
        };
        if let Some(chat_session) = self.chat_sessions.get_mut(&session_id) {
            chat_session.participants.remove(&agent);
            if chat_session.participants.is_empty() {
                self.chat_sessions.remove(&session_id);
            }
        }
    }

    /// Appends a message to a known chat session's server-side history without
    /// any wire traffic — the driver API for the relay topology (the sending
    /// agent's region logs via [`SimSession::send_session_message`], but each
    /// *other* participant's session must record the history it will serve to
    /// a `ChatSessionRequest` `"fetch history"`). Keeps the history cap. A
    /// record for an unknown session is dropped.
    pub fn record_session_history(
        &mut self,
        session_id: ImSessionId,
        message: ServerHistoryMessage,
    ) {
        if let Some(chat_session) = self.chat_sessions.get_mut(&session_id) {
            chat_session.log(message);
        }
    }

    /// Routes an abuse report received over the modern `SendUserReport`
    /// capability to the driver as [`ServerEvent::AbuseReportReceived`] — the
    /// same event the legacy UDP `UserReport` path pushes.
    pub(crate) fn push_abuse_report(&mut self, report: AbuseReport) {
        self.events
            .push_back(ServerEvent::AbuseReportReceived(Box::new(report)));
    }

    /// Parks the report from the first `SendUserReportWithScreenshot` step
    /// until the second step delivers the screenshot bytes; a re-POST
    /// replaces the pending report.
    pub(crate) fn set_pending_screenshot_report(&mut self, report: AbuseReport) {
        self.pending_report_screenshot = Some(Box::new(report));
    }

    /// Takes the parked screenshot-bearing report, if a first
    /// `SendUserReportWithScreenshot` step stored one.
    pub(crate) const fn take_pending_screenshot_report(&mut self) -> Option<Box<AbuseReport>> {
        self.pending_report_screenshot.take()
    }

    /// Routes a completed two-step `SendUserReportWithScreenshot` upload to
    /// the driver as [`ServerEvent::AbuseReportWithScreenshotReceived`].
    pub(crate) fn push_abuse_report_with_screenshot(
        &mut self,
        report: Box<AbuseReport>,
        screenshot: Vec<u8>,
    ) {
        self.events
            .push_back(ServerEvent::AbuseReportWithScreenshotReceived { report, screenshot });
    }

    /// Bumps and returns the monotonic sim serial that mints upload asset ids
    /// and media versions ([`next_sim_serial`](Self::next_sim_serial)).
    const fn next_serial(&mut self) -> u128 {
        self.next_sim_serial = self.next_sim_serial.wrapping_add(1);
        self.next_sim_serial
    }

    /// Parks the parsed step-1 metadata of a two-stage CAPS upload under its
    /// capability name until the raw-bytes step completes it. A re-POST of
    /// step 1 replaces the parked metadata (the same rule as the screenshot
    /// uploader).
    pub(crate) fn park_caps_upload(&mut self, cap: &'static str, metadata: CapsUploadMetadata) {
        self.pending_caps_uploads.insert(cap, metadata);
    }

    /// Takes the parked step-1 metadata for `cap`, if a first step stored one.
    pub(crate) fn take_caps_upload(&mut self, cap: &'static str) -> Option<CapsUploadMetadata> {
        self.pending_caps_uploads.remove(cap)
    }

    /// Completes a two-stage CAPS upload: mints the stored asset id (and, for
    /// the inventory-creating caps, an inventory item id), routes the upload to
    /// the driver as [`ServerEvent::CapsAssetUploaded`], and returns the minted
    /// ids for the `{ state: "complete", new_asset, new_inventory_item? }`
    /// reply. The ids come from the deterministic sim serial
    /// ([`next_sim_serial`](Self::next_sim_serial)).
    pub(crate) fn complete_caps_upload(
        &mut self,
        metadata: CapsUploadMetadata,
        data: Vec<u8>,
    ) -> (AssetKey, Option<InventoryKey>) {
        let new_asset = AssetKey::from(Uuid::from_u128(self.next_serial()));
        let new_inventory_item = metadata
            .creates_inventory_item()
            .then(|| InventoryKey::from(Uuid::from_u128(self.next_serial())));
        self.events.push_back(ServerEvent::CapsAssetUploaded {
            metadata: Box::new(metadata),
            new_asset,
            new_inventory_item,
            data,
        });
        (new_asset, new_inventory_item)
    }

    /// Registers (or replaces) a material in the store the `RenderMaterials`
    /// query serves from, keyed by material id — the driver API for the
    /// materials service.
    pub fn set_region_material(&mut self, material_id: Uuid, material: LegacyMaterial) {
        self.region_materials.insert(material_id, material);
    }

    /// The materials the `RenderMaterials` query asks for: the subset of the
    /// store whose ids are in `ids`, or — when `ids` is empty (the "fetch all
    /// region materials" form) — every stored material. Unknown ids are
    /// omitted.
    pub(crate) fn region_materials(&self, ids: &[Uuid]) -> Vec<RenderMaterialEntry> {
        let entry = |(id, material): (&Uuid, &LegacyMaterial)| RenderMaterialEntry {
            material_id: *id,
            material: material.clone(),
        };
        if ids.is_empty() {
            return self.region_materials.iter().map(entry).collect();
        }
        ids.iter()
            .filter_map(|id| {
                self.region_materials
                    .get_key_value(id)
                    .map(|(id, material)| entry((id, material)))
            })
            .collect()
    }

    /// Registers (or replaces) an object's per-face media in the store the
    /// `ObjectMedia` GET serves from — the driver API for media-on-a-prim.
    pub fn set_object_media(&mut self, object_id: ObjectKey, state: ObjectMediaState) {
        self.object_media.insert(object_id, state);
    }

    /// The stored media for `object_id`, if the `ObjectMedia` store knows it
    /// ([`SimSession::set_object_media`]).
    pub(crate) fn object_media(&self, object_id: ObjectKey) -> Option<&ObjectMediaState> {
        self.object_media.get(&object_id)
    }

    /// Records an `ObjectMedia` UPDATE: stores the new per-face media under a
    /// freshly minted media version and routes it to the driver as
    /// [`ServerEvent::ObjectMediaSet`]. Returns the new version string (for the
    /// handler's ack, though the reference just acks with an undefined body).
    pub(crate) fn set_object_media_update(
        &mut self,
        object_id: ObjectKey,
        faces: Vec<Option<MediaEntry>>,
    ) {
        let version = self.mint_media_version(object_id);
        self.object_media.insert(
            object_id,
            ObjectMediaState {
                version,
                faces: faces.clone(),
            },
        );
        self.events
            .push_back(ServerEvent::ObjectMediaSet { object_id, faces });
    }

    /// Records an `ObjectMediaNavigate`: advances the object's media version
    /// (creating an empty media record if the object is unknown) and routes the
    /// navigation to the driver as [`ServerEvent::ObjectMediaNavigated`].
    pub(crate) fn navigate_object_media(&mut self, object_id: ObjectKey, face: u8, url: String) {
        let version = self.mint_media_version(object_id);
        self.object_media
            .entry(object_id)
            .or_insert_with(|| ObjectMediaState {
                version: String::new(),
                faces: Vec::new(),
            })
            .version = version;
        self.events.push_back(ServerEvent::ObjectMediaNavigated {
            object_id,
            face,
            url,
        });
    }

    /// Mints the next media version string (`x-mv:<serial>/<object-uuid>`) — the
    /// `MediaURL`-style token the simulator advances on every media change.
    fn mint_media_version(&mut self, object_id: ObjectKey) -> String {
        let serial = self.next_serial();
        format!("x-mv:{serial:010}/{}", object_id.uuid())
    }

    /// Replaces the feature document the `SimulatorFeatures` capability
    /// serves. `lsl_syntax_id` is subsequently overwritten by
    /// [`SimSession::set_lsl_syntax`], which owns the consistency invariant
    /// between the advertised id and the served `LSLSyntax` document.
    pub fn set_simulator_features(&mut self, features: SimulatorFeatures) {
        self.simulator_features = features;
    }

    /// The feature document the `SimulatorFeatures` capability serves.
    #[must_use]
    pub const fn simulator_features(&self) -> &SimulatorFeatures {
        &self.simulator_features
    }

    /// Replaces the LSL syntax document the `LSLSyntax` capability serves and
    /// advertises its id in the feature document's `lsl_syntax_id`, keeping
    /// the two capabilities consistent (the client re-fetches `LSLSyntax`
    /// keyed on that id).
    pub fn set_lsl_syntax(&mut self, syntax_id: Uuid, syntax: LslSyntax) {
        self.simulator_features.lsl_syntax_id = Some(syntax_id);
        self.lsl_syntax = syntax;
    }

    /// The LSL syntax document the `LSLSyntax` capability serves.
    pub(crate) const fn lsl_syntax(&self) -> &LslSyntax {
        &self.lsl_syntax
    }

    /// Stores (or replaces) the environment served for
    /// `environment.parcel_id` (`-1` = the region entry) — the driver API the
    /// `ExtEnvironment` GET serves from.
    pub fn set_environment(&mut self, environment: EnvironmentSettings) {
        self.environments.insert(environment.parcel_id, environment);
    }

    /// The environment served for `parcel_id`: its own entry when the driver
    /// stored one, else the region entry (parcels inherit the region
    /// environment). The region entry is seeded at construction and never
    /// removed; the final fallback only fires if a driver somehow displaced
    /// it, and serves a fresh default rather than panicking.
    pub(crate) fn environment(&self, parcel_id: i32) -> EnvironmentSettings {
        self.environments
            .get(&parcel_id)
            .or_else(|| self.environments.get(&-1))
            .cloned()
            .unwrap_or_else(default_region_environment)
    }

    /// Applies an `ExtEnvironment` PUT to the store: merges the update's
    /// `Some` fields over the effective settings for `parcel_id` (a wholesale
    /// day-cycle replacement — `track_no` scopes nothing here and is only
    /// forwarded to the driver), bumps `env_version`, stores the result under
    /// `parcel_id`, surfaces [`ServerEvent::EnvironmentUpdated`], and returns
    /// the stored value for the handler to serialize.
    pub(crate) fn apply_environment_update(
        &mut self,
        parcel_id: i32,
        track_no: Option<i32>,
        update: EnvironmentUpdate,
    ) -> EnvironmentSettings {
        let mut environment = self.environment(parcel_id);
        environment.parcel_id = parcel_id;
        if let Some(day_length) = update.day_length {
            environment.day_length = day_length;
        }
        if let Some(day_offset) = update.day_offset {
            environment.day_offset = day_offset;
        }
        if let Some(track_altitudes) = update.track_altitudes {
            environment.track_altitudes = track_altitudes;
        }
        if let Some(day_cycle) = &update.day_cycle {
            environment.day_cycle = day_cycle.clone();
        }
        environment.flags = update.flags;
        environment.env_version = environment.env_version.saturating_add(1);
        self.environments.insert(parcel_id, environment.clone());
        self.events.push_back(ServerEvent::EnvironmentUpdated {
            parcel_id,
            track_no,
            update: Box::new(update),
        });
        environment
    }

    /// Stores (or replaces) an object's `GetObjectCost` record — the driver
    /// API the cost capability serves from.
    pub fn set_object_cost(&mut self, object_id: ObjectKey, cost: ObjectCost) {
        self.object_costs.insert(object_id, cost);
    }

    /// The stored costs for the requested objects, in id order. Unknown ids
    /// are omitted — the capability's "no such object" signal.
    pub(crate) fn object_costs(&self, ids: &[ObjectKey]) -> Vec<(ObjectKey, ObjectCost)> {
        ids.iter()
            .filter_map(|id| self.object_costs.get(id).map(|cost| (*id, cost.clone())))
            .collect()
    }

    /// Stores (or replaces) an object's `GetObjectPhysicsData` record — the
    /// driver API the physics-data capability serves from.
    pub fn set_object_physics(&mut self, object_id: ObjectKey, data: ObjectPhysicsData) {
        self.object_physics.insert(object_id, data);
    }

    /// The stored physics data for the requested objects, in id order.
    /// Unknown ids are omitted — the capability's "no such object" signal.
    pub(crate) fn object_physics(&self, ids: &[ObjectKey]) -> Vec<(ObjectKey, ObjectPhysicsData)> {
        ids.iter()
            .filter_map(|id| self.object_physics.get(id).map(|data| (*id, *data)))
            .collect()
    }

    /// Stores (or replaces) an object's `ResourceCostSelected` contribution —
    /// the driver API the selection-cost capability sums over.
    pub fn set_selection_cost(&mut self, object_id: ObjectKey, cost: SelectedResourceCost) {
        self.selection_costs.insert(object_id, cost);
    }

    /// The component-wise sum of the stored selection costs of the requested
    /// objects; unknown ids contribute zero.
    pub(crate) fn selection_cost(&self, ids: &[ObjectKey]) -> SelectedResourceCost {
        ids.iter()
            .filter_map(|id| self.selection_costs.get(id))
            .fold(SelectedResourceCost::default(), |sum, cost| {
                SelectedResourceCost {
                    physics: sum.physics + cost.physics,
                    streaming: sum.streaming + cost.streaming,
                    simulation: sum.simulation + cost.simulation,
                }
            })
    }

    /// Sets this region's id, matched against `RemoteParcelRequest` lookups
    /// (and the WebRTC estate voice channel id a scenario seeds).
    pub const fn set_region_id(&mut self, region_id: Uuid) {
        self.region_id = region_id;
    }

    /// This region's id ([`set_region_id`](Self::set_region_id)); nil until
    /// set.
    #[must_use]
    pub const fn region_id(&self) -> Uuid {
        self.region_id
    }

    /// Adds a parcel-cover rectangle to the `RemoteParcelRequest` lookup
    /// store. Rectangles are checked in insertion order; the first containing
    /// one wins.
    pub fn add_parcel(&mut self, parcel: SimParcel) {
        self.parcels.push(parcel);
    }

    /// Resolves a `RemoteParcelRequest` against the parcel-cover store: the
    /// request targets this region iff its non-nil region id matches
    /// [`SimSession::set_region_id`]'s or its non-zero region handle matches
    /// the session's; a hit is the first stored rectangle containing the
    /// requested location. A miss answers `None` (the handler replies with an
    /// empty map, the "could not resolve" signal).
    pub(crate) fn resolve_remote_parcel(&self, request: &RemoteParcelRequest) -> Option<ParcelKey> {
        let by_id = !request.region_id.is_nil() && request.region_id == self.region_id;
        let by_handle =
            request.region_handle.get() != 0 && request.region_handle == self.region_handle;
        if !by_id && !by_handle {
            return None;
        }
        let x = request.location.x();
        let y = request.location.y();
        self.parcels
            .iter()
            .find(|parcel| {
                parcel.west <= x && x < parcel.east && parcel.south <= y && y < parcel.north
            })
            .map(|parcel| parcel.parcel_id)
    }

    /// Replaces the agent's scripted-attachment report the
    /// `AttachmentResources` capability serves.
    pub fn set_attachment_resources(&mut self, report: AttachmentResourcesReport) {
        self.attachment_resources = report;
    }

    /// The stored `AttachmentResources` report.
    pub(crate) const fn attachment_resources(&self) -> &AttachmentResourcesReport {
        &self.attachment_resources
    }

    /// Replaces the script-resource summary the `LandResources` follow-up
    /// summary GET serves.
    pub fn set_land_resource_summary(&mut self, summary: ResourceSummary) {
        self.land_resource_summary = summary;
    }

    /// The stored `LandResources` summary report.
    pub(crate) const fn land_resource_summary(&self) -> &ResourceSummary {
        &self.land_resource_summary
    }

    /// Replaces the per-parcel script-resource details the `LandResources`
    /// follow-up details GET serves.
    pub fn set_land_resource_details(&mut self, parcels: Vec<ParcelScriptResources>) {
        self.land_resource_details = parcels;
    }

    /// The stored `LandResources` per-parcel detail reports.
    pub(crate) fn land_resource_details(&self) -> &[ParcelScriptResources] {
        &self.land_resource_details
    }

    /// The experience serving store — read access for the experience
    /// capability handlers and for tests asserting post-mutation state.
    #[must_use]
    pub const fn experiences(&self) -> &SimExperiences {
        &self.experiences
    }

    /// Mutable access to the experience serving store — the driver/test
    /// population API ([`SimExperiences::insert`] and the `set_*` list
    /// setters).
    pub const fn experiences_mut(&mut self) -> &mut SimExperiences {
        &mut self.experiences
    }

    /// The voice signalling stub — read access for the voice capability
    /// handlers and for tests asserting the live connections.
    #[must_use]
    pub const fn voice(&self) -> &SimVoice {
        &self.voice
    }

    /// Mutable access to the voice signalling stub — the driver/test
    /// population API ([`SimVoice::enable_webrtc`],
    /// [`SimVoice::set_vivox_account`], [`SimVoice::set_parcel_voice_info`],
    /// [`SimVoice::set_agent_parcel`], [`SimVoice::set_channel_credentials`]).
    pub const fn voice_mut(&mut self) -> &mut SimVoice {
        &mut self.voice
    }

    /// Serves one `ProvisionVoiceAccountRequest` from the voice stub and
    /// surfaces [`ServerEvent::VoiceProvisionRequested`] with the outcome.
    /// Returns the reply body on success or the refusal (which the cap maps
    /// to a status code).
    pub(crate) fn provision_voice(
        &mut self,
        request: VoiceProvisionRequest,
    ) -> Result<VoiceAccountInfo, VoiceProvisionRefusal> {
        let (result, outcome) = self.voice.provision(&request);
        self.events.push_back(ServerEvent::VoiceProvisionRequested {
            request: Box::new(request),
            outcome,
        });
        result
    }

    /// Records one `VoiceSignalingRequest` trickle on its connection and
    /// surfaces [`ServerEvent::VoiceSignalingReceived`]. Returns whether the
    /// `viewer_session` was a live connection.
    pub(crate) fn record_voice_signaling(
        &mut self,
        viewer_session: String,
        candidates: Vec<IceCandidate>,
        completed: bool,
    ) -> bool {
        let known = self
            .voice
            .record_signaling(&viewer_session, &candidates, completed);
        self.events.push_back(ServerEvent::VoiceSignalingReceived {
            viewer_session,
            candidates,
            completed,
            known,
        });
        known
    }

    /// Serves one `ParcelVoiceInfoRequest` from the voice stub's parcel
    /// table and surfaces [`ServerEvent::ParcelVoiceInfoRequested`].
    pub(crate) fn parcel_voice_info(&mut self) -> ParcelVoiceInfo {
        let info = self.voice.parcel_voice_info();
        self.events
            .push_back(ServerEvent::ParcelVoiceInfoRequested {
                parcel_local_id: info.parcel_local_id,
                enabled: info.channel_uri.is_some(),
            });
        info
    }

    /// Applies one `ExperiencePreferences` mutation to the serving store
    /// and surfaces [`ServerEvent::ExperiencePermissionSet`]. Returns the
    /// post-mutation `(allowed, blocked)` lists — the reply payload both
    /// the PUT and DELETE forms echo.
    pub(crate) fn set_experience_preference(
        &mut self,
        experience_id: ExperienceKey,
        permission: ExperiencePermission,
    ) -> (Vec<ExperienceKey>, Vec<ExperienceKey>) {
        self.experiences.set_preference(experience_id, permission);
        self.events.push_back(ServerEvent::ExperiencePermissionSet {
            experience_id,
            permission,
        });
        self.experiences.agent_permissions()
    }

    /// Applies one `UpdateExperience` edit to the serving store's record
    /// and surfaces [`ServerEvent::ExperienceUpdated`]. Returns the updated
    /// record for the reply, or `None` when the id is unknown (→ `404`, no
    /// event).
    pub(crate) fn apply_experience_update(
        &mut self,
        update: ExperienceUpdate,
    ) -> Option<ExperienceInfo> {
        let updated = self.experiences.apply_update(&update)?;
        self.events.push_back(ServerEvent::ExperienceUpdated {
            update: Box::new(update),
        });
        Some(updated)
    }

    /// Replaces the region's experience lists wholesale (the
    /// `RegionExperiences` POST) and surfaces
    /// [`ServerEvent::RegionExperiencesSet`]. Returns the stored triple for
    /// the reply's echo.
    pub(crate) fn apply_region_experiences(
        &mut self,
        allowed: Vec<ExperienceKey>,
        blocked: Vec<ExperienceKey>,
        trusted: Vec<ExperienceKey>,
    ) -> (Vec<ExperienceKey>, Vec<ExperienceKey>, Vec<ExperienceKey>) {
        let stored =
            self.experiences
                .apply_region_lists(allowed.clone(), blocked.clone(), trusted.clone());
        self.events.push_back(ServerEvent::RegionExperiencesSet {
            allowed,
            blocked,
            trusted,
        });
        stored
    }

    /// Routes a fire-and-forget server event to the driver. Used by the CAPS
    /// content handlers whose only side effect is surfacing a world mutation
    /// (appearance bake, notecard copy, materials set) the world authority —
    /// out of scope here — would apply.
    pub(crate) fn push_content_event(&mut self, event: ServerEvent) {
        self.events.push_back(event);
    }

    /// The agent's inventory serving tree — read access for the fetch
    /// handlers and for tests asserting post-mutation state.
    #[must_use]
    pub const fn agent_inventory(&self) -> &SimInventoryTree {
        &self.agent_inventory
    }

    /// Mutable access to the agent's inventory serving tree — the driver/test
    /// population API ([`SimInventoryTree::insert_folder`] /
    /// [`SimInventoryTree::insert_item`]).
    pub const fn agent_inventory_mut(&mut self) -> &mut SimInventoryTree {
        &mut self.agent_inventory
    }

    /// The read-only shared-Library serving tree.
    #[must_use]
    pub const fn library_inventory(&self) -> &SimInventoryTree {
        &self.library_inventory
    }

    /// Mutable access to the Library serving tree — the driver/test
    /// population API (the capabilities themselves never mutate it).
    pub const fn library_inventory_mut(&mut self) -> &mut SimInventoryTree {
        &mut self.library_inventory
    }

    /// Creates an inventory folder for an AIS3 `POST /category/<parent>`:
    /// mints the folder id from the deterministic sim serial, applies it to
    /// the agent tree (bumping the parent version), and surfaces
    /// [`ServerEvent::InventoryCategoryCreated`]. Returns the change-set and
    /// the stored folder (for the reply's `_embedded` block).
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the parent does not exist.
    pub(crate) fn ais_create_category(
        &mut self,
        parent: InventoryFolderKey,
        create: &sl_wire::AisCategoryCreate,
    ) -> Result<(sl_wire::AisUpdate, InventoryFolder), SimInventoryError> {
        let folder = InventoryFolder {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(self.next_serial())),
            parent_id: Some(parent),
            name: create.name.clone(),
            folder_type: i8::try_from(create.folder_type).unwrap_or(-1),
            version: 1,
        };
        let update = self.agent_inventory.create_category(folder.clone())?;
        self.events
            .push_back(ServerEvent::InventoryCategoryCreated {
                folder: Box::new(folder.clone()),
            });
        Ok((update, folder))
    }

    /// Creates inventory links for an AIS3 `POST /category/<parent>` carrying
    /// a `links` payload: mints the item ids, stores each link (its
    /// `asset_id` is the linked object's id, per the link convention),
    /// bumps the parent version, and surfaces
    /// [`ServerEvent::InventoryLinksCreated`]. Returns the change-set and the
    /// stored links (for the reply's `_embedded` block).
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the parent does not exist.
    pub(crate) fn ais_create_links(
        &mut self,
        parent: InventoryFolderKey,
        links: &[sl_wire::AisLinkCreate],
    ) -> Result<(sl_wire::AisUpdate, Vec<InventoryItem>), SimInventoryError> {
        let owner_id = self.agent_id.map_or_else(Uuid::nil, |agent| agent.uuid());
        let items: Vec<InventoryItem> = links
            .iter()
            .map(|link| InventoryItem {
                item_id: InventoryKey::from(Uuid::from_u128(self.next_serial())),
                folder_id: parent,
                name: link.name.clone(),
                description: link.description.clone(),
                asset_id: link.linked_id,
                item_type: i8::try_from(link.link_type).unwrap_or(-1),
                inv_type: i8::try_from(link.inv_type).unwrap_or(-1),
                flags: 0,
                sale_type: 0,
                sale_price: None,
                creation_date: 0,
                owner: OwnerKey::Agent(AgentKey::from(owner_id)),
                last_owner_id: Uuid::nil(),
                creator_id: AgentKey::from(owner_id),
                group: None,
                permissions: Permissions5::default(),
            })
            .collect();
        let update = self.agent_inventory.create_links(parent, items.clone())?;
        self.events.push_back(ServerEvent::InventoryLinksCreated {
            links: items.clone(),
        });
        Ok((update, items))
    }

    /// Renames an inventory folder for an AIS3 `PATCH /category/<id>`,
    /// surfacing [`ServerEvent::InventoryCategoryRenamed`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist.
    pub(crate) fn ais_rename_category(
        &mut self,
        id: InventoryFolderKey,
        name: String,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.rename_category(id, name.clone())?;
        self.events
            .push_back(ServerEvent::InventoryCategoryRenamed {
                folder_id: id,
                name,
            });
        Ok(update)
    }

    /// Moves an inventory folder for an AIS3 `PATCH /category/<id>` with
    /// `{ parent_id }`, surfacing [`ServerEvent::InventoryCategoryMoved`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist;
    /// [`SimInventoryError::InvalidParent`] on an unknown new parent or a
    /// cycle-creating move.
    pub(crate) fn ais_move_category(
        &mut self,
        id: InventoryFolderKey,
        parent: InventoryFolderKey,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.move_category(id, parent)?;
        self.events.push_back(ServerEvent::InventoryCategoryMoved {
            folder_id: id,
            parent_id: parent,
        });
        Ok(update)
    }

    /// Moves an inventory item for an AIS3 `PATCH /item/<id>` with
    /// `{ parent_id }`, surfacing [`ServerEvent::InventoryItemMoved`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the item does not exist;
    /// [`SimInventoryError::InvalidParent`] on an unknown destination folder.
    pub(crate) fn ais_move_item(
        &mut self,
        id: InventoryKey,
        parent: InventoryFolderKey,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.move_item(id, parent)?;
        self.events.push_back(ServerEvent::InventoryItemMoved {
            item_id: id,
            folder_id: parent,
        });
        Ok(update)
    }

    /// Updates an inventory item's name/description for an AIS3
    /// `PATCH /item/<id>`, surfacing [`ServerEvent::InventoryItemUpdated`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the item does not exist.
    pub(crate) fn ais_update_item(
        &mut self,
        id: InventoryKey,
        update_fields: &sl_wire::AisItemUpdate,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.update_item(
            id,
            update_fields.name.clone(),
            update_fields.description.clone(),
        )?;
        self.events.push_back(ServerEvent::InventoryItemUpdated {
            item_id: id,
            name: update_fields.name.clone(),
            description: update_fields.description.clone(),
        });
        Ok(update)
    }

    /// Deletes an inventory folder (and its subtree) for an AIS3
    /// `DELETE /category/<id>`, surfacing
    /// [`ServerEvent::InventoryCategoryRemoved`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist.
    pub(crate) fn ais_remove_category(
        &mut self,
        id: InventoryFolderKey,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.remove_category(id)?;
        self.events
            .push_back(ServerEvent::InventoryCategoryRemoved {
                folder_id: id,
                removed_folders: update.categories_removed.clone(),
                removed_items: update.category_items_removed.clone(),
            });
        Ok(update)
    }

    /// Empties an inventory folder for an AIS3
    /// `DELETE /category/<id>/children`, surfacing
    /// [`ServerEvent::InventoryCategoryPurged`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist.
    pub(crate) fn ais_purge_category(
        &mut self,
        id: InventoryFolderKey,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.purge_category(id)?;
        self.events.push_back(ServerEvent::InventoryCategoryPurged {
            folder_id: id,
            removed_folders: update.categories_removed.clone(),
            removed_items: update.category_items_removed.clone(),
        });
        Ok(update)
    }

    /// Deletes an inventory item for an AIS3 `DELETE /item/<id>`, surfacing
    /// [`ServerEvent::InventoryItemRemoved`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the item does not exist.
    pub(crate) fn ais_remove_item(
        &mut self,
        id: InventoryKey,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let update = self.agent_inventory.remove_item(id)?;
        self.events
            .push_back(ServerEvent::InventoryItemRemoved { item_id: id });
        Ok(update)
    }

    /// Creates an inventory folder for the plain `CreateInventoryCategory`
    /// capability (client-chosen folder id, unlike the AIS3 create), applying
    /// it to the agent tree and surfacing
    /// [`ServerEvent::InventoryCategoryCreated`].
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the parent does not exist.
    pub(crate) fn create_inventory_category(
        &mut self,
        request: &sl_wire::CreateInventoryCategoryRequest,
    ) -> Result<sl_wire::AisUpdate, SimInventoryError> {
        let folder = InventoryFolder {
            folder_id: request.folder_id,
            parent_id: Some(request.parent_id),
            name: request.name.clone(),
            folder_type: i8::try_from(request.folder_type).unwrap_or(-1),
            version: 1,
        };
        let update = self.agent_inventory.create_category(folder.clone())?;
        self.events
            .push_back(ServerEvent::InventoryCategoryCreated {
                folder: Box::new(folder),
            });
        Ok(update)
    }

    /// The agent id once the circuit is open.
    #[must_use]
    pub const fn agent_id(&self) -> Option<AgentKey> {
        self.agent_id
    }

    /// The session id once the circuit is open.
    #[must_use]
    pub const fn session_id(&self) -> Option<Uuid> {
        self.session_id
    }

    /// The client's UDP address once a datagram has been received.
    #[must_use]
    pub const fn client_addr(&self) -> Option<SocketAddr> {
        self.client_addr
    }

    /// Returns `true` once the session has reached its terminal state.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self.state, SimState::Closed)
    }

    /// Allocates the next outgoing sequence number.
    const fn next_sequence(&mut self) -> SequenceNumber {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_next();
        sequence
    }

    /// Encodes and queues a message to the client, tracking it for resend when
    /// reliable.
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

    /// Pushes a server message to the client with the given reliability. This is
    /// the general way the simulator sends anything the client decodes —
    /// `RegionHandshake`, `ObjectUpdate`, `LayerData`, `KillObject`, and so on —
    /// alongside the typed convenience helpers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit has been opened yet (the
    /// client address is unknown), or a wire error if the message fails to
    /// encode.
    pub fn push(
        &mut self,
        message: &AnyMessage,
        reliability: Reliability,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        self.send(message, reliability, now)?;
        Ok(())
    }

    /// Sends a `ChatFromSimulator` to the client (the inverse of the client's
    /// `ChatFromViewer`). The `from_name` and `message` strings are sent
    /// NUL-terminated, as a simulator does on the wire.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire ChatData block")]
    pub fn send_chat_from_simulator(
        &mut self,
        from_name: &str,
        source: ChatSource,
        owner_id: Uuid,
        chat_type: ChatType,
        audible: u8,
        position: Vector,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ChatFromSimulator(ChatFromSimulator {
            chat_data: ChatFromSimulatorChatDataBlock {
                from_name: with_nul(from_name),
                source_id: source.source_id(),
                owner_id,
                source_type: source.source_type_byte(),
                chat_type: chat_type.to_u8(),
                audible,
                position,
                message: with_nul(message),
            },
        });
        self.send(&message, Reliability::Unreliable, now)?;
        Ok(())
    }

    /// Sends a `MapBlockReply` reporting `regions` to the client (the inverse of
    /// the client's `MapBlockRequest`/`MapNameRequest`). `flags` is the request's
    /// map-layer flag, echoed in the agent block. The reply is sent reliably, as
    /// a map server sends it. See [`build_map_block_reply`] for how
    /// variable-sized regions are reported; the batch is capped at 255 regions.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode (e.g. more than 255 regions).
    pub fn send_map_block_reply(
        &mut self,
        flags: MapRequestFlags,
        regions: &[MapRegionInfo],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let agent_id = self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
        let message = AnyMessage::MapBlockReply(build_map_block_reply(agent_id, flags, regions));
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `MapItemReply` of the given [`MapItemType`] reporting `items` to
    /// the client (the inverse of the client's `MapItemRequest`). `flags` is the
    /// request's map-layer flag, echoed in the agent block. The reply is sent
    /// reliably; the batch is capped at 255 items.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode (e.g. more than 255 items).
    pub fn send_map_item_reply(
        &mut self,
        flags: MapRequestFlags,
        item_type: MapItemType,
        items: &[MapItem],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let agent_id = self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
        let message =
            AnyMessage::MapItemReply(build_map_item_reply(agent_id, flags, item_type, items));
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `MapLayerReply` reporting `layers` to the client (the inverse of
    /// the client's `MapLayerRequest`). `flags` is the request's map-layer flag,
    /// echoed in the agent block. The reply is sent reliably; the batch is
    /// capped at 255 layers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode (e.g. more than 255 layers).
    pub fn send_map_layer_reply(
        &mut self,
        flags: MapRequestFlags,
        layers: &[MapLayer],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let agent_id = self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
        let message = AnyMessage::MapLayerReply(build_map_layer_reply(agent_id, flags, layers));
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `RegionHandshake` greeting carrying `identity` to the client — the
    /// server-side inverse of the client's `Event::RegionInfoHandshake`. The
    /// client replies with `RegionHandshakeReply` (surfaced as
    /// [`ServerEvent::RegionHandshakeReplied`]). Sent reliably. The grid
    /// coordinates / handle are not wire fields of the handshake, so they are not
    /// part of `identity` here; the client derives them from the circuit.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_region_handshake(
        &mut self,
        identity: &RegionIdentity,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::RegionHandshake(region_handshake_message(identity));
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends `UUIDNameReply` batches resolving agent ids to legacy names — the
    /// reply to a client's `UUIDNameRequest` (surfaced as
    /// [`ServerEvent::AvatarNamesRequested`]). Large lists are split across
    /// several messages. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// a message fails to encode.
    pub fn send_avatar_names(&mut self, names: &[AvatarName], now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        for batch in names.chunks(UUID_NAMES_PER_REPLY) {
            let message = AnyMessage::UUIDNameReply(UUIDNameReply {
                uuid_name_block: batch
                    .iter()
                    .map(|name| UUIDNameReplyUUIDNameBlockBlock {
                        id: name.id.uuid(),
                        first_name: with_nul(&name.first_name),
                        last_name: with_nul(&name.last_name),
                    })
                    .collect(),
            });
            self.send(&message, Reliability::Reliable, now)?;
        }
        Ok(())
    }

    /// Sends `UUIDGroupNameReply` batches resolving group ids to names — the reply
    /// to a client's `UUIDGroupNameRequest` (surfaced as
    /// [`ServerEvent::GroupNamesRequested`]). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// a message fails to encode.
    pub fn send_group_names(&mut self, names: &[GroupName], now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        for batch in names.chunks(UUID_NAMES_PER_REPLY) {
            let message = AnyMessage::UUIDGroupNameReply(UUIDGroupNameReply {
                uuid_name_block: batch
                    .iter()
                    .map(|name| UUIDGroupNameReplyUUIDNameBlockBlock {
                        id: name.id.uuid(),
                        group_name: with_nul(&name.name),
                    })
                    .collect(),
            });
            self.send(&message, Reliability::Reliable, now)?;
        }
        Ok(())
    }

    /// Sends a `CoarseLocationUpdate` with the coarse (minimap) positions of
    /// nearby avatars. `you`/`prey` are indices into `locations` (the agent's own
    /// entry and the tracked agent, if any); out-of-range or absent indices are
    /// sent as `-1`. Sent unreliably (it is refreshed periodically).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_coarse_location_update(
        &mut self,
        locations: &[CoarseLocation],
        you: Option<usize>,
        prey: Option<usize>,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::CoarseLocationUpdate(CoarseLocationUpdate {
            location: locations
                .iter()
                .map(|location| CoarseLocationUpdateLocationBlock {
                    x: location.x,
                    y: location.y,
                    z: u8::try_from(location.z / 4).unwrap_or(u8::MAX),
                })
                .collect(),
            index: CoarseLocationUpdateIndexBlock {
                you: from_index(you),
                prey: from_index(prey),
            },
            agent_data: locations
                .iter()
                .map(|location| CoarseLocationUpdateAgentDataBlock {
                    agent_id: location.agent_id.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Unreliable, now)?;
        Ok(())
    }

    /// Sends a `ViewerEffect` relaying `effects` to the client (look-at /
    /// point-at gaze hints, beams, …) on behalf of `source_agent`. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_viewer_effect(
        &mut self,
        source_agent: AgentKey,
        effects: &[ViewerEffect],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ViewerEffect(ViewerEffectMessage {
            agent_data: ViewerEffectAgentDataBlock {
                agent_id: source_agent.uuid(),
                session_id: Uuid::nil(),
            },
            effect: effects
                .iter()
                .map(|effect| ViewerEffectEffectBlock {
                    id: effect.id,
                    agent_id: effect.agent_id.uuid(),
                    r#type: effect.effect_type.to_code(),
                    duration: effect.duration,
                    color: effect.color,
                    type_data: effect.data.to_wire(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ScriptControlChange` telling the client a scripted object took or
    /// released some of the agent's movement controls (after the agent granted
    /// the script [`ScriptPermissions::TAKE_CONTROLS`](crate::ScriptPermissions::TAKE_CONTROLS)).
    /// Surfaces on the client as [`Event::ScriptControlChange`](crate::Event::ScriptControlChange).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_script_control_change(
        &mut self,
        controls: &[ScriptControl],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ScriptControlChange(ScriptControlChange {
            data: controls
                .iter()
                .map(|control| ScriptControlChangeDataBlock {
                    take_controls: control.action.takes_controls(),
                    controls: control.controls.bits(),
                    pass_to_agent: control.pass_to_agent,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ScriptQuestion` asking the agent to grant
    /// `question.permissions` to the script `question.item_id` in object
    /// `question.task_id` (`llRequestPermissions`). Surfaces on the client as
    /// [`Event::ScriptPermissionRequest`](crate::Event::ScriptPermissionRequest);
    /// the client answers with `ScriptAnswerYes`
    /// ([`ServerEvent::ScriptPermissionAnswer`]). The asked set is recorded
    /// as outstanding ([`SimSession::script_question`]) until the answer
    /// arrives. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_script_question(
        &mut self,
        question: &ScriptPermissionRequest,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ScriptQuestion(ScriptQuestion {
            data: ScriptQuestionDataBlock {
                task_id: question.task_id.uuid(),
                item_id: question.item_id.uuid(),
                object_name: with_nul(&question.object_name),
                object_owner: with_nul(&question.object_owner),
                questions: question.permissions.0,
            },
            experience: ScriptQuestionExperienceBlock {
                experience_id: question
                    .experience_id
                    .map_or_else(Uuid::nil, |experience| experience.uuid()),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        self.script_questions
            .insert((question.task_id, question.item_id), question.permissions);
        Ok(())
    }

    /// Sends a `SetFollowCamProperties` telling the client a scripted object set
    /// follow-camera parameters (`llSetCameraParams`). Surfaces on the client as
    /// [`Event::SetFollowCamProperties`](crate::Event::SetFollowCamProperties).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_set_follow_cam_properties(
        &mut self,
        object_id: ObjectKey,
        properties: &[FollowCamPropertyValue],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::SetFollowCamProperties(SetFollowCamProperties {
            object_data: SetFollowCamPropertiesObjectDataBlock {
                object_id: object_id.uuid(),
            },
            camera_property: properties
                .iter()
                .map(|property| SetFollowCamPropertiesCameraPropertyBlock {
                    r#type: property.property.to_i32(),
                    value: property.value,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ClearFollowCamProperties` telling the client a scripted object
    /// released control of the agent's camera (`llClearCameraParams`). Surfaces
    /// on the client as
    /// [`Event::ClearFollowCamProperties`](crate::Event::ClearFollowCamProperties).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_clear_follow_cam_properties(
        &mut self,
        object_id: ObjectKey,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ClearFollowCamProperties(ClearFollowCamProperties {
            object_data: ClearFollowCamPropertiesObjectDataBlock {
                object_id: object_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a general `AlertMessage` notification to the client: a plain
    /// (already-localized) `message` string, optionally accompanied by structured
    /// localizable `alert_info` keys and the `agents` the alert is directed at.
    /// Surfaces on the client as [`Event::AlertMessage`](crate::Event::AlertMessage).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_alert_message(
        &mut self,
        message: &str,
        alert_info: &[AlertInfo],
        agents: &[Uuid],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::AlertMessage(AlertMessage {
            alert_data: AlertMessageAlertDataBlock {
                message: message.as_bytes().to_vec(),
            },
            alert_info: alert_info
                .iter()
                .map(|info| AlertMessageAlertInfoBlock {
                    message: info.message.as_bytes().to_vec(),
                    extra_params: info.extra_params.as_bytes().to_vec(),
                })
                .collect(),
            agent_info: agents
                .iter()
                .map(|&agent_id| AlertMessageAgentInfoBlock { agent_id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `AgentAlertMessage` notification directed at a specific agent: a
    /// `message` string and a `modal` flag saying whether the viewer should block
    /// on a dialog. Surfaces on the client as
    /// [`Event::AgentAlertMessage`](crate::Event::AgentAlertMessage). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_agent_alert_message(
        &mut self,
        agent_id: AgentKey,
        modal: bool,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::AgentAlertMessage(AgentAlertMessage {
            agent_data: AgentAlertMessageAgentDataBlock {
                agent_id: agent_id.uuid(),
            },
            alert_data: AgentAlertMessageAlertDataBlock {
                modal,
                message: message.as_bytes().to_vec(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `MeanCollisionAlert` reporting one or more "mean collisions" (the
    /// data behind the viewer's "Bumps, Pushes & Hits" panel). Surfaces on the
    /// client as [`Event::MeanCollisionAlert`](crate::Event::MeanCollisionAlert).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_mean_collision_alert(
        &mut self,
        collisions: &[MeanCollision],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::MeanCollisionAlert(MeanCollisionAlert {
            mean_collision: collisions
                .iter()
                .map(|collision| MeanCollisionAlertMeanCollisionBlock {
                    victim: collision.victim,
                    perp: collision.perp,
                    time: collision.time,
                    mag: collision.magnitude,
                    r#type: collision.collision_type.to_u8(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `LandStatReply` carrying the region's (or a parcel's) top scripts
    /// or top colliders, in reply to a client `LandStatRequest`. Surfaces on the
    /// client as [`Event::LandStatReply`](crate::Event::LandStatReply).
    /// `total_object_count` is the full count the report draws from (the `items`
    /// themselves may be only the top rows). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_land_stat_reply(
        &mut self,
        report_type: LandStatReportType,
        request_flags: u32,
        total_object_count: u32,
        items: &[LandStatItem],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::LandStatReply(LandStatReply {
            request_data: LandStatReplyRequestDataBlock {
                report_type: report_type.to_u32(),
                request_flags,
                total_object_count,
            },
            report_data: items
                .iter()
                .map(|item| LandStatReplyReportDataBlock {
                    task_local_id: item.task_local_id.0,
                    task_id: item.task_id.uuid(),
                    location_x: item.location.x(),
                    location_y: item.location.y(),
                    location_z: item.location.z(),
                    score: item.score,
                    task_name: with_nul(&item.task_name),
                    owner_name: with_nul(&item.owner_name),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `HealthMessage` telling the client the agent's current health
    /// (e.g. in a damage-enabled region; `100.0` is full health). Surfaces on the
    /// client as [`Event::HealthMessage`](crate::Event::HealthMessage). Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_health_message(&mut self, health: f32, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::HealthMessage(HealthMessage {
            health_data: HealthMessageHealthDataBlock { health },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `CameraConstraint` telling the client to constrain the camera to
    /// the given collision `plane` (`[nx, ny, nz, d]`). Surfaces on the client as
    /// [`Event::CameraConstraint`](crate::Event::CameraConstraint). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_camera_constraint(&mut self, plane: [f32; 4], now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::CameraConstraint(CameraConstraint {
            camera_collide_plane: CameraConstraintCameraCollidePlaneBlock { plane },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ViewerFrozenMessage` telling the client it has been frozen
    /// (`frozen` = `true`) or thawed (`frozen` = `false`) by an estate manager.
    /// Surfaces on the client as
    /// [`Event::ViewerFrozen`](crate::Event::ViewerFrozen). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_viewer_frozen(&mut self, frozen: bool, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ViewerFrozenMessage(ViewerFrozenMessage {
            frozen_data: ViewerFrozenMessageFrozenDataBlock { data: frozen },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `FindAgent` reply carrying the located global `(x, y)` positions —
    /// the answer to a client's `FindAgent` (surfaced as
    /// [`ServerEvent::FindAgent`]). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_find_agent_reply(
        &mut self,
        hunter: Uuid,
        prey: Uuid,
        locations: &[(f64, f64)],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::FindAgent(FindAgent {
            agent_block: FindAgentAgentBlockBlock {
                hunter,
                prey,
                space_ip: [0, 0, 0, 0],
            },
            location_block: locations
                .iter()
                .map(|&(global_x, global_y)| FindAgentLocationBlockBlock { global_x, global_y })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DirPeopleReply`: the people results of a client's `DirFindQuery`
    /// (surfaced as [`ServerEvent::DirFindQuery`]), echoing its `query_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_dir_people_reply(
        &mut self,
        query_id: Uuid,
        results: &[DirPeopleResult],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DirPeopleReply(DirPeopleReply {
            agent_data: DirPeopleReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            query_data: DirPeopleReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirPeopleReplyQueryRepliesBlock {
                    agent_id: result.agent_id.uuid(),
                    first_name: with_nul(&result.first_name),
                    last_name: with_nul(&result.last_name),
                    group: with_nul(&result.group),
                    online: result.online,
                    reputation: result.reputation,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DirGroupsReply`: the group results of a client's `DirFindQuery`
    /// (surfaced as [`ServerEvent::DirFindQuery`]), echoing its `query_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_dir_groups_reply(
        &mut self,
        query_id: Uuid,
        results: &[DirGroupResult],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DirGroupsReply(DirGroupsReply {
            agent_data: DirGroupsReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            query_data: DirGroupsReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirGroupsReplyQueryRepliesBlock {
                    group_id: result.group_id.uuid(),
                    group_name: with_nul(&result.group_name),
                    members: result.members,
                    search_order: result.search_order,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DirEventsReply`: the event results of a client's `DirFindQuery`
    /// (surfaced as [`ServerEvent::DirFindQuery`]), echoing its `query_id`.
    /// `status` is the `STATUS_SEARCH_EVENTS_*` flags (`0` on success).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_dir_events_reply(
        &mut self,
        query_id: Uuid,
        results: &[DirEventResult],
        status: u32,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DirEventsReply(DirEventsReply {
            agent_data: DirEventsReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            query_data: DirEventsReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirEventsReplyQueryRepliesBlock {
                    owner_id: result.owner_id,
                    name: with_nul(&result.name),
                    event_id: result.event_id.get(),
                    date: with_nul(&result.date),
                    unix_time: result.unix_time,
                    event_flags: result.event_flags,
                })
                .collect(),
            status_data: vec![DirEventsReplyStatusDataBlock { status }],
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DirClassifiedReply`: the results of a client's
    /// `DirClassifiedQuery` (surfaced as [`ServerEvent::DirClassifiedQuery`]),
    /// echoing its `query_id`. `status` is the `STATUS_SEARCH_CLASSIFIEDS_*`
    /// flags (`0` on success).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_dir_classified_reply(
        &mut self,
        query_id: Uuid,
        results: &[DirClassifiedResult],
        status: u32,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DirClassifiedReply(DirClassifiedReply {
            agent_data: DirClassifiedReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            query_data: DirClassifiedReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| {
                    Ok(DirClassifiedReplyQueryRepliesBlock {
                        classified_id: result.classified_id.uuid(),
                        name: with_nul(&result.name),
                        classified_flags: result.classified_flags,
                        creation_date: result.creation_date,
                        expiration_date: result.expiration_date,
                        price_for_listing: crate::types::linden_to_wire(
                            "PriceForListing",
                            &result.price_for_listing,
                        )?,
                    })
                })
                .collect::<Result<_, sl_wire::WireError>>()?,
            status_data: vec![DirClassifiedReplyStatusDataBlock { status }],
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DirPlacesReply`: the results of a client's `DirPlacesQuery`
    /// (surfaced as [`ServerEvent::DirPlacesQuery`]), echoing its `query_id`.
    /// `status` is the `STATUS_SEARCH_PLACES_*` flags (`0` on success).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_dir_places_reply(
        &mut self,
        query_id: Uuid,
        results: &[DirPlaceResult],
        status: u32,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DirPlacesReply(DirPlacesReply {
            agent_data: DirPlacesReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            query_data: vec![DirPlacesReplyQueryDataBlock { query_id }],
            query_replies: results
                .iter()
                .map(|result| DirPlacesReplyQueryRepliesBlock {
                    parcel_id: result.parcel_id.uuid(),
                    name: with_nul(&result.name),
                    for_sale: result.for_sale,
                    auction: result.auction,
                    dwell: result.dwell,
                })
                .collect(),
            status_data: vec![DirPlacesReplyStatusDataBlock { status }],
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DirLandReply`: the results of a client's `DirLandQuery`
    /// (surfaced as [`ServerEvent::DirLandQuery`]), echoing its `query_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_dir_land_reply(
        &mut self,
        query_id: Uuid,
        results: &[DirLandResult],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DirLandReply(DirLandReply {
            agent_data: DirLandReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            query_data: DirLandReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| {
                    Ok(DirLandReplyQueryRepliesBlock {
                        parcel_id: result.parcel_id.uuid(),
                        name: with_nul(&result.name),
                        auction: result.auction,
                        for_sale: result.for_sale,
                        sale_price: crate::types::linden_price_to_wire(
                            "SalePrice",
                            result.sale_price.as_ref(),
                        )?,
                        actual_area: crate::types::land_area_to_wire(
                            "ActualArea",
                            &result.actual_area,
                        )?,
                    })
                })
                .collect::<Result<_, sl_wire::WireError>>()?,
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `AvatarPickerReply`: the results of a client's
    /// `AvatarPickerRequest` (surfaced as [`ServerEvent::AvatarPickerRequest`]),
    /// echoing its `query_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_avatar_picker_reply(
        &mut self,
        query_id: Uuid,
        results: &[AvatarPickerResult],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::AvatarPickerReply(AvatarPickerReply {
            agent_data: AvatarPickerReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                query_id,
            },
            data: results
                .iter()
                .map(|result| AvatarPickerReplyDataBlock {
                    avatar_id: result.avatar_id.uuid(),
                    first_name: with_nul(&result.first_name),
                    last_name: with_nul(&result.last_name),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `PlacesReply`: the land holdings answering a client's `PlacesQuery`
    /// (surfaced as [`ServerEvent::PlacesQuery`]), echoing its `query_id` and
    /// `transaction_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_places_reply(
        &mut self,
        query_id: Uuid,
        transaction_id: Uuid,
        results: &[PlacesResult],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::PlacesReply(PlacesReply {
            agent_data: PlacesReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                query_id,
            },
            transaction_data: PlacesReplyTransactionDataBlock { transaction_id },
            query_data: results
                .iter()
                .map(|result| {
                    Ok(PlacesReplyQueryDataBlock {
                        owner_id: result.owner_id,
                        name: with_nul(&result.name),
                        desc: with_nul(&result.description),
                        actual_area: crate::types::land_area_to_wire(
                            "ActualArea",
                            &result.actual_area,
                        )?,
                        billable_area: crate::types::land_area_to_wire(
                            "BillableArea",
                            &result.billable_area,
                        )?,
                        flags: result.flags,
                        global_x: global_to_f32(result.global_position.x()),
                        global_y: global_to_f32(result.global_position.y()),
                        global_z: global_to_f32(result.global_position.z()),
                        sim_name: with_nul(&sl_wire::region_name_to_wire(result.sim_name.as_ref())),
                        snapshot_id: result.snapshot_id.map_or_else(Uuid::nil, |s| s.uuid()),
                        dwell: result.dwell,
                        price: crate::types::linden_to_wire("Price", &result.price)?,
                    })
                })
                .collect::<Result<_, sl_wire::WireError>>()?,
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `EventInfoReply`: the full detail of an in-world event, in
    /// response to a client's `EventInfoRequest` (surfaced as
    /// [`ServerEvent::EventInfoRequest`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_event_info_reply(&mut self, info: &EventInfo, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let global = info.global_position;
        let message = AnyMessage::EventInfoReply(EventInfoReply {
            agent_data: EventInfoReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            event_data: EventInfoReplyEventDataBlock {
                event_id: info.event_id.get(),
                creator: with_nul(&info.creator.to_string()),
                name: with_nul(&info.name),
                category: with_nul(&info.category),
                desc: with_nul(&info.description),
                date: with_nul(&info.date),
                date_utc: info.date_utc,
                duration: info.duration,
                cover: info.cover,
                amount: crate::types::linden_cover_to_wire("Amount", info.amount.as_ref())?,
                sim_name: with_nul(&sl_wire::region_name_to_wire(info.sim_name.as_ref())),
                global_pos: [global.x(), global.y(), global.z()],
                event_flags: info.flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `PayPriceReply`: an object's pay-button layout, in response to a
    /// client's `RequestPayPrice` (surfaced as [`ServerEvent::RequestPayPrice`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_pay_price_reply(
        &mut self,
        object_id: ObjectKey,
        default_pay_price: i32,
        pay_buttons: &[i32],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::PayPriceReply(PayPriceReply {
            object_data: PayPriceReplyObjectDataBlock {
                object_id: object_id.uuid(),
                default_pay_price,
            },
            button_data: pay_buttons
                .iter()
                .map(|amount| PayPriceReplyButtonDataBlock {
                    pay_button: *amount,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ScriptRunningReply`: a task script's run state, in response to a
    /// client's `GetScriptRunning` (surfaced as
    /// [`ServerEvent::RequestScriptRunning`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_script_running_reply(
        &mut self,
        object_id: ObjectKey,
        item_id: Uuid,
        running: bool,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ScriptRunningReply(ScriptRunningReply {
            script: ScriptRunningReplyScriptBlock {
                object_id: object_id.uuid(),
                item_id,
                running,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GroupAccountSummaryReply`: a group's financial summary, in
    /// response to a client's `GroupAccountSummaryRequest` (surfaced as
    /// [`ServerEvent::RequestGroupAccountSummary`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_group_account_summary_reply(
        &mut self,
        summary: &GroupAccountSummary,
        now: Instant,
    ) -> Result<(), Error> {
        use crate::types::{linden_balance_to_wire, linden_to_wire};
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GroupAccountSummaryReply(GroupAccountSummaryReply {
            agent_data: GroupAccountSummaryReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                group_id: summary.group_id.uuid(),
            },
            money_data: GroupAccountSummaryReplyMoneyDataBlock {
                request_id: summary.request_id,
                interval_days: summary.interval_days,
                current_interval: summary.current_interval,
                start_date: with_nul(&summary.start_date),
                balance: linden_balance_to_wire("Balance", &summary.balance)?,
                total_credits: linden_to_wire("TotalCredits", &summary.total_credits)?,
                total_debits: linden_to_wire("TotalDebits", &summary.total_debits)?,
                object_tax_current: linden_to_wire(
                    "ObjectTaxCurrent",
                    &summary.object_tax_current,
                )?,
                light_tax_current: linden_to_wire("LightTaxCurrent", &summary.light_tax_current)?,
                land_tax_current: linden_to_wire("LandTaxCurrent", &summary.land_tax_current)?,
                group_tax_current: linden_to_wire("GroupTaxCurrent", &summary.group_tax_current)?,
                parcel_dir_fee_current: linden_to_wire(
                    "ParcelDirFeeCurrent",
                    &summary.parcel_dir_fee_current,
                )?,
                object_tax_estimate: linden_to_wire(
                    "ObjectTaxEstimate",
                    &summary.object_tax_estimate,
                )?,
                light_tax_estimate: linden_to_wire(
                    "LightTaxEstimate",
                    &summary.light_tax_estimate,
                )?,
                land_tax_estimate: linden_to_wire("LandTaxEstimate", &summary.land_tax_estimate)?,
                group_tax_estimate: linden_to_wire(
                    "GroupTaxEstimate",
                    &summary.group_tax_estimate,
                )?,
                parcel_dir_fee_estimate: linden_to_wire(
                    "ParcelDirFeeEstimate",
                    &summary.parcel_dir_fee_estimate,
                )?,
                non_exempt_members: summary.non_exempt_members,
                last_tax_date: with_nul(&summary.last_tax_date),
                tax_date: with_nul(&summary.tax_date),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GroupAccountDetailsReply`: a group's itemised accounting detail,
    /// in response to a client's `GroupAccountDetailsRequest` (surfaced as
    /// [`ServerEvent::RequestGroupAccountDetails`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_group_account_details_reply(
        &mut self,
        details: &GroupAccountDetails,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GroupAccountDetailsReply(GroupAccountDetailsReply {
            agent_data: GroupAccountDetailsReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                group_id: details.group_id.uuid(),
            },
            money_data: GroupAccountDetailsReplyMoneyDataBlock {
                request_id: details.request_id,
                interval_days: details.interval_days,
                current_interval: details.current_interval,
                start_date: with_nul(&details.start_date),
            },
            history_data: details
                .entries
                .iter()
                .map(|entry| {
                    Ok(GroupAccountDetailsReplyHistoryDataBlock {
                        description: with_nul(&entry.description),
                        amount: crate::types::linden_balance_to_wire("Amount", &entry.amount)?,
                    })
                })
                .collect::<Result<Vec<_>, sl_wire::WireError>>()?,
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GroupAccountTransactionsReply`: a group's transaction log, in
    /// response to a client's `GroupAccountTransactionsRequest` (surfaced as
    /// [`ServerEvent::RequestGroupAccountTransactions`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_group_account_transactions_reply(
        &mut self,
        transactions: &GroupAccountTransactions,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GroupAccountTransactionsReply(GroupAccountTransactionsReply {
            agent_data: GroupAccountTransactionsReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                group_id: transactions.group_id.uuid(),
            },
            money_data: GroupAccountTransactionsReplyMoneyDataBlock {
                request_id: transactions.request_id,
                interval_days: transactions.interval_days,
                current_interval: transactions.current_interval,
                start_date: with_nul(&transactions.start_date),
            },
            history_data: transactions
                .entries
                .iter()
                .map(|entry| {
                    Ok(GroupAccountTransactionsReplyHistoryDataBlock {
                        time: with_nul(&entry.time),
                        user: with_nul(&entry.user),
                        r#type: entry.transaction_type,
                        item: with_nul(&entry.item),
                        amount: crate::types::linden_balance_to_wire("Amount", &entry.amount)?,
                    })
                })
                .collect::<Result<Vec<_>, sl_wire::WireError>>()?,
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GroupActiveProposalItemReply`: a group's active proposals, in
    /// response to a client's `GroupActiveProposalsRequest` (surfaced as
    /// [`ServerEvent::RequestGroupActiveProposals`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_group_active_proposals_reply(
        &mut self,
        group_id: GroupKey,
        transaction_id: Uuid,
        total_num_items: u32,
        proposals: &[GroupActiveProposalItem],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GroupActiveProposalItemReply(GroupActiveProposalItemReply {
            agent_data: GroupActiveProposalItemReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                group_id: group_id.uuid(),
            },
            transaction_data: GroupActiveProposalItemReplyTransactionDataBlock {
                transaction_id,
                total_num_items,
            },
            proposal_data: proposals
                .iter()
                .map(|item| GroupActiveProposalItemReplyProposalDataBlock {
                    vote_id: item.vote_id.uuid(),
                    vote_initiator: item.vote_initiator.uuid(),
                    terse_date_id: with_nul(&item.terse_date_id),
                    start_date_time: with_nul(&item.start_date_time),
                    end_date_time: with_nul(&item.end_date_time),
                    already_voted: item.already_voted,
                    vote_cast: with_nul(&item.vote_cast),
                    majority: item.majority,
                    quorum: item.quorum,
                    proposal_text: with_nul(&item.proposal_text),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GroupVoteHistoryItemReply`: one finished proposal from a group's
    /// vote history, in response to a client's `GroupVoteHistoryRequest` (surfaced
    /// as [`ServerEvent::RequestGroupVoteHistory`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_group_vote_history_reply(
        &mut self,
        group_id: GroupKey,
        transaction_id: Uuid,
        total_num_items: u32,
        item: &GroupVoteHistoryItem,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GroupVoteHistoryItemReply(GroupVoteHistoryItemReply {
            agent_data: GroupVoteHistoryItemReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                group_id: group_id.uuid(),
            },
            transaction_data: GroupVoteHistoryItemReplyTransactionDataBlock {
                transaction_id,
                total_num_items,
            },
            history_item_data: GroupVoteHistoryItemReplyHistoryItemDataBlock {
                vote_id: item.vote_id.uuid(),
                terse_date_id: with_nul(&item.terse_date_id),
                start_date_time: with_nul(&item.start_date_time),
                end_date_time: with_nul(&item.end_date_time),
                vote_initiator: item.vote_initiator.uuid(),
                vote_type: with_nul(&item.vote_type),
                vote_result: with_nul(&item.vote_result),
                majority: item.majority,
                quorum: item.quorum,
                proposal_text: with_nul(&item.proposal_text),
            },
            vote_item: item
                .votes
                .iter()
                .map(|vote| GroupVoteHistoryItemReplyVoteItemBlock {
                    candidate_id: vote.candidate_id.uuid(),
                    vote_cast: with_nul(&vote.vote_cast),
                    num_votes: vote.num_votes,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `ObjectPropertiesFamily`: an object's condensed broadcast
    /// properties, in response to a client's `RequestObjectPropertiesFamily`
    /// (surfaced as [`ServerEvent::RequestObjectPropertiesFamily`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_object_properties_family(
        &mut self,
        properties: &ObjectPropertiesFamily,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let object_owner_wire =
            crate::types::object_owner_to_wire(properties.owner, properties.group);
        let message = AnyMessage::ObjectPropertiesFamily(ObjectPropertiesFamilyMessage {
            object_data: ObjectPropertiesFamilyObjectDataBlockMessage {
                request_flags: properties.request_flags,
                object_id: properties.object_id.uuid(),
                owner_id: object_owner_wire.0,
                group_id: object_owner_wire.1,
                base_mask: properties.permissions.base.bits(),
                owner_mask: properties.permissions.owner.bits(),
                group_mask: properties.permissions.group.bits(),
                everyone_mask: properties.permissions.everyone.bits(),
                next_owner_mask: properties.permissions.next_owner.bits(),
                ownership_cost: crate::types::linden_to_wire(
                    "OwnershipCost",
                    &properties.ownership_cost,
                )?,
                sale_type: properties.sale_type,
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    properties.sale_price.as_ref(),
                )?,
                category: properties.category,
                last_owner_id: properties.last_owner_id,
                name: with_nul(&properties.name),
                description: with_nul(&properties.description),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ParcelObjectOwnersReply`: the per-owner object tallies for a
    /// parcel, in response to a client's `ParcelObjectOwnersRequest` (surfaced as
    /// [`ServerEvent::RequestParcelObjectOwners`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_parcel_object_owners_reply(
        &mut self,
        owners: &[ParcelObjectOwner],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ParcelObjectOwnersReply(ParcelObjectOwnersReply {
            data: owners
                .iter()
                .map(|owner| ParcelObjectOwnersReplyDataBlock {
                    owner_id: owner.owner.uuid(),
                    is_group_owned: owner.owner.is_group(),
                    count: owner.count,
                    online_status: owner.online_status,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ParcelInfoReply`: a parcel's basic listing, in response to a
    /// client's `ParcelInfoRequest` (surfaced as
    /// [`ServerEvent::RequestParcelInfo`]). The `AgentData.AgentID` is this
    /// session's agent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_parcel_info_reply(
        &mut self,
        details: &ParcelDetails,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ParcelInfoReply(ParcelInfoReply {
            agent_data: ParcelInfoReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            data: ParcelInfoReplyDataBlock {
                parcel_id: details.parcel_id.uuid(),
                owner_id: details.owner_id,
                name: with_nul(&details.name),
                desc: with_nul(&details.description),
                actual_area: crate::types::land_area_to_wire("ActualArea", &details.actual_area)?,
                billable_area: crate::types::land_area_to_wire(
                    "BillableArea",
                    &details.billable_area,
                )?,
                flags: details.flags,
                global_x: global_to_f32(details.global_position.x()),
                global_y: global_to_f32(details.global_position.y()),
                global_z: global_to_f32(details.global_position.z()),
                sim_name: with_nul(&sl_wire::region_name_to_wire(details.sim_name.as_ref())),
                snapshot_id: details.snapshot_id.map_or_else(Uuid::nil, |s| s.uuid()),
                dwell: details.dwell,
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    details.sale_price.as_ref(),
                )?,
                auction_id: details.auction_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ParcelProperties`: one parcel's full record, as a simulator
    /// pushes it unsolicited when an agent enters a parcel and in answer to a
    /// client's `ParcelPropertiesRequest` / `ParcelPropertiesRequestByID`
    /// (surfaced as [`ServerEvent::RequestParcelProperties`] /
    /// [`ServerEvent::RequestParcelPropertiesById`] — echo the request's
    /// `sequence_id` into `info.sequence_id`). The inverse of the client's
    /// [`Event::ParcelProperties`](crate::Event::ParcelProperties) decode.
    /// Sent reliably. The CAPS event-queue form Second Life uses is
    /// [`enqueue_parcel_properties`](Self::enqueue_parcel_properties).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if an L$ amount / the land area does not fit its wire field.
    pub fn send_parcel_properties(&mut self, info: &ParcelInfo, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ParcelProperties(parcel_properties_to_wire(info)?);
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Enqueues a CAPS `ParcelProperties` push — the event-queue form of
    /// [`send_parcel_properties`](Self::send_parcel_properties) that Second
    /// Life (and OpenSim) deliver parcel records through.
    pub fn enqueue_parcel_properties(&mut self, info: &ParcelInfo) {
        self.enqueue_caps_event("ParcelProperties", parcel_properties_to_llsd(info));
    }

    /// Sends one `ParcelOverlay` chunk: `sequence_id` is the chunk index, and
    /// `data` the chunk's per-4 m-cell ownership bytes (a simulator splits the
    /// region's overlay into [`PARCEL_OVERLAY_CHUNK_BYTES`]-byte chunks; see
    /// [`send_parcel_overlay`](Self::send_parcel_overlay) for the whole map).
    /// The inverse of the client's
    /// [`Event::ParcelOverlay`](crate::Event::ParcelOverlay) decode. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_parcel_overlay_chunk(
        &mut self,
        sequence_id: i32,
        data: &[u8],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ParcelOverlay(ParcelOverlay {
            parcel_data: ParcelOverlayParcelDataBlock {
                sequence_id,
                data: data.to_vec(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a whole parcel overlay (one ownership byte per 4 m cell, row-major
    /// from the south-west corner — 4096 bytes for a 256 m region) as the
    /// [`PARCEL_OVERLAY_CHUNK_BYTES`]-byte `ParcelOverlay` chunks a simulator
    /// emits, numbered from `0`. A trailing partial chunk is sent as-is.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if a chunk fails to encode.
    pub fn send_parcel_overlay(&mut self, overlay: &[u8], now: Instant) -> Result<(), Error> {
        for (index, chunk) in overlay.chunks(PARCEL_OVERLAY_CHUNK_BYTES).enumerate() {
            let sequence_id =
                i32::try_from(index).map_err(|_too_many| sl_wire::WireError::ValueOutOfRange {
                    field: "SequenceID",
                    value: i64::try_from(index).unwrap_or(i64::MAX),
                })?;
            self.send_parcel_overlay_chunk(sequence_id, chunk, now)?;
        }
        Ok(())
    }

    /// Sends a full `ObjectUpdate` carrying `objects` (every object in this
    /// session's region, stamped with its handle) — how a simulator rezzes
    /// prims and avatars into a client's view, answers
    /// [`ServerEvent::RequestObjects`], and pushes property changes the terse /
    /// compressed forms cannot carry. The inverse of the client's
    /// [`Event::ObjectAdded`](crate::Event::ObjectAdded) /
    /// [`Event::ObjectUpdated`](crate::Event::ObjectUpdated) decode. Sent
    /// reliably.
    ///
    /// `time_dilation` is the physics time dilation the client reads off the
    /// region-data block, `0xFFFF` meaning "real time".
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_object_update(
        &mut self,
        objects: &[Object],
        time_dilation: u16,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ObjectUpdate(ObjectUpdate {
            region_data: ObjectUpdateRegionDataBlock {
                region_handle: self.region_handle.0,
                time_dilation,
            },
            object_data: objects.iter().map(full_update_block).collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `ObjectUpdateCompressed` carrying `objects` — the packed form a
    /// simulator prefers for bulk rezzing (each object is one
    /// [`encode_compressed_object`](crate::encode_compressed_object) blob).
    /// The client decodes it into the same
    /// [`Event::ObjectAdded`](crate::Event::ObjectAdded) /
    /// [`Event::ObjectUpdated`](crate::Event::ObjectUpdated) as the full form.
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_object_update_compressed(
        &mut self,
        objects: &[Object],
        time_dilation: u16,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ObjectUpdateCompressed(ObjectUpdateCompressed {
            region_data: ObjectUpdateCompressedRegionDataBlock {
                region_handle: self.region_handle.0,
                time_dilation,
            },
            object_data: objects
                .iter()
                .map(|object| ObjectUpdateCompressedObjectDataBlock {
                    update_flags: object.update_flags,
                    data: crate::encode_compressed_object(object),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `KillObject` removing `objects` (by region-local id) from the
    /// client's view — derez, out-of-interest-list, or an avatar leaving. The
    /// inverse of the client's
    /// [`Event::ObjectRemoved`](crate::Event::ObjectRemoved) decode. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_kill_object(
        &mut self,
        objects: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::KillObject(KillObject {
            object_data: objects
                .iter()
                .map(|id| KillObjectObjectDataBlock { id: id.0 })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `EstateCovenantReply`: the estate covenant summary, in response
    /// to a client's `EstateCovenantRequest` (surfaced as
    /// [`ServerEvent::RequestEstateCovenant`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_estate_covenant_reply(
        &mut self,
        covenant: &EstateCovenant,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::EstateCovenantReply(EstateCovenantReply {
            data: EstateCovenantReplyDataBlock {
                covenant_id: covenant.covenant_id.unwrap_or_else(Uuid::nil),
                covenant_timestamp: covenant.covenant_timestamp,
                estate_name: with_nul(&covenant.estate_name),
                estate_owner_id: covenant.estate_owner_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `TelehubInfo`: the region's telehub configuration, in response to
    /// a client's `telehub` `info ui` request (surfaced as
    /// [`ServerEvent::RequestTelehubInfo`]) or after a telehub-management command.
    /// A nil [`TelehubInfo::object_id`] means the region has no telehub.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_telehub_info(&mut self, info: &TelehubInfo, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::TelehubInfo(TelehubInfoMessage {
            telehub_block: TelehubInfoTelehubBlockBlock {
                object_id: info.object_id.map_or_else(Uuid::nil, |o| o.uuid()),
                object_name: with_nul(&info.object_name),
                telehub_pos: info.position.clone(),
                telehub_rot: info.rotation.clone(),
            },
            spawn_point_block: info
                .spawn_points
                .iter()
                .map(|spawn| TelehubInfoSpawnPointBlockBlock {
                    spawn_point_pos: spawn.clone(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `SimStats` carrying the region's periodic performance telemetry
    /// (the inverse of the client's [`Event::SimStats`](crate::Event::SimStats)).
    /// The full 64-bit [`RegionStats::region_flags_extended`] is emitted in a
    /// `RegionInfo` block (so a client reading newer simulators round-trips it),
    /// and `pid` is reported as `0` (the deprecated process-id field the client
    /// ignores). Sent unreliably, at the ~1 Hz cadence a simulator uses.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode (e.g. more than 255 stats).
    pub fn send_sim_stats(&mut self, stats: &RegionStats, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::SimStats(SimStats {
            region: SimStatsRegionBlock {
                region_x: stats.grid_coordinates.x(),
                region_y: stats.grid_coordinates.y(),
                region_flags: stats.region_flags,
                object_capacity: stats.object_capacity,
            },
            stat: stats
                .stats
                .iter()
                .map(|(id, value)| SimStatsStatBlock {
                    stat_id: id.id(),
                    stat_value: *value,
                })
                .collect(),
            pid_stat: SimStatsPidStatBlock { pid: 0 },
            region_info: vec![SimStatsRegionInfoBlock {
                region_flags_extended: stats.region_flags_extended,
            }],
        });
        self.send(&message, Reliability::Unreliable, now)?;
        Ok(())
    }

    /// Sends a `SimulatorViewerTimeMessage` carrying the simulator's world clock
    /// and sun state (the inverse of the client's
    /// [`Event::SimulatorTime`](crate::Event::SimulatorTime)), so the client can
    /// resynchronise its day cycle. Sent unreliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_simulator_time(&mut self, time: &SimulatorTime, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::SimulatorViewerTimeMessage(SimulatorViewerTimeMessage {
            time_info: SimulatorViewerTimeMessageTimeInfoBlock {
                usec_since_start: time.usec_since_start,
                sec_per_day: time.sec_per_day,
                sec_per_year: time.sec_per_year,
                sun_direction: time.sun_direction.clone(),
                sun_phase: time.sun_phase,
                sun_ang_velocity: time.sun_ang_velocity.clone(),
            },
        });
        self.send(&message, Reliability::Unreliable, now)?;
        Ok(())
    }

    /// Sends a `GenericMessage` — the method-name + parameter-list envelope the
    /// simulator uses for a grab-bag of loosely-coupled features (the inverse of
    /// the client's
    /// [`Event::GenericMessage`](crate::Event::GenericMessage)). The method name,
    /// [`InvoiceId`](crate::InvoiceId) and opaque parameter blobs are carried
    /// verbatim; the `AgentData` block reports the circuit's agent/session ids
    /// with a nil transaction id. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode (e.g. more than 255 parameters).
    pub fn send_generic_message(
        &mut self,
        generic: &GenericMessage,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GenericMessage(GenericMessageWire {
            agent_data: GenericMessageAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
                transaction_id: Uuid::nil(),
            },
            method_data: GenericMessageMethodDataBlock {
                method: generic.method.clone().into_bytes(),
                invoice: generic.invoice.get(),
            },
            param_list: generic
                .params
                .iter()
                .map(|parameter| GenericMessageParamListBlock {
                    parameter: parameter.clone(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `LargeGenericMessage` — the same method-name + parameter-list
    /// envelope as [`send_generic_message`](Self::send_generic_message) but with
    /// a larger per-parameter wire limit (the inverse of the client's
    /// [`Event::LargeGenericMessage`](crate::Event::LargeGenericMessage)). Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode (e.g. more than 255 parameters).
    pub fn send_large_generic_message(
        &mut self,
        generic: &GenericMessage,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::LargeGenericMessage(LargeGenericMessageWire {
            agent_data: LargeGenericMessageAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
                transaction_id: Uuid::nil(),
            },
            method_data: LargeGenericMessageMethodDataBlock {
                method: generic.method.clone().into_bytes(),
                invoice: generic.invoice.get(),
            },
            param_list: generic
                .params
                .iter()
                .map(|parameter| LargeGenericMessageParamListBlock {
                    parameter: parameter.clone(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GenericStreamingMessage` — the optimised streaming envelope with
    /// a numeric method id and a single opaque payload (the inverse of the
    /// client's
    /// [`Event::GenericStreamingMessage`](crate::Event::GenericStreamingMessage)),
    /// used for payloads like a GLTF material override. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_generic_streaming_message(
        &mut self,
        streaming: &GenericStreamingMessage,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GenericStreamingMessage(GenericStreamingMessageWire {
            method_data: GenericStreamingMessageMethodDataBlock {
                method: streaming.method,
            },
            data_block: GenericStreamingMessageDataBlockBlock {
                data: streaming.data.clone(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `Error` — the lowest-common-denominator UDP error channel a
    /// simulator (or a service behind it) uses to report a failed exchange (the
    /// inverse of the client's
    /// [`Event::ServerError`](crate::Event::ServerError)). The recipient
    /// [`AgentKey`], HTTP-like [`code`](crate::ServerError::code), short
    /// [`token`](crate::ServerError::token), polymorphic correlation
    /// [`id`](crate::ServerError::id), originating
    /// [`system`](crate::ServerError::system) path, human-readable
    /// [`message`](crate::ServerError::message), and verbatim binary-LLSD
    /// [`data`](crate::ServerError::data) blob are all carried as supplied. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_error(&mut self, error: &ServerError, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::Error(ErrorWire {
            agent_data: ErrorAgentDataBlock {
                agent_id: error.agent.uuid(),
            },
            data: ErrorDataBlock {
                code: error.code,
                token: with_nul(&error.token),
                id: error.id,
                system: with_nul(&error.system),
                message: with_nul(&error.message),
                data: error.data.clone(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `FeatureDisabled` — a notice that a feature the agent asked for is
    /// unavailable (the inverse of the client's
    /// [`Event::FeatureDisabled`](crate::Event::FeatureDisabled)). Carries the
    /// human-readable reason, the recipient [`AgentKey`], and the
    /// [`TransactionId`](crate::TransactionId) of the exchange the feature would
    /// have served (often nil). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_feature_disabled(
        &mut self,
        disabled: &FeatureDisabled,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::FeatureDisabled(FeatureDisabledWire {
            failure_info: FeatureDisabledFailureInfoBlock {
                error_message: with_nul(&disabled.message),
                agent_id: disabled.agent.uuid(),
                transaction_id: disabled.transaction.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `KickUser` — a server-initiated forced logout (the inverse of the
    /// client's [`Event::Kicked`](crate::Event::Kicked)), for example when the
    /// same account logs in elsewhere. Carries the kicked [`AgentKey`] and the
    /// human-readable reason; the `SessionID` echo is filled from the circuit and
    /// the routing `TargetBlock` (target sim address) is zeroed, since the client
    /// drops both. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_kick_user(&mut self, kick: &Kick, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::KickUser(KickUser {
            target_block: KickUserTargetBlockBlock {
                target_ip: [0; 4],
                target_port: 0,
            },
            user_info: KickUserUserInfoBlock {
                agent_id: kick.agent.uuid(),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
                reason: with_nul(&kick.reason),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `ObjectAnimation` — the complete, authoritative set of
    /// animations now signalled on an animated-mesh (animesh) object (the inverse
    /// of the client's
    /// [`Event::ObjectAnimation`](crate::Event::ObjectAnimation)). Pushed whenever
    /// a scripted object's animation set changes (e.g. `llStartObjectAnimation`).
    /// As with avatar animations the list is the full state, not a delta: an
    /// animation that stops simply drops out of a later update. Carries the
    /// animated [`ObjectKey`] and each playing animation's
    /// [`AnimationKey`](crate::AnimationKey) and per-object sequence id. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_object_animation(
        &mut self,
        object_id: ObjectKey,
        animations: &[ObjectPlayingAnimation],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ObjectAnimation(ObjectAnimationWire {
            sender: ObjectAnimationSenderBlock {
                id: object_id.uuid(),
            },
            animation_list: animations
                .iter()
                .map(|animation| ObjectAnimationAnimationListBlock {
                    anim_id: animation.anim_id.uuid(),
                    anim_sequence_id: animation.sequence_id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `RebakeAvatarTextures` — a request that the agent regenerate and
    /// re-upload one of its temporary baked-avatar textures the simulator can no
    /// longer find (the inverse of the client's
    /// [`Event::RebakeAvatarTextures`](crate::Event::RebakeAvatarTextures)).
    /// Carries the [`TextureKey`] of the missing baked texture. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_rebake_avatar_textures(
        &mut self,
        texture_id: TextureKey,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::RebakeAvatarTextures(RebakeAvatarTexturesWire {
            texture_data: RebakeAvatarTexturesTextureDataBlock {
                texture_id: texture_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `TerminateFriendship` — informs the client that a friendship has
    /// ended (the inverse of the client's
    /// [`Event::FriendshipTerminated`](crate::Event::FriendshipTerminated)),
    /// either because the former friend removed this agent or because a removal
    /// this agent requested has been confirmed. Carries the [`FriendKey`] of the
    /// former friend in the `ExBlock`; the echoed `AgentData` identifies the
    /// recipient (this circuit's agent). A client mirroring the buddy list should
    /// drop `other`. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_terminate_friendship(
        &mut self,
        other: FriendKey,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::TerminateFriendship(TerminateFriendship {
            agent_data: TerminateFriendshipAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            ex_block: TerminateFriendshipExBlockBlock {
                other_id: other.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a fully-specified `ImprovedInstantMessage` to the client — the
    /// simulator-side IM delivery, the inverse of the client's
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived)
    /// (or the typed session/friendship events its dialog folds into). This
    /// is the relay primitive for agent-to-agent traffic: a driver takes a
    /// [`ServerEvent::InstantMessage`] off the sender's [`SimSession`] and
    /// passes it — unchanged or with the dialog swapped (e.g. a
    /// [`ImDialog::FriendshipDeclined`](crate::ImDialog::FriendshipDeclined)
    /// relay of a decline) — to the recipient's session. Every
    /// dialog-dependent field ([`InstantMessage::id`](crate::InstantMessage::id),
    /// `from_group`, the binary bucket) is the caller's. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_instant_message(&mut self, im: &InstantMessage, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: im.from_agent_id.uuid(),
                // A simulator-sent IM carries no viewer session id.
                session_id: Uuid::nil(),
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: im.from_group,
                to_agent_id: im.to_agent_id.uuid(),
                parent_estate_id: im.parent_estate_id,
                region_id: crate::types::optional_uuid_to_wire(im.region_id),
                position: Vector {
                    x: im.position.x(),
                    y: im.position.y(),
                    z: im.position.z(),
                },
                offline: u8::from(im.offline),
                dialog: im.dialog.to_u8(),
                id: im.id,
                timestamp: crate::types::optional_u32_to_wire(im.timestamp),
                from_agent_name: with_nul(&im.from_agent_name),
                message: with_nul(&im.message),
                binary_bucket: im.binary_bucket.clone(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `OnlineNotification` — tells the client the listed friends
    /// are online (the inverse of the client's
    /// [`Event::FriendsOnline`](crate::Event::FriendsOnline)). Presence is a
    /// grid-level service — the driver decides who to notify when; this
    /// session only delivers. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_online_notification(
        &mut self,
        friends: &[FriendKey],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: friends
                .iter()
                .map(|friend| OnlineNotificationAgentBlockBlock {
                    agent_id: friend.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `OfflineNotification` — tells the client the listed friends
    /// went offline (the inverse of the client's
    /// [`Event::FriendsOffline`](crate::Event::FriendsOffline)). Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_offline_notification(
        &mut self,
        friends: &[FriendKey],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: friends
                .iter()
                .map(|friend| OfflineNotificationAgentBlockBlock {
                    agent_id: friend.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ChangeUserRights` — tells the client a friendship's rights
    /// changed (the inverse of the client's
    /// [`Event::FriendRightsChanged`](crate::Event::FriendRightsChanged)).
    /// The client decodes the direction from `changer`: this circuit's own
    /// agent id means each entry echoes a grant *this* agent made
    /// ([`UserRightsEntry::agent`] is the friend); any other id means that
    /// friend changed what they grant this agent ([`UserRightsEntry::agent`]
    /// is then this circuit's agent id, as the reference simulators send it).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_change_user_rights(
        &mut self,
        changer: AgentKey,
        rights: &[UserRightsEntry],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ChangeUserRights(ChangeUserRights {
            agent_data: ChangeUserRightsAgentDataBlock {
                agent_id: changer.uuid(),
            },
            rights: rights
                .iter()
                .map(|entry| ChangeUserRightsRightsBlock {
                    agent_related: entry.agent.uuid(),
                    related_rights: entry.rights.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a group/conference session message from `from` into
    /// `session_id` — the relay half of [`ServerEvent::SessionMessageSent`]
    /// (a `SessionSend` IM; `from_group` selects how the client folds it:
    /// `true` surfaces as a group message, `false` as a conference message).
    /// When this session knows `session_id`, the message is also appended to
    /// its server history. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_session_message(
        &mut self,
        session_id: ImSessionId,
        from: AgentKey,
        from_name: &str,
        message: &str,
        from_group: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let im = InstantMessage {
            from_agent_id: from,
            from_agent_name: from_name.to_owned(),
            to_agent_id: self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil())),
            dialog: ImDialog::SessionSend,
            from_group,
            region_id: None,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id: session_id.get(),
            parent_estate_id: 0,
            message: message.to_owned(),
            binary_bucket: Vec::new(),
        };
        self.send_instant_message(&im, now)?;
        if let Some(chat_session) = self.chat_sessions.get_mut(&session_id) {
            chat_session.log(ServerHistoryMessage {
                sender: from,
                sender_name: from_name.to_owned(),
                text: message.to_owned(),
                timestamp: None,
            });
        }
        Ok(())
    }

    /// Sends a group/conference roster notification: `agent` joined
    /// (`SessionAdd`) or left (`SessionLeave`) `session_id` — the relay half
    /// of [`ServerEvent::SessionLeaveRequested`] and of a peer joining
    /// (`from_group` selects the client-side fold, as on
    /// [`SimSession::send_session_message`]). When this session knows
    /// `session_id`, its roster is folded the same way (a session emptied by
    /// a leave is removed). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_session_participant(
        &mut self,
        session_id: ImSessionId,
        agent: AgentKey,
        agent_name: &str,
        joined: bool,
        from_group: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let im = InstantMessage {
            from_agent_id: agent,
            from_agent_name: agent_name.to_owned(),
            to_agent_id: self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil())),
            dialog: if joined {
                ImDialog::SessionAdd
            } else {
                ImDialog::SessionLeave
            },
            from_group,
            region_id: None,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id: session_id.get(),
            parent_estate_id: 0,
            message: String::new(),
            binary_bucket: Vec::new(),
        };
        self.send_instant_message(&im, now)?;
        if let Some(chat_session) = self.chat_sessions.get_mut(&session_id) {
            if joined {
                chat_session.participants.insert(agent);
            } else {
                chat_session.participants.remove(&agent);
                if chat_session.participants.is_empty() {
                    self.chat_sessions.remove(&session_id);
                }
            }
        }
        Ok(())
    }

    /// Materialises a chat session in this session's registry without any
    /// wire traffic — the driver API for the relay topology: a conference
    /// started on one client's [`SimSession`]
    /// ([`ServerEvent::ConferenceStartRequested`]) must also exist on each
    /// invitee's session (whose region never saw the starter's
    /// conference-start IM) before messages relay through it. Extends the
    /// roster of an already-known session.
    pub fn open_chat_session(
        &mut self,
        session_id: ImSessionId,
        kind: SimChatSessionKind,
        participants: &[AgentKey],
    ) {
        let chat_session = self
            .chat_sessions
            .entry(session_id)
            .or_insert_with(|| SimChatSession {
                kind,
                participants: BTreeSet::new(),
                history: Vec::new(),
            });
        chat_session
            .participants
            .extend(participants.iter().copied());
    }

    /// Sends an `OfferCallingCard` — another agent offers this agent their
    /// calling card (the inverse of the client's
    /// [`Event::CallingCardOffered`](crate::Event::CallingCardOffered)), a
    /// reference card to that avatar that, if accepted, is filed in this agent's
    /// Calling Cards folder. This is not a friendship request. `offering_agent`
    /// is the avatar making the offer (carried in `AgentData`); the offer is
    /// addressed to this circuit's agent (the `AgentBlock` destination), and the
    /// [`TransactionId`] correlates the client's accept/decline reply. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_offer_calling_card(
        &mut self,
        offering_agent: AgentKey,
        transaction: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::OfferCallingCard(OfferCallingCard {
            agent_data: OfferCallingCardAgentDataBlock {
                agent_id: offering_agent.uuid(),
                session_id: Uuid::nil(),
            },
            agent_block: OfferCallingCardAgentBlockBlock {
                dest_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                transaction_id: transaction.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `AcceptCallingCard` — a calling card this agent offered was
    /// accepted (the inverse of the client's
    /// [`Event::CallingCardAccepted`](crate::Event::CallingCardAccepted)).
    /// `agent` is the avatar who accepted (carried in `AgentData`), and the
    /// [`TransactionId`] echoes the original offer. The accepter's destination
    /// inventory folder is theirs, not this agent's, so an empty `FolderData` is
    /// sent. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_accept_calling_card(
        &mut self,
        agent: AgentKey,
        transaction: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::AcceptCallingCard(AcceptCallingCard {
            agent_data: AcceptCallingCardAgentDataBlock {
                agent_id: agent.uuid(),
                session_id: Uuid::nil(),
            },
            transaction_block: AcceptCallingCardTransactionBlockBlock {
                transaction_id: transaction.get(),
            },
            folder_data: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DeclineCallingCard` — a calling card this agent offered was
    /// declined (the inverse of the client's
    /// [`Event::CallingCardDeclined`](crate::Event::CallingCardDeclined)).
    /// `agent` is the avatar who declined (carried in `AgentData`), and the
    /// [`TransactionId`] echoes the original offer. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_decline_calling_card(
        &mut self,
        agent: AgentKey,
        transaction: TransactionId,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DeclineCallingCard(DeclineCallingCard {
            agent_data: DeclineCallingCardAgentDataBlock {
                agent_id: agent.uuid(),
                session_id: Uuid::nil(),
            },
            transaction_block: DeclineCallingCardTransactionBlockBlock {
                transaction_id: transaction.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `RemoveInventoryItem` — tells the client the simulator deleted one
    /// or more inventory items server-side, so a client mirroring inventory can
    /// drop them (the inverse of the client's
    /// [`Event::InventoryItemsRemoved`](crate::Event::InventoryItemsRemoved)).
    /// The echoed `AgentData.AgentID` is the recipient agent. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_remove_inventory_item(
        &mut self,
        items: &[InventoryKey],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::RemoveInventoryItem(RemoveInventoryItem {
            agent_data: RemoveInventoryItemAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            inventory_data: items
                .iter()
                .map(|item| RemoveInventoryItemInventoryDataBlock {
                    item_id: item.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `RemoveInventoryFolder` — tells the client the simulator deleted
    /// one or more inventory folders (and their cached descendents) server-side
    /// (the inverse of the client's
    /// [`Event::InventoryFoldersRemoved`](crate::Event::InventoryFoldersRemoved)).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_remove_inventory_folder(
        &mut self,
        folders: &[InventoryFolderKey],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::RemoveInventoryFolder(RemoveInventoryFolder {
            agent_data: RemoveInventoryFolderAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            folder_data: folders
                .iter()
                .map(|folder| RemoveInventoryFolderFolderDataBlock {
                    folder_id: folder.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `RemoveInventoryObjects` — tells the client the simulator deleted a
    /// mixed set of inventory folders and items in one message (the inverse of the
    /// client's
    /// [`Event::InventoryObjectsRemoved`](crate::Event::InventoryObjectsRemoved)).
    /// Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_remove_inventory_objects(
        &mut self,
        folders: &[InventoryFolderKey],
        items: &[InventoryKey],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::RemoveInventoryObjects(RemoveInventoryObjects {
            agent_data: RemoveInventoryObjectsAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            folder_data: folders
                .iter()
                .map(|folder| RemoveInventoryObjectsFolderDataBlock {
                    folder_id: folder.uuid(),
                })
                .collect(),
            item_data: items
                .iter()
                .map(|item| RemoveInventoryObjectsItemDataBlock {
                    item_id: item.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `MoveInventoryItem` — tells the client the simulator re-parented
    /// (and optionally renamed) inventory items server-side (the inverse of the
    /// client's
    /// [`Event::InventoryItemsMoved`](crate::Event::InventoryItemsMoved)). Each
    /// [`InventoryItemMove`] with a `new_name` of `None` packs an empty wire
    /// `NewName`, which the client reads back as "no rename"; `stamp` echoes the
    /// re-timestamp flag. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_move_inventory_item(
        &mut self,
        stamp: bool,
        moves: &[InventoryItemMove],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::MoveInventoryItem(MoveInventoryItem {
            agent_data: MoveInventoryItemAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
                stamp,
            },
            inventory_data: moves
                .iter()
                .map(|item| MoveInventoryItemInventoryDataBlock {
                    item_id: item.item.uuid(),
                    folder_id: item.folder.uuid(),
                    new_name: item
                        .new_name
                        .as_deref()
                        .unwrap_or_default()
                        .as_bytes()
                        .to_vec(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ReplyTaskInventory` — the contents serial and temporary Xfer
    /// filename of an in-world object's task inventory, in reply to the client's
    /// `RequestTaskInventory` (the inverse of the client's
    /// [`Event::TaskInventoryReply`](crate::Event::TaskInventoryReply)). An empty
    /// filename means the task inventory is empty. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_reply_task_inventory(
        &mut self,
        reply: &TaskInventoryReply,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ReplyTaskInventory(ReplyTaskInventory {
            inventory_data: ReplyTaskInventoryInventoryDataBlock {
                task_id: reply.task.uuid(),
                serial: reply.serial,
                filename: reply.filename.as_bytes().to_vec(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sets the account's secure session id (from the login response), enabling
    /// the legacy transaction asset upload path: the simulator derives a stored
    /// asset id as `combine(transaction_id, secure_session_id)`, exactly as the
    /// uploading client predicts it. An `AssetUploadRequest` arriving while this
    /// is unset is refused with a failed `AssetUploadComplete`.
    pub const fn set_secure_session_id(&mut self, id: Uuid) {
        self.secure_session_id = Some(id);
    }

    /// Registers `filename` as servable over the legacy `Xfer` download path:
    /// the next client `RequestXfer` naming it streams `data` in
    /// `SendXferPacket`s (one in flight, paced by the client's
    /// `ConfirmXferPacket`s). The entry is consumed by that request — the
    /// transfers ask for delete-on-completion — so re-serving the same name
    /// needs a fresh registration.
    pub fn register_xfer_file(&mut self, filename: impl Into<String>, data: Vec<u8>) {
        let _prev = self.xfer_files.insert(filename.into(), data);
    }

    /// Serves an in-world object's task inventory — the full server side of the
    /// client's
    /// [`Session::fetch_task_inventory`](crate::Session::fetch_task_inventory):
    /// writes the contents listing in the exact `RequestInventoryFile` text
    /// format (the inverse of the client-side parser behind
    /// [`Event::TaskInventoryContents`](crate::Event::TaskInventoryContents)),
    /// registers it under the deterministic name
    /// `inventory_<task>.tmp` (a real simulator mints a random temp name; the
    /// client treats it as opaque, and a sans-I/O session has no randomness),
    /// and sends the `ReplyTaskInventory` naming it. Pass the current contents
    /// `serial`. For a task whose inventory is empty a real simulator sends an
    /// empty filename instead — use
    /// [`send_reply_task_inventory`](Self::send_reply_task_inventory) directly
    /// for that case.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the reply fails to encode.
    pub fn serve_task_inventory(
        &mut self,
        task: ObjectKey,
        serial: i16,
        items: &[TaskInventoryItem],
        now: Instant,
    ) -> Result<(), Error> {
        let filename = format!("inventory_{}.tmp", task.uuid());
        let listing = build_task_inventory(task, items);
        self.register_xfer_file(filename.clone(), listing.into_bytes());
        self.send_reply_task_inventory(
            &TaskInventoryReply {
                task,
                serial,
                filename,
            },
            now,
        )
    }

    /// Offers the client a server-produced file: registers `data` under
    /// `sim_filename` and sends an `InitiateDownload` naming it, echoing
    /// `viewer_filename` so the client can tag the result. The client follows
    /// the offer automatically with a `RequestXfer` for `sim_filename` and
    /// surfaces the bytes as
    /// [`Event::ServerFileDownloaded`](crate::Event::ServerFileDownloaded) —
    /// the server half of the estate terrain RAW download
    /// ([`ServerEvent::TerrainDownloadRequested`]), where a real simulator
    /// mints a random `sim_filename`. Single-shot: the registered entry is
    /// consumed by the request.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the offer fails to encode.
    pub fn send_initiate_download(
        &mut self,
        sim_filename: impl Into<String>,
        viewer_filename: &str,
        data: Vec<u8>,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let sim_filename = sim_filename.into();
        let message = AnyMessage::InitiateDownload(InitiateDownload {
            agent_data: InitiateDownloadAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |agent| agent.uuid()),
            },
            file_data: InitiateDownloadFileDataBlock {
                sim_filename: with_nul(&sim_filename),
                viewer_filename: with_nul(viewer_filename),
            },
        });
        self.register_xfer_file(sim_filename, data);
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Pulls a named file from the client: sends a `RequestXfer` for
    /// `filename` (no `VFileID`, small packets — the shape OpenSim's
    /// `EstateTerrainXferHandler` uses) and reassembles the client's
    /// `SendXferPacket` stream, confirming each packet, into
    /// [`ServerEvent::XferReceived`]. The server half of the client's
    /// [`Session::request_region_terrain_upload`](crate::Session::request_region_terrain_upload),
    /// issued in answer to [`ServerEvent::TerrainUploadRequested`]. Returns
    /// the simulator-minted transfer id (the one a later
    /// [`abort_xfer`](Self::abort_xfer) names).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the request fails to encode.
    pub fn request_xfer_upload(&mut self, filename: &str, now: Instant) -> Result<XferId, Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let xfer_id = self.alloc_xfer_id();
        self.xfer_receives.insert(
            xfer_id,
            SimXferReceive {
                purpose: SimXferReceivePurpose::NamedFile {
                    filename: filename.to_owned(),
                },
                buffer: Vec::new(),
            },
        );
        let pull = AnyMessage::RequestXfer(RequestXfer {
            xfer_id: RequestXferXferIDBlock {
                id: xfer_id.get(),
                filename: with_nul(filename),
                file_path: 0,
                delete_on_completion: false,
                use_big_packets: false,
                v_file_id: Uuid::nil(),
                v_file_type: 0,
            },
        });
        self.send(&pull, Reliability::Reliable, now)?;
        Ok(xfer_id)
    }

    /// Aborts an in-flight `Xfer` transfer (either direction) with the given
    /// result code: drops its state and tells the client (`AbortXfer`), the
    /// inverse of the client's abort handling that surfaces
    /// [`Event::XferAborted`](crate::Event::XferAborted).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open,
    /// [`Error::UnknownXfer`] if no send or receive with that id is in flight
    /// (nothing is sent), or a wire error if the message fails to encode.
    pub fn abort_xfer(&mut self, xfer_id: XferId, result: i32, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let send = self.xfer_sends.remove(&xfer_id);
        let receive = self.xfer_receives.remove(&xfer_id);
        if send.is_none() && receive.is_none() {
            return Err(Error::UnknownXfer);
        }
        let message = AnyMessage::AbortXfer(AbortXfer {
            xfer_id: AbortXferXferIDBlock {
                id: xfer_id.get(),
                result,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Allocates the next simulator-assigned `Xfer` id (used for asset pulls),
    /// mirroring the client's own mint: monotonically increasing, never zero.
    fn alloc_xfer_id(&mut self) -> XferId {
        let id = self.next_xfer_id;
        self.next_xfer_id = XferId(self.next_xfer_id.get().checked_add(1).unwrap_or(1));
        id
    }

    /// Streams the next chunk of the outbound `Xfer` send `xfer_id` as a
    /// `SendXferPacket` — the server side of the strictly one-packet-in-flight
    /// pacing, the mirror of the client's `send_next_xfer_upload_packet`,
    /// framed by the shared [`sl_wire::xfer`] codec (size prefix on sequence
    /// 0, EOF flag on the last packet). A fully-sent send is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownXfer`] if no send with that id is in flight, or
    /// a wire error if the message fails to encode.
    fn send_next_xfer_send_packet(&mut self, xfer_id: XferId, now: Instant) -> Result<(), Error> {
        let Some(send) = self.xfer_sends.get_mut(&xfer_id) else {
            return Err(Error::UnknownXfer);
        };
        let sequence = send.next_sequence;
        let Some(packet) = next_xfer_chunk(&send.data, send.sent, sequence) else {
            return Ok(());
        };
        send.sent = packet.sent;
        send.last_sent = packet.id.is_last();
        send.next_sequence = sequence.wrapping_add(1);
        let message = AnyMessage::SendXferPacket(SendXferPacket {
            xfer_id: SendXferPacketXferIDBlock {
                id: xfer_id.get(),
                packet: packet.id.raw(),
            },
            data_packet: SendXferPacketDataPacketBlock {
                data: packet.payload,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Answers a [`ServerEvent::TransferRequested`] with the asset bytes: a
    /// `TransferInfo` header (status `Ok`, the declared size, the request
    /// params echoed back) followed by the `TransferPacket` stream — status
    /// `Ok` per packet and `Done` on the last, exactly as the reference
    /// serving side sends them. Packets need no per-packet acknowledgement
    /// (all ride the reliable channel).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open,
    /// [`Error::UnknownTransfer`] if `transfer_id` is not awaiting an answer
    /// (never requested, already answered, or cancelled), or a wire error if a
    /// message fails to encode.
    pub fn send_transfer_asset(
        &mut self,
        transfer_id: TransferId,
        data: &[u8],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let Some(params) = self.transfer_serves.remove(&transfer_id) else {
            return Err(Error::UnknownTransfer);
        };
        let info = AnyMessage::TransferInfo(TransferInfo {
            transfer_info: TransferInfoTransferInfoBlock {
                transfer_id: transfer_id.get(),
                channel_type: TRANSFER_CHANNEL_ASSET,
                target_type: 0,
                status: TransferStatus::Ok.to_code(),
                size: i32::try_from(data.len()).unwrap_or(i32::MAX),
                params,
            },
        });
        self.send(&info, Reliability::Reliable, now)?;
        // Stream the packets; an empty asset is one empty `Done` packet.
        let mut index: i32 = 0;
        let mut offset = 0_usize;
        loop {
            let end = offset.saturating_add(TRANSFER_CHUNK_SIZE).min(data.len());
            let chunk = data.get(offset..end).unwrap_or(&[]);
            let is_last = end >= data.len();
            let status = if is_last {
                TransferStatus::Done
            } else {
                TransferStatus::Ok
            };
            let packet = AnyMessage::TransferPacket(TransferPacket {
                transfer_data: TransferPacketTransferDataBlock {
                    transfer_id: transfer_id.get(),
                    channel_type: TRANSFER_CHANNEL_ASSET,
                    packet: index,
                    status: status.to_code(),
                    data: chunk.to_vec(),
                },
            });
            self.send(&packet, Reliability::Reliable, now)?;
            if is_last {
                break;
            }
            offset = end;
            index = index.saturating_add(1);
        }
        Ok(())
    }

    /// Answers a [`ServerEvent::TransferRequested`] with a refusal: a
    /// `TransferInfo` carrying the non-`Ok` `status` (asset missing ⇒
    /// [`TransferStatus::UnknownSource`], no permission ⇒
    /// [`TransferStatus::InsufficientPermissions`]) and size 0, which the
    /// requesting client surfaces as
    /// [`Event::TransferFailed`](crate::Event::TransferFailed).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open,
    /// [`Error::UnknownTransfer`] if `transfer_id` is not awaiting an answer,
    /// or a wire error if the message fails to encode.
    pub fn send_transfer_fail(
        &mut self,
        transfer_id: TransferId,
        status: TransferStatus,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let Some(params) = self.transfer_serves.remove(&transfer_id) else {
            return Err(Error::UnknownTransfer);
        };
        let message = AnyMessage::TransferInfo(TransferInfo {
            transfer_info: TransferInfoTransferInfoBlock {
                transfer_id: transfer_id.get(),
                channel_type: TRANSFER_CHANNEL_ASSET,
                target_type: 0,
                status: status.to_code(),
                size: 0,
                params,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `AvatarSitResponse` — accepts a [`ServerEvent::SitRequested`]
    /// by placing the agent on `sit_object` with the given seat `transform`
    /// (the inverse of the client's
    /// [`Event::SitResult`](crate::Event::SitResult)). The sit machine then
    /// awaits the client's completing `AgentSit`
    /// ([`ServerEvent::SitConfirmed`]); refusing a sit request is simply not
    /// answering (the reference simulators send no refusal message — the
    /// client's own sit timeout recovers it).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_avatar_sit_response(
        &mut self,
        sit_object: ObjectKey,
        transform: &SitTransform,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::AvatarSitResponse(AvatarSitResponse {
            sit_object: AvatarSitResponseSitObjectBlock {
                id: sit_object.uuid(),
            },
            sit_transform: AvatarSitResponseSitTransformBlock {
                auto_pilot: transform.autopilot,
                sit_position: transform.sit_position.clone(),
                sit_rotation: transform.sit_rotation.clone(),
                camera_eye_offset: transform.camera_eye_offset.clone(),
                camera_at_offset: transform.camera_at_offset.clone(),
                force_mouselook: transform.force_mouselook,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        self.sit = SimSitState::ResponseSent { on: sit_object };
        Ok(())
    }

    /// Sends a `TeleportStart` — the simulator accepted a teleport request and
    /// the client should show its teleport screen (the inverse of the client's
    /// [`Event::TeleportStarted`](crate::Event::TeleportStarted)). The
    /// sequencing of a teleport is the driver's job: a sans-I/O session cannot
    /// know whether an inter-region teleport succeeds (the destination is
    /// another [`SimSession`]), so the driver strings together start /
    /// progress / local / failed and the event-queue trio itself. Teleport
    /// answers are a root-agent affair — sending them on a child circuit is
    /// not an error here, but a real viewer would ignore them.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_teleport_start(&mut self, teleport_flags: u32, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::TeleportStart(TeleportStart {
            info: TeleportStartInfoBlock { teleport_flags },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `TeleportProgress` — a human-readable progress line for the
    /// client's teleport screen (the inverse of the client's
    /// [`Event::TeleportProgress`](crate::Event::TeleportProgress)). The
    /// message is sent NUL-terminated, as a simulator does on the wire.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_teleport_progress(
        &mut self,
        message: &str,
        teleport_flags: u32,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::TeleportProgress(TeleportProgress {
            agent_data: TeleportProgressAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            info: TeleportProgressInfoBlock {
                teleport_flags,
                message: with_nul(message),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `TeleportLocal` — finishes an **intra-region** teleport by
    /// placing the avatar at `position` with no circuit change (the inverse of
    /// the client's [`Event::TeleportLocal`](crate::Event::TeleportLocal)).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_teleport_local(
        &mut self,
        position: RegionCoordinates,
        look_at: Vector,
        teleport_flags: u32,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::TeleportLocal(TeleportLocal {
            info: TeleportLocalInfoBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                // The reference simulators send 0; the viewer ignores it.
                location_id: 0,
                position: Vector {
                    x: position.x(),
                    y: position.y(),
                    z: position.z(),
                },
                look_at,
                teleport_flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `TeleportFailed` — refuses or aborts a requested teleport with
    /// a human-readable `reason`, returning the client to its active state
    /// (the inverse of the client's
    /// [`Event::TeleportFailed`](crate::Event::TeleportFailed)). The reason is
    /// sent NUL-terminated; no extended `AlertInfo` block is attached (pass
    /// the reason itself, as OpenSim does).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_teleport_failed(&mut self, reason: &str, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::TeleportFailed(TeleportFailed {
            info: TeleportFailedInfoBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                reason: with_nul(reason),
            },
            alert_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DisableSimulator` — tells the client to tear down this circuit
    /// (used when retiring a child circuit, e.g. the source region after a
    /// completed teleport or a neighbour leaving the interest set). The client
    /// drops the circuit and reaps its objects.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_disable_simulator(&mut self, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DisableSimulator(DisableSimulator {});
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Enqueues a CAPS `EnableSimulator` event — announces a neighbouring (or
    /// teleport-destination) region so the client opens a **child** circuit to
    /// it (the modern event-queue path; the client answers with a
    /// `UseCircuitCode` on `sim`).
    pub fn enqueue_enable_simulator(&mut self, handle: RegionHandle, sim: SocketAddr) {
        self.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(handle.0, sim),
        );
    }

    /// Enqueues a CAPS `EstablishAgentCommunication` event — hands the client
    /// the child region's seed capability (this event has **no** UDP form).
    /// The client caches the seed and surfaces it so its driver POSTs it,
    /// which is what makes a region start streaming to the child agent.
    pub fn enqueue_establish_agent_communication(&mut self, sim: SocketAddr, seed: &str) {
        self.enqueue_caps_event(
            "EstablishAgentCommunication",
            establish_agent_communication_to_llsd(sim, seed),
        );
    }

    /// Enqueues a CAPS `TeleportFinish` event — completes an **inter-region**
    /// teleport by handing the client the destination simulator's address,
    /// seed capability, maturity rating and teleport flags. The client sends
    /// `CompleteAgentMovement` on its (child) circuit to `dest`; the
    /// destination's `AgentMovementComplete` commits the handover.
    pub fn enqueue_teleport_finish(
        &mut self,
        dest: SocketAddr,
        seed: &str,
        sim_access: u8,
        teleport_flags: u32,
    ) {
        self.enqueue_caps_event(
            "TeleportFinish",
            teleport_finish_to_llsd(dest, seed, sim_access, teleport_flags),
        );
    }

    /// Enqueues a CAPS `CrossedRegion` event — the avatar walked over a region
    /// border; the client promotes its pre-opened child circuit to `dest` to
    /// root (no teleport screen).
    pub fn enqueue_crossed_region(&mut self, handle: RegionHandle, dest: SocketAddr, seed: &str) {
        self.enqueue_caps_event(
            "CrossedRegion",
            crossed_region_to_caps_llsd(handle.0, dest, seed),
        );
    }

    /// Queues a `ChatterBoxInvitation` on the event queue — invites this
    /// session's client into a group/conference chat (the inverse of the
    /// client's
    /// [`Event::ConferenceInvited`](crate::Event::ConferenceInvited), which
    /// is also the expected `invitation` payload — the shape
    /// [`chatterbox_invitation_to_llsd`](crate::chatterbox_invitation_to_llsd)
    /// serializes). Any other [`Event`] variant enqueues nothing (the same
    /// contract as the conversion, which serializes it as undefined).
    pub fn enqueue_chatterbox_invitation(&mut self, invitation: &Event) {
        if !matches!(invitation, Event::ConferenceInvited { .. }) {
            return;
        }
        self.enqueue_caps_event(
            "ChatterBoxInvitation",
            chatterbox_invitation_to_llsd(invitation),
        );
    }

    /// Enqueues a CAPS `ChatterBoxSessionAgentListUpdates` push: per-agent
    /// voice-membership changes in the chat session `session_id`. Each
    /// `(agent, in_voice_now)` pair emits an `ENTER` (voice-capable) or
    /// `LEAVE` transition — the shape
    /// [`agent_list_voice_updates_to_llsd`](crate::agent_list_voice_updates_to_llsd)
    /// serializes and the client's voice-participant handler folds.
    pub fn enqueue_chatterbox_agent_list_updates(
        &mut self,
        session_id: Uuid,
        updates: &[(AgentKey, bool)],
    ) {
        self.enqueue_caps_event(
            "ChatterBoxSessionAgentListUpdates",
            agent_list_voice_updates_to_llsd(session_id, updates),
        );
    }

    /// Replies to a finished (or refused) legacy asset upload with an
    /// `AssetUploadComplete`, the message the uploading client surfaces as
    /// [`Event::InventoryAssetSaved`](crate::Event::InventoryAssetSaved).
    fn send_asset_upload_complete(
        &mut self,
        asset_id: Uuid,
        asset_type: AssetType,
        success: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AssetUploadComplete(AssetUploadComplete {
            asset_block: AssetUploadCompleteAssetBlockBlock {
                uuid: asset_id,
                r#type: i8::try_from(asset_type.to_code()).unwrap_or_default(),
                success,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Sends a `UserInfoReply` — the agent's own account contact preferences, in
    /// reply to the client's `UserInfoRequest` (the inverse of the client's
    /// [`Event::UserInfo`](crate::Event::UserInfo)): whether offline IMs are
    /// forwarded to email, the agent's directory (search) visibility, and the
    /// email address on file. The echoed `AgentData.AgentID` is the recipient
    /// agent. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_user_info_reply(&mut self, info: &UserInfo, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::UserInfoReply(UserInfoReply {
            agent_data: UserInfoReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            user_data: UserInfoReplyUserDataBlock {
                im_via_e_mail: info.im_via_email,
                directory_visibility: info.directory_visibility.to_wire().as_bytes().to_vec(),
                e_mail: info.email.as_bytes().to_vec(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `DeRezAck` — acknowledges that a delayed derez succeeded with no
    /// inventory created on the viewer (e.g. a save into task inventory),
    /// correlated to the client's derez by its [`TransactionId`] (the inverse of
    /// the client's [`Event::DeRezAck`](crate::Event::DeRezAck)). Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_derez_ack(
        &mut self,
        transaction: TransactionId,
        success: bool,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::DeRezAck(DeRezAck {
            transaction_data: DeRezAckTransactionDataBlock {
                transaction_id: transaction.get(),
                success,
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `ForceObjectSelect` — forces the client's object selection to the
    /// given region-local object ids (the inverse of the client's
    /// [`Event::ForceObjectSelect`](crate::Event::ForceObjectSelect)). `reset_list`
    /// clears the client's current selection before applying these; the ids are
    /// region-local [`RegionLocalObjectId`]s, the bare counterpart of the
    /// [`ScopedObjectId`](crate::ScopedObjectId) the client scopes them to. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_force_object_select(
        &mut self,
        reset_list: bool,
        objects: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ForceObjectSelect(ForceObjectSelect {
            header: ForceObjectSelectHeaderBlock { reset_list },
            data: objects
                .iter()
                .map(|object| ForceObjectSelectDataBlock { local_id: object.0 })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `GrantGodlikePowers` — informs the client that the simulator
    /// granted (or, with `god_level` 0, revoked) its god-like powers (the inverse
    /// of the client's
    /// [`Event::GodlikePowersGranted`](crate::Event::GodlikePowersGranted)). The
    /// `AgentData` echoes the recipient agent; the wire `Token` is checked on the
    /// sim and ignored by the viewer, so a nil token is sent. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_grant_godlike_powers(&mut self, god_level: u8, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::GrantGodlikePowers(GrantGodlikePowers {
            agent_data: GrantGodlikePowersAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            grant_data: GrantGodlikePowersGrantDataBlock {
                god_level,
                token: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `StartPingCheck` to the client; the client answers with a
    /// `CompletePingCheck`. Returns the ping id sent (so a caller can match the
    /// reply), or `None` if the circuit is not open.
    ///
    /// # Errors
    ///
    /// Returns a wire error if the message fails to encode.
    pub fn start_ping_check(&mut self, now: Instant) -> Result<Option<PingId>, Error> {
        if self.client_addr.is_none() {
            return Ok(None);
        }
        let ping_id = self.next_ping_id;
        self.next_ping_id = self.next_ping_id.wrapping_next();
        let oldest_unacked = self
            .unacked
            .keys()
            .next()
            .copied()
            .map_or(0, SequenceNumber::get);
        let message = AnyMessage::StartPingCheck(StartPingCheck {
            ping_id: StartPingCheckPingIDBlock {
                ping_id: ping_id.get(),
                oldest_unacked,
            },
        });
        self.send(&message, Reliability::Unreliable, now)?;
        Ok(Some(ping_id))
    }

    // --- CAPS event-queue pushes (typed enqueue helpers) ---------------------
    //
    // The following helpers mirror client inbound EQ batches 1–3: each wraps
    // [`enqueue_caps_event`](Self::enqueue_caps_event) with the `*_to_llsd`
    // serializer that inverts the client's matching decoder in
    // `session/conversions.rs`. They are the server-side mirror of the events
    // the client decodes from its `EventQueueGet` long-poll.

    /// Enqueues a CAPS `AgentStateUpdate` push: whether the agent may currently
    /// rebake this region's navmesh. SL-only (OpenSim never pushes this).
    pub fn enqueue_agent_state_update(&mut self, can_modify_navmesh: bool) {
        self.enqueue_caps_event(
            "AgentStateUpdate",
            agent_state_update_to_llsd(can_modify_navmesh),
        );
    }

    /// Enqueues a CAPS `NavMeshStatusUpdate` push: the region's navmesh build
    /// state and version. SL-only.
    pub fn enqueue_nav_mesh_status(&mut self, status: &NavMeshStatus) {
        self.enqueue_caps_event("NavMeshStatusUpdate", nav_mesh_status_to_llsd(status));
    }

    /// Enqueues a CAPS `AgentDropGroup` push: the simulator removed this agent
    /// from `group`. The echoed `AgentID` is this session's agent.
    pub fn enqueue_agent_drop_group(&mut self, group: GroupKey) {
        let agent_id = self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
        self.enqueue_caps_event("AgentDropGroup", agent_drop_group_to_llsd(agent_id, group));
    }

    /// Enqueues a CAPS `DisplayNameUpdate` push: an avatar's display name
    /// changed. SL-only.
    pub fn enqueue_display_name_update(&mut self, update: &DisplayNameUpdate) {
        self.enqueue_caps_event("DisplayNameUpdate", display_name_update_to_llsd(update));
    }

    /// Enqueues a CAPS `SetDisplayNameReply` push: the result of this agent's
    /// own set-display-name request. SL-only.
    pub fn enqueue_set_display_name_reply(&mut self, reply: &SetDisplayNameReply) {
        self.enqueue_caps_event("SetDisplayNameReply", set_display_name_reply_to_llsd(reply));
    }

    /// Enqueues a CAPS `WindLightRefresh` push: asks the client to re-fetch the
    /// region's environment, interpolating the transition when `interpolate`.
    pub fn enqueue_windlight_refresh(&mut self, interpolate: bool) {
        self.enqueue_caps_event("WindLightRefresh", windlight_refresh_to_llsd(interpolate));
    }

    /// Enqueues a CAPS `SimConsoleResponse` push: the text output of a region
    /// debug-console command (a bare LLSD string body).
    pub fn enqueue_sim_console_response(&mut self, output: &str) {
        self.enqueue_caps_event("SimConsoleResponse", sim_console_response_to_llsd(output));
    }

    /// Enqueues a CAPS `RequiredVoiceVersion` push: the voice protocol version
    /// this region requires. SL-only.
    pub fn enqueue_required_voice_version(&mut self, version: &RequiredVoiceVersion) {
        self.enqueue_caps_event(
            "RequiredVoiceVersion",
            required_voice_version_to_llsd(version),
        );
    }

    /// Enqueues a CAPS `OpenRegionInfo` push: OpenSim's extended per-region
    /// settings/limits. OpenSim-only.
    pub fn enqueue_open_region_info(&mut self, info: &OpenRegionInfo) {
        self.enqueue_caps_event("OpenRegionInfo", open_region_info_to_llsd(info));
    }

    /// Enqueues a CAPS `EventQueueGet` event (a `{message, body}` pair) for the
    /// client to receive on its next long-poll. Drain the batch with
    /// [`SimSession::take_event_queue_response`]. The `*_to_llsd` serializers
    /// (e.g. [`enable_simulator_to_caps_llsd`](crate::enable_simulator_to_caps_llsd))
    /// build the `body` values.
    pub fn enqueue_caps_event(&mut self, message: impl Into<String>, body: Llsd) {
        self.caps_events.push(EventQueueEvent {
            message: message.into(),
            body,
        });
    }

    /// Whether any CAPS events are queued for the next long-poll.
    #[must_use]
    pub const fn has_caps_events(&self) -> bool {
        !self.caps_events.is_empty()
    }

    /// Drains the enqueued CAPS events into an `EventQueueGet` response body
    /// (the LLSD-XML the client's `EventQueueGet` long-poll parses), advancing
    /// the batch id, or returns `None` if no events are queued.
    pub fn take_event_queue_response(&mut self) -> Option<String> {
        if self.caps_events.is_empty() {
            return None;
        }
        let events = std::mem::take(&mut self.caps_events);
        let id = self.event_queue_id;
        self.event_queue_id = self.event_queue_id.wrapping_add(1);
        Some(build_event_queue_response(id, &events))
    }

    /// Records that a datagram was received, resetting the inactivity timer.
    fn note_received(&mut self, now: Instant) {
        self.inactivity = deadline(now, INACTIVITY_TIMEOUT);
    }

    /// Records that we owe an acknowledgement for `sequence`, arming the flush.
    fn queue_ack(&mut self, sequence: SequenceNumber, now: Instant) {
        self.pending_acks.push(sequence);
        if self.ack_flush.is_none() {
            self.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    fn record_acks(&mut self, ids: &[SequenceNumber]) {
        for id in ids {
            self.unacked.remove(id);
        }
    }

    /// Flushes owed acknowledgements as one or more `PacketAck` messages.
    fn flush_acks(&mut self, now: Instant) -> Result<(), WireError> {
        self.ack_flush = None;
        if self.pending_acks.is_empty() {
            return Ok(());
        }
        let acks = std::mem::take(&mut self.pending_acks);
        for chunk in acks.chunks(MAX_ACKS_PER_PACKET) {
            let packets = chunk
                .iter()
                .map(|id| sl_wire::messages::PacketAckPacketsBlock { id: id.get() })
                .collect();
            let message = AnyMessage::PacketAck(PacketAck { packets });
            self.send(&message, Reliability::Unreliable, now)?;
        }
        Ok(())
    }

    /// Retransmits unacknowledged reliable packets whose timeout has elapsed.
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

    /// Handles an inbound datagram from the client at address `from`.
    ///
    /// Parses the framing, records owed/received acknowledgements, decodes the
    /// carried message, and dispatches it: circuit-lifecycle messages are
    /// answered here and surfaced as [`ServerEvent`]s; everything else is decoded
    /// and surfaced. Traffic that arrives once the session is closed is ignored.
    ///
    /// # Errors
    ///
    /// Returns a wire error if the datagram framing is malformed.
    pub fn handle_datagram(
        &mut self,
        from: SocketAddr,
        datagram: &[u8],
        now: Instant,
    ) -> Result<(), Error> {
        if matches!(self.state, SimState::Closed) {
            return Ok(());
        }
        // Bind to the first client address seen; ignore traffic from any other.
        match self.client_addr {
            Some(addr) if addr != from => return Ok(()),
            _ => {}
        }

        let parsed = parse_datagram(datagram)?;
        self.client_addr = Some(from);
        self.note_received(now);
        self.record_acks(&parsed.acks);
        let process = if parsed.flags.contains(PacketFlags::RELIABLE) {
            self.queue_ack(parsed.sequence, now);
            self.seen.insert(parsed.sequence)
        } else {
            true
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
        self.dispatch(&message, now)
    }

    /// Dispatches a decoded client message: answers the circuit-lifecycle
    /// messages and surfaces a [`ServerEvent`] for each.
    fn dispatch(&mut self, message: &AnyMessage, now: Instant) -> Result<(), Error> {
        match message {
            AnyMessage::UseCircuitCode(use_circuit) => {
                let block = &use_circuit.circuit_code;
                self.agent_id = Some(AgentKey::from(block.id));
                self.session_id = Some(block.session_id);
                self.circuit_code = Some(CircuitCode(block.code));
                if matches!(self.state, SimState::AwaitingCircuit) {
                    self.state = SimState::Active;
                    self.ping = Some(deadline(now, PING_INTERVAL));
                }
                self.events.push_back(ServerEvent::CircuitOpened {
                    agent_id: AgentKey::from(block.id),
                    session_id: block.session_id,
                    circuit_code: CircuitCode(block.code),
                });
            }
            AnyMessage::CompleteAgentMovement(_) => {
                // The child agent becomes the root agent: login arrival, or a
                // teleport/crossing destination confirming the handover.
                self.agent_presence = AgentPresence::Root;
                self.send_agent_movement_complete(now)?;
                self.events.push_back(ServerEvent::AgentArrived);
            }
            AnyMessage::RegionHandshakeReply(_) => {
                self.events.push_back(ServerEvent::RegionHandshakeReplied);
            }
            AnyMessage::StartPingCheck(ping) => {
                let ping_id = PingId(ping.ping_id.ping_id);
                let reply = AnyMessage::CompletePingCheck(CompletePingCheck {
                    ping_id: CompletePingCheckPingIDBlock {
                        ping_id: ping_id.get(),
                    },
                });
                self.send(&reply, Reliability::Unreliable, now)?;
                self.events
                    .push_back(ServerEvent::PingRequested { ping_id });
            }
            // The client answering our periodic `StartPingCheck`; consumed.
            AnyMessage::CompletePingCheck(_) => {}
            AnyMessage::PacketAck(ack) => {
                let ids: Vec<SequenceNumber> = ack
                    .packets
                    .iter()
                    .map(|packet| SequenceNumber(packet.id))
                    .collect();
                self.record_acks(&ids);
            }
            AnyMessage::AgentThrottle(throttle) => {
                if let Some(decoded) = decode_throttle(&throttle.throttle.throttles) {
                    self.events.push_back(ServerEvent::Throttle(decoded));
                }
            }
            AnyMessage::AgentUpdate(update) => {
                let data = &update.agent_data;
                let controls = ControlFlags::from_bits(data.control_flags);
                self.events
                    .push_back(ServerEvent::AgentUpdate(Box::new(AgentUpdateInfo {
                        body_rotation: data.body_rotation.clone(),
                        head_rotation: data.head_rotation.clone(),
                        controls,
                        camera: Camera::new_unchecked(
                            data.camera_center.clone(),
                            data.camera_at_axis.clone(),
                            data.camera_left_axis.clone(),
                            data.camera_up_axis.clone(),
                        ),
                        far: data.far,
                        state: data.state,
                        flags: data.flags,
                    })));
                if controls.contains(ControlFlags::STAND_UP)
                    && !matches!(self.sit, SimSitState::NotSitting)
                {
                    self.sit = SimSitState::NotSitting;
                    self.events.push_back(ServerEvent::StoodUp);
                }
            }
            AnyMessage::AgentRequestSit(request) => {
                self.events.push_back(ServerEvent::SitRequested {
                    target: ObjectKey::from(request.target_object.target_id),
                    offset: request.target_object.offset.clone(),
                });
            }
            AnyMessage::AgentSit(_) => {
                let on = match self.sit {
                    SimSitState::ResponseSent { on } => {
                        self.sit = SimSitState::Seated { on };
                        Some(on)
                    }
                    SimSitState::NotSitting | SimSitState::Seated { .. } => None,
                };
                self.events.push_back(ServerEvent::SitConfirmed { on });
            }
            AnyMessage::ChatFromViewer(chat) => {
                self.events.push_back(ServerEvent::Chat {
                    message: trimmed_string(&chat.chat_data.message),
                    channel: ChatChannel(chat.chat_data.channel),
                    chat_type: ChatType::from_u8(chat.chat_data.r#type),
                });
            }
            AnyMessage::ImprovedInstantMessage(im) => {
                // The session dialogs get typed events and fold the session
                // registry (mirroring how the client routes them away from its
                // generic IM event); everything else stays the plain IM path.
                let block = &im.message_block;
                let sender = AgentKey::from(im.agent_data.agent_id);
                match ImDialog::from_u8(block.dialog) {
                    ImDialog::SessionGroupStart => {
                        let group_id = GroupKey::from(block.id);
                        let chat_session = self
                            .chat_sessions
                            .entry(ImSessionId::from(block.id))
                            .or_insert_with(|| SimChatSession {
                                kind: SimChatSessionKind::Group { group_id },
                                participants: BTreeSet::new(),
                                history: Vec::new(),
                            });
                        chat_session.participants.insert(sender);
                        self.events
                            .push_back(ServerEvent::GroupSessionStartRequested { group_id });
                    }
                    ImDialog::SessionConferenceStart => {
                        let session_id = ImSessionId::from(block.id);
                        let invitees: Vec<AgentKey> = unpack_uuids(&block.binary_bucket)
                            .into_iter()
                            .map(AgentKey::from)
                            .collect();
                        let chat_session =
                            self.chat_sessions.entry(session_id).or_insert_with(|| {
                                SimChatSession {
                                    kind: SimChatSessionKind::Conference,
                                    participants: BTreeSet::new(),
                                    history: Vec::new(),
                                }
                            });
                        chat_session.participants.insert(sender);
                        chat_session.participants.extend(invitees.iter().copied());
                        self.events
                            .push_back(ServerEvent::ConferenceStartRequested {
                                session_id,
                                invitees,
                                message: trimmed_string(&block.message),
                            });
                    }
                    ImDialog::SessionSend => {
                        let session_id = ImSessionId::from(block.id);
                        let message = trimmed_string(&block.message);
                        // A send into an unknown session surfaces but creates
                        // no state: membership is the simulator's call, and
                        // the driver polices it (deliberately NOT the client's
                        // lazy-open — the client trusts inbound traffic, the
                        // server must not).
                        if let Some(chat_session) = self.chat_sessions.get_mut(&session_id) {
                            chat_session.log(ServerHistoryMessage {
                                sender,
                                sender_name: trimmed_string(&block.from_agent_name),
                                text: message.clone(),
                                timestamp: None,
                            });
                        }
                        self.events.push_back(ServerEvent::SessionMessageSent {
                            session_id,
                            message,
                        });
                    }
                    ImDialog::SessionLeave => {
                        let session_id = ImSessionId::from(block.id);
                        if let Some(chat_session) = self.chat_sessions.get_mut(&session_id) {
                            chat_session.participants.remove(&sender);
                            if chat_session.participants.is_empty() {
                                self.chat_sessions.remove(&session_id);
                            }
                        }
                        self.events
                            .push_back(ServerEvent::SessionLeaveRequested { session_id });
                    }
                    _ => {
                        self.events.push_back(ServerEvent::InstantMessage(Box::new(
                            instant_message(&im.agent_data, block),
                        )));
                    }
                }
            }
            AnyMessage::AcceptFriendship(accept) => {
                self.events.push_back(ServerEvent::FriendshipAccepted {
                    transaction: TransactionId::from(accept.transaction_block.transaction_id),
                    calling_card_folders: accept
                        .folder_data
                        .iter()
                        .map(|folder| InventoryFolderKey::from(folder.folder_id))
                        .collect(),
                });
            }
            AnyMessage::DeclineFriendship(decline) => {
                self.events.push_back(ServerEvent::FriendshipDeclined {
                    transaction: TransactionId::from(decline.transaction_block.transaction_id),
                });
            }
            AnyMessage::TerminateFriendship(terminate) => {
                self.events
                    .push_back(ServerEvent::FriendshipTerminationRequested {
                        other: FriendKey::from(terminate.ex_block.other_id),
                    });
            }
            AnyMessage::GrantUserRights(grant) => {
                self.events.push_back(ServerEvent::UserRightsGranted {
                    rights: grant
                        .rights
                        .iter()
                        .map(|block| UserRightsEntry {
                            agent: FriendKey::from(block.agent_related),
                            rights: FriendRights(block.related_rights),
                        })
                        .collect(),
                });
            }
            AnyMessage::UUIDNameRequest(request) => {
                let ids = request
                    .uuid_name_block
                    .iter()
                    .map(|block| block.id)
                    .collect();
                self.events
                    .push_back(ServerEvent::AvatarNamesRequested(ids));
            }
            AnyMessage::UUIDGroupNameRequest(request) => {
                let ids = request
                    .uuid_name_block
                    .iter()
                    .map(|block| block.id)
                    .collect();
                self.events.push_back(ServerEvent::GroupNamesRequested(ids));
            }
            AnyMessage::ObjectAttach(attach) => {
                let (attachment_point, mode) =
                    AttachmentPoint::split_code(attach.agent_data.attachment_point);
                for object in &attach.object_data {
                    self.events.push_back(ServerEvent::AttachObject {
                        local_id: RegionLocalObjectId(object.object_local_id),
                        attachment_point,
                        mode,
                        rotation: object.rotation.clone(),
                    });
                }
            }
            AnyMessage::ObjectDetach(detach) => {
                let ids = detach
                    .object_data
                    .iter()
                    .map(|object| RegionLocalObjectId(object.object_local_id))
                    .collect();
                self.events.push_back(ServerEvent::DetachObjects(ids));
            }
            AnyMessage::ObjectDrop(drop) => {
                let ids = drop
                    .object_data
                    .iter()
                    .map(|object| RegionLocalObjectId(object.object_local_id))
                    .collect();
                self.events.push_back(ServerEvent::DropAttachments(ids));
            }
            AnyMessage::RemoveAttachment(remove) => {
                let (attachment_point, _add) =
                    AttachmentPoint::split_code(remove.attachment_block.attachment_point);
                self.events.push_back(ServerEvent::RemoveAttachment {
                    attachment_point,
                    item_id: remove.attachment_block.item_id,
                });
            }
            AnyMessage::RezSingleAttachmentFromInv(rez) => {
                let object = &rez.object_data;
                let (attachment_point, mode) = AttachmentPoint::split_code(object.attachment_pt);
                self.events
                    .push_back(ServerEvent::RezAttachment(Box::new(RezAttachment {
                        item_id: InventoryKey::from(object.item_id),
                        owner_id: object.owner_id,
                        attachment_point,
                        mode,
                        name: trimmed_string(&object.name),
                        description: trimmed_string(&object.description),
                    })));
            }
            AnyMessage::RezMultipleAttachmentsFromInv(rez) => {
                let attachments = rez
                    .object_data
                    .iter()
                    .map(|object| {
                        let (attachment_point, mode) =
                            AttachmentPoint::split_code(object.attachment_pt);
                        RezAttachment {
                            item_id: InventoryKey::from(object.item_id),
                            owner_id: object.owner_id,
                            attachment_point,
                            mode,
                            name: trimmed_string(&object.name),
                            description: trimmed_string(&object.description),
                        }
                    })
                    .collect();
                self.events.push_back(ServerEvent::RezAttachments {
                    compound_id: rez.header_data.compound_msg_id,
                    detach: DetachOrder::from_first_detach_all(rez.header_data.first_detach_all),
                    attachments,
                });
            }
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
                self.events.push_back(ServerEvent::ViewerEffect(effects));
            }
            AnyMessage::ActivateGestures(activate) => {
                let gestures = activate
                    .data
                    .iter()
                    .map(|block| GestureActivation {
                        item_id: InventoryKey::from(block.item_id),
                        asset_id: block.asset_id,
                    })
                    .collect();
                self.events
                    .push_back(ServerEvent::ActivateGestures { gestures });
            }
            AnyMessage::DeactivateGestures(deactivate) => {
                let item_ids = deactivate.data.iter().map(|block| block.item_id).collect();
                self.events
                    .push_back(ServerEvent::DeactivateGestures { item_ids });
            }
            AnyMessage::SetAlwaysRun(set) => {
                self.events.push_back(ServerEvent::SetAlwaysRun {
                    mode: MovementMode::from_always_run_flag(set.agent_data.always_run),
                });
            }
            AnyMessage::AgentPause(pause) => {
                self.events.push_back(ServerEvent::AgentPause {
                    serial_num: pause.agent_data.serial_num,
                });
            }
            AnyMessage::AgentResume(resume) => {
                self.events.push_back(ServerEvent::AgentResume {
                    serial_num: resume.agent_data.serial_num,
                });
            }
            AnyMessage::AgentFOV(fov) => {
                self.events.push_back(ServerEvent::AgentFov {
                    vertical_angle: fov.fov_block.vertical_angle,
                });
            }
            AnyMessage::AgentHeightWidth(size) => {
                self.events.push_back(ServerEvent::AgentHeightWidth {
                    height: size.height_width_block.height,
                    width: size.height_width_block.width,
                });
            }
            AnyMessage::ForceScriptControlRelease(_release) => {
                self.events
                    .push_back(ServerEvent::ForceScriptControlRelease);
            }
            AnyMessage::ScriptAnswerYes(answer) => {
                let task_id = ObjectKey::from(answer.data.task_id);
                let item_id = InventoryKey::from(answer.data.item_id);
                // The answer settles any outstanding question; an unsolicited
                // answer is still recorded — the mirror observes the agent's
                // stated answer, enforcement stays with the simulator.
                self.script_questions.remove(&(task_id, item_id));
                let permissions = ScriptPermissions(answer.data.questions);
                self.script_grants.insert((task_id, item_id), permissions);
                self.events.push_back(ServerEvent::ScriptPermissionAnswer {
                    task_id,
                    item_id,
                    permissions,
                });
            }
            AnyMessage::TrackAgent(track) => {
                self.events.push_back(ServerEvent::TrackAgent {
                    prey_id: AgentKey::from(track.target_data.prey_id),
                });
            }
            AnyMessage::FindAgent(find) => {
                self.events.push_back(ServerEvent::FindAgent {
                    hunter: find.agent_block.hunter,
                    prey: find.agent_block.prey,
                });
            }
            AnyMessage::DirFindQuery(query) => {
                self.events.push_back(ServerEvent::DirFindQuery {
                    query_id: query.query_data.query_id,
                    query_text: trimmed_string(&query.query_data.query_text),
                    flags: DirFindFlags::from_bits(query.query_data.query_flags),
                    query_start: query.query_data.query_start,
                });
            }
            AnyMessage::DirPlacesQuery(query) => {
                self.events.push_back(ServerEvent::DirPlacesQuery {
                    query_id: query.query_data.query_id,
                    query_text: trimmed_string(&query.query_data.query_text),
                    flags: DirFindFlags::from_bits(query.query_data.query_flags),
                    category: category_from_wire(query.query_data.category),
                    sim_name: trimmed_string(&query.query_data.sim_name),
                    query_start: query.query_data.query_start,
                });
            }
            AnyMessage::DirLandQuery(query) => {
                self.events.push_back(ServerEvent::DirLandQuery {
                    query_id: query.query_data.query_id,
                    flags: DirFindFlags::from_bits(query.query_data.query_flags),
                    search_type: LandSearchType::from_bits(query.query_data.search_type),
                    price: query.query_data.price,
                    area: query.query_data.area,
                    query_start: query.query_data.query_start,
                });
            }
            AnyMessage::DirClassifiedQuery(query) => {
                self.events.push_back(ServerEvent::DirClassifiedQuery {
                    query_id: query.query_data.query_id,
                    query_text: trimmed_string(&query.query_data.query_text),
                    flags: DirFindFlags::from_bits(query.query_data.query_flags),
                    category: ClassifiedCategory::from_u32(query.query_data.category),
                    query_start: query.query_data.query_start,
                });
            }
            AnyMessage::AvatarPickerRequest(request) => {
                self.events.push_back(ServerEvent::AvatarPickerRequest {
                    query_id: request.agent_data.query_id,
                    name: trimmed_string(&request.data.name),
                });
            }
            AnyMessage::PlacesQuery(query) => {
                self.events.push_back(ServerEvent::PlacesQuery {
                    query_id: query.agent_data.query_id,
                    transaction_id: query.transaction_data.transaction_id,
                    query_text: trimmed_string(&query.query_data.query_text),
                    flags: DirFindFlags::from_bits(query.query_data.query_flags),
                    category: category_from_wire(query.query_data.category),
                    sim_name: trimmed_string(&query.query_data.sim_name),
                });
            }
            AnyMessage::EventInfoRequest(request) => {
                self.events.push_back(ServerEvent::EventInfoRequest {
                    event_id: EventId::new(request.event_data.event_id),
                });
            }
            AnyMessage::EventNotificationAddRequest(request) => {
                self.events
                    .push_back(ServerEvent::EventNotificationAddRequest {
                        event_id: EventId::new(request.event_data.event_id),
                    });
            }
            AnyMessage::EventNotificationRemoveRequest(request) => {
                self.events
                    .push_back(ServerEvent::EventNotificationRemoveRequest {
                        event_id: EventId::new(request.event_data.event_id),
                    });
            }
            AnyMessage::ObjectBuy(buy) => {
                self.events.push_back(ServerEvent::BuyObject {
                    group_id: GroupKey::from(buy.agent_data.group_id),
                    category_id: buy.agent_data.category_id,
                    objects: buy
                        .object_data
                        .iter()
                        .map(|item| {
                            Ok(ObjectBuyItem {
                                local_id: RegionLocalObjectId(item.object_local_id),
                                sale_type: SaleType::from_code(item.sale_type),
                                sale_price: crate::types::linden_from_wire(
                                    "SalePrice",
                                    item.sale_price,
                                )?,
                            })
                        })
                        .collect::<Result<_, sl_wire::WireError>>()?,
                });
            }
            AnyMessage::BuyObjectInventory(buy) => {
                self.events.push_back(ServerEvent::BuyObjectInventory {
                    object_id: ObjectKey::from(buy.data.object_id),
                    item_id: buy.data.item_id,
                    folder_id: buy.data.folder_id,
                });
            }
            AnyMessage::RequestPayPrice(request) => {
                self.events.push_back(ServerEvent::RequestPayPrice {
                    object_id: ObjectKey::from(request.object_data.object_id),
                });
            }
            AnyMessage::RequestObjectPropertiesFamily(request) => {
                self.events
                    .push_back(ServerEvent::RequestObjectPropertiesFamily {
                        request_flags: request.object_data.request_flags,
                        object_id: ObjectKey::from(request.object_data.object_id),
                    });
            }
            AnyMessage::ObjectSpinStart(spin) => {
                self.events.push_back(ServerEvent::SpinObjectStart {
                    object_id: ObjectKey::from(spin.object_data.object_id),
                });
            }
            AnyMessage::ObjectSpinUpdate(spin) => {
                self.events.push_back(ServerEvent::SpinObjectUpdate {
                    object_id: ObjectKey::from(spin.object_data.object_id),
                    rotation: spin.object_data.rotation.clone(),
                });
            }
            AnyMessage::ObjectSpinStop(spin) => {
                self.events.push_back(ServerEvent::SpinObjectStop {
                    object_id: ObjectKey::from(spin.object_data.object_id),
                });
            }
            AnyMessage::ObjectDuplicateOnRay(dup) => {
                let agent = &dup.agent_data;
                self.events.push_back(ServerEvent::DuplicateObjectsOnRay {
                    local_ids: dup
                        .object_data
                        .iter()
                        .map(|item| RegionLocalObjectId(item.object_local_id))
                        .collect(),
                    group_id: crate::types::optional_key_from_wire(agent.group_id),
                    ray_start: agent.ray_start.clone(),
                    ray_end: agent.ray_end.clone(),
                    bypass_raycast: agent.bypass_raycast,
                    ray_end_is_intersection: agent.ray_end_is_intersection,
                    copy_centers: agent.copy_centers,
                    copy_rotates: agent.copy_rotates,
                    ray_target_id: crate::types::optional_key_from_wire(agent.ray_target_id),
                    duplicate_flags: agent.duplicate_flags,
                });
            }
            AnyMessage::RezRestoreToWorld(restore) => {
                self.events.push_back(ServerEvent::RezRestoreToWorld {
                    item: restore_item_from_inventory_block!(&restore.inventory_data),
                });
            }
            AnyMessage::RezObjectFromNotecard(rez) => {
                let rez_data = &rez.rez_data;
                self.events.push_back(ServerEvent::RezObjectFromNotecard {
                    rez: NotecardRez {
                        group_id: crate::types::optional_key_from_wire(rez.agent_data.group_id),
                        from_task_id: crate::types::optional_key_from_wire(rez_data.from_task_id),
                        bypass_raycast: rez_data.bypass_raycast != 0,
                        ray_start: rez_data.ray_start.clone(),
                        ray_end: rez_data.ray_end.clone(),
                        ray_target_id: crate::types::optional_key_from_wire(rez_data.ray_target_id),
                        ray_end_is_intersection: rez_data.ray_end_is_intersection,
                        rez_selected: rez_data.rez_selected,
                        remove_item: rez_data.remove_item,
                        item_flags: rez_data.item_flags,
                        group_mask: rez_data.group_mask,
                        everyone_mask: rez_data.everyone_mask,
                        next_owner_mask: rez_data.next_owner_mask,
                        notecard_item_id: InventoryKey::from(rez.notecard_data.notecard_item_id),
                        object_id: ObjectKey::from(rez.notecard_data.object_id),
                        item_ids: rez
                            .inventory_data
                            .iter()
                            .map(|item| InventoryKey::from(item.item_id))
                            .collect(),
                    },
                });
            }
            AnyMessage::ParcelJoin(join) => {
                let data = &join.parcel_data;
                self.events.push_back(ServerEvent::JoinParcels {
                    west: data.west,
                    south: data.south,
                    east: data.east,
                    north: data.north,
                });
            }
            AnyMessage::ParcelDivide(divide) => {
                let data = &divide.parcel_data;
                self.events.push_back(ServerEvent::DivideParcel {
                    west: data.west,
                    south: data.south,
                    east: data.east,
                    north: data.north,
                });
            }
            AnyMessage::ParcelObjectOwnersRequest(request) => {
                self.events
                    .push_back(ServerEvent::RequestParcelObjectOwners {
                        local_id: RegionLocalParcelId(request.parcel_data.local_id),
                    });
            }
            AnyMessage::ParcelBuyPass(pass) => {
                self.events.push_back(ServerEvent::BuyParcelPass {
                    local_id: RegionLocalParcelId(pass.parcel_data.local_id),
                });
            }
            AnyMessage::ParcelDisableObjects(disable) => {
                self.events.push_back(ServerEvent::DisableParcelObjects {
                    local_id: RegionLocalParcelId(disable.parcel_data.local_id),
                    return_type: disable.parcel_data.return_type,
                    owner_ids: disable
                        .owner_i_ds
                        .iter()
                        .map(|owner| owner.owner_id)
                        .collect(),
                    task_ids: disable
                        .task_i_ds
                        .iter()
                        .map(|task| ObjectKey::from(task.task_id))
                        .collect(),
                });
            }
            AnyMessage::ParcelInfoRequest(request) => {
                self.events.push_back(ServerEvent::RequestParcelInfo {
                    parcel_id: ParcelKey::from(request.data.parcel_id),
                });
            }
            AnyMessage::GetScriptRunning(request) => {
                self.events.push_back(ServerEvent::RequestScriptRunning {
                    object_id: ObjectKey::from(request.script.object_id),
                    item_id: request.script.item_id,
                });
            }
            AnyMessage::SetScriptRunning(request) => {
                self.events.push_back(ServerEvent::SetScriptRunning {
                    object_id: ObjectKey::from(request.script.object_id),
                    item_id: request.script.item_id,
                    running: request.script.running,
                });
            }
            AnyMessage::ScriptReset(request) => {
                self.events.push_back(ServerEvent::ResetScript {
                    object_id: ObjectKey::from(request.script.object_id),
                    item_id: request.script.item_id,
                });
            }
            AnyMessage::GroupAccountSummaryRequest(request) => {
                self.events
                    .push_back(ServerEvent::RequestGroupAccountSummary {
                        group_id: GroupKey::from(request.agent_data.group_id),
                        request_id: request.money_data.request_id,
                        interval_days: request.money_data.interval_days,
                        current_interval: request.money_data.current_interval,
                    });
            }
            AnyMessage::GroupAccountDetailsRequest(request) => {
                self.events
                    .push_back(ServerEvent::RequestGroupAccountDetails {
                        group_id: GroupKey::from(request.agent_data.group_id),
                        request_id: request.money_data.request_id,
                        interval_days: request.money_data.interval_days,
                        current_interval: request.money_data.current_interval,
                    });
            }
            AnyMessage::GroupAccountTransactionsRequest(request) => {
                self.events
                    .push_back(ServerEvent::RequestGroupAccountTransactions {
                        group_id: GroupKey::from(request.agent_data.group_id),
                        request_id: request.money_data.request_id,
                        interval_days: request.money_data.interval_days,
                        current_interval: request.money_data.current_interval,
                    });
            }
            AnyMessage::GroupActiveProposalsRequest(request) => {
                self.events
                    .push_back(ServerEvent::RequestGroupActiveProposals {
                        group_id: GroupKey::from(request.group_data.group_id),
                        transaction_id: request.transaction_data.transaction_id,
                    });
            }
            AnyMessage::GroupVoteHistoryRequest(request) => {
                self.events.push_back(ServerEvent::RequestGroupVoteHistory {
                    group_id: GroupKey::from(request.group_data.group_id),
                    transaction_id: request.transaction_data.transaction_id,
                });
            }
            AnyMessage::StartGroupProposal(request) => {
                self.events.push_back(ServerEvent::StartGroupProposal {
                    group_id: GroupKey::from(request.proposal_data.group_id),
                    quorum: request.proposal_data.quorum,
                    majority: request.proposal_data.majority,
                    duration: request.proposal_data.duration,
                    proposal_text: trimmed_string(&request.proposal_data.proposal_text),
                });
            }
            AnyMessage::GroupProposalBallot(request) => {
                self.events.push_back(ServerEvent::GroupProposalBallot {
                    proposal_id: ProposalVoteId::from(request.proposal_data.proposal_id),
                    group_id: GroupKey::from(request.proposal_data.group_id),
                    vote_cast: trimmed_string(&request.proposal_data.vote_cast),
                });
            }
            AnyMessage::EstateCovenantRequest(_) => {
                self.events.push_back(ServerEvent::RequestEstateCovenant);
            }
            AnyMessage::EstateOwnerMessage(message) => {
                // Estate commands: the telehub and terrain methods decode to
                // typed events; everything else (and an unknown sub-command
                // of those two) surfaces raw so no estate command is dropped.
                let method = trimmed_string(&message.method_data.method);
                let typed = match method.as_str() {
                    "telehub" => telehub_server_event(&message.param_list),
                    "terrain" => terrain_server_event(&message.param_list),
                    _other => None,
                };
                self.events.push_back(typed.unwrap_or_else(|| {
                    ServerEvent::EstateOwnerRequest {
                        method,
                        invoice: message.method_data.invoice,
                        params: message
                            .param_list
                            .iter()
                            .map(|block| trimmed_string(&block.parameter))
                            .collect(),
                    }
                }));
            }
            AnyMessage::UserReport(report) => {
                let data = &report.report_data;
                self.events
                    .push_back(ServerEvent::AbuseReportReceived(Box::new(AbuseReport {
                        report_type: sl_wire::AbuseReportType::from_u8(data.report_type),
                        category: data.category,
                        position: data.position.clone(),
                        check_flags: data.check_flags,
                        screenshot_id: data.screenshot_id,
                        object_id: ObjectKey::from(data.object_id),
                        abuser_id: data.abuser_id,
                        abuse_region_name: sl_wire::region_name_from_wire(
                            "abuse-region-name",
                            &trimmed_string(&data.abuse_region_name),
                        )?,
                        abuse_region_id: data.abuse_region_id,
                        summary: trimmed_string(&data.summary),
                        details: trimmed_string(&data.details),
                        version_string: trimmed_string(&data.version_string),
                    })));
            }
            AnyMessage::SendPostcard(postcard) => {
                let data = &postcard.agent_data;
                let [pos_x, pos_y, pos_z] = data.pos_global;
                self.events
                    .push_back(ServerEvent::PostcardReceived(Box::new(Postcard {
                        asset_id: data.asset_id,
                        pos_global: GlobalCoordinates::new(pos_x, pos_y, pos_z),
                        to: trimmed_string(&data.to),
                        from: trimmed_string(&data.from),
                        name: trimmed_string(&data.name),
                        subject: trimmed_string(&data.subject),
                        message: trimmed_string(&data.msg),
                        allow_publish: data.allow_publish,
                        mature_publish: data.mature_publish,
                    })));
            }
            AnyMessage::MapBlockRequest(request) => {
                let position = &request.position_data;
                self.events.push_back(ServerEvent::MapBlockRequested {
                    min_x: position.min_x,
                    max_x: position.max_x,
                    min_y: position.min_y,
                    max_y: position.max_y,
                    flags: MapRequestFlags(request.agent_data.flags),
                });
            }
            AnyMessage::MapNameRequest(request) => {
                self.events.push_back(ServerEvent::MapNameRequested {
                    name: trimmed_string(&request.name_data.name),
                    flags: MapRequestFlags(request.agent_data.flags),
                });
            }
            AnyMessage::MapItemRequest(request) => {
                self.events.push_back(ServerEvent::MapItemRequested {
                    item_type: MapItemType::from_u32(request.request_data.item_type),
                    region_handle: RegionHandle(request.request_data.region_handle),
                    flags: MapRequestFlags(request.agent_data.flags),
                });
            }
            AnyMessage::MapLayerRequest(request) => {
                self.events.push_back(ServerEvent::MapLayerRequested {
                    flags: MapRequestFlags(request.agent_data.flags),
                });
            }
            AnyMessage::OfferCallingCard(offer) => {
                self.events.push_back(ServerEvent::CallingCardOffered {
                    dest: AgentKey::from(offer.agent_block.dest_id),
                    transaction: TransactionId::from(offer.agent_block.transaction_id),
                });
            }
            AnyMessage::AcceptCallingCard(accept) => {
                let folder = accept
                    .folder_data
                    .first()
                    .map_or_else(Uuid::nil, |block| block.folder_id);
                self.events.push_back(ServerEvent::CallingCardAccepted {
                    transaction: TransactionId::from(accept.transaction_block.transaction_id),
                    folder: InventoryFolderKey::from(folder),
                });
            }
            AnyMessage::DeclineCallingCard(decline) => {
                self.events.push_back(ServerEvent::CallingCardDeclined {
                    transaction: TransactionId::from(decline.transaction_block.transaction_id),
                });
            }
            AnyMessage::ObjectShape(shape) => {
                for block in &shape.object_data {
                    self.events.push_back(ServerEvent::ObjectShapeSet {
                        local_id: RegionLocalObjectId(block.object_local_id),
                        shape: shape_from_object_shape_block(block),
                    });
                }
            }
            AnyMessage::ObjectImage(image) => {
                for block in &image.object_data {
                    let media_url = trimmed_string(&block.media_url);
                    self.events.push_back(ServerEvent::ObjectImageSet {
                        local_id: RegionLocalObjectId(block.object_local_id),
                        media_url: (!media_url.is_empty()).then_some(media_url),
                        texture_entry: decode_texture_entry(&block.texture_entry, MAX_FACES),
                    });
                }
            }
            AnyMessage::ObjectExtraParams(params) => {
                // The viewer's sendExtraParameters emits one block per subtype for
                // a single object, so collect the distinct object ids (in
                // first-seen order) and fold each object's blocks back into one
                // ObjectExtraParams.
                let mut order: Vec<RegionLocalObjectId> = Vec::new();
                for block in &params.object_data {
                    let id = RegionLocalObjectId(block.object_local_id);
                    if !order.contains(&id) {
                        order.push(id);
                    }
                }
                for local_id in order {
                    let blocks = params
                        .object_data
                        .iter()
                        .filter(|block| RegionLocalObjectId(block.object_local_id) == local_id)
                        .map(|block| {
                            (
                                block.param_type,
                                block.param_in_use,
                                block.param_data.clone(),
                            )
                        });
                    self.events.push_back(ServerEvent::ObjectExtraParamsSet {
                        local_id,
                        params: decode_extra_param_blocks(blocks),
                    });
                }
            }
            AnyMessage::RezObject(rez) => {
                let rez_data = &rez.rez_data;
                self.events.push_back(ServerEvent::RezObjectFromInventory {
                    params: RezObjectParams {
                        group_id: crate::types::optional_key_from_wire(rez.agent_data.group_id),
                        from_task_id: crate::types::optional_key_from_wire(rez_data.from_task_id),
                        bypass_raycast: rez_data.bypass_raycast != 0,
                        ray_start: rez_data.ray_start.clone(),
                        ray_end: rez_data.ray_end.clone(),
                        ray_target_id: crate::types::optional_key_from_wire(rez_data.ray_target_id),
                        ray_end_is_intersection: rez_data.ray_end_is_intersection,
                        rez_selected: rez_data.rez_selected,
                        remove_item: rez_data.remove_item,
                        item_flags: rez_data.item_flags,
                        group_mask: rez_data.group_mask,
                        everyone_mask: rez_data.everyone_mask,
                        next_owner_mask: rez_data.next_owner_mask,
                        item: restore_item_from_inventory_block!(&rez.inventory_data),
                    },
                });
            }
            AnyMessage::RezScript(rez) => {
                self.events.push_back(ServerEvent::RezScript {
                    local_id: RegionLocalObjectId(rez.update_block.object_local_id),
                    params: RezScriptParams {
                        group_id: crate::types::optional_key_from_wire(rez.agent_data.group_id),
                        enabled: rez.update_block.enabled,
                        item: restore_item_from_inventory_block!(&rez.inventory_block),
                    },
                });
            }
            AnyMessage::RevokePermissions(revoke) => {
                self.events.push_back(ServerEvent::RevokeScriptPermissions {
                    object_id: ObjectKey::from(revoke.data.object_id),
                    permissions: ScriptPermissions(revoke.data.object_permissions.cast_signed()),
                });
            }
            AnyMessage::DetachAttachmentIntoInv(detach) => {
                self.events
                    .push_back(ServerEvent::DetachAttachmentIntoInventory {
                        item_id: InventoryKey::from(detach.object_data.item_id),
                    });
            }
            AnyMessage::RequestTaskInventory(request) => {
                self.events.push_back(ServerEvent::RequestTaskInventory {
                    local_id: RegionLocalObjectId(request.inventory_data.local_id),
                });
            }
            AnyMessage::UpdateTaskInventory(update) => {
                self.events.push_back(ServerEvent::UpdateTaskInventory {
                    local_id: RegionLocalObjectId(update.update_data.local_id),
                    key: TaskInventoryKey::from_code(update.update_data.key),
                    item: restore_item_from_inventory_block!(&update.inventory_data),
                });
            }
            AnyMessage::MoveTaskInventory(move_item) => {
                self.events.push_back(ServerEvent::MoveTaskInventory {
                    local_id: RegionLocalObjectId(move_item.inventory_data.local_id),
                    folder_id: InventoryFolderKey::from(move_item.agent_data.folder_id),
                    item_id: InventoryKey::from(move_item.inventory_data.item_id),
                });
            }
            AnyMessage::RemoveTaskInventory(remove) => {
                self.events.push_back(ServerEvent::RemoveTaskInventory {
                    local_id: RegionLocalObjectId(remove.inventory_data.local_id),
                    item_id: InventoryKey::from(remove.inventory_data.item_id),
                });
            }
            AnyMessage::RequestXfer(request) => {
                // The client asks to download a file by name. Only registered
                // files are served; the client picked the transfer id. An
                // unknown name is refused with an `AbortXfer` so the requester
                // is not left hanging.
                let xfer_id = XferId(request.xfer_id.id);
                let filename = trimmed_string(&request.xfer_id.filename);
                let served = if let Some(data) = self.xfer_files.remove(&filename) {
                    self.xfer_sends.insert(
                        xfer_id,
                        SimXferSend {
                            filename: filename.clone(),
                            data,
                            sent: 0,
                            next_sequence: 0,
                            last_sent: false,
                        },
                    );
                    self.send_next_xfer_send_packet(xfer_id, now)?;
                    true
                } else {
                    let abort = AnyMessage::AbortXfer(AbortXfer {
                        xfer_id: AbortXferXferIDBlock {
                            id: xfer_id.get(),
                            // The reference `LL_ERR_ASSET_REQUEST_FAILED`.
                            result: -1,
                        },
                    });
                    self.send(&abort, Reliability::Reliable, now)?;
                    false
                };
                self.events.push_back(ServerEvent::XferRequested {
                    xfer_id,
                    filename,
                    served,
                });
            }
            AnyMessage::ConfirmXferPacket(confirm) => {
                // The client confirmed the packet we last sent for an outbound
                // file send; release the next one, or finish if that was the
                // final packet (strictly one packet in flight).
                let xfer_id = XferId(confirm.xfer_id.id);
                if let Some(send) = self.xfer_sends.get(&xfer_id) {
                    if send.last_sent {
                        if let Some(send) = self.xfer_sends.remove(&xfer_id) {
                            self.events.push_back(ServerEvent::XferServed {
                                xfer_id,
                                filename: send.filename,
                                byte_count: send.data.len(),
                            });
                        }
                    } else {
                        self.send_next_xfer_send_packet(xfer_id, now)?;
                    }
                }
            }
            AnyMessage::AssetUploadRequest(request) => {
                // The legacy transaction asset upload (the in-place wearable
                // save). Small assets arrive inline; an oversized one sent an
                // empty `AssetData`, and we pull it from the client over
                // `Xfer` by its predicted `VFileID`.
                let block = &request.asset_block;
                let transaction_id = TransactionId::new(block.transaction_id);
                let asset_type = AssetType::from_code(i32::from(block.r#type));
                let inline = !block.asset_data.is_empty();
                self.events.push_back(ServerEvent::AssetUploadRequested {
                    transaction_id,
                    asset_type,
                    inline,
                    tempfile: block.tempfile,
                    store_local: block.store_local,
                });
                if let Some(secure) = self.secure_session_id {
                    let asset_id = combine_uuids(block.transaction_id, secure);
                    if inline {
                        self.send_asset_upload_complete(asset_id, asset_type, true, now)?;
                        self.events.push_back(ServerEvent::AssetUploaded {
                            asset_id: AssetKey::from(asset_id),
                            asset_type,
                            transaction_id,
                            data: block.asset_data.clone(),
                        });
                    } else {
                        let xfer_id = self.alloc_xfer_id();
                        self.xfer_receives.insert(
                            xfer_id,
                            SimXferReceive {
                                purpose: SimXferReceivePurpose::AssetUpload {
                                    asset_id,
                                    asset_type,
                                    transaction_id,
                                },
                                buffer: Vec::new(),
                            },
                        );
                        let pull = AnyMessage::RequestXfer(RequestXfer {
                            xfer_id: RequestXferXferIDBlock {
                                id: xfer_id.get(),
                                filename: Vec::new(),
                                file_path: 0,
                                delete_on_completion: false,
                                use_big_packets: false,
                                v_file_id: asset_id,
                                v_file_type: i16::from(block.r#type),
                            },
                        });
                        self.send(&pull, Reliability::Reliable, now)?;
                    }
                } else {
                    // Without the secure session id the stored asset id cannot
                    // be derived; refuse so the client's save does not hang.
                    self.send_asset_upload_complete(Uuid::nil(), asset_type, false, now)?;
                }
            }
            AnyMessage::SendXferPacket(packet) => {
                // A chunk of an oversized asset upload we are pulling from the
                // client — the mirror of the client's download handler: strip
                // the seq-0 length prefix, confirm every packet, finish on the
                // high-bit end-of-file marker.
                let xfer_id = XferId(packet.xfer_id.id);
                let packet_id = XferPacketId::from_raw(packet.xfer_id.packet);
                if self.xfer_receives.contains_key(&xfer_id) {
                    let chunk = decode_xfer_chunk(packet_id, &packet.data_packet.data);
                    if let Some(receive) = self.xfer_receives.get_mut(&xfer_id) {
                        receive.buffer.extend_from_slice(chunk.payload);
                    }
                    let confirm = AnyMessage::ConfirmXferPacket(ConfirmXferPacket {
                        xfer_id: ConfirmXferPacketXferIDBlock {
                            id: xfer_id.get(),
                            packet: packet_id.raw(),
                        },
                    });
                    self.send(&confirm, Reliability::Reliable, now)?;
                    if packet_id.is_last()
                        && let Some(receive) = self.xfer_receives.remove(&xfer_id)
                    {
                        match receive.purpose {
                            SimXferReceivePurpose::AssetUpload {
                                asset_id,
                                asset_type,
                                transaction_id,
                            } => {
                                self.send_asset_upload_complete(asset_id, asset_type, true, now)?;
                                self.events.push_back(ServerEvent::AssetUploaded {
                                    asset_id: AssetKey::from(asset_id),
                                    asset_type,
                                    transaction_id,
                                    data: receive.buffer,
                                });
                            }
                            SimXferReceivePurpose::NamedFile { filename } => {
                                self.events.push_back(ServerEvent::XferReceived {
                                    xfer_id,
                                    filename,
                                    data: receive.buffer,
                                });
                            }
                        }
                    }
                }
            }
            AnyMessage::AbortXfer(abort) => {
                // The client aborted an in-flight transfer in either direction;
                // drop the state and surface the reason.
                let xfer_id = XferId(abort.xfer_id.id);
                let aborted = self.xfer_sends.remove(&xfer_id).is_some()
                    || self.xfer_receives.remove(&xfer_id).is_some();
                if aborted {
                    self.events.push_back(ServerEvent::XferAborted {
                        xfer_id,
                        result: abort.xfer_id.result,
                    });
                }
            }
            AnyMessage::TransferRequest(request) => {
                // A legacy UDP asset Transfer download. Only the two source
                // types with no HTTP alternative on either grid are served
                // (task-inventory item asset, estate asset). The plain
                // asset-by-id source is the ViewerAsset-superseded legacy path:
                // refused as unknown per the legacy-skip rule, but surfaced so
                // a driver can see a client still trying it. Garbage sources
                // are refused silently.
                let block = &request.transfer_info;
                let transfer_id = TransferId::new(block.transfer_id);
                let source = match block.source_type {
                    TRANSFER_SOURCE_SIM_INV_ITEM => {
                        TransferSourceParamsInvItem::decode(&block.params)
                            .ok()
                            .map(TransferRequestSource::TaskInventoryItem)
                    }
                    TRANSFER_SOURCE_SIM_ESTATE => TransferSourceParamsEstate::decode(&block.params)
                        .ok()
                        .map(TransferRequestSource::Estate),
                    TRANSFER_SOURCE_ASSET => {
                        self.events
                            .push_back(ServerEvent::LegacyAssetTransferRefused {
                                transfer_id,
                                params: TransferSourceParamsAsset::decode(&block.params).ok(),
                            });
                        None
                    }
                    _unknown => None,
                };
                if let Some(source) = source {
                    let _prev = self
                        .transfer_serves
                        .insert(transfer_id, block.params.clone());
                    self.events.push_back(ServerEvent::TransferRequested {
                        transfer_id,
                        priority: block.priority,
                        source,
                    });
                } else {
                    let refuse = AnyMessage::TransferInfo(TransferInfo {
                        transfer_info: TransferInfoTransferInfoBlock {
                            transfer_id: block.transfer_id,
                            channel_type: TRANSFER_CHANNEL_ASSET,
                            target_type: 0,
                            status: TransferStatus::UnknownSource.to_code(),
                            size: 0,
                            params: block.params.clone(),
                        },
                    });
                    self.send(&refuse, Reliability::Reliable, now)?;
                }
            }
            AnyMessage::TransferAbort(abort) => {
                let transfer_id = TransferId::new(abort.transfer_info.transfer_id);
                if self.transfer_serves.remove(&transfer_id).is_some() {
                    self.events
                        .push_back(ServerEvent::TransferAborted { transfer_id });
                }
            }
            AnyMessage::ModifyLand(modify) => {
                let block = &modify.modify_block;
                // Prefer the authoritative metre radius from the extended block,
                // falling back to the deprecated legacy index byte, then the
                // default brush for an unrecognised value.
                let brush_size = modify
                    .modify_block_extended
                    .first()
                    .and_then(|extended| LandBrushSize::from_metres(extended.brush_size))
                    .or_else(|| LandBrushSize::from_index(block.brush_size))
                    .unwrap_or_default();
                // The viewer sends exactly one ParcelData block; treat a missing
                // block as a free brush stroke at the region origin.
                let (parcel, area) = modify.parcel_data.first().map_or_else(
                    || (None, TerraformArea::point(0.0, 0.0)),
                    |data| {
                        let parcel =
                            (data.local_id >= 0).then_some(RegionLocalParcelId(data.local_id));
                        let area = TerraformArea::new(data.west, data.south, data.east, data.north);
                        (parcel, area)
                    },
                );
                self.events.push_back(ServerEvent::ModifyLand {
                    edit: LandEdit {
                        action: LandBrushAction::from_code(block.action).unwrap_or_default(),
                        brush_size,
                        strength: block.seconds,
                        height: block.height,
                        parcel,
                        area,
                    },
                });
            }
            AnyMessage::UndoLand(_) => {
                self.events.push_back(ServerEvent::UndoLand);
            }
            AnyMessage::ParcelPropertiesRequestByID(request) => {
                self.events
                    .push_back(ServerEvent::RequestParcelPropertiesById {
                        local_id: RegionLocalParcelId(request.parcel_data.local_id),
                        sequence_id: request.parcel_data.sequence_id,
                    });
            }
            AnyMessage::ParcelPropertiesRequest(request) => {
                let data = &request.parcel_data;
                self.events.push_back(ServerEvent::RequestParcelProperties {
                    west: data.west,
                    south: data.south,
                    east: data.east,
                    north: data.north,
                    sequence_id: data.sequence_id,
                    snap_selection: data.snap_selection,
                });
            }
            AnyMessage::RequestMultipleObjects(request) => {
                self.events.push_back(ServerEvent::RequestObjects {
                    objects: request
                        .object_data
                        .iter()
                        .map(|block| (RegionLocalObjectId(block.id), block.cache_miss_type))
                        .collect(),
                });
            }
            AnyMessage::ParcelSetOtherCleanTime(set) => {
                let minutes = u64::try_from(set.parcel_data.other_clean_time).unwrap_or(0);
                self.events.push_back(ServerEvent::SetParcelOtherCleanTime {
                    local_id: RegionLocalParcelId(set.parcel_data.local_id),
                    clean_time: std::time::Duration::from_secs(minutes.saturating_mul(60)),
                });
            }
            AnyMessage::LinkInventoryItem(link) => {
                // The link target's discriminator is its AssetType: AT_LINK_FOLDER
                // (25) is a folder link, any other value an item link (AT_LINK is
                // 24). The wire carries only the OldItemID, so this byte is the
                // sole signal for item vs folder.
                const AT_LINK_FOLDER: i8 = 25;
                let block = &link.inventory_block;
                let linked_id = if block.r#type == AT_LINK_FOLDER {
                    InventoryItemOrFolderKey::Folder(InventoryFolderKey::from(block.old_item_id))
                } else {
                    InventoryItemOrFolderKey::Item(InventoryKey::from(block.old_item_id))
                };
                self.events.push_back(ServerEvent::LinkInventoryItem {
                    link: NewInventoryLink {
                        folder_id: InventoryFolderKey::from(block.folder_id),
                        linked_id,
                        link_type: AssetType::from_code(i32::from(block.r#type)),
                        inv_type: InventoryType::from_code(i32::from(block.inv_type)),
                        name: trimmed_string(&block.name),
                        description: trimmed_string(&block.description),
                    },
                    callback_id: block.callback_id,
                });
            }
            AnyMessage::UpdateGroupInfo(update) => {
                let group = &update.group_data;
                let insignia_id =
                    (group.insignia_id != Uuid::nil()).then(|| TextureKey::from(group.insignia_id));
                self.events.push_back(ServerEvent::UpdateGroupInfo {
                    params: UpdateGroupInfoParams {
                        group_id: GroupKey::from(group.group_id),
                        charter: trimmed_string(&group.charter),
                        show_in_list: group.show_in_list,
                        insignia_id,
                        membership_fee: crate::types::linden_from_wire(
                            "MembershipFee",
                            group.membership_fee,
                        )?,
                        open_enrollment: group.open_enrollment,
                        allow_publish: group.allow_publish,
                        mature_publish: group.mature_publish,
                    },
                });
            }
            AnyMessage::GroupTitleUpdate(update) => {
                self.events.push_back(ServerEvent::UpdateGroupTitle {
                    group_id: GroupKey::from(update.agent_data.group_id),
                    title_role_id: GroupRoleKey::from(update.agent_data.title_role_id),
                });
            }
            AnyMessage::TeleportLandmarkRequest(request) => {
                // A nil LandmarkID is the wire encoding of "teleport home".
                let landmark_id = request.info.landmark_id;
                let landmark = (landmark_id != Uuid::nil()).then(|| AssetKey::from(landmark_id));
                self.events
                    .push_back(ServerEvent::TeleportViaLandmark { landmark });
            }
            AnyMessage::TeleportLocationRequest(request) => {
                let info = &request.info;
                self.events.push_back(ServerEvent::TeleportRequested {
                    region_handle: RegionHandle(info.region_handle),
                    position: RegionCoordinates::new(
                        info.position.x,
                        info.position.y,
                        info.position.z,
                    ),
                    look_at: info.look_at.clone(),
                });
            }
            AnyMessage::TeleportLureRequest(request) => {
                self.events.push_back(ServerEvent::TeleportViaLure {
                    lure_id: LureId::new(request.info.lure_id),
                    teleport_flags: request.info.teleport_flags,
                });
            }
            AnyMessage::TeleportCancel(_) => {
                self.events.push_back(ServerEvent::CancelTeleport);
            }
            AnyMessage::SetStartLocationRequest(request) => {
                let data = &request.start_location_data;
                // An unrecognised LocationID is malformed; surface the raw
                // message rather than guessing a slot.
                if let Some(slot) = StartLocationSlot::from_code(data.location_id) {
                    self.events.push_back(ServerEvent::SetStartLocation {
                        slot,
                        position: RegionCoordinates::new(
                            data.location_pos.x,
                            data.location_pos.y,
                            data.location_pos.z,
                        ),
                        look_at: data.location_look_at.clone(),
                    });
                } else {
                    self.events
                        .push_back(ServerEvent::ClientMessage(Box::new(message.clone())));
                }
            }
            AnyMessage::AgentDataUpdateRequest(_) => {
                self.events.push_back(ServerEvent::RequestAgentDataUpdate);
            }
            AnyMessage::AgentQuitCopy(quit) => {
                self.events.push_back(ServerEvent::QuitCopy {
                    viewer_circuit_code: CircuitCode(quit.fuse_block.viewer_circuit_code),
                });
            }
            AnyMessage::VelocityInterpolateOn(_) => {
                self.events
                    .push_back(ServerEvent::SetVelocityInterpolation { enabled: true });
            }
            AnyMessage::VelocityInterpolateOff(_) => {
                self.events
                    .push_back(ServerEvent::SetVelocityInterpolation { enabled: false });
            }
            AnyMessage::UserInfoRequest(_) => {
                self.events.push_back(ServerEvent::RequestUserInfo);
            }
            AnyMessage::UpdateUserInfo(update) => {
                self.events.push_back(ServerEvent::UpdateUserInfo {
                    im_via_email: update.user_data.im_via_e_mail,
                    directory_visibility: DirectoryVisibility::from_wire(&trimmed_string(
                        &update.user_data.directory_visibility,
                    )),
                });
            }
            AnyMessage::SoundTrigger(trigger) => {
                let block = &trigger.sound_data;
                self.events.push_back(ServerEvent::TriggerSound {
                    sound: AssetKey::from(block.sound_id),
                    gain: block.gain,
                    region_handle: RegionHandle(block.handle),
                    position: RegionCoordinates::new(
                        block.position.x,
                        block.position.y,
                        block.position.z,
                    ),
                });
            }
            AnyMessage::RequestGodlikePowers(request) => {
                self.events.push_back(ServerEvent::RequestGodlikePowers {
                    godlike: request.request_block.godlike,
                });
            }
            AnyMessage::EjectUser(eject) => {
                // An unrecognised Flags value is malformed; surface the raw
                // message rather than guessing the action.
                if let Some(action) = EjectAction::from_wire(eject.data.flags) {
                    self.events.push_back(ServerEvent::EjectUser {
                        target: AgentKey::from(eject.data.target_id),
                        action,
                    });
                } else {
                    self.events
                        .push_back(ServerEvent::ClientMessage(Box::new(message.clone())));
                }
            }
            AnyMessage::FreezeUser(freeze) => {
                // An unrecognised Flags value is malformed; surface the raw
                // message rather than guessing the action.
                if let Some(action) = FreezeAction::from_wire(freeze.data.flags) {
                    self.events.push_back(ServerEvent::FreezeUser {
                        target: AgentKey::from(freeze.data.target_id),
                        action,
                    });
                } else {
                    self.events
                        .push_back(ServerEvent::ClientMessage(Box::new(message.clone())));
                }
            }
            AnyMessage::SimWideDeletes(deletes) => {
                // An unrecognised Flags bit is malformed; surface the raw message
                // rather than dropping the unknown selection.
                if let Some(flags) = SimWideDeleteFlags::from_wire(deletes.data_block.flags) {
                    self.events.push_back(ServerEvent::SimWideDeletes {
                        owner: AgentKey::from(deletes.data_block.target_id),
                        flags,
                    });
                } else {
                    self.events
                        .push_back(ServerEvent::ClientMessage(Box::new(message.clone())));
                }
            }
            AnyMessage::GodUpdateRegionInfo(update) => {
                let info = &update.region_info;
                // An empty (or invalid) SimName is malformed for a god update;
                // surface the raw message rather than fabricating a region name.
                if let Some(sim_name) =
                    sl_wire::region_name_from_wire("SimName", &trimmed_string(&info.sim_name))?
                {
                    // Recover the full 64-bit extended flags from RegionInfo2 when
                    // present, falling back to the legacy 32-bit block.
                    let region_flags = update.region_info2.first().map_or_else(
                        || u64::from(info.region_flags),
                        |block| block.region_flags_extended,
                    );
                    // The redirect grid coordinates are signed on the wire (`0`
                    // for no redirect); a negative value is meaningless, so clamp
                    // to `0`.
                    let redirect_grid = GridCoordinates::new(
                        u32::try_from(info.redirect_grid_x).unwrap_or(0),
                        u32::try_from(info.redirect_grid_y).unwrap_or(0),
                    );
                    self.events.push_back(ServerEvent::GodUpdateRegionInfo {
                        update: GodRegionUpdate {
                            sim_name,
                            estate_id: info.estate_id,
                            parent_estate_id: info.parent_estate_id,
                            region_flags,
                            billable_factor: info.billable_factor,
                            price_per_meter: info.price_per_meter,
                            redirect_grid,
                        },
                    });
                } else {
                    self.events
                        .push_back(ServerEvent::ClientMessage(Box::new(message.clone())));
                }
            }
            AnyMessage::ParcelGodForceOwner(force) => {
                // The wire carries only an `OwnerID` with no group flag, so the
                // new owner is always decoded as an agent.
                self.events.push_back(ServerEvent::ParcelGodForceOwner {
                    local_id: RegionLocalParcelId(force.data.local_id),
                    owner: OwnerKey::Agent(AgentKey::from(force.data.owner_id)),
                });
            }
            AnyMessage::ParcelGodMarkAsContent(mark) => {
                self.events.push_back(ServerEvent::ParcelGodMarkAsContent {
                    local_id: RegionLocalParcelId(mark.parcel_data.local_id),
                });
            }
            AnyMessage::EventGodDelete(delete) => {
                self.events.push_back(ServerEvent::EventGodDelete {
                    event: EventId::new(delete.event_data.event_id),
                    query_id: QueryId::new(delete.query_data.query_id),
                    query_text: trimmed_string(&delete.query_data.query_text),
                    flags: DirFindFlags::from_bits(delete.query_data.query_flags),
                    query_start: delete.query_data.query_start,
                });
            }
            AnyMessage::StateSave(save) => {
                // The reference viewer sends an empty filename to mean "pick the
                // autosave name"; surface that as `None`.
                let filename = trimmed_string(&save.data_block.filename);
                self.events.push_back(ServerEvent::StateSave {
                    filename: (!filename.is_empty()).then_some(filename),
                });
            }
            AnyMessage::ViewerStartAuction(auction) => {
                // A nil snapshot id means "no snapshot advertising the auction".
                let snapshot_id = auction.parcel_data.snapshot_id;
                self.events.push_back(ServerEvent::ViewerStartAuction {
                    local_id: RegionLocalParcelId(auction.parcel_data.local_id),
                    snapshot: (!snapshot_id.is_nil()).then(|| TextureKey::from(snapshot_id)),
                });
            }
            AnyMessage::LogoutRequest(_) => {
                self.send_logout_reply(now)?;
                self.close(ServerEvent::LoggedOut);
            }
            other => {
                self.events
                    .push_back(ServerEvent::ClientMessage(Box::new(other.clone())));
            }
        }
        Ok(())
    }

    /// Replies to `CompleteAgentMovement` with an `AgentMovementComplete`,
    /// confirming the agent's presence in this region.
    fn send_agent_movement_complete(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentMovementComplete(AgentMovementComplete {
            agent_data: AgentMovementCompleteAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            data: AgentMovementCompleteDataBlock {
                position: Camera::region_center().center,
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                region_handle: self.region_handle.0,
                timestamp: 0,
            },
            sim_data: AgentMovementCompleteSimDataBlock {
                channel_version: self.channel_version.clone(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Replies to `LogoutRequest` with a `LogoutReply` (no inventory items).
    fn send_logout_reply(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::LogoutReply(LogoutReply {
            agent_data: LogoutReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            inventory_data: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Advances time: flushes owed acknowledgements, retransmits timed-out
    /// reliable packets, pings the client on cadence, and closes the session on
    /// inactivity or a retransmission give-up.
    pub fn handle_timeout(&mut self, now: Instant) {
        if matches!(self.state, SimState::Closed) {
            return;
        }
        if now >= self.inactivity {
            self.close(ServerEvent::Disconnected);
            return;
        }
        if let Some(at) = self.ack_flush
            && now >= at
        {
            // A flush failure is a wire-encoding bug, not a runtime condition;
            // drop the owed acks rather than panicking.
            let _result = self.flush_acks(now);
        }
        if self.process_resends(now) {
            self.close(ServerEvent::Disconnected);
            return;
        }
        if let Some(at) = self.ping
            && now >= at
        {
            self.ping = Some(deadline(now, PING_INTERVAL));
            let _result = self.start_ping_check(now);
        }
    }

    /// The next datagram to send to the client, if any.
    pub fn poll_transmit(&mut self) -> Option<Transmit> {
        let destination = self.client_addr?;
        let payload = self.out.pop_front()?;
        Some(Transmit {
            destination,
            payload,
        })
    }

    /// The earliest instant at which [`SimSession::handle_timeout`] should next
    /// run.
    #[must_use]
    pub fn poll_timeout(&self) -> Option<Instant> {
        if matches!(self.state, SimState::Closed) {
            return None;
        }
        let mut earliest = Some(self.inactivity);
        merge_deadline(&mut earliest, self.ack_flush);
        merge_deadline(&mut earliest, self.ping);
        merge_deadline(&mut earliest, self.next_resend_deadline());
        earliest
    }

    /// The next server event, if any.
    pub fn poll_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Transitions to the closed state, emitting `reason` once.
    fn close(&mut self, reason: ServerEvent) {
        if !matches!(self.state, SimState::Closed) {
            self.state = SimState::Closed;
            self.ping = None;
            self.ack_flush = None;
            self.events.push_back(reason);
        }
    }
}

/// Decodes name/message bytes to a `String`, dropping the trailing NUL the
/// client appends to variable string fields.
fn trimmed_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_owned()
}

/// Encodes a string as NUL-terminated UTF-8 bytes, as a simulator sends variable
/// string fields on the wire.
fn with_nul(s: &str) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Encodes an optional array index for the `You`/`Prey` fields of
/// `CoarseLocationUpdate`: `None` (and any index that does not fit) becomes the
/// "absent" sentinel `-1` (the inverse of the `index_into` decoder).
fn from_index(index: Option<usize>) -> i16 {
    match index {
        Some(value) => i16::try_from(value).unwrap_or(-1),
        None => -1,
    }
}

/// Maps a `telehub` `EstateOwnerMessage`'s parameter list to a [`ServerEvent`].
/// The first block holds the sub-command; the second (when present) holds the
/// object/spawn id as a decimal `u32` (the layout `LLClientView` parses).
/// Returns `None` for an unknown sub-command.
fn telehub_server_event(params: &[EstateOwnerMessageParamListBlock]) -> Option<ServerEvent> {
    let command = trimmed_string(&params.first()?.parameter);
    let param1 = || {
        params
            .get(1)
            .map(|block| trimmed_string(&block.parameter))
            .and_then(|text| text.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    let event = match command.trim() {
        "info ui" => ServerEvent::RequestTelehubInfo,
        "connect" => ServerEvent::ConnectTelehub {
            object_local_id: RegionLocalObjectId(param1()),
        },
        "delete" => ServerEvent::DisconnectTelehub,
        "spawnpoint add" => ServerEvent::AddTelehubSpawnPoint {
            object_local_id: RegionLocalObjectId(param1()),
        },
        "spawnpoint remove" => ServerEvent::RemoveTelehubSpawnPoint {
            spawn_index: param1(),
        },
        _ => return None,
    };
    Some(event)
}

/// Maps a `terrain` `EstateOwnerMessage`'s parameter list to a [`ServerEvent`]:
/// `bake`, `download filename <name>`, `upload filename <name>` (the three
/// sub-commands `LLClientView` dispatches). Returns `None` for an unknown
/// sub-command or a missing filename.
fn terrain_server_event(params: &[EstateOwnerMessageParamListBlock]) -> Option<ServerEvent> {
    let command = trimmed_string(&params.first()?.parameter);
    let filename = || params.get(1).map(|block| trimmed_string(&block.parameter));
    let event = match command.trim() {
        "bake" => ServerEvent::TerrainBakeRequested,
        "download filename" => ServerEvent::TerrainDownloadRequested {
            viewer_filename: filename()?,
        },
        "upload filename" => ServerEvent::TerrainUploadRequested {
            viewer_filename: filename()?,
        },
        _ => return None,
    };
    Some(event)
}

/// Decodes the seven little-endian `f32` bits-per-second rates an `AgentThrottle`
/// carries into a [`Throttle`] (the inverse of [`Throttle::bits_per_second`]).
/// Returns `None` if the block is truncated.
fn decode_throttle(bytes: &[u8]) -> Option<Throttle> {
    let mut reader = Reader::new(bytes);
    let mut rates = [0.0_f32; 7];
    for rate in &mut rates {
        *rate = reader.f32().ok()?;
    }
    Some(Throttle::from_bits_per_second(rates))
}
