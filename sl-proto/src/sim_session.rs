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

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_types::chat::ChatChannel;
use sl_types::key::{
    AgentKey, FriendKey, GroupKey, InventoryFolderKey, InventoryKey, ObjectKey, ParcelKey,
    TextureKey,
};
use sl_types::lsl::{Rotation, Vector};
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
    AnyMessage, CircuitCode, ControlFlags, EventQueueEvent, GlobalCoordinates, Llsd, MessageId,
    PacketFlags, Permissions, Permissions5, Reader, RegionHandle, RegionLocalObjectId,
    RegionLocalParcelId, SequenceNumber, WireError, Writer, build_event_queue_response,
    encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::appearance::{MAX_FACES, decode_texture_entry};
use crate::bookkeeping_ids::{PingId, TransactionId};
use crate::error::Error;
use crate::extra_params::decode_extra_param_blocks;
use crate::session::{
    agent_drop_group_to_llsd, agent_state_update_to_llsd, build_map_block_reply,
    build_map_item_reply, build_map_layer_reply, display_name_update_to_llsd, instant_message,
    nav_mesh_status_to_llsd, open_region_info_to_llsd, region_handshake_message,
    required_voice_version_to_llsd, set_display_name_reply_to_llsd, shape_from_object_shape_block,
    sim_console_response_to_llsd, windlight_refresh_to_llsd,
};
use crate::types::EventId;
use crate::types::directory::category_from_wire;
use crate::types::{
    AlertInfo, AttachmentMode, AttachmentPoint, AvatarName, AvatarPickerResult, Camera, ChatSource,
    ChatType, ClassifiedCategory, CoarseLocation, DetachOrder, DirClassifiedResult, DirEventResult,
    DirFindFlags, DirGroupResult, DirLandResult, DirPeopleResult, DirPlaceResult,
    DisplayNameUpdate, EstateCovenant, EventInfo, FeatureDisabled, FollowCamPropertyValue,
    GenericMessage, GenericStreamingMessage, GestureActivation, GroupAccountDetails,
    GroupAccountSummary, GroupAccountTransactions, GroupActiveProposalItem, GroupName,
    GroupVoteHistoryItem, InstantMessage, InventoryItemMove, Kick, LandSearchType, LandStatItem,
    LandStatReportType, MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags,
    MeanCollision, MovementMode, NavMeshStatus, NotecardRez, ObjectBuyItem, ObjectExtraParams,
    ObjectPlayingAnimation, ObjectPropertiesFamily, OpenRegionInfo, ParcelCategory, ParcelDetails,
    ParcelObjectOwner, PlacesResult, Postcard, PrimShapeParams, ProposalVoteId, RegionIdentity,
    RegionStats, Reliability, RequiredVoiceVersion, RestoreItem, RezAttachment, RezObjectParams,
    RezScriptParams, SaleType, ScriptControl, ScriptPermissions, ServerError, SetDisplayNameReply,
    SimulatorTime, TaskInventoryReply, TelehubInfo, TextureEntry, Throttle, Transmit, UserInfo,
    ViewerEffect, ViewerEffectData, ViewerEffectType,
};
use sl_wire::AbuseReport;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimState {
    /// Constructed; no circuit accepted yet (awaiting `UseCircuitCode`).
    AwaitingCircuit,
    /// The circuit is up: `UseCircuitCode` accepted, keep-alives flow. The agent
    /// completes its arrival once `CompleteAgentMovement` is answered.
    Active,
    /// The session is finished (the client logged out or the link timed out).
    Closed,
}

/// The decoded camera/control state carried by a client `AgentUpdate`, surfaced
/// as [`ServerEvent::AgentUpdate`]. The simulator uses this to move the agent
/// and to drive its interest list, mirroring what the client
/// [`Session`](crate::Session) sends.
#[derive(Debug, Clone, PartialEq)]
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

/// A server-side event decoded from a client-only message, the inverse of the
/// client's [`Command`](crate::Command)/[`Event`](crate::Event) split: it is
/// what the simulator observes a client doing.
///
/// Circuit-lifecycle messages are both acted on (the simulator answers them) and
/// surfaced here. Messages with a meaningful payload (`ChatFromViewer`,
/// `ImprovedInstantMessage`, `AgentUpdate`, `AgentThrottle`) are decoded into
/// typed variants. Every other decoded client message is surfaced verbatim as
/// [`ServerEvent::ClientMessage`].
#[derive(Debug, Clone, PartialEq)]
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
    /// The client sent an instant message (`ImprovedInstantMessage`).
    InstantMessage(Box<InstantMessage>),
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
    /// The client filed an abuse / bug report over the legacy `UserReport` UDP
    /// message (the modern path is the `SendUserReport` capability). The
    /// simulator routes it to the grid's abuse desk; fire-and-forget.
    AbuseReportReceived(Box<AbuseReport>),
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
    /// Pending events for the driver.
    events: VecDeque<ServerEvent>,
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
            events: VecDeque::new(),
        }
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
                self.events
                    .push_back(ServerEvent::AgentUpdate(Box::new(AgentUpdateInfo {
                        body_rotation: data.body_rotation.clone(),
                        head_rotation: data.head_rotation.clone(),
                        controls: ControlFlags::from_bits(data.control_flags),
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
            }
            AnyMessage::ChatFromViewer(chat) => {
                self.events.push_back(ServerEvent::Chat {
                    message: trimmed_string(&chat.chat_data.message),
                    channel: ChatChannel(chat.chat_data.channel),
                    chat_type: ChatType::from_u8(chat.chat_data.r#type),
                });
            }
            AnyMessage::ImprovedInstantMessage(im) => {
                self.events
                    .push_back(ServerEvent::InstantMessage(Box::new(instant_message(
                        &im.agent_data,
                        &im.message_block,
                    ))));
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
            AnyMessage::EstateOwnerMessage(message)
                if trimmed_string(&message.method_data.method) == "telehub" =>
            {
                if let Some(event) = telehub_server_event(&message.param_list) {
                    self.events.push_back(event);
                }
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
