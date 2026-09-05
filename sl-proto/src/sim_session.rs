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
use sl_types::money::LindenAmount;
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
    EstateCovenantReply, EstateCovenantReplyDataBlock, EstateOwnerMessage,
    EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
    EstateOwnerMessageParamListBlock, EventInfoReply, EventInfoReplyAgentDataBlock,
    EventInfoReplyEventDataBlock, FindAgent, FindAgentAgentBlockBlock, FindAgentLocationBlockBlock,
    LogoutReply, LogoutReplyAgentDataBlock, PlacesReply, PlacesReplyAgentDataBlock,
    PlacesReplyQueryDataBlock, PlacesReplyTransactionDataBlock, StartPingCheck,
    StartPingCheckPingIDBlock, UUIDGroupNameReply, UUIDGroupNameReplyUUIDNameBlockBlock,
    UUIDNameReply, UUIDNameReplyUUIDNameBlockBlock, ViewerEffect as ViewerEffectMessage,
    ViewerEffectAgentDataBlock, ViewerEffectEffectBlock,
};
use sl_wire::messages::{
    AgentWearablesUpdate, AgentWearablesUpdateAgentDataBlock,
    AgentWearablesUpdateWearableDataBlock, EconomyData as EconomyDataMessage, EconomyDataInfoBlock,
};
use sl_wire::messages::{
    AvatarAnimation as AvatarAnimationWire, AvatarAnimationAnimationListBlock,
    AvatarAnimationAnimationSourceListBlock, AvatarAnimationSenderBlock,
    AvatarAppearance as AvatarAppearanceWire, AvatarAppearanceAppearanceDataBlock,
    AvatarAppearanceAppearanceHoverBlock, AvatarAppearanceAttachmentBlockBlock,
    AvatarAppearanceObjectDataBlock, AvatarAppearanceSenderBlock, AvatarAppearanceVisualParamBlock,
    ImprovedTerseObjectUpdate, ImprovedTerseObjectUpdateObjectDataBlock,
    ImprovedTerseObjectUpdateRegionDataBlock,
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
    ReplyTaskInventoryInventoryDataBlock, UpdateCreateInventoryItem,
    UpdateCreateInventoryItemAgentDataBlock, UpdateCreateInventoryItemInventoryDataBlock,
    UserInfoReply, UserInfoReplyAgentDataBlock, UserInfoReplyUserDataBlock,
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
    ObjectProperties as ObjectPropertiesMessage,
    ObjectPropertiesFamily as ObjectPropertiesFamilyMessage,
    ObjectPropertiesFamilyObjectDataBlock as ObjectPropertiesFamilyObjectDataBlockMessage,
    ObjectPropertiesObjectDataBlock as ObjectPropertiesObjectDataBlockMessage, ParcelDwellReply,
    ParcelDwellReplyAgentDataBlock, ParcelDwellReplyDataBlock, ParcelInfoReply,
    ParcelInfoReplyAgentDataBlock, ParcelInfoReplyDataBlock, ParcelObjectOwnersReply,
    ParcelObjectOwnersReplyDataBlock, PayPriceReply, PayPriceReplyButtonDataBlock,
    PayPriceReplyObjectDataBlock, ScriptRunningReply, ScriptRunningReplyScriptBlock,
    TelehubInfo as TelehubInfoMessage, TelehubInfoSpawnPointBlockBlock,
    TelehubInfoTelehubBlockBlock,
};
use sl_wire::messages::{
    ParcelAccessListReply, ParcelAccessListReplyDataBlock, ParcelAccessListReplyListBlock,
    RegionInfo as RegionInfoMessage, RegionInfoAgentDataBlock, RegionInfoCombatSettingsBlock,
    RegionInfoRegionInfo2Block, RegionInfoRegionInfo3Block, RegionInfoRegionInfo5Block,
    RegionInfoRegionInfoBlock,
};
use sl_wire::{
    AnyMessage, CircuitCode, ControlFlags, EventQueueEvent, ExperienceInfo, ExperiencePermission,
    ExperienceUpdate, GlobalCoordinates, Llsd, MessageId, PacketFlags, Permissions, Permissions5,
    Reader, RegionHandle, RegionLocalObjectId, RegionLocalParcelId, SequenceNumber, WireError,
    Writer, build_event_queue_response, encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::AssetKey;
use crate::ack_flush::send_ack_packets;
use crate::appearance::{MAX_FACES, decode_texture_entry};
use crate::bookkeeping_ids::{
    ImSessionId, InventoryCallbackId, LureId, PingId, QueryId, TransactionId, TransferId, XferId,
};
use crate::error::Error;
use crate::extra_params::decode_extra_param_blocks;
use crate::object_update::TerseUpdate;
use crate::session::{
    CrossedRegionInfo, SERVER_HISTORY_CAP, STANDARD_REGION_SIZE_METRES, ServerHistoryMessage,
    TeleportFinishInfo, XFER_STALL_TIMEOUT, XFER_TIMEOUT_RESULT, agent_drop_group_to_llsd,
    agent_list_voice_updates_to_llsd, agent_state_update_to_llsd, build_map_block_reply,
    build_map_item_reply, build_map_layer_reply, build_task_inventory,
    chatterbox_invitation_to_llsd, chatterbox_session_start_reply_to_llsd,
    crossed_region_to_caps_llsd, display_name_update_to_llsd, enable_simulator_to_caps_llsd,
    establish_agent_communication_to_llsd, full_update_block, instant_message,
    nav_mesh_status_to_llsd, open_region_info_to_llsd, parcel_properties_to_llsd,
    parcel_properties_to_wire, region_handshake_message, required_voice_version_to_llsd,
    set_display_name_reply_to_llsd, shape_from_object_shape_block, sim_console_response_to_llsd,
    teleport_finish_to_llsd, unpack_uuids, windlight_refresh_to_llsd,
};
use crate::sim_experiences::SimExperiences;
use crate::sim_inventory::{SimInventoryError, SimInventoryTree};
use crate::sim_voice::{SimVoice, VoiceProvisionOutcome, VoiceProvisionRefusal};
use crate::types::directory::category_from_wire;
use crate::types::{
    AddPrimParams, AlertInfo, AssetType, AttachmentMode, AttachmentPoint, AvatarAppearance,
    AvatarName, AvatarPickerResult, Camera, ChatSource, ChatType, ClassifiedCategory, ClickAction,
    CoarseLocation, DEFAULT_SKY_FRAME, DEFAULT_WATER_FRAME, DayCycle, DayCycleFrame,
    DeRezDestination, DetachOrder, DirClassifiedResult, DirEventResult, DirFindFlags,
    DirGroupResult, DirLandResult, DirPeopleResult, DirPlaceResult, DirectoryVisibility,
    DisplayNameUpdate, EconomyData, EjectAction, EnvironmentSettings, EnvironmentUpdate,
    EstateAccessKind, EstateCovenant, EstateInfo, EventInfo, FeatureDisabled,
    FollowCamPropertyValue, FreezeAction, FriendRights, GenericMessage, GenericStreamingMessage,
    GestureActivation, GodRegionUpdate, GroupAccountDetails, GroupAccountSummary,
    GroupAccountTransactions, GroupActiveProposalItem, GroupName, GroupVoteHistoryItem, ImDialog,
    InstantMessage, InventoryFolder, InventoryItem, InventoryItemMove, InventoryType, Kick,
    LandBrushAction, LandBrushSize, LandEdit, LandSearchType, LandStatItem, LandStatReportType,
    MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags, Material, MeanCollision,
    MovementMode, NavMeshStatus, NewInventoryLink, NotecardRez, Object, ObjectBuyItem,
    ObjectExtraParams, ObjectFlagSettings, ObjectPlayingAnimation, ObjectProperties,
    ObjectPropertiesFamily, ObjectTransform, OpenRegionInfo, ParcelAccessEntry, ParcelAccessFlags,
    ParcelAccessScope, ParcelCategory, ParcelDetails, ParcelInfo, ParcelObjectOwner,
    ParcelReturnType, ParcelUpdate, PermissionField, PlacesResult, PlayingAnimation, Postcard,
    PrimShape, PrimShapeParams, ProposalVoteId, RegionIdentity, RegionLimits, RegionStats,
    Reliability, RequiredVoiceVersion, RestoreItem, RezAttachment, RezObjectParams,
    RezScriptParams, SaleType, ScriptControl, ScriptPermissionRequest, ScriptPermissions,
    ServerError, SetDisplayNameReply, SimWideDeleteFlags, SimulatorTime, SkySettings,
    StartLocationSlot, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, TelehubInfo,
    TerraformArea, TerrainLayerType, TerrainPatch, TextureEntry, Throttle, TransferStatus,
    Transmit, UpdateGroupInfoParams, UserInfo, ViewerEffect, ViewerEffectData, ViewerEffectType,
    WaterSettings, Wearable,
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
    KillObject, KillObjectObjectDataBlock, LayerData, LayerDataLayerDataBlock,
    LayerDataLayerIDBlock, ObjectUpdate, ObjectUpdateCompressed,
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

/// The floor on the retransmission timeout, however fast the measured round
/// trip is (the reference's `LL_MINIMUM_RELIABLE_TIMEOUT_SECONDS`). The
/// simulator's timeout is this or [`RELIABLE_TIMEOUT_FACTOR`] times the
/// measured round trip, whichever is larger — the same policy the client
/// [`Session`](crate::Session) uses.
const MINIMUM_RESEND_TIMEOUT: Duration = Duration::from_secs(1);

/// The multiple of the measured round trip a reliable packet waits before it is
/// retransmitted (the reference's `LL_RELIABLE_TIMEOUT_FACTOR`), floored at
/// [`MINIMUM_RESEND_TIMEOUT`].
const RELIABLE_TIMEOUT_FACTOR: f32 = 5.0;

/// The weight a fresh round-trip sample carries in the ping average.
const PING_AVERAGE_ALPHA: f32 = 0.2;

/// `1.0 - PING_AVERAGE_ALPHA`, spelled out so the update is literal-only
/// arithmetic.
const PING_AVERAGE_DECAY: f32 = 0.8;

/// The floor the ping average is clamped to.
const PING_AVERAGE_MIN: Duration = Duration::from_millis(100);

/// The ceiling the ping average is clamped to, capping the retransmission
/// timeout at `RELIABLE_TIMEOUT_FACTOR` times this.
const PING_AVERAGE_MAX: Duration = Duration::from_millis(2000);

/// The ping average a circuit starts with, before any round trip is measured.
const INITIAL_PING_AVERAGE: Duration = Duration::from_millis(1000);

/// The cadence at which the simulator pings an active client with a
/// `StartPingCheck`.
const PING_INTERVAL: Duration = Duration::from_secs(5);

/// How many times a reliable packet is sent before it is given up on: the first
/// transmission plus the reference's `LL_DEFAULT_RELIABLE_RETRIES`.
const MAX_RESEND_ATTEMPTS: u32 = 4;

/// How long the sit handshake may sit in
/// [`SimSitState::ResponseSent`] awaiting the client's completing `AgentSit`
/// before the offer is withdrawn. The mirror of the client's `SIT_TIMEOUT`.
const SIT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// The largest asset or file the simulator will accept over an inbound `Xfer`
/// pull. Comfortably above the largest thing a client uploads this way (a
/// standard region's terrain RAW is 13 x 256 x 256 = 832 KiB, and an OpenSim
/// var-region a small multiple of that) and far below what an unbounded stream
/// of `SendXferPacket`s could grow to.
const MAX_XFER_RECEIVE_BYTES: usize = 16 * 1024 * 1024;

/// The largest number of CAPS event-queue events held for a client that is not
/// polling. Past this the oldest are dropped, so a driver that keeps enqueuing
/// to a client that has stopped long-polling cannot grow the queue without
/// bound.
const MAX_CAPS_EVENTS: usize = 4096;

/// The largest number of script-permission answers recorded for one session.
/// The registry is keyed by (task, item) and only ever grows, so it is bounded
/// here rather than by the client's willingness to answer questions.
const MAX_SCRIPT_GRANTS: usize = 4096;

/// How long a `TransferRequest` waits for the driver to serve or refuse it
/// before the simulator answers it itself and drops the parked request.
const TRANSFER_SERVE_TIMEOUT: Duration = Duration::from_secs(60);

/// The bound on the recently-seen inbound reliable sequence window.
const SEEN_CAPACITY: usize = 4096;

/// The maximum number of names packed into a single `UUIDNameReply` /
/// `UUIDGroupNameReply`. Smaller than the request batch because each entry also
/// carries the (variable-length) name strings.
const UUID_NAMES_PER_REPLY: usize = 40;

/// The `EstateOwnerMessage` method a simulator answers an estate `getinfo`
/// with: the estate's configuration.
const ESTATE_UPDATE_INFO_METHOD: &str = "estateupdateinfo";

/// The `EstateOwnerMessage` method one of the estate's access lists is sent
/// under.
const SET_ACCESS_METHOD: &str = "setaccess";

/// The `setaccess` category bit of the allowed-agents list.
const ESTATE_ACCESS_ALLOWED_AGENTS: u32 = 1;

/// The `setaccess` category bit of the allowed-groups list.
const ESTATE_ACCESS_ALLOWED_GROUPS: u32 = 2;

/// The `setaccess` category bit of the banned-agents list.
const ESTATE_ACCESS_BANNED_AGENTS: u32 = 4;

/// The `setaccess` category bit of the estate-managers list.
const ESTATE_ACCESS_MANAGERS: u32 = 8;

/// The `setaccess` category bit an [`EstateAccessKind`] is sent under.
const fn estate_access_code(kind: EstateAccessKind) -> u32 {
    match kind {
        EstateAccessKind::AllowedAgents => ESTATE_ACCESS_ALLOWED_AGENTS,
        EstateAccessKind::AllowedGroups => ESTATE_ACCESS_ALLOWED_GROUPS,
        EstateAccessKind::BannedAgents => ESTATE_ACCESS_BANNED_AGENTS,
        EstateAccessKind::Managers => ESTATE_ACCESS_MANAGERS,
    }
}

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

/// What losing `message` for good costs the simulator's session.
///
/// Only the two packets that establish the agent's presence on the circuit are
/// session-critical: without the region handshake or the movement completion
/// the client never finishes arriving, so there is nothing left to keep the
/// circuit open for. Everything else is one lost message on a live session,
/// which the reference likewise reports through the packet's own failure
/// callback rather than by tearing the circuit down.
const fn severity_of(message: &AnyMessage) -> SimReliableSeverity {
    match *message {
        AnyMessage::RegionHandshake(_) | AnyMessage::AgentMovementComplete(_) => {
            SimReliableSeverity::SessionCritical
        }
        _ => SimReliableSeverity::BestEffort,
    }
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

/// What losing a reliable packet for good costs the simulator's session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimReliableSeverity {
    /// The packet establishes the agent's presence on the circuit
    /// (`RegionHandshake`, `AgentMovementComplete`): a client that never
    /// receives it never finishes arriving, so there is nothing left to keep
    /// the circuit open for.
    SessionCritical,
    /// An ordinary reliable message. Losing it costs that one message; the
    /// session keeps running and the loss is surfaced as
    /// [`ServerEvent::ReliableGiveUp`] — the reference behaviour, where an
    /// exhausted reliable packet invokes its own failure callback and leaves
    /// the circuit alone.
    BestEffort,
}

/// A datagram queued for transmission to the client.
#[derive(Debug, Clone)]
struct SimOutbound {
    /// The outgoing sequence number of the reliable packet this datagram
    /// carries, so popping it can start that packet's retransmission clock.
    /// `None` for an unreliable datagram, which nothing is waiting on.
    sequence: Option<SequenceNumber>,
    /// The fully encoded datagram.
    payload: Vec<u8>,
}

/// A reliable packet awaiting acknowledgement, kept so it can be retransmitted.
#[derive(Debug, Clone)]
struct UnackedPacket {
    /// The fully encoded datagram, ready to resend.
    datagram: Vec<u8>,
    /// When the current attempt's retransmission clock started. While the
    /// datagram is still `queued` this is pushed forward to the latest instant
    /// the session is told about, so time spent waiting on a backed-up driver
    /// does not count as silence from the client.
    sent_at: Instant,
    /// Whether the current attempt's datagram is still sitting in the outbound
    /// queue rather than having been handed to the driver.
    queued: bool,
    /// How many times the packet has been sent so far.
    attempts: u32,
    /// The message name, for the give-up report.
    name: Option<&'static str>,
    /// What losing this packet costs the session.
    severity: SimReliableSeverity,
}

/// A reliable packet that has run out of retransmissions, reported by
/// [`SimSession::process_resends`].
#[derive(Debug, Clone, Copy)]
struct ExhaustedPacket {
    /// The outgoing sequence number the packet was sent with.
    sequence: SequenceNumber,
    /// The message name (`None` for an unrecognised id).
    name: Option<&'static str>,
    /// What the loss costs the session.
    severity: SimReliableSeverity,
}

/// A `TransferRequest` parked for the driver to serve, with the deadline past
/// which the simulator answers it itself.
#[derive(Debug, Clone)]
struct SimTransferServe {
    /// The raw request params, echoed back in the `TransferInfo`.
    params: Vec<u8>,
    /// When the request is answered as unanswerable and dropped.
    expires: Instant,
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
    /// When the send is abandoned for lack of a confirmation.
    expires: Instant,
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
    /// The packet number the next `SendXferPacket` must carry. `Xfer` is a
    /// strictly ordered, one-packet-in-flight stream, so anything else is a
    /// duplicate or a gap and is refused rather than concatenated blindly.
    next_packet: u32,
    /// When the pull is abandoned for lack of a packet.
    expires: Instant,
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

/// Where an agent lands when its movement into the region completes: the
/// `Position` / `LookAt` of the `AgentMovementComplete` reply.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ArrivalPlacement {
    /// The landing position within the region.
    pub position: RegionCoordinates,
    /// The direction the avatar faces on landing.
    pub look_at: Vector,
}

impl Default for ArrivalPlacement {
    /// The region centre, facing +X — what a login lands at without a
    /// placement of its own.
    fn default() -> Self {
        let center = Camera::region_center().center;
        Self {
            position: RegionCoordinates::new(center.x, center.y, center.z),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        }
    }
}

/// The `TeleportProgress` / `TeleportFailed` message **keys** the reference
/// viewer localises (`teleport_strings.xml`: `process_teleport_progress`
/// swaps a known key for its translated text and shows an unknown string
/// raw). A simulator sends the keys, never prose, so every locale reads its
/// own text — the same contract the reference servers follow.
pub mod teleport_strings {
    /// Progress: the destination is being looked up.
    pub const RESOLVING: &str = "resolving";
    /// Progress: the destination simulator is being contacted.
    pub const CONTACTING: &str = "contacting";
    /// Progress: the agent is being sent to an explicit location.
    pub const SENDING_DEST: &str = "sending_dest";
    /// Progress: the agent is being sent home.
    pub const SENDING_HOME: &str = "sending_home";
    /// Progress: the agent is being sent to a landmark.
    pub const SENDING_LANDMARK: &str = "sending_landmark";
    /// Progress: the handover is underway (the last line before arrival).
    pub const ARRIVING: &str = "arriving";
    /// Progress: the teleport is completing.
    pub const COMPLETING: &str = "completing";
    /// Failure: the destination region is unknown or not available.
    pub const INVALID_TPORT: &str = "invalid_tport";
    /// Failure: the destination refuses (banned / access restricted).
    pub const NOACCESS_TPORT: &str = "noaccess_tport";
    /// Failure: the landmark names no usable destination.
    pub const NOLANDMARK_TPORT: &str = "nolandmark_tport";
    /// Failure: the destination never confirmed the arrival.
    pub const TIMEOUT_TPORT: &str = "timeout_tport";
    /// Failure: the destination simulator is not reachable.
    pub const NO_HOST: &str = "no_host";
    /// Failure: the region handoff was refused by the destination.
    pub const INVALID_REGION_HANDOFF: &str = "invalid_region_handoff";
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

/// The largest number of terrain patches
/// [`send_terrain`](SimSession::send_terrain) packs into one `LayerData`
/// message. A compressed 16×16 patch is a few hundred bytes at worst, so four
/// of them stay under the ~1 kB a simulator keeps a `LayerData` datagram to.
pub const TERRAIN_PATCHES_PER_MESSAGE: usize = 4;

/// The patch positions of a `(0, 0)..=(max_x, max_y)` grid in the spiral order
/// OpenSim sends a region's ground in (`LLClientView.SendLayerTopRight` /
/// `SendLayerBottomLeft`): the outer ring first, starting at the south-west
/// corner — east along the south edge, north up the east edge, west back along
/// the north edge, south down the west edge — then the next ring in, until the
/// centre is reached. Every position appears exactly once.
fn spiral_patch_order(max_x: u32, max_y: u32) -> Vec<(u32, u32)> {
    let mut order = Vec::new();
    let (mut west, mut south, mut east, mut north) = (0_u32, 0_u32, max_x, max_y);
    loop {
        for x in west..=east {
            order.push((x, south));
        }
        for y in south.saturating_add(1)..=north {
            order.push((east, y));
        }
        if east <= west || north <= south {
            break;
        }
        south = south.saturating_add(1);
        east = east.saturating_sub(1);
        for x in (west..=east).rev() {
            order.push((x, north));
        }
        for y in (south..north).rev() {
            order.push((west, y));
        }
        if east <= west || north <= south {
            break;
        }
        west = west.saturating_add(1);
        north = north.saturating_sub(1);
    }
    order
}

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

/// Why the simulator refused an inbound message instead of acting on it,
/// reported as [`ServerEvent::Rejected`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RejectionReason {
    /// The message arrived before the circuit was opened with `UseCircuitCode`,
    /// so there is no session to attribute it to.
    NoCircuit,
    /// The message asserted an `AgentData.SessionID` other than the one the
    /// circuit was opened with — another agent's traffic, a stale session, or a
    /// forgery.
    SessionIdMismatch,
    /// A `UseCircuitCode` tried to rebind a live circuit to a different agent,
    /// session or circuit code. A circuit's identity is fixed when it opens.
    CircuitRebind,
    /// The message was rejected because it would have grown a per-session store
    /// past its bound (an oversized `Xfer` upload, a full grant registry).
    LimitExceeded,
    /// An `Xfer` packet arrived out of order on a strictly ordered stream.
    OutOfOrder,
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
    /// The driver retired this circuit ([`SimSession::retire_circuit`]): the
    /// agent completed a teleport elsewhere, `DisableSimulator` went out, and
    /// the session is closed.
    CircuitRetired,
    /// The client acknowledged the region handshake with `RegionHandshakeReply`.
    RegionHandshakeReplied,
    /// The simulator refused an inbound message rather than acting on it. The
    /// message is named by its template name so a harness can assert on the
    /// rejection path without reconstructing the message.
    Rejected {
        /// The template name of the refused message (`None` if its id decoded
        /// to no known message).
        message: Option<String>,
        /// Why it was refused.
        reason: RejectionReason,
    },
    /// A reliable packet the simulator sent exhausted its retransmission budget
    /// and was given up on. The session stays open: only the packets that
    /// establish the agent's presence are fatal, and those close the session
    /// with [`ServerEvent::Disconnected`] instead.
    ReliableGiveUp {
        /// The template name of the packet's message (`None` if unrecognised).
        message: Option<String>,
    },
    /// A sit offer ([`SimSession::send_avatar_sit_response`]) went unanswered
    /// and was withdrawn; the agent is not seated.
    SitOfferExpired {
        /// The object that had been offered as a seat.
        on: ObjectKey,
    },
    /// A `TransferRequest` the driver never served or refused timed out; the
    /// simulator has answered it with an `UnknownSource` `TransferInfo` and
    /// dropped it.
    TransferServeExpired {
        /// The client-minted transfer id that went unanswered.
        transfer_id: TransferId,
    },
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
    /// The client invited more agents into a session that is already open
    /// (the `ChatSessionRequest` `"invite"` method — the modern "add
    /// participants"). The invitees are already in the session's roster; the
    /// driver materialises the session on each of their [`SimSession`]s
    /// ([`SimSession::open_chat_session`]) and delivers the invitation over
    /// their event queues ([`SimSession::enqueue_chatterbox_invitation`]),
    /// exactly as for a conference start.
    SessionInviteRequested {
        /// The session invited into.
        session_id: ImSessionId,
        /// The newly invited agents.
        invitees: Vec<AgentKey>,
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
    /// The client asked for a parcel's dwell — its accumulated traffic score —
    /// by region-local id (`ParcelDwellRequest`); the simulator answers with
    /// [`SimSession::send_parcel_dwell_reply`].
    ///
    /// The request carries the grid-wide id as well, but a viewer sends it
    /// nil (it is asking precisely because it does not know one), so only the
    /// region-local id identifies the parcel.
    RequestParcelDwell {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
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
    /// The client asked for the grid's economy — the L$ prices and this
    /// region's object budget (`EconomyDataRequest`); the simulator answers
    /// with [`SimSession::send_economy_data`].
    ///
    /// The request carries nothing but the agent: which region's capacity to
    /// report is the receiving simulator's own.
    RequestEconomyData,
    /// The client asked what the agent is wearing (`AgentWearablesRequest`);
    /// the simulator answers with
    /// [`SimSession::send_agent_wearables_update`].
    ///
    /// A simulator pushes the same message unsolicited at login and after
    /// every wearable change, so this is the re-ask a viewer makes when it
    /// missed one.
    RequestAgentWearables,
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
    /// The client renamed an object (`ObjectName`). The inverse of the client's
    /// [`Session::set_object_name`](crate::Session::set_object_name). One event
    /// is emitted per object block in the message.
    ObjectNameSet {
        /// The region-local id of the object being renamed.
        local_id: RegionLocalObjectId,
        /// The object's new name.
        name: String,
    },
    /// The client re-described an object (`ObjectDescription`). The inverse of
    /// the client's
    /// [`Session::set_object_description`](crate::Session::set_object_description).
    /// One event is emitted per object block in the message.
    ObjectDescriptionSet {
        /// The region-local id of the object being re-described.
        local_id: RegionLocalObjectId,
        /// The object's new description.
        description: String,
    },
    /// The client set an object's search category (`ObjectCategory`). The
    /// inverse of the client's
    /// [`Session::set_object_category`](crate::Session::set_object_category).
    /// One event is emitted per object block in the message.
    ObjectCategorySet {
        /// The region-local id of the object being categorised.
        local_id: RegionLocalObjectId,
        /// The `LLCategory` code the object is filed under.
        category: u32,
    },
    /// The client set an object's left-click behaviour (`ObjectClickAction`).
    /// The inverse of the client's
    /// [`Session::set_object_click_action`](crate::Session::set_object_click_action).
    /// One event is emitted per object block in the message.
    ObjectClickActionSet {
        /// The region-local id of the object being changed.
        local_id: RegionLocalObjectId,
        /// The new click behaviour.
        click_action: ClickAction,
    },
    /// The client set an object's physical material (`ObjectMaterial`). The
    /// inverse of the client's
    /// [`Session::set_object_material`](crate::Session::set_object_material).
    /// One event is emitted per object block in the message.
    ObjectMaterialSet {
        /// The region-local id of the object being changed.
        local_id: RegionLocalObjectId,
        /// The new physical material.
        material: Material,
    },
    /// The client put an object up for sale, or took it off sale
    /// (`ObjectSaleInfo`). The inverse of the client's
    /// [`Session::set_object_for_sale`](crate::Session::set_object_for_sale).
    /// One event is emitted per object block in the message.
    ObjectSaleInfoSet {
        /// The region-local id of the object being (un)listed.
        local_id: RegionLocalObjectId,
        /// What is sold: the original, a copy, or its contents.
        /// [`SaleType::NotForSale`] takes it off sale.
        sale_type: SaleType,
        /// The asking price, or [`None`] when the object is not for sale. A
        /// for-sale object may still be free (`Some(LindenAmount(0))`).
        sale_price: Option<LindenAmount>,
    },
    /// The client set an object's physics/temporary/phantom flags
    /// (`ObjectFlagUpdate`). The inverse of the client's
    /// [`Session::set_object_flags`](crate::Session::set_object_flags).
    ///
    /// Unlike its neighbours the wire message names exactly one object, so
    /// this is one event per message rather than per block.
    ObjectFlagsSet {
        /// The region-local id of the object being changed.
        local_id: RegionLocalObjectId,
        /// The object's complete new flag state (all four are sent every time,
        /// so this is the state and not a delta).
        flags: ObjectFlagSettings,
    },
    /// The client listed an object in (or removed it from) parcel search
    /// (`ObjectIncludeInSearch`). The inverse of the client's
    /// [`Session::set_object_include_in_search`](crate::Session::set_object_include_in_search).
    /// One event is emitted per object block in the message.
    ObjectIncludeInSearchSet {
        /// The region-local id of the object being (un)listed.
        local_id: RegionLocalObjectId,
        /// Whether the object is now listed in search.
        include_in_search: bool,
    },
    /// The client granted or revoked permission bits on an object
    /// (`ObjectPermissions`). The inverse of the client's
    /// [`Session::set_object_permissions`](crate::Session::set_object_permissions).
    /// One event is emitted per object block in the message, because a viewer
    /// sends one block per (object, mask) pair it is changing.
    ///
    /// A block whose `Field` byte names no mask is dropped rather than
    /// surfaced: there is no default mask to fall back on, and applying the
    /// change to the wrong one would hand out rights nobody granted.
    ObjectPermissionsSet {
        /// The region-local id of the object being changed.
        local_id: RegionLocalObjectId,
        /// Which of the five masks the change applies to.
        field: PermissionField,
        /// Whether the bits in [`mask`](Self::ObjectPermissionsSet::mask) are
        /// being granted (`true`) or revoked (`false`).
        set: bool,
        /// The permission bits being granted or revoked.
        mask: Permissions,
        /// Whether the client claims god powers for the change (the
        /// `HeaderData` `Override` flag). A simulator that honours it lets an
        /// administrator change permissions the owner could not.
        god_override: bool,
    },
    /// The client set the group an object is shared with (`ObjectGroup`). The
    /// inverse of the client's
    /// [`Session::set_object_group`](crate::Session::set_object_group).
    ///
    /// One event per message: a viewer sends its whole selection under one
    /// group id.
    ObjectGroupSet {
        /// The region-local ids of the objects being changed.
        local_ids: Vec<RegionLocalObjectId>,
        /// The group the objects are set to, or [`None`] to clear it.
        group_id: Option<GroupKey>,
    },
    /// The client changed who owns an object (`ObjectOwner`) — in practice the
    /// deed-to-group the build floater offers, which names the group and no
    /// agent. The inverse of the client's
    /// [`Session::deed_objects_to_group`](crate::Session::deed_objects_to_group).
    ObjectOwnerSet {
        /// The region-local ids of the objects being changed.
        local_ids: Vec<RegionLocalObjectId>,
        /// The new owner: an agent, or the group an object is deeded to.
        owner: OwnerKey,
        /// Whether the client claims god powers for the change (the
        /// `HeaderData` `Override` flag).
        god_override: bool,
    },
    /// The client linked a selection into one linkset (`ObjectLink`). The
    /// inverse of the client's
    /// [`Session::link_objects`](crate::Session::link_objects).
    ///
    /// The **first** id is the root the others are parented to (the reference
    /// packs the selection's root first, and OpenSim's `HandleObjectLink`
    /// reads it that way).
    ObjectsLinked {
        /// The region-local ids of the objects being linked, root first.
        local_ids: Vec<RegionLocalObjectId>,
    },
    /// The client broke objects out of their linkset (`ObjectDelink`). The
    /// inverse of the client's
    /// [`Session::delink_objects`](crate::Session::delink_objects).
    ObjectsDelinked {
        /// The region-local ids of the objects being unlinked.
        local_ids: Vec<RegionLocalObjectId>,
    },
    /// The client copied a selection in place (`ObjectDuplicate`). The inverse
    /// of the client's
    /// [`Session::duplicate_objects`](crate::Session::duplicate_objects).
    ///
    /// The simulator mints the copies' ids the way it does for a rez, so the
    /// duplicating client learns them from the `ObjectUpdate` that follows.
    ObjectsDuplicated {
        /// The region-local ids of the objects being copied.
        local_ids: Vec<RegionLocalObjectId>,
        /// How far the copies are offset from the originals, in metres.
        offset: Vector,
        /// The group the copies are set to (`None` for none).
        group_id: Option<GroupKey>,
        /// The `DuplicateFlags` bitfield the client sent.
        duplicate_flags: u32,
    },
    /// The client force-deleted a selection (`ObjectDelete`). The inverse of
    /// the client's [`Session::delete_objects`](crate::Session::delete_objects).
    ///
    /// This is the reference viewer's *force*-delete, which generally needs
    /// estate powers; the ordinary delete-to-trash is a
    /// [`DerezObjects`](Self::DerezObjects) with
    /// [`DeRezDestination::Trash`].
    ObjectsDeleted {
        /// The region-local ids of the objects being deleted.
        local_ids: Vec<RegionLocalObjectId>,
        /// Whether the client claims god powers for the deletion (`Force`).
        force: bool,
    },
    /// The client moved, rotated and/or resized an object
    /// (`MultipleObjectUpdate`). The inverse of the client's
    /// [`Session::update_object`](crate::Session::update_object). One event is
    /// emitted per object block in the message.
    ///
    /// Only the components the client is changing are [`Some`]; the rest keep
    /// whatever the object already has.
    ObjectTransformSet {
        /// The region-local id of the object being moved.
        local_id: RegionLocalObjectId,
        /// The components being changed, and the linkset/uniform modifiers.
        transform: ObjectTransform,
    },
    /// The client asked to undo its last edit of the named objects (`Undo`).
    /// The inverse of the client's
    /// [`Session::undo_objects`](crate::Session::undo_objects).
    ///
    /// The undo stack is the *simulator's*: the message names objects and
    /// nothing else, so what one step undoes is whatever the region recorded
    /// for them. The objects are named by full id, not region-local id, which
    /// is the one place in the object family that is true.
    ObjectsUndone {
        /// The objects whose last edit is to be undone.
        object_ids: Vec<ObjectKey>,
    },
    /// The client asked to redo an edit it had undone (`Redo`). The inverse of
    /// the client's [`Session::redo_objects`](crate::Session::redo_objects).
    ObjectsRedone {
        /// The objects whose undone edit is to be reapplied.
        object_ids: Vec<ObjectKey>,
    },
    /// The client selected objects (`ObjectSelect`) — the subscription a
    /// simulator answers with the full `ObjectProperties`
    /// ([`send_object_properties`](SimSession::send_object_properties)) and
    /// keeps pushing to while the selection stands. The inverse of the
    /// client's
    /// [`Session::request_object_properties`](crate::Session::request_object_properties).
    ObjectsSelected {
        /// The region-local ids of the objects now selected.
        local_ids: Vec<RegionLocalObjectId>,
    },
    /// The client dropped its selection of objects (`ObjectDeselect`), ending
    /// the subscription [`ObjectsSelected`](Self::ObjectsSelected) opened. The
    /// inverse of the client's
    /// [`Session::deselect_objects`](crate::Session::deselect_objects).
    ObjectsDeselected {
        /// The region-local ids of the objects no longer selected.
        local_ids: Vec<RegionLocalObjectId>,
    },
    /// The client saved the About Land form (`ParcelPropertiesUpdate`). The
    /// inverse of the client's
    /// [`Session::update_parcel`](crate::Session::update_parcel).
    ///
    /// The message carries the **whole** record, not the fields that changed:
    /// a viewer starts from what it last read, sets the one field the resident
    /// touched and sends everything back. A simulator therefore cannot tell an
    /// unchanged field from a re-asserted one — which is what makes a stale
    /// About Land floater able to revert somebody else's change without either
    /// of them noticing.
    ParcelPropertiesUpdated {
        /// The complete record the client is asserting.
        update: Box<ParcelUpdate>,
    },
    /// The client changed a parcel's allow or ban list
    /// (`ParcelAccessListUpdate`). The inverse of the client's
    /// [`Session::update_parcel_access_list`](crate::Session::update_parcel_access_list).
    ///
    /// Like the About Land form this is a whole-list assertion rather than a
    /// delta, and a long list is split across several messages — `sections`
    /// says how many the client is sending and `sequence_id` which of them this
    /// is.
    ParcelAccessListUpdated {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// Which list is being replaced.
        scope: ParcelAccessScope,
        /// The entries of this section of the list.
        entries: Vec<ParcelAccessEntry>,
        /// The client's correlation id for the whole multi-section update.
        transaction_id: TransactionId,
        /// Which section of the update this is.
        sequence_id: i32,
        /// How many sections the client is sending.
        sections: i32,
    },
    /// The client asked for a parcel's allow or ban list
    /// (`ParcelAccessListRequest`). The inverse of the client's
    /// [`Session::request_parcel_access_list`](crate::Session::request_parcel_access_list);
    /// a simulator answers with
    /// [`send_parcel_access_list_reply`](SimSession::send_parcel_access_list_reply).
    RequestParcelAccessList {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// Which list is wanted.
        scope: ParcelAccessScope,
        /// The client's sequence id, echoed in the reply.
        sequence_id: i32,
    },
    /// The client bought a parcel (`ParcelBuy`). The inverse of the client's
    /// [`Session::buy_parcel`](crate::Session::buy_parcel).
    ParcelBought {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// The group buying it, when the purchase is for a group.
        group_id: Option<GroupKey>,
        /// Whether the parcel is bought *by* the group rather than deeded to it
        /// later.
        is_group_owned: bool,
        /// Whether the buyer's land contribution to the group is to be removed.
        remove_contribution: bool,
        /// The price the client believes it is paying, in L$ — a simulator
        /// checks this against its own asking price rather than trusting it.
        price: LindenAmount,
        /// The area the client believes it is buying, in square metres.
        area: i32,
    },
    /// The client deeded a parcel to a group (`ParcelDeedToGroup`). The inverse
    /// of the client's
    /// [`Session::deed_parcel_to_group`](crate::Session::deed_parcel_to_group).
    ParcelDeededToGroup {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// The group the parcel is deeded to.
        group_id: GroupKey,
    },
    /// The client abandoned a parcel back to the estate (`ParcelRelease`). The
    /// inverse of the client's
    /// [`Session::release_parcel`](crate::Session::release_parcel).
    ParcelReleased {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
    },
    /// An estate manager reclaimed an abandoned parcel (`ParcelReclaim`). The
    /// inverse of the client's
    /// [`Session::reclaim_parcel`](crate::Session::reclaim_parcel).
    ParcelReclaimed {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
    },
    /// The client returned objects on a parcel to their owners
    /// (`ParcelReturnObjects`). The inverse of the client's
    /// [`Session::return_parcel_objects`](crate::Session::return_parcel_objects).
    ParcelObjectsReturned {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// Which class of objects is being returned.
        return_type: ParcelReturnType,
        /// The specific objects named, when the client is returning a
        /// selection rather than a class.
        task_ids: Vec<ObjectKey>,
        /// The owners whose objects are being returned.
        owner_ids: Vec<OwnerKey>,
    },
    /// The client asked the simulator to highlight a class of objects on a
    /// parcel (`ParcelSelectObjects`) — the "show me what I would be returning"
    /// half of the About Land objects panel. The inverse of the client's
    /// [`Session::select_parcel_objects`](crate::Session::select_parcel_objects);
    /// a simulator answers with
    /// [`send_force_object_select`](SimSession::send_force_object_select).
    ParcelObjectsSelected {
        /// The parcel's region-local id.
        local_id: RegionLocalParcelId,
        /// Which class of objects to highlight.
        return_type: ParcelReturnType,
        /// The owners whose objects to highlight.
        owner_ids: Vec<OwnerKey>,
    },
    /// The client asked for a region's top-scripts or top-colliders report
    /// (`LandStatRequest`). The inverse of the client's
    /// [`Session::request_land_stat`](crate::Session::request_land_stat); a
    /// simulator answers with
    /// [`send_land_stat_reply`](SimSession::send_land_stat_reply).
    RequestLandStat {
        /// Which report is wanted.
        report_type: LandStatReportType,
        /// The request flags (the reference sends the filter mode here).
        request_flags: u32,
        /// The name filter, empty for none.
        filter: String,
        /// The parcel the report is scoped to, or the whole region when zero.
        local_id: RegionLocalParcelId,
    },
    /// The client asked for the region's configuration (`RequestRegionInfo`) —
    /// the Region/Estate floater's first round trip. The inverse of the
    /// client's [`Session::request_region_info`](crate::Session::request_region_info);
    /// a simulator answers with
    /// [`send_region_info`](SimSession::send_region_info).
    RequestRegionInfo,
    /// The client rezzed a **new** primitive from a shape it built itself
    /// (`ObjectAdd`). The inverse of the client's
    /// [`Session::rez_object`](crate::Session::rez_object).
    ///
    /// A simulator answers by minting the object's region-local and full ids,
    /// adding it to the region, and streaming it back in an `ObjectUpdate`
    /// ([`send_object_update`](SimSession::send_object_update)) — which is how
    /// the rezzing client learns the ids it did not choose.
    ///
    /// Distinct from [`RezObjectFromInventory`](Self::RezObjectFromInventory),
    /// which is the `RezObject` *message* and rezzes an existing inventory
    /// item: this one creates a prim from nothing.
    RezObject {
        /// The shape of the new prim and the ray the client placed it with.
        params: AddPrimParams,
    },
    /// The client derezzed one or more in-world objects — a take, a save, a
    /// return, a delete to trash (`DeRezObject`). The inverse of the client's
    /// [`Session::derez_objects`](crate::Session::derez_objects).
    ///
    /// What the simulator does with them is entirely
    /// [`destination`](Self::DerezObjects::destination)'s business:
    /// [`DeRezDestination::agent_folder`] names the folder an inventory item is
    /// minted in (answered with an `UpdateCreateInventoryItem`) and
    /// [`DeRezDestination::removes_from_world`] says whether the world copy
    /// then goes (answered with a `KillObject`). A destination that does
    /// neither is acknowledged with a
    /// [`send_derez_ack`](SimSession::send_derez_ack).
    DerezObjects {
        /// The region-local ids of the objects being derezzed. A viewer sends
        /// its whole selection, so this is a batch even for one object.
        local_ids: Vec<RegionLocalObjectId>,
        /// Where the objects go.
        destination: DeRezDestination,
        /// The client's transaction id, echoed back in a `DeRezAck` or in the
        /// `UpdateCreateInventoryItem` the take produces.
        transaction_id: TransactionId,
        /// The active group the derez is performed under (`None` for none).
        group_id: Option<GroupKey>,
        /// How many packets the client is splitting its selection across, and
        /// which of them this is (`PacketCount` / `PacketNumber`). A viewer
        /// sends `1` / `0` for any selection that fits one message, which is
        /// every selection a viewer actually makes.
        packet: (u8, u8),
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
    /// The client rewrote the metadata of one or more **agent** inventory items
    /// (`UpdateInventoryItem`). The inverse of the client's
    /// [`Session::update_inventory_item`](crate::Session::update_inventory_item)
    /// and of the second half of
    /// [`Session::save_inventory_asset`](crate::Session::save_inventory_asset).
    ///
    /// This is also how a **wearable save** binds its asset: the legacy
    /// transaction upload has no capability and no reply naming an item, so the
    /// client sends the bytes as an `AssetUploadRequest` and then names the item
    /// they belong to here, correlated by the transaction id alone. A simulator
    /// that ignores this message stores the bytes and leaves the item pointing
    /// at the asset it had before — a save that looks like it worked and
    /// silently did not.
    UpdateAgentInventoryItems {
        /// One entry per `InventoryData` block, in wire order.
        items: Vec<UpdatedInventoryItem>,
        /// The `AgentData` transaction id, which the client repeats on every
        /// block ([`UpdatedInventoryItem::bound_asset`] is derived per item).
        transaction_id: TransactionId,
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
    /// The ping the simulator is waiting on a `CompletePingCheck` for, and when
    /// it went out — the round trip the retransmission timeout is derived from.
    outstanding_ping: Option<(PingId, Instant)>,
    /// The relaxed average of the measured round trip to the client, clamped to
    /// `PING_AVERAGE_MIN ..= PING_AVERAGE_MAX`. Drives the retransmission
    /// timeout ([`SimSession::resend_timeout`]).
    ping_average: Duration,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<SequenceNumber>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<SequenceNumber, UnackedPacket>,
    /// Recently seen inbound reliable sequence numbers.
    seen: SeenWindow,
    /// Datagrams ready to be transmitted to the client.
    out: VecDeque<SimOutbound>,
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
    /// back in the `TransferInfo` (as the reference serving side does) plus the
    /// deadline past which the simulator answers the request itself.
    transfer_serves: BTreeMap<TransferId, SimTransferServe>,
    /// Whether this circuit hosts a child or the root agent: `Child` from
    /// `UseCircuitCode`, promoted to `Root` by `CompleteAgentMovement`.
    agent_presence: AgentPresence,
    /// Where the agent lands when its movement completes — the position and
    /// facing the `AgentMovementComplete` reply carries. The region centre
    /// facing +X unless [`SimSession::set_arrival_position`] placed the
    /// arrival (a teleport lands where the request asked).
    arrival: ArrivalPlacement,
    /// The agent's sit state (the server-side mirror of the client's sit
    /// machine).
    sit: SimSitState,
    /// When an outstanding sit offer is withdrawn for want of the client's
    /// completing `AgentSit`. `Some` exactly while
    /// [`SimSitState::ResponseSent`].
    sit_expires: Option<Instant>,
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
    /// What the simulator holds the agent to be wearing, and the serial it
    /// stamps that state with — the `AgentWearablesUpdate` an
    /// `AgentWearablesRequest` is answered from
    /// ([`SimSession::set_agent_wearables`]). Held rather than derived from
    /// [`agent_inventory`](Self::agent_inventory) because a simulator holds it
    /// too: the outfit is appearance state the viewer updates with
    /// `AgentIsNowWearing`, and the Current Outfit Folder is the inventory
    /// record that shadows it.
    agent_wearables: (u32, Vec<Wearable>),
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

/// One `InventoryData` block of an `UpdateInventoryItem`
/// ([`ServerEvent::UpdateAgentInventoryItems`]): the item as the client sent it,
/// the callback id it wants echoed, and the asset its transaction binds to it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdatedInventoryItem {
    /// The item's fields as the client sent them. The block carries **no asset
    /// id** — an item's asset is named by [`bound_asset`](Self::bound_asset) or
    /// left as it was.
    pub item: RestoreItem,
    /// The client's callback id for this block, echoed in the
    /// `UpdateCreateInventoryItem` that confirms the write.
    pub callback_id: InventoryCallbackId,
    /// The asset id this block's transaction binds to the item —
    /// `combine(transaction_id, secure_session_id)`, the same derivation the
    /// matching `AssetUploadRequest` used, so the two halves of a wearable save
    /// meet at one id. `None` when the block's transaction id is nil (a
    /// metadata-only edit: a rename, a permissions change) or when the circuit
    /// has no secure session id to derive it from.
    pub bound_asset: Option<AssetKey>,
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

    /// The inventory item this upload replaces the asset of, when it replaces
    /// one rather than creating one.
    ///
    /// The completion's `new_inventory_item` is this id for every `Update*`
    /// family, **not** a freshly minted one: OpenSim's `ItemUpdater` answers
    /// `uploadComplete.new_inventory_item = m_inventoryItemID`, and it has to —
    /// the item already exists, and handing the client an id nothing holds
    /// would have it file a second copy of a notecard it only edited.
    /// `NewFileAgentInventory` is the one family that mints an id (it creates
    /// the item), and `UploadBakedTexture` has no item at all.
    pub(crate) const fn replaced_item(&self) -> Option<InventoryKey> {
        match self {
            Self::NewFileInventory(_) | Self::BakedTexture => None,
            Self::UpdateAgentItem { item_id, .. } | Self::UpdateTaskItem { item_id, .. } => {
                Some(*item_id)
            }
            Self::UpdateScriptAgent(request) => Some(request.item_id),
            Self::UpdateScriptTask(request) => Some(request.item_id),
        }
    }

    /// Whether this upload **creates** an inventory item, minting its id.
    /// `NewFileAgentInventory` alone does; see [`replaced_item`](Self::replaced_item).
    const fn creates_inventory_item(&self) -> bool {
        matches!(self, Self::NewFileInventory(_))
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
/// default sky-track altitude breakpoints, and a **single-keyframe** day cycle
/// named "Default Daycycle" carrying the reference viewer's own default sky and
/// water frames ([`SkySettings::legacy_windlight_default`],
/// [`WaterSettings::legacy_default`] — `LLSettingsSky::defaults` /
/// `LLSettingsWater::defaults`).
///
/// The cycle was empty, and an empty one is not a neutral choice: it says
/// nothing about the sky, so every client renders its *own* built-in default
/// instead. Two viewers pointed at the same fake region then disagree for
/// reasons that have nothing to do with either renderer, which makes the
/// comparison this grid exists to support meaningless. Serving a real frame
/// makes the sky wire-determined — both our viewer and Firestorm draw the
/// bytes the region sent.
///
/// **One** keyframe, deliberately. A cycle with two frames renders differently
/// depending on the region clock, so two captures minutes apart would not be
/// comparable either; with a single keyframe the day position cannot change
/// anything. A fixture that *wants* a moving sky sets its own environment
/// (`RegionFixture::environment`) — `sl_test_assets::environment::day_cycle`
/// is one — rather than making every region's captures time-dependent.
fn default_region_environment() -> EnvironmentSettings {
    let sky = SkySettings::legacy_windlight_default(DEFAULT_SKY_FRAME);
    let water = WaterSettings::legacy_default(DEFAULT_WATER_FRAME);
    // The two frames are named apart because sky and water frames share one
    // name namespace on the wire (see `DayCycle`): a same-named pair encodes to
    // a single map entry and the sky is the one lost.
    let keyframe = |name: &str| {
        vec![DayCycleFrame {
            keyframe: 0.0,
            name: name.to_owned(),
        }]
    };
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
            water_track: keyframe(DEFAULT_WATER_FRAME),
            sky_tracks: vec![keyframe(DEFAULT_SKY_FRAME)],
            sky_frames: BTreeMap::from([(DEFAULT_SKY_FRAME.to_owned(), sky)]),
            water_frames: BTreeMap::from([(DEFAULT_WATER_FRAME.to_owned(), water)]),
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
            outstanding_ping: None,
            ping_average: INITIAL_PING_AVERAGE,
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
            arrival: ArrivalPlacement::default(),
            sit: SimSitState::NotSitting,
            sit_expires: None,
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
            agent_wearables: (0, Vec::new()),
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

    /// The display-name records matching `names` — the `AvatarPickerSearch`
    /// capability's lookup over the same store [`SimSession::set_display_name`]
    /// fills. At most `page_size` records, in the store's (id) order.
    ///
    /// Searching the **username and display name** next to the legacy name is
    /// the whole point of the modern picker; the legacy UDP one knew only the
    /// legacy name. The query is tokenised on whitespace and **every** token
    /// must appear somewhere in a record's searchable text, so `"marina
    /// vector"` finds `marina.vector` — the client turns a typed `.` into a
    /// space before sending, and this normalises the stored side the same way.
    /// A blank query matches nobody rather than everybody.
    pub(crate) fn search_display_names(&self, names: &str, page_size: u32) -> Vec<DisplayName> {
        let needle = names.to_lowercase();
        let tokens: Vec<&str> = needle.split_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }
        self.display_names
            .values()
            .filter(|name| {
                let haystack = format!(
                    "{} {} {} {}",
                    name.username, name.display_name, name.legacy_first_name, name.legacy_last_name
                )
                .to_lowercase()
                .replace('.', " ");
                tokens.iter().all(|token| haystack.contains(token))
            })
            .take(usize::try_from(page_size).unwrap_or(usize::MAX))
            .cloned()
            .collect()
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

    /// Starts an ad-hoc conference on behalf of this circuit's agent (the
    /// `ChatSessionRequest` `"start conference"` server side, the modern
    /// counterpart of the [`ImDialog::SessionConferenceStart`] instant message
    /// this session also accepts): registers the session with the starter and
    /// the invitees in its roster and pushes a
    /// [`ServerEvent::ConferenceStartRequested`] so the driver relays the
    /// invitations, and returns the roster for the cap reply. The starter is
    /// added when the circuit knows one (it is the same tolerance
    /// [`SimSession::chat_session_accept`] applies).
    ///
    /// The `session_id` is the **temporary** one the client minted; a driver
    /// that re-keys the session answers with
    /// [`SimSession::enqueue_chatterbox_session_start_reply`].
    pub(crate) fn chat_session_start_conference(
        &mut self,
        session_id: ImSessionId,
        invitees: &[AgentKey],
    ) -> Vec<AgentKey> {
        let starter = self.agent_id;
        let chat_session = self
            .chat_sessions
            .entry(session_id)
            .or_insert_with(|| SimChatSession {
                kind: SimChatSessionKind::Conference,
                participants: BTreeSet::new(),
                history: Vec::new(),
            });
        chat_session.participants.extend(starter);
        chat_session.participants.extend(invitees.iter().copied());
        let roster = chat_session.participants.iter().copied().collect();
        self.events
            .push_back(ServerEvent::ConferenceStartRequested {
                session_id,
                invitees: invitees.to_vec(),
                message: String::new(),
            });
        roster
    }

    /// Invites more agents into an already-open session (the
    /// `ChatSessionRequest` `"invite"` server side): adds them to the roster
    /// and pushes a [`ServerEvent::SessionInviteRequested`] so the driver
    /// relays the invitations. Returns the grown roster for the cap reply, or
    /// `None` for a session this simulator does not know — unlike a start,
    /// this names a session that is supposed to exist.
    pub(crate) fn chat_session_invite(
        &mut self,
        session_id: ImSessionId,
        invitees: &[AgentKey],
    ) -> Option<Vec<AgentKey>> {
        let chat_session = self.chat_sessions.get_mut(&session_id)?;
        chat_session.participants.extend(invitees.iter().copied());
        let roster = chat_session.participants.iter().copied().collect();
        self.events.push_back(ServerEvent::SessionInviteRequested {
            session_id,
            invitees: invitees.to_vec(),
        });
        Some(roster)
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
        let new_inventory_item = metadata.replaced_item().or_else(|| {
            metadata
                .creates_inventory_item()
                .then(|| InventoryKey::from(Uuid::from_u128(self.next_serial())))
        });
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

    /// This region's handle — the grid position the session was constructed
    /// with, and what a caller stamps the region's own content (terrain
    /// patches, objects) with.
    #[must_use]
    pub const fn region_handle(&self) -> RegionHandle {
        self.region_handle
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

    /// What the simulator holds this agent to be wearing, and the serial that
    /// state carries — the pair
    /// [`send_agent_wearables_update`](Self::send_agent_wearables_update)
    /// answers an `AgentWearablesRequest` with.
    #[must_use]
    pub fn agent_wearables(&self) -> (u32, &[Wearable]) {
        let (serial, ref wearables) = self.agent_wearables;
        (serial, wearables)
    }

    /// Replaces what the simulator holds this agent to be wearing, advancing
    /// the serial — the driver/test population API.
    ///
    /// The serial advances on every call rather than being passed in, because
    /// that is the one rule the number has: a receiver drops an update whose
    /// serial is not newer than the last it applied, so a fixture that changes
    /// the outfit without moving it on would have its change ignored.
    pub fn set_agent_wearables(&mut self, wearables: Vec<Wearable>) {
        let (serial, _previous) = &self.agent_wearables;
        self.agent_wearables = (serial.saturating_add(1), wearables);
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
    ///
    /// A closed session queues nothing: this is the single funnel every one of
    /// the typed `send_*`/`enqueue_*` helpers goes through, so gating it here is
    /// what stops a driver from talking to a client that is already gone.
    /// Datagrams queued *before* the close still drain — that is how the
    /// goodbye packet of a clean logout or a retired circuit reaches the client.
    fn send(
        &mut self,
        message: &AnyMessage,
        reliability: Reliability,
        now: Instant,
    ) -> Result<(), WireError> {
        if self.is_closed() {
            return Ok(());
        }
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

        let tracked = if matches!(reliability, Reliability::Reliable) {
            self.unacked.insert(
                sequence,
                UnackedPacket {
                    datagram: datagram.clone(),
                    sent_at: now,
                    queued: true,
                    attempts: 1,
                    name: sl_wire::message_name(message.id()),
                    severity: severity_of(message),
                },
            );
            Some(sequence)
        } else {
            None
        };
        self.out.push_back(SimOutbound {
            sequence: tracked,
            payload: datagram,
        });
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

    /// Sends an `EconomyData`: the grid's L$ price list and this region's
    /// object budget, in response to a client's `EconomyDataRequest` (surfaced
    /// as [`ServerEvent::RequestEconomyData`]). The inverse of the client's
    /// [`Event::EconomyData`](crate::Event::EconomyData) decode. Sent
    /// reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode — which for this message means a price or a
    /// capacity too large for its signed 32-bit wire field.
    pub fn send_economy_data(&mut self, economy: &EconomyData, now: Instant) -> Result<(), Error> {
        use crate::types::{land_impact_to_wire, linden_to_wire};
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::EconomyData(EconomyDataMessage {
            info: EconomyDataInfoBlock {
                object_capacity: land_impact_to_wire("ObjectCapacity", economy.object_capacity)?,
                object_count: land_impact_to_wire("ObjectCount", economy.object_count)?,
                price_energy_unit: linden_to_wire("PriceEnergyUnit", &economy.price_energy_unit)?,
                price_object_claim: linden_to_wire(
                    "PriceObjectClaim",
                    &economy.price_object_claim,
                )?,
                price_public_object_decay: linden_to_wire(
                    "PricePublicObjectDecay",
                    &economy.price_public_object_decay,
                )?,
                price_public_object_delete: linden_to_wire(
                    "PricePublicObjectDelete",
                    &economy.price_public_object_delete,
                )?,
                price_parcel_claim: linden_to_wire(
                    "PriceParcelClaim",
                    &economy.price_parcel_claim,
                )?,
                price_parcel_claim_factor: economy.price_parcel_claim_factor,
                price_upload: linden_to_wire("PriceUpload", &economy.price_upload)?,
                price_rent_light: linden_to_wire("PriceRentLight", &economy.price_rent_light)?,
                teleport_min_price: linden_to_wire(
                    "TeleportMinPrice",
                    &economy.teleport_min_price,
                )?,
                teleport_price_exponent: economy.teleport_price_exponent,
                energy_efficiency: economy.energy_efficiency,
                price_object_rent: economy.price_object_rent,
                price_object_scale_factor: economy.price_object_scale_factor,
                price_parcel_rent: linden_to_wire("PriceParcelRent", &economy.price_parcel_rent)?,
                price_group_create: linden_to_wire(
                    "PriceGroupCreate",
                    &economy.price_group_create,
                )?,
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

    /// Sends an `EstateOwnerMessage`: the simulator's half of the estate
    /// channel, which is one message carrying a **method name** and a list of
    /// byte parameters rather than a message per answer.
    ///
    /// The parameters are bytes, not strings, because the channel's are: a
    /// `setaccess` reply carries raw 16-byte UUIDs in the same field an
    /// `estateupdateinfo` carries NUL-terminated decimal text.
    /// [`send_estate_info`](Self::send_estate_info) and
    /// [`send_estate_access_list`](Self::send_estate_access_list) build the two
    /// the client decodes; this is what everything else goes out through.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_estate_owner_message(
        &mut self,
        method: &str,
        invoice: Uuid,
        params: &[Vec<u8>],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
            agent_data: EstateOwnerMessageAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |agent| agent.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
                transaction_id: Uuid::nil(),
            },
            method_data: EstateOwnerMessageMethodDataBlock {
                method: with_nul(method),
                invoice,
            },
            param_list: params
                .iter()
                .map(|parameter| EstateOwnerMessageParamListBlock {
                    parameter: parameter.clone(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends the `estateupdateinfo` answer to an estate `getinfo`: the estate's
    /// name, owner, flags, sun and covenant, as the ten text parameters the
    /// method is defined by.
    ///
    /// Parameter 8 is the literal `"1"` every simulator sends and no viewer
    /// reads — OpenSim's own source marks it "what is this?". It is sent
    /// because the parameters are positional: leaving it out would move the
    /// abuse-report address into its place.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_estate_info(
        &mut self,
        info: &EstateInfo,
        invoice: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let text = |value: &str| with_nul(value);
        let params = vec![
            text(&info.estate_name),
            text(&info.estate_owner.to_string()),
            text(&info.estate_id.to_string()),
            text(&info.estate_flags.to_string()),
            text(&info.sun_position.to_string()),
            text(&info.parent_estate.to_string()),
            text(&info.covenant_id.unwrap_or_else(Uuid::nil).to_string()),
            text(&info.covenant_timestamp.to_string()),
            text("1"),
            text(&info.abuse_email),
        ];
        self.send_estate_owner_message(ESTATE_UPDATE_INFO_METHOD, invoice, &params, now)
    }

    /// Sends one of the estate's four access lists as a `setaccess`.
    ///
    /// The layout is positional and shared with every simulator: the estate id,
    /// the single category bit, one count per category (the count of *this*
    /// list against its own category and zero against the other three), and
    /// then the members as raw 16-byte ids rather than as text.
    ///
    /// An empty list is still sent: "nobody is banned" is an answer, and a
    /// viewer that receives nothing cannot tell it from a reply that was lost.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_estate_access_list(
        &mut self,
        estate_id: u32,
        kind: EstateAccessKind,
        members: &[Uuid],
        invoice: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let code = estate_access_code(kind);
        let count = members.len().to_string();
        let zero = "0".to_owned();
        let per_category = |bit: u32| {
            with_nul(if code & bit == 0 {
                zero.as_str()
            } else {
                count.as_str()
            })
        };
        let mut params = vec![
            with_nul(&estate_id.to_string()),
            with_nul(&code.to_string()),
            per_category(ESTATE_ACCESS_ALLOWED_AGENTS),
            per_category(ESTATE_ACCESS_ALLOWED_GROUPS),
            per_category(ESTATE_ACCESS_BANNED_AGENTS),
            per_category(ESTATE_ACCESS_MANAGERS),
        ];
        params.extend(members.iter().map(|member| member.as_bytes().to_vec()));
        self.send_estate_owner_message(SET_ACCESS_METHOD, invoice, &params, now)
    }

    /// Sends a `ParcelAccessListReply`: one parcel's allow or ban list, in
    /// response to a client's `ParcelAccessListRequest` (surfaced as
    /// [`ServerEvent::RequestParcelAccessList`]).
    ///
    /// An **empty** list goes out as a single nil-agent placeholder block rather
    /// than as no blocks at all, which is what a simulator does and what a
    /// viewer reads as "this list is empty" — a reply with no blocks reads as no
    /// reply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_parcel_access_list_reply(
        &mut self,
        local_id: RegionLocalParcelId,
        scope: ParcelAccessScope,
        sequence_id: i32,
        entries: &[ParcelAccessEntry],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let list: Vec<ParcelAccessListReplyListBlock> = if entries.is_empty() {
            vec![ParcelAccessListReplyListBlock {
                id: Uuid::nil(),
                time: 0,
                flags: 0,
            }]
        } else {
            entries
                .iter()
                .map(|entry| ParcelAccessListReplyListBlock {
                    id: entry.id,
                    time: entry.time,
                    flags: entry.flags.0,
                })
                .collect()
        };
        let message = AnyMessage::ParcelAccessListReply(ParcelAccessListReply {
            data: ParcelAccessListReplyDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |agent| agent.uuid()),
                sequence_id,
                flags: scope.to_u32(),
                local_id: local_id.0,
            },
            list,
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a `RegionInfo`: the region's configuration and limits, in response
    /// to a client's `RequestRegionInfo` (surfaced as
    /// [`ServerEvent::RequestRegionInfo`]) — the Region/Estate floater's first
    /// round trip.
    ///
    /// The optional blocks are sent when [`RegionLimits`] carries them: the
    /// extended region flags always (a modern grid sends them), and the chat and
    /// combat settings only when the region has any, since an absent block and
    /// an all-zero one are different answers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_region_info(&mut self, limits: &RegionLimits, now: Instant) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::RegionInfo(RegionInfoMessage {
            agent_data: RegionInfoAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |agent| agent.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            region_info: RegionInfoRegionInfoBlock {
                sim_name: with_nul(&sl_wire::region_name_to_wire(limits.sim_name.as_ref())),
                estate_id: limits.estate_id,
                parent_estate_id: limits.parent_estate_id,
                region_flags: limits.region_flags,
                sim_access: limits.maturity.to_sim_access(),
                // The legacy 8-bit cap saturates rather than wrapping: a region
                // that allows more agents than a byte can say is reported as
                // "as many as this field can hold", and the 32-bit field below
                // carries the real number.
                max_agents: u8::try_from(limits.max_agents).unwrap_or(u8::MAX),
                billable_factor: limits.billable_factor,
                object_bonus_factor: limits.object_bonus_factor,
                water_height: limits.water_height,
                terrain_raise_limit: limits.terrain_raise_limit,
                terrain_lower_limit: limits.terrain_lower_limit,
                price_per_meter: crate::types::linden_to_wire(
                    "PricePerMeter",
                    &limits.price_per_meter,
                )?,
                redirect_grid_x: limits.redirect_grid_x,
                redirect_grid_y: limits.redirect_grid_y,
                use_estate_sun: limits.use_estate_sun,
                sun_hour: limits.sun_hour,
            },
            region_info2: RegionInfoRegionInfo2Block {
                product_sku: with_nul(""),
                product_name: with_nul(""),
                max_agents32: limits.max_agents,
                hard_max_agents: limits.hard_max_agents,
                hard_max_objects: limits.hard_max_objects,
            },
            region_info3: vec![RegionInfoRegionInfo3Block {
                region_flags_extended: limits.region_flags_extended,
            }],
            region_info5: limits
                .chat_settings
                .as_ref()
                .map(|chat| RegionInfoRegionInfo5Block {
                    chat_whisper_range: chat.whisper_range,
                    chat_normal_range: chat.normal_range,
                    chat_shout_range: chat.shout_range,
                    chat_whisper_offset: chat.whisper_offset,
                    chat_normal_offset: chat.normal_offset,
                    chat_shout_offset: chat.shout_offset,
                    chat_flags: chat.flags,
                })
                .into_iter()
                .collect(),
            combat_settings: limits
                .combat_settings
                .as_ref()
                .map(|combat| RegionInfoCombatSettingsBlock {
                    combat_flags: combat.flags,
                    on_death: combat.on_death,
                    damage_throttle: combat.damage_throttle,
                    regeneration_rate: combat.regeneration_rate,
                    invulnerabily_time: combat.invulnerability_time,
                    damage_limit: combat.damage_limit,
                })
                .into_iter()
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `ObjectProperties`: an object's **full** properties — its name,
    /// description, creator, permissions, sale state and task-inventory serial
    /// — which a simulator sends to every client holding the object *selected*
    /// (surfaced as [`ServerEvent::ObjectsSelected`]), and again whenever any
    /// of them changes.
    ///
    /// This is the read side of the whole object-edit family: an `ObjectUpdate`
    /// carries none of these fields, so a client that renames an object learns
    /// the rename took only from this message. The condensed
    /// [`send_object_properties_family`](Self::send_object_properties_family)
    /// is the hover / pay-dialog form and needs no selection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_object_properties(
        &mut self,
        properties: &ObjectProperties,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let object_owner_wire =
            crate::types::object_owner_to_wire(properties.owner, properties.group);
        let mut texture_ids = Vec::new();
        for texture in &properties.texture_ids {
            texture_ids.extend_from_slice(texture.uuid().as_bytes());
        }
        let message = AnyMessage::ObjectProperties(ObjectPropertiesMessage {
            object_data: vec![ObjectPropertiesObjectDataBlockMessage {
                object_id: properties.object_id.uuid(),
                creator_id: properties.creator_id.uuid(),
                owner_id: object_owner_wire.0,
                group_id: object_owner_wire.1,
                creation_date: properties.creation_date,
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
                aggregate_perms: properties.aggregate_perms,
                aggregate_perm_textures: properties.aggregate_perm_textures,
                aggregate_perm_textures_owner: properties.aggregate_perm_textures_owner,
                category: properties.category,
                inventory_serial: properties.inventory_serial,
                item_id: properties.item_id.uuid(),
                folder_id: properties.folder_id.map_or_else(Uuid::nil, |id| id.uuid()),
                from_task_id: properties
                    .from_task_id
                    .map_or_else(Uuid::nil, |id| id.uuid()),
                last_owner_id: properties.last_owner_id,
                name: with_nul(&properties.name),
                description: with_nul(&properties.description),
                touch_name: with_nul(&properties.touch_name),
                sit_name: with_nul(&properties.sit_name),
                texture_id: texture_ids,
            }],
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

    /// Sends a `ParcelDwellReply`: one parcel's dwell (traffic) score, in
    /// response to a client's `ParcelDwellRequest` (surfaced as
    /// [`ServerEvent::RequestParcelDwell`] — echo its `local_id` back).
    /// Surfaces on the client as [`Event::ParcelDwell`](crate::Event::ParcelDwell).
    ///
    /// Unlike the request, the reply *does* carry the parcel's grid-wide id:
    /// it is the simulator that knows it, which is why the same lookup
    /// answers [`send_parcel_info_reply`](Self::send_parcel_info_reply).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_parcel_dwell_reply(
        &mut self,
        local_id: RegionLocalParcelId,
        parcel_id: ParcelKey,
        dwell: f32,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let RegionLocalParcelId(local_id) = local_id;
        let message = AnyMessage::ParcelDwellReply(ParcelDwellReply {
            agent_data: ParcelDwellReplyAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
            },
            data: ParcelDwellReplyDataBlock {
                local_id,
                parcel_id: parcel_id.uuid(),
                dwell,
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

    /// Sends one `LayerData` message carrying `patches` of `layer` — the
    /// patched-DCT ground heights (`Land`), wind field (`Wind`) or cloud
    /// densities (`Cloud`) a simulator streams at a viewer, and the inverse of
    /// the client's [`Event::TerrainPatch`](crate::Event::TerrainPatch) decode.
    /// The message carries no region handle; the client labels each patch with
    /// the handle it learned from that circuit's first `ObjectUpdate`, so a
    /// region's own avatar must be rezzed before its ground is sent.
    ///
    /// Every patch goes into this one message, so the caller keeps the group
    /// small enough for a datagram — see
    /// [`send_terrain`](Self::send_terrain), which does that for a whole
    /// region's ground. An empty `patches` sends nothing. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_layer_data(
        &mut self,
        layer: TerrainLayerType,
        patches: &[TerrainPatch],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        if patches.is_empty() {
            return Ok(());
        }
        let message = AnyMessage::LayerData(LayerData {
            layer_id: LayerDataLayerIDBlock {
                r#type: layer.code(),
            },
            layer_data: LayerDataLayerDataBlock {
                data: crate::terrain::encode_layer(layer, patches),
            },
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends a whole region's ground as the sequence of `LayerData` messages a
    /// simulator emits on region entry: at most
    /// [`TERRAIN_PATCHES_PER_MESSAGE`] patches per message, walked in
    /// OpenSim's spiral order (`LLClientView.SendLayerTopRight` /
    /// `SendLayerBottomLeft`) — the outer ring of the patch grid from its
    /// south-west corner (east along the south edge, north up the east edge,
    /// west back along the north edge, south down the west edge), then the
    /// next ring in, so the region fills from its edges inwards as the patches
    /// arrive.
    ///
    /// The layer is the first patch's; patches of any other layer are skipped,
    /// since one message carries a single layer. Patches are addressed by their
    /// `(patch_x, patch_y)` grid position, so a coordinate given twice keeps
    /// only the first — the wind layer, whose two patches share position
    /// `(0, 0)`, goes through [`send_layer_data`](Self::send_layer_data)
    /// instead.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if a message fails to encode.
    pub fn send_terrain(&mut self, patches: &[TerrainPatch], now: Instant) -> Result<(), Error> {
        let Some(first) = patches.first() else {
            return Ok(());
        };
        let layer = first.layer;
        let mut by_position: BTreeMap<(u32, u32), &TerrainPatch> = BTreeMap::new();
        for patch in patches.iter().filter(|patch| patch.layer == layer) {
            by_position
                .entry((patch.patch_x, patch.patch_y))
                .or_insert(patch);
        }
        let (max_x, max_y) = by_position.keys().fold((0, 0), |(max_x, max_y), &(x, y)| {
            (max_x.max(x), max_y.max(y))
        });
        let ordered: Vec<TerrainPatch> = spiral_patch_order(max_x, max_y)
            .into_iter()
            .filter_map(|position| by_position.get(&position).map(|patch| (*patch).clone()))
            .collect();
        for group in ordered.chunks(TERRAIN_PATCHES_PER_MESSAGE) {
            self.send_layer_data(layer, group, now)?;
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

    /// Sends an `AvatarAppearance` — another avatar's baked textures, visual
    /// parameters, hover height and attachment list (the inverse of the
    /// client's [`Event::AvatarAppearance`](crate::Event::AvatarAppearance)).
    /// A simulator pushes one per avatar in view whenever that avatar's
    /// appearance changes, and once for each avatar already present when an
    /// agent arrives.
    ///
    /// The [`texture_entry`](AvatarAppearance::texture_entry) is encoded as a
    /// full per-avatar `TextureEntry` whose
    /// [`avatar_texture`](crate::avatar_texture) baked slots name the
    /// composited textures the receiver fetches; the optional
    /// `AppearanceData` block is written whenever the record carries any of
    /// its three fields. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_avatar_appearance(
        &mut self,
        appearance: &AvatarAppearance,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let appearance_data = if appearance.appearance_version.is_some()
            || appearance.cof_version.is_some()
            || appearance.appearance_flags.is_some()
        {
            vec![AvatarAppearanceAppearanceDataBlock {
                appearance_version: appearance.appearance_version.unwrap_or(0),
                cof_version: appearance.cof_version.unwrap_or(0),
                flags: appearance.appearance_flags.unwrap_or(0),
            }]
        } else {
            Vec::new()
        };
        let message = AnyMessage::AvatarAppearance(AvatarAppearanceWire {
            sender: AvatarAppearanceSenderBlock {
                id: appearance.avatar_id.uuid(),
                is_trial: appearance.is_trial,
            },
            object_data: AvatarAppearanceObjectDataBlock {
                texture_entry: crate::appearance::encode_texture_entry(&appearance.texture_entry),
            },
            visual_param: appearance
                .visual_params
                .iter()
                .map(|&param_value| AvatarAppearanceVisualParamBlock { param_value })
                .collect(),
            appearance_data,
            appearance_hover: appearance
                .hover_height
                .iter()
                .map(|hover_height| AvatarAppearanceAppearanceHoverBlock {
                    hover_height: hover_height.clone(),
                })
                .collect(),
            attachment_block: appearance
                .attachments
                .iter()
                .map(|attachment| AvatarAppearanceAttachmentBlockBlock {
                    id: attachment.id.uuid(),
                    attachment_point: attachment.attachment_point,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `AgentWearablesUpdate`: the simulator's view of what **this**
    /// session's agent is wearing, as it pushes unsolicited at login and after
    /// every wearable change, and in answer to an `AgentWearablesRequest`
    /// (surfaced as [`ServerEvent::RequestAgentWearables`]). Surfaces on the
    /// client as [`Event::AgentWearables`](crate::Event::AgentWearables). Sent
    /// reliably.
    ///
    /// `serial` is the outfit's serial number, which the receiver uses to drop
    /// an update that overtook a newer one; it has to advance on every change
    /// the simulator makes, which is the caller's business rather than this
    /// session's.
    ///
    /// A [`Wearable`] with no `asset_id` goes out nil, as a simulator sends one
    /// whose asset it has not resolved (the client then falls back to its
    /// inventory record).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_agent_wearables_update(
        &mut self,
        serial: u32,
        wearables: &[Wearable],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::AgentWearablesUpdate(AgentWearablesUpdate {
            agent_data: AgentWearablesUpdateAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
                serial_num: serial,
            },
            wearable_data: wearables
                .iter()
                .map(|wearable| AgentWearablesUpdateWearableDataBlock {
                    item_id: wearable.item_id.uuid(),
                    asset_id: wearable.asset_id.unwrap_or_else(Uuid::nil),
                    wearable_type: wearable.wearable_type.to_code(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `AvatarAnimation` — the complete, authoritative set of
    /// animations an avatar is now playing (the inverse of the client's
    /// [`Event::AvatarAnimation`](crate::Event::AvatarAnimation)). As with
    /// [`send_object_animation`](Self::send_object_animation) the list is the
    /// full state, not a delta: an animation that stops simply drops out of a
    /// later update.
    ///
    /// An animation whose [`source_id`](PlayingAnimation::source_id) names the
    /// object that triggered it is carried in the positionally-correlated
    /// `AnimationSourceList`, which the receiver reads by index. The list is
    /// written whole as soon as any animation has a source, and an animation
    /// with none is stamped with the **avatar's own id** — what OpenSim's
    /// `SendAnimations` does (`if (objectIDs[i].IsZero()) … = sourceAgentId`),
    /// so a receiver never sees a nil source. Sent reliably.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_avatar_animation(
        &mut self,
        avatar_id: AgentKey,
        animations: &[PlayingAnimation],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let animation_source_list = if animations
            .iter()
            .any(|animation| animation.source_id.is_some())
        {
            animations
                .iter()
                .map(|animation| AvatarAnimationAnimationSourceListBlock {
                    object_id: animation
                        .source_id
                        .as_ref()
                        .map_or_else(|| avatar_id.uuid(), sl_types::key::ObjectKey::uuid),
                })
                .collect()
        } else {
            Vec::new()
        };
        let message = AnyMessage::AvatarAnimation(AvatarAnimationWire {
            sender: AvatarAnimationSenderBlock {
                id: avatar_id.uuid(),
            },
            animation_list: animations
                .iter()
                .map(|animation| AvatarAnimationAnimationListBlock {
                    anim_id: animation.anim_id,
                    anim_sequence_id: animation.sequence_id,
                })
                .collect(),
            animation_source_list,
            physical_avatar_event_list: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)?;
        Ok(())
    }

    /// Sends an `ImprovedTerseObjectUpdate` carrying the new motion of objects
    /// the client already knows — the message a simulator sends every frame for
    /// everything that moves (an avatar walking, a physical prim falling, an
    /// attachment following its wearer). The client applies it to the object it
    /// has and surfaces an
    /// [`Event::ObjectUpdated`](crate::Event::ObjectUpdated); an id it does not
    /// know it refetches in full instead.
    ///
    /// Only motion travels here — a terse update carries no identity, shape or
    /// extra params — and the position is full precision while the velocity,
    /// acceleration, rotation and angular velocity are 16-bit quantized. An
    /// avatar's update additionally carries its collision plane
    /// ([`ObjectMotion::collision_plane`](crate::ObjectMotion::collision_plane)).
    /// Sent unreliably, as a simulator sends it: the next frame's update
    /// supersedes a lost one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error if
    /// the message fails to encode.
    pub fn send_terse_update(
        &mut self,
        updates: &[TerseUpdate],
        time_dilation: u16,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ImprovedTerseObjectUpdate(ImprovedTerseObjectUpdate {
            region_data: ImprovedTerseObjectUpdateRegionDataBlock {
                region_handle: self.region_handle.0,
                time_dilation,
            },
            object_data: updates
                .iter()
                .map(|update| ImprovedTerseObjectUpdateObjectDataBlock {
                    data: crate::object_update::encode_terse_object_data(update),
                    texture_entry: Vec::new(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Unreliable, now)?;
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

    /// Sends an `UpdateCreateInventoryItem` — hands the client the inventory
    /// items the simulator just minted or re-wrote (the inverse of the client's
    /// [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated)).
    ///
    /// This is how every server-side inventory *creation* is announced: the
    /// reply to a `CreateInventoryItem`, the item a take
    /// ([`ServerEvent::DerezObjects`]) files away, an accepted inventory offer,
    /// a completed asset upload. `transaction` echoes whatever the client
    /// correlated its request with (nil where it sent none), and
    /// `sim_approved` is the simulator's verdict on the item's permissions —
    /// `false` marks an item the client should treat as unconfirmed.
    ///
    /// The block is repeatable and the client surfaces one event per entry, so
    /// a batch of items goes in one message.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_inventory_item_created(
        &mut self,
        items: &[InventoryItem],
        transaction: TransactionId,
        sim_approved: bool,
        now: Instant,
    ) -> Result<(), Error> {
        self.send_inventory_items_created(
            &items
                .iter()
                .map(|item| (item.clone(), InventoryCallbackId::new(0)))
                .collect::<Vec<_>>(),
            transaction,
            sim_approved,
            now,
        )
    }

    /// [`send_inventory_item_created`](Self::send_inventory_item_created) with
    /// each item's **callback id** — the number the client put on the request
    /// that produced it, and the only thing correlating a reply with a pending
    /// inventory operation when several are in flight.
    ///
    /// The two are one message; they are two methods because most server-side
    /// creations (a take, an accepted offer) answer no client request and have
    /// no callback to echo, and passing `0` at every such site would read as a
    /// value rather than as its absence.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_inventory_items_created(
        &mut self,
        items: &[(InventoryItem, InventoryCallbackId)],
        transaction: TransactionId,
        sim_approved: bool,
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let mut inventory_data = Vec::with_capacity(items.len());
        for (item, callback) in items {
            let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
            inventory_data.push(UpdateCreateInventoryItemInventoryDataBlock {
                item_id: item.item_id.uuid(),
                folder_id: item.folder_id.uuid(),
                callback_id: callback.get(),
                creator_id: item.creator_id.uuid(),
                owner_id,
                group_id,
                base_mask: item.permissions.base.bits(),
                owner_mask: item.permissions.owner.bits(),
                group_mask: item.permissions.group.bits(),
                everyone_mask: item.permissions.everyone.bits(),
                next_owner_mask: item.permissions.next_owner.bits(),
                group_owned: item.owner.is_group(),
                asset_id: item.asset_id,
                r#type: item.item_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type,
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    item.sale_price.as_ref(),
                )?,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                // The permissions checksum a viewer recomputes to notice a
                // tampered item. The client's decode discards it, so a
                // simulator that has no reason to lie writes zero.
                crc: 0,
            });
        }
        let message = AnyMessage::UpdateCreateInventoryItem(UpdateCreateInventoryItem {
            agent_data: UpdateCreateInventoryItemAgentDataBlock {
                agent_id: self.agent_id.map_or_else(Uuid::nil, |a| a.uuid()),
                sim_approved,
                transaction_id: transaction.get(),
            },
            inventory_data,
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
                next_packet: 0,
                expires: deadline(now, XFER_STALL_TIMEOUT),
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
        send.expires = deadline(now, XFER_STALL_TIMEOUT);
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
        let Some(SimTransferServe { params, .. }) = self.transfer_serves.remove(&transfer_id)
        else {
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
        let Some(SimTransferServe { params, .. }) = self.transfer_serves.remove(&transfer_id)
        else {
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
        // The offer is not open-ended: a client that never answers with
        // `AgentSit` would otherwise leave the handshake half-done for the life
        // of the session. The client's own sit machine gives up after the same
        // interval.
        self.sit_expires = Some(deadline(now, SIT_HANDSHAKE_TIMEOUT));
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

    /// Retires this circuit after the agent moved on: sends `DisableSimulator`
    /// so the client tears the (now child) circuit down, and closes the
    /// session with [`ServerEvent::CircuitRetired`] — the driver's pumps exit
    /// on [`is_closed`](Self::is_closed) instead of waiting for the inactivity
    /// timeout. What a source simulator does once the teleport destination
    /// confirmed the arrival.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode; the session stays open in that case.
    pub fn retire_circuit(&mut self, now: Instant) -> Result<(), Error> {
        self.send_disable_simulator(now)?;
        self.close(ServerEvent::CircuitRetired);
        Ok(())
    }

    /// Abandons the session: closes it with [`ServerEvent::Disconnected`]
    /// without sending anything — for a teleport destination the client never
    /// reached (the arrival timed out), where there is no circuit to retire.
    /// A no-op once closed.
    pub fn abandon(&mut self) {
        self.close(ServerEvent::Disconnected);
    }

    /// Records that the agent is already **seated** on `seat`, with no sit
    /// handshake — what a destination simulator is told about an agent that
    /// arrives riding something.
    ///
    /// A sit is normally a conversation (`AgentRequestSit` →
    /// `AvatarSitResponse` → `AgentSit`), but a ridden region crossing has
    /// none: the agent was already sitting in the region it came from, and the
    /// destination inherits that as part of the agent data rather than asking
    /// for it again. Without this the destination would believe an arriving
    /// rider is standing, and the first thing it sent about them would say so.
    ///
    /// Sending nothing is deliberate — the client is not being asked to sit,
    /// it is being agreed with. The visible half is the avatar's own object
    /// update, whose `ParentID` names the seat.
    pub const fn seat_agent(&mut self, seat: ObjectKey) {
        self.sit = SimSitState::Seated { on: seat };
        self.sit_expires = None;
    }

    /// Places the agent's arrival: the position and facing the
    /// `AgentMovementComplete` reply carries when the client completes its
    /// movement into this region. A teleport destination sets this to the
    /// requested landing spot before the client arrives; unset, the agent
    /// lands at the region centre facing +X.
    pub const fn set_arrival_position(&mut self, position: RegionCoordinates, look_at: Vector) {
        self.arrival = ArrivalPlacement { position, look_at };
    }

    /// Where the agent lands when its movement completes.
    #[must_use]
    pub const fn arrival_position(&self) -> &ArrivalPlacement {
        &self.arrival
    }

    /// Enqueues a CAPS `EnableSimulator` event — announces a neighbouring (or
    /// teleport-destination) region so the client opens a **child** circuit to
    /// it (the modern event-queue path; the client answers with a
    /// `UseCircuitCode` on `sim`).
    pub fn enqueue_enable_simulator(&mut self, handle: RegionHandle, sim: SocketAddr) {
        self.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(
                handle.0,
                sim,
                (STANDARD_REGION_SIZE_METRES, STANDARD_REGION_SIZE_METRES),
            ),
        );
    }

    /// Enqueues a CAPS `EstablishAgentCommunication` event — hands the client
    /// the child region's seed capability (this event has **no** UDP form).
    /// The client caches the seed and surfaces it so its driver POSTs it,
    /// which is what makes a region start streaming to the child agent. The
    /// `agent-id` is this circuit's agent (nil before `UseCircuitCode`).
    pub fn enqueue_establish_agent_communication(&mut self, sim: SocketAddr, seed: &str) {
        let agent_id = self.agent_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
        self.enqueue_caps_event(
            "EstablishAgentCommunication",
            establish_agent_communication_to_llsd(agent_id, sim, seed),
        );
    }

    /// Enqueues a CAPS `TeleportFinish` event — completes an **inter-region**
    /// teleport by handing the client the destination simulator's address,
    /// region handle, seed capability, maturity rating and teleport flags
    /// (the full reference record, see [`TeleportFinishInfo`]). The client
    /// sends `CompleteAgentMovement` on its (child) circuit to `info.dest`;
    /// the destination's `AgentMovementComplete` commits the handover.
    pub fn enqueue_teleport_finish(&mut self, info: &TeleportFinishInfo) {
        self.enqueue_caps_event("TeleportFinish", teleport_finish_to_llsd(info));
    }

    /// Enqueues a CAPS `CrossedRegion` event — the avatar walked over a region
    /// border; the client promotes its pre-opened child circuit to
    /// `info.dest` to root (no teleport screen). See [`CrossedRegionInfo`] for
    /// why the announcement names the agent and its landing spot as well as
    /// the destination simulator.
    pub fn enqueue_crossed_region(&mut self, info: &CrossedRegionInfo) {
        self.enqueue_caps_event("CrossedRegion", crossed_region_to_caps_llsd(info));
    }

    /// Demotes this circuit's root agent back to a **child** agent: the avatar
    /// left for another region and this one only streams its scene from now
    /// on. OpenSim's `ScenePresence.MakeChildAgent`, which is what the source
    /// simulator does once a border crossing has been handed over — unlike a
    /// teleport, whose source circuit is retired outright
    /// ([`retire_circuit`](Self::retire_circuit)).
    ///
    /// Nothing is sent: the client already knows, because it asked the
    /// destination to promote its own circuit. In particular the departing
    /// avatar's object is **not** killed on this circuit — the reference
    /// simulator kills it only for *other* viewers that cannot see the region
    /// the avatar walked into, never for the crossing agent's own client,
    /// whose avatar is one object across every circuit it holds.
    pub const fn make_child_agent(&mut self) {
        self.agent_presence = AgentPresence::Child;
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

    /// Queues a `ChatterBoxSessionStartReply` on the event queue — the answer
    /// to a session start the client requested (an ad-hoc conference over UDP
    /// or the `"start conference"` capability, or a group session), telling it
    /// the id the session **actually** has next to the temporary one it minted
    /// (the inverse of the client's [`Event::ChatSessionStarted`], and the
    /// shape
    /// [`chatterbox_session_start_reply_to_llsd`](crate::chatterbox_session_start_reply_to_llsd)
    /// serializes). Answering with `session_id == temp_session_id` keeps the
    /// client's id, which is what a simulator that mints none does.
    ///
    /// Any other [`Event`] variant enqueues nothing, the same contract as
    /// [`SimSession::enqueue_chatterbox_invitation`].
    pub fn enqueue_chatterbox_session_start_reply(&mut self, reply: &Event) {
        if !matches!(reply, Event::ChatSessionStarted { .. }) {
            return;
        }
        self.enqueue_caps_event(
            "ChatterBoxSessionStartReply",
            chatterbox_session_start_reply_to_llsd(reply),
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
    /// The ping carries this end's oldest unacknowledged outgoing sequence
    /// number in `OldestUnacked`, letting the client retire its own
    /// duplicate-suppression record of anything older. "Oldest" is read off the
    /// wrapping counter rather than the numeric order of the set: once the
    /// counter has wrapped, the numerically smallest outstanding sequence
    /// number is the newest packet, not the oldest.
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
        let oldest = crate::unacked::oldest(&self.unacked, self.next_sequence);
        let message = AnyMessage::StartPingCheck(StartPingCheck {
            ping_id: StartPingCheckPingIDBlock {
                ping_id: ping_id.get(),
                oldest_unacked: oldest.get(),
            },
        });
        self.send(&message, Reliability::Unreliable, now)?;
        self.outstanding_ping = Some((ping_id, now));
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
    ///
    /// The queue is bounded at 4096 events. A client that has stopped
    /// long-polling — one that crashed, or never started — would otherwise let
    /// the queue grow for the life of the session, so past the bound the oldest
    /// events are dropped to make room. Dropping the oldest keeps the queue
    /// carrying what a client that comes back would most want, and every drop
    /// is logged rather than passing silently.
    pub fn enqueue_caps_event(&mut self, message: impl Into<String>, body: Llsd) {
        let message = message.into();
        if self.caps_events.len() >= MAX_CAPS_EVENTS {
            let overflow = self
                .caps_events
                .len()
                .saturating_sub(MAX_CAPS_EVENTS)
                .saturating_add(1);
            let dropped: Vec<String> = self
                .caps_events
                .drain(..overflow.min(self.caps_events.len()))
                .map(|event| event.message)
                .collect();
            tracing::warn!(
                dropped = dropped.len(),
                messages = ?dropped,
                queued = MAX_CAPS_EVENTS,
                "the CAPS event queue is full; dropped the oldest events to enqueue {message}"
            );
        }
        self.caps_events.push(EventQueueEvent { message, body });
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
    ///
    /// A message that fails to encode does not take the acks batched behind it
    /// with it — see [`send_ack_packets`] for why every message is sent even
    /// after one fails, and why the first failure is the one returned.
    fn flush_acks(&mut self, now: Instant) -> Result<(), WireError> {
        self.ack_flush = None;
        if self.pending_acks.is_empty() {
            return Ok(());
        }
        let acks = std::mem::take(&mut self.pending_acks);
        send_ack_packets(&acks, |message| {
            self.send(message, Reliability::Unreliable, now)
        })
    }

    /// The retransmission timeout for this circuit: the reference's
    /// `LL_RELIABLE_TIMEOUT_FACTOR` multiple of the averaged round-trip time to
    /// the client, floored at [`MINIMUM_RESEND_TIMEOUT`].
    fn resend_timeout(&self) -> Duration {
        self.ping_average
            .mul_f32(RELIABLE_TIMEOUT_FACTOR)
            .max(MINIMUM_RESEND_TIMEOUT)
    }

    /// Folds a round-trip `sample` into the ping average with the reference's
    /// fast-attack / slow-decay relaxation (`LLCircuitData::setPingDelay`): the
    /// average first jumps to any worse sample, then relaxes toward it, and the
    /// result is clamped to `PING_AVERAGE_MIN ..= PING_AVERAGE_MAX`.
    fn record_ping_sample(&mut self, sample: Duration) {
        let attacked = self.ping_average.max(sample);
        self.ping_average = attacked
            .mul_f32(PING_AVERAGE_DECAY)
            .saturating_add(sample.mul_f32(PING_AVERAGE_ALPHA))
            .clamp(PING_AVERAGE_MIN, PING_AVERAGE_MAX);
    }

    /// Retransmits unacknowledged reliable packets whose timeout has elapsed.
    ///
    /// The timeout tracks the measured round trip ([`Self::resend_timeout`]),
    /// and a datagram still waiting in the outbound queue has its clock held at
    /// `now` rather than counting the wait as silence from the client — so a
    /// driver that falls behind does not turn its own backlog into a burst of
    /// retransmissions.
    ///
    /// Returns every packet that has now exhausted its retransmission budget;
    /// such packets are dropped from the unacked set, so they are reported once
    /// and stop driving the resend deadline.
    fn process_resends(&mut self, now: Instant) -> Vec<ExhaustedPacket> {
        let timeout = self.resend_timeout();
        let mut exhausted = Vec::new();
        let mut to_send = Vec::new();
        for (sequence, packet) in &mut self.unacked {
            if packet.queued {
                packet.sent_at = now;
                continue;
            }
            if now < deadline(packet.sent_at, timeout) {
                continue;
            }
            if packet.attempts >= MAX_RESEND_ATTEMPTS {
                exhausted.push(ExhaustedPacket {
                    sequence: *sequence,
                    name: packet.name,
                    severity: packet.severity,
                });
                continue;
            }
            let mut datagram = packet.datagram.clone();
            if let Some(first) = datagram.first_mut() {
                *first |= PacketFlags::RESENT.bits();
            }
            packet.sent_at = now;
            packet.queued = true;
            packet.attempts = packet.attempts.saturating_add(1);
            to_send.push(SimOutbound {
                sequence: Some(*sequence),
                payload: datagram,
            });
        }
        self.out.extend(to_send);
        for packet in &exhausted {
            self.unacked.remove(&packet.sequence);
        }
        exhausted
    }

    /// The earliest retransmission deadline across all unacked packets. A packet
    /// whose datagram is still queued has not started its clock, so it does not
    /// contribute a deadline — its wake-up comes from the transmission itself.
    fn next_resend_deadline(&self) -> Option<Instant> {
        let timeout = self.resend_timeout();
        self.unacked
            .values()
            .filter(|packet| !packet.queued)
            .map(|packet| deadline(packet.sent_at, timeout))
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
        // Traffic from anywhere but the bound client is not this circuit's.
        match self.client_addr {
            Some(addr) if addr != from => return Ok(()),
            _ => {}
        }

        let parsed = parse_datagram(datagram)?;

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
        let message = AnyMessage::decode(id, &mut reader).ok();

        // Nothing claims this circuit's endpoint but the packet that opens it.
        // Binding to the first parseable sender instead would let any datagram
        // — including one from an unrelated host that happened to arrive first
        // — take the address the circuit then answers on.
        if self.client_addr.is_none() {
            if !matches!(message, Some(AnyMessage::UseCircuitCode(_))) {
                self.events.push_back(ServerEvent::Rejected {
                    message: sl_wire::message_name(id).map(str::to_owned),
                    reason: RejectionReason::NoCircuit,
                });
                return Ok(());
            }
            self.client_addr = Some(from);
        }

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

        let Some(message) = message else {
            return Ok(());
        };
        if let Some(reason) = self.refusal_for(&message) {
            self.reject(&message, reason);
            return Ok(());
        }
        self.dispatch(&message, now)
    }

    /// Records that `message` was refused, and why.
    fn reject(&mut self, message: &AnyMessage, reason: RejectionReason) {
        self.events.push_back(ServerEvent::Rejected {
            message: sl_wire::message_name(message.id()).map(str::to_owned),
            reason,
        });
    }

    /// Why this simulator refuses to act on `message`, if it does.
    ///
    /// A message that asserts an `AgentData.SessionID` must assert *this*
    /// circuit's session: the identity the circuit was opened with is what the
    /// traffic on it is attributed to. That is a check a real simulator makes
    /// and one the dispatcher's own arms could not make consistently across
    /// 130-odd handlers, so it is made once, here, from the session id the
    /// message template says the message carries.
    ///
    /// `UseCircuitCode` itself carries its session id in a `CircuitCode` block
    /// rather than an `AgentData` one, so it is naturally exempt: it establishes
    /// the identity the others are checked against. Nothing else reaches this
    /// point before the circuit is open — [`SimSession::handle_datagram`] binds
    /// the endpoint only on the packet that opens it — so the unbound case is a
    /// backstop rather than the live path.
    fn refusal_for(&self, message: &AnyMessage) -> Option<RejectionReason> {
        let asserted = message.agent_session_id()?;
        match self.session_id {
            None => Some(RejectionReason::NoCircuit),
            Some(session_id) if session_id != asserted => Some(RejectionReason::SessionIdMismatch),
            Some(_matching) => None,
        }
    }

    /// Dispatches a decoded client message: answers the circuit-lifecycle
    /// messages and surfaces a [`ServerEvent`] for each.
    fn dispatch(&mut self, message: &AnyMessage, now: Instant) -> Result<(), Error> {
        match message {
            AnyMessage::UseCircuitCode(use_circuit) => {
                let block = &use_circuit.circuit_code;
                let agent_id = AgentKey::from(block.id);
                let circuit_code = CircuitCode(block.code);
                // A circuit's identity is fixed the moment it opens. A repeat
                // carrying the same triple is the client re-sending a packet it
                // believes was lost, and is answered again; a repeat carrying a
                // *different* one is another agent's login (or a forgery) trying
                // to take the circuit over, and is refused.
                let bound = (self.agent_id, self.session_id, self.circuit_code);
                let asserted = (Some(agent_id), Some(block.session_id), Some(circuit_code));
                if bound == (None, None, None) {
                    self.agent_id = Some(agent_id);
                    self.session_id = Some(block.session_id);
                    self.circuit_code = Some(circuit_code);
                } else if bound != asserted {
                    self.reject(message, RejectionReason::CircuitRebind);
                    return Ok(());
                }
                if matches!(self.state, SimState::AwaitingCircuit) {
                    self.state = SimState::Active;
                    self.ping = Some(deadline(now, PING_INTERVAL));
                }
                self.events.push_back(ServerEvent::CircuitOpened {
                    agent_id,
                    session_id: block.session_id,
                    circuit_code,
                });
            }
            AnyMessage::CompleteAgentMovement(_) => {
                // The child agent becomes the root agent: login arrival, or a
                // teleport/crossing destination confirming the handover. It only
                // means anything on a circuit that is already up — before
                // `UseCircuitCode` there is no agent to promote, and honouring
                // it there would leave a rooted agent on a circuit whose
                // keep-alive was never armed.
                if !matches!(self.state, SimState::Active) {
                    self.reject(message, RejectionReason::NoCircuit);
                    return Ok(());
                }
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
            // The client answering our periodic `StartPingCheck`. Consumed —
            // and its round trip is the measurement the retransmission timeout
            // is built on, so an answer to the ping we are waiting on is folded
            // into the average. A reply to any other ping id is stale.
            AnyMessage::CompletePingCheck(complete) => {
                let answered = PingId(complete.ping_id.ping_id);
                if let Some((outstanding, sent_at)) = self.outstanding_ping
                    && outstanding == answered
                {
                    self.outstanding_ping = None;
                    self.record_ping_sample(now.saturating_duration_since(sent_at));
                }
            }
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
                    self.sit_expires = None;
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
                        self.sit_expires = None;
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
                // The registry only ever grows — a holder is never forgotten —
                // so it is bounded here. Past the bound a new holder is not
                // recorded: the lookup then reports it as never-answered, which
                // is the safe answer (no permission), and the answer itself is
                // still surfaced for the driver.
                if self.script_grants.len() >= MAX_SCRIPT_GRANTS
                    && !self.script_grants.contains_key(&(task_id, item_id))
                {
                    self.reject(message, RejectionReason::LimitExceeded);
                } else {
                    self.script_grants.insert((task_id, item_id), permissions);
                }
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
            AnyMessage::ParcelDwellRequest(request) => {
                // `Data.ParcelID` is the template's "filled in on sim" field:
                // the viewer sends it nil and the simulator resolves the
                // region-local id itself, so only the local id is surfaced.
                self.events.push_back(ServerEvent::RequestParcelDwell {
                    local_id: RegionLocalParcelId(request.data.local_id),
                });
            }
            AnyMessage::EconomyDataRequest(_request) => {
                self.events.push_back(ServerEvent::RequestEconomyData);
            }
            AnyMessage::AgentWearablesRequest(_request) => {
                self.events.push_back(ServerEvent::RequestAgentWearables);
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
            AnyMessage::ParcelPropertiesUpdate(update) => {
                let data = &update.parcel_data;
                self.events.push_back(ServerEvent::ParcelPropertiesUpdated {
                    update: Box::new(ParcelUpdate {
                        local_id: RegionLocalParcelId(data.local_id),
                        parcel_flags: sl_wire::ParcelFlags::from_bits(data.parcel_flags),
                        sale_price: crate::types::linden_price_from_wire(
                            sl_wire::ParcelFlags::from_bits(data.parcel_flags)
                                .contains(sl_wire::ParcelFlags::FOR_SALE),
                            "SalePrice",
                            data.sale_price,
                        )?,
                        name: trimmed_string(&data.name),
                        description: trimmed_string(&data.desc),
                        music_url: sl_wire::optional_url_from_wire(
                            "MusicURL",
                            &trimmed_string(&data.music_url),
                        )?,
                        media_url: sl_wire::optional_url_from_wire(
                            "MediaURL",
                            &trimmed_string(&data.media_url),
                        )?,
                        media_id: crate::types::optional_key_from_wire(data.media_id),
                        media_auto_scale: data.media_auto_scale != 0,
                        group_id: crate::types::group_from_wire(data.group_id),
                        pass_price: crate::types::linden_from_wire("PassPrice", data.pass_price)?,
                        pass_hours: data.pass_hours,
                        category: ParcelCategory::from_u8(data.category),
                        auth_buyer_id: crate::types::optional_key_from_wire(data.auth_buyer_id),
                        snapshot_id: crate::types::optional_key_from_wire(data.snapshot_id),
                        user_location: RegionCoordinates::new(
                            data.user_location.x,
                            data.user_location.y,
                            data.user_location.z,
                        ),
                        user_look_at: sl_types::map::Direction::new(
                            data.user_look_at.x,
                            data.user_look_at.y,
                            data.user_look_at.z,
                        ),
                        landing_type: data.landing_type,
                    }),
                });
            }
            AnyMessage::ParcelAccessListUpdate(update) => {
                self.events.push_back(ServerEvent::ParcelAccessListUpdated {
                    local_id: RegionLocalParcelId(update.data.local_id),
                    scope: ParcelAccessScope::from_u32(update.data.flags),
                    // The nil-agent placeholder a simulator and a viewer both
                    // use to say "this list is empty" is not a member, and
                    // storing it would put a nameless entry in the About Land
                    // panel.
                    entries: update
                        .list
                        .iter()
                        .filter(|entry| !entry.id.is_nil())
                        .map(|entry| ParcelAccessEntry {
                            id: entry.id,
                            time: entry.time,
                            flags: ParcelAccessFlags(entry.flags),
                        })
                        .collect(),
                    transaction_id: TransactionId::from(update.data.transaction_id),
                    sequence_id: update.data.sequence_id,
                    sections: update.data.sections,
                });
            }
            AnyMessage::ParcelAccessListRequest(request) => {
                self.events.push_back(ServerEvent::RequestParcelAccessList {
                    local_id: RegionLocalParcelId(request.data.local_id),
                    scope: ParcelAccessScope::from_u32(request.data.flags),
                    sequence_id: request.data.sequence_id,
                });
            }
            AnyMessage::ParcelBuy(buy) => {
                self.events.push_back(ServerEvent::ParcelBought {
                    local_id: RegionLocalParcelId(buy.data.local_id),
                    group_id: crate::types::group_from_wire(buy.data.group_id),
                    is_group_owned: buy.data.is_group_owned,
                    remove_contribution: buy.data.remove_contribution,
                    price: crate::types::linden_from_wire("Price", buy.parcel_data.price)?,
                    area: buy.parcel_data.area,
                });
            }
            AnyMessage::ParcelDeedToGroup(deed) => {
                self.events.push_back(ServerEvent::ParcelDeededToGroup {
                    local_id: RegionLocalParcelId(deed.data.local_id),
                    group_id: GroupKey::from(deed.data.group_id),
                });
            }
            AnyMessage::ParcelRelease(release) => {
                self.events.push_back(ServerEvent::ParcelReleased {
                    local_id: RegionLocalParcelId(release.data.local_id),
                });
            }
            AnyMessage::ParcelReclaim(reclaim) => {
                self.events.push_back(ServerEvent::ParcelReclaimed {
                    local_id: RegionLocalParcelId(reclaim.data.local_id),
                });
            }
            AnyMessage::ParcelReturnObjects(returned) => {
                self.events.push_back(ServerEvent::ParcelObjectsReturned {
                    local_id: RegionLocalParcelId(returned.parcel_data.local_id),
                    return_type: ParcelReturnType(returned.parcel_data.return_type),
                    task_ids: returned
                        .task_i_ds
                        .iter()
                        .map(|block| ObjectKey::from(block.task_id))
                        .collect(),
                    owner_ids: returned
                        .owner_i_ds
                        .iter()
                        .map(|block| OwnerKey::Agent(AgentKey::from(block.owner_id)))
                        .collect(),
                });
            }
            AnyMessage::ParcelSelectObjects(select) => {
                self.events.push_back(ServerEvent::ParcelObjectsSelected {
                    local_id: RegionLocalParcelId(select.parcel_data.local_id),
                    return_type: ParcelReturnType(select.parcel_data.return_type),
                    owner_ids: select
                        .return_i_ds
                        .iter()
                        .map(|block| OwnerKey::Agent(AgentKey::from(block.return_id)))
                        .collect(),
                });
            }
            AnyMessage::LandStatRequest(request) => {
                self.events.push_back(ServerEvent::RequestLandStat {
                    report_type: LandStatReportType::from_u32(request.request_data.report_type),
                    request_flags: request.request_data.request_flags,
                    filter: trimmed_string(&request.request_data.filter),
                    local_id: RegionLocalParcelId(request.request_data.parcel_local_id),
                });
            }
            AnyMessage::RequestRegionInfo(_) => {
                self.events.push_back(ServerEvent::RequestRegionInfo);
            }
            AnyMessage::ObjectName(rename) => {
                for block in &rename.object_data {
                    self.events.push_back(ServerEvent::ObjectNameSet {
                        local_id: RegionLocalObjectId(block.local_id),
                        name: trimmed_string(&block.name),
                    });
                }
            }
            AnyMessage::ObjectDescription(describe) => {
                for block in &describe.object_data {
                    self.events.push_back(ServerEvent::ObjectDescriptionSet {
                        local_id: RegionLocalObjectId(block.local_id),
                        description: trimmed_string(&block.description),
                    });
                }
            }
            AnyMessage::ObjectCategory(categorise) => {
                for block in &categorise.object_data {
                    self.events.push_back(ServerEvent::ObjectCategorySet {
                        local_id: RegionLocalObjectId(block.local_id),
                        category: block.category,
                    });
                }
            }
            AnyMessage::ObjectClickAction(click) => {
                for block in &click.object_data {
                    self.events.push_back(ServerEvent::ObjectClickActionSet {
                        local_id: RegionLocalObjectId(block.object_local_id),
                        click_action: ClickAction::from_code(block.click_action),
                    });
                }
            }
            AnyMessage::ObjectMaterial(material) => {
                for block in &material.object_data {
                    self.events.push_back(ServerEvent::ObjectMaterialSet {
                        local_id: RegionLocalObjectId(block.object_local_id),
                        material: Material::from_code(block.material),
                    });
                }
            }
            AnyMessage::ObjectSaleInfo(sale) => {
                for block in &sale.object_data {
                    self.events.push_back(ServerEvent::ObjectSaleInfoSet {
                        local_id: RegionLocalObjectId(block.local_id),
                        sale_type: SaleType::from_code(block.sale_type),
                        sale_price: crate::types::linden_price_from_wire(
                            block.sale_type != SaleType::NotForSale.to_code(),
                            "SalePrice",
                            block.sale_price,
                        )?,
                    });
                }
            }
            AnyMessage::ObjectFlagUpdate(update) => {
                let data = &update.agent_data;
                self.events.push_back(ServerEvent::ObjectFlagsSet {
                    local_id: RegionLocalObjectId(data.object_local_id),
                    flags: ObjectFlagSettings {
                        use_physics: data.use_physics,
                        is_temporary: data.is_temporary,
                        is_phantom: data.is_phantom,
                        casts_shadows: data.casts_shadows,
                    },
                });
            }
            AnyMessage::ObjectIncludeInSearch(search) => {
                for block in &search.object_data {
                    self.events
                        .push_back(ServerEvent::ObjectIncludeInSearchSet {
                            local_id: RegionLocalObjectId(block.object_local_id),
                            include_in_search: block.include_in_search,
                        });
                }
            }
            AnyMessage::ObjectPermissions(permissions) => {
                let god_override = permissions.header_data.r#override;
                for block in &permissions.object_data {
                    let Some(field) = PermissionField::from_code(block.field) else {
                        tracing::debug!(
                            "an ObjectPermissions block named field {:#04x}, which is no mask",
                            block.field
                        );
                        continue;
                    };
                    self.events.push_back(ServerEvent::ObjectPermissionsSet {
                        local_id: RegionLocalObjectId(block.object_local_id),
                        field,
                        set: block.set != 0,
                        mask: Permissions::from_bits(block.mask),
                        god_override,
                    });
                }
            }
            AnyMessage::ObjectGroup(group) => {
                self.events.push_back(ServerEvent::ObjectGroupSet {
                    local_ids: group
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                    group_id: crate::types::group_from_wire(group.agent_data.group_id),
                });
            }
            AnyMessage::ObjectOwner(owner) => {
                let header = &owner.header_data;
                self.events.push_back(ServerEvent::ObjectOwnerSet {
                    local_ids: owner
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                    owner: crate::types::object_owner_from_wire(header.owner_id, header.group_id),
                    god_override: header.r#override,
                });
            }
            AnyMessage::ObjectLink(link) => {
                self.events.push_back(ServerEvent::ObjectsLinked {
                    local_ids: link
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                });
            }
            AnyMessage::ObjectDelink(delink) => {
                self.events.push_back(ServerEvent::ObjectsDelinked {
                    local_ids: delink
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                });
            }
            AnyMessage::ObjectDuplicate(duplicate) => {
                self.events.push_back(ServerEvent::ObjectsDuplicated {
                    local_ids: duplicate
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                    offset: duplicate.shared_data.offset.clone(),
                    group_id: crate::types::group_from_wire(duplicate.agent_data.group_id),
                    duplicate_flags: duplicate.shared_data.duplicate_flags,
                });
            }
            AnyMessage::ObjectDelete(delete) => {
                self.events.push_back(ServerEvent::ObjectsDeleted {
                    local_ids: delete
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                    force: delete.agent_data.force,
                });
            }
            AnyMessage::MultipleObjectUpdate(update) => {
                for block in &update.object_data {
                    let Some(transform) =
                        crate::session::object_transform_from_wire(block.r#type, &block.data)
                    else {
                        tracing::debug!(
                            "a MultipleObjectUpdate block of type {:#04x} carried {} bytes, too \
                             few for the components it names",
                            block.r#type,
                            block.data.len()
                        );
                        continue;
                    };
                    self.events.push_back(ServerEvent::ObjectTransformSet {
                        local_id: RegionLocalObjectId(block.object_local_id),
                        transform,
                    });
                }
            }
            AnyMessage::Undo(undo) => {
                self.events.push_back(ServerEvent::ObjectsUndone {
                    object_ids: undo
                        .object_data
                        .iter()
                        .map(|block| ObjectKey::from(block.object_id))
                        .collect(),
                });
            }
            AnyMessage::Redo(redo) => {
                self.events.push_back(ServerEvent::ObjectsRedone {
                    object_ids: redo
                        .object_data
                        .iter()
                        .map(|block| ObjectKey::from(block.object_id))
                        .collect(),
                });
            }
            AnyMessage::ObjectSelect(select) => {
                self.events.push_back(ServerEvent::ObjectsSelected {
                    local_ids: select
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                });
            }
            AnyMessage::ObjectDeselect(deselect) => {
                self.events.push_back(ServerEvent::ObjectsDeselected {
                    local_ids: deselect
                        .object_data
                        .iter()
                        .map(|block| RegionLocalObjectId(block.object_local_id))
                        .collect(),
                });
            }
            AnyMessage::ObjectAdd(add) => {
                let data = &add.object_data;
                self.events.push_back(ServerEvent::RezObject {
                    params: AddPrimParams {
                        group_id: crate::types::optional_key_from_wire(add.agent_data.group_id),
                        shape: PrimShape {
                            pcode: data.p_code,
                            material: Material::from_code(data.material),
                            add_flags: data.add_flags,
                            path_curve: data.path_curve,
                            profile_curve: data.profile_curve,
                            path_begin: data.path_begin,
                            path_end: data.path_end,
                            path_scale_x: data.path_scale_x,
                            path_scale_y: data.path_scale_y,
                            path_shear_x: data.path_shear_x,
                            path_shear_y: data.path_shear_y,
                            path_twist: data.path_twist,
                            path_twist_begin: data.path_twist_begin,
                            path_radius_offset: data.path_radius_offset,
                            path_taper_x: data.path_taper_x,
                            path_taper_y: data.path_taper_y,
                            path_revolutions: data.path_revolutions,
                            path_skew: data.path_skew,
                            profile_begin: data.profile_begin,
                            profile_end: data.profile_end,
                            profile_hollow: data.profile_hollow,
                            scale: data.scale.clone(),
                            rotation: data.rotation.clone(),
                            // The block carries no position of its own: where
                            // the prim lands is the ray's end point.
                            position: data.ray_end.clone(),
                            state: data.state,
                        },
                        bypass_raycast: data.bypass_raycast != 0,
                        ray_start: data.ray_start.clone(),
                        ray_end: data.ray_end.clone(),
                        ray_target_id: crate::types::optional_key_from_wire(data.ray_target_id),
                        ray_end_is_intersection: data.ray_end_is_intersection != 0,
                    },
                });
            }
            AnyMessage::DeRezObject(derez) => {
                let block = &derez.agent_block;
                // A destination byte no `DRD_*` value uses names nothing the
                // simulator could act on, so the message is dropped whole —
                // the same silence OpenSim's `DeRezObjects` gives an action
                // its switch has no case for.
                let Some(destination) =
                    DeRezDestination::from_code(block.destination, block.destination_id)
                else {
                    return Ok(());
                };
                self.events.push_back(ServerEvent::DerezObjects {
                    local_ids: derez
                        .object_data
                        .iter()
                        .map(|object| RegionLocalObjectId(object.object_local_id))
                        .collect(),
                    destination,
                    transaction_id: TransactionId::from(block.transaction_id),
                    group_id: crate::types::optional_key_from_wire(block.group_id),
                    packet: (block.packet_count, block.packet_number),
                });
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
            AnyMessage::UpdateInventoryItem(update) => {
                // The block carries no asset id: an item's bytes are named by
                // the transaction the client also sent them under. Deriving the
                // id here rather than in every driver keeps the derivation in
                // one place, beside the `AssetUploadRequest` arm that uses it.
                let secure = self.secure_session_id;
                let mut items = Vec::new();
                for block in &update.inventory_data {
                    items.push(UpdatedInventoryItem {
                        item: restore_item_from_inventory_block!(block),
                        callback_id: InventoryCallbackId::new(block.callback_id),
                        bound_asset: (!block.transaction_id.is_nil())
                            .then(|| {
                                secure.map(|secure| combine_uuids(block.transaction_id, secure))
                            })
                            .flatten()
                            .map(AssetKey::from),
                    });
                }
                self.events
                    .push_back(ServerEvent::UpdateAgentInventoryItems {
                        items,
                        transaction_id: TransactionId::new(update.agent_data.transaction_id),
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
                            expires: deadline(now, XFER_STALL_TIMEOUT),
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
                                next_packet: 0,
                                expires: deadline(now, XFER_STALL_TIMEOUT),
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
                    // `Xfer` is a strictly ordered, one-packet-in-flight stream:
                    // a packet that is not the one expected is a duplicate or a
                    // gap, and concatenating its bytes would silently corrupt
                    // the file. Refuse it, and refuse a stream that would grow
                    // the buffer past what any real upload needs — the two
                    // bounds that keep a network-driven buffer from growing
                    // without limit.
                    let refusal = self.xfer_receives.get(&xfer_id).and_then(|receive| {
                        if packet_id.sequence() != receive.next_packet {
                            Some(RejectionReason::OutOfOrder)
                        } else if receive.buffer.len().saturating_add(chunk.payload.len())
                            > MAX_XFER_RECEIVE_BYTES
                        {
                            Some(RejectionReason::LimitExceeded)
                        } else {
                            None
                        }
                    });
                    if let Some(reason) = refusal {
                        self.reject(message, reason);
                        // An oversized stream is over: drop it and tell the
                        // client. An out-of-order packet is not necessarily
                        // fatal — the confirmation it is missing may simply have
                        // been lost — so leave the pull in place and send the
                        // client nothing, letting its own retry find the stream
                        // again.
                        if matches!(reason, RejectionReason::LimitExceeded) {
                            let _receive = self.xfer_receives.remove(&xfer_id);
                            let abort = AnyMessage::AbortXfer(AbortXfer {
                                xfer_id: AbortXferXferIDBlock {
                                    id: xfer_id.get(),
                                    result: XFER_TIMEOUT_RESULT,
                                },
                            });
                            self.send(&abort, Reliability::Reliable, now)?;
                            self.events.push_back(ServerEvent::XferAborted {
                                xfer_id,
                                result: XFER_TIMEOUT_RESULT,
                            });
                        }
                        return Ok(());
                    }
                    if let Some(receive) = self.xfer_receives.get_mut(&xfer_id) {
                        receive.buffer.extend_from_slice(chunk.payload);
                        receive.next_packet = receive.next_packet.saturating_add(1);
                        receive.expires = deadline(now, XFER_STALL_TIMEOUT);
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
                    let _prev = self.transfer_serves.insert(
                        transfer_id,
                        SimTransferServe {
                            params: block.params.clone(),
                            expires: deadline(now, TRANSFER_SERVE_TIMEOUT),
                        },
                    );
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
                position: Vector {
                    x: self.arrival.position.x(),
                    y: self.arrival.position.y(),
                    z: self.arrival.position.z(),
                },
                look_at: self.arrival.look_at.clone(),
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
            && let Err(error) = self.flush_acks(now)
        {
            // A flush failure is a wire-encoding bug, not a runtime condition,
            // and `flush_acks` has already sent every `PacketAck` it could — so
            // there is nothing to do here but say so. `handle_timeout` has no
            // way to report it and must not fail the session over it.
            tracing::warn!(%error, "failed to flush owed acks to the client");
        }
        let exhausted = self.process_resends(now);
        if exhausted
            .iter()
            .any(|packet| matches!(packet.severity, SimReliableSeverity::SessionCritical))
        {
            // Without the handshake or the movement completion the client never
            // finishes arriving; there is nothing left to keep the circuit open
            // for. Report the give-ups first so a driver sees which packet it
            // was before the close.
            self.report_give_ups(&exhausted);
            self.close(ServerEvent::Disconnected);
            return;
        }
        self.report_give_ups(&exhausted);
        self.expire_sit_offer(now);
        self.expire_transfers(now);
        if let Err(error) = self.expire_xfers(now) {
            // Encoding an `AbortXfer` cannot fail on well-formed state, and the
            // transfer is already dropped either way, so say so and carry on
            // rather than failing the session over it.
            tracing::warn!(%error, "failed to tell the client about an expired Xfer");
        }
        if let Some(at) = self.ping
            && now >= at
        {
            // A ping still in flight when the next one is due is itself a
            // round-trip measurement in progress: fold its time so far in, so a
            // client that stops answering widens the retransmission timeout
            // instead of drawing more retransmissions onto a struggling link.
            if let Some((_id, sent_at)) = self.outstanding_ping {
                self.record_ping_sample(now.saturating_duration_since(sent_at));
            }
            self.ping = Some(deadline(now, PING_INTERVAL));
            let _result = self.start_ping_check(now);
        }
    }

    /// Surfaces each reliable packet that has run out of retransmissions as a
    /// [`ServerEvent::ReliableGiveUp`], so a driver always learns *which*
    /// packet was lost — including the session-critical one whose loss the
    /// caller is about to close the session over.
    fn report_give_ups(&mut self, exhausted: &[ExhaustedPacket]) {
        for packet in exhausted {
            self.events.push_back(ServerEvent::ReliableGiveUp {
                message: packet.name.map(str::to_owned),
            });
        }
    }

    /// Withdraws an unanswered sit offer once its handshake timeout elapses.
    fn expire_sit_offer(&mut self, now: Instant) {
        let Some(at) = self.sit_expires else {
            return;
        };
        if now < at {
            return;
        }
        self.sit_expires = None;
        if let SimSitState::ResponseSent { on } = self.sit {
            self.sit = SimSitState::NotSitting;
            self.events.push_back(ServerEvent::SitOfferExpired { on });
        }
    }

    /// Answers and drops every parked `TransferRequest` whose serve deadline has
    /// elapsed, so a driver that never serves one does not leave the client
    /// waiting (and the request parked) forever.
    fn expire_transfers(&mut self, now: Instant) {
        let expired: Vec<TransferId> = self
            .transfer_serves
            .iter()
            .filter(|(_id, serve)| now >= serve.expires)
            .map(|(id, _serve)| *id)
            .collect();
        for transfer_id in expired {
            let Some(serve) = self.transfer_serves.remove(&transfer_id) else {
                continue;
            };
            let refuse = AnyMessage::TransferInfo(TransferInfo {
                transfer_info: TransferInfoTransferInfoBlock {
                    transfer_id: transfer_id.get(),
                    channel_type: TRANSFER_CHANNEL_ASSET,
                    target_type: 0,
                    status: TransferStatus::UnknownSource.to_code(),
                    size: 0,
                    params: serve.params,
                },
            });
            if let Err(error) = self.send(&refuse, Reliability::Reliable, now) {
                tracing::warn!(%error, "failed to refuse an unanswered transfer request");
            }
            self.events
                .push_back(ServerEvent::TransferServeExpired { transfer_id });
        }
    }

    /// Abandons every `Xfer` in either direction that has gone quiet past
    /// [`XFER_STALL_TIMEOUT`], telling the client with an `AbortXfer` — the
    /// mirror of the client's own stalled-transfer reaping.
    ///
    /// # Errors
    ///
    /// Returns a wire error if an `AbortXfer` fails to encode; the transfer is
    /// dropped either way.
    fn expire_xfers(&mut self, now: Instant) -> Result<(), WireError> {
        let expired: Vec<XferId> = self
            .xfer_sends
            .iter()
            .filter(|(_id, send)| now >= send.expires)
            .map(|(id, _send)| *id)
            .chain(
                self.xfer_receives
                    .iter()
                    .filter(|(_id, receive)| now >= receive.expires)
                    .map(|(id, _receive)| *id),
            )
            .collect();
        let mut result = Ok(());
        for xfer_id in expired {
            let _send = self.xfer_sends.remove(&xfer_id);
            let _receive = self.xfer_receives.remove(&xfer_id);
            let message = AnyMessage::AbortXfer(AbortXfer {
                xfer_id: AbortXferXferIDBlock {
                    id: xfer_id.get(),
                    result: XFER_TIMEOUT_RESULT,
                },
            });
            let sent = self.send(&message, Reliability::Reliable, now);
            if result.is_ok() {
                result = sent;
            }
            self.events.push_back(ServerEvent::XferAborted {
                xfer_id,
                result: XFER_TIMEOUT_RESULT,
            });
        }
        result
    }

    /// The next datagram to send to the client, if any.
    ///
    /// Popping a datagram starts the retransmission clock of the reliable packet
    /// it carries: until now that packet was only *queued*, and time spent in
    /// the queue must not count against its timeout.
    pub fn poll_transmit(&mut self) -> Option<Transmit> {
        let destination = self.client_addr?;
        let outbound = self.out.pop_front()?;
        if let Some(sequence) = outbound.sequence
            && let Some(packet) = self.unacked.get_mut(&sequence)
        {
            packet.queued = false;
        }
        Some(Transmit {
            destination,
            payload: outbound.payload,
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
        merge_deadline(&mut earliest, self.sit_expires);
        merge_deadline(
            &mut earliest,
            self.transfer_serves.values().map(|s| s.expires).min(),
        );
        merge_deadline(
            &mut earliest,
            self.xfer_sends
                .values()
                .map(|send| send.expires)
                .chain(self.xfer_receives.values().map(|receive| receive.expires))
                .min(),
        );
        earliest
    }

    /// The next server event, if any.
    pub fn poll_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Transitions to the closed state, emitting `reason` once, and frees every
    /// per-connection store the session was holding.
    ///
    /// The outbound queue is deliberately *not* dropped: a clean logout and a
    /// retired circuit both queue their goodbye packet and then close, and that
    /// datagram still has to reach the client. Nothing new can be queued behind
    /// it — [`SimSession::send`] refuses once closed — so the queue drains and
    /// stays drained.
    fn close(&mut self, reason: ServerEvent) {
        if matches!(self.state, SimState::Closed) {
            return;
        }
        self.state = SimState::Closed;
        self.ping = None;
        self.ack_flush = None;
        self.sit_expires = None;
        self.outstanding_ping = None;
        self.pending_acks = Vec::new();
        self.unacked = BTreeMap::new();
        self.caps_events = Vec::new();
        self.xfer_files = BTreeMap::new();
        self.xfer_sends = BTreeMap::new();
        self.xfer_receives = BTreeMap::new();
        self.transfer_serves = BTreeMap::new();
        self.chat_sessions = BTreeMap::new();
        self.script_questions = BTreeMap::new();
        self.script_grants = BTreeMap::new();
        self.offline_messages = Vec::new();
        self.pending_report_screenshot = None;
        self.pending_caps_uploads = BTreeMap::new();
        self.events.push_back(reason);
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
